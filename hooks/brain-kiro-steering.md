---
inclusion: always
---

# 🧠 Brain MCP — Memory & Context Rules

Este projeto usa o **Brain MCP Server** como cérebro central. O brain
mantém memória persistente entre sessões — decisões arquiteturais, regras
de negócio, resumos de sessões anteriores.

Você DEVE usar o brain ativamente durante a sessão. Não espere o agente
iniciar — a ferramenta está disponível e deve ser usada proativamente.

---

## Tools MCP disponíveis

### `brain_search(query, [layer], [scope], [top_k])`

Busca semântica por similaridade de embedding no vault do cérebro.
Resultados ordenados por score (0..1).

```
# Busca simples
brain_search("regras de banco de dados")

# Filtrar por camada
brain_search("naming conventions", layer="regras")

# Filtrar por scope (projetos ou global)
brain_search("padrões de código", scope="global")
brain_search("regras do meu projeto", scope="projetos")

# Combinar layer e scope
brain_search("arquitetura do sistema", layer="arquitetura", scope="global")

# Controlar quantidade de resultados
brain_search("arquitetura do sistema", top_k=3)
```

**Parâmetros:**
| Nome | Tipo | Obrigatório | Default | Descrição |
|------|------|-------------|---------|-----------|
| `query` | string | ✅ | — | Texto da busca semântica |
| `layer` | string | ❌ | `null` | Filtrar por camada |
| `scope` | string | ❌ | `null` | Filtrar por scope (`projetos` ou `global`) |
| `top_k` | integer | ❌ | `5` | Máximo de resultados (1–20) |

### `brain_store(layer, path, content, [scope])`

Salva uma nota markdown no vault. O conteúdo é automaticamente indexado
para busca semântica por embedding.

```
# Salvar decisão arquitetural (scope obrigatório para arquitetura/regras)
brain_store(
    layer="arquitetura",
    path="meu-projeto/decisao-db",
    content="# Decisão: PostgreSQL\n\n## Contexto\nPrecisamos de um banco relacional...",
    scope="projetos"  # específico do projeto
)

# Salvar regra de negócio (scope obrigatório)
brain_store(
    layer="regras",
    path="meu-projeto/naming-conventions",
    content="## Nomes de tabela\nTabelas em snake_case plural...",
    scope="projetos"  # específico do projeto
)

# Salvar lição global (compartilhada entre todos os projetos)
brain_store(
    layer="regras",
    path="coding-standards",
    content="## Padrões universais\n\n- Nunca usar SELECT *",
    scope="global"  # compartilhado
)
```

**Parâmetros:**
| Nome | Tipo | Obrigatório | Descrição |
|------|------|-------------|-----------|
| `layer` | string | ✅ | Camada do vault (`arquitetura`, `regras`, `sessoes`, `projetos`) |
| `path` | string | ✅ | Path relativo sem `.md` (ex: `"meu-app/stack"`) |
| `content` | string | ✅ | Conteúdo markdown |
| `scope` | string | ⚠️ | **Obrigatório para `arquitetura` e `regras`**: `projetos` ou `global` |

### `brain_read(layer, path, [scope])`

Lê uma nota completa do vault.

```
# Ler nota com scope (obrigatório para arquitetura/regras)
brain_read("regras", "meu-projeto/naming-conventions", scope="projetos")
# Retorna o conteúdo markdown completo com metadados

# Ler nota global
brain_read("regras", "coding-standards", scope="global")
```

**Parâmetros:**
| Nome | Tipo | Obrigatório | Descrição |
|------|------|-------------|-----------|
| `layer` | string | ✅ | Camada do vault |
| `path` | string | ✅ | Path relativo sem `.md` |
| `scope` | string | ⚠️ | **Obrigatório para `arquitetura` e `regras`**: `projetos` ou `global` |

### `brain_reindex([all], [layer], [path])`

Reconstrói o índice de embeddings (parcial ou total). Use quando
arquivos foram alterados manualmente.

```
# Reindexar tudo
brain_reindex(all=True)

# Reindexar apenas uma camada
brain_reindex(layer="regras")

# Reindexar arquivo específico
brain_reindex(path="regras/meu-projeto/naming-conventions")
```

**Parâmetros:**
| Nome | Tipo | Obrigatório | Default | Descrição |
|------|------|-------------|---------|-----------|
| `all` | boolean | ❌ | `false` | Reindexar todos os arquivos |
| `layer` | string | ❌ | `null` | Reindexar apenas esta camada |
| `path` | string | ❌ | `null` | Reindexar apenas este arquivo |

