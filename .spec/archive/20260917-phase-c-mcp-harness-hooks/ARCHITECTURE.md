# ARCHITECTURE — Phase C

## Overview
MCP full Rust + harness Rust + hooks Rust tudo junto, local agents only.

## Components
- `crates/brain-mcp/src/lib.rs` — rmcp `#[tool] ping,store,read,search,delete,recent,status,checkpoints,restore,backup,export,forget_sweep,project_*` + `create_router(db)` axum SSE fallback, `AppState{db}` open per request
- `crates/brain-cli/src/main.rs` + `hook` subcommand — `HookCmd {event,payload,project}` → `Store::note_upsert sessoes/brain/<ymd>` + `brain_search` scope global for context injection
- `crates/brain-web/src/lib.rs` — already ServeDir + /api, add CORS restricted localhost
- `.agents/rules/harness-continuous.md` — rewrite H1 suite `cargo test --workspace`, H2 zero tolerance `cargo test` before dev, H3 E2E brain 12-step, H6 pre-completion 7 Rust items (cargo test/build/clippy, E2E, H4 logic, review zero 🔴, brain_store, ports free)

## Data Model
- `notes` + `chunks` + `hook_events` (optional) `expires_at` TTL
- `AppState{db:String}` per request avoids !Sync Store sharing

## API
- MCP SSE `POST /mcp/tool/:name` JSON, stdio `brain --transport stdio` fallback
- Hook CLI `brain hook --event session-start --project myproj --payload '{"prompt":"..."}'`
- Harness CLI `cargo test --workspace && cargo run -p brain-cli -- --db ./data/brain.db search "test" --explain`

## Flows
1. Agent `brain_search` → MCP → Store::search RRF 3 streams + authority → JSON
2. Hook session-start → `brain hook` → Store sessoes + `brain_search scope global` inject
3. Harness `cargo test` → E2E `mcp ping→store→search→read→delete→recent→export→backup→sweep→status`
