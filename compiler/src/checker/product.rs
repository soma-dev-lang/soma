//! 3.0 (B2): model checking over the PRODUCT of the program's machines.
//!
//!     [verify.before."Pay.paid"]
//!     requires = ["Approval.signed_off"]
//!
//! A property that names states of two cells cannot be read off either
//! machine: it is a fact about what the handlers do. Here one instance id is
//! followed through every machine at once. A product state is the state of
//! that id in each machine, plus which of the required states it has been
//! in. A step is one handler invocation — atomic: it applies every
//! `transition()` it makes, through the handlers it calls in any cell, the
//! listeners of what it emits and the tools its model may call, or it fails
//! and nothing moves. The search runs every entry point from every reachable
//! product state.
//!
//! What is abstracted, always towards MORE behaviours (so a `✓` holds for
//! the program, and a `✗` may name a path the data would never take):
//! - a condition is taken both ways, unless it compares `get_status(id)`
//!   with a state;
//! - an id is the followed one or another one, both tried — the same
//!   variable is the same id throughout one invocation;
//! - a guard on an edge may hold or not (unless `assuming` refutes it);
//! - a `[task]` handler commits step by step and other invocations run in
//!   between: each of its transitions is a step of its own;
//! - a loop body, a lambda, a tool runs any number of times.
//!
//! The search is exhaustive or it is refused: past `LIMIT` product states
//! the property fails with the count, it is not truncated.

use crate::ast::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

/// The published bound on explored product states.
pub const LIMIT: usize = 200_000;

struct Machine {
    cell: String,
    name: String,
    states: Vec<String>,
    initial: usize,
    /// (from, to) → the edge's effect statements
    edges: HashMap<(usize, usize), Vec<Spanned<Statement>>>,
}

/// `cur[m]` for each machine, and the required states seen so far (a bit each).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
struct Abs { cur: Vec<u8>, seen: u64 }

/// A state in the middle of a handler: the id choices made so far.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Exec { st: Abs, bind: BTreeMap<String, bool> }

type Set = BTreeSet<Exec>;

struct Property {
    label: String,
    target: (usize, usize),
    /// bits of `seen`: one of them (any) or each of them (all)
    needs: Vec<u32>,
    all: bool,
}

struct Frame<'a> {
    cell: &'a CellDef,
    /// variable → the key of the id it holds
    env: HashMap<String, String>,
    uid: String,
    depth: usize,
}

/// What a block gives: states that go on, states that returned, states that left a loop.
#[derive(Default)]
struct Flow { normal: Set, returned: Set, broke: Set }

struct Engine<'a> {
    cells: Vec<&'a CellDef>,
    machines: Vec<Machine>,
    /// (machine, state) → bit of `seen`
    watched: HashMap<(usize, usize), u32>,
    props: Vec<Property>,
    /// property index → the entry that broke it (filled once)
    broken: BTreeMap<usize, String>,
    unsupported: Option<String>,
    /// states that left the handler (`return`) or a loop (`break`) from
    /// inside an expression — a `match` arm, an `if` expression: collected
    /// by the statement that holds the expression
    returned: Set,
    broke: Set,
}

fn key_text(s: &str) -> String { format!("lit:{}", s) }

impl<'a> Engine<'a> {
    fn cell(&self, name: &str) -> Option<&'a CellDef> { self.cells.iter().copied().find(|c| c.name == name) }

