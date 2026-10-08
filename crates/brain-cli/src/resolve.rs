//! The four-step cascade that answers "which brain project is this directory?"
//! (RF-02, ADR-06).
//!
//! The order is the requirement, not an implementation detail, and every step has a
//! measured reason to be where it is:
//!
//! 1. **`config.json`**, keyed by absolute path. First because both fallbacks below
//!    have measured false positives — three of the six directories on the owner's
//!    disk are not registered projects at all.
//! 2. **`git remote get-url origin`**, matched against a registered project name.
//!    Before the directory name because `atlas-ecm`, `atlasos` and `atlas-admin`
//!    coexist on that disk, and a directory name is a weaker signal than a remote.
//! 3. **Directory name, matched EXACTLY** against a registered project name.
//! 4. **Ask, once**.
//!
//! **Step 3 is exact and that is the load-bearing word.** A prefix match over those
//! three coexisting directories would send `atlas-admin` to `atlas-ecm`'s notes and
//! report success — a wrong answer with no error, which is the only kind of wrong
//! that survives to production.
//!
//! Two states never reach the question: `desabilitado` (the operator opted this
//! directory out entirely) and a recorded refusal. Re-asking is the failure the
//! cascade is designed around — a prompt that returns every session is a prompt the
//! operator stops reading, and then accepts whatever is first in the list.

use std::path::Path;

use crate::config::{self, Entry};

/// Where a resolved project name came from. Printed on every hook run, because a
/// name with no provenance is not diagnosable: "why did my session land in
/// `mobile`?" is the first question anybody asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Step 1: `config.json` had a project for this absolute path.
    Config,
    /// Step 2: the `origin` remote's repository name is a registered project.
    GitRemote,
    /// Step 3: the directory name is exactly a registered project name.
    DirName,
    /// Step 4: the operator answered, and the answer was recorded.
    Asked,
    /// Step 4 ran before and the operator declined. Recorded, so it does not run again.
    Recusado,
    /// `config.json` says this directory does not want brain.
    Disabled,
    /// `--project` was passed, so the cascade did not run at all.
    Explicit,
    /// The cascade exhausted and could not ask (no tty). Not persisted, deliberately:
    /// a CI run that recorded a refusal would silence a directory whose operator was
    /// never asked. See [`Origin::skip_reason`].
    Unresolved,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Config => "config",
            Origin::GitRemote => "git-remote",
            Origin::DirName => "dir-name",
            Origin::Asked => "asked",
            Origin::Recusado => "recusado",
            Origin::Disabled => "disabled",
            Origin::Explicit => "explicit",
            Origin::Unresolved => "unresolved",
        }
    }

    /// Why the hook must not write anything, or `None` when it should.
    ///
    /// **"Do not use this" is three different answers, and the difference is not
    /// cosmetic** — it decides whether the operator is asked again.
    ///
    /// | origin | wrote nothing? | config | asked again? |
    /// |---|---|---|---|
    /// | `desabilitado` | yes | opt-out recorded | never |
    /// | `recusado` | yes | refusal recorded | never |
    /// | `unresolved` | yes | **nothing** | **yes, next time** |
    ///
    /// The first two are answers a human gave, so they are final. The third is the
    /// absence of an answer — no tty, a CI job, an IDE piping the hook — and it must
    /// not be laundered into the second, because a single non-interactive run would
    /// then permanently silence a directory the operator has never been asked about.
    ///
    /// It still writes no note. That part is *not* symmetric with "asked again": a
    /// CI run that recorded a session note under a directory name nobody chose would
    /// accumulate exactly the kind of noise that made a refusal feel like a bug in the
    /// first place, once per pipeline run.
    pub fn skip_reason(self) -> Option<&'static str> {
        match self {
            Origin::Disabled => Some("is marked desabilitado in config.json"),
            Origin::Recusado => Some("was declined for this directory in config.json"),
            Origin::Unresolved => Some("has no project and nobody could be asked"),
            _ => None,
        }
    }
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The answer: a project name (possibly none) and how it was arrived at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub project: Option<String>,
    pub origin: Origin,
}



/// The project names the cascade is allowed to resolve to.
///
/// A slice of names, not a `&Store`. The resolver needs nothing else from the
/// database, and taking the list keeps every branch of the cascade testable without
/// a schema — which is the only reason the step-3 exactness rule has a test at all.
pub type Known<'a> = &'a [String];

