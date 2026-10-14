# EXPLORATION — Brain Especialista (skill + hooks + instalador dual-IDE)

> Fase 0 dev-planner (Descubra). Status: rascunho aprovado; marcas de assunção
> do rascunho original (defaults autorizados em 2026-10-14, "Assume e segue")
> resolvidas todas em DO-03 (ver DO-03-HANDOFF.md §1).

## 1. Problema real

O brain armazena memória (SQLite FTS5+vetor) mas **não é especialista ativo**:
- Skill `brain` descreve tools, não ensina protocolo de deepsearch (busca larga →
  afunila por layer/scope/project → `read` cheios → síntese). Agentes fazem 1
  `search` rasa ou nenhuma.
- Hooks capturam bytes brutos (spool), não contexto curado. Handoff de sessão é
  resumo fino em `sessoes`; `estudos` completos e `regras/arquitetura` duráveis
  dependem da disciplina do agente.
- Instalação por IDE é parcial: `setup opencode|kiro` gera só captura
  `command-type`. Os 2 agent-hooks + steering que funcionaram no projeto boleto
  são arquivos avulsos, fora do gerador.
- Harness carrega cópia vendored e divergente da skill brain (4 tools,
  texto Obsidian) + referência quebrada (`brain-context-protocol.md` inexistente).
  Handoff `sessoes/brain/2026-10-14-handoff-harness-brain-separation` já delega o
  lado harness; **este projeto cuida só do lado brain**.

Consequência: cada sessão redescobre contexto, aprendizado evapora, desktop
(opencode/kiro) não recebe o padrão que funciona.

## 2. Usuários

| # | Usuário | Uso |
|---|---------|-----|
| U1 | Dev desktop opencode | Instala via `brain setup opencode`; espera contexto injetado + handoff automático |
| U2 | Dev desktop kiro | Instala via `brain setup kiro`; espera 3 hooks + steering no `.kiro/` do projeto |
| U3 | Orquestrador SDD | Espera agentes com protocolo brain-first real (search antes, store depois) |
| U4 | Agente harness | Consome handoff; não deve receber regra brain vendored (fora de escopo aqui) |

## 3. Casos de uso

- **CU1 — Deepsearch especialista:** agente recebe feature, executa roteiro fixo
  (varredura larga top_k alto → filtros layer/scope/project → leitura integral
  dos hits → síntese com paths citados) antes de codar. (Confirmado SK-01:
  roteiro mora na skill, I1 verde.)
- **CU2 — Contexto no prompt:** ao `UserPromptSubmit`/`session-start`, hook injeta
  contexto relevante (últimas sessões do projeto + regras globais + arquitetura do
  domínio citado). Via agent-hook (que manda ler a skill — `disclose_context`
  não existe no Kiro, ver DO-03-HANDOFF.md §2) + command-hook que imprime
  `brain search` resumido; falha de MCP nunca bloqueia.
- **CU3 — Handoff rico:** ao `Stop`/fim de sessão, hook lembra + template 3 camadas
  (`sessoes` resumo, `estudos` completo com frontmatter+loop qualidade,
  `regras/arquitetura` durável), com `skip se trivial`.
- **CU4 — Instalação dual-IDE:** `brain setup opencode|kiro` instala matriz completa
  por IDE + `doctor` valida (hooks presentes, `brain ping` ok como warn).

## 4. Escopo (entra / não entra)

Entra: skill brain especialista (roteiro deepsearch + taxonomia + template),
3 hooks por IDE (contexto, aprendizado, prompt-optimizer), steering kiro,
`setup` completo + `doctor`, docs (`SKILL.md`, `BRAIN.MCP.md`, `hooks/README.md`).
Não entra: qualquer arquivo no repo harness (handoff separado já entregue);
enforcement bloqueante (harness); mudança de schema/busca RRF; Fase C Python.

## 5. Alternativas consideradas

| Alt | Prós | Contras | Veredito |
|-----|------|---------|----------|
| A. Só docs, sem mudar `setup` | barato | ninguém instala manualmente; drift continua | rejeitada |
| B. Tudo via `command-type` (spool) | determinístico, sem custo LLM | provado no boleto que não gera aprendizado curado | rejeitada |
| C. Escolhida: skill especialista + hooks agent+command + setup dual-IDE | padrão validado no boleto, por IDE | custo: agent-hooks consomem loop LLM; `Stop` por turno exige guard anti-spam | **adotada (confirmada Blocos 1–4)** |

## 6. Restrições (confirmadas em CONSTRAINTS.md)

- Prod no ar (8321/8322, `data/brain.db`) intacto; E2E só em scratch + portas efêmeras.
- Sem DDL fora de bump `SCHEMA_VERSION`; RRF k=60 e dim 768 inalterados.
- Falha de MCP/Ollama degrada (warn), nunca bloqueia hook.
- Agent-hook `Stop` dispara por turno no Kiro → guard "skip se trivial" obrigatório.
- Prompts generalizados (projeto via git/config, nunca hardcoded).

## 7. Fontes

- Kiro docs (2026-10-10): `.kiro/hooks/*.json` schema v1, triggers PascalCase
  (`SessionStart/Stop/UserPromptSubmit/PreToolUse/PostToolUse/PreTaskExec/...`),
  `command` (stdin JSON, exit 2 bloqueia) vs `agent` (injeta prompt, não bloqueia),
  bloco `confirm` em `Stop`, hooks globais `~/.kiro/hooks/`. Fontes:
  `kiro.dev/docs/hooks/`, `/hooks/actions/`, `/hooks/types/`, `/ide/whats-new-v1/hooks/`.
- OpenCode docs: plugin = módulo JS/TS com hooks `tool.execute.before/after`,
  eventos `session.*`, `experimental.session.compacting` p/ injetar contexto na
  compactação. Fonte: `opencode.ai/docs/plugins/`.
- Lições internas: `regras/global/kiro-hooks-agent-type-aprendizado`,
  `regras/projetos/boleto/aprendizado-via-mcp`,
  `arquitetura/projetos/boleto/port-harness-opencode-para-kiro`.

## 8. Riscos

- R1: trigger exato de delegação `task` no Kiro (`run_workflow` vs `PreToolUse`) não
  verificável sem teste empírico → mitigar com matcher amplo + log.
- R2: spam de notas por `Stop` por turno → guard + dedup por conteúdo.
- R3: `setup` sobrescreve hooks custom do usuário → merge aditivo + backup.

## 9. Próxima fase

Fase 1 (Analise) → `CONSTRAINTS.md`: restrições técnicas por IDE, dependências
(crate `brain-cli setup.rs`, `hooks/`, skills), compat legado Fase C, compliance
(allowlist `BRAIN_EXPORT_ROOT`, redação de credencial).
