//! V1.6: cost-budget proof obligation — extends [V1.4 memory budget][budget]
//! with token, latency, and USD bounds.
//!
//! `cost { tokens: 5000, latency: 30s, usd: 0.10 }` is no longer advisory —
//! `soma check` walks every `think()` call site, sums the declared
//! `max_tokens` across handlers (max, not sum — only one runs at a time),
//! aggregates latency from `timeout`, estimates USD from a per-model price
//! table, and refuses to build if any axis exceeds the declared budget.
//!
//! Limitations (deferred):
//!   - delegate() into other cells doesn't compose costs yet.
//!   - http_get latency adds to think() latency.
//!   - USD price table is a small hardcoded map; users override via
//!     `[models.<name>] usd_per_1k_input = 1.50` in soma.toml (future).

use crate::ast::*;
use crate::pkg::manifest::Manifest;

#[derive(Debug)]
pub enum CostFinding {
    Exceeded { axis: &'static str, declared: i64, computed: i64, unit: &'static str, where_: String },
    Advisory { axis: &'static str, reason: String },
    Proven { axis: &'static str, computed: i64, declared: i64, unit: &'static str },
}

impl std::fmt::Display for CostFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CostFinding::Exceeded { axis, declared, computed, unit, where_ } =>
                write!(f, "cost: '{}' budget exceeded — computed {} {} > declared {} {} ({})",
                       axis, computed, unit, declared, unit, where_),
            CostFinding::Advisory { axis, reason } =>
                write!(f, "cost: '{}' bound is advisory — {}", axis, reason),
            CostFinding::Proven { axis, computed, declared, unit } =>
                if *axis == "tokens" {
                    write!(f, "cost: 'tokens' bound proven — peak {} reply tokens (the max_tokens caps) ≤ declared {} (prompts are not counted: cap them with set_budget)",
                           computed, declared)
                } else {
                    write!(f, "cost: '{}' bound proven — peak {} {} ≤ declared {} {}",
                           axis, computed, unit, declared, unit)
                },
        }
    }
}

/// Walk every think() / think_json() call in a handler and return the
/// (sum_max_tokens, max_timeout_ms) pair. Unbounded calls (no options
/// map, or missing max_tokens) make the totals partial and return a
/// `is_partial = true` flag.
struct CostWalk<'a> {
    tokens: i64,
    latency_ms: i64,
    unbounded_sites: Vec<String>,
    /// Sibling handlers of the cell, by name — a call to one of them
    /// spends whatever its body spends.
    handlers: &'a std::collections::HashMap<String, &'a [Spanned<Statement>]>,
    /// Handlers currently being expanded (recursion guard).
    stack: Vec<String>,
}

impl<'a> CostWalk<'a> {
    fn new(handlers: &'a std::collections::HashMap<String, &'a [Spanned<Statement>]>) -> Self {
        Self { tokens: 0, latency_ms: 0, unbounded_sites: Vec::new(), handlers, stack: Vec::new() }
    }

