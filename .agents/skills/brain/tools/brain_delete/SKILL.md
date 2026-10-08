---
name: brain_delete
description: >
  Hard-delete a note and its chunks by full path. Recoverable only through the
  audit log, and the path is layer/scope/path — confirm it before calling,
  because there is no confirmation step.
---

# brain_delete

Apaga uma nota e os chunks dela. Não é lixeira: some da busca e do corpus.

## O que acontece com o dado do cliente se você chamar errado

A nota **some da busca e do `brain_export` no mesmo instante**. A boa notícia é
que ela **não** some para sempre: o `note_delete` grava uma entrada `delete` no
audit log **com o conteúdo anterior** antes de apagar (`brain-store:1178-1181`).
O `audit_log` não tem foreign key para `notes` (`brain-store:899-905`), então o
registro sobrevive à nota. Logo: `brain_checkpoints` + `brain_restore` trazem o
**texto** de volta.

O que **não** volta igual, se você precisar: `brain_restore` reaplica o conteúdo
com `project`, `tags`, `pinned` e `expires_at` zerados (veja `brain_restore`).
E o que é apagado junto, sem entrada de audit própria:

- **Os chunks e os vetores** — `chunks.note_id` é `ON DELETE CASCADE`
  (`brain-store:843`). Os embeddings são re-embebidos depois, se é que a nota
  voltar.
- **Os links com o projeto** — `note_projects` é limpa explicitamente
  (`brain-store:1184`).

O risco principal não é apagar: é apagar **a nota errada**. Não há passo de
confirmação, e o `path` é o caminho completo `layer/scope/path` — as duas
primeiras partes são reais, não opcionais.

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `path` | ✅ | — | Path **completo**: `layer/scope/path` para `arquitetura`/`regras`/`estudos`, `layer/path` para os demais |

Validações que o path passa (`brain_core::sanitize_relative_path`,
`brain-core:180-192`): não vazio, não começando com `/`, sem nenhum segmento `..`.

## Retorno

```json
{"ok": true}
```

Path inexistente: erro `INVALID_PARAMS: not found: {path}` — e **nada** é
escrito, nem audit (`brain-store:1178` só grava se achou conteúdo). Ou seja: um
path errado não cria ruído, ele só falha.

## O que NÃO fazer

- **Não apague sem confirmar o path completo.** Use o `path` que veio de um
  `brain_search` ou `brain_read` — eles devolvem o `layer/scope/path` pronto. Não
  monte na mão esperando o acerto do scope.
- **Não apague para "limpar" uma nota que você quer só desatualizar.** Isso é
  `brain_store` no mesmo path: a versão anterior vai para o audit log sozinha e o
  `pinned`/`expires_at`/`tags` continuam.
- **Não apague em laço sem `brain_checkpoints` antes.** A reversão é por entrada
  de audit, e ela não restaura metadado. Um `brain_checkpoints` de 10 antes do
  laço é o seguro mais barato que existe.
- **Não confunda com `brain_forget_sweep`.** Esta apaga **um** path, que você
  nomeia; aquela apaga tudo que está vencido, e `pinned` não segura.
