---
name: prompt-optimizer
description: "Prompt analysis and refinement (v2). Avalia o pedido contra 6 elementos (PICCO) e decide entre 2 rotas: Rota A (não entendi) e Rota B (re-encoding com bloco PICCO em delimiter tags). Zero chute. Mandatory first step for orchestrator on every CHANGE request (read-only queries exempt)."
---

# Prompt Optimizer v2

Portão de entrada de toda demanda: conta 6 elementos no prompt do usuário e escolhe entre parar e perguntar (Rota A) ou re-encodar e executar (Rota B).

## Regra Fundamental

O modelo **não para na sub-especificação**. Lê "melhorar o sistema", escolhe um default plausível, executa 300 linhas e só descobre no fim que era a coisa errada. O default do modelo **não é** o default do usuário — e o modelo não sinaliza qual dos dois usou.

Por isso: **nunca assumir**. Um default escolhido por você é um palpite não marcado, que o usuário não vê e não pode corrigir. Lacuna em branco é lacuna; preenchê-la é inventar requisito.

## Os 6 Elementos

Score = elementos presentes / 6. **Presente = o usuário AFIRMOU**, não que a IA deduziu. Se você deduziu, o elemento está ausente.

| # | Elemento | Pergunta de checagem | Ausente = |
|---|----------|----------------------|-----------|
| **E1** | Objetivo | O que muda no mundo quando isso estiver pronto? | crítico |
| **E2** | Escopo | O que entra? O que NÃO entra? | crítico |
| **E3** | Contexto | Módulo/domínio/tela/endpoint/feature afetado | alto |
| **E4** | Restrições | O que não pode quebrar? (regra, contrato, compatibilidade) | alto |
| **E5** | Critério de aceite | Como se sabe que ficou pronto? | médio |
| **E6** | Intenção de rota | Feature / Bugfix / Refactor / Melhoria? | médio |

E1 e E2 ausentes = gate fechado. Sem eles, qualquer interpretação é chute sobre o objetivo.

## Fluxo

```
RECEBER prompt
   ↓
brain_search(prompt, top_k=3)
   ↓
score = contar E1..E6 afirmados
   ↓
score = 6/6 ?  NÃO → ROTA A        SIM → ROTA B
```

## Rota A — não entendi (score < 6)

