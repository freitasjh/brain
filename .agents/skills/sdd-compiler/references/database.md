# Database Architecture & Data Modeling Patterns

## 1. Multi-Tenant Data Strategies (SaaS)

When designing for SaaS, the database physical layout MUST match the Tenant Strategy defined in the SPEC.

### Row-Level Security (RLS) - PostgreSQL (Default for High-Density SaaS)
- **Concept**: All tenants share the same tables, but a `tenant_id` column isolates data.
- **Implementation Rule**: MUST use native PostgreSQL RLS policies to prevent application-layer bugs from leaking cross-tenant data.
```sql
-- Pattern for RLS Setup
ALTER TABLE orders ENABLE ROW LEVEL SECURITY;

CREATE POLICY tenant_isolation_policy ON orders
    USING (tenant_id = current_setting('app.current_tenant')::UUID);
```

### Schema-per-Tenant or Database-per-Tenant
- **Use when**: Strict compliance (HIPAA, BACEN) or Enterprise-tier customers are required.
- **Rule**: Connection pooling mechanisms (e.g., HikariCP) MUST be dynamically routed based on the context tenant.

## 2. Concurrency & Data Integrity
High-concurrency systems MUST define how they handle simultaneous updates to prevent lost updates or double-spending.

### Optimistic Locking
- **Use when**: Read-heavy operations, low chance of collision (e.g., updating a User Profile or a Medical Record draft).
- **Implementation Rule**: Every core entity MUST include a `version` (integer) column. The ORM throws a `StaleObjectStateException` if versions mismatch on update.
```sql
UPDATE inventory SET qty = 9, version = 2 WHERE id = 123 AND version = 1;
```

### Pessimistic Locking
- **Use when**: High collision probability or financial/inventory transactions.
- **Implementation Rule**: MUST use `SELECT ... FOR UPDATE` to lock the row at the database level until the transaction commits.

## 3. Event-Driven Data Patterns

### The Transactional Outbox Pattern (Mandatory for Microservices/Modular Monoliths)
To guarantee that a domain event is published if and only if the database transaction commits, you MUST use an Outbox table.
```sql
CREATE TABLE outbox_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    aggregate_type VARCHAR(255) NOT NULL, -- e.g., 'Order'
    aggregate_id UUID NOT NULL,
    event_type VARCHAR(255) NOT NULL,     -- e.g., 'OrderCreated'
    payload JSONB NOT NULL,
    status VARCHAR(50) DEFAULT 'PENDING', -- PENDING, PROCESSED, FAILED
    created_at TIMESTAMP DEFAULT NOW()
);
```

## 4. Audit & Compliance (LGPD/GDPR)

- **Soft Deletes**: Critical domain entities (e.g., Invoices, Accounts) MUST NOT be hard-deleted. Use a `deleted_at` timestamp.
- **Audit Trails (Append-Only)**: Highly sensitive tables MUST implement history tracking. 
  - *Pattern*: Use tools like Hibernate Envers, or database triggers that write the old state to a `[table_name]_audit` table on every `UPDATE` or `DELETE`.
- **PII Protection**: Personally Identifiable Information (PII) MUST be encrypted at rest. Do not rely solely on disk encryption; use application-level cryptography (AES-256) for fields like `cpf`, `ssn`, or `medical_notes` before persisting.

## 5. Schema Evolution & Migrations
- **Tooling**: Schema changes MUST be versioned using Flyway or Liquibase.
- **Zero-Downtime Rule (Expand and Contract)**:
  - You are FORBIDDEN from performing destructive operations (e.g., `DROP COLUMN`, `RENAME COLUMN`) in a single deployment.
  - *Phase 1 (Expand)*: Add the new column. App writes to both, reads from old.
  - *Phase 2 (Migrate)*: Backfill data. App reads/writes to new.
  - *Phase 3 (Contract)*: Drop the old column.

## 6. NoSQL & Caching Strategies

### Redis (Advanced Patterns)
Beyond basic caching, Redis MUST be used for:
- **Distributed Locks**: Use Redlock pattern to prevent duplicate background job execution across multiple instances.
- **Idempotency Keys**: Store `Idempotency-Key` headers with a 24-hour TTL.
```redis
-- Idempotency Check (Set if Not eXists)
SET idempotency:payment:req-123 "PROCESSED" NX EX 86400
```

### Document Stores (MongoDB / DynamoDB)
- **Use when**: The schema is highly dynamic (e.g., custom form builders, unstructured product catalogs) or for extremely high-throughput append-only event sourcing.
- **Rule**: Do not use MongoDB just to avoid learning SQL. If the data has relationships, stick to PostgreSQL.
