---
name: sdd-compiler
description: "Deterministic Architectural Compiler for the full SDD Lifecycle. Unifies PROPOSAL, SPEC, PLAN, TASK, VERIFY and ARCHIVE with rigorous validation gates (Asserts)."
---

# SDD Compiler: The Unified Engineering Engine

Você é o compilador central da metodologia SDD (Specification-Driven Development). Seu objetivo é transformar requisitos em código executável através de fases determinísticas, garantindo rastreabilidade e integridade arquitetural. O ciclo completo é: **Explore → Proposal → SPEC → PLAN → TASKS → [implementar] → Verify → Archive**.

---

## 🛠️ Modos de Operação

### 0. `explore` (Investigação Prévia)
**Objetivo**: Investigar requisitos incertos ANTES de criar artefatos. Use quando a feature é ambígua ou mal definida.
- **Entradas**: Descrição vaga da feature, contexto de negócio.
- **Processo**:
  1. Levantar perguntas em aberto (O quê exatamente? Quem usa? Quais edge cases?).
   2. Analisar sistemas existentes que a feature toca (`.agents/rules/`, `AGENTS.md`, código atual).
  3. Formular hipóteses e alternativas de abordagem.
  4. Recomendar se avança para `generate-proposal` ou precisa de mais clareza.
- **Saída**: `.spec/[feature-name]/EXPLORATION.md` (hipóteses, perguntas, alternativas).
- **NÃO gera SPEC.md** — explore produz apenas o rascunho de entendimento.

---

### 1. `generate-proposal` (Alinhamento de Escopo)
**Objetivo**: Criar `PROPOSAL.md` alinhando problema, solução, escopo e riscos antes do SPEC completo.
- **Entradas**: Descrição da feature (ou `EXPLORATION.md` se existir).
- **Processo**:
  1. Converter descrição em ID kebab-case (ex: `patient-appointment-scheduling`).
  2. Aplicar template em `references/proposal-template.md`.
  3. Identificar delta: quais specs/documentos existentes serão afetados (ADDED/MODIFIED/REMOVED).
  4. Validar viabilidade: conflitos com arquitetura atual, riscos de multi-tenancy.
- **Saída**: `.spec/[feature-name]/PROPOSAL.md`.
- **Asserts**: Validar contra `asserts/proposal-gate.md`.

---

### 2. `generate-spec` (Discovery & Architecture)
**Objetivo**: Gerar especificação prescritiva (SPEC) a partir do PROPOSAL aprovado.
- **Entradas**: `PROPOSAL.md`, `.agents/rules/backend-coding-standards.md`, `.agents/rules/frontend-coding-standards.md`.
- **Processo**:
  1. Recuperar contexto tecnológico e padrões do projeto.
  2. Aplicar template em `references/spec-template.md`.
  3. Definir ADRs, Invariantes (PBT) e Modelos de Eventos.
  4. Gerar seção **Delta Specs** (usando `references/delta-template.md`) listando impacto em docs existentes.
- **Saída**: `.spec/[feature-name]/SPEC.md`.
- **Asserts**: Validar contra `asserts/spec-gate.md`.

---

### 3. `generate-plan` (Strategy & Constraint Mapping)
**Objetivo**: Compilar SPEC em plano de implementação executável.
- **Entradas**: `.spec/[feature-name]/SPEC.md`.
- **Processo**:
  1. Mapear restrições técnicas e modos de falha.
  2. Aplicar template em `references/plan-template.md`.
  3. Garantir bootstrap de infraestrutura e ciclo de vida de eventos (4-tasks per event).
- **Saída**: `.spec/[feature-name]/PLAN.md`.
- **Asserts**: Validar contra `asserts/plan-gate.md`.

---

### 4. `generate-tasks` (TDD Explosion)
**Objetivo**: Explodir PLAN em tickets de desenvolvimento TDD-ready.
- **Entradas**: `SPEC.md` e `PLAN.md`.
- **Processo**:
  1. Enriquecer cada tarefa com metadados (Risco, Prioridade).
  2. Definir cenários BDD (Sucesso, Falha, Borda).
  3. Mapear caminhos físicos de arquivos e convenções de nomenclatura.
- **Saída**: `.spec/[feature-name]/TASKS.md`.
- **Asserts**: Validar contra `asserts/tasks-gate.md`.

---

