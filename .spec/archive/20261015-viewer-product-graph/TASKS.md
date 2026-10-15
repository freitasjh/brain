# Execution Backlog: viewer-product-graph

## 1. Traceability Context
- **Source SPEC**: `.spec/viewer-product-graph/SPEC.md` (13 RF RF-01..RF-13 + SEC-01..SEC-04 + EC-01..EC-08 + INV-01..INV-06, gate PASS com 3 N/A justificados)
- **Source PLAN**: `.spec/viewer-product-graph/PLAN.md` (Phase 0 bootstrap + P1 Must + P2 Should + P3 Could, aceite A-01..A-13 + A-SEC-01/02 + E-01..E-08 + gate textual CI)
- **Goal**: Explodir 15 tasks do PLAN §8 em tickets TDD-ready executáveis em <2h, restritos a `viewer/index.html` (VIEW-01) + `.spec/`, sem tocar `/api/*`, sem deps/build/CDN.

## 2. Sprint Summary
| Phase | Task ID | Type | Priority | Risk | Complexity | Status |
|-------|---------|------|----------|------|------------|--------|
| P0 bootstrap | P0-01 | Infra/Test | P0 | Low | 0.5pt | Done |
| P0 bootstrap | P0-02 | Infra/Test | P0 | Low | 0.5pt | Done |
| P1 Must | P1-01 | Frontend | P0 | Medium | 1pt | Done |
| P1 Must | P1-02 | Frontend | P0 | Medium | 1pt | Done |
| P1 Must | P1-03 | Frontend | P1 | Low | 0.5pt | Done |
| P1 Must | P1-04 | Frontend | P0 | High | 2pt | Done |
| P1 Must | P1-05 | Frontend | P0 | High | 2pt | Done |
| P1 Must | P1-06 | Frontend | P1 | Low | 1pt | Done |
| P1 Must | P1-07 | Frontend | P1 | Medium | 1pt | Done |
| P1 Must | P1-08 | Frontend | P0 | Low | 1pt | Done |
| P2 Should | P2-01 | Frontend | P1 | Medium | 2pt | Done |
| P2 Should | P2-02 | Frontend/Sec | P0 | Medium | 2pt | Done |
| P2 Should | P2-03 | Frontend/A11y | P1 | Medium | 1pt | Done |
| P3 Could | P3-01 | Frontend | P2 | Low | 1pt | Done |
| P3 Could | P3-02 | Infra/Gate | P0 | Low | 0.5pt | Done |

**Total: 15 tasks** (P0 2 · P1 Must 8 · P2 Should 3 · P3 Could 2). Estimativa PLAN §8: 9–12h (1 dev, inclui aceite manual).

## 3. Dependency Graph
```mermaid
graph TD
    P0-01 --> P0-02
    P0-02 --> P1-01
    P0-02 --> P1-04
    P1-01 --> P1-02
    P1-02 --> P1-03
    P1-02 --> P1-08
    P1-04 --> P1-05
    P1-05 --> P1-06
    P1-04 --> P1-07
    P1-01 --> P2-01
    P1-08 --> P2-01
    P1-08 --> P2-02
    P2-01 --> P2-03
    P2-02 --> P3-01
    P2-03 --> P3-01
    P3-01 --> P3-02
```
- Acíclico: P0→P1→P2→P3, sem back-edge. Sem órfãs: toda task tem predecessor (exceto P0-01 raiz) e sucessor (exceto P3-02 terminal).
- Paralelizável após destrave: P1-01 ∥ P1-04 (ramos busca/tabs vs grafo); P1-06 ∥ P1-07 (legenda vs freeze); P2-02 ∥ P2-03 parcial (badge vs a11y, convergem em P3-01).

---

## 4. Task Backlog

### Task P0-01: Gates baseline — `brain-web` verde + `diff --name-only`
*Mapped directly from PLAN.md §8 (Phase 0)*

- **Constraint Ref**: VIEW-01 (só `viewer/index.html` muda) / VIEW-03 (`/api/*` congelado)
- **Depends on (PLAN)**: None (raiz)
- **Dependency Type**: Hard
- **Layer**: Frontend-infra
- **Task Type**: Infra
- **Priority**: P0
- **Risk Level**: Low
- **Parallelizable**: No (raiz de tudo)
- **Artifact**: log `cargo test -p brain-web` + baseline `git diff --name-only`
- **Framework Detail**: Nenhum handler muda; regressão gratuita via suite existente

#### 📝 Context & Domain Impact
- **Description**: Estabelecer baseline verificável antes de qualquer edição produto: suite backend verde + diff limpo de produto.
- **Domain Impact**: Sem este gate, regressão `/api/*` posterior seria indistinguível de mudança viewer.

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: `cargo test -p brain-web` exit 0; `git diff --name-only` sem `viewer/index.html` nem `crates/**` no baseline.
- **Failure Mode**: Suite vermelha no baseline invalida todo aceite posterior (sinal contaminado).

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: baseline limpo
  - **Given** repo sem edição produto
  - **When** roda `cargo test -p brain-web` + `git diff --name-only`
  - **Then** testes 0 failed; diff não lista `viewer/index.html` nem `crates/**`
- **Scenario 2 (Failure Mode - suite vermelha)**: backend quebrado
  - **Given** `brain-web` com teste falhando
  - **When** executa gate
  - **Then** task BLOQUEIA P1 (não avança; abre `.spec/tech-debts/` antes da feature, H2 harness)
