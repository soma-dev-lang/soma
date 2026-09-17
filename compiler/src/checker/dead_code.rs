//! Unreachable statements.
//!
//! Statements are newline-separated and adjacent string literals do NOT
//! concatenate, so
//!
//!     return "hello " "world"
//!
//! parses as `return "hello "` followed by a dead expression statement —
//! the handler silently returns half the text. This pass reports:
//!
//!   - a bare string literal right after `return`   → check ERROR
//!   - any other statement after return/break/continue
//!     in the same block                            → check WARNING

use crate::ast::*;

pub struct DeadCodeFinding {
    pub message: String,
    pub span: Span,
}

pub fn check_program(program: &Program) -> (Vec<DeadCodeFinding>, Vec<DeadCodeFinding>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for cell in super::names::collect_cells(program) {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.sections {
            let (label, body): (String, &[Spanned<Statement>]) = match &section.node {
                Section::OnSignal(on) => (format!("{}.{}", cell.name, on.signal_name), &on.body),
                Section::Every(ev) => (format!("{}.every", cell.name), &ev.body),
                Section::After(ev) => (format!("{}.after", cell.name), &ev.body),
                _ => continue,
            };
            check_block(body, &label, &mut errors, &mut warnings);
        }
    }
    (errors, warnings)
}

fn check_block(
    stmts: &[Spanned<Statement>],
    label: &str,
    errors: &mut Vec<DeadCodeFinding>,
    warnings: &mut Vec<DeadCodeFinding>,
) {
    for (i, stmt) in stmts.iter().enumerate() {
        let exit = match &stmt.node {
            Statement::Return { .. } => Some("return"),
            Statement::Break => Some("break"),
            Statement::Continue => Some("continue"),
            _ => None,
        };
        if let (Some(exit), Some(next)) = (exit, stmts.get(i + 1)) {
            let dangling_string = matches!(
                &next.node,
                Statement::ExprStmt { expr } if matches!(expr.node, Expr::Literal(Literal::String(_)))
            );
            if exit == "return" && dangling_string {
                errors.push(DeadCodeFinding {
                    message: format!(
                        "in {label}: a string literal follows `return` and is never evaluated — \
                         adjacent string literals do not concatenate. Write one literal, or \
                         interpolate: \"hello {{name}}\""
                    ),
                    span: next.span,
                });
            } else {
                warnings.push(DeadCodeFinding {
                    message: format!("in {label}: unreachable code after `{exit}`"),
                    span: next.span,
                });
            }
        }
        check_stmt(&stmt.node, label, errors, warnings);
    }
}

fn check_stmt(
    stmt: &Statement,
    label: &str,
    errors: &mut Vec<DeadCodeFinding>,
    warnings: &mut Vec<DeadCodeFinding>,
) {
    match stmt {
        Statement::If { condition, then_body, else_body } => {
            check_expr(&condition.node, label, errors, warnings);
            check_block(then_body, label, errors, warnings);
            check_block(else_body, label, errors, warnings);
        }
        Statement::For { iter, body, .. } => {
            check_expr(&iter.node, label, errors, warnings);
            check_block(body, label, errors, warnings);
        }
        Statement::While { condition, body, .. } => {
            check_expr(&condition.node, label, errors, warnings);
            check_block(body, label, errors, warnings);
        }
        Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => {
            check_expr(&value.node, label, errors, warnings)
        }
        Statement::ExprStmt { expr } => check_expr(&expr.node, label, errors, warnings),
        Statement::IndexSet { index, value, .. } => {
            check_expr(&index.node, label, errors, warnings);
            check_expr(&value.node, label, errors, warnings);
        }
        Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => {
            for a in args {
                check_expr(&a.node, label, errors, warnings);
            }
        }
        Statement::Ensure { condition } => check_expr(&condition.node, label, errors, warnings),
        Statement::Require { .. } | Statement::Break | Statement::Continue => {}
    }
}

/// Only expressions that carry statement blocks matter here.
fn check_expr(
    expr: &Expr,
    label: &str,
    errors: &mut Vec<DeadCodeFinding>,
    warnings: &mut Vec<DeadCodeFinding>,
) {
    match expr {
        Expr::LambdaBlock { stmts, result, .. } => {
            check_block(stmts, label, errors, warnings);
            check_expr(&result.node, label, errors, warnings);
        }
        Expr::Match { subject, arms } => {
            check_expr(&subject.node, label, errors, warnings);
            for arm in arms {
                check_block(&arm.body, label, errors, warnings);
                check_expr(&arm.result.node, label, errors, warnings);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            check_expr(&condition.node, label, errors, warnings);
            check_block(then_body, label, errors, warnings);
            check_expr(&then_result.node, label, errors, warnings);
            check_block(else_body, label, errors, warnings);
            check_expr(&else_result.node, label, errors, warnings);
        }
        Expr::FnCall { args, .. } => {
            for a in args {
                check_expr(&a.node, label, errors, warnings);
            }
        }
        Expr::MethodCall { target, args, .. } => {
            check_expr(&target.node, label, errors, warnings);
            for a in args {
                check_expr(&a.node, label, errors, warnings);
            }
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            check_expr(&left.node, label, errors, warnings);
            check_expr(&right.node, label, errors, warnings);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => check_expr(&i.node, label, errors, warnings),
        Expr::Lambda { body, .. } => check_expr(&body.node, label, errors, warnings),
        _ => {}
    }
}
