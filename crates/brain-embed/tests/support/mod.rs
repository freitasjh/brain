//! Local mock Ollama server used by the `brain-embed` integration tests.
//!
//! The mock is a real HTTP server bound to `127.0.0.1:0`, so tests exercise the
//! genuine `reqwest` code path (status handling, JSON decoding, connection
//! errors, concurrency) without ever requiring a running Ollama instance.
//!
//! Per-prompt behaviour is described by [`Plan`]:
//! - `GET  /api/tags`       → `plan.tags_status`
//! - `POST /api/embeddings` → seed / delay / failure mode looked up by `prompt`
//!
//! The server also tracks how many embedding requests are in flight at once, so
//! tests can assert the batch concurrency cap is real and not just documented.

#![allow(dead_code)] // harness surface is shared by several test binaries' worth of cases

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Per-prompt behaviour of the mock `/api/embeddings` endpoint.
#[derive(Clone, Default)]
pub struct Plan {
    /// Status returned by `GET /api/tags` (200 = healthy).
    pub tags_status: u16,
    /// `prompt` → embedding seed, see [`vec_for`].
    pub seeds: HashMap<String, u32>,
    /// `prompt` → artificial server-side latency, in milliseconds.
    pub delays: HashMap<String, u64>,
    /// Prompts answered with HTTP 500.
    pub failing: HashSet<String>,
    /// Prompts answered HTTP 200 with a body that has no `embedding` key.
    pub missing_key: HashSet<String>,
    /// Prompts answered with a 384-element embedding (wrong dimension).
    pub short_dim: HashSet<String>,
    /// Prompts answered HTTP 200 with this exact body, bypassing every rule above.
    pub raw: HashMap<String, Value>,
    /// When set, **both** `GET /api/tags` and `POST /api/embeddings` answer 302
    /// with this `Location` instead of doing their job.
    ///
    /// Exists to exercise reqwest's *redirect* path, which is a different code
    /// path from the request path and attaches a different URL to its errors:
    /// the redirect target parsed from the `Location` header, which never goes
    /// through the userinfo stripping a normal request gets. So a redirect is
    /// the one way to get a `reqwest::Error` whose embedded URL still carries a
    /// credential.
    ///
    /// It used to apply to `GET /api/tags` only, which is a harness gap with a
    /// real cost: `health_check` was testable against this vector and `embed` was
    /// not, so the one sink that reaches journald on every `brain_search` had no
    /// case exercising it and the leak shipped. A redirect fixture that only
    /// covers half the sinks is a fixture that hides the half that matters.
    pub redirect: Option<String>,
}

impl Plan {
    /// Healthy tags endpoint plus a seed for every prompt given.
    pub fn seeded(prompts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            tags_status: 200,
            seeds: prompts
                .into_iter()
                .enumerate()
                .map(|(i, p)| (p.into(), i as u32))
                .collect(),
            ..Default::default()
        }
    }

    fn reply(&self, prompt: &str) -> (u16, Value) {
        if let Some(body) = self.raw.get(prompt) {
            return (200, body.clone());
        }
        if self.failing.contains(prompt) {
            return (500, json!({"error": "mock ollama failure"}));
        }
        if self.missing_key.contains(prompt) {
            return (200, json!({"models": ["nomic-embed-text"]}));
        }
        let dim = if self.short_dim.contains(prompt) {
            384
        } else {
            brain_core::EMBEDDING_DIM
        };
        let seed = self.seeds.get(prompt).copied().unwrap_or(0);
        (200, json!({"embedding": json_vec(&vec_for(dim, seed))}))
    }
}

/// Deterministic embedding of `dim` elements, offset by `seed`.
///
/// Values are `k / 16.0` with `k < 17`, i.e. exactly representable in binary
/// floating point, so an f64 JSON round-trip must reproduce them bit for bit.
/// Distinct seeds in `0..=16` always produce distinct vectors.
pub fn vec_for(dim: usize, seed: u32) -> Vec<f32> {
    (0..dim)
        .map(|i| ((i as u32).wrapping_add(seed) % 17) as f32 / 16.0)
        .collect()
}

