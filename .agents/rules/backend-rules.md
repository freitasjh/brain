# Backend Rules — Rust

## Code Style
- `brain-core` no IO: pure types + validate + sanitize + chunk
- `brain-store` single `Store {conn}` WAL owns DB; `init_schema` SCHEMA_VERSION, triggers FTS
- `brain-embed` rustls, `chunk_text` split by `## `, `embed_batch_concurrent(4)`
- `brain-mcp` rmcp tools + `sanitize_relative_path`; handlers thin, delegate to Store
- `brain-web` axum `AppState{db:String}` open per request (avoid !Sync), read-only
- `brain-cli` clap derive, `BRAIN_DB_PATH` env, `full_path` validate scope

## No Java/LangChain4j/Flyway/Lombok

## Testing

### Obrigatório — Todo desenvolvimento DEVE incluir testes

### Unit (`cargo test -p brain-core|brain-store`)
- `brain-core`: validate_layer/scope, sanitize traversal, frontmatter, chunk ##, wikilink
- `brain-store`: in-mem `Store::open_in_memory()` CRUD notes/chunks, FTS insert/delete, search RRF, TTL `forget_sweep`, audit `checkpoints/restore`, project CRUD, `cargo test -- --nocapture`
- Mock Ollama: `embed` não deve exigir Ollama real; fallback vec `[0.0;768]` para FTS-only

### Integration (`cargo test --workspace`)
- `store → search → read → delete → export → backup → sweep` end-to-end
- FTS + vector + entity + graph RRF k60 + authority boost
- `cargo build --workspace` zero warnings, `cargo clippy -- -D warnings`

### Cobertura
- Mín 70% `cargo llvm-cov --workspace --html`
- `cargo test --workspace` DEVE passar antes de commit

## Exact Commands
```bash
cargo test -p brain-core -- --nocapture
cargo test --workspace
cargo build --workspace   # release: --release (LTO thin)
cargo clippy --workspace -- -D warnings
cargo llvm-cov --workspace --html
```
