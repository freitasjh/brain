use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_brain"))
}

fn db(tag: &str) -> String {
    format!("/tmp/brain-cli-e2e-{}-{}.db", std::process::id(), tag)
}

/// Runs the shipped binary once, against a dead Ollama.
///
/// The URL is set **explicitly** rather than left to the ambient environment. It
/// used to be inherited, which meant a `store` embedded inline against whatever
/// real model the machine happened to be running — the second half of the same
/// flake `Server::spawn` had (see there for the measurement), and a worse version
/// of it, because a developer's shell export could change a test's outcome. A
/// refused connection is also instant, where a real cold model is not.
///
/// Nothing in this file asserts on embedding: the checks read note content,
/// `/api/status`, and FTS-fallback searches. Tests that want a real backend build
/// their own `Command` with their own URL (a mock, or `DEAD_OLLAMA`).
fn run(db: &str, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(bin());
    cmd.arg("--db").arg(db);
    cmd.env("BRAIN_OLLAMA_URL", DEAD_OLLAMA);
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn brain")
}

fn out(o: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn cleanup(db: &str) {
    let _ = std::fs::remove_file(db);
    let _ = std::fs::remove_file(format!("{}.bak", db));
}

#[test]
fn e2e_ping() {
    let db = db("ping");
    let o = run(&db, &["ping"]);
    assert!(o.status.success());
    assert!(out(&o).contains("pong"));
    cleanup(&db);
}

#[test]
fn e2e_store_read_delete() {
    let db = db("srd");
    let o = run(&db, &["store", "regras", "e2e/n1", "## hello cli", "--scope", "global"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("regras/global/e2e/n1"));
    let o = run(&db, &["read", "regras", "e2e/n1", "--scope", "global"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("hello cli"));
    let o = run(&db, &["read", "regras", "e2e/missing", "--scope", "global"]);
    assert!(!o.status.success());
    let o = run(&db, &["delete", "regras/global/e2e/n1"]);
    assert!(o.status.success(), "{}", out(&o));
    let o = run(&db, &["delete", "regras/global/e2e/n1"]);
    assert!(!o.status.success());
    cleanup(&db);
}

#[test]
fn e2e_store_scope_required() {
    let db = db("scope");
    let o = run(&db, &["store", "regras", "x", "## hi"]);
    assert!(!o.status.success());
    assert!(out(&o).contains("scope required"));
    cleanup(&db);
}

#[test]
fn e2e_search_status_recent_reindex() {
    let db = db("ssr");
    let _ = run(&db, &["store", "regras", "e2e/s", "## zebra cli search", "--scope", "global"]);
    let o = run(&db, &["search", "zebra", "--top-k", "5"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("zebra"));
    let o = run(&db, &["search", "zebra", "--explain"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("total"));
    let o = run(&db, &["status"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("notes="));
    let o = run(&db, &["recent", "--top-k", "5"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("recent"));
    let o = run(&db, &["reindex", "--all"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("REINDEX_DONE"));
    cleanup(&db);
}

#[test]
fn e2e_checkpoints_restore() {
    let db = db("ckpt");
    let _ = run(&db, &["store", "sessoes", "tmp/r", "## restore me"]);
    let _ = run(&db, &["delete", "sessoes/tmp/r"]);
    let o = run(&db, &["checkpoints", "--limit", "5"]);
    assert!(o.status.success(), "{}", out(&o));
    let v: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(&o.stdout)).unwrap();
    let del_id = v.as_array().unwrap().iter().find(|c| c[1] == "delete").unwrap()[0].as_i64().unwrap();
    let o = run(&db, &["restore", &del_id.to_string()]);
    assert!(o.status.success(), "{}", out(&o));
    let o = run(&db, &["read", "sessoes", "tmp/r"]);
    assert!(o.status.success(), "{}", out(&o));
    cleanup(&db);
}

#[test]
fn e2e_backup_export_sweep() {
    let db = db("bes");
    let _ = run(&db, &["store", "regras", "e2e/b", "## backup me", "--scope", "global"]);
    let o = run(&db, &["backup"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(PathBuf::from(format!("{}.bak", db)).exists());
    // W-01: the export destination is contained to BRAIN_EXPORT_ROOT (default
    // /tmp/brain-export), so the fixture lives inside it. The old fixture wrote to
    // a sibling of /tmp, which is exactly the arbitrary-write path the allowlist
    // closes — and `brain export` refuses it now, as it should.
    let exp = format!("/tmp/brain-export/brain-cli-exp-{}-bes", std::process::id());
    let _ = std::fs::remove_dir_all(&exp);
    let o = run(&db, &["export", "--to", &exp, "--force"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(PathBuf::from(&exp).join("regras/global/e2e/b").exists());
    // ... and a destination outside the root is refused rather than created.
    let outside = format!("/tmp/brain-cli-exp-outside-{}", std::process::id());
    let _ = std::fs::remove_dir_all(&outside);
    let o = run(&db, &["export", "--to", &outside, "--force"]);
    assert!(!o.status.success(), "an export outside the root must be refused: {}", out(&o));
    assert!(!PathBuf::from(&outside).exists(), "and must not create the directory either");
    let o = run(&db, &["export", "--to", "/etc/brain-e2e", "--force"]);
    assert!(!o.status.success(), "an absolute path outside the root must be refused: {}", out(&o));
    // A backup destination outside the root is refused too; the default is not.
    let o = run(&db, &["backup", "--to", "/tmp/brain-e2e-stolen.bak"]);
    assert!(!o.status.success(), "a backup outside the root must be refused: {}", out(&o));
    assert!(!PathBuf::from("/tmp/brain-e2e-stolen.bak").exists());
    let _ = run(&db, &["store", "regras", "e2e/ttl", "## gone", "--scope", "global", "--expires-at", "2000-01-01T00:00:00Z"]);
    let o = run(&db, &["forget-sweep", "--dry-run"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("e2e/ttl"));
    let o = run(&db, &["forget-sweep"]);
    assert!(o.status.success(), "{}", out(&o));
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&exp);
}

#[test]
fn e2e_project_lifecycle() {
    let db = db("proj");
    let o = run(&db, &["project", "create", "e2ep", "--description", "d"]);
    assert!(o.status.success(), "{}", out(&o));
    let o = run(&db, &["project", "list"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("e2ep"));
    let _ = run(&db, &["store", "regras", "e2e/p", "## p", "--scope", "global"]);
    let o = run(&db, &["project", "link", "regras/global/e2e/p", "e2ep"]);
    assert!(o.status.success(), "{}", out(&o));
    let o = run(&db, &["project", "notes", "e2ep"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("e2e/p"));
    let o = run(&db, &["project", "unlink", "regras/global/e2e/p", "e2ep"]);
    assert!(o.status.success(), "{}", out(&o));
    let o = run(&db, &["project", "delete", "e2ep"]);
    assert!(o.status.success(), "{}", out(&o));
    cleanup(&db);
}

/// `run` with an explicit `BRAIN_EXPORT_ROOT`.
///
/// B6 makes the import archive its source, and the archive goes to the export
/// root. Without this, `e2e_migrate_vault` wrote `vault.bak.tar.gz` into the
/// **real** `/tmp/brain-export` — the directory the running production server and
/// its operator share — and because the non-clobber naming is per-root, every
/// parallel run of the suite left another numbered copy behind. A test that
/// writes into the live system's allowlist is a test that can collide with
/// production, so the root is redirected to a scratch directory like every other
/// fixture here.
fn run_in_export_root(db: &str, root: &str, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(bin());
    cmd.arg("--db").arg(db);
    for a in args {
        cmd.arg(a);
    }
    cmd.env("BRAIN_EXPORT_ROOT", root);
    cmd.output().expect("spawn brain")
}

#[test]
fn e2e_migrate_vault() {
    let db = db("mig");
    let vault = format!("/tmp/brain-cli-vault-{}", std::process::id());
    let root = format!("/tmp/brain-cli-exp-{}-mig", std::process::id());
    let _ = std::fs::remove_dir_all(&vault);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(format!("{}/regras/global", vault)).unwrap();
    std::fs::write(format!("{}/regras/global/mig.md", vault), "## migrated").unwrap();
    let o = run_in_export_root(&db, &root, &["migrate", "--vault", &vault]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("migrated 1"));
    // B6: the import archives its source before it writes, so the archive is part
    // of what a successful migration produced — not an optional extra.
    assert!(
        PathBuf::from(&root).join("vault.bak.tar.gz").exists(),
        "the import must archive the vault:\n{}",
        out(&o)
    );
    let _ = std::fs::remove_dir_all(&root);
    let o = run(&db, &["read", "regras", "mig", "--scope", "global"]);
    assert!(o.status.success(), "{}", out(&o));
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&vault);
}

#[test]
fn e2e_hook_session_lifecycle() {
    let db = db("hook");
    let xdg = format!("/tmp/brain-cli-xdg-{}", std::process::id());
    let _ = std::fs::remove_dir_all(&xdg);
    let payload = r#"{"id":"e2e-1","tool":"demo"}"#;
    // seed global context so session-start inject prints brain context
    let _ = run(&db, &["store", "regras", "e2e/ctx", "## padroes melhores praticas licoes", "--scope", "global"]);
    let mut cmd = Command::new(bin());
    cmd.env("BRAIN_DB_PATH", &db).env("XDG_RUNTIME_DIR", &xdg);
    let o = cmd.arg("hook").arg("--event").arg("session-start").arg("--project").arg("e2ep").arg("--payload").arg(payload).output().unwrap();
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("hook ok"));
    let mut cmd = Command::new(bin());
    cmd.env("BRAIN_DB_PATH", &db).env("XDG_RUNTIME_DIR", &xdg);
    let o = cmd.arg("hook").arg("--event").arg("session-start").arg("--project").arg("e2ep").arg("--payload").arg(payload).output().unwrap();
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("deduplicated"));
    let mut cmd = Command::new(bin());
    cmd.env("BRAIN_DB_PATH", &db).env("XDG_RUNTIME_DIR", &xdg);
    let o = cmd.arg("hook").arg("--event").arg("tool-result").arg("--project").arg("e2ep").arg("--payload").arg(payload).output().unwrap();
    assert!(o.status.success(), "{}", out(&o));
    let mut cmd = Command::new(bin());
    cmd.env("BRAIN_DB_PATH", &db).env("XDG_RUNTIME_DIR", &xdg);
    let o = cmd.arg("hook").arg("--event").arg("session-end").arg("--project").arg("e2ep").arg("--payload").arg(payload).output().unwrap();
    assert!(o.status.success(), "{}", out(&o));
    assert!(PathBuf::from(format!("{}/brain/hook-spool.jsonl", xdg)).exists());
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&xdg);
}


#[test]
fn e2e_hook_shared_auto_link() {
    // SH-02: hook --project shared cria sessoes/shared/<date> + auto-link erp+mobile
    let db = db("hookshared");
    let xdg = format!("/tmp/brain-cli-xdg-{}-shared", std::process::id());
    let _ = std::fs::remove_dir_all(&xdg);
    let payload = r#"{"id":"sh02-1","tool":"demo"}"#;
    let mut cmd = Command::new(bin());
    cmd.env("BRAIN_DB_PATH", &db).env("XDG_RUNTIME_DIR", &xdg);
    let o = cmd.arg("hook").arg("--event").arg("session-start").arg("--project").arg("shared").arg("--payload").arg(payload).output().unwrap();
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("hook ok"));
    // ambos project notes contêm shared
    let o = run(&db, &["project", "notes", "erp"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("sessoes/shared/"), "erp deve ver shared: {}", out(&o));
    let o = run(&db, &["project", "notes", "mobile"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("sessoes/shared/"), "mobile deve ver shared: {}", out(&o));
    // search --project ambos retornam
    let o = run(&db, &["search", "demo", "--project", "erp"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("sessoes/shared/"), "search erp: {}", out(&o));
    let o = run(&db, &["search", "demo", "--project", "mobile"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("sessoes/shared/"), "search mobile: {}", out(&o));
    // idempotência: segundo hook mesmo id dedup, links intactos
    let mut cmd = Command::new(bin());
    cmd.env("BRAIN_DB_PATH", &db).env("XDG_RUNTIME_DIR", &xdg);
    let o = cmd.arg("hook").arg("--event").arg("session-start").arg("--project").arg("shared").arg("--payload").arg(payload).output().unwrap();
    assert!(o.status.success(), "{}", out(&o));
    let o = run(&db, &["project", "notes", "erp"]);
    assert!(out(&o).contains("sessoes/shared/"));
    // regressão hook privado: sessão privada não cria link shared
    let payload2 = r#"{"id":"sh02-priv","tool":"priv"}"#;
    let mut cmd = Command::new(bin());
    cmd.env("BRAIN_DB_PATH", &db).env("XDG_RUNTIME_DIR", &xdg);
    let o = cmd.arg("hook").arg("--event").arg("session-start").arg("--project").arg("e2epriv").arg("--payload").arg(payload2).output().unwrap();
    assert!(o.status.success(), "{}", out(&o));
    let o = run(&db, &["project", "notes", "erp"]);
    assert!(!out(&o).contains("e2epriv"), "hook privado não deve vazar para erp");
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&xdg);
}

// ---------------------------------------------------- session-handoff-blind --
//
// The bug was two halves of the same silence: `session-start` injected only
// `layer=regras`, so a new session never learned where the previous one stopped,
// and the note the hook writes carried `project_id = NULL` with no link, so it was
// invisible under `project=` as well. D3 links it, D2 injects it.

/// T5 — the hook's own note belongs to its project, and replaying the day does not
/// multiply that membership.
///
/// **Mutation this fails on:** delete the `note_link_project` call from the hook —
/// `project notes projp` then returns `[]`, because the note is neither owned
/// (`project_id` stays `NULL` by design) nor linked, which is the F2 symptom.
#[test]
fn e2e_hook_links_the_session_note_to_its_own_project() {
    let db = db("hooklink");
    let xdg = format!("/tmp/brain-cli-xdg-{}-hooklink", std::process::id());
    let _ = std::fs::remove_dir_all(&xdg);
    let o = run(&db, &["project", "create", "projp"]);
    assert!(o.status.success(), "{}", out(&o));

    for id in ["link-1", "link-2"] {
        let o = hook(&db, &xdg, None, "session-start", "projp", id);
        assert!(o.status.success(), "the hook must never fail an event: {}", out(&o));
        assert!(
            !out(&o).contains("could not link"),
            "the project exists, so the link must succeed: {}",
            out(&o)
        );
    }

    // Two events, two sections: the second append was not deduplicated.
    let today = chrono_today();
    let path = format!("sessoes/projp/{today}");
    let content = read_note(&db, &path);
    assert_eq!(
        content.lines().filter(|l| l.starts_with("## ")).count(),
        2,
        "two session-start events must leave two sections:\n{content}"
    );

    // One membership, and it is the hook's own note. `note_projects` has
    // `PRIMARY KEY(note_path, project_id)`, so the second event's `INSERT OR IGNORE`
    // cannot add a second row; what this asserts is that the *first* one exists and
    // is visible through the project surface the handoff depends on.
    let o = run(&db, &["project", "notes", "projp"]);
    assert!(o.status.success(), "{}", out(&o));
    let listed = out(&o);
    assert_eq!(
        listed.matches(&path).count(),
        1,
        "the session note must appear exactly once in the project's notes:\n{listed}"
    );
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&xdg);
}

/// T6 — a new session is handed the previous one, and the empty marker does not
/// contradict it.
///
/// The seeded note is dated in the past on purpose: it stands in for "the last
/// session", so what is asserted is the content of *another* session, not the echo
/// of this very event in today's note.
///
/// The run carries a `--question` deliberately. That is the shape the plugin calls
/// with (`hooks/opencode/brain-session.js:140`), and it is the only shape in which
/// the session block can be the **sole** thing injected: with no question, the
/// project panorama runs instead and would satisfy "something was printed" on its
/// own. Without this, the `INJECT: (no context found)` assertion below is vacuous —
/// verified by restoring the old three-term condition, which this test then does not
/// notice.
#[test]
fn e2e_hook_session_start_injects_the_previous_session() {
    let db = db("hookinject");
    let xdg = format!("/tmp/brain-cli-xdg-{}-hookinject", std::process::id());
    let _ = std::fs::remove_dir_all(&xdg);
    let o = run(&db, &["project", "create", "inj"]);
    assert!(o.status.success(), "{}", out(&o));
    // A previous session's note: same project prefix, no owner, no link — the
    // legacy shape, reachable only by the prefix branch.
    let o = run(&db, &["store", "sessoes", "inj/2020-01-01", "## onde paramos\n\nONDE-PARAMOS: faltou o teste de integração"]);
    assert!(o.status.success(), "{}", out(&o));

    let o = hook_asking(&db, &xdg, "session-start", "inj", "inj-1", "ONDE-PARAMOS");
    assert!(o.status.success(), "{}", out(&o));
    let text = out(&o);
    assert!(
        text.contains("--- Brain context (sessões) ---"),
        "the handoff block is missing:\n{text}"
    );
    assert!(
        !text.contains("panorama, sem pergunta"),
        "with a question there is no panorama, so the session block is the only source:\n{text}"
    );
    assert!(
        text.contains("sessoes/inj/2020-01-01"),
        "the previous session's note must be named:\n{text}"
    );
    assert!(
        text.contains("ONDE-PARAMOS"),
        "the note's content must be injected, not just its path:\n{text}"
    );
    assert!(
        !text.contains("INJECT: (no context found)"),
        "a session was injected, so the empty marker must not also print:\n{text}"
    );
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&xdg);
}

/// T7 — the budget. A 5 KB session note must not push the rules block out of the
/// 4000-character append budget the plugin enforces
/// (`hooks/opencode/brain-session.js:38`): at most 400 characters of it reach the
/// session, and newlines are flattened so one note stays one line.
#[test]
fn e2e_hook_truncates_the_injected_session_tail_to_the_budget() {
    let db = db("hookbudget");
    let xdg = format!("/tmp/brain-cli-xdg-{}-hookbudget", std::process::id());
    let _ = std::fs::remove_dir_all(&xdg);
    let o = run(&db, &["project", "create", "bud"]);
    assert!(o.status.success(), "{}", out(&o));
    let big = format!("{}\nFIM-DA-NOTA", "X".repeat(5000));
    let o = run(&db, &["store", "sessoes", "bud/2020-01-01", &big]);
    assert!(o.status.success(), "{}", out(&o));

    let o = hook(&db, &xdg, None, "session-start", "bud", "bud-1");
    assert!(o.status.success(), "{}", out(&o));
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(text.contains("sessoes/bud/2020-01-01"), "the note must be injected:\n{text}");
    assert!(text.contains("FIM-DA-NOTA"), "the *tail* is what is injected, not the head:\n{text}");
    assert!(
        text.matches('X').count() <= 400,
        "at most 400 characters of a note may reach the session; got {}:\n{text}",
        text.matches('X').count()
    );
    assert!(
        text.chars().count() < 4000,
        "the whole injection must stay under the plugin's budget; got {}:\n{text}",
        text.chars().count()
    );
    assert_eq!(
        text.lines().filter(|l| l.starts_with("sessoes/bud/2020-01-01")).count(),
        1,
        "a flattened note is one line:\n{text}"
    );
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&xdg);
}

/// T8 — the negative half. With no project the hook skips *before* any injection,
/// so the session block cannot appear and the output is what it always was.
///
/// The seeded note is filed under the working directory's name, so a mutation that
/// fell back to the directory name — the shape the resolution code deliberately
/// refuses — would surface a block here instead of passing silently.
#[test]
fn e2e_hook_without_a_resolvable_project_injects_no_session_block() {
    let db = db("hooknoproj");
    let root = format!("/tmp/brain-cli-noproj-{}", std::process::id());
    let _ = std::fs::remove_dir_all(&root);
    let work = format!("{root}/work/brain-cli-noproj-dir");
    std::fs::create_dir_all(&work).unwrap();
    let brain_dir = format!("{root}/brain");
    let xdg = format!("{root}/xdg");
    let today = chrono_today();
    let seeded = format!("sessoes/brain-cli-noproj-dir/{today}");
    let o = run(&db, &["store", "sessoes", &format!("brain-cli-noproj-dir/{today}"), "## sessão anterior"]);
    assert!(o.status.success(), "{}", out(&o));

    let o = Command::new(bin())
        .current_dir(&work)
        .arg("--db")
        .arg(&db)
        .arg("hook")
        .arg("--event")
        .arg("session-start")
        .arg("--payload")
        .arg(r#"{"id":"np-1"}"#)
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_HOOK_EMBED", "0")
        .env("BRAIN_DIR", &brain_dir)
        .env("XDG_RUNTIME_DIR", &xdg)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("spawn brain hook");
    assert!(o.status.success(), "a hook must never fail the IDE: {}", out(&o));
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(text.contains("hook skipped"), "with nothing to resolve the event is skipped:\n{text}");
    assert!(
        !text.contains("--- Brain context"),
        "a skipped event must not inject anything:\n{text}"
    );
    assert!(
        !text.contains(&seeded),
        "no block, and in particular not the note filed under the directory's name:\n{text}"
    );
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&root);
}


// ------------------------------------ A1/A2 of the session-handoff-blind review --
//
// Two defects in what D1-D3 delivered, both measured on `data/brain.db`:
//
//   A1 — the hook writes the day's note *before* it reads `recent_session_notes`, so
//        the newest row was always today's and slot 1 injected the echo of the event
//        that was running (`## 16:24 session-start id=...` + payload). For a project
//        whose `sessoes` layer holds only those logs, both slots were echo: 800
//        characters of budget and zero handoff.
//   A2 — D3 links that note to the project, so `project_notes` (owned ∪ linked,
//        `updated_at DESC`) returned it first and the panorama's `take(5)` printed the
//        same note the session block prints, and could push the rules out.

/// T9 — the block prefers agent-written sections and demotes a log-only note to one
/// line, instead of spending 400 characters of budget per echo.
///
/// **Mutations this fails on:**
/// - make the `retain` unconditional (drop every `n.0 == full`, as it was before
///   `session-handoff-blind` loop 2) → see the sibling test
///   `e2e_hook_injects_a_curated_section_that_an_agent_appended_to_the_days_note`;
/// - delete `session_notes.retain(...)` → today's note comes back with
///   `session-start id=t9-1` (the running event's own dedup key) as the slot-1 tail;
/// - delete the heading filter in `session_digest` (treat every section as curated) →
///   the mechanical note is injected as a 400-character tail and `PAYLOAD-ECO`
///   reappears in stdout;
/// - drop the `is_clock` positional check and substring-match the event names → the
///   curated heading below (`## tool-result perdido na fila`) contains `tool-result`,
///   so it is demoted to a stub and the `Fila de embed` assertion loses its line.
///   This is why that heading names the event: a heading that mentions no event
///   classifies the same under both implementations, and the mutation survived.
#[test]
fn e2e_hook_session_block_prefers_curated_sections_over_the_event_echo() {
    let db = db("hookcurated");
    let xdg = format!("/tmp/brain-cli-xdg-{}-hookcurated", std::process::id());
    let _ = std::fs::remove_dir_all(&xdg);
    let o = run(&db, &["project", "create", "projx"]);
    assert!(o.status.success(), "{}", out(&o));

    // The mechanical note: nothing but what the hook itself writes. `PAYLOAD-ECO`
    // stands in for the JSON payload those sections carry.
    let mechanical = "# Sessão projx 2026-01-01\n\
                      ## 10:00 session-start id=a\n\nPAYLOAD-ECO anexo do evento\n\
                      ## 10:05 tool-result demo id=b\n\nPAYLOAD-ECO tambem\n";
    let o = run(&db, &["store", "sessoes", "projx/2026-01-01", mechanical]);
    assert!(o.status.success(), "{}", out(&o));

    // The curated note: the shape an agent leaves behind, under a heading the hook
    // never generates — and one that names an event, so a substring detector is
    // caught here.
    let curated = "# Sessão projx 2026-01-02\n## tool-result perdido na fila\nFila de embed com 3 NULL; teste X falhando.";
    let o = run(&db, &["store", "sessoes", "projx/2026-01-02-onde-paramos", curated]);
    assert!(o.status.success(), "{}", out(&o));

    let o = hook(&db, &xdg, None, "session-start", "projx", "t9-1");
    assert!(o.status.success(), "the hook must never fail an event: {}", out(&o));
    let text = String::from_utf8_lossy(&o.stdout).into_owned();

    // The running event's own note is the newest row; it must not be echoed back.
    assert!(
        !text.contains("session-start id=t9-1"),
        "slot 1 must not be the echo of the event that is running:\n{text}"
    );
    assert!(
        text.contains("--- Brain context (sessões) ---"),
        "the handoff block is missing:\n{text}"
    );

    // The curated note is injected as content, not as a stub.
    assert!(
        text.contains("Fila de embed com 3 NULL"),
        "the agent-written section must be injected:\n{text}"
    );

    // The log-only note is one line: path plus the last generated heading, with no
    // slice of its payload anywhere in the stdout.
    let mech_lines: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("sessoes/projx/2026-01-01"))
        .collect();
    assert_eq!(mech_lines.len(), 1, "a log-only note is one line:\n{text}");
    assert_eq!(
        mech_lines[0], "sessoes/projx/2026-01-01 ## 10:05 tool-result demo id=b",
        "the line carries the last generated heading and nothing else:\n{text}"
    );
    assert!(
        !text.contains("PAYLOAD-ECO"),
        "no payload of a mechanical section may reach the session:\n{text}"
    );

    // Exactly the two eligible notes, each on its own line.
    assert_eq!(
        text.lines().filter(|l| l.starts_with("sessoes/projx/")).count(),
        2,
        "two session slots, and today's note is not one of them:\n{text}"
    );
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&xdg);
}

/// A curated section that an agent appended to **today's** note must be injected —
/// and `LogOnly` is what makes the day's note ineligible, not its path.
///
/// The hook writes the day's note before it reads `recent_session_notes`, so the
/// unconditional `retain(|n| n.0 != full)` (A1's fix) also threw away a note an agent
/// had curated: the afternoon session lost the morning's "onde paramos" whenever the
/// agent put it where the hook also writes — a shape `session_digest` was explicitly
/// built for ("A note named `sessoes/<proj>/<YYYY-MM-DD>` can still hold an agent's
/// curated section").
///
/// **Mutation this fails on:** make the retain unconditional again → `CURADO-DO-DIA`
/// is missing from the block and the day's note is invisible.
#[test]
fn e2e_hook_injects_a_curated_section_that_an_agent_appended_to_the_days_note() {
    let db = db("hookcuratedtoday");
    let xdg = format!("/tmp/brain-cli-xdg-{}-hookcuratedtoday", std::process::id());
    let _ = std::fs::remove_dir_all(&xdg);
    let o = run(&db, &["project", "create", "projx"]);
    assert!(o.status.success(), "{}", out(&o));

    // First event: the hook creates the day's note and prints its real path, so this
    // test never has to reconstruct the date itself.
    let o = hook(&db, &xdg, None, "session-start", "projx", "day-1");
    assert!(o.status.success(), "the hook must never fail an event: {}", out(&o));
    let first = String::from_utf8_lossy(&o.stdout).into_owned();
    let note = first
        .split_whitespace()
        .find_map(|t| t.strip_prefix("note="))
        .unwrap_or_else(|| panic!("the hook prints note=<layer/path>:\n{first}"))
        .to_string();
    let (layer, rest) = note.split_once('/').unwrap_or_else(|| panic!("layer/path expected, got {note}"));

    // The agent curates the day's note. This is the write the unconditional retain
    // discarded.
    let curated = "# Sessão projx\n## onde paramos\nCURADO-DO-DIA: fila de embed com 3 NULL.\n";
    let o = run(&db, &["store", layer, rest, curated]);
    assert!(o.status.success(), "{}", out(&o));

    // Second event: appended to the same note, after the curated section.
    let o = hook(&db, &xdg, None, "session-start", "projx", "day-2");
    assert!(o.status.success(), "the hook must never fail an event: {}", out(&o));
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(
        text.contains("--- Brain context (sessões) ---"),
        "the day's curated note must open the block:\n{text}"
    );
    assert!(
        text.contains("CURADO-DO-DIA"),
        "a curated section on the day's own note must be injected:\n{text}"
    );
    assert!(
        !text.contains("id=day-2"),
        "the echo of the running event must still not be injected:\n{text}"
    );
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&xdg);
}

/// T10 — the panorama is the project's overview, not its activity feed: session notes
/// are excluded there and appear only in the session block.
///
/// The run carries **no** `--question`, which is the panorama path — the same shape
/// the measured duplication was observed in (with a question the panorama never runs).
///
/// **Mutation this fails on:** delete `.filter(|n| n.layer != "sessoes")` → today's
/// linked session note takes slot 1 of the panorama, `sessoes/` appears inside the
/// panorama block, and the note is printed twice.
#[test]
fn e2e_hook_panorama_excludes_session_notes() {
    let db = db("hookpanorama");
    let xdg = format!("/tmp/brain-cli-xdg-{}-hookpanorama", std::process::id());
    let _ = std::fs::remove_dir_all(&xdg);
    let o = run(&db, &["project", "create", "pj2"]);
    assert!(o.status.success(), "{}", out(&o));

    let o = run(&db, &["store", "regras", "pj2/naming", "## Naming\n\nUse kebab-case.", "--scope", "projetos", "--project", "pj2"]);
    assert!(o.status.success(), "{}", out(&o));
    let o = run(&db, &["store", "arquitetura", "pj2/stack", "## Stack\n\nRust + SQLite.", "--scope", "projetos", "--project", "pj2"]);
    assert!(o.status.success(), "{}", out(&o));
    for day in ["2026-01-01", "2026-01-02", "2026-01-03"] {
        let o = run(&db, &["store", "sessoes", &format!("pj2/{day}"), "## onde paramos\n\nlog do dia"]);
        assert!(o.status.success(), "{}", out(&o));
    }

    let o = hook(&db, &xdg, None, "session-start", "pj2", "t10-1");
    assert!(o.status.success(), "the hook must never fail an event: {}", out(&o));
    let text = String::from_utf8_lossy(&o.stdout).into_owned();

    let header = "--- Brain context (projeto pj2 — panorama, sem pergunta) ---";
    let start = text.find(header).unwrap_or_else(|| panic!("panorama header missing:\n{text}"));
    let rest = &text[start + header.len()..];
    let end = rest.find("\n---").unwrap_or(rest.len());
    let panorama = &rest[..end];

    assert!(
        panorama.contains("regras/projetos/pj2/naming"),
        "the knowledge layers must be in the panorama:\n{text}"
    );
    assert!(
        panorama.contains("arquitetura/projetos/pj2/stack"),
        "the knowledge layers must be in the panorama:\n{text}"
    );
    assert!(
        !panorama.contains("sessoes/"),
        "the panorama is an overview, not the activity feed:\n{text}"
    );

    // The session block still runs, and it is the only place a session note appears.
    assert!(
        text.contains("--- Brain context (sessões) ---"),
        "the handoff block must still run:\n{text}"
    );
    // Counted over block lines, not raw substrings: the trailing `hook ok` line names
    // the note the hook just wrote, which is a session path too.
    assert_eq!(
        text.lines().filter(|l| l.starts_with("sessoes/pj2/")).count(),
        2,
        "each session note is printed once, in the session block only:\n{text}"
    );
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&xdg);
}


// -------------------------------------------------------------- Y-02 --
//
// `serve` and `serve-mcp` used to be started on **fixed** ports (18341/18342) and
// only the viewer was polled, so the MCP client was fired at a port its server
// had not bound yet. That is one failure mode. The worse one is a *collision*:
//
//   - something already listening on the port  -> the child dies with
//     `Address already in use (os error 98)`, deterministically, and the test
//     reports a protocol error 15 s later that does not exist;
//   - something that is a **working brain server** on the port (two overlapping
//     `cargo test` runs, an orphaned `brain serve`, an IDE cargo task) -> the
//     test talks to the *wrong server* and dies with `IncompleteRead(0 bytes
//     read)` or `no response id=3`, blaming SSE.
//
// Measured on this machine: 1 failure in 5 isolated runs for one agent, 2 in 8
// for another. Both numbers are the same bug.
//
// The fix is ephemeral ports, polled on **both** children, with a liveness check
// that names the child and its own stderr — so a bind failure is reported as a
// bind failure. See `free_port` for the TOCTOU window that is accepted, and why.

/// A port that was free a moment ago.
///
/// Bind-and-release, so there is a **TOCTOU window** between this `drop` and the
/// child binding it. That window is accepted and documented rather than closed:
/// closing it means handing the listening fd to the child (socket activation /
/// `LISTEN_FDS`), which neither `serve` nor `serve-mcp` accepts, and it would
/// make the test a different kind of object than the thing it tests. The window
/// is a few microseconds wide and the kernel does not hand the same ephemeral
/// port to a second bind in between, so the practical risk is a squatter that
/// grabs *that exact* port in *that exact* window — which is what the collision
/// test below covers, and what the diagnostics now make legible instead of
/// mislabelled as a protocol bug.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind an ephemeral port")
        .local_addr()
        .expect("ephemeral port has an address")
        .port()
}

/// A spawned server: killed on drop, with its own stdout and stderr on disk.
///
/// Both streams go to **files**, not pipes: a long-lived server writing to a pipe
/// nobody drains eventually blocks, and a test that can deadlock on its own
/// diagnostics is worse than one that cannot.
struct Server {
    child: std::process::Child,
    name: &'static str,
    port: u16,
    outlog: PathBuf,
    errlog: PathBuf,
}

impl Server {
    fn spawn(name: &'static str, db: &str, port: u16, sub: &str) -> Server {
        Server::spawn_with_env(name, db, port, sub, &[])
    }

    fn spawn_with_env(name: &'static str, db: &str, port: u16, sub: &str, env: &[(&str, &str)]) -> Server {
        let stem = format!("brain-e2e-{}-{name}-{port}", std::process::id());
        let outlog = std::env::temp_dir().join(format!("{stem}.out"));
        let errlog = std::env::temp_dir().join(format!("{stem}.err"));
        let mut cmd = Command::new(bin());
        cmd.arg("--db")
            .arg(db)
            .arg(sub)
            .arg("--port")
            .arg(port.to_string())
            // A dead Ollama, like the rest of this suite. This used to be
            // `env_remove("BRAIN_OLLAMA_URL")`, which does not mean "no Ollama" — it
            // means the **default** Ollama, `http://localhost:11434`, i.e. whatever
            // real model the developer happens to be running. That made this the only
            // fixture in the file that talked to a real backend, and it was the 3-5%
            // flake: `brain_search` embeds the query synchronously
            // (`rmcp_service::embed_query`), bounded by a 30 s socket timeout, so
            // every concurrently running test binary queued on the same single
            // Ollama. Measured on this host, the `tools/call` round trip went from
            // 0.141 s idle to **8.899 s** with the suite running, against a client
            // window of 15 s — so a contended embed lands the response after the
            // client has already asserted. Nothing is lost in transit; the client
            // gives up first. Reproduced deterministically in
            // `mcp_sse_window.rs`, which fails on purpose to keep the mechanism
            // visible.
            //
            // Nothing here needs a real vector: the assertions read `/api/status`
            // and an FTS-fallback search. A caller that genuinely wants a backend
            // passes it in `env` and it wins, because that loop runs after this.
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            .env_remove("BRAIN_EMBED_MAX_FAILURES")
            .stdout(std::fs::File::create(&outlog).expect("create the child's stdout file"))
            .stderr(std::fs::File::create(&errlog).expect("create the child's stderr file"));
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = cmd.spawn().unwrap_or_else(|e| panic!("spawn `{name}`: {e}"));
        Server { child, name, port, outlog, errlog }
    }

    fn stderr(&self) -> String {
        std::fs::read_to_string(&self.errlog).unwrap_or_default()
    }

    /// True when *this child* announced *this port* on its own stdout.
    ///
    /// This is the identity check, and it is why the test does not use a bare TCP
    /// connect as its readiness signal. **A connect succeeds against a squatter**
    /// — the squatter's backlog accepts it — so "can I connect?" cannot tell my
    /// server from somebody else's, which is precisely how the old fixed-port
    /// test ended up talking to the wrong server and blaming SSE. Both `serve` and
    /// `serve-mcp` print the port they are about to serve, so that line is proof
    /// of identity and proof that the bind was reached.
    fn announced_its_port(&self) -> bool {
        let out = std::fs::read_to_string(&self.outlog).unwrap_or_default();
        out.contains(&format!(":{}/", self.port))
    }

    /// True while the process is alive. Checked at every step, so a child that
    /// died is reported as *that child dying* and not as a protocol failure
    /// somewhere downstream.
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// A message that names the child, the port and the reason — including the
    /// `Address already in use` that distinguishes a collision from anything
    /// else.
    fn why_not_listening(&mut self) -> String {
        let err = self.stderr();
        let tail: Vec<&str> = err.lines().rev().take(3).collect();
        let tail = tail.into_iter().rev().collect::<Vec<_>>().join(" | ");
        if tail.contains("Address already in use") || tail.contains("os error 98") {
            format!(
                "PORT COLLISION: {} could not bind 127.0.0.1:{} because something else already holds it. \
                 This is not a protocol bug. Re-run, or free the port. Its stderr: {tail}",
                self.name, self.port
            )
        } else {
            format!("{} never listened on 127.0.0.1:{}. Its stderr: {tail}", self.name, self.port)
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.outlog);
        let _ = std::fs::remove_file(&self.errlog);
    }
}

/// Wait until `server` is provably up: it announced *this* port, it is still
/// alive, and the port accepts a connection.
///
/// All three, and each rules out a different impostor: the announcement rules out
/// another server, the liveness check turns a dead child into a named bind
/// failure instead of a downstream protocol error, and the connect confirms the
/// bind completed. The MCP server answers `/sse` with a stream that never ends, so
/// an HTTP request against it can only ever time out — a connect is the only
/// transport-level probe that works for both children.
fn wait_until_listening(server: &mut Server, timeout: std::time::Duration) -> Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !server.alive() {
            return Err(server.why_not_listening());
        }
        if server.announced_its_port() && std::net::TcpStream::connect(("127.0.0.1", server.port)).is_ok() {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(server.why_not_listening());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// Run the coexistence scenario on a pair of freshly chosen ports.
///
/// Factored out so two tests can drive it: the plain one, and the one that squats
/// the old hard-coded ports first.
/// The two ports the run under the squatters used, returned so a caller can assert they
/// were not the legacy fixed ones.
fn serve_and_mcp_coexist_on_free_ports(db: &str) -> (u16, u16) {
    // Ephemeral, so two runs of this binary — or this binary and a real server,
    // or an IDE cargo task — can never land on each other.
    let viewer_port = free_port();
    let mcp_port = free_port();
    assert_ne!(viewer_port, mcp_port, "the two servers must get different ports");

    let mut viewer = Server::spawn("viewer", db, viewer_port, "serve");
    let mut mcp = Server::spawn("mcp", db, mcp_port, "serve-mcp");

    // **Both** ports are polled before any client is fired. Only polling the
    // viewer is what let the MCP client race a server that had not bound yet.
    let timeout = std::time::Duration::from_secs(30);
    wait_until_listening(&mut viewer, timeout).unwrap_or_else(|e| panic!("{e}"));
    wait_until_listening(&mut mcp, timeout).unwrap_or_else(|e| panic!("{e}"));
    assert!(viewer.alive() && mcp.alive(), "both servers must still be up after binding");

    let get = |url: &str| {
        let o = Command::new("curl").arg("-s").arg("-m").arg("5").arg(url).output().unwrap();
        assert!(o.status.success(), "curl {url} failed");
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    let body = get(&format!("http://127.0.0.1:{viewer_port}/api/status"));
    assert!(body.contains("notes"), "{}", body);

    // Real MCP handshake via helper client (initialize -> tools/list -> tools/call)
    let helper = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/helpers/mcp_client.py");
    let o = Command::new("python3")
        .arg(&helper)
        .arg(format!("http://127.0.0.1:{mcp_port}"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&o.stdout).into_owned() + String::from_utf8_lossy(&o.stderr).as_ref();
    // The children are checked *first*: a client failure against a dead server is
    // a server failure, and saying so is the whole point.
    assert!(mcp.alive() && viewer.alive(), "a server died during the handshake: {text}");
    assert!(o.status.success(), "mcp client failed: {}", text);
    assert!(text.contains("MCP-HANDSHAKE-OK"), "{}", text);

    drop(viewer);
    drop(mcp);
    (viewer_port, mcp_port)
}

#[test]
fn e2e_serve_viewer_and_mcp_coexist() {
    let db = db("serve");
    let _ = run(&db, &["store", "regras", "e2e/sv", "## serve smoke", "--scope", "global"]);
    serve_and_mcp_coexist_on_free_ports(&db);
    cleanup(&db);
}

/// Y-02: the old hard-coded ports being **occupied** must not affect this test.
///
/// This is the mutation that discriminates the fix, and it is here because
/// reverting to fixed ports is *not* caught by running the test once: with nothing
/// on 18341/18342 the fixed-port version passes, which is exactly why the bug
/// survived four review rounds — it only failed when the machine happened to have
/// something there. Occupying both ports makes the difference deterministic: a
/// fixed-port version collides and reports a collision, this one never looks at
/// those ports at all.
///
/// The property is checked on **every** run, with no skip.
///
/// The skip that used to be here was a hole in the only test that discriminates the
/// fix: it returned early when a port was already occupied, which is precisely the
/// condition the test is about, so on a busy host — or on a machine where the bug is
/// live and something really is listening on 18341 — the test could silently not run.
/// Occupying a port does not need our cooperation. If the bind succeeds we hold the
/// squatter; if it fails, the port is occupied by whatever beat us to it, which is the
/// same precondition reached by a different route. There is no third case, so there is
/// nothing to skip.
#[test]
fn a_squatter_on_the_legacy_fixed_ports_does_not_affect_the_coexistence_test() {
    let mut squatters = Vec::new();
    for p in [18341u16, 18342] {
        match std::net::TcpListener::bind(("127.0.0.1", p)) {
            Ok(l) => squatters.push(l),
            Err(e) => println!("port {p} is held by something else ({e}); that is the precondition, so carrying on"),
        }
    }
    let db = db("legacyports");
    let _ = run(&db, &["store", "regras", "e2e/sv", "## serve smoke", "--scope", "global"]);
    let (viewer_port, mcp_port) = serve_and_mcp_coexist_on_free_ports(&db);
    // And the direct form of the same claim: with the legacy ports occupied, the ports
    // this run actually bound are not them. `free_port` takes the kernel's ephemeral
    // range (32768–60999 here), which excludes 18341 and 18342 outright, so this is a
    // discriminator on every host rather than one that needs a skip to stay quiet.
    for p in [viewer_port, mcp_port] {
        assert!(
            p != 18341 && p != 18342,
            "a run must not reuse a legacy fixed port while it is squatted: bound {p}"
        );
    }
    drop(squatters);
    cleanup(&db);
}

/// Y-02: a squatter on the port is reported as a **port** failure, not as an SSE
/// protocol error.
///
/// The regression this pins is the *diagnosis*, not the bind. With a fixed port
/// and a working server sitting on it, the old test spoke to the wrong server and
/// died 15 s later with `IncompleteRead(0 bytes read)` — a bug report about the
/// MCP handshake that does not exist. Here the port is squatted, the child cannot
/// bind, and the message has to say so.
#[test]
fn a_port_already_taken_is_reported_as_a_port_failure() {
    let db = db("squatter");
    // Hold the port for the whole test. `SO_REUSEADDR` is deliberately NOT set:
    // without it, binding the same port again fails, which is the collision.
    let squatter = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind the squatter");
    let taken = squatter.local_addr().unwrap().port();
    let mut server = Server::spawn("squatted", &db, taken, "serve");

    let err = wait_until_listening(&mut server, std::time::Duration::from_secs(15))
        .expect_err("the server must not manage to bind a port the squatter holds");
    // The regression this pins is the *diagnosis*, so the diagnosis is what is
    // asserted. `starts_with`, not `contains`: `why_not_listening`'s other branch
    // embeds the child's stderr verbatim, and that stderr contains
    // `Address already in use (os error 98)` on its own — so a `contains` on the errno
    // is satisfied by the very output it was meant to distinguish from, and deleting
    // the `PORT COLLISION` branch outright left this test green. The prefix exists only
    // on the collision branch, so it is the one token that can carry the claim.
    assert!(
        err.starts_with("PORT COLLISION"),
        "a bind failure must be named as one, not blamed on the protocol: {err}"
    );

    // The announcement is printed only *after* a successful bind, so a server that
    // could not bind must never have claimed the port. This is deterministic once
    // the child has exited — its stdout file is complete — and it is the property
    // the main test's readiness check relies on: an announcement printed before the
    // bind would be satisfied by a child that is about to die, and by a squatter
    // sitting in the port it announced.
    let announcement = std::fs::read_to_string(&server.outlog).unwrap_or_default();
    assert!(
        !announcement.contains(&format!(":{}/", taken)),
        "a server that failed to bind must not have announced the port. Got: {announcement:?}\n\
         Printing it before the bind makes the line a statement of intent, which is what let the old \
         fixed-port test pass its readiness check against the wrong server."
    );
    drop(server);
    cleanup(&db);
}


// ------------------------------------------------------------------ W-03 --
//
// The session hook. The properties that matter, and the bugs they pin:
//
// 1. a hook event that would push the day's note past a write limit **rotates to a
//    new note** instead of being refused. The caller swallows the hook's exit code,
//    so a refusal is a silently broken hook — worse than the bug it fixes. Nothing
//    may be lost: the day's note keeps every section it had.
// 2. the embed is a **diff**. The comment in the source claimed a session note
//    "only ever embeds the new section"; the code embedded the whole accumulated
//    document on every event, which is quadratic. Measured here by counting the
//    requests a mock receives from N hook subprocesses.
//
// Both run the real binary as a subprocess against a local mock Ollama, so the
// measurement is of the shipped path and not of a helper.

/// A mock Ollama that counts embedding requests, plus the URL to hand the CLI.
struct CountingOllama {
    url: String,
    requests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    _handle: std::thread::JoinHandle<()>,
}

fn counting_ollama() -> CountingOllama {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock ollama");
    let addr = listener.local_addr().unwrap();
    let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = requests.clone();
    let handle = std::thread::spawn(move || {
        // One connection at a time, one request each. The CLI makes plain HTTP/1.1
        // calls with `Connection: close` semantics via reqwest, and the only thing
        // the test needs is the count.
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut content_length = 0usize;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                    break;
                }
                if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_length];
            use std::io::Read;
            let _ = reader.read_exact(&mut body);
            let is_embed = line.contains("/api/embeddings");
            if is_embed {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            let payload = if is_embed {
                // A real, non-zero, correctly sized vector.
                let v: Vec<String> = (0..768).map(|i| format!("{}", (i % 17 + 1) as f32 / 16.0)).collect();
                format!("{{\"embedding\":[{}]}}", v.join(","))
            } else {
                "{\"models\":[]}".to_string()
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            let _ = stream.flush();
        }
    });
    CountingOllama { url: format!("http://{addr}"), requests, _handle: handle }
}

impl CountingOllama {
    fn requests(&self) -> usize {
        self.requests.load(std::sync::atomic::Ordering::SeqCst)
    }
}

fn hook(db: &str, xdg: &str, ollama: Option<&str>, event: &str, project: &str, id: &str) -> std::process::Output {
    let payload = format!("{{\"id\":\"{id}\",\"tool\":\"demo\"}}");
    let mut cmd = Command::new(bin());
    cmd.env("BRAIN_DB_PATH", db).env("XDG_RUNTIME_DIR", xdg);
    if let Some(url) = ollama {
        cmd.env("BRAIN_OLLAMA_URL", url);
    } else {
        // A port nobody is listening on: the hook must still capture the event.
        cmd.env("BRAIN_OLLAMA_URL", "http://127.0.0.1:1");
    }
    cmd.arg("hook").arg("--event").arg(event).arg("--project").arg(project).arg("--payload").arg(&payload);
    cmd.output().expect("spawn brain hook")
}

/// The same hook, plus the user's question — the shape the plugin actually calls in
/// (`hooks/opencode/brain-session.js:140` passes the prompt it captured). Used where
/// the test needs the panorama path to stay out of the way.
fn hook_asking(db: &str, xdg: &str, event: &str, project: &str, id: &str, question: &str) -> std::process::Output {
    let payload = format!("{{\"id\":\"{id}\",\"tool\":\"demo\"}}");
    let mut cmd = Command::new(bin());
    cmd.env("BRAIN_DB_PATH", db)
        .env("XDG_RUNTIME_DIR", xdg)
        // A port nobody is listening on: the event must be captured anyway.
        .env("BRAIN_OLLAMA_URL", "http://127.0.0.1:1");
    cmd.arg("hook")
        .arg("--event")
        .arg(event)
        .arg("--project")
        .arg(project)
        .arg("--question")
        .arg(question)
        .arg("--payload")
        .arg(&payload);
    cmd.output().expect("spawn brain hook")
}

/// W-03.1 + W-03.2 together, because they are the same scenario: many events, one
/// growing note, one mock counting every embedding request.
#[test]
fn e2e_hook_embeds_only_the_delta_and_rotates_instead_of_refusing() {
    let db = db("hookdelta");
    let xdg = format!("/tmp/brain-cli-xdg-{}-hookdelta", std::process::id());
    let _ = std::fs::remove_dir_all(&xdg);
    let mock = counting_ollama();

    // Eight events, each adding one `## ` section to the same day's note.
    for i in 0..8 {
        let o = hook(&db, &xdg, Some(&mock.url), "tool-result", "delta", &format!("ev-{i}"));
        assert!(o.status.success(), "the hook must never fail an event: {}", out(&o));
        assert!(out(&o).contains("hook ok"), "{}", out(&o));
    }

    // One request for the note's header chunk plus one per appended section, and
    // nothing more. The old code re-embedded the whole accumulated document on
    // every event, which for this shape is 2+3+...+9 = 44 requests; the diff is 9.
    // (The header counts separately because `chunk_text` emits the text before the
    // first `## ` as its own chunk.)
    assert_eq!(mock.requests(), 9, "the hook must embed only what grew, not the whole note each time");

    // Every event is captured, in the day's note.
    let today = chrono_today();
    let base = format!("sessoes/delta/{today}");
    let content = read_note(&db, &base);
    for i in 0..8 {
        assert!(content.contains(&format!("id=ev-{i}")), "event {i} is missing from {base}");
    }
    let _ = std::fs::remove_dir_all(&xdg);
    let _ = std::fs::remove_file(&db);
}

/// X-01, second bug: the dedup marker used to be a prefix of another event's.
///
/// The store asks "is this event already in the note?" by substring search, so an
/// `id=d1` marker is found inside an `id=d11` section. Whichever of the two landed
/// second was discarded as a duplicate — a lost event with no error, and
/// reachable from a plain sequential run, so it does not need the race to show up.
///
/// Deterministic, unlike the concurrency test: the long id is written first on
/// purpose, because that is the order that triggers it.
#[test]
fn e2e_hook_captures_an_event_whose_id_is_a_prefix_of_another() {
    let db = db("hookprefix");
    let xdg = format!("/tmp/brain-cli-xdg-{}-hookprefix", std::process::id());
    let _ = std::fs::remove_dir_all(&xdg);
    let today = chrono_today();
    let base = format!("sessoes/pfx/{today}");

    // The long id first: it is the one whose text contains the short id.
    for id in ["pfx-11", "pfx-1", "pfx-111", "pfx-1x"] {
        let o = hook(&db, &xdg, None, "tool-result", "pfx", id);
        assert!(o.status.success(), "the hook must never fail an event: {}", out(&o));
    }

    let content = read_note(&db, &base);
    for id in ["pfx-11", "pfx-1", "pfx-111", "pfx-1x"] {
        assert!(content.contains(id), "event {id} was silently deduplicated away. Content:\n{content}");
    }
    assert_eq!(
        content.lines().filter(|l| l.starts_with("## ")).count(),
        4,
        "four events, four sections. Content:\n{content}"
    );

    // And the deduplication itself still works: replaying an id changes nothing.
    let before = read_note(&db, &base);
    let o = hook(&db, &xdg, None, "tool-result", "pfx", "pfx-1");
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("deduplicated"), "a replayed id must be deduplicated: {}", out(&o));
    assert_eq!(read_note(&db, &base), before, "a deduplicated event must not be appended again");

    let _ = std::fs::remove_dir_all(&xdg);
    cleanup(&db);
}

fn chrono_today() -> String {
    // Avoids adding a chrono dev-dependency to the test crate for one format call.
    let out = Command::new("date").arg("-u").arg("+%Y-%m-%d").output().expect("date");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

// ---------------------------------------------------------------------------
// X-01: the hook's read-modify-write of the day's note, under real concurrency.
// ---------------------------------------------------------------------------

/// Events per round, and rounds. Both are load, not correctness: a *fixed* hook
/// satisfies the invariant on every round, so raising them only makes the test
/// better at catching a *reverted* one.
const HOOK_RACE_EVENTS: usize = 12;
const HOOK_RACE_ROUNDS: usize = 8;

/// One event's payload, sized so the write is a real write.
///
/// ~4 KiB, the shape the reviewer measured. It matters that the note *grows*:
/// every `note_upsert` copies the whole accumulated content into `audit_log`, so
/// the later events in a round do the most work and have the widest window to
/// lose a section in.
fn race_payload(id: &str) -> String {
    let filler = "x".repeat(3800);
    format!("{{\"id\":\"{id}\",\"tool\":\"demo\",\"payload\":\"{filler}\"}}")
}

/// X-01. N real hook processes, launched together, must not lose an event.
///
/// **Why processes and not threads.** The bug is a lost update between two
/// *independent* SQLite connections. Threads inside one process would share
/// nothing here either, but spawning the actual binary is the only arrangement
/// that also covers the file lock in the spool and the per-process `Store` the
/// real hook opens — which is the code under test.
///
/// **Why several rounds.** It is a race, so a single round that happens to
/// serialise proves nothing. Eight rounds of twelve events: the test fails on
/// the unfixed code as soon as one section goes missing in any of them, and on
/// fixed code the transaction makes the invariant unconditional rather than
/// probable, so there is nothing for the repetition to flake on.
///
/// The spool is shared across the round (one `XDG_RUNTIME_DIR`) and the database
/// is per-round, so the only thing the processes contend for is the note.
#[test]
fn e2e_concurrent_hooks_never_lose_a_session_event() {
    let today = chrono_today();
    let xdg = format!("/tmp/brain-cli-xdg-{}-hookrace", std::process::id());
    let project = "race";

    for round in 0..HOOK_RACE_ROUNDS {
        let db = db(&format!("hookrace{round}"));
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir_all(&xdg);

        // Spawn every process before waiting on any of them, so they overlap.
        // `BRAIN_HOOK_EMBED=0` keeps Ollama out of it entirely: the property under
        // test is the note's content, and an embed in the middle would make a
        // failure ambiguous between "lost the section" and "the model was cold".
        let mut children = Vec::with_capacity(HOOK_RACE_EVENTS);
        for i in 0..HOOK_RACE_EVENTS {
            let id = format!("r{round}-ev-{i}");
            let mut cmd = Command::new(bin());
            cmd.env("BRAIN_DB_PATH", &db)
                .env("XDG_RUNTIME_DIR", &xdg)
                .env("BRAIN_OLLAMA_URL", "http://127.0.0.1:1")
                .env("BRAIN_HOOK_EMBED", "0")
                .arg("hook")
                .arg("--event")
                .arg("tool-result")
                .arg("--project")
                .arg(project)
                .arg("--payload")
                .arg(race_payload(&id));
            children.push((id, cmd.spawn().expect("spawn brain hook")));
        }

        let mut failures = Vec::new();
        for (id, child) in children {
            match child.wait_with_output() {
                Ok(o) if o.status.success() => {}
                Ok(o) => failures.push(format!("{id} exited {:?}: {}", o.status.code(), out(&o))),
                Err(e) => failures.push(format!("{id} could not be waited on: {e}")),
            }
        }
        assert!(
            failures.is_empty(),
            "round {round}: {} of {HOOK_RACE_EVENTS} concurrent hooks failed outright. A hook that drops an \
             event under concurrency is not a degraded hook, it is a broken one:\n{}",
            failures.len(),
            failures.join("\n")
        );

        let base = format!("sessoes/{project}/{today}");
        let content = read_note(&db, &base);
        // Section count, not just id presence: a lost update can in principle
        // leave an id behind while dropping the section that carried it, and the
        // count is the property that has to hold unconditionally.
        let sections = content.lines().filter(|l| l.starts_with("## ")).count();
        let missing: Vec<String> = (0..HOOK_RACE_EVENTS)
            .map(|i| format!("r{round}-ev-{i}"))
            .filter(|id| !content.contains(id.as_str()))
            .collect();
        assert_eq!(
            sections, HOOK_RACE_EVENTS,
            "round {round}: {base} holds {sections} section(s) for {HOOK_RACE_EVENTS} events — a section was \
             lost to a concurrent writer. Events absent from the note: {missing:?}"
        );
        for i in 0..HOOK_RACE_EVENTS {
            let id = format!("r{round}-ev-{i}");
            assert!(content.contains(&id), "round {round}: event {id} is missing from {base}");
        }

        let _ = std::fs::remove_dir_all(&xdg);
        cleanup(&db);
    }
}

/// The content of a `sessoes/<project>/<date>` note, read back through the real
/// `brain read` rather than by opening the database from the test.
fn read_note(db: &str, path: &str) -> String {
    let rel = path.strip_prefix("sessoes/").expect("a sessoes path");
    out(&run(db, &["read", "sessoes", rel]))
}

/// W-03.1 on its own: the day's note hits the size limit, and the hook must open a
/// new note rather than drop the event.
#[test]
fn e2e_hook_rotates_the_day_note_instead_of_refusing_the_event() {
    let db = db("hookrotate");
    let xdg = format!("/tmp/brain-cli-xdg-{}-hookrotate", std::process::id());
    let _ = std::fs::remove_dir_all(&xdg);
    let today = chrono_today();
    let base = format!("sessoes/big/{today}");

    // Seed a note that is already at the chunk limit: the next event cannot fit.
    // `MAX_CHUNKS` is 64 and the header line before the first `## ` is a chunk of
    // its own, so 63 sections puts the note exactly at the cap.
    let mut seeded = String::from("# Sessão big hoje\n");
    for i in 0..(brain_core_chunks_cap() - 1) {
        seeded.push_str(&format!("## filler {i}\n\nprose to fill the chunk budget {i}\n"));
    }
    let o = run(&db, &["store", "sessoes", &format!("big/{today}"), &seeded]);
    assert!(o.status.success(), "{}", out(&o));
    let before = read_note(&db, &base);
    assert!(before.contains("filler 0"), "the fixture must be the day's note");

    // The event that does not fit. It must succeed, and land in a new note.
    let o = hook(&db, &xdg, None, "tool-result", "big", "overflow-1");
    assert!(o.status.success(), "the hook must not fail when the note is full: {}", out(&o));
    assert!(out(&o).contains("hook ok"), "{}", out(&o));
    assert!(out(&o).contains(&format!("{base}-2")), "a rotated note must be named after the day's note: {}", out(&o));

    // The day's note is untouched, and the event is not lost: it is in the new note.
    assert_eq!(read_note(&db, &base), before, "the day's note must not be truncated or rewritten");
    let rotated = read_note(&db, &format!("{base}-2"));
    assert!(rotated.contains("id=overflow-1"), "the event must be captured in the rotated note: {}", rotated);
    assert!(rotated.contains(&base), "the rotated note must point back at the one it continues");

    // A second overflow rotates again rather than overwriting the first part.
    let o = hook(&db, &xdg, None, "tool-result", "big", "overflow-2");
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains(&format!("{base}-3")), "the second overflow must open a third part: {}", out(&o));
    let second = read_note(&db, &format!("{base}-3"));
    assert!(second.contains("id=overflow-2"), "{}", second);
    // And the first part is still there, unmerged and untruncated.
    assert!(read_note(&db, &format!("{base}-2")).contains("id=overflow-1"));

    let _ = std::fs::remove_dir_all(&xdg);
    let _ = std::fs::remove_file(&db);
}

/// `MAX_CHUNKS`, spelled out rather than read: the test crate deliberately does not
/// depend on `brain-core`, and the number is also the fixture's business.
fn brain_core_chunks_cap() -> usize {
    64
}

/// `BRAIN_HOOK_EMBED=0` turns the embed off without turning the capture off.
#[test]
fn e2e_hook_can_skip_the_embed_but_never_the_capture() {
    let db = db("hooknoembed");
    let xdg = format!("/tmp/brain-cli-xdg-{}-hooknoembed", std::process::id());
    let _ = std::fs::remove_dir_all(&xdg);
    let mock = counting_ollama();
    let today = chrono_today();

    let mut cmd = Command::new(bin());
    cmd.env("BRAIN_DB_PATH", &db)
        .env("XDG_RUNTIME_DIR", &xdg)
        .env("BRAIN_OLLAMA_URL", &mock.url)
        .env("BRAIN_HOOK_EMBED", "0")
        .arg("hook")
        .arg("--event")
        .arg("tool-result")
        .arg("--project")
        .arg("noembed")
        .arg("--payload")
        .arg("{\"id\":\"ne-1\",\"tool\":\"demo\"}");
    let o = cmd.output().expect("spawn brain hook");
    assert!(o.status.success(), "{}", out(&o));
    assert_eq!(mock.requests(), 0, "BRAIN_HOOK_EMBED=0 must issue no embedding request");
    let content = read_note(&db, &format!("sessoes/noembed/{today}"));
    assert!(content.contains("id=ne-1"), "capture must not depend on the embed: {}", content);

    let _ = std::fs::remove_dir_all(&xdg);
    let _ = std::fs::remove_file(&db);
}

fn home_env(home: &str) -> Vec<(String, String)> {
    vec![
        ("HOME".to_string(), home.to_string()),
        ("BRAIN_SETUP_NO_SYSTEMCTL".to_string(), "1".to_string()),
    ]
}

fn fresh_home(tag: &str) -> String {
    let h = format!("/tmp/brain-setup-home-{}-{}", std::process::id(), tag);
    let _ = std::fs::remove_dir_all(&h);
    std::fs::create_dir_all(&h).unwrap();
    h
}

#[test]
fn e2e_setup_opencode_and_systemd() {
    let home = fresh_home("os");
    let db = db("setup");
    let mut cmd = Command::new(bin());
    for (k, v) in home_env(&home) {
        cmd.env(k, v);
    }
    let o = cmd.arg("--db").arg(&db).arg("setup").arg("opencode").output().unwrap();
    assert!(o.status.success(), "{}", out(&o));
    let mcp = PathBuf::from(format!("{}/.config/opencode/mcp.json", home));
    assert!(mcp.exists());
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&mcp).unwrap()).unwrap();
    assert_eq!(v["mcp"]["servers"]["brain"]["url"], "http://localhost:8321/mcp");
    assert_eq!(v["mcp"]["servers"]["brain"]["type"], "remote");
    // idempotent re-run without --force keeps file
    let mut cmd = Command::new(bin());
    for (k, v) in home_env(&home) {
        cmd.env(k, v);
    }
    let o = cmd.arg("--db").arg(&db).arg("setup").arg("opencode").output().unwrap();
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("skipped"));

    let mut cmd = Command::new(bin());
    for (k, v) in home_env(&home) {
        cmd.env(k, v);
    }
    let o = cmd.arg("--db").arg(&db).arg("setup").arg("systemd").arg("--mcp-port").arg("8331").output().unwrap();
    assert!(o.status.success(), "{}", out(&o));
    let unit = std::fs::read_to_string(format!("{}/.config/systemd/user/brain-mcp.service", home)).unwrap();
    assert!(unit.contains("serve-mcp --port 8331"));
    let unit = std::fs::read_to_string(format!("{}/.config/systemd/user/brain-viewer.service", home)).unwrap();
    assert!(unit.contains("--port 8322"));
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn e2e_setup_project_and_dry_run() {
    let home = fresh_home("proj");
    let proj = format!("/tmp/brain-setup-proj-{}", std::process::id());
    let _ = std::fs::remove_dir_all(&proj);
    std::fs::create_dir_all(&proj).unwrap();
    let db = db("setup2");
    let mut cmd = Command::new(bin());
    for (k, v) in home_env(&home) {
        cmd.env(k, v);
    }
    let o = cmd.arg("--db").arg(&db).arg("setup").arg("project").arg("--dir").arg(&proj).arg("--brain-dir").arg("/a/brain").output().unwrap();
    assert!(o.status.success(), "{}", out(&o));
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/opencode.json", proj)).unwrap()).unwrap();
    assert!(v["instructions"].as_array().unwrap().len() == 2);
    assert_eq!(v["mcp"]["servers"]["brain"]["url"], "http://localhost:8321/mcp");
    // dry-run creates nothing
    let proj2 = format!("/tmp/brain-setup-proj2-{}", std::process::id());
    let _ = std::fs::remove_dir_all(&proj2);
    std::fs::create_dir_all(&proj2).unwrap();
    let mut cmd = Command::new(bin());
    for (k, v) in home_env(&home) {
        cmd.env(k, v);
    }
    let o = cmd.arg("--db").arg(&db).arg("setup").arg("all").arg("--dry-run").output().unwrap();
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("dry-run"));
    assert!(!PathBuf::from(format!("{}/.config/opencode/mcp.json", home)).exists());
    // unknown target fails
    let mut cmd = Command::new(bin());
    for (k, v) in home_env(&home) {
        cmd.env(k, v);
    }
    let o = cmd.arg("--db").arg(&db).arg("setup").arg("vscode").output().unwrap();
    assert!(!o.status.success());
    cleanup(&db);
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&proj);
    let _ = std::fs::remove_dir_all(&proj2);
}

