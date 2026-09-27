# Configuração do Brain como MCP Server

O brain é um **servidor MCP** (Model Context Protocol). Qualquer cliente que suporte MCP pode se conectar a ele para usar as tools `brain_store`, `brain_read`, `brain_search`, `brain_reindex`.

## Comando padrão

Em todas as configurações abaixo, o comando para iniciar o brain server é o
**binário Rust** (`brain serve-mcp`), não o Python:

```bash
/caminho/para/brain/target/release/brain \
  --db /caminho/para/brain/data/brain.db serve-mcp --port 8321
```

O `python -m brain_server` que aparece abaixo é o **legado Python**, mantido
só por compatibilidade até a Fase C removê-lo (`AGENTS.md`: *Legado Python
mantido para compat até Fase C*). Ele responde por stdio com um conjunto de
tools **menor** que o Rust e grava num formato diferente. O viewer é
`brain serve --port 8322`.

Variáveis de ambiente relevantes:

| Variável | Default | Descrição |
|----------|---------|-----------|
| `BRAIN_DB_PATH` | `./data/brain.db` | **Única fonte de verdade.** SQLite WAL + FTS5. |
| `BRAIN_OLLAMA_URL` | `http://localhost:11434` | URL do Ollama. Userinfo é suportado (`http://user:pass@host`) e **redigido** em erro e log. |
| `BRAIN_OLLAMA_MODEL` | `nomic-embed-text` | Modelo de embedding. `EMBEDDING_DIM` é fixo em 768. |
| `BRAIN_PORT` | `8321` | Porta MCP SSE |
| `BRAIN_VIEWER_PORT` | `8322` | Porta do viewer (`/api/status,search,read,list`) |
| `BRAIN_EXPORT_ROOT` | `/tmp/brain-export` | **Único** diretório onde `brain export` e `brain backup` podem escrever |
| `BRAIN_EMBED_TIMEOUT_SECS` | `60` | Timeout base de embed; escalado por onda do lote |
| `BRAIN_EMBED_MAX_FAILURES` | `8` | Tentativas antes do dead-letter (~1 min de backoff) |
| `BRAIN_REUSE_SIMILARITY` | `1.0` | Abaixo de 1.0 um chunk editado herda o vetor antigo. Opt-in; a guarda de força normativa (obrigação↔proibição) bloqueia o reuso de qualquer jeito. |
| `BRAIN_HOOK_EMBED` | `1` | `0` pula o embed no hook — a captura nunca depende dele |
| `BRAIN_LOG_LEVEL` | `INFO` | Nível de log |

`BRAIN_VAULT_PATH` e `BRAIN_INDEX_PATH` foram **removidos** (constraint C01:
SQLite-only). Configurá-las é um no-op.

---

## 1. OpenCode

> OpenCode suporta MCP servers via configuração local ou global.

### Configuração local (por projeto)

Em `opencode.json` do **projeto consumidor**, adicione o servidor MCP:

```json
{
  "mcpServers": {
    "brain": {
      "command": "uv",
      "args": [
        "run",
        "--directory", "/caminho/absoluto/para/brain",
        "python", "-m", "brain_server"
      ],
      "env": {
        "BRAIN_DB_PATH": "/caminho/absoluto/para/brain/data/brain.db",
        "BRAIN_OLLAMA_URL": "http://localhost:11434"
      }
    }
  }
}
```

### Configuração global (todos os projetos)

`~/.config/opencode/mcp.json`:

```json
{
  "mcpServers": {
    "brain": {
      "command": "uv",
      "args": [
        "run",
        "--directory", "/caminho/absoluto/para/brain",
        "python", "-m", "brain_server"
      ]
    }
  }
}
```

> **Importante**: Use caminhos **absolutos** em `args`. O diretório de trabalho pode variar.

---

## 2. GitHub CLI (`gh`)

O GitHub CLI tem suporte experimental a MCP. Configure em:

