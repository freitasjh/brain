# Implementation Plan: viewer-product-graph

## 1. Context & SPEC Traceability
- **Target Specification**: `.spec/viewer-product-graph/SPEC.md` (13 RF RF-01..RF-13 + 8 EC EC-01..EC-08 + SEC-01..SEC-04, gate SPEC PASS + emendas governança spec-review 88% / security-review partial)
- **Goal**: Compilar SPEC em plano executável restrito a `viewer/index.html` (VIEW-01), sem tocar `/api/*`, sem deps/build/CDN, com aceite manual numerado por RF/EC + gate textual CI.

### 1.1 SPEC Validation Report
- **Status**: Validated with Warnings (emendas aplicadas nesta delegação)
- **Identified Gaps/Warnings (spec-review 88% + security-review partial → emendados na SPEC)**:
  - ALTA URL-encoding → emendado SEC-04 + RF-11 (`encodeURIComponent` em `query`/`path`/id/`project`).
  - ALTA teste manual sem CI → emendado §11 + §8 deste PLAN (script aceite A-01..A-13/E-01..E-08 + gate textual CI por grep).
  - ALTA RF-06 amostra → emendado RF-06 (método: 1280px, contador observável, amostra = 40 visíveis, overlap 0).
  - ALTA RF-05 ambiente → emendado RF-05 (Chromium estável, 1280×720, fixture local + meta).
  - MÉDIA aria-live → emendado RF-13 (`aria-live="polite"` busca/banner).
  - MÉDIA foco Esc → emendado RF-11 (drawer→invocador; busca→input).
  - MÉDIA badge precedência → emendado RF-12 (`offline > degraded > online`).
  - MÉDIA sugestão empty → emendado EC-01 (texto fixo normado).
  - MÉDIA escopo escapeHtml → emendado SEC-01 (função única, cobre snippet/corpo/`layer`/`path`/erro/`API_BASE`).
  - H-01 onclick ban → emendado SEC-01/SEC-04 (`data-*` + delegado, zero `onclick` interpolado).
  - H-02 erros via textContent/escape → emendado SEC-02.
  - H-03 SEC-02 nomear 5 fetches 3 s + testes XSS/timeout → emendado SEC-02 (lista 5 + `AbortSignal.timeout(3000)`; aceite A-SEC-01/A-SEC-02).
- **Assumptions Made**: Nenhuma premissa aberta — SPEC cobre ambientes, métodos e superfícies. Fixture `fixture-graph-500.json` é local e determinística (não rede, não produção).

## 2. Technical Strategy
### 2.1 Backend Strategy
- **Framework**: N/A — nenhum handler muda (`brain-web` congelado, VIEW-03). Regressão gratuita: `cargo test -p brain-web` verde (Phase 0).

### 2.2 Frontend Strategy
- **Framework**: Nenhum (No-SPA Rule). Single-file `viewer/index.html`, canvas force-directed próprio, `requestAnimationFrame`, pan/zoom por transform.
- **State Management**: Estado local de view (tabs/drawer/badge/banner/SSE) sem store externa; `ensureGraphEventSource()` idempotente; `renderGraph()` cancela só RAF.

### 2.3 Infrastructure Strategy
- **Deployment Model**: N/A — arquivo estático servido por `brain_web::router` existente.
- **CI/CD**: Gate textual (grep, sem executar produto) + `git diff --name-only` + `cargo test -p brain-web`. Sem harness JS no repo.

