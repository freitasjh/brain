//! B6 — the legacy import, and the `vault.bak.tar.gz` that makes it reversible.
//!
//! # What the spec asked for
//!
//! US-01.1 AC3 verbatim: *"WHEN server starts AND legacy `vault/` or `data/index.db`
//! exists THEN system SHALL import legacy data to `brain.db` and backup to
//! `vault.bak.tar.gz`"*, and B6: *"Import `vault/**/*.md` + `data/index.db` →
//! `brain.db`, `vault.bak.tar.gz`"*.
//!
//! The import half existed (`brain migrate`). The **backup half did not**: the
//! import read a directory of markdown and wrote a database, and the only copy of
//! the original stayed where it was — which sounds safe until you notice what the
//! import actually is. It is not a copy. `note_upsert` takes a note's identity
//! from the *path*, and the mapping from vault path to layer/scope/project is
//! re-derived on every run from the frontmatter and the directory shape. Change
//! `parse_frontmatter`, change a layer name, change the scope inference — and a
//! second `migrate` writes a **different** set of rows under paths derived from
//! the same files. Nothing is deleted, so nothing looks destructive, and there is
//! no way back: the DB now holds notes that no longer correspond to the files,
//! and the files say nothing about what the DB decided they meant.
//!
//! So the backup is taken **before** the first write, from the same source the
//! import is about to read, and it is the reason the import is allowed to happen.
//!
//! # The three decisions, and why
//!
//! ## 1. When the backup runs
//!
//! Whenever there is **something to import** — the legacy tree exists and holds at
//! least one `.md`. Not "only if the backup is missing", and not "unconditionally".
//!
//! The tempting alternative, `if !backup.exists()`, is wrong in the exact
//! direction that matters: a *second* import is precisely the run where the
//! mapping may differ, so it is the run that most needs a record of what the
//! first one saw. Skipping the backup because a backup already exists inverts
//! that — it protects the case that needed no protection and skips the one that
//! did.
//!
//! The other tempting alternative, "always back up, even with nothing to import",
//! is harmless but dishonest: it writes a `vault.bak.tar.gz` next to no vault at
//! all, and the operator who finds that file later cannot tell a real backup from
//! an empty one.
//!
//! ## 2. What goes in the tarball
//!
//! **Exactly the bytes the import reads** — the legacy tree, nothing else.
//!
//! `data/index.db` is named by B6, so the honest question is whether it belongs
//! in the archive. It does not: [`import_legacy`] never opens it (that is
//! `Migrate`'s pre-existing `let _ = old_index;`, reported below as a known gap),
//! so archiving it would put a file in a "backup of what I just transformed"
//! bundle that nothing transformed. A backup that silently includes untouched
//! files teaches the operator to trust the bundle as "everything involved", which
//! is the belief that makes them skip verifying it. [`import_legacy`] **reports** a
//! legacy index when it finds one, so the gap is visible rather than hidden.
//!
//! The walk skips nothing: `walkdir` with the same filter the import uses, so the
//! archive and the import see one file list. A backup that filtered differently
//! from the import would be a backup of a subset, which is the failure mode you
//! cannot detect by looking at it.
//!
//! ## 3. Where it is written — and why not next to the vault
//!
//! [`BRAIN_EXPORT_ROOT`](brain_mcp::fs_guard::EXPORT_ROOT_ENV), the one
//! allowlisted write location the system already has, default
//! `/tmp/brain-export`. Not beside `--vault`.
//!
//! Two reasons, and the first is the one that would have burned an operator:
//!
//! - **A vault is usually a git repository.** An Obsidian vault is a folder of
//!   markdown that people version. Writing a whole-corpus `.tar.gz` into it puts
//!   a binary blob where `git status` will report it, where a `git add -A` will
//!   commit it, and where every future clone pays for it. The artifact most likely
//!   to be committed by accident is a backup.
//! - **Reuse beats a second policy.** The export root's containment
//!   ([`resolve_within`](brain_mcp::fs_guard::resolve_within)) already canonicalizes
//!   the deepest existing ancestor before comparing, so `..` and a symlinked
//!   parent are both resolved before the check rather than after it. A fresh
//!   "write beside the vault" check would be a second implementation of that
//!   logic, and the only thing that decides a destination's safety is that logic.
//!   One policy, one audit.
//!
//! ## 4. What happens when the backup fails
//!
//! **The import does not run.** [`import_legacy`] returns the error and writes
//! nothing.
//!
//! This is the one place where a "best-effort" attitude would be actively
//! harmful. The whole argument for the backup is that the import is a one-way
//! transformation of somebody's only copy of their memory. A best-effort backup
//! that silently failed would leave the operator believing a safety net exists
//! when it does not — and the belief is the entire product of this feature. The
//! import is cheap to retry and has no partial state before the first write, so
//! refusing costs the operator one command. Proceeding costs them their vault.
//!
//! ## 5. Second run
//!
//! Never clobbers. [`backup_vault`] writes `vault.bak.tar.gz` when that name is
//! free, and otherwise the first free `vault.bak.N.tar.gz` (N from 2), reporting
//! which one it chose. The first backup's bytes are never touched, so "did the
//! first run see something different from the second?" is answerable after the
//! fact — which is the only reason the archive exists.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;

