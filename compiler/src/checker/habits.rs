//! Habits from other languages that Soma accepts syntactically and then
//! answers wrongly or fails on at runtime. Each was hit by an agent writing
//! Soma for the first time (docs/agent-ux/LEDGER.md); each is now a
//! check-time diagnostic that names the Soma form.
//!
//!   - `xs.includes(x)`          unknown method          → ERROR + did-you-mean
//!   - `rows.push(x)` statement  pure result discarded   → WARNING
//!   - `[x] + rest`              `+` adds numeric lists  → WARNING (concat)

use crate::ast::*;
use std::collections::HashSet;

use super::names::{builtin_names, suggest, ProgramIndex};

pub struct HabitFinding {
    pub message: String,
    pub span: Span,
}

/// Methods the runtime resolves without going through a builtin: memory
/// slot methods and the few native value methods (interpreter MethodCall).
const RUNTIME_METHODS: &[&str] = &[
    "all", "append", "backend", "contains", "count", "delete", "entries", "get", "has", "keys",
    "len", "length", "list", "push", "put", "remove", "set", "size", "values", "split",
];

/// Names models reach for, and what Soma calls them.
const ALIASES: &[(&str, &str)] = &[
    ("includes", "contains(xs, x)"),
    ("include", "contains(xs, x)"),
    ("indexOf", "index_of(s, sub)"),
    ("find_index", "index_of(s, sub)"),
    ("startsWith", "starts_with(s, prefix)"),
    ("endsWith", "ends_with(s, suffix)"),
    ("toUpperCase", "uppercase(s)"),
    ("upper", "uppercase(s)"),
    ("toLowerCase", "lowercase(s)"),
    ("lower", "lowercase(s)"),
    ("strip", "trim(s)"),
    ("substring", "slice(s, start, end)"),
    ("substr", "slice(s, start, end)"),
    ("items", "entries(m) — or m.keys() and m[k]"),
    ("forEach", "for x in xs { … }"),
    ("each", "for x in xs { … }"),
    ("extend", "concat(a, b)"),
    ("add", "push(xs, x) (returns a NEW list: xs = push(xs, x))"),
    ("pop", "slice(xs, 0, -1) / xs[len(xs) - 1]"),
    ("shift", "slice(xs, 1)"),
    ("toString", "to_string(x)"),
    ("str", "to_string(x)"),
    ("size", "len(x)"),
];

/// Builtins that return a new value and have no effect: calling one as a
/// bare statement on a local does nothing.
const PURE: &[&str] = &[
    "push", "append", "with", "sort", "sort_by", "reverse", "concat", "merge", "distinct", "slice",
    "filter", "map", "top", "bottom", "flatten", "replace", "trim", "uppercase", "lowercase",
];

pub fn check_program(program: &Program) -> (Vec<HabitFinding>, Vec<HabitFinding>) {
    let index = ProgramIndex::build(program);
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    for cell in super::names::collect_cells(program) {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        let cell_names: HashSet<String> =
            super::names::collect_cells(program).iter().map(|c| c.name.clone()).collect();
        for section in &cell.sections {
            let body: &[Spanned<Statement>] = match &section.node {
                Section::OnSignal(on) if !on.properties.iter().any(|p| p == "native") => &on.body,
                Section::Every(ev) | Section::After(ev) => &ev.body,
                _ => continue,
            };
            let mut w = Walk { index: &index, cells: &cell_names, errors: &mut errors, warnings: &mut warnings };
            w.stmts(body);
        }
    }
    (errors, warnings)
}

struct Walk<'a> {
    index: &'a ProgramIndex,
    cells: &'a HashSet<String>,
    errors: &'a mut Vec<HabitFinding>,
    warnings: &'a mut Vec<HabitFinding>,
}

