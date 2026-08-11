"""Tests for VectorIndex — SQLite vector store with sqlite-vec."""

from __future__ import annotations

from pathlib import Path

import pytest

from brain_server.index.store import VectorIndex, _cosine_similarity


TEST_DIM = 4


class TestCosineSimilarity:
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
    @pytest.fixture
    def idx(self, tmp_path: Path) -> VectorIndex:
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
        assert results[0].path == "regras/test.md"
        assert results[0].score > results[1].score

    def test_search_with_layer_filter(self, idx: VectorIndex):
        idx.upsert("regras/r.md", "regras", [1.0, 0.0, 0.0, 0.0])
        idx.upsert("arquitetura/a.md", "arquitetura", [0.8, 0.2, 0.0, 0.0])

        results = idx.search([1.0, 0.0, 0.0, 0.0], layer_filter="regras")
        assert len(results) == 1
        assert results[0].layer == "regras"

    def test_search_with_scope_filter(self, idx: VectorIndex):
        idx.upsert("regras/p1.md", "regras", [1.0, 0.0, 0.0, 0.0], scope="projetos")
        idx.upsert("regras/g1.md", "regras", [0.9, 0.1, 0.0, 0.0], scope="global")

        results = idx.search([1.0, 0.0, 0.0, 0.0], top_k=5, scope_filter="projetos")
        assert len(results) == 1
        assert results[0].scope == "projetos"

    def test_upsert_replaces_existing(self, idx: VectorIndex):
        idx.upsert("regras/r.md", "regras", [1.0, 0.0, 0.0, 0.0])
        assert idx.size() == 1
        idx.upsert("regras/r.md", "regras", [0.0, 1.0, 0.0, 0.0])
        assert idx.size() == 1
        results = idx.search([0.0, 1.0, 0.0, 0.0])
        assert results[0].path == "regras/r.md"
        assert results[0].score > 0.99

    def test_remove(self, idx: VectorIndex):
        idx.upsert("regras/r.md", "regras", [1.0, 0.0, 0.0, 0.0])
        assert idx.size() == 1
        idx.remove("regras/r.md")
        assert idx.size() == 0

    def test_remove_multiple_chunks(self, idx: VectorIndex):
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
        db_path = tmp_path / "test_persist.db"
        idx1 = VectorIndex(db_path, embed_dim=TEST_DIM)
        idx1.load()
        idx1.upsert("regras/r.md", "regras", [1.0, 0.0, 0.0, 0.0])
        idx1.upsert("arquitetura/a.md", "arquitetura", [0.0, 1.0, 0.0, 0.0])
        idx1.save()

        idx2 = VectorIndex(db_path, embed_dim=TEST_DIM)
        idx2.load()
        assert idx2.size() == 2
        results = idx2.search([1.0, 0.0, 0.0, 0.0])
        assert len(results) == 2
        assert results[0].path == "regras/r.md"

    def test_snippet_truncation(self, idx: VectorIndex):
        long_snippet = "x" * 500
        idx.upsert("regras/r.md", "regras", [1.0, 0.0, 0.0, 0.0], snippet=long_snippet)
        results = idx.search([1.0, 0.0, 0.0, 0.0])
        assert len(results[0].snippet) <= 200

    def test_missing_db_file(self, tmp_path: Path):
        db_path = tmp_path / "fresh.db"
        assert not db_path.exists()
        idx = VectorIndex(db_path, embed_dim=TEST_DIM)
        idx.load()
        assert idx.size() == 0
        assert db_path.exists()

    def test_search_top_k(self, idx: VectorIndex):
        for i in range(10):
            idx.upsert(f"doc{i}.md", "regras", [1.0 - i * 0.1, 0.0, 0.0, 0.0])
        results = idx.search([1.0, 0.0, 0.0, 0.0], top_k=3)
        assert len(results) == 3

    def test_search_with_tags(self, idx: VectorIndex):
        idx.upsert("regras/r1.md", "regras", [1.0, 0.0, 0.0, 0.0], tags=["java", "fundamentos"])
        idx.upsert("regras/r2.md", "regras", [0.9, 0.1, 0.0, 0.0], tags=["docker"])

        results = idx.search([1.0, 0.0, 0.0, 0.0], tag_filter="java")
        assert len(results) == 1
        assert results[0].tags == ["java", "fundamentos"]

    def test_search_with_project(self, idx: VectorIndex):
        proj = idx.project_create("my-app")
        idx.upsert("regras/r1.md", "regras", [1.0, 0.0, 0.0, 0.0], project_id=proj.id)
        idx.upsert("regras/r2.md", "regras", [0.9, 0.1, 0.0, 0.0])

        results = idx.search([1.0, 0.0, 0.0, 0.0], project_filter="my-app")
        assert len(results) == 1
        assert results[0].project_name == "my-app"


