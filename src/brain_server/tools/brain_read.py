"""MCP tool: brain_read — read a note from the vault."""

from __future__ import annotations

from mcp.server import FastMCP

from brain_server.vault.manager import VaultManager
from brain_server.vault.models import LAYERS_WITH_SCOPE


def register(server: FastMCP, vault: VaultManager) -> None:
    """Register the brain_read tool."""

    @server.tool()
    async def brain_read(layer: str, path: str, scope: str | None = None) -> str:
        """Read a note from the brain vault.

        Args:
            layer: Vault layer (arquitetura, regras, sessoes, projetos, indexacao)
            path: Relative path without extension (e.g. "projeto-x/regra-1")
            scope: Required for 'arquitetura' and 'regras' layers. Use 'projetos' for project-specific content or 'global' for shared content.
        """
        # Validate scope for layers that require it
        if layer in LAYERS_WITH_SCOPE and scope is None:
            return f"INVALID_PARAMS: Layer '{layer}' requires scope parameter ('projetos' or 'global')"
        
        try:
            note = vault.read(layer, path, scope=scope)
        except ValueError as exc:
            return f"INVALID_PARAMS: {exc}"
        except FileNotFoundError as exc:
            return f"NOT_FOUND: {exc}"

        return f"# {note.full_path}\n\n{note.content}"
