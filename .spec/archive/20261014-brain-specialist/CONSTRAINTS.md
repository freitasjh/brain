# CONSTRAINTS — Brain Especialista

> Fase 1 dev-planner (Analise). Toda restrição cita `arquivo:linha` ou fonte externa.
> Sem referência = ficção (regra H8 do harness do brain).

## C1. Kiro — o que o código já garante e o que falta

- Gerador atual (`crates/brain-cli/src/setup.rs:968-1077`): 1 arquivo
  `.kiro/hooks/brain-session.json`, 2 hooks **command-type**
  (`UserPromptSubmit`→`session-start`, `Stop`→`session-end`).
- `KIRO_TRIGGERS` é lista fechada com checagem fatal em runtime
  (`setup.rs:992-998`): trigger inexistente aborta o setup em vez de gerar hook
  morto que o Kiro leria sem nunca disparar.
- Dedup por `id: kiro-{event}-$$` (`setup.rs:1026`): PID do shell; sem payload o
  fallback hasheia constante e grava 1 sessão por projeto por login.
- Sem `--db` no comando gerado (`setup.rs:1049-1060`): `BRAIN_DB_PATH` vem do env
  (`brain setup shell`); risco residual = default `./data/brain.db` relativo ao
  project root vira corpus local se env ausente.
- Docs Kiro v1 (fontes em EXPLORATION §7): schema `"v1"`, triggers PascalCase,
  `command` recebe JSON no stdin e **exit 2 bloqueia** (`PreToolUse`,
  `UserPromptSubmit`, `PreTaskExec`); `agent` injeta prompt e não bloqueia; bloco
  `confirm` em `Stop`; hooks globais em `~/.kiro/hooks/`.
- **Gaps (este projeto fecha):** agent-hooks (`brain-learning`, `brain-prompt-optimizer`)
  e steering fora do gerador; `Stop` dispara por turno → guard anti-spam obrigatório;
  `SessionStart` não carrega pergunta (só `UserPromptSubmit` tem `USER_PROMPT`).

## C2. OpenCode — plugin e hook legado

- Plugin = módulo JS/TS com `tool.execute.before/after`, eventos `session.*`,
  `experimental.session.compacting` (injetar contexto na compactação).
  Fonte: `opencode.ai/docs/plugins/`. Repo usa `.opencode/plugins/` com
  `harness-validator.js` + `hitl-guardrail.js` (`opencode.json:3`).
- Gerador `opencode_plugin_js` (`setup.rs:278`) + `opencode_target` (`setup.rs:490`):
  estender, não duplicar.
- `hooks/brain-hook.sh:1-86` legado: chama `uv run brain` (caminho Python legado),
  fallback curl é no-op (`:32-36`), `session-end` grava só timestamp (`:66-69`),
  `tool-result` grava 1 nota por tool (`:59`) = risco de spam sem throttle.
- `hooks/brain-hook.py` deprecated (só spool, sem retry — decisão Z-01 registrada).

## C3. Dependências internas (não quebrar)

- `brain-cli/setup.rs` (1935 linhas, com testes): `merge_*` aditivos,
  `write_file` com `force/dry_run` (`setup.rs:121`), `.bak` em overwrite.
- Fronteiras (`architecture-rules.md`): `brain-mcp` dono de `embed_chunks`/
  `sync_note_chunks`; `brain-web` read-only; CLI consome MCP.
- Busca RRF k=60, dim 768, `CHUNK_TARGET_TOKENS` 4096: **inalterados** neste projeto.
- Paridade F2: constantes espelhadas no Python legado (`harness-continuous.md` F7);
  mudar nos dois lados no mesmo commit + `grep` nos dois.
- Skills fonte da verdade aqui: `.agents/skills/brain/` (17 tools),
  `.agents/rules/BRAIN.MCP.md`, `AGENTS.md`. Harness é outro repo — **não tocar**.

## C4. Compliance e segurança

- Allowlist `BRAIN_EXPORT_ROOT` (`fs_guard.rs`): export/backup fora da raiz recusados.
- Redação de credencial (`brain-embed`): `BRAIN_OLLAMA_URL` nunca em log/stderr/`Debug`.
- Hook que falha = warn + segue (fail-open); hook nunca bloqueia sessão nem recebe segredo no payload.
- `setup` nunca clobbera custom do usuário: merge aditivo, `.bak`, `--force` explícito (risco R3).
- Scope obrigatório em `arquitetura/regras/estudos`; `path` relativo sem repetir
  `layer/scope`; projeto via git/config, nunca hardcoded.

## C5. Operacionais e portões

- Produção 8321/8322 intacta; E2E em scratch + portas efêmeras (H3).
- Testes: `BRAIN_OLLAMA_URL` para destino morto/mock em todo processo de teste (H2.3);
  nunca `env_remove` (seleciona default real).
- Gates: `cargo test --workspace` (494) + `clippy --all-targets -D warnings` +
  `pytest src/tests/ -q` (124 + 2 legadas nomeadas) antes de declarar fase completa.
- Handoff deste projeto: `sessoes/` resumo + `estudos/` completo + decisão aqui.

## 9. Próxima fase

Fase 2 (Desafie) → `DESIGN-DECISIONS.md`: alternativas de geração (setup emite
agent-hooks? arquivos estáticos versionados?), formato do roteiro deepsearch
(skill vs steering), estratégia anti-spam, ADRs.
