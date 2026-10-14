# DESIGN-DECISIONS — Brain Especialista

> Fase 2 dev-planner (Desafie). Cada ADR: contexto → opções com prós/contras →
> decisão + consequência. Websearch da fase: Kiro confirm-action e OpenCode
> hooks (ver EXPLORATION §7); pitfalls de enforcement em
> `regras/global/opencode-plugin-hooks-enforcement-reality`.

## ADR-1: Quem entrega os agent-hooks — gerador Rust ou arquivos estáticos?

- **Contexto:** `setup kiro` gera só `brain-session.json` (command-type).
  Precisamos de +2 agent-hooks (`brain-learning`, `brain-prompt-optimizer`) +
  1 steering (`.kiro/steering/brain-learning.md`).
- **Opção A — gerar tudo em `setup.rs`:** one-shot completo. Contras: prompts
  instrucionais congelados em Rust (rebuild p/ reword), drift gerador↔skill,
  testes viram assert de string longa.
- **Opção B — command no gerador + agent/steering como arquivos estáticos
  versionados copiados:** prompts evoluem sem rebuild; a lição
  `regras/global/kiro-hooks-agent-type-aprendizado` já prescreve exatamente isso
  ("arquivos estáticos versionados, fora do gerador"). Contras: cópia exige
  versionamento junto + `doctor` que detecta divergência.
- **Decisão: B.** Consequência: novo diretório-fonte (ex.: `hooks/kiro/`,
  `hooks/opencode/`, `steering/`) + `setup --force/--dry-run/.bak` + teste de
  sanidade (placeholders, sem nome de projeto hardcoded, trigger na lista fechada).

## ADR-2: Onde mora o roteiro deepsearch?

- **Opção A — só na skill `brain`:** fonte única, versionada aqui, lida por
  qualquer IDE via `instructions`. Contras: agente precisa ser lembrado de ler.
- **Opção B — duplicar em skill + steering + prompt de hook:** visível em todo
  lugar. Contras: regra órfã (anti-pattern do prompt-optimizer: skill diverge do
  call site); 3 cópias para manter.
- **Decisão: A com pontes finas.** Roteiro completo só em
  `.agents/skills/brain/`; agent-hooks e steering contêm 3–5 linhas apontando
  para a skill, nunca o roteiro. Consequência: teste/grep de divergência
  (roteiro aparece 1x).

## ADR-3: Anti-spam do `Stop` por turno (Kiro)

- **Contexto:** `Stop` dispara por turno, não só fim de sessão; gravar tudo =
  spam de notas finas (o `brain-hook.sh:59` já mostra o risco: 1 nota por tool).
- **Opções:** (a) nada; (b) guard instrucional "skip se trivial" no prompt do
  agent-hook; (c) dedup por hash de conteúdo + throttle temporal no `brain hook`.
- **Decisão: b + c leve.** Guard no prompt (custo zero, provado no boleto) +
  dedup de conteúdo já existente no spool; sem throttle com estado novo (risco
  de perder sessão real). Aprendizado curado só via MCP pelo agente, nunca CLI.
- **Pitfall questionado:** "hook command grava aprendizado" — rejeitado pela
  evidência do boleto (spool ≠ curadoria).

## ADR-4: Contexto no prompt — command, agent ou os dois?

- **Command-hook** (ex.: `session-start`/`UserPromptSubmit` imprimindo resumo de
  `brain search`): determinístico, sem custo LLM, falha = warn. Limite: saída
  grande polui contexto; exige truncamento + top_k pequeno.
- **Agent-hook** (instrução "busque contexto via MCP antes de agir"): flexível,
  cita paths exatos; custa 1 loop LLM; dispensável em consulta somente-leitura.
- **Decisão: os dois, com papéis distintos.** Command = resumo curto best-effort;
  agent = instrução condicional (pula se somente-leitura). Falha de MCP/Ollama
  nunca bloqueia (fail-open, coerente com C4).

## ADR-5: Handoff rico — template e validação sem enforcement

- **Template 3 camadas** (decisão herdada da skill `brain`): `sessoes/` resumo +
  `estudos/` completo (frontmatter + loop qualidade: código, conceitos, refs,
  profundidade, casos reais, >500 chars) + `regras/arquitetura` durável.
- **Validação:** instrucional no prompt do hook + checklist no `doctor`/docs.
  Rejeitado: bloqueio por hook (sem runtime confiável p/ julgar "rico"; Kiro
  `agent` não bloqueia; opencode `tool.before` bloquearia sessão por qualidade
  de texto = falso-positivo caro).
- **Pitfall questionado:** "`permission.ask`/gate garante handoff" — corrigido
  pela reality-check (`opencode-plugin-hooks-enforcement-reality`): soft law não
  enforça; aqui a garantia é convenção + review, não deny.

## ADR-6: `setup` e `doctor` — contrato de instalação

- `setup opencode|kiro`: merge aditivo, `.bak` em overwrite, `--force` explícito,
  `--dry-run` sem writes (padrão já existente em `setup.rs:121`, `kiro_target`).
- `doctor` (novo): valida presença dos 3 hooks + steering + `brain ping`
  (warn, não fail) + `grep` sem projeto hardcoded. Instalação incompleta =
  diagnóstico acionável, não erro opaco.
- OpenCode: estender `opencode_plugin_js` (`setup.rs:278`) para sessão com
  contexto + handoff; considerar `experimental.session.compacting` p/ injetar
  resumo do brain na compactação (fonte: docs OpenCode).

## 9. Próxima fase

Fase 3 (Projete) → `ARCHITECTURE.md`: layout dos arquivos-fonte de hooks por IDE,
fluxo de dados (trigger → ação → MCP/CLI → nota), formato do roteiro deepsearch,
contrato do `doctor`.
