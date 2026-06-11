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
            collect_calls(body, &mut calls);

            let mut seen: HashSet<String> = HashSet::new();
            for name in calls {
                if !seen.insert(name.clone()) {
                    continue; // report each callee once per handler
                }
                if bound.contains(&name)
                    || index.variants.contains(&name)
                    || index.slots.contains(&name)
                    || super::names::builtin_names().contains(name.as_str())
                {
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

/// Collect every FnCall name in a statement list, in source order.
pub(super) fn collect_calls(stmts: &[Spanned<Statement>], out: &mut Vec<String>) {
    for stmt in stmts {
        match &stmt.node {
            Statement::Let { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Return { value }
            | Statement::Ensure { condition: value } => collect_calls_expr(&value.node, out),
            Statement::ExprStmt { expr } => collect_calls_expr(&expr.node, out),
            Statement::If { condition, then_body, else_body } => {
                collect_calls_expr(&condition.node, out);
                collect_calls(then_body, out);
                collect_calls(else_body, out);
            }
            Statement::For { iter, body, .. } => {
                collect_calls_expr(&iter.node, out);
                collect_calls(body, out);
            }
            Statement::While { condition, body, .. } => {
                collect_calls_expr(&condition.node, out);
                collect_calls(body, out);
            }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => {
                for a in args {
                    collect_calls_expr(&a.node, out);
                }
            }
            Statement::Require { constraint, .. } => {
                collect_calls_constraint(&constraint.node, out);
            }
            Statement::Break | Statement::Continue => {}
        }
    }
}

fn collect_calls_constraint(c: &Constraint, out: &mut Vec<String>) {
    match c {
        Constraint::Comparison { left, right, .. } => {
            collect_calls_expr(&left.node, out);
            collect_calls_expr(&right.node, out);
        }
        Constraint::And(a, b) | Constraint::Or(a, b) => {
            collect_calls_constraint(&a.node, out);
            collect_calls_constraint(&b.node, out);
        }
        Constraint::Not(inner) => collect_calls_constraint(&inner.node, out),
        // Predicate names are checker predicates, not dispatch targets.
        Constraint::Predicate { .. } | Constraint::Descriptive(_) => {}
    }
}

pub(super) fn collect_calls_expr(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::FnCall { name, args } => {
            out.push(name.clone());
            for a in args {
                collect_calls_expr(&a.node, out);
            }
        }
        Expr::FieldAccess { target, .. } => collect_calls_expr(&target.node, out),
        Expr::MethodCall { target, args, .. } => {
            collect_calls_expr(&target.node, out);
            for a in args {
                collect_calls_expr(&a.node, out);
            }
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. } => {
            collect_calls_expr(&left.node, out);
            collect_calls_expr(&right.node, out);
        }
        Expr::Pipe { left, right } => {
            collect_calls_expr(&left.node, out);
            // The runtime supports `expr |> fn` with a BARE identifier on
            // the right (interpreter rewrites it to fn(expr)) — that
            // identifier is a call, not a variable reference.
            if let Expr::Ident(name) = &right.node {
                out.push(name.clone());
            } else {
                collect_calls_expr(&right.node, out);
            }
        }
        Expr::Not(inner) | Expr::Try(inner) | Expr::TryPropagate(inner) => {
            collect_calls_expr(&inner.node, out);
        }
        Expr::ListLiteral(items) => {
            for item in items {
                collect_calls_expr(&item.node, out);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                collect_calls_expr(&v.node, out);
            }
        }
        Expr::Lambda { body, .. } => collect_calls_expr(&body.node, out),
        Expr::LambdaBlock { stmts, result, .. } => {
            collect_calls(stmts, out);
            collect_calls_expr(&result.node, out);
        }
        Expr::Match { subject, arms } => {
            collect_calls_expr(&subject.node, out);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_calls_expr(&g.node, out);
                }
                collect_calls(&arm.body, out);
                collect_calls_expr(&arm.result.node, out);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            collect_calls_expr(&condition.node, out);
            collect_calls(then_body, out);
            collect_calls_expr(&then_result.node, out);
            collect_calls(else_body, out);
            collect_calls_expr(&else_result.node, out);
        }
        Expr::Literal(_) | Expr::Ident(_) => {}
    }
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
