---
name: brain_recent
description: >
  The most recently updated notes, newest first, with full content. Use for
  handoff between agents and to see what changed since your last session — not
  for finding something by topic, which is brain_search.
---

# brain_recent

Lista as notas mais recentemente atualizadas, da mais nova para a mais antiga.

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `top_k` | ❌ | `10` | Quantas notas. Inteiro `0`–`255` |

## Retorno

```json
{"recent": [
  ["regras/global/naming", "regras", "global", "# Conteúdo markdown completo…"],
  ["sessoes/meu-app/2026-09-26", "sessoes", null, "## Sessão…"]
]}
```

⚠️ **Cada item é um array posicional, não um objeto.** A tool devolve
`Vec<(path, layer, scope, content)>` e serde serializa tupla como array
(`brain-store:1915-1920`). Então o acesso é por índice:

```python
for path, layer, scope, content in status["recent"]:
    ...
```

Não espere `item["path"]` — isso dá `TypeError`.

Notas com `expires_at` vencido **não** aparecem (`brain-store:1917`): expirada
some do `recent` antes do sweep, o que é diferente de estar apagada.

## Quando usar

- Handoff: "o que o outro agente fez por último".
- Depois de uma pausa, para reencontrar o estado sem refazer buscas.

## O que NÃO fazer

- **Não use para procurar por assunto.** Isso é `brain_search`; `recent` é
  ordem de edição, não relevância. Uma nota topicalmente perfeita e não editada
  há semanas não aparece.
- **Não espere `top_k=0` para "só contar".** `LIMIT 0` devolve lista vazia
  (`brain-store:1917`) — não um total. Para contagem, `brain_status`.
- **Não confie no `content` para edite.** A nota veio de outra fonte; releia com
  `brain_read` antes de sobrescrever, senão você pode apagar texto escrito entre
  a sua leitura e o seu `brain_store`.
- **Não estranhe `scope: null`.** É o normal para `sessoes`, `projetos` e
  `indexacao`, que não usam scope.
