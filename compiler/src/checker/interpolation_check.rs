//! Static interpolation check (V1.7).
//!
//! Every string literal in a handler body is interpolated at runtime
//! (interpreter/mod.rs::interpolate_string). Since the V2.2.1 audit an
//! undefined variable inside `{...}` is a hard runtime error — so a
//! typo like "hello {customr}" passes `soma check` and 500s in
//! production. This pass scans every string literal with EXACTLY the
//! runtime's segmentation rules and reports identifiers that are
//! provably unknown, with a did-you-mean suggestion.
//!
//! Mirrored runtime rules (keep in sync with interpolate_string):
//!   - '{{' and '}}' are escapes for literal braces
//!   - a '{...}' segment that is empty or contains ':' or ';' is
//!     skipped (CSS/HTML), and scanning resumes one byte later
//!   - a segment that is a bare [A-Za-z0-9_]+ word is looked up in the
//!     local environment directly — undefined → runtime error
//!   - any other segment is parsed as an expression; if it does NOT
//!     parse it renders literally (NOT an error), if it does parse it
//!     is evaluated with locals in scope
//!
//! The pass is deliberately conservative: when scope is ambiguous or
//! the expression form is exotic, it stays silent. False negatives are
//! acceptable; false positives are not.

use crate::ast::*;
use std::collections::HashSet;

use super::names::{suggest, ProgramIndex};

#[derive(Debug)]
pub struct InterpolationIssue {
    pub message: String,
    pub span: Span,
    /// Inside `try { }` an interpolation error is catchable by design
    /// (assert_fails blesses error-raising handlers) — demoted to a
    /// warning so check does not contradict a passing test suite.
    pub warning: bool,
}

pub fn check_program(program: &Program) -> Vec<InterpolationIssue> {
    let index = ProgramIndex::build(program);
    let mut issues = Vec::new();

    // Handlers a test cell targets with `assert_fails handler(...)` are
    // EXPECTED to raise — interpolation issues in them demote to
    // warnings, otherwise check contradicts a passing test suite.
    let mut blessed_failing: HashSet<String> = HashSet::new();
    for cell in super::names::collect_cells(program) {
        if !matches!(cell.kind, CellKind::Test) { continue; }
        for section in &cell.sections {
            if let Section::Rules(rules) = &section.node {
                for rule in &rules.rules {
                    if let Rule::AssertFails(expr) | Rule::AssertFailsMatching(expr, _) = &rule.node {
                        if let Expr::FnCall { name, .. } = &expr.node {
                            blessed_failing.insert(name.clone());
                        }
                    }
                }
            }
        }
    }

    for cell in super::names::collect_cells(program) {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.sections {
            match &section.node {
                Section::OnSignal(on) => {
                    let mut w = Walker::new(&index);
                    if blessed_failing.contains(&on.signal_name) {
                        // treat the whole body as recoverable
                        w.try_depth = 1;
                    }
                    for p in &on.params {
                        w.scope.insert(p.name.clone());
                    }
                    w.walk_stmts(&on.body);
                    issues.extend(w.issues);
                }
                Section::Every(ev) | Section::After(ev) => {
                    let mut w = Walker::new(&index);
                    w.walk_stmts(&ev.body);
                    issues.extend(w.issues);
                }
                _ => {}
            }
        }
    }

    issues
}

/// Walks one handler body, tracking which names are bound.
///
/// The scope set is add-only: once a name is bound by a `let`, an
/// assignment, a loop variable, a lambda parameter or a match-arm
/// pattern it stays known for the remainder of the handler. This is
/// linear enough to catch use-before-definition in straight-line code
/// while staying conservative around branches and loops.
struct Walker<'a> {
    index: &'a ProgramIndex,
    /// names `let`-bound in the innermost block being walked
    block_lets: HashSet<String>,
    scope: HashSet<String>,
    issues: Vec<InterpolationIssue>,
    /// > 0 while walking inside a `try { }` — issues found there are
    /// recoverable by design and demote to warnings.
    try_depth: usize,
}

impl<'a> Walker<'a> {
    fn new(index: &'a ProgramIndex) -> Self {
        Self { index, scope: HashSet::new(), block_lets: HashSet::new(), issues: Vec::new(), try_depth: 0 }
    }

    fn known(&self, name: &str) -> bool {
        self.scope.contains(name) || self.index.known.contains(name)
    }

