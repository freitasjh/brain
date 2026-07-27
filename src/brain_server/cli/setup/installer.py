"""Global setup — configure brain for all tools at system level."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from rich.console import Console

HOME = Path.home()
BRAIN_DIR = Path(__file__).resolve().parent.parent.parent.parent.parent

# ── Config file paths ──
CONFIG_PATHS = {
    "opencode": HOME / ".config" / "opencode" / "opencode.jsonc",
    "opencode_global": HOME / ".config" / "opencode" / "mcp.json",
    "copilot": HOME / ".config" / "gh" / "mcp.json",
    "copilot_instructions": HOME / ".config" / "Code" / "User" / "brain-copilot-instructions.md",
    "kiro": HOME / ".kiro" / "settings" / "mcp.json",
    "kiro_steering": HOME / ".kiro" / "steering" / "brain.md",
    "zshrc": HOME / ".zshrc",
    "brain_hooks": BRAIN_DIR / "hooks",
    "systemd_user": HOME / ".config" / "systemd" / "user" / "brain-server.service",
    "vscode_mcp": HOME / ".config" / "Code" / "User" / "mcp.json",
    "vscode_settings": HOME / ".config" / "Code" / "User" / "settings.json",
    "vscode_instructions": HOME / ".config" / "Code" / "User" / ".brain-copilot-instructions.md",
}


def run_setup(
    port: int,
    vault: str,
    ollama: str,
    enable_opencode: bool,
    enable_copilot: bool,
    enable_kiro: bool,
    enable_systemd: bool,
    enable_autostart: bool,
    console: Console,
    enable_copilot_rules: bool = False,
    enable_kiro_steering: bool = False,
    enable_vscode: bool = True,
) -> None:
    """Run global setup for all tools."""
    from rich.panel import Panel
    from rich.table import Table
    from rich import box

    vault_path = Path(vault).resolve()
    brain_url = f"http://localhost:{port}/sse"

    console.print(Panel(f"[bold]🧠 Brain Global Setup[/bold]\n"
                        f"Port: [cyan]{port}[/cyan] | "
                        f"Vault: [cyan]{vault_path}[/cyan] | "
                        f"URL: [cyan]{brain_url}[/cyan]",
                        title="Configuration"))

    summary = []

    # 1. OpenCode global
    if enable_opencode:
        summary.append(_setup_opencode(port, vault_path, brain_url, console))

    # 2. GitHub Copilot MCP
    if enable_copilot:
        summary.append(_setup_copilot(port, vault_path, brain_url, console))

    # 3. Kiro MCP
    if enable_kiro:
        summary.append(_setup_kiro(port, vault_path, brain_url, console))

    # 4. Copilot Instructions (rules for Copilot to use brain)
    if enable_copilot_rules:
        summary.append(_setup_copilot_instructions(console))

    # 5. Kiro Steering (rules for Kiro to use brain)
    if enable_kiro_steering:
        summary.append(_setup_kiro_steering(console))

    # 6. Systemd user service
    if enable_systemd:
        summary.append(_setup_systemd(port, vault_path, ollama, console))

    # 8. Shell autostart
    if enable_autostart:
        summary.append(_setup_autostart(port, vault_path, console))

    # 9. VS Code + Copilot instructions (legacy, kept for compat)
    if enable_vscode:
        summary.append(_setup_vscode(port, vault_path, brain_url, console))

    # ── Summary table ──
    table = Table(box=box.ROUNDED)
    table.add_column("Component", style="bold cyan")
    table.add_column("Status", style="bold")
    table.add_column("Detail", style="dim")

    for item in summary:
        status_icon = "✅" if item["ok"] else "❌"
        table.add_row(item["name"], f"{status_icon} {item['status']}", item.get("detail", ""))

    console.print()
    console.print(Panel(table, title="📋 Setup Summary"))

    # Final tips
    console.print("\n[bold]💡 Tips:[/bold]")
    console.print("  • Start brain server:  [bold]brain server start --daemon[/bold]")
    console.print("  • Check status:        [bold]brain server status[/bold]")
    console.print("  • Auto-start enabled:  já inicia com o terminal\n")


# ═══════════════════════════════════════════════════════════════════
# 1. OpenCode
# ═══════════════════════════════════════════════════════════════════

def _setup_opencode(port: int, vault_path: Path, brain_url: str, console: Console) -> dict:
    """Configure OpenCode globally with brain MCP server."""
    result = {"name": "OpenCode", "status": "", "ok": False, "detail": ""}

    config_dir = CONFIG_PATHS["opencode"].parent
    config_dir.mkdir(parents=True, exist_ok=True)

    mcp_config_path = CONFIG_PATHS["opencode_global"]

    # Load existing or create new
    if mcp_config_path.exists():
        try:
            data = json.loads(mcp_config_path.read_text())
        except (json.JSONDecodeError, ValueError):
            data = {}
    else:
        data = {}

    # Update MCP servers
    if "mcpServers" not in data:
        data["mcpServers"] = {}
    data["mcpServers"]["brain"] = {
        "transport": "sse",
        "url": brain_url,
    }

    mcp_config_path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")

    # Also update opencode.jsonc if it has an mcp section
    jsonc_path = CONFIG_PATHS["opencode"]
    if jsonc_path.exists():
        try:
            content = jsonc_path.read_text()
            # Simple JSONC parse — just strip comments (//)
            import re
            json_str = re.sub(r'//.*', '', content)
            jsonc_data = json.loads(json_str)
            if "mcp" not in jsonc_data:
                jsonc_data["mcp"] = {}
            jsonc_data["mcp"]["brain"] = {
                "type": "sse",
                "url": brain_url,
            }
            jsonc_path.write_text(
                json.dumps(jsonc_data, indent=2, ensure_ascii=False) + "\n"
            )
            result["detail"] = f"Updated {jsonc_path.name}"
        except Exception:
            result["detail"] = f"Created {mcp_config_path.name}"
    else:
        result["detail"] = f"Created {mcp_config_path.name}"

    result["ok"] = True
    result["status"] = "Configured"
    return result


# ═══════════════════════════════════════════════════════════════════
# 2. GitHub Copilot (gh CLI)
# ═══════════════════════════════════════════════════════════════════

def _setup_copilot(port: int, vault_path: Path, brain_url: str, console: Console) -> dict:
    """Configure GitHub Copilot MCP globally."""
    result = {"name": "GitHub Copilot", "status": "", "ok": False, "detail": ""}

    config_dir = CONFIG_PATHS["copilot"].parent
    config_dir.mkdir(parents=True, exist_ok=True)

    # Load existing or create new
    if CONFIG_PATHS["copilot"].exists():
        try:
            data = json.loads(CONFIG_PATHS["copilot"].read_text())
        except (json.JSONDecodeError, ValueError):
            data = {}
    else:
        data = {}

    if "servers" not in data:
        data["servers"] = {}
    data["servers"]["brain"] = {
        "type": "sse",
        "url": brain_url,
    }

    CONFIG_PATHS["copilot"].write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
    result["ok"] = True
    result["status"] = "Configured"
    result["detail"] = str(CONFIG_PATHS["copilot"])
    return result


# ═══════════════════════════════════════════════════════════════════
# 3. Kiro CLI
# ═══════════════════════════════════════════════════════════════════

def _setup_kiro(port: int, vault_path: Path, brain_url: str, console: Console) -> dict:
    """Configure Kiro CLI with brain MCP server."""
    result = {"name": "Kiro CLI", "status": "", "ok": False, "detail": ""}

    config_dir = CONFIG_PATHS["kiro"].parent
    config_dir.mkdir(parents=True, exist_ok=True)

    if CONFIG_PATHS["kiro"].exists():
        try:
            data = json.loads(CONFIG_PATHS["kiro"].read_text())
        except (json.JSONDecodeError, ValueError):
            data = {}
    else:
        data = {}

    if "mcpServers" not in data:
        data["mcpServers"] = {}
    data["mcpServers"]["brain"] = {
        "transport": "sse",
        "url": brain_url,
    }

    CONFIG_PATHS["kiro"].write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")

    # Also add permissions for brain tools
    _ensure_kiro_permissions()

    result["ok"] = True
    result["status"] = "Configured"
    result["detail"] = "brain MCP server + permissions"
    return result


def _ensure_kiro_permissions() -> None:
    """Ensure kiro permissions for brain tools."""
    perm_file = HOME / ".kiro" / "settings" / "permissions.yaml"
    if not perm_file.exists():
        return

    content = perm_file.read_text()
    brain_entry = """  - capability: mcp
    match:
      - brain/*
    effect: allow"""

    if "brain/" not in content:
        # Insert before last rules or append
        if "rules:" in content:
            # Find the rules section and add before closing
            content = content.rstrip()
            if content.endswith("..."):
                content = content[:-3] + "\n" + brain_entry + "\n..."
            else:
                content += "\n" + brain_entry + "\n"
            perm_file.write_text(content)


# ═══════════════════════════════════════════════════════════════════
# 5. Systemd user service
# ═══════════════════════════════════════════════════════════════════

def _setup_systemd(port: int, vault_path: Path, ollama_url: str, console: Console) -> dict:
    """Create systemd user service for brain auto-start."""
    result = {"name": "Systemd Service", "status": "", "ok": False, "detail": ""}

    service_dir = CONFIG_PATHS["systemd_user"].parent
    service_dir.mkdir(parents=True, exist_ok=True)

    service_content = f"""[Unit]
Description=Brain MCP Server — AI agent memory
After=network.target

[Service]
Type=simple
ExecStart={shutil.which("uv") or "uv"} run --directory {BRAIN_DIR} python -m brain_server
Restart=on-failure
RestartSec=5
Environment=BRAIN_TRANSPORT=sse
Environment=BRAIN_PORT={port}
Environment=BRAIN_VAULT_PATH={vault_path}
Environment=BRAIN_OLLAMA_URL={ollama_url}

[Install]
WantedBy=default.target
"""

    CONFIG_PATHS["systemd_user"].write_text(service_content)
    result["detail"] = str(CONFIG_PATHS["systemd_user"])

    # Try to enable and start
    try:
        subprocess.run(
            ["systemctl", "--user", "daemon-reload"],
            capture_output=True, timeout=10,
        )
        subprocess.run(
            ["systemctl", "--user", "enable", "brain-server"],
            capture_output=True, timeout=10,
        )
        subprocess.run(
            ["systemctl", "--user", "start", "brain-server"],
            capture_output=True, timeout=10,
        )
        result["status"] = "Active (enabled + started)"
    except Exception:
        result["status"] = "Created (enable manually)"
        result["detail"] += "\n  Run: systemctl --user enable --now brain-server"

    result["ok"] = True
    return result


# ═══════════════════════════════════════════════════════════════════
# 6. Shell autostart (simple rc export)
# ═══════════════════════════════════════════════════════════════════

def _setup_autostart(port: int, vault_path: Path, console: Console) -> dict:
    """Add brain env vars to shell rc."""
    result = {"name": "Shell Autostart", "status": "", "ok": False, "detail": ""}

    brain_dir_str = str(BRAIN_DIR)
    lines = [
        "\n# 🧠 Brain MCP env vars",
        f"export BRAIN_URL=http://localhost:{port}",
        f"export BRAIN_PORT={port}",
        f"export BRAIN_VAULT_PATH={vault_path}",
        f"export BRAIN_DIR={brain_dir_str}",
        'export PATH="$PATH:$HOME/.local/bin"',
    ]
    block = "\n".join(lines) + "\n"

    rc_path = CONFIG_PATHS["zshrc"]
    if not rc_path.exists():
        rc_path.write_text(block)
        result["detail"] = "Created ~/.zshrc"
    else:
        current = rc_path.read_text()
        marker = "# 🧠 Brain MCP env vars"
        if marker in current:
            result["detail"] = "Already configured"
        else:
            with open(rc_path, "a") as f:
                f.write(block)
            result["detail"] = "Appended to ~/.zshrc"

    result["ok"] = True
    result["status"] = "Configured"
    return result


# ═══════════════════════════════════════════════════════════════════
# 7. VS Code — MCP + Copilot Instructions
# ═══════════════════════════════════════════════════════════════════

def _setup_vscode(port: int, vault_path: Path, brain_url: str, console: Console) -> dict:
    """Configure VS Code: MCP server + Copilot Chat instructions."""
    result = {"name": "VS Code", "status": "", "ok": False, "detail": ""}
    details = []

    # 7a. MCP server in VS Code mcp.json
    mcp_path = CONFIG_PATHS["vscode_mcp"]
    mcp_path.parent.mkdir(parents=True, exist_ok=True)

    if mcp_path.exists():
        try:
            data = json.loads(mcp_path.read_text())
        except (json.JSONDecodeError, ValueError):
            data = {}
    else:
        data = {}

    if "servers" not in data:
        data["servers"] = {}
    data["servers"]["brain"] = {
        "type": "sse",
        "url": brain_url,
    }
    mcp_path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
    details.append(f"MCP -> {mcp_path.name}")

    # 7b. Copilot instructions file
    instr_path = CONFIG_PATHS["copilot_instructions"]
    hooks_dir = BRAIN_DIR / "hooks"
    source_instr = hooks_dir / "brain-copilot-instructions.md"

    if source_instr.exists():
        content = source_instr.read_text()
    else:
        content = _default_copilot_instructions()
    instr_path.write_text(content)
    details.append(f"Instructions -> {instr_path.name}")

    # 7c. Point VS Code settings to the instructions file
    settings_path = CONFIG_PATHS["vscode_settings"]
    if settings_path.exists():
        try:
            settings = json.loads(settings_path.read_text())
        except (json.JSONDecodeError, ValueError):
            settings = {}
    else:
        settings = {}

    settings["github.copilot.chat.codeGeneration.instructions"] = [{
        "file": str(instr_path),
    }]

    if "github.copilot.chat.mcpDiscovery.servers" not in settings:
        settings["github.copilot.chat.mcpDiscovery.servers"] = []
    if "brain" not in settings["github.copilot.chat.mcpDiscovery.servers"]:
        settings["github.copilot.chat.mcpDiscovery.servers"].append("brain")

    settings_path.write_text(json.dumps(settings, indent=2, ensure_ascii=False) + "\n")
    details.append("VS Code settings.json updated")

    result["ok"] = True
    result["status"] = "Configured"
    result["detail"] = " | ".join(details)
    return result


# ═══════════════════════════════════════════════════════════════════
# 8. Copilot Instructions — rules for how Copilot should use brain
# ═══════════════════════════════════════════════════════════════════

def _setup_copilot_instructions(console: Console) -> dict:
    """Install brain rules/instructions for GitHub Copilot Chat.

    Installs the instructions file to ~/.config/Code/User/ and
    updates VS Code settings.json to reference it.
    """
    result = {"name": "Copilot Instructions", "status": "", "ok": False, "detail": ""}

    instr_path = CONFIG_PATHS["copilot_instructions"]
    instr_path.parent.mkdir(parents=True, exist_ok=True)

    hooks_file = CONFIG_PATHS["brain_hooks"] / "brain-copilot-instructions.md"
    if hooks_file.exists():
        content = hooks_file.read_text()
    else:
        content = _default_copilot_instructions()

    instr_path.write_text(content)
    detail = f"Created {instr_path}"

    # Update VS Code settings.json
    settings_path = CONFIG_PATHS["vscode_settings"]
    if settings_path.exists():
        try:
            settings = json.loads(settings_path.read_text())
        except (json.JSONDecodeError, ValueError):
            settings = {}
    else:
        settings = {}

    settings["github.copilot.chat.codeGeneration.instructions"] = [{
        "file": str(instr_path),
    }]
    settings_path.parent.mkdir(parents=True, exist_ok=True)
    settings_path.write_text(json.dumps(settings, indent=2, ensure_ascii=False) + "\n")
    detail += " | VS Code settings.json updated"

    result["ok"] = True
    result["status"] = "Installed"
    result["detail"] = detail
    return result


# ═══════════════════════════════════════════════════════════════════
# 9. Kiro Steering — rules for how Kiro agents should use brain
# ═══════════════════════════════════════════════════════════════════

def _setup_kiro_steering(console: Console) -> dict:
    """Install brain steering rules for Kiro CLI agents.

    Creates ~/.kiro/steering/brain.md with `inclusion: always` so
    every Kiro agent session has brain MCP usage rules.
    """
    result = {"name": "Kiro Steering", "status": "", "ok": False, "detail": ""}

    steering_path = CONFIG_PATHS["kiro_steering"]
    steering_path.parent.mkdir(parents=True, exist_ok=True)

    hooks_file = CONFIG_PATHS["brain_hooks"] / "brain-kiro-steering.md"
    if hooks_file.exists():
        content = hooks_file.read_text()
    else:
        content = _default_kiro_steering()

    steering_path.write_text(content)
    result["detail"] = f"Created {steering_path}"

    # Ensure kiro has proper permissions for brain tools
    _ensure_kiro_permissions()

    result["ok"] = True
    result["status"] = "Installed"
    return result


# ═══════════════════════════════════════════════════════════════════
# Default embedded content (fallback if hooks files are missing)
# ═══════════════════════════════════════════════════════════════════

def _default_copilot_instructions() -> str:
    """Return default Copilot instructions markdown."""
    return """# Brain MCP - Instrucoes para o GitHub Copilot

Voce TEM acesso ao Brain MCP Server - um cerebro central com memoria persistente.

## Tools

### brain_search(query, [layer], [top_k])
Busca semantica por similaridade. Resultados ordenados por score (0..1).
Parametros: query (obrig), layer (opc), top_k (opc, default 5).

### brain_store(layer, path, content)
Salva nota markdown + indexa para busca semantica.
Camadas validas: arquitetura, regras, sessoes, projetos.

### brain_read(layer, path)
Le nota completa do vault.

### brain_reindex([all], [layer], [path])
Reconstroi indice de embeddings.

## Regras obrigatorias
1. ANTES de codificar: brain_search para carregar contexto
2. APOS decisoes: brain_store para registrar
3. AO FINALIZAR: brain_store resumo em "sessoes"

## Camadas
| Camada | Conteudo |
|--------|----------|
| arquitetura | Stack, modulos, decisoes tecnicas |
| regras | Regras de negocio, nomenclatura |
| sessoes | Resumo de sessao, handoff |
| projetos | Visao geral do projeto |
"""


def _default_kiro_steering() -> str:
    """Return default Kiro steering markdown."""
    return """---
inclusion: always
---

# Brain MCP - Memory & Context Rules

Use Brain MCP Server para memoria persistente entre sessoes.

## Tools

### brain_search(query, [layer], [top_k])
Busca semantica por similaridade.

### brain_store(layer, path, content)
Salva nota markdown + indexa.

### brain_read(layer, path)
Le nota completa do vault.

### brain_reindex([all], [layer], [path])
Reconstroi indice de embeddings.

## Regras obrigatorias
1. ANTES de codificar: brain_search("feature")
2. APOS decisoes: brain_store("arquitetura", ...)
3. AO FINALIZAR: brain_store("sessoes", ...)

## Camadas
| Camada | Conteudo |
|--------|----------|
| arquitetura | Stack, modulos, decisoes tecnicas |
| regras | Regras de negocio, nomenclatura |
| sessoes | Resumo de sessao, handoff |
| projetos | Visao geral do projeto |
"""
