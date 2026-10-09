# EXPLORATION — viewer-graph (Fase 0, somente análise)

Data: 2026-10-09 · Feature: tela de grafo estilo Obsidian no viewer · v1 básica read-only
PICCO aprovado: nós = notas+projetos, arestas = wikilinks + note_projects, clique-navega, filtro por projeto.
NENHUM código nesta fase. NENHUM commit. Produção tocada só com SQL read-only (`mode=ro`).

## 1. Problema

Viewer atual (`viewer/index.html`, 284 linhas) é lista + busca textual. Relações entre
notas (wikilinks `[[ ]]`, pertencimento a projeto) existem no banco mas são invisíveis:
o stream de grafo do RRF (`brain-store/src/lib.rs:1842`, `SELECT to_path FROM links
WHERE from_path=? LIMIT 10`) alimenta score, nunca UI. Usuário quer ver estrutura —
clusters por projeto, notas órfãs, hubs — e navegar por clique (grafo → `/api/read`).

## 2. Como o viewer serve hoje (medido, não suposto)

- `crates/brain-web/src/lib.rs:15-27` — `router(db)` monta 4 rotas read-only +
  `ServeDir("viewer")` com fallback `serve_index`. `AppState { db: String }`
  (só path, nunca `Store` — regra `Send`, ver architecture-rules.md). Cada handler
  abre `Store::open(&s.db)` por request, usa sincronamente, dropa antes do `.await`.
- Endpoints: `/api/status` (counts + coverage, erro propaga como `{"error"}` — TD-008),
  `/api/search` (`query` + `top_k` clamp 1..20 + `layer` opt, serve `chunk_index`),
  `/api/read?path=` (casa `notes.path` exato — TD-011: path vai verbatim, sem strip),
  `/api/list` (`recent_paths(1000)` + 1× `GROUP BY path` em chunks, ~2.7 ms warm no
  corpus antigo; serve `chunks_total/chunks_embedded` por nota, nunca bodies).
- Viewer JS: `fetch(location.origin + /api/*)`, abort 3000 ms no status. Sem build,
  sem framework, sem deps (No-SPA rule, frontend-rules.md). Guards de wiring
  (`viewer_*_reads_only_fields_*`) varrem o fonte contra payloads reais — novo
  endpoint + nova render path PRECISAM estender esses guards (padrão estabelecido).
- Clique-navega já existe: `browseAll → onclick="readNote(n.path)" → /api/read`.

## 3. Schema + números de produção (2026-10-09, `mode=ro`)

| Tabela | Linhas | Notas |
|--------|--------|-------|
| `notes` | 522 | arquitetura 64, estudos 27, projetos 5, regras 218, sessoes 208 |
| `chunks` | 2088 | ~4.0 chunks/nota |
| `links` (`from_path → to_path`, sem FK) | 12 | 12 `DISTINCT from_path`, 2 `DISTINCT to_path`; 12 notas com `[[ ]]` |
| `note_projects` (`note_path, project_id` PK) | 110 | atlas-ecm 8, hive 18, mobile 1, mobile-erp 28, progat-b2b 55 |
| `projects` | 8 | atlas-ecm, boleto, hive, mobile, mobile-erp, mobile_erp, progat-b2b, progaterp |
| `entities` / `entity_links` | 378 / 843 | fora do escopo v1 (ver §7) |
| `notes.project_id IS NOT NULL` (posse) | 163 | link (110 rows) ≠ posse — `brain_project_notes` faz JOIN das duas |

Achados que mudam o plano:

- **Grafo quase vazio: 12 arestas wikilink para 522 notas.** 11/12 `to_path` resolvem
  para `sessoes/boleto/2026-10-08` (estrela de sessão, não rede de conhecimento);
  **1 dangling** (`atlas-ecm/acl/root-subject-leaves-scene-lockout` não existe em
  `notes` — `to_path` sem FK resolve para nada). Endpoint que só serve wikilinks
  desenha 522 nós isolados + 1 estrela. **O grosso da conectividade v1 vem de
  `note_projects` (110 arestas nota→projeto), não de wikilinks.**
- `note_projects` cobre 5/8 projetos; `boleto`, `mobile_erp`, `progaterp` têm zero
  links (só posse ou nada). Filtro por projeto precisa unir posse + link
  (mesma armadilha do search: `regras/global/brain/project-filter-linked-blind`).
- Entidades (843 links) são o stream denso, mas ficam FORA da v1 por decisão
  (entidade→nota é bipartido, outro layout, outro endpoint).