    /// A fresh accumulator for a nested scope (loop body, lambda, callee).
    fn child(&self) -> CostWalk<'a> {
        CostWalk {
            tokens: 0,
            latency_ms: 0,
            unbounded_sites: Vec::new(),
            handlers: self.handlers,
            stack: self.stack.clone(),
        }
    }

    fn spends(&self) -> bool {
        self.tokens > 0 || self.latency_ms > 0 || !self.unbounded_sites.is_empty()
    }

    fn visit_stmt(&mut self, stmt: &Statement, handler_name: &str) {
        match stmt {
            Statement::Let { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Return { value }
            | Statement::Ensure { condition: value } => self.visit_expr(&value.node, handler_name),
            Statement::ExprStmt { expr } => self.visit_expr(&expr.node, handler_name),
            Statement::If { condition, then_body, else_body } => {
                self.visit_expr(&condition.node, handler_name);
                // Conservative: assume both branches taken (max).
                // For MVP we take the union — same as summing — to stay
                // sound for the worst case.
                for s in then_body { self.visit_stmt(&s.node, handler_name); }
                for s in else_body { self.visit_stmt(&s.node, handler_name); }
            }
            Statement::While { condition, body, bound, .. } => {
                self.visit_expr(&condition.node, handler_name);
                // Unbounded while → mark advisory.
                if bound.is_none() {
                    self.unbounded_sites.push(format!("{}::while-loop", handler_name));
                }
                let mult = bound.unwrap_or(1) as i64;
                let mut inner = self.child();
                for s in body { inner.visit_stmt(&s.node, handler_name); }
                self.tokens += inner.tokens.saturating_mul(mult);
                // Latency in a loop is sequential — multiply.
                self.latency_ms += inner.latency_ms.saturating_mul(mult);
                self.unbounded_sites.extend(inner.unbounded_sites);
            }
            Statement::For { iter, body, bound, .. } => {
                self.visit_expr(&iter.node, handler_name);
                let mut inner = self.child();
                for s in body { inner.visit_stmt(&s.node, handler_name); }
                // Iteration count: [loop_bound(N)], else a literal range.
                // Anything else (a list, a computed range) is unknown: the
                // x100 figure below is then an ESTIMATE, and a body that
                // spends makes the whole bound advisory, not proven.
                let known = bound.map(|b| b as i64).or_else(|| literal_range_len(&iter.node));
                if known.is_none() && inner.spends() {
                    self.unbounded_sites.push(format!(
                        "{}::for-loop over a collection of unknown size (add [loop_bound(N)])",
                        handler_name
                    ));
                }
                let mult = known.unwrap_or(100);
                self.tokens += inner.tokens.saturating_mul(mult);
                self.latency_ms += inner.latency_ms.saturating_mul(mult);
                self.unbounded_sites.extend(inner.unbounded_sites);
            }
            Statement::MethodCall { args, .. } => {
                for a in args { self.visit_expr(&a.node, handler_name); }
            }
            // `emit ev(…)` runs every `on ev` of the process, synchronously:
            // their think() calls are this handler's spend too
            Statement::Emit { signal_name, args } => {
                for a in args { self.visit_expr(&a.node, handler_name); }
                let suffix = format!(".{}", signal_name);
                let mut listeners: Vec<String> = self.handlers.keys().filter(|k| k.ends_with(&suffix)).cloned().collect();
                listeners.sort();
                for key in listeners {
                    let Some(body) = self.handlers.get(key.as_str()).copied() else { continue };
                    if self.stack.iter().any(|h| h == &key) { continue; }
                    let mut callee = self.child();
                    callee.stack.push(key.clone());
                    for s in body { callee.visit_stmt(&s.node, &key); }
                    self.tokens += callee.tokens;
                    self.latency_ms += callee.latency_ms;
                    self.unbounded_sites.extend(callee.unbounded_sites);
                }
            }
            _ => {}
        }
    }

    fn visit_expr(&mut self, expr: &Expr, handler_name: &str) {
        match expr {
            Expr::FnCall { name, args } => {
                if name == "think" || name == "think_json" {
                    let (max_tokens, timeout_ms) = extract_think_opts(args);
                    match max_tokens {
                        Some(t) => self.tokens += t,
                        None => self.unbounded_sites.push(format!("{}::think (no max_tokens)", handler_name)),
                    }
                    self.latency_ms += timeout_ms.unwrap_or(30_000);
                }
                if matches!(name.as_str(), "http_get" | "http_post" | "http_put" | "http_delete") {
                    let timeout = args.get(1).and_then(|a| extract_timeout_ms(&a.node));
                    self.latency_ms += timeout.unwrap_or(10_000);
                }
                for a in args { self.visit_expr(&a.node, handler_name); }
                // `delegate("Cell", "handler", …)` with literal names runs
                // that handler: it spends what the handler spends
                let delegated: Option<String> = if name == "delegate" && args.len() >= 2 {
                    match (&args[0].node, &args[1].node) {
                        (Expr::Literal(Literal::String(c)), Expr::Literal(Literal::String(h))) => Some(format!("{}.{}", c, h)),
                        _ => { self.unbounded_sites.push(format!("{}::delegate to a non-literal target", handler_name)); None }
                    }
                } else { None };
                let name: &String = delegated.as_ref().unwrap_or(name);
                // A call to a sibling handler spends what its body spends.
                if let Some(body) = self.handlers.get(name.as_str()).copied() {
                    if self.stack.iter().any(|h| h == name) {
                        self.unbounded_sites.push(format!("{}::recursive call to {}", handler_name, name));
                    } else {
                        let mut callee = self.child();
                        callee.stack.push(name.clone());
                        for s in body { callee.visit_stmt(&s.node, name); }
                        self.tokens += callee.tokens;
                        self.latency_ms += callee.latency_ms;
                        self.unbounded_sites.extend(callee.unbounded_sites);
                    }
                }
            }
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. }
            | Expr::Pipe { left, right } => {
                self.visit_expr(&left.node, handler_name);
                self.visit_expr(&right.node, handler_name);
            }
            Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => self.visit_expr(&i.node, handler_name),
            Expr::FieldAccess { target, .. } => self.visit_expr(&target.node, handler_name),
            Expr::MethodCall { target, method, args } => {
                self.visit_expr(&target.node, handler_name);
                for a in args { self.visit_expr(&a.node, handler_name); }
                // `Ledger.settle(x)`: another cell's handler spends too
                if let Expr::Ident(cell) = &target.node {
                    let key = format!("{}.{}", cell, method);
                    if let Some(body) = self.handlers.get(key.as_str()).copied() {
                        if self.stack.iter().any(|h| h == &key) {
                            self.unbounded_sites.push(format!("{}::recursive call to {}", handler_name, key));
                        } else {
                            let mut callee = self.child();
                            callee.stack.push(key.clone());
                            for s in body { callee.visit_stmt(&s.node, &key); }
                            self.tokens += callee.tokens;
                            self.latency_ms += callee.latency_ms;
                            self.unbounded_sites.extend(callee.unbounded_sites);
                        }
                    }
                }
            }
            // A lambda runs once per element of whatever it is mapped over
            // — an unknown count. Count its body once (a lower bound) and,
            // if it spends, make the bound advisory.
            Expr::Lambda { .. } | Expr::LambdaBlock { .. } => {
                let mut inner = self.child();
                match expr {
                    Expr::Lambda { body, .. } => inner.visit_expr(&body.node, handler_name),
                    Expr::LambdaBlock { stmts, result, .. } => {
                        for s in stmts { inner.visit_stmt(&s.node, handler_name); }
                        inner.visit_expr(&result.node, handler_name);
                    }
                    _ => {}
                }
                if inner.spends() {
                    self.unbounded_sites.push(format!(
                        "{}::lambda body spends (runs once per element)", handler_name
                    ));
                }
                self.tokens += inner.tokens;
                self.latency_ms += inner.latency_ms;
                self.unbounded_sites.extend(inner.unbounded_sites);
            }
            Expr::Match { subject, arms } => {
                self.visit_expr(&subject.node, handler_name);
                for arm in arms {
                    if let Some(g) = &arm.guard { self.visit_expr(&g.node, handler_name); }
                    for s in &arm.body { self.visit_stmt(&s.node, handler_name); }
                    self.visit_expr(&arm.result.node, handler_name);
                }
            }
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                self.visit_expr(&condition.node, handler_name);
                for s in then_body { self.visit_stmt(&s.node, handler_name); }
                self.visit_expr(&then_result.node, handler_name);
                for s in else_body { self.visit_stmt(&s.node, handler_name); }
                self.visit_expr(&else_result.node, handler_name);
            }
            Expr::Record { fields, .. } => {
                for (_, v) in fields { self.visit_expr(&v.node, handler_name); }
            }
            Expr::ListLiteral(items) => {
                for it in items { self.visit_expr(&it.node, handler_name); }
            }
            _ => {}
        }
    }
}

