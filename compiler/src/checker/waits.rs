//! 3.0: durable waits — `await approve(msg)`, `await signal(name, key)`,
//! `await sleep(duration)` in a `[task]` handler.
//!
//!     on refund(id: String, amount: Int) [task] {
//!         if amount > LIMIT {
//!             let ok = await approve("refund {amount} for {id}")
//!             if !ok { return response(403, map("error", "refused")) }
//!         }
//!         paid.set(id, amount)
//!     }
//!
//! A wait that nobody can answer now ENDS the step: what the handler wrote
//! so far commits, its locals and its position are stored beside the data,
//! and the call returns a 202 `waiting` response. The rest of the handler
//! runs when the answer arrives — after a restart too.
//!
//! The position stored is "the n-th wait of this handler", which is only
//! meaningful where the statements after it are a plain continuation: a
//! wait is a whole statement (`let x = await …` or `await …`) of the
//! handler body or of an `if` branch there. This module holds that rule
//! and the walk both `check` and the interpreter use to find a wait.

use crate::ast::*;

/// One step down to a wait: the block, and the index of the statement there.
pub type WaitPath<'a> = Vec<(&'a [Spanned<Statement>], usize)>;

/// `let x = await f(…)` / `await f(…)`: (what, arguments, bound name, span of the call).
pub fn wait_of(stmt: &Statement) -> Option<(&str, &[Spanned<Expr>], Option<&str>, Span)> {
    let (value, bind) = match stmt {
        Statement::Let { name, value } => (value, Some(name.as_str())),
        Statement::ExprStmt { expr } => (expr, None),
        _ => return None,
    };
    match &value.node {
        Expr::FnCall { name, args } => name.strip_prefix("await_")
            .filter(|w| matches!(*w, "approve" | "signal" | "sleep"))
            .map(|w| (w, args.as_slice(), bind, value.span)),
        _ => None,
    }
}

