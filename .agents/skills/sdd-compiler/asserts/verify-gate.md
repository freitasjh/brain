# VERIFY Validation Gate (Asserts)

Verificação pós-implementação obrigatória antes do `archive`. Três dimensões devem ser ✅ para avançar.

## Dimensão 1: Completeness (Completude)

- [ ] **Tasks 100%**: Todos os checkboxes em `TASKS.md` marcados como concluídos.
- [ ] **Sem TODOs**: `grep -r "TODO\|FIXME\|HACK" [caminho-implementado]` retorna vazio ou itens pre-existentes documentados.
- [ ] **Sem tasks puladas**: Nenhuma task marcada como "N/A" sem justificativa explícita no VERIFY.md.
- [ ] **BDD cenários cobertos**: Todos os cenários Sucesso/Falha/Borda das tasks têm testes correspondentes.

## Dimensão 2: Correctness (Correção)

- [ ] **Build limpo**: `mvn clean install` (backend) OU `npm run build` (frontend) sem erros.
- [ ] **Testes passando**: `mvn test` OU `npm run test:run` — zero falhas.
- [ ] **Cobertura mínima**: Services ≥ 80%, Domain ≥ 90% (backend). Confirmar com relatório.
- [ ] **Requisitos funcionais**: Cada item de `SPEC.md § Functional Requirements` tem correspondência verificável no código.
- [ ] **RBAC correto**: Permissões implementadas e testadas (se feature tem RBAC).
- [ ] **Multi-tenancy**: Testes de isolamento de tenant passando (se feature toca persistência).

## Dimensão 3: Coherence (Coerência)

- [ ] **ADRs refletidos**: Decisões arquiteturais do `SPEC.md § ADRs` visíveis no código (naming, estrutura, padrões).
- [ ] **Sem vazamento de camada**: Controllers expõem DTOs (não entities). `impl` não depende de outro `impl`.
- [ ] **Naming conventions**: Classes, métodos e variáveis seguem padrões em `.agents/rules/backend-coding-standards.md`.
- [ ] **Frontend DevTools** (se mudança frontend):
  - [ ] Zero `[ERROR]` no console.
  - [ ] Zero `[WARN]` relevantes no console.
  - [ ] Requests HTTP com status esperados (200/201).
  - [ ] Payloads corretos nas chamadas de API.

## Resultado Final

```
VERIFY STATUS: APPROVED ✅ | BLOCKED 🔴

Bloqueadores encontrados:
- [listar gaps se BLOCKED]

Observações:
- [notas relevantes mesmo em APPROVED]
```

## ❌ Bloqueadores Absolutos (BLOCKED automático)
- Build com erros de compilação.
- Testes falhando.
- Requisito da SPEC sem implementação correspondente.
- Vazamento de tenant detectado em testes de integração.
