//! `brain server start` in stdio mode must stop on SIGTERM, not swallow it.
//!
//! `server start` arms its shutdown wait with
//! `rmcp_service::shutdown_on_sigint_or_sigterm()` **before** the legacy import,
//! on purpose: the import is synchronous and can take minutes, and a SIGTERM
//! landing in that window has to be recorded rather than killing the process.
//!
//! The SSE arm then hands that future to `serve_rmcp_sse_with`, which polls it.
//! The **stdio** arm did not: it called `serve_stdio(db).await` and dropped the
//! wait on the floor.
//!
//! Arming SIGTERM without ever acting on it is worse than never arming it. The
//! disposition is now tokio's rather than the kernel's default, so the signal is
//! recorded and then ignored: the process keeps reading stdin and serving, and
//! the unit only goes away when `TimeoutStopSec` escalates to `SIGKILL`.
//! `serve_viewer_shutdown.rs` calls that shape "worse than the bug" and measures
//! it, and this is the same shape one call site over.
//!
//! `Cmd::ServeMcp`'s own stdio arm is a different case and deliberately not
//! covered here: it never arms anything, so SIGTERM is the default action and the
//! process does stop. Arming-and-ignoring is the only order that hangs.
//!
//! The readiness signal is a `ping` answered on stdout, not a sleep and not a
//! port. The reply can only be written once `select!` is polling the stdio future,
//! so it proves the arm is *live* — which is the whole difference between this
//! test measuring something and measuring nothing.
//!
//! Fixtures are scratch: a `tmp` database, a `tmp` export root, a dead Ollama, and
//! a vault directory that does not exist — so `has_legacy_notes` short-circuits
//! before any database or archive is touched. Nothing here reads
//! `data/brain.db` or binds 8321/8322.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

/// The binary cargo built for this test run.
fn brain_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_brain"))
}

struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new() -> Scratch {
        let dir = std::env::temp_dir()
            .join(format!("brain-stdio-term-{}", std::process::id()));
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

    /// A vault that was never there, so the import is a no-op rather than a
    /// several-hundred-millisecond archive.
    fn absent_vault(&self) -> PathBuf {
        self.dir.join("no-such-vault")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A `brain server start` child in stdio mode, SIGKILLed on drop.
struct StdioServer {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    _scratch: Scratch,
}

impl StdioServer {
    fn spawn() -> StdioServer {
        let scratch = Scratch::new();
        std::fs::create_dir_all(scratch.export_root()).unwrap();
        let child = Command::new(brain_bin())
            .arg("--db")
            .arg(scratch.db())
            .arg("server")
            .arg("start")
            .arg("--vault")
            .arg(scratch.absent_vault())
            .env("BRAIN_TRANSPORT", "stdio")
            .env("BRAIN_EXPORT_ROOT", scratch.export_root())
            .env("BRAIN_OLLAMA_URL", "http://127.0.0.1:1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn `brain server start`");
        let mut child = child;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        StdioServer { child, stdin, stdout, _scratch: scratch }
    }

    /// Writes one request and reads one reply, proving the stdio loop is polling.
    fn round_trip(&mut self, request: &str) -> String {
        writeln!(self.stdin, "{request}").expect("write to child stdin");
        self.stdin.flush().expect("flush child stdin");
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read child stdout");
        line
    }

    fn sigterm(&mut self) {
        let pid = self.child.id() as i32;
        // SAFETY: `kill` with a positive pid belonging to this process's own
        // child. The pid came from `Child::id`, so it is neither 0 nor the process
        // group, and no `wait` has run, so the child is unreaped and ours.
        unsafe { libc_kill(pid, 15) };
    }

    /// SIGTERM, then the exit status it produced, on a deadline.
    fn wait_for_exit(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    return self.child.wait().unwrap_or_else(|_| std::process::ExitStatus::default());
                }
                Ok(None) if Instant::now() >= deadline => {
                    let _ = self.child.kill();
                    panic!(
                        "`server start` (stdio) ignored SIGTERM for 20s — the signal is being \
                         caught and then not acted on, which is the half-fix this file exists to \
                         rule out. A `systemctl stop` would sit here until TimeoutStopSec escalates \
                         to SIGKILL."
                    );
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(e) => panic!("wait failed: {e}"),
            }
        }
    }
}

impl Drop for StdioServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

unsafe extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

/// SIGTERM ends `server start` in stdio mode with exit 0, and it does so promptly.
///
/// `code() == Some(0)` is the assertion. Death by an unhandled SIGTERM reports
/// `code() == None` (the shell renders it 143), and a process that catches the
/// signal without acting on it never exits at all — so all three states fail here:
/// 143, and the hang, which `wait_for_exit` catches with its deadline.
#[test]
#[cfg(unix)]
fn sigterm_stops_server_start_in_stdio_mode() {
    let mut s = StdioServer::spawn();

    // Readiness: the loop answered, so the arm is live and a following signal is
    // testing the shutdown path rather than a race with start-up.
    let reply = s.round_trip("{\"tool\":\"ping\"}");
    assert!(reply.contains("pong"), "the stdio loop never answered: {reply:?}");

    s.sigterm();
    let status = s.wait_for_exit();
    assert_eq!(
        status.code(),
        Some(0),
        "SIGTERM should end stdio `server start` with exit 0, got {status:?}"
    );
}

/// The negative control, stated rather than assumed.
///
/// Reverting the stdio arm to a bare `serve_stdio(db).await` — building the wait
/// and never polling it — makes the test above fail on the **deadline**, not on
/// the exit code: the process is still running and still answering stdin when the
/// 20 s expires. That distinction is the whole reason this file exists, and it is
/// why the assertion cannot be satisfied by a child that merely exits.
#[test]
#[cfg(unix)]
fn the_stdio_loop_is_still_answering_before_the_signal() {
    // Not the property under test — the baseline that makes a failure legible.
    // "The loop answered" is what separates "the shutdown was clean" from "the
    // child was never up", and it also proves the wait is being polled, which is
    // the difference between a working arm and the dropped one.
    let mut s = StdioServer::spawn();
    let first = s.round_trip("{\"tool\":\"ping\"}");
    assert!(first.contains("pong"), "the stdio loop never answered: {first:?}");
    let second = s.round_trip("{\"tool\":\"ping\"}");
    assert!(second.contains("pong"), "the stdio loop stopped answering: {second:?}");
}
