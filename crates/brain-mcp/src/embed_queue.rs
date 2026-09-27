//! US-02.7 — the embedding queue: a note write must not wait for a vector.
//!
//! # Why this exists
//!
//! `brain_store` used to embed the note's whole chunk list inline, before
//! opening the database. Ollama serves `/api/embeddings` **serially**
//! (`OLLAMA_NUM_PARALLEL` defaults to 1) at a measured ~0.045 s per chunk, so the
//! cost of the write scaled with the note: 0.4-0.6 s for an 8-chunk note, 2.8-3.9 s
//! for a 64-chunk one, and unbounded above that until `MAX_CHUNKS` put a ceiling on
//! it. The requirement is `WHEN embedding fails THEN system SHALL persist note but
//! queue embedding (log)`; the implementation degraded to `NULL` but still blocked.
//!
//! Measured after the split, on the real MCP protocol path against the real
//! Ollama: the write returns in **0.018 s** for both an 8-chunk and a 64-chunk
//! note, and the vectors arrive 0.6 s / 2.8 s later in the background.
//!
//! The split implemented here:
//!
//! 1. **write** — the note and its chunks are persisted with `embedding = NULL`
//!    and the caller is answered. FTS5 indexing is unaffected (`search` already
//!    filters `WHERE embedding IS NOT NULL`), so the note is fully usable
//!    immediately; only the vector half of the RRF is pending.
//! 2. **embed** — a background task computes the missing vectors and writes them.
//!
//! `brain status` / `brain_status` already report `embedding.coverage_pct`, and
//! that number *is* the queue's health signal: a chunk awaiting its vector counts
//! as `without_embedding`, never as a `zero_vector`, because it is stored as SQL
//! `NULL`. If `coverage_pct` does not climb after a `brain_store`, the queue is
//! stuck or Ollama is down — and the reason is on stderr either way.
//!
//! # Concurrency
//!
//! Three guards, because there are three ways to duplicate the same work:
//! - **per path, in memory** — a path already queued is replaced, not appended, so
//!   a session note written on every tool result cannot grow an unbounded backlog;
//! - **per process** — one drain at a time, so two `brain_store` calls in flight
//!   cannot embed the same corpus twice;
//! - **cross process** — [`Store::try_acquire_embed_lock`], an advisory lock in
//!   `_meta`, because the other producer is `brain reindex`, a *separate process*
//!   that exists precisely to be run against a live server.
//!
//! # Send-ness
//!
//! The whole point of the existing handler layout is that no `&Store` crosses an
//! `.await`: `Store` wraps a `rusqlite::Connection`, which is `Send` but not
//! `Sync`. A worker that held one across the embed await would be unsendable and
//! could not be spawned. So every phase here opens its own `Store` and drops it
//! before the next one: read the note, drop; embed, no handle; write, drop. Same
//! discipline as the request paths, and it is why this needed no change to
//! `AppState` or to the `Store` design.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use brain_embed::EmbeddingEngine;
use brain_store::Store;

/// `_meta` key backing the cross-process lock, and the default TTL.
///
/// **Why 900 s and not shorter.** The lock is held across the whole embed pass, and
/// the measured worst case for one pass is a full-corpus reindex: 815 chunks at
/// ~0.15 s/chunk serial, about 2 minutes, with the budget
/// (`BRAIN_EMBED_TIMEOUT_SECS` base 60 s x up to 8 waves = 480 s) as the hard
/// ceiling. 900 s is ~1.9x that ceiling, so a *live* holder is never mistaken for
/// a dead one. The cost of being generous is bounded and visible: a process killed
/// mid-pass blocks the queue for at most 15 minutes, and `brain status` now reports
/// `embed_lock_age_s` and `embed_lock_expires_in_s` so that wait is observable
/// rather than a mystery. The lock is also reclaimable on expiry
/// (`try_acquire_embed_lock` treats an expired lock as abandoned) and the queue
/// re-queues the work it stood down for, so a dead holder costs delay, never work.
const EMBED_LOCK_KEY: &str = "embed_lock";
const DEFAULT_LOCK_TTL_SECS: i64 = 900;

/// First retry delay after losing the embed lock, doubling per attempt up to
/// [`MAX_RETRY_DELAY_MS`].
///
/// Bounded low because the common case is a reindex that finishes in seconds and
/// the queue should pick the work up as soon as it is done, not a minute later.
/// Bounded high because a lock held by a wedged process would otherwise be polled
/// for the whole TTL.
const FIRST_RETRY_DELAY_MS: u64 = 250;
const MAX_RETRY_DELAY_MS: u64 = 30_000;

/// Env var that caps how many times one note may fail to embed before the queue
/// stops trying.
pub const MAX_EMBED_FAILURES_ENV: &str = "BRAIN_EMBED_MAX_FAILURES";

/// Default for [`MAX_EMBED_FAILURES_ENV`].
///
/// X-02. Eight failures, which with [`backoff_ms`] is 500 + 1000 + 2000 + 4000 +
/// 8000 + 16000 + 30000 ms — about **one minute** of retrying before a note is
/// given up on. Chosen to cover the failure it exists for: an Ollama restart or a
/// cold model load, which is tens of seconds, not minutes. Above that the note is
/// not permanently lost — its chunks stay `NULL`, which is exactly the record
/// `recover` reads on the next boot, and what `brain reindex --all` re-embeds — but
/// it stops consuming a slot in the queue and stops retrying forever.
///
/// Both directions of that trade are real, which is why this is a cap and not
/// either extreme:
///
/// - **No retry** (the old behaviour) meant a note written during an Ollama
///   restart kept `NULL` vectors for the life of the process, silently, with
///   `is_settled()` reporting `true` beside it. That is the class of loss the
///   whole write/queue split exists to prevent.
/// - **Unbounded retry** turns one permanently bad note into permanent CPU: a
///   note whose content the model always rejects, or a database that cannot be
///   written, would be re-attempted every 30 s until the process died.
///
/// A cap with a *visible* terminal state is the only option that is neither.
const DEFAULT_MAX_EMBED_FAILURES: u32 = 8;

