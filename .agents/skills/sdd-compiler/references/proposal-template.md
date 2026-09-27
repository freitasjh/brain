# Change Proposal: [Feature Name]

> **ID**: `[kebab-case-feature-id]`
> **Status**: `Draft | Under Review | Approved | Rejected`
> **Author**: [name]
> **Date**: [YYYY-MM-DD]
> **Source**: [EXPLORATION.md se existir | descrição direta]

---

## 1. Problem Statement
*O que está quebrado, faltando ou causando fricção? Seja específico.*

- **Contexto**: [Qual parte do sistema / fluxo de usuário é afetada?]
- **Impacto atual**: [O que acontece sem essa feature? Qual é o custo?]
- **Evidência**: [Bug report, feedback de usuário, métrica, etc.]

---

## 2. Proposed Solution
*O que será construído. Frase de uma linha + detalhes.*

**Resumo**: [Uma frase clara do que será implementado]

**Abordagem**:
- [Ponto técnico 1]
- [Ponto técnico 2]
- [Alternativas descartadas e por quê]

---

## 3. Scope

### In Scope
- [ ] [O que será entregue nesta mudança]

### Out of Scope
- [O que NÃO será feito agora e por quê]

---

## 4. Architecture Impact

| Aspecto | Impacto | ADR Necessário? |
|---------|---------|-----------------|
| Backend (módulos afetados) | [ex: scheduling/api, scheduling/impl] | [Sim/Não] |
| Frontend (componentes/stores) | [ex: AnamnesisStore, AnamnesisForm.vue] | [Sim/Não] |
| Banco de dados (migrations) | [ex: nova tabela, alter column] | [Sim/Não] |
| Multi-tenancy | [impacto no isolamento de tenant] | [Sim/Não] |
| Segurança / RBAC | [novas permissões, roles] | [Sim/Não] |
| Eventos (Kafka/async) | [novos eventos publicados/consumidos] | [Sim/Não] |

---

## 5. Delta Preview
*Quais documentos existentes serão afetados pelo SPEC desta feature.*

| Documento Alvo | Tipo de Mudança | Seção Afetada |
|----------------|-----------------|---------------|
| `.agents/rules/backend-coding-standards.md` | ADDED / MODIFIED / REMOVED | [seção] |
| `AGENTS.md` | ADDED / MODIFIED / REMOVED | [seção] |

---

## 6. Risk Assessment

| Risco | Probabilidade | Impacto | Mitigação |
|-------|---------------|---------|-----------|
| [ex: Quebra de multi-tenancy] | Alta/Média/Baixa | Alto/Médio/Baixo | [estratégia] |
| [ex: Performance em produção] | Alta/Média/Baixa | Alto/Médio/Baixo | [estratégia] |

---

## 7. Success Criteria
*Como sabemos que a feature está pronta e correta?*

- [ ] [Critério funcional 1 — ex: Médico consegue salvar template de anamnese]
- [ ] [Critério técnico 1 — ex: Testes integração passando, zero erros no console]
- [ ] [Critério de qualidade — ex: Coverage ≥ 80% no service]

---

## 8. Completion Metadata
*Preenchido durante `archive`.*

- **Archived on**: —
- **Duration**: —
- **Archive path**: —
- **Files changed**: —
