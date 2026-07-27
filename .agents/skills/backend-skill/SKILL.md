---
name: backend-skill
description: Build Spring Boot 3.4 + LangChain4j backend — entities, repositories, services, controllers, Flyway migrations, and tests
license: MIT
compatibility: opencode
metadata:
  stack: java-spring
  lang: java
---

## What I do
- Create JPA entities with manual constructors, getters, and setters (no Lombok), using `@PrePersist`/`@PreUpdate` lifecycle hooks
- Create Spring Data JPA repositories with custom finders
- Create service classes with explicit constructor injection — thin controllers, logic in services
- Create REST controllers at `/api/<domain>` with proper HTTP status codes
- Create Flyway migration files in `src/main/resources/db/migration/V<version>__<name>.sql`
- Create DTOs as Java records with `jakarta.validation` annotations
- Create LangChain4j `@Tool` beans as `@Component` classes with `@Tool`-annotated methods
- Set up AiServices builder for agent creation
- Create `@SpringBootTest` + Testcontainers integration tests

## When to use me
Use when creating or modifying backend code: entities, repositories, services, controllers, DTOs, migrations, LangChain4j agents/tools, or tests.

## Conventions
- Package by feature (`agent`, `chat`, `tool`), not by layer
- Entities: `@Entity @Table(name = "snake_case_plural")` with `@Id @GeneratedValue(IDENTITY)`
- All timestamp columns: `created_at` and `updated_at` with `@PrePersist`/`@PreUpdate`
- Services: inject repositories, never inject services into controllers directly — use DTOs
- Controllers: `@RequestMapping("/api/<domain>")`, return DTOs, not entities
- Migrations: `ddl-auto: validate`, never edit applied migrations
- LangChain4j config goes in `config/LangChain4jConfig.java`

## Exact commands
```bash
sdk use java 21.0.10-zulu
mvn spring-boot:run -Dspring-boot.run.profiles=dev
mvn test
mvn verify -Pintegration-test
```