    fn handler(&self, cell: &'a CellDef, name: &str) -> Option<&'a OnSection> {
        cell.sections.iter().find_map(|s| match &s.node { Section::OnSignal(on) if on.signal_name == name => Some(on), _ => None })
    }

    /// The machine `transition()` / `get_status()` reach from `cell`: its
    /// own, or the program's only one.
    fn machine_of(&self, cell: &CellDef) -> Option<usize> {
        self.machines.iter().position(|m| m.cell == cell.name)
            .or_else(|| (self.machines.len() == 1).then_some(0))
    }

    fn key_of(&self, e: &Expr, at: Span, frame: &Frame) -> String {
        match e {
            Expr::Ident(v) => frame.env.get(v).cloned().unwrap_or_else(|| format!("g:{}:{}", frame.cell.name, v)),
            Expr::Literal(Literal::String(s)) => key_text(s),
            Expr::Literal(Literal::Int(n)) => key_text(&n.to_string()),
            // a computed id: its own value at each place it is written
            _ => format!("x:{}:{}", frame.uid, at.start),
        }
    }

    /// Split `states` on whether `key` is the followed id.
    fn split(&self, states: &Set, key: &str) -> (Set, Set) {
        let (mut yes, mut no) = (Set::new(), Set::new());
        for s in states {
            match s.bind.get(key) {
                Some(true) => { yes.insert(s.clone()); }
                Some(false) => { no.insert(s.clone()); }
                None => {
                    let mut a = s.clone(); a.bind.insert(key.to_string(), true); yes.insert(a);
                    let mut b = s.clone(); b.bind.insert(key.to_string(), false); no.insert(b);
                }
            }
        }
        (yes, no)
    }

    /// `transition(id, target)` from `frame`'s cell.
    fn transition(&mut self, states: Set, id: &Spanned<Expr>, source: Option<&Expr>, target: &Expr, frame: &Frame<'a>, entry: &str) -> Set {
        let Some(m) = self.machine_of(frame.cell) else { return states };
        let key = self.key_of(&id.node, id.span, frame);
        let (hit, mut out) = self.split(&states, &key);
        let named = |e: &Expr| match e {
            Expr::Literal(Literal::String(s)) => Some(s.clone()),
            Expr::Ident(v) if !frame.env.contains_key(v) => Some(v.clone()),
            _ => None,
        };
        let to: Option<usize> = named(target).and_then(|t| self.machines[m].states.iter().position(|s| s.eq_ignore_ascii_case(&t)));
        let from: Option<usize> = source.and_then(named).and_then(|t| self.machines[m].states.iter().position(|s| *s == t));
        // a target that is not a literal: any edge out of the current state
        let targets: Vec<usize> = match (to, named(target)) {
            (Some(t), _) => vec![t],
            (None, Some(_)) => vec![],                       // a literal no state has: the call fails
            (None, None) => (0..self.machines[m].states.len()).collect(),
        };
        for s in hit {
            let cur = s.st.cur[m] as usize;
            if from.is_some_and(|f| f != cur) { continue; }
            for &t in &targets {
                let Some(effect) = self.machines[m].edges.get(&(cur, t)).cloned() else { continue };
                let mut next = s.clone();
                next.st.cur[m] = t as u8;
                if let Some(bit) = self.watched.get(&(m, t)) { next.st.seen |= 1 << bit; }
                for (i, p) in self.props.iter().enumerate() {
                    if p.target != (m, t) || self.broken.contains_key(&i) { continue; }
                    let met = if p.all { p.needs.iter().all(|b| next.st.seen & (1 << b) != 0) } else { p.needs.iter().any(|b| next.st.seen & (1 << b) != 0) };
                    if !met { self.broken.insert(i, entry.to_string()); }
                }
                // the edge's effect runs with the transition
                let mut after = Set::new();
                after.insert(next);
                let flow = self.block(&effect, frame, after, entry);
                out.extend(flow.normal); out.extend(flow.returned); out.extend(flow.broke);
            }
        }
        // (a computed id is read once: its choice is not kept)
        if key.starts_with("x:") { out = out.into_iter().map(|mut s| { s.bind.remove(&key); s }).collect(); }
        out
    }

    fn call(&mut self, cell: &'a CellDef, on: &'a OnSection, args: &[Spanned<Expr>], frame: &Frame<'a>, at: Span, states: Set, entry: &str) -> Set {
        if frame.depth >= 12 {
            self.unsupported.get_or_insert(format!("handler calls nest deeper than 12 (through {}.{}) — recursion is not explored", cell.name, on.signal_name));
            return states;
        }
        let uid = format!("{}/{}", frame.uid, at.start);
        let mut env = HashMap::new();
        for (i, p) in on.params.iter().enumerate() {
            let key = match args.get(i) {
                Some(a) => self.key_of(&a.node, a.span, frame),
                None => format!("x:{}:p{}", uid, i),
            };
            env.insert(p.name.clone(), key);
        }
        let inner = Frame { cell, env, uid: uid.clone(), depth: frame.depth + 1 };
        let flow = self.block(&on.body, &inner, states, entry);
        let mut out = flow.normal;
        out.extend(flow.returned);
        out.extend(flow.broke);
        // the callee's own choices end with it
        let local = format!(":{}", uid);
        out.into_iter().map(|mut s| { s.bind.retain(|k, _| !(k.starts_with("l:") || k.starts_with("x:")) || !k.contains(&local)); s }).collect()
    }

    /// Any number of runs of `step`, from `states`.
    fn repeat(&mut self, states: Set, mut step: impl FnMut(&mut Self, Set) -> Set) -> Set {
        let mut all = states.clone();
        let mut frontier = states;
        while !frontier.is_empty() {
            let next = step(self, frontier);
            frontier = next.into_iter().filter(|s| all.insert(s.clone())).collect();
        }
        all
    }

    fn exprs(&mut self, es: &[&Spanned<Expr>], frame: &Frame<'a>, mut states: Set, entry: &str) -> Set {
        for e in es { states = self.expr(e, frame, states, entry); }
        states
    }

    /// The effect of evaluating `e` on the machines.
    fn expr(&mut self, e: &Spanned<Expr>, frame: &Frame<'a>, states: Set, entry: &str) -> Set {
        if states.is_empty() { return states; }
        match &e.node {
            Expr::Literal(_) | Expr::Ident(_) => states,
            Expr::FieldAccess { target, .. } | Expr::Not(target) | Expr::TryPropagate(target) => self.expr(target, frame, states, entry),
            Expr::Index { target, index } => self.exprs(&[target, index], frame, states, entry),
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => self.exprs(&[left, right], frame, states, entry),
            Expr::Record { fields, .. } => { let mut s = states; for (_, f) in fields { s = self.expr(f, frame, s, entry); } s }
            Expr::ListLiteral(items) => { let mut s = states; for i in items { s = self.expr(i, frame, s, entry); } s }
            // a failed `try` undoes what it did: the state it started from
            Expr::Try(inner) => { let mut out = self.expr(inner, frame, states.clone(), entry); out.extend(states); out }
            Expr::Pipe { left, right } => {
                let s = self.expr(left, frame, states, entry);
                self.expr(right, frame, s, entry)
            }
            // a lambda's body runs as often as its caller likes
            Expr::Lambda { body, .. } => self.repeat(states, |me, s| me.expr(body, frame, s, entry)),
            Expr::LambdaBlock { stmts, result, .. } => self.repeat(states, |me, s| {
                let flow = me.block(stmts, frame, s, entry);
                let mut out = me.expr(result, frame, flow.normal, entry);
                out.extend(flow.returned); out.extend(flow.broke);
                out
            }),
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                let s = self.expr(condition, frame, states, entry);
                let (yes, no) = self.branch(&condition.node, frame, s);
                let a = self.block(then_body, frame, yes, entry);
                let mut out = self.expr(then_result, frame, a.normal, entry);
                let b = self.block(else_body, frame, no, entry);
                out.extend(self.expr(else_result, frame, b.normal, entry));
                // a `return` in a branch ends the handler there
                self.returned.extend(a.returned); self.returned.extend(b.returned);
                self.broke.extend(a.broke); self.broke.extend(b.broke);
                out
            }
            Expr::Match { subject, arms } => {
                let s = self.expr(subject, frame, states, entry);
                let mut out = Set::new();
                for arm in arms {
                    let mut t = s.clone();
                    if let Some(g) = &arm.guard { t = self.expr(g, frame, t, entry); }
                    let flow = self.block(&arm.body, frame, t, entry);
                    out.extend(self.expr(&arm.result, frame, flow.normal, entry));
                    self.returned.extend(flow.returned); self.broke.extend(flow.broke);
                }
                out
            }
            Expr::MethodCall { target, method, args } => {
                // `Cell.handler(args)`
                if let Expr::Ident(c) = &target.node {
                    if !frame.env.contains_key(c) {
                        if let Some(cell) = self.cell(c) {
                            let mut s = states;
                            for a in args { s = self.expr(a, frame, s, entry); }
                            return match self.handler(cell, method) {
                                Some(on) => self.call(cell, on, args, frame, e.span, s, entry),
                                None => s,
                            };
                        }
                    }
                }
                let mut s = self.expr(target, frame, states, entry);
                for a in args { s = self.expr(a, frame, s, entry); }
                // `id.transition("paid")`
                if method == "transition" && !args.is_empty() {
                    let (source, to) = if args.len() >= 2 { (Some(&args[0].node), &args[1].node) } else { (None, &args[0].node) };
                    return self.transition(s, target, source, to, frame, entry);
                }
                s
            }
            Expr::FnCall { name, args } => {
                let mut s = states;
                for a in args { s = self.expr(a, frame, s, entry); }
                match name.as_str() {
                    "transition" if args.len() >= 2 => {
                        let (source, to) = if args.len() >= 3 { (Some(&args[1].node), &args[2].node) } else { (None, &args[1].node) };
                        return self.transition(s, &args[0], source, to, frame, entry);
                    }
                    // the model may call any tool of the cell, any number of times, with any arguments
                    "think" | "think_json" | "vote" => {
                        let tools: Vec<&'a OnSection> = frame.cell.sections.iter().flat_map(|sec| match &sec.node {
                            Section::Face(f) => f.declarations.iter().filter_map(|d| match &d.node { FaceDecl::Tool(t) => self.handler(frame.cell, &t.name), _ => None }).collect::<Vec<_>>(),
                            _ => Vec::new(),
                        }).collect();
                        if tools.is_empty() { return s; }
                        let cell = frame.cell;
                        return self.repeat(s, |me, st| {
                            let mut out = Set::new();
                            for on in &tools { out.extend(me.call(cell, on, &[], frame, e.span, st.clone(), entry)); }
                            out
                        });
                    }
                    "delegate" => {
                        return match (args.first().map(|a| &a.node), args.get(1).map(|a| &a.node)) {
                            (Some(Expr::Literal(Literal::String(c))), Some(Expr::Literal(Literal::String(h)))) => {
                                match self.cell(c).and_then(|cell| self.handler(cell, h).map(|on| (cell, on))) {
                                    Some((cell, on)) => self.call(cell, on, &args[2..], frame, e.span, s, entry),
                                    None => s,
                                }
                            }
                            _ => { self.unsupported.get_or_insert("delegate() with a computed cell or handler: what it runs is not known".to_string()); s }
                        };
                    }
                    _ => {}
                }
                // a handler: this cell's own first, then the program's
                let own = self.handler(frame.cell, name).filter(|on| crate::ast::accepted_arities(&on.params).contains(&args.len()));
                if let Some(on) = own { return self.call(frame.cell, on, args, frame, e.span, s, entry); }
                if !super::names::builtin_names().contains(name.as_str()) {
                    let found = self.cells.iter().copied().find_map(|c| self.handler(c, name).map(|on| (c, on)));
                    if let Some((cell, on)) = found { return self.call(cell, on, args, frame, e.span, s, entry); }
                }
                s
            }
        }
    }

    /// The states where `cond` holds and those where it does not. Read
    /// exactly for `get_status(id) == "state"` (and `!=`, `!`, `&&`, `||`
    /// of such tests), also through a handler whose body is one `return`
    /// of such a test (`Approval.approved(id)`); anything else may go
    /// either way.
    fn branch(&self, cond: &Expr, frame: &Frame<'a>, states: Set) -> (Set, Set) {
        match cond {
            Expr::Not(inner) => { let (yes, no) = self.branch(&inner.node, frame, states); (no, yes) }
            Expr::BinaryOp { left, op: BinOp::And, right } => {
                let (ya, na) = self.branch(&left.node, frame, states);
                let (yb, mut nb) = self.branch(&right.node, frame, ya);
                nb.extend(na);
                (yb, nb)
            }
            Expr::BinaryOp { left, op: BinOp::Or, right } => {
                let (mut ya, na) = self.branch(&left.node, frame, states);
                let (yb, nb) = self.branch(&right.node, frame, na);
                ya.extend(yb);
                (ya, nb)
            }
            // `x == true` (what `require x else …` is), `x != true`, `x == false`
            Expr::CmpOp { left, op: op @ (CmpOp::Eq | CmpOp::Ne), right } if matches!(right.node, Expr::Literal(Literal::Bool(_))) => {
                let Expr::Literal(Literal::Bool(b)) = right.node else { unreachable!() };
                let (yes, no) = self.branch(&left.node, frame, states);
                if b == (*op == CmpOp::Eq) { (yes, no) } else { (no, yes) }
            }
            Expr::CmpOp { left, op: op @ (CmpOp::Eq | CmpOp::Ne), right } => {
                let (call, lit) = match (&left.node, &right.node) {
                    (Expr::FnCall { name, args }, Expr::Literal(Literal::String(s))) if name == "get_status" && args.len() == 1 => (&args[0], s),
                    (Expr::Literal(Literal::String(s)), Expr::FnCall { name, args }) if name == "get_status" && args.len() == 1 => (&args[0], s),
                    _ => return (states.clone(), states),
                };
                let Some(m) = self.machine_of(frame.cell) else { return (states.clone(), states) };
                let Some(state) = self.machines[m].states.iter().position(|x| x == lit) else { return (states.clone(), states) };
                let key = self.key_of(&call.node, call.span, frame);
                let (hit, other) = self.split(&states, &key);
                let (mut yes, mut no) = (other.clone(), other);
                for s in hit {
                    if (s.st.cur[m] as usize == state) == (*op == CmpOp::Eq) { yes.insert(s); } else { no.insert(s); }
                }
                (yes, no)
            }
            // a predicate handler: `on approved(id) { return get_status(id) == "signed_off" }`
            Expr::FnCall { name, args } | Expr::MethodCall { method: name, args, .. } if frame.depth < 12 => {
                let callee: Option<(&'a CellDef, &'a OnSection)> = match cond {
                    Expr::MethodCall { target, .. } => match &target.node {
                        Expr::Ident(c) if !frame.env.contains_key(c) => self.cell(c).and_then(|cell| self.handler(cell, name).map(|on| (cell, on))),
                        _ => None,
                    },
                    _ => self.handler(frame.cell, name).map(|on| (frame.cell, on))
                        .or_else(|| if super::names::builtin_names().contains(name.as_str()) { None } else {
                            self.cells.iter().copied().find_map(|c| self.handler(c, name).map(|on| (c, on))) }),
                };
                let Some((cell, on)) = callee else { return (states.clone(), states) };
                let [only] = on.body.as_slice() else { return (states.clone(), states) };
                let Statement::Return { value } = &only.node else { return (states.clone(), states) };
                let mut env = HashMap::new();
                for (i, p) in on.params.iter().enumerate() {
                    if let Some(a) = args.get(i) { env.insert(p.name.clone(), self.key_of(&a.node, a.span, frame)); }
                }
                let inner = Frame { cell, env, uid: format!("{}/b{}", frame.uid, only.span.start), depth: frame.depth + 1 };
                self.branch(&value.node, &inner, states)
            }
            _ => (states.clone(), states),
        }
    }

    fn constraint(&mut self, c: &Constraint, frame: &Frame<'a>, states: Set, entry: &str) -> Set {
        match c {
            Constraint::Comparison { left, op, right } => {
                let s = self.exprs(&[left, right], frame, states, entry);
                // `require get_status(id) == "x" else …`: only those states go on
                let cond = Expr::CmpOp { left: Box::new(left.clone()), op: *op, right: Box::new(right.clone()) };
                self.branch(&cond, frame, s).0
            }
            Constraint::Predicate { args, .. } => { let mut s = states; for a in args { s = self.expr(a, frame, s, entry); } s }
            Constraint::And(a, b) => { let s = self.constraint(&a.node, frame, states, entry); self.constraint(&b.node, frame, s, entry) }
            Constraint::Or(a, b) => { let s = self.constraint(&a.node, frame, states.clone(), entry); let mut out = self.constraint(&b.node, frame, states, entry); out.extend(s); out }
            Constraint::Not(a) => {
                // the calls inside run; which states pass is not narrowed
                let before = states.clone();
                let mut out = self.constraint(&a.node, frame, states, entry);
                out.extend(before);
                out
            }
            Constraint::Descriptive(_) => states,
        }
    }

    fn block(&mut self, stmts: &[Spanned<Statement>], frame: &Frame<'a>, states: Set, entry: &str) -> Flow {
        let mut flow = Flow { normal: states, ..Default::default() };
        // (a block's own `let`s: tracked in a copy of the frame)
        let mut local = Frame { cell: frame.cell, env: frame.env.clone(), uid: frame.uid.clone(), depth: frame.depth };
        for st in stmts {
            if flow.normal.is_empty() { break; }
            let cur = std::mem::take(&mut flow.normal);
            match &st.node {
                Statement::Let { name, value } | Statement::Assign { name, value } => {
                    let mut s = self.expr(value, &local, cur, entry);
                    let key = match &value.node {
                        Expr::Ident(v) if local.env.contains_key(v) => local.env[v].clone(),
                        Expr::Literal(Literal::String(_) | Literal::Int(_)) => self.key_of(&value.node, value.span, &local),
                        _ => {
                            // a new value: the choice made for the old one ends here
                            let key = format!("l:{}:{}:{}", name, local.uid, st.span.start);
                            s = s.into_iter().map(|mut x| { x.bind.remove(&key); x }).collect();
                            key
                        }
                    };
                    local.env.insert(name.clone(), key);
                    flow.normal = s;
                }
                Statement::Return { value } => { flow.returned.extend(self.expr(value, &local, cur, entry)); }
                Statement::Ensure { condition } => { flow.normal = self.expr(condition, &local, cur, entry); }
                Statement::If { condition, then_body, else_body } => {
                    let s = self.expr(condition, &local, cur, entry);
                    let (yes, no) = self.branch(&condition.node, &local, s);
                    let a = self.block(then_body, &local, yes, entry);
                    let b = self.block(else_body, &local, no, entry);
                    flow.normal.extend(a.normal); flow.normal.extend(b.normal);
                    flow.returned.extend(a.returned); flow.returned.extend(b.returned);
                    flow.broke.extend(a.broke); flow.broke.extend(b.broke);
                }
                Statement::For { var, iter, body, .. } => {
                    let s = self.expr(iter, &local, cur, entry);
                    let mut inner = Frame { cell: local.cell, env: local.env.clone(), uid: local.uid.clone(), depth: local.depth };
                    inner.env.insert(var.clone(), format!("l:{}:{}:{}", var, local.uid, st.span.start));
                    let (mut returned, mut broke) = (Set::new(), Set::new());
                    let all = self.repeat(s, |me, x| {
                        let f = me.block(body, &inner, x, entry);
                        returned.extend(f.returned); broke.extend(f.broke);
                        f.normal
                    });
                    flow.normal = all; flow.normal.extend(broke);
                    flow.returned.extend(returned);
                }
                Statement::While { condition, body, .. } => {
                    let (mut returned, mut broke) = (Set::new(), Set::new());
                    let all = self.repeat(cur, |me, x| {
                        let x = me.expr(condition, &local, x, entry);
                        let f = me.block(body, &local, x, entry);
                        returned.extend(f.returned); broke.extend(f.broke);
                        f.normal
                    });
                    flow.normal = self.expr(condition, &local, all, entry); flow.normal.extend(broke);
                    flow.returned.extend(returned);
                }
                Statement::Break | Statement::Continue => { flow.broke.extend(cur); }
                Statement::ExprStmt { expr } => { flow.normal = self.expr(expr, &local, cur, entry); }
                Statement::IndexSet { index, value, .. } => { flow.normal = self.exprs(&[index, value], &local, cur, entry); }
                Statement::Require { constraint, .. } => { flow.normal = self.constraint(&constraint.node, &local, cur, entry); }
                Statement::MethodCall { target, method, args } => {
                    let mut s = cur;
                    for a in args { s = self.expr(a, &local, s, entry); }
                    flow.normal = match self.cell(target).filter(|_| !local.env.contains_key(target)).and_then(|c| self.handler(c, method).map(|on| (c, on))) {
                        Some((cell, on)) => self.call(cell, on, args, &local, st.span, s, entry),
                        None => s,
                    };
                }
                // every cell that handles the event runs, in this invocation
                Statement::Emit { signal_name, args } => {
                    let mut s = cur;
                    for a in args { s = self.expr(a, &local, s, entry); }
                    let listeners: Vec<(&'a CellDef, &'a OnSection)> = self.cells.iter().copied()
                        .filter_map(|c| self.handler(c, signal_name).map(|on| (c, on))).collect();
                    for (cell, on) in listeners { s = self.call(cell, on, args, &local, st.span, s, entry); }
                    flow.normal = s;
                }
            }
            flow.returned.extend(std::mem::take(&mut self.returned));
            flow.broke.extend(std::mem::take(&mut self.broke));
        }
        flow
    }
}

