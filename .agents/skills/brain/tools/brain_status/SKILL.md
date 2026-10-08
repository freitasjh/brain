---
name: brain_status
description: >
  Counts of notes, chunks and projects, plus embedding coverage, the background
  queue state and Ollama health. Call this when a semantic search returns nothing
  — embedding_coverage_pct and queue.dead_lettered are how a stuck queue announces
  itself.
---

# brain_status

Contagens do banco + saúde do índice de embeddings + estado da fila de background.

Sem parâmetros. Não escreve nada.

## Por que existe

`notes` e `chunks` contados sozinho **não dizem se a busca semântica funciona**.
Um índice pode estar com a contagem perfeita e nenhum vetor utilizável — foi
exatamente assim que 735 de 738 chunks viraram BLOB de zeros enquanto o contador
disse "saudável". Os blocos `embedding.coverage` e `queue` são o que torna isso
visível.

## Retorno

```json
{
  "notes": 253, "chunks": 821, "projects": 6,
  "embedding": {
    "coverage": {
      "chunks_total": 821, "chunks_embedded": 821,
      "chunks_without_embedding": 0, "chunks_zero_vector": 0,
      "embedding_coverage_pct": 100.0
    },
    "ollama": { "reachable": true, "model": "nomic-embed-text" }
  },
  "queue": {
    "pending_len": 0, "ready_len": 0, "is_draining": false,
    "dead_lettered": 0, "max_failures": 8,
    "last_drain": {
      "embedded": 8, "nulls": 0, "retriable_nulls": 0, "requeued": 0,
      "dead_lettered": 0, "skipped_locked": 0, "failed": 0,
      "pending_left": 0, "settled": true
    },
    "embed_lock_holder": null, "embed_lock_age_s": null, "embed_lock_expires_in_s": null
  }
}
```

### `embedding.coverage` — o índice semântico

| Campo | Significado |
|-------|-------------|
| `chunks_embedded` | vetor válido (BLOB presente, largura 768, não-zeroblob) |
| `chunks_without_embedding` | `NULL` — **na fila**, o vetor ainda vai chegar |
| `chunks_zero_vector` | BLOB de zeros **armazenado** — inútil, estado corrompido |
| `embedding_coverage_pct` | `embedded / total` — a cobertura real |

`without_embedding` e `zero_vector` são estados **diferentes** e a diferença é o
diagnóstico: `without_embedding` alto = fila; `zero_vector` alto = índice
corrompido, que só `brain reindex --all` conserta.

### `queue` — por que a fila travou

| Campo | Diagnóstico |
|-------|-------------|
| `pending_len` / `ready_len` | dívida em aberto vs. aguardando backoff. Os dois juntos: uma fila adiada e uma fila vazia **parecem iguais** sem o par |
| `is_draining` | dreno travado; enquanto está `true`, nenhum `brain_store` novo consegue puxar worker |
| `dead_lettered` | a fila **desistiu** desta nota. Terminal no processo atual |
| `max_failures` | teto de tentativas (`BRAIN_EMBED_MAX_FAILURES`, default 8) |
| `last_drain` | o que aconteceu na última passagem — `null` se nunca houve |
| `embed_lock_holder` / `_age_s` / `_expires_in_s` | outro embed rodando, e há quanto tempo |

`dead_lettered > 0` é o estado que **precisa** de ação: o chunk fica `NULL` e o
`coverage_pct` não sobe sozinho. O que resolve é o `recover` do próximo boot do
servidor, ou `brain reindex --all` — o `NULL` é exatamente o registro que os dois
leem. **Não** é perda de dado.

## O que NÃO fazer

- **Não leia `notes`/`chunks` como "o índice está ok".** Cobertura é
  `embedding.coverage.embedding_coverage_pct`.
- **Não conclua que `coverage_pct < 100` é bug.** Logo depois de um `brain_store`
  é normal: a escrita não espera vetor. Espere a fila (`pending_len` cair) e
  releia.
- **Não reindexe para resolver fila parada.** `brain reindex --all` recusa se
  outro embed segura o lock — e se `is_draining` está travado, o problema é a
  fila, não o índice.
- **Não faça polling agressivo.** Cada chamada abre o banco e faz um health check
  do Ollama. Duas leituras com alguns segundos de intervalo bastam.
