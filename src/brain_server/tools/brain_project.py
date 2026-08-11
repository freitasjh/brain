"""MCP tools: brain_project_* — manage projects and note-project relationships."""

from __future__ import annotations

import json
import logging

from mcp.server import FastMCP

from brain_server.index.store import VectorIndex

logger = logging.getLogger(__name__)


def register(server: FastMCP, index: VectorIndex) -> None:
    """Register brain_project tools."""

    @server.tool()
    async def brain_project_create(name: str, description: str = "") -> str:
        """Register a new project in the brain.

        Args:
            name: Project name (e.g. "meu-app", "fusion-api")
            description: Optional description
        """
        try:
            project = index.project_create(name, description)
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
        """List all notes belonging to a project (both owned and linked global notes).

        Args:
            project: Project name
        """
        notes = index.project_notes(project)
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
        """Link a global note to a project (for notes shared across projects).

        Args:
            note_path: Full note path (e.g. "estudos/global/java/oo.md")
            project: Project name to link to
        """
        try:
            index.note_link_project(note_path, project)
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
            return f"ok — unlinked '{note_path}' from project '{project}'"
        return f"NOT_FOUND: no link between '{note_path}' and '{project}'"

    @server.tool()
    async def brain_project_delete(name: str) -> str:
        """Delete a project by name.

        Args:
            name: Project name to delete
        """
        deleted = index.project_delete(name)
        if deleted:
            return f"ok — project '{name}' deleted"
        return f"NOT_FOUND: project '{name}' not found"
