# AGENTS.md — brain

## Stack

**MCP server Rust (migration Python→Rust)** — servidor central de memória para agentes de IA. SQLite-only.

| Camada | Tecnologia |
|--------|-----------|
| Server | Rust 1.85 + axum + rmcp (MCP SDK) |
| Storage | SQLite-only `data/brain.db` (WAL, FTS5 porter, vec BLOB cosine) dim 768 fixo |
| Vault | Removido — export temporário `/tmp/brain-export` apenas |
| Embeddings | Ollama local (`nomic-embed-text`) |
| Transport | SSE (`BRAIN_PORT=8321` backend, `8322` web) |
| Testes | `cargo test` (brain-core) + `cargo build --workspace` |
| Legado Python | `src/brain_server/` mantido para compat até Fase C, `BRAIN_VAULT_PATH` deprecated |

## Estrutura

```
crates/
├── brain-core/src/lib.rs   → types (Note/Project/SearchResult), validate/sanitize, chunk ##, wikilink
├── brain-store/src/lib.rs  → Store SQLite WAL, FTS5 porter, vec BLOB cosine, RRF k=60, TTL, audit
├── brain-embed/src/lib.rs  → Ollama client (nomic-embed-text dim 768 rustls)
├── brain-mcp/src/lib.rs    → rmcp tools + axum SSE
├── brain-web/src/lib.rs    → axum viewer /api/* (8322)
└── brain-cli/src/main.rs   → clap 15 cmds (BRAIN_DB_PATH)
src/brain_server/           → legado Python (compat Fase C, vault deprecated)
data/brain.db               → SQLite-only truth (WAL)
```

## Comandos exatos (Rust — fonte da verdade)

```bash
cargo test --workspace                 # 213 testes: 10 brain-core, 56 brain-store, 73 brain-mcp (57 lib + 16 fila), 37 brain-embed, 19 CLI e2e, 6 vec_compat, 2 zero_vector_guard, 7 CLI bin, 3 brain-web
cargo test -p brain-core -- --nocapture
cargo build --workspace                # debug; release: --release (LTO thin)
cargo run -p brain-cli -- --db ./data/brain.db status
cargo run -p brain-cli -- store regras foo "## Bar" --scope global
cargo run -p brain-cli -- search "query" --explain --top-k 5
cargo llvm-cov --workspace --html      # cobertura ≥70%
# Legado Python (Fase C remove):
uv run pytest src/tests/ -v
```

## CLI (`brain` command)

Use o CLI `brain` para interagir com o servidor via terminal.
O servidor precisa estar rodando em modo SSE primeiro.

```bash
# Terminal 1: inicia servidor
BRAIN_TRANSPORT=sse uv run python -m brain_server

# Terminal 2: usa o CLI
uv run brain ping                                    # health-check

# Salvar nota (scope obrigatório para arquitetura/regras)
uv run brain store regras meu-projeto/naming "## Regra" --scope projetos
uv run brain regras coding-standards "## Padrões" --scope global
uv run brain store sessoes 2026-07-25 "## Sessão"   # sessoes não usa scope

# Ler nota com scope
uv run brain read regras meu-projeto/naming --scope projetos

# Buscar com filtro de scope
uv run brain search "select" -k 3
uv run brain search "padrões" --layer regras --scope global
uv run brain search "naming" --layer regras --scope projetos

uv run brain reindex --all                           # reindexa tudo

# URL customizada (default http://localhost:8321/sse)
BRAIN_URL=http://localhost:9000 uv run brain ping
```

## Variáveis de ambiente (prefixo `BRAIN_`)

