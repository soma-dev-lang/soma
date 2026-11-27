//! What a coding agent is handed in a project: the block in `AGENTS.md`, the
//! skill, the hook that checks after every edit — and `soma doctor`, which
//! compares them with this binary and names the command that closes each gap.
//!
//! The three texts are compiled in: a binary installs and expects exactly
//! what it was built with, so they cannot be ahead of it or behind it.

use std::path::{Path, PathBuf};

/// The block for `AGENTS.md` / `CLAUDE.md` — the text the site serves at /agent.md.
pub const AGENTS_MD: &str = include_str!("../../../site/agent.md");
/// The skill — the text the site serves at /skill/SKILL.md.
const SKILL: &str = include_str!("../../../SKILL.md");

const VERSION: &str = env!("CARGO_PKG_VERSION");
const BLOCK_OPEN: &str = "<!-- soma:agent-block ";
const BLOCK_CLOSE: &str = "<!-- /soma:agent-block -->";
const SKILL_MARK: &str = "<!-- soma:skill ";
const SKILL_PATH: &str = ".claude/skills/soma/SKILL.md";
const SETTINGS_PATH: &str = ".claude/settings.json";
/// Does nothing where soma is not installed, or is a soma older than the
/// hook (its "unknown command" would be shown after every edit); `soma hook`
/// reads the edit on stdin.
const HOOK_COMMAND: &str = "soma hook --help >/dev/null 2>&1 || exit 0; soma hook";
const UPDATE: &str = "curl -fsSL https://soma-lang.dev/install.sh | sh";
const INSTALL: &str = "soma init --agent";

fn block() -> String {
    format!("{}{} -->\n{}\n{}\n", BLOCK_OPEN, VERSION, AGENTS_MD.trim_end(), BLOCK_CLOSE)
}

fn skill() -> String {
    format!("{}\n\n{}{} -->\n", SKILL.trim_end(), SKILL_MARK, VERSION)
}

fn hook_group() -> serde_json::Value {
    serde_json::json!({ "matcher": "Edit|Write", "hooks": [ { "type": "command", "command": HOOK_COMMAND, "timeout": 60 } ] })
}

/// `(version, start, end)` of the block in an AGENTS.md, markers included.
fn find_block(text: &str) -> Option<(String, usize, usize)> {
    let at = text.find(BLOCK_OPEN)?;
    let head_end = at + text[at..].find("-->")?;
    let version = text[at + BLOCK_OPEN.len()..head_end].trim().to_string();
    let close = head_end + text[head_end..].find(BLOCK_CLOSE)?;
    let mut end = close + BLOCK_CLOSE.len();
    if text[end..].starts_with('\n') { end += 1; }
    Some((version, at, end))
}

fn newer(a: &str, b: &str) -> bool {
    let n = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.trim().parse().unwrap_or(0)).collect() };
    n(a) > n(b)
}

fn has_hook(settings: &serde_json::Value) -> bool {
    settings["hooks"]["PostToolUse"].as_array().is_some_and(|groups| groups.iter().any(|g| {
        g["hooks"].as_array().is_some_and(|hs| hs.iter().any(|h| h["command"].as_str().is_some_and(|c| c.contains("soma hook"))))
    }))
}

