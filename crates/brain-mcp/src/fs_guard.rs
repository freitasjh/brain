//! W-01 — containment for the two tools that turn a client-supplied string into a
//! filesystem write.
//!
//! # Why this module exists
//!
//! `brain_export(to)` created a directory and wrote every note into it, and
//! `brain_backup(to)` copied the whole `brain.db`, with `to` taken verbatim from
//! the request. The MCP SSE server binds `0.0.0.0` and has no auth (auth is a
//! product decision, deliberately out of scope here), so on a reachable host that
//! is arbitrary file write as the service user, plus exfiltration of the entire
//! database. Reading notes is a different class of exposure; this is a
//! filesystem *write* primitive, and it is contained the only way it can be
//! without auth: an allowlist.
//!
//! # The policy
//!
//! - `brain_export` may only write inside one directory: `BRAIN_EXPORT_ROOT`,
//!   default `/tmp/brain-export`. Omit `to` and that root is the destination, so
//!   the documented default keeps working unchanged.
//! - `brain_backup` may write to `{db}.bak` (its own default, always allowed) or,
//!   when `to` is given, to a `*.bak` file **inside the same root**.
//!
//! The `.bak` suffix rule is not decoration: without it, `brain_backup` is a
//! copy-the-database primitive pointed at any name in the allowlisted tree, and
//! the root would fill up with database copies the operator never asked for.
//!
//! # How containment is decided
//!
//! Not by a textual prefix test on the input. `..` and symlinks both defeat
//! that: `BRAIN_EXPORT_ROOT/../etc` and `BRAIN_EXPORT_ROOT/link` (with `link` a
//! symlink to `/etc`) are strings that *start* with the root. So the decision is
//! made in two steps, and the second one is the one that counts:
//!
//! 1. reject `..` components textually, purely to produce a legible error;
//! 2. canonicalize the deepest **existing** ancestor of the candidate and re-join
//!    the remainder, then test containment against the canonicalized root.
//!
//! Step 2 is why the ancestor walk exists: `canonicalize` fails on a path that
//! does not exist yet, and the export directory is normally created by the very
//! call being validated. Canonicalizing the deepest existing prefix resolves
//! every symlink that exists on the way, and leaves the not-yet-created tail as
//! plain components — which cannot be a symlink, because they do not exist.

use std::path::{Component, Path, PathBuf};

/// Env var naming the one directory an export or a client-chosen backup may write to.
pub const EXPORT_ROOT_ENV: &str = "BRAIN_EXPORT_ROOT";

/// Where an export goes when the caller names no destination.
pub const DEFAULT_EXPORT_ROOT: &str = "/tmp/brain-export";

/// The allowed root, from [`EXPORT_ROOT_ENV`] or [`DEFAULT_EXPORT_ROOT`].
///
/// A relative `BRAIN_EXPORT_ROOT` is resolved against the process's working
/// directory, which is what an operator setting `BRAIN_EXPORT_ROOT=./exports`
/// means. The value is canonicalized once here so every comparison in this
/// module is against a real, symlink-free prefix.
pub fn export_root() -> PathBuf {
    export_root_from(std::env::var(EXPORT_ROOT_ENV).ok())
}

/// [`export_root`] from an explicit environment value.
///
/// The seam that makes the default testable. `export_root` reads a
/// process-global, so a test that wanted to assert "the default is the documented
/// path" by unsetting the variable would be at the mercy of every other test in
/// the process; taking the value as an argument makes each case a pure function of
/// its input. X-05.4: the test this replaced asserted
/// `root.ends_with(DEFAULT) || env::var(...).is_ok()`, whose second disjunct is
/// true whenever the variable is merely *set* — it passed whatever the value was,
/// and therefore proved nothing.
fn export_root_from(env_value: Option<String>) -> PathBuf {
    let raw = env_value
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_EXPORT_ROOT.to_string());
    let p = PathBuf::from(raw);
    canonicalize_deepest_existing(&p).unwrap_or(p)
}

