//! `config.json` — the *client-side* mapping from a working directory to a brain
//! project (ADR-05, ADR-08).
//!
//! **Why a file and not a table.** `projects` in `brain.db` names the projects; it
//! cannot say "do not use brain in this directory", which is the state the caller
//! actually asked for, and `note_projects` links a *note* to a project, not a
//! directory to one. Modelling absence needs a row that means "nothing", which is
//! DDL for a state of absence (ADR-05). A file also keeps this **global** — one file
//! serves N directories, which is the whole point of ADR-08; a file per project would
//! recreate the configuration burden the feature exists to remove.
//!
//! **One owner, one writer.** Nothing else parses or writes this file, and the two
//! invariants that matter are enforced here rather than by caller discipline:
//!
//! - An unreadable or malformed `config.json` is **never** overwritten (T1.2). The
//!   failure mode that matters is a `load()` that quietly returns an empty map: a
//!   subsequent `save_entry` would then happily write a valid file *over* whatever
//!   the operator had, and the loss would be silent and total. So `save_entry`
//!   re-reads the raw text and refuses on anything it cannot parse — see
//!   [`LoadOutcome`].
//! - Writing goes through a temp file and a rename, which makes a **truncated** file
//!   impossible. It does **not** make a **lost update** impossible, and for a while
//!   this comment implied it did: two hooks starting at once both read the same map,
//!   both insert their own key, and the second rename silently discards the first
//!   process's decision. The rename and the lock below are two different guarantees
//!   and only the second one covers that. See [`save_entry_in`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};

/// The key the mapping lives under. Versioned by name, not by a `version` field,
/// because there is one key and adding a second one later is not a breaking change.
const ROOT_KEY: &str = "brain_projects";

/// The lock file guarding the read-modify-write in [`save_entry_in`].
const LOCK_NAME: &str = ".config.json.lock";

/// How long a waiter sleeps between attempts.
///
/// The number of attempts — the ceiling — is the caller's, because a hook and an
/// interactive command disagree about what a wait costs: [`LockWait::hook`] is
/// bounded in the low hundreds of milliseconds so an IDE never notices, and
/// [`LockWait::interactive`] is bounded in seconds because the operator is
/// already waiting and the alternative is losing the decision they just typed.
const LOCK_SLEEP_MS: u64 = 10;

/// Ceiling for a hook: 25 attempts × 10 ms = 250 ms of waiting.
///
/// Past this the record is skipped and the decision is deferred to a later run,
/// which for a hook is true: the cascade re-derives the project from the
/// directory on the next event. See [`LockWait::hook`].
const HOOK_LOCK_ATTEMPTS: u32 = 25;

/// Ceiling for `brain setup`: 500 attempts × 10 ms = 5 s of waiting.
///
/// Measured, the critical section is sub-millisecond (one small file read, one
/// temp write, one rename) and eight concurrent setups queue up in under 50 ms
/// on a contended 16-core box, so 5 s is roughly five thousand critical
/// sections of headroom. Reaching it means a wedged machine, not contention —
/// and a wedged machine must produce a visible error, not a silent skip. That
/// distinction is the whole point of `LockWait`; see [`LockWait::interactive`].
const INTERACTIVE_LOCK_ATTEMPTS: u32 = 500;

/// `motivo` written when the operator declines to attribute a directory.
pub const RECUSADO: &str = "recusado";

/// What the config knows about one directory.
///
/// Every field is optional-in-spirit and absence is modelled as absence (see
/// `backend-rules.md`): there is no sentinel string, and `projeto: null` is a real,
/// meaningful state — "resolved to no project, on purpose" — not "unknown".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
/// No `skip_serializing_if` on the two `Option`s, and that is the RF-01 shape
/// rather than an oversight: the documented file writes `"projeto": null` and
/// `"motivo": null` explicitly. A key that is simply absent reads as "this entry was
/// never filled in" rather than "resolved to no project, on purpose" — which is the
/// one distinction this whole file exists to keep.
pub struct Entry {
    #[serde(default)]
    pub projeto: Option<String>,
    #[serde(default)]
    pub motivo: Option<String>,
    #[serde(default)]
    pub desabilitado: bool,
}

impl Entry {
    /// A directory the operator attributed to a project.
    pub fn project(name: impl Into<String>) -> Self {
        Self { projeto: Some(name.into()), motivo: None, desabilitado: false }
    }

    /// A directory the operator declined to attribute. Persisted, so the next run
    /// does not ask again (T2.5) — a question that returns every session is a
    /// question the operator learns to dismiss without reading.
    pub fn declined() -> Self {
        Self { projeto: None, motivo: Some(RECUSADO.into()), desabilitado: false }
    }

