//! V1.8: memory-invariant checking.
//!
//! `invariant expr` inside a `memory { }` section declares a property of
//! the DATA: every write to a guarded slot must satisfy it. The runtime
//! enforces it before any `.set()`/`.push()` commits (interpreter
//! check_invariants); this module adds the static side:
//!
//!   1. validate_program — check-time validation of the invariant
//!      expressions themselves (unknown names are check errors, not
//!      silent runtime surprises).
//!   2. verify_program_invariants — a refinement-style proof pass for
//!      `soma verify`: writes of statically-known values (literals,
//!      constant folds, clamp(..) ranges) are PROVEN against constant
//!      bounds; a provably-violating write is a verify FAILURE; everything
//!      else is reported as runtime-checked.
//!
//! Bindings an invariant may use: the section's slot names (each bound to
//! the value being written), `value` (alias), `key`, `size` (entry count
//! after the write), the legacy `_slot_len`/`_slot_name`/`_key`, and
//! builtins.

use crate::ast::*;
use std::collections::HashSet;

use super::names::{builtin_names, suggest};
use super::verify::{VerifyCheck, VerifyResult};

pub struct InvariantIssue {
    pub message: String,
    pub span: Span,
}

const GENERIC_BINDINGS: &[&str] = &[
    "value", "key", "size", "_slot_len", "_slot_name", "_key", "true", "false",
];

/// Check-time validation: every name an invariant references must be
/// resolvable when the runtime evaluates it.
pub fn validate_program(program: &Program) -> Vec<InvariantIssue> {
    let mut issues = Vec::new();
    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.node.sections {
            let Section::Memory(mem) = &section.node else { continue };
            let slot_names: HashSet<&str> =
                mem.slots.iter().map(|s| s.node.name.as_str()).collect();
            for inv in &mem.invariants {
                let mut idents = HashSet::new();
                collect_idents(&inv.node, &mut idents);
                for name in idents {
                    if slot_names.contains(name.as_str())
                        || GENERIC_BINDINGS.contains(&name.as_str())
                    {
                        continue;
                    }
                    let candidates: Vec<String> = slot_names
                        .iter()
                        .map(|s| s.to_string())
                        .chain(GENERIC_BINDINGS.iter().map(|s| s.to_string()))
                        .collect();
                    let suggestion = suggest(&name, candidates.iter())
                        .map(|s| format!(" (did you mean '{}'?)", s))
                        .unwrap_or_default();
                    issues.push(InvariantIssue {
                        message: format!(
                            "memory invariant references unknown name '{name}'{suggestion} — \
                             invariants may use the section's slot names, 'value', 'key', \
                             'size', and builtins"
                        ),
                        span: inv.span,
                    });
                }
                // called functions must be builtins — handlers are not
                // callable from an invariant context
                let mut fns = HashSet::new();
                collect_fn_names(&inv.node, &mut fns);
                for f in fns {
                    if !builtin_names().contains(f.as_str()) {
                        issues.push(InvariantIssue {
                            message: format!(
                                "memory invariant calls '{f}' which is not a builtin — \
                                 invariants run outside handler context and may only call builtins"
                            ),
                            span: inv.span,
                        });
                    }
                }
            }
        }
    }
    issues
}

// ── Static proof pass for `soma verify` ─────────────────────────────

/// What we can know about a written value at check time.
#[derive(Clone, Copy, Debug)]
enum Known {
    Exact(f64),
    Range(f64, f64),
    Unknown,
}

/// Three-valued proof outcome.
#[derive(Clone, Copy, PartialEq)]
enum Proof {
    Holds,
    Violated,
    Unknown,
}

