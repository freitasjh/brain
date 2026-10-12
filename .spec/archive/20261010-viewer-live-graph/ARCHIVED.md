# ARCHIVED — viewer-live-graph

- **Data:** 2026-10-10
- **Commit da feature:** `568de23` — `feat(web): live graph via SSE graph-changed`
- **VERIFY:** 3/3 ✅ (Completeness, Correctness, Coherence — ver VERIFY.md)
- **Destino:** `.spec/archive/20261010-viewer-live-graph/`
- **PROPOSAL.md:** não existe nesta feature (artefatos retroativos: SPEC.md + TASKS.md + VERIFY.md apenas, cf. TASKS.md DOC-02)

## Arquivos alterados pela feature (commit 568de23)

| Arquivo | Mudança |
|---------|---------|
| `crates/brain-web/src/lib.rs` | Rota `GET /api/events` (:29), handler `events` SSE + polling 2 s (:301-323), 4 testes (:1434-1572) |
| `crates/brain-web/Cargo.toml` | `tokio-stream = "0.1"` (:15) |
| `viewer/index.html` | `ensureGraphEventSource()` idempotente + listener `graph-changed` (:381-386), `renderGraph()` garante fonte aberta (:394), `stopGraph()` fecha só em troca de view (:128-131) |
| `Cargo.lock` | Lock do `tokio-stream` |
| `workflow-state.json` | `current_feature: viewer-live-graph`, `current_phase: Completed`, `code_review: passed`, `harness_status: passed`, counts Rust 488 / Python 124+2 legadas |
| `.spec/viewer-live-graph/` | SPEC.md + TASKS.md + VERIFY.md (movidos para cá no archive) |

## Delta Specs mesclado (archive 2026-10-10)

- `.agents/rules/frontend-rules.md` — seção Viewer: contrato estendido para `GET /api/status,search,read,list,graph,events` + bullet live update (`/api/events` SSE, `text/event-stream`, evento `graph-changed`, polling 2 s; `EventSource` idempotente, lifecycle separado do RAF loop)
- `AGENTS.md` — **sem mudança**: não há tabela de endpoints REST do viewer (só lista inline em `brain serve --port`); delta mandava adicionar linha apenas se a tabela existisse
- `architecture-rules.md`, `database-rules.md` — **sem mudança** (crate boundary preservada, sem schema), cf. SPEC.md Delta Specs

## Fora de escopo (não virou doc de governança)

Evento global sem payload, sem push/SQLite hooks, sem `ollama` no `/api/status` do viewer, sem retry com log no poll (gap registrado como lição no brain, não como feature) — ver SPEC.md "Fora de escopo".
