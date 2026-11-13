//! V1.6: effect-tracked tool dispatch from `think()`.
//!
//! For every handler that calls `think()`, statically compute the set of
//! tools that the LLM can dispatch from that call site. The set is the
//! cell's `face.tools` minus anything filtered by an explicit
//! `map("tools_allowed", ["t1", "t2"])` argument.
//!
//! This is a *diagnostic* effect summary — it tells the user what the
//! LLM can do, regardless of what the prompt says.

use crate::ast::*;
use std::collections::BTreeSet;

#[derive(Debug)]
pub struct HandlerEffects {
    pub handler: String,
    /// Tools the LLM can dispatch via think() inside this handler.
    pub think_tools: BTreeSet<String>,
    /// Number of think() / think_json() call sites in this handler.
    pub think_sites: usize,
}

pub fn check_cell(cell: &CellDef) -> Vec<HandlerEffects> {
    // Collect the cell's declared tools.
    let mut all_tools: BTreeSet<String> = BTreeSet::new();
    for section in &cell.sections {
        if let Section::Face(ref face) = section.node {
            for d in &face.declarations {
                if let FaceDecl::Tool(ref t) = d.node {
                    all_tools.insert(t.name.clone());
                }
            }
        }
    }

    let mut out = Vec::new();
    for section in &cell.sections {
        if let Section::OnSignal(ref handler) = section.node {
            let mut think_sites = 0usize;
            let mut narrowed: Option<BTreeSet<String>> = None;
            for stmt in &handler.body {
                visit_stmt(&stmt.node, &mut think_sites, &mut narrowed);
            }
            // a think() reached through anything this handler runs — a helper (and
            // the helpers it calls), a tool the model is allowed, an event some
            // listener hears, another cell that may call back: its own think()
            // offers every tool again, so this handler's options narrow nothing
            if think_sites > 0 {
                let body_of = |h: &str| cell.sections.iter().find_map(|s| match &s.node { Section::OnSignal(o) if o.signal_name == h => Some(&o.body), _ => None });
                let mut open = false;
                let mut seen: Vec<String> = vec![handler.signal_name.clone()];
                let mut work: Vec<String> = Vec::new();
                let scan = |body: &[Spanned<Statement>], first: bool, work: &mut Vec<String>, open: &mut bool| {
                    crate::checker::literals::for_each_expr(body, &mut |e| match e {
                        Expr::FnCall { name, .. } | Expr::MethodCall { method: name, .. } if matches!(name.as_str(), "think" | "think_json" | "vote" | "horde" | "delegate" | "resume" | "deliver") => { if !first || !matches!(name.as_str(), "think" | "think_json") { *open = true; } }
                        Expr::MethodCall { target, method, .. } => {
                            if matches!(&target.node, Expr::Ident(t) if crate::checker::names::cellish(t)) { *open = true; }
                            work.push(method.clone());
                        }
                        Expr::FnCall { name, .. } => work.push(name.clone()),
                        Expr::Literal(Literal::String(t)) => work.push(t.clone()),
                        _ => {}
                    });
                    crate::checker::literals::for_each_stmt_deep(body, &mut |st| match st {
                        Statement::Emit { .. } => *open = true,
                        Statement::MethodCall { target, .. } if crate::checker::names::cellish(target) => *open = true,
                        _ => {}
                    });
                };
                scan(&handler.body, true, &mut work, &mut open);
                if let Some(set) = &narrowed { work.extend(set.iter().cloned()); }
                while let Some(h) = work.pop() {
                    if open { break; }
                    if seen.contains(&h) { continue; }
                    seen.push(h.clone());
                    if let Some(body) = body_of(&h) { scan(body, false, &mut work, &mut open); }
                }
                if open { narrowed.get_or_insert_with(BTreeSet::new).insert(OPEN_SITE.to_string()); }
            }
            if think_sites > 0 {
                // (one site without a readable `tools_allowed` offers every tool)
                let tools = narrowed.filter(|s| !s.contains(OPEN_SITE)).unwrap_or_else(|| all_tools.clone());
                out.push(HandlerEffects {
                    handler: handler.signal_name.clone(),
                    think_tools: tools,
                    think_sites,
                });
            }
        }
    }
    out
}