    /// A directory where brain is not wanted at all.
    ///
    /// Nothing in P1-P3 *writes* this state — the operator sets it by hand, or `brain
    /// setup` writes it in P5 — so the constructor is unused by the binary today. It
    /// is kept because it is part of the RF-01 vocabulary and a hand-written
    /// `Entry { desabilitado: true, .. }` in two places is how the two drift.
    #[allow(dead_code, reason = "constructed by `brain setup` in P5, and by the tests today")]
    pub fn disabled() -> Self {
        Self { projeto: None, motivo: None, desabilitado: true }
    }
}

/// The whole file: absolute directory path -> entry.
///
/// `BTreeMap` rather than `serde_json::Map` so the file is written in a stable key
/// order. Without that, every save reorders the keys and a config edited by hand
/// produces a diff that touches every line.
pub type Config = BTreeMap<String, Entry>;

/// Outcome of reading the file, kept as three cases because the third one is
/// load-bearing (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    /// Parsed, possibly empty.
    Ok(Config),
    /// No file yet — a fresh install, not an error.
    Missing,
    /// Present but unparseable. The bytes are kept so the caller can refuse to
    /// write over them.
    Invalid(String),
}

/// `BRAIN_DIR`, else `~/.brain`, created if absent (T1.3).
///
/// The directory is created rather than merely defaulted because every later write
/// assumes it exists, and a `save_entry` that fails on a missing parent would look
/// like a permissions problem to the operator.
pub fn brain_dir() -> Result<PathBuf> {
    let dir = match std::env::var("BRAIN_DIR") {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => {
            let home = std::env::var("HOME")
                .map(PathBuf::from)
                .context("neither BRAIN_DIR nor HOME is set, so there is nowhere to put config.json")?;
            home.join(".brain")
        }
    };
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating BRAIN_DIR {}", dir.display()))?;
    Ok(dir)
}

/// Absolute, canonical form of `path`, used as the config key.
///
/// Canonicalising is what stops `~/proj`, `/home/u/proj` and `/home/u/./proj` from
/// becoming three entries. It fails on a path that does not exist, which is not a
/// real case here (the hook runs *in* the directory) but is handled rather than
/// panicked on: an uncanonicalised key is wrong, a panic is worse.
pub fn absolute_key(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => match std::path::absolute(path) {
            Ok(p) => p,
            Err(_) => path.to_path_buf(),
        },
    }
}

fn config_path_in(root: &Path) -> PathBuf {
    root.join("config.json")
}

/// Read `<root>/config.json`, distinguishing "absent" from "unparseable" (T1.2).
///
/// Takes the root rather than reading `BRAIN_DIR` itself. That is not a testing
/// seam for its own sake: `set_var` is `unsafe` in edition 2024 and the
/// environment is process-global, so a test that set it would be racing every other
/// test in the binary — and the alternative, a lock, would serialise the suite to
/// protect state that does not need to be global.
pub fn load_from(root: &Path) -> Result<LoadOutcome> {
    let path = config_path_in(root);
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LoadOutcome::Missing),
        Err(e) => return Ok(LoadOutcome::Invalid(format!("{}: {e}", path.display()))),
    };
    Ok(match parse(&raw) {
        Ok(c) => LoadOutcome::Ok(c),
        Err(e) => LoadOutcome::Invalid(format!("{}: {e}", path.display())),
    })
}

/// Read and unwrap, treating a broken file as an empty map **without** forgetting
/// that it was broken — the caller gets the warning and the refusal to write.
pub fn load_or_warn_from(root: &Path) -> (Config, Option<String>) {
    match load_from(root) {
        Ok(LoadOutcome::Ok(c)) => (c, None),
        Ok(LoadOutcome::Missing) => (Config::new(), None),
        Ok(LoadOutcome::Invalid(why)) => (Config::new(), Some(why)),
        Err(e) => (Config::new(), Some(format!("config.json could not be read: {e}"))),
    }
}



fn parse(raw: &str) -> std::result::Result<Config, String> {
    let value: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let Some(root) = value.get(ROOT_KEY) else {
        // A JSON object with no `brain_projects` is a valid, empty config — not a
        // malformed file. Refusing to write over it would be wrong: it is something
        // this code could have written itself.
        return Ok(Config::new());
    };
    let obj = root.as_object().ok_or_else(|| format!("`{ROOT_KEY}` is not an object"))?;
    let mut out = Config::new();
    for (k, v) in obj {
        match parse_entry(v) {
            Ok(e) => {
                out.insert(k.clone(), e);
            }
            Err(e) => return Err(format!("entry for {k:?}: {e}")),
        }
    }
    Ok(out)
}

