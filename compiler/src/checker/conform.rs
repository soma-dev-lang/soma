//! Conformance — a flesh may only do what its frame says
//! (SKELETON_DESIGN.md, phase 3).
//!
//! For every signal of a `skeleton`:
//!
//!  - the frame itself must make sense on the skeleton alone: the slots it
//!    names exist, the edges it moves exist, the signals it calls exist;
//!  - once a flesh gives the signal a body, everything that body does —
//!    and the helpers it reaches do — must be inside the frame.
//!
//! The frame is closed: what no clause grants is refused. That is what
//! makes a skeleton a promise — a body adds no write, no call and no edge
//! the skeleton does not already show, so nothing read off the skeleton is
//! made false by the code written afterwards.
//!
//! A frame wider than the body (once there is one) is a warning: the
//! skeleton promises more than the program does.

use crate::ast::*;
use std::collections::{BTreeSet, HashSet};

use super::frame::{self, Bone, CellSkeleton, Frame};

pub struct Finding {
    pub message: String,
    pub span: Span,
}

fn list(items: impl IntoIterator<Item = String>) -> String {
    let v: Vec<String> = items.into_iter().collect();
    if v.is_empty() { "nothing".to_string() } else { v.iter().map(|i| format!("`{}`", i)).collect::<Vec<_>>().join(", ") }
}

fn edge_exists(sm: &StateMachineSection, from: &str, to: &str) -> bool {
    sm.transitions.iter().any(|t| t.node.to == to && (t.node.from == from
        || (from != "*" && t.node.from == "*" && sm.wildcard_applies(&t.node, from))))
}

/// The machine a frame label names, seen from `cell`: "" (the cell's own —
/// with several, the one that has the state `to`), a machine name, or
/// `Cell.machine`. The key is `Cell.machine`.
fn machine_of<'a>(skeletons: &'a [CellSkeleton<'a>], cell: &'a CellDef, label: &str, to: &str) -> Option<(String, &'a StateMachineSection)> {
    let (owner, name): (&CellDef, Option<&str>) = match label.split_once('.') {
        Some((c, m)) => (skeletons.iter().find(|s| s.cell.name == c)?.cell, Some(m)),
        None => (cell, if label.is_empty() { None } else { Some(label) }),
    };
    let machines = frame::machines_of(owner);
    let sm = match name {
        Some(n) => machines.iter().copied().find(|sm| sm.name == n)?,
        None if machines.len() == 1 => machines[0],
        // several machines and no name: the one that has the state
        None => machines.iter().copied().find(|sm| sm.initial == to || sm.transitions.iter().any(|t| t.node.to == to))?,
    };
    Some((format!("{}.{}", owner.name, sm.name), sm))
}

fn in_face(cell: &CellDef, name: &str) -> bool {
    cell.sections.iter().any(|s| matches!(&s.node, Section::Face(f) if f.declarations.iter().any(|d| match &d.node {
        FaceDecl::Signal(sig) => sig.name == name,
        FaceDecl::Tool(t) => t.name == name,
        _ => false,
    })))
}

fn has_handler(cell: &CellDef, name: &str) -> bool {
    cell.sections.iter().any(|s| matches!(&s.node, Section::OnSignal(on) if on.signal_name == name))
}

/// `Own.h` is `h`.
fn own_call(cell: &CellDef, call: &str) -> String {
    match call.split_once('.') {
        Some((c, h)) if c == cell.name => h.to_string(),
        _ => call.to_string(),
    }
}

/// (machine key, from, to) for every edge a frame's `moves` names.
fn move_keys(skeletons: &[CellSkeleton], cell: &CellDef, f: &Frame) -> BTreeSet<(String, String, String)> {
    f.moves.iter().filter_map(|(label, from, to)| {
        machine_of(skeletons, cell, label, to).map(|(key, _)| (key, from.clone(), to.clone()))
    }).collect()
}