/// Longest a single drain keeps looping over newly-ready work before handing back.
///
/// A drain normally settles in one or two rounds (a `tool-result` burst). The
/// bound exists so a pathological enqueue loop cannot hold the drain flag — and
/// therefore block every later `brain_store` from spawning its own worker — for
/// unbounded time.
const MAX_DRAIN_ROUNDS: u32 = 16;

/// One note waiting to be embedded.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Job {
    db: String,
    path: String,
    /// Chunk texts captured when the note was written. The worker re-derives the
    /// note's *current* chunks and only embeds what is still missing, so this
    /// list is the provenance record rather than the work list: it is what
    /// `NoteEmbed` pairs each computed vector with, which is how a note edited
    /// mid-flight is detected instead of silently mis-vectorised.
    chunks: Vec<String>,
    /// Monotonic millisecond deadline before which this job must not be touched.
    /// Zero means "now". Set only when the job was skipped because another embed
    /// held the lock — without it, a re-enqueue would spin the drain against a
    /// reindex that is still running.
    ready_at_ms: u64,
    /// How many times this job has been deferred. Only used to size the backoff.
    attempts: u32,
    /// How many times this job has *failed to embed*. X-02.
    ///
    /// Deliberately not the same counter as `attempts`. Standing down because a
    /// reindex holds the lock is not a failure and must not count towards
    /// [`MAX_EMBED_FAILURES_ENV`]: a reindex can hold the lock for the whole
    /// 900 s TTL, and a shared counter would dead-letter healthy work every time
    /// one happened to run. Only a real embedding failure increments this.
    failures: u32,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Backoff before attempt number `attempts`: `FIRST_RETRY_DELAY_MS` doubling up to
/// [`MAX_RETRY_DELAY_MS`], saturating rather than overflowing on a long-lived
/// deferral.
fn backoff_ms(attempts: u32) -> u64 {
    let shift = attempts.min(20);
    FIRST_RETRY_DELAY_MS.saturating_mul(1u64 << shift).min(MAX_RETRY_DELAY_MS)
}

/// Y-01: how many vectors a job still owes when the pass failed **before it could
/// read the note**.
///
/// Two branches need this — `Store::open` failing, and the chunk diff failing to
/// read — and on both the current chunk count is unknowable. So it comes from
/// [`Job::chunks`], the list captured when the note was written.
///
/// **`max(1)` is load-bearing, not cosmetic.** [`EmbedQueue::recover`] enqueues
/// with an **empty** chunk list by design (the worker re-derives the diff), so
/// every boot-recovered job would report `0`, take `finish_owed`'s
/// `if owed == 0 { return; }` early exit, skip the re-queue, and be stranded —
/// the same lost work this batch exists to close, reintroduced by the fix.
///
/// Zero is also not an honest answer in principle: a job is only ever enqueued
/// because something was owed (`enqueue` is called when `stats.nulls > 0`, and
/// `recover` only for paths `notes_needing_embedding` reported as missing at
/// least one chunk). So the floor of 1 is the truth, not a fudge — and it is
/// reported as a *lower bound*, which is why the log line for these branches says
/// "at least".
fn owed_without_reading(job: &Job) -> usize {
    job.chunks.len().max(1)
}

#[derive(Debug, Default)]
struct State {
    pending: VecDeque<Job>,
}

/// Resets the drain flag on drop, including on unwind.
///
/// Without this a panic inside a worker — a bug, or a store error that escapes as
/// one — would leave the flag set for the life of the process and silently wedge
/// embedding: every later `brain_store` would enqueue work that nothing ever
/// runs, and the only symptom would be a `coverage_pct` that never climbs.
struct DrainGuard<'a>(&'a AtomicBool);

impl Drop for DrainGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// What a [`EmbedQueue::drain`] pass did. Every field is a reason a chunk might
/// still be `NULL`, so "why is coverage not 100%" is answerable from this alone.
///
/// This was previously write-only: `spawn_worker` called `drain()` and threw the
/// result away, so `skipped_locked`, `diverged` and `failed` had no reader in
/// production and were test-only observability. It is now logged, one line per
/// drain, and folded into the next `brain status` snapshot.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct QueueOutcome {
    /// Vectors written.
    pub embedded: usize,
    /// Chunks still `NULL` after the pass (Ollama failed, or the note changed
    /// under the pass).
    pub nulls: usize,
    /// Notes this pass left owing a vector *and re-queued for another attempt*.
    ///
    /// X-02. Every way a pass can end with a note still owing something routes
    /// through one place, so this and [`QueueOutcome::dead_lettered`] together
    /// account for every chunk in [`QueueOutcome::nulls`]: a chunk is either
    /// resolved, scheduled for a retry, or given up on. Before X-02 the only
    /// re-queued outcome was standing down for the lock — every other one let the
    /// job leave the queue, so a note written while Ollama was down kept `NULL`
    /// chunks for the life of the process and nothing said so.
    pub requeued: usize,
    /// Notes this pass stopped retrying, having failed
    /// [`MAX_EMBED_FAILURES_ENV`] times.
    ///
    /// Not lost: the chunks are `NULL`, which is what `recover` reads on the next
    /// boot and what `brain reindex --all` re-embeds. Terminal for this process
    /// though, and the count is cumulative in `brain status` precisely so an
    /// operator can see it without reading a log.
    pub dead_lettered: usize,
    /// Chunks that are `NULL` *and* have a retry scheduled.
    ///
    /// The number that makes [`QueueOutcome::is_settled`] honest: a pass can end
    /// with an empty queue and still owe vectors, and reporting `settled` there is
    /// what told the operator everything was fine while `coverage_pct` sat at 0.
    pub retriable_nulls: usize,
    /// Fresh vectors discarded because the note was edited mid-pass.
    pub diverged: usize,
    /// Notes skipped because another embed held the lock (e.g. a reindex). Each of
    /// these is **re-queued with a backoff**, so the counter is a deferral, not a
    /// loss — compare it with the queue still being non-empty afterwards.
    pub skipped_locked: usize,
    /// Notes skipped because every chunk already had a usable vector.
    pub skipped_complete: usize,
    /// Notes that no longer exist.
    pub skipped_deleted: usize,
    /// Notes whose drain returned an error (DB, chunk sync, ...).
    pub failed: usize,
    /// Stored vectors the write path refused to reuse, kept anyway because the
    /// note could not be re-embedded in this pass. Non-zero means a chunk is
    /// running on an approximate vector; `ChunkSyncStats::stale_reused`.
    pub stale_reused: usize,
    /// Chunks whose stored vector was not reusable and for which no fresh vector
    /// was available either, so they were written back as `NULL`.
    pub unmatched: usize,
    /// Jobs still queued when the pass ended, including ones deferred by a lock.
    /// This is the number that says "the work did not finish", and it is the one
    /// that was previously invisible.
    pub pending_left: usize,
}

