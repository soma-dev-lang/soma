//! Flesh — the handler bodies of a skeleton (SKELETON_DESIGN.md, phase 2).
//!
//! `skeleton Name { … }` declares the structure; `flesh Name { on f(…) { … } }`
//! gives the bodies, in the same file or in one a `use` brings in. `attach`
//! moves each body into its skeleton — after it the program is ordinary
//! cells with ordinary handlers, which is all the interpreter, the native
//! backend and the verifier ever see.
//!
//! A body that cannot be attached stays behind, in a `flesh` cell left in
//! the program; `check` says why. A signal no body was attached to keeps
//! the handler the parser gave it: a *hole* (property `hole`), which fails
//! with `not_implemented` when called.

use crate::ast::*;

/// Why a flesh handler cannot go into `target`.
enum Refusal {
    Duplicate,
    /// the skeleton's parameters, as its signal declares them
    Params(Vec<Param>),
    /// a helper (no face signal of that name) with an untyped parameter
    UntypedHelper(String),
}

fn type_text(t: &TypeExpr) -> String {
    match t {
        TypeExpr::Simple(n) => n.clone(),
        TypeExpr::Generic { name, args } => format!("{}<{}>", name, args.iter().map(|a| type_text(&a.node)).collect::<Vec<_>>().join(", ")),
        TypeExpr::CellRef { cell, member } => format!("{}.{}", cell, member),
    }
}

fn untyped(p: &Param) -> bool {
    matches!(&p.ty.node, TypeExpr::Simple(t) if t == FLESH_PARAM_TYPE)
}

/// The parameters the face of `cell` declares for `name` (a signal or a tool).
fn face_params(cell: &CellDef, name: &str) -> Option<Vec<Param>> {
    cell.sections.iter().find_map(|s| {
        let Section::Face(face) = &s.node else { return None };
        face.declarations.iter().find_map(|d| match &d.node {
            FaceDecl::Signal(sig) if sig.name == name => Some(sig.params.clone()),
            FaceDecl::Tool(t) if t.name == name => Some(t.params.clone()),
            _ => None,
        })
    })
}

fn handler_index(cell: &CellDef, name: &str) -> Option<usize> {
    cell.sections.iter().position(|s| matches!(&s.node, Section::OnSignal(on) if on.signal_name == name))
}

pub fn is_hole(on: &OnSection) -> bool {
    on.properties.iter().any(|p| p == HOLE)
}

fn refusal(target: &CellDef, on: &OnSection) -> Option<Refusal> {
    let existing = handler_index(target, &on.signal_name);
    match face_params(target, &on.signal_name) {
        Some(declared) => {
            if let Some(i) = existing {
                if !matches!(&target.sections[i].node, Section::OnSignal(o) if is_hole(o)) { return Some(Refusal::Duplicate); }
            }
            let same = on.params.len() == declared.len() && on.params.iter().zip(&declared)
                .all(|(p, d)| p.name == d.name && (untyped(p) || type_text(&p.ty.node) == type_text(&d.ty.node)));
            if !same { return Some(Refusal::Params(declared)); }
            None
        }
        None => {
            if existing.is_some() { return Some(Refusal::Duplicate); }
            on.params.iter().find(|p| untyped(p)).map(|p| Refusal::UntypedHelper(p.name.clone()))
        }
    }
}

fn find_skeleton<'a>(cells: &'a mut [Spanned<CellDef>], name: &str) -> Option<&'a mut CellDef> {
    for c in cells.iter_mut() {
        if c.node.name == name && c.node.skeleton { return Some(&mut c.node); }
        for s in c.node.sections.iter_mut() {
            if let Section::Interior(interior) = &mut s.node {
                if let Some(found) = find_skeleton(&mut interior.cells, name) { return Some(found); }
            }
        }
    }
    None
}