## 3. Architecture Realization & Constraint Mapping
| SPEC Reference | Constraint | Enforcement Point | Failure Mode | Implementation Strategy |
|---|---|---|---|---|
| VIEW-01 | Só `viewer/index.html` muda | Gate diff (Phase 0 + F4) | Scope creep p/ `/api/*` ou SPA | `git diff --name-only` só `viewer/index.html` (+ `.spec/`); TASKS barra outro arquivo produto |
| VIEW-02 | Sem `<script src>`/CDN/build | Gate textual CI | Dep externa / build quebrado | Inline apenas; grep barra `src="http` em script |
| VIEW-03 | Nenhum endpoint/param/campo novo | Gate diff + review | Contrato quebrado | Reutilizar 6 endpoints verbatim; nenhum query-param novo |
| SEC-01/SEC-04 (H-01) | XSS + onclick ban + `data-*` + `encodeURIComponent` + `escapeHtml` tudo | Gate textual + aceite XSS | Execução de `<img onerror>` / `javascript:` via snippet, `layer`/`path`/erro | `escapeHtml()` única em todos os sinks; `textContent` p/ erros; `data-*` + delegado; `encodeURIComponent(query,path,project)` |
| SEC-02 (H-03) | 5 fetches com `AbortSignal.timeout(3000)` | Gate textual + aceite timeout | UI pendente infinita | `status,search,list,read,graph` com timeout 3 s → banner + badge (SSE `events` fora da lista) |
| RF-05 | 500 nós ≤5 s, Chromium 1280px, fixture local + meta | Aceite A-05 | O(n²) trava | Cap + freeze + deriva 0.2×; medição com meta anexada |
| RF-06 | 40 labels a 1280px, overlap 0 na amostra de 40 | Aceite A-06 | Labels ilegíveis | Ocultação por prioridade; contador observável |
| RF-12 | Badge precedência `offline>degraded>online` | Aceite A-12 | `online` falso com fila/timeout | Derivação health+timeout+coverage |
| RF-13 | A11y ≥90, `aria-live`, foco, motion | Aceite A-13 | Leitor de tela mudo / animação forçada | `role=img`+`aria-label`, `aria-live="polite"`, foco-Esc, `prefers-reduced-motion` estático |
| RF-11 | Drawer `encodeURIComponent` + foco invocador + bottom-sheet 360px | Aceite A-11 | Id com `/` quebra fetch; foco perdido; overflow mobile | `encodeURIComponent(id)` + `data-*`; `Esc`→invocador; classe bottom-sheet |

## 4. Module & Code Structure
| Module Name | Responsibilities | Boundaries & Communication |
|---|---|---|
| `viewer/index.html` (único) | Tabs Buscar/Explorar/Grafo; banner inline; drawer + bottom-sheet; legenda 6-hex; cap 40 + anti-colisão; header badge + footer; SSE lifecycle; `escapeHtml` + timeouts | Lê só os 6 contratos §5 SPEC via `fetch`/SSE; nenhuma escrita; nenhuma crate cruzada |

## 5. Risks & Complexity Analysis
- **🔥 Layout O(n²) trava acima de 500 nós** — Mitigação: cap 40 + anti-colisão por prioridade; freeze >1500 + anúncio + deriva 0.2×; aceite A-05/A-09 com fixture 500/1501.
- **SSE `EventSource` duplica/vaza (RAF lifecycle)** — Mitigação: `ensureGraphEventSource()` idempotente; `renderGraph()` cancela só RAF; `stopGraph()` fecha só ao sair; aceite E-06 (disconnect/reconnect/duplo `ensure` → 1 fonte).
- **XSS via snippet/drawer/`layer`/`path`/erro** — Mitigação: `escapeHtml()` única em todos os sinks + `textContent` p/ erros + zero `onclick` interpolado; aceite A-SEC-01 (3 payloads, incluso `<img onerror>` e `<script>`, nenhum executa, nenhum request).
- **Timeout fetch (5 fetches 3 s) vs badge** — Mitigação: `AbortSignal.timeout(3000)` nos 5 + precedência `offline>degraded>online`; aceite A-SEC-02 (abort→`offline`, parcial→`degraded`).
- **Scope creep (`/api/*`, SPA, toast)** — Mitigação: gate `diff --name-only` + grep (`src="http`, `onclick=`, `eval(`, `Function(`, `document.write`, `outerHTML`, `data-toast`) em toda fase.

## 6. Implementation Phases & Dependencies
### Phase 0 — Bootstrap + gates (pré-requisito, sem produto)
- **Dependencies**: None.
- **Scope**: `cargo test -p brain-web` verde; `git diff --name-only` baseline; fixture local `fixture-graph-500.json` + `fixture-graph-1501.json` disponíveis; Chromium 1280×720 pronto. Nenhum RF entregue; destrava F1–F3.

### Phase 1 — Must: grafo + busca + header (RF-01..RF-10)
- **Dependencies**: Phase 0.
- **Scope**: Tabs (RF-01), estados busca + banner sem toast (RF-02/RF-04), Esc limpa (RF-03), grafo 500 ≤5 s (RF-05), cap 40 (RF-06), legenda 6-hex (RF-07), simbologia + alfa 0.55 (RF-08), freeze >1500 (RF-09), header + footer (RF-10). SEC-01/SEC-02/SEC-04 aplicados aos sinks tocados.