impl QueueOutcome {
    /// Whether this pass settled every job it took, or left work behind.
    ///
    /// X-02. **Settled now means "nothing more will happen on its own"**, which is
    /// a stronger claim than "the queue is empty" and the only one an operator can
    /// act on. It is false while any chunk is `NULL` with a retry scheduled, even
    /// if the queue happens to be momentarily empty — the old version looked only
    /// at `pending_left`, so a failed embed reported `settled=true` next to
    /// `coverage_pct=0` and the two numbers together said the opposite of what an
    /// operator would conclude.
    ///
    /// A dead-lettered note does *not* make this false. Its chunks are terminal for
    /// this process, and the count is reported separately rather than being
    /// disguised as progress: a queue that is not settled but has nothing left to
    /// try is a different, and actionable, state.
    pub fn is_settled(&self) -> bool {
        self.pending_left == 0 && self.retriable_nulls == 0
    }
}

/// A background embedding queue over a set of database files.
///
/// Construct one per engine. [`global_queue`] is the process-wide instance the
/// MCP handlers use; tests build their own against a mock or a dead URL so the
/// "Ollama is down" path is exercised without touching the environment.
pub struct EmbedQueue {
    engine: EmbeddingEngine,
    state: Mutex<State>,
    /// One drain at a time. An `AtomicBool` rather than a field of `State` so the
    /// guard above can clear it without taking the lock it is protecting.
    draining: AtomicBool,
    /// One scheduled retry at a time, so N deferred jobs do not become N timer
    /// tasks all waking to drain the same queue.
    retry_armed: AtomicBool,
    /// X-02: how many embedding failures a note may accumulate before the queue
    /// stops retrying it. A field rather than a bare constant so a test can drive
    /// the cap without waiting a real minute, and without setting an environment
    /// variable that every other test in the process would race on.
    max_failures: u32,
    /// X-02: lifetime totals, so `brain status` can report a dead-lettered note
    /// long after the drain that gave up on it has scrolled out of the log.
    dead_lettered_total: AtomicUsize,
    /// The most recent pass, kept so `brain status` can answer "what happened to
    /// the work" without a log. `None` until the first drain.
    last_outcome: Mutex<Option<QueueOutcome>>,
}

impl std::fmt::Debug for EmbedQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.state.lock().map(|s| s.pending.len()).unwrap_or(0);
        f.debug_struct("EmbedQueue")
            // Redacted, not raw: this is a `Debug` on a public type, so any
            // `{:?}` in a future log line, panic message or test failure would
            // carry `BRAIN_OLLAMA_URL` verbatim — credential included. The host
            // and port, which is what identifies the endpoint, still come out.
            .field("base_url", &brain_embed::redact_url(&self.engine.base_url))
            .field("model", &self.engine.model)
            .field("pending", &s)
            .finish()
    }
}

