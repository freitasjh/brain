"""VaultManager — orchestrates CRUD operations on the Obsidian vault."""

from __future__ import annotations

import logging
from pathlib import Path

from brain_server.vault.models import Note, sanitize_relative_path, validate_layer, validate_scope, LAYERS_WITH_SCOPE

logger = logging.getLogger(__name__)


class VaultManager:
    """Manages reading and writing markdown notes inside a layered vault directory."""

    def __init__(self, vault_path: Path) -> None:
        self.vault_path = vault_path.resolve()
        self._ensure_layers()

    def _ensure_layers(self) -> None:
        """Create layer subdirectories if they don't exist."""
        from brain_server.vault.models import VALID_LAYERS, SCOPE_PROJECT, SCOPE_GLOBAL

        for layer in VALID_LAYERS:
            layer_dir = self.vault_path / layer
            layer_dir.mkdir(parents=True, exist_ok=True)
            
            # Create scope subdirectories for layers that require scope
            if layer in LAYERS_WITH_SCOPE:
                (layer_dir / SCOPE_PROJECT).mkdir(parents=True, exist_ok=True)
                (layer_dir / SCOPE_GLOBAL).mkdir(parents=True, exist_ok=True)

    def _resolve(self, layer: str, path: str, scope: str | None = None) -> Path:
        """Resolve layer+path to an absolute filesystem path.

        Validates layer, sanitizes path, and ensures the result is inside vault_path.
        For layers in LAYERS_WITH_SCOPE, scope is required.
        """
        validate_layer(layer)
        
        # Sanitize the original path first (before adding scope)
        safe_path = sanitize_relative_path(path)
        
        # Build full path with scope if required
        if layer in LAYERS_WITH_SCOPE:
            if scope is None:
                raise ValueError(f"Layer '{layer}' requires scope ('projetos' or 'global')")
            validate_scope(scope)
            full_path = f"{scope}/{safe_path}"
        else:
            full_path = safe_path
        
        full = (self.vault_path / layer / full_path).with_suffix(".md").resolve()

        # Security: ensure resolved path is within vault (handles symlinks)
        vault_resolved = self.vault_path.resolve()
        if vault_resolved not in full.parents and full != vault_resolved:
            raise ValueError(f"Path escapes vault directory: {path}")

        return full

    def write(self, layer: str, path: str, content: str, scope: str | None = None) -> Note:
        """Write a markdown note. Creates intermediate directories if needed."""
        full = self._resolve(layer, path, scope)
        full.parent.mkdir(parents=True, exist_ok=True)
        full.write_text(content, encoding="utf-8")
        note = Note(layer=layer, path=path, content=content, scope=scope)
        logger.info("Wrote note: %s", note.full_path)
        return note

    def read(self, layer: str, path: str, scope: str | None = None) -> Note:
        """Read a markdown note. Raises FileNotFoundError if missing."""
        full = self._resolve(layer, path, scope)
        if not full.exists():
            raise FileNotFoundError(f"Note not found: {layer}/{path}")
        content = full.read_text(encoding="utf-8")
        logger.info("Read note: %s/%s", layer, path)
        return Note(layer=layer, path=path, content=content, scope=scope)

    def delete(self, layer: str, path: str, scope: str | None = None) -> None:
        """Delete a markdown note."""
        full = self._resolve(layer, path, scope)
        if full.exists():
            full.unlink()
            logger.info("Deleted note: %s/%s", layer, path)
        else:
            raise FileNotFoundError(f"Note not found: {layer}/{path}")

    def list_notes(self, layer: str | None = None, scope: str | None = None) -> list[Note]:
        """List all notes, optionally filtered by layer and scope."""
        from brain_server.vault.models import VALID_LAYERS, VALID_SCOPES

        layers_to_scan = [layer] if layer else list(VALID_LAYERS)
        results: list[Note] = []

        for l in layers_to_scan:
            layer_dir = self.vault_path / l
            if not layer_dir.exists():
                continue
            
            # Determine search directory based on scope
            if l in LAYERS_WITH_SCOPE and scope:
                if scope not in VALID_SCOPES:
                    raise ValueError(f"Invalid scope '{scope}'. Valid scopes: {', '.join(sorted(VALID_SCOPES))}")
                search_dir = layer_dir / scope
            else:
                search_dir = layer_dir
            
            if not search_dir.exists():
                continue
            
            for md_file in search_dir.rglob("*.md"):
                # relative path without extension
                rel = md_file.relative_to(layer_dir)
                rel_stem = str(rel.with_suffix(""))
                content = md_file.read_text(encoding="utf-8")
                
                # Detect scope for layers that use it
                detected_scope = None
                if l in LAYERS_WITH_SCOPE and rel.parts:
                    first_part = rel.parts[0]
                    if first_part in VALID_SCOPES:
                        detected_scope = first_part
                        # Remove scope from path
                        rel_stem = str(rel.relative_to(first_part).with_suffix(""))
                
                results.append(Note(layer=l, path=rel_stem, content=content, scope=detected_scope))

        return results

    def list_all_files(self) -> list[Path]:
        """Return all .md files across all layers."""
        from brain_server.vault.models import VALID_LAYERS

        files: list[Path] = []
        for layer in VALID_LAYERS:
            layer_dir = self.vault_path / layer
            if layer_dir.exists():
                files.extend(layer_dir.rglob("*.md"))
        return files
