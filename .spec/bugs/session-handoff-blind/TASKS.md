# TASKS — session-handoff-blind (bugfix B1 análise, sem fix aplicado)

Origem: investigação do handoff/aprendizado (2026-11-08), furos F1+F2.
Predecessor: `.spec/bugs/authority-boost-dominates-search/` (mesma família: nota correta, fora do alcance).

## Bug

1. **F1 — handoff não lê sessão.** `brain hook --event session-start` injeta só `layer=regras` (`crates/brain-cli/src/main.rs:1244-1252`). Uma sessão nova recebe as regras do projeto, nunca "onde paramos".
2. **F2 — a nota de sessão do hook é órfã de projeto.** `note_append_section(..., project_id: None, ...)` (`main.rs:1096-1106`) e `sync_note_chunks(project_id: None)` (`:1182`). Só `sessoes/shared/*` ganha link (`:1135-1142`).

## Evidência (read-only, `data/brain.db` 2887 notas)

- `sessoes/atlas-agenda/2026-11-08` id **2984** → `project_id = NULL`, `note_projects` vazio, 2 seções (dois `session-start`, zero conteúdo).
- Notas de sessão escritas por **agente** no mesmo dia (ids 2988/2991/2992/2995) têm `project_id = 15` e são encontradas sob `project=`.
- `sessoes`: 1264 notas, **470 com `project_id NULL`** (3501 chunks = 37% do corpus).
- Consequência: o log automático é invisível sob filtro `project=`, enquanto a curadoria manual é visível. O handoff depende de o agente ter lembrado de gravar — que é justamente o que o hook deveria cobrir.

## Causa raiz

- F1: `hook_handle` monta o bloco de injeção com duas buscas fixas em `regras` (`:1244`, `:1249`) e um panorama de projeto (`:1228`). Nenhuma leitura da camada `sessoes`. Não existe método no `Store` para listar sessões por projeto — `project_notes` não as acha porque elas não são owned nem linked (`crates/brain-store/src/lib.rs:1073`).
- F2: `project_id` é passado explicitamente como `None` na escrita (`:1099`) e no sync de chunks (`:1182`); o único caso de link é o prefixo literal `sessoes/shared/` (`:1135`).

## Desenho proposto (B2)

### D1 — `Store::recent_session_notes(project: &str, limit: usize)` (novo, read-only)

Uma query, sem N+1, cobrindo os três jeitos de a nota pertencer ao projeto:

```sql
SELECT path, updated_at, content FROM notes n
WHERE n.layer='sessoes'
  AND (n.expires_at IS NULL OR n.expires_at > ?now)
  AND (n.project_id = ?pid
       OR n.path IN (SELECT note_path FROM note_projects WHERE project_id = ?pid)
       OR substr(n.path, 1, length(?prefix)) = ?prefix)
ORDER BY n.updated_at DESC LIMIT ?limit
```

com `?prefix = format!("sessoes/{project}/")`.

- **Por que `substr` e não `LIKE`:** o nome do projeto chega do filesystem/config e não é sanitizado contra `%`/`_`; `LIKE` transformaria um nome com `_` em wildcard e puxaria sessões de outro projeto. `substr` compara literal.
- **Por que o prefixo é obrigatório:** é o que alcança as 470 notas legadas com `project_id NULL` **sem backfill**.
- **Por que o `/` final:** `atlas` ≠ `atlas-agenda`; sem ele, `sessoes/atlas-agenda/...` casaria com o prefixo `sessoes/atlas/`.
- Retorna `Vec<(path, updated_at, content_tail)>` — o tail é calculado no chamador (CLI), não no Store.

### D2 — injeção na sessão (`main.rs`, bloco `if event == "session-start"`)

Depois dos blocos `regras`, antes do `INJECT: (no context found)`:

```
--- Brain context (sessões) ---
sessoes/atlas-agenda/2026-11-08  <últimos ~400 chars, newlines → espaço>
sessoes/atlas-agenda/2026-11-07-<slug>  <...>
```

- `limit = 2`, tail **400 chars** por nota. Orçamento: 2×400 + headers + regras (~720) ≈ 1.6 KB, sob os 4000 chars que o plugin anexa (`hooks/opencode/brain-session.js:38`).
- Quando `project` é `None`: **nada muda** (sem bloco de sessões). O caminho global já tem as regras globais.
- Bloco de sessões é best-effort: erro vira vetor vazio + `eprintln`, nunca falha (RF-07).

### D3 — F2: link da nota de sessão ao próprio projeto

Após `note_append_section`, se `full.starts_with(&format!("sessoes/{}/", project))`:

```rust
if let Err(e) = store.note_link_project(&full, &project) {
    eprintln!("hook: could not link {full} to {project}: {e}");
}
```

- Idempotente por construção (`INSERT OR IGNORE`, `lib.rs:1103`).
- **Link, não posse:** `project_id` continua `None`. Mudar posse alteraria a semântica de `project_notes` owned e contradiz a decisão R4 de `project-shared-search`.
- Os links extras de `sessoes/shared/*` (erp+mobile) ficam como estão — vêm depois.
- Falha de link não derruba o hook (o evento já foi gravado; o pior caso é a nota voltar a ser alcançável só por prefixo).

