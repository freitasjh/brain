# Template de Saída — Relatório de Revisão de Specification

Use este template **obrigatoriamente** ao gerar relatórios de review de specifications. Não altere a estrutura de seções, mas adapte para o contexto específico.

---

# Relatório de Revisão de Specification — TaskFlow

**Data de Revisão:** `[DD/MM/YYYY]`  
**Revisor:** `[Nome/Skill]`  
**Versão da Spec:** `[v1.0 / v2.0 / ...]`

---

## Resumo Executivo

| Campo | Valor |
|-------|-------|
| **Arquivo analisado** | `[caminho/para/SPEC.md]` |
| **Tipo de Feature** | Backend / Frontend / Infraestrutura / Mista |
| **Domínio afetado** | user / employee / project / team / task / workflow / kanban |
| **Escopo** | Nova feature / Refactor / Correção |
| **Status Geral** | Aprovada / Com ressalvas / Reprovada |
| **Bloqueadores** | Sim / Não — [Quantidade] críticos |
| **Conformidade Total** | `[XX]%` |

### Síntese

[Parágrafo de 2-3 linhas resumindo achados principais]

---

## Scorecard de Conformidade

| Categoria | Conformidade | Status | Observações |
|-----------|--------------|--------|-------------|
| **Estrutura da Spec** | `[XX]%` | / | [Brief comment] |
| **Requisitos Funcionais** | `[XX]%` | / | [Brief comment] |
| **DDD Modular** | `[XX]%` | / | [Brief comment] |
| **Liquibase Migrations** | `[XX]%` | / | [Brief comment] |
| **Frontend (Vue 3)** | `[XX]%` | / | [Brief comment] |
| **Testes Backend** | `[XX]%` | / | [Brief comment] |
| **Segurança** | `[XX]%` | / | [Brief comment] |
| **Performance** | `[XX]%` | / | [Brief comment] |
| **Convenções** | `[XX]%` | / | [Brief comment] |

**Média Geral:** `[XX]%` — [Excelente / Bom / Regular / Ruim]

---

## Gaps Críticos (Bloqueiam Aprovação)

_(Esta seção é VAZIA se não houver críticos - neste caso, remova)_

Estes gaps **impedem a implementação** e devem ser corrigidos **antes de qualquer coding**:

### DDD Modular

