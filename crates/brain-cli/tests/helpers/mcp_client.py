#!/usr/bin/env python3
"""Minimal MCP SSE client (stdlib only): initialize -> tools/list -> tools/call."""
import http.client
import json
import queue
import sys
import threading
import urllib.parse


def sse_reader(base, path, out_q, stop):
    parsed = urllib.parse.urlparse(base)
    conn = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=30)
    conn.request("GET", path, headers={"Accept": "text/event-stream"})
    resp = conn.getresponse()
    buf = b""
    while not stop.is_set():
        chunk = resp.read(1)
        if not chunk:
            break
        buf += chunk
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            line = line.strip()
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


def wait_for(out_q, id_want, timeout=15):
    import time
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
    out_q: queue.Queue = queue.Queue()
    stop = threading.Event()
    t = threading.Thread(target=sse_reader, args=(base, "/sse", out_q, stop), daemon=True)
    t.start()
    endpoint = out_q.get(timeout=15)
    assert "sessionId=" in endpoint, f"no endpoint: {endpoint}"
    rpc(base, endpoint, {"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "e2e", "version": "0"}}})
    init = wait_for(out_q, 1)
    rpc(base, endpoint, {"jsonrpc": "2.0", "method": "notifications/initialized"})
    rpc(base, endpoint, {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    tools = wait_for(out_q, 2)
    rpc(base, endpoint, {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "brain_search", "arguments": {"query": "smoke"}}})
    call = wait_for(out_q, 3)
    stop.set()
    print(json.dumps({"init": init, "tools": tools, "call": call}))
    names = [t["name"] for t in tools["result"]["tools"]]
    assert "brain_search" in names and "brain_store" in names, names
    assert "e2e/sv" in json.dumps(call), json.dumps(call)[:300]
    print("MCP-HANDSHAKE-OK")


main()
