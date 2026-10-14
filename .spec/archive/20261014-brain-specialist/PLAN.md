# Implementation Plan: brain-specialist

## 1. Context & SPEC Traceability
- **Target**: `.spec/brain-specialist/SPEC.md` (RF1–RF6, I1–I7, 6 ADRs)
- **Goal**: roteiro deepsearch + 3 hooks/IDE + setup dual + doctor + docs, sem DDL.

### 1.1 SPEC Validation Report
- **Status**: Validated with Warnings
- **Warnings** (resolvidos DO-03): marcas de assunção em EXPLORATION (restrições prod, pacote de aceite, ordem).
- **Assumptions**: prod 8321/8322 intacta; falha MCP/Ollama = warn; projeto via git/config.

## 2. Technical Strategy
### 2.1 Backend Strategy
- **Framework**: Rust 1.85 workspace (brain-cli/setup, brain-mcp/hook); shell (brain-hook.sh).
- **Pattern**: single `Store`, AppState per-request; setup merge-aditivo + `.bak` + dry-run.
### 2.2 Frontend Strategy
- N/A justificado — viewer read-only intocado; UI = arquivos `.json/.md/.js` de hook.
### 2.3 Infrastructure Strategy
- **Deploy**: local-first; `systemd --user` existente; E2E em scratch + portas efêmeras.
- **CI local**: `cargo test` + `clippy --all-targets -D warnings` + `pytest` (portão H6).

## 3. Constraint Mapping
| SPEC Ref | Constraint | Enforcement Point | Failure Mode | Strategy |
|----------|------------|-------------------|--------------|----------|
| C1 | trigger ∈ lista fechada | setup + doctor D1 | hook morto silencioso | `KIRO_TRIGGERS` fatal + teste |
| C1 | Stop/turno anti-spam | prompt agent-hook + dedup | spam notas finas | guard + métrica notas/dia |
| C2 | sem `uv run`/curl no-op | brain-hook.sh | hook quebrado no desktop | binário + truncamento + throttle |
| C3 | fronteiras crates | code review | vazamento camada | reviewer pós-batch |
| C4 | allowlist + redação + fail-open | hooks/setup/doctor | vazamento segredo / sessão travada | fs_guard + redact + warn |
| C4 | merge aditivo + `.bak` | setup | clobber custom | write_file + teste overwrite |
| C5 | gates H1–H6 | cada batch | regressão | suite completa antes/depois |

## 4. Module & Code Structure
| Module | Responsibilities | Boundaries |
|--------|------------------|------------|
| `.agents/skills/brain/` | roteiro deepsearch (fonte única) | só markdown; sem lógica |
| `hooks/kiro/`, `steering/` | 3 json + 1 md estáticos | sem nome de projeto; triggers fechados |
| `hooks/opencode/` | plugin sessão | estende `opencode_plugin_js`, sem enforcement |
| `crates/brain-cli/src/setup.rs` | cópia + doctor | reusa merge/write_file; sem `--db` congelado |
| `hooks/brain-hook.sh` | spoolnberg CLI | best-effort; exit 0 sempre (salvo uso inválido) |

## 5. Risks
- R1 trigger delegação Kiro (alto): spike T15/DO-02; fallback matcher amplo + log.
- R2 spam (médio): guard + dedup; aborta e replaneja se >5 notas/dia/projeto no demo.
- R3 clobber (médio): `.bak` + teste.
- R4 drift (baixo): grep I1.

## 6. Implementation Phases & Dependencies
### Phase 1: Skill (SK-01–03). Deps: none.
### Phase 2: Kiro estático (KI-01–04). Deps: Phase 1.
### Phase 3: Setup+doctor (SU-01–04). Deps: Phase 2 (p/ cópia), T10 independente.
### Phase 4: OpenCode (OC-01–02). Deps: Phase 1.
### Phase 5: Docs+demo (DO-01–03). Deps: Phases 1–4.

## 7. Critical Execution Path
SK-01 → KI-02a → SU-01a → SU-02a → DO-02a → DO-03