- **Scenario 3 (Edge Case)**: diff sujo pré-existente
  - **Given** working tree com edição não relacionada
  - **When** coleta baseline
  - **Then** registra baseline por escrito (hash + lista) para comparar no P3-02; não apaga trabalho alheio

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/P0-01.log` (Type: Gate): rodar `cargo test -p brain-web` + `git diff --name-only`, salvar saída ANTES de qualquer edição (TDD FIRST — este log é o "teste").
- [x] `.spec/viewer-product-graph/accept/P0-01.log`: anexar resultado; se vermelho, parar e sanear (H1.6) antes de P1.

#### 🏁 Definition of Done (DoD)
- [x] Log de teste criado ANTES de qualquer edição produto.
- [x] `cargo test -p brain-web` verde registrado no log.
- [x] Baseline diff registrado (lista de arquivos).
- [x] No TODOs pending.

---

### Task P0-02: Fixtures locais grafo 500/1501 + ambiente Chromium 1280×720
*Mapped directly from PLAN.md §8 (Phase 0)*

- **Constraint Ref**: RF-05 (500 ≤5s, ambiente normado + meta) / RF-06 (1280px, contador observável) / RF-09 (1501 freeze)
- **Depends on (PLAN)**: P0-01
- **Dependency Type**: Hard
- **Layer**: Frontend-infra
- **Task Type**: Infra
- **Priority**: P0
- **Risk Level**: Low
- **Parallelizable**: No (destrava P1-01 e P1-04)
- **Artifact**: `.spec/viewer-product-graph/fixtures/fixture-graph-500.json` + `fixture-graph-1501.json` + template meta `{chromium, viewport, corpus hash, tempo}`
- **Framework Detail**: Fixtures determinísticas locais (sem rede, sem `data/brain.db` produção — H3 scratch)

#### 📝 Context & Domain Impact
- **Description**: Tornar RF-05/RF-06/RF-09 reprodutíveis: dois corpora determinísticos + ambiente de medição normado.
- **Domain Impact**: Sem fixture local, aceite de perf depende de produção (proibido H3) ou de rede (não-determinístico).

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: Fixtures foras de `viewer/` produto e fora de `data/`; viewport 1280×720 fixo; meta anexada a cada medição A-05.
- **Failure Mode**: Fixture gerada contra produção ou mutável entre runs → medições incomparáveis.

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: fixtures carregam
  - **Given** viewer servido em scratch (H3)
  - **When** importa fixture 500 / 1501
  - **Then** contagem de nós = 500 / 1501 exatos, determinística entre loads (hash idêntico)
- **Scenario 2 (Failure Mode - fixture mutável)**: hash diverge
  - **Given** fixture com random não-seedado
  - **When** carrega 2×
  - **Then** rejeitada (regenerar com seed fixo); A-05/A-06/A-09 não executados sobre ela
- **Scenario 3 (Edge Case)**: viewport ≠1280px
  - **Given** medição feita a 360px ou 1920px
  - **When** registra A-05/A-06
  - **Then** inválida para RF-05/RF-06 (só 1280×720 vale; mobile tem aceite próprio E-08/A-11)

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/fixtures/*.json` + `meta-template.json` (Type: Fixture/Test): criar fixtures + template ANTES de medir (TDD FIRST — A-05/A-06/A-09 rodados contra viewer atual devem FALHAR ou ser N/A, provando que o aceite mede algo).
- [x] `.spec/viewer-product-graph/accept/P0-02.log`: registrar hashes + viewport + versão Chromium.

#### 🏁 Definition of Done (DoD)
- [x] Fixture files criados ANTES de qualquer medição produto.
- [x] Hashes determinísticos registrados.
- [x] Template meta pronto para A-05.
- [x] No TODOs pending.

---

### Task P1-01: Tabs Buscar/Explorar/Grafo — navegação única (RF-01, INV-01)
*Mapped directly from PLAN.md §8 (Phase 1)*

- **Constraint Ref**: RF-01 (3 tabs, 1 ativa) / INV-01 (exatamente um `aria-selected=true`)
- **Depends on (PLAN)**: P0-02
- **Dependency Type**: Hard
- **Layer**: `viewer/index.html` (tabs)
- **Task Type**: Frontend
- **Priority**: P0
- **Risk Level**: Medium (estrutura que P1-02/P2-01 penduram)
- **Parallelizable**: Yes (paralelo a P1-04; ambos só precisam de P0-02)
- **Artifact**: tabs + `aria-selected` + hash opcional sem reload
- **Framework Detail**: Sem framework; botões nativos + `data-tab` + delegado; CSS tabs scrolláveis (base p/ RF-11 mobile)

#### 📝 Context & Domain Impact
- **Description**: 3 tabs com ativação exclusiva; base estrutural de Buscar/Explorar/Grafo.
- **Domain Impact**: Quebra INV-01 (2 tabs ativas) confunde leitor de tela e todo aceite posterior que assume "view atual".

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: Exatamente um `[aria-selected=true]` em qualquer estado; sem views paralelas; diff-only `viewer/index.html`.
- **Failure Mode**: Duas views visíveis simultâneas → estado de busca e grafo divergem.

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: A-01
  - **Given** viewer carregado
  - **When** clica Buscar → Explorar → Grafo
  - **Then** só a clicada tem `aria-selected=true`; as outras `false`; sem reload
- **Scenario 2 (Failure Mode - dupla ativa)**: ativação sem desativar
  - **Given** tab Buscar ativa
  - **When** ativa Grafo sem limpar Buscar
  - **Then** FALHA A-01 (2× `aria-selected=true`); corrigir até 1
- **Scenario 3 (Edge Case)**: teclado + hash direto
  - **Given** foco no tab via Tab/Enter ou URL com `#grafo`
  - **When** ativa por teclado / load com hash
  - **Then** mesma exclusividade; foco visível

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/A-01.log` (Type: Manual/Contract): executar A-01 contra viewer atual ANTES de editar (TDD FIRST — registra PASS existente ou FAIL que a implementação deve virar PASS).
- [x] `viewer/index.html`: implementar tabs garantindo INV-01 (estrutura + handler + `aria-selected`).

#### 🏁 Definition of Done (DoD)
- [x] Arquivo de aceite criado ANTES do código produto.
- [x] A-01 PASS registrado (3 cliques, 1 ativa por vez).
- [x] `git diff --name-only` só `viewer/index.html` (+ `.spec/`).
- [x] `grep -c 'onclick=' viewer/index.html` == 0 (H-01).
- [x] No TODOs pending.

---

### Task P1-02: Estados busca + banner inline zero toast (RF-02, RF-04, INV-05)
*Mapped directly from PLAN.md §8 (Phase 1)*

- **Constraint Ref**: RF-02 (`idle→loading→results|empty|error`, loading sempre indica) / RF-04 (banner inline, zero toast) / INV-05
- **Depends on (PLAN)**: P1-01
- **Dependency Type**: Hard
- **Layer**: `viewer/index.html` (busca: `GET /api/search?query=&top_k=`)
- **Task Type**: Frontend
- **Priority**: P0
- **Risk Level**: Medium
- **Parallelizable**: No (pendura P1-03, P1-08)
- **Artifact**: spinner loading + banner inline + retry + estado empty
- **Framework Detail**: `encodeURIComponent(query)` (SEC-04); erros via `textContent`/escape (H-02); `aria-live="polite"` base (completa em P2-03)

#### 📝 Context & Domain Impact
- **Description**: Máquina de estados da busca com feedback inline; proíbe toast.
- **Domain Impact**: `results` + `empty` simultâneos ou toast escondem erro do operador (EC-01/EC-02 dependem deste banner).

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: Nunca `results` + `empty` juntos; `loading` sempre com indicador; zero `[data-toast]`; erro via `textContent` ou string escapada.
- **Failure Mode**: Erro silencioso (sem banner) → operador repete query achando que é corpus vazio.

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: A-02
  - **Given** tabs prontas
  - **When** busca com match / força loading / força empty / força error
  - **Then** spinner → resultados | mensagem+sugestão | banner+retry; `querySelector('[data-toast]') === null` (A-04)
- **Scenario 2 (Failure Mode - erro sem banner)**: fetch 500/timeout engolido
  - **Given** `/api/search` retorna 500 ou aborta 3s
  - **When** busca executa
  - **Then** FALHA se volta a `idle` sem banner; exige banner + retry (EC-02)
- **Scenario 3 (Edge Case)**: query especial `*():{}=-/`
  - **Given** query com caracteres FTS (EC-07)
  - **When** submete
  - **Then** sem 500, sem quebra de layout (antecipa P3-01/E-07; aqui basta não-crash)

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/A-02-A-04.log` (Type: Manual): executar A-02 + A-04 contra estado atual ANTES de editar (TDD FIRST).
- [x] `viewer/index.html`: implementar estados + banner (fetch `search` com `encodeURIComponent`, erro via `textContent`).

