# Feature Status Templates

## Brief Status (Inline)

```
📊 **{name}** ({stage}) | {progress}% | {done}/{total} features
```

**Exemplo:**
```
📊 **tenant-user-management** (InProgress) | 80% | 2/5 features
```

## Detailed Dashboard

```markdown
📊 Project Feature Status Dashboard

🎯 CURRENT FEATURE
├─ {name} ({stage} - {progress}%)
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

## Dependency Check

```markdown
📊 Can we start {feature}?
   Checking dependencies...
   ✅ {dep1} (complete)
   ✅ {dep2} (complete)
   ⏸️ {dep3} (in_progress)

   ❌ Blocked until {dep3} complete
```

OU

```markdown
📊 Can we start {feature}?
   Checking dependencies...
   ✅ {dep1} (complete)
   ✅ {dep2} (complete)

   ✅ All dependencies satisfied! Ready to proceed.
```

## Progress Bar Characters

| Progress | Bar |
|----------|-----|
| 0% | `○○○○○` |
| 20% | `●○○○○` |
| 40% | `●●○○○` |
| 60% | `●●●○○` |
| 80% | `●●●●○` |
| 100% | `●●●●●` |

## Stage Labels

| Status | Label |
|--------|-------|
| `specified` | Specifying |
| `planned` | Planning |
| `tasked` | Tasking |
| `in_progress` | In Progress |
| `complete` | Complete |
| `deferred` | Deferred |

## Feature Management Commands

### Add Feature
```markdown
📋 New Feature: {name}
   Priority: {priority}
   Dependencies: {deps}
   Description: {desc}

   Adding to feature list...
   {updated_dashboard}
```

### Reorder Features
```markdown
📋 Current Order:
   1. {feature1} ({status1})
   2. {feature2} ({status2})
   3. {feature3} ({status3})

📋 Proposed Order:
   1. {feature1} ({status1})
   2. {feature3} ({status3}) ← moved up
   3. {feature2} ({status2}) ← moved down

   Confirm? (yes/no)
```

### Remove Feature
```markdown
⚠️  Warning: Removing '{name}'
   Dependencies affected: {affected}
   Are you sure? (yes/no)
```