/// Every wait of a handler body that stands where it can be resumed, in
/// source order, with the path to it.
pub fn waits<'a>(body: &'a [Spanned<Statement>]) -> Vec<WaitPath<'a>> {
    fn walk<'a>(block: &'a [Spanned<Statement>], path: &mut WaitPath<'a>, out: &mut Vec<WaitPath<'a>>) {
        for (i, st) in block.iter().enumerate() {
            path.push((block, i));
            if wait_of(&st.node).is_some() { out.push(path.clone()); }
            if let Statement::If { then_body, else_body, .. } = &st.node {
                walk(then_body, path, out);
                walk(else_body, path, out);
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(body, &mut Vec::new(), &mut out);
    out
}

/// The handler's shape, without positions: a stored wait is only resumed
/// into the handler it was taken from.
pub fn fingerprint(params: &[Param], body: &[Spanned<Statement>]) -> String {
    let text = format!("{:?}{:?}", params.iter().map(|p| &p.name).collect::<Vec<_>>(), body);
    // drop `Span { start: N, end: M }` — an edit elsewhere in the file moves them
    let mut clean = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(at) = rest.find("Span {") {
        clean.push_str(&rest[..at]);
        rest = match rest[at..].find('}') { Some(end) => &rest[at + end + 1..], None => "" };
    }
    clean.push_str(rest);
    // FNV-1a: the same on every build
    let mut h: u64 = 0xcbf29ce484222325;
    for b in clean.bytes() { h ^= b as u64; h = h.wrapping_mul(0x100000001b3); }
    format!("{:016x}", h)
}

pub struct WaitIssue {
    pub message: String,
    pub span: Span,
}

/// Check-time rules (kind `await`).
pub fn check_program(program: &Program) -> Vec<WaitIssue> {
    let mut issues = Vec::new();
    for cell in super::names::collect_cells(program) {
        // signals whose face states an `ensures`: checked on the value a call returns
        let ensured: Vec<&str> = super::contracts::contracted(cell).into_iter()
            .filter(|(_, sig, _)| !sig.ensures.is_empty()).map(|(_, sig, _)| sig.name.as_str()).collect();
        for section in &cell.sections {
            let (body, on): (&[Spanned<Statement>], Option<&OnSection>) = match &section.node {
                Section::OnSignal(on) => (&on.body, Some(on)),
                Section::Every(e) | Section::After(e) => (&e.body, None),
                _ => continue,
            };
            let mut calls: Vec<(String, Span)> = Vec::new();
            super::literals::for_each_call(body, &mut |name, _, span| {
                if matches!(name, "await_approve" | "await_signal" | "await_sleep") { calls.push((name.to_string(), span)); }
            });
            if calls.is_empty() { continue; }
            let shown = |name: &str| format!("await {}(…)", &name["await_".len()..]);
            let Some(on) = on else {
                for (name, span) in calls {
                    issues.push(WaitIssue { span, message: format!(
                        "`{}` in a timer block: a wait is stored with the handler it stops, and a timer block has no name to resume into — move the work into an `on …() [task]` handler and call it here", shown(&name)) });
                }
                continue;
            };
            if !on.properties.iter().any(|p| p == "task") {
                issues.push(WaitIssue { span: calls[0].1, message: format!(
                    "`{}` in handler '{}', which is not `[task]`: a wait ends a step (what was written so far commits, the rest runs when the answer arrives) and only a `[task]` handler has steps — write `on {}(…) [task] {{ … }}`",
                    shown(&calls[0].0), on.signal_name, on.signal_name) });
                continue;
            }
            if ensured.contains(&on.signal_name.as_str()) {
                issues.push(WaitIssue { span: calls[0].1, message: format!(
                    "handler '{}' waits (`{}`), and its face states an `ensures`: a call that stops at the wait returns the 202 `waiting` response, not the result the clause speaks of — drop the `ensures`, or check the value with `ensure` before the last `return`",
                    on.signal_name, shown(&calls[0].0)) });
            }
            let ok: Vec<Span> = waits(&on.body).iter()
                .filter_map(|p| p.last().and_then(|(b, i)| wait_of(&b[*i].node)).map(|w| w.3)).collect();
            for (name, span) in calls {
                if ok.contains(&span) { continue; }
                issues.push(WaitIssue { span, message: format!(
                    "`{}` here cannot be resumed: a wait is a whole statement — `let x = {}` or `{}` alone — of the handler body or of an `if` branch there (not in a loop, a lambda, a `match` arm or `try`, and not part of a larger expression). Split the handler at the wait, or loop over calls to a `[task]` handler that waits",
                    shown(&name), shown(&name), shown(&name)) });
            }
        }
    }
    issues
}

/// The state a `transition(…, "state")` call names (its last argument).
fn transition_target(name: &str, args: &[Spanned<Expr>]) -> Option<String> {
    if name != "transition" { return None; }
    match &args.last()?.node {
        Expr::Literal(Literal::String(s)) => Some(s.clone()),
        Expr::Ident(v) => Some(v.clone()),
        _ => None,
    }
}

fn targets_in(stmts: &[Spanned<Statement>]) -> Vec<String> {
    let mut out = Vec::new();
    super::literals::for_each_call(stmts, &mut |name, args, _| if let Some(t) = transition_target(name, args) { out.push(t); });
    out
}

/// `soma verify`: a wait is a place where a handler RESTS, between two
/// committed steps, for as long as the answer takes. Per wait: what is
/// already committed when it stops, the machine state an instance rests in,
/// and whether anything but the answer can move it on — a state only the
/// code after the wait leaves is one an instance never leaves if the answer
/// never comes.
pub fn verify_waits(program: &Program) -> Vec<super::verify::VerifyResult> {
    use super::verify::{VerifyCheck, VerifyResult};
    let mut results = Vec::new();
    for cell in super::names::collect_cells(program) {
        let machines: Vec<&StateMachineSection> = cell.sections.iter()
            .filter_map(|s| if let Section::State(sm) = &s.node { Some(sm) } else { None }).collect();
        let slots: Vec<&str> = cell.sections.iter().flat_map(|s| match &s.node {
            Section::Memory(m) => m.slots.iter().map(|sl| sl.node.name.as_str()).collect::<Vec<_>>(),
            _ => Vec::new(),
        }).collect();
        let mut checks = Vec::new();
        for (index, section) in cell.sections.iter().enumerate() {
            let Section::OnSignal(on) = &section.node else { continue };
            if !on.properties.iter().any(|p| p == "task") { continue; }
            for (n, path) in waits(&on.body).iter().enumerate() {
                let Some((what, args, _, _)) = path.last().and_then(|(b, i)| wait_of(&b[*i].node)) else { continue };
                let shown = format!("{}() wait {} (`await {}({})`)", on.signal_name, n + 1, what,
                    args.iter().map(|a| render_expr(&a.node)).collect::<Vec<_>>().join(", "));
                // statements that ran before the wait, and those that run after
                let mut before: Vec<Spanned<Statement>> = Vec::new();
                let mut after: Vec<Spanned<Statement>> = Vec::new();
                for (level, (block, i)) in path.iter().enumerate() {
                    before.extend(block[..*i].iter().cloned());
                    after.extend(block[*i + 1..].iter().cloned());
                    // the condition (or iterator) of the block the wait stands in ran too
                    if level + 1 < path.len() {
                        if let Statement::If { condition: e, .. } | Statement::While { condition: e, .. } | Statement::For { iter: e, .. } = &block[*i].node {
                            before.push(Spanned::new(Statement::ExprStmt { expr: e.clone() }, block[*i].span));
                        }
                    }
                }
                // …and the wait's own arguments
                for a in args.iter() { before.push(Spanned::new(Statement::ExprStmt { expr: (*a).clone() }, a.span)); }
                // a transition this report cannot place: in a branch that may not
                // have run, or toward a target held in a variable
                let mut unplaced = false;
                super::literals::for_each_call(&before, &mut |name, a, _| if name == "transition" {
                    let target = if a.len() >= 3 { a.get(2) } else { a.get(1) };
                    if !matches!(target.map(|t| &t.node), Some(Expr::Literal(Literal::String(t))) if !t.contains('{')) { unplaced = true; }
                });
                let flat = before.iter().filter(|st| matches!(&st.node, Statement::ExprStmt { expr } | Statement::Let { value: expr, .. } | Statement::Assign { value: expr, .. }
                    if matches!(&expr.node, Expr::FnCall { name, .. } if name == "transition"))).count();
                if targets_in(&before).len() != flat { unplaced = true; }
                let mut written: Vec<String> = Vec::new();
                super::literals::for_each_stmt_deep(&before, &mut |st| match st {
                    Statement::MethodCall { target, .. } if slots.contains(&target.as_str()) => written.push(target.clone()),
                    Statement::IndexSet { name, .. } | Statement::Assign { name, .. } if slots.contains(&name.as_str()) => written.push(name.clone()),
                    _ => {}
                });
                super::literals::for_each_expr(&before, &mut |e| if let Expr::MethodCall { target, method, .. } = e {
                    if let Expr::Ident(t) = &target.node {
                        if slots.contains(&t.as_str()) && matches!(method.as_str(), "set" | "push" | "append" | "delete" | "remove" | "put") { written.push(t.clone()); }
                    }
                });
                written.sort(); written.dedup();
                let rests = targets_in(&before).pop();
                let committed = match (written.is_empty(), &rests) {
                    (true, None) => "nothing is written before it".to_string(),
                    (false, None) => format!("committed before it: writes to {}", written.join(", ")),
                    (true, Some(s)) => format!("committed before it: the transition to `{}`", s),
                    (false, Some(s)) => format!("committed before it: writes to {} and the transition to `{}`", written.join(", "), s),
                };
                // this report reads the handler's own statements: what a handler it
                // calls before the wait writes or moves is not followed — said, not ticked
                let mut called: Vec<String> = Vec::new();
                super::literals::for_each_expr(&before, &mut |e| match e {
                    Expr::FnCall { name, .. } if name != "_coalesce" && !super::names::builtin_names().contains(name.as_str()) => called.push(format!("{}()", name)),
                    Expr::MethodCall { target, method, .. } => if let Expr::Ident(t) = &target.node {
                        if !slots.contains(&t.as_str()) && super::names::cellish(t) { called.push(format!("{}.{}()", t, method)); }
                    },
                    _ => {}
                });
                super::literals::for_each_stmt_deep(&before, &mut |st| match st {
                    Statement::Emit { signal_name, .. } => called.push(format!("emit {}", signal_name)),
                    Statement::MethodCall { target, method, .. } if !slots.contains(&target.as_str()) => called.push(format!("{}.{}()", target, method)),
                    _ => {}
                });
                called.sort(); called.dedup();
                if unplaced && called.is_empty() {
                    checks.push(VerifyCheck::Warning(format!("{}: a transition() before the wait stands in a branch, inside another expression, or names its target by a variable — the state the instance rests in is not followed by this report", shown)));
                    continue;
                }
                if !called.is_empty() {
                    checks.push(VerifyCheck::Warning(format!("{}: {} in its own statements — before the wait it also runs {}, and what those write or move is not followed by this report", shown, committed, called.join(", "))));
                    continue;
                }
                let Some(state) = rests else {
                    checks.push(VerifyCheck::Pass(format!("{}: {} — no machine state is entered for the wait", shown, committed)));
                    continue;
                };
                let Some(sm) = machines.iter().find(|sm| sm.initial == state || sm.transitions.iter().any(|t| t.node.to == state || t.node.from == state)) else {
                    checks.push(VerifyCheck::Pass(format!("{}: {}", shown, committed)));
                    continue;
                };
                // where the machine can go from the resting state
                let exits: Vec<&str> = sm.transitions.iter()
                    .filter(|t| t.node.from == state || (t.node.from == "*" && !t.node.except.contains(&state)))
                    .map(|t| t.node.to.as_str()).filter(|to| *to != state).collect();
                if exits.is_empty() {
                    checks.push(VerifyCheck::Warning(format!(
                        "{}: {} — but machine '{}' declares no transition out of `{}`: the code after the wait cannot move the instance on", shown, committed, sm.name, state)));
                    continue;
                }
                // who else takes one of those exits: another handler, a timer, an earlier part of this one
                let mut others: Vec<String> = Vec::new();
                for (j, other) in cell.sections.iter().enumerate() {
                    let (label, body): (String, &[Spanned<Statement>]) = match &other.node {
                        Section::OnSignal(o) if j != index => (format!("{}()", o.signal_name), &o.body),
                        Section::Every(e) => (format!("every {}ms", e.interval_ms), &e.body),
                        Section::After(e) => (format!("after {}ms", e.interval_ms), &e.body),
                        _ => continue,
                    };
                    // …from THIS state: `transition(id, "b", "c")` names another source
                    let mut leaves = false;
                    super::literals::for_each_call(body, &mut |name, args, _| if let Some(t) = transition_target(name, args) {
                        let source = if name == "transition" && args.len() >= 3 { match &args[1].node { Expr::Literal(Literal::String(s)) => Some(s.clone()), _ => None } } else { None };
                        if exits.contains(&t.as_str()) && source.map_or(true, |s| s == state) { leaves = true; }
                    });
                    if leaves { others.push(label); }
                }
                let by_answer: Vec<String> = { let mut v: Vec<String> = targets_in(&after).into_iter().filter(|t| exits.contains(&t.as_str())).collect(); v.dedup(); v };
                if others.is_empty() {
                    checks.push(VerifyCheck::Warning(format!(
                        "{}: {} — an instance rests in `{}` of machine '{}' until the answer comes, and nothing else leaves that state ({}): one that is never answered stays there forever. Give it another way out — a handler or an `every` sweep that takes `{}` -> `{}`",
                        shown, committed, state, sm.name,
                        if by_answer.is_empty() { "the code after the wait takes none of its exits either".to_string() } else { format!("only the code after the wait does, to `{}`", by_answer.join("`, `")) },
                        state, exits[0])));
                } else {
                    checks.push(VerifyCheck::Pass(format!(
                        "{}: {} — an instance rests in `{}` of machine '{}'; besides the answer, {} take{} a transition out of it", shown, committed, state, sm.name, others.join(", "), if others.len() == 1 { "s" } else { "" })));
                }
            }
        }
        if !checks.is_empty() {
            results.push(VerifyResult {
                machine_name: format!("{}/waits", cell.name),
                states: vec![], initial: String::new(), terminal_states: vec![], transitions: vec![],
                checks,
            });
        }
    }
    results
}
