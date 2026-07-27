"""Tests for EmbeddingEngine — Ollama client and text chunking."""

from __future__ import annotations

import pytest

from brain_server.embeddings.engine import EmbeddingEngine, EmbeddingError


class TestChunkText:
    def test_single_section(self):
        text = "# Just one section\nwithout headings"
        chunks = EmbeddingEngine.chunk_text(text)
        assert len(chunks) == 1

    def test_multi_section(self):
        text = """# Top

## Section 1
Content A

## Section 2
Content B
"""
        chunks = EmbeddingEngine.chunk_text(text)
        # First chunk is the content before any ## heading
        assert chunks[0] == "# Top"
        assert "Section 1" in chunks[1]
        assert "Section 2" in chunks[2]
        assert len(chunks) == 3

    def test_empty_text(self):
        chunks = EmbeddingEngine.chunk_text("")
        assert len(chunks) == 1
        assert chunks[0] == ""

    def test_truncation(self):
        long = "x" * 50_000
        chunks = EmbeddingEngine.chunk_text(long, max_tokens=100)
        assert len(chunks) == 1
        assert len(chunks[0]) <= 100 * 4  # rough tokens


class TestEmbedErrors:
    @pytest.mark.asyncio
    async def test_embed_empty_text(self):
        engine = EmbeddingEngine()
        with pytest.raises(EmbeddingError, match="Cannot embed empty text"):
            await engine.embed("   ")
