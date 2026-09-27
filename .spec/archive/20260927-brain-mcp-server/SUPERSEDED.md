# SUPERSEDED — brain-mcp-server (era Python + vault Obsidian)

**Arquivado 2026-09-27 sem `VERIFY.md`.** Decisão registrada:

## Por que não há VERIFY

Esta spec descreve uma **arquitetura que não existe mais**. `01-requirements.md`
abre com *"Um servidor MCP em Python que atua como cérebro central"* e diz que
o sistema *"persiste informações em um vault Obsidian"*. O sistema atual é
**Rust + SQLite-only, sem vault** (`AGENTS.md` stack; constraint **C01** de
`.spec/brain-rust-sqlite/01-requirements.md:19` — *"nenhum `vault/*.md` como
truth"*).

`03-tasks.md` não tem **nenhum** checkbox `[x]` ou `[ ]` — é um breakdown que
nunca foi executado como checklist. Fabricar um `VERIFY.md` aqui exigiria
avaliar trabalho de uma arquitetura já substituída, e o resultado seria um
documento que afirma conformidade a um design morto. Isso é exatamente o modo
de falha que os 5 ciclos de code review desta sessão caçaram: **afirmar mais do
que o código sustenta**.

## O que a substitui

`.spec/brain-rust-sqlite/` — **também arquivada**, em
`.spec/archive/20260917-brain-rust-sqlite/`, com `VERIFY.md` próprio
(PASS com débito TD-001). Foi ela que especificou a migração Python→Rust
(Fase A fundação + Fase B retrieval híbrido) e é a spec viva do projeto.

## Dívidas que esta spec deixou e que ainda valem

Extraídas antes de arquivar, porque são o conteúdo aproveitável:

1. **TD-004 / US-01.1** — a spec previa `brain server start`; o CLI Rust expõe
   `serve-mcp` e `serve`. O subcomando nunca existiu.
2. **TD-004 / B6** — a spec previa backup do import legado como
   `vault.bak.tar.gz`; `brain migrate` não empacota nada.
3. `01-requirements.md:16` tem **mojibake** (`verifica健康状况`,ideias em
   chinês no meio de português). Corrigido aqui por leitura; não vale a pena
   reescrever o resto de um documento arquivado.
