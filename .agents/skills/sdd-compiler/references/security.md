# Enterprise Security, IAM & Compliance Patterns

## 1. Identity & Access Management (IAM)
You MUST NOT build custom username/password authentication systems unless strictly required by an air-gapped environment. Rely on Identity Providers (IdPs).

### Identity Providers (IdP)
- **Mandatory Tools**: Keycloak, Auth0, AWS Cognito, or Azure AD.
- **Protocols**: OAuth 2.0 and OpenID Connect (OIDC).
- **Flows**: 
  - SPAs / Mobile: MUST use **Authorization Code Flow with PKCE**. Implicit flow is FORBIDDEN.
  - Server-to-Server: MUST use **Client Credentials Flow**.

### Session & Token Management
- **Asymmetric Signing**: JWTs MUST be signed using `RS256` or `ES256` with rotating JWKS (JSON Web Key Sets). `HS256` (symmetric) is highly discouraged for distributed systems.
- **Storage (Web)**: Access Tokens MUST NOT be stored in `localStorage`. Use `HttpOnly`, `Secure`, `SameSite=Strict` cookies.
- **Revocation**: The system MUST implement a Token Blocklist (e.g., in Redis) to handle manual logouts, permission changes, or compromised accounts before the token naturally expires.

## 2. Cryptography & Key Management (KMS)
Data at rest is vulnerable. Database-level encryption (TDE) protects against stolen hard drives, but application-level encryption protects against SQL Injections and leaked backups.

### Key Management
- **Mandatory Tools**: AWS KMS, GCP KMS, or HashiCorp Vault.
- **Rotation**: Cryptographic keys MUST be automatically rotated every 90 days.
- **Tenant Segregation**: In SaaS environments, cryptographic contexts MUST be segregated by `tenant_id` to prevent cross-tenant decryption.

### Envelope Encryption for PII/PHI
Personally Identifiable Information (CPF, SSN, Medical Notes) MUST use Envelope Encryption.
1. Generate a Data Encryption Key (DEK) via KMS.
2. Encrypt the PII with the DEK (AES-256-GCM).
3. Encrypt the DEK with the Master Key (CMK) and store the encrypted DEK alongside the data.

## 3. Application & API Security (OWASP)

### Advanced Rate Limiting & Throttling
Basic IP rate limiting is insufficient for SaaS due to shared office IPs (NAT).
- **Strategy**: Rate limits MUST be applied by `tenant_id` and `user_id` (if authenticated), and fallback to IP (if unauthenticated).
- **Algorithm**: Leaky Bucket or Sliding Window via Redis.
- **WAF**: An API Gateway / WAF (Cloudflare, AWS WAF) MUST be placed in front of the application to block volumetric DDoS and automated bot attacks.

### Security Headers
All HTTP responses MUST include strict security headers:
```http
Strict-Transport-Security: max-age=63072000; includeSubDomains; preload
Content-Security-Policy: default-src 'self'; frame-ancestors 'none';
X-Content-Type-Options: nosniff
X-Frame-Options: DENY
Referrer-Policy: strict-origin-when-cross-origin
```

## 4. Compliance & Audit (LGPD / GDPR)

### Audit Trails (Immutable)
- **Rule**: Actions that mutate state (Create, Update, Delete) or access sensitive PII (Read) MUST generate an Audit Log.
- **Structure**: The log MUST contain: `actor_id` (who), `action` (what), `resource_id` (target), `tenant_id` (context), `timestamp`, and `ip_address`.
- **Immutability**: Audit logs MUST be written to an append-only datastore (e.g., AWS QLDB, WORM storage, or separate secured database).

### The Right to be Forgotten (Data Deletion)
- **Cryptographic Erasure (Crypto-shredding)**: The preferred method for deleting user data from immutable logs or complex backups is deleting the user's specific KMS Encryption Key. Once the key is destroyed, the data is mathematically unrecoverable, fulfilling LGPD/GDPR requirements without breaking database integrity.
- **Anonymization**: If data must be kept for statistical reasons, identifiers (Name, Email, IP) MUST be hashed with a pepper or replaced with UUIDs.

## 5. DevSecOps & Secrets Management
- **No Hardcoded Secrets**: Secrets MUST NEVER exist in source code or `.env` files in production.
- **Secret Injection**: Secrets MUST be injected at runtime via Kubernetes Secrets, AWS Secrets Manager, or Vault.
- **Vulnerability Scanning**: CI/CD pipelines MUST include SAST (Static Application Security Testing) and SCA (Software Composition Analysis - e.g., Dependabot/Snyk) to block builds with known CVEs.
