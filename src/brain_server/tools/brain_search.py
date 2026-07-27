"""MCP tool: brain_search — semantic search over indexed notes."""

from __future__ import annotations

import json
import logging

from mcp.server import FastMCP

from brain_server.embeddings.engine import EmbeddingEngine, EmbeddingError
from brain_server.index.store import VectorIndex

logger = logging.getLogger(__name__)


def register(server: FastMCP, embeddings: EmbeddingEngine, index: VectorIndex) -> None:
    """Register the brain_search tool."""

    @server.tool()
    async def brain_search(
        query: str,
        layer: str | None = None,
        scope: str | None = None,
        top_k: int = 5,
    ) -> str:
        """Search the brain vault by semantic similarity.

        Args:
            query: The search query text
            layer: Optional layer filter (arquitetura, regras, sessoes, projetos, indexacao)
            scope: Optional scope filter ('projetos' or 'global'). Only applies to 'arquitetura' and 'regras' layers.
            top_k: Maximum number of results (1-20, default 5)
        """
        if not query.strip():
            return "INVALID_PARAMS: query cannot be empty"

        if top_k < 1 or top_k > 20:
            top_k = 5

        # Generate embedding for query
        try:
            query_vec = await embeddings.embed(query)
        except EmbeddingError as exc:
            return f"EMBEDDING_FAILED: {exc}"

        # Search with optional scope filter
        results = index.search(query_vec, top_k=top_k, layer_filter=layer, scope_filter=scope)

        if not results:
            return json.dumps({"results": [], "total": 0}, ensure_ascii=False)

        payload = {
            "results": [
                {
                    "path": r.path,
                    "layer": r.layer,
                    "scope": r.scope if hasattr(r, 'scope') else None,
                    "score": r.score,
                    "snippet": r.snippet,
                    "chunk_index": r.chunk_index,
                }
                for r in results
            ],
            "total": len(results),
        }
        return json.dumps(payload, ensure_ascii=False)