#[test]
fn e2e_reindex_reports_the_full_breakdown() {
    // A reindex report that only said "done" is how a half-rebuilt index looked
    // healthy for a month. `null` in particular has to be visible: it is the
    // number of chunks the vector stream is still owed.
    let db = db("reindex-report");
    let _ = run(&db, &["store", "regras", "e2e/rr", "## alpha section\n\nbody one\n## beta section\n\nbody two", "--scope", "global"]);
    let o = run(&db, &["reindex", "--all", "--no-embed"]);
    assert!(o.status.success(), "{}", out(&o));
    let text = out(&o);
    assert!(text.contains("REINDEX_DONE"), "{}", text);
    for field in ["preserved=", "rehydrated=", "null=", "diverged="] {
        assert!(text.contains(field), "REINDEX_DONE must report `{field}`: {}", text);
    }
    cleanup(&db);
}

#[test]
fn e2e_store_rejects_an_oversized_note_before_embedding_it() {
    // The CLI is the third write path and it had no size limit: `chunk_text`
    // splits on `## ` with no bound and embedding is serial, so a huge body held
    // the process open proportionally to its chunk count. The rejection is checked
    // before the store is even opened, so the note must not exist afterwards.
    let db = db("oversize");
    let mut body = String::new();
    for i in 0..65 {
        body.push_str(&format!("## section {i}\n"));
    }
    let o = run(&db, &["store", "regras", "e2e/big", &body, "--scope", "global"]);
    assert!(!o.status.success(), "an oversized note must fail: {}", out(&o));
    let text = out(&o);
    assert!(text.contains("INVALID_PARAMS"), "{}", text);
    assert!(text.contains("chunks, limit is"), "the error must name the limit: {}", text);
    // Nothing was written, which is the proof that no embed was attempted.
    let o = run(&db, &["read", "regras", "e2e/big", "--scope", "global"]);
    assert!(!o.status.success(), "a refused note must leave no trace: {}", out(&o));
    // The byte ceiling cannot be reached through argv at all: Linux caps a single
    // argument at MAX_ARG_STRLEN (128 KiB), well under MAX_CONTENT_BYTES (256 KiB).
    // So on the CLI the chunk limit is the operative one, and the byte limit is the
    // network paths' (MCP/REST) guard. Pinned from the reachable side: a 100 KiB
    // body is under the cap and must be accepted.
    let o = run(&db, &["store", "regras", "e2e/bigbytes", &"x".repeat(100_000), "--scope", "global"]);
    assert!(o.status.success(), "100 KiB is under MAX_CONTENT_BYTES and must be accepted: {}", out(&o));
    // A legitimate note of the size the corpus really contains still works.
    let mut ok_body = String::new();
    for i in 0..60 {
        ok_body.push_str(&format!("## section {i}\n\nprose {i}\n"));
    }
    let o = run(&db, &["store", "regras", "e2e/dense", &ok_body, "--scope", "global"]);
    assert!(o.status.success(), "a 60-chunk note is within budget: {}", out(&o));
    assert!(out(&o).contains("chunks=60"), "{}", out(&o));
    cleanup(&db);
}

