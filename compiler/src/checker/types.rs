//! Static types for expressions (3.0). Local, bidirectional inference with no
//! new annotations: types come from literals, parameter types, face return
//! types, slot declarations, `cell type` fields and a short list of builtins
//! whose result type never varies.
//!
//! The pass is deliberately one-sided: `Unknown` is compatible with
//! everything, and an error is reported only when BOTH sides are known and
//! the interpreter would raise kind `type` on every execution of that
//! expression (the operator table below was measured against the
//! interpreter, all 9 × 9 operand kinds per operator). A program that ran
//! correctly before this pass still checks.
//!
//! Absence is a type (`T?`): `slot.get(k)`, `m[k]`, `m.field` on a Map and
//! `try { }.value` may be `()`. Using a `T?` where a value is required is an
//! error in edition 2027 (the fix is `?? default`, or a test that narrows
//! it); edition 2026 keeps the run-time check.

use crate::ast::*;
use std::collections::{HashMap, HashSet};

pub struct TypeIssue {
    /// `xs + ys` joining two lists: the operands, for `soma fix --edition`
    /// (the `+` between them becomes `++`)
    pub join: Option<(Span, Span)>,
    /// a warning in this edition, an error in the next
    pub advisory: bool,
    pub kind: &'static str,
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    /// not known statically (`Any`, an untyped lambda parameter, a builtin
    /// whose result depends on its arguments): checked at run time
    Unknown,
    Int,
    Float,
    /// Int or Float (`a / b`, a mixed accumulator)
    Num,
    Str,
    Bool,
    /// the literal `()`
    Unit,
    List(Box<Ty>),
    /// a Map with values of this type
    Map(Box<Ty>),
    /// a `cell type` with one variant with named fields
    Record(String),
    /// a value of a sum type (several variants)
    Variant(String),
    /// `try { e }`: {value, error, kind, detail}
    Try(Box<Ty>),
    Fn,
    /// `T?` — a T, or `()`
    Opt(Box<Ty>),
    /// `cell type Lots = Int`: a value of a nominal type and its base. Only
    /// the nominal pass (before erasure, checker/nominal.rs) sees one.
    Nominal(String, Box<Ty>),
}

thread_local! {
    /// The nominal types of the program the nominal pass is checking (name
    /// → base): empty for the ordinary pass, which runs on the erased program.
    static NOMINALS: std::cell::RefCell<HashMap<String, Ty>> = std::cell::RefCell::new(HashMap::new());
}

fn nominal_named(n: &str) -> Option<Ty> {
    NOMINALS.with(|m| m.borrow().get(n).cloned()).map(|base| Ty::Nominal(n.to_string(), Box::new(base)))
}

#[derive(Clone, Copy, PartialEq)]
enum Class { Num, Str, Bool, List, MapLike, Fn }

impl Ty {
    fn opt(inner: Ty) -> Ty {
        match inner {
            Ty::Unknown => Ty::Unknown,
            Ty::Opt(_) | Ty::Unit => inner,
            other => Ty::Opt(Box::new(other)),
        }
    }

    /// The type once `()` is ruled out.
    fn present(&self) -> &Ty {
        match self { Ty::Opt(inner) => inner, other => other }
    }

    fn class(&self) -> Option<Class> {
        Some(match self.present() {
            Ty::Int | Ty::Float | Ty::Num => Class::Num,
            Ty::Str => Class::Str,
            Ty::Bool => Class::Bool,
            Ty::List(_) => Class::List,
            Ty::Map(_) | Ty::Record(_) | Ty::Variant(_) | Ty::Try(_) => Class::MapLike,
            Ty::Fn => Class::Fn,
            Ty::Nominal(_, base) => return base.class(),
            Ty::Unknown | Ty::Unit | Ty::Opt(_) => return None,
        })
    }

    fn is_opt(&self) -> bool { matches!(self, Ty::Opt(_)) }

    pub fn name(&self) -> String {
        match self {
            Ty::Unknown => "value".to_string(),
            Ty::Int => "Int".to_string(),
            Ty::Float => "Float".to_string(),
            Ty::Num => "number".to_string(),
            Ty::Str => "String".to_string(),
            Ty::Bool => "Bool".to_string(),
            Ty::Unit => "()".to_string(),
            Ty::List(_) => "List".to_string(),
            Ty::Map(_) => "Map".to_string(),
            Ty::Record(n) => format!("{n} record"),
            Ty::Variant(n) => format!("{n} variant"),
            Ty::Try(_) => "try result".to_string(),
            Ty::Fn => "function".to_string(),
            Ty::Opt(inner) => format!("{}?", inner.name()),
            Ty::Nominal(n, _) => n.clone(),
        }
    }
}

/// The least type both fit (branches of an `if`, arms of a `match`).
fn join(a: &Ty, b: &Ty) -> Ty {
    if a == b { return a.clone(); }
    match (a, b) {
        (Ty::Unit, other) | (other, Ty::Unit) => Ty::opt(other.clone()),
        (Ty::Opt(x), y) | (y, Ty::Opt(x)) => Ty::opt(join(x, y.present())),
        (Ty::Int | Ty::Float | Ty::Num, Ty::Int | Ty::Float | Ty::Num) => Ty::Num,
        (Ty::List(x), Ty::List(y)) => Ty::List(Box::new(join(x, y))),
        (Ty::Map(x), Ty::Map(y)) => Ty::Map(Box::new(join(x, y))),
        _ => Ty::Unknown,
    }
}

/// Can a value of type `actual` stand where `declared` is required? Only a
/// difference of KIND refuses (a String for an Int); what the runtime
/// coerces (Int → Float, a Map → a record) or checks itself passes.
fn assignable(actual: &Ty, declared: &Ty) -> bool {
    // a `T?` is refused only where `()` is too, which is not decided here
    if actual.is_opt() { return true; }
    // an Int parameter refuses a Float, even `2.0` (a Float one takes an Int)
    if matches!((actual, declared), (Ty::Float, Ty::Int)) { return false; }
    let (Some(a), Some(d)) = (actual.class(), declared.class()) else { return true };
    if a == Class::Fn || d == Class::Fn { return true; }
    if a != d { return false; }
    match (actual.present(), declared.present()) {
        (Ty::List(x), Ty::List(y)) => assignable(x, y),
        _ => true,
    }
}

struct Sig {
    cell: String,
    params: Vec<(String, Ty, String)>,
    ret: Option<(Ty, String)>,
}

/// Builtins whose result type does not depend on their arguments.
fn builtin_result(name: &str, args: usize, first_is_string_literal: bool) -> Ty {
    match name {
        "to_string" | "to_json" | "uppercase" | "lowercase" | "replace" | "join" | "substring"
        | "format" | "pad_left" | "pad_right" | "type_of" | "sha256" | "escape_html" | "to_fixed"
        | "hmac_sha256" | "chr" => Ty::Str,
        "len" | "str_len" | "idiv" | "floor_div" | "div_round" | "days_between" | "months_between"
        | "days_in_month" | "ipow" | "gcd" | "str_at" | "bit_len" => Ty::Int,
        "to_float" | "sqrt" | "sin" | "cos" | "tan" | "exp" | "pi" => Ty::Float,
        "starts_with" | "ends_with" | "has_state" | "is_a" | "str_eq" | "secure_eq" => Ty::Bool,
        "split" | "fields" => Ty::List(Box::new(Ty::Str)),
        "range" => Ty::List(Box::new(Ty::Int)),
        "keys" | "values" | "entries" | "push" => Ty::List(Box::new(Ty::Unknown)),
        // `map("k", v, …)` builds a Map; `map(xs, f)` transforms a list
        "map" if args == 0 || first_is_string_literal => Ty::Map(Box::new(Ty::Unknown)),
        _ => Ty::Unknown,
    }
}

/// Fields every value answers (`xs.len`, `m.keys`, `A.T`): never a typo.
fn pseudo_field(f: &str) -> bool {
    matches!(f, "len" | "size" | "keys" | "values" | "entries" | "first" | "last" | "T" | "shape") || f.starts_with('_')
}

pub struct Program2<'a> {
    records: HashMap<String, Vec<(String, Ty)>>,
    /// sum type → the named fields any of its variants declares
    sums: HashMap<String, Vec<(String, Ty)>>,
    variant_of: HashMap<String, String>,
    handlers: HashMap<String, Vec<Sig>>,
    cells: HashSet<String>,
    /// handlers a test cell expects to raise (`assert_fails h(…)`)
    expected_to_fail: HashSet<String>,
    strict_absence: bool,
    _p: std::marker::PhantomData<&'a ()>,
}

fn type_expr_text(t: &TypeExpr) -> String {
    match t {
        TypeExpr::Simple(n) => n.clone(),
        TypeExpr::Generic { name, args } => format!("{}<{}>", name, args.iter().map(|a| type_expr_text(&a.node)).collect::<Vec<_>>().join(", ")),
        TypeExpr::CellRef { cell, member } => format!("{cell}.{member}"),
    }
}

