# SPEC — viewer-graph

Data: 2026-10-09 · Fase 1 (Análise) · Base: `.spec/viewer-graph/EXPLORATION.md`.
P1 (dangling) e P2 (filtro) **decididas pelo usuário**: podar+contar, subgrafo induzido.
Nenhum código nesta fase. Toda afirmação técnica cita `arquivo:linha` (regra H8).

## 1. Contexto

Viewer serve hoje 4 rotas read-only + `ServeDir("viewer")` com fallback `serve_index`
(`crates/brain-web/src/lib.rs:15-27`). `AppState { db: String }` (`lib.rs:7`) — só path,
nunca `Store` (regra `Send`). Cada handler abre `Store::open(&s.db)` por request
(`lib.rs:96,113,126,178`). Contrato de erro honesto: `{"error": …}` propaga, nunca lista
vazia falsa (`lib.rs:96-100,178-189`; disciplina TD-008/TD-011).

Schema relevante, DDL em `crates/brain-store/src/lib.rs`:
- `links(from_path, to_path)` sem FK (`lib.rs:929`), índice `idx_links_from` (`lib.rs:932`);
  escrita em `lib.rs:1069,1085` (`DELETE` + `INSERT` por `from_path` no sync).
- `note_projects(note_path, project_id)` many-to-many (`lib.rs:945-951`); escrita em
  `lib.rs:1029`, remoção em `lib.rs:1035`; leitura owned+linked em `lib.rs:1009-1010`.
- `projects` listada por `project_list` (`lib.rs:988`); viewer conta via
  `st.project_list().unwrap_or_default().len()` (`lib.rs:104`).
- Filtro projeto no search aplica owned∪linked nos dois lados
  (FTS em `lib.rs:1691`, vetor pré-filtro em `lib.rs:1779`) — mesma armadilha vale aqui.
- Grafo RRF lê `SELECT to_path FROM links WHERE from_path=?1 LIMIT 10` (`lib.rs:1842`):
  alimenta score, nunca UI.

Números de produção 2026-10-09 (`mode=ro`, EXPLORATION §3): notes 522, chunks 2088,
links 12 (12 `DISTINCT from_path`, 2 `DISTINCT to_path`), note_projects 110,
projects 8, entities/entity_links fora do escopo. 11/12 wikilinks em estrela para
`sessoes/boleto/2026-10-08`; 1 dangling (`atlas-ecm/acl/root-subject-leaves-scene-lockout`).

Clique-navega já existe: `onclick="readNote('${n.path}')"` (`viewer/index.html:165`) →
`GET /api/read?path=` que casa `notes.path` exato (`lib.rs:125-130`); path vai verbatim,
sem strip (TD-011, `viewer/index.html:183-187`). Guards de wiring varrem fonte contra
payload real (`lib.rs:606-851`: `viewer_fn`, `entry_field_reads`, `assert_no_phantom_reads`).

## 2. Requisitos funcionais

### Endpoint `GET /api/graph?project=`

- **RF-01 formato**: resposta JSON `{nodes:[], edges:[], dangling:N, projects:[]}`.
  Nó: `{id, kind, label, layer}`. Aresta: `{from, to, kind}`. `dangling` = nº de arestas
  wikilink podadas (inteiro ≥0, sempre presente). `projects` = nomes p/ dropdown (sempre presente).
- **RF-02 kinds e ids**: kind nó `note | project`; kind aresta `wikilink | project`.
  Nó projeto tem id `projeto:<nome>` (prefixo evita colisão com `notes.path`);
  nó nota tem id = `notes.path` verbatim (mesma identidade que `/api/read` casa em `lib.rs:127`).
- **RF-03 filtro owned∪linked**: com `?project=X`, nós = (notas com `notes.project_id=X`)
  ∪ (notas em `note_projects` p/ X, `lib.rs:1010`) ∪ (nó `projeto:X`); arestas = só as que
  têm as duas pontas no conjunto (subgrafo induzido, P2 aprovada — wikilinks cross-project
  escondidos, não pendurados). Predicado aplicado na query, não pós-filtro em memória
  (lição `lib.rs:1682-1691,1779`).
