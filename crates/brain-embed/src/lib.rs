//! Ollama embedding client.
//!
//! Single responsibility for this crate: talk to the local Ollama HTTP API.
//! - `GET  {base_url}/api/tags`       → liveness probe (`health_check`)
//! - `POST {base_url}/api/embeddings` → `nomic-embed-text` vectors of `EMBEDDING_DIM`
//!
//! Timeouts come from [`EMBED_TIMEOUT_ENV`] (default
//! [`DEFAULT_EMBED_TIMEOUT_SECS`]); see [`embed_timeout_secs`] and
//! [`EmbeddingEngine::batch_timeout`].
//!
//! All network IO lives here so `brain-core` stays pure and `brain-store` stays
//! synchronous. Callers are expected to degrade to FTS-only search when this
//! engine is unavailable, so nothing here may panic: an unreachable Ollama must
//! surface as `false` / `Err` / `None`, never as a `panic!` and never as a
//! zero-filled vector standing in for a failed embed.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use brain_core::EMBEDDING_DIM;

/// Endpoint used when `BRAIN_OLLAMA_URL` is unset or blank.
pub const DEFAULT_BASE_URL: &str = "http://localhost:11434";

/// Model used when `BRAIN_OLLAMA_MODEL` is unset or blank.
pub const DEFAULT_MODEL: &str = "nomic-embed-text";

/// Per-request timeout applied by [`EmbeddingEngine::new`], in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Environment variable holding the base embedding timeout, in seconds.
pub const EMBED_TIMEOUT_ENV: &str = "BRAIN_EMBED_TIMEOUT_SECS";

/// Base embedding timeout when [`EMBED_TIMEOUT_ENV`] is unset, blank or unusable.
///
/// 60s, up from the hardcoded 3s the MCP store handler used to wrap its batch in.
/// 3s is shorter than a cold `nomic-embed-text` load (274 MB pulled from disk), so
/// the timeout expired before the model produced anything and *every* note stored
/// through MCP was written with zero vectors — a silent loss of the whole semantic
/// stream. See `chunks_sync` in `brain-store` for why "no vector" is the correct
/// degradation and a zero vector is not.
pub const DEFAULT_EMBED_TIMEOUT_SECS: u64 = 60;

/// Base embedding timeout in seconds, read from [`EMBED_TIMEOUT_ENV`].
///
/// Unset, blank, non-numeric and `0` all resolve to
/// [`DEFAULT_EMBED_TIMEOUT_SECS`], so a bad export degrades to the documented
/// default instead of to "give up immediately".
///
/// The base budget is deliberately independent of
/// [`EmbeddingEngine::timeout_secs`] (the per-request socket timeout): a batch of
/// `n` chunks needs `ceil(n / effective_parallelism)` sequential waves, so a
/// single-request budget would abandon a batch while its later waves are still
/// legitimately in flight. Use [`EmbeddingEngine::batch_timeout`] to get the
/// budget for a concrete batch, which also caps the wave count.
pub fn embed_timeout_secs() -> u64 {
    std::env::var(EMBED_TIMEOUT_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_EMBED_TIMEOUT_SECS)
}

/// Hard cap for the health probe, in seconds. Deliberately short: callers poll
/// this before deciding whether to embed, and must not block on a dead socket.
const HEALTH_TIMEOUT_SECS: u64 = 5;

/// Maximum number of in-flight `/api/embeddings` requests per batch.
///
/// This is a *client-side* ceiling. It is not a claim about how many requests
/// the server actually serves at once — see [`SERVER_PARALLELISM`].
const BATCH_CONCURRENCY: usize = 4;

