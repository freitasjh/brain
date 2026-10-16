//! Streamable HTTP MCP transport (`POST /mcp`) — the wire protocol, end to end.
//!
//! Why this file exists. `/mcp/<tool>` in `crate::mcp_router_with_queue` is a REST
//! shim: one HTTP call per tool, no JSON-RPC envelope, no `initialize`. It answers
//! 200 to anyone, so a check that "the endpoint responds HTTP" passes on a
//! transport no MCP client can use. opencode V2 (`type: "remote"`) speaks
//! Streamable HTTP and *only* POSTs, and pointed at `/sse` it was measured failing
//! with `Error POSTing to endpoint (HTTP 405)`. So the property that matters is
//! not "answers HTTP" — it is **`initialize` → `notifications/initialized` →
//! `tools/list` → `tools/call` completing and returning a `pong`**, and that is
//! what every test here asserts.
//!
//! The client is hand-written HTTP on purpose. An SDK client would prove that the
//! server satisfies whatever version the SDK negotiates; this proves the server
//! satisfies the protocol as it appears on the wire — headers, status codes, SSE
//! framing included — which is the layer the opencode failure was in.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use brain_embed::EmbeddingEngine;
use brain_mcp::EmbedQueue;
use brain_store::Store;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Two budgets, for two different failures. A reply that takes longer than this is
/// a server that stopped serving, and hanging the suite would report that as a
/// timeout nobody can read rather than as an assertion failure.
const REPLY_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 1 MiB: `brain_status` on a large corpus plus the header block.
const MAX_REPLY: usize = 1 << 20;

// ---------------------------------------------------------------- fixtures --

fn tmp_db(tag: &str) -> String {
    let db = format!("/tmp/brain-streamable-{}-{tag}.db", std::process::id());
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{db}{suffix}"));
    }
    Store::open(&db).expect("create the schema through the store, as any real database has");
    let _ = std::fs::remove_file(format!("{db}-wal"));
    let _ = std::fs::remove_file(format!("{db}-shm"));
    db
}

/// A queue pointed at a port nobody is listening on.
///
/// The embedding queue is *shared* with the MCP service on purpose (that sharing is
/// the point of the design), and a dead engine is what keeps this test from firing
/// real embedding requests at whatever Ollama the machine is running.
fn dead_queue() -> Arc<EmbedQueue> {
    Arc::new(EmbedQueue::new(EmbeddingEngine::new(
        "http://127.0.0.1:1".into(),
        "nomic-embed-text".into(),
    )))
}

// ------------------------------------------------------------ http plumbing --

/// One HTTP response, read far enough to parse a JSON-RPC message out of it.
struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The JSON-RPC message, unwrapped from the SSE frame the transport sends it in.
    fn json(&self) -> serde_json::Value {
        let mut payload: Option<String> = None;
        for frame in self.body.split("\n\n") {
            for line in frame.lines() {
                if let Some(data) = line.strip_prefix("data:") {
                    // Continuation frames carry no message of their own (`: ping`),
                    // and the first frame that does is the answer to the request.
                    payload = Some(data.trim().to_string());
                    break;
                }
            }
            if payload.is_some() {
                break;
            }
        }
        let text = payload.unwrap_or_else(|| panic!("no `data:` frame in the response body: {:?}", self.body));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("response frame is not JSON ({e}): {text}"))
    }
}

/// Reads into `buf` until it holds `need` bytes, or the stream ends first.
///
/// Returns `false` for EOF, which is the only reason a read here stops short of
/// `need`. Every caller turns that into an assertion, because a truncated reply is
/// the failure worth naming.
async fn fill(stream: &mut TcpStream, buf: &mut Vec<u8>, need: usize) -> std::io::Result<bool> {
    let mut chunk = [0u8; 8192];
    while buf.len() < need {
        let n = tokio::time::timeout(REPLY_TIMEOUT, stream.read(&mut chunk))
            .await
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("timed out with {} of {} bytes buffered", buf.len(), need),
                )
            })??;
        if n == 0 {
            return Ok(false);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(true)
}

