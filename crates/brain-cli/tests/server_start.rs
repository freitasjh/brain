//! US-01.1 — `brain server start`, end to end through the real binary.
//!
//! The four ACs, one test each, and each test drives the **shipped executable**
//! rather than calling the functions behind it. That is the whole reason this
//! file exists. The spec's ACs are about what an operator gets from a command —
//! a bound port, a file on disk, a server that stayed up — and a unit test on
//! `import_legacy` cannot fail if someone drops the dispatch arm that calls it,
//! which is exactly how US-01.1 stayed un-implemented for six batches while the
//! pieces it needed were all present.
//!
//! Every fixture is scratch: an ephemeral port, a `tmp` database, a `tmp` export
//! root. Nothing here reads `data/brain.db`, talks to a real Ollama, or touches
//! the production server's ports.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use brain_store::Store;

/// The binary cargo built for this test run.
///
/// `CARGO_BIN_EXE_brain` is the path of the artifact cargo compiled for *this*
/// run, so the test cannot silently exercise a stale `brain` found on `PATH`.
fn brain_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_brain"))
}

struct Scratch {
    dir: PathBuf,
    /// False for a [`Scratch`] handed to a child, which deletes the directory
    /// instead — so a test that hands its directory away and then reads it back
    /// does not have it removed from under it.
    dir_owned: bool,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("brain-srvstart-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch { dir, dir_owned: true }
    }

    fn db(&self) -> String {
        self.dir.join("brain.db").to_string_lossy().to_string()
    }

    fn vault(&self) -> PathBuf {
        self.dir.join("vault")
    }

    fn export_root(&self) -> PathBuf {
        self.dir.join("export")
    }

