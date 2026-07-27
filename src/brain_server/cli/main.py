"""Main typer app for brain CLI."""

from __future__ import annotations

import os
import sys
from pathlib import Path

import typer
from rich.console import Console

from brain_server.cli.mcp_client import MCPClient

# ---------- Typer app ----------

app = typer.Typer(
    name="brain",
    help="🧠 Brain MCP server — central memory for AI agents.",
    no_args_is_help=True,
    add_completion=True,
    rich_markup_mode="rich",
)

# Sub-groups
server_app = typer.Typer(
    name="server",
    help="Manage the Brain MCP server (start/stop/status).",
    no_args_is_help=True,
)
config_app = typer.Typer(
    name="config",
    help="View or set brain configuration.",
    no_args_is_help=True,
)
init_app = typer.Typer(
    name="init",
    help="Initialize MCP server or project structure.",
    no_args_is_help=True,
)

setup_app = typer.Typer(
    name="setup",
    help="One-shot global setup for all AI tools (OpenCode, Copilot, Kiro, shell).",
    no_args_is_help=True,
)

app.add_typer(server_app, name="server")
app.add_typer(config_app, name="config")
app.add_typer(init_app, name="init")
app.add_typer(setup_app, name="setup")

console = Console()

# ---------- Helpers ----------

def _get_brain_dir() -> Path:
    """Get brain installation directory.
    
    Works both when installed globally (uv tool install) and when running from source.
    - When installed globally: uses ~/.brain
    - When running from source: uses the repository root
    """
    # Check environment variable first
    env_dir = os.getenv("BRAIN_DIR")
    if env_dir:
        return Path(env_dir)
    
    # Try to detect if running from source
    # __file__ is src/brain_server/cli/main.py, so go up 4 levels to get repo root
    try:
        source_dir = Path(__file__).resolve().parent.parent.parent.parent
        if (source_dir / "pyproject.toml").exists():
            return source_dir
    except Exception:
        pass
    
    # Installed globally - use ~/.brain
    home_brain = Path.home() / ".brain"
    home_brain.mkdir(parents=True, exist_ok=True)
    return home_brain


BRAIN_DIR = _get_brain_dir()


def _version_callback(value: bool) -> None:
    if value:
        from brain_server import __version__

        console.print(f"[bold]brain[/bold] v{__version__}")
        raise typer.Exit()


def _get_mcp_client() -> MCPClient:
    """Create MCP client from env or defaults."""
    url = os.getenv("BRAIN_URL", "http://localhost:8321")
    return MCPClient(url)


# ===========================================================================
# ROOT COMMANDS
# ===========================================================================


@app.callback()
def main(
    version: bool = typer.Option(
        False, "--version", "-V", help="Show version and exit.",
        callback=_version_callback, is_eager=True,
    ),
):
    """🧠  [bold]Brain[/bold] — MCP server for AI agent memory.

    Connect, store, search, and manage your AI agent's long-term memory
    using semantic search over an Obsidian vault with local embeddings.
    """
    pass


# ===========================================================================
# PING
# ===========================================================================


@app.command()
def ping(
    url: str = typer.Option(
        None, "--url", "-u",
        help="Brain server URL (default: BRAIN_URL env or http://localhost:8321)",
    ),
):
    """Health-check the brain MCP server."""
    client = MCPClient(url) if url else _get_mcp_client()
    with console.status("[bold green]Pinging brain server..."):
        try:
            result = client.call("ping")
            console.print(f"[bold green]✅ pong[/bold green]  [dim]→ {client.url}[/dim]")
        except Exception as e:
            console.print(f"[bold red]❌ {e}[/bold red]")
            raise typer.Exit(code=1)


# ===========================================================================
# STORE
# ===========================================================================


