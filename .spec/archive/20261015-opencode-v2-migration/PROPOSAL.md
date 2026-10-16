# PROPOSAL — opencode-v2-migration

- **Data:** 2026-10-15
- **Classificação:** Refactor / migração de plataforma
- **Tipo de rota:** Refactor
- **Status:** CONCLUÍDO — archive 20261015-opencode-v2-migration

## Problema

OpenCode atualizou para V2. Três quebras intencionais: (1) plugin API nova —
implementação V1 **não executa** em V2; (2) config V1 (`plugin` singular,
`permission` por tool, `agent` singular, `mcpServers`) é aceita quieta mas tem
forma nativa V2; (3) instalar V2 substitui o binário V1, sem convivência.

O repo brain tinha:
- `opencode.json` com `plugin`, `permission`, `agent` (V1)
- 3 plugins V1 (`harness-validator`, `hitl-guardrail`, `hooks/opencode/brain-session.js`)
- instalador Rust `brain setup opencode` gerando `mcpServers` V1 e **validando**
  o plugin contra uma lista de hooks V1 (`OPENCODE_VERIFIED_EVENTS`)

## Solução

Migração V2-only nativa em 3 batches:
1. Batch 1 — JS/config/docs (opencode.json, plugins, brain-session.js + teste,
   CONFIGURE_MCP.md, READMEs)
2. Batch 2 — Rust instalador (setup.rs/doctor.rs para V2, merges `mcp.servers`)
3. Batch 3 — harness completo (7 gates)

## Escopo

- **Entra:** repo brain — `opencode.json`, `.opencode/plugins/*.js`,
  `hooks/opencode/brain-session.js` + teste, `.opencode/package.json`,
  `CONFIGURE_MCP.md`, `README.md`, `hooks/README.md`,
  `crates/brain-cli/src/setup.rs`, `crates/brain-cli/src/doctor.rs`,
  `crates/brain-cli/tests/{cli_e2e,setup_interactive}.rs`, `AGENTS.md` §3
- **NÃO entra:** `~/.config/opencode/*` (config global — escopo separado, não tocado)

## Riscos registrados

| Risco | Mitigação aplicada |
|---|---|
| `brain setup opencode` pânica em runtime por assert de hooks V1 (C1) | Follow-up Rust batch 2 — extrator `plugin_session_hooks` + `OPENCODE_V2_SESSION_HOOKS` |
| Instalador emite V1 enquanto docs dizem V2 (C2) | merges migrados para `mcp.servers` |
| `event.system` objeto[] vs string[] | plugin aceita ambos, testado |
| `ctx.location.directory` confirmar | cwd do `execFile` com fallback; confirmado no teste |
| names reais de permission action (`edit`/`shell`/`subagent`) | carregamento V2 sem warnings + `setup` gerando schema V2 |

## Delta Specs

- **ADDED** `CONFIGURE_MCP.md` §1 OpenCode — bloco V2 (`mcp.servers`, `disabled`, `timeout`)
- **MODIFIED** `AGENTS.md` §3 MCP Server — exemplo V2
- **MODIFIED** `README.md`, `hooks/README.md` — hooks V2
- **MODIFIED** `crates/brain-cli/src/setup.rs` — merges V2, consts V2
- **MODIFIED** `crates/brain-cli/src/doctor.rs` — D3 valida shape V2
