//! `soma law`: the program against the laws it is under (RFC 0002) — the
//! conformity table, the file of one rule, the work list.

use std::path::PathBuf;

use crate::checker::law;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports};

pub fn cmd_law(path: &PathBuf, rule: Option<&str>, todo: bool, lock: bool, json: bool, map: bool, no_run: bool) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);

    let root = crate::project::root(path);
    let manifest = crate::pkg::manifest::Manifest::load(&root.join("soma.toml")).ok();
    let mut loaded = law::load(manifest.as_ref(), &root);
    // what the formal part gets wrong is said with the rest; a law the lock
    // holds and soma.toml dropped is said unless the lock is being rewritten
    let formal: Vec<String> = loaded.formal_errors.iter().map(|(_, m)| m.clone()).collect();
    loaded.errors.extend(formal);
    if !lock { if let Some(m) = law::left_behind(&loaded, &root) { loaded.errors.push(m); } }
    if loaded.laws.is_empty() && loaded.errors.is_empty() {
        eprintln!("this program is under no law — name its laws in soma.toml:\n\n[law]\nfiles = [\"lois/sanctions.law\"]\n\nA law is a file of terms and rules that names no code  (`soma docs law`).");
        std::process::exit(1);
    }
    if lock {
        if !loaded.errors.is_empty() {
            for e in &loaded.errors { eprintln!("error: {}", e); }
            eprintln!("the law is not locked: it does not read");
            std::process::exit(1);
        }
        let target = root.join("law.lock");
        match std::fs::write(&target, law::lock_text(&loaded)) {
            Ok(()) => println!("locked {} law file(s) in {}", loaded.files.len(), target.display()),
            Err(e) => { eprintln!("cannot write {}: {}", target.display(), e); std::process::exit(1); }
        }
        return;
    }
    if map {
        let dictionary = law::dictionary(&program, &loaded.laws);
        if json { println!("{}", serde_json::to_string_pretty(&dictionary).unwrap_or_default()); }
        else { print!("{}", law::render_dictionary(&dictionary)); }
        return;
    }
    let mut report = law::analyse(&program, &loaded.laws, &law::tests_beside(&root));
    let program_print = super::refute::fingerprint(path, &program).iter().find(|p| p.kind == "program").map(|p| p.digest.clone()).unwrap_or_default();
    evidence(path, &root, &mut report, no_run, &program_print);
    let locked = law::against_lock(&loaded, &root);
    if json {
        println!("{}", serde_json::to_string_pretty(&law::json(&loaded, &report, &locked)).unwrap_or_default());
    } else if todo {
        print!("{}", law::todo(&loaded, &report, &locked));
    } else {
        if let Some(id) = rule {
            if !report.rules.iter().any(|s| s.rule.id == id) {
                eprintln!("no rule \"{}\" in the laws of this program — the rules are {}", id, report.rules.iter().map(|s| format!("\"{}\"", s.rule.id)).collect::<Vec<_>>().join(", "));
                std::process::exit(1);
            }
        }
        for e in &loaded.errors { println!("error: {}", e); }
        print!("{}", law::render(&loaded, &report, &locked, rule));
    }
    let failing = !loaded.errors.is_empty() || matches!(locked, Ok(Some(_))) || report.issues.iter().any(|i| !i.warning) || !report.owed.is_empty() || !law::evidence_lines(&report).is_empty();
    if failing { std::process::exit(1); }
}

/// This binary, run on the project: its JSON answer.
fn ask(dir: &std::path::Path, args: &[&str]) -> Option<serde_json::Value> {
    let me = std::env::current_exe().ok()?;
    let out = std::process::Command::new(me).args(args).current_dir(dir).stdin(std::process::Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&text[text.find('{')?..]).ok()
}

