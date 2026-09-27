#!/usr/bin/env python3
"""brain-hook.py — Hook script for AI agent lifecycle events.

.. deprecated::
   Use Rust `brain hook --event session-start|tool-result|session-end --project --payload JSON`
   spool `XDG_RUNTIME_DIR/brain/hook-spool.jsonl` file lock instead.
   This Python hook will be removed in Fase C final. See `brain hook --help`.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import warnings
from datetime import datetime, timezone

warnings.warn("hooks/brain-hook.py is deprecated, use `brain hook --event` (Rust)", DeprecationWarning, stacklevel=2)


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


#: Prefix every failed call carries in its return value.
#:
#: The callers below decide whether a result is usable with
#: ``if r and "No results" not in r and "ERROR" not in r``, so a failure that
#: returns anything else is silently injected into the agent's context as if it
#: were search output. Routing every failure through one marker means a new caller
#: inherits the rejection instead of having to remember it.
_ERROR_MARKER = "ERROR: brain hook"


def _report_failure(args: list[str], detail: str, stderr: str = "") -> str:
    """Report a failed `brain` call and return the value callers must reject.

    X-01. `_call_brain` used to return `result.stdout.strip()` without ever
    looking at `returncode`, and swallow exceptions into the string
    `"(hook error: ...)"`. Both are worse than failing: the agent saw a truncated
    or empty string where context should have been, and nothing anywhere said a
    capture had been lost. The reviewer measured the same shape failing outright
    — `exit 1` from the subprocess, returncode never inspected.

    **Reported, not re-enqueued, and that is a decision rather than an omission.**
    Re-enqueueing would mean writing the Rust spool's own record format from a
    deprecated Python file, duplicating the dedup key and field layout that
    `brain hook` owns — a second implementation of a contract that would then be
    able to drift from the one that reads it. The supported capture path is the
    Rust `brain hook` subcommand; this wrapper's job is to not lie about failure.
    """
    tail = (stderr or "").strip().splitlines()
    detail_line = tail[-1] if tail else "(no stderr)"
    print(
        f"{_ERROR_MARKER}: `{' '.join(args)}` {detail}: {detail_line}",
        file=sys.stderr,
    )
    return f"{_ERROR_MARKER}: {detail}: {detail_line}"


def _call_brain(tool: str, positional: list[str] | None = None, **options) -> str:
    """Call brain CLI tool via uv run.

    Args:
        tool: The brain CLI command (e.g. 'search', 'store', 'read')
        positional: Positional arguments for the command (e.g. [layer, path, content])
        **options: Optional flags (e.g. layer='regras', scope='global', top_k=3)

    Returns:
        The command's stdout on success, or an ``ERROR:``-prefixed string that
        every caller in this file already rejects. A non-zero exit is never
        returned as if it were content.
    """
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
    except subprocess.TimeoutExpired:
        return _report_failure(args, "timed out after 30s")
    except Exception as e:
        return _report_failure(args, f"could not be run ({e!r})")

    if result.returncode != 0:
        return _report_failure(args, f"exited {result.returncode}", result.stderr or "")
    return result.stdout.strip()


def _failed(result: str) -> bool:
    """True when `_call_brain` reported a failure rather than content.

    Y-06. The single predicate the capture paths use to decide their exit code.

    Keyed on the prefix rather than a bare `"ERROR" in result`, because a
    **successful** `brain search` can legitimately print a note whose text
    contains the word — a rule about handling errors, say — and a substring test
    would then report a capture that worked as a capture that failed. The marker
    is the first thing `_report_failure` puts in the string, and nothing else
    puts it there.
    """
    return result.startswith(_ERROR_MARKER)


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

    Y-06: this path **always exits 0**, even when every search failed, and that is
    deliberate rather than an oversight. Its stdout is injected into the agent's
    context by the IDE, and a hook that exits non-zero is treated very differently
    across them: a non-zero exit commonly means "this hook failed", and a host
    that reacts by discarding the output loses the context this function exists to
    supply. Failing loudly here would therefore cost the user their context to
    report a condition the stderr line already reports. The capture paths below
    have no such constraint — they produce no injected output — so they do exit
    non-zero.
    """
    project = os.environ.get("BRAIN_PROJECT") or _detect_project()
    results = []

    # Search for GLOBAL context (shared lessons, standards)
    for layer in ["regras", "arquitetura"]:
        r = _call_brain("search", ["padrões melhores práticas lições"],
                       layer=layer, scope="global", top_k=3)
        # Z-05.6, evaluated. These two conditions are **not** two spellings of one
        # check, and unifying them would be a mistake:
        #
        # - `not _failed(r)` is the error gate. It keys on the `_ERROR_MARKER` *prefix*,
        #   which is what Y-06 changed it to, and what stops a failed call from being
        #   injected as if it were search output.
        # - `"No results" not in r` was a *different* predicate — a search that
        #   succeeded and found nothing — expressed as a content substring.
        #
        # The first stays a prefix test because a substring test on "ERROR" misfires on
        # ordinary notes about handling errors. The second is a leftover from the
        # pre-Rust `brain_server`, whose search printed the words "No results". The
        # current Rust CLI prints `{"results": [], "total": 0}`, so **that token never
        # occurs** and the check is currently vacuous: an empty result set does not get
        # filtered, and the section below is emitted with an empty body. The same
        # latent false positive Y-06 fixed for "ERROR" is still open here — a note whose
        # content contains the phrase "No results" would silently lose its whole context
        # section.
        #
        # Left as-is deliberately: the correct replacement is a check on the result
        # *count* rather than on substrings, which is a behaviour change in a file the
        # Fase C removes, and this batch does not change behaviour. Recorded here so the
        # decision is visible rather than rediscovered.
        if r and "No results" not in r and not _failed(r):
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
        
        if r and "No results" not in r and not _failed(r):
            results.append((f"{layer} [scope=projetos]", r))

    # Write results to stdout so the agent can inject them
    for scope_label, r in results:
        print(f"\n--- Brain context ({scope_label}) ---")
        print(r)


