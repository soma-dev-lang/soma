//! Handler termination checker.
//!
//! Proves that individual handler bodies terminate — not just the
//! state machine (which is what `soma verify` proves today), but the
//! actual code inside `on signal_name(...) { body }`.
//!
//! ## What this checks
//!
//! A handler is **structurally terminating** if:
//!   1. Every `for` loop iterates over a bounded source:
//!      - `range(0, N)` with literal N, OR
//!      - `[loop_bound(N)]` annotation, OR
//!      - a collection variable (`.keys()`, `.values()`, the result
//!        of a prior let-binding) — bounded by slot capacity
//!   2. There are no `while` loops (`[loop_bound(N)]` is a budget hint,
//!      not enforced at runtime, so it does not discharge the proof)
//!   3. Every recursive call has an argument of the form `param - N`
//!      (structural recursion on a decreasing Int argument) AND the
//!      handler has a conditional branch that can stop the descent
//!   4. The handler is not on a call cycle through other handlers
//!      (mutual recursion, in this cell or across cells) — no measure
//!      is attempted for those, they are flagged conservatively
//!
//! ## What this does NOT check
//!
//!   - Termination of `think()` / `http_get()` / `delegate()` — these
//!     are external calls; the checker assumes they terminate (timeout
//!     is a runtime concern, not a compile-time one)
//!   - General recursion with non-structural measures (e.g., Ackermann)
//!   - Termination in the presence of `break` inside `while` loops
//!     (the `while` is conservatively flagged even if it has a break)
//!
//! ## Output
//!
//! Per handler: `Terminates` or `MayNotTerminate { reasons }`.

use crate::ast::*;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub enum TerminationFinding {
    /// The handler's body is structurally terminating.
    Terminates {
        handler: String,
    },
    /// The handler may not terminate. Reasons list the specific
    /// constructs that prevent the termination proof.
    MayNotTerminate {
        handler: String,
        reasons: Vec<String>,
    },
}

/// Check termination for all handlers in a cell. `program` supplies the
/// call graph: handlers call each other by bare name, across cells too.
pub fn check_cell_termination(cell: &CellDef, program: &Program) -> Vec<TerminationFinding> {
    let mut findings = check_cell_termination_raw(cell, program);
    // `emit ping(…)` inside `on ping` reaches every OTHER cell's `on ping`
    // (the call graph is by name, so that edge read as a self-call and was
    // dropped): a listener that re-emits it is an endless ping-pong
    {
        let emits = |on: &OnSection, ev: &str| {
            let mut hit = false;
            crate::checker::literals::for_each_stmt_deep(&on.body, &mut |st| if matches!(st, Statement::Emit { signal_name, .. } if signal_name == ev) { hit = true; });
            hit
        };
        for f in findings.iter_mut() {
            let TerminationFinding::Terminates { handler } = f else { continue };
            let Some(on) = cell.sections.iter().find_map(|s| match &s.node { Section::OnSignal(o) if &o.signal_name == handler => Some(o), _ => None }) else { continue };
            if !emits(on, handler) { continue; }
            let echo = super::names::collect_cells(program).into_iter().filter(|c| c.name != cell.name && matches!(c.kind, CellKind::Cell | CellKind::Agent))
                .flat_map(|c| c.sections.iter()).any(|s| matches!(&s.node, Section::OnSignal(o) if &o.signal_name == handler && emits(o, handler)));
            if echo {
                *f = TerminationFinding::MayNotTerminate { handler: handler.clone(), reasons: vec![format!("emits `{}`, which another cell's `on {}` emits back — nothing bounds the exchange", handler, handler)] };
            }
        }
    }
    // a handler that CALLS one that may not terminate may not terminate
    // either (`go` calling an imported `spin` with `while true` was "✓
    // structurally terminates")
    let mut bad: std::collections::HashSet<String> = std::collections::HashSet::new();
    for c in super::names::collect_cells(program) {
        if !matches!(c.kind, CellKind::Cell | CellKind::Agent) { continue; }
        for f in check_cell_termination_raw(c, program) {
            if let TerminationFinding::MayNotTerminate { handler, .. } = f { bad.insert(handler); }
        }
    }
    if bad.is_empty() { return findings; }
    let graph = call_graph(program);
    for f in findings.iter_mut() {
        if let TerminationFinding::Terminates { handler } = f {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut todo: Vec<String> = graph.get(handler.as_str()).cloned().unwrap_or_default();
            // an `every` / `after` block's callees (`after 10ms { spin() }`)
            for s in &cell.sections {
                if let Section::Every(e) | Section::After(e) = &s.node {
                    let label = format!("{} {}ms", if matches!(s.node, Section::Every(_)) { "every" } else { "after" }, e.interval_ms);
                    if &label != handler { continue; }
                    for st in &e.body {
                        walk_stmt(&st.node, &mut |x| {
                            let callee = match x {
                                Expr::FnCall { name, .. } => Some(name),
                                Expr::MethodCall { target, method, .. } if matches!(&target.node, Expr::Ident(c) if super::names::cellish(c)) => Some(method),
                                _ => None,
                            };
                            if let Some(n) = callee { if graph.contains_key(n.as_str()) { todo.push(n.clone()); } }
                        });
                    }
                }
            }
            // its own event, handled by another cell, is a callee too
            let own_event = cell.sections.iter().any(|s| matches!(&s.node, Section::OnSignal(o) if &o.signal_name == handler && {
                let mut hit = false;
                crate::checker::literals::for_each_stmt_deep(&o.body, &mut |st| if matches!(st, Statement::Emit { signal_name, .. } if signal_name == handler) { hit = true; });
                hit
            }));
            let elsewhere = super::names::collect_cells(program).into_iter().any(|c| c.name != cell.name && c.sections.iter().any(|s| matches!(&s.node, Section::OnSignal(o) if &o.signal_name == handler)));
            if own_event && elsewhere && bad.contains(handler.as_str()) {
                *f = TerminationFinding::MayNotTerminate { handler: handler.clone(), reasons: vec![format!("emits `{}`, and another cell's `on {}` may not terminate", handler, handler)] };
                continue;
            }
            let mut culprit: Option<String> = None;
            while let Some(n) = todo.pop() {
                if !seen.insert(n.clone()) { continue; }
                if bad.contains(&n) { culprit = Some(n); break; }
                if let Some(next) = graph.get(n.as_str()) { todo.extend(next.iter().cloned()); }
            }
            if let Some(c) = culprit {
                *f = TerminationFinding::MayNotTerminate { handler: handler.clone(), reasons: vec![format!("`{}` calls `{}`, which may not terminate", handler, c)] };
            }
        }
    }
    findings
}