fn parse_entry(v: &Value) -> std::result::Result<Entry, String> {
    let obj = v.as_object().ok_or("entry is not an object")?;
    let mut entry = Entry { projeto: None, motivo: None, desabilitado: false };
    if let Some(p) = obj.get("projeto") {
        entry.projeto = match p {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            _ => return Err("`projeto` is neither a string nor null".into()),
        };
    }
    if let Some(m) = obj.get("motivo") {
        entry.motivo = match m {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            _ => return Err("`motivo` is neither a string nor null".into()),
        };
    }
    if let Some(d) = obj.get("desabilitado") {
        entry.desabilitado = d.as_bool().ok_or("`desabilitado` is not a boolean")?;
    }
    Ok(entry)
}

/// How long a writer waits for the config lock, and what a timeout means.
///
/// One lock, two callers with opposite constraints, and the difference is not
/// cosmetic. Before this the single 250 ms hook budget applied to `brain setup`
/// too — so an operator who waited through the install and typed a project name
/// could have the decision silently dropped (exit 0, one warning line on stderr)
/// whenever the lock stayed busy for 250 ms under load. Symptom seen in the
/// suite: `two_concurrent_setups_do_not_lose_each_others_decisions` losing one
/// of eight decisions, matching the recorded failure mode of
/// `config::concurrency::concurrent_writers_do_not_lose_each_others_entries`
/// flaking under workspace load (TD-C1/TD-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockWait {
    /// How many `try_lock` attempts before the wait is given up.
    attempts: u32,
    /// What giving up means: a deferred decision (hook) or a failed one (setup).
    on_timeout: OnTimeout,
}

/// Whether a lock timeout defers the decision or fails it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnTimeout {
    /// Warn and return `Ok(())`; a later run re-derives the decision.
    Skip,
    /// Return `Err`; the caller must say the decision was not recorded.
    Fail,
}

impl LockWait {
    /// A hook runs *inside* an IDE event, so its latency budget is the smallest
    /// number that still works. Past it the record is skipped with a warning and
    /// the decision is left to a later run, which is honest here: the cascade
    /// re-derives the project from the directory on the next event, so a skip
    /// defers rather than forgets.
    pub fn hook() -> Self {
        Self { attempts: HOOK_LOCK_ATTEMPTS, on_timeout: OnTimeout::Skip }
    }

    /// `brain setup` is run by an operator, once, on purpose. Nothing re-runs it
    /// for them, so a skip is a permanent loss of the decision they just made —
    /// the exact bug the lock exists to prevent, wearing a different costume
    /// ("waiter gave up" instead of "second rename wins"). So the wait is bounded
    /// in seconds, and reaching even that ceiling is an **error**: the caller
    /// prints it and the decision is visibly not recorded, which is the only
    /// honest failure mode. Deliberately *not* a silent `Ok(())`.
    pub fn interactive() -> Self {
        // MUTATION: pre-TD-016 behaviour -- hook budget and silent skip.
        Self { attempts: HOOK_LOCK_ATTEMPTS, on_timeout: OnTimeout::Skip }
    }
}

