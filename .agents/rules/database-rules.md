# Database Rules — SQLite WAL

## Schema
- `notes(id PK, path UNIQUE, layer, scope, content, project_id FK, tags JSON, pinned BOOL, expires_at TEXT, version INT, created_at, updated_at)` `idx_notes_layer/scope/expires/project`
- `chunks(id PK, note_id FK CASCADE, path, layer, scope, snippet, chunk_index, total_chunks, project_id, tags JSON, embedding BLOB)` `idx_chunks_path/layer`
- `notes_fts(title, body)` FTS5 `porter unicode61` + triggers `notes_fts_insert/delete/update` (plain table, DELETE WHERE rowid)
- `projects(id PK, name UNIQUE, description, created_at)`
- `links(from_path, to_path)` wikilink `[[ ]]`
- `entities(name UNIQUE, normalized)` + `entity_links(entity_id, note_id)`
- `audit_log(id, action, path, prev_content, at)` checkpoints
- `_meta(key PK, value)` `version=4, embedding_dim=768`

## WAL
- `PRAGMA journal_mode=WAL; foreign_keys=ON; synchronous=NORMAL`
- Single `Store {conn}` owns DB; `init_schema` create if not exists, `DROP TRIGGER IF EXISTS` before create to handle legacy

## Migrations
- No Flyway files — `SCHEMA_VERSION` 4 bump 4→5 via `init_schema` + `ALTER TABLE` if needed (see `brain-store/src/lib.rs:72`)
- Never edit applied live DB; bump version and add migration branch `if cols missing then ALTER`

## No MySQL agents/tools tables — archived
