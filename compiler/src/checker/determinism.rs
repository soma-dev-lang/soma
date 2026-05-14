//! V1.6: `[deterministic]` handler contract.
//!
//! A handler annotated `[deterministic]` must produce the same output for
//! the same input. This pass walks every handler body and flags calls
//! that would make the handler depend on state outside its arguments.
//!
//! Required for: golden tests, replay-as-spec, A/B comparisons, and the
//! property-based test machinery (#8) that quantifies over arbitrary inputs.

use crate::ast::*;

#[derive(Debug)]
pub struct DeterminismError {
    pub handler_name: String,
    pub bad_call: String,
    pub reason: &'static str,
}

impl std::fmt::Display for DeterminismError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "handler '{}' is [deterministic] but calls '{}' — {}",
            self.handler_name, self.bad_call, self.reason
        )
    }
}

/// Builtins forbidden in `[deterministic]` handlers, grouped by why.
fn forbid_reason(name: &str) -> Option<&'static str> {
    match name {
        "now" | "now_ms" | "timestamp" | "today" | "date_now" =>
            Some("reads wall-clock time"),
        "random" | "rand" | "uuid" | "next_id" =>
            Some("reads a non-deterministic source"),
        "think" | "think_json" =>
            Some("calls the LLM (output is not a function of the prompt)"),
        "http_get" | "http_post" | "http_put" | "http_delete" | "http_patch" |
        "fetch" | "fetch_json" =>
            Some("performs network I/O"),
        "read_file" | "write_file" | "append_file" | "delete_file" =>
            Some("performs file I/O"),
        "delegate" =>
            Some("invokes another cell (state outside this handler)"),
        _ => None,
    }
}

fn mutating_method(name: &str) -> bool {
    matches!(name, "set" | "delete" | "append" | "clear" | "push")
}

pub fn check_deterministic_handler(
    handler_name: &str,
    body: &[Spanned<Statement>],
) -> Vec<DeterminismError> {
    let mut errs = Vec::new();
    for stmt in body {
        walk_stmt(handler_name, &stmt.node, &mut errs);
    }
    errs
}

fn walk_stmt(handler_name: &str, stmt: &Statement, errs: &mut Vec<DeterminismError>) {
    match stmt {
        Statement::Let { value, .. }
        | Statement::Assign { value, .. }
        | Statement::Return { value }
        | Statement::Ensure { condition: value } => walk_expr(handler_name, &value.node, errs),
        Statement::ExprStmt { expr } => walk_expr(handler_name, &expr.node, errs),
        Statement::If { condition, then_body, else_body } => {
            walk_expr(handler_name, &condition.node, errs);
            for s in then_body { walk_stmt(handler_name, &s.node, errs); }
            for s in else_body { walk_stmt(handler_name, &s.node, errs); }
        }
        Statement::While { condition, body, .. } => {
            walk_expr(handler_name, &condition.node, errs);
            for s in body { walk_stmt(handler_name, &s.node, errs); }
        }
        Statement::For { iter, body, .. } => {
            walk_expr(handler_name, &iter.node, errs);
            for s in body { walk_stmt(handler_name, &s.node, errs); }
        }
        Statement::Emit { .. } => {
            errs.push(DeterminismError {
                handler_name: handler_name.to_string(),
                bad_call: "emit".to_string(),
                reason: "raises a signal (cross-cell effect)",
            });
        }
        Statement::MethodCall { method, args, .. } => {
            if mutating_method(method) {
                errs.push(DeterminismError {
                    handler_name: handler_name.to_string(),
                    bad_call: format!(".{}()", method),
                    reason: "mutates persistent storage",
                });
            }
            for arg in args { walk_expr(handler_name, &arg.node, errs); }
        }
        Statement::Require { .. } | Statement::Break | Statement::Continue => {}
    }
}

fn walk_expr(handler_name: &str, expr: &Expr, errs: &mut Vec<DeterminismError>) {
    match expr {
        Expr::Literal(_) | Expr::Ident(_) => {}
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            walk_expr(handler_name, &left.node, errs);
            walk_expr(handler_name, &right.node, errs);
        }
        Expr::Not(inner) | Expr::Try(inner) | Expr::TryPropagate(inner) => {
            walk_expr(handler_name, &inner.node, errs);
        }
        Expr::FnCall { name, args } => {
            if let Some(reason) = forbid_reason(name) {
                errs.push(DeterminismError {
                    handler_name: handler_name.to_string(),
                    bad_call: name.clone(),
                    reason,
                });
            }
            for arg in args { walk_expr(handler_name, &arg.node, errs); }
        }
        Expr::Lambda { body, .. } => walk_expr(handler_name, &body.node, errs),
        Expr::LambdaBlock { stmts, result, .. } => {
            for s in stmts { walk_stmt(handler_name, &s.node, errs); }
            walk_expr(handler_name, &result.node, errs);
        }
        Expr::FieldAccess { target, .. } => walk_expr(handler_name, &target.node, errs),
        Expr::MethodCall { target, method, args } => {
            walk_expr(handler_name, &target.node, errs);
            if mutating_method(method) {
                errs.push(DeterminismError {
                    handler_name: handler_name.to_string(),
                    bad_call: format!(".{}()", method),
                    reason: "mutates persistent storage",
                });
            }
            for arg in args { walk_expr(handler_name, &arg.node, errs); }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields { walk_expr(handler_name, &v.node, errs); }
        }
        Expr::Match { subject, arms } => {
            walk_expr(handler_name, &subject.node, errs);
            for arm in arms {
                if let Some(g) = &arm.guard { walk_expr(handler_name, &g.node, errs); }
                for s in &arm.body { walk_stmt(handler_name, &s.node, errs); }
                walk_expr(handler_name, &arm.result.node, errs);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            walk_expr(handler_name, &condition.node, errs);
            for s in then_body { walk_stmt(handler_name, &s.node, errs); }
            walk_expr(handler_name, &then_result.node, errs);
            for s in else_body { walk_stmt(handler_name, &s.node, errs); }
            walk_expr(handler_name, &else_result.node, errs);
        }
        Expr::ListLiteral(items) => {
            for it in items { walk_expr(handler_name, &it.node, errs); }
        }
    }
}
