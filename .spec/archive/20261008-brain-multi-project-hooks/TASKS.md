# TASKS — brain-multi-project-hooks

Legenda: [ ] pendente · Risco: A=alto M=medio B=baixo · BDD em uma linha por cenario.

## P1 — config.json (Risco M)
- [x] **T1.1** [x] teste: roundtrip gravar -> ler -> mesma entrada
- [x] **T1.2** [x] teste: JSON invalido lido NAO e sobrescrito na proxima gravacao
- [x] **T1.3** [x] BDD: dado um `BRAIN_DIR` novo, quando gravo, entao `~/.brain` e criado
- [x] **T1.4** [x] impl `config.rs`: `load()`, `save_entry(caminho, Entry)`
- [x] **T1.5** [x] Entry = `{projeto: Option<String>, motivo: Option<String>, desabilitado: bool}`

## P2 — cascata (Risco A)
- [x] **T2.1** [x] teste: passo 1 config tem precedencia sobre git/nome
- [x] **T2.2** [x] teste: passo 2 casa `git remote` com nome de projeto
- [x] **T2.3** [x] teste: passo 3 casa nome do diretorio EXATO (nao prefixo: `atlas` != `atlas-ecm`)
- [x] **T2.4** [x] BDD: sem config, sem remote, nome sem match -> pergunta; aceita -> grava
- [x] **T2.5** [x] BDD: recusa -> grava `{projeto:null,motivo:"recusado"}` e nao pergunta de novo
- [x] **T2.6** [x] impl `resolve_project()` devolvendo `(Option<String>, Origem)` onde Origem e `Config|GitRemote|DirName|Asked|Recusado|Disabled`

## P3 — `brain hook` opcional (Risco A)
- [x] **T3.1** [x] teste: `--project X` explicito produz **exatamente** o mesmo output de hoje (R-06)
- [x] **T3.2** [x] teste: sem `--project`, usa a cascata e imprime a origem
- [x] **T3.3** [x] teste: diretorio `disabled` nao pergunta e nao busca
- [x] **T3.4** [x] impl: `String` -> `Option<String>` em `main.rs:385` + linha de origem no stdout
- [x] **T3.5** [x] regressao: `cli_e2e.rs::e2e_concurrent_hooks_never_lose_a_session_event` (12x8) verde

## P4 — pergunta real (Risco A)
- [x] **T4.1** [x] teste: query enviada = mensagem do usuario (nao a string fixa)
- [x] **T4.2** [x] teste: projeto entra como **filtro**, nao como termo da query
- [x] **T4.3** [x] teste: payload sem mensagem -> query vazia + `INJECT: (no context found)` + aviso stderr (SPEC §2 decisão medida contra PLAN: texto fixo retorna zero, vazio é honesto e grátis)
- [x] **T4.4** [x] impl: remove a query fixa de `main.rs:880` e o `format!("regras {}", project)` de `:883`
- [x] **T4.5** [x] BDD: Ollama fora -> FTS ainda pontua (degradacao, nao erro)

## P5 — `setup` interativo + kiro (Risco M)
- [x] **T5.1** [x] teste: `setup kiro` e alvo valido (`setup.rs:304-306`)
- [x] **T5.2** [x] teste: pergunta IDE, resposta invalida re-pergunta
- [x] **T5.3** [x] teste: `--dry-run` pergunta e NAO escreve
- [x] **T5.4** [x] teste: stdin nao-tty sem `--yes` nao trava, usa default + aviso (R-07)
- [x] **T5.5** [x] impl: perguntas de IDE e projeto; persistir em RF-02

## P6 — artefatos das 2 IDEs (Risco M)
- [x] **T6.1** [x] teste: `brain setup opencode` gera plugin com schema valido
- [x] **T6.2** [x] teste: `brain setup kiro` gera `.kiro/hooks/*.json` com `version:v1`, trigger e `action`
- [x] **T6.3** [x] teste: o binario chamado pelo hook e o `brain hook` Rust (nao python)
- [x] **T6.4** [x] BDD: hook sem servidor MCP nao escreve erro na conversa

## Verificacao (H6) — pre-condicao de fechar
- [x] `cargo test --workspace` 0 falhas
- [x] `cargo clippy --workspace --all-targets -- -D warnings` 0
- [x] `.venv/bin/python -m pytest src/tests/ -q` -> 124 passed, 2 legadas (gate H1.2, **esta feature nao remove Python**)
- [x] `brain --help` mostra `setup` com o novo alvo
- [x] 6 mutacoes aplicadas, 6 pegas
