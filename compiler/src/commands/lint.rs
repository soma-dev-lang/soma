use std::path::PathBuf;

use crate::ast::{self, Expr, Spanned, Statement};
use crate::interpreter::span_to_location;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports};

// ── Lint warning ────────────────────────────────────────────────────

#[derive(Debug)]
pub struct LintWarning {
    pub rule: String,
    pub severity: Severity,
    pub line: usize,
    pub message: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Info,
}

impl Severity {
    fn as_str(&self) -> &'static str {
        match self {
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Severity::Warning => "\u{26a0}",
            Severity::Info => "\u{2139}",
        }
    }
}

// ── AST walker ──────────────────────────────────────────────────────

struct LintPass<'a> {
    source: &'a str,
    warnings: Vec<LintWarning>,
    /// Names of handlers referenced in on request() routing (for private-helper check)
    routed_handlers: Vec<String>,
    /// Names of all on-handlers in the cell (for private-helper check)
    all_handler_names: Vec<(String, usize)>, // (name, line)
}

impl<'a> LintPass<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            warnings: Vec::new(),
            routed_handlers: Vec::new(),
            all_handler_names: Vec::new(),
        }
    }

    fn line_of(&self, span: &ast::Span) -> usize {
        let (line, _) = span_to_location(self.source, span.start);
        line
    }

    fn warn(&mut self, rule: &str, severity: Severity, line: usize, message: &str, suggestion: &str) {
        self.warnings.push(LintWarning {
            rule: rule.to_string(),
            severity,
            line,
            message: message.to_string(),
            suggestion: suggestion.to_string(),
        });
    }

    // ── Top-level walk ──────────────────────────────────────────────

    fn check_program(&mut self, program: &ast::Program) {
        for cell in &program.cells {
            if matches!(cell.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) {
                self.check_cell(&cell.node);
            }
        }
    }

    fn check_cell(&mut self, cell: &ast::CellDef) {
        // Collect memory slot names for context
        let memory_slots: Vec<String> = cell.sections.iter().filter_map(|s| {
            if let ast::Section::Memory(ref mem) = s.node {
                Some(mem.slots.iter().map(|sl| sl.node.name.clone()).collect::<Vec<_>>())
            } else {
                None
            }
        }).flatten().collect();

        // Collect handler names and check for routing references
        self.all_handler_names.clear();
        self.routed_handlers.clear();

        let mut on_sections: Vec<&ast::OnSection> = Vec::new();
        for section in &cell.sections {
            if let ast::Section::OnSignal(ref on) = section.node {
                let line = self.line_of(&section.span);
                self.all_handler_names.push((on.signal_name.clone(), line));
                on_sections.push(on);
            }
        }

        // Find which handlers are referenced by on request() routing
        for on in &on_sections {
            if on.signal_name == "request" {
                self.collect_routed_handlers(&on.body);
            }
        }

        // Now run checks on each handler
        for section in &cell.sections {
            if let ast::Section::OnSignal(ref on) = section.node {
                let line = self.line_of(&section.span);
                self.check_empty_handler(on, line);
                self.check_if_chain_instead_of_match(&on.body, line);
                self.check_statements(&on.body, &memory_slots);
            }
            if let ast::Section::Every(ref ev) = section.node {
                self.check_statements(&ev.body, &memory_slots);
            }
        }

        // Check private helper naming
        self.check_private_helpers();
    }

    // ── Rule 1: redundant to_json / from_json ───────────────────────

    fn check_statements(&mut self, stmts: &[Spanned<Statement>], memory_slots: &[String]) {
        for stmt in stmts {
            self.check_statement(&stmt.node, &stmt.span, memory_slots);
        }
    }

    fn check_statement(&mut self, stmt: &Statement, span: &ast::Span, memory_slots: &[String]) {
        match stmt {
            Statement::MethodCall { target, method, args, .. } => {
                // items.set(id, to_json(data)) -> redundant to_json
                if method == "set" && memory_slots.contains(target) {
                    for arg in args {
                        if let Expr::FnCall { name, args: inner_args } = &arg.node {
                            if name == "to_json" {
                                let line = self.line_of(span);
                                let inner = if let Some(first) = inner_args.first() {
                                    self.expr_to_source(&first.node)
                                } else {
                                    "data".to_string()
                                };
                                self.warn(
                                    "redundant_to_json",
                                    Severity::Warning,
                                    line,
                                    "redundant to_json() \u{2014} storage auto-serializes maps",
                                    &format!("{}.set(..., {})", target, inner),
                                );
                            }
                        }
                    }
                }
            }
            Statement::Let { value, .. } => {
                self.check_expr_for_lints(&value.node, &value.span, memory_slots);
                // Rule 2: unchecked .get() — check if the let binding is used without a ?? or if check
                self.check_unchecked_get_in_let(stmt, span, memory_slots);
            }
            Statement::Assign { value, .. } => {
                self.check_expr_for_lints(&value.node, &value.span, memory_slots);
            }
            Statement::Return { value } => {
                self.check_expr_for_lints(&value.node, &value.span, memory_slots);
            }
            Statement::If { condition, then_body, else_body, .. } => {
                self.check_expr_for_lints(&condition.node, &condition.span, memory_slots);
                self.check_statements(then_body, memory_slots);
                self.check_statements(else_body, memory_slots);
            }
            Statement::For { iter, body, .. } => {
                self.check_expr_for_lints(&iter.node, &iter.span, memory_slots);
                self.check_statements(body, memory_slots);
            }
            Statement::While { condition, body, .. } => {
                self.check_expr_for_lints(&condition.node, &condition.span, memory_slots);
                self.check_statements(body, memory_slots);
            }
            Statement::ExprStmt { expr } => {
                // Check for standalone to_json/from_json or method calls
                self.check_expr_for_lints(&expr.node, &expr.span, memory_slots);
                // Also check for method calls like items.set(id, to_json(data))
                if let Expr::MethodCall { target, method, args, .. } = &expr.node {
                    if method == "set" {
                        if let Expr::Ident(ref tgt_name) = target.node {
                            if memory_slots.contains(tgt_name) {
                                for arg in args {
                                    if let Expr::FnCall { name, args: inner_args } = &arg.node {
                                        if name == "to_json" {
                                            let line = self.line_of(&expr.span);
                                            let inner = if let Some(first) = inner_args.first() {
                                                self.expr_to_source(&first.node)
                                            } else {
                                                "data".to_string()
                                            };
                                            self.warn(
                                                "redundant_to_json",
                                                Severity::Warning,
                                                line,
                                                "redundant to_json() \u{2014} storage auto-serializes maps",
                                                &format!("{}.set(..., {})", tgt_name, inner),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn check_expr_for_lints(&mut self, expr: &Expr, span: &ast::Span, memory_slots: &[String]) {
        match expr {
            // from_json(items.get(id)) -> redundant from_json
            Expr::FnCall { name, args } => {
                if name == "from_json" {
                    if let Some(first) = args.first() {
                        if self.is_storage_get(&first.node, memory_slots) {
                            let line = self.line_of(span);
                            let inner = self.expr_to_source(&first.node);
                            self.warn(
                                "redundant_from_json",
                                Severity::Warning,
                                line,
                                "redundant from_json() \u{2014} storage auto-deserializes",
                                &inner,
                            );
                        }
                    }
                }
                for arg in args {
                    self.check_expr_for_lints(&arg.node, &arg.span, memory_slots);
                }
            }
            Expr::MethodCall { target, args, .. } => {
                self.check_expr_for_lints(&target.node, &target.span, memory_slots);
                for arg in args {
                    self.check_expr_for_lints(&arg.node, &arg.span, memory_slots);
                }
            }
            Expr::Pipe { left, right } => {
                // Check for items.get(id) |> from_json()
                if let Expr::FnCall { name, .. } = &right.node {
                    if name == "from_json" && self.is_storage_get(&left.node, memory_slots) {
                        let line = self.line_of(span);
                        let inner = self.expr_to_source(&left.node);
                        self.warn(
                            "redundant_from_json",
                            Severity::Warning,
                            line,
                            "redundant from_json() \u{2014} storage auto-deserializes",
                            &inner,
                        );
                    }
                }
                self.check_expr_for_lints(&left.node, &left.span, memory_slots);
                self.check_expr_for_lints(&right.node, &right.span, memory_slots);
            }
            Expr::BinaryOp { left, right, .. } => {
                self.check_expr_for_lints(&left.node, &left.span, memory_slots);
                self.check_expr_for_lints(&right.node, &right.span, memory_slots);
            }
            Expr::CmpOp { left, right, .. } => {
                self.check_expr_for_lints(&left.node, &left.span, memory_slots);
                self.check_expr_for_lints(&right.node, &right.span, memory_slots);
            }
            _ => {}
        }
    }

    fn is_storage_get(&self, expr: &Expr, memory_slots: &[String]) -> bool {
        match expr {
            Expr::MethodCall { target, method, .. } => {
                if method == "get" {
                    if let Expr::Ident(ref name) = target.node {
                        return memory_slots.contains(name);
                    }
                }
                false
            }
            _ => false,
        }
    }

    // ── Rule 2: unchecked .get() ────────────────────────────────────

    fn check_unchecked_get_in_let(&mut self, stmt: &Statement, span: &ast::Span, memory_slots: &[String]) {
        if let Statement::Let { name: _var_name, value } = stmt {
            // Check if value is a bare slot.get(key) without ??
            if self.is_bare_storage_get(&value.node, memory_slots) {
                let line = self.line_of(span);
                let get_expr = self.expr_to_source(&value.node);
                self.warn(
                    "unchecked_get",
                    Severity::Warning,
                    line,
                    "unchecked .get() \u{2014} may return ()",
                    &format!("{} ?? map()", get_expr),
                );
            }
        }
    }

    /// Returns true if expr is slot.get(key) but NOT wrapped in ?? (coalesce).
    fn is_bare_storage_get(&self, expr: &Expr, memory_slots: &[String]) -> bool {
        // A BinaryOp with ?? would be a coalesce — that's fine.
        // We only flag direct MethodCall { .get } on a memory slot.
        if self.is_storage_get(expr, memory_slots) {
            return true;
        }
        false
    }

    // ── Rule 3: if-chain instead of match ───────────────────────────

    fn check_if_chain_instead_of_match(&mut self, body: &[Spanned<Statement>], handler_line: usize) {
        let mut consecutive_eq_ifs = 0;
        let mut chain_start_line = 0;
        let mut compared_field: Option<String> = None;

        for stmt in body {
            if let Statement::If { condition, .. } = &stmt.node {
                if let Some(field) = self.is_field_eq_string_check(&condition.node) {
                    if let Some(ref prev) = compared_field {
                        if &field == prev {
                            consecutive_eq_ifs += 1;
                        } else {
                            // Different field, reset
                            if consecutive_eq_ifs >= 3 {
                                self.emit_if_chain_warning(chain_start_line, consecutive_eq_ifs, compared_field.as_deref().unwrap_or("value"));
                            }
                            consecutive_eq_ifs = 1;
                            chain_start_line = self.line_of(&stmt.span);
                            compared_field = Some(field);
                        }
                    } else {
                        consecutive_eq_ifs = 1;
                        chain_start_line = self.line_of(&stmt.span);
                        compared_field = Some(field);
                    }
                    continue;
                }
            }
            // Not a matching if — emit if we had a chain, then reset
            if consecutive_eq_ifs >= 3 {
                self.emit_if_chain_warning(chain_start_line, consecutive_eq_ifs, compared_field.as_deref().unwrap_or("value"));
            }
            consecutive_eq_ifs = 0;
            compared_field = None;
        }

        // Check trailing chain
        if consecutive_eq_ifs >= 3 {
            self.emit_if_chain_warning(chain_start_line, consecutive_eq_ifs, compared_field.as_deref().unwrap_or("value"));
        }
    }

    fn emit_if_chain_warning(&mut self, line: usize, count: usize, field: &str) {
        self.warn(
            "if_chain",
            Severity::Info,
            line,
            &format!("consider using match instead of if-chain ({} branches on '{}')", count, field),
            &format!("match {} {{ ... }}", field),
        );
    }

    /// If the expression is `field == "string"` or `"string" == field`, return the field name.
    fn is_field_eq_string_check(&self, expr: &Expr) -> Option<String> {
        if let Expr::CmpOp { left, op, right } = expr {
            if *op == ast::CmpOp::Eq {
                // field == "string"
                if let Expr::Ident(ref name) = left.node {
                    if matches!(right.node, Expr::Literal(ast::Literal::String(_))) {
                        return Some(name.clone());
                    }
                }
                // "string" == field
                if let Expr::Ident(ref name) = right.node {
                    if matches!(left.node, Expr::Literal(ast::Literal::String(_))) {
                        return Some(name.clone());
                    }
                }
            }
        }
        None
    }

    // ── Rule 4: private helper naming ───────────────────────────────

    fn collect_routed_handlers(&mut self, body: &[Spanned<Statement>]) {
        for stmt in body {
            match &stmt.node {
                Statement::ExprStmt { expr } => {
                    self.collect_fn_calls_from_expr(&expr.node);
                }
                Statement::Return { value } => {
                    self.collect_fn_calls_from_expr(&value.node);
                }
                Statement::If { condition, then_body, else_body, .. } => {
                    self.collect_fn_calls_from_expr(&condition.node);
                    self.collect_routed_handlers(then_body);
                    self.collect_routed_handlers(else_body);
                }
                Statement::Let { value, .. } => {
                    self.collect_fn_calls_from_expr(&value.node);
                }
                _ => {}
            }
        }
    }

    fn collect_fn_calls_from_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::FnCall { name, args } => {
                self.routed_handlers.push(name.clone());
                for arg in args {
                    self.collect_fn_calls_from_expr(&arg.node);
                }
            }
            Expr::MethodCall { target, args, .. } => {
                self.collect_fn_calls_from_expr(&target.node);
                for arg in args {
                    self.collect_fn_calls_from_expr(&arg.node);
                }
            }
            Expr::Match { subject, arms } => {
                self.collect_fn_calls_from_expr(&subject.node);
                for arm in arms {
                    self.collect_fn_calls_from_expr(&arm.result.node);
                    for s in &arm.body {
                        self.collect_routed_handlers(std::slice::from_ref(s));
                    }
                }
            }
            Expr::Pipe { left, right } => {
                self.collect_fn_calls_from_expr(&left.node);
                self.collect_fn_calls_from_expr(&right.node);
            }
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
                self.collect_fn_calls_from_expr(&left.node);
                self.collect_fn_calls_from_expr(&right.node);
            }
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result, .. } => {
                self.collect_fn_calls_from_expr(&condition.node);
                for s in then_body {
                    self.collect_routed_handlers(std::slice::from_ref(s));
                }
                self.collect_fn_calls_from_expr(&then_result.node);
                for s in else_body {
                    self.collect_routed_handlers(std::slice::from_ref(s));
                }
                self.collect_fn_calls_from_expr(&else_result.node);
            }
            _ => {}
        }
    }

    fn check_private_helpers(&mut self) {
        // Only check if the cell has explicit request routing
        // Without on request(), all handlers are public via auto-routing
        let has_request_handler = self.all_handler_names.iter().any(|(n, _)| n == "request");
        if !has_request_handler {
            return;
        }

        let well_known = ["request", "tick", "start", "stop", "init", "run"];

        let handler_names: Vec<(String, usize)> = self.all_handler_names.clone();
        let routed: Vec<String> = self.routed_handlers.clone();

        for (name, line) in &handler_names {
            if well_known.contains(&name.as_str()) {
                continue;
            }
            if name.starts_with('_') {
                continue;
            }
            // Is this handler referenced in routing?
            if !routed.iter().any(|r| r == name) {
                self.warn(
                    "private_helper",
                    Severity::Info,
                    *line,
                    &format!("handler '{}' is not referenced in request routing", name),
                    &format!("consider renaming to '_{}'", name),
                );
            }
        }
    }

    // ── Rule 5: empty handler body ──────────────────────────────────

    fn check_empty_handler(&mut self, on: &ast::OnSection, line: usize) {
        if on.body.is_empty() {
            self.warn(
                "empty_handler",
                Severity::Warning,
                line,
                &format!("handler 'on {}()' has an empty body", on.signal_name),
                "add handler logic or remove the handler",
            );
            return;
        }

        // Also flag handlers that only return Unit
        if on.body.len() == 1 {
            if let Statement::Return { value } = &on.body[0].node {
                if matches!(value.node, Expr::Literal(ast::Literal::Unit)) {
                    self.warn(
                        "empty_handler",
                        Severity::Warning,
                        line,
                        &format!("handler 'on {}()' only returns ()", on.signal_name),
                        "add handler logic or remove the handler",
                    );
                }
            }
        }
    }

    // ── Helpers ─────────────────────────────────────────────────────

    fn expr_to_source(&self, expr: &Expr) -> String {
        match expr {
            Expr::Ident(name) => name.clone(),
            Expr::MethodCall { target, method, args } => {
                let tgt = self.expr_to_source(&target.node);
                let arg_strs: Vec<String> = args.iter().map(|a| self.expr_to_source(&a.node)).collect();
                format!("{}.{}({})", tgt, method, arg_strs.join(", "))
            }
            Expr::FnCall { name, args } => {
                let arg_strs: Vec<String> = args.iter().map(|a| self.expr_to_source(&a.node)).collect();
                format!("{}({})", name, arg_strs.join(", "))
            }
            Expr::Literal(lit) => match lit {
                ast::Literal::Int(n) => n.to_string(),
                ast::Literal::Float(f) => f.to_string(),
                ast::Literal::String(s) => format!("\"{}\"", s),
                ast::Literal::Bool(b) => b.to_string(),
                ast::Literal::Unit => "()".to_string(),
                _ => "...".to_string(),
            },
            Expr::FieldAccess { target, field } => {
                format!("{}.{}", self.expr_to_source(&target.node), field)
            }
            _ => "...".to_string(),
        }
    }
}

// ── Public entry point ──────────────────────────────────────────────

pub fn cmd_lint(path: &PathBuf, json: bool) {
    let source = super::read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);

    let mut pass = LintPass::new(&source);
    pass.check_program(&program);

    // Sort by line number
    pass.warnings.sort_by_key(|w| w.line);

    if json {
        print_json(&pass.warnings);
    } else {
        print_human(path, &pass.warnings);
    }
}

fn print_human(path: &PathBuf, warnings: &[LintWarning]) {
    if warnings.is_empty() {
        println!("soma lint {}", path.display());
        println!("  No issues found.");
        return;
    }

    println!("soma lint {}", path.display());

    let mut warn_count = 0;
    let mut info_count = 0;

    for w in warnings {
        match w.severity {
            Severity::Warning => warn_count += 1,
            Severity::Info => info_count += 1,
        }
        println!("  {} line {}: {}", w.severity.icon(), w.line, w.message);
        println!("    suggestion: {}", w.suggestion);
    }

    let mut parts = Vec::new();
    if warn_count > 0 {
        parts.push(format!("{} warning{}", warn_count, if warn_count == 1 { "" } else { "s" }));
    }
    if info_count > 0 {
        parts.push(format!("{} info", info_count));
    }
    println!("{}", parts.join(", "));
}

fn print_json(warnings: &[LintWarning]) {
    let lints: Vec<serde_json::Value> = warnings.iter().map(|w| {
        serde_json::json!({
            "rule": w.rule,
            "severity": w.severity.as_str(),
            "line": w.line,
            "message": w.message,
            "suggestion": w.suggestion,
        })
    }).collect();

    let output = serde_json::json!({ "lints": lints });
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
