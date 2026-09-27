//! `brain setup` — one-shot installer (opencode MCP + systemd + project snippet).
//! All writers take explicit paths (no env reads) so unit tests stay hermetic.

use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};

pub struct SetupOpts {
    pub target: String,
    pub mcp_port: u16,
    pub viewer_port: u16,
    pub db: String,
    pub brain_dir: Option<String>,
    pub dir: Option<String>,
    pub force: bool,
    pub dry_run: bool,
}

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
    let targets: Vec<&str> = match o.target.as_str() {
        "all" => vec!["opencode", "systemd", "shell"],
        "opencode" | "systemd" | "shell" | "project" => vec![o.target.as_str()],
        other => anyhow::bail!("unknown setup target '{}': expected all|opencode|systemd|shell|project", other),
    };
    if o.dry_run {
        println!("setup dry-run url={} exe={} db={}", url, exe, db);
    }
    for t in targets {
        match t {
            "opencode" => opencode_target(&home, &url, o.force, o.dry_run)?,
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
            _ => unreachable!(),
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
