---
name: brain_backup
description: >
  Copy the whole brain database to a .bak file. Use before a risky bulk change or
  to hand the complete corpus to someone else. Read the "what NOT to do" section
  before restoring anything — a .bak is a full production database, not a patch.
---

# brain_backup

Copia o `brain.db` inteiro para um arquivo `.bak`. É a única tool que leva o
banco completo — todas as notas, todos os vetores, o audit log.

## O que acontece com o dado do cliente se você chamar errado

A tool em si só copia: ela **não** escreve no banco. O perigo está no uso
seguinte — **restaurar um `.bak` por cima do `brain.db`**:

- Um `.bak` de origem desconhecida é um **banco de produção inteiro** de outra
  hora, com o corpus dele. Copiar ele por cima do `brain.db` de hoje **apaga
  tudo que foi escrito depois** e traz de volta notas que já foram apagadas.
- A cópia é de **um arquivo só** (`std::fs::copy` do `brain.db`,
  `rmcp_service.rs:309`). O banco roda em **WAL** (`brain-store:690-709`), então
  existe um sidecar `brain.db-wal` que **não é copiado** — commits recentes podem
  estar só nele. Um `.bak` restaurado pode, por isso, voltar sem as últimas
  escritas.
- Uma cópia desconhecida também traz **o que foi apagado**: o audit log é
  parte do arquivo e nada é filtrado na cópia.

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `to` | ❌ | `<db>.bak` (ao lado do banco) | Destino. **Precisa** ser absoluto, terminar em `.bak` e estar **dentro** de `BRAIN_EXPORT_ROOT` |

Sem `to` o destino é `{db}.bak`, e esse caso é **sempre permitido** — é um
irmão de um arquivo que o processo já lê e escreve (`fs_guard.rs:354-356`).

## Retorno

```json
{"ok": true, "to": "/home/joao/fusionlf/wsProjeto/brain/data/brain.db.bak"}
```

Só `ok` e `to`. A tool **não** diz se o destino já existia — `fs::copy` sobrescreve
em silêncio, então um `to` repetido **substitui** o `.bak` anterior sem aviso.

## O que NÃO fazer

- **Não restaure um `.bak` de origem desconhecida.** Se você não sabe de quando
  ele é, o custo é perder o corpus inteiro mais recente. Consulte `brain_checkpoints`
  para reconstituir o que mudou — o audit log está no banco vivo, e
  `brain_restore` reconstrói nota por nota.
- **Não apague o `brain.db` antes de restaurar.** A forma segura de testar um
  backup é copiar **para** um scratch e abrir o scratch, nunca o contrário.
- **Não mande `to` para fora de `BRAIN_EXPORT_ROOT`.** Recusado. E um destino
  repetido sobrescreve sem perguntar — versione você mesmo
  (`.../brain-2026-09-27.bak`).
- **Não use isto como se fosse o export.** Backup = banco. Export = notas em
  texto. São Complementares, e o export é o único dos dois que você pode
  regenerar.
- **Não mande `to` com caminho relativo.** É recusado: o destino tem que ser
  absoluto (`fs_guard.rs:358-360`), senão o significado depende do diretório de
  trabalho de quem chamou.

## Erros que você vai ver

| Erro | Causa |
|------|-------|
| `backup destination must be an absolute path` | `to` relativo |
| `backup destination must end in .bak` | extensão diferente de `bak` |
| `must stay inside the export root …` | `to` fora da raiz |
| `is writable by any local user (mode …)` | a raiz é legível por outro usuário local — e um backup é o banco inteiro |
