//! Static dispatch resolution (V1.7).
//!
//! Cross-cell calls dispatch by bare name to the first cell that
//! defines a handler with that name — collisions are silent at
//! runtime. This pass resolves every FnCall in every handler body at
//! check time:
//!
//!   - unknown name (no cell, no builtin)        → check ERROR
//!   - 2+ definers, caller is not one of them     → check ERROR
//!   - caller defines it AND another cell does    → check WARNING
//!   - name is a builtin AND a handler, called
//!     from a different handler                   → check WARNING
//!     (builtins win dispatch: the handler body never runs)
//!     (the recursive-self-call trap that forced the Mesa app to
//!     rename its whole domain API)
//!
//! Legitimate recursion (single definer) and lambda-valued variables
//! (`g = x => ...; g(1)`) stay silent. Conservative by design: any
//! name bound anywhere in the handler, any builtin, variant
//! constructor, memory slot or face declaration is skipped.

use crate::ast::*;
use std::collections::HashSet;

use super::interpolation_check::bind_pattern;
use super::names::{suggest, ProgramIndex};

#[derive(Debug)]
pub struct DispatchFinding {
    pub message: String,
    pub span: Span,
}

pub fn check_program(program: &Program) -> (Vec<DispatchFinding>, Vec<DispatchFinding>) {
    let index = ProgramIndex::build(program);
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    for cell in super::names::collect_cells(program) {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.sections {
            let (handler_label, params, body): (String, &[Param], &[Spanned<Statement>]) =
                match &section.node {
                    Section::OnSignal(on) => (on.signal_name.clone(), &on.params, &on.body),
                    Section::Every(ev) => ("every".to_string(), &[], &ev.body),
                    Section::After(ev) => ("after".to_string(), &[], &ev.body),
                    _ => continue,
                };

            // Everything the handler can bind locally — params, lets,
            // assignments, loop vars, lambda params, match bindings.
            // A call to any of these is a lambda-valued variable call.
            let mut bound: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
            bind_all(body, &mut bound);

            let mut calls: Vec<String> = Vec::new();
            let mut arities: Vec<(String, usize, bool)> = Vec::new();
            visit_calls(body, &mut |name, argc, has_lambda| {
                calls.push(name.to_string());
                arities.push((name.to_string(), argc, has_lambda));
            });
            let caller_native = match &section.node {
                Section::OnSignal(on) => on.properties.iter().any(|p| p == "native"),
                _ => false,
            };

            let mut seen: HashSet<String> = HashSet::new();
            for name in calls {
                if !seen.insert(name.clone()) {
                    continue; // report each callee once per handler
                }
                if bound.contains(&name)
                    || index.variants.contains(&name)
                    || index.slots.contains(&name)
                {
                    continue;
                }
                if super::names::builtin_names().contains(name.as_str()) {
                    // Builtins win dispatch over handlers (so `on list()` can
                    // call the builtin list()). From any OTHER handler the
                    // intent is ambiguous and the user's body silently never
                    // runs — say which one the call resolves to.
                    // …when the builtin can actually take the call (else it
                    // falls through to the handler), and not between
                    // [native] handlers, which link to each other directly.
                    let taken = arities
                        .iter()
                        .any(|(n, argc, has_lambda)| n == &name && builtin_accepts(n, *argc, *has_lambda));
                    let native_pair = caller_native && index_native(program, &name);
                    if handler_label != name && taken && !native_pair {
                        if let Some(definers) = index.handler_map.get(&name) {
                            warnings.push(DispatchFinding {
                                message: format!(
                                    "call to '{name}' inside {caller}.{handler_label} resolves to the \
                                     BUILTIN {name}(), not the handler {definer}.{name} — builtins win \
                                     dispatch, so that handler's body never runs from a call site. \
                                     Rename the handler if you meant it",
                                    caller = cell.name,
                                    definer = definers[0],
                                ),
                                span: section.span,
                            });
                        }
                    }
                    continue;
                }
                match index.handler_map.get(&name) {
                    None => {
                        // Not a handler anywhere. Face declarations
                        // without handlers are already reported by the
                        // face-contract check — stay silent for those.
                        if index.known.contains(&name) {
                            continue;
                        }
                        // keys/values/entries/… exist only as METHODS on
                        // maps and memory slots — point at the real form.
                        let suggestion = if matches!(name.as_str(),
                            "keys" | "values" | "entries" | "has" | "delete") {
                            format!(" ('{name}' is a method — write m.{name}())")
                        } else {
                            suggest(&name, index.known.iter().chain(bound.iter()))
                                .map(|s| format!(" (did you mean '{}'?)", s))
                                .unwrap_or_default()
                        };
                        errors.push(DispatchFinding {
                            message: format!(
                                "undefined function '{name}'{suggestion} — no cell defines a \
                                 handler 'on {name}(...)' and it is not a builtin"
                            ),
                            span: section.span,
                        });
                    }
                    Some(definers) => {
                        let caller_defines = definers.contains(&cell.name);
                        if caller_defines && definers.len() >= 2 {
                            let others: Vec<&String> =
                                definers.iter().filter(|d| **d != cell.name).collect();
                            let other = others[0];
                            if handler_label == name {
                                warnings.push(DispatchFinding {
                                    message: format!(
                                        "call to '{name}' inside {caller}.{name} is a recursive \
                                         self-call, but cell {other} also defines '{name}' — if \
                                         you meant {other}'s, rename one of them",
                                        caller = cell.name,
                                    ),
                                    span: section.span,
                                });
                            } else {
                                warnings.push(DispatchFinding {
                                    message: format!(
                                        "call to '{name}' inside {caller}.{handler_label}: both \
                                         {caller} and {other} define '{name}' — dispatch picks \
                                         one silently; rename one handler to disambiguate",
                                        caller = cell.name,
                                    ),
                                    span: section.span,
                                });
                            }
                        } else if !caller_defines && definers.len() >= 2 {
                            let listed = match definers.len() {
                                2 => format!("{} and {}", definers[0], definers[1]),
                                _ => {
                                    let head = definers[..definers.len() - 1].join(", ");
                                    format!("{} and {}", head, definers[definers.len() - 1])
                                }
                            };
                            errors.push(DispatchFinding {
                                message: format!(
                                    "ambiguous call '{name}': defined by cells {listed} — \
                                     rename one handler to disambiguate"
                                ),
                                span: section.span,
                            });
                        }
                        // Exactly one definer → unambiguous, silent.
                    }
                }
            }
        }
    }

    (errors, warnings)
}

