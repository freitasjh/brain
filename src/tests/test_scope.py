"""Tests for scope functionality (projetos vs global)."""

import pytest
from pathlib import Path
from brain_server.vault.models import (
    validate_scope,
    validate_layer,
    SCOPE_PROJECT,
    SCOPE_GLOBAL,
    VALID_SCOPES,
    LAYERS_WITH_SCOPE,
    Note,
)
from brain_server.vault.manager import VaultManager


class TestScopeValidation:
    """Test scope validation functions."""

    def test_validate_scope_valid_project(self):
        """Valid scope 'projetos' should not raise."""
        validate_scope("projetos")

    def test_validate_scope_valid_global(self):
        """Valid scope 'global' should not raise."""
        validate_scope("global")

    def test_validate_scope_invalid(self):
        """Invalid scope should raise ValueError."""
        with pytest.raises(ValueError, match="Invalid scope"):
            validate_scope("invalid")

    def test_validate_scope_empty(self):
        """Empty scope should raise ValueError."""
        with pytest.raises(ValueError, match="Invalid scope"):
            validate_scope("")


class TestLayersWithScope:
    """Test LAYERS_WITH_SCOPE constant."""

    def test_arquitetura_requires_scope(self):
        """arquitetura layer should require scope."""
        assert "arquitetura" in LAYERS_WITH_SCOPE

    def test_regras_requires_scope(self):
        """regras layer should require scope."""
        assert "regras" in LAYERS_WITH_SCOPE

    def test_sessoes_no_scope(self):
        """sessoes layer should not require scope."""
        assert "sessoes" not in LAYERS_WITH_SCOPE

    def test_projetos_no_scope(self):
        """projetos layer should not require scope."""
        assert "projetos" not in LAYERS_WITH_SCOPE


class TestNoteWithScope:
    """Test Note dataclass with scope field."""

    def test_note_with_scope(self):
        """Note should store scope field."""
        note = Note(
            layer="arquitetura",
            path="my-app/db",
            content="# DB",
            scope="projetos"
        )
        assert note.scope == "projetos"
        assert note.full_path == "arquitetura/projetos/my-app/db.md"

    def test_note_without_scope(self):
        """Note without scope should work for non-scoped layers."""
        note = Note(
            layer="sessoes",
            path="2026-07-25",
            content="# Session"
        )
        assert note.scope is None
        assert note.full_path == "sessoes/2026-07-25.md"

    def test_note_global_scope(self):
        """Note with global scope."""
        note = Note(
            layer="regras",
            path="coding-standards",
            content="# Standards",
            scope="global"
        )
        assert note.scope == "global"
        assert note.full_path == "regras/global/coding-standards.md"


class TestVaultManagerWithScope:
    """Test VaultManager with scope support."""

    def test_ensure_layers_creates_scope_dirs(self, tmp_path):
        """VaultManager should create scope subdirectories."""
        vault = VaultManager(tmp_path)
        
        # Check scope directories exist for layers that require scope
        assert (tmp_path / "arquitetura" / "projetos").exists()
        assert (tmp_path / "arquitetura" / "global").exists()
        assert (tmp_path / "regras" / "projetos").exists()
        assert (tmp_path / "regras" / "global").exists()

    def test_write_with_scope(self, tmp_path):
        """Write note with scope should create correct path."""
        vault = VaultManager(tmp_path)
        note = vault.write(
            layer="arquitetura",
            path="my-app/db",
            content="# DB",
            scope="projetos"
        )
        
        assert note.scope == "projetos"
        assert note.full_path == "arquitetura/projetos/my-app/db.md"
        assert (tmp_path / "arquitetura" / "projetos" / "my-app" / "db.md").exists()

    def test_write_without_scope_raises(self, tmp_path):
        """Write to layer that requires scope without scope should raise."""
        vault = VaultManager(tmp_path)
        
        with pytest.raises(ValueError, match="requires scope"):
            vault.write(
                layer="arquitetura",
                path="my-app/db",
                content="# DB",
                scope=None
            )

    def test_write_global_scope(self, tmp_path):
        """Write note with global scope."""
        vault = VaultManager(tmp_path)
        note = vault.write(
            layer="regras",
            path="coding-standards",
            content="# Standards",
            scope="global"
        )
        
        assert note.scope == "global"
        assert note.full_path == "regras/global/coding-standards.md"
        assert (tmp_path / "regras" / "global" / "coding-standards.md").exists()

    def test_write_no_scope_layer(self, tmp_path):
        """Write to layer that doesn't require scope should work without scope."""
        vault = VaultManager(tmp_path)
        note = vault.write(
            layer="sessoes",
            path="2026-07-25",
            content="# Session",
            scope=None
        )
        
        assert note.scope is None
        assert note.full_path == "sessoes/2026-07-25.md"
        assert (tmp_path / "sessoes" / "2026-07-25.md").exists()

    def test_read_with_scope(self, tmp_path):
        """Read note with scope should work."""
        vault = VaultManager(tmp_path)
        
        # Write first
        vault.write(
            layer="arquitetura",
            path="my-app/db",
            content="# DB",
            scope="projetos"
        )
        
        # Read
        note = vault.read(
            layer="arquitetura",
            path="my-app/db",
            scope="projetos"
        )
        
        assert note.content == "# DB"
        assert note.scope == "projetos"

    def test_list_notes_with_scope_filter(self, tmp_path):
        """List notes with scope filter."""
        vault = VaultManager(tmp_path)
        
        # Write notes with different scopes
        vault.write("arquitetura", "app1/db", "# App1", scope="projetos")
        vault.write("arquitetura", "app2/db", "# App2", scope="projetos")
        vault.write("arquitetura", "standards", "# Standards", scope="global")
        
        # List all
        all_notes = vault.list_notes(layer="arquitetura")
        assert len(all_notes) == 3
        
        # List only projetos
        projetos_notes = vault.list_notes(layer="arquitetura", scope="projetos")
        assert len(projetos_notes) == 2
        assert all(n.scope == "projetos" for n in projetos_notes)
        
        # List only global
        global_notes = vault.list_notes(layer="arquitetura", scope="global")
        assert len(global_notes) == 1
        assert global_notes[0].scope == "global"

    def test_delete_with_scope(self, tmp_path):
        """Delete note with scope should work."""
        vault = VaultManager(tmp_path)
        
        # Write first
        vault.write(
            layer="arquitetura",
            path="my-app/db",
            content="# DB",
            scope="projetos"
        )
        
        # Delete
        vault.delete(
            layer="arquitetura",
            path="my-app/db",
            scope="projetos"
        )
        
        # Verify deleted
        assert not (tmp_path / "arquitetura" / "projetos" / "my-app" / "db.md").exists()
