# Template PICCO — Contrato de Saída + Exemplos

Referência de `SKILL.md`. O bloco é efêmero: vai no chat e no prompt de delegação, nunca em arquivo (D1).

## O Template

```xml
<role>[critério de comportamento, não persona]</role>

<task>[E1] em [E3].</task>

<context>
  Projeto: [projeto ativo]
  Escopo: [o que entra e o que NÃO entra]
  Tipo de rota: [E6]
  Fornecido pelo usuário: [lista do que foi afirmado, sem dedução]
</context>

<constraints>
  Sempre: [positivos]
  Nunca: [violações que esta demanda arrisca]
</constraints>

<acceptance>
  [E5]
</acceptance>

<open_questions>
  <!-- Rota A: elementos ausentes. Rota B: vazio -->
</open_questions>
```

| Tag | Preenche com | Elemento |
|-----|-------------|----------|
| `role` | Critério de comportamento | — |
| `task` | Objetivo no domínio | E1 + E3 |
| `context` | Projeto, escopo, rota, o que foi afirmado | E2 + E3 + E6 |
| `constraints` | ≤10 linhas, positivas primeiro | E4 |
| `acceptance` | Verificação de pronto | E5 |
| `open_questions` | Vazio na Rota B | lacunas |

**Escopo entra em `context`**, não em `task`: o `task` é o que muda no mundo, o `context` é onde isso está delimitado.

---

## BOM 1 — Rota B, score 6/6, bloco completo, 1 confirmação

**Prompt:** "Adicionar validação de email no endpoint POST /internal/registrations do módulo company. Rejeitar email inválido com 422. Não pode quebrar o cliente mobile que hoje recebe 400. Bugfix — o cadastro aceita email sem arroba."

**Contagem:** E1 validar email, E2 só POST /internal/registrations, E3 `company` + endpoint nomeado, E4 cliente mobile não pode quebrar, E5 422 para inválido, E6 bugfix. **6/6 → Rota B.**

```xml
<role>Revisa contratos de API procurando mudança de status code e de payload que quebra consumidor</role>

<task>Rejeitar email inválido em POST /internal/registrations com 422</task>

<context>
  Projeto: Atlas ECM
  Escopo: só POST /internal/registrations do módulo company — não toca outros endpoints
  Tipo de rota: Bugfix
  Fornecido pelo usuário: validação de email; módulo company; cliente mobile hoje recebe 400 e não pode quebrar; status de saída 422; bug é o cadastro aceitar email sem arroba
</context>

<constraints>
  Sempre: response 422 no corpo da resposta padronizado do projeto
  Sempre: manter o campo de email no payload de request
  Nunca: alterar o shape da resposta de sucesso
  Nunca: tocar em outros endpoints do módulo company
</constraints>

<acceptance>
  mvn test passa; IT de POST /internal/registrations cobre 422 com email inválido e 200/201 com email válido; contrato de sucesso inalterado
</acceptance>

<open_questions>
</open_questions>
```

**Confirmação (1, só):** "Entendi: 422 só no POST /internal/registrations do `company`, contrato de sucesso e payload intactos, cliente mobile não afetado. Bora?"

**Por que funciona:** 6/6 sem dedução. 4 constraints, todas positivas primeiro, e as duas negativas nomeiam violações concretas em vez de proibir em abstrato. `<role>` traz critério ("procurando mudança de status code e de payload que quebra consumidor"), não persona. `<acceptance>` tem comando, não adjetivo.

---

## BOM 2 — Rota A, 2 bloqueios, perguntas ordenadas por impacto

**Prompt:** "Melhorar o cadastro."

**Contagem:** E1 ❌ ("melhorar" não diz o que muda no mundo), E2 ❌ (sem fronteira), E3 ❌ ("cadastro" = 2 telas no sistema), E4 ❌, E5 ❌, E6 ❌. **0/6 → Rota A.** Como E1 e E2 são críticos e E3 é alto, as 2 primeiras perguntas são E1 e E2 — E3 vem na 2ª rodada se precisar.