@app.command()
def store(
    layer: str = typer.Argument(..., help="Vault layer (arquitetura, regras, sessoes, projetos)"),
    path: str = typer.Argument(..., help="Relative path without .md extension (e.g. 'meu-app/stack')"),
    content: str = typer.Argument(
        ..., help="Markdown content. Use quotes or pipe from stdin.",
    ),
    scope: str = typer.Option(
        None, "--scope", "-s",
        help="Scope for arquitetura/regras layers: 'projetos' or 'global'. Required for these layers.",
    ),
    url: str = typer.Option(None, "--url", "-u", help="Brain server URL"),
):
    """Save a note to the brain vault."""
    client = MCPClient(url) if url else _get_mcp_client()
    with console.status("[bold green]Saving note..."):
        try:
            args = {"layer": layer, "path": path, "content": content}
            if scope:
                args["scope"] = scope
            result = client.call("brain_store", args)
            console.print(f"[bold green]✅[/bold green] {result}")
        except Exception as e:
            console.print(f"[bold red]❌ {e}[/bold red]")
            raise typer.Exit(code=1)


# ===========================================================================
# READ
# ===========================================================================


@app.command()
def read(
    layer: str = typer.Argument(..., help="Vault layer"),
    path: str = typer.Argument(..., help="Relative path without .md extension"),
    scope: str = typer.Option(
        None, "--scope", "-s",
        help="Scope for arquitetura/regras layers: 'projetos' or 'global'. Required for these layers.",
    ),
    url: str = typer.Option(None, "--url", "-u", help="Brain server URL"),
):
    """Read a note from the brain vault."""
    client = MCPClient(url) if url else _get_mcp_client()
    with console.status("[bold green]Reading note..."):
        try:
            args = {"layer": layer, "path": path}
            if scope:
                args["scope"] = scope
            result = client.call("brain_read", args)
            console.print(result)
        except Exception as e:
            console.print(f"[bold red]❌ {e}[/bold red]")
            raise typer.Exit(code=1)


# ===========================================================================
# SEARCH
# ===========================================================================


@app.command()
def search(
    query: str = typer.Argument(..., help="Search query text"),
    layer: str = typer.Option(None, "--layer", "-l", help="Filter by layer"),
    scope: str = typer.Option(
        None, "--scope", "-s",
        help="Filter by scope: 'projetos' or 'global'. Only applies to arquitetura/regras layers.",
    ),
    top_k: int = typer.Option(5, "--top-k", "-k", help="Max results", min=1, max=50),
    json_output: bool = typer.Option(False, "--json", "-j", help="Output as JSON (for scripting)"),
    url: str = typer.Option(None, "--url", "-u", help="Brain server URL"),
):
    """Semantic search over the brain vault using embeddings."""
    from rich.table import Table
    from rich.panel import Panel

    client = MCPClient(url) if url else _get_mcp_client()
    with console.status("[bold green]Searching brain..."):
        try:
            args = {"query": query, "top_k": top_k}
            if layer:
                args["layer"] = layer
            if scope:
                args["scope"] = scope
            result = client.call("brain_search", args)
        except Exception as e:
            console.print(f"[bold red]❌ {e}[/bold red]")
            raise typer.Exit(code=1)

    import json as json_lib
    try:
        data = json_lib.loads(result)
    except json_lib.JSONDecodeError:
        console.print(result)
        return

    if not data.get("results"):
        console.print("[yellow]No results found.[/yellow]")
        return

    if json_output:
        console.print_json(data=data)
        return

    table = Table(title=f"🔍 Search Results — [italic]{query}[/italic]", title_justify="left")
    table.add_column("Layer", style="cyan", no_wrap=True)
    table.add_column("Scope", style="magenta", no_wrap=True)
    table.add_column("Path", style="green")
    table.add_column("Score", style="yellow", justify="right")
    table.add_column("Snippet", style="dim", max_width=60)

    for r in data["results"]:
        snippet = (r.get("snippet") or "")[:80].replace("\n", " ")
        scope = r.get("scope") or "-"
        table.add_row(
            f"[{r['layer']}]",
            scope,
            r["path"],
            f"{r['score']:.4f}",
            snippet,
        )

    console.print(table)
    console.print(f"\n[dim]Total: {data['total']} results[/dim]")


