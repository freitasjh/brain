//! `brain --help` is the first thing an operator sees, and the SPEC for
//! brain-help-visual is about it being a *one-screen overview*.
//!
//! Two things are asserted here, and the split matters:
//!
//! - **What the CLI promises** — the 5 group titles, the 20 one-liners inside them
//!   in the order the ux-designer froze, none of them leaking markdown, all of them
//!   inside 80 columns. That is a contract, and a typo in any one-liner is a bug an
//!   operator reads.
//! - **What clap 4.6 still owns** — every subcommand's own `--help`, which the root
//!   renderer must not touch. `a_subcommand_help_is_still_claps` and
//!   `the_root_help_shows_exactly_what_clap_describes` are the two edges of that
//!   boundary.
//!
//! No network, no Ollama, no database: `--help` returns before any of that. The
//! binary is the real one (`CARGO_BIN_EXE_brain`), so this asserts on the actual
//! rendered bytes rather than on a reconstructed template.

use std::path::PathBuf;
use std::process::Command;

fn brain_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_brain"))
}

/// The root help, with the terminal width pinned to the SPEC's 80.
fn root_help() -> String {
    let o = Command::new(brain_bin())
        .arg("--help")
        .env("COLUMNS", "80")
        .env("BRAIN_DB_PATH", "/nonexistent/db.db")
        .output()
        .unwrap();
    assert!(o.status.success(), "`brain --help` must exit 0");
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// The 5 group titles, in the order the renderer must print them.
const TITLES: &[&str] = &["Uso comum", "Memória", "Manutenção", "Servidor", "Projetos e setup"];

/// The command rows: every line indented by exactly two spaces and starting with a
/// command name, up to (not including) the `Options:` block.
///
/// A shape test, not a content test, and the shape is what makes it total. A command
/// row is `  <name><gutter><about>`; an Options row is either `  -h, --help` (a dash
/// in the third column) or `      --db <DB>` (six spaces, from the long-only indent).
/// The footer rows would also match — `  brain ping` is a two-space lowercase line —
/// so the scan stops at `Options:`, which is the one marker no command row can
/// contain.
fn command_lines(help: &str) -> Vec<&str> {
    let end = help.find("\nOptions:").unwrap_or(help.len());
    help[..end]
        .lines()
        .filter(|l| {
            let mut cs = l.chars();
            cs.clone().next() == Some(' ')
                && cs.clone().nth(1) == Some(' ')
                && cs.nth(2).is_some_and(|c| c.is_lowercase())
        })
        .collect()
}

/// Split a command row into `(name, about)`.
fn split_row(row: &str) -> (&str, &str) {
    let t = row.trim();
    let (name, about) = t.split_once(char::is_whitespace).unwrap_or((t, ""));
    (name, about.trim_start())
}

/// The 20 commands in the order the ux-designer froze, paired with the one-liner
/// each must carry. `help` (clap's own) is excluded on purpose: it is clap's text,
/// not ours, and this test is not the place to police another crate.
const EXPECTED: &[(&str, &str)] = &[
    ("ping", "Verifica se o servidor responde"),
    ("search", "Busca notas por texto e semântica"),
    ("read", "Lê uma nota pelo caminho completo"),
    ("store", "Salva uma nota (scope p/ arquitetura/regras/estudos)"),
    ("recent", "Lista as notas alteradas por último"),
    ("status", "Mostra notas, cobertura e fila de embedding"),
    ("checkpoints", "Consulta o histórico de alterações"),
    ("restore", "Restaura uma versão anterior pelo id"),
    ("delete", "Apaga uma nota e seus trechos"),
    ("export", "Exporta notas p/ diretório temporário"),
    ("backup", "Copia o banco para um .bak"),
    ("forget-sweep", "Apaga notas vencidas (testar com --dry-run)"),
    ("reindex", "Reconstrói o índice (vetor por padrão)"),
    ("migrate", "Importa o acervo legado .md (uso único)"),
    ("server", "Inicia o servidor MCP com importação legada"),
    ("serve-mcp", "Inicia só o MCP via SSE (sem importar)"),
    ("serve", "Inicia só o visualizador web somente-leitura"),
    ("hook", "Registra um evento do agente na sessão"),
    ("project", "Gerencia projetos e vínculos de notas"),
    ("setup", "Instala MCP, regras e serviço (uso único)"),
];

/// RF-01. The 5 titles, in the frozen order.
///
/// Order is the assertion, not membership: the whole point of grouping is that
/// "what do I do most often" comes before "what do I do when something is broken",
/// and a set comparison would pass on a page that lists them alphabetically.
#[test]
fn the_five_group_titles_appear_in_the_frozen_order() {
    let help = root_help();
    let mut cursor = 0;
    for title in TITLES {
        let at = help[cursor..]
            .find(&format!("\n{title}:\n"))
            .unwrap_or_else(|| panic!("group {title:?} missing, or out of order, in:\n{help}"));
        cursor += at + title.len() + 3;
    }
    // A title outside the five would mean the renderer grew a section nobody
    // reviewed. `Options:` and `Outros` are the renderer's own other blocks, and
    // `Exemplos:` is the footer's — so all three are expected here and a fourth
    // would be the finding.
    for line in help.lines() {
        if let Some(title) = line.strip_suffix(':').filter(|l| !l.starts_with(' ')) {
            if !TITLES.contains(&title)
                && title != "Options"
                && title != "Outros"
                && title != "Exemplos"
            {
                panic!("unexpected section title {title:?} in the root help:\n{help}");
            }
        }
    }
}

/// The commands each group must carry, mirroring the renderer's own table.
///
/// Duplicated here on purpose. A test that read the table out of the binary would
/// pass whatever the table says; this one states the *expectation* independently, so
/// moving `migrate` from "Manutenção" to "Servidor" in the source fails here.
const GROUPED: &[(&str, &[&str])] = &[
    ("Uso comum", &["ping", "search", "read", "store", "recent", "status"]),
    ("Memória", &["checkpoints", "restore", "delete", "export", "backup"]),
    ("Manutenção", &["forget-sweep", "reindex", "migrate"]),
    ("Servidor", &["server", "serve-mcp", "serve", "hook"]),
    ("Projetos e setup", &["project", "setup"]),
];

/// Each command appears exactly once, inside the group that claims it.
///
/// The mutation this catches is the one the renderer makes structurally possible:
/// a name listed in two groups (printed twice) or in none (printed nowhere, except
/// in `Outros`). The first is a lie about the CLI, the second is a command an
/// operator has to already know exists to find — and a grouping table is exactly the
/// kind of thing that rots silently.
#[test]
fn every_command_is_printed_exactly_once_inside_its_group() {
    let help = root_help();
    let mut cursor = 0;
    for (title, names) in GROUPED {
        let start = help[cursor..]
            .find(&format!("\n{title}:\n"))
            .unwrap_or_else(|| panic!("group {title:?} missing:\n{help}"))
            + cursor
            + title.len()
            + 3;
        // `write_block` closes every section with a blank line, so that is the
        // boundary; nothing else in the page is blank.
        let body = &help[start..];
        let end = body.find("\n\n").unwrap_or(body.len());
        let printed: Vec<&str> = body[..end]
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| split_row(l).0)
            .collect();
        assert_eq!(printed, *names, "group {title:?} lists the wrong commands");
        cursor = start + end;
    }

    // Nothing is left over: whatever the five groups do not claim must be exactly
    // clap's own `help`, in `Outros`, and not quietly missing.
    let grouped: Vec<&str> = GROUPED.iter().flat_map(|(_, n)| n.iter().copied()).collect();
    let orphans: Vec<&str> = command_lines(&help)
        .iter()
        .map(|row| split_row(row).0)
        .filter(|n| !grouped.contains(n))
        .collect();
    assert_eq!(orphans, vec!["help"], "an unexpected command is outside every group");
    assert!(
        help.contains("\nOutros:\n  help "),
        "the ungrouped command must still be printed, under a heading:\n{help}"
    );
}