/// What the operator said, or the fact that nobody could say anything.
///
/// Three cases, not two, and the third is the point: "I decline" and "there is no
/// one to ask" must not collapse. A decline is an answer and is recorded, so the
/// question is not repeated. The absence of a terminal is **not** an answer — it is
/// what every IDE and every CI run looks like — and recording it would permanently
/// silence a directory whose operator has never been asked anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// A project name. Recorded, so the question is not asked again.
    ///
    /// Produced by `brain setup`'s interactive prompt (`StdioAsker`), not by the
    /// hook: an IDE gives a hook no terminal, so the hook always answers `CannotAsk`
    /// (RF-07.3). This enum is the shared vocabulary of both — the hook's failure to
    /// ask and the setup's success at asking are the same two cases the cascade has to
    /// tell apart.
    Accepted(String),
    /// The operator said no. Recorded as a refusal, so the question is not asked again.
    Declined,
    /// There is nobody to ask. Nothing is recorded.
    CannotAsk,
}

/// How the operator is asked, when step 4 is reached.
///
/// Injected rather than read from stdin inside the resolver, for two reasons: the
/// cascade becomes testable without a pty, and "cannot ask" (no tty — an IDE piping
/// the hook, a CI job) becomes an explicit return value instead of a hang. Hanging
/// on a non-tty is the worst available outcome for something an IDE calls on every
/// event.
pub trait Prompt {
    /// Called **at most once** per resolution — the resolver does not loop, so a
    /// prompt that keeps asking is this trait's implementation's bug, not the
    /// cascade's.
    fn ask(&self, candidates: &[String]) -> Answer;
}

/// What the resolver needs beyond the directory itself. Grouped so the tests can
/// hand in a scripted git and a scripted prompt.
pub struct Env<'a> {
    /// Registered project names.
    pub known: Known<'a>,
    /// The `origin` remote URL, or `None` when git is absent, this is not a repo,
    /// or there is no remote. The caller runs git; the resolver does not, so the
    /// cascade has no subprocess and cannot hang on a credential prompt.
    pub git_remote: Option<String>,
    /// The operator, when the environment can ask.
    pub prompt: &'a dyn Prompt,
    /// Where to record a decision (step 4). Injectable so the cascade's write path
    /// can be observed without a filesystem.
    pub record: &'a dyn Fn(&Path, &Entry) -> anyhow::Result<()>,
    /// The `BRAIN_DIR` to read step 1 from, or `None` when there is no usable one.
    ///
    /// An argument rather than read from the environment inside, for the reason
    /// `config::load_from` documents: the environment is process-global and setting it
    /// from a test is a race.
    ///
    /// **`None` is a normal runtime state, not a test affordance** (RF-07.1): a
    /// container with no `HOME` and a read-only `BRAIN_DIR` both land here, and a
    /// hook that fails on them breaks the IDE. So `None` skips step 1 and turns
    /// `record` into a no-op — the cascade still runs on the remote and the directory
    /// name, it just cannot remember anything it decides.
    pub config_dir: Option<&'a Path>,
}

/// The repository name of a remote URL, or `None` if there is nothing usable.
///
/// Handles both shapes that are actually in the wild — `git@host:org/repo.git` (SCP
/// syntax) and `https://host/org/repo.git` — because matching only one of them
/// means the cascade silently skips step 2 for half the world's repositories, and a
/// skipped step looks exactly like a step that found nothing.
pub fn repo_name_from_remote(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    // Drop query/fragment before anything else: a remote with a token in it is
    // common, and `?` appears in such URLs.
    let url = url.split(['?', '#']).next().unwrap_or(url);
    let last = url.rsplit('/').next().unwrap_or("");
    // SCP syntax has no `/` after the host in the usual form, so the `:` split is
    // what separates `org/repo.git` there.
    let last = last.rsplit(':').next().unwrap_or("");
    let name = last.strip_suffix(".git").unwrap_or(last);
    let name = name.trim();
    if name.is_empty() { None } else { Some(name.to_string()) }
}

