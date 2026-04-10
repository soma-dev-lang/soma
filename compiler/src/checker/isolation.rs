//! Think-isolation check: proves that CTL safety properties hold
//! regardless of what the LLM returns.
//!
//! ## The theorem (informal)
//!
//! For a cell C where every `transition()` call uses a literal target
//! (i.e., no `DynamicTarget` finding from the refinement checker):
//!
//!   - **Safety properties** (`Always`, `Never`, `DeadlockFree`,
//!     `Mutex`) hold for C under ANY oracle substituted for `think()`.
//!     The LLM can influence which branch is taken, but every branch
//!     leads to a state in G(M), and the model checker has verified
//!     the properties for ALL states in G(M).
//!
//!   - **Liveness properties** (`Eventually`, `After`) hold under a
//!     **fairness assumption**: the oracle must eventually produce a
//!     response that allows each handler to reach at least one of its
//!     transition calls. Without this, the LLM can stall the machine
//!     by never triggering a transition.
//!
//! ## Why this works
//!
//! The proof chain:
//!   1. Refinement (V1.3) ensures every literal target is in States(M).
//!   2. Runtime fidelity ensures every actual transition is in →.
//!   3. The model checker verifies CTL properties for ALL paths in G(M).
//!   4. The actual execution is one of those paths.
//!   5. Therefore: safety holds unconditionally, liveness holds under
//!      fairness.
//!
//! The key insight: `think()` can influence *which* path through G(M)
//! the execution takes, but it cannot take the execution *outside*
//! G(M). Since the model checker proves properties for ALL paths,
//! the specific path chosen by the oracle doesn't matter for safety.
//!
//! ## Scope
//!
//! This is a **per-cell** property. If cell A passes think() output
//! to cell B via `delegate()`, and cell B uses it as a dynamic
//! transition target, the isolation breaks for B even if A is
//! isolated. Cross-cell information flow is tracked in ADVERSARIES.md
//! Gap G3.
//!
//! Mechanized in: `docs/rigor/coq/Soma_Isolation.v`.
//! Full theorem: `docs/SOUNDNESS.md` §3.6.

use super::refinement::RefinementFinding;
use crate::ast::*;

/// Result of the think-isolation check for one cell.
#[derive(Debug, Clone)]
pub enum IsolationFinding {
    /// All transition targets are literal AND no tool handler calls
    /// transition(). Safety properties hold regardless of LLM output.
    /// Liveness holds under fairness.
    ThinkIsolated {
        cell: String,
        n_handlers: usize,
        n_transitions: usize,
    },
    /// At least one handler uses a dynamic (computed) transition
    /// target, OR a tool handler calls transition() (the LLM can
    /// invoke tools during think(), triggering transitions that the
    /// model checker doesn't account for in the calling handler's
    /// effect summary).
    NotIsolated {
        cell: String,
        reasons: Vec<String>,
    },
    /// Cell has no state machine — isolation is vacuously true
    /// (there are no transitions to protect).
    NoStateMachine,
}

/// Collect tool names declared in a cell's face section.
fn collect_tool_names(cell: &CellDef) -> Vec<String> {
    let mut tools = Vec::new();
    for section in &cell.sections {
        if let Section::Face(face) = &section.node {
            for decl in &face.declarations {
                if let FaceDecl::Tool(tool) = &decl.node {
                    tools.push(tool.name.clone());
                }
            }
        }
    }
    tools
}

/// Check if a handler body contains any transition() call.
fn handler_has_transition(stmts: &[Spanned<Statement>]) -> bool {
    for stmt in stmts {
        match &stmt.node {
            Statement::ExprStmt { expr } | Statement::Let { value: expr, .. }
            | Statement::Assign { value: expr, .. } | Statement::Return { value: expr } => {
                if expr_has_transition(&expr.node) { return true; }
            }
            Statement::If { then_body, else_body, .. } => {
                if handler_has_transition(then_body) || handler_has_transition(else_body) {
                    return true;
                }
            }
            Statement::For { body, .. } | Statement::While { body, .. } => {
                if handler_has_transition(body) { return true; }
            }
            _ => {}
        }
    }
    false
}

fn expr_has_transition(expr: &Expr) -> bool {
    match expr {
        Expr::FnCall { name, .. } if name == "transition" => true,
        Expr::FnCall { args, .. } => args.iter().any(|a| expr_has_transition(&a.node)),
        Expr::Match { arms, subject } => {
            expr_has_transition(&subject.node)
            || arms.iter().any(|arm| {
                handler_has_transition(&arm.body)
                || expr_has_transition(&arm.result.node)
            })
        }
        Expr::IfExpr { then_body, else_body, then_result, else_result, .. } => {
            handler_has_transition(then_body)
            || handler_has_transition(else_body)
            || expr_has_transition(&then_result.node)
            || expr_has_transition(&else_result.node)
        }
        Expr::Pipe { left, right } => {
            expr_has_transition(&left.node) || expr_has_transition(&right.node)
        }
        _ => false,
    }
}

/// Check whether all transition targets in the given refinement
/// findings are literal (no `DynamicTarget`) AND no tool handler
/// contains a transition() call.
///
/// The tool-handler check addresses a soundness gap discovered by
/// adversarial review: the LLM can invoke tools during think(),
/// and if a tool handler calls transition(), the LLM controls
/// state-machine transitions through the tool-calling side channel.
pub fn check_isolation(
    cell_name: &str,
    cell: &CellDef,
    findings: &[RefinementFinding],
) -> IsolationFinding {
    let mut reasons: Vec<String> = Vec::new();
    let mut n_handlers = 0;
    let mut n_transitions = 0;
    let mut has_any_effect = false;

    // Check 1: no dynamic transition targets.
    for f in findings {
        match f {
            RefinementFinding::DynamicTarget { handler, .. } => {
                let reason = format!(
                    "handler `{}` uses a dynamic (computed) transition target", handler);
                if !reasons.contains(&reason) {
                    reasons.push(reason);
                }
            }
            RefinementFinding::HandlerEffect { targets, has_dynamic, .. } => {
                has_any_effect = true;
                n_handlers += 1;
                n_transitions += targets.len();
                let _ = has_dynamic;
            }
            _ => {}
        }
    }

    if !has_any_effect {
        return IsolationFinding::NoStateMachine;
    }

    // Check 2: no tool handler calls transition().
    // Tools are declared in `face { tool X(...) "desc" }`. The LLM
    // can invoke them during think(). If a tool's handler calls
    // transition(), the LLM controls state-machine transitions.
    let tool_names = collect_tool_names(cell);
    for tool_name in &tool_names {
        // Find the handler for this tool.
        for section in &cell.sections {
            if let Section::OnSignal(on) = &section.node {
                if on.signal_name == *tool_name {
                    if handler_has_transition(&on.body) {
                        reasons.push(format!(
                            "tool `{}` has a handler that calls transition() — LLM can trigger state changes via tool calling during think()",
                            tool_name
                        ));
                    }
                }
            }
        }
    }

    if reasons.is_empty() {
        IsolationFinding::ThinkIsolated {
            cell: cell_name.to_string(),
            n_handlers,
            n_transitions,
        }
    } else {
        IsolationFinding::NotIsolated {
            cell: cell_name.to_string(),
            reasons,
        }
    }
}
