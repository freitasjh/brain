# Trace Template

## Relatório de Traceability — {FEATURE_NAME}

### Visão Geral

| Métrica | Valor |
|---------|-------|
| Total RFs | {total_rfs} |
| Total Tasks | {total_tasks} |
| Total Arquivos | {total_files} |
| Score Geral | {score}% |

### Spec → Task → Code

| RF | Descrição | Task | Arquivo | Status |
|----|-----------|------|---------|--------|
| RF-01 | {desc} | T-01 | {file} | ✅ |
| RF-02 | {desc} | T-02 | — | ⚠️ No code |
| RF-03 | {desc} | — | — | ❌ Orphaned |

### Code → Spec (Reverse)

| Arquivo | Módulo | RF | Status |
|---------|--------|----|--------|
| {file}.java | service | RF-01 | ✅ Mapped |
| {file}.java | infra | — | ⚠️ Infra (ok) |
| {file}.java | legacy | — | ❌ Orphaned |

### Gaps Identificados

#### Orphaned Specs (sem implementação)
- {RF}: {reason}

#### Orphaned Code (sem especificação)
- {module}: {reason}

### Score Breakdown

```
RF→Task:   {rf_task_pct}% ({rf_task_mapped}/{rf_task_total})
Task→Code: {task_code_pct}% ({task_code_mapped}/{task_code_total})
Specs:     {specs_ok}/{specs_total} mapped
Code:      {code_ok}/{code_total} mapped

Overall:   {score}%
```

### Recomendações

1. {recommendation_1}
2. {recommendation_2}
3. {recommendation_3}

### Decisão

- [ ] ✅ PASS — Score ≥80%, prosseguir para archive
- [ ] ⚠️ WARN — Score 60-79%, archive com justificativa
- [ ] ❌ FAIL — Score <60%, voltar para implementação