/// How many embedding requests Ollama really serves concurrently.
///
/// `OLLAMA_NUM_PARALLEL` defaults to **1**, so a fresh install serialises every
/// `/api/embeddings` call no matter how many the client has in flight. Measured
/// with it unset: 1 chunk 0.04 s, 8 chunks 0.40-0.59 s, 32 chunks 1.4-1.8 s,
/// 64 chunks 2.8-3.9 s — strictly linear in the chunk count, ~0.045 s each, so
/// the four-at-a-time semaphore in [`EmbeddingEngine::drain`] bought nothing. The
/// value is deliberately the conservative one: overestimating real parallelism
/// *underestimates* the time a batch needs, which is how a batch gets abandoned
/// half-done and silently leaves chunks `NULL`.
///
/// An operator who raises `OLLAMA_NUM_PARALLEL` also wants this raised, at which
/// point [`BATCH_CONCURRENCY`] is the binding ceiling anyway.
const SERVER_PARALLELISM: usize = 1;

/// Upper bound on the number of sequential waves [`EmbeddingEngine::batch_timeout`]
/// will budget for, whatever the batch size.
///
/// Wave scaling is linear, so without a ceiling a 1,000-chunk corpus asks for
/// `60 * 1,000 = 16.7 hours` and the caller is effectively wedged behind a
/// timeout that can never expire. 8 waves is what makes the budget a bound rather
/// than an escalation, and it comfortably covers a single legal note: a
/// [`brain_core::MAX_CHUNKS`]-chunk note needs ~3 s at the ~0.045 s/chunk this
/// box measures, and ~26 s at the pessimistic
/// [`brain_core::PESSIMISTIC_SECONDS_PER_CHUNK`] the limits are sized against —
/// against a 480 s default budget either way. Past the cap the budget stops
/// growing: an under-budget is a loud `NULL` plus a log line, a multi-hour hang
/// is neither.
pub const BATCH_WAVE_CAP: usize = 8;

/// Effective in-flight requests per batch: the client ceiling, capped by what
/// the server will actually serve.
const fn effective_parallelism() -> usize {
    if SERVER_PARALLELISM < BATCH_CONCURRENCY { SERVER_PARALLELISM } else { BATCH_CONCURRENCY }
}

/// What replaces a URL's userinfo once redacted.
const REDACTED: &str = "***";