fn check_cell_termination_raw(cell: &CellDef, program: &Program) -> Vec<TerminationFinding> {
    let mut findings = Vec::new();
    let graph = call_graph(program);

    for section in &cell.sections {
        // an `every` / `after` block holds the process lock while it runs:
        // `while true { }` there hung every request — judge its loops too
        if let Section::Every(e) | Section::After(e) = &section.node {
            let label = format!("{} {}ms", if matches!(section.node, Section::Every(_)) { "every" } else { "after" }, e.interval_ms);
            let mut reasons = Vec::new();
            for stmt in &e.body {
                check_stmt_termination(&stmt.node, &label, &[], &mut reasons);
            }
            // a lambda applied to itself runs forever here too
            if let Some(f) = lambda_self_call(&e.body, &[], cell, &graph) {
                reasons.push(format!("`{}` calls the function value `{}` — a lambda can call itself (`g => g(g)`), so termination is not proven", label, f));
            }
            if reasons.is_empty() {
                findings.push(TerminationFinding::Terminates { handler: label });
            } else {
                reasons.dedup();
                findings.push(TerminationFinding::MayNotTerminate { handler: label, reasons });
            }
            continue;
        }
        if let Section::OnSignal(ref on) = section.node {
            let mut reasons = Vec::new();
            CURRENT_BODY.with(|b| *b.borrow_mut() = on.body.clone());
            for stmt in &on.body {
                check_stmt_termination(&stmt.node, &on.signal_name, &on.params, &mut reasons);
            }
            if let Some(f) = lambda_self_call(&on.body, &on.params, cell, &graph) {
                reasons.push(format!("handler `{}` calls the function value `{}` — a lambda can call itself (`g => g(g)`), so termination is not proven", on.signal_name, f));
            }
            let mut self_recursive = false;
            for stmt in &on.body {
                walk_stmt(&stmt.node, &mut |e| {
                    if matches!(e, Expr::FnCall { name, args } if name == &on.signal_name && crate::ast::accepted_arities(&on.params).contains(&args.len())) {
                        self_recursive = true;
                    }
                });
            }
            // a decreasing argument over a parameter the body re-binds is no
            // measure — and ONE parameter must decrease in EVERY self-call:
            // `walk(n - 1, m + 1)` beside `walk(n + 1, m - 1)` each decrease
            // something, and (5, 5) → (6, 4) → (5, 5) never ends
            let mut measures: Vec<&Param> = Vec::new();
            if self_recursive && reasons.is_empty() {
                let calls = self_calls(&on.body, &on.signal_name, &on.params);
                measures = on.params.iter().enumerate().filter(|(i, p)| !rebinds(&on.body, &p.name) && calls.iter().all(|args|
                    args.get(*i).map_or(false, |a| decrease_kind(&on.body, a, &p.name).is_some()))).map(|(_, p)| p).collect();
                if measures.is_empty() {
                    let rebound = on.params.iter().enumerate().any(|(i, p)| rebinds(&on.body, &p.name) && calls.iter().all(|args|
                        args.get(i).map_or(false, |a| decrease_kind(&on.body, a, &p.name).is_some())));
                    reasons.push(if rebound { format!(
                        "handler `{}`: recursive call without provable decreasing argument (the parameter is re-assigned or re-bound in the body)",
                        on.signal_name
                    ) } else { format!(
                        "handler `{}`: no single parameter decreases in every recursive call — one call lowering a parameter while another raises it can cycle; one parameter must go down in each call",
                        on.signal_name
                    ) });
                }
            }
            // `delegate("T", name, n)` with a computed handler name may call
            // anything, this handler included — not measured
            {
                let mut computed = false;
                crate::checker::literals::for_each_expr(&on.body, &mut |e| {
                    if let Expr::FnCall { name, args } = e {
                        if name == "delegate" && !matches!((args.get(0).map(|a| &a.node), args.get(1).map(|a| &a.node)),
                            (Some(Expr::Literal(Literal::String(_))), Some(Expr::Literal(Literal::String(_))))) { computed = true; }
                    }
                });
                if computed {
                    reasons.push(format!("handler `{}`: delegate() to a computed cell/handler name — a call the termination proof cannot follow (write the names as literals)", on.signal_name));
                }
            }
            // `(n + 1) |> up()` / `A.up2(n + 1)`: a self-call the
            // decreasing-argument rule does not see — not measured
            if hidden_self_call(&on.body, &on.signal_name) {
                reasons.push(format!(
                    "handler `{}`: recursive call through a pipe or a qualified `Cell.{}(…)` — write it as a plain `{}(n - 1)` call to have it measured",
                    on.signal_name, on.signal_name, on.signal_name
                ));
            }
            if self_recursive && reasons.is_empty() && !has_conditional(&on.body) {
                reasons.push(format!(
                    "handler `{}`: recursion has no base case (no conditional branch can stop the descent)",
                    on.signal_name
                ));
            }
            // `down(n - 1)` descends forever from a negative n unless the
            // base case is a LOWER BOUND on an Int parameter (`if n <= 0 {
            // return … }`, `require n >= 0`): `n == 0` is stepped over
            if self_recursive && reasons.is_empty() {
                let calls = self_calls(&on.body, &on.signal_name, &on.params);
                let bounded_measure = measures.iter().any(|p| {
                    let i = on.params.iter().position(|q| q.name == p.name).unwrap_or(0);
                    matches!(&p.ty.node, TypeExpr::Simple(t) if t == "Int") && has_lower_bound_exit(&on.body, &p.name, &on.signal_name, halving_of(&calls, i, &on.body, &p.name))
                });
                if !bounded_measure {
                    let p = measures[0];
                    let i = on.params.iter().position(|q| q.name == p.name).unwrap_or(0);
                    let halving = halving_of(&calls, i, &on.body, &p.name) != Decrease::Sub;
                    if !matches!(&p.ty.node, TypeExpr::Simple(t) if t == "Int") {
                        reasons.push(format!(
                            "handler `{}`: the decreasing argument `{}` is not an Int (a Float step can stall: x - 1 == x past 2^53)",
                            on.signal_name, p.name
                        ));
                    } else if halving {
                        reasons.push(format!(
                            "handler `{}`: halving `{}` stops at 0 (and -1 when it rounds down), where it no longer decreases — the base case must catch them: `if {} <= 0 {{ return … }}`",
                            on.signal_name, p.name, p.name
                        ));
                    } else {
                        reasons.push(format!(
                            "handler `{}`: `{} - k` decreases but no base case bounds it from below — write `if {} <= 0 {{ return … }}` (an `== 0` test is stepped over from a negative start)",
                            on.signal_name, p.name, p.name
                        ));
                    }
                }
            }
            if let Some(cycle) = find_cycle(&graph, &on.signal_name) {
                reasons.push(format!(
                    "handler `{}`: mutual recursion {} without a provable decreasing measure",
                    on.signal_name,
                    cycle.join(" → ")
                ));
            }
            if reasons.is_empty() {
                findings.push(TerminationFinding::Terminates {
                    handler: on.signal_name.clone(),
                });
            } else {
                reasons.dedup();
                findings.push(TerminationFinding::MayNotTerminate {
                    handler: on.signal_name.clone(),
                    reasons,
                });
            }
        }
    }

    findings
}

