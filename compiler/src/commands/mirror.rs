//! `soma doctor --program app.cell` — the mirror of a program: what of the
//! language its skeletons use, and what they do not state.
//!
//! `soma check` says what is wrong. An agent learns the language from what
//! the toolchain tells it, and a skeleton with no type, no invariant, no
//! contract and no property checks without a word. This says the rest: not
//! errors — a skeleton may have good reasons — but what a skeleton usually
//! states, counted, with the form to write.

use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::checker::frame;
use super::{lex_with_location, parse_with_location, read_source, resolve_imports};

pub struct Gap {
    /// `untyped_face` | `no_invariant` | `no_property` | `no_comment`
    pub kind: &'static str,
    /// `Cell.signal`, `Cell.slot`, or the program
    pub target: String,
    pub message: String,
}

pub struct Mirror {
    /// label, what the program has
    pub lines: Vec<(&'static str, String)>,
    pub gaps: Vec<Gap>,
    /// what the law could name (for its owner)
    pub law_hints: Vec<String>,
    pub skeletons: usize,
}

fn bare(t: &TypeExpr) -> Option<&'static str> {
    match t {
        TypeExpr::Simple(n) if n == "Map" => Some("Map"),
        TypeExpr::Simple(n) if n == "List" => Some("List"),
        _ => None,
    }
}

fn holds_numbers(t: &TypeExpr) -> bool {
    match t {
        TypeExpr::Simple(n) => n == "Int" || n == "Float",
        // a Map or a List of numbers: the value type
        TypeExpr::Generic { args, .. } => args.last().is_some_and(|a| holds_numbers(&a.node)),
        _ => false,
    }
}

/// `name` as a whole word of `text`.
fn names(text: &str, name: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(name).any(|(i, _)| {
        !text[..i].chars().next_back().is_some_and(word) && !text[i + name.len()..].chars().next().is_some_and(word)
    })
}

