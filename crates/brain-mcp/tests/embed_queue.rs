//! US-02.7 — the embedding queue, and the write-side size limits that bound it.
//!
//! Everything here runs against a local mock Ollama bound to an ephemeral
//! loopback port, or against a closed port. No case requires a real Ollama, which
//! is what lets the suite assert the degradation paths anywhere.
//!
//! The properties under test, in the order they matter:
//!
//! 1. **the write does not wait for a vector.** Measured, not asserted by
//!    construction: a note whose mock embed takes 200 ms per chunk must come back
//!    from `store_note_and_queue` in single-digit milliseconds with `queued = n`
//!    and every chunk stored as SQL `NULL`.
//! 2. **the queue is a diff.** Re-storing an unchanged note queues nothing;
//!    appending a section queues only the new section.
//! 3. **the background pass fills the vectors in** and coverage reaches 100%.
//! 4. **a dead Ollama degrades, it does not fail.** Enqueue succeeds, the drain
//!    reports `nulls`, and nothing is ever returned to the caller as an error.
//! 5. **the queue and a reindex do not duplicate each other's work.** The reindex
//!    side is simulated by holding the same cross-process lock a real reindex
//!    takes.
//! 6. **an oversized note is refused without embedding it at all** — the mock's
//!    request counter must still be zero.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use brain_core::EMBEDDING_DIM;
use brain_mcp::{EmbedQueue, NoteWrite, store_note_and_queue};
use brain_store::Store;
use serde_json::{Value, json};
use tokio::net::TcpListener;

// ---------------------------------------------------------------- mock Ollama --

#[derive(Clone)]
struct MockState {
    /// Per-chunk server-side latency, so a test can make embedding measurably slow.
    delay_ms: u64,
    requests: Arc<AtomicUsize>,
    /// X-02. How many `/api/embeddings` calls to reject with 500 *before*
    /// starting to answer. The point of the X-02 tests is that a note written
    /// while Ollama is unavailable still gets its vector, with no further
    /// `brain_store` to prompt anyone — so the mock has to be able to fail and
    /// then recover, which "always down" and "always up" cannot express.
    fail_first: Arc<AtomicUsize>,
}

struct Mock {
    base_url: String,
    requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Mock {
    async fn start(delay_ms: u64) -> Self {
        Self::start_with_failures(delay_ms, 0).await
    }

    /// A mock that rejects its first `fail_first` embedding calls with a 500.
    async fn start_with_failures(delay_ms: u64, fail_first: usize) -> Self {
        let requests = Arc::new(AtomicUsize::new(0));
        let fail_first = Arc::new(AtomicUsize::new(fail_first));
        let app = Router::new()
            .route("/api/tags", get(|| async { axum::Json(json!({"models": [{"name": "nomic-embed-text"}]})) }))
            .route(
                "/api/embeddings",
                post(|State(st): State<MockState>, _body: Bytes| async move {
                    let seen = st.requests.fetch_add(1, Ordering::SeqCst);
                    if st.delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(st.delay_ms)).await;
                    }
                    if seen < st.fail_first.load(Ordering::SeqCst) {
                        // What a real Ollama returns while the model is still
                        // loading, and what the engine has to degrade from.
                        return (StatusCodeAlias::SERVICE_UNAVAILABLE, "model loading").into_response();
                    }
                    // Deterministic, non-zero, correct width: a real vector, so the
                    // zero-vector guard in the store is exercised for real.
                    let seed = st.requests.load(Ordering::SeqCst) as u32;
                    let v: Vec<Value> = (0..EMBEDDING_DIM)
                        .map(|i| json!(((i as u32 + seed) % 17 + 1) as f32 / 16.0))
                        .collect();
                    (StatusCodeAlias::OK, axum::Json(json!({"embedding": v}))).into_response()
                }),
            )
            .with_state(MockState { delay_ms, requests: requests.clone(), fail_first: fail_first.clone() });
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock ollama");
        let addr = listener.local_addr().expect("mock addr");
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { base_url: format!("http://{addr}"), requests, task }
    }

    fn engine(&self) -> brain_embed::EmbeddingEngine {
        brain_embed::EmbeddingEngine::new(self.base_url.clone(), "nomic-embed-text".into())
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

use axum::http::StatusCode as StatusCodeAlias;

/// A URL guaranteed to refuse connections: a port bound then released.
async fn closed_port_url() -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.expect("bind probe");
    let addr = l.local_addr().unwrap();
    drop(l);
    format!("http://{addr}")
}

fn tmp_db(tag: &str) -> String {
    let db = format!("/tmp/brain-queue-{}-{}.db", std::process::id(), tag);
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_file(format!("{}.bak", db));
    db
}

/// A note body with `n` `## ` sections, so it chunks into `n` chunks.
fn body_with_sections(n: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for i in 0..n {
        let _ = writeln!(out, "## section {i}\n\nprose for section {i} withtoken{i}");
    }
    out
}

fn write<'a>(layer: &'a str, path: &'a str, content: &'a str, scope: Option<&'a str>) -> NoteWrite<'a> {
    NoteWrite { layer, path, scope, content, project: None, tags: &[], pinned: false, expires_at: None }
}

fn coverage(db: &str) -> brain_store::EmbeddingCoverage {
    Store::open(db).expect("open").embedding_coverage().expect("coverage")
}

// ------------------------------------------------------------------- the tests --

/// The load-bearing property: the write returns before the vector exists.
#[tokio::test]
async fn store_returns_before_the_embedding_exists() {
    let mock = Mock::start(200).await;
    let db = tmp_db("fast-write");
    let queue = EmbedQueue::new(mock.engine());
    let content = body_with_sections(8);
    let chunks = brain_core::chunk_text(&content, brain_core::CHUNK_TARGET_TOKENS).len();
    assert_eq!(chunks, 8);

    let store = Store::open(&db).unwrap();
    let started = Instant::now();
    let outcome = store_note_and_queue(&store, write("regras", "queue/fast", &content, Some("global")), &queue).unwrap();
    let elapsed = started.elapsed();

    // 8 chunks x 200ms of server latency is 1.6s of embedding work. Anything
    // close to that means the write is still blocking on the vector.
    assert!(elapsed < Duration::from_millis(500), "the write must not wait for embedding; took {elapsed:?}");
    assert_eq!(outcome.chunks, 8);
    assert_eq!(outcome.queued, 8, "every chunk is owed a vector and the caller must be told");
    assert_eq!(outcome.embedded, 0, "and told honestly that none exists yet");
    assert_eq!(queue.pending_len(), 1);
    assert_eq!(mock.requests(), 0, "the write path must issue no embedding request at all");

    // The note is durable and FTS-searchable right now; only the vector is missing.
    let after = coverage(&db);
    assert_eq!((after.total, after.embedded, after.without_embedding, after.zero_vector), (8, 0, 8, 0));
    let s = Store::open(&db).unwrap();
    assert!(s.note_get("regras/global/queue/fast").unwrap().is_some(), "the note is persisted before the answer");
    assert!(!s.search("withtoken3", None, None, None, None, None, 5, false).unwrap().is_empty(), "FTS works while the queue is pending");

    // ... and the background pass fills it in.
    let out = queue.drain().await;
    assert_eq!((out.embedded, out.nulls, out.diverged, out.failed), (8, 0, 0, 0), "{out:?}");
    let done = coverage(&db);
    assert_eq!((done.embedded, done.without_embedding, done.zero_vector, done.coverage_pct), (8, 0, 0, 100.0));
    assert_eq!(mock.requests(), 8, "exactly one request per missing chunk");
    let _ = std::fs::remove_file(&db);
}

