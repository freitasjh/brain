# Checklist de Revisão de Specification — TaskFlow

Use este checklist ao revisar specifications (SPEC.md). Para cada item, marque:
- Conforme / Presente
- Ponto de atenção / Incompleto
- Não conforme / Ausente
- N/A Não aplicável

---

## 1. Estrutura e Documentação da Specification

| Item | Status | Comentário |
|------|--------|-----------|
| **Título claro e descritivo** | [ ] | Descreve o objetivo da feature |
| **Visão geral (Overview)** | [ ] | Contextualiza o problema a resolver |
| **Escopo definido (Incluso/Excluído)** | [ ] | Deixa claro o que faz e não faz parte |
| **Domínio afetado identificado** | [ ] | user / employee / project / team / task / workflow / board/kanban / security |
| **Camada afetada identificada** | [ ] | api / impl / public / dbupdate / frontend |
| **Dependências entre módulos** | [ ] | Quais módulos `api` são necessários |
| **Requisitos não-funcionais** | [ ] | Performance, segurança |
| **Riscos identificados** | [ ] | Riscos técnicos e mitigações planejadas |
| **Data de criação / Versão** | [ ] | Rastreabilidade de quando foi criada |

---

## 2. Requisitos Funcionais

| Item | Status | Comentário |
|------|--------|-----------|
| **User stories claras** | [ ] | Formato "Como [ator], eu quero [ação], para [benefício]" |
| **Critérios de aceite verificáveis** | [ ] | Condições objetivas para completude |
| **Casos de uso documentados** | [ ] | Fluxos principais e alternativos |
| **Regras de negócio descritas** | [ ] | Validações, restrições, cálculos |
| **Fluxo de erro tratado** | [ ] | Como o sistema responde a exceções |
| **Cenários edge case** | [ ] | Limites, casos extremos |

---

## 3. DDD Modular (Backend)

### 3.1 Arquitetura e Estrutura

| Item | Status | Comentário |
|------|--------|-----------|
| **Módulos seguem estrutura api/impl/public** | [ ] | api (interfaces/model), impl (services/repos), public (controllers/DTOs) |
| **impl NÃO depende de impl de outro módulo** | [ ] | Só via api (interfaces) |
| **Camada public usa DTOs, nunca entidades JPA** | [ ] | Controllers recebem/devolvem DTOs |
| **Repository Pattern** | [ ] | AbstractRepository (EntityManager) ou JpaRepository + JpaSpecificationExecutor |
| **Service Layer** | [ ] | Interfaces na `api`, implementações na `impl` |
| **Uso de securityService.getCurrentEmployeeId()** | [ ] | Para obter usuário logado |
| **@HasPermission("PERMISSION_NAME")** | [ ] | Controle de acesso por permissão |
| **Controllers extendem AbstractController** | [ ] | Helpers: buildSuccessResponse, buildSuccessResponseNoContent, etc. |
| **Pré-dimensionamento de coleções** | [ ] | `new ArrayList<>(size)` |
| **Sem Streams em caminhos críticos** | [ ] | Segurança, filtros HTTP |

### 3.2 Performance

| Item | Status | Comentário |
|------|--------|-----------|
| **Coleções pré-dimensionadas** | [ ] | Tamanho inicial definido |
| **Mapeamento manual (Converter com of())** | [ ] | Sem MapStruct, sem reflection |
| **Sem Streams em segurança/filtros** | [ ] | Usar loops for-each ou indexados |

---

## 4. Liquibase Migrations