/// Move every flesh body into its skeleton. Idempotent: called after each
/// parse and again once the imports are in.
pub fn attach(program: &mut Program) {
    if !program.cells.iter().any(|c| c.node.kind == CellKind::Flesh) { return; }
    let (flesh, cells): (Vec<_>, Vec<_>) = std::mem::take(&mut program.cells).into_iter()
        .partition(|c| c.node.kind == CellKind::Flesh);
    program.cells = cells;
    let mut left_over: Vec<Spanned<CellDef>> = Vec::new();
    for f in flesh {
        let Some(target) = find_skeleton(&mut program.cells, &f.node.name) else {
            left_over.push(f);
            continue;
        };
        let Spanned { node: mut cell, span } = f;
        let mut refused = Vec::new();
        for section in std::mem::take(&mut cell.sections) {
            let Section::OnSignal(mut on) = section.node else { continue };
            if refusal(target, &on).is_some() {
                refused.push(Spanned::new(Section::OnSignal(on), section.span));
                continue;
            }
            // the signal's own parameters: a flesh may leave the types out
            if let Some(declared) = face_params(target, &on.signal_name) { on.params = declared; }
            let placed = Spanned::new(Section::OnSignal(on), section.span);
            let name = match &placed.node { Section::OnSignal(o) => o.signal_name.clone(), _ => unreachable!() };
            match handler_index(target, &name) {
                Some(i) => target.sections[i] = placed,
                None => target.sections.push(placed),
            }
        }
        if !refused.is_empty() {
            cell.sections = refused;
            left_over.push(Spanned::new(cell, span));
        }
    }
    program.cells.extend(left_over);
}

/// One message per body `attach` left behind.
pub fn check(program: &Program) -> Vec<(String, Span)> {
    let mut out = Vec::new();
    let all = super::names::collect_cells(program);
    for f in program.cells.iter().filter(|c| c.node.kind == CellKind::Flesh) {
        let name = &f.node.name;
        let Some(target) = all.iter().copied().find(|c| &c.name == name && c.skeleton) else {
            let msg = if all.iter().any(|c| &c.name == name && c.kind != CellKind::Flesh) {
                format!("flesh `{name}`: `{name}` is a `cell`, not a `skeleton` — a cell holds its own handlers; write `skeleton {name} {{ … }}` to give it a flesh")
            } else {
                let hint = super::names::suggest(name, all.iter().filter(|c| c.skeleton).map(|c| &c.name))
                    .map(|s| format!(" — did you mean `flesh {}`?", s)).unwrap_or_default();
                // a flesh file checked by itself
                let alone = if all.iter().any(|c| c.skeleton) { "" } else { " in this program — a flesh is checked with the file that holds its skeleton (`soma check app.cell`, which imports this one)" };
                format!("flesh `{name}`: there is no `skeleton {name}`{alone}{hint}")
            };
            out.push((msg, f.span));
            continue;
        };
        for section in &f.node.sections {
            let Section::OnSignal(on) = &section.node else { continue };
            let h = &on.signal_name;
            let msg = match refusal(target, on) {
                Some(Refusal::Duplicate) => format!("`{name}.{h}` has two bodies — a signal has one `on {h}` across every `flesh {name}`"),
                Some(Refusal::Params(declared)) => format!(
                    "flesh `on {h}({})` does not match the skeleton's `signal {h}({})` — write `on {h}({})`",
                    on.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", "),
                    declared.iter().map(|p| format!("{}: {}", p.name, type_text(&p.ty.node))).collect::<Vec<_>>().join(", "),
                    declared.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ")),
                Some(Refusal::UntypedHelper(p)) => {
                    // a body left behind by a rename or a removal in the skeleton
                    let unfilled: Vec<String> = target.sections.iter().filter_map(|s| match &s.node {
                        Section::OnSignal(o) if o.properties.iter().any(|p| p == HOLE) => Some(o.signal_name.clone()),
                        _ => None,
                    }).collect();
                    let renamed = match unfilled.as_slice() {
                        [] => String::new(),
                        some => format!(". If the signal was renamed in the skeleton, this is its body: `skeleton {name}` has no body for {}", some.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")),
                    };
                    format!("`{h}` is not a signal of `skeleton {name}`, so it is a private helper and its parameters need types: `{p}: Int` / `String` / `Map` … (or declare `signal {h}(…)` in the skeleton's face){renamed}")
                }
                None => continue,
            };
            out.push((msg, section.span));
        }
    }
    out
}

/// The holes of the program: (cell, signal), in source order.
pub fn holes(program: &Program) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for cell in super::names::collect_cells(program) {
        for s in &cell.sections {
            if let Section::OnSignal(on) = &s.node {
                if is_hole(on) { out.push((cell.name.clone(), on.signal_name.clone())); }
            }
        }
    }
    out
}

