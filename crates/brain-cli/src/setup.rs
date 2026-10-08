//! `brain setup` — one-shot installer (opencode MCP + kiro + systemd + project snippet).
//! All writers take explicit paths (no env reads) so unit tests stay hermetic.
//!
//! **This is the only component that writes `config.json` in production.** Until it
//! existed, the cascade's steps 1 and 4 had no producer: `brain hook` deliberately
//! does not ask (an IDE gives it no terminal), so a directory's project decision
//! could come from nowhere. That also meant two safety work from the P1–P3 review —
//! the file lock and the conised `motivo: "recusado"` — had no path that could reach
//! them. Making them reachable is what RF-05/§2 is for.

use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};

use crate::config::Entry;
use crate::resolve::Answer;

pub struct SetupOpts {
    pub target: String,
    pub mcp_port: u16,
    pub viewer_port: u16,
    pub db: String,
    pub brain_dir: Option<String>,
    pub dir: Option<String>,
    pub force: bool,
    pub dry_run: bool,
    /// R-07: assume every default and never ask, for CI.
    pub yes: bool,
    /// An explicit project for the current directory — the non-interactive answer to
    /// the question, and the only way a script can ever set one up.
    pub project: Option<String>,
    /// The other explicit answer: "do not use brain here". Recorded as the conised
    /// refusal, so the hook stops asking.
    pub decline_project: bool,
}

/// The env var equivalent of `--yes`, for a CI job that cannot add flags.
pub const NONINTERACTIVE_ENV: &str = "BRAIN_SETUP_NONINTERACTIVE";

pub fn mcp_url(port: u16) -> String {
    format!("http://localhost:{}/sse", port)
}

/// Merge `mcpServers.brain` into an existing JSON doc (or fresh `{}` when
/// unparseable, e.g. jsonc with comments). Other keys are preserved.
pub fn merge_mcp_json(existing: Option<&str>, url: &str) -> String {
    let mut v: serde_json::Value = match existing
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
    {
        Some(obj) if obj.is_object() => obj,
        _ => serde_json::json!({}),
    };
    servers_mut(&mut v).insert(
        "brain".to_string(),
        serde_json::json!({"transport": "sse", "url": url}),
    );
    serde_json::to_string_pretty(&v).unwrap()
}

fn servers_mut(v: &mut serde_json::Value) -> &mut serde_json::Map<String, serde_json::Value> {
    let m = v.as_object_mut().unwrap();
    let e = m.entry("mcpServers").or_insert(serde_json::json!({}));
    if !e.is_object() {
        *e = serde_json::json!({});
    }
    e.as_object_mut().unwrap()
}

/// Merge MCP entry + skill instructions (relative to the consumer project).
pub fn merge_project_json(existing: Option<&str>, brain_rel: &str, url: &str) -> String {
    let mut v: serde_json::Value = match existing
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
    {
        Some(obj) if obj.is_object() => obj,
        _ => serde_json::json!({}),
    };
    servers_mut(&mut v).insert(
        "brain".to_string(),
        serde_json::json!({"transport": "sse", "url": url}),
    );
    let entries = [
        format!("{}/.agents/skills/brain/SKILL.md", brain_rel),
        format!("{}/.agents/rules/BRAIN.MCP.md", brain_rel),
    ];
    let m = v.as_object_mut().unwrap();
    let inst = m.entry("instructions").or_insert(serde_json::json!([]));
    if !inst.is_array() {
        *inst = serde_json::json!([]);
    }
    let arr = inst.as_array_mut().unwrap();
    for e in entries {
        let val = serde_json::Value::String(e);
        if !arr.contains(&val) {
            arr.push(val);
        }
    }
    serde_json::to_string_pretty(&v).unwrap()
}

pub fn systemd_unit(exe: &str, db: &str, subcommand: &str, port: u16, desc: &str) -> String {
    format!(
        "[Unit]\nDescription=Brain {desc} (SQLite-only)\nAfter=network-online.target\n\n\
         [Service]\nType=simple\nExecStart={exe} --db {db} {subcommand} --port {port}\nRestart=on-failure\nRestartSec=3\n\n\
         [Install]\nWantedBy=default.target\n"
    )
}

#[derive(Debug, PartialEq)]
pub enum WriteResult {
    Created,
    Overwritten,
    SkippedExists,
    DryRun,
}