/// The archive name the spec names. The first backup of a run gets exactly this.
const ARCHIVE_NAME: &str = "vault.bak.tar.gz";

/// What one import produced, for the caller to print and for tests to assert on.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// Notes written to `brain.db`.
    pub notes: usize,
    /// Chunks those notes produced.
    pub chunks: usize,
    /// Chunks left without a vector, to be filled by the embed queue.
    pub without_embedding: usize,
    /// Where the pre-import archive was written. `None` when there was nothing
    /// to import, which is the normal case on a machine that migrated long ago.
    pub backup: Option<PathBuf>,
    /// A legacy `data/index.db` that exists but is **not** imported — see the
    /// module docs. Surfaced so the gap is reported instead of inferred from
    /// silence.
    pub legacy_index_present: Option<PathBuf>,
}

/// An import plus the note bodies it wrote, so a caller that wants vectors can
/// embed them.
///
/// The bodies are returned rather than embedded in place because the two callers
/// want opposite things: the server defers to its queue (chunks stay `NULL` and
/// boot recovery picks them up), while `brain migrate` embeds inline because a
/// one-shot process has no queue to survive it. Sharing the *write* half and
/// letting each caller choose the embedding half is what keeps them from
/// drifting apart.
#[derive(Debug, Default)]
pub struct ImportOutcome {
    pub report: ImportReport,
    /// One entry per imported note, carrying everything a second pass needs to
    /// re-sync the same chunks with vectors — so `brain migrate` never has to
    /// re-read the note back out to find its layer.
    pub staged: Vec<StagedNote>,
}

/// One legacy-vault note staged for import:
/// `(note_id, path, layer, scope, content, project_id, tags)`.
pub type StagedNote = (i64, String, String, Option<String>, String, Option<i64>, Vec<String>);

/// True when `dir` exists, is a directory, and holds at least one `.md` file.
///
/// The `.md` test is what makes this "is there anything to import" rather than
/// "does this path exist": an empty `vault/`, a `vault` that is really a file, or
/// a directory of `README.txt` must all produce no backup and no import, because
/// all three would otherwise write an archive that looks like a safety net and
/// contains nothing to restore.
pub fn has_legacy_notes(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .any(|e| e.path().is_file() && e.path().extension().is_some_and(|x| x == "md"))
}

