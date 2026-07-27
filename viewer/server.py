#!/usr/bin/env python3
"""viewer/server.py — REST API server for Brain Viewer HTML.

Provides HTTP endpoints for the web-based Brain Viewer. Shares core
components (vault, embeddings, index) with the MCP server.

Usage:
    uv run python viewer/server.py
    # Opens on http://localhost:8322 by default (configurable via BRAIN_VIEWER_PORT)
"""

from __future__ import annotations

import json
import logging
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "src"))

# uvicorn import — will be used lazily
logger = logging.getLogger("brain.viewer")

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

VIEWER_PORT = int(os.environ.get("BRAIN_VIEWER_PORT", "8322"))
VAULT_PATH = Path(os.environ.get("BRAIN_VAULT_PATH", "./vault"))
INDEX_PATH = Path(os.environ.get("BRAIN_INDEX_PATH", "./data/index.db"))
OLLAMA_URL = os.environ.get("BRAIN_OLLAMA_URL", "http://localhost:11434")
OLLAMA_MODEL = os.environ.get("BRAIN_OLLAMA_MODEL", "nomic-embed-text")
VIEWER_HOST = os.environ.get("BRAIN_VIEWER_HOST", "0.0.0.0")


# ---------------------------------------------------------------------------
# Core components (module-level singletons — initialized once at import)
# ---------------------------------------------------------------------------

def _init_components():
    """Import and return brain core components (lazy, on first request)."""
    from brain_server.vault.manager import VaultManager
    from brain_server.embeddings.engine import EmbeddingEngine
    from brain_server.index.store import VectorIndex

    _vault = VaultManager(VAULT_PATH)
    _embeddings = EmbeddingEngine(base_url=OLLAMA_URL, model=OLLAMA_MODEL)
    _index = VectorIndex(INDEX_PATH)
    _index.load()
    logger.info(
        "Viewer core initialized — vault=%s index=%d entries ollama=%s",
        VAULT_PATH, _index.size(), OLLAMA_URL,
    )
    return _vault, _embeddings, _index


_vault: "VaultManager | None" = None
_embeddings: "EmbeddingEngine | None" = None
_index: "VectorIndex | None" = None


def _get_vault():
    global _vault, _embeddings, _index
    if _vault is None:
        _vault, _embeddings, _index = _init_components()
    return _vault


def _get_embeddings():
    global _vault, _embeddings, _index
    if _embeddings is None:
        _vault, _embeddings, _index = _init_components()
    return _embeddings


def _get_index():
    global _vault, _embeddings, _index
    if _index is None:
        _vault, _embeddings, _index = _init_components()
    return _index


# ---------------------------------------------------------------------------
# Starlette app
# ---------------------------------------------------------------------------

async def app(scope, receive, send):
    """Minimal ASGI app for the viewer API + static files."""
    vault = _get_vault()
    embeddings = _get_embeddings()
    index = _get_index()

    if scope["type"] == "lifespan":
        await handle_lifespan(scope, receive, send)
        return

    if scope["type"] != "http":
        return

    method = scope["method"]
    path = scope["path"]
    headers = dict(scope.get("headers", []))

    # --- CORS headers ---
    cors_headers = [
        (b"access-control-allow-origin", b"*"),
        (b"access-control-allow-methods", b"GET, POST, OPTIONS"),
        (b"access-control-allow-headers", b"Content-Type"),
    ]

    if method == "OPTIONS":
        await send_response(send, 200, "OK", headers=cors_headers)
        return

    # --- Routes ---
    if path == "/api/search" and method == "GET":
        await handle_search(scope, receive, send, embeddings, index, vault, cors_headers)
    elif path == "/api/read" and method == "GET":
        await handle_read(scope, receive, send, vault, cors_headers)
    elif path == "/api/list" and method == "GET":
        await handle_list(scope, receive, send, vault, index, cors_headers)
    elif path == "/api/status" and method == "GET":
        await handle_status(scope, receive, send, vault, index, embeddings, cors_headers)
    else:
        # Serve static files
        await serve_static(scope, receive, send, path, cors_headers)


# ---------------------------------------------------------------------------
# Route handlers
# ---------------------------------------------------------------------------

async def handle_search(scope, receive, send, embeddings, index, vault, cors_headers):
    import urllib.parse
    from brain_server.index.store import SearchResult

    qs = urllib.parse.parse_qs(scope.get("query_string", b"").decode())
    query = qs.get("query", [""])[0]
    top_k = int(qs.get("top_k", ["10"])[0])
    layer_filter = qs.get("layer", [None])[0] or None

    if not query:
        return await send_json(send, {"error": "query parameter is required"}, 400, cors_headers)

    try:
        results: list[SearchResult] = await index.search(
            query,
            embeddings=embeddings,
            top_k=top_k,
            layer=layer_filter,
        )
        data = [
            {
                "path": r.path,
                "layer": r.layer,
                "snippet": r.snippet,
                "score": r.score,
                "chunk_index": r.chunk_index,
            }
            for r in results
        ]
        return await send_json(send, {"results": data}, 200, cors_headers)
    except Exception as e:
        logger.exception("Search error")
        return await send_json(send, {"error": str(e)}, 500, cors_headers)


