---
name: brain_read
description: >
  Read one full note by its complete path (layer/scope/path). Use when a
  brain_search result has to be read in full, or before overwriting a note you
  did not write in this session.
---

# brain_read

Lê uma nota completa.

## Parâmetros

| Nome | Obrigatório | Descrição |
|------|-------------|-----------|
| `path` | ✅ | Path **completo** da nota: `layer/scope/path` |

**Um parâmetro só.** Não existe `layer` nem `scope` separados: as duas coisas já
fazem parte do `path`. A tool recebe um único campo `path`
(`rmcp_service.rs:55-58`).

## Como usar

O `path` que você quer é o que `brain_search` e `brain_project_notes` já
devolveram — copie, não remonte:

```python
hit = brain_search("naming conventions", layer="regras")["results"][0]
note = brain_read(hit["path"])          # path = "regras/global/naming"
print(note["content"])
```

Não tire nem acrescente nada: o path **não tem extensão** (o `.md` nunca fez
parte do que é gravado, `brain-mcp:134-139`), e o scope faz parte dele.

Para um path que você mesmo está montando:

```
regras/global/naming              → regras + global
regras/projetos/meu-app/naming    → regras + projetos
sessoes/meu-app/2026-09-27        → sessoes, sem scope
```

> **A CLI é diferente da tool.** `brain read <layer> <path> --scope <scope>`
> monta o path completo para você (`brain-cli:658-661`), enquanto a tool MCP
> recebe o path pronto. Não copie a forma de uma para a outra.

## Retorno

```json
{"path": "regras/global/naming", "content": "# Nomes de tabela\n…",
 "layer": "regras", "scope": "global"}
```

O `content` é o markdown **exato** que foi gravado — sem normalização. Nota que
não existe: erro `INVALID_PARAMS: not found: {path}` (`rmcp_service.rs:255`).

## O que NÃO fazer

- **Não passe `layer` e `path` separados.** Não é a assinatura; e como o schema
  só tem `path`, um `layer` extra é ignorado em silêncio e o erro que volta é
  "not found" — o que parece nota ausente, não chamada errada.
- **Não junte `.md`.** `brain_read("regras/global/naming.md")` não acha
  `regras/global/naming`.
- **Não esqueça o scope.** `regras/naming` **não** acha `regras/global/naming` nem
  `regras/projetos/naming`. O scope é o segundo segmento, e é ele que separa
  conhecimento universal de conhecimento do seu projeto.
- **Não sobrescreva com base numa leitura antiga.** Se a nota foi editada por
  outro agente entre a sua leitura e o seu `brain_store`, você apaga o texto
  dele. Releia imediatamente antes de escrever.
