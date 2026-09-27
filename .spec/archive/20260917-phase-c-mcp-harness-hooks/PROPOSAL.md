# PROPOSAL — phase-c-mcp-harness-hooks

## Problema
MCP Python legado, harness mvn kanban, hooks uv run — não nativo Rust, bloqueia agentes locais.

## Solução
Tudo junto Rust: `brain-mcp` rmcp SSE 8321 + stdio, harness cargo+12-step, hook spool Rust `brain hook`.

## Escopo
- IN: MCP 13 tools via Store per request, viewer coexist 8322, harness `cargo test` + brain 12-step, hooks spool XDG, docs rewrite harness-continuous.md substituir
- OUT: auth/multi-machine, HNSW, LLM

## Delta Specs
- ADDED `crates/brain-mcp` full tools
- MODIFIED `.agents/rules/harness-continuous.md` H1-H7 Rust
- MODIFIED `hooks/` Rust hook + deprecate py
- MODIFIED `AGENTS.md` tools table + commands

## Riscos
- rmcp instável → fallback axum (mitiga)
- hook race file lock → mitigado

## Viabilidade: OK, 2w, local only
