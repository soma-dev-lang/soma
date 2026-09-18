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
    // handler name → declared parameter types
    let params: std::collections::HashMap<String, Vec<String>> = cell.sections.iter().filter_map(|s| match &s.node {
        Section::OnSignal(h) => Some((h.signal_name.clone(), h.params.iter().map(|p| match &p.ty.node {
            TypeExpr::Simple(t) => t.clone(),
            TypeExpr::Generic { name, .. } => name.clone(),
            _ => "Any".to_string(),
        }).collect())),
        _ => None,
    }).collect();

    for section in &cell.sections {
        let body: &[Spanned<Statement>] = match &section.node {
            Section::OnSignal(h) => &h.body,
            Section::Every(e) | Section::After(e) => &e.body,
            _ => continue,
        };
        {
            for_each_call(body, &mut |name, args, span| {
                if name == "transition" {
                    if let (Some(states), Some(Spanned { node: Expr::Literal(Literal::String(target)), .. })) = (&states, args.get(1)) {
                        if !states.contains(target) {
                            let near = crate::checker::names::suggest(target, states.iter())
                                .map(|s| format!(" (did you mean \"{}\"?)", s)).unwrap_or_default();
                            out.push(LiteralIssue {
                                kind: "unknown_transition_target",
                                message: format!(
                                    "transition() to \"{}\"{} — no such state in `state {{ }}` of cell '{}': declare the edge or fix the name",
                                    target, near, cell.name
                                ),
                                span,
                            });
                        }
                    }
                }
                if let Some(types) = params.get(name) {
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
