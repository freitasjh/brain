pub mod embed_queue;
pub mod fs_guard;
pub mod rmcp_service;

use brain_store::Store;
use std::sync::{Arc, Mutex};

pub use embed_queue::{EmbedQueue, QueueOutcome, global_queue};
pub use fs_guard::{DEFAULT_EXPORT_ROOT, EXPORT_ROOT_ENV};


/// Embeds a note's chunks, degrading to `None` per chunk rather than failing.
///
/// Shared by the debug REST routes in this module and by the real MCP protocol
/// handlers in [`rmcp_service`], because those two paths used to disagree about
/// embedding: the REST route embedded, the protocol route — the one AI agents
/// actually call — wrote a zero vector for every chunk. One implementation, one
/// behaviour.
///
/// Three layers of tolerance, so that one bad chunk, one slow request or one
/// dead socket each cost only themselves:
/// 1. per chunk — a failing chunk is `None` while its siblings still embed;
/// 2. per wave — the batch budget scales with the number of concurrency waves
///    ([`brain_embed::EmbeddingEngine::batch_timeout`]);
/// 3. overall — if the budget expires anyway, every chunk is `None` and the note
///    is still stored, still FTS-searchable, and still reports *why*.
///
/// Never returns a zero vector. A zero vector scores `0.0` against every query
/// yet still consumes a slot in `search`'s candidate budget, so it is strictly
/// worse than an honest `NULL`.
pub async fn embed_chunks(chunks: &[String], label: &str) -> Vec<Option<Vec<f32>>> {
    if chunks.is_empty() {
        return Vec::new();
    }
    let eng = brain_embed::EmbeddingEngine::from_env();
    let budget = eng.batch_timeout(chunks.len());
    let vecs = match tokio::time::timeout(budget, eng.embed_batch_partial(chunks.to_vec())).await {
        Ok(v) => v,
        Err(_) => {
            eprintln!(
                "brain: embedding budget of {:?} expired for {} chunk(s) of {} — stored without vectors, FTS search still works. Raise {}=<secs> if this is a cold model start.",
                budget, chunks.len(), label, brain_embed::EMBED_TIMEOUT_ENV
            );
            vec![None; chunks.len()]
        }
    };
    let ok = vecs.iter().filter(|v| v.is_some()).count();
    if ok < vecs.len() {
        eprintln!(
            "brain: embedded {}/{} chunks of {} ({} NULL, awaiting backfill)",
            ok, vecs.len(), label, vecs.len() - ok
        );
    }
    vecs
}

/// Everything [`sync_note_chunks`] needs for one note.
///
/// Bundled into a struct rather than a 10-argument function so that adding a
/// field to a note write does not silently change the meaning of a positional
/// call — every call site passes named fields.
pub struct ChunkSyncInput<'a> {
    pub note_id: i64,
    pub path: &'a str,
    pub layer: &'a str,
    pub scope: Option<&'a str>,
    pub content: &'a str,
    pub project_id: Option<i64>,
    pub tags: &'a [String],
    /// Vectors captured before the content changed; see [`Store::chunk_snapshot`].
    pub snapshot: brain_store::ChunkSnapshot,
    /// Vectors just computed, each keyed by its `chunk_index` and paired with the
    /// chunk text it came from.
    ///
    /// Indexed rather than positional because the two producers need different
    /// things from it. A whole-note producer (the CLI `store`, a reindex) has every
    /// chunk and builds a `NoteEmbed`, which `fresh_vectors()` turns into this map.
    /// The session hook has *only the delta* — the sections that grew since the last
    /// event — and a positional type would force it to either embed the whole
    /// accumulated document again (quadratic) or lie about which chunk each vector
    /// belongs to.
    ///
    /// The pairing is not decoration — it is how `chunks_sync` can tell a vector
    /// that still describes the chunk from one computed against text the note no
    /// longer has.
    pub fresh: brain_store::FreshVectors,
}

/// Re-chunks a note through [`Store::chunks_sync`].
///
/// The stored rows are read back by `chunks_sync` itself, so `snapshot` is only
/// needed when the caller captured vectors from somewhere that is no longer the
/// table. Chunks whose text is unchanged keep their existing vector and are never
/// re-embedded, which is what keeps the session hook cheap: a session note grows
/// on every tool result, and re-embedding the whole accumulated document each time
/// would be quadratic work for vectors that already exist.
pub fn sync_note_chunks(store: &Store, input: ChunkSyncInput<'_>) -> anyhow::Result<brain_store::ChunkSyncStats> {
    store.chunks_sync(
        input.note_id,
        input.path,
        input.layer,
        input.scope,
        input.content,
        input.project_id,
        input.tags,
        &input.fresh,
        &input.snapshot,
    )
}

/// Validates a note write and returns the path it will be stored under.
///
/// This is *the* write rule, and every writer goes through it: the REST handler,
/// the MCP tool, the CLI and the session hook. It used to be four copies of the
/// same five checks, and the copy count is exactly why the hook was the one that
/// forgot the size limits — a rule with four implementations has zero.
///
/// It runs before any `Store` is opened and before any embed, so a note that is
/// too big costs microseconds instead of a bounded-but-slow multi-minute embed of
/// text nobody wanted. See [`brain_core::validate_content_limits`] for the two
/// limits and why neither subsumes the other.
pub fn validate_note_write(layer: &str, path: &str, scope: Option<&str>, content: &str) -> anyhow::Result<String> {
    brain_core::validate_layer(layer)?;
    brain_core::sanitize_relative_path(path)?;
    if brain_core::LAYERS_WITH_SCOPE.contains(&layer) {
        let s = scope.ok_or_else(|| anyhow::anyhow!("scope required for {layer}"))?;
        brain_core::validate_scope(s)?;
    }
    brain_core::validate_content_limits(content)?;
    Ok(full_path(layer, path, scope))
}

/// `layer`/`scope`/`path` assembled the way every write path assembles it.
pub fn full_path(layer: &str, path: &str, scope: Option<&str>) -> String {
    if brain_core::LAYERS_WITH_SCOPE.contains(&layer) {
        format!("{}/{}/{}", layer, scope.unwrap_or_default(), path)
    } else {
        format!("{}/{}", layer, path)
    }
}

/// A note write, already validated, still to be persisted and queued.
pub struct NoteWrite<'a> {
    pub layer: &'a str,
    pub path: &'a str,
    pub scope: Option<&'a str>,
    pub content: &'a str,
    pub project: Option<&'a str>,
    pub tags: &'a [String],
    pub pinned: bool,
    pub expires_at: Option<&'a str>,
}

/// What a queued write did. `queued` is the honest answer to "is this note
/// searchable by meaning yet": it is not, and this is the number of chunks the
/// background queue still owes a vector.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NoteStoreOutcome {
    pub path: String,
    pub note_id: i64,
    pub chunks: usize,
    pub embedded: usize,
    pub nulls: usize,
    pub queued: usize,
}

/// Persists a note and queues its embedding. **No network call happens here.**
///
/// The note, its `audit_log` entry, its links/entities and its chunk rows are all
/// written with `embedding = NULL`; FTS5 is populated by the same transaction, so
/// the note is fully searchable the moment this returns. The embedding goes to
/// `queue` and the caller answers immediately — which is the whole point of
/// US-02.7, and the reason a store's latency no longer scales with the chunk
/// count. Measured on the real protocol path: 0.018 s for both an 8-chunk and a
/// 64-chunk note, against 0.4-0.6 s and 2.8-3.9 s for the same notes embedded
/// inline.
///
/// Split out of the axum handler and the rmcp tool so both protocol paths share
/// one implementation (they used to disagree about embedding) and so the
/// write/queue split is testable against an injected queue.
pub fn store_note_and_queue(store: &Store, w: NoteWrite<'_>, queue: &EmbedQueue) -> anyhow::Result<NoteStoreOutcome> {
    let full_path = full_path(w.layer, w.path, w.scope);
    let project_id = match w.project {
        Some(name) => store
            .project_get(name)?
            .or_else(|| store.project_create(name, "").ok())
            .map(|p| p.id),
        None => None,
    };
    let nid = store.note_upsert(&full_path, w.layer, w.scope, w.content, project_id, w.tags, w.pinned, w.expires_at)?;
    // The chunk list is needed twice: to bound the write (`MAX_CHUNKS`) and to
    // record the provenance the queue re-verifies its vectors against.
    let chunks = brain_core::chunk_text(w.content, brain_core::CHUNK_TARGET_TOKENS);
    // No `fresh`: every chunk is written NULL and the queue fills it. A vector
    // that already matches the current text survives via `chunks_sync`'s reuse
    // path, so a re-store of an unchanged note still costs nothing.
    let stats = sync_note_chunks(
        store,
        ChunkSyncInput {
            note_id: nid,
            path: &full_path,
            layer: w.layer,
            scope: w.scope,
            content: w.content,
            project_id,
            tags: w.tags,
            snapshot: brain_store::ChunkSnapshot::new(),
            fresh: brain_store::FreshVectors::new(),
        },
    )?;
    let queued = if stats.nulls > 0 {
        queue.enqueue(store.path(), &full_path, chunks);
        stats.nulls
    } else {
        0
    };
    Ok(NoteStoreOutcome { path: full_path, note_id: nid, chunks: stats.total, embedded: stats.embedded, nulls: stats.nulls, queued })
}


