# TASKS — hook-empty-question-fallback

## Bug
- `session-start` sem pergunta (`USER_PROMPT` vazio no kiro, payload sem `question`) imprime `INJECT: (no context found)` mesmo com projeto resolvido e notas existentes.
- Causa raiz: `hook_handle` (`crates/brain-cli/src/main.rs:1213`) degrada para query vazia via `inject_query("")` e roda `Store::search("", ...)` — FTS vazio retorna 0 em ambos os blocos → cai no marker vazio. Projeto era só filtro do search, nunca fonte de panorama.
- Evidência: T4.3 (`setup_interactive.rs:1404`) pinava marker vazio como correto neste caso.

## Decisão (amenda SPEC §2 #1, opção A aprovada)
- Vazio + projeto conhecido = panorama útil com aviso.
- Vazio + sem projeto / panorama vazio = `INJECT: (no context found)` como hoje.
- Fonte: `Store::project_notes` (owned+linked, `brain-store/src/lib.rs:999`), limite 5, só leitura, erro vira vazio.
- stdout: `--- Brain context (projeto <nome> — panorama, sem pergunta) ---` + `path + snippet 120 chars` cada.
- stderr: mantém `hook: no question...` + `; showing project panorama` só quando panorama impresso.
- `hook ok` byte-idêntica (R-06). Sem DDL. Caso com-pergunta intocado.

## Aceite
- [ ] (a) vazio + projeto com notas → panorama impresso, sem INJECT-vazio.
- [ ] (b) vazio + projeto vazio/unknown → INJECT vazio (T4.3 preservado neste ramo).
- [ ] (c) pergunta real continua igual (T4.1 intacto, sem header panorama).
- [ ] `cargo test -p brain-cli --test hook_project_resolution` 0 failed.
- [ ] `cargo test -p brain-store` 0 failed.
- [ ] `cargo clippy -p brain-cli -p brain-store --all-targets -- -D warnings` 0.

## Riscos
- Panorama pode ser grande → mitigado: limite 5 + snippet 120 chars, só `regras`? Não — project_notes traz todas as layers; aceito (panorama é por projeto, não por layer). Sem paginação.
- Sem DDL: `project_notes` só SELECTs; lock WAL leitura breve antes do search.
- `hook ok` intacto: header novo vai para blocos de contexto, nunca para linha final.
- T4.3 existente (`a_payload_without_a_question_degrades...` em setup_interactive.rs) usava corpus COM notas e esperava INJECT vazio — conflita com nova decisão; atualizar ou mover expectativa para ramo (b) com projeto vazio. Não quebrar silenciosamente: ajustar teste com justificativa nesta pasta.

## Arquivos
- `crates/brain-cli/src/main.rs:1213-1248` (bloco degraded + panorama + gate INJECT)
- `crates/brain-cli/tests/hook_project_resolution.rs` (testes a/b/c)
- Este TASKS.md (governança)
