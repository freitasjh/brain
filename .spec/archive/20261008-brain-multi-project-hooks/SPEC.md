# SPEC — brain-multi-project-hooks

## 1. Contexto e o achado que muda o desenho

`brain hook` **ja existe em Rust** (`main.rs:703-897`) e ja tem o evento
`session-start`, que **ja injeta contexto** (`main.rs:878-891`). O mecanismo nao
precisa ser construido: precisa ser **apontado**.

O que o torna inutil hoje, medido:

1. **A query e fixa.** `main.rs:880` busca a string literal
   `"padrões melhores práticas lições"` — nao sabe o que o usuario perguntou. E o
   fallback `main.rs:883` monta `format!("regras {}", project)`, que casa por
   **palavra** e nao por **semantica**.
2. **`--project` e obrigatorio** (`main.rs:385`, `String` nao-`Option`) e nao tem
   deteccao. O chamador precisa **saber** o nome do projeto — que e o requisito
   que o usuario quer eliminar.
3. **Nao ha config de cliente.** `projects` existe no `data/brain.db` (6), mas
   nenhum cliente consulta para descobrir "em qual projeto estou".
4. **`kiro` nao e alvo do `setup`** (`setup.rs:304-306`: `all|opencode|systemd|shell|project`).
5. **`setup` nao e interativo** (`setup.rs:287 run_setup`) — perguntar IDE e
   projeto e uma mudanca de contrato.

## 2. Requisitos funcionais

- **RF-01 `config.json` global**: arquivo novo em `BRAIN_DIR` (fallback
  `~/.brain`), mapeando caminho absoluto -> estado do diretorio. Nenhum arquivo por
  projeto. Formato:
  `{"brain_projects":{"<abs>":{"projeto":"<nome>"|null,"motivo":"recusado"|null,"desabilitado":bool}}}`
- **RF-02 Cascata de resolucao** (ordem fixa, toda decisao com origem impressa):
  1. `config.json` tem entrada para o caminho **absoluto** → usa
  2. `git remote get-url origin` casa com algum nome de projeto → usa
  3. nome do diretorio casa exatamente com algum projeto → usa
  4. nenhuma das 3 → **pergunta uma vez**:
     - aceita → grava `{"projeto": "<nome>"}` no config
     - recusa → grava `{"projeto": null, "motivo": "recusado"}` e **nao pergunta de novo**
  Diretorio marcado `desabilitado` **nunca** entra na cascata e **nunca** pergunta.
- **RF-03 `brain hook --project` passa a ser opcional**: ausente, o binario resolve
  pela cascata. Presente, mantem o valor (compatibilidade). Saida sempre inclui a
  **origem** da resolucao, em **linha propria** (`hook resolve ...`), para que a
  linha `hook ok ...` fique byte-identica ao comportamento anterior (R-06).

  As **8 origens** realmente emitidas (RF-03 v1 listava 5; medido em producao):

  | origem | escreve nota? | grava config? | pergunta de novo? | final? |
  |---|---|---|---|---|
  | `config` | sim | ja gravado | nao | sim |
  | `git-remote` | sim | **nao** | nao | sim |
  | `dir-name` | sim | **nao** | nao | sim |
  | `asked` | sim | sim | nao | sim |
  | `explicit` | sim | **nao** | nao | sim |
  | `disabled` | **nao** | opt-out ja gravado | **nunca** | sim |
  | `recusado` | **nao** | recusa ja gravada | **nunca** | sim |
  | `unresolved` | **nao** | **nada** | **sim, na proxima** | **nao** |

  Os tres negativos pulam a escrita. So `unresolved` nao e final: nunca vira
  recusa permanente, porque ninguem chegou a perguntar (stdin nao-tty, CI).

