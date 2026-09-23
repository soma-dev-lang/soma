//! Literal-argument checks: things that are decidable by reading the call
//! site alone. `transition(id, "shiped")` to a state no machine declares,
//! and `takes_int("s")` where the handler declares `n: Int`, both used to
//! pass `soma check` and fail at runtime (or, for the transition, only in
//! `soma verify` — which `soma serve` does not run).

use crate::ast::*;
use crate::checker::Span;

pub struct LiteralIssue {
    pub kind: &'static str,
    pub message: String,
    pub span: Span,
}

pub fn check_cell(cell: &CellDef) -> Vec<LiteralIssue> {
    let mut out = Vec::new();
    // states of this cell's machine (one per cell)
    let states: Option<Vec<String>> = cell.sections.iter().find_map(|s| match &s.node {
        Section::State(sm) => {
            let mut v: Vec<String> = vec![sm.initial.clone()];
            for t in &sm.transitions {
                if t.node.from != "*" { v.push(t.node.from.clone()); }
                v.push(t.node.to.clone());
            }
            v.sort();
            v.dedup();
            Some(v)
        }
        _ => None,
    });
    // declared edges (from, to), `*` kept as the source of a wildcard edge
    let edges: Vec<(String, String)> = cell.sections.iter().flat_map(|s| match &s.node {
        Section::State(sm) => sm.transitions.iter().map(|t| (t.node.from.clone(), t.node.to.clone())).collect::<Vec<_>>(),
        _ => Vec::new(),
    }).collect();
    // states some declared edge enters (`* -> x` included)
    let entered: std::collections::HashSet<String> = cell.sections.iter().flat_map(|s| match &s.node {
        Section::State(sm) => sm.transitions.iter().map(|t| t.node.to.clone()).collect::<Vec<_>>(),
        _ => Vec::new(),
    }).collect();
    let initial: Option<String> = cell.sections.iter().find_map(|s| match &s.node {
        Section::State(sm) => Some(sm.initial.clone()),
        _ => None,
    });
    // handler name → declared parameter types
    let params: std::collections::HashMap<String, Vec<String>> = cell.sections.iter().filter_map(|s| match &s.node {
        Section::OnSignal(h) => Some((h.signal_name.clone(), h.params.iter().map(|p| match &p.ty.node {
            TypeExpr::Simple(t) => t.clone(),
            TypeExpr::Generic { name, .. } => name.clone(),
            _ => "Any".to_string(),
        }).collect())),
        _ => None,
    }).collect();

    // two edges for one move: a transition takes the FIRST edge that matches,
    // so `* -> cancelled` before `pending -> cancelled { guard { … } }` left
    // that guard never evaluated (and verify still called it runtime-checked);
    // in the other order a failing guard refused a move the wildcard allows
    for s in &cell.sections {
        let Section::State(sm) = &s.node else { continue };
        let covers = |t: &Transition, state: &str| if t.from == "*" { sm.wildcard_applies(t, state) } else { t.from == state };
        let sources: Vec<String> = states.iter().flatten().cloned().collect();
        for (j, later) in sm.transitions.iter().enumerate() {
            // the same guard (none on both: `resolved -> closed` then
            // `* -> closed`) answers the same whichever edge is taken
            let guard_text = |t: &Transition| t.guard.as_ref().map(|g| crate::ast::render_expr(&g.node));
            let Some(first) = sm.transitions[..j].iter().find(|t| t.node.to == later.node.to
                && guard_text(&t.node) != guard_text(&later.node)
                && sources.iter().any(|st| covers(&t.node, st) && covers(&later.node, st))) else { continue };
            let shared: Vec<&String> = sources.iter().filter(|st| covers(&first.node, st) && covers(&later.node, st)).collect();
            let edge = |t: &Transition| format!("{} -> {}{}", t.from, t.to, if t.guard.is_some() { " { guard … }" } else { "" });
            out.push(LiteralIssue { kind: "duplicate_edge", span: later.span,
                message: format!(
                    "`{}` repeats the move {} -> {} that `{}` already declares with {} — a transition takes the first edge that matches, so {} there. Declare each move once{}",
                    edge(&later.node), shared.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("/"), later.node.to, edge(&first.node),
                    if first.node.guard.is_some() { "a guard" } else { "no guard" },
                    if later.node.guard.is_some() { "this guard is never evaluated" } else { "the first edge's guard refuses a move this one allows" },
                    if first.node.from == "*" { format!(", or keep the state out of the wildcard: `* -> {} except [{}]`", first.node.to, shared.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")) } else { String::new() }) });
        }
    }

    // `* -> cancelled except [setled]`: a name no edge declares excepted
    // nothing, and the wildcard left the state the author meant to keep final
    if let Some(states) = &states {
        for s in &cell.sections {
            let Section::State(sm) = &s.node else { continue };
            for t in &sm.transitions {
                for e in &t.node.except {
                    if states.contains(e) { continue; }
                    let near = crate::checker::names::suggest(e, states.iter()).map(|s| format!(" (did you mean '{}'?)", s)).unwrap_or_default();
                    out.push(LiteralIssue { kind: "unknown_state", span: t.span,
                        message: format!("`* -> {} except [{}]` names '{}'{}, which no transition of `state {}` declares — it excepts nothing",
                            t.node.to, t.node.except.join(", "), e, near, sm.name) });
                }
            }
        }
    }

    for section in &cell.sections {
        let (body, owner): (&[Spanned<Statement>], String) = match &section.node {
            Section::OnSignal(h) => (&h.body, format!("handler '{}'", h.signal_name)),
            Section::Every(e) => (&e.body, "an `every` block".to_string()),
            Section::After(e) => (&e.body, "an `after` block".to_string()),
            _ => continue,
        };
        {
            for_each_call(body, &mut |name, args, span| {
                if name == "transition" && !(2..=3).contains(&args.len()) && !params.contains_key("transition") {
                    out.push(LiteralIssue {
                        kind: "argument_count",
                        message: format!("transition() takes 2 arguments (instance id, target state) or 3 (instance id, source state, target state), got {} — `transition(id, \"shipped\")` or `transition(id, \"paid\", \"shipped\")`", args.len()),
                        span,
                    });
                }
                // transition(id, from, to): the declared edge must exist
                if name == "transition" && args.len() == 3 && !params.contains_key("transition") {
                    if let (Some(states), Some(Spanned { node: Expr::Literal(Literal::String(from)), .. }), Some(Spanned { node: Expr::Literal(Literal::String(to)), .. })) = (&states, args.get(1), args.get(2)) {
                        if !from.contains('{') && !to.contains('{') {
                            if !states.contains(from) {
                                let near = crate::checker::names::suggest(from, states.iter()).map(|s| format!(" (did you mean \"{}\"?)", s)).unwrap_or_default();
                                out.push(LiteralIssue { kind: "unknown_transition_target", span,
                                    message: format!("transition() from \"{}\"{} in {} — no such state in `state {{ }}` of cell '{}'", from, near, owner, cell.name) });
                            } else if !edges.iter().any(|(f, t)| t == to && (f == from || f == "*")) {
                                let leaving: Vec<String> = edges.iter().filter(|(f, _)| f == from || f == "*").map(|(_, t)| t.clone()).collect();
                                out.push(LiteralIssue { kind: "unknown_transition_target", span,
                                    message: format!("transition(id, \"{}\", \"{}\") in {} — cell '{}' declares no edge {} -> {}; edges leaving '{}': [{}]. Declare the edge, or fix the source",
                                        from, to, owner, cell.name, from, to, from, leaving.join(", ")) });
                            }
                        }
                    }
                }
                if name == "transition" {
                    let target_arg = if args.len() >= 3 { args.get(2) } else { args.get(1) };
                    if let (Some(states), Some(Spanned { node: Expr::Literal(Literal::String(target)), .. })) = (&states, target_arg) {
                        // "{g}_x" is interpolated at runtime: a dynamic target, not this text
                        // a declared state no edge ENTERS (the initial state,
                        // typically): the transition fails on every call
                        if states.contains(target) && !target.contains('{') && !entered.contains(target) {
                            out.push(LiteralIssue {
                                kind: "unknown_transition_target",
                                message: format!(
                                    "transition() to \"{}\" in {} — no declared edge of cell '{}' enters '{}'{}, so this raises invalid_transition every time: declare an edge into it, or drop the call",
                                    target, owner, cell.name, target,
                                    if initial.as_deref() == Some(target.as_str()) { " (it is the initial state: a fresh id is already there)" } else { "" }
                                ),
                                span,
                            });
                        }
                        if !states.contains(target) && !target.contains('{') {
                            let near = crate::checker::names::suggest(target, states.iter())
                                .map(|s| format!(" (did you mean \"{}\"?)", s)).unwrap_or_default();
                            out.push(LiteralIssue {
                                kind: "unknown_transition_target",
                                message: format!(
                                    "transition() to \"{}\"{} in {} — no such state in `state {{ }}` of cell '{}': declare the edge or fix the name",
                                    target, near, owner, cell.name
                                ),
                                span,
                            });
                        }
                    }
                }
                // an arity the handler does not take goes to the builtin of
                // that name (or is an arity error reported elsewhere)
                let takes = |types: &Vec<String>| {
                    let required = types.len() - usize::from(types.last().map_or(false, |t| t == "Map"));
                    args.len() >= required && args.len() <= types.len()
                };
                if let Some(types) = params.get(name).filter(|t| takes(t)) {
                    for (i, a) in args.iter().enumerate() {
                        let Some(ty) = types.get(i) else { break };
                        let got = match &a.node {
                            Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::BigInt(_)) => "Int",
                            Expr::Literal(Literal::Float(_)) => "Float",
                            Expr::Literal(Literal::String(_)) => "String",
                            Expr::Literal(Literal::Bool(_)) => "Bool",
                            Expr::ListLiteral(_) => "List",
                            _ => continue,
                        };
                        let ok = match (ty.as_str(), got) {
                            ("Any", _) => true,
                            (t, g) if t == g => true,
                            ("Float" | "BigInt", "Int") => true,
                            ("Map", _) | ("List", "List") => true,
                            _ => false,
                        };
                        if !ok {
                            out.push(LiteralIssue {
                                kind: "argument_type",
                                message: format!(
                                    "{}(): argument {} is a {} literal but the handler declares it {} — fix the value or the parameter type",
                                    name, i + 1, got, ty
                                ),
                                span: a.span,
                            });
                        }
                    }
                }
            });
        }
    }
    out
}

/// Every `f(args)` call in a body, nested expressions included.
pub fn for_each_call(stmts: &[Spanned<Statement>], f: &mut dyn FnMut(&str, &[Spanned<Expr>], Span)) {
    for st in stmts {
        match &st.node {
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value }
            | Statement::Ensure { condition: value } => walk_expr(value, f),
            Statement::ExprStmt { expr } => walk_expr(expr, f),
            Statement::IndexSet { index, value, .. } => { walk_expr(index, f); walk_expr(value, f); }
            Statement::If { condition, then_body, else_body } => {
                walk_expr(condition, f); for_each_call(then_body, f); for_each_call(else_body, f);
            }
            Statement::For { iter, body, .. } => { walk_expr(iter, f); for_each_call(body, f); }
            Statement::While { condition, body, .. } => { walk_expr(condition, f); for_each_call(body, f); }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => { for a in args { walk_expr(a, f); } }
            Statement::Require { constraint, .. } => walk_constraint(&constraint.node, f),
            Statement::Break | Statement::Continue => {}
        }
    }
}

fn walk_constraint(c: &Constraint, f: &mut dyn FnMut(&str, &[Spanned<Expr>], Span)) {
    match c {
        Constraint::Comparison { left, right, .. } => { walk_expr(left, f); walk_expr(right, f); }
        Constraint::And(a, b) | Constraint::Or(a, b) => { walk_constraint(&a.node, f); walk_constraint(&b.node, f); }
        Constraint::Not(inner) => walk_constraint(&inner.node, f),
        Constraint::Predicate { .. } | Constraint::Descriptive(_) => {}
    }
}

fn walk_expr(e: &Spanned<Expr>, f: &mut dyn FnMut(&str, &[Spanned<Expr>], Span)) {
    match &e.node {
        Expr::FnCall { name, args } => {
            f(name, args, e.span);
            for a in args { walk_expr(a, f); }
        }
        Expr::MethodCall { target, args, .. } => { walk_expr(target, f); for a in args { walk_expr(a, f); } }
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => {
            walk_expr(left, f); walk_expr(right, f);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => walk_expr(i, f),
        Expr::FieldAccess { target, .. } => walk_expr(target, f),
        Expr::Index { target, index } => { walk_expr(target, f); walk_expr(index, f); }
        Expr::ListLiteral(items) => { for it in items { walk_expr(it, f); } }
        Expr::Record { fields, .. } => { for (_, v) in fields { walk_expr(v, f); } }
        Expr::Lambda { body, .. } => walk_expr(body, f),
        Expr::LambdaBlock { stmts, result, .. } => { for_each_call(stmts, f); walk_expr(result, f); }
        Expr::Match { subject, arms } => {
            walk_expr(subject, f);
            for arm in arms {
                if let Some(g) = &arm.guard { walk_expr(g, f); }
                for_each_call(&arm.body, f);
                walk_expr(&arm.result, f);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result, .. } => {
            walk_expr(condition, f);
            for_each_call(then_body, f); walk_expr(then_result, f);
            for_each_call(else_body, f); walk_expr(else_result, f);
        }
        _ => {}
    }
}

/// Every expression in a body (nested blocks, lambdas and arms included),
/// pre-order — for walkers that need more than `f(args)` calls
/// (`Cell.handler(…)` is a MethodCall).
pub fn for_each_expr<'a>(stmts: &'a [Spanned<Statement>], f: &mut dyn FnMut(&'a Expr)) {
    fn ex<'a>(e: &'a Spanned<Expr>, f: &mut dyn FnMut(&'a Expr)) { for_each_in_expr(&e.node, f) }
    fn con<'a>(c: &'a Constraint, f: &mut dyn FnMut(&'a Expr)) {
        match c {
            Constraint::Comparison { left, right, .. } => { ex(left, f); ex(right, f); }
            Constraint::And(a, b) | Constraint::Or(a, b) => { con(&a.node, f); con(&b.node, f); }
            Constraint::Not(i) => con(&i.node, f),
            // `require valid(f(x)) else …`: its arguments run too
            Constraint::Predicate { args, .. } => { for a in args { ex(a, f); } }
            Constraint::Descriptive(_) => {}
        }
    }
    for st in stmts {
        match &st.node {
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value }
            | Statement::Ensure { condition: value } => ex(value, f),
            Statement::ExprStmt { expr } => ex(expr, f),
            Statement::IndexSet { index, value, .. } => { ex(index, f); ex(value, f); }
            Statement::If { condition, then_body, else_body } => { ex(condition, f); for_each_expr(then_body, f); for_each_expr(else_body, f); }
            Statement::For { iter, body, .. } => { ex(iter, f); for_each_expr(body, f); }
            Statement::While { condition, body, .. } => { ex(condition, f); for_each_expr(body, f); }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => { for a in args { ex(a, f); } }
            Statement::Require { constraint, .. } => con(&constraint.node, f),
            Statement::Break | Statement::Continue => {}
        }
    }
}

/// `f` on `e` and every expression nested in it (blocks included).
pub fn for_each_in_expr<'a>(e: &'a Expr, f: &mut dyn FnMut(&'a Expr)) {
    let ex = |x: &'a Spanned<Expr>, f: &mut dyn FnMut(&'a Expr)| for_each_in_expr(&x.node, f);
    f(e);
    match e {
        Expr::FnCall { args, .. } => { for a in args { ex(a, f); } }
        Expr::MethodCall { target, args, .. } => { ex(target, f); for a in args { ex(a, f); } }
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => { ex(left, f); ex(right, f); }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => ex(i, f),
        Expr::FieldAccess { target, .. } => ex(target, f),
        Expr::Index { target, index } => { ex(target, f); ex(index, f); }
        Expr::ListLiteral(items) => { for it in items { ex(it, f); } }
        Expr::Record { fields, .. } => { for (_, v) in fields { ex(v, f); } }
        Expr::Lambda { body, .. } => ex(body, f),
        Expr::LambdaBlock { stmts, result, .. } => { for_each_expr(stmts, f); ex(result, f); }
        Expr::Match { subject, arms } => {
            ex(subject, f);
            for arm in arms {
                if let Some(g) = &arm.guard { ex(g, f); }
                for_each_expr(&arm.body, f);
                ex(&arm.result, f);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result, .. } => {
            ex(condition, f);
            for_each_expr(then_body, f); ex(then_result, f);
            for_each_expr(else_body, f); ex(else_result, f);
        }
        _ => {}
    }
}

/// Every STATEMENT of a body, including those inside expression blocks —
/// block lambdas, `try { }`, if-expression branches and match-arm bodies.
/// Statement-level effects (`slot[k] = v`, `emit`, `Cell.h(…)`) written
/// there were invisible to walkers that recursed through If / For / While
/// only (a GET that wrote, a forgeable listener, a false termination).
pub fn for_each_stmt_deep<'a>(stmts: &'a [Spanned<Statement>], f: &mut dyn FnMut(&'a Statement)) {
    fn blocks_in<'a>(e: &'a Expr, f: &mut dyn FnMut(&'a Statement)) {
        let ex = |x: &'a Spanned<Expr>, f: &mut dyn FnMut(&'a Statement)| blocks_in(&x.node, f);
        match e {
            Expr::FnCall { args, .. } => { for a in args { ex(a, f); } }
            Expr::MethodCall { target, args, .. } => { ex(target, f); for a in args { ex(a, f); } }
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => { ex(left, f); ex(right, f); }
            Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => ex(i, f),
            Expr::FieldAccess { target, .. } => ex(target, f),
            Expr::Index { target, index } => { ex(target, f); ex(index, f); }
            Expr::ListLiteral(items) => { for it in items { ex(it, f); } }
            Expr::Record { fields, .. } => { for (_, v) in fields { ex(v, f); } }
            Expr::Lambda { body, .. } => ex(body, f),
            Expr::LambdaBlock { stmts, result, .. } => { for_each_stmt_deep(stmts, f); ex(result, f); }
            Expr::Match { subject, arms } => {
                ex(subject, f);
                for arm in arms {
                    if let Some(g) = &arm.guard { ex(g, f); }
                    for_each_stmt_deep(&arm.body, f);
                    ex(&arm.result, f);
                }
            }
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result, .. } => {
                ex(condition, f);
                for_each_stmt_deep(then_body, f); ex(then_result, f);
                for_each_stmt_deep(else_body, f); ex(else_result, f);
            }
            _ => {}
        }
    }
    for st in stmts {
        f(&st.node);
        match &st.node {
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value }
            | Statement::Ensure { condition: value } => blocks_in(&value.node, f),
            Statement::ExprStmt { expr } => blocks_in(&expr.node, f),
            Statement::IndexSet { index, value, .. } => { blocks_in(&index.node, f); blocks_in(&value.node, f); }
            Statement::If { condition, then_body, else_body } => { blocks_in(&condition.node, f); for_each_stmt_deep(then_body, f); for_each_stmt_deep(else_body, f); }
            Statement::For { iter, body, .. } => { blocks_in(&iter.node, f); for_each_stmt_deep(body, f); }
            Statement::While { condition, body, .. } => { blocks_in(&condition.node, f); for_each_stmt_deep(body, f); }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => { for a in args { blocks_in(&a.node, f); } }
            _ => {}
        }
    }
}

/// The statements inside the expression blocks of `e` (a guard, an
/// invariant): a condition holding `{ slot[k] = v  true }` wrote state.
pub fn for_each_stmt_in_expr<'a>(e: &'a Expr, f: &mut dyn FnMut(&'a Statement)) {
    // for_each_in_expr reaches every nested block expression; visiting each
    // block's statements deeply may visit an inner block twice — harmless
    // for a purity check
    let mut blocks: Vec<&'a Expr> = Vec::new();
    for_each_in_expr(e, &mut |x| {
        if matches!(x, Expr::LambdaBlock { .. } | Expr::IfExpr { .. } | Expr::Match { .. }) { blocks.push(x); }
    });
    for b in blocks {
        match b {
            Expr::LambdaBlock { stmts, .. } => for_each_stmt_deep(stmts, f),
            Expr::IfExpr { then_body, else_body, .. } => { for_each_stmt_deep(then_body, f); for_each_stmt_deep(else_body, f); }
            Expr::Match { arms, .. } => { for a in arms { for_each_stmt_deep(&a.body, f); } }
            _ => {}
        }
    }
}

/// The span of the innermost statement of `body` for which `hit` holds
/// (`hit` sees a whole statement, nested blocks included; the blocks of an
/// `if` / `for` / `while` are searched first). A finding reported there
/// points at its line, not at the handler's `on` line.
pub fn innermost_stmt(body: &[Spanned<Statement>], hit: &mut dyn FnMut(&Spanned<Statement>) -> bool) -> Option<Span> {
    for st in body {
        if !hit(st) { continue; }
        let inner: Vec<&[Spanned<Statement>]> = match &st.node {
            Statement::If { then_body, else_body, .. } => vec![then_body.as_slice(), else_body.as_slice()],
            Statement::For { body, .. } | Statement::While { body, .. } => vec![body.as_slice()],
            _ => vec![],
        };
        for b in inner { if let Some(sp) = innermost_stmt(b, hit) { return Some(sp); } }
        return Some(st.span);
    }
    None
}

/// The innermost statement containing an expression for which `expr` holds.
pub fn stmt_with_expr(body: &[Spanned<Statement>], expr: &mut dyn FnMut(&Expr) -> bool) -> Option<Span> {
    innermost_stmt(body, &mut |st| {
        let mut found = false;
        for_each_expr(std::slice::from_ref(st), &mut |e| if !found && expr(e) { found = true; });
        found
    })
}

/// The span of the first call `name(…)` in a body.
pub fn call_site(body: &[Spanned<Statement>], name: &str) -> Option<Span> {
    let mut at = None;
    for_each_call(body, &mut |n, _, sp| if at.is_none() && n == name { at = Some(sp); });
    at
}
