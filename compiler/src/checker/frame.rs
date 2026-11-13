//! Frames — what a handler body can be seen doing (SKELETON_DESIGN.md, phase 1).
//!
//! A *frame* is the footprint of one handler: the memory slots it reads and
//! writes, the handlers and effectful builtins it calls, the signals it
//! emits and the state-machine edges it takes. This pass INFERS the frame
//! of every handler of a program, so that `soma skeleton` can print a
//! program as its skeleton: memory, machines, and a face whose signals each
//! carry their frame — no bodies.
//!
//! ## Bones and helpers
//!
//! A handler is a *helper* when it is not in the face, is called from its
//! own cell and from no other cell: its effects are charged to every bone
//! that reaches it and it does not appear in the skeleton. Every other
//! handler is a *bone*: a face signal or tool, or a handler nothing in the
//! cell calls (an entry point: the CLI, HTTP, a test, another cell).
//!
//! ## What the inference is
//!
//! Syntactic, like the passes it reuses (`refinement` for transitions,
//! the `determinism` rules for slots and calls):
//!
//!  - a slot is a memory slot of the handler's own cell, hidden by a
//!    parameter of the same name;
//!  - `moves` is sound on targets; the source of an edge is the one the
//!    handler declares (`transition(id, from, to)`) or established, else
//!    every edge of the machine into that target;
//!  - a dynamic transition target is reported (`dynamic_moves`), not guessed.

use crate::ast::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::names::ProgramIndex;

/// The footprint of one handler.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    /// Slots (and machines, by name) read but not written.
    pub reads: BTreeSet<String>,
    pub writes: BTreeSet<String>,
    /// Bones (`f`, `Cell.f`) and effectful builtins.
    pub calls: BTreeSet<String>,
    pub emits: BTreeSet<String>,
    /// (machine, from, to). The machine is "" for the cell's only machine,
    /// its name when the cell has several, `Cell.machine` for another
    /// cell's, "?" when none could be told.
    pub moves: BTreeSet<(String, String, String)>,
    /// A `transition()` whose target is computed at run time, in a cell
    /// that cannot tell which machine it moves. (When it can, the frame
    /// has the edge `* -> *` of that machine.)
    pub dynamic_moves: bool,
    /// The part of `moves` the body pins to one edge: (machine, from, to)
    /// with the source it declares or established. Inferred frames only.
    pub sure_moves: BTreeSet<(String, String, String)>,
    /// The transitions whose source is not known: (machine, to).
    pub target_moves: BTreeSet<(String, String)>,
    /// A machine read (`get_status`…) in a cell that cannot tell which
    /// machine of the program it reads.
    pub unknown_machine_read: bool,
    /// A lambda that touches memory, a handler or a machine is passed to a
    /// signal, emitted or returned: it would run elsewhere, in another
    /// cell's name.
    pub escaping_lambda: bool,
    /// …and one of them is the argument of `map(a, f)` written as a plain call.
    pub plain_map_lambda: bool,
}

impl Frame {
    pub fn is_pure(&self) -> bool {
        self.reads.is_empty() && self.writes.is_empty() && self.calls.is_empty()
            && self.emits.is_empty() && self.moves.is_empty() && !self.dynamic_moves
    }

    /// The frame a signal declares.
    pub fn from_decl(d: &FrameDecl) -> Frame {
        Frame {
            reads: d.reads.iter().filter(|r| !d.writes.contains(r)).cloned().collect(),
            writes: d.writes.iter().cloned().collect(),
            calls: d.calls.iter().cloned().collect(),
            emits: d.emits.iter().cloned().collect(),
            moves: d.moves.iter().cloned().collect(),
            ..Frame::default()
        }
    }

    fn merge(&mut self, other: &Frame) {
        self.reads.extend(other.reads.iter().cloned());
        self.writes.extend(other.writes.iter().cloned());
        self.calls.extend(other.calls.iter().cloned());
        self.emits.extend(other.emits.iter().cloned());
        self.moves.extend(other.moves.iter().cloned());
        self.dynamic_moves |= other.dynamic_moves;
        self.sure_moves.extend(other.sure_moves.iter().cloned());
        self.target_moves.extend(other.target_moves.iter().cloned());
        self.unknown_machine_read |= other.unknown_machine_read;
        self.escaping_lambda |= other.escaping_lambda;
        self.plain_map_lambda |= other.plain_map_lambda;
    }

    /// The frame as clauses, in the order they are written:
    /// `reads`, `writes`, `moves` (one per machine), `calls`, `emits`.
    pub fn clauses(&self) -> Vec<(&'static str, Vec<String>)> {
        let mut out: Vec<(&'static str, Vec<String>)> = Vec::new();
        if !self.reads.is_empty() { out.push(("reads", self.reads.iter().cloned().collect())); }
        if !self.writes.is_empty() { out.push(("writes", self.writes.iter().cloned().collect())); }
        let mut by_machine: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for (m, from, to) in &self.moves {
            by_machine.entry(m.as_str()).or_default().push(format!("{} -> {}", from, to));
        }
        for (m, edges) in by_machine {
            let mut edges = edges;
            if !m.is_empty() { edges[0] = format!("{}: {}", m, edges[0]); }
            out.push(("moves", edges));
        }
        if self.dynamic_moves { out.push(("moves", vec!["?".to_string()])); }
        if !self.calls.is_empty() { out.push(("calls", self.calls.iter().cloned().collect())); }
        if !self.emits.is_empty() { out.push(("emits", self.emits.iter().cloned().collect())); }
        out
    }
}

/// One line of the skeleton's face.
#[derive(Debug, Clone)]
pub struct Bone<'a> {
    pub name: String,
    pub params: &'a [Param],
    pub return_type: Option<&'a Spanned<TypeExpr>>,
    pub requires: &'a [Spanned<Expr>],
    pub ensures: &'a [Spanned<Expr>],
    /// Declared in the cell's `face` (a signal or a tool).
    pub in_face: bool,
    pub tool: bool,
    /// A face declaration no handler implements.
    pub no_handler: bool,
    /// A signal of a skeleton no flesh fills yet.
    pub hole: bool,
    /// `frame` is the one the signal declares (a skeleton's), not one
    /// inferred from the body.
    pub declared: bool,
    pub frame: Frame,
    /// What the body does (with the helpers it reaches), when there is one
    /// that is not a hole.
    pub inferred: Option<Frame>,
    /// The handler (the flesh's `on`), else the face declaration.
    pub span: Span,
    /// The face declaration, else the handler.
    pub decl_span: Span,
}

