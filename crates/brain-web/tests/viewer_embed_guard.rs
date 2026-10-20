//! Regression guard: the viewer HTML must be embedded, never read from disk at
//! runtime by a cwd-relative path.
//!
//! # Why a source scan and not a behavioural test
//!
//! `tests/viewer_index_cwd.rs` already asserts the *behaviour* — `/` answers 200
//! `text/html` with the process cwd moved to a directory that has no `viewer/`.
//! That test covers the path it exercises. This guard covers the *shape* of the
//! code in every file of `brain-web/src/`, which is what the bug actually was:
//! `tokio::fs::read_to_string("viewer/index.html")` in `serve_index`, resolved
//! against the process cwd. Because `cargo install` ships the binary alone and the
//! systemd unit `brain setup systemd` writes has no `WorkingDirectory`, `/` was
//! `404 viewer/index.html not found` on every real install while `/api/*` — whose
//! routes were always absolute — kept working. The store's own precedent for this
//! device is `crates/brain-store/tests/zero_vector_guard.rs`.
//!
//! Three properties, each with the mutation that breaks it:
//!
//! 1. **The embed exists.** Delete `include_str!("../../../viewer/index.html")`
//!    → `the_embed_is_in_the_source`. Without it there is nothing to serve.
//! 2. **No production line touches a path.** Restore
//!    `tokio::fs::read_to_string("viewer/index.html").await`, or re-add
//!    `use tower_http::services::ServeDir;` with a `.fallback_service(...)` →
//!    `no_production_code_resolves_a_path`. Both spellings matter: the reader is
//!    the cwd bug, the directory service is what served `viewer/server.py`
//!    (legacy Python viewer, 10_960 B) as `200 text/plain` on the production
//!    bind. See `router`.
//! 3. **The embedded constant is actually delivered.** Define it and stop using
//!    it in a handler → `the_embedded_index_is_delivered`. A constant nobody
//!    serves is the bug with extra steps.
//!
//! This is a source scan, so what it proves is a *shape*. What it does not prove:
//! other path-based reads (`std::fs::read`, `File::open`) and an absolute-path
//! `ServeFile`, neither of which appears here. The behavioural half of properties
//! 2 and 3 is `tests/viewer_index_cwd.rs` (`/` and a deep route from a cwd with no
//! `viewer/index.html`, and a stray file inside `viewer/` arriving as the SPA
//! document rather than as itself).

use std::path::{Path, PathBuf};

fn crate_src() -> PathBuf {
    // CARGO_MANIFEST_DIR is `<root>/crates/brain-web`.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Every `.rs` file under `brain-web/src/`, with the test module cut off.
///
/// Every crate in this workspace puts `#[cfg(test)]` last, so "before the last
/// marker" is a sound proxy for production code. A file with no marker is treated
/// as entirely production, which is the conservative direction.
fn production_sources() -> Vec<(PathBuf, String)> {
    let mut paths = Vec::new();
    rust_sources(&crate_src(), &mut paths);
    assert!(
        !paths.is_empty(),
        "no .rs sources found under {} — the guard would pass vacuously",
        crate_src().display()
    );
    paths
        .into_iter()
        .map(|p| {
            let src = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
            let cut = src.rfind("#[cfg(test)]").map(|i| src[..i].to_string()).unwrap_or(src);
            (p, cut)
        })
        .collect()
}

#[test]
fn the_embed_is_in_the_source() {
    let embed = r#"include_str!("../../../viewer/index.html")"#;
    let sources = production_sources();
    let found: Vec<&PathBuf> = sources
        .iter()
        .filter(|(_, src)| src.contains(embed))
        .map(|(p, _)| p)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "exactly one production site must embed viewer/index.html with `{embed}`, found {found:?}"
    );
}

#[test]
fn no_production_code_resolves_a_path() {
    // Region-based, not pair-based. The previous version required
    // `read_to_string` *and* `viewer` on the same line, so
    // `Path::new("viewer").join("index.html")` or `fs::read_to_string(&p)`
    // passed silently — while a comment above it claimed to cover "a `&str`
    // variable holding the path". The claim is only true of this version: no
    // production line in this crate resolves a path to the viewer, whatever it
    // names it. `production_sources` is the region of production code, so the
    // read in this crate's own test module is out of scope, correctly.
    //
    // `ServeDir` is here for the other half of the same property: a directory
    // service rooted at the cwd is a path resolution too, and it is what made
    // `viewer/server.py` reachable over HTTP.
    const BANNED: [&str; 2] = ["read_to_string", "ServeDir"];
    let mut violations = Vec::new();
    for (path, src) in production_sources() {
        for (n, line) in src.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for token in BANNED {
                if code.contains(token) {
                    violations.push(format!(
                        "{}:{}: `{}` — production code must resolve no path to the viewer \
                         (the viewer comes from the embedded constant, and the process cwd is \
                         not a cache); `viewer/` holds files that must not be reachable over \
                         HTTP (see VIEWER_INDEX and `router`)",
                        path.display(),
                        n + 1,
                        code.trim()
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "the viewer is being resolved from a path again:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_embedded_index_is_delivered() {
    // Two references: the definition and at least one use. A constant that exists
    // but is never delivered is the same user-visible bug.
    //
    // Comments are stripped before counting — deliberately, and because the first
    // version of this test did *not* strip them and therefore passed while
    // `serve_index` returned `Html("")`: the doc comment on that very function
    // links `[`VIEWER_INDEX`]`, which was enough to make the count look healthy.
    // A count that a comment can satisfy is a count that reports the opposite of
    // the truth.
    let mut seen = 0usize;
    let mut sites = Vec::new();
    for (path, src) in production_sources() {
        let n: usize = src
            .lines()
            .map(|l| l.split("//").next().unwrap_or("").matches("VIEWER_INDEX").count())
            .sum();
        if n > 0 {
            seen += n;
            sites.push(format!("{}: {n}", path.display()));
        }
    }
    assert!(
        seen >= 2,
        "VIEWER_INDEX must be defined *and* used in production code (comments do not count), saw {seen} reference(s) ({sites:?})"
    );
}