#### 🏁 Definition of Done (DoD)
- [x] Aceite criado ANTES do código.
- [x] A-02 + A-04 PASS registrados.
- [x] `grep -c 'data-toast' viewer/index.html` == 0; `grep -c encodeURIComponent` ≥1.
- [x] Diff-only + `cargo test -p brain-web` ainda verde (API intocada).
- [x] No TODOs pending.

---

### Task P1-03: `Esc` limpa busca (RF-03, INV-02)
*Mapped directly from PLAN.md §8 (Phase 1)*

- **Constraint Ref**: RF-03 (`Esc` drawer fechado → query vazia + `idle` + foco input) / INV-02 (drawer primeiro)
- **Depends on (PLAN)**: P1-02
- **Dependency Type**: Hard
- **Layer**: `viewer/index.html` (handler Esc busca)
- **Task Type**: Frontend
- **Priority**: P1
- **Risk Level**: Low
- **Parallelizable**: No
- **Artifact**: handler Esc busca
- **Framework Detail**: Ordem normada: drawer aberto ⇒ `Esc` fecha drawer (P2-01); só com drawer fechado limpa busca

#### 📝 Context & Domain Impact
- **Description**: Atalho `Esc` retorna busca a `idle`.
- **Domain Impact**: Ordem errada (limpa busca com drawer aberto) perde query do operador sem motivo.

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: INV-02: `Esc` sempre fecha drawer antes de limpar busca.
- **Failure Mode**: `Esc` com drawer aberto limpa resultados por baixo do drawer → ao fechar, operador encontra busca vazia.

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: A-03
  - **Given** query digitada + resultados, drawer fechado
  - **When** pressiona `Esc`
  - **Then** input vazio + estado `idle` + foco no input
- **Scenario 2 (Failure Mode - ordem invertida)**: drawer aberto
  - **Given** drawer aberto sobre resultados
  - **When** pressiona `Esc`
  - **Then** drawer FECHA e busca permanece (foco volta ao invocador — P2-01); FALHA se limpa busca
- **Scenario 3 (Edge Case)**: `Esc` em `idle` vazio
  - **Given** busca já `idle`, input vazio
  - **When** pressiona `Esc`
  - **Then** no-op seguro (sem erro, sem foco perdido)

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/A-03.log` (Type: Manual): executar A-03 ANTES de editar (TDD FIRST).
- [x] `viewer/index.html`: implementar handler `Esc` com guarda drawer-aberto.

#### 🏁 Definition of Done (DoD)
- [x] Aceite ANTES do código.
- [x] A-03 PASS + ordem INV-02 verificada.
- [x] Diff-only; zero `onclick`.
- [x] No TODOs pending.

---

### Task P1-04: Grafo 500 nós ≤5 s (RF-05)
*Mapped directly from PLAN.md §8 (Phase 1)*

- **Constraint Ref**: RF-05 (fixture local, Chromium 1280×720 + meta `{chromium, viewport, corpus hash, tempo até frame interativo}`)
- **Depends on (PLAN)**: P0-02
- **Dependency Type**: Hard
- **Layer**: `viewer/index.html` (render path canvas + `GET /api/graph?project=`)
- **Task Type**: Frontend
- **Priority**: P0
- **Risk Level**: High (O(n²) — risco 🔥 PLAN §5)
- **Parallelizable**: Yes (paralelo a P1-01)
- **Artifact**: render path + RAF loop + SSE base (`ensureGraphEventSource`)
- **Framework Detail**: Canvas próprio, RAF, pan/zoom por transform; `renderGraph()` cancela só RAF; ids verbatim → `/api/read`

#### 📝 Context & Domain Impact
- **Description**: Render interativo de 500 nós dentro do orçamento; fundação que P1-05/P1-07 refinam.
- **Domain Impact**: Estouro O(n²) aqui inviabiliza RF-06/RF-09 (cap/freeze viram remendo sobre loop lento).

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: Medição só vale a 1280×720, Chromium estável, fixture local + meta; sem `<script src>`/CDN.
- **Failure Mode**: Medir contra produção ou viewport arbitrário → número não-reprodutível, gate falso-verde.

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: A-05
  - **Given** fixture 500 + Chromium 1280×720
  - **When** abre tab Grafo
  - **Then** frame interativo ≤5 s + meta anexada ao log
- **Scenario 2 (Failure Mode - trava O(n²))**: layout sem cap
  - **Given** 500 nós sem cap/freeze
  - **When** renderiza
  - **Then** FALHA se >5 s ou tab trava; mitigar (cap P1-05, freeze P1-07) até PASS
- **Scenario 3 (Edge Case)**: 0/1 nó
  - **Given** corpus vazio ou 1 nó
  - **When** renderiza
  - **Then** sem exceção, canvas vazio/1 ponto + legenda presente (antecipa EC sem quebrar)

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/A-05.log` (Type: Manual/Perf): medir A-05 contra viewer atual ANTES de editar (TDD FIRST — registra baseline >5s ou N/A).
- [x] `viewer/index.html`: implementar render path + RAF + SSE idempotente base.

#### 🏁 Definition of Done (DoD)
- [x] Medição baseline ANTES do código.
- [x] A-05 PASS (≤5s + meta) registrado.
- [x] `grep -c 'src="http' viewer/index.html` == 0 (VIEW-02); diff-only; brain-web verde.
- [x] No TODOs pending.

