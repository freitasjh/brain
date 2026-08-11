"""Tests for brain_store frontmatter generation."""

from __future__ import annotations

from brain_server.tools.brain_store import _build_frontmatter, _merge_frontmatter


class TestFrontmatter:
    def test_build_frontmatter_all_fields(self):
        fm = _build_frontmatter(
            project="my-app", tags=["java", "fundamentos"],
            scope="global", layer="estudos",
        )
        assert "project: my-app" in fm
        assert "tags: [java, fundamentos]" in fm
        assert "scope: global" in fm
        assert "layer: estudos" in fm
        assert "created_at:" in fm
        assert fm.startswith("---")
        assert fm.endswith("---")

    def test_build_frontmatter_partial(self):
        fm = _build_frontmatter(project="my-app")
        assert "project: my-app" in fm
        assert "tags:" not in fm
        assert "scope:" not in fm

    def test_merge_frontmatter_new_content(self):
        """Content without frontmatter should get frontmatter prepended."""
        content = "# My Note\n\nSome content here."
        fm = _build_frontmatter(project="my-app", tags=["java"])
        result = _merge_frontmatter(content, fm)

        assert result.startswith("---")
        assert "project: my-app" in result
        assert "tags: [java]" in result
        assert "# My Note" in result
        assert "Some content here." in result

    def test_merge_frontmatter_existing(self):
        """Content with existing frontmatter should merge new fields."""
        content = "---\nlayer: estudos\nscope: global\n---\n\n# My Note"
        fm = _build_frontmatter(project="my-app", tags=["java"])
        result = _merge_frontmatter(content, fm)

        # Should have all fields
        assert "project: my-app" in result
        assert "tags: [java]" in result
        assert "layer: estudos" in result  # preserved
        assert "scope: global" in result   # preserved
        assert "# My Note" in result

    def test_merge_frontmatter_overrides(self):
        """New fields should override existing ones."""
        content = "---\nproject: old-project\n---\n\n# Note"
        fm = _build_frontmatter(project="new-project")
        result = _merge_frontmatter(content, fm)

        assert "project: new-project" in result
        assert "old-project" not in result
