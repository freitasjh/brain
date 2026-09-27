# Harness Contínuo (NON-NEGOTIABLE) — Rust SQLite-only

> 🚨 **Fonte da verdade do harness do brain.** Precede `workflow-rules.md` em conflito.
> Toda afirmação técnica deste arquivo cita `arquivo:linha`. Se você não consegue
> apontar o código, **não escreva a afirmação** — ver H8.

## H0 Princípios

1. Teste falhando = defeito de produto, não do teste.
2. Suite completa é 1 linha. Rodar um subconjunto e chamar de "passou" é mentira.
3. Funciona na mão **e** no CI.
4. Viewer quebrado = sistema quebrado.
5. Harness precede orgulho.
6. Regressão bloqueia release.
7. **Documento desatualizado é defeito.** Este arquivo já afirmava `16 tests` quando
   havia 326, e mandava `rm -f ./data/brain.db` contra um servidor no ar. Truthfulness
   é parte do portão, não um extra.

## H1 Suite Completa — **dois portões independentes**

### H1.1 Os números reais (medidos em `0d1b6cb` / `v0.9.1`)

| Crate | Testes | Binários |
|-------|--------|----------|
| `brain-cli` | **63** | 18 `main.rs` + `cli_e2e` 27 + `mcp_sse_window` 5 + `server_start` 8 + `serve_viewer_shutdown` 2 + `server_start_stdio_shutdown` 2 + `serve_mcp_shutdown` 1 |
| `brain-store` | **79** | 71 `lib.rs` + `vec_compat` 6 + `zero_vector_guard` 2 |
| `brain-mcp` | **99** | 68 `lib.rs` + `embed_queue` 29 + `export_guard` 2 |
| `brain-embed` | **65** | `embed_test` 37 + `url_redaction` 28 |
| `brain-core` | **10** | `lib.rs` |
| `brain-web` | **10** | `lib.rs` |
| **Rust — total** | **326** | `cargo test --workspace` → 0 failed |

```
.venv/bin/python -m pytest src/tests/ -q   →  124 passed, 2 failed  (124 = o total,
                                              os 2 são legados declarados, ver H1.4)
```

### H1.2 Portão Python é obrigatório, e é SEPARADO

`.venv/bin/python -m pytest src/tests/ -q` não é opcional nem "cobertaextra".
`pytest` coleta **126** testes (124 passam, 2 falham por H1.4) e nenhum deles roda em
`cargo test`. Foi exatamente essa conta que mostrou que a premissa "pytest não está
instalado neste ambiente" era **falsa** — e que ela vinha sendo usada para pulo
silencioso do portão (`workflow-state.json`, `TD-007`).

**As duas suítes são gates independentes. As duas têm de passar.**

### H1.3 Gatilhos

| Estado | Comando |
|--------|---------|
| Task concluída | `cargo test --workspace` |
| Bug corrigido | `cargo test --workspace` |
| **Pre-commit / PR** | `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `.venv/bin/python -m pytest src/tests/ -q` |
| Sub-fase SDD | os três acima + `cargo build --workspace` |
| Release / tag | os três acima + H3 |

`--all-targets` importa: sem ele o clippy não vê `tests/`, `examples/` nem `benches/`,
e o portão passa limpinho sobre código que nunca compilou.

### H1.4 As 2 falhas Python são CONHECIDAS e DECLARADAS

`test_integration_full_cycle` e `test_integration_errors` sobem o MCP **legado** por
stdio e morrem com `No module named brain_server`: o `.venv` roda o MCP em
`uv run`/3.11, sem o `src/` no path. É **ambiental, do código que a Fase C remove** —
não do produto.

- Esperado hoje: **exatamente** essas 2. Uma 3ª falha é regressão nova eBloqueia.
- Não "conserte" aplicando `skip`. O registro é que elas vão embora com a Fase C.
- As outras 124 são reais e discriminantes. `test_brain_hook_failure.py` roda 15.

### H1.5 Comandos canônicos
```bash
cargo test --workspace
cargo test -p brain-core -- --nocapture
cargo test -p brain-store -- --nocapture
cargo clippy --workspace --all-targets -- -D warnings
cargo llvm-cov --workspace --html          # ≥70%
.venv/bin/python -m pytest src/tests/ -q
```

### H1.6 Protocolo de falha

```
1. PARAR o dev.  2. Reportar crate:linha + msg.  3. Causa raiz.
4. Corrigir código ou teste.  5. Re-rodar a SUITE COMPLETA (as duas).
6. brain_store da lição.
```

## H2 Zero tolerância a falhas pré-existentes

### H2.1 Política
- Referência `cargo test --workspace` → **0 falhas** antes de QUALQUER novo dev.
- Débito pré-existente = bug P1 de saneamento. `#[ignore]` para passar suite é
  fraude, não correção.
