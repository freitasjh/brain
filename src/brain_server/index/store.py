"""VectorIndex — SQLite vector store backed by sqlite-vec.

Schema v3 adds:
  - projects: registry of projects (id, name, description)
  - project_id on chunks: which project OWNS this note
  - tags on chunks: JSON array for categorization (from frontmatter)
  - note_projects: many-to-many linking global notes to projects that use them
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

from brain_server.index.models import IndexEntry, NoteLink, Project, SearchResult

logger = logging.getLogger(__name__)


def vec_to_blob(vector: list[float]) -> bytes:
    """Convert a list of floats to a float32 BLOB for sqlite-vec."""
    return array.array("f", vector).tobytes()


def _tags_to_json(tags: list[str] | None) -> str | None:
    """Serialize tags list to JSON string."""
    return json.dumps(tags) if tags else None


def _json_to_tags(raw: str | None) -> list[str]:
    """Deserialize JSON string to tags list."""
    if not raw:
        return []
    try:
        result = json.loads(raw)
        return result if isinstance(result, list) else []
    except (json.JSONDecodeError, TypeError):
        return []


class VectorIndex:
    """SQLite-based vector index using sqlite-vec for fast similarity search.

    Schema v3:
      - chunks: metadata (path, layer, scope, snippet, project_id, tags)
      - vec_chunks: virtual vec0 table for vector search
      - projects: project registry
      - note_projects: many-to-many (global notes ↔ projects)
    """

    VERSION = 3
    EMBEDDING_DIM = 768
    _conn: sqlite3.Connection | None = None

    def __init__(self, index_path: Path, embed_dim: int | None = None) -> None:
        self.index_path = index_path
        self.embed_dim = embed_dim or self.EMBEDDING_DIM
        self._reindex_lock = asyncio.Lock()

    # ------------------------------------------------------------------
    # Connection / Schema
    # ------------------------------------------------------------------

    def _connect(self) -> sqlite3.Connection:
        if self._conn is not None:
            return self._conn

        self.index_path.parent.mkdir(parents=True, exist_ok=True)
        self._conn = sqlite3.connect(str(self.index_path))

        self._conn.enable_load_extension(True)
        sqlite_vec.load(self._conn)
        self._conn.enable_load_extension(False)

        self._conn.execute("PRAGMA journal_mode=WAL")
        self._conn.execute("PRAGMA foreign_keys=ON")
        self._conn.execute("PRAGMA synchronous=NORMAL")

        self._init_schema()
        self._migrate_v2_to_v3()
        return self._conn

    def _init_schema(self) -> None:
        conn = self._conn
        assert conn is not None

        conn.executescript(f"""
            -- Projects registry
            CREATE TABLE IF NOT EXISTS projects (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                name        TEXT NOT NULL UNIQUE,
                description TEXT DEFAULT '',
                created_at  TEXT DEFAULT (datetime('now'))
            );

            -- Note chunks (without project_id/tags yet — added by migration if needed)
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

            -- Many-to-many: global notes ↔ projects that use them
            CREATE TABLE IF NOT EXISTS note_projects (
                note_path   TEXT NOT NULL,
                project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                created_at  TEXT DEFAULT (datetime('now')),
                PRIMARY KEY (note_path, project_id)
            );
            CREATE INDEX IF NOT EXISTS idx_note_projects_path ON note_projects(note_path);
            CREATE INDEX IF NOT EXISTS idx_note_projects_proj ON note_projects(project_id);

            -- Vector search (virtual table)
            CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(
                id        INTEGER PRIMARY KEY,
                embedding FLOAT[{self.embed_dim}]
            );

            -- Metadata
            CREATE TABLE IF NOT EXISTS _meta (
                key   TEXT PRIMARY KEY,
                value TEXT
            );

            INSERT OR IGNORE INTO _meta(key, value) VALUES ('version', '{self.VERSION}');
            INSERT OR IGNORE INTO _meta(key, value) VALUES ('embedding_dim', '{self.embed_dim}');
        """)
        conn.commit()

        # Run migration AFTER base schema is created
        self._migrate_v2_to_v3()

        # Add indexes for new columns (safe to run even if they exist)
        try:
            conn.execute("CREATE INDEX IF NOT EXISTS idx_chunks_project ON chunks(project_id)")
            conn.execute("CREATE INDEX IF NOT EXISTS idx_chunks_tags ON chunks(tags)")
        except Exception:
            pass  # Columns may not exist yet on old DBs

    def _migrate_v2_to_v3(self) -> None:
        """Add project_id, tags columns if upgrading from v2 schema."""
        conn = self._conn
        assert conn is not None

        cols = {row[1] for row in conn.execute("PRAGMA table_info(chunks)").fetchall()}

        if "project_id" not in cols:
            conn.execute("ALTER TABLE chunks ADD COLUMN project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL")
            logger.info("Migrated: added chunks.project_id")

        if "tags" not in cols:
            conn.execute("ALTER TABLE chunks ADD COLUMN tags TEXT DEFAULT NULL")
            logger.info("Migrated: added chunks.tags")

        conn.execute("UPDATE _meta SET value = ? WHERE key = 'version'", [str(self.VERSION)])
        conn.commit()

    @property
    def reindexing(self) -> bool:
        return self._reindex_lock.locked()

    # ------------------------------------------------------------------
    # Migration from JSON (v1)
    # ------------------------------------------------------------------

    @classmethod
    def migrate_from_json(cls, json_path: Path, db_path: Path) -> VectorIndex | None:
        if not json_path.exists():
            return None

        try:
            raw = json.loads(json_path.read_text(encoding="utf-8"))
            entries_data = raw.get("entries", [])
            if not entries_data:
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
                conn.execute(
                    "INSERT INTO vec_chunks(id, embedding) VALUES (?, ?)",
                    [cursor.lastrowid, embed_blob],
                )

            conn.commit()
            idx.save()
            backup = json_path.with_suffix(".json.bak")
            json_path.rename(backup)
            logger.info("Migration complete — backed up to %s", backup)
            return idx

        except Exception as exc:
            logger.warning("Migration from JSON failed: %s", exc)
            return None

    # ------------------------------------------------------------------
    # Persistence
    # ------------------------------------------------------------------

    def load(self) -> None:
        if not self.index_path.exists():
            json_path = self.index_path.with_suffix(".json")
            if json_path.exists():
                migrated = self.migrate_from_json(json_path, self.index_path)
                if migrated is not None:
                    self._conn = migrated._conn
                    return
        self._connect()

    def save(self) -> None:
        conn = self._connect()
        conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")

    def size(self) -> int:
        conn = self._connect()
        return conn.execute("SELECT count(*) FROM chunks").fetchone()[0]

    # ------------------------------------------------------------------
    # Projects CRUD
    # ------------------------------------------------------------------

    def project_create(self, name: str, description: str = "") -> Project:
        """Register a new project. Raises ValueError if name already exists."""
        conn = self._connect()
        try:
            cursor = conn.execute(
                "INSERT INTO projects(name, description) VALUES (?, ?)",
                [name.strip(), description],
            )
            conn.commit()
            logger.info("Created project '%s' (id=%d)", name, cursor.lastrowid)
            return Project(id=cursor.lastrowid, name=name.strip(), description=description)
        except sqlite3.IntegrityError:
            raise ValueError(f"Project '{name}' already exists")

    def project_get(self, name: str) -> Project | None:
        """Get a project by name."""
        conn = self._connect()
        row = conn.execute(
            "SELECT id, name, description, created_at FROM projects WHERE name = ?",
            [name.strip()],
        ).fetchone()
        if not row:
            return None
        return Project(id=row[0], name=row[1], description=row[2], created_at=row[3])

    def project_get_by_id(self, project_id: int) -> Project | None:
        conn = self._connect()
        row = conn.execute(
            "SELECT id, name, description, created_at FROM projects WHERE id = ?",
            [project_id],
        ).fetchone()
        if not row:
            return None
        return Project(id=row[0], name=row[1], description=row[2], created_at=row[3])

    def project_list(self) -> list[Project]:
        """List all registered projects."""
        conn = self._connect()
        rows = conn.execute(
            "SELECT id, name, description, created_at FROM projects ORDER BY name"
        ).fetchall()
        return [Project(id=r[0], name=r[1], description=r[2], created_at=r[3]) for r in rows]

    def project_delete(self, name: str) -> bool:
        """Delete a project by name. Returns True if deleted."""
        conn = self._connect()
        cursor = conn.execute("DELETE FROM projects WHERE name = ?", [name.strip()])
        conn.commit()
        deleted = cursor.rowcount > 0
        if deleted:
            logger.info("Deleted project '%s'", name)
        return deleted

    def project_notes(self, project_name: str) -> list[dict]:
        """Get all notes (chunks) belonging to a project, plus global notes linked to it.

        Returns a list of dicts with: path, layer, scope, snippet, tags, source ('owned' | 'linked').
        """
        conn = self._connect()
        project = self.project_get(project_name)
        if not project:
            return []

        # 1. Notes owned by this project (scope='projetos', project_id matching)
        owned = conn.execute("""
            SELECT DISTINCT path, layer, scope, snippet, tags
            FROM chunks WHERE project_id = ?
            ORDER BY layer, path
        """, [project.id]).fetchall()

        results = [
            {"path": r[0], "layer": r[1], "scope": r[2], "snippet": r[3],
             "tags": _json_to_tags(r[4]), "source": "owned"}
            for r in owned
        ]

        # 2. Global notes linked to this project
        linked = conn.execute("""
            SELECT DISTINCT c.path, c.layer, c.scope, c.snippet, c.tags
            FROM note_projects np
            JOIN chunks c ON c.path = np.note_path
            WHERE np.project_id = ?
            ORDER BY c.layer, c.path
        """, [project.id]).fetchall()

        results.extend([
            {"path": r[0], "layer": r[1], "scope": r[2], "snippet": r[3],
             "tags": _json_to_tags(r[4]), "source": "linked"}
            for r in linked
        ])

        return results

    # ------------------------------------------------------------------
    # Note ↔ Project linking
    # ------------------------------------------------------------------

    def note_link_project(self, note_path: str, project_name: str) -> None:
        """Link a global note to a project (many-to-many)."""
        conn = self._connect()
        project = self.project_get(project_name)
        if not project:
            raise ValueError(f"Project '{project_name}' not found")
        conn.execute(
            "INSERT OR IGNORE INTO note_projects(note_path, project_id) VALUES (?, ?)",
            [note_path, project.id],
        )
        conn.commit()

    def note_unlink_project(self, note_path: str, project_name: str) -> bool:
        """Remove link between a global note and a project."""
        conn = self._connect()
        project = self.project_get(project_name)
        if not project:
            return False
        cursor = conn.execute(
            "DELETE FROM note_projects WHERE note_path = ? AND project_id = ?",
            [note_path, project.id],
        )
        conn.commit()
        return cursor.rowcount > 0

    def note_linked_projects(self, note_path: str) -> list[Project]:
        """Get all projects linked to a specific note."""
        conn = self._connect()
        rows = conn.execute("""
            SELECT p.id, p.name, p.description, p.created_at
            FROM note_projects np
            JOIN projects p ON p.id = np.project_id
            WHERE np.note_path = ?
            ORDER BY p.name
        """, [note_path]).fetchall()
        return [Project(id=r[0], name=r[1], description=r[2], created_at=r[3]) for r in rows]

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
        project_id: int | None = None,
        tags: list[str] | None = None,
    ) -> None:
        """Add or update a chunk entry with embedding, project ownership, and tags."""
        conn = self._connect()
        embed_blob = vec_to_blob(embedding)

        existing = conn.execute(
            "SELECT id, project_id, tags FROM chunks WHERE path = ? AND chunk_index = ?",
            [path, chunk_index],
        ).fetchone()

        # Bug 2 fix: preserve existing project_id/tags when not provided
        if existing:
            chunk_id = existing[0]
            if project_id is None and existing[1] is not None:
                project_id = existing[1]
            if tags is None and existing[2] is not None:
                tags = _json_to_tags(existing[2])

        tags_json = _tags_to_json(tags)

        if existing:
            chunk_id = existing[0]
            conn.execute("DELETE FROM vec_chunks WHERE id = ?", [chunk_id])
            conn.execute(
                """UPDATE chunks
                   SET layer=?, scope=?, snippet=?, total_chunks=?,
                       project_id=?, tags=?
                   WHERE id=?""",
                [layer, scope, snippet[:200], total_chunks, project_id, tags_json, chunk_id],
            )
        else:
            cursor = conn.execute(
                """INSERT INTO chunks(path, layer, scope, snippet, chunk_index, total_chunks, project_id, tags)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
                [path, layer, scope, snippet[:200], chunk_index, total_chunks, project_id, tags_json],
            )
            chunk_id = cursor.lastrowid

        conn.execute(
            "INSERT INTO vec_chunks(id, embedding) VALUES (?, ?)",
            [chunk_id, embed_blob],
        )
        conn.commit()

    def remove(self, path: str) -> None:
        conn = self._connect()
        ids = [
            row[0]
            for row in conn.execute("SELECT id FROM chunks WHERE path = ?", [path]).fetchall()
        ]
        for chunk_id in ids:
            conn.execute("DELETE FROM vec_chunks WHERE id = ?", [chunk_id])
        conn.execute("DELETE FROM chunks WHERE path = ?", [path])
        conn.execute("DELETE FROM note_projects WHERE note_path = ?", [path])
        conn.commit()
        if ids:
            logger.info("Removed %d entries for '%s'", len(ids), path)

    def clear(self) -> None:
        conn = self._connect()
        conn.executescript("""
            DROP TABLE IF EXISTS vec_chunks;
            DROP TABLE IF EXISTS chunks;
            DROP TABLE IF EXISTS note_projects;
            DROP TABLE IF EXISTS projects;
            DROP TABLE IF EXISTS _meta;
        """)
        conn.commit()
        self._init_schema()

    # ------------------------------------------------------------------
    # Search
    # ------------------------------------------------------------------

    def search(
        self,
        query_embedding: list[float],
        top_k: int = 5,
        layer_filter: str | None = None,
        scope_filter: str | None = None,
        project_filter: str | None = None,
        tag_filter: str | None = None,
    ) -> list[SearchResult]:
        """Search by cosine similarity with optional filters for layer, scope, project, and tags.

        Args:
            query_embedding: The query vector
            top_k: Max results
            layer_filter: Only return notes from this layer
            scope_filter: Only return notes with this scope
            project_filter: Only return notes belonging to this project name
                            (includes both owned and linked global notes)
            tag_filter: Only return notes whose tags contain this string
        """
        conn = self._connect()
        query_blob = vec_to_blob(query_embedding)

        search_k = min(top_k * 4, 200)

        # Build WHERE clauses
        where_clauses = ["1=1"]
        params: list[Any] = [query_blob, search_k]

        if layer_filter:
            where_clauses.append("c.layer = ?")
            params.append(layer_filter)
        if scope_filter:
            where_clauses.append("c.scope = ?")
            params.append(scope_filter)
        if project_filter:
            where_clauses.append("""
                (c.project_id = (SELECT id FROM projects WHERE name = ?)
                 OR c.path IN (
                     SELECT np.note_path FROM note_projects np
                     JOIN projects p ON p.id = np.project_id
                     WHERE p.name = ?
                 ))
            """)
            params.extend([project_filter, project_filter])
        if tag_filter:
            where_clauses.append("c.tags LIKE ?")
            params.append(f"%{tag_filter}%")

        where_sql = " AND ".join(where_clauses)

        sql = f"""
            SELECT c.path, c.layer, c.scope, c.snippet, c.chunk_index,
                   v.distance, c.project_id, c.tags,
                   p.name as project_name
            FROM (SELECT id, distance FROM vec_chunks WHERE embedding MATCH ? AND k = ?) v
            JOIN chunks c ON c.id = v.id
            LEFT JOIN projects p ON p.id = c.project_id
            WHERE {where_sql}
            ORDER BY v.distance ASC
            LIMIT ?
        """
        params.append(top_k)

        rows = conn.execute(sql, params).fetchall()

        return [
            SearchResult(
                path=row[0],
                layer=row[1],
                score=round(1.0 - row[5], 4),
                snippet=row[3] or "(snippet not available)",
                chunk_index=row[4],
                scope=row[2],
                project_id=row[6],
                project_name=row[8],
                tags=_json_to_tags(row[7]),
            )
            for row in rows
        ]


def _cosine_similarity(a: list[float], b: list[float]) -> float:
    """Standalone cosine similarity for testing."""
    if len(a) != len(b):
        raise ValueError(f"Dimension mismatch: {len(a)} vs {len(b)}")
    import math
    dot = sum(x * y for x, y in zip(a, b))
    norm_a = math.sqrt(sum(x * x for x in a))
    norm_b = math.sqrt(sum(y * y for y in b))
    if norm_a == 0 or norm_b == 0:
        return 0.0
    return dot / (norm_a * norm_b)
