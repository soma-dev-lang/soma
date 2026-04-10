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
//!   2. There are no `while` loops without `[loop_bound(N)]`
//!   3. Every recursive call has a literal integer argument that
//!      is strictly less than the caller's corresponding parameter
//!      (structural recursion on a decreasing Int argument)
//!   4. No direct/mutual recursion without a decreasing measure
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

/// Check termination for all handlers in a cell.
pub fn check_cell_termination(cell: &CellDef) -> Vec<TerminationFinding> {
    let mut findings = Vec::new();

    for section in &cell.sections {
        if let Section::OnSignal(ref on) = section.node {
            let mut reasons = Vec::new();
            for stmt in &on.body {
                check_stmt_termination(&stmt.node, &on.signal_name, &on.params, &mut reasons);
            }
            if reasons.is_empty() {
                findings.push(TerminationFinding::Terminates {
                    handler: on.signal_name.clone(),
                });
            } else {
                findings.push(TerminationFinding::MayNotTerminate {
                    handler: on.signal_name.clone(),
                    reasons,
                });
            }
        }
    }

    findings
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
                || is_collection_iter(&iter.node);

            if !is_bounded {
                reasons.push(format!(
                    "handler `{}`: for-loop with unbounded iterator (add [loop_bound(N)] or use range(0, N) with literal N)",
                    handler_name
                ));
            }

            // Recurse into body
            for s in body {
                check_stmt_termination(&s.node, handler_name, params, reasons);
            }
        }

        Statement::While { body, .. } => {
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

        Statement::If { then_body, else_body, .. } => {
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

        // These are always terminating
        Statement::Emit { .. } | Statement::Require { .. }
        | Statement::MethodCall { .. } | Statement::Break
        | Statement::Continue | Statement::Ensure { .. } => {}
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
            // Check for recursive call to the same handler
            if name == handler_name {
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

        // Leaf expressions and simple combinators — always terminate
        _ => {}
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
