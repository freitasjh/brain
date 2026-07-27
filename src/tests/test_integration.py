"""Integration tests — full MCP server lifecycle with mocked Ollama.

Spins up the server as a subprocess via stdio transport, connects via MCP client,
and exercises all tools end-to-end. Ollama is mocked via a local HTTP server.
"""

from __future__ import annotations

import json
import os
import threading
from http.server import HTTPServer, BaseHTTPRequestHandler
from pathlib import Path

import pytest
from mcp.client.session import ClientSession
from mcp.client.stdio import StdioServerParameters, stdio_client


class OllamaMockHandler(BaseHTTPRequestHandler):
    """A minimal HTTP server that mocks Ollama's /api/embeddings endpoint."""

    def do_POST(self):
        if self.path == "/api/embeddings":
            content_length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(content_length)
            _ = json.loads(body)  # parse but ignore
            response = json.dumps({"embedding": [0.1, 0.2, 0.3, 0.4]}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(response)))
            self.end_headers()
            self.wfile.write(response)
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, *args, **kwargs):
        pass  # suppress stderr output


@pytest.fixture(scope="session")
def ollama_mock_server():
    """Start a mock Ollama HTTP server on a dynamic port, yield (host, port)."""
    server = HTTPServer(("127.0.0.1", 0), OllamaMockHandler)
    port = server.server_port
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    yield f"http://127.0.0.1:{port}"
    server.shutdown()


@pytest.fixture
def env_with_temp_paths(
    tmp_path: Path,
    ollama_mock_server: str,
) -> dict[str, str]:
    """Environment variables pointing to temp vault and index."""
    vault = tmp_path / "vault"
    index = tmp_path / "data" / "index.db"
    vault.mkdir()
    index.parent.mkdir(parents=True)
    return {
        "BRAIN_VAULT_PATH": str(vault),
        "BRAIN_INDEX_PATH": str(index),
        "BRAIN_OLLAMA_URL": ollama_mock_server,
        "BRAIN_LOG_LEVEL": "ERROR",
        "BRAIN_TRANSPORT": "stdio",
    }


@pytest.mark.asyncio
async def test_integration_full_cycle(
    env_with_temp_paths: dict[str, str],
    tmp_path: Path,
):
    """End-to-end: store → search → read → reindex → search again."""
    server_params = StdioServerParameters(
        command="uv",
        args=["run", "python", "-m", "brain_server"],
        env={**os.environ, **env_with_temp_paths},
        cwd=str(tmp_path),
    )

    async with stdio_client(server_params) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()

            # ── 1. ping ────────────────────────────────────────────────────
            ping_result = await session.call_tool("ping")
            assert ping_result.content
            text = ping_result.content[0].text
            assert text == "pong", f"Expected pong got: {text}"

            # ── 2. brain_store ─────────────────────────────────────────────
            store_result = await session.call_tool(
                "brain_store",
                arguments={
                    "layer": "regras",
                    "path": "teste/regra-1",
                    "content": "# Regra 1\n\nNunca usar SELECT *.",
                    "scope": "projetos",
                },
            )
            assert store_result.content
            store_text = store_result.content[0].text
            assert "ok" in store_text or "deferred" in store_text

            # ── 3. brain_read ──────────────────────────────────────────────
            read_result = await session.call_tool(
                "brain_read",
                arguments={"layer": "regras", "path": "teste/regra-1", "scope": "projetos"},
            )
            assert read_result.content
            read_text = read_result.content[0].text
            assert "Regra 1" in read_text
            assert "SELECT *" in read_text

            # ── 4. brain_search (after store should be indexed) ────────────
            search_result = await session.call_tool(
                "brain_search",
                arguments={"query": "select proibido", "top_k": 5},
            )
            assert search_result.content
            search_text = search_result.content[0].text
            data = json.loads(search_text)
            assert data["total"] > 0, "Search should find stored notes"
            first = data["results"][0]
            assert first["layer"] == "regras"
            assert first["score"] > 0

            # ── 5. brain_reindex (all) ─────────────────────────────────────
            reindex_result = await session.call_tool(
                "brain_reindex",
                arguments={"all": True},
            )
            assert reindex_result.content
            reindex_text = reindex_result.content[0].text
            assert "Reindexed" in reindex_text

            # ── 6. brain_search after reindex ─────────────────────────────
            search2_result = await session.call_tool(
                "brain_search",
                arguments={"query": "select", "top_k": 5},
            )
            assert search2_result.content
            search2_text = search2_result.content[0].text
            data2 = json.loads(search2_text)
            assert data2["total"] > 0, "Search should find results after reindex"
            assert data2["results"][0]["score"] > 0


@pytest.mark.asyncio
async def test_integration_errors(
    env_with_temp_paths: dict[str, str],
    tmp_path: Path,
):
    """Error paths: invalid layer, missing file, empty query."""
    server_params = StdioServerParameters(
        command="uv",
        args=["run", "python", "-m", "brain_server"],
        env={**os.environ, **env_with_temp_paths},
        cwd=str(tmp_path),
    )

    async with stdio_client(server_params) as (read, write):
        async with ClientSession(read, write) as session:
            await session.initialize()

            # Invalid layer on store
            result1 = await session.call_tool(
                "brain_store",
                arguments={"layer": "invalid", "path": "x", "content": "x", "scope": "projetos"},
            )
            assert "INVALID" in result1.content[0].text

            # Invalid layer on read
            result2 = await session.call_tool(
                "brain_read",
                arguments={"layer": "invalid", "path": "x", "scope": "projetos"},
            )
            assert "INVALID" in result2.content[0].text

            # File not found on read
            result3 = await session.call_tool(
                "brain_read",
                arguments={"layer": "regras", "path": "nonexistent", "scope": "projetos"},
            )
            assert "NOT_FOUND" in result3.content[0].text

            # Empty query on search
            result4 = await session.call_tool(
                "brain_search",
                arguments={"query": ""},
            )
            assert "INVALID_PARAMS" in result4.content[0].text

            # Conflicting reindex params
            result5 = await session.call_tool(
                "brain_reindex",
                arguments={},
            )
            assert "INVALID_PARAMS" in result5.content[0].text