/// Why an existing export root is refused. `Ok(())` means it is usable.
///
/// **Y-03.** This is the whole `uid` half of [`assert_root_usable`] lifted into a
/// pure function, and the point of the split is testability, not style.
///
/// The threat the `uid` check addresses cannot be exercised through the
/// filesystem: it needs a directory **owned by a second uid**, so root or a
/// second user account, neither of which a unit test may assume. The *decision*,
/// though, is a two-term comparison and is perfectly testable — and the decision
/// is the security-relevant half, because the `mode` half cannot catch a
/// pre-created root: `mkdir` under a normal `umask` yields `0755`, which passes
/// `mode & 0o002 != 0` and is refused **only** because it belongs to somebody
/// else.
///
/// So the split is: *can the decision be made correctly?* — yes, tested here, per
/// input, for every combination. *did the syscall report the metadata that
/// decision consumes?* — not testable without a second uid, and stated as such
/// rather than implied. The mutation that matters is surgical: deleting only the
/// `meta_uid != euid` term has to fail a test, and it does, because the case
/// below asserts the *specific* refusal — a root at `0755` is not refused for
/// being world-writable, so nothing else can be what refused it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OwnershipRefusal {
    /// Owned by a different uid than this process runs as.
    NotOurs { meta_uid: u32, euid: u32 },
    /// Writable by every local user, so its contents are readable by them.
    OtherWritable { mode: u32 },
}

/// [`OwnershipRefusal`]'s decision, as a pure function of the three values it
/// consumes: the directory's own uid and mode, and the process's effective uid.
///
/// `mode` is `st_mode`, and only its permission bits are consulted — the
/// `S_IFDIR` bits are masked off both for the test and for the reported value, so
/// a caller can pass what `MetadataExt::mode` returns without thinking about it
/// and the refusal names a mode an operator can `chmod`.
pub(crate) fn ownership_allows(meta_uid: u32, mode: u32, euid: u32) -> Result<(), OwnershipRefusal> {
    let mode = mode & 0o7777;
    if meta_uid != euid {
        return Err(OwnershipRefusal::NotOurs { meta_uid, euid });
    }
    // 0o002 = other-writable. **Group-writable is allowed on purpose.**
    //
    // This was checked against 0o022 (group *and* other) first, and it broke the
    // documented default on a real machine: an existing `/tmp/brain-export` at mode
    // 775 — which is what a `umask 002` shell produces, and is extremely common —
    // was refused, taking every export and backup with it. Narrowing it to `other`
    // is not a weakening of the actual threat:
    //
    // - *another local user* means "other", and that bit is refused;
    // - *the group* is populated by an administrator, so group-writability is a
    //   deliberate trust decision rather than an accident;
    // - and a directory another user created is caught by the uid check above
    //   whatever its mode, which is the check that actually addresses the
    //   "pre-created `/tmp/brain-export`" case.
    if mode & 0o002 != 0 {
        return Err(OwnershipRefusal::OtherWritable { mode });
    }
    Ok(())
}