/// Run the cascade. The whole of RF-02 in one function, in order, once.
pub fn resolve(dir: &Path, env: &Env) -> Resolution {
    let abs = config::absolute_key(dir);

    // --- step 1: config.json ----------------------------------------------------
    // `desabilitado` is checked *inside* step 1 and before `projeto`, because it is
    // the stronger statement: "no brain here" outranks "brain, under this project".
    //
    // A broken config warns and falls through rather than stopping: the whole reason
    // `save_entry` refuses to write over one (T1.2) is that everything else still
    // works without it.
    //
    // No usable `config_dir` (RF-07.1) is the same kind of "go on without it" — the
    // cascade still answers from the remote and the directory name.
    let config = match env.config_dir {
        Some(root) => config::load_or_warn_from(root),
        None => {
            eprintln!("hook: no usable BRAIN_DIR, so there is no config to consult or record");
            (config::Config::new(), None)
        }
    };
    let (config, broken) = config;
    if let Some(why) = broken {
        eprintln!("hook: ignoring config.json for now — {why}");
    }
    if let Some(entry) = config.get(abs.to_string_lossy().as_ref()) {
        if entry.desabilitado {
            return Resolution { project: None, origin: Origin::Disabled };
        }
        match (&entry.projeto, entry.motivo.as_deref()) {
            (Some(p), _) if !p.trim().is_empty() => {
                return Resolution { project: Some(p.clone()), origin: Origin::Config };
            }
            // Only a **conised** refusal is one (RF-03.1). An entry with no project
            // and no recognised reason falls through to the rest of the cascade and
            // records nothing.
            //
            // That is the whole point of the `motivo` check. `parse_entry` defaults
            // every absent key, so a typo in the key — `{"projet":"hive"}` — parses to
            // `projeto: None`, and treating that as a refusal would permanently
            // silence a directory whose operator was never asked anything. The next
            // `save_entry` would then rewrite the file and erase the typo, so the
            // evidence of the mistake would be gone too.
            (None, Some(m)) if m == config::RECUSADO => {
                return Resolution { project: None, origin: Origin::Recusado };
            }
            _ => {}
        }
    }

    // --- step 2: git remote -----------------------------------------------------
    if let Some(remote) = env.git_remote.as_deref() {
        if let Some(name) = repo_name_from_remote(remote) {
            if let Some(hit) = exact(env.known, &name) {
                return Resolution { project: Some(hit), origin: Origin::GitRemote };
            }
        }
    }

    // --- step 3: directory name, EXACTLY ----------------------------------------
    let base = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(hit) = exact(env.known, &base) {
        return Resolution { project: Some(hit), origin: Origin::DirName };
    }

    // --- step 4: ask, once ------------------------------------------------------
    // `record` becomes a no-op without a config: there is nowhere to record to, and
    // inventing a fallback path would be worse than forgetting the answer. The run
    // still resolves, and the next run with a usable `BRAIN_DIR` asks again.
    let record = |dir: &Path, entry: &Entry| -> anyhow::Result<()> {
        if env.config_dir.is_none() {
            return Ok(());
        }
        (env.record)(dir, entry)
    };

    match env.prompt.ask(env.known) {
        Answer::Accepted(answer) if !answer.trim().is_empty() => {
            let name = answer.trim().to_string();
            if let Err(e) = record(&abs, &Entry::project(name.clone())) {
                // The answer is still honoured for this run. Refusing to record it
                // would make a working directory ask again on the next event, which
                // is a worse outcome than a config that lags behind reality.
                eprintln!("hook: could not record {name} for {} — {e}", abs.display());
            }
            Resolution { project: Some(name), origin: Origin::Asked }
        }
        Answer::Accepted(_) => {
            // A blank answer is a decline, not a project named "".
            if let Err(e) = record(&abs, &Entry::declined()) {
                eprintln!("hook: could not record the refusal for {} — {e}", abs.display());
            }
            Resolution { project: None, origin: Origin::Recusado }
        }
        Answer::Declined => {
            if let Err(e) = record(&abs, &Entry::declined()) {
                eprintln!("hook: could not record the refusal for {} — {e}", abs.display());
            }
            Resolution { project: None, origin: Origin::Recusado }
        }
        // Nobody could answer. **Nothing is written** — see `Answer::CannotAsk`.
        Answer::CannotAsk => Resolution { project: None, origin: Origin::Unresolved },
    }
}

