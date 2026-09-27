# Templates de Perguntas — Dev Planner

Use estas perguntas em cada fase para guiar o usuário. Adapte conforme o contexto, mas **nunca pule as perguntas obrigatórias** (marcadas com ⚠️).

---

## FASE 0: Descubra o Problema Real

### ⚠️ Obrigatórias

| # | Pergunta | Propósito |
|---|----------|-----------|
| 0.1 | **"Qual é o PROBLEMA que você está resolvendo? Não me diga a solução — me diga o problema."** | Diferenciar sintoma de causa raiz |
| 0.2 | **"Quem são os USUÁRIOS que vão usar isso? (ex: vendedores, gerentes, clientes)"** | Identificar personas |
| 0.3 | **"Como é feito HOJE esse processo? (manual, planilha, outro sistema)"** | Entender estado atual |
| 0.4 | **"Quais são os 3 CASOS DE USO principais?"** | Definir escopo mínimo |
| 0.5 | **"Existe ALGO que NÃO pode mudar? (ex: integração com sistema X, formato de relatório)"** | Identificar restrições rígidas |

### 🟡 Complementares (use conforme necessário)

| # | Pergunta | Quando usar |
|---|----------|-------------|
| 0.6 | "Qual é o VOLUME esperado? (ex: 100 pedidos/dia, 10.000 clientes)" | Quando performance é crítica |
| 0.7 | "Existe PRAZO? (ex: precisa estar pronto para o lançamento em março)" | Quando há urgência |
| 0.8 | "Quem é o DECISOR final? (quem aprova a feature)" | Quando há múltiplos stakeholders |
| 0.9 | "Existe ORÇAMENTO definido?" | Quando custo é fator |
| 0.10 | "Isso é uma NOVA feature ou uma CORREÇÃO?" | Quando há ambiguidade |

### 🔵 Para Clarificação Adicional

| # | Pergunta | Quando usar |
|---|----------|-------------|
| 0.11 | "Você mencionou [X]. Pode detalhar como isso funcionaria?" | Quando a descrição é vaga |
| 0.12 | "O que acontece se isso NÃO for implementado?" | Para validar prioridade |
| 0.13 | "Existe algum SISTEMA que já faz isso parcialmente?" | Para evitar reinventar a roda |
| 0.14 | "Quais são os DORÇOS dos usuários com o processo atual?" | Para identificar valor da feature |

---

## FASE 1: Analise Restrições e Contexto

### ⚠️ Obrigatórias

| # | Pergunta | Propósito |
|---|----------|-----------|
| 1.1 | **"Esta feature impacta OUTROS módulos do sistema?"** | Mapear dependências |
| 1.2 | **"Precisa de integração com sistemas EXTERNOS? Se sim, quais?"** | Identificar integrações |
| 1.3 | **"Os dados são SENSÍVEIS? (ex: dados bancários, saúde, criança)"** | Definir requisitos de segurança |
| 1.4 | **"Precisa de MULTI-TENANT? (mesmo dado isolado por empresa)"** | Definir isolamento |
| 1.5 | **"Existe requisito de PERFORMANCE? (ex: resposta em <200ms)"** | Definir SLA |

### 🟡 Complementares

| # | Pergunta | Quando usar |
|---|----------|-------------|
| 1.6 | "Precisa de AUDITORIA? (quem fez, quando, o que mudou)" | Quando dados são críticos |
| 1.7 | "Existe requisito de DISPONIBILIDADE? (ex: 99.9% uptime)" | Para sistemas críticos |
| 1.8 | "Precisa de EXPORTAÇÃO? (PDF, Excel, CSV)" | Quando há relatórios |
| 1.9 | "Existe integração com ERPs externos? (SAP, TOTVS, Bling)" | Para módulos financeiros |
| 1.10 | "Precisa de NOTIFICAÇÕES? (email, SMS, push)" | Para fluxos assíncronos |

### 🔵 Para Contexto Adicional

