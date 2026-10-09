# PLAN — viewer-graph (Fase 2, Planejamento)

Data: 2026-10-09 · Base: `.spec/viewer-graph/SPEC.md` (14 RFs, 5 ADRs, R-01..R-07, E5, §UX, segurança pass) + `.spec/viewer-graph/EXPLORATION.md` §8 (~200 JS + ~80 Rust).
Nenhum código nesta fase. Nenhum commit. Nenhum toque em produção (H3 scratch quando chegar a implementação).

Modelo de tabela: `.spec/brain-multi-project-hooks/PLAN.md` **não existe** (verificado em disco — só `EXPLORATION.md`, `PROPOSAL.md`, `SPEC.md` em `.spec/viewer-graph/`); tabela abaixo segue o formato pedido (entrega/dependências/modo de falha) sem alegar herança.

## F0 — Pré-condições H6 (antes de qualquer task)

Comandos (fonte `.agents/rules/harness-continuous.md:H1.5`, SPEC R-06):

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
.venv/bin/python -m pytest src/tests/ -q   # esperado: 124 passed, 2 legadas (H1.4)
cargo build --workspace
cargo run -p brain-cli -- --help            # smoke do help agrupado (HELP-01)
```

Critério: Rust 0 falhas + clippy 0 + Python só as 2 legadas nomeadas (`test_integration_full_cycle`, `test_integration_errors`) + build 0 erros. Terceira falha Python = regressão, bloqueia (H1.4).

## Fases (ordem de dependência e merge)

| Fase | Entrega | Dependências | Modo de falha |
|------|---------|--------------|---------------|
| F1 endpoint `GET /api/graph?project=` (RF-01..RF-07) | rota em `router()` (`crates/brain-web/src/lib.rs:15-27` hoje com 4 rotas) + handler read-only `AppState{db}` (`lib.rs:7`), 4 queries agregadas zero N+1, erro `{"error"}` (`lib.rs:96`) | F0; schema existente `links` sem FK (`crates/brain-store/src/lib.rs:927-932`), `note_projects` (`lib.rs:945-951`), `project_list` (`lib.rs:988`), `recent_paths(1000)` (`lib.rs:1663-1667`) como anti-modelo | filtro só-posse (repete blind-filter `lib.rs:1682-1691,1779`); reusar `recent_paths` cap → grafo parcial (RF-06); `unwrap` em handler → panic em vez de `{"error"}` (R-07); `Store` através de `.await` → erro `Send` (architecture-rules) |
| F2 viewer `renderGraph()` (RF-08..RF-12 + §UX) | 4ª view (`viewer/index.html:43-55` botões atuais), canvas próprio ~200 linhas, clique→`readNote(id)` verbatim (`index.html:165,179-226`), dropdown de `projects:[]`, labels+tooltip, congelamento >1500 + contador | F1 (payload real para desenhar); `escapeHtml` (`index.html:274-278`) para labels; `readNote` casa `notes.path` exato (`lib.rs:125-130`) | clique com strip (`split/slice/replace` — guard `lib.rs:825`); label sem tooltip → colisão `sessoes/x/<data>` ilegível (RF-11); simulação infinita >1500 → 25M ops/frame (EXPLORATION §4a); deps/CDN → quebra offline-first e No-SPA (frontend-rules) |
| F3 guards + testes + mutações (RF-13 + RF-14) | 2 guards estendidos (`assert_no_phantom_reads` `lib.rs:566-583`, `viewer_fn` `lib.rs:592-604`, `JS_MEMBERS`/`JS_ROOTS` `lib.rs:462-473`) + handler tests (5 casos) + 4 mutações pegas | F1+F2 (guards varrem fonte contra payload real; padrão `lib.rs:606-851`) | guard varrendo função errada → scan vazio (guard exige `!reads.is_empty()`, `lib.rs:568`); teste sem caso só-link → filtro só-posse passa (mesma classe do SPEC §9 correção 2); mutação não-paga → teste decorativo (padrão backend-rules "mutação, não suposição") |
| F4 verify + delta docs + E2E H3 | `VERIFY.md` (E5), Delta Specs SPEC §6 (AGENTS.md +1 linha endpoint; frontend-rules +1 parágrafo; sem DDL, sem tool MCP), E2E scratch 12-step H3 (nunca `data/brain.db`, nunca 8321/8322) | F3 verde; portões H6 completos | E2E contra produção → destrói corpus/servidor (H3.1); `BRAIN_OLLAMA_URL` removida em teste → cai no default real `http://localhost:11434` (`crates/brain-embed/src/lib.rs:23`, H2.3) → flake; documentar `projects:[]` como tool MCP → SPEC §6 diz nenhum |

Nota de ordem: SPEC RF-13 vincula guards **desde o commit 1** — extensão mínima de `JS_MEMBERS`/`viewer_fn` para `renderGraph` viaja com o commit do endpoint (F1), e o enrijecimento completo (2 guards verdes + mutações) fecha em F3. Ordem abaixo reflete isso.

## Ordem de merge (commits pequenos)

1. `F1a` rota + handler esqueleto (formato RF-01/RF-02, `{"error"}` R-07) + teste formato — sem filtro ainda.
2. `F1b` filtro owned∪linked na query (RF-03, predicado nos dois lados) + teste só-posse/só-link/outro-projeto + teste inexistente→vazio (RF-04).
3. `F1c` dangling podado+contado (RF-05) + `projects:[]` (RF-07) + query própria sem LIMIT (RF-06) + extensão mínima dos guards p/ `renderGraph` (RF-13a andaime).
4. `F2a` aba + `renderGraph()` canvas esqueleto (RF-08) + clique→`readNote` verbatim + nó-projeto→filtro (RF-09).
5. `F2b` dropdown (RF-10) + labels/tooltip (RF-11) + paleta §UX + `escapeHtml` nos labels.
6. `F2c` congelamento >1500 + contador + `prefers-reduced-motion` estático (RF-12 + tese §UX).
7. `F3` guard (b) clique-resolvível + handler tests restantes (`projeto:` sem colisão, sem-LIMIT) + 4 mutações RF-14 verdes.
8. `F4` E2E H3 scratch + portões H6 + `VERIFY.md` + Delta Specs.

Cada commit: `cargo test -p brain-web` + `cargo clippy --workspace --all-targets -- -D warnings` verdes antes do próximo; F4 exige suite completa nos dois portões (H1.2).
