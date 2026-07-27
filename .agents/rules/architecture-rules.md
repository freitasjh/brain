# Architecture Rules

## Stack
- **Backend**: Java 21, Spring Boot 3.x, LangChain4j, MySQL 8, Flyway
- **Frontend**: Vue 3 (Composition API + `<script setup>`), Pinia, PrimeVue 4, Vite, TypeScript
- **Communication**: REST JSON over HTTP. No WebSocket for now.

## Package Boundaries
- `backend/` — Maven multi-module ready single-module app for now
- `frontend/` — Vite SPA proxying `/api` to backend

## Key Architectural Decisions
- Agents are persisted entities with type, model config, and tool bindings
- Tools are registered as Spring beans annotated with `@Tool` (LangChain4j)
- Each conversation belongs to one agent; messages are append-only
- No auth layer yet — add when needed
- Flyway migrations are the single source of truth for schema; never `ddl-auto=update` in production