#[test]
fn e2e_store_reports_rehydrated_and_diverged() {
    let db = db("store-report");
    let o = run(&db, &["store", "regras", "e2e/sr", "## one\n\nalpha\n## two\n\nbeta", "--scope", "global"]);
    assert!(o.status.success(), "{}", out(&o));
    let text = out(&o);
    for field in ["chunks=", "embedded=", "without_embedding=", "preserved=", "rehydrated=", "diverged="] {
        assert!(text.contains(field), "store must report `{field}`: {}", text);
    }
    cleanup(&db);
}

// -------------------------------------------------------------- Y-05 --
//
// The dead-letter message told the operator to run `brain status` to see
// `queue.dead_lettered`. The CLI printed no queue block at all, so the pointer
// led nowhere — and the queue runs in a systemd service whose stderr goes to a
// journal nobody reads, so the note was undiscoverable. Y-05 also had to be
// *honest*: the queue's work list and dead-letter counter are in-memory state of
// the `serve-mcp` process, so no separate CLI process can read them. The CLI
// prints the half that is in the database and names where the rest is.

/// Y-05: `brain status` names the queue.
#[test]
fn cli_status_reports_the_queue_and_where_to_read_the_rest() {
    let db = db("statusqueue");
    let o = run(&db, &["status"]);
    let text = out(&o);
    assert!(o.status.success(), "{text}");
    assert!(text.contains("queue:"), "the CLI must print a queue block: {text}");

    // The cross-process embed lock is the half a separate process can genuinely
    // read, so it has to be there — with the counter *absent* meaning "nobody
    // holds it", not a silent blank.
    assert!(text.contains("embed_lock_holder="), "the lock holder must be reported: {text}");
    assert!(text.contains("embed_lock_age_s="), "the lock age must be reported: {text}");
    assert!(text.contains("embed_lock_expires_in_s="), "the lock countdown must be reported: {text}");

    // And the in-memory-only half is *named as unavailable*, with the two places
    // it can actually be read. Printing a `dead_lettered=0` here would be a lie:
    // this process has no idea what the server's counter says.
    assert!(
        text.contains("pending_len") && text.contains("dead_lettered"),
        "the fields the CLI cannot read must still be named: {text}"
    );
    assert!(
        text.contains("journalctl -u brain-mcp"),
        "the dead letter's real discovery path is the journal, so `status` must say so: {text}"
    );
    assert!(
        text.contains("brain_status"),
        "and the MCP tool, which is the only reader of the live queue: {text}"
    );
    // The footprint that *is* measurable here is still printed, and the block
    // points at it.
    assert!(text.contains("embedding.without_embedding"), "and the measurable footprint: {text}");
    assert!(text.contains("coverage_pct="), "the coverage alarm must stay: {text}");
    cleanup(&db);
}

