---
name: brain_project_list
description: >
  List every project with its id, name, description and creation date, sorted by
  name. Use it to learn the exact project names before filtering a search by
  project, since brain_store creates projects implicitly and typos go unnoticed.
---

# brain_project_list

Lista todos os projetos, em ordem alfabética de `name`. Sem parâmetros.

## Retorno

Array de `Project` **directly**, sem wrapper:

```json
[
  {"id": 1, "name": "atlas-ecm", "description": "ECM do grupo Atlas", "created_at": "2026-07-02 10:11:00"},
  {"id": 7, "name": "hive", "description": "", "created_at": "2026-09-27 14:02:11"}
]
```

A ordenação é `ORDER BY name` no SQLite (`brain-store:935`), ou seja alfabética
ASCII: maiúsculas antes de minúsculas, e acento não conta.

`description` é `""` quando o projeto nasceu sozinho — foi o
`brain_store(project=…)` criando por baixo (`brain-mcp:183-188`). **Um projeto
sem descrição e com nome quase igual ao de outro é a assinatura de um typo.**

## Quando usar

- Antes de `brain_search(project=…)`, para usar o nome exato.
- Quando uma busca por projeto volta vazia e você suspeita que o nome está errado.

## O que NÃO fazer

- **Não espere `{"projects": [...]}`.** É o array pelado. `result["projects"]` dá
  `TypeError` — o envelope só existe em `brain_project_notes`, que tem nome
  próprio.
- **Não use `id` como referência estável entre installs.** É autoincrement
  local; toda tool aceita `name`.
- **Não presuma que o projeto tem notas.** A lista sai de `projects`, e um
  projeto pode existir sem nenhuma nota linked. Para as notas, `brain_project_notes`.
- **Não chame em laço.** Não há filtro nem paginação: devolve tudo, sempre.