pub fn verify_program_invariants(program: &Program) -> Vec<VerifyResult> {
    let mut results = Vec::new();

    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }

        // invariant → the slots it guards (same scoping rule as runtime)
        let mut guarded: Vec<(Expr, Vec<String>)> = Vec::new();
        for section in &cell.node.sections {
            let Section::Memory(mem) = &section.node else { continue };
            let slot_names: Vec<String> =
                mem.slots.iter().map(|s| s.node.name.clone()).collect();
            for inv in &mem.invariants {
                let mut refs = HashSet::new();
                collect_idents(&inv.node, &mut refs);
                let named: Vec<String> = slot_names
                    .iter()
                    .filter(|n| refs.contains(*n))
                    .cloned()
                    .collect();
                let targets = if named.is_empty() { slot_names.clone() } else { named };
                guarded.push((inv.node.clone(), targets));
            }
        }
        if guarded.is_empty() {
            continue;
        }

        // every write site in every handler: (handler, slot, value expr)
        let mut writes: Vec<(String, String, Expr)> = Vec::new();
        for section in &cell.node.sections {
            if let Section::OnSignal(on) = &section.node {
                collect_writes_stmts(&on.body, &on.signal_name, &mut writes);
            }
        }

        let mut result = VerifyResult {
            machine_name: format!("{}/invariants", cell.node.name),
            states: vec![],
            initial: String::new(),
            terminal_states: vec![],
            transitions: vec![],
            checks: vec![],
        };

        for (inv, targets) in &guarded {
            let inv_text = render_expr(inv);
            let relevant: Vec<&(String, String, Expr)> = writes
                .iter()
                .filter(|(_, slot, _)| targets.contains(slot))
                .collect();
            if relevant.is_empty() {
                result.checks.push(VerifyCheck::Pass(format!(
                    "invariant {inv_text} — no handler writes to guarded slots"
                )));
                continue;
            }
            let mut runtime_checked: Vec<String> = Vec::new();
            for (handler, slot, value_expr) in relevant {
                match prove(inv, slot, static_value(value_expr)) {
                    Proof::Holds => {
                        result.checks.push(VerifyCheck::Pass(format!(
                            "invariant {inv_text} — writer '{handler}' proven (writes {})",
                            render_expr(value_expr)
                        )));
                    }
                    Proof::Violated => {
                        result.checks.push(VerifyCheck::Fail(
                            format!(
                                "invariant {inv_text} — writer '{handler}' writes {} to '{slot}': statically violated",
                                render_expr(value_expr)
                            ),
                            None,
                        ));
                    }
                    Proof::Unknown => {
                        let tag = format!("{handler} → {slot}");
                        if !runtime_checked.contains(&tag) {
                            runtime_checked.push(tag);
                        }
                    }
                }
            }
            if !runtime_checked.is_empty() {
                result.checks.push(VerifyCheck::Warning(format!(
                    "invariant {inv_text} — runtime-checked (computed values): {}",
                    runtime_checked.join(", ")
                )));
            }
        }

        results.push(result);
    }

    results
}

/// Constant-fold a value expression where possible.
fn static_value(expr: &Expr) -> Known {
    match expr {
        Expr::Literal(Literal::Int(n)) => Known::Exact(*n as f64),
        Expr::Literal(Literal::Float(f)) => Known::Exact(*f),
        Expr::BinaryOp { left, op, right } => {
            let (Known::Exact(a), Known::Exact(b)) =
                (static_value(&left.node), static_value(&right.node))
            else {
                return Known::Unknown;
            };
            match op {
                BinOp::Add => Known::Exact(a + b),
                BinOp::Sub => Known::Exact(a - b),
                BinOp::Mul => Known::Exact(a * b),
                BinOp::Div if b != 0.0 => Known::Exact(a / b),
                _ => Known::Unknown,
            }
        }
        // clamp(x, lo, hi) with constant bounds → range [lo, hi]
        Expr::FnCall { name, args } if name == "clamp" && args.len() == 3 => {
            let (Known::Exact(lo), Known::Exact(hi)) =
                (static_value(&args[1].node), static_value(&args[2].node))
            else {
                return Known::Unknown;
            };
            if lo <= hi { Known::Range(lo, hi) } else { Known::Unknown }
        }
        _ => Known::Unknown,
    }
}