impl<'a> Program2<'a> {
    /// The nominal pass: the program BEFORE erasure, with its `cell type
    /// Lots = Int` declarations (name → base type name). Only its
    /// `nominal_mismatch` findings are kept (checker/nominal.rs).
    pub fn new_nominal(program: &Program, decls: &HashMap<String, String>) -> Self {
        let bases: HashMap<String, Ty> = decls.iter().map(|(n, b)| (n.clone(), match b.as_str() {
            "Float" => Ty::Float, "String" => Ty::Str, _ => Ty::Int,
        })).collect();
        NOMINALS.with(|m| *m.borrow_mut() = bases);
        Self::build(program, false)
    }

    /// `strict_absence`: edition 2027 — a `T?` where a value is required is an error.
    pub fn new(program: &Program, strict_absence: bool) -> Self {
        NOMINALS.with(|m| m.borrow_mut().clear());
        Self::build(program, strict_absence)
    }

    /// The nominal pass is over: the ordinary one sees no nominal type.
    pub fn end_nominal() { NOMINALS.with(|m| m.borrow_mut().clear()); }

    fn build(program: &Program, strict_absence: bool) -> Self {
        let mut p = Program2 {
            records: HashMap::new(), sums: HashMap::new(), variant_of: HashMap::new(),
            handlers: HashMap::new(), cells: HashSet::new(), expected_to_fail: HashSet::new(), strict_absence, _p: std::marker::PhantomData,
        };
        // names first (a field may be of a type declared later)
        let mut decls: Vec<(&CellDef, &VariantsSection)> = Vec::new();
        for cell in &program.cells {
            if cell.node.kind != CellKind::Type || !cell.node.type_params.is_empty() { continue; }
            for s in &cell.node.sections {
                if let Section::Variants(v) = &s.node { decls.push((&cell.node, v)); }
            }
        }
        let record_names: HashSet<String> = decls.iter()
            .filter(|(_, v)| v.variants.len() == 1 && matches!(v.variants[0].node.fields, VariantFields::Struct(_)))
            .map(|(c, _)| c.name.clone()).collect();
        let sum_names: HashSet<String> = decls.iter().map(|(c, _)| c.name.clone()).filter(|n| !record_names.contains(n)).collect();
        let to_ty = |t: &TypeExpr| ty_of(t, &record_names, &sum_names);
        for (cell, v) in &decls {
            if record_names.contains(&cell.name) {
                if let VariantFields::Struct(fields) = &v.variants[0].node.fields {
                    p.records.insert(cell.name.clone(), fields.iter().map(|(n, t)| (n.clone(), to_ty(&t.node))).collect());
                }
                p.variant_of.insert(v.variants[0].node.name.clone(), cell.name.clone());
            } else {
                let mut fields: Vec<(String, Ty)> = Vec::new();
                for var in &v.variants {
                    // a variant name two types share names neither
                    if p.variant_of.insert(var.node.name.clone(), cell.name.clone()).is_some() {
                        p.variant_of.insert(var.node.name.clone(), String::new());
                    }
                    if let VariantFields::Struct(fs) = &var.node.fields {
                        for (n, t) in fs {
                            let ty = to_ty(&t.node);
                            match fields.iter_mut().find(|(k, _)| k == n) {
                                Some((_, old)) => *old = join(old, &ty),
                                None => fields.push((n.clone(), ty)),
                            }
                        }
                    }
                }
                p.sums.insert(cell.name.clone(), fields);
            }
        }
        // a handler a test targets with `assert_fails h(…)` is EXPECTED to
        // raise: what it does wrong is the test's subject, not a bug
        for cell in &program.cells {
            if cell.node.kind != CellKind::Test { continue; }
            for s in &cell.node.sections {
                let Section::Rules(rules) = &s.node else { continue };
                for rule in &rules.rules {
                    if let Rule::AssertFails(expr) | Rule::AssertFailsMatching(expr, _) = &rule.node {
                        match &expr.node {
                            Expr::FnCall { name, .. } => { p.expected_to_fail.insert(name.clone()); }
                            Expr::MethodCall { method, .. } => { p.expected_to_fail.insert(method.clone()); }
                            _ => {}
                        }
                    }
                }
            }
        }
        fn collect<'c>(cell: &'c CellDef, p: &mut Program2, rn: &HashSet<String>, sn: &HashSet<String>) {
            if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) { return; }
            p.cells.insert(cell.name.clone());
            let mut returns: HashMap<String, (Ty, String)> = HashMap::new();
            for s in &cell.sections {
                if let Section::Face(face) = &s.node {
                    for d in &face.declarations {
                        if let FaceDecl::Signal(sig) = &d.node {
                            if let Some(rt) = &sig.return_type {
                                returns.insert(sig.name.clone(), (ty_of(&rt.node, rn, sn), type_expr_text(&rt.node)));
                            }
                        }
                    }
                }
            }
            for s in &cell.sections {
                match &s.node {
                    Section::OnSignal(h) => {
                        p.handlers.entry(h.signal_name.clone()).or_default().push(Sig {
                            cell: cell.name.clone(),
                            params: h.params.iter().map(|q| (q.name.clone(), ty_of(&q.ty.node, rn, sn), type_expr_text(&q.ty.node))).collect(),
                            ret: returns.get(&h.signal_name).cloned(),
                        });
                    }
                    Section::Interior(i) => for c in &i.cells { collect(&c.node, p, rn, sn); },
                    _ => {}
                }
            }
        }
        for cell in &program.cells { collect(&cell.node, &mut p, &record_names, &sum_names); }
        p
    }

    fn ty(&self, t: &TypeExpr) -> Ty {
        let rn: HashSet<String> = self.records.keys().cloned().collect();
        let sn: HashSet<String> = self.sums.keys().cloned().collect();
        ty_of(t, &rn, &sn)
    }

    /// The nominal pass types the memory invariants too: a cap in Cents
    /// over a slot of Lots is where the reported bug was. Each slot named in
    /// an invariant stands for the value written (its element type).
    pub fn check_invariants(&self, cell: &CellDef) -> Vec<TypeIssue> {
        let mut out = Vec::new();
        let mut slots: HashMap<String, Ty> = HashMap::new();
        for s in &cell.sections {
            if let Section::Memory(m) = &s.node {
                for slot in &m.slots { slots.insert(slot.node.name.clone(), self.ty(&slot.node.ty.node)); }
            }
        }
        let elem = |t: &Ty| match t { Ty::Map(e) | Ty::List(e) => (**e).clone(), other => other.clone() };
        for s in &cell.sections {
            match &s.node {
                Section::Memory(m) => for inv in &m.invariants {
                    let empty = HashMap::new();
                    let mut cx = Cx {
                        p: self, cell: &cell.name, slots: &empty, scopes: vec![HashMap::new()],
                        reassigned: HashSet::new(), index_set: HashSet::new(), in_try: 0, out: &mut out,
                        handler: "", ret: None,
                    };
                    let mut idents: HashSet<String> = HashSet::new();
                    if let Expr::Ident(n) = &inv.node { idents.insert(n.clone()); }
                    crate::checker::literals::for_each_in_expr(&inv.node, &mut |e| if let Expr::Ident(n) = e { idents.insert(n.clone()); });
                    let named: Vec<&String> = slots.keys().filter(|n| idents.contains(*n)).collect();
                    for n in &named { cx.bind(n, elem(&slots[*n])); }
                    if named.len() == 1 { cx.bind("value", elem(&slots[named[0]])); }
                    let before = cx.out.len();
                    cx.infer(inv);
                    let text = render_expr(&inv.node);
                    for i in &mut out[before..] { i.message = format!("invariant `{text}`: {}", i.message); }
                },
                Section::Interior(i) => for c in &i.cells { out.extend(self.check_invariants(&c.node)); },
                _ => {}
            }
        }
        out
    }

    pub fn check_cell(&self, cell: &CellDef) -> Vec<TypeIssue> {
        let mut out = Vec::new();
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) { return out; }
        let mut slots: HashMap<String, Ty> = HashMap::new();
        for s in &cell.sections {
            if let Section::Memory(m) = &s.node {
                for slot in &m.slots { slots.insert(slot.node.name.clone(), self.ty(&slot.node.ty.node)); }
            }
        }
        for s in &cell.sections {
            let (body, params, name): (&[Spanned<Statement>], &[Param], Option<&str>) = match &s.node {
                Section::OnSignal(h) => (&h.body, &h.params, Some(&h.signal_name)),
                Section::Every(e) | Section::After(e) => (&e.body, &[], None),
                Section::Interior(i) => {
                    for c in &i.cells { out.extend(self.check_cell(&c.node)); }
                    continue;
                }
                _ => continue,
            };
            if name.map_or(false, |n| self.expected_to_fail.contains(n)) { continue; }
            // a [native] handler has its own, stricter typing (checker/native.rs)
            if let Section::OnSignal(h) = &s.node {
                if h.properties.iter().any(|p| p == "native") { continue; }
            }
            let mut reassigned = HashSet::new();
            let mut index_set = HashSet::new();
            collect_writes(body, &mut reassigned, &mut index_set);
            let ret = name.and_then(|n| self.handlers.get(n))
                .and_then(|sigs| sigs.iter().find(|sg| sg.cell == cell.name))
                .and_then(|sg| sg.ret.clone());
            let mut cx = Cx {
                p: self, cell: &cell.name, slots: &slots, scopes: vec![HashMap::new()],
                reassigned, index_set, in_try: 0, out: &mut out,
                handler: name.unwrap_or(""), ret,
            };
            for q in params {
                let t = self.ty(&q.ty.node);
                cx.bind(&q.name, t);
            }
            cx.block(body);
        }
        out
    }
}