### Phase 2 — Should: drawer + status + a11y (RF-11..RF-13)
- **Dependencies**: Phase 1 (tabs/banner/SSE base).
- **Scope**: Drawer `encodeURIComponent` + `data-*` + foco-Esc + bottom-sheet 360px (RF-11), badge precedência + `(N awaiting embed)` (RF-12), `aria-live` + `role=img` + foco + motion + Lighthouse ≥90 (RF-13). Fecha SEC-01 (todos os sinks) e SEC-02 (5 fetches).

### Phase 3 — Could: polish + endurecimento (EC + médias consolidadas)
- **Dependencies**: Phase 2.
- **Scope**: Sugestão empty fixa (EC-01), `*():{}=-/` sem 500 (EC-07), SSE disconnect sutil (EC-06), 360px sem overflow (EC-08), `prefers-reduced-motion` estático (EC-04), revisão final 6-hex/0.55/freeze-deriva. Re-gate textual + diff + Lighthouse.

## 7. Critical Execution Path
- `[P0 gates]` → `[P1 tabs+busca]` → `[P1 grafo cap]` → `[P1 header]` → `[P2 drawer]` → `[P2 badge]` → `[P2 a11y]` → `[P3 polish+gate]`
- MVP mínimo (ignora polish): P0 → tabs/busca (RF-01..04) → grafo (RF-05/06) → header (RF-10) → drawer (RF-11) → badge (RF-12).

## 8. Atomic Task Breakdown & Execution Graph

### Frontend Tasks (Phase 0 — bootstrap)
- **[P0-01]** [ ] `cargo test -p brain-web` verde + baseline `git diff --name-only`
  - **Depends on**: None · **Layer**: Frontend-infra · **Artifact**: log teste + baseline diff · **Enforcement**: API congelada, diff parte de zero produto · **Constraint Ref**: VIEW-01/03
- **[P0-02]** [ ] Fixtures locais `fixture-graph-500.json` + `fixture-graph-1501.json` + ambiente Chromium 1280×720
  - **Depends on**: P0-01 · **Layer**: Frontend-infra · **Artifact**: fixtures + meta template · **Enforcement**: RF-05/RF-06 reprodutíveis sem rede/produção · **Constraint Ref**: RF-05/RF-06

### Frontend Tasks (Phase 1 — Must)
- **[P1-01]** [ ] Tabs Buscar/Explorar/Grafo navegação única (`aria-selected`, 1 ativa — INV-01)
  - **Depends on**: P0-02 · **Artifact**: tabs · **Enforcement**: sem views paralelas · **Constraint Ref**: RF-01
- **[P1-02]** [ ] Estados busca `idle→loading→(results|empty|error)` + banner inline, zero toast
  - **Depends on**: P1-01 · **Artifact**: banner + spinner + retry · **Enforcement**: `data-toast` ausente; `loading` sempre indica · **Constraint Ref**: RF-02/RF-04
- **[P1-03]** [ ] `Esc` limpa busca (drawer fechado → `idle`, input vazio, foco no input)
  - **Depends on**: P1-02 · **Artifact**: handler Esc busca · **Enforcement**: INV-02 (drawer primeiro) · **Constraint Ref**: RF-03
- **[P1-04]** [ ] Grafo 500 nós ≤5 s (fixture local, Chromium 1280px + meta)
  - **Depends on**: P0-02 · **Artifact**: render path + freeze/fonte base · **Enforcement**: O(n²) mitigado · **Constraint Ref**: RF-05
- **[P1-05]** [ ] Cap 40 labels + anti-colisão por prioridade (contador observável)
  - **Depends on**: P1-04 · **Artifact**: cap + hide · **Enforcement**: INV-03 · **Constraint Ref**: RF-06
- **[P1-06]** [ ] Legenda 6-hex fixa + simbologia hub/nota/orfã + alfa 0.35→0.55
  - **Depends on**: P1-05 · **Artifact**: legenda + estilos · **Enforcement**: INV-04 · **Constraint Ref**: RF-07/RF-08
- **[P1-07]** [ ] Freeze >1500 + anúncio + deriva 0.2×
  - **Depends on**: P1-04 · **Artifact**: freeze path · **Enforcement**: nunca congela silencioso · **Constraint Ref**: RF-09
