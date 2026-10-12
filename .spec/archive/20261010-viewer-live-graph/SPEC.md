# SPEC — viewer-live-graph

> Documento retroativo. Implementação, review APPROVED e QA PASS 5/5 já
> concluídos com aprovação do usuário (workflow-state.json
> `current_feature: viewer-live-graph`, `current_phase: Completed`,
> `code_review.last_status: passed`, `harness_status: passed`).
> Este SPEC descreve exatamente o entregue — nada além.
> Toda afirmação técnica cita `arquivo:linha` (regra H8).

## Contexto

Viewer (`viewer/index.html` + `crates/brain-web` axum read-only) exibia grafo
via `renderGraph()` mas exigia refresh manual após qualquer escrita
(store/delete/restore/project link/unlink). Feature adiciona live update via
SSE: backend emite `graph-changed`, frontend re-renderiza sem reload.

## Requisitos funcionais

### RF-01 — `GET /api/events` retorna SSE

Rota registrada em `crates/brain-web/src/lib.rs:29`
(`.route("/api/events", get(events))`).
Responde 200 com `Content-Type: text/event-stream`.
Teste: `events_endpoint_returns_sse_content_type` (`lib.rs:1442`).

### RF-02 — Emite `graph-changed` quando o DB muda

Handler `events` (`lib.rs:301-323`): polling a cada 2 s
(`Duration::from_secs(2)`, `lib.rs:304`), compara snapshot
`(count_notes(), count_chunks())` com anterior; se divergir, emite

```rust
Event::default().event("graph-changed").data("graph-changed") // lib.rs:315
```

Stream construído com `IntervalStream` + `then` (async) + `filter_map(|opt| opt)`
(`lib.rs:304-321`); termina no disconnect do cliente (axum `Sse`).
Latência percebida < 3 s (poll 2 s + propagação).
Teste: `events_emits_graph_changed_on_write` (`lib.rs:1462`) — conecta,
aguarda 2.5 s, escreve nota, exige `event: graph-changed` **e**
`data: graph-changed` dentro de 5 s (`lib.rs:1495`).

### RF-03 — Viewer re-renderiza sem reload

`viewer/index.html:381-386`: `ensureGraphEventSource()` idempotente abre
`new EventSource(`${API_BASE}/api/events`)` e registra
`addEventListener('graph-changed', () => { renderGraph(); })`.
`renderGraph()` chama `ensureGraphEventSource()` (`index.html:394`).
Store/delete no viewer refletem no grafo sem refresh (QA: 576→577 e 577→576).
Teste: `viewer_script_contains_eventsource_and_graph_changed` (`lib.rs:1511`).

### RF-04 — Lifecycle do EventSource separado do RAF loop

`renderGraph()` cancela só o RAF (`index.html:392`), **nunca** fecha o
EventSource. `stopGraph()` fecha (`graphEventSource.close()`,
`index.html:130`) e só é chamado ao sair da view de grafo.
Motivo: handler `graph-changed` chama `renderGraph()`; fechar a fonte ali
mata o stream após o primeiro evento (self-close).
Teste: `render_graph_does_not_close_event_source` (`lib.rs:1525`) — 3 asserts
(sem `stopGraph()` em `renderGraph`, `ensureGraphEventSource()` existe e é
chamada, `stopGraph()` ainda fecha).

### RF-05 — Sem inversão de dependência (crate boundary)

`brain-web` continua folha read-only: depende só de `brain-store` + `brain-core`
(não de `brain-mcp`). Cada poll abre seu próprio `Store` (`lib.rs:309`),
usa sincronamente, dropa antes do próximo `.await` — regra `Send` não `Sync`.
`AppState` continua `{ db: String }` (`lib.rs:11`). `tokio-stream = "0.1"`
adicionado em `crates/brain-web/Cargo.toml:15`; uso em `lib.rs:7-8,301`.

### RF-06 — QA browser 5/5 em produção

Verificação funcional pós-deploy (workflow-state.json `harness_detail`
2026-10-10): store 576→577 sem reload, delete 577→576, EventSource OPEN,
console 0 erros, network só 2xx. Release rebuild + restart 8322 ok, 8321 intacto.

## Fora de escopo (não entregue, não documentar como entregue)

- Notificação por-prefetch ou por-projeto (evento é global `graph-changed`, sem payload de path).
- SQLite hooks / push real — polling 2 s por decisão (ver ADR-01).
- `ollama` em `/api/status` do viewer (rejeitado em TD-008: health 5 s sob abort 3 s mentiria).
- Retry com log em `Store::open` falho no poll — gap registrado como lição
  (`regras/global/.../sse-silent-stream-death`, `.ok()?` em `lib.rs:309`);
  stream termina em falha persistente. Não reescrever como feature.

## ADRs

### ADR-01 — Polling 2 s em vez de SQLite hooks / watch channel

`brain-web` é folha read-only e não pode depender de `brain-mcp` (dono do write
path + fila de embed). Polling é o único mecanismo sem inverter o grafo de
dependência. `tokio::sync::watch` exigiria um writer que o handler read-only não
tem — snapshot em `Arc<Mutex<(i64, i64)>>` compartilhado entre handler e stream
(`lib.rs:296-299`, doc em `lib.rs:286-292`). Intervalo 2 s: live (< 3 s) e barato
(`Store` por poll em WAL com counts indexados).

### ADR-02 — EventSource lifecycle separado do render

`ensureGraphEventSource()` idempotente (`index.html:381-386`) abre a fonte;
`renderGraph()` só garante aberta (`index.html:394`); `stopGraph()` fecha só em
troca de view (`index.html:128-131`). Alternativa rejeitada: abrir/fechar por
render — o handler do próprio evento chama `renderGraph()`, logo fechar ali é
auto-terminação após o primeiro evento. Guard de regressão no teste `lib.rs:1525`.

## Bugs encontrados e corrigidos (evidência, não requisito novo)

1. **SSE sem `event:`** — `Event::default().data(...)` só emite `data:`; evento
   sem `event:` é tipo `message` e `addEventListener('graph-changed')` nunca
   dispara. Fix: `.event("graph-changed").data("graph-changed")` (`lib.rs:315`).
   Teste original (`s.contains("graph-changed")`) passava sem o fix — corrigido
   para exigir ambos os campos (`lib.rs:1495`). Lição em
   `regras/global/sse-event-field-discriminative-test`.
2. **EventSource self-close** — `renderGraph()` chamava `stopGraph()`, fechando a
   fonte que disparou o evento. Fix: cancela só RAF, garante fonte aberta.
   Guard em `lib.rs:1525`. Comentários de aviso em `index.html:375-380,389-391`.

## Delta Specs

- `.agents/rules/frontend-rules.md` — seção Viewer: adicionar rota
  `GET /api/events` (SSE, `text/event-stream`, evento `graph-changed`,
  polling 2 s) ao contrato do viewer (`/api/status,search,read,list` +
  `/api/graph` + `/api/events`); frontend usa `EventSource` idempotente com
  lifecycle separado do RAF loop.
- Nenhuma mudança em `architecture-rules.md` (crate boundary preservada),
  `database-rules.md` (sem schema; só `count_notes/count_chunks`) ou `AGENTS.md`
  tabela de endpoints REST (adicionar linha `/api/events` se a tabela existir).
