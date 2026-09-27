---
name: security-review
description: "Security review skill integrated into the SDD workflow. Validates security at each SDD phase gate against OWASP Top 10, ASVS 4.x, MASVS 2.x, and the TaskFlow security-standard.md (source of truth). Covers web, API, and mobile."
license: TaskFlow Security Standard
---

# Security Review — SDD Integrated Skill

Security review as a **phase-integrated gate** in the Spec-Driven Development (SDD) workflow. This skill is invoked by the orchestrator at defined checkpoints to validate that security requirements are met before advancing between phases.

## When to Apply

### Must Use
- **Fase 1 (Analysis):** After SPEC is generated — validate security requirements coverage.
- **Fase 3 (Review):** After code review — security-focused review of implementation.
- **Fase 4 (Verify):** Before archive — validate against `security-standard.md` zero-tolerance checklist.
- **Bugfix:** When the bug affects auth, authorization, data exposure, or input handling.
- **Any feature** with authentication, authorization, user data, external integrations, or file uploads.

### Recommended
- Every SDD cycle (feature or bugfix) — treat security as a first-class gate.
- When adding new endpoints, DTOs, or data flows.
- When changing auth mechanism or integrating external identity providers.

### Skip
- Pure backend logic with no user-facing input/output (rare).
- Internal tooling with no network exposure and no persistent data (validate with security-engineer).

**Decision criteria:** If the feature touches user data, network, auth, or external systems, this skill **must** be invoked.

---

## Reference Documents

| Document | Purpose |
|----------|---------|
| `.agents/rules/security-standard.md` | **Fonte da verdade** — OWASP Top 10, ASVS 4.x, MASVS 2.x, TaskFlow rules |
| `.agents/rules/backend-coding-standards.md` | Backend coding patterns (injection prevention, DTO projection) |
| `.agents/rules/frontend-coding-standards.md` | Frontend security (XSS, CSP, CSRF, token storage) |
| `.agents/rules/backend-integration-tests.md` | Security test patterns (401/403, IDOR, injection) |
| `.agents/agents/security-engineer/AGENT.md` | Deep audit agent (invoke for complex findings) |

---

## SDD Phase Integration

### Fase 1 — Analysis: Security Requirements Review

After the SPEC is generated, run this review:

#### 1.1 Threat Modeling (STRIDE)
For each new endpoint or data flow, identify:
- **Spoofing:** Can an attacker impersonate a user or system?
- **Tampering:** Can data be modified in transit or at rest?
- **Repudiation:** Are actions logged with non-repudiation?
- **Information Disclosure:** Are sensitive fields exposed in responses, logs, or error messages?
- **Denial of Service:** Can endpoints be overwhelmed (no rate limiting, no pagination)?
- **Elevation of Privilege:** Can a user access resources belonging to another user or role?

#### 1.2 Security Requirements Checklist
- [ ] All endpoints protected with `@HasPermission` — no public endpoint without justification
- [ ] Input validation with `@Valid` on all DTOs
- [ ] SQL/JPQL parameterized — no string concatenation in queries
- [ ] DTOs project explicit fields — no entity serialization (no sensitive field leakage)
- [ ] Auth0 JWT validated: signature, `iss`, `aud`, `exp`
- [ ] Rate limiting on auth endpoints (login, register)
- [ ] Error messages are generic (no stack traces, no internal details)
- [ ] Secrets via environment variables — no hardcoded credentials
- [ ] HTTPS/TLS 1.2+ enforced in production config
- [ ] Actuator/Swagger protected or disabled in production
- [ ] RabbitMQ: TLS for external connections, payload validation, durable queues
- [ ] Mobile (if applicable): Keychain/Keystore for tokens, TLS mandatory, no cleartext

#### 1.3 Output: Security Assessment for SPEC
```markdown
## Security Assessment — Fase 1

### Threat Model (STRIDE)
| Threat | Endpoint/Flow | Risk | Mitigation |
|--------|---------------|------|------------|
| Elevation of Privilege | GET /v1/tasks/{id} | High | @HasPermission + ownership check |
| Information Disclosure | Error response | Medium | Generic error messages |

### Requirements Coverage
- [x] AuthN covered (JWT validation)
- [x] AuthZ covered (@HasPermission on all endpoints)
- [x] Input validation covered (@Valid on DTOs)
- [ ] Rate limiting on login — **GAP: add @RateLimit**
- [x] No entity serialization in responses

### Gaps
- 🔴 Critical: [none / list]
- 🟠 High: [none / list]
- 🟡 Medium: [none / list]
- 🟢 Low: [none / list]

### Verdict
- pass | fail | partial (gaps documented)
```

**If 🔴 Critical gaps exist:** Block advancement to Fase 2. Fix in SPEC before proceeding.

---

### Fase 3 — Review: Security-Focused Code Review

After the `fullstack-code-reviewer` completes the general review, run the security-specific review:

#### 3.1 OWASP Top 10 Code Review
- [ ] **A01 (Broken Access Control):** Every endpoint has `@HasPermission`. Ownership check in service layer. No IDOR patterns.
- [ ] **A02 (Cryptographic Failures):** JWT signed with RS256/ES256. No `none` algorithm. Password hashing with Argon2id/bcrypt. TLS 1.2+ in production.
- [ ] **A03 (Injection):** All JPQL/QueryDSL parameterized. No string concatenation in native queries. `@Valid` on all inputs. RabbitMQ payloads validated.
- [ ] **A04 (Insecure Design):** Rate limiting on auth endpoints. No sensitive data in URLs. Payload size limits enforced.
- [ ] **A05 (Security Misconfiguration):** No default credentials. Verbose errors disabled in production. Actuator/Swagger protected. Dependencies audited (no critical CVEs).
- [ ] **A06 (Vulnerable Components):** `npm audit` clean (no critical/high). OWASP Dependency-Check clean.
- [ ] **A07 (Auth Failures):** JWT expiração curta + refresh rotativo. Logout invalida token. No credentials in logs/URLs.
- [ ] **A08 (Integrity):** RabbitMQ publisher confirms. Idempotency in consumers. Dependencies pinned (lockfiles).
- [ ] **A09 (Logging):** Security events logged (login success/failure, permission denied, data deletion). No secrets in logs. Correlation IDs present.
- [ ] **A10 (SSRF):** URL allowlist for server-side fetches. Private IP ranges blocked.

#### 3.2 Output: Security Review Report
```markdown
## Security Code Review — Fase 3

### OWASP Top 10 Status
| Category | Status | Finding |
|----------|--------|---------|
| A01 Broken Access Control | ✅ | @HasPermission on all endpoints |
| A02 Cryptographic Failures | ✅ | RS256 JWT, Argon2id passwords |
| A03 Injection | ✅ | Parameterized queries, @Valid |
| A04 Insecure Design | 🟡 | Rate limiting missing on /v1/auth/register |
| A05 Misconfiguration | ✅ | |
| A06 Vulnerable Components | ✅ | npm audit + Dependency-Check clean |
| A07 Auth Failures | ✅ | |
| A08 Integrity | ✅ | |
| A09 Logging | 🟡 | Login failure not logged |
| A10 SSRF | ✅ | |

### 🔴 Critical Findings: 0
### 🟡 Attention Findings: 2
### 🟢 Suggestions: 0

### Verdict
- pass | fail | partial
```

**If 🔴 Critical findings:** Return to developer-engineer for fix. Re-review after fix. Max 3 iterations.

---

### Fase 4 — Verify: Security Validation Gate

Before archive, validate against `security-standard.md` zero-tolerance checklist:

#### 4.1 Zero-Tolerance Checklist
- [ ] Controle de acesso server-side em todos os endpoints (`@HasPermission`)?
- [ ] Input validation server-side em todo DTO (`@Valid`)?
- [ ] SQL/JPQL parametrizado — zero concatenação de strings em queries?
- [ ] Senhas com Argon2id/bcrypt — nunca texto puro ou hash fraco?
- [ ] JWT RS256/ES256 com validação de assinatura, `iss`, `aud`, `exp`?
- [ ] HTTPS/TLS 1.2+ em produção (web e API)?
- [ ] Security headers configurados (CSP, X-Content-Type-Options, etc.)?
- [ ] Zero `v-html` com conteúdo não sanitizado no frontend?
- [ ] Sem segredos em código versionado, logs ou respostas de API?
- [ ] Rate limiting em autenticação e endpoints sensíveis?
- [ ] Dependências auditadas (npm audit + OWASP Dependency-Check) — zero CVE alta sem mitigação?
- [ ] Actuator/Swagger protegidos em produção?
- [ ] Mobile (se aplicável): tokens em Keychain/Keystore, TLS obrigatório, sem cleartext?

#### 4.2 Output: Security Verify Report
```markdown
## Security Verify — Fase 4

### Zero-Tolerance Checklist
- [x] All 14 items passed

### Verdict
- ✅ pass — all items met, archive allowed
- ❌ fail — one or more items failed, block archive, return to Fase 3
```

**If fail:** Block archive. Return to Fase 3 for fixes. Re-validate.

---

### Bugfix — Security Review

For bugfixes involving auth, authorization, data exposure, or input handling:

1. Validate the fix doesn't introduce new vulnerabilities (re-run A01–A10 checklist).
2. Validate the fix addresses the root cause securely (not just a band-aid).
3. Add security test cases (401/403, IDOR attempt, injection payload).
4. Include in the HANDOFF REPORT: security impact assessment.

---

## Output Contract

When completing a security review, conclude with a **Security Handoff Report**:

### Security Report
- **Phase:** Fase 1 | Fase 3 | Fase 4 | Bugfix
- **Status:** pass | fail | partial
- **Threat Model Completed:** yes | no
- **OWASP Top 10 Status:** [summary table]
- **ASVS Level 2 Coverage:** [percentage of controls met]
- **Critical Findings:** 0 | [count with list]
- **Attention Findings:** 0 | [count with list]
- **Zero-Tolerance Checklist:** [X/14 passed]
- **Verdict:** pass | fail | partial
- **Gaps:** [list of findings with remediation, or "none"]
- **Recommendation:** [proceed / block / fix and re-review]

### Downstream Context
- **Security Decisions:** [security patterns established, or "none"]
- **Integration Points:** [where downstream work should connect to security requirements, or "none"]
- **Assumptions:** [security assumptions to verify, or "none"]
- **Warnings:** [security gotchas for downstream agents, or "none"]
