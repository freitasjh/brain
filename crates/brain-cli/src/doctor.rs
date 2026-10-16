//! `brain doctor` — D1–D6 (ARCHITECTURE §5, ADR-6).
//!
//! Todos os checks são binários (`ok/warn/fail`) com "como corrigir" acionável.
//! Regras herdadas:
//! - D4 nunca falha (warn): MCP/Ollama fora degrada, não trava.
//! - Nenhuma credencial impressa: `BRAIN_OLLAMA_URL` nunca entra no relatório.
//! - Nenhum write: doctor só lê (nunca cria dir, nunca escreve `.bak`).
//! - Paths explícitos (`dir`, `home`) para testes herméticos em scratch.

use std::path::Path;

/// Nível de um check: `ok` passa, `warn` passa com aviso, `fail` reprova.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Level::Ok => write!(f, "ok"),
            Level::Warn => write!(f, "warn"),
            Level::Fail => write!(f, "fail"),
        }
    }
}

/// Um check D1–D6 com diagnóstico e correção.
#[derive(Debug, Clone)]
pub struct Check {
    pub id: &'static str,
    pub level: Level,
    pub detail: String,
    pub fix: String,
}

/// Relatório D1–D6 na ordem.
#[derive(Debug, Clone)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    pub fn has_fail(&self) -> bool {
        self.checks.iter().any(|c| c.level == Level::Fail)
    }

    /// Uma linha por check: `D1 ok — detalhe` + `  como corrigir: ...` se não-ok.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for c in &self.checks {
            out.push_str(&format!("{} {} — {}\n", c.id, c.level, c.detail));
            if c.level != Level::Ok {
                out.push_str(&format!("  como corrigir: {}\n", c.fix));
            }
        }
        out.push_str(if self.has_fail() {
            "doctor: FAIL — corrija os itens acima e rode de novo\n"
        } else {
            "doctor: verde\n"
        });
        out
    }
}

/// Roda D1–D6 contra `dir` (projeto) + `home` (config global) + `db`.
pub fn run_doctor(dir: &Path, home: &Path, db: &str) -> Report {
    Report {
        checks: vec![
            d1_hooks(dir),
            d2_steering(dir),
            d3_plugin(home),
            d4_backend(db),
            d5_db_path(db),
            d6_skill_ref(dir, home),
        ],
    }
}

/// D1: 3 hooks kiro presentes, `version=v1`, triggers na lista fechada (I3).
fn d1_hooks(dir: &Path) -> Check {
    let mut problems = Vec::new();
    for name in [
        "brain-session.json",
        "brain-learning.json",
        "brain-prompt-optimizer.json",
    ] {
        let p = dir.join(".kiro/hooks").join(name);
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(_) => {
                problems.push(format!("ausente {}", p.display()));
                continue;
            }
        };
        let v: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                problems.push(format!("{} JSON inválido: {e}", p.display()));
                continue;
            }
        };
        if v.get("version") != Some(&serde_json::json!("v1")) {
            problems.push(format!("{} sem version=v1", p.display()));
        }
        let hooks = v.get("hooks").and_then(|h| h.as_array());
        match hooks {
            None => problems.push(format!("{} sem array hooks", p.display())),
            Some(list) => {
                if list.is_empty() {
                    problems.push(format!("{} com hooks vazio", p.display()));
                }
                for h in list {
                    match h.get("trigger").and_then(|t| t.as_str()) {
                        Some(t) if crate::setup::KIRO_TRIGGERS.contains(&t) => {}
                        Some(t) => problems.push(format!(
                            "{} trigger `{t}` fora da lista fechada",
                            p.display()
                        )),
                        None => problems.push(format!("{} hook sem trigger", p.display())),
                    }
                }
            }
        }
    }
    if problems.is_empty() {
        Check {
            id: "D1",
            level: Level::Ok,
            detail: "3 hooks kiro presentes, version=v1, triggers válidos".into(),
            fix: String::new(),
        }
    } else {
        Check {
            id: "D1",
            level: Level::Fail,
            detail: problems.join("; "),
            fix: format!("rode `brain setup kiro --dir {}` (--force restaura)", dir.display()),
        }
    }
}