/// Y-05: when a lock is genuinely held, `status` says a pass is running — the
/// distinction the delegation's "deferred vs idle" case depends on.
#[test]
fn cli_status_reports_a_held_embed_lock() {
    let db = db("statuslock");
    {
        let store = brain_store::Store::open(&db).unwrap();
        assert!(store.try_acquire_embed_lock("brain:queue:9999", 900).unwrap());
    }
    let o = run(&db, &["status"]);
    let text = out(&o);
    assert!(o.status.success(), "{text}");
    assert!(
        text.contains("brain:queue:9999"),
        "a held lock must be named, so a deferred queue is distinguishable from an idle one: {text}"
    );
    assert!(text.contains("in progress"), "and said to be in progress: {text}");
    cleanup(&db);
    let _ = std::fs::remove_file(format!("{db}-wal"));
    let _ = std::fs::remove_file(format!("{db}-shm"));
}

/// Y-05: the dead-letter message names a discovery path that exists.
///
/// The previous text said "`brain status` reports it as `queue.dead_lettered`" —
/// and the CLI printed no queue block, so the instruction pointed at nothing. The
/// message is only observable in a real process's stderr, so this runs the real
/// `serve-mcp` binary with Ollama unreachable and the failure cap at 1, forces a
/// real `brain_store` through the MCP protocol, and reads what the queue said.
///
/// What is asserted is not the prose but the *reachability* of every pointer: the
/// journal command, the MCP tool, and the coverage number the CLI really does
/// print.
#[test]
fn the_dead_letter_message_points_at_reachable_discovery_paths() {
    let db = db("deadletter");
    let port = free_port();
    // Ollama unreachable and a cap of 1, so the first failed pass is terminal.
    let mut server = Server::spawn_with_env(
        "deadletter",
        &db,
        port,
        "serve-mcp",
        &[("BRAIN_OLLAMA_URL", "http://127.0.0.1:1"), ("BRAIN_EMBED_MAX_FAILURES", "1")],
    );
    wait_until_listening(&mut server, std::time::Duration::from_secs(30)).unwrap_or_else(|e| panic!("{e}"));

    let helper = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/helpers/mcp_store.py");
    let o = Command::new("python3")
        .arg(&helper)
        .arg(format!("http://127.0.0.1:{port}"))
        .arg("regras")
        .arg("e2e/deadletter")
        .arg("## dead letter probe\n\nbody token")
        .arg("global")
        .output()
        .unwrap();
    let call = String::from_utf8_lossy(&o.stdout).into_owned() + String::from_utf8_lossy(&o.stderr).as_ref();
    assert!(o.status.success(), "the store call must succeed — queueing is not an error: {call}");
    assert!(call.contains("\"call\""), "{call}");

    // The drain runs in the background; give it room to fail and give up.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut err = server.stderr();
    while !err.contains("GAVE UP") && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
        err = server.stderr();
    }
    assert!(err.contains("GAVE UP"), "the note must have been dead-lettered with Ollama down. stderr:\n{err}");

    // Every pointer in the message has to lead somewhere that exists.
    assert!(
        err.contains("journalctl -u brain-mcp"),
        "the queue's stderr goes to a journal nobody reads by default, so the message must cite it: {err}"
    );
    assert!(
        err.contains("brain_status"),
        "and the MCP tool, which is the only reader of the live queue's counters: {err}"
    );
    assert!(
        err.contains("embedding.without_embedding"),
        "and the field the CLI really does print, so the instruction is not circular: {err}"
    );
    assert!(
        !err.contains("`brain status` reports it as `queue.dead_lettered`"),
        "the old wording must be gone: it sent the operator to a command that cannot show the field: {err}"
    );
    // And the note really is owed a vector, so the message is not crying wolf.
    let st = run(&db, &["status"]);
    let status = out(&st);
    assert!(
        status.contains("without_embedding=") && !status.contains("without_embedding=0 "),
        "the dead-lettered note's chunk must show as owed in coverage: {status}"
    );
    drop(server);
    cleanup(&db);
}

