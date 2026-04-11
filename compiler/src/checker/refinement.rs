//! Refinement checker — proves that handler bodies refine the state machine.
//!
//! ## The gap this closes
//!
//! Soma's tagline is *"the specification is the program"*. The state block
//! is the spec; the handler bodies are supposed to implement it. But before
//! this module landed, `soma verify` only read the state block — it proved
//! CTL properties about a *picture* of the state machine and trusted that
//! the handler bodies actually called `transition()` consistently with the
//! picture. They could lie. Today they can't.
//!
//! ## What this module proves (V1.3 — sound, syntactic)
//!
//!  1. **No undeclared transition targets.** Every `transition("inst", "X")`
//!     call in any handler body must name a state `X` that exists in the
//!     cell's `state { }` block. If a handler tries to transition to
//!     `"completed"` but the state machine only has `{pending, authorized,
//!     captured, settled, refunded}`, the compiler rejects with file:line.
//!
//!  2. **No dead transitions in the spec.** Every transition `S → T`
//!     declared in the state block must be reached by at least one
//!     `transition()` call from some handler (with `T` as the target).
//!     Otherwise the spec is lying about what the program does — emitted
//!     as a warning, not an error, since the spec might be aspirational.
//!
//!  3. **Per-handler effect summary.** For each handler, we compute the
//!     set of target states it can transition to, along with the path
//!     condition leading to each call (the if/else guards on the path).
//!     Surfaced as info in `soma verify` output: "handler `authorize`
//!     transitions to {authorized [if amount > 0]}".
//!
//! ## What this does NOT prove (V1.4+ work, honestly scoped)
//!
//!  - **Source-state correctness.** A handler doesn't know which state
//!     the machine was in when it was called — that's runtime information.
//!     We only check the *target* of every transition() call, not that
//!     the call is legal *from the current state*. Catching that requires
//!     SMT-backed symbolic execution of the surrounding control flow,
//!     deferred to V1.4.
//!
//!  - **Guard implication.** If the state block says `pending → authorized
//!     when amount > 0` and the handler writes `if amount <= 0 { return }
//!     transition("t", "authorized")`, V1.3 doesn't try to prove that the
//!     handler's path condition implies the state-machine's guard — it
//!     just records both as text. SMT integration is V1.4.
//!
//!  - **Dynamic targets.** `transition(id, target_var)` where `target_var`
//!     is computed at runtime: V1.3 records `has_dynamic_target = true` and
//!     emits a warning that this handler's effect can't be statically
//!     analyzed. Better than silently passing.
//!
//! These are intentional. V1.3 is the *syntactic* refinement check —
//! sound (no false positives), incomplete (some real bugs slip through if
//! they hide behind dynamic targets or guard arithmetic). The
//! incompleteness is documented per-handler in the verifier output so the
//! user can see exactly which handlers got the strong check and which
//! ones got the weak one.

use crate::ast::*;
use std::collections::HashSet;

/// One refinement-related finding.
#[derive(Debug, Clone)]
pub enum RefinementFinding {
    /// A handler calls `transition()` with a literal target state that
    /// is not declared anywhere in the cell's state machine. **Hard error.**
    UndeclaredTarget {
        handler: String,
        target: String,
        path: Vec<String>,
        span: Span,
    },
    /// A handler calls `transition()` with a non-literal target.
    /// V1.3 cannot statically verify it; emitted as a warning so the user
    /// knows refinement coverage is incomplete here.
    DynamicTarget {
        handler: String,
        span: Span,
    },
    /// A transition declared in the state block is never reached by any
    /// handler. The spec might be aspirational; emitted as a warning.
    DeadTransition {
        from: String,
        to: String,
    },
    /// Per-handler effect summary — informational. Shows the user which
    /// states each handler can transition to and under what path condition.
    HandlerEffect {
        handler: String,
        targets: Vec<TransitionCall>,
        has_dynamic: bool,
    },
}

/// One transition-call site inside a handler body, with the path condition
/// (chain of `if` guards) leading to it. The path is rendered as text in V1.3
/// — no semantic interpretation, no SMT.
#[derive(Debug, Clone)]
pub struct TransitionCall {
    pub target: String,
    pub path: Vec<String>,
    pub span: Span,
}

/// Effect of a single handler: every static transition() call we found.
#[derive(Debug, Clone)]
pub struct HandlerEffect {
    pub name: String,
    pub span: Span,
    pub static_transitions: Vec<TransitionCall>,
    pub has_dynamic_target: bool,
}

