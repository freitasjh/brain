# Harness Contínuo (NON-NEGOTIABLE) — Rust SQLite-only

> 🚨 **REGRA MÁXIMA — fonte verdade harness brain Rust.** Substitui `workflow-rules.md` em conflito.
> Stack Rust 1.85 `cargo test --workspace` SQLite WAL FTS5 vet 768, ports 8321 MCP SSE 8322 viewer.

## H0 Princípios
1. Teste falhando = defeito produto. 2. Suite completa roda sempre (1 linha = suite). 3. Funciona na mão + CI. 4. Viewer quebrado = sistema quebrado. 5. Harness precede orgulho. 6. Regressão bloqueia release.

## H1 Suite Completa
### H1.1 Gatilhos — rodar suite antes declarar:
| Estado | Comando |
|--------|---------|
| Task concluída | `cargo test --workspace` |
| Sub-fase SDD | `cargo test --workspace && cargo build --workspace` |
| Bug corrigido | `cargo test --workspace` |
| PR/commit | `cargo test --workspace && cargo clippy --workspace -- -D warnings` |
| Sprint/release | `cargo test --workspace && cargo build --workspace` + harness E2E H3 |

### H1.2 Falha → protocolo
```
1. PARAR dev. 2. Report classe:linha msg stack. 3. Diagnosticar causa raiz. 4. Fix código/teste. 5. Re-rodar suite completa. 6. brain_store regras/estudos scope projetos|global
```

### H1.3 Comandos canônicos Rust
```bash
cargo test --workspace                    # suite completa (16 tests: 6 core +10 store)
cargo test -p brain-core -- --nocapture
cargo test -p brain-store -- --nocapture
cargo build --workspace                   # build 6 crates
cargo clippy --workspace -- -D warnings  # 0 warnings
cargo llvm-cov --workspace --html         # ≥70% cobertura
cargo run -p brain-cli -- --db ./data/brain.db status
```

## H2 Zero Tolerância Pré-existentes
### H2.1 Política
- Ref `cargo test --workspace` → 0 falhas 0 erros antes QUALQUER novo dev.
- Débito pré-existente = bug P1 saneamento. Não usar `#[ignore]` para passar suite.
### H2.2 Detecção antes dev
```bash
cargo test --workspace
# 0 falhas → prosseguir. N falhas → bloquear dev, criar .spec/tech-debts/{ctx}/TASKS.md por falha, sanear antes feature
```
### H2.3 Sprint saneamento
1. Listar falhas `.spec/tech-debts/{ctx}/TASKS.md` 2. Classificar 🔴P1 🟡P2 🟢P3 3. 🔴 antes feature mesmo domínio 4. SPEC→PLAN→TASKS→impl→review→verify 5. workflow-state.json `tech_debt_cleanup`

## H3 Harness E2E Greenfield Brain 12-step (Wipe + Re-cadastro)
### H3.1 Propósito — testes unit não pegam:
- SSE não sobe, 5xx em search RRF, viewer 404, scope sem validade, chunk não indexa, TTL não vence pin

### H3.2 Quando (MANDATORY)
| Momento | Freq |
|---------|------|
| Fim cada sub-fase SDD 1.x 2.x | por phase |
| Fim cada bug fullstack | por bug |
| Fim sprint semanal | semanal |
| Antes release/tag | por release |
| Após mudança brain-store/web/cli/mcp schema | imediato |

