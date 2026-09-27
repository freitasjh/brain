# SaaS & Multi-Tenant Specification Patterns

## Architecture Topology

### 1. Control Plane (Gestão Global)
Responsável por orquestrar o ecossistema SaaS. Opera acima do nível do cliente.
- **Responsabilidades**: Gestão de Identidade, criação/onboarding de tenants, billing, roteamento de requisições, métricas globais.
- **Regra de Ouro**: O Control Plane não deve armazenar dados de negócio específicos (ex: produtos, pedidos, faturas de um cliente específico).

### 2. Data Plane (Execução de Negócio)
Onde as regras de negócio e os dados de cada inquilino residem.
- **Responsabilidades**: Lógica do domínio (ex: operações do ERP, fluxos de negócio).
- **Regra de Ouro**: Nenhuma requisição no Data Plane deve ser processada sem um contexto de tenant validado.

---

## Tenant Isolation Strategies (Estratégias de Isolamento)

### Nível Lógico (Row-level / Pooled)
- **Como funciona**: Todos os clientes compartilham o mesmo banco e as mesmas tabelas.
- **Mecanismo**: Toda tabela possui uma coluna `tenant_id`. Todas as queries (SELECT, UPDATE, DELETE) e regras de segurança (RLS) filtram obrigatoriamente por essa coluna.
- **Indicação**: Alta densidade de usuários, sistemas escaláveis horizontalmente com custo otimizado.

### Nível de Schema (Bridge)
- **Como funciona**: Um banco de dados compartilhado, mas esquemas (schemas) distintos para cada cliente (ex: `tenant_a.users`, `tenant_b.users`).
- **Indicação**: Equilíbrio entre isolamento e custo; facilita backups pontuais por cliente.

### Nível Físico (Database-per-tenant / Siloed)
- **Como funciona**: Instâncias de banco de dados completamente separadas por cliente.
- **Indicação**: Clientes Enterprise, conformidade rigorosa de dados, prevenção total do efeito *noisy neighbor* (vizinho barulhento).

---

## Tenant Resolution (Descoberta do Cliente)

A especificação da API deve definir explicitamente como o sistema identifica quem está fazendo a requisição:
1. **Via Subdomain (Recomendado)**: `https://{tenant_slug}.api.ecosystem.com`
2. **Via Header**: Passagem do cabeçalho `X-Tenant-ID: uuid` em cada chamada.
3. **Via JWT Claim**: O token de autenticação contém o claim contextual `{"tenant_id": "uuid"}`.

---

## Tenant Strategy Decision Matrix

When generating a specification, the Architect MUST calculate the correct Tenant Strategy based on the following matrix. **Never guess.**

| Factor | Row-Level (`tenant_id`) | Schema-per-Tenant | Database-per-Tenant |
| :--- | :--- | :--- | :--- |
| **Data Sensitivity** | Low to Medium | Medium to High | Extremely High (PHI/PII) |
| **Regulatory Compliance** | Basic (Standard LGPD) | High (Strict LGPD/GDPR) | Critical (BACEN, HIPAA, Defense) |
| **Tenant Scale (Volume)** | 10k+ Tenants (B2C/Micro B2B) | 100 - 10k Tenants (SMBs) | < 100 Tenants (Enterprise / Tier 1) |
| **Cost to Serve** | Very Low | Medium | High |
| **Backup & Restore** | System-wide only | Tenant-specific possible | Immediate Tenant-specific |

**Decision Rule:** 
Evaluate the Bounded Context. If the system handles critical Healthcare/Fintech data for Enterprise clients, default to **Database-per-tenant**. If it's a high-volume, low-cost B2B SaaS, default to **Row-level**. You MUST justify the decision in the Spec based on these factors.

---

## Identity & Access Management (IAM)

### Estrutura de Entidades Padrão
- **Tenant**: `id`, `name`, `slug`, `status` (ACTIVE, SUSPENDED), `tier_id`
- **Global User**: `id`, `email`, `password_hash`, `global_status` (Identidade única no Control Plane)
- **TenantMembership**: `user_id`, `tenant_id`, `role_id`, `status` (Mapeia o acesso do usuário a um tenant específico)

### Regras de RBAC Multi-Tenant
- Um usuário pode pertencer a múltiplos tenants (ex: Contador que acessa empresas diferentes).
- As permissões (roles) são atreladas ao `TenantMembership` e não ao usuário global (ex: "Admin" no Tenant A, "Viewer" no Tenant B).

---

## Constraints & Edge Cases Específicos para SaaS

- **Data Bleed Prevention**: Exigir testes automatizados que garantam que o Tenant A não consegue acessar recursos com o ID do Tenant B (IDOR - Insecure Direct Object Reference).
- **Noisy Neighbor**: Implementar *Rate Limiting* (controle de limite de taxa) em nível de `tenant_id` para evitar que um cliente sobrecarregue os recursos do Data Plane para os demais.
- **Tenant Lifecycle**: Mapear obrigatoriamente as máquinas de estado de suspensão de tenant (ex: o que acontece com os webhooks em andamento se o tenant for suspenso por falta de pagamento?).