/// `amount > 500 && n <= 3` → per variable, the interval it allows.
fn intervals(e: &Expr, out: &mut HashMap<String, (i64, i64)>) -> bool {
    match e {
        Expr::BinaryOp { left, op: BinOp::And, right } => intervals(&left.node, out) && intervals(&right.node, out),
        Expr::CmpOp { left, op, right } => {
            let (var, op, n) = match (&left.node, &right.node) {
                (Expr::Ident(v), Expr::Literal(Literal::Int(n))) => (v, *op, *n),
                (Expr::Literal(Literal::Int(n)), Expr::Ident(v)) => (v, match op { CmpOp::Lt => CmpOp::Gt, CmpOp::Gt => CmpOp::Lt, CmpOp::Le => CmpOp::Ge, CmpOp::Ge => CmpOp::Le, o => *o }, *n),
                _ => return false,
            };
            let slot = out.entry(var.clone()).or_insert((i64::MIN, i64::MAX));
            match op {
                CmpOp::Lt => slot.1 = slot.1.min(n.saturating_sub(1)),
                CmpOp::Le => slot.1 = slot.1.min(n),
                CmpOp::Gt => slot.0 = slot.0.max(n.saturating_add(1)),
                CmpOp::Ge => slot.0 = slot.0.max(n),
                CmpOp::Eq => { slot.0 = slot.0.max(n); slot.1 = slot.1.min(n); }
                CmpOp::Ne => return false,
            }
            true
        }
        _ => false,
    }
}

