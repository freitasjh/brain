# Phase 2 — Design: brain-rust-sqlite Fase A+B

## Overview

Rewrite Python MCP server to Rust, SQLite-only truth, hybrid FTS5+vector RRF retrieval. Crates minimal, WAL single-writer actor, axum SSE. Legacy import vault+index.db. Export temp.

## Architecture

```
[CLI clap] --HTTP(SSE)--> [axum server:8321] --mpsc--> [store actor rusqlite+sqlite-vec+FTS5]
                               |                            |
                           [embed Ollama] <--- chunks ## ----
                               |
                           [brain.db WAL]
```

Components:
- brain-core: tipos `Note, Chunk, Project, SearchResult`, validate_layer/scope/sanitize, parse_frontmatter
- brain-store: `StoreActor { conn: Connection }` single writer via `tokio::mpsc::unbounded_channel`, reader pool clone
- brain-embed: `EmbeddingEngine { base_url, model=nomic-embed-text, dim=768 }` reqwest, `chunk_text(&str)->Vec<String>`
- brain-mcp: `FastMCP` via `rmcp` (ou axum SSE manual se rmcp não integra), tools registro
- brain-cli: clap `server/start/stop/status, store, read, search, delete, recent, backup, export, reindex, checkpoints, restore, project/*`
- brain-web: axum 8322 `/api/search,read,list,status` read-only sobre `brain.db`

## Data Models

```rust
struct Note { id: i64, path: String, layer: String, scope: Option<String>, content: String, project_id: Option<i64>, tags: Vec<String>, pinned: bool, expires_at: Option<DateTime<Utc>>, version: i32 }
struct Chunk { id: i64, note_id: i64, path: String, layer: String, scope: Option<String>, snippet: String, chunk_index: i32, total_chunks: i32 }
struct Project { id: i64, name: String, description: String, created_at: String }
struct SearchResult { path: String, layer: String, scope: Option<String>, score: f32, snippet: String, chunk_index: i32, project: Option<String>, tags: Vec<String> }
```

Schema v4 SQL (ver 01-requirements.md). Indexes: `idx_notes_layer, idx_chunks_path, idx_notes_expires`. FTS5: `CREATE VIRTUAL TABLE notes_fts USING fts5(title, body, content='notes', content_rowid='id')` + triggers.

## Decisions

### D1: SQLite-only vs git+SQLite (ai-memory)
Context: ai-memory markdown+git truth, brain atual vault+SQLite. User quer SQLite-only.
Options: 1) manter vault, 2) remover vault.
Decision: 2 remover. Rationale: single file deploy, simplicidade Rust, export supre debug. Con: perde Obsidian live edit.

### D2: dim fixa 768
Context: configurável vs fixo.
Decision: fixo. Rationale: nomic-embed-text padrão, simplifica vec0 `FLOAT[768]`, evita mismatch.

### D3: RRF k=60
Context: pesos diferentes streams.
Decision: k=60 padrão literatura RRF, authority +0.15/+0.1 após fusão antes truncar.

### D4: rmcp vs axum manual
Context: MCP SDK Rust.
Decision: tentar `rmcp` 0.3+axum; fallback axum SSE manual se incompat. Protótipo A1 valida.

### D5: Export temp
Context: disco vs memória.
Decision: `/tmp/brain-export` via `tempfile::TempDir` se --to omitido, senão dir informado.

## Interfaces

MCP tools (nomes iguais Python para compat):
- `brain_store(layer, path, content, scope?, project?, tags?, pinned?, expires_at?) -> "ok — layer/scope/path.md"`
- `brain_read(layer, path, scope?) -> "# path\n\ncontent" | NOT_FOUND`
- `brain_search(query, layer?, scope?, project?, tag?, top_k=5, explain=false) -> json {results:[{path,layer,scope,score,snippet,chunk_index,project,tags}], total}`
- `brain_delete_page(path) -> ok | NOT_FOUND`
- `brain_recent(top_k=10) -> json`
- `brain_status -> json {notes,chunks,projects,db_path,ollama_ok,version}`
- `brain_reindex(all|layer|path) -> REINDEX_STARTED | REINDEX_IN_PROGRESS` (background tokio task)
- `brain_backup, brain_checkpoints, brain_restore_page, brain_project_*` mesma assinatura Python

CLI parity `src/brain_server/cli/main.py:125` + novos `delete, recent, export, backup`.

## Error Handling

- `INVALID_PARAMS` para layer/scope/path traversal (`sanitize_relative_path` port `vault/models.py:64`)
- `NOT_FOUND` para read/delete miss
- `EMBEDDING_FAILED` se Ollama down, search fallback FTS5, store persiste vec vazio log warn
- DB corrupt → `anyhow` + tracing error + exit 1

## Testing Strategy

- Unit: store actor (in-mem `:memory:`), embed chunk, sanitize, FTS RRF
- Integration: `cargo test --test mcp_integration` start server `:0` random port, `reqwest` client `brain_store→search→read→delete`
- E2E: import legado `vault/` fixture → assert notes count
- Coverage `cargo llvm-cov --workspace`

## Migration Plan

1. Detect `BRAIN_VAULT_PATH` env → warn ignore
2. If `BRAIN_DB_PATH` missing and `data/index.db` exists → `ATTACH DATABASE` copy
3. If `vault/` dir has `*.md` → walk `list_all_files` port `vault/manager.py:131`, parse frontmatter `parse_frontmatter` port, upsert
4. Write `vault.bak.tar.gz` via `tar+flate2`

## Open Risks

- sqlite-vec crate compat bundled vs `sqlite-vec` extension load (`store.py:79` `enable_load_extension`). Test A2 early.
- `nomic-embed-text` dim 768 confirmado via `curl /api/embeddings` check health.
