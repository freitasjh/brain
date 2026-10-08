---
name: brain
description: >
  Connect to the Brain MCP server — 17 tools: hybrid semantic search, note
  storage with a background embedding queue, projects, audit/restore, TTL.
  Per-tool detail lives in tools/<tool>/SKILL.md, one directory per MCP tool.
  Carregue esta skill em qualquer projeto OpenCode para dar aos seus agentes
  memória persistente entre sessões. Armazenamento é SQLite (WAL + FTS5) com
  vetores 768-d do Ollama — não há vault de arquivos.
license: MIT
compatibility: OpenCode
metadata:
  category: integration
  complexity: beginner
---

# /brain Skill

Conecta qualquer repositório OpenCode ao servidor MCP **brain** — o cérebro central para agentes de IA.

## Pré-requisito

O servidor brain precisa estar rodando:

```bash
# No repositório brain:
brain serve-mcp          # MCP SSE em http://localhost:8321/sse
```

O `brain` aqui é o binário Rust (`cargo run -p brain-cli -- serve-mcp`, ou o
binário instalado). O viewer web read-only é `brain serve` (porta 8322).

## Como usar

No `opencode.json` do seu projeto, referencie esta skill:

```json
{
  "instructions": [
    "../brain/.agents/skills/brain/SKILL.md"
  ]
}
```

## As 17 tools MCP

Uma pasta por tool em `tools/`, com o detalhe de cada uma (parâmetros, retorno,
o que não fazer): `.agents/skills/brain/tools/<tool>/SKILL.md`.

### As 4 que você usa sempre

| Tool | Assinatura | O que faz |
|------|-------------|-----------|
| `brain_search` | `(query, layer?, scope?, project?, tag?, top_k?)` | Busca híbrida FTS5+vetor (RRF). **Chame antes de codar** |
| `brain_store` | `(layer, path, content, scope?, project?, tags?, pinned?, expires_at?)` | Salva nota. `scope` **obrigatório** em `arquitetura`/`regras`/`estudos` |
| `brain_read` | `(path)` | Lê uma nota. **Um parâmetro só** — `path` é o completo `layer/scope/path` |
| `brain_status` | `()` | Contagens + cobertura do embedding + estado da fila |

### As 13 restantes

| Tool | Assinatura | O que faz |
|------|-------------|-----------|
| `ping` | `()` | Health-check → `{"pong": true}` |
| `brain_recent` | `(top_k?)` | Notas mais recentes, com o conteúdo inteiro |
| `brain_checkpoints` | `(limit?)` | Audit log: `create`/`update`/`delete` com o `id` do restore |
| `brain_restore` | `(id)` | ⚠️ **Sobrescreve** a nota. Ver skill antes de usar |
| `brain_delete` | `(path)` | ⚠️ **Apaga** a nota e seus chunks |
| `brain_forget_sweep` | `(dry_run?)` | ⚠️ **Apaga** tudo que está vencido. Sempre `--dry-run` antes |
| `brain_export` | `(to?, force?)` | ⚠️ Escreve as notas como arquivos, só dentro de `BRAIN_EXPORT_ROOT` |
| `brain_backup` | `(to?)` | ⚠️ Copia o `brain.db` inteiro para um `.bak` |
| `brain_project_create` | `(name, description?)` | Cria projeto |
| `brain_project_list` | `()` | Lista projetos |
| `brain_project_notes` | `(name)` | Notas owned + linked de um projeto |
| `brain_project_link` | `(note_path, project)` | Liga nota a um projeto (many-to-many) |
| `brain_project_unlink` | `(note_path, project)` | Desliga. **Não apaga a nota** |

⚠️ = tem efeito destrutivo ou escreve em disco. As 5 têm skill própria com a
seção "O que NÃO fazer" — **leia antes de chamar**.

### Não é tool MCP

- **`brain reindex --all [--no-embed]`** — subcomando de **CLI**
  (`brain-cli:84`), não está no registro MCP. Skill em
  `.agents/skills/brain/cli/brain_reindex/SKILL.md`.
- **`brain_project_delete`**, `brain_migrate`, `brain serve`, `brain server
  start`, `brain setup`, `brain hook` — todos CLI (`brain-cli:66-115`).

