# Phase 3 — Tasks

## Brain MCP Server — Cérebro para Agentes de IA

---

## Estratégia de Sequenciamento

**Foundation-First + Feature-Slice híbrido:**
1. Base do projeto (init, config, diretórios)
2. Core layers individualmente testáveis (vault → embeddings → index)
3. Integração via MCP tools
4. Skill consumidora
5. Polimento

Cada task é 2–4h de trabalho.

---

## Task List

### T1 — Project Scaffolding

**Objetivo:** Inicializar projeto Python com `uv`, estrutura de diretórios, config, entrypoint vazio.

**Arquivos para criar:**
- `pyproject.toml` — config do projeto, dependências (`mcp`, `httpx`, `pydantic`)
- `src/brain_server/__init__.py`
- `src/brain_server/__main__.py` — entrypoint `python -m brain_server`
- `src/brain_server/config.py` — dataclass com env vars, método `from_env()`
- `src/brain_server/server.py` — MCP server vazio com FastMCP, apenas inicialização
- `vault/` — diretórios das 5 camadas com `.gitkeep`
- `data/.gitkeep`
- `.env.example`
- `src/tests/__init__.py`, `src/tests/conftest.py` — fixtures básicas

**Detalhes:**
- `config.py` deve ler de variáveis de ambiente com fallback para defaults (ver tabela no design)
- `server.py` deve instanciar `FastMCP("brain")`, definir `def main(): server.run()`
- `__main__.py` chama `server.main()`

**Critério de conclusão:** `uv run python -m brain_server` inicia e loga "Brain MCP server starting..."

- *Requirements: US-01*

---

### T2 — Vault Manager (CRUD em markdown)

**Objetivo:** Implementar `vault/manager.py` com operações de leitura, escrita e validação de camadas.

**Arquivos para criar:**
- `src/brain_server/vault/__init__.py`
- `src/brain_server/vault/models.py` — dataclass `Note`, constantes `VALID_LAYERS`
- `src/brain_server/vault/manager.py` — classe `VaultManager`

**Métodos de VaultManager:**
- `__init__(vault_path: Path)` — valida/ cria diretórios das camadas
- `write(layer: str, path: str, content: str) -> Note` — escreve arquivo, cria dirs se necessário
- `read(layer: str, path: str) -> Note` — lê arquivo, retorna Note
- `delete(layer: str, path: str) -> None` — remove arquivo
- `list(layer: str | None = None) -> list[Note]` — lista arquivos (com ou sem filtro de layer)
- `list_all_files() -> list[Path]` — scan recursivo no vault
- `validate_layer(layer: str) -> bool`
- `sanitize_path(path: str) -> Path` — valida path traversal

**Regras:**
- Path relativo, sem `..`, sem extensão (manager adiciona `.md`)
- Qualquer operação com layer inválido levanta `ValueError`
- Escrita substitui arquivo existente sem warning

**Testes (T2-T):**
- `test_vault_manager.py`:
  - Escrever e ler nota em cada layer
  - Tentar escrever em layer inválido → erro
  - Tentar ler arquivo inexistente → erro
  - Path traversal (`../../etc/passwd`) → rejeitado
  - List retorna arquivos corretos
  - Delete remove arquivo

- *Requirements: US-02, US-03, US-08*

---

### T3 — Embeddings Engine (Ollama Client)

**Objetivo:** Implementar comunicação com Ollama para gerar embeddings.

**Arquivos para criar:**
- `src/brain_server/embeddings/__init__.py`
- `src/brain_server/embeddings/engine.py` — classe `EmbeddingEngine`

**Métodos de EmbeddingEngine:**
- `__init__(base_url: str, model: str, timeout: int)`
- `embed(text: str) -> list[float]` — gera embedding para um texto
- `embed_batch(texts: list[str]) -> list[list[float]]` — gera embeddings em lote
- `chunk_text(text: str, max_tokens: int) -> list[str]` — divide markdown por seções `##`

**Detalhes:**
- Usa `httpx.AsyncClient` para chamar `POST /api/embeddings` do Ollama
- Payload: `{"model": "nomic-embed-text", "prompt": text}`
- Resposta: `{"embedding": [0.1, 0.2, ...]}`
- Em caso de erro/timeout, levanta `EmbeddingError`
- `chunk_text()` divide por headings `##` (seção), trunca cada chunk em `max_tokens`

**Testes (T3-T):**
- `test_embedding_engine.py`:
  - Mockar HTTPX com respx — testar embed() retorna vetor
  - Mockar timeout — testar EmbeddingError
  - `chunk_text()` com markdown de 3 seções → 3 chunks
  - `chunk_text()` com texto menor que max_tokens → 1 chunk

