"""`brain watch` — daemon que monitora projetos e auto-captura contexto.

Monitora diretórios de projetos em busca de alterações e mantém o cérebro
atualizado automaticamente com o contexto do que você está trabalhando.
"""

from __future__ import annotations

import json
import os
import subprocess
import time
from pathlib import Path
from threading import Lock
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from rich.console import Console

# ── Default watch dirs ──
DEFAULT_WATCH_DIRS = [
    Path.home() / "Documentos" / "wsProjetos",
    Path.home() / "projects",
    Path.home() / "Projetos",
]

SESSION_FILE = Path.home() / ".brain" / "watch-session.json"


def watch_projects(
    dirs: list[str] | None,
    interval: int,
    daemon: bool,
    verbose: bool,
    console: Console,
) -> None:
    """Watch project directories and auto-capture context."""
    # Resolve directories to watch
    watch_dirs = []
    if dirs:
        watch_dirs = [Path(d).resolve() for d in dirs]
    else:
        for d in DEFAULT_WATCH_DIRS:
            if d.exists():
                watch_dirs.append(d)

    if not watch_dirs:
        console.print("[yellow]⚠️  No project directories found.[/yellow]")
        console.print("  Specify with: [bold]brain watch --dirs ~/projetos[/bold]")
        console.print(f"  Default searched: {', '.join(str(d) for d in DEFAULT_WATCH_DIRS)}")
        raise SystemExit(1)

    console.print(f"[bold]🧠 Brain Watch Daemon[/bold]")
    console.print(f"  Watching: [cyan]{', '.join(str(d) for d in watch_dirs)}[/cyan]")
    console.print(f"  Interval: [cyan]{interval}s[/cyan]")
    console.print(f"  Verbose:  [cyan]{verbose}[/cyan]")
    console.print()

    if daemon:
        _start_daemon(watch_dirs, interval, verbose, console)
    else:
        _run_foreground(watch_dirs, interval, verbose, console)


def _start_daemon(
    watch_dirs: list[Path], interval: int, verbose: bool, console: Console,
) -> None:
    """Start in background using watchdog library."""
    try:
        from watchdog.observers import Observer
        from watchdog.events import FileSystemEventHandler
    except ImportError:
        console.print("[red]❌ watchdog not installed. Run: uv add watchdog[/red]")
        raise SystemExit(1)

    brain_dir = Path(__file__).resolve().parent.parent.parent.parent
    pid_file = brain_dir / "data" / "brain-watch.pid"
    log_file = brain_dir / "data" / "brain-watch.log"
    (brain_dir / "data").mkdir(parents=True, exist_ok=True)

    class BrainEventHandler(FileSystemEventHandler):
        def __init__(self):
            self.changes = []
            self.lock = Lock()
            self.last_save = time.time()

        def on_modified(self, event):
            if event.is_directory:
                return
            if not self._is_relevant(event.src_path):
                return

            with self.lock:
                self.changes.append({
                    "path": event.src_path,
                    "time": time.time(),
                })

        def _is_relevant(self, path: str) -> bool:
            """Filter relevant file types."""
            ext = Path(path).suffix.lower()
            return ext in (
                ".py", ".js", ".ts", ".jsx", ".tsx", ".vue",
                ".java", ".kt", ".go", ".rs", ".rb", ".php",
                ".md", ".rst", ".txt",
                ".json", ".yaml", ".yml", ".toml",
                ".sql", ".graphql",
                ".css", ".scss", ".html",
            )

        def flush_changes(self) -> list[dict]:
            with self.lock:
                items = list(self.changes)
                self.changes.clear()
            return items

    event_handler = BrainEventHandler()
    observer = Observer()
    for watch_dir in watch_dirs:
        if watch_dir.exists():
            observer.schedule(event_handler, str(watch_dir), recursive=True)
            console.print(f"  👀 Watching: {watch_dir}")

    observer.start()
    console.print(f"\n[green]✅ Watch daemon started (PID: {os.getpid()})[/green]")
    console.print(f"  Log: {log_file}")
    pid_file.write_text(str(os.getpid()))

    try:
        while True:
            time.sleep(interval)
            changes = event_handler.flush_changes()
            if changes:
                _process_changes(changes, verbose, log_file)
    except KeyboardInterrupt:
        observer.stop()
        pid_file.unlink(missing_ok=True)
        console.print("\n[yellow]Watch daemon stopped[/yellow]")
    observer.join()


