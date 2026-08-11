"""MCP server setup and lifecycle."""

from __future__ import annotations

import asyncio
import logging

from mcp.server import FastMCP

from brain_server.config import Settings
from brain_server.embeddings.engine import EmbeddingEngine
from brain_server.index.store import VectorIndex
from brain_server.vault.manager import VaultManager

logger = logging.getLogger(__name__)


def create_server(settings: Settings) -> FastMCP:
    """Create and configure the MCP server with all tools registered."""
    # Initialize core components
    vault = VaultManager(settings.vault_path)
    embeddings = EmbeddingEngine(
        base_url=settings.ollama_url,
        model=settings.ollama_model,
    )

    # Check Ollama availability (non-blocking, best-effort)
    async def _check_ollama():
        if await embeddings.health_check():
            logger.info("Ollama reachable at %s", settings.ollama_url)
        else:
            logger.warning(
                "Ollama unreachable at %s — starting in degraded mode (embeddings offline)",
                settings.ollama_url,
            )
    try:
        loop = asyncio.get_running_loop()
        loop.create_task(_check_ollama())
    except RuntimeError:
        pass  # no running event loop (stdio mode)

    index = VectorIndex(settings.index_path)
    index.load()

    # Auto-rebuild if index is empty but vault has files (e.g. index.db deleted)
    vault_files = vault.list_all_files()
    if index.size() == 0 and vault_files:
        logger.info(
            "Index empty but vault has %d files — triggering auto-rebuild",
            len(vault_files),
        )

        async def _auto_rebuild():
            from brain_server.tools.brain_reindex import _extract_scope_from_path
            from brain_server.tools.brain_store import _inject_project_link
            from brain_server.vault.models import parse_frontmatter
            
            for f in vault_files:
                try:
                    content = f.read_text(encoding="utf-8")
                    rel = f.relative_to(vault.vault_path)
                    layer = rel.parts[0]
                    path_stem = str(rel.with_suffix(""))
                    # path_stem is already relative to vault root (Bug 1 fix)
                    full_vault_path = f"{path_stem}.md"
                    
                    # Extract scope if layer requires it
                    scope = _extract_scope_from_path(layer, path_stem)

                    # Parse frontmatter for project/tags
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
                                pass
                    tags_list = fm["tags"] or None

                    # Inject [[project]] link if missing
                    if project_name and f"[[{project_name}]]" not in content:
                        content = _inject_project_link(content, project_name)
                        f.write_text(content, encoding="utf-8")

                    chunks = embeddings.chunk_text(content)
                    vecs = await embeddings.embed_batch_concurrent(chunks)
                    index.remove(full_vault_path)

                    for i, (ct, vec) in enumerate(zip(chunks, vecs)):
                        index.upsert(
                            path=full_vault_path,
                            layer=layer,
                            embedding=vec,
                            snippet=ct,
                            chunk_index=i,
                            total_chunks=len(chunks),
                            scope=scope,
                            project_id=project_id,
                            tags=tags_list,
                        )
                except Exception as exc:
                    logger.warning("Auto-rebuild skipped %s: %s", f, exc)
            index.save()
            logger.info("Auto-rebuild complete — %d entries", index.size())

            # Sync Obsidian project index pages
            from brain_server.tools.brain_project import _sync_project_page
            for proj in index.project_list():
                try:
                    _sync_project_page(vault, index, proj.name)
                except Exception as exc:
                    logger.warning("Failed to sync project page '%s': %s", proj.name, exc)

        try:
            loop = asyncio.get_running_loop()
            loop.create_task(_auto_rebuild())
        except RuntimeError:
            pass  # no running event loop (fallback: manual reindex needed)

    server = FastMCP("brain", port=settings.port)

    @server.tool()
    async def ping() -> str:
        """Health-check: always returns 'pong'."""
        return "pong"

    # Register brain tools
    from brain_server.tools import brain_read, brain_search, brain_store
    from brain_server.tools import brain_reindex
    from brain_server.tools import brain_project

    brain_store.register(server, vault, embeddings, index)
    brain_read.register(server, vault)
    brain_search.register(server, embeddings, index)
    brain_reindex.register(server, vault, embeddings, index)
    brain_project.register(server, index, vault)

    logger.info(
        "Server initialized — vault=%s index=%d entries",
        settings.vault_path,
        index.size(),
    )

    return server


def main() -> None:
    """Start the Brain MCP server."""
    from brain_server.config import settings

    logging.basicConfig(
        level=getattr(logging, settings.log_level.upper(), logging.INFO),
        format="%(levelname)s  %(name)s  %(message)s",
    )

    logger.info(
        "Brain MCP server starting — vault=%s ollama=%s transport=%s",
        settings.vault_path,
        settings.ollama_url,
        settings.transport,
    )

    server = create_server(settings)
    server.run(transport=settings.transport)
