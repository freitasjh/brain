use anyhow::Result;
use brain_core::{sanitize_relative_path, validate_layer, validate_scope, LAYERS_WITH_SCOPE};
use brain_mcp::{ChunkSyncInput, embed_chunks, sync_note_chunks};
use brain_store::{NoteEmbed, Store};
use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;

mod legacy_import;
mod setup;

/// US-01.1 AC1. The SSE port: `--port` when given, else `BRAIN_PORT`, else 8321.
///
/// A function rather than a clap `default_value_t` because the default has to be
/// an **environment variable**, and clap's `default_value` cannot read one for a
/// subcommand of `Cli` that already has `--db env=...`. `serve-mcp` spells the
/// same constant in its own `default_value_t`; both are documented in `AGENTS.md`
/// as `BRAIN_PORT=8321`, and this is the one place that actually honours the
/// variable rather than only naming it.
fn default_mcp_port() -> u16 {
    std::env::var("BRAIN_PORT").ok().and_then(|v| v.trim().parse::<u16>().ok()).unwrap_or(8321)
}

/// TD-010. Write one line to stdout, treating a closed stdout as a normal ending.
///
/// `println!` panics when the write fails, and for a CLI the most common reason a
/// stdout write fails is that the reader went away: `brain recent | head -1` closes
/// the pipe while we are still writing, `println!` unwraps the resulting `EPIPE`, and
/// the process dies with exit 101 and a panic on stderr. That is wrong twice over.
/// Closing the pipe early is the ordinary Unix contract — `| head`, `| grep -m`, `| less`
/// that stops paging all do it — and the exit code is precisely what a script tests,
/// so a pipeline that worked reported failure.
///
/// Only `BrokenPipe` is absorbed, and it exits 0 silently, because there is by then no
/// consumer left for the bytes and nothing useful left to report. **Every other write
/// error keeps `println!`'s behaviour — a panic** — so a genuine write failure (a full
/// disk, a write-only descriptor) is still loud instead of being silently truncated
/// into a `0` exit that a caller would read as success. The asymmetry is the point:
/// this macro widens the set of *successful* exits by exactly the one case that is not
/// a failure, and by nothing else.
///
/// A single `println!` of a large value is enough to trigger this: the value is handed
/// to one `write_fmt`, and once it exceeds the pipe buffer the tail of that one write
/// lands on a closed pipe.
macro_rules! outln {
    ($($arg:tt)*) => {{
        if let Err(e) = writeln!(std::io::stdout(), $($arg)*) {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                std::process::exit(0);
            }
            panic!("failed writing to stdout: {e}");
        }
    }};
}

#[derive(Parser)]
#[command(name="brain", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    #[arg(long, env="BRAIN_DB_PATH", default_value="./data/brain.db")]
    db: String,
}

