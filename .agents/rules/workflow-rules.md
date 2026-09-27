# Workflow Rules

Estas regras definem o fluxo de desenvolvimento obrigatório. Devem ser seguidas rigorosamente por todos os agentes de IA.

---

## 0. Prompt Gate — ANÁLISE OBRIGATÓRIA (NON-NEGOTIABLE)

> 🚨 **TODA demanda do usuário DEVE ser analisada ANTES de qualquer ação.**

### Regra Absoluta

1. **Carregar skill `prompt-optimizer`** via `skill` tool
2. **Carregar skill `brain`** → `brain_search(prompt, top_k=3)` para contexto
3. **Classificar prompt:**
   - **Tier 1 (Crítico):** PARAR, perguntar 3-5 vezes antes de qualquer ação
   - **Tier 2 (Ambíguo):** PERGUNTAR 1-3 vezes
   - **Tier 3 (Claro):** PROSSEGUIR com workflow normal
4. **Se vago:** fazer perguntas clarificadoras e AGUARDAR resposta
5. **Se claro:** prosseguir

### Proibido

- ❌ Adivinhar o que o usuário quer
- ❌ "Vou assumir que você quer X"
- ❌ Delegar com contexto vago
- ❌ Pular brain_search
- ❌ Interpretar "melhorar", "corrigir", "fazer" sem contexto

### Obrigatório

- ✅ Carregar `prompt-optimizer` via skill tool
- ✅ Perguntar se houver qualquer ambiguidade
- ✅ Confirmar entendimento antes de prosseguir
- ✅ Salvar patterns de prompts no Brain

### Template de Perguntas

```markdown
## ❓ Preciso de mais contexto

Para prosseguir corretamente, preciso esclarecer:

1. **{PERGUNTA_1}**
2. **{PERGUNTA_2}**
3. **{PERGUNTA_3}**

### Contexto que já entendi:
- {contexto_identificado}

### O que NÃO entendi:
- {ambiguidades}

**Responda para que eu possa ajudar de forma precisa.**
```

---

## 0.1 Divisão de Papéis (NON-NEGOTIABLE)

> 🚨 **REGRA CRÍTICA:** Orchestrator (sdd-orquestrador) **NÃO DESENVOLVE**. Apenas orquestra, governa, documenta, delega.

| Role | Responsabilidade |
|------|------------------|
| **Orchestrator** (este agente) | Triagem, governance, SPEC, PLAN, TASKS, code review, brain, workflow-state.json |
| **developer-engineer** | **Implementação de código** (Rust crates brain-core/store/embed/mcp/web/cli/tests) |
| **fullstack-code-reviewer** | Code review após cada batch |
| **architect** | Decisões arquiteturais (validação) |
| **product-manager** | Validação de negócio |
| **qa-engineer** | Cobertura de testes |
| **security-engineer** | Auditoria de segurança |

**Fluxo correto:**
1. User pede nova phase
2. **Prompt optimizer** — analisar prompt (SEÇÃO 0)
3. **Brain search** para contexto (lições passadas, decisões)
4. **Salvar no Brain** aprendizados/erros/decisões
5. **Delegar para `developer-engineer`** via `task` tool
6. **Receber retorno** do developer
7. **Delegar para `fullstack-code-reviewer`** (regra non-negotiable, ver seção 6)
8. **Aplicar fixes** se necessário (re-invocar developer se crítico)
9. **Atualizar `workflow-state.json`** campo `code_review`
10. **Só então** declarar phase completa

**Erros comuns a evitar:**
- ❌ Orchestrator implementa código diretamente (write tool, edit tool)
- ❌ Pular prompt_optimizer antes de qualquer ação
- ❌ Pular brain_search antes de delegar
- ❌ Não salvar aprendizados no Brain
- ❌ Pular code review após implementação
- ❌ Declarar phase completa sem Pre-Completion Gate (seção 6.3)

---

## 1. Workflow State

- Antes de iniciar **qualquer** desenvolvimento, **SEMPRE** ler `workflow-state.json` na raiz do projeto.
- O arquivo contém o estado atual do workflow: fase ativa, especificações em andamento, tarefas concluídas.
- Após cada alteração relevante, atualizar o `workflow-state.json`.

---

## 2. Economia de Tokens (Caveman Mode)

Para reduzir custos e maximizar eficiência durante o desenvolvimento, **SEMPRE** utilizar modo de comunicação comprimida:

