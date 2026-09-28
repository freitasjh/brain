# SPEC — brain-help-visual

## 1. Contexto
Binário: `brain` (`crates/brain-cli/src/main.rs`, 993 linhas, clap 4.6.6 derive).
Comando afetado: `brain --help` / `brain -h` (raiz). Output atual: lista plana
de 22 comandos, ordem arbitrária, 2 one-liners defeituosos.

Evidência do defeito (output real):
- `Reindex { ... }` — doc-comment de 3 linhas (~40 palavras) no enum `Cmd`
  (`main.rs:81-84`).
- `Server { ... }` — doc-comment de 4 linhas com `**or**` literal
  (`main.rs:101-104`).

## 2. Requisitos funcionais
- **RF-01**: os 22 comandos raiz aparecem distribuídos nas 5 categorias da
  PROPOSAL (Uso comum, Memória, Manutenção, Servidor, Projetos e setup), cada
  grupo com seu título visível, na ordem de grupos do parecer `ux-designer`.
  MECANISMO (revisado, ver ADR-01 v2): `next_help_heading` do clap 4.6.6 NÃO
  aplica a subcomandos — a implementação usa um renderer do help raiz que lê
  nome + one-liner de `Cli::command()` (fonte única continua sendo o clap).
- **RF-02**: one-liner de `reindex` cabe em ~80 colunas e continua descrevendo
  os 2 modos (`--all` default embeda; `--no-embed` estrutural offline).
  Detalhe longo vai para `long_about` do subcomando (fora do escopo visual
  da raiz, mas preserva a informação).
- **RF-03**: one-liner de `server` cabe em ~80 colunas, sem `**`, backtick ou
  qualquer marcação; detalhe US-01.1 vai para `long_about`.
- **RF-04**: rodapé com exemplos via `after_help` no `Cli`: `brain ping`,
  `brain store regras naming "## Regra" --scope global`,
  `brain search "termo" --explain`. Texto em português.
- **RF-05**: strings `server` (help raiz) e `start` (`brain server --help`)
  preservadas — gate do teste `server_start.rs:489-494`.
- **RF-06**: `brain help` **solitário** também é interceptado e devolve a mesma
  página raiz que `--help` e `-h`, byte a byte. É o mesmo pedido de ajuda raiz —
  um operador que digita `brain help` quer a página, não uma lista de jeitos de
  obter a página. `brain help <cmd>` continua sendo do clap (portão de subcomando).
  Teste: `help_and_help_flag_render_the_same_root_page`.

## 3. Requisitos não-funcionais / restrições
- **R-01**: diff limitado a `crates/brain-cli/src/main.rs` (+ testes, se o
  developer julgar necessário). Zero alteração nos outros 5 crates.
- **R-02**: todo texto **próprio** de saída em português — títulos de grupo,
  one-liners dos 20 comandos, `Exemplos:`. **Ressalva explícita**: o texto que o
  *clap* gera (`help`, `--help`, `--version`) permanece no idioma do clap, em
  inglês. Isso não é uma exceção escondida: a ADR-01 v2 elege o texto do clap a
  fonte única dos one-liners, e "fonte única" e "reescrever o texto do clap" são
  mutuamente exclusivos. `Outros:` também é do clap (`help`), pelo mesmo motivo.
  O que a R-02 proíbe é o renderer **inventar** texto em inglês.
- **R-03**: nenhuma linha do help raiz excede ~80 colunas (verificado com
  `COLUMNS=80`).
- **R-04**: zero mudança de comportamento do CLI (flags, defaults, parsing) —
  **com uma mudança de comportamento de *saída*, assumida em RF-06**: `brain help`
  solitário deixa de imprimir a lista plana do clap e passa a imprimir a página
  agrupada. A PROPOSAL não a previu. É a única alteração de comportamento
  introduzida por esta feature, e é visível.
  **Duas páginas raiz coexistem por design** (também deliberado, e fixado por
  `a_root_help_request_with_other_arguments_is_still_claps`):
  | invocação | quem renderiza | formato |
  |---|---|---|
  | `brain --help` / `-h` / `help` | renderer (ADR-01 v2) | agrupada, PT |
  | `brain --db X --help` | clap | plana, `Commands:` única |
  O motivo é **escopo**, não informação: existe exatamente **uma** página custom,
  e ela fica no argv mínimo. A justificativa anterior — "o renderer esconderia o
  `BRAIN_DB_PATH` resolvido" — era **refutada**: `option_row` imprime o valor
  resolvido nas *duas* páginas, provado por
  `the_options_block_carries_every_visible_arg_with_its_env_and_default`.
- **R-05**: `cargo test -p brain-cli` verde; suite completa
  `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings`
  + `.venv/bin/python -m pytest src/tests/ -q` (124 passed, 2 legadas) antes de
  declarar pronto — portão H1/H6 do harness.
- **R-06**: E2E H3 NÃO exigido (sem mudança de schema/servidor/viewer); smoke
  manual: `cargo run -p brain-cli -- --help` com `COLUMNS=80` e default.

