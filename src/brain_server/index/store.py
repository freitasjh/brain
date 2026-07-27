"""VectorIndex — SQLite vector store backed by sqlite-vec for fast similarity search.

Replaces the previous JSON-based in-memory store with a proper SQLite database
using the sqlite-vec extension for vector similarity search (cosine distance).

Benefits over JSON:
  - O(log n) index lookups vs O(n) full scan
  - No need to load everything into RAM
  - SQL queries for complex filtering
  - Transactions for atomic writes
  - Scales to millions of vectors
  - Can be queried externally with any SQLite client
"""

from __future__ import annotations

import asyncio
import array
import json
import logging
from pathlib import Path
from typing import Any

import sqlite3
import sqlite_vec

from brain_server.index.models import IndexEntry, SearchResult

logger = logging.getLogger(__name__)


def vec_to_blob(vector: list[float]) -> bytes:
    """Convert a list of floats to a float32 BLOB for sqlite-vec."""
    return array.array("f", vector).tobytes()


def blob_to_vec(blob: bytes) -> list[float]:
    """Convert a float32 BLOB back to a list of floats."""
    return list(array.array("f", blob))


class VectorIndex:
    """SQLite-based vector index using sqlite-vec for fast similarity search.

    Uses two tables:
      - chunks: metadata (path, layer, scope, snippet, timestamps)
      - vec_chunks: virtual vec0 table (id, embedding) for vector search
    """

    VERSION = 2
    EMBEDDING_DIM = 768  # nomic-embed-text produces 768D vectors
    _conn: sqlite3.Connection | None = None

    def __init__(self, index_path: Path, embed_dim: int | None = None) -> None:
        self.index_path = index_path
        self.embed_dim = embed_dim or self.EMBEDDING_DIM
        self._reindex_lock = asyncio.Lock()

    # ------------------------------------------------------------------
    # Connection / Schema
    # ------------------------------------------------------------------

    def _connect(self) -> sqlite3.Connection:
        """Lazy-connect to SQLite database and initialize schema."""
        if self._conn is not None:
            return self._conn

        self.index_path.parent.mkdir(parents=True, exist_ok=True)
        self._conn = sqlite3.connect(str(self.index_path))

        # Load sqlite-vec extension
        self._conn.enable_load_extension(True)
        sqlite_vec.load(self._conn)
        self._conn.enable_load_extension(False)

        # Performance pragmas
        self._conn.execute("PRAGMA journal_mode=WAL")
        self._conn.execute("PRAGMA synchronous=NORMAL")

        self._init_schema()
        return self._conn

    def _init_schema(self) -> None:
        """Create tables and indexes if they don't exist."""
        conn = self._conn
        assert conn is not None

        conn.executescript(f"""
            CREATE TABLE IF NOT EXISTS chunks (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                path        TEXT NOT NULL,
                layer       TEXT NOT NULL,
                scope       TEXT,
                snippet     TEXT NOT NULL DEFAULT '',
                chunk_index INTEGER NOT NULL DEFAULT 0,
                total_chunks INTEGER NOT NULL DEFAULT 1,
                created_at  TEXT DEFAULT (datetime('now')),
                UNIQUE(path, chunk_index)
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_layer ON chunks(layer);
            CREATE INDEX IF NOT EXISTS idx_chunks_scope ON chunks(scope);
            CREATE INDEX IF NOT EXISTS idx_chunks_path ON chunks(path);

            CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
                id        INTEGER PRIMARY KEY,
                embedding FLOAT[{self.embed_dim}]
            );

            CREATE TABLE IF NOT EXISTS _meta (
                key   TEXT PRIMARY KEY,
                value TEXT
            );

            INSERT OR IGNORE INTO _meta(key, value) VALUES ('version', '{self.VERSION}');
            INSERT OR IGNORE INTO _meta(key, value) VALUES ('embedding_dim', '{self.embed_dim}');
        """)
        conn.commit()

    @property
    def reindexing(self) -> bool:
        """Check if a reindex operation is in progress."""
        return self._reindex_lock.locked()

    # ------------------------------------------------------------------
    # Migration from JSON (v1)
    # ------------------------------------------------------------------

    @classmethod
    def migrate_from_json(cls, json_path: Path, db_path: Path) -> VectorIndex | None:
        """Migrate an existing JSON index to SQLite.

        Reads the old JSON file, creates a new SQLite database, and imports
        all entries. Returns the new VectorIndex, or None if there's nothing to migrate.
        """
        if not json_path.exists():
            return None

        try:
            raw = json.loads(json_path.read_text(encoding="utf-8"))
            entries_data = raw.get("entries", [])
            if not entries_data:
                logger.info("JSON index is empty — nothing to migrate")
                # Remove empty JSON so we don't try again
                json_path.unlink(missing_ok=True)
                return None

            logger.info("Migrating %d entries from %s → %s", len(entries_data), json_path, db_path)
            idx = cls(db_path)
            conn = idx._connect()

            for e in entries_data:
                embedding = e.get("embedding", [])
                if not embedding:
                    continue

                embed_blob = vec_to_blob(embedding)
                cursor = conn.execute(
                    """INSERT INTO chunks(path, layer, scope, snippet, chunk_index, total_chunks)
                       VALUES (?, ?, ?, ?, ?, ?)""",
                    [e["path"], e["layer"], e.get("scope"),
                     e.get("snippet", "")[:200], e.get("chunk_index", 0), e.get("total_chunks", 1)],
                )
                chunk_id = cursor.lastrowid
                conn.execute(
                    "INSERT INTO vec_chunks(id, embedding) VALUES (?, ?)",
                    [chunk_id, embed_blob],
                )

            conn.commit()
            idx.save()

            # Back up old JSON
            backup = json_path.with_suffix(".json.bak")
            json_path.rename(backup)
            logger.info("Migration complete — old index backed up to %s", backup)
            return idx

        except Exception as exc:
            logger.warning("Migration from JSON failed: %s — starting fresh", exc)
            return None

    # ------------------------------------------------------------------
    # Persistence
    # ------------------------------------------------------------------

    def load(self) -> None:
        """Connect to database and ensure schema.

        If the database file doesn't exist, it will be created on first write.
        Also checks for a legacy JSON index and migrates it automatically.
        """
        if not self.index_path.exists():
            # Check for JSON migration
            json_path = self.index_path.with_suffix(".json")
            if json_path.exists():
                migrated = self.migrate_from_json(json_path, self.index_path)
                if migrated is not None:
                    # Copy the connection from migrated instance
                    self._conn = migrated._conn
                    logger.info("Loaded migrated SQLite index with %d entries", self.size())
                    return

        self._connect()
        logger.info("Loaded SQLite index from %s — %d entries", self.index_path, self.size())

    def save(self) -> None:
        """Ensure data is flushed to disk via WAL checkpoint."""
        conn = self._connect()
        conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        logger.debug("Saved index with %d entries", self.size())

    def size(self) -> int:
        """Number of chunk entries in the index."""
        conn = self._connect()
        return conn.execute("SELECT count(*) FROM chunks").fetchone()[0]

    # ------------------------------------------------------------------
    # Mutations
    # ------------------------------------------------------------------

    def upsert(
        self,
        path: str,
        layer: str,
        embedding: list[float],
        snippet: str = "",
        chunk_index: int = 0,
        total_chunks: int = 1,
        scope: str | None = None,
    ) -> None:
        """Add or update an entry. If path+chunk_index exists, replace it.

        sqlite-vec's vec0 virtual table does NOT support INSERT OR REPLACE,
        so we DELETE first, then INSERT.
        """
        conn = self._connect()
        embed_blob = vec_to_blob(embedding)

        # Check if chunk already exists
        existing = conn.execute(
            "SELECT id FROM chunks WHERE path = ? AND chunk_index = ?",
            [path, chunk_index],
        ).fetchone()

        if existing:
            chunk_id = existing[0]
            # Delete from vec0 first (foreign key-like constraint)
            conn.execute("DELETE FROM vec_chunks WHERE id = ?", [chunk_id])
            conn.execute(
                """UPDATE chunks
                   SET layer=?, scope=?, snippet=?, total_chunks=?
                   WHERE id=?""",
                [layer, scope, snippet[:200], total_chunks, chunk_id],
            )
        else:
            cursor = conn.execute(
                """INSERT INTO chunks(path, layer, scope, snippet, chunk_index, total_chunks)
                   VALUES (?, ?, ?, ?, ?, ?)""",
                [path, layer, scope, snippet[:200], chunk_index, total_chunks],
            )
            chunk_id = cursor.lastrowid

        # Insert into vec0
        conn.execute(
            "INSERT INTO vec_chunks(id, embedding) VALUES (?, ?)",
            [chunk_id, embed_blob],
        )
        conn.commit()

    def remove(self, path: str) -> None:
        """Remove all entries for a given path."""
        conn = self._connect()
        ids = [
            row[0]
            for row in conn.execute(
                "SELECT id FROM chunks WHERE path = ?", [path]
            ).fetchall()
        ]
        for chunk_id in ids:
            conn.execute("DELETE FROM vec_chunks WHERE id = ?", [chunk_id])
        conn.execute("DELETE FROM chunks WHERE path = ?", [path])
        removed = len(ids)
        conn.commit()
        if removed:
            logger.info("Removed %d entries for '%s'", removed, path)

    def clear(self) -> None:
        """Remove all entries by dropping and recreating tables."""
        conn = self._connect()
        conn.executescript("""
            DROP TABLE IF EXISTS vec_chunks;
            DROP TABLE IF EXISTS chunks;
            DROP TABLE IF EXISTS _meta;
        """)
        conn.commit()
        self._init_schema()
        logger.info("Index cleared")

    # ------------------------------------------------------------------
    # Search
    # ------------------------------------------------------------------

    def search(
        self,
        query_embedding: list[float],
        top_k: int = 5,
        layer_filter: str | None = None,
        scope_filter: str | None = None,
    ) -> list[SearchResult]:
        """Search by cosine similarity. Returns top_k results sorted by similarity (desc).

        Uses vec0's efficient kNN search, then joins with metadata table
        for filtering and fetching snippet/content.
        """
        conn = self._connect()
        query_blob = vec_to_blob(query_embedding)

        # Query vec0 for nearest neighbors (request more to compensate for filters)
        search_k = min(top_k * 4, 200)  # cap at 200 to avoid excessive results
        base_rows = conn.execute(
            "SELECT id, distance FROM vec_chunks WHERE embedding MATCH ? AND k = ?",
            [query_blob, search_k],
        ).fetchall()

        if not base_rows:
            return []

        # Build the join query with optional filters
        sql = """
            SELECT c.path, c.layer, c.scope, c.snippet, c.chunk_index, v.distance
            FROM (SELECT id, distance FROM vec_chunks WHERE embedding MATCH ? AND k = ?) v
            JOIN chunks c ON c.id = v.id
            WHERE 1=1
        """
        params: list[Any] = [query_blob, search_k]

        if layer_filter:
            sql += " AND c.layer = ?"
            params.append(layer_filter)
        if scope_filter:
            sql += " AND c.scope = ?"
            params.append(scope_filter)

        sql += " ORDER BY v.distance ASC LIMIT ?"
        params.append(top_k)

        rows = conn.execute(sql, params).fetchall()

        return [
            SearchResult(
                path=row[0],
                layer=row[1],
                score=round(1.0 - row[5], 4),  # cosine_distance → similarity
                snippet=row[3] or "(snippet not available)",
                chunk_index=row[4],
                scope=row[2],
            )
            for row in rows
        ]


def _cosine_similarity(a: list[float], b: list[float]) -> float:
    """Standalone cosine similarity for testing (used by tests)."""
    if len(a) != len(b):
        raise ValueError(f"Dimension mismatch: {len(a)} vs {len(b)}")
    import math
    dot = sum(x * y for x, y in zip(a, b))
    norm_a = math.sqrt(sum(x * x for x in a))
    norm_b = math.sqrt(sum(y * y for y in b))
    if norm_a == 0 or norm_b == 0:
        return 0.0
    return dot / (norm_a * norm_b)