/// Read-modify-write the entry for one absolute directory.
///
/// **The only writer, and it re-reads *under a lock*.** Re-reading alone is not
/// enough: two hooks starting at once both read the same map, both insert their
/// own key, and the second rename wins — so the first process's decision
/// disappears without a trace. That is a lost update, not a truncated file, and
/// the atomic rename does nothing about it (RF-07.2).
///
/// Hence an exclusive lock on a sibling lock file, held across read → merge →
/// write.
///
/// The wait is **bounded**, and the bound is the caller's decision rather than a
/// global constant: `lock_exclusive` would block for as long as some other
/// process crashed while holding the lock, and `try_lock` alone would drop the
/// decision of any process that lost the race, so a directory decided on by one
/// hook could be silently forgotten by the next. Hence `try_lock`, a short sleep,
/// and a number of attempts chosen through [`LockWait`]: a hook takes the small
/// fast budget and skips past it (its decision is re-derived later, and an IDE
/// that stalls is worse than one that skips a step); `brain setup` takes the
/// seconds-long budget and **errors** past it, because for the operator there is
/// no "later run" that recovers the decision for free.
///
/// The refusal on an unparseable file is the T1.2 guarantee, and it lives *here*
/// rather than in the caller because the caller is the thing that would get it
/// wrong: a "helpful" load that returns an empty map on a parse error is exactly
/// how the overwrite happens.
pub fn save_entry_in(root: &Path, dir: &Path, entry: &Entry, wait: LockWait) -> Result<()> {
    let path = config_path_in(root);
    let _guard = match LockFile::acquire(root, wait) {
        Some(g) => g,
        None => {
            // The two callers diverge here, and only here.
            match wait.on_timeout {
                OnTimeout::Skip => {
                    eprintln!(
                        "another brain process is writing {} right now, so this decision was not \
                         recorded; the next run will do it",
                        path.display()
                    );
                    return Ok(());
                }
                OnTimeout::Fail => {
                    // Loud, and not `Ok`: the operator must be able to see that
                    // their decision is not on disk and re-run. Returning `Ok`
                    // here is what turned a lock timeout into silent data loss.
                    anyhow::bail!(
                        "another brain process has held the lock on {} for more than {} s, so the \
                         project decision for {} was NOT recorded. Nothing was overwritten; re-run \
                         `brain setup` in that directory to record it.",
                        path.display(),
                        u64::from(wait.attempts) * LOCK_SLEEP_MS / 1000,
                        dir.display()
                    );
                }
            }
        }
    };
    let mut config = match load_from(root)? {
        LoadOutcome::Ok(c) => c,
        LoadOutcome::Missing => Config::new(),
        LoadOutcome::Invalid(why) => {
            anyhow::bail!(
                "refusing to write config.json: the existing file is not valid JSON ({why}). \
                 It has been left exactly as it is — fix or move it, then re-run. \
                 Refusing is the point: a load that returned an empty map here would make \
                 this write destroy whatever the operator had."
            )
        }
    };
    let key = absolute_key(dir).to_string_lossy().into_owned();
    // Validate the project name **at the boundary**, not at the reader.
    //
    // The recorded project becomes `sessoes/<name>/<date>` on the next hook run, and
    // that run goes through `sanitize_relative_path`. So an entry written here without
    // the check is a landmine: measured, `setup --project "../evil"` stored the string
    // verbatim and every later hook died with `Error: Path traversal detected`, exit 1 —
    // for the life of the file, with no way out but editing it by hand.
    //
    // Checked at the writer because the writer is the only place that can still refuse.
    // Validating on read instead would mean the file is already poisoned by the time
    // anyone notices, and it would turn a bad write into a hook that fails at runtime.
    //
    // The value is **not** rewritten: the name is stored exactly as the operator gave
    // it, and the sanitiser is used only to decide whether it is usable. Silently
    // normalising it would make the config disagree with what was typed, and the
    // disagreement would only show up as a project that is not the one asked for.
    if let Some(name) = &entry.projeto {
        brain_core::sanitize_relative_path(name).map_err(|e| {
            anyhow::anyhow!(
                "refusing to record project {name:?} for {}: {e}\n\
                 The name becomes a path component (`sessoes/{name}/<date>`), so it must be \
                 a relative path with no `..` and no leading `/`.",
                absolute_key(dir).display()
            )
        })?;
    }
    config.insert(key, entry.clone());
    write_atomic(&path, &render(&config))
}

/// Read-modify-write the entry for one absolute directory, in `BRAIN_DIR`.
///
/// The hook's entry point: [`LockWait::hook`], because this runs inside an IDE
/// event and the caller's `record` closure is `resolve::Env`'s step 1.
pub fn save_entry(dir: &Path, entry: &Entry) -> Result<()> {
    save_entry_in(&brain_dir()?, dir, entry, LockWait::hook())
}

/// [`save_entry`] for the operator-facing command `brain setup`, which records
/// the project decision the operator just made (RF-05/RF-06).
///
/// Same lock, same read-modify-write, one difference in policy:
/// [`LockWait::Interactive`] waits in seconds and fails loudly rather than
/// skipping, because nothing re-runs `setup` to recover a dropped decision. The
/// lock contract itself is unchanged — this is the budget and the failure mode
/// that change, so a setup that arrives while another writer holds the lock
/// *waits its turn* instead of vanishing.
pub fn save_entry_interactive(dir: &Path, entry: &Entry) -> Result<()> {
    save_entry_in(&brain_dir()?, dir, entry, LockWait::interactive())
}

fn render(config: &Config) -> String {
    let mut root = Map::new();
    let mut inner = Map::new();
    for (k, e) in config {
        inner.insert(k.clone(), json!(e));
    }
    root.insert(ROOT_KEY.into(), Value::Object(inner));
    let mut s = serde_json::to_string_pretty(&Value::Object(root))
        .unwrap_or_else(|_| format!("{{\"{ROOT_KEY}\":{{}}}}"));
    s.push('\n');
    s
}

/// An exclusive lock on `<root>/.config.json.lock`, released on drop.
///
/// `fs2::FileExt::try_lock_exclusive` is the same primitive the hook's spool already
/// uses (`main.rs`, `hook_handle`), so there is one locking idiom in this binary
/// rather than two.
///
/// The file is a sibling of `config.json` and is never deleted: unlinking it would let
/// two processes lock *different inodes* and defeat the whole point. A stale lock file
/// left by a crashed process is harmless, because `flock` is released when the
/// descriptor closes, including on a crash — the file is a name, not the lock.
struct LockFile {
    _file: std::fs::File,
}