/// The whole point of the feature: the name and the one line, in the frozen order.
///
/// Ordered on the *pairs*, not on the commands alone, so a mutation that keeps 20
/// entries but swaps two of them — or keeps a name and drops its description —
/// fails here. Checking membership instead would pass on a shuffled list, which is
/// precisely the defect the SPEC was written against.
#[test]
fn the_commands_appear_in_the_frozen_order_with_their_one_liners() {
    let help = root_help();
    let rendered: Vec<(&str, &str)> = command_lines(&help)
        .iter()
        .map(|row| split_row(row))
        .filter(|(name, _)| *name != "help")
        .collect();

    assert_eq!(
        rendered.len(),
        EXPECTED.len(),
        "expected {} commands, got {:?}",
        EXPECTED.len(),
        rendered
    );
    for (got, want) in rendered.iter().zip(EXPECTED.iter()) {
        assert_eq!(got.0, want.0, "command order/name changed (expected {want:?})");
        assert_eq!(got.1, want.1, "one-liner changed for `{}`", want.0);
    }
}

/// ADR-01 v2's central claim: the renderer does not own the text.
///
/// Each one-liner on the root page is compared against what the *same binary* prints
/// for that subcommand, read live. A hardcoded copy in the renderer passes every
/// other test in this file and fails only here, the first time somebody rewords a
/// doc-comment — which is the entire failure mode the SPEC's "no text duplication"
/// rule exists to prevent.
#[test]
fn the_root_help_shows_exactly_what_clap_describes() {
    let help = root_help();
    for (name, one_liner) in EXPECTED {
        let o = Command::new(brain_bin())
            .arg(name)
            .arg("-h")
            .env("COLUMNS", "100")
            .output()
            .unwrap();
        let own = String::from_utf8_lossy(&o.stdout);
        let described = own.lines().next().unwrap_or_default().trim();
        assert_eq!(
            described, *one_liner,
            "`brain {name} -h` describes itself differently from the root page"
        );
        assert!(
            help.contains(&format!("  {name} ")) && help.contains(one_liner),
            "the root page must carry `brain {name}`'s own one-liner verbatim"
        );
    }
}

