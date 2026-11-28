//! The perimeter of a law (RFC 0006, part C): `--law lois/x.law`.
//!
//! A large program does not fit in what an agent can read; a law does. The
//! perimeter of a law is its rules, the signals that serve, owe or are
//! exempt from one of them, their cells, the types they name — and its
//! frontier, both ways: what those signals call outside, and what the
//! outside must call inside for a held state to stay right.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::checker::{frame, law};
use super::{lex_with_location, parse_with_location, read_source, resolve_imports};

pub struct Scope {
    /// the law file, as soma.toml names it
    pub file: String,
    pub law: String,
    pub rules: Vec<String>,
    pub formal: usize,
    /// `Cell.signal`
    pub signals: BTreeSet<String>,
    pub cells: BTreeSet<String>,
    /// what the perimeter calls outside: (`Cell.signal`, its signature, it has a body)
    pub frontier: Vec<(String, String, bool)>,
    /// signals outside that write a source of a slot held inside: (`Cell.signal`, the held slot)
    pub held_from_outside: Vec<(String, String)>,
}

impl Scope {
    /// Does this text speak of the perimeter — one of its signals, or one of its rules?
    pub fn names(&self, text: &str) -> bool {
        self.signals.iter().any(|s| text.contains(s.as_str())) || self.rules.iter().any(|r| text.contains(&format!("\"{}\"", r)))
    }
    pub fn has_signal(&self, who: &str) -> bool { self.signals.contains(who) }
}

fn load(path: &PathBuf) -> (Program, String) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    (program, source)
}

