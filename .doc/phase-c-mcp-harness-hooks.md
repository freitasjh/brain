# Delta Phase C (arquivado 2026-09-17)

# SPEC — phase-c-mcp-harness-hooks

## Overview
Phase C tudo junto: MCP full Rust + harness Rust + hooks Rust, local agents only, SQLite-only WAL.

## Requirements EARS (9)
1. WHEN agent calls `brain_search` via MCP SSE 8321 THEN system SHALL return RRF hybrid FTS+vector+entity+graph k60 + authority
2. WHEN MCP stdio `brain --transport stdio` called THEN system SHALL fallback axum SSE manual if rmcp unavailable
3. WHEN `cargo test --workspace` run THEN system SHALL return 0 failures (H1 suite)
4. WHEN E2E `mcp ping→store→search→read→delete→recent→export→backup→sweep→status` run THEN system SHALL pass 12 steps zero 5xx
5. WHEN `brain hook --event session-start` called THEN system SHALL spool `XDG_RUNTIME_DIR/brain/hook.jsonl` + `Store sessoes`
6. WHEN hook spool contains 2 events same id THEN system SHALL deduplicate via file lock
7. WHEN `harness-continuous.md` H6 pre-completion checked THEN system SHALL verify 7 items cargo test/build/clippy E2E H4 review brain_store ports free
8. WHEN viewer `curl /api/status` 8322 THEN system SHALL return 200 JSON notes/chunks/projects
9. WHEN Ollama down THEN system SHALL degrade FTS-only warn

## ADRs
- ADR-01 rmcp+axum coexist (fallback)
- ADR-02 harness substituir cargo 12-step
- ADR-03 hook spool XDG lock
- ADR-04 no auth single-tenant

## Delta Specs
- ADDED `crates/brain-mcp` full 13 tools rmcp registry
- MODIFIED `.agents/rules/harness-continuous.md` rewrite H1-H7 Rust table
- MODIFIED `hooks/brain-hook.py` → deprecated warn, add `brain hook` Rust
- MODIFIED `AGENTS.md` tools 18 + commands cargo
- MODIFIED `crates/brain-cli` hook subcommand

## Invariants PBT
- search RRF k60 authority +0.15/+0.1 before trunc
- TTL expires_at beats pinned

## Events
- Hook session-start → Store sessoes + brain_search global inject


## Evidências finais
- 41 testes, clippy 0, cov 86.75% lines / 70.17% regions
- E2E 12-step verde; ver VERIFY.md no archive