/// Returns (max_tokens, timeout_ms) extracted from think()'s second arg.
fn extract_think_opts(args: &[Spanned<Expr>]) -> (Option<i64>, Option<i64>) {
    if args.len() < 2 { return (None, None); }
    let mut max_tokens = None;
    let mut timeout = None;
    // The options map is the LAST argument, exactly as the runtime reads it:
    // think(prompt, map(..)) and think(prompt, system, map(..)) are both bounded.
    if let Expr::FnCall { name, args: kvs } = &args[args.len() - 1].node {
        if name == "map" {
            let mut i = 0;
            while i + 1 < kvs.len() {
                if let Expr::Literal(Literal::String(k)) = &kvs[i].node {
                    if let Expr::Literal(Literal::Int(v)) = &kvs[i + 1].node {
                        match k.as_str() {
                            "max_tokens" => max_tokens = Some(*v),
                            "timeout" => timeout = Some(*v),
                            _ => {}
                        }
                    }
                }
                i += 2;
            }
        }
    }
    (max_tokens, timeout)
}

fn extract_timeout_ms(opts: &Expr) -> Option<i64> {
    if let Expr::FnCall { name, args } = opts {
        if name == "map" {
            let mut i = 0;
            while i + 1 < args.len() {
                if let Expr::Literal(Literal::String(k)) = &args[i].node {
                    if k == "timeout" {
                        if let Expr::Literal(Literal::Int(v)) = &args[i + 1].node {
                            return Some(*v);
                        }
                    }
                }
                i += 2;
            }
        }
    }
    None
}