## 8. Atomic Task Breakdown
### Phase 1 — Skill
- **[SK-01]** [ ] Roteiro deepsearch 5 passos em `SKILL.md`. Dep: none. Artifact: `SKILL.md`. Enforcement: I1 (grep 1 ocorrência). Ref: RF1/ADR-2.
- **[SK-02]** [ ] Ponte 3 linhas em `BRAIN.MCP.md` + grep anti-duplicação. Dep: SK-01. Ref: RF6.
- **[SK-03]** [ ] Self-review checklist spec-gate adaptado. Dep: SK-01–02. Ref: I7.
### Phase 2 — Kiro
- **[KI-01]** [ ] `hooks/kiro/brain-session.json` fiel ao gerador atual. Dep: SK-01. Ref: RF2/C1.
- **[KI-02a]** [ ] `hooks/kiro/brain-learning.json` (template 3 camadas + loop qualidade). Dep: SK-01. Ref: RF3/ADR-3.
- **[KI-02b]** [ ] Guard skip-se-trivial + teste sanidade (sem hardcoded, trigger válido). Dep: KI-02a. Enforcement: I2/I3.
- **[KI-03]** [ ] `hooks/kiro/brain-prompt-optimizer.json` (dispensa leitura). Dep: SK-01. Ref: RF2.
- **[KI-04]** [ ] `steering/brain-learning.md` (buscar-antes/gravar-depois). Dep: SK-01. Ref: RF3.
### Phase 3 — Setup+doctor
- **[SU-01a]** [ ] `setup kiro`: copia 3 json + steering (aditivo, `.bak`, force/dry-run). Dep: KI-01–04. Enforcement: I5. Ref: RF4/ADR-6.
- **[SU-01b]** [ ] `setup opencode`: registra plugin sessão + skill ref. Dep: OC-01. Ref: RF4.
- **[SU-02a]** [ ] `doctor` D1–D6 implementação. Dep: SU-01a. Enforcement: I6. Ref: RF4.
- **[SU-02b]** [ ] Testes `doctor` (matriz verde/vermelha + ping-morto=warn). Dep: SU-02a. TDD first.
- **[SU-03]** [ ] `brain-hook.sh`: binário + truncamento + throttle tool-result. Dep: none. Ref: RF5/C2.
- **[SU-04]** [ ] Suite verde H6 (494 + clippy + pytest). Dep: SU-01–03. Ref: §9 SPEC.
### Phase 4 — OpenCode
- **[OC-01]** [ ] Plugin sessão: contexto em created/prompt-submit + resumo em idle. Dep: SK-01. Ref: RF2.
- **[OC-02]** [ ] Spike `experimental.session.compacting` (se instável → roadmap). Dep: OC-01. Ref: ADR-6.
### Phase 5 — Docs+demo
- **[DO-01]** [ ] `hooks/README.md` + `AGENTS.md` (matriz IDE). Dep: KI-01–OC-01. Ref: RF6.
- **[DO-02a]** [ ] Demo scratch: setup kiro+opencode → `doctor` verde. Dep: SU-02b, DO-01. Ref: RF4/R1.
- **[DO-02b]** [ ] Demo handoff rico → `brain_search` acha; métrica spam ok. Dep: DO-02a. Ref: RF3/R2.
- **[DO-03]** [ ] grep zero p/ token de assunção + handoff p/ Fase 3 (delegação). Dep: DO-02b. Enforcement: I7.

## 9. Event Mapping
Eventos SPEC §6 são triggers locais de IDE (sem broker): Producer=comando/hook gerado (SU-01/OC-01),
Outbox=BROKER=Consumer=N/A justificado (spool local + MCP direto, sem mensageria distribuída).
Idempotência via dedup `$$`/conteúdo (KI-02b, C1).

## 10. Readiness Checklist
- [x] Deps acíclicas (fases lineares + T10/OC paralelos).
- [x] Eventos mapeados (§9, N/A justificado).
- [x] Testes: SU-02b + SU-04 + DO-02 explícitos; falha fail-open em KI-02b/SU-03.
- [x] Infra primeiro: skill (fonte) antes de cópia/instalação.
- [x] Caminho crítico §7 só fluxo nuclear.