fn ty_of(t: &TypeExpr, records: &HashSet<String>, sums: &HashSet<String>) -> Ty {
    if let TypeExpr::Simple(n) = t { if let Some(nt) = nominal_named(n) { return nt; } }
    match t {
        TypeExpr::Simple(n) => match n.as_str() {
            "Int" => Ty::Int, "Float" => Ty::Float, "String" => Ty::Str, "Bool" => Ty::Bool,
            "Map" => Ty::Map(Box::new(Ty::Unknown)), "List" => Ty::List(Box::new(Ty::Unknown)),
            n if records.contains(n) => Ty::Record(n.to_string()),
            n if sums.contains(n) => Ty::Variant(n.to_string()),
            _ => Ty::Unknown,
        },
        TypeExpr::Generic { name, args } => match (name.as_str(), args.len()) {
            ("List", 1) => Ty::List(Box::new(ty_of(&args[0].node, records, sums))),
            ("Map", 2) => Ty::Map(Box::new(ty_of(&args[1].node, records, sums))),
            _ => Ty::Unknown,
        },
        TypeExpr::CellRef { .. } => Ty::Unknown,
    }
}

// ── names a handler writes after binding them ───────────────────────────

fn collect_writes(body: &[Spanned<Statement>], reassigned: &mut HashSet<String>, index_set: &mut HashSet<String>) {
    for s in body { writes_stmt(&s.node, reassigned, index_set); }
}

fn writes_stmt(s: &Statement, re: &mut HashSet<String>, ix: &mut HashSet<String>) {
    match s {
        Statement::Let { value, .. } | Statement::Return { value } => writes_expr(&value.node, re, ix),
        Statement::Assign { name, value } => { re.insert(name.clone()); writes_expr(&value.node, re, ix); }
        Statement::If { condition, then_body, else_body } => {
            writes_expr(&condition.node, re, ix);
            collect_writes(then_body, re, ix);
            collect_writes(else_body, re, ix);
        }
        Statement::For { var, iter, body, .. } => {
            // a loop variable written in the body is a copy with a new type
            let _ = var;
            writes_expr(&iter.node, re, ix);
            collect_writes(body, re, ix);
        }
        Statement::While { condition, body, .. } => { writes_expr(&condition.node, re, ix); collect_writes(body, re, ix); }
        Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => for a in args { writes_expr(&a.node, re, ix); },
        Statement::IndexSet { name, index, value } => {
            ix.insert(name.clone());
            writes_expr(&index.node, re, ix);
            writes_expr(&value.node, re, ix);
        }
        Statement::Ensure { condition } => writes_expr(&condition.node, re, ix),
        Statement::ExprStmt { expr } => writes_expr(&expr.node, re, ix),
        Statement::Require { .. } | Statement::Break | Statement::Continue => {}
    }
}

fn writes_expr(e: &Expr, re: &mut HashSet<String>, ix: &mut HashSet<String>) {
    match e {
        Expr::Literal(_) | Expr::Ident(_) => {}
        Expr::FieldAccess { target, .. } | Expr::Not(target) | Expr::Try(target) | Expr::TryPropagate(target) => writes_expr(&target.node, re, ix),
        Expr::Index { target, index } => { writes_expr(&target.node, re, ix); writes_expr(&index.node, re, ix); }
        Expr::MethodCall { target, args, .. } => { writes_expr(&target.node, re, ix); for a in args { writes_expr(&a.node, re, ix); } }
        Expr::FnCall { args, .. } | Expr::ListLiteral(args) => for a in args { writes_expr(&a.node, re, ix); },
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => {
            writes_expr(&left.node, re, ix); writes_expr(&right.node, re, ix);
        }
        Expr::Record { fields, .. } => for (_, v) in fields { writes_expr(&v.node, re, ix); },
        Expr::Match { subject, arms } => {
            writes_expr(&subject.node, re, ix);
            for a in arms {
                if let Some(g) = &a.guard { writes_expr(&g.node, re, ix); }
                collect_writes(&a.body, re, ix);
                writes_expr(&a.result.node, re, ix);
            }
        }
        Expr::Lambda { body, .. } => writes_expr(&body.node, re, ix),
        Expr::LambdaBlock { stmts, result, .. } => { collect_writes(stmts, re, ix); writes_expr(&result.node, re, ix); }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            writes_expr(&condition.node, re, ix);
            collect_writes(then_body, re, ix); writes_expr(&then_result.node, re, ix);
            collect_writes(else_body, re, ix); writes_expr(&else_result.node, re, ix);
        }
    }
}

// ── inference ───────────────────────────────────────────────────────────

struct Cx<'a, 'b> {
    p: &'a Program2<'b>,
    cell: &'a str,
    slots: &'a HashMap<String, Ty>,
    scopes: Vec<HashMap<String, Ty>>,
    reassigned: HashSet<String>,
    index_set: HashSet<String>,
    /// inside `try { }`: a failure there is a value, not a bug to refuse
    in_try: usize,
    out: &'a mut Vec<TypeIssue>,
    handler: &'a str,
    ret: Option<(Ty, String)>,
}

/// "an Int", "a String".
fn a(t: &Ty) -> String {
    let n = t.name();
    let an = n.starts_with(|c: char| "AEIOUaeiou".contains(c));
    format!("{} {}", if an { "an" } else { "a" }, n)
}

fn is_unit(e: &Expr) -> bool { matches!(e, Expr::Literal(Literal::Unit)) }

/// Names a true condition proves present: `x != ()`, joined by `&&`.
fn present_when_true(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::CmpOp { left, op: CmpOp::Ne, right } => {
            if let (Expr::Ident(n), true) = (&left.node, is_unit(&right.node)) { out.push(n.clone()); }
            if let (true, Expr::Ident(n)) = (is_unit(&left.node), &right.node) { out.push(n.clone()); }
        }
        Expr::BinaryOp { left, op: BinOp::And, right } => { present_when_true(&left.node, out); present_when_true(&right.node, out); }
        Expr::Not(inner) => present_when_false(&inner.node, out),
        _ => {}
    }
}

/// Names a false condition proves present: `x == ()`, joined by `||`.
fn present_when_false(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::CmpOp { left, op: CmpOp::Eq, right } => {
            if let (Expr::Ident(n), true) = (&left.node, is_unit(&right.node)) { out.push(n.clone()); }
            if let (true, Expr::Ident(n)) = (is_unit(&left.node), &right.node) { out.push(n.clone()); }
        }
        Expr::BinaryOp { left, op: BinOp::Or, right } => { present_when_false(&left.node, out); present_when_false(&right.node, out); }
        Expr::Not(inner) => present_when_true(&inner.node, out),
        _ => {}
    }
}

/// `require cond` is parsed as the comparison `cond == true`, the `true`
/// carrying the condition's own span.
fn bare_condition(c: &Constraint) -> Option<&Spanned<Expr>> {
    match c {
        Constraint::Comparison { left, op: CmpOp::Eq, right }
            if matches!(right.node, Expr::Literal(Literal::Bool(true))) && right.span.start == left.span.start && right.span.end == left.span.end => Some(left),
        _ => None,
    }
}

fn constraint_present(c: &Constraint, out: &mut Vec<String>) {
    if let Some(cond) = bare_condition(c) { return present_when_true(&cond.node, out); }
    match c {
        Constraint::Comparison { left, op: CmpOp::Ne, right } => {
            if let (Expr::Ident(n), true) = (&left.node, is_unit(&right.node)) { out.push(n.clone()); }
            if let (true, Expr::Ident(n)) = (is_unit(&left.node), &right.node) { out.push(n.clone()); }
        }
        Constraint::And(a, b) => { constraint_present(&a.node, out); constraint_present(&b.node, out); }
        _ => {}
    }
}

