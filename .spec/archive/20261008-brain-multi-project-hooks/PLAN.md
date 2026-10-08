# PLAN — brain-multi-project-hooks

## 1. Fases (TDD, ordem obrigatoria por dependencia)

| # | Fase | Depende | Entrega |
|---|---|---|---|
| P1 | config.json: leitura/escrita | — | `config.rs` em brain-cli: parse, gravar entrada, detectar orfao |
| P2 | cascata de resolucao | P1 | `resolve_project(dir, store) -> (nome, origem)` |
| P3 | `brain hook --project` opcional | P2 | usa a cascata; imprime origem |
| P4 | injetar a pergunta real | P3 | query vira a mensagem do usuario; projeto vira filtro |
| P5 | `setup`: IDE + projeto, interativo | P1 | `kiro` como alvo; perguntas com `--dry-run`/`--force`/`--yes` |
| P6 | artefatos das 2 IDEs | P4,P5 | plugin JS (opencode) + `.kiro/hooks/*.json` |

## 2. Modo de falha por fase

| Fase | Falha | Comportamento exigido |
|---|---|---|
| P1 | JSON invalido / ilegivel | **nao sobrescreve**; avisa stderr; segue sem config (cascata cai no git/nome) |
| P1 | `BRAIN_DIR` ausente | cria `~/.brain` |
| P2 | `git` ausente ou repo sem remote | passo 2 falha -> passo 3 -> passo 4 |
| P2 | cwd nao e diretorio | erro nomeado, nao panic |
| P3 | config diz `desabilitado` | origem `disabled`; **nao pergunta**, nao busca |
| P3 | config diz `projeto: null` (recusado) | origem `recusado`; **nao pergunta de novo** |
| P4 | pergunta vazia | cai no texto fixo antigo (nao pior que hoje) |
| P4 | Ollama fora | fila enfileira; FTS ainda pontua |
| P5 | stdin nao e tty e sem `--yes` | **nao trava**: usa default e avisa |
| P6 | plugin/carregamento falhar | arvore sem hook, IDE funciona |

## 3. Modos de falha globais (R-07 degradacao silenciosa)
- Sem servidor MCP em 8321 → hook **nao** escreve erro na conversa.
- Sem Ollama → `brain_search` degrada para FTS (comportamento existente).
- `brain hook` != 0 → a IDE ignora (hooks sao best-effort por natureza), mas o
  codigo de saida deve distinguir **degradado** (0, aviso em stderr) de **erro** (≠0).

## 4. Testes por fase (padrao do repo: binario real, sem rede/Ollama real — H2.3)

| Fase | Teste | Mutacao que tem que pegar |
|---|---|---|
| P1 | config roundtrip; JSON invalido nao sobrescreve | gravar sem ler antes |
| P2 | 4 passos da cascata, um teste por passo | trocar ordem dos passos |
| P3 | `--project` explicito identico ao atual; ausente usa cascata | voltar `--project` obrigatorio |
| P4 | query = mensagem do usuario | voltar a query fixa |
| P5 | `--dry-run` nao escreve; sem tty nao trava | remover o bypass |
| P6 | artefato gerado tem schema valido | remover campo obrigatorio |

**Nunca** `env_remove("BRAIN_OLLAMA_URL")` — seleciona o default real (H2.3).
Apontar para `http://127.0.0.1:1` ou mock em porta efemera.

## 5. Fora do escopo (reinforceco)
- Fase C (remocao do Python) — lote separado.
- `hooks/brain-kiro-steering.md` (259 L) — R-08, commit posterior.
- Bug `fts5: syntax error` — `.spec/bugs/` separado.
- Nenhum DDL: `config.json` e arquivo, `projects` ja existe.

## 6. Ordem de merge
P1+P2+P3 num commit (o nucleo da resolucao). P4 no seguinte (muda o que volta
para a conversa — o mais observavel). P5+P6 no terceiro (instalador + artefatos).