/// The frame a face signal of `cell` declares.
pub fn declared_frame<'a>(cell: &'a CellDef, signal: &str) -> Option<&'a FrameDecl> {
    cell.sections.iter().find_map(|s| {
        let Section::Face(face) = &s.node else { return None };
        face.declarations.iter().find_map(|d| match &d.node {
            FaceDecl::Signal(sig) if sig.name == signal => sig.frame.as_ref(),
            FaceDecl::Tool(t) if t.name == signal => t.frame.as_ref(),
            _ => None,
        })
    })
}

/// The hole whose frame declares the edge `from -> to` of machine `machine`
/// of cell `machine_cell` (`Cell.signal`): the edge is not dead, it waits.
pub fn hole_claiming(program: &Program, machine_cell: &str, machine: &str, from: &str, to: &str) -> Option<String> {
    for cell in super::names::collect_cells(program) {
        for s in &cell.sections {
            let Section::OnSignal(on) = &s.node else { continue };
            if !is_hole(on) { continue; }
            let Some(frame) = declared_frame(cell, &on.signal_name) else { continue };
            let claims = frame.moves.iter().any(|(label, f, t)| {
                let (c, m) = match label.split_once('.') {
                    Some((c, m)) => (c, m),
                    None => (cell.name.as_str(), label.as_str()),
                };
                c == machine_cell && (m.is_empty() || m == machine) && (f == from || f == "*") && (t == to || t == "*")
            });
            if claims { return Some(format!("{}.{}", cell.name, on.signal_name)); }
        }
    }
    None
}

/// One line for `soma check` / `soma verify` when the program has holes.
pub fn holes_note(program: &Program, file: &str) -> Option<String> {
    let holes = holes(program);
    if holes.is_empty() { return None; }
    let names: Vec<String> = holes.iter().take(6).map(|(c, h)| format!("{}.{}", c, h)).collect();
    let more = if holes.len() > names.len() { format!(", … ({} more)", holes.len() - names.len()) } else { String::new() };
    Some(format!("○ {} hole{} — signal{} of a skeleton with no body yet: {}{}  (`soma holes {}`)",
        holes.len(), if holes.len() == 1 { "" } else { "s" }, if holes.len() == 1 { "" } else { "s" }, names.join(", "), more, file))
}

// ── a hole, as `soma verify` reads it ────────────────────────────────
//
// A hole has no body yet; ANY body that conforms to its frame may come.
// So a proof must hold for all of them: for the proofs, a hole is the
// body that does everything its frame allows, each thing or not —
//
//     moves a -> b      if ? { transition(?, "a", "b") }        (every edge named, `*` expanded)
//     writes s          if ? { s.set(?, ?) }                    (any value)
//     calls f, C.g      if ? { f(?, …) }  if ? { C.g(?, …) }
//     emits e           if ? { emit e(?, …) }
//     return ?
//
// The analyses already take a condition both ways and an unknown value as
// unknown, so a `✓` they give on this body is a `✓` for every conforming
// flesh, and what they cannot prove for it is reported, not assumed.