/// handler name → names of the handlers its body calls (self-edges are
/// excluded: direct recursion is judged by the decreasing-argument rule).
fn call_graph(program: &Program) -> HashMap<String, Vec<String>> {
    // every cell, interior ones included: the handlers of `interior { }`
    // contributed no edge (A.ping → B.pong → A.ping "✓ terminates")
    let all_cells = super::names::collect_cells(program);
    let mut handlers: Vec<&OnSection> = Vec::new();
    for cell in all_cells.iter().copied() {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.sections {
            if let Section::OnSignal(ref on) = section.node {
                handlers.push(on);
            }
        }
    }
    let names: HashSet<&str> = handlers.iter().map(|h| h.signal_name.as_str()).collect();
    let cells: HashSet<&str> = all_cells.iter().map(|c| c.name.as_str()).collect();
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for on in handlers {
        let mut calls = Vec::new();
        for stmt in &on.body {
            walk_stmt(&stmt.node, &mut |e| {
                let callee = match e {
                    Expr::FnCall { name, .. } => Some(name),
                    // `Other.o1(n)`: a call into another cell
                    Expr::MethodCall { target, method, .. } if matches!(&target.node, Expr::Ident(c) if cells.contains(c.as_str())) => Some(method),
                    _ => None,
                };
                if let Some(name) = callee {
                    if names.contains(name.as_str()) && name != &on.signal_name && !calls.contains(name) {
                        calls.push(name.clone());
                    }
                }
            });
        }
        // `Other.o1(n)` as a statement, and `emit ev(…)` (every `on ev` runs;
        // a handler's own event does not re-enter it)
        // inside `try { }` / block lambdas / match arms too (ping and pong
        // emitting each other in `try` verified "✓ terminates")
        fn stmt_edges(stmts: &[Spanned<Statement>], cells: &HashSet<&str>, out: &mut Vec<String>) {
            crate::checker::literals::for_each_stmt_deep(stmts, &mut |st| match st {
                Statement::MethodCall { target, method, .. } if cells.contains(target.as_str()) => out.push(method.clone()),
                Statement::Emit { signal_name, .. } => out.push(signal_name.clone()),
                _ => {}
            });
        }
        let mut extra = Vec::new();
        stmt_edges(&on.body, &cells, &mut extra);
        // vote(target, …) runs its target; horde(target, …) runs it and its
        // callbacks (a horde started again from on_done ran 20 821 times,
        // "✓ terminates") — a self-reference here is recursion too
        let mut via: Vec<String> = Vec::new();
        crate::checker::literals::for_each_expr(&on.body, &mut |e| if let Expr::FnCall { name, args } = e {
            if (name == "vote" && args.len() == 3) || name == "horde" {
                match args.first().map(|a| &a.node) {
                    Some(Expr::FieldAccess { field, .. }) => via.push(field.clone()),
                    Some(Expr::Ident(h)) => via.push(h.clone()),
                    Some(Expr::Literal(Literal::String(t))) => via.push(t.rsplit('.').next().unwrap_or(t).to_string()),
                    _ => {}
                }
                if name == "horde" {
                    if let Some(Expr::FnCall { name: m, args: kv }) = args.get(2).map(|a| &a.node) {
                        if m == "map" { for c in kv.chunks(2) {
                            if let (Expr::Literal(Literal::String(k)), Some(Expr::Literal(Literal::String(h)))) = (&c[0].node, c.get(1).map(|v| &v.node)) {
                                if matches!(k.as_str(), "on_result" | "apply" | "on_done" | "on_error") { via.push(h.clone()); }
                            }
                        } }
                    }
                }
            }
        });
        for name in via {
            if names.contains(name.as_str()) && !calls.contains(&name) { calls.push(name); }
        }
        for name in extra {
            if names.contains(name.as_str()) && name != on.signal_name && !calls.contains(&name) { calls.push(name); }
        }
        graph.entry(on.signal_name.clone()).or_default().extend(calls);
    }
    // think() hands control to the model, which may call any tool it can
    // dispatch there: a tool that calls back the handler is a recursion the
    // MODEL drives (5 250 LLM calls from one `soma run`, "✓ terminates")
    crate::checker::effects::set_program(program);
    for cell in all_cells.iter().copied() {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) { continue; }
        for fx in crate::checker::effects::check_cell(cell) {
            let edges = graph.entry(fx.handler.clone()).or_default();
            for t in fx.think_tools {
                if names.contains(t.as_str()) && !edges.contains(&t) { edges.push(t); }
            }
        }
    }
    graph
}

/// A path `start → … → start` of length ≥ 2, if one exists.
fn find_cycle(graph: &HashMap<String, Vec<String>>, start: &str) -> Option<Vec<String>> {
    let mut stack: Vec<Vec<String>> = vec![vec![start.to_string()]];
    let mut seen: HashSet<String> = HashSet::new();
    while let Some(path) = stack.pop() {
        let last = path.last().unwrap();
        for next in graph.get(last).map(|v| v.as_slice()).unwrap_or(&[]) {
            if next == start {
                let mut cycle = path.clone();
                cycle.push(start.to_string());
                return Some(cycle);
            }
            if seen.insert(next.clone()) {
                let mut p = path.clone();
                p.push(next.clone());
                stack.push(p);
            }
        }
    }
    None
}

