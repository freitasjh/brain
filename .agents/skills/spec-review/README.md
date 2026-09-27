# Skill: Spec Review — TaskFlow

**Revisor de Specifications TaskFlow** — Valida conformidade de specs com as regras de DDD Modular, padrões Maven multi-module, Vue 3 + Vuetify 3, identifica gaps e gera plano de ação.

---

## Propósito

Esta skill foi criada para garantir que todas as specifications criadas no projeto **TaskFlow** (Spring Boot 3.5 + Vue 3) seguem as regras de arquitetura, segurança e qualidade definidas no repositório antes da implementação.

**Objetivos:**
- Validar conformidade com regras canônicas (`.agents/rules/`)
- Identificar gaps por severidade
- Gerar feedback estruturado
- Preparar plano de ação
- Prevenir retrabalho durante implementação

---

## Estrutura

```
.agents/skills/spec-review/
├── SKILL.md                    # Definição e procedure da skill (adaptada TaskFlow)
├── README.md                   # Este arquivo
└── assets/
    ├── checklist.md            # Checklist de validação TaskFlow (50+ itens)
    ├── output-template.md      # Template de relatório (adaptado TaskFlow)
    └── usage-guide.md          # Guia de uso (adaptado TaskFlow)
```

---

## Quick Start

### Invocar a Skill

```
Revise a specification em .spec/backend/task/ seguindo as regras do projeto.

Revise .spec/backend/task/SPEC.md:
- Verifique conformidade com DDD Modular e padrões Maven multi-module
- Identifique gaps por severidade
- Gere um plano de ação detalhado
```

### Saída Esperada

```
Relatório com:
- Scorecard de conformidade (%)
- Gaps críticos (bloqueadores)
- Gaps de alta prioridade
- Recomendações (média/baixa)
- Pontos positivos
- Plano de ação estruturado
```

---

## O que a Skill Valida

### Estrutura (Forma)

- [ ] Título claro
- [ ] Descrição e escopo definidos
- [ ] Domínio e camada identificados
- [ ] Dependências entre módulos listadas
- [ ] Riscos mapeados

### Requisitos Funcionais (Conteúdo)

- [ ] User stories no formato padrão
- [ ] Critérios de aceite verificáveis
- [ ] Regras de negócio descritas
- [ ] Tratamento de erros especificado
- [ ] Edge cases documentados

### DDD Modular (Backend)

- [ ] Módulos seguem estrutura `api/` → `impl/` → `public/` (ou `public-api/`)
- [ ] `impl` NUNCA depende de `impl` de outro módulo — só via `api`
- [ ] Camada `public` usa DTOs, nunca expõe entidades JPA
- [ ] Repository Pattern: `AbstractRepository` (EntityManager) ou `JpaRepository` + `JpaSpecificationExecutor`
- [ ] Service Layer com interface na `api`, implementação na `impl`
- [ ] `securityService.getCurrentEmployeeId()` para usuário logado
- [ ] `@HasPermission("PERMISSION_NAME")` em métodos de controller
- [ ] Controllers estendem `AbstractController`

### Liquibase Migrations

- [ ] XML changelogs em `dbupdate/src/main/resources/changelog/changes/`
- [ ] Changeset id no formato `{timestamp}-{descricao}`
- [ ] Include adicionado ao `master-changelog.xml` na ordem correta
- [ ] NUNCA alterar changesets já executados — novo changeset sempre

### Frontend (Vue 3)

- [ ] Composition API `<script setup lang="ts">`
- [ ] Stores Pinia (Options API: state/getters/actions)
- [ ] Services class-based com `http` (Axios), endpoint `/api/v1/{resource}`
- [ ] `resetState()` pattern nas stores
- [ ] Models: TS class com construtor e defaults
- [ ] Ícones apenas Remix (`ri-*`), proibido `mdi-*`
- [ ] Auto-imports: vue, vue-router, @vueuse/core, @vueuse/math, pinia
- [ ] SweetAlert2 para toasts/modals/erros via `useHandlerMessage()`
- [ ] CSS scoped nos componentes

### Testes

- [ ] Unitários: JUnit 5 + Mockito, padrão `@ExtendWith(MockitoExtension.class)`
- [ ] Fakes em `src/test/java/.../fake/` com `fake()` e `fakeVO()`
- [ ] Naming: `when{Action}_then{Expected}` camelCase
- [ ] AssertJ preferencial, JUnit assertions aceitáveis
- [ ] Integração: `*IT.java` no módulo `integration-test/`
- [ ] PostgreSQL 16 via Testcontainers, MockMvc (NÃO RestAssured)
- [ ] IntegrationTestUtil para estado compartilhado
- [ ] Controllers testados APENAS via integração
- [ ] Cobertura: 80% service, 90% domain
- [ ] Sem `@SpringBootTest` em unitários, sem H2, sem Lombok

### Segurança

- [ ] JWT Bearer token via header `Authorization`
- [ ] `@HasPermission("PERMISSION_NAME")` em cada método de controller
- [ ] Validação de entrada com `@Valid` nos DTOs
- [ ] `MethodArgumentNotValidException` → 406 com lista de erros
- [ ] Sem dados sensíveis em logs

### Performance

- [ ] Coleções pré-dimensionadas (`new ArrayList<>(size)`)
- [ ] Sem Streams em caminhos críticos (segurança, filtros HTTP)
- [ ] Mapeamento explícito com converters manuais (padrão `of()`) — sem MapStruct

### Convenções

