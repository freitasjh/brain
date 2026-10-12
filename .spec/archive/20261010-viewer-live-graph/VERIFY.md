# VERIFY — viewer-live-graph

> Retroativo. Veredito sobre o entregue, medido em 2026-10-10.
> Produção intacta: só `.spec/` escrito; `crates/` e `viewer/` intocados
> (gate: `git status --porcelain` mostra apenas `.spec/viewer-live-graph/`).

## Dimensão 1 — Completeness ✅

TASKS.md: 16/16 `[x]`, zero `[ ]`, zero TODOs.
RF-01..RF-06 todos com implementação + teste ou evidência QA citados em SPEC.md.
Delta Specs registrado (frontend-rules.md: rota `/api/events`).
Nenhum requisito inventado: seção "Fora de escopo" lista o que NÃO foi entregue
(evento global sem payload, sem push/hooks, sem ollama no status, sem retry com
log no poll — gap registrado como lição, não como feature).

## Dimensão 2 — Correctness ✅

| Gate | Comando | Resultado 2026-10-10 |
|------|---------|----------------------|
| Rust | `cargo test --workspace` | **488 passed, 0 failed** (+4 vs 484: os testes `events_*`/viewer/`render_graph_*` da feature) |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | **0 warnings, exit 0** |
| Python | `.venv/bin/python -m pytest src/tests/ -q` | **124 passed, 2 failed — exatamente as 2 legadas declaradas** (`test_integration_full_cycle`, `test_integration_errors`, `No module named brain_server`, H1.4; código que a Fase C remove) |
| QA browser | sessão 2026-10-10 (workflow-state `harness_detail`) | **5/5**: store 576→577 sem reload, delete 577→576, EventSource OPEN, console 0, network 2xx |
| Prod SSE | `curl -D - http://127.0.0.1:8322/api/events` 2026-10-10 | **200 + `content-type: text/event-stream`** (8321/8322 intactos, sem restart) |

## Dimensão 3 — Coherence ✅

- ADRs refletidos no código: polling (sem dependência de `brain-mcp`,
  `Store` por poll, `AppState { db: String }`) e lifecycle separado
  (`ensureGraphEventSource`, sem `stopGraph()` em `renderGraph`).
- Sem vazamento de camada: `brain-web` continua folha read-only
  (`brain-store` + `brain-core` apenas).
- 2 bugs viraram guards, não voltam em silêncio: teste exige `event:` **e**
  `data:` (`lib.rs:1495`); teste proíbe `stopGraph()` em `renderGraph`
  (`lib.rs:1525`).
- Lições persistidas no brain (3 notas citadas em TASKS.md DOC-01),
  cada uma com `scope` correto (`projetos` para a decisão, `global` para as regras).

## Veredito

**3/3 ✅ — feature arquivável.** Nenhum P1/P2/P3 novo desta onda além dos gaps
já declarados (poll retry sem log = lição registrada; TD-001/TD-009 seguem
deferred/accepted por decisão do usuário).
