# VERIFY — viewer-graph

Data: 2026-10-09 · Fase 4 (Verify pós-implementação) · Commits C1–C4 verificados.
Veredito: **PASS — arquivar.** Nenhuma dimensão falhou.

## 1. Completude — TASKS.md checkboxes

14/14 marcados `[x]` com evidência (`.spec/viewer-graph/TASKS.md`):

| Task | Evidência (teste verde + commit) |
|---|---|
| VG-BE-01 rota+formato | `graph_serves_nodes_edges_dangling_and_projects` — C1 `7fc3702` |
| VG-BE-02 filtro owned∪linked | `graph_filter_keeps_owned_and_linked_drops_other_project` — C1 `7fc3702` |
| VG-BE-03 dangling podado+contado | `graph_dangling_is_pruned_and_counted` — C1 `7fc3702` |
| VG-BE-04 hubs+sem-LIMIT | `graph_project_hubs_do_not_collide…` + `graph_has_no_cap_over_a_thousand_notes` + guard `graph_snapshot_queries…` — C1 `7fc3702` |
| VG-BE-05 inexistente→vazio | `graph_unknown_project_is_empty_not_an_error` — C1 `7fc3702` |
| VG-FE-01 aba+renderGraph | 4ª view + canvas próprio — C2 `be9fb79`, guard C3 `dbc1969` |
| VG-FE-02 clique→readNote | `viewer_graph_click_hands_the_read_endpoint_a_resolvable_id` — C3 `dbc1969` |
| VG-FE-03 dropdown+labels+paleta | dropdown `projects:[]`, tooltip = path cheio, paleta §UX — C2 `be9fb79` |
| VG-FE-04 freeze >1500+deriva | anúncio visível + 0.2× + reduced-motion — C2 `be9fb79` |
| VG-VAL-01 guard phantom-reads | `viewer_graph_reads_only_fields…` incl. `JS_MEMBERS`+`replaceAll` — C3 `dbc1969` |
| VG-VAL-02 guard clique-resolvível | metade estática + metade `note_get` — C3 `dbc1969` |
| VG-VAL-03 handler 5 casos | 5 tests + `assert_graph_wiring` + guards uncapped/R-07 — C1+C3 |
| VG-VAL-04 4 mutações RF-14 | M1 5 falhas / M2 3 / M3 1 / M4 1, tudo revertido (F3a, TASKS.md) |
| VG-VAL-05 E2E+portões+delta | scratch 12+12b+G1-G4 ok, prod intacta (F3a); Delta docs C4 `68e9d3b` |

Zero TODOs abertos. Delta Specs §6 aplicados em C4 (AGENTS.md +1 linha, frontend-rules +1 parágrafo); sem tool MCP nova (SKILL.md intacto por construção); zero DDL (database-rules/architecture-rules intactas por construção).

## 2. Corretude — H6 counts + RFs

- `cargo test --workspace` → **484 passed / 0 failed** (re-medido no fechamento, pós-C4).
- `cargo test -p brain-web` → **20/0** após **cada** commit (C1, C2, C3, C4).
- F3a (inalterado desde, exceto 1 string em allowlist de guard coberta pelos mesmos 20 tests): clippy `--all-targets` 0, pytest 124+2 legadas (H1.4), build 0, `brain --help` ok; mutações RF-14 4/4; E2E scratch 12+12b+G1-G4 ok, produção 8321/8322 intacta (`workflow-state.json`, arquivo do usuário, não tocado).
- RF-01..RF-14 cobertos: formato/kinds/ids (BE-01), filtro owned∪linked em SQL (BE-02), inexistente→vazio 200 (BE-05), dangling (BE-03), sem-LIMIT+projects (BE-04), view/clique/dropdown/labels/freeze (FE-01..04), guards handler+wiring+mutações (VAL-01..04); R-01 read-only, R-02 sem dep nova, R-03 agregação, R-04 orçamento, R-05 No-SPA, R-06 portões, R-07 sem unwrap (guard dedicado).
- E5 (SPEC §4): aba Grafo com clusters por projeto via hubs `projeto:`, clique lê a nota, dropdown filtra subgrafo, badge inalterado, dangling como contador, inexistente vazio honesto.

## 3. Coerência — ADRs no código, sem vazamento

- ADR-01 canvas próprio: `viewer/index.html` sem deps/build/CDN (C2) — guards `viewer_fn` cobrem por função.
- ADR-02 dangling podado+contado: `dangling++` no handler, sem nó fantasma (`crates/brain-web/src/lib.rs`, `graph()`).
- ADR-03 subgrafo induzido: aresta só com 2 pontas no conjunto servido (handler + `assert_graph_wiring`).
- ADR-04 sem entidades na v1: handler lê só `notes/links/note_projects/projects` — zero `entities/entity_links`.
- ADR-05 query própria sem LIMIT: `graph_snapshot` (`crates/brain-store/src/lib.rs`) sem `LIMIT`/`recent_paths` (guard fonte).
- Sem vazamento de camada: `brain-web` segue folha read-only (só `brain-store`+`brain-core`), `AppState{db}` + `Store` por request, erro honesto `{"error"}` sem `unwrap` (guard R-07); `GraphSnapshot` sem `content` (postura `/api/list`).
- Reviews: zero 🔴 (premissa do fechamento; adição do review — `replaceAll` em `JS_MEMBERS` — aplicada pré-C1 com `cargo test -p brain-web` verde).

**Veredito: PASS nas 3 dimensões. Liberado para archive.**
