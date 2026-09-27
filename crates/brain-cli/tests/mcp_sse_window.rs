//! Why the `cli_e2e` MCP handshake used to flake, asserted instead of assumed.
//!
//! # The symptom
//!
//! `e2e_serve_viewer_and_mcp_coexist` and
//! `a_squatter_on_the_legacy_fixed_ports_does_not_affect_the_coexistence_test`
//! failed a few percent of full-suite runs with
//!
//! ```text
//! AssertionError: no response id=3, saw: []
//! ```
//!
//! from `tests/helpers/mcp_client.py`. Not reproducible in isolation; lost only
//! when the whole suite ran, because the suite's test binaries run concurrently.
//!
//! # The mechanism
//!
//! `brain_search` embeds the query **synchronously** before it answers —
//! `brain_mcp::rmcp_service::embed_query` — and that embed is bounded by
//! `brain_embed`'s per-request socket timeout, **30 s** (`DEFAULT_TIMEOUT_SECS`).
//! The helper's `wait_for` gave up at **15 s**. The client's window was therefore
//! half the server's own worst case for the exact call it was waiting on, so any
//! embed landing in the 15-30 s band failed the test while the server was still
//! working exactly as designed. The response is never lost in transit; the client
//! stops listening first, and `saw: []` is the signature — the stream drained
//! fine, there was simply nothing in it yet.
//!
//! What put embeds in that band: `Server::spawn` used to `env_remove`
//! `BRAIN_OLLAMA_URL`, which does not disable the embed, it selects the **default**
//! Ollama at `http://localhost:11434` — the one real model on the host, shared by
//! every concurrently running test binary, each of which also embeds inline on
//! `store`. Measured here: the `tools/call` round trip is 0.141 s idle and
//! **8.899 s** with the suite running, against a 15 s window. With a dead Ollama
//! the same handshake is 0.006-0.020 s under identical load and the tail is gone.
//!
//! # Why there are four tests and not one
//!
//! A single "the suite is green now" test cannot tell a real fix from a lucky run —
//! the flake was 3-5%, so a green run is weak evidence and nobody could find the
//! bug from it. These pin each link separately, and one of them **fails on
//! purpose** so the mechanism stays legible to the next person:
//!
//! * `a_short_window_fails_while_the_server_is_still_working` — the flake, on
//!   demand, with no load and no timing luck.
//! * `a_window_longer_than_the_slow_embed_receives_the_response` — the guarantee:
//!   the same slow server, a long enough window, and the response arrives intact.
//! * `the_client_window_outlives_the_servers_own_bound_on_a_query_embed` — the
//!   structural guard, so the 15 s / 30 s inversion cannot silently return.
//! * `the_sse_reader_drains_a_large_event` — the other defect found while
//!   diagnosing, which was *not* this flake but would have become one.
//!
//! Nothing here reads `data/brain.db`, binds 8321/8322, or reaches a real Ollama:
//! the embed backend is a local mock on an ephemeral port.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const EMBEDDING_DIM: usize = 768;

/// `tests/helpers/mcp_client.py`.
fn helper() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/helpers/mcp_client.py")
}

fn brain_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_brain"))
}

fn free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let p = l.local_addr().unwrap().port();
    drop(l);
    p
}

// ---------------------------------------------------------------- slow Ollama

/// An Ollama stand-in whose `/api/embeddings` answers after a fixed delay.
///
/// This is the shape of a *saturated* real Ollama, which is what the tests used to
/// contend for: accepted, then slow. A refused connection (what `DEAD_OLLAMA` gives)
/// is the opposite shape and returns instantly, which is why pointing the suite at
/// a dead backend removed the tail instead of hiding it.
struct SlowOllama {
    port: u16,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl SlowOllama {
    fn start(delay: Duration) -> SlowOllama {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind mock ollama");
        let port = l.local_addr().unwrap().port();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let s = stop.clone();
        let handle = std::thread::spawn(move || {
            for conn in l.incoming() {
                if s.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = conn else { break };
                let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
                let mut line = String::new();
                // Request line, then headers, then a body sized by Content-Length.
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let is_embed = line.contains("/api/embeddings");
                let mut content_length = 0usize;
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) == 0 {
                        break;
                    }
                    if h.trim().is_empty() {
                        break;
                    }
                    let lower = h.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        content_length = v.trim().parse().unwrap_or(0);
                    }
                }
                if content_length > 0 {
                    let mut body = vec![0u8; content_length];
                    let _ = reader.read_exact(&mut body);
                }
                if is_embed {
                    std::thread::sleep(delay);
                    // A real, non-degenerate vector: brain_embed rejects an all-zero
                    // embedding outright, and a dim mismatch is an error.
                    let v: Vec<String> = (0..EMBEDDING_DIM)
                        .map(|i| format!("{:.6}", (i % 17) as f32 * 0.01 + 0.01))
                        .collect();
                    let body = format!("{{\"embedding\":[{}]}}", v.join(","));
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                } else {
                    // `/api/tags` and anything else: answer at once.
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
                    );
                }
                let _ = stream.flush();
            }
        });
        SlowOllama { port, stop, handle: Some(handle) }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for SlowOllama {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        // Unblock the accept loop so the thread can be joined instead of leaked.
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port)).map(|_| ());
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

