use anyhow::Result;
use brain_core::{sanitize_relative_path, validate_layer, validate_scope, LAYERS_WITH_SCOPE};
use brain_mcp::{ChunkSyncInput, embed_chunks, sync_note_chunks};
use brain_store::{NoteEmbed, Store};
use clap::{CommandFactory, Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;

mod config;
mod legacy_import;
mod resolve;
mod setup;

/// US-01.1 AC1. The SSE port: `--port` when given, else `BRAIN_PORT`, else 8321.
///
/// A function rather than a clap `default_value_t` because the default has to be
/// an **environment variable**, and clap's `default_value` cannot read one for a
/// subcommand of `Cli` that already has `--db env=...`. `serve-mcp` spells the
/// same constant in its own `default_value_t`; both are documented in `AGENTS.md`
/// as `BRAIN_PORT=8321`, and this is the one place that actually honours the
/// variable rather than only naming it.
fn default_mcp_port() -> u16 {
    std::env::var("BRAIN_PORT").ok().and_then(|v| v.trim().parse::<u16>().ok()).unwrap_or(8321)
}

/// TD-010. Write one line to stdout, treating a closed stdout as a normal ending.
///
/// `println!` panics when the write fails, and for a CLI the most common reason a
/// stdout write fails is that the reader went away: `brain recent | head -1` closes
/// the pipe while we are still writing, `println!` unwraps the resulting `EPIPE`, and
/// the process dies with exit 101 and a panic on stderr. That is wrong twice over.
/// Closing the pipe early is the ordinary Unix contract — `| head`, `| grep -m`, `| less`
/// that stops paging all do it — and the exit code is precisely what a script tests,
/// so a pipeline that worked reported failure.
///
/// Only `BrokenPipe` is absorbed, and it exits 0 silently, because there is by then no
/// consumer left for the bytes and nothing useful left to report. **Every other write
/// error keeps `println!`'s behaviour — a panic** — so a genuine write failure (a full
/// disk, a write-only descriptor) is still loud instead of being silently truncated
/// into a `0` exit that a caller would read as success. The asymmetry is the point:
/// this macro widens the set of *successful* exits by exactly the one case that is not
/// a failure, and by nothing else.
///
/// A single `println!` of a large value is enough to trigger this: the value is handed
/// to one `write_fmt`, and once it exceeds the pipe buffer the tail of that one write
/// lands on a closed pipe.
macro_rules! outln {
    ($($arg:tt)*) => {{
        if let Err(e) = writeln!(std::io::stdout(), $($arg)*) {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                std::process::exit(0);
            }
            panic!("failed writing to stdout: {e}");
        }
    }};
}

#[derive(Parser)]
#[command(name="brain", version, after_help = AFTER_HELP)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    #[arg(long, env="BRAIN_DB_PATH", default_value="./data/brain.db")]
    db: String,
}

/// Footer of `brain --help` (RF-04). Three commands covering the whole cycle —
/// health, write, read back — because a first-time operator needs a starting point,
/// and a command list is a *classification*, not a tutorial.
const AFTER_HELP: &str = "\
Exemplos:
  brain ping
  brain store regras naming \"## Regra\" --scope global
  brain search \"termo\" --explain";

// The 5 help groups, and only command *names* (ADR-01 v2).
//
// The one-liner of every command is read from the clap derive at render time and is
// never written here: a literal in this table would be a second copy of the text,
// and the two would drift the first time somebody reworded a doc-comment. The
// mapping is names, or it is not a mapping.
//
// A name that is not a subcommand of `brain` is a `panic!` in `root_help` rather
// than a skipped line, because a skipped line is a hole in the help that nobody
// notices until an operator reports that a command does not exist.
const HELP_GROUPS: &[(&str, &[&str])] = &[
    ("Uso comum", &["ping", "search", "read", "store", "recent", "status"]),
    ("Memória", &["checkpoints", "restore", "delete", "export", "backup"]),
    ("Manutenção", &["forget-sweep", "reindex", "migrate"]),
    ("Servidor", &["server", "serve-mcp", "serve", "hook"]),
    ("Projetos e setup", &["project", "setup"]),
];

/// Title of the section that catches any subcommand not named in [`HELP_GROUPS`].
///
/// It exists to make a forgotten command *visible* instead of invisible. A command
/// that drops out of the groups is still a command, and the alternative — omitting
/// it — is a help page that lies by omission. Today the only member is clap's own
/// `help`.
const UNGROUPED_TITLE: &str = "Outros";

/// Whether `argv` (already past the program name) asks for the *root* help and
/// nothing else.
///
/// Scoped to a single argument on purpose: `brain help search` and `brain --db X
/// --help` are clap's to answer, and this function must not swallow them — the
/// renderer only knows how to print the root page.
///
/// A lone `help` is intercepted alongside `--help`/`-h` (RF-06) because it is the
/// *same request*: an operator typing `brain help` wants the page, not a listing of
/// the ways to get the page. `brain help <cmd>` is a different request and stays
/// with clap.
///
/// Two root pages therefore exist by design — this one and clap's flat list for
/// `brain --db X --help`. That is a choice about consistency (one custom page, on the
/// minimum argv), not a gap; it is pinned by
/// `a_root_help_request_with_other_arguments_is_still_claps` so that changing it
/// cannot happen by accident.
fn is_root_help_request(argv: &[String]) -> bool {
    matches!(argv, [only] if only == "--help" || only == "-h" || only == "help")
}

/// One aligned block: a title, then `  <name>  <description>` rows.
///
/// `longest` is passed in rather than measured here so that every block on the page
/// shares one column. Measuring per block is what makes clap's own output look
/// ragged, and the point of this page is a single vertical read.
fn write_block(out: &mut String, title: &str, rows: &[(String, String)], longest: usize) {
    if rows.is_empty() {
        return;
    }
    out.push('\n');
    out.push_str(title);
    out.push_str(":\n");
    for (name, description) in rows {
        out.push_str("  ");
        out.push_str(name);
        if description.is_empty() {
            out.push('\n');
            continue;
        }
        // Two spaces of gutter after the widest name in the page. `chars().count()`
        // is the right measure: the one-liners are Portuguese and a multi-byte
        // character occupies one terminal cell.
        for _ in name.chars().count()..longest + 2 {
            out.push(' ');
        }
        out.push_str(description);
        out.push('\n');
    }
}

/// The root help page, per ADR-01 v2.
///
/// clap 4.6 has no mechanism for this: `next_help_heading` on a subcommand is read
/// only by the subcommand's *own* help (it renames that command's `Options:` to the
/// group title) and never by the parent's, which writes a single `Commands:` section
/// — measured in `clap_builder-4*/src/output/help_template.rs:394-396` and
/// `:403-415`, and recorded in `tests/help_overview.rs`. So the grouping is ours.
///
/// What is *not* ours: the usage line, every name, every one-liner, every option
/// with its env and default, and the footer all come back out of `Cli::command()`.
/// Only the group titles and the order are added here. `brain <cmd> --help` never
/// reaches this function and stays 100% clap.
fn root_help() -> String {
    let mut cmd = Cli::command();
    // `build()` is what materialises clap's own `--help`/`--version` args; without it
    // the Options block below would list `--db` and nothing else.
    cmd.build();

    // Collect every block before printing any of it: the column width is a property
    // of the whole page, so printing as we went would need a second pass to align.
    let mut sections: Vec<(&str, Vec<(String, String)>)> = Vec::new();
    for (title, names) in HELP_GROUPS {
        let rows = names
            .iter()
            .map(|name| {
                let sub = cmd.find_subcommand(name).unwrap_or_else(|| {
                    panic!("HELP_GROUPS names `{name}` under `{title}`, and it is not a subcommand of `brain`")
                });
                (sub.get_name().to_string(), about_of(sub))
            })
            .collect();
        sections.push((title, rows));
    }

    // Anything a group forgot still gets printed, under a heading of its own. A
    // command that vanishes from the help is worse than one that looks misplaced.
    let ungrouped: Vec<(String, String)> = cmd
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set())
        .filter(|sub| !HELP_GROUPS.iter().any(|(_, names)| names.contains(&sub.get_name())))
        .map(|sub| (sub.get_name().to_string(), about_of(sub)))
        .collect();
    sections.push((UNGROUPED_TITLE, ungrouped));

    let longest = sections
        .iter()
        .flat_map(|(_, rows)| rows.iter())
        .map(|(name, _)| name.chars().count())
        .max()
        .unwrap_or(2)
        .max(2);

    // `render_usage` already emits the `Usage: ` prefix, so it is used verbatim.
    let mut out = format!("{}\n", cmd.render_usage());
    for (title, rows) in &sections {
        write_block(&mut out, title, rows, longest);
    }

    let options: Vec<(String, String)> = cmd
        .get_arguments()
        .filter(|a| !a.is_hide_set())
        .map(|a| option_row(a, &|key| std::env::var(key).unwrap_or_default()))
        .collect();
    let options_longest = options
        .iter()
        .map(|(name, _)| name.chars().count())
        .max()
        .unwrap_or(2)
        .max(2);
    write_block(&mut out, "Options", &options, options_longest);

    if let Some(after) = cmd.get_after_help() {
        out.push('\n');
        out.push_str(after.to_string().trim_end());
        out.push('\n');
    }
    out
}

/// A subcommand's one-liner, straight from the derive.
///
/// `about` and not `long_about`: `long_about` is the paragraph the operator reads
/// *after* choosing the command, and pasting it into the root list is the exact
/// defect brain-help-visual was opened to remove.
fn about_of(sub: &clap::Command) -> String {
    sub.get_about().map(|a| a.to_string()).unwrap_or_default()
}