async def handle_read(scope, receive, send, vault, cors_headers):
    import urllib.parse

    qs = urllib.parse.parse_qs(scope.get("query_string", b"").decode())
    layer = qs.get("layer", [""])[0]
    path = qs.get("path", [""])[0]

    if not layer or not path:
        return await send_json(send, {"error": "layer and path parameters are required"}, 400, cors_headers)

    try:
        content = vault.read_note(layer=layer, path=path)
        if content is None:
            return await send_json(send, {"error": "Note not found"}, 404, cors_headers)
        return await send_json(send, {"content": content, "layer": layer, "path": path}, 200, cors_headers)
    except Exception as e:
        logger.exception("Read error")
        return await send_json(send, {"error": str(e)}, 500, cors_headers)


async def handle_list(scope, receive, send, vault, index, cors_headers):
    try:
        files = vault.list_all_files()
        entries = []
        for f in sorted(files):
            rel = f.relative_to(vault.vault_path)
            layer = rel.parts[0] if len(rel.parts) > 0 else ""
            path = str(rel.with_suffix(""))
            entries.append({
                "layer": layer,
                "path": f"{layer}/{path}.md",
                "indexed": index.get(f"{layer}/{path}.md") is not None,
            })
        return await send_json(send, {"entries": entries}, 200, cors_headers)
    except Exception as e:
        logger.exception("List error")
        return await send_json(send, {"error": str(e)}, 500, cors_headers)


async def handle_status(scope, receive, send, vault, index, embeddings, cors_headers):
    ollama_ok = False
    try:
        ollama_ok = await embeddings.health_check()
    except Exception:
        pass

    return await send_json(send, {
        "vault_path": str(vault.vault_path),
        "vault_files": len(vault.list_all_files()),
        "index_entries": index.size(),
        "ollama_url": OLLAMA_URL,
        "ollama_model": OLLAMA_MODEL,
        "ollama_ok": ollama_ok,
    }, 200, cors_headers)


# ---------------------------------------------------------------------------
# Static files
# ---------------------------------------------------------------------------

async def serve_static(scope, receive, send, path, cors_headers):
    viewer_dir = Path(__file__).resolve().parent
    if path == "/" or path == "":
        path = "/index.html"

    file_path = viewer_dir / path.lstrip("/")
    # Prevent directory traversal
    try:
        file_path = file_path.resolve()
        file_path.relative_to(viewer_dir.resolve())
    except (ValueError, FileNotFoundError):
        return await send_json(send, {"error": "Not found"}, 404, cors_headers)

    if not file_path.is_file():
        return await send_json(send, {"error": "Not found"}, 404, cors_headers)

    content = file_path.read_bytes()
    content_type = {
        ".html": "text/html; charset=utf-8",
        ".css": "text/css; charset=utf-8",
        ".js": "application/javascript",
        ".json": "application/json",
        ".png": "image/png",
        ".svg": "image/svg+xml",
        ".ico": "image/x-icon",
    }.get(file_path.suffix, "application/octet-stream")

    await send_response(send, 200, content, {
        *(cors_headers or []),
        (b"content-type", content_type.encode()),
    })


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

async def handle_lifespan(scope, receive, send):
    while True:
        message = await receive()
        if message["type"] == "lifespan.startup":
            await send({"type": "lifespan.startup.complete"})
        elif message["type"] == "lifespan.shutdown":
            await send({"type": "lifespan.shutdown.complete"})
            return


async def send_json(send, data, status=200, extra_headers=None):
    body = json.dumps(data, ensure_ascii=False).encode("utf-8")
    headers = [
        (b"content-type", b"application/json; charset=utf-8"),
    ]
    if extra_headers:
        headers = list(extra_headers) + headers
    await send_response(send, status, body, headers)


async def send_response(send, status, body, headers=None):
    if isinstance(body, str):
        body = body.encode("utf-8")
    await send({
        "type": "http.response.start",
        "status": status,
        "headers": headers or [(b"content-type", b"text/plain")],
    })
    await send({
        "type": "http.response.body",
        "body": body,
    })


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main():
    logging.basicConfig(
        level=getattr(logging, os.environ.get("BRAIN_LOG_LEVEL", "INFO").upper(), logging.INFO),
        format="%(levelname)s  %(name)s  %(message)s",
    )

    logger.info("Starting Brain Viewer on http://%s:%d", VIEWER_HOST, VIEWER_PORT)
    logger.info("Vault: %s | Index: %s", VAULT_PATH, INDEX_PATH)
    logger.info("Ollama: %s (%s)", OLLAMA_URL, OLLAMA_MODEL)

    import uvicorn
    uvicorn.run(
        "viewer.server:app",
        host=VIEWER_HOST,
        port=VIEWER_PORT,
        log_level="info",
        reload=False,
    )


if __name__ == "__main__":
    main()