| Item | Status | Comentário |
|------|--------|-----------|
| **XML changelogs em dbupdate/.../changelog/changes/** | [ ] | Caminho padrão do módulo dbupdate |
| **Changeset id {timestamp}-{descricao}** | [ ] | Formato obrigatório |
| **Versão ÚNICA entre todos os módulos** | [ ] | Timestamps não conflitam |
| **Include adicionado ao master-changelog.xml** | [ ] | Na ordem correta de dependências |
| **NUNCA alterar changesets já executados** | [ ] | Criar novo changeset sempre |

---

## 5. Frontend (Vue 3 + Vuetify 3)

### 5.1 Estrutura e Estilo

| Item | Status | Comentário |
|------|--------|-----------|
| **Composition API com `<script setup lang="ts">`** | [ ] | Formato obrigatório |
| **Componentes com CSS scoped** | [ ] | style.css só para variáveis globais |
| **Path aliases corretos** | [ ] | @, @core, @layouts, @images, @styles |
| **Prefix API `/api/v1/{resource}`** | [ ] | Proxy Vite para localhost:8080/taskflow |

### 5.2 Estado e Dados

| Item | Status | Comentário |
|------|--------|-----------|
| **Stores Pinia (Options API)** | [ ] | state/getters/actions, resetState() |
| **Services class-based com http (Axios)** | [ ] | save() verifica id===null para create vs update |
| **Interceptores Axios** | [ ] | Bearer token do localStorage, refresh em 401 |
| **Models: TS class com construtor** | [ ] | Defaults definidos no construtor |
| **Ícones Remix (`ri-*`)** | [ ] | Proibido `mdi-*` |
| **Auto-imports** | [ ] | vue, vue-router, @vueuse/core, pinia — sem import explícito |
| **SweetAlert2 para feedback** | [ ] | useHandlerMessage() para toasts/modals/errors |

### 5.3 Testes Frontend

| Item | Status | Comentário |
|------|--------|-----------|
| **Vitest + happy-dom** | [ ] | Sem jsdom |
| **Stores testadas** | [ ] | Actions e getters |
| **Cobertura mínima 70%** | [ ] | Nas novas funções/componentes |

---

## 6. Testes Backend

| Item | Status | Comentário |
|------|--------|-----------|
| **Estratégia: unitários + integração** | [ ] | Ambos obrigatórios |
| **Unitários: JUnit 5 + Mockito** | [ ] | @ExtendWith(MockitoExtension.class) |
| **Naming: when{Action}_then{Expected}** | [ ] | camelCase |
| **Fakes em src/test/java/.../fake/** | [ ] | Métodos fake() e fakeVO() |
| **Integração: *IT.java no module integration-test** | [ ] | PostgreSQL 16 via Testcontainers |
| **MockMvc (NÃO RestAssured)** | [ ] | @AutoConfigureMockMvc |
| **IntegrationTestUtil para estado compartilhado** | [ ] | Static holders para token e entidades |
| **Testes ordenados com @Order** | [ ] | Login sempre primeiro (@Order(1)) |
| **Cobertura 80% service, 90% domain** | [ ] | Mínima obrigatória |

---

## 7. Segurança

| Item | Status | Comentário |
|------|--------|-----------|
| **JWT Bearer token** | [ ] | Header Authorization |
| **@HasPermission("PERMISSION_NAME")** | [ ] | Em cada método de controller |
| **Validação de entrada em todas as camadas** | [ ] | @Valid, Bean Validation |
| **Sem dados sensíveis em logs** | [ ] | Senhas, tokens, PII |
| **BaseException com httpStatus customizado** | [ ] | ControllerExceptionHandler global |
| **SQL Injection prevenido** | [ ] | JPA PreparedStatement |

---

## 8. Performance

| Item | Status | Comentário |
|------|--------|-----------|
| **Pré-dimensionamento de coleções** | [ ] | ArrayList, HashSet com tamanho inicial |
| **Sem Streams em caminhos críticos** | [ ] | Segurança, autenticação, filtros |
| **Mapeamento manual (Converter com of())** | [ ] | Sem MapStruct, sem reflection |

---

## 9. Convenções de Código

| Item | Status | Comentário |
|------|--------|-----------|
| **Nomes de classes em inglês** | [ ] | Service, Repository, Controller |
| **Mensagens UI em PT-BR** | [ ] | Labels, alerts, toasts |
| **Zero comentários no código** | [ ] | Código auto-documentado |
| **Nomenclatura consistente** | [ ] | *ServiceImpl, *Converter, *Controller, *Mapper |
| **Testes: *Test.java (unit) / *IT.java (integration)** | [ ] | Padrão de nomenclatura |
| **Lombok NÃO usado** | [ ] | Proibido no projeto |
| **Injeção por construtor (nunca @Autowired campo)** | [ ] | Spring recomenda |

---

## Resumo de Status

```
Itens Conformes:       [ ] / [ ]
Itens com Atenção:     [ ] / [ ]
Itens Não-Conformes:   [ ] / [ ]
N/A (Não aplicável):   [ ] / [ ]
```

**Percentual de Conformidade:** `[ ]%`

---

## Referências Consultadas

Regras canônicas verificadas durante a análise:
- [ ] `.agents/rules/backend-coding-standards.md`
- [ ] `.agents/rules/frontend-coding-standards.md`
- [ ] `.agents/rules/db-migration-liquibase.md`
- [ ] `.agents/rules/backend-unit-tests.md`
- [ ] `.agents/rules/backend-integration-tests.md`
- [ ] `.agents/rules/workflow-rules.md`
- [ ] `AGENTS.md`
