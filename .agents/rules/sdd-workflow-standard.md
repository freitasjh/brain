# SDD Workflow Standard (MANDATORY)

Esta regra define o contrato de organização e gestão de estado SDD no brain (Rust SQLite-only).

## 1. Organização de Arquivos

Artefatos por feature (single-module Rust, não backend/frontend):

```
.spec/
  brain-rust-sqlite/        # Fase A+B (existente)
  <feature-kebab>/          # ex: phase-c-handoff
    PROPOSAL.md
    SPEC.md    (Delta Specs)
    PLAN.md
    TASKS.md
    VERIFY.md
  bugs/<context>/TASKS.md
  tech-debts/<context>/TASKS.md
  archive/<YYYYMMDD>-<feature>/
```

Cada pasta deve conter `SPEC.md` + `PLAN.md` + `TASKS.md` gerados por `sdd-compiler`.

## 2. Gestão de Estado (`workflow-state.json` na raiz)

Campos obrigatórios:
- `project, version, current_feature, current_phase, previous_phase, next_phase, type (feature|bugfix), artifacts {spec,plan,tasks}, features[], phases_completed, tasks_completed, test_count, code_review {last_status}, harness_status, last_update`

## 3. Registro de Progresso

`spec-developed.md` opcional — lista cronológica features concluídas. Fonte verdade é `workflow-state.json`.

## 4. Portão de Aprovação e Feedback
Antes SPEC→PLAN→TASKS, orquestrador DEVE:
1. Validar vs `sdd-compiler` asserts
2. `spec-review` gerar gaps scorecard
3. Corrigir 🔴 e 🟠 antes avançar
4. Pedir aprovação usuário

## 5. Persistência no Brain
Ao concluir TASKS, salvar `brain_store("sessoes","brain/<date>", ...)` + `brain_store("estudos","brain/<feature>", scope=global|projetos)` + `brain_store("regras","brain/<lesson>", scope=...)` via skill `brain`.