Se você viu um nome de tool numa tabela que não está na lista de cima, ele é de
CLI. Isso já aconteceu: `brain_reindex` e `brain_delete_page` aparecem em
documentação antiga e não são tools.

## O contrato assíncrono do `brain_store`

**Uma escrita não espera por vetor.** É o comportamento central, e o motivo de
`brain_status` existir.

```json
{"ok": true, "path": "regras/projetos/meu-projeto/naming",
 "chunks": 4, "embedded": 0, "without_embedding": 4, "queued": 4}
```

`embedded: 0` numa nota nova **não é bug** e `queued: 4` **não é erro**. O que a
tool garante no momento do retorno é: nota gravada e **FTS5 populado**. O vetor
chega depois, em background. Medido no caminho MCP real, a escrita de 64 chunks
volta em **0.019 s** e os 64 vetores chegam ~2.8 s depois.

Para saber se a fila está andando:

```
brain_status.embedding.coverage.embedding_coverage_pct
```

Esse número **é** o sinal de fila travada. Chunk esperando vetor conta como
`without_embedding`, **nunca** como `zero_vector` — os dois são estados
diferentes e a diferença é o diagnóstico:

| Sinal | Significado | O que fazer |
|---|---|---|
| `coverage_pct` < 100, `without_embedding` > 0, `queue.pending_len` caindo | fila working | esperar |
| `coverage_pct` < 100, `pending_len` parado, `ready_len` > 0 | fila em backoff | esperar o backoff |
| `queue.dead_lettered` > 0 | a fila **desistiu** | `brain reindex --all`, ou o `recover` do próximo boot |
| `chunks_zero_vector` > 0 | índice corrompido (BLOB de zeros) | `brain reindex --all` |

`dead_lettered` **não é perda de dado**: o chunk fica `NULL`, e `NULL` é
exatamente o registro que o `recover` do próximo boot e o `reindex --all` leem.

O bloco `queue` completo está em `tools/brain_status/SKILL.md`.

## Limites de escrita

Rejeitados **antes** de qualquer embed, com o nome do limite no erro:

| Limite | Valor | Exceder |
|--------|-------|---------|
| `MAX_CONTENT_BYTES` | 256 KiB | `INVALID_PARAMS: content too large` |
| `MAX_CHUNKS` | 64 (uma seção `## ` = 1 chunk) | `INVALID_PARAMS: content splits into N chunks` |

Embed é serial (o Ollama serve um por vez, ~0.045 s/chunk), então o custo da
escrita escalava com o número de chunks sem teto. Se bater, divida a nota em
várias sob o mesmo projeto.

## Variáveis de ambiente

As que mudam o comportamento de um agente. Todas com prefixo `BRAIN_`.

| Variável | Default | O que faz |
|----------|---------|-----------|
| `BRAIN_DB_PATH` | `./data/brain.db` | Qual SQLite abrir |
| `BRAIN_OLLAMA_URL` | `http://localhost:11434` | Onde está o Ollama. **Com Ollama fora a busca não quebra** — degrada para só texto |
| `BRAIN_OLLAMA_MODEL` | `nomic-embed-text` | Modelo de embedding (768 dim) |
| `BRAIN_EXPORT_ROOT` | `/tmp/brain-export` | **Allowlist de escrita** de `brain_export` e `brain_backup`. Qualquer destino fora é recusado |
| `BRAIN_EMBED_MAX_FAILURES` | `8` | Tentativas de embed antes do dead-letter (≈1 min de backoff) |
| `BRAIN_REUSE_SIMILARITY` | `1.0` | Abaixo de 1.0 um chunk pode herdar o vetor do texto antigo. **Não mexa** sem ler `brain_reindex` |
| `BRAIN_EMBED_TIMEOUT_SECS` | `60` | Budget **base** por onda de embed |
| `BRAIN_PORT` | `8321` | Porta do MCP SSE |
| `BRAIN_TRANSPORT` | — | `stdio` em vez de SSE |
| `BRAIN_HOOK_EMBED` | `1` | `0` desliga o embed no `brain hook` |
| `BRAIN_VAULT_PATH` | — | ⚠️ **IGNORADA.** O campo foi removido do modelo; a variável não é lida |