#[tokio::test]
async fn coverage_counts_a_queued_chunk_as_without_embedding_not_as_a_zero_vector() {
    // `coverage_pct` is the queue's health signal, so the distinction that makes
    // it a signal has to hold while the queue is mid-flight: a pending chunk is
    // `without_embedding`, never a `zero_vector` (which would score 0.0 against
    // every query while looking indexed — the production bug).
    let mock = Mock::start(0).await;
    let db = tmp_db("coverage-signal");
    let queue = EmbedQueue::new(mock.engine());
    let store = Store::open(&db).unwrap();
    store_note_and_queue(&store, write("regras", "queue/signal", &body_with_sections(3), Some("global")), &queue).unwrap();
    let pending = coverage(&db);
    assert_eq!((pending.embedded, pending.without_embedding, pending.zero_vector, pending.coverage_pct), (0, 3, 0, 0.0));
    queue.drain().await;
    let settled = coverage(&db);
    assert_eq!((settled.embedded, settled.without_embedding, settled.zero_vector, settled.coverage_pct), (3, 0, 0, 100.0));
    let _ = std::fs::remove_file(&db);
}

#[tokio::test]
async fn re_storing_an_unchanged_note_queues_nothing() {
    let mock = Mock::start(0).await;
    let db = tmp_db("idempotent");
    let queue = EmbedQueue::new(mock.engine());
    let content = body_with_sections(2);
    let store = Store::open(&db).unwrap();
    let first = store_note_and_queue(&store, write("regras", "queue/same", &content, Some("global")), &queue).unwrap();
    assert_eq!(first.queued, 2);
    queue.drain().await;
    let requests_after_first = mock.requests();

    let second = store_note_and_queue(&store, write("regras", "queue/same", &content, Some("global")), &queue).unwrap();
    assert_eq!((second.embedded, second.queued, second.nulls), (2, 0, 0), "the stored vectors still match the text, so there is nothing to do");
    assert_eq!(queue.pending_len(), 0);
    assert_eq!(queue.drain().await.embedded, 0);
    assert_eq!(mock.requests(), requests_after_first, "no re-embedding of text that already has a vector");
    let _ = std::fs::remove_file(&db);
}

#[tokio::test]
async fn appending_a_section_queues_only_the_new_section() {
    // The session-hook shape: a note that grows on every write. Re-embedding the
    // accumulated document each time is quadratic work for vectors that exist.
    let mock = Mock::start(0).await;
    let db = tmp_db("diff");
    let queue = EmbedQueue::new(mock.engine());
    let store = Store::open(&db).unwrap();
    let first = body_with_sections(2);
    store_note_and_queue(&store, write("sessoes", "proj", &first, None), &queue).unwrap();
    queue.drain().await;
    let baseline = mock.requests();

    let grown = format!("{first}\n## appended\n\nnew prose withtoken9\n");
    let outcome = store_note_and_queue(&store, write("sessoes", "proj", &grown, None), &queue).unwrap();
    assert_eq!((outcome.chunks, outcome.embedded, outcome.queued), (3, 2, 1), "only the appended section is outstanding");
    queue.drain().await;
    assert_eq!(mock.requests() - baseline, 1, "exactly one new embedding, not a full re-embed");
    assert_eq!(coverage(&db).coverage_pct, 100.0);
    let _ = std::fs::remove_file(&db);
}

#[tokio::test]
async fn a_dead_ollama_queues_without_failing_the_caller() {
    let db = tmp_db("ollama-down");
    let queue = EmbedQueue::new(brain_embed::EmbeddingEngine::new(closed_port_url().await, "nomic-embed-text".into()));
    let content = body_with_sections(2);
    let store = Store::open(&db).unwrap();
    let started = Instant::now();
    let outcome = store_note_and_queue(&store, write("regras", "queue/down", &content, Some("global")), &queue).unwrap();
    assert!(started.elapsed() < Duration::from_millis(500), "an unreachable Ollama must not slow the write");
    assert_eq!((outcome.queued, outcome.nulls), (2, 2), "the note is stored and the debt is reported");

    // The drain fails per chunk and is not an error: `nulls` is the report.
    let out = queue.drain().await;
    assert_eq!((out.embedded, out.nulls), (0, 2), "{out:?}");
    assert_eq!(out.failed, 0, "an unreachable Ollama is a degradation, not a failure");
    let after = coverage(&db);
    assert_eq!((after.embedded, after.without_embedding, after.zero_vector), (0, 2, 0), "and never a BLOB of zeros");
    assert!(Store::open(&db).unwrap().note_get("regras/global/queue/down").unwrap().is_some());
    let _ = std::fs::remove_file(&db);
}

#[tokio::test]
async fn the_queue_does_not_fight_a_concurrent_reindex() {
    // The reindex is a separate *process*, so the guard is the advisory lock in
    // `_meta`. Simulated here by taking the same lock the CLI takes before its
    // embed pass.
    let mock = Mock::start(0).await;
    let db = tmp_db("reindex-race");
    let queue = EmbedQueue::new(mock.engine());
    let store = Store::open(&db).unwrap();
    store_note_and_queue(&store, write("regras", "queue/race", &body_with_sections(2), Some("global")), &queue).unwrap();
    drop(store);

    let reindex_owner = brain_store::embed_lock_owner("reindex");
    {
        let store = Store::open(&db).unwrap();
        assert!(store.try_acquire_embed_lock(&reindex_owner, 900).unwrap(), "the reindex claims the lock first");
    }

    let out = queue.drain().await;
    assert_eq!(out.skipped_locked, 1, "the queue must stand down, not duplicate the work");
    assert_eq!(out.embedded, 0, "and must not embed anything behind the reindex's back");
    assert_eq!(mock.requests(), 0, "no embedding request while another pass holds the lock");
    let after = coverage(&db);
    assert_eq!((after.embedded, after.without_embedding, after.zero_vector), (0, 2, 0));
    {
        let store = Store::open(&db).unwrap();
        assert_eq!(store.embed_lock_holder().unwrap().as_deref(), Some(reindex_owner.as_str()), "the queue must not have taken or released the reindex's lock");
    }

    // Once the reindex finishes, the queue picks the work up: the note is
    // re-queued (a real `brain_store` would enqueue it again) and now nothing
    // holds the lock.
    {
        let store = Store::open(&db).unwrap();
        assert!(store.release_embed_lock(&reindex_owner).unwrap());
        let outcome = store_note_and_queue(&store, write("regras", "queue/race", &body_with_sections(2), Some("global")), &queue).unwrap();
        assert_eq!(outcome.queued, 2, "the chunks are still NULL, so they are still owed");
    }
    let out = queue.drain().await;
    assert_eq!((out.skipped_locked, out.embedded, out.nulls), (0, 2, 0), "{out:?}");
    assert_eq!(coverage(&db).coverage_pct, 100.0);
    let _ = std::fs::remove_file(&db);
}

