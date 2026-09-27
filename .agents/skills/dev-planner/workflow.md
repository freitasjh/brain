# Workflow Detalhado — Dev Planner

Fluxo completo de planejamento técnico. Cada fase possui:
- **Objetivo:** O que se busca alcançar
- **Entradas:** O que precisa existir antes
- **Processo:** Passos executados
- **Saída:** Artefatos gerados
- **Portão:** Critério para avançar

---

## Visão Geral

```
FASE 0          FASE 1          FASE 2          FASE 3          FASE 4          FASE 5
DESCUBRA    →   ANALISE     →   DESAFIE    →   PROJETE    →   ESTIME     →   VALIDE
O PROBLEMA      RESTRIÇÕES      A SOLUÇÃO      ARQUITETURA     ESFORÇO        FINAL
    │               │               │               │               │               │
    ▼               ▼               ▼               ▼               ▼               ▼
EXPLORATION    CONSTRAINTS   DESIGN-DEC    ARCHITECTURE       PLAN        HANDOFF
   .md            .md         DECISIONS.md     .md              .md      (sdd-compiler)
```

---

## FASE 0: Descubra o Problema Real

### Objetivo
Entender profundamente O QUE o usuário precisa resolver, diferenciando sintoma de causa raiz.

### Entradas
- Descrição inicial do usuário (pode ser vaga)
- Contexto do projeto (ler `AGENTS.md`, `.doc/ARCHITECTURE.md`)

### Processo

#### 0.1 — Carregar Contexto
```
1. brain_search("feature similar", layer="arquitetura", scope="projetos")
2. Ler workflow-state.json (verificar se há feature em andamento)
3. Ler AGENTS.md (entender stack, módulos, convenções)
```

#### 0.2 — Escutar e Repetir
```
1. Leia a solicitação do usuário
2. Repita com suas palavras: "Entendi que você quer [X]. Estou certo?"
3. Aguarde confirmação antes de prosseguir
```

#### 0.3 — Perguntar para Clarificar
Use o template em `questions-template.md` — seção "Fase 0".

Perguntas obrigatórias:
- "Qual é o PROBLEMA que você está resolvendo? (não a solução)"
- "Quem são os USUÁRIOS que vão usar isso?"
- "Como é feito HOJE esse processo? (se existir)"
- "Quais são os CASOS DE USO principais?"
- "Existe ALGO que NÃO pode mudar?"

#### 0.4 — Buscar Referências
```
1. websearch("sistema [domínio] funcionalidades features")
2. websearch("[domínio] workflow processos padrão")
3. webfetch em links relevantes para extrair detalhes
4. Documentar fontes consultadas
```

#### 0.5 — Sintetizar e Validar
```
1. Gere resumo: "O problema é [X]. Os usuários são [Y]. Os casos de uso são [Z]."
2. Pergunte: "Está correto? Algo faltando?"
3. Identifique o TIPO: Feature / Bugfix / Refactor
```

### Saída
- `.spec/[feature]/EXPLORATION.md` com:
  - Problema definido
  - Usuários identificados
  - Casos de uso listados
  - Fontes consultadas (websearch)
  - Classificação (Feature/Bugfix/Refactor)

### Portão
```
"Classifiquei como [FEATURE/BUGFIX/REFACTOR]. 
O problema é [resumo]. 
Está correto? Posso avançar para Análise?"
→ Aguarde SIM explícito
```

---

## FASE 1: Analise Restrições e Contexto

### Objetivo
Mapear todas as limitações técnicas, dependências e requisitos não-funcionais.

### Entradas
- EXPLORATION.md aprovado
- Contexto do projeto (`.doc/ARCHITECTURE.md`, `.doc/STACK.md`)

### Processo

#### 1.1 — Identificar Restrições Técnicas
Pergunte:
- "Esta feature impacta OUTROS módulos?"
- "Exige integração com sistemas EXTERNOS?"
- "Precisa de PERFORMANCE específica? (ex: <200ms)"
- "Precisa de SEGURANÇA especial? (ex: dados sensíveis, PCI)"
- "Precisa de MULTI-TENANT? (isolamento por tenant)"