/// Removes the credential from a URL, leaving scheme, host, port and path intact.
///
/// `BRAIN_OLLAMA_URL` is an operator-supplied string and nothing stops it from
/// being `http://user:pass@host:11434`. The **host** belongs in an error message
/// on purpose — callers degrade to FTS-only on that error, so naming the
/// endpoint is the only thing that makes the fallback diagnosable — but the
/// credential must not ride along into a log file, an MCP client, or a
/// `brain status` dump.
///
/// Returns the input **byte-identical** whenever it carries no userinfo, so a
/// deployment without Ollama auth cannot tell this function ran. That also
/// avoids the parser's own normalisation (`http://h:1` would otherwise come back
/// as `http://h:1/`) on the overwhelmingly common credential-free case: the diff
/// in a log is invisible until there is something to hide.
///
/// ## Why the parser and not a split on `@` or `:`
///
/// `user:pass@host` is not separable from its text, and the obvious reading of
/// the string is the one that leaks:
///
/// - A raw `@` inside a password is legal input and is *not* a delimiter. The
///   boundary is the **last** raw `@` in the authority; earlier ones belong to
///   the userinfo and come back percent-encoded. Given `http://user:p@ss@host`,
///   `url::Url` yields user `user`, password `p%40ss` and host `host`. Splitting
///   on the **first** `@` — the "everything before the `@` is the credential"
///   reading — redacts only `user` and publishes `p@ss@host:11434` to the log: a
///   password fragment, which is the failure mode that actually matters.
/// - Conversely, a percent-encoded `%40` is never a boundary, so a splitter that
///   looks for a literal `@` is right for `p%40ss` and wrong for `p@ss` — the
///   same code, two opposite outcomes, and nothing in the string says which case
///   it is in.
/// - `split_once(':')` to find a port finds the userinfo separator instead,
///   because `user:pass` *is* that separator; and `/` and `%` inside a password
///   are `%2F` and `%25`, so the authority terminator is equally ambiguous.
/// - Multibyte userinfo is percent-encoded (`usér` → `us%C3%A9r`), so byte-index
///   arithmetic on the raw string lands mid-codepoint.
///
/// `reqwest::Url` is a re-export of `url::Url` and already applies every one of
/// those rules, so the only decision left is *which side of the boundary is the
/// credential* — and that is exactly what `username()` / `password()` answer,
/// because they hand back the userinfo as one field with no delimiter to get
/// wrong. Redacting the whole field cannot leak a fragment of it. The textual
/// backstop below reproduces the same last-`@` rule, but the parser is what
/// makes that rule correct rather than lucky.
///
/// ## Input the parser cannot help with
///
/// Two classes of input do not go down the authoritative path, and both fall to
/// the same conservative textual backstop rather than being passed through
/// unchanged:
///
/// 1. **Unparseable input.** `Url::parse` rejects a surprising amount of what
///    operators actually type — `""`, `http://h:99999` (port out of range),
///    `http://` (empty host) — and being unparseable says nothing about whether
///    the string contains a password; a mistyped `BRAIN_OLLAMA_URL` is precisely
///    the value that ends up in a log.
/// 2. **Parsed, but with no authority to strip.** `user:pass@host/api` — what an
///    operator gets by forgetting the scheme on the way out of a
///    `curl -u user:pass host` — *parses*: scheme `user`, `cannot-be-a-base`,
///    `has_authority() == false`. So `username()` and `password()` both report
///    "no userinfo" while the credential sits in the `Url`'s **path**, verbatim.
///    Trusting the parser's "there is nothing here" on a URL it could not give
///    an authority for is the leak; this function only trusts it where an
///    authority exists.
///
/// The backstop redacts the `[scheme]://…@` span — or, with no `://`, the span
/// up to the first `/`, `?` or `#` — ending at the **last** raw `@` before that
/// boundary, which is the only split that cannot leave a password fragment
/// behind. With no `@` before the boundary, there is nothing to redact and the
/// input is returned verbatim.
pub fn redact_url(u: &str) -> String {
    match reqwest::Url::parse(u) {
        // Authoritative path: redact only when there is something to redact, so
        // the credential-free case never picks up the parser's normalisation.
        Ok(mut parsed) => {
            if !parsed.has_authority() {
                // No authority ⇒ no userinfo field for the parser to have
                // emptied, so "no userinfo" here means "no place to look", not
                // "nothing to hide". Backstop, which can see the raw `@`.
                return redact_url_textually(u);
            }
            if !has_userinfo(&parsed) {
                return u.to_string();
            }
            set_redacted_userinfo(&mut parsed);
            parsed.to_string()
        }
        Err(_) => redact_url_textually(u),
    }
}

/// Strips the credential out of a `reqwest::Error`'s embedded URL, in place.
///
/// Needed because `reqwest`'s own `Display` interpolates the request URL into
/// every message it builds — `error.rs` ends with `write!(f, " for url ({url})")`
/// — so an operator who printed `{e}` never formatted the URL by hand and the
/// credential still reached the log. `Error::url_mut` exists for this and is
/// documented by reqwest as "useful if you need to remove sensitive information
/// from the URL".
///
/// Redacting the field in place rather than rebuilding the string keeps reqwest's
/// own wording, `Kind`, status and source chain byte-for-byte; reconstructing them
/// here would duplicate reqwest's message format and drift the day reqwest
/// rewords it. A `None` URL — a client-build failure, which never reaches the
/// request path — is left alone because there is nothing in it to redact.
///
/// ## This is a primitive, not the thing to call
///
/// In-place redaction is a **parser** operation, so it can only express a
/// credential that the parser put in an authority. Two shapes defeat it:
///
/// - A URL with **no authority**: `user:pass@host/api` parses as scheme `user`
///   with the credential in the path, so there is no userinfo to empty, and
///   `redact_url`'s answer for it is text that no longer parses as an absolute
///   URL — so it cannot be written back through `url_mut` either. The `Url` value
///   is a faithful copy of a secret and there is no way to say so through it.
/// - A redirect target, where reqwest attaches a URL it never stripped, which is
///   what makes the error carry a credential at all (this one *is* fixable in
///   place, and is the case this function was written for).
///
/// So the in-place pass is kept as-is and [`redacted_error_message`] — which runs
/// it and then closes what it cannot — is what every sink in this crate prints.
/// Call this directly only when you are mutating an error you are going to render
/// with `Debug` on a `Url` the in-place pass *did* fix.
pub fn redact_reqwest_error(e: &mut reqwest::Error) {
    let Some(url) = e.url_mut() else { return };
    if !has_userinfo(url) {
        return;
    }
    set_redacted_userinfo(url);
}

