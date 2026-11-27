//! What a coding agent is handed to write Soma: the block in AGENTS.md, the
//! skill, the hook that checks after every edit, and `soma doctor`, which
//! says when any of them is out of step with the binary. (Design note
//! "l'outillage qui rend un agent efficace", lot A.)

use std::io::Write;
use std::process::{Command, Stdio};

fn soma(dir: &std::path::Path, args: &[&str], stdin: &str) -> (String, String, i32) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_soma"));
    c.args(args).current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().expect("soma runs");
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.code().unwrap_or(-1))
}

fn fresh(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_agent_tools_{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn edited(path: &std::path::Path) -> String {
    serde_json::json!({
        "hook_event_name": "PostToolUse", "tool_name": "Edit", "cwd": path.parent().unwrap(),
        "tool_input": { "file_path": path }, "tool_response": { "filePath": path }
    }).to_string()
}

#[test]
fn the_agent_block_and_the_skill_keep_their_size() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let block = std::fs::read_to_string(root.join("site/agent.md")).unwrap();
    let skill = std::fs::read_to_string(root.join("SKILL.md")).unwrap();
    assert!(block.len() < 5_000, "site/agent.md is {} bytes: what an agent loads ahead stays under 5 KB — the rest is a `soma docs` topic", block.len());
    assert!(skill.len() < 3_072, "SKILL.md is {} bytes: the skill stays under 3 KB", skill.len());
    // no figure that another page also states, no release note, no tool nothing distributes
    for (name, text) in [("site/agent.md", &block), ("SKILL.md", &skill)] {
        assert!(!text.contains("Rust tests"), "{name} states validation figures: they live on /status alone");
        assert!(!text.contains("mcp__") && !text.contains("soma_generate"), "{name} recommends MCP tools nothing distributes");
        assert!(!text.contains("2.8."), "{name} carries release notes: they are `soma docs releases`");
    }
    // every command and topic the skill names exists
    let dir = fresh("size");
    for topic in ["method", "flesh", "skeleton", "law", "gotchas", "builtins", "agent", "agents", "native", "guarantees", "serving", "operations", "releases", "handbook", "reference"] {
        assert!(skill.contains(&format!("`{topic}`")), "the skill lists the topic {topic}");
        let (out, err, code) = soma(&dir, &["docs", topic], "");
        assert!(code == 0 && out.len() > 500, "soma docs {topic}: {code} {err}");
    }
    // what left the skill is served by the binary
    let (handbook, _, _) = soma(&dir, &["docs", "handbook"], "");
    assert!(handbook.contains("Idiomatic templates") && !handbook.contains("mcp__"), "the handbook holds the former skill, without the MCP tools");
}

#[test]
fn init_installs_the_block_the_skill_and_the_hook_and_doctor_agrees() {
    let dir = fresh("init");
    let (out, err, code) = soma(&dir, &["init", "demo"], "");
    assert_eq!(code, 0, "{out}{err}");
    let p = dir.join("demo");
    let block = std::fs::read_to_string(p.join("AGENTS.md")).unwrap();
    assert!(block.contains("<!-- soma:agent-block ") && block.contains("<!-- /soma:agent-block -->"), "{block}");
    assert!(p.join(".claude/skills/soma/SKILL.md").exists());
    let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p.join(".claude/settings.json")).unwrap()).unwrap();
    let group = &settings["hooks"]["PostToolUse"][0];
    assert_eq!(group["matcher"], "Edit|Write");
    assert_eq!(group["hooks"][0]["type"], "command");
    assert!(group["hooks"][0]["command"].as_str().unwrap().contains("soma hook"), "{group}");
    let (out, err, code) = soma(&p, &["doctor"], "");
    assert_eq!(code, 0, "{out}{err}");
    let (out, _, code) = soma(&p, &["doctor", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).expect(&out);
    assert_eq!(code, 0);
    assert_eq!(v["ok"], true, "{out}");
    assert!(v["checks"].as_array().unwrap().len() >= 5, "{out}");
}

