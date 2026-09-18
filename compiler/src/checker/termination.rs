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
            for stmt in &on.body {
                check_stmt_termination(&stmt.node, &on.signal_name, &on.params, &mut reasons);
            }
            let mut self_recursive = false;
            for stmt in &on.body {
                walk_stmt(&stmt.node, &mut |e| {
                    if matches!(e, Expr::FnCall { name, args } if name == &on.signal_name && crate::ast::accepted_arities(&on.params).contains(&args.len())) {
                        self_recursive = true;
                    }
                });
            }
            // a decreasing argument over a parameter the body re-binds is no measure
            if self_recursive && reasons.is_empty() {
                let mut measured = false;
                for stmt in &on.body {
                    walk_stmt(&stmt.node, &mut |e| {
                        if let Expr::FnCall { name, args } = e {
                            if name == &on.signal_name && args.iter().enumerate().any(|(i, a)|
                                on.params.get(i).map_or(false, |p| is_decreasing_arg(&a.node, &p.name) && !rebinds(&on.body, &p.name)))
                            { measured = true; }
                        }
                    });
                }
                if !measured {
                    reasons.push(format!(
                        "handler `{}`: recursive call without provable decreasing argument (the parameter is re-assigned or re-bound in the body)",
                        on.signal_name
                    ));
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
                let decreasing: Vec<&Param> = on.params.iter().filter(|p| {
                    let mut used = false;
                    for stmt in &on.body {
                        walk_stmt(&stmt.node, &mut |e| {
                            if let Expr::FnCall { name, args } = e {
                                if name == &on.signal_name && args.iter().any(|a| is_decreasing_arg(&a.node, &p.name)) { used = true; }
                            }
                        });
                    }
                    used
                }).collect();
                for p in decreasing {
                    let is_int = matches!(&p.ty.node, TypeExpr::Simple(t) if t == "Int");
                    if !is_int {
                        reasons.push(format!(
                            "handler `{}`: the decreasing argument `{}` is not an Int (a Float step can stall: x - 1 == x past 2^53)",
                            on.signal_name, p.name
                        ));
                    } else if !has_lower_bound_exit(&on.body, &p.name, &on.signal_name) {
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
    let mut handlers: Vec<&OnSection> = Vec::new();
    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.node.sections {
            if let Section::OnSignal(ref on) = section.node {
                handlers.push(on);
            }
        }
    }
    let names: HashSet<&str> = handlers.iter().map(|h| h.signal_name.as_str()).collect();
    let cells: HashSet<&str> = program.cells.iter().map(|c| c.node.name.as_str()).collect();
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
        fn stmt_edges(stmts: &[Spanned<Statement>], cells: &HashSet<&str>, out: &mut Vec<String>) {
            for st in stmts {
                match &st.node {
                    Statement::MethodCall { target, method, .. } if cells.contains(target.as_str()) => out.push(method.clone()),
                    Statement::Emit { signal_name, .. } => out.push(signal_name.clone()),
                    Statement::If { then_body, else_body, .. } => { stmt_edges(then_body, cells, out); stmt_edges(else_body, cells, out); }
                    Statement::For { body, .. } | Statement::While { body, .. } => stmt_edges(body, cells, out),
                    _ => {}
                }
            }
        }
        let mut extra = Vec::new();
        stmt_edges(&on.body, &cells, &mut extra);
        for name in extra {
            if names.contains(name.as_str()) && name != on.signal_name && !calls.contains(&name) { calls.push(name); }
        }
        graph.entry(on.signal_name.clone()).or_default().extend(calls);
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

        Statement::While { condition, body, .. } => {
            check_expr_termination(&condition.node, handler_name, params, reasons);
            // While loops are NEVER structurally terminating without
            // additional analysis. Flag them unconditionally.
            // Future: check for [loop_bound(N)] on while loops.
            reasons.push(format!(
                "handler `{}`: while-loop without provable termination bound (consider replacing with a bounded for-loop or adding a max-iteration guard)",
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
    )
}

/// Check if an argument is provably smaller than a parameter.
/// Recognizes patterns like: `param - 1`, `param - literal`.
fn is_decreasing_arg(arg: &Expr, param_name: &str) -> bool {
    if let Expr::BinaryOp { left, op, right } = arg {
        if matches!(op, BinOp::Sub) {
            if let Expr::Ident(name) = &left.node {
                if name == param_name {
                    if let Expr::Literal(Literal::Int(n)) = &right.node {
                        return *n > 0;
                    }
                }
            }
        }
    }
    false
}

/// Does the body stop the descent with a lower bound on `param` before any
/// statement can recurse: `if param <= c { … return … }` (or `< c`, or the
/// flipped spelling), or `require param >= c` — with a literal `c`?
fn has_lower_bound_exit(body: &[Spanned<Statement>], param: &str, handler: &str) -> bool {
    fn bounded(cond: &Expr, param: &str) -> bool {
        match cond {
            Expr::CmpOp { left, op, right } => {
                let lit = |e: &Expr| matches!(e, Expr::Literal(Literal::Int(_)));
                match (&left.node, &right.node) {
                    (Expr::Ident(n), r) if n == param && lit(r) => matches!(op, CmpOp::Le | CmpOp::Lt),
                    (l, Expr::Ident(n)) if n == param && lit(l) => matches!(op, CmpOp::Ge | CmpOp::Gt),
                    _ => false,
                }
            }
            Expr::BinaryOp { left, op: BinOp::Or, right } => bounded(&left.node, param) || bounded(&right.node, param),
            _ => false,
        }
    }
    for st in body {
        match &st.node {
            Statement::If { condition, then_body, .. } => {
                let exits = matches!(then_body.last().map(|s| &s.node), Some(Statement::Return { .. }))
                    || matches!(then_body.last().map(|s| &s.node), Some(Statement::ExprStmt { expr }) if matches!(&expr.node, Expr::FnCall { name, .. } if name == "fail"));
                if exits && bounded(&condition.node, param) && !calls_self(then_body, handler) { return true; }
            }
            Statement::Require { constraint, .. } => {
                if let Constraint::Comparison { left, op, right } = &constraint.node {
                    let lit = |e: &Expr| matches!(e, Expr::Literal(Literal::Int(_)));
                    let ok = match (&left.node, &right.node) {
                        (Expr::Ident(n), r) if n == param && lit(r) => matches!(op, CmpOp::Ge | CmpOp::Gt),
                        (l, Expr::Ident(n)) if n == param && lit(l) => matches!(op, CmpOp::Le | CmpOp::Lt),
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