/// Run the refinement check for one (state machine, handlers) pair.
/// Returns the set of findings; the caller decides how to render them.
pub fn check_refinement(
    sm: &StateMachineSection,
    handlers: &[(&OnSection, Span)],
) -> Vec<RefinementFinding> {
    let mut findings = Vec::new();

    // Collect the set of valid state names from the state machine.
    // Includes the initial state, every `from` (except wildcard `*`),
    // and every `to`. This is the universe of legal transition() targets.
    let mut valid_states: HashSet<String> = HashSet::new();
    valid_states.insert(sm.initial.clone());
    for t in &sm.transitions {
        if t.node.from != "*" {
            valid_states.insert(t.node.from.clone());
        }
        valid_states.insert(t.node.to.clone());
    }

    // For each handler, walk the body and extract its effect.
    let mut effects: Vec<HandlerEffect> = Vec::with_capacity(handlers.len());
    for (h, span) in handlers {
        let effect = analyze_handler(h, *span);
        effects.push(effect);
    }

    // ── Check 1: every static transition() target must be a declared state
    for eff in &effects {
        for call in &eff.static_transitions {
            if !valid_states.contains(&call.target) {
                findings.push(RefinementFinding::UndeclaredTarget {
                    handler: eff.name.clone(),
                    target: call.target.clone(),
                    path: call.path.clone(),
                    span: call.span,
                });
            }
        }
        // Warn about dynamic targets
        if eff.has_dynamic_target {
            findings.push(RefinementFinding::DynamicTarget {
                handler: eff.name.clone(),
                span: eff.span,
            });
        }
    }

    // ── Check 2: every declared transition's target must be reached by some handler
    let reached_targets: HashSet<&str> = effects.iter()
        .flat_map(|e| e.static_transitions.iter().map(|c| c.target.as_str()))
        .collect();
    // Some handlers have dynamic targets — if any handler has one, we can't
    // be sure dead-transition warnings are accurate, so suppress them.
    let any_dynamic = effects.iter().any(|e| e.has_dynamic_target);
    if !any_dynamic {
        for t in &sm.transitions {
            if !reached_targets.contains(t.node.to.as_str()) {
                findings.push(RefinementFinding::DeadTransition {
                    from: t.node.from.clone(),
                    to: t.node.to.clone(),
                });
            }
        }
    }

    // ── Check 3: emit per-handler effect summary (always, for the reader)
    for eff in effects {
        if !eff.static_transitions.is_empty() || eff.has_dynamic_target {
            findings.push(RefinementFinding::HandlerEffect {
                handler: eff.name.clone(),
                targets: eff.static_transitions.clone(),
                has_dynamic: eff.has_dynamic_target,
            });
        }
    }

    findings
}

/// Walk a handler body and extract every transition() call along with
/// its path condition. The path condition is the stack of `if` guards
/// (rendered as text) leading to the call site.
fn analyze_handler(on: &OnSection, span: Span) -> HandlerEffect {
    let mut effect = HandlerEffect {
        name: on.signal_name.clone(),
        span,
        static_transitions: Vec::new(),
        has_dynamic_target: false,
    };
    let mut path: Vec<String> = Vec::new();
    walk_stmts(&on.body, &mut path, &mut effect);
    effect
}

fn walk_stmts(body: &[Spanned<Statement>], path: &mut Vec<String>, eff: &mut HandlerEffect) {
    for stmt in body {
        walk_stmt(&stmt.node, stmt.span, path, eff);
    }
}

fn walk_stmt(stmt: &Statement, span: Span, path: &mut Vec<String>, eff: &mut HandlerEffect) {
    match stmt {
        Statement::ExprStmt { expr } => {
            walk_expr(&expr.node, span, path, eff);
        }
        Statement::Let { value, .. } | Statement::Assign { value, .. } => {
            walk_expr(&value.node, span, path, eff);
        }
        Statement::Return { value } => {
            walk_expr(&value.node, span, path, eff);
        }
        Statement::Ensure { condition } => {
            walk_expr(&condition.node, span, path, eff);
        }
        Statement::If { condition, then_body, else_body } => {
            // The condition itself can contain a transition (rare but legal).
            walk_expr(&condition.node, span, path, eff);
            // Then-branch: push the condition as text on the path
            let cond_text = render_expr(&condition.node);
            path.push(format!("if {}", cond_text));
            walk_stmts(then_body, path, eff);
            path.pop();
            // Else-branch: push the negated condition
            if !else_body.is_empty() {
                path.push(format!("if not ({})", cond_text));
                walk_stmts(else_body, path, eff);
                path.pop();
            }
        }
        Statement::For { iter, body, .. } => {
            walk_expr(&iter.node, span, path, eff);
            path.push("(in for loop)".to_string());
            walk_stmts(body, path, eff);
            path.pop();
        }
        Statement::While { condition, body, .. } => {
            walk_expr(&condition.node, span, path, eff);
            path.push(format!("while {}", render_expr(&condition.node)));
            walk_stmts(body, path, eff);
            path.pop();
        }
        Statement::Emit { args, .. } => {
            for a in args {
                walk_expr(&a.node, span, path, eff);
            }
        }
        Statement::MethodCall { args, .. } => {
            for a in args {
                walk_expr(&a.node, span, path, eff);
            }
        }
        Statement::Require { .. } | Statement::Break | Statement::Continue => {}
    }
}