---

## Regras obrigatórias

### 🔴 Regra 1: SEMPRE busque contexto ANTES de codificar

Antes de iniciar QUALQUER implementação, chame `brain_search` com o nome
da feature, tecnologia ou módulo para carregar contexto relevante.

```
brain_search("modulo de pagamento")
brain_search("regra de negocio", layer="regras", scope="projetos")
brain_search("arquitetura do sistema", layer="arquitetura", scope="projetos", top_k=3)
brain_search("padrões de código", scope="global")  # lições universais
```

**Motivação**: o brain contém decisões arquiteturais, regras de negócio e
convenções de sessões anteriores. Ignorar o contexto gera retrabalho.

### 🔴 Regra 2: SEMPRE registre decisões importantes APÓS decisões

Toda decisão arquitetural ou regra de negócio descoberta DEVE ser salva:

```
# Específica do projeto
brain_store(
  layer="arquitetura",
  path="app/database",
  content="# Banco\n\n## Decisao\nPostgreSQL 15\n\n## Motivo\n...",
  scope="projetos"
)

# Universal (compartilhada)
brain_store(
  layer="regras",
  path="coding-standards",
  content="## Padrões\n\n- Nunca usar SELECT *",
  scope="global"
)
```

### 🔴 Regra 2.1: Escolha o scope correto

**Nunca** salve em `global` sem verificar se é realmente universal.

Pergunte: "Essa regra/lição se aplica a **TODOS** os projetos ou só ao meu?"
- Se só ao meu → `scope="projetos"`
- Se a todos → `scope="global"`

### 🔴 Regra 3: SEMPRE salve resumo AO FINALIZAR sessão

```
brain_store(
  layer="sessoes",
  path="app/2026-07-22",
  content="## Sessao\n\n### Features\n- Login OAuth2\n\n### Decisoes\n- ..."
)
```

### 🔴 Regra 4: SALVE ESTUDOS COMPLETOS (não resumos)

Quando você aprender algo novo (ex: estudar Java, Docker, padrões), salve o **CONTEÚDO COMPLETO** na camada `estudos`, com tags no topo:

```
brain_store(
  layer="estudos",
  path="java/orientacao-objetos",
  content="---
tags: [java, oo, fundamentos]
nivel: iniciante
---

# Java OO — Estudo Completo

## Conceitos
... (conteudo completo com codigo, exemplos) ...
",
  scope="global"
)
```

**Nunca** salve estudo como resumo de sessão — perde o conhecimento.
Use `estudos` com conteúdo completo + tags para busca semântica.

### 🟡 Regra 5: Use as camadas e scopes corretos

| Camada | Scope | Conteúdo |
|--------|-------|----------|
| `arquitetura` | `projetos` | Stack, módulos, decisões específicas do projeto |
| `arquitetura` | `global` | Padrões universais, melhores práticas |
| `regras` | `projetos` | Regras de negócio, nomenclatura, workflow específico |
| `regras` | `global` | Lições aprendidas, padrões de código compartilhados |
| `estudos` | `projetos` | Estudos completos específicos do projeto |
| `estudos` | `global` | Conhecimento geral (Java, Docker, padrões) |
| `sessoes` | — | Resumo de sessões, handoff entre agentes |
| `projetos` | — | Metadados e visão geral do projeto |

**Scope obrigatório** para `arquitetura`, `regras` e `estudos`. Previne vazamento de contexto.

### 🟡 Regra 6: Verifique duplicatas antes de salvar

Sempre busque antes de criar:

```
brain_search("decisao banco de dados", layer="arquitetura")
```

---

## Fluxo recomendado

```
1. RECEBE TAREFA: "implementar feature X"
2. BUSCA CONTEXTO: brain_search("feature X", ...)  ← REGRA 1
3. IMPLEMENTA: codifica a feature
4. REGISTRA DECISÕES: brain_store(...)              ← REGRA 2
5. SE APRENDEU ALGO: brain_store estudos completo   ← REGRA 4
6. FINALIZA: brain_store resumo da sessão           ← REGRA 3
```

## Handoff entre agentes

Quando um agente repassa contexto para outro:

```
# Agente A — salva resumo
brain_store("sessoes", "app/2026-07-22",
  "## Sessao\n\nStatus: 80%\nProximo: testes")
```