### H3.3 Protocolo 12 passos (brain Rust)
```bash
# 1. WIPE DB
rm -f /tmp/phasec.db ./data/brain.db && mkdir -p data
# 2. Build
cargo build --workspace
# 3. ServeMCP + Viewer em bg (opcional, smoke via CLI direto)
cargo run -p brain-cli -- --db /tmp/phasec.db status &
cargo run -p brain-cli -- --db /tmp/phasec.db serve --port 8322 &
cargo run -p brain-cli -- --db /tmp/phasec.db serve-mcp --port 8321 &
# 4. Smoke 12-step via CLI (sem rede) — EQUIVALE ao E2E
cargo run -p brain-cli -- --db /tmp/phasec.db ping                          #1 ping→pong
cargo run -p brain-cli -- --db /tmp/phasec.db store regras phasec/e2e "## E2E test" --scope global  #2 store
cargo run -p brain-cli -- --db /tmp/phasec.db search "E2E" --top-k 5 --explain  #3 search RRF k60 authority +0.15
cargo run -p brain-cli -- --db /tmp/phasec.db read regras phasec/e2e --scope global  #4 read
cargo run -p brain-cli -- --db /tmp/phasec.db recent --top-k 5              #5 recent
cargo run -p brain-cli -- --db /tmp/phasec.db export --to /tmp/brain-export --force  #6 export
cargo run -p brain-cli -- --db /tmp/phasec.db backup --to /tmp/phasec.bak   #7 backup
cargo run -p brain-cli -- --db /tmp/phasec.db store regras phasec/ttl "## ttl" --scope global --expires-at 2000-01-01T00:00:00Z  #8 TTL
cargo run -p brain-cli -- --db /tmp/phasec.db forget-sweep --dry-run        #9 sweep dry
cargo run -p brain-cli -- --db /tmp/phasec.db forget-sweep                  #9b sweep
cargo run -p brain-cli -- --db /tmp/phasec.db status                        #10 status
cargo run -p brain-cli -- --db /tmp/phasec.db checkpoints --limit 5         #11 checkpoints
curl http://localhost:8322/api/status | jq .                                #12 viewer 200
curl http://localhost:8321/sse | head -n 5                                   #12b MCP SSE pong
# 5. Viewer direct search/read
curl "http://localhost:8322/api/search?query=E2E&top_k=5" | jq .results
curl "http://localhost:8322/api/read?path=regras/global/phasec/e2e" | jq .content
# 6. Encerrar
lsof -ti :8321 | xargs kill -9 2>/dev/null; lsof -ti :8322 | xargs kill -9 2>/dev/null
```

### H3.4 Fluxo 12-step detalhado
1. ping pong 2. store regras global 3. search RRF k60 authority 4. read 5. recent 6. export /tmp/brain-export 7. backup .bak 8. store TTL expires 9. forget_sweep dry+real 10. status notes/chunks/projects 11. checkpoints 12. viewer /api/status 200 + /api/search

### H3.5 Validações por passo
| Aspecto | Critério |
|---------|----------|
| CLI exit | 0, stdout OK, sem 5xx |
| Viewer | 200 JSON notes/chunks/projects, /api/search results[] |
| MCP SSE | 200 text/event-stream pong, POST /mcp/search 200 |
| Console | zero ERROR/WARN |
| Ports | 8321 MCP + 8322 viewer coexistem |

### H3.6 Encerramento limpo
```bash
lsof -ti :8321 | xargs kill -9 2>/dev/null
lsof -ti :8322 | xargs kill -9 2>/dev/null
lsof -i :8321 || echo "8321 free"
lsof -i :8322 || echo "8322 free"
```

## H4 Avaliação Contínua
### H4.1 Checklist por área — cada entrega UI/fullstack/Rust:
**Search/Rank**
- [ ] RRF k60 híbrido FTS+vector+entity+graph + authority +0.15 regras/arquitetura +0.1 pin antes trunc?
- [ ] Batch IN ≤3q (filter 1q, assembly ≤3q) não N+1?
- [ ] FTS escape `*"():{}=-/` não injection?
- [ ] TTL expires_at vence pin?
**Store/Schema**
- [ ] WAL foreign_keys ON synchronous NORMAL?
- [ ] Scope obrigatório arquitetura/regras/estudos?
- [ ] Sanitize traversal `../` bloqueado?
- [ ] Chunk `## ` split + embedding 768 dim?
**Viewer/MCP**
- [ ] `/api/status,search,read,list` 200 JSON?
- [ ] `/sse` pong + stdio `BRAIN_TRANSPORT=stdio` fallback?
- [ ] AppState{db} per request evita !Sync?
**Performance**
- [ ] list recent limitado top_k?
- [ ] search truncate 50 FTS 50 vec antes fuse?

### H4.2 Melhorias
P1 quebra → `.spec/bugs/{ctx}/TASKS.md` P2 degrada → `.spec/tech-debts/{ctx}/TASKS.md` P3 cosmético → `.spec/roadmap/{ctx}/TASKS.md`
### H4.3 Brain
`brain_store(layer="regras", path="brain/<componente>", content="## Achado ...", scope="projetos"|"global")`

## H5 Encerramento Limpo + Brain
### H5.1 Processos `:8321/:8322`
```bash
lsof -ti :8321 | xargs kill -9 2>/dev/null; lsof -ti :8322 | xargs kill -9 2>/dev/null
lsof -i :8321 || echo "8321 free"; lsof -i :8322 || echo "8322 free"
```
### H5.2 Brain store tipos
| Tipo | Layer | Scope | Quando |
|------|-------|-------|--------|
| Erro/bug | regras/estudos | projetos/global | pós-fix |
| Decisão | arquitetura | projetos | pós-ADR |
| Estudo | estudos | global | após estudo |
| Sessão | sessoes | — | fim dia |
| Anti-pattern | regras | projetos/global | ao identificar |
### H5.3 Retorno subagente verifica
- [ ] processos finalizados ✅ `8321 free 8322 free`
- [ ] brain_store ✅ sessoes/brain/<date>
- [ ] suite completa ✅ `cargo test --workspace` 0 falhas