/// Legacy router for backward compat (shared Store via Mutex)
pub fn create_router(store: Arc<Mutex<Store>>) -> axum::Router {
    use axum::{routing::get, Json, extract::State};
    use serde_json::Value;

    async fn ping() -> &'static str { "pong" }

    async fn status(State(store): State<Arc<Mutex<Store>>>) -> Json<Value> {
        let s = store.lock().unwrap();
        let notes = s.count_notes().unwrap_or(0);
        let chunks = s.count_chunks().unwrap_or(0);
        let projects = s.project_list().unwrap_or_default().len();
        Json(serde_json::json!({"notes": notes, "chunks": chunks, "projects": projects, "version": 4}))
    }

    axum::Router::new()
        .route("/ping", get(ping))
        .route("/status", get(status))
        .with_state(store)
}

// ---- New Phase C: per-request AppState{db} + full tool registry + SSE + stdio fallback ----

use axum::{extract::{Query, State}, Json, Router, routing::{get, post}, response::{IntoResponse, sse::{Event, Sse}}, http::StatusCode};
use serde::Deserialize;
use std::convert::Infallible;

/// Per-request application state: which database, and which embedding queue.
///
/// `queue` is `Arc` because it is shared, `Send`-safe background state — not a
/// `Store`. `Store` is `!Sync` (rusqlite's statement cache is a `RefCell`), which
/// is why it is opened per request and dropped before every `.await` instead.
#[derive(Clone)]
pub struct AppState {
    pub db: String,
    pub queue: Arc<EmbedQueue>,
}

impl AppState {
    /// Production state: the env-configured queue.
    pub fn new(db: String) -> Self {
        Self { db, queue: global_queue() }
    }
}

#[derive(Deserialize)] pub struct SearchQuery { pub query: String, pub top_k: Option<usize>, pub layer: Option<String>, pub scope: Option<String>, pub project: Option<String>, pub tag: Option<String> }
#[derive(Deserialize)] pub struct ReadQuery { pub path: String }
#[derive(Deserialize)] pub struct StoreBody { pub layer: String, pub path: String, pub content: String, pub scope: Option<String>, pub project: Option<String>, pub tags: Option<Vec<String>>, pub pinned: Option<bool>, pub expires_at: Option<String> }
#[derive(Deserialize)] pub struct DeleteBody { pub path: String }
#[derive(Deserialize)] pub struct RestoreBody { pub id: i64 }
#[derive(Deserialize)] pub struct BackupBody { pub to: Option<String> }
#[derive(Deserialize)] pub struct ExportBody { pub to: Option<String>, pub force: Option<bool> }
#[derive(Deserialize)] pub struct ForgetSweepBody { pub dry_run: Option<bool> }
#[derive(Deserialize)] pub struct ProjectBody { pub name: String, pub description: Option<String> }
#[derive(Deserialize)] pub struct ProjectLinkBody { pub note_path: String, pub project: String }
#[derive(Deserialize)] pub struct CheckpointsQuery { pub limit: Option<usize> }
#[derive(Deserialize)] pub struct RecentQuery { pub top_k: Option<usize> }

/// MCP SSE + stdio fallback router — per-request Store::open via AppState{db}
/// Re-queues everything the last process left owed, and starts a worker if there
/// is anything to do. Returns how many notes were recovered.
///
/// X-04. Extracted from [`serve_rmcp_sse_with`] so the boot path is a name that can
/// be called and asserted on, rather than a statement buried in a function that
/// then blocks on ctrl-c forever. See that function for why the parameters matter.
///
/// # Why recovery runs *before* the listener binds
///
/// The ordering is a decision, and the reviewer's question was whether moving it
/// after the listener would be better. It would not, and the reason is the
/// property that matters: **boot recovery must not compete with the first
/// `brain_store`.** Before the listener is up, no client can connect, so the
/// recovery scan has the database to itself — no `brain_store` can interleave a
/// write, take the write lock, or re-queue the same path underneath it. Moving it
/// after the bind would put a full-corpus scan in the path of the first request
/// that arrives, which is exactly the request that gets a slow answer.
///
/// # Why there is no cap on the scan
///
/// The obvious mitigation — bound the number of notes recovered so boot stays
/// fast — is **worse than the delay it prevents**, and worth being explicit about
/// because it looks like the careful choice. `recover` does not embed anything: it
/// reads `(path, content)` for every note, re-chunks each in memory, and enqueues
/// the paths that still owe a vector. That is a linear scan with no network and no
/// writes, so it is milliseconds at 253 notes and remains tens of milliseconds at
/// 10k — the embedding, which *is* expensive, is spawned to a background worker
/// and does not run here. Meanwhile a cap would have to drop notes from the
/// *front* of the list, so the same notes would be deferred on every subsequent
/// boot and never recovered at all: a silent, permanent loss introduced to
/// optimise a delay nobody was feeling. The delay is bounded, reported on stdout,
/// and self-cancelling; the loss would not be.
pub fn boot_queue(queue: &Arc<EmbedQueue>, db: &str) -> usize {
    let recovered = queue.recover(db);
    if recovered > 0 {
        queue.spawn_worker();
    }
    recovered
}

pub fn mcp_router(db: String) -> Router {
    mcp_router_with_queue(db, global_queue())
}

/// [`mcp_router`] with an explicit queue.
///
/// The queue is a field of the state rather than a process-global lookup so that a
/// test can drive the real handlers against a mock Ollama. It used to be
/// `global_queue()` inside every handler, which meant the only way to test a
/// handler was to swap process-global state — so the tests that did asserted
/// against whatever `BRAIN_OLLAMA_URL` happened to be, and one of them fired 60
/// real embedding requests at whatever Ollama the developer was running.
pub fn mcp_router_with_queue(db: String, queue: Arc<EmbedQueue>) -> Router {
    let state = AppState { db, queue };
    Router::new()
        .route("/ping", get(mcp_ping))
        .route("/sse", get(sse_handler))
        .route("/mcp/ping", get(mcp_ping).merge(post(mcp_ping)))
        .route("/mcp/status", get(mcp_status))
        .route("/mcp/store", post(mcp_store))
        .route("/mcp/read", get(mcp_read).merge(post(mcp_read)))
        .route("/mcp/search", get(mcp_search_get).merge(post(mcp_search)))
        .route("/mcp/delete", post(mcp_delete))
        .route("/mcp/recent", get(mcp_recent))
        .route("/mcp/checkpoints", get(mcp_checkpoints))
        .route("/mcp/restore", post(mcp_restore))
        .route("/mcp/backup", post(mcp_backup))
        .route("/mcp/export", post(mcp_export))
        .route("/mcp/forget_sweep", post(mcp_forget_sweep))
        .route("/mcp/project_create", post(mcp_project_create))
        .route("/mcp/project_list", get(mcp_project_list))
        .route("/mcp/project_delete", post(mcp_project_delete))
        .route("/mcp/project_notes", get(mcp_project_notes))
        .route("/mcp/project_link", post(mcp_project_link))
        .route("/mcp/project_unlink", post(mcp_project_unlink))
        // legacy compatibility
        .route("/api/status", get(mcp_status))
        .route("/api/search", get(mcp_search_get))
        .route("/api/read", get(mcp_read))
        .with_state(state)
}

async fn mcp_ping() -> &'static str { "pong" }

async fn sse_handler() -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let stream = tokio_stream::iter(vec![Ok(Event::default().data("pong"))]);
    Sse::new(stream)
}

/// Ollama liveness + model, for the status tools.
///
/// Delegates to [`ollama_status_with`] so a caller that already holds an engine —
/// a handler with the queue on its state, a test with a mock — never has to go back
/// to the environment. Building an engine per status call is also just wasteful:
/// it is two strings and a `reqwest::Client`.
pub async fn ollama_status() -> serde_json::Value {
    ollama_status_with(&brain_embed::EmbeddingEngine::from_env()).await
}

