# Backend Rules

## Code Style
- Use records for DTOs
- **Lombok** Esta proibido o uso, não deve ser utilizado.
  - Alternativa: escrever construtores, getters e setters manualmente, ou usar `record` para DTOs.
- Services inject repositories, never the other way
- Controllers are thin — delegate to services
- Package by feature (`agent`, `chat`, `tool`), not by layer

## LangChain4j
- Tools are Spring `@Component` classes with `@Tool`-annotated methods
- Agent creation uses `AiServices.builder(interface.class).chatLanguageModel(model).tools(...).build()`
- Store `ChatMemory` per conversation in DB via `ChatMemoryStore` interface

## Flyway
- Never disable Flyway in production
- Dev profiles may use `flyway.baseline-on-migrate: true`
- All schema changes go through migration files in `db/migration/`

## Testing

### Obrigatório — Todo desenvolvimento DEVE incluir testes

### Testes Unitários (JUnit 5 + Mockito)
- Todo **service** deve ter teste unitário:
  - Fluxo principal (happy path)
  - Fluxos de erro (validações, entidade não encontrada, conflitos)
  - Casos de borda (edge cases)
- Todo **controller** deve testar:
  - HTTP status codes (200, 201, 400, 401, 404, 500)
  - Validação de entrada (`@Valid`, `@NotNull`, etc.)
  - Serialização JSON (resposta igual ao DTO esperado)
- Todo **JwtService** / utilitário crítico deve ter teste unitário
- Use Mockito para mockar dependências: `@ExtendWith(MockitoExtension.class)`

### Testes de Integração (Testcontainers + MySQL real)
- Toda **repository** deve ter teste de integração:
  - Validar SQL nativo e queries do Spring Data
  - Operações CRUD completas
  - Constraints (unique, foreign key, not null)
- Todo **fluxo completo (controller → service → repository)** deve ter pelo menos 1 teste de integração
- Configuração padrão:
  ```java
  @SpringBootTest
  @AutoConfigureTestDatabase(replace = NONE)
  @Testcontainers
  ```
- Use `@DynamicPropertySource` para configurar datasource do container MySQL
- `application-test.properties` ou `@TestPropertySource` para configs específicas

### Estrutura de Testes
```
src/test/java/com/chatbot/
├── auth/
│   ├── AuthServiceTest.java          // unitário
│   ├── AuthControllerTest.java       // unitário (MockMvc)
│   └── AuthRepositoryIntegrationTest.java  // integração
├── config/
│   └── JwtServiceTest.java           // unitário
└── exception/
    └── GlobalExceptionHandlerTest.java    // unitário (MockMvc)
```

### Cobertura
- Mínima **80%** nas classes novas/alteredas
- Verificar com Jacoco: `mvn verify -Pintegration-test`
- Não aceitar cobertura abaixo do mínimo — criar testes até atingir

### Execução
```bash
sdk use java 21.0.10-zulu         # Java 21 obrigatório
mvn test                          # unit tests only (exclui *IntegrationTest)
mvn verify -Pintegration-test     # unit + integration tests (requer Docker)
```
- Os testes DEVEM passar antes de qualquer commit

## Exact Commands
```bash
sdk use java 21.0.10-zulu         # sempre trocar para Java 21 primeiro
mvn spring-boot:run -Dspring-boot.run.profiles=dev
mvn test                          # unit tests
mvn verify -Pintegration-test     # full checks (requer Docker)
```