/// Visit every FnCall (name, argument count) in a statement list, in
/// source order.
fn visit_calls(stmts: &[Spanned<Statement>], out: &mut dyn FnMut(&str, usize, bool)) {
    for stmt in stmts {
        match &stmt.node {
            Statement::Let { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Return { value }
            | Statement::Ensure { condition: value } => visit_calls_expr(&value.node, out),
            Statement::ExprStmt { expr } => visit_calls_expr(&expr.node, out),
            Statement::IndexSet { index, value, .. } => { visit_calls_expr(&index.node, out); visit_calls_expr(&value.node, out); }
            Statement::If { condition, then_body, else_body } => {
                visit_calls_expr(&condition.node, out);
                visit_calls(then_body, out);
                visit_calls(else_body, out);
            }
            Statement::For { iter, body, .. } => {
                visit_calls_expr(&iter.node, out);
                visit_calls(body, out);
            }
            Statement::While { condition, body, .. } => {
                visit_calls_expr(&condition.node, out);
                visit_calls(body, out);
            }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => {
                for a in args {
                    visit_calls_expr(&a.node, out);
                }
            }
            Statement::Require { constraint, .. } => {
                visit_calls_constraint(&constraint.node, out);
            }
            Statement::Break | Statement::Continue => {}
        }
    }
}

fn visit_calls_constraint(c: &Constraint, out: &mut dyn FnMut(&str, usize, bool)) {
    match c {
        Constraint::Comparison { left, right, .. } => {
            visit_calls_expr(&left.node, out);
            visit_calls_expr(&right.node, out);
        }
        Constraint::And(a, b) | Constraint::Or(a, b) => {
            visit_calls_constraint(&a.node, out);
            visit_calls_constraint(&b.node, out);
        }
        Constraint::Not(inner) => visit_calls_constraint(&inner.node, out),
        // Predicate names are checker predicates, not dispatch targets.
        Constraint::Predicate { .. } | Constraint::Descriptive(_) => {}
    }
}

fn visit_calls_expr(expr: &Expr, out: &mut dyn FnMut(&str, usize, bool)) {
    match expr {
        Expr::FnCall { name, args } => {
            let has_lambda = args
                .iter()
                .any(|a| matches!(a.node, Expr::Lambda { .. } | Expr::LambdaBlock { .. }));
            out(name, args.len(), has_lambda);
            for a in args {
                visit_calls_expr(&a.node, out);
            }
        }
        Expr::FieldAccess { target, .. } => visit_calls_expr(&target.node, out),
        Expr::Index { target, index } => { visit_calls_expr(&target.node, out); visit_calls_expr(&index.node, out); }
        Expr::MethodCall { target, args, .. } => {
            visit_calls_expr(&target.node, out);
            for a in args {
                visit_calls_expr(&a.node, out);
            }
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. } => {
            visit_calls_expr(&left.node, out);
            visit_calls_expr(&right.node, out);
        }
        Expr::Pipe { left, right } => {
            visit_calls_expr(&left.node, out);
            // The runtime supports `expr |> fn` with a BARE identifier on
            // the right (interpreter rewrites it to fn(expr)) — that
            // identifier is a call, not a variable reference.
            if let Expr::Ident(name) = &right.node {
                out(name, 1, false);
            } else {
                visit_calls_expr(&right.node, out);
            }
        }
        Expr::Not(inner) | Expr::Try(inner) | Expr::TryPropagate(inner) => {
            visit_calls_expr(&inner.node, out);
        }
        Expr::ListLiteral(items) => {
            for item in items {
                visit_calls_expr(&item.node, out);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                visit_calls_expr(&v.node, out);
            }
        }
        Expr::Lambda { body, .. } => visit_calls_expr(&body.node, out),
        Expr::LambdaBlock { stmts, result, .. } => {
            visit_calls(stmts, out);
            visit_calls_expr(&result.node, out);
        }
        Expr::Match { subject, arms } => {
            visit_calls_expr(&subject.node, out);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    visit_calls_expr(&g.node, out);
                }
                visit_calls(&arm.body, out);
                visit_calls_expr(&arm.result.node, out);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            visit_calls_expr(&condition.node, out);
            visit_calls(then_body, out);
            visit_calls_expr(&then_result.node, out);
            visit_calls(else_body, out);
            visit_calls_expr(&else_result.node, out);
        }
        Expr::Literal(_) | Expr::Ident(_) => {}
    }
}

