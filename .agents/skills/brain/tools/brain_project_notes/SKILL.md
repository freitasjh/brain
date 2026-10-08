---
name: brain_project_notes
description: >
  Every note owned by or linked to a project, as full Note objects, newest first.
  Use to enumerate a project's corpus in one call instead of paginating
  brain_search.
---

# brain_project_notes

Lista as notas de um projeto — as **owned** e as **linked**, com o conteúdo
completo.

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `name` | ✅ | — | Nome do projeto. Precisa existir |
| `description` | ❌ | — | **Ignorado.** O mesmo struct serve a `brain_project_create`; aqui não tem efeito |

Erro se o projeto não existir: `INVALID_PARAMS: project not found: {name}`
(`brain-store:946`).

## Retorno

Array de `Note` **directly**, sem wrapper:

```json
[{
  "path": "arquitetura/projetos/atlas-ecm/stack",
  "layer": "arquitetura", "scope": "projetos",
  "content": "# Stack\n…",
  "project_id": 1, "tags": ["java", "spring"],
  "pinned": true, "expires_at": null, "version": 4
}]
```

Duas consultas, unidas: `notes.project_id = id` (a nota **pertence** ao projeto) e
`notes JOIN note_projects` (a nota está **ligada**). São relações diferentes e
levam a tools diferentes — `brain_project` é posse, `brain_project_link` é
many-to-many (`brain-store:949,956`).

Notas com `expires_at` vencido são omitidas nas duas consultas
(`brain-store:949,956`) — igual ao `recent`.

⚠️ `path` aqui é o **completo** (`layer/scope/path`), e é exatamente o que
`brain_read` e `brain_delete` aceitam. Não é preciso remontar.

## Quando usar

- Enumerar o corpus de um projeto de uma vez.
- Conferir o que um projeto tem antes de um `brain_delete` em massa.

## O que NÃO fazer

- **Não espere `{"notes": [...]}`.** É o array pelado. (O `brain_recent` e o
  `brain_checkpoints` **sim** usam envelope; aqui não.)
- **Não use como "buscar no projeto".** Não há ranking nem query: devolve
  **todas** as notas, ordenadas por `updated_at DESC`, o conteúdo inteiro incluso.
  Para filtro por assunto dentro do projeto, `brain_search(project=…)`.
- **Não passe `description` esperando filtro.** É aceito pelo schema e ignorado —
  erro silencioso, não erro visível.
- **Não conte com ordem estável.** `updated_at` tem resolução de segundo; duas
  notas editadas no mesmo segundo podem vir em qualquer ordem.