/// Can `guard` hold when `assumption` does? `false` only when both are
/// conjunctions of `variable <op> integer` and some variable has no value
/// left — anything else may hold.
pub fn guard_may_hold(guard: &Expr, assumption: &Expr) -> bool {
    let mut both: HashMap<String, (i64, i64)> = HashMap::new();
    if !intervals(assumption, &mut both) { return true; }
    let mut probe = both.clone();
    if !intervals(guard, &mut probe) {
        // a guard this reading does not cover: its integer conjuncts alone
        let mut conjuncts = Vec::new();
        fn split<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
            match e { Expr::BinaryOp { left, op: BinOp::And, right } => { split(&left.node, out); split(&right.node, out); } other => out.push(other) }
        }
        split(guard, &mut conjuncts);
        return !conjuncts.iter().any(|c| { let mut p = both.clone(); intervals(c, &mut p) && p.values().any(|(lo, hi)| lo > hi) });
    }
    !probe.values().any(|(lo, hi)| lo > hi)
}

/// A machine without the edges whose guard cannot hold under `assumption`.
pub fn machine_assuming(sm: &StateMachineSection, assumption: &Expr) -> (StateMachineSection, Vec<String>) {
    let mut out = sm.clone();
    let mut removed = Vec::new();
    out.transitions.retain(|t| match &t.node.guard {
        Some(g) if !guard_may_hold(&g.node, assumption) => { removed.push(format!("{} -> {}", t.node.from, t.node.to)); false }
        _ => true,
    });
    (out, removed)
}