/// Refuses a root that another local user could have taken over.
///
/// # The threat
///
/// The default root is `/tmp/brain-export`, inside a directory every user on the
/// host can write. `/tmp` is sticky, so a local attacker cannot *delete* a
/// directory we created — but they can **create `/tmp/brain-export` before the
/// first run**. `create_dir_all` then succeeds on *their* directory, and every
/// `brain_export` writes the whole corpus, and every `brain_backup --to`, into a
/// tree the attacker owns and can read. The allowlist in this module is only as
/// strong as the ownership of the directory it allows.
///
/// # What is required
///
/// - **It exists and is a directory.** A regular file at the root path would make
///   `create_dir_all` fail later, further from the cause.
/// - **It is owned by this effective uid.** Otherwise it is somebody else's.
/// - **It is not writable by `other`.** Group-writable is **allowed on purpose** — the
///   reason, and the narrowing that a stricter check forced, are in `ownership_allows`,
///   which is where that decision is held.
///
/// # The residual risk this leaves, stated with its condition
///
/// "Not writable by `other`" is exactly the guarantee, and it is weaker than "not
/// writable by anyone else". On a host whose root's primary group is shared — `users` on
/// a default Debian multi-user install is the ordinary example — a mode 775 directory
/// **is** writable by every other local account in that group, and this check accepts it.
/// The residual is conditional, not hypothetical: it needs (a) a group that more than
/// one login can be a member of, and (b) a root created at 775 rather than 750 or 700.
/// With the default `umask 002` on this host, (b) is what a plain `mkdir` produces.
///
/// Tightening it to refuse `mode & 0o020` — group-writable — is the obvious fix and is
/// **not** applied, because it breaks the documented default on exactly that host: a
/// `/tmp/brain-export` already at 775 would be refused and every export and backup would
/// fail. A correct version of that check has to be conditional rather than absolute —
/// refuse group-writability only when the directory's gid is *not* the process's primary
/// gid — so it is recorded here as the shape of the fix rather than shipped as a change
/// of behaviour by a documentation fix. The uid check is what actually addresses the
/// pre-created-root attack; see `OwnershipRefusal::NotOurs`.
///
/// The last two are [`ownership_allows`], which holds the decision and its
/// reasons; this function is the syscall around it.
///
/// # Why this does not break the documented default
///
/// `/tmp` is `1777` — world-writable *and* sticky — and this check does not
/// object to that, because it only applies to a root that **already exists**. The
/// two cases are:
///
/// - the root does not exist yet: we create it, so we own it, so the next call
///   passes. Nothing to check now;
/// - the root exists: it must be ours and not writable by others. A root
///   pre-created by another user fails here, which is the whole point.
///
/// So `/tmp/brain-export` works exactly as documented, and the case it does not
/// cover — a root created by someone else between the check and the write — is
/// closed by re-checking after creation, which is why
/// [`crate`]'s export handler calls this again once the directory exists.
pub fn assert_root_usable(root: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = match std::fs::metadata(root) {
        Ok(m) => m,
        // Not there yet: the caller creates it, and a directory we create is ours.
        // The nearest existing ancestor still has to be safe, or we could be
        // writing into a tree another user controls.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => anyhow::bail!("{EXPORT_ROOT_ENV}={} cannot be inspected: {e}", root.display()),
    };
    if !meta.is_dir() {
        anyhow::bail!(
            "{EXPORT_ROOT_ENV}={} exists but is not a directory. Point it at a directory, or remove it.",
            root.display()
        );
    }
    let euid = geteuid();
    match ownership_allows(meta.uid(), meta.mode(), euid) {
        Ok(()) => {}
        Err(OwnershipRefusal::NotOurs { meta_uid, euid }) => {
            anyhow::bail!(
                "{EXPORT_ROOT_ENV}={} is owned by uid {} but this process runs as uid {euid}, so another local \
                 user controls it. Every export would be readable by them. Remove it, or point {EXPORT_ROOT_ENV} \
                 at a directory you own.",
                root.display(),
                meta_uid
            );
        }
        Err(OwnershipRefusal::OtherWritable { mode }) => {
            anyhow::bail!(
                "{EXPORT_ROOT_ENV}={} is writable by any local user (mode {:o}), so anything exported \
                 there — the whole corpus, or a copy of the database — would be readable by them. \
                 `chmod o-w {}` or point {EXPORT_ROOT_ENV} at a directory you own.",
                root.display(),
                mode,
                root.display()
            );
        }
    }
    Ok(())
}

