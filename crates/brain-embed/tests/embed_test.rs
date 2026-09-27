//! `brain-embed` integration tests.
//!
//! Every case runs against the local mock in `support::` — no test in this file
//! requires a real Ollama instance.

mod support;

use brain_core::EMBEDDING_DIM;
use brain_embed::{
    BATCH_WAVE_CAP, DEFAULT_BASE_URL, DEFAULT_EMBED_TIMEOUT_SECS, DEFAULT_MODEL, EMBED_TIMEOUT_ENV,
    EmbeddingEngine, embed_timeout_secs,
};
use support::{Mock, Plan, json_vec, vec_for};

const MODEL: &str = "nomic-embed-text";

// ---------------------------------------------------------------- EMBED-02 --
// new() / from_options() / from_env()

#[test]
fn new_trims_single_trailing_slash() {
    let eng = EmbeddingEngine::new("http://x:11434/".into(), MODEL.into());
    assert_eq!(eng.base_url, "http://x:11434");
}

#[test]
fn new_trims_repeated_trailing_slashes() {
    let eng = EmbeddingEngine::new("http://x:11434///".into(), MODEL.into());
    assert_eq!(eng.base_url, "http://x:11434");
}

#[test]
fn new_keeps_url_without_trailing_slash_and_applies_defaults() {
    let eng = EmbeddingEngine::new("http://x:11434".into(), MODEL.into());
    assert_eq!(eng.base_url, "http://x:11434");
    assert_eq!(eng.model, MODEL);
    assert_eq!(eng.timeout_secs, 30);
}

#[test]
fn new_preserves_sub_path_and_still_trims_only_trailing_slash() {
    let eng = EmbeddingEngine::new("http://x:11434/proxy/ollama//".into(), MODEL.into());
    assert_eq!(eng.base_url, "http://x:11434/proxy/ollama");
}

#[test]
fn from_options_falls_back_to_defaults_when_absent() {
    let eng = EmbeddingEngine::from_options(None, None);
    assert_eq!(eng.base_url, DEFAULT_BASE_URL);
    assert_eq!(eng.model, DEFAULT_MODEL);
}

#[test]
fn from_options_falls_back_to_defaults_when_blank() {
    // A blank export (e.g. `BRAIN_OLLAMA_URL=`) must not yield a relative URL.
    let eng = EmbeddingEngine::from_options(Some("   ".into()), Some(String::new()));
    assert_eq!(eng.base_url, DEFAULT_BASE_URL);
    assert_eq!(eng.model, DEFAULT_MODEL);
}

#[test]
fn from_options_honours_explicit_values_and_trims() {
    let eng = EmbeddingEngine::from_options(
        Some("http://ollama:9999/".into()),
        Some("mxbai-embed-large".into()),
    );
    assert_eq!(eng.base_url, "http://ollama:9999");
    assert_eq!(eng.model, "mxbai-embed-large");
    assert_eq!(eng.timeout_secs, 30);
}