/// The two defects the PROPOSAL named, as a general rule.
///
/// Asserting on `**` and backticks rather than on the two strings that used to
/// carry them is deliberate: the SPEC's own lesson is that an assert of string shape
/// is what lets the next leak through. A `##` inside the `store` example *is* a
/// `#`, and the check must not fire on it — hence the scope, which is the command
/// rows only.
#[test]
fn no_markdown_leaks_into_the_command_rows() {
    for row in command_lines(&root_help()) {
        for marker in ["**", "`", "_", "*"] {
            assert!(
                !row.contains(marker),
                "markdown marker {marker:?} leaked into a command row: {row:?}"
            );
        }
    }
}

/// R-03: a one-screen overview, verified rather than eyeballed.
///
/// Scoped to the command rows on purpose. The `Options:` block legitimately exceeds
/// 80 columns — `--db` prints the *resolved* `BRAIN_DB_PATH`, and an operator cannot
/// shorten that — and asserting on the whole page would either fail on a line the
/// CLI does not control or push us into faking the env value out of the help, which
/// is the one thing a help test must never do.
#[test]
fn every_command_row_fits_in_eighty_columns() {
    for row in command_lines(&root_help()) {
        let w = row.chars().count();
        assert!(w <= 80, "command row is {w} columns, over the 80 limit: {row:?}");
    }
}

/// RF-04: the footer. Asserted for the three commands and the title, not for the
/// exact block, so a reworded example does not fail the build while a *missing*
/// example does.
#[test]
fn the_footer_carries_the_three_examples() {
    let help = root_help();
    assert!(help.contains("Exemplos:"), "footer title missing:\n{help}");
    for example in [
        "brain ping",
        "brain store regras naming \"## Regra\" --scope global",
        "brain search \"termo\" --explain",
    ] {
        assert!(help.contains(example), "footer example missing: {example}\n{help}");
    }
}

/// ADR-02: detail moved out of the one-liner, not deleted.
///
/// The mutation this catches is "someone shortened the one-liner and dropped the
/// sentence". A shortened `reindex` reads fine and loses the fact that `--no-embed`
/// exists; the information has to live in the subcommand's own `--help`.
#[test]
fn the_detail_moved_to_long_about_is_still_reachable() {
    for (cmd, needle) in [
        ("reindex", "--no-embed"),
        ("reindex", "NÃO destrutivo"),
        ("server", "US-01.1"),
        ("server", "SIGTERM"),
        ("search", "RRF"),
        ("search", "Ollama"),
    ] {
        let o = Command::new(brain_bin())
            .arg(cmd)
            .arg("--help")
            .env("COLUMNS", "100")
            .output()
            .unwrap();
        let help = String::from_utf8_lossy(&o.stdout);
        assert!(
            help.contains(needle),
            "`brain {cmd} --help` must still carry {needle:?} (ADR-02)"
        );
    }
}

