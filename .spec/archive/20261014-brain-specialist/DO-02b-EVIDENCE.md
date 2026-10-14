# DO-02b — EVIDÊNCIA demo handoff pesquisável (scratch)

> Executado 2026-10-14. Scratch `/tmp/do02b-scratch.8y2c52`, mock Ollama
> `/tmp/mock_ollama.py` (porta efêmera 37111, `POST /api/embeddings` dim 768
> determinístico) — vetores reais, produção intocada.
> Log integral: `/tmp/do02b.log`.

## 1. Handoff 3 camadas (projeto `do02b-demo`, termo `zircon-handoff`)

```
store sessoes do02b-demo/2026-10-14-zircon-handoff  → chunks=1 embedded=1
store estudos do02b-demo/zircon-handoff             → chunks=2 embedded=2
store regras  do02b-demo/zircon-handoff             → chunks=1 embedded=1
```

Sem `without_embedding`: mock dá vetor na hora. Armadilha do roteiro:
conteúdo começando com `---` exige `--` antes (`store … -- "---\n…"`),
senão o clap lê como flag — primeira tentativa falhou assim, refeita com
`--` (falha honesta registrada, não escondida).

## 2. `search "zircon-handoff" --top-k 3` → as 3 camadas (BDD achado top-3)

```
total: 3
1 regras/projetos/do02b-demo/zircon-handoff
2 estudos/projetos/do02b-demo/zircon-handoff
3 sessoes/do02b-demo/2026-10-14-zircon-handoff
```

## 3. Spam ≤5/dia/projeto (BDD edge)

`notas_hoje_do02b-demo = 3` (as 3 acima, via `notes.created_at`) → `spam_ok`.

## 4. Loop de qualidade do estudo (BDD failure: sem loop → reprovado)

Positivo (estudo com frontmatter): `tags:/topico:/nivel:/fonte: ok` +
`read` devolve o conteúdo integral idêntico ao escrito.

Negativo (nota sem frontmatter submetida ao mesmo check):
`reprovado no loop, faltando: ['tags:','topico:','nivel:','fonte:']` —
o check discrimina, não passa tudo.

## Produção

`LSOF_BEFORE == LSOF_AFTER == [1380 1381 10906 456874]`; mock morto ao fim
(`kill`); scratch só em `/tmp` (fora do repo, fora do corpus).

Resíduo no corpus real: `do02a-demo|do02b-demo|zircon-handoff` → 0 (DO-03).

## Veredito DO-02b

BDD (1) 3 camadas no top-3 ✅ · (2) sem-loop reprovado, com-loop passa ✅ ·
(3) spam 3 ≤ 5 ✅ · DoD ✅.