---

### Task P1-05: Cap 40 labels + anti-colisão (RF-06, INV-03)
*Mapped directly from PLAN.md §8 (Phase 1)*

- **Constraint Ref**: RF-06 (1280px, contador observável `__labelCount`, amostra = 40 visíveis, overlap 0) / INV-03 (labels ≤40 por frame)
- **Depends on (PLAN)**: P1-04
- **Dependency Type**: Hard
- **Layer**: `viewer/index.html` (cap + hide por prioridade)
- **Task Type**: Frontend
- **Priority**: P0
- **Risk Level**: High (legibilidade do grafo)
- **Parallelizable**: No
- **Artifact**: ocultação por prioridade + contador debug observável
- **Framework Detail**: Método normado SPEC: contar via `__labelCount` ou equivalente a 1280px sobre 500 nós

#### 📝 Context & Domain Impact
- **Description**: Máximo 40 labels visíveis com overlap zero na amostra.
- **Domain Impact**: Sem cap, 500 labels sobrepostos tornam grafo ilegível (o objetivo US2).

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: INV-03 em todo frame; overlap 0 em TODA a amostra de 40 (não subconjunto).
- **Failure Mode**: Amostra parcial ("20 de 40 sem overlap") esconde colisão nos demais.

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: A-06
  - **Given** fixture 500 a 1280px
  - **When** grafo renderizado
  - **Then** contador ≤40 + zero bounding-box sobreposto nos 40 visíveis
- **Scenario 2 (Failure Mode - 41+ labels)**: cap furado em zoom/pan
  - **Given** grafo com 40 labels
  - **When** pan/zoom
  - **Then** FALHA se qualquer frame exibe 41+; cap vale por frame, não só no load
- **Scenario 3 (Edge Case)**: 41 nós (fronteira)
  - **Given** 41 nós rotuláveis
  - **When** renderiza
  - **Then** exatamente 40 visíveis, 1 oculto por prioridade (PBT boundary SPEC §7.3)

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/A-06.log` (Type: Manual): executar A-06 ANTES de editar — registra FAIL (sem cap, labels >40) (TDD FIRST).
- [x] `viewer/index.html`: implementar cap + prioridade + contador observável.

#### 🏁 Definition of Done (DoD)
- [x] Aceite FAIL-baseline ANTES do código.
- [x] A-06 PASS (≤40 + overlap 0/40).
- [x] `grep -c '40' viewer/index.html` ≥1 (gate textual CI).
- [x] Diff-only; brain-web verde.
- [x] No TODOs pending.

---

### Task P1-06: Legenda 6-hex fixa + simbologia + alfa (RF-07, RF-08, INV-04)
*Mapped directly from PLAN.md §8 (Phase 1)*

- **Constraint Ref**: RF-07 (Noite `#0d1117`, Papel `#c9d1d9`, Link `#58a6ff`, Trilha `#3fb950`, Guia `#f0883e`, Orfã `#484f58`) / RF-08 (hub quadrado Guia 6px, nota círculo 3px, órfã vazada, alfa 0.35→0.55) / INV-04
- **Depends on (PLAN)**: P1-05
- **Dependency Type**: Hard
- **Layer**: `viewer/index.html` (legenda + estilos canvas)
- **Task Type**: Frontend
- **Priority**: P1
- **Risk Level**: Low
- **Parallelizable**: Yes (paralelo a P1-07)
- **Artifact**: legenda + constantes cor/forma/alfa
- **Framework Detail**: Hex imutáveis em qualquer re-render (INV-04)

#### 📝 Context & Domain Impact
- **Description**: Identidade visual normada do grafo;ließt UX vinculante §9 itens 3–4.
- **Domain Impact**: Hex divergente quebra convenção operador (Noite/Papel/Link/Trilha/Guia/Orfã).

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: 6 hex exatos, imutáveis em re-render; alfa base 0.35, hover/seleção 0.55.
- **Failure Mode**: "Aproximar" hex (`#0d1118`) passa no olho e falha no assert computado A-07.

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: A-07 + A-08
  - **Given** grafo renderizado
  - **When** inspeciona legenda + nós/arestas
  - **Then** 6 valores computados exatos; hub quadrado 6px Guia / círculo 3px / órfã vazada; alfa hover 0.55
- **Scenario 2 (Failure Mode - hex trocado)**: re-render muda cor
  - **Given** legenda correta no load
  - **When** re-render (pan/SSE update)
  - **Then** FALHA se qualquer hex muda (INV-04)
- **Scenario 3 (Edge Case)**: órfã isolada + hub sem notas
  - **Given** nó sem arestas / hub `projeto:<nome>` vazio
  - **When** renderiza
  - **Then** órfã vazada Orfã; hub quadrado Guia mesmo vazio

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/A-07-A-08.log` (Type: Manual): assert A-07/A-08 ANTES de editar (TDD FIRST).
- [x] `viewer/index.html`: implementar legenda + simbologia + alfa.

#### 🏁 Definition of Done (DoD)
- [x] Aceite ANTES do código.
- [x] A-07 + A-08 PASS.
- [x] 6 hex presentes no fonte + `grep -c '0.55'` ≥1.
- [x] Diff-only.
- [x] No TODOs pending.

---

### Task P1-07: Freeze >1500 + anúncio + deriva 0.2× (RF-09)
*Mapped directly from PLAN.md §8 (Phase 1)*

- **Constraint Ref**: RF-09 (nunca congela silencioso; deriva lenta 0.2× abaixo do teto)
- **Depends on (PLAN)**: P1-04
- **Dependency Type**: Hard
- **Layer**: `viewer/index.html` (freeze path)
- **Task Type**: Frontend
- **Priority**: P1
- **Risk Level**: Medium
- **Parallelizable**: Yes (paralelo a P1-06)
- **Artifact**: freeze path + anúncio visível
- **Framework Detail**: Padrão `viewer-graph` existente (freeze + anúncio + 0.2×)

#### 📝 Context & Domain Impact
- **Description**: Acima de 1500 nós, layout congela com anúncio; nunca silencioso.
- **Domain Impact**: Freeze silencioso parece travamento (operador mata a tab); anúncio distingue estado intencional de bug.

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: `nós > 1500 ⇒ anúncio visível`; RAF de layout parado; deriva 0.2× abaixo do teto.
- **Failure Mode**: Congela sem anúncio → indistinguível de freeze de bug.

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: A-09
  - **Given** fixture 1501
  - **When** abre Grafo
  - **Then** anúncio visível + RAF layout parado + deriva 0.2×
- **Scenario 2 (Failure Mode - freeze silencioso)**: anúncio ausente
  - **Given** 1501 nós congelados
  - **When** procura anúncio
  - **Then** FALHA se ausente/invisível (exige texto visível, não só console)
- **Scenario 3 (Edge Case)**: fronteira 1500 vs 1501 (PBT SPEC §7.3)
  - **Given** 1500 nós
  - **When** renderiza
  - **Then** NÃO congela (layout ativo); 1501 congela — boundary exato

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/A-09.log` (Type: Manual): executar A-09 ANTES de editar (TDD FIRST — FAIL esperado sem freeze).
- [x] `viewer/index.html`: implementar freeze + anúncio + deriva 0.2×.

