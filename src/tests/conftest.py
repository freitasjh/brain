"""Shared fixtures for brain_server tests."""

from __future__ import annotations

from pathlib import Path
from typing import AsyncGenerator

import pytest
import pytest_asyncio

from brain_server.vault.manager import VaultManager
from brain_server.index.store import VectorIndex


@pytest.fixture
def vault_path(tmp_path: Path) -> Path:
    """Temporary vault directory."""
    p = tmp_path / "vault"
    p.mkdir()
    return p


@pytest.fixture
def vault_manager(vault_path: Path) -> VaultManager:
    """VaultManager pointing at a temporary directory."""
    return VaultManager(vault_path)


@pytest.fixture
def index_path(tmp_path: Path) -> Path:
    """Temporary index file path."""
    return tmp_path / "index.db"


@pytest.fixture
def vector_index(index_path: Path) -> VectorIndex:
    """Empty VectorIndex backed by a temp SQLite DB (4D for tests)."""
    idx = VectorIndex(index_path, embed_dim=4)
    idx.load()
    return idx


@pytest.fixture
def sample_markdown() -> str:
    """Multi-section markdown for testing."""
    return """# Project Overview

This is the top-level description.

## Architecture

The system uses a modular monolith pattern.

## Database

PostgreSQL with Flyway migrations.

## Rules

All queries must use named parameters.
"""