// ----------------------------------------------------------------- TD-010 --
//
// A reader that goes away is not a failure.
//
// `brain recent | head -1` closes the pipe while `brain` is still writing. The write
// then fails with `EPIPE`, and the question this pins is what the process does with
// that. `println!` panics on a failed write, so the answer was exit 101 and
// `failed printing to stdout: Broken pipe` on stderr — noise from a pipeline that
// worked, and a non-zero status for every script that tests it. Closing the read end
// early is the ordinary Unix contract (`| head`, `| grep -m`, `| less` all do it), so
// the answer is exit 0 and silence, and only for `BrokenPipe`.
//
// Two independent ways the pipe breaks, because either one alone leaves a gap:
//
// 1. the read end is closed before the child has finished starting up, so a write meets
//    a pipe with no reader at all — this is what carries `search`, whose single result
//    is far smaller than the pipe buffer;
// 2. the note is larger than the 64 KiB pipe buffer, so the tail of `recent`'s single
//    write overflows even if the child *did* win the startup race.
//
// The fix's discrimination is established by mutation rather than asserted here: with
// `outln!` reverted to `println!`, this test reports exit 101 and the panic text.

/// Comfortably over the 64 KiB Linux pipe buffer, and well under
/// `brain_core::MAX_CONTENT_BYTES` (256 KiB), so one `store` is one chunk and one embed.
const BIG_NOTE_BYTES: usize = 80 * 1024;

