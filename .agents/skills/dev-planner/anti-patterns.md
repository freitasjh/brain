# Anti-Patterns — Dev Planner

Lista de padrões ruins que você DEVE questionar quando o usuário os propor. Para cada anti-pattern, forneça:
- **Por que é ruim** (explicação técnica)
- **O que fazer** (alternativa recomendada)
- **Como questionar** (como abordar o usuário)

---

## 🔴 CRÍTICOS (Bloqueiam progresso)

### AP-001: Lógica de Negócio em ManagedBean/Controller

**Sinal:** "Vou colocar a regra de negócio direto no ManagedBean/Controller"

**Por que é ruim:**
- Viola separação de camadas (Presentation → Business → Persistence)
- Impossibilita reuso em outros contextos (API REST, job, outro módulo)
- Dificulta testes unitários (precisa de container para testar)
- Torna o código acoplado à tecnologia de apresentação

**O que fazer:**
- Colocar lógica de negócio em `@Stateless` Services
- ManagedBean/Controller apenas orquestra chamadas
- Seguir padrão: `ManagedBean → Service → DAO`

**Como questionar:**
> "Entendo que é mais rápido colocar no ManagedBean, mas isso viola a separação de camadas. Se amanhã precisar chamar essa regra de negócio de uma API REST ou de um job, terá que duplicar o código. Recomputar para o Service. Posso mostrar como fica a estrutura?"

---

### AP-002: DAO que não estende AbstractDAO/GenericDAOImpl

**Sinal:** "Vou criar um DAO novo sem herdar de nada"

**Por que é ruim:**
- Perde funcionalidades padrão (add, update, delete, findByID, lazyLoad)
- Perde logging automático de operações CRUD
- Perde dual-write DBF (quando aplicável)
- Inconsistência no padrão do projeto

**O que fazer:**
- Novos DAOs devem herdar de `AbstractDAO<T>`
- DAOs legados podem herdar de `GenericDAOImpl<T>`
- Seguir padrão existente no projeto

**Como questionar:**
> "O projeto possui AbstractDAO que já fornece add, update, delete, findByID e logging automático. Criar um DAO sem herdar isso significa reimplementar tudo. Posso mostrar como herdar de AbstractDAO e adicionar apenas os métodos customizados?"

---

### AP-003: Testes de DAO

**Sinal:** "Vou criar testes para o DAO"

**Por que é ruim:**
- DAO é camada de persistência sem lógica de negócio
- Testes de DAO são frágeis (dependem de banco de dados)
- Lógica de negócio deve ser testada no Service
- Viola regra do projeto: "NUNCA criar testes de DAO"

**O que fazer:**
- Testar Services (onde está a lógica de negócio)
- DAOs são validados indiretamente via testes de integração
- Seguir `unit-testing-standards.md`

**Como questionar:**
> "O projeto tem a regra de que não se testa DAO diretamente — a lógica de negócio está nos Services. Testes de DAO são frágeis e dependem de banco. Posso criar testes para o Service, que é onde está a regra de negócio?"

---

### AP-004: Expor Entidade JPA diretamente na API REST

**Sinal:** "Vou retornar a entidade JPA direto no endpoint"

**Por que é ruim:**
- LazyInitializationException (coleções não carregadas)
- Expõe estrutura interna do banco (campos sensíveis)
- Acoplamento entre API e modelo de persistência
- Impossibilita versão de contrato (breaking changes)

**O que fazer:**
- Usar DTOs (Data Transfer Objects)
- Criar Converters (Entity ↔ DTO)
- Seguir padrão BI Layer (`Controller → BI → Service`)

**Como questionar:**
> "Retornar a entidade JPA direto causa LazyInitializationException quando o cliente tentar acessar coleções (ex: itens do pedido). Além disso, expõe campos internos como senhas ou dados sensíveis. Posso criar um DTO e um Converter para isolar o contrato da API?"

---

### AP-005: Spring Boot no projeto Java EE 7

**Sinal:** "Vou usar Spring Boot para essa feature"

