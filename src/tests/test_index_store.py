"""Tests for VectorIndex — SQLite vector store with sqlite-vec."""

from __future__ import annotations

from pathlib import Path

import pytest

from brain_server.index.store import VectorIndex, _cosine_similarity


# Test dimension: small enough for quick tests, vec0 only needs 2D+
TEST_DIM = 4


class TestCosineSimilarity:
    """Standalone cosine similarity (used by tests, not SQLite)."""

    def test_identical_vectors(self):
        assert _cosine_similarity([1.0, 0.0], [1.0, 0.0]) == pytest.approx(1.0)

    def test_orthogonal_vectors(self):
        assert _cosine_similarity([1.0, 0.0], [0.0, 1.0]) == pytest.approx(0.0)

    def test_opposite_vectors(self):
        assert _cosine_similarity([1.0, 0.0], [-1.0, 0.0]) == pytest.approx(-1.0)

    def test_zero_vector(self):
        assert _cosine_similarity([0.0, 0.0], [1.0, 0.0]) == pytest.approx(0.0)

    def test_dimension_mismatch(self):
        with pytest.raises(ValueError):
            _cosine_similarity([1.0], [1.0, 2.0])


class TestVectorIndex:
    """Test SQLite-based VectorIndex."""

    @pytest.fixture
    def idx(self, tmp_path: Path) -> VectorIndex:
        """Create a fresh VectorIndex backed by a temp SQLite DB."""
        return VectorIndex(tmp_path / "test.db", embed_dim=TEST_DIM)

    def test_empty_index(self, idx: VectorIndex):
        assert idx.size() == 0
        results = idx.search([1.0, 0.0, 0.0, 0.0])
        assert results == []

    def test_upsert_and_search(self, idx: VectorIndex):
        idx.upsert("regras/test.md", "regras", [1.0, 0.0, 0.0, 0.0])
        idx.upsert("arquitetura/sys.md", "arquitetura", [0.0, 1.0, 0.0, 0.0])

        results = idx.search([1.0, 0.0, 0.0, 0.0], top_k=5)
        assert len(results) == 2
        # First result should be the most similar (distance 0 → similarity 1)
        assert results[0].path == "regras/test.md"
        assert results[0].score > results[1].score

    def test_search_with_layer_filter(self, idx: VectorIndex):
        idx.upsert("regras/r.md", "regras", [1.0, 0.0, 0.0, 0.0])
        idx.upsert("arquitetura/a.md", "arquitetura", [0.8, 0.2, 0.0, 0.0])

        results = idx.search([1.0, 0.0, 0.0, 0.0], layer_filter="regras")
        assert len(results) == 1
        assert results[0].layer == "regras"
        assert results[0].path == "regras/r.md"

    def test_search_with_scope_filter(self, idx: VectorIndex):
        idx.upsert("regras/p1.md", "regras", [1.0, 0.0, 0.0, 0.0], scope="projetos")
        idx.upsert("regras/g1.md", "regras", [0.9, 0.1, 0.0, 0.0], scope="global")

        results = idx.search([1.0, 0.0, 0.0, 0.0], top_k=5, scope_filter="projetos")
        assert len(results) == 1
        assert results[0].scope == "projetos"

    def test_upsert_replaces_existing(self, idx: VectorIndex):
        """Upsert with same path+chunk_index should replace embedding."""
        idx.upsert("regras/r.md", "regras", [1.0, 0.0, 0.0, 0.0])
        assert idx.size() == 1

        # Replace with different embedding
        idx.upsert("regras/r.md", "regras", [0.0, 1.0, 0.0, 0.0])

        assert idx.size() == 1  # still 1 entry
        results = idx.search([0.0, 1.0, 0.0, 0.0])
        assert results[0].path == "regras/r.md"
        assert results[0].score > 0.99  # now matches the new vector

    def test_remove(self, idx: VectorIndex):
        idx.upsert("regras/r.md", "regras", [1.0, 0.0, 0.0, 0.0])
        assert idx.size() == 1
        idx.remove("regras/r.md")
        assert idx.size() == 0

    def test_remove_multiple_chunks(self, idx: VectorIndex):
        """Remove a document with multiple chunks."""
        idx.upsert("doc.md", "regras", [1.0, 0.0, 0.0, 0.0], chunk_index=0, total_chunks=2)
        idx.upsert("doc.md", "regras", [0.0, 1.0, 0.0, 0.0], chunk_index=1, total_chunks=2)
        assert idx.size() == 2

        idx.remove("doc.md")
        assert idx.size() == 0

    def test_clear(self, idx: VectorIndex):
        idx.upsert("a", "regras", [1.0, 0.0, 0.0, 0.0])
        idx.upsert("b", "regras", [0.5, 0.5, 0.0, 0.0])
        assert idx.size() == 2
        idx.clear()
        assert idx.size() == 0

    def test_persist_and_reload(self, tmp_path: Path):
        """Save to SQLite, close, reopen and verify data persists."""
        db_path = tmp_path / "test_persist.db"

        # First session
        idx1 = VectorIndex(db_path, embed_dim=TEST_DIM)
        idx1.load()
        idx1.upsert("regras/r.md", "regras", [1.0, 0.0, 0.0, 0.0])
        idx1.upsert("arquitetura/a.md", "arquitetura", [0.0, 1.0, 0.0, 0.0])
        idx1.save()
        assert idx1.size() == 2

        # Second session (fresh instance, same DB)
        idx2 = VectorIndex(db_path, embed_dim=TEST_DIM)
        idx2.load()
        assert idx2.size() == 2

        results = idx2.search([1.0, 0.0, 0.0, 0.0])
        assert len(results) == 2
        assert results[0].path == "regras/r.md"

    def test_snippet_truncation(self, idx: VectorIndex):
        """Snippet should be truncated to 200 chars."""
        long_snippet = "x" * 500
        idx.upsert("regras/r.md", "regras", [1.0, 0.0, 0.0, 0.0], snippet=long_snippet)
        results = idx.search([1.0, 0.0, 0.0, 0.0])
        assert len(results[0].snippet) <= 200

    def test_missing_db_file(self, tmp_path: Path):
        """Loading with non-existent DB should create tables."""
        db_path = tmp_path / "fresh.db"
        assert not db_path.exists()
        idx = VectorIndex(db_path, embed_dim=TEST_DIM)
        idx.load()
        assert idx.size() == 0
        assert db_path.exists()  # DB was created

    def test_search_top_k(self, idx: VectorIndex):
        """top_k should limit results correctly."""
        for i in range(10):
            idx.upsert(f"doc{i}.md", "regras", [1.0 - i * 0.1, 0.0, 0.0, 0.0])

        results = idx.search([1.0, 0.0, 0.0, 0.0], top_k=3)
        assert len(results) == 3

        results = idx.search([1.0, 0.0, 0.0, 0.0], top_k=10)
        assert len(results) == 10
