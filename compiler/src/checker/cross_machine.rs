//! Cross-machine composition lint (V1.7), part of `soma verify`.
//!
//! Per-cell verification proves each state machine is sound in
//! isolation — it cannot see a handler that advances machine A and
//! then calls another cell's handler that advances machine B and can
//! fail. If the callee fails, A is left advanced with no compensation
//! (the courier double-booking bug in the Mesa delivery app).
//!
//! This pass walks each handler's statement sequence in order. If a
//! `transition(...)` call (bare or inside `try`) is followed by a
//! cross-cell call to a handler that itself contains a transition
//! guarded by try/require (i.e. can fail), it emits a verify WARNING —
//! never a failure.

use crate::ast::*;
use std::collections::HashSet;

use super::dispatch::{collect_calls, collect_calls_expr};
use super::names::{collect_cells, ProgramIndex};

#[derive(Debug)]
pub struct CrossMachineWarning {
    pub cell: String,
    pub handler: String,
    pub machine: String,
    pub callee: String,
    pub callee_cell: String,
}

impl CrossMachineWarning {
    pub fn message(&self) -> String {
        format!(
            "composition: '{}' transitions machine '{}' and then calls '{}' (cell {}) \
             which can fail — a failure would leave '{}' advanced with no compensation. \
             Pre-check the callee's guard or compensate on failure.",
            self.handler, self.machine, self.callee, self.callee_cell, self.machine
        )
    }
}

pub fn check_program(program: &Program) -> Vec<CrossMachineWarning> {
    let index = ProgramIndex::build(program);
    let cells = collect_cells(program);

    // Which handlers can fail mid-flight? A callee is fallible if its
    // body contains a transition() call together with a try/require
    // guard — exactly the "try { transition(...) }" pattern.
    let mut fallible: HashSet<(String, String)> = HashSet::new(); // (cell, handler)
    for cell in &cells {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.sections {
            if let Section::OnSignal(on) = &section.node {
                if body_has_guarded_transition(&on.body) {
                    fallible.insert((cell.name.clone(), on.signal_name.clone()));
                }
            }
        }
    }

    let mut warnings = Vec::new();
    for cell in &cells {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        // The machine this cell's transition() calls advance.
        let machine = cell.sections.iter().find_map(|s| {
            if let Section::State(sm) = &s.node { Some(sm.name.clone()) } else { None }
        });
        let Some(machine) = machine else { continue };

        for section in &cell.sections {
            let Section::OnSignal(on) = &section.node else { continue };

            // Flatten the handler's statements depth-first, preserving
            // source order, and replay them: once a transition() has
            // happened, any later fallible cross-cell call is a hole.
            let mut events = Vec::new();
            collect_events(&on.body, &mut events);

            let mut transitioned = false;
            let mut reported: HashSet<String> = HashSet::new();
            for name in events {
                if name == "transition" {
                    transitioned = true;
                    continue;
                }
                if !transitioned || reported.contains(&name) {
                    continue;
                }
                let Some(definers) = index.handler_map.get(&name) else { continue };
                if definers.contains(&cell.name) {
                    continue; // same-cell call — same machine, not composition
                }
                if let Some(callee_cell) = definers
                    .iter()
                    .find(|d| fallible.contains(&((*d).clone(), name.clone())))
                {
                    reported.insert(name.clone());
                    warnings.push(CrossMachineWarning {
                        cell: cell.name.clone(),
                        handler: on.signal_name.clone(),
                        machine: machine.clone(),
                        callee: name.clone(),
                        callee_cell: callee_cell.clone(),
                    });
                }
            }
        }
    }

    warnings
}

/// All call names in a statement list, depth-first in source order.
/// Reuses the dispatch collector so ordering rules stay identical.
fn collect_events(stmts: &[Spanned<Statement>], out: &mut Vec<String>) {
    collect_calls(stmts, out);
}

/// True if a handler body contains a transition() that is guarded by
/// try (`try { transition(...) }`) or sits alongside a require — the
/// shapes that mean "this handler can refuse mid-flight".
fn body_has_guarded_transition(body: &[Spanned<Statement>]) -> bool {
    let mut calls = Vec::new();
    collect_calls(body, &mut calls);
    if !calls.iter().any(|c| c == "transition") {
        return false;
    }
    has_require(body) || has_try_transition_stmts(body)
}

fn has_require(body: &[Spanned<Statement>]) -> bool {
    body.iter().any(|stmt| match &stmt.node {
        Statement::Require { .. } => true,
        Statement::If { then_body, else_body, .. } => {
            has_require(then_body) || has_require(else_body)
        }
        Statement::For { body, .. } | Statement::While { body, .. } => has_require(body),
        _ => false,
    })
}

fn has_try_transition_stmts(body: &[Spanned<Statement>]) -> bool {
    fn expr_has(expr: &Expr) -> bool {
        match expr {
            Expr::Try(inner) | Expr::TryPropagate(inner) => {
                let mut calls = Vec::new();
                collect_calls_expr(&inner.node, &mut calls);
                calls.iter().any(|c| c == "transition") || expr_has(&inner.node)
            }
            Expr::FnCall { args, .. } => args.iter().any(|a| expr_has(&a.node)),
            Expr::MethodCall { target, args, .. } => {
                expr_has(&target.node) || args.iter().any(|a| expr_has(&a.node))
            }
            Expr::FieldAccess { target, .. } => expr_has(&target.node),
            Expr::BinaryOp { left, right, .. }
            | Expr::CmpOp { left, right, .. }
            | Expr::Pipe { left, right } => expr_has(&left.node) || expr_has(&right.node),
            Expr::Not(inner) => expr_has(&inner.node),
            Expr::ListLiteral(items) => items.iter().any(|i| expr_has(&i.node)),
            Expr::Record { fields, .. } => fields.iter().any(|(_, v)| expr_has(&v.node)),
            Expr::Lambda { body, .. } => expr_has(&body.node),
            Expr::LambdaBlock { stmts, result, .. } => {
                stmts_have(stmts) || expr_has(&result.node)
            }
            Expr::Match { subject, arms } => {
                expr_has(&subject.node)
                    || arms.iter().any(|arm| {
                        arm.guard.as_ref().map_or(false, |g| expr_has(&g.node))
                            || stmts_have(&arm.body)
                            || expr_has(&arm.result.node)
                    })
            }
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                expr_has(&condition.node)
                    || stmts_have(then_body)
                    || expr_has(&then_result.node)
                    || stmts_have(else_body)
                    || expr_has(&else_result.node)
            }
            Expr::Literal(_) | Expr::Ident(_) => false,
        }
    }
    fn stmts_have(body: &[Spanned<Statement>]) -> bool {
        body.iter().any(|stmt| match &stmt.node {
            Statement::Let { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Return { value }
            | Statement::Ensure { condition: value } => expr_has(&value.node),
            Statement::ExprStmt { expr } => expr_has(&expr.node),
            Statement::If { condition, then_body, else_body } => {
                expr_has(&condition.node) || stmts_have(then_body) || stmts_have(else_body)
            }
            Statement::For { iter, body, .. } => expr_has(&iter.node) || stmts_have(body),
            Statement::While { condition, body, .. } => {
                expr_has(&condition.node) || stmts_have(body)
            }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => {
                args.iter().any(|a| expr_has(&a.node))
            }
            Statement::Require { .. } | Statement::Break | Statement::Continue => false,
        })
    }
    stmts_have(body)
}