- Ativar caveman ao iniciar sessão: `/caveman` (modo `full`, padrão)
- Níveis disponíveis: `lite` (artigos mantidos), `full` (caveman clássico), `ultra` (telegráfico)
- **Exceções** (desabilitar automaticamente): avisos de segurança, ações irreversíveis, sequências multi-passo onde ambiguidade pode causar erro, usuário pedir esclarecimento
- **SEMPRE** manter em modo caveman para: respostas técnicas, code review, planejamento, debugging
- **NUNCA** usar caveman em: código gerado, mensagens de commit, PRs — estes mantêm prosa normal
- Reactivar após exceção: caveman retoma automaticamente depois da parte crítica
- Ver SKILL.md em `~/.config/opencode/skills/caveman/SKILL.md` para regras completas
- Estatísticas: `/caveman-stats` mostra tokens salvos na sessão

---

## 3. Workflow SDD (obrigatório para toda funcionalidade)

Toda nova funcionalidade/inovação **OBRIGATORIAMENTE** segue o fluxo SDD (Spec-Driven Development). Durante todo o SDD, manter `/caveman` ativo para economia de tokens.

**Ciclo completo**: `Explore (opcional) → Proposal → SPEC → PLAN → TASKS → [implementar] → Verify → Archive`

### Fase 0 — Exploração & Planejamento Técnico
> Para features com escopo claro, pular direto para Fase 1. Para features complexas/inovações, usar `dev-planner` para planejamento detalhado.

1. Carregar `brain` — health check MCP automático, consultar memória de longo prazo
2. Classificar demanda: **Feature**, **Bugfix** ou **Refactor**
3. **Se Feature complexa ou Inovação** (recomendado para novos módulos, integrações, mudanças arquiteturais):
   - Carregar skill `dev-planner`
   - Executar **Fase 0 (Descubra)**: questionar problema real, usuários, casos de uso → gerar `EXPLORATION.md`
   - Executar **Fase 1 (Analise)**: mapear restrições técnicas, dependências, compliance → gerar `CONSTRAINTS.md`
   - Executar **Fase 2 (Desafie)**: comparar alternativas, registrar ADRs → gerar `DESIGN-DECISIONS.md`
   - Executar **Fase 3 (Projete)**: modelo de dados, API, fluxos → gerar `ARCHITECTURE.md`
   - Executar **Fase 4 (Estime)**: decompor tarefas, estimar esforço, riscos → gerar `PLAN.md`
   - Executar **Fase 5 (Valide)**: consistência, revisão cruzada, handoff para `sdd-compiler`
   - **Portão:** aprovação explícita do usuário em cada fase do dev-planner antes de avançar
4. **Se Feature simples** (CRUD básico, ajuste pontual):
   - Carregar `sdd-compiler` modo `explore`
   - Levantar perguntas, hipóteses e alternativas de abordagem
   - Gerar `.spec/[feature]/EXPLORATION.md`
5. Avançar para Fase 1 somente após clareza suficiente e aprovação do usuário

### Fase 1 — Análise (Requirements + Design)
1. Carregar `brain` — health check MCP automático, consultar memória de longo prazo
2. Carregar `sdd-architecture-board` — arquitetura e decisões técnicas
3. **Se dev-planner foi executado:** incorporar artefatos (`EXPLORATION.md`, `CONSTRAINTS.md`, `DESIGN-DECISIONS.md`, `ARCHITECTURE.md`, `PLAN.md`) como base para SPEC
4. Gerar relatório de análise do domínio
5. **Design Frontend** (se frontend): carregar `frontend-design`, definir direção visual
6. Invocar agente `architect` para validar arquitetura
7. Corrigir gaps identificados
8. **Gerar PROPOSAL**: `sdd-compiler` modo `generate-proposal` → `.spec/[feature]/PROPOSAL.md`
   - Validar contra `asserts/proposal-gate.md` antes de avançar
9. **Gerar SPEC**: `sdd-compiler` modo `generate-spec` → `.spec/[feature]/SPEC.md`
   - SPEC inclui seção **Delta Specs** (impacto em `.doc/`)
10. **Revisar SPEC**: skill `spec-review` — gerar relatório de gaps e conformidade
11. **Corrigir gaps**: endereçar todos 🔴 críticos e 🟠 alta prioridade
12. Salvar artefatos em `.spec/backend/{dominio}/` ou `.spec/frontend/{dominio}/`
13. **Aprovação do usuário**: perguntar se deseja prosseguir com a implementação

### Fase 2 — Implementação
1. Carregar skill de domínio: `dev-planner` se feature complexa, senão `sdd-compiler`
2. **Gerar PLAN**: `sdd-compiler` modo `generate-plan` → `PLAN.md` a partir do `SPEC.md`
3. **Gerar TASKS**: `sdd-compiler` modo `generate-tasks` → `TASKS.md` a partir do `PLAN.md`
4. Implementar código seguindo as tasks (TDD — teste antes da implementação)
5. Escrever **testes unitários** (`cargo test -p brain-core|brain-store` in-mem, `brain-embed` mock Ollama)
6. Escrever **testes de integração** (`cargo test --workspace`) — todo tool deve ter IT `brain_store→search→read`
7. Verificar quebras de contrato com `cargo build --workspace` / `cargo check -p brain-store` a cada task