| Variável | Default | Descrição |
|----------|---------|-----------|
| `BRAIN_DB_PATH` | `./data/brain.db` | SQLite-only DB (WAL + FTS5) — **BRAIN_VAULT_PATH/BRAIN_INDEX_PATH removidos** |
| `BRAIN_OLLAMA_URL` | `http://localhost:11434` | URL do Ollama |
| `BRAIN_OLLAMA_MODEL` | `nomic-embed-text` | Modelo de embedding (dim 768 fixo) |
| `BRAIN_PORT` | `8321` | Porta MCP SSE |
| `BRAIN_VIEWER_PORT` | `8322` | Porta web viewer `/api/*` |
| `BRAIN_LOG_LEVEL` | `INFO` | Nível de log |
| `BRAIN_EMBED_TIMEOUT_SECS` | `60` | Budget **base** por onda de embed. O budget do batch é `base × min(ceil(n/paralelismo), 8)` — ver `EmbeddingEngine::batch_timeout` |
| `BRAIN_EXPORT_ROOT` | `/tmp/brain-export` | **Allowlist de escrita** de `brain_export(to)` e `brain_backup(to)`. Qualquer destino fora desta raiz é rejeitado (canonicalizado, então `..` e symlink não passam) |
| `BRAIN_REUSE_SIMILARITY` | `1.0` | Opt-in **explícito** para reusar vetor através de edição de texto (ex.: `0.9`). Abaixo de 1.0 um chunk pode herdar o vetor do texto antigo; a guarda de negação (§ W-02) continua valendo em qualquer valor |
| `BRAIN_HOOK_EMBED` | `1` | `0` desliga o embed no `brain hook` — a captura do evento nunca depende disso |
| `BRAIN_BENCH_OUT` | `bench-hybrid.json` (no repo) | Saída do bench híbrido. No repo e não em `/tmp` porque a evidência de recall não pode ser efêmera |
| `BRAIN_BENCH_FORCE` | unset | `1` sobrescreve a saída do bench; senão a anterior vira `.prev` e sai um aviso |
| `BRAIN_EMBED_MAX_FAILURES` | `8` | Teto de tentativas de embed por nota antes do dead-letter. ≈1 min de backoff acumulado, cobrindo restart/cold start do Ollama. `0`/inválido = default. Também sai em `brain status` como `queue.max_failures` |

## Comandos CLI (`brain setup`)

| Comando | Descrição |
|---------|-----------|
| `brain setup all` | One-shot: MCP + rules + systemd |
| `brain setup copilot` | Apenas MCP server do brain para Copilot |
| `brain setup copilot-instructions` | Rules de como USAR brain para Copilot Chat |
| `brain setup kiro` | Apenas MCP server do brain para Kiro |
| `brain setup kiro-steering` | Steering rules de como USAR brain para Kiro |
| `brain setup opencode` | MCP server do brain para OpenCode |
| `brain setup systemd` | Serviço systemd user para auto-start |

## Tools MCP expostas