pub fn mirror(path: &PathBuf) -> Mirror {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    let root = crate::project::root(path);
    let manifest = crate::pkg::manifest::Manifest::load(&root.join("soma.toml")).ok();

    // the ids of the rules of the law: a comment that follows one with a sentence restates the rule
    let rule_ids: Vec<String> = crate::checker::law::load(manifest.as_ref(), &root).laws.iter().flat_map(|l| l.rules.iter().map(|r| r.id.clone())).collect();
    let all = frame::build(&program);
    let skeletons: Vec<&frame::CellSkeleton> = all.iter().filter(|s| s.cell.skeleton).collect();
    let mut gaps: Vec<Gap> = Vec::new();

    // types
    let nominal = crate::checker::nominal::declared().len();
    fn count_types(cells: &[Spanned<CellDef>]) -> usize {
        cells.iter().map(|c| usize::from(matches!(c.node.kind, CellKind::Type)) + c.node.sections.iter().map(|s| match &s.node {
            Section::Interior(i) => count_types(&i.cells), _ => 0 }).sum::<usize>()).sum()
    }
    let types = nominal + count_types(&program.cells);
    let mut signals = 0usize;
    let mut contracts = 0usize;
    let mut untyped = 0usize;
    let mut uncommented = 0usize;
    for sk in &skeletons {
        for b in sk.bones.iter().filter(|b| b.in_face) {
            signals += 1;
            contracts += b.requires.len() + b.ensures.len();
            let who = format!("{}.{}", sk.cell.name, b.name);
            // `request(method, path, body) -> Map` is the HTTP entry: its Map is the response
            let http = b.name == "request";
            let found: Vec<String> = if http { Vec::new() } else { b.params.iter().filter_map(|p| bare(&p.ty.node).map(|t| format!("`{}: {}`", p.name, t)))
                .chain(b.return_type.and_then(|t| bare(&t.node)).map(|t| format!("returns `{}`", t))).collect() };
            if !found.is_empty() {
                untyped += 1;
                gaps.push(Gap { kind: "untyped_face", target: who.clone(), message: format!(
                    "`{}` — {}: a bare Map or List says nothing of what is in it. Declare a record (`cell type Line {{ variants {{ Line {{ sku: String, qty: Int }} }} }}`), or give the types (`Map<String, Int>`, `List<Line>`)", who, found.join(", ")) });
            }
            let comment = super::skeleton::intent_of(b.decl_span, &source, true).join(" ");
            let restated: Vec<&String> = rule_ids.iter().filter(|id| comment.match_indices(id.as_str()).any(|(i, _)| {
                // `ID : sentence`, `ID — sentence`: the rule's text again, in other words
                let after = comment[i + id.len()..].trim_start();
                (after.starts_with(':') || after.starts_with('—') || after.starts_with("- ")) && after.chars().filter(|c| c.is_alphabetic()).count() > 12
            })).collect();
            if !restated.is_empty() {
                gaps.push(Gap { kind: "rule_restated", target: who.clone(), message: format!(
                    "the comment of `{}` restates {} — the brief of the signal already carries the law's own words (its `law` and `owes` lines), and a copy can diverge. Say HOW this signal serves the rule (the parameter, the slot, the call), and name it by its id only",
                    who, restated.iter().map(|r| format!("\"{}\"", r)).collect::<Vec<_>>().join(", ")) });
            }
            if comment.is_empty() {
                uncommented += 1;
                gaps.push(Gap { kind: "no_comment", target: who.clone(), message: format!(
                    "`{}` has no comment: the brief its body is written from will say nothing of what the signal is for — one line above it, `// Pays a redemption of n to holder.`", who) });
            }
        }
    }

    // invariants
    let (mut slots, mut numeric, mut invariants, mut unbounded) = (0usize, 0usize, 0usize, 0usize);
    for sk in &skeletons {
        for section in &sk.cell.sections {
            let Section::Memory(mem) = &section.node else { continue };
            invariants += mem.invariants.len();
            let texts: Vec<String> = mem.invariants.iter().map(|i| render_expr(&i.node)).collect();
            for slot in &mem.slots {
                slots += 1;
                if !holds_numbers(&slot.node.ty.node) { continue; }
                numeric += 1;
                if texts.iter().any(|t| names(t, &slot.node.name)) { continue; }
                unbounded += 1;
                gaps.push(Gap { kind: "no_invariant", target: format!("{}.{}", sk.cell.name, slot.node.name), message: format!(
                    "the slot `{}` of `{}` holds numbers and no invariant bounds it — `invariant {} >= 0` in its `memory` (checked before every write, proven by `soma verify` when every write is bounded), or a type that carries the bound (`cell type Cents = Int where value >= 0`)",
                    slot.node.name, sk.cell.name, slot.node.name) });
            }
        }
    }

    // properties
    let machines: usize = skeletons.iter().map(|sk| sk.cell.sections.iter().filter(|s| matches!(s.node, Section::State(_))).count()).sum();
    let properties = manifest.as_ref().map(|m| {
        let v = &m.verify;
        usize::from(v.deadlock_free) + v.eventually.len() + v.never.len() + v.always.len() + v.after.len() + v.before.len()
    }).unwrap_or(0);
    if machines > 0 && properties == 0 {
        gaps.push(Gap { kind: "no_property", target: file_str.clone(), message: format!(
            "{} state machine{} and no property in soma.toml: `soma verify` proves only what every machine owes (reachability, no deadlock). Say what THIS lifecycle must do — `[verify]` `eventually = [\"closed\"]`, `never = [\"lost\"]`, or a rule between two machines (`[verify.before.\"Pay.paid\"] requires = [\"Approval.signed\"]`)",
            machines, if machines == 1 { "" } else { "s" }) });
    }

    // the law
    let loaded = crate::checker::law::load(manifest.as_ref(), &root);
    let mut law_line = None;
    let mut law_hints = Vec::new();
    if !loaded.laws.is_empty() {
        let report = crate::checker::law::analyse(&program, &loaded.laws, &crate::checker::law::tests_beside(&root));
        let formal: Vec<&crate::checker::law::RuleStatus> = report.rules.iter().filter(|r| r.rule.requires.is_some() || r.rule.always.is_some()).collect();
        let tested = formal.iter().filter(|r| !r.tests.is_empty()).count();
        let exempt: usize = report.realizations.iter().map(|r| r.exempt.len()).sum();
        law_line = Some(format!("{} rule{}, {} with a formal reading; {} of those {} have a test; {} `exempt`",
            report.rules.len(), if report.rules.len() == 1 { "" } else { "s" }, formal.len(), tested, formal.len(), exempt));
        law_hints = report.hints.clone();
    }

    let mut lines: Vec<(&'static str, String)> = vec![
        ("types", format!("{} declared ({} nominal); {} of {} signals take or return a bare Map or List", types, nominal, untyped, signals)),
        ("invariants", format!("{} on {} slots; {} of the {} slots that hold numbers have none", invariants, slots, unbounded, numeric)),
        ("contracts", format!("{} `requires` / `ensures` on {} signals", contracts, signals)),
        ("properties", format!("{} in soma.toml `[verify]` for {} state machine{}", properties, machines, if machines == 1 { "" } else { "s" })),
        ("comments", format!("{} of {} signals have none", uncommented, signals)),
    ];
    if let Some(l) = law_line { lines.push(("law", l)); }
    Mirror { lines, gaps, law_hints, skeletons: skeletons.len() }
}