**Por que é ruim:**
- Incompatibilidade técnica (Java EE 7 vs Spring Boot)
- O projeto usa WildFly (JBoss) com EJBs @Stateless
- Duas stacks de DI (CDI vs Spring) causam conflitos
- Duplicação de dependências (Jackson, JPA, etc.)
- Complexidade desnecessária de manutenção

**O que fazer:**
- Usar EJBs @Stateless para Services
- Usar CDI para injeção de dependências
- Usar JPA (Hibernate) para persistência
- Seguir stack existente do projeto

**Como questionar:**
> "O projeto já usa Java EE 7 com EJBs @Stateless no WildFly. Adicionar Spring Boot criaria conflitos de DI (CDI vs Spring) e duplicaria dependências. A não ser que haja uma razão muito específica, recomendo manter a stack existente. Qual é amotivação para usar Spring?"

---

## 🟠 ALTOS (Riscos significativos)

### AP-006: God Object / God Service

**Sinal:** "Vou criar um Service que faz tudo"

**Por que é ruim:**
- Viola princípio de coesão (SRP)
- Torna o código impossível de testar
- Dificulta manutenção e debugging
- Cria dependências circulares

**O que fazer:**
- Separar por domínio (PedidoService, EstoqueService, FinanceiroService)
- Cada Service deve ter uma única responsabilidade
- Usar Services compostos quando necessário

**Como questionar:**
> "Um Service que faz tudo viola o princípio de responsabilidade única. Se amanhã precisar alterar apenas uma parte, terá que mexer em tudo. Posso dividir em Services menores, cada um responsável por um domínio?"

---

### AP-007: Queries JPQL sem parâmetros nomeados

**Sinal:** "Vou concatenar strings na query: WHERE id = " + id

**Por que é ruim:**
- SQL Injection (vulnerabilidade de segurança)
- Impossibilita cache de query
- Dificulta debug e manutenção
- Viola boas práticas JPA

**O que fazer:**
- Usar parâmetros nomeados: `WHERE id = :id`
- Usar `setParameter("id", valor)`
- Seguir padrão do projeto

**Como questionar:**
> "Concatenar strings em queries JPQL cria vulnerabilidade de SQL Injection. Além disso, parâmetros nomeados permitem cache de query. Posso usar `:parametro` em vez de concatenação?"

---

### AP-008: hardcode de credenciais

**Sinal:** "Vou colocar a senha do banco direto no código: senha = "123456""

**Por que é ruim:**
- Vazamento de segurança (senhas no código fonte)
- Impossibilita rotação de credenciais
- Viola políticas de segurança
- Expõe em repositórios versionados

**O que fazer:**
- Usar variáveis de ambiente
- Usar arquivos de configuração externos
- Usar JNDI (WildFly)
- Seguir `.agents/rules/backend-coding-standards.md`

**Como questionar:**
> "Colocar credenciais no código fonte é uma vulnerabilidade de segurança crítica — qualquer pessoa com acesso ao repositório vê a senha. Recomendo usar variáveis de ambiente ou JNDI. Posso configurar?"

---

### AP-009: Ignorar Multi-tenancy

**Sinal:** "Não precisa se preocupar com multi-tenancy, é só pra uma empresa"

**Por que é ruim:**
- O projeto é multi-tenant por design (schema por tenant)
- Ignorar isso cria inconsistência de dados
- Dificulta futura expansão
- Viola arquitetura do projeto

**O que fazer:**
- Sempre usar `getEntityManager()` do DAO (usa MultiTenancyEntityManagerWrapper)
- Nunca criar EntityManager diretamente
- Testar isolamento entre tenants

**Como questionar:**
> "O projeto é multi-tenant por design. Mesmo que hoje seja para uma empresa, o sistema já isola dados por tenant. Ignorar isso cria inconsistência. Posso garantir que a feature use o MultiTenancyEntityManagerWrapper?"

---

### AP-010: Não criar testes

**Sinal:** "Não precisa de teste, é rápido demais"

**Por que é ruim:**
- Regra do projeto: testes são obrigatórios
- Sem testes, regressões passam despercebidas
- Dificulta refatoração futura
- Aumenta custo de manutenção

