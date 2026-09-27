"""MCP tool: brain_reindex — rebuild vector index partially or fully (background)."""

from __future__ import annotations

import asyncio
import logging

from mcp.server import FastMCP

from brain_server.embeddings.engine import EmbeddingEngine, EmbeddingError
from brain_server.index.store import VectorIndex
from brain_server.tools.brain_store import _inject_project_link
from brain_server.vault.manager import VaultManager
from brain_server.vault.models import LAYERS_WITH_SCOPE, VALID_SCOPES, parse_frontmatter

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
        """Rebuild the vector index (partial or full) in background.

        Returns immediately; heavy embedding work runs asynchronously.
        If a reindex is already running, returns REINDEX_IN_PROGRESS.

        Args:
            all: If True, reindex every file in the vault
            layer: If provided (and all=False), reindex only this layer
            path: If provided (and all=False, layer=None), reindex specific file
        """
        # Validate parameters — sync, no await
        params = [all, layer is not None, path is not None]
        if sum(params) != 1:
            return (
                "INVALID_PARAMS: provide exactly one of all=True, layer=<name>, or path=<path>"
            )

        # Guard against concurrent reindex operations (lock + pending task)
        if index.reindexing:
            return "REINDEX_IN_PROGRESS: another reindex operation is already running"

        # Quick synchronous validation for fast error feedback (before background)
        if path is not None:
            if "/" not in path:
                return "INVALID_PARAMS: path must include layer, e.g. 'arquitetura/projeto-x'"
            layer_name, rel_path = path.split("/", 1)
            if rel_path.endswith(".md"):
                rel_path = rel_path[:-3]
            scope = _extract_scope_from_path(layer_name, rel_path)
            try:
                vault.read(layer_name, rel_path, scope=scope)
            except ValueError as exc:
                return f"INVALID_PARAMS: {exc}"
            except FileNotFoundError as exc:
                return f"NOT_FOUND: {exc}"

        if layer is not None:
            from brain_server.vault.models import VALID_LAYERS
            if layer not in VALID_LAYERS:
                return f"INVALID_PARAMS: Invalid layer '{layer}'. Valid: {', '.join(sorted(VALID_LAYERS))}"

        # Capture values for closure (avoid shadowing builtin `all`)
        all_flag = all
        layer_flag = layer
        path_flag = path

        async def _background() -> None:
            async with index._reindex_lock:
                try:
                    if all_flag:
                        files = vault.list_all_files()
                        logger.info("Background reindexing all %d files", len(files))
                        result = await _reindex_files(vault, embeddings, index, files)
                        logger.info("Background reindex complete: %s", result)
                    elif layer_flag is not None:
                        notes = vault.list_notes(layer=layer_flag)
                        logger.info("Background reindexing layer '%s' (%d notes)", layer_flag, len(notes))
                        result = await _reindex_notes(vault, embeddings, index, notes)
                        logger.info("Background reindex complete: %s", result)
                    elif path_flag is not None:
                        if "/" not in path_flag:
                            logger.error("Background reindex invalid path: %s", path_flag)
                            return
                        layer_name, rel_path = path_flag.split("/", 1)
                        if rel_path.endswith(".md"):
                            rel_path = rel_path[:-3]
                        scope = _extract_scope_from_path(layer_name, rel_path)
                        note = vault.read(layer_name, rel_path, scope=scope)
                        result = await _reindex_notes(vault, embeddings, index, [note])
                        logger.info("Background reindex complete: %s", result)
                except ValueError as exc:
                    logger.error("Background reindex invalid params: %s", exc)
                except FileNotFoundError as exc:
                    logger.error("Background reindex not found: %s", exc)
                except Exception as exc:
                    logger.error("Background reindex failed: %s", exc, exc_info=True)

        # Create task synchronously — sets _reindex_task before any await,
        # so concurrent calls see reindexing=True even before lock is acquired.
        task = asyncio.create_task(_background())

        def _done_callback(t: asyncio.Task) -> None:
            try:
                t.result()
            except Exception as exc:
                logger.error("Background reindex task crashed: %s", exc, exc_info=True)

        task.add_done_callback(_done_callback)
        index._reindex_task = task  # type: ignore[attr-defined]

        # Immediate response — no timeout
        if all_flag:
            return "REINDEX_STARTED: reindex of all files started in background"
        if layer_flag is not None:
            return f"REINDEX_STARTED: reindex of layer '{layer_flag}' started in background"
        return f"REINDEX_STARTED: reindex of '{path_flag}' started in background"


async def _reindex_files(
    vault: VaultManager,
    embeddings: EmbeddingEngine,
    index: VectorIndex,
    files: list,
) -> str:
    """Reindex files from a list of Path objects.

    Parses frontmatter for project/tags, computes embeddings concurrently,
    then swaps old↔new atomically per file.
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

            # Parse frontmatter for project/tags (Bug 2 residual fix)
            fm = parse_frontmatter(content)
            project_id = None
            project_name = None
            if fm["project"]:
                project_name = fm["project"]
                proj = index.project_get(project_name)
                if proj:
                    project_id = proj.id
                else:
                    try:
                        proj = index.project_create(project_name)
                        project_id = proj.id
                    except ValueError:
                        pass  # project name conflict, skip
            tags_list = fm["tags"] or None

            # Inject [[project]] link if missing (Obsidian navigation)
            if project_name and f"[[{project_name}]]" not in content:
                content = _inject_project_link(content, project_name)
                f.write_text(content, encoding="utf-8")

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
                    project_id=project_id,
                    tags=tags_list,
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

    Parses frontmatter for project/tags, computes embeddings concurrently,
    then swaps old↔new atomically per note.
    """
    count = 0
    errors = 0

    for note in notes:
        try:
            # Parse frontmatter for project/tags (Bug 2 residual fix)
            fm = parse_frontmatter(note.content)
            project_id = None
            project_name = None
            if fm["project"]:
                project_name = fm["project"]
                proj = index.project_get(project_name)
                if proj:
                    project_id = proj.id
                else:
                    try:
                        proj = index.project_create(project_name)
                        project_id = proj.id
                    except ValueError:
                        pass
            tags_list = fm["tags"] or None

            # Inject [[project]] link into content for indexing
            index_content = note.content
            if project_name and f"[[{project_name}]]" not in index_content:
                index_content = _inject_project_link(index_content, project_name)

            chunks = embeddings.chunk_text(index_content)
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
                    project_id=project_id,
                    tags=tags_list,
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
