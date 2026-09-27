# PLAN — Phase C

## Constraints
- Rust 1.85 axum rmcp WAL ports 8321/8322

## Strategy
- MCP per request AppState{db} open, hooks spool file lock, harness brain 12-step

## Bootstrap
- `cargo build --workspace` before MCP tools

## Tasks per event
- Hook session-start: Producer Hook→Outbox Store sessoes→Consumer brain_search

## Risks
- rmcp fallback axum, hook race lock
