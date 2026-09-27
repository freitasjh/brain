# TASKS Validation Gate (Asserts)

O backlog de tarefas (TASKS) deve ser validado contra os seguintes critérios antes de ser aprovado para execução.

## 1. Fidelidade e Detalhamento
- [ ] **Traceability 1:1**: Todos os Task IDs do PLAN estão presentes nas TASKS.
- [ ] **Metadata Enrichment**: Priority (P0-P2), Risk (Low-High), Task Type e Metadata de dependência preenchidos.
- [ ] **Layer-to-Folder Mapping**: Paths físicos e convenções de nomenclatura seguem os padrões da stack tecnológica.

## 2. Rigor de Testes (TDD/BDD)
- [ ] **BDD Scenarios**: Cada tarefa possui cenários de Sucesso, Falha (Failure Mode) e Caso de Borda/Idempotência.
- [ ] **TDD Order**: Passos de implementação listam a criação do arquivo de teste ANTES da implementação.
- [ ] **Granularidade**: Tarefas são atômicas e executáveis em menos de 2 horas.

## 3. Integridade do Grafo
- [ ] **Mermaid Graph**: Grafo de dependências sem ciclos e sem tarefas órfãs.
- [ ] **Consistency Check**: Validação de que não foram inventadas macro-tarefas ausentes no PLAN.

## 4. Definição de Pronto (DoD)
- [ ] **DoD Per Task**: Critérios claros de finalização para cada ticket.