**O que fazer:**
- Criar testes unitários para Services (JUnit 4/5 + Mockito)
- Criar testes de contrato para API REST
- Seguir `unit-testing-standards.md`

**Como questionar:**
> "O projeto tem a regra de que todo desenvolvimento precisa de testes. Sem testes, qualquer alteração futura pode quebrar algo sem perceber. Posso criar testes unitários para o Service? É mais rápido do que parece."

---

## 🟡 MÉDIOS (Riscos moderados)

### AP-011: Tela XHTML com mais de 500 linhas

**Sinal:** "Vou colocar tudo em uma tela só"

**Por que é ruim:**
- Dificulta manutenção
- Torna o código ilegível
- Cria componentes repetidos
- Viola boas práticas JSF

**O que fazer:**
- Quebrar em sub-componentes ou compositions
- Usar `<ui:include>` para reutilizar partes
- Manter cada tela focada em uma funcionalidade

**Como questionar:**
> "Uma tela com mais de 500 linhas é difícil de manter e entender. Posso quebrar em sub-componentes usando `<ui:include>`? Fica mais organizado e reutilizável."

---

### AP-012: Service com mais de 1000 linhas

**Sinal:** "Vou colocar toda a lógica de negócio em um único Service"

**Por que é ruim:**
- Viola coesão (SRP)
- Dificulta testes
- Cria dependências circulares
- Torna manutenção dolorosa

**O que fazer:**
- Extrair lógica para Services menores
- Separar por domínio ou responsabilidade
- Usar composição quando necessário

**Como questionar:**
> "Um Service com 1000+ linhas é difícil de testar e manter. Posso extrair partes para Services menores? Por exemplo, separar validação, cálculo e persistência?"

---

### AP-013: Múltiplos h:form aninhados

**Sinal:** "Vou colocar um form dentro do outro"

**Por que é ruim:**
- JSF NÃO suporta formulários aninhados
- Causa comportamento inesperado
- Dificulta validação
- Viola padrão JSF

**O que fazer:**
- Usar um único `<h:form>` por página
- Usar `<a4j:region>` ou `<f:ajax>` para atualização parcial
- Separar formulários em áreas distintas

**Como questionar:**
> "JSF não suporta formulários aninhados — isso causa comportamento inesperado. Recomendar usar um único `<h:form>` por página e usar `<f:ajax>` para atualização parcial. Posso mostrar como?"

---

### AP-014: @ManagedProperty para injeção

**Sinal:** "Vou usar @ManagedProperty para injetar beans"

**Por que é ruim:**
- @ManagedProperty é legado (JSF 2.0)
- CDI é o padrão recomendado
- @ManagedProperty tem limitações de escopo
- Dificulta testes

**O que fazer:**
- Usar `@EJB` para injetar EJBs
- Usar `@Inject` para injetar CDI beans
- Evitar @ManagedProperty

**Como questionar:**
> "@ManagedProperty é legado do JSF 2.0. O projeto usa CDI para injeção. Posso usar `@EJB` ou `@Inject`? É mais moderno e testável."

---

### AP-015: Navegação por navigation-rule

**Sinal:** "Vou criar regras de navegação no faces-config.xml"

**Por que é ruim:**
- Navigation-rule é verboso e difícil de manter
- Navegação implícita (return string) é mais simples
- Dificulta fluxos condicionais
- Viola convenção do projeto

**O que fazer:**
- Usar navegação implícita (return string de action)
- Usar `<redirect/>` quando necessário
- Manter faces-config.xml limpo

**Como questionar:**
> "Navigation-rule é verboso. O projeto usa navegação implícita (return string). Posso usar `return "pagina"` no action? É mais simples e mantenável."

---

## 🟢 BAIXOS (Melhorias opcionais)

### AP-016: System.out.println em produção

**Sinal:** "Vou usar System.out.println para debug"

**Por que é ruim:**
- Não tem níveis de severidade
- Não pode ser desativado em produção
- Polui console do servidor
- Viola boas práticas de logging

**O que fazer:**
- Usar SLF4J Logger
- Usar níveis apropriados (debug, info, warn, error)
- Seguir padrão do projeto