## H6 Portão Conclusão Expandido (7 itens) — NON-NEGOTIABLE
Antes declarar **QUALQUER** fase completa (feature/bugfix):
```markdown
## Pre-Completion Checklist (Harness Gate — Rust)
### Build & Testes
- [ ] `cargo test --workspace` → 0 falhas (6 core +10 store + mcp/web)
- [ ] `cargo build --workspace` → 0 erros
- [ ] `cargo clippy --workspace -- -D warnings` → 0 warnings
### Harness Funcional H3
- [ ] E2E brain 12-step ping→store→search RRF→read→recent→export→backup→sweep→status → todos OK
- [ ] Viewer curl /api/status 200 + /api/search 200, MCP /sse pong
- [ ] Network 0 5xx, Console 0 ERROR/WARN
### Avaliação Contínua H4
- [ ] Checklist search/store/viewer/mcp avaliado
- [ ] Melhorias P1→bugs P2/P3→tech-debts/roadmap
### Code Review
- [ ] fullstack-code-reviewer após última mudança
- [ ] Zero 🔴 Críticos, 🟡 corrigidos/justificados
### Brain & Workflow
- [ ] `brain_store` lições fase (regras/arquitetura/estudos scope correto)
- [ ] `workflow-state.json` updated `code_review_status: passed` + `harness_status: passed` + `version`
### Encerramento
- [ ] `lsof :8321 :8322 free` ✅ `hook-spool.jsonl` sem duplicata (lock)
```
**Qualquer ❌ = fase NÃO completa. Voltar corrigir.**

## H7 Cadência
| Atividade | Freq | Resp | Duração |
|-----------|------|------|---------|
| Suite cargo test | por task/bug | developer-engineer | 1-3 min |
| E2E 12-step | fim sub-fase + sprint + release | qa/developer | 5-10 min |
| Avaliação H4 | por entrega | developer+product | embutido |
| Brain store | por achado | qualquer | 2 min |
| Saneamento débito | quando falhas>0 | developer | variável |
| Auditoria regra | mensal | architect+qa | 2h |

## Anexo A Comandos Canônicos Rust
```bash
cargo test --workspace
cargo test -p brain-core -- --nocapture
cargo test -p brain-store -- --nocapture
cargo test -p brain-mcp -- --nocapture
cargo build --workspace && cargo clippy --workspace -- -D warnings
cargo llvm-cov --workspace --html  # ≥70%
cargo run -p brain-cli -- --db /tmp/phasec.db ping
cargo run -p brain-cli -- --db /tmp/phasec.db store regras foo "## Bar" --scope global
cargo run -p brain-cli -- --db /tmp/phasec.db search "query" --explain --top-k 5
cargo run -p brain-cli -- --db /tmp/phasec.db serve-mcp --port 8321 &
cargo run -p brain-cli -- --db /tmp/phasec.db serve --port 8322 &
cargo run -p brain-cli -- hook --event session-start --project myproj --payload '{"id":"1"}'
curl http://localhost:8322/api/status | jq
curl http://localhost:8321/sse
lsof -ti :8321 | xargs kill -9; lsof -ti :8322 | xargs kill -9
```

## Anexo B Template Relatório Harness Rust
```markdown
## Relatório Harness — phase-c-mcp-harness-hooks
### H1 Suite
- cmd: cargo test --workspace → ✅ PASS 16 passed 0 failed
- clippy: 0 warnings
### H3 E2E 12-step
- wipe: ✅ /tmp/phasec.db
- steps 1-12: ✅ ping pong, store ok, search RRF authority, read ok, recent 1, export ok, backup ok, sweep 1, status notes=1, checkpoints 2, viewer 200, mcp sse pong
- ports free: ✅ 8321 free 8322 free
### H4 Avaliação
- search batch 3q ✅ FTS escape ✅ TTL pin ✅ AppState per request ✅
### H5 Brain
- brain_store: ✅ sessoes/brain/2026-09-17 + hook-spool.jsonl lock dedup
### Conclusão
- fase completa: ✅/❌ bloqueios: [...]
```

## Governança
Precedência sobre workflow-rules em conflito. Violação = retrabalho completo. Audit mensal architect+qa.
**Última:** 2026-09-17 Rust rewrite 0.6.0 Phase C 12-step brain `cargo test --workspace`
