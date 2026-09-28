# PROPOSAL — brain-help-visual

## Problema
`brain --help` exibe 22 comandos em lista plana, sem hierarquia. Duas entradas
(`reindex`, `server`) têm one-liners de ~40 palavras; `server` vaza markdown
literal (`SIGINT **or** SIGTERM`) no terminal. Usuário novo não identifica onde
procurar nem por onde começar.

## Solução (opção A, aprovada pelo usuário)
Reorganizar SOMENTE o help raiz:
1. Agrupar comandos em 5 categorias via mecanismos do próprio clap 4
   (`flatten_help` + `next_help_heading`).
2. Encurtar os 2 one-liners defeituosos; remover vazamento de markdown.
3. Bloco de exemplos de uso inicial no rodapé (`after_help`).

## Escopo
- ENTRA: `brain --help` / `brain -h` raiz; `crates/brain-cli/src/main.rs` apenas.
- NÃO ENTRA: `brain <cmd> --help` de subcomandos; novos comandos; mudança de
  comportamento; viewer web; README/docs.

## Grupos aprovados
| Grupo | Comandos |
|---|---|
| Uso comum | ping, store, read, search, recent, status |
| Memória | delete, checkpoints, restore, backup, export |
| Manutenção | reindex, migrate, forget-sweep |
| Servidor | serve, serve-mcp, server, hook |
| Projetos e setup | project, setup |

## Riscos
- Teste `crates/brain-cli/tests/server_start.rs:489-494` afirma que o help raiz
  contém `server` e que `brain server --help` contém `start` — agrupar NÃO pode
  remover essas strings.
- clap 4.6.6: confirmar `flatten_help` + `next_help_heading` disponíveis
  (confirmado: 4.6.6 >= 4.2 onde ambos existem).

## Delta Specs
- Nenhum doc existente descreve o texto do help; sem deltas em `.agents/rules/`,
  `AGENTS.md` ou README. Se o developer precisar de bump de versão
  (regra: patch em fixes), registrar em `workflow-state.json`.
