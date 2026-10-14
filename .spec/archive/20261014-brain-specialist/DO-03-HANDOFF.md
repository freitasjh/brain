# DO-03 — zero marcadores de assunção + handoff Fase 3

> I7 verificado 2026-10-14: `grep -rn ASSUMIDO` (case-sensitive, fora de
> `target/.git/data/.venv/node_modules`) → **2 hits totais: auto-menção deste
> handoff (:3, string do comando) + 1 legado em `.spec/brain-help-visual/VERIFY.md:96` com decisão explícita (#13
> abaixo, fora de escopo, original mantido); 0 no escopo DO-03**.

## 1. Contabilidade por ocorrência (15 no tree + 8 no corpus)

14/15 ocorrências em arquivo reescritas em texto explícito (decisão registrada
no próprio arquivo); 1/15 exceção legada (#13 abaixo, fora de escopo, decisão
explícita do usuário) — sem incerteza aberta.

| # | Arquivo:linha (pré-DO-03) | Resolução |
|---|---------------------------|-----------|
| 1 | `EXPLORATION.md:4` (legenda do token) | Nota histórica: rascunho 2026-10-14, tudo resolvido aqui |
| 2 | `EXPLORATION.md:40` CU1 roteiro na skill | Confirmado SK-01 (I1 verde) |
| 3 | `EXPLORATION.md:44` CU2 `disclose_context`-like | Ponte reescrita — ver §2 |
| 4 | `EXPLORATION.md:66` Alt C adotada | Confirmada Blocos 1–4 |
| 5 | `EXPLORATION.md:68` restrições | Confirmadas em `CONSTRAINTS.md` |
| 6 | `SPEC.md:66` I7 | Reescrito sem o token (invariante preservada) |
| 7–9 | `TASKS.md:77,187,189` (DoD/título/BDD) | Reescritos sem o token (sentido preservado) |
| 10–11 | `PLAN.md:9,83` | Marcados como resolvidos DO-03 |
| 12 | `PROPOSAL.md:98` | Reescrito sem o token |
| 13 | `brain-help-visual/VERIFY.md:96` | Revertido — fora de escopo DO-03: original mantido; decisão explícita do usuário (opcao A) em 2026-09-28, não reescrita |
| 14–15 | `workflow-state.json:16,222` | `validation` + `reviewer_notes` reescritos |

Corpus (`data/brain.db`, untracked): 8 notas de sessão contêm o token
(`sessoes/brain/2026-10-14-*`, `regras/projetos/brain/su01b-*`,
`arquitetura/projetos/atlas-cobranca/fase4-*`). **Decisão explícita:
histórico de sessão não se reescreve** — reescrever trilha de auditoria p/
higienizar palavra é falsificação; o token no tree (fonte) está em 0.

## 2. `disclose_context` resolvido (debt Bloco 1+2 → DO-03)

Veredito: **mecanismo inexistente** — action `agent` do Kiro carrega só
`prompt` livre (EXPLORATION §7: `command` vs `agent`); não há parâmetro/ação
`disclose_context` no schema de hooks Kiro nem na API de plugin opencode.
Nenhuma doc oficial cita; nenhum teste deste repo o ancora (grep
`disclose_context` em `crates/` = 0 antes e depois).

Ação: ponte reescrita em `hooks/kiro/brain-prompt-optimizer.json` —
"leia a skill prompt-optimizer (`.agents/skills/prompt-optimizer/SKILL.md`,
existe) e aplique a rota indicada". Sanity 3/3 pós-mudança; instalado
byte-fiel (evidência DO-02a §1). Cópia operador-local `.kiro/hooks/` deste
checkout mantém o texto antigo — é snapshot de install, atualiza com
`setup kiro --force`; fonte manda.

## 3. Handoff p/ Fase 3 (delegação pronta)

| Item | Estado | Onde |
|------|--------|------|
| Feature `brain-specialist` | Blocos 1–5 entregues, último bloco este | `.spec/brain-specialist/` + este arquivo |
| Tarefas | 20/20 TASKS convertidas; DO-01→DO-02a→DO-02b→DO-03 verdes | `TASKS.md` §4, `DO-02a/b-EVIDENCE.md` |
| Contexto p/ continuar | SPEC §8 RF6 + §9–§11, ADR-2 (pontes sem drift), OC-02-DECISION (GO-condicional fechado na demo) | mesmos paths |
| Portão | H6 pendente de rodada final: `cargo test --workspace` + clippy + pytest + review (bloco 5 mexeu só docs/estáticos, mas o portão é cego a isso) | — |
| Produção | Intocada o bloco inteiro (lsof igual nas 3 medições; PIDs 1380/1381/10906/456874) | evidências |
| Aceite Fase 3 | `VERIFY.md` contra SPEC §11 + archive SDD (Fase 5 do workflow) | a gerar |

Prompt de delegação (Fase 3 / verify+archive): "Feche `brain-specialist`:
rode H6 completo (`cargo test --workspace`, `clippy --all-targets -D
warnings`, pytest), gere `.spec/brain-specialist/VERIFY.md` (3 dimensões),
`brain_store` das lições com scope correto, archive p/
`.spec/archive/<data>-brain-specialist/`. Fontes: SPEC §8–§11, evidências
DO-02a/DO-02b, este handoff. Produção 8321/8322 intocada; demos só em
scratch. Critério: zero 🔴 review + H6 verde."

## 4. Observações fora de escopo (anotadas, intocadas)

1. `.kiro/hooks/` deste checkout ≠ `hooks/kiro/` (nomes/prompts antigos, exe
   absoluto): snapshot de install operador-local, não drift de fonte. (P3)
2. `store <CONTENT>` começando com `---` exige `--` antes (clap lê como flag);
   demo DO-02b mordeu e documentou no roteiro. Candidato a exemplo no
   `--help` do `store`. (P3 cosmético)
3. Spool `/run/user/1000/brain/hook-spool.jsonl` carrega 2 linhas `do02a-*`
   do demo: spool não tem leitor (Z-01), não é corpus; sem ação.
4. `sanity.sh` A2 usa `grep -w` com alternação (limitação conhecida Bloco
   1+2): continua verde; sem ação neste bloco.
