# VERIFY — brain-help-visual

Data: 2026-09-28. Fase: Verify (pos-implementacao).
Todas as afirmacoes abaixo foram medidas pelo orquestrador neste ciclo, nao
herdadas do relato do developer nem do code-reviewer.

## Resumo das 3 dimensoes

| Dimensao | Veredicto | Base |
|---|---|---|
| Completude | ✅ | RF-01..06 + R-01..06 implementados; zero TODO; 21 one-liners, 5 titulos, rodape |
| Correcao | ✅ | 342 passed / 0 failed; clippy 0; pytest 124+2 legadas; smoke 73 col |
| Coerencia | ✅ | ADR-01 v2, ADR-02, ADR-03, ADR-04 refletidos no codigo e com teste |

## 1. Completude — ✅

| Req | Status | Evidencia |
|---|---|---|
| RF-01 5 categorias, ordem do ux | ✅ | smoke com `Uso comum/Memória/Manutenção/Servidor/Projetos e setup` na ordem; `the_five_group_titles_appear_in_the_frozen_order` (ordem, nao conjunto) |
| RF-02 one-liner `reindex` + 2 modos | ✅ | 33 col; modos em `long_about` (`the_detail_moved_to_long_about_is_still_reachable`, 6 needles) |
| RF-03 `server` sem `**`/backtick | ✅ | `no_markdown_leaks_into_the_command_rows` — assercao de forma, nao das 2 strings |
| RF-04 rodape 3 exemplos | ✅ | `the_footer_carries_the_three_examples`; path `regras naming` (nao duplica scope) |
| RF-05 `server`/`start` preservados | ✅ | `server_start.rs` 8/8 (gate existente) |
| RF-06 `brain help` solitario | ✅ | `help_and_help_flag_render_the_same_root_page` (comparacao de bytes entre `--help`, `-h`, `help`) |

Zero checkbox pendente, zero TODO no diff.

## 2. Correcao — ✅

Medido pelo orquestrador neste ciclo:

```
cargo test --workspace                → 342 passed / 0 failed
cargo test -p brain-cli               → 79 passed / 0 failed (21 unit + 13 help_overview + 8 server_start)
cargo clippy --workspace --all-targets -- -D warnings → 0
.venv/bin/python -m pytest src/tests/ -q → 124 passed, 2 failed
   (as 2 legadas declaradas de H1.4: test_integration_{full_cycle,errors})
brain --help | wc -c                  → 1590 B
largura max das linhas de comando     → 73 col (COLUMNS=80)
```

- **R-01**: diff limitado a `crates/brain-cli/src/main.rs` + `tests/help_overview.rs`.
  `crates/brain-mcp/src/rmcp_service.rs` aparece no `git status` mas e de ONDA
  ANTERIOR (descricao de `brain_forget_sweep`) — nao pertence a esta feature e
  nao foi tocado aqui. O commit precisa isolar os dois.
- **R-03**: 73 col na secao de comandos. A linha `--db` estoura porque imprime o
  `BRAIN_DB_PATH` resolvido — waiver registrado e escopado com justificativa.
- **R-05**: os DOIS portoes (Rust e Python) verificados, como H1.2 exige.
- **R-06**: sem E2E H3, conforme a SPEC — a feature nao toca schema, servidor
  nem viewer. Smoke `COLUMNS=80` executado.

E2E por mutacao (padrao do `backend-rules.md` — 7 mutacoes aplicadas, 7 pegas,
1 declarada como nao-pegavel):

| # | Mutacao | Falhas |
|---|---|---|
| M1 | hardcode de one-liner no renderer | 2 |
| M2 | `next_help_heading` reintroduzido (o bug v1) | 1 |
| M3 | vazar `**` no one-liner do `store` | 2 |
| M4 | remover `after_help` | 1 |
| M5 | apagar `long_about` do `reindex` | 1 |
| M2-rodada2 | remover `<DB>` + `[env:]` | 1 (era 11/11 verde) |
| M3-cmd-novo | comando novo fora de `HELP_GROUPS` | 1 (`ungrouped: ["doctor","help"]`) |
| M6 | `print!` no lugar de `outln!` | ❌ **NAO pega, e declarado** — pagina tem 1590 B, buffer de pipe 64 KB, `BrokenPipe` inalcancavel. O doc do teste afirma o que prova e o que nao prova. `outln!` e escolha por consistencia. |

## 3. Coerencia — ✅