#[test]
fn doctor_names_each_gap_and_the_command_that_closes_it() {
    let dir = fresh("doctor");
    std::fs::write(dir.join("soma.toml"), "[package]\nname = \"x\"\nentry = \"app.cell\"\nedition = \"2027\"\n").unwrap();
    std::fs::write(dir.join("app.cell"), "cell A {\n    on f() { return 1 }\n}\n").unwrap();
    // nothing installed: three gaps, one command
    let (out, err, code) = soma(&dir, &["doctor"], "");
    let all = format!("{out}{err}");
    assert_ne!(code, 0, "{all}");
    for what in ["AGENTS.md", "skill", "hook"] { assert!(all.contains(what), "{what}: {all}"); }
    assert!(all.contains("soma init --agent"), "{all}");
    // `soma init --agent` closes them, in a project that exists, without touching its program
    let (out, err, code) = soma(&dir, &["init", "--agent"], "");
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(std::fs::read_to_string(dir.join("app.cell")).unwrap(), "cell A {\n    on f() { return 1 }\n}\n");
    let (out, err, code) = soma(&dir, &["doctor"], "");
    assert_eq!(code, 0, "{out}{err}");
    // a block written by a NEWER soma: the binary is what is old
    let block = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
    let at = block.find("<!-- soma:agent-block ").unwrap();
    let end = at + block[at..].find("-->").unwrap();
    let newer = format!("{}<!-- soma:agent-block 99.0.0 {}\nA rule this binary does not know.\n", &block[..at], &block[end..]);
    std::fs::write(dir.join("AGENTS.md"), newer).unwrap();
    let (out, err, code) = soma(&dir, &["doctor"], "");
    let all = format!("{out}{err}");
    assert_ne!(code, 0, "{all}");
    assert!(all.contains("99.0.0") && all.contains("install.sh"), "the fix is to update the binary: {all}");
    assert!(!all.contains("soma init --agent"), "rewriting the block with an older binary is not the fix: {all}");
    // a block somebody edited, same version: rewrite it
    std::fs::write(dir.join("AGENTS.md"), block.replace("Never guess", "Guess")).unwrap();
    let (out, err, code) = soma(&dir, &["doctor"], "");
    let all = format!("{out}{err}");
    assert_ne!(code, 0, "{all}");
    assert!(all.contains("soma init --agent"), "{all}");
    // an edition this binary does not know
    let _ = soma(&dir, &["init", "--agent"], "");
    std::fs::write(dir.join("soma.toml"), "[package]\nname = \"x\"\nentry = \"app.cell\"\nedition = \"2031\"\n").unwrap();
    let (out, err, code) = soma(&dir, &["doctor", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).expect(&format!("{out}{err}"));
    assert_ne!(code, 0);
    assert_eq!(v["ok"], false);
    let bad: Vec<&serde_json::Value> = v["checks"].as_array().unwrap().iter().filter(|c| c["ok"] == false).collect();
    assert_eq!(bad.len(), 1, "{out}");
    assert_eq!(bad[0]["name"], "edition");
    assert!(bad[0]["fix"].as_str().unwrap().contains("install.sh"), "{out}");
}

#[test]
fn init_agent_keeps_what_the_project_already_has() {
    let dir = fresh("keep");
    std::fs::create_dir_all(dir.join(".claude")).unwrap();
    std::fs::write(dir.join(".claude/settings.json"), r#"{
  "permissions": { "allow": ["Bash(npm test)"] },
  "hooks": {
    "PostToolUse": [
      { "matcher": "Edit|Write", "hooks": [ { "type": "command", "command": "npx prettier --write ." } ] }
    ],
    "Stop": [ { "hooks": [ { "type": "command", "command": "say done" } ] } ]
  }
}"#).unwrap();
    std::fs::write(dir.join("AGENTS.md"), "# Our project\n\nUse tabs.\n").unwrap();
    let (out, err, code) = soma(&dir, &["init", "--agent"], "");
    assert_eq!(code, 0, "{out}{err}");
    let (out2, err2, code2) = soma(&dir, &["init", "--agent"], "");
    assert_eq!(code2, 0, "{out2}{err2}");
    let text = std::fs::read_to_string(dir.join(".claude/settings.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["permissions"]["allow"][0], "Bash(npm test)");
    assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
    let post = v["hooks"]["PostToolUse"].as_array().unwrap();
    assert_eq!(post[0]["hooks"][0]["command"], "npx prettier --write .", "the hook that was there is still first");
    assert_eq!(post.len(), 2, "installed once, however often it is run: {text}");
    assert!(post[1]["hooks"][0]["command"].as_str().unwrap().ends_with("soma hook"), "{text}");
    let agents = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
    assert!(agents.starts_with("# Our project\n\nUse tabs.\n"), "{agents}");
    assert_eq!(agents.matches("<!-- soma:agent-block ").count(), 1, "{agents}");
    // a settings file that does not read as JSON is left as it is, and the lines to add are shown
    let dir = fresh("keep_bad");
    std::fs::create_dir_all(dir.join(".claude")).unwrap();
    std::fs::write(dir.join(".claude/settings.json"), "{ not json").unwrap();
    let (out, err, _) = soma(&dir, &["init", "--agent"], "");
    assert_eq!(std::fs::read_to_string(dir.join(".claude/settings.json")).unwrap(), "{ not json");
    assert!(format!("{out}{err}").contains("\"PostToolUse\""), "{out}{err}");
}

#[test]
fn the_hook_returns_the_diagnostics_of_an_edit_and_nothing_else() {
    let dir = fresh("hook");
    let (_, _, code) = soma(&dir, &["init", "--skeleton", "demo"], "");
    assert_eq!(code, 0);
    let p = dir.join("demo");
    // an edit that leaves the program right: silence
    let flesh = p.join("lib/counter.cell");
    let (out, err, code) = soma(&p, &["hook"], &edited(&flesh));
    assert_eq!((out.as_str(), err.as_str(), code), ("", "", 0));
    // a file that is not Soma: silence, whatever it holds
    std::fs::write(p.join("notes.md"), "cell {{{").unwrap();
    let (out, err, code) = soma(&p, &["hook"], &edited(&p.join("notes.md")));
    assert_eq!((out.as_str(), err.as_str(), code), ("", "", 0));
    // input that is not a hook's: silence
    let (out, err, code) = soma(&p, &["hook"], "not json");
    assert_eq!((out.as_str(), err.as_str(), code), ("", "", 0));
    // an error in the FLESH: the hook checks the program (the entry of soma.toml), not the file alone
    let before = std::fs::read_to_string(&flesh).unwrap();
    std::fs::write(&flesh, before.replace("on ", "on zzz_nowhere() { return undefined_name_q }\n    on ")).unwrap();
    let broken = std::fs::read_to_string(&flesh).unwrap();
    let (out, err, code) = soma(&p, &["hook"], &edited(&flesh));
    assert_eq!(code, 2, "{out}{err}");
    assert_eq!(out, "");
    let (_, check_err, _) = soma(&p, &["check", "app.cell"], "");
    let (check_out, _, _) = soma(&p, &["check", "app.cell"], "");
    assert!(!err.trim().is_empty());
    assert!(format!("{check_out}{check_err}").contains(err.trim().lines().next().unwrap()), "the hook prints what `soma check` prints:\n{err}");
    // it changed nothing
    assert_eq!(std::fs::read_to_string(&flesh).unwrap(), broken);
    // soma.toml and a law are Soma files too
    std::fs::write(&flesh, &before).unwrap();
    let (_, err, code) = soma(&p, &["hook"], &edited(&p.join("soma.toml")));
    assert_eq!((err.as_str(), code), ("", 0));
}
