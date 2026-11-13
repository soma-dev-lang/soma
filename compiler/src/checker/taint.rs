//! Untrusted values (3.0, spec B6). What a model answers — and what an HTTP
//! call returns — is data an outsider can shape: a prompt-injected invoice
//! decides what `think()` says. Such a value may be stored, shown and
//! computed with; it may not DECIDE without a check: which state an
//! instance moves to, which URL is fetched, which file is opened, how large
//! the token budget is.
//!
//! The rule, per handler, in statement order: a value is untrusted when it
//! comes from think() / think_json() / http_*() / read_stdin(), or is
//! computed from one. It is cleared once the handler tests it — it appears
//! in a `require`, an `if` / `while` condition or a `match` subject — or
//! hands it to another handler (whose parameters are typed and whose own
//! body is checked). An untrusted value reaching a sink uncleared is
//! reported: a warning in edition 2026, an error in 2027.

use crate::ast::*;
use std::collections::{HashMap, HashSet};

pub struct TaintIssue {
    pub message: String,
    pub span: Span,
}

const SOURCES: &[&str] = &["think", "think_json", "http_get", "http_post", "http_put", "http_patch", "http_delete", "read_stdin",
    // the winner of a vote among model answers is a model's answer
    "vote"];

/// Builtins that apply a lambda to the elements of their first argument.
const APPLIES: &[&str] = &["map", "filter", "find", "any", "all", "count", "reduce", "sort_by", "group_by", "min_by", "max_by", "sum_by", "each", "flat_map"];

/// Methods that put their argument into the value they are called on.
const STORES: &[&str] = &["push", "append", "set", "put", "insert", "extend"];

/// (builtin, argument index, what the argument decides)
const SINKS: &[(&str, usize, &str)] = &[
    ("http_get", 0, "the URL fetched"), ("http_post", 0, "the URL posted to"), ("http_put", 0, "the URL written to"),
    ("http_patch", 0, "the URL written to"), ("http_delete", 0, "the URL deleted"),
    ("read_file", 0, "the file read"), ("write_file", 0, "the file written"), ("read_csv", 0, "the file read"),
    ("write_csv", 0, "the file written"), ("load", 0, "the file loaded"), ("include", 0, "the file included"),
    ("set_budget", 0, "the token budget"),
];

struct Cx<'a> {
    handlers: &'a HashSet<String>,
    /// untrusted local → where its value came from
    tainted: HashMap<String, String>,
    out: Vec<TaintIssue>,
}

/// Every name an expression reads.
fn idents(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::Ident(n) => out.push(n.clone()),
        Expr::Literal(_) => {}
        Expr::FieldAccess { target, .. } | Expr::Not(target) | Expr::Try(target) | Expr::TryPropagate(target) => idents(&target.node, out),
        Expr::Index { target, index } => { idents(&target.node, out); idents(&index.node, out); }
        Expr::MethodCall { target, args, .. } => { idents(&target.node, out); for a in args { idents(&a.node, out); } }
        Expr::FnCall { args, .. } | Expr::ListLiteral(args) => for a in args { idents(&a.node, out); },
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => { idents(&left.node, out); idents(&right.node, out); }
        Expr::Record { fields, .. } => for (_, v) in fields { idents(&v.node, out); },
        Expr::Match { subject, .. } => idents(&subject.node, out),
        Expr::IfExpr { condition, .. } => idents(&condition.node, out),
        Expr::Lambda { .. } | Expr::LambdaBlock { .. } => {}
    }
}