#### 1.2 — Mapear Dependências
```
1. Identifique módulos Maven afetados
2. Identifique entidades JPA impactadas
3. Identifique services/DAOs que precisam ser criados/alterados
4. Identifique endpoints REST afetados
5. Identifique telas JSF impactadas
```

#### 1.3 — Validar Compliance
```
1. websearch("[domínio] regulatory compliance [Brasil]")
2. websearch("[tipo_dados] LGPD requirements")
3. Identifique se há requisitos regulatórios
```

#### 1.4 — Buscar Padrões de Arquitetura
```
1. websearch("[tipo_sistema] architecture patterns best practices")
2. websearch("[domínio] database design patterns")
3. Compare com a arquitetura atual do projeto
```

### Saída
- `.spec/[feature]/CONSTRAINTS.md` com:
  - Restrições técnicas listadas
  - Dependências mapeadas
  - Requisitos não-funcionais
  - Compliance identificado
  - Fontes consultadas

### Portão
```
"Identifiquei as seguintes restrições: [lista].
Dependências: [lista].
Requisitos não-funcionais: [lista].
Está completo? Posso avançar para Desafio?"
→ Aguarde SIM explícito
```

---

## FASE 2: Desafie a Solução

### Objetivo
Questionar se a abordagem proposta é a melhor, apresentar alternativas e fundamentar decisões.

### Entradas
- EXPLORATION.md + CONSTRAINTS.md aprovados
- Ideia inicial do usuário (se houver)

### Processo

#### 2.1 — Questionar Pressupostos
Se o usuário já tem uma solução em mente, pergunte:
- "Por que escolheu essa abordagem?"
- "Você considerou [alternativa]?"
- "Quais são os RISCOS que você vê nessa abordagem?"
- "O que acontece se [cenário de falha]?"

#### 2.2 — Buscar Alternativas
```
1. websearch("[solução proposta] vs [alternativa1] comparison")
2. websearch("[solução proposta] vs [alternativa2] comparison")
3. websearch("[solução proposta] common pitfalls problems")
4. websearch("[alternativa] advantages disadvantages enterprise")
```

#### 2.3 — Analisar Prós e Contras
Para cada alternativa, documente:

| Critério | Solução A | Solução B | Solução C |
|----------|-----------|-----------|-----------|
| Complexidade | | | |
| Performance | | | |
| Manutenibilidade | | | |
| Alinhamento com stack | | | |
| Risco | | | |
| Esforço | | | |

#### 2.4 — Fundamentar Recomendação
Para cada critério, explique tecnicamente:
- "Solução A é melhor porque [justificativa técnica]"
- "Solução B tem o problema de [risco específico]"
- "Considerando que o projeto usa [tecnologia], recomendo [X] porque [razão]"

#### 2.5 — Registrar Decisões (ADRs)
Crie Architecture Decision Records:
```
## ADR-001: [Título da Decisão]

### Status
Aceito

### Contexto
[O que está acontecendo]

### Decisão
[O que foi decidido]

### Consequências
[O que muda com essa decisão]

### Fontes
- [Referência websearch 1]
- [Referência websearch 2]
```

### Saída
- `.spec/[feature]/DESIGN-DECISIONS.md` com:
  - Alternativas analisadas (mínimo 2)
  - Tabela comparativa
  - Decisões documentadas (ADRs)
  - Justificativas técnicas
  - Fontes consultadas

### Portão
```
"Analisei [N] alternativas. 
Recomendo [SOLUÇÃO X] porque [justificativa].
Prós: [lista]. Contras: [lista].
Aprova essa abordagem? Quer considerar outra alternativa?"
→ Aguarde aprovação explícita
```

---

## FASE 3: Projete a Arquitetura

### Objetivo
Definir modelo de dados, contratos de API, fluxos e integrações de forma detalhada.

### Entradas
- DESIGN-DECISIONS.md aprovado
- Stack do projeto (Java EE 7, EJB, JSF, PostgreSQL)

### Processo

#### 3.1 — Modelagem de Dados
```
1. Defina entidades principais (JPA @Entity)
2. Defina relações (1:1, 1:N, N:N)
3. Defina campos obrigatórios vs opcionais
4. Defina indexes necessários
5. Considere multi-tenancy (schema por tenant)
```