/// Picks the archive path for this run: `vault.bak.tar.gz`, or the first free
/// `vault.bak.N.tar.gz`.
///
/// The existence probe is `symlink_metadata`, so a **dangling symlink** named
/// `vault.bak.tar.gz` still counts as taken. A plain `exists()` follows the link,
/// returns false for a broken one, and the archive would then be written *through*
/// the link to wherever it points — reintroducing exactly the arbitrary write the
/// export-root containment exists to close, one symlink at a time. Opening with
/// `create_new` afterwards is what actually makes the reservation atomic; the
/// probe is only here to pick a name.
fn free_archive_path(root: &Path) -> Result<PathBuf> {
    let first = root.join(ARCHIVE_NAME);
    if std::fs::symlink_metadata(&first).is_err() {
        return Ok(first);
    }
    for n in 2..10_000u32 {
        let candidate = root.join(format!("vault.bak.{n}.tar.gz"));
        if std::fs::symlink_metadata(&candidate).is_err() {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "no free vault.bak.N.tar.gz name left in {} — move the existing archives aside and retry",
        root.display()
    )
}

/// Archives `vault_dir` into the export root, returning the file it wrote.
///
/// Fails rather than returning a path: [`import_legacy`] turns that into a
/// refusal to import, which is the point (module docs §4).
pub fn backup_vault(vault_dir: &Path) -> Result<PathBuf> {
    let root = brain_mcp::fs_guard::export_root();
    // The same uid/other-writable gate export and backup use. A root another
    // local user controls is a root they can read the whole corpus out of, and
    // this archive is the whole corpus.
    brain_mcp::fs_guard::assert_root_usable(&root)?;
    std::fs::create_dir_all(&root)?;
    // Re-checked after creation: the pre-creation check is a no-op on a root that
    // did not exist, so this is the half that closes the window (same reasoning,
    // and same ordering, as the export handler in brain-mcp).
    brain_mcp::fs_guard::assert_root_usable(&root)?;

    // The candidate is named through the containment check first — the same
    // `resolve_within` export uses, so the rule is one rule — and the
    // non-colliding name is then picked *inside* the resolved root. Both halves
    // are needed: the check proves the name is allowed, the picker proves nothing
    // is already there under it.
    brain_mcp::fs_guard::resolve_within(&root, ARCHIVE_NAME, "vault backup")?;
    let dest = free_archive_path(&root)?;

    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest)
        .map_err(|e| anyhow::anyhow!("cannot create {}: {e}", dest.display()))?;
    let gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(gz);

    // Archive names are relative to the vault's **parent**, so the tree restores
    // as `vault/...` and keeps the name that identifies it. The parent rather
    // than the vault dir itself, because anchoring at the vault would strip the
    // `vault/` component and produce a directory of loose files.
    //
    // An entry name is **data** in a tarball: whatever is written here is what a
    // future `tar xzf` will create paths from, so an absolute path or a `..` in
    // one is an extraction escape, not a cosmetic wart. Both are rejected rather
    // than sanitised — a name that needed sanitising is a name whose provenance
    // this function does not understand, and quietly rewriting it would produce an
    // archive that differs from the tree it claims to be a copy of.
    let base = vault_dir.parent().unwrap_or_else(|| Path::new("."));
    let mut written = 0usize;
    for entry in walkdir::WalkDir::new(vault_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.path().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(base)
            .map_err(|_| anyhow::anyhow!("{} is not under {}", entry.path().display(), base.display()))?;
        if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            anyhow::bail!(
                "refusing to archive {} under the name {:?}: an archive entry that escapes its root is an \
                 extraction escape",
                entry.path().display(),
                rel
            );
        }
        builder.append_path_with_name(entry.path(), rel)?;
        written += 1;
    }
    if written == 0 {
        anyhow::bail!("{} holds no readable file to archive", vault_dir.display());
    }
    let gz = builder.into_inner()?;
    // The encoder must be finished explicitly: dropping it would leave the gzip
    // stream without its trailer, and every reader would report a truncated
    // archive — so the error is surfaced here instead of at the first restore.
    let mut file = gz.finish()?;
    file.flush()?;
    file.sync_all()?;
    drop(file);
    if std::fs::metadata(&dest)?.len() == 0 {
        anyhow::bail!("the archive at {} came out empty", dest.display());
    }
    Ok(dest)
}

