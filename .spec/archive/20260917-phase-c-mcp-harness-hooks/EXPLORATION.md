# EXPLORATION — Phase C MCP/Harness/Hooks (tudo junto)

## Problema
Brain 0.5.0 Rust SQLite-only RRF funciona via CLI (`brain store/search`) mas MCP ainda Python legado (`src/brain_server/server.py` FastMCP), harness ainda mvn/Atlas ECM, hooks ainda `hooks/brain-hook.py uv run brain` subprocess. Phase C unifica tudo em Rust nativo para agentes locais.

## Usuários
- Agentes IA locais (OpenCode, Kiro, Copilot Chat) via MCP SSE `8321` + stdio fallback
- Operador humano via `cargo test` + `brain serve` viewer `8322`
- Hooks lifecycle session-start/tool-result capturam contexto sem `uv run`

## Contexto
- Stack atual: 6 crates, `Store WAL FTS5`, `Embed Ollama 768`, `brain-web` axum, `brain-cli` 15 cmds, viewer ServeDir OK, debt batch fix 3q
- Legado: `src/brain_server/` + `viewer/server.py` + `hooks/brain-hook.py` + `.agents/rules/harness-continuous.md` mvn
- Constraints: dim 768 fixo, WAL single writer, no auth single-tenant, port 8321/8322

## Alternativas consideradas
1. **Tudo junto (escolhido)**: MCP rmcp + harness cargo + hooks spool em um sprint 2w — risco maior mas entrega valor completo, evita 3 verifies.
2. Split C1/C2/C3 incrementais — mais seguro mas 3× overhead SPEC/VERIFY, usuário rejeitou (opção 1 tudo junto).
3. Manter Python MCP + apenas harness — deixaria dívida Python, rejeitado.

## Escopo Phase C (tudo junto)
- MCP full: `crates/brain-mcp` rmcp SSE `BRAIN_PORT 8321` + stdio fallback, tools `store/read/search/delete/recent/status/checkpoints/restore/backup/export/forget-sweep/project` via `Store::open` per request, coexist viewer `8322` (AppState db String)
- Harness substituir: `.agents/rules/harness-continuous.md` reescrever H1-H7 `cargo test --workspace` + brain 12-step `store→search RRF→read→recent→export→backup→sweep` pre-completion 7 items Rust, E2E `mcp ping→store→search→read→delete`
- Hooks substituir: Rust `brain hook --event session-start|tool-result|session-end` spool sanitizado `ignore_paths` (sem uv run), `hooks/brain-hook.py` arquivado para compat, `hooks/brain-hook.rs`? Na verdade `brain-cli hook` subcommand.

## Fontes consultadas (websearch simulado)
- rmcp 0.3 + axum SSE example (MCP transport)
- FTS5 porter vs trigram benchmarks
- Harness zero-tolerance rust `cargo llvm-cov` 70%

## Riscos
- rmcp 0.3 API instável — fallback axum SSE manual já provado em viewer
- Hooks spool idempotência — usar `XDG_RUNTIME_DIR/brain-hook-spool.jsonl` + file lock

## Próximo
- CONSTRAINTS.md → restrições técnicas, dependências, compliance
