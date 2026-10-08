# TASKS — project-shared-search (bugfix B1 análise, sem fix aplicado)

Legenda: [ ] pendente · Risco: A=alto M=médio B=baixo · BDD uma linha por cenário.

## Bug

- search "mobile devolução offline" com `project=mobile` → 0 resultados; mesma query com `project=progaterp` → 5 resultados (decisao-mobile-sem-devolucao, regra-duplicada-offline, etc.).
- Sessões ficam às cegas: `sessoes/*` quase nunca aparece sob filtro `project`.
- Projetos reais: `mobile`(id 2), `mobile-erp`(id 3), `mobile_erp`(id 7), `progaterp`(id 6).

## Causa raiz (confirmada, só leitura, sem alterar código/fonte nem data/brain.db)

- **C1 — vector pre-filter owned-only exclui linked.** `crates/brain-store/src/lib.rs:1844`: `filter_sql_parts.push("project_id = ?")` sobre `chunks`. `chunks.project_id` é posse (`chunk_insert` em `lib.rs:1346-1364`), `note_projects` nunca entra no WHERE do scan vetorial. Post-filter em `lib.rs:1966-1970` aceita `owned_match || linked_match`, mas o vetor linked já foi descartado antes do `truncate(50)` em `lib.rs:1883` — nunca chega à fusão RRF. Prova em produção: `SELECT count(*) FROM chunks WHERE project_id=2` → **0** (projeto `mobile` não possui nenhum chunk); logo stream vetorial para `project=mobile` é sempre vazio.
- **C2 — FTS global top-50 + post-filter = corte prematuro.** `fts_candidates` em `lib.rs:1773-1782`: `SELECT title, rank FROM notes_fts ... LIMIT 50` sem filtro; depois `lib.rs:1966-1970` descarta o que não é owned/linked. Nota relevante além do top-50 global nunca entra no RRF. Não é a causa do zero atual (FTS achou os 5 de progaterp), mas trunca recall sob filtro `project`.
- **C3 — `sessoes/*` sem dono e sem link = invisível sob filtro.** `SELECT project_id, count(*) FROM notes WHERE layer='sessoes' GROUP BY project_id` → `NULL:127, 1:41, 4:5, 5:1, 6:1, 7:1`. `sessoes/mobile-erp/*` = 28 notas, 28 com `project_id IS NULL`. Post-filter `lib.rs:1966-1970` exige owned ou linked; NULL + sem link → `continue`. `note_projects` tem 82 linhas totais, **zero** com `note_path LIKE '%mobile%'`, e só 1 link para `project_id=2` (`arquitetura/projetos/progaterp/mcp-server`, owned por 6). Notas `progaterp` com "mobile" no path (`validacao-item-mobile-tempo-real`, `decisao-mobile-sem-devolucao`, `regra-duplicada-offline`, todas `project_id=6`) são corretamente descartadas para `project=mobile` pela lógica atual — o usuário esperava vocabulário compartilhado, o código entrega isolamento por posse.
- **Não-causas (descartadas):** `project_notes` owned+linked em `lib.rs:1090-1101` OK (duas queries + dedup); repasse `project` OK no MCP (`crates/brain-mcp/src/lib.rs:527`, `crates/brain-mcp/src/rmcp_service.rs:267` com `clamp(1,20)`) e no CLI (`crates/brain-cli/src/main.rs:1306-1310` repassa `project.as_deref()` ao `store.search`).

## Esperado vs atual

- Atual: `project=X` = "notas cuja posse é X OU linkadas a X", mas stream vetorial só vê posse, FTS vê top-50 global, e `sessoes` sem posse some.
- Esperado (a decidir em B2 com usuário): ou (a) `project` inclui linked no vetor + `sessoes` ganha dono/link no hook, ou (b) `project` vira hint de boost em vez de filtro duro. Sem decisão, qualquer fix quebra isolamento (`progaterp` ↔ `mobile` hoje separados por `project_id`).

## Solução proposta (B2, NÃO implementar neste B1)

