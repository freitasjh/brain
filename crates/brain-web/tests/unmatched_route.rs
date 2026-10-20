//! The router's fallback answers two different things, and swapping them is a
//! bug in both directions.
//!
//! A deep non-API path (`/deep/link`) is a client route of a single-page app and
//! must be answered with the viewer document. An unknown path under `/api` is a
//! client mistake and must be answered `404` with a JSON body — not with the
//! document.
//!
//! The second half was live: under the previous `ServeDir` fallback, `/api/stat`
//! (a typo for `/api/status`) returned `200 text/html` carrying the viewer. That
//! hides the error from every caller that checks the status — `curl -f` exits 0 —
//! and a JSON-reading client ends up string-searching an HTML document.
//!
//! # Mutations that must fail this file
//!
//! - `unmatched_route` returns `Html(VIEWER_INDEX)` for every path →
//!   `unknown_api_paths_are_404_json` fails on the status.
//! - `unmatched_route` returns the `404` JSON for every path →
//!   `a_deep_client_route_is_the_viewer_document` fails, which is the SPA half of
//!   the original bug coming back.
//! - `unmatched_route` tests the raw path instead of the slash-normalised one
//!   (`path.starts_with("/api")`) → `unknown_api_paths_are_404_json` fails on the
//!   `//api/status` case, which is the doubled-slash form the fallback used to
//!   answer with the document.
//!
//! These four assertions live in one binary because none of them touches the
//! process cwd; `viewer_index_cwd.rs` holds the cwd test and holds exactly one
//! test, for the opposite reason.

use axum::body::Body;
use axum::http::{Request, StatusCode, header::CONTENT_TYPE};
use tower::ServiceExt as _;

fn db_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("brain-web-unmatched-{}.db", std::process::id()))
}

/// One request through the real router, from the real cwd.
fn get(db: &str, uri: &str) -> (StatusCode, String, Vec<u8>) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let app = brain_web::router(db.to_string());
        let resp = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .expect("router response");
        let status = resp.status();
        let ctype = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap().to_vec();
        (status, ctype, body)
    })
}

#[test]
fn unknown_api_paths_are_404_json() {
    let db = db_path();
    let db = db.to_string_lossy().into_owned();
    // `/api` bare, a typo, a rejected extra segment, and a path that only looks
    // like the namespace. Every one of them is a mistake, and every one of them
    // used to be `200 text/html`.
    // `//api/status` is the doubled-slash form: a base URL with a trailing `/`
    // concatenated with `/api/status` produces it, and it misses every explicit
    // route, so it lands in this fallback. `/apifoo` pins the opposite decision —
    // the namespace test is a prefix, so an `api`-prefixed segment is reserved
    // too. Neither is a typo, and both used to answer `200 text/html`.
    for uri in ["/api", "/api/stat", "/api/nonexistent", "/api/status/extra", "//api/status", "/apifoo"] {
        let (status, ctype, body) = get(&db, uri);
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "`{uri}` must be 404, not 200 — a typo that answers 200 is a typo nobody finds ({ctype})"
        );
        assert!(
            ctype.starts_with("application/json"),
            "`{uri}` must be JSON, got `{ctype}` — the viewer is not an API response"
        );
        let text = String::from_utf8_lossy(&body).to_string();
        assert!(
            text.contains("not found") && text.contains(uri),
            "`{uri}` must say what was not found and which path was asked for, got `{text}`"
        );
        assert!(
            !text.contains("<script>") && body != brain_web::VIEWER_INDEX.as_bytes(),
            "`{uri}` must not carry the viewer document"
        );
    }
}

#[test]
fn a_deep_client_route_is_the_viewer_document() {
    let db = db_path();
    let db = db.to_string_lossy().into_owned();
    let (status, ctype, body) = get(&db, "/deep/link");
    assert_eq!(status, StatusCode::OK, "a client route must not 404 — that is the SPA half of the bug");
    assert!(ctype.starts_with("text/html"), "a client route must be html, got `{ctype}`");
    assert_eq!(
        body.as_slice(),
        brain_web::VIEWER_INDEX.as_bytes(),
        "a client route must serve the embedded document byte for byte"
    );
}

#[test]
fn the_api_routes_are_not_shadowed_by_the_fallback() {
    let db = db_path();
    let _ = std::fs::remove_file(&db);
    let db = db.to_string_lossy().into_owned();
    {
        let _s = brain_store::Store::open(&db).unwrap();
    }
    let (status, ctype, body) = get(&db, "/api/status");
    assert_eq!(status, StatusCode::OK, "/api/status must still be the route, not the fallback");
    assert!(ctype.starts_with("application/json"), "/api/status must stay JSON, got `{ctype}`");
    let v: serde_json::Value = serde_json::from_slice(&body).expect("/api/status must be valid JSON");
    assert!(v.get("notes").is_some() && v.get("chunks").is_some(), "got {v}");
    let _ = std::fs::remove_file(&db);
}
