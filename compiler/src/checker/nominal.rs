//! Nominal types: `cell type Lots = Int` (docs/NOMINAL_TYPES_DESIGN.md).
//!
//! A nominal type is checked, then ERASED. `prepare` runs once the whole
//! program is loaded (imports and flesh included):
//!
//! 1. the type pass runs on the program as written, with `Lots` and `Cents`
//!    as distinct types, and keeps what it finds about them
//!    (`nominal_mismatch`) on `program.nominal_issues` for `soma check`;
//! 2. the program is rewritten: every `Lots` becomes its base (`Int`), every
//!    `Lots(x)` becomes `x`, and the declarations go.
//!
//! Everything after — the ordinary checks, `verify`, the interpreter,
//! `[native]`, storage, JSON — sees an ordinary program, so nothing proven
//! about an `Int` is lost and no stored data changes. A program without a
//! nominal declaration is not touched at all.

use crate::ast::*;
use std::collections::HashMap;

pub fn prepare(program: &mut Program) {
    program.nominal_issues.clear();
    let mut decls: HashMap<String, String> = HashMap::new();
    let mut issues: Vec<NominalIssue> = Vec::new();
    for c in &program.cells {
        let Some(base) = &c.node.nominal else { continue };
        match decls.get(&c.node.name) {
            Some(first) if first != base => issues.push(NominalIssue {
                advisory: false,
                message: format!("`cell type {n} = {base}` — {n} is already declared `= {first}`: one name, one base (keep one declaration, or name the other type differently)", n = c.node.name),
                span: c.span,
            }),
            _ => { decls.insert(c.node.name.clone(), base.clone()); }
        }
    }
    if decls.is_empty() { return; }

    // 0. a bound (`where …`) becomes what verify already proves
    let mut bounds: HashMap<String, Spanned<Expr>> = HashMap::new();
    for c in &program.cells {
        let (Some(_), Some(b)) = (&c.node.nominal, &c.node.nominal_where) else { continue };
        let mut names = std::collections::HashSet::new();
        names_in(&b.node, &mut names);
        names.remove("value");
        if names.is_empty() {
            bounds.insert(c.node.name.clone(), b.clone());
        } else {
            let mut names: Vec<String> = names.into_iter().collect();
            names.sort();
            issues.push(NominalIssue {
                advisory: false,
                message: format!("the bound of `{}` reads {} — a `where` bound reads only `value` (the value of the type), literals and constants: `where value >= 0 && value <= 50`",
                    c.node.name, names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")),
                span: b.span,
            });
        }
    }
    if !bounds.is_empty() {
        for c in &mut program.cells { bound_cell(&mut c.node, &bounds); }
    }

    // 1. checked as written
    let types = crate::checker::types::Program2::new_nominal(program, &decls);
    // (check_cell and check_invariants walk the interior cells themselves)
    for c in program.cells.iter().filter(|c| matches!(c.node.kind, CellKind::Cell | CellKind::Agent)) {
        for i in types.check_cell(&c.node).into_iter().chain(types.check_invariants(&c.node)) {
            if i.kind == "nominal_mismatch" && !issues.iter().any(|x| x.span.start == i.span.start && x.message == i.message) {
                issues.push(NominalIssue { advisory: i.advisory, message: i.message, span: i.span });
            }
        }
    }
    crate::checker::types::Program2::end_nominal();

    // 2. erased
    program.cells.retain(|c| c.node.nominal.is_none());
    let e = Erase { decls: &decls, bounds: &bounds, found: std::cell::RefCell::new(Vec::new()) };
    for c in &mut program.cells { e.cell(&mut c.node); }
    issues.extend(e.found.into_inner());
    issues.sort_by_key(|i| i.span.start);
    program.nominal_issues = issues;
}

struct Erase<'d> {
    decls: &'d HashMap<String, String>,
    /// the bounds, to judge a literal wrapped in its type (`Lots(60)`)
    bounds: &'d HashMap<String, Spanned<Expr>>,
    found: std::cell::RefCell<Vec<NominalIssue>>,
}