pub fn check(program: &Program) -> (Vec<Finding>, Vec<Finding>) {
    let mut errors: Vec<Finding> = Vec::new();
    let mut warnings: Vec<Finding> = Vec::new();
    let skeletons = frame::build(program);

    // `emit x(…)` runs every handler `on x` of the program. A listener that
    // is a signal answers for what it does with its own frame, as a callee
    // does; a private helper has no frame, so it cannot listen.
    let private: Vec<(&str, &str)> = skeletons.iter().filter(|sk| sk.cell.skeleton)
        .flat_map(|sk| sk.helpers.iter().map(move |h| (h.as_str(), sk.cell.name.as_str()))).collect();
    if !private.is_empty() {
        for sk in &skeletons {
            for b in &sk.bones {
                let emitted: BTreeSet<&String> = b.frame.emits.iter().chain(b.inferred.iter().flat_map(|f| f.emits.iter())).collect();
                for e in emitted {
                    if let Some((_, owner)) = private.iter().find(|(h, _)| h == e) {
                        errors.push(Finding {
                            message: format!("`{}.{}` emits `{e}`, and `{e}` is a private helper of `skeleton {owner}`: an emit would run it outside every frame — emit a signal of its face (`signal {}(…)` with its frame), or call the helper from a signal of `{owner}`", sk.cell.name, b.name, e.trim_start_matches('_')),
                            span: if b.inferred.as_ref().map_or(false, |f| f.emits.contains(e)) { b.span } else { b.decl_span },
                        });
                    }
                }
            }
            for s in &sk.schedules {
                for e in &s.frame.emits {
                    if let Some((_, owner)) = private.iter().find(|(h, _)| h == e) {
                        errors.push(Finding { message: format!("`{}` in `{}` emits `{e}`, a private helper of `skeleton {owner}`: an emit would run it outside every frame", s.label, sk.cell.name), span: s.span });
                    }
                }
            }
        }
    }

    // a slot named like a cell of the program, or like the cell's own machine:
    // `Log.set(k, 1)` is the slot at run time and a call of `Log` in a frame,
    // a read of the slot `order` passes for a read of the machine `order`
    for sk in skeletons.iter().filter(|sk| sk.cell.skeleton) {
        for s in &sk.cell.sections {
            let Section::Memory(m) = &s.node else { continue };
            for sl in &m.slots {
                let n = &sl.node.name;
                if skeletons.iter().any(|o| &o.cell.name == n) {
                    errors.push(Finding { message: format!("slot `{}` of `skeleton {}` has the name of the cell `{}` — `{}.f(…)` would be the slot at run time and a call of the cell in a frame: rename the slot", n, sk.cell.name, n, n), span: sl.span });
                }
                if frame::machines_of(sk.cell).iter().any(|sm| &sm.name == n) {
                    errors.push(Finding { message: format!("slot `{}` of `skeleton {}` has the name of its state machine — a frame could not tell a read of one from a read of the other: rename one of them", n, sk.cell.name), span: sl.span });
                }
            }
        }
    }
    for sk in &skeletons {
        for (caller, callee, span) in &sk.private_calls {
            let (c, h) = callee.split_once('.').unwrap_or(("", callee));
            errors.push(Finding {
                message: format!("`{}.{}` calls `{}`, a private helper of `skeleton {}` — only the signals of its face can be called from another cell; declare `signal {}(…)` there, or call one of its signals", sk.cell.name, caller, callee, c, h),
                span: *span,
            });
        }
        if !sk.cell.skeleton { continue; }
        let cell = sk.cell;
        let slots = frame::slots_of(cell);
        let own_machines = frame::machines_of(cell);

        for b in sk.bones.iter().filter(|b| b.declared) {
            let who = format!("{}.{}", cell.name, b.name);
            let before = errors.len();
            well_formed(&skeletons, sk, b, &who, &slots, &own_machines, &mut errors, &mut warnings);
            let (Some(body), true) = (&b.inferred, errors.len() == before) else { continue };
            let before = errors.len();
            conforms(&skeletons, sk, b, body, &who, &mut errors);
            if errors.len() == before { wider(&skeletons, sk, b, body, &who, &mut warnings); }
        }

        // `runtime { }` has a body and no frame either: in a skeleton it only
        // calls signals (it wrote slots and ran private helpers of the flesh,
        // outside every frame — and `soma run app.cell` executes it)
        for section in &cell.sections {
            let Section::Runtime(rt) = &section.node else { continue };
            let is_signal_call = |e: &Expr| match e {
                Expr::FnCall { name, .. } => in_face(cell, name),
                Expr::MethodCall { target, method, .. } => matches!(&target.node, Expr::Ident(c) if skeletons.iter().any(|o| &o.cell.name == c)) && !method.starts_with('_'),
                _ => false,
            };
            let bad = super::runtime_section::as_statements(rt).iter().any(|st| match &st.node {
                Statement::ExprStmt { expr } | Statement::Let { value: expr, .. } => !is_signal_call(&expr.node),
                Statement::MethodCall { target, method, .. } => !skeletons.iter().any(|o| &o.cell.name == target) || method.starts_with('_'),
                // (`signal greet("a")`: an event raised at start — its listeners are signals)
                Statement::Emit { signal_name, .. } => signal_name.starts_with('_'),
                _ => true,
            });
            if bad {
                errors.push(Finding {
                    message: format!("`runtime {{ }}` in `skeleton {}` does more than call signals — it has no frame of its own: put the work in a signal with its frame and call that signal here", cell.name),
                    span: section.span,
                });
            }
        }
        // a schedule has a body and no frame: in a skeleton it only calls
        for s in &sk.schedules {
            // …signals: another cell's private helper is not this cell's to call
            if let Some(h) = s.frame.calls.iter().find(|c| c.split_once('.').is_some_and(|(o, h)| o != cell.name && h.starts_with('_'))) {
                errors.push(Finding {
                    message: format!("`{}` in `{}` calls `{}`, a private helper of another cell — a helper is reached from its own cell only: call a signal of that cell", s.label, cell.name, h),
                    span: s.span,
                });
            }
            let f = &s.frame;
            let builtins: Vec<String> = f.calls.iter().filter(|c| !c.contains('.') && !in_face(cell, c)).cloned().collect();
            if f.reads.is_empty() && f.writes.is_empty() && f.moves.is_empty() && f.emits.is_empty()
                && !f.dynamic_moves && !f.unknown_machine_read && builtins.is_empty() { continue; }
            errors.push(Finding {
                message: format!("`{}` in `skeleton {}` does more than call signals ({}) — a schedule has no frame of its own: put the work in a signal (`signal tick()` with its frame) and write `{} {{ tick() }}`",
                    s.label, cell.name, frame_text(f), s.label),
                span: s.span,
            });
        }
        for (h, span) in &sk.public_helpers {
            errors.push(Finding {
                message: format!("`{h}` is not a signal of `skeleton {}` — a handler the face does not declare is a private helper, and a private handler is named with a leading underscore: `on _{h}(…)` (never routed, never callable from outside the cell). Or declare `signal {h}(…)` with its frame in the skeleton's face", cell.name),
                span: *span,
            });
        }
        for (h, span) in &sk.unused_helpers {
            if !h.starts_with('_') { continue; }
            warnings.push(Finding {
                message: format!("helper `{}` of `skeleton {}` is called by no signal — remove it, or declare `signal {}(…)` in the face if it is meant to be called from outside", h, cell.name, h),
                span: *span,
            });
        }

        // what the skeleton alone shows to be missing (once its frames hold:
        // a frame in error is not evidence of anything)
        if !errors.is_empty() { continue; }
        let has_effect_blocks = own_machines.iter().any(|sm| sm.transitions.iter().any(|t| !t.node.effect.is_empty()));
        if !has_effect_blocks && sk.bones.iter().all(|b| b.declared) {
            for s in &cell.sections {
                let Section::Memory(mem) = &s.node else { continue };
                for slot in &mem.slots {
                    let n = &slot.node.name;
                    if sk.bones.iter().any(|b| b.frame.writes.contains(n)) { continue; }
                    let readers: Vec<String> = sk.bones.iter().filter(|b| b.frame.reads.contains(n)).map(|b| b.name.clone()).collect();
                    warnings.push(Finding {
                        message: if readers.is_empty() {
                            format!("no signal of `skeleton {}` reads or writes the slot `{}` — remove it, or add it to the frame of the signal that uses it", cell.name, n)
                        } else {
                            format!("no signal of `skeleton {}` may write the slot `{}` ({} read{} it): it stays empty — add `writes {}` to the signal that fills it", cell.name, n, list(readers.clone()), if readers.len() == 1 { "s" } else { "" }, n)
                        },
                        span: slot.span,
                    });
                }
            }
        }
    }

    // an edge of a skeleton's machine that no frame in the program moves
    // …unless some frame can do what no analysis names: a transition it
    // cannot place, or a builtin that runs a handler chosen at run time
    let opaque = skeletons.iter().any(|sk| sk.bones.iter().map(|b| &b.frame).chain(sk.schedules.iter().map(|s| &s.frame))
        .any(|f| f.dynamic_moves || f.moves.iter().any(|(m, _, _)| m == "?") || f.calls.iter().any(|c| frame::OPAQUE_BUILTINS.contains(&c.as_str()))));
    if !opaque && errors.is_empty() {
        let mut claimed: BTreeSet<(String, String, String)> = BTreeSet::new();
        for sk in &skeletons {
            for f in sk.bones.iter().map(|b| &b.frame).chain(sk.schedules.iter().map(|s| &s.frame)) {
                claimed.extend(move_keys(&skeletons, sk.cell, f));
            }
        }
        for sk in skeletons.iter().filter(|sk| sk.cell.skeleton) {
            for s in &sk.cell.sections {
                let Section::State(sm) = &s.node else { continue };
                let key = format!("{}.{}", sk.cell.name, sm.name);
                for t in &sm.transitions {
                    let (from, to) = (&t.node.from, &t.node.to);
                    if claimed.iter().any(|(k, f, x)| k == &key && (x == to || x == "*") && (f == from || f == "*" || from == "*")) { continue; }
                    warnings.push(Finding {
                        message: format!("no signal moves `{} -> {}` of `{}.{}`: the edge can never be taken — add `moves {} -> {}` to the signal that takes it, or remove the edge", from, to, sk.cell.name, sm.name, from, to),
                        span: t.span,
                    });
                }
            }
        }
    }
    (errors, warnings)
}

