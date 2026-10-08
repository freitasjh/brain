---
name: brain_checkpoints
description: >
  Read the audit log: every create, update and delete, newest first, with the id
  that brain_restore takes. Call it before any destructive operation to find out
  what a restore would actually do.
---

# brain_checkpoints

Lê o audit log: o histórico de `create`, `update` e `delete` de cada nota.

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `limit` | ❌ | `10` | Quantas entradas. Inteiro `0`–`255` |

## Retorno

```json
{"checkpoints": [
  [42, "update", "regras/global/naming", "2026-09-27 14:02:11"],
  [41, "create", "regras/global/naming", "2026-09-20 09:31:00"],
  [40, "delete", "sessoes/meu-app/2026-01-02", "2026-09-19 18:44:02"]
]}
```

⚠️ **Array posicional de 4 campos, na ordem `(id, action, path, at)`** — não é
objeto. A tool devolve `Vec<(i64, String, String, String)>`
(`brain-store:1922-1925`).

```python
for cp_id, action, path, at in result["checkpoints"]:
    ...
```

O `content` anterior **não** vem aqui — ele está no banco, e `brain_restore` o
aplica. Se você precisa ver o texto antes de restaurar, não dá para ver por esta
tool: restaure para uma cópia e leia, ou restitua o conteúdo na mão.

## `action` decide o que o restore faz

Este campo é o mais importante da resposta, porque `brain_restore` **não** é um
undo genérico:

| `action` | O que `brain_restore(id)` faz |
|----------|-------------------------------|
| `create` | **apaga a nota** (`brain-store:1934`) |
| `update` | volta o texto, e **zera** `project`/`tags`/`pinned`/`expires_at` |
| `delete` | recria a nota com o texto anterior, com o mesmo zeroing de metadado |

## Quando usar

- **Antes de qualquer `brain_delete`, `brain_restore` ou `brain_forget_sweep`**,
  para saber o que existe e o que é recuperável.
- Para encontrar o `id` de um `brain_restore`.
- Para auditar quem mudou o quê.

## O que NÃO fazer

- **Não passe `limit` alto esperando paginação.** Não há offset nem cursor: é
  `ORDER BY at DESC LIMIT ?` (`brain-store:1923`). Acima de ~255 você perde o
  histórico antigo sem forma de chegar nele.
- **Não confunda `at` com ordem de causalidade rigorosa.** É
  `datetime('now')` do SQLite, com resolução de **segundo** e sem fuso
  (`brain-store:904`). Duas escritas no mesmo segundo empatam, e a ordem entre
  elas é a do `id`.
- **Não use isto como log de auditoria confiável.** `audit_log` não tem foreign
  key e nada o expurga — mas também não registra `brain_export` nem
  `brain_backup`, que escrevem no disco sem tocar no banco.
- **Não chame `brain_restore` com um `id` que você não leu aqui.** O `action`
  errado apaga a nota.