- **[P1-08]** [ ] Header produto + badge base + footer `API_BASE` (escapado)
  - **Depends on**: P1-02 · **Artifact**: header/footer · **Enforcement**: SEC-01 (API_BASE escapado) · **Constraint Ref**: RF-10

### Frontend Tasks (Phase 2 — Should)
- **[P2-01]** [ ] Drawer `/api/read` via `data-*` + `encodeURIComponent(id)`, `Esc`→invocador, bottom-sheet 360px, tabs scrolláveis
  - **Depends on**: P1-01, P1-08 · **Artifact**: drawer · **Enforcement**: H-01 (zero onclick), foco-Esc · **Constraint Ref**: RF-11/SEC-04
- **[P2-02]** [ ] Badge `online/degraded/offline` precedência + `(N awaiting embed)`; 5 fetches `AbortSignal.timeout(3000)`; erros via `textContent`/escape
  - **Depends on**: P1-08 · **Artifact**: badge + fetch wrapper · **Enforcement**: H-02/H-03 · **Constraint Ref**: RF-12/SEC-02
- **[P2-03]** [ ] A11y: `canvas role=img` + `aria-label`, `aria-*` controles, `aria-live="polite"` busca/banner, foco drawer, motion estático, Lighthouse ≥90
  - **Depends on**: P2-01 · **Artifact**: attrs a11y · **Enforcement**: RF-13 · **Constraint Ref**: RF-13

### Frontend Tasks (Phase 3 — Could/polish)
- **[P3-01]** [ ] Polish EC: sugestão empty fixa, `*():{}=-/` sem 500, SSE disconnect sutil idempotente, 360px sem overflow, motion estático
  - **Depends on**: P2-02, P2-03 · **Artifact**: polish · **Enforcement**: EC-01..EC-08 · **Constraint Ref**: EC-01/EC-04/EC-06/EC-07/EC-08
- **[P3-02]** [ ] Re-gate final: textual CI + `diff --name-only` + Lighthouse + `cargo test -p brain-web`
  - **Depends on**: P3-01 · **Artifact**: gate log · **Enforcement**: VIEW-01/02/SEC-01/02/04 · **Constraint Ref**: §11 SPEC

**Estimativas**: P0 0.5 h · Phase 1 (Must) 4–6 h · Phase 2 (Should) 3–4 h · Phase 3 (Could) 1–2 h. Total 9–12 h (1 dev, inclui aceite manual).

## 8b. Script de aceite numerado (manual, ~5 s por item + Lighthouse; sem código produto aqui)
- **A-01 (RF-01)**: clicar 3 tabs → só 1 `aria-selected=true` por vez.
- **A-02 (RF-02)**: forçar loading/empty/error → spinner / mensagem+sugestão / banner+retry; `data-toast` ausente.
- **A-03 (RF-03)**: digitar → `Esc` (drawer fechado) → input vazio + `idle` + foco no input.
- **A-04 (RF-04)**: fluxos erro/vazio → `querySelector('[data-toast]') === null`.
- **A-05 (RF-05)**: fixture 500, Chromium 1280×720 → interativo ≤5 s + meta anexada.
- **A-06 (RF-06)**: fixture 500 a 1280px → contador ≤40 + overlap 0 na amostra de 40 visíveis.
- **A-07 (RF-07)**: assert 6 hex computados (`#0d1117 #c9d1d9 #58a6ff #3fb950 #f0883e #484f58`).
- **A-08 (RF-08)**: hub quadrado Guia 6px / nota círculo 3px / orfã vazada; alfa hover 0.55 (base 0.35).
- **A-09 (RF-09)**: fixture 1501 → anúncio visível + RAF layout parado + deriva 0.2×.
- **A-10 (RF-10)**: DOM tem título + badge + footer; footer = origem efetiva.
- **A-11 (RF-11)**: click nó → conteúdo = `/api/read` (id com espaço/`/` funciona); `Esc` → fechado + foco no invocador; 360px → bottom-sheet.
- **A-12 (RF-12)**: mock down → `degraded (N awaiting embed)`; abort → `offline`; precedência `offline>degraded>online`.
- **A-13 (RF-13)**: Lighthouse a11y ≥90; `aria-live` presente; `reduce-motion` → zero RAF animação.
- **A-SEC-01 (SEC-01)**: 3 payloads (`<img onerror>`, `<script>`, `javascript:` em nota + `layer`/`path` + erro) → texto literal, zero execução, zero request.
- **A-SEC-02 (SEC-02)**: 5 fetches abort 3 s → banner + badge; nenhum fetch sem timeout.
- **E-01..E-08**: EC-01 sugestão fixa · EC-02 down/timeout→banner+`offline`+`idle` recuperável · EC-03 freeze+anúncio · EC-04 motion estático · EC-05 XSS literal · EC-06 SSE reconnect 1 fonte · EC-07 `*():{}=-/` sem 500 · EC-08 360px sem overflow + `Esc`.
- **Gate textual CI** (grep no fonte, falha = bloqueia TASKS/Verify): `grep -c encodeURIComponent ≥1` (query/path/project); `grep -c escapeHtml ≥1`; `grep -E 'AbortSignal.timeout|AbortController' ≥1`; `grep -c '40' ≥1` (cap); `grep -c '0.55' ≥1` (alfa); 6 hex presentes; `aria-live` presente; `zero onclick interpolado` (`grep -c 'onclick=' == 0`); `grep -E 'eval\(|Function\(|document.write|outerHTML' == 0`; `git diff --name-only` só `viewer/index.html` (+ `.spec/`).