fn frame_text(f: &Frame) -> String {
    let parts: Vec<String> = f.clauses().iter().map(|(kw, items)| format!("{} {}", kw, items.join(", "))).collect();
    if parts.is_empty() { "an effect the analysis cannot name".to_string() } else { parts.join("  ") }
}

/// The frame as the skeleton declares it makes sense on the skeleton alone.
#[allow(clippy::too_many_arguments)]
fn well_formed(
    skeletons: &[CellSkeleton], sk: &CellSkeleton, b: &Bone, who: &str,
    slots: &HashSet<String>, own_machines: &[&StateMachineSection],
    errors: &mut Vec<Finding>, warnings: &mut Vec<Finding>,
) {
    let cell = sk.cell;
    let span = b.decl_span;
    let mut err = |message: String| errors.push(Finding { message, span });
    let near = |name: &str| super::names::suggest(name, slots.iter())
        .map(|s| format!(" — did you mean `{}`?", s)).unwrap_or_default();
    let foreign_slot = |name: &str| skeletons.iter().find(|o| o.cell.name != cell.name && frame::slots_of(o.cell).contains(name)).map(|o| o.cell.name.clone());

    for w in &b.frame.writes {
        if slots.contains(w) { continue; }
        if own_machines.iter().any(|sm| &sm.name == w) {
            err(format!("`{who}`: `writes {w}` — `{w}` is a state machine: it is moved, not written (`moves from -> to`)"));
        } else if let Some(other) = foreign_slot(w) {
            err(format!("`{who}`: `writes {w}` — `{w}` is a slot of `{other}`; a frame names its own cell's memory: reach `{other}`'s through one of its signals (`calls {other}.…`)"));
        } else {
            err(format!("`{who}`: `writes {w}` names no memory slot of `{}` (slots: {}){}", cell.name, list(sorted(slots)), near(w)));
        }
    }
    for r in &b.frame.reads {
        if slots.contains(r) || own_machines.iter().any(|sm| &sm.name == r) { continue; }
        if let Some((c, m)) = r.split_once('.') {
            let found = skeletons.iter().find(|o| o.cell.name == c).map_or(false, |o| frame::machines_of(o.cell).iter().any(|sm| sm.name == m));
            if !found { err(format!("`{who}`: `reads {r}` — there is no state machine `{m}` in a cell `{c}`")); }
            // as for `moves`: a get_status() written here reads another
            // cell's machine only when this cell has none and the program one
            else if c != cell.name {
                let machines_in_program: usize = skeletons.iter().map(|o| frame::machines_of(o.cell).len()).sum();
                if !own_machines.is_empty() || machines_in_program != 1 {
                    let why = if !own_machines.is_empty() { format!("`{}` has a state machine of its own, and a get_status() written in it reads that one", cell.name) }
                        else { format!("the program has {machines_in_program} state machines, and a cell without one can only read the program's ONLY machine") };
                    err(format!("`{who}`: `reads {r}` — `{who}` cannot read `{r}`: {why}. Call a signal of `{c}` that answers the status (`calls {c}.…`)"));
                }
            }
        } else if let Some(other) = foreign_slot(r) {
            err(format!("`{who}`: `reads {r}` — `{r}` is a slot of `{other}`; a frame names its own cell's memory: reach `{other}`'s through one of its signals (`calls {other}.…`)"));
        } else {
            err(format!("`{who}`: `reads {r}` names no memory slot or state machine of `{}` (slots: {}){}", cell.name, list(sorted(slots)), near(r)));
        }
    }
    for (label, from, to) in &b.frame.moves {
        let shown = if label.is_empty() { format!("{from} -> {to}") } else { format!("{label}: {from} -> {to}") };
        // another cell's machine: a `transition()` written here reaches it
        // only when this cell has no machine and the program has exactly one
        if let Some((c, _)) = label.split_once('.') {
            let machines_in_program: usize = skeletons.iter().map(|o| frame::machines_of(o.cell).len()).sum();
            if c != cell.name && (!own_machines.is_empty() || machines_in_program != 1) {
                let why = if !own_machines.is_empty() { format!("`{}` has a state machine of its own, and a transition() written in it moves that one", cell.name) }
                    else { format!("the program has {machines_in_program} state machines, and a cell without one can only move the program's ONLY machine") };
                err(format!("`{who}`: `moves {shown}` — `{who}` cannot move `{label}`: {why}. Call a signal of `{c}` that takes the edge (`calls {c}.…`)"));
                continue;
            }
        }
        let Some((key, sm)) = machine_of(skeletons, cell, label, to) else {
            if label.is_empty() && own_machines.is_empty() {
                err(format!("`{who}`: `moves {shown}` — `{}` has no state machine; to move another cell's write `moves Cell.machine: {from} -> {to}`", cell.name));
            } else if label.is_empty() {
                err(format!("`{who}`: `moves {shown}` — no state machine of `{}` has a state `{to}`", cell.name));
            } else {
                err(format!("`{who}`: `moves {shown}` — there is no state machine `{label}`"));
            }
            continue;
        };
        let ok = if to == "*" { from == "*" || sm.transitions.iter().any(|t| &t.node.from == from) }
            else if from == "*" { sm.transitions.iter().any(|t| &t.node.to == to) }
            else { edge_exists(sm, from, to) };
        if ok { continue; }
        let into: Vec<String> = sm.transitions.iter().filter(|t| &t.node.to == to).map(|t| format!("{} -> {}", t.node.from, t.node.to)).collect();
        let out: Vec<String> = sm.transitions.iter().filter(|t| &t.node.from == from).map(|t| format!("{} -> {}", t.node.from, t.node.to)).collect();
        let hint = if !into.is_empty() { format!("the edges into `{to}`: {}", list(into)) }
            else if !out.is_empty() { format!("the edges out of `{from}`: {}", list(out)) }
            else { format!("its edges: {}", list(sm.transitions.iter().map(|t| format!("{} -> {}", t.node.from, t.node.to)))) };
        err(format!("`{who}`: `moves {shown}` is not an edge of `{key}` — {hint}"));
    }
    for c in &b.frame.calls {
        if let Some((cn, h)) = c.split_once('.') {
            let Some(other) = skeletons.iter().find(|o| o.cell.name == cn) else {
                err(format!("`{who}`: `calls {c}` — there is no cell `{cn}`{}", super::names::suggest(cn, skeletons.iter().map(|o| &o.cell.name)).map(|s| format!(" — did you mean `{}.{}`?", s, h)).unwrap_or_default()));
                continue;
            };
            let callable: Vec<&String> = other.bones.iter().filter(|x| x.in_face || !other.cell.skeleton).map(|x| &x.name).collect();
            if callable.iter().any(|n| *n == h) { continue; }
            if other.cell.skeleton && has_handler(other.cell, h) {
                err(format!("`{who}`: `calls {c}` — `{h}` is a private helper of `skeleton {cn}`, not a signal of its face"));
            } else {
                err(format!("`{who}`: `calls {c}` — `{cn}` has no signal `{h}`{}", super::names::suggest(h, callable.into_iter()).map(|s| format!(" — did you mean `{}.{}`?", cn, s)).unwrap_or_default()));
            }
            continue;
        }
        if in_face(cell, c) || frame::effectful_builtin(c) { continue; }
        if has_handler(cell, c) {
            err(format!("`{who}`: `calls {c}` — `{c}` is a helper of `{}`: what it does is already counted in this frame; `calls` names signals", cell.name));
        } else if let Some(other) = skeletons.iter().find(|o| o.cell.name != cell.name && o.bones.iter().any(|x| &x.name == c)) {
            err(format!("`{who}`: `calls {c}` — `{c}` is a signal of `{}`: write `calls {}.{c}`", other.cell.name, other.cell.name));
        } else if super::names::builtin_names().contains(c.as_str()) {
            warnings.push(Finding { message: format!("`{who}`: `calls {c}` — `{c}` is a pure builtin: it is free, a frame does not name it"), span });
        } else {
            let known: Vec<String> = sk.bones.iter().filter(|x| x.in_face).map(|x| x.name.clone()).collect();
            err(format!("`{who}`: `calls {c}` names no signal of `{}` and no builtin{}", cell.name,
                super::names::suggest(c, known.iter()).map(|s| format!(" — did you mean `{}`?", s)).unwrap_or_default()));
        }
    }
}