def on_tool_result() -> bool:
    """Capture tool execution results.

    Returns False when the capture was attempted and failed, so `main` can exit
    non-zero. Y-06: this used to discard the return of `_call_brain` entirely, so
    an IDE hook configuration that only inspects the exit code recorded a success
    for a capture that never happened — and this path produces no injected
    context, so a non-zero exit costs the user nothing.
    """
    data = _read_stdin()
    if not data:
        # No payload is not a failure: there was nothing to capture.
        return True

    project = os.environ.get("BRAIN_PROJECT") or _detect_project()
    tool_name = data.get("tool") or data.get("name") or "unknown"
    result = json.dumps(data.get("result") or data.get("output") or data, ensure_ascii=False)[:2000]

    if not result or result == "null":
        return True
    return not _failed(_call_brain(
        "store",
        positional=["sessoes", f"{project}/tool-{tool_name}", f"## {tool_name}\n\n```json\n{result}\n```"],
    ))


def on_session_end() -> bool:
    """Save session end timestamp. See [`on_tool_result`] for the exit code."""
    project = os.environ.get("BRAIN_PROJECT") or _detect_project()
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    return not _failed(_call_brain(
        "store",
        positional=["sessoes", f"{project}/session-end", f"## Session End\n\nProject: {project}\nTime: {ts}"],
    ))


def on_prompt_submit() -> bool:
    """Capture user prompts. See [`on_tool_result`] for the exit code."""
    data = _read_stdin()
    if not data:
        return True

    project = os.environ.get("BRAIN_PROJECT") or _detect_project()
    prompt = json.dumps(data, ensure_ascii=False)[:2000]

    return not _failed(_call_brain(
        "store",
        positional=[
            "sessoes",
            f"{project}/prompt-{datetime.now(timezone.utc).strftime('%Y%m%d%H%M%S')}",
            f"## Prompt\n\n```\n{prompt}\n```",
        ],
    ))


def main():
    if len(sys.argv) < 2:
        print("Usage: brain-hook.py <event> [--stdin]")
        print("Events: session-start, tool-result, session-end, prompt-submit")
        sys.exit(1)

    event = sys.argv[1]

    # Y-06. `session-start` is pinned to 0 in every case, including total failure:
    # its stdout is injected context, and a non-zero exit can make the host throw
    # that context away. The other three produce no output, so a failed capture
    # exits 1 and an IDE that only watches the exit code learns about it.
    if event == "session-start":
        on_session_start()
        return
    ok = True
    if event == "tool-result":
        ok = on_tool_result()
    elif event == "session-end":
        ok = on_session_end()
    elif event == "prompt-submit":
        ok = on_prompt_submit()
    else:
        print(f"Unknown event: {event}")
        sys.exit(1)
    if not ok:
        # Z-01. The previous wording here said `brain hook` "spools the event and
        # retries on its own". That was false, and it was false in the direction
        # that costs an event: the spool has a writer (`hook_handle`) and **no
        # reader anywhere** — no drain, no recovery pass, in any crate. What
        # `brain hook` actually does is append the event to the spool *first* and
        # only then touch the database, so a failure in `note_append_section` or
        # `sync_note_chunks` propagates, exits 1, and leaves the event in the spool
        # and nowhere else. The payload is not lost — it is a full JSON line on
        # disk — but nothing in the system will ever replay it, so saying "retries
        # on its own" told an operator to wait for a recovery that does not exist.
        #
        # The advice below is the accurate one, and the dedup claim is checked
        # rather than assumed: `dedup_key` is the payload's `id` when it has one,
        # so re-sending the same event is a no-op if the first attempt in fact
        # landed and an append if it did not.
        print(
            f"{_ERROR_MARKER}: the '{event}' capture did not reach brain. Nothing was enqueued. See the "
            "stderr line above for the cause. The supported capture path is `brain hook` (Rust), which "
            "spools the event before it touches the database — so on a failed write the event exists only "
            "in that spool, and nothing reads the spool back: there is no retry. Re-send the event, or "
            "re-run `brain hook` with the same payload id once the database is writable; the dedup marker "
            "makes the second attempt a no-op if the first one landed after all.",
            file=sys.stderr,
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