impl<'a> Cx<'a> {
    /// Where an expression's value comes from, when that is untrusted.
    fn source(&self, e: &Expr) -> Option<String> {
        match e {
            Expr::Literal(Literal::String(s)) if s.contains('{') => {
                // "…{x}…": the text carries x
                self.tainted.iter().find(|(name, _)| interpolates(s, name)).map(|(_, src)| src.clone())
            }
            Expr::Literal(_) => None,
            Expr::Ident(n) => self.tainted.get(n).cloned(),
            Expr::FnCall { name, .. } if SOURCES.contains(&name.as_str()) => Some(format!("{name}()")),
            // another handler is a boundary: typed parameters, its own checks
            Expr::FnCall { name, .. } if self.handlers.contains(name) => None,
            Expr::FnCall { args, .. } | Expr::ListLiteral(args) => args.iter().find_map(|a| self.source(&a.node)),
            Expr::MethodCall { target, method, args } => {
                if matches!(&target.node, Expr::Ident(n) if super::names::cellish(n)) || self.handlers.contains(method) {
                    return None;
                }
                self.source(&target.node).or_else(|| args.iter().find_map(|a| self.source(&a.node)))
            }
            Expr::FieldAccess { target, .. } | Expr::Try(target) | Expr::TryPropagate(target) => self.source(&target.node),
            Expr::Index { target, index } => self.source(&target.node).or_else(|| self.source(&index.node)),
            Expr::BinaryOp { left, op, right } if !matches!(op, BinOp::And | BinOp::Or) => self.source(&left.node).or_else(|| self.source(&right.node)),
            // a comparison's result is the handler's own verdict
            Expr::BinaryOp { .. } | Expr::CmpOp { .. } | Expr::Not(_) => None,
            Expr::Record { fields, .. } => fields.iter().find_map(|(_, v)| self.source(&v.node)),
            Expr::Pipe { left, .. } => self.source(&left.node),
            Expr::IfExpr { then_result, else_result, .. } => self.source(&then_result.node).or_else(|| self.source(&else_result.node)),
            Expr::Match { arms, .. } => arms.iter().find_map(|a| self.source(&a.result.node)),
            Expr::Lambda { .. } | Expr::LambdaBlock { .. } => None,
        }
    }

    /// The handler tested these names: what they hold is now its decision.
    fn clear(&mut self, e: &Expr) {
        let mut names = Vec::new();
        idents(e, &mut names);
        for n in names { self.tainted.remove(&n); }
    }

    fn sink(&mut self, what: &str, arg: &Spanned<Expr>, call: &str, fix: &str) {
        if let Some(src) = self.source(&arg.node) {
            if self.out.iter().any(|i| i.span.start == arg.span.start) { return; }
            self.out.push(TaintIssue {
                span: arg.span,
                message: format!(
                    "`{}` in {call} decides {what}, and its value comes from {src} with no check in between — whoever shapes that answer (a prompt-injected document, a remote server) chooses it; {fix}",
                    render_expr(&arg.node)),
            });
        }
    }

