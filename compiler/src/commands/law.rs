//! `soma law`: the program against the laws it is under (RFC 0002) — the
//! conformity table, the file of one rule, the work list.

use std::path::PathBuf;

use crate::checker::law;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports};

pub fn cmd_law(path: &PathBuf, rule: Option<&str>, todo: bool, lock: bool, json: bool, map: bool) {
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
    let report = law::analyse(&program, &loaded.laws, &law::tests_beside(&root));
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
    let failing = !loaded.errors.is_empty() || matches!(locked, Ok(Some(_))) || report.issues.iter().any(|i| !i.warning);
    if failing { std::process::exit(1); }
}