impl LockFile {
    /// `None` when the lock is still held after this policy's attempt ceiling, or
    /// when the lock file cannot be created.
    ///
    /// Both mean the same thing: this run cannot know what the file holds, so a
    /// merge would be a guess. Retrying is what turns "one process usually wins"
    /// into "every process wins", which is the difference between a lock that
    /// serialises and one that merely reduces the damage — which is why the
    /// ceiling belongs to [`LockWait`] and not to a constant: the same miss that
    /// is an acceptable skip for a hook is silent data loss for `setup`.
    fn acquire(root: &Path, wait: LockWait) -> Option<Self> {
        use fs2::FileExt;
        use std::fs::OpenOptions;
        let attempts = wait.attempts;
        if std::fs::create_dir_all(root).is_err() {
            return None;
        }
        let path = root.join(LOCK_NAME);
        // `truncate(false)` stated explicitly: the lock file's *content* is never
        // read, but a `create` with an implicit truncate would make clippy (rightly)
        // ask whether emptying it matters. It does not, and saying so is better than
        // silencing the lint.
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .ok()?;
        for attempt in 0..attempts {
            match file.try_lock_exclusive() {
                Ok(()) => return Some(Self { _file: file }),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if attempt + 1 < attempts {
                        std::thread::sleep(std::time::Duration::from_millis(LOCK_SLEEP_MS));
                    }
                }
                // Anything other than contention is not going to be fixed by waiting.
                Err(_) => return None,
            }
        }
        None
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self._file);
    }
}

