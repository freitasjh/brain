---
name: brain_read
description: >
  Read a specific note from the Brain MCP server vault. Use when you know
  the exact path from a brain_search result.
---

# brain_read

Lê uma nota completa do vault do cérebro.

## Quando usar

Use quando `brain_search` retornar um resultado e você precisar do
conteúdo completo, não apenas do snippet.

## Como usar

```
brain_read("regras", "meu-projeto/naming-conventions")
```

## Parâmetros

| Nome | Obrigatório | Descrição |
|------|-------------|-----------|
| `layer` | ✅ | Camada da nota |
| `path` | ✅ | Path relativo sem extensão |

## Exemplo de fluxo

```
# 1. Buscar
results = brain_search("naming conventions", layer="regras")

# 2. Se achar, ler o conteúdo completo
brain_read(results[0].layer, results[0].path.replace(".md", ""))
```
