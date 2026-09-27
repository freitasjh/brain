---
name: backend-skill
description: >
  Escrever código backend no brain — workspace Rust de 6 crates, SQLite-only
  (WAL + FTS5 + vetor 768), fila de embedding, allowlist de escrita em disco.
  Carregue antes de criar ou alterar handler, tool, query, handler de streaming
  ou guarda de segurança neste repositório.
license: MIT
compatibility: opencode
metadata:
  stack: rust-sqlite
  lang: rust
---

# Backend do brain — como escrever código aqui

Este repositório é um **workspace Rust de 6 crates** com SQLite como única
persistência. Não há ORM, não há migration em SQL versionado, e não há camada de
rede fora de uma crate só. A maior parte das decisões que parecem arbitrárias já
foram medidas; as regras abaixo citam o código que as aplica.

## As 6 crates, e o que cada uma pode e não pode fazer

Direção de dependência, lida de cada `Cargo.toml`:

```
brain-core   → nada
brain-store  → brain-core
brain-embed  → brain-core
brain-mcp    → brain-core, brain-store, brain-embed
brain-web    → brain-store, brain-core        (folha read-only)
brain-cli    → core, store, embed, web, mcp
```

| Crate | Pode | Não pode |
|---|---|---|
| `brain-core` | types, `validate_*`, `sanitize_relative_path`, `chunk_text`, wikilink | **nenhum IO** |
| `brain-store` | SQL, schema, FTS5, RRF, auditoria | **não conhece rede** |
| `brain-embed` | HTTP para o Ollama, batch concorrente | não escreve no banco |
| `brain-mcp` | tools rmcp, SSE, fila de embedding, `fs_guard` | — |
| `brain-web` | `GET /api/status,search,read,list` | **não embede**; não depende de `brain-mcp` |
| `brain-cli` | binário `brain`, 15+ subcomandos | — |

> `brain-web` é folha de propósito: como não depende de `brain-mcp`, ela não alcança
> `embed_chunks`/`sync_note_chunks` (`brain-mcp/src/lib.rs:31,97`). O viewer **lê** o
> que a fila já escreveu; ele não produz embedding.

## A regra de `Send` que decide o layout dos handlers

`Store` embrulha `rusqlite::Connection`, que é `Send` mas **não** `Sync`. Logo `&Store`
não é `Send`:

> **`Store` pode ser *mantido* através de um `.await`, nunca *emprestado*.** Cada fase
> abre o seu próprio `Store`, usa sincronamente e **dropa antes do próximo `.await`**.

Isso não é estilo — é o compilador recusando spawnar o handler, com **erro de tipo**
(`brain-store/src/lib.rs:584-595`). O guard `assert_send::<Store>()` é o tripwire
contra um campo novo `!Send`.

Na prática: `AppState` guarda `db: String`, **não** um `Store`
(`brain-mcp/src/lib.rs:254-256`). Não introduza um `Store` compartilhado no estado.

## Ausência se modela como ausência

`chunk_insert` recebe `embedding: Option<&[f32]>` (`brain-store:1205`):

- `None` → grava SQL `NULL`. O chunk já é pesquisável por FTS e aparece em
  `coverage.without_embedding` como "na fila".
- `Some(v)` → grava BLOB, e **recusa** dimensão ≠ 768 (`:1210-1212`) e norma zero
  (`:1213-1215`).

> 🔴 **Nunca escreva `vec![0.0; 768]`.** `cosine` devolve `0.0` para norma zero: o chunk
> faz score `0.0` contra toda query **e ainda assim consome slot no orçamento
> `truncate(50)`** (`lib.rs:1196-1200`), expulsando notas relevantes. Foi assim que
> 735 de 738 chunks do índice de produção viraram peso morto com `brain status`
> mostrando contagem saudável (`lib.rs:175-177`).

O fallback de `brain_search` quando o Ollama está fora é o **stream textual do RRF**,
não um vetor zero.

`crates/brain-store/tests/zero_vector_guard.rs` faz varredura de fonte de todo
`crates/*/src/` e falha se um literal de vetor zero reaparecer. Ele existe porque
um teste comportamental só afirma sobre os caminhos que exercita — e o bug original
apareceu em 7 call sites que nenhum teste tocava.

## A fila: uma escrita não espera por vetor

O contrato, em ordem:

1. `validate_note_write` valida escopo/limites — **antes** de abrir `Store` ou tocar
   a rede (`brain-core:158`)
2. `chunks_sync` grava os chunks com `embedding = NULL`, e o FTS5 já está populado
   na **mesma** transação
