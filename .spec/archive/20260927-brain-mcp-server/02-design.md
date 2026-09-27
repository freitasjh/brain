# Phase 2 — Design

## Brain MCP Server — Cérebro para Agentes de IA

---

## Overview

Servidor MCP em Python puro (sem framework web). Usa o pacote oficial `mcp` para implementar o protocolo MCP. Toda persistência é em sistema de arquivos: vault Obsidian para notas markdown, arquivo JSON para índice de embeddings. Ollama local para geração de embeddings.

Não há banco de dados relacional. Não há cache externo. Não há dependência cloud.

---

## Architecture

### Diagrama de Componentes

```
┌─────────────────────────────────────────────────────────────────────┐
│                        brain-server (MCP Server)                     │
│                                                                     │
│  ┌──────────────┐   ┌──────────────┐   ┌────────────────────────┐  │
│  │  mcp          │   │  config      │   │  embeddings/engine     │  │
│  │  server.py    │──▶│  config.py   │   │  (Ollama client)       │  │
│  │  (protocol)   │   └──────────────┘   └───────────┬────────────┘  │
│  └──────┬───────┘                                    │              │
│         │                                            │              │
│  ┌──────▼────────────────────────────────────────────▼──────────┐   │
│  │                      tools/                                    │   │
│  │  ┌──────────┐ ┌──────────┐ ┌───────────┐ ┌──────────────┐    │   │
│  │  │ store    │ │ read     │ │ search    │ │ reindex      │    │   │
│  │  └────┬─────┘ └────┬─────┘ └─────┬─────┘ └──────┬───────┘    │   │
│  └───────┼────────────┼─────────────┼───────────────┼────────────┘   │
│          │            │             │               │                │
│  ┌───────▼────────────▼─────────────▼───────────────▼────────────┐   │
│  │                     vault/manager.py                           │   │
│  │  (CRUD em arquivos .md, validação de layers, estrutura dir)   │   │
│  └────────────────────────────┬──────────────────────────────────┘   │
│                               │                                      │
│  ┌────────────────────────────▼──────────────────────────────────┐   │
│  │                     index/store.py                              │   │
│  │  (índice de embeddings: JSON persistido, busca cosine sim)     │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
                               │
          ┌────────────────────┼────────────────────┐
          ▼                    ▼                    ▼
   ┌──────────┐        ┌──────────────┐     ┌──────────┐
   │ Obsidian │        │ Ollama       │     │ Disco    │
   │ Vault    │        │ (embeddings) │     │ (index)  │
   │ .md      │        │ :11434       │     │ .json    │
   └──────────┘        └──────────────┘     └──────────┘
```

### Fluxo de Dados

**brain_search(query, layer?, top_k=5):**
```
query ──▶ engine.embed(query) ──▶ vetor q (768d)
                                      │
                                      ▼
                              index.store.cosine_similarity(q, all_vectors)
                                      │
                                      ▼
                              top_k resultados (path, score, snippet)
                                      │
                                      ▼
                              retorna para MCP
```

**brain_store(layer, path, content):**
```
content ──▶ vault.write(layer/path.md, content)
     │
     ├─▶ engine.embed(content) ──▶ vetor
     │       │
     │       ▼
     │   index.upsert(path, vetor, metadata)
     │       │
     │       ▼
     │   index.persist() (salva JSON)
     │
     └─▶ returna sucesso
```

**brain_reindex(layer?, path?):**
```
scan vault files ──▶ for each file:
                        engine.embed(content) ──▶ vetor
                        index.upsert(path, vetor, metadata)
                    index.persist()
```

---

## Estrutura de Diretórios