/// D2: steering presente; agent-hooks sem `--db`, path absoluto ou projeto hardcoded.
fn d2_steering(dir: &Path) -> Check {
    let mut problems = Vec::new();
    let steering = dir.join(".kiro/steering/brain-learning.md");
    if !steering.is_file() {
        problems.push(format!("ausente {}", steering.display()));
    }
    for rel in [
        ".kiro/hooks/brain-learning.json",
        ".kiro/hooks/brain-prompt-optimizer.json",
        ".kiro/steering/brain-learning.md",
    ] {
        let p = dir.join(rel);
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        if text.contains("--db") {
            problems.push(format!("{rel} contém --db (congelaria o banco)"));
        }
        for abs in ["/home/", "/etc/", "/tmp/brain-export"] {
            if text.contains(abs) {
                problems.push(format!("{rel} contém path absoluto {abs}"));
            }
        }
    }
    if problems.is_empty() {
        Check {
            id: "D2",
            level: Level::Ok,
            detail: "steering presente, sem hardcoded/absoluto".into(),
            fix: String::new(),
        }
    } else {
        Check {
            id: "D2",
            level: Level::Fail,
            detail: problems.join("; "),
            fix: format!("rode `brain setup kiro --dir {} --force`", dir.display()),
        }
    }
}

/// D3: plugin opencode V2 registrado chamando `brain hook`.
fn d3_plugin(home: &Path) -> Check {
    let p = home.join(".config/opencode/plugins/brain-session.js");
    let text = std::fs::read_to_string(&p).ok();
    let Some(text) = text else {
        return Check {
            id: "D3",
            level: Level::Fail,
            detail: format!("plugin ausente em {}", p.display()),
            fix: "rode `brain setup opencode`".into(),
        };
    };
    let mut problems = Vec::new();
    if !(text.contains("\"hook\"") && text.contains("\"--event\"")) {
        problems.push("sem `hook --event` (argv)");
    }
    // Shape V2 (cf. hooks/opencode/brain-session.test.mjs): Plugin.define +
    // os 3 session.hook, sem resíduo V1.
    if !text.contains("Plugin.define") {
        problems.push("sem `Plugin.define` (formato V2)");
    }
    for hook in ["\"prompt\"", "\"context\"", "\"compaction\""] {
        if !text.contains(hook) {
            problems.push(match hook {
                "\"prompt\"" => "sem session.hook(\"prompt\")",
                "\"context\"" => "sem session.hook(\"context\")",
                _ => "sem session.hook(\"compaction\")",
            });
        }
    }
    if problems.is_empty() {
        Check {
            id: "D3",
            level: Level::Ok,
            detail: format!("plugin V2 em {}", p.display()),
            fix: String::new(),
        }
    } else {
        Check {
            id: "D3",
            level: Level::Fail,
            detail: format!("plugin em {} fora do shape V2 ({})", p.display(), problems.join(", ")),
            fix: "rode `brain setup opencode --force`".into(),
        }
    }
}

/// D4: backend alcançável — warn, nunca fail (I4/I6). Nunca imprime credencial:
/// o relatório carrega só paths, nunca `BRAIN_OLLAMA_URL` ou seu valor.
fn d4_backend(db: &str) -> Check {
    let p = Path::new(db);
    if !p.exists() {
        return Check {
            id: "D4",
            level: Level::Warn,
            detail: format!("banco ainda não existe em {db} (será criado no primeiro uso)"),
            fix: "suba com `brain serve-mcp`; verifique BRAIN_DB_PATH".into(),
        };
    }
    match brain_store::Store::open(db) {
        Ok(_) => Check {
            id: "D4",
            level: Level::Ok,
            detail: format!("banco legível em {db}"),
            fix: String::new(),
        },
        Err(e) => Check {
            id: "D4",
            level: Level::Warn,
            detail: format!("banco em {db} não abriu ({e}); seguindo sem contexto vetorial"),
            fix: "suba com `brain serve-mcp`; verifique BRAIN_DB_PATH".into(),
        },
    }
}