- **RF-04 inexistente→vazio**: `project=` sem row em `projects` (`lib.rs:988`) →
  `{nodes:[], edges:[], dangling:0, projects:[…]}` com HTTP 200, sem `{"error"}`
  (mesma convenção do search, EXPLORATION §5). Ausência de filtro = grafo completo.
- **RF-05 dangling podado+contado (P1 aprovada)**: aresta `links.to_path` sem row em
  `notes` não gera nó nem aresta; incrementa `dangling`. Sem nó fantasma clicável
  (fantasma resolveria para `{"error":"not found"}`, `lib.rs:129`).
- **RF-06 sem LIMIT parcial**: query própria sem `LIMIT`; NÃO reusar `recent_paths(1000)`
  (`lib.rs:1663-1666`) — grafo parcial cria arestas para nós ausentes (EXPLORATION §5).
  Custo: 4 queries agregadas, zero N+1 (padrão `/api/list`: `recent_paths` + 1× `GROUP BY`
  em `lib.rs:178-189`): (1) `SELECT path,layer FROM notes` (+ predicado RF-03 sob filtro);
  (2) `SELECT from_path,to_path FROM links`; (3) `SELECT note_path,project_id FROM note_projects`;
  (4) `SELECT name FROM projects` via `project_list` (`lib.rs:988`). Latência esperada <10 ms
  warm; payload ~60–80 KB sem filtro (530 nós × ~120 B).
- **RF-07 `projects:[]` p/ dropdown**: lista de nomes de `projects` servida no payload
  para o filtro não depender de digitação (armadilha `mobile`≠`mobile-erp`≠`mobile_erp`≠`progaterp`,
  EXPLORATION R6; `project_list` em `lib.rs:988`).

### Viewer — view nova `renderGraph()`

- **RF-08 aba/view nova**: 4ª view no `viewer/index.html` (ao lado de search/browse;
  botões atuais em `index.html:43-55`), função `renderGraph()` + canvas force-directed
  próprio (~200 linhas, `requestAnimationFrame`, pan/zoom por transform, hit-test por
  distância — EXPLORATION §4a). Sem deps, sem build, offline intacto.
- **RF-09 clique→readNote**: clique em nó nota chama `readNote(id)` existente
  (`index.html:179-226`) com id verbatim; clique em nó `projeto:X` aplica filtro RF-03
  (mesma ação do dropdown). Nenhum path reescrito (proibido `slice/split/replace/substr`,
  guard `lib.rs:825`).
- **RF-10 filtro projeto UI**: dropdown populado de `projects:[]` (RF-07) + opção "todos";
  troca refaz `fetch(/api/graph?project=)` e re-renderiza. Nome com typo impossível
  por construção (sem digitação livre).
- **RF-11 labels**: label = último segmento do path; `title`/tooltip = path cheio.
  Colisão conhecida `sessoes/x/<data>` (muitos `2026-…`): tooltip desambigua; sem
  sufixo automático na v1 (P3 respondida: tooltip basta; sufixo com data é v2 se
  usuários reclamarem).