- As 2 falhas Python de H1.4 são a **única** exceção, e estão nomeadas aqui.

### H2.2 Detecção antes de começar
```bash
cargo test --workspace && .venv/bin/python -m pytest src/tests/ -q
# 0 falhas Rust + só as 2 legadas → prosseguir.
# qualquer outra → .spec/tech-debts/{ctx}/TASKS.md, sanear antes da feature.
```

### H2.3 Um portão que alcança o modelo de produção NÃO é hermético

`env_remove("BRAIN_OLLAMA_URL")` **não desabilita o embed** — seleciona o default
real, `DEFAULT_BASE_URL = "http://localhost:11434"`
(`crates/brain-embed/src/lib.rs:23`), que é o modelo de produção, compartilhado por
todo binário de teste concorrente. Foi a causa raiz do flake de 3–5% do handshake MCP:
com embed real, o round trip foi de 0.141 s ocioso para **8.899 s** sob suite, contra
janela de 15 s (`crates/brain-cli/tests/mcp_sse_window.rs:29-33`).

**Regra:** em qualquer processo de teste, aponte `BRAIN_OLLAMA_URL` para um destino
**morto** (`http://127.0.0.1:1`) ou para um **mock local em porta efêmera**. Nunca
remova a variável. O guard é `no_fixture_falls_through_to_a_real_embedding_backend`
(`crates/brain-cli/tests/mcp_sse_window.rs:454`), que varre o fonte de `cli_e2e.rs`
e falha se `env_remove("BRAIN_OLLAMA_URL")` voltar.

## H3 E2E funcional — scratch, portas efêmeras, **nunca produção**

### H3.1 Por que scratch obrigatório
O corpus real (`data/brain.db`, 285 notas / 1027 chunks) tem servidor **no ar**
(PIDs 3934190/3934191). O H3 antigo mandava `rm -f ./data/brain.db` e bindar
8321/8322 — as duas coisas destroem produção. **H3 é scratch ou não é H3.**

### H3.2 Quando
Fim de sub-fase SDD · fim de bug · mudança de schema em `brain-store`/`web`/`cli`/`mcp` ·
antes de release/tag.

### H3.3 Protocolo