/// An `every` / `after` block: a body with a frame and no name.
#[derive(Debug, Clone)]
pub struct Schedule {
    pub label: String,
    pub frame: Frame,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct CellSkeleton<'a> {
    pub cell: &'a CellDef,
    /// The cell whose `interior { }` holds this one.
    pub parent: Option<&'a str>,
    pub bones: Vec<Bone<'a>>,
    pub schedules: Vec<Schedule>,
    /// Handlers folded into the bones that reach them.
    pub helpers: Vec<String>,
    /// In a skeleton: helpers no signal reaches.
    pub unused_helpers: Vec<(String, Span)>,
    /// In a skeleton: helpers whose name does not start with `_` — the
    /// runtime would let the outside call them.
    pub public_helpers: Vec<(String, Span)>,
    /// Calls, from this cell, of another skeleton's private helper:
    /// (caller, `Cell.helper`, where).
    pub private_calls: Vec<(String, String, Span)>,
}

/// The slot methods that write (`put` is `set`, `remove` is `delete`).
fn mutating_method(name: &str) -> bool {
    matches!(name, "set" | "put" | "delete" | "remove" | "append" | "push" | "clear")
}

fn machine_builtin(name: &str) -> bool {
    matches!(name, "transition" | "get_status" | "has_state" | "valid_transitions")
}

/// A builtin a frame must name: its answer or its effect lies outside the
/// handler's arguments. Printing and sleeping change no value.
pub(crate) fn effectful_builtin(name: &str) -> bool {
    !machine_builtin(name) && !matches!(name, "print" | "sleep")
        && (super::names::unstable_builtin(name) || super::names::EFFECT_BUILTINS.contains(&name))
}

/// Builtins that run a handler named at run time (or any builtin): a frame
/// that calls one can reach every signal of the program. The frame says so
/// by naming the builtin; analyses that ask "who can do X" count such a
/// frame as able to.
pub const OPAQUE_BUILTINS: &[&str] = &["delegate", "_std", "resume", "deliver", "horde", "vote"];

/// Builtins that apply the lambda they are given before they return.
const RUNS_ITS_LAMBDA: &[&str] = &["map", "filter", "find", "any", "all", "count", "reduce", "sort_by", "group_by", "min_by", "max_by", "sum_by"];

fn root_of(name: &str) -> &str {
    name.split(|c| c == '.' || c == '[').next().unwrap_or("")
}

pub(crate) fn slots_of(cell: &CellDef) -> HashSet<String> {
    cell.sections.iter()
        .filter_map(|s| if let Section::Memory(m) = &s.node { Some(m) } else { None })
        .flat_map(|m| m.slots.iter().map(|sl| sl.node.name.clone()))
        .collect()
}

pub(crate) fn machines_of(cell: &CellDef) -> Vec<&StateMachineSection> {
    cell.sections.iter()
        .filter_map(|s| if let Section::State(sm) = &s.node { Some(sm) } else { None })
        .collect()
}

fn has_state(sm: &StateMachineSection, state: &str) -> bool {
    sm.initial == state || sm.transitions.iter().any(|t| t.node.to == state || t.node.from == state)
}

/// Every cell of the program with the cell whose interior holds it.
pub fn cells_with_parents(program: &Program) -> Vec<(&CellDef, Option<&str>)> {
    fn rec<'a>(cell: &'a CellDef, parent: Option<&'a str>, out: &mut Vec<(&'a CellDef, Option<&'a str>)>) {
        out.push((cell, parent));
        for s in &cell.sections {
            if let Section::Interior(interior) = &s.node {
                for child in &interior.cells { rec(&child.node, Some(&cell.name), out); }
            }
        }
    }
    let mut out = Vec::new();
    for c in &program.cells { rec(&c.node, None, &mut out); }
    out
}