- **RF-12 congelamento >1500**: nós >1500 → layout congela após N ticks (estático +
  pan/zoom), em vez de simular para sempre (EXPLORATION §4a: O(n²) ~280k ops/frame a
  530 nós é trivial; a 5k vira 25M). Contador de nós visível no canvas ("532 nós ·
  layout congelado" vs "532 nós · simulando"). Threshold default 1500, documentado,
  medido no corpus real na implementação (P5 respondida).

### Guards + testes

- **RF-13 guards estendidos no commit 1**: `renderGraph()` entra em
  `crates/brain-web/src/lib.rs` testes `mod tests` (`lib.rs:203-944`) seguindo o padrão
  existente: (a) `viewer_graph_reads_only_fields_the_graph_endpoint_serves`
  (`assert_no_phantom_reads`, `lib.rs:566-583`); (b) clique entrega path resolúvel
  (metade estática anti-`slice/split/replace`, metade comportamental `note_get`,
  padrão `lib.rs:801-850`). `JS_MEMBERS`/`JS_ROOTS` (`lib.rs:462-473`) estendidos só com
  membros DOM reais que o canvas usar (`getContext`, `requestAnimationFrame` via `window`).
- **RF-14 testes (handler + wiring + mutações)**:
  - Handler: payload tem `nodes/edges/dangling/projects`; filtro owned∪linked
    (nota só-posse + nota só-link aparecem; nota de outro projeto não);
    inexistente → vazio 200; dangling podado e contado; `projeto:` sem colisão.
  - Wiring: os dois guards de RF-13 verdes.
  - Mutações (padrão `.spec/brain-help-visual/SPEC.md §8`): remover `dangling` do payload
    → falha; remover `projects` → falha; `renderGraph` ler campo inexistente → falha;
    aplicar filtro só em posse (sem `note_projects`) → falha no caso só-link.

## 3. Requisitos não-funcionais / restrições

- **R-01 read-only**: handler sem `brain-embed`, sem escrita, sem `Store` através de
  `.await` (disciplina `lib.rs:95-201`; `AppState{db}` em `lib.rs:7`). Erro de store →
  `{"error": …}` (`lib.rs:96`), nunca lista vazia falsa.
- **R-02 sem dependência nova**: `brain-web` continua folha read-only
  (depende de `brain-store`+`brain-core`, nunca `brain-mcp`/`brain-embed` —
  `.agents/rules/architecture-rules.md`). Sem `brain-embed` no viewer pelo mesmo motivo
  de `lib.rs:80-93` (probe de rede sob deadline de fetch mente).
- **R-03 agregação, não N+1**: RF-06 lista as 4 queries; nenhuma query por nó/aresta
  (padrão `chunk_embedding_counts` 1× `GROUP BY`, `lib.rs:186-189`, ~2.7 ms warm).
- **R-04 orçamento**: payload sem filtro ≤100 KB no corpus atual; handler <50 ms warm;
  canvas 60fps a 530 nós; congelamento RF-12 acima de 1500.
- **R-05 No-SPA**: sem framework, sem `npm`, sem `vendor/` novo, sem CDN
  (`.agents/rules/frontend-rules.md`). Diff: `crates/brain-web/src/lib.rs` + testes +
  `viewer/index.html`. Zero DDL (`SCHEMA_VERSION` intacto).
- **R-06 portões**: `cargo test --workspace` + `cargo clippy --workspace --all-targets --
  -D warnings` + `.venv/bin/python -m pytest src/tests/ -q` (124 passed, 2 legadas —
  `.agents/rules/harness-continuous.md:H1`); E2E H3 scratch quando a fase de
  implementação chegar (nunca `data/brain.db`, nunca 8321/8322).
- **R-07 sem `unwrap` em handler**: todo `Store::open`/query falha para `{"error"}`
  (padrão `lib.rs:96-100`).

## 4. Critério de aceite (E5)

Operador abre viewer, alterna para aba Grafo, vê ~530 nós em clusters por projeto
(projeto-hubs com raios nota→projeto, não 522 isolados), clica num nó e lê a nota,
filtra por `progaterp-b2b` via dropdown e vê só o subgrafo, com badge de cobertura
inalterado. Inexistente → vazio honesto. Dangling visível como contador, não como nó.

## 5. ADRs

- **ADR-01 canvas próprio (a)**: force-directed O(n²) inline, ~8–12 KB JS, zero dep,
  offline por construção. Rejeitadas: (b) vendored (cytoscape 420 KB min é ~30× o
  `index.html` ~15 KB; force-graph 6.46 MB/15 deps; dialeto nos guards) e (c) CDN
  (quebra offline-first, updates sem commit). Reavaliar (b/cytoscape) se >2k nós ou
  layouts hierárquicos. Ref: EXPLORATION §4.
- **ADR-02 dangling podado+contado (P1, aprovada)**: `to_path` sem `notes` vira
  `dangling++`, sem nó. Alternativa nó-fantasma rejeitada: fantasma clica para
  `{"error":"not found"}` (`lib.rs:129`).
- **ADR-03 subgrafo induzido (P2, aprovada)**: filtro esconde wikilinks cross-project;
  aresta só existe com as duas pontas no conjunto. Alternativa "filtrar só project,
  manter wikilinks externos" rejeitada: pendura aresta em nó ausente (mesma classe do
  RF-06).