/// Prove an invariant against a known written value. Supports the
/// shapes that cover real risk limits: comparisons of the slot value
/// (optionally through abs()) against constants, joined with &&.
fn prove(inv: &Expr, slot: &str, value: Known) -> Proof {
    match inv {
        Expr::BinaryOp { left, op: BinOp::And, right } => {
            let a = prove(&left.node, slot, value);
            let b = prove(&right.node, slot, value);
            if a == Proof::Violated || b == Proof::Violated {
                Proof::Violated
            } else if a == Proof::Holds && b == Proof::Holds {
                Proof::Holds
            } else {
                Proof::Unknown
            }
        }
        Expr::CmpOp { left, op, right } => {
            // normalize to subject(value) OP constant
            let (subject, op, bound) = match (subject_of(&left.node, slot), const_of(&right.node)) {
                (Some(s), Some(c)) => (s, *op, c),
                _ => match (const_of(&left.node), subject_of(&right.node, slot)) {
                    (Some(c), Some(s)) => (s, flip(*op), c),
                    _ => return Proof::Unknown,
                },
            };
            let range = match value {
                Known::Exact(v) => (v, v),
                Known::Range(lo, hi) => (lo, hi),
                Known::Unknown => return Proof::Unknown,
            };
            // apply abs() to the written-value range if the subject does
            let (lo, hi) = if subject {
                // abs over [lo, hi]
                let alo = if range.0 <= 0.0 && range.1 >= 0.0 {
                    0.0
                } else {
                    range.0.abs().min(range.1.abs())
                };
                (alo, range.0.abs().max(range.1.abs()))
            } else {
                range
            };
            let holds_at = |v: f64| match op {
                CmpOp::Lt => v < bound,
                CmpOp::Gt => v > bound,
                CmpOp::Le => v <= bound,
                CmpOp::Ge => v >= bound,
                CmpOp::Eq => v == bound,
                CmpOp::Ne => v != bound,
            };
            // monotone predicates: checking the endpoints decides the range
            let (at_lo, at_hi) = (holds_at(lo), holds_at(hi));
            match (at_lo, at_hi) {
                (true, true) if op != CmpOp::Ne && op != CmpOp::Eq => Proof::Holds,
                (false, false) if op != CmpOp::Ne && op != CmpOp::Eq => Proof::Violated,
                _ if lo == hi => {
                    if at_lo { Proof::Holds } else { Proof::Violated }
                }
                _ => Proof::Unknown,
            }
        }
        _ => Proof::Unknown,
    }
}

/// Is this expression the guarded subject? Returns Some(true) for
/// abs(slot|value), Some(false) for a bare slot|value reference.
fn subject_of(expr: &Expr, slot: &str) -> Option<bool> {
    match expr {
        Expr::Ident(n) if n == slot || n == "value" => Some(false),
        Expr::FnCall { name, args } if name == "abs" && args.len() == 1 => {
            match &args[0].node {
                Expr::Ident(n) if n == slot || n == "value" => Some(true),
                _ => None,
            }
        }
        _ => None,
    }
}

fn const_of(expr: &Expr) -> Option<f64> {
    match static_value(expr) {
        Known::Exact(v) => Some(v),
        _ => None,
    }
}

fn flip(op: CmpOp) -> CmpOp {
    match op {
        CmpOp::Lt => CmpOp::Gt,
        CmpOp::Gt => CmpOp::Lt,
        CmpOp::Le => CmpOp::Ge,
        CmpOp::Ge => CmpOp::Le,
        other => other,
    }
}

// ── Write-site collection ───────────────────────────────────────────