## 9. Event Implementation Mapping (MANDATORY)
| Event Name | Producer Task ID | Outbox Task ID | Broker Task ID | Consumer Task ID |
|---|---|---|---|---|
| `graph-changed` (reutilizado, nenhum evento novo — SPEC §6) | N/A justificado (producer `brain-web` existente) | N/A justificado (sem outbox, SSE polling 2 s existente) | N/A justificado (sem broker novo) | P1-04/P3-01 (`ensureGraphEventSource()` idempotente + reconnect, aceite E-06) |

## 10. Execution Readiness Checklist
- [x] All tasks have valid dependencies (no circular references — P0→P1→P2→P3).
- [x] All SPEC events generated the 4 mandatory tasks (Producer, Outbox, Broker, Consumer) — `graph-changed` reutilizado com N/A justificado + consumer task (P1-04/P3-01).
- [x] Mandatory testing tasks created for critical constraints — A-SEC-01 (XSS), A-SEC-02 (timeout 5 fetches), A-05/A-06 (perf/cap), A-13 (Lighthouse).
- [x] Base Infra tasks explicitly defined — P0-01 (brain-web verde + diff baseline), P0-02 (fixtures + Chromium 1280px).
- [x] Critical Execution Path maps only the core flow — §7 (MVP sem polish).
- [x] Traceability 1:1 — RF-01..RF-13 → P1-01..P2-03; EC-01..EC-08 → A/E + P3-01; SEC-01..04 → gates + A-SEC-01/02.

## Self-Validation contra `asserts/plan-gate.md`
| Gate | Status | Evidência |
|---|---|---|
| Traceability 1:1 | ✅ PASS | §8 cobre RF-01..13; §8b A-01..A-13 + E-01..E-08; §10 marcado |
| Constraint Mapping | ✅ PASS | §3 tabela (VIEW-01..03, SEC-01/02/04, RF-05/06/11/12/13) com enforcement + failure mode |
| Infra Bootstrap First | ✅ PASS | Phase 0 (P0-01/P0-02) precede domínio |
| Execution Graph | ✅ PASS | P0→P1→P2→P3; camadas infra→frontend; sem ciclo |
| Critical Execution Path | ✅ PASS | §7 MVP sem polish |
| 4-Task Event Lifecycle | ✅ PASS (N/A justificado) | §9 `graph-changed` reutilizado, consumer P1-04/P3-01 |
| Mandatory Testing Tasks | ✅ PASS | A-SEC-01/A-SEC-02/A-05/A-06/A-13 + P0-01 regressão backend |
| Failure Mode Testing | ✅ PASS | A-SEC-02, E-02/E-06, A-12 (abort/down/timeout) |
| Idempotency Validation | ✅ PASS (adaptado) | E-06 duplo `ensure` → 1 fonte (idempotência SSE, sem broker) |

**Gate: PASS.** Próximo passo: `generate-tasks` (1:1 PLAN→TASKS, TDD-ready).