### Fase 3 — Review
1. Invocar agente `fullstack-code-reviewer` para revisar o código
2. Corrigir todos os débitos 🔴 **Críticos**
3. Avaliar e corrigir débitos 🟡 **Atenção** se aplicável
4. **Ao final**: `cargo build --workspace && cargo test --workspace` (zero erros, testes passando)

### Fase 4 — Verify (pós-implementação)
1. Carregar `sdd-compiler` modo `verify`
2. Checar **3 dimensões** contra `asserts/verify-gate.md`:
   - **Completeness**: todos os checkboxes TASKS.md marcados, zero TODOs
   - **Correctness**: build limpo, testes passando, requisitos SPEC cobertos
   - **Coherence**: ADRs refletidos no código, sem vazamento de camada
3. Se frontend: validar com Chrome DevTools (Network + Console — zero erros/warnings)
4. Gerar `.spec/[feature]/VERIFY.md` com resultado
5. **BLOQUEIA archive** se qualquer dimensão falhar — voltar para Fase 2/3

### Fase 5 — Archive (finalização)
> Executar somente com `VERIFY.md` aprovado (todas as 3 dimensões ✅)

1. Carregar `sdd-compiler` modo `archive`
2. Mesclar **Delta Specs** do `SPEC.md` nos documentos `.doc/` correspondentes
3. Mover `.spec/[feature]/` → `.spec/archive/[YYYYMMDD]-[feature]/`
4. **Registrar aprendizados** no Brain (skill `brain`):
   - Tipo: `success` | `decision` | `anti-pattern`
   - Template em `.agents/rules/brain-context-protocol.md` seção 3
   - Salvar via `ObsidianBrain_write_note` em `lessons/{tipo}/{subcategoria}/`
5. Commit sugerido pelo `sdd-compiler archive` — confirmar antes de executar

### 3.6 Feedback Obrigatório (10-Point Summary)

> 🚨 **REGRA NON-NEGOTIABLE.** Após CADA comando SDD que gere artefatos, orquestrador DEVE gerar 10-point summary.

**Comandos que exigem summary:**
- `generate-proposal` → ler PROPOSAL.md
- `generate-spec` → ler SPEC.md
- `generate-plan` → ler PLAN.md
- `generate-tasks` → ler TASKS.md
- `generate-integration` → ler INTEGRATION.md
- `verify` → ler VERIFY.md

**Template:** Ver `.agents/skills/sdd-feedback/SKILL.md`

**Fluxo:**
1. Comando SDD executa e gera artefato
2. Orquestrador lê artefato
3. Gera 10-point summary: Decisions → Generated → Review → Watch-outs → Next Steps
4. Inclui brief status: `📊 **{name}** ({stage}) | {progress}%`
5. **OPCIONAL:** brain_store se summary revelar lição relevante

**Anti-pattern eliminado:** "Fase completa ✅" sem evidência do que foi gerado.

### 3.7 Feature Status Dashboard

> Dashboard automático para rastrear progresso de features.

**Fonte de verdade:** `workflow-state.json` campo `features`

**Brief status** (em todo summary):
```
📊 **{name}** ({stage}) | {progress}% | {done}/{total} features
```

**Detailed status** (sob demanda: "show status", "what blocks X?"):
```markdown
📊 Project Feature Status Dashboard
🎯 CURRENT: {name} ({pct}%)
✅ COMPLETED: {n}
📋 UPCOMING: {n}
⚠️  BLOCKED: {n}
```

**Natural language management:**
- "add feature X" → criar entry
- "move X before Y" → reordenar
- "skip X" → marcar deferred
- "what blocks X?" → check dependencies

**Skill:** `.agents/skills/feature-status-dashboard/SKILL.md`

### 3.8 Traceability Validation (antes de Archive)

> 🚨 OBRIGATÓRIO antes de archive. Verifica se specs batem com código.

**Comando:** `/sdd.trace [feature]`

**Critérios:**
- RF→Task mapping ≥80%
- Task→Code mapping ≥80%
- Zero orphaned specs
- Score geral ≥80%

**Assert:** `.agents/skills/sdd-compiler/asserts/traceability-gate.md`

**Bloqueia archive** se score <60%. Warn se 60-79% (arquivo com justificativa).

---

## 4. Workflow Bugfix (obrigatório para toda correção de bug)

Toda demanda de correção de bug (backend, frontend ou fullstack) **OBRIGATORIAMENTE** segue o fluxo abaixo em 4 fases. Durante todo o bugfix, manter `/caveman` ativo para economia de tokens.

### 4.0 Regras Mandatórias de Harness e Análise para Bugfix (NON-NEGOTIABLE)

