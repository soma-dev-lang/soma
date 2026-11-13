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

/// (handler names, cell names) of every cell, interior ones included.
pub fn program_names(program: &Program) -> (HashSet<String>, HashSet<String>) {
    let all = crate::checker::names::collect_cells(program);
    let handlers: HashSet<String> = all.iter().flat_map(|c| c.sections.iter().filter_map(|s| match &s.node {
        Section::OnSignal(on) => Some(on.signal_name.clone()),
        _ => None,
    })).collect();
    let cells: HashSet<String> = all.iter().map(|c| c.name.clone()).collect();
    (handlers, cells)
}

thread_local! {
    /// The product search reads a string's calls where the string stands,
    /// and a `require`'s operands once: nothing is hoisted for it.
    static IN_PLACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `expose_for_analysis` for an analysis that follows the ORDER of what
/// runs (the product of the machines): UFCS, pipes and `std.` made
/// explicit, but no call moved out of the expression that holds it —
/// `[transition(…), "{Visit.open(id)}"]` opened the visit first, and
/// `require Visit.bump(id) == 1` bumped twice.
pub fn expose_in_place(program: &Program) -> Program {
    IN_PLACE.with(|f| f.set(true));
    let p = expose_for_analysis(program);
    IN_PLACE.with(|f| f.set(false));
    p
}

/// The expressions a string evaluates in its `{…}`, exposed like the rest.
pub fn segment_exprs(text: &str, names: &(HashSet<String>, HashSet<String>), cell: &CellDef) -> Vec<Expr> {
    let ctx = Ctx { handlers: names.0.clone(), cells: names.1.clone(), slots: slots_of(cell) };
    let was = IN_PLACE.with(|f| f.replace(true));
    let out = segments(text).iter().filter_map(|seg| parse_segment(seg)).map(|x| {
        let mut sx = Spanned::new(x, Span::new(0, 0));
        fix_expr(&mut sx, &ctx, &mut Vec::new());
        sx.node
    }).collect();
    IN_PLACE.with(|f| f.set(was));
    out
}

pub fn expose_for_analysis(program: &Program) -> Program {
    let mut p = program.clone();
    crate::checker::names::set_program_cells(program_names(program).1);
    // the statements of `runtime { }` run like a handler's (`soma run
    // app.cell`): one more entry, for every analysis that reads this copy
    crate::checker::runtime_section::as_handler(&mut p);
    // interior cells too: their handlers are proven (and run) as well
    let (handlers, cells) = program_names(program);
    fn fix_cell(cell: &mut CellDef, handlers: &HashSet<String>, cells: &HashSet<String>) {
        let ctx = Ctx { handlers: handlers.clone(), cells: cells.clone(), slots: slots_of(cell) };
        for sec in &mut cell.sections {
            match &mut sec.node {
                Section::OnSignal(on) => fix_stmts(&mut on.body, &ctx),
                Section::Every(e) | Section::After(e) => fix_stmts(&mut e.body, &ctx),
                Section::Interior(i) => { for child in &mut i.cells { fix_cell(&mut child.node, handlers, cells); } }
                _ => {}
            }
        }
    }
    for cell in &mut p.cells {
        fix_cell(&mut cell.node, &handlers, &cells);
    }
    p
}

fn slots_of(cell: &CellDef) -> HashSet<String> {
    cell.sections.iter().filter_map(|s| match &s.node {
        Section::Memory(m) => Some(m.slots.iter().map(|sl| sl.node.name.clone()).collect::<Vec<_>>()),
        _ => None,
    }).flatten().collect()
}

/// One handler of `cell` with its hidden calls made explicit (as
/// `expose_for_analysis` does for the whole program).
pub fn expose_handler(on: &OnSection, cell: &CellDef, names: &(HashSet<String>, HashSet<String>)) -> OnSection {
    let ctx = Ctx { handlers: names.0.clone(), cells: names.1.clone(), slots: slots_of(cell) };
    let mut out = on.clone();
    fix_stmts(&mut out.body, &ctx);
    out
}

fn fix_stmts(stmts: &mut Vec<Spanned<Statement>>, ctx: &Ctx) {
    let old = std::mem::take(stmts);
    for mut st in old {
        let mut extra: Vec<Spanned<Expr>> = Vec::new();
        let mut maybe: Vec<Spanned<Statement>> = Vec::new();
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
                // (at each turn, not once before the loop)
                fix_lazy(condition, ctx);
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
                    if !IN_PLACE.with(|f| f.get()) && (!segs.is_empty() || has_effect(&o.node, ctx)) {
                        extra.extend(segs);
                        extra.push(o);
                    }
                }
                let detail = match else_signal.split_once('\u{1f}') {
                    Some((_, d)) => Some(d.to_string()),
                    None if !else_signal.chars().all(|c| c.is_alphanumeric() || c == '_') => Some(else_signal.clone()),
                    None => None,
                };
                // (built only when the require fails: it may not run)
                if let Some(d) = detail {
                    let mut lit = Spanned::new(Expr::Literal(Literal::String(d)), st.span);
                    let mut inner = Vec::new();
                    fix_expr(&mut lit, ctx, &mut inner);
                    if !inner.is_empty() {
                        let cond = Spanned::new(Expr::Literal(Literal::Bool(true)), st.span);
                        maybe.push(Spanned::new(Statement::If { condition: cond, then_body: as_stmts(inner), else_body: Vec::new() }, st.span));
                    }
                }
            }
            _ => {}
        }
        if let Some(r) = replace { st.node = r; }
        for e in extra {
            let span = e.span;
            stmts.push(Spanned::new(Statement::ExprStmt { expr: e }, span));
        }
        stmts.extend(maybe);
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

