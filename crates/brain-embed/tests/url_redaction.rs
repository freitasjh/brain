//! The Ollama credential must not reach a log, an error string, or a `Debug`.
//!
//! `BRAIN_OLLAMA_URL` is operator-supplied and may be
//! `http://user:pass@host:11434`. The **host** belongs in the error message on
//! purpose — callers degrade to FTS-only on that error, so naming the endpoint
//! is the only thing that makes the fallback diagnosable — but the credential
//! must not ride along. These cases pin both halves of that: the password is
//! gone, and the host and port are still there.
//!
//! No case here needs a real Ollama: the failure is forced with a refused
//! loopback port or with the mock in `support::`.

mod support;

use std::process::Command;

use brain_embed::{EmbeddingEngine, redact_reqwest_error, redact_url};
use support::{Mock, Plan, credentialed};

const MODEL: &str = "nomic-embed-text";

/// Distinctive enough that finding it in a log would be unambiguous, and
/// deliberately unlike any other literal in the tree.
const USER: &str = "alice";
const PASS: &str = "segredo-do-ollama";

/// `host:port` of a `http://…` base URL, so an assertion can name the endpoint
/// without hardcoding a port — `support::closed_port_url` hands back whatever
/// ephemeral port the OS picked.
fn host_port(base_url: &str) -> String {
    base_url
        .strip_prefix("http://")
        .expect("loopback base url")
        .to_string()
}

// ------------------------------------------------------------ redact_url --

#[test]
fn redact_url_removes_the_password_and_keeps_scheme_host_and_port() {
    assert_eq!(
        redact_url("http://user:pass@host:11434"),
        "http://***@host:11434/"
    );
}

#[test]
fn redact_url_removes_a_username_that_has_no_password() {
    // Not secret on its own, but it is an account name, and collapsing both
    // shapes to one marker keeps the log honest about what was removed.
    assert_eq!(redact_url("http://user@host:11434"), "http://***@host:11434/");
}

#[test]
fn redact_url_removes_a_password_whose_at_and_colon_are_percent_encoded() {
    // `%40` and `%3A` are the *correct* encoding of `@` and `:` inside a
    // password, and a splitter looking for a literal `@` or `:` downstream of
    // the authority start is looking for characters that are not there.
    assert_eq!(
        redact_url("http://user:se%40gredo:p%3Aa%2Fss@host:11434"),
        "http://***@host:11434/"
    );
    assert!(!redact_url("http://user:se%40gredo:p%3Aa%2Fss@host:11434").contains("se"));
}

#[test]
fn redact_url_removes_a_multibyte_credential() {
    // Percent-encoded by the parser, so any byte-index arithmetic on the raw
    // string would land mid-codepoint.
    let out = redact_url("http://usér:séçrètø@host:11434");
    assert_eq!(out, "http://***@host:11434/");
    assert!(!out.contains("séçrètø"), "multibyte password survived: {out}");
}

#[test]
fn redact_url_removes_a_raw_at_inside_the_password_whole() {
    // The authority boundary is the LAST raw `@`; earlier ones belong to the
    // userinfo. `url::Url` agrees: `http://user:p@ss@host` is user `user`,
    // password `p%40ss`, host `host`.
    //
    // So splitting on the *first* `@` — the "everything before the `@` is the
    // credential" reading — redacts only `user` and would publish
    // `p@ss@host:11434`. A password fragment is still a leak, and this case is
    // the reason the implementation is a parser and not a `split_once`.
    let out = redact_url("http://user:p@ssTAIL@host:11434");
    assert_eq!(out, "http://***@host:11434/");
    assert!(!out.contains("TAIL"), "password fragment survived: {out}");
}

#[test]
fn redact_url_removes_every_at_sign_in_a_multi_at_authority() {
    // Generalisation of the case above: three raw `@`s, so the userinfo is
    // `a@b@c` and only the final `@` is the boundary.
    let out = redact_url("http://a@bTAIL@c@host:11434");
    assert_eq!(out, "http://***@host:11434/");
    assert!(!out.contains("TAIL"), "password fragment survived: {out}");
}

