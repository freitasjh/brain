# Tech Debt — project-shared followup (B3 + B4)

Origem: code-reviewer B3 + B4. Escopo: governança. Nenhum código alterado neste arquivo.

## TD-F1 — pid.to_string() binda TEXT contra INTEGER (P3)

- Local: `crates/brain-store/src/lib.rs:1870`
- Problema: `pid.to_string()` 2x binda TEXT contra coluna INTEGER. Funciona por affinity, frágil.
- Fix proposto (futuro): trocar por `Vec<Box<dyn ToSql>>` com `i64`.
- Severidade: P3 (cosmético/robustez)
- Esforço estimado: S (~0.5h + testes)
- Aceite:
  - [ ] bind usa tipo INTEGER nativo
  - [ ] `cargo test -p brain-store` passa
  - [ ] sem mudança de comportamento em `brain_project_notes`

## TD-F2 — if aninhado !missing_project + if let (P3)

- Local: `crates/brain-store/src/lib.rs:1818`
- Problema: `if !missing_project` aninhado com `if let`. Só estilo/legibilidade.
- Fix proposto (futuro): limpar com let-chains ou early-return.
- Severidade: P3 (estilo)
- Esforço estimado: S (~0.25h)
- Aceite:
  - [ ] lógica idêntica, leitura simplificada
  - [ ] `cargo test -p brain-store` passa

## TD-F3 — typo docs "camadas corretos?" (P3)

- Local: `.agents/rules/BRAIN.MCP.md` + SKILLs (checklist agentes)
- Problema: concordância errada ("camadas corretos?").
- Fix proposto (futuro): corrigir na próxima passada de docs.
- Severidade: P3 (cosmético)
- Esforço estimado: XS (~5min)
- Aceite:
  - [ ] texto corrigido para "camadas corretas?"
  - [ ] grep não retorna mais a forma errada

## TD-F4 — store.project com typo cria projeto silencioso (P2)

- Problema: `project` inexistente em `brain_store` cria projeto novo sem aviso (`mobile` ≠ `mobile-erp` ≠ `mobile_erp` ≠ `progaterp`). Typo vira fragmentação silenciosa.
- Fix proposto (a decidir): avaliar guard — sugerir existentes via `project_list`, ou warning/log, ou modo estrito opt-in. Produto decide.
- Severidade: P2 (degrada UX/integridade, sem perda de dado)
- Esforço estimado: M (design + decisão produto, ~2-4h)
- Aceite:
  - [ ] decisão produto registrada (sugerir vs warning vs estrito)
  - [ ] comportamento documentado em `BRAIN.MCP.md` + skill `brain_store`
  - [ ] teste cobrindo o comportamento escolhido

## TD-F5 — backfill 28 sessoes/mobile-erp/* NULL via brain_project_link (P3)

- Problema: 28 notas `sessoes/mobile-erp/*` com `project_id NULL` e sem link ficam invisíveis sob filtro `project=mobile-erp`.
- Fix proposto (futuro, com confirmação): executar backfill via `brain_project_link`, nunca migração silenciosa.
- Severidade: P3 (dados existentes, sem perda)
- Esforço estimado: S (~0.5h, manual + verificação)
- Aceite:
  - [ ] confirmação explícita do usuário antes de executar
  - [ ] 28 notas linkadas a `mobile-erp`
  - [ ] `brain_project_notes(mobile-erp)` retorna as 28
  - [ ] sem alteração de `project_id` (só link N:N)
