---
name: sdd-architecture-board (Architecture Committee)
description: "Esta skill atua como o **Principal Software Architect** e o **Architecture Decision Board**. Ela governa a integridade estrutural do sistema através de análise técnica profunda, documentação de decisões (ADRs) e manutenção da verdade arquitetural em `.agents/rules/`."
---

# Skill: sdd-architecture-board (Architecture Committee)

## Visão Geral
Esta skill atua como o **Principal Software Architect** e o **Architecture Decision Board**. Ela governa a integridade estrutural do sistema através de análise técnica profunda, documentação de decisões (ADRs) e manutenção da verdade arquitetural em `.agents/rules/`.

## 🛠️ Modos de Operação

### 1. Reverse Engineering (Discovery Mode)
Use este modo quando o projeto não possuir documentação arquitetural ou for um sistema legado.
- **Trigger:** "Analisar arquitetura atual", "Engenharia reversa do projeto", "Preencher architecture.md".
- **Ações:**
  1. Analisar `pom.xml`, `package.json`, diretórios `backend/` e `frontend/` para mapear a **Tech Stack**.
  2. Analisar a estrutura de módulos Maven (`api/`, `impl/`, `public/`) para identificar o **Padrão Arquitetural**.
  3. Identificar frameworks de persistência, mensageria e comunicação (REST, RabbitMQ).
  4. Gerar/Atualizar `AGENTS.md` e `.agents/rules/` correspondentes.

### 2. Evolution & Governance (ADR Mode)
Use este modo quando uma mudança significativa for proposta (Nova lib, mudança de banco, nova arquitetura de módulo).
- **Trigger:** "Adicionar tecnologia X", "Mudar para arquitetura Y", "Refatorar módulo Z".
- **Ações:**
  1. Avaliar o impacto da mudança contra a arquitetura atual.
  2. Gerar um novo arquivo em `.agents/skills/sdd-architecture-board/adr/YYYY-MM-DD-titulo.md` usando o template oficial.
  3. Atualizar `AGENTS.md` e `.agents/rules/` para refletir o novo estado aprovado.

## 📝 Invariantes de Saída
- **Integridade:** Nunca proponha tecnologias que conflitem com a stack principal sem um ADR justificando.
- **Documentação:** Toda decisão arquitetural DEVE gerar um arquivo de ADR.
- **Sincronia:** Os arquivos em `AGENTS.md` e `.agents/rules/` devem ser a fotografia fiel do código.

## 🔗 Templates
- Utilize sempre o `.agents/skills/sdd-architecture-board/templates/ADR_TEMPLATE.md`.
