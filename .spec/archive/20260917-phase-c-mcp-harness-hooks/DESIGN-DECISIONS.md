# DESIGN DECISIONS — Phase C ADRs

## ADR-01 MCP transport rmcp + axum SSE coexist viewer
Context: MCP precisa SSE 8321 + stdio fallback, viewer 8322 separado. rmcp 0.3 instável.
Options: 1) rmcp SSE, 2) axum SSE manual, 3) stdio only.
Decision: 1 + fallback 2. `brain-mcp` tenta rmcp, fallback axum SSE como `brain-web` já faz. `brain serve-mcp --port 8321` + `brain serve --port 8322` coexist (AppState db String per request).
Rationale: Reusa padrão viewer, single DB per request evita !Sync, rmcp fallback garante compat.

## ADR-02 Harness substituir .agents/rules/harness-continuous.md
Context: harness atual mvn/npm/docker+12-step kanban não roda em brain.
Decision: substituir conteúdo H1-H7 com Rust `cargo test --workspace` + brain 12-step store→search→export, manter H0 princípios zero-tolerance.
Rationale: Usuário confirmou substituir, single source truth, `cargo llvm-cov 70%` + `lsof :8321/:8322` checks.

## ADR-03 Hooks Rust `brain hook` spool
Context: `hooks/brain-hook.py` usa `uv run brain` subprocess + ` BRAIN_VAULT_PATH` env, session-start/tool-result.
Decision: Rust `brain-cli hook --event session-start|tool-result|session-end --payload JSON` spool `XDG_RUNTIME_DIR/brain/hook-spool.jsonl` + `Store::note_upsert` via `sessoes/brain/<date>` sem uv run. Python hook deprecated warn.
Rationale: Elimina deps Python, idempotência file lock, scope local apenas.

## ADR-04 No auth Fase C
Decision: single-tenant local, `brain serve` bind 127.0.0.1 only. Auth deferred Fase D.

## ADR-05 Schema hook_events optional
Decision: SCHEMA_VERSION 4→5 adicionar `hook_events(id PK, event TEXT, project TEXT, payload TEXT, at)` apenas se hook Rust precisar persistir; senão reusa `sessoes` notes + `audit_log`.