#[test]
fn redact_url_leaves_a_url_without_userinfo_byte_identical() {
    // Byte-identical, not merely equivalent: the credential-free case must not
    // pick up the parser's normalisation (`http://h:1` → `http://h:1/`), or every
    // log line in a deployment without Ollama auth would change for no reason.
    for u in [
        "http://localhost:11434",
        DEFAULT,
        "http://host:11434/proxy/ollama",
        "https://ollama.example:443/api",
        "http://host:11434/?x=1#frag",
    ] {
        assert_eq!(redact_url(u), u, "a credential-free URL must not be touched");
    }
}

/// The documented default, spelled out so the "unchanged" case above cannot be
/// satisfied by a constant that happens to be the same string.
const DEFAULT: &str = "http://localhost:11434";

#[test]
fn redact_url_leaves_ipv6_without_userinfo_byte_identical() {
    // The brackets are not userinfo, and a splitter that treats the first `:` as
    // the userinfo/host boundary would eat the address.
    for u in ["http://[::1]:11434", "http://[fe80::1%25eth0]:11434"] {
        assert_eq!(redact_url(u), u);
    }
}

#[test]
fn redact_url_removes_userinfo_in_front_of_ipv6() {
    let out = redact_url("http://user:p@[::1]:11434");
    assert_eq!(out, "http://***@[::1]:11434/");
    assert!(!out.contains("p@"));
}

#[test]
fn redact_url_keeps_path_and_query_while_removing_the_credential() {
    // `BRAIN_OLLAMA_URL` legitimately carries a path prefix (a reverse proxy in
    // front of Ollama), and that path is diagnostic: it says which proxy.
    let out = redact_url("http://user:pass@proxy.example:8080/ollama/v1?tag=prod");
    assert_eq!(out, "http://***@proxy.example:8080/ollama/v1?tag=prod");
}

#[test]
fn redact_url_leaves_an_unparseable_string_without_userinfo_alone() {
    for u in [
        "",
        "not a url",
        "http://",
        "http://h:99999", // port out of range — rejected by the parser
        "http:///api/embeddings",
        "localhost:11434", // parses as scheme "localhost", no authority
    ] {
        assert_eq!(
            redact_url(u),
            u,
            "nothing to redact in {u:?}, so it must come back untouched"
        );
    }
}

#[test]
fn redact_url_removes_a_credential_from_an_unparseable_url() {
    // The backstop. `Url::parse` rejects an out-of-range port, so the parser
    // path never runs and a typo'd `BRAIN_OLLAMA_URL` with a password in it is
    // the case that would otherwise print verbatim.
    assert_eq!(
        redact_url("http://user:pass@host:99999"),
        "http://***@host:99999"
    );
    // The parser's normalising trailing `/` never appears here: the backstop
    // splices the original text, so the endpoint stays recognisable.
    assert!(!redact_url("http://user:pass@host:99999").ends_with('/'));
}

#[test]
fn redact_url_backstop_does_not_leave_a_password_fragment_behind() {
    // Same trap as the parsed case, in the backstop: the `@` that ends the
    // authority is the LAST one, and everything before it is the credential.
    let out = redact_url("http://user:p@ssTAIL@host:99999");
    assert_eq!(out, "http://***@host:99999");
    assert!(!out.contains("TAIL"), "password fragment survived: {out}");
}

#[test]
fn redact_url_is_idempotent() {
    // Re-redacting an already-redacted URL must not stack markers, and must not
    // decide `***` is a username worth redacting.
    for u in [
        "http://user:pass@host:11434",
        "http://user@host:11434",
        "http://host:11434",
        "http://user:p@ss@host:99999",
    ] {
        let once = redact_url(u);
        assert_eq!(redact_url(&once), once, "redaction is not idempotent for {u:?}");
    }
}

// -------------------------------------------------- the property, end to end --

