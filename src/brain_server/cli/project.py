"""Project initialization and listing commands."""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import TYPE_CHECKING

from rich.syntax import Syntax

if TYPE_CHECKING:
    from rich.console import Console


def _get_vault_root(vault_path: str | None = None) -> Path:
    """Resolve vault path from argument, env, or default."""
    if vault_path:
        return Path(vault_path).resolve()
    return Path(os.getenv("BRAIN_VAULT_PATH", "./vault")).resolve()


def list_projects(vault_path: str | None = None) -> None:
    """List all projects registered in the brain vault."""
    from rich.console import Console
    from rich.table import Table
    from rich.panel import Panel

    console = Console()
    vault = _get_vault_root(vault_path)
    projects_dir = vault / "projetos"

    if not projects_dir.exists() or not any(projects_dir.iterdir()):
        console.print("[yellow]📂 No projects found in vault.[/yellow]")
        console.print("  Create one with: [bold]brain init project <name>[/bold]")
        return

    projects = sorted(d.name for d in projects_dir.iterdir() if d.is_dir())
    table = Table(title=f"📂 Projects in Brain Vault", title_justify="left")
    table.add_column("Project", style="bold cyan")
    table.add_column("Description", style="dim")
    table.add_column("Notes", justify="right")

    for proj in projects:
        overview = projects_dir / proj / "visao-geral.md"
        desc = ""
        if overview.exists():
            for line in overview.read_text().splitlines():
                stripped = line.strip()
                if stripped and not stripped.startswith("#"):
                    desc = stripped[:60]
                    break

        # Count notes
        total = 0
        for layer in ("arquitetura", "regras", "sessoes"):
            layer_dir = vault / layer / proj
            if layer_dir.exists():
                total += len(list(layer_dir.glob("*.md")))

        table.add_row(proj, desc or "(no description)", str(total) if total else "—")

    console.print(table)
    console.print(f"\n[dim]Total: {len(projects)} project(s)[/dim]")


def init_project_structure(
    name: str,
    description: str,
    vault_path: str | None,
    port: int,
    enable_opencode: bool,
    enable_claude: bool,
    enable_kiro: bool,
    enable_copilot: bool,
    console: Console,
    brain_dir: Path,
) -> None:
    """Initialize vault structure for a new project and optionally generate tool configs."""
    from rich.panel import Panel
    from rich.table import Table
    vault = _get_vault_root(vault_path)
    desc = description or f"Projeto {name}"
    enabled_tools = []

    # ── Create vault structure ──
    console.print(f"[bold]📦 Initializing project '[cyan]{name}[/cyan]'[/bold]")
    console.print(f"  Vault: [cyan]{vault}[/cyan]\n")

    layers = ["arquitetura", "regras", "sessoes", "projetos"]
    created_files = []

    for layer in layers:
        layer_dir = vault / layer / name
        layer_dir.mkdir(parents=True, exist_ok=True)
        created_files.append(f"{layer}/{name}/")

    # Create visao-geral
    overview = vault / "projetos" / name / "visao-geral.md"
    if not overview.exists():
        overview.write_text(
            f"# {name}\n\n"
            f"{desc}\n\n"
            f"## Stack\n\n"
            f"(preencher)\n\n"
            f"## Time\n\n"
            f"(preencher)\n\n"
            f"## Links\n\n"
            f"(preencher)\n"
        )
        created_files.append(f"projetos/{name}/visao-geral.md")

    # Create template notes
    templates = {
        "arquitetura": [
            ("stack-decisao", f"# Stack — {name}\n\n## Frontend\n\n(preencher)\n\n## Backend\n\n(preencher)\n\n## Banco de Dados\n\n(preencher)\n\n## Infra\n\n(preencher)\n"),
        ],
        "regras": [
            ("naming-conventions", f"# Naming Conventions — {name}\n\n## Banco de Dados\n- Tabelas: (definir)\n- Colunas: (definir)\n\n## API\n- Endpoints: (definir)\n- JSON: (definir)\n"),
            ("git-workflow", f"# Git Workflow — {name}\n\n- Branch strategy: (definir)\n- Commit style: (definir)\n- Review process: (definir)\n"),
        ],
    }

    for layer, notes in templates.items():
        for note_name, note_content in notes:
            note_file = vault / layer / name / f"{note_name}.md"
            if not note_file.exists():
                note_file.write_text(note_content)
                created_files.append(f"{layer}/{name}/{note_name}.md")

    # Show created files
    for f in created_files:
        console.print(f"  [green]✅[/green] {f}")

    console.print(f"\n[green]✅ Project '{name}' initialized![/green]\n")

    # ── Generate tool configs ──
    if any([enable_opencode, enable_claude, enable_kiro, enable_copilot]):
        console.print("[bold]📋 Generating tool configurations...[/bold]\n")

    if enable_opencode:
        enabled_tools.append("OpenCode")
        _generate_opencode_config(name, port, brain_dir, console)

    if enable_claude:
        enabled_tools.append("Claude Desktop")
        _generate_claude_config(name, port, brain_dir, console)

    if enable_kiro:
        enabled_tools.append("Kiro CLI")
        _generate_kiro_config(name, port, brain_dir, console)

    if enable_copilot:
        enabled_tools.append("GitHub Copilot")
        _generate_copilot_config(name, port, brain_dir, console)

    if enabled_tools:
        console.print(f"[green]✅ Config generated for: {', '.join(enabled_tools)}[/green]")
        console.print("  [dim]Copy the snippets above into the respective config files.[/dim]")
    else:
        console.print("  [dim]Tip: add [bold]--all[/bold] to generate config for all AI tools[/dim]")

    # ── Summary panel ──
    grid = Table.grid(padding=1)
    grid.add_column(style="bold")
    grid.add_column()
    grid.add_row("Project", f"[cyan]{name}[/cyan]")
    grid.add_row("Vault", f"[cyan]{vault}[/cyan]")
    grid.add_row("Notes", str(len(created_files)))
    grid.add_row("MCP URL", f"[cyan]http://localhost:{port}/sse[/cyan]")

    console.print()
    console.print(Panel(grid, title="📋 Summary"))