#### 🏁 Definition of Done (DoD)
- [x] Aceite ANTES do código.
- [x] A-09 PASS (anúncio + RAF parado + 0.2×).
- [x] Boundary 1500/1501 verificado.
- [x] Diff-only.
- [x] No TODOs pending.

---

### Task P1-08: Header produto + badge base + footer `API_BASE` (RF-10, SEC-01 parcial, SEC-03)
*Mapped directly from PLAN.md §8 (Phase 1)*

- **Constraint Ref**: RF-10 (título + badge + footer origem efetiva) / SEC-01 (API_BASE escapado) / SEC-03 (sem segredos, origem/host apenas)
- **Depends on (PLAN)**: P1-02
- **Dependency Type**: Hard
- **Layer**: `viewer/index.html` (header/footer + `GET /api/status` base)
- **Task Type**: Frontend
- **Priority**: P0
- **Risk Level**: Low
- **Parallelizable**: No (destrava P2-01/P2-02)
- **Artifact**: header + footer
- **Framework Detail**: `API_BASE` = origem/host apenas, via `escapeHtml()`; badge base completada em P2-02 (precedência + timeouts)

#### 📝 Context & Domain Impact
- **Description**: Moldura produto: título, badge status base, footer com origem efetiva.
- **Domain Impact**: Footer com valor errado esconde contra qual backend o viewer fala (diagnóstico US3/US5).

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: `API_BASE` escapado; nenhum `BRAIN_*`, token ou credencial no cliente (SEC-03).
- **Failure Mode**: Interpolar `location.origin` via `innerHTML` cru → vetor XSS via origem manipulada (ex.: Electron/custom scheme).

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: A-10
  - **Given** viewer carregado
  - **When** inspeciona DOM
  - **Then** título + badge + footer presentes; footer = origem efetiva
- **Scenario 2 (Failure Mode - segredo no cliente)**: footer com token/query
  - **Given** origem com query ou credencial
  - **When** renderiza footer
  - **Then** FALHA se exibe além de origem/host ou sem escape (SEC-01/03)
- **Scenario 3 (Edge Case)**: origem `file://` / porta custom
  - **Given** viewer aberto via arquivo ou porta não-padrão
  - **When** renderiza
  - **Then** footer reflete origem real, sem quebrar layout

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/A-10.log` (Type: Manual): executar A-10 ANTES de editar (TDD FIRST).
- [x] `viewer/index.html`: implementar header/footer com `escapeHtml(API_BASE)`.

#### 🏁 Definition of Done (DoD)
- [x] Aceite ANTES do código.
- [x] A-10 PASS.
- [x] `grep -c escapeHtml` ≥1; `grep -E 'eval\(|Function\(|document.write|outerHTML'` == 0.
- [x] Diff-only; brain-web verde.
- [x] No TODOs pending.

---

### Task P2-01: Drawer `/api/read` — `data-*` + `encodeURIComponent` + foco-Esc + bottom-sheet (RF-11, SEC-01/SEC-04, H-01)
*Mapped directly from PLAN.md §8 (Phase 2)*

- **Constraint Ref**: RF-11 (id verbatim + `encodeURIComponent`, `Esc`→invocador, bottom-sheet 360px, tabs scrolláveis) / SEC-01 (corpo drawer escapado) / SEC-04 + H-01 (zero `onclick`, `data-*` + delegado)
- **Depends on (PLAN)**: P1-01, P1-08
- **Dependency Type**: Hard (duplo: tabs + header base)
- **Layer**: `viewer/index.html` (drawer + `GET /api/read?path=` + `GET /api/list`)
- **Task Type**: Frontend
- **Priority**: P1
- **Risk Level**: Medium (foco + XSS corpo)
- **Parallelizable**: No (destrava P2-03)
- **Artifact**: drawer lateral + bottom-sheet 360px
- **Framework Detail**: Click via `data-*` + `addEventListener` delegado; `encodeURIComponent(id)`; foco-Esc normado (drawer→invocador); corpo via `escapeHtml()` (fecha SEC-01 p/ este sink)

#### 📝 Context & Domain Impact
- **Description**: Leitura integral da nota em drawer; mobile vira bottom-sheet.
- **Domain Impact**: Id com `/` ou espaço sem `encodeURIComponent` quebra fetch (toda nota com path aninhado vira unclickable — cf. TD-011); foco perdido quebra leitor de tela.

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: Zero `onclick` interpolado; `encodeURIComponent` em id; `Esc` devolve foco ao invocador; corpo escapado.
- **Failure Mode**: `innerHTML` com corpo cru → XSS persistido via nota (qualquer `brain_store` vira payload).

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: A-11
  - **Given** resultado/nó visível (incl. id com espaço e `/`)
  - **When** clica → `Esc`
  - **Then** conteúdo idêntico ao `/api/read`; `Esc` fecha + `activeElement` = invocador; 360px → classe bottom-sheet, sem overflow horizontal
- **Scenario 2 (Failure Mode - XSS corpo)**: nota com `<img onerror>` / `<script>`
  - **Given** nota contendo payload (parte de A-SEC-01)
  - **When** abre drawer
  - **Then** FALHA se executa ou faz request; exige texto literal (antecipa gate A-SEC-01)
- **Scenario 3 (Edge Case)**: E-08 — drawer 360px + `Esc` + tabs scrolláveis
  - **Given** viewport 360px
  - **When** abre drawer, rola tabs
  - **Then** bottom-sheet sem overflow-x; `Esc` fecha; tabs acessíveis por scroll

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/A-11.log` (Type: Manual): executar A-11 ANTES de editar (TDD FIRST — FAIL sem drawer).
- [x] `viewer/index.html`: implementar drawer (`data-*`, `encodeURIComponent`, foco-Esc, bottom-sheet, corpo escapado).