impl Erase<'_> {
    fn ty(&self, t: &mut TypeExpr) {
        match t {
            TypeExpr::Simple(n) => if let Some(b) = self.decls.get(n.as_str()) { *n = b.clone(); },
            TypeExpr::Generic { args, .. } => for a in args { self.ty(&mut a.node); },
            TypeExpr::CellRef { .. } => {}
        }
    }

    fn params(&self, ps: &mut [Param]) {
        for p in ps { self.ty(&mut p.ty.node); }
    }

    fn cell(&self, c: &mut CellDef) {
        for s in &mut c.sections { self.section(&mut s.node); }
    }

    fn section(&self, s: &mut Section) {
        match s {
            Section::Face(f) => for d in &mut f.declarations {
                match &mut d.node {
                    FaceDecl::Signal(sig) => {
                        self.params(&mut sig.params);
                        if let Some(r) = &mut sig.return_type { self.ty(&mut r.node); }
                        for e in sig.requires.iter_mut().chain(sig.ensures.iter_mut()) { self.expr(e); }
                    }
                    FaceDecl::Tool(t) => {
                        self.params(&mut t.params);
                        if let Some(r) = &mut t.return_type { self.ty(&mut r.node); }
                    }
                    FaceDecl::Await(a) => {
                        self.params(&mut a.params);
                        if let Some(r) = &mut a.return_type { self.ty(&mut r.node); }
                    }
                    FaceDecl::Given(_) | FaceDecl::Promise(_) => {}
                }
            },
            Section::Memory(m) => {
                for sl in &mut m.slots { self.ty(&mut sl.node.ty.node); }
                for e in &mut m.invariants { self.expr(e); }
            }
            Section::Interior(i) => for c in &mut i.cells { self.cell(&mut c.node); },
            Section::OnSignal(on) => { self.params(&mut on.params); self.stmts(&mut on.body); }
            Section::Rules(r) => for rule in &mut r.rules {
                match &mut rule.node {
                    Rule::Check(b) => self.stmts(b),
                    Rule::Assert(e) | Rule::AssertFails(e) | Rule::AssertFailsMatching(e, _) | Rule::Let { value: e, .. }
                    | Rule::MockThink { reply: e, .. } | Rule::MockApprove { reply: e } | Rule::MockHandler { reply: e, .. }
                    | Rule::Property { body: e, .. } => self.expr(e),
                    _ => {}
                }
            },
            Section::Runtime(r) => for en in &mut r.entries {
                match &mut en.node {
                    RuntimeEntry::Emit { args, .. } => for a in args { self.expr(a); },
                    RuntimeEntry::Stmt(st) => self.stmt(st),
                    _ => {}
                }
            },
            Section::State(sm) => for t in &mut sm.transitions {
                if let Some(g) = &mut t.node.guard { self.expr(g); }
                self.stmts(&mut t.node.effect);
            },
            Section::Every(e) | Section::After(e) => self.stmts(&mut e.body),
            Section::Variants(v) => for var in &mut v.variants {
                match &mut var.node.fields {
                    VariantFields::Tuple(ts) => for t in ts { self.ty(&mut t.node); },
                    VariantFields::Struct(fs) => for (_, t) in fs { self.ty(&mut t.node); },
                    VariantFields::Unit => {}
                }
            },
            Section::Scale(_) | Section::Cost(_) => {}
        }
    }

    fn stmts(&self, b: &mut [Spanned<Statement>]) {
        for s in b { self.stmt(&mut s.node); }
    }

    fn stmt(&self, s: &mut Statement) {
        match s {
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => self.expr(value),
            Statement::If { condition, then_body, else_body } => { self.expr(condition); self.stmts(then_body); self.stmts(else_body); }
            Statement::For { iter, body, .. } => { self.expr(iter); self.stmts(body); }
            Statement::While { condition, body, .. } => { self.expr(condition); self.stmts(body); }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => for a in args { self.expr(a); },
            Statement::Require { constraint, .. } => self.constraint(&mut constraint.node),
            Statement::IndexSet { index, value, .. } => { self.expr(index); self.expr(value); }
            Statement::Ensure { condition } => self.expr(condition),
            Statement::ExprStmt { expr } => self.expr(expr),
            Statement::Break | Statement::Continue => {}
        }
    }

    fn constraint(&self, c: &mut Constraint) {
        match c {
            Constraint::Comparison { left, right, .. } => { self.expr(left); self.expr(right); }
            Constraint::Predicate { args, .. } => for a in args { self.expr(a); },
            Constraint::And(a, b) | Constraint::Or(a, b) => { self.constraint(&mut a.node); self.constraint(&mut b.node); }
            Constraint::Not(a) => self.constraint(&mut a.node),
            Constraint::Descriptive(_) => {}
        }
    }

    fn expr(&self, e: &mut Spanned<Expr>) {
        match &mut e.node {
            Expr::Literal(_) | Expr::Ident(_) => {}
            Expr::FieldAccess { target, .. } => self.expr(target),
            Expr::Index { target, index } => { self.expr(target); self.expr(index); }
            Expr::MethodCall { target, args, .. } => { self.expr(target); for a in args { self.expr(a); } }
            Expr::FnCall { args, .. } => for a in args { self.expr(a); },
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => { self.expr(left); self.expr(right); }
            Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => self.expr(i),
            Expr::Record { fields, .. } => for (_, v) in fields { self.expr(v); },
            Expr::Match { subject, arms } => {
                self.expr(subject);
                for arm in arms {
                    if let Some(g) = &mut arm.guard { self.expr(g); }
                    self.stmts(&mut arm.body);
                    self.expr(&mut arm.result);
                }
            }
            Expr::Lambda { body, .. } => self.expr(body),
            Expr::LambdaBlock { stmts, result, .. } => { self.stmts(stmts); self.expr(result); }
            Expr::ListLiteral(items) => for i in items { self.expr(i); },
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                self.expr(condition);
                self.stmts(then_body);
                self.expr(then_result);
                self.stmts(else_body);
                self.expr(else_result);
            }
        }
        // `Lots(x)` is `x` — and a literal must be inside the bound
        if let Expr::FnCall { name, args } = &mut e.node {
            if let (Some(b), [arg]) = (self.bounds.get(name.as_str()), args.as_slice()) {
                if let Some(v) = literal_value(&arg.node) {
                    if eval(&b.node, &v) == Some(Val::Bool(false)) {
                        self.found.borrow_mut().push(NominalIssue { advisory: false, span: e.span, message: format!(
                            "`{name}({})` is outside the bound of {name}: `{}` — use a value it admits, or widen the bound on `cell type {name}`",
                            render_expr(&arg.node), render_expr(&b.node)) });
                    }
                }
            }
            if args.len() == 1 && self.decls.contains_key(name.as_str()) {
                e.node = args.remove(0).node;
            }
        }
    }
}