/// Write the block, the skill and the hook into `dir`, keeping what is there:
/// the rest of an AGENTS.md, the other settings and hooks. One line per file.
pub fn install(dir: &Path) -> Vec<String> {
    let mut said = Vec::new();
    // AGENTS.md: the block between its markers; the rest is the project's
    let path = dir.join("AGENTS.md");
    let had = std::fs::read_to_string(&path).ok();
    let text = match &had {
        None => block(),
        Some(t) => match find_block(t) {
            Some((_, a, b)) => format!("{}{}{}", &t[..a], block(), &t[b..]),
            // the block as `soma init` wrote it before it had markers: the whole file
            None if t.starts_with("# Soma — instructions for coding agents") && t.matches("\n# ").count() == 0 => block(),
            None => format!("{}{}{}", t, if t.ends_with("\n\n") || t.is_empty() { "" } else if t.ends_with('\n') { "\n" } else { "\n\n" }, block()),
        },
    };
    if had.as_deref() != Some(text.as_str()) {
        match std::fs::write(&path, &text) {
            Ok(()) => said.push(format!("AGENTS.md                      {}", if had.is_some() { "the Soma block, brought to this binary's" } else { "instructions for coding agents" })),
            Err(e) => said.push(format!("AGENTS.md                      not written: {}", e)),
        }
    }
    // the skill: a file of ours
    let path = dir.join(SKILL_PATH);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(skill().as_str()) {
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        match std::fs::write(&path, skill()) {
            Ok(()) => said.push(format!("{}  the skill: the loop, and what to ask the binary", SKILL_PATH)),
            Err(e) => said.push(format!("{}  not written: {}", SKILL_PATH, e)),
        }
    }
    // the hook: added to the settings, never in place of them
    let path = dir.join(SETTINGS_PATH);
    let to_add = || serde_json::to_string_pretty(&serde_json::json!({ "hooks": { "PostToolUse": [ hook_group() ] } })).unwrap_or_default();
    let existing = std::fs::read_to_string(&path).ok();
    let mut settings = match &existing {
        None => serde_json::json!({}),
        Some(t) => match serde_json::from_str::<serde_json::Value>(t) {
            Ok(v) if v.is_object() => v,
            _ => {
                said.push(format!("{}          does not read as JSON and was left as it is — add to it:\n{}", SETTINGS_PATH, to_add()));
                return said;
            }
        },
    };
    if !has_hook(&settings) {
        let fits = (settings.get("hooks").is_none() || settings["hooks"].is_object())
            && (settings["hooks"].get("PostToolUse").is_none() || settings["hooks"]["PostToolUse"].is_array());
        if !fits {
            said.push(format!("{}          its `hooks` are not in the shape expected and were left as they are — add:\n{}", SETTINGS_PATH, to_add()));
            return said;
        }
        if settings.get("hooks").is_none() { settings["hooks"] = serde_json::json!({}); }
        if settings["hooks"].get("PostToolUse").is_none() { settings["hooks"]["PostToolUse"] = serde_json::json!([]); }
        if let Some(groups) = settings["hooks"]["PostToolUse"].as_array_mut() { groups.push(hook_group()); }
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        match std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&settings).unwrap_or_default())) {
            Ok(()) => said.push(format!("{}          the hook: `soma check` after every edit of a .cell, a .law or soma.toml", SETTINGS_PATH)),
            Err(e) => said.push(format!("{}          not written: {}", SETTINGS_PATH, e)),
        }
    }
    said
}

/// `soma init --agent`: the block, the skill and the hook, in a project that exists.
pub fn cmd_init_agent(dir: &Path) {
    let said = install(dir);
    if said.is_empty() {
        println!("nothing to do: the block, the skill and the hook are this binary's (soma {})", VERSION);
    } else {
        for line in &said { println!("  {}", line); }
        println!();
        println!("`soma doctor` says whether they are still current.");
    }
}