- **RF-03.1 `motivo` conizado**: so `motivo == "recusado"` produz `Origin::Recusado`.
  Entrada com `projeto` ausente e `motivo` desconhecido/ausente **nao** e recusa —
  cai na cascata sem gravar nada. Sem isso, um typo de chave (`"projet"`) viraria
  recusa permanente de um diretorio cujo operador nunca foi perguntado, e o save
  seguinte apagaria o typo sem rastro.
- **RF-07.1 config e opcional em tempo de execucao**: sem `BRAIN_DIR` e sem `HOME`
  gravavel, o hook roda **degradado** (cascata sem passo 1, sem gravar) e sai 0.
  Um `?` no caminho do config transforma ausencia de env em exit 1, que quebra a
  IDE — o oposto de RF-07.
- **RF-07.2 `save_entry` com lock de arquivo** (`fs2::try_lock_exclusive`, o padrao
  ja usado no spool do hook em `main.rs:942`). Sem lock, dois processos (opencode +
  kiro, ou dois terminais) fazem read-modify-write e um perde a entrada do outro em
  silencio — o rename atomico evita arquivo truncado, nao update perdido.
- **RF-07.3 `git remote` sem timeout e sem `read_line` no hook**: nenhuma das duas
  operacoes pode bloquear a IDE. A pergunta do passo 4 nao roda dentro do hook
  (nao ha tty); se rodar, precisa de flag explicita.
- **RF-04 Injetar a pergunta real, nao uma query fixa**: o payload do
  `session-start` carrega a mensagem do usuario; a busca passa a usar esse texto.
  A query fixa `"padrões melhores práticas lições"` e o fallback
  `format!("regras {}", project)` saem. Heuristica por palavra no nome do projeto
  **nao** substitui busca semantica — o projeto vira **filtro** da busca, nao
  termo dela.

  **Duas decisoes medidas, ambas contrarias ao `PLAN.md` §2, e registradas aqui porque o
  plano ainda diz o contrario:**

  1. **Sem pergunta -> query vazia, e nao o texto fixo antigo.** O plano diz "cai no
     texto fixo". Medido contra um corpus que tem as regras do projeto, aquele texto
     fixo retorna **zero**:
     `search("padroes melhores praticas licoes", layer=regras, scope=global) -> total = 0`.
     Ele e um RRF de 4 streams com resultado garantidamente vazio. A query vazia e
     gratis: sem query nao ha FTS, vetor, entidade nem grafo, entao `search` retorna em
     `all_paths.is_empty()`. E o que importa mais: quando o texto fixo **acha** alguma
     coisa, ele imprime notas que nao respondem a pergunta que ninguem fez. Vazio
     imprime `INJECT: (no context found)`, que e verdade. A degradacao e anunciada em
     stderr.
  2. **Termos unidos por `OR` na injecao, nao por `AND`.** Medido:
     `search("kebab-case nos caminhos", project=hive) -> total = 0`, enquanto
     `"kebab" OR "case" OR "nos" OR "caminhos" -> total = 1`. `AND` exige *todos* os
     termos, e uma pergunta de 6 palavras quase nunca tem todas na nota relevante — o
     que faria a feature devolver "nada", como a query fixa, por outro motivo.
     `Store::search` **mantem** o `AND` do FTS5, porque ali quem chama pediu uma busca.
     A conjuncao e politica do chamador: `fts5_match_expr_joined(query, "OR")`.
  3. **A pergunta viaja em `--question`, nao dentro do `--payload`.** O payload e JSON
     dentro de uma string de shell: uma aspa na pergunta quebraria o JSON. Como `argv`,
     nao precisa de escape nenhum. A chave `question` do payload segue aceita como
     fallback, e as duas em conflicto sao avisadas em vez de silenciosamente fundidas.