Pergunte:
- "Quais campos são OBRIGATÓRIOS?"
- "Quais campos podem ser NULOS?"
- "Existe histórico que precisa ser mantido?"
- "Qual é o volume esperado de registros?"

#### 3.2 — Buscar Padrões de Modelo
```
1. websearch("[domínio] entity relationship diagram pattern")
2. websearch("[domínio] database normalization best practices")
3. websearch("JPA entity design patterns enterprise")
```

#### 3.3 — Design de API
```
1. Defina endpoints REST (método, path, parâmetros)
2. Defina contratos de request/response (DTOs)
3. Defina códigos de status HTTP
4. Defina autenticação/autorização por endpoint
5. Defina paginação (se aplicável)
```

Pergunte:
- "Quais operações CRUD são necessárias?"
- "Existe busca avançada? Quais filtros?"
- "Precisa de exportação? (PDF, Excel)"
- "Quem pode acessar cada endpoint?"

#### 3.4 — Buscar Padrões de API
```
1. websearch("REST API design patterns [domínio]")
2. websearch("API versioning best practices")
3. websearch("REST error handling patterns enterprise")
```

#### 3.5 — Fluxos Principais
Para cada caso de uso, desenhe:
```
1. Fluxo Principal (happy path)
2. Fluxos de Exceção (erros, validações)
3. Fluxos de Integração (sistemas externos)
4. Fluxos de Segurança (autenticação, autorização)
```

#### 3.6 — Integrações
Se houver integrações externas:
```
1. Identifique sistemas a integrar
2. Defina protocolos (REST, SOAP, fila)
3. Defina autenticação com sistemas externos
4. Defina tratamento de falhas
5. websearch("[sistema] API integration patterns")
```

### Saída
- `.spec/[feature]/ARCHITECTURE.md` com:
  - Modelo de dados (entidades, relações, campos)
  - Contratos de API (endpoints, DTOs, status)
  - Fluxos principais e exceções
  - Integrações externas
  - Fontes consultadas

### Portão
```
"Modelo de dados: [N] entidades, [M] relationships.
API: [N] endpoints, [M] DTOs.
Fluxos: [lista].
Integrações: [lista] (ou nenhuma).
Está correto? Posso avançar para Estimativa?"
→ Aguarde SIM explícito
```

---

## FASE 4: Estime e Priorize

### Objetivo
Decompor em tarefas, estimar esforço, identificar dependências e riscos.

### Entradas
- ARCHITECTURE.md aprovado
- Padrões de estimação do projeto

### Processo

#### 4.1 — Decompor em Tarefas
Para cada componente, crie tarefas seguindo TDD:
```
1. Tarefa de Migration (Liquibase)
2. Tarefa de Entity (JPA)
3. Tarefa de DAO
4. Tarefa de Service
5. Tarefa de Teste Unitário (ANTES da implementação)
6. Tarefa de Controller/Endpoint
7. Tarefa de Tela (se frontend)
8. Tarefa de Teste de Contrato
```

#### 4.2 — Estimar Esforço
Para cada tarefa, estime:
- **Tamanho:** PP (1-2h) / P (4h) / M (1d) / G (2-3d) / GG (1 semana)
- **Risco:** Baixo / Médio / Alto
- **Dependências:** O que precisa estar pronto antes

```
1. websearch("story points estimation enterprise software")
2. websearch("[tipo_tarefa] development time estimation")
```

#### 4.3 — Identificar Dependências
```
1. Mapeie dependências entre tarefas
2. Identifique tarefas que podem ser paralelizadas
3. Identifique gargalos (tarefas que bloqueiam outras)
4. Proponha ordem de execução
```

#### 4.4 — Identificar Riscos
Para cada risco, documente:
```
## Risco: [Descrição]
- Probabilidade: Baixa/Média/Alta
- Impacto: Baixo/Médio/Alto
- Mitigação: [O que fazer para reduzir]
- Contingência: [O que fazer se acontecer]
```

#### 4.5 — Priorizar
Use MoSCoW:
- **Must Have:** Sem isso, a feature não funciona
- **Should Have:** Importante, mas pode ir na próxima versão
- **Could Have:** Desejável, mas não essencial
- **Won't Have:** Explicitamente fora de escopo

