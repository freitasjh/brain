# TD — `serve` não encerra com `GET /api/events` (SSE) aberto

**Severidade:** P2 (degrada operação; sem perda de dado)
**Descoberto:** 2026-10-19, deploy do `viewer-html-not-served` (QA)
**Origem:** pré-existente — reproduz no binário antigo e no novo

## Sintoma

`systemctl --user restart brain-viewer` leva **90,14 s** e termina em
`Main process exited, code=killed, status=9/KILL` / `Failed with result 'timeout'`.

Isolado em scratch (binário novo, porta efêmera):
- SIGTERM **sem** conexão aberta → exit em **1 ms**, rc=0 (handler OK).
- SIGTERM **com** `GET /api/events` aberto → processo vivo após 10,1 s; só morreu com `kill -9`.

## Causa raiz

`crates/brain-cli/src/main.rs:1516` — `axum::serve(...).with_graceful_shutdown(shutdown)` aguarda
conexões in-flight terminarem. `/api/events` é `text/event-stream` infinito
(`crates/brain-web/src/lib.rs`), então o `EventSource` de qualquer aba do viewer segura o
shutdown até `TimeoutStopSec=90`, quando o systemd escala para SIGKILL.

Mesma classe do F6 de `.agents/rules/harness-continuous.md`, na direção oposta: o handler existe,
mas o encerramento depende de uma conexão que por desenho nunca fecha.

## Impacto

- `restart`/`stop` sempre pagam 90 s quando há viewer aberto.
- `systemctl stop` reporta `timeout` + `SIGKILL` — parece falha de serviço.
- Sem perda de dado: viewer é read-only e o DB é WAL.

## Solução proposta

1. Cancel token compartilhado entre `axum::serve` e o handler de `/api/events`; no shutdown,
   abortar o stream SSE antes de aguardar as conexões.
2. Alternativa/simultânea: `TimeoutStopSec=` explícito na unit gerada por `brain setup systemd`
   (`crates/brain-cli/src/setup.rs`), documentando o trade-off.
3. Teste: subir router real, abrir `/api/events`, enviar SIGTERM e exigir exit < 2 s.

## Testes necessários

- Integração `crates/brain-cli/tests/serve_viewer_shutdown.rs` (já existe): caso com SSE aberto.
- Garantir que `/api/*` válidos e o viewer continuam intactos após o shutdown limpo.

## Riscos

- Abortar SSE cedo demais pode cortar clientes legítimos no meio de um evento; o evento
  `graph-changed` é idempotente (viewer reconcilia no próximo poll de 2 s).
