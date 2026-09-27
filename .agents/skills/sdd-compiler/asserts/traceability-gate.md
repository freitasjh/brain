# Traceability Gate

## Pré-condições

- [ ] Feature tem SPEC.md completo
- [ ] Feature tem TASKS.md com checkboxes
- [ ] Feature tem código implementado (pelo menos parcial)

## Critérios de Aprovação

### Spec → Code Traceability

- [ ] Cada RF (Requisito Funcional) da SPEC tem pelo menos 1 task associada
- [ ] Cada task tem pelo menos 1 arquivo de código associado
- [ ] Não existem RFs sem implementação (orphaned specs)
- [ ] Não existem módulos sem RF correspondente (orphaned code) — aceitar se infra/comum

### Score mínimo: 80%

```
Score = (itens mapeados / total itens) × 100
```

Onde:
- `itens mapeados` = RFs com task E código + tasks com código
- `total itens` = total de RFs + total de tasks

### Checklist de Validação

| Critério | Peso | Passa se |
|----------|------|----------|
| RF→Task mapping | 30% | ≥80% RFs têm task |
| Task→Code mapping | 30% | ≥80% tasks têm código |
| Zero orphaned specs | 20% | 0 RFs sem task |
| Zero orphaned code | 20% | 0 módulos sem RF (aceitar infra) |

### Bloqueios

| Score | Status | Ação |
|-------|--------|------|
| ≥80% | ✅ PASS | Pode prosseguir para archive |
| 60-79% | ⚠️ WARN | Revisar gaps, pode archive com justificativa |
| <60% | ❌ FAIL | Voltar para implementação, completar gaps |

## Formato do Relatório

```markdown
## Traceability Report — {feature}

### Spec → Code
| RF | Task | Code | Status |
|----|------|------|--------|
| RF-01 | T-01 | UserService.java | ✅ Mapped |
| RF-02 | T-02 | — | ⚠️ No code |
| RF-03 | — | — | ❌ Orphaned |

### Code → Spec
| Module | RF | Status |
|--------|----|----|
| UserService.java | RF-01 | ✅ Mapped |
| ValidationUtil.java | — | ⚠️ Infra (ok) |
| LegacyHelper.java | — | ❌ Orphaned |

### Score
- RF→Task: 85% (11/13)
- Task→Code: 90% (9/10)
- Orphaned specs: 1 (RF-03)
- Orphaned code: 1 (LegacyHelper)
- **Overall: 82% ✅ PASS**

### Gaps
1. RF-03: Sem task — adicionar task ou remover da SPEC
2. LegacyHelper: Sem RF — documentar como infra ou remover
```
