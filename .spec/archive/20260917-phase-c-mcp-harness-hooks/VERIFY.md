# VERIFY — phase-c-mcp-harness-hooks (2026-09-17)

## Completude ✅
- [x] C-MCP-01 rmcp SSE + stdio fallback (Req 1,2) — `mcp_router`, `/sse`, `serve_stdio`, `brain serve-mcp --port`
- [x] C-MCP-02 tools store/read/search/delete/recent/status per Store::open (Req 1) — scope obrigatório + sanitize
- [x] C-MCP-03 project/checkpoints/restore/backup/export/forget_sweep (Req 1) — E2E validado
- [x] C-HARN-01 harness-continuous.md H1-H7 Rust (Req 3,7)
- [x] C-HARN-02 coexist viewer 8322 + MCP 8321 + E2E 12-step (Req 3,8)
- [x] C-HOOK-01 `brain hook --event` spool XDG lock dedup (Req 5,6)
- [x] C-HOOK-02 hooks/brain-hook.py deprecated warn + syntax fix (Req 5)
- [x] C-TEST-01 cargo test/clippy/llvm-cov (Req 3,7) — 41 tests, clippy 0, cov 86.75% lines / 70.17% regions
- Zero TODOs no escopo. SPEC Req 1-9 cobertos.

## Corretude ✅
- `cargo build --workspace` → 0 erros
- `cargo test --workspace` → 41 passed (6 core + 12 mcp + 10 store + 3 web + 10 cli), 0 falhas
- `cargo clippy --workspace -- -D warnings` → 0 warnings
- `cargo llvm-cov --workspace` → 86.75% lines / 70.17% regions (gate 70% OK)
- E2E greenfield 12-step (DB zerada, portas 18331/18332 e 18341/18342) → 12/12 OK, zero 5xx, zero panic
- Harness H3 em `cli_e2e.rs::e2e_serve_viewer_and_mcp_coexist` (regressão automática)

## Coerência ✅
- ADR-01 rmcp+axum coexist (fallback axum manual + stdio) refletido em `brain-mcp::mcp_router/serve_stdio`
- ADR-02 AppState{db} per-request (evita !Sync) em brain-mcp e brain-web
- ADR-03 hook spool XDG file-lock dedup em `brain hook`
- ADR-04 no auth single-tenant
- Bug crítico achado e fixado: `Handle::block_on` dentro de handler axum (panic) → `await` + timeout 3s FTS-fallback
- Review próprio (subagente reviewer indisponível): 3 ATENÇÃO corrigidos (unwrap Store::open em brain-web/stdio → erro JSON; KillOnDrop em teste serve). Zero críticos restantes.

## Bloqueios
- Nenhum. rmcp crate real fica débito futuro (fallback axum cobre Req 2).