/// **The boundary ADR-01 v2 draws.** The root help is ours; a subcommand's own help
/// is clap's, and it must stay clap's.
///
/// The specific way it went wrong before: `next_help_heading` on a `Cmd` variant does
/// not reach the *parent* help at all, but it does reach the subcommand's own —
/// titling `brain reindex --help`'s flag list "Manutenção:" instead of "Options:".
/// Now that the grouping is a renderer, the temptation is to reintroduce the
/// attribute as "belt and braces". This test is the receipt for that.
///
/// Both the short (`-h`) and the long (`--help`) form are checked, because they are
/// different templates in clap and the long one is the one that was broken.
#[test]
fn a_subcommand_help_is_still_claps() {
    for cmd in ["reindex", "server", "search", "setup", "hook", "store"] {
        for flag in ["-h", "--help"] {
            let o = Command::new(brain_bin())
                .arg(cmd)
                .arg(flag)
                .env("COLUMNS", "100")
                .output()
                .unwrap();
            let help = String::from_utf8_lossy(&o.stdout);
            assert!(
                help.contains("Options:"),
                "`brain {cmd} {flag}` must keep clap's `Options:` heading:\n{help}"
            );
            for title in TITLES.iter().chain(std::iter::once(&"Servidor")) {
                assert!(
                    !help.contains(&format!("{title}:")),
                    "`brain {cmd} {flag}` leaked the group heading {title:?} into its \
                     own help:\n{help}"
                );
            }
            // And the renderer did not follow it down: no group title at all.
            assert!(
                !help.contains("Uso comum:") && !help.contains("Exemplos:"),
                "`brain {cmd} {flag}` looks like the root page — the renderer must \
                 not reach a subcommand:\n{help}"
            );
        }
    }
}

/// The `Options:` block, tied to the `Arg` it is rendered from.
///
/// This block was ~40 lines of `option_row` and `write_block` with **no**
/// behavioural coverage at all: a mutation that dropped the value name `<DB>` and the
/// `[env: …]` annotation passed the whole suite, because every other test looks at
/// the command rows. An Options block that lost its `<DB>` and its env still reads
/// like a help page, which is why the omission survived review.
///
/// The second half is the one that earns its keep. `root_help()` pins
/// `BRAIN_DB_PATH=/nonexistent/db.db`, so the first block can only prove the
/// annotation exists; this one re-runs with a *different* env and asserts the **other**
/// value comes out. That is the semantic the comment promises — the *resolved* value,
/// not the default — and it is the difference between an operator reading the right
/// number off the line and reading the one they would get anyway.
#[test]
fn the_options_block_carries_every_visible_arg_with_its_env_and_default() {
    let help = root_help();
    let options = &help[help.find("\nOptions:").expect("Options block")..];
    // A long-only arg carries four spaces of its own so its `--` lines up with the
    // `--` of a short+long one; the value name follows the flag.
    assert!(options.contains("--db <DB>"), "long-only + value name: {options}");
    assert!(options.contains("-h, --help"), "short+long: {options}");
    assert!(options.contains("-V, --version"), "short+long: {options}");
    assert!(options.contains("[env: BRAIN_DB_PATH="), "resolved env: {options}");
    assert!(options.contains("[default: ./data/brain.db]"), "default: {options}");

    // The resolved value, not the default: a different env must print differently.
    let o = Command::new(brain_bin())
        .arg("--help")
        .env("BRAIN_DB_PATH", "/tmp/x.db")
        .env("COLUMNS", "80")
        .output()
        .unwrap();
    let h = String::from_utf8_lossy(&o.stdout);
    assert!(
        h.contains("[env: BRAIN_DB_PATH=/tmp/x.db]"),
        "the rendered value must be the resolved env, not the default:\n{h}"
    );
    assert!(
        !h.contains("[env: BRAIN_DB_PATH=./data/brain.db]"),
        "the default leaked into the env annotation:\n{h}"
    );
}

