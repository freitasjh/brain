//! The `serve-mcp` shutdown signal set, asserted against the shipped binary.
//!
//! `serve-mcp` used to build its own wait out of `tokio::signal::ctrl_c()` —
//! SIGINT only — while `server start` used `shutdown_on_sigint_or_sigterm`. A
//! terminal Ctrl-C is SIGINT, so on a terminal the difference was invisible; a
//! systemd unit sends SIGTERM, so on the units `brain setup systemd` installs,
//! every `systemctl stop` and every deploy killed the process **by signal** and
//! never reached the `ct.cancel()` that the wait is there to reach.
//!
//! This file cannot be a unit test. The property is "the OS sends SIGTERM and the
//! process exits 0 instead of dying by signal", and only a real signal delivered
//! to a real process observes it. So it spawns `brain serve-mcp`, waits for the
//! port announcement, sends SIGTERM with `kill(2)`, and asserts `exit 0`.
//!
//! The negative control is what makes it a real test: under the old code the
//! default SIGTERM action terminates the process, `ExitStatus::code()` is `None`,
//! and this assertion fails. A passing run cannot come from a build that ignores
//! the signal.
//!
//! Fixtures are scratch: an ephemeral port, a `tmp` database, a `tmp` export
//! root, and a dead Ollama. Nothing here reads `data/brain.db`, binds a
//! production port, or talks to a real embedding backend.

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
        let dir = std::env::temp_dir().join(format!("brain-servemcp-term-{}-{tag}", std::process::id()));
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

/// A `brain serve-mcp` child, SIGKILLed on drop.
struct ServeMcp {
    child: Child,
    port: u16,
    outlog: PathBuf,
    errlog: PathBuf,
    _scratch: Scratch,
}

impl ServeMcp {
    fn spawn(tag: &str) -> ServeMcp {
        let scratch = Scratch::new(tag);
        std::fs::create_dir_all(scratch.export_root()).unwrap();
        let port = free_port();
        let outlog = scratch.dir.join("serve-mcp.out");
        let errlog = scratch.dir.join("serve-mcp.err");
        let mut cmd = Command::new(brain_bin());
        cmd.arg("--db")
            .arg(scratch.db())
            .arg("serve-mcp")
            .arg("--port")
            .arg(port.to_string())
            .env("BRAIN_EXPORT_ROOT", scratch.export_root())
            // An inherited `BRAIN_TRANSPORT=stdio` would make the child serve
            // stdio and exit, and the test would be about nothing. A dead Ollama
            // keeps it off the network without needing one.
            .env_remove("BRAIN_TRANSPORT")
            .env("BRAIN_OLLAMA_URL", "http://127.0.0.1:1")
            .env("BRAIN_EMBED_MAX_FAILURES", "0")
            .stdout(Stdio::from(std::fs::File::create(&outlog).unwrap()))
            .stderr(Stdio::from(std::fs::File::create(&errlog).unwrap()));
        let child = cmd.spawn().expect("spawn `brain serve-mcp`");
        ServeMcp { child, port, outlog, errlog, _scratch: scratch }
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

    /// Waits until the child announced **this** port and the port answers.
    ///
    /// The announcement is printed after the bind, so it means "this port is
    /// live" — the same identity check `server start`'s test uses. Signalling a
    /// process that has not reached its wait yet would be a race, not a test.
    fn wait_listening(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => panic!("`brain serve-mcp` exited before binding:\n{}", self.diagnostics()),
                Err(e) => panic!("wait failed: {e}"),
                Ok(None) => {}
            }
            if self.stdout().contains(&format!(":{}/sse", self.port))
                && std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("`brain serve-mcp` never announced :{}/sse:\n{}", self.port, self.diagnostics());
    }

    /// SIGTERM — what a systemd unit sends — and the exit status it produced.
    fn sigterm(mut self) -> std::process::ExitStatus {
        {
            let pid = self.child.id() as i32;
            // SAFETY: `kill` with a positive pid belonging to this process's own
            // child. The pid came from `Child::id`, so it is neither 0 nor the
            // process group, and no `wait` has run, so the child is unreaped and
            // ours to signal.
            unsafe { libc_kill(pid, 15) };
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return self.child.wait().unwrap_or_else(|_| std::process::ExitStatus::default()),
                Ok(None) if Instant::now() >= deadline => {
                    let _ = self.child.kill();
                    panic!("`brain serve-mcp` ignored SIGTERM for 20s:\n{}", self.diagnostics());
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(e) => panic!("wait failed: {e}"),
            }
        }
    }
}

impl Drop for ServeMcp {
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
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let p = l.local_addr().unwrap().port();
    drop(l);
    p
}

/// SIGTERM — not SIGINT — ends `serve-mcp` with exit 0.
///
/// `code() == Some(0)` is the whole assertion. A process killed by an unhandled
/// SIGTERM reports `code() == None`, and one that hangs until `TimeoutStopSec`
/// turns into SIGKILL reports `None` too after this test's own deadline, so both
/// wrong behaviours fail here instead of passing quietly.
#[test]
#[cfg(unix)]
fn sigterm_stops_serve_mcp_cleanly() {
    let mut s = ServeMcp::spawn("sigterm");
    s.wait_listening();
    let status = s.sigterm();
    assert_eq!(status.code(), Some(0), "SIGTERM should end `serve-mcp` with exit 0, got {status:?}");
}
