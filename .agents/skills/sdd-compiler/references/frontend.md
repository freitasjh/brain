# Frontend Architecture & Multi-Framework Patterns

## 1. Framework Selection & Core Paradigms
The frontend framework MUST be explicitly defined in the PLAN. Once selected, the following strict paradigms apply:

- **If React**: 
  - Functional components and Hooks are MANDATORY. Class components are forbidden.
  - Strict Mode MUST be enabled.
  - State Management: Use **Zustand** (preferred for modern/lightweight) or **Redux Toolkit** (for complex enterprise state).
- **If Vue (3.x)**:
  - Composition API (`<script setup>`) is MANDATORY. Options API is forbidden for new code.
  - State Management: Use **Pinia**.
- **If Angular (15+)**:
  - Standalone Components are RECOMMENDED over NgModules for new architectures.
  - State Management: Use **RxJS Services** (default) or **NgRx** (for Redux-pattern).
  - Strict TypeScript mode MUST be enabled.
- **If Vanilla JS / HTMX**:
  - For SSR-heavy backends (e.g., Go/Templ, Python/Django, Java/Thymeleaf).
  - Minimal client-side state. Use ES6 Modules and native DOM APIs. No bloated jQuery.

## 2. Structural Architecture (Feature-Sliced Design)
Regardless of the framework, DO NOT group files purely by technical type (e.g., all components in one folder). Group them by **Business Feature** to mirror the Backend's Bounded Contexts.
```text
src/
├── core/             # Global instances (HTTP Client, Router, Auth Guards)
├── shared/           # Generic UI components (Buttons, Modals, Layouts)
└── features/         # Business Modules
    ├── billing/      # Bounded Context: Billing
    │   ├── components/
    │   ├── store / services/
    │   └── types.ts  # Interfaces
    └── identity/     # Bounded Context: IAM
```

## 3. API Integration & Resiliency
- **HTTP Client**: Axios or native `fetch` wrapper.
- **Interceptors Required**:
  - **Request**: MUST inject `Authorization: Bearer <token>`.
  - **Request**: MUST inject `X-Correlation-ID` (UUID) to trace the request to the backend.
  - **Response (401)**: MUST intercept 401 Unauthorized errors, pause the queue, execute the Refresh Token silent request, and replay failed requests.
- **Idempotency**: Mutating requests (POST/PATCH) involving payments or critical states MUST generate and send an `Idempotency-Key` header.

## 4. Security & Session Handling
- **Token Storage**: Tokens MUST NOT be stored in `localStorage` if vulnerable to XSS. Prefer `HttpOnly` cookies set by the backend. If standard JWT is used, store it in memory (App State) and use `localStorage` ONLY for the opaque Refresh Token.
- **Routing**: Global Router Guards/Middleware MUST verify token validity and RBAC permissions before rendering internal routes.

## 5. UI/UX Libraries & Styling
- **Component Libraries**: 
  - React: Radix UI, Shadcn/ui, or MUI.
  - Vue: PrimeVue or Vuetify.
  - Angular: Angular Material or PrimeNG.
- **Styling**: **Tailwind CSS** is universally recommended for utility-first styling. Avoid custom CSS/SCSS unless strictly necessary.

## 6. Error Handling & Observability
- **Global Error Boundary/Handler**: Unhandled component exceptions MUST be caught globally (e.g., React Error Boundaries, Vue `app.config.errorHandler`) and sent to an APM (e.g., Sentry, Datadog).
- **Graceful Degradation**: If an API fails, the UI MUST display skeleton loaders or fallback states, never a raw error string.

## 7. Testing Strategy
- **Unit Testing**: **Vitest** or **Jest**. Focus on state management, reducers/actions, and complex logic.
- **Component Testing**: React Testing Library / Vue Test Utils. Focus on accessibility and user interactions.
- **E2E Testing**: **Playwright** or **Cypress**. MUST cover Critical User Journeys (e.g., Login, Checkout).
