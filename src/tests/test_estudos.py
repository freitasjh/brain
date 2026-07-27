"""Tests for estudos layer (full study content)."""

import pytest
from brain_server.vault.models import VALID_LAYERS, LAYERS_WITH_SCOPE
from brain_server.vault.manager import VaultManager


class TestEstudosLayer:
    """Test estudos layer functionality."""

    def test_estudos_in_valid_layers(self):
        """estudos should be in VALID_LAYERS."""
        assert "estudos" in VALID_LAYERS

    def test_estudos_in_layers_with_scope(self):
        """estudos should require scope."""
        assert "estudos" in LAYERS_WITH_SCOPE

    def test_write_estudo_global(self, tmp_path):
        """Write a full study to estudos/global."""
        vault = VaultManager(tmp_path)
        study_content = """---
tags: [java, oo, fundamentos]
topico: Java OO
nivel: iniciante
---

# Java OO — Estudo Completo

## Conceitos
Java é uma linguagem orientada a objetos.

```java
public class Hello {
    public static void main(String[] args) {
        System.out.println("Hello");
    }
}
```
"""
        note = vault.write(
            layer="estudos",
            path="java/orientacao-objetos",
            content=study_content,
            scope="global"
        )
        
        assert note.scope == "global"
        assert note.full_path == "estudos/global/java/orientacao-objetos.md"
        assert (tmp_path / "estudos" / "global" / "java" / "orientacao-objetos.md").exists()

    def test_write_estudo_projeto(self, tmp_path):
        """Write a full study to estudos/projetos."""
        vault = VaultManager(tmp_path)
        note = vault.write(
            layer="estudos",
            path="meu-app/analise-db",
            content="# Análise de BD\n\n## Conteúdo completo...",
            scope="projetos"
        )
        
        assert note.scope == "projetos"
        assert note.full_path == "estudos/projetos/meu-app/analise-db.md"

    def test_estudo_without_scope_raises(self, tmp_path):
        """estudos without scope should raise ValueError."""
        vault = VaultManager(tmp_path)
        with pytest.raises(ValueError, match="requires scope"):
            vault.write(
                layer="estudos",
                path="java/oo",
                content="# Java",
                scope=None
            )

    def test_list_estudos_by_scope(self, tmp_path):
        """List estudos filtered by scope."""
        vault = VaultManager(tmp_path)
        vault.write("estudos", "java/oo", "# Java", scope="global")
        vault.write("estudos", "docker/basico", "# Docker", scope="global")
        vault.write("estudos", "meu-app/db", "# DB", scope="projetos")
        
        all_estudos = vault.list_notes(layer="estudos")
        assert len(all_estudos) == 3
        
        global_estudos = vault.list_notes(layer="estudos", scope="global")
        assert len(global_estudos) == 2
        assert all(n.scope == "global" for n in global_estudos)
