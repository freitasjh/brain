//! The viewer HTML is part of the binary, not of the working directory.
//!
//! `brain serve` used to resolve `viewer/index.html` with a cwd-relative path,
//! so `/` answered `404 viewer/index.html not found` for every process whose cwd
//! was not the repo root — which is every real install: `cargo install` ships the
//! binary alone, and the systemd unit `brain setup systemd` writes has no
//! `WorkingDirectory`. The `/api/*` routes were always absolute, so only the HTML
//! broke, which is what made the failure look like "the API installed, the viewer
//! did not".
//!
//! This file holds **exactly one test** on purpose: `set_current_dir` is
//! process-wide, and a second test running in parallel in this binary would have
//! its cwd moved underneath it. One test in the process makes the chdir safe,
//! which is what lets the assertion be about cwd at all instead of about a
//! constant.
//!
//! # Mutations that must make this fail
//!
//! 1. Restore the cwd-relative read in `serve_index`:
//!
//! ```ignore
//! tokio::fs::read_to_string("viewer/index.html").await
//! ```
//!
//! The temp cwd below has no `viewer/index.html`, so that line answers 404 and
//! the test fails. If the test still passes with that mutation, it is not
//! anchored to the behaviour it claims to protect.
//!
//! 2. Restore a directory service rooted at the cwd — the old
//!    `ServeDir::new("viewer").fallback(get(serve_index))`: the temp cwd **does**
//!    contain a `viewer/stray.txt`, and `stray.txt` must arrive as the viewer
//!    document, never as itself. That is the half of the property the embed alone
//!    does not cover, and the deliberate difference between the two
//!    preconditions: no `viewer/index.html` (so the cwd read is broken), but a
//!    `viewer/` that a directory service would happily serve from.

use std::path::PathBuf;

/// Content of the `viewer/stray.txt` this test plants in the temp cwd. Distinctive
/// on purpose: if it ever appears in a response body, a directory service served it
/// from disk.
const STRAY_BYTES: &[u8] = b"stray-file-contents-that-must-never-be-served\n";

/// Restores the original cwd even when the assertion panics, so a failure here
/// cannot cascade into every test that resolves a relative path.
struct CwdGuard(PathBuf);

impl CwdGuard {
    fn enter(dir: &std::path::Path) -> Self {
        let prev = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(dir).expect("set cwd");
        Self(prev)
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

fn content_type_of(resp: &axum::response::Response) -> String {
    resp.headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

/// `/`, a deep non-API route and a file that exists inside a `viewer/` directory
/// under the process cwd: the first two serve the embedded viewer from a cwd that
/// is not the repo root, and the third must **also** be the viewer document rather
/// than the file on disk.
#[test]
fn the_viewer_index_is_served_from_a_cwd_without_the_viewer_directory() {
    let tmp = std::env::temp_dir().join(format!("brain-web-cwd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("temp cwd");
    // Two preconditions, and the difference between them is the point.
    //
    // (a) No `viewer/index.html`: the directory this process adopts as cwd cannot
    // satisfy the old cwd-relative lookup, which is what gives mutation 1 above
    // its teeth. Asserted rather than assumed, because a stray file here would
    // make the test pass for the wrong reason.
    //
    // (b) A `viewer/stray.txt` that *is* there: a directory service rooted at the
    // cwd would serve it, so the `/stray.txt` assertion below fails if one
    // comes back (mutation 2). The file has to exist for that assertion to mean
    // anything — it is the `viewer/server.py` leak in miniature: a real file inside
    // a real `viewer/` that the old code answered `200 text/plain`.
    let stray_dir = tmp.join("viewer");
    std::fs::create_dir_all(&stray_dir).expect("temp viewer dir");
    std::fs::write(stray_dir.join("stray.txt"), STRAY_BYTES).expect("stray file");
    assert!(
        !stray_dir.join("index.html").exists(),
        "the cwd for this test must not contain a `viewer/index.html`: {}",
        stray_dir.display()
    );

    let _guard = CwdGuard::enter(&tmp);
    let db = tmp.join("brain.db").to_string_lossy().into_owned();

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    use tower::ServiceExt as _;

    let (status, ctype, body) = rt.block_on(async {
        let app = brain_web::router(db.clone());
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("router response");
        let status = resp.status();
        let ctype = content_type_of(&resp);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, ctype, body)
    });

    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "`/` must be served from the binary with cwd={} — a 404 here is the \
         `viewer/index.html not found` that breaks every `cargo install`",
        tmp.display()
    );
    assert!(
        ctype.starts_with("text/html"),
        "`/` must be text/html, got `{ctype}`"
    );
    assert_eq!(
        body.as_ref(),
        brain_web::VIEWER_INDEX.as_bytes(),
        "`/` must serve the embedded viewer byte for byte"
    );
    assert!(
        body.len() > 40_000,
        "a truncated viewer is not a viewer: {} bytes",
        body.len()
    );

    // The SPA fallback half: a deep route that is not an API path is also the
    // viewer, never a 404. Same process, same cwd.
    let rt2 = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (status, ctype) = rt2.block_on(async {
        let app = brain_web::router(db.clone());
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/some/deep/route")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("router response");
        (resp.status(), content_type_of(&resp))
    });
    assert_eq!(status, axum::http::StatusCode::OK, "SPA fallback must serve the viewer");
    assert!(ctype.starts_with("text/html"), "SPA fallback must be html, got `{ctype}`");

    // ...and a file that really is on disk under the cwd's `viewer/` still arrives
    // as the document. This is the assertion that fails if any directory service
    // rooted at the process cwd comes back: `stray.txt` would be `200
    // text/plain` with `STRAY_BYTES` in the body (see mutation 2 in the header).
    // Same process, same cwd, so it belongs in this test rather than a second one.
    let rt_stray = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let (status, ctype, body) = rt_stray.block_on(async {
        let app = brain_web::router(db.clone());
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/stray.txt")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("router response");
        let status = resp.status();
        let ctype = content_type_of(&resp);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (status, ctype, body)
    });
    assert_eq!(
        status,
        axum::http::StatusCode::OK,
        "/stray.txt is a client route, not an API path: it gets the document"
    );
    assert!(ctype.starts_with("text/html"), "/stray.txt must be html, got `{ctype}`");
    assert_eq!(
        body.as_ref(),
        brain_web::VIEWER_INDEX.as_bytes(),
        "/stray.txt must be the embedded document"
    );
    assert!(
        !body.windows(STRAY_BYTES.len()).any(|w| w == STRAY_BYTES),
        "a file from the cwd's `viewer/` directory was served — nothing in `viewer/` \
         besides `index.html` may be reachable over HTTP (viewer/server.py was, on 0.0.0.0:8322)"
    );

    // And the API half is untouched by the embed — the bug report was that the
    // API *worked* while the viewer did not, so a fix that broke the API would
    // be a different bug wearing the same name.
    let rt3 = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let status = rt3.block_on(async {
        let app = brain_web::router(db.clone());
        app.oneshot(
            axum::http::Request::builder()
                .uri("/api/status")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("router response")
        .status()
    });
    assert_eq!(status, axum::http::StatusCode::OK, "/api/status must still answer 200");

    let _ = std::fs::remove_dir_all(&tmp);
}