/// The one function in this crate that turns a `reqwest::Error` into something
/// safe to print. Every request site in the crate goes through it, and there is
/// exactly one call to [`redact_reqwest_error`] — so the next sink cannot
/// reintroduce the gap by forgetting a step, only by not calling this.
///
/// Two mechanisms, in order, because neither is total:
///
/// 1. **In place**, via [`redact_reqwest_error`]. This is what actually fixes the
///    common case — a redirect target whose `Location` carried a credential that
///    reqwest never stripped — and it keeps reqwest's own wording, `Kind`, status
///    and source chain exactly as the shipped version renders them.
/// 2. **The rendered string**, for the shapes in-place redaction cannot
///    represent. A `Url` with no authority holds the credential in its path, and
///    its redacted form is not an absolute URL, so there is no `Url` value to
///    substitute. The URL `reqwest` interpolated is then swapped for
///    [`redact_url`]'s answer, keying on reqwest's own `Display` of it — which is
///    the exact text the needle has to be, since a normalised re-serialisation
///    would not match and would silently leave the string alone.
///
/// An error with no URL, or whose URL carries nothing to redact, is returned as
/// reqwest rendered it.
pub fn redacted_error_message(e: reqwest::Error) -> String {
    let mut e = e;
    redact_reqwest_error(&mut e);
    let rendered = e.to_string();
    let Some(url) = e.url() else { return rendered };
    // Keyed on `Display`, not `as_str`: this is the form `reqwest`'s own message
    // embedded, so a needle taken from anywhere else could fail to match and
    // return the unredacted string in a way no assertion would notice.
    let needle = url.to_string();
    let safe = redact_url(&needle);
    if safe == needle { rendered } else { rendered.replace(&needle, &safe) }
}

/// Whether a parsed URL carries any userinfo, i.e. whether there is a credential
/// to hide. A bare `user@host` counts: it is not secret, but it is an account
/// name, and `http://user:pass@` → `http://***@` is the least surprising shape
/// for a reader of the log.
fn has_userinfo(parsed: &reqwest::Url) -> bool {
    !parsed.username().is_empty() || parsed.password().is_some()
}

/// Replaces the whole userinfo with [`REDACTED`], keeping host, port and path.
///
/// `set_username` / `set_password` only fail on a URL that cannot be a base, and
/// the caller has already established an authority (a URL with a non-empty
/// username or a password always has one), so the `Err` arm is unreachable
/// rather than merely unlikely — hence the deliberate `let _`.
fn set_redacted_userinfo(parsed: &mut reqwest::Url) {
    if !parsed.has_authority() {
        return;
    }
    let _ = parsed.set_username(REDACTED);
    let _ = parsed.set_password(None);
}