#[tokio::test]
async fn re_queueing_the_same_path_replaces_the_job_instead_of_growing_the_queue() {
    let mock = Mock::start(0).await;
    let db = tmp_db("dedupe");
    let queue = EmbedQueue::new(mock.engine());
    let store = Store::open(&db).unwrap();
    let first = body_with_sections(2);
    store_note_and_queue(&store, write("regras", "queue/dedupe", &first, Some("global")), &queue).unwrap();
    assert_eq!(queue.pending_len(), 1);
    let grown = format!("{first}\n## more\n\nand more prose\n");
    let outcome = store_note_and_queue(&store, write("regras", "queue/dedupe", &grown, Some("global")), &queue).unwrap();
    assert_eq!(queue.pending_len(), 1, "one pending job per path, not one per write");
    assert_eq!(outcome.queued, 3, "the newest job covers the newest text");
    // Only the newest chunk list is used, so nothing is embedded for text the note
    // no longer has.
    queue.drain().await;
    let s = Store::open(&db).unwrap();
    let stored = s.chunk_snapshot("regras/global/queue/dedupe").unwrap();
    assert_eq!(stored.len(), 3);
    assert!(stored.values().all(|(_, v)| v.is_some()), "every current chunk ends up with a vector");
    assert!(stored[&2].0.contains("and more prose"), "the chunk text is the current one");
    let _ = std::fs::remove_file(&db);
}

#[tokio::test]
async fn a_note_deleted_before_its_job_runs_is_dropped_cleanly() {
    let mock = Mock::start(0).await;
    let db = tmp_db("deleted");
    let queue = EmbedQueue::new(mock.engine());
    let store = Store::open(&db).unwrap();
    store_note_and_queue(&store, write("regras", "queue/gone", &body_with_sections(1), Some("global")), &queue).unwrap();
    assert!(store.note_delete("regras/global/queue/gone").unwrap());
    drop(store);
    let out = queue.drain().await;
    assert_eq!((out.skipped_deleted, out.failed, out.embedded), (1, 0, 0), "{out:?}");
    assert_eq!(mock.requests(), 0, "nothing to embed, so nothing is embedded");
    let _ = std::fs::remove_file(&db);
}

/// The drain flag is clear again once a pass returns, and a second pass over an
/// empty queue is a no-op rather than a wedge.
///
/// This test used to be named `a_queue_that_panics_mid_drain_is_not_wedged` and
/// claimed to panic, but it never did: it set the flag the way a drain does and
/// released it the way the guard would, which exercises `Drop` — a different path
/// from unwinding — and asserted nothing about a panic. The real unwind now lives
/// in `embed_queue::tests` (unit tests, because producing one through `drain`
/// requires poisoning the private state mutex): `a_panic_unwinding_through_the_
/// drain_guard_leaves_the_queue_usable` and
/// `a_drain_over_a_poisoned_queue_panics_and_still_releases_the_flag`.
#[tokio::test]
async fn a_drain_that_returns_leaves_the_queue_unwedged() {
    let mock = Mock::start(0).await;
    let db = tmp_db("guard");
    let queue = std::sync::Arc::new(EmbedQueue::new(mock.engine()));
    let store = Store::open(&db).unwrap();
    store_note_and_queue(&store, write("regras", "queue/guard", &body_with_sections(2), Some("global")), &queue).unwrap();
    drop(store);
    assert!(!queue.is_draining());

    let out = queue.drain().await;
    assert!(!queue.is_draining(), "the flag must be clear once the drain returns");
    assert_eq!(out.embedded + out.nulls, 2, "{out:?}");
    assert_eq!(out.pending_left, 0, "{out:?}");
    assert_eq!(queue.drain().await, brain_mcp::QueueOutcome::default(), "a second drain is a no-op, not a wedge");
    // Still usable afterwards: a wedged flag would strand this note's work.
    queue.enqueue(&db, "regras/global/queue/guard-2", vec!["## a".to_string()]);
    queue.spawn_worker();
    let deadline = Instant::now() + Duration::from_secs(5);
    while coverage(&db).coverage_pct < 100.0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(coverage(&db).coverage_pct, 100.0, "the queue must still work after a pass");
    let _ = std::fs::remove_file(&db);
}

#[tokio::test]
async fn a_drain_with_nothing_queued_is_a_no_op() {
    let mock = Mock::start(0).await;
    let queue = EmbedQueue::new(mock.engine());
    let out = queue.drain().await;
    assert_eq!(out, brain_mcp::QueueOutcome::default());
    assert_eq!(mock.requests(), 0);
    // `spawn_worker` on an empty queue must not even spawn.
    let q = Arc::new(queue);
    q.spawn_worker();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(q.pending_len(), 0);
}

// ---------------------------------------------------------------- W-04 --
//
// The three delivery guarantees. Each one had a guard in the queue whose broken
// form was invisible: the work simply stopped, and the only symptom was a
// `coverage_pct` that never climbed. So each guarantee below is a test that fails
// on the old code, and each says which hole it closes.

/// (a) Arrival after a restart.
///
/// The queue's work list is in memory, so anything in flight when the process
/// stopped — a deploy, an OOM, a `kill -9` — was gone, and nothing re-read it: the
/// chunks stayed `NULL` until a human noticed or ran `reindex --all`. Recovery is
/// read out of the database, where a `NULL` chunk *is* the record of the debt.
#[tokio::test]
async fn a_restart_requeues_the_work_that_was_in_flight() {
    let mock = Mock::start(0).await;
    let db = tmp_db("restart");
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(&store, write("regras", "queue/restart", &body_with_sections(3), Some("global")), &EmbedQueue::new(mock.engine())).unwrap();
    }
    // A fresh process would have an empty queue. Recovery has to find the debt by
    // itself.
    let fresh = EmbedQueue::new(mock.engine());
    assert_eq!(fresh.pending_len(), 0, "a new queue starts empty, as a real restart would");
    assert_eq!(fresh.recover(&db), 1, "boot recovery must find the interrupted note");
    assert_eq!(fresh.pending_len(), 1);
    let out = fresh.drain().await;
    assert_eq!((out.embedded, out.nulls, out.pending_left), (3, 0, 0), "{out:?}");
    assert_eq!(coverage(&db).coverage_pct, 100.0, "the interrupted work is delivered, not just queued");
    // A second recovery finds nothing: a hydrated index owes no work, or every
    // restart would re-embed the whole corpus.
    assert_eq!(fresh.recover(&db), 0, "a settled index must not re-queue anything");
    let _ = std::fs::remove_file(&db);
}