```bash
# 1. Scratch DB em diretório novo. NUNCA ./data/brain.db.
SCRATCH=$(mktemp -d)/brain.db
export BRAIN_EXPORT_ROOT="$SCRATCH/../export"   # allowlist própria, ver F3
export BRAIN_OLLAMA_URL="http://127.0.0.1:1"   # embedoffline proposital (H2.3)

cargo build --workspace

# 2. Portas efêmeras. 8321/8322 são de produção.
VP=$(python3 -c 'import socket;s=socket.socket();s.bind(("127.0.0.1",0));print(s.getsockname()[1]);s.close()')
MP=$(python3 -c 'import socket;s=socket.socket();s.bind(("127.0.0.1",0));print(s.getsockname()[1]);s.close()')

cargo run -p brain-cli -- --db "$SCRATCH" serve --port "$VP"      &
cargo run -p brain-cli -- --db "$SCRATCH" serve-mcp --port "$MP" &

# 3. Prontidão = ANÚNCIO pós-bind, não sleep e não poll cego.
#    MCP anuncia ":{port}/sse"; viewer anuncia "viewer http://0.0.0.0:{port}/".
#    As duas strings são distintas de propósito: o MCP serve em loopback e o viewer
#    em 0.0.0.0, e sem announcement um connect bem-sucedido não prova que é
#    *aquele* processo. (crates/brain-cli/tests/serve_viewer_shutdown.rs:19-20,154-179)
#    Regra dos três: anunciar ESTA porta + vivo + connect OK. Cada um elimina um
#    impostor diferente (crates/brain-cli/tests/cli_e2e.rs:455-470).

# 4. Smoke 12-step no CLI (sem rede; o 12b é o SSE real)
B="cargo run -p brain-cli -- --db $SCRATCH"
$B ping                                                     # 1  → pong
$B store regras h3/e2e "## E2E" --scope global              # 2
$B search "E2E" --top-k 5 --explain                          # 3  RRF 4 streams
$B read regras h3/e2e --scope global                          # 4
$B recent --top-k 5                                          # 5
$B export --to "$BRAIN_EXPORT_ROOT" --force                   # 6  allowlist (F3)
$B backup                                                    # 7  {db}.bak
$B store regras h3/ttl "## ttl" --scope global \
     --expires-at 2000-01-01T00:00:00Z                        # 8
$B forget-sweep --dry-run && $B forget-sweep                 # 9
$B status                                                    # 10 coverage + queue
$B checkpoints --limit 5                                     # 11
curl -sf "http://127.0.0.1:$VP/api/status"                    # 12 viewer 200
curl -sf "http://127.0.0.1:$VP/api/search?query=E2E&top_k=5"
curl -N  --max-time 2 "http://127.0.0.1:$MP/sse"              # 12b text/event-stream

# 5. Encerrar (H5.1)
```

### H3.4 O que cada passo tem que provar

| # | Passo | Critério |
|---|-------|----------|
| 1-2 | ping, store | `pong`; exit 0 |
| 3 | search | explain com 4 campos de stream; **FTS pontua mesmo com Ollama morto** (fallback) |
| 4-5 | read, recent | conteúdo idêntico ao escrito; nota aparece |
| 6 | export | `{BRAIN_EXPORT_ROOT}/regras/global/h3/e2e.md` existe; `--to /etc/x` **recusado** |
| 7 | backup | `.bak` criado; `--to /tmp/x.db` **recusado** |
| 8-9 | TTL + sweep | expirada some; **pinada não** |
| 10-11 | status, checkpoints | `coverage_pct` e `queue.*` presentes; ≥1 entrada |
| 12 | viewer | 200 + `results[]` |
| 12b | MCP SSE | stream que não termina |

## H4 Avaliação contínua — o que este sistema **realmente** tem

### H4.1 Search / rank
- [ ] RRF **k=60** (`1.0/(60.0+rank)`) sobre **4 streams** — vec, fts, entity, graph —
      somados em `brain-store/src/lib.rs:1819`
- [ ] `truncate(50)` por stream **antes** de fundir (`lib.rs:1717`); `truncate(top_k)`
      só no fim (`lib.rs:1827`)
- [ ] Authority: `arquitetura`/`regras` **+0.15**, `pinned` **+0.1**
      (`lib.rs:1822-1823`), somado **antes** do trunc final
- [ ] FTS escapa `"*():{}=-/`; filter e assembly são batch, **não** N+1
- [ ] `expires_at` vence `pinned`; filtro de scope antes do score
- [ ] Entidades: 243 linhas em produção — o stream **é** exercitado
- [ ] **Grafo: 0 linhas** em produção (ver `database-rules.md`) — o stream é
      testado e nunca foi exercitado

### H4.2 Store / schema
- [ ] `chunk_insert(None)` → `NULL`; **nunca** `vec![0.0; 768]`
      (`lib.rs:1207-1218`; guard em `tests/zero_vector_guard.rs`)
