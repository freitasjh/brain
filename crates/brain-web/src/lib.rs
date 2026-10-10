use axum::{extract::{Query, State}, Json, Router, routing::get, debug_handler, http::StatusCode, response::{Html, IntoResponse, sse::{Event, Sse}}};
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tokio_stream::wrappers::IntervalStream;
use tokio_stream::StreamExt;

#[derive(Clone)]
pub struct AppState { pub db: String }

#[derive(Deserialize)]
pub struct SearchQ { query: String, top_k: Option<usize>, layer: Option<String> }

#[derive(Deserialize)]
pub struct ReadQ { path: String }

pub fn router(db: String) -> Router {
    let state = AppState{ db };
    // static viewer fallback
    let serve_dir = ServeDir::new("viewer").not_found_service(axum::routing::get(serve_index));
    Router::new()
        .route("/api/status", get(status))
        .route("/api/search", get(search))
        .route("/api/read", get(read))
        .route("/api/list", get(list))
        .route("/api/graph", get(graph))
        .route("/api/events", get(events))
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn serve_index() -> impl IntoResponse {
    match tokio::fs::read_to_string("viewer/index.html").await {
        Ok(s) => Html(s).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "viewer/index.html not found").into_response(),
    }
}

/// Pure, synchronous projection of chunk embedding coverage into the
/// `/api/status` payload.
///
/// Mirrors `brain_mcp::coverage_status` field for field, on purpose: the three
/// human-facing surfaces (viewer, CLI, MCP) are only comparable if they name
/// the same quantity the same way. It is duplicated rather than shared because
/// importing `brain-mcp` here would invert the layering — `brain-web` is the
/// read-only leaf, `brain-mcp` is the server. **If one changes, change both.**
fn coverage_status(cov: &brain_store::EmbeddingCoverage) -> serde_json::Value {
    serde_json::json!({
        "chunks_total": cov.total,
        "chunks_embedded": cov.embedded,
        "chunks_without_embedding": cov.without_embedding,
        "chunks_zero_vector": cov.zero_vector,
        "embedding_coverage_pct": cov.coverage_pct,
    })
}

/// DB counts plus embedding coverage, the read-only diagnostic surface.
///
/// # TD-008 — why a coverage failure fails the whole status
///
/// The two obvious fallbacks are both worse than an honest error, and the reason
/// is one line of arithmetic: `EmbeddingCoverage::default()` is all zeros, and
/// all zeros is *also* the honest answer for an empty database. So
/// `unwrap_or_default()` renders a broken DB as `embedding_coverage_pct: 0.0`,
/// which is byte-identical to "this DB has chunks and not one of them is
/// embedded", and to "this DB is empty". Three very different states, one
/// payload — and the operator has no way to tell them apart.
///
/// Dropping the field is the mirror-image mistake: a missing field on a
/// diagnostic endpoint is indistinguishable from a binary that predates this
/// change, so a failing query would vanish without a trace. That is precisely
/// the failure this whole TD exists to close — 99.6% of the production index was
/// BLOB-of-zeros and the counts in this very payload stayed perfectly healthy.
///
/// So the error propagates, reusing the `{"error": ...}` shape `Store::open`
/// already returns one line above — no new response shape invented — and
/// matching what the CLI does (`brain status` uses `?`).
///
/// A side effect worth naming: `embedding_coverage` calls `count_chunks` first,
/// so this strictness also covers the `chunks` counter below, whose
/// `unwrap_or(0)` can no longer be the thing that hides a broken database.
///
/// # Why there is no `embedding.ollama` here
///
/// Deliberate, and the same call Y-05 made on the CLI read path: **network does
/// not go on a read-only diagnostic route.** Adding it would need a new
/// `brain-embed` dependency in `brain-web`, and `health_check` is an HTTP GET
/// with a 5 s timeout (`HEALTH_TIMEOUT_SECS`) — while this file's own consumer,
/// `viewer/index.html`, aborts its `/api/status` fetch at 3000 ms. A 5 s probe
/// under a 3 s deadline is not a slower status, it is a status that lies about
/// Ollama on every load.
///
/// The gap is discoverable rather than silent: live embedder health is on the
/// MCP tool `brain_status` and in `journalctl -u brain-mcp.service`, and
/// `coverage_pct` below already degrades when the embedder is down, which is the
/// part a reader of a search index actually needs.
#[debug_handler]
async fn status(State(s): State<AppState>) -> Json<serde_json::Value> {
    let st = match brain_store::Store::open(&s.db) { Ok(v) => v, Err(e) => return Json(serde_json::json!({"error": e.to_string()})) };
    let cov = match st.embedding_coverage() {
        Ok(c) => c,
        Err(e) => return Json(serde_json::json!({"error": format!("embedding_coverage: {e}")})),
    };
    Json(serde_json::json!({
        "notes": st.count_notes().unwrap_or(0),
        "chunks": st.count_chunks().unwrap_or(0),
        "projects": st.project_list().unwrap_or_default().len(),
        "embedding": { "coverage": coverage_status(&cov) }
    }))
}

#[debug_handler]
async fn search(State(s): State<AppState>, Query(q): Query<SearchQ>) -> Json<serde_json::Value> {
    if q.query.trim().is_empty() { return Json(serde_json::json!({"error":"query required"})); }
    let top_k = q.top_k.unwrap_or(10).clamp(1,20);
    let st = match brain_store::Store::open(&s.db) { Ok(v) => v, Err(e) => return Json(serde_json::json!({"error": e.to_string()})) };
    let res = st.search(&q.query, None, q.layer.as_deref(), None, None, None, top_k, false).unwrap_or_default();
    // `chunk_index` is not decoration: search is hybrid, so a result row *is* a
    // chunk, and which chunk of the note matched is the first thing you need when
    // a hybrid result looks wrong. The MCP `brain_search` serves the whole
    // `SearchResult`, `chunk_index` included, so this projection — not the tool —
    // was the outlier, and the viewer rendered "Chunk: undefined" for every hit
    // (TD-011 sweep). Already in hand, so serving it costs no query.
    Json(serde_json::json!({"results": res.iter().map(|r| serde_json::json!({"path": r.path, "layer": r.layer, "scope": r.scope, "score": r.score, "chunk_index": r.chunk_index, "snippet": r.snippet})).collect::<Vec<_>>()}))
}

#[debug_handler]
async fn read(State(s): State<AppState>, Query(q): Query<ReadQ>) -> Json<serde_json::Value> {
    let st = match brain_store::Store::open(&s.db) { Ok(v) => v, Err(e) => return Json(serde_json::json!({"error": e.to_string()})) };
    if let Ok(Some(n)) = st.note_get(&q.path) {
        Json(serde_json::json!({"path": n.path, "content": n.content, "layer": n.layer, "scope": n.scope}))
    } else { Json(serde_json::json!({"error":"not found"})) }
}

/// Browse listing: every note's identity, plus whether it is semantically
/// searchable yet.
///
/// # TD-011 — why the payload carries counts and not a verdict
///
/// `browseAll()` used to read `n.indexed`, a field this endpoint has never
/// served, so the badge fell through to its false branch and every note in the
/// list was labelled `⚠️ Unindexed` — permanently, and for the wrong reason: the
/// note was not unindexed, the *field* was missing. That is the second instance
/// of the same class in this file after the status badge (TD-008), and it is
/// only reachable because a warning's default branch is "something is wrong".
///
/// The three payload states and what each one means:
///
/// * `chunks_embedded >= 1` — indexed for semantic search. The note is reachable
///   by the vector half of the RRF.
/// * `chunks_embedded == 0`, `chunks_total > 0` — the note's chunks are `NULL`,
///   i.e. the embed queue is behind on exactly this note. Reachable by FTS5,
///   invisible to semantic search. This is the state worth an operator's
///   attention, and the one the old badge was *trying* to say.
/// * `chunks_total == 0` — the note has no chunk rows at all. A different fault
///   (chunk sync never ran, or the body was empty) pointing at a different fix,
///   so it is reported as its own state rather than folded into the one above.
///
/// Counts, not a boolean, because `chunks_total`/`chunks_embedded` are the exact
/// names `EmbeddingCoverage` already publishes in `/api/status`; a per-note
/// `indexed: true/false` would have been a fourth vocabulary for one quantity
/// that the codebase already names three ways. Deriving the three states is the
/// viewer's job, from fields the store already computed.
///
/// Both keys are emitted for **every** entry, including a note with no chunks
/// (`chunks_total` 0, `chunks_embedded` 0), so a total payload is the normal
/// case and "field missing" stays a genuinely exceptional state the viewer can
/// refuse to guess about.
///
/// # Why both store calls propagate instead of degrading
///
/// `recent(...).unwrap_or_default()` was the same defect TD-008 removed from
/// `/api/status`, in the same file: a failing query becomes an empty list, and an
/// empty list is indistinguishable from a vault with no notes. Left in place, it
/// would now be worse — the aggregate's failure would present as "all notes are
/// unindexed", a confident false diagnosis. Errors reuse the `{"error": ...}`
/// envelope `Store::open` already returns one line above.
#[debug_handler]
async fn list(State(s): State<AppState>) -> Json<serde_json::Value> {
    let st = match brain_store::Store::open(&s.db) { Ok(v) => v, Err(e) => return Json(serde_json::json!({"error": e.to_string()})) };
    let recent = match st.recent_paths(1000) {
        Ok(v) => v,
        Err(e) => return Json(serde_json::json!({"error": format!("recent_paths: {e}")})),
    };
    // One `GROUP BY path` over `chunks`, not one count per note. Measured on the
    // production corpus (278 notes / 1000 chunks) at ~2.7 ms warm, and the
    // handler gives up the 516 KiB of note bodies it used to select and discard,
    // so the endpoint as a whole gets cheaper than it was.
    let counts = match st.chunk_embedding_counts() {
        Ok(v) => v,
        Err(e) => return Json(serde_json::json!({"error": format!("chunk_embedding_counts: {e}")})),
    };
    let entries = recent.iter().map(|(p, l, sc)| {
        let c = counts.get(p).copied().unwrap_or_default();
        serde_json::json!({
            "path": p,
            "layer": l,
            "scope": sc,
            "chunks_total": c.total,
            "chunks_embedded": c.embedded,
        })
    }).collect::<Vec<_>>();
    Json(serde_json::json!({ "entries": entries }))
}

