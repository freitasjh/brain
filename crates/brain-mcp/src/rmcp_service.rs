//! Real MCP server (rmcp 0.5, SSE transport) — the wire protocol AI agents speak.
//! Thin adapters over [`brain_store::Store`]; same semantics as the debug REST routes.

use std::sync::Arc;

use brain_store::Store;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::*,
    tool, tool_handler, tool_router,
    transport::sse_server::SseServer,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::embed_queue::EmbedQueue;
use crate::fs_guard;

#[derive(Clone)]
pub struct Brain {
    db: String,
    /// The embedding queue, injected rather than looked up in a process global.
    ///
    /// Two reasons, one production and one testability. Production: the same
    /// `EmbeddingEngine` serves the queue, the query embedding and the status
    /// probe, so there is one client and one place the model is configured. Tests:
    /// every tool handler used to reach for `global_queue()` / `from_env()`, so
    /// calling `brain_store` in a test fired real embedding requests at whatever
    /// `BRAIN_OLLAMA_URL` pointed at, and the only way to redirect that was to
    /// mutate process-global state from a parallel test.
    queue: Arc<EmbedQueue>,
    tool_router: ToolRouter<Self>,
}

// ---------- args ----------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StoreArgs {
    /// Layer: arquitetura|regras|sessoes|projetos|estudos|indexacao
    pub layer: String,
    /// Relative path without .md (e.g. "projeto/assunto")
    pub path: String,
    /// Markdown content
    pub content: String,
    /// Required for arquitetura/regras/estudos: projetos|global
    pub scope: Option<String>,
    pub project: Option<String>,
    pub tags: Option<Vec<String>>,
    pub pinned: Option<bool>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// Full note path (e.g. "regras/global/coding-standards")
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    pub query: String,
    pub layer: Option<String>,
    pub scope: Option<String>,
    pub project: Option<String>,
    pub tag: Option<String>,
    pub top_k: Option<u8>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteArgs {
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecentArgs {
    pub top_k: Option<u8>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CheckpointsArgs {
    pub limit: Option<u8>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RestoreArgs {
    pub id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BackupArgs {
    pub to: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExportArgs {
    pub to: Option<String>,
    pub force: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForgetSweepArgs {
    pub dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectArgs {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectLinkArgs {
    pub note_path: String,
    pub project: String,
}

// ---------- helpers ----------

fn open(db: &str) -> Result<Store, McpError> {
    Store::open(db).map_err(|e| McpError::internal_error(e.to_string(), None))
}

fn ok(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(
        serde_json::to_string(&v).unwrap(),
    )]))
}

fn bad(msg: String) -> McpError {
    McpError::invalid_params(msg, None)
}

fn fail(e: impl ToString) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

/// Embeds a query, degrading to `None` (FTS-only search) when Ollama is
/// unavailable or slow.
///
/// Budget comes from [`brain_embed::EmbeddingEngine::batch_timeout`], not a
/// hardcoded 3s: 3s expires during a cold `nomic-embed-text` load, which
/// silently downgraded every semantic query to a keyword one.
async fn embed_query(eng: &brain_embed::EmbeddingEngine, query: &str) -> Option<Vec<f32>> {
    match tokio::time::timeout(eng.batch_timeout(1), eng.embed(query)).await {
        Ok(Ok(v)) => Some(v),
        Ok(Err(e)) => {
            eprintln!("brain: query not embedded (FTS only): {e:#}");
            None
        }
        Err(_) => {
            eprintln!("brain: query embedding timed out (FTS only)");
            None
        }
    }
}

// ---------- service ----------

#[tool_router]
impl Brain {
    /// Production service: the env-configured queue.
    pub fn new(db: String) -> Self {
        Self::with_queue(db, crate::global_queue())
    }

    /// Service backed by an explicit queue. The seam tests use instead of
    /// mutating process-global state.
    pub fn with_queue(db: String, queue: Arc<EmbedQueue>) -> Self {
        Self {
            db,
            queue,
            tool_router: Self::tool_router(),
        }
    }

    fn store(&self) -> Result<Store, McpError> {
        open(&self.db)
    }

    #[tool(description = "Health-check, always returns pong")]
    async fn ping(&self) -> Result<CallToolResult, McpError> {
        ok(serde_json::json!({"pong": true}))
    }

    #[tool(description = "DB counts: notes, chunks, projects, plus embedding coverage, queue state and Ollama health")]
    async fn brain_status(&self) -> Result<CallToolResult, McpError> {
        let s = self.store()?;
        let notes = s.count_notes().map_err(fail)?;
        let chunks = s.count_chunks().map_err(fail)?;
        let projects = s.project_list().map_err(fail)?.len();
        // `Store` is !Sync (rusqlite's statement cache is a RefCell), so the
        // borrow must end before the health probe's `.await` or this tool's
        // future stops being Send and rmcp cannot spawn it.
        let cov = s.embedding_coverage().unwrap_or_default();
        let lock = s.embed_lock_state();
        drop(s);
        let coverage = crate::coverage_status(&cov);
        ok(serde_json::json!({
            "notes": notes,
            "chunks": chunks,
            "projects": projects,
            "embedding": { "coverage": coverage, "ollama": crate::ollama_status_with(self.queue.engine()).await },
            // W-04e. Without this block a wedged queue is indistinguishable from a
            // healthy one: the chunk counts stay right while `coverage_pct` sits
            // below 100 and nothing is coming.
            "queue": crate::queue_status(&self.queue, lock),
        }))
    }

    #[tool(description = "Save a markdown note (auto-indexed for semantic search)")]
    async fn brain_store(&self, Parameters(a): Parameters<StoreArgs>) -> Result<CallToolResult, McpError> {
        // The shared write rule: layer/path/scope validation and the size limits,
        // before a store is opened and before any embed. `MAX_CHUNKS` is what
        // bounds the work a single note can ask for, and the check is pure and
        // synchronous, so an oversized note is refused in microseconds rather than
        // after an embed pass nobody wanted. The returned path is the value that
        // gets written; this call is the fail-fast guard, not a duplicate of the
        // logic inside `store_note_and_queue`.
        crate::validate_note_write(&a.layer, &a.path, a.scope.as_deref(), &a.content).map_err(|e| bad(e.to_string()))?;
        let tags = a.tags.unwrap_or_default();

        // No network call, and no `&Store` anywhere near one: the note and its
        // chunk rows are written with `embedding = NULL`, FTS5 is populated in
        // the same transaction, and the vectors are queued (US-02.7). This is the
        // path AI agents actually take, and it used to write a BLOB-of-zeros for
        // every chunk while Ollama sat right there running, so semantic search
        // over MCP-stored notes returned nothing but noise.
        let s = self.store()?;
        let outcome = crate::store_note_and_queue(&s, crate::NoteWrite {
            layer: &a.layer, path: &a.path, scope: a.scope.as_deref(), content: &a.content,
            project: a.project.as_deref(), tags: &tags, pinned: a.pinned.unwrap_or(false),
            expires_at: a.expires_at.as_deref(),
        }, &self.queue).map_err(fail)?;
        self.queue.spawn_worker();
        // `queued` is the honest contract: `embedded` is 0 on a fresh note, and
        // the vector arrives a moment later. Callers that need the vector now
        // should re-read `brain_status.embedding.coverage`.
        ok(serde_json::json!({
            "ok": true,
            "path": outcome.path,
            "chunks": outcome.chunks,
            "embedded": outcome.embedded,
            "without_embedding": outcome.nulls,
            "queued": outcome.queued,
        }))
    }

    #[tool(description = "Read a full note by path")]
    async fn brain_read(&self, Parameters(a): Parameters<ReadArgs>) -> Result<CallToolResult, McpError> {
        brain_core::sanitize_relative_path(&a.path).map_err(|e| bad(e.to_string()))?;
        let s = self.store()?;
        match s.note_get(&a.path).map_err(fail)? {
            Some(n) => ok(serde_json::json!({"path": n.path, "content": n.content, "layer": n.layer, "scope": n.scope})),
            None => Err(bad(format!("not found: {}", a.path))),
        }
    }

    #[tool(description = "Hybrid semantic search (FTS5+vector RRF). Call BEFORE coding.")]
    async fn brain_search(&self, Parameters(a): Parameters<SearchArgs>) -> Result<CallToolResult, McpError> {
        if a.query.trim().is_empty() {
            return Err(bad("query required".to_string()));
        }
        let s = self.store()?;
        let qvec = embed_query(self.queue.engine(), &a.query).await;
        let res = s
            .search(&a.query, qvec.as_deref(), a.layer.as_deref(), a.scope.as_deref(), a.project.as_deref(), a.tag.as_deref(), a.top_k.unwrap_or(5).clamp(1, 20) as usize, false)
            .map_err(fail)?;
        ok(serde_json::json!({"results": res, "total": res.len()}))
    }

    #[tool(description = "Hard-delete a note by path")]
    async fn brain_delete(&self, Parameters(a): Parameters<DeleteArgs>) -> Result<CallToolResult, McpError> {
        brain_core::sanitize_relative_path(&a.path).map_err(|e| bad(e.to_string()))?;
        let s = self.store()?;
        match s.note_delete(&a.path).map_err(fail)? {
            true => ok(serde_json::json!({"ok": true})),
            false => Err(bad(format!("not found: {}", a.path))),
        }
    }

    #[tool(description = "Latest notes by updated_at")]
    async fn brain_recent(&self, Parameters(a): Parameters<RecentArgs>) -> Result<CallToolResult, McpError> {
        let s = self.store()?;
        ok(serde_json::json!({"recent": s.recent(a.top_k.unwrap_or(10) as usize).map_err(fail)?}))
    }

    #[tool(description = "Audit log for time-travel")]
    async fn brain_checkpoints(&self, Parameters(a): Parameters<CheckpointsArgs>) -> Result<CallToolResult, McpError> {
        let s = self.store()?;
        ok(serde_json::json!({"checkpoints": s.checkpoints(a.limit.unwrap_or(10) as usize).map_err(fail)?}))
    }

    #[tool(description = "Restore an audit entry by id")]
    async fn brain_restore(&self, Parameters(a): Parameters<RestoreArgs>) -> Result<CallToolResult, McpError> {
        let s = self.store()?;
        match s.restore_audit(a.id).map_err(fail)? {
            true => ok(serde_json::json!({"ok": true})),
            false => Err(bad(format!("audit not found: {}", a.id))),
        }
    }

    #[tool(description = "Copy brain.db to a .bak file inside the export root (BRAIN_EXPORT_ROOT)")]
    async fn brain_backup(&self, Parameters(a): Parameters<BackupArgs>) -> Result<CallToolResult, McpError> {
        // W-01: the destination was the request's `to` verbatim, which is arbitrary
        // file write as the service user plus a copy of the whole database. Now
        // `{db}.bak` by default, or a `*.bak` inside `BRAIN_EXPORT_ROOT`.
        let dst = fs_guard::backup_file(&self.db, a.to.as_deref()).map_err(|e| bad(e.to_string()))?;
        std::fs::copy(&self.db, &dst).map_err(fail)?;
        ok(serde_json::json!({"ok": true, "to": dst.to_string_lossy()}))
    }

    #[tool(description = "Dump notes to a directory inside the export root (BRAIN_EXPORT_ROOT)")]
    async fn brain_export(&self, Parameters(a): Parameters<ExportArgs>) -> Result<CallToolResult, McpError> {
        // W-01, same allowlist as the REST path and for the same reason: this
        // handler creates a directory and writes every note into it from a
        // client-supplied path.
        let dir = fs_guard::export_dir(a.to.as_deref()).map_err(|e| bad(e.to_string()))?;
        if dir.exists() && !a.force.unwrap_or(false) {
            return Err(bad("export dir exists, use force".to_string()));
        }
        std::fs::create_dir_all(&dir).map_err(fail)?;
        // X-05.3: re-check ownership now the directory exists — see the REST
        // handler for why the check in `export_dir` is not sufficient on its own.
        fs_guard::assert_root_usable(&fs_guard::export_root()).map_err(fail)?;
        let s = self.store()?;
        let mut written = 0usize;
        let mut refused = 0usize;
        for (path, _l, _sc, content) in s.recent(10000).map_err(fail)? {
            let fp = match fs_guard::note_file_within(&dir, &path) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("brain: export skipped {path}: {e:#}");
                    refused += 1;
                    continue;
                }
            };
            if let Some(parent) = fp.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&fp, content).is_ok() { written += 1; } else { refused += 1; }
        }
        ok(serde_json::json!({"ok": true, "to": dir.to_string_lossy(), "written": written, "refused": refused}))
    }

    #[tool(description = "Delete TTL-expired notes (pinned never expires)")]
    async fn brain_forget_sweep(&self, Parameters(a): Parameters<ForgetSweepArgs>) -> Result<CallToolResult, McpError> {
        let s = self.store()?;
        ok(serde_json::json!({"deleted": s.forget_sweep(a.dry_run.unwrap_or(false)).map_err(fail)?}))
    }

    #[tool(description = "Create a project")]
    async fn brain_project_create(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<CallToolResult, McpError> {
        let s = self.store()?;
        match s.project_create(&a.name, a.description.as_deref().unwrap_or("")) {
            Ok(pr) => ok(serde_json::json!(pr)),
            Err(e) => Err(bad(e.to_string())),
        }
    }

    #[tool(description = "List projects")]
    async fn brain_project_list(&self) -> Result<CallToolResult, McpError> {
        let s = self.store()?;
        ok(serde_json::json!(s.project_list().map_err(fail)?))
    }

    #[tool(description = "Notes owned or linked by a project")]
    async fn brain_project_notes(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<CallToolResult, McpError> {
        let s = self.store()?;
        match s.project_notes(&a.name) {
            Ok(v) => ok(serde_json::json!(v)),
            Err(e) => Err(bad(e.to_string())),
        }
    }

    #[tool(description = "Link a note to a project")]
    async fn brain_project_link(&self, Parameters(a): Parameters<ProjectLinkArgs>) -> Result<CallToolResult, McpError> {
        let s = self.store()?;
        match s.note_link_project(&a.note_path, &a.project) {
            Ok(_) => ok(serde_json::json!({"ok": true})),
            Err(e) => Err(bad(e.to_string())),
        }
    }

    #[tool(description = "Unlink a note from a project")]
    async fn brain_project_unlink(&self, Parameters(a): Parameters<ProjectLinkArgs>) -> Result<CallToolResult, McpError> {
        let s = self.store()?;
        match s.note_unlink_project(&a.note_path, &a.project) {
            Ok(v) => ok(serde_json::json!({"ok": v})),
            Err(e) => Err(bad(e.to_string())),
        }
    }
}

#[tool_handler]
impl ServerHandler for Brain {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("Brain: persistent memory for AI agents. ALWAYS brain_search before coding, brain_store decisions after.".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

/// Serve real MCP over SSE on `port` until SIGINT or SIGTERM.
///
/// The wait is built by a `let` **before** `serve_rmcp_sse_with` is called, not
/// inside the argument list, because `shutdown_on_sigint_or_sigterm` installs the
/// signal dispositions when it is *called* — so this arms the handlers ahead of
/// the boot recovery, the bind, and everything else in the body. See that
/// function for why that ordering is the point rather than an accident.
pub async fn serve_rmcp_sse(db: String, port: u16) -> anyhow::Result<()> {
    let shutdown = shutdown_on_sigint_or_sigterm();
    serve_rmcp_sse_with(db, port, crate::global_queue(), shutdown).await
}

/// The shutdown wait every long-running server path uses: SIGINT **or** SIGTERM.
///
/// It is the only signal set in the crate, and its callers are `server start`,
/// [`serve_rmcp_sse`] and the read-only viewer's `serve` subcommand — so all three
/// servers `brain` can run take one path on a `systemctl stop`. (The third caller
/// lives in `brain-cli`, not here, and is the reason this says "every" rather than
/// the "both" this doc used to say while `serve` still died by signal.) SIGINT is
/// what a terminal Ctrl-C sends; SIGTERM is what
/// `systemd` sends, and it is the **default** `KillSignal` of a unit, so a server
/// that waits on `ctrl_c` alone dies by signal on every `systemctl stop`: it never
/// reaches the `ct.cancel()` that follows the wait in [`serve_rmcp_sse_with`], and
/// a `TimeoutStopSec` that expires turns that into a `SIGKILL`. Invisible on a
/// terminal, not invisible under a unit — and the unit files `brain setup systemd`
/// writes are the ones on the other end of it.
///
/// An earlier revision left `serve-mcp` on `ctrl_c` on purpose, on the grounds
/// that changing the shutdown of a unit that is already deployed is the
/// operator's call rather than a bugfix's. The operator took that call and
/// accepted the change, so the asymmetry is gone and the doc that recorded it —
/// which told a reader that `serve-mcp` did *not* handle SIGTERM — is gone with
/// it.
///
/// Waits for SIGINT **or** SIGTERM, and registers **SIGTERM only** eagerly.
///
/// # Only SIGTERM is eager, and the difference is not cosmetic
///
/// This is a function that *registers and then returns* a wait, rather than an
/// `async fn` whose body does both — and that only pays off for SIGTERM. The
/// `tokio::signal::unix::signal(..)` stream is constructed by the statement
/// below, on the caller's thread, before the returned future is ever polled, so
/// calling this arms SIGTERM at the call site. The `ctrl_c()` arm sits **inside**
/// the `async move` block, and an `async fn` body does not run until the future
/// is polled, so SIGINT is registered on first poll like any other.
///
/// An `async fn` would therefore register *neither* signal until its first poll,
/// leaving a window in which the process has done work and has no handler
/// installed — and a `systemctl stop` landing in that window is the default
/// action. So the shape earns its keep exactly once, for the signal that
/// `systemd` actually sends, and the honest summary is **SIGTERM** rather than
/// "either signal": `main.rs` says the same thing at the `serve` call site, six
/// hundred lines from an earlier revision of this doc that claimed both were
/// eager. Callers invoke it **before** the work they want interruptible: the
/// legacy import in `server start`, and the boot recovery and bind in
/// [`serve_rmcp_sse`].
///
/// The residual asymmetry for SIGINT is a window between the call and the first
/// poll. A Ctrl-C inside it takes the default action; a SIGTERM does not. Strictly
/// better than being unreached, and not worth an `async` block per signal to close.
///
/// Both signals are raced once polled, so a terminal Ctrl-C and a `systemctl stop`
/// take one path: [`serve_rmcp_sse_with`] gets one resolved future either way, and
/// the cancel-token teardown it performs runs exactly once.
pub fn shutdown_on_sigint_or_sigterm() -> impl std::future::Future<Output = ()> {
    // Constructing the stream installs the signal disposition. This statement
    // runs now, on the caller's thread, before the returned future is polled.
    let mut sigterm = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(s) => Some(s),
        // A platform without `SignalKind::terminate` still gets SIGINT, so the
        // wait is not turned into a hang.
        Err(e) => {
            eprintln!("brain: cannot listen for SIGTERM ({e}); waiting for SIGINT only");
            None
        }
    };
    async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = async {
                match sigterm.as_mut() {
                    Some(s) => { s.recv().await; }
                    // Nothing to race against: park forever so the other arm wins.
                    None => std::future::pending::<()>().await,
                }
            } => {}
        }
    }
}

/// X-04. The server, with its two ambient dependencies passed in.
///
/// `queue` and `shutdown` are parameters so that the *boot path itself* is
/// testable. The previous shape ran the boot recovery inline in
/// `serve_rmcp_sse` and then waited for a terminal interrupt forever, which meant
/// the only way to observe it was to start a server and hope — so a test could
/// call `EmbedQueue::recover` directly, prove the function works, and prove
/// nothing about whether the server calls it. The reviewer deleted that one line
/// and all 213 tests stayed green.
///
/// Now a test runs this exact function on an ephemeral port with a shutdown that
/// resolves immediately, and asserts the work was recovered. Deleting the boot
/// call from *here* fails the test; deleting it from `serve_rmcp_sse` fails it too,
/// because this is what `serve_rmcp_sse` delegates to.
pub async fn serve_rmcp_sse_with(
    db: String,
    port: u16,
    queue: Arc<EmbedQueue>,
    shutdown: impl std::future::Future<Output = ()> + Send,
) -> anyhow::Result<()> {
    // W-04a, boot recovery. The queue's work list is in memory, so anything that was
    // in flight when the process last stopped — a deploy, an OOM, a `kill -9` — was
    // gone, and nothing else would ever re-read it: `coverage_pct` would sit below
    // 100% until a human noticed or ran `reindex --all` by hand. The owed work is
    // recoverable from the database alone, because a `NULL` chunk *is* the record
    // of it, so the server asks for it before accepting a connection.
    let recovered = crate::boot_queue(&queue, &db);
    let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse()?;
    let served_db = db.clone();
    let ct = SseServer::serve(addr).await?.with_service(move || Brain::with_queue(served_db.clone(), Arc::clone(&queue)));
    println!("mcp SSE http://0.0.0.0:{}/sse (protocol MCP 2024-11)", port);
    if recovered > 0 {
        println!("boot recovery: {recovered} note(s) re-queued for embedding");
    }
    shutdown.await;
    ct.cancel();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::RawContent;

    fn text_of(r: &CallToolResult) -> &str {
        let first = r.content.as_ref().unwrap().first().unwrap();
        match &first.raw {
            RawContent::Text(t) => &t.text,
            _ => panic!("expected text content"),
        }
    }

    fn tmp_db(tag: &str) -> String {
        let db = format!("/tmp/brain-rmcp-{}-{}.db", std::process::id(), tag);
        let _ = std::fs::remove_file(&db);
        db
    }

    /// A queue pointed at a port nobody is listening on.
    ///
    /// Every tool test uses one. The handlers used to build their engine from the
    /// environment, so a test of `brain_store` issued real embedding requests to
    /// whatever `BRAIN_OLLAMA_URL` named — 60 of them in one case — and the only
    /// way to redirect that was to mutate process-global state from a test that
    /// runs in parallel with others.
    async fn dead_queue() -> Arc<EmbedQueue> {
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
        let addr = probe.local_addr().unwrap();
        drop(probe);
        Arc::new(EmbedQueue::new(brain_embed::EmbeddingEngine::new(format!("http://{addr}"), "nomic-embed-text".into())))
    }

    async fn brain(db: &str) -> Brain {
        Brain::with_queue(db.to_string(), dead_queue().await)
    }

    /// A scratch directory inside the real export root.
    fn export_scratch(tag: &str) -> std::path::PathBuf {
        let p = crate::fs_guard::export_root().join(format!("brain-rmcp-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create scratch inside the export root");
        p.canonicalize().expect("canonicalize scratch")
    }

    #[tokio::test]
    async fn test_ping_and_status() {
        let db = tmp_db("ping");
        let b = brain(&db).await;
        assert!(b.ping().await.is_ok());
        let r = b.brain_status().await.unwrap();
        let v: serde_json::Value = serde_json::from_str(text_of(&r)).unwrap();
        assert_eq!(v["notes"], 0);
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_validation_errors() {
        let db = tmp_db("val");
        let b = brain(&db).await;
        // empty query
        assert!(b.brain_search(Parameters(SearchArgs { query: "  ".into(), layer: None, scope: None, project: None, tag: None, top_k: None })).await.is_err());
        // missing scope
        assert!(b.brain_store(Parameters(StoreArgs { layer: "regras".into(), path: "x".into(), content: "## h".into(), scope: None, project: None, tags: None, pinned: None, expires_at: None })).await.is_err());
        // traversal
        assert!(b.brain_store(Parameters(StoreArgs { layer: "regras".into(), path: "../e".into(), content: "## h".into(), scope: Some("global".into()), project: None, tags: None, pinned: None, expires_at: None })).await.is_err());
        // read missing
        assert!(b.brain_read(Parameters(ReadArgs { path: "regras/global/nope".into() })).await.is_err());
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_all_tools_smoke() {
        let db = tmp_db("all");
        let b = brain(&db).await;
        // status empty
        assert!(b.brain_status().await.is_ok());
        // recent empty + checkpoints empty
        assert!(b.brain_recent(Parameters(RecentArgs { top_k: None })).await.is_ok());
        assert!(b.brain_checkpoints(Parameters(CheckpointsArgs { limit: None })).await.is_ok());
        // TTL + sweep dry + real
        assert!(b.brain_store(Parameters(StoreArgs { layer: "regras".into(), path: "rmcp/ttl".into(), content: "## ttl".into(), scope: Some("global".into()), project: None, tags: None, pinned: None, expires_at: Some("2000-01-01T00:00:00Z".into()) })).await.is_ok());
        let r = b.brain_forget_sweep(Parameters(ForgetSweepArgs { dry_run: Some(true) })).await.unwrap();
        assert!(text_of(&r).contains("rmcp/ttl"));
        assert!(b.brain_forget_sweep(Parameters(ForgetSweepArgs { dry_run: Some(false) })).await.is_ok());
        // restore missing -> err; create+delete -> restore delete audit
        assert!(b.brain_restore(Parameters(RestoreArgs { id: 999999 })).await.is_err());
        assert!(b.brain_store(Parameters(StoreArgs { layer: "sessoes".into(), path: "rmcp/tmp".into(), content: "## t".into(), scope: None, project: None, tags: None, pinned: None, expires_at: None })).await.is_ok());
        assert!(b.brain_delete(Parameters(DeleteArgs { path: "sessoes/rmcp/tmp".into() })).await.is_ok());
        let cps = b.brain_checkpoints(Parameters(CheckpointsArgs { limit: Some(5) })).await.unwrap();
        let cv: serde_json::Value = serde_json::from_str(text_of(&cps)).unwrap();
        let del_id = cv["checkpoints"].as_array().unwrap().iter().find(|c| c[1] == "delete" && c[2] == "sessoes/rmcp/tmp").unwrap()[0].as_i64().unwrap();
        assert!(b.brain_restore(Parameters(RestoreArgs { id: del_id })).await.is_ok());
        assert!(b.brain_read(Parameters(ReadArgs { path: "sessoes/rmcp/tmp".into() })).await.is_ok());
        // backup + export, under the W-01 allowlist: the default destination is a
        // sibling of the database, an export goes inside `BRAIN_EXPORT_ROOT`, and
        // anything else is refused.
        let bak = format!("{}.bak", db);
        assert!(b.brain_backup(Parameters(BackupArgs { to: None })).await.is_ok());
        assert!(std::path::Path::new(&bak).exists());
        assert!(b.brain_backup(Parameters(BackupArgs { to: Some("/etc/brain.bak".into()) })).await.is_err());
        let exp = export_scratch("all-tools");
        let exp = exp.to_string_lossy().into_owned();
        assert!(b.brain_export(Parameters(ExportArgs { to: Some(exp.clone()), force: Some(true) })).await.is_ok());
        assert!(b.brain_export(Parameters(ExportArgs { to: Some(exp.clone()), force: Some(false) })).await.is_err());
        for escaped in ["/etc", "/etc/brain-x", "../brain-escape", "/tmp/brain-rmcp-exp-nope"] {
            assert!(
                b.brain_export(Parameters(ExportArgs { to: Some(escaped.into()), force: Some(true) })).await.is_err(),
                "{escaped} must be refused"
            );
        }
        assert!(!std::path::Path::new("/etc/brain-x").exists());
        // projects
        assert!(b.brain_project_create(Parameters(ProjectArgs { name: "pr".into(), description: None })).await.is_ok());
        assert!(b.brain_project_create(Parameters(ProjectArgs { name: "pr".into(), description: None })).await.is_err());
        assert!(b.brain_project_list().await.is_ok());
        assert!(b.brain_project_notes(Parameters(ProjectArgs { name: "pr".into(), description: None })).await.is_ok());
        assert!(b.brain_project_notes(Parameters(ProjectArgs { name: "nope".into(), description: None })).await.is_err());
        assert!(b.brain_project_link(Parameters(ProjectLinkArgs { note_path: "sessoes/rmcp/tmp".into(), project: "pr".into() })).await.is_ok());
        assert!(b.brain_project_link(Parameters(ProjectLinkArgs { note_path: "sessoes/missing".into(), project: "pr".into() })).await.is_err());
        assert!(b.brain_project_unlink(Parameters(ProjectLinkArgs { note_path: "sessoes/rmcp/tmp".into(), project: "pr".into() })).await.is_ok());
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&bak);
        let _ = std::fs::remove_dir_all(&exp);
    }

    /// The MCP protocol path is what AI agents actually call, and it used to
    /// write a BLOB-of-zeros for every chunk while Ollama was up and healthy —
    /// so every note an agent stored was semantically invisible. These assert the
    /// vector stream is intact afterwards, in any environment.
    async fn assert_no_zero_vectors(db: &str, label: &str) {
        let s = Store::open(db).unwrap();
        let cov = s.embedding_coverage().unwrap();
        assert_eq!(cov.zero_vector, 0, "{label}: a BLOB-of-zeros reached the index");
        assert_eq!(cov.embedded + cov.without_embedding, cov.total, "{label}: chunks must be embedded or NULL, got {:?}", cov);
    }

    #[tokio::test]
    async fn test_brain_store_reports_chunk_and_embedding_counts() {
        let db = tmp_db("storevec");
        let b = brain(&db).await;
        let r = b.brain_store(Parameters(StoreArgs { layer: "regras".into(), path: "rmcp/vec".into(), content: "## protocol path vectorstest".into(), scope: Some("global".into()), project: None, tags: None, pinned: None, expires_at: None })).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(text_of(&r)).unwrap();
        assert!(v["chunks"].as_u64().unwrap() >= 1, "tool must report how many chunks it wrote: {}", v);
        // embedded + without_embedding == chunks, and never a zero vector.
        assert_eq!(v["embedded"].as_u64().unwrap() + v["without_embedding"].as_u64().unwrap(), v["chunks"].as_u64().unwrap(), "{}", v);
        assert_no_zero_vectors(&db, "rmcp brain_store").await;
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_brain_status_exposes_embedding_coverage_and_ollama() {
        let db = tmp_db("statusvec");
        let b = brain(&db).await;
        assert!(b.brain_store(Parameters(StoreArgs { layer: "regras".into(), path: "rmcp/cov".into(), content: "## coverage".into(), scope: Some("global".into()), project: None, tags: None, pinned: None, expires_at: None })).await.is_ok());
        let r = b.brain_status().await.unwrap();
        let v: serde_json::Value = serde_json::from_str(text_of(&r)).unwrap();
        let cov = &v["embedding"]["coverage"];
        for key in ["chunks_total", "chunks_embedded", "chunks_without_embedding", "chunks_zero_vector", "embedding_coverage_pct"] {
            assert!(!cov[key].is_null(), "brain_status.embedding.coverage.{} missing from {}", key, v);
        }
        assert_eq!(cov["chunks_zero_vector"], 0, "{}", v);
        assert!(v["embedding"]["ollama"]["reachable"].is_boolean(), "{}", v);
        let _ = std::fs::remove_file(&db);
    }

    /// W-04e on the protocol path AI agents actually call. Every other number in
    /// `brain_status` stays healthy while the queue is wedged, so the queue block
    /// is the only thing that distinguishes "fine" from "nothing is coming".
    #[tokio::test]
    async fn test_brain_status_exposes_the_queue_state() {
        let db = tmp_db("queuestatus");
        let b = brain(&db).await;
        assert!(b.brain_store(Parameters(StoreArgs { layer: "regras".into(), path: "rmcp/q".into(), content: "## queued".into(), scope: Some("global".into()), project: None, tags: None, pinned: None, expires_at: None })).await.is_ok());
        let r = b.brain_status().await.unwrap();
        let v: serde_json::Value = serde_json::from_str(text_of(&r)).unwrap();
        let q = v["queue"].as_object().expect("status.queue must be an object");
        for key in ["pending_len", "ready_len", "is_draining", "embed_lock_holder", "embed_lock_age_s", "embed_lock_expires_in_s"] {
            assert!(q.contains_key(key), "brain_status.queue.{} missing from {}", key, v);
        }
        // The lock fields are legitimately `null` when nobody holds the lock; what
        // must be true is that they are *present*, so an operator can tell "free"
        // from "not implemented".
        assert!(q["is_draining"].is_boolean(), "{}", v["queue"]);
        assert!(q["pending_len"].is_u64(), "{}", v["queue"]);
        assert!(q["ready_len"].is_u64(), "{}", v["queue"]);
        // The debt this block exists to make visible: the note just stored owes a
        // vector, and every other number in the payload still looks healthy.
        assert_eq!(v["embedding"]["coverage"]["chunks_without_embedding"], 1, "{}", v);
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_brain_store_is_idempotent_about_chunk_rows() {
        // brain_store twice on the same path: `chunks_sync` upserts on
        // (path, chunk_index) and prunes, so the second write must not trip the
        // unique index or duplicate rows.
        let db = tmp_db("twice");
        let b = brain(&db).await;
        let args = || Parameters(StoreArgs { layer: "regras".into(), path: "rmcp/twice".into(), content: "## written twice".into(), scope: Some("global".into()), project: None, tags: None, pinned: None, expires_at: None });
        assert!(b.brain_store(args()).await.is_ok());
        assert!(b.brain_store(args()).await.is_ok());
        let s = Store::open(&db).unwrap();
        assert_eq!(s.count_chunks().unwrap(), 1, "re-storing must not duplicate chunk rows");
        assert_no_zero_vectors(&db, "double store").await;
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_brain_store_shrinking_content_prunes_stale_chunks() {
        // Upsert-then-prune must drop rows past the new chunk count; otherwise a
        // shortened note keeps vectors for text it no longer contains.
        let db = tmp_db("shrink");
        let b = brain(&db).await;
        let long = "## one\n\n## two\n\n## three\n\n## four";
        assert!(b.brain_store(Parameters(StoreArgs { layer: "regras".into(), path: "rmcp/shrink".into(), content: long.into(), scope: Some("global".into()), project: None, tags: None, pinned: None, expires_at: None })).await.is_ok());
        let before = Store::open(&db).unwrap().count_chunks().unwrap();
        assert!(before > 1, "fixture must produce several chunks, got {}", before);
        assert!(b.brain_store(Parameters(StoreArgs { layer: "regras".into(), path: "rmcp/shrink".into(), content: "## only one now".into(), scope: Some("global".into()), project: None, tags: None, pinned: None, expires_at: None })).await.is_ok());
        let s = Store::open(&db).unwrap();
        assert_eq!(s.count_chunks().unwrap(), 1, "stale chunks past the new count must be pruned");
        assert_eq!(s.embedding_coverage().unwrap().zero_vector, 0);
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_brain_store_preserves_vectors_across_an_unrelated_edit() {
        // The hook writes an append-only session note over and over. Each write
        // must not throw away the vectors of the sections that did not change.
        let db = tmp_db("preserve");
        let b = brain(&db).await;
        let mk = |content: &str| Parameters(StoreArgs { layer: "sessoes".into(), path: "rmcp/keep".into(), content: content.into(), scope: None, project: None, tags: None, pinned: None, expires_at: None });
        let first = "## stable first section";
        assert!(b.brain_store(mk(first)).await.is_ok());
        let embedded_once = {
            let s = Store::open(&db).unwrap();
            let c = s.embedding_coverage().unwrap();
            c.embedded + c.without_embedding // total, environment independent
        };
        assert!(b.brain_store(mk(&format!("{}\n\n## appended second", first))).await.is_ok());
        let s = Store::open(&db).unwrap();
        assert_eq!(s.count_chunks().unwrap(), embedded_once + 1, "the new section adds a chunk");
        assert_eq!(s.embedding_coverage().unwrap().zero_vector, 0);
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_store_search_delete_flow() {
        let db = tmp_db("flow");
        let b = brain(&db).await;
        assert!(b.brain_store(Parameters(StoreArgs { layer: "regras".into(), path: "rmcp/flow".into(), content: "## rmcp flow test".into(), scope: Some("global".into()), project: None, tags: None, pinned: None, expires_at: None })).await.is_ok());
        assert!(b.brain_read(Parameters(ReadArgs { path: "regras/global/rmcp/flow".into() })).await.is_ok());
        let r = b.brain_search(Parameters(SearchArgs { query: "rmcp flow".into(), layer: None, scope: None, project: None, tag: None, top_k: Some(5) })).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(text_of(&r)).unwrap();
        assert_eq!(v["total"], 1);
        assert!(b.brain_delete(Parameters(DeleteArgs { path: "regras/global/rmcp/flow".into() })).await.is_ok());
        assert!(b.brain_delete(Parameters(DeleteArgs { path: "regras/global/rmcp/flow".into() })).await.is_err());
        let _ = std::fs::remove_file(&db);
    }

    // ------------------------------------------------------------------
    // V-01 / US-02.7 on the protocol path — the one AI agents actually call.
    // ------------------------------------------------------------------

    fn args_with(content: String) -> Parameters<StoreArgs> {
        Parameters(StoreArgs { layer: "regras".into(), path: "rmcp/limits".into(), content, scope: Some("global".into()), project: None, tags: None, pinned: None, expires_at: None })
    }

    /// A body over `MAX_CHUNKS` that stays under `MAX_CONTENT_BYTES`.
    fn oversize_body() -> String {
        let mut body = String::new();
        for i in 0..(brain_core::MAX_CHUNKS + 1) {
            body.push_str(&format!("## {i}\n"));
        }
        assert!(body.len() < brain_core::MAX_CONTENT_BYTES);
        body
    }

    #[tokio::test]
    async fn test_brain_store_rejects_a_note_over_the_chunk_limit() {
        let db = tmp_db("oversize");
        let b = brain(&db).await;
        let err = b.brain_store(args_with(oversize_body())).await.expect_err("an oversized note must be refused as a parameter error");
        let msg = err.to_string();
        assert!(msg.contains("chunks, limit is"), "the error must name the limit: {msg}");
        // Nothing written, so nothing queued and nothing embedded.
        assert!(Store::open(&db).unwrap().note_get("regras/global/rmcp/limits").unwrap().is_none());
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_brain_store_rejects_a_note_over_the_byte_limit() {
        let db = tmp_db("oversize-bytes");
        let b = brain(&db).await;
        let err = b.brain_store(args_with("x".repeat(brain_core::MAX_CONTENT_BYTES + 1))).await.expect_err("an oversized note must be refused");
        assert!(err.to_string().contains("content too large"), "got: {}", err);
        assert!(Store::open(&db).unwrap().note_get("regras/global/rmcp/limits").unwrap().is_none());
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn test_brain_store_reports_what_it_queued_rather_than_promising_a_vector() {
        // US-02.7: the caller must be able to tell "saved and embedded" from
        // "saved, vector pending" without a second round trip.
        let db = tmp_db("queued");
        let b = brain(&db).await;
        let r = b.brain_store(args_with("## one\n\nalpha\n## two\n\nbeta".into())).await.unwrap();
        let v: serde_json::Value = serde_json::from_str(text_of(&r)).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["chunks"], 2, "{v}");
        assert_eq!(v["embedded"], 0, "a fresh note has no vector yet, and the tool says so: {v}");
        assert_eq!(v["without_embedding"], 2, "{v}");
        assert_eq!(v["queued"], 2, "the outstanding debt is reported, not hidden: {v}");
        // The note is readable and searchable by keyword immediately.
        assert!(b.brain_read(Parameters(ReadArgs { path: "regras/global/rmcp/limits".into() })).await.is_ok());
        let sr = b.brain_search(Parameters(SearchArgs { query: "alpha".into(), layer: None, scope: None, project: None, tag: None, top_k: Some(5) })).await.unwrap();
        let sv: serde_json::Value = serde_json::from_str(text_of(&sr)).unwrap();
        assert_eq!(sv["total"], 1, "FTS works while the queue is still pending");
        assert_no_zero_vectors(&db, "queued store").await;
        let _ = std::fs::remove_file(&db);
    }

    // ------------------------------------------------------------------
    // X-04: the boot path itself.
    //
    // `a_restart_requeues_the_work_that_was_in_flight` in `tests/embed_queue.rs`
    // calls `EmbedQueue::recover` directly, so it proves the *function* works and
    // says nothing about whether the server calls it. The reviewer deleted that one
    // line from the server's startup and every test in the suite stayed green.
    //
    // This runs the real startup instead: `serve_rmcp_sse_with` is the function
    // `serve_rmcp_sse` delegates to, so deleting the boot call from either one
    // fails here.
    // ------------------------------------------------------------------

    /// A note left owing a vector, exactly as a killed process leaves it.
    ///
    /// Built through the store's own API rather than by raw SQL, and the chunk
    /// rows are the ones `chunk_text` actually produces for this content — the
    /// recovery diff works by re-chunking the note and comparing, so a fixture
    /// whose rows disagree with the content would report no debt at all and the
    /// test would pass for the wrong reason.
    fn note_owed_a_vector(db: &str) {
        use brain_core::EMBEDDING_DIM;
        let content = "## one\n\nalpha\n\n## two\n\nbeta";
        let store = Store::open(db).unwrap();
        let path = "regras/global/boot";
        store.note_upsert(path, "regras", Some("global"), content, None, &[], false, None).unwrap();
        let nid = store.note_id(path).unwrap().unwrap();
        let chunks = brain_core::chunk_text(content, 4096);
        assert_eq!(chunks.len(), 2, "the fixture must produce exactly two chunks, got {}", chunks.len());
        let v: Vec<f32> = (0..EMBEDDING_DIM).map(|i| ((i % 17 + 1) as f32) / 16.0).collect();
        // The first chunk is embedded; the second is SQL NULL — interrupted work.
        store.chunk_insert(nid, path, "regras", Some("global"), &chunks[0], 0, 2, None, &[], Some(&v)).unwrap();
        store.chunk_insert(nid, path, "regras", Some("global"), &chunks[1], 1, 2, None, &[], None).unwrap();
        let cov = store.embedding_coverage().unwrap();
        assert_eq!(cov.without_embedding, 1, "the fixture must owe exactly one vector: {cov:?}");
    }

    #[tokio::test]
    async fn starting_the_server_recovers_the_work_the_last_process_left_owed() {
        let db = tmp_db("boot");
        note_owed_a_vector(&db);
        let before = Store::open(&db).unwrap().embedding_coverage().unwrap();
        assert!(before.without_embedding > 0, "the fixture must actually owe a vector: {before:?}");

        // A queue pointed at a port nobody is listening on, so the recovery itself
        // (which is pure database work) is what is observed, not an embedding.
        let queue = Arc::new(EmbedQueue::new(
            brain_embed::EmbeddingEngine::new("http://127.0.0.1:1".into(), "nomic-embed-text".into()),
        ));
        assert_eq!(queue.pending_len(), 0, "a fresh queue starts empty, as a real restart would");

        // Port 0: the OS picks a free port, and the shutdown resolves at once, so
        // this exercises the startup and returns.
        crate::rmcp_service::serve_rmcp_sse_with(db.clone(), 0, Arc::clone(&queue), async {})
            .await
            .expect("the server must start");

        assert!(
            queue.pending_len() > 0,
            "starting the server must re-queue the note the last process left owed. The boot call is what \
             does this; without it a restart silently loses every in-flight vector until a human runs \
             `reindex --all`."
        );
        let _ = std::fs::remove_file(&db);
    }

    /// The recovered work is in the queue by the time the server is serving.
    ///
    /// Stated precisely, because the obvious over-claim here is "this proves the
    /// recovery runs before the listener binds", and it does not: recovery placed
    /// immediately *after* the bind would satisfy this test too, because both
    /// happen before the shutdown future resolves. Moving the recovery after the
    /// bind was tried as a mutation and this test stayed green.
    ///
    /// So this is a necessary condition, not the ordering itself: it rules out
    /// recovery that never happens, or that happens only once a request arrives.
    /// The ordering — recovery before the bind, so it cannot race the first
    /// `brain_store` — is a design decision, and the argument for it is written
    /// down in [`crate::boot_queue`] rather than asserted here, because pinning it
    /// would need a deliberately slowed-down recovery and a racing client, which
    /// is a timing test of exactly the kind this suite is trying not to grow.
    #[tokio::test]
    async fn the_recovered_work_is_queued_by_the_time_the_server_is_serving() {
        let db = tmp_db("boot-order");
        note_owed_a_vector(&db);
        let queue = Arc::new(EmbedQueue::new(
            brain_embed::EmbeddingEngine::new("http://127.0.0.1:1".into(), "nomic-embed-text".into()),
        ));
        // Port 0 again, but shutdown only resolves once the server is up.
        let served = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&served);
        crate::rmcp_service::serve_rmcp_sse_with(db.clone(), 0, Arc::clone(&queue), async move {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        })
        .await
        .expect("the server must start");
        assert!(served.load(std::sync::atomic::Ordering::SeqCst), "precondition: the server reached its serving state");
        assert!(queue.pending_len() > 0, "and the work was already recovered by then, not deferred until a request arrived");
        let _ = std::fs::remove_file(&db);
        drop(served);
    }
}