```
brain/
├── AGENTS.md                    ← (existe)
├── opencode.json                ← (existe)
├── workflow-state.json          ← (existe)
├── .agents/                     ← (existe, skills/rules/agent)
│   ├── rules/
│   │   ├── workflow-rules.md    ← usar apenas processo, ignorar stack
│   │   ├── architecture-rules.md
│   │   ├── backend-rules.md
│   │   ├── frontend-rules.md
│   │   └── database-rules.md
│   ├── skills/
│   │   ├── brain/               ← /brain skill (a criar)
│   │   │   ├── SKILL.md
│   │   │   └── assets/
│   │   └── ... (skills herdadas)
│   └── agent/
│       ├── developer-engineer/
│       ├── fullstack-code-reviewer/
│       └── qa-engineer/
│
├── .spec/
│   └── brain-mcp-server/        ← (spec atual)
│       ├── 01-requirements.md
│       ├── 02-design.md
│       └── 03-tasks.md          ← (a criar)
│
├── src/
│   ├── brain_server/
│   │   ├── __init__.py
│   │   ├── __main__.py          ← entrypoint: `python -m brain_server`
│   │   ├── config.py            ← pydantic-settings ou dataclass
│   │   ├── server.py            ← MCP server setup, tool registration
│   │   │
│   │   ├── vault/
│   │   │   ├── __init__.py
│   │   │   ├── manager.py       ← CRUD operações no vault
│   │   │   └── models.py        ← Note, Layer, VaultEntry
│   │   │
│   │   ├── embeddings/
│   │   │   ├── __init__.py
│   │   │   └── engine.py        ← Ollama client, embed function
│   │   │
│   │   ├── index/
│   │   │   ├── __init__.py
│   │   │   ├── store.py         ← VectorIndex: load/save/upsert/search
│   │   │   └── models.py        ← IndexEntry, SearchResult
│   │   │
│   │   └── tools/
│   │       ├── __init__.py
│   │       ├── brain_store.py   ← tool: brain_store
│   │       ├── brain_read.py    ← tool: brain_read
│   │       ├── brain_search.py  ← tool: brain_search
│   │       └── brain_reindex.py ← tool: brain_reindex
│   │
│   └── tests/
│       ├── __init__.py
│       ├── conftest.py          ← fixtures: mock vault, mock ollama
│       ├── test_vault_manager.py
│       ├── test_embedding_engine.py
│       ├── test_index_store.py
│       ├── test_tools.py
│       └── test_integration.py  ← integração com temp dir + ollama real
│
├── vault/                       ← Obsidian vault (default local)
│   ├── arquitetura/
│   │   └── .gitkeep
│   ├── regras/
│   │   └── .gitkeep
│   ├── sessoes/
│   │   └── .gitkeep
│   ├── projetos/
│   │   └── .gitkeep
│   └── indexacao/
│       └── .gitkeep
│
├── data/                        ← dados runtime (não versionado)
│   └── index.json               ← índice de embeddings persistido
│
├── pyproject.toml               ← uv/poetry project config
├── .env.example                 ← template de variáveis de ambiente
└── README.md
```

---

## Data Models

### Note (vault/models.py)
```python
@dataclass
class Note:
    layer: str          # ex: "arquitetura", "regras"
    path: str           # ex: "projeto-x/banco-de-dados"
    content: str        # markdown bruto
    metadata: dict      # extra (future: frontmatter parsed)
```

### IndexEntry (index/models.py)
```python
@dataclass
class IndexEntry:
    path: str           # "arquitetura/projeto-x/banco-de-dados"
    layer: str          # "arquitetura"
    embedding: list[float]  # vetor 768d (nomic-embed-text)
    chunk_index: int    # 0 = primeiro chunk da nota
    total_chunks: int   # total de chunks da nota
```

### SearchResult (index/models.py)
```python
@dataclass
class SearchResult:
    path: str
    layer: str
    score: float        # cosine similarity 0..1
    snippet: str        # primeiros ~200 chars do chunk
    chunk_index: int
```

### Valid Layers
```python
VALID_LAYERS: frozenset[str] = frozenset({
    "arquitetura",
    "regras",
    "sessoes",
    "projetos",
    "indexacao",
})
```

---

## Interfaces (MCP Tools)

### `brain_search`

```
brain_search(query: str, layer: str | None = None, top_k: int = 5) -> list[SearchResult]
```

| Parâmetro | Tipo | Default | Descrição |
|-----------|------|---------|-----------|
| query | string | obrigatório | Texto para busca semântica |
| layer | string? | null | Filtrar por camada |
| top_k | integer | 5 | Máx resultados (1–20) |

**Retorno:** `ToolResult` com array de `{path, layer, score, snippet, chunk_index}` ordenados por score descendente.

**Erros:**
- `query` vazia → erro `"INVALID_PARAMS"`
- `layer` inválido → erro `"INVALID_LAYER"`
- Ollama timeout → erro `"EMBEDDING_FAILED"`