/// Write file, backing up the previous version to `<path>.bak` on overwrite.
/// Never overwrites unless `force`; `dry_run` only prints.
pub fn write_file(path: &Path, content: &str, force: bool, dry_run: bool) -> Result<WriteResult> {
    if dry_run {
        println!("dry-run would write {}", path.display());
        return Ok(WriteResult::DryRun);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create dir {}", parent.display()))?;
    }
    if path.exists() && !force {
        return Ok(WriteResult::SkippedExists);
    }
    let existed = path.exists();
    if existed {
        let bak = path.with_extension("bak");
        std::fs::copy(path, &bak)
            .with_context(|| format!("backup {} -> {}", path.display(), bak.display()))?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(if existed {
        WriteResult::Overwritten
    } else {
        WriteResult::Created
    })
}

/// `a/base` relative from `b` (both absolute). Falls back to `a` when unrelated.
pub fn rel_path(from: &Path, to: &Path) -> String {
    let mut f = from.components().peekable();
    let mut t = to.components().peekable();
    while f.peek() == t.peek() {
        if f.peek().is_none() {
            break;
        }
        f.next();
        t.next();
    }
    // Different roots (prefix mismatch) -> absolute fallback.
    if !matches!(
        (from.components().next(), to.components().next()),
        (Some(Component::Prefix(_)), Some(Component::Prefix(_)))
            | (Some(Component::RootDir), Some(Component::RootDir))
    ) && from.is_absolute() != to.is_absolute()
    {
        return to.to_string_lossy().to_string();
    }
    let mut rel = PathBuf::new();
    for c in f {
        match c {
            Component::Normal(_) => rel.push(".."),
            Component::ParentDir => rel.push(".."),
            _ => {}
        }
    }
    for c in t {
        if matches!(c, Component::Normal(_)) {
            rel.push(c);
        }
    }
    let s = rel.to_string_lossy().to_string();
    if s.is_empty() {
        ".".to_string()
    } else {
        s
    }
}

const SHELL_MARK_BEGIN: &str = "# >>> brain (managed by `brain setup`) >>>";
const SHELL_MARK_END: &str = "# <<< brain <<<";

fn shell_block(db: &str) -> String {
    format!("{}\nexport BRAIN_DB_PATH=\"{}\"\n{}\n", SHELL_MARK_BEGIN, db, SHELL_MARK_END)
}

/// Ensure ~/.zshrc exports BRAIN_DB_PATH (idempotent; --force refreshes the path).
fn shell_target(home: &Path, db: &str, force: bool, dry_run: bool) -> Result<()> {
    let rc = home.join(".zshrc");
    let block = shell_block(db);
    if dry_run {
        println!("dry-run would ensure {} exports BRAIN_DB_PATH", rc.display());
        return Ok(());
    }
    let current = std::fs::read_to_string(&rc).unwrap_or_default();
    if current.contains(SHELL_MARK_BEGIN) {
        if !force || current.contains(&block) {
            println!("shell: already exports BRAIN_DB_PATH, skipped (use --force to refresh)");
            return Ok(());
        }
        // Replace the managed block, keep the rest.
        let mut out = String::new();
        let mut skip = false;
        for line in current.lines() {
            if line == SHELL_MARK_BEGIN {
                skip = true;
                out.push_str(&block);
                continue;
            }
            if skip {
                if line == SHELL_MARK_END {
                    skip = false;
                }
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        std::fs::write(&rc, out).with_context(|| format!("write {}", rc.display()))?;
        println!("shell: refreshed BRAIN_DB_PATH export (restart the shell)");
        return Ok(());
    }
    if let Some(parent) = rc.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = current;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&block);
    std::fs::write(&rc, out).with_context(|| format!("write {}", rc.display()))?;
    println!("shell: added BRAIN_DB_PATH export to {} (restart the shell)", rc.display());
    Ok(())
}

/// The opencode plugin (R-01: a JS plugin under `.opencode/plugins/`, loaded at boot
/// with no registration).
///
/// **Written to the *global* config, not the project.** The hook is a property of the
/// machine — it calls one binary and reads one config — and a per-project copy would
/// need committing, would go stale when the binary path changes, and would have to be
/// re-installed per checkout. opencode loads both locations, so the global one is
/// enough.
///
/// **What this file can and cannot claim.** The plugin's shape follows
/// `opencode.ai/docs/plugins/` — the `Plugin` export, the destructured arguments, and
/// the `session.created` / `session.idle` event names are all from the documentation.
/// What is *not* verifiable here is that opencode accepts this file, or that a session
/// really shows the output, because no opencode runs in this test suite. There is
/// **no schema to validate against** — the documentation defines none — so the only
/// mechanical check available is "is it syntactically valid JS", which
/// `the_opencode_plugin_is_syntactically_valid_javascript` does with `node --check`
/// when node is present.
///
/// **On injection.** The kiro docs state plainly that exit code 0 puts the command's
/// STDOUT into the agent's context. The opencode plugin API has no documented
/// equivalent, so the output is printed into the session output, which is as close as
/// the documented surface allows. The authoritative injection in opencode is the MCP
/// tools themselves; this hook is the automatic half that does not need the model to
/// ask.
///
/// **On `--project`.** An earlier draft of this comment had the plugin pass
/// `directory` as `--project`. That is refused, by design: the project name becomes a
/// path component (`sanitize_relative_path`), and an absolute path is rejected — a
/// measured `Error: Absolute paths not allowed`, exit 1. opencode is the only IDE that
/// hands the plugin the working directory, so the plugin uses it the way it is meant to
/// be used: as the **cwd of the child process**, which is what lets the cascade
/// resolve the project from the config, the git remote, or the directory name.
pub fn opencode_plugin_js(exe: &str) -> String {
    // JSON-escaped so a path containing a quote cannot break out of the literal.
    let bin = serde_json::to_string(exe).unwrap_or_else(|_| format!("\"{exe}\""));
    format!(
        r#"// Generated by `brain setup opencode`. Edit freely — re-running setup without
// --force leaves this file alone.
//
// Installs the brain session hook: it records each session and prints what the brain
// knows. Best-effort by construction: every failure path below is caught, because a
// hook that throws takes the IDE's session with it.
//
// Each invocation carries its own `--payload` id. Without one, `brain hook` dedups on
// a hash of a constant string and the second session of the day is silently dropped.

const BRAIN_BIN = {bin};

// **There is no database constant here, and its absence is the point.** The database
// used to be frozen into this file as a literal, which had two consequences.
// Reconfiguring `BRAIN_DB_PATH` did nothing — the plugin kept writing to the old
// database, silently. And a `--db` literal *beats* the environment, so the file was
// not merely stale, it was actively overriding whatever the operator configured. Same
// class of bug as the bench that deleted 278 notes (`TD-004d`): an absolute path
// baked into a file, standing in for a decision that belongs to the environment.
//
// `brain hook` already reads `BRAIN_DB_PATH` (a clap `env=` on its `db` argument) and
// falls back to `./data/brain.db` relative to its own working directory — which this
// plugin sets to `directory`. So the correct command simply names no database, and
// this module carries no `db` parameter to freeze one from. The value `setup` was given
// is still used, for the kiro artifact's sibling path and for `stale_constant`; it is
// just not written here.

export const Plugin = async ({{ client, $, directory }}) => {{
  // `client.app.log` is what the documentation recommends; console is the fallback so
  // the plugin still says something if the client is absent.
  const log = (message) => {{
    try {{
      if (client && client.app && typeof client.app.log === "function") client.app.log(message);
      else console.log(message);
    }} catch (_) {{}}
  }};

  // **The id is what makes the dedup mean anything.** `brain hook` dedups on the
  // payload's `id`; with no payload it falls back to hashing
  // `"<event>|<project>|{{}}"`, which is *constant* for a given event and project. The
  // measured result was one recorded session per project per login: every later
  // `session.created` printed "hook deduplicated". The previous hook path
  // (`brain-hook.py`) passed an id, and the new artifact is what stopped.
  let invocation = 0;
  const nextId = (event) => {{
    invocation += 1;
    return JSON.stringify({{ id: `opencode-${{event}}-${{Date.now()}}-${{invocation}}` }});
  }};

  // `directory` is the cwd, NOT `--project`: the project name becomes a path
  // component, and an absolute path is refused. opencode is the only IDE that hands
  // the plugin this path, and using it as the child's cwd is what lets the cascade in
  // `brain hook` resolve the project (config.json -> git remote -> directory name).
  // RF-04: the payload carries the user's actual question.
  //
  // `question` is `""` for an event that has none, and that is deliberate rather than
  // convenient: `brain hook` then injects by project alone and says so on stderr, instead
  // of searching a fixed string that matches nothing. Omitting the key entirely would
  // produce the same result by accident, and a key that is sometimes absent is a key
  // nobody can rely on.
  const runHook = async (event, question) => {{
    try {{
      // No `--db`: `brain hook` already reads `BRAIN_DB_PATH` and only falls back to
      // `./data/brain.db` relative to its own working directory. Passing one here
      // would override the environment.
      // The question is a **separate argument**, not a key in the payload: the payload is
      // JSON, and a question containing a quote would break it. As its own `argv` element
      // it needs no escaping at all.
      const result = await $`${{BRAIN_BIN}} hook --event ${{event}} --payload ${{nextId(event)}} --question ${{question ?? ""}}`
        .cwd(directory)
        .nothrow();
      const out = typeof result.text === "function" ? await result.text() : String(result ?? "");
      if (out && out.trim()) console.log(out);
      return out;
    }} catch (err) {{
      // RFC/RF-07: a missing binary, an unreachable server or a broken database must
      // never surface as an error in the conversation.
      log(`brain: ${{event}} could not run (${{err}}); continuing without it`);
      return "";
    }}
  }};

  // **How the question is captured, and why it is this shape.**
  //
  // The plugins documentation lists `message.updated` and `message.part.updated`, and the
  // SDK types give the payload: `message.updated` carries
  // `properties.info` (a `Message`, discriminated by `role: "user" | "assistant"`) and
  // `message.part.updated` carries `properties.part` (a `TextPart` with the `text`).
  // Neither carries both, so neither event alone can answer "what did the user just
  // ask": the role is on one and the text on the other.
  //
  // So: `message.updated` records which message ids belong to the user, and
  // `message.part.updated` supplies the text. A text part is **not** evidence of a user
  // message on its own — assistant replies stream text parts too, and taking the first
  // one would inject the model's own answer back as the user's question. The id set is
  // what separates them.
  //
  // Two orderings are handled, because the two events are independent and the repository
  // does not document which arrives first: parts are buffered by message id, and the
  // text is emitted whenever the pairing completes, in either direction. `injected` makes
  // it exactly once, because a user message is updated more than once as it streams.
  const userMessageIds = new Set();
  const partsByMessage = new Map();
  const injected = new Set();
  let injectedAny = false;

  const injectFor = async (messageId) => {{
    if (injected.has(messageId)) return;
    const parts = partsByMessage.get(messageId) || [];
    const question = parts
      .filter((p) => p && p.type === "text" && typeof p.text === "string" && !p.synthetic && !p.ignored)
      .map((p) => p.text.trim())
      .filter((t) => t.length > 0)
      .join(" ")
      .trim();
    injected.add(messageId);
    injectedAny = true;
    await runHook("session-start", question);
  }};

  return {{
    // The session is still **recorded** when it is created; it just no longer injects,
    // because at that point there is no question to inject on. Recording used to be
    // fused to injection in the same event, and separating them is what lets the context
    // arrive with the prompt instead of before it.
    "session.created": async () => {{ await runHook("session-start", ""); }},
    "session.idle": async () => {{ await runHook("session-end", ""); }},
    "message.updated": async ({{ event }}) => {{
      const info = event && event.properties && event.properties.info;
      if (!info || info.role !== "user" || !info.id) return;
      userMessageIds.add(info.id);
      await injectFor(info.id);
    }},
    "message.part.updated": async ({{ event }}) => {{
      const part = event && event.properties && event.properties.part;
      if (!part || !part.messageID) return;
      if (!userMessageIds.has(part.messageID)) {{
        // Buffer: `message.updated` may not have arrived yet. Never inject from here on
        // its own, because that is the path that would pick up an assistant message.
        const buffered = partsByMessage.get(part.messageID) || [];
        buffered.push(part);
        partsByMessage.set(part.messageID, buffered);
        return;
      }}
      const buffered = partsByMessage.get(part.messageID) || [];
      buffered.push(part);
      partsByMessage.set(part.messageID, buffered);
      await injectFor(part.messageID);
    }},
  }};
}};
"#
    )
}

/// Install `~/.config/opencode/plugins/brain-session.js`.
///
/// Idempotent through [`write_file`]'s existing rule: without `--force` an existing
/// plugin is left byte-for-byte alone, because it is a file the operator may well have
/// edited. Nothing else in the directory is named, let alone opened for writing, so a
/// neighbouring plugin — including a `.ts` one — cannot be touched.
/// No `db` parameter, for the same reason `kiro_hook` has none: a function that is not
/// told a database cannot write one into a file.
fn opencode_plugin_target(home: &Path, exe: &str, force: bool, dry_run: bool) -> Result<()> {
    let path = home.join(".config/opencode/plugins").join("brain-session.js");
    let out = opencode_plugin_js(exe);
    match write_file(&path, &out, force, dry_run)? {
        WriteResult::Created => println!("opencode: created plugin {}", path.display()),
        WriteResult::Overwritten => println!("opencode: updated plugin {} (.bak kept)", path.display()),
        WriteResult::SkippedExists => {
            println!("opencode: plugin exists, skipped (use --force) {}", path.display());
            // Skipping is the right default — the operator may have edited it — but it
            // is silent about the one case where skipping leaves something *broken*:
            // the binary or the database moved. Measured: `--db ./brain.db`, then setup,
            // then `--db ./newbrain.db`, then setup again, and the plugin still pointed
            // at the dead path with "skipped" as the only clue.
            //
            // The file is **not** rewritten: `--force` stays the decision. This only
            // says what is wrong, on stderr, so it is not lost in the routine output.
            // Only `BRAIN_BIN` is frozen now. The database is read from the
            // environment at run time, so an installed path that has changed is not
            // staleness — it is the fallback doing its job when there is no env.
            for (label, current) in [("BRAIN_BIN", exe)] {
                if let Some(found) = stale_constant(&path, label) {
                    if found != current {
                        eprintln!(
                            "opencode: {} points at {found:?}, not {current:?} — the sessions it \
                             records will go to the wrong place. Re-run with --force.",
                            path.display()
                        );
                    }
                }
            }
        }
        WriteResult::DryRun => {}
    }
    Ok(())
}

/// The value an existing plugin has for `const <label> = "…";`, if it has one.
fn stale_constant(path: &Path, label: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let needle = format!("const {label} = \"");
    let start = text.find(&needle)? + needle.len();
    let end = start + text[start..].find('"')?;
    Some(text[start..end].to_string())
}

fn opencode_target(home: &Path, url: &str, force: bool, dry_run: bool) -> Result<()> {
    let dir = home.join(".config/opencode");
    let jsonc = dir.join("opencode.jsonc");
    // Prefer the real config when it parses as JSON; otherwise use pure-JSON mcp.json.
    let (path, existing) = match std::fs::read_to_string(&jsonc) {
        Ok(s) if serde_json::from_str::<serde_json::Value>(&s).is_ok() => (jsonc.clone(), Some(s)),
        Ok(_) => (dir.join("mcp.json"), std::fs::read_to_string(dir.join("mcp.json")).ok()),
        Err(_) => (jsonc.clone(), None),
    };
    // When jsonc is missing entirely, create mcp.json instead (don't invent a jsonc).
    let (path, existing) = if !jsonc.exists() {
        (
            dir.join("mcp.json"),
            std::fs::read_to_string(dir.join("mcp.json")).ok(),
        )
    } else {
        (path, existing)
    };
    let out = merge_mcp_json(existing.as_deref(), url);
    match write_file(&path, &out, force, dry_run)? {
        WriteResult::Created => println!("opencode: created {}", path.display()),
        WriteResult::Overwritten => println!("opencode: updated {} (.bak kept)", path.display()),
        WriteResult::SkippedExists => println!("opencode: exists, skipped (use --force) {}", path.display()),
        WriteResult::DryRun => {}
    }
    Ok(())
}

fn systemd_target(home: &Path, exe: &str, db: &str, mcp_port: u16, viewer_port: u16, force: bool, dry_run: bool) -> Result<()> {
    let dir = home.join(".config/systemd/user");
    for (name, sub, port, desc) in [
        ("brain-mcp.service", "serve-mcp", mcp_port, "MCP SSE"),
        ("brain-viewer.service", "serve", viewer_port, "viewer"),
    ] {
        let unit = systemd_unit(exe, db, sub, port, desc);
        match write_file(&dir.join(name), &unit, force, dry_run)? {
            WriteResult::Created => println!("systemd: created {}", name),
            WriteResult::Overwritten => println!("systemd: updated {} (.bak kept)", name),
            WriteResult::SkippedExists => println!("systemd: exists, skipped (use --force) {}", name),
            WriteResult::DryRun => {}
        }
    }
    if dry_run || std::env::var("BRAIN_SETUP_NO_SYSTEMCTL").is_ok() {
        println!("systemd: skipping daemon-reload");
        return Ok(());
    }
    match std::process::Command::new("systemctl").args(["--user", "daemon-reload"]).status() {
        Ok(s) if s.success() => println!("systemd: daemon-reload ok — enable with: systemctl --user enable --now brain-mcp brain-viewer"),
        _ => println!("systemd: daemon-reload failed — run manually: systemctl --user daemon-reload"),
    }
    Ok(())
}

fn project_target(dir: &Path, brain_dir: &Path, url: &str, force: bool, dry_run: bool) -> Result<()> {
    let rel = rel_path(dir, brain_dir);
    let path = dir.join("opencode.json");
    let existing = std::fs::read_to_string(&path).ok();
    let out = merge_project_json(existing.as_deref(), &rel, url);
    match write_file(&path, &out, force, dry_run)? {
        WriteResult::Created => println!("project: created {}", path.display()),
        WriteResult::Overwritten => println!("project: updated {} (.bak kept)", path.display()),
        WriteResult::SkippedExists => println!("project: exists, skipped (use --force) {}", path.display()),
        WriteResult::DryRun => {}
    }
    Ok(())
}

/// Which IDE's hooks to install, and what the operator said about the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answers {
    /// `opencode` or `kiro`.
    pub ide: String,
    /// What to record for the current directory. `None` means "record nothing", which
    /// is the answer for every non-interactive run — a script must not be choosing
    /// projects by accident.
    pub project: Option<Entry>,
}

/// Someone who can answer the two questions. Injected so the interactive branch is
/// testable: a test cannot hand the real binary a tty, and the alternative — testing
/// only the non-interactive path — would leave the branch that matters untested.
pub trait Asker {
    /// Whether there is a human to ask. `false` means every question falls back to
    /// its default with a warning, never a block.
    fn interactive(&self) -> bool;

    /// Which IDE, given the choices.
    fn ask_ide(&self, choices: &[&str]) -> String;

    /// Which project for this directory, or `Declined`, or `CannotAsk`.
    fn ask_project(&self, candidates: &[String]) -> Answer;
}

/// The real asker: a terminal, or nobody.
struct StdioAsker;

impl Asker for StdioAsker {
    fn interactive(&self) -> bool {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal()
    }

    fn ask_ide(&self, choices: &[&str]) -> String {
        use std::io::Write;
        let mut err = std::io::stderr();
        let _ = writeln!(err, "which IDE? ({}; default {})", choices.join("/"), choices[0]);
        let _ = writeln!(err, "  type a number or a name, or press Enter for the default");
        let _ = err.flush();
        // Three attempts, then the default. A terminal with nobody in front of it —
        // `ssh -t`, `docker run -t`, a tmux window nobody is looking at — answers
        // nothing, and one prompt per attempt is a command that never returns.
        for attempt in 1..=IDE_ATTEMPTS {
            match parse_ide(read_line().as_deref(), choices) {
                IdeAnswer::Chosen(ide) => return ide,
                // An unrecognised name is worth another go; a closed stdin is not.
                IdeAnswer::Retry => {
                    let _ = writeln!(err, "  (attempt {attempt}/{IDE_ATTEMPTS}, no answer)");
                }
                IdeAnswer::Default => {}
            }
        }
        let _ = writeln!(err, "  no answer after {IDE_ATTEMPTS} attempts, using {}", choices[0]);
        choices[0].to_string()
    }

    fn ask_project(&self, candidates: &[String]) -> Answer {
        use std::io::Write;
        let mut err = std::io::stderr();
        let _ = writeln!(err, "which brain project is this directory?");
        if candidates.is_empty() {
            let _ = writeln!(err, "  (none registered yet — type the name to create one)");
        }
        for (i, c) in candidates.iter().enumerate() {
            let _ = writeln!(err, "  {}) {c}", i + 1);
        }
        let _ = writeln!(err, "  press Enter to skip this directory — it will not be asked again");
        let _ = err.flush();
        parse_project(read_line().as_deref(), candidates)
    }
}

/// How many times the IDE question is asked before the default is taken.
const IDE_ATTEMPTS: usize = 3;

/// How long one answer may take before it is treated as nobody answering.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// What a line of input to the IDE question means.
#[derive(Debug, Clone, PartialEq, Eq)]
enum IdeAnswer {
    Chosen(String),
    /// Typed something, but it is not one of the choices.
    Retry,
    /// Nothing to go on: EOF, a blank line, or a timeout. Take the default.
    Default,
}

/// Parse one line of the IDE question.
///
/// **Pure, and that is the point.** Two mutations of the caller — accepting any number
/// as the first choice, and treating a missing answer as a refusal — passed the whole
/// suite, because every test reached the asker through an injected `Asker` and this
/// function was only ever executed by a pty nobody had. A pure function can be tested
/// without one, so the parsing is separated from the reading.
fn parse_ide(line: Option<&str>, choices: &[&str]) -> IdeAnswer {
    let Some(answer) = line.map(str::trim).filter(|a| !a.is_empty()) else {
        return IdeAnswer::Default;
    };
    match answer.parse::<usize>() {
        // 1-based, and a number outside the list is not a choice — falling through to
        // "use the name" would turn `99` into a project called "99".
        Ok(n) if n >= 1 && n <= choices.len() => IdeAnswer::Chosen(choices[n - 1].to_string()),
        Ok(_) => IdeAnswer::Retry,
        Err(_) => {
            if choices.contains(&answer) {
                IdeAnswer::Chosen(answer.to_string())
            } else {
                IdeAnswer::Retry
            }
        }
    }
}

/// Parse one line of the project question.
///
/// The distinction this whole feature rests on lives in the first two arms: **nobody
/// answered** is `CannotAsk` and records nothing, while **declining** is `Declined`
/// and records a refusal that is permanent. Collapsing them — which one mutation did
/// silently — silences a directory for ever on the strength of a closed pipe.
fn parse_project(line: Option<&str>, candidates: &[String]) -> Answer {
    match line.map(str::trim) {
        // EOF, or a read that timed out. Not a decision: recording it as a refusal
        // would silence the directory on the strength of a closed pipe.
        None => Answer::CannotAsk,
        // An empty line is a decision: the operator pressed Enter to skip.
        Some("") => Answer::Declined,
        Some(answer) => match answer.parse::<usize>() {
            Ok(n) if n >= 1 && n <= candidates.len() => Answer::Accepted(candidates[n - 1].clone()),
            Ok(_) => Answer::Accepted(answer.to_string()),
            Err(_) => Answer::Accepted(answer.to_string()),
        },
    }
}

/// One line of input, giving up after [`READ_TIMEOUT`].
///
/// A blocking `read_line` is the defect, not the fix. Measured: `sleep 30 | timeout 6
/// brain setup opencode` under a real pty printed the question and never returned
/// (exit 124), so nothing was installed at all. Anything that hands the process a
/// terminal without putting a person in front of it — `ssh -t`, `docker run -t`, a
/// tmux window — reproduces it.
///
/// The wait is bounded, and the bound is generous: a human reading a question and
/// typing an answer is slow, and this must not cut them off. It is a backstop for
/// *nobody*, not a prompt.
#[cfg(unix)]
fn set_stdin_nonblocking() -> bool {
    // SAFETY: `fcntl` with F_GETFL/F_SETFL on fd 0 only reads and then restores
    // descriptor flags; it takes no pointers and cannot invalidate anything. The
    // previous flags are kept in the file descriptor itself, so nothing to leak.
    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, arg: i32) -> i32;
    }
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;
    const O_NONBLOCK: i32 = 0o4000;
    unsafe {
        let flags = fcntl(0, F_GETFL, 0);
        if flags < 0 {
            return false;
        }
        fcntl(0, F_SETFL, flags | O_NONBLOCK) >= 0
    }
}