struct Check { name: &'static str, ok: bool, found: String, fix: Option<String> }

fn one_of_ours(name: &'static str, what: &str, found: Option<(String, bool)>) -> Check {
    match found {
        None => Check { name, ok: false, found: format!("{}: absent", what), fix: Some(INSTALL.to_string()) },
        Some((_, true)) => Check { name, ok: true, found: format!("{}: this binary's", what), fix: None },
        Some((v, false)) if newer(&v, VERSION) => Check {
            name, ok: false,
            found: format!("{}: written by soma {}, newer than this binary ({}) — it may name what this binary does not have", what, v, VERSION),
            fix: Some(UPDATE.to_string()),
        },
        Some((v, false)) => Check {
            name, ok: false,
            found: format!("{}: {} — not this binary's ({})", what, if v.is_empty() { "no version mark".to_string() } else if v == VERSION { "edited since it was written".to_string() } else { format!("written by soma {}", v) }, VERSION),
            fix: Some(INSTALL.to_string()),
        },
    }
}

/// The directory of the nearest soma.toml at or above `from`.
pub fn project_root(from: &Path) -> Option<PathBuf> {
    let mut d = Some(from);
    while let Some(dir) = d {
        if dir.join("soma.toml").is_file() { return Some(dir.to_path_buf()); }
        d = dir.parent();
    }
    None
}

/// `soma doctor [dir] [--json]`: the binary, the edition the project declares,
/// the block, the skill and the hook. Exits 1 when one of them is out of step.
pub fn cmd_doctor(dir: &Path, json: bool) {
    let dir = project_root(&std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf())).unwrap_or_else(|| dir.to_path_buf());
    let mut checks = vec![Check { name: "binary", ok: true, found: format!("soma {}", VERSION), fix: None }];
    // the edition the project declares
    let manifest = std::fs::read_to_string(dir.join("soma.toml")).ok();
    let edition = manifest.as_deref().and_then(|t| t.parse::<toml::Value>().ok())
        .and_then(|v| v.get("package").and_then(|p| p.get("edition")).and_then(|e| e.as_str().map(str::to_string)));
    let editions = crate::pkg::manifest::EDITIONS;
    checks.push(match (&manifest, edition) {
        (None, _) => Check { name: "edition", ok: true, found: "no soma.toml here: no edition declared".to_string(), fix: None },
        (_, None) => Check { name: "edition", ok: true, found: format!("soma.toml declares no edition: the program is read as {}", editions[0]), fix: None },
        (_, Some(e)) if editions.contains(&e.as_str()) => Check { name: "edition", ok: true, found: format!("edition {}", e), fix: None },
        (_, Some(e)) => Check {
            name: "edition", ok: false,
            found: format!("soma.toml declares edition {}, which this binary ({}) does not know — it knows {}", e, VERSION, editions.join(", ")),
            fix: Some(UPDATE.to_string()),
        },
    });
    let agents = std::fs::read_to_string(dir.join("AGENTS.md")).ok();
    checks.push(one_of_ours("agent-block", "AGENTS.md block", agents.as_deref().map(|t| match find_block(t) {
        Some((v, a, b)) => (v, t[a..b].trim_end() == block().trim_end()),
        None => (String::new(), false),
    })));
    let skill_file = std::fs::read_to_string(dir.join(SKILL_PATH)).ok();
    checks.push(one_of_ours("skill", "skill (.claude/skills/soma)", skill_file.as_deref().map(|t| {
        let v = t.rfind(SKILL_MARK).and_then(|at| t[at + SKILL_MARK.len()..].split("-->").next()).map(|v| v.trim().to_string()).unwrap_or_default();
        (v, t == skill())
    })));
    let installed = std::fs::read_to_string(dir.join(SETTINGS_PATH)).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).is_some_and(|v| has_hook(&v));
    checks.push(Check {
        name: "hook", ok: installed,
        found: if installed { "hook: `soma check` runs after every edit (.claude/settings.json)".to_string() } else { "hook: absent — nothing checks an edit unless the agent thinks of it".to_string() },
        fix: if installed { None } else { Some(INSTALL.to_string()) },
    });
    let ok = checks.iter().all(|c| c.ok);
    if json {
        let list: Vec<serde_json::Value> = checks.iter().map(|c| serde_json::json!({ "name": c.name, "ok": c.ok, "found": c.found, "fix": c.fix })).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "schema": "soma.doctor/1", "soma": VERSION, "project": dir.display().to_string(), "ok": ok, "checks": list })).unwrap_or_default());
    } else {
        for c in &checks {
            println!("{} {}", if c.ok { "✓" } else { "✗" }, c.found);
            if let Some(f) = &c.fix { println!("    fix: {}", f); }
        }
        let mut fixes: Vec<&str> = checks.iter().filter_map(|c| c.fix.as_deref()).collect();
        fixes.dedup();
        fixes.sort(); fixes.dedup();
        println!();
        if ok { println!("nothing to do: this project's agent setup is this binary's"); }
        else { println!("run: {}", fixes.join("  &&  ")); }
    }
    if !ok { std::process::exit(1); }
}

/// `soma hook`: the command the hook runs after an edit. Reads the tool call
/// on stdin; when the edited file is a `.cell`, a `.law` or a `soma.toml`,
/// checks the program it belongs to. Silent and 0 when there is nothing to
/// say; the diagnostics of `soma check` on stderr and 2 (what Claude Code
/// shows the model) otherwise. It writes nothing.
pub fn cmd_hook() {
    use std::io::Read;
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let Ok(call) = serde_json::from_str::<serde_json::Value>(&input) else { return };
    let Some(file) = call["tool_input"]["file_path"].as_str().or_else(|| call["tool_response"]["filePath"].as_str()) else { return };
    let mut file = PathBuf::from(file);
    if file.is_relative() { if let Some(cwd) = call["cwd"].as_str() { file = Path::new(cwd).join(file); } }
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let is_cell = name.ends_with(".cell");
    if !(is_cell || name.ends_with(".law") || name == "soma.toml") { return; }
    // the program: the entry of the nearest soma.toml; without one, the file itself
    let root = file.parent().and_then(project_root);
    let entry = root.as_ref().and_then(|r| {
        let m = std::fs::read_to_string(r.join("soma.toml")).ok()?.parse::<toml::Value>().ok()?;
        let e = m.get("package")?.get("entry")?.as_str()?.to_string();
        Some(r.join(e)).filter(|p| p.is_file())
    });
    let (dir, target) = match (root, entry) {
        (Some(r), Some(e)) => (r, e),
        _ if is_cell && file.is_file() => (file.parent().map(Path::to_path_buf).unwrap_or_default(), file.clone()),
        _ => return,
    };
    let Ok(me) = std::env::current_exe() else { return };
    let shown = target.strip_prefix(&dir).map(Path::to_path_buf).unwrap_or(target);
    let Ok(out) = std::process::Command::new(me).arg("check").arg(&shown).current_dir(&dir).stdin(std::process::Stdio::null()).output() else { return };
    if out.status.success() { return; }
    let text = format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
    eprint!("{}", text);
    if !text.ends_with('\n') { eprintln!(); }
    std::process::exit(2);
}
