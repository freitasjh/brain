---
name: brain_reindex
description: >
  Rebuild the Brain vector index. Use when embedding coverage in brain_status is
  below 100%, after a bulk import, or after text was changed outside the server.
---

# brain_reindex

Reconstrói chunks + embeddings do cérebro.

> **Não é uma tool MCP.** É o subcomando de CLI `brain reindex --all
> [--no-embed]` (`brain-cli:84`). Não chame `brain_reindex(...)` como tool: o
> servidor MCP não a expõe, e `tools/` aqui tem uma pasta por tool MCP — por isso
> esta skill mora em `cli/`, não em `tools/`.
>
> Para o que **é** tool MCP e tem skill própria em `tools/`, veja
> `brain_status` (é ele que diz se o reindex é o remédio) e `brain_store` (que
> mantém os vetores em dia sozinho, sem reindex).

## Quando usar

- `brain_status.embedding.coverage.embedding_coverage_pct` < 100 com Ollama no ar
- Depois de importar um vault (`brain migrate`)
- Depois de editar texto em massa **fora** do servidor
- `chunks_zero_vector > 0` no `brain_status` (estado corrompido herdado)

**Não** use para "acelerar a busca": um comando de escrita normal já mantém os
vetores em dia, e a fila de background reembede o que falta sozinho.

## Como usar

```bash
brain reindex --all              # reindexa + embeda o que falta
brain reindex --all --no-embed   # só a parte estrutural; novos chunks ficam NULL
```

## Comportamento

- **Foreground.** A passagem de embed de um corpo grande leva minutos (Ollama
  serve um embed por vez, ~0.045 s/chunk). Planeje o tempo.
- **Não destrutivo.** Nunca `DELETE FROM chunks`. Um chunk cujo texto continua
  igual **mantém** o vetor; texto reescrito volta como `NULL`, nunca como vetor
  de outro texto. É por isso que rodar o comando não destrói mais o índice.
- **Ordem:** snapshot do corpus → passagem de embed → transação de escrita. Cada
  vetor carrega o trecho de texto de onde veio, então uma nota editada durante a
  passagem é detectada (`diverged`) em vez de receber o vetor do texto antigo.
- **Lock:** se outro embed estiver rodando (a fila do servidor), o comando falha
  sem escrever nada. Rode de novo depois.
- `--no-embed` não faz rede: útil offline, para só realinhar chunks com o texto.

## Retornos

```
REINDEX_DONE notes=253 chunks=821 embedded=821 preserved=814 rehydrated=7 \
             null=0 diverged=0 stale_reused=0 unmatched=0
REINDEX_PARTIAL 12 chunk(s) have no vector — rerun without --no-embed while Ollama is reachable
REINDEX_DIVERGED 2 chunk vector(s) were computed from text the note no longer has …
REINDEX_STALE 3 chunk(s) kept a vector that is slightly out of date …
```

| Token | Significado | Ação |
|-------|-------------|-------|
| `preserved` | vetores que sobreviveram do run anterior | — |
| `rehydrated` | vetores recuperados neste run | — |
| `null` | chunks sem vetor | rode de novo com Ollama no ar |
| `diverged` | vetor de texto que a nota já não tem, **não** aplicado | rode de novo |
| `stale_reused` | vetor mantido por similaridade. **Só é alcançável com opt-in explícito**: o default é `1.0` (igualdade exata), então precisa de `BRAIN_REUSE_SIMILARITY=0.9` | opcional: `reindex` para refrescar |
| `unmatched` | vetor perdido: texto mudou demais | rode de novo |

`null` e `diverged` **sempre** visíveis: um relatório que só diz "done" é como um
índice meio reconstruído pareceu saudável por um mês.
