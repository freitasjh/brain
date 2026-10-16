# TASKS — opencode-v2-migration

Todas as tasks concluídas. Riscos/critérios conforme PROPOSAL.md.

## Batch 1 — JS / config / docs

- [x] **T1** — `opencode.json` para schema V2 nativo
  - `plugin:` → `plugins:`; `permission:{task:"ask"}` → `permissions:[{action:"subagent",resource:"*",effect:"ask"}]`
  - `agent:` → `agents:`; `prompt` → `system`; `tools:{write,edit,bash}` → `permissions[{edit},{shell}]`
  - `instructions:[".agents/rules/*.md"]` preservado
  - **DoD:** JSON válido, `opencode` carrega sem warning de compat
- [x] **T2** — `.opencode/plugins/harness-validator.js` + `hitl-guardrail.js` para V2
  - `Plugin.define({id, setup(ctx)})` com `@opencode/plugin`
  - harness-validator: `ctx.tool.hook("execute.before")` bloqueia `mvn`, libera `cargo`
  - hitl-guardrail: `ctx.permission.hook("evaluate")` default-allow fail-open
  - **DoD:** `plugin list` exibe IDs estáveis; bloqueio `mvn` verificado
- [x] **T3** — `hooks/opencode/brain-session.js` para V2 + teste
  - `ctx.session.hook("prompt")` (chat.message), `("context")` (system.transform), `("compaction")` (compacting)
  - `execFile` argv-array sem shell, `BRAIN_BIN` lazy (`brainBin()`), fail-open `done("")`, once-per-session
  - `userQuestion` filtra `role=user`; nunca vaza texto do assistante; `MAX_APPEND_CHARS=4000`
  - teste reancorado V2, sem extração de binário 1.18.31
  - **DoD:** `node --check` + `node --test` 11/11
- [x] **T4** — `.opencode/package.json` + docs
  - `@opencode-ai/plugin@1.18.30` → `@opencode/plugin@2.0.29` (+ `"type":"module"`, lock)
  - `CONFIGURE_MCP.md` §1 V2 (`mcp.servers`, binário Rust nos exemplos, tabela 17 tools rmcp)
  - `README.md`, `hooks/README.md` atualizados

## Batch 2 — Rust instalador (P1 do code review)

- [x] **T5** — C1: eliminar pânica `setup.rs:359`
  - removidos `OPENCODE_VERIFIED_EVENTS` / `OPENCODE_DEAD_EVENTS` / `plugin_hook_keys`
  - `OPENCODE_V2_SESSION_HOOKS = ["compaction","context","prompt"]` + `plugin_session_hooks()` (extrai `session.hook("…")`)
  - `stale_constant`/`stale_env_fallback` lê `brainBin()` (fallback legado preservado)
  - **DoD:** `cargo test -p brain-cli` 0 falhas (era 26+2+6); `brain setup opencode --yes` exit 0
- [x] **T6** — C2: merges para V2
  - `servers_mut` grava `mcp.servers`; `brain_remote_entry = {type:"remote",url,disabled:false,timeout:{catalog,execution}}`
  - V1 nunca escrito; pré-existente preservado
- [x] **T7** — P2/P3: doctor D3 valida shape V2 (`Plugin.define` + 3 hooks + argv); fixture V2; `AGENTS.md` §3 V2
- [x] **T8** — testes reescritos: ~16 em `setup.rs`, 2 em `cli_e2e.rs`, 6 em `setup_interactive.rs`

## Batch 3 — Harness

- [x] **T9** — 7 gates: `cargo test --workspace`, `clippy -D warnings`, `node --test`,
  load opencode v2.0.29, `brain ping`, `brain setup opencode --yes` em tmp, grep residual V1

## Débitos não bloqueantes (registrados, não executados)

- 🟢 `stale_env_fallback`/`stale_constant` parseiam `"` naive — valor exibido corta em path com `"` escapado (direção do veredito correta)
- 🟢 doctor D3 não aplica `plugin_code_only` antes de `contains` — smoke aceitável
- 🟢 `harness-validator`/`hitl-guardrail` sem teste unitário próprio (verificação via load runtime)
- 🟢 testes `MAX_APPEND_CHARS` clip e `userQuestion(` `prompt:string` não exercitados
