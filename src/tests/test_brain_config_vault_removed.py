"""B6b — `BRAIN_VAULT_PATH` is no longer a setting, and the warning survives.

The spec's B6 says *remove* it from `config.py:45`. What was there instead was a
field annotated "deprecated" that still parsed `BRAIN_VAULT_PATH` **and** still
gained a default, so the variable kept working and nothing ever stopped using it.
A deprecation warning next to a live field is not a deprecation.

These tests pin the three properties that make it actually gone, because each one
is a different way the change could be made cosmetically:

1. the field is gone from the model, so nothing can read it;
2. the variable is *ignored* rather than fatal, so a leftover export does not
   turn the legacy server into a startup crash;
3. the warning still fires, because it is the only thing that tells the operator
   their variable stopped doing anything.
"""

from __future__ import annotations

import os
import warnings
from pathlib import Path

import pytest

from brain_server.config import Settings, legacy_vault_path


def test_settings_has_no_vault_path_field():
    """The field is absent, not merely annotated as deprecated.

    Asserted through the pydantic model rather than with `hasattr`, because
    `hasattr(Settings(), "vault_path")` is false for a field that was declared
    and left unset, and true for one that was declared and filled in — neither of
    which is the question. The question is whether the name is part of the
    model's contract, which is `model_fields`.
    """
    assert "vault_path" not in Settings.model_fields, (
        "BRAIN_VAULT_PATH was removed from the spec's B6, so the field must be "
        f"gone, not deprecated. Present: {sorted(Settings.model_fields)}"
    )


def test_brain_vault_path_is_ignored_rather_than_fatal(monkeypatch, tmp_path: Path):
    """A leftover variable must not crash the legacy server.

    `pydantic-settings` ignores unknown `BRAIN_`-prefixed variables by default,
    but that is a library behaviour, not a property this file can rely on
    silently: it changed between major versions, and if it ever becomes
    `extra="forbid"` the legacy server stops starting for every operator who has
    the variable in their `.env`. Asserting the outcome here is what turns it into
    a property of *this* code.
    """
    monkeypatch.setenv("BRAIN_VAULT_PATH", str(tmp_path / "vault"))
    with warnings.catch_warnings():
        # The warning is asserted separately; here it would only be noise.
        warnings.simplefilter("ignore", DeprecationWarning)
        s = Settings()
    assert not hasattr(s, "vault_path")
    # And the variable did not quietly become the db path, which is the shape the
    # bug took when `index_path`/`db_path` fell through to each other.
    assert s.db_path is not None
    assert str(tmp_path / "vault") not in str(s.db_path)


def test_the_deprecation_warning_still_fires(monkeypatch, tmp_path: Path):
    """The warning is the only signal left, so it must not go quiet.

    Without the field there is nothing for a caller to notice: setting the
    variable produces no attribute, no error, and no log line. The warning is
    therefore the whole of the migration message, and its wording has to say the
    variable is **ignored** — "deprecated" alone reads as "still works, will be
    removed later", which after this change is false.
    """
    monkeypatch.setenv("BRAIN_VAULT_PATH", str(tmp_path / "vault"))
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        Settings()
    deprecations = [w for w in caught if issubclass(w.category, DeprecationWarning)]
    assert len(deprecations) == 1, f"expected exactly one DeprecationWarning, got {caught}"
    text = str(deprecations[0].message)
    assert "BRAIN_VAULT_PATH" in text
    assert "BRAIN_DB_PATH" in text, f"the warning must name the replacement: {text}"
    assert "ignored" in text, f"the warning must say the variable does nothing: {text}"


def test_no_warning_when_the_variable_is_absent(monkeypatch):
    """A clean environment must be silent, or the warning is noise nobody reads."""
    monkeypatch.delenv("BRAIN_VAULT_PATH", raising=False)
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        Settings()
    assert not [w for w in caught if issubclass(w.category, DeprecationWarning)]


def test_legacy_vault_path_follows_brain_dir(monkeypatch, tmp_path: Path):
    """The legacy server still finds a vault, at the location the field defaulted to.

    The field used to fill itself in with `<base>/vault`, so an operator who was
    not overriding the variable saw no change when it was removed. That property
    has to be asserted, not assumed: without it, removing the field would have
    quietly pointed the legacy server at a different directory than the one it
    read yesterday.
    """
    monkeypatch.setenv("BRAIN_DIR", str(tmp_path))
    assert legacy_vault_path() == tmp_path / "vault"