#[test]
fn from_env_resolves_against_actual_environment() {
    // Read-only: never mutates the process environment (unsafe in edition 2024
    // and racy against other threads), so it is safe to run alongside the rest.
    let eng = EmbeddingEngine::from_env();
    let expected_url = std::env::var("BRAIN_OLLAMA_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let expected_model = std::env::var("BRAIN_OLLAMA_MODEL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    assert_eq!(eng.base_url, expected_url.trim_end_matches('/'));
    assert_eq!(eng.model, expected_model);
    assert_eq!(eng.timeout_secs, 30);
}

// ---------------------------------------------------------------- EMBED-03 --
// health_check()

#[tokio::test]
async fn health_check_true_on_200() {
    let mock = Mock::start(Plan {
        tags_status: 200,
        ..Default::default()
    })
    .await;
    assert!(mock.engine().health_check().await);
}

#[tokio::test]
async fn health_check_false_on_404() {
    let mock = Mock::start(Plan {
        tags_status: 404,
        ..Default::default()
    })
    .await;
    assert!(!mock.engine().health_check().await);
}

#[tokio::test]
async fn health_check_false_on_500() {
    let mock = Mock::start(Plan {
        tags_status: 500,
        ..Default::default()
    })
    .await;
    assert!(!mock.engine().health_check().await);
}

#[tokio::test]
async fn health_check_false_when_connection_refused_and_does_not_panic() {
    // Regression: the client builder used to be unwrapped, so a dead socket
    // could take the process down. Must return false, not panic.
    let url = support::closed_port_url().await;
    let eng = EmbeddingEngine::new(url, MODEL.into());
    assert!(!eng.health_check().await);
}

// ---------------------------------------------------------------- EMBED-04 --
// embed()

#[tokio::test]
async fn embed_rejects_empty_text() {
    let mock = Mock::start(Plan::default()).await;
    let err = mock
        .engine()
        .embed("")
        .await
        .expect_err("empty text must fail");
    assert!(err.to_string().contains("empty text"), "got: {err}");
}

#[tokio::test]
async fn embed_rejects_whitespace_only_text() {
    let mock = Mock::start(Plan::default()).await;
    let err = mock
        .engine()
        .embed("   \n\t  ")
        .await
        .expect_err("blank text must fail");
    assert!(err.to_string().contains("empty text"), "got: {err}");
}

#[tokio::test]
async fn embed_error_names_the_target_url_so_fts_fallback_is_diagnosable() {
    // Callers swallow this error and fall back to FTS-only, so the message is
    // the only diagnostic left when embeddings silently disappear.
    let url = support::closed_port_url().await;
    let eng = EmbeddingEngine::new(url.clone(), MODEL.into());
    let err = eng
        .embed("hello")
        .await
        .expect_err("refused connection must fail");
    let msg = err.to_string();
    assert!(msg.contains(&url), "error should name the base url: {msg}");
    assert!(
        msg.contains("/api/embeddings"),
        "error should name the endpoint: {msg}"
    );
}

#[tokio::test]
async fn embed_ok_returns_full_width_vector() {
    let mock = Mock::start(Plan::seeded(["hello brain"])).await;
    let out = mock
        .engine()
        .embed("hello brain")
        .await
        .expect("embed should succeed");
    assert_eq!(out.len(), EMBEDDING_DIM);
    assert_eq!(out, vec_for(EMBEDDING_DIM, 0));
}

#[tokio::test]
async fn embed_on_500_reports_ollama_status() {
    let mut plan = Plan::default();
    plan.failing.insert("boom".into());
    let mock = Mock::start(plan).await;
    let err = mock
        .engine()
        .embed("boom")
        .await
        .expect_err("500 must fail");
    assert!(err.to_string().contains("ollama"), "got: {err}");
    assert!(err.to_string().contains("500"), "got: {err}");
}

#[tokio::test]
async fn embed_without_embedding_key_reports_missing_embedding() {
    let mut plan = Plan::default();
    plan.missing_key.insert("no-key".into());
    let mock = Mock::start(plan).await;
    let err = mock
        .engine()
        .embed("no-key")
        .await
        .expect_err("missing key must fail");
    assert!(err.to_string().contains("missing embedding"), "got: {err}");
}

#[tokio::test]
async fn embed_wrong_dimension_reports_dim_mismatch() {
    let mut plan = Plan::default();
    plan.short_dim.insert("small".into());
    let mock = Mock::start(plan).await;
    let err = mock
        .engine()
        .embed("small")
        .await
        .expect_err("384 dims must fail");
    let msg = err.to_string();
    assert!(msg.contains("dim mismatch"), "got: {msg}");
    assert!(msg.contains("384"), "got: {msg}");
    assert!(msg.contains(&EMBEDDING_DIM.to_string()), "got: {msg}");
}

#[tokio::test]
async fn embed_coerces_non_numeric_entries_to_zero_without_panicking() {
    let mut values: Vec<serde_json::Value> = vec![serde_json::json!(0.0); EMBEDDING_DIM];
    values[0] = serde_json::json!("abc");
    values[1] = serde_json::Value::Null;
    values[2] = serde_json::json!(true);
    values[3] = serde_json::json!(0.75);
    let mut plan = Plan::default();
    plan.raw
        .insert("weird".into(), serde_json::json!({"embedding": values}));

    let mock = Mock::start(plan).await;
    let out = mock
        .engine()
        .embed("weird")
        .await
        .expect("non-numeric must degrade, not fail");
    assert_eq!(out.len(), EMBEDDING_DIM);
    assert_eq!(out[0], 0.0, "string entry becomes 0.0");
    assert_eq!(out[1], 0.0, "null entry becomes 0.0");
    assert_eq!(out[2], 0.0, "bool entry becomes 0.0");
    assert_eq!(out[3], 0.75, "valid neighbour survives");
}

#[tokio::test]
async fn embed_preserves_values_across_f64_json_roundtrip() {
    let expected = vec_for(EMBEDDING_DIM, 5);
    let mut plan = Plan::default();
    plan.raw.insert(
        "precision".into(),
        serde_json::json!({"embedding": json_vec(&expected)}),
    );

    let mock = Mock::start(plan).await;
    let out = mock
        .engine()
        .embed("precision")
        .await
        .expect("embed should succeed");
    assert_eq!(out.len(), expected.len());
    for (i, (got, want)) in out.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "value {i} changed during f64 -> f32 roundtrip");
    }
}