1. `crates/brain-store/src/lib.rs:1832-1848` — vetor: trocar `project_id = ?` por `path IN (SELECT path FROM notes WHERE project_id=? UNION SELECT note_path FROM note_projects WHERE project_id=?)` ou remover pre-filter e filtrar no post (custo: scan maior; medir contra `chunks` 1853 linhas). Arquivo único, impacto isolado ao stream vec.
2. `lib.rs:1773-1782` — FTS: aplicar filtro `project` antes do `LIMIT 50` (join `notes`+`note_projects`) em vez de post-filtrar; ou elevar limite sob filtro. Impacto: query FTS muda, sem schema.
3. Hook/store `sessoes`: garantir `project_id` ou link `note_projects` na escrita (`brain hook --project`, `store --project`); backfill pontual das 28 `sessoes/mobile-erp/*` NULL via `brain_project_link` (decisão de produto, não migração silenciosa).
4. Não quebrar isolamento: `project` inexistente continua `skip_vector` (`lib.rs:1837`) + post-filter descarta tudo; `project=None` inalterado; `mobile` vs `mobile-erp` vs `mobile_erp` vs `progaterp` continuam ids distintos.
5. Docs: atualizar `.agents/rules/BRAIN.MCP.md` semântica de `project` (filtro owned+linked, `sessoes` NULL invisível até linkar).

## Testes necessários (B2)

- [ ] **T1** [ ] unit in-mem: nota owned-A + nota owned-B linkada a A; `search(project=A)` retorna as duas no stream vec (mutação: voltar `project_id=?` → falha). (Risco M)
- [ ] **T2** [ ] unit in-mem: `sessoes` com `project_id NULL` sem link some sob `project=X`; com link aparece. (Risco B)
- [ ] **T3** [ ] unit in-mem: `project` inexistente → 0 resultados, sem erro. (Risco B)
- [ ] **T4** [ ] unit in-mem: isolamento — `search(project=mobile)` não retorna nota owned só por `progaterp` sem link. (Risco A)
- [ ] **T5** [ ] BDD: FTS além do top-50 global com filtro `project` ainda ranqueia (seed 60 notas distratoras + 1 alvo do projeto). (Risco M)
- [ ] **T6** [ ] `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` verdes antes de declarar B2 completo (gate H1.3; NÃO rodado neste B1 por ordem da delegação).

## Riscos

- Remover pre-filter vetorial sem medir → scan full `chunks` por query (perf; hoje 1853 linhas, cresce). Mitigar com subselect por path. (Risco M)
- Backfill de `sessoes` NULL como dono em vez de link → muda posse, quebra `project_notes` owned. Preferir `note_projects`. (Risco A)
- Três projetos `mobile*` quase homônimos (id 2/3/7) → typo passa em silêncio (`store` cria projeto se não existir). Fix não resolve; documentar `project_list` antes de filtrar. (Risco B)
- FTS `LIMIT 50` elevado sem bound → latência; bound por `top_k` + filtro no SQL. (Risco B)

## Evidências SQL (somente leitura `file:data/brain.db?mode=ro`, 2026-10-07)

- `projects`: (1 atlas-ecm) (5 hive) (2 mobile) (3 mobile-erp) (7 mobile_erp) (4 progat-b2b) (6 progaterp)
- `notes WHERE layer='sessoes' GROUP BY project_id`: NULL 127 / 1:41 / 4:5 / 5:1 / 6:1 / 7:1
- `notes WHERE project_id=2`: 0 linhas; `chunks WHERE project_id=2`: 0; `chunks GROUP BY`: NULL 1173 / 1:435 / 3:9 / 4:35 / 5:71 / 6:129 / 7:1
- `note_projects`: 82 linhas; `WHERE note_path LIKE '%mobile%'`: 0; `WHERE project_id=2`: 1 (`arquitetura/projetos/progaterp/mcp-server`, owned 6)
- `sessoes/mobile-erp/*`: 28 notas, 28 com `project_id IS NULL`

## Verificação B1 (nada alterado)

- [x] grep/read `lib.rs:1773-1782,1832-1848,1966-1970,1090-1101` + MCP `lib.rs:527`/`rmcp_service.rs:267` + CLI `main.rs:1306-1310`
- [x] SQL read-only acima; nenhum `rm`/write em `data/brain.db`
- [x] `cargo test` NÃO rodado (ordem); código-fonte NÃO alterado