# ===========================================================================
# REINDEX
# ===========================================================================


@app.command()
def reindex(
    all_files: bool = typer.Option(False, "--all", help="Reindex every file"),
    layer: str = typer.Option(None, "--layer", "-l", help="Reindex only this layer"),
    path: str = typer.Option(None, "--path", "-p", help="Reindex only this file"),
    url: str = typer.Option(None, "--url", "-u", help="Brain server URL"),
):
    """Rebuild the vector index (partial or full)."""
    if not (all_files or layer or path):
        console.print("[red]Provide --all, --layer, or --path[/red]")
        raise typer.Exit(code=1)

    client = MCPClient(url) if url else _get_mcp_client()
    with console.status("[bold green]Reindexing..."):
        try:
            result = client.call("brain_reindex", {
                "all": all_files, "layer": layer, "path": path,
            })
            console.print(f"[bold green]✅[/bold green] {result}")
        except Exception as e:
            console.print(f"[bold red]❌ {e}[/bold red]")
            raise typer.Exit(code=1)


# ===========================================================================
# PROJECTS
# ===========================================================================


@app.command()
def projects(
    vault_path: str = typer.Option(
        None, "--vault", "-v",
        help="Vault path (default: BRAIN_VAULT_PATH env or ./vault)",
    ),
):
    """List all projects registered in the brain vault."""
    from brain_server.cli.project import list_projects
    list_projects(vault_path)


# ===========================================================================
# INIT — MCP
# ===========================================================================


@init_app.command("mcp")
def init_mcp(
    port: int = typer.Option(8321, "--port", "-p", help="SSE server port", min=1024, max=65535),
    vault: str = typer.Option("./vault", "--vault", "-v", help="Path to vault directory"),
    ollama: str = typer.Option("http://localhost:11434", "--ollama", "-o", help="Ollama URL"),
    start: bool = typer.Option(False, "--start", "-s", help="Auto-start the server"),
    daemon: bool = typer.Option(False, "--daemon", "-d", help="Start as daemon (background)"),
):
    """Initialize and configure the Brain MCP server."""
    from brain_server.cli.server import init_mcp_server
    init_mcp_server(
        port=port,
        vault_path=Path(vault).resolve(),
        ollama_url=ollama,
        start=start,
        daemon=daemon,
        console=console,
        brain_dir=BRAIN_DIR,
    )


# ===========================================================================
# INIT — PROJECT
# ===========================================================================


@init_app.command("project")
def init_project(
    name: str = typer.Argument(..., help="Project name (e.g. 'meu-app')"),
    description: str = typer.Option("", "--description", "-d", help="Project description"),
    vault_path: str = typer.Option(
        None, "--vault", "-v",
        help="Vault path (default: BRAIN_VAULT_PATH env or ./vault)",
    ),
    port: int = typer.Option(
        8321, "--port", "-p",
        help="MCP server port (for generated configs)",
    ),
    opencode: bool = typer.Option(False, "--opencode", help="Generate OpenCode config"),
    claude: bool = typer.Option(False, "--claude", help="Generate Claude Desktop config"),
    kiro: bool = typer.Option(False, "--kiro", help="Generate Kiro CLI config"),
    copilot: bool = typer.Option(False, "--copilot", help="Generate GitHub Copilot config"),
    all_tools: bool = typer.Option(False, "--all", help="Generate config for ALL tools"),
):
    """Initialize a new project in the brain vault."""
    from brain_server.cli.project import init_project_structure
    init_project_structure(
        name=name,
        description=description,
        vault_path=vault_path,
        port=port,
        enable_opencode=opencode or all_tools,
        enable_claude=claude or all_tools,
        enable_kiro=kiro or all_tools,
        enable_copilot=copilot or all_tools,
        console=console,
        brain_dir=BRAIN_DIR,
    )


# ===========================================================================
# SERVER — START / STOP / STATUS
# ===========================================================================


