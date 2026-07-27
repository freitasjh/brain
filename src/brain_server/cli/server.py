"""Server management commands (start/stop/status)."""

from __future__ import annotations

import os
import signal
import subprocess
import sys
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from rich.console import Console


def _get_pid_file(brain_dir: Path) -> Path:
    return brain_dir / "data" / "brain-server.pid"


def _get_log_file(brain_dir: Path) -> Path:
    return brain_dir / "data" / "brain-server.log"


def start_server(
    port: int,
    vault_path: Path,
    daemon: bool,
    console: Console,
    brain_dir: Path,
) -> None:
    """Start the brain MCP server."""
    # Determine if running from source or installed globally
    from_source = (brain_dir / "pyproject.toml").exists()
    
    # Check if already running
    pid_file = _get_pid_file(brain_dir)
    if pid_file.exists():
        try:
            pid = int(pid_file.read_text().strip())
            os.kill(pid, 0)  # Check if alive
            console.print(f"[yellow]⚠️  Server already running (PID: {pid})[/yellow]")
            console.print("  Use [bold]brain server stop[/bold] first, or [bold]brain server status[/bold]")
            return
        except (ProcessLookupError, ValueError):
            pid_file.unlink(missing_ok=True)

    # Ensure vault layers exist
    for layer in ("arquitetura", "regras", "sessoes", "projetos", "indexacao"):
        (vault_path / layer).mkdir(parents=True, exist_ok=True)

    # Ensure data dir
    (brain_dir / "data").mkdir(parents=True, exist_ok=True)

    env = {
        **os.environ,
        "BRAIN_TRANSPORT": "sse",
        "BRAIN_PORT": str(port),
        "BRAIN_VAULT_PATH": str(vault_path),
        "BRAIN_LOG_LEVEL": os.getenv("BRAIN_LOG_LEVEL", "INFO"),
    }

    # Build command based on installation type
    if from_source:
        # Running from source - use uv run
        if not _find_uv():
            console.print("[red]❌ 'uv' not found. Install: curl -LsSf https://astral.sh/uv/install.sh | sh[/red]")
            raise SystemExit(1)
        cmd = [_find_uv(), "run", "--directory", str(brain_dir), "python", "-m", "brain_server"]
    else:
        # Installed globally - run directly
        cmd = [sys.executable, "-m", "brain_server"]

    if daemon:
        log_file = _get_log_file(brain_dir)
        console.print(f"[bold]🧠 Starting Brain MCP server (daemon mode)...[/bold]")
        console.print(f"  Port:    [cyan]{port}[/cyan]")
        console.print(f"  Vault:   [cyan]{vault_path}[/cyan]")
        console.print(f"  Log:     [cyan]{log_file}[/cyan]")
        console.print(f"  URL:     [cyan]http://localhost:{port}/sse[/cyan]")

        with log_file.open("w") as log:
            proc = subprocess.Popen(
                cmd,
                stdout=log,
                stderr=log,
                env=env,
                stdin=subprocess.DEVNULL,
            )

        pid_file.write_text(str(proc.pid))
        console.print(f"[green]✅ Server started (PID: {proc.pid})[/green]")
        console.print(f"  Stop with: [bold]brain server stop[/bold]")
        console.print(f"  Status:    [bold]brain server status[/bold]")
    else:
        console.print(f"[bold]🧠 Starting Brain MCP server (foreground)...[/bold]")
        console.print(f"  Port:  [cyan]{port}[/cyan]")
        console.print(f"  Vault: [cyan]{vault_path}[/cyan]")
        console.print(f"  URL:   [cyan]http://localhost:{port}/sse[/cyan]")
        console.print(f"  [dim]Press Ctrl+C to stop[/dim]\n")

        os.execve(cmd[0], cmd, env)


def stop_server(console: Console, brain_dir: Path) -> None:
    """Stop the brain MCP server."""
    pid_file = _get_pid_file(brain_dir)
    if not pid_file.exists():
        console.print("[yellow]⚠️  Server PID file not found — is it running?[/yellow]")
        # Try to find by process name
        try:
            result = subprocess.run(
                ["pgrep", "-f", "brain_server"],
                capture_output=True, text=True, timeout=5,
            )
            if result.stdout.strip():
                pids = result.stdout.strip().split()
                console.print(f"  Found process(es): {', '.join(pids)}")
                for pid in pids:
                    os.kill(int(pid), signal.SIGTERM)
                console.print(f"[green]✅ Stopped {len(pids)} process(es)[/green]")
            else:
                console.print("[red]No brain server process found[/red]")
        except FileNotFoundError:
            console.print("[red]No brain server process found[/red]")
        return

    try:
        pid = int(pid_file.read_text().strip())
        os.kill(pid, signal.SIGTERM)
        pid_file.unlink(missing_ok=True)
        console.print(f"[green]✅ Server stopped (PID: {pid})[/green]")
    except ProcessLookupError:
        console.print("[yellow]⚠️  Process not found — removing stale PID file[/yellow]")
        pid_file.unlink(missing_ok=True)
    except ValueError:
        console.print("[red]❌ Invalid PID file[/red]")
        pid_file.unlink(missing_ok=True)


