# 🧠 Brain MCP — Instruções para o GitHub Copilot

Você TEM acesso ao **Brain MCP Server** — um cérebro central com memória
persistente de todas as sessões, decisões e regras dos projetos.

Você DEVE usar o brain ativamente durante a sessão. Não espere o usuário
pedir — a ferramenta está disponível e deve ser usada proativamente.

---

## Tools MCP disponíveis

### `brain_search(query, [layer], [scope], [top_k])`

Busca semântica por similaridade de embedding no vault do cérebro.
Resultados ordenados por score (0..1).

```markdown
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

```markdown
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

```markdown
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

```markdown
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

## 📋 Regras obrigatórias

### 🔴 Regra 1: ANTES de codificar — BUSQUE CONTEXTO

**Sempre** que o usuário pedir uma implementação, modificação ou correção:

1. Chame `brain_search` com o nome da feature/módulo
2. Chame `brain_search` filtrando por `layer="arquitetura"` e `scope="projetos"` para decisões técnicas
3. Chame `brain_search` filtrando por `layer="regras"` e `scope="projetos"` para regras de negócio
4. Chame `brain_search` com `scope="global"` para lições universais

```markdown
brain_search("login OAuth2")
brain_search("arquitetura do sistema", layer="arquitetura", scope="projetos")
brain_search("regras de segurança", layer="regras", scope="projetos")
brain_search("padrões de código", scope="global")  # lições universais
```

**Nunca pule este passo.** O brain pode conter decisões ou regras que
evitam retrabalho e inconsistências.

### 🔴 Regra 2: APÓS decisões — REGISTRE

Sempre que você ou o usuário tomarem uma decisão importante:

```markdown
# Específica do projeto
brain_store(
    layer="arquitetura",
    path="app/banco-de-dados",
    content="# Banco de Dados\n\n## Decisão\nPostgreSQL 15\n\n## Motivo\n...",
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

### 🔴 Regra 3: AO FINALIZAR — RESUMO DA SESSÃO

Salve um resumo do que foi feito, decisões e próximos passos na camada
`sessoes`:

```markdown
brain_store(
    layer="sessoes",
    path="app/2026-07-22",
    content="## Sessão 2026-07-22\n\n### Features\n- Implementado login OAuth2\n\n### Decisões\n- ...",
)
```

### 🔴 Regra 4: SALVE ESTUDOS COMPLETOS (não resumos)

Quando você aprender algo novo (estudar tecnologia, framework, conceito), salve o **CONTEÚDO COMPLETO** na camada `estudos` com tags:

```markdown
brain_store(
    layer="estudos",
    path="java/orientacao-objetos",
    content="---
tags: [java, oo, fundamentos]
nivel: iniciante
---

# Java OO — Estudo Completo

## Conceitos
... (conteudo completo com codigo, exemplos, referencias) ...
",
    scope="global"
)
```

**Nunca** salve estudo em `sessoes` como resumo — isso perde o conhecimento.
Use `estudos` com conteúdo completo + frontmatter com tags para busca semântica.

### 🟡 Regra 5: Camadas e scopes corretos

| Camada | Scope | Uso |
|--------|-------|----------|
| `arquitetura` | `projetos` | Stack, módulos, decisões específicas do projeto |
| `arquitetura` | `global` | Padrões universais, melhores práticas |
| `regras` | `projetos` | Regras de negócio, naming, fluxos específicos |
| `regras` | `global` | Lições aprendidas, padrões de código compartilhados |
| `estudos` | `projetos` | Estudos completos específicos do projeto |
| `estudos` | `global` | Conhecimento geral (Java, Docker, padrões) |
| `sessoes` | — | Resumo de sessão, handoff |
| `projetos` | — | Visão geral do projeto |

**Scope obrigatório** para `arquitetura`, `regras` e `estudos`. Previne vazamento de contexto.

### 🟡 Regra 6: Evite duplicatas

Antes de salvar, busque para ver se já existe:

```markdown
brain_search("banco de dados", layer="arquitetura")
```

---

## 🔄 Fluxo de trabalho completo

```
1. USUÁRIO PEDE: "implementar login OAuth2"
2. VOCÊ BUSCA: brain_search("login")          ← REGRA 1
3. VOCÊ IMPLEMENTA: código
4. VOCÊ REGISTRA: brain_store("arquitetura")   ← REGRA 2
5. SE APRENDEU: brain_store("estudos")         ← REGRA 4 (completo)
6. VOCÊ FINALIZA: brain_store("sessoes")       ← REGRA 3
```

## 💡 Dica: carregue no início da sessão

Ao iniciar uma sessão com um projeto, carregue o contexto principal:

```markdown
brain_search("projeto", layer="arquitetura", top_k=5)
brain_search("projeto", layer="regras", top_k=5)
```