/// The dead Ollama the rest of this suite uses, so this test never waits on — or is
/// perturbed by — a real model. A refused connection also keeps the fixture cheap: the
/// chunk is stored `NULL`, which is all `recent` and the FTS fallback of `search` read.
const DEAD_OLLAMA: &str = "http://127.0.0.1:1";

/// Run `brain` with a stdout pipe whose read end is closed the instant the child exists,
/// and return its (exit code, stderr).
fn run_with_the_reader_gone(db: &str, args: &[&str]) -> (Option<i32>, String) {
    use std::process::Stdio;
    let mut child = Command::new(bin())
        .arg("--db")
        .arg(db)
        .args(args)
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn brain");
    // The read end of the child's stdout. Dropping it here — while the child is still
    // exec'ing and opening the database — is what makes the write fail, and it does so
    // for output of any size: a pipe with no reader is not a pipe with room in it.
    drop(child.stdout.take());
    let done = child.wait_with_output().expect("wait for brain");
    (done.status.code(), String::from_utf8_lossy(&done.stderr).into_owned())
}

#[test]
fn a_closed_reader_is_not_reported_as_a_failure() {
    let db = db("closedreader");
    let body = "a".repeat(BIG_NOTE_BYTES);
    let mut store_cmd = Command::new(bin());
    let stored = store_cmd
        .arg("--db")
        .arg(&db)
        .args(["store", "sessoes", "td010/big", &format!("## Big\n{body}")])
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .output()
        .expect("spawn brain store");
    assert!(stored.status.success(), "the fixture note must be stored: {}", out(&stored));

    // `recent` and `search --explain` are the two commands whose output is routinely
    // larger than a pipe buffer, which is why they are the two that panic.
    for args in [vec!["recent", "--top-k", "40"], vec!["search", "sessao", "--explain", "--top-k", "40"]] {
        let (code, stderr) = run_with_the_reader_gone(&db, &args);
        assert_eq!(
            code,
            Some(0),
            "`brain {}` lost its reader, which is not a failure: exit {code:?}, stderr: {stderr}",
            args.join(" ")
        );
        assert!(
            !stderr.contains("Broken pipe") && !stderr.contains("panicked"),
            "a closed stdout is not something to report to the operator: {}",
            stderr
        );
    }
    cleanup(&db);
}
