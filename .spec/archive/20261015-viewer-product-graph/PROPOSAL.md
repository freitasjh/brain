# Change Proposal: viewer-product-graph

> **ID**: `viewer-product-graph`
> **Status**: `Under Review`
> **Author**: sdd-compiler generate-proposal
> **Date**: 2026-10-15
> **Source**: descrição direta — demanda definida pelo ux-designer (Fase 1 Análise)

---

## 1. Problem Statement

- **Contexto**: Viewer estático `viewer/index.html` (692 linhas) + `brain-web` read-only (`GET /api/status,search,read,list,graph,events` sobre `brain.db`). Viewer atual é funcional mas não é cara de produto final: busca sem feedback, grafo pouco legível, layout sem hierarquia visual, sem padrão de navegação por tabs.
- **Impacto atual**: Sem esta feature, operador continua com viewer utilitário — busca sem estado de loading/erro explícito, grafo ilegível acima de ~100 nós (overlap de labels, sem legenda, sem drawer de leitura), sem header de produto. Custo: fricção diária de navegação no corpus (500+ notas) e retrabalho visual a cada evolução do viewer.
- **Evidência**: `viewer/index.html:1-80` — search-box único sem tabs Buscar/Explorar/Grafo, sem drawer lateral, sem legenda fixa; `frontend-rules.md` congela viewer como minimal No-SPA (Fase A+B); batches Architect Batch 1 + PM Batch 2 + decisão vinculante ux-designer definem escopo fechado.

---

## 2. Proposed Solution

**Resumo**: Transformar viewer em cara de produto final com refactor exclusivamente visual em `viewer/index.html`, sem tocar `/api/*`: tabs Buscar/Explorar/Grafo, drawer lateral de leitura, grafo legível até 500 nós, header de produto com status.

**Abordagem**:
- Refactor visual restrito a `viewer/index.html` (HTML+CSS+JS inline, sem deps/build/CDN, sem Vue/Pinia/PrimeVue) — ADRs VIEW-01..03 do Architect Batch 1.
- `/api/*` congelado: nenhum endpoint novo, nenhuma mudança de contrato (`/api/status,search,read,list,graph,events` inalterados).
- Tabs Buscar/Explorar/Grafo + drawer lateral (bottom-sheet em mobile 360px) + legenda 6-hex fixa + cap 40 labels com anti-colisão (decisões vinculantes ux-designer, §9).
- Alternativas descartadas:
  - SPA dedicado (Vue/React) — descartado por No-SPA Rule Fase A+B (`frontend-rules.md`); Fase C pode revisitar.
  - Novo endpoint `/api/*` para agregação de grafo — descartado; `GET /api/graph?project=` + `/api/read` + `/api/search` já suprem os 3 tabs.
  - Biblioteca de grafo externa (CDN) — descartada; canvas force-directed próprio (~8–12 KB JS, RAF, pan/zoom) já existe e evita build/deps.

**Viabilidade**: Viável. Escopo é só frontend estático lendo contratos existentes; nenhum DDL (`SCHEMA_VERSION 4` intacto), nenhuma crate Rust além de eventual doc (`frontend-rules.md`), nenhum impacto em `brain-store`/`brain-mcp`/`brain-embed`/`brain-cli`. Riscos conhecidos (O(n²) layout, SSE lifecycle, XSS via `innerHTML`, timeout de fetch) já mapeados com mitigação (§6).

---

## 3. Scope

### In Scope
- [ ] US2 (Must) — grafo legível até 500 nós: canvas próprio, cap 40 labels + anti-colisão, legenda 6-hex fixa, alfa 0.35→0.55, `prefers-reduced-motion` → estático
- [ ] US1 (Must) — busca com feedback: tabs Buscar/Explorar/Grafo, estados loading/erro/vazio, Esc limpa busca, banner inline sem toast
- [ ] US3 (Must) — header de produto: título, badge status (`/api/status` coverage + `queue.*`), footer com `API_BASE`
- [ ] US4 (Should) — drawer lateral de leitura: abre via `/api/read` com id verbatim, bottom-sheet em mobile 360px, `Esc` fecha
- [ ] US5 (Should) — status visível: badge online/degraded/offline + `(N awaiting embed)` abaixo de 100%
- [ ] Refactor visual restrito a `viewer/index.html` (único arquivo produto tocado)
- [ ] Requisitos vinculantes ux-designer (§9) como critérios de aceite visuais

### Out of Scope
- [ ] Qualquer mudança em `/api/*` (`brain-web/src/lib.rs`, contratos `status,search,read,list,graph,events`) — NÃO entra, API congelada por Architect Batch 1
- [ ] Qualquer mudança em `brain-store` / `brain-mcp` / `brain-embed` / `brain-cli` / schema v4 — NÃO entra
- [ ] SPA, framework, build, CDN, deps novas — NÃO entra (No-SPA Rule)
- [ ] Toast notifications — NÃO entra (banner inline é o padrão, sem toast)
- [ ] Mudança de RRF k=60, FTS porter, dim 768, WAL — NÃO entra
- [ ] SPEC/PLAN/TASKS nesta delegação — NÃO entra (só PROPOSAL)

---

## 4. Architecture Impact

