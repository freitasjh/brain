# ARCHITECTURE — Brain Especialista

> Fase 3 dev-planner (Projete). Sem mudança de schema, sem tool MCP nova, sem
> mudança em RRF/dimensão. Tudo reusa `notes/chunks/projects` + tools existentes.

## 1. Layout-alvo (arquivos-fonte versionados)

```
brain/
  .agents/skills/brain/
    SKILL.md                      # +seção "Roteiro deepsearch" (fonte única, §4)
    tools/<17 existentes>/        # inalteradas
  .agents/rules/BRAIN.MCP.md      # +ponte p/ roteiro (3 linhas, sem duplicar)
  hooks/
    brain-hook.sh/.py             # legado; corrigir chamada p/ binário `brain`
    kiro/
      brain-session.json          # command (gerado hoje; passa a copiado+fiel)
      brain-learning.json         # agent  (NOVO, estático — ADR-1)
      brain-prompt-optimizer.json # agent  (NOVO, estático — ADR-1)
    opencode/
      brain-session.js            # estende opencode_plugin_js (setup.rs:278)
    steering/
      brain-learning.md           # steering kiro sempre-ativa (NOVO)
  crates/brain-cli/src/setup.rs   # estende: copia estáticos + doctor (§5)
```

## 2. Modelo de dados (existente, convenções novas)

Sem DDL. Convenções de gravação (já prescritas no boleto, viram contrato):

| Camada | Path | Conteúdo |
|--------|------|----------|
| `sessoes` | `<proj>/<AAAA-MM-DD>-<assunto>` | resumo + decisões + próximos passos + evidências |
| `estudos` | `<proj\|global>/<assunto>` | completo + frontmatter `tags/topico/nivel/fonte` + loop qualidade |
| `regras`/`arquitetura` | `<proj\|global>/<assunto>` | durável, scope correto |
| `projetos` | `<nome>` | metadados do projeto consumidor |

Regras: `path` relativo (sem repetir `layer/scope`), `project=<nome>` sempre em
nota de projeto, preferir atualizar a duplicar, dedup por conteúdo no spool.

## 3. Fluxos

### F1. Deepsearch (CU1) — só agente + skill, sem hook
```
prompt feature → skill "Roteiro deepsearch":
  1. varredura larga (query literal, top_k 10, sem filtro)
  2. afunila (layer=scopes do domínio, scope=projetos, project=<nome>)
  3. contraponto global (scope=global, top_k 3)
  4. brain_read integral dos hits que decidem
  5. síntese com paths citados ou "busca vazia" declarado
→ só então codar. Busca vazia ≠ falha: registra ausência e segue.
```

### F2. Contexto no prompt (CU2)
```
Kiro UserPromptSubmit ─┬─ command: `brain hook --event session-start
                       │   --question "$USER_PROMPT"` → spool + stdout curto
                       │   (top 3 sessões + top 3 regras, truncado; falha=warn)
                       └─ agent: "antes de agir, rode o roteiro deepsearch
                           (§4); dispense se somente-leitura"
OpenCode ────────────── plugin session/prompt-submit equivalente;
                       compactação: experimental.session.compacting injeta
                       resumo (últimas sessões + decisões abertas).
```

### F3. Handoff rico (CU3)
```
Stop/fim ── agent: "avalie o turno; se trivial, skip; senão grave via MCP:
  sessoes/<proj>/<data>-<assunto> (resumo) + estudos/... (se aprendeu) +
  regras|arquitetura/... (se durável). Rode o loop qualidade antes de
  considerar o estudo pronto."
command (existente): spool bruto como fallback auditável.
```

### F4. Instalação (CU4)
```
brain setup opencode → MCP SSE + plugin sessão + skill ref
brain setup kiro    → copia hooks/kiro/*.json p/ .kiro/hooks/ + steering
brain setup shell/systemd/--project (existentes, inalterados)
brain doctor (NOVO, §5) → valida tudo; warn acionável, nunca erro opaco.
```

## 4. Roteiro deepsearch (conteúdo que entra na skill)

1. Query = prompt literal (sem paráfrase) + 2 variações de domínio.
2. Larga → afunila → contraponto global (F1 acima).
3. `brain_read` integral antes de citar; nunca afirmar pelo snippet.
4. Saída do agente: tabela decisão→fonte (`layer/scope/path`) ou "sem contexto".
5. Pós-decisão: `brain_store` imediato (decisão não espera fim de sessão).

## 5. Contrato do `doctor` (CLI, reusa funções de `setup.rs`)

Checks (todos binários, saída `ok/warn/fail` + "como corrigir"):
- D1: 3 hooks kiro presentes e `version=v1`, triggers na lista fechada.
- D2: steering presente; agent-hooks sem nome de projeto hardcoded.
- D3: plugin opencode registrado (quando IDE=opencode).
- D4: `brain ping` → pong (fail = warn "suba com `brain serve-mcp`").
- D5: `BRAIN_DB_PATH` resolvido e gravável; avisa se cairia no default relativo.
- D6: skill `brain` referenciada no `opencode.json`/`instructions` do projeto.

## 6. O que NÃO muda (fronteiras)

- Schema v4, RRF k=60/4 streams/authority, dim 768, chunk `## `, fila de embedding.
- `brain-web` read-only; sem endpoint novo. `brain hook` sem drain do spool (Z-01).
- Harness: nenhum arquivo; integração = referência externa + `brainStore` evidência.

## 9. Próxima fase

Fase 4 (Estime) → `PLAN.md`: decomposição em tarefas, esforço, riscos, ordem
(sugerida: skill → kiro estáticos → setup/doctor → opencode plugin → docs).