**[#CRÍTICA-001]** — Violação de dependência entre módulos
- **Linha da Spec:** [X]
- **Regra Violada:** `backend-coding-standards.md` — impl NUNCA depende de impl de outro módulo
- **Problema:** A spec faz um módulo impl depender diretamente de impl de outro domínio
- **Impacto:** Viola DDD modular, cria acoplamento indesejado
- **Correção Obrigatória:**
  ```
  Depender apenas da camada api do módulo externo:
  - Certo: task/impl → project/api
  - Errado: task/impl → project/impl
  ```

**[#CRÍTICA-002]** — Entidade JPA exposta via controller
- **Linha da Spec:** [X]
- **Regra Violada:** `backend-coding-standards.md` — Camada public usa DTOs, nunca entidades JPA
- **Problema:** Controller recebe ou devolve @Entity diretamente
- **Impacto:** Viola separação de camadas, expõe detalhes de persistência
- **Correção Obrigatória:**
  ```
  Criar DTOs específicos para request/response no módulo public
  ```

### Liquibase

**[#CRÍTICA-003]** — Migration sem changeset no master-changelog.xml
- **Linha da Spec:** [X]
- **Regra Violada:** `db-migration-liquibase.md` — Include no master na ordem correta
- **Problema:** Migration não incluída no master-changelog.xml
- **Impacto:** Liquibase não executa a migration
- **Correção Obrigatória:**
  ```
  Adicionar include em master-changelog.xml na posição correta da ordem de dependências
  ```

---

## Gaps de Alta Prioridade

_(Recomendações importantes que devem ser endereçadas antes da implementação)_

### Requisitos Funcionais

**[#ALTA-001]** — Critérios de aceite incompletos
- **Categoria:** Requisitos Funcionais
- **Recomendação:** Adicionar critérios de aceite verificáveis para cada user story
- **Impacto:** Critérios claros evitam retrabalho

**[#ALTA-002]** — Edge cases não documentados
- **Categoria:** Requisitos Funcionais
- **Recomendação:** Incluir tabela de edge cases
- **Impacto:** Evita bugs em produção

### DDD Modular

**[#ALTA-003]** — Service sem interface na camada api
- **Categoria:** DDD Modular
- **Recomendação:** Criar interface no módulo api, implementação no impl
- **Exemplo esperado:**
  ```
  task/api/.../service/TaskService.java (interface)
  task/impl/.../service/TaskServiceImpl.java (implementação)
  ```
- **Impacto:** Garante baixo acoplamento entre módulos

**[#ALTA-004]** — Controller sem `@HasPermission`
- **Categoria:** Segurança
- **Recomendação:** Adicionar `@HasPermission("PERMISSION_NAME")` nos métodos do controller
- **Impacto:** Controle de acesso obrigatório

### Testes

**[#ALTA-005]** — Estratégia de testes incompleta
- **Categoria:** Testes
- **Recomendação:** Definir cobertura mínima e casos de teste principais
- **Exemplo esperado:**
  ```
  - Unitários: TaskServiceImpl, domain converters, exceptions
  - Integração: TaskControllerV1IT (POST, GET, PUT, DELETE)
  - Cobertura mínima: 80% service, 90% domain
  ```
- **Impacto:** Garante qualidade do código

---

## Gaps de Média Prioridade

_(Melhorias importantes para qualidade, recomendadas mas não blocantes)_

### Convenções

**[#MÉDIA-001]** — Nomenclatura inconsistente
- **Categoria:** Convenções
- **Recomendação:** Seguir padrão *ServiceImpl, *Converter, *Controller, *Mapper
- **Benefício:** Consistência e navegabilidade

**[#MÉDIA-002]** — Idiomas misturados
- **Categoria:** Convenções
- **Recomendação:** Classes/métodos em inglês, mensagens de UI em PT-BR
- **Benefício:** Clareza para desenvolvedores + usabilidade

### Documentação

**[#MÉDIA-003]** — Diagrama de fluxo ausente
- **Categoria:** Documentação
- **Recomendação:** Incluir diagrama de sequência ou fluxo
- **Benefício:** Facilita entendimento e onboarding

### Frontend

**[#MÉDIA-004]** — Ícones mdi usados em vez de ri
- **Categoria:** Frontend
- **Recomendação:** Substituir `mdi-*` por `ri-*` (apenas Remix icons permitidos)
- **Benefício:** Consistência visual

---

## Gaps de Baixa Prioridade

_(Recomendações opcionais - boas práticas)_

- **[#BAIXA-001]** Adicionar seção de métricas de performance esperadas (ex: resposta < 200ms)
- **[#BAIXA-002]** Incluir troubleshooting comum na spec
- **[#BAIXA-003]** Documentar versões de dependências usadas

---

## Pontos Positivos da Specification

Aspectos bem estruturados e alinhados com as regras:

- **User stories bem definidas** — Formato claro e objetivo
- **Escopo explícito** — Deixa claro o que faz e não faz parte
- **DDD Modular respeitado** — Separação correta entre api/impl/public
- **Liquibase bem estruturado** — Migrations na ordem correta
- **Testes planejados** — Estratégia de testes definida

---

## Plano de Ação

### Imediato (Antes da Aprovação)

**Responsável:** [Autor da Spec]  
**Prazo:** [Hoje]

- [ ] **[#CRÍTICA-001]** Corrigir violação de dependência entre módulos
- [ ] **[#CRÍTICA-002]** Substituir entidade JPA por DTO no controller
- [ ] **[#CRÍTICA-003]** Adicionar include no master-changelog.xml

**Checklist:**
- [ ] Todos os críticos foram endereçados
- [ ] Spec foi revisada novamente
- [ ] Está pronta para aprovação final

### Esta Sprint (Antes da Implementação)

**Responsável:** [Dev Lead]  
**Prazo:** [Durante a sprint atual]

- [ ] **[#ALTA-001]** Adicionar critérios de aceite
- [ ] **[#ALTA-002]** Incluir tabela de edge cases
- [ ] **[#ALTA-003]** Criar interface de service no módulo api
- [ ] **[#ALTA-004]** Adicionar @HasPermission nos controllers
- [ ] **[#ALTA-005]** Detalhar estratégia de testes

**Resultado esperado:** Spec pronta para implementação

### Próxima Sprint

**Responsável:** [Autor]  
**Prazo:** [Próxima sprint]

- [ ] **[#MÉDIA-001]** Revisar nomenclatura de classes
- [ ] **[#MÉDIA-002]** Verificar idioma de métodos vs UI
- [ ] **[#MÉDIA-003]** Adicionar diagramas de fluxo
- [ ] **[#MÉDIA-004]** Substituir mdi por ri icons

---

## Referências Consultadas

Regras canônicas verificadas durante a análise:

- `.agents/rules/backend-coding-standards.md` — DDD modular, performance
- `.agents/rules/frontend-coding-standards.md` — Vue 3, Pinia, componentes
- `.agents/rules/db-migration-liquibase.md` — Liquibase versionamento
- `.agents/rules/backend-unit-tests.md` — Testes unitários
- `.agents/rules/backend-integration-tests.md` — Testes de integração
- `.agents/rules/workflow-rules.md` — Regras gerais de workflow
- `AGENTS.md` — Visão geral do projeto

---

## Próximos Passos

1. **Autor:** Implementar correções dos críticos (Imediato)
2. **Revisor:** Re-revisar após correções
3. **Dev Lead:** Quebrar gaps de alta prioridade em tasks
4. **Team:** Implementar conforme plano de ação

---

## Dados da Revisão

| Campo | Valor |
|-------|-------|
| Data de Revisão | [DD/MM/YYYY HH:MM] |
| Revisor | [Nome / Skill] |
| Tempo de Revisão | [X minutos] |
| Versão do Checklist | v2.1 (TaskFlow) |
| Total de Gaps | [Críticos: X / Alta: X / Média: X / Baixa: X] |
| Recomendação | [Aprovada / Com Ressalvas / Reprovada] |

---

_Relatório gerado seguindo checklist TaskFlow `spec-review.v2.1`_