/// Visit every expression reachable from a statement, nested statements
/// and sub-expressions included.
pub(super) fn walk_stmt(stmt: &Statement, f: &mut dyn FnMut(&Expr)) {
    match stmt {
        Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => {
            walk_expr(&value.node, f)
        }
        Statement::If { condition, then_body, else_body } => {
            walk_expr(&condition.node, f);
            for s in then_body.iter().chain(else_body) {
                walk_stmt(&s.node, f);
            }
        }
        Statement::For { iter, body, .. } => {
            walk_expr(&iter.node, f);
            for s in body {
                walk_stmt(&s.node, f);
            }
        }
        Statement::While { condition, body, .. } => {
            walk_expr(&condition.node, f);
            for s in body {
                walk_stmt(&s.node, f);
            }
        }
        Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => {
            for a in args {
                walk_expr(&a.node, f);
            }
        }
        Statement::IndexSet { index, value, .. } => {
            walk_expr(&index.node, f);
            walk_expr(&value.node, f);
        }
        Statement::Ensure { condition } => walk_expr(&condition.node, f),
        Statement::ExprStmt { expr } => walk_expr(&expr.node, f),
        Statement::Require { .. } | Statement::Break | Statement::Continue => {}
    }
}

pub(super) fn walk_expr(expr: &Expr, f: &mut dyn FnMut(&Expr)) {
    f(expr);
    match expr {
        Expr::Literal(_) | Expr::Ident(_) => {}
        Expr::FieldAccess { target, .. } => walk_expr(&target.node, f),
        Expr::Index { target, index } => {
            walk_expr(&target.node, f);
            walk_expr(&index.node, f);
        }
        Expr::MethodCall { target, args, .. } => {
            walk_expr(&target.node, f);
            for a in args {
                walk_expr(&a.node, f);
            }
        }
        Expr::FnCall { args, .. } => {
            for a in args {
                walk_expr(&a.node, f);
            }
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            walk_expr(&left.node, f);
            walk_expr(&right.node, f);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => walk_expr(&i.node, f),
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                walk_expr(&v.node, f);
            }
        }
        Expr::ListLiteral(items) => {
            for i in items {
                walk_expr(&i.node, f);
            }
        }
        Expr::Lambda { body, .. } => walk_expr(&body.node, f),
        Expr::LambdaBlock { stmts, result, .. } => {
            for s in stmts {
                walk_stmt(&s.node, f);
            }
            walk_expr(&result.node, f);
        }
        Expr::Match { subject, arms } => {
            walk_expr(&subject.node, f);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    walk_expr(&g.node, f);
                }
                for s in &arm.body {
                    walk_stmt(&s.node, f);
                }
                walk_expr(&arm.result.node, f);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            walk_expr(&condition.node, f);
            for s in then_body.iter().chain(else_body) {
                walk_stmt(&s.node, f);
            }
            walk_expr(&then_result.node, f);
            walk_expr(&else_result.node, f);
        }
    }
}

/// Does the body contain any conditional branch (if / match / if-expr)?
/// A self-recursive handler without one has no base case.
fn has_conditional(body: &[Spanned<Statement>]) -> bool {
    let mut found = false;
    for s in body {
        if matches!(s.node, Statement::If { .. }) {
            return true;
        }
        walk_stmt(&s.node, &mut |e| {
            if matches!(e, Expr::Match { .. } | Expr::IfExpr { .. }) {
                found = true;
            }
        });
        // nested statements (loops, lambdas) may hold the `if`
        if !found {
            found = nested_if(&s.node);
        }
    }
    found
}

fn nested_if(stmt: &Statement) -> bool {
    match stmt {
        Statement::If { .. } => true,
        Statement::For { body, .. } | Statement::While { body, .. } => {
            body.iter().any(|s| nested_if(&s.node))
        }
        _ => false,
    }
}

fn check_stmt_termination(
    stmt: &Statement,
    handler_name: &str,
    params: &[Param],
    reasons: &mut Vec<String>,
) {
    match stmt {
        Statement::For { iter, body, bound, .. } => {
            // A `for` loop is bounded if:
            // 1. It has an explicit [loop_bound(N)]
            // 2. Its iterator is range(lo, hi) with literals
            // 3. Its iterator is a variable (collection — bounded by capacity)
            let is_bounded = bound.is_some()
                || is_literal_range(&iter.node)
                || is_collection_iter(&iter.node)
                // `for l in [[0,1,2],[3,4,5]]`: a literal list has a length
                || matches!(iter.node, Expr::ListLiteral(_));
            if let (Some(b), Expr::ListLiteral(items)) = (bound, &iter.node) {
                if items.len() as u64 > *b {
                    reasons.push(format!("handler `{}`: `for [loop_bound({})]` over a literal list of {} items — the bound is too small (it raises loop_bound at run time)", handler_name, b, items.len()));
                }
            }

            if !is_bounded {
                reasons.push(format!(
                    "handler `{}`: for-loop with unbounded iterator (write `for [loop_bound(N)] x in xs` or use range(0, N) with literal N)",
                    handler_name
                ));
            }
            check_expr_termination(&iter.node, handler_name, params, reasons);

            // Recurse into body
            for s in body {
                check_stmt_termination(&s.node, handler_name, params, reasons);
            }
        }

        Statement::While { condition, body, bound } => {
            check_expr_termination(&condition.node, handler_name, params, reasons);
            // `while [loop_bound(N)] cond { … }` terminates: iteration N+1
            // raises (checked at run time, interpreter and native)
            if bound.is_some() {
                for s in body { check_stmt_termination(&s.node, handler_name, params, reasons); }
                return;
            }
            reasons.push(format!(
                "handler `{}`: while-loop without provable termination bound (write `while [loop_bound(N)] cond {{ … }}`, or a for-loop over a bounded range)",
                handler_name
            ));

            for s in body {
                check_stmt_termination(&s.node, handler_name, params, reasons);
            }
        }

        Statement::If { condition, then_body, else_body } => {
            check_expr_termination(&condition.node, handler_name, params, reasons);
            for s in then_body {
                check_stmt_termination(&s.node, handler_name, params, reasons);
            }
            for s in else_body {
                check_stmt_termination(&s.node, handler_name, params, reasons);
            }
        }

        Statement::ExprStmt { expr } => {
            check_expr_termination(&expr.node, handler_name, params, reasons);
        }

        Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => {
            check_expr_termination(&value.node, handler_name, params, reasons);
        }

        Statement::IndexSet { index, value, .. } => {
            check_expr_termination(&index.node, handler_name, params, reasons);
            check_expr_termination(&value.node, handler_name, params, reasons);
        }

        Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => {
            for a in args {
                check_expr_termination(&a.node, handler_name, params, reasons);
            }
        }
        Statement::Ensure { condition } => {
            check_expr_termination(&condition.node, handler_name, params, reasons);
        }

        // These are always terminating
        Statement::Require { .. } | Statement::Break | Statement::Continue => {}
    }
}