def _run_foreground(
    watch_dirs: list[Path], interval: int, verbose: bool, console: Console,
) -> None:
    """Simple polling-based watcher for foreground mode."""
    from rich.live import Live
    from rich.table import Table

    snapshots: dict[str, float] = {}

    with Live(refresh_per_second=1, console=console) as live:
        try:
            while True:
                now = time.time()
                changes = []

                for watch_dir in watch_dirs:
                    if not watch_dir.exists():
                        continue
                    for f in _walk_files(watch_dir):
                        mtime = os.path.getmtime(f)
                        last = snapshots.get(f, 0)
                        if mtime > last + 2:  # 2s debounce
                            snapshots[f] = mtime
                            if last > 0 and (mtime - last) < interval * 3:
                                changes.append(f)

                if changes:
                    _process_changes_simple(changes, verbose)
                    snapshots = {f: t for f, t in snapshots.items()
                                 if now - t < 300}  # forget after 5min

                # Build live table
                table = Table(title="🧠 Brain Watch", title_justify="left")
                table.add_column("Time", style="dim")
                table.add_column("File", style="cyan")
                table.add_column("Project", style="green")
                table.add_column("Status", style="yellow")

                recent = sorted(
                    [(t, f) for f, t in snapshots.items() if now - t < 60],
                    reverse=True,
                )[:10]
                if recent:
                    for ts, fp in recent:
                        proj = _detect_project(fp)
                        table.add_row(
                            time.strftime("%H:%M:%S", time.localtime(ts)),
                            Path(fp).name,
                            proj or "—",
                            "saved",
                        )
                else:
                    table.add_row("—", "Waiting for changes...", "", "")

                live.update(table)
                time.sleep(1)

        except KeyboardInterrupt:
            console.print("\n[yellow]Watch stopped[/yellow]")


# ═══════════════════════════════════════════════════════════════════
# CONTEXT AUTO-CAPTURE
# ═══════════════════════════════════════════════════════════════════


def _process_changes(
    changes: list[dict], verbose: bool, log_file: Path,
) -> None:
    """Process and save changes to the brain."""
    # Group by project
    projects: dict[str, list[str]] = {}
    for c in changes:
        proj = _detect_project(c["path"])
        if proj:
            projects.setdefault(proj, []).append(c["path"])

    for proj, files in projects.items():
        # Save session context
        summary = _build_session_context(proj, files)
        _save_to_brain(proj, summary, verbose, log_file)


def _process_changes_simple(changes: list[str], verbose: bool) -> None:
    """Simple version for foreground mode."""
    projects: dict[str, list[str]] = {}
    for f in changes:
        proj = _detect_project(f)
        if proj:
            projects.setdefault(proj, []).append(f)

    for proj, files in projects.items():
        summary = _build_session_context(proj, files, brief=True)
        _save_to_brain(proj, summary, verbose, None)


def _build_session_context(
    project: str, files: list[str], brief: bool = False,
) -> str:
    """Build a context summary from changed files."""
    ext_counts: dict[str, int] = {}
    for f in files:
        ext = Path(f).suffix or "(no ext)"
        ext_counts[ext] = ext_counts.get(ext, 0) + 1

    file_list = "\n".join(f"  - {f}" for f in sorted(set(files))[:20])
    if len(set(files)) > 20:
        file_list += f"\n  ... e mais {len(set(files)) - 20} arquivos"

    ext_summary = ", ".join(f"{ext}: {n}" for ext, n in
                           sorted(ext_counts.items(), key=lambda x: -x[1]))

    if brief:
        return f"""## Alterações recentes

**Projeto:** {project}
**Arquivos alterados:** {len(set(files))}
**Tipos:** {ext_summary}

### Arquivos
{file_list}
"""
    else:
        timestamp = time.strftime("%Y-%m-%d %H:%M:%S")
        return f"""## Sessão de trabalho — {timestamp}

**Projeto:** {project}
**Arquivos alterados:** {len(set(files))}
**Tipos de arquivo:** {ext_summary}

### Arquivos modificados
{file_list}

### Contexto
Sessão de trabalho detectada automaticamente pelo Brain Watch.
"""


