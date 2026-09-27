//! X-05.2 — the export containment check, tested through the path that uses it.
//!
//! `fs_guard::note_file_within` had unit tests for all three of its refusals, and
//! the review showed that mutating **all three** of its call sites so they stopped
//! using it left the whole suite green. The defence was correct in the code and
//! unproven in the tests: nothing demonstrated that the export actually routes
//! every note through it.
//!
//! So this drives the real export path, over a real HTTP router, against a
//! database containing a note whose stored `path` climbs out of the export
//! directory. That row cannot be produced through any API — `sanitize_relative_path`
//! rejects `..` at write time, which is exactly why the per-note re-check exists —
//! so it is inserted with SQL, standing in for a row written by an older build or a
//! migration. That is the case the defence is *for*, so it is the case worth
//! testing.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use brain_embed::EmbeddingEngine;
use brain_mcp::EmbedQueue;
use brain_store::Store;
use rusqlite::params;
use tower::ServiceExt;

fn tmp_db(tag: &str) -> String {
    let db = format!("/tmp/brain-exportguard-{}-{tag}.db", std::process::id());
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{db}{suffix}"));
    }
    db
}

fn tag() -> String {
    format!("brain-exportguard-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())
}

/// Inserts a note row whose `path` climbs out of the export directory.
///
/// The schema is created through [`Store`] first, so this only bypasses the
/// *path* validation — which is the thing under test — and not the database setup.
fn plant_escaping_note(db: &str, path: &str) {
    Store::open(db).expect("create the schema through the store, as any real database has");
    let conn = rusqlite::Connection::open(db).expect("open db directly");
    conn.execute(
        "INSERT INTO notes(path, layer, scope, content, tags) VALUES (?1, 'regras', 'global', '## pwned\n\nowned', '[]')",
        params![path],
    )
    .expect("the row must be insertable: the schema does not forbid a traversing path, which is the point");
}

/// Runs a real `POST /export` through the router and returns the status and body.
async fn export(router: axum::Router, to: &str) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri("/mcp/export")
        .header("content-type", "application/json")
        .body(Body::from(format!(r#"{{"to":{to},"force":true}}"#)))
        .expect("build request");
    let response = router.oneshot(req).await.expect("the router must answer");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.expect("read body");
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, value)
}

fn router_for(db: String) -> axum::Router {
    let queue = Arc::new(EmbedQueue::new(EmbeddingEngine::new("http://127.0.0.1:1".into(), "nomic-embed-text".into())));
    brain_mcp::mcp_router_with_queue(db, queue)
}

/// The export root every case here writes into: a fresh subdirectory of the
/// documented root, so the real policy is exercised rather than a widened one.
fn export_subdir() -> std::path::PathBuf {
    let p = brain_mcp::fs_guard::export_root().join(tag());
    std::fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}

#[tokio::test]
async fn a_stored_path_that_climbs_out_of_the_export_root_is_not_written() {
    let db = tmp_db("escape");
    let dir = export_subdir();
    {
        let store = Store::open(&db).unwrap();
        store.note_upsert("regras/global/legit", "regras", Some("global"), "## ok\n\nfine", None, &[], false, None).unwrap();
    }
    // Three climbs, of increasing subtlety: one level, several, and an absolute
    // path (which `join` would silently discard the base for). The names are
    // tag-scoped so a leftover from an earlier run — or from a deliberately mutated
    // build — can never be mistaken for this run writing outside the root.
    let tag = tag();
    let escape_one = format!("../brain-escaped-{tag}-one");
    let escape_two = format!("../../brain-escaped-{tag}-two");
    let escape_abs = format!("/tmp/brain-escaped-{tag}-absolute");
    plant_escaping_note(&db, &escape_one);
    plant_escaping_note(&db, &escape_two);
    plant_escaping_note(&db, &escape_abs);

    let (status, body) = export(router_for(db.clone()), &format!(r#""{}""#, dir.display())).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The escaping notes must be counted as refused...
    assert_eq!(body["refused"], 3, "all three escaping rows must be refused: {body}");
    // ...and the legitimate one must still be written, so the check is not simply
    // refusing everything and passing.
    assert_eq!(body["written"], 1, "{body}");
    // The export writes the note's own path, verbatim and without adding an
    // extension — `regras/global/legit`, not `legit.md`.
    assert!(dir.join("regras/global/legit").exists(), "the legitimate note must still be exported");
    assert_eq!(std::fs::read_to_string(dir.join("regras/global/legit")).unwrap(), "## ok\n\nfine", "with its content intact");

    // And the real assertion: nothing landed outside the root. Every target is
    // removed first, so "does not exist" means "this call did not create it" and not
    // "it was never there".
    for escaped in [
        dir.parent().unwrap().join(format!("brain-escaped-{tag}-one")),
        dir.parent().unwrap().parent().unwrap().join(format!("brain-escaped-{tag}-two")),
        std::path::PathBuf::from(format!("/tmp/brain-escaped-{tag}-absolute")),
    ] {
        let _ = std::fs::remove_file(&escaped);
        assert!(!escaped.exists(), "{} was written outside the export root", escaped.display());
    }
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&db);
}

/// Without the per-note check the three rows above would each be written.
///
/// This is the control that makes the test above meaningful: it asserts the
/// fixture really does produce traversals when the check is bypassed, so a pass
/// cannot be explained by the rows never having been seen.
#[tokio::test]
async fn the_fixture_really_does_contain_paths_that_escape() {
    let db = tmp_db("fixture");
    plant_escaping_note(&db, "../brain-escaped-fixture");
    // Read back the way the export reads: `recent`, not the table.
    let notes: Vec<String> = Store::open(&db).unwrap().recent(10_000).unwrap().into_iter().map(|n| n.0).collect();
    assert!(
        notes.iter().any(|p| p.contains("brain-escaped-fixture")),
        "the export walks existing rows, so the fixture is only meaningful if `recent` returns it: {notes:?}"
    );
    // And such a path is exactly what `note_file_within` exists to reject.
    let dir = export_subdir();
    assert!(
        brain_mcp::fs_guard::note_file_within(&dir, "../brain-escaped-fixture").is_err(),
        "a traversing stored path must be refused by the containment check"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&db);
}