### `brain_store`

```
brain_store(layer: str, path: str, content: str) -> {status: "ok"}
```

| Parâmetro | Tipo | Descrição |
|-----------|------|-----------|
| layer | string | Camada (validadas) |
| path | string | Path relativo sem extensão, ex: "projeto-x/regra-1" |
| content | string | Conteúdo markdown |

**Retorno:** `{status: "ok"}` em sucesso.

**Erros:**
- `layer` inválido → `"INVALID_LAYER"`
- `path` vazio ou inválido → `"INVALID_PATH"`

**Efeitos colaterais:**
- Cria diretórios intermediários se necessário
- Trigger async de reindex do arquivo

### `brain_read`

```
brain_read(layer: str, path: str) -> {content: str, path: str, layer: str}
```

| Parâmetro | Tipo | Descrição |
|-----------|------|-----------|
| layer | string | Camada |
| path | string | Path relativo sem extensão |

**Retorno:** Objeto com `content` (markdown), `path`, `layer`.

**Erros:**
- Arquivo não encontrado → `"NOT_FOUND"`
- `layer` inválido → `"INVALID_LAYER"`

### `brain_reindex`

```
brain_reindex(all: bool = False, layer: str | None = None, path: str | None = None) -> {status: "reindexing", target: str}
```

| Parâmetro | Tipo | Default | Descrição |
|-----------|------|---------|-----------|
| all | boolean | false | Reindexar vault inteiro |
| layer | string? | null | Reindexar apenas camada |
| path | string? | null | Reindexar apenas arquivo específico |

**Retorno:** `{status: "reindexing", target: "...", estimate: "N files"}`

**Erros:**
- Nenhum parâmetro informado → `"INVALID_PARAMS"` (exige `all=true` ou `layer` ou `path`)
- `layer` + `path` simultâneos → `"CONFLICTING_PARAMS"`

**Comportamento:**
- Reindex é bloqueante (síncrono) — espera conclusão
- Durante reindex, novas stores ainda funcionam
- Se index.json não existir, cria do zero

---

## Decisões de Design

### Decisão 1: Persistência em JSON ao invés de banco vetorial

**Contexto:** Precisamos armazenar embeddings para busca por similaridade.

**Opções:**
1. JSON flat file — implementação simples, sem dependências, fácil de debugar
2. SQLite com extensão vetorial (sqlite-vec) — mais escalável, busca performática
3. ChromaDB local — banco vetorial dedicado, feature-rich

**Decisão:** JSON flat file (fase inicial).

**Rationale:** O volume esperado é pequeno (centenas de notas, não milhões). JSON é versionável, debuggável, e não adiciona dependências. Se escalar, migramos para sqlite-vec.

**Consequências:** Busca linear O(n) no número de chunks. Para <10k chunks, latência <10ms.

### Decisão 2: Sem frontend web

**Contexto:** Precisamos expor as funcionalidades do cérebro.

**Opções:**
1. Apenas MCP (protocolo nativo)
2. MCP + API REST
3. MCP + dashboard web

**Decisão:** Apenas MCP.

**Rationale:** O público-alvo são agentes de IA, não humanos. Interface web seria over-engineering. A skill `/brain` é o ponto de consumo.

### Decisão 3: Embedding por chunk (seção ##) ao invés de nota inteira

**Contexto:** Notas longas precisam ser buscáveis com precisão.

