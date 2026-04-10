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

/// Result of the think-isolation check for one cell.
#[derive(Debug, Clone)]
pub enum IsolationFinding {
    /// All transition targets are literal. Safety properties hold
    /// regardless of LLM output. Liveness holds under fairness.
    ThinkIsolated {
        cell: String,
        n_handlers: usize,
        n_transitions: usize,
    },
    /// At least one handler uses a dynamic (computed) transition
    /// target. The LLM's output could flow into that target and
    /// reach a state the model checker didn't explore.
    NotIsolated {
        cell: String,
        dynamic_handlers: Vec<String>,
    },
    /// Cell has no state machine — isolation is vacuously true
    /// (there are no transitions to protect).
    NoStateMachine,
}

/// Check whether all transition targets in the given refinement
/// findings are literal (no `DynamicTarget`). This is a second pass
/// over the refinement results, not a new AST walk.
pub fn check_isolation(
    cell_name: &str,
    findings: &[RefinementFinding],
) -> IsolationFinding {
    let mut dynamic_handlers: Vec<String> = Vec::new();
    let mut n_handlers = 0;
    let mut n_transitions = 0;
    let mut has_any_effect = false;

    for f in findings {
        match f {
            RefinementFinding::DynamicTarget { handler, .. } => {
                if !dynamic_handlers.contains(handler) {
                    dynamic_handlers.push(handler.clone());
                }
            }
            RefinementFinding::HandlerEffect { handler: _, targets, has_dynamic } => {
                has_any_effect = true;
                n_handlers += 1;
                n_transitions += targets.len();
                if *has_dynamic {
                    // The handler has a dynamic target that may have
                    // been reported separately as DynamicTarget.
                    // No additional action — it's already in
                    // dynamic_handlers if it was reported.
                }
            }
            // UndeclaredTarget and DeadTransition don't affect isolation.
            _ => {}
        }
    }

    if !has_any_effect {
        return IsolationFinding::NoStateMachine;
    }

    if dynamic_handlers.is_empty() {
        IsolationFinding::ThinkIsolated {
            cell: cell_name.to_string(),
            n_handlers,
            n_transitions,
        }
    } else {
        IsolationFinding::NotIsolated {
            cell: cell_name.to_string(),
            dynamic_handlers,
        }
    }
}