| Aspecto | Impacto | ADR Necessário? |
|---------|---------|-----------------|
| Backend (módulos afetados) | Nenhum — `brain-web` read-only inalterado, sem novo handler | Não |
| Frontend (componentes/stores) | `viewer/index.html` único arquivo: tabs, drawer, legenda, header, cap labels, banner inline | Sim — VIEW-01..03 (Architect Batch 1, incorporar na SPEC) |
| Banco de dados (migrations) | Nenhuma — `SCHEMA_VERSION 4` intacto, sem DDL | Não |
| Multi-tenancy | Sem impacto — viewer sem conceito de tenant; filtro existente `?project=` reutilizado | Não |
| Segurança / RBAC | Sem novas permissões; atenção XSS: sanitizar `innerHTML` em snippet/drawer (risco mapeado §6) | Não (avaliado explicitamente) |
| Eventos (Kafka/async) | Nenhum evento novo; reutiliza `GET /api/events` SSE (`graph-changed`, polling 2 s) com `ensureGraphEventSource()` idempotente | Não |

Conflitos: nenhum — mudança não cruza boundary de crate (`architecture-rules.md`); `AppState{db:String}` per-request mantido; `Store` Send-não-Sync não afetado.

---

## 5. Delta Preview

| Documento Alvo | Tipo de Mudança | Seção Afetada |
|----------------|-----------------|---------------|
| `viewer/index.html` | MODIFIED | tabs Buscar/Explorar/Grafo + drawer + header + legenda + cap labels (único diff produto) |
| `.agents/rules/frontend-rules.md` | MODIFIED | Viewer / Aba Grafo — documentar tabs, drawer, legenda 6-hex, cap 40, `Esc`, alfa, banner inline, mobile 360px (se SPEC confirmar) |
| `crates/brain-web/src/lib.rs` (`/api/*`) | NONE | contratos congelados — nenhuma mudança |
| `AGENTS.md` | NONE | tabela tools inalterada (sem nova tool) |
| `.agents/skills/brain/SKILL.md` | NONE | sem nova tool/skill |

---

## 6. Risk Assessment

| Risco | Probabilidade | Impacto | Mitigação |
|-------|---------------|---------|-----------|
| Layout grafo O(n²) trava acima de 500 nós | Média | Alto | cap 40 labels + anti-colisão; freeze layout acima de 1500 nós com anúncio (padrão `viewer-graph` existente); deriva lenta 0.2×; teste 5 s com corpus 500 nós |
| SSE `EventSource` duplica/ vaza (lifecycle RAF) | Média | Médio | `ensureGraphEventSource()` idempotente; `renderGraph()` cancela só RAF; `stopGraph()` fecha só ao sair da view |
| XSS via `innerHTML` (snippet `/api/search`, drawer `/api/read`) | Baixa | Alto | escapar HTML antes de injetar; sem `innerHTML` com conteúdo cru; review de segurança na SPEC |
| Timeout fetch vs health `/api/status` (abort 3 s) | Média | Médio | badge degraded/offline explícito; sem pender UI; banner inline, sem toast |
| Scope creep (tocar `/api/*` ou virar SPA) | Média | Alto | gate: diff permitido só em `viewer/index.html` (+ doc `frontend-rules.md`); CI valida `git diff --name-only` |

---

## 7. Success Criteria

- [ ] Grafo renderiza 500 nós em ≤5 s em teste local (critério funcional — US2 Must)
- [ ] Busca exibe loading/erro/vazio + `Esc` limpa + banner inline sem toast (funcional — US1 Must)
- [ ] Header com badge status reflete `/api/status` (online/degraded/offline + awaiting embed) (funcional — US3/US5)
- [ ] Drawer abre nota via `/api/read` e vira bottom-sheet em viewport 360px (funcional — US4 Should)
- [ ] Acessibilidade Lighthouse ≥90 + AA: `canvas role=img` + `aria-label`, foco no drawer, `Esc` fecha, `prefers-reduced-motion` estático (técnico)
- [ ] `git diff --name-only` contém só `viewer/index.html` (+ `.spec/` governança) — nenhum `/api/*` tocado (técnico)
- [ ] `cargo test -p brain-web` verde (zero regressão API congelada) (técnico)

---

## 8. Completion Metadata

- **Archived on**: —
- **Duration**: —
- **Archive path**: —
- **Files changed**: —

---

## 9. Requisitos vinculantes ux-designer (não-negociáveis)

Decisões do ux-designer incorporadas verbatim como requisitos da SPEC futura:

1. **Tabs**: 3 tabs — Buscar / Explorar / Grafo (navegação única, sem views paralelas).
2. **Drawer**: lateral de leitura; em mobile vira bottom-sheet (viewport referência 360px); `Esc` fecha drawer e limpa busca.
3. **Legenda**: 6 hex fixa — Noite `#0d1117`, Papel `#c9d1d9`, Link `#58a6ff`, Trilha `#3fb950`, Guia `#f0883e`, Orfã `#484f58`.
4. **Grafo**: cap 40 labels + anti-colisão; alfa arestas 0.35→0.55 (hover/seleção); hub `projeto:<nome>` quadrado Guia 6px; nota círculo 3px; órfã círculo vazado Orfã.
5. **Feedback**: banner inline, sem toast; estados loading/erro/vazio explícitos na busca.
6. **A11y**: AA + `aria-*` nos controles + `canvas role=img` com descrição textual; `prefers-reduced-motion` → grafo estático.
7. **Mobile**: layout utilizável em 360px (drawer bottom-sheet, tabs scrolláveis).