// ---------------------------------------------------------------- EMBED-05 --
// embed_batch_concurrent()

#[tokio::test]
async fn batch_of_nothing_succeeds() {
    let mock = Mock::start(Plan::default()).await;
    let out = mock
        .engine()
        .embed_batch_concurrent(vec![])
        .await
        .expect("empty batch ok");
    assert!(out.is_empty());
}

#[tokio::test]
async fn batch_preserves_input_order_despite_shuffled_latency() {
    // Deliberately inverted latency: the last input answers first.
    let mut plan = Plan::default();
    plan.seeds.insert("first".into(), 1);
    plan.seeds.insert("second".into(), 2);
    plan.seeds.insert("third".into(), 3);
    plan.delays.insert("first".into(), 300);
    plan.delays.insert("second".into(), 150);
    plan.delays.insert("third".into(), 0);

    let mock = Mock::start(plan).await;
    let texts = vec![
        "first".to_string(),
        "second".to_string(),
        "third".to_string(),
    ];
    let out = mock
        .engine()
        .embed_batch_concurrent(texts)
        .await
        .expect("batch ok");

    assert_eq!(out.len(), 3);
    for (i, seed) in [1u32, 2, 3].into_iter().enumerate() {
        assert_eq!(
            out[i],
            vec_for(EMBEDDING_DIM, seed),
            "position {i} holds the wrong text"
        );
    }
}

#[tokio::test]
async fn batch_fails_entirely_when_one_text_fails() {
    let mut plan = Plan::default();
    plan.seeds.insert("good".into(), 0);
    plan.failing.insert("bad".into());
    let mock = Mock::start(plan).await;

    let texts = vec!["good".to_string(), "bad".to_string()];
    let err = mock
        .engine()
        .embed_batch_concurrent(texts)
        .await
        .expect_err("batch must fail");
    assert!(err.to_string().contains("ollama"), "got: {err}");
}

#[tokio::test]
async fn batch_rejects_blank_text_inside_the_batch() {
    let mut plan = Plan::default();
    plan.seeds.insert("good".into(), 0);
    let mock = Mock::start(plan).await;

    let texts = vec!["good".to_string(), "  ".to_string()];
    let err = mock
        .engine()
        .embed_batch_concurrent(texts)
        .await
        .expect_err("batch must fail");
    assert!(err.to_string().contains("empty text"), "got: {err}");
}