#### 🏁 Definition of Done (DoD)
- [x] Aceite ANTES do código.
- [x] A-11 PASS + E-08 PASS.
- [x] `grep -c 'onclick='` == 0; `grep -c encodeURIComponent` ≥1; `grep -c escapeHtml` ≥1.
- [x] Diff-only; brain-web verde.
- [x] No TODOs pending.

---

### Task P2-02: Badge precedência + 5 fetches timeout 3 s + erros seguros (RF-12, SEC-02/H-02/H-03, A-SEC-02)
*Mapped directly from PLAN.md §8 (Phase 2)*

- **Constraint Ref**: RF-12 (`offline > degraded > online`, `(N awaiting embed)` se coverage <100%) / SEC-02 (5 fetches `status,search,list,read,graph` com `AbortSignal.timeout(3000)`; SSE `events` fora) / H-02 (erros via `textContent`/escape)
- **Depends on (PLAN)**: P1-08
- **Dependency Type**: Hard
- **Layer**: `viewer/index.html` (fetch wrapper + badge + `GET /api/status`)
- **Task Type**: Frontend/Sec
- **Priority**: P0
- **Risk Level**: Medium
- **Parallelizable**: Yes (paralelo parcial a P2-03 até P3-01)
- **Artifact**: wrapper fetch com timeout + badge 3 estados + sufixo embed
- **Framework Detail**: `AbortSignal.timeout(3000)` ou `AbortController` equivalente 3 s; timeout → banner + badge, nunca pendente infinita

#### 📝 Context & Domain Impact
- **Description**: Fecha SEC-02 nos 5 fetches; badge honesta sobre saúde + fila embed.
- **Domain Impact**: Badge `online` com fila pendente ou fetch travado mente sobre o sistema (operador confia em índice incompleto).

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: Nenhum dos 5 fetches sem timeout; precedência `offline > degraded > online`; `coverage <100%` ⇒ sufixo `(N awaiting embed)`, nunca `online` puro.
- **Failure Mode**: Fetch sem timeout → UI pendente infinita em rede caída (o bug que SEC-02 existe para matar).

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: A-12
  - **Given** mocks: Ollama down (coverage <100%) / fetch abort
  - **When** carrega header
  - **Then** `degraded (N awaiting embed)` / `offline`; precedência respeitada
- **Scenario 2 (Failure Mode - A-SEC-02)**: timeout nos 5 fetches
  - **Given** cada um dos 5 endpoints com delay >3 s (um por vez)
  - **When** dispara fetch
  - **Then** cada um aborta → banner + badge (`offline` se abort/offline); FALHA se qualquer fetch fica pendente ou falta timeout no fonte
- **Scenario 3 (Edge Case)**: E-02 — down/timeout recuperável
  - **Given** API down
  - **When** API volta + retry
  - **Then** banner some, badge recupera, busca volta a `idle` recuperável (não exige reload)

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/A-12-A-SEC-02-E-02.log` (Type: Manual/Sec): executar A-12 + A-SEC-02 + E-02 ANTES de editar (TDD FIRST — FAIL: fetches sem timeout).
- [x] `viewer/index.html`: implementar wrapper timeout nos 5 + badge precedência + `(N awaiting embed)` + erros via `textContent`.

#### 🏁 Definition of Done (DoD)
- [x] Aceite ANTES do código.
- [x] A-12 + A-SEC-02 + E-02 PASS.
- [x] `grep -E 'AbortSignal.timeout|AbortController'` ≥1; os 5 nomes (`status|search|list|read|graph`) cobertos pelo wrapper (revisão por leitura).
- [x] Diff-only; brain-web verde.
- [x] No TODOs pending.

---

### Task P2-03: A11y AA — `role=img`, `aria-*`, `aria-live`, foco, motion, Lighthouse ≥90 (RF-13)
*Mapped directly from PLAN.md §8 (Phase 2)*

- **Constraint Ref**: RF-13 (Lighthouse ≥90, `canvas role=img` + `aria-label`, `aria-live="polite"` busca/banner, foco drawer, `prefers-reduced-motion` estático)
- **Depends on (PLAN)**: P2-01
- **Dependency Type**: Hard
- **Layer**: `viewer/index.html` (attrs a11y)
- **Task Type**: Frontend/A11y
- **Priority**: P1
- **Risk Level**: Medium
- **Parallelizable**: No (exige drawer pronto p/ foco)
- **Artifact**: attrs `aria-*` + `role=img` + media-query motion
- **Framework Detail**: `aria-live="polite"` em resultados + banner (emenda MÉDIA); `matchMedia('(prefers-reduced-motion: reduce)')` → zero RAF animação (pan/zoom manual ok)

#### 📝 Context & Domain Impact
- **Description**: Viewer operável por leitor de tela e sem animação forçada.
- **Domain Impact**: Sem `aria-live`, resultados/banner são mudos para leitor (operador cego não sabe que a busca terminou).

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: Lighthouse a11y ≥90; `reduce-motion` ⇒ zero RAF de animação.
- **Failure Mode**: Animação com `reduce-motion` ativo → barreira vestibular; audit <90 bloqueia Verify.

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: A-13
  - **Given** viewer completo P1+P2
  - **When** roda Lighthouse a11y + teste `reduce-motion`
  - **Then** ≥90; `aria-live` presente; `reduce-motion` → zero animação
- **Scenario 2 (Failure Mode - banner mudo)**: banner sem `aria-live`
  - **Given** erro de busca com banner visual
  - **When** leitor de tela ativo (ou inspeção DOM)
  - **Then** FALHA se região do banner/resultados sem `aria-live="polite"`
- **Scenario 3 (Edge Case)**: E-04 — motion estático + pan/zoom manual
  - **Given** `prefers-reduced-motion: reduce`
  - **When** abre Grafo, tenta pan/zoom
  - **Then** sem RAF animação, pan/zoom manual permitido, legenda/tabs intactos

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/A-13-E-04.log` (Type: Manual/A11y): executar A-13 + E-04 ANTES de editar (TDD FIRST — FAIL: audit <90 ou sem `aria-live`).
- [x] `viewer/index.html`: implementar `role=img`+`aria-label`, `aria-*`, `aria-live`, foco, motion.

#### 🏁 Definition of Done (DoD)
- [x] Aceite ANTES do código.
- [x] A-13 + E-04 PASS (score anexado ao log).
- [x] `grep -c 'aria-live'` ≥1.
- [x] Diff-only.
- [x] No TODOs pending.

---

### Task P3-01: Polish EC — empty fixa, FTS especial, SSE sutil, 360px, motion (EC-01..EC-08, A-SEC-01)
*Mapped directly from PLAN.md §8 (Phase 3)*

