"""Tests for VaultManager — CRUD operations on vault notes."""

from __future__ import annotations

from pathlib import Path

import pytest

from brain_server.vault.manager import VaultManager
from brain_server.vault.models import VALID_LAYERS, Note


class TestVaultManagerInit:
    def test_creates_layer_directories(self, vault_path: Path):
        manager = VaultManager(vault_path)
        for layer in VALID_LAYERS:
            assert (vault_path / layer).is_dir()

    def test_uses_absolute_path(self, vault_path: Path):
        manager = VaultManager(vault_path)
        assert manager.vault_path.is_absolute()


class TestVaultManagerWrite:
    def test_write_and_read(self, vault_manager: VaultManager):
        note = vault_manager.write("regras", "teste/regra-1", "# Regra 1\nConteúdo", scope="projetos")
        assert note.path == "teste/regra-1"
        assert note.layer == "regras"
        assert note.scope == "projetos"

        # Verify file exists on disk
        expected = vault_manager.vault_path / "regras" / "projetos" / "teste" / "regra-1.md"
        assert expected.read_text() == "# Regra 1\nConteúdo"

    def test_write_invalid_layer(self, vault_manager: VaultManager):
        with pytest.raises(ValueError, match="Invalid layer"):
            vault_manager.write("invalid_layer", "path", "content", scope="projetos")

    def test_write_creates_intermediate_dirs(self, vault_manager: VaultManager):
        vault_manager.write("sessoes", "a/b/c/sessao", "content")
        expected = vault_manager.vault_path / "sessoes" / "a" / "b" / "c" / "sessao.md"
        assert expected.exists()

    def test_write_overwrites_existing(self, vault_manager: VaultManager):
        vault_manager.write("projetos", "foo", "v1")
        vault_manager.write("projetos", "foo", "v2")
        content = (vault_manager.vault_path / "projetos" / "foo.md").read_text()
        assert content == "v2"


class TestVaultManagerRead:
    def test_read_existing(self, vault_manager: VaultManager):
        vault_manager.write("arquitetura", "sys-design", "## System Design", scope="global")
        note = vault_manager.read("arquitetura", "sys-design", scope="global")
        assert note.content == "## System Design"
        assert note.layer == "arquitetura"
        assert note.path == "sys-design"
        assert note.scope == "global"

    def test_read_nonexistent(self, vault_manager: VaultManager):
        with pytest.raises(FileNotFoundError):
            vault_manager.read("regras", "nope", scope="projetos")

    def test_read_invalid_layer(self, vault_manager: VaultManager):
        with pytest.raises(ValueError, match="Invalid layer"):
            vault_manager.read("bad", "path", scope="projetos")


class TestVaultManagerDelete:
    def test_delete_existing(self, vault_manager: VaultManager):
        vault_manager.write("regras", "del-me", "content", scope="projetos")
        vault_manager.delete("regras", "del-me", scope="projetos")
        assert not (vault_manager.vault_path / "regras" / "projetos" / "del-me.md").exists()

    def test_delete_nonexistent(self, vault_manager: VaultManager):
        with pytest.raises(FileNotFoundError):
            vault_manager.delete("sessoes", "ghost")


class TestVaultManagerList:
    def test_list_all(self, vault_manager: VaultManager):
        vault_manager.write("regras", "r1", "c1", scope="projetos")
        vault_manager.write("regras", "r2", "c2", scope="global")
        vault_manager.write("arquitetura", "a1", "c3", scope="global")
        notes = vault_manager.list_notes()
        assert len(notes) == 3

    def test_list_by_layer(self, vault_manager: VaultManager):
        vault_manager.write("regras", "r1", "c1", scope="projetos")
        vault_manager.write("arquitetura", "a1", "c2", scope="global")
        notes = vault_manager.list_notes(layer="regras")
        assert len(notes) == 1
        assert notes[0].layer == "regras"


class TestVaultManagerSecurity:
    def test_path_traversal_rejected(self, vault_manager: VaultManager):
        with pytest.raises(ValueError, match="Path traversal"):
            vault_manager.write("regras", "../../etc/passwd", "evil", scope="projetos")

    def test_absolute_path_rejected(self, vault_manager: VaultManager):
        with pytest.raises(ValueError, match="Absolute paths"):
            vault_manager.write("regras", "/etc/passwd", "evil", scope="projetos")

    def test_symlink_outside_rejected(self, vault_path: Path, tmp_path: Path):
        """Write with a symlink pointing outside vault should be rejected."""
        # First create the vault manager (creates layer directories)
        manager = VaultManager(vault_path)

        outside = tmp_path / "outside.txt"
        outside.write_text("evil content")

        # Create a symlink inside vault that points outside
        link_path = vault_path / "regras" / "projetos" / "badlink.md"
        link_path.parent.mkdir(parents=True, exist_ok=True)
        link_path.symlink_to(outside)

        # Attempt to read via the symlink — should be rejected by _resolve
        with pytest.raises(ValueError, match="escapes vault"):
            manager.read("regras", "badlink", scope="projetos")