- *Requirements: US-05*

---

### T4 — Vector Index (Store + Search)

**Objetivo:** Implementar índice de embeddings em JSON com busca por cosine similarity.

**Arquivos para criar:**
- `src/brain_server/index/__init__.py`
- `src/brain_server/index/models.py` — dataclass `IndexEntry`, `SearchResult`
- `src/brain_server/index/store.py` — classe `VectorIndex`

**Métodos de VectorIndex:**
- `__init__(index_path: Path)`
- `load()` — carrega `index.json` do disco
- `save()` — persiste índice em disco
- `upsert(path: str, layer: str, embedding: list[float], chunk_index: int, total_chunks: int)` — adiciona ou atualiza entry
- `remove(path: str)` — remove entries de um path
- `search(query_embedding: list[float], top_k: int, layer_filter: str | None) -> list[SearchResult]` — cosine similarity, ranking
- `clear()` — limpa índice em memória
- `size() -> int` — número de entries
- `_cosine_similarity(a: list[float], b: list[float]) -> float`

**Detalhes:**
- Index JSON schema: `{"version": 1, "entries": [IndexEntry, ...]}`
- Busca linear O(n) — suficiente para <10k entries
- Se `layer_filter` fornecida, filtra antes do rankeamento
- Snippet = primeiros 200 caracteres do chunk

**Testes (T4-T):**
- `test_index_store.py`:
  - Criar índice, upsert 3 entries, search retorna top scores
  - Persistir e recarregar — dados preservados
  - Search com `layer_filter` — só resultados da camada
  - Search em índice vazio → lista vazia
  - Remover entry e verificar que sumiu

- *Requirements: US-04, US-05*

---

### T5 — MCP Tools (Store, Read, Search, Reindex)

**Objetivo:** Implementar as 4 tools MCP e conectá-las ao servidor.

**Arquivos para criar:**
- `src/brain_server/tools/__init__.py`
- `src/brain_server/tools/brain_store.py`
- `src/brain_server/tools/brain_read.py`
- `src/brain_server/tools/brain_search.py`
- `src/brain_server/tools/brain_reindex.py`

**Atualizar:**
- `src/brain_server/server.py` — instanciar VaultManager, EmbeddingEngine, VectorIndex; registrar tools

**Detalhes de cada tool:**

**brain_store:**
1. Valida layer e path
2. Escreve no vault via VaultManager
3. Chama EmbeddingEngine.embed(content)
4. Faz chunking e upsert no índice
5. Persiste índice
6. Retorna sucesso

**brain_read:**
1. Valida layer e path
2. Lê do vault via VaultManager
3. Retorna conteúdo

**brain_search:**
1. Valida query não vazia
2. Gera embedding da query via EmbeddingEngine
3. Busca no VectorIndex (com layer_filter opcional)
4. Retorna resultados ordenados

**brain_reindex:**
1. Determina escopo (all, layer, path)
2. Se path: reindexa arquivo específico
3. Se layer: scan da camada, reindexa todos
4. Se all: scan completo do vault
5. Para cada arquivo: chunk → embed → upsert
6. Persiste índice ao final

**server.py wiring:**
```python
from mcp.server import FastMCP

server = FastMCP("brain")

vault = VaultManager(config.vault_path)
embeddings = EmbeddingEngine(config.ollama_url, config.ollama_model)
index = VectorIndex(config.index_path)
index.load()

@server.tool()
async def brain_search(query: str, layer: str | None = None, top_k: int = 5):
    # ...

@server.tool()
async def brain_store(layer: str, path: str, content: str):
    # ...
```

**Testes (T5-T):**
- `test_tools.py`:
  - Mockar VaultManager, EmbeddingEngine, VectorIndex
  - Chamar cada tool e verificar retorno
  - brain_store com layer inválido → erro MCP
  - brain_search com query vazia → erro

- *Requirements: US-01, US-02, US-03, US-04, US-06*

---

### T6 — /brain Skill para Consumo Externo

**Objetivo:** Criar a skill OpenCode que outros repositórios carregam para conectar ao cérebro.

**Arquivos para criar:**
- `.agents/skills/brain/SKILL.md`

