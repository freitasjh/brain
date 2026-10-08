# Perguntas Clarificadoras por Elemento (v2)

Rota A pergunta **por elemento ausente**, não por tipo de demanda. O bloqueio é o elemento, não o assunto.

Ordenação de impacto: **E1 > E2 > E3 > E4 > E5 > E6**. Se só cabe 1 pergunta, é E1. Se cabem 3, são E1, E2 e o próximo ausente mais alto.

Máximo 2 rodadas. Depois da 2ª, desempate entre 2 interpretações (ver "Perguntas de Desempate").

---

## E1 — Objetivo (crítico)

Ausente quando o verbo é genérico: "melhorar", "arrumar", "resolver", "fazer", "ajustar". O verbo genérico não diz o que muda no mundo.

**Pergunta primária:** O que muda no mundo quando isso estiver pronto?

- Variação A: "Qual é o resultado observável? O que passa a funcionar/acontecer que não acontece hoje?"
- Variação B: "Quem sente a diferença e como?"
- Variação C: "Isso substitui o que existe hoje, ou soma ao que já existe?"

**NÃO perguntar:** o que fazer com o resultado (E5) — sai natural depois que o objetivo está claro e vira pergunta redundante.

---

## E2 — Escopo (crítico)

Ausente quando o pedido nomeia alvo mas não fronteira: "no módulo X" sem dizer se toca só backend, só frontend, ou ambos.

**Pergunta primária:** O que entra e o que NÃO entra?

- Variação A: "Além de [alvo], isso toca mais alguma coisa? (outro módulo, outra tela, dados)"
- Variação B: "Tem parte que você NÃO quer tocar? Nomeia pra eu não ir por lá."
- Variação C: "É só backend, só frontend, ou os dois?"

**NÃO perguntar:** "em qual módulo" — isso é E3, e E3 tem pergunta própria. Não empilhar duas lacunas na mesma pergunta.

---

## E3 — Contexto (alto)

Ausente quando o alvo é vago demais para localizar: "no sistema", "no cadastro", "no login" sem qual cadastro/login, sem versão do schema, sem ambiente.

**Pergunta primária:** Onde exatamente? (módulo, arquivo, tela, endpoint)

- Variação A: "Qual arquivo/classe ou qual rota da tela?"
- Variação B: "Em qual ambiente/versionamento isso acontece? (branch, migration, ambiente)"
- Variação C: "Tem um exemplo de referência? Um ponto do sistema que se parece com o que você quer?"

**NÃO perguntar:** a stack (já está no contexto do projeto) nem o comportamento esperado (E1).

---

## E4 — Restrições (alto)

Ausente quando o pedido toca regra do projeto, contrato existente ou dado compartilhado — e o usuário não disse o que não pode quebrar.

**Pergunta primária:** O que não pode quebrar aqui?

- Variação A: "Isso quebra contrato de API que alguém consome? Qual consumidor?"
- Variação B: "Toca dado já persistido? Precisa de migration, ou só código?"
- Variação C: "Qual regra do projeto esse código precisa respeitar além das já escritas?"

**NÃO perguntar:** a lista de regras do projeto (já está no contexto). Perguntar o que não pode quebrar é mais específico e mais barato de responder.

---

## E5 — Critério de aceite (médio)

Ausente quando o objetivo existe mas "pronto" é indefinido — sem isso o trabalho não tem condição de parada e vira refino infinito.

**Pergunta primária:** Como se sabe que ficou pronto?

- Variação A: "Qual comando/verificação dá 'pronto'? (build, teste, tela, curl)"
- Variação B: "Quem confere e o que essa pessoa precisa ver para aprovar?"
- Variação C: "Tem um caso concreto de entrada/saída que prova que funcionou?"

**NÃO perguntar:** prazo, estimativa, quem faz (E1 já cobre o destinatário do valor).

---

## E6 — Intenção de rota (médio)

Ausente quando o pedido pode ser lido como mais de uma rota: "mudar X" pode ser feature, bugfix ou refactor — e cada rota tem portão e artefato diferentes.

**Pergunta primária:** Isso é feature nova, correção de bug, refactor, ou melhoria?