/// Reads `vault_dir/**/*.md` into `brain.db`, archiving it first when it holds
/// anything.
///
/// Embedding is deliberately **not** done here (see [`ImportReport`]): the
/// server's boot recovery re-queues every chunk left `NULL`, so the imported
/// corpus is hydrated by the same queue that hydrates an ordinary write, and a
/// start never blocks on a network batch.
pub fn import_legacy(db: &str, vault_dir: &Path, old_index: &str) -> Result<ImportOutcome> {
    let mut out = ImportOutcome::default();
    if Path::new(old_index).exists() {
        out.report.legacy_index_present = Some(PathBuf::from(old_index));
    }
    if !has_legacy_notes(vault_dir) {
        return Ok(out);
    }
    out.report.backup = Some(backup_vault(vault_dir)?);

    let store = Store::open(db)?;
    for entry in walkdir::WalkDir::new(vault_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.path().is_file() || entry.path().extension().is_none_or(|e| e != "md") {
            continue;
        }
        let rel = match entry.path().strip_prefix(vault_dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let content = std::fs::read_to_string(entry.path()).unwrap_or_default();
        let path_str = rel.with_extension("").to_string_lossy().to_string();
        let parts: Vec<&str> = path_str.split('/').collect();
        if parts.is_empty() {
            continue;
        }
        let layer = parts[0].to_string();
        if !brain_core::VALID_LAYERS.contains(&layer.as_str()) {
            continue;
        }
        let (proj, tags) = brain_core::parse_frontmatter(&content);
        let pid = proj
            .as_ref()
            .and_then(|p| store.project_get(p).ok().flatten().or_else(|| store.project_create(p, "").ok()).map(|pr| pr.id));
        let scope = if brain_core::LAYERS_WITH_SCOPE.contains(&layer.as_str())
            && parts.len() >= 2
            && brain_core::VALID_SCOPES.contains(&parts[1])
        {
            Some(parts[1].to_string())
        } else {
            None
        };
        if let Ok(nid) = store.note_upsert(&path_str, &layer, scope.as_deref(), &content, pid, &tags, false, None) {
            let st = store.chunks_sync(
                nid,
                &path_str,
                &layer,
                scope.as_deref(),
                &content,
                pid,
                &tags,
                &Default::default(),
                &brain_store::ChunkSnapshot::new(),
            )?;
            out.report.chunks += st.total;
            out.report.without_embedding += st.nulls;
            out.report.notes += 1;
            out.staged.push((nid, path_str.clone(), layer, scope, content.clone(), pid, tags));
        }
    }
    Ok(out)
}

use brain_store::Store;

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the tests that call `backup_vault`, because the export root is
    /// read from a **process-global** environment variable.
    ///
    /// Letting each test pick a different root is not available: the variable
    /// holds one value at a time, so two tests with different roots would race
    /// and each would write into the other's directory. A shared root plus this
    /// lock is the only honest arrangement — and it also means these tests never
    /// touch the real `/tmp/brain-export` that the running production server and
    /// its operator share.
    static EXPORT_ROOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Points `BRAIN_EXPORT_ROOT` at a fresh scratch directory for the duration
    /// of one test, restoring the previous value on drop — including on panic, so
    /// a failing test cannot leave every later test writing somewhere else.
    struct ScratchRoot {
        dir: PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
        prev: Option<String>,
    }

    impl ScratchRoot {
        fn new(tag: &str) -> ScratchRoot {
            let guard = EXPORT_ROOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let dir = std::env::temp_dir().join(format!("brain-legacy-root-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let prev = std::env::var(brain_mcp::fs_guard::EXPORT_ROOT_ENV).ok();
            // Safety: held under EXPORT_ROOT_LOCK, so no other test in this binary
            // reads the variable concurrently. `set_var` is `unsafe` only because
            // another thread *could* be reading it, which the lock excludes.
            unsafe { std::env::set_var(brain_mcp::fs_guard::EXPORT_ROOT_ENV, &dir) };
            ScratchRoot { dir, _guard: guard, prev }
        }
    }

    impl Drop for ScratchRoot {
        fn drop(&mut self) {
            unsafe {
                match &self.prev {
                    Some(v) => std::env::set_var(brain_mcp::fs_guard::EXPORT_ROOT_ENV, v),
                    None => std::env::remove_var(brain_mcp::fs_guard::EXPORT_ROOT_ENV),
                }
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("brain-legacy-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn vault_with(dir: &Path, n: usize) {
        for i in 0..n {
            let p = dir.join("regras/global").join(format!("n{i}.md"));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, format!("## Nota {i}\nconteudo unico-{i}\n")).unwrap();
        }
    }

    /// Reads a `.tar.gz` back with an independent reader, so the tests assert on
    /// the **format** and not merely on the crate that wrote it. Round-tripping
    /// through the same library would pass on a container that is well-formed for
    /// that library and unreadable by `tar xzf` — which is the failure an operator
    /// restoring their vault would actually hit.
    fn read_targz(path: &Path) -> Vec<(String, String)> {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let f = std::fs::File::open(path).unwrap();
        let mut ar = tar::Archive::new(GzDecoder::new(f));
        let mut out = Vec::new();
        for e in ar.entries().unwrap() {
            let mut e = e.unwrap();
            let mut body = String::new();
            e.read_to_string(&mut body).unwrap();
            out.push((e.path().unwrap().to_string_lossy().to_string(), body));
        }
        out.sort();
        out
    }

    #[test]
    fn has_legacy_notes_rejects_a_directory_with_no_markdown() {
        let d = scratch("none");
        std::fs::create_dir_all(d.join("regras")).unwrap();
        assert!(!has_legacy_notes(&d));
        std::fs::write(d.join("regras/readme.txt"), "x").unwrap();
        assert!(!has_legacy_notes(&d), "a .txt is not a note");
        vault_with(&d, 1);
        assert!(has_legacy_notes(&d));
        // A path that is a file, not a directory.
        let f = d.join("vault");
        std::fs::write(&f, "x").unwrap();
        assert!(!has_legacy_notes(&f));
        // A path that does not exist at all.
        assert!(!has_legacy_notes(Path::new("/nonexistent-legacy-vault")));
    }

    /// B6: the first backup is named exactly what the spec names, and it lands
    /// inside the export root rather than next to the vault.
    #[test]
    fn the_first_backup_is_vault_bak_tar_gz_inside_the_export_root() {
        let root = ScratchRoot::new("first");
        let d = scratch("vfirst");
        vault_with(&d, 2);
        let b = backup_vault(&d).unwrap();
        assert_eq!(b, root.dir.join("vault.bak.tar.gz"));
        let entries = read_targz(&b);
        assert_eq!(entries.len(), 2, "both notes must be in the archive: {entries:?}");
        for (name, body) in &entries {
            assert!(!name.starts_with('/'), "an absolute entry name is an extraction escape: {name}");
            assert!(!name.contains(".."), "a climbing entry name is an extraction escape: {name}");
            assert!(name.contains("regras/global/"), "entry lost its layer path: {name}");
            assert!(body.contains("conteudo unico"), "entry lost its body: {name}");
        }
    }

    /// B6: a second run must not be able to destroy the first run's record.
    ///
    /// This is the case the `if !backup.exists()` shortcut gets wrong: the second
    /// import is the one whose mapping may differ, so the second run is the one
    /// that most needs to be distinguishable from the first.
    #[test]
    fn a_second_backup_does_not_touch_the_first() {
        let root = ScratchRoot::new("second");
        let d = scratch("vsecond");
        vault_with(&d, 2);
        let first = backup_vault(&d).unwrap();
        let bytes = std::fs::read(&first).unwrap();
        let second = backup_vault(&d).unwrap();
        assert_eq!(second, root.dir.join("vault.bak.2.tar.gz"));
        assert_eq!(
            std::fs::read(&first).unwrap(),
            bytes,
            "the first archive must survive byte-identical"
        );
        let third = backup_vault(&d).unwrap();
        assert_eq!(third, root.dir.join("vault.bak.3.tar.gz"));
        for p in [&first, &second, &third] {
            assert_eq!(read_targz(p).len(), 2, "{} must hold both notes", p.display());
        }
    }

    /// A dangling symlink named like the archive must not be written *through*.
    ///
    /// `Path::exists()` follows the link and reports false for a broken one, so a
    /// picker built on it would hand back the symlink's own name and the archive
    /// would land wherever the link points — the arbitrary write the export root
    /// exists to close, one symlink at a time.
    #[test]
    fn a_dangling_symlink_named_like_the_archive_is_not_reused() {
        let root = ScratchRoot::new("symlink");
        let link = root.dir.join(ARCHIVE_NAME);
        std::os::unix::fs::symlink(root.dir.join("does-not-exist"), &link).unwrap();
        assert!(!link.exists(), "the link is dangling, which is the whole point");
        let picked = free_archive_path(&root.dir).unwrap();
        assert_ne!(picked, link);
        // And the real backup neither follows the link nor creates the target.
        let d = scratch("vsym");
        vault_with(&d, 1);
        let b = backup_vault(&d).unwrap();
        assert_ne!(b, link);
        assert!(!root.dir.join("does-not-exist").exists(), "the write followed the dangling link");
        assert_eq!(read_targz(&b).len(), 1);
    }

    /// An export root another local user controls must stop the backup, because
    /// the archive is the whole corpus in one file.
    #[test]
    fn a_backup_into_a_root_we_do_not_own_is_refused() {
        let root = ScratchRoot::new("refuse");
        // A group-writable-but-not-ours directory cannot be made without a second
        // uid, so what is exercised here is the other branch that needs no
        // privilege: the path is a **file**, so `assert_root_usable` refuses it
        // before a single byte is written.
        std::fs::remove_dir_all(&root.dir).unwrap();
        std::fs::write(&root.dir, b"not a directory").unwrap();
        let d = scratch("vrefuse");
        vault_with(&d, 1);
        let e = backup_vault(&d).unwrap_err().to_string();
        assert!(e.contains("not a directory"), "{e}");
    }

    #[test]
    fn an_empty_legacy_tree_imports_nothing_and_writes_no_archive() {
        let _root = ScratchRoot::new("empty");
        let d = scratch("vempty");
        std::fs::create_dir_all(d.join("regras")).unwrap();
        let o = import_legacy("/tmp/brain-legacy-empty-nope.db", &d, "/tmp/nope-index.db").unwrap();
        assert_eq!(o.report, ImportReport::default());
        assert!(o.report.backup.is_none());
        assert!(o.staged.is_empty());
    }

    /// B6's `data/index.db`: detected and reported, and deliberately *not*
    /// archived. Archiving a file nothing read would put an untouched file in a
    /// bundle labelled "what I just transformed", which teaches the operator to
    /// trust the bundle as a complete record when it is not one.
    #[test]
    fn a_legacy_index_is_reported_but_not_archived() {
        let root = ScratchRoot::new("idx");
        let idx = scratch("vidx").join("index.db");
        std::fs::write(&idx, b"sqlite").unwrap();
        let o = import_legacy(
            "/tmp/brain-legacy-idx-nope.db",
            Path::new("/nonexistent-vault"),
            idx.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(o.report.legacy_index_present.as_deref(), Some(idx.as_path()));
        assert!(o.report.backup.is_none(), "no vault to import means no archive");
        assert!(!root.dir.join(ARCHIVE_NAME).exists());
    }

    /// End to end through the real store: two legacy notes are archived, land in
    /// `brain.db`, and are searchable by FTS5 with no vector at all.
    #[test]
    fn an_import_archives_the_vault_and_writes_searchable_notes() {
        let _root = ScratchRoot::new("e2e");
        let d = scratch("ve2e");
        let vault = d.join("vault");
        vault_with(&vault, 2);
        let db = d.join("brain.db");
        let o = import_legacy(db.to_str().unwrap(), &vault, "/nonexistent-index.db").unwrap();
        assert_eq!(o.report.notes, 2, "{:?}", o.report);
        assert_eq!(o.staged.len(), 2);
        assert!(o.report.chunks >= 2);
        let archive = o.report.backup.expect("a vault with notes must be archived");
        assert_eq!(read_targz(&archive).len(), 2);

        let store = Store::open(db.to_str().unwrap()).unwrap();
        assert_eq!(store.count_notes().unwrap(), 2);
        // FTS-only search: no vector was produced, which is what the queue's job is.
        let hits = store.search("unico", None, None, None, None, None, 5, false).unwrap();
        assert_eq!(hits.len(), 2, "both imported notes are FTS-searchable: {hits:?}");
        let cov = store.embedding_coverage().unwrap();
        assert_eq!(cov.embedded, 0, "the import must leave vectors to the queue, not invent them");
        assert_eq!(cov.without_embedding, cov.total);
    }

    /// The import is refused before it writes anything when the archive cannot be
    /// taken. A best-effort backup that silently failed would leave the operator
    /// believing a safety net exists when it does not.
    #[test]
    fn a_failed_archive_stops_the_import_before_any_note_is_written() {
        let root = ScratchRoot::new("norefs");
        std::fs::remove_dir_all(&root.dir).unwrap();
        std::fs::write(&root.dir, b"not a directory").unwrap();
        let d = scratch("vnoref");
        let vault = d.join("vault");
        vault_with(&vault, 2);
        let db = d.join("brain.db");
        let err = import_legacy(db.to_str().unwrap(), &vault, "/nonexistent-index.db").unwrap_err();
        assert!(err.to_string().contains("not a directory"), "{err:#}");
        assert!(
            !db.exists(),
            "the database must not even be created when the archive could not be taken"
        );
    }
}