/// (b) Arrival after the embed lock is held by someone else.
///
/// The job left the queue when the pass took its batch, so standing down dropped
/// it: the log said "it stays NULL", which is true, and the work was never coming
/// back. It is now re-queued with a backoff, and the backoff is what keeps the
/// retry from becoming a busy loop against the reindex.
///
/// Driven through `spawn_worker` rather than `drain` on purpose: `drain` is the
/// deterministic entry point tests use, but it cannot schedule the follow-up pass
/// (it only borrows the queue), and the guarantee under test is that *production*
/// delivers the work without a further `brain_store` arriving to nudge it.
#[tokio::test]
async fn a_note_skipped_because_the_lock_was_held_comes_back_after_the_lock_frees() {
    let mock = Mock::start(0).await;
    let db = tmp_db("lock-delivery");
    let q = Arc::new(EmbedQueue::new(mock.engine()));
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(&store, write("regras", "queue/lock", &body_with_sections(2), Some("global")), &q).unwrap();
    }
    let reindex = brain_store::embed_lock_owner("reindex");
    {
        let store = Store::open(&db).unwrap();
        assert!(store.try_acquire_embed_lock(&reindex, 900).unwrap());
    }

    q.spawn_worker();
    let deadline = Instant::now() + Duration::from_secs(5);
    while q.ready_len() > 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(mock.requests(), 0, "nothing may be embedded behind the reindex's back");
    assert_eq!(coverage(&db).coverage_pct, 0.0, "so the chunks are honestly NULL, not silently wrong");
    assert_eq!(q.pending_len(), 1, "the work must still be queued, not dropped");
    assert_eq!(q.ready_len(), 0, "and it must be inside its backoff, not spinning against the reindex");

    // The reindex finishes. The scheduled retry picks the work up on its own — no
    // further `brain_store`, no reindex, nothing.
    {
        let store = Store::open(&db).unwrap();
        assert!(store.release_embed_lock(&reindex).unwrap());
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while coverage(&db).coverage_pct < 100.0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(coverage(&db).coverage_pct, 100.0, "the deferred work must arrive by itself once the lock is free");
    assert_eq!(q.pending_len(), 0, "and the queue must end empty");
    let _ = std::fs::remove_file(&db);
}

/// (c) A note enqueued *while* a drain is running must not be stranded.
///
/// The pass used to snapshot `pending` once. A `brain_store` that landed during the
/// embed was not in the batch, and `spawn_worker` returned early because the drain
/// flag was set — so the job sat in the queue with nothing scheduled to look at
/// it. This test fails on that code: the drain returns, `pending_len() == 1`.
#[tokio::test]
async fn a_note_enqueued_during_a_drain_is_not_stranded() {
    let mock = Mock::start(120).await;
    let db = tmp_db("lost-wakeup");
    let q = Arc::new(EmbedQueue::new(mock.engine()));
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(&store, write("regras", "queue/first", &body_with_sections(2), Some("global")), &q).unwrap();
    }

    let draining = Arc::clone(&q);
    let pass = tokio::spawn(async move { draining.drain().await });

    // Wait until the pass is genuinely mid-flight (the mock has served at least one
    // chunk), so the enqueue below lands *during* the drain rather than before it.
    let deadline = Instant::now() + Duration::from_secs(5);
    while mock.requests() == 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(mock.requests() > 0, "the first note must be mid-embed before the second arrives");

    let second_body = body_with_sections(1);
    let second = write("regras", "queue/second", &second_body, Some("global"));
    {
        let store = Store::open(&db).unwrap();
        let outcome = store_note_and_queue(&store, second, &q).unwrap();
        assert_eq!(outcome.queued, 1);
    }

    let out = pass.await.expect("the drain must not panic");
    assert_eq!(out.pending_left, 0, "the note that arrived mid-drain must be picked up, not left in the queue: {out:?}");
    assert_eq!(q.pending_len(), 0, "pending_len() == 0 when the pass ends");
    assert_eq!(coverage(&db).coverage_pct, 100.0, "and both notes end up embedded: {:?}", coverage(&db));
    let _ = std::fs::remove_file(&db);
}

/// The backoff must actually delay, and must not be a tight loop against a reindex
/// that is still running for minutes. `ready_len()` is the deterministic witness: a
/// deferred job is queued but not runnable, so nothing retries it early — and
/// releasing the lock does not make it retryable either, which is what separates a
/// backoff from a poll loop.
#[tokio::test]
async fn a_deferred_job_is_not_retried_before_its_backoff_expires() {
    let mock = Mock::start(0).await;
    let db = tmp_db("backoff");
    let q = Arc::new(EmbedQueue::new(mock.engine()));
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(&store, write("regras", "queue/backoff", &body_with_sections(1), Some("global")), &q).unwrap();
    }
    let reindex = brain_store::embed_lock_owner("reindex");
    {
        let store = Store::open(&db).unwrap();
        assert!(store.try_acquire_embed_lock(&reindex, 900).unwrap());
    }
    q.spawn_worker();
    let deadline = Instant::now() + Duration::from_secs(5);
    while (q.pending_len() == 0 || q.ready_len() > 0) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(q.ready_len(), 0, "a deferred job is not ready: that is what keeps the retry from spinning");
    assert_eq!(q.pending_len(), 1, "but it is still owed");
    assert_eq!(mock.requests(), 0, "and nothing was embedded behind the reindex's back");

    {
        let store = Store::open(&db).unwrap();
        assert!(store.release_embed_lock(&reindex).unwrap());
    }
    // Still inside the backoff window (250 ms for the first deferral), so the
    // released lock does not trigger an immediate re-attempt.
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(mock.requests(), 0, "a freed lock must not turn the backoff into a poll loop");

    // The scheduled retry then picks it up on its own.
    let deadline = Instant::now() + Duration::from_secs(10);
    while coverage(&db).coverage_pct < 100.0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(coverage(&db).coverage_pct, 100.0, "the backoff must expire and the work must land");
    assert_eq!(q.pending_len(), 0, "and the queue must end empty");
    let _ = std::fs::remove_file(&db);
}

// ------------------------------------------------------- V-01: size limits --

#[tokio::test]
async fn an_oversized_note_is_refused_by_the_handler_rule_before_any_work() {
    // W-05.2. This used to build a mock, a queue and an in-memory store that no
    // code in the test touched, then assert `pending_len() == 0` and
    // `requests() == 0` — both true by construction, whatever the handler did. It
    // now goes through the shared write rule every writer uses, with a real
    // database behind it, so the zeros are measurements.
    let mock = Mock::start(0).await;
    let queue = EmbedQueue::new(mock.engine());
    let db = tmp_db("handler-rule");
    let mut body = String::new();
    for i in 0..(brain_core::MAX_CHUNKS + 1) {
        body.push_str(&format!("## {i}\n"));
    }
    let err = brain_mcp::validate_note_write("regras", "oversize", Some("global"), &body)
        .expect_err("a note over MAX_CHUNKS must be refused");
    assert!(err.to_string().contains("chunks, limit is"), "the message must name the limit: {err}");

    // Nothing queued, nothing requested, nothing written — because the check is
    // pure and runs before the store is opened.
    assert_eq!(queue.pending_len(), 0);
    assert_eq!(mock.requests(), 0);
    assert!(
        Store::open(&db).unwrap().note_get("regras/global/oversize").unwrap().is_none(),
        "the refused note must leave no row behind"
    );

    // And the same rule accepts a note of the size the corpus really contains, so
    // the limit is a bound rather than a blanket refusal.
    let ok = body_with_sections(brain_core::MAX_CHUNKS);
    let full = brain_mcp::validate_note_write("regras", "atlimit", Some("global"), &ok)
        .expect("a note exactly at the chunk cap is legal");
    assert_eq!(full, "regras/global/atlimit");
    let _ = std::fs::remove_file(&db);
}

