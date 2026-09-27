# Frontend Rules — viewer (brain-web)

## Viewer
- Viewer é `viewer/index.html` estático + `crates/brain-web` axum `GET /api/status,search,read,list` read-only sobre `brain.db`
- No Vue/Pinia/PrimeVue — não há SPA complexo nesta fase
- Servir estático via `brain_web::router` + `axum::serve` em `brain serve --port 8322`

## No-SPA Rule (Fase A+B)
- Fase C pode adicionar frontend dedicado; até lá, manter viewer minimal

## Testing (se viewer evoluir)
- Teste axum handler via `cargo test -p brain-web` com `tower::ServiceExt::oneshot`
- Validar `/api/search?query=foo&top_k=5` → 200 JSON `results[]`, `/api/list` → entries

## Exact Commands
```bash
cargo run -p brain-cli -- serve --port 8322
curl "http://localhost:8322/api/status" | jq
curl "http://localhost:8322/api/search?query=test&top_k=5"
```