> 🚨 **Estas 4 regras são OBRIGATÓRIAS e NON-NEGOTIABLE. Violação = retrabalho completo do bugfix.**

#### R1 — Harness-First: Verificar o erro REAL na tela antes de codificar
- Sempre que um **bug for reportado**, o agente de codebase (`developer-engineer` ou `qa-engineer`) **DEVE**:
  1. Subir o sistema (backend `:8080` + frontend `:5173` se down — `mvn spring-boot:run -pl bootstrap` + `npm run dev`; credenciais dev se necessário).
  2. Abrir o sistema via **Chrome DevTools** (`mcp__chrome-devtools__new_page` → URL, `take_snapshot`, `list_network_requests`, `list_console_messages`, `take_screenshot`).
  3. **Reproduzir o fluxo exato reportado** e **confirmar que o erro realmente existe na tela** (status 500/400, payload, console `[ERROR]`/`[WARN]`, snapshot vazio/dados ausentes).
  4. **Só APÓS confirmar na tela**, analisar o código e retornar **relatório detalhado** do que foi encontrado (arquivo:linha, causa, stack trace, evidências de network+console+snapshot).
- **PROIBIDO adivinhar no escuro.** Sem evidência de harness (screenshot, network 5xx, console erro), a análise é considerada inválida e o orquestrador deve **re-delegar**.
- Artefatos obrigatórios no retorno do subagente: `backend UP/DOWN`, `frontend UP/DOWN`, `console N errors`, `network M 2xx/5xx`, `screenshot path`, status do erro (reproduzido ✅/não reproduzido ❌).

#### R2 — Encerramento limpo: finalizar front e backend ao terminar DevTools
- Ao finalizar os **testes do sistema com DevTools** (harness), o subagente **SEMPRE** deve **finalizar tanto o front quanto o backend que foram iniciados** para o teste.
- Não deixar processos órfãos (`:8080`/`:5173`) consumindo porta/CPU. Use `lsof -i :8080` / `lsof -i :5173` + `kill` ou equivalente, ou garantir que o harness encerra os processos ao sair.
- O orquestrador deve verificar no retorno: `processos finalizados ✅/❌`. Se deixado aberto, registrar como débito técnico e corrigir na próxima iteração.

#### R3 — Análise detalhada ANTES do desenvolvimento
- Para **todo bug**, deve ser **SEMPRE validado primeiro o comportamento real do sistema** (R1), **realizar uma análise bem detalhada** (causa raiz com arquivo:linha, camada, módulo) e **só DEPOIS iniciar o desenvolvimento**.
- Sem essa análise, o agente se perde na hora de avaliar o real problema do sistema — gera correções no lugar errado, mascarando o bug.
- Artefato obrigatório: `.spec/bugs/{contexto}/TASKS.md` com **Bug**, **Causa raiz** (com referência a código), **Solução proposta** (arquivos a alterar, impacto), **Testes necessários**, **Riscos** — preenchido **antes** de delegar B2 (Implementação).
- **Portão B1→B2 (HITL):** orquestrador apresenta bug + causa raiz + evidências de harness ao usuário e pergunta: *"Bug analisado: [causa] em [arquivo:linha]. Solução proposta: [resumo]. Aprova seguir para implementação da correção?"* — aguardar resposta explícita.

#### R4 — Persistência de erros no Brain
- **Sempre salvar qualquer erro encontrado no Brain do projeto.**
- Classificação obrigatória de `scope`:
  - Erro/líção **específica do projeto** (ex: `Project.projectTeams` LAZY + `NOT_SUPPORTED` → LIE) → `scope="projetos"` em `brain_store(layer="regras"|"arquitetura"|"estudos", scope="projetos")`.
  - Erro/líção **de regra da stack** (ex: padrão Hibernate `open-in-view=false` + `EntityGraph` + `distinct` p/ paginação, ou padrão `ControllerExceptionHandler` não traduzir `i18nKey`) → `scope="global"` para reuso entre todos os projetos.
- Tipos: `sessoes/atlas-ecm/<data>` (resumo da sessão) + `estudos/atlas-ecm/<feature>/<conceito>` (conteúdo completo) + `regras/atlas-ecm/<regra>` (quando virar regra).
- **Checklist:** antes de marcar bug como `Completed`, verificar `brain_store` executado com scope correto. Sem registro no Brain, o bug NÃO está fechado.

### Fase 1 — Análise do Bug

1. **Carregar `brain`** — consultar memória de longo prazo do sistema, buscar lições aprendidas relacionadas
2. **Reproduzir o bug** — entender:
   - Cenário exato que dispara o erro
   - Comportamento esperado vs comportamento atual
   - Stack trace / mensagem de erro completa
   - Evidências (logs, prints, requests)
