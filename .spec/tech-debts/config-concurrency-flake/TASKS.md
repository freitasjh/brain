# Tech Debt — flake `config::concurrency` (P2)

Origem: review Bloco 3 iteração 1/3 (§6.2 must-fix #2) sobre
`.spec/brain-specialist/SU-04-CHECKLIST.md:15-20`. Nenhum código alterado
neste arquivo.

## TD-C1 — flake arquivado "não-P1" sem artefato (P2)

- Teste: `config::concurrency::concurrent_writers_do_not_lose_each_others_entries`
- Ocorrência: falhou **1x** sob carga paralela do workspace (`cargo test
  --workspace`); verde isolado e verde no re-run completo (508/0).
- Causa raiz conhecida: contenção sob paralelismo do workspace — hipótese é
  timing/lock sob carga, **não** perda real de escrita (isolado sempre verde).
  Causa exata ainda aberta; é o que este TD rastreia.
- Violação processual: arquivar como "não P1 por ora" sem artefato viola
  H1.6 (protocolo de falha: parar, causa raiz, re-rodar suite completa,
  brain_store da lição) e H2.2 (detecção antes de começar: qualquer falha
  não-declarada vira TASKS de saneamento antes da feature). Este arquivo é o
  artefato que faltava.
- Severidade: P2 (degrada confiança no harness; sem perda de dado; 1 ocorrência)
- Esforço estimado: S (~1h investigação + estabilização ou quarentena justificada)

### Tracking

- [ ] 1ª ocorrência registrada aqui (2026-10-14, SU-04 Bloco 3)
- [ ] causa exata isolada (reproduzir sob carga; busy-handler / retry / serialização do teste)
- [ ] fix ou quarentena justificada por escrito (nunca `#[ignore]` silencioso — H2.1)
- [ ] `cargo test --workspace` verde + re-run sob carga verde

### Critério de promoção P1 (non-negotiable)

- **2ª ocorrência → P1 imediato**: abrir `.spec/bugs/config-concurrency-flake/TASKS.md`,
  parar feature em curso (H1.6), sanear antes de prosseguir (H2.2).
- Contador: `ocorrências = 1` (2026-10-14). Atualizar aqui a cada ocorrência
  com data + log + contexto de carga.