thread_local! {
    /// The body of the handler being judged, for the local-binding rule.
    static CURRENT_BODY: std::cell::RefCell<Vec<Spanned<Statement>>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn check_expr_termination(
    expr: &Expr,
    handler_name: &str,
    params: &[Param],
    reasons: &mut Vec<String>,
) {
    match expr {
        Expr::FnCall { name, args } => {
            // Check for recursive call to the same handler (an arity the
            // handler does not take goes to the builtin: `list(1, 2)`
            // inside `on list()` is not recursion)
            if name == handler_name && crate::ast::accepted_arities(params).contains(&args.len()) {
                // Structural recursion: the call must have at least one
                // argument that is provably smaller than the corresponding
                // parameter. Simplest check: arg is `param - 1` or
                // `param - literal`.
                let is_structural = args.iter().enumerate().any(|(i, arg)| {
                    if i < params.len() {
                        is_decreasing_arg(&arg.node, &params[i].name)
                            // `let m = n - 1  … c(m)` decreases too
                            || CURRENT_BODY.with(|b| decreasing_through_local(&b.borrow(), &arg.node, &params[i].name))
                    } else {
                        false
                    }
                });

                if !is_structural {
                    reasons.push(format!(
                        "handler `{}`: recursive call without provable decreasing argument",
                        handler_name
                    ));
                }
            }

            // Recurse into arguments
            for a in args {
                check_expr_termination(&a.node, handler_name, params, reasons);
            }
        }

        Expr::Match { subject, arms } => {
            check_expr_termination(&subject.node, handler_name, params, reasons);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    check_expr_termination(&g.node, handler_name, params, reasons);
                }
                for s in &arm.body {
                    check_stmt_termination(&s.node, handler_name, params, reasons);
                }
                check_expr_termination(&arm.result.node, handler_name, params, reasons);
            }
        }

        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            check_expr_termination(&condition.node, handler_name, params, reasons);
            for s in then_body {
                check_stmt_termination(&s.node, handler_name, params, reasons);
            }
            check_expr_termination(&then_result.node, handler_name, params, reasons);
            for s in else_body {
                check_stmt_termination(&s.node, handler_name, params, reasons);
            }
            check_expr_termination(&else_result.node, handler_name, params, reasons);
        }

        Expr::Pipe { left, right } => {
            check_expr_termination(&left.node, handler_name, params, reasons);
            check_expr_termination(&right.node, handler_name, params, reasons);
        }

        Expr::LambdaBlock { stmts, result, .. } => {
            for s in stmts {
                check_stmt_termination(&s.node, handler_name, params, reasons);
            }
            check_expr_termination(&result.node, handler_name, params, reasons);
        }

        // Combinators: a recursive call can hide in any operand
        // (`return 1 + f(n)`), so descend everywhere.
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
            check_expr_termination(&left.node, handler_name, params, reasons);
            check_expr_termination(&right.node, handler_name, params, reasons);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => {
            check_expr_termination(&i.node, handler_name, params, reasons);
        }
        Expr::FieldAccess { target, .. } => {
            check_expr_termination(&target.node, handler_name, params, reasons);
        }
        Expr::Index { target, index } => {
            check_expr_termination(&target.node, handler_name, params, reasons);
            check_expr_termination(&index.node, handler_name, params, reasons);
        }
        Expr::MethodCall { target, args, .. } => {
            check_expr_termination(&target.node, handler_name, params, reasons);
            for a in args {
                check_expr_termination(&a.node, handler_name, params, reasons);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                check_expr_termination(&v.node, handler_name, params, reasons);
            }
        }
        Expr::ListLiteral(items) => {
            for i in items {
                check_expr_termination(&i.node, handler_name, params, reasons);
            }
        }
        Expr::Lambda { body, .. } => {
            check_expr_termination(&body.node, handler_name, params, reasons);
        }

        Expr::Literal(_) | Expr::Ident(_) => {}
    }
}

/// Check if a `for` iterator is `range(lo, hi)` with literal bounds.
fn is_literal_range(expr: &Expr) -> bool {
    if let Expr::FnCall { name, args } = expr {
        if name == "range" {
            return args.iter().all(|a| matches!(a.node, Expr::Literal(Literal::Int(_))));
        }
    }
    false
}

/// Check if a `for` iterator is a collection-like expression
/// (variable, field access, method call returning a collection).
/// These are bounded by the collection's size, which is finite.
fn is_collection_iter(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Ident(_)
        | Expr::FieldAccess { .. }
        | Expr::MethodCall { .. }
        | Expr::FnCall { .. }  // e.g., keys(), values(), list()
        // a `for` walks a SNAPSHOT of a finite value, whatever expression
        // produced it (`for x in m["items"]` was "unbounded iterator")
        | Expr::Index { .. }
        | Expr::Pipe { .. }
        | Expr::ListLiteral(_)
        | Expr::IfExpr { .. }
        | Expr::Match { .. }
        | Expr::BinaryOp { .. }
        | Expr::Try(_)
        | Expr::TryPropagate(_)
    )
}

/// `n - k`, or halving: truncating toward zero (`idiv`, `/`: 0 is the only
/// fixed point) or rounding down (`floor_div`, `div`, `shr`: -1 too).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Decrease { Sub, HalveTrunc, HalveFloor }