/// Only the *lone* `--help`/`-h`/`help` is ours (RF-06 for `help`).
///
/// `brain help search` and `brain --db X --help` carry more than one argument, and
/// both are clap's to answer.
///
/// **The reason is scope, not information.** An earlier version of this comment
/// claimed the renderer was kept away because it would hide the resolved
/// `BRAIN_DB_PATH` from the operator — that was refuted: `option_row` prints the
/// resolved value on *both* pages, and
/// `the_options_block_carries_every_visible_arg_with_its_env_and_default` is what
/// proves it. The real reason is that there is exactly one custom page and it sits
/// on the minimum argv, so `brain --help` (grouped, PT) and `brain --db X --help`
/// (clap's flat list) are two root pages by design.
///
/// The marker of the renderer having run is a group title and nothing else, so that
/// is what is asserted to be absent — the footer deliberately is not. `after_help` is
/// a clap attribute, so the renderer *reads* the footer out of `Cli::command()`
/// rather than owning it, and clap's own rendering of the root carries it too. One
/// owner, two readers.
#[test]
fn a_root_help_request_with_other_arguments_is_still_claps() {
    for args in [
        vec!["help", "search"],
        vec!["--db", "/tmp/x.db", "--help"],
        vec!["search", "--help"],
    ] {
        let o = Command::new(brain_bin())
            .args(&args)
            .env("COLUMNS", "100")
            .output()
            .unwrap();
        let help = String::from_utf8_lossy(&o.stdout);
        for title in TITLES {
            assert!(
                !help.contains(&format!("{title}:")),
                "`brain {}` was taken by the root renderer:\n{help}",
                args.join(" ")
            );
        }
    }

    // And the one case that *is* the root page, rendered by clap: its own flat
    // `Commands:` list, and the footer that `after_help` owns.
    let o = Command::new(brain_bin())
        .args(["--db", "/tmp/x.db", "--help"])
        .env("COLUMNS", "100")
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&o.stdout);
    assert!(
        help.contains("Commands:"),
        "`brain --db X --help` must show clap's flat list:\n{help}"
    );
    assert!(
        help.contains("Exemplos:"),
        "the footer lives on the clap `after_help`, so clap's own root page carries \
         it too — one owner, two readers:\n{help}"
    );
}

/// RF-06: the three spellings of the same request give the same page.
///
/// `brain help` is not a fourth page, and letting it drift into clap's flat list
/// would mean an operator gets a different answer depending on which synonym they
/// typed. Asserted as byte equality, not as "both contain a group title": the point
/// is that there is one page, and a near-miss is the defect.
#[test]
fn help_and_help_flag_render_the_same_root_page() {
    let of = |arg: &str| {
        let o = Command::new(brain_bin())
            .arg(arg)
            .env("COLUMNS", "80")
            .env("BRAIN_DB_PATH", "/nonexistent/db.db")
            .output()
            .unwrap();
        assert!(o.status.success(), "`brain {arg}` must exit 0");
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let flag = of("--help");
    assert_eq!(of("-h"), flag, "`-h` and `--help` must render one page");
    assert_eq!(
        of("help"),
        flag,
        "a lone `help` is the same request as `--help` (RF-06) and must render the \
         same page"
    );
}

/// Reading only part of the root help is an ordinary pipeline, and must not fail.
///
/// **What this does and does not prove.** It catches a panic, an abort, or a non-zero
/// exit on the renderer path under a closed stdout — anything from a stray `unwrap` to
/// a future `expect`. It does **not** pin the `outln!` over `print!` choice, and it
/// is worth being explicit about why: the root page is 1590 B and a pipe buffer is
/// 64 KB, so the whole write completes before any reader could close, and
/// `BrokenPipe` is not reachable here. `outln!` is still the right call — it is the
/// same path every other write in this binary takes, and the renderer will outgrow
/// 64 KB if the command list ever does — but that is consistency, not a measured
/// failure, and no test below pretends otherwise.
#[test]
fn a_truncated_read_of_the_root_help_still_exits_zero() {
    let mut child = Command::new(brain_bin())
        .arg("--help")
        .env("COLUMNS", "80")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Read a few bytes and drop the read end, which is what `| head -3` does.
    {
        use std::io::Read;
        let mut buf = [0u8; 32];
        let _ = child.stdout.as_mut().unwrap().read(&mut buf);
    }
    drop(child.stdout.take());
    let status = child.wait().unwrap();
    assert!(status.success(), "`brain --help` into a closed pipe must exit 0, got {status}");
}