| # | Pergunta | Quando usar |
|---|----------|-------------|
| 1.11 | "Quais são os LIMITES do banco atual? (ex: espaço, performance)" | Para infraestrutura |
| 1.12 | "Existe-time disponível para desenvolvimento?" | Para estimativas |
| 1.13 | "Precisa de HOMOLOGAÇÃO com cliente antes de release?" | Para validação |
| 1.14 | "Existe requisito de MIGRAÇÃO de dados?" | Quando há sistema legado |

---

## FASE 2: Desafie a Solução

### ⚠️ Obrigatórias

| # | Pergunta | Propósito |
|---|----------|-----------|
| 2.1 | **"Por que você escolheu essa abordagem? O que a torna melhor?"** | Questionar pressupostos |
| 2.2 | **"Você considerou [alternativa]? Por que descartou?"** | Explorar alternativas |
| 2.3 | **"Quais são os RISCOS que você vê nessa abordagem?"** | Identificar riscos |
| 2.4 | **"O que acontece se [cenário de falha]?"** | Testar resiliência |
| 2.5 | **"Isso está ALINHADO com a stack do projeto? (Java EE 7, EJB, JSF)"** | Validar compatibilidade |

### 🟡 Complementares

| # | Pergunta | Quando usar |
|---|----------|-------------|
| 2.6 | "Qual é o CUSTO de manutenção dessa solução?" | Para sustentabilidade |
| 2.7 | "Isso pode ser feito de forma MAIS SIMPLES?" | Para reduzir complexidade |
| 2.8 | "Existe alguma LIB open-source que resolve isso?" | Para evitar código desnecessário |
| 2.9 | "Isso escala para [volume futuro]?" | Para crescimento |
| 2.10 | "Qual é o IMPACTO na experiência do usuário?" | Para UX |

### 🔵 Para Desafio Profundo

| # | Pergunta | Quando usar |
|---|----------|-------------|
| 2.11 | "Se fosse fazer do zero, como faria?" | Para questionar legado |
| 2.12 | "O que um arquiteto sênior diria sobre essa abordagem?" | Para validação externa |
| 2.13 | "Existe case de SUCESSO usando isso?" | Para fundamentar |
| 2.14 | "Existe case de FALHA usando isso?" | Para evitar armadilhas |
| 2.15 | "Isso viola algum princípio SOLID?" | Para qualidade |

---

## FASE 3: Projete a Arquitetura

### ⚠️ Obrigatórias

| # | Pergunta | Propósito |
|---|----------|-----------|
| 3.1 | **"Quais são as ENTIDADES principais? (ex: Pedido, Cliente, Produto)"** | Modelar dados |
| 3.2 | **"Quais campos são OBRIGATÓRIOS?"** | Definir validações |
| 3.3 | **"Quais campos podem ser NULOS?"** | Definir schema |
| 3.4 | **"Quais OPERAÇÕES CRUD são necessárias?"** | Definir API |
| 3.5 | **"Quem pode ACESSAR cada operação? (perfis, roles)"** | Definir autorização |

### 🟡 Complementares

| # | Pergunta | Quando usar |
|---|----------|-------------|
| 3.6 | "Precisa de BUSCA AVANÇADA? Quais filtros?" | Para listagens |
| 3.7 | "Precisa de PAGINAÇÃO? Qual tamanho de página?" | Para listagens grandes |
| 3.8 | "Precisa de ORDENAÇÃO? Quais colunas?" | Para listagens |
| 3.9 | "Precisa de HISTÓRICO? (log de alterações)" | Para auditoria |
| 3.10 | "Precisa de SOFT DELETE? (desativar em vez de remover)" | Para dados críticos |

### 🔵 Para Detalhamento

| # | Pergunta | Quando usar |
|---|----------|-------------|
| 3.11 | "Qual é o FORMATO das datas? (brasileiro, ISO)" | Para consistência |
| 3.12 | "Precisa de CAMPOS CALCULADOS? (ex: total = qtd * preco)" | Para regras de negócio |
| 3.13 | "Existe REGRA DE NEGÓCIO complexa? (ex: desconto progressivo)" | Para Services |
| 3.14 | "Precisa de NOTIFICAÇÕES automáticas? (ex: email ao salvar)" | Para eventos |
| 3.15 | "Precisa de WORKFLOW? (ex: aprovação em etapas)" | Para processos |

