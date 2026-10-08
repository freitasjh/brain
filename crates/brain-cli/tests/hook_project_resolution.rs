//! `brain hook` without `--project` — the cascade (RF-02, RF-03, T2.x, T3.x).
//!
//! **The environment is a per-test sandbox, not the developer's.** Every test gets
//! its own database, its own `BRAIN_DIR` and its own working directory, because the
//! cascade keys on the *absolute path* of the cwd and on `BRAIN_DIR`: a test that
//! inherited either would read the operator's real config.json and answer with the
//! operator's real project.
//!
//! **`BRAIN_OLLAMA_URL` is pointed at a dead port, never removed.** Removing it does
//! not disable embedding — it selects the real default of `brain-embed:23`, shared
//! by every concurrent test binary, which is how a suite of hooks turns into a suite
//! of calls to whatever model the machine happens to be running.
//!
//! `BRAIN_HOOK_EMBED=0` for the same reason: the property under test is *which
//! project the event landed in*, and an embed in the middle would make a failure
//! ambiguous between "resolved the wrong project" and "the model was cold".

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DEAD_OLLAMA: &str = "http://127.0.0.1:1";

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_brain"))
}

/// The `sessoes/<project>/<date>` path the hook reported, read out of its own output.
///
/// **This deliberately has no clock of its own.** It used to shell out to `date`, in a
/// different process from the binary under test: a hook starting at 23:59:59.999 UTC
/// would write today's note while the test read tomorrow's, and the failure would
/// point at the cascade rather than at a date boundary. The hook already prints the
/// path it resolved, so the test reads the answer from the thing it is testing.
fn note_path_of(o: &std::process::Output) -> String {
    stdout(o)
        .lines()
        .find_map(|l| l.strip_prefix("hook ok "))
        .and_then(|l| l.split_whitespace().find_map(|t| t.strip_prefix("note=")))
        .unwrap_or_else(|| panic!("no `note=` in the `hook ok` line:\n{}", out(o)))
        .to_string()
}


/// One test's world: a database, a `BRAIN_DIR`, and a working directory.
struct World {
    root: PathBuf,
}

impl World {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("brain-hookres-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&root);
        for sub in ["brain_dir", "work"] {
            std::fs::create_dir_all(root.join(sub)).expect("sandbox");
        }
        Self { root }
    }

    fn db(&self) -> String {
        self.root.join("brain.db").to_string_lossy().into_owned()
    }

    fn brain_dir(&self) -> PathBuf {
        self.root.join("brain_dir")
    }

    /// A working directory whose **basename is exactly `name`** — step 3 reads the
    /// basename, so a sandbox directory that merely contained the name would test
    /// nothing.
    /// The key the binary uses for `workdir` in the config: the **canonical** path.
    ///
    /// The binary canonicalises (symlinks resolved) before it writes a key, so a test
    /// that indexes with the raw path agrees only while the temp dir has no symlinks
    /// in it. On macOS `/var` is a symlink to `/private/var`, so `/tmp` aliases and
    /// five of these tests fail on that platform while passing on Linux — which is
    /// worse than a plain failure, because the bug is invisible where you run.
    fn key(&self, workdir: &Path) -> String {
        std::fs::canonicalize(workdir)
            .unwrap_or_else(|e| panic!("canonicalizing {}: {e}", workdir.display()))
            .to_string_lossy()
            .into_owned()
    }

    fn work(&self, name: &str) -> PathBuf {
        let d = self.root.join("work").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("workdir");
        d
    }