/// [`ollama_status`] against a specific engine.
pub async fn ollama_status_with(eng: &brain_embed::EmbeddingEngine) -> serde_json::Value {
    serde_json::json!({ "reachable": eng.health_check().await, "model": eng.model })
}

/// Pure, synchronous projection of chunk embedding coverage.
///
/// The coverage block is what makes a dead vector index visible: `notes` and
/// `chunks` counts alone stayed perfectly healthy while 99.6% of chunk vectors
/// were BLOB-of-zeros that scored `0.0` against everything.
pub fn coverage_status(cov: &brain_store::EmbeddingCoverage) -> serde_json::Value {
    serde_json::json!({
        "chunks_total": cov.total,
        "chunks_embedded": cov.embedded,
        "chunks_without_embedding": cov.without_embedding,
        "chunks_zero_vector": cov.zero_vector,
        "embedding_coverage_pct": cov.coverage_pct,
    })
}

/// State of the background embedding queue, for `status`.
///
/// W-04e. Every field here answers a question that previously had no answer, and
/// each one corresponds to a way the work gets stranded:
/// - `pending_len` / `ready_len` — is anything owed, and is it waiting on a backoff
///   rather than being lost? A queue that is deferred and a queue that is empty look
///   identical without the pair.
/// - `is_draining` — a drain that is wedged (the flag outliving its guard) blocks
///   every later `brain_store` from spawning a worker, and that is invisible from
///   the outside.
/// - `embed_lock_holder` / `_age_s` / `_expires_in_s` — is another embed running, and
///   for how long. "Standing down" and "a process died holding the lock" are the
///   same symptom and need different responses.
/// - X-02: `dead_lettered` / `last_drain` — the queue gave up. Before this, a note
///   whose embed failed left the queue with its chunks `NULL` and nothing anywhere
///   said so: `coverage_pct` sat below 100 and the queue looked idle. A dead
///   letter is the one state that is terminal, so it is the one that has to be
///   visible without a log.
pub fn queue_status(
    queue: &EmbedQueue,
    lock: brain_store::EmbedLockState,
) -> serde_json::Value {
    let last = queue.last_outcome();
    serde_json::json!({
        "pending_len": queue.pending_len(),
        "ready_len": queue.ready_len(),
        "is_draining": queue.is_draining(),
        "dead_lettered": queue.dead_lettered_total(),
        "max_failures": queue.max_failures(),
        "last_drain": last.map(|o| serde_json::json!({
            "embedded": o.embedded,
            "nulls": o.nulls,
            "retriable_nulls": o.retriable_nulls,
            "requeued": o.requeued,
            "dead_lettered": o.dead_lettered,
            "skipped_locked": o.skipped_locked,
            "failed": o.failed,
            "pending_left": o.pending_left,
            "settled": o.is_settled(),
        })),
        "embed_lock_holder": lock.holder,
        "embed_lock_age_s": lock.age_secs,
        "embed_lock_expires_in_s": lock.expires_in_secs,
    })
}

async fn mcp_status(State(s): State<AppState>) -> Json<serde_json::Value> {
    let (notes, chunks, projects, coverage, lock) = {
        let store = match Store::open(&s.db) {
            Ok(v) => v,
            Err(e) => return Json(serde_json::json!({"error": e.to_string()})),
        };
        let cov = store.embedding_coverage().unwrap_or_default();
        (
            store.count_notes().unwrap_or(0),
            store.count_chunks().unwrap_or(0),
            store.project_list().unwrap_or_default().len(),
            coverage_status(&cov),
            store.embed_lock_state(),
        )
    };
    Json(serde_json::json!({
        "notes": notes,
        "chunks": chunks,
        "projects": projects,
        "version": 4,
        "db": s.db,
        "embedding": { "coverage": coverage, "ollama": ollama_status_with(s.queue.engine()).await },
        "queue": queue_status(&s.queue, lock),
    }))
}

async fn mcp_store(State(s): State<AppState>, Json(b): Json<StoreBody>) -> impl IntoResponse {
    // The shared write rule, before any store is opened and before any work at all.
    if let Err(e) = validate_note_write(&b.layer, &b.path, b.scope.as_deref(), &b.content) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response();
    }
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    let tags = b.tags.unwrap_or_default();
    let outcome = match store_note_and_queue(&store, NoteWrite {
        layer: &b.layer, path: &b.path, scope: b.scope.as_deref(), content: &b.content,
        project: b.project.as_deref(), tags: &tags, pinned: b.pinned.unwrap_or(false),
        expires_at: b.expires_at.as_deref(),
    }, &s.queue) {
        Ok(o) => o,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };
    // The note is durable and FTS-searchable at this point; the vectors are the
    // queue's problem. `queued` says what is still owed, so nothing here implies
    // a vector already exists.
    s.queue.spawn_worker();
    (StatusCode::OK, Json(serde_json::json!({
        "ok": true,
        "path": outcome.path,
        "chunks": outcome.chunks,
        "embedded": outcome.embedded,
        "without_embedding": outcome.nulls,
        "queued": outcome.queued,
    }))).into_response()
}

async fn mcp_read(State(s): State<AppState>, Query(q): Query<ReadQuery>) -> impl IntoResponse {
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    // sanitize path
    if let Err(e) = brain_core::sanitize_relative_path(&q.path) { return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":e.to_string()}))).into_response(); }
    match store.note_get(&q.path).unwrap_or(None) {
        Some(n) => (StatusCode::OK, Json(serde_json::json!({"path": n.path, "content": n.content, "layer": n.layer, "scope": n.scope}))).into_response(),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"not found"}))).into_response(),
    }
}

async fn mcp_search(State(s): State<AppState>, Json(q): Json<SearchQuery>) -> impl IntoResponse {
    if q.query.trim().is_empty() { return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "query required"}))).into_response(); }
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    // try vector query if Ollama up, else FTS only. Same budget discipline as
    // the store path: a cold model start must not silently drop the vector half
    // of the query.
    let qvec: Option<Vec<f32>> = {
        // The queue's own engine, not a fresh one from the environment: same model
        // and URL by construction, one client instead of one per request, and a
        // handler that a test can point at a mock.
        let embed = s.queue.engine();
        match tokio::time::timeout(embed.batch_timeout(1), embed.embed(&q.query)).await {
            Ok(Ok(v)) => Some(v),
            Ok(Err(e)) => { eprintln!("brain: query not embedded (FTS only): {e:#}"); None },
            Err(_) => { eprintln!("brain: query embedding timed out (FTS only)"); None },
        }
    };
    let res = store.search(&q.query, qvec.as_deref(), q.layer.as_deref(), q.scope.as_deref(), q.project.as_deref(), q.tag.as_deref(), q.top_k.unwrap_or(5).clamp(1,20), false).unwrap_or_default();
    (StatusCode::OK, Json(serde_json::json!({"results": res, "total": res.len()}))).into_response()
}

async fn mcp_search_get(State(s): State<AppState>, Query(q): Query<SearchQuery>) -> impl IntoResponse {
    mcp_search(State(s), Json(q)).await
}

async fn mcp_delete(State(s): State<AppState>, Json(b): Json<DeleteBody>) -> impl IntoResponse {
    if let Err(e) = brain_core::sanitize_relative_path(&b.path) { return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":e.to_string()}))).into_response(); }
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    match store.note_delete(&b.path).unwrap_or(false) { true => (StatusCode::OK, Json(serde_json::json!({"ok":true}))).into_response(), false => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"not found"}))).into_response() }
}

async fn mcp_recent(State(s): State<AppState>, Query(q): Query<RecentQuery>) -> impl IntoResponse {
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    let rows = store.recent(q.top_k.unwrap_or(10)).unwrap_or_default();
    (StatusCode::OK, Json(serde_json::json!({"recent": rows}))).into_response()
}

async fn mcp_checkpoints(State(s): State<AppState>, Query(q): Query<CheckpointsQuery>) -> impl IntoResponse {
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    let cps = store.checkpoints(q.limit.unwrap_or(10)).unwrap_or_default();
    (StatusCode::OK, Json(serde_json::json!({"checkpoints": cps}))).into_response()
}

async fn mcp_restore(State(s): State<AppState>, Json(b): Json<RestoreBody>) -> impl IntoResponse {
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    match store.restore_audit(b.id).unwrap_or(false) { true => (StatusCode::OK, Json(serde_json::json!({"ok":true}))).into_response(), false => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"not found"}))).into_response() }
}

