---
name: brain_store
description: >
  Save a note to the Brain MCP server vault. Use AFTER learning something
  important — a decision, a rule, an architecture choice, a session summary.
---

# brain_store

Salva uma nota markdown no cérebro. O índice de **palavras-chave (FTS5) fica
pronto** quando a chamada retorna; o **vetor** (busca semântica) chega em
background.

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

## Retorno

```json
{"ok": true, "path": "regras/global/meu-projeto/naming",
 "chunks": 4, "embedded": 0, "without_embedding": 4, "queued": 4}
```

| Campo | Significado |
|-------|-------------|
| `chunks` | chunks que a nota gerou (1 por secao `## `) |
| `embedded` | chunks **ja** com vetor — 0 numa nota nova |
| `without_embedding` | chunks `NULL`, aguardando vetor |
| `queued` | divida enfileirada para o embed em background |

`queued > 0` e normal e nao e erro: a nota ja esta persistida e pesquisavel por
palavra-chave. Para conferir o indice semantico, leia
`brain_status.embedding.coverage.embedding_coverage_pct` — e ele que diz se a fila
esta andando.

## Limites (rejeicao antes de qualquer embed)

| Limite | Valor | Exceder |
|--------|-------|---------|
| `MAX_CONTENT_BYTES` | 256 KiB | `INVALID_PARAMS: content too large` |
| `MAX_CHUNKS` | 64 (uma secao `## ` = 1 chunk) | `INVALID_PARAMS: content splits into N chunks` |

Motivo: o Ollama embede **um chunk por vez** (~0.045 s cada), entao o custo de
escrever escalava com o numero de chunks sem teto. Se bater o limite, divida a
nota em varias sob o mesmo projeto, ou una as secoes pequenas. A rejeicao e
instantanea e nao deixa a nota gravada.