3. **Análise de causa raiz** — rastrear até a origem:
   - Qual camada? (backend/frontend/ambos)
   - Qual módulo/domínio? (scheduling, clinical, etc.)
   - Qual arquivo/função? (usar `grep`, `glob`, `explore` para localizar)
4. **Criar TASKS.md** em `.spec/bugs/{contexto}/TASKS.md` documentando:
   - **Bug**: descrição concisa do problema
   - **Causa raiz**: explicada com referência ao código
   - **Solução proposta**: abordagem, arquivos a alterar, impacto
   - **Testes necessários**: unitários, integração, frontend
   - **Riscos**: efeitos colaterais potenciais
5. **Atualizar `workflow-state.json`**: registrar bug em análise

### Fase 2 — Implementação da Correção

1. Executar as tasks na ordem definida no TASKS.md
2. Para bugs **fullstack** (backend + frontend):
   - Carregar skill `java-architecture-specialist` + `frontend-vue-specialist`
3. Implementar a correção seguindo as tasks:
   - **Backend**: alterar código, atualizar testes unitários existentes (se quebrarem), adicionar novos testes se necessário
   - **Frontend**: alterar código, atualizar/adicionar testes (`*.spec.ts`)
4. **Verificar compilação** a cada task concluída:
   - Backend: `mvn compile -pl {modulo} -am`
   - Frontend: `npx vite build` (verificação rápida) ou `npm run build`
5. Se aplicável: criar/adicionar **testes de integração** no módulo `integration-test/`

### Fase 3 — Revisão Obrigatória (Iterativa)

1. **Invocar `fullstack-code-reviewer`** com contexto completo do bug e das correções — passar diff ou arquivos alterados
2. **Avaliar resultado da revisão**:
   - Se 🔴 **Críticos** ou 🟡 **Atenção** → retornar para Fase 2, corrigir todos os gaps apontados
   - Se apenas 🟢 **Sugestões** → avaliar e corrigir se pertinente
3. **Loop de qualidade**:
   - Após corrigir gaps na Fase 2, invocar novamente `fullstack-code-reviewer`
   - Repetir até que o relatório aponte **zero 🔴 e zero 🟡**
   - Máximo de 3 iterações; se exceder, escalar para revisão manual
4. **Frontend — Teste Obrigatório com Chrome DevTools** (NÃO PULAR):
   - Servidor dev rodando (`npm run dev`)
   - Chrome DevTools (F12) aberto nas abas **Network** + **Console**
   - Executar o fluxo completo que foi corrigido
   - **Validar Network**:
     - URLs corretas (sem `/api/api/` duplicado)
     - Métodos HTTP corretos (GET/POST/PUT/DELETE)
     - Headers de autorização presentes (Bearer token, X-Tenant-ID)
     - Payloads com estrutura esperada pelo backend
     - Status codes 200/201 (nunca 400/404/500)
   - **Validar Console**: zero erros (`[ERROR]`), zero warnings (`[WARN]`)
   - **Validar UI**: renderização correta, dados aparecendo, sem tela branca
   - **Se DevTools falhar** (qualquer ponto acima) → retornar para Fase 2
5. Se correção for **apenas backend** (sem frontend envolvido), pular etapa 4

### Fase 4 — Finalização

> Zero tolerância a falhas pré-existentes — ver `.agents/rules/harness-continuous.md` H2.

1. **Backend**: `mvn clean install -Dintegration.test.skip=false -Pintegration` na raiz (zero erros, todos os testes passando — **incluindo pré-existentes**)
2. **Frontend**: `npm run test:run` (zero falhas — **incluindo pré-existentes**)
3. **Atualizar `workflow-state.json`**:
   - `current_feature`: nome do bug
   - `current_phase`: `Completed`
   - Registrar arquivos alterados e resumo da correção
4. **Registrar aprendizados no Brain** via skill `brain`:
   - Tipo obrigatório: `error` (template `ERROR_LESSON_TEMPLATE.md` em `.agents/rules/brain-context-protocol.md` seção 3.1)
   - Preencher: causa raiz, solução aplicada, prevenção, severity, frequency
   - Tags: `eng/{backend|frontend|fullstack}` + `type/error` + `bugfix`
   - Salvar via `ObsidianBrain_write_note` em `lessons/errors/{subcategoria}/{data}-{titulo}.md`
   - Executar checklist da seção 4.3 do brain-context-protocol
5. **Se o bug foi introduzido por implementação anterior**: registrar débito técnico como `related_notes` no Brain

---

## 5. Testes Obrigatórios

> 🚨 **Harness Contínuo é NON-NEGOTIABLE.** Ver `.agents/rules/harness-continuous.md` (fonte da verdade). Esta seção 5 é complementar; em conflito, harness-continuous prevalece.