    /// `xs.map(x => …)` / `map(xs, x => …)` over an untrusted `xs`: `x` is
    /// the outsider's too, inside the lambda.
    fn applied(&mut self, collection: &Expr, lambda: &Spanned<Expr>) -> bool {
        let Some(src) = self.source(collection) else { return false };
        let param = match &lambda.node { Expr::Lambda { param, .. } | Expr::LambdaBlock { param, .. } => param.clone(), _ => return false };
        let before = self.tainted.clone();
        for p in param.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|p| !p.is_empty()) { self.tainted.insert(p.to_string(), src.clone()); }
        self.expr(lambda);
        self.tainted = before;
        true
    }

    /// `xs.push(v)`, `m.set(k, v)` with an untrusted `v`: the local `xs` /
    /// `m` holds the outsider's value from here on.
    fn stored(&mut self, target: &str, method: &str, args: &[Spanned<Expr>]) {
        if !STORES.contains(&method) || self.handlers.contains(method) { return; }
        if let Some(src) = args.iter().find_map(|a| self.source(&a.node)) { self.tainted.insert(target.to_string(), src); }
    }

    fn expr(&mut self, e: &Spanned<Expr>) {
        match &e.node {
            Expr::FnCall { name, args } if APPLIES.contains(&name.as_str()) && !self.handlers.contains(name) && args.len() >= 2
                && matches!(args.last().map(|a| &a.node), Some(Expr::Lambda { .. } | Expr::LambdaBlock { .. })) => {
                for a in &args[..args.len() - 1] { self.expr(a); }
                let last = &args[args.len() - 1];
                if !self.applied(&args[0].node, last) { self.expr(last); }
            }
            Expr::MethodCall { target, method, args } if APPLIES.contains(&method.as_str()) && !self.handlers.contains(method)
                && matches!(args.last().map(|a| &a.node), Some(Expr::Lambda { .. } | Expr::LambdaBlock { .. })) => {
                self.expr(target);
                for a in &args[..args.len() - 1] { self.expr(a); }
                let last = &args[args.len() - 1];
                if !self.applied(&target.node, last) { self.expr(last); }
            }
            Expr::FnCall { name, args } => {
                for a in args { self.expr(a); }
                if name == "transition" && !self.handlers.contains("transition") {
                    if let Some(target) = if args.len() >= 3 { args.get(2) } else { args.get(1) } {
                        self.sink("the state an instance moves to", target, "transition()",
                            "match it against the states you allow (`match next { \"approved\" -> transition(id, \"approved\")  _ -> fail(\"bad_state\", next) }`) or `require contains([…], next) else BadState`");
                    }
                }
                if !self.handlers.contains(name) {
                    for (sink, idx, what) in SINKS {
                        if name == sink {
                            if let Some(arg) = args.get(*idx) {
                                self.sink(what, arg, &format!("{name}()"),
                                    "test it first (`require starts_with(url, \"https://api.example.com/\") else BadTarget`, a `match` on the values you allow), or build it from your own data");
                            }
                        }
                    }
                }
            }
            Expr::MethodCall { target, method, args } => {
                self.expr(target);
                for a in args { self.expr(a); }
                if let Expr::Ident(t) = &target.node { self.stored(t, method, args); }
            }
            Expr::FieldAccess { target, .. } | Expr::Not(target) | Expr::Try(target) | Expr::TryPropagate(target) => self.expr(target),
            Expr::Index { target, index } => { self.expr(target); self.expr(index); }
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => { self.expr(left); self.expr(right); }
            Expr::Record { fields, .. } => for (_, v) in fields { self.expr(v); },
            Expr::ListLiteral(items) => for i in items { self.expr(i); },
            Expr::Match { subject, arms } => {
                self.expr(subject);
                let from = self.source(&subject.node);
                // matching IS the test of the subject…
                self.clear(&subject.node);
                for arm in arms {
                    let before = self.tainted.clone();
                    // …but what an arm binds out of it is still the outsider's
                    if let Some(src) = &from {
                        let mut bound = Vec::new();
                        pattern_bindings(&arm.pattern, &mut bound);
                        for b in bound { self.tainted.insert(b, src.clone()); }
                    }
                    if let Some(g) = &arm.guard { self.expr(g); self.clear(&g.node); }
                    self.block(&arm.body);
                    self.expr(&arm.result);
                    self.tainted = before;
                }
            }
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                self.expr(condition);
                self.clear(&condition.node);
                self.block(then_body); self.expr(then_result);
                self.block(else_body); self.expr(else_result);
            }
            Expr::Lambda { body, .. } => self.expr(body),
            Expr::LambdaBlock { stmts, result, .. } => { self.block(stmts); self.expr(result); }
            Expr::Literal(_) | Expr::Ident(_) => {}
        }
    }

    fn block(&mut self, body: &[Spanned<Statement>]) {
        for s in body { self.stmt(&s.node); }
    }

    fn stmt(&mut self, s: &Statement) {
        match s {
            Statement::Let { name, value } | Statement::Assign { name, value } => {
                self.expr(value);
                match self.source(&value.node) {
                    Some(src) => { self.tainted.insert(name.clone(), src); }
                    None => { self.tainted.remove(name); }
                }
            }
            Statement::Return { value } | Statement::ExprStmt { expr: value } => self.expr(value),
            Statement::If { condition, then_body, else_body } => {
                self.expr(condition);
                self.clear(&condition.node);
                self.block(then_body);
                self.block(else_body);
            }
            Statement::For { var, iter, body, .. } => {
                self.expr(iter);
                match self.source(&iter.node) {
                    Some(src) => { self.tainted.insert(var.clone(), src); }
                    None => { self.tainted.remove(var); }
                }
                self.block(body);
            }
            Statement::While { condition, body, .. } => {
                self.expr(condition);
                self.clear(&condition.node);
                self.block(body);
            }
            Statement::Emit { args, .. } => for a in args { self.expr(a); },
            Statement::MethodCall { target, method, args } => {
                for a in args { self.expr(a); }
                self.stored(target, method, args);
            }
            Statement::Require { constraint, .. } => self.constraint(&constraint.node),
            Statement::IndexSet { name, index, value } => {
                self.expr(index); self.expr(value);
                // `m["a"] = think(q)`: `m` holds the outsider's value
                if let Some(src) = self.source(&value.node) {
                    let root = name.split(|c| c == '.' || c == '[').next().unwrap_or(name).to_string();
                    self.tainted.insert(root, src);
                }
            }
            Statement::Ensure { condition } => { self.expr(condition); self.clear(&condition.node); }
            Statement::Break | Statement::Continue => {}
        }
    }

    fn constraint(&mut self, c: &Constraint) {
        match c {
            Constraint::Comparison { left, right, .. } => {
                self.expr(left); self.expr(right);
                self.clear(&left.node); self.clear(&right.node);
            }
            Constraint::Predicate { args, .. } => for a in args { self.expr(a); self.clear(&a.node); },
            Constraint::And(a, b) | Constraint::Or(a, b) => { self.constraint(&a.node); self.constraint(&b.node); }
            Constraint::Not(a) => self.constraint(&a.node),
            Constraint::Descriptive(_) => {}
        }
    }
}

