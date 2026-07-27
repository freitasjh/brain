# BRAIN.MCP.md — Regras de uso do Brain MCP

Este arquivo define como os agentes de IA devem usar o **Brain MCP Server**
— o cérebro central com memória persistente entre sessões e projetos.

## 📋 Como incluir no seu projeto

### Opção 1 — Referência no `AGENTS.md`

No `AGENTS.md` do seu projeto, adicione:

```markdown
## Brain MCP

Este projeto usa um cérebro central (Brain MCP Server) para memória
persistente entre sessões de IA.

Regras de uso: `.agents/rules/BRAIN.MCP.md`
```

### Opção 2 — Via `opencode.json` / `kiro.json`

```json
{
  "instructions": [
    ".agents/rules/*.md",
    "../brain/.agents/skills/brain/SKILL.md"
  ],
  "mcpServers": {
    "brain": {
      "transport": "sse",
      "url": "http://localhost:8321/sse"
    }
  }
}
```

---

## 🧠 Ferramentas MCP disponíveis

Quando o Brain MCP está configurado, os agentes têm acesso a estas tools:

| Tool | Descrição | Quando usar |
|------|-----------|-------------|
| `brain_search(query, layer?, scope?, top_k?)` | Busca semântica por embedding | **ANTES** de codificar |
| `brain_store(layer, path, content, scope?)` | Salva nota markdown + indexa | **APÓS** decisões |
| `brain_read(layer, path, scope?)` | Lê nota completa do vault | Quando precisar do conteúdo completo |
| `brain_reindex(all?, layer?, path?)` | Reconstrói índice vetorial | Raramente — após alterações manuais |

### Parâmetros

#### `brain_search`
| Parâmetro | Tipo | Obrigatório | Default | Descrição |
|-----------|------|-------------|---------|-----------|
| `query` | string | ✅ | — | Texto da busca semântica |
| `layer` | string | ❌ | `null` | Filtrar por camada |
| `scope` | string | ❌ | `null` | Filtrar por scope (`projetos` ou `global`) |
| `top_k` | integer | ❌ | `5` | Máximo de resultados |

#### `brain_store`
| Parâmetro | Tipo | Obrigatório | Default | Descrição |
|-----------|------|-------------|---------|-----------|
| `layer` | string | ✅ | — | Camada do vault |
| `path` | string | ✅ | — | Path relativo sem `.md` |
| `content` | string | ✅ | — | Conteúdo markdown |
| `scope` | string | ⚠️ | — | **Obrigatório para `arquitetura` e `regras`**: `projetos` ou `global` |

---

## 📂 Camadas do vault com Scope

As camadas `arquitetura` e `regras` e `estudos` usam **scope** para separar conteúdo:

| Camada | Scope | Conteúdo | Exemplo |
|--------|-------|----------|---------|
| `arquitetura` | `projetos` | Stack, módulos, decisões específicas do projeto | `brain_store("arquitetura", "app/stack", "# Stack...", scope="projetos")` |
| `arquitetura` | `global` | Padrões universais, melhores práticas | `brain_store("arquitetura", "patterns", "# Patterns...", scope="global")` |
| `regras` | `projetos` | Regras de negócio específicas do projeto | `brain_store("regras", "app/naming", "## Naming...", scope="projetos")` |
| `regras` | `global` | Lições aprendidas, padrões de código | `brain_store("regras", "coding-standards", "## Padrões...", scope="global")` |
| `estudos` | `projetos` | Estudos completos específicos do projeto | `brain_store("estudos", "app/analise-db", "# Estudo...", scope="projetos")` |
| `estudos` | `global` | Conhecimento geral (ex: Java, Docker) | `brain_store("estudos", "java/oo", "# Java OO...", scope="global")` |
| `sessoes` | — | Resumo de sessões, handoff entre agentes | `brain_store("sessoes", "app/2026-07-21", "## Sessão...")` |
| `projetos` | — | Metadados e visão geral do projeto | `brain_store("projetos", "app/visao-geral", "# App...")` |