impl Walk<'_> {
    fn stmts(&mut self, stmts: &[Spanned<Statement>]) {
        for (i, s) in stmts.iter().enumerate() {
            // the last expression of a block is its value (implicit return)
            self.stmt(s, i + 1 == stmts.len());
        }
    }

    fn stmt(&mut self, stmt: &Spanned<Statement>, is_last: bool) {
        match &stmt.node {
            Statement::ExprStmt { expr } => {
                if !is_last {
                    self.discarded(&expr.node, stmt.span);
                }
                self.expr(expr);
            }
            Statement::MethodCall { target, method, args } => {
                if !is_last && PURE.contains(&method.as_str()) && !self.index.slots.contains(target) {
                    self.warn_discarded(method, Some(target), stmt.span);
                }
                for a in args {
                    self.expr(a);
                }
            }
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => {
                self.expr(value)
            }
            Statement::Ensure { condition } => self.expr(condition),
            Statement::IndexSet { index, value, .. } => {
                self.expr(index);
                self.expr(value);
            }
            Statement::If { condition, then_body, else_body } => {
                self.expr(condition);
                self.stmts(then_body);
                self.stmts(else_body);
            }
            Statement::For { iter, body, .. } => {
                self.expr(iter);
                self.stmts(body);
            }
            Statement::While { condition, body, .. } => {
                self.expr(condition);
                self.stmts(body);
            }
            Statement::Emit { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
            Statement::Require { .. } | Statement::Break | Statement::Continue => {}
        }
    }

    /// `push(rows, x)` / `rows.push(x)` as a whole statement.
    fn discarded(&mut self, expr: &Expr, span: Span) {
        match expr {
            Expr::FnCall { name, args } if PURE.contains(&name.as_str()) && !self.index.handler_map.contains_key(name) => {
                let first = args.first().and_then(|a| if let Expr::Ident(n) = &a.node { Some(n.as_str()) } else { None });
                if first.map_or(true, |n| !self.index.slots.contains(n)) {
                    self.warn_discarded(name, first, span);
                }
            }
            Expr::MethodCall { target, method, .. } if PURE.contains(&method.as_str()) => {
                if let Expr::Ident(n) = &target.node {
                    if !self.index.slots.contains(n) {
                        self.warn_discarded(method, Some(n), span);
                    }
                }
            }
            _ => {}
        }
    }

    fn warn_discarded(&mut self, name: &str, target: Option<&str>, span: Span) {
        let fix = match target {
            Some(t) => format!("write `{t} = {name}({t}, …)`"),
            None => "bind or return the result".to_string(),
        };
        self.warnings.push(HabitFinding {
            message: format!(
                "the result of `{name}` is discarded — `{name}` returns a NEW value and leaves its \
                 argument unchanged, so this statement does nothing; {fix}"
            ),
            span,
        });
    }

    fn expr(&mut self, expr: &Spanned<Expr>) {
        match &expr.node {
            Expr::MethodCall { target, method, args } => {
                let on_cell = matches!(&target.node, Expr::Ident(n) if self.cells.contains(n));
                if !on_cell
                    && !builtin_names().contains(method.as_str())
                    && !RUNTIME_METHODS.contains(&method.as_str())
                    && !self.index.handler_map.contains_key(method)
                {
                    let hint = ALIASES
                        .iter()
                        .find(|(a, _)| a == method)
                        .map(|(_, soma)| format!(" — in Soma: {soma}"))
                        .or_else(|| {
                            let names: Vec<String> = builtin_names().iter().map(|s| s.to_string()).collect();
                            suggest(method, names.iter()).map(|s| format!(" — did you mean '{s}'?"))
                        })
                        .unwrap_or_default();
                    self.errors.push(HabitFinding {
                        message: format!(
                            "no method '{method}'{hint}. Any builtin is a method (xs.sort(), s.trim()); \
                             `soma describe --builtins` lists them"
                        ),
                        span: expr.span,
                    });
                }
                self.expr(target);
                for a in args {
                    self.expr(a);
                }
            }
            Expr::BinaryOp { left, op, right } => {
                if matches!(op, BinOp::Add)
                    && (matches!(left.node, Expr::ListLiteral(_)) || matches!(right.node, Expr::ListLiteral(_)))
                {
                    self.warnings.push(HabitFinding {
                        message: "`+` with a list literal: on numeric lists `+` ADDS element-wise \
                                  ([1] + [2] = [3]); to join lists write concat(a, b), to append one \
                                  item push(xs, x)"
                            .to_string(),
                        span: expr.span,
                    });
                }
                self.expr(left);
                self.expr(right);
            }
            Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => {
                self.expr(left);
                self.expr(right);
            }
            Expr::FnCall { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
            Expr::FieldAccess { target, .. } => self.expr(target),
            Expr::Index { target, index } => {
                self.expr(target);
                self.expr(index);
            }
            Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => self.expr(i),
            Expr::ListLiteral(items) => {
                for i in items {
                    self.expr(i);
                }
            }
            Expr::Record { fields, .. } => {
                for (_, v) in fields {
                    self.expr(v);
                }
            }
            Expr::Lambda { body, .. } => self.expr(body),
            Expr::LambdaBlock { stmts, result, .. } => {
                self.stmts(stmts);
                self.expr(result);
            }
            Expr::Match { subject, arms } => {
                self.expr(subject);
                for arm in arms {
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    self.stmts(&arm.body);
                    self.expr(&arm.result);
                }
            }
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                self.expr(condition);
                self.stmts(then_body);
                self.expr(then_result);
                self.stmts(else_body);
                self.expr(else_result);
            }
            Expr::Literal(_) | Expr::Ident(_) => {}
        }
    }
}
