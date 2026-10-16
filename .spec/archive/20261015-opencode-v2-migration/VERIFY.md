# VERIFY — opencode-v2-migration

- **Data:** 2026-10-15
- **Entradas:** PROPOSAL.md, tasks.md (todos checkboxes marcados), código implementado
- **Harness:** relatório H1 consolidado (qa-engineer, 7/7 gates)

## Dimensão 1 — Completude ✅

- [x] **Tasks 100%:** T1–T9 marcadas em `tasks.md`
- [x] **Sem TODOs:** grep `TODO|FIXME|HACK` nos arquivos migrados — vazio (débitos são 🟢 listados, não TODO aninhado)
- [x] **Sem tasks puladas:** 0 N/A; os débitos 🟢 estão explícitos em `tasks.md` e neste VERIFY
- [x] **Cenários cobertos:** fail-open (binários ausente/MCP down), append-only (`string[]` e objeto[]), once-per-session, `userQuestion` só role=user, compaction preserva prompt default, greps V1 negativos

## Dimensão 2 — Correção ✅

- [x] **Build limpo:** `cargo clippy --all-targets -- -D warnings` exit 0, 0 warnings
- [x] **Testes passando:** `cargo test --workspace` **516 passed / 0 failed**; `node --test` **11 pass / 0 fail**
- [x] **Runtime V2:** `opencode v2.0.29` carrega o projeto sem warnings de compat; `plugin list` exibe IDs estáveis `harness-validator`/`hitl-guardrail`
- [x] **Requisitos verificáveis:** RF1–RF6 em `trace.md` (score 100%, 6/6)
- [x] **Backend MCP:** `brain ping` → `pong` (scheme SSE 8321, pid pré-existente intacto)
- [x] **Instalador:** `brain setup opencode --yes` em dir temporário exit 0, sem pânica; `mcp.json`/`opencode.json` em V2 `mcp.servers`
- [x] **Segurança:** `execFile` sem shell, argv separado (injeção impossível), cwd explícito, timeout 15s, `done("")` fail-open sem rethrow, payload só `{id}` + pergunta — sem segredo; texto do assistente nunca vaza como pergunta (teste `SEGREDO-DO-MODELO`)
- N/A justificado: RBAC/multi-tenancy — migração de plataforma não toca persistência nem autorização; suite de integração existente passa

## Dimensão 3 — Coerência ✅

- [x] **Decisões refletidas:** V2-only nativo (sem dual), IDs estáveis, fail-open como contrato do plugin, Rust `serve-mcp` como comando padrão — todos visíveis no código/docs
- [x] **Sem vazamento:** instalador Rust escreve só dentro do projeto consumidor; `~/.config/opencode/*` não tocado; nenhuma feature Rust alterada
- [x] **Naming:** `OPENCODE_V2_SESSION_HOOKS`, `plugin_session_hooks`, `brainBin`, `brain_remote_entry`, `stale_env_fallback` — consistentes com o domínio
- [x] **Docs coerentes:** `CONFIGURE_MCP.md` §1 == o que o instalador gera (verificado: `has V2 mcp.servers: true`, `has V1 mcpServers: false`)
- N/A justificado: Chrome DevTools — migração sem UI; harness frontend não aplicável

## Resultado Final

```
VERIFY STATUS: APPROVED ✅
```

### Observações (mesmo em APPROVED)

1. **Débitos 🟢 aceitos** (não bloqueiam): parse `"` naive em `stale_*`; D3 sem `plugin_code_only`; `harness-validator`/`hitl-guardrail` sem teste unitário próprio; clip de `MAX_APPEND_CHARS` e `userQuestion(` `prompt:string` não exercitados.
2. **Brain MCP indisponível** durante a orquestração (Code Mode sem tools `brain_*`) — `brain_search`/`brain_store` de sessão/lições **não executados**. Registrado como pendência; registro de lições deve ocorrer na primeira sessão com MCP no ar.
3. **Escopo global** `~/.config/opencode/*` (mcp.json + plugins caveman/toon) permanece em V1 — fora do escopo aprovado, tratado em demanda separada.
4. **Contagem de testes**: `AGENTS.md` citava 213; suite agora 516 — gate é 0 falhas, atendido; bookkeeping de contagem no workflow-state.