fn sorted(set: &HashSet<String>) -> Vec<String> {
    let mut v: Vec<String> = set.iter().cloned().collect();
    v.sort();
    v
}

/// Everything the body does is inside the frame.
fn conforms(skeletons: &[CellSkeleton], sk: &CellSkeleton, b: &Bone, body: &Frame, who: &str, errors: &mut Vec<Finding>) {
    let cell = sk.cell;
    let d = &b.frame;
    let amend = format!("if the design means it, add it to `signal {}` in `skeleton {}`", b.name, cell.name);
    let mut err = |message: String| errors.push(Finding { message, span: b.span });
    let clause = |kw: &str, items: &BTreeSet<String>| if items.is_empty() { format!("no `{kw}`") } else { format!("`{kw} {}`", items.iter().cloned().collect::<Vec<_>>().join(", ")) };

    for w in body.writes.iter().filter(|w| !d.writes.contains(*w)) {
        err(format!("`{who}` writes `{w}`, outside its frame ({}) — {amend}: `writes {w}`", clause("writes", &d.writes)));
    }
    // (a machine the frame moves is already out of `body.reads`: the frame
    // inference drops it — here the DECLARED moves grant the read)
    let moved: HashSet<String> = d.moves.iter().filter_map(|(label, _, to)| machine_of(skeletons, cell, label, to))
        .map(|(key, sm)| if key.starts_with(&format!("{}.", cell.name)) { sm.name.clone() } else { key }).collect();
    for r in body.reads.iter().filter(|r| !d.reads.contains(*r) && !d.writes.contains(*r) && !moved.contains(*r)) {
        err(format!("`{who}` reads `{r}`, outside its frame ({}) — {amend}: `reads {r}`", clause("reads", &d.reads)));
    }
    let may_call: BTreeSet<String> = d.calls.iter().map(|c| own_call(cell, c)).collect();
    for c in body.calls.iter().filter(|c| !may_call.contains(*c)) {
        err(format!("`{who}` calls `{c}`, outside its frame ({}) — {amend}: `calls {c}`", clause("calls", &d.calls)));
    }
    for e in body.emits.iter().filter(|e| !d.emits.contains(*e)) {
        err(format!("`{who}` emits `{e}`, outside its frame ({}) — {amend}: `emits {e}`", clause("emits", &d.emits)));
    }
    let may_move = move_keys(skeletons, cell, d);
    let shown: Vec<String> = d.moves.iter().map(|(l, f, t)| if l.is_empty() { format!("{f} -> {t}") } else { format!("{l}: {f} -> {t}") }).collect();
    let frame_moves = if shown.is_empty() { "no `moves`".to_string() } else { format!("`moves {}`", shown.join(", ")) };
    let label_of = |label: &str, key: &str| if label.is_empty() { String::new() } else if label.contains('.') { format!("{key}: ") } else { format!("{label}: ") };
    for (label, from, to) in &body.sure_moves {
        let Some((key, _)) = machine_of(skeletons, cell, label, to) else {
            err(format!("`{who}` moves `{from} -> {to}`, and the analysis cannot tell which state machine that is — call a signal of the cell that owns the machine"));
            continue;
        };
        if may_move.iter().any(|(k, f, t)| k == &key && (t == to || t == "*") && (f == from || f == "*")) { continue; }
        if to == "*" {
            err(format!("`{who}` calls transition() with a target computed at run time, so it can take any edge of `{key}` — outside its frame ({frame_moves}). Write the targets out (`if to == \"paid\" {{ transition(id, \"paid\") }}`; a typed state is written bare, `transition(id, Paid)`), or {amend}: `moves {}* -> *`", label_of(label, &key)));
            continue;
        }
        err(format!("`{who}` moves `{from} -> {to}`, outside its frame ({frame_moves}) — {amend}: `moves {}{from} -> {to}`", label_of(label, &key)));
    }
    // `transition(id, to)` from a state the body did not establish takes
    // WHICHEVER edge into `to` the instance is on: every one of them must
    // be in the frame (a body under `moves resolved -> closed` also took
    // `new -> closed`)
    for (label, to) in &body.target_moves {
        let Some((key, sm)) = machine_of(skeletons, cell, label, to) else {
            err(format!("`{who}` moves an instance to `{to}`, and the analysis cannot tell which state machine that is — call a signal of the cell that owns the machine"));
            continue;
        };
        let covered = |from: &str| may_move.iter().any(|(k, f, t)| k == &key && (t == to || t == "*") && (f == from || f == "*"));
        let into: Vec<&str> = sm.transitions.iter().filter(|t| &t.node.to == to).map(|t| t.node.from.as_str()).collect();
        let missing: Vec<String> = into.iter().filter(|from| !covered(from)).map(|from| format!("{from} -> {to}")).collect();
        if missing.is_empty() && !into.is_empty() { continue; }
        if into.is_empty() { continue; } // no such state: the refinement check says so
        let granted = into.len() - missing.len();
        if granted == 0 {
            err(format!("`{who}` moves an instance to `{to}`, outside its frame ({frame_moves}) — {amend}: `moves {}{}`", label_of(label, &key), missing.join(", ")));
        } else {
            err(format!("`{who}` calls `transition(id, \"{to}\")` without saying from where, so it also takes {} — outside its frame ({frame_moves}). Name the source (`transition(id, \"from\", \"{to}\")`), or {amend}: `moves {}{}`",
                list(missing.clone()), label_of(label, &key), missing.join(", ")));
        }
    }
    if body.dynamic_moves {
        err(format!("`{who}` calls transition() with a target computed at run time, and `{}` cannot tell which state machine that moves (it has none; the program has several) — call a signal of the cell that owns the machine", cell.name));
    }
    if body.moves.iter().any(|(m, _, _)| m == "?") {
        err(format!("`{who}` calls transition() toward a state the analysis finds in no single state machine — name the cell that owns the machine by calling one of its signals"));
    }
    if body.plain_map_lambda {
        err(format!("`{who}`: `map(a, f)` written as a plain call builds a Map holding `f` when `a` is a String, so its lambda is not known to run here — and this one reads or writes memory, calls a handler or moves a machine. Write `xs.map(x => …)` or `xs |> map(x => …)`, which always apply it"));
    } else if body.escaping_lambda {
        err(format!("`{who}` writes an impure lambda (it reads or writes memory, calls a handler or an effectful builtin, or moves a machine) somewhere other than as the direct argument of a builtin that runs it at once — a lambda that is bound, stored, passed to a signal or returned runs later, in the cell of whoever calls it, outside every frame. Write it inline (`xs.map(x => …)`), or pass data and let a signal do the work"));
    }
    if body.unknown_machine_read {
        err(format!("`{who}` reads a state machine (get_status / has_state / valid_transitions) but `{}` has none and the program has several — call a signal of the cell that owns the machine", cell.name));
    }
}