`BRAIN_VIEWER_PORT` e `BRAIN_LOG_LEVEL` aparecem em `CONFIGURE_MCP.md` e no
`README`, mas são lidas **só pelo Python legado** — o binário Rust usa default
fixo (`brain-cli:98`). Não espere que elas mudem o servidor Rust.

## Boas práticas para agentes

### ⚡ Seja PROATIVO — não espere o usuário pedir

Sempre que você aprender algo durante o desenvolvimento — um conceito novo, uma tecnologia, um padrão, uma solução — **salve automaticamente** no brain com conteúdo completo.

Não espere o usuário dizer "salve isso" ou "estude aquilo". Se você:
- Leu a documentação de uma tecnologia nova → salve o estudo
- Entendeu um conceito que não conhecia → salve o estudo
- Descobriu uma solução para um problema → salve o estudo
- Aprendeu um padrão ou workflow → salve o estudo

**Regra:** Toda descoberta significativa vira um estudo completo em `estudos/`.

---

### 📚 Metodologia de Aprendizado Ativo

Quando um tópico novo aparece (tecnologia, conceito, padrão), siga este ciclo:

```
1. IDENTIFICAR → percebeu que precisa aprender algo novo
2. PESQUISAR  → busca fontes, docs, exemplos
3. SINTETIZAR → organiza o conhecimento em estrutura clara
4. SALVAR     → brain_store com CONTEÚDO COMPLETO em estudos/
5. ANALISAR   → avalia: está completo? rico? útil?
6. ITERAR     → se fraco, aprofunda e salva versão melhorada
```

**Nunca pule o passo 5 (ANALISAR).** É ele que garante qualidade.

---

### 🔄 Loop de Qualidade: Análise do Estudo

**Após salvar um estudo, SEMPRE faça esta auto-análise:**

```python
# Pergunte-se após salvar cada estudo:
analise = {
    "tem_exemplos_codigo": True/False,    # ❌ se não tem, é resumo
    "tem_conceitos_detalhados": True/False,
    "tem_referencias": True/False,
    "tem_explicacao_profunda": True/False, # não só "o que é", mas "como funciona"
    "cobre_casos_reais": True/False,       # uso prático, não só teoria
    "tamanho_util": len(conteudo) > 500,   # 500 chars mínimo para ser útil
}

if not all(analise.values()):
    # ❌ Estudo fraco! Precisa aprofundar antes de considerar pronto
    print(f"Estudo incompleto. Faltam: {[k for k,v in analise.items() if not v]}")
    print("Aprofunde e salve versão melhorada.")
```

**Critérios de aprovação:** O estudo só é considerado pronto quando TODOS os itens são `True`.

| Se o estudo... | Ação |
|----------------|------|
| Tem só definições rasas | ❌ Reprovado — adicione exemplos práticos |
| Tem teoria mas sem código | ❌ Reprovado — adicione código real |
| Tem exemplos mas sem contexto | ❌ Reprovado — explique por que funciona |
| Tem tudo acima | ✅ Aprovado — salvo com sucesso |

---

### 📋 Template de Estudo Completo

Este é o formato OBRIGATÓRIO para todo estudo salvo no brain:

````
---
tags: [java, orientacao-objetos, fundamentos]
topico: Java OO
nivel: iniciante
fonte: documentação oficial, curso alura
---

# Java: Orientação a Objetos — Estudo Completo

## Resumo
(2-3 frases sobre o que é o tópico e por que é importante)

## Conceitos Aprendidos

### 1. (Conceito Principal)
- O que é: explicação clara e direta
- Como funciona: detalhamento técnico
- Por que é importante: contexto de uso

```java
// Exemplo prático que ilustra o conceito
public class Exemplo {
    // código real, não pseudo-código
}
```

### 2. (Próximo Conceito)
(mesma estrutura)

## Comparação com Alternativas (se aplicável)
| Abordagem | Prós | Contras |
|-----------|------|---------|
| Esta | ... | ... |
| Alternativa | ... | ... |