/// Does the text interpolate `name` (`{name}`, `{name.field}`, `{f(name)}`)?
fn interpolates(text: &str, name: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let (mut i, mut depth) = (0usize, 0usize);
    let word: Vec<char> = name.chars().collect();
    while i < chars.len() {
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') && depth == 0 => { i += 2; continue; }
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ if depth > 0 && chars[i..].starts_with(&word) => {
                let before_ok = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '.');
                let after = chars.get(i + word.len());
                if before_ok && !after.map_or(false, |c| c.is_alphanumeric() || *c == '_') { return true; }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

fn pattern_bindings(p: &MatchPattern, out: &mut Vec<String>) {
    match p {
        MatchPattern::Variable(n) => out.push(n.clone()),
        MatchPattern::Or(ps) => for q in ps { pattern_bindings(q, out); },
        MatchPattern::MapDestructure(fs) => for (_, q) in fs { pattern_bindings(q, out); },
        MatchPattern::StringPrefix { rest, .. } => out.push(rest.clone()),
        MatchPattern::Variant { fields, .. } => match fields {
            VariantPatternFields::Unit => {}
            VariantPatternFields::Tuple(ps) => for q in ps { pattern_bindings(q, out); },
            VariantPatternFields::Struct { fields, .. } => for (_, q) in fields { pattern_bindings(q, out); },
        },
        MatchPattern::Literal(_) | MatchPattern::Wildcard | MatchPattern::Range { .. } => {}
    }
}

pub fn check_program(program: &Program) -> Vec<TaintIssue> {
    fn names(cell: &CellDef, out: &mut HashSet<String>) {
        for s in &cell.sections {
            match &s.node {
                Section::OnSignal(h) => { out.insert(h.signal_name.clone()); }
                Section::Interior(i) => for c in &i.cells { names(&c.node, out); },
                _ => {}
            }
        }
    }
    fn walk(cell: &CellDef, handlers: &HashSet<String>, out: &mut Vec<TaintIssue>) {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) { return; }
        for s in &cell.sections {
            let body: &[Spanned<Statement>] = match &s.node {
                Section::OnSignal(h) => &h.body,
                Section::Every(e) | Section::After(e) => &e.body,
                Section::Interior(i) => { for c in &i.cells { walk(&c.node, handlers, out); } continue; }
                _ => continue,
            };
            let mut cx = Cx { handlers, tainted: HashMap::new(), out: Vec::new() };
            cx.block(body);
            out.extend(cx.out);
        }
    }
    let mut handlers = HashSet::new();
    for c in &program.cells { names(&c.node, &mut handlers); }
    let mut out = Vec::new();
    for c in &program.cells { walk(&c.node, &handlers, &mut out); }
    out
}