/// Does control never leave the end of this block?
fn diverges(body: &[Spanned<Statement>]) -> bool {
    match body.last().map(|s| &s.node) {
        Some(Statement::Return { .. } | Statement::Break | Statement::Continue) => true,
        Some(Statement::ExprStmt { expr }) => matches!(&expr.node, Expr::FnCall { name, .. } if name == "fail"),
        Some(Statement::If { then_body, else_body, .. }) => !else_body.is_empty() && diverges(then_body) && diverges(else_body),
        _ => false,
    }
}

// ── nominal types (`cell type Lots = Int`) ─────────────────────────────

/// The nominal type of a value, if it has one.
fn nominal(t: &Ty) -> Option<&str> {
    match t.present() { Ty::Nominal(n, _) => Some(n.as_str()), _ => None }
}

/// The builtin that leaves a nominal type for its base.
fn leave(t: &Ty) -> &'static str {
    match t.present() { Ty::Nominal(_, b) if **b == Ty::Float => "to_float", Ty::Nominal(_, b) if **b == Ty::Str => "to_string", _ => "to_int" }
}

/// A literal (`50`, `-5`, `2.5`): it takes the type of the other side.
fn is_literal(e: &Expr) -> bool {
    match e {
        Expr::Literal(_) => true,
        Expr::BinaryOp { left, op: BinOp::Sub, right } => matches!(left.node, Expr::Literal(Literal::Int(0))) && is_literal(&right.node),
        _ => false,
    }
}

/// A value whose type is lost on the way in: a Map field, an index, a get.
fn read_from_map(e: &Expr) -> bool {
    match e {
        Expr::FieldAccess { .. } | Expr::Index { .. } => true,
        Expr::MethodCall { method, .. } => method == "get",
        Expr::FnCall { name, args } if name == "_coalesce" => args.first().map_or(false, |a| read_from_map(&a.node)),
        _ => false,
    }
}

/// "a plain Int" / "an Int" for messages about the other side.
fn plain(t: &Ty) -> String { a(t.present()) }

impl<'a, 'b> Cx<'a, 'b> {
    fn nominal_issue(&mut self, span: Span, message: String) {
        self.issue("nominal_mismatch", span, message);
    }

    /// A value of unknown type meeting a nominal one: a warning (the
    /// program from the report read its cap from a Map).
    fn nominal_unknown(&mut self, other: &Spanned<Expr>, n: &str) {
        if self.in_try > 0 || self.out.iter().any(|i| i.span.start == other.span.start && i.kind == "nominal_mismatch") { return; }
        let src = render_expr(&other.node);
        self.out.push(TypeIssue { join: None, advisory: true, kind: "nominal_mismatch", span: other.span, message: format!(
            "`{src}` has no known type (it is read from a Map or an untyped value), so it cannot be checked against {n} — make it a record field or a typed parameter (`qty: {n}`) to have it checked") });
    }