// ── bounds ───────────────────────────────────────────────────────────

fn names_in(e: &Expr, out: &mut std::collections::HashSet<String>) {
    if let Expr::Ident(n) = e { out.insert(n.clone()); }
    crate::checker::literals::for_each_in_expr(e, &mut |x| if let Expr::Ident(n) = x { out.insert(n.clone()); });
}

/// The bound with `value` replaced by `by` (a parameter, `result`, a slot).
fn subst(e: &Spanned<Expr>, by: &str) -> Spanned<Expr> {
    let mut out = e.clone();
    fn go(e: &mut Spanned<Expr>, by: &str) {
        match &mut e.node {
            Expr::Ident(n) if n == "value" => *n = by.to_string(),
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => { go(left, by); go(right, by); }
            Expr::Not(i) => go(i, by),
            Expr::FnCall { args, .. } => for a in args { go(a, by); },
            Expr::MethodCall { target, args, .. } => { go(target, by); for a in args { go(a, by); } }
            Expr::FieldAccess { target, .. } => go(target, by),
            _ => {}
        }
    }
    go(&mut out, by);
    out
}

/// The nominal type a type expression names, when it has a bound.
fn bounded<'b>(t: &TypeExpr, bounds: &'b HashMap<String, Spanned<Expr>>) -> Option<(&'b String, &'b Spanned<Expr>)> {
    match t { TypeExpr::Simple(n) => bounds.get_key_value(n.as_str()), _ => None }
}

/// The element a slot holds: `Map<K, Lots>`, `List<Lots>`.
fn held<'b>(t: &TypeExpr, bounds: &'b HashMap<String, Spanned<Expr>>) -> Option<(&'b String, &'b Spanned<Expr>)> {
    match t {
        TypeExpr::Generic { name, args } if (name == "Map" && args.len() == 2) || (name == "List" && args.len() == 1) =>
            bounded(&args.last().unwrap().node, bounds),
        other => bounded(other, bounds),
    }
}