- [ ] Nomes em inglês (classes, métodos, variáveis), mensagens UI em PT-BR
- [ ] Zero comentários no código
- [ ] Nomenclatura: `*ServiceImpl`, `*Controller`, `*Mapper`, `*Converter`
- [ ] Testes: `*Test.java` (unit), `*IT.java` (integration)
- [ ] Lombok NÃO usado
- [ ] Injeção por construtor (nunca `@Autowired` em campo)

---

## Severidades de Gaps

| Severidade | Símbolo | Bloqueador | Prazo | Exemplo |
|-----------|---------|-----------|-------|---------|
| **Crítica** | 🔴 | SIM | Imediato | Violação de dependência entre módulos DDD |
| **Alta** | 🟠 | SIM | Sprint atual | Falta de `@HasPermission` em controller |
| **Média** | 🟡 | NÃO | Próxima sprint | Nomenclatura inconsistente |
| **Baixa** | 🟢 | NÃO | Opcional | Métricas de performance na spec |

---

## Assets Incluídos

### 1. `SKILL.md` (Definição)

Descreve o propósito, procedure e referências da skill adaptada para TaskFlow.

**Seções:**
- When to Use (quando invocar)
- Procedure (passos detalhados)
- Regras canônicas a verificar
- Referências consultadas

### 2. `assets/checklist.md` (50+ Itens)

Checklist completo adaptado para TaskFlow.

**Categorias:**
1. Estrutura (9 itens)
2. Requisitos Funcionais (6 itens)
3. DDD Modular (10 itens)
4. Liquibase (5 itens)
5. Frontend Vue 3 (10 itens)
6. Testes Backend (9 itens)
7. Segurança (6 itens)
8. Performance (4 itens)
9. Convenções (6 itens)

### 3. `assets/output-template.md` (Relatório Estruturado)

Template profissional para gerar relatórios de revisão.

**Seções:**
- Resumo executivo com scorecard
- Gaps críticos (com exemplos de correção)
- Gaps de alta/média/baixa prioridade
- Pontos positivos
- Plano de ação com prazos

### 4. `assets/usage-guide.md` (Guia Prático)

Guia de uso com exemplos práticos e dicas.

---

## Regras Canônicas Consultadas

A skill consulta **sempre** as seguintes regras:

### Obrigatórias (Todas as Specs)

- `.agents/rules/backend-coding-standards.md` — DDD modular, performance
- `.agents/rules/frontend-coding-standards.md` — Vue 3, Pinia, componentes
- `.agents/rules/workflow-rules.md` — Regras gerais de workflow

### Conforme Tipo de Spec

- **Backend:** `backend-coding-standards.md`, `backend-unit-tests.md`, `backend-integration-tests.md`
- **Frontend:** `frontend-coding-standards.md`
- **Liquibase:** `db-migration-liquibase.md`

### Documentos de Referência

- `AGENTS.md` — Visão geral do projeto e comandos
- `.agents/rules/` — Regras canônicas do projeto

---

## Workflow Completo

```
1. SPEC é Criada (SDD Compiler)
   └── sdd-compiler generate-spec → SPEC.md
   ↓
2. Invoke spec-review nos arquivos
   ├── Lê a specification
   ├── Executa checklist (50+ itens)
   ├── Cruza com regras canônicas
   ├── Classifica gaps por severidade
   └── Gera relatório
   ↓
3. Relatório com Gaps
   ├── 🔴 Críticos (bloqueadores)
   ├── 🟠 Altos
   ├── 🟡 Médios
   └── 🟢 Baixos
   ↓
4. Autor Corrige Críticos
   ├── Atualiza spec
   └── Re-invoca spec-review
   ↓
5. Spec Aprovada
   ├── sdd-compiler generate-plan → PLAN.md
   ├── sdd-compiler generate-tasks → TASKS.md
   ├── Implementação via tasks
   └── Code review (fullstack-code-reviewer)
   ↓
6. QA + Merge
```

---

## Dicas de Uso

### Use spec-review quando:
- Spec foi criada e precisa ser revisada antes da implementação
- Quer validar conformidade com DDD Modular e boas práticas TaskFlow
- Precisa gerar feedback estruturado para o autor
- Quer criar um plano de ação para correções

### Não use quando:
- Quer revisar código implementado (use subagent `fullstack-code-reviewer`)
- Quer validar testes (use subagent `qa-engineer`)
- Quer análise profunda de segurança (use subagent `security-engineer`)

### Boas práticas:
1. **Faça iterações:** v0.1 → Review → v0.9 → Review → v1.0
2. **Endereçe críticos primeiro:** Bloqueiam aprovação
3. **Use plano de ação:** Quebra tasks de forma organizada
4. **Compartilhe relatório:** Com autor e stakeholders
5. **Re-revise após mudanças:** Mudanças significativas = novo review

---

## Benefícios

| Benefício | Descrição |
|-----------|-----------|
| **Qualidade** | Specs validadas contra 50+ critérios TaskFlow |
| **Arquitetura** | Garante DDD Modular + padrões Maven multi-module |
| **Consistência** | Estrutura api/impl/public, padrões Vue 3 |
| **Eficiência** | Menos retrabalho durante implementação |
| **Rastreabilidade** | Plano de ação estruturado |

---

## Histórico de Versões

| Versão | Data | Mudanças |
|--------|------|----------|
| v1.0 | 2026-06-17 | Versão inicial (PDV Java/JavaFX) |
| v2.0 | 2026-06-20 | Adaptado para MedFlow (DDD Modular, Vue 3) |
| v2.1 | 2026-07-17 | Adaptado para TaskFlow (Jira-alternative, Liquibase, @HasPermission) |

---

_Skill `spec-review` v2.1 — Revisor de Specifications TaskFlow_
