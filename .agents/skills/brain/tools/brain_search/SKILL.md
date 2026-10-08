---
name: brain_search
description: >
  Search the Brain by hybrid relevance — full-text plus vector, fused, with
  optional layer/scope/project/tag filters. Call BEFORE coding and before
  answering from memory, to load decisions and rules from past sessions.
---

# brain_search

Busca no cérebro por relevância: **palavra-chave + semântica**, fundidas.

É a tool que mais se chama e a que mais responde. Chame **antes** de codar, não
depois — o que ela traz é contexto que evita retrabalho.

## Parâmetros

| Nome | Obrigatório | Default | Descrição |
|------|-------------|---------|-----------|
| `query` | ✅ | — | Texto da busca. Vazio ou só espaços é erro (`rmcp_service.rs:261-263`) |
| `layer` | ❌ | `null` | Restringe a uma camada |
| `scope` | ❌ | `null` | Restringe a `projetos` ou `global` |
| `project` | ❌ | `null` | Restringe ao projeto — **inclui** notas linked, não só owned |
| `tag` | ❌ | `null` | Restringe a uma tag. Comparação **exata**, sem caixa e sem parcial (`brain-store:1799`) |
| `top_k` | ❌ | `5` | Máximo de resultados. **Preso a 1–20**: um valor maior é cortado em 20 (`rmcp_service.rs:267`) |

## Retorno

```json
{"results": [{
  "path": "regras/global/naming",
  "layer": "regras", "scope": "global",
  "score": 0.032, "snippet": "…trecho relevante…",
  "chunk_index": 2, "project": "atlas-ecm", "tags": ["naming"]
}], "total": 1}
```

| Campo | Significado |
|-------|-------------|
| `path` | completo (`layer/scope/path`) — é o que `brain_read` e `brain_delete` aceitam |
| `score` | score fundido. **Magnitudes pequenas** (RRF): 0.03 é excellent, não é "fraco" |
| `snippet` | o trecho que casou, **não** a nota inteira |
| `chunk_index` | qual chunk da nota casou. Uma nota pode aparecer em vários resultados |

`total` é o tamanho do array devolvido, não o total de candidatos.

**Um resultado por chunk, não por nota.** A mesma nota pode voltar 3 vezes, com
`chunk_index` diferente. Para a nota inteira, `brain_read` no `path`.

## Como fica o ranking

Quatro streams fundidos por RRF (k=60) — vetor, FTS5, entidade e grafo, cada um
pontuado `1.0/(60.0 + rank)` (`brain-store:1643,1719,1733,1744`) — mais um
**boost de autoridade**: `+0.15` para `arquitetura` e `regras`, `+0.10` para
`pinned` (`brain-store:1822-1823`). Notas expiradas são filtradas antes do score
(`brain-store:1798`).

**O FTS segura a busca sozinho.** Se o Ollama estiver fora, a chamada degrada
para só texto e **não dá erro** — o campo `explain` não aparece na resposta MCP
(explain é do CLI, `--explain`). Resultado vazio com Ollama no ar é sinal de
problema; com Ollama fora, é sinal de que o texto não casou.

## Quando usar

| Situação | Chamada |
|----------|---------|
| Antes de codar uma feature | `brain_search("<feature>")` |
| Regra do seu projeto | `brain_search("<tópico>", layer="regras", scope="projetos")` |
| Padrão universal | `brain_search("<tópico>", layer="regras", scope="global")` |
| Recomendação de biblioteca | `brain_search("async", layer="estudos", scope="global")` |
| Tudo de um projeto | `brain_search("<tópico>", project="atlas-ecm")` |

## O que NÃO fazer

- **Não peça `explain`.** Não é parâmetro da tool MCP — `SearchArgs` tem seis
  campos e `explain` não é um deles (`rmcp_service.rs:61-68`); o handler passa
  `false` fixo para o store (`rmcp_service.rs:267`). Passar `explain` é **ignorado
  em silêncio**. Para ver os quatro streams, é `brain search --explain` no CLI.
- **Não espere `score` alto.** RRF produz 0.01–0.05 para o topo. Se você está
  filtrando por `score > 0.5`, volta vazio com o índice saudável.
- **Não use `tag` como busca textual.** `tag="naming"` acha a tag `naming`, e
  **não** a tag `naming-convention` nem o conteúdo que menciona "naming"
  (`brain-store:1799`).
- **Não increase `top_k` achando que traz mais.** Acima de 20 é truncado em 20.
  Para volume grande, filtre por `project`/`layer` em vez de pedir 200.
- **Não interprete vazio como "não existe".** Pode ser filtro de mais, Ollama fora
  sem FTS casando, ou tag escrita diferente. Tente sem filtro antes de concluir.
