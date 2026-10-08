---
name: brain_export
description: >
  Dump every note to a directory of plain files, for reading outside the brain or
  handing the corpus to another tool. Restricted to BRAIN_EXPORT_ROOT. Use when
  you need the notes as files — NOT as a backup (that is brain_backup) and never
  as a source to delete.
---

# brain_export

Escreve cada nota em um arquivo dentro de `BRAIN_EXPORT_ROOT`, para você ler o
corpus fora do brain.

## O que acontece com o dado do cliente se você chamar errado

Nada some do brain: a tool **só lê** o banco e escreve uma **cópia derivada**. O
`brain.db` é o original e não é tocado. O risco real é o oposto do que parece —
tratar a cópia como se fosse a fonte e sair apagando, ou supor que o export é
cópia fiel de tudo. Ele não é:

| O que você pode supor | O que é verdade |
|---|---|
| "o export tem tudo" | **Não.** Notas expiradas (`expires_at` vencido) são puladas, e o teto é 10.000 notas (`rmcp_service.rs:329` itera `recent(10000)`, que filtra expiradas em `brain-store:1917`) |
| "cada arquivo é `<path>.md`" | **Não.** O nome do arquivo é o path da nota, **sem extensão**: `regras/global/naming`, não `naming.md` (`fs_guard.rs:391` faz `dir.join(note_path)`; o path nunca tem `.md`, `brain-mcp:134-139`) |
| "`force` limpa o diretório" | **Não.** `force` só autoriza escrever em cima. Arquivos de um export anterior que não existem mais no banco **continuam lá** (`rmcp_service.rs:319-321`) |
| "`to` é onde eu quiser" | Recusado se apontar fora de `BRAIN_EXPORT_ROOT` |

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `to` | ❌ | a raiz (`BRAIN_EXPORT_ROOT`) | Subdiretório **relativo à raiz**. Relativo ou absoluto, mas tem que resolver **dentro** da raiz |
| `force` | ❌ | `false` | Necessário se o destino já existir |

## Retorno

```json
{"ok": true, "to": "/tmp/brain-export", "written": 253, "refused": 0}
```

| Campo | Significado |
|-------|-------------|
| `to` | diretório efetivamente escrito |
| `written` | notas gravadas |
| `refused` | notas **não** gravadas — path que escapava do diretório, ou erro de escrita. `refused > 0` = export incompleto |

`refused` não é cosmético: um `refused` alto significa que o diretório não
representa o corpus.

## O que NÃO fazer

- **Não apague o export achando que ele é o original.** O original é o
  `brain.db` (ou o `vault/` legado). O diretório de export é regenerável a
  qualquer momento e não tem nada de único. Apagar o export não destrói memória;
  tratar o export como fonte e escrever por cima dele, sim, perde trabalho.
- **Não use `to` para sair da raiz** esperando funcionar. `to=/etc/x` ou
  `to=../vault` é recusado com erro `INVALID_PARAMS`, resolvido por
  canonicalização — `..` e symlink não passam (`fs_guard.rs:296-318`).
- **Não conte com `force` para recomeçar do zero.** Ele autoriza sobrescrever;
  arquivos órfãos de exports anteriores permanecem e você vai ler conteúdo que
  foi apagado do banco. Apague o diretório na mão se quiser um export limpo.
- **Não trate o export como backup.** Ele não tem o estado do banco: só as
  notas, sem vetores, sem fila, sem audit log. Para o banco, use `brain_backup`.
- **Não assuma que a raiz é privada por padrão.** A raiz precisa ser diretório,
  do seu `euid`, e não gravável por `other` (`fs_guard.rs:199-235`) — senão outro
  usuário local que criou `/tmp/brain-export` antes do primeiro start lê o corpus
  inteiro. A checagem acontece duas vezes: antes de criar e de novo depois
  (`rmcp_service.rs:325`), para fechar a corrida.

## Erros que você vai ver

| Erro | Causa |
|------|-------|
| `export dir exists, use force` | destino existe e `force` não foi enviado |
| `must stay inside the export root …` | `to` fora da raiz |
| `is writable by any local user (mode …)` | raiz gravável por `other` — `chmod o-w` ou aponte `BRAIN_EXPORT_ROOT` para outro lugar |
| `is owned by uid … but this process runs as uid …` | raiz de outro usuário |
