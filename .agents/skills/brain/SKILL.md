---
name: brain
description: >
  Connect to the Brain MCP server — 17 tools: hybrid semantic search, note
  storage with a background embedding queue, projects, audit/restore, TTL.
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
cd <caminho-para-brain>
uv run python -m brain_server
```

O servidor escuta em `http://localhost:8321` (SSE transport).

## Como usar

No `opencode.json` do seu projeto, referencie esta skill:

```json
{
  "instructions": [
    "../brain/.agents/skills/brain/SKILL.md"
  ]
}
```

## Tools expostas

### `brain_search(query, [layer], [scope], [top_k])`

Busca semântica por similaridade de embedding no vault do cérebro.

```python
# Exemplo: buscar regras de banco de dados
brain_search("regras de banco de dados")
# Resultados ordenados por score (0..1)

# Filtrar por camada
brain_search("naming conventions", layer="regras")

# Filtrar por scope (projetos ou global)
brain_search("padrões de código", scope="global")
brain_search("regras do meu projeto", scope="projetos")

# Combinar layer e scope
brain_search("arquitetura do sistema", layer="arquitetura", scope="global")

# Controlar quantidade de resultados
brain_search("arquitetura do sistema", top_k=3)
```

**Parâmetros:**
| Nome | Tipo | Obrigatório | Default | Descrição |
|------|------|-------------|---------|-----------|
| `query` | string | ✅ | — | Texto da busca |
| `layer` | string | ❌ | `null` | Filtrar por camada |
| `scope` | string | ❌ | `null` | Filtrar por scope (`projetos` ou `global`) |
| `top_k` | integer | ❌ | `5` | Máx resultados (1–20) |

### `brain_store(layer, path, content, [scope])`

Salva uma nota markdown no vault. O conteúdo é automaticamente indexado para busca semântica.

```python
# Salvar decisão arquitetural (scope obrigatório para arquitetura/regras)
brain_store(
    layer="arquitetura",
    path="meu-projeto/decisao-db",
    content="# Decisão: PostgreSQL\n\n## Contexto\nPrecisamos de um banco relacional...",
    scope="projetos"  # específico do projeto
)

# Salvar regra de negócio (scope obrigatório)
brain_store(
    layer="regras",
    path="meu-projeto/naming-conventions",
    content="## Nomes de tabela\nTabelas em snake_case plural...",
    scope="projetos"  # específico do projeto
)

# Salvar lição global (compartilhada entre todos os projetos)
brain_store(
    layer="regras",
    path="coding-standards",
    content="## Padrões universais\n\n- Nunca usar SELECT *\n- Sempre validar input...",
    scope="global"  # compartilhado
)

# Salvar sessão (scope NÃO necessário para sessoes/projetos)
brain_store(
    layer="sessoes",
    path="meu-projeto/2026-07-25",
    content="## Sessão\n\nTrabalhei em..."
)
```

**Camadas válidas:** `arquitetura`, `regras`, `sessoes`, `projetos`, `estudos`, `indexacao`

**Scope obrigatório:** `arquitetura` e `regras` e `estudos` exigem `scope="projetos"` ou `scope="global"`

**Retorno:** `{ok, path, chunks, embedded, without_embedding, queued}`

| Campo | Significado |
|-------|-------------|
| `chunks` | chunks que a nota gerou |
| `embedded` | chunks **já** com vetor (0 numa nota nova) |
| `without_embedding` | chunks `NULL`, aguardando vetor |
| `queued` | dívida de embedding enfileirada para background |

O vetor **não** é síncrono (US-02.7): a nota e seu índice FTS5 estão gravados
quando a chamada retorna, e os vetores chegam em background. Se a busca
semântica não achar a nota de imediato, isso é o motivo — releia
`brain_status` e veja `embedding.coverage.embedding_coverage_pct`.

**Limites de escrita** (rejeição *antes* de qualquer embed, erro nomeando o
limite):

| Limite | Valor | Exceder |
|--------|-------|---------|
| `MAX_CONTENT_BYTES` | 256 KiB | `INVALID_PARAMS: content too large` |
| `MAX_CHUNKS` | 64 (uma seção `## ` = 1 chunk) | `INVALID_PARAMS: content splits into N chunks` |

Um chunk custa ~0.045 s para embedar e o Ollama serve **um por vez**, então os
limites existem para que uma nota não vire trabalho de minutos. Se bater, divida
em várias notas sob o mesmo projeto ou una as seções pequenas.

### `brain_read(layer, path, [scope])`

Lê uma nota completa do vault.

```python
# Ler nota salva anteriormente (scope obrigatório para arquitetura/regras)
brain_read("regras", "meu-projeto/naming-conventions", scope="projetos")
# Retorna o conteúdo markdown completo com metadados

# Ler nota global
brain_read("regras", "coding-standards", scope="global")

# Ler sessão (scope não necessário)
brain_read("sessoes", "meu-projeto/2026-07-25")
```

### `brain_status()`

Contagens + a saúde do índice de embeddings.

```json
{
  "notes": 253, "chunks": 821, "projects": 6,
  "embedding": {
    "coverage": {
      "chunks_total": 821, "chunks_embedded": 821,
      "chunks_without_embedding": 0, "chunks_zero_vector": 0,
      "embedding_coverage_pct": 100.0
    },
    "ollama": { "reachable": true, "model": "nomic-embed-text" }
  }
}
```

`embedding_coverage_pct` é **o sinal de fila travada**: chunk esperando vetor conta
como `without_embedding`, nunca como `zero_vector`. Um `zero_vector > 0` é um
estado corrompido herdado (BLOB de zeros que scoreia 0.0 para toda query) e
precisa de `brain reindex --all`.

### Reindex — `brain reindex --all [--no-embed]` (CLI, não é tool MCP)

Reconstroi chunks + embeddings. **Roda em foreground** (a passagem de embed de um
corpo grande leva minutos) e é **não destrutivo**: nunca `DELETE FROM chunks`, e
reaproveita o vetor de todo chunk cujo texto continua igual.

```bash
brain reindex --all              # reindexa + embeda o que falta
brain reindex --all --no-embed   # só a parte estrutural; novos chunks ficam NULL
```

```
REINDEX_DONE notes=253 chunks=821 embedded=821 preserved=814 rehydrated=7              null=0 diverged=0 stale_reused=0 unmatched=0
```

| Token | Significado |
|-------|-------------|
| `preserved` | vetores que sobreviveram do run anterior |
| `rehydrated` | vetores recuperados nesta passagem |
| `null` | chunks sem vetor — **precisa de outro run** |
| `diverged` | vetor calculado de texto que a nota já não tem; **não** foi aplicado (nota editada durante o run). Sai `REINDEX_DIVERGED` |
| `stale_reused` | vetores mantidos por similaridade. **Default `1.0` (exato)**: só aparece com opt-in `BRAIN_REUSE_SIMILARITY=0.9`. A guarda de força normativa (obrigação ↔ proibição) bloqueia o reuso **independentemente** desse threshold |
| `unmatched` | vetores perdidos: o texto mudou demais para reaproveitar |

Linhas extras: `REINDEX_PARTIAL` (há `null > 0`) e `REINDEX_DIVERGED` (há
`diverged > 0`). Se outro embed segura o lock (a fila do servidor), o comando
falha sem escrever nada — é para não duplicar trabalho.

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

As camadas `arquitetura` e `regras` e `estudos` usam **scope** para separar conteúdo:

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