fn walk_expr(expr: &Expr, span: Span, path: &mut Vec<String>, eff: &mut HandlerEffect) {
    match expr {
        Expr::FnCall { name, args } if name == "transition" => {
            // Found a transition call. The target is the second arg (index 1).
            // First arg is the instance id; we ignore it for refinement.
            if let Some(target_expr) = args.get(1) {
                if let Expr::Literal(Literal::String(s)) = &target_expr.node {
                    eff.static_transitions.push(TransitionCall {
                        target: s.clone(),
                        path: path.clone(),
                        span,
                    });
                } else {
                    // Dynamic target — flag it but don't try to analyze
                    eff.has_dynamic_target = true;
                }
            }
            // Also walk the args in case there's a nested transition (unlikely)
            for a in args {
                walk_expr(&a.node, span, path, eff);
            }
        }
        Expr::FnCall { args, .. } => {
            for a in args { walk_expr(&a.node, span, path, eff); }
        }
        Expr::MethodCall { target, args, .. } => {
            walk_expr(&target.node, span, path, eff);
            for a in args { walk_expr(&a.node, span, path, eff); }
        }
        Expr::FieldAccess { target, .. } => walk_expr(&target.node, span, path, eff),
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
            walk_expr(&left.node, span, path, eff);
            walk_expr(&right.node, span, path, eff);
        }
        Expr::Not(e) => walk_expr(&e.node, span, path, eff),
        Expr::Try(e) | Expr::TryPropagate(e) => walk_expr(&e.node, span, path, eff),
        Expr::Pipe { left, right } => {
            walk_expr(&left.node, span, path, eff);
            walk_expr(&right.node, span, path, eff);
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            walk_expr(&condition.node, span, path, eff);
            let cond_text = render_expr(&condition.node);
            path.push(format!("if {}", cond_text));
            walk_stmts(then_body, path, eff);
            walk_expr(&then_result.node, span, path, eff);
            path.pop();
            path.push(format!("if not ({})", cond_text));
            walk_stmts(else_body, path, eff);
            walk_expr(&else_result.node, span, path, eff);
            path.pop();
        }
        Expr::Match { subject, arms } => {
            walk_expr(&subject.node, span, path, eff);
            for arm in arms {
                path.push(format!("(match arm {})", render_pattern(&arm.pattern)));
                if let Some(g) = &arm.guard {
                    walk_expr(&g.node, span, path, eff);
                }
                walk_stmts(&arm.body, path, eff);
                walk_expr(&arm.result.node, span, path, eff);
                path.pop();
            }
        }
        Expr::Lambda { body, .. } => walk_expr(&body.node, span, path, eff),
        Expr::LambdaBlock { stmts, result, .. } => {
            walk_stmts(stmts, path, eff);
            walk_expr(&result.node, span, path, eff);
        }
        Expr::Record { fields, .. } => {
            for (_, e) in fields { walk_expr(&e.node, span, path, eff); }
        }
        Expr::ListLiteral(items) => {
            for e in items { walk_expr(&e.node, span, path, eff); }
        }
        Expr::Literal(_) | Expr::Ident(_) => {}
    }
}

/// Render an expression as a short, human-readable text snippet for path
/// conditions. This is **not** a faithful round-trip — it's a hint for
/// the verifier output. SMT integration in V1.4 will replace this with
/// real predicate logic.
fn render_expr(e: &Expr) -> String {
    match e {
        Expr::Literal(Literal::Int(n)) => n.to_string(),
        Expr::Literal(Literal::String(s)) => format!("\"{}\"", s),
        Expr::Literal(Literal::Bool(b)) => b.to_string(),
        Expr::Literal(Literal::Float(f)) => f.to_string(),
        Expr::Literal(Literal::BigInt(s)) => s.clone(),
        Expr::Literal(Literal::Unit) => "()".to_string(),
        Expr::Literal(_) => "<literal>".to_string(),
        Expr::Ident(s) => s.clone(),
        Expr::FieldAccess { target, field } => format!("{}.{}", render_expr(&target.node), field),
        Expr::CmpOp { left, op, right } => format!("{} {} {}", render_expr(&left.node), op, render_expr(&right.node)),
        Expr::BinaryOp { left, op, right } => format!("{} {} {}", render_expr(&left.node), op, render_expr(&right.node)),
        Expr::Not(e) => format!("!{}", render_expr(&e.node)),
        Expr::FnCall { name, args } => {
            let arg_strs: Vec<String> = args.iter().map(|a| render_expr(&a.node)).collect();
            format!("{}({})", name, arg_strs.join(", "))
        }
        Expr::MethodCall { target, method, args } => {
            let arg_strs: Vec<String> = args.iter().map(|a| render_expr(&a.node)).collect();
            format!("{}.{}({})", render_expr(&target.node), method, arg_strs.join(", "))
        }
        _ => "…".to_string(),
    }
}

fn render_pattern(p: &MatchPattern) -> String {
    match p {
        MatchPattern::Literal(Literal::String(s)) => format!("\"{}\"", s),
        MatchPattern::Literal(Literal::Int(n)) => n.to_string(),
        MatchPattern::Literal(Literal::Bool(b)) => b.to_string(),
        MatchPattern::Wildcard => "_".to_string(),
        MatchPattern::Variable(s) => s.clone(),
        _ => "…".to_string(),
    }
}
