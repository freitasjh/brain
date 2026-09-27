"""Configuration via environment variables with pydantic-settings."""

from __future__ import annotations

import os
from pathlib import Path

from pydantic_settings import BaseSettings, SettingsConfigDict


def _get_default_base_dir() -> Path:
    """Get default base directory for vault and data.
    
    - If BRAIN_DIR env var is set, use it
    - If running from source (pyproject.toml exists), use current directory
    - Otherwise (installed globally), use ~/.brain
    """
    # Check environment variable
    env_dir = os.getenv("BRAIN_DIR")
    if env_dir:
        return Path(env_dir)
    
    # Check if running from source
    try:
        import brain_server
        package_dir = Path(brain_server.__file__).parent
        source_dir = package_dir.parent.parent.parent
        if (source_dir / "pyproject.toml").exists():
            return Path(".")  # Use current directory when running from source
    except Exception:
        pass
    
    # Installed globally - use ~/.brain
    return Path.home() / ".brain"


def _setting_is_present(env_file: str | None, key: str) -> bool:
    """Whether `key` is set in the environment **or** in the configured `.env`.

    The deprecation warning used to be `os.getenv("BRAIN_VAULT_PATH")`, which
    only sees the process environment. That misses the route the spec's B6 is
    actually about: `SettingsConfigDict(env_file=".env")` is how a real
    deployment sets these, so an operator whose `.env` still carries
    `BRAIN_VAULT_PATH=` got **no warning at all** — the setting vanished
    silently, which is the one outcome a deprecation is not allowed to produce.

    Both sources are consulted, and the `.env` is read with the same
    `python-dotenv` parser `pydantic-settings` itself uses, so this agrees with
    what the library parsed rather than re-implementing the lookup. A missing
    file is not an error: it is the normal case, and `None` simply means "absent".
    """
    if os.getenv(key):
        return True
    if not env_file:
        return False
    try:
        from dotenv import dotenv_values

        return bool(dotenv_values(env_file).get(key))
    except Exception:
        # A malformed or unreadable `.env` is the settings layer's problem to
        # report, not this warning's. Raising here would turn a deprecation notice
        # into a startup failure, which is the exact failure mode this change
        # exists to remove.
        return False


class Settings(BaseSettings):
    model_config = SettingsConfigDict(
        env_prefix="BRAIN_",
        env_file=".env",
        env_file_encoding="utf-8",
        # B6b. Required, and not a style choice.
        #
        # `pydantic-settings` treats an unknown `BRAIN_`-prefixed key from the
        # **environment** as ignorable, but the same key from **`.env`** as
        # `extra_forbidden` — it validates every source it finds, and a field it
        # does not recognise is a validation error. So deleting the
        # `vault_path` field without this line did not produce "deprecated and
        # ignored"; it produced an `extra_forbidden` ValidationError at *import
        # time* of this module, for every operator whose `.env` still carried
        # `BRAIN_VAULT_PATH=` — and that file is exactly what the deprecation
        # exists to help them clean up.
        #
        # `ignore` is what makes the warning below reachable: the variable has to
        # survive parsing for there to be anything to warn about. The tests in
        # `test_brain_config_vault_removed.py` pin both halves — the `.env` route
        # in a subprocess, because it does not go through `monkeypatch`.
        extra="ignore",
    )

    # Ollama endpoint
    ollama_url: str = "http://localhost:11434"
    ollama_model: str = "nomic-embed-text"

    # SQLite-only DB path (replaces vault_path/index_path)
    db_path: Path | None = None
    # Legacy
    index_path: Path | None = None

    # MCP server
    port: int = 8321
    transport: str = "sse"  # "stdio" | "sse"

    # Embedding
    chunk_max_tokens: int = 4096

    # Logging
    log_level: str = "INFO"

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        base_dir = _get_default_base_dir()
        # B6b. The variable is no longer a *field* — `vault_path` is gone, so
        # setting it does not configure anything and `Settings` has no attribute
        # to read. What survives is this warning, because it is the only thing
        # that tells somebody who still exports the variable that the setting
        # they think they are making is not being made. pydantic-settings ignores
        # unknown `BRAIN_`-prefixed variables rather than raising, so a leftover
        # `BRAIN_VAULT_PATH` degrades to "ignored, with a warning" instead of
        # turning the legacy server into a hard crash on startup.
        import warnings

        if _setting_is_present(self.model_config.get("env_file"), "BRAIN_VAULT_PATH"):
            warnings.warn(
                "BRAIN_VAULT_PATH is no longer read — the Rust store is SQLite-only "
                "and the vault directory is not configurable. Use BRAIN_DB_PATH. "
                "The variable is ignored; the legacy server resolves its vault from BRAIN_DIR.",
                DeprecationWarning,
            )
        if self.db_path is None:
            # prefer BRAIN_DB_PATH, fallback to index_path legacy
            if self.index_path is not None and os.getenv("BRAIN_INDEX_PATH"):
                self.db_path = self.index_path
            else:
                self.db_path = base_dir / "data" / "brain.db"
        if self.index_path is None:
            self.index_path = self.db_path


def legacy_vault_path() -> Path:
    """The vault directory of the **legacy Python** server.

    B6b moved this out of `Settings` and into a function, on purpose. The field
    used to be a live setting: `BRAIN_VAULT_PATH=<dir>` changed where the legacy
    server read and wrote, and `__init__` filled in `<base>/vault` when it was
    unset. That is the behaviour the spec asked to remove — a vault path is a
    configuration surface that the SQLite-only store does not have, and keeping
    it alive meant the deprecated variable still *worked*, so nothing ever
    migrated off it.

    A function instead of a field means there is nothing for an operator to set
    and nothing for `Settings` to advertise. The only caller is
    `brain_server.server`, which is itself removed in Fase C; the value is
    derived from `BRAIN_DIR` (or the source/global base directory), which is the
    default the field used to fill in, so an operator who was *not* overriding
    the variable sees no change at all.

    An operator who *was* overriding it loses that override silently, which is why
    the deprecation warning above now says the variable is ignored rather than
    merely deprecated.
    """
    return _get_default_base_dir() / "vault"


settings = Settings()
