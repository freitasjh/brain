#!/usr/bin/env python3
"""Minimal MCP SSE client (stdlib only): initialize -> tools/list -> tools/call.

Two things here are load-bearing and were both wrong at some point; the reasons are
kept inline because the next person to "simplify" them reintroduces a 3-5% flake.

1. The stream is read with `readline()`, not `read(1)`.

   The original read **one byte per iteration** and then did `while b"\\n" in buf` on
   the accumulated buffer, so every byte re-scanned the buffer from the start: O(n^2)
   in the length of a single SSE line. Measured on this machine, draining 3 events:

       payload   read(1)     readline()
         2 KiB     11 ms        3 ms
        32 KiB    226 ms        7 ms
       128 KiB   1495 ms       18 ms
       512 KiB  21376 ms       85 ms

   At 512 KiB the old loop alone exceeds the 15 s window by 7 s. SSE is
   line-oriented and `readline()` is the natural read for that, so the cost is
   linear and the same parsing rule applies.

   `resp.fp.readline()` reads the *raw* stream, so chunk-size lines
   (`"1f4"\\r\\n`) come back interleaved with the payload. They are harmless: a
   chunk header is its own line and can never begin with `data: `, which is the
   only prefix that produces an event. A `data:` line split across two chunks is
   still reassembled, because `readline()` scans the byte stream for `\\n` and does
   not care where chunk boundaries fall.

2. The wait window must be **longer than the server's own bound** on the call it is
   waiting for.

   `brain_search` embeds the query synchronously before answering
   (`brain_mcp::rmcp_service::embed_query`). That embed is bounded by
   `brain_embed`'s per-request socket timeout, `DEFAULT_TIMEOUT_SECS` = 30 s. A
   15 s window is therefore shorter than the server's worst case by 2x: whenever the
   embed lands in the 15-30 s band the client asserts failure while the server is
   still working exactly as designed, and the response arrives 15+ s too late to be
   seen. Nothing is lost in transit; the client simply gave up first.

   The default below is derived from that number with a margin, and
   `mcp_window_exceeds_server_bound` in the Rust suite asserts the relationship, so
   the inversion cannot come back unnoticed. The window is overridable because the
   test that *demonstrates* the mechanism needs it deliberately short.
"""
import http.client
import json
import os
import queue
import sys
import threading
import time
import urllib.parse

# brain_embed::DEFAULT_TIMEOUT_SECS — the per-request socket timeout that bounds a
# single query embed. Kept as a literal because this file is stdlib-only and must
# not import the Rust crate; the Rust side asserts the two stay consistent.
SERVER_EMBED_BOUND_SECS = 30

# Client window: the server's bound plus margin. The margin covers the round trip
# and the SSE hop, and is deliberately generous rather than tight — this number is
# only ever *reached* on a real failure, because a healthy local server answers in
# single-digit milliseconds.
DEFAULT_WAIT_SECS = SERVER_EMBED_BOUND_SECS + 30


def wait_secs(argv):
    """Window override: argv[2], else BRAIN_E2E_WAIT_SECS, else the default."""
    if len(argv) > 2:
        return float(argv[2])
    env = os.environ.get("BRAIN_E2E_WAIT_SECS")
    if env:
        try:
            return float(env)
        except ValueError:
            pass
    return float(DEFAULT_WAIT_SECS)


def sse_reader(base, path, out_q, stop):
    parsed = urllib.parse.urlparse(base)
    conn = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=30)
    conn.request("GET", path, headers={"Accept": "text/event-stream"})
    resp = conn.getresponse()
    while not stop.is_set():
        # See module docstring (1): linear, and chunk headers fall through as
        # non-`data:` lines.
        raw = resp.fp.readline()
        if not raw:
            break
        line = raw.strip()
        if line.startswith(b"data: "):
            out_q.put(line[6:].decode())
    conn.close()


def rpc(base, endpoint, payload):
    parsed = urllib.parse.urlparse(base)
    body = json.dumps(payload).encode()
    conn = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=15)
    conn.request("POST", endpoint, body=body,
                 headers={"Content-Type": "application/json", "Accept": "application/json, text/event-stream"})
    resp = conn.getresponse()
    data = resp.read()
    conn.close()
    return resp.status, data


def wait_for(out_q, id_want, timeout):
    deadline = time.time() + timeout
    stash = []
    while time.time() < deadline:
        try:
            msg = out_q.get(timeout=0.5)
        except queue.Empty:
            continue
        try:
            v = json.loads(msg)
        except Exception:
            continue
        if v.get("id") == id_want:
            return v
        stash.append(v)
    raise AssertionError(f"no response id={id_want}, saw: {stash[:3]}")


def main():
    base = sys.argv[1]  # e.g. http://localhost:18342
    window = wait_secs(sys.argv)
    out_q: queue.Queue = queue.Queue()
    stop = threading.Event()
    t = threading.Thread(target=sse_reader, args=(base, "/sse", out_q, stop), daemon=True)
    t.start()
    endpoint = out_q.get(timeout=window)
    assert "sessionId=" in endpoint, f"no endpoint: {endpoint}"
    rpc(base, endpoint, {"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "e2e", "version": "0"}}})
    init = wait_for(out_q, 1, window)
    rpc(base, endpoint, {"jsonrpc": "2.0", "method": "notifications/initialized"})
    rpc(base, endpoint, {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    tools = wait_for(out_q, 2, window)
    rpc(base, endpoint, {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "brain_search", "arguments": {"query": "smoke"}}})
    call = wait_for(out_q, 3, window)
    stop.set()
    print(json.dumps({"init": init, "tools": tools, "call": call}))
    names = [t["name"] for t in tools["result"]["tools"]]
    assert "brain_search" in names and "brain_store" in names, names
    assert "e2e/sv" in json.dumps(call), json.dumps(call)[:300]
    print("MCP-HANDSHAKE-OK")


if __name__ == "__main__":
    main()
