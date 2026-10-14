# OC-02 — Spike compacting: decisão GO (condicional, append-only)

> Timebox 2h non-negotiable. Veredito: **GO** para o handler
> `experimental.session.compacting` append-only (já embarcado no plugin
> OC-01 — zero código morto em qualquer desfecho). E2E com overflow real
> deferido para a demo DO-02a (scratch). Sem novo código neste ticket.

## Pergunta do spike

O resumo do brain entra no prompt de compactação? E contexto gigante é
truncado em vez de explodir o prompt?

## Evidência (runtime instalado opencode 1.18.31)

Call site em `SessionCompaction.process` (binário
`~/.local/lib/node_modules/opencode-ai/bin/opencode.exe`):

```js
Ve = yield* i.trigger("experimental.session.compacting",
  { sessionID: w.sessionID }, { context: [], prompt: void 0 })
// ...
to = Ve.prompt ?? [Aa({ previousSummary: D, context: [ze] }), ...Ve.context]
  .filter(Boolean).join("\n\n")
```

Leitura:

1. **O hook dispara** no caminho real de compactação (trigger literal —
   mesma classe de verificação do teste OC-01 T1, que lista os 16).
2. **Anexar a `output.context` ENTRA** no prompt final (`...Ve.context`
   dentro do join). Resposta ao BDD (1): sim.
3. **Sobrescrever `output.prompt` APAGARIA** o resumo default do runtime
   (`??` — prompt definido vence). Por isso o plugin nunca toca `prompt`.
4. **Truncamento (BDD edge)**: o plugin corta o anexo em 4000 chars
   (`MAX_APPEND_CHARS`, mesma classe do truncamento 4000 B de
   `hooks/brain-hook.sh`). Sem teto, um handoff gigante seria joinado
   integralmente no prompt de compactação.

## Por que GO-condicional e não E2E neste ticket

Disparar `SessionCompaction.process` de verdade exige overflow de contexto
(sessão longa com modelo pago) — fora do timebox e com custo de API, e o
contrato relevante (formato de input/output + join) já está provado no
call site acima. Risco residual honesto: a ordem exata
compactação→transform e o rendering final só se observam com overflow real.

## Consequência

- Handler embarcado em `hooks/opencode/brain-session.js` (append-only,
  fail-open, coberto pelos testes OC-01 T7/T8). Nenhum arquivo de spike
  descartável — nada para apagar.
- Follow-up registrado: demo DO-02a executa sessão scratch até compactar e
  anexa a evidência (prompt de compactação contendo o marcador do brain).
- Se a demo refutar (hook não dispara / formato diverge): o handler sai do
  plugin no mesmo commit da demo — sem virar código morto silencioso.
- Degradado correspondente (BDD edge OC-01): brain sem resumo na hora de
  compactar → nada anexado, resumo default preservado.