fn visit_stmt(stmt: &Statement, count: &mut usize, narrow: &mut Option<BTreeSet<String>>) {
    match stmt {
        Statement::Let { value, .. }
        | Statement::Assign { value, .. }
        | Statement::Return { value }
        | Statement::Ensure { condition: value } => visit_expr(&value.node, count, narrow),
        Statement::ExprStmt { expr } => visit_expr(&expr.node, count, narrow),
        Statement::If { condition, then_body, else_body } => {
            visit_expr(&condition.node, count, narrow);
            for s in then_body { visit_stmt(&s.node, count, narrow); }
            for s in else_body { visit_stmt(&s.node, count, narrow); }
        }
        Statement::While { condition, body, .. } => {
            visit_expr(&condition.node, count, narrow);
            for s in body { visit_stmt(&s.node, count, narrow); }
        }
        Statement::For { iter, body, .. } => {
            visit_expr(&iter.node, count, narrow);
            for s in body { visit_stmt(&s.node, count, narrow); }
        }
        Statement::MethodCall { args, .. } | Statement::Emit { args, .. } => {
            for a in args { visit_expr(&a.node, count, narrow); }
        }
        // (`m["a"] = think(…)`, `require think(…) == "ok" else …`: a think() all the same)
        Statement::IndexSet { index, value, .. } => { visit_expr(&index.node, count, narrow); visit_expr(&value.node, count, narrow); }
        Statement::Require { constraint, .. } => {
            fn operands(c: &Constraint, count: &mut usize, narrow: &mut Option<BTreeSet<String>>) {
                match c {
                    Constraint::Comparison { left, right, .. } => { visit_expr(&left.node, count, narrow); visit_expr(&right.node, count, narrow); }
                    Constraint::Predicate { args, .. } => for a in args { visit_expr(&a.node, count, narrow); },
                    Constraint::And(a, b) | Constraint::Or(a, b) => { operands(&a.node, count, narrow); operands(&b.node, count, narrow); }
                    Constraint::Not(i) => operands(&i.node, count, narrow),
                    Constraint::Descriptive(_) => {}
                }
            }
            operands(&constraint.node, count, narrow);
        }
        _ => {}
    }
}

fn visit_expr(expr: &Expr, count: &mut usize, narrow: &mut Option<BTreeSet<String>>) {
    match expr {
        Expr::FnCall { name, args } => {
            if name == "think" || name == "think_json" {
                *count += 1;
                // Several think() sites union their narrowings; a site with no
                // options, or options this reading cannot list, offers every
                // tool — and stays so whatever the other sites say
                // (the options are the LAST argument, as the run reads them)
                let set = args.last().filter(|_| args.len() >= 2).and_then(|o| extract_tools_allowed(&o.node))
                    .unwrap_or_else(|| [OPEN_SITE.to_string()].into_iter().collect());
                match narrow {
                    Some(existing) => { for s in set { existing.insert(s); } }
                    None => *narrow = Some(set),
                }
            }
            for a in args { visit_expr(&a.node, count, narrow); }
        }
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            visit_expr(&left.node, count, narrow);
            visit_expr(&right.node, count, narrow);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => visit_expr(&i.node, count, narrow),
        Expr::FieldAccess { target, .. } => visit_expr(&target.node, count, narrow),
        Expr::Index { target, index } => { visit_expr(&target.node, count, narrow); visit_expr(&index.node, count, narrow); }
        Expr::MethodCall { target, args, .. } => {
            visit_expr(&target.node, count, narrow);
            for a in args { visit_expr(&a.node, count, narrow); }
        }
        Expr::Lambda { body, .. } => visit_expr(&body.node, count, narrow),
        Expr::LambdaBlock { stmts, result, .. } => {
            for s in stmts { visit_stmt(&s.node, count, narrow); }
            visit_expr(&result.node, count, narrow);
        }
        Expr::Match { subject, arms } => {
            visit_expr(&subject.node, count, narrow);
            for arm in arms {
                if let Some(g) = &arm.guard { visit_expr(&g.node, count, narrow); }
                for s in &arm.body { visit_stmt(&s.node, count, narrow); }
                visit_expr(&arm.result.node, count, narrow);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            visit_expr(&condition.node, count, narrow);
            for s in then_body { visit_stmt(&s.node, count, narrow); }
            visit_expr(&then_result.node, count, narrow);
            for s in else_body { visit_stmt(&s.node, count, narrow); }
            visit_expr(&else_result.node, count, narrow);
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields { visit_expr(&v.node, count, narrow); }
        }
        Expr::ListLiteral(items) => {
            for it in items { visit_expr(&it.node, count, narrow); }
        }
        _ => {}
    }
}

/// In a narrowing: a think() site that offers every tool.
const OPEN_SITE: &str = "\u{1f}every tool";

fn extract_tools_allowed(opts: &Expr) -> Option<BTreeSet<String>> {
    if let Expr::FnCall { name, args } = opts {
        if name != "map" { return None; }
        // the key written twice, or under a computed name: not read
        let keys = args.iter().step_by(2).filter(|a| matches!(&a.node, Expr::Literal(Literal::String(k)) if k == "tools_allowed")).count();
        if keys != 1 || args.iter().step_by(2).any(|a| !matches!(a.node, Expr::Literal(Literal::String(_)))) { return None; }
        let mut i = 0;
        while i + 1 < args.len() {
            if let Expr::Literal(Literal::String(k)) = &args[i].node {
                if k == "tools_allowed" {
                    let items: Option<Vec<&Expr>> = match &args[i + 1].node {
                        Expr::ListLiteral(items) =>
                            Some(items.iter().map(|s| &s.node).collect()),
                        Expr::FnCall { name, args: largs } if name == "list" =>
                            Some(largs.iter().map(|s| &s.node).collect()),
                        _ => None,
                    };
                    if let Some(items) = items {
                        // (an element that is not a literal name may be any tool)
                        return items.iter().map(|it| match it {
                            Expr::Literal(Literal::String(s)) if crate::checker::desugar::segments(s).is_empty() => Some(s.clone()),
                            _ => None,
                        }).collect();
                    }
                }
            }
            i += 2;
        }
    }
    None
}