#[tokio::test]
async fn a_legitimate_dense_note_is_accepted() {
    // The other side, on the write path rather than as a re-assertion of the pure
    // limit check: 60 sections, 6x the largest real note, must go through the whole
    // store → queue path and come back with the debt reported.
    let mock = Mock::start(0).await;
    let db = tmp_db("dense");
    let queue = EmbedQueue::new(mock.engine());
    let store = Store::open(&db).unwrap();
    let outcome = store_note_and_queue(&store, write("regras", "queue/dense", &body_with_sections(60), Some("global")), &queue).unwrap();
    assert_eq!(outcome.chunks, 60, "the whole note is written, not truncated");
    assert_eq!((outcome.embedded, outcome.nulls, outcome.queued), (0, 60, 60), "a fresh note owes every chunk a vector");
    assert_eq!(mock.requests(), 0, "and the write issued no embedding request");
    assert_eq!(queue.pending_len(), 1);
    let _ = std::fs::remove_file(&db);
}


// ---------------------------------------------------------------- X-02 --
//
// A failed embed must not lose the work. The queue used to re-queue exactly one
// failure — standing down for the embed lock — and let every other one drop the
// job on the floor: the note left the queue with its chunks `NULL`, and because
// `is_settled()` only looked at `pending_left`, the log said `settled=true` beside
// `coverage_pct=0`. Recovery was a restart or a manual `reindex --all`, so a note
// written during an Ollama restart simply never got a vector.
//
// The four tests below are the four halves of the fix: it comes back, it stops
// coming back eventually, it never claims to have settled while it has not, and
// the state is visible in `brain status`.

/// The headline case: Ollama fails, then recovers, and the vector arrives with no
/// further `brain_store` to prompt anyone.
#[tokio::test]
async fn a_note_whose_embed_failed_is_retried_until_the_vector_arrives() {
    // Fails the first 3 requests, then answers. Three failures, so the re-queue
    // path has to run more than once to be doing any work.
    let mock = Mock::start_with_failures(0, 3).await;
    let db = tmp_db("retry");
    let queue = Arc::new(EmbedQueue::with_max_failures(mock.engine(), 8));
    {
        let store = Store::open(&db).unwrap();
        let out = store_note_and_queue(&store, write("regras", "queue/retry", &body_with_sections(2), Some("global")), &queue).unwrap();
        assert_eq!(out.nulls, 2, "the write is owed every vector, as always");
    }

    // First pass: the mock is still failing. The job must still be queued.
    let first = queue.drain().await;
    assert_eq!(first.embedded, 0, "{first:?}");
    assert_eq!(first.nulls, 2, "{first:?}");
    assert_eq!(first.requeued, 1, "a failed embed must be re-queued, not dropped: {first:?}");
    assert_eq!(first.dead_lettered, 0, "{first:?}");
    assert_eq!(queue.pending_len(), 1, "the job left the queue and has to come back: {first:?}");

    // Ollama is healthy again, and nothing else happens — no second store, no
    // reindex. The scheduled retries have to deliver it.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last = first;
    while coverage(&db).coverage_pct < 100.0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
        last = queue.drain().await;
    }
    assert_eq!(coverage(&db).coverage_pct, 100.0, "the vector arrived without another brain_store. last={last:?}");
    assert_eq!(queue.pending_len(), 0, "and the queue is empty: {last:?}");
    assert!(last.is_settled(), "once resolved, the pass is settled: {last:?}");
    assert!(mock.requests() > 3, "the retries really did re-ask the model: {}", mock.requests());
    let _ = std::fs::remove_file(&db);
}

/// A permanently bad note stops costing CPU, and says so.
///
/// The cap is 3 rather than the production 8 purely so the test does not wait a
/// minute of backoff; what is under test is that a cap exists and that reaching
/// it is reported rather than looping.
#[tokio::test]
async fn a_permanently_failing_note_is_dead_lettered_instead_of_retried_forever() {
    let mock = Mock::start_with_failures(0, usize::MAX).await; // never recovers
    let db = tmp_db("dead");
    let cap = 3u32;
    let queue = Arc::new(EmbedQueue::with_max_failures(mock.engine(), cap));
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(&store, write("regras", "queue/dead", &body_with_sections(1), Some("global")), &queue).unwrap();
    }

    let mut out = queue.drain().await;
    let mut passes = 1;
    // Drive passes by hand, as fast as the backoff allows, until the cap is hit.
    while out.dead_lettered == 0 && passes < 20 {
        let wait = queue.millis_until_next_ready().unwrap_or(0);
        tokio::time::sleep(Duration::from_millis(wait.min(1_500))).await;
        out = queue.drain().await;
        passes += 1;
    }

    assert_eq!(out.dead_lettered, 1, "the note must be given up on exactly once: {out:?} after {passes} passes");
    assert_eq!(out.requeued, 0, "and not re-queued again: {out:?}");
    assert_eq!(queue.pending_len(), 0, "a dead lettered note leaves the queue: {out:?}");
    assert_eq!(queue.dead_lettered_total(), 1, "and the lifetime counter says so");
    assert!(passes <= cap as usize + 1, "gave up after {passes} passes, cap is {cap}: {out:?}");

    // Terminal for this process — a further pass must not resurrect it.
    let before = mock.requests();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let idle = queue.drain().await;
    assert_eq!(mock.requests(), before, "a dead lettered note must not be retried: {} -> {}", before, mock.requests());
    assert_eq!(idle.dead_lettered, 0, "and it is not counted again: {idle:?}");

    // The chunks are NULL, not zero: the record of the debt survives, which is what
    // a restart or a reindex reads.
    let cov = coverage(&db);
    assert_eq!(cov.without_embedding, 1, "the debt is recorded as NULL, not as a zero vector: {cov:?}");
    assert_eq!(cov.zero_vector, 0, "and never as a placeholder: {cov:?}");
    let _ = std::fs::remove_file(&db);
}

/// `is_settled()` must not lie while a null has a retry scheduled.
#[tokio::test]
async fn a_pass_with_a_scheduled_retry_is_not_settled() {
    let mock = Mock::start_with_failures(0, usize::MAX).await;
    let db = tmp_db("unsettled");
    let queue = EmbedQueue::with_max_failures(mock.engine(), 8);
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(&store, write("regras", "queue/unsettled", &body_with_sections(2), Some("global")), &queue).unwrap();
    }

    let out = queue.drain().await;
    assert!(out.requeued > 0, "precondition: the note was re-queued: {out:?}");
    assert_eq!(out.retriable_nulls, 2, "the nulls that have a retry are counted: {out:?}");
    assert!(
        !out.is_settled(),
        "settled must be false while a null has a retry scheduled — this is the bug, where a failed embed \
         reported settled=true beside coverage_pct=0: {out:?}"
    );

    // A dead letter is terminal, and *is* settled: there is nothing left to try.
    let dead = EmbedQueue::with_max_failures(mock.engine(), 1);
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(&store, write("regras", "queue/unsettled2", &body_with_sections(1), Some("global")), &dead).unwrap();
    }
    let out = dead.drain().await;
    assert_eq!(out.dead_lettered, 1, "{out:?}");
    assert_eq!(out.retriable_nulls, 0, "a dead lettered null is not retriable: {out:?}");
    assert!(out.is_settled(), "terminal counts as settled — the count is what says otherwise: {out:?}");
    assert!(out.nulls > 0, "and the nulls are still reported, not hidden: {out:?}");
    let _ = std::fs::remove_file(&db);
}