**TODO** desenvolvimento (features OU bugfixes) **OBRIGATORIAMENTE** deve incluir testes.

### 5.1 Backend — Testes Unitários

Consultar regras detalhadas em: `.agents/rules/backend-unit-tests.md`

**Obrigatório:**
- **Service Layer**: todo `*ServiceImpl.java` deve ter teste unitário cobrindo:
  - Fluxo principal (happy path)
  - Fluxos de erro (validações, entidade não encontrada, conflitos)
  - Casos de borda (edge cases)
- **Domain Entities**: invariantes, métodos de fábrica, validações
- **Mappers/Converters** (MapStruct): via `Mappers.getMapper()` — nunca mockar
- **Validators**: regras de negócio customizadas
- **Utils/Helpers**: funções utilitárias puras

**Proibido em testes unitários:**
- ❌ Repository layer — testado via integração
- ❌ Controller/REST layer — testado via integração (`*IT.java`)
- ❌ `@SpringBootTest` — apenas integração
- ❌ `@WebMvcTest` — proibido, controllers vão para integração
- ❌ H2 / banco de dados — mocks isolam a lógica
- ❌ Lombok (proibido no projeto)
- ❌ `Thread.sleep()` — usar Awaitility

**Ferramentas:**
- JUnit 5 (`@Test`, `@BeforeEach`, `@DisplayName`, `@ExtendWith(MockitoExtension.class)`)
- Mockito (`@Mock`, `@InjectMocks`, `verify()`, `ArgumentCaptor`)
- AssertJ (preferencial) ou JUnit assertions
- Fakes em `src/test/java/.../fake/` para dados de teste

**Execução:**
```bash
mvn test                              # Todos os testes unitários
mvn test -pl scheduling/impl          # Testes de um módulo específico
```

### 5.2 Backend — Testes de Integração

Consultar regras detalhadas em: `.agents/rules/backend-integration-tests.md`

**Obrigatório:**
- **REST Controllers**: TODO controller DEVE ter teste de integração validando:
  - HTTP success paths (200, 201, 204)
  - Error/validation paths (400, 404, 409)
  - Auth/Authorization paths (401, 403)
  - Multi-tenancy isolation (SAAS mode)
- **Repository Layer**: queries JPA, `@Query` nativas, paginação, constraints
- **Cross-module flows**: outbox patterns, eventos, sagas
- **Flyway migrations**: bootstrap smoke test valida migrations

**Regras:**
- Arquivos: `*IT.java` (nunca `*Test.java`)
- Localização: `backend/integration-test/src/test/java/`
- Banco: PostgreSQL 16.3-alpine real via Testcontainers (nunca H2)
- Base class: `WebIntegrationTest` (SAAS) ou `WebOnPremiseIntegrationTest` (ONPREMISE)
- Limpeza entre testes: `DatabaseCleaner.reset()` com `TRUNCATE ... CASCADE`
- Autenticação: `JwtTestHelper.loginAndGetToken()` ou `loginAndGetToken()` (legado)
- Setup de dados: `TestUserFactory`/`TestTenantFactory` (preferencial) ou `seedCommonData()` (legado)

**Proibido em testes de integração:**
- ❌ H2 ou banco em memória — PostgreSQL real obrigatório
- ❌ `@WebMvcTest` — usar `@SpringBootTest(webEnvironment = RANDOM_PORT)`
- ❌ `repository.deleteAll()` para limpeza — usar `DatabaseCleaner.reset()`
- ❌ RestAssured — projeto usa MockMvc
- ❌ Testes de integração em módulos de domínio — apenas no módulo `integration-test/`

**Execução:**
```bash
mvn verify -pl integration-test                     # Todos os ITs (requer Docker)
mvn verify -pl integration-test -Dit.test=PatientIT # IT específico
mvn clean install -Dintegration.test.skip=false     # Build completo com ITs
```

### 5.3 Frontend — Testes

Consultar regras detalhadas em: `.agents/rules/frontend-coding-standards.md`

**Obrigatório:**
- **Stores (Pinia)**: testar actions e getters
- **Views/Páginas**: testar renderização e interações do usuário
- **Serviços API**: testar chamadas HTTP e tratamento de erros
- **Componentes**: validar renderização com props, slots e eventos

**Ferramentas:** Vitest + happy-dom (sem jsdom)
**Localização:** `src/**/__tests__/*.spec.ts`
**Coverage mínima:** 70% nas novas funções/componentes

**Execução:**
```bash
npm run test:run                              # vitest --run
npm run build                                 # vue-tsc --noEmit && vite build
```

### 5.4 Regra Geral de Testes

> Substituída por `.agents/rules/harness-continuous.md` H1 + H2 (zero tolerância a falhas pré-existentes).

