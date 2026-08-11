# AGENTS.md — brain

## Stack

**MCP server Python** — servidor central de memória para agentes de IA.

| Camada | Tecnologia |
|--------|-----------|
| Server | Python 3.10+ + `mcp` (MCP protocol SDK) |
| Vault | Obsidian (notas `.md` em disco) |
| Embeddings | Ollama local (`nomic-embed-text`) |
| Transport | stdio (default) ou SSE |
| Testes | pytest + pytest-asyncio |

## Estrutura

```
src/brain_server/
├── __main__.py        → python -m brain_server
├── config.py          → env vars (prefix BRAIN_)
├── server.py          → FastMCP setup + tool registration
├── vault/             → CRUD em arquivos .md (manager.py, models.py)
├── embeddings/        → Ollama client + text chunking (engine.py)
├── index/             → índice vetorial JSON + cosine search (store.py, models.py)
└── tools/             → tools MCP (brain_store, brain_read, brain_search, brain_reindex)
```

## Comandos exatos

```bash
uv run python -m brain_server        # inicia servidor (stdio default)
BRAIN_TRANSPORT=sse uv run python -m brain_server  # modo SSE
uv run pytest src/tests/ -v          # unit + integration tests
uv run pytest src/tests/ --cov       # com cobertura
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
| `BRAIN_VAULT_PATH` | `./vault` | Path do vault Obsidian |
| `BRAIN_OLLAMA_URL` | `http://localhost:11434` | URL do Ollama |
| `BRAIN_OLLAMA_MODEL` | `nomic-embed-text` | Modelo de embedding |
| `BRAIN_INDEX_PATH` | `./data/index.db` | Banco vetorial SQLite |
| `BRAIN_PORT` | `8321` | Porta (transporte SSE) |
| `BRAIN_TRANSPORT` | `sse` | `stdio` ou `sse` |
| `BRAIN_LOG_LEVEL` | `INFO` | Nível de log |

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
| `brain_store(layer, path, content, scope?, project?, tags?)` | Salva nota + indexa. Scope obrigatório para arquitetura/regras/estudos |
| `brain_read(layer, path, scope?)` | Lê nota do vault. Scope obrigatório para arquitetura/regras/estudos |
| `brain_search(query, layer?, scope?, project?, tag?, top_k?)` | Busca semântica (embedding). Filtros por layer, scope, projeto, tag |
| `brain_reindex(all?, layer?, path?)` | Reconstrói índice |
| `brain_project_create(name, description?)` | Cria um projeto no registro |
| `brain_project_list()` | Lista todos os projetos registrados |
| `brain_project_notes(project)` | Lista notas de um projeto (owned + linked) |
| `brain_project_link(note_path, project)` | Liga nota global a um projeto |
| `brain_project_unlink(note_path, project)` | Remove ligação nota ↔ projeto |
| `brain_project_delete(name)` | Deleta um projeto |

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

1. `src/brain_server/tools/<tool>.py` — implementação
2. `src/brain_server/server.py` — registro da tool
3. `src/brain_server/tools/__init__.py` — export (se aplicável)
4. `src/tests/test_tools.py` — teste unitário
5. `src/tests/test_integration.py` — teste de integração
6. `AGENTS.md` — tabela de tools
7. `.agents/skills/brain/SKILL.md` — documentação da tool
8. `.agents/skills/brain/tools/<tool>/SKILL.md` — skill individual
9. `hooks/brain-hook.py` — se aplicável ao auto-capture

### Ao adicionar endpoint REST (futuro):

1. `src/brain_server/api/` — handler
2. `src/brain_server/server.py` — registro
3. `src/tests/` — teste
4. `AGENTS.md` — tabela de endpoints

### Ao alterar schema do índice:

1. `src/brain_server/index/models.py` — IndexEntry / SearchResult
2. `src/brain_server/index/store.py` — upsert/search/persist
3. `src/tests/test_index_store.py` — teste
4. `src/brain_server/tools/brain_reindex.py` — reindex
5. `src/tests/test_integration.py` — integração

## Regras de desenvolvimento

- **Versionamento obrigatório**: Ao adicionar features, fixes ou breaking changes, SEMPRE atualizar a versão em TODOS os arquivos:
  1. `pyproject.toml` → `version = "x.y.z"`
  2. `src/brain_server/__init__.py` → `__version__ = "x.y.z"`
  3. `workflow-state.json` → `test_count`, `tasks_completed`, `last_updated`
  - **Patch** (x.y.Z): fixes, docs, refactoring sem mudança de API
  - **Minor** (x.Y.0): novas features, tools, parâmetros (backward-compatible)
  - **Major** (X.0.0): breaking changes, remoção de tools, mudança de schema
- **Spec-driven-development** para features novas (`.spec/brain-mcp-server/`)
- **workflow-state.json** deve ser lido/criado antes de começar
- **Code review** obrigatório ao finalizar tarefas
- **Testes**: `uv run pytest src/tests/ -v` antes de cada commit
- **Cobertura mínima**: 70% (verificar com `--cov`)
- Arquivos `.agents/rules/` descrevem stack Java/Spring+Vue legada — ignorar seções específicas

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