**Opções:**
1. Embedding da nota inteira
2. Embedding por seção (## headings)
3. Embedding por sliding window

**Decisão:** Embedding por seção (split por `##` headings).

**Rationale:** Notas no Obsidian naturalmente têm seções. Embedding por seção permite busca granular. Sliding window é mais complexo e gera ruído.

**Consequências:** Uma nota com 5 seções gera 5 entries no index. `brain_search` retorna o chunk mais relevante, não a nota inteira (mas inclui caminho para ler a nota completa).

---

## Error Handling

### Categorias de Erro

| Categoria | Código MCP | Descrição |
|-----------|-----------|-----------|
| Invalid params | `INVALID_PARAMS` | Parâmetros obrigatórios ausentes ou tipo errado |
| Invalid layer | `INVALID_LAYER` | Layer não está em VALID_LAYERS |
| Not found | `NOT_FOUND` | Arquivo não encontrado no vault |
| Embedding failed | `EMBEDDING_FAILED` | Ollama retornou erro ou timeout |
| Index empty | `INDEX_EMPTY` | Nenhum embedding no índice (busca retorna vazio) |
| Internal error | `INTERNAL_ERROR` | Erro inesperado (IO, permissão, etc.) |

### Estratégia

- **Validação de entrada**: todo tool validates params antes de chamar IO
- **Erros de IO**: capturados e convertidos para erro MCP com mensagem descritiva
- **Ollama**: timeout configurável (default 30s); falha de embedding não quebra store, apenas impede indexação daquele arquivo
- **Logging**: estrutura de logs com níveis (INFO, WARNING, ERROR) usando módulo `logging`

---

## Configuração (Environment Variables)

| Variável | Default | Descrição |
|----------|---------|-----------|
| `BRAIN_VAULT_PATH` | `./vault` | Path absoluto ou relativo para o vault Obsidian |
| `BRAIN_OLLAMA_URL` | `http://localhost:11434` | URL base do Ollama |
| `BRAIN_OLLAMA_MODEL` | `nomic-embed-text` | Modelo de embedding |
| `BRAIN_INDEX_PATH` | `./data/index.json` | Path do arquivo de índice |
| `BRAIN_PORT` | `8321` | Porta do MCP server (stdio ou SSE) |
| `BRAIN_TRANSPORT` | `stdio` | Transporte MCP: `stdio` ou `sse` |
| `BRAIN_LOG_LEVEL` | `INFO` | Nível de log |
| `BRAIN_CHUNK_MAX_TOKENS` | `4096` | Tamanho máximo do chunk para embedding |

---

## Segurança

- O MCP server não implementa autenticação (público na rede local)
- O server só acessa arquivos dentro do `BRAIN_VAULT_PATH` (path traversal validation)
- Sem execução de código arbitrário
- Sem exposição de variáveis de ambiente ou credenciais

---

## Testing Strategy

### Unit Tests (vitais, cobertura > 80%)

| Módulo | Abordagem | Mock |
|--------|-----------|------|
| `vault/manager.py` | CRUD em temp directory | `tmp_path` fixture |
| `embeddings/engine.py` | Chamadas HTTP ao Ollama | `responses` ou `httpx.MockTransport` |
| `index/store.py` | In-memory index, persist mock | `tmp_path` + dict |
| `tools/` | Simular chamadas às dependências | `unittest.mock` |

### Integration Tests (um fluxo completo)

- Servidor MCP real rodando em processo filho
- Temp vault + temp index
- Ollama real ou mock HTTP que retorna vetor fixo
- Testar ciclo: store → search → read → reindex

### Test Files

```
src/tests/
├── conftest.py
├── test_vault_manager.py
├── test_embedding_engine.py
├── test_index_store.py
├── test_tools.py
└── test_integration.py
```

### Comandos de Teste

```bash
cd brain
pytest                          # unit tests
pytest --cov=src/brain_server   # com cobertura
pytest -xvs tests/test_integration.py  # integração
```

---

## Dependências (pyproject.toml)

```toml
[project]
name = "brain-server"
version = "0.1.0"
description = "MCP server — cérebro para agentes de IA"
requires-python = ">=3.11"
dependencies = [
    "mcp>=1.0.0",          # MCP protocol SDK
    "httpx>=0.27",         # HTTP client for Ollama
    "pydantic>=2.0",       # config/data validation
]

[project.optional-dependencies]
dev = [
    "pytest>=8",
    "pytest-cov>=5",
    "pytest-asyncio>=0.24",
    "respx>=0.21",         # mock HTTPX
]
```

---

## /brain Skill (para projetos consumidores)

A skill fica em `.agents/skills/brain/SKILL.md` e é carregada por outros repositórios via:

```json
// opencode.json do projeto consumidor
{
  "instructions": [
    "../brain/.agents/skills/brain/SKILL.md"
  ]
}
```

A skill deve:
1. Expor a configuração de conexão (MCP transport, URL)
2. Documentar as 3 tools principais com exemplos
3. Fornecer um "quickstart" para o agente usar o cérebro

(Detalhado na fase de Tasks — a skill é um deliverable do projeto)