/// How an argument decreases a parameter, directly or through a local
/// (`let m = n - 1  … c(m)`).
fn decrease_kind(body: &[Spanned<Statement>], arg: &Expr, param: &str) -> Option<Decrease> {
    let kind = |e: &Expr| if !is_decreasing_arg(e, param) { None } else { Some(match e {
        Expr::BinaryOp { op: BinOp::Sub, .. } => Decrease::Sub,
        Expr::FnCall { name, .. } if name == "idiv" => Decrease::HalveTrunc,
        Expr::BinaryOp { op: BinOp::Div, .. } => Decrease::HalveTrunc,
        _ => Decrease::HalveFloor,
    }) };
    if let Some(k) = kind(arg) { return Some(k); }
    if !decreasing_through_local(body, arg, param) { return None; }
    let Expr::Ident(local) = arg else { return None };
    let mut found = None;
    crate::checker::literals::for_each_stmt_deep(body, &mut |st| if let Statement::Let { name, value } = st {
        if name == local { found = kind(&value.node); }
    });
    found
}

/// The weakest decrease of parameter `i` over the self-calls: rounding-down
/// halving over truncating halving over subtraction.
fn halving_of(calls: &[Vec<&Expr>], i: usize, body: &[Spanned<Statement>], param: &str) -> Decrease {
    let kinds: Vec<Decrease> = calls.iter().filter_map(|args| args.get(i).and_then(|a| decrease_kind(body, a, param))).collect();
    if kinds.contains(&Decrease::HalveFloor) { Decrease::HalveFloor }
    else if kinds.contains(&Decrease::HalveTrunc) { Decrease::HalveTrunc }
    else { Decrease::Sub }
}

/// The argument lists of every direct self-call.
fn self_calls<'a>(body: &'a [Spanned<Statement>], handler: &str, params: &[Param]) -> Vec<Vec<&'a Expr>> {
    let mut out = Vec::new();
    crate::checker::literals::for_each_expr(body, &mut |e| if let Expr::FnCall { name, args } = e {
        if name == handler && crate::ast::accepted_arities(params).contains(&args.len()) {
            out.push(args.iter().map(|a| &a.node).collect());
        }
    });
    out
}

/// Check if an argument is provably smaller than a parameter.
/// Recognizes patterns like: `param - 1`, `param - literal`.
fn is_decreasing_arg(arg: &Expr, param_name: &str) -> bool {
    match arg {
        Expr::BinaryOp { left, op, right } if matches!(op, BinOp::Sub) =>
            matches!((&left.node, &right.node), (Expr::Ident(n), Expr::Literal(Literal::Int(k))) if n == param_name && *k > 0),
        // halving terminates too: `c(idiv(n, 2))`, `floor_div`, `n / 2`
        Expr::FnCall { name, args } if matches!(name.as_str(), "idiv" | "floor_div" | "div" | "shr") && args.len() == 2 =>
            matches!((&args[0].node, &args[1].node), (Expr::Ident(n), Expr::Literal(Literal::Int(k)))
                if n == param_name && ((name == "shr" && *k >= 1) || *k > 1)),
        Expr::BinaryOp { left, op, right } if matches!(op, BinOp::Div) =>
            matches!((&left.node, &right.node), (Expr::Ident(n), Expr::Literal(Literal::Int(k))) if n == param_name && *k > 1),
        _ => false,
    }
}

/// `let m = n - 1  … c(m)`: the argument is a local bound, once and before
/// the call, to an expression that decreases the parameter.
fn decreasing_through_local(body: &[Spanned<Statement>], arg: &Expr, param_name: &str) -> bool {
    let Expr::Ident(local) = arg else { return false };
    if local == param_name { return false; }
    let mut bound: Vec<&Expr> = Vec::new();
    crate::checker::literals::for_each_stmt_deep(body, &mut |st| if let Statement::Let { name, value } = st {
        if name == local { bound.push(&value.node); }
    });
    // …and it is never re-assigned to something else
    let mut assigned = false;
    crate::checker::literals::for_each_stmt_deep(body, &mut |st| if let Statement::Assign { name, .. } = st {
        if name == local { assigned = true; }
    });
    // …nor re-bound by a loop, a lambda or a match arm: `match n + 1 { m ->
    // rec(m) }` / `map(m => rec(m))` pass THAT binding, not the `let`
    let mut shadowed = false;
    crate::checker::literals::for_each_stmt_deep(body, &mut |st| if let Statement::For { var, .. } = st {
        if var == local { shadowed = true; }
    });
    crate::checker::literals::for_each_expr(body, &mut |e| match e {
        Expr::Lambda { param, .. } | Expr::LambdaBlock { param, .. } if param == local => shadowed = true,
        Expr::Match { arms, .. } => for arm in arms {
            let mut b = HashSet::new();
            crate::checker::invariants::pattern_binders(&arm.pattern, &mut b);
            if b.contains(local) { shadowed = true; }
        },
        _ => {}
    });
    !assigned && !shadowed && bound.len() == 1 && is_decreasing_arg(bound[0], param_name)
}