- [ ] Reindex é **upsert + prune**, nunca `DELETE FROM chunks` (`lib.rs:2007-2013`)
- [ ] `ReusePolicy` mora no campo do `Store` (`lib.rs:581`), não no call site
- [ ] Scope obrigatório para `arquitetura`/`regras`/`estudos` (`brain-core:15`)
- [ ] `sanitize_relative_path` barra `../` antes do DB
- [ ] `## ` split + dim 768 + `CHUNK_TARGET_TOKENS` 4096 (`brain-core:26`)

### H4.3 Fila de embedding (F1)
- [ ] `brain_store` devolve `queued=N` e **não** espera vetor (`brain-mcp/lib.rs:211`)
- [ ] `status` distingue "nada a fazer" / "aguardando backoff" / "outro embed rodando" /
      "desisti" — `pending_len`, `ready_len`, `is_draining`, `dead_lettered`
- [ ] `coverage_pct` distingue fila (`without_embedding`) de `zero_vector`
- [ ] `reindex` rejeita se outro embed segura o lock

### H4.4 MCP / viewer / disco
- [ ] `/api/status,search,read,list` 200 JSON
- [ ] `/sse` pong; `BRAIN_TRANSPORT=stdio` faz shutdown limpo
- [ ] `AppState { db: String, queue }` per-request — nenhum `&Store` cruza `.await`
- [ ] Export/backup **só** dentro de `BRAIN_EXPORT_ROOT` (F3)
- [ ] `BRAIN_OLLAMA_URL` sem credencial em erro, stderr e `Debug` (F4)
- [ ] `server start` arquiva `vault.bak.tar.gz` **antes** de importar (F5)
- [ ] Limites de escrita recusam **antes** de abrir `Store` (F2)

### H4.5 Onde o achado vai
P1 quebra → `.spec/bugs/{ctx}/TASKS.md` · P2 degrada → `.spec/tech-debts/{ctx}/TASKS.md`
· P3 cosmético → `.spec/roadmap/{ctx}/TASKS.md` · depois `brain_store` da lição.

## H5 Encerramento limpo

### H5.1 Portas do scratch
```bash
lsof -ti :$VP | xargs kill -9 2>/dev/null; lsof -ti :$MP | xargs kill -9 2>/dev/null
```
Portas efêmeras morrem com o processo. **Não** use `lsof -ti :8321` num harness:
essa é a de produção. Shutdown é SIGINT **e** SIGTERM — see F6.

## H6 Portão de conclusão — 8 itens

```markdown
## Pre-Completion Checklist
### Build & testes  (os DOIS portões)
- [ ] `cargo test --workspace` → 326 passed / 0 failed
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` → 0
- [ ] `.venv/bin/python -m pytest src/tests/ -q` → 124 passed, 2 legadas
- [ ] `cargo build --workspace` → 0 erros
### E2E funcional (H3, em scratch)
- [ ] 12 passos + 12b, todos OK
- [ ] viewer 200 em /api/status e /api/search; MCP /sse abre stream
- [ ] export para fora de `BRAIN_EXPORT_ROOT` **recusado**; backup idem
### Avaliação contínua (H4)
- [ ] checklist avaliado; P1→bugs, P2/P3→tech-debts/roadmap
### Code review
- [ ] reviewer invocado **depois** da última mudança
- [ ] zero 🔴; 🟡 corrigidos ou justificados por escrito
### Brain & workflow
- [ ] `brain_store` da lição, com `scope` correto
- [ ] `workflow-state.json` com `code_review_status: passed`, `harness_status: passed`
### Encerramento
- [ ] processos do scratch mortos; **produção 3934190/3934191 intacta**
- [ ] nenhum `restart`/`kill` em 8321/8322
### Teste que passa por acidente
- [ ] nenhum teste novo depende de ordem de execução, de `--nocapture`, ou de
      rede/Ollama real