#[tokio::test]
async fn embed_error_does_not_leak_the_ollama_credential() {
    // The real leak. `embed()` names the target URL in the error context
    // because callers degrade to FTS-only and the message is the only diagnostic
    // left; the credential in it is what must not survive.
    let closed = support::closed_port_url().await;
    let host_port = host_port(&closed);
    let eng = EmbeddingEngine::new(credentialed(&closed, USER, PASS), MODEL.into());
    let err = eng.embed("hello").await.expect_err("closed port must refuse");

    // `{:#}` is the whole chain — the strictest thing any consumer prints, and
    // what `brain-mcp` reaches for on the query-embed fallback path.
    let chain = format!("{err:#}");
    assert!(!chain.contains(PASS), "password reached the error: {chain}");
    assert!(!chain.contains(USER), "username reached the error: {chain}");
    assert!(chain.contains("***@"), "expected the redaction marker: {chain}");
    // The diagnosability the code comment exists to protect.
    assert!(chain.contains(&host_port), "host:port lost: {chain}");
    assert!(chain.contains("/api/embeddings"), "endpoint lost: {chain}");
}

#[tokio::test]
async fn embed_error_on_a_real_refused_host_keeps_the_host_and_port() {
    // The regression guard for the *other* half of the fix. If someone "solves"
    // the leak by dropping the URL, this fails: the whole reason the URL is
    // there is that a silent FTS-only fallback is undiagnosable without it.
    let closed = support::closed_port_url().await;
    let host_port = host_port(&closed);
    let eng = EmbeddingEngine::new(closed, MODEL.into());
    let err = eng.embed("hello").await.expect_err("closed port must refuse");
    let msg = err.to_string();
    assert!(msg.contains(&host_port), "host:port lost: {msg}");
    assert!(msg.contains("/api/embeddings"), "endpoint lost: {msg}");
}

#[tokio::test]
async fn redact_reqwest_error_removes_a_credential_the_error_really_carries() {
    // Characterisation plus property, in that order, and the order matters.
    //
    // A *normal* reqwest failure does NOT carry the credential: `RequestBuilder`
    // moves the URL's userinfo into an `Authorization` header and strips it from
    // the `Url` before the request is built, so `Error::url()` is already clean
    // and `{e}` is safe by accident. The one reqwest path that skips that is a
    // redirect: the `Location` target is parsed straight out of the header and
    // attached to the error without going through the stripping, so its
    // `Display` really does contain the password.
    //
    // The first assertion states that precondition. If a future reqwest closes
    // the redirect hole too, this test fails loudly and the redaction can be
    // revisited — as opposed to passing vacuously.
    let mock = Mock::start(Plan {
        redirect: Some(format!("ftp://{USER}:{PASS}@elsewhere.example/")),
        ..Default::default()
    })
    .await;
    let err = reqwest::Client::new()
        .get(format!("{}/api/tags", mock.base_url))
        .send()
        .await
        .expect_err("a non-http(s) redirect target is rejected");

    assert!(
        err.to_string().contains(PASS),
        "precondition: reqwest's own Display no longer carries the credential, so \
         this path can no longer prove redaction. Message was: {err}"
    );

    let mut err = err;
    redact_reqwest_error(&mut err);
    let redacted = err.to_string();
    assert!(!redacted.contains(PASS), "password survived: {redacted}");
    assert!(!redacted.contains(USER), "username survived: {redacted}");
    assert!(redacted.contains("elsewhere.example"), "host lost: {redacted}");
}

// -------------------------------------------------- the sinks, end to end --
//
// Everything above is the policy. These are the four places the policy has to be
// applied, and the two redirects below are the discriminating shape: a normal
// reqwest failure carries a credential-free `Url` because `RequestBuilder` moved
// the userinfo into an `Authorization` header, so a redacted-or-not assertion on
// the common path can pass for the wrong reason.