/// The frame promises more than the body does.
fn wider(skeletons: &[CellSkeleton], sk: &CellSkeleton, b: &Bone, body: &Frame, who: &str, warnings: &mut Vec<Finding>) {
    let cell = sk.cell;
    let d = &b.frame;
    let mut warn = |message: String| warnings.push(Finding { message, span: b.decl_span });
    for w in d.writes.iter().filter(|w| !body.writes.contains(*w)) {
        if body.reads.contains(w) {
            warn(format!("`{who}`: the frame says `writes {w}`, the body only reads it — declare `reads {w}`"));
        } else {
            warn(format!("`{who}`: the frame says `writes {w}`, the body never touches `{w}` — narrow the frame"));
        }
    }
    // (a machine the frame also moves: `moves` already grants the read, and
    // the inferred frame drops it — whether the body reads it is not known here)
    let moved: HashSet<String> = d.moves.iter().filter_map(|(label, _, to)| machine_of(skeletons, cell, label, to))
        .map(|(key, sm)| if key.starts_with(&format!("{}.", cell.name)) { sm.name.clone() } else { key }).collect();
    for r in d.reads.iter().filter(|r| !body.reads.contains(*r) && !body.writes.contains(*r) && !moved.contains(*r)) {
        warn(format!("`{who}`: the frame says `reads {r}`, the body never reads it — narrow the frame"));
    }
    for c in d.calls.iter().filter(|c| !body.calls.contains(&own_call(cell, c))) {
        // a pure builtin was already reported by the frame's own check
        if !c.contains('.') && !in_face(cell, c) && !frame::effectful_builtin(c) { continue; }
        warn(format!("`{who}`: the frame says `calls {c}`, the body never calls it — narrow the frame"));
    }
    for e in d.emits.iter().filter(|e| !body.emits.contains(*e)) {
        warn(format!("`{who}`: the frame says `emits {e}`, the body never emits it — narrow the frame"));
    }
    for (label, from, to) in &d.moves {
        let Some((key, _)) = machine_of(skeletons, cell, label, to) else { continue };
        let same = |l: &str, t: &str| (t == to || to == "*" || t == "*") && machine_of(skeletons, cell, l, t).map_or(false, |(k, _)| k == key);
        let used = body.target_moves.iter().any(|(l, t)| same(l, t))
            || body.sure_moves.iter().any(|(l, f, t)| same(l, t) && (f == from || from == "*" || f == "*"));
        if !used { warn(format!("`{who}`: the frame says `moves {from} -> {to}`, the body never takes that edge — narrow the frame")); }
    }
}
