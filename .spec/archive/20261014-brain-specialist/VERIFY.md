# VERIFY — brain-specialist (sdd-compiler modo verify)

> Executado 2026-10-14 por reviewer (somente leitura + suítes + escrita deste arquivo).
> Produção 8321/8322 intocada. Nenhum código/doc existente alterado.
> Gate adaptado: comandos Java/npm do `verify-gate.md` NÃO valem neste repo;
> fonte é `.agents/rules/harness-continuous.md` H1–H6 (portão: `cargo test --workspace`
> + `clippy --all-targets -D warnings` + `pytest` + suítes shell/node + E2E scratch).

## Evidência FRESCA (rodada por este verify, comandos + contagens)

| Portão | Comando | Resultado |
|--------|---------|-----------|
| Rust | `cargo test --workspace` (2x, 2ª p/ soma) | **516 passed, 0 failed** |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | **0 warnings** (exit 0) |
| Python | `.venv/bin/python -m pytest src/tests/ -q` | **124 passed, 2 failed** = exatamente as 2 legadas declaradas H1.4 (`test_integration_full_cycle`, `test_integration_errors` — MCP legado, `No module named brain_server`, Fase C remove) |
| Hook | `bash hooks/brain-hook.test.sh` | **7 passed, 0 failed** (obs: SU-04-CHECKLIST dizia 5 — arquivo cresceu, continua verde) |
| Sanidade Kiro | `bash hooks/kiro/sanity.sh` | **3/3 asserts verdes** |
| Plugin opencode | `node --test hooks/opencode/brain-session.test.mjs` | **9 pass, 0 fail** |

## Dimensão 1 — Completeness: ✅ (20/20 substância com evidência)

TASKS.md lista 20 tickets (SK 3, KI 5, SU 6, OC 2, DO 4). Todos têm entrega + evidência:

| Ticket | Entrega | Evidência |
|--------|---------|-----------|
| SK-01 | Roteiro deepsearch em `.agents/skills/brain/SKILL.md:450` | SK-03-CHECKLIST item 1 (grep = 1, re-verificado neste verify) |
| SK-02 | Ponte 4 linhas `BRAIN.MCP.md:121-124`, sem roteiro colado | SK-03 item 2; sanity drift green |
| SK-03 | Self-review N/A-justificado, zero 🔴 | SK-03-CHECKLIST.md |
| KI-01 | `hooks/kiro/brain-session.json` fiel ao gerador | SK-03 item 3; DO-02a §1 byte-fiel |
| KI-02a | `hooks/kiro/brain-learning.json` (14 linhas, guard + loop) | SK-03 item 4 |
| KI-02b | Guard + `hooks/kiro/sanity.sh` 3/3 | SK-03 item 5; re-rodado 3/3 aqui |
| KI-03 | `hooks/kiro/brain-prompt-optimizer.json` (dispensa + ponte p/ skill existente) | SK-03 item 6; DO-03 §2 |
| KI-04 | `steering/brain-learning.md` (21 linhas) | SK-03 item 7 |
| SU-01a | `setup kiro` copia 4 estáticos (aditivo + `.bak` + dry-run) + `ensure_db_parent` | SU-04-CHECKLIST E2E; DO-02a §1 |
| SU-01b | `setup opencode` plugin verificado + skill ref (dep OC-01 cumprida após Bloco 4) | `setup.rs:250-357` (`OPENCODE_VERIFIED_EVENTS`, `plugin_hook_keys` assert, `opencode_plugin_js`); nota brain `su01b-*` |
| SU-02a | `doctor` D1–D6 (`crates/brain-cli/src/doctor.rs`, D4 nunca falha) | código + testes matriz |
| SU-02b | Matriz 7/7 + 2 guards (mcp_down warn, sem credencial) | SU-04-CHECKLIST |
| SU-03 | `hooks/brain-hook.sh` (binário, truncamento 4000 B, throttle) | hook-test 7/0 re-rodado aqui |
| SU-04 | Suite H6 verde | SU-04-CHECKLIST; re-rodada maior aqui (516 vs 508 — suite cresceu) |
| OC-01 | `hooks/opencode/brain-session.js` (3 eventos verificados, fail-open) | node 9/9 re-rodado; call sites extraídos do binário 1.18.31 |
| OC-02 | Spike GO-condicional, handler append-only, overflow fechado na demo | OC-02-DECISION.md + DO-02a §6 (20000→4000, marker ok, default preservado) |
| DO-01 | `hooks/README.md` + `AGENTS.md` matriz IDE, 0 refs quebradas | DO-02a roteiro literal executável |
| DO-02a | Demo scratch setup+doctor verdes, lsof igual, R1 fail exato | DO-02a-EVIDENCE.md |
| DO-02b | Demo handoff top-3, loop discrimina, spam 3≤5 | DO-02b-EVIDENCE.md |
| DO-03 | I7 0 no escopo + handoff Fase 3 | DO-03-HANDOFF.md; re-verificado aqui (2 hits, ambos explicados — ver I7) |

- Sem TODOs: `grep -r TODO|FIXME|HACK` nos paths implementados → vazio.
- BDD sucesso/falha/borda por ticket: cobertos (coluna Evidência + §Correctness).

## Dimensão 2 — Correctness: ✅

RF→código (todos verificáveis neste tree):