/// Finds the first occurrence of `needle`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    assert!(!needle.is_empty(), "searching for an empty pattern matches at 0, which is not a result");
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// One request/response exchange on a **fresh** connection.
///
/// Fresh every time, because a Streamable HTTP response to a request is an SSE
/// stream that stays open for server-initiated messages: there is no end-of-body
/// to read, so a second response could never be framed out of the same socket.
struct Client {
    addr: SocketAddr,
}

impl Client {
    fn new(addr: SocketAddr) -> Self {
        Client { addr }
    }

    /// Sends one request and reads one response.
    async fn send(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> Reply {
        let mut stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(self.addr))
            .await
            .expect("connect timed out")
            .unwrap_or_else(|e| panic!("connect to {} failed: {e}", self.addr));
        stream
            .set_nodelay(true)
            .expect("nodelay: a round trip is 39ms without it");

        let mut req = format!("{method} {path} HTTP/1.1\r\nhost: {}\r\n", self.addr);
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        match body {
            Some(b) => req.push_str(&format!("content-length: {}\r\n\r\n{}", b.len(), b)),
            None => req.push_str("content-length: 0\r\n\r\n"),
        }
        tokio::time::timeout(REPLY_TIMEOUT, stream.write_all(req.as_bytes()))
            .await
            .expect("write timed out")
            .expect("write the request");
        let reply = read_reply(&mut stream).await;
        drop(stream);
        reply
    }

    /// `POST` with the two headers a Streamable HTTP client must send, plus the
    /// session id once `initialize` has handed one back.
    async fn mcp_post(&self, path: &str, session: Option<&str>, body: &str) -> Reply {
        let mut headers: Vec<(&str, &str)> = vec![
            ("content-type", "application/json"),
            // Both, and not just JSON: the transport answers with `text/event-stream`
            // and refuses a client that cannot accept it.
            ("accept", "application/json, text/event-stream"),
        ];
        if let Some(s) = session {
            headers.push(("mcp-session-id", s));
        }
        self.send("POST", path, &headers, Some(body)).await
    }
}

/// Reads one response: head, then body by `content-length`, by chunks, or to EOF.
async fn read_reply(stream: &mut TcpStream) -> Reply {
    let mut buf: Vec<u8> = Vec::new();
    let mut have = 0usize;
    fill(stream, &mut buf, 1).await.expect("read the response head");
    while find(&buf, b"\r\n\r\n").is_none() {
        assert!(have < MAX_REPLY, "response head exceeds {MAX_REPLY} bytes");
        have += 1;
        assert!(
            fill(stream, &mut buf, have).await.expect("read the response head"),
            "the connection closed before the head ended: {:?}",
            String::from_utf8_lossy(&buf)
        );
    }
    let split = find(&buf, b"\r\n\r\n").expect("head terminator, checked just above");
    let head = String::from_utf8_lossy(&buf[..split]).into_owned();
    buf.drain(..split + 4);

    let mut lines = head.lines();
    let status_line = lines.next().expect("status line");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .expect("status code")
        .parse()
        .expect("a numeric status code");
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let header = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());

    let body = match header("content-length").and_then(|v| v.parse::<usize>().ok()) {
        Some(n) => {
            let complete = fill(stream, &mut buf, n).await.expect("read the body");
            assert!(complete, "the connection closed {} bytes into a {n}-byte body", buf.len());
            let raw: Vec<u8> = buf.drain(..n).collect();
            String::from_utf8_lossy(&raw).into_owned()
        }
        None if header("transfer-encoding").is_some_and(|v| v.contains("chunked")) => {
            read_chunked(stream, &mut buf).await
        }
        // No framing at all: read to EOF, which for this transport is the connection
        // being closed by the peer.
        None => {
            let mut have = 0usize;
            while fill(stream, &mut buf, have).await.expect("read to EOF") {
                have += 1;
                assert!(have < MAX_REPLY, "an unframed body exceeds {MAX_REPLY} bytes");
            }
            String::from_utf8_lossy(&buf).into_owned()
        }
    };
    Reply { status, headers, body }
}

