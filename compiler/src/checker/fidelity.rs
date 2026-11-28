//! What a verdict did not judge (RFC 0006, part A).
//!
//! `soma check` and `soma verify` establish that a program keeps to its
//! frames and to the formal rules of its law. They do not establish that it
//! is faithful to what was meant, and a line that reads "all good" lets
//! whoever reads it — an agent first — believe that it is. For a program
//! under a law or with a skeleton, the last line says both: what holds, and
//! what was not judged.
//!
//! Two kinds of numbers, kept apart:
//!   - what no tool can judge: the rules in sentences, the `silent` signals,
//!     the signals that say neither `entry` nor `internal`;
//!   - what is computed on the frames, with no body, and that makes fidelity
//!     impossible: a signal nobody calls, an event nobody hears, a slot read
//!     only by its writers.

use std::collections::BTreeSet;
use std::path::Path;

use crate::ast::*;
use crate::checker::frame;

#[derive(Debug, Default, Clone)]
pub struct NotJudged {
    /// the program is under a law
    pub under_law: bool,
    /// rules of the law with no formal reading: no tool reads them
    pub sentence_rules: usize,
    /// signals that write under the law and say they are no action of it
    pub silent: Vec<String>,
    /// signals of a skeleton that say neither `entry` nor `internal`
    pub undeclared: Vec<String>,
    /// signals in no `calls` and heard by no `emits`
    pub uncalled: Vec<String>,
    /// events emitted that no signal of the program is named after
    pub unheard: Vec<String>,
    /// slots written, and read by no other signal, no `maps`, no invariant
    pub writer_only: Vec<String>,
    /// `entry` signals that take an entity the law guards and neither realize
    /// an action on it nor call a signal that does. `None`: no signal says
    /// `entry`, so nothing could be computed.
    pub second_paths: Option<Vec<String>>,
    /// signals that write a source of a `held` slot and neither are its
    /// holder nor call it. `None`: no slot is declared `held`.
    pub stale_held: Option<Vec<String>>,
}

/// A finding on the declarations of RFC 0006.
pub struct Issue {
    pub kind: &'static str,
    pub warning: bool,
    pub message: String,
    pub span: Span,
}

/// `Cell.signal` for a name in a `calls` clause of `cell`.
fn qualified(cell: &str, name: &str) -> String {
    if name.contains('.') { name.to_string() } else { format!("{}.{}", cell, name) }
}

/// The type of a parameter as it is written (a nominal type is erased before the program is read here).
fn written(p: &Param) -> String {
    crate::checker::nominal::shown(p.ty.span).unwrap_or_else(|| crate::commands::describe::format_type(&p.ty.node))
}

/// `name` as a whole word of `text`.
fn names(text: &str, name: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    text.match_indices(name).any(|(i, _)| {
        !text[..i].chars().next_back().is_some_and(word) && !text[i + name.len()..].chars().next().is_some_and(word)
    })
}