/// An unknown value, and an unknown condition: a name no program can
/// write, which every analysis reads as "anything" and prints as `?`.
const UNKNOWN: &str = "?";

/// Replace the body of every hole by the model of its frame. For
/// `soma verify` only: the result is not a program to run or to check.
/// In a hole's model: `__status_in(id, "Cell.s", …)` — the instance `id` of
/// `Cell`'s machine is in one of those states. Read by `checker::product`.
pub const STATUS_IN: &str = "\u{1f}status_in";

pub fn model_holes(program: &mut Program) {
    if holes(program).is_empty() { return; }
    let snapshot = program.clone();
    let all = super::names::collect_cells(&snapshot);
    let machines: Vec<(&CellDef, &StateMachineSection)> = all.iter().copied()
        .flat_map(|c| c.sections.iter().filter_map(move |s| if let Section::State(sm) = &s.node { Some((c, sm)) } else { None }))
        .collect();
    fn rec(cells: &mut [Spanned<CellDef>], all: &[&CellDef], machines: &[(&CellDef, &StateMachineSection)]) {
        for c in cells.iter_mut() {
            let frames: Vec<(String, Option<FrameDecl>)> = c.node.sections.iter().filter_map(|s| match &s.node {
                Section::OnSignal(on) if is_hole(on) => Some((on.signal_name.clone(), declared_frame(&c.node, &on.signal_name).cloned())),
                _ => None,
            }).collect();
            let cell = c.node.clone();
            for s in c.node.sections.iter_mut() {
                match &mut s.node {
                    Section::OnSignal(on) if is_hole(on) => {
                        let frame = frames.iter().find(|(n, _)| n == &on.signal_name).and_then(|(_, f)| f.clone()).unwrap_or_default();
                        on.body = model_body(&cell, &frame, s.span, all, machines);
                    }
                    Section::Interior(i) => rec(&mut i.cells, all, machines),
                    _ => {}
                }
            }
        }
    }
    rec(&mut program.cells, &all, &machines);
}

