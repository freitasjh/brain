---
name: brain_project_link
description: >
  Attach an existing note to a project without changing the note's owner. Use to
  group one note under several projects. Both the note and the project must
  already exist.
---

# brain_project_link

Liga uma nota existente a um projeto, **sem** mudar quem é o dono da nota.

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `note_path` | ✅ | — | Path **completo** da nota: `layer/scope/path` |
| `project` | ✅ | — | Nome do projeto. Precisa existir |

Ambos são validados **antes** de qualquer escrita (`brain-store:970-976`):

- projeto inexistente → `project not found: {name}`
- nota inexistente → `note not found: {note_path}`

Ou seja: **não há criação implícita aqui.** Diferente de `brain_store(project=…)`,
que cria o projeto se faltar, esta tool falha. É o que torna dela uma boa forma
de pegar um typo — mas também significa que você tem que criar o projeto antes.

## Retorno

```json
{"ok": true}
```

## Duas relações, e a diferença importa

| Relação | Quem escreve | O que é |
|---------|--------------|---------|
| `notes.project_id` | `brain_store(project=…)` | **posse** — a nota pertence ao projeto |
| `note_projects` | `brain_project_link` | **link** many-to-many — a nota é citada por N projetos |

`brain_project_notes` faz `JOIN` nas duas (`brain-store:945-968`), então uma nota
linked aparece mesmo sem ser owned. E `brain_search(project=…)` também considera
as duas (`brain-store:1800-1805`) — o filtro não é só owned, como seria se a
tabela `note_projects` não existisse.

## Quando usar

- A mesma nota serve a dois contextos ("padrão de nomenclatura" em dois projetos).
- Reorganizar o agrupamento sem reescrever as notas.

## O que NÃO fazer

- **Não use para "mudar a nota de projeto".** Isso não muda o dono: cria um
  segundo link. Para mover, `brain_store` de novo com o `project` certo (o
  anterior sai, porque `note_upsert` sobrescreve `project_id`) e depois
  `brain_project_unlink` se o link antigo tiver sobrado.
- **Não espere deduplicar.** O insert é `INSERT OR IGNORE`
  (`brain-store:975`), então religar é silenciosamente um no-op — mesmo com
  `ok: true`. Não dá para distinguir "liguei" de "já estava ligado" pela resposta.
- **Não passe path parcial.** Aqui não há `sanitize_relative_path` nem montagem
  de path: o `note_path` vai cru para a query (`brain-store:973`) e só casa se
  for o path completo, com scope e tudo. `regras/naming` **não** acha
  `regras/global/naming`.
- **Não use como verificação.** Ligar a uma nota errada não dá erro, e o
  `brain_project_notes` do projeto passa a devolver uma nota que não é dele.