/// Does the body stop the descent with a lower bound on `param` before any
/// statement can recurse: `if param <= c { … return … }` (or `< c`, or the
/// flipped spelling), or `require param >= c` — with a literal `c`?
/// Halving (`idiv(n, 2)`) stops decreasing at 0, and at -1 when it rounds
/// down: its base case must catch both (`n <= c` with c >= 0, `n < c` with
/// c >= 1, `require n >= 1`) — `if n < 0` let `log2(0)` recurse forever.
fn has_lower_bound_exit(body: &[Spanned<Statement>], param: &str, handler: &str, decrease: Decrease) -> bool {
    let halving = decrease != Decrease::Sub;
    // `param <= c` (strict: `param < c`) with a literal c: does it stop there?
    let stops = |c: i64, strict: bool| !halving || if strict { c >= 1 } else { c >= 0 };
    // truncating halving only ever lands on 0: `if n == 0` catches it
    let eq_zero_stops = decrease == Decrease::HalveTrunc;
    let bounded = |cond: &Expr| -> bool {
        fn walk(cond: &Expr, param: &str, stops: &dyn Fn(i64, bool) -> bool, eq_zero: bool) -> bool {
            match cond {
                Expr::CmpOp { left, op, right } => {
                    let lit = |e: &Expr| if let Expr::Literal(Literal::Int(k)) = e { Some(*k) } else { None };
                    match (&left.node, &right.node) {
                        (Expr::Ident(n), r) if n == param && lit(r).is_some() => match op {
                            CmpOp::Le => stops(lit(r).unwrap(), false),
                            CmpOp::Lt => stops(lit(r).unwrap(), true),
                            CmpOp::Eq => eq_zero && lit(r) == Some(0),
                            _ => false,
                        },
                        (l, Expr::Ident(n)) if n == param && lit(l).is_some() => match op {
                            CmpOp::Ge => stops(lit(l).unwrap(), false),
                            CmpOp::Gt => stops(lit(l).unwrap(), true),
                            CmpOp::Eq => eq_zero && lit(l) == Some(0),
                            _ => false,
                        },
                        _ => false,
                    }
                }
                Expr::BinaryOp { left, op: BinOp::Or, right } => walk(&left.node, param, stops, eq_zero) || walk(&right.node, param, stops, eq_zero),
                _ => false,
            }
        }
        walk(cond, param, &stops, eq_zero_stops)
    };
    for st in body {
        // a self-call BEFORE any base case runs unconditionally: `let rest =
        // count(n - 1)  if n <= 0 { return 0 }` never reaches the base case
        let is_base_if = matches!(&st.node, Statement::If { .. });
        if !is_base_if && calls_self(std::slice::from_ref(st), handler) { return false; }
        // …and so does one in the base case's own condition (`if f(n - 1) >
        // 100 || n <= 0 { return 0 }` recursed before testing n)
        if let Statement::If { condition, .. } = &st.node {
            let cond = Spanned::new(Statement::ExprStmt { expr: condition.clone() }, st.span);
            if calls_self(std::slice::from_ref(&cond), handler) { return false; }
        }
        match &st.node {
            Statement::If { condition, then_body, else_body } => {
                let exits = matches!(then_body.last().map(|s| &s.node), Some(Statement::Return { .. }))
                    || matches!(then_body.last().map(|s| &s.node), Some(Statement::ExprStmt { expr }) if matches!(&expr.node, Expr::FnCall { name, .. } if name == "fail"));
                if exits && bounded(&condition.node) && !calls_self(then_body, handler) { return true; }
                // a self-call in a branch of an `if` that is no base case runs
                // before any later one (`if n < 100 { depth(n - 1) }` then
                // `if n <= 0 { return 0 }` descended forever): the branch
                // must be taken only above the bound, or stop on its own
                // then-branch: taken when the condition holds, so its negation
                // must be a stop (`if n > 0 { … f(n - 1) … }`)
                let negated_stops = |c: &Expr| -> bool {
                    fn neg(c: &Expr) -> Vec<Expr> {
                        match c {
                            Expr::CmpOp { left, op, right } => vec![Expr::CmpOp { left: left.clone(), op: match op {
                                CmpOp::Lt => CmpOp::Ge, CmpOp::Le => CmpOp::Gt, CmpOp::Gt => CmpOp::Le, CmpOp::Ge => CmpOp::Lt,
                                CmpOp::Eq => CmpOp::Ne, CmpOp::Ne => CmpOp::Eq }, right: right.clone() }],
                            // `a && b` holds: each conjunct holds, one negation stopping is enough
                            Expr::BinaryOp { left, op: BinOp::And, right } => { let mut v = neg(&left.node); v.extend(neg(&right.node)); v }
                            _ => vec![],
                        }
                    }
                    neg(c).iter().any(|n| bounded(n))
                };
                if calls_self(then_body, handler) && !negated_stops(&condition.node) && !has_lower_bound_exit(then_body, param, handler, decrease) { return false; }
                // else-branch: taken when the condition fails, so the condition must be a stop
                if calls_self(else_body, handler) && !bounded(&condition.node) && !has_lower_bound_exit(else_body, param, handler, decrease) { return false; }
            }
            Statement::Require { constraint, .. } => {
                // `require n >= 0` parses as the expression `n >= 0` == true:
                // unwrapped, or this rule never matched
                let cmp = match &constraint.node {
                    Constraint::Comparison { left, op: CmpOp::Eq, right } if matches!(right.node, Expr::Literal(Literal::Bool(true))) =>
                        match &left.node { Expr::CmpOp { left, op, right } => Some((left.as_ref(), *op, right.as_ref())), _ => None },
                    Constraint::Comparison { left, op, right } => Some((left, *op, right)),
                    _ => None,
                };
                if let Some((left, op, right)) = cmp {
                    let op = &op;
                    let lit = |e: &Expr| matches!(e, Expr::Literal(Literal::Int(_)));
                    // `require n >= c`: past it the call fails, which ends the
                    // descent — for halving only if c excludes 0 (idiv(0, 2) is 0)
                    let at_least = |c: i64, strict: bool| !halving || if strict { c >= 0 } else { c >= 1 };
                    let ok = match (&left.node, &right.node) {
                        (Expr::Ident(n), r) if n == param && lit(r) => match (op, &r) {
                            (CmpOp::Ge, Expr::Literal(Literal::Int(c))) => at_least(*c, false),
                            (CmpOp::Gt, Expr::Literal(Literal::Int(c))) => at_least(*c, true),
                            _ => false,
                        },
                        (l, Expr::Ident(n)) if n == param && lit(l) => match (op, &l) {
                            (CmpOp::Le, Expr::Literal(Literal::Int(c))) => at_least(*c, false),
                            (CmpOp::Lt, Expr::Literal(Literal::Int(c))) => at_least(*c, true),
                            _ => false,
                        },
                        _ => false,
                    };
                    if ok { return true; }
                }
            }
            _ => {}
        }
    }
    false
}

/// Is `name` assigned, re-`let`, or re-bound (a loop variable, a lambda
/// parameter, a match binding) anywhere in the body? Then `rec(name - 1)`
/// does not measure the parameter: `n = n + 5  rec(n - 1)` never ends.
fn rebinds(stmts: &[Spanned<Statement>], name: &str) -> bool {
    fn st_walk(stmts: &[Spanned<Statement>], name: &str) -> bool {
        stmts.iter().any(|st| match &st.node {
            Statement::Let { name: n, .. } | Statement::Assign { name: n, .. } => n == name,
            Statement::For { var, body, .. } => var == name || st_walk(body, name),
            Statement::While { body, .. } => st_walk(body, name),
            Statement::If { then_body, else_body, .. } => st_walk(then_body, name) || st_walk(else_body, name),
            _ => false,
        })
    }
    if st_walk(stmts, name) { return true; }
    let mut hit = false;
    crate::checker::literals::for_each_expr(stmts, &mut |e| match e {
        Expr::Lambda { param, .. } | Expr::LambdaBlock { param, .. } if param == name => hit = true,
        Expr::LambdaBlock { stmts, .. } => { if st_walk(stmts, name) { hit = true; } }
        Expr::IfExpr { then_body, else_body, .. } => { if st_walk(then_body, name) || st_walk(else_body, name) { hit = true; } }
        Expr::Match { arms, .. } => {
            for arm in arms {
                let mut b = HashSet::new();
                crate::checker::invariants::pattern_binders(&arm.pattern, &mut b);
                if b.contains(name) || st_walk(&arm.body, name) { hit = true; }
            }
        }
        _ => {}
    });
    hit
}