3. `store_note_and_queue` enfileira e responde, com `queued=N` no retorno
   (`brain-mcp/src/lib.rs:211`)
4. worker em background embeda e escreve de volta

`queued>0` **não** é bug — é o contrato. `coverage_pct` que não sobe depois de um
`brain_store` é o sinal de fila travada.

Falha de embed **reenfileira** com backoff; só depois de
`BRAIN_EMBED_MAX_FAILURES` (default 8, `embed_queue.rs:111`) a nota vai para
**dead-letter**: sai da fila, `brain status` reporta `queue.dead_lettered`, e os
chunks ficam `NULL`. Isso não é perda — `NULL` é o registro que `recover`
(`embed_queue.rs:476`) lê no próximo boot e que `reindex --all` re-embera.
`finish_owed` (`embed_queue.rs:895`) é o funil **único** dessa decisão, então
re-queue e dead-letter não podem divergir.

O **CLI embeda inline** (`main.rs:653`), não enfileira: é um processo one-shot e a
task de background morreria com ele.

## Limites de escrita

`MAX_CONTENT_BYTES = 256 KiB` e `MAX_CHUNKS = 64` (`brain-core:61,71`), validados por
`validate_content_limits` (`brain-core:158`) nos **3** paths de escrita — MCP rmcp,
REST axum e CLI. O embedding é serial (`OLLAMA_NUM_PARALLEL=1`), então sem teto o
custo da escrita escala com o número de chunks.

Os dois tetos não se substituem: 200 KiB com `## ` a cada 100 bytes são 2.000
chunks e passa pelo teto de bytes.

## Escrita em disco: só dentro da allowlist

`brain_export` e `brain_backup` só escrevem dentro de `BRAIN_EXPORT_ROOT`
(default `/tmp/brain-export`) — `fs_guard.rs:47,50`. A defesa tem duas partes:

1. **Contenção canônica** (`resolve_within`, `fs_guard.rs:296`): canonicaliza o
   ancestral mais profundo que exista e testa contenção. `..` e symlink não passam.
2. **Ownership** (`assert_root_usable`, `fs_guard.rs:199`): a raiz precisa ser
   diretório, do nosso `euid`, e não gravável por `other` — senão outro usuário
   local que criou `/tmp/brain-export` antes do primeiro start lê o corpus.

O path de **cada nota** é rechecado na escrita (`note_file_within`,
`fs_guard.rs:383`), não só o diretório destino.

> Ao adicionar uma escrita em disco: passe por `export_dir`/`backup_file`, nunca
> interpole um path de usuário direto em `fs::write`.

## Credencial nunca em log

`BRAIN_OLLAMA_URL` aceita `http://user:pass@host`. `redacted_error_message`
(`brain-embed:266`) é o **wrapper único** para transformar um `reqwest::Error` em
texto seguro, e ele contém a única chamada a `redact_reqwest_error` no crate. O
caminho perigoso é indireto: o `Display` do reqwest concatena a URL do erro, que num
redirect vem do header `Location` cru.

Teste de redaction afirma **ausência da senha e presença do marcador**, não a forma
da mensagem. Assert de string é o que deixou dois holes passarem.

## Como escrever um teste que prova uma garantia

**Mutação, não suposição.** Depois de escrever o teste, aplique a mutação que
introduziria o bug e confirme que ele falha. Se você não sabe qual mutação prende o
seu teste, ele não está preso ao comportamento que você quer. Dois exemplos no repo:
redaction (3 mutações, 3-4 falhas cada) e zero-vector (varredura de fonte +
`chunk_insert` recusando).

Regras do harness que se aplicam a teste neste repo:

- `BRAIN_OLLAMA_URL` **apontada para um destino morto** (`http://127.0.0.1:1`) ou
  mock em porta efêmera. `env_remove` **não** desabilita o embed — seleciona o
  default real (`brain-embed:23`), compartilhado por todo binário concorrente.
- Banco em scratch, portas efêmeras. **Nunca** `data/brain.db` nem 8321/8322 — há
  servidor no ar.
- Prontidão = anúncio pós-bind + vivo + connect OK, não sleep.
- Detalhes: `.agents/rules/harness-continuous.md`.

Portões antes de commitar:
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
.venv/bin/python -m pytest src/tests/ -q
```
As três. A última coleta 126 testes que **nenhum** `cargo test` executa.

## Comandos
```bash
cargo test --workspace
cargo test -p brain-core -- --nocapture
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo llvm-cov --workspace --html          # ≥70%
cargo run -p brain-cli -- --db ./data/brain.db status
```
