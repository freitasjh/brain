# VERIFY — brain-multi-project-hooks

Data: 2026-10-08 · HEAD: `9b9e7f4` · Asserts: `.agents/skills/sdd-compiler/asserts/verify-gate.md`
Harness declarado no HEAD: `cargo test --workspace` 455 testes 0 fail · `cargo clippy --workspace --all-targets -- -D warnings` 0 · `.venv/bin/python -m pytest src/tests/ -q` 124 passed + 2 legadas (H1.4). Esta VERIFY consome esse harness; nenhum teste foi re-executado aqui e nenhum serviço/DB de produção foi tocado.

```
VERIFY STATUS: APPROVED ✅
```

## Dimensão 1: Completeness ✅

- [x] Tasks 100%: T1.1–T6.4 + H6 todos `[x]` no TASKS.md (marcados nesta VERIFY).
- [x] Sem TODOs: `grep TODO` em `crates/brain-cli/src/config.rs` + `resolve.rs` vazio.
- [x] Sem tasks puladas: nenhuma marcada N/A.
- [x] BDD cobertos por teste de binário real (sem rede/Ollama real, H2.3):
  - P1: roundtrip ler→gravar (`write_config`/`config` em `hook_project_resolution.rs`), JSON inválido nunca clobberado (`a_malformed_config_is_reported_and_never_clobbered`), `BRAIN_DIR` criado (`a_missing_brain_dir_is_created_rather_than_failing_the_hook`), typo não é recusa (`a_typo_in_the_config_is_not_a_refusal_and_is_left_on_disk`, `an_unrecognised_motivo_is_not_a_refusal`).
  - P2: config precede git/nome (`an_explicit_project_ignores_a_config_that_disagrees`, `without_project_the_config_resolves_and_the_origin_is_reported`), remote (`without_project_a_matching_remote_resolves`, `the_remote_wins_over_a_directory_naming_a_different_project`), dirname exato + não-prefixo (`without_project_an_exact_directory_name_resolves`, `a_directory_that_only_shares_a_prefix_resolves_to_itself_not_the_neighbour`), pergunta→grava / recusa→`{projeto:null,motivo:"recusado"}` sem re-perguntar (`a_recorded_refusal_writes_nothing_and_does_not_ask_again`, `a_recorded_refusal_is_left_exactly_as_it_was`), `unresolved` re-pergunta (`an_unanswerable_question_writes_nothing_and_records_no_refusal`, `a_recorded_refusal_…`/`a_directory_left_unanswerable_is_asked_again_on_the_next_run`).
  - P3: `--project` explícito byte-idêntico + origem em linha própria (`an_explicit_project_produces_exactly_the_output_it_always_did`, `the_origin_is_reported_on_its_own_line`, R-06), `disabled` não pergunta/não busca (`a_disabled_directory_writes_nothing_and_reports_its_origin`), stdin pipado não trava (`a_piped_stdin_does_not_stall_the_hook`), regressão `cli_e2e.rs::e2e_concurrent_hooks_never_lose_a_session_event` (12×8) presente e verde no harness.
  - P4: query = mensagem do usuário (`the_injected_context_is_the_user_question_and_the_project_is_a_filter`), projeto como filtro nunca como termo (`the_project_name_never_reaches_the_query`), vazio → `INJECT: (no context found)` + stderr (`a_payload_without_a_question_degrades_to_project_context_and_says_so`), Ollama fora → FTS pontua (`with_ollama_down_the_question_still_scores_through_fts`).
  - P5: `kiro` alvo válido + desconhecido lista kiro (`setup_kiro_writes_the_kiro_hook_artifact`, `an_unknown_target_is_refused_and_the_message_lists_kiro`), re-pergunta IDE inválida, `--dry-run` nada escreve (`dry_run_writes_nothing_either_to_disk_or_to_the_ide`, `dry_run_with_an_explicit_answer_records_nothing`), non-tty usa default + aviso (`a_non_tty_run_keeps_the_default_and_warns`, `a_terminal_with_nobody_answers_eventually`, `yes_is_silent_and_records_nothing`), `--project`/`--decline-project` + recusa mútua (`an_explicit_answer_beats_yes`, `declining_records_the_conised_refusal_and_the_hook_then_skips`, `naming_a_project_and_declining_at_once_is_refused`), concorrência (`two_concurrent_setups_do_not_lose_each_others_decisions`).
  - P6: plugin opencode (`setup_opencode_installs_the_session_plugin`, `running_setup_twice_installs_one_plugin_and_leaves_the_neighbours_alone`, `force_refreshes_the_plugin_and_keeps_the_previous_copy`), kiro `PromptSubmit`+agent (`the_generated_artifact_uses_prompt_submit_and_agent_stop`, `setup_kiro_writes_the_kiro_hook_artifact`), binário Rust no hook (`a_stale_binary_in_the_plugin_is_reported`, `the_kiro_command_records_every_session_it_is_run_for`), sem MCP nada na conversa + exit 0 (`a_degraded_hook_writes_nothing_and_exits_zero`).
- [x] H6: harness acima todo verde; `brain setup --help` mostra `opencode | kiro | systemd | shell | project | all` (verificado no binário dev nesta sessão); 6 mutações aplicadas e pegas (declaradas no TASKS; comentários de mutação presentes nos testes, ex. swap passos 2/3 e `recusado` sem suprimir).