#[derive(Subcommand)]
enum Cmd {
    /// Health-check (prints pong)
    Ping,
    /// Store a note (scope required for arquitetura/regras/estudos)
    Store { layer: String, path: String, content: String, #[arg(long)] scope: Option<String>, #[arg(long)] project: Option<String>, #[arg(long)] tags: Option<String>, #[arg(long)] pinned: bool, #[arg(long)] expires_at: Option<String> },
    /// Read a note by layer/path (+scope)
    Read { layer: String, path: String, #[arg(long)] scope: Option<String> },
    /// Hybrid search (FTS5+vector RRF, filters, --explain)
    Search { query: String, #[arg(long)] layer: Option<String>, #[arg(long)] scope: Option<String>, #[arg(long)] project: Option<String>, #[arg(long)] tag: Option<String>, #[arg(long, default_value_t=5)] top_k: usize, #[arg(long)] explain: bool },
    /// Hard-delete a note by full path
    Delete { path: String },
    /// Latest notes by updated_at
    Recent { #[arg(long, default_value_t=10)] top_k: usize },
    /// DB counts (notes/chunks/projects)
    Status,
    /// Rebuild FTS5+vector index. Embeds first, then writes in one transaction.
    /// `--no-embed` skips the embedding pass (offline structural reindex; vectors
    /// that already match their chunk text are preserved, the rest stay NULL).
    Reindex { #[arg(long)] all: bool, #[arg(long)] no_embed: bool },
    /// Audit log (time-travel)
    Checkpoints { #[arg(long, default_value_t=10)] limit: usize },
    /// Restore an audit entry by id
    Restore { id: i64 },
    /// Copy brain.db to .bak
    Backup { #[arg(long)] to: Option<String> },
    /// Dump notes to a directory (temp)
    Export { #[arg(long, default_value="/tmp/brain-export")] to: String, #[arg(long)] force: bool },
    /// TTL sweep (expired notes, pin never expires)
    ForgetSweep { #[arg(long)] dry_run: bool },
    /// Import legacy vault .md files. Embeds the imported corpus unless --no-embed.
    Migrate { #[arg(long, default_value="./vault")] vault: String, #[arg(long, default_value="./data/index.db")] old_index: String, #[arg(long)] no_embed: bool },
    /// Read-only viewer (default 8322)
    Serve { #[arg(long, default_value_t=8322)] port: u16 },
    /// MCP SSE server (default 8321) + stdio fallback
    ServeMcp { #[arg(long, default_value_t=8321)] port: u16 },
    /// Operator lifecycle of the MCP server (US-01.1). The spec asks for `start`
    /// and only `start`; `serve` / `serve-mcp` bind the same ports as before and
    /// serve the same tools. Their shutdown is also the same now: all three end on
    /// SIGINT **or** SIGTERM, so `systemctl stop` runs the teardown.
    Server { #[command(subcommand)] sub: ServerCmd },
    /// Agent lifecycle hook (session-start|tool-result|session-end)
    Hook { #[arg(long, value_parser=["session-start","tool-result","session-end"])] event: String, #[arg(long)] project: String, #[arg(long)] payload: Option<String> },
    /// One-shot installer: setup [all|opencode|systemd|shell|project]
    Setup { #[arg(default_value = "all")] target: String, #[arg(long, default_value_t = 8321)] mcp_port: u16, #[arg(long, default_value_t = 8322)] viewer_port: u16, #[arg(long)] brain_dir: Option<String>, #[arg(long)] dir: Option<String>, #[arg(long)] force: bool, #[arg(long)] dry_run: bool },
    /// Projects CRUD + note link/unlink
    Project { #[command(subcommand)] sub: ProjectCmd },
}

#[derive(Subcommand)]
enum ProjectCmd { Create { name: String, #[arg(long, default_value="")] description: String }, List, Delete { name: String }, Notes { name: String }, Link { note_path: String, project: String }, Unlink { note_path: String, project: String } }

/// US-01.1. One subcommand, because the spec asks for one.
#[derive(Subcommand)]
enum ServerCmd {
    /// Start the MCP SSE server: import legacy data (archiving it first), then
    /// serve. Port defaults to `BRAIN_PORT`, 8321.
    Start {
        /// Override the SSE port. Omit to use `BRAIN_PORT` (default 8321).
        #[arg(long)]
        port: Option<u16>,
        /// Legacy vault directory to import from, if it holds notes.
        #[arg(long, default_value="./vault")]
        vault: String,
        /// Legacy index database. Detected and reported; not imported (see B6).
        #[arg(long, default_value="./data/index.db")]
        old_index: String,
    },
}

static REINDEXING: std::sync::OnceLock<std::sync::Mutex<bool>> = std::sync::OnceLock::new();
fn reindex_lock() -> &'static std::sync::Mutex<bool> { REINDEXING.get_or_init(|| std::sync::Mutex::new(false)) }

fn full_path(layer: &str, path: &str, scope: Option<&str>) -> Result<String> {
    validate_layer(layer)?;
    sanitize_relative_path(path)?;
    if LAYERS_WITH_SCOPE.contains(&layer) {
        let s = scope.ok_or_else(|| anyhow::anyhow!("scope required for {}", layer))?;
        validate_scope(s)?;
        Ok(format!("{}/{}/{}", layer, s, path))
    } else { Ok(format!("{}/{}", layer, path)) }
}

/// One legacy-vault note staged for import:
/// `(note_id, path, layer, scope, content, project_id, tags)`.
/// B6 moved this into [`legacy_import`], next to the code that builds it — the
/// import and the type describing its output belong in one file, and there is
/// only one place that constructs one now.
#[allow(unused_imports)]
use legacy_import::StagedNote as PendingImport;

fn hook_spool_path() -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(base).join("brain/hook-spool.jsonl")
}

/// Embeds every chunk of every note in a single pass, returning the
/// `{path: [vector per chunk_index]}` map that [`Store::reindex_all_with`] takes.
///
/// One batch for the whole corpus rather than one per note: the batch runs 4
/// requests in flight, so a single batch amortises that concurrency across every
/// note instead of restarting it (and re-paying connection setup) per note.
///
/// Each vector is returned paired with the chunk text it was computed from, in a
/// [`NoteEmbed`]. That pairing is what makes the write safe: this pass takes
/// minutes against a serial Ollama, and a `brain_store` landing inside that
/// window would otherwise have the *old* text's vectors written onto the *new*
/// text's chunks. The first chunk that fails to embed truncates that note's list —
/// everything after the gap is dropped rather than shifted, because a vector
/// attributed to the wrong chunk is a wrong answer, not a degraded one.
async fn embed_all_notes(notes: &[(String, String)]) -> std::collections::HashMap<String, NoteEmbed> {
    use std::collections::HashMap;
    let mut out: HashMap<String, NoteEmbed> = HashMap::new();
    if notes.is_empty() { return out; }
    // (note index, chunk index) for every enqueued text, same order as `texts`.
    let mut owner: Vec<(usize, usize)> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    for (ni, (_path, content)) in notes.iter().enumerate() {
        for (ci, ch) in brain_core::chunk_text(content, brain_core::CHUNK_TARGET_TOKENS).iter().enumerate() {
            owner.push((ni, ci));
            texts.push(ch.clone());
        }
    }
    let eng = brain_embed::EmbeddingEngine::from_env();
    let budget = eng.batch_timeout(texts.len());
    let vecs = match tokio::time::timeout(budget, eng.embed_batch_partial(texts)).await {
        Ok(v) => v,
        Err(_) => {
            eprintln!("reindex: embedding budget of {:?} expired for {} chunk(s); stored vectors are preserved and the rest stay NULL", budget, owner.len());
            return out;
        }
    };
    let total_chunks = owner.len();
    let mut broken: HashMap<usize, ()> = HashMap::new();
    let mut filled = 0usize;
    for ((ni, ci), v) in owner.into_iter().zip(vecs) {
        if broken.contains_key(&ni) { continue; }
        let Some(v) = v else {
            broken.insert(ni, ()); // everything from here on would be misaligned
            continue;
        };
        if v.len() != brain_core::EMBEDDING_DIM {
            eprintln!("reindex: chunk {} of note {} has dim {}, expected {} — truncating this note's vectors", ci, notes[ni].0, v.len(), brain_core::EMBEDDING_DIM);
            broken.insert(ni, ());
            continue;
        }
        let slot = out.entry(notes[ni].0.clone()).or_default();
        // `ci` is the index into this note's own chunk list, so `slot.chunks` and
        // `slot.vectors` stay aligned and each vector keeps its source text.
        if slot.vectors.len() == ci { slot.vectors.push(v); filled += 1; } else { broken.insert(ni, ()); }
    }
    // Fill the provenance list from the same content the pass chunked. A vector at
    // position i came from chunk i of that content, by construction above.
    for (path, content) in notes {
        if let Some(ne) = out.get_mut(path) {
            ne.chunks = brain_core::chunk_text(content, brain_core::CHUNK_TARGET_TOKENS)
                .into_iter().take(ne.vectors.len()).collect();
        }
    }
    if filled < total_chunks {
        eprintln!("reindex: embedded {}/{} chunk vectors", filled, total_chunks);
    }
    out
}

#[allow(dead_code)]
fn detect_project() -> String {
    // try git toplevel basename, else cwd basename
    if let Ok(out) = std::process::Command::new("git").args(["rev-parse","--show-toplevel"]).output() {
        if out.status.success() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    if let Some(name) = std::path::Path::new(trimmed).file_name().and_then(|n| n.to_str()) {
                        return name.to_string();
                    }
                }
            }
        }
    }
    std::env::current_dir().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_else(|| "brain".into())
}

/// Rebuilds the whole index, embedding before any write transaction opens.
///
/// `brain reindex --all` used to `DELETE FROM chunks` and re-insert every row
/// with a zero vector, so running it destroyed the entire vector index. It is
/// now non-destructive: unchanged chunks keep their vectors, changed ones come
/// back as `NULL`, and the embedding pass runs first so as many as possible come
/// back hydrated.
///
/// Three properties this function is responsible for:
/// - **snapshot before embed.** `all_notes` is read *before* the network pass, and
///   each [`NoteEmbed`] carries the chunk texts its vectors were computed from.
///   Embedding the whole corpus takes minutes against a serial Ollama; a
///   `brain_store` landing in that window would otherwise get the old text's
///   vectors written onto the new text's chunks. `chunks_sync` drops those and
///   reports them as `diverged`, so the run says so instead of lying.
/// - **lock around the embed pass.** The MCP server's background queue embeds on
///   the same chunks. Both take the advisory lock in `_meta`, so a backfill is
///   not run twice. The lock is released before the write transaction opens.
/// - **honest reporting.** `preserved`, `rehydrated`, `null` and `diverged` are all
///   printed. A report that only said "done" is how a half-rebuilt index looked
///   healthy for a month.
async fn run_reindex(db: &str, no_embed: bool) -> Result<()> {
    let store = Store::open(db)?;
    // Snapshot the corpus first. Everything the embed pass reads comes from here.
    let notes = store.all_notes()?;
    let embeds = if no_embed {
        eprintln!("reindex: --no-embed, keeping stored vectors and leaving new chunks NULL");
        std::collections::HashMap::new()
    } else {
        // Embed before the write transaction opens: the store is a synchronous
        // rusqlite handle, and awaiting a network batch with a write lock held
        // would block every other reader for the duration.
        let owner = brain_store::embed_lock_owner("reindex");
        let locked = store.try_acquire_embed_lock(&owner, 3600)?;
        if !locked {
            let holder = store.embed_lock_holder()?.unwrap_or_else(|| "unknown".into());
            anyhow::bail!(
                "another embed holds the lock ({}), so this reindex would duplicate its work; \
                 retry when it finishes. Nothing was written.",
                holder
            );
        }
        let embeds = embed_all_notes(&notes).await;
        let _ = store.release_embed_lock(&owner);
        embeds
    };
    let (cnt, stats) = store.reindex_all_with(&embeds)?;
    outln!(
        "REINDEX_DONE notes={} chunks={} embedded={} preserved={} rehydrated={} null={} diverged={} stale_reused={} unmatched={}",
        cnt, stats.total, stats.embedded, stats.preserved, stats.rehydrated, stats.nulls, stats.diverged,
        stats.stale_reused, stats.unmatched
    );
    if stats.nulls > 0 {
        outln!("REINDEX_PARTIAL {} chunk(s) have no vector — rerun without --no-embed while Ollama is reachable", stats.nulls);
    }
    if stats.diverged > 0 {
        outln!(
            "REINDEX_DIVERGED {} chunk vector(s) were computed from text the note no longer has and were NOT \
             applied; those notes were edited during this run. Re-run `brain reindex --all` to embed them.",
            stats.diverged
        );
    }
    if stats.stale_reused > 0 {
        outln!(
            "REINDEX_STALE {} chunk(s) kept a vector that is slightly out of date (text similar enough to reuse); \
             the log names them. Rerun to refresh if that matters.",
            stats.stale_reused
        );
    }
    Ok(())
}

/// Whether the session hook embeds the sections it appends.
///
/// `BRAIN_HOOK_EMBED=0` turns it off. The embed is now a *diff* (only the sections
/// that grew), so it costs one request per event against a serial Ollama — but an
/// operator who does not want their agent's tool output in the semantic index, or
/// who runs the hook on a machine that should not talk to Ollama at all, should not
/// have to pay for it. The note is written either way, so capture is never
/// conditional on this.
const HOOK_EMBED_ENV: &str = "BRAIN_HOOK_EMBED";

fn hook_embed_enabled() -> bool {
    match std::env::var(HOOK_EMBED_ENV) {
        Ok(v) => !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"),
        Err(_) => true,
    }
}

/// The dedup marker for a rendered section.
///
/// X-01. **The trailing newline is load-bearing.** The store decides "already
/// captured" by asking whether the note contains this marker, and the marker used
/// to be the bare `id=<key>`. That is a prefix test over free text, so any event
/// id that is a prefix of another one matched the wrong section: with events
/// `d11` and `d1`, the note already containing `id=d11` also contains `id=d1`, and
/// `d1` was silently dropped as a duplicate — a lost session event with no error
/// and no log line, reachable from a *sequential* run and not only a concurrent
/// one.
///
/// Every section format ends the heading with `id=<key>` followed by a newline, so
/// including it makes the marker a whole token: `id=d1\n` cannot match inside
/// `id=d11\n`. Derived from the section's own first line rather than restated, so
/// a change to the heading format cannot silently un-anchor it.
fn dedup_marker(section: &str, dedup_key: &str) -> String {
    let head = section.lines().next().unwrap_or_default();
    if head.ends_with(&format!("id={dedup_key}")) {
        return format!("id={dedup_key}\n");
    }
    // A heading that no longer ends with the id: fall back to the bare token
    // rather than to a marker that can never match, which would silently disable
    // deduplication entirely.
    eprintln!("hook: section heading for id={dedup_key} does not end with the id; dedup marker unanchored");
    format!("id={dedup_key}")
}

/// Next rotated part number for a day's session note: the highest existing
/// `{date}-N` plus one, starting at 2.
///
/// X-01: this moved into [`Store::note_append_section`], which computes it inside
/// the same `BEGIN IMMEDIATE` transaction that performs the append. It used to be
/// a separate read here, which meant two hooks could both read "no parts exist",
/// both pick `-2`, and the second would overwrite the first's section. The
/// highest-plus-one rule is unchanged; what changed is that it is now evaluated
/// against a snapshot no other writer can be modifying.
fn rotated_header<'a>(
    project: &'a str,
    date: &'a str,
    base: &'a str,
    section: &'a str,
) -> impl Fn(u32, &str) -> String {
    move |part, limit_error| {
        format!(
            "# Sessão {project} {date} (parte {part})\n\nContinuação de [[{base}]], que atingiu o limite de \
             escrita: {limit_error}\n\n{}",
            fit_section(section)
        )
    }
}

/// Truncates `section` until it fits under both write limits on its own.
///
/// The last resort of the hook, and the reason it never refuses: a single event
/// whose payload is larger than the whole note budget cannot be rotated away, since
/// rotating produces a new note that has to hold it too. The event is still
/// captured — truncated, with the fact stated in the text — because a silently
/// dropped session entry is worse than a shortened one.
fn fit_section(section: &str) -> String {
    if brain_core::validate_content_limits(section).is_ok() {
        return section.to_string();
    }
    // Leave room for the note header and the rotation notice around it.
    let budget = brain_core::MAX_CONTENT_BYTES / 2;
    let mut cut = section.len().min(budget);
    while cut > 0 {
        // Back off to a char boundary rather than splitting a UTF-8 sequence.
        while cut > 0 && !section.is_char_boundary(cut) {
            cut -= 1;
        }
        let candidate = &section[..cut];
        if brain_core::validate_content_limits(candidate).is_ok() {
            return format!("{candidate}\n\n[truncated by the brain hook: this event exceeded the note size limit]\n");
        }
        cut -= 1;
    }
    "[truncated by the brain hook: this event exceeded the note size limit]\n".to_string()
}

async fn hook_handle(event: String, project: String, payload: Option<String>, db: String) -> Result<()> {
    use chrono::Utc;
    use fs2::FileExt;
    use std::fs::OpenOptions;
    use std::io::{Read, Write};

    // strict event validation (clap value_parser already restricts, double-check)
    if !["session-start", "tool-result", "session-end"].contains(&event.as_str()) {
        anyhow::bail!("invalid event '{}': expected session-start|tool-result|session-end", event);
    }
    if project.trim().is_empty() {
        anyhow::bail!("project required: pass --project <name>");
    }
    // The project name becomes a path component. It arrived unsanitised, so
    // `--project ../../etc` used to produce the note path `sessoes/../../etc/<date>`
    // — a namespace escape that also makes `brain_export` write outside its
    // directory. One component, no separators, no climb.
    let project = sanitize_relative_path(&project)?;
    let now = Utc::now().to_rfc3339();
    let date = Utc::now().format("%Y-%m-%d").to_string();
    let payload_raw = payload.clone().unwrap_or_else(|| "{}".into());
    let payload_val: serde_json::Value = serde_json::from_str(&payload_raw).unwrap_or(serde_json::Value::Null);
    // dedup key: payload id if present, else hash(event+project+payload_raw)
    let raw_id = payload_val.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let dedup_key = if raw_id.is_empty() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        format!("{}|{}|{}", event, project, payload_raw).hash(&mut h);
        format!("{:x}", h.finish())
    } else { raw_id };
    // spool path: XDG_RUNTIME_DIR/brain/hook-spool.jsonl or /tmp/brain/hook-spool.jsonl
    let spool = hook_spool_path();
    if let Some(parent) = spool.parent() { std::fs::create_dir_all(parent)?; }
    let line = serde_json::json!({"event": event, "project": project, "payload": payload_val, "ts": now, "id": dedup_key});
    let line_str = serde_json::to_string(&line)?;
    let mut file = OpenOptions::new().create(true).append(true).read(true).open(&spool)?;
    file.lock_exclusive()?;
    let mut existing = String::new();
    {
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(0))?;
        file.read_to_string(&mut existing)?;
        if existing.contains(&format!("\"id\":\"{}\"", dedup_key)) {
            fs2::FileExt::unlock(&file)?;
            outln!("hook deduplicated id={}", dedup_key);
            return Ok(());
        }
    }
    writeln!(file, "{}", line_str)?;
    fs2::FileExt::unlock(&file)?;

    let pretty = serde_json::to_string_pretty(&payload_val).unwrap_or_default();
    let section = match event.as_str() {
        "session-start" => format!("## {} session-start id={}\n\nProject: {}\nTime: {}\n\n```json\n{}\n```\n", &now[11..16.min(now.len())], dedup_key, project, now, pretty),
        "tool-result" => {
            let tool = payload_val.get("tool").or_else(|| payload_val.get("name")).and_then(|v| v.as_str()).unwrap_or("unknown");
            format!("## {} tool-result {} id={}\n\n```json\n{}\n```\n", &now[11..16.min(now.len())], tool, dedup_key, pretty)
        },
        _ => format!("## {} session-end id={}\n\nProject: {}\nTime: {}\n\n```json\n{}\n```\n", &now[11..16.min(now.len())], dedup_key, project, now, pretty),
    };

    // --- phase 1: everything that needs the database, and no network ---------
    // `Store` is `!Sync` (a `RefCell` in rusqlite's statement cache), so no
    // `&Store` may be live across the embed `.await`. Every handle is opened in its
    // own scope and dropped before the next phase.
    //
    // X-01: the append below is a single `note_append_section` call, which reads
    // the note and writes it back inside one `BEGIN IMMEDIATE` transaction. It
    // used to be three steps here — `note_get`, build the content, `note_upsert` —
    // with no lock between them, so two overlapping hooks both read the same
    // content and the second write dropped the first hook's section. This is the
    // guarantee the code now makes: **every event is appended, whatever the
    // interleaving**, because no interleaving can be observed between the read and
    // the write. It does *not* serialise agents: the lock is SQLite's write lock,
    // held for one `SELECT` and one `UPDATE`, and never across the embed.
    let (full, content, todo) = {
        let store = Store::open(&db)?;
        let base = format!("sessoes/{}/{}", project, date);
        let header = format!("# Sessão {} {}\n", project, date);
        let appended = store.note_append_section(
            &base,
            "sessoes",
            None,
            &section,
            &dedup_marker(&section, &dedup_key),
            &header,
            &rotated_header(&project, &date, &base, &section),
            None,
            &[],
        )?;
        if !appended.appended {
            outln!("hook store deduplicated id={}", dedup_key);
        }
        if let Some(why) = &appended.rotated_because {
            // W-03.1: rotate, never refuse. The hook's caller
            // (`hooks/brain-hook.py`) runs it with a 30s timeout, so a rejection
            // would be a silently broken hook — strictly worse than the bug it
            // fixes. The day's note is left exactly as it is (never truncated,
            // never dropped) and the new section starts a new note that links back
            // to it, so no section is lost.
            eprintln!(
                "hook: {base} is at the write limit, so this event starts {}. Nothing is lost: the previous \
                 note keeps every section it had. ({why})",
                appended.path
            );
        }
        let full = appended.path;
        let content = appended.content;
        // W-03.2: embed the *delta*, not the accumulated document. The note
        // grows by one section per event, so embedding all of it every time is
        // quadratic — measured at 76 events x up to 76 chunks each, ~3,000
        // embedding requests for one session, ~4 s per event against a serial
        // Ollama. `chunks_needing_embedding` is a diff: it returns only the
        // chunks that have no vector for their current text, so the cost is one
        // request per new section.
        let todo = if appended.appended { store.chunks_needing_embedding(&full, &content)? } else { Vec::new() };
        // SH-02: sessoes/shared/* auto-link erp+mobile (ADR-001 single-source,
        // ADR-003 só prefixo shared/)
        if full.starts_with("sessoes/shared/") {
            for pname in ["erp", "mobile"] {
                if store.project_get(pname)?.is_none() {
                    let _ = store.project_create(pname, "");
                }
                let _ = store.note_link_project(&full, pname);
            }
        }
        (full, content, todo)
    };

    // --- phase 2: embed, with no database handle in scope --------------------
    let embed = hook_embed_enabled();
    if !embed {
        eprintln!("hook: {HOOK_EMBED_ENV} is off, so this event is stored FTS-only (no vector requested)");
    }
    let fresh = if embed && !todo.is_empty() {
        let texts: Vec<String> = todo.iter().map(|(_, t)| t.clone()).collect();
        let vecs = embed_chunks(&texts, &full).await;
        let mut fresh = brain_store::FreshVectors::new();
        for ((idx, text), v) in todo.iter().zip(vecs.into_iter()) {
            if let Some(v) = v {
                fresh.insert(*idx, (text.clone(), v));
            }
        }
        fresh
    } else {
        brain_store::FreshVectors::new()
    };

    // --- phase 3: write the vectors back -------------------------------------
    {
        let store = Store::open(&db)?;
        if store.note_get(&full)?.is_none() {
            anyhow::bail!("hook: {full} vanished between write and chunk sync");
        }
        let Some(nid) = store.note_id(&full)? else {
            anyhow::bail!("hook: {full} has no row id");
        };
        let stats = sync_note_chunks(
            &store,
            ChunkSyncInput {
                note_id: nid,
                path: &full,
                layer: "sessoes",
                scope: None,
                content: &content,
                project_id: None,
                tags: &[],
                // No explicit snapshot: `chunks_sync` reads the note's current rows
                // itself and prefers them, and `note_upsert` does not delete them —
                // which is what the old comment here claimed it did, and the reason
                // this snapshot was believed to be load-bearing.
                snapshot: brain_store::ChunkSnapshot::new(),
                fresh,
            },
        )?;
        eprintln!(
            "hook: {} chunks={} embedded={} NULL={} preserved={} rehydrated={} owed_before={}",
            full, stats.total, stats.embedded, stats.nulls, stats.preserved, stats.rehydrated, todo.len()
        );
    }

    // session-start inject: brain_search scope global para inject
    if event == "session-start" {
        let store = Store::open(&db)?;
        let res_global = store.search("padrões melhores práticas lições", None, Some("regras"), Some("global"), None, None, 3, false).unwrap_or_default();
        if !res_global.is_empty() {
            outln!("--- Brain context (global) ---");
            for r in &res_global { outln!("{} [{}] {}", r.path, r.score, r.snippet.chars().take(120).collect::<String>()); }
        }
        let res_proj = store.search(&format!("regras {}", project), None, Some("regras"), Some("projetos"), None, None, 3, false).unwrap_or_default();
        if !res_proj.is_empty() {
            outln!("--- Brain context (projetos) ---");
            for r in &res_proj { outln!("{} [{}] {}", r.path, r.score, r.snippet.chars().take(120).collect::<String>()); }
        }
        if res_global.is_empty() && res_proj.is_empty() {
            outln!("INJECT: (no context found)");
        }
    }
    outln!("hook ok event={} project={} note={} spool={}", event, project, full, spool.display());
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let db = cli.db.clone();
    // stdio transport fallback via env
    let transport = std::env::var("BRAIN_TRANSPORT").unwrap_or_default();
    if transport == "stdio" {
        // if cmd is ping or not serve, handle stdio mode for MCP
        // but keep normal flow for other cmds; only intercept ServeMcp or Ping
        if matches!(cli.cmd, Cmd::Ping) {
            brain_mcp::serve_stdio(db).await?;
            return Ok(());
        }
    }
    match cli.cmd {
        Cmd::Ping => outln!("pong"),
        Cmd::Store { layer, path, content, scope, project, tags, pinned, expires_at } => {
            // The shared write rule — the same one the MCP handlers and the hook
            // use, so a limit cannot be enforced on three paths out of four.
            let fp = brain_mcp::validate_note_write(&layer, &path, scope.as_deref(), &content)
                .map_err(|e| anyhow::anyhow!("INVALID_PARAMS: {e}"))?;
            let store = Store::open(&db)?;
            let tags_v: Vec<String> = tags.unwrap_or_default().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let project_id = if let Some(pname) = project {
                let proj = store.project_get(&pname)?.or_else(|| Some(store.project_create(&pname,"").unwrap()));
                proj.map(|p| p.id)
            } else { None };
            let nid = store.note_upsert(&fp, &layer, scope.as_deref(), &content, project_id, &tags_v, pinned, expires_at.as_deref())?;
            let chunks = brain_core::chunk_text(&content, brain_core::CHUNK_TARGET_TOKENS);
            // The CLI embeds inline rather than queueing, and that is deliberate:
            // this is a one-shot process, so a background task would be killed with
            // the process and the note would never get a vector at all. The
            // blocking cost is bounded by `MAX_CHUNKS` (~26s worst case), and the
            // server paths — which are long-lived — do use the queue.
            let vecs = embed_chunks(&chunks, &fp).await;
            let fresh = brain_store::NoteEmbed { chunks: chunks.clone(), vectors: vecs.into_iter().flatten().collect() };
            let stats = sync_note_chunks(&store, ChunkSyncInput { note_id: nid, path: &fp, layer: &layer, scope: scope.as_deref(), content: &content, project_id, tags: &tags_v, snapshot: brain_store::ChunkSnapshot::new(), fresh: fresh.fresh_vectors() })?;
            outln!("ok — {} (chunks={} embedded={} without_embedding={} preserved={} rehydrated={} diverged={})", fp, stats.total, stats.embedded, stats.nulls, stats.preserved, stats.rehydrated, stats.diverged);
        }
        Cmd::Read { layer, path, scope } => {
            let fp = full_path(&layer, &path, scope.as_deref())?;
            let store = Store::open(&db)?;
            if let Some(n) = store.note_get(&fp)? { outln!("# {}\n\n{}", n.path, n.content); } else { eprintln!("NOT_FOUND: {}", fp); std::process::exit(1); }
        }
        Cmd::Search { query, layer, scope, project, tag, top_k, explain } => {
            let store = Store::open(&db)?;
            let embed = brain_embed::EmbeddingEngine::from_env();
            let qvec = embed.embed(&query).await.ok();
            let res = store.search(&query, qvec.as_deref(), layer.as_deref(), scope.as_deref(), project.as_deref(), tag.as_deref(), top_k, explain)?;
            if explain {
                let out = serde_json::json!({"results": res.iter().map(|r| {
                    let mut v = serde_json::json!({"path": r.path, "layer": r.layer, "scope": r.scope, "score": r.score, "snippet": r.snippet, "project": r.project, "tags": r.tags});
                    if let Some(e) = &r.explain {
                        v["explain"] = serde_json::json!({"rrf_vec": e.rrf_vec, "rrf_fts": e.rrf_fts, "rrf_entity": e.rrf_entity, "rrf_graph": e.rrf_graph, "authority": e.authority, "score": e.score, "rank_vec": e.rank_vec, "rank_fts": e.rank_fts, "rank_entity": e.rank_entity, "rank_graph": e.rank_graph});
                    }
                    v
                }).collect::<Vec<_>>(), "total": res.len()});
                outln!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                let out = serde_json::json!({"results": res.iter().map(|r| serde_json::json!({"path": r.path, "layer": r.layer, "scope": r.scope, "score": r.score, "snippet": r.snippet, "project": r.project, "tags": r.tags})).collect::<Vec<_>>(), "total": res.len()});
                outln!("{}", serde_json::to_string_pretty(&out)?);
            }
        }
        Cmd::Delete { path } => {
            if let Err(e) = sanitize_relative_path(&path) {
                eprintln!("INVALID_PARAMS: {}", e);
                std::process::exit(2);
            }
            let store = Store::open(&db)?;
            if store.note_delete(&path)? { outln!("ok deleted {}", path); } else { eprintln!("NOT_FOUND"); std::process::exit(1); }
        }
        Cmd::Recent { top_k } => {
            let store = Store::open(&db)?;
            let rows = store.recent(top_k)?;
            outln!("{}", serde_json::to_string_pretty(&serde_json::json!({"recent": rows}))?);
        }
        Cmd::Status => {
            let store = Store::open(&db)?;
            outln!("notes={} chunks={} projects={} db={}", store.count_notes()?, store.count_chunks()?, store.project_list()?.len(), db);
            // Embedding coverage + Ollama health. `notes`/`chunks` alone stayed
            // healthy while 99.6% of chunk vectors were BLOB-of-zeros scoring
            // 0.0 against every query, so this block is the regression alarm.
            let cov = store.embedding_coverage()?;
            let eng = brain_embed::EmbeddingEngine::from_env();
            outln!("embedding: chunks_total={} embedded={} without_embedding={} zero_vector={} coverage_pct={}",
                cov.total, cov.embedded, cov.without_embedding, cov.zero_vector, cov.coverage_pct);
            outln!("ollama: reachable={} model={}", eng.health_check().await, eng.model);
            // Y-05. The embedding queue, or the part of it a *separate process*
            // can see.
            //
            // A dead letter used to be discoverable only through a message that
            // said "run `brain status`", and this command printed no queue
            // block at all — so the pointer led nowhere, and the queue lives in a
            // systemd service whose stderr goes to a journal nobody reads. The
            // queue's work list and its dead-letter counter are **in-memory state
            // of the `serve-mcp` process**, not rows in the database, so this
            // process cannot read them no matter what it prints. Claiming
            // otherwise here would be a lie an operator acts on.
            //
            // What *is* in the database is the cross-process embed lock, and it
            // is the half that matters from outside: it is what "another embed is
            // running, and for how long" means, and it is the only way to tell a
            // queue that is deferred from one that is idle. The rest is named
            // explicitly below, with where it can actually be read.
            let lock = store.embed_lock_state();
            outln!(
                "queue: embed_lock_holder={} embed_lock_age_s={} embed_lock_expires_in_s={}",
                lock.holder.as_deref().unwrap_or("none"),
                lock.age_secs.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                lock.expires_in_secs.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
            );
            if lock.holder.is_some() {
                outln!(
                    "queue: an embed pass is in progress and holds the cross-process lock. A note that stays \
                     without a vector while this runs is waiting on it, not lost."
                );
            }
            outln!(
                "queue: pending_len/ready_len/dead_lettered/last_drain live in the running server process, \
                 not in this database, so `brain status` cannot show them. Read them with the brain_status MCP \
                 tool (which queries the live server), or `journalctl -u brain-mcp` for the dead-letter line. \
                 Their footprint in the database is embedding.without_embedding above: a chunk stuck NULL with \
                 a note that no longer moves is the shape to look for."
            );
        }
        Cmd::Reindex { all: _, no_embed } => {
            let lock = reindex_lock();
            {
                let guard = lock.lock().unwrap();
                if *guard { outln!("REINDEX_IN_PROGRESS"); std::process::exit(2); }
            }
            *lock.lock().unwrap() = true;
            // The flag is cleared on the error path too: leaving it set would
            // wedge every later reindex in this process.
            let result = run_reindex(&db, no_embed).await;
            *lock.lock().unwrap() = false;
            result?;
        }
        Cmd::Checkpoints { limit } => {
            let store = Store::open(&db)?;
            let cps = store.checkpoints(limit)?;
            outln!("{}", serde_json::to_string_pretty(&cps)?);
        }
        Cmd::Restore { id } => {
            let store = Store::open(&db)?;
            if store.restore_audit(id)? { outln!("ok restored {}", id); } else { eprintln!("NOT_FOUND audit {}", id); std::process::exit(1); }
        }
        Cmd::Backup { to } => {
            // W-01: the destination is contained to `BRAIN_EXPORT_ROOT` (or the
            // `{db}.bak` default). The CLI is local, so this is defence in depth
            // rather than the primary control — but it is the same binary the
            // systemd units run, and "local" is a property of the caller, not of the
            // process.
            let dst = brain_mcp::fs_guard::backup_file(&db, to.as_deref())
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            std::fs::copy(&db, &dst)?; outln!("backup -> {}", dst.display());
        }
        Cmd::Export { to, force } => {
            let dir = brain_mcp::fs_guard::export_dir(Some(&to))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if dir.exists() && !force { anyhow::bail!("export dir exists, use --force"); }
            std::fs::create_dir_all(&dir)?;
            // X-05.3: re-check ownership now the directory exists. `export_dir`
            // could only skip the check when the root was absent, and in `/tmp`
            // another local user can create it inside that window.
            brain_mcp::fs_guard::assert_root_usable(&brain_mcp::fs_guard::export_root())?;
            let store = Store::open(&db)?;
            let recent = store.recent(10000)?;
            let mut written = 0usize;
            for (path, _layer, _scope, content) in recent {
                let fp = brain_mcp::fs_guard::note_file_within(&dir, &path)?;
                if let Some(parent) = fp.parent() { std::fs::create_dir_all(parent)?; }
                std::fs::write(fp, content)?;
                written += 1;
            }
            outln!("export -> {} ({} note(s))", dir.display(), written);
        }
        Cmd::ForgetSweep { dry_run } => {
            let store = Store::open(&db)?;
            let expired = store.forget_sweep(dry_run)?;
            if dry_run { outln!("dry-run would delete {}: {:?}", expired.len(), expired); } else { outln!("deleted {}: {:?}", expired.len(), expired); }
        }
        Cmd::Serve { port } => {
            // Armed before the bind, for the same reason `server start` arms it
            // before its import and `serve_rmcp_sse` before its boot recovery: this
            // **call** installs the SIGTERM disposition on the caller's thread, and
            // only the future it returns is a wait. The bind below is an `.await`,
            // so a SIGTERM landing in that window must be *recorded* — with the
            // handler armed it is, and the wait is already resolved when first
            // polled. Armed later, the same SIGTERM is the default action and the
            // process dies by signal.
            //
            // `with_graceful_shutdown` is what makes that wait reachable at all:
            // without it there is no wait to reach, and `systemctl stop` on
            // `brain-viewer.service` kills the process mid-request.
            //
            // One caveat, inherited from the helper rather than introduced here:
            // only SIGTERM is armed eagerly. Its `ctrl_c()` arm sits *inside* the
            // returned `async move` block, and an `async fn` body does not run until
            // the future is polled, so SIGINT is still registered on first poll and
            // a Ctrl-C inside this window would take the default action. That is
            // true of `serve-mcp` and `server start` too, and it is why this says
            // SIGTERM rather than "either signal".
            let shutdown = brain_mcp::rmcp_service::shutdown_on_sigint_or_sigterm();
            let app = brain_web::router(db.clone());
            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
            // Y-02. Printed **after** the bind, so the line means "this port is
            // live" and not "this port was intended". It was printed before, which
            // announced a URL for a server that then died with `Address already in
            // use` — wrong for an operator reading the log, and worse for the test
            // that treats the announcement as its readiness signal: a squatter on
            // the port would satisfy the announcement while the bind failed. An
            // `?` that never announced anything would have been the honest version.
            outln!("viewer http://0.0.0.0:{}/ — /api/status|search|read|list", port);
            axum::serve(listener, app).with_graceful_shutdown(shutdown).await?;
        }
        Cmd::ServeMcp { port } => {
            if std::env::var("BRAIN_TRANSPORT").unwrap_or_default() == "stdio" {
                brain_mcp::serve_stdio(db).await?;
            } else {
                brain_mcp::rmcp_service::serve_rmcp_sse(db.clone(), port).await?;
            }
        }
        Cmd::Hook { event, project, payload } => {
            hook_handle(event, project, payload, db).await?;
        }
        Cmd::Setup { target, mcp_port, viewer_port, brain_dir, dir, force, dry_run } => {
            setup::run_setup(&setup::SetupOpts { target, mcp_port, viewer_port, db: db.clone(), brain_dir, dir, force, dry_run })?;
        }
        Cmd::Server { sub } => match sub {
            ServerCmd::Start { port, vault, old_index } => {
                let port = port.unwrap_or_else(default_mcp_port);
                // Armed first, on purpose: the import below is synchronous and
                // can take minutes, and a signal that arrives during it has to be
                // *recorded* rather than killing the process. See
                // `shutdown_on_sigint_or_sigterm`.
                let shutdown = brain_mcp::rmcp_service::shutdown_on_sigint_or_sigterm();
                // AC3, before the listener exists. The import reads a directory of
                // markdown and writes a database, archiving the source first — see
                // `legacy_import` for why that has to happen before, not after, and
                // why a failed archive stops the import instead of degrading it.
                //
                // Embedding is left to the queue: the imported chunks are written
                // `NULL` and `serve_rmcp_sse_with`'s boot recovery re-queues them,
                // so a start with Ollama down still starts.
                let imported = legacy_import::import_legacy(&db, std::path::Path::new(&vault), &old_index)?;
                let report = &imported.report;
                if let Some(b) = &report.backup {
                    outln!("server: legacy vault archived to {}", b.display());
                }
                if let Some(idx) = &report.legacy_index_present {
                    outln!(
                        "server: WARNING legacy index {} exists but its contents are NOT imported \
                         (the Rust store reads the vault's markdown only); it is left untouched on disk",
                        idx.display()
                    );
                }
                if report.notes > 0 {
                    outln!(
                        "server: imported {} legacy note(s) (chunks={} without_embedding={})",
                        report.notes, report.chunks, report.without_embedding
                    );
                }
                if std::env::var("BRAIN_TRANSPORT").unwrap_or_default() == "stdio" {
                    // The wait built above is armed but, on this branch, nothing
                    // ever polled it — and arming SIGTERM without acting on it is
                    // *worse* than never arming it. The disposition is now tokio's
                    // rather than the default, so `systemctl stop` is recorded and
                    // then ignored: the process keeps reading stdin and serving,
                    // and the unit only goes away when `TimeoutStopSec` escalates to
                    // `SIGKILL`. `serve_viewer_shutdown.rs` calls that half-fix
                    // "worse than the bug" and this branch is that half-fix.
                    //
                    // Raced rather than dropped, so both outcomes are honest: EOF
                    // on stdin still returns cleanly, and a signal ends the
                    // process. `serve_rmcp_sse_with` receives the same future by
                    // move in the other arm; `select!` and the move are exclusive.
                    tokio::select! {
                        r = brain_mcp::serve_stdio(db.clone()) => r?,
                        () = shutdown => {
                            outln!("server: shutting down on signal");
                            // `exit`, not `return`, and the reason is specific
                            // rather than stylistic. `serve_stdio` reads stdin
                            // through `tokio::io::stdin`, which parks a
                            // `spawn_blocking` task on the read, and a runtime's
                            // shutdown **waits** for its blocking tasks. Returning
                            // from `main` drops the runtime, so a read on a pipe
                            // the client still holds open never returns and the
                            // process does not exit — a server that has stopped
                            // serving and still refuses to die, which is the same
                            // `TimeoutStopSec`-to-`SIGKILL` outcome the race above
                            // was added to remove. Measured before this line: the
                            // message printed, the process stayed alive.
                            //
                            // Skipping the async drop is safe *here* specifically:
                            // this arm has no database handle, no embedding queue
                            // and no listener to tear down — the import
                            // short-circuited or finished before it, and
                            // `serve_stdio` opens a `Store` per request and drops
                            // it within the request. `outln!` above already
                            // reaches for `process::exit` on a broken pipe, so this
                            // is the same choice this file already makes for the
                            // same reason.
                            std::process::exit(0);
                        }
                    }
                } else {
                    // Not `serve_rmcp_sse`: that builds its own wait, and this arm
                    // needs it armed *before* the import above — an import that takes
                    // minutes would otherwise run with no SIGTERM handler installed.
                    // Both are the same signal set; only the construction point differs.
                    brain_mcp::rmcp_service::serve_rmcp_sse_with(db.clone(), port, brain_mcp::global_queue(), shutdown)
                        .await?;
                }
            }
        },
        Cmd::Migrate { vault, old_index, no_embed } => {
            // B6: the archive is taken by the shared import path, so the explicit
            // command and `server start` cannot drift — an operator who chose the
            // safe, explicit route gets the same safety as the automatic one.
            let imported = legacy_import::import_legacy(&db, std::path::Path::new(&vault), &old_index)?;
            if let Some(b) = &imported.report.backup {
                outln!("migrated: legacy vault archived to {}", b.display());
            }
            if let Some(idx) = &imported.report.legacy_index_present {
                outln!(
                    "migrated: WARNING legacy index {} exists but its contents are NOT imported \
                     (the Rust store reads the vault's markdown only); it is left untouched on disk",
                    idx.display()
                );
            }
            let store = Store::open(&db)?;
            // Hydrate during the import rather than after it: a legacy import is
            // a bulk one-shot event where Ollama is up by definition, and it is
            // the only moment the whole corpus is guaranteed to be in hand. The
            // import used to write a zero vector per chunk, leaving a freshly
            // migrated vault with a 0% usable vector index.
            let notes: Vec<(String, String)> =
                imported.staged.iter().map(|(_, f, _, _, c, _, _)| (f.clone(), c.clone())).collect();
            let embeds = if no_embed {
                eprintln!("migrate: --no-embed, imported notes stay FTS-only");
                std::collections::HashMap::new()
            } else {
                embed_all_notes(&notes).await
            };
            let (mut chunks_total, mut embedded, mut nulls) = (0usize, 0usize, 0usize);
            for (nid, full, layer, scope, content, pid, tags) in &imported.staged {
                let fresh = embeds
                    .get(full)
                    .map(|ne| ne.fresh_vectors())
                    .unwrap_or_default();
                let st = store.chunks_sync(*nid, full, layer, scope.as_deref(), content, *pid, tags, &fresh, &brain_store::ChunkSnapshot::new())?;
                chunks_total += st.total;
                embedded += st.embedded;
                nulls += st.nulls;
            }
            outln!("migrated {} vault files (chunks={} embedded={} without_embedding={})", imported.staged.len(), chunks_total, embedded, nulls);
            if nulls > 0 {
                outln!("MIGRATE_PARTIAL {} chunk(s) have no vector — run `brain reindex --all` with Ollama reachable to backfill", nulls);
            }
        }
        Cmd::Project { sub } => {
            let store = Store::open(&db)?;
            match sub {
                ProjectCmd::Create { name, description } => { let pr = store.project_create(&name, &description)?; outln!("ok project {} id={}", pr.name, pr.id); }
                ProjectCmd::List => { let ps = store.project_list()?; outln!("{}", serde_json::to_string_pretty(&ps)?); }
                ProjectCmd::Delete { name } => { if store.project_delete(&name)? { outln!("ok deleted {}", name); } else { eprintln!("NOT_FOUND"); } }
                ProjectCmd::Notes { name } => { let notes = store.project_notes(&name)?; outln!("{}", serde_json::to_string_pretty(&notes)?); }
                ProjectCmd::Link { note_path, project } => { store.note_link_project(&note_path, &project)?; outln!("ok linked {} -> {}", note_path, project); }
                ProjectCmd::Unlink { note_path, project } => { if store.note_unlink_project(&note_path, &project)? { outln!("ok unlinked {} -/-> {}", note_path, project); } else { outln!("no link"); } }
            }
        }
    }
    Ok(())
}