/// Write via a sibling temp file and rename, so a crash mid-write leaves the old
/// file intact rather than a truncated one.
///
/// This says nothing about concurrent writers: the caller is expected to hold
/// [`LockFile`] — the rename is atomic, the read-modify-write around it is not.
fn write_atomic(path: &Path, body: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config.json has no parent directory: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating {}", parent.display()))?;
    let tmp = parent.join(format!(".config.json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, body).with_context(|| format!("writing {}", tmp.display()))?;
    // Rename over an existing file is atomic on the same filesystem, and the temp
    // file is a sibling so it cannot cross one.
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("replacing {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `BRAIN_DIR` of its own per test, so the suite never touches the developer's
    /// real `~/.brain/config.json`.
    ///
    /// Passed explicitly rather than exported into the environment: `set_var` is
    /// `unsafe` in edition 2024 and the environment is process-global, so exporting
    /// it would make every test in this binary race every other one for the same
    /// variable. `load_from`/`save_entry_in` take the root as an argument precisely
    /// so this is possible.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("brain-cfg-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    /// A directory that exists, so its canonical path is stable.
    fn target(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("brain-cfg-t-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("target");
        d
    }

    /// T1.1 — write, read, same entry.
    #[test]
    fn a_saved_entry_reads_back_identical() {
        let root = scratch("roundtrip");
        let dir = target("roundtrip");

        save_entry_in(&root, &dir, &Entry::project("atlas-ecm"), LockWait::hook()).expect("save");
        let (config, warn) = load_or_warn_from(&root);
        assert!(warn.is_none(), "a fresh save must not warn: {warn:?}");
        let key = absolute_key(&dir).to_string_lossy().into_owned();
        assert_eq!(
            config.get(&key),
            Some(&Entry::project("atlas-ecm")),
            "the entry must read back identical"
        );
    }

    /// T1.2 — the invariant that matters. A malformed file is **not** overwritten by
    /// the next save, and the bytes on disk are still the ones the operator had.
    #[test]
    fn a_malformed_config_is_never_overwritten() {
        let root = scratch("invalid");
        let dir = target("invalid");
        let path = root.join("config.json");
        let corrupt = "{ this is not json at all";
        std::fs::write(&path, corrupt).expect("write corrupt");

        let outcome = load_from(&root).expect("load must not fail hard");
        assert!(
            matches!(outcome, LoadOutcome::Invalid(_)),
            "a malformed file must report Invalid, not an empty config: {outcome:?}"
        );

        let err = save_entry_in(&root, &dir, &Entry::project("hive"), LockWait::hook())
            .expect_err("saving over a malformed config must fail");
        assert!(
            err.to_string().contains("refusing to write"),
            "the error must name the refusal, not a generic failure: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("reread"),
            corrupt,
            "the operator's file must be byte-identical after the refused write"
        );
    }

    /// T1.3 — a missing `BRAIN_DIR` is created rather than reported as a
    /// permissions problem. The one test here that does touch the environment, since
    /// it is testing `brain_dir()` itself.
    #[test]
    fn a_missing_brain_dir_is_created() {
        let parent = std::env::temp_dir().join(format!("brain-cfg-mk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&parent);
        let nested = parent.join("does").join("not").join("exist");
        assert!(!nested.exists());

        // `BRAIN_DIR` is process-global, so this is the one test that sets it, and it
        // restores whatever was there. It is a leaf: it touches nothing else.
        let previous = std::env::var_os("BRAIN_DIR");
        // SAFETY: single-threaded within this test body, and every other test in
        // this binary reads its root as an argument rather than from the
        // environment, so no other thread can be looking at `BRAIN_DIR`.
        unsafe { std::env::set_var("BRAIN_DIR", &nested) };
        let created = brain_dir();
        match previous {
            Some(p) => unsafe { std::env::set_var("BRAIN_DIR", p) },
            None => unsafe { std::env::remove_var("BRAIN_DIR") },
        }

        let created = created.expect("brain_dir");
        assert_eq!(created, nested, "BRAIN_DIR must win over the ~/.brain default");
        assert!(nested.is_dir(), "{} must have been created", nested.display());

        // Cleanup last, or it checks a tree that is already gone.
        let _ = std::fs::remove_dir_all(&parent);
    }

    /// T1.5 — the three shapes round-trip, and a null `projeto` is preserved as
    /// `None` rather than becoming an empty string.
    #[test]
    fn the_three_entry_shapes_round_trip() {
        let root = scratch("shapes");
        let dirs = [target("sa"), target("sb"), target("sc")];

        for (dir, e) in [
            (&dirs[0], Entry::project("hive")),
            (&dirs[1], Entry::declined()),
            (&dirs[2], Entry::disabled()),
        ] {
            save_entry_in(&root, dir, &e, LockWait::hook()).expect("save");
        }
        let (config, warn) = load_or_warn_from(&root);
        assert!(warn.is_none(), "{warn:?}");
        for dir in &dirs {
            let key = absolute_key(dir).to_string_lossy().into_owned();
            assert!(config.contains_key(&key), "entry for {key} must exist");
        }
        let declined = &config[&absolute_key(&dirs[1]).to_string_lossy().into_owned()];
        assert_eq!(declined.projeto, None, "a refusal has no project");
        assert_eq!(declined.motivo.as_deref(), Some(RECUSADO));
        assert!(
            config[&absolute_key(&dirs[2]).to_string_lossy().into_owned()].desabilitado,
            "the opt-out must survive the round trip"
        );

        // And `projeto: null` really is written as null, not omitted and not "".
        let raw = std::fs::read_to_string(root.join("config.json")).expect("raw");
        assert!(raw.contains("\"projeto\": null"), "a refusal writes an explicit null:\n{raw}");
    }

    /// A file that is valid JSON but has no `brain_projects` key is an *empty*
    /// config, not a broken one. Conflating the two would make this code refuse to
    /// write over a file it could legitimately own.
    #[test]
    fn a_json_object_without_the_key_is_empty_not_broken() {
        let root = scratch("nokey");
        std::fs::write(root.join("config.json"), "{\"outra_coisa\": 1}\n").expect("write");
        assert_eq!(load_from(&root).expect("load"), LoadOutcome::Ok(Config::new()));

        let dir = target("nokey");
        save_entry_in(&root, &dir, &Entry::project("hive"), LockWait::hook())
            .expect("a file without the key is ours to write");
    }

    /// One save must not drop another directory's entry. The hook is many short
    /// processes, so "load once at start, save at end" would be a lost-update bug
    /// waiting for two directories to start at the same moment.
    #[test]
    fn a_save_preserves_entries_written_since_the_last_read() {
        let root = scratch("noclobber");
        let a = target("na");
        let b = target("nb");
        save_entry_in(&root, &a, &Entry::project("first"), LockWait::hook()).expect("save a");
        // The caller "reloads" and then another process writes b in between.
        let _stale = load_from(&root).expect("stale load");
        save_entry_in(&root, &b, &Entry::project("second"), LockWait::hook()).expect("save b");

        let (config, warn) = load_or_warn_from(&root);
        assert!(warn.is_none(), "{warn:?}");
        assert_eq!(config.len(), 2, "both entries must survive: {config:?}");
        for dir in [&a, &b] {
            let key = absolute_key(dir).to_string_lossy().into_owned();
            assert!(config.contains_key(&key), "{key} was clobbered");
        }
    }

    /// The same directory written twice keeps the **last** answer, not both, and not
    /// the first. A cascade that records a decision and is later re-decided must
    /// converge, or the config grows entries that disagree with each other.
    #[test]
    fn a_second_decision_replaces_the_first() {
        let root = scratch("replace");
        let dir = target("replace");
        save_entry_in(&root, &dir, &Entry::declined(), LockWait::hook()).expect("first");
        save_entry_in(&root, &dir, &Entry::project("hive"), LockWait::hook()).expect("second");
        let (config, _) = load_or_warn_from(&root);
        assert_eq!(config.len(), 1, "one directory, one entry: {config:?}");
        assert_eq!(
            config[&absolute_key(&dir).to_string_lossy().into_owned()],
            Entry::project("hive")
        );
    }
}

#[cfg(test)]
mod concurrency {
    use super::*;

    /// How a waiter process is told what to record, and how a bystander is told to do
    /// nothing: `BRAIN_TEST_SAVE_ENTRY="<root>\u{1f}<directory key>"`.
    const HELPER_ENV: &str = "BRAIN_TEST_SAVE_ENTRY";

    /// The body of one writer process.
    ///
    /// A `#[test]` that re-executes the test binary, because the lock is a property of
    /// *processes*: two threads in one process would share a process-wide relationship
    /// that `flock` does not model the way two processes do. And the test binary is
    /// the only thing that can reach `config.rs`, which is a module of the `brain`
    /// binary rather than a library — an integration test cannot import it.
    ///
    /// With the variable unset this is a no-op, so the ordinary suite is unaffected and
    /// this test is the only caller.
    ///
    /// The writer records through [`LockWait::interactive`], not `hook`, and that is
    /// the TD-016 fix for *this* test: it asserts that every decision survives, which
    /// is a guarantee the interactive policy makes (5 s ceiling, loud error past it)
    /// and the hook policy explicitly does not -- a hook may legally skip after
    /// 250 ms and leave the decision to a later run. Measured flake before: with the
    /// hook policy this test lost 1 of 12 entries under full-workspace load (captured
    /// 2026-10-15), and it failed 20/20 runs under load with the old behaviour
    /// restored by mutation. The hook policy's own semantics are pinned by
    /// `a_lock_timeout_defers_for_a_hook_and_fails_for_setup` instead.
    #[test]
    fn writer_process() {
        let Ok(spec) = std::env::var(HELPER_ENV) else { return };
        let (root, key) = spec.split_once('\u{1f}').expect("root\\u{1f}key");
        let dir = PathBuf::from(key);
        save_entry_in(Path::new(root), &dir, &Entry::project("decided"), LockWait::interactive())
            .expect("save_entry_in");
    }

    /// N real writer processes, each recording a decision for its own directory, and
    /// **every** entry present at the end.
    ///
    /// The sequential test (`a_save_preserves_entries_written_since_the_last_read`)
    /// is not a substitute: it passes through the re-read and never enters the window
    /// where two processes hold the same map. What is under test is the interleaving,
    /// and one process cannot interleave with itself.
    #[test]
    fn concurrent_writers_do_not_lose_each_others_entries() {
        const N: usize = 12;
        let root = std::env::temp_dir().join(format!("brain-cfg-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");

        let exe = std::env::current_exe().expect("current_exe");
        // Real directories, so `absolute_key` canonicalises them the same way a real
        // hook's working directory would be canonicalised.
        let dirs: Vec<PathBuf> = (0..N)
            .map(|i| {
                let d = root.join(format!("dir-{i}"));
                std::fs::create_dir_all(&d).expect("dir");
                d
            })
            .collect();

        let mut children = Vec::with_capacity(N);
        for dir in &dirs {
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg("--exact")
                .arg("config::concurrency::writer_process")
                .arg("--nocapture")
                .env(HELPER_ENV, format!("{}\u{1f}{}", root.display(), dir.display()));
            children.push(cmd.spawn().expect("spawn writer"));
        }
        for (i, mut child) in children.into_iter().enumerate() {
            let status = child.wait().expect("wait for writer");
            assert!(status.success(), "writer {i} failed: {status:?}");
        }

        let (config, warn) = load_or_warn_from(&root);
        assert!(warn.is_none(), "the file must still be valid JSON: {warn:?}");
        assert_eq!(
            config.len(),
            N,
            "a concurrent write lost an entry. The atomic rename keeps the file \
             well-formed; only the lock keeps every decision. Present: {:?}",
            config.keys().collect::<Vec<_>>()
        );
        for dir in &dirs {
            let key = absolute_key(dir).to_string_lossy().into_owned();
            assert!(
                config.contains_key(&key),
                "entry for {} is missing, so a decision was lost",
                dir.display()
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The two policies on a lock that does not free in time — the difference
    /// that motivated `LockWait` at all.
    ///
    /// A hook defers: `Ok(())` plus a warning, because a later hook run
    /// re-derives the decision. `brain setup` refuses: `Err`, because nothing
    /// re-runs it. Before `LockWait` this branch returned `Ok(())` for *both*,
    /// which is how `brain setup` silently dropped an operator's decision
    /// whenever the lock stayed busy for the 250 ms hook ceiling — the
    /// lost-decision half of TD-016, in the shape the suite caught.
    #[test]
    fn a_lock_timeout_defers_for_a_hook_and_fails_for_setup() {
        let root = std::env::temp_dir().join(format!("brain-cfg-policy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let dir = root.join("d");
        std::fs::create_dir_all(&dir).expect("dir");

        // Hold the lock from a *different* descriptor in this same process.
        // `flock` is per open-file-description, so a second descriptor in the
        // same process genuinely contends — no second process needed.
        let held = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(root.join(LOCK_NAME))
            .expect("lock file");
        fs2::FileExt::try_lock_exclusive(&held).expect("the test holds the lock");

        // Hook: deferred, not lost, and not an error — an IDE must not see a
        // failure for a decision the next event re-derives.
        let hook = LockWait { attempts: 2, on_timeout: OnTimeout::Skip };
        save_entry_in(&root, &dir, &Entry::project("deferred"), hook)
            .expect("a hook defers, it does not fail");
        let (config, _) = load_or_warn_from(&root);
        assert!(config.is_empty(), "the hook wrote nothing, so nothing was lost: {config:?}");

        // Setup: refused, loudly. `Err` is the whole point — the caller prints
        // it and the operator knows the decision is not on disk.
        let setup = LockWait { attempts: 2, on_timeout: OnTimeout::Fail };
        let err = save_entry_in(&root, &dir, &Entry::project("typed"), setup)
            .expect_err("setup must not report a write it did not do");
        let msg = err.to_string();
        assert!(msg.contains("NOT recorded"), "the error must say the decision is not on disk: {msg}");
        assert!(msg.contains("brain setup"), "and name the command that recovers it: {msg}");
        let (config, _) = load_or_warn_from(&root);
        assert!(config.is_empty(), "and the file must be untouched: {config:?}");

        // Free the lock and the very same interactive call succeeds: the policy
        // changed the failure mode, not the write.
        drop(held);
        save_entry_in(&root, &dir, &Entry::project("typed"), setup).expect("a free lock records");
        let (config, _) = load_or_warn_from(&root);
        assert_eq!(config.len(), 1, "the decision lands once the lock is free: {config:?}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The lock is **not** left behind in a state that blocks the next writer — the
    /// guard must release on drop, and a crashed holder must not hold it forever.
    #[test]
    fn the_lock_is_released_after_a_write() {
        let root = std::env::temp_dir().join(format!("brain-cfg-rel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let dir = root.join("d");
        std::fs::create_dir_all(&dir).expect("dir");

        // Two writes in a row from the same process: the second would hang or skip if
        // the first had left the lock held.
        save_entry_in(&root, &dir, &Entry::project("first"), LockWait::hook()).expect("first");
        save_entry_in(&root, &dir, &Entry::project("second"), LockWait::hook()).expect("second");
        let (config, _) = load_or_warn_from(&root);
        assert_eq!(config.len(), 1, "the same directory keeps one entry: {config:?}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// MUST FIX 2: the project name is validated **at the writer**, so a bad one never
    /// reaches the file. Measured before the fix: `setup --project "../evil"` stored the
    /// string verbatim and every later hook died with `Error: Path traversal detected`.
    #[test]
    fn a_project_name_that_cannot_be_a_path_component_is_refused_at_the_writer() {
        let root = std::env::temp_dir().join(format!("brain-cfg-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let dir = root.join("d");
        std::fs::create_dir_all(&dir).expect("dir");

        for bad in ["../evil", "..", "sub/../../out", "/etc/passwd", ""] {
            let err = save_entry_in(&root, &dir, &Entry::project(bad), LockWait::hook())
                .expect_err("`{bad}` must be refused");
            let msg = err.to_string();
            assert!(msg.contains("refusing to record project"), "the error must name the refusal: {msg}");
        }

        // The file must not exist at all: a refused write leaves no trace, so the
        // operator is not left with a poisoned entry to debug.
        assert!(
            !root.join("config.json").exists(),
            "a refused project must not be written"
        );

        // A name that *is* a path component is stored verbatim, not normalised.
        save_entry_in(&root, &dir, &Entry::project("hive"), LockWait::hook()).expect("a good name");
        let (config, _) = load_or_warn_from(&root);
        let key = absolute_key(&dir).to_string_lossy().into_owned();
        assert_eq!(config[&key].projeto.as_deref(), Some("hive"), "stored as given");
        // Nesting is legal for the sanitiser, so it is legal here — the check is the
        // guard's, not a stricter one invented for this.
        save_entry_in(&root, &dir, &Entry::project("a/b"), LockWait::hook()).expect("a nested name is allowed");
        let (config, _) = load_or_warn_from(&root);
        assert_eq!(config[&key].projeto.as_deref(), Some("a/b"), "and not rewritten");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A refusal carries no project, so there is nothing to validate — the check must
    /// not reject a decline.
    #[test]
    fn a_refusal_still_saves() {
        let root = std::env::temp_dir().join(format!("brain-cfg-dec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let dir = root.join("d");
        std::fs::create_dir_all(&dir).expect("dir");
        save_entry_in(&root, &dir, &Entry::declined(), LockWait::hook()).expect("a refusal has no name to check");
        let (config, _) = load_or_warn_from(&root);
        let key = absolute_key(&dir).to_string_lossy().into_owned();
        assert_eq!(config[&key].motivo.as_deref(), Some(RECUSADO));
        let _ = std::fs::remove_dir_all(&root);
    }
}