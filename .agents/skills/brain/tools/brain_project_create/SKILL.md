---
name: brain_project_create
description: >
  Create a project — the namespace that groups a set of notes. Most agents never
  need this, because brain_store creates the project implicitly when you pass
  project= to it. Use it when you want the project to exist with a description
  before any note references it.
---

# brain_project_create

Cria um projeto — o agrupamento que liga notas a um mesmo contexto.

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `name` | ✅ | — | Nome do projeto. É a chave: não pode repetir, e é o que `brain_search(project=…)` casa |
| `description` | ❌ | `""` | Texto livre |

`name` é aparado dos dois lados (`brain-store:920`), então `" x "` e `"x"` são o
mesmo projeto — e o segundo `create` falha por duplicata.

## Retorno

O objeto `Project` **directly**, sem wrapper:

```json
{"id": 7, "name": "atlas-ecm", "description": "ECM do grupo Atlas", "created_at": "2026-09-27 14:02:11"}
```

Erro em nome repetido: `INVALID_PARAMS: project exists: …` — a mensagem vem do
erro do SQLite, então o texto depois do `:` não é uma frase feita para ser lida.

## Provavelmente você não precisa disto

`brain_store(project=…)` **cria o projeto se ele não existir**
(`brain-mcp:183-188`):

```rust
store.project_get(name)?.or_else(|| store.project_create(name, "").ok())
```

Isso é deliberado, e é a armadilha: **um `project` com typo cria um projeto novo
em vez de falhar.** Um `brain_store(project="atlas-ecom")` quando você queria
`atlas-ecm` não dá erro — cria `atlas-ecom` e a nota fica linked a ele, invisível
para quem filtra pelo nome certo. Para ver que o projeto errado nasceu,
`brain_project_list` é o jeito.

Então: use `brain_project_create` quando quiser o projeto **antes** das notas (para
a `description` valer) ou quando quiser um erro cedo e explícito de duplicata.

## O que NÃO fazer

- **Não crie o projeto só para poderlinkar.** `brain_project_link` exige que o
  projeto exista, mas `brain_store` já resolve isso sozinho.
- **Não recrie para "renomear".** Não há update: `create` em nome novo e
  `link` nas notas, ou apague o antigo — que **não** apaga as notas ligadas
  (só o registro em `projects`).
- **Não dependa do `id`.** Ele é autoincrement e é interno; toda tool de projeto
  aceita `name`.
- **Não use `description` como documentação.** Nenhuma busca a indexa — é um
  campo de bookkeeping. O que a busca lê é a nota.
