# DO-02a — EVIDÊNCIA demo setup+doctor (scratch)

> Executado 2026-10-14, binário `target/debug/brain` rebuildado pós-DO-01
> (`include_str!` dos estáticos). Roteiro literal de `hooks/README.md`.
> Log integral: `/tmp/do02a.log` (+ overflow `/tmp/do02a_compact.mjs`).

## H3 / produção intacta

- `LSOF_BEFORE=[1380 1381 10906 456874]` = `LSOF_AFTER=[1380 1381 10906 456874]`
  (portas 8321/8322, mesmos PIDs, antes e depois).
- `BRAIN_OLLAMA_URL=http://127.0.0.1:1` (morto proposital), `BRAIN_DB_PATH`,
  `HOME`, `BRAIN_DIR` todos apontando p/ `/tmp/do02a-scratch.ehe7pi`.
- Nenhum servidor subido; nenhum restart/kill; nenhum write fora do scratch
  (spool `/run/user/.../hook-spool.jsonl` só recebe as 2 linhas do demo).

## 1. `setup kiro --dir $PROJ --yes` → 4 arquivos

```
kiro: created $PROJ/.kiro/hooks/brain-session.json
kiro: created $PROJ/.kiro/hooks/brain-learning.json
kiro: created $PROJ/.kiro/hooks/brain-prompt-optimizer.json
kiro: created $PROJ/.kiro/steering/brain-learning.md
```

Fidelidade instalado==fonte: `learning byte-fiel`, `optimizer byte-fiel`
(inclui a ponte DO-01 sem `disclose_context`), `steering byte-fiel`,
`session funcionalmente fiel (triggers: ['UserPromptSubmit', 'Stop'])`.

## 2. `setup opencode --dir $PROJ --yes` (HOME scratch + BRAIN_DIR=repo)

```
opencode: created $HOME/.config/opencode/mcp.json
opencode: created plugin $HOME/.config/opencode/plugins/brain-session.js
project: created $PROJ/opencode.json
```

## 3. `doctor --dir $PROJ` → D1–D6 ok, exit 0 (2 estados)

Com DB fresco (criado pelo setup) e com DB pós-hook: ambos

```
D1 ok — 3 hooks kiro presentes, version=v1, triggers válidos
D2 ok — steering presente, sem hardcoded/absoluto
D3 ok — plugin em $HOME/.config/opencode/plugins/brain-session.js
D4 ok — banco legível em $SCRATCH/brain.db
D5 ok — BRAIN_DB_PATH absoluto e gravável
D6 ok — skill brain referenciada na config
doctor: verde  (exit 0)
```

D4-warn puro (DB inexistente, pai existente): `D4 warn — banco ainda não
existe …; D5 ok; doctor: verde (exit 0)`. Warn nunca falha (I4/I6).

## 4. Hook no scratch (captura fecha o loop)

`hook --event session-start/end --project do02a-demo` → nota
`sessoes/do02a-demo/2026-10-14` com os 2 eventos, visível em `recent`
(Ollama morto: chunks NULL, `awaiting backfill`, exit 0 — nunca erro).

## 5. R1: trigger divergente → log + fail exato (BDD failure)

Trigger `Stop`→`OnFrogJump` no instalado:

```
D1 fail — $PROJ/.kiro/hooks/brain-learning.json trigger `OnFrogJump` fora da lista fechada
  como corrigir: rode `brain setup kiro --dir $PROJ` (--force restaura)
doctor: FAIL … (exit 1)
```

Restaurado → `doctor: verde (exit 0)`. Matcher amplo + caminho exato: mitigação
R1 observável.

## 6. Overflow real do compacting (debt OC-02, `node /tmp/do02a_compact.mjs`)

Handler `experimental.session.compacting` do plugin real com resumo de
20000 chars (5× o teto):

```
context_len_antes=1 depois=2
anexo_chars=4000 (teto 4000)
anexo_tem_marker=true
prompt_intocado=true
join_runtime_contem_marker=true   (prompt ?? [default, ...context].join)
join_runtime_preserva_default=true
failopen_context_intacto=true     (binário inexistente → sem mutação)
failopen_sem_throw=true
DO-02a-compact: PASS
```

Risco residual do OC-02 fechado no que é observável sem custo de API:
anexo entra truncado, default preservado, falha degrada.

## Veredito DO-02a

BDD (1) scratch kiro+opencode → doctor verde ✅ · (2) trigger diverge →
fail exato + restore ✅ · (3) prod intocada (lsof igual) ✅ · overflow
compacting exercitado ✅ · DoD: doctor verde + prod intacta ✅.