`~/.config/gh/mcp.json` (Linux/macOS) ou `%APPDATA%\GitHub CLI\mcp.json` (Windows):

```json
{
  "servers": {
    "brain": {
      "type": "stdio",
      "command": ["uv", "run", "--directory", "/caminho/absoluto/para/brain", "python", "-m", "brain_server"]
    }
  }
}
```

Ative com:
```bash
gh mcp setup brain
```

---

## 3. Claude Desktop

`~/Library/Application Support/Claude/claude_desktop_config.json` (macOS)
ou `%APPDATA%\Claude\claude_desktop_config.json` (Windows):

```json
{
  "mcpServers": {
    "brain": {
      "command": "uv",
      "args": [
        "run",
        "--directory", "/caminho/absoluto/para/brain",
        "python", "-m", "brain_server"
      ]
    }
  }
}
```

---

## 4. Cursor / VS Code (Cline / Continue)

### Cursor

`~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "brain": {
      "command": "uv",
      "args": [
        "run",
        "--directory", "/caminho/absoluto/para/brain",
        "python", "-m", "brain_server"
      ]
    }
  }
}
```

### Continue (VS Code extension)

`~/.continue/config.json`:

```json
{
  "experimental": {
    "mcpServers": {
      "brain": {
        "command": "uv",
        "args": [
          "run",
          "--directory", "/caminho/absoluto/para/brain",
          "python", "-m", "brain_server"
        ]
      }
    }
  }
}
```

---

## 5. Servidor dedicado (compartilhado entre projetos)

Para não iniciar um processo por cliente, rode o brain em modo SSE e configure todos os clientes para conectar via HTTP:

```bash
# Terminal 1: servidor dedicado (pode ser systemd/supervisor)
BRAIN_TRANSPORT=sse BRAIN_PORT=8321 uv run python -m brain_server
```

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

> ⚠️ **Importante**: Use `http://` e não `https://`. O brain não implementa TLS. Se precisar de HTTPS, coloque um reverse proxy (nginx, Caddy) na frente.
>
> **Nota**: Em modo SSE o servidor precisa estar rodando previamente. Vantagem: compartilhado entre projetos.

---

## Tools expostas

Uma vez conectado, o cliente terá acesso a estas tools:

| Tool | Descrição |
|------|-----------|
| `ping` | Health-check |
| `brain_store(layer, path, content)` | Salva nota (SQLite; sem arquivo) |
| `brain_read(layer, path)` | Lê nota |
| `brain_search(query, layer?, top_k?)` | Busca semântica |
| `brain_reindex(all?, layer?, path?)` | Reconstrói índice |

Camadas válidas: `arquitetura`, `regras`, `sessoes`, `projetos`, `indexacao`

---

## Troubleshooting

### Conexão recusada / SSL erro

| Sintoma | Causa mais provável | Solução |
|---------|---------------------|---------|
| `SSL: CERTIFICATE_VERIFY_FAILED` | Usou `https://` na URL | Troque para `http://` |
| `Connection refused` | Servidor não está rodando | Rode `BRAIN_TRANSPORT=sse uv run python -m brain_server` |
| `404 Not Found` | URL sem `/sse` no final | Use `http://localhost:8321/sse` |
| `Timeout` | Porta errada | Verifique `BRAIN_PORT` (default `8321`) |

### Teste rápido

```bash
# Verificar se o servidor está no ar
curl -s -o /dev/null -w "%{http_code}" http://localhost:8321/sse
# Deve retornar 200

# Verificar via CLI
BRAIN_URL=http://localhost:8321 uv run brain ping
# Deve retornar pong
```

## Verificação

Para testar se a configuração está correta:

```bash
# Via CLI do brain (requer servidor rodando em SSE)
BRAIN_URL=http://localhost:8321 uv run brain ping
# → pong

# Ou via MCP Inspector
uv run mcp dev src/brain_server/server.py
```