```

**Qualquer ❌ = fase NÃO completa.**

> A última caixa existe porque a review encontrou um teste que só passava
> serializado. Um teste que depende de serialização está medindo estado global
> de outro teste — e falha em CI sem dizer por quê. O conserto é isolar a
> dependência, **não** rodar a suite com `--test-threads=1` e chamar de verde.

## H7 Cadência
Quem roda, e quando, já está nas tabelas: H1.3 (gatilhos) e H3.2 (quando o E2E é
obrigatório). O que sobra de calendário é o `brain_store` da lição — por achado,
2 min, por qualquer agente.

---

# F — Comportamentos que a onda entregou e que **não** tinham regra

> As seções H0-H7 são o portão. F1-F7 são o sistema. Um agente que leu só H0-H7
> passa no portão e ainda assim não sabe por que `queued>0` é bom, nem que
> `export --to /etc` é uma pergunta perigosa.

## F1 Fila de embedding

**O contrato:** uma escrita **não espera por vetor**.

1. `validate_note_write` (F2) → 2. `chunks_sync` grava com `embedding = NULL` e o
   FTS5 já está populado na **mesma** transação → 3. `store_note_and_queue`
   enfileira e responde → 4. worker em background embeda e escreve de volta.

Medido no caminho MCP real com Ollama no ar (`embed_queue.rs:13-15`): escrita
retorna em **0.018 s** para nota de 8 e de 64 chunks; os vetores chegam 0.6 s /
2.8 s depois. Antes, a escrita custava 0.4–0.6 s (8 chunks) a 2.8–3.9 s (64).

- **`queued=N` na resposta é o contrato** (`brain-mcp/src/lib.rs:211`). `embedded=0`
  numa nota nova **não** é bug. `coverage_pct` que não sobe depois de um
  `brain_store` é o sinal de fila travada.
- **A fila é um diff, não um rebuild.** `chunks_needing_embedding` (`lib.rs:1477`)
  devolve só o que não tem vetor válido, então regravar uma nota que cresceu custa
  1 embed, não N.
- **Ollama fora** → enfileira, loga, conta `nulls`. Nunca erro ao cliente.
- **`finish_owed` é o funil único** (`embed_queue.rs:895`). Todo caminho que termina
  com chunk devendo vetor passa por ele, então re-queue e dead-letter não podem
  divergir: abaixo de `BRAIN_EMBED_MAX_FAILURES` (default **8**, `embed_queue.rs:111`,
  ≈1 min de backoff cumulativo) reenfileira com backoff; **no cap, dead-letter** —
  sai da fila, `status` reporta `queue.dead_lettered`, e os chunks ficam `NULL`.
- **Dead-letter não é perda.** `NULL` é exatamente o registro que `recover`
  (`embed_queue.rs:476`, chamado por `boot_queue`, `brain-mcp/src/lib.rs:312`) lê no
  próximo boot, e o que `reindex --all` re-embera. Retry sem teto seria CPU eterna;
  sem retry o vetor sumiria em silêncio.
- **Lock cross-processo**: `_meta.embed_lock`, TTL **900 s**
  (`embed_queue.rs:72-73`), impede a fila do servidor e um `brain reindex` — que é
  um processo separado, feito para rodar contra servidor vivo — de embeder o mesmo
  chunk duas vezes. Recusar o lock por causa de reindex **não** conta como falha de
  embed: `attempts` (backoff) e `failures` (teto) são contadores separados.
- **CLI embeda inline**, não enfileira (`main.rs:653`): processo one-shot morre com a
  task de background e a nota ficaria sem vetor para sempre. Custo limitado por F2.

## F2 Limites de escrita

`MAX_CONTENT_BYTES = 256 KiB` (`brain-core:61`) e `MAX_CHUNKS = 64`
(`brain-core:71`), validados por `validate_content_limits` (`brain-core:158`), que é
**puro**: roda antes de abrir `Store` e antes de qualquer chamada de rede, nos **3
paths de escrita** — MCP rmcp, REST axum e CLI.

Dimensionados por `PESSIMISTIC_SECONDS_PER_CHUNK = 400 ms` (`brain-core:38`), ~9x o
~0.045 s/chunk medido — **não** pelo número mais rápido já observado, que é um
snapshot de uma máquina morna. Contra o corpus real: a maior nota tem 6.286 B, a
média 1.750 B (`brain-core:54-57`) — o teto é 41.7x a maior.

Sem teto, embedding serial (`OLLAMA_NUM_PARALLEL=1`) torna o custo da escrita
proporcional ao número de chunks. Os dois limites não se substituem: 200 KiB com
`## ` a cada 100 bytes são 2.000 chunks e passa pelo teto de bytes.

