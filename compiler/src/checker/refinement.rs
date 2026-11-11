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
    /// 3.0: the handler tested the instance's state (`require get_status(id)
    /// == "held"`, an `if`, a `match`, an earlier transition) and then moves
    /// it to a state the machine has no edge to from there: on that path
    /// the call always raises `invalid_transition`. **Hard error.**
    IllegalEdge {
        handler: String,
        from: String,
        to: String,
        path: Vec<String>,
        span: Span,
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
    /// `transition(id, from, to)`: the source the handler declares
    pub source: Option<String>,
    /// 3.0: the states the instance can be in at the call, when the handler
    /// established them (empty: not known)
    pub inferred: Vec<String>,
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
    /// walker state: instance id (as written) → the states it can be in
    facts: std::collections::HashMap<String, Vec<String>>,
    /// walker state: `let s = get_status(id)` → the id
    aliases: std::collections::HashMap<String, String>,
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

    let has_edge = |from: &str, to: &str| sm.transitions.iter().any(|t| t.node.to == to
        && (t.node.from == from || (t.node.from == "*" && sm.wildcard_applies(&t.node, from))));
    // a source with no edge to the target is a path that raises: the edges
    // the call can take start from the others
    for eff in effects.iter_mut() {
        for call in eff.static_transitions.iter_mut() {
            if call.inferred.iter().any(|s| has_edge(s, &call.target)) {
                call.inferred.retain(|s| has_edge(s, &call.target));
            }
        }
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
        // 3.0: a move from a state the handler itself established. The
        // analysis joins paths without their data conditions (`if a > n {
        // … "passed" }  if a <= n { … "failed" }` reaches the second call
        // knowing "voting or passed"), so only a call NO known source has
        // an edge for is refused: that one raises on every path.
        for call in &eff.static_transitions {
            if call.source.is_some() || !valid_states.contains(&call.target) { continue; }
            let known: Vec<&String> = call.inferred.iter().filter(|s| valid_states.contains(*s)).collect();
            if known.is_empty() || known.iter().any(|from| has_edge(from, &call.target)) { continue; }
            for from in known {
                findings.push(RefinementFinding::IllegalEdge {
                    handler: eff.name.clone(),
                    from: from.clone(),
                    to: call.target.clone(),
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
    // (a call that declares its source reaches only the edge it names)
    let reached = |from: &str, to: &str| effects.iter().any(|e| e.static_transitions.iter().any(|c|
        c.target == to && match (&c.source, c.inferred.is_empty()) {
            (Some(s), _) => s == from || from == "*",
            (None, false) => from == "*" || c.inferred.iter().any(|s| s == from),
            (None, true) => true,
        }));
    // Some handlers have dynamic targets — if any handler has one, we can't
    // be sure dead-transition warnings are accurate, so suppress them.
    let any_dynamic = effects.iter().any(|e| e.has_dynamic_target);
    if !any_dynamic {
        for t in &sm.transitions {
            if !reached(&t.node.from, &t.node.to) {
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
        facts: Default::default(),
        aliases: Default::default(),
    };
    let mut path: Vec<String> = Vec::new();
    walk_stmts(&on.body, &mut path, &mut effect);
    effect
}


// ── 3.0: what the handler knows about an instance's state ────────────────

/// A state written as a literal: `"held"`, or a typed state `Held`.
fn state_literal(e: &Expr) -> Option<String> {
    match e {
        Expr::Literal(Literal::String(s)) if !s.contains('{') => Some(s.clone()),
        Expr::Ident(name) if name.chars().next().map_or(false, |c| c.is_ascii_uppercase()) => Some(name.clone()),
        _ => None,
    }
}

/// `get_status(id)`, or a local bound to it: the id, as written.
fn status_subject(e: &Expr, eff: &HandlerEffect) -> Option<String> {
    match e {
        Expr::FnCall { name, args } if name == "get_status" && args.len() == 1 => Some(render_expr(&args[0].node)),
        Expr::Ident(n) => eff.aliases.get(n).cloned(),
        _ => None,
    }
}

/// (id, states) a TRUE condition establishes: `get_status(id) == "a"`,
/// `… || get_status(id) == "b"`.
fn fact_when_true(e: &Expr, eff: &HandlerEffect) -> Option<(String, Vec<String>)> {
    match e {
        Expr::CmpOp { left, op: CmpOp::Eq, right } => {
            if let (Some(id), Some(st)) = (status_subject(&left.node, eff), state_literal(&right.node)) { return Some((id, vec![st])); }
            if let (Some(st), Some(id)) = (state_literal(&left.node), status_subject(&right.node, eff)) { return Some((id, vec![st])); }
            None
        }
        Expr::BinaryOp { left, op: BinOp::Or, right } => {
            let (a, b) = (fact_when_true(&left.node, eff)?, fact_when_true(&right.node, eff)?);
            if a.0 != b.0 { return None; }
            let mut states = a.1;
            for st in b.1 { if !states.contains(&st) { states.push(st); } }
            Some((a.0, states))
        }
        // either conjunct alone is enough
        Expr::BinaryOp { left, op: BinOp::And, right } => fact_when_true(&left.node, eff).or_else(|| fact_when_true(&right.node, eff)),
        Expr::Not(inner) => fact_when_false(&inner.node, eff),
        _ => None,
    }
}

/// (id, states) a FALSE condition establishes: `get_status(id) != "a"`,
/// `… && get_status(id) != "b"`.
fn fact_when_false(e: &Expr, eff: &HandlerEffect) -> Option<(String, Vec<String>)> {
    match e {
        Expr::CmpOp { left, op: CmpOp::Ne, right } => {
            if let (Some(id), Some(st)) = (status_subject(&left.node, eff), state_literal(&right.node)) { return Some((id, vec![st])); }
            if let (Some(st), Some(id)) = (state_literal(&left.node), status_subject(&right.node, eff)) { return Some((id, vec![st])); }
            None
        }
        Expr::BinaryOp { left, op: BinOp::And, right } => {
            let (a, b) = (fact_when_false(&left.node, eff)?, fact_when_false(&right.node, eff)?);
            if a.0 != b.0 { return None; }
            let mut states = a.1;
            for st in b.1 { if !states.contains(&st) { states.push(st); } }
            Some((a.0, states))
        }
        Expr::BinaryOp { left, op: BinOp::Or, right } => fact_when_false(&left.node, eff).or_else(|| fact_when_false(&right.node, eff)),
        Expr::Not(inner) => fact_when_true(&inner.node, eff),
        _ => None,
    }
}

type Facts = std::collections::HashMap<String, Vec<String>>;

/// What holds after two paths join: an id both know, in either's states.
fn join_facts(a: &Facts, b: &Facts) -> Facts {
    let mut out = Facts::new();
    for (id, sa) in a {
        if let Some(sb) = b.get(id) {
            let mut states = sa.clone();
            for st in sb { if !states.contains(st) { states.push(st.clone()); } }
            out.insert(id.clone(), states);
        }
    }
    out
}

/// Does control never reach the end of this block?
fn block_diverges(body: &[Spanned<Statement>]) -> bool {
    match body.last().map(|s| &s.node) {
        Some(Statement::Return { .. } | Statement::Break | Statement::Continue) => true,
        Some(Statement::ExprStmt { expr }) => matches!(&expr.node, Expr::FnCall { name, .. } if name == "fail"),
        Some(Statement::If { then_body, else_body, .. }) => !else_body.is_empty() && block_diverges(then_body) && block_diverges(else_body),
        _ => false,
    }
}

/// A call after which another piece of code may have moved any instance.
fn forgets_states(name: &str) -> bool {
    matches!(name, "think" | "think_json" | "vote" | "approve" | "horde" | "sleep" | "delegate")
        || !crate::checker::names::builtin_names().contains(name)
}

/// One `if`: each branch under what its condition establishes, then the
/// join of the branches that fall through.
fn walk_branches(
    condition: &Expr,
    eff: &mut HandlerEffect,
    then_diverges: bool,
    else_diverges: bool,
    mut then_walk: impl FnMut(&mut HandlerEffect),
    mut else_walk: impl FnMut(&mut HandlerEffect),
) {
    let (when_true, when_false) = (fact_when_true(condition, eff), fact_when_false(condition, eff));
    let before = eff.facts.clone();
    if let Some((id, states)) = when_true { eff.facts.insert(id, states); }
    then_walk(eff);
    let after_then = std::mem::replace(&mut eff.facts, before);
    if let Some((id, states)) = when_false { eff.facts.insert(id, states); }
    else_walk(eff);
    let after_else = eff.facts.clone();
    eff.facts = match (then_diverges, else_diverges) {
        (true, true) => Facts::new(),
        (true, false) => after_else,
        (false, true) => after_then,
        (false, false) => join_facts(&after_then, &after_else),
    };
}

fn walk_stmts(body: &[Spanned<Statement>], path: &mut Vec<String>, eff: &mut HandlerEffect) {
    for stmt in body {
        walk_stmt(&stmt.node, stmt.span, path, eff);
        // a transition past a `return` (or a `break`/`continue`) never runs:
        // it was counted as an edge the handler takes, so `verify` answered
        // that a handler reaches a state nothing could reach through it.
        // `soma check` already warns that the statement is unreachable.
        if matches!(stmt.node, Statement::Return { .. } | Statement::Break | Statement::Continue) {
            break;
        }
    }
}

fn walk_stmt(stmt: &Statement, span: Span, path: &mut Vec<String>, eff: &mut HandlerEffect) {
    match stmt {
        Statement::ExprStmt { expr } => {
            walk_expr(&expr.node, span, path, eff);
        }
        Statement::Let { name, value } => {
            walk_expr(&value.node, span, path, eff);
            // `let s = get_status(id)`: s names that instance's state
            eff.aliases.remove(name);
            if let Expr::FnCall { name: f, args } = &value.node {
                if f == "get_status" && args.len() == 1 { eff.aliases.insert(name.clone(), render_expr(&args[0].node)); }
            }
        }
        Statement::Assign { name, value } => {
            walk_expr(&value.node, span, path, eff);
            // the name no longer means what a fact was stated about
            eff.aliases.remove(name);
            eff.aliases.retain(|_, id| id != name);
            eff.facts.remove(name);
        }
        Statement::Return { value } => {
            walk_expr(&value.node, span, path, eff);
        }
        Statement::IndexSet { index, value, .. } => {
            walk_expr(&index.node, span, path, eff);
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
            let (p1, p2) = (path.clone(), path.clone());
            walk_branches(&condition.node, eff, block_diverges(then_body), !else_body.is_empty() && block_diverges(else_body),
                |eff| {
                    let mut path = p1.clone();
                    path.push(format!("if {}", cond_text));
                    walk_stmts(then_body, &mut path, eff);
                },
                |eff| {
                    // Else-branch: push the negated condition
                    if !else_body.is_empty() {
                        let mut path = p2.clone();
                        path.push(format!("if not ({})", cond_text));
                        walk_stmts(else_body, &mut path, eff);
                    }
                });
        }
        Statement::For { iter, body, .. } => {
            walk_expr(&iter.node, span, path, eff);
            path.push("(in for loop)".to_string());
            // a later iteration starts where an earlier one left the instance
            eff.facts.clear();
            walk_stmts(body, path, eff);
            eff.facts.clear();
            path.pop();
        }
        Statement::While { condition, body, .. } => {
            walk_expr(&condition.node, span, path, eff);
            path.push(format!("while {}", render_expr(&condition.node)));
            eff.facts.clear();
            walk_stmts(body, path, eff);
            eff.facts.clear();
            path.pop();
        }
        Statement::Emit { args, .. } => {
            for a in args {
                walk_expr(&a.node, span, path, eff);
            }
            // a listener may move any instance
            eff.facts.clear();
        }
        Statement::MethodCall { method, args, .. } => {
            for a in args {
                walk_expr(&a.node, span, path, eff);
            }
            // a slot method moves nothing; `Cell.handler(…)` may
            if !matches!(method.as_str(), "set" | "get" | "push" | "delete" | "len" | "size" | "keys" | "values" | "entries") {
                eff.facts.clear();
            }
        }
        // `require get_status(id) == "held" else …` (parsed as `cond == true`)
        Statement::Require { constraint, .. } => {
            if let Constraint::Comparison { left, op: CmpOp::Eq, right } = &constraint.node {
                let fact = if matches!(right.node, Expr::Literal(Literal::Bool(true))) {
                    fact_when_true(&left.node, eff)
                } else {
                    match (status_subject(&left.node, eff), state_literal(&right.node)) {
                        (Some(id), Some(st)) => Some((id, vec![st])),
                        _ => None,
                    }
                };
                if let Some((id, states)) = fact { eff.facts.insert(id, states); }
            }
        }
        Statement::Break | Statement::Continue => {}
    }
}

fn walk_expr(expr: &Expr, span: Span, path: &mut Vec<String>, eff: &mut HandlerEffect) {
    match expr {
        Expr::FnCall { name, args } if name == "transition" => {
            // Found a transition call. The target is the second arg (index 1),
            // or the third when the handler declares its source:
            // `transition(id, from, to)`. First arg is the instance id.
            let literal_state = |e: &Expr| -> Option<String> {
                match e {
                    // `"{t}"` is interpolated: a computed target (it was
                    // refused as the literal state "{t}")
                    Expr::Literal(Literal::String(s)) if !s.contains('{') => Some(s.clone()),
                    // Bare TypeIdent: `transition(id, Validated)`.  Parsed
                    // as Expr::Ident("Validated").  The interpreter resolves
                    // it via the variant registry; statically the target
                    // name is the identifier itself.
                    Expr::Ident(name) if name.chars().next().map_or(false, |c| c.is_ascii_uppercase()) => Some(name.clone()),
                    _ => None,
                }
            };
            let (source, target_expr) = if args.len() >= 3 {
                (args.get(1).and_then(|a| literal_state(&a.node)), args.get(2))
            } else {
                (None, args.get(1))
            };
            // Also walk the args in case there's a nested transition (unlikely)
            for a in args {
                walk_expr(&a.node, span, path, eff);
            }
            let id = args.first().map(|a| render_expr(&a.node));
            if let Some(target_expr) = target_expr {
                match &target_expr.node {
                    Expr::Literal(Literal::String(_)) | Expr::Ident(_) if literal_state(&target_expr.node).is_some() => {
                        let target = literal_state(&target_expr.node).unwrap();
                        let inferred = id.as_ref().and_then(|i| eff.facts.get(i)).cloned().unwrap_or_default();
                        eff.static_transitions.push(TransitionCall {
                            target: target.clone(),
                            source,
                            inferred,
                            path: path.clone(),
                            span,
                        });
                        // the instance is there now
                        if let Some(i) = id { eff.facts.insert(i, vec![target]); }
                    }
                    _ => {
                        // Dynamic target — flag it but don't try to analyze
                        eff.has_dynamic_target = true;
                        if let Some(i) = id { eff.facts.remove(&i); }
                    }
                }
            }
        }
        Expr::FnCall { name, args } => {
            for a in args { walk_expr(&a.node, span, path, eff); }
            // a handler (or a lambda) may move any instance; a model call
            // lets other requests run
            if forgets_states(name) { eff.facts.clear(); }
        }
        Expr::MethodCall { target, method, args } => {
            walk_expr(&target.node, span, path, eff);
            for a in args { walk_expr(&a.node, span, path, eff); }
            if !matches!(method.as_str(), "set" | "get" | "push" | "delete" | "len" | "size" | "keys" | "values" | "entries")
                && (forgets_states(method) || matches!(&target.node, Expr::Ident(n) if n.chars().next().map_or(false, |c| c.is_ascii_uppercase()))) {
                eff.facts.clear();
            }
        }
        Expr::FieldAccess { target, .. } => walk_expr(&target.node, span, path, eff),
        Expr::Index { target, index } => {
            walk_expr(&target.node, span, path, eff);
            walk_expr(&index.node, span, path, eff);
        }
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
            walk_expr(&left.node, span, path, eff);
            walk_expr(&right.node, span, path, eff);
        }
        Expr::Not(e) => walk_expr(&e.node, span, path, eff),
        Expr::Try(e) | Expr::TryPropagate(e) => {
            // a transition inside `try` may have been refused: the instance
            // is where it was, or where the call put it
            let before = eff.facts.clone();
            walk_expr(&e.node, span, path, eff);
            eff.facts = join_facts(&before, &eff.facts);
        }
        Expr::Pipe { left, right } => {
            walk_expr(&left.node, span, path, eff);
            walk_expr(&right.node, span, path, eff);
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            walk_expr(&condition.node, span, path, eff);
            let cond_text = render_expr(&condition.node);
            let (p1, p2) = (path.clone(), path.clone());
            walk_branches(&condition.node, eff, block_diverges(then_body), block_diverges(else_body),
                |eff| {
                    let mut path = p1.clone();
                    path.push(format!("if {}", cond_text));
                    walk_stmts(then_body, &mut path, eff);
                    walk_expr(&then_result.node, span, &mut path, eff);
                },
                |eff| {
                    let mut path = p2.clone();
                    path.push(format!("if not ({})", cond_text));
                    walk_stmts(else_body, &mut path, eff);
                    walk_expr(&else_result.node, span, &mut path, eff);
                });
        }
        Expr::Match { subject, arms } => {
            walk_expr(&subject.node, span, path, eff);
            // `match get_status(id) { "held" -> …  "paid" | "void" -> … }`:
            // each arm knows the state it matched
            let subject_id = status_subject(&subject.node, eff);
            fn arm_states(p: &MatchPattern) -> Option<Vec<String>> {
                match p {
                    MatchPattern::Literal(Literal::String(s)) if !s.contains('{') => Some(vec![s.clone()]),
                    MatchPattern::Variant { name, fields: VariantPatternFields::Unit, .. } => Some(vec![name.clone()]),
                    MatchPattern::Or(ps) => {
                        let mut all = Vec::new();
                        for q in ps { for st in arm_states(q)? { if !all.contains(&st) { all.push(st); } } }
                        Some(all)
                    }
                    _ => None,
                }
            }
            let before = eff.facts.clone();
            let mut joined: Option<Facts> = None;
            for arm in arms {
                eff.facts = before.clone();
                if let Some(id) = &subject_id {
                    match arm_states(&arm.pattern) {
                        Some(states) => { eff.facts.insert(id.clone(), states); }
                        _ => { eff.facts.remove(id); }
                    }
                }
                path.push(format!("(match arm {})", render_pattern(&arm.pattern)));
                if let Some(g) = &arm.guard {
                    walk_expr(&g.node, span, path, eff);
                }
                walk_stmts(&arm.body, path, eff);
                walk_expr(&arm.result.node, span, path, eff);
                path.pop();
                let leaves = block_diverges(&arm.body) || matches!(&arm.result.node, Expr::FnCall { name, .. } if name == "fail");
                if !leaves {
                    joined = Some(match joined { Some(j) => join_facts(&j, &eff.facts), None => eff.facts.clone() });
                }
            }
            eff.facts = joined.unwrap_or_default();
        }
        // a lambda runs later, from an unknown state; defining it moves nothing
        Expr::Lambda { body, .. } => {
            let before = std::mem::take(&mut eff.facts);
            walk_expr(&body.node, span, path, eff);
            eff.facts = before;
        }
        Expr::LambdaBlock { stmts, result, .. } => {
            let before = std::mem::take(&mut eff.facts);
            walk_stmts(stmts, path, eff);
            walk_expr(&result.node, span, path, eff);
            eff.facts = before;
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
    // the AST renderer keeps the parentheses precedence needs:
    // `(x == 1 || x == 2) && y > 3` printed as `x == 1 || x == 2 && y > 3`
    crate::ast::render_expr(e)
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
