# Traceability — opencode-v2-migration

RF → Task → Evidência. Score 100% (6/6).

| RF | Requisito | Task(s) | Evidência |
|----|-----------|---------|-----------|
| RF1 | `opencode.json` em schema V2 nativo | T1 | JSON válido; `opencode` v2.0.29 carrega sem warning de compat |
| RF2 | Plugins locais V2-only | T2 | `plugin list` exibe `harness-validator` + `hitl-guardrail` com IDs estáveis, sem warnings |
| RF3 | Plugin `brain-session` V2 (prompt/context/compaction), fail-open, sem segredo | T3 | `node --test` 11/11: append-only, once-per-session, `userQuestion` só role=user, `SEGREDO-DO-MODELO` não vaza, compaction não sobrescreve prompt |
| RF4 | Instalador `brain setup opencode` gera e valida V2 sem pânica | T5–T7 | `brain setup opencode --yes` em tmp exit 0, `mcp.json`/`opencode.json` em `mcp.servers`, zero V1 |
| RF5 | Docs no schema V2 | T4, T7 | `CONFIGURE_MCP.md` §1 V2, `AGENTS.md` §3 V2, READMEs V2 |
| RF6 | Gate: zero regressão de suite | T9 | `cargo test --workspace` 516/0, `clippy -D warnings` 0, `brain ping` pong |

**Code review:** Batch 1 CHANGES_REQUIRED (2🔴) → Batch 2 cor rige → re-review APPROVED 0🔴.
**Segurança:** diff aprovado — `execFile` sem shell, argv separado, cwd correto, fail-open sem rethrow, sem credencial no payload.