- **RF-04b O artefato kiro trocou `SessionStart` por `PromptSubmit`.** RF-05 descrevia o
  artefato com `SessionStart`, e P4 removeu esse trigger. O motivo e o proprio requisito:
  a injecao de contexto acontece no `session-start`, e nao ha contexto relevante sem
  algo que o usuario perguntou — no `SessionStart` ele ainda nao digitou nada. A doc do
  kiro expoe a pergunta na env `USER_PROMPT` ("When using the shell command action, the
  user prompt can be accessed via the `USER_PROMPT` environment variable"), e so o
  trigger que dispara quando ela existe.

  **Instalar os dois foi considerado e rejeitado**, e nao esquecido: `brain hook` usa o
  unico nome de evento `session-start` para registrar *e* injetar, e o id de dedup e
  `$$` (o pid do shell). Dois triggers instalados registrariam **duas** secoes de sessao
  por sessao, ambas distintas — que e o bug de "uma sessao por login" que o `$$` existe
  para resolver, reintroduzido.

  `SessionStart` **continua em `KIRO_TRIGGERS`**, de proposito: aquela lista e o conjunto
  de nomes de trigger *validos*, nao os que este artefato instala, e tirar um nome
  dela faria o guard rejeitar um hook legitimo escrito depois. O nome do evento gravado
  continua `session-start`; o que mudou e *quando* o kiro o dispara.

- **RF-05 `kiro` como alvo do `setup`**; `setup` passa a perguntar **qual IDE**
  (opencode | kiro) e, na sequencia, o nome do projeto, persistindo em RF-02.

  **Decisao: `all` NAO inclui `kiro`.** `all` pergunta qual IDE e instala
  **exatamente uma** (`opencode` por padrao, `kiro` se for a resposta), mais
  `systemd` e `shell`. Dois motivos, ambos medidos:

  1. `all` hoje significa "a IDE **esta** maquina usa, mais os servicos". Incluir
     `kiro` faria toda invocacao existente de `brain setup` escrever artefatos de uma
     segunda IDE — e `kiro` escreve **dentro do projeto** (`.kiro/hooks/`), ou seja,
     no diretorio em que a pessoa estava quando rodou o comando. Isso e efeito
     colateral em checkout, nao configuracao de maquina.
  2. `kiro` e **opt-in**: por nome (`brain setup kiro`) ou pela resposta.

  **Decisao: alvo nomeado NAO e bundle.** `setup opencode` instala so opencode;
  `setup systemd` so systemd. Isto ja foi um bug nesta fase: tratar alvo de IDE como
  bundle fez `setup opencode` escrever as units do systemd, e um `setup systemd
  --mcp-port 8331` posterior encontrou os arquivos ja la, pulou sem `--force`, e
  deixou a porta errada. Pego por `cli_e2e.rs::e2e_setup_opencode_and_systemd`.

  **Resposta explicita nao-interativa**: `--project <nome>` e `--decline-project`
  gravam a decisao sem perguntar. Sem isso, **nenhum CI jamais conseguiria** registrar
  um projeto, porque a unica forma de perguntar e um terminal. Os dois juntos sao
  contradicao e sao recusados.
- **RF-06 `setup` interativo respeita `--dry-run` e `--force`**: com `--dry-run`, as
  perguntas sao feitas e as escritas sao apenas **exibidas**; com `--force`, nao ha
  confirmacao por arquivo.
- **RF-07 Degradacao silenciosa**: hook sem brain no ar, sem MCP, ou com Ollama
  fora → nao quebra a IDE, nao escreve na conversa um erro. Saida no stderr.
- **RF-08 `brain-hook.py` sai do caminho suportado** (317 L): o mecanismo novo usa
  `brain hook` Rust. Remocao fisica do `.py` fica na **Fase C** (lote separado).

## 3. Nao-funcionais e restricoes
- **R-01** Hooks por IDE usam a API **nativa** de cada uma (plugin JS no opencode;
  `.kiro/hooks/*.json` com `action.type: command|agent` no kiro). Nao existe hook
  unico — medido que nao ha API comum.
- **R-01b O opencode tem evento com a mensagem do usuario; o limite e de `role`, nao de
  evento.** Verificado na doc (<https://opencode.ai/docs/plugins/>) e nos tipos do SDK:
  existem `message.updated` (`properties.info`, discriminated por `role: "user"`) e
  `message.part.updated` (`properties.part`, o `TextPart` com o texto). **Nenhum dos dois
  carrega os dois**, entao nenhum sozinho responde "o que o usuario perguntou": o papel
  esta em um e o texto no outro. O plugin emparelha os dois por `messageID` e trata as
  duas ordens, porque os eventos sao independentes e a doc nao diz qual chega primeiro.

  O limite honesto: **um `TextPart` sozinho nao prova mensagem de usuario** — a resposta
  do assistant tambem faz stream de text parts, e o primeiro deles injetaria a resposta
  do modelo de volta como se fosse a pergunta. O conjunto de ids de usuario e o que os
  separa. Nao ha evento dedicado tipo "user prompt submitted" na doc, e nao foi inventado
  um.

  Ressalva de robustez: os dois eventos **nao** sao um par ordenado, e a doc nao promete
  entrega. A ordem inversa esta testada, mas um par que nunca se completasse degradaria
  para o caminho T4.3 (injeta por projeto, avisa em stderr) — o que e pior que injetar
  errado, e nao silencioso.
- **R-02** Toda logica de resolucao fica no **binario** (`brain hook`), nao no hook:
  o opencode entrega `directory` ao plugin; o kiro nao entrega path no STDIN.
- **R-03** `config.json` e **global**. Zero arquivo por projeto — e o requisito.
- **R-04** Diff em `crates/brain-cli/`, `hooks/`, `.kiro/`, `.opencode/plugins/`.
  `data/brain.db` **nao** tem DDL novo (o config e arquivo; `projects` ja existe).
- **R-05** Suite completa: `cargo test --workspace` 0 falhas, clippy `-D warnings` 0.
  Gate Python **segue obrigatorio** (H1.2) — esta feature nao remove Python.
- **R-06** Zero regressao no `brain hook` atual: `--project` explicito continua
  funcionando identico (12 processos x 8 rodadas em `cli_e2e.rs`).
- **R-07** Toda pergunta do `setup` tem bypass nao-interativo (`--yes`/variavel de
  ambiente), para nao travar CI.
- **R-08** `hooks/brain-kiro-steering.md` (259 L) **nao e apagado** nesta feature:
  sai com o mecanismo novo funcionando, em commit proprio.

## 4. Criterio de aceite (E5)
Em um diretorio mapeado, abrir a IA numa das 2 IDEs, fazer uma pergunta que o brain
tem contexto, e ela **busca sem ninguem pedir** — com o resultado **dividido pelo
projeto certo**. Em diretorio novo, a 1a pergunta identifica o projeto uma unica
vez; recusar **nao** pergunta de novo. Brain fora do ar **nao** quebra a IDE.

## 5. ADRs
- **ADR-05 Config em arquivo, nao no banco**: `projects` no `data/brain.db` nao
  modela "nao utilizar", que e o estado que o usuario pediu. `note_projects` liga
  nota a projeto, nao diretorio a projeto. Schema novo seria DDL por um estado de
  ausencia.
- **ADR-06 Cascata config -> git -> nome -> pergunta**: config primeiro porque os
  dois fallbacks tem falsos positivos medidos (3 dos 6 projetos em disco nao estao
  no banco). Git antes do nome porque `atlas-ecm` e `atlasos` e `atlas-admin` coexistem.
- **ADR-07 Injectar a pergunta, nao uma query fixa**: o mecanismo ja existia e era
  inutil por buscar texto fixo. Corrigir a query e o menor caminho para o requisito;
  um payload novo por IDE seria maior sem mudar o resultado.
- **ADR-08 Config global, nao por projeto**: unico arquivo serve N diretorios, e e o
  que elimina a configuracao por projeto que motivou a demanda.