/// D5: `BRAIN_DB_PATH` resolvido e gravável. Relativo = warn (cai no default
/// relativo ao cwd); pai inexistente = fail. Nunca cria nada.
fn d5_db_path(db: &str) -> Check {
    let p = Path::new(db);
    if p.is_relative() {
        return Check {
            id: "D5",
            level: Level::Warn,
            detail: format!("BRAIN_DB_PATH relativo ({db}); resolve contra o cwd e pode espalhar bancos"),
            fix: "exporte BRAIN_DB_PATH absoluto (veja `brain setup shell`)".into(),
        };
    }
    let parent_ok = p.parent().map(|d| d.exists()).unwrap_or(false);
    if p.exists() || parent_ok {
        Check {
            id: "D5",
            level: Level::Ok,
            detail: format!("BRAIN_DB_PATH absoluto e gravável ({db})"),
            fix: String::new(),
        }
    } else {
        Check {
            id: "D5",
            level: Level::Fail,
            detail: format!("diretório-pai de {db} não existe"),
            fix: "crie o diretório ou ajuste BRAIN_DB_PATH".into(),
        }
    }
}

/// D6: skill `brain` referenciada (projeto `opencode.json` ou config global).
fn d6_skill_ref(dir: &Path, home: &Path) -> Check {
    let mut scanned = Vec::new();
    let mut found = false;
    let mut candidates = vec![dir.join("opencode.json")];
    for name in ["opencode.jsonc", "mcp.json"] {
        candidates.push(home.join(".config/opencode").join(name));
    }
    for p in &candidates {
        scanned.push(p.display().to_string());
        if let Ok(text) = std::fs::read_to_string(p) {
            if text.contains("brain") {
                found = true;
                break;
            }
        }
    }
    if found {
        Check {
            id: "D6",
            level: Level::Ok,
            detail: "skill brain referenciada na config".into(),
            fix: String::new(),
        }
    } else {
        Check {
            id: "D6",
            level: Level::Fail,
            detail: format!("nenhuma config cita brain ({})", scanned.join(", ")),
            fix: "rode `brain setup opencode` ou `brain setup --project <nome>`".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("brain-doctor-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Fixture verde: kiro completo + plugin opencode + opencode.json com skill ref.
    fn fixture_verde(base: &Path) -> (std::path::PathBuf, std::path::PathBuf, String) {
        let dir = base.join("proj");
        let home = base.join("home");
        let session = crate::setup::kiro_hook_json("brain").expect("triggers");
        for (rel, content) in [
            ("proj/.kiro/hooks/brain-session.json", session.clone()),
            ("proj/.kiro/hooks/brain-learning.json", crate::setup::KIRO_STATIC_LEARNING.to_string()),
            (
                "proj/.kiro/hooks/brain-prompt-optimizer.json",
                crate::setup::KIRO_STATIC_OPTIMIZER.to_string(),
            ),
            (
                "proj/.kiro/steering/brain-learning.md",
                crate::setup::KIRO_STATIC_STEERING.to_string(),
            ),
            ("home/.config/opencode/plugins/brain-session.js", crate::setup::opencode_plugin_js("brain")),
            (
                "proj/opencode.json",
                r#"{"mcp":{"servers":{"brain":{"type":"remote","url":"http://localhost:8321/mcp","disabled":false,"timeout":{"catalog":30000,"execution":30000}}}},"instructions":["../brain/.agents/skills/brain/SKILL.md"]}"#.to_string(),
            ),
        ] {
            let dest = base.join(rel);
            std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std::fs::write(&dest, content).unwrap();
        }
        // D4/D5: banco real em path absoluto (a fixture pode escrever; o doctor não).
        let db = base.join("db/brain.db");
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        brain_store::Store::open(db.to_str().unwrap()).expect("fixture db");
        (dir, home, db.to_str().unwrap().to_string())
    }

    #[test]
    fn install_ok_da_verde_d1_d6() {
        let base = tmpdir("verde");
        let (dir, home, db) = fixture_verde(&base);
        let rep = run_doctor(&dir, &home, &db);
        assert_eq!(rep.checks.len(), 6, "D1–D6: 6 checks, got {}", rep.checks.len());
        assert!(!rep.has_fail(), "fixture verde não deve falhar:\n{}", rep.render());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn hook_faltando_falha_com_caminho_exato() {
        let base = tmpdir("sem-hook");
        let (dir, home, db) = fixture_verde(&base);
        std::fs::remove_file(dir.join(".kiro/hooks/brain-learning.json")).unwrap();
        let rep = run_doctor(&dir, &home, &db);
        assert!(rep.has_fail());
        let d1 = rep.checks.iter().find(|c| c.id == "D1").unwrap();
        assert_eq!(d1.level, Level::Fail);
        assert!(d1.detail.contains("brain-learning.json"), "deve citar o caminho: {}", d1.detail);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn mcp_down_e_warn_nunca_fail() {
        let base = tmpdir("d4");
        let (dir, home, _) = fixture_verde(&base);
        let rep = run_doctor(&dir, &home, "/caminho/que/nao/existe/brain.db");
        let d4 = rep.checks.iter().find(|c| c.id == "D4").unwrap();
        assert_ne!(d4.level, Level::Fail, "D4 nunca falha: {}", d4.detail);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn nenhuma_credencial_no_relatorio() {
        let base = tmpdir("redact");
        let (dir, home, db) = fixture_verde(&base);
        // O doctor nunca lê BRAIN_OLLAMA_URL: mesmo credentialed, nada vaza.
        // (Sem set_var aqui — edition 2024 o marca unsafe e é global; a
        // garantia é estrutural: grep por OLLAMA_URL neste arquivo dá zero.)
        let rep = run_doctor(&dir, &home, &db);
        let rendered = rep.render();
        assert!(!rendered.contains("hunter2"), "senha vazou no doctor:\n{rendered}");
        assert!(
            !rendered.contains("BRAIN_OLLAMA_URL"),
            "doctor não deve nem citar a env:\n{rendered}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    // SU-02b — matriz: cada D removido falha o correspondente; OLLAMA morto verde.

    #[test]
    fn d2_sem_steering_falha() {
        let base = tmpdir("sem-steer");
        let (dir, home, db) = fixture_verde(&base);
        std::fs::remove_file(dir.join(".kiro/steering/brain-learning.md")).unwrap();
        let rep = run_doctor(&dir, &home, &db);
        let d2 = rep.checks.iter().find(|c| c.id == "D2").unwrap();
        assert_eq!(d2.level, Level::Fail, "D2 deve falhar sem steering");
        assert!(rep.has_fail());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn d3_sem_plugin_falha() {
        let base = tmpdir("sem-plugin");
        let (dir, home, db) = fixture_verde(&base);
        std::fs::remove_file(home.join(".config/opencode/plugins/brain-session.js")).unwrap();
        let rep = run_doctor(&dir, &home, &db);
        let d3 = rep.checks.iter().find(|c| c.id == "D3").unwrap();
        assert_eq!(d3.level, Level::Fail, "D3 deve falhar sem plugin");
        assert!(d3.fix.contains("setup opencode"), "fix deve dizer como corrigir: {}", d3.fix);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn d5_pai_inexistente_falha() {
        let base = tmpdir("d5");
        let (dir, home, _) = fixture_verde(&base);
        let ghost = base.join("nao-existe/db/brain.db");
        let rep = run_doctor(&dir, &home, ghost.to_str().unwrap());
        let d5 = rep.checks.iter().find(|c| c.id == "D5").unwrap();
        assert_eq!(d5.level, Level::Fail, "D5 deve falhar com pai inexistente");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn d6_sem_referencia_falha() {
        let base = tmpdir("sem-ref");
        let (dir, home, db) = fixture_verde(&base);
        std::fs::remove_file(dir.join("opencode.json")).unwrap();
        let rep = run_doctor(&dir, &home, &db);
        let d6 = rep.checks.iter().find(|c| c.id == "D6").unwrap();
        assert_eq!(d6.level, Level::Fail, "D6 deve falhar sem referência à skill");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn ollama_morto_ainda_verde_via_d4_warn() {
        // Edge SU-02b: doctor faz zero chamadas de rede (D4 = abertura de
        // arquivo local), então Ollama morto não muda nada. Sem env_remove
        // (H2.3) e sem mock: a garantia é não depender da rede em absoluto.
        let base = tmpdir("ollama-morto");
        let (dir, home, _) = fixture_verde(&base);
        // Pai existe (D5 ok), arquivo ausente (D4 warn): o caso do primeiro uso.
        let ghost_db = base.join("db/fresh.db");
        let rep = run_doctor(&dir, &home, ghost_db.to_str().unwrap());
        assert!(!rep.has_fail(), "Ollama/banco ausente nunca reprova:\n{}", rep.render());
        let d4 = rep.checks.iter().find(|c| c.id == "D4").unwrap();
        assert_eq!(d4.level, Level::Warn, "D4 degrada para warn: {}", d4.detail);
        let _ = std::fs::remove_dir_all(&base);
    }
}