/// Backstop for input [`reqwest::Url`] could not give an authority for. See
/// [`redact_url`].
///
/// `authority_start` is the byte the authority begins at — just past `://`, or
/// `0` when the operator left the scheme off — and `authority_end` the first
/// `/`, `?` or `#` at or after it, which is the same terminator either way: with
/// no scheme the authority still ends at the first path, query or fragment
/// separator, so `user:pass@host/api` bounds the credential at `user:pass@host`
/// and leaves `/api` alone.
fn redact_url_textually(u: &str) -> String {
    let (authority_start, authority_end) = match u.find("://") {
        Some(scheme_end) => {
            let start = scheme_end + "://".len();
            let end = u[start..].find(['/', '?', '#']).map_or(u.len(), |i| start + i);
            (start, end)
        }
        // No `://` at all. The scheme-less reading is the dangerous one to get
        // wrong here: `user:pass@host/api` has a credential and no scheme, and
        // the paragraph above this function used to call that "nothing to
        // redact" — the value a `curl -u` line produces when the operator drops
        // the `http://` on the way into the env var.
        None => (0, u.find(['/', '?', '#']).unwrap_or(u.len())),
    };
    let authority = &u[authority_start..authority_end];
    // Last raw `@`, not the first: see `redact_url` for why the first can leave a
    // password fragment in the log. Rebased onto `u`, because `rfind` counts from
    // the start of `authority` and the two index spaces differ by `authority_start`.
    // The slice starts *at* the `@`, not after it, so this path emits the same
    // `http://***@host` shape the parser path does — a marker that reads as a
    // placeholder rather than as a mangled hostname.
    let Some(at) = authority.rfind('@').map(|i| authority_start + i) else {
        return u.to_string();
    };
    format!("{}{REDACTED}{}", &u[..authority_start], &u[at..])
}

pub struct EmbeddingEngine {
    pub base_url: String,
    pub model: String,
    pub timeout_secs: u64,
}

impl EmbeddingEngine {
    /// Explicit constructor. Trailing `/` characters are trimmed so that
    /// `"http://x:11434///"` and `"http://x:11434"` behave identically.
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    /// Builds an engine from the environment, honouring `BRAIN_OLLAMA_URL` and
    /// `BRAIN_OLLAMA_MODEL` (see `AGENTS.md`). Unset *or blank* variables fall
    /// back to [`DEFAULT_BASE_URL`] / [`DEFAULT_MODEL`], so a misconfigured
    /// empty export degrades to the documented defaults instead of producing an
    /// unusable relative URL.
    ///
    /// This is the constructor every binary and tool handler should use; it
    /// exists so the env contract lives in exactly one place.
    pub fn from_env() -> Self {
        Self::from_options(
            std::env::var("BRAIN_OLLAMA_URL").ok(),
            std::env::var("BRAIN_OLLAMA_MODEL").ok(),
        )
    }

    /// Env-independent core of [`EmbeddingEngine::from_env`]: `None` and blank
    /// values for either field resolve to the corresponding default.
    pub fn from_options(base_url: Option<String>, model: Option<String>) -> Self {
        let base_url = base_url
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let model = model
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        Self::new(base_url, model)
    }

