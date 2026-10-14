# Aprendizado no brain (via MCP)

Regra sempre-ativa: buscar antes de codar, gravar depois de decidir —
via MCP `brain_search` / `brain_store`, nunca via CLI.

## Antes de codar

Execute o roteiro de busca em 5 passos da skill brain
(`.agents/skills/brain/SKILL.md`): query literal → afunila por
layer/scope → `brain_read` integral → tabela decisão→fonte.
Busca vazia declara-se "sem contexto"; nunca bloqueia.

## Depois de aprender

Se o turno gerou algo durável (decisão, bug com causa raiz,
convenção, armadilha, contrato), grave via `brain_store`:
`sessoes/<proj>/<data>-<assunto>` + `estudos/…` (se novo,
com frontmatter) + `regras|arquitetura/…` (se durável).
path relativo sem repetir layer/scope; scope `projetos`
se específico, `global` se universal; projeto ativo via git/config.
Só leitura ou trivial: não grave nada.
