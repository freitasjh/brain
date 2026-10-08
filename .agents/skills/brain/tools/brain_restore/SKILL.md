---
name: brain_restore
description: >
  Restore a note to the content it had at an audit_log id. Overwrites the note as
  it is now, and what it restores depends on the action: restoring a "create"
  entry DELETES the note. Always read brain_checkpoints first.
---

# brain_restore

Reescreve uma nota com o conteúdo que ela tinha num ponto do audit log.

## O que acontece com o dado do cliente se você chamar errado

A nota é **sobrescrita** com o texto antigo. Tudo que foi escrito nela depois
**segue gravado** — a tool não pergunta e não faz merge. E o que acontece depende
do `action` da entrada, que **você precisa ler antes**:

| `action` da entrada | O que o restore faz |
|---|---|
| `create` | **APAGA a nota.** `restore_audit` mapeia `create` para `note_delete` (`brain-store:1934`) — restaurar a criação desfaz a criação |
| `update` | Sobrescreve o conteúdo com o de antes, e **perde metadado** (abaixo) |
| `delete` | Recria a nota com o conteúdo anterior, e **perde metadado** (abaixo) |

O item `update` é o que quase todo mundo espera ("desfazer a última edição") e
funciona — mas com um custo que não está no nome da tool. O `restore` chama
`note_upsert` com **metadado hardcoded**, não com o que a nota tinha
(`brain-store:1941`):

```rust
self.note_upsert(&path, &layer, scope, &prev_content, None, &[], false, None)
//                                                      project  tags  pinned  expires_at
```

Então um `restore` bem-sucedido devolve o **texto** e zera:

- `project` → perdido (a nota deixa de ter dono; os links em `note_projects`
  sobrevivem)
- `tags` → **perdidas** (vira `[]`)
- `pinned` → **perdido** (vira `false`)
- `expires_at` → **perdido** (vira `null` — uma nota que estava vencendo volta
  como permanente, e uma que ia vencer nunca mais vence)

O `restore` também grava uma **nova** entrada `update` no audit log
(`brain-store:1004`), então ele é reversível — mas o conteúdo do metadado já foi.

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `id` | ✅ | — | `id` de `brain_checkpoints`. **Inteiro**, não path |

## Retorno

```json
{"ok": true}
```

Erro: `audit not found: {id}` quando o `id` não existe **ou** a entrada existe mas
o `action` não é restaurável (`brain-store:1956` cai no `_ => Ok(false)`). A
mensagem não distingue os dois casos — se ela aparecer, confira se você pegou o
`id` da linha certa e qual era o `action`.

## O que NÃO fazer

- **Não chute o `id`.** Chame `brain_checkpoints` e use o `id` que veio de lá.
  Como o `action` decide entre apagar e sobrescrever, um `id` errado é um
  `brain_delete` ou uma sobrescrita que você não pediu.
- **Não restaure entrada `create` esperando trazer a nota de volta.** Traz o
  oposto: apaga. Se você quer o conteúdo de uma nota que foi criada, o que você
  quer é o `id` da entrada `update` seguinte.
- **Não restaure sem salvar o texto atual.** Se a nota evoluiu desde então e você
  ainda quer as duas versões, `brain_read` antes e guarde o conteúdo — o restore
  não faz merge, e o metadado (tags, pin, TTL, projeto) não volta.
- **Não use como undo de `brain_delete` em massa.** Um por vez, conferindo o
  `action` de cada entrada.