## F3 Allowlist de escrita em disco

`brain_export` e `brain_backup` só escrevem dentro de `BRAIN_EXPORT_ROOT`
(default `/tmp/brain-export`) — `fs_guard.rs:47,50`. Antes escreviam em path
arbitrário, sem auth, em `0.0.0.0`.

A defesa tem **duas** partes, e as duas importam:

1. **Contenção canônica.** `resolve_within` (`fs_guard.rs:296`) canonicaliza o
   ancestral **mais profundo que exista** do candidato e re-join o resto, depois
   testa contenção contra a raiz canonicalizada. O passo do ancestral existe porque
   `canonicalize` falha em path inexistente — e `/etc/passwd/novo` ainda é `/etc`.
   `..` e symlink não passam.
2. **Ownership** (`assert_root_usable`, `fs_guard.rs:199`): a raiz precisa ser
   diretório, do nosso `euid`, e não gravável por `other`. Sem isso, outro usuário
   local que tenha criado `/tmp/brain-export` **antes** do primeiro start lê o
   corpus e o `.bak` do banco.

O path de **cada nota** é rechecado na escrita
(`note_file_within`, `fs_guard.rs:383`), não só o diretório destino. O teste que
prova isso planta uma row com `path='../escape'` **por SQL** e roda o
`POST /mcp/export` de verdade (`tests/export_guard.rs`) — nenhuma API consegue
produzir uma, que é exatamente o ponto.

## F4 Redação de credencial

`BRAIN_OLLAMA_URL` aceita `http://user:pass@host`. A senha **não pode** aparecer em
log, stderr ou `Debug` — e o caminho perigoso é indireto: o `Display` do
`reqwest::Error` **concatena a URL do erro**, que num redirect vem do header
`Location` cru e não passa por `extract_authority`.

- **1 wrapper**: `redacted_error_message` (`brain-embed:266`), com **exatamente 1**
  chamada a `redact_reqwest_error` dentro dele. Faz duas coisas porque nenhuma é
  total: redaction **in place** (corrige o caso comum e preserva o
  `Kind`/status/cadeia de sources do reqwest) e substituição da **string
  renderizada** (para as formas que in-place não representa).
- **4 sinks**, todos por ele ou por `redact_url`:
  `client()` build `brain-embed:389` · `health_check` `:452` · `embed` `:496-497` ·
  `Debug` do `EmbedQueue` `embed_queue.rs:324`.
- **Backstop textual** `redact_url_textually` (`brain-embed:310`) para o que o parser
  não deu autoridade — e cobre o caso **sem `://`**, que é o que um operador produz
  quando tira o scheme na hora de colar na env var. `Url::parse("user:pass@host")`
  **tem sucesso** (scheme `user`, `has_authority()==false`), então o caminho do
  parser devolve a entrada byte-idêntica com a senha. Foi um hole em produção.
- **Fail-closed e nomeado**: over-redaction existe e é documentada
  (`mailto:someone@example.com` → `***@example.com`). Entrada **sem** credencial
  volta byte-idêntica — a propriedade que impede o redactor de 매일 com texto
  inútil.
- **Teste por ausência, não por forma.** Assert de string é o que deixou os holes
  passarem: a forma da mensagem não é o contrato, a **ausência da senha e a presença
  do marcador** são.

