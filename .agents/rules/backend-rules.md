# Backend Rules — Rust

## Code Style
- `brain-core` no IO: pure types + validate + sanitize + chunk
- `brain-store` single `Store {conn}` WAL owns DB; `init_schema` SCHEMA_VERSION, triggers FTS
- `brain-embed` rustls, `chunk_text` split by `## `, `embed_batch_concurrent(4)`
- `brain-mcp` rmcp tools + `sanitize_relative_path`; handlers thin, delegate to Store
- `brain-web` axum `AppState{db:String}` open per request (avoid !Sync), read-only
- `brain-cli` clap derive, `BRAIN_DB_PATH` env, `full_path` validate scope

> As fronteiras entre crates e a regra de `Send`/`Sync` estão em
> `.agents/rules/architecture-rules.md`. Este arquivo é sobre **as decisões que a onda
> provou ser erradas quando escritas ao contrário**.

## Ausência se modela como ausência, nunca como sentinela

`chunk_insert` aceita `embedding: Option<&[f32]>` (`brain-store:1205`).
- `None` grava SQL `NULL` — o chunk fica **pesquisável por FTS imediatamente** e
  aparece em `coverage.without_embedding`, que é o sinal de "a fila ainda vai
  resolver isso".
- `Some(v)` grava BLOB, e **recusa** duas coisas na fronteira: dimensão ≠ 768
  (`:1210-1212`) e norma zero (`:1213-1215`).

> 🔴 **Um BLOB de zeros é pior que `NULL`.** `cosine` dá `0.0` para norma zero, então
> todo chunk zero faz score `0.0` contra toda query **e ainda assim consome slot no
> orçamento `truncate(50)`** (`lib.rs:1196-1200`), expulsando notas relevantes. Foi
> exatamente assim que 735 de 738 chunks do índice de produção viraram peso morto
> enquanto `brain status` mostrava contagem saudável (`lib.rs:175-177`).

**Nunca** escreva `vec![0.0; 768]` como fallback. FTS-only não precisa de vetor
nenhum — o fallback de `brain_search` é o stream textual do RRF, não um zero.

## Guard de regressão por **varredura de fonte**

`crates/brain-store/tests/zero_vector_guard.rs` lê o fonte de todo `crates/*/src/` e
falha se um literal de vetor zero reaparecer em código de produção. É um **source scan**,
não um teste comportamental, de propósito: um teste comportamental só afirma sobre os
caminhos que ele exercita; a varredura cobre todos os arquivos, inclusive os call
sites que nenhum teste toca — que foi a forma do bug original (7 call sites em
store/CLI/MCP).

**Ao adicionar um guard, prefira a varredura de fonte** quando o defeito é uma
*forma de código* proibida. Ela é o que "provou a perfuração" aqui.

## Policy em campo do `Store`, nunca em call site

`ReusePolicy` é um **campo** de `Store` (`lib.rs:581`), resolvido uma vez por
`Store::open` a partir de `BRAIN_REUSE_SIMILARITY` (`lib.rs:472-495`) — não uma
função livre com default em cada call site.

Motivo concreto: `chunks_sync` e `chunks_needing_embedding` já divergiram por
exatamente essa razão. A fila tratava `Similar` como "já tem vetor" e nunca
re-embedava, enquanto `chunks_sync` continuava reusando o stale — o erro se
sustentava sozinho e só um `reindex --all` manual limpava (`lib.rs:447-451`).
Colocar a política no campo tornou a divergência **estruturalmente impossível**,
porque as duas leem o mesmo objeto.

O mesmo vale para `ReusePolicy::EXACT`, que deriva de `DEFAULT_REUSE_SIMILARITY` em vez
de restatar `1.0` (`lib.rs:464`): uma constante que só aparece em comentário não é
constante que alguém testa.

## Reindex é upsert + prune, nunca `DELETE` + reinserção