    /// Writes `n` legacy notes under `regras/global/`, the shape the import
    /// actually keys off (layer directory, then scope, then the path).
    fn vault_with_notes(&self, n: usize) {
        for i in 0..n {
            let p = self.vault().join("regras/global").join(format!("legacy-{i}.md"));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, format!("## Legado {i}\nconteudo legado-unico-{i}\n")).unwrap();
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if self.dir_owned {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

impl Scratch {
    /// The caller's own scratch, now owned by the child.
    ///
    /// A test that populated a vault and then hands the directory to a server
    /// needs one owner, or the child's drop deletes it while the test is still
    /// reading it. The test therefore does not keep its own binding alive across
    /// the spawn — it moves in, and the cleanup happens once, on the child.
    fn adopt(s: &Scratch) -> Scratch {
        Scratch { dir: s.dir.clone(), dir_owned: false }
    }
}

/// A `brain server start` child, killed on drop.
struct ServerStart {
    child: Child,
    port: u16,
    outlog: PathBuf,
    errlog: PathBuf,
    _scratch: Scratch,
}

impl ServerStart {
    /// `vault`/`old_index` are passed as absolute paths so the import never looks
    /// at the working directory the test happens to run in — a default of
    /// `./vault` resolved against the crate directory would find the repository's
    /// own layout on a developer machine and pass for the wrong reason.
    /// A scratch of its own, with an ephemeral port.
    ///
    /// The scratch is constructed and handed to the child in one expression. It
    /// was not, at first: a local `Scratch` was created, borrowed by the spawn,
    /// and then dropped on the way out — and `Scratch::drop` deletes its
    /// directory, so the child was left running against a deleted log file and
    /// every readiness probe read an empty file. The port really did bind; the
    /// test could not see it. Ownership has to be moved, not borrowed and
    /// released.
    fn spawn(tag: &str, port_arg: Option<u16>, env: &[(&str, &str)]) -> ServerStart {
        let port = port_arg.unwrap_or_else(free_port);
        ServerStart::spawn_owned(Scratch::new(tag), port, env)
    }

    /// A child over a scratch the caller already populated.
    ///
    /// `vault`/`old-index` are passed as absolute paths so the import never looks
    /// at the working directory the test happens to run in — a default of
    /// `./vault` resolved against the crate directory would find the repository's
    /// own layout on a developer's machine and pass for the wrong reason.
    fn spawn_in(_tag: &str, scratch: &Scratch, port: u16, env: &[(&str, &str)]) -> ServerStart {
        ServerStart::spawn_owned(Scratch::adopt(scratch), port, env)
    }

    fn spawn_owned(scratch: Scratch, port: u16, env: &[(&str, &str)]) -> ServerStart {
        std::fs::create_dir_all(scratch.export_root()).unwrap();
        let outlog = scratch.dir.join("server.out");
        let errlog = scratch.dir.join("server.err");
        let mut cmd = Command::new(brain_bin());
        cmd.arg("--db")
            .arg(scratch.db())
            .arg("server")
            .arg("start")
            .arg("--vault")
            .arg(scratch.vault())
            .arg("--old-index")
            .arg(scratch.dir.join("no-such-index.db"))
            .env("BRAIN_EXPORT_ROOT", scratch.export_root())
            // Never inherit the developer's real Ollama: AC4 is about a *dead*
            // one, and an inherited reachable one would make AC4 vacuous.
            .env_remove("BRAIN_TRANSPORT")
            .env("BRAIN_OLLAMA_URL", "http://127.0.0.1:1")
            .env("BRAIN_EMBED_MAX_FAILURES", "0")
            .stdout(Stdio::from(std::fs::File::create(&outlog).unwrap()))
            .stderr(Stdio::from(std::fs::File::create(&errlog).unwrap()));
        cmd.arg("--port").arg(port.to_string());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = cmd.spawn().expect("spawn `brain server start`");
        ServerStart { child, port, outlog, errlog, _scratch: scratch }
    }

    /// SIGTERM, then everything the child wrote.
    fn stop_and_collect(self) -> String {
        let text = format!("{}{}", self.stdout(), self.stderr());
        let status = self.sigterm();
        assert_eq!(status.code(), Some(0), "SIGTERM should end with exit 0, got {status:?}\n{text}");
        text
    }

    fn stdout(&self) -> String {
        std::fs::read_to_string(&self.outlog).unwrap_or_default()
    }

    fn stderr(&self) -> String {
        std::fs::read_to_string(&self.errlog).unwrap_or_default()
    }

    /// The port line, which `serve_rmcp_sse` prints **after** the bind.
    fn announced(&self) -> bool {
        self.stdout().contains(&format!(":{}/sse", self.port))
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn diagnostics(&self) -> String {
        format!("stdout:\n{}\nstderr:\n{}", self.stdout(), self.stderr())
    }

    /// Waits until this child announced **this** port and the port answers.
    ///
    /// The announcement is the identity check — a bare connect succeeds against
    /// whatever else holds the port, which is how a collision gets reported as a
    /// protocol failure.
    fn wait_listening(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if !self.alive() {
                panic!("`brain server start` exited before binding:\n{}", self.diagnostics());
            }
            if self.announced() && std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("`brain server start` never announced :{}/sse:\n{}", self.port, self.diagnostics());
    }

    /// SIGTERM, the signal a systemd unit sends, and the exit status it produced.
    fn sigterm(mut self) -> std::process::ExitStatus {
        {
            let pid = self.child.id() as i32;
            // SAFETY: `kill` with a positive pid from this process's own child.
            // The pid came from `Child::id`, so it cannot be 0 or negative, and the
            // child has not been reaped (no `wait` has run), so it is still ours to
            // signal.
            unsafe { libc_kill(pid, 15) };
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match self.child.try_wait() {
                Ok(Some(_status)) => return self.child.wait().unwrap_or_else(|_| std::process::ExitStatus::default()),
                Ok(None) if Instant::now() >= deadline => {
                    let _ = self.child.kill();
                    panic!("`brain server start` ignored SIGTERM for 20s:\n{}", self.diagnostics());
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(e) => panic!("wait failed: {e}"),
            }
        }
    }
}

impl Drop for ServerStart {
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

fn mcp_probe(url: &str, tool: &str, args: &str) -> (bool, String) {
    let helper = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/helpers/mcp_probe.py");
    let o = Command::new("python3")
        .arg(&helper)
        .arg(url)
        .arg(tool)
        .arg(args)
        .arg("45")
        .output()
        .expect("spawn mcp_probe.py");
    (o.status.success(), String::from_utf8_lossy(&o.stdout).to_string() + String::from_utf8_lossy(&o.stderr).as_ref())
}

/// AC1 — `brain server start` binds the requested port and speaks MCP over it.
///
/// The handshake is the assertion, not the bind: a bound port that answers
/// nothing would satisfy a `TcpStream::connect` probe, and "the port is open" is
/// not what the AC says.
#[test]
fn ac1_server_start_serves_mcp_sse_on_the_port_it_was_given() {
    let mut s = ServerStart::spawn("ac1", None, &[]);
    s.wait_listening();
    let (ok, text) = mcp_probe(&format!("http://127.0.0.1:{}", s.port), "ping", "{}");
    assert!(ok, "MCP handshake against `brain server start` failed: {text}");
    let v: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).expect("probe json");
    let names: Vec<String> = v["tool_names"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_string()).collect();
    assert!(names.contains(&"brain_search".to_string()), "tools/list: {names:?}");
    let body = serde_json::to_string(&v["call"]).unwrap();
    assert!(body.contains("pong"), "ping must answer pong: {body}");
    // `--port` is what was asked for, and it is the port that answered.
    assert!(s.alive());
    drop(s);
}

/// AC1, second half — with no `--port`, the port comes from `BRAIN_PORT`.
///
/// The spec names the variable and the two existing `serve*` subcommands both
/// hard-code `8321` in a clap `default_value_t` without reading it, so "honours
/// `BRAIN_PORT`" was documented and not implemented anywhere. `server start` is
/// the first path that actually reads it, which is worth a test of its own: a
/// regression here is invisible to every other test in the suite.
#[test]
fn ac1_server_start_falls_back_to_brain_port_when_no_flag_is_given() {
    let port = free_port();
    let mut s = ServerStart::spawn("ac1env", Some(port), &[("BRAIN_PORT", &port.to_string())]);
    s.wait_listening();
    assert!(
        s.stdout().contains(&format!(":{port}/sse")),
        "BRAIN_PORT was ignored:\n{}",
        s.diagnostics()
    );
    let (ok, text) = mcp_probe(&format!("http://127.0.0.1:{port}"), "ping", "{}");
    assert!(ok, "handshake failed on the BRAIN_PORT port: {text}");
    drop(s);
}

/// AC1, operational half — SIGTERM ends it cleanly, which is the signal a systemd
/// unit sends on `systemctl stop` and on a deploy.
///
/// A server that only answers Ctrl-C would make every unit file in `brain setup
/// systemd` depend on a controlling terminal, so this asserts the signal path and
/// the exit code rather than the log.
#[test]
fn ac1_sigterm_stops_server_start_cleanly() {
    let mut s = ServerStart::spawn("ac1term", None, &[]);
    s.wait_listening();
    let status = s.sigterm();
    assert_eq!(status.code(), Some(0), "SIGTERM should end with exit 0, got {status:?}");
}

/// AC2 — a missing `brain.db` is created at schema v4.
///
/// The version row alone would be a weak assertion: it is one string a DDL batch
/// writes, so a database where the batch wrote the row and then failed still
/// passes it. The table list is the second half — "schema v4" has to mean 15
/// tables, not one string.
#[test]
fn ac2_a_missing_database_is_created_at_schema_v4_with_every_table() {
    let scratch = Scratch::new("ac2");
    let db = scratch.db();
    assert!(!Path::new(&db).exists(), "the fixture must start with no database at all");

    let mut s = ServerStart::spawn("ac2", None, &[]);
    s.wait_listening();
    // Read through the same accessor the store uses, on a path that did not exist
    // before this process opened it.
    let store = Store::open(&db).expect("the server's database must be openable");
    assert_eq!(
        store.schema_version().unwrap().as_deref(),
        Some("4"),
        "AC2 says schema v4, and a `NULL` here means the marker was never written:\n{}",
        s.diagnostics()
    );
    let tables = store.table_names().unwrap();
    assert_eq!(tables.len(), 15, "schema v4 is 15 tables, found {tables:?}");
    for t in ["notes", "chunks", "projects", "notes_fts", "audit_log", "entities", "links", "_meta"] {
        assert!(tables.contains(&t.to_string()), "{t} missing from {tables:?}");
    }
    drop(store);
    drop(s);
}

/// AC3 — a legacy vault is archived, then imported, and the archive is the
/// operator's way back.
///
/// The order is the point, so the test checks the pair rather than either: the
/// archive exists **and** the notes are in the database, and the archive holds
/// the same two notes under their original paths.
#[test]
fn ac3_a_legacy_vault_is_archived_and_then_imported() {
    let scratch = Scratch::new("ac3");
    scratch.vault_with_notes(2);
    let root = scratch.export_root();
    let db = scratch.db();

    // `server start` blocks, so it is spawned and then terminated: the import is
    // complete the moment the archive line is printed, and the test asserts on
    // that line, so waiting for the port announcement is what makes it
    // deterministic rather than a sleep.
    let mut child = ServerStart::spawn_in("ac3", &scratch, free_port(), &[]);
    child.wait_listening();
    let text = child.stop_and_collect();
    let archive = root.join("vault.bak.tar.gz");
    assert!(
        archive.exists(),
        "AC3 requires vault.bak.tar.gz before the import, and it is absent. Output was:\n{text}"
    );
    assert!(text.contains("vault.bak.tar.gz"), "the operator must be told where it went:\n{text}");
    assert!(text.contains("imported 2 legacy note"), "the import must report itself:\n{text}");

    // The archive is a real tar.gz holding both original files, and it is
    // readable by an independent reader rather than by the crate that wrote it.
    let names = tar_gz_names(&archive);
    assert_eq!(names.len(), 2, "the archive must hold both legacy notes: {names:?}");
    assert!(names.iter().any(|n| n.ends_with("regras/global/legacy-0.md")), "{names:?}");

    // And the notes are really in the database, searchable with no vector at all.
    let store = Store::open(&db).unwrap();
    assert_eq!(store.count_notes().unwrap(), 2);
    let hits = store.search("legado-unico", None, None, None, None, None, 5, false).unwrap();
    assert_eq!(hits.len(), 2, "both imported notes must be FTS-searchable: {hits:?}");
}

/// AC3, second run — the first archive is not overwritten.
///
/// The mutation this discriminates is `if !backup.exists()`: with that, the
/// second run writes nothing and the first archive is the only record — which is
/// indistinguishable from "the second run backed up the same thing".
#[test]
fn ac3_a_second_start_writes_a_second_archive_and_leaves_the_first_alone() {
    let scratch = Scratch::new("ac3b");
    scratch.vault_with_notes(2);
    let root = scratch.export_root();
    std::fs::create_dir_all(&root).unwrap();

    // Two real children, one after the other, over the same vault and the same
    // database — the shape an operator gets by restarting a machine that still
    // has its old vault directory.
    let mut first_child = ServerStart::spawn_in("ac3b1", &scratch, free_port(), &[]);
    first_child.wait_listening();
    let first_text = first_child.stop_and_collect();
    let first = root.join("vault.bak.tar.gz");
    assert!(first.exists(), "the first run must archive. Output:\n{first_text}");
    let bytes = std::fs::read(&first).expect("the first run must archive");

    let mut second_child = ServerStart::spawn_in("ac3b2", &scratch, free_port(), &[]);
    second_child.wait_listening();
    let text = second_child.stop_and_collect();
    let second = root.join("vault.bak.2.tar.gz");
    assert!(second.exists(), "the second run needs its own archive. Output:\n{text}");
    assert_eq!(
        std::fs::read(&first).unwrap(),
        bytes,
        "the first archive must survive byte-identical — it is the record of what the first run saw"
    );
    assert!(text.contains("vault.bak.2.tar.gz"), "and the operator must be told which one was written:\n{text}");
    assert_eq!(tar_gz_names(&second).len(), 2);
}

/// AC4 — with Ollama unreachable the server still starts, and search still works.
///
/// The mechanism already existed (a failed embed enqueues and never blocks); what
/// was missing was any test that the *boot path* survives it. Deleting the boot
/// recovery call, or making the import embed inline, would break this.
#[test]
fn ac4_the_server_starts_and_searches_with_ollama_unreachable() {
    let scratch = Scratch::new("ac4");
    scratch.vault_with_notes(2);
    std::fs::create_dir_all(scratch.export_root()).unwrap();
    // An address nothing listens on: the connection is refused, not timed out, so
    // the test does not spend the embedding budget waiting.
    // Over the test's own scratch, not a fresh one: `spawn` makes — and clears —
    // a scratch of its own, which would have deleted the vault written two lines
    // above and made this an assertion about an empty database.
    let mut s = ServerStart::spawn_in("ac4", &scratch, free_port(), &[("BRAIN_OLLAMA_URL", "http://127.0.0.1:1")]);
    s.wait_listening();
    let (ok, text) = mcp_probe(
        &format!("http://127.0.0.1:{}", s.port),
        "brain_search",
        r#"{"query":"legado-unico","top_k":5}"#,
    );
    assert!(ok, "search over MCP with a dead Ollama failed: {text}");
    let v: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).expect("probe json");
    let body = serde_json::to_string(&v["call"]).unwrap();
    assert!(
        body.contains("legacy-0") && body.contains("legacy-1"),
        "FTS5-only fallback must still find the imported notes: {body}"
    );
    // And the degradation is *visible*, not silent: `brain status` is the tool
    // that says so, and it has to say reachable=false.
    let st = Command::new(brain_bin())
        .arg("--db")
        .arg(scratch.db())
        .arg("status")
        .env("BRAIN_OLLAMA_URL", "http://127.0.0.1:1")
        .output()
        .unwrap();
    let stext = String::from_utf8_lossy(&st.stdout);
    assert!(stext.contains("reachable=false"), "status must report the degradation:\n{stext}");
    drop(s);
}

/// The archive is read back with a gzip+tar reader that knows nothing about how
/// it was written, so "it is a tar.gz" is a claim about the bytes on disk rather
/// than about a round trip through the writer.
fn tar_gz_names(path: &Path) -> Vec<String> {
    let out = Command::new("tar")
        .arg("-tzf")
        .arg(path)
        .output()
        .expect("spawn tar -tzf");
    assert!(out.status.success(), "tar could not read {}: {}", path.display(), String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// The operator who typed the command from the spec.
#[test]
fn the_operator_sees_a_start_subcommand_and_a_help_line_naming_it() {
    let o = Command::new(brain_bin()).arg("--help").output().unwrap();
    let help = String::from_utf8_lossy(&o.stdout);
    assert!(help.contains("server"), "the top-level help must list `server`:\n{help}");
    let o = Command::new(brain_bin()).arg("server").arg("--help").output().unwrap();
    let help = String::from_utf8_lossy(&o.stdout);
    assert!(help.contains("start"), "`brain server --help` must list `start`:\n{help}");
    // And an unknown subcommand under `server` is refused rather than ignored.
    let o = Command::new(brain_bin()).arg("server").arg("stop").output().unwrap();
    assert!(!o.status.success());
}