@server_app.command()
def start(
    port: int = typer.Option(8321, "--port", "-p", help="SSE server port"),
    vault: str = typer.Option(None, "--vault", "-v", help="Vault path (default: BRAIN_DIR/vault)"),
    daemon: bool = typer.Option(False, "--daemon", "-d", help="Run in background"),
):
    """Start the Brain MCP server."""
    from brain_server.cli.server import start_server
    
    # Use BRAIN_DIR/vault as default if not specified
    vault_path = Path(vault).resolve() if vault else (BRAIN_DIR / "vault").resolve()
    
    start_server(
        port=port,
        vault_path=vault_path,
        daemon=daemon,
        console=console,
        brain_dir=BRAIN_DIR,
    )


@server_app.command()
def stop():
    """Stop the Brain MCP server."""
    from brain_server.cli.server import stop_server
    stop_server(console=console, brain_dir=BRAIN_DIR)


@server_app.command()
def status():
    """Show Brain MCP server status."""
    from brain_server.cli.server import server_status
    server_status(console=console, brain_dir=BRAIN_DIR)


# ===========================================================================
# CONFIG — SHOW / SET
# ===========================================================================


@config_app.command()
def show(
    vault_path: str = typer.Option(
        None, "--vault", "-v",
        help="Vault path (default: BRAIN_VAULT_PATH env or ./vault)",
    ),
):
    """Show current brain configuration."""
    from brain_server.cli.config_cmd import show_config
    show_config(vault_path=vault_path, console=console)


@config_app.command()
def set(
    key: str = typer.Argument(..., help="Config key (e.g. BRAIN_PORT, BRAIN_VAULT_PATH)"),
    value: str = typer.Argument(..., help="Config value"),
    global_cfg: bool = typer.Option(
        False, "--global", "-g",
        help="Save to global config (~/.brain/config.env)",
    ),
):
    """Set a brain configuration value (env var)."""
    from brain_server.cli.config_cmd import set_config
    set_config(key=key, value=value, global_cfg=global_cfg, console=console)


# ===========================================================================
# SETUP — Global one-shot configuration
# ===========================================================================


@setup_app.command()
def all(
    port: int = typer.Option(8321, "--port", "-p", help="Brain server port", min=1024, max=65535),
    vault: str = typer.Option("./vault", "--vault", "-v", help="Vault path"),
    ollama: str = typer.Option("http://localhost:11434", "--ollama", "-o", help="Ollama URL"),
    no_opencode: bool = typer.Option(False, "--no-opencode", help="Skip OpenCode config"),
    no_copilot: bool = typer.Option(False, "--no-copilot", help="Skip GitHub Copilot MCP config"),
    no_copilot_rules: bool = typer.Option(False, "--no-copilot-rules", help="Skip Copilot rules/instructions"),
    no_kiro: bool = typer.Option(False, "--no-kiro", help="Skip Kiro MCP config"),
    no_kiro_steering: bool = typer.Option(False, "--no-kiro-steering", help="Skip Kiro steering rules"),
    no_systemd: bool = typer.Option(False, "--no-systemd", help="Skip systemd service"),
    no_autostart: bool = typer.Option(False, "--no-autostart", help="Skip shell autostart"),
):
    """One-shot global setup: configure brain for ALL tools.

    Installs global MCP configs for OpenCode, GitHub Copilot, Kiro,
    plus Copilot instructions, Kiro steering rules,
    systemd service, and shell autostart.

    \b
    Examples:
        brain setup all                          # everything (defaults)
        brain setup all --port 9000 --vault ~/vault
        brain setup all --no-systemd --no-autostart    # skip some
    """
    from brain_server.cli.setup.installer import run_setup
    run_setup(
        port=port,
        vault=vault,
        ollama=ollama,
        enable_opencode=not no_opencode,
        enable_copilot=not no_copilot,
        enable_kiro=not no_kiro,
        enable_systemd=not no_systemd,
        enable_autostart=not no_autostart,
        enable_copilot_rules=not no_copilot_rules,
        enable_kiro_steering=not no_kiro_steering,
        console=console,
    )