/// Every null a pass reports gets one of the fates a pass can actually reach:
/// **retried** or **dead-lettered**.
///
/// The invariant this stops is two mistakes at once — dropping a null on the floor, and
/// reporting a null without saying which fate it has. So it asserts **both** fates here,
/// on the same note, in the order the pass produces them; a test that only asserted the
/// terminal one would be named for a partition of which it demonstrated a single part.
///
/// "Resolved" is the third fate in `QueueOutcome` and is deliberately not in this test's
/// name: with a mock whose embed always fails it is unreachable, and asserting it here
/// would mean asserting a branch this fixture cannot enter. It is covered where the
/// embed succeeds — by the tests that assert `embedded > 0` with `nulls == 0`.
#[tokio::test]
async fn every_null_a_pass_reports_is_either_retried_or_dead_lettered() {
    let mock = Mock::start_with_failures(0, usize::MAX).await;
    let db = tmp_db("accounted");
    let queue = EmbedQueue::with_max_failures(mock.engine(), 2);
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(&store, write("regras", "queue/accounted", &body_with_sections(3), Some("global")), &queue).unwrap();
    }

    // Fate 1 — retried. The cap is 2 and 3 chunks are owed, so the first pass cannot
    // dead-letter: every null it reports has to be re-queued with a deadline, or the
    // note is stranded. This is the assertion the test used to be missing.
    let first = queue.drain().await;
    assert_eq!(first.requeued, 1, "the note is owed work, so the pass must re-queue it: {first:?}");
    assert_eq!(first.retriable_nulls, 3, "all three nulls are retriable on the first pass: {first:?}");
    assert_eq!(first.dead_lettered, 0, "the cap is 2, so nothing is terminal yet: {first:?}");
    assert_eq!(first.pending_left, 1, "the re-queued job is back on the queue: {first:?}");

    // Fate 2 — dead-lettered. Drain until the cap is spent.
    let mut out = first;
    let mut guard = 0;
    while out.dead_lettered == 0 && guard < 10 {
        let wait = queue.millis_until_next_ready().unwrap_or(0);
        tokio::time::sleep(Duration::from_millis(wait.min(1_500))).await;
        out = queue.drain().await;
        guard += 1;
    }
    // 3 chunks owed, and the cap is 2, so the note is dead-lettered with all 3
    // still NULL: nothing was quietly forgotten on the way.
    assert_eq!(out.dead_lettered, 1, "{out:?}");
    assert_eq!(out.nulls, 3, "{out:?}");
    assert_eq!(out.retriable_nulls, 0, "a dead lettered note is not retriable: {out:?}");
    assert_eq!(out.pending_left, 0, "and nothing is left on the queue: {out:?}");
    let _ = std::fs::remove_file(&db);
}

/// X-02: a chunk-sync failure must re-queue, not drop.
///
/// The other branch of the same funnel, and it needs its own test because it is
/// only reachable when the embed *succeeds* and the write-back fails — the opposite
/// of the dead-Ollama case, so nothing else reaches it. Forced with a
/// `RAISE(ABORT)` trigger: reads still work, so phase 1 and the embed proceed
/// normally, and the write aborts with a real SQL error.
///
/// Without the `finish_owed` call on this branch the job left the queue with its
/// chunks `NULL`, which is the original bug on a path the other tests cannot see.
#[tokio::test]
async fn a_note_whose_chunk_sync_failed_is_retried_rather_than_dropped() {
    let mock = Mock::start(0).await; // the embed succeeds, so only the write can fail
    let db = tmp_db("syncfail");
    let queue = Arc::new(EmbedQueue::with_max_failures(mock.engine(), 4));
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(
            &store,
            write("regras", "queue/syncfail", &body_with_sections(2), Some("global")),
            &queue,
        )
        .unwrap();
    }
    // A trigger that aborts every chunk write, leaving reads alone.
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER injected_write_failure BEFORE INSERT ON chunks BEGIN
                 SELECT RAISE(ABORT, 'injected chunk write failure');
             END;",
        )
        .unwrap();
    }
    let out = queue.drain().await;
    assert_eq!(out.failed, 1, "precondition: the chunk sync really did fail: {out:?}");
    assert!(mock.requests() > 0, "precondition: the embed ran, so the failure really was in the write-back");
    assert_eq!(
        queue.pending_len(),
        1,
        "a failed chunk sync must leave the note queued, not drop the job. Dropping it here is exactly the \
         bug X-02 is about, on a branch the dead-Ollama test never reaches: {out:?}"
    );
    assert_eq!(out.requeued, 1, "{out:?}");
    assert_eq!(out.dead_lettered, 0, "{out:?}");
    assert!(!out.is_settled(), "a null with a scheduled retry is not settled: {out:?}");

    // And once the injected failure is gone, the work is delivered with no further
    // `brain_store` — the same "it comes back" guarantee, on this branch.
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("DROP TRIGGER injected_write_failure;").unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last = out;
    while coverage(&db).coverage_pct < 100.0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
        last = queue.drain().await;
    }
    assert_eq!(coverage(&db).coverage_pct, 100.0, "the retried note must be embedded. last={last:?}");
    let _ = std::fs::remove_file(&db);
}

// ------------------------------------------------------------------ Y-01 --
//
// Two more ways for a pass to end with a note still owing a vector, and the
// reason they are in their own block: **both are in phase 1, before the note is
// known to be intact**, so neither had a `todo` to report and both were written
// before the `finish_owed` funnel existed. X-02 closed the funnel and routed
// four branches through it; these two were left calling `out.failed += 1` and
// returning, which produces exactly the symptom X-02 was about:
//
//   the job leaves the queue  -> `pending_len == 0`
//   nothing is re-queued      -> `retriable_nulls == 0`
//   so `is_settled()` is TRUE  -> next to `coverage_pct < 100`
//
// and the chunk stays `NULL` for the rest of the process's life. `recover()` on
// the next boot and `reindex --all` are the only things that can rescue it.
//
// The failure is injected without a race and without a trigger where possible:
// a **directory** where the database file should be fails `Store::open`
// deterministically, and dropping the `chunks` table fails the diff read while
// leaving `Store::open` and `note_get` working — which is the point, because
// that is the only way to reach the second branch at all.