/// One line for `soma check`, or nothing: the program has skeletons and they leave things unsaid.
pub fn note(path: &PathBuf) -> Option<String> {
    let m = mirror(path);
    if m.skeletons == 0 || m.gaps.is_empty() { return None; }
    let count = |k: &str| m.gaps.iter().filter(|g| g.kind == k).count();
    let mut parts: Vec<String> = Vec::new();
    for (kind, one, many) in [("untyped_face", "signal with a bare Map or List", "signals with a bare Map or List"), ("no_invariant", "slot of numbers with no invariant", "slots of numbers with no invariant"),
        ("no_property", "program with no property to prove", "programs with no property to prove"), ("no_comment", "signal with no comment", "signals with no comment"),
        ("rule_restated", "comment that restates a rule of the law", "comments that restate a rule of the law")] {
        let n = count(kind);
        if n > 0 { parts.push(format!("{} {}", n, if n == 1 { one } else { many })); }
    }
    Some(format!("○ not stated by the skeleton: {}  (`soma doctor --program {}`)", parts.join(", "), path.display()))
}

/// `soma doctor --program app.cell [--json]`. Exits 0: nothing here is an error.
pub fn cmd_mirror(path: &Path, json: bool) {
    let path = path.to_path_buf();
    if !path.is_file() {
        eprintln!("error: {}: no such program — `soma doctor --program app.cell`", path.display());
        std::process::exit(4);
    }
    let m = mirror(&path);
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "schema": "soma.mirror/1", "program": path.display().to_string(), "skeletons": m.skeletons,
            "uses": m.lines.iter().map(|(k, v)| serde_json::json!({ "what": k, "found": v })).collect::<Vec<_>>(),
            "not_stated": m.gaps.iter().map(|g| serde_json::json!({ "kind": g.kind, "target": g.target, "message": g.message })).collect::<Vec<_>>(),
            "law_hints": m.law_hints,
        })).unwrap_or_default());
        return;
    }
    if m.skeletons == 0 {
        println!("{} has no `skeleton`: its cells hold their own handlers, and this mirror reads skeletons (`soma skeleton {} --split` rewrites a program as skeleton + flesh)", path.display(), path.display());
        return;
    }
    println!("What {} uses of the language ({} skeleton{}):\n", path.display(), m.skeletons, if m.skeletons == 1 { "" } else { "s" });
    for (k, v) in &m.lines { println!("  {:<11} {}", k, v); }
    if m.gaps.is_empty() {
        println!("\nNothing a skeleton usually states is missing.");
    } else {
        println!("\nNot stated ({}) — none is an error; each is something the toolchain would hold for you if it were written:\n", m.gaps.len());
        // by kind, the first few of each: a list of 200 lines teaches nothing
        for kind in ["untyped_face", "no_invariant", "no_property", "no_comment", "rule_restated"] {
            let of: Vec<&Gap> = m.gaps.iter().filter(|g| g.kind == kind).collect();
            for g in of.iter().take(3) { println!("  - {}", g.message); }
            if of.len() > 3 { println!("    … and {} more of the same kind: {}", of.len() - 3, of.iter().skip(3).take(8).map(|g| g.target.clone()).collect::<Vec<_>>().join(", ") + if of.len() > 11 { ", …" } else { "" }); }
        }
        println!("\nThe page to read before writing a skeleton: `soma docs skeleton --brief`.");
    }
    if !m.law_hints.is_empty() {
        println!("\nFor the owner of the law ({}) — a state of the law that only `silent` signals change is changed by acts the law does not name. If one is a business act (creating, publishing, declaring), the law names it as an `action` and the signal `realizes` it:\n", m.law_hints.len());
        for h in m.law_hints.iter().take(12) { println!("  - {}", h); }
        if m.law_hints.len() > 12 { println!("    … and {} more (`soma law {} --json`, field `hints`)", m.law_hints.len() - 12, path.display()); }
    }
}
