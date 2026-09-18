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
    /// A plain advisory (not a try-demoted error): reported as a habit warning.
    pub habit: bool,
    /// Stable machine-readable class for `check --json`.
    pub kind: &'static str,
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
        // test cells: the rules' expressions, with `let` bindings in scope
        // (an undefined `{var}` inside an assert used to be found at run time)
        if matches!(cell.kind, CellKind::Test) {
            let mut w = Walker::new(&index);
            for section in &cell.sections {
                if let Section::Rules(rules) = &section.node {
                    for rule in &rules.rules {
                        match &rule.node {
                            Rule::Assert(e) | Rule::AssertFails(e) | Rule::AssertFailsMatching(e, _) => w.walk_expr(e),
                            Rule::Let { name, value } => { w.walk_expr(value); w.scope.insert(name.clone()); }
                            Rule::Property { var, body, .. } => {
                                let var = var.clone();
                                w.scoped(&[var], |w| w.walk_expr(body));
                            }
                            Rule::MockThink { reply, .. } | Rule::MockApprove { reply } | Rule::MockHandler { reply, .. } => w.walk_expr(reply),
                            _ => {}
                        }
                    }
                }
            }
            issues.extend(w.issues);
            continue;
        }
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.sections {
            match &section.node {
                Section::OnSignal(on) => {
                    let mut w = Walker::new(&index);
                    if blessed_failing.contains(&on.signal_name) {
                        // interpolation issues in it are recoverable (a bare
                        // undefined name stays an error: that is a bug, not
                        // the failure the test expects)
                        w.blessed = true;
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
    /// > 0 inside a `for` / `while` body (`break` is legal there)
    loop_depth: usize,
    /// a handler a test cell expects to fail (`assert_fails h(…)`)
    blessed: bool,
}

impl<'a> Walker<'a> {
    fn new(index: &'a ProgramIndex) -> Self {
        Self { index, scope: HashSet::new(), block_lets: HashSet::new(), issues: Vec::new(), try_depth: 0, loop_depth: 0, blessed: false }
    }

    fn known(&self, name: &str) -> bool {
        self.scope.contains(name) || self.index.known.contains(name)
    }

    fn walk_stmts(&mut self, stmts: &[Spanned<Statement>]) {
        for (i, stmt) in stmts.iter().enumerate() {
            // `let j = 1 2`: the `2` is a statement of its own that does
            // nothing (only the LAST statement of a block is a value)
            if i + 1 < stmts.len() {
                if let Statement::ExprStmt { expr } = &stmt.node {
                    let inert = matches!(&expr.node,
                        Expr::Literal(_) | Expr::Ident(_) | Expr::BinaryOp { .. } | Expr::CmpOp { .. }
                        | Expr::ListLiteral(_) | Expr::FieldAccess { .. } | Expr::Not(_))
                        && !expr_has_call(&expr.node);
                    if inert {
                        // name the Soma form where the cause is plain
                        let unclosed = |e: &Expr| matches!(e, Expr::Literal(Literal::String(t))
                            if t.matches('{').count() > t.matches('}').count());
                        let prev_open = i > 0 && match &stmts[i - 1].node {
                            Statement::Return { value } | Statement::ExprStmt { expr: value } | Statement::Let { value, .. } => unclosed(&value.node),
                            _ => false,
                        };
                        let hint = match &expr.node {
                            Expr::Ident(w) if w == "and" => " — Soma writes `a && b`".to_string(),
                            Expr::Ident(w) if w == "or" => " — Soma writes `a || b`".to_string(),
                            Expr::Ident(w) if w == "not" => " — Soma writes `!a`".to_string(),
                            _ if prev_open => " — a `\"` inside `{…}` ends the string: bind the value first (`let v = m[\"k\"]`, then `\"{v}\"`)".to_string(),
                            _ => " — a value that is not the last statement of its block is thrown away (a missing operator between two values? `let x = a b` is two statements)".to_string(),
                        };
                        self.issues.push(InterpolationIssue {
                            message: format!(
                                "`{}` on its own does nothing{}",
                                crate::ast::render_expr(&expr.node), hint
                            ),
                            span: expr.span,
                            warning: false,
                            habit: false,
                            kind: "unused_value",
                        });
                    }
                }
            }
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
                if self.index.slots.contains(name) && !self.scope.contains(name) {
                    self.issues.push(InterpolationIssue {
                        message: format!("`let {name}` hides the memory slot `{name}` for the rest of this block — reads, indexes and writes of `{name}` now mean the local; rename it (e.g. `{name}_local`) unless that is intended"),
                        span: stmt.span,
                        warning: true,
                        habit: true,
                        kind: "local_shadows_slot",
                    });
                }
                self.scope.insert(name.clone());
                self.block_lets.insert(name.clone());
            }
            Statement::Assign { name, value } => {
                self.walk_expr(value);
                if !self.scope.contains(name) && !self.index.slots.contains(name) && !self.index.known.contains(name) {
                    // `totl = x` creates a NEW variable: the typo'd name is
                    // never seen again (`total` keeps its old value)
                    self.issues.push(InterpolationIssue {
                        message: format!("assignment creates a new variable '{name}' — declare it with `let {name} = …`, or fix the name if you meant an existing variable"),
                        span: stmt.span,
                        warning: true,
                        habit: true,
                        kind: "assignment_without_let",
                    });
                }
                if !self.scope.contains(name) && self.index.slots.contains(name) {
                    let kind = if self.index.list_slots.contains(name) { Some("List") } else { Some("Map") };
                    self.issues.push(InterpolationIssue {
                        message: crate::interpreter::slot_assign_message(name, kind),
                        span: stmt.span,
                        warning: false,
                habit: false,
                kind: "slot_assignment",
                    });
                }
                self.scope.insert(name.clone());
            }
            Statement::Return { value } | Statement::Ensure { condition: value } => {
                self.walk_expr(value);
            }
            Statement::ExprStmt { expr } => self.walk_expr(expr),
            Statement::IndexSet { name, index, value } => {
                // `rows[0] = v` on a slot is a slot write, not a local:
                // it must not hide a later `rows = …` from the check above
                if !self.index.slots.contains(name) || self.scope.contains(name) {
                    self.scope.insert(name.clone());
                }
                self.walk_expr(index);
                self.walk_expr(value);
            }
            Statement::If { condition, then_body, else_body } => {
                self.int_builtin_as_bool(condition);
                self.walk_expr(condition);
                self.scoped(&[], |w| w.walk_stmts(then_body));
                self.scoped(&[], |w| w.walk_stmts(else_body));
            }
            Statement::For { var, iter, body, .. } => {
                // `for r in rows { r.v = 2 }`: r is a COPY — the write is
                // lost unless r is used afterwards (written back, pushed…)
                if let Some(pos) = body.iter().position(|b| matches!(&b.node, Statement::IndexSet { name, .. } if name == var)) {
                    let mut later: HashSet<String> = HashSet::new();
                    for b in &body[pos + 1..] {
                        if let Statement::IndexSet { name, index, value } = &b.node {
                            if name == var {
                                crate::interpreter::free_names_expr(&index.node, &mut later);
                                crate::interpreter::free_names_expr(&value.node, &mut later);
                                continue;
                            }
                        }
                        crate::interpreter::free_names_stmts(std::slice::from_ref(b), &mut later);
                    }
                    if !later.contains(var.as_str()) {
                        self.issues.push(InterpolationIssue {
                            message: format!(
                                "`{var}` is a copy of the element, so this write is lost when the iteration ends — write it back (`for i in range(0, len(xs)) {{ let {var} = xs[i]  {var}.f = …  xs[i] = {var} }}`, or `rows[i] = {var}` for a List slot)"
                            ),
                            span: body[pos].span,
                            warning: true,
                            habit: true,
                            kind: "write_to_loop_copy",
                        });
                    }
                }
                self.walk_expr(iter);
                let var = var.clone();
                self.scoped(&[var], |w| {
                    // Pre-bind everything the body binds: on iteration 2+
                    // those names exist, so flagging them would be a false
                    // positive for any string evaluated after the binding.
                    bind_stmts(body, &mut w.scope);
                    w.loop_depth += 1;
                    w.walk_stmts(body);
                    w.loop_depth -= 1;
                });
            }
            Statement::While { condition, body, .. } => {
                self.int_builtin_as_bool(condition);
                self.walk_expr(condition);
                self.scoped(&[], |w| {
                    bind_stmts(body, &mut w.scope);
                    w.loop_depth += 1;
                    w.walk_stmts(body);
                    w.loop_depth -= 1;
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
            Statement::MethodCall { target, method, args } => {
                // statement position: `xs.push(x)` on a local does nothing
                // (push returns a new list) and `m.set(k, v)` raises
                if matches!(method.as_str(), "set" | "put" | "delete" | "push" | "append")
                    && self.scope.contains(target) && !self.index.slots.contains(target)
                {
                    let fix = match method.as_str() {
                        "push" | "append" => format!("a local list is rebuilt: `{target} = push({target}, x)`"),
                        "delete" => format!("a local map is rebuilt: `{target} = without({target}, k)`"),
                        _ => format!("a local map is written with brackets: `{target}[k] = v`"),
                    };
                    self.issues.push(InterpolationIssue {
                        message: format!("`.{method}()` is a memory-slot method and '{target}' is a local — {fix}"),
                        span: stmt.span,
                        warning: false,
                        habit: false,
                        kind: "slot_method_on_local",
                    });
                }
                for a in args {
                    self.walk_expr(a);
                }
            }
            Statement::Break | Statement::Continue => {
                if self.loop_depth == 0 {
                    self.issues.push(InterpolationIssue {
                        message: format!("`{}` outside a loop — it only means something inside `for` / `while` (to leave a handler, `return`)",
                            if matches!(stmt.node, Statement::Break) { "break" } else { "continue" }),
                        span: stmt.span,
                        warning: false,
                        habit: false,
                        kind: "break_outside_loop",
                    });
                }
            }
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

    /// `if !regex_match(s, p)`: a builtin that answers Int 1/0 used as a
    /// Bool raises "expected Bool, got Int" at run time — say it here.
    fn int_builtin_as_bool(&mut self, e: &Spanned<Expr>) {
        let mut parts = vec![e];
        while let Some(x) = parts.pop() {
            match &x.node {
                Expr::BinaryOp { left, op: BinOp::And | BinOp::Or, right } => { parts.push(left); parts.push(right); }
                Expr::FnCall { name, .. } if !self.index.handler_map.contains_key(name) => {
                    if let Some(b) = crate::interpreter::builtins::registry::BUILTINS.iter().find(|b| b.name == name) {
                        if b.signature.trim_end().ends_with("-> Int") {
                            self.issues.push(InterpolationIssue {
                                message: format!("{}() answers an Int (1 or 0), not a Bool — compare it: `{}(…) == 1`", name, name),
                                span: x.span,
                                warning: false,
                                habit: false,
                                kind: "int_as_bool",
                            });
                        }
                    }
                }
                _ => {}
            }
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
                // `.set/.delete/.push` are slot methods: on a LOCAL map or
                // list they were a runtime error ("not a memory slot")
                if let Expr::Ident(name) = &target.node {
                    if matches!(method.as_str(), "set" | "put" | "delete")
                        && self.scope.contains(name) && !self.index.slots.contains(name)
                    {
                        let fix = match method.as_str() {
                            "push" | "append" => format!("a local list is rebuilt: `{name} = push({name}, x)`"),
                            "delete" => format!("a local map is rebuilt: `{name} = without({name}, k)`"),
                            _ => format!("a local map is written with brackets: `{name}[k] = v`"),
                        };
                        self.issues.push(InterpolationIssue {
                            message: format!("`.{method}()` is a memory-slot method and '{name}' is a local — {fix}"),
                            span: expr.span,
                            warning: false,
                            habit: false,
                            kind: "slot_method_on_local",
                        });
                    }
                }
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
                habit: false,
                kind: "unknown_handler",
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
                self.int_builtin_as_bool(inner);
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
                habit: false,
                kind: "pipe_right_side",
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
                    // a lambda body is not inside the enclosing loop
                    let outer = std::mem::replace(&mut w.loop_depth, 0);
                    w.walk_stmts(stmts);
                    w.walk_expr(result);
                    w.loop_depth = outer;
                });
            }
            Expr::Match { subject, arms } => {
                if arms.is_empty() {
                    self.issues.push(InterpolationIssue {
                        message: "`match` with no arms always evaluates to `()` — add at least one arm (`_ -> …`)".to_string(),
                        span,
                        warning: false,
                        habit: false,
                        kind: "empty_match",
                    });
                }
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
                        message: "string literal ends inside `{…}` — a nested quote inside an interpolation? bind the inner value first: `let inner = \"lit\"` then `\"… {inner}\"` (a literal brace is `{{`)".to_string(),
                        span,
                        warning: true,
                        habit: true,
                        kind: "nested_quote",
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
                warning: self.try_depth > 0 || self.blessed,
                habit: false,
                kind: "undefined_variable",
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
            warning: self.try_depth > 0 || self.blessed,
                habit: false,
                kind: "undefined_variable",
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
                habit: false,
                kind: "foreign_idiom",
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
                habit: false,
                kind: "undefined_variable",
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
            warning: self.try_depth > 0 || self.blessed,
                habit: false,
                kind: "undefined_function",
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

fn expr_has_call(e: &Expr) -> bool {
    match e {
        Expr::FnCall { .. } | Expr::MethodCall { .. } | Expr::Pipe { .. } => true,
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => expr_has_call(&left.node) || expr_has_call(&right.node),
        Expr::Not(i) => expr_has_call(&i.node),
        Expr::FieldAccess { target, .. } => expr_has_call(&target.node),
        Expr::ListLiteral(items) => items.iter().any(|i| expr_has_call(&i.node)),
        _ => false,
    }
}