1. **PARAR.** Nenhuma **outra** skill, subagente ou artefato antes disso. Nada é criado.
2. **Máximo 3 perguntas**, ordenadas por impacto no retrabalho: E1 > E2 > E3 > E4 > E5 > E6.
3. **Proibido:** preencher lacuna com default, dizer "vou assumir que...", delegar com contexto parcial, emitir prompt otimizado.
4. **Saída:** bloco de perguntas no [formato canônico da Rota A](references/clarifying-questions.md#formato-de-saida-da-rota-a) — nomeia o elemento bloqueante + o que já foi afirmado.
5. **Repete a partir do score. Máximo 2 rodadas.** Se a 2ª não resolver, apresentar as 2 interpretações mais prováveis e pedir desempate (ver "Perguntas de Desempate" em [clarifying-questions.md](references/clarifying-questions.md)). Interrogatório infinito é falha, não diligence.

Variações de pergunta por elemento: [references/clarifying-questions.md](references/clarifying-questions.md).

## Rota B — entendi (score = 6/6)

1. **Re-encoding:** emitir bloco PICCO estruturado com delimiter tags ([contrato](#contrato-de-saída-delimiter-tags)). O que entra no bloco é o que o usuário AFIRMOU, re-organizado — não interpretado, não enriquecido.
2. **1 pergunta de confirmação** — "entendi X/Y/Z, sigo?". Sempre perguntar = nunca assumir. Não perguntar = nunca executar.
3. **Sobre "sim"** → seguir o workflow SDD carregando o bloco re-encoded como contexto.
4. **Sobre ajuste** → volta pra Rota A só com os elementos corrigidos.
5. **O bloco é efêmero:** vai no prompt de delegação e no chat. Nunca em arquivo.

O mesmo bloco serve aos 3 destinos de consumo:

| Destino | Como entra |
|---------|-----------|
| Auto-uso do orquestrador | contexto do próprio turno, sem colar de volta |
| Prompt de delegação | colado inteiro no prompt do subagente — o subagente não tem a conversa original |
| Colável noutra tool | copy-paste do bloco; delimiters preservam a estrutura para qualquer parser |

## Contrato de Saída (delimiter tags)

```xml
<role>[critério de comportamento + o que conta como defeito — nunca rótulo de persona]</role>

<task>Entregar [E1] em [E3].</task>

<context>
  Projeto: [projeto ativo]
  Escopo: [o que entra e o que NÃO entra]
  Tipo de rota: [E6]
  Fornecido pelo usuário: [lista do que foi afirmado]
</context>

<constraints>
  Sempre: [E4 como positivos — "sempre X", nunca negação pura]
  Nunca: [violações do projeto que esta demanda arrisca]
</constraints>

<acceptance>
  [E5 — como se verifica pronto]
</acceptance>

<open_questions>
  <!-- Rota A: elementos ausentes. Rota B: vazio -->
</open_questions>
```

4 regras do contrato:

1. **≤ 10 constraints.** Acima disso o bloco está mal — cortar. Adesão a instrução cai com o volume.
2. **Role declara critério, não persona.** "Analisa o diff procurando vazamento de camada entre módulos" funciona — diz o que procurar e o que conta como defeito. "Você é um especialista em DDD" é rótulo: o modelo aprende o rótulo e nada mais. O template acima traz placeholder, não exemplo de persona — não copie rótulo para dentro da tag.
3. **Constraints positivas primeiro.** "Sempre X" antes de "nunca Y". Negação pura é mais fácil de ignorar que afirmação.
4. **Sem "pense passo a passo".** CoT explícito em prompt prejudica modelo de raciocínio. Não escrever, não sugerir.

Exemplos completos (bons e ruins) + contagem de constraints: [references/picco-template.md](references/picco-template.md).

## Rota de Conflito

Dispara quando o pedido viola regra do projeto — Lombok, `@Disabled`, pular harness, `pkill -f`, editar código no orquestrador, pular code review. Não é ambiguidade, é conflito: não adianta perguntar para resolver.

**Precedência:** a rota de conflito vence. Não é ambiguidade, é contradição — perguntar por elemento faltando não resolve violação de regra. Se o pedido for violador E incompleto: trate o conflito primeiro, retome a Rota A só para o que falta.

1. **Nomeia a regra** com caminho: "isso viola `<arquivo>:<seção>`".
2. **Oferece o caminho permitido mais próximo** que atende à mesma intenção.
3. **Pergunta:** executo o caminho permitido, ou você quer abrir exceção explícita no portão?
4. **Exceção →** registrada em `workflow-state.json.overrides`. Override não apaga a regra, deixa rastro.

Perguntas prontas: [references/clarifying-questions.md](references/clarifying-questions.md#perguntas-de-conflito).

## Exceção: Comandos de Consulta

> **Pulam o gate.** Sem exceção, o portão vira taxação no dia a dia.

- `show status`, `o que bloqueia X?`, `onde paramos?`, `onde está X?`, `o que Y faz?`, `existe Z?` (consulta factual ao repo — leitura, não mudança), leitura de `workflow-state.json` → **pula** brain_search, score, re-encoding e confirmação.
- "add feature X", "move X before Y", "skip X" → **não pulam**: alteram estado, contam como demanda de mudança.

Só demandas de **mudança** passam pelo gate. Consulta é leitura — responder já é a resposta.

Override de 1 palavra ("já vai") também vale: o usuário respondendo curto é o sinal, não o número de elementos.

## Anti-Patterns

| ❌ Errado | ✅ Certo | Por quê |
|-----------|-----------|---------|
| Persona genérica sem critério: "Você é um especialista em DDD. Analise o sistema." | Critério no role: "Analisa o diff procurando vazamento de camada" | Rótulo não muda comportamento; critério muda |
| Negação pura: "não use Lombok" como única constraint | Positivo primeiro: "sempre construtor explícito e getters manuais" | Negação é mais fácil de ignorar que afirmação |
| Mega-prompt com 14+ constraints | Máximo 10; acima disso o bloco está mal | Acima de ~10 a adesão cai |
| "pense passo a passo" / CoT explícito | Nenhuma instrução de raciocínio | CoT escrito prejudica modelo de raciocínio |
| Prompt que se repete em 3 seções | Um bloco, 3 destinos de consumo | Repetição consome contexto sem adicionar |
| Regra órfã: skill diverge do call site | Call site cita a skill e seu template | Call site desatualizado é o defeito; a skill é a fonte da verdade. Sincronizar o call site, nunca o contrário. |
| "Vou assumir que você quer X" | "Você quer X ou Y?" | Default escolhido por você é palpite não marcado |
| 3 rodadas de perguntas | Máximo 2 rodadas, depois desempate entre 2 interpretações | Interrogatório infinito é falha |

## Brain

**Antes** de|scorear:

```python
brain_search(query=<prompt do usuário, literal>, top_k=3)
```

A query é o prompt do usuário, não uma paráfrase — a paráfrase já é uma interpretação.

**Depois**, quando um padrão se repetir:

```python
# Critério do 2x: 1 ocorrência é coincidência, 2 é padrão. Não poluir o Brain com evento único.
brain_store(
    layer="regras",
    path="projeto/prompt-patterns/<elemento>-<padrao>",
    content="## <título>\n\n### Lacuna\n[elemento ausente]\n### Pergunta que resolveu\n...\n### Resposta\n...",
    scope="projetos"
)
```

## Verificação

| # | Verificação | Critério |
|---|-------------|----------|
| **V1** | Prompt vago real ("melhorar o sistema") | Rota A, ≤3 perguntas, ZERO artefato criado |
| **V2** | Prompt claro real | Rota B, bloco PICCO válido, 1 confirmação, depois avança |
| **V3** | `grep -c "Tier 1|Tier 2|Tier 3"` nos 4 call sites (orchestrator/AGENT.md, sdd-orquestrador/AGENT.md, workflow-rules.md, harness-continuous.md) | 0 |
| **V4** | `grep -c "prompt-optimizer" .agents/rules/harness-continuous.md` | >= 1 |
| **V5** | Constraints por exemplo no picco-template.md | ≤ 10 em todos |
| **V6** | `workflow-state.json` | JSON válido, `type: refactor` |
