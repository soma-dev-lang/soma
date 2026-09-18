//! A copy of the program for the static ANALYSES (proofs, termination,
//! cost, the serve 405 rule) in which hidden calls are explicit:
//!
//! - `"total {f(x)}"` — the calls in a string interpolation run where the
//!   string is built: each segment becomes an expression statement just
//!   before the statement that holds the string;
//! - `x.handler(a)` / `p.think()` (UFCS) → `handler(x, a)` / `think(p)`;
//! - `x |> f(a)` → `f(x, a)`.
//!
//! Every analysis walked `FnCall`s only, so a write, a transition, a
//! `think()` or a recursive call written in one of these forms was invisible
//! (false proofs, a GET that wrote state). The runtime is not changed.

use crate::ast::*;
use std::collections::HashSet;

/// Builtins with effects that UFCS could hide (`p.think()`).
const EFFECT_BUILTINS: &[&str] = &[
    "think", "think_json", "transition", "publish", "remember", "delegate", "approve",
    "http_get", "http_post", "http_put", "http_delete", "write_file", "set_budget",
];

struct Ctx {
    handlers: HashSet<String>,
    cells: HashSet<String>,
    slots: HashSet<String>,
}

pub fn expose_for_analysis(program: &Program) -> Program {
    let mut p = program.clone();
    let handlers: HashSet<String> = p.cells.iter().flat_map(|c| c.node.sections.iter().filter_map(|s| match &s.node {
        Section::OnSignal(on) => Some(on.signal_name.clone()),
        _ => None,
    })).collect();
    let cells: HashSet<String> = p.cells.iter().map(|c| c.node.name.clone()).collect();
    for cell in &mut p.cells {
        let slots: HashSet<String> = cell.node.sections.iter().filter_map(|s| match &s.node {
            Section::Memory(m) => Some(m.slots.iter().map(|sl| sl.node.name.clone()).collect::<Vec<_>>()),
            _ => None,
        }).flatten().collect();
        let ctx = Ctx { handlers: handlers.clone(), cells: cells.clone(), slots };
        for sec in &mut cell.node.sections {
            match &mut sec.node {
                Section::OnSignal(on) => fix_stmts(&mut on.body, &ctx),
                Section::Every(e) | Section::After(e) => fix_stmts(&mut e.body, &ctx),
                _ => {}
            }
        }
    }
    p
}

fn fix_stmts(stmts: &mut Vec<Spanned<Statement>>, ctx: &Ctx) {
    let old = std::mem::take(stmts);
    for mut st in old {
        let mut extra: Vec<Spanned<Expr>> = Vec::new();
        let mut replace: Option<Statement> = None;
        match &mut st.node {
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value }
            | Statement::Ensure { condition: value } => fix_expr(value, ctx, &mut extra),
            Statement::ExprStmt { expr } => fix_expr(expr, ctx, &mut extra),
            Statement::If { condition, then_body, else_body } => {
                fix_expr(condition, ctx, &mut extra);
                fix_stmts(then_body, ctx);
                fix_stmts(else_body, ctx);
            }
            Statement::While { condition, body, .. } => {
                fix_expr(condition, ctx, &mut extra);
                fix_stmts(body, ctx);
            }
            Statement::For { iter, body, .. } => {
                fix_expr(iter, ctx, &mut extra);
                fix_stmts(body, ctx);
            }
            Statement::IndexSet { index, value, .. } => {
                fix_expr(index, ctx, &mut extra);
                fix_expr(value, ctx, &mut extra);
            }
            Statement::MethodCall { target, method, args } => {
                for a in args.iter_mut() { fix_expr(a, ctx, &mut extra); }
                // `k._w()` as a statement: a call of the handler `_w(k)`
                if ctx.handlers.contains(method.as_str()) && !ctx.cells.contains(target.as_str()) && !ctx.slots.contains(target.as_str()) {
                    let mut all = vec![Spanned::new(Expr::Ident(target.clone()), st.span)];
                    all.extend(args.iter().cloned());
                    replace = Some(Statement::ExprStmt { expr: Spanned::new(Expr::FnCall { name: method.clone(), args: all }, st.span) });
                }
            }
            Statement::Emit { args, .. } => { for a in args.iter_mut() { fix_expr(a, ctx, &mut extra); } }
            // `require r(n) > 0 else Neg` / `require … else Big "{think(x)}"`:
            // the condition (and, when it fails, the detail) runs here — a
            // call written there was invisible to termination, cost and the
            // invariant prover
            Statement::Require { constraint, else_signal } => {
                let mut operands: Vec<Spanned<Expr>> = Vec::new();
                constraint_operands(&constraint.node, st.span, &mut operands);
                for mut o in operands {
                    let mut segs = Vec::new();
                    fix_expr(&mut o, ctx, &mut segs);
                    if !segs.is_empty() || has_effect(&o.node, ctx) {
                        extra.extend(segs);
                        extra.push(o);
                    }
                }
                let detail = match else_signal.split_once('\u{1f}') {
                    Some((_, d)) => Some(d.to_string()),
                    None if !else_signal.chars().all(|c| c.is_alphanumeric() || c == '_') => Some(else_signal.clone()),
                    None => None,
                };
                if let Some(d) = detail {
                    let mut lit = Spanned::new(Expr::Literal(Literal::String(d)), st.span);
                    fix_expr(&mut lit, ctx, &mut extra);
                }
            }
            _ => {}
        }
        if let Some(r) = replace { st.node = r; }
        for e in extra {
            let span = e.span;
            stmts.push(Spanned::new(Statement::ExprStmt { expr: e }, span));
        }
        stmts.push(st);
    }
}

