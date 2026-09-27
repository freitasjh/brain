# CONSTRAINTS — Phase C

## Técnicas
- Rust 1.85 edition 2024, tokio full, axum 0.7 macros, rmcp 0.3 (fallback axum), rusqlite bundled, reqwest rustls, clap env
- DB WAL `journal_mode=WAL synchronous=NORMAL foreign_keys=ON`, SCHEMA_VERSION 4→5 se hook spool table `hook_events(id, event, payload, at)` adicionar
- Ports 8321 MCP + 8322 viewer não conflitar, `lsof -i :8321`/`8322` check free em harness
- Ollama `BRAIN_OLLAMA_URL` env (não hardcoded localhost:11434 fix A11)
- Dim 768 fixo, no auth local

## Dependências
- `brain-store` single writer `Store::open(db)` per request (evita !Sync), `brain-core` types
- `brain-web` already `AppState{db}` per request pattern — reuse para MCP
- `viewer/index.html` static ServeDir

## Compliance
- Scope mandatory `arquitetura/regras/estudos` validate antes DB
- TTL `expires_at` beats pin, audit `checkpoints`
- Harness princípios H0 zero-tolerance: `cargo test --workspace` 0 falhas antes delegar

## Limitações
- No multi-machine/auth — single tenant local, `hooks/brain-hook.py` deprecated warn
- No vector HNSW ainda — full scan + pre-filter layer/scope já mitigado

## Próximo
- DESIGN-DECISIONS.md ADRs