## 4. Critério de aceite (E5 aprovado)
Usuário que roda `brain --help` sem saber nenhum comando identifica onde
procurar em uma leitura vertical, sem rolagem horizontal (`COLUMNS=80`).

## 5. ADRs
- **ADR-01 v1 (REVOGADO)**: agrupamento via `flatten_help`/`next_help_heading`
  do clap. **Medido e falso**: `next_help_heading` não se aplica a subcomandos
  (clap_builder `help_template.rs:394-396` monta seções a partir dos argumentos
  do pai) e propaga para o help do próprio subcomando, renomeando `Options:`
  dos 20 subcomandos; `flatten_help` inlina as flags de cada subcomando no pai.
- **ADR-01 v2 (VIGENTE)**: renderer do help raiz lendo `Cli::command()`, com
  títulos e ordem de grupo nossos. Custo aceito: a formatação do help raiz passa
  a ser código nosso (manutenção futura não vem do clap). Fonte única dos
  one-liners permanece o derive do clap — o renderer não os duplica.
  Portão do subcomando (`brain <cmd> --help`) permanece 100% no clap.
  O rodapé tem **um dono e dois leitores**: `after_help` é atributo do clap, e o
  renderer o *lê* de `Cli::command()`; por isso `Exemplos:` aparece também na
  página plana que o clap gera.
- **ADR-02**: detalhe removido dos one-liners vai para `long_about` dos
  subcomandos, não é apagado — informação preservada onde o usuário que já
  escolheu o comando a encontra.
- **ADR-03 (VIGENTE)**: o valor resolvido de um env global impresso no bloco
  `Options:` passa por `brain_embed::redact_url` antes de ir para o stdout.
  Impacto zero hoje — o único arg global é `db`, um path, e `redact_url` devolve
  entrada sem credencial **byte-idêntica** (propriedade testada). O motivo é o
  risco **futuro** nomeado em `harness-continuous.md` F4: um arg global sobre env
  tipo `BRAIN_OLLAMA_URL` (aceita `http://user:pass@host`) imprimiria a senha no
  `--help`, e uma página de ajuda viaja muito mais longe que uma linha de log.
  Teste: `an_env_value_is_redacted_but_a_credential_free_one_is_untouched` — o
  valor é **injetado**, não posto no ambiente, porque `set_var` é `unsafe` na
  edition 2024 e mexeria em estado que todos os testes paralelos do binário
  compartilham. A redaction fica dentro de `option_row`; só a leitura do env é
  injetada.
- **ADR-04 (VIGENTE)**: um subcomando que `HELP_GROUPS` esqueça é impresso sob
  `Outros:` em vez de sumir. Uma página de ajuda que mente por omissão é pior do
  que uma que mostra um comando fora do lugar. O único membro hoje é o `help` do
  próprio clap. O teste unitário
  `the_help_groups_cover_every_subcommand_exactly_once` exige que o órfão seja
  **exatamente** `["help"]`, então o `Outros:` não pode virar um esconderijo.

## 6. Delta Specs
Nenhum. Nenhum doc existente transcreve o texto do help.

## 7. Honestidade do gate (registrado pelo code-review)

Duas afirmações que este SPEC já carregou e que a medição refutou, agora
corrigidas no código para não voltarem:

- **`outln!` no renderer**: a justificativa era "`brain --help | head -20` fecha o
  pipe enquanto escrevemos, e um panic ali reportaria falha de um pipeline que
  funcionou". **Falso**: a página tem 1590 B e o buffer de pipe é 64 KB, então a
  escrita inteira completa antes de qualquer leitor fechar e `BrokenPipe` é
  inalcançável. `outln!` fica por **consistência** com as demais escritas do
  binário e porque a página cresce além de 64 KB se a lista de comandos crescer —
  e o teste `a_truncated_read_of_the_root_help_still_exits_zero` diz exatamente
  isso, em vez de fingir que prende a escolha.
- **Escopo do intercept**: a justificativa de manter `brain --db X --help` no clap
  era "é assim que o operador confere o `BRAIN_DB_PATH` resolvido". **Falso**, pelo
  mesmo motivo acima — `option_row` imprime o valor resolvido nas duas páginas. O
  motivo real é escopo: uma página custom, no argv mínimo.

## 8. Evidência de mutação (rodada 2)

`option_row` e o bloco `Options:` ficaram ~40 linhas com **zero** cobertura
comportamental: a mutação que remove o value name `<DB>` e a anotação
`[env: …]` passava a suíte inteira (11/11), porque todo o resto dos testes olha
as linhas de comando. Um bloco `Options:` que perde o `<DB>` e o env ainda parece
uma página de ajuda — foi por isso que passou na review.

| mutação | pego por | falhas |
|---|---|---|
| M2  remover `<DB>` **e** `[env:]` | `the_options_block_carries_every_visible_arg_with_its_env_and_default` | 1 |
| M2b remover só o value name | idem | 1 |
| M2c remover só a anotação `[env:]` | idem | 1 |
| M2d `redact_url` → texto cru (ADR-03 revertida) | `an_env_value_is_redacted_but_a_credential_free_one_is_untouched` | 1 |