/// Put each bound where verify reads it: a face parameter → `requires`, a
/// face result → `ensures`, a slot → a memory invariant, the parameter of
/// a handler with no face signal → a `require` at the top of its body.
fn bound_cell(c: &mut CellDef, bounds: &HashMap<String, Spanned<Expr>>) {
    let native: std::collections::HashSet<String> = c.sections.iter().filter_map(|s| match &s.node {
        Section::OnSignal(on) if on.properties.iter().any(|p| p == "native") => Some(on.signal_name.clone()), _ => None }).collect();
    let mut faced: std::collections::HashSet<String> = std::collections::HashSet::new();
    for s in &mut c.sections {
        match &mut s.node {
            Section::Face(f) => for d in &mut f.declarations {
                let FaceDecl::Signal(sig) = &mut d.node else { continue };
                faced.insert(sig.name.clone());
                if native.contains(&sig.name) { continue; } // a [native] handler takes no contract
                for p in &sig.params {
                    if let Some((_, b)) = bounded(&p.ty.node, bounds) { sig.requires.push(subst(b, &p.name)); }
                }
                if let Some((_, b)) = sig.return_type.as_ref().and_then(|r| bounded(&r.node, bounds)) {
                    sig.ensures.push(subst(b, "result"));
                }
            },
            Section::Memory(m) => {
                let extra: Vec<Spanned<Expr>> = m.slots.iter()
                    .filter_map(|sl| held(&sl.node.ty.node, bounds).map(|(_, b)| subst(b, &sl.node.name))).collect();
                m.invariants.extend(extra);
            }
            Section::Interior(i) => for ic in &mut i.cells { bound_cell(&mut ic.node, bounds); },
            _ => {}
        }
    }
    for s in &mut c.sections {
        let Section::OnSignal(on) = &mut s.node else { continue };
        if faced.contains(&on.signal_name) || native.contains(&on.signal_name) { continue; }
        let mut checks = Vec::new();
        for p in &on.params {
            let Some((n, b)) = bounded(&p.ty.node, bounds) else { continue };
            let cond = subst(b, &p.name);
            let span = cond.span;
            checks.push(Spanned::new(Statement::Require {
                constraint: Spanned::new(Constraint::Comparison { left: cond, op: CmpOp::Eq, right: Spanned::new(Expr::Literal(Literal::Bool(true)), span) }, span),
                else_signal: format!("OutOfRange\u{1f}{}: {n} admits only `{}`", p.name, render_expr(&b.node)),
            }, span));
        }
        if !checks.is_empty() { on.body.splice(0..0, checks); }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Val { Num(f64), Bool(bool), Str(String) }

fn literal_value(e: &Expr) -> Option<Val> {
    match e {
        Expr::Literal(Literal::Int(n)) => Some(Val::Num(*n as f64)),
        Expr::Literal(Literal::Float(x)) => Some(Val::Num(*x)),
        Expr::Literal(Literal::String(s)) => Some(Val::Str(s.clone())),
        Expr::BinaryOp { left, op: BinOp::Sub, right } if matches!(left.node, Expr::Literal(Literal::Int(0))) =>
            match literal_value(&right.node)? { Val::Num(x) => Some(Val::Num(-x)), _ => None },
        _ => None,
    }
}

/// The bound at a literal value: Some(Bool) when it can be decided here.
fn eval(e: &Expr, v: &Val) -> Option<Val> {
    match e {
        Expr::Ident(n) if n == "value" => Some(v.clone()),
        Expr::Literal(_) => literal_value(e).or(match e { Expr::Literal(Literal::Bool(b)) => Some(Val::Bool(*b)), _ => None }),
        Expr::Not(i) => match eval(&i.node, v)? { Val::Bool(b) => Some(Val::Bool(!b)), _ => None },
        Expr::BinaryOp { left, op, right } => {
            let (l, r) = (eval(&left.node, v)?, eval(&right.node, v)?);
            match (l, op, r) {
                (Val::Bool(a), BinOp::And, Val::Bool(b)) => Some(Val::Bool(a && b)),
                (Val::Bool(a), BinOp::Or, Val::Bool(b)) => Some(Val::Bool(a || b)),
                (Val::Num(a), BinOp::Add, Val::Num(b)) => Some(Val::Num(a + b)),
                (Val::Num(a), BinOp::Sub, Val::Num(b)) => Some(Val::Num(a - b)),
                (Val::Num(a), BinOp::Mul, Val::Num(b)) => Some(Val::Num(a * b)),
                _ => None,
            }
        }
        Expr::CmpOp { left, op, right } => {
            let (l, r) = (eval(&left.node, v)?, eval(&right.node, v)?);
            let ord = match (&l, &r) {
                (Val::Num(a), Val::Num(b)) => a.partial_cmp(b)?,
                (Val::Str(a), Val::Str(b)) => a.cmp(b),
                _ => return None,
            };
            use std::cmp::Ordering::*;
            Some(Val::Bool(match op {
                CmpOp::Lt => ord == Less, CmpOp::Le => ord != Greater, CmpOp::Gt => ord == Greater,
                CmpOp::Ge => ord != Less, CmpOp::Eq => ord == Equal, CmpOp::Ne => ord != Equal,
            }))
        }
        Expr::FnCall { name, args } if name == "str_len" || name == "len" => match eval(&args.first()?.node, v)? {
            Val::Str(s) => Some(Val::Num(s.chars().count() as f64)), _ => None },
        _ => None,
    }
}
