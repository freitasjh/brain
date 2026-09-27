# E-Commerce Specification Patterns

## Typical Bounded Contexts

When specifying e-commerce features, strictly define which context is being touched:
- **Catalog Context**: Products, categories, variations (SKUs), pricing.
- **Inventory Context**: Stock levels, reservations (soft/hard allocation), warehouses.
- **Checkout & Cart Context**: Session management, price calculations, promotions, shipping calculation.
- **Order Management System (OMS)**: Order lifecycle, fulfillment, returns.
- **Payment Context**: Gateways, transactions, refunds.

## Core Entities & Data Sensitivity

| Entity | Core Fields | PII / Sensitive | Notes |
|--------|-------------|-----------------|-------|
| **Customer** | `id`, `document`, `email`, `phone`, `addresses` | **Yes** (Mask/Encrypt) | Strict LGPD/GDPR compliance required. |
| **Product** | `id`, `sku`, `name`, `base_price`, `status` | No | Highly cached (Redis/CDN). |
| **Order** | `id`, `customer_id`, `items`, `total`, `status` | **Yes** (Shipping Address) | Must use optimistic locking. |
| **Inventory**| `sku`, `available_qty`, `reserved_qty` | No | High concurrency. Requires locks. |

## State Transitions: Order Lifecycle (OMS)

Orders must follow a strict state machine. Never jump states bypassing business rules.

| From State | To State | Trigger / Event | Action / Compensation |
|------------|----------|-----------------|-----------------------|
| `PENDING` | `PAYMENT_PENDING` | Checkout submitted | Reserve inventory (Soft Allocation). |
| `PAYMENT_PENDING` | `PAID` | Webhook: Payment success | Hard allocate inventory. |
| `PAYMENT_PENDING` | `CANCELLED` | Webhook: Payment failed / Timeout | Release reserved inventory. |
| `PAID` | `SHIPPED` | Fulfillment processed | Notify customer, generate tracking. |
| `SHIPPED` | `DELIVERED` | Webhook: Carrier update | Complete order lifecycle. |

## Distributed Patterns & Event Flow

### The Checkout Saga (Event-Driven)
Avoid synchronous calls during checkout to external systems. Use an event-driven Saga pattern:
1. **Emit**: `checkout.completed`
2. **Payment Service** listens, attempts charge.
   - If success: Emits `payment.authorized`.
   - If failed: Emits `payment.failed`.
3. **Inventory Service** listens to `payment.authorized` to finalize deduction, or `payment.failed` to release the 15-minute reservation lock.

### Inventory Concurrency (Critical)
- **Reservation (Soft Allocation)**: When item is added to cart or checkout begins, reserve stock temporarily (e.g., in Redis with a 15-minute TTL).
- **Concurrency Control**: Must specify **Optimistic Locking** (e.g., `version` column in SQL) or **Pessimistic Locking** (`SELECT FOR UPDATE`) to prevent overselling when 500 users buy the last iPhone simultaneously.

## Common Exclusions (Scope Boundaries for Phase 1)

Explicitly exclude these to keep the MVP specifications focused:
- B2B Pricing layers (tier-based discounts).
- Multi-vendor Marketplace architecture (split payments).
- Subscription billing (recurring payments).
- Complex promotional engines (Buy 1 Get 1, Cart-level thresholds).

## Integration Resiliency
- **Payment Gateways**: Must specify Webhook endpoints for asynchronous payment confirmation.
- **Idempotency**: The `POST /checkout` or `POST /orders` endpoints **must** require an `Idempotency-Key` header to prevent double-charging the customer on network retries.