## Dimensão 2: Correctness ✅

Build/testes: consumidos do harness do HEAD (não re-rodados aqui por ordem explícita de não tocar serviços/DB).

| Req | Implementação | Teste |
|---|---|---|
| RF-01 config.json global (`BRAIN_DIR`, `brain_projects{abs:{projeto,motivo,desabilitado}}`) | `crates/brain-cli/src/config.rs` (765 L: `load`, `save_entry` com lock `fs2`, RF-07.1 degradado sem `BRAIN_DIR`/`HOME`, RF-07.2 lock, motivo conizado RF-03.1) | P1 acima |
| RF-02 cascata config→git→nome→pergunta + `desabilitado`/`recusado`/`unresolved` (8 origens) | `crates/brain-cli/src/resolve.rs` (784 L) | P2 acima |
| RF-03 `--project` opcional, origem em `hook resolve …`, `hook ok …` byte-idêntica (R-06) | `main.rs` (`Option<String>`, `hook resolve project={} origin={}`, nota sempre `sessoes/…`) | P3 acima |
| RF-03.1 / RF-07.1 / RF-07.2 / RF-07.3 | `config.rs` + `resolve.rs` + `main.rs:752-781` (git `stdin=null`, sem `read_line` no hook, pergunta fora do hook) | typo/motivo + `a_hook_without_home_or_a_writable_brain_dir_still_exits_zero`, `a_read_only_brain_dir_degrades…` |
| RF-04 pergunta real via `--question` (+fallback `payload.question`, conflito avisado), sem query fixa, filtro não termo, vazio honesto, `OR` na injeção (`AND` mantido no `Store::search`) | `main.rs:908 inject_query` + `fts5_match_expr_joined(q,"OR")`, `main.rs:1213-1238` | P4 acima |
| RF-04b kiro `PromptSubmit` (não `SessionStart`; `SessionStart` mantido em `KIRO_TRIGGERS` como nome válido) | artefato kiro em `setup.rs` | `the_generated_artifact_uses_prompt_submit_and_agent_stop` |
| RF-05 kiro alvo + pergunta IDE (exatamente uma IDE; `all` não inclui kiro; alvo nomeado não é bundle; `--project`/`--decline-project` mutuamente exclusivos) | `setup.rs` (+1452 L) | P5 acima |
| RF-06 `--dry-run` exibe sem escrever; `--force` sem confirmação por arquivo | `setup.rs` (`write_file` DryRun, `shell_target`/`force`) | `dry_run_*`, `force_refreshes_…` |
| RF-07 degradação silenciosa (sem MCP/Ollama → exit 0, stderr, nada na conversa) | `main.rs:1213+` + hook paths | `a_degraded_hook_…`, `with_ollama_down_…` |
| RF-08 `.py` fora do caminho suportado, remoção física na Fase C | sem remoção neste lote (conforme SPEC) | — (ausência intencional) |
| R-01/R-01b/R-02/R-03/R-04 | plugin JS opencode (message+part por `messageID`, duas ordens; limite `TextPart` honesto), resolução no binário, config global, diff só em `crates/brain-cli/` + hooks/artefatos, zero DDL | P6 + `a_project_name_can_never_produce_a_note_outside_sessoes` |
| R-05/R-06/R-07/R-08 | harness + R-06 + non-interactive + steering mantido (259 L, commit posterior) | H6 + P3/P5 |

Bloqueadores absolutos: nenhum (build ok, 0 falhas, todo RF com implementação + teste, sem vazamento de tenant — projeto é filtro, isolamento `mobile*` coberto no lote `d5f8939`).

## Dimensão 3: Coherence ✅

- ADR-05 (arquivo, não banco): `config.rs` existe; nenhum DDL (`data/brain.db` intocado; `projects` reutilido, `note_projects` intocado).
- ADR-06 (config→git→nome→pergunta): ordem implementada em `resolve.rs` e travada por teste de mutação (swap 2/3 pega).
- ADR-07 (pergunta, não query fixa): `inject_query` + remoção da literal em `main.rs:880/883`; filtro vs termo separado.
- ADR-08 (global, não por projeto): zero arquivo por projeto; `BRAIN_DIR`/`~/.brain` único.
- Sem vazamento de camada: resolução/config/setup vivem em `brain-cli`; `brain-store` só ganhou filtro de projeto no lote `d5f8939`; `brain-mcp`/`brain-web` intocados (unico `rmcp_service.rs` no tree é churn alheio, fora do commit).
- PLAN §2 desatualizado registrado como SPEC-manda: SPEC §2 lista 3 decisões medidas contrárias ao PLAN (vazio em vez de texto fixo; `OR` em vez de `AND`; `--question` em vez de payload) + RF-04b (`PromptSubmit` em vez de `SessionStart`). PLAN mantido como está; SPEC prevalece.

## Delta Specs

SPEC não tem seção Delta explícita. Impacto real em docs (`AGENTS.md`/`README.md`/skills) existe no working tree mas é entulho de outro lote, FORA desta feature → registrado aqui como **docs-pendentes em lote próprio** e INTENCIONALMENTE não mexido (ver task follow-up).

## Observações

- `brain setup --help` verificado nesta sessão: `TARGET … opencode | kiro | systemd | shell | project | all`.
- Follow-ups intencionais fora deste lote: `.spec/tech-debts/multi-project-hooks-followup/TASKS.md` (untracked).