/// Serialises a float vector into a JSON array.
pub fn json_vec(v: &[f32]) -> Value {
    Value::Array(v.iter().map(|x| json!(*x)).collect())
}

/// A running mock Ollama bound to an ephemeral loopback port.
pub struct Mock {
    /// Base URL to hand to `EmbeddingEngine::new`, no trailing slash.
    pub base_url: String,
    /// Peak number of embedding requests observed in flight simultaneously.
    pub max_inflight: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl Mock {
    /// Binds an ephemeral port and spawns the server.
    pub async fn start(plan: Plan) -> Self {
        let inflight = Arc::new(AtomicUsize::new(0));
        let max_inflight = Arc::new(AtomicUsize::new(0));
        let plan = Arc::new(plan);
        let app = Router::new()
            .route("/api/tags", get(tags_handler))
            .route("/api/embeddings", post(embed_handler))
            .with_state(Arc::new(State0 {
                plan: plan.clone(),
                inflight: inflight.clone(),
                max_inflight: max_inflight.clone(),
            }));

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock ollama");
        let addr: SocketAddr = listener.local_addr().expect("mock ollama addr");
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Self {
            base_url: format!("http://{addr}"),
            max_inflight,
            task,
        }
    }

    /// Convenience: engine pointed at this mock, using `model` as the model name.
    pub fn engine(&self) -> brain_embed::EmbeddingEngine {
        brain_embed::EmbeddingEngine::new(self.base_url.clone(), "nomic-embed-text".into())
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct State0 {
    plan: Arc<Plan>,
    inflight: Arc<AtomicUsize>,
    max_inflight: Arc<AtomicUsize>,
}

async fn tags_handler(State(st): State<Arc<State0>>) -> Response {
    if let Some(r) = redirect_response(&st.plan) {
        return r;
    }
    let status =
        StatusCode::from_u16(st.plan.tags_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        axum::Json(json!({"models": [{"name": "nomic-embed-text"}]})),
    )
        .into_response()
}

/// The 302 the `redirect` plan describes, shared by both endpoints so the fixture
/// cannot cover one sink and quietly skip the other.
fn redirect_response(plan: &Plan) -> Option<Response> {
    let location = plan.redirect.as_ref()?;
    let Ok(value) = HeaderValue::from_str(location) else {
        return Some((StatusCode::INTERNAL_SERVER_ERROR, "bad redirect fixture").into_response());
    };
    Some((StatusCode::FOUND, [(axum::http::header::LOCATION, value)], "").into_response())
}

async fn embed_handler(State(st): State<Arc<State0>>, body: Bytes) -> Response {
    if let Some(r) = redirect_response(&st.plan) {
        return r;
    }
    let prompt = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|v| v.get("prompt").and_then(|p| p.as_str()).map(str::to_string))
        .unwrap_or_default();

    let current = st.inflight.fetch_add(1, Ordering::SeqCst) + 1;
    st.max_inflight.fetch_max(current, Ordering::SeqCst);

    let delay_ms = st.plan.delays.get(&prompt).copied().unwrap_or(0);
    if delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    }

    st.inflight.fetch_sub(1, Ordering::SeqCst);

    let (code, payload) = st.plan.reply(&prompt);
    let status = StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, axum::Json(payload)).into_response()
}

/// Reserves a loopback port and immediately releases it, yielding a URL that is
/// guaranteed to refuse connections.
pub async fn closed_port_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind probe port");
    let addr = listener.local_addr().expect("probe addr");
    drop(listener);
    format!("http://{addr}")
}

/// Splices `user:pass@` into the authority of an `http://…` base URL — what an
/// operator gets from `BRAIN_OLLAMA_URL=http://user:pass@host:11434`.
///
/// Written as a string splice rather than a parse/serialise round-trip on
/// purpose: the redaction tests must feed `EmbeddingEngine` exactly the bytes an
/// operator would type, not a URL a parser has already normalised.
pub fn credentialed(base_url: &str, user: &str, pass: &str) -> String {
    base_url.replacen("://", &format!("://{user}:{pass}@"), 1)
}
