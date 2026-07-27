---
name: brain_store
description: >
  Save a note to the Brain MCP server vault. Use AFTER learning something
  important — a decision, a rule, an architecture choice, a session summary.
---

# brain_store

Salva uma nota markdown no vault do cérebro. O conteúdo é automaticamente
indexado para busca semântica por outros agentes.

## Quando usar

Use **depois** de:
- Tomar uma decisão arquitetural
- Descobrir uma regra de negócio
- Resolver um bug complexo
- Finalizar uma sessão (handoff para outro agente)
- Aprender uma convenção do projeto

## Como usar

```
brain_store(
    layer="arquitetura",
    path="meu-projeto/decisao-db",
    content="# Decisão: PostgreSQL\n\n## Contexto\n...",
)
```

## Camadas

| Camada | Quando usar |
|--------|-------------|
| `arquitetura` | Estrutura do projeto, módulos, stacks, dependências |
| `regras` | Regras de negócio, constraints, convenções |
| `sessoes` | Resumo de sessão, handoff entre agentes |
| `projetos` | Metadados e contexto do projeto |
| `indexacao` | Uso interno do sistema |

## Exemplos

```
# Decisão arquitetural
brain_store("arquitetura", "meu-projeto/por-que-postgres",
            "# Por que PostgreSQL\n\n## Motivo\n...")

# Regra de negócio
brain_store("regras", "meu-projeto/calc-frete",
            "## Cálculo de frete\n\n...")

# Handoff de sessão
brain_store("sessoes", "meu-projeto/2026-07-13",
            "## Sessão\n\nFeature: auth\nStatus: tests failing\n...")
```