- **ADR-01 v2** (renderer do help raiz): refletido em `main.rs`; a negativa esta
  MEDIDA (`next_help_heading` propaga para o help do subcomando) e tem recibo em
  `a_subcommand_help_is_still_claps` (6 subcomandos × 2 flags).
- **ADR-02** (detalhe -> `long_about`): `reindex`/`server`/`search`.
- **ADR-03** (redaction do env no `Options:`): `brain_embed::redact_url` em
  `main.rs:281`; `an_env_value_is_redacted_but_a_credential_free_one_is_untouched`
  prova redaction E byte-identidade sem credencial (over-redaction que degrada o
  caso comum esta barrado).
- **ADR-04** (`Outros:`): comando esquecido visivel; o teste exige que o orfao
  seja exatamente `["help"]`, entao `Outros:` nao pode virar esconderijo.
- **Sem vazamento de camada**: `HELP_GROUPS` e `const` local do crate; a
  unica dependencia nova ja existia (`brain-cli` -> `brain-embed`, Cargo.toml:13).
  Zero crate novo, zero schema, zero migration.

## Ressalvas honestas (nao bloqueiam)

1. **R-02 emendado, nao cumprido literalmente.** `help`, `--help`, `--version` e
   `Outros:` continuam em ingles — sao texto do clap, e a ADR-01 v2 elege o texto
   do clap como fonte unica. Emendado na SPEC com a justificativa: fonte unica e
   reescrita do texto sao mutuamente exclusivos. R-02 proibe o renderer INVENTAR
   ingles, nao proibe o clap falar ingles.
2. **Duas paginas raiz por design.** `brain --help` (agrupada, PT) e
   `brain --db X --help` (plana, clap). A justificativa original da exclusao
   ("esconderia o `BRAIN_DB_PATH` resolvido") foi REFUTADA — `option_row` tambem
   imprime o valor resolvido. Registrado em R-04 com a razao real: uma pagina
   custom so no argv minimo.
3. **RF-06 e mudanca de comportamento real** que a PROPOSAL nao previa: `brain
   help` solitario saiu da lista plana do clap. `grep` por `brain help` no repo
   nao achou consumidor, mas consumidor pode viver FORA do repo. ASSUMIDO pelo
   usuario (opcao A) em 2026-09-28.
4. **Sem guard de compile-time** entre `HELP_GROUPS` e o enum `Cmd`. Um comando
   novo fora da tabela cai em `Outros:` (visivel, e o teste pega), nao some em
   silencio — mas o mecanismo e reacao, nao erro de compilacao.
5. **44 linhas de saida** > terminal tipico de 24. E5 prometeu so "sem rolagem
   horizontal" — cumprido (73 col). A altura e consequencia de 21 comandos + 5
   grupos, nao um defeito.
6. **`ui-ux-pro-max` indisponivel** neste ambiente; o parecer visual usou
   `frontend-design` + principios de UX de CLI. Ressalva de proveniencia, nao
   falha de resultado.

## Afirmacoes que a medicao refutou (registradas na SPEC §7)

1. **ADR-01 v1**: "agrupamento via `next_help_heading`/`flatten_help`" — FALSO.
   O clap 4.6.6 monta secoes de heading a partir dos argumentos do PAI, nunca de
   subcomando; e o atributo propaga, renomeando `Options:` dos 20 subcomandos.
2. **`brain --db X --help` discordaria do comando** — FALSO. `option_row` le o
   env resolvido, entao a pagina custom mostraria o mesmo valor.

## Erros de governancia desta sessao (registrados, nao varridos)

1. **Override P1→P2**: o usuario aprovou antes do conteudo da SPEC ser exibido.
   Registrado em `workflow-state.json.overrides`.
2. **ADR-01 v1 na SPEC**: afirmacao escrita sem medir, desmentida por medicao no
   ciclo seguinte. Corrigida para v2 com a v1 marcada REVOGADA.
3. **`fullstack-code-reviewer`** (nome das regras) nao existe no repo; o
   registered e `code-reviewer` (`opencode.json:76`). Duas delegacoes falharam
   com `Subagent depth limit reached (1)` antes disso — limite de plataforma, nao
   de configuracao (nao ha chave `subagent_depth` em nenhum opencode.json).

## Veredicto

**APROVADO.** 3/3 dimensoes ✅, zero debito 🔴, zero 🟡 nao justificado.
Pronto para archive e commit, com as 6 ressalvas acima ledidas junto.