fn model_body(cell: &CellDef, frame: &FrameDecl, span: Span, all: &[&CellDef], machines: &[(&CellDef, &StateMachineSection)]) -> Vec<Spanned<Statement>> {
    let sp = |e: Expr| Spanned::new(e, span);
    let unknown = || sp(Expr::Ident(UNKNOWN.to_string()));
    let args = |n: usize| (0..n).map(|_| unknown()).collect::<Vec<_>>();
    let maybe = |st: Statement| Spanned::new(Statement::If {
        condition: unknown(),
        then_body: vec![Spanned::new(st, span)],
        else_body: vec![],
    }, span);
    let arity = |c: &CellDef, h: &str| c.sections.iter().find_map(|s| match &s.node {
        Section::OnSignal(on) if on.signal_name == h => Some(on.params.len()),
        _ => None,
    });
    let mut body: Vec<Spanned<Statement>> = Vec::new();

    for (label, from, to) in &frame.moves {
        // the machine the label names: the cell's own, or (from a cell
        // without one) the program's only machine — the two a
        // `transition()` written in this cell can reach
        let own: Vec<&StateMachineSection> = machines.iter().filter(|(c, _)| c.name == cell.name).map(|(_, sm)| *sm).collect();
        let sm = match label.split_once('.') {
            Some((c, m)) => machines.iter().find(|(mc, sm)| mc.name == c && sm.name == m).map(|(_, sm)| *sm),
            None if label.is_empty() => own.first().copied().or_else(|| (machines.len() == 1).then(|| machines[0].1)),
            None => own.iter().copied().find(|sm| &sm.name == label),
        };
        let Some(sm) = sm else { continue };
        let mut states: Vec<&str> = vec![sm.initial.as_str()];
        for t in &sm.transitions {
            for x in [t.node.from.as_str(), t.node.to.as_str()] { if x != "*" && !states.contains(&x) { states.push(x); } }
        }
        let has_edge = |f: &str, t: &str| sm.transitions.iter().any(|e| e.node.to == t && (e.node.from == f || (e.node.from == "*" && sm.wildcard_applies(&e.node, f))));
        for f in states.iter().copied().filter(|f| from == "*" || f == from) {
            for t in states.iter().copied().filter(|t| to == "*" || t == to) {
                if !has_edge(f, t) { continue; }
                let lit = |x: &str| sp(Expr::Literal(Literal::String(x.to_string())));
                let take = Statement::ExprStmt { expr: sp(Expr::FnCall { name: "transition".to_string(), args: vec![unknown(), lit(f), lit(t)] }) };
                // `when Other.s`: the edge is taken only under its condition — what the
                // frame promises, and what `verify` holds every body to
                let cond = frame.conditions.iter().find(|c| &c.machine == label && (c.from == f || c.from == "*") && (c.to == t || c.to == "*"));
                match cond {
                    Some(c) => {
                        let id = match &c.via {
                            Some(v) => sp(Expr::MethodCall { target: Box::new(sp(Expr::Ident(v.clone()))), method: "get".to_string(), args: vec![unknown()] }),
                            None => unknown(),
                        };
                        let mut args = vec![id];
                        args.extend(c.states.iter().map(|s| lit(s)));
                        body.push(maybe(Statement::If { condition: sp(Expr::FnCall { name: STATUS_IN.to_string(), args }), then_body: vec![Spanned::new(take, span)], else_body: vec![] }));
                    }
                    None => body.push(maybe(take)),
                }
            }
        }
    }
    for slot in &frame.writes {
        let is_list = cell.sections.iter().any(|s| matches!(&s.node, Section::Memory(m) if m.slots.iter().any(|sl| &sl.node.name == slot
            && matches!(&sl.node.ty.node, TypeExpr::Generic { name, .. } | TypeExpr::Simple(name) if name == "List"))));
        body.push(maybe(if is_list {
            Statement::MethodCall { target: slot.clone(), method: "push".to_string(), args: vec![unknown()] }
        } else {
            Statement::MethodCall { target: slot.clone(), method: "set".to_string(), args: vec![unknown(), unknown()] }
        }));
    }
    for call in &frame.calls {
        let expr = match call.split_once('.') {
            Some((c, h)) => {
                let n = all.iter().copied().find(|x| x.name == c).and_then(|x| arity(x, h)).unwrap_or(0);
                Expr::MethodCall { target: Box::new(sp(Expr::Ident(c.to_string()))), method: h.to_string(), args: args(n) }
            }
            None => match arity(cell, call) {
                Some(n) => Expr::FnCall { name: call.clone(), args: args(n) },
                // a builtin: one unknown argument (an unbounded `think`)
                None => Expr::FnCall { name: call.clone(), args: args(1) },
            },
        };
        body.push(maybe(Statement::ExprStmt { expr: sp(expr) }));
    }
    for event in &frame.emits {
        let n = all.iter().copied().find_map(|x| arity(x, event)).unwrap_or(0);
        body.push(maybe(Statement::Emit { signal_name: event.clone(), args: args(n) }));
    }
    // a body may do what its frame allows in ANY order and ANY number of
    // times (call twice, take `open -> closing` then `closing -> done`): the
    // model is that set of steps under a loop, not one ordered pass — a rule
    // ticked on a skeleton must hold for every flesh that conforms
    let mut body = if body.is_empty() { body } else {
        vec![Spanned::new(Statement::While { condition: unknown(), body, bound: None }, span)]
    };
    body.push(Spanned::new(Statement::Return { value: unknown() }, span));
    body
}

/// The holes of one cell.
pub fn holes_of(cell: &CellDef) -> Vec<String> {
    cell.sections.iter().filter_map(|s| match &s.node {
        Section::OnSignal(on) if is_hole(on) => Some(on.signal_name.clone()),
        _ => None,
    }).collect()
}