#[tokio::test]
async fn batch_runs_concurrently_but_never_exceeds_four_in_flight() {
    // 8 texts, each with 150ms of server latency: if the batch were serial the
    // mock would only ever see 1 request in flight, and 4 is the documented cap.
    const N: usize = 8;
    const DELAY_MS: u64 = 150;
    let plan = Plan {
        tags_status: 200,
        seeds: (0..N).map(|i| (format!("text-{i}"), i as u32)).collect(),
        delays: (0..N).map(|i| (format!("text-{i}"), DELAY_MS)).collect(),
        ..Default::default()
    };
    let mock = Mock::start(plan).await;

    let texts: Vec<String> = (0..N).map(|i| format!("text-{i}")).collect();
    let out = mock
        .engine()
        .embed_batch_concurrent(texts)
        .await
        .expect("batch ok");

    assert_eq!(out.len(), N);
    let peak = mock.max_inflight.load(std::sync::atomic::Ordering::SeqCst);
    eprintln!("[harness] peak in-flight embedding requests = {peak} (batch of {N})");
    assert!(
        peak <= 4,
        "semaphore must cap in-flight requests at 4, observed {peak}"
    );
    assert!(peak > 1, "batch ran serially (peak in-flight = {peak})");
    for (i, v) in out.iter().enumerate() {
        assert_eq!(
            *v,
            vec_for(EMBEDDING_DIM, i as u32),
            "position {i} holds the wrong text"
        );
    }
}

// ---------------------------------------------------------------- EMBED-06 --
// embed_batch_partial(): per-chunk tolerance.
//
// The reported bug was not that embedding failed — Ollama was up and returning
// vectors. It was that a *single* failure in the batch took down every sibling
// chunk, and the caller's only remaining option was to write a BLOB of zeros.
// `embed_batch_partial` is the fix, so these cases pin the tolerance itself.

#[tokio::test]
async fn partial_batch_keeps_successful_chunks_when_one_text_fails() {
    // The exact shape of the production bug: `good` embeds, `bad` does not, and
    // the strict variant failed the whole batch so both were lost.
    let mut plan = Plan::default();
    plan.seeds.insert("good".into(), 3);
    plan.failing.insert("bad".into());
    let mock = Mock::start(plan).await;

    let out = mock
        .engine()
        .embed_batch_partial(vec!["good".to_string(), "bad".to_string()])
        .await;

    assert_eq!(out.len(), 2, "one slot per input, always");
    assert_eq!(out[0].as_ref().unwrap(), &vec_for(EMBEDDING_DIM, 3), "the healthy chunk must survive its failing sibling");
    assert!(out[1].is_none(), "only the failing chunk is dropped");
}

#[tokio::test]
async fn partial_batch_reports_none_per_failing_text_not_all_or_nothing() {
    // Four distinct failure modes; the three healthy neighbours must all live.
    let mut plan = Plan::default();
    plan.seeds.insert("ok1".into(), 1);
    plan.seeds.insert("ok2".into(), 2);
    plan.failing.insert("http500".into());
    plan.missing_key.insert("nokey".into());
    plan.short_dim.insert("shortdim".into());
    let mock = Mock::start(plan).await;

    let out = mock
        .engine()
        .embed_batch_partial(
            ["ok1", "http500", "ok2", "nokey", "shortdim"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )
        .await;

    assert_eq!(out.iter().filter(|v| v.is_some()).count(), 2, "2 of 5 embedded");
    assert_eq!(out[0].as_ref().unwrap(), &vec_for(EMBEDDING_DIM, 1));
    assert_eq!(out[2].as_ref().unwrap(), &vec_for(EMBEDDING_DIM, 2), "position 2 must hold ok2, not a shifted result");
    for i in [1usize, 3, 4] {
        assert!(out[i].is_none(), "slot {i} must be None");
    }
}

#[tokio::test]
async fn partial_batch_preserves_input_order_despite_shuffled_latency() {
    let mut plan = Plan::default();
    plan.seeds.insert("first".into(), 1);
    plan.seeds.insert("second".into(), 2);
    plan.seeds.insert("third".into(), 3);
    plan.delays.insert("first".into(), 300);
    plan.delays.insert("second".into(), 150);
    plan.delays.insert("third".into(), 0);
    let mock = Mock::start(plan).await;

    let out = mock
        .engine()
        .embed_batch_partial(vec!["first".into(), "second".into(), "third".into()])
        .await;

    for (i, seed) in [1u32, 2, 3].into_iter().enumerate() {
        assert_eq!(out[i].as_ref().unwrap(), &vec_for(EMBEDDING_DIM, seed), "position {i} holds the wrong text");
    }
}

#[tokio::test]
async fn partial_batch_is_all_none_when_ollama_is_unreachable() {
    // Regression: the client builder used to be unwrapped, so a dead socket could
    // take the process down. Must be an all-None vector, never a panic and never
    // a zero-filled one.
    let url = support::closed_port_url().await;
    let eng = EmbeddingEngine::new(url, MODEL.into());
    let out = eng
        .embed_batch_partial(vec!["a".into(), "b".into()])
        .await;
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(Option::is_none), "unreachable Ollama yields all-None");
}