| Tool | Descrição |
|------|-----------|
| `ping` | Health-check → `"pong"` |
| `brain_store(layer, path, content, scope?, project?, tags?, pinned?, expires_at?)` | Salva nota SQLite-only + FTS5 **imediato**; embedding vai para a fila (US-02.7). Responde `{chunks, embedded, without_embedding, queued}` — `queued>0` significa vetor pendente. Rejeita `content` acima de `MAX_CONTENT_BYTES` (256 KiB) ou `MAX_CHUNKS` (64) **antes** de qualquer embed. Scope obrigatório para arquitetura/regras/estudos |
| `brain_read(layer, path, scope?)` | Lê nota SQLite (`notes.path`). Scope obrigatório para arquitetura/regras/estudos |
| `brain_search(query, layer?, scope?, project?, tag?, top_k?, explain?)` | Híbrido FTS5+vector RRF k60 + entity+graph + authority. Fallback FTS se Ollama down |
| `brain_delete_page(path)` | Delete hard + audit |
| `brain_recent(top_k?)` | Últimas notas `updated_at DESC` |
| `brain_status` | `{notes,chunks,projects,db,version,embedding:{coverage:{chunks_total,chunks_embedded,chunks_without_embedding,chunks_zero_vector,embedding_coverage_pct}, ollama:{reachable,model}}, queue:{pending_len,ready_len,is_draining,dead_lettered,max_failures,last_drain:{embedded,nulls,retriable_nulls,requeued,dead_lettered,skipped_locked,failed,pending_left,settled},embed_lock_holder,embed_lock_age_s,embed_lock_expires_in_s}}`. `coverage_pct` é o sinal de fila travada: chunk pendente conta como `without_embedding`, nunca `zero_vector`. O bloco `queue` distingue "nada a fazer" de "aguardando backoff", de "outro embed rodando há N s" e de "desisti desta nota" (`dead_lettered` > 0 = terminal neste processo; o próximo boot e o `reindex --all` leem os `NULL`) |
| `brain_checkpoints(limit?)` | Audit log `action/path/at` |
| `brain_restore_page(id)` | Restore versão audit |
| `brain_backup(to?)` | `cp brain.db → .bak`. Sem `to` = `{db}.bak` (sempre permitido); com `to` precisa ser absoluto, terminar em `.bak` e estar **dentro de `BRAIN_EXPORT_ROOT`** |
| `brain_export(to?, force?)` | Dump `{BRAIN_EXPORT_ROOT}/layer/scope/path.md` temp. `to` é restringido à raiz (allowlist, canonicalizada), e a raiz precisa ser **diretório, do nosso euid, e não gravável por `other`** — senão outro usuário local que tenha criado `/tmp/brain-export` antes do primeiro start lê o corpus e o `.bak` do banco (checado em `export_dir` e **de novo depois** do `create_dir_all`, para fechar a janela de TOCTOU). O path de cada nota é rechecado na escrita — verificado no wiring por `crates/brain-mcp/tests/export_guard.rs::a_stored_path_that_climbs_out_of_the_export_root_is_not_written`, que planta uma row com `path='../escape'` por SQL e roda o `POST /mcp/export` de verdade. Responde `{ok,to,written,refused}` |
| `brain_forget_sweep(dry_run?)` | TTL `expires_at` hard-delete (pin não vence) |
| `brain_reindex` (CLI `brain reindex --all [--no-embed]`) | **Não destrutivo**: nunca `DELETE FROM chunks`; reusa vetores whose texto ainda bate. Embeda antes da transação e reporta `REINDEX_DONE notes= chunks= embedded= preserved= rehydrated= null= diverged= stale_reused= unmatched=`. `diverged>0` = nota editada durante o run, vetor stale **não** aplicado (`REINDEX_DIVERGED`). Rejeita se outro embed segura o lock |
| `brain_project_create(name, description?)` | Cria projeto |
| `brain_project_list()` | Lista projetos |
| `brain_project_notes(project)` | Lista notas owned+linked |
| `brain_project_link(note_path, project)` | Liga nota global |
| `brain_project_unlink(note_path, project)` | Desliga nota |
| `brain_project_delete(name)` | Deleta projeto |
| `brain_migrate(vault?, old_index?)` | Import legado `vault/*.md` → `brain.db` |
| `brain serve --port` | Viewer axum 8322 `/api/status,search,read,list` |
| `brain serve-mcp --port 8321` | MCP SSE rmcp 8321 `/sse`, per-request Store + fila de embedding em background, coexist viewer 8322 |
| `brain server start [--port] [--vault] [--old-index]` | **O nome da spec (US-01.1).** Auto-importa o legado (`vault/`) e **arquiva em `vault.bak.tar.gz`** dentro de `BRAIN_EXPORT_ROOT` *antes* de servir; se o archive falhar, o import não roda. Bind e ferramentas idênticos a `serve-mcp`; shutdown idêntico (SIGINT **ou** SIGTERM). `serve`/`serve-mcp` não importam — use `migrate` se não quiser o archive |
| `brain hook --event session-start\|tool-result\|session-end --project --payload JSON` | Hook Rust spool `XDG_RUNTIME_DIR/brain/hook-spool.jsonl` file lock + Store `sessoes/brain/<date>` |

## Fila de embedding e limites de escrita (US-02.7)

Uma escrita **não espera por vetor**. `brain_store` grava nota + chunks com
`embedding = NULL` numa transação que já popula o FTS5, responde, e a fila embeda
em background. Medido no caminho real do protocolo MCP com o Ollama no ar:

| chunks | `brain_store` (MCP) | dreno da fila | `brain store` (CLI, síncrono) |
|--------|---------------------|---------------|--------------------------------|
| 1 | — | — | 0.04 s |
| 8 | **0.018 s** | +0.59 s → 8/8, 100% | 0.40–0.59 s |
| 64 (máx) | **0.019 s** | +2.75 s → 64/64, 100% | 2.77–3.94 s |

Regras:

- **Limites de escrita** (`brain_core::MAX_CONTENT_BYTES` = 256 KiB,
  `MAX_CHUNKS` = 64) validados nos 3 paths de escrita — MCP rmcp, REST axum e CLI
  — **antes** de qualquer embed. Erro `INVALID_PARAMS` nomeando o limite. Motivo:
  embedding é serial (`OLLAMA_NUM_PARALLEL=1`, ~0.045 s/chunk medido), então o
  custo da escrita escalava com o número de chunks sem teto. Os limites são
  dimensionados por `PESSIMISTIC_SECONDS_PER_CHUNK` (0.4 s, ~9x o medido), não
  pelo número mais rápido já observado.
