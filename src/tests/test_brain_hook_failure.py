"""X-01: `hooks/brain-hook.py` must not report a failed capture as content.

The wrapper used to return `result.stdout.strip()` without ever reading
`returncode`, and turned exceptions into the string `"(hook error: ...)"`. Every
caller then decided usability with `if r and "No results" not in r and "ERROR" not
in r`, so a subprocess that exited 1 was indistinguishable from a search that
found nothing — the agent was handed an empty string where its context should have
been, and the event was never captured, with nothing logged.

These are unit tests over `_call_brain` with the subprocess call substituted, so
they assert the contract rather than a live brain: a non-zero exit is reported and
is rejected by the callers' own filter.
"""
from __future__ import annotations

import importlib.util
import subprocess
import sys
from pathlib import Path

import pytest

_HOOK_PATH = Path(__file__).resolve().parents[2] / "hooks" / "brain-hook.py"


def _load_hook():
    """Import `hooks/brain-hook.py` by path.

    It is a script, not an installed module, and importing it emits the
    deprecation warning it is meant to emit.
    """
    spec = importlib.util.spec_from_file_location("brain_hook", _HOOK_PATH)
    module = importlib.util.module_from_spec(spec)
    with pytest.warns(DeprecationWarning):
        spec.loader.exec_module(module)
    return module


hook = _load_hook()


class _Completed:
    def __init__(self, returncode: int, stdout: str, stderr: str = ""):
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr


def test_a_non_zero_exit_is_reported_and_never_returned_as_content(monkeypatch, capsys):
    """`exit 1` must not come back as a usable result."""
    monkeypatch.setattr(
        hook.subprocess,
        "run",
        lambda *a, **k: _Completed(1, "", "Error: database is locked\n"),
    )
    result = hook._call_brain("search", ["padrões"])

    assert result.startswith("ERROR:"), f"a failed call must be marked, got {result!r}"
    assert "exited 1" in result, result
    # The caller filter is `"ERROR" not in r`; this is what makes a failure skip
    # injection rather than become the injected context.
    assert "ERROR" in result, "callers reject results containing ERROR"
    assert "database is locked" in result, f"the underlying reason must survive: {result!r}"
    # And it is said out loud, not only returned.
    assert "database is locked" in capsys.readouterr().err


def test_a_failing_call_produces_no_search_results_shape(monkeypatch):
    """The exact condition the callers use must be false for a failure."""
    monkeypatch.setattr(hook.subprocess, "run", lambda *a, **k: _Completed(1, ""))

    def usable(r: str) -> bool:
        return bool(r) and "No results" not in r and "ERROR" not in r

    assert not usable(hook._call_brain("search", ["x"])), (
        "a failed call must not be treated as usable context"
    )


def test_an_exception_is_reported_rather_than_stringified_into_content(monkeypatch):
    monkeypatch.setattr(
        hook.subprocess,
        "run",
        lambda *a, **k: (_ for _ in ()).throw(OSError("uv not found")),
    )
    result = hook._call_brain("store", ["sessoes", "p/x", "## y"])
    assert result.startswith("ERROR:"), result
    assert "uv not found" in result, result


def test_a_timeout_is_reported(monkeypatch):
    def _timeout(*a, **k):
        raise subprocess.TimeoutExpired(cmd="brain", timeout=30)

    monkeypatch.setattr(hook.subprocess, "run", _timeout)
    result = hook._call_brain("search", ["x"])
    assert result.startswith("ERROR:"), result
    assert "timed out" in result, result


def test_a_successful_call_still_returns_stdout(monkeypatch):
    """The fix must not break the path that works: content passes through."""
    monkeypatch.setattr(hook.subprocess, "run", lambda *a, **k: _Completed(0, "  results here\n"))
    assert hook._call_brain("search", ["x"]) == "results here"


def test_a_failure_with_output_on_stdout_is_still_a_failure(monkeypatch):
    """A non-zero exit wins over whatever the process managed to print."""
    monkeypatch.setattr(hook.subprocess, "run", lambda *a, **k: _Completed(2, "partial output"))
    result = hook._call_brain("read", ["regras", "x"])
    assert result.startswith("ERROR:"), result
    assert "partial output" not in result, "a failed call must not leak partial stdout as content"


# ------------------------------------------------------------------ Y-06 --
#
# `returncode` inspection (X-01) made a failure *visible* — stderr, and a return
# value every caller's filter rejects — but `main()` still exited 0 on the three
# capture paths. An IDE hook configuration that only inspects the exit code
# therefore recorded a success for a capture that never happened, which is the
# half of the problem that reporting alone does not fix.
#
# The invariant these pin: **a capture path exits non-zero when, and only when, a
# capture was attempted and failed** — and `session-start` never exits non-zero,
# because its stdout is injected context that a host may discard on a non-zero
# exit.
#
# `pytest` is not installed in every environment this runs in (it is not in the
# Rust developer's), so the same assertions are also executed directly by
# `python3 -c` against the imported module; see the batch report. These are the
# real tests.

def test_a_failed_tool_result_capture_exits_non_zero(monkeypatch):
    monkeypatch.setattr(hook.subprocess, "run", lambda *a, **k: _Completed(1, ""))
    monkeypatch.setattr(hook, "_read_stdin", lambda: {"tool": "edit", "result": "x"})
    assert hook.on_tool_result() is False, "a failed capture must report failure to main()"