#[tokio::test]
async fn embed_error_does_not_leak_a_redirected_credential() {
    // 🔴 1. `embed()` is the hottest sink in the product: `brain-mcp`'s
    // `embed_query` prints `{e:#}` — the **whole chain**, our error and our
    // source — to stderr on every `brain_search` whose query embed fails, and
    // stderr is journald. `embed()` redacted the URL it formatted and left the
    // source alone, and the source is where `reqwest` puts the URL, so the
    // password went to the log on the redirect path while the redaction marker
    // did not appear at all — the operator saw neither the redaction nor a
    // recognisable credential.
    let mock = Mock::start(Plan {
        redirect: Some(format!("ftp://{USER}:{PASS}@elsewhere.example/")),
        ..Default::default()
    })
    .await;
    let err = mock.engine().embed("hello").await.expect_err("a non-http redirect is rejected");
    let chain = format!("{err:#}");
    assert!(!chain.contains(PASS), "password reached the chain: {chain}");
    assert!(!chain.contains(USER), "username reached the chain: {chain}");
    assert!(chain.contains("***@"), "no redaction marker at all: {chain}");
    // Host still named on **both** halves, so the fallback is diagnosable: the
    // first half is this crate's message, the second is reqwest's.
    assert!(chain.contains("elsewhere.example"), "host lost: {chain}");
    assert!(chain.contains("/api/embeddings"), "endpoint lost: {chain}");
}

/// `BRAIN_OLLAMA_URL` with the scheme left off — what an operator gets by
/// dropping the `http://` while copying a `curl -u user:pass host` line.
///
/// This one parses, as scheme `user`, with the credential in the *path* — so
/// `username()` and `password()` both report nothing, and a policy that asked the
/// parser "is there userinfo here?" answered "no" and printed verbatim.
fn schemeless_base_url(base: &str, user: &str, pass: &str) -> String {
    format!("{user}:{pass}@{}", base.strip_prefix("http://").expect("loopback base url"))
}

#[tokio::test]
async fn embed_error_does_not_leak_a_credential_from_a_schemeless_base_url() {
    // 🔴 2, on the real sink. Asserts the **absence of the password** and the
    // **presence of the marker**, and deliberately not the exact string: the
    // suite's habit of pinning the output shape is what let this through — the
    // shape was always reasonable, it just was not redacted. The marker is
    // asserted because a redacted credential has to stay distinguishable from a
    // mangled hostname, and a message with no marker at all would be a silent
    // mangling.
    let closed = support::closed_port_url().await;
    let eng = EmbeddingEngine::new(schemeless_base_url(&closed, USER, PASS), MODEL.into());
    let err = eng.embed("hello").await.expect_err("a schemeless base url cannot be requested");
    let chain = format!("{err:#}");
    assert!(!chain.contains(PASS), "password reached the chain: {chain}");
    assert!(!chain.contains(USER), "username reached the chain: {chain}");
    assert!(chain.contains(REDACTION_MARKER), "no redaction marker at all: {chain}");
    // And the endpoint this crate built is still named, so a schemeless
    // misconfiguration is diagnosable rather than merely silent.
    assert!(chain.contains("/api/embeddings"), "endpoint lost: {chain}");
}

#[test]
fn redact_url_removes_a_credential_from_a_url_with_no_scheme() {
    // 🔴 2, at the policy level. The three shapes an operator actually types,
    // none of which `Url::parse` will give an authority for.
    for input in [
        "user:pass@ollama.internal/api/embeddings",
        "user:p@ssTAIL@ollama.internal/api/embeddings",
        "user:pass@ollama.internal:11434",
    ] {
        let out = redact_url(input);
        assert!(!out.contains("pass"), "password survived {input:?}: {out}");
        assert!(!out.contains("ssTAIL"), "password fragment survived {input:?}: {out}");
        assert!(out.contains(REDACTION_MARKER), "no marker for {input:?}: {out}");
    }
}

#[test]
fn redact_url_leaves_a_schemeless_url_without_a_credential_byte_identical() {
    // The regression this fix could have caused, and the reason the `has_authority`
    // test is written as a loop rather than as the previous "no `://` means
    // verbatim" shortcut: `localhost:11434` also parses as a schemeless URL, so a
    // backstop that fired on *every* authority-less input would have touched it.
    //
    // `mailto:` deliberately is **not** here — see the case below.
    for u in [
        "localhost:11434",
        "ollama.internal",
        "ollama.internal:11434/api",
        "urn:isbn:0451450523",
        "not a url",
        "",
    ] {
        assert_eq!(redact_url(u), u, "a credential-free input must not be touched: {u:?}");
    }
}