/// Reads chunked encoding, stopping at the end of the first SSE frame.
///
/// Stopping matters more than it looks. The frame carrying a tool result is the
/// *whole* reply for that request, but the body is a stream that stays open for
/// server-initiated messages, so the terminating `0` chunk never arrives while the
/// session is alive. The end of the first frame is therefore the correct stop, and
/// the connection is dropped right after.
async fn read_chunked(stream: &mut TcpStream, buf: &mut Vec<u8>) -> String {
    let mut out = String::new();
    loop {
        let size = read_chunk_line(stream, buf).await;
        let size = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16)
            .expect("a chunk size in hex");
        if size == 0 {
            break;
        }
        assert!(size + 2 < MAX_REPLY, "a single chunk of {size} bytes is not an MCP reply");
        let complete = fill(stream, buf, size + 2).await.expect("read a chunk");
        assert!(complete, "the connection closed inside a {size}-byte chunk");
        let chunk: Vec<u8> = buf.drain(..size).collect();
        buf.drain(..2); // the CRLF that ends every chunk
        out.push_str(&String::from_utf8_lossy(&chunk));
        // A `data:` line followed by a blank line is one complete SSE message.
        if out.contains("data:") && out.ends_with("\n\n") {
            break;
        }
        assert!(out.len() < MAX_REPLY, "the SSE frame exceeds {MAX_REPLY} bytes");
    }
    out
}

/// Reads one CRLF-terminated chunk-size line, consuming it.
async fn read_chunk_line(stream: &mut TcpStream, buf: &mut Vec<u8>) -> String {
    loop {
        if let Some(i) = find(buf, b"\r\n") {
            let line: Vec<u8> = buf.drain(..i).collect();
            buf.drain(..2);
            return String::from_utf8_lossy(&line).into_owned();
        }
        assert!(buf.len() < MAX_REPLY, "no line terminator in {} bytes", buf.len());
        let want = buf.len() + 1;
        assert!(
            fill(stream, buf, want).await.expect("read a chunk header"),
            "the connection closed where a chunk size was expected"
        );
    }
}

// ----------------------------------------------------------------- protocol --

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"brain-streamable-http-test","version":"0.0.0"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
const TOOLS_LIST: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
const PING_CALL: &str = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ping","arguments":{}}}"#;

/// Drives the full handshake the way an MCP client does, returning the last reply.
///
/// `notifications/initialized` is **not optional**, and that is measured here
/// rather than assumed. rmcp's `serve_server` waits for it as its next transport
/// message: a client that sends a request instead gets that request consumed by
/// the `expect_notification` wait, the initialize fails, and the session is closed
/// on the spot — the following call then answers `401 Session not found`. The MCP
/// spec requires the ack, so this is enforcement and not a defect, but the failure
/// mode is invisible from the client side (a clean 401) and it is why the ack sits
/// between `initialize` and everything else here.
async fn handshake(c: &Client, path: &str) -> (Reply /* initialize */, Reply /* tools/list */, Reply /* ping */) {
    let init = c.mcp_post(path, None, INITIALIZE).await;
    assert_eq!(init.status, 200, "initialize must be accepted: {:?}", init.body);
    let session = init
        .header("mcp-session-id")
        .expect("initialize must answer with an mcp-session-id header — the session is how every later call is routed");
    assert!(!session.is_empty(), "the session id must not be empty");
    // The protocol's ordering: the client's `notifications/initialized` is an
    // ack, not a request, so it is accepted (202) rather than answered.
    let ack = c.mcp_post(path, Some(session), INITIALIZED).await;
    assert_eq!(ack.status, 202, "notifications/initialized must be accepted: {:?}", ack.body);
    let tools = c.mcp_post(path, Some(session), TOOLS_LIST).await;
    let ping = c.mcp_post(path, Some(session), PING_CALL).await;
    (init, tools, ping)
}

