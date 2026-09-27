# Backend Architecture & Technology Patterns

## 1. Structural Architecture (Clean Code Boundaries)
Regardless of the chosen language or framework, the backend MUST adhere to **Hexagonal Architecture (Ports and Adapters)** or strict **Clean Architecture**. The Domain MUST be completely isolated from external frameworks, databases, and HTTP layers.

### Feature-Sliced / Modular Monolith Structure
```text
src/
├── core/                  # Global cross-cutting concerns (Auth Filters, Tenant Interceptors)
└── modules/               # Bounded Contexts (e.g., billing, identity, inventory)
    └── billing/
        ├── domain/        # Entities, Value Objects, Domain Exceptions (NO FRAMEWORK IMPORTS)
        ├── application/   # Use Cases, Command/Query handlers, Outbound Port Interfaces
        ├── infrastructure/# DB Repositories, External API Clients, Outbox Publishers
        └── presentation/  # REST Controllers, GraphQL Resolvers, gRPC Endpoints
```
*Rule:* The `domain` cannot import `application`. The `application` cannot import `infrastructure`. Dependencies MUST point inwards.

## 2. Domain-Driven Design (DDD) & State Management
- **Rich Models**: Anemic domain models (getters and setters only) are FORBIDDEN. Entities MUST encapsulate business rules and protect their invariants.
- **Domain Events**: Changes to aggregates MUST publish Domain Events. If a `Transaction` is completed, it publishes `TransactionSettledEvent`.
- **Transactions**: Database transactions MUST be kept as short as possible. External API calls (e.g., Stripe, SendGrid) MUST NOT be inside a database transaction block.

## 3. Handling Multi-Tenancy (Code Level)
If the SPEC defines a SaaS Tenant Strategy, the backend MUST implement:
- **Tenant Context**: A global request-scoped or thread-local context (e.g., `TenantContextHolder`) that stores the extracted `tenant_id` from the HTTP Header/JWT.
- **ORM Isolation**: 
  - *Row-Level Security*: The ORM (Hibernate, Prisma, TypeORM) MUST automatically append `WHERE tenant_id = ?` to ALL queries via filters or interceptors. Developers MUST NOT manually write tenant filters to avoid accidental data leakage.
  - *Database/Schema-per-tenant*: The framework MUST dynamically route the connection pool based on the `TenantContext`.

## 4. Resiliency & Event-Driven Patterns
- **The Outbox Pattern**: When emitting events, the backend MUST save the event payload to an `outbox_events` table within the SAME transaction as the business entity update. A background worker or CDC (Change Data Capture like Debezium) will poll and publish to the Message Broker.
- **Circuit Breakers**: External HTTP calls MUST be wrapped in a Circuit Breaker (e.g., Resilience4j for Java, opossum for Node.js) with configured timeouts and fallback methods.
- **Idempotency Implementation**: Mutating endpoints MUST check the `Idempotency-Key` against an `idempotency_records` table or Redis cache before executing the Use Case.

## 5. API Contracts & Error Handling
- **Problem Details**: REST API errors MUST follow the **RFC 7807** standard (`application/problem+json`).
  ```json
  {
    "type": "[https://api.example.com/errors/insufficient-funds](https://api.example.com/errors/insufficient-funds)",
    "title": "Insufficient Funds",
    "status": 400,
    "detail": "Account balance is lower than the requested transfer amount.",
    "instance": "/accounts/12345/transfers",
    "traceId": "abc-123-xyz"
  }
  ```
- **Pagination**: Collection endpoints MUST use offset/limit pagination (for standard grids) or cursor-based pagination (for infinite scroll/high volume).

## 6. Ecosystem-Specific Constraints

### Java Ecosystem (Spring Boot / Quarkus)
- **Spring Boot**: Use `ApplicationEventPublisher` for intra-module communication in Modular Monoliths. Use Spring Data JPA with `@TenantId` (Hibernate 6+) for row-level isolation.
- **Quarkus**: Ideal for high-density Control Planes and fast startup. Use Panache for data access and SmallRye Fault Tolerance for Circuit Breakers.
- **Immutability**: Use Java `record` types extensively for DTOs, Value Objects, and Events.

### Node.js Ecosystem (NestJS / TypeScript)
- **NestJS**: Use CQRS module for complex domains. Interceptors MUST be used for Tenant extraction and logging.
- **ORM**: Prisma or TypeORM. Prisma Client extensions MUST be used to enforce tenant isolation rules globally.

### Go Ecosystem
- **Structure**: Stick to standard Go project layouts (`cmd/`, `internal/`, `pkg/`).
- **Context**: The `context.Context` MUST be strictly passed down from the HTTP handler to the DB layer to carry tenant information, correlation IDs, and cancellation signals.