**Regra**: Scope é **obrigatório** para `arquitetura`, `regras` e `estudos`. Previne vazamento de contexto entre projetos.

---

## ⚠️ Regras obrigatórias para agentes

### 🔴 Regra 1: SEMPRE busque contexto antes de codificar

**Todo** agente, **antes** de iniciar qualquer implementação, DEVE chamar
`brain_search` para carregar contexto relevante.

```python
# Ao receber uma tarefa como "implementar módulo de pagamento":
brain_search("módulo de pagamento")
brain_search("pagamento", layer="regras", scope="projetos")
brain_search("arquitetura do sistema", layer="arquitetura", scope="projetos", top_k=3)
brain_search("padrões de código", scope="global")  # lições universais
```

**Motivação**: o brain pode conter decisões arquiteturais, regras de negócio
e convenções de projetos anteriores que evitam retrabalho e inconsistências.

### 🔴 Regra 2: SEMPRE registre decisões importantes

**Toda** decisão arquitetural ou regra de negócio descoberta DEVE ser
registrada no brain imediatamente.

```python
# Após decidir usar PostgreSQL (específico do projeto):
brain_store(
    layer="arquitetura",
    path="app/database",
    content="# Banco de Dados\n\n## Decisão\nPostgreSQL 15\n\n## Motivo\n...",
    scope="projetos",  # específico do projeto
)

# Após descobrir uma regra de negócio (específica do projeto):
brain_store(
    layer="regras",
    path="app/calculo-frete",
    content="## Cálculo de frete\n\nRegras:\n- Frete grátis acima de R$ 200\n- ...",
    scope="projetos",  # específica do projeto
)

# Após descobrir um padrão universal (compartilhado):
brain_store(
    layer="regras",
    path="coding-standards",
    content="## Padrões universais\n\n- Nunca usar SELECT *\n- Sempre validar input...",
    scope="global",  # compartilhado entre todos os projetos
)
```

### 🔴 Regra 2.1: Escolha o scope correto

**Nunca** salve em `global` sem verificar se é realmente universal.

Pergunte: "Essa regra/lição se aplica a **TODOS** os projetos ou só ao meu?"
- Se só ao meu → `scope="projetos"`
- Se a todos → `scope="global"`

**Exemplos:**
- ✅ `global/`: "Nunca usar SELECT * em produção", "Sempre validar input do usuário"
- ✅ `projetos/`: "Tabela de usuários tem campo `tenant_id`", "API usa paginação offset"

### 🔴 Regra 3: SEMPRE salve resumo ao finalizar sessão

Ao finalizar uma sessão de trabalho (ou quando perceber que vai perder o
contexto), salve um resumo no brain.

```python
brain_store(
    layer="sessoes",
    path="app/2026-07-21",
    content="""## Sessão 2026-07-21

### Features trabalhadas
- Módulo de pagamento com Stripe

### Decisões
- Usar stripe SDK v7
- Webhook em /api/v1/webhooks/stripe

### Próximos passos
- Implementar fluxo de reembolso
- Testes de integração

### Problemas encontrados
- Idempotency key necessária para evitar duplicatas
""",
)
```

### 🔴 Regra 4: SALVE ESTUDOS COMPLETOS, não apenas resumos

**DIFERENTE de resumo de sessão.** Quando você ou o usuário estudar/aprender algo (ex: "aprender Java", "estudar Docker"), salve o **CONTEÚDO COMPLETO** na camada `estudos`, não apenas um resumo.

```python
# ✅ CERTO: salva o estudo completo com tags
brain_store(
    layer="estudos",
    path="java/orientacao-objetos",
    content="""---
tags: [java, oo, fundamentos]
topico: Java OO
nivel: iniciante
---

# Java OO — Estudo Completo

## Conceitos
... (conteúdo completo com código, exemplos, referências) ...
""",
    scope="global",  # conhecimento geral
)

# ❌ ERRADO: salvar apenas um resumo perde o conhecimento
brain_store("sessoes", "app/2026-07-25", "## Estudei Java OO hoje")
```

**Use o template no SKILL.md para estrutura completa com tags, código e referências.**