/// Collect every FnCall name in a statement list, in source order.
pub(super) fn collect_calls(stmts: &[Spanned<Statement>], out: &mut Vec<String>) {
    visit_calls(stmts, &mut |name, _, _| out.push(name.to_string()));
}

pub(super) fn collect_calls_expr(expr: &Expr, out: &mut Vec<String>) {
    visit_calls_expr(expr, &mut |name, _, _| out.push(name.to_string()));
}

/// Can the builtin `name` accept a call with `argc` arguments? Read off
/// the registry signature ("f(a, b) -> T | f(list: List) -> T"). A
/// builtin only wins dispatch when it takes the call — `floor()` with no
/// argument falls through to a user handler `on floor()`. Unknown or
/// unparseable shapes answer true (warn rather than stay silent).
fn builtin_accepts(name: &str, argc: usize, has_lambda: bool) -> bool {
    let Some(doc) = crate::interpreter::builtins::registry::BUILTINS.iter().find(|d| d.name == name) else {
        return true;
    };
    // Lambda builtins (find, count, filter…) are only tried when an
    // argument is a lambda; otherwise the call reaches the handler.
    if doc.category == "lambda" && !has_lambda {
        return false;
    }
    let mut any_parsed = false;
    for alt in doc.signature.split(" | ") {
        let alt = alt.trim();
        let head = format!("{name}(");
        if alt.contains("|>") || !alt.starts_with(&head) {
            return true;
        }
        let rest = &alt[head.len()..];
        let mut depth = 1usize;
        let mut close = None;
        for (i, c) in rest.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else { return true };
        any_parsed = true;
        let params = rest[..close].trim();
        let variadic = params.contains("...");
        let mut n = 0usize;
        if !params.is_empty() {
            let mut d = 0i32;
            n = 1;
            for c in params.chars() {
                match c {
                    '(' | '[' | '<' => d += 1,
                    ')' | ']' => d -= 1,
                    // `=>` in lambda params is not a closing bracket
                    '>' if d > 0 => d -= 1,
                    ',' if d == 0 => n += 1,
                    _ => {}
                }
            }
        }
        if variadic {
            // "items..." itself may be empty; a bare ", ..." adds no slot
            let fixed = if params.trim_end().ends_with(", ...") { n - 1 } else { n.saturating_sub(1) };
            if argc >= fixed {
                return true;
            }
        } else if argc == n {
            return true;
        }
    }
    !any_parsed
}

