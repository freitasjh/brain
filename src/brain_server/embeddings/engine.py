"""EmbeddingEngine — generates embeddings via Ollama API."""

from __future__ import annotations

import logging
import re

import httpx

logger = logging.getLogger(__name__)


class EmbeddingError(Exception):
    """Raised when embedding generation fails."""


class EmbeddingEngine:
    """Client for Ollama embeddings API."""

    def __init__(
        self,
        base_url: str = "http://localhost:11434",
        model: str = "nomic-embed-text",
        timeout: int = 30,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.model = model
        self.timeout = timeout

    async def health_check(self) -> bool:
        """Check if Ollama is reachable by listing available models."""
        try:
            async with httpx.AsyncClient(timeout=5) as client:
                resp = await client.get(f"{self.base_url}/api/tags")
                return resp.status_code == 200
        except Exception:
            return False

    async def embed(self, text: str) -> list[float]:
        """Generate embedding vector for a single text string."""
        if not text.strip():
            raise EmbeddingError("Cannot embed empty text")

        async with httpx.AsyncClient(timeout=self.timeout) as client:
            try:
                resp = await client.post(
                    f"{self.base_url}/api/embeddings",
                    json={"model": self.model, "prompt": text},
                )
                resp.raise_for_status()
                data = resp.json()
                return data["embedding"]
            except httpx.TimeoutException as exc:
                raise EmbeddingError(
                    f"Ollama timeout after {self.timeout}s"
                ) from exc
            except httpx.HTTPStatusError as exc:
                raise EmbeddingError(
                    f"Ollama HTTP {exc.response.status_code}: {exc.response.text}"
                ) from exc
            except (KeyError, ValueError) as exc:
                raise EmbeddingError(
                    f"Unexpected Ollama response: {exc}"
                ) from exc

    async def embed_batch(self, texts: list[str]) -> list[list[float]]:
        """Generate embeddings for multiple texts sequentially.

        Ollama does not have a native batch endpoint, so we loop.
        """
        results: list[list[float]] = []
        for t in texts:
            vec = await self.embed(t)
            results.append(vec)
        return results

    @staticmethod
    def chunk_text(text: str, max_tokens: int = 4096) -> list[str]:
        """Split markdown text into chunks by ## sections.

        Each chunk is truncated to max_tokens (rough estimate via word count).
        """
        # Split on ## headings (but not ### or more)
        sections = re.split(r"(?=^## )", text, flags=re.MULTILINE)

        chunks: list[str] = []
        for section in sections:
            section = section.strip()
            if not section:
                continue
            # Rough token estimation: ~1 token per 4 chars
            if len(section) > max_tokens * 4:
                # Truncate to max_tokens
                section = section[: max_tokens * 4]
            chunks.append(section)

        return chunks if chunks else [text[: max_tokens * 4]]