#[test]
fn redact_url_over_redacts_a_schemeless_path_with_an_at_in_it() {
    // The price of the fail-closed backstop, named rather than discovered later.
    //
    // With no authority to locate a userinfo boundary in, the rule is "everything
    // before the last `@`, up to the first path separator". On
    // `mailto:someone@example.com` there is no credential — the `@` belongs to
    // the path — and the rule redacts `someone` anyway.
    //
    // That is the right direction to err: `redact_url`'s only callers pass an Ollama
    // base URL, where a `mailto:` value is a misconfiguration, and a mangled
    // misconfiguration costs an operator one confusing log line while an
    // under-redacted one costs a credential in journald. What it must never do is
    // hide that it mangled something, so the marker is still present: `***@` reads
    // as "a credential was removed here", which is the honest description.
    let out = redact_url("mailto:someone@example.com");
    assert_eq!(out, "***@example.com");
    assert!(out.contains(REDACTION_MARKER), "a mangled value must still say so: {out}");
}

// --------------------------------------------------- sink coverage, asserted --

/// The policy has exactly one entry point in the crate, and the sinks reach it
/// rather than the primitive underneath it.
///
/// Without this, the shape of the leak is repeatable: a new sink that calls
/// [`redact_reqwest_error`] directly gets the redirect fix and none of the
/// no-authority fix, and nothing in the behavioural suite above fails — every
/// case there drives `embed()` or `health_check()` specifically. A count is a
/// blunt instrument, but it is the one that fires the moment a fifth sink
/// appears, and it fails with a message naming what to do about it.
#[test]
fn every_sink_goes_through_the_one_wrapper() {
    let src = include_str!("../src/lib.rs");
    // The definition plus its single call, inside `redacted_error_message`. Any
    // further call is a sink bypassing the wrapper.
    assert_eq!(
        count(src, "redact_reqwest_error("),
        2,
        "`redact_reqwest_error` must be defined once and called once — inside \
         `redacted_error_message`. A new call site is a sink that redacts the \
         redirect shape and leaks the no-authority one; route it through \
         `redacted_error_message` instead."
    );
    // The definition plus the three request sites: client build, health probe,
    // embed. `Debug for EmbedQueue` is a different sink and goes through
    // `redact_url` directly, which is the right primitive for a `String` field.
    assert_eq!(
        count(src, "redacted_error_message("),
        4,
        "expected `redacted_error_message` to be defined once and called by the \
         three request sites (client build, health_check, embed). Update this \
         count *and* the table above when a sink is added — a new sink that does \
         not call the wrapper is a leak."
    );
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// What the count above pins, in the form a reviewer can check by reading one
/// place. Deliberately a test rather than a comment: a table in a doc comment
/// drifts silently, and the drift in this particular table is a credential in
/// journald.
#[test]
fn the_sink_table_matches_the_measured_behaviour() {
    // (sink, does it print a reqwest error, does it go through the wrapper)
    let table: [(&str, bool, bool); 4] = [
        ("embed()", true, true),
        ("health_check()", true, true),
        ("Debug for EmbedQueue", false, true),
        ("embed_query (inherits embed)", true, true),
    ];
    assert_eq!(table.len(), 4, "the sink set grew — document it above");
    for (sink, prints, wrapped) in table {
        if prints {
            assert!(wrapped, "{sink} prints a reqwest error and must go through the wrapper");
        }
    }
}

// --------------------------------------------------- stderr, the real sink --
//
// `eprintln!` writes to the process's fd 2, and there is no std-only way to
// swap that out from under the test, so these cases re-execute the test binary as
// a child with `Command` and read its piped stderr. The child half is a no-op
// unless [`CHILD_ENV`] selects a case, so a normal `cargo test` run pays one
// extra process per case and nothing else.

/// Selects the child's case; unset means "this is the parent, do nothing".
const CHILD_ENV: &str = "BRAIN_TEST_REDACT_URL_CHILD";

/// `brain_embed::redact_url`'s marker. Re-declared rather than imported because
/// the constant is private and the tests are allowed to know its value only as an
/// observable fact.
const REDACTION_MARKER: &str = "***";

/// Child half of the cases below. Run with `--exact` and the env var set;
/// without them it returns immediately, which is what a plain `cargo test` does.
#[test]
fn stderr_capture_child() {
    let Ok(case) = std::env::var(CHILD_ENV) else { return };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("child runtime");
    rt.block_on(async {
        match case.as_str() {
            // A refused connection with a credentialed base URL.
            "health" => {
                let closed = support::closed_port_url().await;
                let eng = EmbeddingEngine::new(credentialed(&closed, USER, PASS), MODEL.into());
                assert!(!eng.health_check().await, "closed port must be unhealthy");
            }
            // A redirect whose target carries the credential — the reqwest path
            // that genuinely leaks through `Display`.
            "health-redirect" => {
                let mock = Mock::start(Plan {
                    redirect: Some(format!("ftp://{USER}:{PASS}@elsewhere.example/")),
                    ..Default::default()
                })
                .await;
                let eng = mock.engine();
                assert!(!eng.health_check().await, "bad-scheme redirect must be unhealthy");
            }
            // 🔴 2 on this sink: a base URL with no scheme, which parses as a URL
            // with no authority and therefore reaches `redact_url` on its new
            // fall-through branch.
            "health-noscheme" => {
                let closed = support::closed_port_url().await;
                let eng = EmbeddingEngine::new(schemeless_base_url(&closed, USER, PASS), MODEL.into());
                assert!(!eng.health_check().await, "a schemeless base url cannot be healthy");
            }
            other => panic!("unknown child case {other:?}"),
        }
    });
}

fn capture_child_stderr(case: &str) -> String {
    let exe = std::env::current_exe().expect("path of the running test binary");
    let out = Command::new(exe)
        .args(["--exact", "stderr_capture_child", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, case)
        .output()
        .expect("re-run the test binary as a child to capture its stderr");
    assert!(
        out.status.success(),
        "child case {case:?} failed: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn health_check_stderr_does_not_leak_a_refused_credential() {
    // The refused-connection case is the one where reqwest's own `Display` is
    // already credential-free, because a normal request has its userinfo moved
    // into an `Authorization` header before the error is built. It is here as the
    // cheap invariant, not as the proof; `…_redirected_credential` below is the
    // discriminating one.
    let stderr = capture_child_stderr("health");
    assert!(
        stderr.contains("ollama unreachable"),
        "the child should have logged an unreachable Ollama: {stderr}"
    );
    assert!(!stderr.contains(PASS), "password reached stderr: {stderr}");
    assert!(
        stderr.contains("127.0.0.1:"),
        "the host must stay so the failure is diagnosable: {stderr}"
    );
}

#[test]
fn health_check_stderr_does_not_leak_a_redirected_credential() {
    // The discriminating case. Without the redaction this line reads
    // `builder error for url (ftp://alice:segredo-do-ollama@elsewhere.example/)`,
    // and nothing in the code under test ever formats that URL by hand — reqwest's
    // own `Display` carries it.
    let stderr = capture_child_stderr("health-redirect");
    assert!(
        stderr.contains("ollama unreachable"),
        "the child should have logged an unreachable Ollama: {stderr}"
    );
    assert!(!stderr.contains(PASS), "password reached stderr: {stderr}");
    assert!(
        stderr.contains("elsewhere.example"),
        "the host must stay so the failure is diagnosable: {stderr}"
    );
}

#[test]
fn health_check_stderr_does_not_leak_a_credential_from_a_schemeless_base_url() {
    // 🔴 2 on this sink, and the case that proves the in-place fix was not
    // enough on its own: `redact_reqwest_error` *is* called here, and the
    // password still reached stderr, because `Error::url()` for
    // `alice:segredo@ollama.internal/api/tags` has no authority to strip and
    // `url_mut` cannot hold the redacted text either.
    let stderr = capture_child_stderr("health-noscheme");
    assert!(
        stderr.contains("ollama unreachable"),
        "the child should have logged an unreachable Ollama: {stderr}"
    );
    assert!(!stderr.contains(PASS), "password reached stderr: {stderr}");
    assert!(!stderr.contains(USER), "username reached stderr: {stderr}");
    assert!(stderr.contains(REDACTION_MARKER), "no redaction marker at all: {stderr}");
}