def test_legacy_vault_path_ignores_the_removed_variable(monkeypatch, tmp_path: Path):
    """`BRAIN_VAULT_PATH` must not steer the legacy server either.

    This is the behaviour change B6b is asking for, and the test that names it:
    an operator who was overriding the variable no longer gets their directory.
    Asserted explicitly so the change is recorded as intended rather than
    discovered later as a broken vault.
    """
    monkeypatch.setenv("BRAIN_DIR", str(tmp_path))
    monkeypatch.setenv("BRAIN_VAULT_PATH", str(tmp_path / "somewhere-else"))
    assert legacy_vault_path() == tmp_path / "vault"


def test_the_legacy_server_module_imports_without_the_removed_field():
    """`server.py` used `settings.vault_path` in three places.

    A module-level import does not catch that — the reads are inside functions, so
    the module imports fine and only raises `AttributeError` the first time the
    legacy server is actually started, which is the moment nobody is watching a
    test suite. So the three call sites are checked directly.

    The scan is over the **AST**, not the text. A text scan also matches the
    comment explaining that the field is gone, which means a correct file fails
    the test and the only way to make it pass is to delete the explanation — the
    exact incentive a reviewer should not be offered.
    """
    import ast
    import inspect

    from brain_server import server

    tree = ast.parse(inspect.getsource(server))
    reads = [
        node.lineno
        for node in ast.walk(tree)
        if isinstance(node, ast.Attribute)
        and node.attr == "vault_path"
        and isinstance(node.value, ast.Name)
        and node.value.id == "settings"
    ]
    assert not reads, (
        f"brain_server.server still reads the removed field at line(s) {reads}; it would raise "
        "AttributeError the first time the legacy server starts"
    )
    # And the replacement is wired in, so the fix is not "delete the reads".
    names = {n.id for n in ast.walk(tree) if isinstance(n, ast.Name)}
    assert "legacy_vault_path" in names, f"server.py no longer resolves a vault at all: {sorted(names)}"
    assert inspect.isfunction(server.create_server)
    assert inspect.isfunction(server.main)


@pytest.mark.parametrize("module", ["brain_server.vault.manager"])
def test_vault_manager_keeps_its_own_attribute(module: str):
    """`VaultManager.vault_path` is a different thing and must not be collateral damage.

    The grep for the removed field also matches this one — it is the manager's own
    resolved path — so without this test a future reader could "clean up" the
    wrong one. The legacy vault layer is Fase C's to remove, not this change's.
    """
    import importlib

    mod = importlib.import_module(module)
    assert "vault_path" in inspect_source(mod)


def inspect_source(mod) -> str:
    import inspect

    return inspect.getsource(mod)


def test_env_file_cannot_reintroduce_vault_path(tmp_path: Path):
    """`.env` is a second route to the same field, and it is checked too.

    `SettingsConfigDict(env_file=".env")` is how a real deployment would have set
    the variable, so a test that only covers `monkeypatch.setenv` would miss the
    case that matters most. Written to an isolated directory so the repository's
    own `.env` cannot influence it.
    """
    import subprocess
    import sys

    env = dict(os.environ)
    env["BRAIN_DIR"] = str(tmp_path)
    env.pop("BRAIN_VAULT_PATH", None)
    (tmp_path / ".env").write_text("BRAIN_VAULT_PATH=/somewhere\n")
    code = (
        "import warnings, sys\n"
        "with warnings.catch_warnings(record=True) as c:\n"
        "    warnings.simplefilter('always')\n"
        "    from brain_server.config import Settings\n"
        "    s = Settings()\n"
        "d = [w for w in c if issubclass(w.category, DeprecationWarning)]\n"
        # `len(d) >= 1`, not `== 1`: importing brain_server.config instantiates\n"
        "# Settings once at module level (`settings = Settings()`), so the probe's\n"
        "# own construction is the second warning. What matters is that it warned\n"
        "# at all and that nothing is left to read.\n"
        "sys.exit(0 if (not hasattr(s, 'vault_path') and len(d) >= 1) else 1)\n"
    )
    r = subprocess.run([sys.executable, "-c", code], env=env, capture_output=True, text=True, cwd=tmp_path)
    assert r.returncode == 0, (
        "a .env carrying BRAIN_VAULT_PATH must warn and be ignored, not fatal.\n"
        f"stdout={r.stdout}\nstderr={r.stderr}"
    )
