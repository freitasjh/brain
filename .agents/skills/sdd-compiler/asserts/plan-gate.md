# PLAN Validation Gate (Asserts)

Todo plano de implementação (PLAN) deve ser validado contra os seguintes critérios antes de gerar as tarefas de execução.

## 1. Rastreabilidade e Alinhamento
- [ ] **Traceability 1:1**: Cada requisito da SPEC está mapeado para pelo menos uma tarefa no PLAN.
- [ ] **Constraint Mapping**: Tabela de mapeamento de restrições (Constraint -> Enforcement -> Failure Mode).

## 2. Sequenciamento e Estrutura
- [ ] **Infra Bootstrap First**: Tarefas de infraestrutura (Tenant Context, Security, DB Connection) precedem a lógica de domínio.
- [ ] **Execution Graph**: Sequência lógica respeita as camadas (Infra → Domain → Data → App → Interface → Integration → Frontend).
- [ ] **Critical Execution Path**: Caminho mínimo para o MVP identificado, ignorando fluxos secundários.

## 3. Ciclo de Vida de Eventos
- [ ] **4-Task Event Lifecycle**: Para cada evento da SPEC, existem 4 tarefas: Producer, Outbox, Broker e Consumer (ou N/A justificado).

## 4. Verificação e Testes
- [ ] **Mandatory Testing Tasks**: Tarefas explícitas para Integration Tests de isolamento multi-tenant.
- [ ] **Failure Mode Testing**: Tarefas para testar comportamentos em caso de falha.
- [ ] **Idempotency Validation**: Testes para fluxos de eventos duplicados.
