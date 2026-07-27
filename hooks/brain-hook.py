#!/usr/bin/env python3
"""brain-hook.py — Hook script for AI agent lifecycle events.

Connects to the brain MCP server and auto-captures context.

Usage:
    brain-hook.py session-start                    # load context on session start
    brain-hook.py tool-result < json-input         # capture tool results
    brain-hook.py session-end                      # save session summary
    brain-hook.py prompt-submit < json-input       # capture prompts

Environment:
    BRAIN_URL    MCP server URL (default: http://localhost:8321)
    BRAIN_PROJECT  Project name (default: detected from git)
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from datetime import datetime, timezone


def _detect_project() -> str:
    """Detect project name from git remote or cwd."""
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True, text=True, timeout=3,
        )
        if result.returncode == 0:
            return os.path.basename(result.stdout.strip())
    except Exception:
        pass
    return os.path.basename(os.getcwd())


def _call_brain(tool: str, positional: list[str] | None = None, **options) -> str:
    """Call brain CLI tool via uv run.
    
    Args:
        tool: The brain CLI command (e.g. 'search', 'store', 'read')
        positional: Positional arguments for the command (e.g. [layer, path, content])
        **options: Optional flags (e.g. layer='regras', scope='global', top_k=3)
    """
    import subprocess
    brain_dir = os.environ.get("BRAIN_DIR") or os.path.join(
        os.path.dirname(__file__), ".."
    )
    args = ["uv", "run", "--directory", brain_dir, "brain", tool]
    
    # Add positional arguments first
    if positional:
        args.extend(positional)
    
    # Add optional flags
    for k, v in options.items():
        if v is not None:
            args.extend([f"--{k.replace('_', '-')}", str(v)])
    
    try:
        result = subprocess.run(
            args, capture_output=True, text=True, timeout=30,
            env={**os.environ, "BRAIN_URL": os.environ.get("BRAIN_URL", "http://localhost:8321")},
        )
        return result.stdout.strip()
    except Exception as e:
        return f"(hook error: {e})"


def _read_stdin() -> dict:
    """Read JSON from stdin."""
    try:
        return json.loads(sys.stdin.read())
    except (json.JSONDecodeError, EOFError):
        return {}


def on_session_start():
    """Load relevant context from brain when session starts.
    
    Searches for:
    1. Global lessons/standards (shared across all projects)
    2. Project-specific rules and architecture
    """
    project = os.environ.get("BRAIN_PROJECT") or _detect_project()
    results = []

    # Search for GLOBAL context (shared lessons, standards)
    for layer in ["regras", "arquitetura"]:
        r = _call_brain("search", ["padrões melhores práticas lições"], 
                       layer=layer, scope="global", top_k=3)
        if r and "No results" not in r and "ERROR" not in r:
            results.append((f"{layer} [scope=global]", r))

    # Search for PROJECT-SPECIFIC context
    for layer, query in [
        ("projetos", f"projeto {project}"),
        ("regras", f"regras {project}"),
        ("arquitetura", f"arquitetura {project}"),
    ]:
        if layer in ["regras", "arquitetura"]:
            # These layers require scope
            r = _call_brain("search", [query], layer=layer, scope="projetos", top_k=3)
        else:
            # projetos/sessoes don't use scope
            r = _call_brain("search", [query], layer=layer, top_k=3)
        
        if r and "No results" not in r and "ERROR" not in r:
            results.append((f"{layer} [scope=projetos]", r))

    # Write results to stdout so the agent can inject them
    for scope_label, r in results:
        print(f"\n--- Brain context ({scope_label}) ---")
        print(r)


def on_tool_result():
    """Capture tool execution results."""
    data = _read_stdin()
    if not data:
        return

    project = os.environ.get("BRAIN_PROJECT") or _detect_project()
    tool_name = data.get("tool") or data.get("name") or "unknown"
    result = json.dumps(data.get("result") or data.get("output") or data, ensure_ascii=False)[:2000]

    if result and result != "null":
        _call_brain(
            "store",
            positional=["sessoes", f"{project}/tool-{tool_name}", f"## {tool_name}\n\n```json\n{result}\n```"],
        )


def on_session_end():
    """Save session end timestamp."""
    project = os.environ.get("BRAIN_PROJECT") or _detect_project()
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    _call_brain(
        "store",
        positional=["sessoes", f"{project}/session-end", f"## Session End\n\nProject: {project}\nTime: {ts}"],
    )


def on_prompt_submit():
    """Capture user prompts."""
    data = _read_stdin()
    if not data:
        return

    project = os.environ.get("BRAIN_PROJECT") or _detect_project()
    prompt = json.dumps(data, ensure_ascii=False)[:2000]

    _call_brain(
        "store",
        positional=[
            "sessoes",
            f"{project}/prompt-{datetime.now(timezone.utc).strftime('%Y%m%d%H%M%S')}",
            f"## Prompt\n\n```\n{prompt}\n```",
        ],
    )


def main():
    if len(sys.argv) < 2:
        print("Usage: brain-hook.py <event> [--stdin]")
        print("Events: session-start, tool-result, session-end, prompt-submit")
        sys.exit(1)

    event = sys.argv[1]

    if event == "session-start":
        on_session_start()
    elif event == "tool-result":
        on_tool_result()
    elif event == "session-end":
        on_session_end()
    elif event == "prompt-submit":
        on_prompt_submit()
    else:
        print(f"Unknown event: {event}")
        sys.exit(1)


if __name__ == "__main__":
    main()