## 4. Alternativas de render (critérios: offline-first, 500+ nós fluido, sem build SPA)

### (a) Canvas 2D próprio, ~200 linhas, zero dependência — RECOMENDADA

- Force-directed layout próprio (repulsão O(n²) por tick + molas nas arestas) em
  `<canvas>` + `requestAnimationFrame`, pan/zoom via transform, clique por
  hit-test de distância. Tudo inline no `index.html` existente, mesmo padrão do
  JS atual (fetch → render, sem framework).
- Tamanho: ~8–12 KB de JS adicionado ao arquivo atual (estimado por contagem de
  linhas × densidade atual; nada para baixar, nada para versionar).
- Perf: n=530, ticks O(n²)=~280k ops/frame — trivial para CPU moderna a 60fps;
  arestas 122. Gargalo real seria n>2k sem Barnes-Hut; corpus cresce ~10/semana,
  teto distante. Degradação honesta: se nós >1500, congelar layout após N ticks
  (estático + pan/zoom) em vez de simular para sempre.
- Prós: offline por construção; sem supply-chain; sem build; guards de wiring
  existentes continuam aplicáveis (mesmo arquivo, mesmas convenções);
  clique-navega = `readNote()` já testada.
- Contras: layout menos bonito que libs dedicadas; sem física avançada
  (colisão, clusters hierárquicos); manutenção nossa (~200 linhas para testar
  a olho — sem framework de teste frontend por decisão No-SPA).

### (b) Lib vendored local (force-graph / cytoscape.js / d3-force)

- Medido via npm/Bundlephobia (2026-10-09): `force-graph` 1.51 unpacked **6.46 MB**,
  **15 deps**, 64 arquivos; `cytoscape` 3.34 min **420 KB** (**131.5 KB** gzip);
  `d3-force` sozinho é dezenas de KB mas exige renderer próprio (volta a (a)
  para a metade do trabalho).
- Vendored = copiar `dist/*.min.js` para `viewer/vendor/` e servir via `ServeDir`
  existente (zero mudança de serve). Offline OK, sem CDN.
- Prós: física e interação (drag, zoom, labels, setas) prontas; cytoscape tem
  layouts múltiplos (cose, concentric — útil p/ estrela nota→projeto).
- Contras: (i) peso: cytoscape 420 KB min é **~30× o `index.html` atual** (~15 KB);
  force-graph 6.46 MB é indefensável para viewer local; (ii) supply-chain
  vendored sem lockfile nem update (arquivo morto no repo); (iii) API da lib
  vira dialeto no viewer — guards de wiring precisam entender chamadas da lib,
  não só `fetch`; (iv) dramatico overkill para 122 arestas.
- Veredito: rejeitada para v1. Reavaliar se corpus passar ~2k nós OU se v2
  pedir layouts hierárquicos (aí cytoscape vendored, nunca force-graph).

### (c) CDN externo (`<script src="https://unpkg.com/...">`)

- Prós: zero bytes no repo, updates automáticos.
- Contras (fatais): **quebra offline-first** — viewer local com servidor no
  `127.0.0.1` deixa de funcionar sem internet / sob CSP restritiva; SRI pinning
  ameniza mas não resolve indisponibilidade; "updates automáticos" sem lockfile
  é mudança de comportamento sem commit (anti-harness). **Rejeitada sem
  ressalva** — contradiz a razão de existir do viewer (diagnóstico local).

## 5. Endpoint proposto: `GET /api/graph?project=`

```json
{"nodes":[{"id":"regras/global/x","kind":"note","label":"x","layer":"regras"}],
 "edges":[{"from":"regras/global/x","to":"projeto:mobile-erp","kind":"project"}]}
```

- `kind` nó: `note | project`. `kind` aresta: `wikilink | project`.
  IDs de projeto com prefixo `projeto:` para não colidir com `notes.path`.
- Estimativa produção (sem filtro): **~530 nós** (522 + 8), **~122 arestas**
  (12 wikilink + 110 project). Com `?project=X`: nós = owned ∪ linked ∪ projeto,
  arestas idem filtradas (ex. progat-b2b: ~55+ arestas project + wikilinks internos).
