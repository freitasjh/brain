---
name: prompt-optimizer
description: "Prompt analysis and refinement. Detects vague/ambiguous requests and asks clarifying questions BEFORE any work begins. Eliminates guesswork. Mandatory first step for orchestrator."
---

# Prompt Optimizer

Analisa prompt do usuário, detecta ambiguidades e faz perguntas clarificadoras ANTES de qualquer ação.

## Quando Usar

- **SEMPRE** no início de qualquer demanda (feature, bugfix, refactor)
- **ANTES** de carregar qualquer outra skill
- **ANTES** de delegar para qualquer subagente

## Regra Fundamental

> 🚨 **NUNCA adivinhar.** Se o prompt for vago, ambíguo ou sem contexto suficiente, PERGUNTAR antes de prosseguir.

## Fluxo

```
1. Receber prompt do usuário
2. Carregar skill "brain" → brain_search(prompt, top_k=3)
3. Analisar prompt contra critérios abaixo
4. Se vago → fazer perguntas clarificadoras
5. Se claro → prosseguir com workflow normal
```

## Critérios de Análise

### Prompt COMPLETO (pode prosseguir)

| Critério | Obrigatório? | Exemplo |
|----------|--------------|---------|
| **Objetivo claro** | ✅ SIM | "Criar endpoint de login" |
| **Contexto do módulo** | ✅ SIM | "No módulo identity" |
| **Escopo definido** | ⚠️ RECOMENDADO | "Apenas backend, sem frontend" |
| **Cenário de uso** | ⚠️ RECOMENDADO | "Para usuários que esqueceram a senha" |
| **Restrições conhecidas** | ❌ OPCIONAL | "Sem mudar banco existente" |

### Prompt VAGO (precisa perguntar)

| Sinal | Pergunta sugerida |
|-------|-------------------|
| "Melhorar o sistema" | "Qual parte especificamente? Backend, frontend, performance, segurança?" |
| "Corrigir o bug" | "Qual bug? Qual tela/fluxo? Qual erro exato?" |
| "Criar uma feature" | "O que essa feature faz? Quem usa? Qual fluxo?" |
| "Refatorar isso" | "O que exatamente precisa refatorar? Por quê?" |
| "Não tá funcionando" | "O que você tentou fazer? Qual erro apareceu?" |
| "Faz igual o outro" | "Qual módulo/feature de referência? O que especificamente copiar?" |

## Template de Perguntas

```markdown
## ❓ Preciso de mais contexto

Para prosseguir corretamente, preciso esclarecer:

1. **{PERGUNTA_1}**
2. **{PERGUNTA_2}**
3. **{PERGUNTA_3}**

### Contexto que já entendi:
- {contexto_identificado}

### O que NÃO entendi:
- {ambiguidades}

**Responda para que eu possa ajudar de forma precisa.**
```

## Integração com Brain

### ANTES de analisar prompt

```python
# 1. Buscar contexto similar
brain_search(query=prompt_usuario, top_k=3)

# 2. Verificar se há lições sobre prompts vagos
brain_search("prompt vago ambiguidade", layer="regras", scope="projetos", top_k=2)

# 3. Se encontrar contexto relevante, usar como base
```

### DEPOIS de refinar prompt

```python
# Salvar lição se pattern se repetir
brain_store(
    layer="regras",
    path="atlas-ecm/prompt-patterns/<pattern>",
    content="## Pattern: {pattern}\n\n### Pergunta que resolveu\n{pergunta}\n\n### Resposta do usuário\n{resposta}",
    scope="projetos"
)
```

## Classificação de Prompts

### Tier 1: Crítico (PARAR imediatamente)

Prompts que, se interpretados errados, geram retrabalho massivo.

**Exemplos:**
- "Mudar toda a arquitetura"
- "Reescrever o módulo X"
- "Deletar tudo e recomeçar"

**Ação:** Perguntar 3-5 vezes antes de qualquer ação.

### Tier 2: Ambíguo (PERGUNTAR antes de prosseguir)

Prompts com múltiplas interpretações possíveis.

**Exemplos:**
- "Melhorar o login" (performance? UX? segurança?)
- "Criar relatório" (que dados? que formato? que período?)
- "Corrigir a validação" (onde? qual regra? qual erro?)

**Ação:** Perguntar 1-3 vezes.

### Tier 3: claro (PROSSEGUIR)

Prompts com objetivo, contexto e escopo definidos.

**Exemplos:**
- "Adicionar campo 'phone' no DTO de response do módulo company"
- "Criar endpoint GET /internal/users com paginação"
- "Corrigir bug no login onde senha com caracteres especiais falha"

**Ação:** Prosseguir com workflow normal.

## Exemplo: Prompt Vago

**Usuário:** "Melhorar o sistema"

**Análise:**
```
Objetivo: ❌ NÃO definido ("melhorar" é vago)
Módulo: ❌ NÃO especificado
Escopo: ❌ NÃO definido
Cenário: ❌ NÃO informado
```

**Resposta do agente:**
```markdown
## ❓ Preciso de mais contexto

Para "melhorar o sistema", preciso entender:

1. **Qual parte do sistema?** Backend, frontend, ambos?
2. **Que tipo de melhoria?** Performance, UX, segurança, código?
3. **Que módulo?** Identity, company, document, search?
4. **Que problema você está enfrentando?** Algo específico está ruim?

### Contexto que já tenho:
- Projeto Atlas ECM (Java + Vue 3)
- Módulos: identity, company, document, search

### O que NÃO entendi:
- "Melhorar" pode significar milhares de coisas
- Sem escopo, posso gastar tokens à toa

**Responda para que eu possa ajudar de forma precisa.**
```

## Exemplo: Prompt Claro

**Usuário:** "Adicionar validação de email no endpoint POST /internal/registrations do módulo company"

**Análise:**
```
Objetivo: ✅ Validação de email
Módulo: ✅ Company
Endpoint: ✅ POST /internal/registrations
Escopo: ✅ Apenas validação
```

**Ação:** Prosseguir com workflow SDD.

## Anti-Patterns

| ❌ Errado | ✅ Certo |
|-----------|----------|
| Adivinhar e implementar | Perguntar e confirmar |
| "Vou assumir que você quer X" | "Você quer X ou Y?" |
| Delegar com contexto vago | Refinar prompt antes de delegar |
| Pular brain_search | SEMPRE buscar contexto primeiro |

## Métricas de Qualidade

Após refinar prompt, verificar:

- [ ] Objetivo definido? (O quê exatamente?)
- [ ] Módulo/área definido? (Onde no sistema?)
- [ ] Escopo definido? (Backend/frontend/ambos?)
- [ ] Restrições conhecidas? (O que NÃO fazer?)
- [ ] Critérios de aceite? (Como saber que está pronto?)

Se qualquer ✅ obrigatório estiver faltando → PERGUNTAR.