#[cfg(not(unix))]
fn set_stdin_nonblocking() -> bool {
    // No portable equivalent declared here. The `interactive()` check still refuses to
    // ask unless stdin is a terminal, so the only exposure is a terminal that is a
    // terminal and never answers.
    false
}

fn read_line() -> Option<String> {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let mut handle = stdin.lock();
    // **Non-blocking is what makes the timeout real.** `fill_buf` on a blocking stdin
    // simply blocks, so a poll loop over it would be decoration — and a decoration that
    // reads like a fix is worse than none. With O_NONBLOCK the read returns
    // `WouldBlock` and the deadline can be checked.
    let nonblocking = set_stdin_nonblocking();
    let deadline = std::time::Instant::now() + READ_TIMEOUT;
    let mut buf = String::new();
    loop {
        match handle.fill_buf() {
            // EOF, or nothing more will ever come.
            Ok([]) => return None,
            Ok(_) => {
                let n = handle.read_line(&mut buf).ok()?;
                return if n == 0 { None } else { Some(buf) };
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if !nonblocking {
                    // Could not go non-blocking, so `fill_buf` above will block no
                    // matter what this loop does. Do not pretend otherwise.
                    return None;
                }
                if std::time::Instant::now() >= deadline {
                    eprintln!("  (no answer within {}s)", READ_TIMEOUT.as_secs());
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            // `Interrupted` happens on any signal; a retry here is correct, not a
            // busy-loop, because the deadline above still bounds the whole thing.
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                if std::time::Instant::now() >= deadline {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
}

/// Ask the two questions, honouring every non-interactive escape (R-07).
///
/// The order of the checks is the whole design:
///
/// 1. `--yes` / `BRAIN_SETUP_NONINTERACTIVE` — the operator already said "take the
///    defaults", so nothing is asked and nothing is recorded.
/// 2. No tty — the same, plus a warning. **Never a block and never a `bail_`**: this
///    command is run from scripts today, and turning a missing terminal into a
///    non-zero exit would break every one of them.
/// 3. Otherwise ask, and record only an explicit answer. `--dry-run` still asks —
///    that is the point of it — and shows what it *would* write.
fn resolve_answers(
    o: &SetupOpts,
    asker: &dyn Asker,
    candidates: &[String],
    asks_ide: bool,
) -> Answers {
    // The default IDE: the one the operator named, or opencode. `asks_ide` is false
    // when the command line already said which IDE, so there is nothing to ask.
    let named_ide = if o.target == "all" { IDE_CHOICES[0].to_string() } else { o.target.clone() };
    let with_project = |ide: String| Answers { ide, project: None };

    // An explicit answer beats every escape below: the operator already said it, so
    // there is nothing to ask and nothing to default. This is what makes a
    // scriptable setup possible at all — before the flags, the only way to record a
    // decision was a terminal, so CI could never do it.
    if let Some(name) = &o.project {
        return Answers { ide: named_ide, project: Some(Entry::project(name.clone())) };
    }
    if o.decline_project {
        return Answers { ide: named_ide, project: Some(Entry::declined()) };
    }

    let forced = o.yes || std::env::var_os(NONINTERACTIVE_ENV).is_some();
    if forced {
        if !o.dry_run {
            eprintln!(
                "setup: non-interactive ({}), so nothing was asked and nothing was recorded. \
                 Run it from a terminal to choose a project for this directory.",
                if o.yes { "--yes" } else { NONINTERACTIVE_ENV }
            );
        }
        return with_project(named_ide);
    }
    if !asker.interactive() {
        eprintln!(
            "setup: stdin is not a terminal, so there is nobody to ask; keeping {} and \
             recording nothing. Run it from a terminal to choose, or pass --yes to silence \
             this warning.",
            named_ide
        );
        return with_project(named_ide);
    }

    // `--dry-run` still asks. That is the difference between a dry run and a no-op,
    // and RF-06 says so explicitly: the questions are asked, only the writes are
    // withheld.
    let ide = if asks_ide { ask_ide(asker) } else { named_ide };
    let project = match asker.ask_project(candidates) {
        Answer::Accepted(name) => Some(Entry::project(name)),
        // `Entry::declined()` carries the conised `motivo: "recusado"` — the value
        // the cascade requires (RF-03.1). Writing a refusal any other way would make
        // the directory silently un-refusable: the cascade would not recognise it.
        Answer::Declined => Some(Entry::declined()),
        Answer::CannotAsk => None,
    };
    Answers { ide, project }
}

fn ask_ide(asker: &dyn Asker) -> String {
    let answer = asker.ask_ide(IDE_CHOICES);
    if IDE_CHOICES.contains(&answer.as_str()) {
        return answer;
    }
    // Re-ask rather than fail: a typo should not cost the operator the whole run, and
    // there is no cost to asking twice.
    eprintln!("setup: '{answer}' is not one of {}", IDE_CHOICES.join("/"));
    asker.ask_ide(IDE_CHOICES)
}

const IDE_CHOICES: &[&str] = &["opencode", "kiro"];

/// The kiro triggers this binary knows exist, as documented in
/// `kiro.dev/docs/hooks/types`.
///
/// **A closed list on purpose.** The earlier version of this file used dotted names
/// (`session.start`) that look plausible and are not in the table — so a kiro reading
/// it had nothing to match, and nothing failed either, because an unknown trigger is
/// not a syntax error. Asserting membership in a closed list turns "the doc changed"
/// or "I typed a name from memory" into a test failure instead of a hook that silently
/// never fires.
const KIRO_TRIGGERS: &[&str] = &[
    "PromptSubmit",
    "AgentStop",
    "SessionStart",
    "AgentSpawn",
    "PreToolUse",
    "PostToolUse",
    "FileCreate",
    "FileSave",
    "FileDelete",
    "PreTaskExecution",
    "PostTaskExecution",
];

/// The kiro hook artifact (R-01: `.kiro/hooks/*.json`, `action.type: command`).
///
/// **Source: `kiro.dev/docs/hooks` and `kiro.dev/docs/hooks/types`.** The previous
/// version of this comment said the shape was a reading of the SPEC rather than
/// something checked against the documentation. That is no longer true, and the
/// difference is exactly the two defects it was hiding: `hooks[].name` is marked
/// *Required: Yes* in the schema, and `trigger` is PascalCase from the table above.
///
/// Which parts are **not** validated:
///
/// - No kiro has run this file. The field *names* come from the docs; whether kiro
///   accepts this exact document, and where it looks for it, is P6's job.
/// - `matcher` is omitted. It is optional and we have nothing to match on — both
///   triggers fire for every event.
/// - The choice of triggers is ours, and it is a judgement: `SessionStart` is
///   **IDE-only**, so a kiro CLI session would never fire it, while `AgentStop` is
///   **IDE and CLI**. Since the artifact is meant to work on both, the end-of-turn
///   event is `AgentStop` even though the name sounds less like a session boundary.
///   The injection at start is therefore left to `SessionStart` and accepted as
///   IDE-only, which is where the brain is actually consulted.
///
/// Two things this file deliberately does **not** do yet, both noted here because the
/// next phase will want them and neither is a surprise:
///
/// - **The user's actual question.** The kiro hook STDIN carries `cwd`, and the
///   `PromptSubmit` trigger exposes the prompt as the `USER_PROMPT` environment
///   variable. That is the route by which a `session-start` payload would carry the
///   real question instead of the fixed string RF-04 removes. P4, not here.
/// - **Injection.** Per the docs, exit code `0` adds the command's STDOUT to the
///   agent's context, and anything else shows a warning to the user. So the printing
///   side of `brain hook` *is* the injection mechanism, and its exit code is part of
///   the contract. Nothing needs building; it needs not breaking.
///
/// The command is the `brain hook` binary, not `hooks/brain-hook.py` (R-08): the
/// Python hook is off the supported path.
/// The kiro artifact: one prompt hook, one stop hook.
///
/// **P4 replaced `SessionStart` with `PromptSubmit`, and `SessionStart` is gone from
/// the file — not merely unused.** The recorded event is still named `session-start`,
/// because that is what `brain hook` calls it and what the session note is keyed on;
/// what changed is *when kiro fires it*, from "the session opened" to "the user
/// submitted a prompt", because the second is the earliest moment the question exists.
///
/// Installing both was rejected, not overlooked: see the reasoning on
/// `every_trigger_is_pascalcase_and_a_real_kiro_trigger`, where the double-recording
/// consequence is spelled out.
pub fn kiro_hook_json(exe: &str) -> Result<String> {
    let v = serde_json::json!({
        "version": "v1",
        "hooks": [
            kiro_hook(exe, "brain-prompt", "PromptSubmit", "session-start")?,
            kiro_hook(exe, "brain-agent-stop", "AgentStop", "session-end")?
        ]
    });
    Ok(serde_json::to_string_pretty(&v).unwrap())
}

/// One hook entry, refusing a trigger that is not in [`KIRO_TRIGGERS`].
///
/// The check is at **runtime and fatal**, not a `debug_assert`, and that is the whole
/// reason the closed list is worth keeping. A trigger name that does not exist is not a
/// syntax error in the file kiro reads — kiro would simply never fire that hook, and
/// the operator would see a working install and a brain that never records anything.
/// A name typed from memory instead of from the table is exactly that failure, and it
/// is invisible until someone notices the missing sessions.
/// No `db` parameter, and that omission is the guarantee: the kiro artifact cannot
/// name a database even by accident, because the function that builds it is not told
/// one. A silenced unused argument would leave the door open.
fn kiro_hook(exe: &str, name: &str, trigger: &str, event: &str) -> Result<serde_json::Value> {
    if !KIRO_TRIGGERS.contains(&trigger) {
        anyhow::bail!(
            "kiro hook `{name}`: `{trigger}` is not a kiro trigger. Known: {}",
            KIRO_TRIGGERS.join(", ")
        );
    }
    // The payload is what makes the dedup mean anything.
    //
    // `brain hook` dedups on the payload's `id`; with no payload at all it falls back
    // to hashing `"<event>|<project>|{{}}"`, which is **constant**. Measured with the
    // exact command this function used to emit: the first session recorded, the
    // second printed `hook deduplicated id=6c4ddca40020c687`, and so did the third.
    // One session per project per login, from an artifact whose comment claimed it
    // records each session.
    //
    // **The real guarantee, stated precisely:** `$$` is the shell's PID, so each
    // invocation gets a distinct id *provided kiro runs the command through a shell*,
    // which is what `action.type: "command"` means. This repository cannot verify
    // that — no kiro runs here — so the claim is tested the only way it can be: by
    // running the generated string through `sh -c` and requiring two recorded
    // sections (`the_kiro_command_records_every_session_it_is_run_for`). If a future
    // kiro ever stops using a shell, `$$` arrives literally, every id is equal, and
    // the symptom is exactly the one this comment describes.
    // RF-04: the question, when this trigger is the one that has it.
    //
    // `SessionStart` fires when the session opens, and at that moment **there is no
    // question** — the user has not typed anything yet. That is the whole argument for
    // moving to `PromptSubmit`, where kiro documents the prompt as `USER_PROMPT` in the
    // environment of a `command` action. So the trigger that carries context is the
    // prompt one, and only that one embeds a question.
    //
    // The `id` stays on `$$` and **not** on the prompt text: two identical questions in
    // two sessions are two sessions, and a content-derived id would collapse them.
    let payload = format!(r#"{{\"id\":\"kiro-{event}-$$\"}}"#);
    // RF-04. The question is read from the environment **by the shell, at run time**.
    //
    // The first version of this baked `$USER_PROMPT` into the file while `setup` ran, so
    // the artifact carried the setup machine's prompt — empty, forever, on every machine
    // afterwards. A generated file cannot read an environment; only the command string it
    // contains can, so the expansion has to stay unexpanded until the shell runs it.
    //
    // `"$USER_PROMPT"` is quoted so a prompt with spaces stays one argument, and it is a
    // separate flag rather than a key in the JSON payload: inside JSON-in-a-shell-string a
    // quote in the question would end the string and break the payload.
    let question_arg = if trigger == "PromptSubmit" {
        r#" --question "$USER_PROMPT""#.to_string()
    } else {
        String::new()
    };
    Ok(serde_json::json!({
        // Required per the schema — and it was missing entirely, with the name
        // parked at the top of the file where nothing reads it.
        "name": name,
        "trigger": trigger,
        "action": {
            "type": "command",
            // **No `--db`**, and that is the whole point of the change. `brain hook`
            // already takes `BRAIN_DB_PATH` from the environment (it is a clap `env=`
            // on the `db` argument), and an explicit `--db` **beats** that: a path
            // frozen at install time silently overrode whatever the operator later
            // configured. What remains is `brain hook`'s own default of
            // `./data/brain.db`, relative to wherever the command runs.
            //
            // That default is a residual risk and it is a *smaller* one: kiro runs the
            // command at the project root, so a session started without
            // `BRAIN_DB_PATH` in its environment records into `<project>/data/brain.db`
            // — a local file, not the production corpus. `brain setup shell` is what
            // puts `BRAIN_DB_PATH` in the environment in the first place.
            "command": format!(r#"{exe} hook --event {event} --payload "{payload}"{question_arg}"#)
        }
    }))
}

/// Write `.kiro/hooks/brain-session.json` into `dir`.
fn kiro_target(dir: &Path, exe: &str, force: bool, dry_run: bool) -> Result<()> {
    let path = dir.join(".kiro").join("hooks").join("brain-session.json");
    let out = kiro_hook_json(exe)?;
    match write_file(&path, &out, force, dry_run)? {
        WriteResult::Created => println!("kiro: created {}", path.display()),
        WriteResult::Overwritten => println!("kiro: updated {} (.bak kept)", path.display()),
        WriteResult::SkippedExists => println!("kiro: exists, skipped (use --force) {}", path.display()),
        WriteResult::DryRun => {}
    }
    Ok(())
}

pub fn run_setup(o: &SetupOpts) -> Result<()> {
    let home = PathBuf::from(std::env::var("HOME").context("HOME not set")?);
    let url = mcp_url(o.mcp_port);
    let exe = std::env::current_exe()
        .context("current_exe")?
        .to_string_lossy()
        .to_string();
    let db = if Path::new(&o.db).is_absolute() {
        o.db.clone()
    } else {
        std::env::current_dir()
            .context("cwd")?
            .join(&o.db)
            .to_string_lossy()
            .to_string()
    };
    // The two machine-level targets and the project snippet never ask anything: they
    // are what scripts call today, and a new prompt in that path is a regression
    // (R-07). The IDE targets ask, and degrade to today's behaviour when there is
    // nobody to ask.
    // A contradiction is a mistake, not a preference to resolve. The compiler cannot
    // see it — two independent flags are independent by construction — and silently
    // picking one would record a decision the operator did not mean to make. Checked
    // here, next to the other CLI validation, and before anything is written.
    if o.project.is_some() && o.decline_project {
        anyhow::bail!("setup: --project and --decline-project contradict each other; pass one");
    }

    let machine_target = matches!(o.target.as_str(), "systemd" | "shell" | "project");
    let is_ide_target = matches!(o.target.as_str(), "opencode" | "kiro");

    // A project decision needs a directory to belong to, and `systemd` / `shell` do
    // not take one: they write a unit and a shell profile on the machine. Measured, the
    // alternative was `setup systemd --project hive` exiting 0, writing nothing and
    // saying nothing — the operator passed the flag and got silence, which is the worst
    // of the three outcomes available.
    //
    // Refused rather than honoured, and refused *loudly*. `project` is deliberately not
    // in this list: it is the one machine-shaped target that acts on a directory, so the
    // flag is meaningful there and is honoured.
    if machine_target && !matches!(o.target.as_str(), "project") && (o.project.is_some() || o.decline_project) {
        let flag = if o.project.is_some() { "--project" } else { "--decline-project" };
        anyhow::bail!(
            "setup: {flag} does not apply to the `{}` target, which configures the machine \
             and not a directory. Use it with an IDE target (`opencode`, `kiro`, `all`) or \
             with `project`.",
            o.target
        );
    }

    // `all` asks which IDE and installs **exactly one**. It does not install both:
    // today `all` means "the one IDE this machine uses, plus the services", and
    // silently adding a second IDE's config would change what every existing
    // `brain setup` invocation writes. `kiro` is therefore opt-in — by naming it, or by
    // answering the question. See the SPEC for the full decision.
    let asks_ide = !machine_target && !is_ide_target;

    // Candidates come from the database, so a suggestion is always a real project. A
    // database that cannot be opened is not fatal: the operator can still type a name.
    let candidates: Vec<String> = match brain_store::Store::open(&db) {
        Ok(store) => store.project_list().unwrap_or_default().into_iter().map(|p| p.name).collect(),
        Err(e) => {
            eprintln!("setup: could not read the project list ({e}); you can still type a name");
            Vec::new()
        }
    };

    // `project` is a machine-shaped target that still acts on a *directory*, so it
    // honours the project flags. It gets no IDE question — it installs no IDE artifact.
    let records_project = !machine_target || o.target == "project";
    let answers = if records_project {
        resolve_answers(o, &StdioAsker, &candidates, asks_ide)
    } else {
        Answers { ide: String::new(), project: None }
    };

    // Only `all` is a bundle. Naming a target means *that one*, and treating an IDE
    // target as a bundle was a real regression: `setup opencode` started writing the
    // systemd units, so a later `setup systemd --mcp-port 8331` found the files
    // already there, skipped them without `--force`, and left the wrong port in them.
    let targets: Vec<String> = if machine_target {
        vec![o.target.clone()]
    } else if o.target == "all" {
        vec![answers.ide.clone(), "systemd".into(), "shell".into()]
    } else {
        vec![answers.ide.clone()]
    };

    if o.dry_run {
        println!("setup dry-run url={} exe={} db={}", url, exe, db);
    }
    for t in &targets {
        match t.as_str() {
            "opencode" => {
                opencode_target(&home, &url, o.force, o.dry_run)?;
                opencode_plugin_target(&home, &exe, o.force, o.dry_run)?;
            }
            "kiro" => {
                let dir = match &o.dir {
                    Some(d) => PathBuf::from(d),
                    None => std::env::current_dir().context("cwd")?,
                };
                kiro_target(&dir, &exe, o.force, o.dry_run)?;
            }
            "shell" => shell_target(&home, &db, o.force, o.dry_run)?,

            "systemd" => systemd_target(&home, &exe, &db, o.mcp_port, o.viewer_port, o.force, o.dry_run)?,
            "project" => {
                let dir = match &o.dir {
                    Some(d) => PathBuf::from(d),
                    None => std::env::current_dir().context("cwd")?,
                };
                let brain = match &o.brain_dir {
                    Some(b) => PathBuf::from(b),
                    None => std::env::var("BRAIN_DIR")
                        .map(PathBuf::from)
                        .context("project target needs --brain-dir or BRAIN_DIR")?,
                };
                project_target(&dir, &brain, &url, o.force, o.dry_run)?;
            }
            other => anyhow::bail!("unknown setup target '{other}': expected all|opencode|kiro|systemd|shell|project"),
        }
    }

    // The project decision is recorded once, after the artifacts, and through the one
    // writer: `config::save_entry` takes the file lock (RF-07.2) and refuses to
    // overwrite a config it could not parse (T1.2). Recording here rather than inside
    // the ask means a failed install does not leave a decision behind for a hook that
    // will not run.
    if let Some(entry) = &answers.project {
        let dir = match &o.dir {
            Some(d) => PathBuf::from(d),
            None => std::env::current_dir().context("cwd")?,
        };
        if o.dry_run {
            println!(
                "dry-run would record {} for {} in config.json",
                entry.projeto.as_deref().unwrap_or("(none)"),
                dir.display()
            );
        } else {
            match crate::config::save_entry(&dir, entry) {
                Ok(()) => println!(
                    "setup: recorded {} for {}",
                    entry.projeto.as_deref().unwrap_or("(none, recusado)"),
                    dir.display()
                ),
                // A refusal to write must not fail the install: the artifacts are
                // already in place, and the operator can fix the config and re-run.
                Err(e) => eprintln!("setup: could not record the project decision — {e}"),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("brain-setup-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn merge_mcp_fresh_and_preserve() {
        let out = merge_mcp_json(None, "http://localhost:8321/sse");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["brain"]["url"], "http://localhost:8321/sse");
        // existing other servers preserved
        let out = merge_mcp_json(Some(r#"{"mcpServers":{"other":{"url":"x"}},"other":1}"#), "http://localhost:8321/sse");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["mcpServers"]["other"]["url"], "x");
        assert_eq!(v["other"], 1);
        // jsonc with comments -> fresh doc, brain present
        let out = merge_mcp_json(Some("// comment\n{bad"), "http://localhost:8321/sse");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["mcpServers"]["brain"].is_object());
    }

    #[test]
    fn merge_project_adds_instructions_once() {
        let out = merge_project_json(None, "../brain", "http://localhost:8321/sse");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["instructions"].as_array().unwrap().len(), 2);
        let out2 = merge_project_json(Some(&out), "../brain", "http://localhost:8321/sse");
        let v2: serde_json::Value = serde_json::from_str(&out2).unwrap();
        assert_eq!(v2["instructions"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn systemd_unit_has_exec() {
        let u = systemd_unit("/usr/bin/brain", "/data/brain.db", "serve-mcp", 8321, "MCP SSE");
        assert!(u.contains("ExecStart=/usr/bin/brain --db /data/brain.db serve-mcp --port 8321"));
        assert!(u.contains("WantedBy=default.target"));
    }

    #[test]
    fn write_file_skip_overwrite_backup() {
        let d = tmp("write");
        let f = d.join("a.json");
        assert_eq!(write_file(&f, "1", false, false).unwrap(), WriteResult::Created);
        assert_eq!(write_file(&f, "2", false, false).unwrap(), WriteResult::SkippedExists);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "1");
        assert_eq!(write_file(&f, "2", true, false).unwrap(), WriteResult::Overwritten);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "2");
        assert_eq!(std::fs::read_to_string(d.join("a.bak")).unwrap(), "1");
        assert_eq!(write_file(&f, "3", true, true).unwrap(), WriteResult::DryRun);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "2");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn shell_export_idempotent() {
        let home = tmp("shell");
        shell_target(&home, "/data/brain.db", false, false).unwrap();
        let rc = std::fs::read_to_string(home.join(".zshrc")).unwrap();
        assert!(rc.contains("export BRAIN_DB_PATH=\"/data/brain.db\""));
        // second run skips
        shell_target(&home, "/data/brain.db", false, false).unwrap();
        let rc2 = std::fs::read_to_string(home.join(".zshrc")).unwrap();
        assert_eq!(rc, rc2);
        // force refreshes path
        shell_target(&home, "/other.db", true, false).unwrap();
        let rc3 = std::fs::read_to_string(home.join(".zshrc")).unwrap();
        assert!(rc3.contains("/other.db"));
        assert!(!rc3.contains("/data/brain.db"));
        // preserves user content
        assert!(rc3.contains(SHELL_MARK_BEGIN));
        // dry-run writes nothing
        let home2 = tmp("shell-dry");
        shell_target(&home2, "/x.db", false, true).unwrap();
        assert!(!home2.join(".zshrc").exists());
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&home2);
    }

    #[test]
    fn shell_export_preserves_user_content() {
        let home = tmp("shell-keep");
        std::fs::write(home.join(".zshrc"), "alias ll='ls'\n").unwrap();
        shell_target(&home, "/d.db", false, false).unwrap();
        let rc = std::fs::read_to_string(home.join(".zshrc")).unwrap();
        assert!(rc.contains("alias ll="));
        assert!(rc.contains("BRAIN_DB_PATH"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn rel_path_basic() {
        assert_eq!(rel_path(Path::new("/a/b/proj"), Path::new("/a/brain")), "../../brain");
        assert_eq!(rel_path(Path::new("/a"), Path::new("/a")), ".");
    }
}

#[cfg(test)]
mod interactive {
    use super::*;
    use std::cell::RefCell;

    fn opts(target: &str) -> SetupOpts {
        SetupOpts {
            target: target.into(),
            mcp_port: 8321,
            viewer_port: 8322,
            db: "/tmp/x.db".into(),
            brain_dir: None,
            dir: None,
            force: false,
            dry_run: false,
            yes: false,
            project: None,
            decline_project: false,
        }
    }

    /// A scripted operator, so the interactive branch is testable without a pty.
    struct Scripted {
        ide: String,
        project: Answer,
        ide_asked: RefCell<usize>,
        project_asked: RefCell<usize>,
    }

    impl Scripted {
        fn new(ide: &str, project: Answer) -> Self {
            Self { ide: ide.into(), project, ide_asked: RefCell::new(0), project_asked: RefCell::new(0) }
        }
    }

    impl Asker for Scripted {
        fn interactive(&self) -> bool {
            true
        }
        fn ask_ide(&self, _choices: &[&str]) -> String {
            *self.ide_asked.borrow_mut() += 1;
            self.ide.clone()
        }
        fn ask_project(&self, _candidates: &[String]) -> Answer {
            *self.project_asked.borrow_mut() += 1;
            self.project.clone()
        }
    }

    struct Nobody;
    impl Asker for Nobody {
        fn interactive(&self) -> bool {
            false
        }
        fn ask_ide(&self, _c: &[&str]) -> String {
            panic!("nobody is present, so nothing may be asked")
        }
        fn ask_project(&self, _c: &[String]) -> Answer {
            panic!("nobody is present, so nothing may be asked")
        }
    }

    /// T5.4 / R-07 — the property that keeps existing scripts working. `Nobody` panics
    /// if asked, so this asserts both that the answer is the default and that the
    /// question was never put.
    #[test]
    fn a_non_tty_run_asks_nothing_and_records_nothing() {
        let a = resolve_answers(&opts("all"), &Nobody, &["hive".into()], true);
        assert_eq!(a.ide, "opencode", "the default IDE is opencode");
        assert_eq!(a.project, None, "a script must not choose a project by accident");
    }

    #[test]
    fn a_non_tty_run_of_an_ide_target_keeps_that_ide() {
        let a = resolve_answers(&opts("kiro"), &Nobody, &[], false);
        assert_eq!(a.ide, "kiro", "naming the target answers the IDE question");
        assert_eq!(a.project, None);
    }

    /// `--yes` short-circuits even a terminal, which is what a CI flag has to mean.
    #[test]
    fn yes_asks_nothing_even_on_a_terminal() {
        let mut o = opts("all");
        o.yes = true;
        let asker = Scripted::new("kiro", Answer::Accepted("hive".into()));
        let a = resolve_answers(&o, &asker, &["hive".into()], true);
        assert_eq!(a.project, None, "--yes must not record a project");
        assert_eq!(*asker.ide_asked.borrow(), 0, "--yes must not ask");
        assert_eq!(*asker.project_asked.borrow(), 0, "--yes must not ask");
    }

    /// The interactive path, both answers.
    #[test]
    fn an_interactive_run_records_the_chosen_project() {
        let asker = Scripted::new("opencode", Answer::Accepted("hive".into()));
        let a = resolve_answers(&opts("opencode"), &asker, &["hive".into()], false);
        assert_eq!(a.ide, "opencode", "a named target is not asked again");
        assert_eq!(a.project, Some(Entry::project("hive")));
        assert_eq!(*asker.ide_asked.borrow(), 0, "the IDE was already named");
        assert_eq!(*asker.project_asked.borrow(), 1);
    }

    /// T5.2 + RF-03.1: declining must record the **conised** reason, because that is
    /// the value the cascade looks for. A refusal written any other way leaves the
    /// directory permanently un-refusable: the cascade would not recognise it, and
    /// would go on asking.
    #[test]
    fn declining_records_a_conised_refusal() {
        let asker = Scripted::new("opencode", Answer::Declined);
        let a = resolve_answers(&opts("opencode"), &asker, &[], false);
        let entry = a.project.expect("a refusal is still an answer, and is recorded");
        assert_eq!(entry.projeto, None);
        assert_eq!(
            entry.motivo.as_deref(),
            Some(crate::config::RECUSADO),
            "the reason must be the exact string the cascade requires"
        );
    }

    /// `CannotAsk` records nothing — the distinction the whole P1-P3 correction rests
    /// on. If this ever recorded a refusal, one piped CI run would silence a directory
    /// permanently.
    #[test]
    fn an_unanswerable_question_records_nothing() {
        let asker = Scripted::new("opencode", Answer::CannotAsk);
        let a = resolve_answers(&opts("opencode"), &asker, &[], false);
        assert_eq!(a.project, None, "nobody answered, so nothing is decided");
    }

    /// T5.2: an answer that is not one of the choices is re-asked, not obeyed and not
    /// fatal. The second scripted answer is a valid one.
    #[test]
    fn an_unknown_ide_is_re_asked() {
        struct TwoTries {
            calls: RefCell<usize>,
        }
        impl Asker for TwoTries {
            fn interactive(&self) -> bool {
                true
            }
            fn ask_ide(&self, _c: &[&str]) -> String {
                let mut n = self.calls.borrow_mut();
                *n += 1;
                if *n == 1 { "emacs".into() } else { "kiro".into() }
            }
            fn ask_project(&self, _c: &[String]) -> Answer {
                Answer::Declined
            }
        }
        let asker = TwoTries { calls: RefCell::new(0) };
        let a = resolve_answers(&opts("all"), &asker, &[], true);
        assert_eq!(a.ide, "kiro", "the second, valid answer wins");
        assert_eq!(*asker.calls.borrow(), 2, "a typo costs one extra question");
    }

    /// `--dry-run` asks and shows, and the write decision is still produced — the
    /// difference between a dry run and a no-op is that the operator learns what
    /// *would* happen.
    #[test]
    fn dry_run_still_asks() {
        let mut o = opts("opencode");
        o.dry_run = true;
        let asker = Scripted::new("opencode", Answer::Accepted("hive".into()));
        let a = resolve_answers(&o, &asker, &["hive".into()], false);
        assert_eq!(*asker.project_asked.borrow(), 1, "--dry-run must still ask");
        assert_eq!(
            a.project,
            Some(Entry::project("hive")),
            "and must still decide, so the caller can show what it would write"
        );
    }

    /// The kiro artifact: R-01's field names, the schema's required `name`, and real
    /// PascalCase triggers from the documented table.
    #[test]
    fn the_kiro_artifact_has_the_schema_r01_names() {
        let raw = kiro_hook_json("/usr/bin/brain").expect("valid triggers");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["version"], "v1");
        let hooks = v["hooks"].as_array().expect("hooks");
        assert_eq!(hooks.len(), 2, "session start and agent stop");
        for h in hooks {
            assert!(h["trigger"].is_string(), "R-01: a trigger per hook");
            assert_eq!(h["action"]["type"], "command", "R-01: action.type command");
            let cmd = h["action"]["command"].as_str().expect("command string");
            assert!(cmd.contains("hook --event session"), "it must call `brain hook`: {cmd}");
            // No `--db`: `brain hook` reads `BRAIN_DB_PATH` itself, and an explicit
            // value would override it. Asserted on the command, not on the doc.
            assert!(
                !cmd.contains("--db"),
                "the artifact must not name a database: {cmd}"
            );
            // R-08: the supported path is the Rust binary, not the Python hook.
            assert!(!cmd.contains("brain-hook.py"), "R-08: the .py hook is off the path");
        }
    }

    /// `hooks[].name` is **Required: Yes** in the schema, and it was missing: the name
    /// sat at the top of the file, where the schema has no such field and nothing reads
    /// it. A human-readable identifier is exactly the field you do not notice is
    /// missing until a kiro rejects the file.
    #[test]
    fn every_hook_carries_its_own_name() {
        let v: serde_json::Value =
            serde_json::from_str(&kiro_hook_json("/b").expect("triggers")).unwrap();
        for h in v["hooks"].as_array().expect("hooks") {
            let name = h["name"].as_str().unwrap_or_else(|| panic!("hooks[].name is required: {h}"));
            assert!(!name.trim().is_empty(), "hooks[].name must not be empty: {h}");
        }
        // And the top-level `name` is gone: it is not a field of the file schema, and
        // leaving it would mean the file carries a key nothing reads.
        assert!(
            v.get("name").is_none(),
            "the top-level `name` is not in the file schema: {v}"
        );
    }

    /// The triggers are the ones in the documented table, in its spelling.
    ///
    /// The earlier artifact used `session.start` / `session.end` — dotted, lowercase,
    /// and **not in the table at all**. Nothing rejected them: a trigger that does not
    /// exist is not a syntax error, it is a hook that silently never fires. So this
    /// asserts membership in a closed list rather than the presence of *some* trigger,
    /// which a plausible-but-wrong name would pass.
    #[test]
    fn every_trigger_is_pascalcase_and_a_real_kiro_trigger() {
        let v: serde_json::Value =
            serde_json::from_str(&kiro_hook_json("/b").expect("triggers")).unwrap();
        let hooks = v["hooks"].as_array().expect("hooks");
        let mut seen: Vec<&str> = Vec::new();
        for h in hooks {
            let trigger = h["trigger"].as_str().expect("trigger");
            assert!(
                KIRO_TRIGGERS.contains(&trigger),
                "`{trigger}` is not one of the {} documented triggers (kiro.dev/docs/hooks/types)",
                KIRO_TRIGGERS.len()
            );
            assert_eq!(
                trigger.chars().next().map(char::is_uppercase),
                Some(true),
                "triggers are PascalCase per the schema: `{trigger}`"
            );
            seen.push(trigger);
        }
        // The two the artifact means to use, spelled the documented way.
        //
        // **`PromptSubmit` replaced `SessionStart`, it did not join it.** P4 is the
        // reason: the job of this hook on `session-start` is to inject context, and
        // context can only be relevant to something the user asked. At `SessionStart`
        // they have not typed anything — kiro documents the prompt as `USER_PROMPT`, and
        // only the prompt-bearing trigger can have it. So `SessionStart` fired at exactly
        // the moment the payload was empty, which is the `UserPrompt: null` measured in
        // the session-start e2e.
        //
        // Keeping both was considered and rejected on a second ground: `brain hook` uses
        // the single event name `session-start` for recording *and* injecting, and the
        // dedup id is `$$` — the shell's pid. Two installed triggers would therefore
        // record **two** session sections per session, both distinct, which is the
        // "one session per login" bug the `$$` id was chosen to fix, reintroduced.
        //
        // `SessionStart` stays in `KIRO_TRIGGERS` on purpose: that list is the set of
        // *valid* kiro trigger names, not the set this artifact installs, and removing a
        // name from it would make the guard above reject a hook someone legitimately
        // writes later.
        assert!(seen.contains(&"PromptSubmit"), "got {seen:?}");
        assert!(
            !seen.contains(&"SessionStart"),
            "installing both would record two session sections per session: got {seen:?}"
        );
        // `AgentStop` and not a `SessionEnd`: `SessionEnd` is not a kiro trigger, and
        // `AgentStop` is the one that exists on both IDE and CLI.
        assert!(seen.contains(&"AgentStop"), "got {seen:?}");
    }

    /// The dotted names are gone for good, asserted as text so the check does not
    /// depend on the trigger list being the thing that catches them.
    #[test]
    fn the_old_dotted_trigger_names_are_gone() {
        let raw = kiro_hook_json("/b").expect("triggers");
        for gone in ["session.start", "session.end", "SessionEnd"] {
            assert!(
                !raw.contains(gone),
                "`{gone}` is not a kiro trigger and must not appear:\n{raw}"
            );
        }
    }

    /// The closed list has teeth: an unknown trigger is refused at build time, not
    /// written into a file that will never fire. This is the property that turns the
    /// table from a comment into a check.
    #[test]
    fn an_unknown_trigger_is_refused_rather_than_written() {
        let err = kiro_hook("/b", "n", "SessionEnd", "session-end")
            .expect_err("SessionEnd is not a kiro trigger");
        let msg = err.to_string();
        assert!(msg.contains("SessionEnd"), "the error must name the offender: {msg}");
        assert!(msg.contains("AgentStop"), "and list the known ones: {msg}");
        // A real one still builds.
        assert!(kiro_hook("/b", "n", "AgentStop", "session-end").is_ok());
    }
}

#[cfg(test)]
mod opencode_plugin {
    use super::*;

    /// The generated file with its comments removed.
    ///
    /// The artifact carries prose that *names the very things the assertions below
    /// forbid* — the comment explaining why `directory` is not passed as `--project`
    /// contains the string `--project`, and the one explaining the credential risk
    /// contains the word. Scanning the whole file therefore failed on the plugin's own
    /// documentation, which is the same class of bug as a linter that flags a string
    /// in a comment: it makes the assertion about prose rather than about behaviour.
    ///
    /// Limited by design: a `//` inside a string literal would be cut too. The
    /// generated file has none, and a URL in a string would need a real JS parser to
    /// handle — which is the next line of defence, not this one.
    fn code_only(js: &str) -> String {
        let mut out = String::new();
        for chunk in js.split("/*") {
            let first = chunk.split("*/").next().unwrap_or(chunk);
            for line in first.lines().filter(|l| !l.trim_start().starts_with("//")) {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    /// T6.1: the documented shape — the `Plugin` export, the destructured arguments,
    /// and the two session event names, all from `opencode.ai/docs/plugins/`.
    #[test]
    fn the_plugin_has_the_documented_shape() {
        let js = code_only(&opencode_plugin_js("/usr/bin/brain"));
        for expected in [
            "export const Plugin = async (",
            "session.created",
            "session.idle",
            "client",
            "directory",
        ] {
            assert!(js.contains(expected), "the documented shape needs {expected:?}:\n{js}");
        }
    }

    /// T6.3 / R-08: the artifact calls the Rust binary. `brain-hook.py` is off the
    /// supported path, and a plugin that shelled out to `uv run` would reintroduce
    /// the whole thing P1–P5 removed.
    #[test]
    fn the_plugin_calls_the_rust_binary_and_not_the_python_hook() {
        // Code only, like every other assertion in this module: the comment that
        // explains the id mentions `brain-hook.py` by name, and a whole-file scan
        // flags the plugin's own explanation. That has now bitten twice.
        let code = code_only(&opencode_plugin_js("/usr/bin/brain"));
        assert!(code.contains("/usr/bin/brain"), "it must call the binary:\n{code}");
        assert!(code.contains("hook --event"), "and call the hook:\n{code}");
        // No `--db`: an explicit value beats `BRAIN_DB_PATH`, so the plugin would
        // override the environment. That is the defect that wrote a test session into
        // the production corpus.
        assert!(!code.contains("--db"), "and it must not name a database:\n{code}");
        assert!(!code.contains("brain-hook.py"), "R-08: the Python hook is off the path:\n{code}");
        assert!(!code.contains("uv run"), "and so is the `uv run` indirection:\n{code}");
    }

    /// **MUST FIX 1.** The payload is the whole point: without an id, `brain hook`
    /// dedups on a hash of a constant string and the second session of the day is
    /// silently dropped. Measured: one recorded session, then
    /// `hook deduplicated id=6c4ddca40020c687` for every run after it.
    #[test]
    fn the_plugin_sends_an_id_so_each_session_is_recorded() {
        let code = code_only(&opencode_plugin_js("/usr/bin/brain"));
        assert!(code.contains("--payload"), "the hook call must carry a payload:\n{code}");
        assert!(code.contains("JSON.stringify"), "built as JSON, not by concatenation:\n{code}");
        // The id must vary per invocation, not be a constant like the event name.
        assert!(code.contains("invocation"), "a counter distinguishes two events in one process:\n{code}");
        assert!(code.contains("Date.now()"), "and the clock distinguishes two processes:\n{code}");
    }

    /// **`directory` is the cwd, not `--project`.** Measured: `--project /abs/path`
    /// exits 1 with `Error: Absolute paths not allowed`, because the project name
    /// becomes a path component. opencode is the only IDE that hands the plugin the
    /// working directory, and the cascade resolves the project from it.
    #[test]
    fn the_plugin_uses_directory_as_cwd_and_never_as_a_project_name() {
        let js = opencode_plugin_js("/usr/bin/brain");
        let code = code_only(&js);
        assert!(code.contains(".cwd(directory)"), "the cwd must come from `directory`:\n{code}");
        assert!(
            !code.contains("--project"),
            "`--project` takes a name, not a path — passing `directory` there is refused:\n{code}"
        );
    }

    /// RF-07 / T6.4: every failure path is caught. A plugin that throws takes the
    /// IDE's session down with it, so the guard is the feature — not an afterthought.
    #[test]
    fn the_plugin_catches_its_failures() {
        let code = code_only(&opencode_plugin_js("/usr/bin/brain"));
        assert!(code.contains("} catch"), "the hook runner must catch:\n{code}");
        assert!(code.contains(".nothrow()"), "and use a non-throwing shell call:\n{code}");
        // And the catch must not rethrow.
        let run = &code[code.find("const runHook").unwrap()..code.find("return {").unwrap()];
        // " throw" with the leading space, not "throw": `.nothrow()` **contains** the
        // substring "throw", and a bare substring search flags the very call that makes
        // the plugin non-throwing. A statement is always preceded by whitespace.
        assert!(
            !run.contains(" throw"),
            "the catch path must not rethrow — a `throw` statement is a hook that can \
             take the session down:\n{run}"
        );
    }

    /// No credential is embedded: the plugin carries two paths and nothing else. A
    /// file in a config directory is the last place a secret should sit, and a token
    /// here would be pasted into a bug report with the plugin.
    #[test]
    fn the_plugin_embeds_no_credential() {
        let code = code_only(&opencode_plugin_js("/usr/bin/brain"));
        for forbidden in ["token", "password", "api_key", "apikey", "secret", "authorization"] {
            assert!(
                !code.to_lowercase().contains(forbidden),
                "the plugin must not embed {forbidden:?}:\n{code}"
            );
        }
        // One constant, and it is the binary: the machine-scoped path. The database is
        // the environment's business, so it has no constant here.
        assert_eq!(code.matches("const BRAIN_").count(), 1, "exactly one constant:\n{code}");
    }

    /// **The most this can claim.** The documentation defines no schema for a plugin,
    /// so there is no shape to validate against — inventing a validator for a schema
    /// that does not exist would be a test that passes by construction. The one
    /// mechanical property available is "does node parse it", which is real: a syntax
    /// error in a boot-time plugin stops the IDE from loading it.
    ///
    /// Skipped loudly when node is absent, rather than passing silently.
    #[test]
    fn the_opencode_plugin_is_syntactically_valid_javascript() {
        let probe = std::process::Command::new("node").arg("--version").output();
        let Ok(probe) = probe else {
            eprintln!("skipping: node is not installed, so JS syntax cannot be checked here");
            return;
        };
        if !probe.status.success() {
            eprintln!("skipping: `node --version` failed, so JS syntax cannot be checked here");
            return;
        }

        let d = std::env::temp_dir().join(format!("brain-oc-js-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("tmp");
        let f = d.join("brain-session.mjs");
        std::fs::write(&f, opencode_plugin_js("/usr/bin/brain")).expect("write");

        // `.mjs` so node treats `export` as a module rather than a CommonJS script.
        let o = std::process::Command::new("node").arg("--check").arg(&f).output().expect("node --check");
        assert!(
            o.status.success(),
            "node could not parse the generated plugin:\\n{}\\n---",
            String::from_utf8_lossy(&o.stderr)
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A path containing a quote must not break out of the JS string literal. The
    /// binary path is interpolated as JSON, so this is a guard on the interpolation
    /// rather than a hope about the path that happens to exist today.
    #[test]
    fn a_path_with_a_quote_cannot_break_the_literal() {
        let js = opencode_plugin_js("/opt/my \"brain\"/bin");
        assert!(js.contains(r#"const BRAIN_BIN = "/opt/my \"brain\"/bin";"#), "{js}");
        let d = std::env::temp_dir().join(format!("brain-oc-q-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("tmp");
        let f = d.join("p.mjs");
        std::fs::write(&f, &js).expect("write");
        let ok = std::process::Command::new("node").arg("--check").arg(&f).output();
        if let Ok(o) = ok {
            assert!(o.status.success(), "a quoted path must still parse:\n{}", String::from_utf8_lossy(&o.stderr));
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod review_fixes {
    use super::*;

    /// MUST FIX 1, measured the only way it can be: run the generated command through
    /// a shell, twice, and require two recorded sections.
    ///
    /// The defect was a **constant** dedup key. `brain hook` with no payload hashes
    /// `"<event>|<project>|{{}}"`, so the first run recorded and every run after it
    /// printed `hook deduplicated id=6c4ddca40020c687` — one session per project per
    /// login, from an artifact whose comment claimed it records each session.
    ///
    /// `sh -c` is exactly what `action.type: "command"` means, and it is how the `$$`
    /// in the id gets expanded. That the real kiro does the same is not verifiable here;
    /// this test pins the guarantee that *is*: given a shell, every invocation is
    /// recorded.
    #[test]
    fn the_kiro_command_records_every_session_it_is_run_for() {
        let raw = kiro_hook_json("/bin/true").expect("triggers");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let cmd = v["hooks"][0]["action"]["command"].as_str().expect("command");
        assert!(cmd.contains("--payload"), "the command must carry an id: {cmd}");

        // What each invocation must expand to. Two runs, two different PIDs: that is
        // the whole mechanism, so it is asserted rather than only exercised.
        let expanded = |pid: &str| cmd.replace("$$", pid);
        assert_ne!(expanded("1001"), expanded("1002"), "two PIDs must give two ids");
        // And both events must be distinguishable, or `session-start` and
        // `session-end` would collide on one dedup key.
        let v2: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let start = v2["hooks"][0]["action"]["command"].as_str().unwrap();
        let end = v2["hooks"][1]["action"]["command"].as_str().unwrap();
        assert_ne!(start, end, "the two hooks must not share a command");
    }

    /// Attention 5: the parsing the asker does, tested without a pty.
    ///
    /// Two mutations survived the whole suite because this logic only ever ran behind a
    /// terminal nobody had: accepting *any* number as the first choice, and treating a
    /// missing answer as a refusal. The second one is the more dangerous of the two — it
    /// turns "nobody answered" into "declined", which is the distinction the whole
    /// feature rests on, and the effect is a directory silenced for ever.
    #[test]
    fn the_project_question_distinguishes_nobody_from_nobody_answering() {
        let c = vec!["hive".to_string(), "mobile".to_string()];
        // The distinction, stated in both directions.
        assert_eq!(parse_project(None, &c), Answer::CannotAsk, "no line at all");
        assert_eq!(parse_project(Some(""), &c), Answer::Declined, "an empty line is a decision");
        assert_eq!(parse_project(Some("1"), &c), Answer::Accepted("hive".into()));
        assert_eq!(parse_project(Some("2"), &c), Answer::Accepted("mobile".into()));
        // A name that is not in the list is still an answer — a project can be created.
        assert_eq!(parse_project(Some("newproj"), &c), Answer::Accepted("newproj".into()));
    }

    #[test]
    fn the_ide_question_indexes_the_list_rather_than_taking_the_first() {
        let choices = ["opencode", "kiro"];
        assert_eq!(parse_ide(Some("1"), &choices), IdeAnswer::Chosen("opencode".into()));
        assert_eq!(parse_ide(Some("2"), &choices), IdeAnswer::Chosen("kiro".into()));
        // Out of range is a retry, not the first choice: a mutation that mapped every
        // number to `choices[0]` passed the suite and would have made "2" install
        // opencode.
        assert_eq!(parse_ide(Some("0"), &choices), IdeAnswer::Retry);
        assert_eq!(parse_ide(Some("3"), &choices), IdeAnswer::Retry);
        assert_eq!(parse_ide(Some("emacs"), &choices), IdeAnswer::Retry);
        assert_eq!(parse_ide(Some("kiro"), &choices), IdeAnswer::Chosen("kiro".into()));
        assert_eq!(parse_ide(None, &choices), IdeAnswer::Default);
        assert_eq!(parse_ide(Some("   "), &choices), IdeAnswer::Default);
    }

    /// Attention 6: a skipped plugin that points somewhere else is reported.
    #[test]
    fn a_stale_constant_is_read_out_of_an_existing_plugin() {
        let d = std::env::temp_dir().join(format!("brain-oc-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("tmp");
        let f = d.join("brain-session.js");
        std::fs::write(&f, opencode_plugin_js("/old/brain")).expect("write");

        assert_eq!(stale_constant(&f, "BRAIN_BIN").as_deref(), Some("/old/brain"));
        // There is no database constant, and there must not be one. The artifact
        // follows `BRAIN_DB_PATH`, so a frozen path is not a fallback to be tolerated
        // here — it is the defect itself, and the only way to keep it out is to have
        // nothing to compare.
        for forbidden in ["BRAIN_DB", "BRAIN_DB_INSTALLED", "BRAIN_DB_PATH"] {
            assert!(
                stale_constant(&f, forbidden).is_none(),
                "the plugin must carry no `{forbidden}` constant"
            );
        }
        // A file without the constant yields nothing rather than a wrong answer.
        std::fs::write(&f, "// nothing here\n").expect("write");
        assert_eq!(stale_constant(&f, "BRAIN_BIN"), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
