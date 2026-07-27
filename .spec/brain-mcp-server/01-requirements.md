# Phase 1 — Requirements

## Brain MCP Server — Cérebro para Agentes de IA

---

## Visão Geral

Um servidor MCP (Model Context Protocol) em Python que atua como "cérebro" central para agentes de IA. O sistema persiste informações em um vault Obsidian, indexa conteúdo via embeddings locais (Ollama), e permite busca semântica, leitura e escrita de notas. Outros projetos consomem o cérebro através de uma skill OpenCode (`/brain`).

## Atores

| Ator | Descrição |
|------|-----------|
| **Agente consumidor** | Qualquer repositório OpenCode externo que carrega a skill `/brain` e se conecta ao MCP server |
| **Operador** | Desenvolvedor humano que administra o vault, reindexa dados, verifica健康状况 |

---

## User Stories

### US-01 — Inicialização do servidor
**Como** operador  
**Quero** iniciar o servidor MCP com um único comando  
**Para** que agentes consumidores possam se conectar imediatamente

#### Acceptance Criteria (EARS)
1. WHEN operator runs `brain-server` THEN system SHALL start MCP server on configured port
2. WHEN server starts THEN system SHALL validate connection to Obsidian vault directory
3. WHEN server starts THEN system SHALL validate connection to Ollama endpoint
4. WHEN server starts AND vault is unreachable THEN system SHALL log error and exit with code 1
5. WHEN server starts AND Ollama is unreachable THEN system SHALL start in degraded mode with warning log

### US-02 — Armazenar nota no vault
**Como** agente consumidor  
**Quero** salvar uma nota markdown em uma camada específica do vault  
**Para** que informações estruturadas (arquitetura, regras, sessões, projetos) sejam persistidas

#### Acceptance Criteria (EARS)
1. WHEN brain_store(layer, path, content) is called THEN system SHALL write markdown file to `<vault>/<layer>/<path>.md`
2. WHEN brain_store is called AND layer is invalid THEN system SHALL return error listing valid layers
3. WHEN brain_store is called AND file already exists THEN system SHALL overwrite with new content
4. WHEN brain_store succeeds THEN system SHALL trigger async reindex of the affected file
5. WHEN brain_store writes to a layer THEN system SHALL ensure the layer subdirectory exists

**Valid layers:** `arquitetura`, `regras`, `sessoes`, `projetos`, `indexacao`

### US-03 — Ler nota do vault
**Como** agente consumidor  
**Quero** ler uma nota markdown do vault informando camada e path  
**Para** recuperar informações previamente salvas

#### Acceptance Criteria (EARS)
1. WHEN brain_read(layer, path) is called AND file exists THEN system SHALL return full markdown content
2. WHEN brain_read is called AND file does not exist THEN system SHALL return "not found" error
3. WHEN brain_read is called AND layer is invalid THEN system SHALL return error listing valid layers

### US-04 — Busca semântica no vault
**Como** agente consumidor  
**Quero** buscar notas por similaridade semântica (embedding)  
**Para** encontrar informações relevantes mesmo sem saber o path exato

#### Acceptance Criteria (EARS)
1. WHEN brain_search(query) is called THEN system SHALL return list of results with: path, layer, score, snippet
2. WHEN brain_search is called AND no results meet threshold THEN system SHALL return empty list
3. WHEN brain_search is called THEN system SHALL rank results by cosine similarity score descending
4. WHEN brain_search is called WITH filter_layer THEN system SHALL restrict search to that layer only
5. WHEN brain_search is called WITH top_k parameter THEN system SHALL return at most top_k results (default 5, max 20)
6. WHEN brain_search is called AND index is empty THEN system SHALL return empty list with warning

### US-05 — Indexação de embeddings
**Como** operador  
**Quero** que o sistema construa e mantenha índices de embeddings para todas as notas  
**Para** que a busca semântica funcione com baixa latência

#### Acceptance Criteria (EARS)
1. WHEN server starts THEN system SHALL load existing index from disk
2. WHEN a note is stored or updated THEN system SHALL recompute its embedding and update index
3. WHEN index is updated THEN system SHALL persist index to disk
4. WHEN embedding computation fails (Ollama unavailable) THEN system SHALL queue the file for later indexing
5. WHEN index file does not exist on startup THEN system SHALL rebuild full index from all vault files

