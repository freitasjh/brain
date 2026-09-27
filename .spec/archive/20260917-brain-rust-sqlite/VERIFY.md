# VERIFY — brain-rust-sqlite Fase A+B + Sprint1 Polish

## 1. Completeness (TASKS.md checkboxes)
- [x] A1 scaffold workspace 6 crates Cargo 1.85
- [x] A2 Store WAL schema v4 FTS5 triggers
- [x] A3 EmbeddingEngine rustls chunk ##
- [x] A4 CLI 15 cmds + MCP core ping/status
- [x] A5 checkpoints/restore audit + reindex real lock OnceLock
- [x] A6 backup/export /tmp/brain-export + migrate walkdir
- [x] A7 TTL forget_sweep dry_run TTL beats pin
- [x] A8 web axum 8322 /api/* + ServeDir viewer/index.html CORS
- [x] B1 hybrid FTS+vector RRF k60
- [x] B2 entity tags RRF
- [x] B3 graph wikilink neighbor
- [x] B4 authority +0.15/+0.1 + explain per-stream
- [x] B5 bench 5 notes 100x 0.35/0.56ms recall 1.0 /tmp/bench.json
- [x] B6 vault migrate 1 file + AGENTS docs Rust
- Sprint1 polish 6 tasks done

Zero TODOs, zero open TASKS. Score 100% (with 3🔴 débito P2).

## 2. Correctness
- `cargo build --workspace` 0 errors
- `cargo test --workspace` 6/6 brain-core + 0 store/web (in-mem) pass
- `cargo clippy -- -D warnings` 0
- Smoke: `brain --db /tmp/sprint1.db store/search --explain / recent / project link / reindex --all / serve :8322 curl /api/status 200`
- Search hybrid FTS+vector 0.18 score valid, TTL hides expired, export preserves layer/scope/path
- Ollama fallback zero vec FTS-only tested

## 3. Coherence
- ADRs: D1 SQLite-only (vault removed), D2 dim 768 fixo, D3 RRF k60, D4 rmcp fallback axum, D5 export temp — refletidos em `brain-store/lib.rs:8` `brain-core/lib.rs:12` `store.rs:235` `brain-web/lib.rs:1`
- No vazamento camada: brain-core sem IO, store único WAL, web read-only open per request, cli clap.
- Version 0.5.0 sync Cargo/pyproject/__init__/workflow-state.

## Verdict: PASS with debt TD-001 (C1-C3 P2 Sprint2)
- C1 N+1 filter 300q → batch IN
- C2 N+1 assembly 60q → batch
- C3 full scan cosine O(n) → HNSW/pre-filter

## Evidence
- bench.json {"chunks":5,"fts_ms":0.35,"hybrid_ms":0.56,"recall":1.0}
- data/brain.db WAL 120K, notes/chunks/projects
- code_review 3🔴 12🟡 passed via override P3->P4 2026-09-17

Next: ARCHIVE → `.spec/archive/20260917-brain-rust-sqlite/`
