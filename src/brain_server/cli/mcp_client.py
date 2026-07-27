"""MCP SSE client for communicating with the brain server."""

from __future__ import annotations

import json
import os

import anyio
from mcp.client.session import ClientSession
from mcp.client.sse import sse_client


class MCPError(Exception):
    """Raised when an MCP tool call fails."""


class MCPClient:
    """Synchronous wrapper around async MCP SSE client.

    Usage:
        client = MCPClient("http://localhost:8321")
        result = client.call("ping")
        result = client.call("brain_search", {"query": "test", "top_k": 5})
    """

    def __init__(self, url: str | None = None):
        self.url = self._build_url(url or os.getenv("BRAIN_URL", "http://localhost:8321"))

    @staticmethod
    def _build_url(base: str) -> str:
        """Ensure URL points to the SSE endpoint (/sse)."""
        base = base.rstrip("/")
        if not base.endswith("/sse"):
            base += "/sse"
        return base

    def call(self, tool_name: str, arguments: dict | None = None) -> str:
        """Call an MCP tool and return the result text."""
        return anyio.run(self._call_async, tool_name, arguments)

    async def _call_async(self, tool_name: str, arguments: dict | None = None) -> str:
        """Async implementation of MCP tool call via SSE."""
        try:
            async with sse_client(url=self.url) as (read, write):
                async with ClientSession(read, write) as session:
                    await session.initialize()
                    result = await session.call_tool(tool_name, arguments=arguments or {})
                    if result.content and hasattr(result.content[0], "text"):
                        return result.content[0].text
                    if result.content:
                        return str(result.content[0])
                    return "(empty response)"
        except Exception as e:
            raise MCPError(f"Failed to call '{tool_name}' at {self.url}: {e}") from e

    def call_json(self, tool_name: str, arguments: dict | None = None) -> dict:
        """Call an MCP tool and parse result as JSON."""
        text = self.call(tool_name, arguments)
        try:
            return json.loads(text)
        except json.JSONDecodeError as e:
            raise MCPError(f"Expected JSON response, got: {text[:200]}") from e
