# Guia de Uso — Skill Spec Review (TaskFlow)

Este guia descreve como usar a skill `spec-review` para revisar specifications do **TaskFlow** (Spring Boot 3.5 + Vue 3).

---

## Quick Start

### 1 Invocar a Skill

```
Analise a specification em .spec/backend/task/ seguindo as regras do projeto.
```

Ou para uma análise completa com plano de ação:

```
Revise a specification em .spec/backend/project/SPEC.md:
- Verifique conformidade com DDD Modular e boas práticas TaskFlow
- Identifique gaps por severidade
- Gere um plano de ação detalhado
```

### 2 A Skill vai:

Ler a specification
Executar checklist (50+ itens)
Cruzar com regras canônicas do TaskFlow
Gerar relatório com gaps
Priorizar ações por severidade

### 3 Você recebe:

Scorecard de conformidade
Gaps críticos (bloqueadores)
Gaps de alta prioridade
Recomendações
Plano de ação

---

## Mapeamento: Tipo de Spec → Regras

Identifique o tipo de sua specification:

### Backend — Módulo de Domínio

**Quando:** api/impl/public em user, employee, project, team, task, workflow, kanban
**Regras aplicáveis:**
- `backend-coding-standards.md` — DDD modular, performance
- `backend-unit-tests.md` — Testes unitários
- `backend-integration-tests.md` — Testes de integração

**Exemplo de gaps comuns:**
- impl dependendo de impl de outro módulo
- Entidade JPA exposta via controller
- Falta de interface na camada api
- Falta de `@HasPermission("PERMISSION_NAME")` no controller

### Liquibase Migration

**Quando:** dbupdate em qualquer domínio
**Regras aplicáveis:**
- `db-migration-liquibase.md` — Versionamento, unicidade, estrutura XML

**Exemplo de gaps comuns:**
- Changeset sem include no master-changelog.xml
- Changeset alterado após execução em produção

### Frontend — Vue 3

**Quando:** Páginas, componentes, stores, services
**Regras aplicáveis:**
- `frontend-coding-standards.md` — Componentes, Pinia, Axios, ícones

**Exemplo de gaps comuns:**
- Store monolítica (não separada por domínio)
- Service sem padrão class-based
- Ícone mdi- usado (apenas ri- permitido)
- CSS não-scoped no style global

### Mista (Backend + Frontend)

**Regras:** Todas acima

---

## Estrutura do Relatório

### Resumo Executivo

```
Arquivo:        .spec/backend/task/SPEC.md
Tipo:           Backend (task/impl + task/public)
Dominio:        Task
Status:         Com ressalvas
Conformidade:   78%
```

### Scorecard

Tabela com conformidade por categoria:

| Categoria | Conformidade | Status |
|-----------|--------------|--------|
| Estrutura | 85% | |
| Requisitos | 70% | |
| DDD Modular | 90% | |
| Liquibase | 100% | |
| Testes | 60% | |

### Gaps Críticos

Seção — Bloqueadores que impedem implementação:

```
[#CRITICA-001] impl dependendo de impl de outro modulo
   Regra: backend-coding-standards.md
   Impacto: Viola DDD Modular
   Correcao: Depender apenas da camada api
```

### Gaps de Alta Prioridade

Seção — Riscos de segurança, quebra de padrão:

```
[#ALTA-001] Falta de @HasPermission no controller
   Categoria: Seguranca
   Recomendacao: Adicionar @HasPermission("TASK_WRITE") nos metodos
```

---

## Interpretando Severidades

### Critica (Bloqueador)

**Impede:** Aprovação e implementação
**Ação:** Corrigir **antes** de qualquer coding
**Exemplo:** Violação de dependência entre módulos DDD

### Alta (Risco)

**Impede:** Qualidade da implementação
**Ação:** Corrigir **durante** implementação
**Exemplo:** Falta de edge cases documentados

### Media (Recomendação)

**Impede:** Qualidade
**Ação:** Endereçar em **próxima sprint**
**Exemplo:** Nomenclatura inconsistente

### Baixa (Boa prática)