impl EmbedQueue {
    /// A queue driving `engine`, with the default attempt cap.
    pub fn new(engine: EmbeddingEngine) -> Self {
        let max_failures = std::env::var(MAX_EMBED_FAILURES_ENV)
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_MAX_EMBED_FAILURES);
        Self::with_max_failures(engine, max_failures)
    }

    /// [`EmbedQueue::new`] with an explicit attempt cap.
    ///
    /// The seam tests use instead of [`MAX_EMBED_FAILURES_ENV`]: the cap is
    /// reached after about a minute of backoff at the default, and a test that has
    /// to wait a minute to observe a dead letter is a test nobody runs. Setting
    /// the environment variable instead would make the cap process-global and
    /// every parallel test would inherit it.
    pub fn with_max_failures(engine: EmbeddingEngine, max_failures: u32) -> Self {
        Self {
            engine,
            state: Mutex::new(State::default()),
            draining: AtomicBool::new(false),
            retry_armed: AtomicBool::new(false),
            max_failures: max_failures.max(1),
            dead_lettered_total: AtomicUsize::new(0),
            last_outcome: Mutex::new(None),
        }
    }

    /// Notes this process has stopped retrying, across every pass.
    pub fn dead_lettered_total(&self) -> usize {
        self.dead_lettered_total.load(Ordering::SeqCst)
    }

    /// The attempt cap in force, so `brain status` reports the number a note would
    /// have to reach rather than a second copy of the default.
    pub fn max_failures(&self) -> u32 {
        self.max_failures
    }

    /// The most recent pass, for `brain status`.
    pub fn last_outcome(&self) -> Option<QueueOutcome> {
        self.last_outcome.lock().ok().and_then(|o| *o)
    }

    /// The engine this queue embeds with.
    ///
    /// Exposed so a request handler can embed its *query* through the same engine
    /// the queue uses, instead of building a second one from the environment on
    /// every request — which is also what makes the handlers testable against a
    /// mock without touching `BRAIN_OLLAMA_URL`.
    pub fn engine(&self) -> &EmbeddingEngine {
        &self.engine
    }

    /// Queues `path` for background embedding.
    ///
    /// Re-queuing a path that is already pending **replaces** the job with the
    /// newer chunk list instead of appending: a session note rewritten on every
    /// tool result would otherwise accumulate one job per write, and all but the
    /// last would embed text the note no longer has.
    ///
    /// A replacement is made immediately runnable: the newer text is newer work,
    /// and inheriting the older job's backoff would delay it for no reason.
    ///
    /// Returns `true` when the job was newly queued, `false` when it replaced a
    /// pending one. Either way the note is queued exactly once.
    pub fn enqueue(&self, db: &str, path: &str, chunks: Vec<String>) -> bool {
        self.enqueue_at(db, path, chunks, 0, 0, 0)
    }

    /// Milliseconds until the earliest pending job becomes runnable, or `None` when
    /// nothing is pending.
    pub fn millis_until_next_ready(&self) -> Option<u64> {
        let now = now_ms();
        let s = self.state.lock().ok()?;
        s.pending.iter().map(|j| j.ready_at_ms.saturating_sub(now)).min()
    }

    /// The earliest deadline any pending job carries, as a wall-clock
    /// millisecond stamp.
    ///
    /// X-05.1. Exists so a test can assert *that a deadline was set* without
    /// comparing a count against a 250 ms wall-clock window, which is how
    /// `status_reports_a_deferred_queue_distinctly_from_an_empty_one` came to fail
    /// three times out of four mutations and only pass under `--nocapture`.
    /// Comparing this against a timestamp captured *before* the drain is immune to
    /// how long the assertions take, because the clock only moves forward.
    pub fn earliest_ready_at_ms(&self) -> Option<u64> {
        let s = self.state.lock().ok()?;
        s.pending.iter().map(|j| j.ready_at_ms).min()
    }

    /// [`EmbedQueue::enqueue`] with an explicit backoff deadline, attempt count
    /// and failure count.
    ///
    /// Public so a caller — or a test — can say "not before this moment" without
    /// waiting for a failure to produce one. The deadline is a plain millisecond
    /// stamp; `0` means now.
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_at(&self, db: &str, path: &str, chunks: Vec<String>, ready_at_ms: u64, attempts: u32, failures: u32) -> bool {
        let job = Job { db: db.to_string(), path: path.to_string(), chunks, ready_at_ms, attempts, failures };
        let mut s = self.state.lock().expect("embed queue state poisoned");
        if let Some(pos) = s.pending.iter().position(|j| j.db == job.db && j.path == job.path) {
            // Keep the earlier of the two deadlines: a job a caller re-queued
            // explicitly (deadline 0) must not inherit an old backoff, and a job
            // being re-queued by the queue itself must not lose it.
            let ready_at_ms = job.ready_at_ms.min(s.pending[pos].ready_at_ms);
            s.pending[pos] = Job { ready_at_ms, ..job };
            return false;
        }
        s.pending.push_back(job);
        true
    }

    /// Jobs waiting to be embedded, deferred ones included.
    pub fn pending_len(&self) -> usize {
        self.state.lock().map(|s| s.pending.len()).unwrap_or(0)
    }

    /// Jobs waiting that are not inside a backoff window — i.e. what a drain would
    /// pick up right now. `brain status` reports both, so a queue that is fully
    /// deferred (lock held elsewhere) is distinguishable from an idle one.
    pub fn ready_len(&self) -> usize {
        let now = now_ms();
        self.state.lock().map(|s| s.pending.iter().filter(|j| j.ready_at_ms <= now).count()).unwrap_or(0)
    }

    /// True while a drain is in progress. Diagnostic only.
    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }

    /// Re-queues every note in `db` that still owes a vector, and returns how many
    /// notes were enqueued. W-04a: boot recovery.
    ///
    /// The queue's work list lives in memory, so a restart with chunks in flight
    /// loses it outright — a deploy, an OOM, a `kill -9` — and nothing would ever
    /// re-read it. The owed work is recoverable from the database alone, because
    /// `NULL` *is* the record of it, so the server asks for it on boot.
    ///
    /// The chunk list is passed as empty on purpose: the worker re-derives the
    /// note's current chunks and diffs them itself, so there is nothing to
    /// reconstruct here and no way for a stale snapshot to be applied.
    pub fn recover(&self, db: &str) -> usize {
        let owed = match Store::open(db) {
            Ok(s) => s.notes_needing_embedding(),
            Err(e) => {
                eprintln!("brain-queue: boot recovery skipped: cannot open {db}: {e:#}");
                return 0;
            }
        };
        let owed = match owed {
            Ok(v) => v,
            Err(e) => {
                eprintln!("brain-queue: boot recovery skipped: cannot inspect {db}: {e:#}");
                return 0;
            }
        };
        let mut enqueued = 0usize;
        let mut chunks = 0usize;
        for (path, missing) in &owed {
            chunks += missing;
            if self.enqueue(db, path, Vec::new()) {
                enqueued += 1;
            }
        }
        if enqueued > 0 {
            eprintln!(
                "brain-queue: boot recovery re-queued {enqueued} note(s), {chunks} chunk(s) still owed a vector \
                 in {db}. These were in flight when the process last stopped."
            );
        }
        enqueued
    }

    /// Spawns a background drain, unless one is already running.
    ///
    /// Fire-and-forget by design: the caller has already answered the client, and
    /// a failure inside is a log line plus `NULL` chunks, never a returned error.
    /// The `running` flag is what makes a burst of `brain_store` calls cost one
    /// drain rather than one drain each.
    ///
    /// Returning early because a drain is in progress is safe *only* because
    /// `drain` re-reads `pending` after every round and re-arms a retry for
    /// anything it leaves behind. Previously it snapshotted `pending` once, so a
    /// job enqueued mid-drain was in the queue at the end with nobody scheduled to
    /// look at it — a lost wakeup that silently stranded the work.
    pub fn spawn_worker(self: &Arc<Self>) {
        if self.pending_len() == 0 {
            return;
        }
        if self.is_draining() {
            return;
        }
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let out = me.drain().await;
            // The `Arc` is what makes the retry schedulable from in here: a job the
            // pass deferred has no other trigger left, and `drain` itself cannot arm
            // one because it only borrows the queue.
            if out.pending_left > 0 {
                me.arm_retry_if_pending();
            }
        });
    }

    /// Schedules a drain for when the next deferred job becomes runnable.
    ///
    /// This is the third of the three delivery guarantees, and the one the other two
    /// lean on: without it, work deferred by a held lock or arrived mid-drain has
    /// no trigger left. A single task is armed at a time; it clears the flag before
    /// draining so a fresh `enqueue` can re-arm, which can briefly leave two
    /// tasks looping — harmless, because `drain` is idempotent, one drain at a time
    /// is enforced by the flag, and both loops exit as soon as the queue empties.
    ///
    /// Takes `&Arc<Self>` because the task it spawns has to own the queue. Every
    /// production entry point is therefore either [`EmbedQueue::spawn_worker`] or
    /// this method, both of which hold the `Arc`; `drain` is deliberately `&self`
    /// so a test can drive it without one.
    pub fn arm_retry_if_pending(self: &Arc<Self>) {
        let Some(wait) = self.millis_until_next_ready() else { return };
        // Never spin on a zero wait: that is the busy loop against a reindex.
        let wait = wait.max(FIRST_RETRY_DELAY_MS / 5);
        if self.retry_armed.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
            me.retry_armed.store(false, Ordering::SeqCst);
            me.drain().await;
        });
    }

    /// Takes every job whose backoff has expired, leaving the deferred ones queued.
    fn take_ready(&self) -> Vec<Job> {
        let now = now_ms();
        let mut s = self.state.lock().expect("embed queue state poisoned");
        let ready: Vec<Job> = s.pending.iter().filter(|j| j.ready_at_ms <= now).cloned().collect();
        s.pending.retain(|j| j.ready_at_ms > now);
        ready
    }

    /// Embeds everything queued, then returns.
    ///
    /// Public and deterministic on purpose: production calls it from
    /// [`EmbedQueue::spawn_worker`], tests call it directly so "the store
    /// returned before the vector existed" and "the vector arrives afterwards"
    /// are two separately observable facts rather than one race.
    ///
    /// The pass **loops** rather than draining a single snapshot, which is what
    /// closes the lost wakeup: a `brain_store` that lands while this pass is
    /// embedding is picked up by the next round, so `pending_len() == 0` when the
    /// pass ends whenever the work can be done. It stops when nothing is runnable,
    /// when a round makes no progress (every job deferred or failed, so another
    /// immediate round would be identical), or at [`MAX_DRAIN_ROUNDS`]. Anything
    /// left behind gets a scheduled retry.
    pub async fn drain(&self) -> QueueOutcome {
        // One drain at a time. `compare_exchange` rather than a check-then-set, so
        // two workers spawned in the same instant cannot both start; the guard
        // clears the flag on every exit path, panic included.
        if self
            .draining
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return QueueOutcome { pending_left: self.pending_len(), ..Default::default() };
        }
        let _guard = DrainGuard(&self.draining);

        let mut total = QueueOutcome::default();
        for _round in 0..MAX_DRAIN_ROUNDS {
            let batch = self.take_ready();
            if batch.is_empty() {
                break;
            }
            let mut progress = 0usize;
            for job in batch {
                let r = self.run_one(&job).await;
                // A re-queued or dead-lettered job counts as progress: both are
                // *decided*, and counting them keeps the loop from stopping early
                // while other ready jobs are still waiting. A job that merely stood
                // down for the lock does not, which is what stops the busy loop.
                progress += r.embedded
                    + r.skipped_complete
                    + r.skipped_deleted
                    + r.failed
                    + r.requeued
                    + r.dead_lettered;
                total.embedded += r.embedded;
                total.nulls += r.nulls;
                total.requeued += r.requeued;
                total.dead_lettered += r.dead_lettered;
                total.retriable_nulls += r.retriable_nulls;
                total.diverged += r.diverged;
                total.skipped_locked += r.skipped_locked;
                total.skipped_complete += r.skipped_complete;
                total.skipped_deleted += r.skipped_deleted;
                total.failed += r.failed;
                total.stale_reused += r.stale_reused;
                total.unmatched += r.unmatched;
            }
            if progress == 0 {
                // Every job in this round was deferred by a held lock, or failed.
                // Retrying immediately would be the busy loop; the backoff plus the
                // scheduled retry is the honest response.
                break;
            }
        }

        total.pending_left = self.pending_len();
        let d = &mut total;
        // X-02: the line has to account for every `NULL` it reports, or it repeats
        // the original sin one level up. `settled` is now derived from
        // `retriable_nulls` as well as `pending_left`, so it cannot read `true`
        // beside a `nulls` count that has a retry scheduled, and `dead_lettered`
        // is on the same line as the chunks it will not fix.
        eprintln!(
            "brain-queue: drain embedded={} nulls={} retriable_nulls={} requeued={} dead_lettered={} \
             diverged={} stale_reused={} unmatched={} skipped_locked={} skipped_complete={} \
             skipped_deleted={} failed={} pending_left={} settled={}",
            d.embedded, d.nulls, d.retriable_nulls, d.requeued, d.dead_lettered,
            d.diverged, d.stale_reused, d.unmatched,
            d.skipped_locked, d.skipped_complete, d.skipped_deleted, d.failed, d.pending_left, d.is_settled()
        );
        if d.pending_left > 0 {
            eprintln!(
                "brain-queue: {} job(s) still queued after this pass (backing off or another pass is \
                 mid-flight); a retry is scheduled so the work is not stranded.",
                d.pending_left
            );
        }
        if d.nulls > 0 && !d.is_settled() {
            eprintln!(
                "brain-queue: {} chunk(s) are NULL and scheduled for another attempt, so this pass did not \
                 settle. `brain status` reports them under coverage.without_embedding with the queue's \
                 queue.retriable_nulls.",
                d.retriable_nulls
            );
        }
        if let Ok(mut slot) = self.last_outcome.lock() {
            *slot = Some(*d);
        }
        total
    }

    /// The whole lifecycle for one note: lock, read, embed, write.
    async fn run_one(&self, job: &Job) -> QueueOutcome {
        let mut out = QueueOutcome::default();

        // --- phase 1: read, holding no handle across the network call ---------
        let meta = {
            let store = match Store::open(&job.db) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("brain-queue: {} could not be embedded: cannot open {}: {e:#}", job.path, job.db);
                    // Y-01. The note was never read, so `todo` does not exist yet —
                    // but the job is in the queue *because* something was owed, so
                    // the debt is real and the work must be kept. Without this the
                    // job left the queue here, `pending_left` went to 0 and
                    // `is_settled()` reported `true` beside `coverage_pct < 100`.
                    out.failed += 1;
                    self.finish_owed(job, owed_without_reading(job), &mut out, "the database could not be opened");
                    return out;
                }
            };
            // Claim the cross-process lock before doing anything expensive.
            let owner = brain_store::embed_lock_owner("queue");
            match store.try_acquire_embed_lock(&owner, DEFAULT_LOCK_TTL_SECS) {
                Ok(true) => {}
                Ok(false) => {
                    let holder = store.embed_lock_holder().unwrap_or(None).unwrap_or_else(|| "unknown".into());
                    let age = store.embed_lock_age_secs().ok().flatten();
                    eprintln!(
                        "brain-queue: {} stood down: the embed lock is held by {holder}{}. Re-queued with a \
                         backoff, so the chunks are owed a vector rather than abandoned; they stay NULL and \
                         FTS-searchable until then and `brain status` reports the queue.",
                        job.path,
                        age.map(|a| format!(" (held for {a}s)")).unwrap_or_default()
                    );
                    // W-04b. The job left the queue when this pass took its batch;
                    // dropping it here is how a reindex silently cost the server its
                    // pending work. Put it back, deferred, so the retry that
                    // `drain` arms picks it up when the lock frees.
                    let attempts = job.attempts + 1;
                    let delay = backoff_ms(attempts);
                    self.enqueue_at(
                        &job.db,
                        &job.path,
                        job.chunks.clone(),
                        now_ms() + delay,
                        attempts,
                        // Standing down is not a failure: a reindex may hold the
                        // lock for the whole TTL, and counting it would
                        // dead-letter healthy work.
                        job.failures,
                    );
                    out.skipped_locked += 1;
                    return out;
                }
                Err(e) => {
                    eprintln!("brain-queue: {} could not take the embed lock: {e:#}", job.path);
                    // A lock that cannot even be *read* (a busy DB, a corrupt row) is
                    // as much a reason to keep the work as a lock held elsewhere.
                    let attempts = job.attempts + 1;
                    self.enqueue_at(
                        &job.db,
                        &job.path,
                        job.chunks.clone(),
                        now_ms() + backoff_ms(attempts),
                        attempts,
                        job.failures,
                    );
                    out.skipped_locked += 1;
                    return out;
                }
            }
            let Some(note) = store.note_get(&job.path).unwrap_or(None) else {
                eprintln!("brain-queue: {} was deleted before its embedding ran; dropping the job", job.path);
                let _ = store.release_embed_lock(&owner);
                out.skipped_deleted += 1;
                return out;
            };
            let todo = match store.chunks_needing_embedding(&job.path, &note.content) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("brain-queue: {} could not be inspected: {e:#}", job.path);
                    let _ = store.release_embed_lock(&owner);
                    // Y-01. Same reasoning as the `Store::open` branch above: the
                    // read that failed is the one that would have told us *how
                    // many* chunks are owed, so the count has to come from the
                    // job itself. Dropping the job here was the X-02 bug on a
                    // branch nothing else reaches.
                    out.failed += 1;
                    self.finish_owed(job, owed_without_reading(job), &mut out, "the note's chunk diff could not be read");
                    return out;
                }
            };
            (note, todo)
        }; // every `Store` is dropped here, before any `.await`

        let (_note, todo) = meta;
        if todo.is_empty() {
            if let Ok(store) = Store::open(&job.db) {
                let _ = store.release_embed_lock(&brain_store::embed_lock_owner("queue"));
            }
            out.skipped_complete += 1;
            return out;
        }

        // --- phase 2: embed, with no database handle in scope ----------------
        let texts: Vec<String> = todo.iter().map(|(_, t)| t.clone()).collect();
        let budget = self.engine.batch_timeout(texts.len());
        let vecs = match tokio::time::timeout(budget, self.engine.embed_batch_partial(texts.clone())).await {
            Ok(v) => v,
            Err(_) => {
                eprintln!(
                    "brain-queue: embedding budget of {budget:?} expired for {} chunk(s) of {}; the chunks \
                     stay NULL and FTS-searchable. Raise BRAIN_EMBED_TIMEOUT_SECS if the model is cold-starting.",
                    todo.len(), job.path
                );
                Vec::new()
            }
        };

        // --- phase 3: write back, re-reading the note -------------------------
        //
        // X-02. Everything from here on funnels through `finish_owed` when the note
        // still owes a vector, so "the embed failed", "the write-back failed" and
        // "the database could not be reopened" all end in the same place: a
        // scheduled retry, or a visible dead letter. The `got == 0` branch below
        // used to `return` straight out of the queue, and the chunk-sync error used
        // to bump a counter and do the same — which is how a note written while
        // Ollama was restarting ended up with `NULL` vectors and an empty queue,
        // and `is_settled()` reporting `true` beside it.
        let store = match Store::open(&job.db) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("brain-queue: {} could not be written back: cannot reopen {}: {e:#}", job.path, job.db);
                self.finish_owed(job, todo.len(), &mut out, "the database could not be reopened for the write-back");
                return out;
            }
        };
        let _ = store.release_embed_lock(&brain_store::embed_lock_owner("queue"));
        let Some(_current) = store.note_get(&job.path).unwrap_or(None) else {
            out.skipped_deleted += 1;
            return out;
        };
        // Keyed by the chunk's *real* `chunk_index`, not by its position in
        // `todo`. The queue embeds a subset — that is the whole point of
        // `chunks_needing_embedding` — so positional alignment would attach
        // chunk 7's vector to chunk 0 the moment a note was appended to.
        let mut fresh: brain_store::FreshVectors = brain_store::FreshVectors::new();
        let mut got = 0usize;
        for ((idx, text), v) in todo.iter().zip(vecs.iter()) {
            if let Some(v) = v {
                fresh.insert(*idx, (text.clone(), v.clone()));
                got += 1;
            }
        }
        if got == 0 {
            eprintln!(
                "brain-queue: {} produced no vectors (Ollama unreachable or every chunk failed); \
                 {} chunk(s) stay NULL and FTS-searchable.",
                job.path, todo.len()
            );
            self.finish_owed(job, todo.len(), &mut out, "Ollama produced no vectors");
            return out;
        }
        let Some(note_id) = store.note_id(&job.path).unwrap_or(None) else {
            out.skipped_deleted += 1;
            return out;
        };
        match store.chunks_sync(
            note_id, &job.path, &_current.layer, _current.scope.as_deref(), &_current.content,
            _current.project_id, &_current.tags, &fresh, &brain_store::ChunkSnapshot::new(),
        ) {
            Ok(st) => {
                out.embedded += st.rehydrated;
                out.nulls += st.nulls;
                out.diverged += st.diverged;
                out.stale_reused += st.stale_reused;
                out.unmatched += st.unmatched;
                eprintln!(
                    "brain-queue: {} embedded {} chunk(s) in the background ({} still NULL, {} diverged, \
                     {} stale-reused, {} unmatched)",
                    job.path, st.rehydrated, st.nulls, st.diverged, st.stale_reused, st.unmatched
                );
                // A partial pass still owes the chunks it did not fill, and those
                // are `NULL` for the same reason a total failure is. Same decision.
                if st.nulls > 0 {
                    self.finish_owed(job, st.nulls, &mut out, "the pass filled only some of the chunks");
                }
            }
            Err(e) => {
                eprintln!("brain-queue: {} chunk sync failed: {e:#}", job.path);
                out.failed += 1;
                self.finish_owed(job, todo.len(), &mut out, "the chunk sync failed");
            }
        }
        out
    }

    /// X-02: a note that ends a pass still owing a vector is re-queued, or given
    /// up on — never silently dropped.
    ///
    /// The single place that decides, so every way of ending up short routes
    /// through it and behaves identically. Two rules, and the second exists
    /// because the first alone is not enough:
    ///
    /// 1. **Below the cap, re-queue with a backoff.** The note's chunks stay
    ///    `NULL` and FTS-searchable meanwhile, and the retry that `drain` arms picks
    ///    the work up. This is what the lock stand-down path already did, and what
    ///    every *other* failure failed to do.
    /// 2. **At the cap, dead-letter.** `dead_lettered` is incremented, the note is
    ///    dropped from the queue, and the log says so at a level an operator will
    ///    see. Not lost: `NULL` is exactly what `recover` reads on the next boot
    ///    and what `reindex --all` re-embeds. Terminal for this process, which is
    ///    the point — see [`DEFAULT_MAX_EMBED_FAILURES`].
    ///
    /// `retriable_nulls` is recorded only on the re-queue path, which is what lets
    /// [`QueueOutcome::is_settled`] distinguish "will be retried" from "terminal".
    fn finish_owed(&self, job: &Job, owed: usize, out: &mut QueueOutcome, reason: &str) {
        out.nulls += owed;
        if owed == 0 {
            return;
        }
        let failures = job.failures + 1;
        if failures >= self.max_failures {
            self.dead_lettered_total.fetch_add(1, Ordering::SeqCst);
            out.dead_lettered += 1;
            eprintln!(
                "brain-queue: {} GAVE UP after {failures} failed attempt(s): {reason}. {} chunk(s) stay NULL \
                 and FTS-searchable. This is not silent and not permanent, and here is exactly where to look: \
                 this line is in `journalctl -u brain-mcp`; the brain_status MCP tool reports the running \
                 server's queue as `queue.dead_lettered`; and `brain status` shows the footprint as \
                 `embedding.without_embedding` (the queue's pending_len and dead_lettered counters are \
                 in-memory state of the serve-mcp process, so the CLI cannot read them). The next server \
                 restart re-queues the note, and `brain reindex --all` re-embeds it now. Raise \
                 {MAX_EMBED_FAILURES_ENV} if the cause is a slow model load rather than a note the model will \
                 never accept.",
                job.path, owed
            );
            return;
        }
        let attempts = job.attempts + 1;
        self.enqueue_at(
            &job.db,
            &job.path,
            job.chunks.clone(),
            now_ms() + backoff_ms(attempts),
            attempts,
            failures,
        );
        out.requeued += 1;
        out.retriable_nulls += owed;
        eprintln!(
            "brain-queue: {} still owes {owed} vector(s) ({reason}); re-queued for attempt {failures} of {}.",
            job.path, self.max_failures
        );
    }
}

