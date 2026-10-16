# Configuração do Brain como MCP Server

O brain é um **servidor MCP** (Model Context Protocol). Qualquer cliente que suporte MCP pode se conectar a ele para usar as tools `brain_store`, `brain_read`, `brain_search`, `brain_reindex`.

## Comando padrão

Em todas as configurações abaixo, o comando para iniciar o brain server é o
**binário Rust** (`brain serve-mcp`), não o Python:

```bash
/caminho/para/brain/target/release/brain \
  --db /caminho/para/brain/data/brain.db serve-mcp --port 8321
```

O `python -m brain_server` era o **legado Python**, removido dos exemplos
abaixo (mantido só por compatibilidade até a Fase C removê-lo — `AGENTS.md`:
*Legado Python mantido para compat até Fase C*). Ele responde por stdio com
um conjunto de tools **menor** que o Rust e grava num formato diferente. O viewer é
`brain serve --port 8322`.

Variáveis de ambiente relevantes:

| Variável | Default | Descrição |
|----------|---------|-----------|
| `BRAIN_DB_PATH` | `./data/brain.db` | **Única fonte de verdade.** SQLite WAL + FTS5. |
| `BRAIN_OLLAMA_URL` | `http://localhost:11434` | URL do Ollama. Userinfo é suportado (`http://user:pass@host`) e **redigido** em erro e log. |
| `BRAIN_OLLAMA_MODEL` | `nomic-embed-text` | Modelo de embedding. `EMBEDDING_DIM` é fixo em 768. |
| `BRAIN_PORT` | `8321` | Porta MCP: `/mcp` (Streamable HTTP) e `/sse` (SSE legado) |
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

## 1. OpenCode (V2 nativo)

> OpenCode V2 agrupa servidores em `mcp.servers`, troca `enabled` pelo
> inverso `disabled` e separa timeouts em `{catalog, execution}`
> (cf. https://opencode.ai/v2/docs/migrate-v1/).
> O campo V1 `mcpServers` (sem `servers`) e `timeout` número puro são
> legado — não usar em config nova.

### Transportes: qual URL usar

`serve-mcp` expõe **dois** transportes MCP na mesma porta, sobre a mesma lista
de 17 tools e o mesmo banco:

| URL | Transporte | Uso |
|-----|------------|-----|
| `http://localhost:8321/mcp` | **Streamable HTTP** (`POST`) | opencode V2, qualquer cliente atual — **é este** |
| `http://localhost:8321/sse` | SSE legado (`GET` + `POST /message`) | clientes antigos; **mantido**, não use em config nova |

> ### Por que `/sse` não serve para o opencode V2
>
> O opencode V2 com `type: "remote"` fala MCP **Streamable HTTP**, que é
> **só POST**: abre a sessão com `POST /mcp`, leu o `mcp-session-id` da resposta
> e seguiu usando esse header. Apontado para `/sse` ele falha na hora com
> `Error POSTing to endpoint (HTTP 405)` — porque `/sse` é uma rota **GET** que
> abre um stream de longa duração e cujas respostas voltam por um **segundo**
> `POST /message?sessionId=...`. O método bate, o protocolo não: 405 é o sintoma
> de um transporte incompatível, não de uma URL errada. Config nova usa `/mcp`.

### Configuração local (por projeto)

Em `opencode.json` do **projeto consumidor**, formato V2 nativo com o
**binário Rust** via stdio:

```json
{
  "mcp": {
    "servers": {
      "brain": {
        "type": "local",
        "command": ["/caminho/absoluto/para/brain/target/release/brain", "--db", "/caminho/absoluto/para/brain/data/brain.db", "serve-mcp", "--port", "8321"],
        "disabled": false,
        "timeout": { "catalog": 30000, "execution": 30000 },
        "env": {
          "BRAIN_DB_PATH": "/caminho/absoluto/para/brain/data/brain.db",
          "BRAIN_OLLAMA_URL": "http://localhost:11434"
        }
      }
    }
  }
}
```

Alternativa: servidor dedicado já rodando (`brain serve-mcp --port 8321`), config
remota **Streamable HTTP**:

```json
{
  "mcp": {
    "servers": {
      "brain": {
        "type": "remote",
        "url": "http://localhost:8321/mcp",
        "disabled": false,
        "timeout": { "catalog": 30000, "execution": 30000 }
      }
    }
  }
}
```

### Configuração global (todos os projetos)

`~/.config/opencode/opencode.json` — mesmo padrão Rust (stdio), forma V2:

```json
{
  "mcp": {
    "servers": {
      "brain": {
        "type": "local",
        "command": ["/caminho/absoluto/para/brain/target/release/brain", "--db", "/caminho/absoluto/para/brain/data/brain.db", "serve-mcp", "--port", "8321"],
        "disabled": false,
        "timeout": { "catalog": 30000, "execution": 30000 }
      }
    }
  }
}
```

Ou remota **Streamable HTTP** (servidor `brain serve-mcp --port 8321` já no ar):

```json
{
  "mcp": {
    "servers": {
      "brain": {
        "type": "remote",
        "url": "http://localhost:8321/mcp",
        "disabled": false,
        "timeout": { "catalog": 30000, "execution": 30000 }
      }
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
      "command": ["/caminho/absoluto/para/brain/target/release/brain", "--db", "/caminho/absoluto/para/brain/data/brain.db", "serve-mcp", "--port", "8321"]
    }
  }
}
```

> Legado Python (`uv run --directory ... python -m brain_server`) removido
> desta doc: responde por stdio com menos tools e formato diferente.
> Usar o binário Rust acima.

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
      "command": "/caminho/absoluto/para/brain/target/release/brain",
      "args": ["--db", "/caminho/absoluto/para/brain/data/brain.db", "serve-mcp", "--port", "8321"]
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
      "command": "/caminho/absoluto/para/brain/target/release/brain",
      "args": ["--db", "/caminho/absoluto/para/brain/data/brain.db", "serve-mcp", "--port", "8321"]
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
        "command": "/caminho/absoluto/para/brain/target/release/brain",
        "args": ["--db", "/caminho/absoluto/para/brain/data/brain.db", "serve-mcp", "--port", "8321"]
      }
    }
  }
}
```

---

## 5. Servidor dedicado (compartilhado entre projetos)

Para não iniciar um processo por cliente, rode o brain com os transportes HTTP e
configure todos os clientes para conectar via HTTP (Streamable HTTP, `/mcp`):

```bash
# Terminal 1: servidor dedicado (pode ser systemd/supervisor)
/caminho/para/brain/target/release/brain --db /caminho/para/brain/data/brain.db serve-mcp --port 8321
```

```json
{
  "mcp": {
    "servers": {
      "brain": {
        "type": "remote",
        "url": "http://localhost:8321/mcp",
        "disabled": false,
        "timeout": { "catalog": 30000, "execution": 30000 }
      }
    }
  }
}
```

> ⚠️ **Importante**: Use `http://` e não `https://`. O brain não implementa TLS. Se precisar de HTTPS, coloque um reverse proxy (nginx, Caddy) na frente.
>
> **Nota**: com transporte HTTP o servidor precisa estar rodando previamente. Vantagem: compartilhado entre projetos. A URL termina em `/mcp` (Streamable HTTP); `/sse` segue existindo para clientes legados.