**Impede:** Nada (opcional)
**Ação:** Considerar **se tempo permitir**
**Exemplo:** Métricas de performance na spec

---

## Exemplos Práticos

### Exemplo 1: Spec Backend Task

**Input:**
```
Revise: .spec/backend/task/ (CRUD de tarefas - modulo task)
```

**Output esperado:**
```
Status: Com ressalvas (82%)

Criticos:
- Nenhum

Altos:
- Testes de integracao nao especificados
- Edge cases de transicao de status nao documentados

Medios:
- Falta diagrama de fluxo de estados

Acao imediata: 0 itens
Prazo: Pronto para coding com ressalvas
```

### Exemplo 2: Spec Frontend Project

**Input:**
```
Revise a specification de projetos em .spec/frontend/project/
```

**Output esperado:**
```
Status: Com ressalvas (75%)

Criticos:
- Store monolitica (nao separada por dominio)

Altos:
- Service sem metodo save() padrao
- Interceptor 401 nao documentado

Medios:
- Testes Vitest nao previstos

Acao imediata: 1 item
Prazo: Corrigir critico antes de coding
```

---

## Fluxo Completo: Spec → Implementação → Code Review

```
1. SPEC CRIADA (SDD Compiler)
   sdd-compiler generate-spec -> SPEC.md
   |
   v
2. SPEC REVIEW (Voce esta aqui)
   invoke spec-review nos arquivos
   Gera relatorio com gaps e scorecard
   |
   +-- Criticos? -- Sim --> Corrigir SPEC
   |                            |
   +-- Nao                      |
       |                        |
       v                        v
   Aprovada com               (loop)
   ressalhas
       |
       v
3. PLAN + TASKS (SDD Compiler)
   sdd-compiler generate-plan -> PLAN.md
   sdd-compiler generate-tasks -> TASKS.md
       |
       v
4. IMPLEMENTACAO (T-01 a T-N)
   Tasks seguem TASKS.md
   Codigo + testes unitarios + integracao
       |
       v
5. CODE REVIEW (fullstack-code-reviewer subagent)
   Verifica codigo contra spec e regras
```

---

## Duvidas Frequentes

### P: E se minha spec for mista (Backend + Frontend)?

R: A skill reconhecerá e aplicará regras de ambos os domínios:
- Verificará DDD Modular + Liquibase (Backend)
- Verificará Pinia stores + componentes Vue 3 (Frontend)
- Scorecard mostrará conformidade por categoria

### P: Posso invocar spec-review em uma spec preliminar?

R: Sim! Use-a iterativamente:
1. Versão 0.1 → Spec Review → Encontra gaps principais
2. Corrige críticos e altos
3. Versão 0.9 → Spec Review novamente → Valida correções
4. Versão 1.0 → Aprovada

### P: Qual é o percentual mínimo de conformidade para aprovação?

R: **Sem gaps críticos** = Pode ser aprovada com ressalvas
**Com 80%+** = Boa qualidade
**Com <60%** = Retrabalho significativo necessário

### P: A skill valida conteúdo ou apenas forma?

R: Ambos:
- **Forma:** Checklist estrutural (título, seções, dependências)
- **Conteúdo:** Conformidade com regras (DDD, Liquibase, segurança)

---

## Dicas Profissionais

### Use spec-review quando:

- Spec foi criada e precisa ser revisada
- Quer validar antes de colocar em backlog
- Precisa gerar feedback estruturado
- Quer garantir qualidade desde o design

### Não use quando:

- Quer revisar código (use subagent `fullstack-code-reviewer`)
- Quer validar testes (use subagent `qa-engineer`)
- Quer análise de segurança profunda (use subagent `security-engineer`)

### Boas práticas:

1. **Faça iterações:** Spec v0.1 → Review → v0.9 → Review → v1.0
2. **Endereçe críticos primeiro:** Outros gaps podem seguir
3. **Use plano de ação:** Quebra tasks de forma organizada
4. **Compartilhe relatório:** Com autor e stakeholders
5. **Re-revise após mudanças:** Grandes alterações = novo review

---

_Baseado no template spec-review.v2.1, adaptado para TaskFlow_