// ------------------------------------------------------------------ the server

struct Fixture {
    dir: PathBuf,
    child: Child,
    port: u16,
    outlog: PathBuf,
}

impl Fixture {
    /// A `brain serve-mcp` whose embed backend answers after `delay`.
    fn spawn(tag: &str, ollama_url: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("brain-window-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("export")).unwrap();
        let port = free_port();
        let outlog = dir.join("serve.out");
        let mut cmd = Command::new(brain_bin());
        cmd.arg("--db")
            .arg(dir.join("brain.db"))
            .arg("serve-mcp")
            .arg("--port")
            .arg(port.to_string())
            .env("BRAIN_OLLAMA_URL", ollama_url)
            .env("BRAIN_EXPORT_ROOT", dir.join("export"))
            .env_remove("BRAIN_TRANSPORT")
            .stdout(Stdio::from(std::fs::File::create(&outlog).unwrap()))
            .stderr(Stdio::from(std::fs::File::create(dir.join("serve.err")).unwrap()));
        let child = cmd.spawn().expect("spawn serve-mcp");
        Fixture { dir, child, port, outlog }
    }

    /// Waits for the post-bind announcement, then seeds one note to search for.
    fn ready(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        let want = format!(":{}/sse", self.port);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                panic!("serve-mcp exited early:\n{}", self.diagnostics());
            }
            let out = std::fs::read_to_string(&self.outlog).unwrap_or_default();
            if out.contains(&want) && std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok()
            {
                let db = self.dir.join("brain.db");
                let o = Command::new(brain_bin())
                    .arg("--db")
                    .arg(&db)
                    .env("BRAIN_OLLAMA_URL", "http://127.0.0.1:1")
                    .args(["store", "regras", "e2e/sv", "## serve smoke", "--scope", "global"])
                    .output()
                    .expect("seed store");
                assert!(o.status.success(), "seed store failed: {}", String::from_utf8_lossy(&o.stderr));
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("serve-mcp never announced {want}:\n{}", self.diagnostics());
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn diagnostics(&self) -> String {
        let e = std::fs::read_to_string(self.dir.join("serve.err")).unwrap_or_default();
        let o = std::fs::read_to_string(&self.outlog).unwrap_or_default();
        format!("stdout:\n{o}\nstderr:\n{e}")
    }

    /// The helper's own exit status and combined output, with `window_secs` forced.
    fn handshake(&self, window_secs: f64) -> std::process::Output {
        Command::new("python3")
            .arg(helper())
            .arg(format!("http://127.0.0.1:{}", self.port))
            .arg(window_secs.to_string())
            .output()
            .expect("run mcp_client.py")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn combined(o: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A 3 s embed delay, and a window well under it.
///
/// **This test asserts a failure.** That is the point: it reproduces the reported
/// flake on demand, with no load, so the mechanism is inspectable rather than
/// folklore. It fails if the helper ever becomes immune to a short window, which
/// is the signal to go and re-read *why* before deleting it.
#[test]
#[cfg(unix)]
fn a_short_window_fails_while_the_server_is_still_working() {
    let ollama = SlowOllama::start(Duration::from_secs(3));
    let mut f = Fixture::spawn("shortwindow", &ollama.url());
    f.ready();

    let out = f.handshake(1.0);
    let text = combined(&out);

    // 1. The exact reported symptom.
    assert!(
        text.contains("no response id=3"),
        "expected the short-window failure; got:\n{text}"
    );
    // 2. And the part that makes it a *client* failure and not a server one: the
    //    server is still up, still inside its 30 s budget, and about to answer.
    //    Without this the test would also pass against a dead server, which would
    //    make it a test of nothing.
    assert!(
        f.alive(),
        "the server died, so this is not the flake being demonstrated:\n{}",
        f.diagnostics()
    );
    // 3. The embed really was still in flight, rather than the server having
    //    answered and the response being lost in transit.
    let err = std::fs::read_to_string(f.dir.join("serve.err")).unwrap_or_default();
    assert!(
        !err.contains("query not embedded") && !err.contains("query embedding timed out"),
        "the server gave up on the embed early, so the window was not the cause:\n{err}"
    );
}

/// The same slow server, a window longer than the delay: the response arrives.
///
/// This is the guarantee the old window could not give. It is the half that says
/// the fix is not "the flake got rarer" — the response was never lost, and with a
/// window that outlives the server's own bound it cannot be.
#[test]
#[cfg(unix)]
fn a_window_longer_than_the_slow_embed_receives_the_response() {
    let ollama = SlowOllama::start(Duration::from_secs(3));
    let mut f = Fixture::spawn("longwindow", &ollama.url());
    f.ready();

    let out = f.handshake(30.0);
    let text = combined(&out);
    assert!(
        text.contains("MCP-HANDSHAKE-OK"),
        "a window longer than the embed must still receive it; got:\n{text}\n{}",
        f.diagnostics()
    );
    // The result is a real one, not an empty success: the note seeded before the
    // handshake is in the payload.
    assert!(text.contains("e2e/sv"), "the search result came back empty:\n{text}");
}

/// The structural guard: the client's default window outlives the server's own
/// bound on the call it waits for.
///
/// The bound is the *smaller* of the two nested limits on one query embed — the
/// per-request socket timeout and the batch budget — because whichever fires first
/// is what the client has to outlast. Both come from the live engine, not from
/// literals, so this tracks the code rather than restating it.
///
/// The helper's side is read by *importing* it and asking for the value it will
/// actually use, rather than by re-deriving it here. A Rust mirror of a Python
/// expression is a second copy that can drift in exactly the way this test exists
/// to catch.
#[test]
fn the_client_window_outlives_the_servers_own_bound_on_a_query_embed() {
    let eng = brain_embed::EmbeddingEngine::from_env();
    let per_request = eng.timeout_secs;
    let batch = eng.batch_timeout(1).as_secs();
    let server_bound = per_request.min(batch).max(1);

    // `mirrored` is the helper's copy of the per-request timeout; `window` is the
    // deadline it will really wait, with no override in the environment.
    let script = std::env::temp_dir().join(format!("brain-window-guard-{}.py", std::process::id()));
    std::fs::write(
        &script,
        format!(
            r#"
import importlib.util
spec = importlib.util.spec_from_file_location("mcp_client", {helper:?})
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
print(m.SERVER_EMBED_BOUND_SECS, m.wait_secs([]))
"#,
            helper = helper().display().to_string(),
        ),
    )
    .expect("write guard script");
    let out = Command::new("python3").arg(&script).output().expect("run guard script");
    let _ = std::fs::remove_file(&script);
    let text = combined(&out);
    assert!(out.status.success(), "guard script failed:\n{text}");
    let nums: Vec<f64> = text.split_whitespace().filter_map(|w| w.parse().ok()).collect();
    assert!(nums.len() == 2, "unexpected guard output:\n{text}");
    let (mirrored, window) = (nums[0], nums[1]);

    // The helper's mirror of `per_request` must not drift from the real constant:
    // the window is derived from it, so a silent edit on either side is exactly the
    // failure this file exists to prevent.
    assert_eq!(
        mirrored, per_request as f64,
        "mcp_client.py's SERVER_EMBED_BOUND_SECS ({mirrored}) has drifted from \
         brain_embed's per-request timeout ({per_request})"
    );
    assert!(
        window > server_bound as f64,
        "the client waits {window}s but the server may legitimately take {server_bound}s \
         (per-request {per_request}s, batch {batch}s) to answer a query embed: that \
         inversion is the flake"
    );
    assert!(
        window >= 2.0 * server_bound as f64,
        "margin too thin: {window}s window against a {server_bound}s server bound"
    );
}

// ------------------------------------------------- the reader, which was O(n^2)

/// No fixture in this suite may fall through to a real embedding backend.
///
/// This one is textual, and deliberately so. The defect it pins — `Server::spawn`
/// calling `env_remove("BRAIN_OLLAMA_URL")`, which selects the **default**
/// `http://localhost:11434` rather than disabling anything — cannot be caught by a
/// behavioural test on a machine that happens to have Ollama installed: both the
/// old and the new code reach *some* backend and both answer quickly, and making
/// `localhost:11434` hostile is not an option, because that is the developer's real
/// model. A guard that only fails on machines without Ollama is not a guard.
///
/// So this asserts the shape of the code instead: every server spawn names its
/// backend explicitly. Reintroducing the `env_remove` fails here, on every machine.
/// Drops `//` comments while respecting string literals.
///
/// Naively splitting each line on `//` is wrong in *both* directions, and both
/// happened here while this guard was being written: it flagged the fix's own
/// explanatory comment, and then truncated the very constant it searches for,
/// because `"http://127.0.0.1:1"` contains `//`. So: a `//` only starts a comment
/// when it is not inside a `"` string. Char literals are not tracked, because this
/// file has none that could contain a slash.
fn strip_line_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for n in chars.by_ref() {
                if n == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[test]
fn no_fixture_falls_through_to_a_real_embedding_backend() {
    let src = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cli_e2e.rs"),
    )
    .expect("read cli_e2e.rs");

    // Comments are stripped first, because the prose here *quotes* the old call —
    // that is the point of the comment above it — so a plain substring search
    // reports the fix itself as the defect.
    let code = strip_line_comments(&src);

    assert!(
        !code.contains("env_remove(\"BRAIN_OLLAMA_URL\")"),
        "cli_e2e.rs removes BRAIN_OLLAMA_URL, which selects the default Ollama \
         (http://localhost:11434) instead of disabling it — that is the shared \
         dependency behind the 3-5% handshake flake. Set it to DEAD_OLLAMA explicitly."
    );
    // And the default every spawn now inherits has to be the dead one. The URL is
    // matched on the declaration only, so a spawn pointing somewhere real is caught
    // by the first assertion rather than hiding behind this one.
    assert!(
        code.contains("const DEAD_OLLAMA: &str ="),
        "the DEAD_OLLAMA constant the spawn path depends on is gone or changed"
    );
    assert!(
        src.contains("const DEAD_OLLAMA: &str = \"http://127.0.0.1:1\";"),
        "DEAD_OLLAMA must stay a refused connection: a slow or real backend here is \
         exactly what put a 30s embed inside a 15s window"
    );
}

/// A chunked SSE server that emits `events` events of `event_bytes` each.
fn big_sse_server(events: usize, event_bytes: usize) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind sse");
    let port = l.local_addr().unwrap().port();
    let payload = "x".repeat(event_bytes);
    std::thread::spawn(move || {
        for conn in l.incoming().take(1) {
            let Ok(mut stream) = conn else { break };
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                    break;
                }
            }
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n"
            );
            for i in 0..events {
                let ev = format!(
                    "event: message\ndata: {{\"jsonrpc\":\"2.0\",\"id\":{i},\"result\":{{\"blob\":\"{payload}\"}}}}\n\n"
                );
                // Chunked, because that is what hyper sends, and because the
                // reader has to survive the interleaved chunk-size lines.
                let _ = write!(stream, "{:x}\r\n{ev}\r\n", ev.len());
                let _ = stream.flush();
            }
            let _ = write!(stream, "0\r\n\r\n");
            let _ = stream.flush();
        }
    });
    port
}

/// The SSE reader drains a large event; the old byte-at-a-time loop could not.
///
/// Measured on this machine, 3 events of 512 KiB: the old `read(1)` loop took
/// **21.4 s**, `readline()` takes **85 ms**. So this is not a micro-optimisation —
/// the old reader alone would blow a 15 s window on a result of that size, and a
/// `brain_search` against a real corpus returns far more than a test fixture does.
/// The bound here is set where the old implementation fails and the new one passes
/// with room to spare.
#[test]
fn the_sse_reader_drains_a_large_event() {
    const EVENTS: usize = 3;
    const EVENT_BYTES: usize = 512 * 1024;
    let port = big_sse_server(EVENTS, EVENT_BYTES);

    let script = std::env::temp_dir().join(format!("brain-sse-reader-{}.py", std::process::id()));
    std::fs::write(
        &script,
        format!(
            r#"
import importlib.util, queue, sys, threading, time
spec = importlib.util.spec_from_file_location("mcp_client", {helper:?})
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

q = queue.Queue()
stop = threading.Event()
t = threading.Thread(target=m.sse_reader, args=("http://127.0.0.1:{port}", "/sse", q, stop), daemon=True)
start = time.time()
t.start()
got = 0
while got < {EVENTS}:
    try:
        msg = q.get(timeout=60)
    except queue.Empty:
        break
    # The payload really arrived whole, not truncated: a `readline` that stopped at
    # a chunk boundary would hand back a short event.
    assert len(msg) > {EVENT_BYTES}, "event %d truncated: %d bytes" % (got, len(msg))
    got += 1
stop.set()
elapsed = time.time() - start
print("got", got, "elapsed", round(elapsed, 3))
assert got == {EVENTS}, "only got %d of {EVENTS} events" % got
assert elapsed < 5.0, "reader took %.1fs; the byte-at-a-time loop needed 21.4s" % elapsed
"#,
            helper = helper().display().to_string(),
        ),
    )
    .expect("write reader script");

    let out = Command::new("python3").arg(&script).output().expect("run reader script");
    let _ = std::fs::remove_file(&script);
    let text = combined(&out);
    assert!(out.status.success(), "reader script failed:\n{text}");
    assert!(text.contains(&format!("got {EVENTS}")), "{text}");
}