/// `assuming = "amount > 500"` as an expression.
pub fn parse_assumption(text: &str) -> Result<Expr, String> {
    let e = super::desugar::parse_segment(text).ok_or_else(|| format!("`assuming = \"{}\"` is not an expression", text))?;
    let mut probe = HashMap::new();
    if !intervals(&e, &mut probe) {
        return Err(format!("`assuming = \"{}\"`: an assumption compares variables with integers, joined by `&&` (`amount > 500`, `n >= 1 && n <= 9`)", text));
    }
    if probe.values().any(|(lo, hi)| lo > hi) {
        return Err(format!("`assuming = \"{}\"` can never hold", text));
    }
    Ok(e)
}

/// One `[verify.before."Cell.state"]` entry.
pub struct Spec {
    pub target: String,
    pub requires: Vec<String>,
    pub all: bool,
    pub assuming: Option<String>,
}

/// Check the cross-machine properties. Empty when there is none.
pub fn check(program: &Program, specs: &[Spec]) -> Vec<super::verify::VerifyResult> {
    use super::verify::{VerifyCheck, VerifyResult};
    if specs.is_empty() { return Vec::new(); }
    let exposed = super::desugar::expose_for_analysis(program);
    let cells: Vec<&CellDef> = super::names::collect_cells(&exposed).into_iter()
        .filter(|c| matches!(c.kind, CellKind::Cell | CellKind::Agent)).collect();
    let mut result = VerifyResult {
        machine_name: "product".to_string(), states: vec![], initial: String::new(), terminal_states: vec![], transitions: vec![], checks: vec![],
    };
    // properties that share an assumption share a search
    let mut groups: Vec<(Option<String>, Vec<&Spec>)> = Vec::new();
    for s in specs {
        match groups.iter_mut().find(|g| g.0 == s.assuming) { Some(g) => g.1.push(s), None => groups.push((s.assuming.clone(), vec![s])) }
    }
    for (assuming, group) in groups {
        let assumption = match assuming.as_deref().map(parse_assumption) {
            Some(Err(e)) => { for s in &group { result.checks.push(VerifyCheck::Fail(format!("before {}: {}", s.target, e), None)); } continue; }
            Some(Ok(e)) => Some(e),
            None => None,
        };
        let mut machines: Vec<Machine> = Vec::new();
        let mut dropped: Vec<String> = Vec::new();
        for cell in &cells {
            for section in &cell.sections {
                let Section::State(sm) = &section.node else { continue };
                let sm = match &assumption {
                    Some(a) => { let (m, gone) = machine_assuming(sm, a); dropped.extend(gone.into_iter().map(|g| format!("{}: {}", cell.name, g))); m }
                    None => sm.clone(),
                };
                let graph = super::temporal::StateMachineGraph::from_ast(&sm);
                let mut states: Vec<String> = graph.states.iter().cloned().collect();
                states.sort();
                let index = |s: &str| states.iter().position(|x| x == s);
                let mut edges: HashMap<(usize, usize), Vec<Spanned<Statement>>> = HashMap::new();
                for t in &sm.transitions {
                    let Some(to) = index(&t.node.to) else { continue };
                    let froms: Vec<usize> = if t.node.from == "*" {
                        (0..states.len()).filter(|i| *i != to && !t.node.except.contains(&states[*i])).collect()
                    } else { index(&t.node.from).into_iter().collect() };
                    for f in froms { edges.entry((f, to)).or_default().extend(t.node.effect.iter().cloned()); }
                }
                machines.push(Machine { cell: cell.name.clone(), name: sm.name.clone(), initial: index(&sm.initial).unwrap_or(0), states, edges });
            }
        }
        // `Cell.state` → (machine, state)
        let resolve = |name: &str| -> Result<(usize, usize), String> {
            let found: Vec<(usize, usize)> = match name.split_once('.') {
                Some((cell, state)) => machines.iter().enumerate().filter(|(_, m)| m.cell == cell)
                    .filter_map(|(i, m)| m.states.iter().position(|s| s == state).map(|s| (i, s))).collect(),
                None => machines.iter().enumerate().filter_map(|(i, m)| m.states.iter().position(|s| s == name).map(|s| (i, s))).collect(),
            };
            match found.as_slice() {
                [one] => Ok(*one),
                [] => Err(format!("`{}` is not a state of any machine{}", name, if name.contains('.') { "" } else { " (write `Cell.state`)" })),
                _ => Err(format!("`{}` is a state of several machines — write `Cell.state`", name)),
            }
        };
        let mut watched: HashMap<(usize, usize), u32> = HashMap::new();
        let mut props: Vec<Property> = Vec::new();
        for s in &group {
            let label = format!("before {} requires {}{}{}", s.target, if s.all { "each of " } else { "" }, s.requires.join(if s.all { ", " } else { " | " }),
                assuming.as_ref().map(|a| format!(", assuming {}", a)).unwrap_or_default());
            let parts: Result<Vec<(usize, usize)>, String> = s.requires.iter().map(|r| resolve(r)).collect();
            match (resolve(&s.target), parts) {
                (Ok(target), Ok(parts)) => {
                    let needs = parts.into_iter().map(|p| { let n = watched.len() as u32; *watched.entry(p).or_insert(n) }).collect();
                    props.push(Property { label, target, needs, all: s.all });
                }
                (Err(e), _) | (_, Err(e)) => result.checks.push(VerifyCheck::Fail(format!("{}: {}", label, e), None)),
            }
        }
        if props.is_empty() { continue; }
        if machines.len() > 12 || machines.iter().any(|m| m.states.len() > 250) || watched.len() > 60 {
            for p in &props { result.checks.push(VerifyCheck::Fail(format!("{}: not explored — more than 12 machines, 250 states in one machine or 60 required states", p.label), None)); }
            continue;
        }

        // entry points: (label, cell, body, parameters, each transition a step of its own?)
        struct Entry<'e> { label: String, cell: &'e CellDef, body: &'e [Spanned<Statement>], params: &'e [Param], task: bool }
        let mut entries: Vec<Entry> = Vec::new();
        for cell in &cells {
            for (i, section) in cell.sections.iter().enumerate() {
                match &section.node {
                    Section::OnSignal(on) => entries.push(Entry { label: format!("{}.{}", cell.name, on.signal_name), cell, body: &on.body, params: &on.params,
                        task: on.properties.iter().any(|p| p == "task") }),
                    Section::Every(e) | Section::After(e) => entries.push(Entry { label: format!("{} timer #{}", cell.name, i), cell, body: &e.body, params: &[], task: e.task }),
                    _ => {}
                }
            }
        }
        let mut engine = Engine { cells: cells.clone(), machines, watched, props, broken: BTreeMap::new(), unsupported: None, returned: Set::new(), broke: Set::new() };
        let mut start = Abs { cur: engine.machines.iter().map(|m| m.initial as u8).collect(), seen: 0 };
        for (m, mach) in engine.machines.iter().enumerate() {
            if let Some(bit) = engine.watched.get(&(m, mach.initial)) { start.seen |= 1 << bit; }
        }
        let mut seen: HashSet<Abs> = HashSet::new();
        let mut from: HashMap<Abs, (Abs, String)> = HashMap::new();
        let mut queue: VecDeque<Abs> = VecDeque::new();
        seen.insert(start.clone());
        queue.push_back(start.clone());
        let mut refused = false;
        // property → (the state the breaking step started from, the entry)
        let mut witness: BTreeMap<usize, (Abs, String)> = BTreeMap::new();
        'search: while let Some(state) = queue.pop_front() {
            for entry in &entries {
                let mut env = HashMap::new();
                for p in entry.params { env.insert(p.name.clone(), format!("e:{}", p.name)); }
                let frame = Frame { cell: entry.cell, env, uid: String::new(), depth: 0 };
                let mut init = Set::new();
                init.insert(Exec { st: state.clone(), bind: BTreeMap::new() });
                let before: BTreeSet<usize> = engine.broken.keys().copied().collect();
                let flow = engine.block(entry.body, &frame, init, &entry.label);
                let mut ends: Vec<Abs> = flow.normal.into_iter().chain(flow.returned).chain(flow.broke).map(|e| e.st).collect();
                // a `[task]` commits between its steps and others run in
                // between: every state it passed through is one it can stop in
                if entry.task { ends.extend(engine.task_states(entry.body, &frame, &state, &entry.label)); }
                for i in engine.broken.keys() {
                    if !before.contains(i) { witness.insert(*i, (state.clone(), entry.label.clone())); }
                }
                for next in ends {
                    if seen.insert(next.clone()) {
                        from.insert(next.clone(), (state.clone(), entry.label.clone()));
                        queue.push_back(next);
                        if seen.len() > LIMIT { refused = true; break 'search; }
                    }
                }
            }
        }
        let shape = engine.machines.iter().map(|m| format!("{}.{} ({})", m.cell, m.name, m.states.len())).collect::<Vec<_>>().join(" × ");
        let assumed = if dropped.is_empty() { String::new() } else { format!("; edges whose guard cannot hold then, left out: {}", dropped.join(", ")) };
        for (i, p) in engine.props.iter().enumerate() {
            if let Some(why) = &engine.unsupported {
                result.checks.push(VerifyCheck::Fail(format!("{}: not proven — {}", p.label, why), None));
            } else if let Some((at, entry)) = witness.get(&i) {
                // the invocations that lead there, then the one that breaks it
                let mut steps = vec![format!("{}  ← enters {} here", entry, engine.machines[p.target.0].states[p.target.1])];
                let mut cur = at.clone();
                while let Some((prev, by)) = from.get(&cur) { steps.push(by.clone()); cur = prev.clone(); }
                steps.reverse();
                let show = |a: &Abs| a.cur.iter().enumerate().map(|(m, s)| format!("{}={}", engine.machines[m].cell, engine.machines[m].states[*s as usize])).collect::<Vec<_>>().join(", ");
                result.checks.push(VerifyCheck::Fail(
                    format!("{}: a run of the program's handlers enters `{}` without it (from {}){}", p.label, engine.machines[p.target.0].states[p.target.1], show(at), assumed),
                    Some(steps)));
            } else if refused {
                result.checks.push(VerifyCheck::Fail(format!("{}: refused — more than {} product states over {} (the search is exhaustive or it is not a proof)", p.label, LIMIT, shape), None));
            } else {
                result.checks.push(VerifyCheck::Pass(format!("{} — every run of the handlers, {} product state(s) explored over {}{}", p.label, seen.len(), shape, assumed)));
            }
        }
    }
    vec![result]
}

