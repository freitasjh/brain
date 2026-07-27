---
name: brain_reindex
description: >
  Rebuild the Brain MCP server vector index. Use when the index is out of
  sync with the vault (e.g. after manual file changes).
---

# brain_reindex

Reconstrói o índice de embeddings do cérebro.

## Quando usar

Use **raramente**, apenas quando:
- Notas foram alteradas manualmente no Obsidian
- O índice foi corrompido ou deletado
- `brain_search` não retorna resultados esperados

## Como usar

```
# Reindexar tudo
brain_reindex(all=True)

# Apenas uma camada
brain_reindex(layer="regras")

# Apenas um arquivo
brain_reindex(path="regras/meu-projeto/naming-conventions")
```

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `all` | ❌ | `false` | Reindexar tudo |
| `layer` | ❌ | `null` | Reindexar camada |
| `path` | ❌ | `null` | Reindexar arquivo |
