# brain 🧠

**Servidor MCP de memória central para agentes de IA.**  
Persiste conhecimento em vault Obsidian, indexa via embeddings locais (Ollama) e expõe busca semântica, leitura e escrita através do protocolo MCP.

---

## Índice

- [Stack](#stack)
- [Instalação](#instalação)
- [Quick Start](#quick-start)
- [Estrutura do Vault com Scope](#estrutura-do-vault-com-scope)
- [CLI — Uso Diário](#cli--uso-diário)
- [Tools MCP](#tools-mcp)
- [Como Consumir de Outro Projeto](#como-consumir-de-outro-projeto)
- [Web Viewer](#web-viewer)
- [Auto-Capture Hooks](#auto-capture-hooks)
- [Testes](#testes)
- [Variáveis de Ambiente](#variáveis-de-ambiente)

---

## Stack

| Camada | Tecnologia |
|--------|-----------|
| Runtime | Python 3.10+ |
| Protocolo | `mcp` SDK (Model Context Protocol) |
| Vault | Obsidian — notas `.md` em disco |
| Embeddings | Ollama local (`nomic-embed-text`) |
| Transporte | stdio (default) ou SSE |
| Persistência | Arquivos `.md` + SQLite vetorial (`.db`) |
| Testes | pytest + pytest-asyncio + respx |

**Sem banco externo.** Toda persistência é em sistema de arquivos.  
**Sem GPU.** Embeddings rodam via Ollama (CPU) ou serviço externo.

---

## Instalação

### Opção 1: Instalação Global (recomendado)

Instale o brain como ferramenta global para usar o comando `brain` diretamente:

```bash
# Clonar o repositório
git clone <repo> brain
cd brain

# Instalar globalmente com uv
uv tool install .

# Verificar instalação
brain --version
brain ping
```

Agora você pode usar `brain` em qualquer lugar sem `uv run`:

```bash
brain server start
brain store regras meu-app/naming "## Regra" --scope projetos
brain search "naming" --layer regras --scope projetos
```

**Diretórios padrão (instalação global):**
- Vault: `~/.brain/vault`
- Dados: `~/.brain/data`
- Logs: `~/.brain/data/brain-server.log`

Você pode customizar com a variável de ambiente `BRAIN_DIR`:
```bash
export BRAIN_DIR=/caminho/personalizado
brain server start
```

### Opção 2: Instalação com pip

Se preferir usar pip:

```bash
# Clonar o repositório
git clone <repo> brain
cd brain

# Instalar globalmente
pip install .

# Ou em modo editable (para desenvolvimento)
pip install -e .

# Verificar instalação
brain --version
```

### Opção 3: Usar com uv run (sem instalação)

Se não quiser instalar globalmente, use `uv run`:

```bash
cd brain
uv sync
uv run brain ping
uv run brain store regras meu-app/naming "## Regra" --scope projetos
```

### Instalar Ollama e modelo de embeddings

```bash
# macOS/Linux
curl -fsSL https://ollama.com/install.sh | sh

# Pull do modelo de embeddings
ollama pull nomic-embed-text
```

### Setup automático (recomendado após instalação)

O comando `brain setup all` configura tudo de uma vez:

```bash
brain setup all
```

Isso configura:
- ✅ MCP server para OpenCode
- ✅ MCP server para GitHub Copilot
- ✅ MCP server para Kiro
- ✅ Regras/instructions para Copilot Chat
- ✅ Steering rules para Kiro
- ✅ Serviço systemd (auto-start)
- ✅ Shell autostart (zsh)

### Setup manual (alternativo)

Se preferir configurar manualmente:

```bash
# Apenas MCP server para OpenCode
brain setup opencode

# Apenas MCP server para Copilot
brain setup copilot

# Apenas regras para Copilot Chat
brain setup copilot-instructions

# Apenas MCP server para Kiro
brain setup kiro

# Apenas steering rules para Kiro
brain setup kiro-steering

# Apenas serviço systemd
brain setup systemd
```

### Verificar instalação

```bash
# Iniciar servidor
brain server start

# Testar conexão
brain ping  # → pong
```

---

## Quick Start

Após instalar globalmente (veja [Instalação](#instalação)):

```bash
# 1. Iniciar servidor (modo SSE — recomendado)
brain server start

# 2. Em outro terminal, testar
brain ping                    # → pong

# 3. Salvar nota (scope obrigatório para arquitetura/regras)
brain store regras meu-projeto/naming "## Regra de naming" --scope projetos
brain store regras coding-standards "## Padrões universais" --scope global

# 4. Buscar com filtro de scope
brain search "naming" --layer regras --scope projetos
brain search "padrões" --layer regras --scope global

# 5. Ler nota
brain read regras meu-projeto/naming --scope projetos
```

> 💡 O servidor em modo SSE escuta em `http://localhost:8321` (configurável via `BRAIN_PORT`).

> Se não instalou globalmente, use `uv run brain` em vez de `brain` nos comandos acima.

---

## Estrutura do Vault com Scope

As camadas `arquitetura`, `regras` e `estudos` usam **scope** para separar conteúdo específico de projeto de conteúdo compartilhado:

```
vault/
├── arquitetura/
│   ├── projetos/          ← específico por projeto
│   │   └── meu-app/
│   │       ├── stack.md
│   │       └── database.md
│   └── global/            ← compartilhado entre todos
│       ├── patterns.md
│       └── best-practices.md
├── regras/
│   ├── projetos/          ← regras de negócio específicas
│   │   └── meu-app/
│   │       ├── naming.md
│   │       └── workflows.md
│   └── global/            ← lições gerais, padrões universais
│       ├── coding-standards.md
│       └── lessons-learned.md
├── estudos/
│   ├── projetos/          ← estudos específicos do projeto
│   │   └── meu-app/
│   │       └── analise-db.md
│   └── global/            ← conhecimento geral compartilhado
│       ├── java-oo.md
│       └── docker-fundamentos.md
├── sessoes/               ← já é por projeto (sem scope)
│   └── meu-app/
│       └── 2026-07-25.md
├── projetos/              ← metadados (sem scope)
│   └── meu-app.md
└── indexacao/             ← interno (sem scope)
```

### Regra de Ouro: Escolha o Scope Correto

**Nunca** salve em `global` sem verificar se é realmente universal.

Pergunte: "Essa regra/lição se aplica a **TODOS** os projetos ou só ao meu?"

| Se... | Use scope | Exemplo |
|-------|-----------|---------|
| Específico do projeto | `projetos` | "Tabela de usuários tem campo `tenant_id`" |
| Universal/compartilhado | `global` | "Nunca usar SELECT * em produção" |

### Estudos: Conteúdo Completo, Não Resumo

Quando você aprender algo (ex: estudar Java, Docker, padrões), salve na camada `estudos` o **CONTEÚDO COMPLETO**, não apenas um resumo.

Inclua **tags no topo** (YAML frontmatter) para melhorar a busca semântica:

```yaml
---
tags: [java, orientacao-objetos, fundamentos]
topico: Java OO
nivel: iniciante
---
```

Isso permite que o brain encontre estudos relevantes quando você buscar por tecnologia, conceito ou nível de dificuldade.

---

## CLI — Uso Diário

O CLI `brain` é a forma mais prática de interagir com o servidor no terminal.

### Pré-requisito

O servidor MCP precisa estar rodando:

```bash
brain server start
```

### Comandos

| Comando | Descrição | Exemplo |
|---------|-----------|---------|
| `brain ping` | Health-check | `brain ping` |
| `brain store <layer> <path> <content> [--scope]` | Salva nota | `brain store regras meu-app/naming "## Regra" --scope projetos` |
| `brain read <layer> <path> [--scope]` | Lê nota | `brain read regras meu-app/naming --scope projetos` |
| `brain search <query> [--layer] [--scope]` | Busca semântica | `brain search "naming" --layer regras --scope projetos` |
| `brain reindex` | Reconstrói índice | `brain reindex --all` |

### Scope obrigatório

Para camadas `arquitetura` e `regras`, o parâmetro `--scope` é **obrigatório**:

```bash
# ✅ Correto
brain store regras meu-app/naming "## Regra" --scope projetos
brain store regras coding-standards "## Padrões" --scope global

# ❌ Erro (scope obrigatório)
brain store regras meu-app/naming "## Regra"
```

Para camadas `sessoes` e `projetos`, não use scope:

```bash
# ✅ Correto (sem scope)
brain store sessoes meu-app/2026-07-25 "## Sessão"
brain store projetos meu-app "## Visão geral"
```

### Flags do CLI

```bash
brain search --help
Usage: brain search [OPTIONS] QUERY

Options:
  -k, --top-k INTEGER  Número de resultados (default: 5)
  -l, --layer TEXT     Filtrar por camada
  -s, --scope TEXT     Filtrar por scope (projetos ou global)
  -j, --json           Output como JSON
  --help               Mostrar ajuda
```

### URL customizada

```bash
# Se o servidor estiver em porta diferente
BRAIN_URL=http://localhost:9000 brain ping
```

---

## Tools MCP

Quando um **agente de IA** (Claude Code, OpenCode, Kiro, Copilot) se conecta ao brain via MCP, ele ganha acesso a estas tools:

### `ping()`
**Health-check.** Retorna `"pong"`.

### `brain_store(layer, path, content, scope?)`
**Salva nota + indexa para busca semântica.**

```python
# Salvar decisão arquitetural (scope obrigatório)
brain_store(
    layer="arquitetura",
    path="meu-projeto/decisao-db",
    content="# Decisão: PostgreSQL\n\n## Contexto\nPrecisamos de um banco relacional...",
    scope="projetos"  # específico do projeto
)

# Salvar lição global (compartilhada)
brain_store(
    layer="regras",
    path="coding-standards",
    content="## Padrões universais\n\n- Nunca usar SELECT *",
    scope="global"  # compartilhado
)
```

### `brain_read(layer, path, scope?)`
**Lê nota completa do vault.**

```python
# Ler nota com scope (obrigatório para arquitetura/regras)
content = brain_read("regras", "meu-projeto/naming-conventions", scope="projetos")

# Ler nota global
content = brain_read("regras", "coding-standards", scope="global")
```

### `brain_search(query, layer?, scope?, top_k?)`
**Busca semântica por similaridade de embedding.**

```python
# Buscar com filtro de scope
results = brain_search("qual banco de dados usamos", scope="projetos", top_k=3)

# Buscar lições globais
results = brain_search("padrões de código", scope="global")
```

### `brain_reindex(all?, layer?, path?)`
**Reconstrói o índice vetorial.** Use quando notas forem alteradas manualmente no vault.

```python
brain_reindex(all=True)           # tudo
brain_reindex(layer="regras")     # só uma camada
brain_reindex(path="regras/projetos/app/naming-conventions")  # só um arquivo
```

---

## Como Consumir de Outro Projeto

Para que agentes de outro projeto usem o brain como memória central:

### 1. Configurar MCP server

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

### 2. Carregar a skill (recomendado)

A skill fornece documentação inline para os agentes:

```json
{
  "instructions": [
    "../brain/.agents/skills/brain/SKILL.md"
  ],
  "mcpServers": {
    "brain": {
      "transport": "sse",
      "url": "http://localhost:8321/sse"
    }
  }
}
```

### 3. Carregar regras (opcional)

Para regras completas de uso do brain:

```json
{
  "instructions": [
    "../brain/.agents/rules/BRAIN.MCP.md"
  ]
}
```

### 4. Usar

Os agentes do projeto terão automaticamente acesso a `brain_search`, `brain_store`, `brain_read` e `brain_reindex` — com documentação inline via SKILL.md.

### Exemplo completo

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

### Per-tool skills

Para documentação individual de cada tool:

```
.agents/skills/brain/tools/
├── brain_search/SKILL.md
├── brain_store/SKILL.md
├── brain_read/SKILL.md
└── brain_reindex/SKILL.md
```

---

## Web Viewer

Navegue e busque no vault pelo navegador.

### Iniciar

```bash
# Terminal 1: servidor brain (SSE)
brain server start

# Terminal 2: viewer server
uv run python viewer/server.py
# → http://0.0.0.0:8322
```

### Funcionalidades

- **Busca semântica** na barra de pesquisa (usa embeddings Ollama)
- **Browse all** — lista todas as notas do vault
- **Leitura** de notas completas com renderização markdown
- **Filtro por camada e scope** (Arquitetura, Regras, Sessões, Projetos)
- **Status** — mostra conexão, tamanho do índice, status do Ollama

### API REST do viewer

O `viewer/server.py` expõe uma API REST na porta `8322`:

| Endpoint | Descrição |
|----------|-----------|
| `GET /api/status` | Status do servidor, índice, Ollama |
| `GET /api/list` | Lista todas as notas |
| `GET /api/search?query=&top_k=&layer=&scope=` | Busca semântica |
| `GET /api/read?layer=&path=&scope=` | Lê nota completa |

> O viewer não substitui o servidor MCP. Ambos compartilham o mesmo vault e índice.

---

## Auto-Capture Hooks

Hooks capturam automaticamente o contexto das sessões de IA sem intervenção manual.

### Como funciona

Scripts em `hooks/` que escutam eventos do agente (início de sessão, fim de sessão, resultado de tool) e salvam/consultam o brain automaticamente.

### Hooks disponíveis

| Hook | Evento | O que faz |
|------|--------|-----------|
| `brain-hook.sh` | Genérico (shell) | Exemplo de hook multi-evento |
| `brain-hook.py` | Genérico (Python) | Hook programável com parse de JSON |

O hook `brain-hook.py` automaticamente:
- Busca contexto global (lições universais) no início da sessão
- Busca contexto específico do projeto no início da sessão
- Captura resultados de tools
- Salva resumo ao final da sessão

### Instalação no OpenCode

No `opencode.json` do projeto consumidor:

```json
{
  "hooks": {
    "session-start": "/absoluto/path/para/brain/hooks/brain-hook.py",
    "session-end": "/absoluto/path/para/brain/hooks/brain-hook.py",
    "tool-result": "/absoluto/path/para/brain/hooks/brain-hook.py"
  }
}
```

### Uso manual via CLI

```bash
# Buscar contexto global antes de começar
brain search "padrões" --layer regras --scope global -k 5

# Buscar contexto do projeto
brain search "contexto do projeto" --layer regras --scope projetos -k 5

# Salvar decisão importante
brain store arquitetura app/decisao "# Decisão: ..." --scope projetos

# Salvar lição universal
brain store regras coding-standards "## Padrão..." --scope global

# Salvar resumo ao finalizar sessão
brain store sessoes app/$(date +%Y-%m-%d) "## Sessão: ..."
```

> Detalhes completos em [`hooks/README.md`](hooks/README.md).

---

## Testes

```bash
# Unit + integration (67 testes)
uv run pytest src/tests/ -v

# Com cobertura
uv run pytest src/tests/ --cov

# Ver cobertura mínima (70%)
uv run pytest src/tests/ --cov --cov-fail-under=70
```

Os testes de integração:
- Iniciam um servidor MCP real em subprocesso
- Usam mock HTTP para o Ollama (`respx`)
- **Não requerem** Ollama real rodando
- Testam o ciclo completo: store → index → search → read → reindex

---

## Variáveis de Ambiente

Todas com prefixo `BRAIN_`:

| Variável | Default | Descrição |
|----------|---------|-----------|
| `BRAIN_DIR` | `~/.brain` (global) ou `.` (source) | Diretório base do brain |
| `BRAIN_VAULT_PATH` | `BRAIN_DIR/vault` | Diretório do vault Obsidian |
| `BRAIN_OLLAMA_URL` | `http://localhost:11434` | URL do Ollama |
| `BRAIN_OLLAMA_MODEL` | `nomic-embed-text` | Modelo de embedding |
| `BRAIN_INDEX_PATH` | `BRAIN_DIR/data/index.db` | Banco SQLite vetorial |
| `BRAIN_PORT` | `8321` | Porta do servidor SSE |
| `BRAIN_TRANSPORT` | `sse` | Transporte (`stdio` ou `sse`) |
| `BRAIN_LOG_LEVEL` | `INFO` | Nível de log |
| `BRAIN_VIEWER_PORT` | `8322` | Porta do web viewer |
| `BRAIN_URL` | `http://localhost:8321` | URL para o CLI `brain` |

**Nota:** Quando instalado globalmente (`uv tool install`), o `BRAIN_DIR` padrão é `~/.brain`. Quando rodando do source, é o diretório do repositório.

---

## Desenvolvimento

```bash
uv sync                    # Instalar dependências
brain server start         # Iniciar servidor (se instalado globalmente)
brain ping                 # CLI health-check
uv run pytest src/tests/ -v # Rodar testes
```

Ou sem instalação global:

```bash
uv run brain server start  # Iniciar servidor
uv run brain ping          # CLI health-check
```

### Comandos úteis

```bash
# MCP Inspector (debug)
uv run mcp dev src/brain_server/server.py

# Ver documentos do spec
ls .spec/brain-mcp-server/

# Workflow state
cat workflow-state.json

# Desinstalar (se instalado globalmente)
uv tool uninstall brain-server
```

---

> Projeto desenvolvido com **spec-driven-development** — veja `.spec/brain-mcp-server/` para os documentos de requirements, design e tasks.
