# BUG — viewer HTML não é servido quando `brain serve` roda fora da raiz do repo

**Tipo:** Bugfix (backend/CLI — `brain-web` + `brain-cli`)
**Severidade:** Alta (viewer inutilizável após `cargo install` / systemd)
**Data:** 2026-10-19

## Bug

`brain serve --port 8322` responde `200` em `/api/status`, `/api/search`, `/api/read`,
`/api/list`, `/api/graph`, `/api/events`, mas `/` — e qualquer rota não-API — retorna
`404 viewer/index.html not found` quando o processo não tem `cwd = raiz do repo`.

## Reprodução

1. `cargo build --workspace`
2. `cd /tmp && /caminho/para/brain --db /caminho/data/brain.db serve --port 8399`
3. `curl -si http://localhost:8399/` → 404 `viewer/index.html not found`
4. `curl -s -o /dev/null -w '%{http_code}' http://localhost:8399/api/status` → 200

Contraprova no ambiente atual: processo em `:8322` com `cwd = /home/joao/fusionlf/wsProjeto/brain`
serviu `/` → `200 text/html` (49.437 bytes, `viewer/index.html`). Ou seja: HTML funciona
**apenas** porque o cwd coincide com a raiz do repo.

## Causa raiz

HTML não é embutido no binário — é resolvido por caminho **relativo ao cwd** em runtime:

- `crates/brain-web/src/lib.rs:22` — `ServeDir::new("viewer")`
- `crates/brain-web/src/lib.rs:36` — `tokio::fs::read_to_string("viewer/index.html")`
- `crates/brain-cli/src/setup.rs:132-138` — unit systemd gerada **sem** `WorkingDirectory`,
  então o serviço do usuário sobe com cwd=`$HOME` → HTML 404, API ok.

Toda rota `/api/*` é absoluta no router (`crates/brain-web/src/lib.rs:24-30`), por isso
só o HTML quebra. `cargo install --path crates/brain-cli` instala só o binário — o
diretório `viewer/` não vem junto.

## Solução proposta

Hermetizar o viewer: embutir `viewer/index.html` em tempo de compilação e usá-lo como
fonte primária; manter `ServeDir` como fallback de dev (arquivo editado sem rebuild).

- `crates/brain-web/src/lib.rs`: `const VIEWER_INDEX: &str = include_str!("../../../viewer/index.html");`
  - `serve_index()` devolve `VIEWER_INDEX` (sem IO de disco).
  - rota `/` e fallback de SPA continuam servindo o índice embutido.
- `crates/brain-cli/src/setup.rs`: opcional — adicionar `WorkingDirectory` à unit
  gerada **não** é suficiente sozinho; o embed é o que fecha `cargo install`.
- Atualizar doc (`README.md` §Iniciar servidores) se o comportamento mudar.

## Testes necessários

1. `brain-web` unit: `serve_index()` retorna 200 + `text/html` **independente de cwd**
   (rodar o teste a partir de `/tmp` com `ServeDir` inacessível).
2. Guard de regressão: falha se `read_to_string("viewer/index.html")` voltar a ser o
   caminho primário (padrão source-scan já usado em `zero_vector_guard.rs`).
3. `setup.rs` unit: unit systemd gerada continua válida; se `WorkingDirectory` for
   adicionado, teste prende a linha.
4. Harness scratch (H3): binário copiado para `/tmp`, `serve` com cwd=`/tmp`,
   `curl /` → 200 HTML e `/api/status` → 200.
5. `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings`.

## Riscos

- Mudanças em `viewer/index.html` passam a exigir rebuild do binário (trade-off aceito;
  `include_str!` é o padrão já usado em `setup.rs` para hooks Kiro/OpenCode).
- `test_serve_index_fallback` (`crates/brain-web/src/lib.rs:1067`) assume cwd=crate dir e
  404; precisa ser reescrito para a nova semântica.
- Tamanho do binário +~50 KB — irrelevante.

## Débito relacionado

`brain setup systemd` gerou unit com `WorkingDirectory` inexistente no gerador atual
(`crates/brain-cli/src/setup.rs:132-138`), o que torna o bug silencioso em instalações
novas.

## Nota da revisão (2026-10-19, iteração 1)

A linha "manter `ServeDir` como fallback de dev" acima **não** foi implementada assim,
por decisão da revisão: o `ServeDir` foi **removido**, não gateado por perfil de build.
Ele servia *qualquer* arquivo que o cwd tivesse sob `viewer/` e, num checkout do repo,
isso inclui `viewer/server.py` (viewer Python legado) — `GET /server.py` respondia
`200 text/plain` (10.960 B) no bind de produção `0.0.0.0:8322`. Um gate de build
manteria a classe viva em todo `cargo run`. Consequência: o fluxo de dev "editar o HTML
sem rebuild" acabou; `include_str!` já era o trade-off aceito. Ver
`crates/brain-web/src/lib.rs` (`router`, `VIEWER_INDEX`) e
`crates/brain-web/tests/{unmatched_route,viewer_embed_guard,viewer_index_cwd}.rs`.