## F5 `server start` e o archive

`brain server start` importa o legado e **arquiva antes**: `vault.bak.tar.gz`
(`legacy_import.rs:110`) dentro de `BRAIN_EXPORT_ROOT`.

- O archive é tirado **antes** do import, e **se o archive falhar o import não roda**
  (`main.rs:856-858,864`). Um import que destrói a fonte sem backup não é reversível.
- Nome nunca é clobberado: `free_archive_path` (`legacy_import.rs:179`) reserva
  `vault.bak.N.tar.gz`. Usa `lstat`, não `exists()` — `exists()` segue symlink e
  devolveria falso num link quebrado, e o archive seria escrito **através** dele.
- Archive pela **mesma** função de `brain migrate` (`main.rs:939`): o caminho
  explícito e o automático não podem divergir.
- `serve` / `serve-mcp` **não** importam. Archive só com `migrate` ou `server start`.

## F6 Shutdown — SIGINT **e** SIGTERM

`shutdown_on_sigint_or_sigterm` (`rmcp_service.rs:470`) corre nos dois.
`systemctl stop` manda **SIGTERM**; um `ctrl_c` de terminal é SIGINT. Tratar só um
dos dois é um bug invisível no terminal e visível na unit que o `brain setup systemd`
instala — foi assim que `serve-mcp` morreu por sinal sem nunca alcançar o
`ct.cancel()`.

`server start` arma o handler **antes do import** (`main.rs:855`, import em `:864`):
o import é síncrono e pode levar minutos, e um sinal que chega durante ele tem que
ser *registrado*, não matar o processo.

> **O meio-caminho é pior que não tratar.** Uma versão anterior do ramo stdio
> armava SIGTERM e nunca fazia `poll` nele (`main.rs:880-888`): a disposição passa a
> ser a do tokio, `systemctl stop` é registrado e **ignorado**, e o processo segue
> servindo até `TimeoutStopSec` escalar para `SIGKILL`. Handler armado sem
> aguardado = `systemctl stop` que parece funcionar e não para nada. Se um ramo não
> pode agir sobre o sinal, ele **não arma** o handler.

## F7 Sincronização de números entre linguagens

O legado Python (`src/brain_server/`) declara constantes **independentes** das do
Rust. Hoje elas concordam — e **nenhum teste garante que continuem**:

| Constante | Rust | Python |
|-----------|------|--------|
| `EMBEDDING_DIM` | 768 `brain-core:18` | 768 `index/store.py:59` |
| `CHUNK_TARGET_TOKENS` | 4096 `brain-core:26` | 4096 `embeddings/engine.py:94` |
| timeout de embed | 30 s `brain-embed:29` | 30 s `engine.py:24` |
| timeout de health | 5 s `brain-embed:66` | 5 s `engine.py:33` |
| concorrência de batch | 4 `brain-embed:72` | 4 `engine.py:78` |

Verificado: nenhum teste em qualquer das duas suítes lê o fonte da outra
linguagem. O único `768` no lado Python é o literal de um **mock**
(`test_integration.py:29`), não uma asserção de paridade.

**Regra:** mude as duas no **mesmo commit**, e `grep` as duas. Não existe guard.
E **não afirme** que elas já divergiram: uma revisão anterior levantou essa
hipótese e ela foi **retirada** por não se sustentar
(`workflow-state.json`, `reviewer_discoveries[3]`). Se você não consegue apontar
arquivo, não escreva.

## H8 A regra sobre este arquivo

Uma afirmação técnica aqui que não aponte `arquivo:linha` é um bug com TTL infinito.
Este arquivo já afirmava `16 tests` (H1), `rmcp 0.3` (A2) e mandava apagar o banco
de produção (H3). **Regra:** antes de escrever um número, um nome de campo, um
comportamento ou um comando, `grep` e cole a referência. Se o grep não confirma,
apague a afirmação — não o hedge. Um `grep` que volta vazio é o sinal de que a
ideia é ficção, e a resposta certa é não escrever.
