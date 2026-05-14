//! V1.6: model capability contracts on `think()`.
//!
//! `think(prompt, map("requires", ["json_mode", "tools"]))` declares the
//! capabilities the handler needs from the LLM. This pass walks every
//! `think()` / `think_json()` call in a cell, extracts the required
//! capability list, resolves the cell's model (via `agent_model` + soma.toml,
//! or a built-in default table), and emits an error for every required
//! capability not satisfied by the configured model.
//!
//! Without this check, `SOMA_LLM_MODEL` is a runtime coin flip.

use crate::ast::*;
use crate::pkg::manifest::Manifest;
use std::collections::HashSet;

#[derive(Debug)]
pub struct CapabilityError {
    pub handler_name: String,
    pub model: String,
    pub missing: Vec<String>,
}

impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "handler '{}' calls think() requiring {:?}, but model '{}' does not advertise {:?}",
            self.handler_name, self.missing, self.model, self.missing,
        )
    }
}

/// Built-in capability table for popular LLMs.
/// Users can override / extend via `[models.<name>] capabilities = [...]`.
fn known_capabilities(model: &str) -> HashSet<&'static str> {
    let m = model.to_lowercase();
    let mut caps: HashSet<&'static str> = HashSet::new();
    // OpenAI
    if m.starts_with("gpt-4o") || m.starts_with("gpt-4-turbo") || m.starts_with("gpt-4.1") {
        caps.extend(["tools", "json_mode", "structured_output", "vision", "long_context"]);
    } else if m.starts_with("gpt-4") {
        caps.extend(["tools", "json_mode"]);
    } else if m.starts_with("gpt-3.5") {
        caps.extend(["tools", "json_mode"]);
    } else if m.starts_with("o1") || m.starts_with("o3") {
        caps.extend(["reasoning", "tools", "structured_output"]);
    }
    // Anthropic
    if m.starts_with("claude-opus") || m.starts_with("claude-sonnet")
        || m.starts_with("claude-haiku") || m.starts_with("claude-3")
        || m.starts_with("claude-4") {
        caps.extend(["tools", "json_mode", "vision", "long_context", "structured_output"]);
    }
    // Ollama / open
    if m.contains("llama-3") || m.contains("llama3") {
        caps.extend(["tools", "long_context"]);
    }
    if m.starts_with("gemma") {
        // Gemma 3 introduced JSON mode and tool calling in some sizes
        caps.extend(["json_mode"]);
    }
    if m.contains("vision") || m.contains("vl") {
        caps.insert("vision");
    }
    caps
}

fn model_capabilities(
    cell_model: Option<&str>,
    manifest: Option<&Manifest>,
) -> (String, HashSet<String>) {
    let mut caps: HashSet<String> = HashSet::new();
    let mut model_name = String::new();

    if let Some(m) = manifest {
        let cfg = cell_model
            .and_then(|n| m.models.get(n))
            .unwrap_or(&m.agent);
        model_name = cfg.resolve_model();
        // From soma.toml — explicit declarations override defaults
        caps.extend(cfg.capabilities.iter().cloned());
    }
    if model_name.is_empty() {
        // No manifest or no resolved model — fall back to env var,
        // then to a conservative default.
        if let Ok(env_model) = std::env::var("SOMA_LLM_MODEL") {
            model_name = env_model;
        } else {
            model_name = "gpt-4o-mini".to_string();
        }
    }
    for c in known_capabilities(&model_name) {
        caps.insert(c.to_string());
    }
    (model_name, caps)
}

pub fn check_cell(cell: &CellDef, manifest: Option<&Manifest>) -> Vec<CapabilityError> {
    let (model_name, model_caps) =
        model_capabilities(cell.agent_model.as_deref(), manifest);

    let mut errs = Vec::new();
    for section in &cell.sections {
        if let Section::OnSignal(ref handler) = section.node {
            let mut visitor = ThinkVisitor::default();
            for stmt in &handler.body {
                visitor.visit_stmt(&stmt.node);
            }
            for required in visitor.required_caps {
                let missing: Vec<String> = required
                    .iter()
                    .filter(|c| !model_caps.contains(*c))
                    .cloned()
                    .collect();
                if !missing.is_empty() {
                    errs.push(CapabilityError {
                        handler_name: handler.signal_name.clone(),
                        model: model_name.clone(),
                        missing,
                    });
                }
            }
        }
    }
    errs
}

