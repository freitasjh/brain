# SPEC — Brain Especialista (skill + hooks + instalador dual-IDE)

> Feature: `brain-specialist` | Fase: SDD-1 | Base: PROPOSAL.md + 5 artefatos dev-planner.
> Gates enterprise inaplicáveis marcados **N/A justificado** (repo Rust single-module,
> local-first, sem auth/rede multi-tenant — ver `architecture-rules.md`, `harness-continuous.md`).

## 1. ADRs (resumo; íntegra em `DESIGN-DECISIONS.md`)

| ADR | Veredito |
|-----|----------|
| ADR-1 entrega agent-hooks | arquivos estáticos versionados copiados pelo setup, não gerados em Rust |
| ADR-2 roteiro deepsearch | fonte única na skill; pontes de 3–5 linhas nos hooks/steering |
| ADR-3 anti-spam Stop/turno | guard "skip se trivial" + dedup; curadoria só via MCP |
| ADR-4 contexto no prompt | command (resumo truncado best-effort) + agent (instrução condicional); fail-open |
| ADR-5 handoff rico | 3 camadas + loop qualidade; validação instrucional, sem bloqueio |
| ADR-6 setup/doctor | merge aditivo + `.bak`; `doctor` D1–D6 binário |

## 2. Contexto de domínio e fronteiras

- **Bounded context**: este repo (`brain`); harness é outro repo — integração só por
  referência externa + campo evidência `brainStore` (intocado aqui).
- **System type**: CLI + MCP SSE (8321) + viewer read-only (8322) + skills/hooks instaláveis.
- **Tenant strategy**: N/A justificado — corpus único; isolamento lógico = `scope` + `project`.
- **Provisioning/lifecycle**: N/A justificado — sem tenants; projeto via `brain_project_*` existente.

## 3. IAM / RBAC

N/A justificado — sem camada de autenticação (operador local). Segurança aplicável §4.

## 4. Segurança (fonte: `security-standard`/C4 + F3/F4 do harness)

- Allowlist `BRAIN_EXPORT_ROOT` preservada em qualquer escrita nova de hook/setup.
- `BRAIN_OLLAMA_URL` nunca em log/stderr/`Debug`/payload de hook (redação existente).
- Hook que falha = warn + segue; nenhum hook recebe segredo; nenhum hook bloqueia sessão.
- `doctor` nunca imprime env com credencial (segue padrão do `--help` com redact).
- Threat model: hook malicioso com `exit 2`? Fora de escopo (nossos hooks não bloqueiam);
  setup nunca escreve fora do target/`.kiro`/home (contenção canônica como `fs_guard`).

## 5. Dados (sem DDL — convenções)

Modelo físico inalterado (`notes/chunks/projects`, schema v4). Convenções contratuais:
- `sessoes/<proj>/<AAAA-MM-DD>-<assunto>`; `estudos/<proj|global>/<assunto>` + frontmatter;
  `regras|arquitetura/<proj|global>/<assunto>`; `path` relativo; `project` sempre; scope obrigatório.
- ERD/migrations: N/A justificado (zero DDL; migrações = bump `SCHEMA_VERSION` + `init_schema`).

## 6. Eventos e fluxos (sem broker; triggers de IDE)

| Trigger | Produtor | Consumidor | Payload | Idempotência |
|---------|----------|------------|---------|--------------|
| Kiro `UserPromptSubmit` | Kiro | command-hook + agent-hook | `USER_PROMPT` env / prompt injetado | dedup `kiro-{event}-$$` no spool |
| Kiro `Stop` | Kiro | agent-hook handoff | instrução + guard | skip-se-trivial + dedup conteúdo |
| Kiro `SessionStart` | Kiro | (não usado p/ contexto — sem pergunta ainda) | — | — |
| OpenCode `session.created`/idle, `tool.before/after` | runtime | plugin sessão | contexto/resumo | best-effort, sem retry |
| `experimental.session.compacting` | runtime | plugin (spike T13) | resumo brain | best-effort |

## 7. Invariantes

| # | Invariante | Onde testar |
|---|------------|-------------|
| I1 | Roteiro deepsearch existe em exatamente 1 arquivo-fonte | grep anti-duplicação |
| I2 | Agent-hook/steering nunca contém nome de projeto hardcoded | teste sanidade |
| I3 | Trigger de hook gerado/copiado ∈ lista fechada (`KIRO_TRIGGERS` / eventos plugin) | teste unitário setup + `doctor` D1 |
| I4 | Falha de MCP/Ollama em hook nunca falha a sessão (fail-open) | teste com backend morto |
| I5 | `setup` nunca sobrescreve sem `.bak` nem escreve fora do alvo | teste overwrite + contenção |
| I6 | `doctor` só sai verde com D1–D6 todos ok; `ping` fail = warn, não fail | teste matriz |
| I7 | Zero marcadores de assunção restantes ao fim da implementação | grep pré-verify |

## 8. Requisitos funcionais e jornadas

- **RF1 (CU1)**: skill contém roteiro deepsearch em 5 passos executável por qualquer agente.
- **RF2 (CU2)**: `setup kiro|opencode` entrega contexto no prompt (command curto + instrução agent).
- **RF3 (CU3)**: `Stop`/fim gera (ou instrui) handoff 3 camadas com loop qualidade.
- **RF4 (CU4)**: instalação dual-IDE completa + `doctor` verde em scratch.
- **RF5**: `brain-hook.sh` sem `uv run`, com truncamento e throttle.
- **RF6**: docs (`SKILL`, `BRAIN.MCP`, `hooks/README`, `AGENTS.md`) consistentes com o instalado.
- Jornada primária: `setup → dev com contexto → handoff rico → doctor verde → busca acha tudo`.

## 9. Estratégia de testes

- Unit: `setup.rs` (merge, triggers, sanidade estáticos), `doctor` matriz D1–D6, script shell (shellcheck + teste morto-vivo MCP).
- Integração: install scratch por IDE → `doctor` → handoff demo → `brain_search` acha (H3 scratch, portas efêmeras).
- Portão H6: `cargo test` 494/0 + clippy 0 + pytest 124/2-legadas + review zero 🔴 + `brain_store` lições.

## 10. Delta Specs

| Doc alvo | Tipo | Seção |
|----------|------|-------|
| `.agents/skills/brain/SKILL.md` | MODIFIED | +Roteiro deepsearch |
| `.agents/rules/BRAIN.MCP.md` | MODIFIED | +ponte |
| `hooks/README.md` | MODIFIED | matriz IDE |
| `AGENTS.md` | MODIFIED | setup/doctor |
| `crates/brain-cli/src/setup.rs` | MODIFIED | cópia + doctor |
| `hooks/kiro/*`, `steering/*`, `hooks/opencode/*` | ADDED | novos |
| `.agents/skills/brain/tools/` | UNCHANGED | — |
| Repo harness | UNTOUCHED | handoff separado |

## 11. Critérios de aceite (de PROPOSAL §7, rastreáveis)

RF1→T1–T3, RF2→T4/T6/T8/T12, RF3→T5/T7, RF4→T8/T9/T15, RF5→T10, RF6→T2/T14; I1–I7 no verify.
