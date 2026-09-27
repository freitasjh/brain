#!/usr/bin/env python3
"""MCP SSE probe: handshake, then one tools/call, printing the result as JSON.

Separate from `mcp_client.py` because that one asserts a *specific* note id
(`e2e/sv`) that belongs to the coexistence test's fixture, and because it takes
its timeout from a fixed default. This one takes the tool name, the arguments
and the timeout on the command line, so a test that is about the server's boot
path can assert on its own fixture without a second copy of the protocol
handshake drifting out of sync with the first.
"""
import http.client
import json
import queue
import sys
import threading
import time
import urllib.parse


def sse_reader(parsed, path, out_q, stop):
    conn = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=60)
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


def rpc(parsed, endpoint, payload):
    body = json.dumps(payload).encode()
    conn = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=30)
    conn.request("POST", endpoint, body=body,
                 headers={"Content-Type": "application/json",
                          "Accept": "application/json, text/event-stream"})
    resp = conn.getresponse()
    data = resp.read()
    conn.close()
    return resp.status, data


def wait_for(out_q, id_want, timeout, stash):
    deadline = time.time() + timeout
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
    return None


def main():
    base, tool, args_json = sys.argv[1], sys.argv[2], sys.argv[3]
    timeout = float(sys.argv[4]) if len(sys.argv) > 4 else 30.0
    parsed = urllib.parse.urlparse(base)
    out_q = queue.Queue()
    stop = threading.Event()
    threading.Thread(target=sse_reader, args=(parsed, "/sse", out_q, stop), daemon=True).start()

    try:
        endpoint = out_q.get(timeout=timeout)
    except queue.Empty:
        print(json.dumps({"error": "no SSE endpoint event"}))
        sys.exit(3)
    if "sessionId=" not in endpoint:
        print(json.dumps({"error": f"malformed endpoint event: {endpoint}"}))
        sys.exit(3)

    stash = []
    rpc(parsed, endpoint, {"jsonrpc": "2.0", "id": 1, "method": "initialize",
                           "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                                      "clientInfo": {"name": "server-start-e2e", "version": "0"}}})
    init = wait_for(out_q, 1, timeout, stash)
    if init is None:
        print(json.dumps({"error": "no initialize response", "saw": stash[:3]}))
        sys.exit(3)
    rpc(parsed, endpoint, {"jsonrpc": "2.0", "method": "notifications/initialized"})
    rpc(parsed, endpoint, {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    tools = wait_for(out_q, 2, timeout, stash)
    if tools is None:
        print(json.dumps({"error": "no tools/list response", "saw": stash[:3]}))
        sys.exit(3)
    rpc(parsed, endpoint, {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                           "params": {"name": tool, "arguments": json.loads(args_json)}})
    call = wait_for(out_q, 3, timeout, stash)
    stop.set()
    if call is None:
        print(json.dumps({"error": f"no tools/call response for {tool}", "saw": stash[:3]}))
        sys.exit(3)
    names = [t["name"] for t in tools["result"]["tools"]]
    print(json.dumps({"tool_names": names, "call": call}))


main()
