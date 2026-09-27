# Fintech Specification Patterns

## Typical Bounded Contexts

When specifying fintech features, maintain strict boundaries between domains:
- **Onboarding & Identity (KYC)**: Customer data, document verification, biometric checks.
- **Core Banking (Ledger)**: Account balances, double-entry accounting, transaction history.
- **Payments / Transfers**: External integrations (PIX, TED, Boletos), routing.
- **Fraud & Compliance**: Anti-Money Laundering (AML) rules, transaction screening, limits.

## Core Entities & Data Sensitivity

| Entity | Core Fields | PII / Sensitive | Notes |
|--------|-------------|-----------------|-------|
| **Customer** | `id`, `document(CPF/CNPJ)`, `full_name` | **Yes** (Strict) | Fully encrypted at rest. LGPD compliance. |
| **Account** | `id`, `customer_id`, `status`, `routing_number` | No | *Never* store the balance directly as an editable field. |
| **LedgerEntry**| `id`, `account_id`, `amount`, `type (DR/CR)` | No | Immutable. Balance is a derived sum of these entries. |
| **Transaction**| `id`, `amount`, `currency`, `status` | No | Links two or more LedgerEntries. |

## Critical Architectural Constraints

### 1. Money Representation
- **NEVER** use `float` or `double` for monetary values.
- **Pattern**: Store amounts as **Integers** (e.g., in cents: R$ 10,50 is stored as `1050`) OR use precise arbitrary-precision decimals (`BigDecimal` in Java/C#, `numeric` in PostgreSQL).

### 2. Double-Entry Accounting (Partidas Dobradas)
- Money is never created or destroyed, only moved.
- Every transaction MUST have at least two `LedgerEntry` records: a Debit (DR) from the source account and a Credit (CR) to the destination account. The sum of all entries for a transaction must equal zero.

### 3. Concurrency & Locks
- **Balance Checks**: When checking balance to authorize a transaction, use **Pessimistic Locking** (`SELECT ... FOR UPDATE`) on the account record to prevent race conditions (e.g., two concurrent R$100 withdrawals when the balance is only R$100).

## State Transitions

### Customer Onboarding Lifecycle
| From State | To State | Trigger / Event | Action / Compliance |
|------------|----------|-----------------|-----------------------|
| `PENDING` | `KYC_REVIEW` | Docs submitted | Trigger background OCR / background check. |
| `KYC_REVIEW` | `ACTIVE` | KYC approved | Create Core Banking Account. |
| `KYC_REVIEW` | `REJECTED` | Fraud detected | Log reason, notify user, block document. |

### Transaction Lifecycle
| From State | To State | Trigger / Event | Action / Compensation |
|------------|----------|-----------------|-----------------------|
| `INITIATED` | `AML_CHECK` | Request received | Check against user limits and suspicious patterns. |
| `AML_CHECK` | `PROCESSING` | AML cleared | Lock balance, execute external API call (e.g., PIX). |
| `PROCESSING` | `SETTLED` | Gateway confirmed | Write immutable LedgerEntries. Release lock. |
| `PROCESSING` | `FAILED` | Gateway timeout | Rollback. Emits `transaction.failed`. |

## Resiliency & Idempotency

- **Strict Idempotency**: All payment and transfer endpoints MUST require an `Idempotency-Key` header. If a network drops and the client retries the transfer, the system must recognize the key and return the original success response without moving money twice.
- **Saga Pattern**: Use distributed transactions (Sagas) for transfers involving external banks. If the external bank rejects the transfer, the Saga must execute a compensation transaction (credit the money back to the user's ledger).

## Regulatory & Compliance Requirements (Brazil / BACEN)
- Audit trails for every state change.
- Mandatory AML (Anti-Money Laundering) screening for transactions over specific thresholds (e.g., R$ 10.000).
- Data retention: Financial logs must be retained securely according to Central Bank regulations (usually 5 to 10 years).
