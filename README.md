# brain 🧠

**Servidor MCP de memória central para agentes de IA — Rust + SQLite-only.**

Persiste conhecimento em `data/brain.db` (WAL, FTS5 porter, vetor BLOB 768), indexa via embeddings locais (Ollama `nomic-embed-text`, com fallback FTS) e expõe busca híbrida (FTS5 + vetor + entidade + grafo, RRF k=60) via CLI, MCP SSE e viewer HTTP.

> Legado Python em `src/brain_server/` mantido só para compatibilidade até a Fase C. Fonte da verdade é o workspace Rust.

---

## Os dois públicos deste README

Este arquivo serve a dois leitores com perguntas opostas. Escolha o seu e pule
para a seção.

| Você quer… | Vá para | Onde está a verdade |
|-----------|---------|---------------------|
| **USAR** o brain no seu projeto (`atlas-ecm`, `hive`, `progat-erp`) | [Quick start](#quick-start) · [Consumir de outro projeto](#consumir-de-outro-projeto) | `.agents/skills/brain/SKILL.md` (17 tools) e `.agents/rules/BRAIN.MCP.md` (regras) |
| **DESENVOLVER** o brain | [Estrutura do repo](#estrutura-do-repo) · [Testes / harness](#testes--harness) | [`AGENTS.md`](AGENTS.md) (stack e comandos) e `.agents/rules/` (as regras, uma por assunto) |

**Toda a documentação de desenvolvimento está em `.agents/`, não aqui.** Este
README cobre instalar, rodar e operar. Ele não documenta a arquitetura interna,
o harness, os limites de escrita ou o contrato das tools — e quando precisa
fazê-lo, cita o arquivo em vez de repetir, para não haver duas versões do mesmo
fato.

Para um agente de outro projeto, o caminho é o bloco
[Consumir de outro projeto](#consumir-de-outro-projeto): dois arquivos de
instrução e o servidor SSE.

---

## Índice

**Para usar:**

- [Quick start](#quick-start)
- [Consumir de outro projeto](#consumir-de-outro-projeto)
- [Cadastrar projeto](#cadastrar-projeto)
- [Iniciar servidores](#iniciar-servidores)
- [Configuração](#configuração)
- [Viewer API](#viewer-api)
- [Sessões compartilhadas (B2 MVP)](#sessões-compartilhadas-b2-mvp)
- [Hooks lifecycle](#hooks-lifecycle)

**Para desenvolver:**

- [Visão geral (Rust)](#visão-geral-rust)
- [Pré-requisitos](#pré-requisitos)
- [Instalação](#instalação)
- [Estrutura do repo](#estrutura-do-repo)
- [Testes / harness](#testes--harness)
- [Troubleshooting](#troubleshooting)

---

## Visão geral (Rust)

| Camada | Tecnologia |
|--------|-----------|
| Server | Rust 1.85 + axum + rmcp (MCP SDK) |
| Storage | SQLite-only `data/brain.db` (WAL, FTS5 porter, vec BLOB cosine, dim 768 fixa) |
| Embeddings | Ollama local (`nomic-embed-text`), fallback FTS se down |
| Transport | SSE (`8321` MCP) + viewer HTTP (`8322`) + stdio fallback (`BRAIN_TRANSPORT=stdio`) |
| Busca | Híbrida FTS5 + vetor + entidade + grafo, fusão RRF k=60 + boost autoridade |
| Testes | `cargo test --workspace` + `cargo clippy` |

Crates em `crates/`: `brain-core` (tipos, validate/sanitize, chunk `##`, wikilink), `brain-store` (SQLite WAL, schema, RRF, TTL, audit), `brain-embed` (cliente Ollama), `brain-mcp` (registro das 17 tools em `rmcp_service.rs` + SSE), `brain-web` (viewer `/api/*`), `brain-cli` (CLI `brain`).

Versão atual: `0.9.1` (ver `workflow-state.json`).

---

## Pré-requisitos

- **Rust 1.85** (edition 2024) + `cargo`
- **Ollama opcional** — sem Ollama o brain continua funcionando em modo FTS-only:
  ```bash
  curl -fsSL https://ollama.com/install.sh | sh
  ollama pull nomic-embed-text
  ```
- Linux (testado), SQLite via `rusqlite` (sem servidor externo, sem GPU).

---

## Instalação

```bash
git clone <repo> brain
cd brain

# Build completo do workspace (fonte da verdade)
cargo build --workspace

# Binário gerado
./target/debug/brain --help
./target/debug/brain status

# Opcional: instalar no PATH
cargo install --path crates/brain-cli
brain --help
```

> A partir daqui, `brain` significa `./target/debug/brain` ou o binário instalado. Todos os exemplos usam `--db` explícito quando necessário; o default já é `./data/brain.db`.

---

## Configuração

Todas as variáveis usam prefixo `BRAIN_`. Esta é a lista do **binário Rust** — as
que o servidor e a CLI leem de fato:

| Variável | Default | Descrição |
|----------|---------|-----------|
| `BRAIN_DB_PATH` | `./data/brain.db` | SQLite-only DB (WAL + FTS5) |
| `BRAIN_OLLAMA_URL` | `http://localhost:11434` | URL do Ollama. Com ele fora, a busca degrada para FTS-only — **não dá erro** |
| `BRAIN_OLLAMA_MODEL` | `nomic-embed-text` | Modelo de embedding (dim 768 fixa) |
| `BRAIN_PORT` | `8321` | Porta MCP SSE (`serve-mcp`, `server start`) |
| `BRAIN_TRANSPORT` | — | `stdio` em vez de SSE |
| `BRAIN_EXPORT_ROOT` | `/tmp/brain-export` | **Allowlist de escrita** de `brain export` e `brain backup` |
| `BRAIN_EMBED_MAX_FAILURES` | `8` | Tentativas de embed antes do dead-letter |
| `BRAIN_REUSE_SIMILARITY` | `1.0` | Abaixo de 1.0 um chunk pode herdar o vetor do texto antigo |
| `BRAIN_EMBED_TIMEOUT_SECS` | `60` | Budget base por onda de embed |
| `BRAIN_HOOK_EMBED` | `1` | `0` desliga o embed no `brain hook` |

⚠️ **`BRAIN_VIEWER_PORT` e `BRAIN_LOG_LEVEL` não são lidas pelo binário Rust**,
apesar de aparecerem no `CONFIGURE_MCP.md` e no `.env.example`: elas são lidas
só pelo Python legado (`viewer/server.py`). No Rust a porta do viewer é a flag
`--port` (default 8322, `brain-cli:98`) e não há nível de log configurável — o
binário Rust não tem logger. A porta do MCP **é** `BRAIN_PORT`.

`BRAIN_VAULT_PATH` foi **removida** do modelo e não é lida por nada; sobrou só o
aviso no legado, que é o que ainda informa o operador de que a variável parou de
fazer alguma coisa.

Flag global (sobrescreve env por comando):

```bash
brain --db ./data/brain.db status
brain --db /tmp/test.db status
BRAIN_DB_PATH=/tmp/test.db brain status
```

Scope é **obrigatório** para `arquitetura`, `regras` e `estudos` (`projetos` ou `global`). `sessoes` e `projetos` não usam scope.

---

## Quick start

```bash
# 0. Saúde do banco
brain status
# → notes=0 chunks=0 projects=0 db=./data/brain.db

# 1. Salvar nota (scope obrigatório para regras)
brain store regras meu-projeto/naming "## Regra de naming" --scope projetos
brain store regras coding-standards "## Padrões universais" --scope global

# 2. Buscar (híbrida FTS5+vetor RRF)
brain search "naming" --layer regras --scope projetos
brain search "padrões" --layer regras --scope global --top-k 5
brain search "naming" --explain --top-k 5

# 3. Ler nota
brain read regras meu-projeto/naming --scope projetos

# 4. Atalhos úteis
brain recent --top-k 10
brain checkpoints --limit 5
```

Sessão sem scope (camada `sessoes`):

```bash
brain store sessoes meu-app/2026-09-17 "## Sessão: quick start OK"
brain read sessoes meu-app/2026-09-17
```

> **Uma escrita não espera por vetor.** `brain store` grava a nota e popula o
> FTS5 na mesma transação e responde; os embeddings chegam em background. No
> CLI eles são embedded inline, então a chamada já volta indexada — mas via MCP o
> `brain_store` devolve `{"embedded": 0, "queued": N}` e `embedded: 0` **não é
> bug**. Para o índice semântico, veja `brain status` →
> `embedding.coverage.embedding_coverage_pct`. Detalhes em
> `.agents/rules/BRAIN.MCP.md`.

---

## Cadastrar projeto

Projetos vivem na tabela `projects`. Notas podem ser vinculadas via `note_projects` (owned via `store --project` ou ligadas via `project link`).

```bash
# Criar projetos
brain project create erp --description "ERP principal"
brain project create mobile --description "App mobile"

# Listar
brain project list

# Salvar nota já vinculada ao projeto (cria o projeto se não existir)
brain store regras erp/pedidos "## Pedidos: status inicial rascunho" --scope projetos --project erp
brain store regras mobile/sync "## Sync offline-first" --scope projetos --project mobile

# Buscar filtrando por projeto
brain search "pedidos" --project erp
brain search "sync" --project mobile --top-k 5

# Listar notas de um projeto (owned + linked)
brain project notes erp

# Vincular / desvincular nota global existente a um projeto
brain store regras shared/pagamento "## Pagamento via Pix" --scope global
brain project link regras/global/shared/pagamento erp
brain project link regras/global/shared/pagamento mobile
brain project notes erp
brain project unlink regras/global/shared/pagamento mobile

# Remover projeto (não apaga as notas)
brain project delete mobile
```

> Caminho completo (`full path`) segue `layer/scope/path` para camadas com scope (ex: `regras/global/shared/pagamento`) e `layer/path` para as demais. É esse path que `delete` e `project link` esperam.

Comandos de manutenção relacionados:

```bash
brain delete regras/global/shared/pagamento
brain backup                              # -> {db}.bak, o default documentado
brain backup --to /tmp/brain-export/brain.bak   # precisa estar DENTRO da export root
brain export --to /tmp/brain-export --force

# Auto-import do legado + archive antes de servir (o nome da spec, US-01.1)
brain server start --port 8321
# arquiva ./vault em /tmp/brain-export/vault.bak.tar.gz e só então importa.
# Nunca clobbera: a 2a vez vira vault.bak.2.tar.gz. Se o archive falhar,
# o import não roda — backup best-effort que falhou em silêncio deixa o
# operador acreditando numa rede de segurança que não existe.
brain forget-sweep --dry-run
```

> **`brain backup --to` só aceita destinos dentro da export root.** Um caminho
> arbitrário é copia-do-banco-para-qualquer-lugar, então o destino é restringido a
> `BRAIN_EXPORT_ROOT` (default `/tmp/brain-export`), tem que ser absoluto e tem que
> terminar em `.bak`. `brain backup` sem `--to` grava `{db}.bak`, que é sempre
> permitido. Para gravar em outro lugar, aponte `BRAIN_EXPORT_ROOT` para lá — e note
> que a raiz precisa ser um diretório seu, não gravável por qualquer usuário local
> (o servidor recusa caso contrário).

---

## Sessões compartilhadas (B2 MVP)

**Problema:** duas frentes (ex: `erp` + `mobile`) precisam ler/escrever a mesma sessão sem duplicar conteúdo.

**Convenção (SH-02):** salvar em `sessoes/shared/<data-tema>` e vincular aos dois projetos. O hook com `--project shared` faz o auto-link para `erp` + `mobile` via `note_projects`. Leitura = 2x `search` (uma por projeto) + merge.

Passo a passo copy-paste (ERP + mobile):

```bash
# 1. Garantir projetos
brain project create erp --description "ERP principal"
brain project create mobile --description "App mobile"

# 2. Criar sessão compartilhada (sem scope — sessoes não usa scope)
brain store sessoes shared/2026-09-17-pagamento-pix "## Sessão shared: pagamento Pix

Decisão: Pix com idempotency-key.
ERP cria cobrança, mobile exibe QR."

# 3. Vincular aos dois projetos (ou usar o hook com --project shared, que faz isso sozinho)
brain project link sessoes/shared/2026-09-17-pagamento-pix erp
brain project link sessoes/shared/2026-09-17-pagamento-pix mobile

# 4. Confirmar vínculo
brain project notes erp
brain project notes mobile

# 5. Leitura por cada frente (merge no agente)
brain search "pagamento Pix" --project erp --top-k 5
brain search "pagamento Pix" --project mobile --top-k 5

# 6. Via hook (auto-link erp+mobile quando path é sessoes/shared/*)
brain hook --event session-start --project shared --payload '{"id":"pix-001"}'
brain project notes erp
```

Regras do MVP:

- Só o prefixo `sessoes/shared/` dispara auto-link. `sessoes/erp/...` ou `sessoes/mobile/...` continuam isolados.
- Projetos-alvo do auto-link hoje: `erp` + `mobile` (criados sob demanda se faltarem).
- `search --project` filtra por vínculo; sem filtro, a busca híbrida já ranqueia o shared para ambos.

---

## Iniciar servidores

MCP SSE (porta `8321`) e viewer (porta `8322`) coexistem:

```bash
# MCP SSE (protocolo rmcp, 17 tools) — default 8321
brain serve-mcp --port 8321

# Viewer read-only — default 8322
brain serve --port 8322

# Com DB customizado
brain --db ./data/brain.db serve-mcp --port 8321
brain --db ./data/brain.db serve --port 8322

# Checagem rápida
curl http://localhost:8322/api/status
curl http://localhost:8321/sse | head -n 5
```

Auto-start via systemd (one-shot installer):

```bash
brain setup all
brain setup opencode
brain setup systemd
```

`brain setup` alvos: `all|opencode|kiro|systemd|shell|project` (ver `brain setup --help`).

`opencode` e `kiro` instalam o **hook de sessao** que liga a IDE ao brain: o
opencode recebe um plugin em `~/.config/opencode/plugins/brain-session.js` e o kiro
recebe `.kiro/hooks/brain-session.json`. `kiro` grava **dentro do projeto**; o resto
configura a maquina.

`all` **pergunta qual IDE usar** (`opencode` | `kiro`) e instala exatamente uma, mais
`systemd` e `shell`. Em terminal nao-interativo assume `opencode` e avisa. `kiro` nao
entra no `all` por padrao: escreveria artefatos de uma segunda IDE no diretorio em que
o comando foi rodado.

O `setup` tambem registra a **decisao de projeto** do diretorio atual em
`config.json` (`BRAIN_DIR`, fallback `~/.brain`) — e essa decisao e o que faz
`brain hook` saber em qual projeto gravar sem ninguem passar `--project`:

```bash
brain setup opencode --project hive     # grava, sem perguntar
brain setup opencode --decline-project # grava "nao usar brain aqui"
```

| Flag | Efeito |
|------|--------|
| `--project <nome>` | grava o projeto do diretorio atual, sem perguntar |
| `--decline-project` | grava a recusa (`motivo: "recusado"`), sem perguntar |
| `--yes` | nao interativo: assume defaults, nao pergunta, nao grava projeto |
| `--dry-run` | pergunta e mostra o que gravar, sem gravar nada |
| `--force` | regrava artefatos que ja existem (o padrao e nao tocar) |

`BRAIN_SETUP_NONINTERACTIVE=1` e o equivalente de `--yes` para quem nao pode passar
flag. `systemd` e `shell` nao aceitam `--project`/`--decline-project` e dizem isso.

Encerramento limpo:

```bash
lsof -ti :8321 | xargs kill -9 2>/dev/null
lsof -ti :8322 | xargs kill -9 2>/dev/null
lsof -i :8321 || echo "8321 free"
lsof -i :8322 || echo "8322 free"
```

---

## Consumir de outro projeto

### 1. MCP server (SSE)

No `opencode.json` do projeto consumidor:

```json
{
  "mcpServers": {
    "brain": {
      "transport": "sse",
      "url": "http://localhost:8321/sse"
    }
  }
}
```

### 2. Skill + regras (instruções para o agente)

```json
{
  "instructions": [
    "../brain/.agents/skills/brain/SKILL.md",
    "../brain/.agents/rules/BRAIN.MCP.md"
  ],
  "mcpServers": {
    "brain": {
      "transport": "sse",
      "url": "http://localhost:8321/sse"
    }
  }
}
```

- `SKILL.md` — as 17 tools, o contrato assíncrono do `brain_store`, e
  `brain_status` como sinal de fila travada.
- `BRAIN.MCP.md` — regras completas (camadas, scope, fluxo tarefa → busca →
  implementa → registra → resumo). É o **mesmo contrato** que o `SKILL.md`: se
  divergirem, é bug.
- **Uma skill por tool** em `.agents/skills/brain/tools/` — 17 diretórios, um por
  tool MCP, cada um com parâmetros, formato do retorno e "o que NÃO fazer". As 5
  com efeito destrutivo ou escrita em disco (`brain_export`, `brain_backup`,
  `brain_forget_sweep`, `brain_restore`, `brain_delete`) têm essa seção
  obrigatória.
- Subcomandos de **CLI** ficam em `.agents/skills/brain/cli/` (hoje
  `brain_reindex`) — não são tools MCP e não estão no registro.

Fluxo recomendado por tarefa: `brain_search` (contexto) → implementa → `brain_store` (decisões em `arquitetura`/`regras` com scope correto) → `brain_store` em `sessoes` (resumo).

---

## Viewer API

Viewer é read-only sobre o mesmo `brain.db` (axum, `brain-web`):

```bash
curl http://localhost:8322/api/status | jq .
curl "http://localhost:8322/api/search?query=pagamento&top_k=5" | jq .results
curl "http://localhost:8322/api/read?path=sessoes/shared/2026-09-17-pagamento-pix" | jq .content
curl http://localhost:8322/api/list | jq .
```

| Endpoint | Descrição |
|----------|-----------|
| `GET /api/status` | `{notes, chunks, projects, embedding:{coverage:{…}}}` |
| `GET /api/list` | `{entries:[{path, layer, scope, chunks_total, chunks_embedded}]}` |
| `GET /api/search?query=&top_k=&layer=&scope=` | `{results:[{path, layer, scope, score, chunk_index, snippet}]}` |
| `GET /api/read?path=` | Lê nota por full path (`notes.path`, exato) |

`chunks_total`/`chunks_embedded` em `/api/list` usam a **mesma** definição de
"embeddado" que `embedding.coverage` em `/api/status` (presente, largura 3072 B e
não `zeroblob`). Três estados, derivados pelo viewer:

| Estado | Significado | Badge |
|--------|-------------|-------|
| `chunks_embedded >= 1` | alcançável pela busca semântica | 📝 |
| `chunks_embedded == 0`, `chunks_total > 0` | FTS-only, fila de embed atrasada | ⚠️ FTS only |
| `chunks_total == 0` | nota sem chunks (sync não rodou / corpo vazio) | ⚠️ No chunks |

`/api/list` faz **2 queries** (paths sem corpo + um `GROUP BY path` sobre
`chunks`) e **não** serve `content` — a listagem nunca renderiza o corpo, e
selecioná-lo custava ~516 KiB descartados no corpus de produção.

---

## Hooks lifecycle

Hook Rust com spool em `$XDG_RUNTIME_DIR/brain/hook-spool.jsonl` (file lock + dedup por `id`):

```bash
brain hook --event session-start --project erp --payload '{"id":"s1"}'
brain hook --event tool-result --project erp --payload '{"id":"t1","tool":"search"}'
brain hook --event session-end --project erp --payload '{"id":"s1-end"}'

# Sessão compartilhada (auto-link erp+mobile)
brain hook --event session-start --project shared --payload '{"id":"pix-001"}'
```

Comportamento:

- `session-start` — anexa seção em `sessoes/<project>/<data>.md` e imprime contexto (`regras/global` + `regras/projetos`) para injeção.
- `tool-result` — anexa JSON do resultado na sessão do dia.
- `session-end` — fecha a sessão do dia.
- `sessoes/shared/*` + `--project shared` — auto-link para `erp` + `mobile`.
- Repetir o mesmo `payload.id` é deduplicado (`hook deduplicated`).

---

## Testes / harness

```bash
cargo test --workspace
cargo test -p brain-core -- --nocapture
cargo test -p brain-store -- --nocapture
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings   # --all-targets: sem ele o clippy não vê tests/
cargo llvm-cov --workspace --html   # cobertura ≥70%
```

E2E 12-step (resumo — ver `.agents/rules/harness-continuous.md` H3 para o protocolo completo):

```bash
rm -f /tmp/phasec.db && mkdir -p data
cargo build --workspace
cargo run -p brain-cli -- --db /tmp/phasec.db ping
cargo run -p brain-cli -- --db /tmp/phasec.db store regras phasec/e2e "## E2E test" --scope global
cargo run -p brain-cli -- --db /tmp/phasec.db search "E2E" --top-k 5 --explain
cargo run -p brain-cli -- --db /tmp/phasec.db read regras phasec/e2e --scope global
cargo run -p brain-cli -- --db /tmp/phasec.db recent --top-k 5
cargo run -p brain-cli -- --db /tmp/phasec.db export --to /tmp/brain-export --force
# backup --to tem que ser ABSOLUTO, terminar em .bak e estar DENTRO da export root
cargo run -p brain-cli -- --db /tmp/phasec.db backup --to /tmp/brain-export/phasec.bak
cargo run -p brain-cli -- --db /tmp/phasec.db store regras phasec/ttl "## ttl" --scope global --expires-at 2000-01-01T00:00:00Z
cargo run -p brain-cli -- --db /tmp/phasec.db forget-sweep --dry-run
cargo run -p brain-cli -- --db /tmp/phasec.db status
cargo run -p brain-cli -- --db /tmp/phasec.db checkpoints --limit 5
```

Portão de conclusão (resumo): `cargo test` 0 falhas + `cargo build` 0 erros + `clippy` 0 warnings + E2E 12-step OK + viewer `/api/status` 200 + MCP `/sse` pong + `workflow-state.json` atualizado.

---

## Troubleshooting

| Sintoma | Causa provável | Fix |
|---------|---------------|-----|
| `embedding failed (FTS only)` | Ollama down | Normal — busca segue via FTS. Suba com `ollama serve && ollama pull nomic-embed-text` |
| `database is locked` | Dois writers / WAL travado | Um `Store` por processo; feche CLI/servidor duplicado, `lsof data/brain.db` |
| `scope required for arquitetura/regras/estudos` | Faltou `--scope` | Adicione `--scope projetos` ou `--scope global` |
| `NOT_FOUND: <path>` | Full path errado | Confira `layer/scope/path` (ex: `regras/global/shared/pagamento`) |
| `export dir exists, use --force` | Export repetido | `brain export --to /tmp/brain-export --force` |
| `REINDEX_IN_PROGRESS` | Reindex concorrente | Aguarde; lock em memória por processo |
| Porta `8321`/`8322` ocupada | Servidor órfão | `lsof -ti :8321 \| xargs kill -9`, idem `8322` |
| `projects=0` em `status` | Banco zerado/bootstrap | `brain project create erp` + `brain project create mobile`, depois `project list` |
| `hook deduplicated` | `payload.id` repetido | Esperado — use `id` novo por evento |

Diagnóstico rápido:

```bash
brain status                          # contagens + coverage + fila
brain checkpoints --limit 5
curl http://localhost:8322/api/status | jq .
lsof -i :8321; lsof -i :8322
brain search "teste" --explain       # o --explain é do CLI; mostra os 4 streams do RRF
```

> O binário Rust **não** tem nível de log por env var: `BRAIN_LOG_LEVEL` é lida
> só pelo Python legado. Para o comportamento do embed, use os números de
> `brain status` (`embedding.coverage` e `queue`), que são a superfície de
> diagnóstico do Rust.

---

## Estrutura do repo

```
crates/
├── brain-core/src/lib.rs   → types (Note/Project/SearchResult), validate/sanitize, chunk ##, wikilink
├── brain-store/src/lib.rs  → Store SQLite WAL, FTS5 porter, vec BLOB cosine, RRF k=60, TTL, audit
├── brain-embed/src/lib.rs  → Ollama client (nomic-embed-text dim 768)
├── brain-mcp/src/rmcp_service.rs → REGISTRO das 17 tools MCP + transporte SSE (8321)
├── brain-mcp/src/lib.rs    → store_note_and_queue, fila de embedding, fs_guard, rotas REST de debug
├── brain-web/src/lib.rs    → axum viewer /api/* (8322)
└── brain-cli/src/main.rs   → clap (ping/store/read/search/delete/r…/hook/setup/project/server)
src/brain_server/           → legado Python (compat Fase C)
data/brain.db               → SQLite-only (WAL) — verdade única
viewer/index.html           → viewer estático
.agents/
├── skills/brain/SKILL.md   → skill /brain para outros repos: as 17 tools, o contrato
│                             assíncrono e as env vars
├── skills/brain/tools/     → 1 skill por tool MCP (17), com "o que NÃO fazer" nas destrutivas
├── skills/brain/cli/       → subcomandos de CLI (brain_reindex) — não são tools MCP
├── rules/BRAIN.MCP.md      → mesmo contrato, para o agente consumidor
└── rules/                  → regras de desenvolvimento, uma por assunto (harness, arquitetura,
                              banco, backend, frontend, segurança, SDD, workflow)
hooks/                      → hooks lifecycle + steering/instructions
.spec/                      → SPEC/PLAN/TASKS por feature
workflow-state.json         → estado SDD (versão, fase, testes)
AGENTS.md                   → fonte da verdade (stack, comandos, tools MCP)
```

### Onde está a documentação

| Pergunta | Arquivo |
|----------|---------|
| Como uso isso no meu projeto? | `.agents/rules/BRAIN.MCP.md` |
| O que cada tool faz e devolve? | `.agents/skills/brain/tools/<tool>/SKILL.md` |
| Quero que meu agente USE o brain | `.agents/skills/brain/SKILL.md` |
| Como o brain é construído e testado? | `AGENTS.md` + `.agents/rules/` |
| O que o portão de conclusão exige? | `.agents/rules/harness-continuous.md` |
| Referência de comandos | `AGENTS.md` |

> Projeto desenvolvido com **spec-driven-development** — ver `.spec/` para requirements, design e tasks.
