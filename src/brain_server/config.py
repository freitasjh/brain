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


class Settings(BaseSettings):
    model_config = SettingsConfigDict(
        env_prefix="BRAIN_",
        env_file=".env",
        env_file_encoding="utf-8",
    )

    # Path to the Obsidian vault directory
    vault_path: Path | None = None

    # Ollama endpoint
    ollama_url: str = "http://localhost:11434"
    ollama_model: str = "nomic-embed-text"

    # Index persistence
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
        # Set default paths based on installation type
        base_dir = _get_default_base_dir()
        if self.vault_path is None:
            self.vault_path = base_dir / "vault"
        if self.index_path is None:
            self.index_path = base_dir / "data" / "index.db"


settings = Settings()
