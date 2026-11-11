//! 3.0: face contracts — `requires` / `ensures` on a signal.
//!
//!     face {
//!         signal debit(amount: Int) -> Int
//!             requires amount > 0
//!             ensures result >= 0
//!     }
//!
//! A `requires` is what a caller owes, an `ensures` what the handler owes.
//! The interpreter evaluates both around every call (kinds `requires` and
//! `ensures`), so a contract is true of every call that returns — which is
//! what lets `soma verify` use it across the cell boundary: a caller in
//! another cell reads `ensures` as the range of the result without looking
//! at the handler, and the handler reads `requires` as a fact about its
//! parameters without looking at its callers.
//!
//! This module holds the check-time rules (what a clause may say) and the
//! rewriting the prover works on (`inline_requires`). The proof itself is
//! in `invariants.rs`, beside the interval domain it uses.

use crate::ast::*;
use std::collections::HashSet;

pub struct ContractIssue {
    pub message: String,
    pub span: Span,
}

/// Builtins a clause may not call: a contract is evaluated on every call and
/// must give the same answer to the caller, the handler and the prover.
fn impure(name: &str) -> bool {
    matches!(name, "think" | "think_json" | "vote" | "delegate" | "transition" | "get_status" | "has_state"
        | "print" | "println" | "now" | "now_ms" | "today" | "random" | "rand" | "uuid" | "read_stdin"
        | "set_budget" | "approve" | "sleep" | "emit" | "fail" | "trace" | "env")
        || ["http_", "read_", "write_", "file_", "sql_", "db_"].iter().any(|p| name.starts_with(p))
}

/// The signals of `cell` that carry a contract, with the handler they bind.
pub(crate) fn contracted<'a>(cell: &'a CellDef) -> Vec<(&'a Spanned<FaceDecl>, &'a SignalDecl, Option<&'a OnSection>)> {
    let mut out = Vec::new();
    for section in &cell.sections {
        let Section::Face(face) = &section.node else { continue };
        for d in &face.declarations {
            let FaceDecl::Signal(sig) = &d.node else { continue };
            if sig.requires.is_empty() && sig.ensures.is_empty() { continue; }
            let on = cell.sections.iter().find_map(|s| match &s.node {
                Section::OnSignal(on) if on.signal_name == sig.name => Some(on),
                _ => None,
            });
            out.push((d, sig, on));
        }
    }
    out
}

