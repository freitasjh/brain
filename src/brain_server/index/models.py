"""Data models for the vector index."""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass
class IndexEntry:
    """A single entry in the vector index (one chunk of one note)."""

    path: str  # e.g. "arquitetura/projetos/projeto-x/banco-de-dados"
    layer: str
    embedding: list[float]  # vector (e.g. 768 dimensions)
    snippet: str = ""  # first ~200 chars of the chunk
    chunk_index: int = 0
    total_chunks: int = 1
    scope: str | None = None  # "projetos" or "global" (for arquitetura/regras layers)


@dataclass
class SearchResult:
    """Result of a semantic search query."""

    path: str
    layer: str
    score: float  # cosine similarity 0..1
    snippet: str  # first ~200 chars of the chunk
    chunk_index: int = 0
    scope: str | None = None  # "projetos" or "global" (for arquitetura/regras layers)