impl<'a> Engine<'a> {
    /// Every state a `[task]` body can stop in: each prefix of its work is
    /// committed where a step ends, and — since other invocations run
    /// between its steps — the rest then runs from whatever state the
    /// instance is in. Over-approximated: each transition the task (or what
    /// it calls) can make, taken alone from `state`.
    fn task_states(&mut self, body: &'a [Spanned<Statement>], frame: &Frame<'a>, state: &Abs, entry: &str) -> Vec<Abs> {
        let mut sites: Vec<(&'a CellDef, &'a Spanned<Expr>)> = Vec::new();
        let mut visited: HashSet<(String, String)> = HashSet::new();
        self.transition_sites(frame.cell, body, &mut sites, &mut visited);
        let mut out = Vec::new();
        for (cell, call) in sites {
            let inner = Frame { cell, env: HashMap::new(), uid: format!("t{}", call.span.start), depth: 1 };
            let mut init = Set::new();
            init.insert(Exec { st: state.clone(), bind: BTreeMap::new() });
            out.extend(self.expr(call, &inner, init, entry).into_iter().map(|e| e.st));
            out.extend(std::mem::take(&mut self.returned).into_iter().map(|e| e.st));
            out.extend(std::mem::take(&mut self.broke).into_iter().map(|e| e.st));
        }
        out
    }

