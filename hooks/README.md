# Brain Auto-Capture Hooks

Hooks capturam automaticamente o contexto das sessões dos agentes sem que o agente precise chamar `brain_store` explicitamente.

## Como funciona

Cada hook é um script que:
1. Recebe dados do evento via stdin (JSON)
2. Extrai informações relevantes
3. Chama o brain MCP server para salvar/consultar

## Hooks disponíveis

| Hook | Evento | O que captura |
|------|--------|---------------|
| `on-session-start` | Início da sessão | Carrega contexto relevante do brain |
| `on-prompt-submit` | Usuário enviou prompt | Salva o prompt no brain |
| `on-tool-result` | Tool retornou resultado | Salva resultados importantes |
| `on-session-end` | Fim da sessão | Salva resumo da sessão |

## Instalação por agente

### Claude Code

`~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": "/caminho/para/brain/hooks/on-session-start.sh",
    "PostToolUse": "/caminho/para/brain/hooks/on-tool-result.sh",
    "Stop": "/caminho/para/brain/hooks/on-session-end.sh"
  }
}
```

### OpenCode

Configure via `opencode.json` do projeto consumidor:

```json
{
  "hooks": {
    "session-start": "/caminho/para/brain/hooks/on-session-start.sh",
    "tool-result": "/caminho/para/brain/hooks/on-tool-result.sh",
    "session-end": "/caminho/para/brain/hooks/on-session-end.sh"
  }
}
```

### Kiro

Hooks declarativos em `.kiro/hooks/*.json` (`version: "v1"`). Três arquivos coexistem:

| Arquivo | Tipo | Trigger | Papel |
|---------|------|---------|-------|
| `brain-session.json` | `command` | `UserPromptSubmit` + `Stop` | Captura automática (spool via `brain hook --event`) |
| `brain-learning.json` | `agent` | `Stop` | Lembra o agente de curar aprendizado durável via MCP `brain_store` antes de encerrar (skip se trivial) |
| `brain-prompt-optimizer.json` | `agent` | `UserPromptSubmit` | Ativa a skill `prompt-optimizer` via `disclose_context` antes de agir (dispensável em consulta somente-leitura) |

`brain hook` (command) faz spool; aprendizado curado é gravado pelo **agente**
via MCP `brain_store`, nunca via CLI. `setup kiro` gera **só** o command
(`brain-session.json`); os 2 agent-hooks são padrão do repo, copiados manualmente.

### Genérico (qualquer MCP cliente)

Use o comando `brain` para scripts manuais:

```bash
# Antes de codificar: buscar contexto
brain search "contexto do projeto" --top-k 5

# Depois de uma decisão: salvar
brain store arquitetura meu-projeto/decisao "# Decisão..."

# Ao finalizar sessão: salvar resumo
brain store sessoes meu-projeto/$(date +%Y-%m-%d) "## Sessão..."
```

## Variáveis de ambiente

| Variável | Default | Descrição |
|----------|---------|-----------|
| `BRAIN_URL` | `http://localhost:8321` | URL do brain server |
| `BRAIN_PROJECT` | `(detectado pelo git)` | Nome do projeto atual |