## Testes necessários (B2)

- [ ] **T1 (Risco M)** in-mem: nota `sessoes/proj-x/2026-01-01` com `project_id NULL` e sem link → `recent_session_notes("proj-x", 5)` **retorna**. Mutação: remover o ramo `substr` → falha.
- [ ] **T2 (Risco A)** isolamento: `recent_session_notes("atlas", 5)` **não** retorna `sessoes/atlas-agenda/...` (prefixo sem `/`) nem `sessoes/outro/...`.
- [ ] **T3 (Risco M)** TTL e ordem: 3 notas com `updated_at` distintos, uma expirada → ordem DESC e expirada fora.
- [ ] **T4 (Risco M)** `search(project=proj-x)` passa a encontrar a nota de sessão **depois** do link do hook (antes: invisível). Mutação: remover o `note_link_project` → falha.
- [ ] **T5 (Risco B)** link idempotente: dois eventos na mesma data → **uma** linha em `note_projects`, nota com **duas** seções.
- [ ] **T6 (Risco B)** sem `project` (ou projeto irresolúvel): saída do hook inalterada, zero bloco `sessões`.
- [ ] **T7 (Risco B)** orçamento: conteúdo de 5 KB no tail → saída truncada em 400 chars por nota, plugin não estoura.
- [ ] **T8** `cargo test --workspace` (baseline 534) + `clippy --all-targets -D warnings` + `pytest` 124/2 legadas.

## Riscos

- **R1 (M)** — injeção maior empurra o contexto útil para fora dos 4000 chars do plugin. Mitigação: 2 notas × 400 chars, medido no teste T7.
- **R2 (A)** — prefixo mal fechado vaza sessão de outro projeto (`atlas` × `atlas-agenda`). Mitigação: `/` obrigatório + T2.
- **R3 (M)** — link em massa se alguém rodar backfill sem escopo. Fora deste batch (ver pergunta ao usuário).
- **R4 (B)** — `sessoes/sessoes/projeto/atlas-financeiro/...` existe no corpus (path escrito por agente duplicando a layer); o prefixo do hook não o alcança. Não é regressão, é dado legado; documentar.
- **R5 (B)** — `content` inteiro em memória por linha: 2 notas, ~3 KB cada. Irrelevante.

## Verificação B1 (nada alterado)

- [x] `main.rs:932-1269` (hook inteiro), `:1244-1259` (injeção), `:1096-1142` (append + link shared)
- [x] `lib.rs:1098-1105` (`note_link_project`, `INSERT OR IGNORE`), `:1277-1288` (`note_append_section`), `:1798-1805` (`recent_paths`, estilo de query), `:1073` (`project_notes`)
- [x] `hooks/opencode/brain-session.js:38,146-165` (budget 4000, três hooks, sem nudge)
- [x] SQL read-only: 1264 notas `sessoes`, 470 NULL, id 2984 NULL, ids 2988-2995 com project 15
- [x] nenhuma escrita em `data/brain.db`, nenhum processo tocado

## Resultado B2+B3 (aplicado)

- [x] **T1** prefixo alcança nota NULL — mutação (remover `substr`) → falha
- [x] **T2** isolamento `atlas` ≠ `atlas-agenda` — mutação (prefixo sem `/`) → falha
- [x] **T3** TTL + ordem — mutação (remover `updated_at DESC`) → falha, **após** ajustar o teste para não empatar no mesmo segundo
- [x] **T4** `search(project=)` encontra a nota depois do link — mutação (sem link) → falha
- [x] **T5** idempotência: 2 eventos → 1 linha em `note_projects`, 2 seções
- [x] **T6** bloco de sessões injetado; `INJECT: (no context found)` não contradiz
- [x] **T7** tail ≤ 400 chars
- [x] **T8** sem projeto resolvível → saída inalterada
- [x] **T9** curadoria vence o eco; heading curado nomeia o evento (mata a mutação substring)
- [x] **T10** panorama exclui `sessoes` (sem duplicar a nota do dia)
- [x] **T11** projeto não registrado: prefixo resgata (mutação: early return → falha)
- [x] **+2** `hook_section` ↔ `session_digest` pinado nos 3 eventos; heading `HH:MM session-end` **sem `id=`** é curadoria

Desvios do plano, todos medidos: `ORDER BY updated_at DESC, id DESC` (resolução de 1 s);
`substr` + `Option<i64>` no lugar do early return de `project_get` (134 notas em 12
prefixos sem projeto em produção); `SESSION_SLOTS + 1` no fetch para o `retain` não
custar slot; digest por conteúdo (`Curated`/`LogOnly`) em vez de por nome.

Harness: `cargo test --workspace` **550 passed / 0 failed** · clippy 0 · build 0 ·
pytest 124 + 2 legadas. Versão permanece **0.12.0** (não deployada).

**Operacional:** o plugin instalado chama `target/release/brain` — a mudança só chega
ao IDE depois de `cargo build --release` + `brain setup opencode`.

**Segue aberto (F3-F7):** `session-end` sem resumo, `tool-result` morto, `sessoes`
poluindo ranking (37% dos chunks), spool sem leitor, drift do plugin.