## Exemplos Práticos
(Exemplos completos e funcionais, não snippets isolados)

## Armadilhas Comuns
- Erro comum #1: ... → Como evitar
- Erro comum #2: ... → Como evitar

## Referências
- [Documentação oficial](url)
- [Tutorial recomendado](url)
- [Fonte original do estudo](url)
````

**Cada seção deve ter conteúdo substancial.** Não escreva "aprendi X" — escreva o que é X, como funciona, exemplos, por que é útil.

---

### 🏷️ Tags e Metadados para Busca

Sempre inclua **frontmatter YAML** no topo do estudo. Isso melhora drasticamente a busca semântica:

```yaml
---
tags: [tag1, tag2, tag3]     # OBRIGATÓRIO — categorias do estudo
topico: Nome do Tópico        # Recomendado — nome legível
nivel: iniciante              # Opcional — iniciante|intermediario|avancado
fonte: onde aprendeu          # Recomendado — documentação, curso, artigo
---
```

**Tags sugeridas:**

| Tag | Uso | Exemplo |
|-----|-----|---------|
| `fundamentos` | Conceitos base | java-fundamentos, sql-basico |
| `avancado` | Tópicos complexos | multithreading, otimizacao |
| `framework` | Frameworks | spring-boot, react, vue |
| `ferramenta` | Ferramentas | docker, git, kubernetes |
| `padrao` | Design patterns | strategy, observer, mvc |
| `pratica` | Exemplos práticos | crud-java, api-rest |
| `comparacao` | Comparações | nosql-vs-sql, rest-vs-graphql |
| `solucao` | Solução de problema | deploy-automatizado, CI-CD |
| `arquitetura` | Arquitetura de software | microservicos, event-driven |

---

### 🎯 Gatilhos Proativos — Quando Salvar

**Salve um estudo completo automaticamente quando:**

| Gatilho | Exemplo | Layer | Scope |
|---------|---------|-------|-------|
| Aprendeu tecnologia nova | "Nunca usei Docker, aprendi agora" | `estudos` | `global` |
| Entendeu conceito do projeto | "Como o módulo X funciona internamente" | `estudos` | `projetos` |
| Resolveu problema complexo | "Debug de memory leak em produção" | `estudos` | `projetos` |
| Descobriu padrão reutilizável | "Pattern para retry com backoff" | `estudos` | `global` |
| Leu documentação relevante | "Li spec do HTTP/3" | `estudos` | `global` |
| Comparou tecnologias | "Diferenças entre SQL e NoSQL" | `estudos` | `global` |

**Não salve apenas um resumo.** Salve o conteúdo completo que você estudou/gerou. O resumo vai em `sessoes` para handoff. O conhecimento completo vai em `estudos`.

---

### 🚫 O Que NÃO Fazer

| ❌ Errado | ✅ Certo |
|-----------|----------|
| `brain_store("sessoes", "2026-07-25", "## Estudei Java OO")` | `brain_store("estudos", "java/oo", "---\ntags: [java]\n---\n# Java OO\n\nConteúdo completo...", scope="global")` |
| Salvar tutorial inteiro sem refinar | Sintetizar com suas palavras + exemplos |
| Salvar só links | Salvar o conhecimento, não apenas referências |
| Criar estudo sem tags | Sempre incluir frontmatter com tags |
| Ignorar estudo raso ("já entendi") | Rodar o loop de qualidade e aprofundar |

---

### ⏱️ Ciclo de Vida do Conhecimento

```
APRENDEU → SALVA ESTUDO COMPLETO → ANALISA QUALIDADE
                                         ↓
                              ┌──── APROVADO? ────┐
                              ↓                    ↓
                           ✅ Pronto            ❌ Muito raso
                              ↓                    ↓
                        Disponível para        Aprofunda + salva
                        busca semântica        versão melhorada
```

**Sempre que recuperar um estudo do brain (`brain_search`) e perceber que está desatualizado ou incompleto → atualize-o com `brain_store` (sobrescreve).**

---

### Exemplo Completo: Ciclo Proativo