**Como questionar:**
> "System.out.println não pode ser desativado em produção e não tem níveis. Posso usar SLF4J Logger? Permite desativar debug em produção e é o padrão do projeto."

---

### AP-017: e.printStackTrace() em produção

**Sinal:** "Vou usar e.printStackTrace() para ver o erro"

**Por que é ruim:**
- Vazamento de informação (stack trace exposta)
- Não vai para o log do servidor
- Não tem contexto (qual operação falhou)
- Viola boas práticas

**O que fazer:**
- Usar logger.error("mensagem", exception)
- Incluir contexto na mensagem
- Tratar exceção adequadamente

**Como questionar:**
> "e.printStackTrace() vai para stdout, não para o log do servidor. Além disso, vaza informações técnicas. Posso usar `logger.error("Erro ao salvar cliente", e)`? É mais seguro e rastreável."

---

### AP-018: Criar entidade sem estender BaseEntity

**Sinal:** "Vou criar uma entidade sem herdar de BaseEntity"

**Por que é ruim:**
- Perde campos padrão (id, cod, lastModification)
- Inconsistência no padrão do projeto
- Dificulta busca por código
- Perde timestamp automático

**O que fazer:**
- Sempre estender BaseEntity
- Usar campos padrão (id, cod, lastModification)
- Seguir `backend-coding-standards.md`

**Como questionar:**
> "O projeto usa BaseEntity que já fornece id, cod e lastModification. Criar entidade sem herdar isso cria inconsistência. Posso herdar de BaseEntity?"

---

### AP-019: Ignorar @TransactionAttribute

**Sinal:** "Não preciso colocar @TransactionAttribute"

**Por que é ruim:**
- Transação pode não iniciar corretamente
- Pode causar commit parcial
- Dificulta debug de problemas de transação
- Viola regra do projeto

**O que fazer:**
- Declarar @TransactionAttribute no DAO e ServiceImpl quando aplicável
- Usar REQUIRED para operações de escrita
- Seguir `backend-coding-standards.md`

**Como questionar:**
> "O projeto exige @TransactionAttribute em DAO e ServiceImpl quando o DAO estende AbstractDAO. Isso garante que a transação inicie corretamente. Posso adicionar?"

---

### AP-020: Usar Lombok

**Sinal:** "Vou usar Lombok para gerar getters/setters"

**Por que é ruim:**
- O projeto NÃO usa Lombok
- Gera dependência desnecessária
- Dificulta debugging (código invisível)
- Viola regra do projeto: "Nenhuma anotação Lombok"

**O que fazer:**
- Escrever getters/setters manualmente
- Usar IDE para gerar automaticamente
- Seguir convenção do projeto

**Como questionar:**
> "O projeto não usa Lombok e tem a regra de não usar anotações Lombok. Posso gerar getters/setters via IDE? É mais transparente para debugging."

---

## 📋 Como Usar os Anti-Patterns

### Regras
1. **SEMPRE** questione anti-patterns críticos e altos
2. **SEMPRE** explique o porquê (não apenas "não faça isso")
3. **SEMPRE** ofereça alternativa viável
4. **NUNCA** seja agressivo — seja educado mas firme
5. **REGISTRE** se o usuário insistir em seguir o anti-pattern

### Formato de Questionamento
```
"Entendo que [solução proposta do usuário]. 
Porém, [anti-pattern] é problemático porque [razão técnica]. 
Recomendo [alternativa] porque [justificativa]. 
Posso mostrar como fica?"
```

### Quando Aceitar o Anti-Pattern
Se o usuário insistir:
1. Documente o risco
2. Registre como "débito técnico"
3. Siga em frente com ressalva
4. Registre no Brain para iteração futura

---

## 🔗 Referências

| Fonte | Propósito |
|-------|-----------|
| OWASP Top 10 | Padrões de segurança |
| SOLID Principles | Princípios de design |
| Clean Architecture | Arquitetura limpa |
| `.agents/rules/backend-coding-standards.md` | Padrões do projeto |
| `.agents/rules/frontend-coding-standards.md` | Padrões frontend |
| `.agents/rules/unit-testing-standards.md` | Regras de testes |
