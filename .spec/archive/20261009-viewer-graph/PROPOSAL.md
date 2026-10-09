# PROPOSAL — viewer-graph

Data: 2026-10-09 · Fase 1 (Análise) · Base: `.spec/viewer-graph/EXPLORATION.md` (fonte da verdade).
Decisões P0→P1 aprovadas pelo usuário: **P1 = podar+contar**, **P2 = subgrafo induzido**.
Nenhum código nesta fase. Nenhum commit. Nenhum toque em produção.

## Problema

Viewer atual (`viewer/index.html:1-284`) é lista + busca textual. Relações entre
notas existem no banco mas são invisíveis: stream de grafo do RRF
(`crates/brain-store/src/lib.rs:1842`, `SELECT to_path FROM links WHERE from_path=?1 LIMIT 10`)
alimenta score, nunca UI. Usuário quer ver estrutura — clusters por projeto,
notas órfãs, hubs — e navegar por clique (grafo → `/api/read`).

Agravante medido (`EXPLORATION.md §3`, produção `mode=ro` 2026-10-09):
522 notas, só **12 arestas wikilink** (`links`, 12 `DISTINCT from_path`, 2 `DISTINCT to_path`),
11/12 em estrela para `sessoes/boleto/2026-10-08`, **1 dangling**
(`atlas-ecm/acl/root-subject-leaves-scene-lockout` sem row em `notes`).
Conectividade real v1 vem de `note_projects` (110 rows), não de wikilinks.
Endpoint só-wikilink desenharia 522 isolados + 1 estrela.

## Solução recomendada

Canvas 2D próprio (alternativa (a) da EXPLORATION) + `GET /api/graph?project=`
agregado sem N+1 + v1 = nós nota+projeto, arestas wikilink+project,
clique→`readNote()` existente (`viewer/index.html:165,179-226`),
filtro por projeto (owned∪linked), dangling podado+contado,
guards de wiring estendidos no commit 1, teto de layout documentado.

- Backend: 1 rota nova em `router(db)` (`crates/brain-web/src/lib.rs:15-27`),
  handler segue disciplina dos 4 existentes (`lib.rs:95-201`): abre
  `Store::open(&s.db)` por request, usa sincronamente, dropa antes do `.await`
  (regra `Send`, `AppState { db: String }` em `lib.rs:7`).
- Frontend: 1 view nova `renderGraph()` no `index.html` existente, fetch → render,
  sem framework, sem deps (No-SPA rule, `.agents/rules/frontend-rules.md`).
- Custo estimado: ~200 linhas JS + ~80 linhas Rust (handler + teste) + guards.

## Escopo

**ENTRA**
- `GET /api/graph?project=` — `nodes[{id,kind,label,layer}]`, `edges[{from,to,kind}]`,
  `dangling:N`, `projects:[]`. Kinds nó `note|project`, aresta `wikilink|project`.
  IDs projeto com prefixo `projeto:` (sem colisão com `notes.path`).
- Filtro `project=` owned∪linked (posse `notes.project_id` + `note_projects`;
  `brain_project_notes` faz JOIN das duas em `lib.rs:1009-1010`). Inexistente → vazio.
- Dangling podado + contado (P1 aprovada). Sem `LIMIT` parcial — query própria,
  sem reusar `recent_paths(1000)` (`lib.rs:1663`).
- Viewer: aba/view nova `renderGraph()`, pan/zoom, clique→`readNote(path)` verbatim
  (path casa `notes.path` exato, `lib.rs:125-130`; TD-011 proíbe strip),
  filtro projeto via `projects:[]`, labels último-segmento + tooltip path cheio,
  congelamento >1500 nós.
- Guards de wiring estendidos no commit 1 (padrão `lib.rs:606-851`:
  `leaf_paths`/`entry_field_reads`/`assert_no_phantom_reads`, `viewer_fn` por função).
- Testes: handler (payload, filtro, dangling, vazio, sem N+1 por construção),
  wiring (graph lê só o que `/api/graph` serve + clique entrega path resolúvel),
  mutações (remover campo → ≥1 falha).

**NÃO-ENTRA**
- Edição pelo grafo (criar link, arrastar-para-conectar, renomear, deletar). View-only.
- Entidades (`entities` 378 / `entity_links` 843 em `EXPLORATION.md §3`): bipartido
  entidade↔nota, outro layout, dobra payload. V2 candidata, não gap (R5).
- Novos layouts (hierárquico, concentric), física avançada (colisão, Barnes-Hut),
  drag de nós, setas direcionais animadas, export PNG/SVG.
- MCP tool nova, mudança de schema/FTS5/RRF, escrita fora de `BRAIN_EXPORT_ROOT`.
- Auth/permissão no endpoint (viewer é diagnóstico local, mesma superfície dos 4 handlers).

## Alternativas rejeitadas (com motivo)

| Alternativa | Motivo da rejeição |
|---|---|
| (b) Lib vendored local (cytoscape 3.34 min **420 KB / 131.5 KB gzip**, force-graph 1.51 **6.46 MB / 15 deps**) — `EXPLORATION.md §4b` | Peso ~30× o `index.html` atual (~15 KB); supply-chain vendored sem lockfile nem update (arquivo morto); API da lib vira dialeto nos guards; overkill para 122 arestas. Reavaliar se >2k nós ou v2 hierárquica (aí cytoscape, nunca force-graph). |
| (c) CDN externo (`unpkg`) — `EXPLORATION.md §4c` | **Rejeitada sem ressalva**: quebra offline-first (viewer em `127.0.0.1` morre sem internet/CSP); updates sem commit = anti-harness; contradiz razão do viewer. |
| Nó fantasma p/ dangling | Nó clicável que resolve para `{"error":"not found"}` (`lib.rs:129`) é 404 desenhado; podar+contar é diagnóstico honesto (P1 aprovada). |
| Reusar `recent_paths(1000)` no graph | Grafo parcial = arestas para nós ausentes; cap de browse não é teto de grafo (EXPLORATION §5). |
| Entidades na v1 | Layout e payload diferentes; dobra escopo sem provar o básico (EXPLORATION R5). |

## Riscos top-3

1. **R1 — grafo wikilink quase vazio (o maior).** 12 arestas / 522 notas; v1 parecerá
   vazia se avaliada como "rede de conhecimento". Calibragem: SPEC declara
   "grafo de projetos com overlay de wikilinks"; VERIFY documenta `links=12`, sem prometer rede.
2. **R4 — wiring guards.** TD-011 provou campo-lido≠campo-servido quebra silencioso
   (5 ocorrências, 3 vivas). `/api/graph` + `renderGraph()` entram nos guards desde o
   commit 1, não como follow-up — `viewer_fn` + `assert_no_phantom_reads` (`lib.rs:566-604`).
3. **R3 — read-only deve continuar read-only.** Handler sem `brain-embed`, sem escrita,
   sem `Store` através de `.await`. Qualquer "criar link pelo grafo" é v2 fora desta SPEC.
   (R2-perf e R6-typo ficam como R-0x na SPEC, não top-3.)