def test_a_failed_session_end_capture_exits_non_zero(monkeypatch):
    monkeypatch.setattr(hook.subprocess, "run", lambda *a, **k: _Completed(1, ""))
    assert hook.on_session_end() is False


def test_a_failed_prompt_submit_capture_exits_non_zero(monkeypatch):
    monkeypatch.setattr(hook.subprocess, "run", lambda *a, **k: _Completed(1, ""))
    monkeypatch.setattr(hook, "_read_stdin", lambda: {"prompt": "x"})
    assert hook.on_prompt_submit() is False


def test_a_successful_capture_exits_zero(monkeypatch):
    """The fix must not break the path that works."""
    monkeypatch.setattr(hook.subprocess, "run", lambda *a, **k: _Completed(0, "stored"))
    monkeypatch.setattr(hook, "_read_stdin", lambda: {"tool": "edit", "result": "x"})
    assert hook.on_tool_result() is True
    assert hook.on_session_end() is True
    monkeypatch.setattr(hook, "_read_stdin", lambda: {"prompt": "x"})
    assert hook.on_prompt_submit() is True


def test_an_empty_payload_is_not_a_failure(monkeypatch):
    """Nothing to capture is not a failed capture — the hook must not cry wolf."""
    monkeypatch.setattr(hook, "_read_stdin", lambda: {})
    assert hook.on_tool_result() is True
    assert hook.on_prompt_submit() is True


def test_the_failure_predicate_does_not_misfire_on_content_mentioning_error(monkeypatch):
    """A successful search whose *content* says "ERROR" is still a success.

    This is why `_failed` keys on the marker prefix instead of a substring test:
    a rule about handling errors is a perfectly ordinary note, and a substring
    check would report that capture as failed.
    """
    monkeypatch.setattr(hook.subprocess, "run", lambda *a, **k: _Completed(0, "never log ERROR: handle it"))
    assert hook._call_brain("search", ["x"]) == "never log ERROR: handle it"
    assert hook._failed(hook._call_brain("search", ["x"])) is False
    # And the real failure is still caught.
    monkeypatch.setattr(hook.subprocess, "run", lambda *a, **k: _Completed(1, ""))
    assert hook._failed(hook._call_brain("search", ["x"])) is True


def test_session_start_never_reports_failure_to_main(monkeypatch):
    """`session-start` is pinned to success even when every search failed.

    Its stdout is injected context, and a non-zero exit can make the host discard
    it — so failing loudly would cost the user the context this path exists to
    supply, to report a condition stderr already reports.
    """
    monkeypatch.setattr(hook.subprocess, "run", lambda *a, **k: _Completed(1, ""))
    assert hook.on_session_start() is None, (
        "session-start must not signal failure: its output is injected and a non-zero exit can cost it"
    )


def test_main_exits_one_on_a_failed_capture_and_zero_on_session_start(monkeypatch, capsys):
    """The end-to-end claim: the exit code itself, not just the return value."""
    monkeypatch.setattr(hook.subprocess, "run", lambda *a, **k: _Completed(1, ""))
    monkeypatch.setattr(hook, "_read_stdin", lambda: {"tool": "edit", "result": "x"})

    monkeypatch.setattr(sys, "argv", ["brain-hook.py", "tool-result"])
    with pytest.raises(SystemExit) as exc:
        hook.main()
    assert exc.value.code == 1, "a failed capture must exit non-zero"
    assert "did not reach brain" in capsys.readouterr().err, "and say so on stderr"

    monkeypatch.setattr(sys, "argv", ["brain-hook.py", "session-start"])
    hook.main()  # must NOT raise SystemExit


def test_the_failure_message_does_not_promise_a_retry_that_does_not_exist(monkeypatch, capsys):
    """The operator-facing half of Z-01: say what actually happens.

    The message used to say `brain hook` "spools the event and retries on its own".
    Nothing reads the spool — there is one writer (`hook_handle`) and no drain in any
    crate — so `brain hook` does its work inline: if the write fails, the event is in
    the spool and *only* in the spool, and nothing replays it. The payload is not lost
    (it is a full JSON line on disk), but the sentence told an operator to wait for a
    recovery that does not exist, and that is the one failure mode in this file that
    makes someone act wrongly rather than merely misinformed.

    So the guarantee pinned here is a negative one, and it is the useful kind: the
    forbidden phrase cannot come back, and what replaces it has to name the absence of
    a retry rather than stay silent about it.
    """
    monkeypatch.setattr(hook.subprocess, "run", lambda *a, **k: _Completed(1, ""))
    monkeypatch.setattr(hook, "_read_stdin", lambda: {"tool": "edit", "result": "x"})

    monkeypatch.setattr(sys, "argv", ["brain-hook.py", "tool-result"])
    with pytest.raises(SystemExit):
        hook.main()
    err = capsys.readouterr().err

    assert "retries on its own" not in err, (
        "the message must not promise an automatic retry: nothing drains the spool. Got: " + err
    )
    assert "no retry" in err, (
        "and it must state the absence of one, so re-sending is a deliberate act: " + err
    )
    # The advice has to be actionable, or "no retry" is only the absence of a promise.
    # Case-insensitive on purpose: what is being pinned is that a next step is offered,
    # not how the sentence happens to be capitalised.
    assert "re-send the event" in err.lower(), (
        "an operator told there is no retry needs the next step: " + err
    )


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-v"]))
