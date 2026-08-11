"""MCP tool: brain_store — save a note to the vault and index it."""

from __future__ import annotations

import logging
import re
from datetime import datetime, timezone

from mcp.server import FastMCP

from brain_server.embeddings.engine import EmbeddingEngine, EmbeddingError
from brain_server.index.store import VectorIndex
from brain_server.vault.manager import VaultManager
from brain_server.vault.models import LAYERS_WITH_SCOPE

logger = logging.getLogger(__name__)


def _build_frontmatter(
    project: str | None = None,
    tags: list[str] | None = None,
    scope: str | None = None,
    layer: str | None = None,
) -> str:
    """Build YAML frontmatter string from metadata."""
    lines = ["---"]
    if layer:
        lines.append(f"layer: {layer}")
    if scope:
        lines.append(f"scope: {scope}")
    if project:
        lines.append(f"project: {project}")
    if tags:
        lines.append(f"tags: [{', '.join(tags)}]")
    lines.append(f"created_at: {datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')}")
    lines.append("---")
    return "\n".join(lines)


def _inject_project_link(content: str, project: str) -> str:
    """Append Obsidian [[wiki-link]] to project at the end of content.

    Adds a '## Projeto' section with [[project-name]] link so Obsidian
    can resolve navigation between notes and project pages.
    """
    link_section = f"\n\n---\n## Projeto\n\n[[{project}]]"
    # Don't duplicate if already present
    if f"[[{project}]]" in content:
        return content
    return content + link_section


def _merge_frontmatter(content: str, frontmatter: str) -> str:
    """Merge new frontmatter into existing content.

    If content already has YAML frontmatter (--- ... ---), merge new fields.
    If not, prepend frontmatter.
    """
    fm_match = re.match(r"^---\n(.+?)\n---\n?", content, re.DOTALL)
    if fm_match:
        # Content already has frontmatter — parse and merge
        existing_fm = fm_match.group(1)
        body = content[fm_match.end():]

        # Extract existing keys
        existing = {}
        for line in existing_fm.split("\n"):
            if ":" in line:
                key, val = line.split(":", 1)
                existing[key.strip()] = val.strip()

        # Extract new keys from frontmatter
        new_keys = {}
        for line in frontmatter.split("\n"):
            if line.strip() == "---":
                continue
            if ":" in line:
                key, val = line.split(":", 1)
                new_keys[key.strip()] = val.strip()

        # Merge: new keys override existing
        existing.update(new_keys)

        # Rebuild frontmatter
        merged_lines = ["---"]
        for key, val in existing.items():
            merged_lines.append(f"{key}: {val}")
        merged_lines.append("---")
        return "\n".join(merged_lines) + "\n" + body
    else:
        # No existing frontmatter — prepend
        return frontmatter + "\n\n" + content


def register(server: FastMCP, vault: VaultManager, embeddings: EmbeddingEngine, index: VectorIndex) -> None:
    """Register the brain_store tool."""

    @server.tool()
    async def brain_store(
        layer: str,
        path: str,
        content: str,
        scope: str | None = None,
        project: str | None = None,
        tags: str | None = None,
    ) -> str:
        """Save a note to the brain vault and index it for semantic search.

        Args:
            layer: Vault layer (arquitetura, regras, sessoes, projetos, indexacao)
            path: Relative path without extension (e.g. "projeto-x/regra-1")
            content: Markdown content
            scope: Required for 'arquitetura' and 'regras' layers. Use 'projetos' for project-specific content or 'global' for shared content.
            project: Project name this note belongs to (registers automatically if new)
            tags: Comma-separated tags for categorization (e.g. "java,fundamentos,oo")
        """
        # Validate scope for layers that require it
        if layer in LAYERS_WITH_SCOPE and scope is None:
            return f"INVALID_PARAMS: Layer '{layer}' requires scope parameter ('projetos' or 'global')"

        # Resolve project_id if project name provided
        project_id = None
        if project:
            proj = index.project_get(project)
            if not proj:
                try:
                    proj = index.project_create(project)
                except ValueError as exc:
                    return f"INVALID_PARAMS: {exc}"
            project_id = proj.id

        # Parse tags
        tags_list = [t.strip() for t in tags.split(",") if t.strip()] if tags else None

        # Build frontmatter and merge into content for vault storage
        vault_content = content
        if project or tags_list or scope:
            frontmatter = _build_frontmatter(
                project=project, tags=tags_list, scope=scope, layer=layer,
            )
            vault_content = _merge_frontmatter(content, frontmatter)

        # Inject Obsidian [[project]] link for navigation
        if project:
            vault_content = _inject_project_link(vault_content, project)

        # Write to vault
        try:
            note = vault.write(layer, path, vault_content, scope=scope)
        except ValueError as exc:
            return f"INVALID_PARAMS: {exc}"

        # Embed and index (use original content, not vault_content with frontmatter)
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
                    project_id=project_id,
                    tags=tags_list,
                )

            index.save()
            logger.info(
                "Indexed '%s' — %d chunks, project=%s, tags=%s",
                note.full_path, len(chunks), project, tags_list,
            )
        except EmbeddingError as exc:
            logger.warning("Note saved but indexing deferred: %s", exc)
            return f"Note saved but indexing deferred: {exc}"

        return f"ok — {note.full_path}"
