# Workflow Rules

Estas regras definem o fluxo de desenvolvimento obrigatório. Devem ser seguidas rigorosamente.

## Workflow State

- Antes de iniciar qualquer desenvolvimento, **SEMPRE** ler `workflow-state.json` na raiz do projeto.
- O arquivo contém o estado atual do workflow: fase ativa, especificações em andamento, tarefas concluídas.
- Após cada alteração relevante, atualizar o `workflow-state.json`.

## Novas Funcionalidades (Features)

Toda nova funcionalidade/inovação **OBRIGATORIAMENTE** deve seguir o spec-driven-development:

1. Carregar a skill `spec-driven-development` via `skill({ name: "spec-driven-development" })`
2. Executar as 3 fases completas:
   - **Phase 1 — Requirements**: User stories + acceptance criteria em EARS
   - **Phase 2 — Design**: Arquitetura, componentes, data models, interfaces
   - **Phase 3 — Tasks**: Tasks de implementação sequenciadas (2-4h cada)
3. Salvar o spec em `.spec/<feature-name>/`
4. Só então iniciar a implementação

## Correções de Bug (Bugfix)

- **Não** executar o spec-driven-development completo.
- Apenas criar as tasks necessárias usando a skill `task-breakdown`.
- Documentar o bug e a solução diretamente nas tasks.
- Atualizar `workflow-state.json` com as tasks de bugfix.

## Regras Gerais

- Nunca pular a leitura do `workflow-state.json`.
- Nunca implementar sem spec (para features) ou sem tasks (para bugfixes).
- Atualizar o workflow state ao concluir cada fase ou milestone.

## Testes Obrigatórios

**TODO** desenvolvimento (features OU bugfixes) **OBRIGATORIAMENTE** deve incluir testes:

### Backend (Java / Spring Boot)
1. **Ambiente**: SEMPRE trocar para Java 21 antes de qualquer comando:
   ```bash
   sdk use java 21.0.10-zulu
   ```
2. **Testes Unitários** (JUnit 5 + Mockito):
   - Todo **service** deve ter teste unitário cobrindo:
     - Fluxo principal (happy path)
     - Fluxos de erro (validações, entidade não encontrada, conflitos)
     - Casos de borda (edge cases)
   - Todo **controller** deve testar HTTP status codes, validação de entrada e serialização
   - Todo **JwtService** / utilitário crítico deve ter teste unitário
3. **Testes de Integração** (Testcontainers + MySQL real):
   - Toda **repository** deve ter teste de integração validando SQL nativo e queries do Spring Data
   - Todo **fluxo completo (controller → service → repository)** deve ter pelo menos 1 teste de integração
   - Testcontainers com `@SpringBootTest` + `@AutoConfigureTestDatabase(replace = NONE)`
4. **Cobertura mínima**: nunca abaixo de 80% nas classes novas/alteredas. Verificar com `jacoco`.

### Frontend (Vue 3 / TypeScript)
1. **Testes Unitários** (Vitest + Vue Testing Library):
   - Toda **store (Pinia)** deve testar actions e getters
   - Toda **view/página** deve testar renderização e interações do usuário
   - Todo **serviço API** deve testar chamadas HTTP e tratamento de erros
2. **Testes de Componente**: validar renderização com props, slots e eventos
3. **Cobertura mínima**: nunca abaixo de 70% nas novas funções/componentes. Verificar com `vitest --coverage`.

### Execução
- Os testes DEVEM passar antes de qualquer commit/push.
- Os testes DEVEM ser executados localmente (ou via CI) e comprovados.
- Se um teste falhar, a implementação deve ser corrigida até os testes passarem.

**Comandos:**
```bash
sdk use java 21.0.10-zulu         # Java 21 obrigatório
mvn test                          # unit tests (exclui *IntegrationTest)
mvn verify -Pintegration-test     # unit + integration tests (requer Docker)
```

## Code Review Obrigatório

Após **finalizar a implementação** de qualquer tarefa (feature ou bugfix), **OBRIGATORIAMENTE**:

1. Invocar o subagente `code-reviewer` para analisar o código gerado
2. O code-reviewer DEVE verificar:
   - Qualidade do código (naming, estrutura, complexidade)
   - Cobertura e qualidade dos testes
   - Segurança (injeção de SQL, exposição de dados sensíveis, validação de entrada)
   - Performance (N+1 queries, loops desnecessários)
   - Adesão às regras de arquitetura (package boundaries, padrões definidos)
3. **TODA** sugestão do code-reviewer marcada como 🔴 **Crítico** deve ser corrigida antes de dar a tarefa como concluída
4. Sugestões 🟡 **Atenção** devem ser avaliadas e corrigidas se aplicável
5. Sugestões 🟢 **Sugestão** ficam a critério, mas devem ser consideradas

## Uso Obrigatório de Skills

Para **TODO** desenvolvimento, as skills apropriadas **OBRIGATORIAMENTE** devem ser carregadas:

- **Desenvolvimento Backend** (Java/Spring Boot/LangChain4j):
  `skill({ name: "backend-skill" })` — contém instruções sobre entidades, repositórios, serviços, controllers, migrations, LangChain4j tools/agents e **testes**

- **Desenvolvimento Frontend** (Vue 3/Pinia/PrimeVue):
  `skill({ name: "frontend-skill" })` — contém instruções sobre componentes, views, stores, router, API service e **testes**

- **Criação de Spec** (features novas):
  `skill({ name: "spec-driven-development" })` — obrigatório para toda nova funcionalidade

- **Task Breakdown** (bugfixes):
  `skill({ name: "task-breakdown" })` — obrigatório para correções de bug

**Regra**: o desenvolvedor (agente) SEMPRE deve carregar a skill correspondente ao que está implementando. As skills contêm templates, exemplos e boas práticas que garantem consistência e qualidade.
