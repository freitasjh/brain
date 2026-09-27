//! The viewer's (`brain serve`) shutdown signal set, asserted against the shipped binary.
//!
//! `serve-mcp` was fixed for this in `serve_mcp_shutdown.rs`: it built its wait out
//! of `tokio::signal::ctrl_c()` — SIGINT only — so a systemd unit's SIGTERM, the
//! default `KillSignal`, killed the process by signal on every `systemctl stop`.
//! `brain serve` had the same defect in a worse shape: its body was
//! `axum::serve(listener, app).await?` with no signal handling of any kind, so it
//! did not even have the SIGINT path. `brain setup systemd` installs a
//! `brain-viewer.service` whose `ExecStart` is exactly this command, so every
//! `systemctl stop` and every deploy of the viewer killed it by signal.
//!
//! This is deliberately **not** a copy of `serve_mcp_shutdown.rs`, because the two
//! servers differ in a way that changes what is worth asserting:
//!
//!   * The MCP server's win from a graceful stop is a *teardown* — a cancel token
//!     stops its background embedding queue. The viewer has no teardown to run, so
//!     the exit code is not a proxy for anything; it is the whole observable
//!     difference, and it is asserted directly.
//!   * Readiness is a different string. The MCP child announces `:{port}/sse`; the
//!     viewer announces `viewer http://0.0.0.0:{port}/`. Matching the MCP string
//!     here would compile and pass without ever proving the *viewer* was up.
//!   * The viewer binds the wildcard `0.0.0.0`, not loopback.
//!   * Both signals are covered, so the two subcommands cannot drift apart again
//!     with nobody noticing.
//!
//! # There is deliberately no "in-flight request" assertion, and here is why
//!
//! The obvious extra property — a request that is mid-flight when the signal
//! arrives still gets its response — is **not** asserted, because it is not
//! deterministically observable from outside the process, and asserting it anyway
//! is how this file shipped a flake of its own while fixing one. Measured here:
//!
//!   * A **half-sent** request (headers without the terminating CRLF) is a race
//!     between the client's final bytes and the server's teardown. It passed 30/30
//!     in isolation and failed under full-suite load, because an RST is the
//!     *correct* TCP outcome for a process that exits with unread data in its
//!     receive queue — so the RST says nothing about whether the request was
//!     drained.
//!   * An **idle keep-alive** connection does not survive the signal at all: with a
//!     completed request still open, the server was gone within 500 ms. hyper's
//!     graceful shutdown waits for requests it is actively processing, not for
//!     sockets it is holding, so "it waited for my connection" is false here.
//!
//! What would be needed is a request guaranteed to still be *processing* when the
//! signal lands, which means a slow endpoint. `/api/status` on a scratch database
//! is sub-millisecond, and the corpus large enough to change that would cost more
//! suite time than the property is worth. So the exit code carries the assertion,
//! and the mutation below shows it discriminates.
//!
//! # The negative control, and a hazard it found
//!
//! Reverting `Cmd::Serve` to its old body makes this file fail: the exit status is
//! 143, i.e. death by SIGTERM rather than `code() == Some(0)`.
//!
//! A *partial* revert is worse, and that is why the deadline in `wait_for_exit`
//! matters. Arming the signal disposition without ever awaiting the wait —
//! `let shutdown = ...;` with the future dropped — replaces the default SIGTERM
//! action with tokio's handler, and then nothing acts on it: the process does not
//! die, does not exit, and does not stop serving. It becomes a server that ignores
//! `systemctl stop` until `TimeoutStopSec` escalates to SIGKILL. The `let` and the
//! `with_graceful_shutdown` are one fix, and a passing exit code alone cannot tell
//! the working version from that one — the half-fix hangs instead of exiting, which
//! is what the deadline catches.
//!
//! Fixtures are scratch: an ephemeral port, a `tmp` database, a `tmp` export root,
//! and a dead Ollama. Nothing here reads `data/brain.db`, binds 8321/8322, or talks
//! to a real embedding backend.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The binary cargo built for this test run.
fn brain_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_brain"))
}

struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir()
            .join(format!("brain-viewer-term-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch { dir }
    }

    fn db(&self) -> String {
        self.dir.join("brain.db").to_string_lossy().to_string()
    }

    fn export_root(&self) -> PathBuf {
        self.dir.join("export")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A `brain serve` child, SIGKILLed on drop.
struct Viewer {
    child: Child,
    port: u16,
    outlog: PathBuf,
    errlog: PathBuf,
    _scratch: Scratch,
}

impl Viewer {
    fn spawn(tag: &str) -> Viewer {
        let scratch = Scratch::new(tag);
        std::fs::create_dir_all(scratch.export_root()).unwrap();
        let port = free_port();
        let outlog = scratch.dir.join("serve.out");
        let errlog = scratch.dir.join("serve.err");
        let mut cmd = Command::new(brain_bin());
        cmd.arg("--db")
            .arg(scratch.db())
            .arg("serve")
            .arg("--port")
            .arg(port.to_string())
            .env("BRAIN_EXPORT_ROOT", scratch.export_root())
            // An inherited `BRAIN_TRANSPORT=stdio` is not this command's business,
            // but the export root check is, and a dead Ollama keeps the child off
            // the network without needing a real one.
            .env_remove("BRAIN_TRANSPORT")
            .env("BRAIN_OLLAMA_URL", "http://127.0.0.1:1")
            .stdout(Stdio::from(std::fs::File::create(&outlog).unwrap()))
            .stderr(Stdio::from(std::fs::File::create(&errlog).unwrap()));
        let child = cmd.spawn().expect("spawn `brain serve`");
        Viewer { child, port, outlog, errlog, _scratch: scratch }
    }

    fn stdout(&self) -> String {
        std::fs::read_to_string(&self.outlog).unwrap_or_default()
    }

    fn stderr(&self) -> String {
        std::fs::read_to_string(&self.errlog).unwrap_or_default()
    }

    fn diagnostics(&self) -> String {
        format!("stdout:\n{}\nstderr:\n{}", self.stdout(), self.stderr())
    }

    /// The viewer's own announcement, which is **not** the MCP one.
    ///
    /// Y-02 prints it after the bind, so it means "this port is live". A connect
    /// alone would not: the viewer binds `0.0.0.0`, and a squatter's backlog
    /// accepts loopback connects too, so only the announcement identifies *this*
    /// child.
    fn announcement(&self) -> String {
        format!("viewer http://0.0.0.0:{}/", self.port)
    }

    fn wait_listening(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => panic!("`brain serve` exited before binding:\n{}", self.diagnostics()),
                Err(e) => panic!("wait failed: {e}"),
                Ok(None) => {}
            }
            if self.stdout().contains(&self.announcement())
                && std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("`brain serve` never announced {}:\n{}", self.announcement(), self.diagnostics());
    }

    fn addr(&self) -> SocketAddr {
        format!("127.0.0.1:{}", self.port).parse().unwrap()
    }

    fn sigterm(&mut self) {
        let pid = self.child.id() as i32;
        // SAFETY: `kill` with a positive pid belonging to this process's own child.
        // The pid came from `Child::id`, so it is neither 0 nor the process group,
        // and no `wait` has run, so the child is unreaped and ours to signal.
        unsafe { libc_kill(pid, 15) };
    }

    /// SIGTERM, then the exit status it produced.
    fn wait_for_exit(&mut self, what: &str) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    return self.child.wait().unwrap_or_else(|_| std::process::ExitStatus::default())
                }
                Ok(None) if Instant::now() >= deadline => {
                    let _ = self.child.kill();
                    // Also report whether it was still *serving*: a process that
                    // caught the signal and never acted on it lands here, and the
                    // difference between "hung" and "died" is the whole hazard.
                    panic!("`brain serve` {what} for 20s:\n{}", self.diagnostics());
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(e) => panic!("wait failed: {e}"),
            }
        }
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

unsafe extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

fn free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let p = l.local_addr().unwrap().port();
    drop(l);
    p
}

/// A completed request, start to finish, proving this child is actually serving.
///
/// Not the property under test — it is the baseline that makes a failure legible:
/// "the endpoint answered before the signal" is what separates "the shutdown was
/// clean" from "the child was never up".
fn assert_serves_status(addr: SocketAddr) {
    let mut s = TcpStream::connect(addr).expect("connect to viewer");
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(b"GET /api/status HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 200"), "viewer did not serve /api/status: {out:?}");
}

/// SIGTERM — not SIGINT — ends `serve` with exit 0, and the port is released.
///
/// `code() == Some(0)` is the assertion. Death by an unhandled SIGTERM reports
/// `code() == None` (the shell renders it 143), and a process that catches the
/// signal without acting on it never exits at all, which `wait_for_exit` catches
/// with its deadline. So all three states fail here: 143, and a hang.
#[test]
#[cfg(unix)]
fn sigterm_stops_serve_cleanly() {
    let mut v = Viewer::spawn("sigterm");
    v.wait_listening();
    assert_serves_status(v.addr());

    v.sigterm();
    let status = v.wait_for_exit("ignored SIGTERM");
    assert_eq!(
        status.code(),
        Some(0),
        "SIGTERM should end `serve` with exit 0, got {status:?}\n{}",
        v.diagnostics()
    );

    // The listener is gone with the process, so the port is connectable again. This
    // is the same fact as the exit code from the other side, and it is what makes
    // `systemctl stop` release the port for the next start rather than leaving a
    // socket in the way.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", v.port)).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("port {} still accepting after `serve` exited 0:\n{}", v.port, v.diagnostics());
}

/// SIGINT — a terminal Ctrl-C — takes the same path, so the two signals cannot
/// drift apart again.
///
/// This is the cheap half of the property and the half most likely to rot: a later
/// edit that reached for `ctrl_c()` alone would still satisfy the SIGTERM test on a
/// machine where nobody presses Ctrl-C, and the drift would stay invisible until a
/// terminal was involved.
#[test]
#[cfg(unix)]
fn sigint_stops_serve_cleanly() {
    let mut v = Viewer::spawn("sigint");
    v.wait_listening();
    assert_serves_status(v.addr());
    let pid = v.child.id() as i32;
    // SAFETY: as in the SIGTERM case; our own unreaped child, a real signal.
    unsafe { libc_kill(pid, 2) };
    let status = v.wait_for_exit("ignored SIGINT");
    assert_eq!(status.code(), Some(0), "SIGINT should end `serve` with exit 0, got {status:?}");
}
