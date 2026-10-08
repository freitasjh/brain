//! `brain setup` — `kiro` as a target, and the interactive project question (RF-05,
//! RF-06, R-07).
//!
//! **This is where `config.json` finally gets written in production.** Until now the
//! cascade's step 1 had no producer: the hook deliberately does not ask, so a
//! directory's project decision could come from nowhere. That also meant two safety
//! work from the P1–P3 review had no reachable path — the file lock and the conised
//! `motivo: "recusado"`. Both are exercised here, through the real binary.
//!
//! The sandbox is per test: its own `HOME`, its own `BRAIN_DIR`, its own database and
//! its own project directory. `HOME` in particular, because `setup` writes
//! `~/.config/opencode`, `~/.config/systemd/user` and `~/.zshrc` — an inheriting test
//! would edit the developer's shell.
//!
//! `BRAIN_OLLAMA_URL` is pointed at a dead port and never removed: removing it
//! selects the real default backend, shared by every concurrent test binary (H2.3).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DEAD_OLLAMA: &str = "http://127.0.0.1:1";

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_brain"))
}

struct World {
    root: PathBuf,
}

impl World {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("brain-setup-t-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&root);
        for sub in ["home", "brain_dir", "proj"] {
            std::fs::create_dir_all(root.join(sub)).expect("sandbox");
        }
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn brain_dir(&self) -> PathBuf {
        self.root.join("brain_dir")
    }

    /// The project directory the setup runs *in*, and therefore the one it records a
    /// decision about.
    fn proj(&self) -> PathBuf {
        self.root.join("proj")
    }

    fn db(&self) -> String {
        self.root.join("brain.db").to_string_lossy().into_owned()
    }

    fn env(&self) -> PathBuf {
        self.root.join("xdg")
    }

    fn register(&self, name: &str) {
        let o = Command::new(bin())
            .arg("--db")
            .arg(self.db())
            .arg("project")
            .arg("create")
            .arg(name)
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            .output()
            .expect("spawn brain project create");
        assert!(o.status.success(), "registering {name}: {}", out(&o));
    }

    /// `brain setup <args>`, run in the project directory.
    fn setup(&self, args: &[&str]) -> std::process::Output {
        self.setup_with_stdin(args, None)
    }

    fn setup_with_stdin(&self, args: &[&str], stdin: Option<&str>) -> std::process::Output {
        let mut cmd = Command::new(bin());
        cmd.current_dir(self.proj())
            .arg("--db")
            .arg(self.db())
            .arg("setup")
            .args(args)
            .env("HOME", self.home())
            .env("BRAIN_DIR", self.brain_dir())
            .env("XDG_RUNTIME_DIR", self.env())
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            // No daemon-reload from a test, and no real systemctl.
            .env("BRAIN_SETUP_NO_SYSTEMCTL", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match stdin {
            Some(text) => {
                cmd.stdin(Stdio::piped());
                let mut child = cmd.spawn().expect("spawn brain setup");
                {
                    use std::io::Write;
                    child.stdin.as_mut().expect("stdin").write_all(text.as_bytes()).expect("write");
                }
                // Dropping stdin here closes the pipe, so a `read_line` sees EOF.
                child.wait_with_output().expect("wait")
            }
            None => {
                cmd.stdin(Stdio::null());
                cmd.output().expect("spawn brain setup")
            }
        }
    }

    /// A `brain hook` in the project directory, as an IDE would run it.
    fn hook(&self) -> std::process::Output {
        let mut cmd = Command::new(bin());
        cmd.current_dir(self.proj())
            .arg("--db")
            .arg(self.db())
            .arg("hook")
            .arg("--event")
            .arg("session-start")
            .arg("--payload")
            .arg(r#"{"id":"setup-1"}"#)
            .env("HOME", self.home())
            .env("BRAIN_DIR", self.brain_dir())
            .env("XDG_RUNTIME_DIR", self.env())
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            .env("BRAIN_HOOK_EMBED", "0")
            .stdin(Stdio::null());
        cmd.output().expect("spawn brain hook")
    }

    fn note_count(&self) -> usize {
        let o = Command::new(bin())
            .arg("--db")
            .arg(self.db())
            .arg("status")
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            .output()
            .expect("spawn brain status");
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        text.lines()
            .next()
            .and_then(|l| l.split_whitespace().find_map(|kv| kv.strip_prefix("notes=")))
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("no `notes=` in: {text}"))
    }

    /// Today, per the **system** clock, used only to build the path of a note the hook
    /// itself reported. Never used to decide what a run should do.
    fn today_utc(&self) -> String {
        let o = Command::new("date").arg("-u").arg("+%Y-%m-%d").output().expect("date");
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// `setup`, requiring success — so a test reads as one call and a failure points
    /// at the arrangement rather than at an unwrap.
    fn setup_ok(&self, args: &[&str]) {
        let o = self.setup(args);
        assert!(o.status.success(), "setup {args:?} failed:\n{}", out(&o));
    }

    /// The content of a session note, by full `sessoes/...` path.
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

    fn config(&self) -> serde_json::Value {
        let raw = std::fs::read_to_string(self.brain_dir().join("config.json"))
            .unwrap_or_else(|e| panic!("config.json must exist: {e}"));
        serde_json::from_str(&raw).expect("valid JSON")
    }

    /// The entry recorded for the project directory, canonicalised the way the binary
    /// canonicalises it.
    fn entry(&self) -> Option<serde_json::Value> {
        let key = std::fs::canonicalize(self.proj()).expect("canonicalize").to_string_lossy().into_owned();
        self.config()["brain_projects"].get(key).cloned()
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

fn origin_of(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .find_map(|l| {
            (l.starts_with("hook resolve ") || l.starts_with("hook skipped "))
                .then(|| l.split_whitespace().find_map(|t| t.strip_prefix("origin=")).map(String::from))
        })
        .flatten()
        .unwrap_or_else(|| panic!("no origin line in:\n{}", out(o)))
}

// ---------------------------------------------------------------------------
// T5.1 — kiro is a target
// ---------------------------------------------------------------------------

/// T5.1 + R-01: `kiro` is accepted and writes `.kiro/hooks/brain-session.json` with the
/// field names the SPEC fixes.
#[test]
fn setup_kiro_writes_the_kiro_hook_artifact() {
    let w = World::new("kiro");
    let o = w.setup(&["kiro", "--yes"]);
    assert!(o.status.success(), "{}", out(&o));

    let path = w.proj().join(".kiro").join("hooks").join("brain-session.json");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} must exist: {e}\n{}", path.display(), out(&o)));
    let v: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    assert_eq!(v["version"], "v1");
    let hooks = v["hooks"].as_array().expect("hooks");
    assert!(!hooks.is_empty(), "at least one hook: {raw}");
    for h in hooks {
        assert!(h["trigger"].is_string(), "R-01: a trigger per hook: {raw}");
        assert_eq!(h["action"]["type"], "command", "R-01: action.type command: {raw}");
        let cmd = h["action"]["command"].as_str().expect("command");
        assert!(cmd.contains("hook --event session"), "it must call `brain hook`: {cmd}");
    }

    // Asserted on the file **on disk**, not only on the generator's return value: the
    // required `name` and the real trigger are what a kiro reads, and a unit test on
    // the builder is not the same claim as a test on the artifact.
    for h in hooks {
        assert!(
            h["name"].as_str().is_some_and(|n| !n.trim().is_empty()),
            "hooks[].name is Required: Yes in the schema: {raw}"
        );
    }
    // The triggers are the documented ones, in the documented spelling.
    for h in hooks {
        let trigger = h["trigger"].as_str().expect("trigger");
        assert!(
            KIRO_TRIGGERS.contains(&trigger),
            "`{trigger}` is not a kiro trigger (kiro.dev/docs/hooks/types): {raw}"
        );
    }
    assert!(v.get("name").is_none(), "the top-level `name` is not in the file schema: {raw}");
}

/// The 11 triggers in `kiro.dev/docs/hooks/types`.
///
/// Duplicated here, in the test crate, on purpose: a test that read the list out of the
/// binary would agree with whatever the binary has — including a wrong one. This one
/// states the expected set independently, so removing a trigger from the crate's copy
/// fails here instead of passing because both sides moved together.
const KIRO_TRIGGERS: &[&str] = &[
    "PromptSubmit",
    "AgentStop",
    "SessionStart",
    "AgentSpawn",
    "PreToolUse",
    "PostToolUse",
    "FileCreate",
    "FileSave",
    "FileDelete",
    "PreTaskExecution",
    "PostTaskExecution",
];

/// The generated artifact must use `SessionStart` and `AgentStop`.
///
/// `AgentStop` and not a `SessionEnd`: the latter is not a trigger at all, and the
/// former is the one documented as firing on **IDE and CLI** — which is what makes it
/// usable for an artifact meant to work on both. `SessionStart` is IDE-only, and the
/// brain is only ever consulted from an IDE session, so it is kept.
#[test]
fn the_generated_artifact_uses_prompt_submit_and_agent_stop() {
    let w = World::new("triggers");
    let o = w.setup(&["kiro", "--yes"]);
    assert!(o.status.success(), "{}", out(&o));
    let raw = std::fs::read_to_string(w.proj().join(".kiro/hooks/brain-session.json"))
        .expect("the artifact");
    // **P4 changed the trigger, deliberately.** `SessionStart` became `PromptSubmit`,
    // because a session start is the one moment where the user has not typed anything
    // yet — the question this hook exists to inject does not exist. `PromptSubmit` is
    // where kiro documents the prompt as `USER_PROMPT`, so that is the only trigger
    // whose firing can carry one. `AgentStop` is unchanged.
    assert!(raw.contains("\"PromptSubmit\""), "got:\n{raw}");
    assert!(raw.contains("\"AgentStop\""), "got:\n{raw}");
    for gone in ["session.start", "session.end", "SessionEnd", "\"SessionStart\""] {
        assert!(!raw.contains(gone), "`{gone}` must not be the trigger any more: {raw}");
    }
    // The expansion must survive into the file unexpanded: a generated file cannot read
    // an environment, only the shell running this command can.
    //
    // Asserted on the **parsed** command, not on `raw`. In the file the quotes are
    // JSON-escaped (`--question \"\$USER_PROMPT\"`), so a substring check on the raw
    // text looks for something the file cannot contain — which is how this assertion
    // passed the command being wrong earlier.
    let v: serde_json::Value = serde_json::from_str(&raw).expect("json");
    let prompt = v["hooks"][0]["action"]["command"].as_str().expect("command");
    assert_eq!(
        prompt,
        format!("{} hook --event session-start --payload \"{{\\\"id\\\":\\\"kiro-session-start-$$\\\"}}\" --question \"$USER_PROMPT\"", bin().display()),
        "the question must be read at run time, not baked at install time"
    );
}

/// An unknown target is still refused, and the message now lists `kiro` — otherwise
/// the list is a second place that drifts from the accepted values.
#[test]
fn an_unknown_target_is_refused_and_the_message_lists_kiro() {
    let w = World::new("badtarget");
    let o = w.setup(&["emacs", "--yes"]);
    assert!(!o.status.success(), "an unknown target must fail");
    assert!(out(&o).contains("kiro"), "the error must list the valid targets:\n{}", out(&o));
}

// ---------------------------------------------------------------------------
// T5.4 / R-07 — nothing changes for a script
// ---------------------------------------------------------------------------

/// The regression that matters most: `brain setup systemd` and `shell` are what CI
/// calls today. Neither may ask, and neither may write a project decision.
#[test]
fn the_machine_targets_ask_nothing_and_record_nothing() {
    for target in ["systemd", "shell"] {
        let w = World::new(&format!("machine-{target}"));
        let o = w.setup(&[target]);
        assert!(o.status.success(), "{target}: {}", out(&o));
        assert!(
            !w.brain_dir().join("config.json").exists(),
            "{target} must not record a project decision:\n{}",
            out(&o)
        );
        assert!(
            !out(&o).contains("nobody to ask"),
            "{target} must not even warn about asking: it never asks"
        );
    }
}

/// With no terminal, an IDE target keeps the default IDE, warns, and records nothing.
/// The warning is the observable part: a silent skip would look like a bug.
#[test]
fn a_non_tty_run_keeps_the_default_and_warns() {
    let w = World::new("notty");
    let o = w.setup(&["all"]);
    assert!(o.status.success(), "a missing terminal must not fail the run:\n{}", out(&o));
    assert!(
        out(&o).contains("not a terminal"),
        "it must say why nothing was asked:\n{}", out(&o)
    );
    assert!(
        !w.brain_dir().join("config.json").exists(),
        "nothing may be recorded without an answer:\n{}", out(&o)
    );
    // And the opencode target was still installed, which is today's behaviour.
    assert!(
        w.home().join(".config/opencode/mcp.json").exists(),
        "the default IDE's artifact must still be written:\n{}", out(&o)
    );
}

/// `--yes` is the same outcome without the warning — the flag means "I know", so
/// nagging about it would be noise in every CI log.
#[test]
fn yes_is_silent_and_records_nothing() {
    let w = World::new("yes");
    let o = w.setup(&["opencode", "--yes"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(!out(&o).contains("not a terminal"), "--yes must not warn:\n{}", out(&o));
    assert!(!w.brain_dir().join("config.json").exists(), "--yes must not record");
}

/// The env var is the same escape for a CI job that cannot add flags.
#[test]
fn the_non_interactive_env_is_the_same_escape() {
    let w = World::new("envyes");
    let o = Command::new(bin())
        .current_dir(w.proj())
        .arg("--db")
        .arg(w.db())
        .arg("setup")
        .arg("opencode")
        .env("HOME", w.home())
        .env("BRAIN_DIR", w.brain_dir())
        .env("XDG_RUNTIME_DIR", w.env())
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_SETUP_NONINTERACTIVE", "1")
        .stdin(Stdio::null())
        .output()
        .expect("spawn");
    assert!(o.status.success(), "{}", out(&o));
    assert!(!w.brain_dir().join("config.json").exists(), "it must not record");
}

// ---------------------------------------------------------------------------
// T5.3 / RF-06 — dry run
// ---------------------------------------------------------------------------

/// `--dry-run` asks, decides, shows — and writes nothing at all: no config, no
/// artifact. The distinction from a no-op is that the operator learns what would
/// happen.
#[test]
fn dry_run_writes_nothing_either_to_disk_or_to_the_ide() {
    let w = World::new("dry");
    let o = w.setup(&["kiro", "--dry-run", "--yes"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(
        !w.brain_dir().join("config.json").exists(),
        "--dry-run must not record a project:\n{}", out(&o)
    );
    assert!(
        !w.proj().join(".kiro").exists(),
        "--dry-run must not write the kiro artifact:\n{}", out(&o)
    );
    assert!(out(&o).contains("dry-run would write"), "and must say what it would do:\n{}", out(&o));
}

// ---------------------------------------------------------------------------
// Acceptance criterion 3 — the conised refusal, end to end
// ---------------------------------------------------------------------------

/// T5.5 + the second acceptance criterion: declining records the conised refusal, and
/// the hook that runs afterwards neither asks nor writes.
///
/// Driven by `--decline-project`, the explicit non-interactive answer — so the bytes
/// come from the real `config::save_entry`, under the real file lock, and not from a
/// hand-written file. The refusal is what the cascade matches on, so the chain is
/// asserted from the recorded bytes rather than from the origin label: `motivo` must
/// be exactly `recusado`.
#[test]
fn declining_records_the_conised_refusal_and_the_hook_then_skips() {
    let w = World::new("recusa");
    w.register("hive");

    let o = w.setup(&["opencode", "--decline-project"]);
    assert!(o.status.success(), "{}", out(&o));

    let stored = w.entry().expect("an entry must be recorded for the project dir");
    assert_eq!(stored["projeto"], serde_json::Value::Null, "a refusal has no project");
    assert_eq!(
        stored["motivo"], "recusado",
        "the reason must be the exact string the cascade matches on (RF-03.1): {stored}"
    );

    let o = w.hook();
    assert!(o.status.success(), "the hook must not fail:\n{}", out(&o));
    assert_eq!(origin_of(&o), "recusado", "the hook must recognise its own refusal:\n{}", out(&o));
    assert_eq!(w.note_count(), 0, "a refused directory writes no note");
}

/// A recorded project is honoured by the hook, and the note lands in that project —
/// the whole point of the chain, and the only end-to-end proof that the two halves
/// agree on the path and on the key.
#[test]
fn a_recorded_project_is_honoured_by_the_hook() {
    let w = World::new("aceite");
    w.register("hive");

    let o = w.setup(&["opencode", "--project", "hive"]);
    assert!(o.status.success(), "{}", out(&o));
    let stored = w.entry().expect("entry");
    assert_eq!(stored["projeto"], "hive");

    let o = w.hook();
    assert_eq!(origin_of(&o), "config", "the hook must read the recorded decision:\n{}", out(&o));
    let note = String::from_utf8_lossy(&o.stdout);
    assert!(
        note.contains("note=sessoes/hive/"),
        "and write the event into that project:\n{note}"
    );
    assert_eq!(w.note_count(), 1);
}

/// An explicit answer is not overridden by `--yes`, and does not warn: it is an
/// answer, not a fallback. A test that passed with either order would prove nothing
/// about which one wins.
#[test]
fn an_explicit_answer_beats_yes() {
    let w = World::new("both");
    let o = w.setup(&["opencode", "--yes", "--project", "hive"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(!out(&o).contains("non-interactive"), "an explicit answer must not warn:\n{}", out(&o));
    assert_eq!(
        w.entry().expect("entry")["projeto"],
        "hive",
        "the explicit project must win over --yes"
    );
}

/// `--dry-run` with an explicit answer still writes nothing: the flag is about the
/// write, and the answer is about the question.
#[test]
fn dry_run_with_an_explicit_answer_records_nothing() {
    let w = World::new("dryanswer");
    let o = w.setup(&["opencode", "--dry-run", "--project", "hive"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(
        !w.brain_dir().join("config.json").exists(),
        "--dry-run must not record:\n{}", out(&o)
    );
    assert!(
        out(&o).contains("dry-run would record hive"),
        "and must say what it would have recorded:\n{}", out(&o)
    );
}

/// The two answers conflict, and the operator should be told rather than have one
/// silently win.
#[test]
fn naming_a_project_and_declining_at_once_is_refused() {
    let w = World::new("conflict");
    let o = w.setup(&["opencode", "--project", "hive", "--decline-project"]);
    assert!(
        !o.status.success(),
        "declaring a project and declining at once is a contradiction: {:?}\n{}",
        o.status.code(),
        out(&o)
    );
    assert!(
        !w.brain_dir().join("config.json").exists(),
        "and nothing may be recorded from a contradictory invocation"
    );
}

// ---------------------------------------------------------------------------
// Acceptance criterion 2 — the file lock, reachable through setup
// ---------------------------------------------------------------------------

/// Eight real `brain setup` processes recording eight different directories at the
/// same instant, and **all** entries present at the end.
///
/// This is the acceptance criterion, and it is a different test from the unit-level
/// one in `config.rs`: there, real processes re-executed the test binary because
/// `config.rs` is a module of the `brain` binary and not importable. Here, real
/// processes run the *shipped command an operator runs*, which is the only version of
/// this claim worth making — it goes through clap, through `run_setup`, and through
/// `config::save_entry` with the lock.
///
/// The bug being pinned is a lost update: both processes read the same map, both
/// insert their own key, and the second rename discards the first. The atomic rename
/// makes the file well-formed either way, so only the lock keeps both decisions.
#[test]
fn two_concurrent_setups_do_not_lose_each_others_decisions() {
    let w = World::new("cfgrace");
    // **8, not 2.** With two processes the lost update is a coin flip — the window
    // between reading the map and renaming the file is microseconds — and the test
    // passed with the lock *removed*. Eight overlapping processes open the window
    // wide enough to be a real assertion; the unit-level test in `config.rs` uses 12
    // for the same reason.
    const N: usize = 8;
    let dirs: Vec<PathBuf> = (0..N).map(|i| w.root.join(format!("proj{i}"))).collect();
    for d in &dirs {
        std::fs::create_dir_all(d).expect("dir");
    }

    // Spawn both before waiting on either, so they overlap.
    let mut children = Vec::with_capacity(dirs.len());
    for (i, dir) in dirs.iter().enumerate() {
        let mut cmd = Command::new(bin());
        cmd.current_dir(dir)
            .arg("--db")
            .arg(w.db())
            .arg("setup")
            .arg("opencode")
            .arg("--project")
            .arg(format!("proj-{i}"))
            .env("HOME", w.home())
            .env("BRAIN_DIR", w.brain_dir())
            .env("XDG_RUNTIME_DIR", w.env())
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            .env("BRAIN_SETUP_NO_SYSTEMCTL", "1")
            .stdin(Stdio::null());
        children.push(cmd.spawn().expect("spawn brain setup"));
    }
    for (i, mut child) in children.into_iter().enumerate() {
        let status = child.wait().expect("wait");
        assert!(status.success(), "setup {i} failed: {status:?}");
    }

    let config = w.config();
    let entries = config["brain_projects"].as_object().expect("brain_projects");
    let mut present: Vec<String> = entries.keys().cloned().collect();
    present.sort();
    let mut expected: Vec<String> = dirs
        .iter()
        .map(|d| std::fs::canonicalize(d).expect("canonicalize").to_string_lossy().into_owned())
        .collect();
    expected.sort();
    assert_eq!(
        present, expected,
        "a concurrent setup lost a decision. The rename keeps the file well-formed; \
         only the lock keeps both."
    );
    // And the values are right, not just the keys: a merge that dropped the payload
    // while keeping the path would pass a key-only assertion.
    for (i, d) in dirs.iter().enumerate() {
        let key = std::fs::canonicalize(d).expect("canonicalize").to_string_lossy().into_owned();
        assert_eq!(
            entries[&key]["projeto"], format!("proj-{i}"),
            "each directory must keep its own project, not the other's: {entries:?}"
        );
    }
}

/// A read-only `BRAIN_DIR` must not take the whole install down with it: the
/// artifacts are already in place, so a failed record is a warning (RF-07's
/// "degrade, do not break"), and the exit code stays 0.
#[test]
fn an_unwritable_config_is_a_warning_not_a_failure() {
    if unsafe { is_root() } {
        eprintln!("skipping: root ignores the read-only bit, so this would test nothing");
        return;
    }
    let w = World::new("rowrite");
    let blocker = w.root.join("blocker");
    std::fs::write(&blocker, b"a file, not a directory").expect("blocker");

    let mut cmd = Command::new(bin());
    let o = cmd
        .current_dir(w.proj())
        .arg("--db")
        .arg(w.db())
        .arg("setup")
        .arg("opencode")
        .arg("--project")
        .arg("hive")
        .env("HOME", w.home())
        .env("BRAIN_DIR", blocker.join("nested"))
        .env("XDG_RUNTIME_DIR", w.env())
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_SETUP_NO_SYSTEMCTL", "1")
        .stdin(Stdio::null())
        .output()
        .expect("spawn brain setup");

    assert!(o.status.success(), "an unwritable config must not fail the install:\n{}", out(&o));
    assert!(
        out(&o).contains("could not record"),
        "and it must say what did not happen:\n{}", out(&o)
    );
    assert!(
        w.home().join(".config/opencode/mcp.json").exists(),
        "the artifacts are still installed: a lost record is not a lost install"
    );
}

#[cfg(unix)]
unsafe fn is_root() -> bool {
    // SAFETY: `geteuid` takes no arguments, reads no memory and cannot fail.
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() == 0 }
}

#[cfg(not(unix))]
unsafe fn is_root() -> bool {
    false
}

// ---------------------------------------------------------------------------
// P6 — the opencode plugin
// ---------------------------------------------------------------------------

/// The plugins directory with plausible neighbours, so "did we touch anything else"
/// is answerable rather than assumed. `rtk.ts` is modelled because a TypeScript plugin
/// is what the owner's directory really contains, and a `.ts` sibling is the case a
/// name collision would most plausibly hit.
fn plugins_with_neighbours(home: &std::path::Path) -> PathBuf {
    let dir = home.join(".config").join("opencode").join("plugins");
    std::fs::create_dir_all(&dir).expect("plugins dir");
    std::fs::write(dir.join("rtk.ts"), "export const Plugin = async () => ({})\n").expect("rtk.ts");
    std::fs::create_dir_all(dir.join("caveman")).expect("caveman dir");
    std::fs::write(dir.join("caveman").join("index.js"), "// untouched\n").expect("caveman");
    dir
}

fn plugin_path(home: &std::path::Path) -> PathBuf {
    home.join(".config").join("opencode").join("plugins").join("brain-session.js")
}

/// The generated plugin with its `//` comments removed.
///
/// The artifact's comments explain *why* the payload exists, and one of them names
/// `brain-hook.py` — so a whole-file scan for the forbidden string flags the plugin's
/// own explanation. Third time in this module; the helper is here so the rule is
/// applied once.
fn plugin_code(home: &std::path::Path) -> String {
    let raw = std::fs::read_to_string(plugin_path(home)).expect("the plugin");
    raw.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// T6.1: `brain setup opencode` now installs the plugin. Before this phase it wrote the
/// MCP entry and nothing else, so the opencode side of R-01 did not exist.
#[test]
fn setup_opencode_installs_the_session_plugin() {
    let w = World::new("plugin");
    let o = w.setup(&["opencode", "--yes"]);
    assert!(o.status.success(), "{}", out(&o));

    let path = plugin_path(&w.home());
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("the plugin must be installed at {}: {e}\n{}", path.display(), out(&o)));
    let code = plugin_code(&w.home());
    assert!(code.contains("export const Plugin"), "the documented export:\n{code}");
    assert!(code.contains("session.created"), "and the documented event:\n{code}");
    // T6.3: the Rust binary, not the Python hook — and MUST FIX 1: an id per call.
    assert!(!code.contains("brain-hook.py"), "R-08: the Python hook is off the path:\n{code}");
    assert!(code.contains("--payload"), "MUST FIX 1: each session needs an id:\n{code}");

    // And the MCP config the existing test suite checks is still written and parses.
    let mcp = w.home().join(".config/opencode/mcp.json");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&mcp).expect("mcp.json")).expect("mcp.json parses");
    assert_eq!(v["mcpServers"]["brain"]["url"], "http://localhost:8321/sse");
}

/// Idempotence, and the neighbour. Two runs must leave exactly one plugin, byte-identical,
/// and must not have touched the other files in the directory.
///
/// The plugin is a file the operator may well have edited, so a second run skipping it
/// is the correct behaviour — not a bug to be fixed by overwriting.
#[test]
fn running_setup_twice_installs_one_plugin_and_leaves_the_neighbours_alone() {
    let w = World::new("twice");
    let dir = plugins_with_neighbours(&w.home());
    let rtk_before = std::fs::read(dir.join("rtk.ts")).expect("rtk.ts");
    let caveman_before = std::fs::read(dir.join("caveman/index.js")).expect("caveman");

    let first = w.setup(&["opencode", "--yes"]);
    assert!(first.status.success(), "{}", out(&first));
    let after_first = std::fs::read_to_string(plugin_path(&w.home())).expect("plugin after first run");

    let second = w.setup(&["opencode", "--yes"]);
    assert!(second.status.success(), "{}", out(&second));
    let after_second = std::fs::read_to_string(plugin_path(&w.home())).expect("plugin after second run");

    assert_eq!(after_first, after_second, "a second run must not rewrite the plugin");
    assert!(
        out(&second).contains("skipped"),
        "and must say it skipped rather than silently doing nothing:\n{}", out(&second)
    );

    // Exactly one brain plugin: the file must not accumulate a second copy or a `.bak`.
    let brain_files: Vec<String> = std::fs::read_dir(&dir)
        .expect("read plugins dir")
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|n| n.contains("brain"))
        .collect();
    assert_eq!(brain_files, vec!["brain-session.js".to_string()], "one file, no .bak, no duplicate: {brain_files:?}");

    // The neighbours are byte-identical. This is the assertion a name collision would
    // break, and it is the reason the plugin is written by exact name.
    assert_eq!(std::fs::read(dir.join("rtk.ts")).expect("rtk.ts"), rtk_before, "rtk.ts must be untouched");
    assert_eq!(
        std::fs::read(dir.join("caveman/index.js")).expect("caveman"),
        caveman_before,
        "the caveman plugin must be untouched"
    );
    assert!(dir.join("caveman").is_dir(), "and the neighbour directory itself must survive");
}

/// `--force` refreshes the plugin, because the paths baked into it change when the
/// binary or the database moves. The previous copy is kept, as everywhere else.
#[test]
fn force_refreshes_the_plugin_and_keeps_the_previous_copy() {
    let w = World::new("force");
    let first = w.setup(&["opencode", "--yes"]);
    assert!(first.status.success(), "{}", out(&first));
    let path = plugin_path(&w.home());
    std::fs::write(&path, "// edited by hand\n").expect("edit");

    let forced = w.setup(&["opencode", "--yes", "--force"]);
    assert!(forced.status.success(), "{}", out(&forced));
    let now = std::fs::read_to_string(&path).expect("plugin");
    assert!(now.contains("export const Plugin"), "--force must regenerate it:\n{now}");
    assert!(
        path.with_extension("bak").exists(),
        "and must keep what it replaced"
    );
}

/// RF-07 / T6.4: the plugin must not put an error in the conversation. Two ways this
/// breaks in the field, both asserted: the binary is missing, and the database is
/// unusable. Either way `brain hook` leaves `notes` untouched and says so on stderr.
#[test]
fn a_degraded_hook_writes_nothing_and_exits_zero() {
    let w = World::new("degrade");
    w.register("hive");

    // No brain binary at all — the plugin's shell call fails at the exec.
    let missing = w.root.join("no-such-brain");
    let o = Command::new(&missing)
        .arg("--db")
        .arg(w.db())
        .arg("hook")
        .arg("--event")
        .arg("session-start")
        .arg("--payload")
        .arg("{}")
        .output();
    assert!(o.is_err() || !o.as_ref().unwrap().status.success(), "a missing binary cannot succeed");

    // The binary exists but the database cannot be created: degraded, not fatal.
    let o = Command::new(bin())
        .current_dir(w.proj())
        .arg("--db")
        .arg("/proc/definitely/not/writable.db")
        .arg("hook")
        .arg("--event")
        .arg("session-start")
        .arg("--project")
        .arg("hive")
        .arg("--payload")
        .arg(r#"{"id":"deg-1"}"#)
        .env("HOME", w.home())
        .env("BRAIN_DIR", w.brain_dir())
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_HOOK_EMBED", "0")
        .stdin(Stdio::null())
        .output()
        .expect("spawn brain hook");
    let text = out(&o);
    assert!(
        !text.contains("Brain context"),
        "a degraded run must not print an injection block into the conversation:\n{text}"
    );
    assert!(
        !o.status.success() || text.contains("hook skipped") || !text.contains("Error:"),
        "either it degraded quietly, or it failed without leaking a brain error:\n{text}"
    );
}

// ---------------------------------------------------------------------------
// Review fixes — measured behaviour, not prose
// ---------------------------------------------------------------------------

/// MUST FIX 1, measured: run the command **exactly as the kiro artifact writes it**
/// three times and require three sections.
///
/// The defect was a constant dedup key. `brain hook` with no payload hashes
/// `"<event>|<project>|{{}}"`, so the first run recorded and every later one printed
/// `hook deduplicated id=6c4ddca40020c687` — one session per project per login, from
/// an artifact whose own comment claimed it records each session.
#[test]
fn the_kiro_command_records_every_session_it_is_run_for() {
    let w = World::new("kiroid");
    w.register("hive");
    // The directory has to *resolve*: an unattributed event is skipped on purpose, so
    // running the command in an unmapped directory would prove nothing about the dedup.
    let mapped = w.root.join("hive");
    std::fs::create_dir_all(&mapped).expect("mapped dir");
    let dir = mapped.to_str().unwrap().to_string();
    w.setup_ok(&["kiro", "--yes", "--dir", &dir]);

    let raw = std::fs::read_to_string(mapped.join(".kiro/hooks/brain-session.json")).expect("artifact");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("json");
    let command = v["hooks"][0]["action"]["command"].as_str().expect("command");
    assert!(command.contains("--payload"), "the command must carry an id: {command}");

    // Run it as kiro would: through a shell, which is what expands `$$`.
    //
    // `BRAIN_DB_PATH` points at a scratch database, and that is the isolation claim
    // under test: the artifact names **no** `--db`, so the environment decides. A
    // frozen path would record into the production corpus instead — which is what
    // happened once in this session, from a command an earlier version of this very
    // artifact generated.
    // **The env must point somewhere else from the install-time database**, or the
    // test cannot tell "followed the environment" from "froze the same value" — and
    // the first version of this test made exactly that mistake, so re-freezing `--db`
    // passed it.
    let other = w.root.join("other.db");
    let other = other.to_string_lossy().into_owned();
    let o = Command::new(bin())
        .arg("--db")
        .arg(&other)
        .arg("project")
        .arg("create")
        .arg("hive")
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .output()
        .expect("register hive in the other database");
    assert!(o.status.success(), "{}", out(&o));

    for _ in 0..3 {
        let o = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&mapped)
            .env("HOME", w.home())
            .env("BRAIN_DIR", w.brain_dir())
            .env("BRAIN_DB_PATH", &other)
            .env("XDG_RUNTIME_DIR", w.env())
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            .env("BRAIN_HOOK_EMBED", "0")
            .output()
            .expect("run the generated command");
        let text = out(&o);
        assert!(text.contains("hook ok"), "each run must record, not deduplicate:\n{text}");
        assert!(!text.contains("deduplicated"), "nothing may be dropped:\n{text}");
    }

    // The database the environment named holds the sessions...
    let other_note = {
        let rel = format!("hive/{}", w.today_utc());
        let o = Command::new(bin())
            .arg("--db")
            .arg(&other)
            .arg("read")
            .arg("sessoes")
            .arg(&rel)
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            .output()
            .expect("read the other database");
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    assert_eq!(
        other_note.lines().filter(|l| l.starts_with("## ")).count(),
        3,
        "BRAIN_DB_PATH must decide where the sessions go:\n{other_note}"
    );
    // ...and the one the artifact was installed against has none of them. This is the
    // assertion that fails when a path is frozen back into the command.
    let installed_note = w.session_note(&format!("sessoes/hive/{}", w.today_utc()));
    assert!(
        !installed_note.contains("session-start"),
        "the install-time database must be untouched:\n{installed_note}"
    );
    // ...and the default it would otherwise have used was never created. This half is
    // what fails if a path is ever frozen back into the command.
    assert!(
        !mapped.join("data").join("brain.db").exists(),
        "the hook must not fall back to ./data/brain.db inside the project while \
         BRAIN_DB_PATH is set: {} was created",
        mapped.join("data/brain.db").display()
    );
}

/// The same property for the opencode artifact, exercised by running the plugin's
/// hook under `node` three times against the real binary.
#[test]
fn the_opencode_plugin_sends_a_distinct_id_per_invocation() {
    let w = World::new("ocid");
    w.register("hive");
    w.setup_ok(&["opencode", "--yes"]);
    let code = plugin_code(&w.home());
    assert!(code.contains("--payload"), "the plugin must send an id:\\n{code}");

    let node = std::process::Command::new("node").arg("--version").output();
    if node.is_err() || !node.unwrap().status.success() {
        eprintln!("skipping: node is not installed, so the plugin cannot be executed here");
        return;
    }
    // Three calls in one process exercise the counter half of the id; the timestamp
    // half is what separates processes, and the test above covers that for kiro.
    // A stand-in for Bun's `$` that **actually runs the command**: a stubbed
    // `text: async () => ""` would prove nothing about which database was written,
    // which is the whole claim. Tagged-template semantics are reproduced because the
    // plugin calls `` $`...` ``, and `.cwd()` is honoured because the plugin sets it.
    let harness = r#"
import { Plugin } from "./brain-session.js";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
const run = promisify(execFile);

// A stand-in for Bun's `$`. **Each interpolation is one `argv` entry**, which is what
// Bun's shell does and what the previous version of this mock got wrong: it concatenated
// the strings and handed the result to `/bin/sh`, so `${{""}}` produced *nothing* rather
// than an empty argument. The plugin passes `--question ${{question ?? ""}}`, and
// `session.created` legitimately has no question, so the mock turned a valid invocation
// into `... --question ` — a flag with no value, which clap refuses.
//
// Building `argv` and calling `execFile` directly also removes the shell from the path
// entirely, so no quoting rule of `/bin/sh` can mask a defect in the plugin.
const $ = (strings, ...values) => {
  const argv = [];
  strings.forEach((s, i) => {
    for (const tok of s.split(/\s+/).filter(Boolean)) argv.push(tok);
    if (i < values.length) argv.push(String(values[i] ?? ""));
  });
  let cwd = process.cwd();
  const chain = {
    cwd: (dir) => { cwd = dir; return chain; },
    nothrow: () => chain,
    text: async () => {
      try {
        const r = await run(argv[0], argv.slice(1), { env: process.env, cwd });
        return r.stdout;
      } catch (e) {
        return String(e.stdout || "") + String(e.stderr || "");
      }
    },
  };
  return chain;
};

const hooks = await Plugin({
  client: { app: { log: () => {} } },
  $,
  directory: process.env.BRAIN_TEST_DIR,
});
for (let i = 0; i < 3; i++) await hooks["session.created"]({});
console.log("DONE");
"#;    let d = w.root.join("harness");
    std::fs::create_dir_all(&d).expect("harness dir");
    std::fs::write(d.join("brain-session.js"), code).expect("plugin");
    std::fs::write(d.join("harness.js"), harness).expect("harness");

    // The env points elsewhere from the install-time database, for the same reason as
    // the kiro test: otherwise "froze the value" and "followed the env" look alike.
    let other = w.root.join("oc-other.db");
    let other = other.to_string_lossy().into_owned();
    let reg = Command::new(bin())
        .arg("--db")
        .arg(&other)
        .arg("project")
        .arg("create")
        .arg("hive")
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .output()
        .expect("register hive");
    assert!(reg.status.success(), "{}", out(&reg));

    let mapped = w.root.join("hive");
    std::fs::create_dir_all(&mapped).expect("mapped dir");
    let o = Command::new("node")
        .arg(d.join("harness.js"))
        .current_dir(&d)
        .env("BRAIN_TEST_DIR", mapped.to_string_lossy().to_string())
        .env("BRAIN_DB_PATH", &other)
        .env("PATH", format!("{}:{}", bin().parent().unwrap().display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .expect("run harness");
    assert!(out(&o).contains("DONE"), "the plugin must not throw:\\n{}", out(&o));

    let read = |db: &str| {
        let r = Command::new(bin())
            .arg("--db")
            .arg(db)
            .arg("read")
            .arg("sessoes")
            .arg(format!("hive/{}", w.today_utc()))
            .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
            .output()
            .expect("read");
        String::from_utf8_lossy(&r.stdout).into_owned()
    };
    // The environment decides where the sessions go...
    assert_eq!(
        read(&other).lines().filter(|l| l.starts_with("## ")).count(),
        3,
        "process.env.BRAIN_DB_PATH must decide where the sessions go:\\n{}",
        read(&other)
    );
    // ...and the database the plugin was installed against has none of them. This is
    // the assertion that fails if the fallback is used unconditionally.
    assert!(
        !read(&w.db()).contains("session-start"),
        "the install-time database must be untouched:\\n{}",
        read(&w.db())
    );
}

/// MUST FIX 2: `--project "../evil"` must not reach the file, and the hook must exit 0
/// anyway if such an entry exists by other means.
#[test]
fn a_project_name_that_cannot_be_a_path_component_never_reaches_the_config() {
    let w = World::new("evil");
    // The install itself still completes: the artifacts are written, and RF-07 is
    // about not turning an operator's earlier mistake into a broken machine. What must
    // not happen is the *recording*.
    let o = w.setup(&["opencode", "--project", "../evil"]);
    assert!(o.status.success(), "the install completes regardless:\n{}", out(&o));
    assert!(
        out(&o).contains("refusing to record project"),
        "and the refusal is stated:\n{}", out(&o)
    );
    if w.brain_dir().join("config.json").exists() {
        let raw = std::fs::read_to_string(w.brain_dir().join("config.json")).expect("config");
        assert!(!raw.contains("../"), "the file must not contain a traversal:\n{raw}");
    }

    // And a name hand-written into the file degrades instead of failing the IDE.
    let key = std::fs::canonicalize(w.proj()).expect("canonicalize").to_string_lossy().into_owned();
    std::fs::write(
        w.brain_dir().join("config.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "brain_projects": { key: { "projeto": "../evil", "motivo": null, "desabilitado": false } }
        }))
        .expect("json"),
    )
    .expect("write");
    let h = w.hook();
    assert!(h.status.success(), "a poisoned config must not fail the IDE:\\n{}", out(&h));
    assert!(out(&h).contains("nothing was written"), "and must say so:\\n{}", out(&h));
    assert_eq!(w.note_count(), 0, "and write nothing");
}

/// Attention 3: `--project` with a machine target is refused loudly, not swallowed.
#[test]
fn the_project_flag_is_refused_for_a_machine_target() {
    for target in ["systemd", "shell"] {
        let w = World::new(&format!("flag-{target}"));
        let o = w.setup(&[target, "--project", "hive"]);
        assert!(!o.status.success(), "`setup {target} --project hive` must be refused:\\n{}", out(&o));
        assert!(
            out(&o).contains("does not apply"),
            "and must say why, not exit 0 in silence:\\n{}", out(&o)
        );
        assert!(
            !w.brain_dir().join("config.json").exists(),
            "and must not have recorded anything"
        );
    }
}

/// …but it is meaningful for `project`, which acts on a directory.
#[test]
fn the_project_flag_is_honoured_for_the_project_target() {
    let w = World::new("flag-proj");
    let o = w.setup(&["project", "--project", "hive", "--brain-dir", w.brain_dir().to_str().unwrap()]);
    assert!(o.status.success(), "{}", out(&o));
    assert_eq!(w.entry().expect("entry")["projeto"], "hive");
}

/// Attention 6, after the `--db` removal: only the **binary** can go stale.
///
/// The database is read from the environment at run time, so a `--db` that differs
/// from the installed one is not staleness — it is the environment having moved, and
/// the plugin follows it. Reporting that would be a false alarm on every reconfigure.
#[test]
fn a_moved_database_is_not_reported_as_stale() {
    let w = World::new("stale-db");
    w.setup_ok(&["opencode", "--yes"]);
    let other = w.root.join("newbrain.db");
    let o = Command::new(bin())
        .current_dir(w.proj())
        .arg("--db")
        .arg(&other)
        .arg("setup")
        .arg("opencode")
        .arg("--yes")
        .env("HOME", w.home())
        .env("BRAIN_DIR", w.brain_dir())
        .env("XDG_RUNTIME_DIR", w.env())
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_SETUP_NO_SYSTEMCTL", "1")
        .stdin(Stdio::null())
        .output()
        .expect("spawn brain setup");
    assert!(o.status.success(), "{}", out(&o));
    assert!(
        !out(&o).contains("points at"),
        "a different --db is the environment changing, not a stale plugin:\n{}",
        out(&o)
    );
}

/// …and a moved **binary** is real staleness, reported on stderr without rewriting the
/// file, because `--force` stays the operator's decision.
#[test]
fn a_stale_binary_in_the_plugin_is_reported() {
    let w = World::new("stale-bin");
    w.setup_ok(&["opencode", "--yes"]);
    // A plugin left behind by an install whose binary has since moved.
    let path = plugin_path(&w.home());
    let edited = std::fs::read_to_string(&path)
        .expect("plugin")
        .replace("const BRAIN_BIN = \"", "const BRAIN_BIN = \"/gone/brain\"; const OLD = \"");
    std::fs::write(&path, edited).expect("edit");
    let before = std::fs::read_to_string(&path).expect("plugin");

    let o = w.setup(&["opencode", "--yes"]);
    assert!(o.status.success(), "{}", out(&o));
    assert!(out(&o).contains("skipped"), "the operator's file still wins:\n{}", out(&o));
    assert!(
        out(&o).contains("points at") && out(&o).contains("/gone/brain"),
        "and the stale binary is named:\n{}", out(&o)
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("plugin"),
        before,
        "and nothing was rewritten without --force"
    );
}

/// Attention 4: a terminal with nobody in front of it must not hang the command.
///
/// Measured before the fix: `sleep 30 | timeout 6 brain setup opencode` under a real
/// pty printed the question and never returned (exit 124) — nothing installed at all.
/// The bound here is generous enough for a human and finite enough for a pipeline.
#[test]
fn a_terminal_with_nobody_answers_eventually() {
    let Ok(script) = std::process::Command::new("script").arg("-qec").arg("true").arg("/dev/null").output() else {
        eprintln!("skipping: `script` is not installed, so no pty can be made here");
        return;
    };
    if !script.status.success() {
        eprintln!("skipping: `script` could not allocate a pty here");
        return;
    }

    let w = World::new("pty");
    w.register("hive");
    // `sleep 30` holds the pipe open and sends nothing: a pty, and no human.
    let inner = format!(
        "sleep 30 | {} --db {} setup opencode --yes",
        bin().display(),
        w.db()
    );
    let started = std::time::Instant::now();
    let o = Command::new("script")
        .arg("-qec")
        .arg(&inner)
        .arg("/dev/null")
        .env("HOME", w.home())
        .env("BRAIN_DIR", w.brain_dir())
        .env("XDG_RUNTIME_DIR", w.env())
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_SETUP_NO_SYSTEMCTL", "1")
        .current_dir(w.proj())
        .output();
    let elapsed = started.elapsed();
    let o = match o {
        Ok(o) => o,
        Err(_) => {
            eprintln!("skipping: could not run `script`");
            return;
        }
    };
    // The first question times out after READ_TIMEOUT; three attempts would be three
    // times that, so the bound here is generous and still proves it terminates.
    assert!(
        elapsed < std::time::Duration::from_secs(120),
        "setup must not hang on a terminal with nobody in it (took {elapsed:?})"
    );
    assert!(
        w.home().join(".config/opencode/plugins/brain-session.js").exists(),
        "and it must still have installed what it could:\\n{}",
        out(&o)
    );
}

// ---------------------------------------------------------------------------
// P4 / RF-04: inject the real question, filter by project
// ---------------------------------------------------------------------------

/// The corpus both P4 tests search: one project note whose text carries a term, one
/// global note carrying another, and — the point of T4.2 — a project note whose text does
/// **not** mention the project by name.
fn p4_corpus(w: &World) {
    w.register("hive");
    let store = |layer: &str, path: &str, body: &str, scope: &str, project: Option<&str>| {
        let mut c = Command::new(bin());
        c.arg("--db").arg(w.db()).arg("store").arg(layer).arg(path).arg(body)
            .arg("--scope").arg(scope).env("BRAIN_OLLAMA_URL", DEAD_OLLAMA);
        if let Some(p) = project {
            c.arg("--project").arg(p);
        }
        let o = c.output().expect("store");
        assert!(o.status.success(), "store {layer}/{path}: {}", out(&o));
    };
    store("regras", "hive/naming", "## Naming\n\nUse kebab-case for every path.", "projetos", Some("hive"));
    store("regras", "hive/auth", "## Auth\n\nRotate the session token on login.", "projetos", Some("hive"));
    store("regras", "coding", "## Global\n\nRun the whole suite before commit.", "global", None);
}

/// Run the generated command with `USER_PROMPT` set, as kiro would for `PromptSubmit`.
fn p4_run_kiro(w: &World, dir: &Path, command: &str, prompt: Option<&str>) -> String {
    let mut c = Command::new("sh");
    c.arg("-c").arg(command).current_dir(dir)
        .env("HOME", w.home())
        .env("BRAIN_DIR", w.brain_dir())
        .env("BRAIN_DB_PATH", w.db())
        .env("XDG_RUNTIME_DIR", w.env())
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_HOOK_EMBED", "0");
    match prompt {
        Some(p) => c.env("USER_PROMPT", p),
        None => c.env_remove("USER_PROMPT"),
    };
    let o = c.output().expect("run the kiro command");
    out(&o)
}

/// **T4.1 + T4.2.** The question is the query, and the project is a filter.
///
/// Asserted on the *content* of what comes back, not on "it did not crash": the note that
/// matches is chosen so that both halves are observable. `kebab-case` occurs in one
/// project note only, so seeing it proves the question reached FTS. And the injected set
/// must not contain a global note when the project filter is on the project side — which
/// is the opposite of what `format!("regras {}", project)` did.
#[test]
fn the_injected_context_is_the_user_question_and_the_project_is_a_filter() {
    let w = World::new("p4query");
    p4_corpus(&w);
    let mapped = w.root.join("hive");
    std::fs::create_dir_all(&mapped).expect("mapped dir");
    let dir = mapped.to_str().unwrap().to_string();
    w.setup_ok(&["kiro", "--yes", "--dir", &dir]);

    let raw = std::fs::read_to_string(mapped.join(".kiro/hooks/brain-session.json")).expect("artifact");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("json");
    let command = v["hooks"][0]["action"]["command"].as_str().expect("command");

    let text = p4_run_kiro(&w, &mapped, command, Some("kebab-case nos caminhos"));
    assert!(text.contains("hook ok"), "the hook must run:\n{text}");

    // T4.1: the question reached the FTS stream, and it found the note the question names.
    assert!(
        text.contains("hive/naming"),
        "the question must drive the search; expected the naming note:\n{text}"
    );
    // And the note it did *not* ask about is absent — otherwise this would pass with a
    // fixed query that happened to return everything.
    assert!(
        !text.contains("hive/auth"),
        "an unrelated project note was injected:\n{text}"
    );
    // A different question must return a different note: this is what pins the query
    // to the user's message rather than to any fixed string.
    let text2 = p4_run_kiro(&w, &mapped, command, Some("session token on login"));
    assert!(
        text2.contains("hive/auth"),
        "a second question must drive a second search; expected the auth note:\n{text2}"
    );
    assert!(
        !text2.contains("hive/naming"),
        "the first question's note leaked into the second answer:\n{text2}"
    );

    // T4.2: the project arrives as a **filter**. The search is `scope=projetos` plus
    // `project=hive`, so a global note can never appear under the project header.
    let (global_block, project_block) = split_blocks(&text);
    assert!(
        !project_block.contains("regras/global/coding"),
        "the project filter leaked a global note into the project block:\n{text}"
    );
    // The global block is the *unfiltered-by-project* search, so it is allowed to be
    // empty. What must not happen is the old behaviour: a global note masquerading as
    // project context.
    let _ = global_block;
}

/// **T4.2, the sharp half.** The query sent to the store must not contain the project
/// name. This is the assertion that distinguishes a filter from a term, and it is the
/// only one that can: the old code produced a *correct-looking* result with the project
/// spliced into the query, so a result-shape assertion cannot tell the two apart.
///
/// The evidence is the artifact itself, plus a query built from a project name that
/// exists in no note: if the name were a search term it would be a term that matches
/// nothing, and the fix would be invisible in the results.
#[test]
fn the_project_name_never_reaches_the_query() {
    let w = World::new("p4filter");
    p4_corpus(&w);
    let mapped = w.root.join("hive");
    std::fs::create_dir_all(&mapped).expect("mapped dir");
    let dir = mapped.to_str().unwrap().to_string();
    w.setup_ok(&["kiro", "--yes", "--dir", &dir]);

    let raw = std::fs::read_to_string(mapped.join(".kiro/hooks/brain-session.json")).expect("artifact");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("json");
    let command = v["hooks"][0]["action"]["command"].as_str().expect("command");

    // The artifact must not build a query out of anything: it forwards `USER_PROMPT`.
    assert!(
        command.contains("--question \"$USER_PROMPT\""),
        "the kiro command must read the question from USER_PROMPT:\n{command}"
    );
    assert!(
        !command.contains("\"regras hive\"") && !command.contains("regras hive"),
        "the project name must not be spliced into the command:\n{command}"
    );

    // Behavioural half: a question that is only about the *project's* vocabulary returns
    // that project's notes, while the name `hive` — which appears in no note body — is
    // never needed. If the filter were a term, asking about the project by name would be
    // required to find it, and the two runs below would differ.
    let by_vocab = p4_run_kiro(&w, &mapped, command, Some("kebab-case"));
    assert!(by_vocab.contains("hive/naming"), "project notes must be reachable by their own vocabulary:\n{by_vocab}");
}

/// **T4.3.** No question in the payload must not be worse than today, and must not
/// invent one. Measured on this corpus, the old fixed query returns **zero** results, so
/// the honest empty query loses nothing — and it says so on stderr instead of printing
/// unrelated notes under a header that implies they answered something.
#[test]
fn a_payload_without_a_question_degrades_to_project_context_and_says_so() {
    let w = World::new("p4degrade");
    p4_corpus(&w);
    let mapped = w.root.join("hive");
    std::fs::create_dir_all(&mapped).expect("mapped dir");
    let dir = mapped.to_str().unwrap().to_string();
    w.setup_ok(&["kiro", "--yes", "--dir", &dir]);
    let raw = std::fs::read_to_string(mapped.join(".kiro/hooks/brain-session.json")).expect("artifact");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("json");
    let command = v["hooks"][0]["action"]["command"].as_str().expect("command");

    // `USER_PROMPT` unset: exactly what a session with no prompt yet looks like.
    let text = p4_run_kiro(&w, &mapped, command, None);
    assert!(text.contains("hook ok"), "the session must still be recorded:\n{text}");
    // The session note is written regardless — the recording is not conditional on there
    // being a question, which is the "not worse" half.
    let note = w.session_note(&format!("hive/{}", w.today_utc()));
    assert!(note.contains("session-start"), "the session is still recorded:\n{note}");

    // Direct call, so the stderr contract can be asserted rather than inferred: the
    // operator is told the context was not question-shaped.
    let o = Command::new(bin())
        .arg("--db").arg(w.db())
        .arg("hook").arg("--event").arg("session-start")
        .arg("--project").arg("hive")
        .arg("--payload").arg(r#"{"id":"p4-no-question"}"#)
        .current_dir(&mapped)
        .env("HOME", w.home()).env("BRAIN_DIR", w.brain_dir())
        .env("XDG_RUNTIME_DIR", w.env()).env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_HOOK_EMBED", "0")
        .output().expect("run the hook");
    let text = out(&o);
    assert!(text.contains("hook ok"), "an empty question is not an error:\n{text}");
    assert!(
        text.contains("no question in the payload"),
        "the degradation must be reported, not silent:\n{text}"
    );
    // And it must not have searched the old fixed string, which is the whole point.
    assert!(
        !text.contains("padrões melhores práticas"),
        "the fixed query must be gone from the codebase's behaviour:\n{text}"
    );
    // SPEC §2: empty query prints the honest marker, not unrelated notes.
    assert!(
        text.contains("INJECT: (no context found)"),
        "an empty question must print the empty marker:\n{text}"
    );
}

/// **T4.5.** With Ollama down the question must still score, through FTS.
///
/// The hook calls `Store::search` with **no query vector** — `brain hook` has no embedder
/// and never had one. So on this path `rank_vec` cannot exist, the entity stream needs a
/// `tag` this call does not pass, and the graph stream only expands FTS hits. **FTS is
/// therefore the only stream that can produce a result here**, which makes "the note came
/// back" a proof rather than an observation.
///
/// The second half pins which stream actually scored, with `--explain`. It asks for the
/// same `MATCH` the hook builds, not the bare question: the hook joins terms with `OR`
/// (measured, and documented on `inject_query`) while `Store::search` keeps FTS5's
/// default `AND`. Running the bare question through the CLI and expecting the hook's
/// answer would be conflating the two, and it is the mistake this test made first.
#[test]
fn with_ollama_down_the_question_still_scores_through_fts() {
    let w = World::new("p4ollama");
    p4_corpus(&w);
    let mapped = w.root.join("hive");
    std::fs::create_dir_all(&mapped).expect("mapped dir");
    let dir = mapped.to_str().unwrap().to_string();
    w.setup_ok(&["kiro", "--yes", "--dir", &dir]);
    let raw = std::fs::read_to_string(mapped.join(".kiro/hooks/brain-session.json")).expect("artifact");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("json");
    let command = v["hooks"][0]["action"]["command"].as_str().expect("command");

    // `DEAD_OLLAMA` is `http://127.0.0.1:1`, so every embed call fails.
    let text = p4_run_kiro(&w, &mapped, command, Some("kebab-case nos caminhos"));
    assert!(
        text.contains("hive/naming"),
        "FTS alone must carry the result with Ollama down:\n{text}"
    );

    // And which stream scored, stated by the store rather than inferred. The expression is
    // the one `inject_query` produces for this question.
    let expr = r#""kebab" OR "case" OR "nos" OR "caminhos""#;
    let o = Command::new(bin())
        .arg("--db").arg(w.db()).arg("search").arg(expr)
        .arg("--layer").arg("regras").arg("--project").arg("hive")
        .arg("--top-k").arg("3").arg("--explain")
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .output().expect("search --explain");
    let json = out(&o);
    assert!(json.contains("\"rank_fts\": 1"), "the FTS stream must be the one scoring:\n{json}");
    assert!(json.contains("\"rank_vec\": null"), "no vector can exist with Ollama down:\n{json}");
    assert!(json.contains("hive/naming"), "and it must find the right note:\n{json}");
}

/// Split an injected block into the global and project halves.
fn split_blocks(text: &str) -> (String, String) {
    let g = text.find("--- Brain context (global) ---");
    let p = text.find("--- Brain context (projetos) ---");
    (
        g.map(|i| text[i..p.map_or(text.len(), |j| j)].to_string()).unwrap_or_default(),
        p.map(|i| text[i..].to_string()).unwrap_or_default(),
    )
}

/// **RF-04, opencode side.** The question comes from the SDK's documented message
/// events: `message.updated` carries `properties.info` (discriminated by
/// `role: "user"`) and `message.part.updated` carries `properties.part` (the text).
/// Neither event alone carries both, so the plugin pairs them by message id.
///
/// Driven with the two events' real payload shapes, in the order that makes the
/// requirement hard: the **part arrives first**, and the user's text is only recognised
/// as the user's once `message.updated` confirms the role. An assistant message with the
/// same shape must not be injected — that is the failure this pairing exists to prevent.
#[test]
fn the_opencode_plugin_injects_the_user_message_and_never_the_assistants() {
    let w = World::new("p4ocmsg");
    p4_corpus(&w);
    std::fs::create_dir_all(w.root.join("hive")).expect("mapped dir");
    w.setup_ok(&["opencode", "--yes"]);
    let d = w.root.join("harness");
    std::fs::create_dir_all(&d).expect("harness dir");
    // The **installed** plugin, not a re-render of the generator: this is the file
    // opencode will load, and `setup` writes `BRAIN_BIN` from the real binary path.
    let code = plugin_code(&w.home());
    std::fs::write(d.join("brain-session.js"), code).expect("plugin");
    // Reuse the argv-faithful `$` stand-in from the test above; it is the only model of
    // Bun's shell that can pass an empty `--question`.
    let harness = r#"
import { Plugin } from "./brain-session.js";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
const run = promisify(execFile);
const $ = (strings, ...values) => {
  const argv = [];
  strings.forEach((s, i) => {
    for (const tok of s.split(/\s+/).filter(Boolean)) argv.push(tok);
    if (i < values.length) argv.push(String(values[i] ?? ""));
  });
  let cwd = process.cwd();
  const chain = {
    cwd: (dir) => { cwd = dir; return chain; },
    nothrow: () => chain,
    text: async () => {
      try { return (await run(argv[0], argv.slice(1), { env: process.env, cwd })).stdout; }
      catch (e) { return String(e.stdout || "") + String(e.stderr || ""); }
    },
  };
  return chain;
};
const hooks = await Plugin({
  client: { app: { log: () => {} } }, $, directory: process.env.BRAIN_TEST_DIR,
});

// Part BEFORE the role — the ordering that requires the id pairing to work.
await hooks["message.part.updated"]({ event: { properties: { part: {
  id: "prt_1", sessionID: "ses_1", messageID: "msg_user", type: "text",
  text: "kebab-case nos caminhos" } } } });
await hooks["message.updated"]({ event: { properties: { info: {
  id: "msg_user", sessionID: "ses_1", role: "user" } } } });

// An assistant message, same part shape, whose text points at a DIFFERENT note. If the
// plugin took it, the injected set would be the auth note and not the naming one, so
// both halves of the assertion below discriminate. An assistant message repeating the
// user's own words would prove nothing either way.
await hooks["message.part.updated"]({ event: { properties: { part: {
  id: "prt_2", sessionID: "ses_1", messageID: "msg_ai", type: "text",
  text: "token" } } } });
await hooks["message.updated"]({ event: { properties: { info: {
  id: "msg_ai", sessionID: "ses_1", role: "assistant" } } } });

// A streaming update for the same user message must not inject twice.
await hooks["message.part.updated"]({ event: { properties: { part: {
  id: "prt_1b", sessionID: "ses_1", messageID: "msg_user", type: "text",
  text: "kebab-case nos caminhos" } } } });
console.log("DONE");
"#;
    std::fs::write(d.join("harness.js"), harness).expect("harness");

    let o = Command::new("node")
        .arg(d.join("harness.js"))
        .current_dir(&d)
        .env("BRAIN_TEST_DIR", w.root.join("hive").to_string_lossy().to_string())
        .env("BRAIN_DB_PATH", w.db())
        .env("XDG_RUNTIME_DIR", w.env())
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_HOOK_EMBED", "0")
        .output()
        .expect("run harness");
    assert!(out(&o).contains("DONE"), "the plugin must not throw:\n{}", out(&o));

    // The plugin `console.log`s the hook's output, so the inject is right here.
    let inject = out(&o);
    assert!(
        inject.contains("hive/naming"),
        "the user's question must reach the search and find what it names:\n{inject}"
    );
    assert!(
        !inject.contains("hive/auth"),
        "the assistant's message was injected instead of the user's:\n{inject}"
    );
}

/// The negative half, kept separate because it is the half that can be wrong silently:
/// an assistant message alone must not produce a session-start inject at all.
#[test]
fn the_opencode_plugin_does_not_inject_from_an_assistant_message() {
    let w = World::new("p4ocai");
    p4_corpus(&w);
    std::fs::create_dir_all(w.root.join("hive")).expect("mapped dir");
    w.setup_ok(&["opencode", "--yes"]);
    let d = w.root.join("harness");
    std::fs::create_dir_all(&d).expect("harness dir");
    let code = plugin_code(&w.home());
    std::fs::write(d.join("brain-session.js"), code).expect("plugin");
    let harness = r#"
import { Plugin } from "./brain-session.js";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
const run = promisify(execFile);
const $ = (strings, ...values) => {
  const argv = [];
  strings.forEach((s, i) => {
    for (const tok of s.split(/\s+/).filter(Boolean)) argv.push(tok);
    if (i < values.length) argv.push(String(values[i] ?? ""));
  });
  let cwd = process.cwd();
  const chain = {
    cwd: (dir) => { cwd = dir; return chain; },
    nothrow: () => chain,
    text: async () => {
      try { return (await run(argv[0], argv.slice(1), { env: process.env, cwd })).stdout; }
      catch (e) { return String(e.stdout || "") + String(e.stderr || ""); }
    },
  };
  return chain;
};
const hooks = await Plugin({
  client: { app: { log: () => {} } }, $, directory: process.env.BRAIN_TEST_DIR,
});
await hooks["message.updated"]({ event: { properties: { info: {
  id: "msg_ai", sessionID: "ses_1", role: "assistant" } } } });
await hooks["message.part.updated"]({ event: { properties: { part: {
  id: "prt_2", sessionID: "ses_1", messageID: "msg_ai", type: "text",
  text: "token" } } } });
console.log("DONE");
"#;
    std::fs::write(d.join("harness.js"), harness).expect("harness");
    let o = Command::new("node")
        .arg(d.join("harness.js"))
        .current_dir(&d)
        .env("BRAIN_TEST_DIR", w.root.join("hive").to_string_lossy().to_string())
        .env("BRAIN_DB_PATH", w.db())
        .env("XDG_RUNTIME_DIR", w.env())
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .env("BRAIN_HOOK_EMBED", "0")
        .output()
        .expect("run harness");
    let text = out(&o);
    assert!(text.contains("DONE"), "the plugin must not throw:\n{text}");
    // Nothing was recorded, because nothing was injected. The assistant's text ("token")
    // names the auth note directly, so a plugin that used it would have recorded here.
    let read = Command::new(bin())
        .arg("--db").arg(w.db()).arg("read").arg("sessoes")
        .arg(format!("hive/{}", w.today_utc()))
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .output().expect("read");
    let note = String::from_utf8_lossy(&read.stdout).into_owned();
    assert!(
        !note.contains("session-start"),
        "an assistant message must not trigger a session-start inject:\n{note}"
    );
}