- **ADR-04 sem entidades na v1**: 843 `entity_links` são o grafo denso real mas
  bipartido (outro layout, payload dobrado). V1 = wikilink + project (EXPLORATION R5).
  V2 candidata, não gap.
- **ADR-05 query própria sem LIMIT**: `recent_paths(1000)` (`lib.rs:1663`) é teto de
  browse, não de grafo; grafo parcial = arestas órfãs. 4 agregadas, zero N+1.

## 6. Delta Specs

| Doc | Impacto |
|---|---|
| `AGENTS.md` tabela tools/endpoints | +1 linha: `GET /api/graph?project=` (viewer read-only 8322) — sem tool MCP nova |
| `.agents/skills/brain/SKILL.md` | nenhum (sem tool MCP; viewer não é tool) |
| `.agents/rules/frontend-rules.md` | +1 parágrafo: aba Grafo (`renderGraph()`, canvas próprio, teto 1500) sob No-SPA |
| `.agents/rules/database-rules.md` | nenhum (zero DDL; só lê `links`/`note_projects`/`projects`/`notes` existentes) |
| `.agents/rules/architecture-rules.md` | nenhum (fronteiras intactas; `brain-web` segue folha) |
| `viewer/index.html` (cabeçalho comentado, se houver) | documentar 5ª função + teto na UI |

## 7. UX / direção visual (vinculante)

Nota de processo: subagente `ux-designer` **não existe neste ambiente**; apoio usado foi
a skill `frontend-design` (direção intencional, anti-template, 1 risco justificado).
Decisões abaixo vinculam a implementação.

- **Tese**: "mapa de constelações do acervo" — fundo `github-dark` existente
  (`index.html:9`, `#0d1117`) permanece; grafo é noite onde projetos são estrelas-guia
  e notas são pontos. 1 risco: física nunca dorme de verdade — simulação desacelera para
  deriva lenta (0.2×) em vez de parar, salvo >1500 (RF-12 congela). Justificativa: mapa
  vivo sinaliza "índice vivo" sem custo; deriva é barata e respeita `prefers-reduced-motion`
  (vira estático).
