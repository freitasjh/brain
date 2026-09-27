# Database Rules — SQLite WAL

## Schema
`SCHEMA_VERSION = 4` (`crates/brain-store/src/lib.rs:10`). DDL completo em um único
`execute_batch` dentro de transação `IMMEDIATE` (`lib.rs:815-909`):

- `notes(id PK, path UNIQUE, layer, scope, content, project_id FK SET NULL, tags JSON, pinned BOOL, expires_at TEXT, version INT, created_at, updated_at)` `idx_notes_layer/scope/expires/project` (`lib.rs:822-839`)
- `chunks(id PK, note_id FK CASCADE, path, layer, scope, snippet, chunk_index, total_chunks, project_id, tags JSON, embedding BLOB, UNIQUE(path, chunk_index))` `idx_chunks_path/layer` (`lib.rs:841-856`)
- `notes_fts(title, body)` — **virtual table** FTS5 `tokenize='porter unicode61'` + triggers `notes_fts_insert/delete/update` (`lib.rs:858-871`)
- `projects(id PK, name UNIQUE, description, created_at)` (`lib.rs:815-820`)
- `links(id PK, from_path, to_path)` wikilink `[[ ]]` (`lib.rs:873-878`)
- `entities(name UNIQUE, normalized)` + `entity_links(entity_id, note_id)` (`lib.rs:880-889`)
- `note_projects(note_path, project_id)` — liga nota a projeto **sem** ser dona dela (`lib.rs:891-897`)
- `audit_log(id, action, path, prev_content, at)` checkpoints (`lib.rs:899-905`)
- `_meta(key PK, value)` `version=4, embedding_dim=768` (`lib.rs:907-909`)

> A descrição anterior dizia "plain table" para o `notes_fts`. É
> `CREATE VIRTUAL TABLE ... USING fts5` (`lib.rs:858`) — a diferença importa porque
> `rowid` e as regras de DELETE são as do FTS5, não de uma tabela comum.

## `note_projects` — a tabela que não estava listada

`notes.project_id` é **posse**: a nota *pertence* ao projeto. `note_projects` é
**link many-to-many** e é o que `brain_project_link` escreve (`lib.rs:975`) e
`brain_project_unlink` remove (`lib.rs:981`); `brain_project_notes` faz `JOIN` nas
duas (`lib.rs:956`). Sem essa tabela, um projeto só via `notes.project_id` não acharia
as notas ligadas, e o filtro `project` do `search` cairia para owned-only
(`lib.rs:1795-1801`).

## WAL
- `PRAGMA journal_mode=WAL` com retry (`lib.rs:690-709`);
  `PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL` (`lib.rs:790`)
- `Store {conn}` é dono único do DB. `init_schema` roda o DDL **só** quando
  `_meta.version` não bate — `schema_is_current` é uma **leitura** (`lib.rs:712-730`).  Sem isso, todo `Store::open` — ou seja, todo request MCP, toda fase da fila e duas
  vezes por evento de hook — tomava o write lock, e um `brain status` read-only perdia
  por `SQLITE_BUSY`.

## `NULL` e zero-vector são estados diferentes, e o `coverage_pct` precisa distinguir

`embedding_coverage` (`lib.rs:1548-1562`) reporta **quatro** números, não dois:

| Número | Query | Significado |
|---|---|---|
| `embedded` | BLOB presente, largura 768, não-zeroblob | indexado |
| `without_embedding` | `embedding IS NULL` | **na fila** — vai chegar |
| `zero_vector` | `= zeroblob(3072)` e `length = 3072` | **armazenado e inútil** |
| `coverage_pct` | `embedded / total` | a cobertura semântica real |

`coverage_pct` que não sobe depois de um `brain_store` = fila travada ou Ollama fora
(`without_embedding`). Um `zero_vector` subindo é o outro problema, e é o que o
`chunk_insert` recusa na fronteira (`lib.rs:1213-1215`). Confundir os dois é o que
deixou 99.6% do índice inútil com a contagem `NOT NULL` saudável (`lib.rs:175-177`).

## Reindex preserva por texto normalizado, e o default é 1.0

O match de reuso é `(path, chunk_index)` + `normalize_for_match` (casefold + colapso
de whitespace, `lib.rs:400-411`) contra `DEFAULT_REUSE_SIMILARITY = 1.0`
(`lib.rs:315`). O motivo do default ser **exatamente** 1.0: reciclar
`DEVE validar` → `NÃO DEVE validar` custa **um token** e pontua 0.96, acima de
qualquer threshold que valha a pena; um vetor reusado nesse caso responde o **oposto**
do que a nota diz, que é estritamente pior que `NULL` (`lib.rs:300-303`).

`BRAIN_REUSE_SIMILARITY=0.9` é opt-in explícito, resolvido uma vez por
`Store::open` (`lib.rs:472-495`). O guard de negação (`lib.rs:547`) continua valendo
em qualquer valor — é verificado **antes** do threshold, deliberadamente.

> Honestidade sobre o corpus real: `links` tem **0 linhas** em produção. O stream de
> grafo do RRF existe, tem query (`lib.rs:1742-1744`) e é testado, mas **nunca foi
> exercitado**. O stream de entidade tem 243 linhas e 556 `entity_links`, então ele
> **é** exercitado. Tratar os dois como igualmente provados seria mentira.

## Migrations
- Sem Flyway. `SCHEMA_VERSION` bump + `init_schema` (ver `brain-store/src/lib.rs:10`),
  transação `IMMEDIATE` porque `DEFERRED` pega o write lock preguiçosamente e o SQLite
  devolve `SQLITE_BUSY` **sem consultar o busy handler** nessa upgrade (`lib.rs:800-812`)
- `table_names()` (`lib.rs:760`) existe porque a linha de versão é uma string que um
  DDL escreve: afirmar só nela passaria num banco onde o batch escreveu a linha e
  depois falhou ao criar as tabelas

## No MySQL agents/tools tables — archived
