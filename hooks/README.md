# Brain Hooks — matriz por IDE

Hooks ligam o brain ao loop do agente: contexto entra no prompt,
aprendizado curado volta via MCP `brain_store`. Fonte única de verdade:
os arquivos abaixo (o instalador copia; nunca o contrário).

## Matriz IDE

| IDE | Instala (`brain setup`) | Agent-hooks no projeto | Steering | Valida (`brain doctor`) | Global |
|-----|-------------------------|------------------------|----------|-------------------------|--------|
| Kiro | `brain setup kiro --dir <proj>` → 4 arquivos em `<proj>/.kiro/` | `hooks/brain-session.json` (command) + `hooks/brain-learning.json` (agent) + `hooks/brain-prompt-optimizer.json` (agent) | `.kiro/steering/brain-learning.md` | D1 (hooks) + D2 (steering) | `~/.kiro/hooks/` — convenção do Kiro p/ hooks fora de projeto; documentado, não instalado pelo setup |
| OpenCode | `brain setup opencode --dir <proj> --brain-dir <checkout>` → plugin + `opencode.json` | plugin `brain-session.js` (eventos verificados `chat.message`, `experimental.chat.system.transform`, `experimental.session.compacting`) | n/a (skill via `opencode.json`) | D3 (plugin) + D6 (skill ref) | `~/.config/opencode/plugins/brain-session.js` |
| Ambos | — | — | — | D4 (backend warn-nunca-fail) + D5 (DB gravável) | — |

Detalhe dos instalados em `crates/brain-cli/src/setup.rs` (`kiro_target`,
`opencode_target`, `opencode_plugin_target`) e dos checks em
`crates/brain-cli/src/doctor.rs` (D1–D6).

## Kiro — 3 hooks + steering

Fontes (o `setup kiro` copia os 4, byte-fiel; `sanity.sh` valida):

| Arquivo (fonte) | Tipo | Trigger | Papel |
|-----------------|------|---------|-------|
| `hooks/kiro/brain-session.json` | `command` | `UserPromptSubmit` + `Stop` | Captura automática (spool via `brain hook --event session-start/session-end`) |
| `hooks/kiro/brain-learning.json` | `agent` | `Stop` | Lembra o agente de curar aprendizado via MCP `brain_store` (skip se trivial; nunca via CLI) |
| `hooks/kiro/brain-prompt-optimizer.json` | `agent` | `UserPromptSubmit` | Manda ler a skill `prompt-optimizer` (`.agents/skills/prompt-optimizer/SKILL.md`) + roteiro de busca da skill brain (dispensa em somente-leitura) |
| `steering/brain-learning.md` | steering | sempre-ativo | Regra buscar-antes/gravar-depois via MCP |

Regras dos estáticos (checadas por `bash hooks/kiro/sanity.sh`, 3 asserts):
triggers ∈ lista fechada · sem projeto hardcoded · sem path absoluto ·
sem `--db` · agent-hook nunca instrui `brain hook --event` (curadoria só via
MCP) · prompt ≤40 linhas (warn) · roteiro mora só na skill (ponte, sem drift).

Aprendizado curado é gravado pelo **agente** via MCP `brain_store`, nunca via
CLI. `path` sempre relativo sem repetir layer/scope; `scope` correto
(`projetos` vs `global`).

## OpenCode — plugin de sessão

Fonte: `hooks/opencode/brain-session.js` (+ testes `hooks/opencode/brain-session.test.mjs`,
`node --test`). Só assina eventos com call site verificado no runtime
instalado (reality-check OC-01: `session.created/session.idle/message.updated`
são internos sem trigger — assinar seria hook morto). Fail-open total (MCP
fora = sem mutação, sem throw); anexo truncado em 4000 chars (mesma classe de
`hooks/brain-hook.sh`); `output.prompt` da compactação nunca sobrescrito.
Decisão do spike: `.spec/brain-specialist/OC-02-DECISION.md`.

## `doctor` D1–D6 (só leitura; fail → exit 1, warn → exit 0)

| Check | O que valida | Falha típica |
|-------|--------------|--------------|
| D1 | 3 hooks kiro presentes, `version=v1`, triggers na lista | hook removido/editado → fail + caminho exato |
| D2 | steering presente; agent-hooks sem `--db`/absoluto/hardcoded | steering apagado → fail |
| D3 | plugin opencode registrado chamando `brain hook` | plugin ausente → fail |
| D4 | backend alcançável — **warn, nunca fail** | MCP/Ollama fora → warn, exit 0 |
| D5 | `BRAIN_DB_PATH` resolvido e gravável | pai inexistente → fail (`setup` já cria via `ensure_db_parent`) |
| D6 | skill `brain` referenciada (`opencode.json` do projeto ou config global) | sem ref → fail + como corrigir |

Nenhum check imprime credencial (segue o padrão do `--help` com redação).

## Genérico (qualquer cliente MCP)

```bash
# Antes de codificar: buscar contexto
brain search "contexto do projeto" --top-k 5

# Depois de uma decisão: salvar (scope obrigatório em arquitetura/regras/estudos)
brain store regras meu-projeto/decisao "## Decisão..." --scope projetos

# Ao finalizar sessão: salvar resumo
brain store sessoes meu-projeto/$(date +%Y-%m-%d) "## Sessão..."
```

## Variáveis de ambiente

| Variável | Default | Descrição |
|----------|---------|-----------|
| `BRAIN_URL` | `http://localhost:8321` | URL do brain server |
| `BRAIN_PROJECT` | `(detectado pelo git)` | Nome do projeto atual |
| `BRAIN_DB_PATH` | `./data/brain.db` | SQLite-only DB (doctor D5 valida) |
| `BRAIN_OLLAMA_URL` | `http://localhost:11434` | Embeddings (D4 nunca imprime com credencial) |
| `BRAIN_DIR` / `--brain-dir` | — | Checkout do brain p/ skill ref no `opencode.json` do projeto |