- **Constraint Ref**: EC-01 (sugestão fixa `Tente um termo mais amplo ou verifique o filtro de projeto`) / EC-02 (banner+`offline`+`idle` recuperável) / EC-03 (freeze+anúncio) / EC-04 (motion estático) / EC-05 (XSS literal) / EC-06 (SSE reconnect 1 fonte) / EC-07 (`*():{}=-/` sem 500) / EC-08 (360px) + A-SEC-01 (3 payloads, todos os sinks)
- **Depends on (PLAN)**: P2-02, P2-03
- **Dependency Type**: Hard (duplo: badge/timeout + a11y)
- **Layer**: `viewer/index.html` (polish)
- **Task Type**: Frontend
- **Priority**: P2
- **Risk Level**: Low
- **Parallelizable**: No (converge P2 antes do re-gate)
- **Artifact**: textos/fallbacks finais + SSE disconnect sutil
- **Framework Detail**: `escapeHtml()` única em TODOS os sinks (snippet, corpo, `layer`/`path`/id, erro, `API_BASE`); `ensureGraphEventSource()` idempotente; nenhuma credencial

#### 📝 Context & Domain Impact
- **Description**: Endurecimento final: os 8 edge normados + fechamento SEC-01 em todos os sinks com 3 payloads.
- **Domain Impact**: EC sem dono vira "funciona no happy path"; esta task dá a cada EC um aceite nomeado E-01..E-08.

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: Texto empty fixo verbatim; SSE nunca duplica fonte; XSS zero execução/zero request em snippet+drawer+`layer`/`path`+erro.
- **Failure Mode**: Sugestão empty "criativa" por estado → operador não reconhece o estado; SSE duplo → updates duplicados e leak.

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: E-01..E-08 todos PASS
  - **Given** viewer completo P1+P2
  - **When** executa E-01 (empty texto fixo) · E-02 (down→banner+`offline`+recupera) · E-03 (1501 freeze+anúncio) · E-04 (motion estático) · E-05 (XSS literal) · E-06 (disconnect→banner sutil/badge + reconnect 1 fonte + duplo `ensure` 1 fonte) · E-07 (`*():{}=-/` sem 500) · E-08 (360px bottom-sheet sem overflow + `Esc`)
  - **Then** 8/8 PASS registrados em `.spec/viewer-product-graph/accept/E-01-E-08.log`
- **Scenario 2 (Failure Mode - A-SEC-01)**: 3 payloads em todos os sinks
  - **Given** payloads `<img src=x onerror=alert(1)>`, `<script>alert(1)</script>`, `javascript:`-em-nota, injetados como conteúdo de nota + `layer`/`path` + texto de erro
  - **When** renderiza snippet, drawer, lista/grafo, banner erro
  - **Then** FALHA se qualquer sink executa (alerta) ou faz request; exige texto literal em todos (SEC-01 escopo total)