---

## Tools expostas (17 tools rmcp)

Uma vez conectado, o cliente terá acesso a estas tools:

| Tool | Descrição |
|------|-----------|
| `ping` | Health-check → `"pong"` |
| `brain_store(layer, path, content, scope?, project?, tags?, pinned?, expires_at?)` | Salva nota (SQLite + FTS5 imediato; embedding via fila) |
| `brain_read(layer, path, scope?)` | Lê nota (`scope` obrigatório p/ arquitetura/regras/estudos) |
| `brain_search(query, layer?, scope?, project?, tag?, top_k?, explain?)` | Busca híbrida FTS5+vetor (RRF k=60) |
| `brain_delete(path)` | Hard-delete + audit |
| `brain_recent(top_k?)` | Últimas notas (`updated_at DESC`) |
| `brain_status` | Notas/chunks/projetos + cobertura de embedding + estado da fila |
| `brain_checkpoints(limit?)` | Audit log |
| `brain_restore(id)` | Restaura versão do audit |
| `brain_backup(to?)` | `cp brain.db → .bak` (allowlist `BRAIN_EXPORT_ROOT`) |
| `brain_export(to?, force?)` | Dump temporário sob `BRAIN_EXPORT_ROOT` |
| `brain_forget_sweep(dry_run?)` | Hard-delete por TTL (`expires_at`) |
| `brain_project_create(name, description?)` | Cria projeto |
| `brain_project_list()` | Lista projetos |
| `brain_project_notes(project)` | Notas owned+linked do projeto |
| `brain_project_link(note_path, project)` | Liga nota a projeto |
| `brain_project_unlink(note_path, project)` | Desliga nota de projeto |

Camadas válidas: `arquitetura`, `regras`, `estudos` (exigem `scope`: `global`|`projetos`), `sessoes`, `projetos`. `brain_reindex` é via CLI (`brain reindex --all`).

---

## Troubleshooting

### Conexão recusada / SSL erro

| Sintoma | Causa mais provável | Solução |
|---------|---------------------|---------|
| `SSL: CERTIFICATE_VERIFY_FAILED` | Usou `https://` na URL | Troque para `http://` |
| `Connection refused` | Servidor não está rodando | Rode `brain --db ./data/brain.db serve-mcp --port 8321` |
| `404 Not Found` | URL não termina em `/mcp` | Use `http://localhost:8321/mcp` |
| `HTTP 405` em `POST .../sse` | Cliente Streamable HTTP apontado no transporte SSE | Use `/mcp` — `/sse` é GET e responde por `POST /message` |
| `Timeout` | Porta errada | Verifique `BRAIN_PORT` (default `8321`) |

### Teste rápido

```bash
# 1) Transporte Streamable HTTP (o que o opencode V2 usa): handshake MCP real.
#    initialize devolve mcp-session-id; as chamadas seguintes usam esse header.
SID=$(curl -s -D - -o /tmp/brain-init.json -X POST http://localhost:8321/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"curl","version":"0.0.0"}}}' \
  | awk 'tolower($1)=="mcp-session-id:" {sub(/\r$/,"",$2); print $2}')
cat /tmp/brain-init.json   # frame SSE com serverInfo

# 2) tools/list com a sessão (deve listar 17 tools)
curl -s -X POST http://localhost:8321/mcp \
  -H "mcp-session-id: $SID" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | head -c 400

# 3) SSE legado: só o anúncio do endpoint (GET, stream que não fecha)
curl -s -N http://localhost:8321/sse | head -2
# → event: endpoint
# → data: /message?sessionId=<uuid>
```

## Verificação

Para testar se a configuração está correta:

```bash
# Via CLI do brain (requer servidor rodando com os transportes HTTP)
BRAIN_URL=http://localhost:8321 ./target/release/brain ping
# → pong
```
