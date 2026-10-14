# SU-04 — suite verde H6 (Bloco 3)

> Executado 2026-10-14. SU-01b pulado: depende de OC-01 (Bloco 4, inexistente).

## Suite (contagens reais)

| Portão | Comando | Resultado |
|--------|---------|-----------|
| Rust | `cargo test --workspace` | **508 passed, 0 failed** (494 base + 14 novos: 4 `kiro_static_copy` + 9 `doctor` + 1 `ensure_db_parent`) |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | **0 warnings** |
| Python | `.venv/bin/python -m pytest src/tests/ -q` | **124 passed, 2 failed** = as 2 legadas declaradas H1.4 (`test_integration_full_cycle`, `test_integration_errors` — MCP legado, `No module named brain_server`, Fase C remove) |
| Hook | `bash hooks/brain-hook.test.sh` | **5 passed, 0 failed** (+1 skip: shellcheck ausente; `bash -n` + `dash -n` ok) |
| Sanidade Bloco 1+2 | `bash hooks/kiro/sanity.sh` | **3/3** intacto |

## Flake (regra: re-rodar 1x; persistindo vira bug P1)

- `config::concurrency::concurrent_writers_do_not_lose_each_others_entries`
  falhou **1x** sob carga paralela do workspace; verde isolado e verde no
  re-run completo (508/0). Registrado como flake único — **não P1 por ora**;
  2ª ocorrência vira `.spec/bugs/` P1.

## E2E scratch (H3: tmp, Ollama morto, produção intacta)

- `setup kiro --dir $S/proj` → 4 arquivos; `setup opencode` → plugin+mcp;
  `doctor --dir` → **D1–D6 ok, exit 0** (D4 ok após setup criar pai do banco).
- Hook removido → doctor exit 1 com caminho exato; D4 sem banco → warn, exit 0.
- `brain-hook.sh` via binário real: 4 eventos exit 0; nota `sessoes/e2e-h3/<data>` gravada.
- Produção 8321/8322: nenhum restart/kill (só `cargo test`, tmp e portas efêmeras).

## Matriz SU-02b (7 cenários)

verde + D1/D2/D3/D5/D6 removidos→fail + OLLAMA-morto→verde-via-D4-warn = **7/7**.
(+2 guards: `mcp_down_e_warn_nunca_fail`, `nenhuma_credencial_no_relatorio`.)