/// The string literals of a match pattern.
pub(crate) fn pattern_texts(p: &MatchPattern, out: &mut Vec<String>) {
    match p {
        MatchPattern::Literal(Literal::String(s)) => out.push(s.clone()),
        MatchPattern::Or(ps) => for q in ps { pattern_texts(q, out); },
        MatchPattern::MapDestructure(fields) => for (_, q) in fields { pattern_texts(q, out); },
        MatchPattern::StringPrefix { prefix, .. } => out.push(prefix.clone()),
        MatchPattern::Variant { fields, .. } => match fields {
            VariantPatternFields::Tuple(ps) => for q in ps { pattern_texts(q, out); },
            VariantPatternFields::Struct { fields, .. } => for (_, q) in fields { pattern_texts(q, out); },
            VariantPatternFields::Unit => {}
        },
        _ => {}
    }
}

/// A call an analysis must see: a handler, an effect builtin, a slot write
/// or a pipe / method form of one.
fn has_effect(e: &Expr, ctx: &Ctx) -> bool {
    let mut hit = false;
    // (a statement inside an expression block: `if c { slot[k] = v  true } else { … }`)
    crate::checker::literals::for_each_stmt_in_expr(e, &mut |_| hit = true);
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

/// `fix_expr` for an expression that may not run (the right of `&&`, a
/// match guard, …) or runs more than once (a `while` condition): the calls
/// its strings make are not hoisted before the statement — they would then
/// have run for sure, once — but kept with it, as `if true { calls… ; e }
/// else { e }`, which every analysis reads as "may run".
fn fix_lazy(e: &mut Spanned<Expr>, ctx: &Ctx) {
    let mut inner = Vec::new();
    fix_expr(e, ctx, &mut inner);
    if inner.is_empty() { return; }
    let span = e.span;
    let plain = e.clone();
    e.node = Expr::IfExpr {
        condition: Box::new(Spanned::new(Expr::Literal(Literal::Bool(true)), span)),
        then_body: as_stmts(inner),
        then_result: Box::new(plain.clone()),
        else_body: Vec::new(),
        else_result: Box::new(plain),
    };
}

fn fix_expr(e: &mut Spanned<Expr>, ctx: &Ctx, extra: &mut Vec<Spanned<Expr>>) {
    let span = e.span;
    let mut replacement: Option<Expr> = None;
    match &mut e.node {
        Expr::Literal(Literal::String(_)) if IN_PLACE.with(|f| f.get()) => {}
        Expr::Literal(Literal::String(s)) => {
            for seg in segments(s) {
                if let Some(x) = parse_segment(&seg) {
                    let mut sx = Spanned::new(x, span);
                    fix_expr(&mut sx, ctx, extra);
                    extra.push(sx);
                }
            }
        }
        // `std.transition(…)`, `std.delegate(…)`, `std.think(…)`: the builtin,
        // under the name every analysis knows it by (as `_std("transition", …)`
        // it moved a machine, recursed or spent, unseen)
        Expr::FnCall { name, args } if name == "_std" && matches!(args.first().map(|a| &a.node),
            Some(Expr::Literal(Literal::String(f))) if !ctx.handlers.contains(f.as_str())
                && (EFFECT_BUILTINS.contains(&f.as_str()) || crate::checker::names::EFFECT_BUILTINS.contains(&f.as_str()) || crate::checker::names::unstable_builtin(f)
                    || matches!(f.as_str(), "get_status" | "has_state" | "valid_transitions" | "horde" | "vote" | "next_id"))) => {
            let Some(Expr::Literal(Literal::String(f))) = args.first().map(|a| a.node.clone()) else { unreachable!() };
            e.node = Expr::FnCall { name: f, args: args[1..].to_vec() };
            fix_expr(e, ctx, extra);
            return;
        }
        Expr::FnCall { name, args } if name == "_coalesce" && args.len() == 2 => {
            fix_expr(&mut args[0], ctx, extra);
            fix_lazy(&mut args[1], ctx);
        }
        Expr::FnCall { name, args } => {
            for a in args.iter_mut() { fix_expr(a, ctx, extra); }
            // `delegate("Cell", "h", a…)` with literal names is a call of h(a…)
            // (not when a handler is named `delegate`: that handler is what a bare call runs)
            if name == "delegate" && args.len() >= 2 && !ctx.handlers.contains("delegate") {
                // (not when this cell has a SLOT of that name: `Cell.h(…)` would be read as the slot)
                if let (Expr::Literal(Literal::String(c)), Expr::Literal(Literal::String(h))) = (&args[0].node, &args[1].node) { if !ctx.slots.contains(c.as_str()) {
                    let target = Spanned::new(Expr::Ident(c.clone()), span);
                    replacement = Some(Expr::MethodCall { target: Box::new(target), method: h.clone(), args: args[2..].to_vec() });
                } }
            }
        }
        Expr::MethodCall { target, method, args } => {
            fix_expr(target, ctx, extra);
            for a in args.iter_mut() { fix_expr(a, ctx, extra); }
            let named = matches!(&target.node, Expr::Ident(t) if ctx.cells.contains(t) || ctx.slots.contains(t));
            // (`o.get_status()`, `d.sleep()`, `2.random()`: read by name like the rest)
            let seen_by_name = EFFECT_BUILTINS.contains(&method.as_str()) || crate::checker::names::EFFECT_BUILTINS.contains(&method.as_str())
                || matches!(method.as_str(), "get_status" | "has_state" | "valid_transitions") || crate::checker::names::unstable_builtin(method);
            if !named && (ctx.handlers.contains(method.as_str()) || seen_by_name) {
                let mut all = vec![(**target).clone()];
                all.extend(args.iter().cloned());
                replacement = Some(Expr::FnCall { name: method.clone(), args: all });
            }
        }
        // `a && b`, `a || b`, `a ?? b`: `b` runs or not — the calls its
        // strings make stay with it
        Expr::BinaryOp { left, op: BinOp::And | BinOp::Or, right } => {
            fix_expr(left, ctx, extra);
            fix_lazy(right, ctx);
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
            // `k |> c.set(x)` is `c.set(k, x)`, `x |> log.push()` is `log.push(x)`:
            // a slot write (it was "no handler writes to guarded slots")
            if let Expr::MethodCall { target, method, args } = &right.node {
                let mut all = vec![(**left).clone()];
                all.extend(args.iter().cloned());
                replacement = Some(Expr::MethodCall { target: target.clone(), method: method.clone(), args: all });
            }
        }
        // (a failed `try` undoes what its strings did with the rest)
        Expr::Try(i) => fix_lazy(i, ctx),
        Expr::Not(i) | Expr::TryPropagate(i) => fix_expr(i, ctx, extra),
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
                // a pattern's string is built to be compared: the calls in
                // its `{…}` run with the match (no analysis read them)
                let mut texts: Vec<String> = Vec::new();
                pattern_texts(&arm.pattern, &mut texts);
                // (the analysis that follows the order of what runs reads them in the match)
                if IN_PLACE.with(|f| f.get()) { texts.clear(); }
                for t in texts {
                    for seg in segments(&t) {
                        if let Some(x) = parse_segment(&seg) {
                            let mut sx = Spanned::new(x, span);
                            fix_expr(&mut sx, ctx, extra);
                            extra.push(sx);
                        }
                    }
                }
                if let Some(g) = arm.guard.as_mut() { fix_lazy(g, ctx); }
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
pub(crate) fn segments(s: &str) -> Vec<String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < b.len() {
        if b[pos] == b'{' && b.get(pos + 1) == Some(&b'{') { pos += 2; continue; }
        if b[pos] == b'}' && b.get(pos + 1) == Some(&b'}') { pos += 2; continue; }
        if b[pos] == b'{' {
            if let Some(end) = crate::interpreter::interp_segment_end(s, pos) {
                let seg = &s[pos + 1..pos + 1 + end];
                // a segment that does not parse is literal text and the
                // runtime rescans from the next byte: `"{ { wipe() } }"`
                // ran wipe() while every analysis skipped the whole thing
                if !crate::interpreter::interp_segment_is_literal(seg) && parse_segment(seg).is_some() {
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

pub(crate) fn parse_segment(seg: &str) -> Option<Expr> {
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

/// Every sub-expression of `e`, including those inside the interpolation
/// segments of its string literals (what the runtime evaluates).
pub(crate) fn for_each_deep(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    let mut inner: Vec<Expr> = Vec::new();
    crate::checker::literals::for_each_in_expr(e, &mut |x| {
        f(x);
        if let Expr::Literal(Literal::String(s)) = x {
            inner.extend(segments(s).iter().filter_map(|seg| parse_segment(seg)));
        }
    });
    for x in &inner { for_each_deep(x, f); }
}
