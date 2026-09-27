# Implementation Plan: [Feature/System Name]

## 1. Context & SPEC Traceability
- **Target Specification**: `.spec/[feature-name]/SPEC.md`
- **Goal**: Translate the SPEC into a strictly determinist, risk-aware, and executable engineering plan. 

### 1.1 SPEC Validation Report
- **Status**: [e.g., Validated with Warnings | Strictly Validated]
- **Identified Gaps/Warnings**: 
  - [e.g., Event `[event.name]` does not define payload structure]
- **Assumptions Made**: [Explicitly state assumptions adopted to bypass any gaps. If an event is incomplete, document assumed fields here to unblock execution.]

## 2. Technical Strategy
### 2.1 Backend Strategy
- **Framework**: [Choice]
- **Architectural Pattern**: [e.g., Modular Monolith, Hexagonal Architecture]

### 2.2 Frontend Strategy
- **Framework**: [Choice]
- **State Management**: [e.g., Pinia, Zustand]

### 2.3 Infrastructure Strategy
- **Deployment Model**: [Choice]
- **CI/CD**: [Pipeline steps required]

## 3. Architecture Realization & Constraint Mapping
| SPEC Reference | Constraint | Enforcement Point | Failure Mode | Implementation Strategy |
|----------------|------------|-------------------|--------------|-------------------------|
| `[e.g., ADR-001]` | **Multi-Tenancy** | [e.g., Persistence Layer] | [e.g., Cross-tenant data leakage] | [e.g., Spring `AbstractRoutingDataSource` with ThreadLocal] |

## 4. Module & Code Structure
| Module Name | Responsibilities | Boundaries & Communication |
|-------------|------------------|----------------------------|
| `[module]`  |                  |                            |

## 5. Risks & Complexity Analysis
- **🔥 CRITICAL RISK: Multi-Tenant Data Leakage**
  - **Mitigation**: Mandatory integration tests validating tenant isolation.

## 6. Implementation Phases & Dependencies
### Phase 1: Foundation (Modules: `core.infra`, `core.security`)
- **Dependencies**: None.
- **Scope**: Infra setup, IAM integration, Tenant provisioning base.

## 7. Critical Execution Path
*Minimum sequence of tasks required to deliver the MVP core flow. MUST ignore secondary integrations and UI.*
- `[Task ID]` → `[Task ID]` → `[Task ID]` → `[Task ID]`

## 8. Atomic Task Breakdown & Execution Graph
*Tasks ordered sequentially. Each task MUST have an ID and explicit technical dependencies.*

### Backend Tasks (Phase X – [Module Name])
- **[BE-01]** [ ] `[Action: e.g., Implement TenantContext extractor]`
  - **Depends on**: `[None]`
  - **Layer**: `[Infrastructure | Domain | Data | Application | Interface | Integration | Frontend]`
  - **Artifact**: `[e.g., TenantInterceptor]`
  - **Enforcement**: `[e.g., Reject requests without valid JWT tenant claim]`
  - **Constraint Ref**: `[ADR-001]`
  - **Framework Detail**: `[e.g., Spring HandlerInterceptorAdapter]`

- **[BE-02]** [ ] `[Action: e.g., Write Integration Test for Tenant Isolation]`
  - **Depends on**: `[BE-01]`
  - **Layer**: `[Infrastructure]`
  - **Artifact**: `[e.g., TenantInterceptorTest]`
  - **Enforcement**: `[Verify isolation and failure modes]`
  - **Constraint Ref**: `[ADR-001]`
  - **Framework Detail**: `[e.g., JUnit 5 + Testcontainers]`

### Frontend Tasks (Phase X – [Module Name])
- **[FE-01]** [ ] `[Action: e.g., Create API Service Client]`
  - **Depends on**: `[None]`
  - **Layer**: `[Frontend]`
  - **Artifact**: `[e.g., Axios Interceptor]`
  - **Enforcement**: `[e.g., Catch 401 and trigger silent token refresh]`
  - **Constraint Ref**: `[Section X - Security]`
  - **Framework Detail**: `[e.g., Axios.interceptors.response]`

## 9. Event Implementation Mapping (MANDATORY)
*Every event defined in the SPEC MUST appear in this table and generate 4 specific tasks.*

| Event Name | Producer Task ID | Outbox Task ID | Broker Task ID | Consumer Task ID |
|------------|------------------|----------------|----------------|------------------|
| `[event.name]` | `[e.g., BE-05]` | `[e.g., BE-06]` | `[e.g., BE-09]` | `[e.g., BE-12 or N/A]` |

## 10. Execution Readiness Checklist
- [ ] All tasks have valid dependencies (no circular references).
- [ ] All SPEC events generated the 4 mandatory tasks (Producer, Outbox, Broker, Consumer).
- [ ] Mandatory testing tasks created for critical constraints (Multi-tenancy, Idempotency, Events).
- [ ] Base Infra tasks explicitly defined (Tenant context, IAM, DB connections).
- [ ] Critical Execution Path maps only the core flow.
