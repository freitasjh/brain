# 10-Point Summary Template

## Formato Base

```markdown
## ✅ {COMMAND_NAME} Completed

### 🎯 Key Decisions Made (Top 3)
1. {DECISION_1} — **Rationale:** {WHY_1}
2. {DECISION_2} — **Rationale:** {WHY_2}
3. {DECISION_3} — **Rationale:** {WHY_3}

### 📋 What Was Generated
- {ARTIFACT_1}: {DESCRIPTION_1}
- {ARTIFACT_2}: {DESCRIPTION_2}

### 🔍 Important Items to Review (Top 3)
1. {ITEM_1} — Why it matters: {REASON_1}
2. {ITEM_2} — Potential impact: {IMPACT_2}
3. {ITEM_3} — How it affects design: {EFFECT_3}

### ⚠️ Watch Out For (Top 2)
- {ISSUE_1} — **How to avoid:** {AVOIDANCE_1}
- {ISSUE_2} — **How to avoid:** {AVOIDANCE_2}

### 🔄 What This Enables Next
- **Option 1:** {NEXT_STEP_1} — Best if: {CONDITION_1}
- **Option 2:** {NEXT_STEP_2} — Best if: {CONDITION_2}

📊 **Feature Status:** {FEATURE_NAME} ({STAGE}) → Next: {NEXT_FEATURE}
   Progress: {PROGRESS_BAR} {PERCENTAGE}% | Completed: {DONE} of {TOTAL} | Dependencies: {DEPS}

**Your options:** [A] {OPTION_A} [B] {OPTION_B} [C] {OPTION_C} [D] {OPTION_D}
```

## Valores de Stage

| Stage | Progress | Indicador |
|-------|----------|-----------|
| Specifying | 20% | `●○○○○` |
| Planning | 40% | `●●○○○` |
| Tasking | 60% | `●●●○○` |
| InProgress | 80% | `●●●●○` |
| Complete | 100% | `●●●●●` |

## Formato Brief (para inline)

```
📊 **{name}** ({stage}) | {progress}% | {done}/{total} features
```

## Formato Detailed (dashboard)

```markdown
📊 Project Feature Status Dashboard

🎯 CURRENT FEATURE
├─ {name} ({stage} - {progress}%)
│  ├─ ✅ Requirements specified
│  ├─ 🔄 Implementation plan in progress
│  ├─ ⏸️  Tasks not started
│  └─ ⏸️  Implementation not started
│  Dependencies: {deps}

✅ COMPLETED FEATURES ({n})
└─ {name} (100% complete)

📋 UPCOMING FEATURES ({n})
├─ {name} (depends on: {dep})
└─ {name} (depends on: {dep})

⚠️  BLOCKED FEATURES ({n})
{list blocked with reasons}
```

## Placeholders

| Placeholder | Descrição | Exemplo |
|-------------|-----------|---------|
| `{COMMAND_NAME}` | Nome do comando SDD executado | `generate-spec` |
| `{DECISION_N}` | Decisão tomada | "Flyway para migrations" |
| `{WHY_N}` | Justificativa da decisão | "Controle versionado" |
| `{ARTIFACT_N}` | Arquivo gerado | "SPEC.md" |
| `{DESCRIPTION_N}` | Conteúdo do artefato | "12 requisitos, 8 ADRs" |
| `{ITEM_N}` | Item para review | "RF-05: invite flow" |
| `{ISSUE_N}` | Potencial problema | "CompanyUser composite key" |
| `{AVOIDANCE_N}` | Como evitar | "Usar @IdClass" |
| `{NEXT_STEP_N}` | Próximo passo | "Run /sdd.plan" |
| `{CONDITION_N}` | Condição para opção | "Requisitos claros" |
| `{FEATURE_NAME}` | Nome da feature | "tenant-user-management" |
| `{STAGE}` | Estágio atual | "Specified" |
| `{PROGRESS_BAR}` | Barra visual | "●●○○○" |
| `{PERCENTAGE}` | Percentual | "40" |
| `{DONE}` | Features completas | "1" |
| `{TOTAL}` | Total de features | "3" |
| `{DEPS}` | Dependências | "None" ou "auth ✅" |