/// The perimeter of the law `wanted` (a path as soma.toml writes it, or its file name).
pub fn scope(path: &PathBuf, wanted: &str) -> Result<Scope, String> {
    let (program, _) = load(path);
    let root = crate::project::root(path);
    let manifest = crate::pkg::manifest::Manifest::load(&root.join("soma.toml")).ok();
    let loaded = law::load(manifest.as_ref(), &root);
    let base = |f: &str| Path::new(f).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let Some(the_law) = loaded.laws.iter().find(|l| l.file == wanted || base(&l.file) == base(wanted)) else {
        return Err(format!("`{}` is not a law of this program — its laws (soma.toml `[law] files`): {}", wanted,
            if loaded.laws.is_empty() { "none".to_string() } else { loaded.laws.iter().map(|l| l.file.clone()).collect::<Vec<_>>().join(", ") }));
    };
    let report = law::analyse(&program, &loaded.laws, &law::tests_beside(&root));
    let mut signals: BTreeSet<String> = BTreeSet::new();
    let mut rules = Vec::new();
    let mut formal = 0;
    for s in report.rules.iter().filter(|s| s.rule.law == the_law.name) {
        rules.push(s.rule.id.clone());
        if s.rule.requires.is_some() || s.rule.always.is_some() { formal += 1; }
        // who serves it, owes it, or is exempt from it
        for (who, _) in &s.served { if who.contains('.') && !who.contains(' ') { signals.insert(who.clone()); } }
        for who in &s.missing { if who.contains('.') && !who.contains(' ') { signals.insert(who.clone()); } }
        for (who, _) in &s.exempt { signals.insert(who.clone()); }
    }
    let all = frame::build(&program);
    let cells: BTreeSet<String> = signals.iter().filter_map(|s| s.split_once('.').map(|(c, _)| c.to_string())).collect();
    // the frontier: what the perimeter calls and is not of it
    let mut frontier: Vec<(String, String, bool)> = Vec::new();
    for sk in &all {
        for b in &sk.bones {
            if !signals.contains(&format!("{}.{}", sk.cell.name, b.name)) { continue; }
            for c in &b.frame.calls {
                let who = if c.contains('.') { c.clone() } else { format!("{}.{}", sk.cell.name, c) };
                if signals.contains(&who) || frontier.iter().any(|(w, _, _)| w == &who) { continue; }
                let (cell, name) = who.split_once('.').unwrap_or(("", ""));
                // (a builtin in `calls` is no signal: it is not of the frontier)
                let Some(callee) = all.iter().find(|o| o.cell.name == cell).and_then(|o| o.bones.iter().find(|x| x.name == name)) else { continue };
                let ret = callee.return_type.map(|t| format!(" -> {}", crate::checker::nominal::shown(t.span).unwrap_or_else(|| crate::commands::describe::format_type(&t.node)))).unwrap_or_default();
                let params: Vec<String> = callee.params.iter().map(|p| format!("{}: {}", p.name, crate::checker::nominal::shown(p.ty.span).unwrap_or_else(|| crate::commands::describe::format_type(&p.ty.node)))).collect();
                frontier.push((who.clone(), format!("{}({}){}", who, params.join(", "), ret), !callee.hole && !callee.no_handler));
            }
        }
    }
    // the other way: a slot held inside whose source is written outside
    let mut held_from_outside = Vec::new();
    for sk in all.iter().filter(|sk| cells.contains(&sk.cell.name)) {
        for sec in &sk.cell.sections {
            let Section::Memory(mem) = &sec.node else { continue };
            for h in &mem.held {
                for src in &h.node.sources {
                    let (c, slot) = src.split_once('.').unwrap_or((sk.cell.name.as_str(), src.as_str()));
                    for o in all.iter().filter(|o| o.cell.name == c) {
                        for b in o.bones.iter().filter(|b| b.frame.writes.contains(slot)) {
                            let who = format!("{}.{}", o.cell.name, b.name);
                            if !signals.contains(&who) && !held_from_outside.iter().any(|(w, _)| w == &who) {
                                held_from_outside.push((who, format!("{}.{}", sk.cell.name, h.node.slot)));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(Scope { file: the_law.file.clone(), law: the_law.name.clone(), rules, formal, signals, cells, frontier, held_from_outside })
}

/// The perimeter, or the refusal said and exit 4: a perimeter that does not exist restricts nothing.
pub fn scope_or_exit(path: &PathBuf, wanted: &str) -> Scope {
    match scope(path, wanted) {
        Ok(s) => s,
        Err(m) => { eprintln!("error: {}", m); std::process::exit(4); }
    }
}

/// `soma skeleton app.cell --index --law lois/x.law`: the perimeter, on one screen.
pub fn cmd_index(path: &PathBuf, wanted: &str, json: bool) {
    let s = scope_or_exit(path, wanted);
    let frontier_cells: BTreeSet<&str> = s.frontier.iter().filter_map(|(w, _, _)| w.split_once('.').map(|(c, _)| c)).filter(|c| !s.cells.contains(*c)).collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "schema": "soma.scope/1", "law": s.law, "file": s.file, "rules": s.rules, "formal_rules": s.formal,
            "signals": s.signals, "cells": s.cells,
            "frontier": s.frontier.iter().map(|(w, sig, body)| serde_json::json!({ "signal": w, "signature": sig, "body": body })).collect::<Vec<_>>(),
            "frontier_cells": frontier_cells,
            "held_from_outside": s.held_from_outside.iter().map(|(w, slot)| serde_json::json!({ "signal": w, "slot": slot })).collect::<Vec<_>>(),
        })).unwrap_or_default());
        return;
    }
    println!("the perimeter of the law {} ({}): {} rule{} ({} formal), {} signal{} in {} cell{}\n", s.law, s.file,
        s.rules.len(), if s.rules.len() == 1 { "" } else { "s" }, s.formal,
        s.signals.len(), if s.signals.len() == 1 { "" } else { "s" }, s.cells.len(), if s.cells.len() == 1 { "" } else { "s" });
    for cell in &s.cells {
        let of: Vec<&str> = s.signals.iter().filter_map(|w| w.strip_prefix(&format!("{}.", cell))).collect();
        println!("  {:<22} {}", cell, of.join(", "));
    }
    if s.frontier.is_empty() {
        println!("\nfrontier: none — these signals call nothing outside the perimeter");
    } else {
        println!("\nfrontier: {} signal{} called outside the perimeter, in {} other cell{}", s.frontier.len(), if s.frontier.len() == 1 { "" } else { "s" },
            frontier_cells.len(), if frontier_cells.len() == 1 { "" } else { "s" });
        for (_, sig, body) in &s.frontier { println!("  {}   [{}]", sig, if *body { "body" } else { "hole" }); }
    }
    if !s.held_from_outside.is_empty() {
        println!("\nheld from outside: a slot of the perimeter is rebuilt from what these signals write — they must call its holder");
        for (w, slot) in &s.held_from_outside { println!("  {}   (writes a source of {})", w, slot); }
    }
    println!("\nEvery command takes the same perimeter: `soma check | verify | law | next | doctor --program {} --law {}`", path.display(), s.file);
}

/// What the verdict does not judge, for the perimeter alone.
pub fn not_judged(path: &PathBuf, s: &Scope) -> Option<crate::checker::fidelity::NotJudged> {
    let (program, _) = load(path);
    let root = crate::project::root(path);
    let whole = crate::checker::fidelity::not_judged(&program, &root)?;
    let sentences = s.rules.len() - s.formal;
    Some(crate::checker::fidelity::restrict(&whole, sentences, &s.signals, &s.cells))
}