    /// `L op x`: the nominal rules, or None when neither side is nominal.
    fn nominal_arith(&mut self, left: &Spanned<Expr>, l: &Ty, op: BinOp, right: &Spanned<Expr>, r: &Ty, span: Span) -> Option<Ty> {
        let (ln, rn) = (nominal(l).map(str::to_string), nominal(r).map(str::to_string));
        if ln.is_none() && rn.is_none() { return None; }
        let (ls, rs) = (render_expr(&left.node), render_expr(&right.node));
        let ratio = || Ty::Num;
        match (ln, rn) {
            (Some(a), Some(b)) => {
                if op == BinOp::Div { return Some(ratio()); }
                if a == b && matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mod) { return Some(l.present().clone()); }
                let (lf, rf) = (leave(l), leave(r));
                let msg = match op {
                    BinOp::Mul if a == b => format!("`{ls} * {rs}` multiplies {a} by {a}, which is not {a} — scale by a plain number (`{ls} * 2`), or leave the type: `{lf}({ls}) * {rf}({rs})`"),
                    BinOp::Mul => format!("`{ls} * {rs}` multiplies {a} by {b} — leave the types and say what the product counts: `X({lf}({ls}) * {rf}({rs}))` with X the type of the result"),
                    BinOp::Add => format!("`{ls} + {rs}` adds {b} to {a} — two nominal types never mix: convert one side if that is meant (`{a}({rf}({rs}))`)"),
                    BinOp::Sub => format!("`{ls} - {rs}` subtracts {b} from {a} — two nominal types never mix: convert one side if that is meant (`{a}({rf}({rs}))`)"),
                    _ => format!("`{ls} {op} {rs}` mixes {a} and {b} — two nominal types never mix: convert one side if that is meant (`{a}({rf}({rs}))`)"),
                };
                self.nominal_issue(span, msg);
                Some(Ty::Unknown)
            }
            (Some(n), None) | (None, Some(n)) => {
                let (nt, other, ot) = if nominal(l).is_some() { (l, right, r) } else { (r, left, l) };
                let (ns, os) = if nominal(l).is_some() { (&ls, &rs) } else { (&rs, &ls) };
                if op == BinOp::Div { return Some(ratio()); }
                if is_literal(&other.node) { return Some(nt.present().clone()); }
                if matches!(ot, Ty::Unknown) || ot.is_opt() && matches!(ot.present(), Ty::Unknown) {
                    if read_from_map(&other.node) { self.nominal_unknown(other, &n); }
                    return Some(nt.present().clone());
                }
                if ot.class() != Some(Class::Num) { return None; } // a String with a Lots: the base rules say it
                let f = leave(nt);
                let what = plain(ot);
                let msg = match op {
                    BinOp::Mul => format!("`{ls} * {rs}` multiplies {n} by {what} — if `{os}` is a plain multiplier write `{n}({f}({ns}) * {os})`; a literal (`{ns} * 2`) keeps {n}"),
                    BinOp::Add => format!("`{ls} + {rs}` adds {what} to {n} — say what `{os}` counts: `{n}({os})` if it is {n}, or `{f}({ns}) + {os}` to leave the type"),
                    BinOp::Sub => format!("`{ls} - {rs}` subtracts {what} from {n} — say what `{os}` counts: `{n}({os})` if it is {n}, or `{f}({ns}) - {os}` to leave the type"),
                    _ => format!("`{ls} {op} {rs}` mixes {n} and {what} — say what `{os}` counts: `{n}({os})`, or leave the type: `{f}({ns})`"),
                };
                self.nominal_issue(span, msg);
                Some(Ty::Unknown)
            }
            (None, None) => None,
        }
    }

    /// `L cmp x`: the nominal rules, or None when neither side is nominal.
    fn nominal_compare(&mut self, left: &Spanned<Expr>, l: &Ty, op: CmpOp, right: &Spanned<Expr>, r: &Ty, span: Span) -> Option<Ty> {
        let (ln, rn) = (nominal(l).map(str::to_string), nominal(r).map(str::to_string));
        if ln.is_none() && rn.is_none() { return None; }
        let (ls, rs) = (render_expr(&left.node), render_expr(&right.node));
        match (ln, rn) {
            (Some(a), Some(b)) if a == b => Some(Ty::Bool),
            (Some(a), Some(b)) => {
                let (lf, rf) = (leave(l), leave(r));
                self.nominal_issue(span, format!(
                    "`{ls} {op} {rs}` compares {a} with {b} — two nominal types never compare: if both sides count {a}, declare the {b} side as {a} (`{a}(…)`, `const … = {a}(…)`); if a conversion is meant, write it (`{lf}({ls})`, `{rf}({rs})`)"));
                Some(Ty::Bool)
            }
            (Some(n), None) | (None, Some(n)) => {
                let (nt, other, ot) = if nominal(l).is_some() { (l, right, r) } else { (r, left, l) };
                let (ns, os) = if nominal(l).is_some() { (&ls, &rs) } else { (&rs, &ls) };
                if is_literal(&other.node) || matches!(other.node, Expr::Literal(Literal::Unit)) { return Some(Ty::Bool); }
                if matches!(ot.present(), Ty::Unknown) {
                    if read_from_map(&other.node) { self.nominal_unknown(other, &n); }
                    return Some(Ty::Bool);
                }
                if ot.class() != nt.class() { return None; }
                let f = leave(nt);
                self.nominal_issue(span, format!(
                    "`{ls} {op} {rs}` compares {n} with {} — say what `{os}` counts: `{n}({os})` if it is {n}, or leave the type: `{f}({ns})`", plain(ot)));
                Some(Ty::Bool)
            }
            (None, None) => None,
        }
    }

    /// Can a value of type `actual` stand where `declared` is required, as
    /// far as nominal types go? Some((what the value is, the fix)) when not.
    fn nominal_fit(&self, value: &Spanned<Expr>, actual: &Ty, declared: &Ty) -> Option<(String, String)> {
        let (an, dn) = (nominal(actual), nominal(declared));
        if an.is_none() && dn.is_none() { return None; }
        if is_literal(&value.node) || matches!(actual.present(), Ty::Unknown) || matches!(actual, Ty::Unit) { return None; }
        let src = render_expr(&value.node);
        match (an, dn) {
            (Some(a), Some(d)) if a == d => None,
            (Some(a), Some(d)) => Some((a.to_string(), format!("two nominal types never convert silently: `{d}({}({src}))` if that is meant", leave(actual)))),
            (Some(a), None) => Some((a.to_string(), format!("leave the type: `{}({src})`", leave(actual)))),
            (None, Some(d)) if actual.class() == declared.class() => Some((plain(actual), format!("say what it counts: `{d}({src})`"))),
            (None, Some(_)) => None, // a String for a Lots: the base rules say it
            (None, None) => None,
        }
    }

    /// A write into a slot of a nominal type (or of a plain one).
    fn slot_write(&mut self, target: &str, method: &str, args: &[Spanned<Expr>], arg_tys: &[Ty]) {
        if self.lookup(target).is_some() { return; }
        let (Some(slot), Some((v, vt))) = (self.slots.get(target).cloned(), args.last().zip(arg_tys.last())) else { return };
        let elem = match (&slot, method, args.len()) {
            (Ty::Map(e), "set" | "put", 2) | (Ty::List(e), "push", 1) | (Ty::List(e), "set", 2) => (**e).clone(),
            _ => return,
        };
        if let Some((is, fix)) = self.nominal_fit(v, vt, &elem) {
            let msg = format!("`{target}.{method}(…, {})` writes {is} into `{target}`, a slot of {} — {fix}", render_expr(&v.node), elem.name());
            self.nominal_issue(v.span, msg);
        }
    }

    fn bind(&mut self, name: &str, ty: Ty) {
        let ty = if self.reassigned.contains(name) {
            Ty::Unknown
        } else if self.index_set.contains(name) {
            // an element write keeps the container, not what it holds
            match ty {
                Ty::List(_) => Ty::List(Box::new(Ty::Unknown)),
                Ty::Map(_) => Ty::Map(Box::new(Ty::Unknown)),
                other => other,
            }
        } else { ty };
        self.scopes.last_mut().unwrap().insert(name.to_string(), ty);
    }

    fn lookup(&self, name: &str) -> Option<Ty> {
        self.scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    fn narrow(&mut self, names: &[String]) {
        for n in names {
            if let Some(Ty::Opt(inner)) = self.lookup(n) {
                self.scopes.last_mut().unwrap().insert(n.clone(), *inner);
            }
        }
    }

    fn issue(&mut self, kind: &'static str, span: Span, message: String) {
        if self.in_try > 0 { return; }
        if self.out.iter().any(|i| i.span.start == span.start && i.kind == kind) { return; }
        self.out.push(TypeIssue { join: None, advisory: false, kind, message, span });
    }

    /// Refused from edition 2027 on; until then a warning that says so.
    fn issue_next_edition(&mut self, kind: &'static str, span: Span, message: String) {
        if self.p.strict_absence { return self.issue(kind, span, message); }
        if self.in_try > 0 || self.out.iter().any(|i| i.span.start == span.start && i.kind == kind) { return; }
        self.out.push(TypeIssue { join: None, advisory: true, kind, message: format!("{message} (an error in edition 2027)"), span });
    }

    fn scoped<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(HashMap::new());
        let r = f(self);
        self.scopes.pop();
        r
    }

    fn block(&mut self, body: &[Spanned<Statement>]) {
        for s in body { self.stmt(s); }
    }

    fn condition(&mut self, cond: &Spanned<Expr>, what: &str) {
        let t = self.infer(cond);
        if let Some(c) = t.class() {
            if c != Class::Bool && !t.is_opt() {
                self.issue("type_mismatch", cond.span, format!(
                    "the condition of `{what}` is {}, not a Bool: `{}` — compare it (`x != 0`, `len(xs) > 0`, `s != \"\"`)",
                    a(&t), render_expr(&cond.node)));
            }
        }
    }

    fn stmt(&mut self, s: &Spanned<Statement>) {
        match &s.node {
            Statement::Let { name, value } => {
                let t = self.infer(value);
                self.bind(name, t);
            }
            Statement::Assign { value, .. } => { self.infer(value); }
            Statement::Return { value } => {
                let t = self.infer(value);
                // `request` answers whatever a route returns: its face type is not a contract
                if let (Some((declared, text)), true) = (self.ret.clone(), self.handler != "request") {
                    if let Some((is, fix)) = self.nominal_fit(value, &t, &declared) {
                        let msg = format!("handler '{}' returns {is} (`{}`) but its face declares `-> {}` — {fix}", self.handler, render_expr(&value.node), text);
                        self.nominal_issue(value.span, msg);
                    } else if !assignable(&t, &declared) && !matches!(value.node, Expr::Literal(_)) {
                        // the runtime validates only record / sum returns today
                        self.issue_next_edition("type_mismatch", value.span, format!(
                            "handler '{}' returns {} (`{}`) but its face declares `-> {}` — fix the value or the face",
                            self.handler, a(&t), render_expr(&value.node), text));
                    }
                }
            }
            Statement::If { condition, then_body, else_body } => {
                self.condition(condition, "if");
                let mut then_present = Vec::new();
                present_when_true(&condition.node, &mut then_present);
                let mut else_present = Vec::new();
                present_when_false(&condition.node, &mut else_present);
                self.scoped(|cx| { cx.narrow(&then_present); cx.block(then_body) });
                self.scoped(|cx| { cx.narrow(&else_present); cx.block(else_body) });
                // `if x == () { return … }`: x is present from here on
                if diverges(then_body) { self.narrow(&else_present); }
                if !else_body.is_empty() && diverges(else_body) { self.narrow(&then_present); }
            }
            Statement::For { var, iter, body, .. } => {
                let t = self.infer(iter);
                let elem = match t.present() {
                    Ty::List(e) => (**e).clone(),
                    Ty::Str => Ty::Str,
                    // over `()` a `for` runs zero times: a `T?` is not refused
                    Ty::Int | Ty::Float | Ty::Num | Ty::Bool if !t.is_opt() => {
                        self.issue("type_mismatch", iter.span, format!(
                            "`for` over {}: `{}` — iterate a list (`for i in range(0, n)`), the lines of a String, or `keys(m)` / `entries(m)` of a map",
                            a(&t), render_expr(&iter.node)));
                        Ty::Unknown
                    }
                    _ => Ty::Unknown,
                };
                self.scoped(|cx| { cx.bind(var, elem); cx.block(body) });
            }
            Statement::While { condition, body, .. } => {
                self.condition(condition, "while");
                self.scoped(|cx| cx.block(body));
            }
            Statement::Emit { args, .. } => for a in args { self.infer(a); },
            Statement::Require { constraint, .. } => {
                self.constraint(&constraint.node);
                let mut present = Vec::new();
                constraint_present(&constraint.node, &mut present);
                self.narrow(&present);
            }
            Statement::MethodCall { target, method, args } => {
                let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(a)).collect();
                self.slot_write(target, method, args, &arg_tys);
                if self.lookup(target).is_none() && !self.slots.contains_key(target) && self.p.cells.contains(target) {
                    self.call_handler(method, Some(target), args, &arg_tys, s.span);
                }
            }
            Statement::IndexSet { index, value, .. } => { self.infer(index); self.infer(value); }
            Statement::Ensure { condition } => self.condition(condition, "ensure"),
            Statement::ExprStmt { expr } => { self.infer(expr); }
            Statement::Break | Statement::Continue => {}
        }
    }

    fn constraint(&mut self, c: &Constraint) {
        if let Some(cond) = bare_condition(c) { return self.condition(cond, "require"); }
        match c {
            Constraint::Comparison { left, op, right } => {
                let (l, r) = (self.infer(left), self.infer(right));
                self.compare(left, &l, *op, right, &r, left.span.merge(right.span));
            }
            Constraint::Predicate { args, .. } => for a in args { self.infer(a); },
            Constraint::And(a, b) | Constraint::Or(a, b) => { self.constraint(&a.node); self.constraint(&b.node); }
            Constraint::Not(a) => self.constraint(&a.node),
            Constraint::Descriptive(_) => {}
        }
    }

    /// A `T?` where a value is required (edition 2027).
    fn require_present(&mut self, e: &Spanned<Expr>, t: &Ty, doing: &str) {
        if !self.p.strict_absence || !t.is_opt() { return; }
        let src = render_expr(&e.node);
        let default = match t.present() {
            Ty::Int | Ty::Num => "0", Ty::Float => "0.0", Ty::Str => "\"\"", Ty::Bool => "false",
            Ty::List(_) => "[]", Ty::Map(_) => "map()", _ => "…",
        };
        self.issue("maybe_absent", e.span, format!(
            "`{src}` may be absent (`()`) {doing} — default it (`({src} ?? {default})`) or test it first (`require {src} != () else Missing`)"));
    }

    fn arithmetic(&mut self, left: &Spanned<Expr>, l: &Ty, op: BinOp, right: &Spanned<Expr>, r: &Ty, span: Span) -> Ty {
        // a literal `()` operand: every execution raises
        let unit = matches!(l, Ty::Unit) || matches!(r, Ty::Unit);
        if !unit {
            self.require_present(left, l, &format!("in `{op}`"));
            self.require_present(right, r, &format!("in `{op}`"));
        }
        if !unit {
            if let Some(t) = self.nominal_arith(left, l, op, right, r, span) { return t; }
        }
        let (lc, rc) = (l.class(), r.class());
        let ok = match (lc, rc) {
            _ if unit => matches!(l, Ty::Unknown) && matches!(r, Ty::Unknown),
            (None, _) | (_, None) => true,
            (Some(Class::Fn), _) | (_, Some(Class::Fn)) => true,
            (Some(Class::Num), Some(Class::Num)) => true,
            (Some(Class::Num), Some(Class::List)) | (Some(Class::List), Some(Class::Num)) | (Some(Class::List), Some(Class::List)) => op != BinOp::Mod,
            (Some(Class::Str), Some(Class::Str)) => op == BinOp::Add,
            _ => false,
        };
        if !ok {
            let verb = match op { BinOp::Add => "add", BinOp::Sub => "subtract", BinOp::Mul => "multiply", BinOp::Div => "divide", _ => "take the remainder of" };
            let fix = match (lc, rc, op) {
                (Some(Class::Str), Some(Class::Num), BinOp::Add) | (Some(Class::Num), Some(Class::Str), BinOp::Add) =>
                    "build the text with interpolation (`\"{a}{b}\"`) or to_string(n); for a sum, parse the text first (to_int, to_float)".to_string(),
                (Some(Class::Str), _, BinOp::Mul) | (_, Some(Class::Str), BinOp::Mul) =>
                    "to repeat text use pad_left / pad_right or a loop; for a product, parse the text first (to_int, to_float)".to_string(),
                _ if unit => "`()` is the absent value: default it with `?? 0` before the operation".to_string(),
                _ => "convert one side (to_int, to_float, to_string) or fix the value".to_string(),
            };
            self.issue("type_mismatch", span, format!(
                "cannot {verb} {} and {}: `{} {op} {}` — {fix}",
                a(l), a(r), render_expr(&left.node), render_expr(&right.node)));
            return Ty::Unknown;
        }
        // edition 2027: `+` on lists is element-wise arithmetic only — lists
        // of anything else are joined with `++`
        if op == BinOp::Add && self.p.strict_absence {
            if let (Ty::List(a), Ty::List(b)) = (l.present(), r.present()) {
                let not_num = |t: &Ty| matches!(t.class(), Some(c) if c != Class::Num);
                if not_num(a) || not_num(b) {
                    self.issue("type_mismatch", span, format!(
                        "`{} + {}` joins two lists — in edition 2027 `+` on lists is element-wise arithmetic only; join with `++` (`{} ++ {}`)",
                        render_expr(&left.node), render_expr(&right.node), render_expr(&left.node), render_expr(&right.node)));
                    if let Some(last) = self.out.last_mut() {
                        if last.span.start == span.start && last.kind == "type_mismatch" { last.join = Some((left.span, right.span)); }
                    }
                }
            }
        }
        match (l.present(), r.present()) {
            (Ty::List(_), _) | (_, Ty::List(_)) => Ty::List(Box::new(Ty::Unknown)),
            (Ty::Str, Ty::Str) => Ty::Str,
            // `7 / 2` is 3.5 and `6 / 3` is 2
            (Ty::Int, Ty::Int) if op == BinOp::Div => Ty::Num,
            (Ty::Int, Ty::Int) => Ty::Int,
            (Ty::Float, Ty::Int | Ty::Float | Ty::Num) | (Ty::Int | Ty::Num, Ty::Float) => Ty::Float,
            (Ty::Int | Ty::Num, Ty::Int | Ty::Num) => Ty::Num,
            _ => Ty::Unknown,
        }
    }

    fn compare(&mut self, left: &Spanned<Expr>, l: &Ty, op: CmpOp, right: &Spanned<Expr>, r: &Ty, span: Span) -> Ty {
        let equality = matches!(op, CmpOp::Eq | CmpOp::Ne);
        // `x == ()` is the presence test, for every type
        if equality && (matches!(l, Ty::Unit) || matches!(r, Ty::Unit)) { return Ty::Bool; }
        if !equality {
            self.require_present(left, l, &format!("in `{op}`"));
            self.require_present(right, r, &format!("in `{op}`"));
        }
        if let Some(t) = self.nominal_compare(left, l, op, right, r, span) { return t; }
        let (Some(lc), Some(rc)) = (l.class(), r.class()) else {
            // an ordering against the literal `()` always raises
            if !equality && (matches!(l, Ty::Unit) && r.class().is_some() || matches!(r, Ty::Unit) && l.class().is_some()) {
                self.issue("type_mismatch", span, format!(
                    "`{} {op} {}` orders a value against `()` — that raises kind `type`; test presence with `== ()` / `!= ()`",
                    render_expr(&left.node), render_expr(&right.node)));
            }
            return Ty::Unknown;
        };
        if lc == Class::Fn || rc == Class::Fn { return Ty::Unknown; }
        // `() == x` is false, not an error: a `T?` side settles nothing
        if equality && (l.is_opt() || r.is_opt()) { return Ty::Bool; }
        let mask = (lc == Class::List && rc == Class::Num) || (lc == Class::Num && rc == Class::List);
        let ok = if equality {
            lc == rc || mask
        } else {
            mask || (lc == rc && matches!(lc, Class::Num | Class::Str))
        };
        if !ok {
            let why = if equality {
                format!("compares {} with {} — that raises kind `type` at run time; convert one side, or the values can never be equal", a(l), a(r))
            } else if lc == rc {
                format!("orders two {}s — only numbers and Strings have an order; compare a field or a key (`a.total < b.total`, `len(a) < len(b)`)", l.name())
            } else {
                format!("orders {} against {} — that raises kind `type` at run time; convert one side", a(l), a(r))
            };
            self.issue("type_mismatch", span, format!("`{} {op} {}` {why}", render_expr(&left.node), render_expr(&right.node)));
            return Ty::Unknown;
        }
        if mask { Ty::List(Box::new(Ty::Unknown)) } else { Ty::Bool }
    }

    fn logical(&mut self, e: &Spanned<Expr>, t: &Ty, op: &str) {
        if let Some(c) = t.class() {
            if c != Class::Bool && !t.is_opt() {
                self.issue("type_mismatch", e.span, format!(
                    "`{op}` needs Bools, and `{}` is {} — compare it (`x != 0`, `len(xs) > 0`, `s != \"\"`)",
                    render_expr(&e.node), a(t)));
            }
        }
    }

    fn field(&mut self, target: &Spanned<Expr>, t: &Ty, field: &str, span: Span) -> Ty {
        if pseudo_field(field) {
            return match (field, t.present()) {
                ("len" | "size", Ty::List(_) | Ty::Str) => Ty::Int,
                ("first" | "last", Ty::List(e)) => Ty::opt((**e).clone()),
                _ => Ty::Unknown,
            };
        }
        let src = render_expr(&target.node);
        if t.is_opt() && matches!(t.present(), Ty::Record(_) | Ty::Variant(_) | Ty::Try(_)) {
            self.require_present(target, t, &format!("when `.{field}` is read"));
        }
        match t.present().clone() {
            Ty::Record(name) => {
                let fields = self.p.records.get(&name).cloned().unwrap_or_default();
                match fields.iter().find(|(f, _)| f == field) {
                    Some((_, ft)) => ft.clone(),
                    None => {
                        let names: Vec<String> = fields.iter().map(|(f, _)| f.clone()).collect();
                        let near = crate::checker::names::suggest(field, names.iter()).map(|s| format!(" (did you mean '{s}'?)")).unwrap_or_default();
                        self.issue("unknown_field", span, format!(
                            "`{src}.{field}`: record type '{name}' has no field '{field}'{near} — its fields are [{}]; an undeclared field reads as `()`",
                            names.join(", ")));
                        Ty::Unknown
                    }
                }
            }
            Ty::Variant(name) => {
                let fields = self.p.sums.get(&name).cloned().unwrap_or_default();
                match fields.iter().find(|(f, _)| f == field) {
                    // present in SOME variant: `()` in the others
                    Some((_, ft)) => Ty::opt(ft.clone()),
                    None => {
                        let names: Vec<String> = fields.iter().map(|(f, _)| f.clone()).collect();
                        let near = crate::checker::names::suggest(field, names.iter()).map(|s| format!(" (did you mean '{s}'?)")).unwrap_or_default();
                        self.issue("unknown_field", span, format!(
                            "`{src}.{field}`: no variant of '{name}' has a field '{field}'{near} — the fields its variants declare are [{}]; `match` on the variant to read them",
                            names.join(", ")));
                        Ty::Unknown
                    }
                }
            }
            Ty::Try(inner) => match field {
                "value" => Ty::opt(*inner),
                "error" | "kind" | "detail" => Ty::opt(Ty::Str),
                _ => {
                    self.issue("unknown_field", span, format!(
                        "`{src}.{field}`: a try result has only `value`, `error`, `kind` and `detail` — the value's own field is `{src}.value.{field}` (after `if {src}.error != () {{ … }}`)"));
                    Ty::Unknown
                }
            },
            Ty::Map(v) => Ty::opt(*v),
            Ty::Int | Ty::Float | Ty::Num | Ty::Bool if !t.is_opt() => {
                self.issue("type_mismatch", span, format!(
                    "`{src}.{field}`: `{src}` is {}, which has no fields", a(t.present())));
                Ty::Unknown
            }
            _ => Ty::Unknown,
        }
    }

    /// A call that resolves to exactly one handler: its arguments are
    /// checked against the declared parameters, its result is the face's.
    fn call_handler(&mut self, name: &str, cell: Option<&str>, args: &[Spanned<Expr>], arg_tys: &[Ty], _span: Span) -> Option<Ty> {
        let sigs = self.p.handlers.get(name)?;
        let candidates: Vec<&Sig> = match cell {
            Some(c) => sigs.iter().filter(|s| s.cell == c).collect(),
            None => {
                let own: Vec<&Sig> = sigs.iter().filter(|s| s.cell == self.cell).collect();
                if own.is_empty() { sigs.iter().collect() } else { own }
            }
        };
        let [sig] = candidates.as_slice() else { return Some(Ty::Unknown) };
        if sig.params.len() != args.len() {
            // an arity the handler does not take reaches the builtin of that name
            return if cell.is_some() || !crate::checker::names::builtin_names().contains(name) { Some(Ty::Unknown) } else { None };
        }
        let mut issues = Vec::new();
        let mut nominal_args: Vec<Span> = Vec::new();
        for (i, ((pname, pty, ptext), (arg, aty))) in sig.params.iter().zip(args.iter().zip(arg_tys)).enumerate() {
            if matches!(arg.node, Expr::Literal(_)) { continue; } // literals: checker/literals.rs
            if let Some((is, fix)) = self.nominal_fit(arg, aty, pty) {
                issues.push((arg.span, format!("{name}(): argument {} (`{}`) is {is} but the handler declares `{pname}: {ptext}` — {fix}", i + 1, render_expr(&arg.node))));
                nominal_args.push(arg.span);
                continue;
            }
            if !assignable(aty, pty) {
                issues.push((arg.span, format!(
                    "{name}(): argument {} (`{}`) is {} but the handler declares `{pname}: {ptext}` — fix the value or the parameter type",
                    i + 1, render_expr(&arg.node), a(aty))));
            }
        }
        for (span, message) in issues {
            if nominal_args.contains(&span) { self.nominal_issue(span, message); } else { self.issue("argument_type", span, message); }
        }
        Some(sig.ret.as_ref().map(|(t, _)| t.clone()).unwrap_or(Ty::Unknown))
    }

    fn infer(&mut self, e: &Spanned<Expr>) -> Ty {
        match &e.node {
            Expr::Literal(l) => match l {
                Literal::Int(_) | Literal::BigInt(_) => Ty::Int,
                Literal::Float(_) => Ty::Float,
                Literal::String(_) => Ty::Str,
                Literal::Bool(_) => Ty::Bool,
                Literal::Unit => Ty::Unit,
                Literal::Duration(_) | Literal::Percentage(_) => Ty::Unknown,
            },
            Expr::Ident(name) => {
                if let Some(t) = self.lookup(name) { return t; }
                // a slot read whole: a copy of the Map / List
                self.slots.get(name).cloned().unwrap_or(Ty::Unknown)
            }
            Expr::FieldAccess { target, field } => {
                // `slot.len`, `slot.keys`: a slot answers these itself
                if let Expr::Ident(n) = &target.node {
                    if self.lookup(n).is_none() {
                        if self.slots.contains_key(n) {
                            return if matches!(field.as_str(), "len" | "size") { Ty::Int } else { Ty::Unknown };
                        }
                        if self.p.cells.contains(n) { return Ty::Unknown; }
                    }
                }
                let t = self.infer(target);
                self.field(target, &t, field, e.span)
            }
            Expr::Index { target, index } => {
                let t = self.infer(target);
                let i = self.infer(index);
                if t.is_opt() { self.require_present(target, &t, "when it is indexed"); }
                match t.present() {
                    Ty::List(elem) => {
                        if matches!(i.class(), Some(Class::Str | Class::Bool | Class::List | Class::MapLike)) && !i.is_opt() && !t.is_opt() {
                            self.issue("type_mismatch", index.span, format!(
                                "`{}` is a List, indexed by position — `{}` is {}, not an Int",
                                render_expr(&target.node), render_expr(&index.node), a(&i)));
                        }
                        (**elem).clone()
                    }
                    Ty::Map(v) => Ty::opt((**v).clone()),
                    Ty::Record(_) | Ty::Variant(_) | Ty::Try(_) => match &index.node {
                        Expr::Literal(Literal::String(f)) if !f.contains('{') => self.field(target, &t, f, e.span),
                        _ => Ty::Unknown,
                    },
                    _ => Ty::Unknown,
                }
            }
            Expr::MethodCall { target, method, args } => {
                let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(a)).collect();
                if let Expr::Ident(n) = &target.node {
                    if self.lookup(n).is_none() {
                        if let Some(slot) = self.slots.get(n).cloned() {
                            self.slot_write(n, method, args, &arg_tys);
                            return match (method.as_str(), &slot) {
                                ("get", Ty::Map(v)) | ("get", Ty::List(v)) => Ty::opt((**v).clone()),
                                ("len" | "size", _) => Ty::Int,
                                _ => Ty::Unknown,
                            };
                        }
                        if self.p.cells.contains(n) {
                            return self.call_handler(method, Some(n), args, &arg_tys, e.span).unwrap_or(Ty::Unknown);
                        }
                    }
                }
                let t = self.infer(target);
                // `xs.len()` / `s.uppercase()`: a builtin as a method, when
                // no handler takes that name
                if self.p.handlers.contains_key(method.as_str()) || matches!(t, Ty::Unknown) { return Ty::Unknown; }
                match (method.as_str(), t.present()) {
                    ("get", Ty::Map(v)) => Ty::opt((**v).clone()),
                    ("get", _) => Ty::Unknown,
                    _ => builtin_result(method, args.len() + 1, false),
                }
            }
            Expr::FnCall { name, args } => {
                let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer(a)).collect();
                // 3.0 durable waits: an approval is a Bool, a sleep gives nothing
                if name == "await_approve" { return Ty::Bool; }
                if name == "await_sleep" { return Ty::Unit; }
                // `a ?? b`: a, or b when a is `()`
                if name == "_coalesce" && arg_tys.len() == 2 {
                    // `pos.get(k) ?? 0` on a slot of Lots: the default takes the type
                    if nominal(&arg_tys[0]).is_some() && (is_literal(&args[1].node) || nominal(&arg_tys[1]) == nominal(&arg_tys[0])) {
                        return arg_tys[0].present().clone();
                    }
                    return match (&arg_tys[0], &arg_tys[1]) {
                        (Ty::Unknown, _) => Ty::Unknown,
                        (Ty::Unit, r) => r.clone(),
                        (l, r) => join(l.present(), r),
                    };
                }
                // `think_json(prompt, map("as", "Decision"))` is a Decision
                if name == "think_json" {
                    for a in args {
                        let Expr::FnCall { name: m, args: kv } = &a.node else { continue };
                        if m != "map" { continue; }
                        for pair in kv.chunks(2) {
                            let [k, v] = pair else { continue };
                            if !matches!(&k.node, Expr::Literal(Literal::String(k)) if k == "as") { continue; }
                            let Expr::Literal(Literal::String(t)) = &v.node else { return Ty::Unknown };
                            if self.p.records.contains_key(t) { return Ty::Record(t.clone()); }
                            if self.p.sums.contains_key(t) { return Ty::Variant(t.clone()); }
                            let mut known: Vec<String> = self.p.records.keys().chain(self.p.sums.keys()).cloned().collect();
                            known.sort();
                            let near = crate::checker::names::suggest(t, known.iter()).map(|s| format!(" (did you mean \"{s}\"?)")).unwrap_or_default();
                            self.issue("think_option", v.span, format!(
                                "think_json(): `as` names \"{t}\"{near}, which is not a declared `cell type` — declare it (`cell type {t} {{ variants {{ … }} }}`); the declared types are [{}]",
                                known.join(", ")));
                            return Ty::Unknown;
                        }
                    }
                    return Ty::Map(Box::new(Ty::Unknown));
                }
                // `Lots(x)`: the explicit way into a nominal type
                if let Some(nt) = nominal_named(name) {
                    if let (Some(arg), Some(at)) = (args.first(), arg_tys.first()) {
                        let src = render_expr(&arg.node);
                        let Ty::Nominal(_, base) = &nt else { unreachable!() };
                        if let Some(other) = nominal(at) {
                            if other != name.as_str() {
                                self.nominal_issue(e.span, format!(
                                    "`{name}({src})`: `{src}` is {other} — two nominal types never convert silently: `{name}({}({src}))` if that is meant", leave(at)));
                            }
                        } else if !assignable(at, base) || (matches!(**base, Ty::Int) && matches!(at.present(), Ty::Float)) {
                            self.nominal_issue(e.span, format!(
                                "`{name}({src})`: {name} is {}, and `{src}` is {} — fix the value", a(base), a(at)));
                        }
                    }
                    return nt;
                }
                // leaving a nominal type
                if matches!(name.as_str(), "to_int" | "to_float" | "to_string") && arg_tys.len() == 1 && nominal(&arg_tys[0]).is_some() {
                    return match name.as_str() { "to_int" => Ty::Int, "to_float" => Ty::Float, _ => Ty::Str };
                }
                if let Some(n) = arg_tys.first().and_then(|t| nominal(t)).map(str::to_string) {
                    let same = |t: &Ty, e: &Spanned<Expr>| nominal(t) == Some(n.as_str()) || is_literal(&e.node);
                    match name.as_str() {
                        "abs" if arg_tys.len() == 1 => return arg_tys[0].present().clone(),
                        "min" | "max" if args.iter().zip(&arg_tys).all(|(e, t)| same(t, e)) => return arg_tys[0].present().clone(),
                        "idiv" | "mod" if arg_tys.len() == 2 => {
                            if is_literal(&args[1].node) || matches!(arg_tys[1].present(), Ty::Unknown) { return arg_tys[0].present().clone(); }
                            let (ns, os) = (render_expr(&args[0].node), render_expr(&args[1].node));
                            self.nominal_issue(e.span, format!(
                                "`{name}({ns}, {os})` divides {n} by {} — if `{os}` is a plain divisor write `{n}({name}({}({ns}), {os}))`; a literal (`{name}({ns}, 2)`) keeps {n}",
                                plain(&arg_tys[1]), leave(&arg_tys[0])));
                            return Ty::Unknown;
                        }
                        _ => {}
                    }
                }
                if name == "sum" && arg_tys.len() == 1 {
                    if let Ty::List(el) = arg_tys[0].present() { if nominal(el).is_some() { return (**el).clone(); } }
                }
                // a local holding a lambda
                if self.lookup(name).is_some() { return Ty::Unknown; }
                if let Some(t) = self.call_handler(name, None, args, &arg_tys, e.span) { return t; }
                let first_str = matches!(args.first().map(|a| &a.node), Some(Expr::Literal(Literal::String(_))));
                builtin_result(name, args.len(), first_str)
            }
            Expr::BinaryOp { left, op, right } => {
                // `a ?? b` reaches here as its own operator in some forms; `&&` / `||` are logical
                let l = self.infer(left);
                match op {
                    BinOp::And | BinOp::Or => {
                        let sym = if *op == BinOp::And { "&&" } else { "||" };
                        self.logical(left, &l, sym);
                        // the right side runs only when the left allows it
                        let mut present = Vec::new();
                        if *op == BinOp::And { present_when_true(&left.node, &mut present); } else { present_when_false(&left.node, &mut present); }
                        let r = self.scoped(|cx| { cx.narrow(&present); cx.infer(right) });
                        self.logical(right, &r, sym);
                        Ty::Bool
                    }
                    _ => {
                        let r = self.infer(right);
                        self.arithmetic(left, &l, *op, right, &r, e.span)
                    }
                }
            }
            Expr::CmpOp { left, op, right } => {
                let (l, r) = (self.infer(left), self.infer(right));
                self.compare(left, &l, *op, right, &r, e.span)
            }
            Expr::Not(inner) => {
                let t = self.infer(inner);
                self.logical(inner, &t, "!");
                Ty::Bool
            }
            Expr::Record { type_name, fields } => {
                let tys: Vec<Ty> = fields.iter().map(|(_, v)| self.infer(v)).collect();
                if let Some(decl) = self.p.records.get(type_name).cloned() {
                    for ((fname, value), vt) in fields.iter().zip(&tys) {
                        if matches!(value.node, Expr::Literal(_)) { continue; }
                        if let Some((_, ft)) = decl.iter().find(|(f, _)| f == fname) {
                            if let Some((is, fix)) = self.nominal_fit(value, vt, ft) {
                                let msg = format!("{type_name} {{ {fname}: … }}: `{}` is {is} but the field is declared {} — {fix}", render_expr(&value.node), ft.name());
                                self.nominal_issue(value.span, msg);
                            } else if !assignable(vt, ft) {
                                self.issue("type_mismatch", value.span, format!(
                                    "{type_name} {{ {fname}: … }}: `{}` is {} but the field is declared {} — fix the value or the type",
                                    render_expr(&value.node), a(vt), ft.name()));
                            }
                        }
                    }
                    return Ty::Record(type_name.clone());
                }
                match self.p.variant_of.get(type_name) {
                    Some(t) if !t.is_empty() => {
                        if self.p.records.contains_key(t) { Ty::Record(t.clone()) } else { Ty::Variant(t.clone()) }
                    }
                    // an ad-hoc record (`Game { bet: 10 }`): fields are what it was built with
                    _ => Ty::Unknown,
                }
            }
            Expr::Try(inner) => {
                self.in_try += 1;
                let t = self.infer(inner);
                self.in_try -= 1;
                Ty::Try(Box::new(t))
            }
            Expr::TryPropagate(inner) => { self.infer(inner); Ty::Unknown }
            Expr::Pipe { left, right } => {
                self.infer(left);
                // the right side receives the left as its first argument:
                // only its own arguments are expressions here
                match &right.node {
                    Expr::FnCall { args, .. } | Expr::MethodCall { args, .. } => for a in args { self.infer(a); },
                    _ => {}
                }
                Ty::Unknown
            }
            Expr::Match { subject, arms } => {
                self.infer(subject);
                let mut result: Option<Ty> = None;
                for arm in arms {
                    let t = self.scoped(|cx| {
                        cx.bind_pattern(&arm.pattern);
                        if let Some(g) = &arm.guard { cx.infer(g); }
                        cx.block(&arm.body);
                        cx.infer(&arm.result)
                    });
                    result = Some(match result { Some(prev) => join(&prev, &t), None => t });
                }
                result.unwrap_or(Ty::Unknown)
            }
            Expr::Lambda { param, body } => {
                self.scoped(|cx| { cx.bind(param, Ty::Unknown); cx.infer(body); });
                Ty::Fn
            }
            Expr::LambdaBlock { param, stmts, result } => {
                self.scoped(|cx| { cx.bind(param, Ty::Unknown); cx.block(stmts); cx.infer(result); });
                Ty::Fn
            }
            Expr::ListLiteral(items) => {
                let mut elem: Option<Ty> = None;
                for it in items {
                    let t = self.infer(it);
                    elem = Some(match elem { Some(prev) => join(&prev, &t), None => t });
                }
                Ty::List(Box::new(elem.unwrap_or(Ty::Unknown)))
            }
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                self.condition(condition, "if");
                let mut then_present = Vec::new();
                present_when_true(&condition.node, &mut then_present);
                let mut else_present = Vec::new();
                present_when_false(&condition.node, &mut else_present);
                let a = self.scoped(|cx| { cx.narrow(&then_present); cx.block(then_body); cx.infer(then_result) });
                let b = self.scoped(|cx| { cx.narrow(&else_present); cx.block(else_body); cx.infer(else_result) });
                join(&a, &b)
            }
        }
    }

    /// Names a match pattern binds: their types are the subject's parts,
    /// which this pass does not track.
    fn bind_pattern(&mut self, p: &MatchPattern) {
        match p {
            MatchPattern::Variable(n) => self.bind(n, Ty::Unknown),
            MatchPattern::Or(ps) => for q in ps { self.bind_pattern(q); },
            MatchPattern::MapDestructure(fs) => for (_, q) in fs { self.bind_pattern(q); },
            MatchPattern::StringPrefix { rest, .. } => self.bind(rest, Ty::Str),
            MatchPattern::Variant { fields, .. } => match fields {
                VariantPatternFields::Unit => {}
                VariantPatternFields::Tuple(ps) => for q in ps { self.bind_pattern(q); },
                VariantPatternFields::Struct { fields, .. } => for (_, q) in fields { self.bind_pattern(q); },
            },
            MatchPattern::Literal(_) | MatchPattern::Wildcard | MatchPattern::Range { .. } => {}
        }
    }
}