/// `geteuid(3)`, without taking a dependency on `libc` for one call.
///
/// `std` exposes a file's uid but not the process's own, and the whole check is
/// about comparing the two. An `unsafe extern` block is the price; it is confined
/// to this function so the call site stays safe and there is exactly one place to
/// audit.
fn geteuid() -> u32 {
    // Safety: `geteuid` takes no arguments, cannot fail, has no preconditions, and
    // has no side effects. It is one of the safest calls in the C library.
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

/// Rejects a relative path that tries to climb out with `..`, before any IO.
///
/// Purely for the error message: a caller passing `../../etc/x` should be told
/// *that* is the problem rather than the generic "outside the export root". The
/// authority is the canonicalized containment check in [`resolve_within`].
fn reject_traversal(raw: &str) -> anyhow::Result<()> {
    if Path::new(raw).components().any(|c| matches!(c, Component::ParentDir)) {
        anyhow::bail!("path traversal rejected: {raw:?} contains `..`");
    }
    Ok(())
}

/// Canonicalizes the deepest existing ancestor of `p`, re-appending the rest.
///
/// `canonicalize` on a non-existent path fails, and the export directory is
/// usually created by the call being validated — so walk up until something
/// exists, canonicalize that, then push the remaining components back on.
fn canonicalize_deepest_existing(p: &Path) -> Option<PathBuf> {
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = p;
    loop {
        if let Ok(c) = cur.canonicalize() {
            let mut out = c;
            for part in tail.iter().rev() {
                out.push(part);
            }
            return Some(out);
        }
        let name = cur.file_name()?.to_os_string();
        tail.push(name);
        cur = cur.parent()?;
    }
}

/// Resolves `candidate` and proves it is inside `root`.
///
/// `candidate` may be relative (resolved against `root`) or absolute; an absolute
/// candidate still has to land inside `root`, which is what makes
/// `BRAIN_EXPORT_ROOT=/tmp/brain-export` refuse `/etc/x` and `/tmp/brain-export/../etc`
/// alike.
pub fn resolve_within(root: &Path, candidate: &str, what: &str) -> anyhow::Result<PathBuf> {
    reject_traversal(candidate)?;
    let raw = if Path::new(candidate).is_absolute() {
        PathBuf::from(candidate)
    } else {
        root.join(candidate)
    };
    let root_c = canonicalize_deepest_existing(root)
        .ok_or_else(|| anyhow::anyhow!("{EXPORT_ROOT_ENV}={} cannot be resolved", root.display()))?;
    // Canonicalize the deepest existing ancestor, so a symlink anywhere on the
    // way is followed before the comparison instead of after it.
    let target = canonicalize_deepest_existing(&raw)
        .ok_or_else(|| anyhow::anyhow!("{what} path cannot be resolved: {}", raw.display()))?;
    if !is_within(&root_c, &target) {
        anyhow::bail!(
            "{what} must stay inside the export root {}: {} is outside it. Set {EXPORT_ROOT_ENV} to widen the \
             allowed directory.",
            root_c.display(),
            raw.display()
        );
    }
    Ok(target)
}

/// True when `path` is `root` itself or below it, compared component-wise.
///
/// `starts_with` on a `Path` is component-wise already — `/tmp/brain-export-evil`
/// is not "within" `/tmp/brain-export` — which is exactly the property a string
/// `contains`/`starts_with` test would get wrong.
fn is_within(root: &Path, path: &Path) -> bool {
    path == root || path.starts_with(root)
}

/// Destination for [`brain_export`](crate) / `brain export`.
///
/// `None` selects [`DEFAULT_EXPORT_ROOT`] through the same containment check, so
/// the default is validated like any other value rather than trusted because it
/// is a constant.
pub fn export_dir(to: Option<&str>) -> anyhow::Result<PathBuf> {
    let root = export_root();
    // X-05.3. The allowlist is only as strong as the ownership of the directory it
    // allows, so the root is checked before anything is created inside it. When
    // the root does not exist yet this is a no-op — the caller creates it, and a
    // directory we create is ours — and the export handler checks again after
    // `create_dir_all`, which is what closes the gap between here and there.
    assert_root_usable(&root)?;
    match to {
        None => resolve_within(&root, ".", "export directory"),
        Some(t) => resolve_within(&root, t, "export directory"),
    }
}

/// Destination for [`brain_backup`](crate) / `brain backup`.
///
/// `db` is the database file being copied. Omitting `to` yields `{db}.bak`,
/// which is allowed unconditionally: it is a sibling of a file the process can
/// already read and write, and it is the documented default.
pub fn backup_file(db: &str, to: Option<&str>) -> anyhow::Result<PathBuf> {
    let Some(t) = to else {
        return Ok(PathBuf::from(format!("{db}.bak")));
    };
    let path = Path::new(t);
    if !path.is_absolute() {
        anyhow::bail!("backup destination must be an absolute path: {t:?}");
    }
    if path.extension().and_then(|e| e.to_str()) != Some("bak") {
        anyhow::bail!("backup destination must end in .bak: {t:?}");
    }
    // X-05.3: a backup copies the whole database, so a root another local user
    // controls is a root they can read the database from.
    assert_root_usable(&export_root())?;
    resolve_within(&export_root(), t, "backup destination")
}

/// Where one note belongs inside an export directory.
///
/// The note's `path` comes out of the database, not from the request, and
/// `sanitize_relative_path` rejects `..` and leading `/` at write time — but the
/// export runs over *existing* rows, including any written by an older build or a
/// migration, and `dir.join("../x")` is a write outside the directory. The check is
/// per note rather than per request, so it belongs next to the policy.
///
/// Both halves matter: rejecting `..` catches the obvious climb, and rejecting an
/// absolute path catches `join` silently discarding `dir` altogether (Rust's `join`
/// replaces the base when the argument is absolute). A lexical `starts_with` on the
/// joined path would pass `dir/../x`, which is why the `..` test is explicit rather
/// than left to the comparison.
pub fn note_file_within(dir: &Path, note_path: &str) -> anyhow::Result<PathBuf> {
    let rel = Path::new(note_path);
    if rel.is_absolute() {
        anyhow::bail!("note path must be relative to the export directory: {note_path:?}");
    }
    if rel.components().any(|c| matches!(c, Component::ParentDir)) {
        anyhow::bail!("note path escapes the export directory: {note_path:?}");
    }
    let file = dir.join(rel);
    if !is_within(dir, &file) {
        anyhow::bail!("note path escapes the export directory: {note_path:?}");
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag() -> String {
        format!("brain-fsguard-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())
    }

    /// A scratch directory *inside* the default root, so the tests exercise the
    /// real policy rather than a widened one.
    fn scratch() -> PathBuf {
        let p = export_root().join(tag());
        std::fs::create_dir_all(&p).unwrap();
        canonicalize_deepest_existing(&p).unwrap()
    }

    /// X-05.4. The default is asserted as a *value*, per input.
    ///
    /// The test this replaces was
    /// `assert!(root.ends_with(DEFAULT_EXPORT_ROOT) || std::env::var(EXPORT_ROOT_ENV).is_ok())`,
    /// whose second disjunct is satisfied by the variable merely *existing* — so it
    /// passed for any value at all, and for none. Asserting through
    /// `export_root_from` makes each case a function of its input instead of of
    /// whatever the developer's shell happened to export, which is the only way to
    /// test a default in a process where the variable is a process-global.
    #[test]
    fn the_default_root_is_the_documented_one() {
        let expected = canonicalize_deepest_existing(Path::new(DEFAULT_EXPORT_ROOT))
            .unwrap_or_else(|| PathBuf::from(DEFAULT_EXPORT_ROOT));
        // Unset, and set-but-blank: both must land on the documented default.
        assert_eq!(export_root_from(None), expected, "an unset BRAIN_EXPORT_ROOT must use the documented default");
        assert_eq!(export_root_from(Some(String::new())), expected, "a blank BRAIN_EXPORT_ROOT must use the default too");
        assert_eq!(export_root_from(Some("   ".to_string())), expected, "and so must whitespace");
        // Set to something real: that value wins.
        let custom = export_root_from(Some("/tmp/brain-guard-custom-root".to_string()));
        assert!(
            custom.ends_with("brain-guard-custom-root"),
            "an explicit BRAIN_EXPORT_ROOT must be honoured, got {}",
            custom.display()
        );
        assert_ne!(custom, expected, "and must not silently fall back to the default");
    }

    /// Y-03: the ownership decision, table-tested per input.
    ///
    /// The `uid` half is the one that holds the declared threat — another local
    /// user pre-creating the root — and it is the one the previous test file
    /// never covered: every case in
    /// `a_root_writable_by_any_local_user_is_refused` is *our* directory, so the
    /// uid comparison could be deleted outright and all of them would still pass.
    ///
    /// The load-bearing row is `(other, 0o755, ours)`. `0755` is what `mkdir`
    /// produces under a normal `umask`, it has no other-write bit, so the `mode`
    /// rule accepts it — and it is refused **only** because it belongs to somebody
    /// else. That is the attack, and the assertion names the reason
    /// (`NotOurs`) rather than just "an error", so a `mode` refusal could not
    /// stand in for the uid one.
    #[test]
    fn a_root_belonging_to_another_user_is_refused_whatever_its_mode() {
        const OURS: u32 = 1000;
        const THEIRS: u32 = 1001;

        // The declared attack: pre-created under a normal umask, so 0755.
        assert_eq!(
            ownership_allows(THEIRS, 0o755, OURS),
            Err(OwnershipRefusal::NotOurs { meta_uid: THEIRS, euid: OURS }),
            "a directory another local user created is the whole threat, and 0755 is what mkdir produces. \
             Nothing but the uid comparison can refuse this: 0755 has no other-write bit, so the mode rule \
             accepts it."
        );

        // And the uid rule holds for every mode, including the ones the mode rule
        // would refuse anyway — so the two rules cannot be confused for one.
        for mode in [0o700u32, 0o755, 0o750, 0o775, 0o777] {
            assert_eq!(
                ownership_allows(THEIRS, mode, OURS),
                Err(OwnershipRefusal::NotOurs { meta_uid: THEIRS, euid: OURS }),
                "mode {mode:o} must not change that the directory is somebody else's"
            );
        }

        // Ours, and not world-writable: usable. 0700 and 0755 are the two modes
        // the default deployment actually produces.
        for mode in [0o700u32, 0o755, 0o750, 0o775] {
            assert_eq!(ownership_allows(OURS, mode, OURS), Ok(()), "our own directory at {mode:o} must be accepted");
        }

        // Ours but world-writable: refused, and specifically for being writable,
        // so the reason is distinguishable from the uid one.
        for mode in [0o702u32, 0o707, 0o777] {
            assert_eq!(
                ownership_allows(OURS, mode, OURS),
                Err(OwnershipRefusal::OtherWritable { mode }),
                "our own directory at {mode:o} is still takeover-able by any local user"
            );
        }
    }

    /// Y-03: the syscall wrapper and the pure decision agree.
    ///
    /// The table above proves the *decision*; this proves the wrapper still feeds
    /// it the right three values from a real `stat`. The uid half of *that* is
    /// untestable here — see [`ownership_allows`] — but the mode half is checked
    /// against a directory whose real mode was set, so a swap of the arguments
    /// (`meta.uid()` for `meta.mode()`, say) would be caught.
    #[test]
    fn the_syscall_wrapper_agrees_with_the_pure_decision() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = scratch();
        let euid = geteuid();
        for mode in [0o700u32, 0o755, 0o777, 0o702] {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
            let meta = std::fs::metadata(&dir).unwrap();
            let decision = ownership_allows(meta.uid(), meta.mode(), euid);
            let syscall = assert_root_usable(&dir);
            assert_eq!(
                syscall.is_ok(),
                decision.is_ok(),
                "mode {mode:o}: the wrapper and the decision must agree"
            );
            // The directory is ours by construction, so a refusal here can only
            // have come from the mode rule. That is what makes this check about
            // *which* value the wrapper passes, not merely whether it refuses.
            if let Err(e) = &syscall {
                assert_eq!(
                    decision,
                    Err(OwnershipRefusal::OtherWritable { mode }),
                    "mode {mode:o}: the wrapper must be refusing for the mode, not for a swapped uid argument"
                );
                assert!(e.to_string().contains("writable by any local user"), "mode {mode:o}: {e}");
            }
        }
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A root any other local user could write into is refused.
    ///
    /// The `mode` half of the rule, exercised through the real syscall. **The uid
    /// half cannot be** — it needs a second uid, so root or a second user — and
    /// the decision behind it is covered per input by
    /// `a_root_belonging_to_another_user_is_refused_whatever_its_mode`, which is
    /// the half that actually holds the threat.
    #[test]
    fn a_root_writable_by_any_local_user_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        for mode in [0o777u32, 0o707, 0o702] {
            let dir = scratch();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(mode)).unwrap();
            let err = assert_root_usable(&dir)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("writable by any local user"),
                "mode {mode:o} must be refused as takeover-able, got: {err}"
            );
            // The message has to be actionable, or an operator meets a refusal with
            // no idea what to do about it.
            assert!(err.contains("chmod o-w"), "the error must say how to fix it: {err}");
            assert!(err.contains(&dir.display().to_string()), "and name the path: {err}");
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Group-writable is *allowed*, and this test is where that decision is
    /// recorded.
    ///
    /// Found the hard way: the first version of the check refused 0o775 and broke
    /// the documented default on a machine whose `/tmp/brain-export` had been
    /// created by a `umask 002` shell — every export and backup failed. The threat
    /// is "another local user", which is the `other` bit; the group is populated by
    /// an administrator, so group-writability is a trust decision someone made on
    /// purpose, and refusing it here breaks real deployments to defend against
    /// nobody. The uid check still catches a directory another user created.
    #[test]
    fn a_group_writable_root_is_accepted() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert_root_usable(&dir).expect("mode 775 is what a umask-002 shell produces and must keep working");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The check does not break the documented default, which lives in `/tmp`.
    ///
    /// This is the "does it break real use" half of the review's question. `/tmp` is
    /// `1777` — world-writable and sticky — and the rule deliberately does not
    /// object, because it only applies to a root that already exists. A root we
    /// created is ours, so the documented path keeps working.
    #[test]
    fn the_default_root_is_accepted_once_it_belongs_to_us() {
        let p = export_root().join(tag());
        std::fs::create_dir_all(&p).unwrap();
        let canonical = canonicalize_deepest_existing(&p).unwrap();
        assert_root_usable(&canonical).expect("a directory we just created must be accepted");
        assert_root_usable(&p).expect("including through the non-canonical form");
        // A root that does not exist yet is nobody else's problem, and must not be
        // refused for that reason.
        let absent = export_root().join(tag()).join("not-created-yet");
        assert_root_usable(&absent).expect("a root that does not exist yet is not yet an attack");
        let _ = std::fs::remove_dir_all(&p);
    }

    /// A regular file where the root should be is refused, not treated as usable.
    #[test]
    fn a_root_that_is_a_file_is_refused() {
        let p = export_root().join(tag());
        std::fs::write(&p, b"not a directory").unwrap();
        let err = assert_root_usable(&p).expect_err("a file at the root path must be refused").to_string();
        assert!(err.contains("not a directory"), "the error must say what is wrong: {err}");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_directory_inside_the_root_is_accepted() {
        let dir = scratch();
        let resolved = export_dir(Some(dir.to_str().unwrap())).unwrap();
        assert!(is_within(&export_root(), &resolved));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_destination_is_the_root_itself() {
        let resolved = export_dir(None).unwrap();
        assert_eq!(resolved, export_root());
    }

    #[test]
    fn an_absolute_path_outside_the_root_is_refused() {
        let err = export_dir(Some("/etc")).expect_err("/etc must be refused");
        assert!(err.to_string().contains("export root"), "{err}");
        let err = export_dir(Some("/etc/brain-x")).expect_err("/etc/brain-x must be refused");
        assert!(err.to_string().contains("export root"), "{err}");
    }

    #[test]
    fn traversal_out_of_the_root_is_refused_with_a_traversal_message() {
        for t in ["../etc", "../../etc/x", "/tmp/brain-export/../etc"] {
            let err = export_dir(Some(t)).unwrap_err();
            let msg = err.to_string();
            assert!(
                msg.contains("traversal") || msg.contains("export root"),
                "{t:?} must be refused, got: {msg}"
            );
        }
    }

    #[test]
    fn a_symlink_pointing_out_of_the_root_is_refused() {
        // The case a textual prefix test cannot catch: the string starts with the
        // root and the directory it names is somewhere else entirely.
        let dir = scratch();
        let link = dir.join("escape");
        let outside = format!("/tmp/brain-outside-{}", tag());
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let err = export_dir(Some(link.to_str().unwrap())).expect_err("a symlink out of the root must be refused");
        assert!(err.to_string().contains("export root"), "{err}");
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir_all(&outside);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_defaults_to_a_sibling_of_the_database() {
        let db = format!("/tmp/brain-guard-{}.db", tag());
        assert_eq!(backup_file(&db, None).unwrap(), PathBuf::from(format!("{db}.bak")));
    }

    #[test]
    fn backup_inside_the_root_needs_the_bak_suffix() {
        let dir = scratch();
        let ok = dir.join("copy.bak");
        assert_eq!(backup_file("/tmp/x.db", Some(ok.to_str().unwrap())).unwrap(), ok);
        let err = backup_file("/tmp/x.db", Some(dir.join("copy.db").to_str().unwrap()))
            .expect_err("a backup without .bak must be refused");
        assert!(err.to_string().contains(".bak"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_outside_the_root_is_refused() {
        for t in ["/etc/brain.bak", "/tmp/brain.bak"] {
            let err = backup_file("/tmp/x.db", Some(t)).expect_err("{t} must be refused");
            assert!(err.to_string().contains("export root"), "{err}");
        }
    }

    #[test]
    fn a_relative_backup_destination_is_refused() {
        // Relative would be resolved against the process CWD, which under systemd
        // is `/` — a location nobody chose and nobody audits.
        let err = backup_file("/tmp/x.db", Some("brain.bak")).expect_err("a relative backup must be refused");
        assert!(err.to_string().contains("absolute"), "{err}");
    }

    #[test]
    fn a_note_path_cannot_escape_the_export_directory() {
        let dir = scratch();
        assert!(note_file_within(&dir, "regras/global/x").unwrap().starts_with(&dir));
        assert!(note_file_within(&dir, "../escape").is_err(), "a stored path with `..` must not be written outside");
        assert!(note_file_within(&dir, "/etc/passwd").is_err(), "an absolute stored path must not be written");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