### 5. `verify` (Validação Pós-Implementação)
**Objetivo**: Verificar se a implementação entregou exatamente o que a SPEC exige. Executar APÓS todas as tasks concluídas.
- **Entradas**: `SPEC.md`, `TASKS.md` (com checkboxes), código implementado.
- **Processo**:
  1. **Completeness**: Todos os checkboxes em TASKS.md marcados? Nenhum TODO pendente?
  2. **Correctness**: Cada requisito funcional da SPEC tem correspondência no código? Testes passando?
  3. **Coherence**: Decisões de design (ADRs) refletem no código? Naming conventions corretas? Sem vazamentos de camada?
  4. Executar build e testes: `mvn clean install` (backend) ou `npm run build && npm run test:run` (frontend).
  5. Se frontend: checar Chrome DevTools (Network + Console) — zero erros/warnings.
- **Saída**: `.spec/[feature-name]/VERIFY.md` com resultado das 3 dimensões + lista de gaps (se houver).
- **Asserts**: Validar contra `asserts/verify-gate.md`.
- **BLOQUEIA archive** se qualquer dimensão falhar.

---

### 6. `archive` (Finalização & Merge de Specs)
**Objetivo**: Finalizar feature, mesclar delta specs em `.agents/rules/`, mover para arquivo histórico.
- **Entradas**: `PROPOSAL.md`, `SPEC.md`, `TASKS.md`, `VERIFY.md` (aprovado).
- **Pré-condição**: `VERIFY.md` deve estar aprovado (todas as 3 dimensões ✅).
- **Processo**:
  1. Ler seção **Delta Specs** do `SPEC.md`.
  2. Para cada entrada delta:
     - **ADDED**: Criar nova seção no doc alvo em `.agents/rules/` ou `AGENTS.md`.
     - **MODIFIED**: Substituir seção existente no doc alvo.
     - **REMOVED**: Mover seção para bloco `## Deprecated` no doc alvo com data de remoção.
  3. Mover pasta `.spec/[feature-name]/` → `.spec/archive/[YYYYMMDD]-[feature-name]/`.
  4. Adicionar metadados de conclusão em `PROPOSAL.md` arquivado (data, duração, arquivos alterados).
  5. Sugerir git commit com mensagem padrão:
     ```
     feat: archive [feature-name] — SDD lifecycle complete
     
     Spec: .spec/archive/[YYYYMMDD]-[feature-name]/
     Delta merged into: .agents/rules/[affected-docs]
     ```
- **NÃO executa `git commit` automaticamente** — sugere e aguarda confirmação.

---

## 🚦 Portões de Validação (Asserts)

| Fase | Assert File |
|------|-------------|
| PROPOSAL | `.agents/skills/sdd-compiler/asserts/proposal-gate.md` |
| SPEC | `.agents/skills/sdd-compiler/asserts/spec-gate.md` |
| PLAN | `.agents/skills/sdd-compiler/asserts/plan-gate.md` |
| TASKS | `.agents/skills/sdd-compiler/asserts/tasks-gate.md` |
| VERIFY | `.agents/skills/sdd-compiler/asserts/verify-gate.md` |

---

## 📚 Referências (Templates)

Pasta `.agents/skills/sdd-compiler/references/`:
- `proposal-template.md` — template de PROPOSAL
- `spec-template.md` — template de SPEC
- `plan-template.md` — template de PLAN
- `task-template.md` — template de TASKS
- `delta-template.md` — template de Delta Specs
- `references/healthcare.md`, `fintech.md`, `saas-multitenant.md` — padrões de domínio

---

## 🔗 Regras Críticas (MANDATÓRIO)

1. **Rastreabilidade 1:1**: Nunca pule requisito da SPEC no PLAN, nem task do PLAN nas TASKS.
2. **TDD First**: Teste listado ANTES da implementação em todas as tasks.
3. **Multi-tenancy**: Isolamento de tenant validado em todas as camadas (Assert-first).
4. **Event Integrity**: Ciclo completo obrigatório (Producer → Outbox → Broker → Consumer).
5. **Verify antes de Archive**: NUNCA arquivar sem VERIFY.md aprovado.
6. **Delta rastreável**: Toda SPEC inclui seção Delta listando impacto em docs existentes.
7. **Explore antes de Proposal**: Feature ambígua → `explore` primeiro, `generate-proposal` depois.
