# Architecture Rules — brain Rust SQLite-only

> Toda afirmação técnica cita `arquivo:linha`. Sem referência, não escreva.

## Stack
- **Server**: Rust 1.85 edition 2024 + tokio full + axum 0.7 + **rmcp `=0.5.0`**
  (`crates/brain-mcp/Cargo.toml:19`) + serde
- **Storage**: SQLite-only `BRAIN_DB_PATH` (`./data/brain.db` WAL `journal_mode=WAL`
  `synchronous=NORMAL` `foreign_keys=ON`) FTS5 `porter unicode61`
  `notes_fts(title,body)`, vec BLOB `embedding` dim 768 fixo, RRF k=60
- **Embed**: Ollama `nomic-embed-text` dim 768 via `brain-embed` rustls, chunk por `## `
- **Transport**: SSE `BRAIN_PORT 8321` MCP + `BRAIN_VIEWER_PORT 8322` web `brain-web::router`
- **Vault**: REMOVED — export só dentro de `BRAIN_EXPORT_ROOT` (default `/tmp/brain-export`)

> `rmcp 0.3` foi o que este arquivo afirmava até `v0.9.1`; o pin real é `=0.5.0`
> (`crates/brain-mcp/Cargo.toml:19`). Verifique a versão antes de citá-la.

## Crate Boundaries

Direção de dependência, lida de cada `Cargo.toml` `[dependencies]`:

```
brain-core   → (nenhuma brain-*)
brain-store  → brain-core
brain-embed  → brain-core
brain-mcp    → brain-core, brain-store, brain-embed
brain-web    → brain-store, brain-core          (folha read-only)
brain-cli    → brain-core, brain-store, brain-embed, brain-web, brain-mcp
```

- `brain-core` — types `Note/Project/SearchResult`, `validate_layer`/`scope`/`sanitize`,
  chunk, wikilink `[[ ]]`, frontmatter, **nenhum IO**
- `brain-store` — `Store {conn}` WAL, `init_schema` `SCHEMA_VERSION` 4, tabelas
  `projects/notes/chunks/notes_fts/links/entities/entity_links/note_projects/audit_log/_meta`,
  triggers FTS, `search` híbrido RRF + authority. **Não conhece rede.**
- `brain-embed` — `EmbeddingEngine` health_check + embed + batch concurrent 4.
  **A única crate que faz HTTP.**
- `brain-mcp` — rmcp tool registry (17 tools), `AppState{db, queue}`,
  `embed_queue`, `fs_guard`, e `embed_chunks`/`sync_note_chunks`
- `brain-web` — axum read-only `/api/status,search,read,list`. **Folha:** não depende de
  `brain-mcp`, e por isso não alcança `embed_chunks`/`sync_note_chunks` — o viewer não
  embede, ele lê o que a fila já escreveu.
- `brain-cli` — clap; consome `brain-mcp` (portanto o CLI **depende** do MCP, não o
  contrário — `crates/brain-cli/Cargo.toml:23`)
- `src/brain_server/` — legado Python, compat até a Fase C

## A regra que sustenta o layout: `Store` é `Send`, não `Sync`

`Store` embrulha `rusqlite::Connection`, que é `Send` mas **não** `Sync`. Logo `&Store`
não é `Send`, e um handler que o mantivesse vivo através de uma chamada de rede deixaria
de ser `Send` — axum e rmcp recusam spawná-lo, com **erro de tipo**, não surpresa em
runtime. Por isso vale a regra, e ela é imposta pelo compilador:

> **`Store` pode ser *mantido* através de um `.await`, nunca *emprestado*.** Cada fase
> abre o seu próprio `Store`, usa sincronamente e **dropa antes do próximo `.await`**.

O tripwire contra um campo novo `!Send` é `assert_send::<Store>()`
(`crates/brain-store/src/lib.rs:594-595`). Se um campo compartilhado aparecer, isso não
compila — que é o ponto.

Consequência prática: `AppState` guarda `db: String`, **não** um `Store`
(`crates/brain-mcp/src/lib.rs:254-256`); o viewer abre por request pelo mesmo motivo.

## Key Decisions
- Agents are NOT persisted separately — projects are `projects` table, notes are SQLite
  `notes.path = layer/scope/path`
- Tools are rmcp `#[tool]` + axum handlers, sanitize via `brain-core::sanitize_relative_path`
- Scope mandatory for `arquitetura`/`regras`/`estudos` (`projetos|global`), validated
  before DB (`brain-core:15`)
- Schema migrations via `SCHEMA_VERSION` bump + `init_schema`, not Flyway
- TTL `expires_at` beats `pinned`
- `BRAIN_VAULT_PATH` **não é mais lido** — o campo saiu do modelo
  (`src/brain_server/config.py:114-126`); sobrou o aviso, que é o que ainda informa o
  operador de que a variável parou de fazer alguma coisa. Não descreva como
  "deprecated field".

## Correções aplicadas em `v0.9.1`

| Afirmação antiga | Realidade | Fonte |
|---|---|---|
| `rmcp 0.3` | `=0.5.0` | `crates/brain-mcp/Cargo.toml:19` |
| `BRAIN_VAULT_PATH warn deprecated` (campo) | campo removido, resta o aviso | `src/brain_server/config.py:114-126` |
| `AppState{db}` "evita !Sync" | o compilador impõe; `assert_send` é o tripwire | `brain-store/src/lib.rs:584-595` |
| `embed_chunks`/`sync_note_chunks` sem dono | vivem em `brain-mcp`; o CLI consome o MCP | `brain-mcp/src/lib.rs:31,97`; `brain-cli/Cargo.toml:23` |
