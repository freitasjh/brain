"""Data models for vault notes and layer validation."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

VALID_LAYERS: frozenset[str] = frozenset({
    "arquitetura",
    "regras",
    "sessoes",
    "projetos",
    "estudos",
    "indexacao",
})

# Layers that require scope (projetos or global)
LAYERS_WITH_SCOPE: frozenset[str] = frozenset({
    "arquitetura",
    "regras",
    "estudos",
})

# Scope constants
SCOPE_PROJECT = "projetos"
SCOPE_GLOBAL = "global"
VALID_SCOPES: frozenset[str] = frozenset({
    SCOPE_PROJECT,
    SCOPE_GLOBAL,
})


@dataclass
class Note:
    """A single markdown note in the vault."""

    layer: str
    path: str  # relative path without extension, e.g. "projeto-x/banco-de-dados"
    content: str
    scope: str | None = None  # "projetos" or "global" (required for arquitetura/regras)
    metadata: dict = field(default_factory=dict)

    @property
    def full_path(self) -> str:
        if self.scope and self.layer in LAYERS_WITH_SCOPE:
            return f"{self.layer}/{self.scope}/{self.path}.md"
        return f"{self.layer}/{self.path}.md"


def validate_layer(layer: str) -> None:
    """Raise ValueError if layer is not in VALID_LAYERS."""
    if layer not in VALID_LAYERS:
        valid = ", ".join(sorted(VALID_LAYERS))
        raise ValueError(f"Invalid layer '{layer}'. Valid layers: {valid}")


def validate_scope(scope: str) -> None:
    """Raise ValueError if scope is not in VALID_SCOPES."""
    if scope not in VALID_SCOPES:
        valid = ", ".join(sorted(VALID_SCOPES))
        raise ValueError(f"Invalid scope '{scope}'. Valid scopes: {valid}")


def sanitize_relative_path(raw: str) -> str:
    """Sanitize a user-provided path to prevent directory traversal.

    Raises ValueError if the path attempts to escape via '..' or absolute paths.
    """
    p = Path(raw)
    if p.is_absolute():
        raise ValueError(f"Absolute paths not allowed: {raw}")
    if ".." in p.parts:
        raise ValueError(f"Path traversal detected: {raw}")
    # Normalize to forward-slash relative
    return str(p)
