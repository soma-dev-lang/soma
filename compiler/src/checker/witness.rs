//! A `✗` on a contract is an input that breaks it.
//!
//! The prover says "proven" or "not proven". What it cannot prove it used
//! to call a failure whenever the returned expression, read alone, could
//! never satisfy the clause — on a `return` no input reaches, that failed a
//! correct program (12 of 1,510 random ones after every dead path the
//! prover knows; any number of them on arithmetic it does not do). And a
//! clause it could not decide was a `⚠` even when the first input tried
//! breaks it.
//!
//! So the handler is RUN: on the inputs its text suggests — every Int
//! between the smallest and the largest constant it and its face write
//! when they are close, those constants and their neighbours otherwise —
//! and each `ensures` is evaluated on what it returned, as the runtime
//! does. An input that breaks the clause is the `✗`, shown. No such input
//! among those tried: the clause is not proven and is checked at run time,
//! which is all that is known.
//!
//! Only a handler that cannot touch the world is run: no slot, no machine,
//! no effect, no clock, nothing but arithmetic, text and collections — in
//! itself and in what it calls — on a copy of the program that has no
//! memory at all, with a bound on the statements it may execute.

use crate::ast::*;
use crate::interpreter::{Interpreter, Value};
use std::collections::{HashMap, HashSet};

/// Statements one call may execute.
const FUEL: u64 = 200_000;
/// Calls made for one handler.
const INPUTS: usize = 4096;

pub struct Found {
    /// `f(n = 7, k = 2)`
    pub call: String,
    /// what it returned
    pub returned: String,
}

pub enum Search {
    Found(Found),
    /// no input among `tried` (those the `requires` admit and that return) breaks it
    Clean { tried: usize },
    /// the handler is not one that can be run here, and why
    NotRun(String),
}

struct Table {
    interp: Interpreter,
    cell: String,
    signal: String,
    params: Vec<String>,
    /// (arguments, what the call returned)
    rows: Vec<(Vec<Value>, Value)>,
}

/// The searches of one `verify`: a handler is run once, whatever the number of its clauses.
pub struct Runner<'p> {
    program: &'p Program,
    tables: HashMap<(String, String), Result<Table, String>>,
}

impl<'p> Runner<'p> {
    pub fn new(program: &'p Program) -> Self { Runner { program, tables: HashMap::new() } }

    /// An input of `cell.signal` that breaks `clause` (one conjunct of an `ensures`).
    pub fn search(&mut self, cell: &str, signal: &str, clause: &Expr) -> Search {
        let key = (cell.to_string(), signal.to_string());
        if !self.tables.contains_key(&key) {
            let table = build(self.program, cell, signal);
            self.tables.insert(key.clone(), table);
        }
        let table = match self.tables.get_mut(&key).expect("just inserted") {
            Ok(t) => t,
            Err(why) => return Search::NotRun(why.clone()),
        };
        for (args, result) in &table.rows {
            let mut env: HashMap<String, Value> = table.params.iter().cloned().zip(args.iter().cloned()).collect();
            env.insert("result".to_string(), result.clone());
            table.interp.fuel = Some(FUEL);
            // (as the runtime reads it: anything but `true` breaks the clause)
            let held = table.interp.eval_expr_with_env(clause, &env, &table.cell, &table.signal);
            if !matches!(held, Ok(Value::Bool(true))) {
                let shown: Vec<String> = table.params.iter().zip(args).map(|(p, a)| format!("{} = {}", p, a)).collect();
                return Search::Found(Found { call: format!("{}({})", table.signal, shown.join(", ")), returned: format!("{}", result) });
            }
        }
        Search::Clean { tried: table.rows.len() }
    }
}

fn handler<'a>(cell: &'a CellDef, name: &str) -> Option<&'a OnSection> {
    cell.sections.iter().find_map(|s| match &s.node { Section::OnSignal(on) if on.signal_name == name => Some(on), _ => None })
}

fn slots(cell: &CellDef) -> HashSet<&str> {
    cell.sections.iter().flat_map(|s| match &s.node {
        Section::Memory(m) => m.slots.iter().map(|sl| sl.node.name.as_str()).collect::<Vec<_>>(),
        _ => Vec::new(),
    }).collect()
}

/// The builtins a run may call: they compute, and nothing else.
fn computes(name: &str) -> bool {
    crate::interpreter::builtins::registry::BUILTINS.iter().any(|b| b.name == name && b.deterministic
        && matches!(b.category, "collection" | "lambda" | "math" | "pipeline" | "string" | "types" | "internal"))
}