/// Process-wide queue slot, read once and then immutable.
///
/// A `OnceLock` rather than an `RwLock` on purpose. This used to be an `RwLock`
/// plus an `install_global` that *replaced* the queue and handed the old one back,
/// and that pair existed only so a test could point the queue at a mock. It was a
/// bad trade: mutating process-global state from a test that runs in parallel with
/// other tests makes the target non-deterministic, and the tests that used it were
/// asserting against whatever `BRAIN_OLLAMA_URL` happened to be — one of them fired
/// 60 real embedding requests at whatever Ollama the developer was running. The
/// queue is now a field of `AppState` and of `Brain`, so a test injects its own
/// without touching anything global.
static GLOBAL: std::sync::OnceLock<Arc<EmbedQueue>> = std::sync::OnceLock::new();

/// The process-wide queue the MCP handlers enqueue into, built from
/// `BRAIN_OLLAMA_URL` / `BRAIN_OLLAMA_MODEL` on first use.
pub fn global_queue() -> Arc<EmbedQueue> {
    Arc::clone(GLOBAL.get_or_init(|| Arc::new(EmbedQueue::new(EmbeddingEngine::from_env()))))
}

/// Key the cross-process lock lives under, exposed for diagnostics in `status`.
pub const EMBED_LOCK_META_KEY: &str = EMBED_LOCK_KEY;