/// What was not judged, or `None` for a program with no law and no skeleton
/// (its verdict is the one it had).
pub fn not_judged(program: &Program, root: &Path) -> Option<NotJudged> {
    let manifest = crate::pkg::manifest::Manifest::load(&root.join("soma.toml")).ok();
    let loaded = crate::checker::law::load(manifest.as_ref(), root);
    let all = frame::build(program);
    let skeletons: Vec<&frame::CellSkeleton> = all.iter().filter(|s| s.cell.skeleton).collect();
    if loaded.laws.is_empty() && skeletons.is_empty() { return None; }

    let mut out = NotJudged {
        under_law: !loaded.laws.is_empty(),
        sentence_rules: loaded.laws.iter().flat_map(|l| l.rules.iter()).filter(|r| r.requires.is_none() && r.always.is_none()).count(),
        ..Default::default()
    };

    // who is called, and what is heard: over every cell of the program
    let mut called: BTreeSet<String> = BTreeSet::new();
    let mut emitted: BTreeSet<String> = BTreeSet::new();
    for sk in &all {
        for b in &sk.bones {
            for c in &b.frame.calls {
                called.insert(if c.contains('.') { c.clone() } else { format!("{}.{}", sk.cell.name, c) });
            }
            emitted.extend(b.frame.emits.iter().cloned());
        }
        // a schedule calls too
        for s in &sk.schedules {
            for c in &s.frame.calls { called.insert(if c.contains('.') { c.clone() } else { format!("{}.{}", sk.cell.name, c) }); }
            emitted.extend(s.frame.emits.iter().cloned());
        }
    }
    let signal_names: BTreeSet<&str> = all.iter().flat_map(|sk| sk.bones.iter().map(|b| b.name.as_str())).collect();
    out.unheard = emitted.iter().filter(|e| !signal_names.contains(e.as_str())).cloned().collect();

    for sk in &skeletons {
        for b in sk.bones.iter().filter(|b| b.in_face) {
            let who = format!("{}.{}", sk.cell.name, b.name);
            if b.frame.silent.is_some() { out.silent.push(who.clone()); }
            if !b.frame.entry && b.frame.internal.is_none() { out.undeclared.push(who.clone()); }
            // an entry is called from outside: that nobody calls it inside says nothing
            if !b.frame.entry && !called.contains(&who) && !emitted.contains(&b.name) { out.uncalled.push(who); }
        }
        // slots: written by some signal, read by none but their writers
        for section in &sk.cell.sections {
            let Section::Memory(mem) = &section.node else { continue };
            let invariants: Vec<String> = mem.invariants.iter().map(|i| render_expr(&i.node)).collect();
            let maps: Vec<String> = sk.cell.maps.iter().map(|m| render_expr(&m.node.expr.node)).collect();
            for slot in &mem.slots {
                let name = &slot.node.name;
                let writers: Vec<&frame::Bone> = sk.bones.iter().filter(|b| b.frame.writes.contains(name)).collect();
                if writers.is_empty() { continue; }
                let read = sk.bones.iter().any(|b| b.frame.reads.contains(name))
                    || invariants.iter().any(|t| names(t, name)) || maps.iter().any(|t| names(t, name));
                if !read { out.writer_only.push(format!("{}.{}", sk.cell.name, name)); }
            }
        }
    }
    // second paths: only where a signal says `entry`
    if skeletons.iter().any(|sk| sk.bones.iter().any(|b| b.frame.entry)) {
        // entity -> the signals that realize an action with a formal rule that takes it
        let guarded_actions: Vec<&crate::checker::law::ActionDecl> = loaded.laws.iter().flat_map(|l| l.actions.iter())
            .filter(|a| loaded.laws.iter().flat_map(|l| l.rules.iter()).any(|r| r.requires.as_ref().is_some_and(|f| f.action == a.name))).collect();
        let entities: BTreeSet<&str> = guarded_actions.iter().flat_map(|a| a.params.iter().map(|(_, t)| t.as_str()))
            .filter(|t| loaded.laws.iter().any(|l| l.entities.iter().any(|e| e.name == *t))).collect();
        let guards = |entity: &str| -> BTreeSet<String> {
            all.iter().flat_map(|sk| sk.bones.iter().filter(|b| b.frame.bindings.iter().any(|(act, _)|
                guarded_actions.iter().any(|a| &a.name == act && a.params.iter().any(|(_, t)| t == entity))))
                .map(move |b| format!("{}.{}", sk.cell.name, b.name))).collect()
        };
        let mut found = Vec::new();
        for sk in &skeletons {
            for b in sk.bones.iter().filter(|b| b.in_face && b.frame.entry) {
                for p in b.params {
                    let ty = written(p);
                    if !entities.contains(ty.as_str()) { continue; }
                    let bound = b.frame.bindings.iter().any(|(_, args)| args.iter().any(|a| a == &p.name || a.starts_with(&format!("{} via ", p.name))));
                    let through = b.frame.calls.iter().any(|c| guards(&ty).contains(&qualified(&sk.cell.name, c)));
                    if !bound && !through { found.push(format!("{}.{}", sk.cell.name, b.name)); break; }
                }
            }
        }
        out.second_paths = Some(found);
    }

    // held states: only where a slot is declared held
    let held: Vec<(&str, &HeldDecl)> = skeletons.iter().flat_map(|sk| sk.cell.sections.iter().filter_map(move |s| match &s.node {
        Section::Memory(m) => Some(m.held.iter().map(move |h| (sk.cell.name.as_str(), &h.node))), _ => None }).flatten()).collect();
    if !held.is_empty() {
        let mut found = Vec::new();
        for (cell, h) in &held {
            let holder = format!("{}.{}", cell, h.by);
            for src in &h.sources {
                let (src_cell, src_slot) = src.split_once('.').unwrap_or((cell, src.as_str()));
                for sk in all.iter().filter(|sk| sk.cell.name == src_cell) {
                    for b in sk.bones.iter().filter(|b| b.frame.writes.contains(src_slot)) {
                        let who = format!("{}.{}", sk.cell.name, b.name);
                        if who == holder || b.frame.calls.iter().any(|c| qualified(&sk.cell.name, c) == holder) { continue; }
                        if !found.contains(&who) { found.push(who); }
                    }
                }
            }
        }
        out.stale_held = Some(found);
    }
    Some(out)
}

