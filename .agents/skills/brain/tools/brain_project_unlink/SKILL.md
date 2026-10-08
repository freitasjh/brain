---
name: brain_project_unlink
description: >
  Remove the link between a note and a project, leaving the note itself and its
  ownership untouched. Use to take a note out of a project grouping. The project
  must exist; the link does not have to.
---

# brain_project_unlink

Desliga uma nota de um projeto. A nota **continua existindo**; só o link some.

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `note_path` | ✅ | — | Path **completo** da nota: `layer/scope/path` |
| `project` | ✅ | — | Nome do projeto. Precisa existir |

Projeto inexistente → `project not found: {name}` (`brain-store:980`).

## Retorno

```json
{"ok": true}
```

⚠️ O `ok` **espelha o resultado do delete** e é este que importa:
`{"ok": false}` significa "não havia link" — não é erro, e a tool **não** falha
neste caso. É o único ponto do conjunto em que um "não fez nada" volta como
sucesso.

```python
if not result["ok"]:
    # não estava ligado — nada mudou
```

## O que NÃO acontece aqui

- **A nota não é apagada.** Só a linha de `note_projects` sai
  (`brain-store:981`).
- **A posse não muda.** `notes.project_id` continua apontando para o projeto. Se a
  nota era **owned** por ele, ela continua aparecendo em
  `brain_project_notes` — que consulta as duas relações (`brain-store:949,956`).
  Desligar não tira a nota owned da lista.
- **Não há entrada de audit.** É a diferença em relação a `brain_store`: nada é
  gravado em `audit_log`, então não há como desfazer um unlink errado pelo
  `brain_restore`. Refazer é um `brain_project_link` de novo.

## Quando usar

- Retirar uma nota de um agrupamento sem destruí-la.
- Corrigir um link criado por engano.

## O que NÃO fazer

- **Não use para tirar a nota do projeto dela.** Isso é `brain_store` sem
  `project` (ou com outro), não unlink.
- **Não ignore `{"ok": false}`.** Se você está num laço de limpeza, é o único sinal
  de que aquele par já não estava ligado — e o laço seguir como se tivesse
  funcionado esconde link órfão em outro lugar.
- **Não passe path parcial.** Como no `brain_project_link`, o path vai cru para a
  query: o `DELETE` casa `note_path` exato. `regras/naming` não desliga
  `regras/global/naming` — e ainda assim pode dizer `ok: false` sem doer.
- **Não chame em `nota que nunca foi linked` esperando erro.** Não há erro; há
  `ok: false`.