#[tokio::test]
async fn partial_batch_never_yields_a_zero_vector() {
    // The load-bearing property. A `Some(vec![0.0; 768])` here would travel all
    // the way into `chunks.embedding` and reproduce the production bug exactly,
    // so assert it at the source rather than trusting every call site.
    let mut plan = Plan::default();
    plan.raw.insert(
        "allzero".into(),
        serde_json::json!({"embedding": vec![serde_json::json!(0.0); EMBEDDING_DIM]}),
    );
    plan.seeds.insert("healthy".into(), 5);
    let mock = Mock::start(plan).await;

    let out = mock
        .engine()
        .embed_batch_partial(vec!["allzero".into(), "healthy".into()])
        .await;

    let zero_len = out
        .iter()
        .filter_map(|v| v.as_ref())
        .filter(|v| v.iter().all(|f| *f == 0.0))
        .count();
    assert_eq!(zero_len, 0, "embed_batch_partial must never return a zero-norm vector");
    assert_eq!(out[1].as_ref().unwrap(), &vec_for(EMBEDDING_DIM, 5));
}

#[tokio::test]
async fn partial_batch_of_nothing_is_empty() {
    let mock = Mock::start(Plan::default()).await;
    assert!(mock.engine().embed_batch_partial(vec![]).await.is_empty());
}

#[tokio::test]
async fn partial_batch_still_caps_in_flight_requests_at_four() {
    // Tolerance must not come at the cost of the concurrency cap.
    const N: usize = 8;
    let plan = Plan {
        tags_status: 200,
        seeds: (0..N).map(|i| (format!("text-{i}"), i as u32)).collect(),
        delays: (0..N).map(|i| (format!("text-{i}"), 150)).collect(),
        ..Default::default()
    };
    let mock = Mock::start(plan).await;
    let texts: Vec<String> = (0..N).map(|i| format!("text-{i}")).collect();
    let out = mock.engine().embed_batch_partial(texts).await;
    assert_eq!(out.len(), N);
    assert!(out.iter().all(Option::is_some));
    let peak = mock.max_inflight.load(std::sync::atomic::Ordering::SeqCst);
    assert!(peak <= 4, "in-flight cap breached: {peak}");
    assert!(peak > 1, "batch ran serially: {peak}");
}

// ---------------------------------------------------------------- EMBED-07 --
// Timeout budget. The old hardcoded 3s expired during a cold
// `nomic-embed-text` load, which is why every MCP-stored note ended up with
// zero vectors while Ollama was up and healthy.

#[test]
fn embed_timeout_defaults_to_sixty_seconds_when_env_is_unusable() {
    // Read-only: never mutates the process environment (unsafe in edition 2024
    // and racy against other threads). Asserts the default resolves when the
    // variable is absent, and that a junk value cannot resolve to 0.
    assert_eq!(DEFAULT_EMBED_TIMEOUT_SECS, 60, "3s was shorter than a cold model load");
    let secs = embed_timeout_secs();
    match std::env::var(EMBED_TIMEOUT_ENV) {
        Err(_) => assert_eq!(secs, 60),
        Ok(raw) if raw.trim().parse::<u64>().map(|v| v > 0).unwrap_or(false) => {
            assert_eq!(secs, raw.trim().parse::<u64>().unwrap())
        }
        // Blank, non-numeric or 0 must all fall back rather than disable embedding.
        Ok(_) => assert_eq!(secs, 60, "unusable {} must fall back to the default", EMBED_TIMEOUT_ENV),
    }
}

