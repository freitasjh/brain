//! TD-F4-A: `brain store --project <nome>` with an unknown project name warns
//! on stderr but still creates the project (exit 0).
//!
//! The warning is **stderr-only** so scripts parsing stdout keep working, and
//! the exit stays 0 so no caller breaks. The property under test is which
//! stream carries the message, not just "it did not crash".
//!
//! `BRAIN_OLLAMA_URL` points at a dead port, never removed: removing it
//! selects the real default (`brain-embed:23`), shared by every concurrent
//! test binary. The CLI embeds inline, so a live URL would make the test
//! about model latency instead of about the warning.

use std::path::PathBuf;
use std::process::Command;

const DEAD_OLLAMA: &str = "http://127.0.0.1:1";

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_brain"))
}

fn scratch(tag: &str) -> PathBuf {
    let db = std::env::temp_dir().join(format!(
        "brain-storewarn-{}-{tag}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&db);
    db
}

fn store(db: &PathBuf, path: &str, project: &str) -> std::process::Output {
    Command::new(bin())
        .arg("--db")
        .arg(db)
        .arg("store")
        .arg("regras")
        .arg(path)
        .arg("## Warn\n\nCorpus for the project warning.")
        .arg("--scope")
        .arg("global")
        .arg("--project")
        .arg(project)
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .output()
        .expect("spawn brain store")
}

/// TD-F4-A: unknown project → exit 0, warning on stderr, project created.
#[test]
fn store_with_new_project_warns_on_stderr_but_succeeds() {
    let db = scratch("new");
    let o = store(&db, "warn/naming", "warnproj-new");
    assert!(
        o.status.success(),
        "store must stay exit 0, got {}: stdout={} stderr={}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(
        stderr.contains("warnproj-new") && stderr.contains("project_list"),
        "stderr must name the new project and point at project_list, got: {stderr}"
    );
    // The project was still created: no warning on the second store.
    let o2 = store(&db, "warn/naming2", "warnproj-new");
    assert!(
        o2.status.success(),
        "second store: {}",
        String::from_utf8_lossy(&o2.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&o2.stderr).contains("novo"),
        "existing project must not warn: {}",
        String::from_utf8_lossy(&o2.stderr)
    );
    // And the project really exists.
    let list = Command::new(bin())
        .arg("--db")
        .arg(&db)
        .arg("project")
        .arg("list")
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .output()
        .expect("project list");
    assert!(
        String::from_utf8_lossy(&list.stdout).contains("warnproj-new"),
        "project_list must show the created project: {}",
        String::from_utf8_lossy(&list.stdout)
    );
}

/// TD-F4-A: pre-created project → exit 0, no warning at all.
#[test]
fn store_with_existing_project_stays_silent() {
    let db = scratch("existing");
    let c = Command::new(bin())
        .arg("--db")
        .arg(&db)
        .arg("project")
        .arg("create")
        .arg("warnproj-old")
        .env("BRAIN_OLLAMA_URL", DEAD_OLLAMA)
        .output()
        .expect("project create");
    assert!(c.status.success(), "setup: {}", String::from_utf8_lossy(&c.stderr));
    let o = store(&db, "warn/other", "warnproj-old");
    assert!(
        o.status.success(),
        "store: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&o.stderr).contains("novo"),
        "no warning for an existing project: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}
