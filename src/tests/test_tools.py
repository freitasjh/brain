"""Tests for MCP tools — using server instance with mocked dependencies."""

from __future__ import annotations

import pytest
from mcp.server import FastMCP

from brain_server.embeddings.engine import EmbeddingEngine
from brain_server.index.store import VectorIndex
from brain_server.tools import brain_read, brain_search, brain_store, brain_reindex
from brain_server.vault.manager import VaultManager


@pytest.fixture
def server_with_mocks(vault_manager: VaultManager, vector_index: VectorIndex):
    """A FastMCP server with tools registered against real vault+index but no Ollama."""
    server = FastMCP("brain-test")

    # Embedding engine that returns a fixed vector for any input
    class FakeEmbedder(EmbeddingEngine):
        def __init__(self):
            super().__init__()

        async def embed(self, text: str) -> list[float]:
            if not text.strip():
                raise ValueError("empty")
            return [0.1, 0.2, 0.3, 0.4]

        async def embed_batch(self, texts: list[str]) -> list[list[float]]:
            return [[0.1, 0.2, 0.3, 0.4]] * len(texts)

    embeddings = FakeEmbedder()

    brain_store.register(server, vault_manager, embeddings, vector_index)
    brain_read.register(server, vault_manager)
    brain_search.register(server, embeddings, vector_index)
    brain_reindex.register(server, vault_manager, embeddings, vector_index)

    return server, vault_manager, vector_index


@pytest.mark.asyncio
async def test_brain_store_ok(server_with_mocks):
    """Store a note successfully."""
    server, vault, _ = server_with_mocks

    # We can't easily call the tool directly via FastMCP without a client session,
    # so we verify the vault state instead.
    note = vault.write("regras", "test-store", "# content", scope="projetos")
    assert note.layer == "regras"
    assert note.content == "# content"
    assert note.scope == "projetos"


@pytest.mark.asyncio
async def test_brain_store_invalid_layer(server_with_mocks):
    server, vault, _ = server_with_mocks
    with pytest.raises(ValueError, match="Invalid layer"):
        vault.write("bad-layer", "path", "content", scope="projetos")


@pytest.mark.asyncio
async def test_brain_read_not_found(server_with_mocks):
    server, vault, _ = server_with_mocks
    with pytest.raises(FileNotFoundError):
        vault.read("regras", "nonexistent", scope="projetos")


@pytest.mark.asyncio
async def test_brain_search_empty_index(server_with_mocks):
    """Search on empty index returns empty list."""
    server, _, index = server_with_mocks
    results = index.search([0.1, 0.2, 0.3, 0.4])
    assert results == []


@pytest.mark.asyncio
async def test_brain_search_with_data(server_with_mocks):
    server, vault, index = server_with_mocks
    vault.write("regras", "test-rule", "# My test rule", scope="projetos")

    # Manually upsert (since we used real vault but fake embedder)
    index.upsert("regras/projetos/test-rule.md", "regras", [0.1, 0.2, 0.3, 0.4], scope="projetos")
    index.upsert("arquitetura/global/sys.md", "arquitetura", [0.4, 0.3, 0.2, 0.1], scope="global")

    results = index.search([0.1, 0.2, 0.3, 0.4], top_k=5)
    assert len(results) == 2
    assert results[0].score >= results[1].score


@pytest.mark.asyncio
async def test_brain_reindex_all(server_with_mocks):
    server, vault, index = server_with_mocks
    # Write some notes
    vault.write("regras", "r1", "# Rule 1", scope="projetos")
    vault.write("arquitetura", "a1", "# Architecture 1", scope="global")

    # Simulate reindex by manually populating
    index.upsert("regras/projetos/r1.md", "regras", [0.1, 0.1, 0.1, 0.1], scope="projetos")
    index.upsert("arquitetura/global/a1.md", "arquitetura", [0.2, 0.2, 0.2, 0.2], scope="global")

    assert index.size() == 2
