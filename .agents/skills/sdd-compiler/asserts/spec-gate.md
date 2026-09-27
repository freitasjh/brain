# SPEC Validation Gate (Asserts)

Toda especificação (SPEC) deve ser validada contra os seguintes critérios antes de prosseguir para a fase de planejamento.

## 1. Integridade Arquitetural
- [ ] **ADRs**: Contém registros de decisão (ADRs) explicando o "Porquê" das escolhas críticas.
- [ ] **DDD Modular**: Separação clara entre camadas api/impl/public respeitada.
- [ ] **Segurança/IAM**: Modelo de autenticação (JWT) e `@HasPermission` definidos.

## 2. Modelo de Dados e Eventos
- [ ] **ERD**: Diagrama de Entidade-Relacionamento incluído (se aplicável).
- [ ] **Event Registry**: Lista de eventos com Produtor, Consumidor, Payload (se aplicável).
- [ ] **Migrations**: Arquivos Flyway planejados com `V{YYYYMMDD}{NNN}`.

## 3. Qualidade e Testabilidade
- [ ] **Invariantes de Entidade**: Propriedades que devem ser sempre verdadeiras para cada entidade principal.
- [ ] **Invariantes de Workflow**: Regras de transição de estado definidas.
- [ ] **Test Strategy**: Estratégia de testes (unitários + integração) definida.

## 4. Estrutura e Escopo
- [ ] **Single File**: O documento é coeso e está em um único arquivo.
- [ ] **Domínio claro**: Domínio afetado identificado (user/employee/project/team/task/workflow/kanban).