@setup_app.command()
def opencode(
    port: int = typer.Option(8321, "--port", "-p", help="Brain server port"),
    vault: str = typer.Option("./vault", "--vault", "-v", help="Vault path"),
):
    """Configure OpenCode globally with brain MCP server."""
    from brain_server.cli.setup.installer import CONFIG_PATHS, _setup_opencode
    brain_url = f"http://localhost:{port}/sse"
    result = _setup_opencode(port, Path(vault).resolve(), brain_url, console)
    console.print(f"[green]✅[/green] {result['name']}: {result['status']}")
    console.print(f"  {result.get('detail', '')}")


@setup_app.command()
def copilot(
    port: int = typer.Option(8321, "--port", "-p", help="Brain server port"),
    vault: str = typer.Option("./vault", "--vault", "-v", help="Vault path"),
):
    """Configure GitHub Copilot MCP server connection only.

    For Copilot rules/instructions (how to USE brain), run:
      brain setup copilot-instructions
    """
    from brain_server.cli.setup.installer import _setup_copilot
    brain_url = f"http://localhost:{port}/sse"
    result = _setup_copilot(port, Path(vault).resolve(), brain_url, console)
    console.print(f"[green]✅[/green] {result['name']}: {result['status']}")


@setup_app.command()
def kiro(
    port: int = typer.Option(8321, "--port", "-p", help="Brain server port"),
    vault: str = typer.Option("./vault", "--vault", "-v", help="Vault path"),
):
    """Configure Kiro CLI MCP server connection only.

    For Kiro steering rules (how to USE brain), run:
      brain setup kiro-steering
    """
    from brain_server.cli.setup.installer import _setup_kiro
    brain_url = f"http://localhost:{port}/sse"
    result = _setup_kiro(port, Path(vault).resolve(), brain_url, console)
    console.print(f"[green]✅[/green] {result['name']}: {result['status']}")


@setup_app.command("copilot-instructions")
def copilot_instructions():
    """Install brain rules/instructions for GitHub Copilot Chat.

    Creates ~/.config/Code/User/brain-copilot-instructions.md and
    updates VS Code settings.json to reference it.
    This tells Copilot WHEN and HOW to use brain MCP tools.
    """
    from brain_server.cli.setup.installer import _setup_copilot_instructions
    result = _setup_copilot_instructions(console)
    console.print(f"[green]✅[/green] {result['name']}: {result['status']}")
    console.print(f"  {result.get('detail', '')}")
    console.print("  Restart VS Code for changes to take effect.")


@setup_app.command("kiro-steering")
def kiro_steering():
    """Install brain steering rules for Kiro CLI agents.

    Creates ~/.kiro/steering/brain.md with inclusion: always so
    every Kiro agent session has brain MCP usage rules.
    """
    from brain_server.cli.setup.installer import _setup_kiro_steering
    result = _setup_kiro_steering(console)
    console.print(f"[green]✅[/green] {result['name']}: {result['status']}")
    console.print(f"  {result.get('detail', '')}")


@setup_app.command()
def systemd(
    port: int = typer.Option(8321, "--port", "-p", help="Brain server port"),
    vault: str = typer.Option("./vault", "--vault", "-v", help="Vault path"),
    ollama: str = typer.Option("http://localhost:11434", "--ollama", "-o", help="Ollama URL"),
):
    """Create systemd user service for brain auto-start."""
    from brain_server.cli.setup.installer import _setup_systemd
    result = _setup_systemd(port, Path(vault).resolve(), ollama, console)
    console.print(f"[green]✅[/green] {result['name']}: {result['status']}")
    console.print(f"  {result.get('detail', '')}")


# ===========================================================================
# WATCH — Daemon de monitoramento
# ===========================================================================


