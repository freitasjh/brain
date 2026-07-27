"""MCP tool: brain_store — save a note to the vault and index it."""

from __future__ import annotations

import logging

from mcp.server import FastMCP

from brain_server.embeddings.engine import EmbeddingEngine, EmbeddingError
from brain_server.index.store import VectorIndex
from brain_server.vault.manager import VaultManager
from brain_server.vault.models import LAYERS_WITH_SCOPE

logger = logging.getLogger(__name__)


def register(server: FastMCP, vault: VaultManager, embeddings: EmbeddingEngine, index: VectorIndex) -> None:
    """Register the brain_store tool."""

    @server.tool()
    async def brain_store(layer: str, path: str, content: str, scope: str | None = None) -> str:
        """Save a note to the brain vault and index it for semantic search.

        Args:
            layer: Vault layer (arquitetura, regras, sessoes, projetos, indexacao)
            path: Relative path without extension (e.g. "projeto-x/regra-1")
            content: Markdown content
            scope: Required for 'arquitetura' and 'regras' layers. Use 'projetos' for project-specific content or 'global' for shared content.
        """
        # Validate scope for layers that require it
        if layer in LAYERS_WITH_SCOPE and scope is None:
            return f"INVALID_PARAMS: Layer '{layer}' requires scope parameter ('projetos' or 'global')"
        
        # Write to vault
        try:
            note = vault.write(layer, path, content, scope=scope)
        except ValueError as exc:
            return f"INVALID_PARAMS: {exc}"

        # Embed and index
        try:
            chunks = embeddings.chunk_text(content)
            embeddings_list = await embeddings.embed_batch(chunks)

            for i, (chunk_text, vec) in enumerate(zip(chunks, embeddings_list)):
                index.upsert(
                    path=note.full_path,
                    layer=layer,
                    embedding=vec,
                    snippet=chunk_text,
                    chunk_index=i,
                    total_chunks=len(chunks),
                    scope=scope,
                )

            index.save()
            logger.info(
                "Indexed '%s' — %d chunks", note.full_path, len(chunks)
            )
        except EmbeddingError as exc:
            logger.warning("Note saved but indexing deferred: %s", exc)
            return f"Note saved but indexing deferred: {exc}"

        return f"ok — {note.full_path}"