    fn walk_stmts(&mut self, stmts: &[Spanned<Statement>]) {
        for stmt in stmts {
            self.walk_stmt(stmt);
        }
    }

    /// Walk a nested block with the runtime's scoping: names `let`-bound
    /// inside it (and the extra names given, e.g. a loop variable or a
    /// lambda parameter) are gone afterwards; plain assignments persist.
    fn scoped(&mut self, extra: &[String], f: impl FnOnce(&mut Self)) {
        let before = self.scope.clone();
        for e in extra {
            self.scope.insert(e.clone());
        }
        f(self);
        // keep plain-assigned names (they persist at runtime), drop the rest
        let assigned: HashSet<String> = self.scope.difference(&before).cloned()
            .filter(|n| !self.block_lets.contains(n) && !extra.contains(n))
            .collect();
        self.scope = before;
        self.scope.extend(assigned);
        self.block_lets.clear();
    }

    fn walk_stmt(&mut self, stmt: &Spanned<Statement>) {
        match &stmt.node {
            Statement::Let { name, value } => {
                self.walk_expr(value);
                self.scope.insert(name.clone());
                self.block_lets.insert(name.clone());
            }
            Statement::Assign { name, value } => {
                self.walk_expr(value);
                if !self.scope.contains(name) && self.index.slots.contains(name) {
                    let kind = if self.index.list_slots.contains(name) { Some("List") } else { Some("Map") };
                    self.issues.push(InterpolationIssue {
                        message: crate::interpreter::slot_assign_message(name, kind),
                        span: stmt.span,
                        warning: false,
                    });
                }
                self.scope.insert(name.clone());
            }
            Statement::Return { value } | Statement::Ensure { condition: value } => {
                self.walk_expr(value);
            }
            Statement::ExprStmt { expr } => self.walk_expr(expr),
            Statement::IndexSet { name, index, value } => {
                self.scope.insert(name.clone());
                self.walk_expr(index);
                self.walk_expr(value);
            }
            Statement::If { condition, then_body, else_body } => {
                self.walk_expr(condition);
                self.scoped(&[], |w| w.walk_stmts(then_body));
                self.scoped(&[], |w| w.walk_stmts(else_body));
            }
            Statement::For { var, iter, body, .. } => {
                self.walk_expr(iter);
                let var = var.clone();
                self.scoped(&[var], |w| {
                    // Pre-bind everything the body binds: on iteration 2+
                    // those names exist, so flagging them would be a false
                    // positive for any string evaluated after the binding.
                    bind_stmts(body, &mut w.scope);
                    w.walk_stmts(body);
                });
            }
            Statement::While { condition, body, .. } => {
                self.walk_expr(condition);
                self.scoped(&[], |w| {
                    bind_stmts(body, &mut w.scope);
                    w.walk_stmts(body);
                });
            }
            Statement::Emit { args, .. } => {
                for a in args {
                    self.walk_expr(a);
                }
            }
            Statement::Require { constraint, .. } => {
                self.walk_constraint(&constraint.node);
            }
            Statement::MethodCall { args, .. } => {
                for a in args {
                    self.walk_expr(a);
                }
            }
            Statement::Break | Statement::Continue => {}
        }
    }

    fn walk_constraint(&mut self, c: &Constraint) {
        match c {
            Constraint::Comparison { left, right, .. } => {
                self.walk_expr(left);
                self.walk_expr(right);
            }
            Constraint::And(a, b) | Constraint::Or(a, b) => {
                self.walk_constraint(&a.node);
                self.walk_constraint(&b.node);
            }
            Constraint::Not(inner) => self.walk_constraint(&inner.node),
            // Predicate args are not evaluated at runtime — stay silent.
            Constraint::Predicate { .. } | Constraint::Descriptive(_) => {}
        }
    }