def _generate_opencode_config(name: str, port: int, brain_dir: Path, console: Console) -> None:
    """Generate and print OpenCode config."""
    config = {
        "instructions": [
            ".agents/rules/*.md",
            f"{brain_dir}/.agents/skills/brain/SKILL.md",
        ],
        "mcpServers": {
            "brain": {
                "transport": "sse",
                "url": f"http://localhost:{port}/sse",
            }
        },
    }
    console.print("[bold]1️⃣  OpenCode[/bold] — add to [italic]opencode.json[/italic]:")
    console.print(Syntax(json.dumps(config, indent=2, ensure_ascii=False), "json", theme="monokai"))
    console.print()


def _generate_claude_config(name: str, port: int, brain_dir: Path, console: Console) -> None:
    """Generate and print Claude Desktop config."""
    config = {
        "mcpServers": {
            "brain": {
                "command": "uv",
                "args": ["run", "--directory", str(brain_dir), "python", "-m", "brain_server"],
            }
        }
    }
    console.print("[bold]2️⃣  Claude Desktop[/bold] — add to [italic]claude_desktop_config.json[/italic]:")
    console.print(Syntax(json.dumps(config, indent=2, ensure_ascii=False), "json", theme="monokai"))
    console.print()


def _generate_kiro_config(name: str, port: int, brain_dir: Path, console: Console) -> None:
    """Generate and print Kiro CLI config."""
    config = {
        "mcpServers": {
            "brain": {
                "transport": "sse",
                "url": f"http://localhost:{port}/sse",
            }
        }
    }
    console.print("[bold]3️⃣  Kiro CLI[/bold] — add to [italic].kiro/config.json[/italic] or [italic]kiro.json[/italic]:")
    console.print(Syntax(json.dumps(config, indent=2, ensure_ascii=False), "json", theme="monokai"))
    console.print()


def _generate_copilot_config(name: str, port: int, brain_dir: Path, console: Console) -> None:
    """Generate and print GitHub Copilot MCP config."""
    config = {
        "servers": {
            "brain": {
                "type": "stdio",
                "command": ["uv", "run", "--directory", str(brain_dir), "python", "-m", "brain_server"],
            }
        }
    }
    console.print("[bold]4️⃣  GitHub Copilot[/bold] — add to [italic]~/.config/gh/mcp.json[/italic]:")
    console.print(Syntax(json.dumps(config, indent=2, ensure_ascii=False), "json", theme="monokai"))
    console.print()