**Conteúdo da SKILL.md:**
```markdown
---
name: brain
description: Connect to the Brain MCP server — semantic search, store, and read notes
---

# /brain Skill

Connect to the central Brain MCP server from any OpenCode project.

## Setup

1. Ensure the brain MCP server is running:
   ```bash
   cd <brain-repo> && uv run python -m brain_server
   ```

2. The agent will connect automatically via MCP.

## Tools

### brain_search(query, [layer], [top_k])
Search notes by semantic similarity.

Example: `brain_search("database schema rules", layer="regras")`

### brain_store(layer, path, content)
Save a note to the brain vault.

Example: `brain_store("arquitetura", "meu-projeto/visao-geral", "# Visão Geral...")`

### brain_read(layer, path)
Read a note from the brain vault.

Example: `brain_read("regras", "meu-projeto/naming-conventions")`

## Best Practices

- **Before coding**: search brain for relevant context
- **After decisions**: store architecture decisions
- **Session handoff**: write session summary before context switch
- **Layers**: use `arquitetura`, `regras`, `sessoes`, `projetos` appropriately
```

**Critério de conclusão:** Skill legível e publicável. Pode ser referenciada por outros repositórios.

- *Requirements: US-07*

---

### T7 — Integration Tests + CI

**Objetivo:** Teste de integração completo (MCP server real em processo + temp dir) e roteiro de CI.

**Arquivos para criar/atualizar:**
- `src/tests/test_integration.py` — fluxo end-to-end
- `src/tests/conftest.py` — fixtures de integração
- `pyproject.toml` — adicionar script de teste

**Testes de Integração:**
1. Iniciar MCP server em processo filho com temp vault
2. Chamar `brain_store` → verificar arquivo criado no disco
3. Chamar `brain_read` → verificar conteúdo igual ao escrito
4. Chamar `brain_search` com query relacionada → verificar resultado não vazio
5. Chamar `brain_reindex(all=True)` → verificar índice populado
6. Testar caminhos de erro: layer inválido, path inexistente

**Mock para Ollama:**
- Servidor HTTP mock simples que retorna embedding fixo (vetor 4D [0.1]*4)
- Ou usar `respx` para interceptar chamadas do httpx

**Critério de conclusão:** `pytest -xvs tests/test_integration.py` passa.

- *Requirements: US-01, US-02, US-03, US-04, US-06*

---

### T8 — Error Handling, Logging e Polimento

**Objetivo:** Garantir tratamento de erros consistente, logging estruturado e documentação.

**Arquivos para atualizar:**
- `src/brain_server/server.py` — logging em cada operação
- `src/brain_server/vault/manager.py` — logging de IO
- `src/brain_server/embeddings/engine.py` — logging de chamadas Ollama
- `src/brain_server/index/store.py` — logging de persistência
- `README.md` — instruções de setup, exemplos

**Detalhes:**
- Adicionar `logging.getLogger(__name__)` em cada módulo
- Logar: operações (INFO), erros recuperáveis (WARNING), erros fatais (ERROR)
- Tratamento de erros padronizado:
  ```python
  try:
      ...
  except ValueError as e:
      return server.ErrorResult(f"INVALID_PARAMS: {e}")
  except EmbeddingError as e:
      return server.ErrorResult(f"EMBEDDING_FAILED: {e}")
  except Exception as e:
      logger.error(f"Unexpected error: {e}", exc_info=True)
      return server.ErrorResult(f"INTERNAL_ERROR: {e}")
  ```
- README com: setup, env vars, exemplos de uso, arquitetura
- Atualizar `AGENTS.md` com comandos exatos

**Critério de conclusão:** Servidor roda sem warnings, logs são informativos, README cobre setup.

- *Requirements: US-01*

---

## Dependências Entre Tasks

```
T1 (scaffold)
 ├── T2 (vault manager) ───┐
 ├── T3 (embeddings) ──────┤
 └── T4 (index) ───────────┤
                           ▼
                        T5 (tools)
                           │
                  ┌────────┼────────┐
                  ▼        ▼        ▼
                T6       T7       T8
              (skill)  (tests)  (polish)
```

T1 é requisito para T2/T3/T4. T2+T3+T4 são requisitos para T5. T6/T7/T8 podem rodar em paralelo após T5.

---

## Sumário de Esforço

| Task | Horas | Depende de |
|------|-------|-----------|
| T1 — Scaffold | 2h | — |
| T2 — Vault Manager | 3h | T1 |
| T3 — Embeddings Engine | 3h | T1 |
| T4 — Vector Index | 3h | T1 |
| T5 — MCP Tools | 4h | T2, T3, T4 |
| T6 — /brain Skill | 2h | — (independente) |
| T7 — Integration Tests | 3h | T5 |
| T8 — Error Handling & Polish | 2h | T5 |
| **Total** | **22h** | |
