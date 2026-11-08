# TASKS — authority-boost-dominates-search (bugfix B1 análise, sem fix aplicado)

Legenda: [ ] pendente · Risco: A=alto M=médio B=baixo
Origem: GitHub issue #1 — `Project-filtered search buries linked global estudos (recall/ranking)`
Predecessor: `.spec/bugs/project-shared-search/TASKS.md` (corrigiu **inclusão** dos linked; este bug é de **ordenação**)

## Bug

Nota `estudos` global, densa, pinned e linkada fica fora do top-k em `search(project=...)` — e também sem `project` — mesmo sendo rank 2 no FTS e rank 4 no vetor. Não é recall: o candidato **chega** à fusão e perde a ordenação.

## Causa raiz (confirmada, só leitura)

**C1 — o boost de autoridade é 9× maior que a relevância inteira.**
`crates/brain-store/src/lib.rs:2002-2003` soma `+0.15` (layer `arquitetura`/`regras`) e `+0.10` (`pinned`) **ao** score RRF. O RRF é `1.0/(60.0+rank)` (`lib.rs:1821`, `:1901`, `:1914`, `:1925`): rank 1 = `0.01639`, e o teto somando os 4 streams é `4/61 = 0.06557`.

| grandeza | valor | razão vs. 1 stream |
|---|---|---|
| max score de 1 stream (rank 1) | 0.01639 | 1.0× |
| teto dos 4 streams somados | 0.06557 | 4.0× |
| boost `arquitetura`/`regras` | 0.15000 | **9.15×** |
| boost `pinned` | 0.10000 | 6.10× |

Logo o ranking não é decidido por relevância: **qualquer** nota `arquitetura`/`regras` (mesmo sem nenhum hit em FTS, rank vetorial 40) supera **qualquer** `estudos` não boostada, e supera `estudos` pinned que seja rank 1 em todos os streams (`0.0656 + 0.1 = 0.1656` vs. `0.15 + 0.0164 = 0.1664` — empate técnico decidido por ruído). O corpus é ~90% `arquitetura`/`regras` por projeto; `estudos/global/*` é estruturalmente soterramento.

**C2 — `top_k` do MCP é fixo em ≤20.**
`crates/brain-mcp/src/lib.rs:544` — `q.top_k.unwrap_or(5).clamp(1,20)`. A nota medida cai na posição **31**; via MCP nenhum `top_k` a alcança. Isso é por que a issue reporta "ausente mesmo em top_k=20" e não "rank 31".

**C3 — orçamento de candidatos por stream (`truncate(50)`), secundário aqui.**
`lib.rs:1781` (FTS `LIMIT 50`) e `lib.rs:1899` (vetor `truncate(50)`), ambos aplicados **depois** do filtro de projeto. Com 1116 notas owned no projeto, uma linked pode ser cortada antes da fusão. No repro desta issue **não** é a causa (rank_fts=2, rank_vec=4), mas é risco real em query mais larga.

**Hipóteses da issue revisadas:**
- H1 (`search_k = top_k*4`) — **não se aplica**. `search_k` só existe no legado Python (`src/brain_server/index/store.py:502`); o path Rust não tem essa variável, tem `truncate(50)` fixo por stream. Corrigir a issue.
- H2 (boost assimétrico) — **confirmada e é a causa dominante** (C1).
- H3 (filtro antes do top-50 descarta linked) — **verdadeira como risco** (C3), **não** como causa neste caso: a nota chega viva ao RRF com rank 2/4 e mesmo assim fica em 31º.

## Evidências medidas (2026-11-08, `data/brain.db` 2887 notas, coverage 99.98%, read-only)

Nota: `estudos/global/scheduling/grade-lote-mensal-cascata` — id 2990, layer `estudos`, scope `global`, `project_id NULL`, `pinned=1`, 3304 B, 10 chunks (10 embeddados), linkada a `atlas-agenda`(15) e `atlas-pet`(16). FTS row presente.

```
# sem tag, sem project — query "grade lote mensal cascata deduplicação DISPLAY_ORDER"
total 20 · todos top-20 com auth=0.15 e fts=0.0000 · densa ausente

# com tag=scheduling — query "grade lote mensal cascata"
0.1481 auth=0.10 fts=0.0161 vec=0.0156 ent=0.0164 | estudos/global/scheduling/grade-lote-mensal-cascata   ← 1º

# com project=atlas-agenda, top_k 120 — query "grade lote mensal cascata"
total 51 · 31 candidatos com score >= 0.15 (todos auth puro)
POS 31 score 0.1318 auth=0.10 fts=0.01613(rank 2) vec=0.01563(rank 4) | estudos/global/scheduling/grade-lote-mensal-cascata
```

Interpretação: relevância máxima observável (rank 2 FTS + rank 4 vetor + pinned) = 0.1318; 31 concorrentes com **zero** sinal de relevância (`fts=0`, `rank_vec>=20`) pontuam ≥0.15 só por camada. Os 20 slots do MCP são consumidos por eles.

## Solução proposta (opções para B2 — NÃO implementar neste B1)

