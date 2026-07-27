# Database Rules

## Naming Conventions
- Tables: `snake_case` plural (`agents`, `tools`, `conversations`, `messages`)
- Columns: `snake_case` (`created_at`, `agent_id`, `tool_name`)
- Primary keys: `id BIGINT AUTO_INCREMENT`
- Foreign keys: `<referenced_table_singular>_id` (`agent_id`, `conversation_id`)
- Join table: `agent_tools` (agent_id, tool_id + PK)

## Migration Rules
- Never edit a migration that has already been applied
- Name files: `V<version>__<descriptive_name>.sql`
- Always include both `CREATE` and `CREATE INDEX` in the same migration
- Use `AFTER` and `NOT NULL` with defaults explicitly

## Key Tables
- `agents` — type, name, model_config (JSON), system_prompt, active
- `tools` — name, description, method_signature (JSON schema), enabled
- `agent_tools` — many-to-many join
- `conversations` — agent_id, title, created_at, updated_at
- `messages` — conversation_id, role (USER/ASSISTANT/TOOL), content, tokens, created_at