`reindex_all_with` (`lib.rs:2000`) reconcilia por `INSERT OR REPLACE` em
`(path, chunk_index)` e **prune** do que sobrou (`lib.rs:2007-2013`). A implementação
anterior rodava `DELETE FROM chunks` e reinseria tudo com vetor zero — o que fazia
`brain reindex --all` **destruir o índice vetorial inteiro** silenciosamente
(`lib.rs:1977-1982`). Os três vetores sobreviventes no banco de produção sobreviveram
só porque ninguém tinha rodado o comando.

`chunk_insert` usa `OR REPLACE` porque toda linha de chunk é chaveada em
`(path, chunk_index)` e todo rebuild reinsere a mesma chave (`lib.rs:1219-1222`).

## Doc que descreve premissa: verifique contra o código

Duas regras que vieram da mesma reprovação:

1. **Meça antes de documentar.** `MAX_CONTENT_BYTES` é 256 KiB dimensionado por
   `PESSIMISTIC_SECONDS_PER_CHUNK = 400 ms` (~9x o ~0.045 s/chunk medido), **não** pelo
   número mais rápido observado (`brain-core:38-52`). Um teto dimensionado pelo melhor
   caso já visto deixa de ser teto num cold start.
2. **Se a medição refutar a doc, corrija a doc — não o comportamento.** O guard de
   negação tinha um doc que afirmava prevenir "um vetor que responde o oposto da nota".
   Era falso: `proibido` estava na lista de obrigação, então inverter uma obrigação
   numa proibição dava `+1` dos dois lados e o guard não via mudança nenhuma no eixo
   que existe para detectar (`lib.rs:518-534`). A review mediu; a correção foi no
   **texto** e a afirmação foi reduzida ao que o código faz: um heurístico de contagem
   de tokens sobre lista fixa, não entendimento de significado.

Se você não consegue apontar o código, a afirmação não entra.

## Testing

### Obrigatório — Todo desenvolvimento DEVE incluir testes

### Unit (`cargo test -p brain-core|brain-store`)
- `brain-core`: validate_layer/scope, sanitize traversal, frontmatter, chunk ##, wikilink,
  `validate_content_limits` nos dois tetos e **no limite exato** (aceita) e acima (recusa)
- `brain-store`: in-mem `Store::open_in_memory()` CRUD notes/chunks, FTS insert/delete,
  search RRF, TTL `forget_sweep`, audit `checkpoints/restore`, project CRUD,
  `cargo test -- --nocapture`
- **Mock Ollama, sempre.** `embed` nunca pode exigir Ollama real; e o destino do mock
  num teste de CLI tem que ser morto ou efêmero — ver H2.3 do `harness-continuous.md`.
  `env_remove("BRAIN_OLLAMA_URL")` seleciona o default real de produção.

### Integration (`cargo test --workspace`)
- `store → search → read → delete → export → backup → sweep` end-to-end
- FTS + vector + entity + graph RRF k60 + authority boost
- `cargo build --workspace` zero warnings, `cargo clippy --workspace --all-targets -- -D warnings`

### Teste que prova uma garantia
**Mutação, não suposição.** O padrão que a review aprovou: aplicar a mutação e ver o
teste falhar. Dois exemplos no repo:
- redaction — 3 mutações, 3-4 falhas cada; o commit registra os números
- zero vector — source scan + `chunk_insert` recusando

Um teste que passa porque ninguém tentou quebrá-lo não está provando nada. Se você não
souber dizer **qual mutação do código faz seu teste falhar**, o teste não está preso ao
comportamento que você quer.

### Cobertura
- Mín 70% `cargo llvm-cov --workspace --html`
- `cargo test --workspace` DEVE passar antes de commit

## Exact Commands
```bash
cargo test -p brain-core -- --nocapture
cargo test --workspace
cargo build --workspace   # release: --release (LTO thin)
cargo clippy --workspace --all-targets -- -D warnings
cargo llvm-cov --workspace --html
```