fn collect_writes_stmts(stmts: &[Spanned<Statement>], handler: &str, out: &mut Vec<(String, String, Expr)>) {
    for stmt in stmts {
        match &stmt.node {
            Statement::Let { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Return { value } => collect_writes_expr(&value.node, handler, out),
            Statement::ExprStmt { expr } => collect_writes_expr(&expr.node, handler, out),
            Statement::If { condition, then_body, else_body } => {
                collect_writes_expr(&condition.node, handler, out);
                collect_writes_stmts(then_body, handler, out);
                collect_writes_stmts(else_body, handler, out);
            }
            Statement::While { condition, body, .. } => {
                collect_writes_expr(&condition.node, handler, out);
                collect_writes_stmts(body, handler, out);
            }
            Statement::For { iter, body, .. } => {
                collect_writes_expr(&iter.node, handler, out);
                collect_writes_stmts(body, handler, out);
            }
            _ => {}
        }
    }
}

fn collect_writes_expr(expr: &Expr, handler: &str, out: &mut Vec<(String, String, Expr)>) {
    match expr {
        Expr::MethodCall { target, method, args } => {
            if let Expr::Ident(slot) = &target.node {
                if matches!(method.as_str(), "set" | "put") && args.len() >= 2 {
                    out.push((handler.to_string(), slot.clone(), args[1].node.clone()));
                }
                if matches!(method.as_str(), "push" | "append") && !args.is_empty() {
                    out.push((handler.to_string(), slot.clone(), args[0].node.clone()));
                }
            }
            collect_writes_expr(&target.node, handler, out);
            for a in args {
                collect_writes_expr(&a.node, handler, out);
            }
        }
        Expr::FnCall { args, .. } => {
            for a in args {
                collect_writes_expr(&a.node, handler, out);
            }
        }
        Expr::FieldAccess { target, .. } => collect_writes_expr(&target.node, handler, out),
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            collect_writes_expr(&left.node, handler, out);
            collect_writes_expr(&right.node, handler, out);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => {
            collect_writes_expr(&i.node, handler, out)
        }
        Expr::ListLiteral(items) => {
            for i in items {
                collect_writes_expr(&i.node, handler, out);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                collect_writes_expr(&v.node, handler, out);
            }
        }
        Expr::Lambda { body, .. } => collect_writes_expr(&body.node, handler, out),
        Expr::LambdaBlock { stmts, result, .. } => {
            collect_writes_stmts(stmts, handler, out);
            collect_writes_expr(&result.node, handler, out);
        }
        Expr::Match { subject, arms } => {
            collect_writes_expr(&subject.node, handler, out);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_writes_expr(&g.node, handler, out);
                }
                collect_writes_stmts(&arm.body, handler, out);
                collect_writes_expr(&arm.result.node, handler, out);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            collect_writes_expr(&condition.node, handler, out);
            collect_writes_stmts(then_body, handler, out);
            collect_writes_expr(&then_result.node, handler, out);
            collect_writes_stmts(else_body, handler, out);
            collect_writes_expr(&else_result.node, handler, out);
        }
        _ => {}
    }
}

// ── Small AST utilities ─────────────────────────────────────────────

fn collect_idents(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::Ident(n) => {
            out.insert(n.clone());
        }
        Expr::FieldAccess { target, .. } => collect_idents(&target.node, out),
        Expr::MethodCall { target, args, .. } => {
            collect_idents(&target.node, out);
            for a in args {
                collect_idents(&a.node, out);
            }
        }
        Expr::FnCall { args, .. } => {
            for a in args {
                collect_idents(&a.node, out);
            }
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            collect_idents(&left.node, out);
            collect_idents(&right.node, out);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => collect_idents(&i.node, out),
        Expr::ListLiteral(items) => {
            for i in items {
                collect_idents(&i.node, out);
            }
        }
        _ => {}
    }
}

fn collect_fn_names(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::FnCall { name, args } => {
            out.insert(name.clone());
            for a in args {
                collect_fn_names(&a.node, out);
            }
        }
        Expr::FieldAccess { target, .. } => collect_fn_names(&target.node, out),
        Expr::MethodCall { target, args, .. } => {
            collect_fn_names(&target.node, out);
            for a in args {
                collect_fn_names(&a.node, out);
            }
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            collect_fn_names(&left.node, out);
            collect_fn_names(&right.node, out);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => collect_fn_names(&i.node, out),
        Expr::ListLiteral(items) => {
            for i in items {
                collect_fn_names(&i.node, out);
            }
        }
        _ => {}
    }
}