def server_status(console: Console, brain_dir: Path) -> None:
    """Show brain MCP server status."""
    from rich.panel import Panel
    from rich.table import Table

    pid_file = _get_pid_file(brain_dir)
    log_file = _get_log_file(brain_dir)

    grid = Table.grid(padding=1)
    grid.add_column(style="bold")
    grid.add_column()

    # Check PID file
    running = False
    pid = None
    if pid_file.exists():
        try:
            pid = int(pid_file.read_text().strip())
            os.kill(pid, 0)
            running = True
        except (ProcessLookupError, ValueError, OSError):
            pid_file.unlink(missing_ok=True)

    if running:
        grid.add_row("Status", "[green]🟢 Running[/green]")
        grid.add_row("PID", str(pid))

        # Try to get port from env
        try:
            proc_env = f"/proc/{pid}/environ"
            if os.path.exists(proc_env):
                env_data = open(proc_env, "rb").read().decode("latin-1")
                for var in ["BRAIN_PORT", "BRAIN_VAULT_PATH"]:
                    for entry in env_data.split("\0"):
                        if entry.startswith(var + "="):
                            val = entry.split("=", 1)[1]
                            grid.add_row(var.replace("BRAIN_", ""), f"[cyan]{val}[/cyan]")
        except Exception:
            pass

        grid.add_row("URL", f"[cyan]http://localhost:{os.getenv('BRAIN_PORT', '8321')}/sse[/cyan]")
        grid.add_row("Log", str(log_file))
    else:
        grid.add_row("Status", "[red]🔴 Stopped[/red]")

    # Show log tail
    log_tail = ""
    if log_file.exists():
        try:
            lines = log_file.read_text().splitlines()
            log_tail = "\n".join(lines[-5:])
        except Exception:
            pass

    console.print(Panel(grid, title="🧠 Brain Server Status"))

    if log_tail:
        console.print("\n[bold]Recent log:[/bold]")
        console.print(f"[dim]{log_tail}[/dim]")

    if not running:
        console.print("\nStart with: [bold]brain server start --daemon[/bold]")


def init_mcp_server(
    port: int,
    vault_path: Path,
    ollama_url: str,
    start: bool,
    daemon: bool,
    console: Console,
    brain_dir: Path,
) -> None:
    """Initialize MCP server configuration and optionally start it."""
    from rich.panel import Panel
    from rich.table import Table

    # Create vault structure
    for layer in ("arquitetura", "regras", "sessoes", "projetos", "indexacao"):
        (vault_path / layer).mkdir(parents=True, exist_ok=True)

    # Ensure data dir
    (brain_dir / "data").mkdir(parents=True, exist_ok=True)

    # Show summary
    grid = Table.grid(padding=1)
    grid.add_column(style="bold")
    grid.add_column()
    grid.add_row("Port", f"[cyan]{port}[/cyan]")
    grid.add_row("Vault", f"[cyan]{vault_path}[/cyan]")
    grid.add_row("Ollama", f"[cyan]{ollama_url}[/cyan]")
    grid.add_row("Server URL", f"[cyan]http://localhost:{port}/sse[/cyan]")
    grid.add_row("Start command", f"[bold]brain server start{' --daemon' if daemon else ''}[/bold]")

    console.print(Panel(grid, title="🧠 Brain MCP Server — Configuration"))
    console.print()

    # Print configuration snippets for different tools
    console.print("[bold]📋 Config for your AI tools:[/bold]\n")
    _print_config_snippet(console, "OpenCode (opencode.json)", {
        "mcpServers": {
            "brain": {
                "transport": "sse",
                "url": f"http://localhost:{port}/sse",
            }
        }
    })
    _print_config_snippet(console, "Claude Desktop (claude_desktop_config.json)", {
        "mcpServers": {
            "brain": {
                "command": "uv",
                "args": [
                    "run", "--directory", str(brain_dir),
                    "python", "-m", "brain_server",
                ],
            }
        }
    })
    _print_config_snippet(console, "Kiro CLI (.kiro/config.json)", {
        "mcpServers": {
            "brain": {
                "transport": "sse",
                "url": f"http://localhost:{port}/sse",
            }
        }
    })
    _print_config_snippet(console, "GitHub Copilot (~/.config/gh/mcp.json)", {
        "servers": {
            "brain": {
                "type": "stdio",
                "command": ["uv", "run", "--directory", str(brain_dir), "python", "-m", "brain_server"],
            }
        }
    })

    # Auto-start
    if start:
        console.print()
        start_server(port=port, vault_path=vault_path, daemon=daemon, console=console, brain_dir=brain_dir)


def _print_config_snippet(console: Console, label: str, config: dict) -> None:
    """Print a formatted JSON config snippet."""
    import json
    from rich.syntax import Syntax

    console.print(f"  [bold]{label}[/bold]")
    json_str = json.dumps(config, indent=2, ensure_ascii=False)
    syntax = Syntax(json_str, "json", theme="monokai", line_numbers=False)
    console.print(syntax)
    console.print()


def _find_uv() -> str:
    """Find uv executable."""
    import shutil
    uv = shutil.which("uv")
    if uv:
        return uv
    # Common fallback paths
    for p in [
        os.path.expanduser("~/.local/bin/uv"),
        "/usr/local/bin/uv",
        "/opt/homebrew/bin/uv",
    ]:
        if os.path.isfile(p):
            return p
    return "uv"  # Let it fail with a clearer message later
