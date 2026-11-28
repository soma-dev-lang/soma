use std::path::PathBuf;
use std::process;

use crate::checker;
use crate::registry::Registry;
use super::{read_source, lex_with_location, parse_collecting, resolve_imports, load_meta_cells_from_program};

pub fn cmd_check(path: &PathBuf, json: bool, registry: &mut Registry, law: Option<&str>) {
    // RFC 0006, part C: the perimeter of one law
    let scope = law.map(|l| super::scope::scope_or_exit(path, l));
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    // every syntax error in one pass; the ones that left the AST complete
    // (`=>` in a match arm, `-> T` on a handler) are reported beside the
    // semantic errors instead of hiding them
    let (mut program, syntax_errors) = parse_collecting(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);

    load_meta_cells_from_program(&program, registry, path);

    // Load the project manifest, if present alongside the cell file.
    // It's used by V1.6 model-capability checks; ignored when absent.
    let manifest = Some(crate::project::root(path)).and_then(|d| {
        let toml = d.join("soma.toml");
        if toml.exists() {
            crate::pkg::manifest::Manifest::load(&toml).ok()
        } else { None }
    });

    let mut chk = checker::Checker::new(registry);
    chk.manifest = manifest.as_ref();
    chk.source = Some((file_str.clone(), source.clone()));
    chk.check(&program);
    for e in syntax_errors.iter().rev() {
        chk.errors.insert(0, checker::CheckError::Static {
            kind: "syntax",
            message: e.to_string(),
            span: e.span().unwrap_or(crate::ast::Span { start: 0, end: 0 }),
        });
    }

    let mut holes = checker::flesh::holes(&program);
    // in a perimeter: what speaks of its signals and its rules — the errors stay, all of them
    let mut outside = 0usize;
    if let Some(sc) = &scope {
        let before = chk.warnings.len();
        chk.warnings.retain(|w| sc.names(&w.to_string()));
        outside = before - chk.warnings.len();
        holes.retain(|(c, h)| sc.has_signal(&format!("{}.{}", c, h)));
    }
    if json {
        let report = chk.report_json();
        // a skeleton's unfilled signals: not errors, and not nothing
        match (holes.is_empty(), serde_json::from_str::<serde_json::Value>(&report)) {
            (false, Ok(mut v)) if v.is_object() => {
                v["holes"] = serde_json::json!(holes.iter().map(|(c, h)| format!("{}.{}", c, h)).collect::<Vec<_>>());
                let owed = checker::law::owed(&program, &crate::project::root(path)).owed;
                if !owed.is_empty() { v["owed"] = serde_json::json!(owed.iter().map(|o| serde_json::json!({"rule": o.rule, "signal": o.signal})).collect::<Vec<_>>()); }
                if let Some(n) = checker::fidelity::not_judged(&program, &crate::project::root(path)) { v["not_judged"] = checker::fidelity::json(&n); }
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or(report));
            }
            (true, Ok(mut v)) if v.is_object() => {
                if let Some(n) = checker::fidelity::not_judged(&program, &crate::project::root(path)) { v["not_judged"] = checker::fidelity::json(&n); }
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or(report));
            }
            _ => println!("{}", report),
        }
    } else {
        print!("{}", chk.report());
        if let Some(sc) = &scope {
            if !chk.has_errors() {
                println!("○ the perimeter of the law {} ({}): {} rule{}, {} signal{} in {} cell{}, {} of them with no body yet; {} warning{} outside it not shown  (`soma skeleton {} --index --law {}`)",
                    sc.law, sc.file, sc.rules.len(), if sc.rules.len() == 1 { "" } else { "s" }, sc.signals.len(), if sc.signals.len() == 1 { "" } else { "s" },
                    sc.cells.len(), if sc.cells.len() == 1 { "" } else { "s" }, holes.len(), outside, if outside == 1 { "" } else { "s" }, file_str, sc.file);
                if let Some(n) = super::scope::not_judged(path, sc) {
                    println!("{} — in this perimeter, {}", checker::fidelity::established(&n), checker::fidelity::line(&n));
                }
            }
            if chk.has_errors() { process::exit(1); }
            return;
        }
        if !chk.has_errors() {
            if let Some(note) = checker::flesh::holes_note(&program, &file_str) { println!("{}", note); }
            if chk.law_untested > 0 {
                println!("○ {} rule{} of the law served and with no test — a test shows a case the rule refuses  (`soma law {} --todo`)", chk.law_untested, if chk.law_untested == 1 { "" } else { "s" }, file_str);
            }
            if let Some(note) = super::mirror::note(path) { println!("{}", note); }
            if !holes.is_empty() {
                if let Some(note) = checker::law::owed_note(&checker::law::owed(&program, &crate::project::root(path))) { println!("{}  (`soma law {} --todo`)", note, file_str); }
            }
        }
    }

    // the last line: what holds, and what was not judged (RFC 0006) — a
    // program under a law or with a skeleton does not end on "all good"
    if !json && !chk.has_errors() {
        if let Some(n) = checker::fidelity::not_judged(&program, &crate::project::root(path)) {
            println!("{} — {}", checker::fidelity::established(&n), checker::fidelity::line(&n));
        }
    }

    if chk.has_errors() {
        process::exit(1);
    }
}
