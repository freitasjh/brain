# Change Proposal: Brain Especialista

> **ID**: `brain-specialist`
> **Status**: `Archived`
> **Author**: orchestrator (Brain Core)
> **Date**: 2026-10-14
> **Source**: `.spec/brain-specialist/EXPLORATION.md` (dev-planner Fase 0, aprovado)

---

## 1. Problem Statement

- **Contexto**: skill `brain` descreve tools mas não o protocolo de uso especialista;
  hooks gravam spool bruto, não curadoria; `setup opencode|kiro` instala só captura
  command-type; harness carrega cópia divergente da skill (fora de escopo aqui).
- **Impacto atual**: agentes fazem 1 search rasa ou nenhuma; aprendizado evapora entre
  sessões; desktop (opencode/kiro) não recebe o padrão agent+command validado no boleto.
- **Evidência**: `regras/projetos/boleto/aprendizado-via-mcp` (0 notas `sessoes`,
  spool ≠ curadoria); `regras/global/kiro-hooks-agent-type-aprendizado` (só agent-type
  gerou aprendizado); `hooks/brain-hook.sh:66-69` (session-end só timestamp).

---

## 2. Proposed Solution

**Resumo**: tornar o brain especialista ativo via roteiro deepsearch na skill + 3 hooks
por IDE (contexto, handoff rico, prompt-optimizer) + `setup` dual-IDE completo + `doctor`.

**Abordagem**:
- Roteiro deepsearch mora só na skill; hooks/steering com pontes de 3–5 linhas (ADR-2).
- Agent-hooks + steering como arquivos estáticos versionados copiados pelo setup (ADR-1).
- Contexto dual command+agent com fail-open; handoff 3 camadas sem bloqueio (ADR-3/4/5).
- Alternativas descartadas: só-docs (ninguém instala), só-command (refutado pelo boleto) —
  ver EXPLORATION §5 e DESIGN-DECISIONS.

---

## 3. Scope

### In Scope
- [ ] Roteiro deepsearch em `.agents/skills/brain/SKILL.md` + ponte em `BRAIN.MCP.md`
- [ ] `hooks/kiro/*.json` (3) + `steering/brain-learning.md` (estáticos versionados)
- [ ] `setup kiro|opencode` estendido (cópia aditiva, `.bak`, `--force/--dry-run`)
- [ ] Comando `doctor` D1–D6 + testes
- [ ] Plugin sessão opencode estendido (+ spike `compacting`)
- [ ] `brain-hook.sh` modernizado (binário, truncamento, throttle)
- [ ] Docs (`hooks/README.md`, `AGENTS.md`) + demo fim-a-fim (T15)

### Out of Scope
- Qualquer arquivo no repo harness (handoff separado já entregue)
- DDL/schema, RRF k=60, dim 768, tool MCP nova, drain do spool (Z-01), enforcement bloqueante

---

## 4. Architecture Impact

| Aspecto | Impacto | ADR Necessário? |
|---------|---------|-----------------|
| Backend crates (`brain-cli` setup, `brain-mcp` hook) | estende `setup.rs`, `brain-hook.sh`; sem tocar store/embed | Sim (ADR-1/6 em DESIGN-DECISIONS) |
| Frontend viewer | nenhum (read-only; só docs se badge mudar — não muda) | Não |
| Banco de dados | zero DDL; só convenções de gravação | Não |
| Multi-tenancy | N/A — corpus único local; isolamento = `scope`+`project` já existentes | Não |
| Segurança | allowlist export + redação credencial + fail-open preservados; sem segredo em payload | Sim (C4, sem IAM — justificado: local-first sem camada auth) |
| Fila embedding | nenhum (reuso; hooks nunca embedam inline) | Não |

---

## 5. Delta Preview

| Documento Alvo | Tipo de Mudança | Seção Afetada |
|----------------|-----------------|---------------|
| `.agents/skills/brain/SKILL.md` | MODIFIED | +seção Roteiro deepsearch |
| `.agents/rules/BRAIN.MCP.md` | MODIFIED | +ponte 3 linhas |
| `hooks/README.md` | MODIFIED | matriz por IDE (3 hooks + steering + global) |
| `AGENTS.md` | MODIFIED | tabela setup (agent-hooks, steering, doctor) |
| `crates/brain-cli/src/setup.rs` | MODIFIED | cópia estáticos + `doctor` |
| `hooks/kiro/*.json`, `steering/` | ADDED | 3 hooks + 1 steering |
| `.agents/skills/brain/tools/` | UNCHANGED | 17 tools intactas |

---

## 6. Risk Assessment

| Risco | Probabilidade | Impacto | Mitigação |
|-------|---------------|---------|-----------|
| R1 trigger delegação Kiro difere do previsto | Média | Alto | spike empírico T15; matcher amplo + log |
| R2 spam de notas (Stop por turno) | Alta | Médio | guard + dedup; métrica notas/dia |
| R3 setup clobbera custom do usuário | Baixa | Alto | merge aditivo + `.bak` + teste overwrite |
| R4 drift skill↔hooks | Média | Médio | roteiro 1x + grep anti-duplicação |

---

## 7. Success Criteria

- [ ] Funcional: `setup kiro` + `setup opencode` em scratch instalam matriz completa; `doctor` verde
- [ ] Funcional: demo fim-a-fim gera handoff 3 camadas pesquisável via `brain_search`
- [ ] Técnico: `cargo test --workspace` 0 falhas + `clippy --all-targets` 0 + `pytest` 124/2-legadas
- [ ] Qualidade: zero marcadores de assunção restantes; gates proposal/spec verdes (N/A justificados)

---

## 8. Completion Metadata

- **Archived on**: 2026-10-14
- **Duration**: 1 dia (2026-10-14, EXPLORATION→archive)
- **Archive path**: `.spec/archive/20261014-brain-specialist/`
- **Files changed**: `.agents/skills/brain/SKILL.md`, `.agents/rules/BRAIN.MCP.md`, `hooks/README.md`, `AGENTS.md`, `crates/brain-cli/src/setup.rs`, `crates/brain-cli/src/doctor.rs` (new), `crates/brain-cli/src/main.rs`, `crates/brain-cli/tests/setup_interactive.rs`, `crates/brain-cli/tests/help_overview.rs`, `hooks/brain-hook.sh`, `hooks/brain-hook.test.sh` (new), `hooks/kiro/` (new: 3 json + sanity.sh), `hooks/opencode/` (new: js + test), `steering/brain-learning.md` (new), `Cargo.toml` + `pyproject.toml` + `src/brain_server/__init__.py` + `uv.lock` (0.9.1→0.10.0), `workflow-state.json`
