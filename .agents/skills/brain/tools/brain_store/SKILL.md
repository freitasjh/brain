---
name: brain_store
description: >
  Save a markdown note to the Brain, auto-indexed for search. Use AFTER learning
  something worth keeping — a decision, a rule, an architecture choice, a session
  handoff. Text search works immediately; the vector arrives a moment later.
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

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `layer` | ✅ | — | Uma das 6 camadas da tabela abaixo |
| `path` | ✅ | — | Relativo, **sem** `.md`, **sem** `/` no começo, **sem** `..` |
| `content` | ✅ | — | Markdown. Frontmatter `tags:` no topo melhora a busca |
| `scope` | **condicional** | — | **Obrigatório** em `arquitetura`, `regras` e `estudos`. `projetos` ou `global` |
| `project` | ❌ | `null` | Nome do projeto. **Criado automaticamente se não existir** |
| `tags` | ❌ | `[]` | Lista de tags. Também pelo frontmatter `tags:` |
| `pinned` | ❌ | `false` | Sobe o score da busca (`+0.10`). **Não impede o sweep** |
| `expires_at` | ❌ | `null` | RFC3339. Depois disso a nota some da busca e do sweep apaga |

Escopos inválidos são recusados (`INVALID_PARAMS: Invalid scope …`), e um
`scope` faltando numa camada que exige um dá `scope required for {layer}` — a
chamada **não** grava nada (`brain-mcp:122-131`).

## Camadas

| Camada | `scope` | Quando usar |
|--------|---------|-------------|
| `arquitetura` | **obrigatório** | Estrutura do projeto, módulos, stacks, dependências |
| `regras` | **obrigatório** | Regras de negócio, constraints, convenções |
| `estudos` | **obrigatório** | Estudo completo de um tema |
| `sessoes` | não usa | Resumo de sessão, handoff entre agentes |
| `projetos` | não usa | Metadados e contexto do projeto |
| `indexacao` | não usa | Uso interno do sistema |

Os três com scope existem para **não vazar contexto entre projetos**: é o que
separa `global` (universal) de `projetos` (só o seu).

## Como usar

```python
# Decisão arquitetural — scope obrigatório
brain_store(
    layer="arquitetura",
    path="meu-projeto/por-que-postgres",
    content="# Por que PostgreSQL\n\n## Motivo\n...",
    scope="projetos",
)

# Regra universal
brain_store(
    layer="regras",
    path="coding-standards",
    content="## Padrões\n\n- Nunca usar SELECT *\n...",
    scope="global",
)

# Handoff — sessoes não usa scope
brain_store(
    layer="sessoes",
    path="meu-projeto/2026-09-27",
    content="## Sessão\n\nFeature: auth\nStatus: tests failing\n...",
)
```

## Retorno

```json
{"ok": true, "path": "regras/projetos/meu-projeto/naming",
 "chunks": 4, "embedded": 0, "without_embedding": 4, "queued": 4}
```

| Campo | Significado |
|-------|-------------|
| `path` | o path **completo** montado pela tool — é o que `brain_read`/`brain_delete` aceitam |
| `chunks` | chunks que a nota gerou (1 por seção `## `) |
| `embedded` | chunks **já** com vetor — 0 numa nota nova |
| `without_embedding` | chunks `NULL`, aguardando vetor |
| `queued` | dívida enfileirada para o embed em background |

`queued > 0` **não é erro**. A nota já está persistida e pesquisável por
palavra-chave. Para conferir o índice semântico, leia
`brain_status.embedding.coverage.embedding_coverage_pct` — é ele que diz se a
fila está andando.

## Limites (rejeição antes de qualquer embed)

| Limite | Valor | Exceder |
|--------|-------|---------|
| `MAX_CONTENT_BYTES` | 256 KiB | `INVALID_PARAMS: content too large` |
| `MAX_CHUNKS` | 64 (uma seção `## ` = 1 chunk) | `INVALID_PARAMS: content splits into N chunks` |

Motivo: o Ollama embede **um chunk por vez** (~0.045 s cada), então o custo de
escrever escalava com o número de chunks sem teto. Se bater o limite, divida a
nota em várias sob o mesmo projeto, ou una as seções pequenas. A rejeição é
instantânea e não deixa a nota gravada.

## Reescrever é normal — e versiona

`brain_store` no **mesmo** `layer`/`path`/`scope` **sobrescreve** a nota, e a
versão anterior vai para o audit log (`brain-store:1004`). Então o fluxo "corrigi,
salvo de novo" é o fluxo certo.

Mas: `brain_store` **substitui o conteúdo inteiro**, não faz merge. Se você
quer acrescentar uma seção, mande o documento completo de novo. E uma leitura
velha como base sobrescreve o que outro agente escreveu no meio-tempo — releia
antes.

## O que NÃO fazer

- **Não esqueça o `scope` em `arquitetura`/`regras`/`estudos`.** A chamada é
  recusada inteira; nada é gravado. É o erro mais comum desta tool.
- **Não use `pinned` como proteção contra o sweep.** `pinned` só sobe o score.
  Quem apaga nota vencida é `expires_at`, e o TTL **vence** o pin.
- **Não confie que `project=` valida o nome.** O projeto é **criado se não
  existir** (`brain-mcp:183-188`), então um typo cria um projeto novo em silêncio
  e a nota fica invisível para quem filtra pelo nome certo. Confira com
  `brain_project_list`.
- **Não mande `content` gigante esperando o embed acompanhar.** Os limites de
  acima existem; acima deles a escrita seria minutos de embed pendente.
- **Não use para guardar segredo.** Não há auth nem criptografia: qualquer
  processo local com acesso ao `brain.db` e ao export lê tudo.
