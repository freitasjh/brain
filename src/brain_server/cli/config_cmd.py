"""Configuration commands (show/set)."""

from __future__ import annotations

import os
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from rich.console import Console


# Well-known env vars with descriptions
KNOWN_VARS = {
    "BRAIN_PORT": ("8321", "SSE server port"),
    "BRAIN_VAULT_PATH": ("./vault", "Vault directory path"),
    "BRAIN_OLLAMA_URL": ("http://localhost:11434", "Ollama server URL"),
    "BRAIN_OLLAMA_MODEL": ("nomic-embed-text", "Embedding model name"),
    "BRAIN_INDEX_PATH": ("./data/index.db", "Vector index file (SQLite)"),
    "BRAIN_TRANSPORT": ("sse", "Transport (stdio or sse)"),
    "BRAIN_LOG_LEVEL": ("INFO", "Log level"),
    "BRAIN_URL": ("http://localhost:8321", "Server URL for CLI"),
}

CONFIG_DIR = Path.home() / ".brain"
CONFIG_FILE = CONFIG_DIR / "config.env"


def show_config(vault_path: str | None, console: Console) -> None:
    """Show current brain configuration."""
    from rich.table import Table
    from rich.panel import Panel

    # Load global config
    global_config = {}
    if CONFIG_FILE.exists():
        for line in CONFIG_FILE.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                key, val = line.split("=", 1)
                global_config[key.strip()] = val.strip()

    table = Table(title="🧠 Brain Configuration", title_justify="left")
    table.add_column("Variable", style="bold cyan")
    table.add_column("Current Value", style="green")
    table.add_column("Default", style="dim")
    table.add_column("Source", style="yellow")
    table.add_column("Description", style="white")

    for var, (default, desc) in KNOWN_VARS.items():
        # Priority: env > global config > default
        current = os.getenv(var)
        source = "env"
        if current is None:
            current = global_config.get(var)
            source = "~/.brain/config.env"
        if current is None:
            current = default
            source = "default"

        # For vault path, resolve the value
        if var == "BRAIN_VAULT_PATH" and vault_path:
            current = str(Path(vault_path).resolve())
            source = "argument"

        table.add_row(var, str(current), str(default), source, desc)

    console.print(Panel(table, title="🧠 Brain Config"))
    console.print(f"\n[dim]Global config file: {CONFIG_FILE}[/dim]")
    console.print("[dim]Set a value: [bold]brain config set BRAIN_PORT 9000[/bold][/dim]")


def set_config(key: str, value: str, global_cfg: bool, console: Console) -> None:
    """Set a brain configuration value."""
    key = key.upper()
    if not key.startswith("BRAIN_"):
        # Auto-prefix if user forgets
        key = f"BRAIN_{key}"

    if global_cfg:
        # Save to ~/.brain/config.env
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)

        config = {}
        if CONFIG_FILE.exists():
            for line in CONFIG_FILE.read_text().splitlines():
                if "=" in line and not line.strip().startswith("#"):
                    k, v = line.split("=", 1)
                    config[k.strip()] = v.strip()

        config[key] = value

        lines = []
        for k, v in config.items():
            desc = KNOWN_VARS.get(k)
            if desc:
                lines.append(f"# {desc[1]}")
            lines.append(f"{k}={v}")

        CONFIG_FILE.write_text("\n".join(lines) + "\n")
        console.print(f"[green]✅[/green] [bold]{key}[/bold] saved to [cyan]{CONFIG_FILE}[/cyan]")
        console.print("  [dim]This value will be used when BRAIN_* env vars are not set.[/dim]")
    else:
        # Just print export command
        console.print(f"[yellow]ℹ️  Set the env var directly:[/yellow]")
        console.print(f"  [bold]export {key}={value}[/bold]")
        console.print(f"  [dim]Or use [bold]--global[/bold] to persist in ~/.brain/config.env[/dim]")