- **`queued=N` no retorno** é o contrato: `embedded` é 0 numa nota nova e o
  vetor chega depois. `coverage_pct` em `brain status` é o sinal de fila travada.
- **A fila é um diff.** `Store::chunks_needing_embedding` devolve só os chunks sem
  vetor válido, então regravar uma nota que cresceu custa 1 embed, não N.
- **Ollama fora** → enfileira, loga, `nulls` no relatório. Nunca erro ao cliente.
  Uma falha de embed **reenfileira** o job com backoff (`finish_owed`); só depois
  de `BRAIN_EMBED_MAX_FAILURES` tentativas (default 8 ≈ 1 min de backoff) a nota
  vai para **dead-letter**: sai da fila, `brain status` reporta
  `queue.dead_lettered`, e os chunks ficam `NULL` — que é o registro que o
  `recover` do próximo boot e o `reindex --all` leem. Retry sem teto seria CPU
  eterna; sem retry, o vetor sumiria em silêncio. `QueueOutcome::is_settled()` é
  falso enquanto houver `NULL` com retry agendado.
- **Lock** (`_meta.embed_lock`, TTL 900 s, `Store::try_acquire_embed_lock`) impede
  que a fila do servidor e um `brain reindex` (processo separado) embedem o mesmo
  chunk duas vezes. Perdida a corrida custa CPU duplicada, nunca escrita errada.
  Recusar o lock por causa do reindex **não** conta como falha de embed: são
  contadores separados (`attempts` para o backoff, `failures` para o teto), senão um
  reindex segurando o lock pelos 900 s do TTL mataria trabalho saudável.
- **CLI embeda inline** (não enfileira): processo one-shot morre com a task de
  background e a nota ficaria sem vetor para sempre. Custo limitado por
  `MAX_CHUNKS`.

### ⚠️ Ordem de deploy (8321/8322) — janela de incompatibilidade known

O valor de `_meta.embed_lock` mudou de `owner|expires` para
`owner|taken_at|expires` **sem DDL** (o formato é o valor da linha, não a coluna).

- **Escritor novo** não rouba um lock legado ainda vivo: sem `taken_at` não há
  idade, e o que está vivo é respeitado. Lock expirado é retomado normalmente.
- **Binário antigo** lê o campo 2 (`taken_at`) como se fosse `expires`. Como
  `taken_at` é um timestamp bem menor que um expiry, o binário antigo julga o lock
  **expirado** e o rouba enquanto o novo o considera vivo.

**Impacto é CPU duplicada, nunca escrita errada.** Toda escrita de chunk é
`INSERT OR REPLACE` chaveada em `(path, chunk_index)` e re-verificada contra o texto
do chunk, então dois embeds concorrentes no mesmo chunk escrevem o mesmo vetor; e a
divergência se auto-cura no run seguinte. **Ainda assim, o deploy de 8321/8322 deve
preceder qualquer outro uso** — em particular, não rodar `brain reindex` contra um
servidor ainda no binário antigo enquanto o novo não subiu.

## Estrutura de Camadas com Scope

As camadas `arquitetura`, `regras` e `estudos` usam **scope** para separar conteúdo:

| Camada | Scope | Conteúdo | Exemplo |
|--------|-------|----------|---------|
| `arquitetura` | `projetos` | Stack, módulos, decisões específicas do projeto | `arquitetura/projetos/meu-app/stack.md` |
| `arquitetura` | `global` | Padrões universais, melhores práticas | `arquitetura/global/patterns.md` |
| `regras` | `projetos` | Regras de negócio específicas do projeto | `regras/projetos/meu-app/naming.md` |
| `regras` | `global` | Lições aprendidas, padrões de código | `regras/global/coding-standards.md` |
| `estudos` | `projetos` | Estudos completos específicos do projeto | `estudos/projetos/meu-app/analise-db.md` |
| `estudos` | `global` | Conhecimento geral (ex: Java, Docker) | `estudos/global/java-oo.md` |
| `sessoes` | — | Resumos de sessão (já é por projeto) | `sessoes/meu-app/2026-07-25.md` |
| `projetos` | — | Metadados e visão geral | `projetos/meu-app.md` |

**Regra**: Scope é **obrigatório** para `arquitetura`, `regras` e `estudos`. Previne vazamento de contexto entre projetos.

## /brain skill

Em `.agents/skills/brain/SKILL.md` — carregue em outros repositórios via:

```json
{ "instructions": ["../brain/.agents/skills/brain/SKILL.md"] }
```