/// A node of a handler body, as `walk_scoped` hands it out.
enum Node<'a> {
    Stmt(&'a Statement),
    Expr(&'a Expr),
}

/// Every statement and expression of a body, each once, with the locals
/// CERTAINLY bound where it stands: the parameters, the `let`s that come
/// before it in an enclosing block, the loop variable inside a loop, the
/// parameter inside a lambda, the binders of a match arm inside the arm.
fn walk_scoped<'a>(stmts: &'a [Spanned<Statement>], scope: &mut Vec<String>, f: &mut dyn FnMut(Node<'a>, &[String])) {
    let mark = scope.len();
    for st in stmts {
        f(Node::Stmt(&st.node), scope);
        match &st.node {
            Statement::Let { name, value } => {
                walk_scoped_expr(&value.node, scope, f);
                scope.push(name.clone());
            }
            Statement::Assign { value, .. } | Statement::Return { value } | Statement::Ensure { condition: value } => walk_scoped_expr(&value.node, scope, f),
            Statement::ExprStmt { expr } => walk_scoped_expr(&expr.node, scope, f),
            Statement::IndexSet { index, value, .. } => { walk_scoped_expr(&index.node, scope, f); walk_scoped_expr(&value.node, scope, f); }
            Statement::If { condition, then_body, else_body } => {
                walk_scoped_expr(&condition.node, scope, f);
                walk_scoped(then_body, scope, f);
                walk_scoped(else_body, scope, f);
            }
            Statement::For { var, iter, body, .. } => {
                walk_scoped_expr(&iter.node, scope, f);
                scope.push(var.clone());
                walk_scoped(body, scope, f);
                scope.pop();
            }
            Statement::While { condition, body, .. } => {
                walk_scoped_expr(&condition.node, scope, f);
                walk_scoped(body, scope, f);
            }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => { for a in args { walk_scoped_expr(&a.node, scope, f); } }
            Statement::Require { constraint, .. } => walk_scoped_constraint(&constraint.node, scope, f),
            Statement::Break | Statement::Continue => {}
        }
    }
    scope.truncate(mark);
}

fn walk_scoped_constraint<'a>(c: &'a Constraint, scope: &mut Vec<String>, f: &mut dyn FnMut(Node<'a>, &[String])) {
    match c {
        Constraint::Comparison { left, right, .. } => { walk_scoped_expr(&left.node, scope, f); walk_scoped_expr(&right.node, scope, f); }
        Constraint::And(a, b) | Constraint::Or(a, b) => { walk_scoped_constraint(&a.node, scope, f); walk_scoped_constraint(&b.node, scope, f); }
        Constraint::Not(i) => walk_scoped_constraint(&i.node, scope, f),
        Constraint::Predicate { args, .. } => { for a in args { walk_scoped_expr(&a.node, scope, f); } }
        Constraint::Descriptive(_) => {}
    }
}

fn walk_scoped_expr<'a>(e: &'a Expr, scope: &mut Vec<String>, f: &mut dyn FnMut(Node<'a>, &[String])) {
    f(Node::Expr(e), scope);
    let mut ex = |x: &'a Spanned<Expr>, scope: &mut Vec<String>| walk_scoped_expr(&x.node, scope, f);
    match e {
        Expr::FnCall { args, .. } => { for a in args { ex(a, scope); } }
        Expr::MethodCall { target, args, .. } => { ex(target, scope); for a in args { ex(a, scope); } }
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => { ex(left, scope); ex(right, scope); }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => ex(i, scope),
        Expr::FieldAccess { target, .. } => ex(target, scope),
        Expr::Index { target, index } => { ex(target, scope); ex(index, scope); }
        Expr::ListLiteral(items) => { for it in items { ex(it, scope); } }
        Expr::Record { fields, .. } => { for (_, v) in fields { ex(v, scope); } }
        Expr::Lambda { param, body } => {
            scope.push(param.clone());
            ex(body, scope);
            scope.pop();
        }
        Expr::LambdaBlock { param, stmts, result } => {
            let mark = scope.len();
            scope.push(param.clone());
            // the block's lets stay bound for its result
            for st in stmts.iter() { walk_scoped_keep(std::slice::from_ref(st), scope, f); }
            walk_scoped_expr(&result.node, scope, f);
            scope.truncate(mark);
        }
        Expr::Match { subject, arms } => {
            walk_scoped_expr(&subject.node, scope, f);
            for arm in arms {
                let mark = scope.len();
                let mut bound = HashSet::new();
                super::interpolation_check::bind_pattern(&arm.pattern, &mut bound);
                scope.extend(bound);
                if let Some(g) = &arm.guard { walk_scoped_expr(&g.node, scope, f); }
                for st in arm.body.iter() { walk_scoped_keep(std::slice::from_ref(st), scope, f); }
                walk_scoped_expr(&arm.result.node, scope, f);
                scope.truncate(mark);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            walk_scoped_expr(&condition.node, scope, f);
            for (body, result) in [(then_body, then_result), (else_body, else_result)] {
                let mark = scope.len();
                for st in body.iter() { walk_scoped_keep(std::slice::from_ref(st), scope, f); }
                walk_scoped_expr(&result.node, scope, f);
                scope.truncate(mark);
            }
        }
        _ => {}
    }
}

/// One statement of an expression block, its `let` left in scope for the
/// statements and the result that follow it.
fn walk_scoped_keep<'a>(st: &'a [Spanned<Statement>], scope: &mut Vec<String>, f: &mut dyn FnMut(Node<'a>, &[String])) {
    let name = match &st[0].node { Statement::Let { name, .. } => Some(name.clone()), _ => None };
    walk_scoped(st, scope, f);
    if let Some(n) = name { scope.push(n); }
}

/// What one body does by itself: its frame, less the handler calls, which
/// are kept apart until bones and helpers are told apart.
#[derive(Debug, Clone, Default)]
struct Direct {
    frame: Frame,
    /// (cell, handler)
    callees: BTreeSet<(String, String)>,
}

struct Ctx<'a> {
    index: ProgramIndex,
    /// every machine of the program, with its cell
    machines: Vec<(&'a str, &'a StateMachineSection)>,
    /// (handler names, cell names), for `desugar::expose_handler`
    names: (HashSet<String>, HashSet<String>),
}

impl<'a> Ctx<'a> {
    /// The machine a `transition()` / `get_status()` written in `cell`
    /// reaches, and its label in a frame. A cell's own machine first; a
    /// machine-less cell moves the program's only machine, or the only one
    /// that has the state.
    fn machine_for(&self, cell: &CellDef, state: Option<&str>) -> Option<(String, &'a StateMachineSection)> {
        let own: Vec<&'a StateMachineSection> = self.machines.iter()
            .filter(|(c, _)| *c == cell.name).map(|(_, sm)| *sm).collect();
        if !own.is_empty() {
            let sm = state.and_then(|s| own.iter().copied().find(|sm| has_state(sm, s))).unwrap_or(own[0]);
            let label = if own.len() == 1 { String::new() } else { sm.name.clone() };
            return Some((label, sm));
        }
        let pick = if self.machines.len() == 1 {
            Some(self.machines[0])
        } else {
            let with: Vec<(&'a str, &'a StateMachineSection)> = match state {
                Some(s) => self.machines.iter().copied().filter(|(_, sm)| has_state(sm, s)).collect(),
                None => Vec::new(),
            };
            if with.len() == 1 { Some(with[0]) } else { None }
        };
        pick.map(|(c, sm)| (format!("{}.{}", c, sm.name), sm))
    }

    /// Interpreter rule for `f(args)`: the caller's own handler `f`, else
    /// another cell's, when one takes that many arguments; the builtin `f`
    /// otherwise.
    fn resolve_call(&self, cell: &CellDef, name: &str, argc: usize) -> Option<String> {
        let definers = self.index.handler_map.get(name)?;
        let is_builtin = super::names::builtin_names().contains(name);
        let fits = |c: &str| self.index.arity.get(&(c.to_string(), name.to_string()))
            .map_or(false, |(lo, hi)| argc >= *lo && argc <= *hi);
        if definers.contains(&cell.name) && (fits(&cell.name) || !is_builtin) {
            return Some(cell.name.clone());
        }
        // a handler of ANOTHER cell never replaces a builtin: `xs.filter(f)`
        // is the builtin whatever signal some cell calls `filter`
        if is_builtin { return None; }
        definers.first().cloned()
    }

    fn direct(&self, cell: &CellDef, on: &OnSection, span: Span) -> Direct {
        // the calls and slot reads written inside string interpolations
        // (`"{level(sku)} left"`) and other sugar, as expressions
        let written = on;
        let exposed = super::desugar::expose_handler(on, cell, &self.names);
        let on = &exposed;
        let slots = slots_of(cell);
        let mut d = Direct::default();
        let mut reads_machine = false;
        // `std.transition(…)`: a move the refinement walker does not read
        let mut std_moves = false;
        // A name is the slot / the cell it names unless a local of that name
        // is CERTAINLY bound where it is written (a parameter, a `let`
        // earlier in an enclosing block, a loop variable, a lambda or match
        // binder): the interpreter looks the environment up first. A local
        // that only may be bound — in a branch, later in the body — hides
        // nothing here. A call is counted whatever is bound: `f(x)` runs the
        // handler `f` whenever no lambda is in the way, and the analysis
        // does not know which values are lambdas.
        // A lambda runs in the cell of WHOEVER CALLS IT — its slots, its
        // helpers — and possibly after the handler that wrote it returned.
        // Following where a lambda value goes is a losing game (a list, an
        // alias, a branch, a helper's answer…), so the rule is about where
        // it is WRITTEN: a lambda that touches memory, a handler, an
        // effectful builtin or a machine may only stand as the direct
        // argument of a builtin that runs it at once (`xs.map(x => …)`).
        // There it runs here and now, and what it does is the body's.
        // Anywhere else — bound, stored, passed to a signal, returned — it
        // must be pure.
        let run_now: std::cell::RefCell<HashSet<*const Expr>> = std::cell::RefCell::new(HashSet::new());
        let impure = |e: &Expr, scope: &[String]| -> bool {
            // the same lexical scope as the body's: a binder hides a slot only
            // where it is certainly bound (a `let` in a dead branch of the
            // lambda, a parameter of a lambda nested in it, hide nothing outside)
            let mut found = false;
            let mut inner: Vec<String> = scope.to_vec();
            walk_scoped_expr(e, &mut inner, &mut |node, scope| {
                let local = |n: &str| scope.iter().any(|x| x == n);
                let is_slot = |n: &str| slots.contains(n) && !local(n);
                match node {
                    Node::Expr(x) => match x {
                        Expr::Ident(n) if is_slot(n) => found = true,
                        Expr::FnCall { name, args } => {
                            if name == "transition" || name == "_std" || machine_builtin(name) || effectful_builtin(name) || self.resolve_call(cell, name, args.len()).is_some() { found = true; }
                        }
                        Expr::MethodCall { target, method, args } => {
                            if machine_builtin(method) || method == "transition" || effectful_builtin(method) || self.resolve_call(cell, method, args.len() + 1).is_some() { found = true; }
                            if let Expr::Ident(t) = &target.node { if (self.index.cells.contains(t) && !local(t)) || is_slot(t) { found = true; } }
                        }
                        _ => {}
                    },
                    Node::Stmt(st) => match st {
                        Statement::Emit { .. } => found = true,
                        Statement::MethodCall { target, .. } if is_slot(root_of(target)) || (self.index.cells.contains(root_of(target)) && !local(root_of(target))) => found = true,
                        Statement::IndexSet { name, .. } | Statement::Assign { name, .. } if is_slot(root_of(name)) => found = true,
                        _ => {}
                    },
                }
            });
            found
        };
        // the lambdas written as a direct argument of a builtin that applies it
        // Which lambda does a builtin apply before it returns? The LAST
        // argument (`reduce(xs, initial, f)`: not the initial value), of a
        // name that IS a builtin and that no local hides. `map(a, f)` as a
        // plain call BUILDS a map holding `f` when `a` is a String: it
        // applies `f` only over what is written as a list.
        fn list_shaped(e: &Expr) -> bool {
            match e {
                Expr::ListLiteral(_) => true,
                Expr::FnCall { name, .. } => matches!(name.as_str(), "list" | "range"),
                _ => false,
            }
        }
        let plain_map = std::cell::Cell::new(false);
        let note_run_now = |name: &str, subject: Option<&Expr>, args: &[Spanned<Expr>], scope: &[String]| {
            if !RUNS_ITS_LAMBDA.contains(&name) || !super::names::builtin_names().contains(name) || scope.iter().any(|x| x == name) { return; }
            let Some(last) = args.last() else { return };
            if !matches!(last.node, Expr::Lambda { .. } | Expr::LambdaBlock { .. }) { return; }
            if name == "map" && !subject.map_or(false, list_shaped) { plain_map.set(true); return; }
            run_now.borrow_mut().insert(&last.node as *const Expr);
        };
        // …judged on the body AS WRITTEN: `xs |> map(f)` and `xs.map(f)` refuse a
        // String at run time and always apply `f`; `map(a, f)` written as a
        // plain call builds a map holding `f` when `a` is a String.
        let mut lambda_scope: Vec<String> = written.params.iter().map(|p| p.name.clone()).collect();
        let mut escaping = false;
        walk_scoped(&written.body, &mut lambda_scope, &mut |node, scope| {
            let Node::Expr(e) = node else { return };
            match e {
                // `xs |> map(f)` and `xs.map(f)` refuse a String at run time: they apply `f`
                Expr::Pipe { right, .. } => {
                    if let Expr::FnCall { name, args } = &right.node {
                        if self.resolve_call(cell, name, args.len() + 1).is_none() { note_run_now(name, Some(&Expr::ListLiteral(Vec::new())), args, scope); }
                    }
                }
                Expr::MethodCall { target, method, args } => {
                    // `Cell.filter(f)` is the signal `filter` of `Cell`, not the builtin
                    let on_cell = matches!(&target.node, Expr::Ident(t) if self.index.cells.contains(t) && !scope.iter().any(|x| x == t));
                    if !on_cell && self.resolve_call(cell, method, args.len() + 1).is_none() { note_run_now(method, Some(&Expr::ListLiteral(Vec::new())), args, scope); }
                }
                Expr::FnCall { name, args } => {
                    if self.resolve_call(cell, name, args.len()).is_none() { note_run_now(name, args.first().map(|a| &a.node), args, scope); }
                }
                Expr::Lambda { .. } | Expr::LambdaBlock { .. } => {
                    if run_now.borrow().contains(&(e as *const Expr)) { return; }
                    // with what it hides in interpolations made explicit
                    let probe = OnSection { signal_name: written.signal_name.clone(), params: written.params.clone(), properties: Vec::new(),
                        body: vec![Spanned::new(Statement::ExprStmt { expr: Spanned::new(e.clone(), span) }, span)] };
                    let shown = super::desugar::expose_handler(&probe, cell, &self.names);
                    let judged = shown.body.iter().any(|st| match &st.node {
                        Statement::ExprStmt { expr } => impure(&expr.node, scope),
                        _ => true,
                    });
                    if judged { escaping = true; }
                }
                _ => {}
            }
        });
        d.frame.escaping_lambda = escaping;
        d.frame.plain_map_lambda = escaping && plain_map.get();
        let mut scope: Vec<String> = on.params.iter().map(|p| p.name.clone()).collect();
        // the names the handler binds SOMEWHERE (an assignment without `let`
        // makes a local for the whole handler; a branch, one alternative of
        // an `||` pattern, a lambda parameter bind one that may be in scope)
        let mut maybe: HashSet<String> = HashSet::new();
        super::invariants::collect_binders_stmts(&on.body, &mut maybe);
        super::literals::for_each_stmt_deep(&on.body, &mut |st| match st {
            Statement::Let { name, .. } | Statement::Assign { name, .. } => { maybe.insert(root_of(name).to_string()); }
            Statement::For { var, .. } => { maybe.insert(var.clone()); }
            _ => {}
        });
        super::literals::for_each_expr(&on.body, &mut |e| match e {
            Expr::Lambda { param, .. } | Expr::LambdaBlock { param, .. } => for p in param.split(|c: char| !(c.is_alphanumeric() || c == '_')) { if !p.is_empty() { maybe.insert(p.to_string()); } },
            Expr::Match { arms, .. } => for arm in arms { super::invariants::pattern_binders(&arm.pattern, &mut maybe); },
            _ => {}
        });
        walk_scoped(&on.body, &mut scope, &mut |node, scope| {
            let local = |n: &str| scope.iter().any(|x| x == n);
            let slot = |n: &str| slots.contains(n) && !local(n);
            let is_cell = |t: &str| self.index.cells.contains(t);
            match node {
                Node::Expr(e) => match e {
                    // `std.f(…)` is the builtin `f`, whatever handlers are named `f`
                    Expr::FnCall { name, args } if name == "_std" => {
                        match args.first().map(|a| &a.node) {
                            Some(Expr::Literal(Literal::String(f))) => {
                                if f == "transition" { std_moves = true; }
                                else if machine_builtin(f) { reads_machine = true; }
                                else if effectful_builtin(f) { d.frame.calls.insert(f.clone()); }
                            }
                            _ => { d.frame.calls.insert("_std".to_string()); }
                        }
                    }
                    Expr::FnCall { name, args } => {
                        // `vote("Cell.h", …)`, `horde("Cell.h", …)`: the handler, named
                        // in a string — a call of it, as `vote(Cell.h, …)` is
                        // …and by a bare name, a string or a callback option
                        // (`vote("_w", …)`, `vote(_w, …)`, `horde(R.review, xs,
                        // map("on_result", "_store"))`): this cell's handler — its
                        // effects are this signal's
                        if matches!(name.as_str(), "vote" | "horde") {
                            let own = |h: &str| cell.sections.iter().any(|s| matches!(&s.node, Section::OnSignal(on) if on.signal_name == h));
                            for a in args {
                                super::literals::for_each_in_expr(&a.node, &mut |x| match x {
                                    Expr::Literal(Literal::String(t)) => match t.split_once('.') {
                                        Some((c, h)) => if self.index.cells.contains(c) && self.index.handler_map.get(h).is_some_and(|cs| cs.iter().any(|x| x == c)) {
                                            d.callees.insert((c.to_string(), h.to_string()));
                                        },
                                        None => if own(t) { d.callees.insert((cell.name.clone(), t.clone())); },
                                    },
                                    Expr::Ident(h) if own(h) && !local(h) => { d.callees.insert((cell.name.clone(), h.clone())); }
                                    _ => {}
                                });
                            }
                            // the target by a VARIABLE (`let w = "_evil"  vote(w, …)`): any
                            // handler of this cell may be the one that runs — all of them count
                            let by_variable = match args.first().map(|a| &a.node) {
                                Some(Expr::Ident(n)) => local(n) || maybe.contains(n) || !own(n),
                                Some(Expr::Literal(Literal::String(t))) => !super::desugar::segments(t).is_empty(),
                                Some(Expr::FieldAccess { target, .. }) => !matches!(&target.node, Expr::Ident(c) if is_cell(c) && !local(c) && !maybe.contains(c)),
                                Some(_) => true,
                                None => false,
                            };
                            if by_variable {
                                for s in &cell.sections {
                                    if let Section::OnSignal(on) = &s.node { d.callees.insert((cell.name.clone(), on.signal_name.clone())); }
                                }
                            }
                        }
                        if name == "transition" {
                            // the refinement walker below reads these
                        } else if machine_builtin(name) {
                            reads_machine = true;
                        } else if let Some(c) = self.resolve_call(cell, name, args.len()) {
                            d.callees.insert((c, name.clone()));
                        } else if effectful_builtin(name) {
                            d.frame.calls.insert(name.clone());
                        }
                    }
                    Expr::MethodCall { target, method, args } => {
                        if let Expr::Ident(t) = &target.node {
                            // `Cell.h(…)`: counted unless a local of that name is certainly in the way
                            // (a slot of that name is the slot: the interpreter looks it up first)
                            if is_cell(t) && !local(t) && !slots.contains(t) {
                                d.callees.insert((t.clone(), method.clone()));
                                // …and a local that MAY be in the way (bound by an
                                // assignment, in a branch, by one alternative of an
                                // `||` pattern) makes it `method(t, …)` as well
                                if maybe.contains(t) {
                                    if let Some(c) = self.resolve_call(cell, method, args.len() + 1) { d.callees.insert((c, method.clone())); }
                                }
                                    return;
                            }
                            // …with a local in the way it is `method(t, …)`: a
                            // handler called in method form
                            if is_cell(t) && local(t) {
                                if let Some(c) = self.resolve_call(cell, method, args.len() + 1) { d.callees.insert((c, method.clone())); }
                            }
                            if slot(t) && mutating_method(method) { d.frame.writes.insert(t.clone()); }
                            // a local that bears a SLOT's name (certainly, or perhaps) makes
                            // `t.h(…)` the call `h(t, …)` of a handler: desugar left it alone
                            if slots.contains(t) && (local(t) || maybe.contains(t)) {
                                if let Some(c) = self.resolve_call(cell, method, args.len() + 1) { d.callees.insert((c, method.clone())); }
                            }
                        }
                        // `x.think()`, `id.transition("paid")`, `id.bump()`: a
                        // builtin or a handler called in method form
                        if machine_builtin(method) { reads_machine = true; }
                        else if effectful_builtin(method) { d.frame.calls.insert(method.clone()); }
                    }
                    // `horde(Reviewer.review, …)`: a handler passed by name
                    Expr::FieldAccess { target, field } => {
                        if let Expr::Ident(t) = &target.node {
                            if is_cell(t) && !local(t) && self.index.arity.contains_key(&(t.clone(), field.clone())) {
                                d.callees.insert((t.clone(), field.clone()));
                            }
                        }
                    }
                    Expr::Ident(n) if slot(n) => { d.frame.reads.insert(n.clone()); }
                    _ => {}
                },
                Node::Stmt(st) => match st {
                    Statement::Emit { signal_name, .. } => { d.frame.emits.insert(signal_name.clone()); }
                    Statement::MethodCall { target, method, args } => {
                        let root = root_of(target);
                        if root == target.as_str() && is_cell(root) && !local(root) && !slots.contains(root) {
                            d.callees.insert((root.to_string(), method.clone()));
                            if maybe.contains(root) {
                                if let Some(c) = self.resolve_call(cell, method, args.len() + 1) { d.callees.insert((c, method.clone())); }
                            }
                        } else if root == target.as_str() && is_cell(root) && local(root) {
                            if let Some(c) = self.resolve_call(cell, method, args.len() + 1) { d.callees.insert((c, method.clone())); }
                        } else if slot(root) {
                            if mutating_method(method) { d.frame.writes.insert(root.to_string()); }
                            else { d.frame.reads.insert(root.to_string()); }
                            if root == target.as_str() && maybe.contains(root) {
                                if let Some(c) = self.resolve_call(cell, method, args.len() + 1) { d.callees.insert((c, method.clone())); }
                            }
                        } else if root == target.as_str() && slots.contains(root) && local(root) {
                            if let Some(c) = self.resolve_call(cell, method, args.len() + 1) { d.callees.insert((c, method.clone())); }
                        } else if machine_builtin(method) {
                            reads_machine = true;
                        } else if effectful_builtin(method) {
                            d.frame.calls.insert(method.clone());
                        }
                    }
                    Statement::IndexSet { name, .. } | Statement::Assign { name, .. } if slot(root_of(name)) => {
                        d.frame.writes.insert(root_of(name).to_string());
                    }
                    _ => {}
                },
            }
        });

        if reads_machine {
            match self.machine_for(cell, None) {
                Some((l, sm)) if l.is_empty() => { d.frame.reads.insert(sm.name.clone()); }
                Some((l, _)) => { d.frame.reads.insert(l); }
                // no machine in the program: the call raises, it reads nothing
                None if self.machines.is_empty() => {}
                None => d.frame.unknown_machine_read = true,
            }
        }

        let eff = super::refinement::analyze_handler(on, span);
        // a target computed at run time (`transition(id, cmd)`: the machine
        // refuses what it has no edge for) can take any edge: `moves * -> *`
        if eff.has_dynamic_target || std_moves {
            match self.machine_for(cell, None) {
                Some((label, _)) => {
                    d.frame.moves.insert((label.clone(), "*".to_string(), "*".to_string()));
                    d.frame.sure_moves.insert((label, "*".to_string(), "*".to_string()));
                }
                None => d.frame.dynamic_moves = true,
            }
        }
        for call in &eff.static_transitions {
            let Some((label, sm)) = self.machine_for(cell, Some(&call.target)) else {
                d.frame.moves.insert(("?".to_string(), "?".to_string(), call.target.clone()));
                continue;
            };
            let has_edge = |from: &str| sm.transitions.iter().any(|t| t.node.to == call.target
                && (t.node.from == from || (t.node.from == "*" && sm.wildcard_applies(&t.node, from))));
            // Only a source the body NAMES pins the edge (`transition(id,
            // from, to)`, enforced at run time). What a body works out for
            // itself — `if get_status(id) == "a" { transition(id, "x") }` — is
            // tracked by the text of the id, and a rebinding, an alias, a
            // call in the same condition or a lambda defeat it: it is not
            // evidence for a frame.
            let known: Vec<String> = call.source.iter().cloned().collect();
            if known.is_empty() {
                d.frame.target_moves.insert((label.clone(), call.target.clone()));
            } else {
                for from in &known { d.frame.sure_moves.insert((label.clone(), from.clone(), call.target.clone())); }
            }
            let mut froms: Vec<String> = if !known.is_empty() { known } else {
                sm.transitions.iter().filter(|t| t.node.to == call.target).map(|t| t.node.from.clone()).collect()
            };
            if froms.is_empty() { froms.push("?".to_string()); }
            for from in froms {
                d.frame.moves.insert((label.clone(), from, call.target.clone()));
            }
        }
        d
    }
}

fn duration_label(kind: &str, ms: u64) -> String {
    if ms >= 1000 && ms % 1000 == 0 { format!("{} {}s", kind, ms / 1000) } else { format!("{} {}ms", kind, ms) }
}

/// The skeleton of every `cell` / `cell agent` of the program. In a plain
/// cell, a handler outside the face that only its own cell calls is folded
/// into its callers: the summary an agent reads.
pub fn build(program: &Program) -> Vec<CellSkeleton<'_>> {
    build_with(program, false)
}

