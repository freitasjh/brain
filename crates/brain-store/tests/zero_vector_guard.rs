//! Regression guard: no production call site may write a BLOB-of-zeros vector.
//!
//! # Why this test exists
//!
//! 735 of the 738 chunks in the production index held a BLOB of 3072 zero bytes.
//! `cosine` returns `0.0` for a zero-norm vector, so every one of them scored
//! `0.0` against every query while *still* consuming a slot in `search`'s
//! candidate budget — the vector half of the RRF was pure noise, and the
//! `notes`/`chunks` counts that `brain status` used to print stayed perfectly
//! healthy. Nothing failed, nothing warned, and semantic search was simply off.
//!
//! The store now rejects a zero vector at the `chunk_insert` boundary and
//! `chunks_sync` writes SQL `NULL` instead, so the damage cannot recur through
//! the store API. This test closes the other door: it reads the source of every
//! production crate and fails if a zero-vector literal reappears in it, which is
//! the shape the original bug took in all seven store/CLI/MCP call sites.
//!
//! It is a source scan rather than a behavioural test on purpose. A behavioural
//! test can only assert on the paths it happens to exercise; this one covers
//! every file in `crates/*/src/`, including the call sites no test touches.

use std::path::{Path, PathBuf};

/// Zero-vector literal shapes that have actually appeared in this codebase.
const FORBIDDEN: &[&str] = &[
    "vec![0.0; 768]",
    "vec![0.0;768]",
    "vec![0.0; EMBEDDING_DIM]",
    "vec![0.0;EMBEDDING_DIM]",
    "[0.0; EMBEDDING_DIM]",
    "[0.0f32; EMBEDDING_DIM]",
];

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `<root>/crates/brain-store`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/brain-store must live two levels below the workspace root")
        .to_path_buf()
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

/// Byte offset of the last `#[cfg(test)]` marker, i.e. where test-only code begins.
///
/// Every crate in this workspace puts its test module last, so "an offset past
/// this marker" is a sound proxy for "inside `#[cfg(test)]`". A file with no
/// marker yields `None`, which the caller treats as "the whole file is
/// production" — the conservative direction.
fn test_region_start(src: &str) -> Option<usize> {
    src.rfind("#[cfg(test)]")
}

#[test]
fn no_zero_vector_literal_in_production_source() {
    let root = workspace_root();
    let crates = root.join("crates");
    assert!(crates.is_dir(), "workspace crates dir not found at {}", crates.display());

    let mut sources = Vec::new();
    rust_sources(&crates, &mut sources);
    assert!(sources.len() >= 6, "expected the 6 workspace crates, found {} .rs files under {}", sources.len(), crates.display());

    let mut scanned = 0usize;
    let mut violations: Vec<String> = Vec::new();

    for file in &sources {
        // `examples/` and `tests/` are runnable/compiled separately from the
        // library, and the acceptance criterion for this fix is specifically
        // about `crates/*/src/`.
        if !file.components().any(|c| c.as_os_str() == "src") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(file) else {
            violations.push(format!("{}: could not be read", file.display()));
            continue;
        };
        scanned += 1;
        let test_start = test_region_start(&src);
        for needle in FORBIDDEN {
            let mut from = 0usize;
            while let Some(rel) = src[from..].find(needle) {
                let at = from + rel;
                let in_tests = test_start.is_some_and(|t| at > t);
                if !in_tests {
                    let line = src[..at].lines().count();
                    violations.push(format!(
                        "{}:{}: zero-vector literal `{}` in production code — a zero-norm vector scores 0.0 for every query; pass None so the chunk is stored as SQL NULL",
                        file.strip_prefix(&root).unwrap_or(file).display(),
                        line,
                        needle
                    ));
                }
                from = at + needle.len();
            }
        }
    }

    assert!(scanned >= 6, "expected to scan at least the 6 crate `src` roots, scanned {}", scanned);
    assert!(
        violations.is_empty(),
        "zero-vector literals reintroduced in production code:\n{}",
        violations.join("\n")
    );
}

#[test]
fn every_workspace_crate_is_covered_by_the_scan() {
    // Guards against the scan silently covering nothing if the layout changes:
    // a vacuous pass is the one way this guard could stop guarding.
    let root = workspace_root();
    let mut sources = Vec::new();
    rust_sources(&root.join("crates"), &mut sources);
    let names: Vec<String> = sources.iter().filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string())).collect();
    for expected in ["lib.rs", "main.rs"] {
        assert!(
            names.iter().any(|n| n == expected),
            "expected at least one {} under crates/; found {:?}",
            expected,
            names
        );
    }
}