// ------------------------------------------------------------------- tests --

/// The real MCP handshake, over a real socket, on the transport `serve-mcp` serves.
///
/// `POST /mcp` → `initialize` → `notifications/initialized` → `tools/list` →
/// `tools/call ping`. Everything up to and including the JSON-RPC envelope is on
/// the wire here: this is the test that fails when an endpoint merely answers 200.
#[tokio::test]
async fn the_streamable_http_transport_completes_a_real_mcp_handshake() {
    let db = tmp_db("handshake");
    let (addr, ct) = brain_mcp::rmcp_service::spawn_rmcp_app(db.clone(), dead_queue())
        .await
        .expect("the MCP app must bind and serve");
    let c = Client::new(addr);

    let (init, tools, ping) = handshake(&c, "/mcp").await;

    let iv = init.json();
    assert_eq!(iv["jsonrpc"], "2.0");
    assert_eq!(iv["id"], 1);
    // The protocol version the *server* negotiates back, which is what a client
    // compares against its own. 2024-11-05 was requested; anything the server
    // echoes is fine, and silence is not.
    assert!(
        iv["result"]["protocolVersion"].as_str().is_some_and(|v| !v.is_empty()),
        "initialize must negotiate a protocol version: {iv}"
    );
    assert!(
        iv["result"]["serverInfo"]["name"].as_str().is_some_and(|n| !n.is_empty()),
        "initialize must identify the server: {iv}"
    );

    let tv = tools.json();
    let names: Vec<String> = tv["result"]["tools"]
        .as_array()
        .expect("tools/list returns an array")
        .iter()
        .map(|t| t["name"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(tv["id"], 2);
    assert!(
        names.len() >= 17,
        "expected the full 17-tool registry, got {}: {names:?}",
        names.len()
    );
    for expected in [
        "ping",
        "brain_search",
        "brain_store",
        "brain_read",
        "brain_status",
        "brain_recent",
        "brain_delete",
        "brain_project_list",
    ] {
        assert!(names.iter().any(|n| n == expected), "the tool registry is missing {expected}: {names:?}");
    }

    // The end-to-end proof: a real tool call, through the real transport.
    let pv = ping.json();
    assert_eq!(pv["id"], 3);
    assert_eq!(pv["result"]["isError"], false, "ping is not an error: {pv}");
    let text = pv["result"]["content"][0]["text"]
        .as_str()
        .expect("a successful tool result carries text content");
    let inner: serde_json::Value = serde_json::from_str(text).expect("the tool's payload is JSON");
    assert_eq!(inner["pong"], true, "{inner}");

    ct.cancel();
    let _ = std::fs::remove_file(&db);
}

/// A session id is required, and a missing one must not take the server with it.
///
/// The 401 is the protocol refusing an unsessioned GET, not a crash: the assertion
/// that matters is the one *after* it — a fresh `initialize` on the same server
/// still completes. A panic inside the handler, or a poisoned session manager,
/// would show up here and nowhere else.
#[tokio::test]
async fn an_unsessioned_get_is_refused_and_the_server_keeps_serving() {
    let db = tmp_db("unsessioned");
    let (addr, ct) = brain_mcp::rmcp_service::spawn_rmcp_app(db.clone(), dead_queue())
        .await
        .expect("the MCP app must bind and serve");
    let c = Client::new(addr);

    let bad = c
        .send("GET", "/mcp", &[("accept", "text/event-stream")], None)
        .await;
    assert_eq!(
        bad.status, 401,
        "a GET with no mcp-session-id must be refused by the session manager, not crash: {:?}",
        bad.body
    );

    // And the server is still alive: a whole handshake on the same address.
    let (init, tools, ping) = handshake(&c, "/mcp").await;
    assert_eq!(init.json()["id"], 1);
    assert!(tools.json()["result"]["tools"].as_array().is_some_and(|a| !a.is_empty()));
    assert_eq!(ping.json()["result"]["isError"], false);

    ct.cancel();
    let _ = std::fs::remove_file(&db);
}

/// The legacy SSE transport still announces `event: endpoint`.
///
/// This is the regression guard for the constraint that `/sse` must not be
/// touched. An SSE client's first bytes *are* the endpoint announcement — without
/// it the client has no URL to POST back to and the session never starts — so it
/// is the cheapest complete check that the legacy transport is intact, and it
/// fails the moment the router composition drops or relocates the SSE route.
///
/// It asserts `/sse` **only**. The coexistence of the two transports is real and
/// is proved structurally — `spawn_rmcp_app` is one server on one address, and the
/// `/mcp` tests and this one connect to that same address — but mixing a `/mcp`
/// handshake in here would make a broken `/mcp` fail this test too, and a failure
/// that has two possible causes is a failure nobody can read.
#[tokio::test]
async fn the_legacy_sse_transport_still_announces_the_endpoint_event() {
    let db = tmp_db("sse");
    let (addr, ct) = brain_mcp::rmcp_service::spawn_rmcp_app(db.clone(), dead_queue())
        .await
        .expect("the MCP app must bind and serve");
    let c = Client::new(addr);

    let sse = c.send("GET", "/sse", &[("accept", "text/event-stream")], None).await;
    assert_eq!(sse.status, 200, "/sse must still answer GET: {:?}", sse.body);
    assert!(
        sse.body.contains("event: endpoint"),
        "the legacy handshake is the `endpoint` event; without it no SSE client can start: {:?}",
        sse.body
    );
    assert!(
        sse.body.contains("/message?sessionId="),
        "the endpoint event must carry the reply path and session: {:?}",
        sse.body
    );

    ct.cancel();
    let _ = std::fs::remove_file(&db);
}

/// `/mcp` must not swallow the `/mcp/<tool>` REST shims.
///
/// Nesting a service at `/mcp` registers a catch-all under it, so the hazard is
/// specific: `/mcp/ping` and `/mcp/status` are registered in the same router and
/// could silently lose resolution to the MCP service. That is invisible until a
/// caller gets a 404 or a JSON-RPC error where a REST body used to be, so it is
/// asserted directly — both routes, in the same server that answers `/mcp`.
#[tokio::test]
async fn the_rest_shims_below_mcp_are_not_shadowed_by_the_mcp_service() {
    let db = tmp_db("shims");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind an ephemeral port");
    let addr = listener.local_addr().expect("the listener has an address");
    let router = brain_mcp::mcp_router_with_queue(db, dead_queue());
    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve the debug router");
    });
    let c = Client::new(addr);

    // The MCP transport, on the router that also carries the shims.
    let (init, tools, ping) = handshake(&c, "/mcp").await;
    assert_eq!(init.json()["id"], 1);
    assert!(tools.json()["result"]["tools"].as_array().is_some_and(|a| a.len() >= 17));
    assert_eq!(ping.json()["result"]["isError"], false);

    // And the shims, still REST.
    let pong = c
        .send("POST", "/mcp/ping", &[("content-type", "application/json")], Some("{}"))
        .await;
    assert_eq!(pong.status, 200, "/mcp/ping must still be the REST shim: {:?}", pong.body);
    assert_eq!(pong.body.trim(), "pong");

    let status = c.send("GET", "/mcp/status", &[], None).await;
    assert_eq!(status.status, 200, "/mcp/status must still be the REST shim: {:?}", status.body);
    let sv: serde_json::Value = serde_json::from_slice(status.body.as_bytes()).expect("status is JSON");
    assert!(sv.get("notes").is_some(), "the REST status body, not an MCP envelope: {sv}");

    // The route one level up, untouched.
    let ping = c.send("GET", "/ping", &[], None).await;
    assert_eq!(ping.status, 200);
    assert_eq!(ping.body.trim(), "pong");
}