- Os testes DEVEM passar **antes** de qualquer commit/push.
- Se um teste falhar — **novo, existente ou pré-existente** — a implementação DEVE ser corrigida até os testes passarem (ver H1.2 do harness).
- `mvn clean install -Dintegration.test.skip=false -Pintegration` DEVE retornar 0 falhas/erros/skips não-autorizados antes de finalizar tarefa **E** antes de iniciar próximo dev (ver H2.2).

---

## 6. Code Review Obrigatório

### 6.1 Invocação e Aplicação

> 🚨 **REGRA ABSOLUTA — NÃO PULAR.** Pular o code review após implementação = violar o harness. Sem exceção.

Após finalizar a implementação de **QUALQUER** tarefa, sub-fase ou mudança de código (incluindo sub-fases de SDD, bugfixes, ou qualquer modificação em `src/`), invocar **OBRIGATORIAMENTE** o agente `fullstack-code-reviewer` antes de:

- Declarar a tarefa como concluída
- Avançar para a próxima sub-fase
- Marcar TODOs como completos
- Reportar status como "done" para o usuário

**A revisão DEVE verificar:**

| Critério | O que verificar |
|----------|-----------------|
| **Qualidade** | Naming, estrutura, complexidade, coesão |
| **Testes** | Cobertura adequada, qualidade dos asserts, isolamento |
| **Segurança** | SQL injection, exposição de dados sensíveis, validação de entrada |
| **Performance** | N+1 queries, streams em caminhos críticos, alocação de coleções |
| **Arquitetura** | Package boundaries, DDD, dependências entre módulos |
| **Regras** | Conformidade com `.agents/rules/*.md` |

**Classificação dos débitos:**
- 🔴 **Crítico** — DEVE ser corrigido antes de concluir a tarefa
- 🟡 **Atenção** — DEVE ser avaliado e corrigido se aplicável
- 🟢 **Sugestão** — Fica a critério, mas deve ser considerada

### 6.2 Loop de Correção (MANDATORY)

Após o code review, executar loop de correção **SEM SAIR** até zero 🔴:

```
1. Receber review do fullstack-code-reviewer
2. Se 🔴 existir: corrigir TODOS os críticos
3. Se 🟡 existir: avaliar caso a caso, corrigir os aplicáveis
4. Re-executar build/tests
5. Re-invocar fullstack-code-reviewer
6. Repetir 2-5 até zero 🔴
7. Máximo 3 iterações; após 3 iterações, escalar para revisão manual do user
```

### 6.3 Checklist Pré-Conclusão (MANDATORY)

> 🚨 **Substituído** por `.agents/rules/harness-continuous.md` **H6 — Portão de Conclusão Expandido (7 itens)**. Esta versão reduzida é apenas resumo; o portão vinculante é o do harness.

Antes de declarar QUALQUER fase de implementação como completa, **TODOS** os itens abaixo devem ser verdadeiros:

```markdown
## Pre-Completion Checklist (Harness Gate — NON-NEGOTIABLE)

- [ ] `fullstack-code-reviewer` foi invocado APÓS a última mudança de código
- [ ] Zero débitos 🔴 Críticos no report mais recente
- [ ] `mvn clean install -Dintegration.test.skip=false -Pintegration` → 0 falhas (H1 + H2)
- [ ] E2E Greenfield executado quando aplicável (H3) — DB wipe + re-cadastro UI 12 passos
- [ ] Avaliação contínua H4 (telas + regras + lógica + performance) preenchida
- [ ] `brain_store` de lições executado (H5.2)
- [ ] Processos :8080/:5173 finalizados (H5.1)
```

**Se QUALQUER item falhar → fase NÃO está completa.** Voltar e corrigir.

### 6.4 Tracking em `workflow-state.json`

Após cada code review, atualizar o `workflow-state.json` com:

```json
{
  "code_review": {
    "last_invoked_at": "2026-07-30T19:30:00Z",
    "last_status": "passed | failed",
    "critical_issues": 0,
    "attention_issues": 0,
    "reviewer_notes": "Resumo dos principais achados"
  }
}
```

**Não avançar para próxima fase se `code_review.last_status != "passed"`.**

### 6.5 Em Que Momento Invocar

| Momento | Invocar? |
|---------|----------|
| Após cada sub-fase de SDD (1.0, 1.1, 1.2, ...) | ✅ SIM — sempre |
| Após criar/modificar qualquer arquivo `.java`, `.ts`, `.vue`, `.sql` | ✅ SIM — ao final do batch |
| Antes de dizer "Phase X completa" para o user | ✅ SIM — sem exceção |
| Antes de iniciar nova fase | ✅ SIM — se última fase não revisada |
| Durante o desenvolvimento (mid-task) | ❌ NÃO — esperar fim do batch |

