# Architecture Rules — brain Rust SQLite-only

## Stack
- **Server**: Rust 1.85 edition 2024 + tokio full + axum 0.7 + rmcp 0.3 + serde
- **Storage**: SQLite-only `BRAIN_DB_PATH` (`./data/brain.db` WAL `journal_mode=WAL` `synchronous=NORMAL` `foreign_keys=ON`) FTS5 `porter unicode61` `notes_fts(title,body)`, vec BLOB `embedding` dim 768 fixo, RRF k=60
- **Embed**: Ollama `nomic-embed-text` dim 768 via `brain-embed` rustls, `chunk_text` by `## `
- **Transport**: SSE `BRAIN_PORT 8321` MCP + `BRAIN_VIEWER_PORT 8322` web `brain-web::router`
- **Vault**: REMOVED — export only `/tmp/brain-export` via `brain export --force`

## Crate Boundaries
- `crates/brain-core` — types `Note/Project/SearchResult`, validate_layer/scope/sanitize, chunk, wikilink `[[ ]]`, frontmatter `project/tags`, no IO
- `crates/brain-store` — `Store {conn}` WAL, `init_schema` SCHEMA_VERSION 4, tables `projects/notes/chunks/notes_fts/links/entities/entity_links/audit_log/_meta`, triggers FTS, `search` hybrid FTS+vector+entity+graph RRF + authority
- `crates/brain-embed` — `EmbeddingEngine` health_check + embed + batch concurrent 4
- `crates/brain-mcp` — rmcp tool registry, State `AppState{db}` (open per request to avoid !Sync)
- `crates/brain-web` — axum read-only `/api/status,search,read,list`
- `crates/brain-cli` — clap `brain ping/store/read/search/delete/recent/status/reindex/checkpoints/restore/backup/export/forget-sweep/migrate/serve/project`
- `src/brain_server/` — legado Python compat Fase C, `BRAIN_VAULT_PATH` warn deprecated

## Key Decisions
- Agents are NOT persisted separately — projects are `projects` table, notes are SQLite `notes.path=layer/scope/path`
- Tools are rmcp `#[tool]` + axum handlers, sanitize via `brain-core::sanitize_relative_path`
- Scope mandatory for `arquitetura/regras/estudos` (`projetos|global`), validated before DB
- No auth — add later Fase C; single-tenant WAL
- Schema migrations via `SCHEMA_VERSION` bump 4→5 + `init_schema` triggers, not Flyway; TTL `expires_at` beats `pinned`