/// Collect every name a handler body can bind, including nested blocks,
/// lambda params and match patterns.
fn bind_all(stmts: &[Spanned<Statement>], bound: &mut HashSet<String>) {
    for stmt in stmts {
        match &stmt.node {
            Statement::Let { name, value } | Statement::Assign { name, value } => {
                bound.insert(name.clone());
                bind_all_expr(&value.node, bound);
            }
            Statement::Return { value } | Statement::Ensure { condition: value } => {
                bind_all_expr(&value.node, bound);
            }
            Statement::ExprStmt { expr } => bind_all_expr(&expr.node, bound),
            Statement::IndexSet { name, index, value } => { bound.insert(name.clone()); bind_all_expr(&index.node, bound); bind_all_expr(&value.node, bound); }
            Statement::If { condition, then_body, else_body } => {
                bind_all_expr(&condition.node, bound);
                bind_all(then_body, bound);
                bind_all(else_body, bound);
            }
            Statement::For { var, iter, body, .. } => {
                bound.insert(var.clone());
                bind_all_expr(&iter.node, bound);
                bind_all(body, bound);
            }
            Statement::While { condition, body, .. } => {
                bind_all_expr(&condition.node, bound);
                bind_all(body, bound);
            }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => {
                for a in args {
                    bind_all_expr(&a.node, bound);
                }
            }
            Statement::Require { .. } | Statement::Break | Statement::Continue => {}
        }
    }
}

fn bind_all_expr(expr: &Expr, bound: &mut HashSet<String>) {
    match expr {
        Expr::Lambda { param, body } => {
            bound.insert(param.clone());
            bind_all_expr(&body.node, bound);
        }
        Expr::LambdaBlock { param, stmts, result } => {
            bound.insert(param.clone());
            bind_all(stmts, bound);
            bind_all_expr(&result.node, bound);
        }
        Expr::Match { subject, arms } => {
            bind_all_expr(&subject.node, bound);
            for arm in arms {
                bind_pattern(&arm.pattern, bound);
                bind_all(&arm.body, bound);
                bind_all_expr(&arm.result.node, bound);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            bind_all_expr(&condition.node, bound);
            bind_all(then_body, bound);
            bind_all_expr(&then_result.node, bound);
            bind_all(else_body, bound);
            bind_all_expr(&else_result.node, bound);
        }
        Expr::FnCall { args, .. } => {
            for a in args {
                bind_all_expr(&a.node, bound);
            }
        }
        Expr::Index { target, index } => {
            bind_all_expr(&target.node, bound);
            bind_all_expr(&index.node, bound);
        }
        Expr::MethodCall { target, args, .. } => {
            bind_all_expr(&target.node, bound);
            for a in args {
                bind_all_expr(&a.node, bound);
            }
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            bind_all_expr(&left.node, bound);
            bind_all_expr(&right.node, bound);
        }
        Expr::Not(inner) | Expr::Try(inner) | Expr::TryPropagate(inner) => {
            bind_all_expr(&inner.node, bound);
        }
        Expr::ListLiteral(items) => {
            for item in items {
                bind_all_expr(&item.node, bound);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                bind_all_expr(&v.node, bound);
            }
        }
        Expr::FieldAccess { target, .. } => bind_all_expr(&target.node, bound),
        Expr::Literal(_) | Expr::Ident(_) => {}
    }
}

/// Is some handler named `name` marked [native]?
fn index_native(program: &Program, name: &str) -> bool {
    super::names::collect_cells(program).iter().any(|cell| {
        cell.sections.iter().any(|s| match &s.node {
            Section::OnSignal(on) => on.signal_name == name && on.properties.iter().any(|p| p == "native"),
            _ => false,
        })
    })
}