| RF | Código |
|----|--------|
| RF1 roteiro 5 passos | `SKILL.md:450` § Roteiro deepsearch (1 ocorrência literal — I1) |
| RF2 contexto no prompt dual-IDE | `hooks/kiro/*.json` + `setup.rs` cópia/gerador + `brain-session.js` (created/compacting append-only) |
| RF3 handoff 3 camadas + loop | `brain-learning.json` + `steering/brain-learning.md` + demo DO-02b (top-3, loop reprova sem frontmatter) |
| RF4 install + doctor verde | `setup.rs` + `doctor.rs` D1–D6 + demo DO-02a (verde 2 estados, R1 fail exato + restore) |
| RF5 hook sem `uv run` | `brain-hook.sh` via `BRAIN_BIN`; 2 hits de `uv run` são **comentário** (`:9-10`, classe code_only) |
| RF6 docs consistentes | ponte `BRAIN.MCP.md:121` + `hooks/README.md` (roteiro executado literalmente) + `AGENTS.md` |

I1–I7 (re-verificados aqui onde barato):

| I | Veredito |
|---|----------|
| I1 1 fonte | ✅ `grep -c deepsearch SKILL.md` = 1; ponte usa paráfrase (sanity drift green) |
| I2 sem hardcoded | ✅ sanity A2 3/3 |
| I3 lista fechada | ✅ `KIRO_TRIGGERS` + `plugin_hook_keys` assert + doctor D1; R1 fail exato DO-02a §5 |
| I4 fail-open | ✅ hook-test backend morto; D4 warn; plugin sem throw |
| I5 `.bak` + contenção | ✅ testes SU-01a (created/overwritten/skipped/dry-run) |
| I6 doctor binário | ✅ matriz 7/7 + D4 warn |
| I7 zero assunção | ✅ `grep -rn ASSUMIDO` (fora de target/.git/.venv/data) = 2 hits: auto-menção `DO-03-HANDOFF.md:3` + legado `brain-help-visual/VERIFY.md:96` (decisão explícita usuário 2026-09-28, fora de escopo); **0 no escopo** |

Build/testes: tabela Evidência FRESCA (516/0 + clippy 0 + 124+2-legadas + 7/0 + 3/3 + 9/9).
RBAC/multi-tenancy: N/A justificado (SPEC §1–§4, repo local-first sem auth).
Cobertura ≥70%: portão H6 não exige relatório por crate neste verify; suite 516/0 sem skips novos.

## Dimensão 3 — Coherence: ✅

- 6/6 ADRs refletidos: ADR-1 estáticos copiados (não gerados, exceto session command + opencode com enforcement) · ADR-2 fonte única + pontes finas · ADR-3 guard + dedup · ADR-4 command curto + agent condicional, fail-open · ADR-5 template 3 camadas, validação instrucional · ADR-6 merge aditivo + `.bak`, doctor D1–D6.
- Sem vazamento de camada: diff só em `brain-cli` (setup/doctor/main dispatch), docs, hooks/steering; `brain-store`/`brain-core`/`brain-embed`/`brain-mcp`/`brain-web` intocados (zero DDL — SPEC §5); review pós-batch sem 🔴 aberto (`workflow-state.json` code_review passed, 0 críticos).
- `.py` legado intocado: `git diff HEAD -- hooks/brain-hook.py src/brain_server/` vazio (Z-01 respeitado).
- `uv.lock`: só bump do pin editável `brain-server 0.8.1→0.9.1`, sem troca de dependência.
- Segurança (SPEC §4): allowlist preservada (nenhuma escrita nova fora de target/`.kiro`/home); sem credencial em hook/log/relatório (grep `BRAIN_OLLAMA_URL|password|secret` nos hooks → 0; guard `nenhuma_credencial_no_relatorio`); fail-open em todo hook.

## Traceability score

- RF→task: 6/6 (SPEC §11 mapeia todos; confirmado acima). Task→code+evidência: 20/20.
- **Score: 26/26 = 100%.** Nenhum RF órfão, nenhuma task sem entrega.

## Gaps (nenhum bloqueante)

1. 🟡 **TASKS.md com status `Todo`/checkboxes desmarcados** — substância 20/20 entregue, mas o arquivo-fonte não foi flipado. Recomendação: orquestrador flipa no archive (ou aceita evidências como Done). Não bloqueia: verify-gate mira trabalho feito+testado, não tinta em checkbox.
2. 🟡 P2 flake `config::concurrency` (1 ocorrência, TD arquivado em `.spec/tech-debts/config-concurrency-flake/`, critério 2ª→P1). Harness, não feature. Não re-ocorreu neste verify (516/0 limpo, 2 runs).
3. 🟡 P2 TD-F4 (typo em `project` cria projeto silencioso) + P3s TD-F1/F2/F3 — registrados em `.spec/tech-debts/project-shared-followup/`, fora de escopo desta feature.
4. Obs (não gap): hook-test 5→7 testes e suite 508→516 desde SU-04 — crescimento verde, sem regressão.

## Veredito

```
VERIFY STATUS: APPROVED ✅

Dimensões: Completeness ✅ | Correctness ✅ | Coherence ✅
Traceability: 100% (26/26)
Bloqueadores: nenhum
Archive: SIM — pode mover p/ .spec/archive/<data>-brain-specialist/ (com flip dos checkboxes TASKS.md no ato)
```

Observações p/ archive: incluir este VERIFY.md; `brain_store` das lições com scope correto fica com o orquestrador (fora do escopo read-only deste verify); produção segue intocada.