| # | Abordagem | Efeito | Risco |
|---|---|---|---|
| A (recomendada) | Boost **relativo**: `score = base * factor` (ex. `*1.15` arq/regras, `*1.10` pinned) em vez de aditivo absoluto (`lib.rs:2002-2003`). Preserva "camada desempata", nunca inventa sinal onde `base=0`. | `estudos` rank 1-4 mantém top; nota irrelevante sem boost não sobe | M — muda ranking global de todos os consumidores (MCP, viewer, CLI) |
| B | Boost **limitado** ao teto da relevância: `+min(0.15, base)` ou `+k*base`, `k` derivado de `4/61`. Equivalente a A com piso rígido; mais fácil de justificar numericamente. | idem A | M |
| C | Dois passes sob `project`: owned top_N + linked top_N, fundir antes do RRF. | ataca C3 (corte), **não** ataca C1 — a linked ainda perde na ordenação | M |
| D | Elevar `clamp(1,20)` do MCP e/ou `truncate(50)` sob projeto. | não resolve: posição 31 com `top_k=120` continua fora de 20; só mascara | B |

Combinação mínima recomendada: **A + teste de regressão de ordenação**; C como item separado se medição mostrar corte em query larga. Não fazer D sem A — aumentar o pool só aumenta o número de concorrentes boostados.

## Testes necessários (B2)

- [ ] **T1 discriminator (Risco M)** — in-mem: 31 distratores `regras/projetos/x/distra-N` sem match (chunks sem embedding ou vetor ortogonal) + 1 `estudos/global/alvo` pinned com match exato FTS e vetor rank 1; `search(..., project=Some("x"), ..., top_k=20)` → `alvo` **presente e no top 3**. Mutação que deve falhar o teste: voltar `boost += 0.15` absoluto.
- [ ] **T2 (Risco M)** — boost não inventa sinal: nota `regras` sem hit em nenhum stream não ultrapassa nota `estudos` com `rrf_fts` rank 1 (ordenação comparada por `score`).
- [ ] **T3 (Risco A)** — isolamento preservado: `search(project=other)` não retorna `alvo`; testes T1-T5 de `.spec/bugs/project-shared-search/TASKS.md` continuam verdes.
- [ ] **T4 (Risco M)** — `pinned` continua desempatando entre notas de mesma `base` (não regride a intenção original do boost).
- [ ] **T5 (Risco B)** — `explain.authority` reflete o novo esquema (campo continua reportando o delta efetivo, senão o `--explain` mente).
- [ ] **T6** — `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `.venv/bin/python -m pytest src/tests/ -q` (124 passed, 2 legadas) verdes.

## Riscos

- **R1 (A)** — mudar a escala do boost altera ranking de **todo** o corpus, não só do caso patológico; consumidores (viewer `/api/search`, MCP, CLI) herdam a mudança. Precisa de antes/depois medido em 3-5 queries reais antes do merge.
- **R2 (M)** — `explain.authority` documentado em `.agents/rules/harness-continuous.md` H4.1 com os valores `+0.15/+0.1` (`harness-continuous.md`, seção H4.1). Mudar o código sem atualizar a regra deixa a doc mentindo (H8).
- **R3 (B)** — A tabela H4.1 e a linha `lib.rs:1822-1823` citada na regra já estão com numeração defasada; corrigir junto.
- **R4 (M)** — fix de ordenação **não** resolve C3; issue #1 pode reaparecer em projeto grande com query de termo comum. Registrar C3 como débito separado, não fechar a issue como resolvida sem testar query larga.

## Verificação B1 (nada alterado)

- [x] leitura `lib.rs:1789-2007` (search, streams, fusão, boost), `lib.rs:1769-1787` (`fts_candidates`), `lib.rs:75-109` (`fts5_match_expr*`)
- [x] leitura `crates/brain-mcp/src/lib.rs:544` (`clamp(1,20)`)
- [x] repro read-only via `brain --db ./data/brain.db search ... --explain` (3 cenários) + `python3 sqlite3` URI `mode=ro`
- [x] nenhum processo de produção tocado (`:8321` PID 1370 / `:8322` intactos), nenhuma escrita em `data/brain.db`
- [x] código-fonte NÃO alterado

## Resultado B2+B3 (aplicado)

- [x] **T1** `authority_does_not_let_a_layer_outrank_relevance` — mutação (voltar aditivo) → alvo ausente do top-20, reproduz o sintoma
- [x] **T2** `authority_does_not_lift_a_thin_layer_hit_over_an_fts_hit`
- [x] **T3** `authority_does_not_leak_a_linked_target_into_another_project` + os 5 testes de `project-shared-search` intactos
- [x] **T4** `pinned_still_decides_between_near_equal_bases` — discriminante é o delta de `authority`, não a ordem (documentado)
- [x] **T5** `explain_authority_stays_the_effective_delta` — `score == base + authority`, composição `1.265x`
- [x] **T6** harness: 534 passed / 0 failed · clippy 0 · build 0 · pytest 124/2 legadas

Fix: `AUTHORITY_LAYER_FACTOR = 1.15`, `AUTHORITY_PINNED_FACTOR = 1.10` (`lib.rs:45,50`), ranking em `lib.rs:2055-2067`. Versão → **0.12.0**.

Medição (5 queries, `data/brain.db` read-only): alvo da issue **POS 1** nos dois casos patológicos (era 31 em 51 e 30 em 50). Notas com `score >= 0.15`: 31 → **0** na query de `atlas-agenda`, 29 → 0 sem filtro.

**Não resolvido (débito separado):** C3 `truncate(50)` por stream sob projeto grande e `clamp(1,20)` do MCP. Issue não deve ser fechada como "resolvida em qualquer query" sem o teste de query larga (R4).