#[derive(Deserialize)]
pub struct GraphQ { project: Option<String> }

/// Display label of a graph node: the last path segment (RF-11). The full
/// path stays in `id` verbatim, so the viewer can still show it as a tooltip
/// and hand it to `/api/read` unchanged.
fn graph_label(path: &str) -> &str {
    match path.rsplit('/').next() {
        Some(seg) if !seg.is_empty() => seg,
        _ => path,
    }
}

/// `GET /api/graph?project=` — project-aware note graph (viewer-graph RF-01..RF-07).
///
/// Read-only like the other four handlers: one `Store` per request, no embed,
/// no write. Nodes are notes (`id` = `notes.path` verbatim, the same identity
/// `/api/read` matches) plus `projeto:<nome>` hubs; edges are wikilinks plus
/// note→project memberships. A wikilink whose target has no row in `notes` is
/// pruned and counted in `dangling` (RF-05) instead of becoming a clickable
/// node that would resolve to `{"error":"not found"}`.
///
/// R-01/R-07: every fallible step degrades to the shared `{"error"}` envelope
/// — an unknown project name is the one exception, and it is a name rather
/// than a failure, so it serves the honest empty graph with HTTP 200 (RF-04).
#[debug_handler]
async fn graph(State(s): State<AppState>, Query(q): Query<GraphQ>) -> Json<serde_json::Value> {
    let st = match brain_store::Store::open(&s.db) { Ok(v) => v, Err(e) => return Json(serde_json::json!({"error": e.to_string()})) };
    // The dropdown's "todos" arrives as absent or blank — both mean unfiltered.
    let project: Option<String> = q.project.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(str::to_string);
    let snap = match st.graph_snapshot(project.as_deref()) { Ok(v) => v, Err(e) => return Json(serde_json::json!({"error": format!("graph_snapshot: {e}")})) };
    let members: std::collections::HashSet<&str> = snap.notes.iter().map(|(p, _)| p.as_str()).collect();
    let mut nodes: Vec<serde_json::Value> = snap.notes.iter().map(|(p, l)| {
        serde_json::json!({"id": p, "kind": "note", "label": graph_label(p), "layer": l})
    }).collect();
    // Hubs: every project unfiltered (522 notes + 8 hubs on the live corpus),
    // only the named one under a filter that resolved to a real project.
    if let Some(name) = project.as_deref() {
        if snap.projects.iter().any(|n| n == name) {
            nodes.push(serde_json::json!({"id": format!("projeto:{name}"), "kind": "project", "label": name, "layer": serde_json::Value::Null}));
        }
    } else {
        for n in &snap.projects {
            nodes.push(serde_json::json!({"id": format!("projeto:{n}"), "kind": "project", "label": n, "layer": serde_json::Value::Null}));
        }
    }
    let ids: std::collections::HashSet<&str> = nodes.iter().filter_map(|v| v["id"].as_str()).collect();
    let mut edges: Vec<serde_json::Value> = Vec::new();
    let mut dangling: i64 = 0;
    for (from, to, to_missing) in &snap.links {
        // Induced subgraph (ADR-03): a wikilink only exists with both ends in
        // the served node set, so cross-project links hide instead of hanging.
        if !members.contains(from.as_str()) { continue; }
        // Pruned and counted (RF-05): the target was never a note.
        if *to_missing { dangling += 1; continue; }
        if !members.contains(to.as_str()) { continue; }
        edges.push(serde_json::json!({"from": from, "to": to, "kind": "wikilink"}));
    }
    for (note_path, proj_name) in &snap.memberships {
        if !members.contains(note_path.as_str()) { continue; }
        let target = format!("projeto:{proj_name}");
        // Memberships are scoped to the served hubs, so this guard is dead code
        // on a healthy join — it exists so a join skew can never orphan an edge.
        if !ids.contains(target.as_str()) { continue; }
        edges.push(serde_json::json!({"from": note_path, "to": target, "kind": "project"}));
    }
    nodes.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    edges.sort_by(|a, b| (a["from"].as_str(), a["to"].as_str(), a["kind"].as_str()).cmp(&(b["from"].as_str(), b["to"].as_str(), b["kind"].as_str())));
    Json(serde_json::json!({"nodes": nodes, "edges": edges, "dangling": dangling, "projects": snap.projects}))
}

