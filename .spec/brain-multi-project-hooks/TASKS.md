# TASKS — brain-multi-project-hooks

Legenda: [ ] pendente · Risco: A=alto M=medio B=baixo · BDD em uma linha por cenario.

## P1 — config.json (Risco M)
- [ ] **T1.1** [ ] teste: roundtrip gravar -> ler -> mesma entrada
- [ ] **T1.2** [ ] teste: JSON invalido lido NAO e sobrescrito na proxima gravacao
- [ ] **T1.3** [ ] BDD: dado um `BRAIN_DIR` novo, quando gravo, entao `~/.brain` e criado
- [ ] **T1.4** [ ] impl `config.rs`: `load()`, `save_entry(caminho, Entry)`
- [ ] **T1.5** [ ] Entry = `{projeto: Option<String>, motivo: Option<String>, desabilitado: bool}`

## P2 — cascata (Risco A)
- [ ] **T2.1** [ ] teste: passo 1 config tem precedencia sobre git/nome
- [ ] **T2.2** [ ] teste: passo 2 casa `git remote` com nome de projeto
- [ ] **T2.3** [ ] teste: passo 3 casa nome do diretorio EXATO (nao prefixo: `atlas` != `atlas-ecm`)
- [ ] **T2.4** [ ] BDD: sem config, sem remote, nome sem match -> pergunta; aceita -> grava
- [ ] **T2.5** [ ] BDD: recusa -> grava `{projeto:null,motivo:"recusado"}` e nao pergunta de novo
- [ ] **T2.6** [ ] impl `resolve_project()` devolvendo `(Option<String>, Origem)` onde Origem e `Config|GitRemote|DirName|Asked|Recusado|Disabled`

## P3 — `brain hook` opcional (Risco A)
- [ ] **T3.1** [ ] teste: `--project X` explicito produz **exatamente** o mesmo output de hoje (R-06)
- [ ] **T3.2** [ ] teste: sem `--project`, usa a cascata e imprime a origem
- [ ] **T3.3** [ ] teste: diretorio `disabled` nao pergunta e nao busca
- [ ] **T3.4** [ ] impl: `String` -> `Option<String>` em `main.rs:385` + linha de origem no stdout
- [ ] **T3.5** [ ] regressao: `cli_e2e.rs::e2e_concurrent_hooks_never_lose_a_session_event` (12x8) verde

## P4 — pergunta real (Risco A)
- [ ] **T4.1** [ ] teste: query enviada = mensagem do usuario (nao a string fixa)
- [ ] **T4.2** [ ] teste: projeto entra como **filtro**, nao como termo da query
- [ ] **T4.3** [ ] teste: payload sem mensagem -> query vazia + `INJECT: (no context found)` + aviso stderr (SPEC §2 decisão medida contra PLAN: texto fixo retorna zero, vazio é honesto e grátis)
- [ ] **T4.4** [ ] impl: remove a query fixa de `main.rs:880` e o `format!("regras {}", project)` de `:883`
- [ ] **T4.5** [ ] BDD: Ollama fora -> FTS ainda pontua (degradacao, nao erro)

## P5 — `setup` interativo + kiro (Risco M)
- [ ] **T5.1** [ ] teste: `setup kiro` e alvo valido (`setup.rs:304-306`)
- [ ] **T5.2** [ ] teste: pergunta IDE, resposta invalida re-pergunta
- [ ] **T5.3** [ ] teste: `--dry-run` pergunta e NAO escreve
- [ ] **T5.4** [ ] teste: stdin nao-tty sem `--yes` nao trava, usa default + aviso (R-07)
- [ ] **T5.5** [ ] impl: perguntas de IDE e projeto; persistir em RF-02

## P6 — artefatos das 2 IDEs (Risco M)
- [ ] **T6.1** [ ] teste: `brain setup opencode` gera plugin com schema valido
- [ ] **T6.2** [ ] teste: `brain setup kiro` gera `.kiro/hooks/*.json` com `version:v1`, trigger e `action`
- [ ] **T6.3** [ ] teste: o binario chamado pelo hook e o `brain hook` Rust (nao python)
- [ ] **T6.4** [ ] BDD: hook sem servidor MCP nao escreve erro na conversa

## Verificacao (H6) — pre-condicao de fechar
- [ ] `cargo test --workspace` 0 falhas
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 0
- [ ] `.venv/bin/python -m pytest src/tests/ -q` -> 124 passed, 2 legadas (gate H1.2, **esta feature nao remove Python**)
- [ ] `brain --help` mostra `setup` com o novo alvo
- [ ] 6 mutacoes aplicadas, 6 pegas
