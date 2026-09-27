# Delta Specs: [Feature Name]

> **Feature ID**: `[kebab-case-feature-id]`
> **SPEC Source**: `.spec/[feature-name]/SPEC.md`
> **Purpose**: Rastrear exatamente o que muda em documentos existentes quando esta feature for arquivada.

---

## Instruções de Uso

Esta seção DEVE ser incluída no final do `SPEC.md` de toda feature.  
No momento do `archive`, o `sdd-compiler` usa este delta para atualizar os documentos alvo (`.agents/rules/`, `AGENTS.md`).

**Tipos de mudança**:
- `ADDED` — nova seção/conteúdo não existia antes
- `MODIFIED` — substitui conteúdo existente (indicar a seção exata)
- `REMOVED` — conteúdo será movido para `## Deprecated` no doc alvo

---

## Delta: `.agents/rules/backend-coding-standards.md`

### ADDED
```
[Cole aqui o conteúdo novo a ser adicionado neste documento]
```
> **Inserir após**: `[Nome da seção existente após a qual este conteúdo entra]`

### MODIFIED
> **Seção alvo**: `[ex: ## 3. Módulo Task]`
```
[Cole aqui o conteúdo NOVO que substitui a seção acima]
```

### REMOVED
> **Seção alvo**: `[ex: ## 5. Legacy Flow]`
> **Motivo**: `[Por que está sendo removido]`

---

## Delta: `AGENTS.md`

### ADDED
```
[Conteúdo a adicionar]
```
> **Inserir após**: `[seção]`

### MODIFIED
> **Seção alvo**: `[seção]`
```
[Conteúdo substituto]
```

---

## Delta: `[outro doc se necessário]`

### ADDED / MODIFIED / REMOVED
*(Repetir estrutura acima para cada documento afetado)*

---

## Resumo do Impacto

| Documento | ADDED | MODIFIED | REMOVED |
|-----------|-------|----------|---------|
| `.agents/rules/backend-coding-standards.md` | [n seções] | [n seções] | [n seções] |
| `AGENTS.md` | [n seções] | [n seções] | [n seções] |
