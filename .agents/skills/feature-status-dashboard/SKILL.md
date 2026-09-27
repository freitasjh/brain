---
name: feature-status-dashboard
description: "Automatic feature status tracking and dashboard. Reads workflow-state.json, calculates progress, visualizes dependencies. Brief status in every summary; detailed dashboard on demand."
---

# Feature Status Dashboard

Rastreamento automático de progresso de features com dashboard visual.

## Quando Usar

| Comando | Tipo |
|---------|------|
| Após qualquer 10-point summary | Brief (inline) |
| "show status" / "show features" | Detailed |
| "what blocks X?" | Dependency check |
| "add feature X" | Management |
| "move X before Y" | Reorder |
| "skip X" | Defer |

## Fonte de Verdade

`workflow-state.json` na raiz do projeto.

### Schema de Features

```json
{
  "features": [
    {
      "name": "feature-name",
      "status": "specified|planned|tasked|in_progress|complete",
      "progress": 20,
      "depends_on": ["other-feature"],
      "blocks": ["blocked-feature"],
      "started_at": "2026-09-08T10:00:00Z",
      "completed_at": null,
      "artifacts": {
        "spec": ".spec/feature-name/SPEC.md",
        "plan": ".spec/feature-name/PLAN.md",
        "tasks": ".spec/feature-name/TASKS.md"
      }
    }
  ],
  "feature_management": {
    "total": 5,
    "completed": 2,
    "current": "feature-name"
  }
}
```

## Renderização

### Brief Status (inline)

Incluir no final de todo 10-point summary:

```
📊 **Feature Status:** {name} ({stage}) → Next: {next}
   Progress: {bar} {pct}% | Completed: {done}/{total} | Dependencies: {deps}
```

### Detailed Dashboard

```markdown
📊 Project Feature Status Dashboard

🎯 CURRENT FEATURE
├─ {name} ({stage} - {pct}%)
│  ├─ ✅ Requirements specified
│  ├─ 🔄 Implementation plan in progress
│  ├─ ⏸️  Tasks not started
│  └─ ⏸️  Implementation not started
│  Blockers: {blockers}
│  Dependencies: {deps}

✅ COMPLETED FEATURES ({n})
└─ {name} (100% complete)

📋 UPCOMING FEATURES ({n})
├─ {name} (depends on: {dep})
└─ {name} (depends on: {dep})

⚠️  BLOCKED FEATURES ({n})
{list with reasons}
```

## Cálculo de Progresso

| Status | Progress | Indicador |
|--------|----------|-----------|
| `specified` | 20% | `●○○○○` |
| `planned` | 40% | `●●○○○` |
| `tasked` | 60% | `●●●○○` |
| `in_progress` | 80% | `●●●●○` |
| `complete` | 100% | `●●●●●` |

Progresso manual override: campo `progress` numérico (0-100).

## Natural Language Management

### Detectar Intenção

| Input do usuário | Intenção | Ação |
|------------------|----------|------|
| "add feature X" | Nova feature | Criar entrada no workflow |
| "move X before Y" | Reorder | Reordenar array |
| "skip X for now" | Defer | Status "deferred" |
| "we finished X" | Complete | Status "complete", 100% |
| "what blocks X?" | Dependency check | Listar dependências |
| "show features" | Dashboard | Renderizar detailed |
| "let's do X first" | Priority | Mover para current |

### Fluxo: Add Feature

```
1. Detectar: "add feature for notifications"
2. Perguntar: Priority? Dependencies? Description?
3. Criar entry no workflow-state.json
4. Criar pasta .spec/{name}/
5. Mostrar dashboard atualizado
```

### Fluxo: Reorder

```
1. Detectar: "move auth before profile"
2. Ler ordem atual
3. Propor nova ordem
4. Pedir confirmação
5. Atualizar workflow-state.json
6. Mostrar dashboard atualizado
```

### Fluxo: Dependency Check

```
1. Detectar: "can we start profile?"
2. Ler dependências de profile
3. Verificar status de cada dependência
4. Reportar:
   - ✅ auth (complete)
   - ✅ db-setup (complete)
   → Ready to proceed!
   OU
   - ⏸️ auth (in_progress)
   → Blocked until auth complete
```

## Integração com Orquestrador

### Após Cada Fase SDD

```python
# 1. Atualizar status da feature no workflow
update_feature_status(feature_name, new_status)

# 2. Incluir brief status no 10-point summary
brief = render_brief_status(feature_name)

# 3. Se summary revelou completion, atualizar
if all_tasks_done:
    mark_feature_complete(feature_name)
```

### Sob Demanda

```python
# Usuário: "show status"
dashboard = render_detailed_dashboard()
print(dashboard)

# Usuário: "what blocks X?"
blockers = check_dependencies(feature_name)
print(blockers)
```

## Triggers de Atualização Automática

| Evento | Ação |
|--------|------|
| `generate-spec` completo | status → `specified` |
| `generate-plan` completo | status → `planned` |
| `generate-tasks` completo | status → `tasked` |
| Primeira task implementada | status → `in_progress` |
| Todas tasks + verify pass | status → `complete` |
| Feature movida | reordenar array |
| Feature removida | marcar `deferred` |

## Exemplo: Dashboard Detalhado

```markdown
📊 Project Feature Status Dashboard

🎯 CURRENT FEATURE
├─ tenant-user-management (InProgress - 80%)
│  ├─ ✅ Requirements specified
│  ├─ ✅ Implementation plan
│  ├─ ✅ Tasks broken down
│  └─ 🔄 Implementation in progress (18/21 tasks)
│  Blockers: None
│  Dependencies: company-rich ✅, identity-rich ✅

✅ COMPLETED FEATURES (2)
├─ company-rich-registration (100% complete)
└─ identity-rich-user (100% complete)

📋 UPCOMING FEATURES (1)
└─ document-management (depends on: tenant-user-management)

⚠️  BLOCKED FEATURES (0)
None — all features unblocked!

Progress: [●●●●○] 80% | 2 of 5 complete | Next: document-management
```
