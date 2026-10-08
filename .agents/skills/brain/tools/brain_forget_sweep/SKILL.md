---
name: brain_forget_sweep
description: >
  Delete every note whose expires_at has passed. Destructive and irreversible
  outside the audit log. ALWAYS run with dry_run=true first and read the list
  before the real call — TTL beats pinned, so a pinned note is not protected.
---

# brain_forget_sweep

Apaga todas as notas cujo `expires_at` já venceu.

## O que acontece com o dado do cliente se você chamar errado

A nota **some**. Sem `dry_run` não há confirmação: a tool não pergunta nada, não
mostra o que vai apagar antes, e apaga tudo que estiver vencido na hora da
chamada. Dois fatos que surpreendem:

- **`pinned` NÃO protege.** A query de seleção é
  `WHERE expires_at IS NOT NULL AND expires_at <= now` — não filtra por `pinned`
  (`brain-store:2173`). Uma nota pinada **e** vencida é apagada assim mesmo; o
  código só emite `warn pinned+expiring … (TTL wins)` no log
  (`brain-store:2176`). O TTL vence o pin, sempre. (A descrição da própria tool
  no servidor diz "(pinned never expires)" — está **errada**; o código faz o
  contrário.)
- **A lista devolvida pode mentir.** O delete é `let _ = self.note_delete(p)` —
  o erro é **descartado** (`brain-store:2178`). Um path pode aparecer na resposta
  e não ter sido apagado. Não use a resposta como confirmação de que sumiu.

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `dry_run` | ❌ | `false` | `true` = só lista o que seria apagado, **não apaga** |

## Retorno

```json
{"deleted": ["sessoes/meu-projeto/2026-07-01", "estudos/projetos/x/y"]}
```

`deleted` é uma **lista de paths**, não uma contagem — com `dry_run` é a lista do
que *seria* apagado.

## O que NÃO fazer

- **Nunca chame sem `dry_run` primeiro.** Leia a lista. Se algum path não deveria
  vencer, corrija o `expires_at` **antes** — depois do sweep só resta o audit log.
- **Não confie em `pinned` como proteção.** Se você pinou uma nota e ela está
  vencida, ela vai ser apagada. Para não vencer, mande `expires_at` no futuro ou
  deixe `null`.
- **Não conte com reversão barata.** Cada nota apagada vira uma entrada `delete`
  no audit log com o conteúdo anterior (`brain-store:1180`), então
  `brain_checkpoints` + `brain_restore` trazem o **texto** de volta — mas o
  `restore` **não** devolve `tags`, `pinned`, `expires_at` nem o dono do projeto
  (veja `brain_restore`). Metadado perdido em sweep é perdido.
- **Não rode em horário de pico sem querer.** A tool varre o banco inteiro e
  deleta em laço, sem limite e sem transação: um sweep grande é uma janela longa
  de escrita.
- **Não use para "limpar o índice".** Isso é `brain_delete` num path, ou
  `brain reindex`.

## Erros que você vai ver

Nenhum erro próprio: a seleção é por `expires_at` e apaga o que achar. O
`warn pinned+expiring` vai para o log do servidor, não para a resposta.