fn constraint_operands(c: &Constraint, span: Span, out: &mut Vec<Spanned<Expr>>) {
    match c {
        Constraint::Comparison { left, right, .. } => { out.push(left.clone()); out.push(right.clone()); }
        Constraint::Predicate { name, args } => out.push(Spanned::new(Expr::FnCall { name: name.clone(), args: args.clone() }, span)),
        Constraint::And(a, b) | Constraint::Or(a, b) => { constraint_operands(&a.node, span, out); constraint_operands(&b.node, span, out); }
        Constraint::Not(i) => constraint_operands(&i.node, span, out),
        Constraint::Descriptive(_) => {}
    }
}

/// A call an analysis must see: a handler, an effect builtin, a slot write
/// or a pipe / method form of one.
fn has_effect(e: &Expr, ctx: &Ctx) -> bool {
    let mut hit = false;
    crate::checker::literals::for_each_in_expr(e, &mut |x| match x {
        Expr::FnCall { name, .. } if ctx.handlers.contains(name.as_str()) || EFFECT_BUILTINS.contains(&name.as_str()) || name == "next_id" => hit = true,
        Expr::MethodCall { method, .. } if ctx.handlers.contains(method.as_str()) || EFFECT_BUILTINS.contains(&method.as_str())
            || matches!(method.as_str(), "set" | "put" | "delete" | "remove" | "push" | "append" | "clear") => hit = true,
        Expr::Pipe { .. } => hit = true,
        _ => {}
    });
    hit
}

fn as_stmts(exprs: Vec<Spanned<Expr>>) -> Vec<Spanned<Statement>> {
    exprs.into_iter().map(|e| { let span = e.span; Spanned::new(Statement::ExprStmt { expr: e }, span) }).collect()
}