/// What the tests under each rule say when they are run, and what the
/// refutations the project keeps (`refutations/*.trace`) show when they
/// are replayed. A table that lists a failing test among those that back a
/// rule, or stays silent on a known counter-example, vouches for nothing.
fn evidence(path: &PathBuf, root: &std::path::Path, report: &mut law::Report, no_run: bool, program_print: &str) {
    let wanted: std::collections::BTreeSet<String> = report.rules.iter().flat_map(|s| s.tests.iter().cloned()).collect();
    if no_run {
        for s in report.rules.iter_mut() { for t in s.tests.clone() { s.results.insert(t, "not run"); } }
        return;
    }
    // where each test cell lives: the program's file, the files beside it, tests/
    let mut files: Vec<PathBuf> = vec![path.clone()];
    for dir in [root.to_path_buf(), root.join("tests")] {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut found: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "cell")).collect();
        found.sort();
        for f in found { if std::fs::canonicalize(&f).ok() != std::fs::canonicalize(path).ok() { files.push(f); } }
    }
    let mut results: std::collections::BTreeMap<String, &'static str> = std::collections::BTreeMap::new();
    // what the tests said of THIS program is kept: a table is read more often
    // than a program changes (the key: the program's fingerprint and the
    // text of every test file)
    let key = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(program_print.as_bytes());
        for f in &files { if let Ok(t) = std::fs::read_to_string(f) { h.update(f.display().to_string().as_bytes()); h.update(t.as_bytes()); } }
        h.finalize().iter().map(|b| format!("{:02x}", b)).collect::<String>()
    };
    let cache_path = root.join(".soma_cache").join("law-tests.json");
    let cached = std::fs::read_to_string(&cache_path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).filter(|v| v["key"] == key.as_str());
    if let Some(c) = &cached {
        for t in &wanted {
            results.insert(t.clone(), match c["results"][t.as_str()].as_str() { Some("passes") => "passes", Some("fails") => "fails", _ => "not run" });
        }
    }
    let known = cached.is_some();
    for file in files.iter().filter(|_| !known).cloned().collect::<Vec<_>>() {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let here: Vec<&String> = wanted.iter().filter(|t| text.contains(&format!("cell test {} ", t)) || text.contains(&format!("cell test {}{{", t))).collect();
        if here.is_empty() { continue; }
        let shown = file.strip_prefix(root).unwrap_or(&file).display().to_string();
        let Some(answer) = ask(root, &["test", &shown, "--json"]) else {
            for t in here { results.entry(t.clone()).or_insert("not run"); }
            continue;
        };
        for t in here {
            let mine: Vec<&serde_json::Value> = answer["results"].as_array().into_iter().flatten().filter(|r| r["cell"] == t.as_str()).collect();
            let verdict = if mine.is_empty() { "not run" } else if mine.iter().all(|r| r["status"] == "pass") { "passes" } else { "fails" };
            // a test that fails anywhere fails
            let slot = results.entry(t.clone()).or_insert(verdict);
            if verdict == "fails" { *slot = "fails"; }
        }
    }
    if !known && !wanted.is_empty() {
        let _ = std::fs::create_dir_all(root.join(".soma_cache"));
        let _ = std::fs::write(&cache_path, serde_json::json!({ "key": key, "results": results }).to_string());
    }
    for s in report.rules.iter_mut() {
        for t in s.tests.clone() { s.results.insert(t.clone(), results.get(&t).copied().unwrap_or("not run")); }
    }
    // the refutations the project keeps
    let Ok(entries) = std::fs::read_dir(root.join("refutations")) else { return };
    let mut traces: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "trace")).collect();
    traces.sort();
    let entry = path.strip_prefix(root).unwrap_or(path).display().to_string();
    for trace in traces {
        let shown = trace.strip_prefix(root).unwrap_or(&trace).display().to_string();
        let Some(answer) = ask(root, &["refute", &entry, "--trace", &shown, "--json"]) else { continue };
        match answer["verdict"].as_str() {
            Some("refuted") => {
                let rule = answer["rule"].as_str().unwrap_or("").to_string();
                let what = format!("step {}: {} is accepted, then {}", answer["step"], answer["call"].as_str().unwrap_or(""),
                    answer["values"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "));
                if let Some(s) = report.rules.iter_mut().find(|s| s.rule.id == rule) { s.refuted_by.push((shown, what)); }
            }
            // it refuted once, or was written to: said under every rule that has a monitor
            Some("holds") | Some("rejected") => {
                for id in answer["monitors"].as_array().into_iter().flatten().filter_map(|v| v.as_str()) {
                    if let Some(s) = report.rules.iter_mut().find(|s| s.rule.id == id) { s.stale_refutations.push(shown.clone()); }
                }
            }
            _ => {}
        }
    }
}

/// The law a program is under, as a verdict names it: `law.lock 3f2a9c1b`
/// (the first 8 hex of the lock's sha256), `a law not locked yet`, or nothing
/// when the project is under no law.
pub fn lock_name(program: Option<&PathBuf>) -> Option<String> {
    let root = crate::project::root(program?);
    let manifest = crate::pkg::manifest::Manifest::load(&root.join("soma.toml")).ok()?;
    if manifest.law.files.is_empty() { return None; }
    match std::fs::read(root.join("law.lock")) {
        Ok(bytes) => {
            use sha2::{Digest, Sha256};
            let digest = Sha256::digest(&bytes);
            Some(format!("law.lock {}", digest.iter().take(4).map(|b| format!("{:02x}", b)).collect::<String>()))
        }
        Err(_) => Some("a law not locked yet".to_string()),
    }
}