/// A self-call in any form: `h(…)`, `x |> h(…)`, `Cell.h(…)`.
fn calls_self(stmts: &[Spanned<Statement>], handler: &str) -> bool {
    let mut hit = false;
    crate::checker::literals::for_each_expr(stmts, &mut |e| match e {
        Expr::FnCall { name, .. } if name == handler => hit = true,
        Expr::MethodCall { target, method, .. } if method == handler
            && matches!(&target.node, Expr::Ident(c) if c.chars().next().map_or(false, |ch| ch.is_uppercase())) => hit = true,
        _ => {}
    });
    hit
}

fn hidden_self_call(stmts: &[Spanned<Statement>], handler: &str) -> bool {
    let mut hit = false;
    crate::checker::literals::for_each_expr(stmts, &mut |e| match e {
        Expr::Pipe { right, .. } if matches!(&right.node, Expr::FnCall { name, .. } if name == handler) => hit = true,
        Expr::MethodCall { target, method, .. } if method == handler
            && matches!(&target.node, Expr::Ident(c) if c.chars().next().map_or(false, |ch| ch.is_uppercase())) => hit = true,
        _ => {}
    });
    hit
}

/// A lambda of `body` that calls — or hands to map / filter / … — a function
/// value that could be itself, or a call of a function-valued parameter:
/// the name called, when there is one.
fn lambda_self_call(body: &[Spanned<Statement>], params: &[Param], cell: &CellDef, graph: &std::collections::HashMap<String, Vec<String>>) -> Option<String> {
            // recursion with no handler call: a LAMBDA whose body calls — or
            // hands to map / filter / find / … — a function value that could
            // be itself (`g => g(g)`, `g => [g] |> map(g)`, an alias
            // `let k = [g][0]  k(k)`), or a handler calling its own
            // function-valued parameter (`_ap(f) { f(f) }`). A lambda bound by
            // a handler-level `let` cannot refer to itself, so calling one
            // (`let dbl = x => inc(x)`) is fine.
            {
                let handler_names: std::collections::HashSet<&str> = graph.keys().map(|s| s.as_str()).collect();
                let builtins = crate::checker::names::builtin_names();
                fn top_lets(stmts: &[Spanned<Statement>], out: &mut std::collections::HashSet<String>) {
                    for st in stmts {
                        match &st.node {
                            Statement::Let { name, .. } => { out.insert(name.clone()); }
                            Statement::If { then_body, else_body, .. } => { top_lets(then_body, out); top_lets(else_body, out); }
                            Statement::For { body, .. } | Statement::While { body, .. } => top_lets(body, out),
                            _ => {}
                        }
                    }
                }
                let mut safe: std::collections::HashSet<String> = std::collections::HashSet::new();
                top_lets(body, &mut safe);
                // a memory slot holds data, never a function value (`history |>
                // filter(…)` inside a nested lambda was "calls the function value")
                for sec in &cell.sections {
                    if let Section::Memory(m) = &sec.node { for sl in &m.slots { safe.insert(sl.node.name.clone()); } }
                }
                // (a parameter shadowing a slot may be a function value —
                // unless its declared type is data: `month: String`)
                for p in params {
                    let data = matches!(&p.ty.node, crate::ast::TypeExpr::Simple(t) if matches!(t.as_str(), "String" | "Int" | "Float" | "Bool" | "Map" | "List"))
                        || matches!(&p.ty.node, crate::ast::TypeExpr::Generic { name, .. } if name == "Map" || name == "List");
                    if data { safe.insert(p.name.clone()); } else { safe.remove(&p.name); }
                }
                // every name the body binds (lambda / loop / match binders,
                // lets inside lambdas): a call of one is a function value,
                // even when it is capitalized (`let K = g  K(K)`) or named
                // like a builtin (`abs => abs(abs)`)
                let mut locals: std::collections::HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
                crate::checker::dispatch::bind_all(body, &mut locals);
                let risky = |n: &str| !safe.contains(n)
                    && (locals.contains(n) || (!handler_names.contains(n) && !builtins.contains(n) && !n.starts_with(|c: char| c.is_uppercase())));
                const HIGHER: &[&str] = &["map", "filter", "find", "reduce", "any", "all", "count", "sort_by", "each", "flat_map", "fold", "group_by", "min_by", "max_by"];
                let mut hit: Option<String> = None;
                let scan = |x: &Expr, hit: &mut Option<String>| {
                    crate::checker::literals::for_each_in_expr(x, &mut |y| {
                        if hit.is_some() { return; }
                        if let Expr::FnCall { name, args } = y {
                            if risky(name) { *hit = Some(name.clone()); return; }
                            // `map("k", v)` builds a Map: no function argument
                            let constructor = name == "map" && matches!(args.first().map(|a| &a.node), Some(Expr::Literal(_)));
                            if HIGHER.contains(&name.as_str()) && !constructor {
                                for a in args {
                                    if let Expr::Ident(v) = &a.node { if risky(v) { *hit = Some(v.clone()); return; } }
                                }
                            }
                        }
                    });
                };
                crate::checker::literals::for_each_expr(body, &mut |e| {
                    if hit.is_some() { return; }
                    match e {
                        Expr::Lambda { body, .. } => scan(&body.node, &mut hit),
                        Expr::LambdaBlock { stmts, result, .. } => {
                            crate::checker::literals::for_each_expr(stmts, &mut |x| scan(x, &mut hit));
                            scan(&result.node, &mut hit);
                        }
                        _ => {}
                    }
                });
                if hit.is_none() {
                    crate::checker::literals::for_each_expr(body, &mut |e| if let Expr::FnCall { name, .. } = e {
                        if hit.is_none() && params.iter().any(|p| &p.name == name) && !safe.contains(name) { hit = Some(name.clone()); }
                    });
                }
                hit
            }
}