@app.command()
def watch(
    dirs: str = typer.Option(
        None, "--dirs", "-d",
        help="Diretórios para monitorar (separados por vírgula)",
    ),
    interval: int = typer.Option(
        30, "--interval", "-i",
        help="Intervalo entre capturas (segundos)", min=5, max=600,
    ),
    background: bool = typer.Option(
        False, "--daemon", "-D",
        help="Rodar em background (usa watchdog)",
    ),
    verbose: bool = typer.Option(
        False, "--verbose", "-v",
        help="Log detalhado",
    ),
):
    """Watch projects and auto-capture context to brain.

    Monitora alterações em arquivos dos projetos e salva automaticamente
    o contexto de trabalho no cérebro.

    \b
    Examples:
        brain watch                    # monitora ~/Documentos/wsProjetos
        brain watch --dirs ~/projetos  # diretório customizado
        brain watch --daemon           # background (requer watchdog)
        brain watch --interval 60      # captura a cada 60s
    """
    from brain_server.cli.watch import watch_projects

    parsed_dirs = None
    if dirs:
        parsed_dirs = [d.strip() for d in dirs.split(",")]

    watch_projects(
        dirs=parsed_dirs,
        interval=interval,
        daemon=background,
        verbose=verbose,
        console=console,
    )


# ===========================================================================
# CONTEXT — Buscar contexto do projeto atual
# ===========================================================================


@app.command()
def context(
    query: str = typer.Argument(
        None, help="Consulta opcional (usa nome do projeto se omitido)",
    ),
    top_k: int = typer.Option(5, "--top-k", "-k", help="Max resultados"),
    json_output: bool = typer.Option(False, "--json", "-j", help="Output JSON"),
    url: str = typer.Option(None, "--url", "-u", help="Brain server URL"),
):
    """Get relevant brain context for current project.

    Útil para pipe em comandos de IA:
        brain context | kiro "faça uma feature"
        brain context --json | jq '.results[].snippet'

    Se chamado sem argumentos, detecta o projeto atual pelo git.
    """
    from brain_server.cli.watch import _detect_project

    cwd = os.getcwd()
    detected = _detect_project(os.path.join(cwd, "dummy.txt"))

    if not query:
        query = detected or Path(cwd).name
        if not query:
            console.print("[red]Could not detect project. Provide a query.[/red]")
            raise typer.Exit(code=1)

    client = MCPClient(url) if url else _get_mcp_client()

    with console.status(f"[bold green]Loading context for '{query}'..."):
        try:
            result = client.call("brain_search", {
                "query": query, "top_k": top_k,
            })
        except Exception as e:
            console.print(f"[red]❌ {e}[/red]")
            raise typer.Exit(code=1)

    import json as json_lib
    try:
        data = json_lib.loads(result)
    except json_lib.JSONDecodeError:
        console.print(result)
        return

    if json_output:
        console.print_json(data=data)
        return

    if not data.get("results"):
        console.print(f"[yellow]No context found for '{query}'[/yellow]")
        console.print("  Start saving context with: [bold]brain store[/bold]")
        return

    from rich.table import Table

    console.print(f"[bold]🧠 Context for: [cyan]{query}[/cyan][/bold]\n")

    table = Table(box=None)
    table.add_column("Layer", style="cyan", no_wrap=True)
    table.add_column("Scope", style="magenta", no_wrap=True)
    table.add_column("Path", style="green")
    table.add_column("Score", style="yellow", justify="right")
    table.add_column("Snippet", style="dim", max_width=80)

    for r in data["results"]:
        snippet = (r.get("snippet") or "")[:100].replace("\n", " ")
        scope = r.get("scope") or "-"
        table.add_row(
            f"[{r['layer']}]",
            scope,
            r["path"],
            f"{r['score']:.2f}",
            snippet,
        )

    console.print(table)
    console.print(f"\n[dim]Tip: pipe this into your AI tool:[/dim]")
    console.print(f"  [bold]brain context | kiro \"implemente a feature\"[/bold]")


# ===========================================================================
# ENTRY POINT
# ===========================================================================

def main() -> None:
    """Entry point for the brain CLI."""
    app()