```python
# 1. Durante o desenvolvimento, você encontra um conceito novo
#    (ex: "Nunca usei async/await em Python, preciso estudar")

# 2. Você pesquisa e entende o conceito

# 3. Salva o ESTUDO COMPLETO (não resumo)
brain_store(
    layer="estudos",
    path="python/async-await",
    content="""---
tags: [python, async, fundamentos]
topico: Async/Await em Python
nivel: intermediario
fonte: documentação oficial Python, Real Python
---

# Async/Await em Python — Estudo Completo

## Resumo
Async/await permite concorrência em Python usando event loop.
Diferente de threading, roda em uma única thread com switching cooperativo.

## Conceitos

### 1. Event Loop
Gerencia e distribui tarefas assíncronas. `asyncio.run()` cria um.

### 2. Corrotinas
Funções declaradas com `async def`. Só executam quando `await` é chamado.

```python
async def fetch_data(url):
    async with aiohttp.ClientSession() as session:
        async with session.get(url) as response:
            return await response.json()
```

### 3. Tasks vs Corrotinas
- Corrotina: função que pode ser pausada
- Task: corrotina agendada no event loop

## Exemplos Práticos
... (conteúdo completo) ...

## Armadilhas
- Esquecer `await` dentro de async function
- Bloquear event loop com chamadas síncronas

## Referências
- https://docs.python.org/3/library/asyncio.html
""",
    scope="global"  # conhecimento universal sobre Python
)

# 4. AUTO-ANÁLISE DE QUALIDADE
#    Pergunta: "Esse estudo tem exemplos de código? Sim."
#    Pergunta: "Tem explicação profunda? Sim."
#    Pergunta: "Tem referências? Sim."
#    → ✅ APROVADO

# 5. Se estivesse fraco, NÃO aceitaria. Aprofundaria e salvaria de novo.

# ❌ ERRADO (resumo perdido):
# brain_store("sessoes", "projeto/2026-07-25", "## Aprendi async/await hoje")
```

---

### Escopo: Projeto vs Global

As camadas `arquitetura`, `regras` e `estudos` usam **scope** para separar conteúdo:

- **`scope="projetos"`**: Conhecimento específico do projeto atual
  - Ex: "Arquitetura de módulos do sistema X"
  - Ex: "Regras de negócio do módulo de pagamento"
  - Ex estudo: "Análise do banco de dados do projeto X"

- **`scope="global"`**: Conhecimento universal compartilhado entre todos os projetos
  - Ex: "Padrões de código limpo"
  - Ex: "Nunca usar SELECT * em produção"
  - Ex estudo: "Java Orientação a Objetos"

**Regra de ouro**: Antes de salvar em `global`, pergunte: "Esse conhecimento se aplica a TODOS os projetos ou só ao meu?"

---

### Antes de codificar
Sempre consulte o cérebro para contexto relevante:

```
brain_search("arquitetura", layer="arquitetura", scope="projetos")
brain_search("<feature-name>", layer="regras", scope="projetos")
brain_search("padrões de código", scope="global")  # lições universais
brain_search("async", layer="estudos", scope="global")  # estudos salvos antes
```

### Depois de decisões
Registre decisões arquiteturais no cérebro:

```
brain_store("arquitetura", "<projeto>/<decisao>", "# Decisão...", scope="projetos")
brain_store("regras", "coding-standards", "## Padrões...", scope="global")  # se universal
```

### Handoff de sessão
Antes de perder contexto, salve um resumo da sessão:

```
brain_store("sessoes", "<projeto>/<data>", "## Sessão...")
```

**Nota:** Resumo de sessão em `sessoes` é para handoff entre agentes. Estudo completo em `estudos` é para aprendizado permanente. São coisas diferentes.

### Organização
Use as camadas e scopes corretos:
- `arquitetura/projetos/` — estrutura específica do projeto
- `arquitetura/global/` — padrões universais de arquitetura
- `regras/projetos/` — regras de negócio específicas
- `regras/global/` — lições aprendidas, padrões de código
- `estudos/projetos/` — estudos completos específicos do projeto
- `estudos/global/` — estudos completos de conhecimento geral
- `sessoes/` — resumos de sessão (já é por projeto)
- `projetos/` — metadados e contexto de cada projeto