Skills individuais por tool em `.agents/skills/brain/tools/`.

## Consistência — Regras Obrigatórias

### Ao adicionar ou modificar uma MCP tool, DEVE atualizar TODOS:

1. `crates/brain-store/src/lib.rs` — Store schema/search/audit se afeta storage
2. `crates/brain-core/src/lib.rs` — types/validate se afeta Note/SearchResult
3. `crates/brain-mcp/src/lib.rs` — rmcp tool registry + sanitize
4. `crates/brain-cli/src/main.rs` — clap Cmd variant + full_path
5. `crates/brain-web/src/lib.rs` — `/api/*` se exposição read-only
6. `cargo test --workspace` — teste (brain-core + store in-mem)
7. `AGENTS.md` — tabela de tools
8. `.agents/skills/brain/SKILL.md` — documentação da tool
9. `.agents/skills/brain/tools/<tool>/SKILL.md` — skill individual
10. `hooks/brain-hook.py` — se auto-capture afetado
11. **Legado** `src/brain_server/tools/<tool>.py` — manter compat até Fase C

### Ao adicionar endpoint REST:

1. `crates/brain-web/src/lib.rs` — router
2. `AGENTS.md` — tabela de endpoints
3. `cargo test -p brain-web` — teste

### Ao alterar schema do índice:

1. `crates/brain-store/src/lib.rs` — SCHEMA_VERSION bump 4→5, init_schema triggers
2. `crates/brain-core/src/lib.rs` — IndexEntry / SearchResult
3. `cargo test -p brain-store` — teste store in-mem
4. `crates/brain-cli/src/main.rs` — migrate path se necessário

## Regras de desenvolvimento

- **Versionamento obrigatório**: Ao adicionar features, fixes ou breaking changes, SEMPRE atualizar a versão em TODOS os arquivos:
  1. `Cargo.toml` → `workspace.package.version = "x.y.z"`
  2. `pyproject.toml` → `version = "x.y.z"` (legado Fase C)
  3. `src/brain_server/__init__.py` → `__version__ = "x.y.z"`
  4. `workflow-state.json` → `version`, `test_count`, `tasks_completed`, `last_updated`
  - **Patch** (x.y.Z): fixes, docs, refactoring sem mudança de API
  - **Minor** (x.Y.0): novas features, tools, parâmetros (backward-compatible)
  - **Major** (X.0.0): breaking changes, remoção de tools, mudança de schema
- **Spec-driven-development** para features novas (`.spec/brain-rust-sqlite/`, `.spec/<feature>/`)
- **workflow-state.json** deve ser lido/criado antes de começar (campos `version,current_feature,current_phase,artifacts`)
- **Code review** obrigatório ao finalizar tarefas
- **Testes**: `cargo test --workspace` antes de cada commit; legado `uv run pytest src/tests/ -v` até Fase C
- **Cobertura mínima**: 70% (`cargo llvm-cov --workspace --html`)
- **Orchestrator** não desenvolve — delega via `task` para `developer-engineer` (ver `.agents/rules/workflow-rules.md:0.1`)

## Regras específicas para hooks/

Os hooks em `hooks/` NÃO são apenas para auto-start. Eles são a **fonte única de
verdade** para rules/instructions/steering de cada IA:

| Arquivo | Instalado por | Destino |
|---------|---------------|---------|
| `hooks/brain-copilot-instructions.md` | `brain setup copilot-instructions` | `~/.config/Code/User/brain-copilot-instructions.md` |
| `hooks/brain-kiro-steering.md` | `brain setup kiro-steering` | `~/.kiro/steering/brain.md` |

**Sempre** que alterar um desses hooks, o instalador correspondente deve ser
atualizado OU a fonte do hook deve ser usada (cópia literal do arquivo).
O instalador tem fallback embedded (funções `_default_*`), mas o ideal é
manter os hooks como fonte.

### Concorrência do `brain hook` (X-01)

O hook roda em **processo separado, um por tool-result**, e esses processos se
sobrepõem por construção. Três garantias, e as três são testadas:

1. **O append é atômico.** `Store::note_append_section` faz read *e* write num
   único `BEGIN IMMEDIATE`, então nenhum interleaving é observável. Antes eram três
   passos (`note_get` → monta → `note_upsert`) sem lock: 2/8 e 7/8 eventos perdidos
   sob concorrência, com o controle sequencial 8/8. O lock do spool **não** serializa
   o DB (é por arquivo de spool, e é liberado antes de qualquer trabalho de banco) e
   foi rejeitado por isso — segurá-lo através do embed serializaria projetos
   diferentes entre si.
