---
name: ping
description: >
  Health-check for the brain MCP connection. Returns {"pong": true} and touches
  nothing. Use it to confirm the server is reachable before blaming search for
  returning no results.
---

# ping

Verifica se o servidor MCP está vivo. Sem parâmetros, sem escrita, sem rede
além do próprio processo.

## Retorno

```json
{"pong": true}
```

Note a forma: é um **objeto JSON**, não a string `pong`. O CLI `brain ping` imprime
a palavra `pong` em texto puro (`brain-cli:634`) — as duas coisas são diferentes e
a comparação que um agente costuma fazer (`== "pong"`) falha na tool.

## Quando usar

- No começo da sessão, para distinguir "o brain não tem isso" de "o brain não
  está no ar". Sem isto, uma busca vazia é ambígua entre as duas.
- Quando `brain_search` volta vazio e `brain_status` também não responde.

## O que NÃO fazer

- **Não use para checar saúde do índice.** `ping` é deliberadamente burro: não
  abre o banco, não olha embedding, não fala com o Ollama. Para isso, `brain_status`.
- **Não interprete `pong` como "está tudo certo".** Um servidor com o Ollama
  fora do ar responde `pong` normalmente — a busca semântica é que degrada para
  só texto. `ping` verde com busca ruim é o cenário esperado, não uma contradição.
- **Não faça polling.** É uma chamada de rede por tentativa, sem cache.
