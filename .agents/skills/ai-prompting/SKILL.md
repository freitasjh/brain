---
name: ai-prompting
description: >
  A disciplina de consultar o brain ANTES de codar e registrar DEPOIS de decidir.
  Escreva para qualquer agente com o Brain MCP conectado: quando buscar, como ler um
  resultado híbrido, e o que fazer quando a busca volta vazia. Pare de carregar esta
  skill se você só quer a lista de tools — essa é a skill `brain`.
license: MIT
compatibility: opencode
metadata:
  category: methodology
  complexity: beginner
  stack: brain-mcp
---

# Consultar o brain é parte de escrever o código

## A tese

Um brain que ninguém consulta é um banco de dados parado. Não é um acúmulo passivo
de boas intenções: é o único mecanismo de memória que sobrevive ao fim de uma
sessão, e ele só funciona se **você o usar nas duas pontas** — ler antes de decidir,
gravar depois de decidir.

O modo de falha não é o brain ausente. É o brain presente e ignorado, que produz a
sensação de contexto sem o contexto, e é indistinguível de um agente competente
working from scratch. Ele só é visível *depois*, quando você refaz um trabalho que
outro agente já tinha feito, ou quando um colega descobre que a decisão que você
tomou sem busca já estava registrada como erro.

Esta skill é o **par** da skill `brain`. Aquela documenta as tools — assinatura,
parâmetros, formato. Esta documenta **quando** e **por que** consultar, e o que fazer
quando a consulta falha. As tools não são repetidas aqui.

## Quando consultar — o gatilho

Consulte o brain **antes** de escrever a primeira linha de código que depende de uma
convenção. Convenção é tudo aquilo que poderia ter sido decidido de outro jeito sem
quebrar nada: nome de campo, formato de log, camada onde uma função mora, o que se
faz quando a dependência está fora.

Os três momentos que importam:

1. **Antes de implementar algo que parece óbvio.** Se a resposta é óbvia, é porque
   você ainda não viu a regra que a torna óbvia. Essa é a hora mais barata de
   descobrir que o nome do campo já estava padronizado.
2. **Quando algo te surpreende.** Código que faz algo estranho quase sempre tem um
   comentário explicando por quê, e o comentário raramente está no código — está no
   brain. Surpresa é o sinal mais forte de que uma busca foi adiada demais.
3. **Antes de mudar uma convenção.** Se você vai renomear, mover ou "simplificar"
   alguma coisa, pergunte antes. A maioria dessas mudanças já foi tentada.

## Quando não consultar

- Para verificar a assinatura de uma função que você já tem aberta.
- Para conferir aritmética ou o estado de um processo em execução — use a
  ferramenta própria, não a memória.
- Para procurar algo que nunca foi escrito. A busca é por substância acumulada; ela
  não adivinha o que você ainda não decidiu.

## Como ler o resultado

A busca é **híbrida**: texto (FTS5) e vetor (embeddings) fundidos por RRF, mais dois
streams de entidade e de grafo. Peça `explain` e **leia os campos** — o score sozinho
não diz por que um resultado está no topo.

O que o `explain` te diz:

| Campo | Leitura |
|---|---|
| `rrf_fts` > 0 | casou por **palavra**. O termo existe literalmente no texto. |
| `rrf_vec` > 0 | casou por **sentido**. Não há palavra em comum; o significado é o mesmo. |
| `rrf_entity` > 0 | casou por **entidade** extraída. |
| `rrf_graph` > 0 | casou por **ligação** com outro documento. |
| `authority` | bônus de camada: `arquitetura` e `regras` pesam mais que `estudos`. |

O padrão que vale atenção: **`rrf_fts` sozinho, repetido em vários resultados**,
tipicamente significa que a busca degenerou em busca de substring. Os dois streams
têm pesos comparáveis (`1/(60+rank)` cada), então um bom resultado sem sobreposição
literal de palavras é o sinal de que a parte semântica está viva.

E há um **fallback** que você precisa conhecer: se o serviço de embeddings estiver
fora, a busca **não falha** — ela devolve só o stream textual, e o resultado parece
plausível mas perdeu metade dos sinais. Se os resultados vierem todos com `rrf_vec`
em zero, o que você tem na mão é uma busca de texto, e vale saber disso antes de
concluir que a memória não tinha o que você procurava.

## Quando a busca volta vazia