async fn mcp_backup(State(s): State<AppState>, Json(b): Json<BackupBody>) -> impl IntoResponse {
    // W-01. `to` came straight from the request and became a `std::fs::copy`
    // destination: arbitrary file write as the service user, plus a copy of the
    // whole database anywhere the caller named. Contained by an allowlist — see
    // `fs_guard`. Omitting `to` still yields `{db}.bak` and is always allowed.
    let dst = match fs_guard::backup_file(&s.db, b.to.as_deref()) {
        Ok(p) => p,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };
    match std::fs::copy(&s.db, &dst) {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({"ok":true, "to": dst.to_string_lossy()}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response(),
    }
}

async fn mcp_export(State(s): State<AppState>, Json(b): Json<ExportBody>) -> impl IntoResponse {
    // W-01, same containment as `brain_backup`: the destination is restricted to
    // `BRAIN_EXPORT_ROOT` (default `/tmp/brain-export`) before a single directory
    // is created.
    let dir = match fs_guard::export_dir(b.to.as_deref()) {
        Ok(p) => p,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };
    let force = b.force.unwrap_or(false);
    if dir.exists() && !force { return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"export dir exists, use --force"}))).into_response(); }
    if let Err(e) = std::fs::create_dir_all(&dir) { return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response(); }
    // X-05.3: re-check now that the directory exists. `export_dir` could only skip
    // this when the root was absent, and between that check and `create_dir_all`
    // another local user could have created it — in `/tmp`, which is
    // world-writable, that is a real window. Checking after creation is what closes
    // it, and it is the last point at which the export can be refused.
    if let Err(e) = fs_guard::assert_root_usable(&fs_guard::export_root()) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response();
    }
    let notes = match Store::open(&s.db) {
        Ok(v) => v.recent(10000).unwrap_or_default(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response(),
    };
    let mut written = 0usize;
    let mut refused = 0usize;
    for (path, _l, _sc, content) in notes {
        // The note's own path is re-checked per note: it comes from the database
        // rather than the request, but the export walks *existing* rows, including
        // any written before `sanitize_relative_path` existed or by a migration.
        let fp = match fs_guard::note_file_within(&dir, &path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("brain: export skipped {path}: {e:#}");
                refused += 1;
                continue;
            }
        };
        if let Some(parent) = fp.parent() { let _ = std::fs::create_dir_all(parent); }
        if std::fs::write(&fp, content).is_ok() { written += 1; } else { refused += 1; }
    }
    (StatusCode::OK, Json(serde_json::json!({"ok":true, "to": dir.to_string_lossy(), "written": written, "refused": refused}))).into_response()
}