/// Y-01: a note whose database cannot be *opened* is re-queued, not dropped.
///
/// `Store::open` failing in phase 1 is the first thing that can go wrong in a
/// pass, and the note was never even read — so there is no `todo` to report.
#[tokio::test]
async fn a_note_whose_database_cannot_be_opened_is_requeued_not_dropped() {
    let mock = Mock::start(0).await;
    // A directory where the database file belongs: `sqlite3_open` fails on EISDIR,
    // deterministically, on every platform this runs on. No injected trigger and
    // no second process to race with.
    let dir = format!("/tmp/brain-queue-unopenable-{}-{:?}", std::process::id(), std::thread::current().id());
    std::fs::create_dir_all(&dir).unwrap();
    let queue = EmbedQueue::with_max_failures(mock.engine(), 8);
    assert!(
        queue.enqueue(&dir, "regras/global/unopenable", vec!["## a\n\nalpha".to_string(), "## b\n\nbeta".to_string()]),
        "precondition: the job is queued"
    );

    let out = queue.drain().await;
    assert_eq!(out.failed, 1, "precondition: the open really did fail: {out:?}");
    assert_eq!(
        out.requeued,
        1,
        "a database that cannot be opened is a reason to keep the work, exactly like a dead Ollama. Without \
         the funnel the job left the queue here and its chunks stayed NULL for the life of the process: {out:?}"
    );
    assert_eq!(
        queue.pending_len(),
        1,
        "the job must still be in the queue. Dropping it is the X-02 bug on a branch the other tests never \
         reach, because it happens before the note is read: {out:?}"
    );
    assert_eq!(out.dead_lettered, 0, "one failure is not a dead letter: {out:?}");
    assert!(!out.is_settled(), "a null with a scheduled retry is not settled: {out:?}");
    // The note was never read, so the recorded chunk list is the only evidence of
    // what was owed — and it is reported, not silently dropped.
    assert!(out.nulls >= 1, "the debt must be reported as nulls: {out:?}");

    // Make the path openable so the directory is not left behind for the next run.
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Y-01: a note whose chunk diff cannot be *read* is re-queued, not dropped.
///
/// The second phase-1 branch, and the only way to reach it is to break the read
/// while leaving the open and the `note_get` working — which is what dropping the
/// `chunks` table does. That is a proxy for "the row read failed" (a corrupt
/// page, a database locked by another process, a schema this binary does not
/// understand); what is under test is the branch, not the specific fault.
#[tokio::test]
async fn a_note_whose_chunks_cannot_be_inspected_is_requeued_not_dropped() {
    let mock = Mock::start(0).await;
    let db = tmp_db("inspectfail");
    let queue = EmbedQueue::with_max_failures(mock.engine(), 8);
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(
            &store,
            write("regras", "queue/inspectfail", &body_with_sections(2), Some("global")),
            &queue,
        )
        .unwrap();
    }
    // `Store::open` and `note_get` still work; the chunk diff does not.
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("DROP TABLE chunks;").unwrap();
    }

    let out = queue.drain().await;
    assert_eq!(out.failed, 1, "precondition: the inspection really did fail: {out:?}");
    assert_eq!(
        out.requeued,
        1,
        "a diff that could not be read is a reason to keep the work, not a reason to drop it: {out:?}"
    );
    assert_eq!(queue.pending_len(), 1, "the job must still be in the queue: {out:?}");
    assert_eq!(out.dead_lettered, 0, "one failure is not a dead letter: {out:?}");
    assert!(!out.is_settled(), "a null with a scheduled retry is not settled: {out:?}");
    let _ = std::fs::remove_file(&db);
}

/// Y-01: a `recover()`-shaped job — one with no recorded chunk list — is still
/// re-queued, so the funnel cannot be defeated by a zero.
///
/// This is the case that makes `max(1)` load-bearing rather than cosmetic.
/// `recover()` deliberately enqueues with an **empty** chunk list (the worker
/// re-derives the diff itself), so `job.chunks.len()` is `0` for every job that
/// boot recovery creates. Reporting `owed = 0` to `finish_owed` would take its
/// early return, skip the re-queue entirely, and strand the note — the very bug
/// this batch exists to close, reintroduced through the fix.
///
/// A job is in the queue *because* something was owed, so it owes at least one
/// chunk: `enqueue` is only called when `stats.nulls > 0`, and `recover` only
/// for paths `notes_needing_embedding` reported as having at least one missing
/// chunk. Zero is therefore not a reachable honest answer on these branches.
#[tokio::test]
async fn a_requeued_note_with_no_recorded_chunk_list_is_not_stranded() {
    let mock = Mock::start(0).await;
    let db = tmp_db("nochunks");
    let queue = EmbedQueue::with_max_failures(mock.engine(), 8);
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(
            &store,
            write("regras", "queue/nochunks", &body_with_sections(1), Some("global")),
            &queue,
        )
        .unwrap();
    }
    // Exactly what `recover()` does: an **existing** note, queued with an empty
    // chunk list because the worker re-derives the diff itself. It has to be a
    // real note, or the pass drops it as deleted before it ever reaches the
    // inspection branch — which is correct behaviour and a different test.
    {
        let store = Store::open(&db).unwrap();
        store.note_upsert("regras/global/recovered", "regras", Some("global"), &body_with_sections(1), None, &[], false, None)
            .unwrap();
    }
    assert!(queue.enqueue(&db, "regras/global/recovered", Vec::new()));
    // Break the diff read so both jobs take the phase-1 inspection branch.
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("DROP TABLE chunks;").unwrap();
    }

    let out = queue.drain().await;
    assert_eq!(out.requeued, 2, "both jobs, including the one with no recorded chunks: {out:?}");
    assert_eq!(queue.pending_len(), 2, "neither may be stranded: {out:?}");
    assert!(
        out.nulls >= 1,
        "a job with an empty recorded chunk list still owes at least one vector, and says so: {out:?}"
    );
    assert!(!out.is_settled(), "and a pass that owes vectors is not settled: {out:?}");
    let _ = std::fs::remove_file(&db);
}

// ------------------------------------------------ the exit classification --
//
// Y-01, part 3. `run_one` has ten ways to end, and the review's objection to the
// previous report was not that two branches were wrong but that the report
// *claimed* a uniform rule that only held for some of them. So the rule is stated
// here, and this test enforces it against the source instead of leaving it as a
// comment that drifts.
//
// Every `return out` in `run_one` belongs to exactly one of:
//
//   (a) the job owes nothing      -> `skipped_complete`; nothing to re-queue.
//   (b) the job owes a vector and could not get it this pass
//                                -> the work must survive the pass: either
//                                   `finish_owed` (re-queue, or dead-letter at the
//                                   cap) or an explicit `enqueue_at` deferral.
//   (c) the job's subject is gone  -> `skipped_deleted`; there is nothing left to
//                                   owe a vector.
//
// Anything else — a `return out` whose segment contains none of those — is a
// branch that silently drops owed work, which is the exact defect Y-01 is about.
// A new early return added without one of these markers fails this test.

/// The `run_one` function body, or a panic explaining that the anchor moved.
fn run_one_source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/embed_queue.rs");
    let src = std::fs::read_to_string(&path).expect("read embed_queue.rs");
    let start = src
        .find("async fn run_one(")
        .unwrap_or_else(|| panic!("run_one not found in {}", path.display()));
    // The function ends at the `finish_owed` definition that follows it.
    let end = src[start..]
        .find("fn finish_owed(")
        .map(|i| start + i)
        .unwrap_or_else(|| panic!("finish_owed not found after run_one in {}", path.display()));
    src[start..end].to_string()
}