/// The same, with the rule a `skeleton` lives by: only a `_private` handler
/// is a helper. Every other handler of a plain cell is reachable from
/// outside (the CLI, an HTTP route) and so is a signal with its own frame —
/// what `--split` must write for the program to stay the same program.
pub fn build_faithful(program: &Program) -> Vec<CellSkeleton<'_>> {
    build_with(program, true)
}

fn build_with(program: &Program, faithful: bool) -> Vec<CellSkeleton<'_>> {
    let all = cells_with_parents(program);
    let cells: Vec<(&CellDef, Option<&str>)> = all.iter().copied()
        .filter(|(c, _)| matches!(c.kind, CellKind::Cell | CellKind::Agent)).collect();
    let ctx = Ctx {
        index: ProgramIndex::build(program),
        names: super::desugar::program_names(program),
        machines: cells.iter().flat_map(|(c, _)| machines_of(c).into_iter().map(move |sm| (c.name.as_str(), sm))).collect(),
    };

    // every body's own effects
    let mut direct: HashMap<(String, String), Direct> = HashMap::new();
    let mut spans: HashMap<(String, String), Span> = HashMap::new();
    let mut sched: HashMap<String, Vec<(String, Direct, Span)>> = HashMap::new();
    for (cell, _) in &cells {
        for s in &cell.sections {
            match &s.node {
                Section::OnSignal(on) => {
                    spans.entry((cell.name.clone(), on.signal_name.clone())).or_insert(s.span);
                    direct.entry((cell.name.clone(), on.signal_name.clone()))
                        .or_insert_with(|| ctx.direct(cell, on, s.span));
                }
                Section::Every(ev) | Section::After(ev) => {
                    let kind = if matches!(s.node, Section::Every(_)) { "every" } else { "after" };
                    let on = OnSection { signal_name: kind.to_string(), params: Vec::new(), body: ev.body.clone(), properties: Vec::new() };
                    sched.entry(cell.name.clone()).or_default()
                        .push((duration_label(kind, ev.interval_ms), ctx.direct(cell, &on, s.span), s.span));
                }
                _ => {}
            }
        }
    }

    // who calls whom: a handler called from its own cell only is a helper
    let mut from_own: HashSet<(String, String)> = HashSet::new();
    let mut from_other: HashSet<(String, String)> = HashSet::new();
    let mut note = |caller_cell: &str, caller: Option<&str>, callees: &BTreeSet<(String, String)>| {
        for (c, h) in callees {
            if c == caller_cell {
                if caller != Some(h.as_str()) { from_own.insert((c.clone(), h.clone())); }
            } else {
                from_other.insert((c.clone(), h.clone()));
            }
        }
    };
    for ((c, h), d) in &direct { note(c, Some(h), &d.callees); }
    for (c, list) in &sched { for (_, d, _) in list { note(c, None, &d.callees); } }

    let mut out = Vec::new();
    for (cell, parent) in &cells {
        let mut face: Vec<Bone> = Vec::new();
        for s in &cell.sections {
            if let Section::Face(f) = &s.node {
                for decl in &f.declarations {
                    let (name, params, ret, requires, ensures, tool, decl_frame): (&str, &[Param], _, &[Spanned<Expr>], &[Spanned<Expr>], bool, Option<&FrameDecl>) = match &decl.node {
                        FaceDecl::Signal(sig) => (&sig.name, &sig.params, sig.return_type.as_ref(), &sig.requires, &sig.ensures, false, sig.frame.as_ref()),
                        FaceDecl::Tool(t) => (&t.name, &t.params, t.return_type.as_ref(), &[], &[], true, t.frame.as_ref()),
                        _ => continue,
                    };
                    if face.iter().any(|b| b.name == name) { continue; }
                    let handler = cell.sections.iter().find_map(|s| match &s.node { Section::OnSignal(on) if on.signal_name == name => Some(on), _ => None });
                    let hole = handler.map_or(false, super::flesh::is_hole);
                    // a body reads ITS parameter names: where a cell's handler
                    // names them differently from the face, the skeleton it
                    // would have carries the handler's
                    let params = match handler {
                        Some(on) if !cell.skeleton && on.params.len() == params.len() => &on.params[..],
                        _ => params,
                    };
                    face.push(Bone {
                        name: name.to_string(), params, return_type: ret, requires, ensures,
                        in_face: true, tool,
                        no_handler: !direct.contains_key(&(cell.name.clone(), name.to_string())),
                        hole,
                        declared: decl_frame.is_some(),
                        frame: decl_frame.map(Frame::from_decl).unwrap_or_default(),
                        inferred: None,
                        span: spans.get(&(cell.name.clone(), name.to_string())).copied().unwrap_or(decl.span),
                        decl_span: decl.span,
                    });
                }
            }
        }
        let in_face: HashSet<String> = face.iter().map(|b| b.name.clone()).collect();
        // in a skeleton the face IS the list of signals: every other
        // handler is a private helper, whoever calls it
        let is_helper = |h: &str| {
            let key = (cell.name.clone(), h.to_string());
            !in_face.contains(h) && (cell.skeleton
                || (from_own.contains(&key) && !from_other.contains(&key) && (!faithful || h.starts_with('_'))))
        };

        // the frame of a body: its own effects and those of the helpers it
        // reaches; a call of a bone stays a call
        let mut reached: HashSet<String> = HashSet::new();
        let close = |start: &Direct, me: Option<&str>, reached: &mut HashSet<String>| -> Frame {
            let mut frame = start.frame.clone();
            let mut seen: HashSet<String> = HashSet::new();
            let mut todo: Vec<(String, String)> = start.callees.iter().cloned().collect();
            while let Some((c, h)) = todo.pop() {
                if c == cell.name && is_helper(&h) && me != Some(h.as_str()) {
                    if !seen.insert(h.clone()) { continue; }
                    reached.insert(h.clone());
                    if let Some(d) = direct.get(&(c, h)) {
                        frame.merge(&d.frame);
                        todo.extend(d.callees.iter().cloned());
                    }
                } else if c == cell.name {
                    if me != Some(h.as_str()) { frame.calls.insert(h); }
                } else {
                    frame.calls.insert(format!("{}.{}", c, h));
                }
            }
            let writes = frame.writes.clone();
            frame.reads.retain(|r| !writes.contains(r));
            // a signal that moves a machine may read it, as one that
            // writes a slot may read it
            let moved: HashSet<String> = frame.moves.iter().map(|(label, _, _)| {
                if label.contains('.') { label.clone() }
                else if label.is_empty() { machines_of(cell).first().map(|sm| sm.name.clone()).unwrap_or_default() }
                else { label.clone() }
            }).collect();
            frame.reads.retain(|r| !moved.contains(r));
            frame
        };

        let mut bones = face;
        for b in bones.iter_mut() {
            if let Some(d) = direct.get(&(cell.name.clone(), b.name.clone())) {
                // the body is walked either way: the helpers it reaches are
                // helpers whether or not the frame is declared
                let inferred = close(d, Some(&b.name), &mut reached);
                if !b.declared { b.frame = inferred.clone(); }
                if !b.hole { b.inferred = Some(inferred); }
            }
        }
        let handlers: Vec<&OnSection> = cell.sections.iter()
            .filter_map(|s| if let Section::OnSignal(on) = &s.node { Some(on) } else { None }).collect();
        let loose = |name: &str, reached: &mut HashSet<String>| -> (Frame, Span) {
            let key = (cell.name.clone(), name.to_string());
            (close(&direct[&key], Some(name), reached), spans[&key])
        };
        fn loose_bone<'a>(on: &'a OnSection, frame: Frame, span: Span) -> Bone<'a> {
            Bone {
                name: on.signal_name.clone(), params: &on.params, return_type: None, requires: &[], ensures: &[],
                in_face: false, tool: false, no_handler: false, hole: false, declared: false,
                inferred: Some(frame.clone()), frame, span, decl_span: span,
            }
        }
        for on in &handlers {
            if in_face.contains(&on.signal_name) || is_helper(&on.signal_name) { continue; }
            if bones.iter().any(|b| b.name == on.signal_name) { continue; }
            let (frame, span) = loose(&on.signal_name, &mut reached);
            bones.push(loose_bone(on, frame, span));
        }
        let schedules: Vec<Schedule> = sched.get(&cell.name).map(|list| list.iter()
            .map(|(label, d, span)| Schedule { label: label.clone(), frame: close(d, None, &mut reached), span: *span }).collect())
            .unwrap_or_default();
        // helpers that only call one another are reached by no bone: in a
        // cell they are bones, or their effects would be in no frame; in a
        // skeleton they are dead code
        let mut helpers: Vec<String> = Vec::new();
        let mut unused_helpers: Vec<(String, Span)> = Vec::new();
        for on in &handlers {
            if !is_helper(&on.signal_name) || helpers.contains(&on.signal_name) { continue; }
            if reached.contains(&on.signal_name) {
                helpers.push(on.signal_name.clone());
            } else if cell.skeleton {
                helpers.push(on.signal_name.clone());
                unused_helpers.push((on.signal_name.clone(), spans[&(cell.name.clone(), on.signal_name.clone())]));
            } else if !bones.iter().any(|b| b.name == on.signal_name) {
                let (frame, span) = loose(&on.signal_name, &mut reached);
                bones.push(loose_bone(on, frame, span));
            }
        }
        // another skeleton's helper is not this cell's to call
        let mut private_calls: Vec<(String, String, Span)> = Vec::new();
        let skeleton_helper = |c: &str, h: &str| cells.iter().any(|(other, _)| other.name == c && other.skeleton
            && direct.contains_key(&(c.to_string(), h.to_string()))
            && !other.sections.iter().any(|s| matches!(&s.node, Section::Face(f) if f.declarations.iter().any(|d| match &d.node {
                FaceDecl::Signal(sig) => sig.name == h, FaceDecl::Tool(t) => t.name == h, _ => false }))));
        for on in &handlers {
            let key = (cell.name.clone(), on.signal_name.clone());
            for (c, h) in &direct[&key].callees {
                if c != &cell.name && skeleton_helper(c, h) {
                    private_calls.push((on.signal_name.clone(), format!("{}.{}", c, h), spans[&key]));
                }
            }
        }
        let public_helpers: Vec<(String, Span)> = if cell.skeleton {
            handlers.iter().filter(|on| !in_face.contains(&on.signal_name) && !on.signal_name.starts_with('_'))
                .map(|on| (on.signal_name.clone(), spans[&(cell.name.clone(), on.signal_name.clone())])).collect()
        } else { Vec::new() };
        out.push(CellSkeleton { cell, parent: *parent, bones, schedules, helpers, unused_helpers, public_helpers, private_calls });
    }
    out
}