    fn xdg(&self) -> PathBuf {
        let d = self.root.join("xdg");
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// Run `brain hook` inside `workdir`.
    fn hook(&self, workdir: &PathBuf, args: &[&str]) -> std::process::Output {
        let mut cmd = Command::new(bin());
        cmd.current_dir(workdir)
            .arg("--db")
            .arg(self.db())
            .arg("hook")
            .arg("--event")
            .arg("session-start")
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            .env("BRAIN_HOOK_EMBED", "0")
            .env("BRAIN_DIR", self.brain_dir())
            .env("XDG_RUNTIME_DIR", self.xdg())
            // Not a terminal, and not inheriting one: the cascade must not be able to
            // block on a question nobody is there to answer.
            .stdin(Stdio::null());
        for a in args {
            cmd.arg(a);
        }
        cmd.output().expect("spawn brain hook")
    }

    /// Write a `config.json` the way an operator's file looks.
    fn write_config(&self, entries: serde_json::Value) {
        std::fs::write(
            self.brain_dir().join("config.json"),
            serde_json::to_string_pretty(&serde_json::json!({ "brain_projects": entries })).unwrap(),
        )
        .expect("write config");
    }

    fn config(&self) -> serde_json::Value {
        let raw = std::fs::read_to_string(self.brain_dir().join("config.json"))
            .expect("config.json must exist");
        serde_json::from_str(&raw).expect("config.json must be valid")
    }

    /// The content of the session note the hook reported, read back through the CLI.
    fn session_note(&self, note_path: &str) -> String {
        let rel = note_path.strip_prefix("sessoes/").unwrap_or(note_path);
        let o = Command::new(bin())
            .arg("--db")
            .arg(self.db())
            .arg("read")
            .arg("sessoes")
            .arg(rel)
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            .output()
            .expect("spawn brain read");
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    /// Every note the database currently holds, as JSON from `brain recent`.
    ///
    /// This was a call to `brain list`, which **does not exist** — so it returned an
    /// error for every test, and the four assertions built on it were passing
    /// vacuously, including the one that proves a disabled directory writes nothing.
    /// A helper that cannot fail is worse than no helper: it converts the test that
    /// matters most into the test that proves least.
    fn notes(&self) -> String {
        let o = Command::new(bin())
            .arg("--db")
            .arg(self.db())
            .arg("recent")
            .arg("--top-k")
            .arg("50")
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            .output()
            .expect("spawn brain recent");
        assert!(
            o.status.success(),
            "the note inventory must be readable, or every assertion on it is vacuous: {}",
            out(&o)
        );
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        let json: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("recent must print JSON ({e}): {text}"));
        let notes = json["recent"].as_array().unwrap_or_else(|| panic!("no `recent` array: {text}"));
        // `recent` prints positional arrays, not objects: `[path, layer, project, content]`.
        // Reading `n["path"]` here yields null and every test passes on an empty
        // string, so the shape is asserted rather than assumed.
        notes
            .iter()
            .map(|n| {
                n.as_array()
                    .and_then(|a| a.first())
                    .and_then(|p| p.as_str())
                    .unwrap_or_else(|| panic!("a `recent` entry has no leading path string: {n}"))
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The note count, so "nothing was written" is assertable directly rather than
    /// through a substring that a wrong project name would also satisfy.
    ///
    /// `brain status` prints `key=value` lines, not JSON, so the field is read out of
    /// the first line.
    fn note_count(&self) -> usize {
        let o = Command::new(bin())
            .arg("--db")
            .arg(self.db())
            .arg("status")
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            .output()
            .expect("spawn brain status");
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        assert!(o.status.success(), "status must succeed: {text}");
        text.lines()
            .next()
            .and_then(|l| l.split_whitespace().find_map(|kv| kv.strip_prefix("notes=")))
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or_else(|| panic!("no `notes=` in the first line of status: {text}"))
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn out(o: &std::process::Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn stdout(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// The `hook resolve …` line, for the runs that resolve a project and go on to write.
fn origin_line(o: &std::process::Output) -> String {
    stdout(o)
        .lines()
        .find(|l| l.starts_with("hook resolve "))
        .unwrap_or_else(|| panic!("no `hook resolve` line in:\n{}", out(o)))
        .to_string()
}

/// The origin value, wherever it is printed.
///
/// Two forms, because a run either resolves and writes or skips:
/// `hook resolve project=… origin=…` and `hook skipped event=… origin=… dir=…`.
/// Reading it as a token rather than matching a prefix is what lets one assertion
/// talk about the *decision* without also pinning the wording around it.
fn origin_of(o: &std::process::Output) -> String {
    let text = stdout(o);
    for line in text.lines() {
        if line.starts_with("hook resolve ") || line.starts_with("hook skipped ") {
            return line
                .split_whitespace()
                .find_map(|t| t.strip_prefix("origin="))
                .unwrap_or_else(|| panic!("no `origin=` in: {line}"))
                .to_string();
        }
    }
    panic!("neither a `hook resolve` nor a `hook skipped` line in:\n{text}")
}

/// Register a project so the cascade has something to resolve *to*.
fn register(w: &World, name: &str) {
    let o = Command::new(bin())
        .arg("--db")
        .arg(w.db())
        .arg("project")
        .arg("create")
        .arg(name)
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .output()
        .expect("spawn brain project create");
    assert!(o.status.success(), "registering {name} must work: {}", out(&o));
}

// ---------------------------------------------------------------------------
// T3.1 — an explicit --project is unchanged (R-06)
// ---------------------------------------------------------------------------

/// The whole of R-06: with `--project`, the `hook ok` line is byte-for-byte the
/// format it has always had, and the event lands in that project's note.
///
/// The format string is asserted as a whole rather than by substring, because a
/// substring check would pass on a line that had quietly grown a field — which is
/// exactly the change R-06 exists to forbid.
#[test]
fn an_explicit_project_produces_exactly_the_output_it_always_did() {
    let w = World::new("explicit");
    let wd = w.work("anything");
    let o = w.hook(&wd, &["--project", "hive", "--payload", r#"{"id":"exp-1"}"#]);
    assert!(o.status.success(), "{}", out(&o));

    let so = stdout(&o);
    let ok = so
        .lines()
        .find(|l| l.starts_with("hook ok "))
        .unwrap_or_else(|| panic!("no `hook ok` line in:\n{}", stdout(&o)));
    let expected_prefix = "hook ok event=session-start project=hive note=sessoes/hive/";
    assert!(
        ok.starts_with(expected_prefix),
        "the `hook ok` line changed shape.\n  expected prefix: {expected_prefix}<date> spool=\n  got: {ok}"
    );
    assert_eq!(note_path_of(&o), ok.split("note=").nth(1).unwrap().split(' ').next().unwrap());

    // The event really is in that project's note, so the assertion above is not
    // satisfied by a line that merely looks right. The date comes from the hook's own
    // output, not from a second clock.
    let note = w.session_note(&note_path_of(&o));
    assert!(note.contains("exp-1"), "the event must be recorded under `hive`:\n{note}");
    assert_eq!(
        w.notes(),
        note_path_of(&o),
        "the event must be the only note, and under the explicit project — the cwd \
         name must not leak in"
    );
}

/// R-06's other half: an explicit `--project` does not consult the cascade, so a
/// config that would resolve *differently* has no effect. Both are set up to
/// disagree, and the explicit one wins.
#[test]
fn an_explicit_project_ignores_a_config_that_disagrees() {
    let w = World::new("explicitwins");
    let wd = w.work("hive");
    w.write_config(serde_json::json!({
        w.work("hive").to_string_lossy(): { "projeto": "from-config" }
    }));
    register(&w, "hive");
    register(&w, "from-config");

    let o = w.hook(&wd, &["--project", "hive", "--payload", r#"{"id":"exp-2"}"#]);
    assert!(o.status.success(), "{}", out(&o));
    assert_eq!(origin_line(&o), "hook resolve project=hive origin=explicit");
    assert!(w.session_note(&note_path_of(&o)).contains("exp-2"));
    assert_eq!(
        w.notes(),
        note_path_of(&o),
        "the config must not have been consulted; the only note is the explicit one"
    );
}

/// The origin is on its own line precisely so the `hook ok` line above can stay
/// identical. If they ever merge, this fails.
#[test]
fn the_origin_is_reported_on_its_own_line() {
    let w = World::new("originline");
    let wd = w.work("anything");
    let o = w.hook(&wd, &["--project", "hive", "--payload", r#"{"id":"exp-3"}"#]);
    let so = stdout(&o);
    let lines: Vec<&str> = so.lines().collect();
    let resolve_at = lines.iter().position(|l| l.starts_with("hook resolve ")).expect("origin line");
    let ok_at = lines.iter().position(|l| l.starts_with("hook ok ")).expect("ok line");
    assert!(
        resolve_at < ok_at,
        "the origin is printed first so the `hook ok` line stays last and unchanged:\n{}",
        stdout(&o)
    );
    assert!(
        !lines[ok_at].contains("origin="),
        "the `hook ok` line must not have grown an origin field: {}",
        lines[ok_at]
    );
}

// ---------------------------------------------------------------------------
// T3.2 / T2.1 — the cascade, step by step
// ---------------------------------------------------------------------------

/// T2.1 + T3.2 — no `--project`, and the config answers.
#[test]
fn without_project_the_config_resolves_and_the_origin_is_reported() {
    let w = World::new("cfg");
    let wd = w.work("anything");
    w.write_config(serde_json::json!({ w.key(&wd): { "projeto": "hive" } }));

    let o = w.hook(&wd, &["--payload", r#"{"id":"cfg-1"}"#]);
    assert!(o.status.success(), "{}", out(&o));
    assert_eq!(origin_line(&o), "hook resolve project=hive origin=config");
    assert!(
        w.session_note(&note_path_of(&o)).contains("cfg-1"),
        "the event must land in the configured project"
    );
}

/// T2.2 — no config, and the `origin` remote names a registered project.
#[test]
fn without_project_a_matching_remote_resolves() {
    let w = World::new("git");
    let wd = w.work("some-checkout");
    register(&w, "hive");
    // A real repository with a real `origin`, so `git remote get-url` answers exactly
    // as it would in the field.
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&wd)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
    };
    let ok = git(&["init", "-q"]).map(|o| o.status.success()).unwrap_or(false)
        && git(&["remote", "add", "origin", "git@github.com:acme/hive.git"])
            .map(|o| o.status.success())
            .unwrap_or(false);
    if !ok {
        eprintln!("skipping: git is unavailable here, so step 2 cannot be exercised");
        return;
    }

    let o = w.hook(&wd, &["--payload", r#"{"id":"git-1"}"#]);
    assert!(o.status.success(), "{}", out(&o));
    assert_eq!(origin_line(&o), "hook resolve project=hive origin=git-remote");
    assert!(w.session_note(&note_path_of(&o)).contains("git-1"));
}

/// ADR-06's ordering claim through the real binary: the remote beats the directory
/// name, and here they name **different** projects.
///
/// The mutation this was written for: swapping steps 2 and 3 passed the whole suite,
/// because every other test lets at most one of the two answer, and the one test that
/// set both up pointed them at the *same* project. A cascade-ordering test has to make
/// the two steps disagree.
#[test]
fn the_remote_wins_over_a_directory_naming_a_different_project() {
    let w = World::new("order");
    let wd = w.work("atlas-ecm");
    register(&w, "atlas-ecm");
    register(&w, "mobile-erp");
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&wd)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
    };
    let ok = git(&["init", "-q"]).map(|o| o.status.success()).unwrap_or(false)
        && git(&["remote", "add", "origin", "git@github.com:acme/mobile-erp.git"])
            .map(|o| o.status.success())
            .unwrap_or(false);
    if !ok {
        eprintln!("skipping: git is unavailable here, so step 2 cannot be exercised");
        return;
    }

    let o = w.hook(&wd, &["--payload", r#"{"id":"ord-1"}"#]);
    assert!(o.status.success(), "{}", out(&o));
    assert_eq!(
        origin_line(&o),
        "hook resolve project=mobile-erp origin=git-remote",
        "the remote must win over the directory name, which says atlas-ecm"
    );
    assert!(
        w.notes().contains("mobile-erp") && !w.notes().contains("atlas-ecm"),
        "the event must land in the remote's project, not the directory's:\n{}",
        w.notes()
    );
}

/// T2.3 — the directory basename is the project name. Exact, and no `git` present.
#[test]
fn without_project_an_exact_directory_name_resolves() {
    let w = World::new("dirname");
    let wd = w.work("hive");
    register(&w, "hive");

    let o = w.hook(&wd, &["--payload", r#"{"id":"dn-1"}"#]);
    assert!(o.status.success(), "{}", out(&o));
    assert_eq!(origin_line(&o), "hook resolve project=hive origin=dir-name");
}

/// T2.3's negative half, and the reason it exists: on one disk `atlas-ecm`,
/// `atlasos` and `atlas-admin` coexist, so a prefix match would put `atlas-admin`'s
/// session into `atlas-ecm`'s notes and report success.
#[test]
fn a_directory_that_only_shares_a_prefix_resolves_to_itself_not_the_neighbour() {
    let w = World::new("prefix");
    register(&w, "atlas-ecm");
    let wd = w.work("atlas-admin");

    let o = w.hook(&wd, &["--payload", r#"{"id":"pf-1"}"#]);
    assert!(o.status.success(), "{}", out(&o));
    assert_ne!(
        origin_of(&o),
        "dir-name",
        "`atlas-admin` must not resolve to atlas-ecm — a prefix match is a wrong \
         project with no error. The origin must be `unresolved`, not a match.\n{}",
        stdout(&o)
    );
    assert!(
        !w.notes().contains("atlas-ecm"),
        "no note may land in atlas-ecm for a directory called atlas-admin:\n{}",
        w.notes()
    );
}

// ---------------------------------------------------------------------------
// T2.4 / T2.5 — asking, refusing, and not asking again
// ---------------------------------------------------------------------------

/// T2.5 + the "mark as not used" fix — a recorded refusal **writes nothing**.
///
/// The regression this was rewritten for, and it is worth keeping the old behaviour
/// on the record: with a refusal in the config the hook printed
/// `hook ok … note=sessoes/<dirname>/<date>` — declining the question had silently
/// created a session note in a project the operator never chose. `desabilitado`
/// skipped; `recusado` did not, so the two answers to the same question had opposite
/// meanings.
///
/// The count is the assertion, not a substring: "no note for project X" would also
/// hold if the event had been filed somewhere else entirely.
#[test]
fn a_recorded_refusal_writes_nothing_and_does_not_ask_again() {
    let w = World::new("refused");
    let wd = w.work("unmapped");
    register(&w, "hive");
    w.write_config(serde_json::json!({ w.key(&wd): { "projeto": null, "motivo": "recusado" } }));
    assert_eq!(
        w.note_count(),
        0,
        "a clean database is the precondition: a refusal must be measured against zero"
    );

    let o = w.hook(&wd, &["--payload", r#"{"id":"ref-1"}"#]);
    assert!(o.status.success(), "a refusal must exit 0 so the IDE is not broken:\n{}", out(&o));
    assert_eq!(origin_of(&o), "recusado", "a recorded refusal must not be re-asked");
    assert!(
        stdout(&o).contains("hook skipped"),
        "a refusal is a skip, not a write:\n{}", stdout(&o)
    );
    // The *reason* is asserted, not just the skip. Two layers can produce "skipped
    // and wrote nothing" — the origin's own reason, and the defensive branch for a
    // resolution with no project — and the note count alone cannot tell them apart.
    // Without this, a mutation that makes `recusado` stop suppressing still passes
    // here, silently, because the second layer catches it.
    assert!(
        out(&o).contains("was declined for this directory"),
        "a refusal must skip for its own reason:\n{}", out(&o)
    );
    assert!(
        !out(&o).contains("no project and nobody could be asked"),
        "a refusal must not be reported as the unanswerable case — that is a different \
         answer with a different consequence (it is asked again):\n{}", out(&o)
    );
    assert_eq!(
        w.note_count(),
        0,
        "a refused directory must write NO note, but the database holds:\n{}",
        w.notes()
    );
    assert!(
        !stdout(&o).contains("Brain context"),
        "a refused directory must not search either:\n{}", stdout(&o)
    );
}

/// The refusal must survive the run untouched — a skip is not a rewrite, and a hook
/// that normalised the entry on every event would be a second, silent writer.
#[test]
fn a_recorded_refusal_is_left_exactly_as_it_was() {
    let w = World::new("refusedkeep");
    let wd = w.work("unmapped");
    w.write_config(serde_json::json!({ w.key(&wd): { "projeto": null, "motivo": "recusado" } }));
    let before = std::fs::read_to_string(w.brain_dir().join("config.json")).expect("config");

    let o = w.hook(&wd, &["--payload", r#"{"id":"rk-1"}"#]);
    assert!(o.status.success(), "{}", out(&o));
    assert_eq!(
        std::fs::read_to_string(w.brain_dir().join("config.json")).expect("config"),
        before,
        "the operator's recorded refusal must not be rewritten by a run that skipped it"
    );
}

/// The counterpart, and the distinction the whole change turns on: **nobody could be
/// asked** is not **nobody answered**.
///
/// It writes no note, like a refusal. It records *nothing*, unlike a refusal. If
/// `unresolved` were laundered into `recusado`, the first non-interactive run — every
/// CI job, every IDE invocation on a fresh checkout — would permanently silence a
/// directory whose operator has still never been asked anything, and the only way out
/// would be hand-editing the config.
#[test]
fn an_unanswerable_question_writes_nothing_and_records_no_refusal() {
    let w = World::new("cannotask");
    let wd = w.work("brand-new");
    register(&w, "hive");

    let o = w.hook(&wd, &["--payload", r#"{"id":"ca-1"}"#]);
    assert!(o.status.success(), "{}", out(&o));
    assert_eq!(origin_of(&o), "unresolved");
    assert!(stdout(&o).contains("hook skipped"), "an unanswerable directory writes nothing:\n{}", stdout(&o));
    assert!(
        out(&o).contains("no project and nobody could be asked"),
        "an unanswerable directory skips for its own reason:\n{}", out(&o)
    );
    assert!(
        !out(&o).contains("was declined for this directory"),
        "`unresolved` must never be reported as a refusal — that is the confusion this \
         distinction exists to prevent:\n{}", out(&o)
    );
    assert_eq!(
        w.note_count(),
        0,
        "an unanswerable directory must write no note either, or every CI run leaves one"
    );

    // The config is allowed not to exist, because `unresolved` records nothing. What
    // must not happen is a refusal appearing.
    let path = w.brain_dir().join("config.json");
    if path.exists() {
        let config = w.config();
        let key = w.key(&wd);
        assert!(
            config["brain_projects"].get(key.as_str()).is_none(),
            "a run with nobody to ask must not record anything for this directory — \
             the operator has still never been asked. Wrote: {config}"
        );
    }
}

/// …and the other half: because nothing was recorded, the **question is reached
/// again** on the next run.
///
/// A tty cannot be provided here, so what is asserted is the observable that does not
/// depend on one: the prompt is *entered*. The stderr line the prompt prints when it
/// finds no terminal only appears if `Prompt::ask` was called, so seeing it on run 2
/// proves the cascade got to step 4 twice instead of short-circuiting at step 1 on a
/// refusal the first run left behind. That is the exact thing a
/// `unresolved`-becomes-`recusado` regression would break.
#[test]
fn a_directory_left_unanswerable_is_asked_again_on_the_next_run() {
    let w = World::new("cannotask2");
    let wd = w.work("brand-new-2");
    register(&w, "hive");
    register(&w, "mobile-erp");

    let runs: Vec<std::process::Output> = (0..2)
        .map(|i| w.hook(&wd, &["--payload", &format!(r#"{{"id":"cu-{i}"}}"#)]))
        .collect();

    for (i, o) in runs.iter().enumerate() {
        assert!(o.status.success(), "run {i}: {}", out(o));
        assert_eq!(origin_of(o), "unresolved", "run {i} must resolve to unresolved, not a refusal");
        assert!(
            out(o).contains("nobody to ask"),
            "run {i} must reach the prompt and find no terminal — a run that short-circuited \
             at step 1 never gets here, which is exactly what a persisted refusal would \
             do:\n{}",
            out(o)
        );
        assert_eq!(w.note_count(), 0, "run {i} must not write a note");
    }

    // And nothing was ever recorded, so neither run can have become permanent.
    let path = w.brain_dir().join("config.json");
    if path.exists() {
        let config = w.config();
        let key = w.key(&wd);
        assert!(
            config["brain_projects"].get(key.as_str()).is_none(),
            "no run may record anything for a directory nobody was asked about: {config}"
        );
    }
}

/// An IDE does not give the hook a terminal either, so the common case is a pipe or
/// nothing at all. The hook must not block on it — a hook that stalls the IDE is worse
/// than one that skips a step, and `read_line` on a pipe is exactly that.
///
/// The assertion is a wall-clock bound, because "it did not hang" is otherwise not
/// observable from the outside except by the suite's own timeout.
#[test]
fn a_piped_stdin_does_not_stall_the_hook() {
    let w = World::new("pipe");
    let wd = w.work("brand-new-3");
    register(&w, "hive");

    let started = std::time::Instant::now();
    let child = Command::new(bin())
        .current_dir(&wd)
        .arg("--db")
        .arg(w.db())
        .arg("hook")
        .arg("--event")
        .arg("session-start")
        .arg("--payload")
        .arg(r#"{"id":"pipe-1"}"#)
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_HOOK_EMBED", "0")
        .env("BRAIN_DIR", w.brain_dir())
        .env("XDG_RUNTIME_DIR", w.xdg())
        // A pipe that is never written to and never closed: the worst case for a naive
        // `read_line`, since it blocks until the write end goes away.
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn brain hook");
    let done = child.wait_with_output().expect("wait");
    let elapsed = started.elapsed();

    assert!(done.status.success(), "{}", out(&done));
    assert_eq!(origin_of(&done), "unresolved");
    assert!(
        elapsed < std::time::Duration::from_secs(20),
        "the hook took {elapsed:?} with a pipe on stdin; it must not wait for a terminal"
    );
    assert_eq!(w.note_count(), 0, "nothing was written");
}

/// T3.3 — `desabilitado` stops the hook, and writes nothing at all.
#[test]
fn a_disabled_directory_writes_nothing_and_reports_its_origin() {
    let w = World::new("disabled");
    let wd = w.work("hive");
    register(&w, "hive");
    w.write_config(serde_json::json!({ w.key(&wd): { "projeto": null, "motivo": null, "desabilitado": true } }));

    let o = w.hook(&wd, &["--payload", r#"{"id":"dis-1"}"#]);
    assert!(o.status.success(), "an opt-out must exit 0 so the IDE is not broken:\n{}", out(&o));
    assert!(
        stdout(&o).contains("hook skipped") && stdout(&o).contains("origin=disabled"),
        "the skip and its origin must be visible:\n{}", out(&o)
    );
    assert!(
        out(&o).contains("is marked desabilitado in config.json"),
        "an opt-out must say so, in its own words:\n{}", out(&o)
    );
    assert!(
        !out(&o).contains("was declined") && !out(&o).contains("nobody could be asked"),
        "the three negative origins must stay distinguishable in the message:\n{}", out(&o)
    );
    assert_eq!(
        w.note_count(),
        0,
        "a disabled directory must write no note at all, but the database holds:\n{}",
        w.notes()
    );
    assert!(
        !stdout(&o).contains("Brain context"),
        "a disabled directory must not search either:\n{}", stdout(&o)
    );
}

// ---------------------------------------------------------------------------
// config.json behaviour at the binary level
// ---------------------------------------------------------------------------

/// T1.2 through the real binary: a malformed config is left alone, the hook still
/// works, and the file is byte-identical afterwards.
#[test]
fn a_malformed_config_is_reported_and_never_clobbered() {
    let w = World::new("badcfg");
    let wd = w.work("hive");
    register(&w, "hive");
    let corrupt = "{ nope, not json";
    std::fs::write(w.brain_dir().join("config.json"), corrupt).expect("write corrupt");

    let o = w.hook(&wd, &["--payload", r#"{"id":"bad-1"}"#]);
    assert!(o.status.success(), "a broken config must not fail the hook:\n{}", out(&o));
    assert!(
        out(&o).contains("config.json"),
        "the operator must be told the config is being ignored:\n{}", out(&o)
    );
    // The cascade still worked — the directory name matched.
    assert_eq!(origin_line(&o), "hook resolve project=hive origin=dir-name");
    assert_eq!(
        std::fs::read_to_string(w.brain_dir().join("config.json")).expect("reread"),
        corrupt,
        "the operator's file must be byte-identical after the run"
    );
}

/// `BRAIN_DIR` is what the cascade reads, and a missing one is created rather than
/// being an error — the first hook run in a fresh environment would otherwise fail.
#[test]
fn a_missing_brain_dir_is_created_rather_than_failing_the_hook() {
    let w = World::new("nobraindir");
    let wd = w.work("hive");
    register(&w, "hive");
    let nested = w.root.join("fresh").join("nested");
    assert!(!nested.exists());

    let mut cmd = Command::new(bin());
    let o = cmd
        .current_dir(&wd)
        .arg("--db")
        .arg(w.db())
        .arg("hook")
        .arg("--event")
        .arg("session-start")
        .arg("--payload")
        .arg(r#"{"id":"nb-1"}"#)
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_HOOK_EMBED", "0")
        .env("BRAIN_DIR", &nested)
        .env("XDG_RUNTIME_DIR", w.xdg())
        .stdin(Stdio::null())
        .output()
        .expect("spawn brain hook");

    assert!(o.status.success(), "a missing BRAIN_DIR must not fail the hook:\n{}", out(&o));
    assert!(nested.is_dir(), "{} must have been created", nested.display());
    assert_eq!(origin_line(&o), "hook resolve project=hive origin=dir-name");
}

/// The project name becomes `sessoes/<name>/<date>`, so the property that matters is
/// **containment**: a name must never produce a note outside `sessoes/`, because
/// `brain_export` writes relative to its note's path and would follow it out.
///
/// `sanitize_relative_path` refuses absolute paths and any `..` component, and allows
/// nesting (`a/b` is a legal relative path). An earlier version of this test asserted
/// "one component, no separators" — a stronger rule than the guard has, and one the
/// hook's own comment claimed. Both are corrected to the real guarantee here, and the
/// comment in `main.rs` was corrected with them.
#[test]
fn a_project_name_can_never_produce_a_note_outside_sessoes() {
    let w = World::new("escape");

    // The two shapes that would climb out, and the absolute one. **Degraded, not
    // refused** (RF-07): a hook that exits non-zero breaks the IDE that called it, and
    // the bad name is the operator's own earlier input rather than a fault of the
    // moment. Exit 0, a warning naming the fix, and nothing written.
    for hostile in ["../escape", "..", "sub/../../out", "/etc/passwd"] {
        let wd = w.work("hive");
        let o = w.hook(&wd, &["--project", hostile, "--payload", r#"{"id":"esc-1"}"#]);
        assert!(
            o.status.success(),
            "`--project {hostile}` must degrade to exit 0, not break the IDE:\n{}",
            out(&o)
        );
        assert!(
            out(&o).contains("nothing was written"),
            "and must say what it did instead:\n{}",
            out(&o)
        );
        assert_eq!(w.note_count(), 0, "`--project {hostile}` must not have written a note");
    }

    // Allowed but contained: a nested relative path stays under `sessoes/`.
    let wd = w.work("hive");
    let o = w.hook(&wd, &["--project", "a/b", "--payload", r#"{"id":"esc-2"}"#]);
    assert!(o.status.success(), "a nested relative path is legal input:\n{}", out(&o));
    let written = w.notes();
    assert!(
        written.starts_with("sessoes/") && !written.contains(".."),
        "every note must stay under sessoes/, with no `..` anywhere in the path:\n{written}"
    );
}

/// The *new* input — a directory name — cannot be a traversal at all, and this says
/// why rather than staging one: a real directory's `file_name()` cannot contain `/` or
/// be `..`, because the filesystem forbids both. So there is no escape to construct
/// here, and a test that built one anyway would be testing a fiction. What the input
/// *can* do is reach the same sanitiser, which the passing case below shows.
#[test]
fn an_ordinary_directory_name_with_punctuation_is_filed_under_its_own_name() {
    let w = World::new("punctuated");
    let wd = w.work("meu.projeto-2");
    register(&w, "meu.projeto-2");

    let o = w.hook(&wd, &["--payload", r#"{"id":"pt-1"}"#]);
    assert!(o.status.success(), "{}", out(&o));
    assert_eq!(origin_line(&o), "hook resolve project=meu.projeto-2 origin=dir-name");
    assert_eq!(w.notes(), note_path_of(&o));
}

// ---------------------------------------------------------------------------
// RF-07.1 — no usable BRAIN_DIR degrades, it does not fail
// ---------------------------------------------------------------------------

/// **The critical one.** A missing `HOME` and a `BRAIN_DIR` that cannot be created
/// both used to abort the hook: `config::brain_dir()?` sat in `resolve_hook_project`,
/// before any gate, so the `?` produced exit 1 and an `Error:` on stderr — and every
/// `session-start`, `tool-result` and `session-end` died with it.
///
/// The old `hooks/brain-hook.py` applied a 30s timeout and degraded; RF-01 took it off
/// the supported path, which left nothing between an unusable environment and a broken
/// IDE.
///
/// Asserted on the exit status, because that is the failure: an IDE that gets a
/// non-zero from its hook reports the hook as broken.
#[test]
fn a_hook_without_home_or_a_writable_brain_dir_still_exits_zero() {
    let w = World::new("nohome");
    let wd = w.work("hive");
    register(&w, "hive");
    // A path under a *file*, so `create_dir_all` cannot succeed. This is the
    // read-only-parent case without depending on the test's uid.
    let blocker = w.root.join("blocker");
    std::fs::write(&blocker, b"not a directory").expect("blocker file");
    let unusable = blocker.join("nested").join("brain");

    let mut cmd = Command::new(bin());
    cmd.current_dir(&wd)
        .arg("--db")
        .arg(w.db())
        .arg("hook")
        .arg("--event")
        .arg("session-start")
        .arg("--payload")
        .arg(r#"{"id":"nh-1"}"#)
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_HOOK_EMBED", "0")
        .env("BRAIN_DIR", &unusable)
        // No HOME at all: the `~/.brain` fallback cannot be resolved either.
        .env_remove("HOME")
        .env("XDG_RUNTIME_DIR", w.xdg())
        .stdin(Stdio::null());
    let o = cmd.output().expect("spawn brain hook");

    assert!(
        o.status.success(),
        "an unusable BRAIN_DIR must degrade to exit 0, not fail the IDE. Got {:?}:\\n{}",
        o.status.code(),
        out(&o)
    );
    assert!(
        !out(&o).contains("Error:"),
        "the failure must not be reported as an error to the IDE:\\n{}",
        out(&o)
    );
    // The cascade still answered — the directory name is step 3 and needs no config.
    assert_eq!(origin_of(&o), "dir-name", "the cascade must run without a config:\\n{}", out(&o));
    assert!(
        w.session_note(&note_path_of(&o)).contains("nh-1"),
        "and the event is still recorded, under the resolved project"
    );
}

/// The same degradation for a path the process cannot write, which is the other way
/// this fails in practice: the directory exists but is read-only, so `create_dir_all`
/// succeeds and the *write* does not.
#[test]
fn a_read_only_brain_dir_degrades_rather_than_failing() {
    if !nix_read_only_supported() {
        eprintln!("skipping: the filesystem ignores the read-only bit for this uid");
        return;
    }
    let w = World::new("rodir");
    let wd = w.work("hive");
    register(&w, "hive");
    let ro = w.root.join("ro");
    std::fs::create_dir_all(&ro).expect("ro dir");
    make_read_only(&ro);

    let mut cmd = Command::new(bin());
    let o = cmd
        .current_dir(&wd)
        .arg("--db")
        .arg(w.db())
        .arg("hook")
        .arg("--event")
        .arg("session-start")
        .arg("--payload")
        .arg(r#"{"id":"ro-1"}"#)
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_HOOK_EMBED", "0")
        .env("BRAIN_DIR", &ro)
        .env("XDG_RUNTIME_DIR", w.xdg())
        .stdin(Stdio::null())
        .output()
        .expect("spawn brain hook");
    make_writable(&ro);

    assert!(
        o.status.success(),
        "a read-only BRAIN_DIR must not fail the hook. Got {:?}:\\n{}",
        o.status.code(),
        out(&o)
    );
    assert_eq!(origin_of(&o), "dir-name", "the cascade must still resolve:\\n{}", out(&o));
}

/// Whether a read-only directory is actually read-only *for this uid* — root ignores
/// the bit, and a test that silently stops testing anything under root is worse than
/// one that skips loudly.
fn nix_read_only_supported() -> bool {
    !nix_is_root()
}

#[cfg(unix)]
fn nix_is_root() -> bool {
    // SAFETY: `geteuid` takes no arguments, reads no memory and cannot fail.
    unsafe { libc_geteuid() == 0 }
}

#[cfg(not(unix))]
fn nix_is_root() -> bool {
    false
}

#[cfg(unix)]
unsafe fn libc_geteuid() -> u32 {
    // Declared locally rather than adding a libc dependency for one call.
    // SAFETY: `geteuid` takes no arguments, touches no memory and cannot fail; the
    // only requirement is that it exists, which it does on every unix target this
    // crate builds for.
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

#[cfg(unix)]
fn make_read_only(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o500)).expect("chmod ro");
}

#[cfg(not(unix))]
fn make_read_only(_p: &Path) {}

#[cfg(unix)]
fn make_writable(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn make_writable(_p: &Path) {}

// ---------------------------------------------------------------------------
// RF-03.1 — a typo is not a refusal
// ---------------------------------------------------------------------------

/// A mistyped key must not read as "the operator declined".
///
/// `parse_entry` defaults every absent key, so `{"projet":"hive"}` parses to
/// `projeto: None` — and treating that as a refusal made a directory *permanently
/// skipped* on the strength of a spelling mistake nobody had made a decision about.
/// The next `save_entry` would then rewrite the whole file and erase the typo, so the
/// evidence would be gone as well.
#[test]
fn a_typo_in_the_config_is_not_a_refusal_and_is_left_on_disk() {
    let w = World::new("typo");
    let wd = w.work("unmapped");
    register(&w, "hive");
    let typed = serde_json::json!({ w.key(&wd): { "projet": "hive" } });
    w.write_config(typed);
    let before = std::fs::read_to_string(w.brain_dir().join("config.json")).expect("config");

    let o = w.hook(&wd, &["--payload", r#"{"id":"ty-1"}"#]);
    assert!(o.status.success(), "{}", out(&o));
    assert_ne!(
        origin_of(&o),
        "recusado",
        "a typo must not become a refusal:\n{}", out(&o)
    );
    assert_eq!(
        std::fs::read_to_string(w.brain_dir().join("config.json")).expect("config"),
        before,
        "the operator's file must be byte-identical: the typo is the only evidence that \
         something is wrong with it"
    );
}

/// The other half of the same rule: an entry that *does* carry a project still works,
/// so the typo fix cannot have been "ignore the entry".
#[test]
fn a_well_formed_entry_with_a_project_is_still_honoured() {
    let w = World::new("goodkey");
    let wd = w.work("unmapped");
    register(&w, "hive");
    w.write_config(serde_json::json!({ w.key(&wd): { "projeto": "hive" } }));

    let o = w.hook(&wd, &["--payload", r#"{"id":"gk-1"}"#]);
    assert!(o.status.success(), "{}", out(&o));
    assert_eq!(origin_of(&o), "config");
    assert!(w.session_note(&note_path_of(&o)).contains("gk-1"));
}

/// An unrecognised `motivo` is not a refusal either — the word has to be the one the
/// code wrote. Anything else is a file to be fixed, not a decision to be obeyed.
#[test]
fn an_unrecognised_motivo_is_not_a_refusal() {
    let w = World::new("motivo");
    let wd = w.work("unmapped");
    register(&w, "hive");
    w.write_config(serde_json::json!({
        w.key(&wd): { "projeto": null, "motivo": "nao sei" }
    }));
    let before = std::fs::read_to_string(w.brain_dir().join("config.json")).expect("config");

    let o = w.hook(&wd, &["--payload", r#"{"id":"mv-1"}"#]);
    assert_ne!(origin_of(&o), "recusado", "an unknown motivo is not a refusal:\n{}", out(&o));
    assert_eq!(
        std::fs::read_to_string(w.brain_dir().join("config.json")).expect("config"),
        before,
        "an unrecognised motivo must be left for the operator to fix"
    );
}

// ---------------------------------------------------------------------------
// RF-07.2 — the config write is locked
// ---------------------------------------------------------------------------
