"""MCP tool: brain_reindex — rebuild vector index partially or fully."""

from __future__ import annotations

import logging

from mcp.server import FastMCP

from brain_server.embeddings.engine import EmbeddingEngine, EmbeddingError
from brain_server.index.store import VectorIndex
from brain_server.vault.manager import VaultManager
from brain_server.vault.models import LAYERS_WITH_SCOPE, VALID_SCOPES

logger = logging.getLogger(__name__)


def _extract_scope_from_path(layer: str, path_stem: str) -> str | None:
    """Extract scope from path if layer requires it.
    
    E.g. for layer='arquitetura' and path_stem='projetos/my-app/db',
    returns 'projetos'. For layers not in LAYERS_WITH_SCOPE, returns None.
    """
    if layer not in LAYERS_WITH_SCOPE:
        return None
    
    parts = path_stem.split("/", 1)
    if len(parts) >= 1 and parts[0] in VALID_SCOPES:
        return parts[0]
    return None


def register(
    server: FastMCP,
    vault: VaultManager,
    embeddings: EmbeddingEngine,
    index: VectorIndex,
) -> None:
    """Register the brain_reindex tool."""

    @server.tool()
    async def brain_reindex(
        all: bool = False,
        layer: str | None = None,
        path: str | None = None,
    ) -> str:
        """Rebuild the vector index (partial or full).

        Args:
            all: If True, reindex every file in the vault
            layer: If provided (and all=False), reindex only this layer
            path: If provided (and all=False, layer=None), reindex specific file
        """
        # Validate parameters
        params = [all, layer is not None, path is not None]
        if sum(params) != 1:
            return (
                "INVALID_PARAMS: provide exactly one of all=True, layer=<name>, or path=<path>"
            )

        # Guard against concurrent reindex operations
        if index.reindexing:
            return "REINDEX_IN_PROGRESS: another reindex operation is already running"

        try:
            async with index._reindex_lock:
                if all:
                    files = vault.list_all_files()
                    logger.info("Reindexing all %d files", len(files))
                    return await _reindex_files(vault, embeddings, index, files)

                if layer:
                    notes = vault.list_notes(layer=layer)
                    return await _reindex_notes(vault, embeddings, index, notes)

                if path:
                    # path should include layer, e.g. "arquitetura/projeto-x/banco.md"
                    if "/" not in path:
                        return "INVALID_PARAMS: path must include layer, e.g. 'arquitetura/projeto-x'"
                    layer_name, rel_path = path.split("/", 1)
                    # Strip trailing .md to avoid double-extension (Bug 5)
                    if rel_path.endswith(".md"):
                        rel_path = rel_path[:-3]
                    # Extract scope for layers that require it (Bug 3)
                    scope = _extract_scope_from_path(layer_name, rel_path)
                    note = vault.read(layer_name, rel_path, scope=scope)
                    return await _reindex_notes(vault, embeddings, index, [note])

        except ValueError as exc:
            return f"INVALID_PARAMS: {exc}"
        except FileNotFoundError as exc:
            return f"NOT_FOUND: {exc}"

        return "INTERNAL_ERROR: unexpected path"


async def _reindex_files(
    vault: VaultManager,
    embeddings: EmbeddingEngine,
    index: VectorIndex,
    files: list,
) -> str:
    """Reindex files from a list of Path objects.

    Computes all embeddings first, then swaps old↔new atomically per file.
    Uses concurrent embedding for speed (Bug 4 fix).
    """
    count = 0
    errors = 0

    for f in files:
        try:
            content = f.read_text(encoding="utf-8")
            # Derive layer and relative path from the file path
            rel = f.relative_to(vault.vault_path)
            layer = rel.parts[0]
            path_stem = str(rel.with_suffix(""))

            # Extract scope if layer requires it
            scope = _extract_scope_from_path(layer, path_stem)

            chunks = embeddings.chunk_text(content)
            embeddings_list = await embeddings.embed_batch_concurrent(chunks)

            # path_stem is already relative to vault root (e.g. "estudos/global/java/x")
            # Do NOT prepend layer again — it's already the first segment (Bug 1 fix)
            full_vault_path = f"{path_stem}.md"

            # Atomic swap: compute embeddings BEFORE removing old entries
            index.remove(full_vault_path)

            for i, (chunk_text, vec) in enumerate(zip(chunks, embeddings_list)):
                index.upsert(
                    path=full_vault_path,
                    layer=layer,
                    embedding=vec,
                    snippet=chunk_text,
                    chunk_index=i,
                    total_chunks=len(chunks),
                    scope=scope,
                )
            count += 1
        except EmbeddingError as exc:
            logger.warning("Skipping %s (embedding failed): %s", f, exc)
            errors += 1
        except Exception as exc:
            logger.error("Failed to index %s: %s", f, exc)
            errors += 1

    index.save()
    return f"Reindexed {count} files ({errors} errors)"


async def _reindex_notes(
    vault: VaultManager,
    embeddings: EmbeddingEngine,
    index: VectorIndex,
    notes: list,
) -> str:
    """Reindex from a list of Note objects.

    Computes all embeddings first, then swaps old↔new atomically per note.
    Uses concurrent embedding for speed (Bug 4 fix).
    """
    count = 0
    errors = 0

    for note in notes:
        try:
            chunks = embeddings.chunk_text(note.content)
            embeddings_list = await embeddings.embed_batch_concurrent(chunks)

            full_path = note.full_path

            # Atomic swap: compute embeddings BEFORE removing old entries
            index.remove(full_path)

            for i, (chunk_text, vec) in enumerate(zip(chunks, embeddings_list)):
                index.upsert(
                    path=full_path,
                    layer=note.layer,
                    embedding=vec,
                    snippet=chunk_text,
                    chunk_index=i,
                    total_chunks=len(chunks),
                    scope=note.scope,
                )
            count += 1
        except EmbeddingError as exc:
            logger.warning("Skipping %s (embedding failed): %s", note.full_path, exc)
            errors += 1
        except Exception as exc:
            logger.error("Failed to index %s: %s", note.full_path, exc)
            errors += 1

    index.save()
    return f"Reindexed {count} notes ({errors} errors)"