/// One option as `(left column, right column)`, with clap's env and default
/// annotations — read from the `Arg`, never written as a literal, so a new global
/// option appears here without anyone remembering this function.
///
/// `resolve` reads the environment rather than calling `std::env::var` inline. That
/// is not a testing seam added for its own sake: `set_var` is `unsafe` in edition
/// 2024 and mutates process-global state that every parallel test in this binary
/// shares, so a test that needed a credential-shaped value would either be unsound or
/// need a lock. The *redaction* stays in here — which is the part worth pinning — and
/// only the lookup is injected.
fn option_row(arg: &clap::Arg, resolve: &dyn Fn(&str) -> String) -> (String, String) {
    // `write_block` supplies the two-space indent, so a long-only option carries
    // four more of its own: that lines its `--` up with the `--` of a short+long
    // one, which is what keeps the value column straight across a mix.
    let mut name = match (arg.get_short(), arg.get_long()) {
        (Some(s), Some(l)) => format!("-{s}, --{l}"),
        (Some(s), None) => format!("-{s}"),
        (None, Some(l)) => format!("    --{l}"),
        (None, None) => String::new(),
    };
    if let Some(values) = arg.get_value_names() {
        for value in values {
            name.push_str(&format!(" <{value}>"));
        }
    }

    let mut description = arg.get_help().unwrap_or_default().to_string();
    if let Some(env) = arg.get_env() {
        let env = env.to_string_lossy();
        if !description.is_empty() {
            description.push(' ');
        }
        // The *resolved* value, not the default, because that is the number the
        // operator is trying to read off this line.
        //
        // Redacted, because this goes to stdout and stdout gets pasted into a
        // ticket. Zero effect today — the only global arg is `db`, a path, and
        // `redact_url` returns a credential-free input byte-identical. The point is
        // the future named in F4: a global arg over an env like `BRAIN_OLLAMA_URL`
        // (which accepts `http://user:pass@host`) would otherwise print the password
        // into the help, and a help page travels much further than a log line.
        let resolved = resolve(env.as_ref());
        description.push_str(&format!("[env: {env}={}]", brain_embed::redact_url(&resolved)));
    }
    let defaults: Vec<String> = arg
        .get_default_values()
        .iter()
        .map(|v| v.to_string_lossy().into_owned())
        .collect();
    if !defaults.is_empty() {
        if !description.is_empty() {
            description.push(' ');
        }
        description.push_str(&format!("[default: {}]", defaults.join(" ")));
    }
    (name, description)
}