/// Exact, case-sensitive membership. See the module docs: prefix is the bug.
fn exact(known: &[String], name: &str) -> Option<String> {
    known.iter().find(|k| k.as_str() == name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;
    use std::path::PathBuf;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// A `BRAIN_DIR` of its own, handed to the cascade as an argument. Nothing here
    /// touches the process environment — `set_var` is `unsafe` in edition 2024 and
    /// global, so exporting it would race every other test in the binary.
    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("brain-res-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("scratch");
        d
    }

    /// A working directory whose **basename is exactly `name`**, because step 3 reads
    /// the basename and that is the whole point of the test.
    fn workdir(name: &str) -> PathBuf {
        let parent = std::env::temp_dir().join(format!("brain-res-wd-{}", std::process::id()));
        std::fs::create_dir_all(&parent).expect("workdir parent");
        let d = parent.join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("workdir");
        d
    }

    /// Write a config by hand, which is the shape an operator's file has.
    fn write_config(root: &Path, entries: serde_json::Value) {
        std::fs::create_dir_all(root).expect("root");
        std::fs::write(
            root.join("config.json"),
            serde_json::to_string_pretty(&json!({ "brain_projects": entries })).unwrap(),
        )
        .expect("write config");
    }

    struct Scripted {
        answer: Answer,
        asked: RefCell<usize>,
    }

    impl Scripted {
        fn accepts(answer: &str) -> Self {
            Self { answer: Answer::Accepted(answer.into()), asked: RefCell::new(0) }
        }
        fn declines() -> Self {
            Self { answer: Answer::Declined, asked: RefCell::new(0) }
        }
        fn cannot_ask() -> Self {
            Self { answer: Answer::CannotAsk, asked: RefCell::new(0) }
        }
    }

    impl Prompt for Scripted {
        fn ask(&self, _candidates: &[String]) -> Answer {
            *self.asked.borrow_mut() += 1;
            self.answer.clone()
        }
    }

    /// Panics if consulted — for every case that must not reach the question.
    struct NeverAsks;
    impl Prompt for NeverAsks {
        fn ask(&self, _candidates: &[String]) -> Answer {
            panic!("the cascade asked, and this case must not reach the question")
        }
    }

    // --- remote URL shapes ------------------------------------------------------

    /// Both real shapes, because matching only one of them makes step 2 silently
    /// skip half the repositories in the world, and a skipped step is
    /// indistinguishable from one that found nothing.
    #[test]
    fn both_remote_url_shapes_yield_the_repository_name() {
        for (url, want) in [
            ("git@github.com:org/atlas-ecm.git", "atlas-ecm"),
            ("https://github.com/org/atlas-ecm.git", "atlas-ecm"),
            ("https://github.com/org/atlas-ecm", "atlas-ecm"),
            ("ssh://git@gitlab.internal:2222/group/hive.git", "hive"),
            ("https://host/org/repo.git?token=abc#frag", "repo"),
        ] {
            assert_eq!(repo_name_from_remote(url).as_deref(), Some(want), "url: {url}");
        }
        assert_eq!(repo_name_from_remote("   "), None);
        assert_eq!(repo_name_from_remote("https://host/"), None);
    }

    // --- step 1 ------------------------------------------------------------------

    /// T2.1 — config wins over both fallbacks. Both are set up to match, so a
    /// cascade that consulted them first would answer something else and fail here.
    #[test]
    fn config_beats_both_git_and_the_directory_name() {
        let root = scratch("step1");
        // Named exactly `hive`, and carrying a remote for `hive` too: if either
        // fallback ran, the answer would be "hive" and not "from-config".
        let wd = workdir("hive");
        write_config(
            &root,
            json!({ config::absolute_key(&wd).to_string_lossy(): { "projeto": "from-config" } }),
        );

        let known = names(&["hive"]);
        let res = resolve(
            &wd,
            &Env {
                known: &known,
                git_remote: Some("git@github.com:org/hive.git".into()),
                prompt: &NeverAsks,
                record: &|_, _| Ok(()),
                config_dir: Some(&root),
            },
        );
        assert_eq!(res.project.as_deref(), Some("from-config"));
        assert_eq!(res.origin, Origin::Config);
    }

    // --- step 2 ------------------------------------------------------------------

    /// T2.2 — the remote's repository name matches a registered project.
    #[test]
    fn a_matching_git_remote_resolves_at_step_two() {
        let root = scratch("step2");
        // The directory is named after nothing registered, so step 3 cannot answer.
        let wd = workdir("step2");
        let known = names(&["atlas-ecm", "mobile-erp"]);
        let res = resolve(
            &wd,
            &Env {
                known: &known,
                git_remote: Some("git@github.com:acme/mobile-erp.git".into()),
                prompt: &NeverAsks,
                record: &|_, _| Ok(()),
                config_dir: Some(&root),
            },
        );
        assert_eq!(res.project.as_deref(), Some("mobile-erp"));
        assert_eq!(res.origin, Origin::GitRemote);
    }

    /// A remote that names something unregistered must not resolve — otherwise step 2
    /// invents projects, and the "known" list stops meaning anything.
    #[test]
    fn a_remote_naming_an_unregistered_project_does_not_resolve() {
        let root = scratch("step2b");
        let wd = workdir("step2b");
        let known = names(&["atlas-ecm"]);
        let prompt = Scripted::declines();
        let res = resolve(
            &wd,
            &Env {
                known: &known,
                git_remote: Some("git@github.com:acme/some-fork.git".into()),
                prompt: &prompt,
                record: &|_, _| Ok(()),
                config_dir: Some(&root),
            },
        );
        assert_eq!(res.origin, Origin::Recusado, "an unknown remote must fall through, not match");
        assert_eq!(res.project, None);
    }

    // --- step 3 ------------------------------------------------------------------

    /// T2.3 — **exact** match only, and the case is the measured one.
    ///
    /// The negative half is the test. A prefix match answers `Some("atlas-ecm")` for
    /// `atlas-admin` — a wrong project with no error — so it is asserted absent, not
    /// merely unasserted.
    #[test]
    fn a_directory_name_matches_only_exactly_never_by_prefix() {
        let root = scratch("step3");
        let known = names(&["atlas-ecm"]);

        // The exact name resolves.
        let exact_dir = workdir("atlas-ecm");
        let res = resolve(
            &exact_dir,
            &Env { known: &known, git_remote: None, prompt: &NeverAsks, record: &|_, _| Ok(()), config_dir: Some(&root) },
        );
        assert_eq!(res.project.as_deref(), Some("atlas-ecm"), "an exact name must match");
        assert_eq!(res.origin, Origin::DirName);

        // A directory that merely *starts with* a registered name does not. These are
        // the neighbours that coexist on the owner's disk.
        for neighbour in ["atlasos", "atlas-admin", "atlas", "atlas-ecm-backend"] {
            let d = workdir(neighbour);
            let prompt = Scripted::declines();
            let res = resolve(
                &d,
                &Env { known: &known, git_remote: None, prompt: &prompt, record: &|_, _| Ok(()), config_dir: Some(&root) },
            );
            assert_ne!(
                res.project.as_deref(),
                Some("atlas-ecm"),
                "`{neighbour}` must NOT resolve to atlas-ecm — a prefix match is a wrong \
                 project with no error"
            );
        }
    }

    // --- step 4 ------------------------------------------------------------------

    /// T2.4 — nothing matched, so it asks once, and an acceptance is recorded *on
    /// disk*, so run two can be shown not to ask again.
    #[test]
    fn an_accepted_answer_is_recorded_on_disk_and_used() {
        let root = scratch("step4");
        let wd = workdir("step4");
        let known = names(&["atlas-ecm", "hive"]);
        let prompt = Scripted::accepts("hive");
        let cfg = root.clone();

        let res = resolve(
            &wd,
            &Env {
                known: &known,
                git_remote: None,
                prompt: &prompt,
                record: &move |dir, e| config::save_entry_in(&cfg, dir, e),
                config_dir: Some(&root),
            },
        );
        assert_eq!(res.project.as_deref(), Some("hive"));
        assert_eq!(res.origin, Origin::Asked);
        assert_eq!(*prompt.asked.borrow(), 1, "the question must be asked exactly once");
        let (stored, warn) = config::load_or_warn_from(&root);
        assert!(warn.is_none(), "{warn:?}");
        assert_eq!(
            stored.get(config::absolute_key(&wd).to_string_lossy().as_ref()),
            Some(&Entry::project("hive")),
            "the acceptance must be on disk, under the absolute path"
        );
    }

    /// T2.5 — a refusal is recorded as "no project, on purpose" and does not ask
    /// again. The second resolution is the real assertion: the first could pass
    /// simply by having written a project.
    #[test]
    fn a_refusal_is_recorded_and_the_question_is_not_repeated() {
        let root = scratch("refused");
        let wd = workdir("refused");
        let known = names(&["atlas-ecm"]);
        let cfg = root.clone();

        let first = Scripted::declines();
        let res = resolve(
            &wd,
            &Env {
                known: &known,
                git_remote: None,
                prompt: &first,
                record: &move |dir, e| config::save_entry_in(&cfg, dir, e),
                config_dir: Some(&root),
            },
        );
        assert_eq!(res.project, None);
        assert_eq!(res.origin, Origin::Recusado);
        assert_eq!(*first.asked.borrow(), 1);

        // Second run: same directory, a prompt that *panics* if consulted.
        let res2 = resolve(
            &wd,
            &Env { known: &known, git_remote: None, prompt: &NeverAsks, record: &|_, _| Ok(()), config_dir: Some(&root) },
        );
        assert_eq!(res2.project, None);
        assert_eq!(res2.origin, Origin::Recusado, "a recorded refusal must not ask again");
    }

    /// The distinction that makes an IDE usable: **nobody could answer** is not
    /// **nobody answered**.
    ///
    /// Every IDE invocation and every CI run reaches this branch. If it recorded a
    /// refusal, the first non-interactive run would permanently silence a directory
    /// whose operator has still never been asked anything, and the only way out would
    /// be hand-editing the config.
    #[test]
    fn an_unanswerable_question_records_nothing_and_asks_again_next_time() {
        let root = scratch("unanswerable");
        let wd = workdir("unanswerable");
        let known = names(&["atlas-ecm"]);
        let written = RefCell::new(Vec::new());
        let record = |_: &Path, e: &Entry| {
            written.borrow_mut().push(e.clone());
            Ok(())
        };

        let first = Scripted::cannot_ask();
        let res = resolve(
            &wd,
            &Env { known: &known, git_remote: None, prompt: &first, record: &record, config_dir: Some(&root) },
        );
        assert_eq!(res.origin, Origin::Unresolved);
        assert_eq!(res.project, None, "nothing resolved, so no project is claimed");
        assert!(
            written.borrow().is_empty(),
            "a run with nobody to ask must not write a refusal — that would silence \
             the directory for an operator who was never asked"
        );

        // And the directory is therefore still askable.
        let second = Scripted::accepts("hive");
        let res2 = resolve(
            &wd,
            &Env { known: &known, git_remote: None, prompt: &second, record: &record, config_dir: Some(&root) },
        );
        assert_eq!(res2.origin, Origin::Asked, "the question must still be asked");
        assert_eq!(res2.project.as_deref(), Some("hive"));
    }

    // --- the two states that never reach the question ---------------------------

    /// T3.3 — `desabilitado` outranks a project, and the cascade never looks at the
    /// remote or the directory name. Both are set up to match, so a cascade that
    /// consulted a fallback would answer with a project.
    #[test]
    fn a_disabled_directory_never_enters_the_cascade() {
        let root = scratch("disabled");
        let wd = workdir("atlas-ecm");
        write_config(
            &root,
            json!({ config::absolute_key(&wd).to_string_lossy(): { "desabilitado": true } }),
        );
        let known = names(&["atlas-ecm"]);
        let res = resolve(
            &wd,
            &Env {
                known: &known,
                git_remote: Some("git@github.com:acme/atlas-ecm.git".into()),
                prompt: &NeverAsks,
                record: &|_, _| Ok(()),
                config_dir: Some(&root),
            },
        );
        assert_eq!(res.project, None);
        assert_eq!(res.origin, Origin::Disabled);
    }

    /// `desabilitado` outranks a *project* in the same entry — the operator saying
    /// "no brain here" is a stronger statement than "brain, under this project", and
    /// reading `projeto` first would quietly re-enable a directory.
    #[test]
    fn disabled_outranks_a_project_in_the_same_entry() {
        let root = scratch("disabled2");
        let wd = workdir("disabled2");
        write_config(
            &root,
            json!({ config::absolute_key(&wd).to_string_lossy():
                { "projeto": "atlas-ecm", "desabilitado": true } }),
        );
        let known = names(&["atlas-ecm"]);
        let res = resolve(
            &wd,
            &Env { known: &known, git_remote: None, prompt: &NeverAsks, record: &|_, _| Ok(()), config_dir: Some(&root) },
        );
        assert_eq!(res.origin, Origin::Disabled);
        assert_eq!(res.project, None);
    }

    /// A refusal recorded by a *previous install* — read from the real file, not
    /// injected — is honoured the same way. This is the shape that actually occurs:
    /// the config outlives the process that asked.
    #[test]
    fn a_refusal_read_from_the_file_is_honoured_without_asking() {
        let root = scratch("filed");
        let wd = workdir("filed");
        config::save_entry_in(&root, &wd, &Entry::declined()).expect("record refusal");
        assert!(root.join("config.json").exists(), "T1.3 must have created the file");
        let known = names(&["hive"]);
        let res = resolve(
            &wd,
            &Env {
                known: &known,
                git_remote: Some("git@host:hive.git".into()),
                prompt: &NeverAsks,
                record: &|_, _| Ok(()),
                config_dir: Some(&root),
            },
        );
        assert_eq!(res.origin, Origin::Recusado, "the recorded refusal outranks the remote");
        assert_eq!(res.project, None);
    }

    /// A broken config must not stop the cascade: the point of refusing to overwrite
    /// it (T1.2) is that everything else still works without it.
    #[test]
    fn a_broken_config_does_not_stop_the_cascade() {
        let root = scratch("broken");
        let wd = workdir("broken");
        std::fs::write(root.join("config.json"), "{ nope").expect("write corrupt");
        let known = names(&["hive"]);
        let prompt = Scripted::accepts("hive");
        let res = resolve(
            &wd,
            &Env { known: &known, git_remote: None, prompt: &prompt, record: &|_, _| Ok(()), config_dir: Some(&root) },
        );
        assert_eq!(res.project.as_deref(), Some("hive"), "the cascade must still work");
        assert_eq!(res.origin, Origin::Asked);
    }

    /// The three "do not use this here" origins, and the one that must be asked again.
    ///
    /// The asymmetry is the requirement, so it is asserted rather than described: a
    /// test that only checked "they all skip" would pass on an implementation that
    /// also made `unresolved` permanent.
    #[test]
    fn the_three_negative_origins_all_suppress_capture_but_only_two_are_final() {
        let r = |o| Resolution { project: None, origin: o };
        for o in [Origin::Disabled, Origin::Recusado, Origin::Unresolved] {
            assert!(o.skip_reason().is_some(), "{o} must not write anything");
        }
        // A resolved project is never suppressed.
        for o in [Origin::Config, Origin::GitRemote, Origin::DirName, Origin::Asked, Origin::Explicit] {
            assert!(o.skip_reason().is_none(), "{o} has a project and must write");
        }
        assert_eq!(r(Origin::Recusado).origin.as_str(), "recusado");
        // And the recorded decision differs: only `Answer::Declined` writes a refusal.
        // `&dyn Fn`, so the recorder has to be shared immutably — hence the RefCell.
        let written: RefCell<Vec<Entry>> = RefCell::new(Vec::new());
        let record = |_: &Path, e: &Entry| {
            written.borrow_mut().push(e.clone());
            Ok(())
        };
        let wd = workdir("persist");
        let root = scratch("persist");
        resolve(
            &wd,
            &Env {
                known: &names(&["atlas-ecm"]),
                git_remote: None,
                prompt: &Scripted::declines(),
                record: &record,
                config_dir: Some(&root),
            },
        );
        assert_eq!(*written.borrow(), vec![Entry::declined()], "a refusal is persisted");

        written.borrow_mut().clear();
        resolve(
            &wd,
            &Env {
                known: &names(&["atlas-ecm"]),
                git_remote: None,
                prompt: &Scripted::cannot_ask(),
                record: &record,
                config_dir: Some(&root),
            },
        );
        assert!(
            written.borrow().is_empty(),
            "`unresolved` must not persist anything — it would make a non-interactive \
             run permanent for a directory the operator was never asked about"
        );
    }

}