/// The handlers `cell.signal` reaches, itself included — or why one of them
/// is not a pure computation.
fn reached(program: &Program, cell: &str, signal: &str) -> Result<HashSet<(String, String)>, String> {
    let cells: HashMap<&str, &CellDef> = program.cells.iter().map(|c| (c.node.name.as_str(), &c.node)).collect();
    let stalled = super::termination::may_not_terminate(program);
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut todo = vec![(cell.to_string(), signal.to_string())];
    while let Some((c, h)) = todo.pop() {
        if !seen.insert((c.clone(), h.clone())) { continue; }
        let def = *cells.get(c.as_str()).ok_or_else(|| format!("`{}` is not a cell of the program's top level", c))?;
        let on = handler(def, &h).ok_or_else(|| format!("`{}.{}` has no body here", c, h))?;
        if !on.properties.is_empty() { return Err(format!("`{}` is [{}]", h, on.properties.join(", "))); }
        if stalled.contains(&format!("{}.{}", c, h)) { return Err(format!("`{}` may not terminate", h)); }
        let own = slots(def);
        let mut why: Option<String> = None;
        let mut calls: Vec<(String, String)> = Vec::new();
        super::literals::for_each_stmt_deep(&on.body, &mut |st| match st {
            Statement::Emit { signal_name, .. } => { why.get_or_insert(format!("`{}` emits `{}`", h, signal_name)); }
            Statement::MethodCall { target, method, .. } => match cells.get(target.as_str()) {
                Some(_) if !own.contains(target.as_str()) => calls.push((target.clone(), method.clone())),
                _ => { why.get_or_insert(format!("`{}` writes `{}`", h, target)); }
            },
            Statement::IndexSet { name, .. } if own.contains(name.split('.').next().unwrap_or(name)) => { why.get_or_insert(format!("`{}` writes the slot `{}`", h, name)); }
            _ => {}
        });
        super::literals::for_each_expr(&on.body, &mut |e| match e {
            Expr::Ident(n) if own.contains(n.as_str()) => { why.get_or_insert(format!("`{}` reads the slot `{}`", h, n)); }
            // (a string's `{…}` may call anything)
            Expr::Literal(Literal::String(t)) if !super::desugar::segments(t).is_empty() => { why.get_or_insert(format!("`{}` builds a string from `{{…}}`", h)); }
            Expr::FnCall { name, .. } => {
                if handler(def, name).is_some() { calls.push((c.clone(), name.clone())); }
                else if cells.values().any(|x| handler(x, name).is_some()) { why.get_or_insert(format!("`{}` calls `{}` of another cell by its bare name", h, name)); }
                else if !computes(name) && crate::checker::names::builtin_names().contains(name.as_str()) { why.get_or_insert(format!("`{}` calls {}()", h, name)); }
                else if !computes(name) && name.starts_with('_') { why.get_or_insert(format!("`{}` calls {}()", h, name)); }
            }
            Expr::MethodCall { target, method, .. } => {
                if let Expr::Ident(t) = &target.node {
                    if own.contains(t.as_str()) { why.get_or_insert(format!("`{}` reads the slot `{}`", h, t)); }
                    else if cells.contains_key(t.as_str()) { calls.push((t.clone(), method.clone())); return; }
                }
                if cells.values().any(|x| handler(x, method).is_some()) { why.get_or_insert(format!("`{}` may call the handler `{}` as a method", h, method)); }
                else if crate::checker::names::builtin_names().contains(method.as_str()) && !computes(method) { why.get_or_insert(format!("`{}` calls .{}()", h, method)); }
            }
            Expr::Pipe { .. } => { why.get_or_insert(format!("`{}` uses a pipe", h)); }
            _ => {}
        });
        if let Some(w) = why { return Err(w); }
        todo.extend(calls);
    }
    Ok(seen)
}

/// The Int constants written in `e`.
fn constants(e: &Expr, out: &mut Vec<i64>) {
    super::literals::for_each_in_expr(e, &mut |x| if let Expr::Literal(Literal::Int(n)) = x { out.push(*n); });
}