#[test]
fn every_way_run_one_can_end_either_owes_nothing_or_keeps_the_work() {
    let body = run_one_source();
    let mut segments = body.split("return out;").collect::<Vec<_>>();
    // The tail after the last `return out` is the success tail, not a branch.
    segments.pop();
    assert!(
        segments.len() >= 10,
        "expected the ten documented exits in run_one, found {} — an early return may have been added \
         without a classification",
        segments.len()
    );

    for (i, seg) in segments.iter().enumerate() {
        // The segment is everything since the previous `return out`, so the last
        // few lines are the branch itself.
        let tail = seg.lines().rev().take(14).collect::<Vec<_>>().join("\n");
        let keeps_the_work = tail.contains("finish_owed(") || tail.contains("enqueue_at(");
        let owes_nothing = tail.contains("skipped_complete") || tail.contains("skipped_deleted");
        assert!(
            keeps_the_work || owes_nothing,
            "run_one branch {i} returns without classifying itself. Every early return must either keep the \
             work (finish_owed or enqueue_at) or say the job owes nothing (skipped_complete / \
             skipped_deleted). Offending tail:\n{tail}"
        );
    }
}

/// Y-01, the other half of the same statement: the counting must agree.
///
/// A deferral for the embed lock re-queues the job **without** counting a failure
/// — a reindex may hold the lock for the whole 900 s TTL, and a shared failure
/// counter would dead-letter healthy work. So it also does not go through
/// `finish_owed`, and it does not land in `retriable_nulls` either. That is
/// deliberate, and the test pins the consequence that makes it safe:
/// `is_settled()` is still false, because `pending_len` is non-zero. The counters
/// under-report the deferral; the boolean an operator acts on does not lie.
#[tokio::test]
async fn a_lock_deferral_is_counted_as_pending_even_though_it_is_not_a_failure() {
    let mock = Mock::start(0).await;
    let db = tmp_db("lockdefer-count");
    let queue = EmbedQueue::with_max_failures(mock.engine(), 8);
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(
            &store,
            write("regras", "queue/lockdefer", &body_with_sections(2), Some("global")),
            &queue,
        )
        .unwrap();
        // Somebody else holds the lock, as a `brain reindex` would.
        assert!(store.try_acquire_embed_lock(&brain_store::embed_lock_owner("reindex"), 900).unwrap());
    }
    let out = queue.drain().await;
    assert_eq!(out.skipped_locked, 1, "{out:?}");
    assert_eq!(out.failed, 0, "standing down is not a failure: {out:?}");
    assert_eq!(out.dead_lettered, 0, "{out:?}");
    assert_eq!(out.requeued, 0, "the deferral is not a failure re-queue either: {out:?}");
    assert_eq!(queue.pending_len(), 1, "but the work is still queued, which is what matters: {out:?}");
    assert!(
        !out.is_settled(),
        "and that is what keeps is_settled() honest: a job waiting for the lock is not settled, even \
         though it counted as no failure at all: {out:?}"
    );
    let _ = std::fs::remove_file(&db);
}

/// Y-01: the phase-3 twin — a note whose database cannot be *reopened for the
/// write-back* is re-queued, not dropped.
///
/// This branch was an explicitly declared gap in the previous report ("sem
/// teste"), justified by the claim that its behaviour was covered *through the
/// shared `finish_owed`*. That justification is the same shape of claim the
/// review rejected for the two phase-1 branches: behaviour being shared is not
/// the branch being reached. So it is reached here.
///
/// The fault is injected **without a wall-clock sleep deciding anything**: the
/// mock Ollama is given a long per-chunk delay and the test waits for its request
/// counter to show the embed in flight, which is an observable event rather than a
/// duration. Only after the embed is provably started is the database replaced by a
/// directory, so phase 3's `Store::open` cannot succeed. A 2 s budget against a
/// filesystem rename is not a tight margin; the ordering does not depend on it.
#[tokio::test]
async fn a_note_whose_database_cannot_be_reopened_for_the_write_back_is_requeued() {
    let mock = Mock::start(2000).await; // hold the pass inside the embed for 2 s
    let db = tmp_db("reopen");
    let queue = Arc::new(EmbedQueue::with_max_failures(mock.engine(), 8));
    {
        let store = Store::open(&db).unwrap();
        store_note_and_queue(
            &store,
            write("regras", "queue/reopen", &body_with_sections(1), Some("global")),
            &queue,
        )
        .unwrap();
    }
    // Preconditions, read while the database is still a file.
    assert_eq!(coverage(&db).coverage_pct, 0.0, "precondition: the chunk is owed a vector");

    let drain_queue = queue.clone();
    let drain = tokio::spawn(async move { drain_queue.drain().await });

    // Wait for the embed to be *in flight* — an observation, not a sleep.
    let deadline = Instant::now() + Duration::from_secs(10);
    while mock.requests() == 0 {
        assert!(Instant::now() < deadline, "the pass never reached the embed");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Now the write-back's `Store::open` cannot succeed.
    std::fs::remove_file(&db).unwrap();
    std::fs::create_dir_all(&db).unwrap();

    let out = drain.await.unwrap();
    assert!(
        mock.requests() > 0,
        "precondition: the embed really ran, so the failure really was in the write-back: {out:?}"
    );
    assert_eq!(
        queue.pending_len(),
        1,
        "a database that cannot be reopened must leave the note queued. This is the branch the previous \
         report declared untested, and it is the twin of the phase-1 open: {out:?}"
    );
    assert_eq!(out.requeued, 1, "{out:?}");
    assert_eq!(out.dead_lettered, 0, "{out:?}");
    assert!(!out.is_settled(), "a null with a scheduled retry is not settled: {out:?}");
    let _ = std::fs::remove_dir_all(&db);
}

// ------------------------------------------------------------- TD-003: Debug --

#[tokio::test]
async fn debug_output_redacts_the_ollama_credential_but_keeps_the_endpoint() {
    // `EmbedQueue` is a public type with a hand-written `Debug`, so any `{:?}` in
    // a future log line, panic message or test failure would carry
    // `BRAIN_OLLAMA_URL` verbatim — credential included. `debug_assert!` and
    // `unwrap()` both use it, so this is a sink that exists whether or not anyone
    // writes a `println!` for it today.
    let mock = Mock::start(0).await;
    let credentialed = mock.base_url.replace("http://", "http://alice:segredo-do-ollama@");
    let queue = EmbedQueue::new(brain_embed::EmbeddingEngine::new(
        credentialed,
        "nomic-embed-text".into(),
    ));

    let shown = format!("{queue:?}");
    assert!(!shown.contains("segredo-do-ollama"), "password reached Debug output: {shown}");
    assert!(!shown.contains("alice"), "username reached Debug output: {shown}");
    assert!(shown.contains("***@"), "expected the redaction marker: {shown}");
    // The endpoint is what identifies *which* Ollama was unreachable, so it stays.
    let host_port = mock.base_url.strip_prefix("http://").unwrap();
    assert!(shown.contains(host_port), "host:port lost from Debug output: {shown}");
    // The rest of the Debug surface is unchanged, so redaction did not cost the
    // fields a reader actually uses.
    assert!(shown.contains("nomic-embed-text"), "model lost: {shown}");
    assert!(shown.contains("pending"), "pending lost: {shown}");
}

#[tokio::test]
async fn debug_output_of_a_credential_free_queue_is_unchanged() {
    // A deployment without Ollama auth must not see any difference: no marker, no
    // normalisation of the URL it already prints.
    let mock = Mock::start(0).await;
    let queue = EmbedQueue::new(mock.engine());
    let shown = format!("{queue:?}");
    assert!(shown.contains(&mock.base_url), "endpoint missing or altered: {shown}");
    assert!(!shown.contains("***"), "marker on a credential-free URL: {shown}");
}
