"""MCP tools: brain_project_* — manage projects and note-project relationships.

Also generates Obsidian project index pages with [[wiki-links]] to all
notes belonging to each project.
"""

from __future__ import annotations

import json
import logging

from mcp.server import FastMCP

from brain_server.index.store import VectorIndex
from brain_server.vault.manager import VaultManager

logger = logging.getLogger(__name__)

# Layer display names for the project index page
_LAYER_LABELS = {
    "arquitetura": "Arquitetura",
    "regras": "Regras",
    "estudos": "Estudos",
    "sessoes": "Sessoes",
    "projetos": "Projeto",
    "indexacao": "Indexacao",
}


def _build_project_page(
    project_name: str,
    description: str,
    notes: list[dict],
) -> str:
    """Generate Obsidian project index page with [[wiki-links]] to all notes.

    The page is structured so Obsidian's graph view shows connections between
    the project and all its notes across layers.
    """
    lines = [
        "---",
        f"tags: [{project_name}, projeto, visao-geral]",
        "layer: projetos",
        "---",
        "",
        f"# {project_name}",
        "",
    ]

    if description:
        lines.extend([description, ""])

    # Group notes by layer
    by_layer: dict[str, list[dict]] = {}
    for n in notes:
        layer = n["layer"]
        by_layer.setdefault(layer, []).append(n)

    if by_layer:
        lines.append("## Notas")
        lines.append("")
        for layer in ["arquitetura", "regras", "estudos", "sessoes", "projetos", "indexacao"]:
            layer_notes = by_layer.get(layer, [])
            if not layer_notes:
                continue
            label = _LAYER_LABELS.get(layer, layer)
            lines.append(f"### {label}")
            lines.append("")
            for n in layer_notes:
                # Wiki-link: [[layer/scope/path]] or [[layer/path]]
                note_path = n["path"]
                scope = n.get("scope")
                if scope:
                    link = f"[[{layer}/{scope}/{note_path}]]"
                else:
                    link = f"[[{layer}/{note_path}]]"
                snippet = (n.get("snippet") or "")[:80].replace("\n", " ")
                lines.append(f"- {link} — {snippet}")
            lines.append("")

    # Cross-project links (linked global notes)
    linked = [n for n in notes if n.get("source") == "linked"]
    if linked:
        lines.append("## Notas compartilhadas")
        lines.append("")
        for n in linked:
            note_path = n["path"]
            layer = n["layer"]
            scope = n.get("scope")
            if scope:
                link = f"[[{layer}/{scope}/{note_path}]]"
            else:
                link = f"[[{layer}/{note_path}]]"
            lines.append(f"- {link}")
        lines.append("")

    return "\n".join(lines)


def _sync_project_page(
    vault: VaultManager,
    index: VectorIndex,
    project_name: str,
) -> None:
    """Generate or update the Obsidian project index page."""
    project = index.project_get(project_name)
    if not project:
        return

    notes = index.project_notes(project_name)
    page_content = _build_project_page(
        project_name=project.name,
        description=project.description,
        notes=notes,
    )

    # Write to vault (overwrite if exists)
    vault.write("projetos", project.name, page_content)
    logger.info("Synced project page: projetos/%s.md (%d notes)", project.name, len(notes))


def register(server: FastMCP, index: VectorIndex, vault: VaultManager) -> None:
    """Register brain_project tools."""

    @server.tool()
    async def brain_project_create(name: str, description: str = "") -> str:
        """Register a new project in the brain and create its Obsidian index page.

        Args:
            name: Project name (e.g. "meu-app", "fusion-api")
            description: Optional description
        """
        try:
            project = index.project_create(name, description)
            _sync_project_page(vault, index, project.name)
            return f"ok — project '{project.name}' created (id={project.id})"
        except ValueError as exc:
            return f"INVALID_PARAMS: {exc}"

    @server.tool()
    async def brain_project_list() -> str:
        """List all registered projects."""
        projects = index.project_list()
        if not projects:
            return json.dumps({"projects": [], "total": 0}, ensure_ascii=False)

        payload = {
            "projects": [
                {
                    "id": p.id,
                    "name": p.name,
                    "description": p.description,
                    "created_at": p.created_at,
                }
                for p in projects
            ],
            "total": len(projects),
        }
        return json.dumps(payload, ensure_ascii=False)

    @server.tool()
    async def brain_project_notes(project: str) -> str:
        """List all notes belonging to a project and sync its Obsidian index page.

        Args:
            project: Project name
        """
        notes = index.project_notes(project)

        # Sync project page with current notes
        _sync_project_page(vault, index, project)

        if not notes:
            return json.dumps({"project": project, "notes": [], "total": 0}, ensure_ascii=False)

        payload = {
            "project": project,
            "notes": [
                {
                    "path": n["path"],
                    "layer": n["layer"],
                    "scope": n["scope"],
                    "snippet": (n["snippet"] or "")[:100],
                    "tags": n["tags"],
                    "source": n["source"],
                }
                for n in notes
            ],
            "total": len(notes),
        }
        return json.dumps(payload, ensure_ascii=False)

    @server.tool()
    async def brain_project_link(note_path: str, project: str) -> str:
        """Link a global note to a project and update the project index page.

        Args:
            note_path: Full note path (e.g. "estudos/global/java/oo.md")
            project: Project name to link to
        """
        try:
            index.note_link_project(note_path, project)
            _sync_project_page(vault, index, project)
            return f"ok — linked '{note_path}' to project '{project}'"
        except ValueError as exc:
            return f"INVALID_PARAMS: {exc}"

    @server.tool()
    async def brain_project_unlink(note_path: str, project: str) -> str:
        """Remove link between a global note and a project.

        Args:
            note_path: Full note path
            project: Project name to unlink from
        """
        removed = index.note_unlink_project(note_path, project)
        if removed:
            _sync_project_page(vault, index, project)
            return f"ok — unlinked '{note_path}' from project '{project}'"
        return f"NOT_FOUND: no link between '{note_path}' and '{project}'"

    @server.tool()
    async def brain_project_delete(name: str) -> str:
        """Delete a project by name and remove its Obsidian index page.

        Args:
            name: Project name to delete
        """
        deleted = index.project_delete(name)
        if deleted:
            # Remove project index page from vault
            try:
                vault.delete("projetos", name)
            except FileNotFoundError:
                pass
            return f"ok — project '{name}' deleted"
        return f"NOT_FOUND: project '{name}' not found"
