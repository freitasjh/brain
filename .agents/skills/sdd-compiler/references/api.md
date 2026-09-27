# API Design Patterns

## REST Conventions

- Resource naming: lowercase, plural, kebab-case (e.g., `/user-accounts`)
- HTTP methods:
  - GET `/resources` - List resources
  - GET `/resources/:id` - Get specific resource
  - POST `/resources` - Create new resource
  - PUT `/resources/:id` - Replace resource entirely
  - PATCH `/resources/:id` - Partial update
  - DELETE `/resources/:id` - Delete resource
- Nested resources: Limit to one level deep max (e.g., `/users/:id/orders`)
- Query params: snake_case for parameters (`sort_by`, `is_active`)

## Response Envelope & Pagination

### Offset Pagination (Small/Medium Datasets)
```json
{
  "data": [ ],
  "meta": {
    "page": 1,
    "limit": 20,
    "total_records": 150,
    "total_pages": 8
  }
}
```

### Cursor Pagination (Large/Real-time Datasets)
- Recommended for high-frequency data (transactions, logs)
- Query: `?limit=20&cursor=xyz123`
```json
{
  "data": [ ],
  "meta": {
    "next_cursor": "abc456",
    "has_more": true
  }
}
```

## Error Format (RFC 7807 - Problem Details)

Errors should be standardized to help consumers handle exceptions programmatically.

```json
{
  "type": "https://api.ecosystem.com/errors/validation-error",
  "title": "Unprocessable Entity",
  "status": 422,
  "detail": "The request payload failed validation.",
  "instance": "/users/123/orders",
  "trace_id": "req_8f7b2c9a",
  "errors": [
    {
      "field": "amount",
      "message": "Must be greater than zero"
    }
  ]
}
```

## Resiliency & Idempotency

- **Idempotency-Key**: Required header for all `POST` and `PATCH` requests on critical domains (billing, orders). Prevents duplicate operations if a client retries a timed-out request.
  - Header: `Idempotency-Key: <uuid>`
  - Retention: Keys must be stored and honored for 24 hours.

## Observability & Tracing

- **Correlation ID**: Required header to trace requests across distributed systems.
  - Header: `X-Correlation-ID: <uuid>`
  - Behavior: If provided by the client, propagate it. If absent, the API Gateway must generate one.

## Rate Limiting Headers

Responses must include current rate limit status to allow client throttling:
- `X-RateLimit-Limit`: 100
- `X-RateLimit-Remaining`: 99
- `X-RateLimit-Reset`: 1623456789 (Unix timestamp)

## Authentication & Versioning

- Versioning via URL path: `/v1/resources`
- Authentication: Bearer token in `Authorization` header
- Tenant Resolution (If applicable): Passed via `X-Tenant-ID` header or JWT claim.

## Webhooks Design

When specifying webhook endpoints that the system will call:
- Method: `POST`
- Timeout: Expect 5 seconds max response time from receiver.
- Retries: Exponential backoff (e.g., 5 retries over 24h).
- Security: Must sign the payload using HMAC-SHA256 and send in `X-Signature` header.

**Webhook Payload Example:**
```json
{
  "event_id": "evt_123",
  "event_type": "order.created",
  "created_at": "2026-05-01T10:00:00Z",
  "data": { }
}
```
