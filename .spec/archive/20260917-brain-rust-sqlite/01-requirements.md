# Phase 1 — Requirements: brain-rust-sqlite (Fase A+B)

## Visão Geral

Migrar brain de Python+Obsidian+sqlite-vec para Rust+SQLite-only. DB único `brain.db` como source truth. FTS5 lexical + vector RRF híbrido + entity + graph. Export temporário em disco apenas para debug. Dimensão embedding fixa 768. Manter hooks Python até Fase C. Sem auth/multi-machine nesta fase.

Baseado em análise ai-memory (akitaonrails, 5.3k★, 11 crates, git+SQLite, 18 MCP tools). Esta spec cobre Fase A (fundação) + Fase B (retrieval híbrido).

## Atores

| Ator | Descrição |
|------|-----------|
| Agente IA | consome MCP `brain_*` via SSE stdio |
| Operador | usa CLI `brain` Rust para store/search/ops |
| Sistema | Rust server axum+rmcp+rusqlite+sqlite-vec |

## Constraints

- C01 SQLite-only: nenhum `vault/*.md` como truth. `BRAIN_VAULT_PATH` removido, apenas `BRAIN_DB_PATH` (default `~/.brain/data/brain.db` global, `./data/brain.db` source)
- C02 Dim fixa 768 (`nomic-embed-text`), sem configurabilidade nesta fase
- C03 Rust 1.85+ edition 2024, tokio full, rmcp, axum, rusqlite bundled
- C04 Ollama único provider embedding nesta fase (sem OpenAI/Voyage)
- C05 Export temporário `/tmp/brain-export/<layer>/<scope>/<path>.md` com `rm -rf` manual
- C06 Hooks Python mantidos (`hooks/brain-hook.py`) até Fase C

## User Stories

### US-01 — Inicialização Rust
**Como** operador **Quero** rodar `brain server start` Rust **Para** ter MCP SSE em 8321

AC (EARS):
1. WHEN operator runs `brain server start` THEN system SHALL start axum SSE at `BRAIN_PORT` (8321)
2. WHEN server starts AND `brain.db` missing THEN system SHALL create DB with schema v4
3. WHEN server starts AND legacy `vault/` or `data/index.db` exists THEN system SHALL import legacy data to `brain.db` and backup to `vault.bak.tar.gz`
4. WHEN Ollama unreachable THEN system SHALL start degraded (log warn, search fallback FTS5 only)

### US-02 — Store SQLite-only
**Como** agente **Quero** `brain_store(layer,path,content,scope?,project?,tags?,pinned?,expires_at?)` **Para** persistir sem arquivo

AC:
1. WHEN `brain_store` called with valid layer THEN system SHALL insert/update `notes` row (path=`layer/scope/path`) and chunks + vec
2. IF layer in {arquitetura,regras,estudos} AND scope is null THEN system SHALL return `INVALID_PARAMS`
3. WHEN note exists THEN system SHALL overwrite (version++) and audit_log
4. WHEN `project` provided THEN system SHALL auto-create project if missing
5. WHEN `expires_at` provided (RFC3339 or YYYY-MM-DD) THEN system SHALL store and hide from normal search after TTL
6. WHEN `pinned=true` THEN system SHALL exempt from sweep
7. WHEN embedding fails THEN system SHALL persist note but queue embedding (log)

Valid layers: same 6 plus `estudos` scope handling identical.

### US-03 — Read
**Como** agente **Quero** `brain_read(layer,path,scope?)` **Para** ler conteúdo

AC:
1. WHEN file exists AND not expired (or include_expired handling future) THEN system SHALL return `# path\n\ncontent`
2. WHEN not found THEN system SHALL return `NOT_FOUND`
3. WHEN expired and normal read THEN system SHALL return `NOT_FOUND` (TTL hides)