- Variação A: "Já está quebrado e quer consertar, ou nunca existiu e quer criar?"
- Variação B: "Vou mexer em código que já funciona — isso é bugfix ou refactor?"
- Variação C: "A entrega é comportamento novo ou só forma? (impacta se SDD completo ou fast-track de bugfix)"

**NÃO perguntar:** o escopo da rota (E2). Perguntar a rota só rotula; perguntar o escopo entrega o conteúdo.

---

## Formato de Saída da Rota A

Saída única da Rota A. Não variar o heading. As seções "Já entendi" e "Regra" são obrigatórias — são elas que impedem a IA de ceder no turno seguinte e preencher a lacuna com default do modelo.

Saída:
- `## Falta contexto — preciso de X resposta(s)` — X = número de elementos ausentes que cabe em 3
- `Bloqueio (o que não vou assumir): [elemento ausente, ex: E2 Escopo — e por que não vou assumir]`
- 1 a 3 perguntas numeradas, na ordem E1 > E2 > E3 > E4 > E5 > E6
- `### Já entendi (afirmado por você)` — lista SÓ o que o usuário AFIRMOU
- `### Regra` — "Não vou assumir o que ficou em branco. Escolher por você é palpite caro."

Regras de contagem:
- Máximo 3 perguntas. Se só cabe 1, é E1.
- Inferência da IA NÃO conta como afirmação: anota como `E3 parcial (inferido de "cadastro" — não conta como afirmado)`, nunca como afirmação.
- Rota A não emite bloco PICCO. Bloco PICCO é exclusivo da Rota B.

---

## Perguntas de Conflito

Usar quando o pedido **viola regra do projeto**. Não confundir com E1..E6: aqui não falta informação, existe contradição.

Estrutura fixa da resposta:

1. Nomeia a regra com caminho: "isso viola `<arquivo>:<seção>`".
2. Nomeia o que exatamente no pedido colide: "<trecho do pedido> colide com <regra>".
3. Oferece o caminho permitido mais próximo: "o caminho permitido que atende a mesma intenção é <X>".
4. Pergunta: "Executo o caminho permitido, ou você quer abrir exceção explícita no portão?"

Perguntas por violação recorrente:

| Pedido | Viola | Caminho permitido mais próximo |
|--------|-------|-------------------------------|
| "Usa Lombok pra não escrever os getters" | `backend-coding-standards.md` | "construtor e getters manuais, como nas outras entidades" |
| "Pula o harness, só roda o build" | `harness-continuous.md` H1 | "roda a suite completa; é mais lento e é o portão" |
| "Comenta o teste pra passar" / `@Disabled` | `harness-continuous.md` H2.1 | "corrige a causa do teste falhando" |
| "Mata o processo com pkill -f" | `harness-continuous.md` H5.1 | "lsof -ti :PORT → conferir dono com ps → kill pelo PID" |
| "Corrige esse CSS direto" (no orquestrador) | `orchestrator/AGENT.md` guardrail | "delegar ao developer-engineer" |
| "Pula o code review e avança" | `workflow-rules.md` §6 | "revisão roda antes de declarar fase completa" |

Exceção aberta → registrar em `workflow-state.json.overrides`. Nunca executar a violação em silêncio.

---

## Perguntas de Desempate (2ª rodada da Rota A)

Dispara quando a 2ª rodada não fechou o score. Não é interrogatório: é escolha binária para o usuário decidir.

Estrutura:

```markdown
## Ainda tenho 2 leituras possíveis — me diz qual

**Leitura A:** [interpretação mais provável, com o que muda no resultado]
**Leitura B:** [segunda interpretação, com o que muda no resultado]

Por que pergunto em vez de escolher: as duas produzem código diferente, e a diferença só aparece no fim.
```

Regras:

- Máximo 2 interpretações. "Mais uma opção" só dilui a decisão.
- Cada leitura precisa mostrar o que muda no resultado, senão o usuário não tem base para escolher.
- Se as 2 leituras levarem ao mesmo resultado, o elemento não era bloqueante — declarar o elemento ausente como irrelevante e seguir.
- Nunca "qual das 2 você prefere" sem explicar a consequência.
