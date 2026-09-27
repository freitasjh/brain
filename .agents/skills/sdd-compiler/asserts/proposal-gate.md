# PROPOSAL Validation Gate (Asserts)

Todo `PROPOSAL.md` deve ser validado antes de avançar para `generate-spec`.

## 1. Clareza do Problema
- [ ] **Problem Statement**: Descreve situação atual concreta, não abstrata. Tem evidência ou motivação real.
- [ ] **Impacto**: Quantifica ou qualifica o custo do problema atual.

## 2. Escopo
- [ ] **In Scope definido**: Lista específica do que SERÁ entregue (não vaga).
- [ ] **Out of Scope definido**: Explicita o que NÃO será feito. Previne scope creep.
- [ ] **Escopo único**: Mudança foca em uma unidade lógica. Não mistura features distintas.

## 3. Viabilidade Arquitetural
- [ ] **Architecture Impact preenchido**: Todos os aspectos relevantes marcados (backend, frontend, DB, tenancy, RBAC, eventos).
- [ ] **Conflitos identificados**: Nenhuma colisão com `impl → impl` dependency ou regras DDD violadas.
- [ ] **RBAC**: Impacto nas permissões e `@HasPermission` avaliado (mesmo que "sem impacto" — deve ser explícito).

## 4. Delta Preview
- [ ] **Documentos alvo listados**: Pelo menos um documento (`.agents/rules/` ou `AGENTS.md`) identificado como afetado.
- [ ] **Tipo de mudança especificado**: Cada documento tem ADDED/MODIFIED/REMOVED definido.

## 5. Critérios de Sucesso
- [ ] **Success Criteria testáveis**: Cada critério é verificável (binário: passou/falhou).
- [ ] **Critério funcional presente**: Ao menos um critério do ponto de vista do usuário.
- [ ] **Critério técnico presente**: Ao menos um critério técnico (testes, build, cobertura).

## ❌ Bloqueadores (não avança se qualquer um destes falhar)
- [ ] Problem Statement não é "implementar X" — deve ser o problema, não a solução.
- [ ] Scope não inclui tudo que o Architecture Impact lista como afetado.
- [ ] Success Criteria são subjetivos ou não verificáveis.