---

## FASE 4: Estime e Priorize

### ⚠️ Obrigatórias

| # | Pergunta | Propósito |
|---|----------|-----------|
| 4.1 | **"O que é MUST HAVE? (sem isso, não funciona)"** | Definir MVP |
| 4.2 | **"O que é SHOULD HAVE? (importante, mas pode esperar)"** | Priorizar |
| 4.3 | **"O que é COULD HAVE? (desejável, mas não essencial)"** | Cortar escopo |
| 4.4 | **"O que é WON'T HAVE? (explicitamente fora de escopo)"** | Delimitar |
| 4.5 | **"Qual é a DATA LIMITE para entrega?"** | Definir prazo |

### 🟡 Complementares

| # | Pergunta | Quando usar |
|---|----------|-------------|
| 4.6 | "Quantos DESENVOLVEDORES vão trabalhar nisso?" | Para estimativa |
| 4.7 | "Existe_algum BLOQUEADOR conhecido?" | Para identificar riscos |
| 4.8 | "Precisa de TESTES EXTRAS? (integração, carga, stress)" | Para qualidade |
| 4.9 | "Precisa de HOMOLOGAÇÃO? Quanto tempo?" | Para cronograma |
| 4.10 | "Precisa de DEPLOY especial? (janela de manutenção)" | Para operação |

### 🔵 Para Estimativa Detalhada

| # | Pergunta | Quando usar |
|---|----------|-------------|
| 4.11 | "Qual é a COMPLEXIDADE esperada de cada tarefa?" | Para sizing |
| 4.12 | "Existem TAREFAS que podem ser paralelizadas?" | Para otimizar |
| 4.13 | "Qual é o GARGALO principal?" | Para identificar crítico |
| 4.14 | "Existe DÉBITO TÉCNICO que precisa ser quitado?" | Para qualidade |
| 4.15 | "Precisa de DOCUMENTAÇÃO? (técnica, usuário)" | Para sustentabilidade |

---

## FASE 5: Validação Final

### ⚠️ Obrigatórias

| # | Pergunta | Propósito |
|---|----------|-----------|
| 5.1 | **"Todos os requisitos estão CLAROS? Algo faltando?"** | Validar completude |
| 5.2 | **"As restrições estão CORRETAS? Algo mudou?"** | Validar consistência |
| 5.3 | **"As decisões de design estão APROVADAS?"** | Validar alinhamento |
| 5.4 | **"O plano de tarefas está COMPLETO?"** | Validar execução |
| 5.5 | **"Posso encerrar o planejamento e handoff para implementação?"** | Obter aprovação final |

### 🟡 Complementares

| # | Pergunta | Quando usar |
|---|----------|-------------|
| 5.6 | "Existe ALGO que você quer adicionar antes de fechar?" | Para catch-all |
| 5.7 | "Alguma DECISÃO que precisa ser revisada?" | Para reconsideração |
| 5.8 | "Quer que eu EXPLIQUE alguma parte com mais detalhes?" | Para clareza |
| 5.9 | "Prefere que eu GERE um resumo executivo?" | Para stakeholders |
| 5.10 | "Precisa de mais ALGUMA COISA de mim?" | Para closure |

---

## 📋 Como Usar os Templates

### Regras
1. **NUNCA** pule as perguntas obrigatórias (⚠️)
2. **SEMPRE** aguarde resposta antes de continuar
3. **SEMPRE** repita o que entendeu e peça confirmação
4. **ADAPTE** as perguntas complementares ao contexto
5. **ADICIONE** perguntas customizadas conforme necessário

### Formato de Resposta
Para cada pergunta, documente:
```
Pergunta: [pergunta feita]
Resposta: [resumo da resposta]
Implicação: [o que isso muda no planejamento]
```

### Exceções
- Se o usuário não souber responder, ofereça opções
- Se a resposta for vaga, peça detalhamento
- Se houver conflito, apresente e peça resolução
