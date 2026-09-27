# Phase 3 — Tasks: brain-rust-sqlite Fase A+B

- [ ] A1 Scaffold workspace + crates
  - Criar `Cargo.toml` workspace resolver 3 edition 2024, crates `brain-core/store/embed/mcp/cli/web`, bin `brain`
  - Deps: tokio full, axum 0.7, serde, anyhow, tracing, clap, rusqlite bundled, reqwest rustls
  - _Requirements: US-01_

- [ ] A2 Store actor + schema v4
  - `brain-store/src/lib.rs`: `StoreActor`, `init_schema()` SQL v4, `upsert/remove/search/size`, WAL, FTS5 triggers, `_meta`
  - Test in-mem `:memory:` dim 768 fixo
  - _Requirements: US-02, US-04_

- [ ] A3 EmbeddingEngine port
  - `brain-embed/src/lib.rs`: `health_check`, `embed`, `embed_batch_concurrent(4)`, `chunk_text ##` port `engine.py:94`
  - _Requirements: US-01, US-02_

- [ ] A4 MCP tools core
  - `brain-mcp`: `brain_store/read/search` SQLite-only (sanitize `vault/models.py:64`), `brain_project_*`
  - _Requirements: US-02, US-03, US-08_

- [ ] A5 Ops tools
  - `brain_delete_page`, `brain_recent`, `brain_status`, `brain_checkpoints` (audit_log), `brain_restore_page`, `brain_reindex` background + lock
  - _Requirements: US-04, US-06_

- [ ] A6 Backup + export temp
  - `brain_backup` sqlite backup API, `brain export [--to /tmp/brain-export]` dump `layer/scope/path.md` temp
  - _Requirements: US-06_

- [ ] A7 TTL sweep
  - `brain_forget_sweep [--dry-run]`, `expires_at` filter search, pin exempt, TTL beats pin
  - _Requirements: US-07_

- [ ] A8 Web viewer fix
  - `brain-web` axum 8322 `/api/status,search,read,list` sobre `brain.db`, corrige `viewer/server.py:147`
  - _Requirements: US-04_

- [ ] B1 Hybrid FTS5+vector RRF
  - `search` vec `vec_chunks MATCH` + `notes_fts MATCH` → RRF k=60
  - _Requirements: US-05_

- [ ] B2 Entity RRF
  - Parse `tags/entities` frontmatter → `entities/entity_links` → 3º stream RRF prefix
  - _Requirements: US-05_

- [ ] B3 Graph links RRF
  - Parser `[[path]]` → `links` → neighbor expansion 1-hop RRF
  - _Requirements: US-05_

- [ ] B4 Authority + explain
  - Boost `arquitetura/regras` + `pinned` antes truncar, `explain=true` per-stream ranks
  - _Requirements: US-05_

- [ ] B5 Benchmark
  - Eval recall híbrido vs cosine puro, report `/tmp/bench.json`
  - _Requirements: US-05_

- [ ] B6 Migration legado + remover vault env
  - Import `vault/**/*.md` + `data/index.db` → `brain.db`, `vault.bak.tar.gz`, remover `BRAIN_VAULT_PATH` de `config.py:45` e docs `AGENTS.md:79`
  - _Requirements: US-09_

Sequência: A1→A2→A3→A4→A5→A6→A7→A8→B1→B2→B3→B4→B5→B6
Estimativa: A 16h + B 12h = 28h