/// Collects every `think(..., map("requires", [...]))` requires-list in a
/// handler body. One entry per `think()` call site.
#[derive(Default)]
struct ThinkVisitor {
    required_caps: Vec<Vec<String>>,
}

impl ThinkVisitor {
    fn visit_stmt(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Let { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Return { value }
            | Statement::Ensure { condition: value } => self.visit_expr(&value.node),
            Statement::ExprStmt { expr } => self.visit_expr(&expr.node),
            Statement::If { condition, then_body, else_body } => {
                self.visit_expr(&condition.node);
                for s in then_body { self.visit_stmt(&s.node); }
                for s in else_body { self.visit_stmt(&s.node); }
            }
            Statement::While { condition, body, .. } => {
                self.visit_expr(&condition.node);
                for s in body { self.visit_stmt(&s.node); }
            }
            Statement::For { iter, body, .. } => {
                self.visit_expr(&iter.node);
                for s in body { self.visit_stmt(&s.node); }
            }
            Statement::MethodCall { args, .. } => {
                for a in args { self.visit_expr(&a.node); }
            }
            Statement::Emit { args, .. } => {
                for a in args { self.visit_expr(&a.node); }
            }
            _ => {}
        }
    }

    fn visit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::FnCall { name, args } => {
                if (name == "think" || name == "think_json") && args.len() >= 2 {
                    if let Some(caps) = extract_required_caps(&args[1].node) {
                        self.required_caps.push(caps);
                    }
                }
                for a in args { self.visit_expr(&a.node); }
            }
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. }
            | Expr::Pipe { left, right } => {
                self.visit_expr(&left.node);
                self.visit_expr(&right.node);
            }
            Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => self.visit_expr(&i.node),
            Expr::Lambda { body, .. } => self.visit_expr(&body.node),
            Expr::LambdaBlock { stmts, result, .. } => {
                for s in stmts { self.visit_stmt(&s.node); }
                self.visit_expr(&result.node);
            }
            Expr::FieldAccess { target, .. } => self.visit_expr(&target.node),
            Expr::MethodCall { target, args, .. } => {
                self.visit_expr(&target.node);
                for a in args { self.visit_expr(&a.node); }
            }
            Expr::Match { subject, arms } => {
                self.visit_expr(&subject.node);
                for arm in arms {
                    if let Some(g) = &arm.guard { self.visit_expr(&g.node); }
                    for s in &arm.body { self.visit_stmt(&s.node); }
                    self.visit_expr(&arm.result.node);
                }
            }
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                self.visit_expr(&condition.node);
                for s in then_body { self.visit_stmt(&s.node); }
                self.visit_expr(&then_result.node);
                for s in else_body { self.visit_stmt(&s.node); }
                self.visit_expr(&else_result.node);
            }
            Expr::Record { fields, .. } => {
                for (_, v) in fields { self.visit_expr(&v.node); }
            }
            Expr::ListLiteral(items) => {
                for it in items { self.visit_expr(&it.node); }
            }
            _ => {}
        }
    }
}

/// The `options` argument of `think()` is built with `map("k", v, "k2", v2, ...)`.
/// Find the `"requires"` key and return its string-list value.
fn extract_required_caps(opts: &Expr) -> Option<Vec<String>> {
    if let Expr::FnCall { name, args } = opts {
        if name != "map" { return None; }
        let mut i = 0;
        while i + 1 < args.len() {
            if let Expr::Literal(Literal::String(k)) = &args[i].node {
                if k == "requires" {
                    // Accept both [a, b] (ListLiteral) and list(a, b) (FnCall) shapes.
                    let items: Option<Vec<&Expr>> = match &args[i + 1].node {
                        Expr::ListLiteral(items) =>
                            Some(items.iter().map(|s| &s.node).collect()),
                        Expr::FnCall { name, args: largs } if name == "list" =>
                            Some(largs.iter().map(|s| &s.node).collect()),
                        _ => None,
                    };
                    if let Some(items) = items {
                        let out: Vec<String> = items.iter().filter_map(|it| {
                            if let Expr::Literal(Literal::String(s)) = it {
                                Some(s.clone())
                            } else { None }
                        }).collect();
                        return Some(out);
                    }
                }
            }
            i += 2;
        }
    }
    None
}
