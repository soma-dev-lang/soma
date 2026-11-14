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
    let e = Erase { decls: &decls };
    for c in &mut program.cells { e.cell(&mut c.node); }
    issues.sort_by_key(|i| i.span.start);
    program.nominal_issues = issues;
}

struct Erase<'d> {
    decls: &'d HashMap<String, String>,
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
        // `Lots(x)` is `x`
        if let Expr::FnCall { name, args } = &mut e.node {
            if args.len() == 1 && self.decls.contains_key(name.as_str()) {
                e.node = args.remove(0).node;
            }
        }
    }
}
