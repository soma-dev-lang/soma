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
}

pub fn check_program(program: &Program) -> Vec<InterpolationIssue> {
    let index = ProgramIndex::build(program);
    let mut issues = Vec::new();

    for cell in super::names::collect_cells(program) {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.sections {
            match &section.node {
                Section::OnSignal(on) => {
                    let mut w = Walker::new(&index);
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
    scope: HashSet<String>,
    issues: Vec<InterpolationIssue>,
}

impl<'a> Walker<'a> {
    fn new(index: &'a ProgramIndex) -> Self {
        Self { index, scope: HashSet::new(), issues: Vec::new() }
    }

    fn known(&self, name: &str) -> bool {
        self.scope.contains(name) || self.index.known.contains(name)
    }

    fn walk_stmts(&mut self, stmts: &[Spanned<Statement>]) {
        for stmt in stmts {
            self.walk_stmt(stmt);
        }
    }

    fn walk_stmt(&mut self, stmt: &Spanned<Statement>) {
        match &stmt.node {
            Statement::Let { name, value } | Statement::Assign { name, value } => {
                self.walk_expr(value);
                self.scope.insert(name.clone());
            }
            Statement::Return { value } | Statement::Ensure { condition: value } => {
                self.walk_expr(value);
            }
            Statement::ExprStmt { expr } => self.walk_expr(expr),
            Statement::If { condition, then_body, else_body } => {
                self.walk_expr(condition);
                self.walk_stmts(then_body);
                self.walk_stmts(else_body);
            }
            Statement::For { var, iter, body, .. } => {
                self.walk_expr(iter);
                self.scope.insert(var.clone());
                // Pre-bind everything the body binds: on iteration 2+
                // those names exist, so flagging them would be a false
                // positive for any string evaluated after the binding.
                bind_stmts(body, &mut self.scope);
                self.walk_stmts(body);
            }
            Statement::While { condition, body, .. } => {
                self.walk_expr(condition);
                bind_stmts(body, &mut self.scope);
                self.walk_stmts(body);
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
            Expr::Literal(_) | Expr::Ident(_) => {}
            Expr::FieldAccess { target, .. } => self.walk_expr(target),
            Expr::MethodCall { target, args, .. } => {
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
            Expr::Not(inner) | Expr::Try(inner) | Expr::TryPropagate(inner) => {
                self.walk_expr(inner);
            }
            Expr::Pipe { left, right } => {
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
                self.scope.insert(param.clone());
                self.walk_expr(body);
            }
            Expr::LambdaBlock { param, stmts, result } => {
                self.scope.insert(param.clone());
                self.walk_stmts(stmts);
                self.walk_expr(result);
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