def _save_to_brain(
    project: str, summary: str, verbose: bool, log_file: Path | None,
) -> None:
    """Save context to brain MCP server."""
    try:
        from brain_server.cli.mcp_client import MCPClient

        client = MCPClient()
        layer = "sessoes"
        path = f"{project}/watch-{int(time.time())}"

        result = client.call("brain_store", {
            "layer": layer,
            "path": path,
            "content": summary,
        })

        msg = f"  📝 Saved: {layer}/{path}.md"
        if verbose:
            print(msg)
        if log_file:
            with open(log_file, "a") as f:
                f.write(f"{time.strftime('%Y-%m-%d %H:%M:%S')} {msg}\n")

    except Exception as e:
        err = f"  ⚠️  Failed to save to brain: {e}"
        if verbose:
            print(err)
        if log_file:
            with open(log_file, "a") as f:
                f.write(f"{time.strftime('%Y-%m-%d %H:%M:%S')} {err}\n")


# ═══════════════════════════════════════════════════════════════════
# HELPERS
# ═══════════════════════════════════════════════════════════════════


def _detect_project(filepath: str) -> str | None:
    """Detect which project a file belongs to."""
    p = Path(filepath).resolve()
    # Check for git root
    for parent in p.parents:
        if (parent / ".git").exists():
            # Use dir name or git remote
            try:
                remote = subprocess.run(
                    ["git", "remote", "get-url", "origin"],
                    capture_output=True, text=True, timeout=3,
                    cwd=str(parent),
                ).stdout.strip()
                if remote:
                    name = Path(remote).stem
                    if name == "brain":
                        return None  # skip brain itself
                    return name
            except Exception:
                pass
            name = parent.name
            if name == "brain":
                return None
            return name
    return None


def _walk_files(directory: Path) -> list[str]:
    """Walk directory and return relevant files."""
    result = []
    try:
        for root, dirs, files in os.walk(directory):
            # Skip common non-project dirs
            dirs[:] = [d for d in dirs if not d.startswith((".", "__", "node_modules",
                                                              "venv", ".venv", "env",
                                                              "dist", "build", "target",
                                                              ".git", ".mypy_cache",
                                                              ".pytest_cache", "__pycache__"))]
            for f in files:
                ext = Path(f).suffix.lower()
                if ext in (".py", ".js", ".ts", ".jsx", ".tsx", ".vue",
                           ".java", ".kt", ".go", ".rs",
                           ".md", ".json", ".yaml", ".yml",
                           ".sql", ".css", ".scss", ".html",
                           ".toml", ".xml"):
                    result.append(os.path.join(root, f))
    except PermissionError:
        pass
    return result


def show_context(console: Console) -> None:
    """Show current watch session context."""
    if not SESSION_FILE.exists():
        console.print("[yellow]No active watch session.[/yellow]")
        console.print("  Start with: [bold]brain watch[/bold]")
        return

    data = json.loads(SESSION_FILE.read_text())
    from rich.table import Table

    table = Table(title="🧠 Watch Session")
    table.add_column("Project", style="cyan")
    table.add_column("Files", justify="right")
    table.add_column("Last Activity", style="dim")

    for proj, info in data.get("projects", {}).items():
        table.add_row(
            proj,
            str(info.get("files", 0)),
            info.get("last_seen", ""),
        )

    console.print(table)
    console.print(f"\nStart time: {data.get('started', 'unknown')}")