class TestProjects:
    @pytest.fixture
    def idx(self, tmp_path: Path) -> VectorIndex:
        return VectorIndex(tmp_path / "test.db", embed_dim=TEST_DIM)

    def test_create_project(self, idx: VectorIndex):
        proj = idx.project_create("my-app", "Meu projeto")
        assert proj.name == "my-app"
        assert proj.description == "Meu projeto"
        assert proj.id > 0

    def test_create_duplicate_project_raises(self, idx: VectorIndex):
        idx.project_create("my-app")
        with pytest.raises(ValueError, match="already exists"):
            idx.project_create("my-app")

    def test_get_project(self, idx: VectorIndex):
        idx.project_create("my-app")
        proj = idx.project_get("my-app")
        assert proj is not None
        assert proj.name == "my-app"

    def test_get_nonexistent_project(self, idx: VectorIndex):
        proj = idx.project_get("nonexistent")
        assert proj is None

    def test_list_projects(self, idx: VectorIndex):
        idx.project_create("alpha")
        idx.project_create("beta")
        projects = idx.project_list()
        assert len(projects) == 2
        assert [p.name for p in projects] == ["alpha", "beta"]

    def test_delete_project(self, idx: VectorIndex):
        idx.project_create("my-app")
        assert idx.project_delete("my-app") is True
        assert idx.project_get("my-app") is None

    def test_delete_nonexistent_project(self, idx: VectorIndex):
        assert idx.project_delete("nonexistent") is False

    def test_upsert_with_project(self, idx: VectorIndex):
        proj = idx.project_create("my-app")
        idx.upsert(
            "arquitetura/projetos/my-app/db.md",
            "arquitetura",
            [1.0, 0.0, 0.0, 0.0],
            project_id=proj.id,
            tags=["database", "postgres"],
        )

        results = idx.search([1.0, 0.0, 0.0, 0.0], project_filter="my-app")
        assert len(results) == 1
        assert results[0].project_name == "my-app"
        assert results[0].tags == ["database", "postgres"]

    def test_note_link_project(self, idx: VectorIndex):
        proj = idx.project_create("my-app")
        idx.note_link_project("estudos/global/java/oo.md", "my-app")

        linked = idx.note_linked_projects("estudos/global/java/oo.md")
        assert len(linked) == 1
        assert linked[0].name == "my-app"

    def test_note_unlink_project(self, idx: VectorIndex):
        proj = idx.project_create("my-app")
        idx.note_link_project("estudos/global/java/oo.md", "my-app")
        assert idx.note_unlink_project("estudos/global/java/oo.md", "my-app") is True
        assert idx.note_linked_projects("estudos/global/java/oo.md") == []

    def test_project_notes_owned(self, idx: VectorIndex):
        proj = idx.project_create("my-app")
        idx.upsert("regras/projetos/my-app/naming.md", "regras", [1.0, 0.0, 0.0, 0.0],
                    scope="projetos", project_id=proj.id)

        notes = idx.project_notes("my-app")
        assert len(notes) == 1
        assert notes[0]["source"] == "owned"

    def test_project_notes_linked(self, idx: VectorIndex):
        proj = idx.project_create("my-app")
        idx.upsert("estudos/global/java/oo.md", "estudos", [1.0, 0.0, 0.0, 0.0], scope="global")
        idx.note_link_project("estudos/global/java/oo.md", "my-app")

        notes = idx.project_notes("my-app")
        assert len(notes) == 1
        assert notes[0]["source"] == "linked"
        assert notes[0]["path"] == "estudos/global/java/oo.md"

    def test_project_notes_empty(self, idx: VectorIndex):
        idx.project_create("my-app")
        notes = idx.project_notes("my-app")
        assert notes == []

    def test_project_notes_nonexistent(self, idx: VectorIndex):
        notes = idx.project_notes("nonexistent")
        assert notes == []

    def test_search_project_filter_includes_linked(self, idx: VectorIndex):
        """Search with project_filter should include both owned AND linked global notes."""
        proj = idx.project_create("my-app")

        # Owned note
        idx.upsert("regras/projetos/my-app/naming.md", "regras", [1.0, 0.0, 0.0, 0.0],
                    scope="projetos", project_id=proj.id)

        # Global note linked to project
        idx.upsert("estudos/global/java/oo.md", "estudos", [0.9, 0.1, 0.0, 0.0], scope="global")
        idx.note_link_project("estudos/global/java/oo.md", "my-app")

        # Global note NOT linked
        idx.upsert("estudos/global/docker/basics.md", "estudos", [0.8, 0.2, 0.0, 0.0], scope="global")

        results = idx.search([1.0, 0.0, 0.0, 0.0], project_filter="my-app")
        paths = [r.path for r in results]
        assert "regras/projetos/my-app/naming.md" in paths
        assert "estudos/global/java/oo.md" in paths
        assert "estudos/global/docker/basics.md" not in paths

    def test_remove_cascades_note_projects(self, idx: VectorIndex):
        idx.project_create("my-app")
        idx.upsert("estudos/global/java/oo.md", "estudos", [1.0, 0.0, 0.0, 0.0], scope="global")
        idx.note_link_project("estudos/global/java/oo.md", "my-app")
        assert idx.note_linked_projects("estudos/global/java/oo.md") == [idx.project_get("my-app")]

        idx.remove("estudos/global/java/oo.md")
        assert idx.note_linked_projects("estudos/global/java/oo.md") == []