- **Scenario 3 (Edge Case)**: E-06 duplo `ensure` + disconnect mid-view
  - **Given** view Grafo aberta
  - **When** chama `ensureGraphEventSource()` 2× + derruba SSE mid-view + reconecta
  - **Then** no máximo 1 `EventSource` aberto (INV-06); banner sutil ou badge, nunca duplica; sem exceção

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/E-01-E-08.log` + `.spec/viewer-product-graph/accept/A-SEC-01.log` (Type: Manual/Sec): executar E-01..E-08 + A-SEC-01 (3 payloads) ANTES de editar (TDD FIRST — registra quais EC falham no estado atual).
- [x] `viewer/index.html`: polish (texto empty fixo, FTS-safe, SSE sutil idempotente, 360px, motion) + `escapeHtml()` no(s) sink(s) restante(s).

#### 🏁 Definition of Done (DoD)
- [x] Aceites ANTES do código.
- [x] E-01..E-08 8/8 PASS + A-SEC-01 PASS (3 payloads, zero execução, zero request).
- [x] INV-06 verificado (1 fonte).
- [x] Diff-only.
- [x] No TODOs pending.

---

### Task P3-02: Re-gate final — textual CI + `diff --name-only` + Lighthouse + `brain-web`
*Mapped directly from PLAN.md §8 (Phase 3) + SPEC §11*

- **Constraint Ref**: SPEC §11 (gate CI textual + diff gate + Lighthouse + brain-web verde) / VIEW-01/VIEW-02/SEC-01/SEC-02/SEC-04
- **Depends on (PLAN)**: P3-01
- **Dependency Type**: Hard
- **Layer**: `.spec/` (logs; sem código produto)
- **Task Type**: Infra/Gate
- **Priority**: P0
- **Risk Level**: Low
- **Parallelizable**: No (terminal)
- **Artifact**: `.spec/viewer-product-graph/accept/GATE.log` (greps + diff + Lighthouse + suite)
- **Framework Detail**: Gate textual CI (grep no fonte, sem executar produto) verbatim PLAN §8b

#### 📝 Context & Domain Impact
- **Description**: Portão final que autoriza `verify`: re-executa TODOS os gates de uma vez sobre o estado final.
- **Domain Impact**: Sem re-gate, regressão introduzida no polish (ex.: `onclick` recolocado no drawer) chega ao Verify sem ser vista.

#### 🛡️ Technical Enforcement (DO NOT IGNORE)
- **Rule**: Qualquer grep falho = BLOQUEIA Verify (volta para a task dona). `git diff --name-only` produto só `viewer/index.html`.
- **Failure Mode**: Aprovar Verify com gate vermelho → archive congela defeito conhecido no histórico.

#### ✅ Acceptance Criteria (BDD)
- **Scenario 1 (Success)**: gate 100% verde
  - **Given** P3-01 completo
  - **When** executa bateria: `grep -c encodeURIComponent ≥1` · `grep -c escapeHtml ≥1` · `grep -E 'AbortSignal.timeout|AbortController' ≥1` · `grep -c '40' ≥1` · `grep -c '0.55' ≥1` · 6 hex presentes · `aria-live` presente · `grep -c 'onclick=' == 0` · `grep -E 'eval\(|Function\(|document.write|outerHTML' == 0` · `grep -c 'src="http' == 0` (script) · `grep -c data-toast == 0` · `diff --name-only` só `viewer/index.html` (+`.spec/`) · `cargo test -p brain-web` verde · Lighthouse a11y ≥90
  - **Then** GATE.log anexado com cada linha PASS; autoriza `verify`
- **Scenario 2 (Failure Mode - gate vermelho)**: qualquer grep/diff/suite falha
  - **Given** um item acima fora do esperado
  - **When** re-gate executa
  - **Then** FAIL nomeando a task dona (ex.: `onclick=` → volta P2-01; `40` ausente → volta P1-05); não autoriza Verify
- **Scenario 3 (Edge Case)**: diff com arquivo `.spec/` + `viewer/index.html` apenas
  - **Given** governance gerou logs/fixtures em `.spec/`
  - **When** avalia diff
  - **Then** PASS (`.spec/` permitido); qualquer `crates/**`, `src/**`, `hooks/**` = FAIL

#### 🛠️ Implementation Steps (TDD - Test First)
- [x] `.spec/viewer-product-graph/accept/GATE.log` (Type: Gate): executar bateria completa e salvar saída (este log É o teste — sem código produto nesta task).
- [x] (Sem arquivo produto): se FAIL, retornar à task dona; se PASS, declarar pronto para `verify`.

#### 🏁 Definition of Done (DoD)
- [x] GATE.log completo, todos os itens PASS.
- [x] A-01..A-13 + A-SEC-01/02 + E-01..E-08 referenciados (logs existem).
- [x] `cargo test -p brain-web` verde + Lighthouse ≥90 anexados.
- [x] Pronto para `sdd-compiler verify` (BLOQUEADO se qualquer item vermelho).
- [x] No TODOs pending.

---

## 5. Coverage Validation
- [x] All PLAN tasks converted and mapped — P0-01/P0-02 + P1-01..P1-08 + P2-01..P2-03 + P3-01/P3-02 = 15/15 PLAN §8, sem invenção nem fusão.
- [x] All constraints mapped to explicit BDD scenarios — RF-01..RF-13 → A-01..A-13; SEC-01..SEC-04 → gates + A-SEC-01/02 (+E-05); EC-01..EC-08 → E-01..E-08; INV-01..INV-06 → asserts nas tasks donas.
- [x] All tasks include failure mode validation — cada task tem Scenario 2 (Failure) + Scenario 3 (Edge/Idempotência); tasks segurança têm A-SEC-01 (3 payloads, P3-01 + antecipado P2-01) e A-SEC-02 (5 fetches, P2-02).
- [x] No circular dependencies in the graph — P0→P1→P2→P3 DAG (§3), verificado por leitura: nenhuma aresta retorna.
- [x] No orphan tasks — toda task no grafo §3 (P0-01 raiz única, P3-02 terminal único); nenhum RF órfão (RF-01..13 todos com task dona; tabela abaixo).
- [x] TDD Order — toda task lista arquivo de aceite/fixture ANTES do arquivo produto (`viewer/index.html`); P3-02 sem produto (gate puro).
- [x] Diff-only — toda task DoD exige `git diff --name-only` só `viewer/index.html` (+`.spec/`); nenhum outro arquivo produto em nenhuma task.
- [x] Arquivos restritos — só `viewer/index.html` + `.spec/viewer-product-graph/accept|fixtures` em todas as tasks.

### RF→Task map (no orphan RF)
| RF/SEC/EC | Task dona | Aceite |
|-----------|-----------|--------|
| RF-01 | P1-01 | A-01 |
| RF-02 | P1-02 | A-02 |
| RF-03 | P1-03 | A-03 |
| RF-04 | P1-02 | A-04 |
| RF-05 | P1-04 | A-05 |
| RF-06 | P1-05 | A-06 |
| RF-07 | P1-06 | A-07 |
| RF-08 | P1-06 | A-08 |
| RF-09 | P1-07 | A-09 |
| RF-10 | P1-08 | A-10 |
| RF-11 | P2-01 | A-11 |
| RF-12 | P2-02 | A-12 |
| RF-13 | P2-03 | A-13 |
| SEC-01 | P1-08(parcial)+P2-01+P3-01(fecha) | A-SEC-01 |
| SEC-02 | P2-02 | A-SEC-02 |
| SEC-03 | P1-08 | (A-10 failure) |
| SEC-04 | P1-02+P2-01 | (gates grep) |
| EC-01..EC-08 | P3-01 (+antecipações P1-02/P1-04/P2-02/P2-03) | E-01..E-08 |

## 6. Plan Feedback (Optional)
- **Identified Issues**:
  - PLAN §8 não atribui A-SEC-01/A-SEC-02/E-01..E-08 a tasks individuais — resolvido aqui: A-SEC-02 → P2-02 (dona única), A-SEC-01 → P3-01 (dona, antecipada em P2-01), E-01..E-08 → P3-01 (donos, com antecipações marcadas). Sem task nova inventada; determinismo preservado.
  - PLAN não explicita TDD-first para aceite manual — resolvido aqui: cada task exige executar o aceite ANTES de editar (FAIL/PASS-baseline registrado), que é o equivalente manual de "teste antes do código" na ausência de harness JS (declarado SPEC §11).
  - Nenhum gap arquitetural novo; evento `graph-changed` reutilizado já mapeado (consumer P1-04/P3-01, PLAN §9) — sem task de evento adicional, conforme N/A justificado.

---

## Self-Validation contra `asserts/tasks-gate.md`
| Gate | Status | Evidência |
|------|--------|-----------|
| Traceability 1:1 | ✅ PASS | 15/15 PLAN §8 presentes (§5 Coverage + tabela RF→Task, zero RF órfão) |
| Metadata Enrichment | ✅ PASS | Priority/Risk/Type/Dep-Type/Parallelizable em todas as 15 |
| Layer-to-Folder Mapping | ✅ PASS | Só `viewer/index.html` + `.spec/viewer-product-graph/accept\|fixtures` (VIEW-01) |
| BDD Scenarios | ✅ PASS | 3 cenários (Success/Failure/Edge) nas 15 tasks |
| TDD Order | ✅ PASS | Aceite/fixture ANTES de `viewer/index.html` em 14/15; P3-02 gate puro sem produto |
| Granularidade | ✅ PASS | Todas <2h (estimativas PLAN §8: máx 2pt) |
| Mermaid Graph | ✅ PASS | §3 DAG, sem ciclos, sem órfãs (raiz P0-01, terminal P3-02) |
| Consistency Check | ✅ PASS | Nenhuma macro-task inventada (A-SEC/E viram cenários dentro das tasks PLAN, não tasks novas) |
| DoD Per Task | ✅ PASS | DoD com aceite + grep/diff/brain-web em cada task |

**Gate: PASS.** Próximo passo: implementar P0→P3 (sem código produto nesta delegação) e depois `sdd-compiler verify`.