- **Paleta (6 hex, nomeada)**: `Noite #0d1117` (fundo, existente), `Papel #c9d1d9`
  (labels, existente), `Link #58a6ff` (aresta wikilink), `Trilha #3fb950` (aresta project),
  `Guia #f0883e` (nó projeto — mesmo laranja dos `h2/h3` do `readNote`, `index.html:202-204`),
  `Órfã #484f58` (nó sem aresta — cinza do `.empty`, `index.html:31`). Kind carrega
  **forma + cor**: projeto = quadrado laranja `Guia`; nota = círculo; órfã = círculo
   vazado `Órfã`. Camada carrega **anel**, não preenchimento (anel fino na cor da camada;
   `estudos`/`indexacao` usam Órfã `#484f58` como fallback documentado — paleta
   fechada, sem cor nova; órfã-vazado vs anel se distinguem pela forma.
   Sem legenda nova — tooltip diz `id + layer` (scope no prefixo do path;
   endpoint não serve scope por RF-01).
- **Tipo**: sistema existente (`index.html:9`) mantido — sem webfont (offline-first);
  labels em 11px sistema, tooltip monospace (`SF Mono/Fira Code`, como `.note-path`,
  `index.html:27`). Hierarquia por opacidade, não por família.
- **Legibilidade a 530 nós**: arestas `Link/Trilha` a 35% opacidade, 1px; nós 3px
  (projeto 6px quadrado); labels só >zoom 1.5× ou hover/selecionado, senão só hubs
  (projetos + top-5 grau); viewport mostra "nós visíveis/total" durante zoom.
  Congelado >1500 anuncia (RF-12) em vez de degradar em silêncio.
- **Assinatura**: aresta project é **trilha pontilhada verde** (`Trilha`, dash 4/3) —
  "fio que amarra a nota ao seu projeto"; wikilink é linha sólida azul (`Link`).
  Nenhum outro tracejado existe na UI. Nada de gradiente roxo-azul genérico, nada de
  "big number + accent" — o hero é o próprio mapa, não um KPI.

## 8. Parecer de segurança (security-review light, Fase 1)

Read-only, mesma superfície dos 4 handlers (`lib.rs:15-27`). STRIDE resumido:

| Ameaça | Fluxo | Risco | Mitigação na SPEC |
|---|---|---|---|
| Spoofing | `GET /api/graph` sem auth | Baixo (aceito) | mesma postura dos 4 endpoints; viewer é diagnóstico em loopback, sem dados sensíveis novos |
| Tampering | `?project=` reflete em query | Baixo | query parametrizada (`?1`, padrão `lib.rs:1691,1779`); sem concatenação; `project` nunca vira path de disco |
| Repudiation | leitura sem audit | Baixo (aceito) | sem `audit_log` para GET (padrão dos 4 handlers) |
| Information Disclosure | bodies no payload | Baixo | grafo serve ids/labels/counts, **nunca `content`** (padrão `/api/list` que exclui bodies, `lib.rs:897-899`); erro generoso só `{"error"}` |
| DoS | `?project=` gigante / grafo completo | Baixo | sem filtro = ~80 KB (menor que `recent --top-k 40`); sem `LIMIT` mas com teto natural (nº de notas); R-04 orçamenta; sem regex/FTS no handler (sem ReDoS); `readNote` escapa via `escapeHtml` (`index.html:274-278`) para labels vindas do path |
| Elevation | path traversal via `project=` | N/A | `project=` é nome lógico, não path; nenhuma escrita em disco (fora de `BRAIN_EXPORT_ROOT` por construção — nada escreve) |

OWASP: sem escrita (A01 N/A além da postura existente), sem SQL concatenado (A03 ok),
sem auth nova (A07 inalterado), sem secret/log (A09 ok — sem `BRAIN_OLLAMA_URL` no viewer,
`lib.rs:80-93`), sem SSRF (nenhum fetch server-side; `brain-web` sem HTTP).
**Veredito: pass (Fase 1)** — nenhum 🔴; implementar com R-01/R-02/R-07 e re-revisar na Fase 3.

## 9. Auto-revisão (spec-review + conformidade)

Varrido contra `.agents/rules/` (harness H8: sem `arquivo:linha`, sem afirmação):

- 🔴 **Corrigido 1 — `recent_paths` como fonte do grafo**: rascunho sugeria reusar o cap
  1000; refutado por `lib.rs:1663` + EXPLORATION §5 (grafo parcial). Virou RF-06.
- 🔴 **Corrigido 2 — filtro só-posse**: rascunho filtrava por `notes.project_id`;
  refutado por `lib.rs:1009-1010` (JOIN owned+linked) e lição do blind-filter
  (`lib.rs:1682-1691`). Virou RF-03 + caso de mutação RF-14.
- 🔴 **Corrigido 3 — clique com strip de layer**: rascunho herdava `split('/')`;
  refutado por `lib.rs:125-130` + guard `lib.rs:825`. Virou RF-09 verbatim.
- 🟠 **Corrigido — P3/P5 abertas**: P3 (label) respondida com tooltip (RF-11);
  P5 (threshold) respondida com 1500 + anúncio (RF-12). P4 (aba vs seção) respondida:
  aba/view nova (RF-08) porque guards cobrem por função (`viewer_fn`, `lib.rs:592-604`).
- Conformidade: zero DDL (database-rules), `brain-web` folha (architecture-rules),
  No-SPA sem deps (frontend-rules), portões H1/H6 em R-06, Delta Specs §6 sem tool MCP.
