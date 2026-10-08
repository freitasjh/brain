# PROPOSAL — brain-multi-project-hooks

## Problema
1. Nao existe hook que faca a IA **buscar e salvar** no brain. Hoje isso depende de
   prompt estatico: `hooks/brain-kiro-steering.md` sao **259 linhas** de markdown que
   precisam ser copiadas por projeto — exatamente o que o usuario quer eliminar.
2. Nao existe forma de o brain **saber em qual projeto** a IA esta. `projects` existe no
   `data/brain.db` (6 cadastrados), mas nenhum cliente consulta. Resultado: a busca nao
   filtra por projeto.
3. **`kiro` nao e alvo do `setup`**: `crates/brain-cli/src/setup.rs:304-306` so conhece
   `all|opencode|systemd|shell|project`.
4. O unico hook que funciona hoje (`hooks/brain-hook.py`, 317 L) esta **deprecated** e
   slated para remocao na Fase C, sem contraparte instalada.

## Solucao aprovada
1. **config.json global** (em `BRAIN_DIR`), nao por projeto: mapeia `caminho -> projeto`,
   e sabe registrar "nao utilizar".
2. **Cascata de deteccao** 1->2->3->4 (config / git remote / nome do diretorio / perguntar).
3. **Hooks para as 2 IDEs**, com mecanismo **nativo de cada uma** (nao ha como
   compartilhar logica — ver Restricoes).
4. **Fase C (remocao do Python) como lote SEPARADO** — decisao do orquestrador, ver
   §Defaults.

## Defaults do orquestrador (aprovados sem conteudo - override P0->P1 registrado)
| Q | Default escolhido | Por que |
|---|---|---|
| Escopo | Hooks + config **agora**; Fase C em lote proprio | 7048 L de Python, dos quais 1792 L sao teste. Lotar junto deixa o harness sem meio de provar o resto. |
| Bug `fts5: syntax error near ","` | `.spec/bugs/` **separado** | E defeito de produto em `brain_search`, nao em hook. Misturar esconde um dos dois. |
| Versao | Hooks/config = **minor** (`0.9.1 -> 0.10.0`); Fase C = **major** (`1.0.0`) | Regra do AGENTS.md: minor = feature/param novo; major = remocao. |

## Escopo
**ENTRA**
- `config.json` global + leitura/resolucao em cascata
- `kiro` como alvo do `setup`; `setup` passa a perguntar **qual IDE** e **qual projeto**
- Hook de auto-busca para opencode (plugin JS) e kiro (`.kiro/hooks/*.json`)
- Migracao de `brain-kiro-steering.md` (259 L) para o mecanismo novo

**NAO ENTRA**
- Fase C / remocao do Python (lote separado)
- Tool MCP nova
- `brain search` filtrando por `project` (hoje ja filtra — nao mexer)

## Mecanismo por IDE (nao ha como unificar)

| IDE | Como o brain e alcancado | API |
|---|---|---|
| opencode | plugin JS em `.opencode/plugins/` ou `~/.config/opencode/plugins/` | eventos `session.created`, `experimental.session.compacting`, `tool.execute.before`; o plugin recebe **`directory`** |
| kiro | `.kiro/hooks/*.json` | `action.type` = `command` (shell, JSON no STDIN) ou **`agent`** (injeta prompt na conversa) |

O opencode injeta por JS; o kiro so sabe injetar por JSON declarativo + acao `agent`.
O unico trecho compartilhavel e **o payload**: um binario que recebe o contexto e devolve
o texto.

**Assimetria que decide o desenho:** o opencode entrega `directory` ao plugin; o kiro nao
entrega o path do projeto no STDIN. Por isso a resolucao precisa existir no binario
(`brain hook`), e nao no hook de cada IDE.

## Config.json (em `BRAIN_DIR`, nao no repo do projeto)
```json
{ "brain_projects": {
    "/abs/hive":      { "projeto": "hive" },
    "/abs/estudo":    { "projeto": null, "motivo": "recusado" },
    "/abs/pagamento": { "desabilitado": true } } }
```
Um JSON global serve os N diretorios. Nenhum arquivo por projeto.

## Riscos
- **Gate Python**: apagar `src/tests/` (1792 L) tira 124 testes do gate. `harness-continuous.md`
  H1.2 trata as duas suites como gates independentes e **TD-007 registra que isso ja foi
  um erro** (alegaram "pytest nao instalado" e estavam errados). Por isso Fase C e lote
  proprio com C-1 (reescrever) ANTES de C-2 (apagar).
- **`HELP_GROUPS`-style orfao**: um diretorio novo que o usuario recusa 2x deve nao
  perguntar de novo. Precisa de marcador persistido.
- 3 dos 6 projetos em disco nao estao no banco (`pagamento`, `medflow`, ...): a cascata
  cai no passo 4 (pergunta) — que e o comportamento correto, nao um bug.