2. **Abrir o banco não toma o write lock.** `init_schema` lê o marcador de versão e
   só roda o DDL quando ele não bate (fresh ou v3), em transação `IMMEDIATE`. Antes
   re-executava o batch inteiro a cada `Store::open` — ou seja, em todo request MCP e
   duas vezes por evento de hook — e um `status` read-only perdia por `SQLITE_BUSY`.
3. **O marcador de dedup é um token inteiro.** `id=d1` é substring de `id=d11`, e o
   evento curto era descartado como duplicata — perda silenciosa, alcançável em
   execução **sequencial**. Por isso o marcador carrega o `\n` do heading.

Testes: `crates/brain-cli/tests/cli_e2e.rs::e2e_concurrent_hooks_never_lose_a_session_event`
(12 processos reais × 8 rodadas) e `::e2e_hook_captures_an_event_whose_id_is_a_prefix_of_another`.

### Gaps conhecidos (fora do escopo do lote X)

- **Não existe `SKILL.md` para `brain_export` / `brain_backup`** em
  `.agents/skills/brain/tools/`. Gap pré-existente, registrado e **não** corrigido
  aqui: as duas tools escrevem no filesystem e a doc delas é a parte que mais
  precisa de instrução de uso.
- `hooks/brain-hook.py` está **deprecated** (usa `uv run brain`, sem spool). Agora
  checa `returncode` e reporta em stderr em vez de devolver string vazia.
  - **A mensagem de falha diz, literalmente, que o spool não é lido** e que **não há
    retry** — alinhado com o que o código faz. A versão anterior dizia que o
    `brain hook` "spools the event and retries on its own", e isso era falso na
    direção cara: o spool tem **um escritor** (`hook_handle`) e **nenhum leitor** em
    qualquer crate. O `brain hook` faz o trabalho **inline**; se `note_append_section`
    ou `sync_note_chunks` falhar, o evento fica só no spool e nada o reproduz. O
    payload não se perde (é uma linha JSON inteira em disco), mas a frase mandava o
    operador esperar por uma recuperação inexistente. Fixado em Z-01, com teste em
    `src/tests/test_brain_hook_failure.py::test_the_failure_message_does_not_promise_a_retry_that_does_not_exist`
    (falha se a frase antiga voltar).
  - **Decisão registrada (Z-01), não implementada**: o `.py` **sai do caminho
    suportado na Fase C** e o `brain hook` **não** ganha drain do spool. Um drain
    exigiria um leitor do formato do spool em processo de boot, o que é um item de
    produto, não de correção de texto — e nada hoje perde dado, porque o payload
    está no disco e o operador é avisado para reenviar. Se a decisão mudar, o drain é
    trabalho novo, não um bug de hook.
- **A suíte Python é um gate obrigatório**: `.venv/bin/python -m pytest src/tests/ -q`
  → **114 passed, 2 failed**. Os 2 são `test_integration.py::test_integration_full_cycle`
  e `::test_integration_errors`, que sobem o MCP **legado** por stdio e morrem com
  `No module named brain_server` (o `.venv` roda o MCP em `uv run`/3.11, sem o
  `src/` no path) — é **ambiental, do código que a Fase C remove**, não do produto.
  Afirmação anterior de que "pytest não está instalado neste ambiente" estava **errada**
  (Z-05): pytest 9.1.1 está no `.venv`, e `test_brain_hook_failure.py` roda — 15
  testes, todos reais, e discriminantes. As suítes Rust e Python são gates
  independentes; ambas têm de passar.

## Regras de uso para projetos consumidores

Projetos que consomem o brain como memória central devem incluir:

1. **`BRAIN.MCP.md`** — regras completas de uso do Brain MCP.
   ```json
   { "instructions": ["../brain/.agents/rules/BRAIN.MCP.md"] }
   ```
2. **Skill /brain** — instruções detalhadas para agentes.
   ```json
   { "instructions": ["../brain/.agents/skills/brain/SKILL.md"] }
   ```
3. **MCP Server** — conectar ao brain via SSE.
   ```json
   { "mcpServers": { "brain": { "transport": "sse", "url": "http://localhost:8321/sse" } } }
   ```

O arquivo `.agents/rules/BRAIN.MCP.md` contém a documentação completa
com exemplos, boas práticas e checklist.