// Declaration order here IS the printed order of `brain --help`: clap 4 sorts
// neither the subcommands nor the one-liners, it emits them as declared. That is
// the only reason the variants are not in the alphabetical shape they were in
// before brain-help-visual — the 5 groups the ux-designer froze are contiguous
// blocks, and the block order is the group order.
//
// Two clap mechanisms the SPEC assumed would render the group titles, and neither
// does (measured, not guessed — see `tests/help_overview.rs`):
//
// - `next_help_heading` on a variant: clap builds its heading sections from the
//   *parent's* arguments (`clap_builder-4*/src/output/help_template.rs:394-396`),
//   never from a subcommand's, and writes one `Commands:` section (`:403-415`).
//   It is not merely inert here: the heading propagates to the subcommand's *own*
//   args, so `brain reindex --help` titled its flag list "Manutenção:".
// - `flatten_help`: it inlines each subcommand's args into the parent, turning
//   `brain --help` into every flag of all 20 commands. Wrong feature for this.
//
// A doc comment on this enum is also load-bearing to avoid: clap derive reads it as
// the subcommand group's `long_about`, and a multi-line `long_about` switches
// `--help` into the extended template that expands every subcommand in full.
#[derive(Subcommand)]
enum Cmd {
    // ---- Uso comum -------------------------------------------------------------
    /// Verifica se o servidor responde
    Ping,
    #[command(long_about = "\
Busca híbrida: os 4 streams do RRF (vetor, FTS5, entidades, grafo) fundidos com
k=60, mais o bônus de autoridade (arquitetura/regras +0.15, pinned +0.10) e o
truncamento final em --top-k.

Com o Ollama fora, o stream vetorial não pontua e a busca degrada para texto
(--explain mostra os 4 campos de stream). O --explain é só do CLI; a tool MCP
não o expõe.")]
    /// Busca notas por texto e semântica
    Search { query: String, #[arg(long)] layer: Option<String>, #[arg(long)] scope: Option<String>, #[arg(long)] project: Option<String>, #[arg(long)] tag: Option<String>, #[arg(long, default_value_t=5)] top_k: usize, #[arg(long)] explain: bool },
    /// Lê uma nota pelo caminho completo
    Read { layer: String, path: String, #[arg(long)] scope: Option<String> },
    /// Salva uma nota (scope p/ arquitetura/regras/estudos)
    Store { layer: String, path: String, content: String, #[arg(long)] scope: Option<String>, #[arg(long)] project: Option<String>, #[arg(long)] tags: Option<String>, #[arg(long)] pinned: bool, #[arg(long)] expires_at: Option<String> },
    /// Lista as notas alteradas por último
    Recent { #[arg(long, default_value_t=10)] top_k: usize },
    /// Mostra notas, cobertura e fila de embedding
    Status,

    // ---- Memória ----------------------------------------------------------------
    /// Consulta o histórico de alterações
    Checkpoints { #[arg(long, default_value_t=10)] limit: usize },
    /// Restaura uma versão anterior pelo id
    Restore { id: i64 },
    /// Apaga uma nota e seus trechos
    Delete { path: String },
    /// Exporta notas p/ diretório temporário
    Export { #[arg(long, default_value="/tmp/brain-export")] to: String, #[arg(long)] force: bool },
    /// Copia o banco para um .bak
    Backup { #[arg(long)] to: Option<String> },

    // ---- Manutenção -------------------------------------------------------------
    /// Apaga notas vencidas (testar com --dry-run)
    ForgetSweep { #[arg(long)] dry_run: bool },
    #[command(long_about = "\
Reindex é NÃO destrutivo: nunca faz DELETE FROM chunks. Embeda antes da transação
e reconcilia por INSERT OR REPLACE em (path, chunk_index), preservando o vetor
de quem o texto ainda casa e deixando NULL o resto.

--all    reindexa o acervo inteiro (default o que interessa).
--no-embed  só a parte estrutural: reconstrói o FTS5 sem tocar na rede, útil
         offline. Vetores que batem com o texto são preservados.")]
    /// Reconstrói o índice (vetor por padrão)
    Reindex { #[arg(long)] all: bool, #[arg(long)] no_embed: bool },
    /// Importa o acervo legado .md (uso único)
    Migrate { #[arg(long, default_value="./vault")] vault: String, #[arg(long, default_value="./data/index.db")] old_index: String, #[arg(long)] no_embed: bool },

    // ---- Servidor ---------------------------------------------------------------
    #[command(long_about = "\
Ciclo de vida do servidor MCP (US-01.1). Auto-importa o acervo legado e arquiva
em vault.bak.tar.gz dentro de BRAIN_EXPORT_ROOT antes de servir; se o archive
falhar, o import não roda.

`start` é o único subcomando porque a spec pede um. Bind e ferramentas são os
mesmos de serve-mcp; o shutdown é o mesmo nos três: SIGINT ou SIGTERM, para que
`systemctl stop` rode o teardown.")]
    /// Inicia o servidor MCP com importação legada
    Server { #[command(subcommand)] sub: ServerCmd },
    /// Inicia só o MCP via SSE (sem importar)
    ServeMcp { #[arg(long, default_value_t=8321)] port: u16 },
    /// Inicia só o visualizador web somente-leitura
    Serve { #[arg(long, default_value_t=8322)] port: u16 },
    /// Registra um evento do agente na sessão
    Hook {
        #[arg(long, value_parser=["session-start","tool-result","session-end"])]
        event: String,
        /// Projeto. Ausente: resolvido pela cascata (config.json -> remote git ->
        /// nome do diretório -> pergunta). Presente: usado como está.
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        payload: Option<String>,
        /// A pergunta que o usuario esta fazendo agora (RF-04). Vira a query da injecao
        /// de contexto do `session-start`.
        ///
        /// **Um argumento proprio, e nao uma chave dentro do `--payload`.** O payload e
        /// JSON dentro de uma string de shell, entao a pergunta teria de ser escapada
        /// duas vezes — e uma aspa na pergunta quebraria o JSON. Aqui a pergunta viaja
        /// como `argv`, onde as aspas nao tem significado nenhum. Ausente: a chave
        /// `question` do payload e usada, e sem as duas a injecao degrada (T4.3).
        #[arg(long)]
        question: Option<String>,
    },

    // ---- Projetos e setup -------------------------------------------------------
    /// Gerencia projetos e vínculos de notas
    Project { #[command(subcommand)] sub: ProjectCmd },
    /// Instala MCP, regras e serviço (uso único)
    Setup {
        /// opencode | kiro | systemd | shell | project | all
        #[arg(default_value = "all")]
        target: String,
        #[arg(long, default_value_t = 8321)]
        mcp_port: u16,
        #[arg(long, default_value_t = 8322)]
        viewer_port: u16,
        #[arg(long)]
        brain_dir: Option<String>,
        #[arg(long)]
        dir: Option<String>,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        dry_run: bool,
        /// Nao interativo: assume os defaults, nao pergunta, nao grava projeto
        /// (R-07). Tambem aceito como `BRAIN_SETUP_NONINTERACTIVE=1`.
        #[arg(long)]
        yes: bool,
        /// Grava este projeto para o diretorio atual, sem perguntar. A resposta
        /// explicita nao-interativa a pergunta do projeto.
        #[arg(long)]
        project: Option<String>,
        /// Grava "nao usar brain aqui" para o diretorio atual, sem perguntar.
        #[arg(long)]
        decline_project: bool,
    },
}

#[derive(Subcommand)]
enum ProjectCmd { Create { name: String, #[arg(long, default_value="")] description: String }, List, Delete { name: String }, Notes { name: String }, Link { note_path: String, project: String }, Unlink { note_path: String, project: String } }

/// US-01.1. One subcommand, because the spec asks for one.
#[derive(Subcommand)]
enum ServerCmd {
    /// Start the MCP SSE server: import legacy data (archiving it first), then
    /// serve. Port defaults to `BRAIN_PORT`, 8321.
    Start {
        /// Override the SSE port. Omit to use `BRAIN_PORT` (default 8321).
        #[arg(long)]
        port: Option<u16>,
        /// Legacy vault directory to import from, if it holds notes.
        #[arg(long, default_value="./vault")]
        vault: String,
        /// Legacy index database. Detected and reported; not imported (see B6).
        #[arg(long, default_value="./data/index.db")]
        old_index: String,
    },
}

static REINDEXING: std::sync::OnceLock<std::sync::Mutex<bool>> = std::sync::OnceLock::new();
fn reindex_lock() -> &'static std::sync::Mutex<bool> { REINDEXING.get_or_init(|| std::sync::Mutex::new(false)) }

fn full_path(layer: &str, path: &str, scope: Option<&str>) -> Result<String> {
    validate_layer(layer)?;
    sanitize_relative_path(path)?;
    if LAYERS_WITH_SCOPE.contains(&layer) {
        let s = scope.ok_or_else(|| anyhow::anyhow!("scope required for {}", layer))?;
        validate_scope(s)?;
        Ok(format!("{}/{}/{}", layer, s, path))
    } else { Ok(format!("{}/{}", layer, path)) }
}

/// One legacy-vault note staged for import:
/// `(note_id, path, layer, scope, content, project_id, tags)`.
/// B6 moved this into [`legacy_import`], next to the code that builds it — the
/// import and the type describing its output belong in one file, and there is
/// only one place that constructs one now.
#[allow(unused_imports)]
use legacy_import::StagedNote as PendingImport;

fn hook_spool_path() -> PathBuf {
    let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(base).join("brain/hook-spool.jsonl")
}

/// Embeds every chunk of every note in a single pass, returning the
/// `{path: [vector per chunk_index]}` map that [`Store::reindex_all_with`] takes.
///
/// One batch for the whole corpus rather than one per note: the batch runs 4
/// requests in flight, so a single batch amortises that concurrency across every
/// note instead of restarting it (and re-paying connection setup) per note.
///
/// Each vector is returned paired with the chunk text it was computed from, in a
/// [`NoteEmbed`]. That pairing is what makes the write safe: this pass takes
/// minutes against a serial Ollama, and a `brain_store` landing inside that
/// window would otherwise have the *old* text's vectors written onto the *new*
/// text's chunks. The first chunk that fails to embed truncates that note's list —
/// everything after the gap is dropped rather than shifted, because a vector
/// attributed to the wrong chunk is a wrong answer, not a degraded one.
async fn embed_all_notes(notes: &[(String, String)]) -> std::collections::HashMap<String, NoteEmbed> {
    use std::collections::HashMap;
    let mut out: HashMap<String, NoteEmbed> = HashMap::new();
    if notes.is_empty() { return out; }
    // (note index, chunk index) for every enqueued text, same order as `texts`.
    let mut owner: Vec<(usize, usize)> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    for (ni, (_path, content)) in notes.iter().enumerate() {
        for (ci, ch) in brain_core::chunk_text(content, brain_core::CHUNK_TARGET_TOKENS).iter().enumerate() {
            owner.push((ni, ci));
            texts.push(ch.clone());
        }
    }
    let eng = brain_embed::EmbeddingEngine::from_env();
    let budget = eng.batch_timeout(texts.len());
    let vecs = match tokio::time::timeout(budget, eng.embed_batch_partial(texts)).await {
        Ok(v) => v,
        Err(_) => {
            eprintln!("reindex: embedding budget of {:?} expired for {} chunk(s); stored vectors are preserved and the rest stay NULL", budget, owner.len());
            return out;
        }
    };
    let total_chunks = owner.len();
    let mut broken: HashMap<usize, ()> = HashMap::new();
    let mut filled = 0usize;
    for ((ni, ci), v) in owner.into_iter().zip(vecs) {
        if broken.contains_key(&ni) { continue; }
        let Some(v) = v else {
            broken.insert(ni, ()); // everything from here on would be misaligned
            continue;
        };
        if v.len() != brain_core::EMBEDDING_DIM {
            eprintln!("reindex: chunk {} of note {} has dim {}, expected {} — truncating this note's vectors", ci, notes[ni].0, v.len(), brain_core::EMBEDDING_DIM);
            broken.insert(ni, ());
            continue;
        }
        let slot = out.entry(notes[ni].0.clone()).or_default();
        // `ci` is the index into this note's own chunk list, so `slot.chunks` and
        // `slot.vectors` stay aligned and each vector keeps its source text.
        if slot.vectors.len() == ci { slot.vectors.push(v); filled += 1; } else { broken.insert(ni, ()); }
    }
    // Fill the provenance list from the same content the pass chunked. A vector at
    // position i came from chunk i of that content, by construction above.
    for (path, content) in notes {
        if let Some(ne) = out.get_mut(path) {
            ne.chunks = brain_core::chunk_text(content, brain_core::CHUNK_TARGET_TOKENS)
                .into_iter().take(ne.vectors.len()).collect();
        }
    }
    if filled < total_chunks {
        eprintln!("reindex: embedded {}/{} chunk vectors", filled, total_chunks);
    }
    out
}

#[allow(dead_code)]
fn detect_project() -> String {
    // try git toplevel basename, else cwd basename
    if let Ok(out) = std::process::Command::new("git").args(["rev-parse","--show-toplevel"]).output() {
        if out.status.success() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    if let Some(name) = std::path::Path::new(trimmed).file_name().and_then(|n| n.to_str()) {
                        return name.to_string();
                    }
                }
            }
        }
    }
    std::env::current_dir().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())).unwrap_or_else(|| "brain".into())
}

/// Rebuilds the whole index, embedding before any write transaction opens.
///
/// `brain reindex --all` used to `DELETE FROM chunks` and re-insert every row
/// with a zero vector, so running it destroyed the entire vector index. It is
/// now non-destructive: unchanged chunks keep their vectors, changed ones come
/// back as `NULL`, and the embedding pass runs first so as many as possible come
/// back hydrated.
///
/// Three properties this function is responsible for:
/// - **snapshot before embed.** `all_notes` is read *before* the network pass, and
///   each [`NoteEmbed`] carries the chunk texts its vectors were computed from.
///   Embedding the whole corpus takes minutes against a serial Ollama; a
///   `brain_store` landing in that window would otherwise get the old text's
///   vectors written onto the new text's chunks. `chunks_sync` drops those and
///   reports them as `diverged`, so the run says so instead of lying.
/// - **lock around the embed pass.** The MCP server's background queue embeds on
///   the same chunks. Both take the advisory lock in `_meta`, so a backfill is
///   not run twice. The lock is released before the write transaction opens.
/// - **honest reporting.** `preserved`, `rehydrated`, `null` and `diverged` are all
///   printed. A report that only said "done" is how a half-rebuilt index looked
///   healthy for a month.
async fn run_reindex(db: &str, no_embed: bool) -> Result<()> {
    let store = Store::open(db)?;
    // Snapshot the corpus first. Everything the embed pass reads comes from here.
    let notes = store.all_notes()?;
    let embeds = if no_embed {
        eprintln!("reindex: --no-embed, keeping stored vectors and leaving new chunks NULL");
        std::collections::HashMap::new()
    } else {
        // Embed before the write transaction opens: the store is a synchronous
        // rusqlite handle, and awaiting a network batch with a write lock held
        // would block every other reader for the duration.
        let owner = brain_store::embed_lock_owner("reindex");
        let locked = store.try_acquire_embed_lock(&owner, 3600)?;
        if !locked {
            let holder = store.embed_lock_holder()?.unwrap_or_else(|| "unknown".into());
            anyhow::bail!(
                "another embed holds the lock ({}), so this reindex would duplicate its work; \
                 retry when it finishes. Nothing was written.",
                holder
            );
        }
        let embeds = embed_all_notes(&notes).await;
        let _ = store.release_embed_lock(&owner);
        embeds
    };
    let (cnt, stats) = store.reindex_all_with(&embeds)?;
    outln!(
        "REINDEX_DONE notes={} chunks={} embedded={} preserved={} rehydrated={} null={} diverged={} stale_reused={} unmatched={}",
        cnt, stats.total, stats.embedded, stats.preserved, stats.rehydrated, stats.nulls, stats.diverged,
        stats.stale_reused, stats.unmatched
    );
    if stats.nulls > 0 {
        outln!("REINDEX_PARTIAL {} chunk(s) have no vector — rerun without --no-embed while Ollama is reachable", stats.nulls);
    }
    if stats.diverged > 0 {
        outln!(
            "REINDEX_DIVERGED {} chunk vector(s) were computed from text the note no longer has and were NOT \
             applied; those notes were edited during this run. Re-run `brain reindex --all` to embed them.",
            stats.diverged
        );
    }
    if stats.stale_reused > 0 {
        outln!(
            "REINDEX_STALE {} chunk(s) kept a vector that is slightly out of date (text similar enough to reuse); \
             the log names them. Rerun to refresh if that matters.",
            stats.stale_reused
        );
    }
    Ok(())
}

/// Whether the session hook embeds the sections it appends.
///
/// `BRAIN_HOOK_EMBED=0` turns it off. The embed is now a *diff* (only the sections
/// that grew), so it costs one request per event against a serial Ollama — but an
/// operator who does not want their agent's tool output in the semantic index, or
/// who runs the hook on a machine that should not talk to Ollama at all, should not
/// have to pay for it. The note is written either way, so capture is never
/// conditional on this.
const HOOK_EMBED_ENV: &str = "BRAIN_HOOK_EMBED";

fn hook_embed_enabled() -> bool {
    match std::env::var(HOOK_EMBED_ENV) {
        Ok(v) => !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no" | "off"),
        Err(_) => true,
    }
}

/// The dedup marker for a rendered section.
///
/// X-01. **The trailing newline is load-bearing.** The store decides "already
/// captured" by asking whether the note contains this marker, and the marker used
/// to be the bare `id=<key>`. That is a prefix test over free text, so any event
/// id that is a prefix of another one matched the wrong section: with events
/// `d11` and `d1`, the note already containing `id=d11` also contains `id=d1`, and
/// `d1` was silently dropped as a duplicate — a lost session event with no error
/// and no log line, reachable from a *sequential* run and not only a concurrent
/// one.
///
/// Every section format ends the heading with `id=<key>` followed by a newline, so
/// including it makes the marker a whole token: `id=d1\n` cannot match inside
/// `id=d11\n`. Derived from the section's own first line rather than restated, so
/// a change to the heading format cannot silently un-anchor it.
fn dedup_marker(section: &str, dedup_key: &str) -> String {
    let head = section.lines().next().unwrap_or_default();
    if head.ends_with(&format!("id={dedup_key}")) {
        return format!("id={dedup_key}\n");
    }
    // A heading that no longer ends with the id: fall back to the bare token
    // rather than to a marker that can never match, which would silently disable
    // deduplication entirely.
    eprintln!("hook: section heading for id={dedup_key} does not end with the id; dedup marker unanchored");
    format!("id={dedup_key}")
}

/// Next rotated part number for a day's session note: the highest existing
/// `{date}-N` plus one, starting at 2.
///
/// X-01: this moved into [`Store::note_append_section`], which computes it inside
/// the same `BEGIN IMMEDIATE` transaction that performs the append. It used to be
/// a separate read here, which meant two hooks could both read "no parts exist",
/// both pick `-2`, and the second would overwrite the first's section. The
/// highest-plus-one rule is unchanged; what changed is that it is now evaluated
/// against a snapshot no other writer can be modifying.
fn rotated_header<'a>(
    project: &'a str,
    date: &'a str,
    base: &'a str,
    section: &'a str,
) -> impl Fn(u32, &str) -> String {
    move |part, limit_error| {
        format!(
            "# Sessão {project} {date} (parte {part})\n\nContinuação de [[{base}]], que atingiu o limite de \
             escrita: {limit_error}\n\n{}",
            fit_section(section)
        )
    }
}

/// Truncates `section` until it fits under both write limits on its own.
///
/// The last resort of the hook, and the reason it never refuses: a single event
/// whose payload is larger than the whole note budget cannot be rotated away, since
/// rotating produces a new note that has to hold it too. The event is still
/// captured — truncated, with the fact stated in the text — because a silently
/// dropped session entry is worse than a shortened one.
fn fit_section(section: &str) -> String {
    if brain_core::validate_content_limits(section).is_ok() {
        return section.to_string();
    }
    // Leave room for the note header and the rotation notice around it.
    let budget = brain_core::MAX_CONTENT_BYTES / 2;
    let mut cut = section.len().min(budget);
    while cut > 0 {
        // Back off to a char boundary rather than splitting a UTF-8 sequence.
        while cut > 0 && !section.is_char_boundary(cut) {
            cut -= 1;
        }
        let candidate = &section[..cut];
        if brain_core::validate_content_limits(candidate).is_ok() {
            return format!("{candidate}\n\n[truncated by the brain hook: this event exceeded the note size limit]\n");
        }
        cut -= 1;
    }
    "[truncated by the brain hook: this event exceeded the note size limit]\n".to_string()
}

/// The `origin` remote's URL, or `None` when git cannot answer.
///
/// Every failure is `None`, deliberately, and none of them is an error: git absent,
/// not a repository, no remote, a detached worktree. The cascade treats them all the
/// same — step 2 simply
/// does not fire — because the alternative is failing a hook that an IDE calls on
/// every event, over a step that has three more fallbacks behind it.
///
/// `stdin` is null so git can never stop to ask for a credential (RF-07.3). There is
/// **no timeout** here, and the risk is accepted rather than mitigated: this reads
/// `.git/config` through plumbing that does no network and takes no index lock, so the
/// work is bounded by a local file read. A deadline would mean a watchdog thread
/// around a call that is not slow — more machinery than the hazard earns.
fn git_origin_remote(dir: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("remote")
        .arg("get-url")
        .arg("origin")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

/// The hook's prompt: it does not ask, and says why.
///
/// **The question moved out of the hook (RF-07.3).** An IDE does not hand a hook a
/// terminal, so the `is_terminal` branch was nearly dead code, and the `read_line` in
/// the live branch was **unbounded** — a hook that blocks is worse for the IDE than one
/// that skips a step, and nothing inside the hook can bound somebody else's read.
///
/// So the answer is always `CannotAsk`, which is not a degraded prompt but the accurate
/// one: nobody is present. `brain setup` asks the operator interactively in P5 and
/// records the answer, after which this function is never reached. The trait stays, so
/// P5 has somewhere to plug the real question in.
struct HookPrompt;

impl resolve::Prompt for HookPrompt {
    fn ask(&self, _candidates: &[String]) -> resolve::Answer {
        eprintln!(
            "hook: there is nobody to ask which project this is, so nothing was written and \
             nothing was recorded. Run `brain setup` to answer once for this directory."
        );
        resolve::Answer::CannotAsk
    }
}

/// Resolve the project for a hook run: explicit `--project` wins, else the cascade.
///
/// Degrades rather than fails (RF-07). Two things that used to be fatal are warnings:
/// a `BRAIN_DIR` that cannot be created or written (RF-07.1) and a database that
/// cannot be opened. The hook runs on every event, inside something the operator is
/// trying to work in, so an environment problem must never become a non-zero exit.
///
/// The return is a plain `Resolution`, not `Result<Option<…>>`. It used to be wrapped
/// in both, with a doc promising a `None` for `disabled` that never arrived and an
/// `.expect` at the call site whose stated reason ("unless disabled") was the very
/// thing that was untrue — a panic in a hook, the worst of RF-07's failure modes.
fn resolve_hook_project(
    db: &str,
    explicit: Option<&str>,
    dir: &std::path::Path,
) -> resolve::Resolution {
    if let Some(name) = explicit {
        return resolve::Resolution {
            project: Some(name.to_string()),
            origin: resolve::Origin::Explicit,
        };
    }

    // The registered names come from the database, so the cascade's steps 2 and 3
    // can only resolve to something the brain actually knows. A store that cannot be
    // opened is not fatal: the cascade still has step 1 and step 4.
    let known: Vec<String> = match Store::open(db) {
        Ok(store) => store
            .project_list()
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.name)
            .collect(),
        Err(e) => {
            eprintln!("hook: project list unavailable ({e}); the cascade will run with no candidates");
            Vec::new()
        }
    };

    let record = |dir: &std::path::Path, entry: &config::Entry| config::save_entry(dir, entry);
    // RF-07.1: no usable `BRAIN_DIR` is a degraded run, not a failed one. The cascade
    // loses step 1 and the ability to record; it keeps the remote and the directory
    // name.
    let config_dir = match config::brain_dir() {
        Ok(d) => Some(d),
        Err(e) => {
            eprintln!("hook: {e}; running without config.json, so nothing will be recorded");
            None
        }
    };
    resolve::resolve(
        dir,
        &resolve::Env {
            known: &known,
            git_remote: git_origin_remote(dir),
            prompt: &HookPrompt,
            record: &record,
            config_dir: config_dir.as_deref(),
        },
    )
}

/// The query to inject context with, and whether the caller had to degrade.
///
/// **This is a change of decision from `PLAN.md` §2, and it is a measured one.** The
/// plan says an empty question "cai no texto fixo antigo". Measured against a corpus
/// that actually has project rules, the old fixed text returns **zero** results:
///
/// ```text
/// search("padroes melhores praticas licoes", layer=regras, scope=global)  ->  total = 0
/// search("padroes melhores praticas licoes", layer=regras, project=hive) ->  total = 0
/// ```
///
/// against a note whose text is "Always run the full test suite before commit". So the
/// planned fallback is a four-stream RRF round trip whose result is guaranteed empty, and
/// keeping it would preserve nothing while keeping the lie that produced it.
///
/// The empty query is the better fallback for three reasons, in order of weight:
///
/// 1. **A fixed string that matches is worse than nothing.** When those three words do
///    occur, the old code printed the notes it found under a header that implies they
///    answer the session. They answer a question nobody asked. An empty query prints
///    `INJECT: (no context found)`, which is true.
/// 2. It is free. `Store::search` has no FTS, vector, entity or graph stream without a
///    query, so `all_paths` comes back empty and it returns at `lib.rs:1906` — one early
///    return instead of four scans. The fixed text would pay for all four.
/// 3. It keeps the degraded path honest for the caller, which is what `eprintln!` on the
///    other side of the hook reports upward.
///
/// So: the question when there is one, nothing when there is not, and the IDE is told
/// which of the two happened.
///
/// **The second measured decision: terms are joined with `OR`, not `AND`.** This one is
/// not in the plan and it is the difference between the feature working and not working.
/// Measured on a corpus that has the note the question is about:
///
/// ```text
/// search("kebab-case nos caminhos", project=hive)  ->  total = 0
/// search("kebab" OR "case" OR "nos" OR "caminhos") ->  total = 1  regras/projetos/hive/naming
/// ```
///
/// A natural-language question is a **retrieval** query, not a filter. Strict AND requires
/// *every* term to be present, and a six-word question almost never has all of its words
/// in the one relevant note — so AND answers "nothing", which is what the fixed query it
/// replaces also did, for a different reason. Recall is the right bias here: the RRF
/// fusion and FTS5's own ranking still put the best match first, and the project filter
/// (not the conjunction) is what keeps the result set small.
///
/// `Store::search` keeps FTS5's default conjunction. This looser one is scoped to the
/// inject, because `Store::search` also backs the MCP `brain_search` tool, where the
/// caller asked for a search and expects the precision that implies.
fn inject_query(question: &str) -> (String, bool) {
    let q = question.trim();
    if q.is_empty() {
        (String::new(), true)
    } else {
        let joined = brain_store::fts5_match_expr_joined(q, "OR");
        // A question of only FTS operators/stripped punctuation joins to nothing:
        // searching it would run four streams for a guaranteed empty result, so
        // degrade like the empty question instead of reporting healthy.
        if joined.trim().is_empty() {
            (String::new(), true)
        } else {
            (joined, false)
        }
    }
}

async fn hook_handle(event: String, project: Option<String>, payload: Option<String>, question: Option<String>, db: String) -> Result<()> {
    use chrono::Utc;
    use fs2::FileExt;
    use std::fs::OpenOptions;
    use std::io::{Read, Write};

    // strict event validation (clap value_parser already restricts, double-check)
    if !["session-start", "tool-result", "session-end"].contains(&event.as_str()) {
        anyhow::bail!("invalid event '{}': expected session-start|tool-result|session-end", event);
    }

    // RF-02/RF-03: an explicit `--project` short-circuits the cascade; without one
    // the directory is resolved. The working directory is the hook's cwd, which is
    // where the IDE launched it.
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("hook: the current directory is unreadable — {e}"))?;
    if !cwd.is_dir() {
        anyhow::bail!("hook: {} is not a directory", cwd.display());
    }
    let resolution = resolve_hook_project(&db, project.as_deref(), &cwd);

    // "Do not use this here" — all three of its shapes, and the difference between
    // them is not cosmetic. `desabilitado` and `recusado` are answers a human gave
    // and are final; `unresolved` is the *absence* of an answer, and it writes no
    // note either but records nothing, so the directory is still asked the next time
    // a human is present. See `Origin::skip_reason` for the table.
    //
    // Measured before this gate existed: a recorded refusal produced
    // `hook ok … note=sessoes/<dirname>/<date>` — declining a question had silently
    // created a session note under a project the operator never chose, which is not
    // what "mark as not used" means.
    if let Some(why) = resolution.origin.skip_reason() {
        outln!("hook skipped event={} origin={} dir={}", event, resolution.origin, cwd.display());
        eprintln!("hook: {} {why}, so nothing was written", cwd.display());
        return Ok(());
    }

    // The origin is on its own line so that R-06 still holds: with an explicit
    // `--project` the `hook ok …` line below is byte-for-byte what it always was.
    let origin_line = format!(
        "hook resolve project={} origin={}",
        resolution.project.as_deref().unwrap_or("(none)"),
        resolution.origin
    );
    outln!("{origin_line}");

    // Every origin that reaches here has a project, so the note path always has one
    // component to attribute the event to.
    //
    // There used to be a fallback here — the directory's own name, for a resolution
    // with no project. It is gone, and that is the point of the change above: it is
    // what turned a refusal into a note. A *new* origin that arrives without a project
    // must make a deliberate choice, so this is a distinct branch with its own
    // message rather than a silent substitution. It skips rather than bails, because a
    // hook that exits non-zero breaks the IDE that called it (RF-07) — and that is
    // exactly the kind of mistake that should be loud in the log rather than fatal.
    let resolved_project = match resolution.project.clone() {
        Some(p) => p,
        None => {
            outln!("hook skipped event={} origin={} dir={}", event, resolution.origin, cwd.display());
            eprintln!(
                "hook: {} resolved to origin={} with no project, which no origin is \
                 supposed to do; nothing was written. Add the new origin to \
                 `Origin::skip_reason` or give it a project.",
                cwd.display(),
                resolution.origin
            );
            return Ok(());
        }
    };
    let project = resolved_project;
    // The project name becomes part of the note path, and it arrived unsanitised, so
    // `--project ../../etc` used to produce the note path `sessoes/../../etc/<date>`
    // — a namespace escape that also makes `brain_export` write outside its directory.
    //
    // What `sanitize_relative_path` guarantees is **containment**, not a single
    // component: it refuses an absolute path and any `..` component, and allows
    // nesting (`a/b` is a legal relative path). The comment here used to claim "one
    // component, no separators", which was never true of the guard — restating it
    // would have been a claim in the source that no test could support.
    //
    // The name can now also come from a directory, and that input is safe by
    // construction rather than by this check: a real directory's `file_name()` cannot
    // contain `/` or be `..`, because the filesystem forbids both. It still goes
    // through the sanitiser so there is exactly one place where a name becomes a path.
    // Degrade, do not fail. RF-07: a hook that exits non-zero breaks the IDE that
    // called it, and the cause here — a project name that cannot be a path component —
    // is the operator's own earlier input, not a fault of the moment. A file that
    // carries such a name (hand-edited, or written by a build older than the check in
    // `config::save_entry_in`) must not turn every event from here on into an error.
    let project = match sanitize_relative_path(&project) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "hook: {e}\n\
                 This directory's recorded project cannot be used as a path component, so \
                 nothing was written. Fix it with `brain setup --project <name>` in this \
                 directory, or remove the entry from config.json."
            );
            return Ok(());
        }
    };
    let now = Utc::now().to_rfc3339();
    let date = Utc::now().format("%Y-%m-%d").to_string();
    let payload_raw = payload.clone().unwrap_or_else(|| "{}".into());
    let payload_val: serde_json::Value = serde_json::from_str(&payload_raw).unwrap_or(serde_json::Value::Null);
    // dedup key: payload id if present, else hash(event+project+payload_raw)
    let raw_id = payload_val.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let dedup_key = if raw_id.is_empty() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        format!("{}|{}|{}", event, project, payload_raw).hash(&mut h);
        format!("{:x}", h.finish())
    } else { raw_id };
    // spool path: XDG_RUNTIME_DIR/brain/hook-spool.jsonl or /tmp/brain/hook-spool.jsonl
    let spool = hook_spool_path();
    if let Some(parent) = spool.parent() { std::fs::create_dir_all(parent)?; }
    let line = serde_json::json!({"event": event, "project": project, "payload": payload_val, "ts": now, "id": dedup_key});
    let line_str = serde_json::to_string(&line)?;
    let mut file = OpenOptions::new().create(true).append(true).read(true).open(&spool)?;
    file.lock_exclusive()?;
    let mut existing = String::new();
    {
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(0))?;
        file.read_to_string(&mut existing)?;
        if existing.contains(&format!("\"id\":\"{}\"", dedup_key)) {
            fs2::FileExt::unlock(&file)?;
            outln!("hook deduplicated id={}", dedup_key);
            return Ok(());
        }
    }
    writeln!(file, "{}", line_str)?;
    fs2::FileExt::unlock(&file)?;

    let pretty = serde_json::to_string_pretty(&payload_val).unwrap_or_default();
    let section = match event.as_str() {
        "session-start" => format!("## {} session-start id={}\n\nProject: {}\nTime: {}\n\n```json\n{}\n```\n", &now[11..16.min(now.len())], dedup_key, project, now, pretty),
        "tool-result" => {
            let tool = payload_val.get("tool").or_else(|| payload_val.get("name")).and_then(|v| v.as_str()).unwrap_or("unknown");
            format!("## {} tool-result {} id={}\n\n```json\n{}\n```\n", &now[11..16.min(now.len())], tool, dedup_key, pretty)
        },
        _ => format!("## {} session-end id={}\n\nProject: {}\nTime: {}\n\n```json\n{}\n```\n", &now[11..16.min(now.len())], dedup_key, project, now, pretty),
    };

    // --- phase 1: everything that needs the database, and no network ---------
    // `Store` is `!Sync` (a `RefCell` in rusqlite's statement cache), so no
    // `&Store` may be live across the embed `.await`. Every handle is opened in its
    // own scope and dropped before the next phase.
    //
    // X-01: the append below is a single `note_append_section` call, which reads
    // the note and writes it back inside one `BEGIN IMMEDIATE` transaction. It
    // used to be three steps here — `note_get`, build the content, `note_upsert` —
    // with no lock between them, so two overlapping hooks both read the same
    // content and the second write dropped the first hook's section. This is the
    // guarantee the code now makes: **every event is appended, whatever the
    // interleaving**, because no interleaving can be observed between the read and
    // the write. It does *not* serialise agents: the lock is SQLite's write lock,
    // held for one `SELECT` and one `UPDATE`, and never across the embed.
    let (full, content, todo) = {
        let store = Store::open(&db)?;
        let base = format!("sessoes/{}/{}", project, date);
        let header = format!("# Sessão {} {}\n", project, date);
        let appended = store.note_append_section(
            &base,
            "sessoes",
            None,
            &section,
            &dedup_marker(&section, &dedup_key),
            &header,
            &rotated_header(&project, &date, &base, &section),
            None,
            &[],
        )?;
        if !appended.appended {
            outln!("hook store deduplicated id={}", dedup_key);
        }
        if let Some(why) = &appended.rotated_because {
            // W-03.1: rotate, never refuse. The hook's caller
            // (`hooks/brain-hook.py`) runs it with a 30s timeout, so a rejection
            // would be a silently broken hook — strictly worse than the bug it
            // fixes. The day's note is left exactly as it is (never truncated,
            // never dropped) and the new section starts a new note that links back
            // to it, so no section is lost.
            eprintln!(
                "hook: {base} is at the write limit, so this event starts {}. Nothing is lost: the previous \
                 note keeps every section it had. ({why})",
                appended.path
            );
        }
        let full = appended.path;
        let content = appended.content;
        // W-03.2: embed the *delta*, not the accumulated document. The note
        // grows by one section per event, so embedding all of it every time is
        // quadratic — measured at 76 events x up to 76 chunks each, ~3,000
        // embedding requests for one session, ~4 s per event against a serial
        // Ollama. `chunks_needing_embedding` is a diff: it returns only the
        // chunks that have no vector for their current text, so the cost is one
        // request per new section.
        let todo = if appended.appended { store.chunks_needing_embedding(&full, &content)? } else { Vec::new() };
        // SH-02: sessoes/shared/* auto-link erp+mobile (ADR-001 single-source,
        // ADR-003 só prefixo shared/)
        if full.starts_with("sessoes/shared/") {
            for pname in ["erp", "mobile"] {
                if store.project_get(pname)?.is_none() {
                    let _ = store.project_create(pname, "");
                }
                let _ = store.note_link_project(&full, pname);
            }
        }
        (full, content, todo)
    };

    // --- phase 2: embed, with no database handle in scope --------------------
    let embed = hook_embed_enabled();
    if !embed {
        eprintln!("hook: {HOOK_EMBED_ENV} is off, so this event is stored FTS-only (no vector requested)");
    }
    let fresh = if embed && !todo.is_empty() {
        let texts: Vec<String> = todo.iter().map(|(_, t)| t.clone()).collect();
        let vecs = embed_chunks(&texts, &full).await;
        let mut fresh = brain_store::FreshVectors::new();
        for ((idx, text), v) in todo.iter().zip(vecs.into_iter()) {
            if let Some(v) = v {
                fresh.insert(*idx, (text.clone(), v));
            }
        }
        fresh
    } else {
        brain_store::FreshVectors::new()
    };

    // --- phase 3: write the vectors back -------------------------------------
    {
        let store = Store::open(&db)?;
        if store.note_get(&full)?.is_none() {
            anyhow::bail!("hook: {full} vanished between write and chunk sync");
        }
        let Some(nid) = store.note_id(&full)? else {
            anyhow::bail!("hook: {full} has no row id");
        };
        let stats = sync_note_chunks(
            &store,
            ChunkSyncInput {
                note_id: nid,
                path: &full,
                layer: "sessoes",
                scope: None,
                content: &content,
                project_id: None,
                tags: &[],
                // No explicit snapshot: `chunks_sync` reads the note's current rows
                // itself and prefers them, and `note_upsert` does not delete them —
                // which is what the old comment here claimed it did, and the reason
                // this snapshot was believed to be load-bearing.
                snapshot: brain_store::ChunkSnapshot::new(),
                fresh,
            },
        )?;
        eprintln!(
            "hook: {} chunks={} embedded={} NULL={} preserved={} rehydrated={} owed_before={}",
            full, stats.total, stats.embedded, stats.nulls, stats.preserved, stats.rehydrated, todo.len()
        );
    }

    // session-start inject: the user's own question, filtered by project.
    if event == "session-start" {
        let store = Store::open(&db)?;
        // RF-04. The query is what the user actually asked, taken from the payload. It
        // used to be the fixed string `"padroes melhores praticas licoes"`, which meant
        // the mechanism worked and was useless: it could not know what anyone asked.
        // The flag wins over the payload key, and the payload key is kept as a fallback
        // so a caller that already embeds a question does not have to know about the
        // flag. A caller passing both has a bug, and silently preferring one of them
        // would hide it — so they are compared, not merged.
        let from_flag = question.as_deref().map(str::trim).filter(|q| !q.is_empty());
        let from_payload = payload_val.get("question").and_then(|v| v.as_str()).map(str::trim).filter(|q| !q.is_empty());
        if from_flag.is_some() && from_payload.is_some() && from_flag != from_payload {
            eprintln!(
                "hook: --question and the payload's `question` disagree; using --question. \
                 Pass only one."
            );
        }
        let question = from_flag.or(from_payload).unwrap_or("");
        // T4.3, and a change of decision from the plan. The plan said "falls back to the
        // old fixed text"; see `inject_query` for why a fixed string is the wrong thing
        // to fall back to.
        let (query, degraded) = inject_query(question);
        debug_assert!(!degraded || query.trim().is_empty());
        if degraded {
            eprintln!("hook: no question in the payload; injecting by project only (no fixed query)");
        }
        // The project is a **filter**, not a term. It used to be spliced into the query
        // as `format!("regras {}", project)`, which is the opposite of a filter: it made
        // a note match only if its *text* happened to contain the project's name, and it
        // searched `scope = 'projetos'` — so a project note stored with
        // `scope = "global"` was unreachable from its own project.
        //
        // `Store::search` has taken `project=` all along, and the filter is applied over
        // owned **and** linked paths after the RRF merge, so it filters every stream at
        // once instead of biasing one of them.
        let res_global = store.search(&query, None, Some("regras"), Some("global"), None, None, 3, false).unwrap_or_default();
        if !res_global.is_empty() {
            outln!("--- Brain context (global) ---");
            for r in &res_global { outln!("{} [{}] {}", r.path, r.score, r.snippet.chars().take(120).collect::<String>()); }
        }
        let res_proj = store.search(&query, None, Some("regras"), Some("projetos"), Some(&project), None, 3, false).unwrap_or_default();
        if !res_proj.is_empty() {
            outln!("--- Brain context (projetos) ---");
            for r in &res_proj { outln!("{} [{}] {}", r.path, r.score, r.snippet.chars().take(120).collect::<String>()); }
        }
        if res_global.is_empty() && res_proj.is_empty() {
            outln!("INJECT: (no context found)");
        }
    }
    // R-06: with an explicit `--project` this line is byte-for-byte what it always
    // was. The resolution origin is printed separately, above, precisely so that it
    // is not.
    outln!("hook ok event={} project={} note={} spool={}", event, project, full, spool.display());
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // ADR-01 v2. The root help is ours; everything else is clap's. Checked before
    // `parse()` so that clap never gets the chance to print its own single
    // `Commands:` section, and scoped to a lone `--help`/`-h`/`help` so that
    // `brain help search` and `brain --db X --help` still go to clap untouched.
    //
    // The narrow scope is a deliberate choice, not a gap: there is exactly **one**
    // custom page, and it is the one an operator reaches by asking for help with no
    // further qualification. `brain --db X --help` therefore prints clap's flat list,
    // so two root pages exist by design. (It is *not* about which page shows the
    // resolved `BRAIN_DB_PATH` — `option_row` prints it on both, and
    // `the_options_block_carries_every_visible_arg_with_its_env_and_default` is what
    // pins that.)
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if is_root_help_request(&argv) {
        // `outln!` for consistency with every other write in this binary, and because
        // the root page will outgrow a 64 KB pipe buffer if the command list ever does.
        // At today's 1.55 KB `BrokenPipe` is unreachable -- see the M6 note in
        // tests/help_overview.rs: `a_truncated_read_of_the_root_help_still_exits_zero`
        // proves no panic, not this choice.
        outln!("{}", root_help().trim_end_matches('\n'));
        return Ok(());
    }
    let cli = Cli::parse();
    let db = cli.db.clone();
    // stdio transport fallback via env
    let transport = std::env::var("BRAIN_TRANSPORT").unwrap_or_default();
    if transport == "stdio" {
        // if cmd is ping or not serve, handle stdio mode for MCP
        // but keep normal flow for other cmds; only intercept ServeMcp or Ping
        if matches!(cli.cmd, Cmd::Ping) {
            brain_mcp::serve_stdio(db).await?;
            return Ok(());
        }
    }
    match cli.cmd {
        Cmd::Ping => outln!("pong"),
        Cmd::Store { layer, path, content, scope, project, tags, pinned, expires_at } => {
            // The shared write rule — the same one the MCP handlers and the hook
            // use, so a limit cannot be enforced on three paths out of four.
            let fp = brain_mcp::validate_note_write(&layer, &path, scope.as_deref(), &content)
                .map_err(|e| anyhow::anyhow!("INVALID_PARAMS: {e}"))?;
            let store = Store::open(&db)?;
            let tags_v: Vec<String> = tags.unwrap_or_default().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let project_id = if let Some(pname) = project {
                // F-00 (was `.unwrap()`) + TD-F4-A: a new project name still
                // creates the project (exit 0, scripts unaffected) but warns
                // on stderr so a typo does not silently spawn a namespace.
                match store.project_get(&pname)? {
                    Some(p) => Some(p.id),
                    None => {
                        eprintln!("warning: project '{pname}' novo — será criado; confira com project_list");
                        Some(store.project_create(&pname, "")?.id)
                    }
                }
            } else { None };
            let nid = store.note_upsert(&fp, &layer, scope.as_deref(), &content, project_id, &tags_v, pinned, expires_at.as_deref())?;
            let chunks = brain_core::chunk_text(&content, brain_core::CHUNK_TARGET_TOKENS);
            // The CLI embeds inline rather than queueing, and that is deliberate:
            // this is a one-shot process, so a background task would be killed with
            // the process and the note would never get a vector at all. The
            // blocking cost is bounded by `MAX_CHUNKS` (~26s worst case), and the
            // server paths — which are long-lived — do use the queue.
            let vecs = embed_chunks(&chunks, &fp).await;
            let fresh = brain_store::NoteEmbed { chunks: chunks.clone(), vectors: vecs.into_iter().flatten().collect() };
            let stats = sync_note_chunks(&store, ChunkSyncInput { note_id: nid, path: &fp, layer: &layer, scope: scope.as_deref(), content: &content, project_id, tags: &tags_v, snapshot: brain_store::ChunkSnapshot::new(), fresh: fresh.fresh_vectors() })?;
            outln!("ok — {} (chunks={} embedded={} without_embedding={} preserved={} rehydrated={} diverged={})", fp, stats.total, stats.embedded, stats.nulls, stats.preserved, stats.rehydrated, stats.diverged);
        }
        Cmd::Read { layer, path, scope } => {
            let fp = full_path(&layer, &path, scope.as_deref())?;
            let store = Store::open(&db)?;
            if let Some(n) = store.note_get(&fp)? { outln!("# {}\n\n{}", n.path, n.content); } else { eprintln!("NOT_FOUND: {}", fp); std::process::exit(1); }
        }
        Cmd::Search { query, layer, scope, project, tag, top_k, explain } => {
            let store = Store::open(&db)?;
            let embed = brain_embed::EmbeddingEngine::from_env();
            let qvec = embed.embed(&query).await.ok();
            let res = store.search(&query, qvec.as_deref(), layer.as_deref(), scope.as_deref(), project.as_deref(), tag.as_deref(), top_k, explain)?;
            if explain {
                let out = serde_json::json!({"results": res.iter().map(|r| {
                    let mut v = serde_json::json!({"path": r.path, "layer": r.layer, "scope": r.scope, "score": r.score, "snippet": r.snippet, "project": r.project, "tags": r.tags});
                    if let Some(e) = &r.explain {
                        v["explain"] = serde_json::json!({"rrf_vec": e.rrf_vec, "rrf_fts": e.rrf_fts, "rrf_entity": e.rrf_entity, "rrf_graph": e.rrf_graph, "authority": e.authority, "score": e.score, "rank_vec": e.rank_vec, "rank_fts": e.rank_fts, "rank_entity": e.rank_entity, "rank_graph": e.rank_graph});
                    }
                    v
                }).collect::<Vec<_>>(), "total": res.len()});
                outln!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                let out = serde_json::json!({"results": res.iter().map(|r| serde_json::json!({"path": r.path, "layer": r.layer, "scope": r.scope, "score": r.score, "snippet": r.snippet, "project": r.project, "tags": r.tags})).collect::<Vec<_>>(), "total": res.len()});
                outln!("{}", serde_json::to_string_pretty(&out)?);
            }
        }
        Cmd::Delete { path } => {
            if let Err(e) = sanitize_relative_path(&path) {
                eprintln!("INVALID_PARAMS: {}", e);
                std::process::exit(2);
            }
            let store = Store::open(&db)?;
            if store.note_delete(&path)? { outln!("ok deleted {}", path); } else { eprintln!("NOT_FOUND"); std::process::exit(1); }
        }
        Cmd::Recent { top_k } => {
            let store = Store::open(&db)?;
            let rows = store.recent(top_k)?;
            outln!("{}", serde_json::to_string_pretty(&serde_json::json!({"recent": rows}))?);
        }
        Cmd::Status => {
            let store = Store::open(&db)?;
            outln!("notes={} chunks={} projects={} db={}", store.count_notes()?, store.count_chunks()?, store.project_list()?.len(), db);
            // Embedding coverage + Ollama health. `notes`/`chunks` alone stayed
            // healthy while 99.6% of chunk vectors were BLOB-of-zeros scoring
            // 0.0 against every query, so this block is the regression alarm.
            let cov = store.embedding_coverage()?;
            let eng = brain_embed::EmbeddingEngine::from_env();
            outln!("embedding: chunks_total={} embedded={} without_embedding={} zero_vector={} coverage_pct={}",
                cov.total, cov.embedded, cov.without_embedding, cov.zero_vector, cov.coverage_pct);
            outln!("ollama: reachable={} model={}", eng.health_check().await, eng.model);
            // Y-05. The embedding queue, or the part of it a *separate process*
            // can see.
            //
            // A dead letter used to be discoverable only through a message that
            // said "run `brain status`", and this command printed no queue
            // block at all — so the pointer led nowhere, and the queue lives in a
            // systemd service whose stderr goes to a journal nobody reads. The
            // queue's work list and its dead-letter counter are **in-memory state
            // of the `serve-mcp` process**, not rows in the database, so this
            // process cannot read them no matter what it prints. Claiming
            // otherwise here would be a lie an operator acts on.
            //
            // What *is* in the database is the cross-process embed lock, and it
            // is the half that matters from outside: it is what "another embed is
            // running, and for how long" means, and it is the only way to tell a
            // queue that is deferred from one that is idle. The rest is named
            // explicitly below, with where it can actually be read.
            let lock = store.embed_lock_state();
            outln!(
                "queue: embed_lock_holder={} embed_lock_age_s={} embed_lock_expires_in_s={}",
                lock.holder.as_deref().unwrap_or("none"),
                lock.age_secs.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                lock.expires_in_secs.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
            );
            if lock.holder.is_some() {
                outln!(
                    "queue: an embed pass is in progress and holds the cross-process lock. A note that stays \
                     without a vector while this runs is waiting on it, not lost."
                );
            }
            outln!(
                "queue: pending_len/ready_len/dead_lettered/last_drain live in the running server process, \
                 not in this database, so `brain status` cannot show them. Read them with the brain_status MCP \
                 tool (which queries the live server), or `journalctl -u brain-mcp` for the dead-letter line. \
                 Their footprint in the database is embedding.without_embedding above: a chunk stuck NULL with \
                 a note that no longer moves is the shape to look for."
            );
        }
        Cmd::Reindex { all: _, no_embed } => {
            let lock = reindex_lock();
            {
                let guard = lock.lock().unwrap();
                if *guard { outln!("REINDEX_IN_PROGRESS"); std::process::exit(2); }
            }
            *lock.lock().unwrap() = true;
            // The flag is cleared on the error path too: leaving it set would
            // wedge every later reindex in this process.
            let result = run_reindex(&db, no_embed).await;
            *lock.lock().unwrap() = false;
            result?;
        }
        Cmd::Checkpoints { limit } => {
            let store = Store::open(&db)?;
            let cps = store.checkpoints(limit)?;
            outln!("{}", serde_json::to_string_pretty(&cps)?);
        }
        Cmd::Restore { id } => {
            let store = Store::open(&db)?;
            if store.restore_audit(id)? { outln!("ok restored {}", id); } else { eprintln!("NOT_FOUND audit {}", id); std::process::exit(1); }
        }
        Cmd::Backup { to } => {
            // W-01: the destination is contained to `BRAIN_EXPORT_ROOT` (or the
            // `{db}.bak` default). The CLI is local, so this is defence in depth
            // rather than the primary control — but it is the same binary the
            // systemd units run, and "local" is a property of the caller, not of the
            // process.
            let dst = brain_mcp::fs_guard::backup_file(&db, to.as_deref())
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            std::fs::copy(&db, &dst)?; outln!("backup -> {}", dst.display());
        }
        Cmd::Export { to, force } => {
            let dir = brain_mcp::fs_guard::export_dir(Some(&to))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if dir.exists() && !force { anyhow::bail!("export dir exists, use --force"); }
            std::fs::create_dir_all(&dir)?;
            // X-05.3: re-check ownership now the directory exists. `export_dir`
            // could only skip the check when the root was absent, and in `/tmp`
            // another local user can create it inside that window.
            brain_mcp::fs_guard::assert_root_usable(&brain_mcp::fs_guard::export_root())?;
            let store = Store::open(&db)?;
            let recent = store.recent(10000)?;
            let mut written = 0usize;
            for (path, _layer, _scope, content) in recent {
                let fp = brain_mcp::fs_guard::note_file_within(&dir, &path)?;
                if let Some(parent) = fp.parent() { std::fs::create_dir_all(parent)?; }
                std::fs::write(fp, content)?;
                written += 1;
            }
            outln!("export -> {} ({} note(s))", dir.display(), written);
        }
        Cmd::ForgetSweep { dry_run } => {
            let store = Store::open(&db)?;
            let expired = store.forget_sweep(dry_run)?;
            if dry_run { outln!("dry-run would delete {}: {:?}", expired.len(), expired); } else { outln!("deleted {}: {:?}", expired.len(), expired); }
        }
        Cmd::Serve { port } => {
            // Armed before the bind, for the same reason `server start` arms it
            // before its import and `serve_rmcp_sse` before its boot recovery: this
            // **call** installs the SIGTERM disposition on the caller's thread, and
            // only the future it returns is a wait. The bind below is an `.await`,
            // so a SIGTERM landing in that window must be *recorded* — with the
            // handler armed it is, and the wait is already resolved when first
            // polled. Armed later, the same SIGTERM is the default action and the
            // process dies by signal.
            //
            // `with_graceful_shutdown` is what makes that wait reachable at all:
            // without it there is no wait to reach, and `systemctl stop` on
            // `brain-viewer.service` kills the process mid-request.
            //
            // One caveat, inherited from the helper rather than introduced here:
            // only SIGTERM is armed eagerly. Its `ctrl_c()` arm sits *inside* the
            // returned `async move` block, and an `async fn` body does not run until
            // the future is polled, so SIGINT is still registered on first poll and
            // a Ctrl-C inside this window would take the default action. That is
            // true of `serve-mcp` and `server start` too, and it is why this says
            // SIGTERM rather than "either signal".
            let shutdown = brain_mcp::rmcp_service::shutdown_on_sigint_or_sigterm();
            let app = brain_web::router(db.clone());
            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
            // Y-02. Printed **after** the bind, so the line means "this port is
            // live" and not "this port was intended". It was printed before, which
            // announced a URL for a server that then died with `Address already in
            // use` — wrong for an operator reading the log, and worse for the test
            // that treats the announcement as its readiness signal: a squatter on
            // the port would satisfy the announcement while the bind failed. An
            // `?` that never announced anything would have been the honest version.
            outln!("viewer http://0.0.0.0:{}/ — /api/status|search|read|list", port);
            axum::serve(listener, app).with_graceful_shutdown(shutdown).await?;
        }
        Cmd::ServeMcp { port } => {
            if std::env::var("BRAIN_TRANSPORT").unwrap_or_default() == "stdio" {
                brain_mcp::serve_stdio(db).await?;
            } else {
                brain_mcp::rmcp_service::serve_rmcp_sse(db.clone(), port).await?;
            }
        }
        Cmd::Hook { event, project, payload, question } => {
            hook_handle(event, project, payload, question, db).await?;
        }
        Cmd::Setup { target, mcp_port, viewer_port, brain_dir, dir, force, dry_run, yes, project, decline_project } => {
            setup::run_setup(&setup::SetupOpts {
                target, mcp_port, viewer_port, db: db.clone(), brain_dir, dir,
                force, dry_run, yes, project, decline_project,
            })?;
        }
        Cmd::Server { sub } => match sub {
            ServerCmd::Start { port, vault, old_index } => {
                let port = port.unwrap_or_else(default_mcp_port);
                // Armed first, on purpose: the import below is synchronous and
                // can take minutes, and a signal that arrives during it has to be
                // *recorded* rather than killing the process. See
                // `shutdown_on_sigint_or_sigterm`.
                let shutdown = brain_mcp::rmcp_service::shutdown_on_sigint_or_sigterm();
                // AC3, before the listener exists. The import reads a directory of
                // markdown and writes a database, archiving the source first — see
                // `legacy_import` for why that has to happen before, not after, and
                // why a failed archive stops the import instead of degrading it.
                //
                // Embedding is left to the queue: the imported chunks are written
                // `NULL` and `serve_rmcp_sse_with`'s boot recovery re-queues them,
                // so a start with Ollama down still starts.
                let imported = legacy_import::import_legacy(&db, std::path::Path::new(&vault), &old_index)?;
                let report = &imported.report;
                if let Some(b) = &report.backup {
                    outln!("server: legacy vault archived to {}", b.display());
                }
                if let Some(idx) = &report.legacy_index_present {
                    outln!(
                        "server: WARNING legacy index {} exists but its contents are NOT imported \
                         (the Rust store reads the vault's markdown only); it is left untouched on disk",
                        idx.display()
                    );
                }
                if report.notes > 0 {
                    outln!(
                        "server: imported {} legacy note(s) (chunks={} without_embedding={})",
                        report.notes, report.chunks, report.without_embedding
                    );
                }
                if std::env::var("BRAIN_TRANSPORT").unwrap_or_default() == "stdio" {
                    // The wait built above is armed but, on this branch, nothing
                    // ever polled it — and arming SIGTERM without acting on it is
                    // *worse* than never arming it. The disposition is now tokio's
                    // rather than the default, so `systemctl stop` is recorded and
                    // then ignored: the process keeps reading stdin and serving,
                    // and the unit only goes away when `TimeoutStopSec` escalates to
                    // `SIGKILL`. `serve_viewer_shutdown.rs` calls that half-fix
                    // "worse than the bug" and this branch is that half-fix.
                    //
                    // Raced rather than dropped, so both outcomes are honest: EOF
                    // on stdin still returns cleanly, and a signal ends the
                    // process. `serve_rmcp_sse_with` receives the same future by
                    // move in the other arm; `select!` and the move are exclusive.
                    tokio::select! {
                        r = brain_mcp::serve_stdio(db.clone()) => r?,
                        () = shutdown => {
                            outln!("server: shutting down on signal");
                            // `exit`, not `return`, and the reason is specific
                            // rather than stylistic. `serve_stdio` reads stdin
                            // through `tokio::io::stdin`, which parks a
                            // `spawn_blocking` task on the read, and a runtime's
                            // shutdown **waits** for its blocking tasks. Returning
                            // from `main` drops the runtime, so a read on a pipe
                            // the client still holds open never returns and the
                            // process does not exit — a server that has stopped
                            // serving and still refuses to die, which is the same
                            // `TimeoutStopSec`-to-`SIGKILL` outcome the race above
                            // was added to remove. Measured before this line: the
                            // message printed, the process stayed alive.
                            //
                            // Skipping the async drop is safe *here* specifically:
                            // this arm has no database handle, no embedding queue
                            // and no listener to tear down — the import
                            // short-circuited or finished before it, and
                            // `serve_stdio` opens a `Store` per request and drops
                            // it within the request. `outln!` above already
                            // reaches for `process::exit` on a broken pipe, so this
                            // is the same choice this file already makes for the
                            // same reason.
                            std::process::exit(0);
                        }
                    }
                } else {
                    // Not `serve_rmcp_sse`: that builds its own wait, and this arm
                    // needs it armed *before* the import above — an import that takes
                    // minutes would otherwise run with no SIGTERM handler installed.
                    // Both are the same signal set; only the construction point differs.
                    brain_mcp::rmcp_service::serve_rmcp_sse_with(db.clone(), port, brain_mcp::global_queue(), shutdown)
                        .await?;
                }
            }
        },
        Cmd::Migrate { vault, old_index, no_embed } => {
            // B6: the archive is taken by the shared import path, so the explicit
            // command and `server start` cannot drift — an operator who chose the
            // safe, explicit route gets the same safety as the automatic one.
            let imported = legacy_import::import_legacy(&db, std::path::Path::new(&vault), &old_index)?;
            if let Some(b) = &imported.report.backup {
                outln!("migrated: legacy vault archived to {}", b.display());
            }
            if let Some(idx) = &imported.report.legacy_index_present {
                outln!(
                    "migrated: WARNING legacy index {} exists but its contents are NOT imported \
                     (the Rust store reads the vault's markdown only); it is left untouched on disk",
                    idx.display()
                );
            }
            let store = Store::open(&db)?;
            // Hydrate during the import rather than after it: a legacy import is
            // a bulk one-shot event where Ollama is up by definition, and it is
            // the only moment the whole corpus is guaranteed to be in hand. The
            // import used to write a zero vector per chunk, leaving a freshly
            // migrated vault with a 0% usable vector index.
            let notes: Vec<(String, String)> =
                imported.staged.iter().map(|(_, f, _, _, c, _, _)| (f.clone(), c.clone())).collect();
            let embeds = if no_embed {
                eprintln!("migrate: --no-embed, imported notes stay FTS-only");
                std::collections::HashMap::new()
            } else {
                embed_all_notes(&notes).await
            };
            let (mut chunks_total, mut embedded, mut nulls) = (0usize, 0usize, 0usize);
            for (nid, full, layer, scope, content, pid, tags) in &imported.staged {
                let fresh = embeds
                    .get(full)
                    .map(|ne| ne.fresh_vectors())
                    .unwrap_or_default();
                let st = store.chunks_sync(*nid, full, layer, scope.as_deref(), content, *pid, tags, &fresh, &brain_store::ChunkSnapshot::new())?;
                chunks_total += st.total;
                embedded += st.embedded;
                nulls += st.nulls;
            }
            outln!("migrated {} vault files (chunks={} embedded={} without_embedding={})", imported.staged.len(), chunks_total, embedded, nulls);
            if nulls > 0 {
                outln!("MIGRATE_PARTIAL {} chunk(s) have no vector — run `brain reindex --all` with Ollama reachable to backfill", nulls);
            }
        }
        Cmd::Project { sub } => {
            let store = Store::open(&db)?;
            match sub {
                ProjectCmd::Create { name, description } => { let pr = store.project_create(&name, &description)?; outln!("ok project {} id={}", pr.name, pr.id); }
                ProjectCmd::List => { let ps = store.project_list()?; outln!("{}", serde_json::to_string_pretty(&ps)?); }
                ProjectCmd::Delete { name } => { if store.project_delete(&name)? { outln!("ok deleted {}", name); } else { eprintln!("NOT_FOUND"); } }
                ProjectCmd::Notes { name } => { let notes = store.project_notes(&name)?; outln!("{}", serde_json::to_string_pretty(&notes)?); }
                ProjectCmd::Link { note_path, project } => { store.note_link_project(&note_path, &project)?; outln!("ok linked {} -> {}", note_path, project); }
                ProjectCmd::Unlink { note_path, project } => { if store.note_unlink_project(&note_path, &project)? { outln!("ok unlinked {} -/-> {}", note_path, project); } else { outln!("no link"); } }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The grouping table and the derive must agree in both directions.
    ///
    /// `root_help` panics on a name that is not a subcommand, so a typo in
    /// `HELP_GROUPS` is loud — but only for the names it *does* reach. The silent
    /// half is the one that matters: a command the table forgot is printed under
    /// `Outros` instead of its group, or, if the `Outros` block ever goes away,
    /// not printed at all. Neither shows up in a smoke test of the page, and both
    /// are the same defect wearing different clothes.
    #[test]
    fn the_help_groups_cover_every_subcommand_exactly_once() {
        let mut cmd = Cli::command();
        cmd.build();
        let declared: Vec<&str> = cmd
            .get_subcommands()
            .filter(|s| !s.is_hide_set())
            .map(|s| s.get_name())
            .collect();

        let grouped: Vec<&str> = HELP_GROUPS
            .iter()
            .flat_map(|(_, names)| names.iter().copied())
            .collect();

        let mut unique = grouped.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            grouped.len(),
            "a command is listed in two groups: {grouped:?}"
        );
        for name in &grouped {
            assert!(
                declared.contains(name),
                "HELP_GROUPS names `{name}`, which is not a subcommand of `brain`"
            );
        }
        let orphans: Vec<&&str> = declared.iter().filter(|n| !grouped.contains(n)).collect();
        assert_eq!(
            orphans,
            vec![&"help"],
            "every subcommand must be in exactly one group; ungrouped: {orphans:?}"
        );
    }

    /// A root help request is recognised only when it is the *whole* argv.
    ///
    /// The two negative cases are the point: `brain --db X --help` is how an operator
    /// checks what `BRAIN_DB_PATH` resolved to, and handing that to the renderer
    /// would print a page that silently disagrees with the command about to run.
    #[test]
    fn only_a_lone_help_flag_is_a_root_help_request() {
        let owned = |v: &[&str]| is_root_help_request(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert!(owned(&["--help"]));
        assert!(owned(&["-h"]));
        assert!(owned(&["help"]));
        assert!(!owned(&[]));
        assert!(!owned(&["help", "search"]));
        assert!(!owned(&["--db", "/tmp/x.db", "--help"]));
        assert!(!owned(&["search", "--help"]));
    }

    /// An env-resolved value goes to stdout, and stdout gets pasted into a ticket.
    ///
    /// Built on a synthetic `Arg` rather than a real one because the only global arg
    /// today is `db` — a path with no credential to leak. The point is the *future*
    /// named in F4: a global arg over an env like `BRAIN_OLLAMA_URL` (which accepts
    /// `http://user:pass@host`) would print the password straight into `--help`, and a
    /// help page travels much further than a log line. A test cannot wait for that arg
    /// to exist, so it constructs the shape it will have.
    ///
    /// The two halves are both load-bearing. Redaction alone would pass while the
    /// wiring was dead, and byte-identity alone would pass while a password leaked.
    #[test]
    fn an_env_value_is_redacted_but_a_credential_free_one_is_untouched() {
        let arg = clap::Arg::new("ollama-url")
            .long("ollama-url")
            .env("BRAIN_OLLAMA_URL");

        // With a credential: the password must not survive into the rendered row.
        // The value is injected rather than set in the environment, because
        // `set_var` is unsafe in edition 2024 and would mutate state every parallel
        // test in this binary shares. The redaction is inside `option_row`, so the
        // wiring under test is the real one.
        let credentialed = |_: &str| "http://user:hunter2@ollama:11434".to_string();
        let (_name, description) = option_row(&arg, &credentialed);
        assert!(
            !description.contains("hunter2"),
            "the password reached the rendered help row: {description}"
        );
        assert!(
            description.contains("[env: BRAIN_OLLAMA_URL="),
            "the annotation itself must survive redaction: {description}"
        );

        // Without a credential: byte-identical, so the common case cannot be
        // degraded by an over-eager redactor (F4 names `mailto:` as the known
        // over-redaction, and this is the property that stops that from mattering).
        let plain_path = "/home/u/data/brain.db".to_string();
        let (_name, description) = option_row(&arg, &|_| plain_path.clone());
        assert!(
            description.contains("[env: BRAIN_OLLAMA_URL=/home/u/data/brain.db]"),
            "a credential-free value must reach the page unchanged: {description}"
        );
    }

    /// Yellow Batch2: a question of only FTS operators degrades like an empty
    /// one — the joined query is empty, so a healthy flag would lie to the
    /// caller about running four streams for a guaranteed empty result.
    #[test]
    fn an_operators_only_question_degrades_like_an_empty_one() {
        for q in ["", "   ", "OR", "and", "OR AND NOT", "or and not", "\"():{}=-/"] {
            let (query, degraded) = inject_query(q);
            assert!(query.is_empty(), "query for {q:?} must be empty, got {query:?}");
            assert!(degraded, "query for {q:?} must report degraded");
        }
        let (query, degraded) = inject_query("kebab-case nos caminhos");
        assert!(!query.is_empty() && !degraded, "a real question stays healthy: {query:?}");
        let (query, degraded) = inject_query("a OR b");
        assert_eq!(query, "a OR b");
        assert!(!degraded);
    }
}