Esse é o caso que não está documentado em lugar nenhum, e é o que mais destrói o
hábito: vazio parece "não há nada", e a conclusão que se tira é "siga em frente".
São três coisas diferentes, e elas pedem ações opostas.

**Não conclua "não existe" a partir de um resultado vazio.** Antes de tratar o vazio
como ausência de conhecimento, percorra esta escada na ordem:

1. **A busca está no nível certo?** Filtrar por `layer` ou `scope` errado é a causa
   mais comum de vazio, e é silenciosa: um filtro restritivo demais devolve zero
   sem erro. Tente sem filtro, depois adicione.
2. **Você procura as palavras que alguém usaria?** A parte de texto casa por
   **termo**, depois de *stemming*. Um sinônimo técnico não casa; a parte de vetor
   pega. Se os resultados vieram só do texto, sua query pode estar precisa demais.
3. **O corpus está indexado?** Um `brain_status` resolve. O sinal que importa é
   **cobertura de embedding**: chunks sem vetor contam como "na fila", não como
   "indexados". Cobertura baixa com fila ativa é normal e passageiro. Cobertura baixa
   com fila parada é o real motivo do seu vazio — e a causa está em
   `queue.pending_len` e `queue.dead_lettered`.
4. **O serviço de embeddings está no ar?** Se não estiver, você está vendo metade da
   busca sem nenhum sinal de que isso aconteceu. Verifique em `brain_status`.
5. **Só agora:** o conhecimento realmente não está no brain.

Só depois do passo 5 "não existe" é uma conclusão legítima — e ainda assim é uma
conclusão sobre o **corpus**, não sobre o mundo. Daí a lição abaixo.

### O que fazer em cada ramo

| Ramo | Ação |
|---|---|
| Filtro restritivo | repita sem filtro |
| QueryLiteral demais | reformule com sinônimo, ou peça mais `top_k` |
| Cobertura baixa, fila ativa | aguarde; a fila drena sozinha |
| Cobertura baixa, fila parada | o problema é infraestrutura, não a sua query |
| Embeddings fora | use o resultado parcial, sabendo que é parcial |
| Corpus realmente vazio | **então escreva** — ver abaixo |

## Gravar depois de decidir

Uma busca sem gravação devolveu o valor que já estava no brain. O ganho do ciclo
fecha quando a decisão volta.

Grave quando uma destas coisas acontece, e não antes:

- **Você decidiu** algo que um próximo agente teria de adivinhar de novo.
- **Você descobriu** que a forma óbvia estava errada, e sabe por quê. O *porquê* é a
  parte cara; o *o quê* qualquer um descobre lendo.
- **Você perdeu tempo** com algo que uma nota teria evitado. Esse é o momento mais
  honesto de gravar, porque é o custo real, e é o que a torna útil para o próximo.

Duas regras que evitam o vazamento que mais custa caro:

- **Escolha o escopo pelo alcance da regra, não pelo assunto.** Uma regra que vale
  para qualquer projeto vai para o escopo global. Uma regra sobre a tabela de um
  sistema só é daquele sistema. Colocar uma regra local no global contamina todos os
  projetos que consultarem depois.
- **Grave o conteúdo, não o sumário.** Uma nota que diz "vi sobre Flyway" não
  consulta; uma nota que explica a decisão, o motivo e a alternativa rejeitada é o
  que a busca encontra daqui a três meses.

## O que esta disciplina não faz

- Não substitui ler o código. O brain guarda **por que**, não **o que é** — o código
  muda e o comentário dele junto. Decisão que envelhece é dívida, e o brain não
  avisa que uma nota envelheceu.
- Não garante que a nota existe. Um registro ausente e uma memória de ninguém têm a
  mesma aparência de fora.
- Não é backup. Ele não sobrevive à perda do banco sem um export explícito, e o
  export tem escopo restrito por desenho.

## Checklist

Antes de escrever código que depende de uma convenção:
- [ ] consultei o brain sobre a convenção específica, sem filtro
- [ ] li o `explain` do resultado, não só o score
- [ ] se voltou vazio, percorri a escada de 5 passos antes de concluir que não existe
- [ ] se a cobertura de embedding estiver baixa, sei se a fila está ativa ou parada

Depois de decidir algo que levaria outro agente a adivinhar:
- [ ] gravei o conteúdo e o motivo, não o sumário
- [ ] escolhi o escopo pelo alcance da regra