/// `GET /api/events` — SSE stream that emits `graph-changed` when the DB changes.
///
/// Polls `count_notes()` and `count_chunks()` every 2 seconds. When either count
/// differs from the previous snapshot, emits a `graph-changed` event. The stream
/// terminates when the client disconnects (axum's `Sse` handles this automatically).
///
/// # Why polling instead of SQLite hooks
///
/// `brain-web` is a read-only leaf — it cannot depend on `brain-mcp` (which owns
/// the write path and the embedding queue). Polling is the only mechanism available
/// without inverting the dependency graph. The 2-second interval is a compromise:
/// fast enough for the viewer to feel live (< 3s latency), cheap enough that
/// opening a `Store` every 2s is negligible (WAL mode, indexed counts).
///
/// # Why `Arc<Mutex<(i64, i64)>>` for the snapshot
///
/// The snapshot must survive across polling iterations. `Arc` shares it between
/// the handler and the stream; `Mutex` guards concurrent access. The alternative
/// (a `tokio::sync::watch` channel) would require a writer, which the read-only
/// handler does not have.
#[debug_handler]
async fn events(State(s): State<AppState>) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let db = s.db.clone();
    let snapshot = Arc::new(Mutex::new((0i64, 0i64)));
    let stream = IntervalStream::new(tokio::time::interval(std::time::Duration::from_secs(2)))
        .then(move |_| {
            let db = db.clone();
            let snapshot = snapshot.clone();
            async move {
                let st = brain_store::Store::open(&db).ok()?;
                let notes = st.count_notes().unwrap_or(0);
                let chunks = st.count_chunks().unwrap_or(0);
                let mut snap = snapshot.lock().unwrap();
                if snap.0 != notes || snap.1 != chunks {
                    *snap = (notes, chunks);
                    Some(Ok(Event::default().event("graph-changed").data("graph-changed")))
                } else {
                    None
                }
            }
        })
        .filter_map(|opt| opt);
    Sse::new(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_db(tag: &str) -> String {
        let db = format!("/tmp/brain-web-{}-{}.db", std::process::id(), tag);
        let _ = std::fs::remove_file(&db);
        db
    }

    /// A real, non-zero 768-dim vector. `chunk_insert` refuses a zero-norm vector
    /// (and `zero_vector_guard` would flag the literal outside a test module), so
    /// the "embedded" half of the coverage fixture has to be a genuine vector.
    fn fixture_vec(seed: u32) -> Vec<f32> {
        (0..brain_core::EMBEDDING_DIM).map(|i| ((i as u32 + seed) % 17 + 1) as f32 / 16.0).collect()
    }

    /// Stores one note as one chunk, with or without a vector.
    fn store_with_chunk(s: &brain_store::Store, path: &str, content: &str, embedding: Option<&[f32]>) {
        let nid = s.note_upsert(path, "regras", Some("global"), content, None, &[], false, None).unwrap();
        s.chunk_insert(nid, path, "regras", Some("global"), content, 0, 1, None, &[], embedding).unwrap();
    }

    /// `viewer/index.html`, located from the manifest dir because `cargo test`
    /// runs with cwd set to the crate directory (see `test_serve_index_fallback`).
    fn viewer_source() -> String {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../viewer/index.html");
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
    }

    #[test]
    fn test_status_counts() {
        let db = tmp_db("status");
        {
            let s = brain_store::Store::open(&db).unwrap();
            // One embedded chunk and one NULL — the mixed state whose 50% coverage
            // is the whole point of TD-008. A single embedded chunk would prove
            // only that the happy path formats.
            let v = fixture_vec(1);
            store_with_chunk(&s, "regras/global/embedded", "## embedded chunk", Some(&v));
            store_with_chunk(&s, "regras/global/pending", "## pending chunk", None);
        }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let v = rt.block_on(async { status(State(AppState{ db: db.clone() })).await.0 });
        assert_eq!(v["notes"], 2);
        assert_eq!(v["chunks"], 2);
        assert_eq!(v["projects"], 0);
        assert!(v.get("error").is_none(), "a healthy DB must not report an error: {}", v);
        let c = &v["embedding"]["coverage"];
        assert_eq!(c["chunks_total"], 2, "got {}", v);
        assert_eq!(c["chunks_embedded"], 1, "got {}", v);
        assert_eq!(c["chunks_without_embedding"], 1, "got {}", v);
        assert_eq!(c["chunks_zero_vector"], 0, "got {}", v);
        assert_eq!(c["embedding_coverage_pct"], 50.0, "got {}", v);
        let _ = std::fs::remove_file(&db);
    }

    /// The empty database is the case where a coverage ratio is most likely to
    /// divide by zero. `EmbeddingCoverage` documents `0.0` for it, and a `NaN`
    /// would serialise into the JSON payload as `null` — the operator would read
    /// "no coverage reported" as "no data", which are different conversations.
    #[test]
    fn test_status_coverage_on_empty_db_is_zero_not_nan() {
        let db = tmp_db("empty");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let v = rt.block_on(async { status(State(AppState{ db: db.clone() })).await.0 });
        assert!(v.get("error").is_none(), "an empty DB is not an error: {}", v);
        assert_eq!(v["notes"], 0);
        assert_eq!(v["chunks"], 0);
        let c = &v["embedding"]["coverage"];
        assert_eq!(c["chunks_total"], 0, "got {}", v);
        assert_eq!(c["chunks_embedded"], 0, "got {}", v);
        assert_eq!(c["chunks_without_embedding"], 0, "got {}", v);
        assert_eq!(c["chunks_zero_vector"], 0, "got {}", v);
        assert_eq!(c["embedding_coverage_pct"], 0.0, "got {}", v);
        assert!(!c["embedding_coverage_pct"].is_null(), "0/0 must not serialise as null: {}", v);
        let _ = std::fs::remove_file(&db);
    }

    /// Field names of a `serde_json::Value` object, flattened to dotted paths
    /// (`notes`, `embedding.coverage.embedding_coverage_pct`, ...).
    fn leaf_paths(v: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
        let Some(m) = v.as_object() else {
            out.push(prefix.to_string());
            return;
        };
        for (k, val) in m {
            let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
            if val.is_object() && !val.as_object().is_some_and(|o| o.is_empty()) {
                leaf_paths(val, &p, out);
            } else {
                out.push(p);
            }
        }
    }

    /// The `<script>` body of `viewer/index.html` — the only region that reads
    /// payloads, and the only region a JS lexer should be pointed at.
    ///
    /// Scoping here rather than lexing the whole document is not tidiness: the
    /// page is HTML and CSS wrapped around one script, and element text such as
    /// `>http://localhost:8321<` is bare markup where a `//` legitimately starts
    /// a comment. Any quote imbalance in the markup would desynchronise a
    /// whole-file lexer and the scan would quietly stop finding functions.
    fn viewer_script() -> String {
        let src = viewer_source();
        let start = src.find("<script>").expect("viewer has no <script> block") + "<script>".len();
        let end = src[start..].find("</script>").expect("viewer <script> is unterminated") + start;
        src[start..end].to_string()
    }

    /// Lexes viewer JavaScript, dropping comments and (unless `keep_literals`)
    /// string and template-literal *contents* while keeping `${…}` bodies.
    ///
    /// Two modes, because two different questions are being asked of the same
    /// file. A **field read** is code, so literal text must go: `statusEl.title`
    /// is assigned the literal `journalctl -u brain-mcp.service`, and a scan
    /// that reads string text sees a member `service` on a root `mcp` and reports
    /// it as a phantom field. A guard that fires on prose has to be muted by
    /// editing prose, which is the same hole the original version had with
    /// `http://` in a string. A **call site** is the opposite — the click
    /// handler is `onclick="readNote('${n.path}')"`, whose target and argument
    /// punctuation are template-literal *markup*; blanking them would erase the
    /// very wiring under test, while leaving comments in would let a paragraph
    /// of prose about `.replace('.md','')` fake a failure.
    ///
    /// Comments are always dropped and newlines are always preserved, so line
    /// structure — and therefore a closing brace found at column 0 — survives
    /// either way.
    ///
    /// **Contexts nest, so mode and brace depth are saved together.** `browseAll`
    /// renders `${notes.map(n => \`…${n.path}…\`).join('')}` — a template literal
    /// inside an interpolation hole inside another template literal, and the
    /// shape the viewer uses everywhere. Two things break if nesting is tracked
    /// with single variables, and both fail *silently and permissively*:
    ///
    /// * a lone "am I in a hole" flag forgets the outer hole as soon as the
    ///   inner template closes, after which the rest of the outer template is
    ///   lexed as code and the next backtick reopens a template;
    /// * a lone brace-depth counter is clobbered by the inner hole and reads `0`
    ///   when control returns, so the outer hole's `}` looks like ordinary code
    ///   and the hole is never closed.
    ///
    /// Either way `viewer_fn` hands a function a body that runs on into the next
    /// one, and the scan reports reads belonging to a different endpoint — the
    /// guard then passes on a body it never really examined. So the return path
    /// is a stack of `(mode, depth)`, and every close restores both.
    fn lex_js(src: &str, keep_literals: bool) -> String {
        #[derive(PartialEq, Clone, Copy)]
        enum S { Code, Line, Block, Str(char), Tmpl }
        let b: Vec<char> = src.chars().collect();
        let mut out = String::with_capacity(src.len());
        let mut ret: Vec<(S, usize)> = Vec::new();
        let mut st = S::Code;
        let mut hole = 0usize;
        let mut i = 0usize;
        let blank = |out: &mut String, c: char| out.push(if c == '\n' { '\n' } else { ' ' });
        macro_rules! close {
            () => {{
                let (m, d) = ret.pop().unwrap_or((S::Code, 0));
                st = m;
                hole = d;
            }};
        }
        while i < b.len() {
            let c = b[i];
            match st {
                S::Code => {
                    if c == '/' && b.get(i + 1) == Some(&'/') { st = S::Line; i += 2; continue; }
                    if c == '/' && b.get(i + 1) == Some(&'*') { st = S::Block; i += 2; continue; }
                    if c == '"' || c == '\'' { ret.push((S::Code, hole)); st = S::Str(c); out.push(c); i += 1; continue; }
                    if c == '`' { ret.push((S::Code, hole)); st = S::Tmpl; out.push(c); i += 1; continue; }
                    if hole > 0 {
                        if c == '{' {
                            hole += 1;
                        } else if c == '}' {
                            hole -= 1;
                            if hole == 0 {
                                close!();
                                out.push(c);
                                i += 1;
                                continue;
                            }
                        }
                    }
                    out.push(c);
                    i += 1;
                }
                S::Line => {
                    if c == '\n' { st = S::Code; out.push(c); } else { blank(&mut out, c); }
                    i += 1;
                }
                S::Block => {
                    if c == '*' && b.get(i + 1) == Some(&'/') { st = S::Code; i += 2; continue; }
                    blank(&mut out, c);
                    i += 1;
                }
                S::Str(q) => {
                    if c == q { close!(); out.push(c); i += 1; continue; }
                    if c == '\\' {
                        // The pair is handled together, so an escaped quote cannot
                        // be mistaken for the terminator and cut the string short.
                        if keep_literals { out.push(c); } else { blank(&mut out, c); }
                        if let Some(n) = b.get(i + 1) {
                            if keep_literals { out.push(*n); } else { blank(&mut out, *n); }
                        }
                        i += 2;
                        continue;
                    }
                    if keep_literals { out.push(c); } else { blank(&mut out, c); }
                    i += 1;
                }
                S::Tmpl => {
                    if c == '`' { close!(); out.push(c); i += 1; continue; }
                    if c == '\\' {
                        if keep_literals { out.push(c); } else { blank(&mut out, c); }
                        if let Some(n) = b.get(i + 1) {
                            if keep_literals { out.push(*n); } else { blank(&mut out, *n); }
                        }
                        i += 2;
                        continue;
                    }
                    if c == '$' && b.get(i + 1) == Some(&'{') {
                        ret.push((S::Tmpl, hole));
                        st = S::Code;
                        hole = 1;
                        out.push_str("${");
                        i += 2;
                        continue;
                    }
                    if keep_literals { out.push(c); } else { blank(&mut out, c); }
                    i += 1;
                }
            }
        }
        out
    }

    /// The viewer script with comments and literal text removed — the view under
    /// which payload **field reads** are visible and prose is not.
    fn viewer_code() -> String {
        lex_js(&viewer_script(), false)
    }

    /// The viewer script with comments removed and literal text kept — the view
    /// under which **markup** such as an `onclick="…"` call site is visible.
    fn viewer_markup() -> String {
        lex_js(&viewer_script(), true)
    }

    /// ECMAScript and DOM members the viewer reads off things that are not
    /// payloads, keyed by member name.
    ///
    /// Every entry is a hole in the guard: a phantom field named `title` or `map`
    /// would be silently allowed. The list is derived from the viewer's actual
    /// reads, not from the language, so it stays as small as the file allows.
    /// `entries` is here for `Object.entries`; `data.entries` survives because it
    /// is collected as a dotted chain before this filter runs.
    const JS_MEMBERS: &[&str] = &[
        "json", "ok", "message", "innerHTML", "className", "textContent", "title",
        "getElementById", "querySelectorAll", "map", "length", "trim", "set",
        "replace", "replaceAll", "split", "join", "slice", "toFixed", "entries", "timeout",
        "addEventListener", "close",
    ];

    /// Roots that hold no payload data, so any member read off one is a library
    /// or DOM call by construction.
    ///
    /// `cv`/`ctx` are the graph canvas and its 2d context, `ev` a UI event and
    /// `rect` its bounding rect (`renderGraph` hit-test/pan/zoom) — all real DOM
    /// objects, none carrying endpoint data. `Math` is the same class of root as
    /// the `Object` already listed: a builtin, not a payload.
    const JS_ROOTS: &[&str] = &[
        "Object", "document", "window", "console", "location", "resp", "err",
        "resultsEl", "statusEl", "AbortSignal", "params",
        "Math", "cv", "ctx", "ev", "rect",
    ];

    /// Every dotted `data.<a>.<b>…` chain in `src`, each walked as far as the
    /// following identifier characters allow.
    fn data_chains(src: &str) -> Vec<String> {
        fn ident(tail: &str) -> String {
            tail.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$').collect()
        }
        let mut out = Vec::new();
        let mut from = 0usize;
        while let Some(rel) = src[from..].find("data.") {
            let at = from + rel;
            let tail = &src[at + "data.".len()..];
            let mut path = ident(tail);
            if path.is_empty() {
                from = at + "data.".len();
                continue;
            }
            let mut consumed = path.len();
            for _ in 0..3 {
                match tail[consumed..].strip_prefix('.') {
                    Some(rest) => {
                        let seg = ident(rest);
                        if seg.is_empty() { break; }
                        path.push('.');
                        path.push_str(&seg);
                        consumed += 1 + seg.len();
                    }
                    None => break,
                }
            }
            out.push(path);
            from = at + "data.".len();
        }
        out
    }

    /// The payload field names a render path reads, as bare names.
    ///
    /// This is the piece that was missing. The TD-008 scan only understood
    /// `data.*` chains, so it could only see `checkStatus` — the one render path
    /// that happens to bind the response to a variable named `data`. `browseAll`
    /// reads its entries as `item.*` and `n.*`, and `n.indexed` rendered
    /// `undefined` for the whole life of the field while the guard built to catch
    /// that class of bug never looked at the function.
    ///
    /// Name-based rather than path-based is what makes it work here: the entry
    /// variables are bound by a `for…of`, by array destructuring, and by a `.map`
    /// arrow parameter, so their provenance is not statically recoverable — but
    /// the *field* they reach for is a plain identifier in every case.
    fn entry_field_reads(body: &str) -> Vec<String> {
        let b: Vec<char> = body.chars().collect();
        let word = |from: usize| -> String {
            b[from..].iter().take_while(|x| x.is_ascii_alphanumeric() || **x == '_' || **x == '$').collect()
        };
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < b.len() {
            let c = b[i];
            if !(c.is_ascii_alphabetic() || c == '_' || c == '$') { i += 1; continue; }
            let root = word(i);
            let after = i + root.len();
            if b.get(after) != Some(&'.') || JS_ROOTS.contains(&root.as_str()) { i = if after > i { after } else { i + 1 }; continue; }
            let prop = word(after + 1);
            if !prop.is_empty() && !JS_MEMBERS.contains(&prop.as_str()) { out.push(prop); }
            i = after + 1;
        }
        out.sort();
        out.dedup();
        out
    }

    /// Leaf *names* of a payload, ignoring nesting — the vocabulary
    /// [`entry_field_reads`] is checked against.
    fn leaf_names(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, val) in m {
                    out.push(k.clone());
                    leaf_names(val, out);
                }
            }
            serde_json::Value::Array(a) => { for v in a { leaf_names(v, out); } }
            _ => {}
        }
    }

    /// Asserts every field `body` reads is a field `payload` serves.
    ///
    /// `error` is exempt: it is the handlers' documented failure envelope, absent
    /// on a healthy DB by construction, so it can never appear in a served leaf
    /// set. Reading it is correct, and the render path that reads it is the one
    /// that handles a broken store.
    fn assert_no_phantom_reads(body: &str, payload: &serde_json::Value, what: &str) {
        let reads = entry_field_reads(body);
        assert!(!reads.is_empty(), "scanned no field reads out of {what} — the scan is vacuous");
        let mut names = Vec::new();
        leaf_names(payload, &mut names);
        names.sort();
        names.dedup();
        let phantom: Vec<String> = reads
            .iter()
            .filter(|r| r.as_str() != "error")
            .filter(|r| !names.contains(r))
            .cloned()
            .collect();
        assert!(
            phantom.is_empty(),
            "{what} reads fields its endpoint does not serve — these render as `undefined`: {phantom:?} (served: {names:?})"
        );
    }

    /// The body of one top-level `function <name>() { … }` in the viewer script.
    ///
    /// Scoped deliberately: the viewer talks to three endpoints (`/api/status`,
    /// `/api/search`, `/api/list`, `/api/read`) and each render path is only
    /// valid against its own payload. Scanning the whole file against the status
    /// payload would flag `data.results` — which `/api/status` genuinely does not
    /// serve, and which `search()` needs.
    fn viewer_fn(src: &str, name: &str) -> String {
        let marker = format!("function {name}(");
        let start = src
            .find(&marker)
            .unwrap_or_else(|| panic!("viewer has no `{name}` function — the status render path moved"));
        let rest = &src[start..];
        // A top-level `}` closes the body: the next declaration starts at column 0.
        let end = rest
            .find("\n}\n")
            .map(|e| e + 3)
            .unwrap_or(rest.len());
        rest[..end].to_string()
    }

    /// Field paths the status badge reads, checked against a real payload.
    ///
    /// This is the assertion that makes TD-008's *second* half real. The viewer's
    /// `fetch('/api/status')` used to throw the response away, and what it did
    /// read — `data.index_entries` and `data.ollama_ok` — were fields the
    /// endpoint has never returned, so the badge rendered the literal text
    /// "undefined entries" and a permanently-yellow Ollama dot. An API that
    /// reports coverage nobody renders leaves the debt open, and a reader who
    /// trusts a field that does not exist cannot notice the field is missing.
    ///
    /// It is a source scan against a real response, not a browser test: it proves
    /// every field the status line reads is a field the handler emits. It does
    /// **not** prove anything is painted, and it is not a substitute for one —
    /// per the No-SPA rule in `.agents/rules/frontend-rules.md` this static viewer
    /// carries no frontend test framework, and adding one to check a template
    /// string would cost more than the string.
    #[test]
    fn viewer_status_badge_reads_only_fields_the_status_endpoint_actually_serves() {
        let db = tmp_db("viewer-wiring");
        {
            let s = brain_store::Store::open(&db).unwrap();
            let v = fixture_vec(2);
            store_with_chunk(&s, "regras/global/e", "## embedded", Some(&v));
            store_with_chunk(&s, "regras/global/p", "## pending", None);
        }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let payload = rt.block_on(async { status(State(AppState{ db: db.clone() })).await.0 });
        let _ = std::fs::remove_file(&db);

        let mut served = Vec::new();
        leaf_paths(&payload, "", &mut served);
        assert!(served.iter().any(|p| p == "embedding.coverage.embedding_coverage_pct"),
            "the endpoint must serve the coverage block; served: {served:?}");

        let html = viewer_code();
        let body = viewer_fn(&html, "checkStatus");

        // Alias table for local bindings, e.g.
        // `const cov = (data.embedding && data.embedding.coverage) || {}` — so a
        // later `cov.embedding_coverage_pct` resolves to a real payload path. The
        // target is the *longest* `data.*` chain in the initialiser, which is what
        // makes the parenthesised guard resolve to `embedding.coverage` rather
        // than to the `embedding` half of it.
        let mut aliases: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for decl in body.match_indices("const ") {
            let tail = &body[decl.0 + "const ".len()..];
            let name: String = tail
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if name.is_empty() {
                continue;
            }
            let init = &tail[name.len()..];
            let init = &init[..init.find(';').unwrap_or(init.len())];
            let mut longest: Option<String> = None;
            for chain in data_chains(init) {
                if longest.as_ref().is_none_or(|l| chain.len() > l.len()) {
                    longest = Some(chain);
                }
            }
            if let Some(path) = longest {
                aliases.insert(name, path);
            }
        }

        let mut referenced: Vec<String> = data_chains(&body);
        // `cov.embedding_coverage_pct` — resolve the alias to a real path.
        for (alias, path) in &aliases {
            let needle = format!("{alias}.");
            let mut f = 0usize;
            while let Some(rel) = body[f..].find(&needle) {
                let at = f + rel;
                let seg: String = body[at + needle.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if !seg.is_empty() {
                    referenced.push(format!("{path}.{seg}"));
                }
                f = at + needle.len();
            }
        }
        referenced.sort();
        referenced.dedup();

        assert!(!referenced.is_empty(), "scanned no `data.` reads out of checkStatus — the scan is vacuous");
        assert!(referenced.iter().any(|p| p == "embedding.coverage.embedding_coverage_pct"),
            "the status badge must read the coverage percentage; reads: {referenced:?}");
        assert!(referenced.iter().any(|p| p == "embedding.coverage.chunks_without_embedding"),
            "the status badge must read the pending-chunk count; reads: {referenced:?}");

        // `error` is the handler's documented failure envelope: absent on a healthy
        // DB, so it can never appear in `served`, but reading it is correct.
        let phantom: Vec<String> = referenced
            .iter()
            .filter(|p| p.as_str() != "error")
            // A read of a container (`data.embedding.coverage`) is satisfied by any
            // leaf beneath it; `leaf_paths` flattens objects away.
            .filter(|p| !served.iter().any(|s| s == *p || s.starts_with(&format!("{p}."))))
            .cloned()
            .collect();
        assert!(
            phantom.is_empty(),
            "the status badge reads fields /api/status does not serve — these render as `undefined`: {phantom:?} (served: {served:?})"
        );
    }

    /// TD-011. `browseAll()` read `n.indexed`, which `/api/list` has never served.
    ///
    /// The failure mode is the one the status badge shares: a truthiness test on
    /// a missing field falls through to its false branch, and the false branch
    /// said something was wrong. So every note in the browse list carried
    /// `⚠️ Unindexed` — permanently, for notes that were indexed — and the badge
    /// was pure noise.
    ///
    /// The fix serves the counts `/api/status` already publishes, so the assertion
    /// is that the browse path reads fields that exist, *and* that it reads the
    /// coverage pair at all. A `browseAll` that dropped the badge entirely would
    /// pass a pure phantom check; the operator is owed the distinction between a
    /// note that is semantically searchable and one that is text-only.
    #[test]
    fn viewer_browse_list_reads_only_fields_the_list_endpoint_actually_serves() {
        let db = tmp_db("viewer-list-wiring");
        {
            let s = brain_store::Store::open(&db).unwrap();
            let v = fixture_vec(3);
            store_with_chunk(&s, "regras/global/e", "## embedded", Some(&v));
            store_with_chunk(&s, "regras/global/p", "## pending", None);
        }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let payload = rt.block_on(async { list(State(AppState{ db: db.clone() })).await.0 });
        let _ = std::fs::remove_file(&db);

        let mut names = Vec::new();
        leaf_names(&payload, &mut names);
        assert!(names.contains(&"chunks_total".to_string()) && names.contains(&"chunks_embedded".to_string()),
            "/api/list must serve the per-note coverage pair; served: {names:?}");

        let html = viewer_code();
        let body = viewer_fn(&html, "browseAll");
        assert_no_phantom_reads(&body, &payload, "browseAll");

        let reads = entry_field_reads(&body);
        assert!(reads.contains(&"chunks_total".to_string()) && reads.contains(&"chunks_embedded".to_string()),
            "browseAll must derive its badge from the served coverage counts, not from a field of its own; reads: {reads:?}");
    }

    /// The rest of the sweep: the two render paths nobody had scanned.
    ///
    /// `search()` was the third instance of the class, found by reading the file
    /// rather than by running the guard — `item.chunk_index` is a real field of
    /// `SearchResult` and the MCP `brain_search` serves it, so the projection in
    /// `/api/search` was the lone outlier and the viewer printed `Chunk:
    /// undefined` on every single hit. `readNote()` is asserted too, and passes:
    /// a path that is clean today is worth a test if the next field read is not.
    #[test]
    fn viewer_search_and_read_paths_read_only_fields_their_endpoints_serve() {
        let db = tmp_db("viewer-search-wiring");
        {
            let s = brain_store::Store::open(&db).unwrap();
            let v = fixture_vec(4);
            store_with_chunk(&s, "regras/global/z", "## zebra topic", Some(&v));
        }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let search_payload = rt.block_on(async {
            search(State(AppState{ db: db.clone() }), Query(SearchQ{ query: "zebra".into(), top_k: Some(5), layer: None })).await.0
        });
        let read_payload = rt.block_on(async {
            read(State(AppState{ db: db.clone() }), Query(ReadQ{ path: "regras/global/z".into() })).await.0
        });
        let _ = std::fs::remove_file(&db);

        let html = viewer_code();
        assert_no_phantom_reads(&viewer_fn(&html, "search"), &search_payload, "search");
        assert_no_phantom_reads(&viewer_fn(&html, "readNote"), &read_payload, "readNote");

        // `chunk_index` is the assertion that makes the search half meaningful:
        // the field exists on `SearchResult` and the MCP tool serves it, so if it
        // ever drops out of this projection the guard must say so.
        let hits = search_payload["results"].as_array().unwrap();
        assert!(!hits.is_empty(), "the fixture must produce a hit for the search payload to be meaningful");
        assert!(hits[0].get("chunk_index").is_some(), "/api/search must serve chunk_index for a hybrid hit: {}", search_payload);
    }

    /// `/api/read` matches `notes.path` exactly, so the browse list has to hand it
    /// that column verbatim.
    ///
    /// It did not: `readNote` was called with `n.path.replace('.md','')
    /// .split('/').slice(1).join('/')`, which strips the leading layer segment, and
    /// `/api/read` was additionally sent a `layer=` query parameter its `ReadQ`
    /// has no field for. Every note in the browse list therefore resolved to
    /// `{"error":"not found"}` — the list rendered and nothing was clickable. The
    /// layer is not decoration to be trimmed: it is `notes.path`'s first segment.
    #[test]
    fn viewer_browse_list_hands_the_read_endpoint_a_resolvable_path() {
        let db = tmp_db("viewer-path");
        {
            let s = brain_store::Store::open(&db).unwrap();
            // A scoped path (layer/scope/rest) and an unscoped one (layer/rest,
            // which is what `sessoes` produces) — the two shapes whose segment
            // counts differ, and therefore the two a positional strip gets wrong.
            s.note_upsert("regras/global/deep/nested/name", "regras", Some("global"), "## scoped", None, &[], false, None).unwrap();
            s.note_upsert("sessoes/brain/2026-01-02", "sessoes", None, "## unscoped", None, &[], false, None).unwrap();
        }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let payload = rt.block_on(async { list(State(AppState{ db: db.clone() })).await.0 });

        // The static half: the argument the click handler passes. Read from the
        // markup view — the call site is `onclick="readNote('${n.path}')"`, whose
        // target and punctuation are template-literal text, so the field-read
        // view would have blanked the very thing under test. Comments are still
        // gone, so prose about `.replace('.md','')` cannot fake a failure.
        let markup = viewer_fn(&viewer_markup(), "browseAll");
        let call = markup
            .find("readNote(")
            .unwrap_or_else(|| panic!("browseAll no longer navigates via readNote — re-check this guard"));
        let args = &markup[call + "readNote(".len()..];
        let args = &args[..args.find(')').expect("unterminated readNote call")];
        for banned in ["slice", "split", "replace", "replaceAll", "substr", "substring"] {
            assert!(
                !args.contains(&format!(".{banned}(")),
                "browseAll rewrites the note path with `.{banned}(` before handing it to readNote: {args:?} — /api/read matches notes.path exactly, so the click resolves to \"not found\""
            );
        }
        assert!(
            args.contains(".path"),
            "browseAll must hand readNote the entry's `path`; args: {args:?}"
        );

        // The behavioural half: whatever the list serves must be a path the read
        // endpoint can actually resolve. This is the invariant the static check
        // cannot see — that `list` and `read` agree on what a path *is*.
        {
            let s = brain_store::Store::open(&db).unwrap();
            for e in payload["entries"].as_array().unwrap() {
                let p = e["path"].as_str().unwrap();
                assert!(
                    s.note_get(p).unwrap().is_some(),
                    "/api/list serves `{p}` but /api/read cannot resolve it — every click on that row would 404"
                );
            }
        }
        let _ = std::fs::remove_file(&db);
    }

    /// The per-note coverage counts, one row per documented state.
    ///
    /// Three states, and the third is the reason this is not a boolean:
    /// `chunks_total == 0` (no chunk rows at all) is a different fault from
    /// `chunks_embedded == 0` with chunks present (the embed queue is behind on
    /// this note), and they point at different fixes. Both keys are present even
    /// at zero, so the viewer's "field missing" branch stays a state it can
    /// refuse to guess about rather than the normal path.
    #[test]
    fn test_list_reports_per_note_embedding_coverage() {
        let db = tmp_db("list-coverage");
        {
            let s = brain_store::Store::open(&db).unwrap();
            let v = fixture_vec(5);
            // (a) semantically searchable
            store_with_chunk(&s, "regras/global/embed", "## embed", Some(&v));
            // (b) FTS-only: chunks exist, none embedded
            let nid = s.note_upsert("regras/global/pending", "regras", Some("global"), "## pending", None, &[], false, None).unwrap();
            s.chunk_insert(nid, "regras/global/pending", "regras", Some("global"), "## pending", 0, 2, None, &[], None).unwrap();
            s.chunk_insert(nid, "regras/global/pending", "regras", Some("global"), "## pending 2", 1, 2, None, &[], None).unwrap();
            // (c) no chunk rows at all
            s.note_upsert("regras/global/bare", "regras", Some("global"), "## bare", None, &[], false, None).unwrap();
        }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let v = rt.block_on(async { list(State(AppState{ db: db.clone() })).await.0 });
        assert!(v.get("error").is_none(), "a healthy DB must not report an error: {}", v);

        let entries = v["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 3, "got {}", v);
        let by = |p: &str| entries.iter().find(|e| e["path"] == p).unwrap_or_else(|| panic!("{p} missing from {}", v)).clone();
        let e = by("regras/global/embed");
        assert_eq!(e["chunks_total"], 1, "got {v}");
        assert_eq!(e["chunks_embedded"], 1, "got {v}");
        let e = by("regras/global/pending");
        assert_eq!(e["chunks_total"], 2, "got {v}");
        assert_eq!(e["chunks_embedded"], 0, "got {v}");
        let e = by("regras/global/bare");
        assert_eq!(e["chunks_total"], 0, "got {v}");
        assert_eq!(e["chunks_embedded"], 0, "got {v}");
        // Total payload, including the zero case: no entry may be missing a key.
        for e in entries {
            assert!(e.get("chunks_total").is_some() && e.get("chunks_embedded").is_some(),
                "every entry must carry both counts, so the viewer never has to guess: {e}");
            assert!(e.get("layer").is_some() && e.get("scope").is_some() && e.get("path").is_some(), "{e}");
        }
        // The bodies are not served: the browse view never renders them, and
        // selecting them was 516 KiB of discarded payload on the production corpus.
        assert!(entries.iter().all(|e| e.get("content").is_none()), "/api/list must not carry note bodies: {v}");
        let _ = std::fs::remove_file(&db);
    }

    /// An empty database is not an error, and the aggregate must not turn one into
    /// a diagnosis. The two failure shapes are distinct on the wire: a broken DB
    /// arrives as `{"error": …}` and the viewer prints it, an empty one arrives as
    /// `{"entries": []}` and the viewer says the vault is empty.
    #[test]
    fn test_list_on_empty_db_is_empty_not_an_error() {
        let db = tmp_db("list-empty");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let v = rt.block_on(async { list(State(AppState{ db: db.clone() })).await.0 });
        assert!(v.get("error").is_none(), "an empty DB is not an error: {}", v);
        assert_eq!(v["entries"].as_array().unwrap().len(), 0, "got {}", v);
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn test_search_read_list_roundtrip() {
        let db = tmp_db("rtl");
        { let s = brain_store::Store::open(&db).unwrap(); s.note_upsert("regras/global/w", "regras", Some("global"), "## web zebra", None, &[], false, None).unwrap(); }
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let v = search(State(AppState{ db: db.clone() }), Query(SearchQ{ query: "zebra".into(), top_k: Some(5), layer: None })).await.0;
            assert_eq!(v["results"].as_array().unwrap().len(), 1);
            let v = search(State(AppState{ db: db.clone() }), Query(SearchQ{ query: "   ".into(), top_k: None, layer: None })).await.0;
            assert!(v.get("error").is_some());
            let v = read(State(AppState{ db: db.clone() }), Query(ReadQ{ path: "regras/global/w".into() })).await.0;
            assert_eq!(v["path"], "regras/global/w");
            let v = read(State(AppState{ db: db.clone() }), Query(ReadQ{ path: "regras/global/nope".into() })).await.0;
            assert!(v.get("error").is_some());
            let v = list(State(AppState{ db: db.clone() })).await.0;
            assert_eq!(v["entries"].as_array().unwrap().len(), 1);
        });
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn test_serve_index_fallback() {
        // cargo runs unit tests with cwd=crate dir -> viewer/index.html missing
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let resp = rt.block_on(async { serve_index().await.into_response() });
        assert_eq!(resp.status(), axum::http::StatusCode::NOT_FOUND);
    }

    // ------------------------------------------------------------------
    // /api/graph (viewer-graph, batch F1: VG-BE-01..05 + endpoint-side half
    // of RF-13). Requests go through the real router via `oneshot`, so a
    // missing route registration fails here as a 404 rather than passing
    // against a handler called directly.
    // ------------------------------------------------------------------

    async fn graph_get_once(db: &str, project: Option<&str>) -> (axum::http::StatusCode, serde_json::Value) {
        use tower::ServiceExt as _;
        let app = router(db.to_string());
        let uri = match project {
            Some(p) => format!("/api/graph?project={p}"),
            None => "/api/graph".to_string(),
        };
        let req = axum::http::Request::builder()
            .uri(uri)
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    fn graph_get(db: &str, project: Option<&str>) -> (axum::http::StatusCode, serde_json::Value) {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(graph_get_once(db, project))
    }

    /// Two projects, one owned note, one linked-only note, one foreign note,
    /// one internal wikilink and one dangling wikilink — the minimal shape
    /// every filter/dangling test below needs.
    fn seed_graph(db: &str) {
        let s = brain_store::Store::open(db).unwrap();
        let a = s.project_create("proj-a", "").unwrap();
        let b = s.project_create("proj-b", "").unwrap();
        s.note_upsert("regras/global/owned-a", "regras", Some("global"), "## owned a", Some(a.id), &[], false, None).unwrap();
        s.note_upsert("regras/global/linked-a", "regras", Some("global"), "## linked a", None, &[], false, None).unwrap();
        s.note_link_project("regras/global/linked-a", "proj-a").unwrap();
        s.note_upsert("regras/global/owned-b", "regras", Some("global"), "## owned b", Some(b.id), &[], false, None).unwrap();
        // Internal wikilink (owned-a → linked-a) plus one dangling target.
        s.note_upsert("regras/global/owned-a", "regras", Some("global"), "## owned a\n\nSee [[regras/global/linked-a]]", Some(a.id), &[], false, None).unwrap();
        s.note_upsert("regras/global/owned-b", "regras", Some("global"), "## owned b\n\nSee [[regras/global/ghost]]", Some(b.id), &[], false, None).unwrap();
    }

    /// Endpoint-side half of RF-13(b): every served id resolves the way a
    /// click would resolve it — note nodes via `note_get` (the exact lookup
    /// `/api/read` performs), hubs via the `projeto:` prefix against
    /// `project_list`, and no edge pointing off the served node set.
    fn assert_graph_wiring(payload: &serde_json::Value, db: &str, what: &str) {
        let s = brain_store::Store::open(db).unwrap();
        let projects: Vec<String> = s.project_list().unwrap().into_iter().map(|p| p.name).collect();
        let nodes = payload["nodes"].as_array().unwrap();
        let edges = payload["edges"].as_array().unwrap();
        for n in nodes {
            match n["kind"].as_str().unwrap() {
                "note" => assert!(
                    s.note_get(n["id"].as_str().unwrap()).unwrap().is_some(),
                    "{what}: node `{}` is not resolvable via /api/read — every click on it would 404", n["id"]
                ),
                "project" => {
                    let id = n["id"].as_str().unwrap();
                    let name = id.strip_prefix("projeto:").unwrap_or("");
                    assert!(projects.iter().any(|p| p == name), "{what}: hub `{id}` names no known project");
                }
                k => panic!("{what}: unknown node kind `{k}`"),
            }
        }
        for e in edges {
            let (from, to) = (e["from"].as_str().unwrap(), e["to"].as_str().unwrap());
            assert!(
                nodes.iter().any(|n| n["id"] == from) && nodes.iter().any(|n| n["id"] == to),
                "{what}: edge {from}->{to} dangles off the served node set"
            );
            match e["kind"].as_str().unwrap() {
                "wikilink" => assert!(
                    s.note_get(from).unwrap().is_some() && s.note_get(to).unwrap().is_some(),
                    "{what}: wikilink {from}->{to} does not resolve on both ends"
                ),
                "project" => assert!(
                    to.starts_with("projeto:") && projects.iter().any(|p| *p == to["projeto:".len()..]),
                    "{what}: project edge points at `{to}`, not a served hub"
                ),
                k => panic!("{what}: unknown edge kind `{k}`"),
            }
        }
    }

    /// VG-BE-01 (RF-01/RF-02): shape, vocabularies, counts on the fixture.
    #[test]
    fn graph_serves_nodes_edges_dangling_and_projects() {
        let db = tmp_db("graph-shape");
        seed_graph(&db);
        let (status, v) = graph_get(&db, None);
        assert_eq!(status, axum::http::StatusCode::OK);
        let nodes = v["nodes"].as_array().unwrap();
        let edges = v["edges"].as_array().unwrap();
        assert!(v.get("dangling").and_then(|d| d.as_i64()).is_some(), "dangling must be an integer, always present: {v}");
        assert!(v["projects"].as_array().is_some(), "projects must always be present: {v}");
        let projs: Vec<&str> = v["projects"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
        assert_eq!(projs, vec!["proj-a", "proj-b"]);
        for n in nodes {
            for k in ["id", "kind", "label", "layer"] {
                assert!(n.get(k).is_some(), "node missing `{k}`: {n}");
            }
            assert!(n["kind"] == "note" || n["kind"] == "project", "kind out of vocabulary: {n}");
        }
        for e in edges {
            for k in ["from", "to", "kind"] {
                assert!(e.get(k).is_some(), "edge missing `{k}`: {e}");
            }
            assert!(e["kind"] == "wikilink" || e["kind"] == "project", "kind out of vocabulary: {e}");
        }
        // 3 notes + 2 hubs; 1 wikilink + 3 memberships served, 1 pruned.
        assert_eq!(nodes.len(), 5, "got {v}");
        assert_eq!(edges.len(), 4, "got {v}");
        assert_eq!(v["dangling"], 1, "got {v}");
        assert_graph_wiring(&v, &db, "unfiltered graph");
        let _ = std::fs::remove_file(&db);
    }

    /// VG-BE-02 (RF-03): the filter is owned ∪ linked computed in SQL, and the
    /// subgraph is induced — foreign notes, hubs and edges stay out.
    #[test]
    fn graph_filter_keeps_owned_and_linked_drops_other_project() {
        let db = tmp_db("graph-filter");
        seed_graph(&db);
        let (status, v) = graph_get(&db, Some("proj-a"));
        assert_eq!(status, axum::http::StatusCode::OK);
        let ids: Vec<&str> = v["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap()).collect();
        assert!(ids.contains(&"regras/global/owned-a"), "owned note must appear: {v}");
        assert!(ids.contains(&"regras/global/linked-a"), "linked-only note must appear: {v}");
        assert!(ids.contains(&"projeto:proj-a"), "hub must appear: {v}");
        assert!(!ids.contains(&"regras/global/owned-b"), "foreign note must not appear: {v}");
        assert!(!ids.contains(&"projeto:proj-b"), "foreign hub must not appear: {v}");
        let edges = v["edges"].as_array().unwrap();
        assert!(
            edges.iter().any(|e| e["from"] == "regras/global/owned-a" && e["to"] == "regras/global/linked-a" && e["kind"] == "wikilink"),
            "internal wikilink must survive: {v}"
        );
        assert_eq!(v["dangling"], 0, "dangling of a filtered-out note must not leak into the count: {v}");
        assert!(edges.iter().all(|e| e["to"] != "projeto:proj-b"), "no edge may point at the foreign hub: {v}");
        assert_graph_wiring(&v, &db, "proj-a subgraph");
        let _ = std::fs::remove_file(&db);
    }

    /// VG-BE-05 (RF-04): unknown project is a name, not a failure.
    #[test]
    fn graph_unknown_project_is_empty_not_an_error() {
        let db = tmp_db("graph-unknown");
        seed_graph(&db);
        let (status, v) = graph_get(&db, Some("nope"));
        assert_eq!(status, axum::http::StatusCode::OK);
        assert!(v.get("error").is_none(), "unknown project is a name, not a failure: {v}");
        assert_eq!(v["nodes"].as_array().unwrap().len(), 0);
        assert_eq!(v["edges"].as_array().unwrap().len(), 0);
        assert_eq!(v["dangling"], 0);
        assert_eq!(v["projects"].as_array().unwrap().len(), 2, "dropdown still served: {v}");
        let _ = std::fs::remove_file(&db);
    }

    /// VG-BE-03 (RF-05): dangling wikilinks prune to a counter, never to a node.
    #[test]
    fn graph_dangling_is_pruned_and_counted() {
        let db = tmp_db("graph-dangling");
        seed_graph(&db);
        let (_, v) = graph_get(&db, None);
        assert_eq!(v["dangling"], 1, "got {v}");
        let ids: Vec<&str> = v["nodes"].as_array().unwrap().iter().map(|n| n["id"].as_str().unwrap()).collect();
        assert!(!ids.contains(&"regras/global/ghost"), "no phantom node: {v}");
        assert!(
            v["edges"].as_array().unwrap().iter().all(|e| e["from"] != "regras/global/ghost" && e["to"] != "regras/global/ghost"),
            "no edge may reference the ghost: {v}"
        );
        assert_graph_wiring(&v, &db, "dangling graph");
        let _ = std::fs::remove_file(&db);
    }

    /// VG-BE-04 (RF-02/RF-07): hub ids cannot collide with note paths, and a
    /// hub carries absence-of-layer as null rather than as a guess.
    #[test]
    fn graph_project_hubs_do_not_collide_with_note_paths() {
        let db = tmp_db("graph-collision");
        seed_graph(&db);
        {
            let s = brain_store::Store::open(&db).unwrap();
            // A note whose path *contains* the hub prefix must still be a note.
            s.note_upsert("regras/global/projeto:tricky", "regras", Some("global"), "## tricky", None, &[], false, None).unwrap();
        }
        let (_, v) = graph_get(&db, None);
        let nodes = v["nodes"].as_array().unwrap();
        let tricky = nodes.iter().find(|n| n["id"] == "regras/global/projeto:tricky").unwrap();
        assert_eq!(tricky["kind"], "note");
        assert_eq!(tricky["label"], "projeto:tricky", "label is the last segment: {tricky}");
        let hub = nodes.iter().find(|n| n["id"] == "projeto:proj-a").unwrap();
        assert_eq!(hub["kind"], "project");
        assert!(hub.get("layer").is_some() && hub["layer"].is_null(), "hub has no layer — absence as null, not as guess: {hub}");
        assert_graph_wiring(&v, &db, "collision graph");
        let _ = std::fs::remove_file(&db);
    }

    /// VG-BE-04 (RF-06): past the 1000-row browse cap the graph is still
    /// complete, with zero orphaned edges.
    #[test]
    fn graph_has_no_cap_over_a_thousand_notes() {
        let db = tmp_db("graph-nocap");
        {
            let s = brain_store::Store::open(&db).unwrap();
            s.project_create("load", "").unwrap();
            for i in 0..1050 {
                let p = format!("sessoes/load/{i:04}");
                s.note_upsert(&p, "sessoes", None, "## load", None, &[], false, None).unwrap();
            }
            s.note_upsert("sessoes/load/hub", "sessoes", None, "## hub\n\nSee [[sessoes/load/0000]]", None, &[], false, None).unwrap();
            s.note_link_project("sessoes/load/hub", "load").unwrap();
        }
        let (status, v) = graph_get(&db, None);
        assert_eq!(status, axum::http::StatusCode::OK);
        // 1051 notes + 1 hub = every row, past the 1000 browse cap.
        assert_eq!(v["nodes"].as_array().unwrap().len(), 1052, "a capped query would serve fewer");
        assert!(
            v["edges"].as_array().unwrap().iter().any(|e| e["kind"] == "wikilink"),
            "the wikilink must survive: {v}"
        );
        assert_graph_wiring(&v, &db, "uncapped graph");
        let _ = std::fs::remove_file(&db);
    }

    /// RF-06 as a source scan: `graph_snapshot` must not cap and must not reuse
    /// the capped browse listing. A cap here draws edges toward nodes that were
    /// never served — the exact failure RF-06 exists to close.
    #[test]
    fn graph_snapshot_queries_are_uncapped_and_cover_linked_notes() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../brain-store/src/lib.rs");
        let src = std::fs::read_to_string(&p).unwrap();
        let start = src.find("pub fn graph_snapshot(").expect("graph_snapshot moved — update this guard");
        let tail = &src[start..];
        let end = tail.find("\n    pub fn ").unwrap_or(tail.len());
        let body = &tail[..end];
        assert!(!body.contains("LIMIT"), "graph_snapshot caps its queries — a capped graph orphans edges");
        assert!(!body.contains("recent_paths"), "graph_snapshot must not reuse the capped browse listing (RF-06)");
        assert!(body.contains("note_projects"), "graph_snapshot must union owned and linked notes in SQL (RF-03) — dropping the linked side blinds the filter");
    }

        /// RF-13(a): `renderGraph` reads only fields `/api/graph` serves.
    ///
    /// Same shape as the TD-011 guard, against the real handler payload: every
    /// dotted read in the function body must be a served leaf (`nodes`, `edges`,
    /// `dangling`, `projects`, node `id`/`kind`/`label`/`layer`, edge
    /// `from`/`to`/`kind`), an entry in `JS_MEMBERS`, or a read off a
    /// `JS_ROOTS` root. A canvas that reached for a field the endpoint never
    /// sent would render `undefined` labels on every node — the exact class
    /// TD-011 closed for the other three views.
    ///
    /// The assertion is two-sided on purpose: a pure phantom check passes for a
    /// `renderGraph` that reads nothing, so the test also requires the four
    /// top-level keys to be read — the dropdown cannot be populated from
    /// `projects` it never looks at, and the counter cannot show a `dangling`
    /// it never looks at either.
    #[test]
    fn viewer_graph_reads_only_fields_the_graph_endpoint_serves() {
        let db = tmp_db("viewer-graph-wiring");
        seed_graph(&db);
        let (_, payload) = graph_get(&db, None);
        let _ = std::fs::remove_file(&db);

        let html = viewer_code();
        let body = viewer_fn(&html, "renderGraph");
        assert_no_phantom_reads(&body, &payload, "renderGraph");

        let reads = entry_field_reads(&body);
        for need in ["nodes", "edges", "dangling", "projects"] {
            assert!(
                reads.contains(&need.to_string()),
                "renderGraph must read `{need}` from the graph payload, or the view it feeds is dead; reads: {reads:?}"
            );
        }
    }

    /// RF-13(b): a graph click resolves the way `/api/read` resolves it.
    ///
    /// Static half first, on the markup view: the `readNote(` call site inside
    /// `renderGraph` must hand over the node id verbatim — no
    /// `slice`/`split`/`replace`/`replaceAll`/`substr`/`substring` rewrite between
    /// the entry and the call (the layer-strip that made every browse row
    /// unclickable, guarded for `browseAll` three tests up). `replaceAll` is
    /// listed separately because the `.replace(` substring never matches inside
    /// `.replaceAll(` — without it the ban is evadable by one method name.
    /// The project-hub path needs no rewrite either: the hub's served `label` already *is* the project name,
    /// so filtering reads `label` instead of stripping a `projeto:` prefix.
    ///
    /// Behavioural half: every served note id resolves via `note_get` — the
    /// exact lookup `/api/read` performs — and every hub id is
    /// `projeto:` + its own served `label`, so the filter it applies names a
    /// real project.
    #[test]
    fn viewer_graph_click_hands_the_read_endpoint_a_resolvable_id() {
        let db = tmp_db("viewer-graph-click");
        seed_graph(&db);
        let (_, payload) = graph_get(&db, None);

        let markup = viewer_fn(&viewer_markup(), "renderGraph");
        let call = markup
            .find("readNote(")
            .unwrap_or_else(|| panic!("renderGraph no longer navigates via readNote — re-check this guard"));
        let args = &markup[call + "readNote(".len()..];
        let args = &args[..args.find(')').expect("unterminated readNote call")];
        for banned in ["slice", "split", "replace", "replaceAll", "substr", "substring"] {
            assert!(
                !args.contains(&format!(".{banned}(")),
                "renderGraph rewrites the node id with `.{banned}(` before handing it to readNote: {args:?} — /api/read matches notes.path exactly, so the click resolves to \"not found\""
            );
        }
        assert!(
            args.contains(".id"),
            "renderGraph must hand readNote the node's `id` verbatim; args: {args:?}"
        );
        assert!(
            markup.contains(".label"),
            "the project-hub click must filter from the hub's served `label` (already the project name), not by stripping the `projeto:` prefix"
        );

        {
            let s = brain_store::Store::open(&db).unwrap();
            for nd in payload["nodes"].as_array().unwrap() {
                let id = nd["id"].as_str().unwrap();
                if nd["kind"] == "note" {
                    assert!(
                        s.note_get(id).unwrap().is_some(),
                        "/api/graph serves note `{id}` but /api/read cannot resolve it — every click on that node would 404"
                    );
                } else {
                    let label = nd["label"].as_str().unwrap();
                    assert_eq!(
                        id,
                        format!("projeto:{label}"),
                        "hub `{id}` must be `projeto:` + its served label, or the click filters a name no project owns"
                    );
                }
            }
        }
        let _ = std::fs::remove_file(&db);
    }

    /// R-07 as a source scan: the graph handler degrades to `{"error"}`,
    /// never to a panic. The file's established `unwrap_or_default` shape stays
    /// allowed; hard accessors do not.
    #[test]
    fn graph_handler_fails_honest_without_panicking_accessors() {        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let src = std::fs::read_to_string(&p).unwrap();
        let start = src.find("async fn graph(").expect("graph handler moved — update this guard");
        let tail = &src[start..];
        let end = tail.find("\n}\n").expect("graph handler end not found");
        let body = &tail[..end];
        for banned in [".unwrap()", ".expect("] {
            assert!(!body.contains(banned), "graph handler uses `{banned}` — degrade to an error envelope instead (R-07)");
        }
        assert!(body.contains("{\"error\""), "graph handler must keep the honest error envelope");
    }

    // ------------------------------------------------------------------
    // /api/events (viewer-live-graph): SSE stream that emits `graph-changed`
    // when the DB changes. Tests verify content-type, event emission on
    // write, and viewer wiring.
    // ------------------------------------------------------------------

    /// `GET /api/events` returns 200 with `text/event-stream` content type.
    #[test]
    fn events_endpoint_returns_sse_content_type() {
        use tower::ServiceExt as _;
        let db = tmp_db("events-sse");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let app = router(db.clone());
        let req = axum::http::Request::builder()
            .uri("/api/events")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = rt.block_on(async { app.oneshot(req).await.unwrap() });
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let ct = resp.headers().get("content-type").unwrap().to_str().unwrap().to_string();
        assert!(ct.contains("text/event-stream"), "content-type must be text/event-stream, got: {ct}");
        let _ = std::fs::remove_file(&db);
    }

    /// After a write, the SSE stream emits `graph-changed` within the polling
    /// interval. We connect, wait for the first tick (2s), write a note, and
    /// assert the event arrives.
    #[test]
    fn events_emits_graph_changed_on_write() {
        use tower::ServiceExt as _;
        use tokio_stream::StreamExt as _;
        let db = tmp_db("events-emit");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let app = router(db.clone());
        let req = axum::http::Request::builder()
            .uri("/api/events")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = rt.block_on(async { app.oneshot(req).await.unwrap() });
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let mut body = resp.into_body().into_data_stream();
        // Wait for the first poll tick (interval = 2s). We don't consume the
        // body; we just wait, then write, then check the stream produces data.
        rt.block_on(async {
            tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
            // Write a note — this changes count_notes/count_chunks.
            let s = brain_store::Store::open(&db).unwrap();
            s.note_upsert("regras/global/sse-test", "regras", Some("global"), "## sse", None, &[], false, None).unwrap();
            drop(s);
            // The next tick (within 2s) should emit graph-changed.
            // We read from the body with a timeout.
            let mut buf = Vec::new();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                match tokio::time::timeout(
                    std::time::Duration::from_millis(500),
                    body.next(),
                ).await {
                    Ok(Some(Ok(chunk))) => {
                        buf.extend_from_slice(&chunk);
                        let s = String::from_utf8_lossy(&buf);
                        if s.contains("event: graph-changed") && s.contains("data: graph-changed") {
                            return;
                        }
                    }
                    Ok(Some(Err(_))) | Ok(None) => break,
                    Err(_) => continue,
                }
            }
            panic!("graph-changed event not received within 5s after write");
        });
        let _ = std::fs::remove_file(&db);
    }

    /// Viewer script contains `EventSource` and `graph-changed` — the frontend
    /// wiring for live graph updates.
    #[test]
    fn viewer_script_contains_eventsource_and_graph_changed() {
        let src = viewer_source();
        assert!(src.contains("EventSource"), "viewer must use EventSource for SSE");
        assert!(src.contains("graph-changed"), "viewer must listen for graph-changed events");
        assert!(src.contains("/api/events"), "viewer must connect to /api/events");
    }

    /// Guard against the self-close bug: `renderGraph()` must NOT call
    /// `stopGraph()`. The `graph-changed` SSE handler calls `renderGraph()`;
    /// if `renderGraph()` closes the EventSource, the stream dies after the
    /// first event and "live" updates are broken.
    ///
    /// Regression test for: EventSource self-close on first `graph-changed`.
    #[test]
    fn render_graph_does_not_close_event_source() {
        let src = viewer_source();
        // Extract renderGraph() body: from its declaration to the next top-level
        // function declaration (or end of file).
        let rg_start = src.find("async function renderGraph()")
            .expect("renderGraph must exist");
        let rg_rest = &src[rg_start..];
        let next_fn = rg_rest[1..]
            .find("\nasync function")
            .or_else(|| rg_rest[1..].find("\nfunction"))
            .map(|i| i + 1)
            .unwrap_or(rg_rest.len());
        let rg_body = &rg_rest[..next_fn];

        // 1. renderGraph() must NOT call stopGraph() — that would close the
        //    EventSource that fired 'graph-changed', killing live updates.
        assert!(
            !rg_body.contains("stopGraph()"),
            "renderGraph() must NOT call stopGraph() — that closes the EventSource \
             that fired 'graph-changed', killing live updates after the first event"
        );

        // 2. ensureGraphEventSource() must exist and be called from renderGraph.
        assert!(
            src.contains("function ensureGraphEventSource()"),
            "ensureGraphEventSource() must exist to manage EventSource lifecycle"
        );
        assert!(
            rg_body.contains("ensureGraphEventSource()"),
            "renderGraph() must call ensureGraphEventSource() to open the SSE stream"
        );

        // 3. stopGraph() must still close EventSource — but only when leaving
        //    the graph view (checkStatus/browseAll/search call it).
        let stop_start = src.find("function stopGraph()")
            .expect("stopGraph must exist");
        let stop_rest = &src[stop_start..];
        let stop_next = stop_rest[1..]
            .find("\nasync function")
            .or_else(|| stop_rest[1..].find("\nfunction"))
            .map(|i| i + 1)
            .unwrap_or(stop_rest.len());
        let stop_body = &stop_rest[..stop_next];
        assert!(
            stop_body.contains("graphEventSource.close()"),
            "stopGraph() must still close EventSource for view switches"
        );
    }
}