- Custo: **4 queries agregadas, zero N+1** — (1) `SELECT path,layer FROM notes`
  (+ `WHERE project_id=? OR path IN note_projects` sob filtro — predicado
  owned∪linked nos DOIS lugares, lição do blind-filter); (2) `SELECT * FROM links`
  (12 rows, full scan irrelevante); (3) `SELECT note_path,project FROM
  note_projects (+join)`; (4) `SELECT name FROM projects`. Mesmo padrão do
  `/api/list` (`GROUP BY`, sem bodies). Latência esperada <10 ms warm; payload
  ~60–80 KB JSON sem filtro (530 nós × ~120 B) — menor que um `recent --top-k 40`.
- Filtro `project=` com nome inexistente → `{nodes:[],edges:[]}`, sem erro
  (mesma convenção do search). `links` dangling (1 hoje): servir a aresta com
  `to` sem nó correspondente OU podar? **Pergunta aberta P1** (§6) — recomendação
  exploração: podar + contar como `dangling:N` no payload (diagnóstico honesto,
  sem nó fantasma clicável para 404).
- `recent_paths(1000)` cap: 522 notas cabem; se corpus passar 1000, `/api/graph`
  NÃO deve reusar o cap (grafo parcial = arestas para nós ausentes). Query
  própria sem `LIMIT`, ou `COUNT` + erro honesto acima de teto documentado.

## 6. Perguntas em aberto

- **P1 (bloqueia SPEC):** aresta dangling — podar+contar vs nó fantasma? (recom: podar + `dangling` count)
- **P2 (bloqueia SPEC):** filtro `project=` filtra só arestas project ou também
  esconde wikilinks cross-project? (recom: esconder — grafo por projeto é subgrafo induzido)
- **P3 (SPEC):** label do nó = último segmento do path ou path cheio? (recom: último segmento + tooltip path cheio; paths `sessoes/x/<data>` colidem no último segmento — sufixo com data?)
- **P4 (SPEC):** onde mora o botão/aba no `index.html` (4ª view vs seção em browse)? guards de wiring cobrem por função — nova função `renderGraph()` + teste de wiring novo.
- **P5 (não-bloqueante):** threshold de congelamento do layout (nós N onde simulação vira estática)? Medir no corpus real durante implementação; default proposto 1500.

## 7. Riscos

- **R1 — grafo wikilink nunca exercitado (o maior).** Produção tem 12 arestas para
  522 notas; o stream de grafo do RRF existe mas nunca desenhou nada denso. V1
  **parecerá vazia** se avaliada só por wikilinks — expectativa deve ser calibrada
  no SPEC ("grafo de projetos com overlay de wikilinks", não "grafo Obsidian").
  Cobertura honesta: documentar `links=12` no VERIFY, não prometer rede de conhecimento.
- **R2 — perf é não-problema hoje, armadilha amanhã.** O(n²) próprio é fluido a
  530 nós; a 5k vira 25M ops/frame. Mitigação: congelamento + nota no SPEC do
  teto medido. NÃO otimizar prematuramente (Barnes-Hut antes de 2k nós é ouro).
- **R3 — read-only deve continuar read-only.** `/api/graph` abre `Store` por
  request, sem `brain-embed`, sem escrita (mesma disciplina dos 4 handlers).
  Qualquer acc de "criar link pelo grafo" é v2 e fora desta SPEC.
- **R4 — wiring guards.** TD-011 provou que campo lido ≠ campo servido quebra
  silencioso. `/api/graph` + `renderGraph()` entram nos guards (`leaf_names` vs
  `entry_field_reads`) desde o primeiro commit — não como follow-up.
- **R5 — entidades fora do escopo, deliberadamente.** 843 `entity_links` são o
  grafo denso real, mas bipartido entidade↔nota pede outro layout e dobra o
  payload. V1 = wikilink + project. Registrar como v2 candidata, não como gap.
- **R6 — nomes de projeto com typo (`mobile` vs `mobile-erp` vs `mobile_erp` vs
  `progaterp`).** Filtro por projeto herda a armadilha: conferir com
  `brain_project_list` / servir lista de projetos no payload (`projects:[]`)
  para o dropdown não depender de digitação.

## 8. Recomendação (para SPEC)

Canvas 2D próprio (a) + `GET /api/graph?project=` agregado sem N+1 + v1 =
nós nota+projeto, arestas wikilink+project, clique→`readNote()`, filtro por
projeto (owned∪linked), dangling podado+contado, guards de wiring estendidos,
teto de layout documentado. Custo estimado: ~200 linhas JS + ~80 linhas Rust
(handler + teste) + guards. Sem deps, sem build, offline intacto.