**Definição de "batch"**: conjunto de mudanças relacionadas que entregam uma unidade funcional (ex: Phase 1.6 = 3 tasks FE-01..03 = um batch = um review).

---

## 7. Uso Obrigatório de Skills

Para **TODO** desenvolvimento, as skills apropriadas **OBRIGATORIAMENTE** devem ser carregadas:

| Contexto | Skill | Propósito |
|----------|-------|-----------|
| **Análise inicial** | `brain` | `brain_search` scope `global|projetos` antes de codar |
| **Prompt Gate** | `prompt-optimizer` | Tier1-3 classificação, `brain_search(prompt, top_k=3)` |
| **Planejamento Técnico** | `dev-planner` | Feature complexa: Descubra→Valide (6 fases) → artefatos p/ sdd-compiler |
| **Arquitetura** | `sdd-architecture-board` | ADRs, validação crates `brain-core/store` |
| **Desenvolvimento** | `brain` + `brain-core/store/embed` (via delegate `developer-engineer`) | Rust crates, Store WAL, chunk ##, sanitize |
| **SDD Compiler** | `sdd-compiler` | `explore/proposal/spec/plan/tasks/verify/archive` com asserts |
| **Revisão de Spec** | `spec-review` | Gaps vs `.agents/rules/architecture-rules.md` |
| **Revisão de Segurança** | `security-review` | SDD gate se FTS/auth futuro |
| **Revisão de Código** | *(task `code-reviewer`)* | `fullstack-code-reviewer` zero 🔴 |
| **Microtasks** | `jira-microtask-breaker` | Split tasks 2-4h |
| **Dashboard** | `feature-status-dashboard` | `workflow-state.json` features progress |
| **Comunicação** | `caveman` | `full` por padrão (`~65%` tokens) |

**Orchestrator** carrega `prompt-optimizer+brain+sdd-compiler+spec-review`; **developer-engineer** carrega `brain+caveman` + domínio Rust. Nunca `java-architecture-specialist`/`frontend-vue-specialist` (legado).

**Regra:** O desenvolvedor (agente) SEMPRE deve carregar a skill correspondente ao que está implementando. As skills contêm templates, exemplos e boas práticas que garantem consistência e qualidade.

---

## 8. Regras Gerais

- **Nunca** pular a leitura do `workflow-state.json`.
- **Nunca** implementar sem spec (features) ou sem análise (bugfixes).
- **Nunca** pular a fase de testes — testes unitários + integração são obrigatórios.
- **Nunca** pular o code review — invocar `fullstack-code-reviewer` é obrigatório. **Esta regra é NON-NEGOTIABLE.** Ver seção 6 para protocolo completo.
- **Nunca** declarar fase completa sem o Code Review Gate da seção 6.3.
- **Economia de tokens:** Utilizar caveman mode (`/caveman`) como padrão durante desenvolvimento para reduzir consumo de tokens (~65%). Exceções: avisos de segurança, ações irreversíveis, usuário solicitar "normal mode".
- Atualizar o workflow state ao concluir cada fase ou milestone.
- Registrar aprendizados no Brain ao final de cada tarefa.

---

## 9. Documentos de Referência

| Documento | Propósito |
|-----------|-----------|
| `.doc/stacks.md` | Stacks e versões |
| `.doc/arquitetura-backend.md` | Arquitetura detalhada do sistema |
| `.doc/especificacao-sistema.md` | Especificação completa do sistema |
| `.spec/backend/sdd-etapa{N}-{dominio}.md` | SDD de cada domínio |
| `.agents/rules/backend-unit-tests.md` | Regras detalhadas de testes unitários |
| `.agents/rules/backend-integration-tests.md` | Regras detalhadas de testes de integração |
| `.agents/rules/backend-coding-standards.md` | Padrões de codificação backend |
| `.agents/rules/frontend-coding-standards.md` | Padrões de codificação frontend |
| `.agents/rules/db-migration-flyway.md` | Regras de migração Flyway |
| `.agents/rules/security-standard.md` | **Fonte da verdade** de segurança (OWASP) — web, API, mobile |
| `.agents/rules/brain-context-protocol.md` | Protocolo de contexto do Brain |
| `.agents/rules/sdd-workflow-standard.md` | Padrão do workflow SDD |
| `.agents/skills/dev-planner/SKILL.md` | **Planejamento técnico detalhado** — 6 fases (Descubra→Analise→Desafie→Projete→Estime→Valide), gera artefatos para sdd-compiler |
| `AGENTS.md` | Visão geral do projeto e comandos essenciais |
| `~/.config/opencode/skills/caveman/SKILL.md` | Regras completas do modo caveman |
| **`.agents/rules/harness-continuous.md`** | **FONTE DA VERDADE** do harness (H1-H7) — suite completa, zero tolerância, E2E greenfield, avaliação contínua, portão de conclusão |
