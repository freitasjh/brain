---
name: brain_search
description: >
  Search the Brain MCP server by semantic similarity. Use BEFORE starting
  any coding task to load relevant context from past sessions.
---

# brain_search

Busca notas no cérebro por similaridade semântica (embedding).

## Quando usar

**SEMPRE** antes de começar a codificar. O cérebro pode ter informações sobre:
- Regras de negócio do projeto
- Decisões arquiteturais passadas
- Sessões anteriores de outros agentes
- Estrutura e convenções do projeto

## Como usar

```
brain_search("regras de banco de dados")
brain_search("naming conventions", layer="regras")
brain_search("arquitetura do sistema", top_k=3)
```

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `query` | ✅ | — | Texto da busca |
| `layer` | ❌ | `null` | Filtrar camada |
| `top_k` | ❌ | `5` | Máx resultados |

## Exemplo de fluxo

```
# 1. Buscar contexto geral
brain_search("<feature-name>")

# 2. Buscar regras específicas
brain_search("<feature-name>", layer="regras")

# 3. Buscar arquitetura
brain_search("<feature-name>", layer="arquitetura")
```