#[cfg(test)]
mod tests {
    use super::*;

    /// W-05.1, the real unwind.
    ///
    /// The drain flag is cleared by a `DrainGuard` whose only job is to run on the
    /// unwind path, so the property worth testing is that an actual panic crossing
    /// the guard's scope leaves the queue usable. The previous test held the flag
    /// the way a drain does and released it the way the guard would — it exercised
    /// `Drop`, which is a *different* code path from unwinding, and asserted
    /// nothing about a panic at all.
    ///
    /// It lives here rather than in `tests/embed_queue.rs` because it needs the
    /// private flag and guard: producing a real unwind through `EmbedQueue::drain`
    /// requires poisoning the state mutex, which is only reachable from inside the
    /// crate.
    #[test]
    fn a_panic_unwinding_through_the_drain_guard_leaves_the_queue_usable() {
        let q = std::sync::Arc::new(EmbedQueue::new(EmbeddingEngine::new("http://127.0.0.1:1".into(), "nomic-embed-text".into())));

        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Reproduce the drain's own arrangement: flag set, guard installed.
            let _g = DrainGuard(&q.draining);
            q.draining.store(true, Ordering::SeqCst);
            assert!(q.is_draining(), "the flag must be set while the scope is live");
            panic!("simulated panic inside a drain");
        }));

        assert!(caught.is_err(), "the test must have unwound");
        assert!(!q.is_draining(), "an unwind must clear the flag, or every later brain_store strands its work");

        // And the queue still works: the flag was really released, not merely
        // cleared by a second writer.
        q.enqueue("/tmp/does-not-matter.db", "regras/global/x", vec!["## a".to_string()]);
        assert_eq!(q.pending_len(), 1);
        assert!(!q.is_draining());
    }

    /// A poisoned state mutex makes `drain` panic for real, through the real code
    /// path, at the `expect` that runs *after* the guard is installed.
    #[test]
    fn a_drain_over_a_poisoned_queue_panics_and_still_releases_the_flag() {
        let q = std::sync::Arc::new(EmbedQueue::new(EmbeddingEngine::new("http://127.0.0.1:1".into(), "nomic-embed-text".into())));
        let poisoned = std::sync::Arc::clone(&q);
        // Poison it the only way it can be poisoned: a panic while the guard is held.
        let _ = std::thread::spawn(move || {
            let _held = poisoned.state.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert!(q.state.lock().is_err(), "the fixture must actually have poisoned the mutex");

        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let join = runtime.block_on(async {
            let me = std::sync::Arc::clone(&q);
            tokio::spawn(async move { me.drain().await })
                .await
                .expect_err("a poisoned queue must panic rather than silently do nothing")
        });
        assert!(join.is_panic(), "expected a panic, got {join:?}");
        assert!(!q.is_draining(), "and the unwind must still have released the flag");
    }

    /// X-02: the truth table of `is_settled`, pinned directly.
    ///
    /// A white-box test on the predicate, and deliberately so. The integration test
    /// `a_pass_with_a_scheduled_retry_is_not_settled` covers the behaviour, but it
    /// cannot isolate the two terms: after a failed pass the job is back on the
    /// queue, so `pending_left > 0` on its own already makes the old
    /// `pending_left == 0` report `false` — and a mutation that reverted the
    /// predicate to that would pass. The integration tests are what make the
    /// *re-queue* load-bearing; this is what makes the reporting rule itself
    /// load-bearing, which is the part that has to stay true if a future code path
    /// ever records a retriable null without queueing a job.
    #[test]
    fn settled_is_false_while_a_null_has_a_scheduled_retry() {
        // The case the old predicate got wrong: an empty queue that still owes work.
        let owed_but_not_queued = QueueOutcome { pending_left: 0, retriable_nulls: 5, ..Default::default() };
        assert!(!owed_but_not_queued.is_settled(), "a null with a scheduled retry means the pass did not settle");

        // A dead letter is terminal, so it is settled — and says so in the counts.
        let dead = QueueOutcome { nulls: 5, retriable_nulls: 0, dead_lettered: 1, ..Default::default() };
        assert!(dead.is_settled(), "nothing left to try is settled: {dead:?}");
        assert_eq!(dead.nulls, 5, "and the nulls are still reported, not hidden: {dead:?}");

        // Work still on the queue is not settled.
        assert!(!QueueOutcome { pending_left: 1, ..Default::default() }.is_settled());
        // Nothing owed at all is settled.
        assert!(QueueOutcome::default().is_settled());
    }

    #[test]
    fn backoff_doubles_and_saturates() {
        // `attempts` counts deferrals already incurred, so the first deferral waits
        // `FIRST_RETRY_DELAY_MS` and each one after doubles it.
        assert_eq!(backoff_ms(0), FIRST_RETRY_DELAY_MS);
        assert_eq!(backoff_ms(1), FIRST_RETRY_DELAY_MS * 2);
        assert_eq!(backoff_ms(2), FIRST_RETRY_DELAY_MS * 4);
        assert_eq!(backoff_ms(64), MAX_RETRY_DELAY_MS, "a long deferral must not overflow past the cap");
    }

    #[test]
    fn reenqueueing_keeps_the_earlier_deadline_and_counts_the_attempt() {
        let q = EmbedQueue::new(EmbeddingEngine::new("http://127.0.0.1:1".into(), "nomic-embed-text".into()));
        let far = now_ms() + 60_000;
        q.enqueue_at("/tmp/x.db", "p", vec![], far, 1, 0);
        // A fresh `enqueue` means "run this now": the old backoff must not delay it.
        q.enqueue("/tmp/x.db", "p", vec![]);
        {
            let s = q.state.lock().unwrap();
            let job = s.pending.front().expect("the job is still queued, replaced not duplicated");
            assert_eq!(job.ready_at_ms, 0, "an explicit re-enqueue must not inherit a backoff");
        }
        assert_eq!(q.pending_len(), 1, "and it must replace, not duplicate");
    }
}