```markdown
## Falta contexto — preciso de 2 resposta(s)

Bloqueio (o que não vou assumir): E1 Objetivo e E2 Escopo. "Melhorar" não diz o que muda no mundo, e sem fronteira eu escolho o que tocar — escolher por você é palpite caro.

1. **O que muda no mundo quando isso estiver pronto?** O que passa a funcionar ou acontecer que hoje não acontece?
2. **O que entra e o que NÃO entra?** Além do cadastro, isso toca backend, frontend, dados? Tem parte que você não quer que eu toque?

### Já entendi (afirmado por você)
- E3 parcial: tela de cadastro (falta saber qual: usuário, empresa ou documento)

### Regra
Não vou assumir o que ficou em branco. Escolher por você é palpite caro.
```

**Por que funciona:** 2 perguntas, não 3 — a contagem segue o que está bloqueado, não um número fixo. A ordem respeita E1 > E2 > E3, então as duas primeiras são as de maior retrabalho. Nomeia o elemento bloqueante e justifica por que não vai assumir, que é o que a v1 não fazia e é o que segura a IA no turno seguinte. Zero artefato criado.

---

## RUIM 1 — persona sem critério

```xml
<role>Você é um especialista em DDD. Analise o sistema.</role>

<task>Melhore a arquitetura.
<context>
  Projeto: Atlas ECM
</context>
```

**Contagem:** E1 ❌, E2 ❌, E4 ❌, E5 ❌. E3 parcial ("sistema" não localiza), E6 ❌. **1/6 — mas passou como se fosse Rota B.** Faltam as tags, o `task` não é re-encoding do pedido, e ninguém contou o score.

**Por que falha:** o `<role>` é rótulo. "Especialista em DDD" não diz o que olhar nem o que conta como defeito — o modelo sabe que é especialista e nada mais. Trocar por critério muda o comportamento: "Analisa o diff procurando vazamento de camada entre módulos" diz exatamente o que procurar. Além disso, score 1/6 passou: sem o gate explícito, o bloco bonito mascara a lacuna.

---

## RUIM 2 — 14 constraints + CoT explícito

```xml
<role>Você é um especialista em segurança. Pense passo a passo antes de responder.</role>

<task>Corrigir a validação de email.</task>

<constraints>
  1. Pense passo a passo
  2. Não use Lombok
  3. Não altere o banco
  4. Não quebre o cliente mobile
  5. Não adicione dependências
  6. Não escreva teste
  7. Não altere o status code
  8. Não crie endpoint novo
  9. Não toque em outros módulos
  10. Não use System.out
  11. Não faça commit
  12. Não pule code review
  13. Não use subagente
  14. Não rode pkill
</constraints>
```

**Contagem: 14 constraints.** V5 reprova (>10). 13 são negações, 0 positivas.

**Por que falha:** dois defeitos independentes.

**Volume:** 14 regras simultâneas. Acima de ~10 a adesão cai — as últimas entram como ruído e o modelo cumpre as primeiras. O bloco está mal, não a instrução.

**Negação pura:** "não use X" pede para a ausência de X. Quem tem dúvida sobre o default cai no X. "Sempre construtor e getters manuais" é mais forte que "não use Lombok".

**CoT explícito:** "pense passo a passo" escrito no prompt prejudica modelo de raciocínio — ele já raciocina, e forçar o formato no texto degrada a qualidade. E conta como constraint nº 1, gastando um dos slots com uma instrução que não é sobre o domínio.

`<role>` também é rótulo ("especialista em segurança"), não critério.

---

## Checklist do bloco antes de emitir

- [ ] Score 6/6 declarado antes de emitir (Rota B) ou o bloco é de perguntas (Rota A)
- [ ] `<role>` traz critério de comportamento, não persona
- [ ] ≤ 10 constraints
- [ ] Positivas antes das negativas
- [ ] Zero instrução de raciocínio
- [ ] `<acceptance>` tem comando/verificação, não adjetivo
- [ ] `<context>` lista só o que o usuário AFIRMOU
- [ ] Rota B: `<open_questions>` vazio
- [ ] Bloco não foi escrito em arquivo