### US-06 — Reindexação manual/trigger
**Como** operador  
**Quero** disparar uma reindexação parcial ou total  
**Para** recuperar de falhas ou atualizar o índice após alterações manuais no vault

#### Acceptance Criteria (EARS)
1. WHEN brain_reindex(all=true) is called THEN system SHALL recompute embeddings for every file in vault
2. WHEN brain_reindex(layer="regras") is called THEN system SHALL recompute embeddings only for that layer
3. WHEN brain_reindex(path="projetos/meu-projeto") is called THEN system SHALL recompute embedding for that single file
4. WHEN reindex is in progress AND another reindex is requested THEN system SHALL ignore the duplicate request
5. WHEN reindex completes THEN system SHALL persist updated index to disk

### US-07 — /brain skill para projetos consumidores
**Como** desenvolvedor de outro projeto OpenCode  
**Quero** carregar a skill `/brain` no meu repositório  
**Para** que os agentes do meu projeto possam consultar e escrever no cérebro central

#### Acceptance Criteria (EARS)
1. WHEN a project loads `/brain` skill THEN system SHALL expose tools: brain_search, brain_store, brain_read
2. WHEN skill is loaded AND MCP server is unreachable THEN system SHALL show clear error message with connection instructions
3. WHEN skill is loaded THEN system SHALL use MCP transport configured via env var `BRAIN_MCP_URL` (default: `http://localhost:8321`)

### US-08 — Organização por camadas
**Como** operador  
**Quero** que as notas sejam organizadas em camelas predefinidas dentro do vault  
**Para** manter a estrutura consistente entre projetos e facilitar a navegação manual no Obsidian

#### Acceptance Criteria (EARS)
1. WHEN vault is initialized THEN system SHALL create subdirectories: `arquitetura/`, `regras/`, `sessoes/`, `projetos/`, `indexacao/`
2. WHEN brain_store receives unknown layer THEN system SHALL reject with error listing valid layers
3. WHEN system is idle THEN it SHALL NOT create, modify, or delete any files outside the vault directory

---

## Casos de Borda (Edge Cases)

| # | Cenário | Comportamento Esperado |
|---|---------|----------------------|
| EC-01 | Vault path configurado não existe | Server cria o vault com camadas vazias no primeiro start |
| EC-02 | Ollama retorna timeout | Embedding é marcado como pendente; retry na próxima operação ou reindex |
| EC-03 | Arquivo markdown mal formatado | Embedding é gerado sobre o texto bruto (sem parsing de frontmatter) |
| EC-04 | Múltiplos agentes chamam `brain_store` concorrentemente | Escrita sequencial (locking por arquivo) — sem condição de corrida |
| EC-05 | Vault é modificado manualmente via Obsidian | Index só reflete alterações após reindex trigger |
| EC-06 | Query de busca vazia | Retorna erro "query cannot be empty" |
| EC-07 | Arquivo muito grande para embedding | Trunca para N tokens antes de gerar embedding (N configurável, default 4096) |

---

## Constraints

| # | Constraint | Detalhes |
|---|-----------|----------|
| C-01 | Embeddings locais | Ollama obrigatório. Nenhum serviço cloud de embeddings |
| C-02 | Vault Obsidian | Diretório local com arquivos `.md`. Sem dependência do app Obsidian |
| C-03 | Python >= 3.11 | Versão mínima para recursos de tipagem e async |
| C-04 | MCP protocol | Usar `mcp` PyPI package (protocolo oficial) |
| C-05 | Sem banco externo | Toda persistência é no sistema de arquivos (markdown + JSON index) |
| C-06 | Skill consumidora | A skill `/brain` deve ser auto-contida em diretório `.agents/skills/brain/` |

---

## Questions em Aberto

1. Modelo Ollama padrão? Sugestão inicial: `nomic-embed-text` (leve, bom para searches). Confirmar.
2. Porta padrão do MCP server? Sugestão: `8321`. Confirmar.
3. Estratégia de chunking para notas longas? Sugestão: split por seções (##), embedding por seção.