#[test]
fn batch_timeout_scales_with_the_number_of_concurrency_waves() {
    // Ollama's `OLLAMA_NUM_PARALLEL` defaults to 1, so a batch is served
    // *serially* and needs one wave per text. The old formula divided by the
    // client-side semaphore width (4) on the assumption that four requests were
    // really in flight; they were not, so every multi-chunk batch got a quarter
    // of the budget it needed and came back as a wall of NULL chunks.
    let eng = EmbeddingEngine::new("http://x:11434".into(), MODEL.into());
    let base = embed_timeout_secs();
    for (n, waves) in [(1usize, 1u64), (4, 4), (5, 5), (8, 8)] {
        assert_eq!(eng.batch_waves(n), waves as usize, "batch_waves({n}) should be {waves}");
        assert_eq!(eng.batch_timeout(n).as_secs(), base * waves, "batch_timeout({n}) should be {waves} wave(s)");
    }
    assert_eq!(eng.batch_waves(9), BATCH_WAVE_CAP, "the cap takes over at 9");
    assert!(eng.batch_timeout(1).as_secs() > 3, "a single chunk must get more than the old 3s");
    assert_eq!(eng.batch_timeout(0).as_secs(), base, "an empty batch still gets one wave, never zero");
}

#[test]
fn batch_timeout_is_capped_so_a_large_batch_cannot_ask_for_an_unbounded_wait() {
    // The formula is linear in the wave count, so without a ceiling a
    // 1,000-chunk corpus asks for 60 * 1,000 = 16.7 hours and the caller is
    // wedged behind a timeout that can never expire. The cap is what makes the
    // budget a bound rather than an escalation.
    let eng = EmbeddingEngine::new("http://x:11434".into(), MODEL.into());
    let base = embed_timeout_secs();
    for n in [BATCH_WAVE_CAP, BATCH_WAVE_CAP + 1, 100, 1_000, 100_000] {
        assert_eq!(eng.batch_waves(n), BATCH_WAVE_CAP, "batch_waves({n}) must be capped");
        assert_eq!(eng.batch_timeout(n).as_secs(), base * BATCH_WAVE_CAP as u64);
    }
    // The default ceiling is 480s, against ~26s of real work for the largest
    // note MAX_CHUNKS allows: ~19x headroom, not a wall.
    assert_eq!(BATCH_WAVE_CAP, 8);
    let ceiling = eng.batch_timeout(brain_core::MAX_CHUNKS).as_secs();
    assert!(ceiling <= 480, "default ceiling must stay bounded, got {ceiling}s");
}

#[test]
fn the_default_ceiling_covers_the_worst_case_a_legal_note_can_reach() {
    // The wave cap is smaller than MAX_CHUNKS (8 vs 64), so the ceiling does not
    // come from "one wave per chunk" — it comes from the ceiling being much
    // larger than the time a legal note actually needs. Measured: ~0.4 s per
    // chunk served serially, so the largest legal note costs ~26 s against a
    // 480 s budget, ~18x headroom. The design bound assumed here is a pessimistic
    // 2 s/chunk (5x the measurement) and it still fits.
    const SLOW_SECONDS_PER_CHUNK: u64 = 2;
    let worst_case = SLOW_SECONDS_PER_CHUNK * brain_core::MAX_CHUNKS as u64;
    let eng = EmbeddingEngine::new("http://x:11434".into(), MODEL.into());
    let ceiling = eng.batch_timeout(brain_core::MAX_CHUNKS).as_secs();
    assert_eq!(BATCH_WAVE_CAP, 8, "the cap is what keeps the budget bounded");
    assert_eq!(ceiling, embed_timeout_secs() * BATCH_WAVE_CAP as u64);
    assert!(
        ceiling > worst_case,
        "a {}-chunk note can need {worst_case}s at a pessimistic {SLOW_SECONDS_PER_CHUNK}s/chunk, \
         but the ceiling is only {ceiling}s — legal notes would be silently abandoned",
        brain_core::MAX_CHUNKS
    );
    // Consequence of the same relationship, stated as an operator rule: lowering
    // the base below `SLOW_SECONDS_PER_CHUNK * MAX_CHUNKS / BATCH_WAVE_CAP` (3s
    // here) starts costing legitimate chunks. Documented rather than enforced,
    // because the base is a deliberate operator knob.
    let floor = worst_case / BATCH_WAVE_CAP as u64;
    assert!(floor >= 1, "the base must not fall below {floor}s without losing chunks");
}