/// What the declarations themselves get wrong (errors), and what they show (warnings).
pub fn issues(program: &Program, root: &Path) -> Vec<Issue> {
    let all = frame::build(program);
    let mut out = Vec::new();
    let Some(n) = not_judged(program, root) else { return out };
    for sk in all.iter().filter(|s| s.cell.skeleton) {
        let cell = &sk.cell.name;
        for b in sk.bones.iter().filter(|b| b.in_face) {
            let who = format!("{}.{}", cell, b.name);
            if b.frame.entry && b.frame.internal.is_some() {
                out.push(Issue { kind: "frame_entry_internal", warning: false, span: b.decl_span,
                    message: format!("`{}` says both `entry` and `internal`: a signal is called from outside the program, or only by its signals — keep one (a signal both use is an `entry`: it must take nothing for granted)", who) });
            }
            if b.frame.internal.as_deref().is_some_and(|w| w.trim().is_empty()) {
                out.push(Issue { kind: "internal_reason", warning: false, span: b.decl_span,
                    message: format!("`{}` is `internal` without a reason — say what its callers already hold: `internal \"the caller holds the account\"`", who) });
            }
            if b.frame.internal.is_some() && n.uncalled.contains(&who) {
                out.push(Issue { kind: "fidelity", warning: true, span: b.decl_span,
                    message: format!("`{}` is `internal` and no signal of the program has it in its `calls` (nor emits `{}`): nothing reaches it — add it to the `calls` of the signal that needs it, say `entry` if it is called from outside, or remove it", who, b.name) });
            }
            if n.second_paths.as_ref().is_some_and(|p| p.contains(&who)) {
                out.push(Issue { kind: "fidelity", warning: true, span: b.decl_span,
                    message: format!("`{}` is an `entry` that takes an entity the law guards, and neither `realizes` an action of the law on it nor has a signal that does in its `calls`: a second path — whoever calls it reaches the entity without the rules that guard it. Realize the action, or call the guarded signal first", who) });
            }
            if n.stale_held.as_ref().is_some_and(|p| p.contains(&who)) {
                out.push(Issue { kind: "fidelity", warning: true, span: b.decl_span,
                    message: format!("`{}` writes a source of a `held` slot and does not have its holder in its `calls`: the held slot goes stale — add the holder to its `calls` (the frame allows the call; the body must make it)", who) });
            }
        }
        for section in &sk.cell.sections {
            let Section::Memory(mem) = &section.node else { continue };
            let slots: BTreeSet<&str> = mem.slots.iter().map(|s| s.node.name.as_str()).collect();
            for h in &mem.held {
                let d = &h.node;
                let mut wrong: Vec<String> = Vec::new();
                if !slots.contains(d.slot.as_str()) { wrong.push(format!("`{}` is not a slot of `{}`", d.slot, cell)); }
                if d.sources.is_empty() { wrong.push("it names no source (`from a, b`)".to_string()); }
                for src in &d.sources {
                    let ok = match src.split_once('.') {
                        Some((c, s)) => all.iter().any(|o| o.cell.name == c && o.cell.sections.iter().any(|x| matches!(&x.node, Section::Memory(m) if m.slots.iter().any(|sl| sl.node.name == s)))),
                        None => slots.contains(src.as_str()),
                    };
                    if !ok { wrong.push(format!("`{}` is not a slot{}", src, if src.contains('.') { " of that cell" } else { " of this cell (a slot of another is `Cell.slot`)" })); }
                }
                match sk.bones.iter().find(|b| b.name == d.by) {
                    None if d.by.is_empty() => wrong.push("it names no holder (`by signal`)".to_string()),
                    None => wrong.push(format!("`{}` is not a signal of `{}`", d.by, cell)),
                    Some(b) if !b.frame.writes.contains(&d.slot) => wrong.push(format!("`{}` does not have `{}` in its `writes`: it cannot rebuild it", d.by, d.slot)),
                    Some(_) => {}
                }
                if !wrong.is_empty() {
                    out.push(Issue { kind: "held_unknown", warning: false, span: h.span,
                        message: format!("`held {} from {} by {}` in `{}`: {} — the form is `held <slot> from <sources> by <signal>`: the signal rebuilds the slot from the sources", d.slot, d.sources.join(", "), d.by, cell, wrong.join("; ")) });
                    continue;
                }
                // a signal other than the holder writes the held slot
                for b in sk.bones.iter().filter(|b| b.name != d.by && b.frame.writes.contains(&d.slot)) {
                    out.push(Issue { kind: "fidelity", warning: true, span: b.decl_span,
                        message: format!("`{}.{}` writes `{}`, which is `held … by {}`: a held slot has one writer, the signal that rebuilds it — call `{}` instead, or the slot is not held", cell, b.name, d.slot, d.by, d.by) });
                }
            }
        }
    }
    out
}