    fn transition_sites(&self, cell: &'a CellDef, body: &'a [Spanned<Statement>], out: &mut Vec<(&'a CellDef, &'a Spanned<Expr>)>, visited: &mut HashSet<(String, String)>) {
        let mut found: Vec<&'a Spanned<Expr>> = Vec::new();
        fn spanned<'e>(stmts: &'e [Spanned<Statement>], out: &mut Vec<&'e Spanned<Expr>>) {
            fn in_expr<'e>(e: &'e Spanned<Expr>, out: &mut Vec<&'e Spanned<Expr>>) {
                out.push(e);
                match &e.node {
                    Expr::FieldAccess { target, .. } | Expr::Not(target) | Expr::TryPropagate(target) | Expr::Try(target) => in_expr(target, out),
                    Expr::Index { target, index } => { in_expr(target, out); in_expr(index, out); }
                    Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => { in_expr(left, out); in_expr(right, out); }
                    Expr::MethodCall { target, args, .. } => { in_expr(target, out); for a in args { in_expr(a, out); } }
                    Expr::FnCall { args, .. } | Expr::ListLiteral(args) => for a in args { in_expr(a, out); },
                    Expr::Record { fields, .. } => for (_, f) in fields { in_expr(f, out); },
                    Expr::Lambda { body, .. } => in_expr(body, out),
                    Expr::LambdaBlock { stmts, result, .. } => { spanned(stmts, out); in_expr(result, out); }
                    Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                        in_expr(condition, out); spanned(then_body, out); in_expr(then_result, out); spanned(else_body, out); in_expr(else_result, out);
                    }
                    Expr::Match { subject, arms } => {
                        in_expr(subject, out);
                        for a in arms { if let Some(g) = &a.guard { in_expr(g, out); } spanned(&a.body, out); in_expr(&a.result, out); }
                    }
                    Expr::Literal(_) | Expr::Ident(_) => {}
                }
            }
            for st in stmts {
                match &st.node {
                    Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } | Statement::Ensure { condition: value } | Statement::ExprStmt { expr: value } => in_expr(value, out),
                    Statement::If { condition, then_body, else_body } => { in_expr(condition, out); spanned(then_body, out); spanned(else_body, out); }
                    Statement::For { iter, body, .. } => { in_expr(iter, out); spanned(body, out); }
                    Statement::While { condition, body, .. } => { in_expr(condition, out); spanned(body, out); }
                    Statement::IndexSet { index, value, .. } => { in_expr(index, out); in_expr(value, out); }
                    Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => for a in args { in_expr(a, out); },
                    Statement::Require { constraint, .. } => {
                        fn in_c<'e>(c: &'e Constraint, out: &mut Vec<&'e Spanned<Expr>>) {
                            match c {
                                Constraint::Comparison { left, right, .. } => { in_expr(left, out); in_expr(right, out); }
                                Constraint::Predicate { args, .. } => for a in args { in_expr(a, out); },
                                Constraint::And(a, b) | Constraint::Or(a, b) => { in_c(&a.node, out); in_c(&b.node, out); }
                                Constraint::Not(a) => in_c(&a.node, out),
                                Constraint::Descriptive(_) => {}
                            }
                        }
                        in_c(&constraint.node, out);
                    }
                    Statement::Break | Statement::Continue => {}
                }
            }
        }
        spanned(body, &mut found);
        // (statement-level `Cell.h(…)` and `emit` reach handlers too)
        let mut callees: Vec<(&'a CellDef, &'a OnSection)> = Vec::new();
        super::literals::for_each_stmt_deep(body, &mut |st| match st {
            Statement::MethodCall { target, method, .. } => if let Some(c) = self.cell(target) { if let Some(on) = self.handler(c, method) { callees.push((c, on)); } },
            Statement::Emit { signal_name, .. } => for c in self.cells.iter().copied() { if let Some(on) = self.handler(c, signal_name) { callees.push((c, on)); } },
            _ => {}
        });
        for e in found {
            match &e.node {
                Expr::FnCall { name, .. } if name == "transition" => out.push((cell, e)),
                Expr::MethodCall { method, target, .. } if method == "transition" && !matches!(&target.node, Expr::Ident(c) if self.cell(c).is_some()) => out.push((cell, e)),
                Expr::FnCall { name, .. } if matches!(name.as_str(), "think" | "think_json" | "vote") => {
                    for sec in &cell.sections {
                        if let Section::Face(f) = &sec.node {
                            for d in &f.declarations { if let FaceDecl::Tool(t) = &d.node { if let Some(on) = self.handler(cell, &t.name) { callees.push((cell, on)); } } }
                        }
                    }
                }
                Expr::FnCall { name, .. } => {
                    if let Some(on) = self.handler(cell, name) { callees.push((cell, on)); }
                    else if !super::names::builtin_names().contains(name.as_str()) {
                        if let Some(found) = self.cells.iter().copied().find_map(|c| self.handler(c, name).map(|on| (c, on))) { callees.push(found); }
                    }
                }
                Expr::MethodCall { target, method, .. } => if let Expr::Ident(c) = &target.node {
                    if let Some(cell2) = self.cell(c) { if let Some(on) = self.handler(cell2, method) { callees.push((cell2, on)); } }
                },
                _ => {}
            }
        }
        for (c, on) in callees {
            if visited.insert((c.name.clone(), on.signal_name.clone())) { self.transition_sites(c, &on.body, out, visited); }
        }
    }
}
