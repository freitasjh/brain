# Frontend Rules — viewer (brain-web)

## Viewer
- Viewer é `viewer/index.html` **embutido no binário** (`brain_web::VIEWER_INDEX = include_str!("../../../viewer/index.html")`) + `crates/brain-web` axum `GET /api/status,search,read,list,graph,events` read-only sobre `brain.db`
- No Vue/Pinia/PrimeVue — não há SPA complexo nesta fase
- Servir o índice embutido via `brain_web::router` + `axum::serve` em `brain serve --port 8322`; fallback: rota não-API → documento (SPA), `/api/*` desconhecida → `404` JSON. **Nenhum diretório é servido** (`ServeDir` removido — `GET /server.py` respondia `200 text/plain` em `0.0.0.0:8322`); trade-off aceito: editar `viewer/index.html` exige rebuild do binário
- Live update do grafo (viewer-live-graph, 2026-10-10): `GET /api/events` SSE (`text/event-stream`, evento `graph-changed`, polling 2 s sobre snapshot `(count_notes(), count_chunks())`); frontend usa `EventSource` idempotente via `ensureGraphEventSource()` com lifecycle separado do RAF loop — `renderGraph()` cancela só o RAF e garante a fonte aberta, `stopGraph()` fecha só ao sair da view de grafo

## No-SPA Rule (Fase A+B)
- Fase C pode adicionar frontend dedicado; até lá, manter viewer minimal

## Aba Grafo (viewer-graph)
- 4ª view `renderGraph()` em `viewer/index.html`: canvas force-directed próprio (~8–12 KB JS, `requestAnimationFrame`, pan/zoom por transform), sem deps/build/CDN; lê `GET /api/graph?project=` (`{nodes,edges,dangling,projects}`, ids verbatim p/ `/api/read`, hubs `projeto:<nome>`); layout congela acima de 1500 nós com anúncio visível; deriva lenta 0.2× abaixo do teto, `prefers-reduced-motion` → estático
- Produto (viewer-product-graph): 3 tabs Buscar/Explorar/Grafo (navegação única); drawer lateral de leitura via `/api/read` (id verbatim, `Esc` fecha, bottom-sheet em 360px, tabs scrolláveis); legenda 6-hex fixa (Noite `#0d1117`, Papel `#c9d1d9`, Link `#58a6ff`, Trilha `#3fb950`, Guia `#f0883e`, Orfã `#484f58`); cap 40 labels + anti-colisão; alfa arestas 0.35→0.55 (hover/seleção); hub quadrado Guia 6px, nota círculo 3px, órfã círculo vazado; header produto com badge status (`/api/status` coverage + `queue.*`, `(N awaiting embed)` abaixo de 100%) + footer `API_BASE`; feedback por banner inline (loading/erro/vazio, sem toast); `Esc` limpa busca; SSE `ensureGraphEventSource()` idempotente mantido; fetch com `AbortController` 3 s; XSS via `escapeHtml()` (sem `innerHTML` cru)

## Testing (se viewer evoluir)
- Teste axum handler via `cargo test -p brain-web` com `tower::ServiceExt::oneshot`
- Validar `/api/search?query=foo&top_k=5` → 200 JSON `results[]`, `/api/list` → entries

## Exact Commands
```bash
cargo run -p brain-cli -- serve --port 8322
curl "http://localhost:8322/api/status" | jq
curl "http://localhost:8322/api/search?query=test&top_k=5"
```