    fn client(&self, timeout_secs: u64) -> Result<reqwest::Client> {
        // Routed through the same wrapper as the requests themselves. A build
        // failure has no URL today, so this is free — and it means a `reqwest`
        // error in this crate has exactly one way to become a string, which is
        // the property that stops the next sink from inventing a second.
        reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| anyhow!("failed to build reqwest client: {}", redacted_error_message(e)))
    }

    /// Wall-clock budget for one batch of `n_texts`, from [`embed_timeout_secs`].
    ///
    /// `budget = base x min(ceil(n / effective_parallelism), BATCH_WAVE_CAP)`
    ///
    /// The division used to be by [`BATCH_CONCURRENCY`] (4) on the assumption
    /// that four requests really are in flight. They are not: `OLLAMA_NUM_PARALLEL`
    /// defaults to 1, so a 32-chunk batch really takes 32 sequential round-trips
    /// and the old formula budgeted a quarter of what it needed. Every such batch
    /// hit its ceiling mid-flight and came back as a wall of `NULL` chunks — a
    /// silent loss of the whole vector half of the index. Dividing by
    /// [`effective_parallelism`] instead makes the budget match the wall clock.
    ///
    /// The cap is the other half of the fix: the formula is linear, so an
    /// unbounded wave count turns a large batch into an unbounded timeout
    /// (16.7 hours for 1,000 chunks at the default base). See
    /// [`BATCH_WAVE_CAP`] for why 8.
    ///
    /// An empty batch still gets one wave, so the budget is never zero.
    pub fn batch_timeout(&self, n_texts: usize) -> Duration {
        let waves = n_texts.div_ceil(effective_parallelism()).clamp(1, BATCH_WAVE_CAP) as u64;
        Duration::from_secs(embed_timeout_secs().saturating_mul(waves))
    }

    /// Waves [`batch_timeout`] budgets for. Exposed so the formula is testable
    /// without reaching into a `Duration`, and so a caller can log why a batch
    /// was abandoned.
    pub fn batch_waves(&self, n_texts: usize) -> usize {
        n_texts.div_ceil(effective_parallelism()).clamp(1, BATCH_WAVE_CAP)
    }

    /// Liveness probe. Never panics: an unreachable, misconfigured or erroring
    /// Ollama is reported as `false` so the caller can fall back to FTS-only.
    pub async fn health_check(&self) -> bool {
        let client = match self.client(HEALTH_TIMEOUT_SECS) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("brain-embed: health_check client build failed: {e:#}");
                return false;
            }
        };
        match client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => true,
            Ok(resp) => {
                eprintln!("brain-embed: ollama unhealthy: status {}", resp.status());
                false
            }
            Err(e) => {
                // `redacted_error_message`, not a bare `{e}`: `reqwest`'s
                // `Display` appends the request URL to every message it builds,
                // so the credential in `BRAIN_OLLAMA_URL` reaches this line even
                // though nobody formatted the URL by hand. The redirect path is
                // the one that really carries it — a normal request has its
                // userinfo moved into an `Authorization` header and stripped
                // from the `Url` before the error is built, so `Error::url()` is
                // already clean there. Host and port stay, which is what makes
                // an unreachable Ollama diagnosable at all.
                eprintln!("brain-embed: ollama unreachable: {}", redacted_error_message(e));
                false
            }
        }
    }

    /// Embeds a single non-blank text into a `EMBEDDING_DIM` vector.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        if text.trim().is_empty() {
            bail!("empty text");
        }
        let client = self.client(self.timeout_secs)?;
        let url = format!("{}/api/embeddings", self.base_url);
        let resp = client
            .post(&url)
            .json(&serde_json::json!({"model": self.model, "prompt": text}))
            .send()
            .await
            // Two URLs, and both are named, and neither may carry the credential.
            //
            // The first is this crate's own: it belongs in the message because
            // callers degrade to FTS-only on this error, so a silent fallback is
            // impossible to diagnose without it. Only the userinfo goes; host,
            // port and endpoint path all stay.
            //
            // The second is `reqwest`'s: it interpolates the request URL into its
            // `Display` on its own, so it arrives here as the error's **source**
            // and `{e:#}` — the whole chain, which is what
            // `brain-mcp`'s query-embed fallback prints to stderr on every
            // `brain_search` whose embed fails — carried it. Redacting the URL
            // this crate formats and leaving the source alone redacted the
            // operator-facing half and leaked the other half. The redirect path
            // is what makes that visible, and it is the same shape
            // `health_check` was already handling.
            //
            // The source is therefore rendered by [`redacted_error_message`] and
            // baked into the message, rather than kept as a `reqwest::Error`.
            // Nothing downcasts on it — `grep -rn "downcast.*reqwest" crates/`
            // is empty — so the chain's only consumer is the text, and this
            // makes the text and the source the same string instead of two that
            // can disagree.
            .map_err(|e| {
                anyhow!(
                    "ollama request to {} failed: {}",
                    redact_url(&url),
                    redacted_error_message(e)
                )
            })?;
        if !resp.status().is_success() {
            bail!("ollama {}", resp.status());
        }
        let v: serde_json::Value = resp.json().await.context("invalid ollama JSON")?;
        let arr = v
            .get("embedding")
            .and_then(|e| e.as_array())
            .ok_or_else(|| anyhow::anyhow!("missing embedding"))?;
        // Non-numeric entries (null, strings, booleans) degrade to 0.0 instead of
        // failing the whole batch; the dimension check below is the real guard.
        let vec: Vec<f32> = arr
            .iter()
            .map(|x| x.as_f64().unwrap_or(0.0) as f32)
            .collect();
        if vec.len() != EMBEDDING_DIM {
            bail!("dim mismatch {} vs {}", vec.len(), EMBEDDING_DIM);
        }
        // An all-zero vector is a failed embed, not an embedding. `cosine` in
        // brain-store scores a zero-norm vector 0.0 against everything, so
        // accepting one here would reproduce the exact production bug (735 of 738
        // chunks dead) from the server side instead of the caller side — and this
        // response is a legitimate 200 with the right width, so the dimension
        // check above cannot catch it.
        if vec.iter().all(|f| *f == 0.0) {
            bail!("ollama returned an all-zero embedding (a failed embed, not a vector)");
        }
        Ok(vec)
    }

    /// Embeds many texts with at most [`BATCH_CONCURRENCY`] requests in flight,
    /// tolerating per-text failures.
    ///
    /// Output order always matches input order regardless of response ordering.
    /// A text that fails — HTTP error, malformed body, wrong dimension, blank
    /// input — yields `None` at its position while every other text still
    /// succeeds. This is the entry point every note-writing path should use: a
    /// single un-embeddable chunk must not cost the whole note its semantic
    /// index, and `None` is the honest answer where the strict variant's `Err`
    /// used to force callers into writing a zero vector.
    ///
    /// Never panics and never returns a zero vector: an unreachable Ollama
    /// produces an all-`None` vector, which callers store as SQL `NULL`.
    pub async fn embed_batch_partial(&self, texts: Vec<String>) -> Vec<Option<Vec<f32>>> {
        self.drain(texts)
            .await
            .into_iter()
            .map(|(_, res)| res.ok())
            .collect()
    }

    /// Strict batch variant: succeeds only if *every* text embeds.
    ///
    /// Kept for callers that treat a partial index as a failure. Prefer
    /// [`EmbeddingEngine::embed_batch_partial`] when degrading to `NULL` per
    /// chunk is acceptable, which is the case for every note-writing path.
    pub async fn embed_batch_concurrent(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::new();
        for (idx, res) in self.drain(texts).await {
            out.push(res.map_err(|e| anyhow::anyhow!("chunk {idx}: {e}"))?);
        }
        Ok(out)
    }

    /// Shared fan-out: one request per text, capped at [`BATCH_CONCURRENCY`] in
    /// flight, results returned in input order as `(index, Result)`.
    ///
    /// The accumulator is `Option`, never a zero-filled placeholder: a
    /// BLOB-of-zeros in `chunks.embedding` scores `0.0` for every query and then
    /// competes for the candidate budget in `search`, so a placeholder that
    /// survives to disk is indistinguishable from a real degradation.
    async fn drain(&self, texts: Vec<String>) -> Vec<(usize, Result<Vec<f32>>)> {
        use futures::stream::{FuturesUnordered, StreamExt};
        let n = texts.len();
        let mut futs = FuturesUnordered::new();
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(BATCH_CONCURRENCY));
        for (idx, txt) in texts.into_iter().enumerate() {
            let base = self.base_url.clone();
            let model = self.model.clone();
            let timeout_secs = self.timeout_secs;
            let sem = sem.clone();
            futs.push(async move {
                // The semaphore is only ever dropped, never closed, so `acquire`
                // cannot fail; handle it defensively instead of unwrapping.
                let Ok(_permit) = sem.acquire_owned().await else {
                    return (idx, Err(anyhow::anyhow!("embedding semaphore closed")));
                };
                let eng = EmbeddingEngine {
                    base_url: base,
                    model,
                    timeout_secs,
                };
                (idx, eng.embed(&txt).await)
            });
        }
        let mut results: Vec<(usize, Result<Vec<f32>>)> = Vec::with_capacity(n);
        while let Some(r) = futs.next().await {
            // Draining continues past a failure on purpose: partial tolerance is
            // the whole point of `embed_batch_partial`, and aborting here would
            // leave the remaining futures unpolled.
            results.push(r);
        }
        results.sort_by_key(|(i, _)| *i);
        results
    }
}