fn build(program: &Program, cell: &str, signal: &str) -> Result<Table, String> {
    let reach = reached(program, cell, signal)?;
    let def = &program.cells.iter().find(|c| c.node.name == cell).ok_or("not a top-level cell")?.node;
    let on = handler(def, signal).ok_or("no body")?;
    let sig = def.sections.iter().find_map(|s| match &s.node {
        Section::Face(f) => f.declarations.iter().find_map(|d| match &d.node { FaceDecl::Signal(x) if x.name == signal => Some(x), _ => None }),
        _ => None,
    }).ok_or("no signal")?;
    if on.params.len() != sig.params.len() { return Err("its parameters are not those of its signal".to_string()); }

    // the inputs: per parameter
    let mut seen: Vec<i64> = vec![-2, -1, 0, 1, 2];
    for st in &on.body { super::literals::for_each_expr(std::slice::from_ref(st), &mut |x| if let Expr::Literal(Literal::Int(n)) = x { seen.push(*n); }); }
    for e in sig.requires.iter().chain(sig.ensures.iter()) { constants(&e.node, &mut seen); }
    // (`0 - 7` is how a negative bound is often written: each constant, and its opposite)
    let opposite: Vec<i64> = seen.iter().map(|n| n.wrapping_neg()).collect();
    seen.extend(opposite);
    seen.retain(|n| n.abs() < 1 << 40);
    let (lo, hi) = (*seen.iter().min().unwrap_or(&0), *seen.iter().max().unwrap_or(&0));
    let mut ints: Vec<i64> = if hi - lo <= 40 { (lo - 1..=hi + 1).collect() } else { seen.iter().flat_map(|n| [n - 1, *n, n + 1]).collect() };
    ints.sort();
    ints.dedup();
    let mut domains: Vec<Vec<Value>> = Vec::new();
    for p in &on.params {
        match &p.ty.node {
            TypeExpr::Simple(t) if t == "Int" => domains.push(ints.iter().map(|n| Value::Int(crate::interpreter::SomaInt::from_i64(*n))).collect()),
            TypeExpr::Simple(t) if t == "Bool" => domains.push(vec![Value::Bool(false), Value::Bool(true)]),
            // the same numbers, the halves between them, and what is no number
            TypeExpr::Simple(t) if t == "Float" => {
                let mut fs: Vec<f64> = ints.iter().flat_map(|n| [*n as f64, *n as f64 + 0.5]).collect();
                fs.extend([f64::NAN, f64::INFINITY, f64::NEG_INFINITY]);
                domains.push(fs.into_iter().map(Value::Float).collect());
            }
            other => return Err(format!("a parameter of type {} is not tried", crate::commands::describe::format_type(other))),
        }
    }
    // too many combinations: fewer values per parameter, spread over each domain
    let total = |d: &Vec<Vec<Value>>| d.iter().fold(1usize, |n, x| n.saturating_mul(x.len().max(1)));
    while total(&domains) > INPUTS {
        let widest = domains.iter_mut().max_by_key(|d| d.len()).expect("a parameter");
        if widest.len() <= 2 { break; }
        let keep = widest.len() * 2 / 3;
        let step = widest.len() as f64 / keep as f64;
        *widest = (0..keep).map(|i| widest[((i as f64) * step) as usize].clone()).collect();
    }
    if total(&domains) > INPUTS { return Err("too many parameters to try".to_string()); }

    // the program the run sees: the cells it reaches, without their memory,
    // machines and timers; the target's `ensures` is read here, not there
    let touched: HashSet<&str> = reach.iter().map(|(c, _)| c.as_str()).collect();
    let mut pure = program.clone();
    pure.cells.retain(|c| !matches!(c.node.kind, CellKind::Cell | CellKind::Agent | CellKind::Test) || touched.contains(c.node.name.as_str()));
    for c in &mut pure.cells {
        if !matches!(c.node.kind, CellKind::Cell | CellKind::Agent) { continue; }
        let name = c.node.name.clone();
        c.node.sections.retain(|s| match &s.node {
            Section::Face(_) => true,
            Section::OnSignal(on) => reach.contains(&(name.clone(), on.signal_name.clone())),
            _ => false,
        });
        for s in &mut c.node.sections {
            if let Section::Face(f) = &mut s.node {
                f.declarations.retain(|d| matches!(&d.node, FaceDecl::Signal(x) if reach.contains(&(name.clone(), x.name.clone()))));
                for d in &mut f.declarations {
                    if let FaceDecl::Signal(x) = &mut d.node { if name == cell && x.name == signal { x.ensures.clear(); } }
                }
            }
        }
    }
    let mut interp = Interpreter::new(&pure);
    let mut rows: Vec<(Vec<Value>, Value)> = Vec::new();
    let mut at = vec![0usize; domains.len()];
    loop {
        let args: Vec<Value> = at.iter().zip(&domains).map(|(i, d)| d[*i].clone()).collect();
        interp.fuel = Some(FUEL);
        if let Ok(v) = interp.call_signal(cell, signal, args.clone()) { rows.push((args, v)); }
        // the next combination (the last parameter turns fastest)
        let mut i = domains.len();
        loop {
            if i == 0 { 
                interp.fuel = None;
                return Ok(Table { interp, cell: cell.to_string(), signal: signal.to_string(), params: on.params.iter().map(|p| p.name.clone()).collect(), rows });
            }
            i -= 1;
            at[i] += 1;
            if at[i] < domains[i].len() { break; }
            at[i] = 0;
        }
    }
}