### Saída
- `.spec/[feature]/PLAN.md` com:
  - Lista de tarefas com estimativas
  - Ordem de execução
  - Dependências mapeadas
  - Riscos documentados
  - Priorização MoSCoW

### Portão
```
"Total: [N] tarefas, [X] horas estimadas.
Tarefas críticas: [lista].
Riscos principais: [lista].
Ordem de execução: [resumo].
Aprova o plano? Posso gerar as tasks detalhadas?"
→ Aguarde SIM explícito
```

---

## FASE 5: Validação Final

### Objetivo
Revisar tudo, garantir consistência e preparar handoff para implementação.

### Entradas
- Todos os artefatos anteriores (EXPLORATION, CONSTRAINTS, DESIGN-DECISIONS, ARCHITECTURE, PLAN)

### Processo

#### 5.1 — Checklist de Consistência
- [ ] EXPLORATION.md está claro e completo?
- [ ] CONSTRAINTS.md mapeia todas as restrições?
- [ ] DESIGN-DECISIONS.md possui justificativas técnicas?
- [ ] ARCHITECTURE.md está alinhado com as decisões?
- [ ] PLAN.md cobre todos os componentes?
- [ ] Não há contradições entre artefatos?
- [ ] Todas as fontes websearch foram documentadas?

#### 5.2 — Revisão Cruzada
Para cada tarefa do PLAN, verifique:
- [ ] A tarefa está descrita na ARCHITECTURE.md?
- [ ] A tarefa respeita as CONSTRAINTS.md?
- [ ] A tarefa está alinhada com DESIGN-DECISIONS.md?
- [ ] A tarefa cobre o caso de uso do EXPLORATION.md?

#### 5.3 — Validação Técnica
```
1. websearch("[tecnologia] project checklist [ano]")
2. websearch("[padrão] architecture review checklist")
3. Compare com checklists do mercado
```

#### 5.4 — Preparar Handoff
Gere resumo para o `sdd-compiler`:
```
## Resumo para SDD Compiler

### Tipo: [Feature/Bugfix/Refactor]
### Domínio: [nome do domínio]
### Módulos Maven afetados: [lista]

### Artefatos Gerados:
- .spec/[feature]/EXPLORATION.md
- .spec/[feature]/CONSTRAINTS.md
- .spec/[feature]/DESIGN-DECISIONS.md
- .spec/[feature]/ARCHITECTURE.md
- .spec/[feature]/PLAN.md

### Próximo Passo:
Invocar sdd-compiler modo generate-spec
```

### Saída
- Handoff claro para `sdd-compiler`
- Todos os artefatos consistentes e validados
- Decisões registradas no Brain

### Portão Final
```
"Todos os artefatos revisados e consistentes.
Pronto para handoff ao sdd-compiler.
Confirma? Posso encerrar o planejamento?"
→ Aguarde SIM explícito
```

---

## 🔄 Tratamento de Exceções

### Usuário muda de ideia durante o processo
1. Pare imediatamente
2. Pergunte: "O que mudou? Quer recomeçar a fase [X]?"
3. Refaça a fase afetada
4. Valide consistência com fases anteriores

### Usuário quer pular uma fase
1. Explique o risco de pular
2. Se insistir, documente como override
3. Registre no workflow-state.json
4. Siga em frente com ressalva

### Informações conflitantes
1. Apresente o conflito ao usuário
2. Pergunte: "Você disse [A] e [B], mas são conflitantes. Qual prevalece?"
3. Registre a decisão

### Usuário não sabe responder
1. Ofereça opções: "Posso sugerir [X] ou [Y], qual prefere?"
2. Use websearch para buscar referências
3. Recomende baseado em boas práticas
4. Aguarde aprovação da recomendação

---

## 📊 Métricas de Qualidade

Ao final do planejamento, verifique:

| Métrica | Meta |
|---------|------|
| Claridade dos requisitos | 100% dos casos de uso definidos |
| Cobertura de restrições | 100% das restrições mapeadas |
| Alternativas analisadas | Mínimo 2 por decisão importante |
| Fontes websearch | Mínimo 3 por fase |
| Tarefas estimadas | 100% com tamanho e risco |
| Dependências mapeadas | 100% das dependências identificadas |
| Riscos documentados | 100% dos riscos com mitigação |