/// Per-model USD cost per 1000 input tokens (very rough). Override
/// future in soma.toml. Returns 0 for unknown models (no USD bound).
fn usd_milli_per_1k_tokens(model: &str) -> i64 {
    let m = model.to_lowercase();
    if m.starts_with("gpt-4o-mini") { 150 }       // $0.150 / 1k
    else if m.starts_with("gpt-4o") { 5_000 }      // $5 / 1k
    else if m.starts_with("o1") { 15_000 }         // $15 / 1k
    else if m.starts_with("claude-opus") { 15_000 }
    else if m.starts_with("claude-sonnet") { 3_000 }
    else if m.starts_with("claude-haiku") { 800 }
    else if m.starts_with("gemma") || m.contains("ollama") { 0 }  // self-hosted
    else { 0 }  // unknown
}

pub type AllHandlers = std::collections::HashMap<String, std::collections::HashMap<String, Vec<Spanned<Statement>>>>;

pub fn check_cell(cell: &CellDef, manifest: Option<&Manifest>, all: &AllHandlers) -> Vec<CostFinding> {
    let cost_section = cell.sections.iter().find_map(|s| {
        if let Section::Cost(ref c) = s.node { Some(c.clone()) } else { None }
    });
    let cost = match cost_section {
        Some(c) => c,
        None => return Vec::new(),
    };

    // For each handler in the cell, compute its cost. Take max across
    // handlers (only one runs at a time).
    let mut peak_tokens = 0i64;
    let mut peak_latency_ms = 0i64;
    let mut peak_tokens_in = String::new();
    let mut peak_latency_in = String::new();
    let mut advisory_sites: Vec<String> = Vec::new();
    // Own handlers by name; other cells' handlers by bare name (the runtime
    // resolves a bare call to any cell that defines it) and as
    // `Cell.handler` — a 5×think() helper in another cell used to be
    // invisible and the bound "proven" at 50 tokens.
    let mut handlers: std::collections::HashMap<String, &[Spanned<Statement>]> = std::collections::HashMap::new();
    for (cname, hs) in all {
        for (hname, body) in hs {
            handlers.insert(format!("{}.{}", cname, hname), body.as_slice());
            if cname != &cell.name {
                handlers.entry(hname.clone()).or_insert(body.as_slice());
            }
        }
    }
    for s in &cell.sections {
        if let Section::OnSignal(h) = &s.node {
            handlers.insert(h.signal_name.clone(), h.body.as_slice());
        }
    }
    for section in &cell.sections {
        // every / after blocks are handler invocations: a tick with two
        // think() calls used to count as "peak 0"
        let (hname, body): (String, &[Spanned<Statement>]) = match &section.node {
            Section::OnSignal(handler) => (handler.signal_name.clone(), &handler.body),
            Section::Every(e) => (format!("every@{}ms", e.interval_ms), &e.body),
            Section::After(e) => (format!("after@{}ms", e.interval_ms), &e.body),
            _ => continue,
        };
        {
            let mut walk = CostWalk::new(&handlers);
            walk.stack.push(hname.clone());
            for s in body {
                walk.visit_stmt(&s.node, &hname);
            }
            if walk.tokens > peak_tokens { peak_tokens = walk.tokens; peak_tokens_in = hname.clone(); }
            if walk.latency_ms > peak_latency_ms { peak_latency_ms = walk.latency_ms; peak_latency_in = hname.clone(); }
            advisory_sites.extend(walk.unbounded_sites);
        }
    }

    // Resolve model + USD cost.
    let model_name = cell.agent_model.as_ref().and_then(|n| {
        manifest.and_then(|m| m.models.get(n)).map(|cfg| cfg.resolve_model())
    }).unwrap_or_else(|| "gpt-4o-mini".to_string());
    let usd_per_1k = usd_milli_per_1k_tokens(&model_name);
    let peak_usd_milli = (peak_tokens * usd_per_1k + 999) / 1000;

    let mut findings = Vec::new();

    // With an unbounded site the computed peaks are LOWER bounds: they
    // can still prove a budget is exceeded, never that it holds.
    advisory_sites.sort();
    advisory_sites.dedup();
    let bounded = advisory_sites.is_empty();
    if !advisory_sites.is_empty() {
        findings.push(CostFinding::Advisory {
            axis: "tokens",
            reason: format!("{} unbounded think()/loop site(s): [{}]",
                            advisory_sites.len(),
                            advisory_sites.join(", ")),
        });
    }

    if let Some(declared) = cost.tokens {
        if peak_tokens > declared {
            findings.push(CostFinding::Exceeded {
                axis: "tokens", declared, computed: peak_tokens, unit: "tokens",
                where_: format!("peak in {}.{}", cell.name, peak_tokens_in),
            });
        } else if bounded {
            findings.push(CostFinding::Proven {
                axis: "tokens", computed: peak_tokens, declared, unit: "tokens",
            });
        }
    }
    if let Some(declared) = cost.latency_ms {
        if peak_latency_ms > declared {
            findings.push(CostFinding::Exceeded {
                axis: "latency", declared, computed: peak_latency_ms, unit: "ms",
                where_: format!("peak in {}.{}", cell.name, peak_latency_in),
            });
        } else if bounded {
            findings.push(CostFinding::Proven {
                axis: "latency", computed: peak_latency_ms, declared, unit: "ms",
            });
        }
    }
    if let Some(declared) = cost.usd_milli {
        if usd_per_1k == 0 {
            findings.push(CostFinding::Advisory {
                axis: "usd",
                reason: format!("no price table entry for model '{}'", model_name),
            });
        } else if peak_usd_milli > declared {
            findings.push(CostFinding::Exceeded {
                axis: "usd", declared, computed: peak_usd_milli, unit: "milli-USD", where_: format!("peak in {}.{}", cell.name, peak_tokens_in),
            });
        } else if bounded {
            findings.push(CostFinding::Proven {
                axis: "usd", computed: peak_usd_milli, declared, unit: "milli-USD",
            });
        }
    }
    findings
}

/// Iteration count of `range(n)` / `range(lo, hi)` with literal bounds.
fn literal_range_len(iter: &Expr) -> Option<i64> {
    let Expr::FnCall { name, args } = iter else { return None };
    if name != "range" {
        return None;
    }
    let lit = |e: &Spanned<Expr>| match e.node {
        Expr::Literal(Literal::Int(n)) => Some(n),
        _ => None,
    };
    match args.len() {
        1 => lit(&args[0]).map(|n| n.max(0)),
        2 => Some((lit(&args[1])? - lit(&args[0])?).max(0)),
        _ => None,
    }
}
