# Frontend Rules

## Code Style
- Always `<script setup lang="ts">` — no Options API
- Pinia stores use the Composition API style (setup stores), not Options stores
- Components go under `src/components/<domain>/`, views under `src/views/`
- API calls live in `src/services/api.ts`, not scattered across components

## PrimeVue
- Use PrimeVue 4 with the Aura theme
- Import components globally in `main.ts` — no per-component imports
- Use `PrimeVue.defineTheme()` for custom tokens if needed

## State Management
- Chat state: messages, loading, streaming flag → `useChatStore`
- Agent/Tool config state → `useAgentStore`
- No component-scoped reactive state for data that survives route changes

## API Layer
- All calls go through `api.ts` using `fetch` (no Axios needed)
- Backend base URL from `import.meta.env.VITE_API_BASE` (default `/api`)
- Types are manually defined in `src/services/api.ts`

## Testing

### Obrigatório — Todo desenvolvimento DEVE incluir testes

### Testes Unitários (Vitest + Vue Testing Library)
- Toda **store (Pinia)** deve testar:
  - Actions (chamadas à API, mutations no state)
  - Getters (cálculos derivados do state)
  - Estado inicial e após mutações
- Toda **view/página** deve testar:
  - Renderização com dados mockados
  - Interações do usuário (cliques, submissão de formulários)
  - Estados de loading, vazio e erro
- Todo **serviço API** deve testar:
  - Chamadas HTTP (método, URL, headers, body)
  - Tratamento de erros (timeout, 4xx, 5xx)
  - Parsing de resposta JSON

### Testes de Componente
- Renderização com diferentes props
- Eventos emitidos (`emitted()`)
- Slots e conteúdo condicional
- Integração com PrimeVue componentes

### Estrutura de Testes
```
src/
├── components/__tests__/
│   └── ChatMessage.test.ts
├── views/__tests__/
│   └── LoginView.test.ts
├── stores/__tests__/
│   └── chatStore.test.ts
└── services/__tests__/
    └── api.test.ts
```

### Cobertura
- Mínima **70%** nas novas funções/componentes
- Verificar com: `npx vitest --coverage`
- Não aceitar cobertura abaixo do mínimo

### Execução
```bash
npx vitest                    # unit tests (watch mode)
npx vitest run                # unit tests (single run)
npx vitest --coverage          # unit tests + coverage report
```
- Os testes DEVEM passar antes de qualquer commit

## Exact Commands
```bash
npm install     # first time
npm run dev     # dev server on :5173, proxies /api to backend
npm run build   # production build → dist/
npm run lint    # eslint + prettier check
npm run typecheck  # vue-tsc --noEmit
```