fn fix_expr(e: &mut Spanned<Expr>, ctx: &Ctx, extra: &mut Vec<Spanned<Expr>>) {
    let span = e.span;
    let mut replacement: Option<Expr> = None;
    match &mut e.node {
        Expr::Literal(Literal::String(s)) => {
            for seg in segments(s) {
                if let Some(x) = parse_segment(&seg) {
                    let mut sx = Spanned::new(x, span);
                    fix_expr(&mut sx, ctx, extra);
                    extra.push(sx);
                }
            }
        }
        Expr::FnCall { name, args } => {
            for a in args.iter_mut() { fix_expr(a, ctx, extra); }
            // `delegate("Cell", "h", a…)` with literal names is a call of h(a…)
            if name == "delegate" && args.len() >= 2 {
                if let (Expr::Literal(Literal::String(c)), Expr::Literal(Literal::String(h))) = (&args[0].node, &args[1].node) {
                    let target = Spanned::new(Expr::Ident(c.clone()), span);
                    replacement = Some(Expr::MethodCall { target: Box::new(target), method: h.clone(), args: args[2..].to_vec() });
                }
            }
        }
        Expr::MethodCall { target, method, args } => {
            fix_expr(target, ctx, extra);
            for a in args.iter_mut() { fix_expr(a, ctx, extra); }
            let named = matches!(&target.node, Expr::Ident(t) if ctx.cells.contains(t) || ctx.slots.contains(t));
            if !named && (ctx.handlers.contains(method.as_str()) || EFFECT_BUILTINS.contains(&method.as_str())) {
                let mut all = vec![(**target).clone()];
                all.extend(args.iter().cloned());
                replacement = Some(Expr::FnCall { name: method.clone(), args: all });
            }
        }
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
            fix_expr(left, ctx, extra);
            fix_expr(right, ctx, extra);
        }
        Expr::Pipe { left, right } => {
            fix_expr(left, ctx, extra);
            fix_expr(right, ctx, extra);
            if let Expr::FnCall { name, args } = &right.node {
                let mut all = vec![(**left).clone()];
                all.extend(args.iter().cloned());
                replacement = Some(Expr::FnCall { name: name.clone(), args: all });
            }
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => fix_expr(i, ctx, extra),
        Expr::FieldAccess { target, .. } => fix_expr(target, ctx, extra),
        Expr::Index { target, index } => { fix_expr(target, ctx, extra); fix_expr(index, ctx, extra); }
        Expr::ListLiteral(items) => { for it in items.iter_mut() { fix_expr(it, ctx, extra); } }
        Expr::Record { fields, .. } => { for (_, v) in fields.iter_mut() { fix_expr(v, ctx, extra); } }
        // what runs inside a lambda stays inside it (it may run many times)
        Expr::Lambda { param, body } => {
            let mut inner = Vec::new();
            fix_expr(body, ctx, &mut inner);
            if !inner.is_empty() {
                replacement = Some(Expr::LambdaBlock { param: param.clone(), stmts: as_stmts(inner), result: body.clone() });
            }
        }
        Expr::LambdaBlock { stmts, result, .. } => {
            fix_stmts(stmts, ctx);
            let mut inner = Vec::new();
            fix_expr(result, ctx, &mut inner);
            stmts.extend(as_stmts(inner));
        }
        Expr::Match { subject, arms } => {
            fix_expr(subject, ctx, extra);
            for arm in arms.iter_mut() {
                if let Some(g) = arm.guard.as_mut() { fix_expr(g, ctx, extra); }
                fix_stmts(&mut arm.body, ctx);
                let mut inner = Vec::new();
                fix_expr(&mut arm.result, ctx, &mut inner);
                arm.body.extend(as_stmts(inner));
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            fix_expr(condition, ctx, extra);
            fix_stmts(then_body, ctx);
            let mut inner = Vec::new();
            fix_expr(then_result, ctx, &mut inner);
            then_body.extend(as_stmts(inner));
            fix_stmts(else_body, ctx);
            let mut inner = Vec::new();
            fix_expr(else_result, ctx, &mut inner);
            else_body.extend(as_stmts(inner));
        }
        _ => {}
    }
    if let Some(r) = replacement { e.node = r; }
}

/// The `{…}` segments the runtime evaluates (same rules as
/// `interpolate_string`: `{{`/`}}` escapes, CSS-like and regex-quantifier
/// segments are literal text).
fn segments(s: &str) -> Vec<String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < b.len() {
        if b[pos] == b'{' && b.get(pos + 1) == Some(&b'{') { pos += 2; continue; }
        if b[pos] == b'}' && b.get(pos + 1) == Some(&b'}') { pos += 2; continue; }
        if b[pos] == b'{' {
            if let Some(end) = s[pos + 1..].find('}') {
                let seg = &s[pos + 1..pos + 1 + end];
                let quantifier = seg.chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' ');
                if !(seg.is_empty() || seg.contains(':') || seg.contains(';') || quantifier) {
                    out.push(seg.to_string());
                    pos = pos + 1 + end + 1;
                    continue;
                }
            }
        }
        pos += 1;
    }
    out
}

fn parse_segment(seg: &str) -> Option<Expr> {
    let wrapped = format!("cell _T {{ on _e() {{ return {} }} }}", seg);
    let tokens = crate::lexer::Lexer::new(&wrapped).tokenize().ok()?;
    let program = crate::parser::Parser::new(tokens).parse_program().ok()?;
    program.cells.first().and_then(|cell| cell.node.sections.iter().find_map(|s| match &s.node {
        Section::OnSignal(on) => on.body.first().and_then(|st| match &st.node {
            Statement::Return { value } => Some(value.node.clone()),
            _ => None,
        }),
        _ => None,
    }))
}