Sempre inclua frontmatter com `tags:` no topo do conteúdo para melhorar a busca semântica.

### 🟡 Regra 6: Organização consistente

Use sempre as camadas corretas:

- `arquitetura` → estrutura do projeto, stacks, decisões técnicas
- `regras` → regras de negócio, convenções de código, workflows
- `estudos` → estudos completos, aprendizado (ex: Java, Docker)
- `sessoes` → resumos de sessão, handoffs
- `projetos` → visão geral, metadados

Nomeie os paths como `projeto-nome/assunto` (kebab-case).

### 🟡 Regra 7: Verifique antes de duplicar

Antes de salvar uma nota, faça uma busca rápida para evitar duplicatas:

```python
# Verificar se já existe antes de criar
brain_search("decisão banco de dados", layer="arquitetura")
```

---

## 💡 Boas práticas

### Fluxo recomendado para cada tarefa

```
1. RECEBE TAREFA: "implementar feature X"
2. BUSCA CONTEXTO: brain_search("X", ...)  ← REGRA 1
3. IMPLEMENTA: codifica a feature
4. REGISTRA DECISÕES: brain_store(...)      ← REGRA 2
5. SE APRENDEU ALGO: brain_store estudos    ← REGRA 4 (completo, não resumo)
6. FINALIZA: brain_store resumo da sessão   ← REGRA 3
```

### Handoff entre agentes

Quando um agente precisa passar o contexto para outro:

```python
# Agente A — salva resumo
brain_store(
    "sessoes", "app/2026-07-21",
    "## Sessão\n\nStatus: 80% completo\nPróximo: testes de integração",
)

# Agente B — carrega contexto antes de começar
brain_search("app", layer="sessoes", top_k=3)
brain_search("app", layer="arquitetura", top_k=5)
```

### Cache local (opcional)

Para evitar chamadas repetidas ao brain durante uma sessão:

```python
# Ao iniciar sessão, carregue todo contexto relevante de uma vez
ctx_arquitetura = brain_search("app", layer="arquitetura", top_k=10)
ctx_regras = brain_search("app", layer="regras", top_k=10)
# Use os resultados em memória durante a sessão
```

---

## 🔧 Exemplo completo de AGENTS.md

```markdown
# AGENTS.md — Meu Projeto

## Stack
- Backend: Python FastAPI + PostgreSQL
- Frontend: React + Next.js

## Brain MCP

Este projeto usa o [Brain MCP Server](../brain/) para memória persistente.

### Configuração MCP

O servidor brain roda em modo SSE em `http://localhost:8321/sse`.

### Regras de uso obrigatórias

1. **SEMPRE** busque contexto no brain antes de codificar (`brain_search`)
2. **SEMPRE** registre decisões no brain (`brain_store`)
3. **SEMPRE** salve resumo ao finalizar sessão (`brain_store` na camada `sessoes`)
4. Use camadas: `arquitetura`, `regras`, `sessoes`, `projetos`
5. Paths em kebab-case: `meu-projeto/assunto`

### Tools disponíveis

- `brain_search(query, layer?, top_k?)` → busca semântica
- `brain_store(layer, path, content)` → salva nota
- `brain_read(layer, path)` → lê nota
- `brain_reindex(all?, layer?, path?)` → reconstrói índice

### Skill

Para instruções detalhadas, veja:
[`../brain/.agents/skills/brain/SKILL.md`](../brain/.agents/skills/brain/SKILL.md)
```

---

## ✅ Checklist para agentes

Antes de dar uma tarefa como concluída, verifique:

- [ ] Busquei contexto no brain antes de começar? (`brain_search`)
- [ ] Registrei decisões importantes? (`brain_store` em `arquitetura`/`regras`)
- [ ] Salvei estudos completos com tags? (`brain_store` em `estudos` com frontmatter)
- [ ] Salvei resumo da sessão? (`brain_store` em `sessoes`)
- [ ] Usei as camadas corretas?
- [ ] Paths estão em kebab-case?
- [ ] Evitei duplicatas? (busquei antes de salvar)
