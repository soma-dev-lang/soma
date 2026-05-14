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
            if think_sites > 0 {
                let tools = narrowed.unwrap_or_else(|| all_tools.clone());
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
        _ => {}
    }
}

fn visit_expr(expr: &Expr, count: &mut usize, narrow: &mut Option<BTreeSet<String>>) {
    match expr {
        Expr::FnCall { name, args } => {
            if name == "think" || name == "think_json" {
                *count += 1;
                if args.len() >= 2 {
                    if let Some(set) = extract_tools_allowed(&args[1].node) {
                        // Multiple think() sites union their narrowings;
                        // an unnarrowed site is permissive (no filter).
                        match narrow {
                            Some(existing) => { for s in set { existing.insert(s); } }
                            None => *narrow = Some(set),
                        }
                    } else {
                        // unnarrowed site → permissive
                        *narrow = None;
                    }
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

fn extract_tools_allowed(opts: &Expr) -> Option<BTreeSet<String>> {
    if let Expr::FnCall { name, args } = opts {
        if name != "map" { return None; }
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
                        return Some(items.iter().filter_map(|it| {
                            if let Expr::Literal(Literal::String(s)) = it {
                                Some(s.clone())
                            } else { None }
                        }).collect());
                    }
                }
            }
            i += 2;
        }
    }
    None
}