/// Check-time rules. A clause names the signal's parameters (and `result`
/// in an `ensures`), literals and pure builtins — nothing a caller in
/// another cell could not evaluate.
pub fn check_program(program: &Program) -> Vec<ContractIssue> {
    let mut issues = Vec::new();
    let builtins = super::names::builtin_names();
    for cell in super::names::collect_cells(program) {
        let handlers: HashSet<&str> = cell.sections.iter().filter_map(|s| match &s.node {
            Section::OnSignal(on) => Some(on.signal_name.as_str()),
            _ => None,
        }).collect();
        for (decl, sig, on) in contracted(cell) {
            // (a signal without a handler is its own error)
            let Some(on) = on else { continue };
            if on.properties.iter().any(|p| p == "native") {
                issues.push(ContractIssue {
                    message: format!("signal '{}' has a face contract, but its handler is `[native]`: compiled code calls its siblings directly, past the contract — state it with `require` in the interpreted caller, or drop `[native]`", sig.name),
                    span: decl.span,
                });
                continue;
            }
            if let Some(i) = (0..sig.params.len().min(on.params.len())).find(|i| sig.params[*i].name != on.params[*i].name) {
                issues.push(ContractIssue {
                    message: format!("the contract of '{}' names the face's parameters, but the handler calls parameter {} `{}` (the face: `{}`) — use one name",
                        sig.name, i + 1, on.params[i].name, sig.params[i].name),
                    span: decl.span,
                });
                continue;
            }
            let params: HashSet<&str> = sig.params.iter().map(|p| p.name.as_str()).collect();
            for (clause, is_ensures) in sig.requires.iter().map(|c| (c, false)).chain(sig.ensures.iter().map(|c| (c, true))) {
                let word = if is_ensures { "ensures" } else { "requires" };
                let mut names: Vec<String> = super::invariants::deep_idents(&clause.node).into_iter().collect();
                names.sort();
                for n in names {
                    if params.contains(n.as_str()) || n == "true" || n == "false" || (is_ensures && n == "result") { continue; }
                    let why = if n == "result" {
                        "`result` is the returned value: it exists in `ensures` only".to_string()
                    } else {
                        format!("a clause names the signal's parameters ({}){} — a slot or a local belongs in the handler, as a `require`",
                            if params.is_empty() { "none here".to_string() } else { sig.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ") },
                            if is_ensures { " and `result`" } else { "" })
                    };
                    issues.push(ContractIssue {
                        message: format!("`{} {}` on signal '{}' reads `{}`: {}", word, render_expr(&clause.node), sig.name, n, why),
                        span: clause.span,
                    });
                }
                let mut calls: Vec<String> = Vec::new();
                super::desugar::for_each_deep(&clause.node, &mut |e| if let Expr::FnCall { name, .. } = e { calls.push(name.clone()); });
                calls.sort();
                calls.dedup();
                for name in calls {
                    // (`_coalesce` and friends: what an operator desugars to)
                    if name.starts_with('_') { continue; }
                    let reason = if handlers.contains(name.as_str()) {
                        "a handler"
                    } else if !builtins.contains(name.as_str()) {
                        "which is not a builtin"
                    } else if impure(&name) {
                        "a builtin with an effect or a changing answer"
                    } else { continue };
                    issues.push(ContractIssue {
                        message: format!("`{} {}` on signal '{}' calls `{}()`, {}: a contract is evaluated on every call, by the caller's proof as well — it may only compare the parameters{} with pure builtins",
                            word, render_expr(&clause.node), sig.name, name, reason, if is_ensures { " and `result`" } else { "" }),
                        span: clause.span,
                    });
                }
            }
        }
    }
    issues
}

/// The program the prover reads: each `requires` of a face becomes a
/// `require` at the top of its handler — the interpreter has checked it
/// before the body runs, so it is a fact for every statement there.
/// (Signals whose contract `check` refuses are left as they are.)
pub fn inline_requires(program: &Program) -> Program {
    let mut program = program.clone();
    let refused: HashSet<usize> = check_program(&program).iter().map(|i| i.span.start).collect();
    fn rewrite(cell: &mut CellDef, refused: &HashSet<usize>) {
        let mut inject: Vec<(String, Vec<Spanned<Expr>>)> = Vec::new();
        for (decl, sig, on) in contracted(cell) {
            if on.is_none() || sig.requires.is_empty() { continue; }
            if refused.contains(&decl.span.start) || sig.requires.iter().chain(sig.ensures.iter()).any(|c| refused.contains(&c.span.start)) { continue; }
            inject.push((sig.name.clone(), sig.requires.clone()));
        }
        for section in cell.sections.iter_mut() {
            match &mut section.node {
                Section::OnSignal(on) => {
                    let Some((_, clauses)) = inject.iter().find(|(n, _)| *n == on.signal_name) else { continue };
                    let mut body: Vec<Spanned<Statement>> = clauses.iter().map(|c| Spanned::new(Statement::Require {
                        constraint: Spanned::new(Constraint::Comparison {
                            left: c.clone(),
                            op: CmpOp::Eq,
                            right: Spanned::new(Expr::Literal(Literal::Bool(true)), c.span),
                        }, c.span),
                        else_signal: "requires".to_string(),
                    }, c.span)).collect();
                    body.append(&mut on.body);
                    on.body = body;
                }
                Section::Interior(interior) => {
                    for inner in interior.cells.iter_mut() { rewrite(&mut inner.node, refused); }
                }
                _ => {}
            }
        }
    }
    for cell in program.cells.iter_mut() { rewrite(&mut cell.node, &refused); }
    program
}