### US-04 — Delete + Recent + Status
AC:
1. WHEN `brain_delete_page(path)` called AND exists THEN system SHALL delete note+chunks+vec+fts+links and audit
2. WHEN `brain_recent(top_k)` called THEN system SHALL return latest notes ordered `updated_at DESC`
3. WHEN `brain_status` called THEN system SHALL return counts (notes, chunks, projects), db path, ollama health, version

### US-05 — Busca híbrida RRF
**Como** agente **Quero** `brain_search(query,layer?,scope?,project?,tag?,top_k?,explain?)` **Para** recall lexical+semântico

AC:
1. WHEN `brain_search` called THEN system SHALL run vector `vec MATCH` (k=top_k*4) AND `notes_fts MATCH` AND entity RRF AND graph neighbor RRF then fuse via RRF k=60
2. WHEN query empty THEN system SHALL return `INVALID_PARAMS`
3. WHEN top_k invalid (1-20 clamp) THEN system SHALL default 5
4. WHEN filters provided THEN system SHALL apply WHERE layer/scope/project/tag/expires_at
5. WHEN `explain=true` THEN system SHALL include per-stream ranks and scores
6. WHEN Ollama down THEN system SHALL fallback FTS5-only with warning
7. WHEN no hits THEN system SHALL return `{"results":[],"total":0}`

### US-06 — Reindex + Backup + Export
AC:
1. WHEN `brain_reindex(all=true)` THEN system SHALL recompute all chunks vec and FTS
2. WHEN `brain_reindex reindexing` already THEN system SHALL return `REINDEX_IN_PROGRESS`
3. WHEN `brain_backup` THEN system SHALL run sqlite backup API to `brain.db.bak` or tar
4. WHEN `brain export --to /tmp/brain-export` THEN system SHALL dump all notes to temp dir preserving `layer/scope/path.md`

### US-07 — TTL Sweep
AC:
1. WHEN `brain_forget_sweep --dry-run` THEN system SHALL preview expired/pinned conflicts
2. WHEN sweep runs THEN system SHALL hard-delete where `expires_at < now()` (pin does not override per TTL beats pin rule)
3. WHEN pinned+expiring THEN system SHALL warn but TTL wins

### US-08 — Projects
Same as existing `brain_project_*` but backed by SQLite `projects` table, without vault file sync. Path `projetos/<name>` no longer written to disk except via export.

AC:
1. WHEN `brain_project_create` THEN system SHALL insert project
2. WHEN `brain_project_list/notes/link/unlink/delete` THEN system SHALL operate on `projects` + `notes.project_id`
3. WHEN search filter `project` THEN system SHALL include owned + linked notes (via entity_links or project_id)

### US-09 — Import legado
AC:
1. WHEN `vault/` exists on first Rust start THEN system SHALL import every `*.md` with frontmatter parse to `notes`
2. WHEN `data/index.db` exists THEN system SHALL migrate chunks/tags/projects
3. WHEN import done THEN system SHALL log count and keep legacy as `.bak`

## Edge Cases

| # | Cenário | Esperado |
|---|---------|----------|
| EC-01 | DB corrupt | log error, exit 1, suggest `brain restore` |
| EC-02 | Ollama timeout | note saved, vec empty, FTS works, retry on reindex |
| EC-03 | Content > max_tokens 4096 | chunk by `##` + truncate 4*max_tokens |
| EC-04 | Concurrent store | serialized via tokio mpsc single-writer actor |
| EC-05 | Export dir exists | overwrite or error if not empty, require --force |
| EC-06 | expires_at invalid format | `INVALID_PARAMS` |
| EC-07 | path traversal `../` | `INVALID_PARAMS` via sanitize |
| EC-08 | `BRAIN_VAULT_PATH` env set (legacy) | warn deprecated, ignore |

## Questions resolvidas

- Export: disco temporário `/tmp/brain-export`
- Env vault: removido
- Dim: fixo 768
- Hooks: manter Python até Fase C