    fn walk_expr(&mut self, expr: &Spanned<Expr>) {
        let span = expr.span;
        match &expr.node {
            Expr::Literal(Literal::String(s)) => self.scan_string(s, span),
            Expr::Literal(_) => {}
            // A bare identifier read. Same conservative scope as the
            // interpolation scan: add-only, so a name bound anywhere
            // earlier in the handler is known. What is left is a name
            // bound NOWHERE — a typo or an incomplete rename, which would
            // otherwise only fail at runtime, on the path that reads it.
            Expr::Ident(name) => {
                if !self.known(name)
                    && !super::names::builtin_names().contains(name.as_str())
                    && !matches!(name.as_str(), "true" | "false" | "_" | "self" | "value" | "key" | "size")
                {
                    self.report_undefined_ident(name, span);
                }
            }
            Expr::FieldAccess { target, .. } => self.walk_expr(target),
            Expr::Index { target, index } => { self.walk_expr(target); self.walk_expr(index); }
            Expr::MethodCall { target, method, args } => {
                // `Ledger.deposit(a, n)` — a call into another cell: the
                // handler must exist there (the runtime resolves it by name)
                if let Expr::Ident(cell) = &target.node {
                    if self.index.cells.contains(cell) && !self.scope.contains(cell) {
                        let defined = self.index.handler_map.get(method)
                            .map(|cs| cs.contains(cell)).unwrap_or(false);
                        if !defined {
                            let owners = self.index.handler_map.get(method).cloned().unwrap_or_default();
                            let hint = if !owners.is_empty() {
                                format!(" — '{method}' is a handler of {}", owners.join(", "))
                            } else {
                                let names: Vec<&String> = self.index.handler_map.iter()
                                    .filter(|(_, cs)| cs.contains(cell)).map(|(h, _)| h).collect();
                                match suggest(method, names.iter().copied()) {
                                    Some(s) => format!(" (did you mean '{s}'?)"),
                                    None => String::new(),
                                }
                            };
                            self.issues.push(InterpolationIssue {
                                message: format!("cell '{cell}' has no handler '{method}'{hint}"),
                                span: target.span,
                                warning: false,
                            });
                        }
                    }
                }
                self.walk_expr(target);
                for a in args {
                    self.walk_expr(a);
                }
            }
            Expr::FnCall { args, .. } => {
                for a in args {
                    self.walk_expr(a);
                }
            }
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
                self.walk_expr(left);
                self.walk_expr(right);
            }
            Expr::Not(inner) => {
                self.walk_expr(inner);
            }
            Expr::Try(inner) | Expr::TryPropagate(inner) => {
                self.try_depth += 1;
                self.walk_expr(inner);
                self.try_depth -= 1;
            }
            Expr::Pipe { left, right } => {
                // the runtime refuses anything but a call on the right:
                // say so here, with the parenthesised form
                if !matches!(right.node, Expr::FnCall { .. } | Expr::MethodCall { .. } | Expr::Pipe { .. } | Expr::Ident(_)) {
                    self.issues.push(InterpolationIssue {
                        message: "the right side of `|>` must be a call — `x |> f(a)` means f(x, a); to combine the result, parenthesise: `(x |> f()) + 1`".to_string(),
                        span: right.span,
                        warning: false,
                    });
                }
                self.walk_expr(left);
                self.walk_expr(right);
            }
            Expr::Record { fields, .. } => {
                for (_, v) in fields {
                    self.walk_expr(v);
                }
            }
            Expr::ListLiteral(items) => {
                for item in items {
                    self.walk_expr(item);
                }
            }
            Expr::Lambda { param, body } => {
                let param = param.clone();
                self.scoped(&[param], |w| w.walk_expr(body));
            }
            Expr::LambdaBlock { param, stmts, result } => {
                let param = param.clone();
                self.scoped(&[param], |w| {
                    w.walk_stmts(stmts);
                    w.walk_expr(result);
                });
            }
            Expr::Match { subject, arms } => {
                self.walk_expr(subject);
                for arm in arms {
                    bind_pattern(&arm.pattern, &mut self.scope);
                    if let Some(g) = &arm.guard {
                        self.walk_expr(g);
                    }
                    self.walk_stmts(&arm.body);
                    self.walk_expr(&arm.result);
                }
            }
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                self.walk_expr(condition);
                self.walk_stmts(then_body);
                self.walk_expr(then_result);
                self.walk_stmts(else_body);
                self.walk_expr(else_result);
            }
        }
    }

    // ── String scanning (mirrors interpolate_string) ─────────────────

    fn scan_string(&mut self, s: &str, span: Span) {
        let bytes = s.as_bytes();
        let mut pos = 0;
        while pos < s.len() {
            let byte = bytes[pos];
            // {{ and }} escape literal braces
            if byte == b'{' && bytes.get(pos + 1) == Some(&b'{') {
                pos += 2;
                continue;
            }
            if byte == b'}' && bytes.get(pos + 1) == Some(&b'}') {
                pos += 2;
                continue;
            }
            if byte == b'{' {
                // `"v={x} and {"` — the literal ends INSIDE a `{`: a nested
                // quote split the string. Name that, not the stray word after.
                // (a literal that IS just "{" is a brace, not a split string)
                if !s[pos + 1..].contains('}') && s[pos + 1..].trim().is_empty() && s.trim() != "{" {
                    self.issues.push(InterpolationIssue {
                        message: "string literal ends inside `{…}` — no nested quotes inside an interpolation; bind the inner value first: `let inner = \"lit\"` then `\"… {inner}\"`".to_string(),
                        span,
                        warning: false,
                    });
                    return;
                }
                if let Some(end) = s[pos + 1..].find('}') {
                    let expr_str = &s[pos + 1..pos + 1 + end];
                    // Skipped as CSS/HTML — runtime advances one byte
                    // and rescans, so nested segments are still found.
                    if expr_str.is_empty() || expr_str.contains(':') || expr_str.contains(';') {
                        pos += 1;
                        continue;
                    }
                    if self.check_segment(expr_str, span) {
                        // Segment evaluates at runtime — skip past it.
                        pos = pos + 1 + end + 1;
                    } else {
                        // Not parseable as an expression: literal text.
                        pos += 1;
                    }
                    continue;
                }
            }
            pos += 1;
        }
    }

    /// Check a single `{...}` segment. Returns true if the runtime
    /// would treat it as an expression (and so skip past it), false if
    /// it renders literally.
    fn check_segment(&mut self, expr_str: &str, span: Span) -> bool {
        // Fast path mirror: a bare word is looked up in env directly.
        if expr_str.chars().all(|c| c.is_alphanumeric() || c == '_') {
            let starts_like_ident = expr_str
                .chars()
                .next()
                .map_or(false, |c| c.is_alphabetic() || c == '_');
            if starts_like_ident && !self.known(expr_str) {
                self.report_undefined_var(expr_str, span);
            }
            return true;
        }

        // The runtime's segment evaluator cannot handle nested string
        // literals — `{len("xy")}` errors at runtime even though the
        // outer parser accepts it. (Colon/semicolon segments never get
        // here: they are skipped as CSS, matching the runtime.)
        if expr_str.contains('"') {
            self.issues.push(InterpolationIssue {
                message: format!(
                    "string interpolation cannot evaluate a nested string literal in '{{{expr_str}}}' — \
                     bind the value with a let first, then interpolate the variable"
                ),
                span,
                warning: self.try_depth > 0,
            });
            return true;
        }

        // Full path mirror: parse the wrapped segment with the real
        // parser. Failure to parse means it renders literally.
        let wrapped = format!("cell _T {{ on _e() {{ return {} }} }}", expr_str);
        let mut lexer = crate::lexer::Lexer::new(&wrapped);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(_) => return false,
        };
        let mut parser = crate::parser::Parser::new(tokens);
        let program = match parser.parse_program() {
            Ok(p) => p,
            Err(_) => return false,
        };
        let expr = program.cells.first().and_then(|cell| {
            cell.node.sections.iter().find_map(|s| {
                if let Section::OnSignal(ref on) = s.node {
                    on.body.first().and_then(|stmt| {
                        if let Statement::Return { ref value } = stmt.node {
                            Some(value.node.clone())
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            })
        });
        match expr {
            Some(e) => {
                let mut bound = HashSet::new();
                self.check_segment_expr(&e, &mut bound, span);
                true
            }
            None => false,
        }
    }

    /// Walk a parsed segment expression looking for provably unknown
    /// identifier leaves and call targets. Exotic forms stay silent.
    fn check_segment_expr(&mut self, expr: &Expr, bound: &mut HashSet<String>, span: Span) {
        match expr {
            Expr::Ident(name) => {
                if !bound.contains(name) && !self.known(name) {
                    self.report_undefined_var(name, span);
                }
            }
            Expr::FnCall { name, args } => {
                if !bound.contains(name) && !self.known(name) {
                    self.report_undefined_fn(name, span);
                }
                for a in args {
                    self.check_segment_expr(&a.node, bound, span);
                }
            }
            Expr::FieldAccess { target, .. } => {
                self.check_segment_expr(&target.node, bound, span);
            }
            Expr::Index { target, index } => {
                self.check_segment_expr(&target.node, bound, span);
                self.check_segment_expr(&index.node, bound, span);
            }
            Expr::MethodCall { target, args, .. } => {
                self.check_segment_expr(&target.node, bound, span);
                for a in args {
                    self.check_segment_expr(&a.node, bound, span);
                }
            }
            Expr::BinaryOp { left, right, .. }
            | Expr::CmpOp { left, right, .. }
            | Expr::Pipe { left, right } => {
                self.check_segment_expr(&left.node, bound, span);
                self.check_segment_expr(&right.node, bound, span);
            }
            Expr::Not(inner) | Expr::Try(inner) | Expr::TryPropagate(inner) => {
                self.check_segment_expr(&inner.node, bound, span);
            }
            Expr::ListLiteral(items) => {
                for item in items {
                    self.check_segment_expr(&item.node, bound, span);
                }
            }
            Expr::Lambda { param, body } => {
                bound.insert(param.clone());
                self.check_segment_expr(&body.node, bound, span);
            }
            Expr::LambdaBlock { param, stmts, result } => {
                bound.insert(param.clone());
                bind_stmts(stmts, bound);
                self.check_segment_expr(&result.node, bound, span);
            }
            // Nested string literals, match/if expressions and record
            // literals inside an interpolation segment: too exotic to
            // reason about scope — stay silent.
            Expr::Literal(_) | Expr::Match { .. } | Expr::IfExpr { .. } | Expr::Record { .. } => {}
        }
    }

    fn report_undefined_var(&mut self, name: &str, span: Span) {
        let suggestion = suggest(name, self.scope.iter().chain(self.index.known.iter()))
            .map(|s| format!(" (did you mean '{}'?)", s))
            .unwrap_or_default();
        self.issues.push(InterpolationIssue {
            message: format!(
                "string interpolation references undefined variable '{name}'{suggestion} — \
                 define it before this line, or escape literal braces as '{{{{{name}}}}}'"
            ),
            span,
            warning: self.try_depth > 0,
        });
    }

    fn report_undefined_ident(&mut self, name: &str, span: Span) {
        let foreign = match name {
            "null" | "None" | "nil" | "undefined" | "NULL" => Some("Soma's null is `()`: `if x == () { … }`, `x ?? default`"),
            "const" | "var" | "val" => Some("bindings are `let x = …`; reassign with `x = …`"),
            "function" | "def" | "fn" | "func" | "lambda" => Some("a function is a handler: `on name(x: Int) { … }`; a lambda is `x => expr`"),
            "this" | "self" => Some("there is no `this`: memory slots and handlers of the cell are in scope by name"),
            "True" | "TRUE" => Some("write `true`"),
            "False" | "FALSE" => Some("write `false`"),
            _ => None,
        };
        if let Some(hint) = foreign {
            self.issues.push(InterpolationIssue {
                message: format!("'{name}' does not exist in Soma — {hint}"),
                span,
                warning: self.try_depth > 0,
            });
            return;
        }
        let suggestion = suggest(name, self.scope.iter().chain(self.index.known.iter()))
            .map(|s| format!(" (did you mean '{}'?)", s))
            .unwrap_or_default();
        self.issues.push(InterpolationIssue {
            message: format!(
                "undefined variable '{name}'{suggestion} — no let, parameter, loop variable, \
                 match binding or memory slot with that name is in scope"
            ),
            span,
            warning: self.try_depth > 0,
        });
    }

    fn report_undefined_fn(&mut self, name: &str, span: Span) {
        let suggestion = suggest(name, self.scope.iter().chain(self.index.known.iter()))
            .map(|s| format!(" (did you mean '{}'?)", s))
            .unwrap_or_default();
        self.issues.push(InterpolationIssue {
            message: format!(
                "string interpolation calls undefined function '{name}'{suggestion} — \
                 define it before this line, or escape literal braces as '{{{{{name}(...)}}}}'"
            ),
            span,
            warning: self.try_depth > 0,
        });
    }
}

/// Collect every name a statement list can bind (lets, assignments,
/// loop variables, lambda params, match-arm patterns), recursively.
/// Used to pre-bind loop bodies and to skip lambda-local names.
fn bind_stmts(stmts: &[Spanned<Statement>], scope: &mut HashSet<String>) {
    for stmt in stmts {
        match &stmt.node {
            Statement::Let { name, value } | Statement::Assign { name, value } => {
                scope.insert(name.clone());
                bind_expr(&value.node, scope);
            }
            Statement::For { var, iter, body, .. } => {
                scope.insert(var.clone());
                bind_expr(&iter.node, scope);
                bind_stmts(body, scope);
            }
            Statement::While { condition, body, .. } => {
                bind_expr(&condition.node, scope);
                bind_stmts(body, scope);
            }
            Statement::If { condition, then_body, else_body } => {
                bind_expr(&condition.node, scope);
                bind_stmts(then_body, scope);
                bind_stmts(else_body, scope);
            }
            Statement::Return { value } | Statement::Ensure { condition: value } => {
                bind_expr(&value.node, scope);
            }
            Statement::ExprStmt { expr } => bind_expr(&expr.node, scope),
            Statement::IndexSet { name, index, value } => { scope.insert(name.clone()); bind_expr(&index.node, scope); bind_expr(&value.node, scope); }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => {
                for a in args {
                    bind_expr(&a.node, scope);
                }
            }
            Statement::Require { .. } | Statement::Break | Statement::Continue => {}
        }
    }
}

fn bind_expr(expr: &Expr, scope: &mut HashSet<String>) {
    match expr {
        Expr::Lambda { param, body } => {
            scope.insert(param.clone());
            bind_expr(&body.node, scope);
        }
        Expr::LambdaBlock { param, stmts, result } => {
            scope.insert(param.clone());
            bind_stmts(stmts, scope);
            bind_expr(&result.node, scope);
        }
        Expr::Match { subject, arms } => {
            bind_expr(&subject.node, scope);
            for arm in arms {
                bind_pattern(&arm.pattern, scope);
                bind_stmts(&arm.body, scope);
                bind_expr(&arm.result.node, scope);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            bind_expr(&condition.node, scope);
            bind_stmts(then_body, scope);
            bind_expr(&then_result.node, scope);
            bind_stmts(else_body, scope);
            bind_expr(&else_result.node, scope);
        }
        Expr::FnCall { args, .. } => {
            for a in args {
                bind_expr(&a.node, scope);
            }
        }
        Expr::MethodCall { target, args, .. } => {
            bind_expr(&target.node, scope);
            for a in args {
                bind_expr(&a.node, scope);
            }
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            bind_expr(&left.node, scope);
            bind_expr(&right.node, scope);
        }
        Expr::Not(inner) | Expr::Try(inner) | Expr::TryPropagate(inner) => {
            bind_expr(&inner.node, scope);
        }
        Expr::ListLiteral(items) => {
            for item in items {
                bind_expr(&item.node, scope);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                bind_expr(&v.node, scope);
            }
        }
        Expr::FieldAccess { target, .. } => bind_expr(&target.node, scope),
        Expr::Index { target, index } => { bind_expr(&target.node, scope); bind_expr(&index.node, scope); }
        Expr::Literal(_) | Expr::Ident(_) => {}
    }
}

/// Names bound by a match pattern.
pub(super) fn bind_pattern(pattern: &MatchPattern, scope: &mut HashSet<String>) {
    match pattern {
        MatchPattern::Variable(name) => {
            scope.insert(name.clone());
        }
        MatchPattern::Or(alts) => {
            for alt in alts {
                bind_pattern(alt, scope);
            }
        }
        MatchPattern::MapDestructure(fields) => {
            for (_, sub) in fields {
                bind_pattern(sub, scope);
            }
        }
        MatchPattern::StringPrefix { rest, .. } => {
            scope.insert(rest.clone());
        }
        MatchPattern::Variant { fields, .. } => match fields {
            VariantPatternFields::Unit => {}
            VariantPatternFields::Tuple(pats) => {
                for p in pats {
                    bind_pattern(p, scope);
                }
            }
            VariantPatternFields::Struct { fields, .. } => {
                for (_, p) in fields {
                    bind_pattern(p, scope);
                }
            }
        },
        MatchPattern::Literal(_) | MatchPattern::Wildcard | MatchPattern::Range { .. } => {}
    }
}