/// The same, for the perimeter of one law: its rules in sentences, and of
/// each list what is of its signals or of its cells.
pub fn restrict(n: &NotJudged, sentence_rules: usize, signals: &BTreeSet<String>, cells: &BTreeSet<String>) -> NotJudged {
    let of = |v: &Vec<String>| -> Vec<String> { v.iter().filter(|w| signals.contains(*w)).cloned().collect() };
    NotJudged {
        under_law: n.under_law,
        sentence_rules,
        silent: of(&n.silent), undeclared: of(&n.undeclared), uncalled: of(&n.uncalled),
        // an event is heard or not by the whole program: said as it is
        unheard: n.unheard.clone(),
        writer_only: n.writer_only.iter().filter(|w| w.split_once('.').is_some_and(|(c, _)| cells.contains(c))).cloned().collect(),
        second_paths: n.second_paths.as_ref().map(of),
        stale_held: n.stale_held.as_ref().map(of),
    }
}

/// What the check did establish, in the words of the last line.
pub fn established(n: &NotJudged) -> &'static str {
    if n.under_law { "conforms to its frames and to the formal rules of its law" } else { "conforms to its frames" }
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

/// The line: `fidelity not judged: …`. Every number is there, zero included:
/// a number left out reads as a thing that was looked at and found right.
pub fn line(n: &NotJudged) -> String {
    format!("fidelity not judged: {} (no tool reads them), {}, {}, {}, {}, {}",
        count(n.sentence_rules, "rule in a sentence", "rules in sentences"),
        count(n.silent.len(), "silent signal", "silent signals"),
        count(n.undeclared.len(), "signal neither entry nor internal", "signals neither entry nor internal"),
        count(n.uncalled.len(), "signal nobody calls", "signals nobody calls"),
        count(n.unheard.len(), "event nobody hears", "events nobody hears"),
        count(n.writer_only.len(), "slot read only by its writers", "slots read only by their writers"))
        + &match &n.second_paths { Some(p) => format!(", {}", count(p.len(), "second path", "second paths")), None => ", second paths not computed (no signal says entry)".to_string() }
        + &match &n.stale_held { Some(p) => format!(", {}", count(p.len(), "held state that can go stale", "held states that can go stale")), None => ", held states not computed (no slot says held)".to_string() }
}

pub fn json(n: &NotJudged) -> serde_json::Value {
    serde_json::json!({
        "rules_in_sentences": n.sentence_rules,
        "silent_signals": n.silent,
        "signals_neither_entry_nor_internal": n.undeclared.len(),
        "signals_nobody_calls": n.uncalled,
        "events_nobody_hears": n.unheard,
        "slots_read_only_by_their_writers": n.writer_only,
        "second_paths": n.second_paths,
        "held_states_that_can_go_stale": n.stale_held,
    })
}
