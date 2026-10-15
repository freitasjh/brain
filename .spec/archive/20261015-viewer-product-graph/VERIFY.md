# VERIFY — viewer-product-graph (sdd-compiler modo verify)

Data: 2026-10-15 · Método: sdd-compiler `verify` + `asserts/verify-gate.md`
(adaptado: `cargo test --workspace` no lugar de `mvn`; RBAC/tenant N/A
justificado na SPEC §3 — viewer local read-only).
Entradas: SPEC.md (§11 Test Strategy) · PLAN.md (§8/§8b) · TASKS.md (15/15
Done) · código `viewer/index.html` (único diff produto) ·
accept/{GATE,HARNESS-verify,A-*,E-*,lighthouse}.

## Dimensão 1 — Completeness ✅ PASS

- Tasks 100%: 15/15 Done em TASKS.md (P0 2 · P1 8 · P2 3 · P3 2); 101
  checkboxes `[x]`; grafo §3 DAG sem órfãs (verificado por leitura).
- Sem TODOs: `grep -c 'TODO\|FIXME\|HACK' viewer/index.html` = 0.
- Sem N/A sem justificativa: os 3 N/A da SPEC (IAM/ERD/Eventos) estão
  justificados na SPEC §3/§5/§6 e Delta registra NONE onde nada muda.
- BDD coberto: A-01..A-13 + A-SEC-01/02 + E-01..E-08, cada um com log em
  accept/ (este batch re-mediu em browser o que era estático: A-05 37 ms,
  A-06 overlap 0 + 7 frames + boundary-41, A-09 1500/1501/1502 + race,
  E-08 360px + Esc, E-04 0 RAF, Lighthouse navigation 94).

## Dimensão 2 — Correctness ✅ PASS

- Build: `cargo build --workspace` Finished (0 erros); crates intocadas
  (diff produto só viewer/index.html) → cobertura Rust inalterada
  (não re-medida neste batch; última 86.75%/70.17% ≥70%).
- Testes: `cargo test --workspace` exit 0 — **516 passed / 0 failed**
  (== workflow-state 516; 504 únicos; justificativa em HARNESS-verify.log);
  `cargo test -p brain-web` **24/24** (inclui pós-fix graphGen).
- RF→código (amostra verificada em browser, resto em logs citados):
  RF-01 tabs/INV-01 (A-01 + snapshot: 1 selected) · RF-02/04 banner sem
  toast (A-02-A-04, `data-toast` 0) · RF-03 Esc (A-03) · RF-05 37 ms ≤5 s
  (A-05) · RF-06 32≤40 + overlap 0 + boundary-41 =40 (A-06) ·
  RF-07 6 hex computados (A-07-A-08) · RF-08 6px/3px/vazada/0.55 (idem) ·
  RF-09 freeze+anúncio+raf 0+deriva 0.2× (A-09) · RF-10 header/badge/footer
  (A-10 + snapshot) · RF-11 drawer verbatim+Esc+bottom-sheet (A-11, E-08) ·
  RF-12 precedência offline>degraded>online + awaiting-embed (A-12,
  badge Degraded 0% ao vivo) · RF-13 a11y 94 + aria-live + foco + motion
  (A-13, E-04). SEC-01 3 payloads literal (A-SEC-01) · SEC-02 5 fetches
  3 s (A-SEC-02) · SEC-03 sem segredos (footer origem/host) · SEC-04
  encodeURIComponent + zero onclick/eval (gate). EC-01..08 8/8 (E-log).
- RBAC/multi-tenancy: N/A (SPEC §2/§3) — sem auth, sem escrita, sem DDL.

## Dimensão 3 — Coherence ✅ PASS

- ADRs: VIEW-01 (diff --name-only só viewer/index.html + .spec/) ·
  VIEW-02 (src="http 0, sem deps/build) · VIEW-03 (nenhum endpoint/param
  novo; 6 contratos reutilizados, todos 200).
- Sem vazamento de camada: zero linhas Rust tocadas; viewer não embebe
  (lê o que a fila escreveu).
- Naming: LABEL_CAP/GRAPH_FROZEN_AT/EDGE_ALPHA_* /ensureGraphEventSource
  conforme SPEC; nenhuma convenção violada.
- DevTools (harness verify): console 0 messages (2 páginas); network
  status/search/list/read/graph/events 200 + SSE abre, zero 4xx/5xx;
  i18n PT-BR (lang, título, badge, empty, freeze).

## Bug encontrado e fechado neste batch (P2)

Loop RAF stale ressuscitado por re-render SSE quebrava RF-09 sob race
(tick 846, raf vivo, drift 20u/s com frozen=true). Fix viewer-only
(`graphGen/myGen`, +9/-0 linhas lógicas): race reproduzida pós-fix →
tick 300, raf 0, 10 s estável. brain-web 24/24 pós-fix.

## Gaps não-bloqueantes (P3 → roadmap, H4.5)

1. Lighthouse: color-contrast, label-content-name-mismatch,
   landmark-one-main, meta-description (a11y segue 94 ≥ 90).
2. E-04 CSS `@media reduce` só estático (sem CDP media emulation).
3. `workflow-state.json`: registrar `test_unique` (504) além da soma (516).
4. Resize WM mínimo 1280 (contornado com emulate viewport CDP).

```
VERIFY STATUS: APPROVED ✅

Bloqueadores encontrados:
- (nenhum)

Observações:
- Archive liberado: deltas SPEC §Delta (frontend-rules.md ## Aba Grafo).
  Sem commit neste batch (restrição da delegação).
- Pendências p/ archive: mesclar delta + mover .spec/ → archive/ + sugerir
  commit (tudo fora deste batch).
```
