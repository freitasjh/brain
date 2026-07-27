"""CLI tool for the Brain MCP server — professional CLI with typer + rich.

Usage:
    brain init mcp --port 8321 --vault ./vault
    brain init project "meu-app" --all
    brain ping
    brain search "regras de banco" -k 5 --json
    brain server start --daemon
    brain config show
"""

from brain_server.cli.main import app, main

__all__ = ["app", "main"]
