# TASKS — viewer-live-graph

> Retroativo: todos os checkboxes já executados e verificados.
> `[x]` = feito com evidência citada. Nenhum `[ ]` — spec sem pendência.

## Backend (`crates/brain-web`)

- [x] BE-01 — Registrar rota `.route("/api/events", get(events))` (`lib.rs:29`)
- [x] BE-02 — Handler `events` com `IntervalStream` 2 s + snapshot
      `Arc<Mutex<(i64, i64)>>` + `then`/`filter_map` (`lib.rs:301-323`)
- [x] BE-03 — Emitir `.event("graph-changed").data("graph-changed")` com ambos
      os campos (`lib.rs:315`; bug do `event:` ausente corrigido)
- [x] BE-04 — Adicionar `tokio-stream = "0.1"` em `crates/brain-web/Cargo.toml:15`
      e `use` em `lib.rs:7-8`
- [x] BE-05 — Preservar crate boundary: sem dependência de `brain-mcp`, `Store`
      por poll, `AppState { db: String }` intacto (`lib.rs:11,309`)

## Frontend (`viewer/index.html`)

- [x] FE-01 — `graphEventSource` module-level + `ensureGraphEventSource()`
      idempotente (`index.html:126,381-386`)
- [x] FE-02 — `addEventListener('graph-changed', () => { renderGraph(); })`
      (`index.html:384`)
- [x] FE-03 — `renderGraph()` chama `ensureGraphEventSource()`, cancela só RAF,
      nunca fecha EventSource (`index.html:392-394`; self-close corrigido)
- [x] FE-04 — `stopGraph()` continua fechando fonte só em troca de view
      (`index.html:128-131`)

## Testes (`crates/brain-web/src/lib.rs:1434-1572`)

- [x] TE-01 — `events_endpoint_returns_sse_content_type` (200 + `text/event-stream`)
- [x] TE-02 — `events_emits_graph_changed_on_write` (exige `event:` **e** `data:`)
- [x] TE-03 — `viewer_script_contains_eventsource_and_graph_changed` (source scan)
- [x] TE-04 — `render_graph_does_not_close_event_source` (guard 3 asserts)

## Review / QA / Deploy

- [x] RV-01 — Code review: 3 loops, APPROVED zero 🔴
      (workflow-state.json `code_review.last_status: passed`, 2026-10-10)
- [x] QA-01 — QA browser PASS 5/5: store 576→577 sem reload, delete 577→576,
      EventSource OPEN, console 0, network 2xx
      (workflow-state.json `harness_detail`, 2026-10-10)
- [x] DP-01 — Release rebuild + restart 8322 ok, 8321 intacto (mesmo `harness_detail`)
- [x] GT-01 — Gates Rust/Python verdes re-rodados (counts em VERIFY.md)
- [x] DOC-01 — Lições salvas no brain: `arquitetura/projetos/brain/viewer-live-graph-sse`,
      `regras/global/sse-event-field-discriminative-test`,
      `regras/global/.../sse-silent-stream-death`
- [x] DOC-02 — Artefatos SDD retroativos: SPEC.md + TASKS.md + VERIFY.md
      em `.spec/viewer-live-graph/`