async fn mcp_forget_sweep(State(s): State<AppState>, Json(b): Json<ForgetSweepBody>) -> impl IntoResponse {
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    match store.forget_sweep(b.dry_run.unwrap_or(false)) {
        Ok(v) => (StatusCode::OK, Json(serde_json::json!({"deleted": v, "dry_run": b.dry_run.unwrap_or(false)}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response(),
    }
}

async fn mcp_project_create(State(s): State<AppState>, Json(b): Json<ProjectBody>) -> impl IntoResponse {
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    match store.project_create(&b.name, b.description.as_deref().unwrap_or("")) { Ok(p)=> (StatusCode::OK, Json(serde_json::json!(p))).into_response(), Err(e)=> (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":e.to_string()}))).into_response() }
}
async fn mcp_project_list(State(s): State<AppState>) -> impl IntoResponse {
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    match store.project_list() { Ok(v)=> (StatusCode::OK, Json(serde_json::json!(v))).into_response(), Err(e)=> (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() }
}
async fn mcp_project_delete(State(s): State<AppState>, Json(b): Json<ProjectBody>) -> impl IntoResponse {
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    match store.project_delete(&b.name).unwrap_or(false) { true => (StatusCode::OK, Json(serde_json::json!({"ok":true}))).into_response(), false => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"not found"}))).into_response() }
}
async fn mcp_project_notes(State(s): State<AppState>, Query(q): Query<ProjectBody>) -> impl IntoResponse {
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    match store.project_notes(&q.name) { Ok(v)=> (StatusCode::OK, Json(serde_json::json!(v))).into_response(), Err(e)=> (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":e.to_string()}))).into_response() }
}
async fn mcp_project_link(State(s): State<AppState>, Json(b): Json<ProjectLinkBody>) -> impl IntoResponse {
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    match store.note_link_project(&b.note_path, &b.project) { Ok(_)=> (StatusCode::OK, Json(serde_json::json!({"ok":true}))).into_response(), Err(e)=> (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":e.to_string()}))).into_response() }
}
async fn mcp_project_unlink(State(s): State<AppState>, Json(b): Json<ProjectLinkBody>) -> impl IntoResponse {
    let store = match Store::open(&s.db) { Ok(v)=>v, Err(e)=> return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error":e.to_string()}))).into_response() };
    match store.note_unlink_project(&b.note_path, &b.project) { Ok(v)=> (StatusCode::OK, Json(serde_json::json!({"ok": v}))).into_response(), Err(e)=> (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":e.to_string()}))).into_response() }
}

/// stdio fallback — read JSON lines from stdin, dispatch to Store, write JSON to stdout
pub async fn serve_stdio(db: String) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = tokio::io::stdout();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() { continue; }
        let val: serde_json::Value = match serde_json::from_str(&line) { Ok(v)=>v, Err(e)=> { let err = serde_json::json!({"error": e.to_string()}); stdout.write_all(format!("{}\n", err).as_bytes()).await?; continue; } };
        let tool = val.get("tool").and_then(|v| v.as_str()).unwrap_or("ping");
        let resp = match tool {
            "ping" => serde_json::json!({"pong": true}),
            "status" => {
                match Store::open(&db) {
                    Ok(st) => serde_json::json!({"notes": st.count_notes().unwrap_or(0), "chunks": st.count_chunks().unwrap_or(0)}),
                    Err(e) => serde_json::json!({"error": e.to_string()}),
                }
            },
            "search" => {
                let q = val.get("query").and_then(|v| v.as_str()).unwrap_or("");
                match Store::open(&db) {
                    Ok(st) => serde_json::json!({"results": st.search(q, None, None, None, None, None, 5, false).unwrap_or_default()}),
                    Err(e) => serde_json::json!({"error": e.to_string()}),
                }
            },
            _ => serde_json::json!({"error": format!("unknown tool {}", tool)}),
        };
        stdout.write_all(format!("{}\n", resp).as_bytes()).await?;
        stdout.flush().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_embed::EmbeddingEngine;

    /// A URL guaranteed to refuse connections: a port bound, then released.
    ///
    /// Every test in this module runs against one of these or a counting mock, so
    /// the suite never depends on a developer's Ollama and never spends 60 real
    /// embedding requests proving something about the write path. Previously the
    /// handlers reached for `global_queue()` / `from_env()`, so a "unit" test of
    /// `mcp_store` fired real requests at whatever `BRAIN_OLLAMA_URL` pointed at.
    async fn closed_port_url() -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
        let addr = l.local_addr().unwrap();
        drop(l);
        format!("http://{addr}")
    }

    /// A queue on a dead port: embeds nothing, instantly, in any environment.
    async fn dead_queue() -> Arc<EmbedQueue> {
        Arc::new(EmbedQueue::new(EmbeddingEngine::new(closed_port_url().await, "nomic-embed-text".into())))
    }

    /// A queue on a mock that counts requests, for the paths where "did it embed?"
    /// is the assertion.
    async fn counting_mock() -> (EmbeddingEngine, Arc<std::sync::atomic::AtomicUsize>, tokio::task::JoinHandle<()>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = Arc::new(AtomicUsize::new(0));
        let app = axum::Router::new()
            .route("/api/tags", axum::routing::get(|| async { axum::Json(serde_json::json!({"models": []})) }))
            .route(
                "/api/embeddings",
                axum::routing::post(move |axum::extract::State(h): axum::extract::State<Arc<AtomicUsize>>| async move {
                    h.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::OK, axum::Json(serde_json::json!({"embedding": vec![1.0; brain_core::EMBEDDING_DIM]})))
                }),
            )
            .with_state(hits.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (EmbeddingEngine::new(format!("http://{addr}"), "nomic-embed-text".into()), hits, task)
    }

    async fn state(db: String) -> AppState {
        AppState { db, queue: dead_queue().await }
    }

    fn tmp_db(tag: &str) -> String {
        let db = format!("/tmp/brain-mcp-{}-{}.db", std::process::id(), tag);
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(format!("{}.bak", db));
        db
    }

    /// A scratch directory inside the real export root, so the allowlist tests
    /// exercise the shipped policy instead of a widened one.
    fn export_scratch(tag: &str) -> std::path::PathBuf {
        let p = fs_guard::export_root().join(format!("brain-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create scratch inside the export root");
        p.canonicalize().expect("canonicalize scratch")
    }

    async fn status_of(resp: impl IntoResponse) -> StatusCode {
        resp.into_response().status()
    }

    /// JSON body of a 200 response.
    async fn body_of(resp: impl IntoResponse) -> serde_json::Value {
        let r = resp.into_response();
        assert_eq!(r.status(), StatusCode::OK, "expected 200, got {}", r.status());
        let bytes = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn store_body(content: String) -> StoreBody {
        StoreBody {
            layer: "regras".into(),
            path: "oversize".into(),
            content,
            scope: Some("global".into()),
            project: None,
            tags: None,
            pinned: None,
            expires_at: None,
        }
    }

    // -------------------------------------------------------------- router --

    #[tokio::test]
    async fn the_router_actually_serves_ping() {
        // Was `assert!(true, "router created")`, which asserted nothing: a router
        // that panics on first request, or one wired to no handler at all, passes
        // a construction check. Driving a request through it is the only version of
        // this test that can fail.
        use tower::ServiceExt;
        let app = mcp_router_with_queue(tmp_db("router"), dead_queue().await);
        let resp = app
            .oneshot(axum::http::Request::get("/ping").body(axum::body::Body::empty()).unwrap())
            .await
            .expect("the router must answer");
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 16).await.unwrap();
        assert_eq!(bytes.as_ref(), b"pong");
    }

    #[test]
    fn test_create_router_legacy() {
        let s = Store::open_in_memory().unwrap();
        let r = create_router(Arc::new(Mutex::new(s)));
        let _ = r;
    }

    #[tokio::test]
    async fn test_ping_pong() {
        let resp = mcp_ping().await;
        assert_eq!(resp, "pong");
    }

    #[test]
    fn test_per_request_state() {
        let st = AppState { db: ":memory:".into(), queue: global_queue() };
        let _ = Store::open(&st.db).is_ok();
    }

    // ------------------------------------------------------------- handlers --

    #[tokio::test]
    async fn test_status_counts_notes() {
        let db = tmp_db("status");
        { let s = Store::open(&db).unwrap(); s.note_upsert("regras/global/a", "regras", Some("global"), "## hi", None, &[], false, None).unwrap(); }
        let Json(v) = mcp_status(State(state(db.clone()).await)).await;
        assert_eq!(v["notes"], 1);
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_store_scope_required_and_traversal() {
        let st = state(tmp_db("validate")).await;
        // missing scope for regras -> 400
        let r = mcp_store(State(st.clone()), Json(StoreBody{ layer: "regras".into(), path: "x".into(), content: "## h".into(), scope: None, project: None, tags: None, pinned: None, expires_at: None })).await;
        assert_eq!(status_of(r).await, StatusCode::BAD_REQUEST);
        // traversal -> 400
        let r = mcp_store(State(st.clone()), Json(StoreBody{ layer: "regras".into(), path: "../evil".into(), content: "## h".into(), scope: Some("global".to_owned()), project: None, tags: None, pinned: None, expires_at: None })).await;
        assert_eq!(status_of(r).await, StatusCode::BAD_REQUEST);
        // bad layer -> 400
        let r = mcp_store(State(st.clone()), Json(StoreBody{ layer: "nope".into(), path: "x".into(), content: "## h".into(), scope: None, project: None, tags: None, pinned: None, expires_at: None })).await;
        assert_eq!(status_of(r).await, StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_file(&st.db);
    }

    #[tokio::test]
    async fn test_store_read_delete_roundtrip() {
        let db = tmp_db("roundtrip");
        let st = state(db.clone()).await;
        let r = mcp_store(State(st.clone()), Json(StoreBody{ layer: "regras".into(), path: "phasec/rt".into(), content: "## roundtrip".to_owned(), scope: Some("global".to_owned()), project: None, tags: None, pinned: None, expires_at: None })).await;
        assert_eq!(status_of(r).await, StatusCode::OK);
        let r = mcp_read(State(st.clone()), Query(ReadQuery{ path: "regras/global/phasec/rt".into() })).await;
        assert_eq!(status_of(r).await, StatusCode::OK);
        let r = mcp_read(State(st.clone()), Query(ReadQuery{ path: "regras/global/missing".into() })).await;
        assert_eq!(status_of(r).await, StatusCode::NOT_FOUND);
        let r = mcp_delete(State(st.clone()), Json(DeleteBody{ path: "regras/global/phasec/rt".into() })).await;
        assert_eq!(status_of(r).await, StatusCode::OK);
        let r = mcp_delete(State(st.clone()), Json(DeleteBody{ path: "regras/global/phasec/rt".into() })).await;
        assert_eq!(status_of(r).await, StatusCode::NOT_FOUND);
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_store_with_project_tags_pinned() {
        let db = tmp_db("full");
        let st = state(db.clone()).await;
        let r = mcp_store(State(st.clone()), Json(StoreBody{ layer: "regras".into(), path: "phasec/full".into(), content: "## full".to_owned(), scope: Some("global".to_owned()), project: Some("pfull".into()), tags: Some(vec!["t1".into()]), pinned: Some(true), expires_at: None })).await;
        assert_eq!(status_of(r).await, StatusCode::OK);
        let s = Store::open(&db).unwrap();
        let names: Vec<String> = s.project_list().unwrap().into_iter().map(|pr| pr.name).collect();
        assert!(names.contains(&"pfull".to_string()));
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_search_validation_and_hit() {
        let db = tmp_db("search");
        { let s = Store::open(&db).unwrap(); s.note_upsert("regras/global/b", "regras", Some("global"), "## unique zebra phrase", None, &[], false, None).unwrap(); }
        let st = state(db.clone()).await;
        let r = mcp_search(State(st.clone()), Json(SearchQuery{ query: "   ".into(), top_k: None, layer: None, scope: None, project: None, tag: None })).await;
        assert_eq!(status_of(r).await, StatusCode::BAD_REQUEST);
        let r = mcp_search(State(st.clone()), Json(SearchQuery{ query: "zebra".into(), top_k: Some(5), layer: None, scope: None, project: None, tag: None })).await;
        assert_eq!(status_of(r).await, StatusCode::OK);
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_recent_checkpoints_backup_export_sweep() {
        let db = tmp_db("misc");
        let st = state(db.clone()).await;
        { let s = Store::open(&db).unwrap(); s.note_upsert("regras/global/c", "regras", Some("global"), "## c", None, &[], false, None).unwrap(); }
        assert_eq!(status_of(mcp_recent(State(st.clone()), Query(RecentQuery{ top_k: Some(5) })).await).await, StatusCode::OK);
        assert_eq!(status_of(mcp_checkpoints(State(st.clone()), Query(CheckpointsQuery{ limit: Some(5) })).await).await, StatusCode::OK);
        // No `to`: the documented default is `{db}.bak`, a sibling of the database.
        assert_eq!(status_of(mcp_backup(State(st.clone()), Json(BackupBody{ to: None })).await).await, StatusCode::OK);
        assert!(std::path::Path::new(&format!("{db}.bak")).exists());
        let exp = export_scratch("misc");
        assert_eq!(status_of(mcp_export(State(st.clone()), Json(ExportBody{ to: Some(exp.to_string_lossy().into_owned()), force: Some(true) })).await).await, StatusCode::OK);
        assert!(exp.join("regras/global/c").exists());
        // sweep with nothing expired -> ok empty
        let r = mcp_forget_sweep(State(st.clone()), Json(ForgetSweepBody{ dry_run: Some(true) })).await.into_response();
        assert_eq!(r.status(), StatusCode::OK);
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(format!("{db}.bak"));
        let _ = std::fs::remove_dir_all(&exp);
    }

    #[tokio::test]
    async fn test_project_lifecycle_and_restore() {
        let db = tmp_db("proj");
        let st = state(db.clone()).await;
        { let s = Store::open(&db).unwrap(); s.note_upsert("regras/global/c", "regras", Some("global"), "## c", None, &[], false, None).unwrap(); }
        assert_eq!(status_of(mcp_project_create(State(st.clone()), Json(ProjectBody{ name: "p1".into(), description: None })).await).await, StatusCode::OK);
        assert_eq!(status_of(mcp_project_list(State(st.clone())).await).await, StatusCode::OK);
        assert_eq!(status_of(mcp_project_link(State(st.clone()), Json(ProjectLinkBody{ note_path: "regras/global/c".into(), project: "p1".into() })).await).await, StatusCode::OK);
        assert_eq!(status_of(mcp_project_notes(State(st.clone()), Query(ProjectBody{ name: "p1".into(), description: None })).await).await, StatusCode::OK);
        assert_eq!(status_of(mcp_project_unlink(State(st.clone()), Json(ProjectLinkBody{ note_path: "regras/global/c".into(), project: "p1".into() })).await).await, StatusCode::OK);
        // restore flow: create + delete via store, restore delete audit
        { let s = Store::open(&db).unwrap(); s.note_upsert("sessoes/tmp/x", "sessoes", None, "## x", None, &[], false, None).unwrap(); s.note_delete("sessoes/tmp/x").unwrap(); }
        let del_id = { let s = Store::open(&db).unwrap(); s.checkpoints(5).unwrap().into_iter().find(|(_, a, p, _)| a == "delete" && p == "sessoes/tmp/x").map(|(id, _, _, _)| id).unwrap() };
        assert_eq!(status_of(mcp_restore(State(st.clone()), Json(RestoreBody{ id: del_id })).await).await, StatusCode::OK);
        assert_eq!(status_of(mcp_read(State(st.clone()), Query(ReadQuery{ path: "sessoes/tmp/x".into() })).await).await, StatusCode::OK);
        assert_eq!(status_of(mcp_project_delete(State(st.clone()), Json(ProjectBody{ name: "p1".into(), description: None })).await).await, StatusCode::OK);
        let _ = std::fs::remove_file(&db);
    }

    // ------------------------------------------------------------------
    // P0-ZV: the vector stream must never be fed BLOB-of-zeros, and status must
    // make that visible. These hold whether or not Ollama is running, which is
    // what lets the suite assert the invariant in any environment.
    // ------------------------------------------------------------------

    /// Core invariant, independent of Ollama: after any store, every chunk is
    /// either a usable vector or NULL. Nothing in between, ever.
    async fn assert_no_zero_vectors(db: &str, label: &str) {
        let s = Store::open(db).unwrap();
        let cov = s.embedding_coverage().unwrap();
        assert_eq!(cov.zero_vector, 0, "{label}: a BLOB-of-zeros reached the index");
        assert_eq!(
            cov.embedded + cov.without_embedding,
            cov.total,
            "{label}: every chunk must be either embedded or NULL, got {:?}",
            cov
        );
    }

    #[tokio::test]
    async fn test_mcp_store_never_writes_a_zero_vector() {
        let db = tmp_db("zerovec");
        let st = state(db.clone()).await;
        let r = mcp_store(State(st.clone()), Json(StoreBody { layer: "regras".into(), path: "zv/one".into(), content: "## multi\n\n## section two withcontent".into(), scope: Some("global".into()), project: None, tags: None, pinned: None, expires_at: None })).await;
        assert_eq!(status_of(r).await, StatusCode::OK);
        assert_no_zero_vectors(&db, "mcp_store").await;
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_mcp_status_reports_embedding_coverage_and_ollama_health() {
        // TD-004 / US-04.3: `status` reported only notes/chunks/projects, which
        // stayed perfectly healthy while the whole vector index was dead.
        let db = tmp_db("cov");
        { let s = Store::open(&db).unwrap(); s.note_upsert("regras/global/c", "regras", Some("global"), "## c", None, &[], false, None).unwrap(); }
        let Json(v) = mcp_status(State(state(db.clone()).await)).await;
        let emb = &v["embedding"];
        assert!(emb["coverage"].is_object(), "status.embedding.coverage missing from {}", v);
        for key in ["chunks_total", "chunks_embedded", "chunks_without_embedding", "chunks_zero_vector", "embedding_coverage_pct"] {
            assert!(!emb["coverage"][key].is_null(), "status.embedding.coverage.{} missing from {}", key, v);
        }
        assert_eq!(emb["coverage"]["chunks_zero_vector"], 0, "no zero vectors may exist after a plain store");
        assert!(emb["ollama"]["reachable"].is_boolean(), "ollama.reachable must be a bool, got {}", emb["ollama"]);
        assert!(emb["ollama"]["model"].is_string(), "ollama.model must be a string, got {}", emb["ollama"]);
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn test_coverage_status_projects_every_field() {
        use brain_store::EmbeddingCoverage;
        let cov = EmbeddingCoverage { total: 738, embedded: 3, without_embedding: 0, zero_vector: 735, coverage_pct: 0.41 };
        let v = coverage_status(&cov);
        assert_eq!(v["chunks_total"], 738);
        assert_eq!(v["chunks_embedded"], 3);
        assert_eq!(v["chunks_without_embedding"], 0);
        assert_eq!(v["chunks_zero_vector"], 735, "the 735 dead vectors must be reported, not folded into `embedded`");
        assert_eq!(v["embedding_coverage_pct"], 0.41);
    }

    #[tokio::test]
    async fn test_ollama_status_does_not_panic_when_ollama_is_down() {
        // Bounded by brain-embed's health timeout and must resolve to a bool
        // rather than propagating or panicking. Driven against a dead port on
        // purpose: the test is named "when ollama is down", so it must actually
        // have Ollama down rather than whatever the environment happens to serve.
        let v = ollama_status_with(&EmbeddingEngine::new(closed_port_url().await, "nomic-embed-text".into())).await;
        assert_eq!(v["reachable"], false, "a dead port is not reachable");
        assert!(v["model"].is_string(), "got {}", v);
    }

    #[tokio::test]
    async fn test_embed_chunks_of_nothing_is_empty_and_never_blocks() {
        assert!(embed_chunks(&[], "empty").await.is_empty());
    }

    #[tokio::test]
    async fn test_search_via_store_per_request() {
        let db = format!("/tmp/brain-mcp-test-{}.db", std::process::id());
        let _ = std::fs::remove_file(&db);
        {
            let s = Store::open(&db).unwrap();
            s.note_upsert("regras/global/test-mcp", "regras", Some("global"), "## hello mcp test", None, &[], false, None).unwrap();
        }
        // search via mcp logic (FTS fallback) — same file DB shared across opens
        let st = Store::open(&db).unwrap();
        let res = st.search("hello", None, None, None, None, None, 5, false).unwrap();
        assert!(!res.is_empty());
        let _ = std::fs::remove_file(&db);
    }

    // ------------------------------------------------------------------
    // W-01: the two tools that turn a request string into a filesystem write.
    // Both are unauthenticated on a `0.0.0.0` listener, so the destination has to
    // be contained. Each case below is a path that used to be accepted.
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn export_refuses_a_destination_outside_the_export_root() {
        let db = tmp_db("export-escape");
        let st = state(db.clone()).await;
        // `/etc` exists already; the point is that the handler refused it, and that
        // the names below were not created.
        let invented = ["/etc/brain-x", "/tmp/brain-not-the-root", "/var/tmp/brain-y"];
        for to in ["/etc", "/tmp/brain-not-the-root", "/var/tmp/brain-y", "/etc/brain-x"] {
            for force in [Some(true), Some(false), None] {
                let r = mcp_export(State(st.clone()), Json(ExportBody { to: Some(to.to_string()), force })).await;
                let resp = r.into_response();
                assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{to:?} (force={force:?}) must be refused");
                let bytes = axum::body::to_bytes(resp.into_body(), 1 << 16).await.unwrap();
                let msg = String::from_utf8_lossy(&bytes).to_string();
                assert!(msg.contains("export root"), "the error must name the allowlist, got {msg}");
            }
        }
        for p in invented {
            assert!(!std::path::Path::new(p).exists(), "{p} must not have been created");
        }
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn export_refuses_traversal_out_of_the_export_root() {
        let db = tmp_db("export-traversal");
        let st = state(db.clone()).await;
        let root = fs_guard::export_root();
        // Relative climb, and the same climb written as an absolute path.
        for to in ["../brain-escape", "../../etc/brain-x", &format!("{}/../etc/brain-y", root.display())] {
            let r = mcp_export(State(st.clone()), Json(ExportBody { to: Some(to.to_string()), force: Some(true) })).await;
            assert_eq!(status_of(r).await, StatusCode::BAD_REQUEST, "{to:?} must be refused");
        }
        assert!(!std::path::Path::new("/etc/brain-x").exists());
        assert!(!std::path::Path::new("/etc/brain-y").exists());
        assert!(!root.parent().unwrap().join("brain-escape").exists(), "nothing may be created beside the root");
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn export_accepts_a_destination_inside_the_export_root() {
        // The other side of the allowlist: the documented default and an explicit
        // subdirectory both work, so containment is not a blanket refusal.
        let db = tmp_db("export-inside");
        let st = state(db.clone()).await;
        { let s = Store::open(&db).unwrap(); s.note_upsert("regras/global/inside", "regras", Some("global"), "## inside", None, &[], false, None).unwrap(); }
        let dir = export_scratch("inside");
        let v = body_of(mcp_export(State(st.clone()), Json(ExportBody { to: Some(dir.to_string_lossy().into_owned()), force: Some(true) })).await).await;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["written"], 1, "{v}");
        assert!(dir.join("regras/global/inside").exists());
        // Without `force` an existing directory is still refused.
        let r = mcp_export(State(st.clone()), Json(ExportBody { to: Some(dir.to_string_lossy().into_owned()), force: Some(false) })).await;
        assert_eq!(status_of(r).await, StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn export_without_a_destination_writes_to_the_documented_root() {
        let db = tmp_db("export-default");
        let st = state(db.clone()).await;
        { let s = Store::open(&db).unwrap(); s.note_upsert("regras/global/deflt", "regras", Some("global"), "## default target", None, &[], false, None).unwrap(); }
        let v = body_of(mcp_export(State(st.clone()), Json(ExportBody { to: None, force: Some(true) })).await).await;
        assert_eq!(v["to"], fs_guard::export_root().to_string_lossy().as_ref(), "{v}");
        assert!(fs_guard::export_root().join("regras/global/deflt").exists(), "the default root is the documented destination");
        let _ = std::fs::remove_file(fs_guard::export_root().join("regras/global/deflt"));
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn backup_refuses_a_destination_outside_the_export_root_or_without_a_bak_suffix() {
        let db = tmp_db("backup-escape");
        { let s = Store::open(&db).unwrap(); s.note_upsert("regras/global/b", "regras", Some("global"), "## b", None, &[], false, None).unwrap(); }
        let st = state(db.clone()).await;
        for to in ["/etc/brain.bak", "/tmp/brain-stolen.bak", "/root/x.bak"] {
            let r = mcp_backup(State(st.clone()), Json(BackupBody { to: Some(to.to_string()) })).await;
            assert_eq!(status_of(r).await, StatusCode::BAD_REQUEST, "{to:?} must be refused");
            assert!(!std::path::Path::new(to).exists(), "{to:?} must not exist");
        }
        // Inside the root but not a .bak: a copy-the-database primitive aimed at
        // any name is not what the allowlist is for.
        let dir = export_scratch("backup");
        let r = mcp_backup(State(st.clone()), Json(BackupBody { to: Some(dir.join("brain.db").to_string_lossy().into_owned()) })).await;
        assert_eq!(status_of(r).await, StatusCode::BAD_REQUEST);
        assert!(!dir.join("brain.db").exists());
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn backup_accepts_a_bak_file_inside_the_export_root_and_its_own_default() {
        let db = tmp_db("backup-inside");
        // `fs::copy` needs the source to exist, which is the store's doing.
        { let s = Store::open(&db).unwrap(); s.note_upsert("regras/global/b", "regras", Some("global"), "## b", None, &[], false, None).unwrap(); }
        let st = state(db.clone()).await;
        let dir = export_scratch("backup-ok");
        let dst = dir.join("copy.bak");
        let v = body_of(mcp_backup(State(st.clone()), Json(BackupBody { to: Some(dst.to_string_lossy().into_owned()) })).await).await;
        assert_eq!(v["ok"], true, "{v}");
        assert!(dst.exists(), "a .bak inside the root is allowed");
        // The default (no `to`) is a sibling of the database, which is always allowed.
        let r = mcp_backup(State(st.clone()), Json(BackupBody { to: None })).await;
        assert_eq!(status_of(r).await, StatusCode::OK);
        assert!(std::path::Path::new(&format!("{db}.bak")).exists());
        let _ = std::fs::remove_file(&dst);
        let _ = std::fs::remove_file(format!("{db}.bak"));
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ------------------------------------------------------------------
    // W-04e: `status` must expose the queue. A wedged queue leaves every other
    // number in the payload looking healthy, which is how a 99.6% dead vector
    // index passed unnoticed for a month.
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn status_reports_the_queue_state() {
        let db = tmp_db("queue-status");
        let st = state(db.clone()).await;
        let queue = st.queue.clone();
        queue.enqueue(&db, "regras/global/q", vec!["## a".to_string()]);
        // A lock held by somebody else, so the lock fields are not all null.
        { let s = Store::open(&db).unwrap(); assert!(s.try_acquire_embed_lock(&brain_store::embed_lock_owner("reindex"), 900).unwrap()); }
        let Json(v) = mcp_status(State(st.clone())).await;
        let q = v["queue"].clone();
        let keys = q.as_object().expect("status.queue must be an object");
        for key in [
            "pending_len",
            "ready_len",
            "is_draining",
            // X-02: a queue that gave up is the one state with no other signal.
            "dead_lettered",
            "max_failures",
            "last_drain",
            "embed_lock_holder",
            "embed_lock_age_s",
            "embed_lock_expires_in_s",
        ] {
            assert!(keys.contains_key(key), "status.queue.{} missing from {}", key, v);
        }
        assert_eq!(q["pending_len"], 1, "{q}");
        assert_eq!(q["ready_len"], 1, "{q}");
        assert_eq!(q["is_draining"], false, "{q}");
        assert!(q["embed_lock_holder"].as_str().unwrap().contains("reindex"), "{q}");
        assert!(q["embed_lock_age_s"].as_i64().unwrap() >= 0, "{q}");
        assert!(q["embed_lock_expires_in_s"].as_i64().unwrap() > 0, "{q}");
        let _ = std::fs::remove_file(&db);
    }

    /// X-05.1. The pair `pending_len > 0, ready_len == 0` means "waiting on a
    /// backoff", which is a different operational story from "nothing owed".
    ///
    /// **Split into two tests, neither of which reads a clock.** This used to be
    /// one test that drained against a held lock and then asserted `ready_len == 0`
    /// — a claim about a 500 ms window that also had to survive a `Store::open`, a
    /// set of counts and an Ollama health check. The review found it failed three
    /// times out of four mutations and passed only under `--nocapture`, which is
    /// the signature of a test whose result depends on how busy the machine is.
    ///
    /// What is actually being claimed splits cleanly in two:
    /// - *standing down sets a real future deadline* — asserted against a timestamp
    ///   taken before the drain, so the monotonic clock makes it unconditional;
    /// - *the status payload reports a deferred queue differently from an empty
    ///   one* — asserted by putting a job a whole hour out, so there is no window
    ///   to race at all.
    #[tokio::test]
    async fn standing_down_for_the_lock_sets_a_future_deadline() {
        let db = tmp_db("queue-deferred-deadline");
        let st = state(db.clone()).await;
        st.queue.enqueue(&db, "regras/global/d", vec!["## a".to_string()]);
        {
            let s = Store::open(&db).unwrap();
            assert!(s.try_acquire_embed_lock(&brain_store::embed_lock_owner("reindex"), 900).unwrap());
        }
        let before_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let out = st.queue.drain().await;
        assert_eq!(out.skipped_locked, 1, "{out:?}");
        assert_eq!(out.pending_left, 1, "{out:?}");

        let deadline = st
            .queue
            .earliest_ready_at_ms()
            .expect("the deferred job must still be queued, carrying its deadline");
        assert!(
            deadline > before_ms,
            "standing down must schedule the job for later, not re-run it immediately: deadline {deadline} \
             vs before {before_ms}"
        );
        assert!(
            st.queue.millis_until_next_ready().unwrap_or(0) > 0,
            "and the remaining wait must be positive, which is what ready_len == 0 reports"
        );
        let _ = std::fs::remove_file(&db);
    }

    /// The status payload must distinguish a deferred queue from an empty one, with
    /// no timing involved.
    ///
    /// The job is enqueued an hour out rather than produced by a failure with a
    /// 500 ms backoff, so the assertion cannot be affected by how long the status
    /// call takes. `enqueue_at` is public precisely so this arrangement is
    /// expressible: a test that has to provoke a real failure to observe a deadline
    /// is a test whose window it does not control.
    #[tokio::test]
    async fn status_reports_a_deferred_queue_distinctly_from_an_empty_one() {
        let db = tmp_db("queue-deferred");
        let st = state(db.clone()).await;
        let an_hour_out = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
            + 3_600_000;
        st.queue.enqueue_at(&db, "regras/global/d", vec!["## a".to_string()], an_hour_out, 1, 0);

        let Json(v) = mcp_status(State(st.clone())).await;
        assert_eq!(
            (v["queue"]["pending_len"].as_u64(), v["queue"]["ready_len"].as_u64()),
            (Some(1), Some(0)),
            "a deferred queue must read differently from an empty one: {}",
            v["queue"]
        );

        // And the contrast that gives the pair its meaning: with nothing owed, both
        // are zero. Without this half the assertion above would also hold for a
        // queue that had simply lost the job.
        st.queue.drain().await;
        let Json(v) = mcp_status(State(st)).await;
        assert_eq!(
            (v["queue"]["pending_len"].as_u64(), v["queue"]["ready_len"].as_u64()),
            (Some(1), Some(0)),
            "a deferred job is not ready now, which is the whole distinction: {}",
            v["queue"]
        );
        let _ = std::fs::remove_file(&db);
    }

    /// X-02.5: a queue that gave up on a note has to be visible in `status`.
    ///
    /// This is the state that had no representation at all: the job left the queue,
    /// the chunks stayed `NULL`, and the only evidence was a log line. An operator
    /// reading `status` saw a queue with nothing in it and a coverage below 100%,
    /// with no way to tell "nothing owed" from "gave up".
    #[tokio::test]
    async fn status_reports_a_queue_that_gave_up_on_a_note() {
        let db = tmp_db("queue-dead");
        // A queue that cannot embed anything, and gives up after one attempt.
        let queue = std::sync::Arc::new(EmbedQueue::with_max_failures(
            brain_embed::EmbeddingEngine::new("http://127.0.0.1:1".into(), "nomic-embed-text".into()),
            1,
        ));
        {
            let store = Store::open(&db).unwrap();
            store
                .note_upsert("regras/global/dead", "regras", Some("global"), "## one\n\nalpha", None, &[], false, None)
                .unwrap();
            let nid = store.note_id("regras/global/dead").unwrap().unwrap();
            store
                .chunk_insert(nid, "regras/global/dead", "regras", Some("global"), "## one\n\nalpha", 0, 1, None, &[], None)
                .unwrap();
        }
        // What a write path does: the note is stored with a NULL vector and the
        // path is queued for embedding. With nothing queued there is no job to
        // fail, which is the point of the next assertion.
        queue.enqueue(&db, "regras/global/dead", vec!["## one\n\nalpha".to_string()]);
        let out = queue.drain().await;
        assert_eq!(out.dead_lettered, 1, "precondition: the note was given up on: {out:?}");

        let st = AppState { db: db.clone(), queue };
        let Json(v) = mcp_status(State(st)).await;
        let q = &v["queue"];
        assert_eq!(q["dead_lettered"], 1, "status must show the dead letter: {q}");
        assert_eq!(q["max_failures"], 1, "and the cap it reached: {q}");
        let last = &q["last_drain"];
        assert_eq!(last["dead_lettered"], 1, "the last pass must say so too: {last}");
        assert_eq!(last["nulls"], 1, "including the chunks it will not fix: {last}");
        assert_eq!(last["settled"], true, "a dead letter is terminal, and that is reported as settled: {last}");
        // And the debt is still on the coverage side, so the two halves agree.
        assert_eq!(v["embedding"]["coverage"]["chunks_without_embedding"], 1, "{v}");
        let _ = std::fs::remove_file(&db);
    }

    // ------------------------------------------------------------------
    // V-01: write-side size limits, enforced before anything is written or
    // embedded. A note with no ceiling is a self-inflicted DoS: `chunk_text`
    // splits on `## ` with no bound, and embedding is serial at ~0.4s a chunk,
    // so a multi-megabyte body held a request open for minutes.
    // ------------------------------------------------------------------

    /// A body over `MAX_CHUNKS` that stays under `MAX_CONTENT_BYTES`, i.e. the
    /// shape the byte ceiling alone would wave through.
    fn oversize_body() -> String {
        let mut body = String::new();
        for i in 0..(brain_core::MAX_CHUNKS + 1) {
            body.push_str(&format!("## {i}\n"));
        }
        assert!(body.len() < brain_core::MAX_CONTENT_BYTES, "the fixture must exercise the chunk cap, not the byte cap");
        body
    }

    #[tokio::test]
    async fn test_mcp_store_rejects_a_note_over_the_chunk_limit_without_embedding_it() {
        let db = tmp_db("oversize");
        let (engine, hits, task) = counting_mock().await;
        let queue = Arc::new(EmbedQueue::new(engine));
        let st = AppState { db: db.clone(), queue };
        let r = mcp_store(State(st.clone()), Json(store_body(oversize_body()))).await;
        let status = r.into_response().status();
        task.abort();
        assert_eq!(status, StatusCode::BAD_REQUEST, "an oversized note must be refused, not embedded");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0, "the limit must be checked before any embedding request");
        assert_eq!(st.queue.pending_len(), 0, "a refused note must not be queued for embedding either");
        assert!(Store::open(&db).unwrap().note_get("regras/global/oversize").unwrap().is_none(), "a refused note must leave no trace");
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_mcp_store_rejects_a_note_over_the_byte_limit() {
        let db = tmp_db("oversize-bytes");
        let (engine, hits, task) = counting_mock().await;
        let st = AppState { db: db.clone(), queue: Arc::new(EmbedQueue::new(engine)) };
        let r = mcp_store(State(st.clone()), Json(store_body("x".repeat(brain_core::MAX_CONTENT_BYTES + 1)))).await;
        let status = r.into_response().status();
        task.abort();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0, "the limit must be checked before any embedding request");
        assert!(Store::open(&db).unwrap().note_get("regras/global/oversize").unwrap().is_none());
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_mcp_store_accepts_a_note_at_the_limit_and_reports_what_is_queued() {
        let db = tmp_db("atlimit");
        // A mock, not the environment: this test used to fire 60 real embedding
        // requests at whatever Ollama the developer happened to be running, via the
        // `global_queue()` the handler reached for.
        let (engine, hits, task) = counting_mock().await;
        let st = AppState { db: db.clone(), queue: Arc::new(EmbedQueue::new(engine)) };
        // 60 sections: 6x the largest note in the real corpus, inside the budget.
        use std::fmt::Write as _;
        let mut content = String::new();
        for i in 0..60 {
            let _ = writeln!(content, "## section {i}\n\nprose {i}");
        }
        let v = body_of(mcp_store(State(st.clone()), Json(store_body(content))).await).await;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["chunks"], 60, "one chunk per `## ` section: {v}");
        // US-02.7: the response reports the debt and does not pretend a vector
        // already exists, and the write itself issues no embedding request.
        assert!(v["queued"].as_u64().unwrap() > 0, "a fresh note owes every chunk a vector: {v}");
        assert_eq!(v["without_embedding"], v["chunks"], "{v}");
        assert_eq!(v["embedded"], 0, "nothing can be embedded before the answer: {v}");
        assert!(hits.load(std::sync::atomic::Ordering::SeqCst) <= 60, "the write must not have embedded the note inline");
        let cov = Store::open(&db).unwrap().embedding_coverage().unwrap();
        assert_eq!(cov.zero_vector, 0, "a queued chunk is NULL, never a BLOB of zeros");
        task.abort();
        let _ = std::fs::remove_file(&db);
    }

    // ------------------------------------------------------------------
    // The shared write rule every writer goes through — including the session
    // hook, which used to be the one path that forgot the limits.
    // ------------------------------------------------------------------

    #[test]
    fn the_write_rule_covers_every_layer_shape() {
        assert_eq!(validate_note_write("regras", "a/b", Some("global"), "## ok").unwrap(), "regras/global/a/b");
        assert_eq!(validate_note_write("sessoes", "p/2026-01-01", None, "## ok").unwrap(), "sessoes/p/2026-01-01");
        assert!(validate_note_write("regras", "a", None, "## ok").is_err(), "scope is mandatory for regras");
        assert!(validate_note_write("nope", "a", None, "## ok").is_err(), "unknown layer");
        assert!(validate_note_write("regras", "../a", Some("global"), "## ok").is_err(), "traversal");
        assert!(validate_note_write("regras", "a", Some("nowhere"), "## ok").is_err(), "unknown scope");
        assert!(validate_note_write("regras", "a", Some("global"), &"## x\n".repeat(brain_core::MAX_CHUNKS + 1)).is_err(), "chunk cap");
        assert!(validate_note_write("regras", "a", Some("global"), &"x".repeat(brain_core::MAX_CONTENT_BYTES + 1)).is_err(), "byte cap");
    }
}
