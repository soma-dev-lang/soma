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
use std::rc::Rc;

/// The published bound on explored product states.
pub const LIMIT: usize = 200_000;

struct Machine {
    cell: String,
    name: String,
    states: Vec<String>,
    initial: usize,
    /// the address of the cell that declares it (two cells may share a name)
    owner: usize,
    /// (from, to) → the edge's effect statements
    edges: HashMap<(usize, usize), Rc<Vec<Spanned<Statement>>>>,
}

/// `cur[m]` for each machine, and the required states seen so far (a bit each).
/// (a search is over 12 machines at most: see `attempt`)
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
struct Abs { cur: [u8; 12], seen: u64 }

/// A state in the middle of a handler: the id choices made so far.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Exec { st: Abs, bind: Bind }

/// The id choices of a state: key → "it is the followed id" (or the outcome
/// of a `try`). A key is a text (`e:id`, `l:x:/12:340`, `§link:e:id`…); the
/// states hold its number — a search copies and compares millions of these.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
struct Bind(Vec<(u32, bool)>);

impl Bind {
    fn at(&self, k: u32) -> Result<usize, usize> { self.0.binary_search_by_key(&k, |e| e.0) }
    fn get(&self, k: u32) -> Option<bool> { self.at(k).ok().map(|i| self.0[i].1) }
    fn contains_key(&self, k: u32) -> bool { self.at(k).is_ok() }
    fn insert(&mut self, k: u32, v: bool) { match self.at(k) { Ok(i) => self.0[i].1 = v, Err(i) => self.0.insert(i, (k, v)) } }
    fn remove(&mut self, k: u32) -> Option<bool> { self.at(k).ok().map(|i| self.0.remove(i).1) }
}

thread_local! {
    /// The keys of the search this thread is running: text ↔ number. Emptied
    /// when a search starts and when it ends (`attempt`).
    static KEYS: std::cell::RefCell<(HashMap<Rc<str>, u32>, Vec<Rc<str>>)> = std::cell::RefCell::new(Default::default());
}

/// The number of a key.
fn kid(text: &str) -> u32 {
    KEYS.with(|k| {
        let mut k = k.borrow_mut();
        if let Some(n) = k.0.get(text) { return *n; }
        let (n, t): (u32, Rc<str>) = (k.1.len() as u32, Rc::from(text));
        k.1.push(t.clone());
        k.0.insert(t, n);
        n
    })
}

/// `states` without the keys whose text `gone` names (asked once per key).
fn without(states: Set, gone: impl Fn(&str) -> bool) -> Set {
    let mut known: HashMap<u32, bool> = HashMap::new();
    states.into_iter().map(|mut s| {
        s.bind.0.retain(|(k, _)| !*known.entry(*k).or_insert_with(|| KEYS.with(|t| gone(&t.borrow().1[*k as usize]))));
        s
    }).collect()
}

type Set = BTreeSet<Exec>;

struct Property {
    label: String,
    target: (usize, usize),
    /// bits of `seen`: one of them (any) or each of them (all)
    needs: Vec<u32>,
    all: bool,
    /// `while`: (machine, state) pairs — each machine named must BE in one
    /// of its states when the target is entered. Empty for a `requires`.
    now: Vec<(usize, usize)>,
    /// only the entries into the target from this state (a frame's edge)
    from: Option<usize>,
}

struct Frame<'a> {
    cell: &'a CellDef,
    /// variable → the key of the id it holds
    env: Rc<HashMap<String, String>>,
    uid: Rc<str>,
    depth: usize,
    /// variable → the status it was given (`let s = get_status(id)`, or
    /// through a getter): machine, id key, `??` fallback. Kept only until
    /// the next statement that calls anything.
    status: Rc<HashMap<String, StatusRead>>,
}

type StatusRead = (usize, String, Option<String>);

/// `get_status(id)` or `get_status(id) ?? "state"`: the id, and the fallback.
fn status_read(e: &Expr) -> Option<(&Spanned<Expr>, Option<&str>)> {
    match e {
        Expr::FnCall { name, args } if name == "get_status" && args.len() == 1 => Some((&args[0], None)),
        Expr::FnCall { name, args } if name == "_coalesce" && args.len() == 2 => match (&args[0].node, &args[1].node) {
            (Expr::FnCall { name, args: inner }, Expr::Literal(Literal::String(f))) if name == "get_status" && inner.len() == 1 => Some((&inner[0], Some(f.as_str()))),
            _ => None,
        },
        _ => None,
    }
}

/// Does `st` call anything (a handler, a builtin, a method, an emit)?
fn calls_something(st: &Spanned<Statement>) -> bool {
    let mut hit = false;
    let one = std::slice::from_ref(st);
    super::literals::for_each_expr(one, &mut |e| match e {
        Expr::FnCall { name, .. } if name == "_coalesce" => {}
        Expr::FnCall { .. } | Expr::MethodCall { .. } | Expr::Pipe { .. } => hit = true,
        Expr::Literal(Literal::String(s)) if !super::desugar::segments(s).is_empty() => hit = true,
        _ => {}
    });
    super::literals::for_each_stmt_deep(one, &mut |s| if matches!(s, Statement::Emit { .. } | Statement::MethodCall { .. }) { hit = true; });
    // (a match pattern's string calls too)
    super::literals::for_each_expr(one, &mut |e| if let Expr::Match { arms, .. } = e {
        for arm in arms { let mut texts = Vec::new(); super::desugar::pattern_texts(&arm.pattern, &mut texts); if texts.iter().any(|t| !super::desugar::segments(t).is_empty()) { hit = true; } }
    });
    hit
}

/// The variables assigned somewhere in `stmts`, at any depth.
fn assigned_in(stmts: &[Spanned<Statement>]) -> Vec<String> {
    let mut out = Vec::new();
    super::literals::for_each_stmt_deep(stmts, &mut |s| {
        if let Statement::Assign { name, .. } = s { if !out.contains(name) { out.push(name.clone()); } }
    });
    // …and inside a string's `{…}` (`"{id = other}"`): any `name =` there
    super::literals::for_each_expr(stmts, &mut |e| if let Expr::Literal(Literal::String(t)) = e {
        for seg in super::desugar::segments(t) {
            let b: Vec<char> = seg.chars().collect();
            for i in 0..b.len() {
                if b[i] == '=' && b.get(i + 1) != Some(&'=') && (i == 0 || !matches!(b[i - 1], '=' | '!' | '<' | '>')) {
                    let name: String = b[..i].iter().rev().skip_while(|c| c.is_whitespace()).take_while(|c| c.is_alphanumeric() || **c == '_').collect::<Vec<_>>().into_iter().rev().collect();
                    if !name.is_empty() && !out.contains(&name) { out.push(name); }
                }
            }
        }
    });
    out
}

/// The tuple-variant constructors of the program's types.
fn variant_names(program: &Program) -> HashSet<String> {
    let mut out = HashSet::new();
    for cell in super::names::collect_cells(program) {
        for s in &cell.sections {
            if let Section::Variants(v) = &s.node { for x in &v.variants { out.insert(x.node.name.clone()); } }
        }
    }
    out
}

/// The names a lambda binds: `x`, or the `a`, `b` of `(a, b) => …`.
fn lambda_params(param: &str) -> Vec<String> {
    param.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|p| !p.is_empty()).map(|p| p.to_string()).collect()
}

/// What a block gives: states that go on, states that returned, states that left a loop.
#[derive(Default)]
struct Flow { normal: Set, returned: Set, broke: Set, /// left by `continue`: they start the loop's next turn
    continued: Set }

struct Engine<'a> {
    cells: Vec<&'a CellDef>,
    machines: Vec<Machine>,
    /// owners (cell addresses) of the machines left out of this search (the
    /// cone of influence): their status is unknown, their transitions succeed
    sliced: HashSet<usize>,
    /// in a cone: the handlers ("Cell.handler") that may move a kept machine;
    /// a call of any other is skipped (it cannot change the state followed)
    prune: Option<HashSet<String>>,
    /// (machine, state) → bit of `seen`
    watched: HashMap<(usize, usize), u32>,
    props: Vec<Property>,
    /// property index → the entry that broke it (filled once)
    broken: BTreeMap<usize, String>,
    unsupported: Option<String>,
    /// (handler names, cell names) of the program: what a string's `{…}` is read with
    names: (HashSet<String>, HashSet<String>),
    /// `via`: the slot (cell, slot) an instance names the other instance in,
    /// and the machines that follow that other instance
    link: Option<(String, String)>,
    secondary: HashSet<usize>,
    /// edges taken so far by the search, and that count where the condition
    /// under `narrow` began
    moves: usize,
    /// `std.f(…)` under evaluation: f is the builtin, whatever handler bears the name
    forced: bool,
    /// tuple-variant constructors (`Opener(id)` builds a value, calls nothing)
    variants: HashSet<String>,
    /// addresses of the top-level cells (an `emit` from one of them is not
    /// delivered inside another cell's interior)
    top: HashSet<usize>,
    /// interior cell → its parent (by address)
    parent_of: HashMap<usize, usize>,
    /// states that left the handler (`return`) or a loop (`break`) from
    /// inside an expression — a `match` arm, an `if` expression: collected
    /// by the statement that holds the expression
    returned: Set,
    broke: Set,
    continued: Set,
    /// What the search asks at every statement it passes, answered once:
    /// the cell of a name, a cell's handler, the first handler of a name in
    /// the program, a cell's slots, what a statement calls and assigns, the
    /// expressions of a string's `{…}`. (Asked of the whole program each
    /// time, a search of Orbite spent most of its time looking names up.)
    cell_ix: HashMap<&'a str, &'a CellDef>,
    handler_ix: HashMap<usize, HashMap<&'a str, &'a OnSection>>,
    first_ix: HashMap<&'a str, (&'a CellDef, &'a OnSection)>,
    slots_ix: HashMap<usize, HashSet<String>>,
    facts: HashMap<usize, Rc<StmtFacts>>,
    segs: HashMap<(usize, usize, String), Rc<Vec<Spanned<Expr>>>>,
}

/// What `block` needs of a statement, whatever state it is run from.
struct StmtFacts {
    head_calls: bool, calls: bool, assigned: Vec<String>,
    /// the variables whose `.error` the statement reads, at any depth
    /// (`None`: it may read any — inside a string's `{…}`)
    errors: Option<Vec<String>>,
}

fn key_text(s: &str) -> String { format!("lit:{}", s) }

/// The names a text reads `.error` of (`"… {r.error}"`): added to `out`,
/// which becomes `None` — any name — where what precedes `.error` is not a name.
fn error_names(text: &str, out: &mut Option<Vec<String>>) {
    for (at, _) in text.match_indices(".error") {
        let name: String = text[..at].chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<Vec<_>>().into_iter().rev().collect();
        match out.as_mut() {
            Some(names) if !name.is_empty() => names.push(name),
            _ => { *out = None; return; }
        }
    }
}

/// The mark of a state that stands for both outcomes of a `try` (see `block`).
fn either_key(try_key: &str) -> String { format!("either:{}", try_key) }

/// The `ok:` keys `cond` reads: `p.error == ()` / `!= ()` of a variable of `frame`.
fn tries_read(cond: &Expr, frame: &Frame, out: &mut Vec<String>) {
    match cond {
        Expr::Not(i) => tries_read(&i.node, frame, out),
        Expr::BinaryOp { left, right, .. } => { tries_read(&left.node, frame, out); tries_read(&right.node, frame, out); }
        Expr::CmpOp { left, right, .. } => {
            if let (Expr::FieldAccess { target, field }, Expr::Literal(_)) = (&left.node, &right.node) {
                if let (true, Expr::Ident(v)) = (field == "error", &target.node) {
                    if let Some(k) = frame.env.get(v).filter(|k| k.starts_with("l:")) { out.push(format!("ok:{}", k)); }
                }
            }
            tries_read(&left.node, frame, out);
        }
        _ => {}
    }
}

/// Two states that differ only in the outcome of one `try` are one state
/// marked "either" again: what `if p.error != () { … }` told apart and both
/// branches left alike.
fn rejoin(states: &mut Set, key: &str) {
    let (key, either) = (kid(key), kid(&either_key(key)));
    let pairs: Vec<Exec> = states.iter().filter(|x| x.bind.get(key) == Some(true)).filter(|x| {
        let mut twin = (*x).clone(); twin.bind.insert(key, false); states.contains(&twin)
    }).cloned().collect();
    for mut x in pairs {
        states.remove(&x);
        x.bind.insert(key, false); states.remove(&x);
        x.bind.remove(key); x.bind.insert(either, true); states.insert(x);
    }
}

impl<'a> Engine<'a> {
    fn index(&mut self) {
        for c in self.cells.iter().copied() {
            self.cell_ix.entry(c.name.as_str()).or_insert(c);
            let own = self.handler_ix.entry(c as *const CellDef as usize).or_default();
            for s in &c.sections {
                if let Section::OnSignal(on) = &s.node {
                    own.entry(on.signal_name.as_str()).or_insert(on);
                    self.first_ix.entry(on.signal_name.as_str()).or_insert((c, on));
                }
            }
        }
    }

    fn cell(&self, name: &str) -> Option<&'a CellDef> { self.cell_ix.get(name).copied() }

    fn handler(&self, cell: &'a CellDef, name: &str) -> Option<&'a OnSection> {
        self.handler_ix.get(&(cell as *const CellDef as usize)).and_then(|h| h.get(name).copied())
    }

    /// The first handler of that name, in the program's order of cells.
    fn first(&self, name: &str) -> Option<(&'a CellDef, &'a OnSection)> { self.first_ix.get(name).copied() }

    fn is_slot(&mut self, cell: &CellDef, name: &str) -> bool {
        self.slots_ix.entry(cell as *const CellDef as usize).or_insert_with(|| super::frame::slots_of(cell)).contains(name)
    }

    /// The expressions of a string's `{…}`, parsed once (they are kept: what
    /// the search remembers of a statement is remembered by its address).
    fn seg_exprs(&mut self, text: &str, cell: &CellDef, span: Span) -> Rc<Vec<Spanned<Expr>>> {
        let key = (cell as *const CellDef as usize, span.start, text.to_string());
        if let Some(found) = self.segs.get(&key) { return found.clone(); }
        let parsed = Rc::new(super::desugar::segment_exprs(text, &self.names, cell).into_iter().map(|x| Spanned::new(x, span)).collect::<Vec<_>>());
        self.segs.insert(key, parsed.clone());
        parsed
    }

    fn stmt_facts(&mut self, st: &Spanned<Statement>) -> Rc<StmtFacts> {
        let at = st as *const Spanned<Statement> as usize;
        if let Some(f) = self.facts.get(&at) { return f.clone(); }
        // a status held in a variable is not the status once anything is called:
        // also WITHIN the statement that reads it (`if close(id) == "done" && s == "open"`)
        let head_calls = match &st.node {
            Statement::If { condition: h, .. } | Statement::While { condition: h, .. } | Statement::For { iter: h, .. } =>
                calls_something(&Spanned::new(Statement::ExprStmt { expr: h.clone() }, st.span)),
            _ => calls_something(st),
        };
        let one = std::slice::from_ref(st);
        let mut errors = Some(Vec::new());
        super::literals::for_each_expr(one, &mut |e| match e {
            Expr::FieldAccess { target, field } if field == "error" => if let (Expr::Ident(v), Some(out)) = (&target.node, errors.as_mut()) { out.push(v.clone()); },
            Expr::Literal(Literal::String(t)) if t.contains(".error") => error_names(t, &mut errors),
            Expr::Match { arms, .. } => for arm in arms {
                let mut texts = Vec::new();
                super::desugar::pattern_texts(&arm.pattern, &mut texts);
                for t in &texts { error_names(t, &mut errors); }
            },
            _ => {}
        });
        let f = Rc::new(StmtFacts { head_calls, calls: calls_something(st), assigned: assigned_in(one), errors });
        self.facts.insert(at, f.clone());
        f
    }

    /// The machine `transition()` / `get_status()` reach from `cell`: its
    /// own, or the program's only one.
    fn machine_of(&self, cell: &CellDef) -> Option<usize> {
        // by identity: an interior cell may bear the name of a top-level one
        let owner = cell as *const CellDef as usize;
        if self.sliced.contains(&owner) { return None; }
        // (the program's only machine — not the only one left by the cone)
        self.machines.iter().position(|m| m.owner == owner)
            .or_else(|| (self.machines.len() == 1 && self.sliced.is_empty()).then_some(0))
    }

    /// `branch`, unless evaluating the condition moved a machine: a status
    /// it read (or a variable holding one) was read BEFORE or AFTER that
    /// move, and the states at hand are those after it — nothing is
    /// narrowed then.
    fn narrow(&self, cond: &Expr, frame: &Frame<'a>, before: &BTreeSet<Abs>, after: Set, moves_before: usize) -> (Set, Set) {
        let now: BTreeSet<Abs> = after.iter().map(|x| x.st.clone()).collect();
        // (the count of edges taken, too: a round trip leaves the SET of states as it was)
        if *before != now || moves_before != self.moves { return (after.clone(), after); }
        self.branch(cond, frame, after)
    }

    fn key_of(&self, e: &Expr, at: Span, frame: &Frame) -> String {
        match e {
            Expr::Ident(v) => frame.env.get(v).cloned().unwrap_or_else(|| format!("g:{}:{}", frame.cell.name, v)),
            // (`"{x}"` is computed: its text is not its value)
            Expr::Literal(Literal::String(s)) if super::desugar::segments(s).is_empty() => key_text(s),
            Expr::Literal(Literal::Int(n)) => key_text(&n.to_string()),
            // the link slot read in its cell: the instance that entry names
            Expr::MethodCall { target, method, args } if method == "get" && args.len() == 1 && self.is_link(&target.node, frame) =>
                format!("link:{}", self.key_of(&args[0].node, args[0].span, frame)),
            Expr::Index { target, index } if self.is_link(&target.node, frame) => format!("link:{}", self.key_of(&index.node, index.span, frame)),
            // `desk_de.get(id) ?? ""`: the same instance — the default stands
            // where the plain read gives `()`, for an id with no link either way
            Expr::FnCall { name, args } if name == "_coalesce" && args.len() == 2 && matches!(args[1].node, Expr::Literal(_))
                && matches!(&args[0].node, Expr::MethodCall { target, method, args: a } if method == "get" && a.len() == 1 && self.is_link(&target.node, frame)) =>
                self.key_of(&args[0].node, args[0].span, frame),
            // a computed id: its own value at each place it is written
            _ => format!("x:{}:{}", frame.uid, at.start),
        }
    }

    /// The status `e` reads, when it is one: `get_status(id)` (with or
    /// without `?? "state"`) in this cell, the same through a handler whose
    /// body is that one `return` (`Approval.status(id)`), or a variable just
    /// given one of these.
    fn status_of(&self, e: &Expr, frame: &Frame<'a>) -> Option<StatusRead> {
        if let Expr::Ident(v) = e { return frame.status.get(v).cloned(); }
        if let Some((id, fallback)) = status_read(e) {
            let m = self.machine_of(frame.cell)?;
            return Some((m, self.key_of(&id.node, id.span, frame), fallback.map(|f| f.to_string())));
        }
        if frame.depth >= 12 { return None; }
        let (cell, on, args): (&'a CellDef, &'a OnSection, &[Spanned<Expr>]) = match e {
            Expr::MethodCall { target, method, args } => match &target.node {
                Expr::Ident(c) if !frame.env.contains_key(c) => { let cell = self.cell(c)?; (cell, self.handler(cell, method)?, args) }
                _ => return None,
            },
            Expr::FnCall { name, args } => match self.handler(frame.cell, name) {
                Some(on) => (frame.cell, on, args),
                None if !super::names::builtin_names().contains(name.as_str()) => {
                    let (c, on) = self.first(name)?;
                    (c, on, args)
                }
                None => return None,
            },
            _ => return None,
        };
        if !crate::ast::accepted_arities(&on.params).contains(&args.len()) { return None; }
        let [only] = on.body.as_slice() else { return None };
        let Statement::Return { value } = &only.node else { return None };
        let mut env = HashMap::new();
        for (i, p) in on.params.iter().enumerate() {
            let key = match args.get(i) { Some(a) => self.key_of(&a.node, a.span, frame), None => format!("x:{}:g{}p{}", frame.uid, only.span.start, i) };
            env.insert(p.name.clone(), key);
        }
        let inner = Frame { cell, env: Rc::new(env), uid: format!("{}/g{}", frame.uid, only.span.start).into(), depth: frame.depth + 1, status: Rc::default() };
        self.status_of(&value.node, &inner)
    }

    /// A lambda that may move a machine (it calls a handler, `transition`, an
    /// effect builtin, or emits), written where it does not run: bound to a
    /// name, returned, stored in a list or a record, handed to a handler.
    /// When it runs is then not known here, and the search says so instead
    /// of reading its body where it stands.
    fn kept_lambda(&mut self, e: &Expr) {
        self.scan_lambdas(e, false);
    }

    /// `direct`: `e` is the argument a builtin applies before it returns
    /// (`xs.map(HERE)`), the one place a lambda's body runs where it stands.
    fn scan_lambdas(&mut self, e: &Expr, direct: bool) {
        const RUNS: &[&str] = &["map", "filter", "find", "any", "all", "count", "reduce", "sort_by", "group_by", "min_by", "max_by", "sum_by"];
        match e {
            Expr::Lambda { .. } | Expr::LambdaBlock { .. } => {
                if !direct {
                    let mut moves = false;
                    super::literals::for_each_in_expr(e, &mut |x| match x {
                        Expr::FnCall { name, .. } if name == "_coalesce" => {}
                        Expr::FnCall { name, .. } if name == "transition" || name == "_std" || !super::names::builtin_names().contains(name.as_str())
                            || super::names::EFFECT_BUILTINS.contains(&name.as_str()) || self.names.0.contains(name) => moves = true,
                        // (a string's `{…}` may call anything)
                        Expr::Literal(Literal::String(t)) if !super::desugar::segments(t).is_empty() => moves = true,
                        Expr::MethodCall { method, .. } if self.names.0.contains(method) => moves = true,
                        Expr::MethodCall { target, method, .. } if method == "transition" || matches!(&target.node, Expr::Ident(c) if self.cell(c).is_some()) => moves = true,
                        _ => {}
                    });
                    super::literals::for_each_stmt_in_expr(e, &mut |st| if matches!(st, Statement::Emit { .. } | Statement::MethodCall { .. }) { moves = true; });
                    if moves {
                        self.unsupported.get_or_insert("a lambda that calls a handler or moves a machine is kept for later (bound to a name, returned, stored, or handed to a handler) — when it runs is not known: write it where it runs (`xs.map(x => …)`)".to_string());
                    }
                }
                match e {
                    Expr::Lambda { body, .. } => self.scan_lambdas(&body.node, false),
                    Expr::LambdaBlock { result, .. } => self.scan_lambdas(&result.node, false),
                    _ => {}
                }
            }
            Expr::FnCall { name, args } => {
                let handler = self.first(name).is_some();
                // (`map("f", z => …)` builds a Map: `map(xs, f)` runs f only over a visible list)
                let a_list = matches!(args.first().map(|a| &a.node), Some(Expr::ListLiteral(_)))
                    || matches!(args.first().map(|a| &a.node), Some(Expr::FnCall { name, .. }) if name == "list" || name == "range");
                let runs = RUNS.contains(&name.as_str()) && !handler && (name != "map" || (args.len() == 2 && a_list));
                for (i, a) in args.iter().enumerate() { self.scan_lambdas(&a.node, runs && i + 1 == args.len()); }
            }
            Expr::MethodCall { target, method, args } => {
                self.scan_lambdas(&target.node, false);
                let to_cell = matches!(&target.node, Expr::Ident(c) if self.cell(c).is_some());
                let runs = RUNS.contains(&method.as_str()) && !to_cell;
                for (i, a) in args.iter().enumerate() { self.scan_lambdas(&a.node, runs && i + 1 == args.len()); }
            }
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => { self.scan_lambdas(&left.node, false); self.scan_lambdas(&right.node, false); }
            Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => self.scan_lambdas(&i.node, false),
            Expr::FieldAccess { target, .. } => self.scan_lambdas(&target.node, false),
            Expr::Index { target, index } => { self.scan_lambdas(&target.node, false); self.scan_lambdas(&index.node, false); }
            Expr::ListLiteral(items) => for it in items { self.scan_lambdas(&it.node, false); },
            Expr::Record { fields, .. } => for (_, v) in fields { self.scan_lambdas(&v.node, false); },
            Expr::Match { subject, arms } => {
                self.scan_lambdas(&subject.node, false);
                for arm in arms {
                    if let Some(g) = &arm.guard { self.scan_lambdas(&g.node, false); }
                    self.scan_lambdas(&arm.result.node, false);
                }
            }
            Expr::IfExpr { condition, then_result, else_result, .. } => {
                self.scan_lambdas(&condition.node, false);
                self.scan_lambdas(&then_result.node, false);
                self.scan_lambdas(&else_result.node, false);
            }
            _ => {}
        }
    }

    /// `split`, for machine `m`: with a link, the machines that follow the
    /// instance the target's instance names answer under their own binding
    /// (`§key`), and `link:k` IS that instance when `k` is the followed one.
    fn split_on(&self, states: &Set, key: &str, m: usize) -> (Set, Set) {
        if !self.secondary.contains(&m) { return self.split(states, key); }
        let skey = kid(&format!("§{}", key));
        let names = key.strip_prefix("link:").map(kid);
        let (mut yes, mut no) = (Set::new(), Set::new());
        for s in states {
            if let Some(k) = names {
                match s.bind.get(k) {
                    // the followed instance names the followed other: it IS it
                    Some(true) => { yes.insert(s.clone()); continue; }
                    Some(false) => {}
                    // undecided: decide `k` too, or a later "`k` is the followed one" would
                    // contradict a "this is not the instance it names" made here
                    None => {
                        let mut a = s.clone(); a.bind.insert(k, true); yes.insert(a);
                        let mut rest = s.clone(); rest.bind.insert(k, false);
                        match rest.bind.get(skey) {
                            Some(true) => { yes.insert(rest); }
                            Some(false) => { no.insert(rest); }
                            None => {
                                let mut b = rest.clone(); b.bind.insert(skey, true); yes.insert(b);
                                let mut c = rest; c.bind.insert(skey, false); no.insert(c);
                            }
                        }
                        continue;
                    }
                }
            }
            match s.bind.get(skey) {
                Some(true) => { yes.insert(s.clone()); }
                Some(false) => { no.insert(s.clone()); }
                None => {
                    let mut a = s.clone(); a.bind.insert(skey, true); yes.insert(a);
                    let mut b = s.clone(); b.bind.insert(skey, false); no.insert(b);
                }
            }
        }
        (yes, no)
    }

    /// `slot.get(k)` / `slot[k]` of the link slot, read in its cell.
    fn is_link(&self, target: &Expr, frame: &Frame) -> bool {
        matches!((&self.link, target), (Some((c, sl)), Expr::Ident(t)) if t == sl && frame.cell.name == *c && !frame.env.contains_key(t))
    }

    /// Split `states` on whether `key` is the followed id.
    fn split(&self, states: &Set, key: &str) -> (Set, Set) {
        let (mut yes, mut no) = (Set::new(), Set::new());
        // (with a link: "the instance `key` names is not the followed other"
        // was decided — `key` cannot be the followed one then)
        let named = kid(&format!("§link:{}", key));
        let key = kid(key);
        for s in states {
            if !s.bind.contains_key(key) && s.bind.get(named) == Some(false) {
                let mut b = s.clone(); b.bind.insert(key, false); no.insert(b);
                continue;
            }
            match s.bind.get(key) {
                Some(true) => { yes.insert(s.clone()); }
                Some(false) => { no.insert(s.clone()); }
                None => {
                    let mut a = s.clone(); a.bind.insert(key, true); yes.insert(a);
                    let mut b = s.clone(); b.bind.insert(key, false); no.insert(b);
                }
            }
        }
        (yes, no)
    }

    /// `transition(id, target)` from `frame`'s cell.
    fn transition(&mut self, states: Set, id: &Spanned<Expr>, source: Option<&Expr>, target: &Expr, frame: &Frame<'a>, entry: &str) -> Set {
        let Some(m) = self.machine_of(frame.cell) else { return states };
        let key = self.key_of(&id.node, id.span, frame);
        let (hit, mut out) = self.split_on(&states, &key, m);
        let named = |e: &Expr| match e {
            Expr::Literal(Literal::String(s)) if super::desugar::segments(s).is_empty() => Some(s.clone()),
            // (a bare name is a state when it IS one: any other is a variable —
            // also where the environment is not known, after an `await`)
            Expr::Ident(v) if !frame.env.contains_key(v) && self.machines[m].states.iter().any(|s| s == v) => Some(v.clone()),
            _ => None,
        };
        let to: Option<usize> = named(target).and_then(|t| self.machines[m].states.iter().position(|s| *s == t)
            .or_else(|| self.machines[m].states.iter().position(|s| s.eq_ignore_ascii_case(&t))));
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
                let Some(effect) = self.machines[m].edges.get(&(cur, t)).cloned() else { continue };  // (shared, not copied)
                self.moves += 1;
                let mut next = s.clone();
                next.st.cur[m] = t as u8;
                if let Some(bit) = self.watched.get(&(m, t)) { next.st.seen |= 1 << bit; }
                for (i, p) in self.props.iter().enumerate() {
                    if p.target != (m, t) || self.broken.contains_key(&i) || p.from.is_some_and(|f| f != cur) { continue; }
                    let met = if !p.now.is_empty() {
                        p.now.iter().all(|(om, _)| p.now.iter().any(|(mm, st)| mm == om && next.st.cur[*mm] as usize == *st))
                    } else if p.all { p.needs.iter().all(|b| next.st.seen & (1 << b) != 0) } else { p.needs.iter().any(|b| next.st.seen & (1 << b) != 0) };
                    if !met { self.broken.insert(i, entry.to_string()); }
                }
                // the edge's effect runs with the transition
                let mut after = Set::new();
                after.insert(next);
                let flow = self.block(&effect, frame, after, entry);
                out.extend(flow.normal); out.extend(flow.returned); out.extend(flow.broke); out.extend(flow.continued);
            }
        }
        // (a computed id is read once: its choice is not kept)
        if key.starts_with("x:") { let (k, sk) = (kid(&key), kid(&format!("§{}", key))); out = out.into_iter().map(|mut s| { s.bind.remove(k); s.bind.remove(sk); s }).collect(); }
        out
    }

    fn call(&mut self, cell: &'a CellDef, on: &'a OnSection, args: &[Spanned<Expr>], frame: &Frame<'a>, at: Span, states: Set, entry: &str) -> Set {
        // a handler that cannot move a machine of this search leaves the state
        // followed as it was, whether it succeeds or fails
        if let Some(p) = &self.prune { if !p.contains(&format!("{}.{}", cell.name, on.signal_name)) { return states; } }
        if frame.depth >= 12 {
            self.unsupported.get_or_insert(format!("handler calls nest deeper than 12 (through {}.{}) — recursion is not explored", cell.name, on.signal_name));
            return states;
        }
        for a in args { self.kept_lambda(&a.node); }
        // a `[task]` handler that awaits hands `{waiting}` back to its caller at
        // the wait: the caller goes on while the callee has done only its first
        // step — not "the call returns when the callee is done"
        if on.properties.iter().any(|p| p == "task") {
            let mut waits = false;
            super::literals::for_each_expr(&on.body, &mut |e| if matches!(e, Expr::FnCall { name, .. } if name.starts_with("await_")) { waits = true; });
            if waits { self.unsupported.get_or_insert(format!("`{}.{}` is a `[task]` that awaits, called by a handler: the caller goes on from the wait, which this search does not follow", cell.name, on.signal_name)); }
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
        let inner = Frame { cell, env: Rc::new(env), uid: uid.as_str().into(), depth: frame.depth + 1, status: Rc::default() };
        let flow = self.block(&on.body, &inner, states, entry);
        let mut out = flow.normal;
        out.extend(flow.returned);
        out.extend(flow.broke);
        out.extend(flow.continued);
        // the callee's own choices end with it
        let local = format!(":{}", uid);
        // (…and the outcomes of its `try`s: `ok:l:…`, read only through its own variables)
        without(out, |k| { let k = k.trim_start_matches('§').trim_start_matches("either:").trim_start_matches("ok:"); (k.starts_with("l:") || k.starts_with("x:")) && k.contains(&local) })
    }

    /// `frame` with `names` bound to values nothing is known of (a loop
    /// variable, a match binder, a lambda parameter, a variable a nested
    /// block assigns): each gets a key of its own, returned with the frame.
    fn rebound(&self, frame: &Frame<'a>, names: impl IntoIterator<Item = String>, at: usize, tag: &str) -> (Frame<'a>, Vec<String>) {
        let mut inner = Frame { cell: frame.cell, env: frame.env.clone(), uid: frame.uid.clone(), depth: frame.depth, status: Rc::default() };
        let mut keys = Vec::new();
        for n in names {
            let key = format!("l:{}:{}:{}:{}", n, frame.uid, at, tag);
            Rc::make_mut(&mut inner.env).insert(n, key.clone());
            keys.push(key);
        }
        (inner, keys)
    }

    /// The states with the choices made for `keys` forgotten (and those for
    /// the computed ids of `frame`): what a new turn of a loop starts from —
    /// its variable, and whatever the body assigns, hold other values then.
    fn forget(states: Set, keys: &[String], frame: &Frame) -> Set {
        let computed = format!("x:{}:", frame.uid);
        without(states, |k| { let k = k.trim_start_matches('§'); keys.iter().any(|x| x == k) || k.starts_with(&computed) })
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
            // the calls a string makes in its `{…}` run where the string stands
            Expr::Literal(Literal::String(text)) if !super::desugar::segments(text).is_empty() => {
                let mut s = states;
                for x in self.seg_exprs(text, frame.cell, e.span).iter() { s = self.expr(x, frame, s, entry); }
                s
            }
            Expr::Literal(_) | Expr::Ident(_) => states,
            // `a && b`, `a || b`, `a ?? b`: `b` runs or not
            Expr::BinaryOp { left, op: BinOp::And | BinOp::Or, right } => {
                let s = self.expr(left, frame, states, entry);
                let mut out = self.expr(right, frame, s.clone(), entry);
                out.extend(s);
                out
            }
            Expr::FnCall { name, args } => self.fn_call(name, args, e.span, frame, states, entry),
            Expr::FieldAccess { target, .. } | Expr::Not(target) | Expr::TryPropagate(target) => self.expr(target, frame, states, entry),
            // (the index, then what is indexed: the interpreter's order)
            Expr::Index { target, index } => self.exprs(&[index, target], frame, states, entry),
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
            // (its parameter is a value of its own at each call, whatever the name)
            Expr::Lambda { param, body } => {
                let (inner, keys) = self.rebound(frame, lambda_params(param), e.span.start, "f");
                self.repeat(states, |me, s| me.expr(body, &inner, Self::forget(s, &keys, &inner), entry))
            }
            Expr::LambdaBlock { param, stmts, result } => {
                let mut names = lambda_params(param);
                names.extend(assigned_in(stmts));
                let (inner, keys) = self.rebound(frame, names, e.span.start, "f");
                self.repeat(states, |me, s| {
                    let flow = me.block(stmts, &inner, Self::forget(s, &keys, &inner), entry);
                    let mut out = me.expr(result, &inner, flow.normal, entry);
                    out.extend(flow.returned); out.extend(flow.broke); out.extend(flow.continued);
                    out
                })
            }
            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                let before: BTreeSet<Abs> = states.iter().map(|x| x.st.clone()).collect();
                let moves_before = self.moves;
                let s = self.expr(condition, frame, states, entry);
                let (yes, no) = self.narrow(&condition.node, frame, &before, s, moves_before);
                let a = self.block(then_body, frame, yes, entry);
                let mut out = self.expr(then_result, frame, a.normal, entry);
                let b = self.block(else_body, frame, no, entry);
                out.extend(self.expr(else_result, frame, b.normal, entry));
                // a `return` in a branch ends the handler there
                self.returned.extend(a.returned); self.returned.extend(b.returned);
                self.broke.extend(a.broke); self.broke.extend(b.broke);
                self.continued.extend(a.continued); self.continued.extend(b.continued);
                out
            }
            Expr::Match { subject, arms } => {
                let s = self.expr(subject, frame, states, entry);
                let mut out = Set::new();
                // (an arm that is tried and not taken has run its pattern and its
                // guard all the same: the next arm starts from there)
                let mut carried = s.clone();
                for (i, arm) in arms.iter().enumerate() {
                    let s = carried.clone();
                    // what the pattern binds is the matched value, not the
                    // id (or the state) whose name it takes
                    let mut binders = HashSet::new();
                    super::invariants::pattern_binders(&arm.pattern, &mut binders);
                    let (inner, keys) = self.rebound(frame, binders, e.span.start, &format!("m{}", i));
                    let frame = &inner;
                    let mut t = Self::forget(s.clone(), &keys, frame);
                    // (a pattern's string is built when the arm is tried)
                    let mut texts = Vec::new();
                    super::desugar::pattern_texts(&arm.pattern, &mut texts);
                    for text in &texts {
                        for x in self.seg_exprs(text, frame.cell, e.span).iter() { t = self.expr(x, frame, t, entry); }
                    }
                    if let Some(g) = &arm.guard { t = self.expr(g, frame, t, entry); }
                    carried.extend(Self::forget(t.clone(), &keys, frame));
                    let flow = self.block(&arm.body, frame, t, entry);
                    out.extend(self.expr(&arm.result, frame, flow.normal, entry));
                    self.returned.extend(flow.returned); self.broke.extend(flow.broke); self.continued.extend(flow.continued);
                }
                out
            }
            Expr::MethodCall { target, method, args } => {
                // `Cell.handler(args)`
                if let Expr::Ident(c) = &target.node {
                    // (a slot of that name is the slot: the interpreter looks it up first)
                    if !frame.env.contains_key(c) && !self.is_slot(frame.cell, c) {
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
                // (as the interpreter does: the arguments, then the receiver)
                let is_lambda = |a: &Spanned<Expr>| matches!(a.node, Expr::Lambda { .. } | Expr::LambdaBlock { .. });
                let mut s = states;
                for a in args.iter().filter(|a| !is_lambda(a)) { s = self.expr(a, frame, s, entry); }
                s = self.expr(target, frame, s, entry);
                // (a lambda handed to the method runs when the method does: after the receiver)
                for a in args.iter().filter(|a| is_lambda(a)) { s = self.expr(a, frame, s, entry); }
                // `id.transition("paid")`
                if method == "transition" && !args.is_empty() {
                    let (source, to) = if args.len() >= 2 { (Some(&args[0].node), &args[1].node) } else { (None, &args[0].node) };
                    return self.transition(s, target, source, to, frame, entry);
                }
                // `x.h(a)` is `h(x, a)` when a handler is named h — also when `x`
                // is a local that bears a cell's name (`let Aux = id  Aux.closer()`)
                let ufcs = self.handler(frame.cell, method).filter(|on| crate::ast::accepted_arities(&on.params).contains(&(args.len() + 1))).map(|on| (frame.cell, on))
                    .or_else(|| if super::names::builtin_names().contains(method.as_str()) { None } else {
                        self.first(method) });
                let mut out = match ufcs {
                    Some((cell, on)) => {
                        let mut all: Vec<Spanned<Expr>> = vec![(**target).clone()];
                        all.extend(args.iter().cloned());
                        self.call(cell, on, &all, frame, e.span, s.clone(), entry)
                    }
                    None => s.clone(),
                };
                // …and, the local being perhaps not bound on this path, the cell's own handler
                if let Expr::Ident(c) = &target.node {
                    if let Some((cell, on)) = self.cell(c).and_then(|cell| self.handler(cell, method).map(|on| (cell, on))) {
                        out.extend(self.call(cell, on, args, frame, e.span, s, entry));
                    }
                }
                out
            }
        }
    }

    /// `name(args…)`, written at `span`.
    fn fn_call(&mut self, name: &str, args: &[Spanned<Expr>], span: Span, frame: &Frame<'a>, states: Set, entry: &str) -> Set {
        match name {
            "_coalesce" if args.len() == 2 => {
                let s = self.expr(&args[0], frame, states, entry);
                let mut out = self.expr(&args[1], frame, s.clone(), entry);
                out.extend(s);
                out
            }
            // `std.f(a…)`: the builtin f
            "_std" if matches!(args.first().map(|a| &a.node), Some(Expr::Literal(Literal::String(_)))) => {
                let Some(Expr::Literal(Literal::String(f))) = args.first().map(|a| &a.node) else { unreachable!() };
                let was = std::mem::replace(&mut self.forced, true);
                let out = self.fn_call(f, &args[1..], span, frame, states, entry);
                self.forced = was;
                out
            }
            // `Opener(id)`: a value of a declared type, whatever handler is named `Opener`
            _ if self.variants.contains(name) => {
                let mut s = states;
                for a in args { s = self.expr(a, frame, s, entry); }
                s
            }
            _ => {
                let mut s = states;
                let forced = std::mem::replace(&mut self.forced, false);
                for a in args { s = self.expr(a, frame, s, entry); }
                // this cell's own handler of that name and arity is what runs — also
                // for `delegate`, `vote`… (not under `std.`)
                let own = if forced { None } else { self.handler(frame.cell, name).filter(|on| crate::ast::accepted_arities(&on.params).contains(&args.len())) };
                if let Some(on) = own.filter(|_| name != "transition") { return self.call(frame.cell, on, args, frame, span, s, entry); }
                match name {
                    "transition" if args.len() >= 2 => {
                        let (source, to) = if args.len() >= 3 { (Some(&args[1].node), &args[2].node) } else { (None, &args[1].node) };
                        return self.transition(s, &args[0], source, to, frame, entry);
                    }
                    // `deliver()` / `resume()` run the rest of a waiting `[task]` handler,
                    // `horde()` runs handlers in tasks and calls back: not followed
                    "deliver" | "resume" | "horde" => {
                        self.unsupported.get_or_insert(format!("{}() runs a handler (the rest of a waiting task, a horde's target and callbacks) that this search does not follow", name));
                        return s;
                    }
                    // `vote(Cell.handler, …)` runs that handler, any number of times
                    "vote" if !args.is_empty() => {
                        let target = match &args[0].node {
                            Expr::Literal(Literal::String(t)) if super::desugar::segments(t).is_empty() => match t.split_once('.') {
                                Some((c, h)) => self.cell(c).and_then(|cell| self.handler(cell, h).map(|on| (cell, on))),
                                None => self.handler(frame.cell, t).map(|on| (frame.cell, on)),
                            },
                            // (`Aux.nop` on a LOCAL `Aux` is a value read, a handler named by it)
                            Expr::FieldAccess { target, field } => match &target.node {
                                Expr::Ident(c) if !frame.env.contains_key(c) => self.cell(c).and_then(|cell| self.handler(cell, field).map(|on| (cell, on))),
                                _ => None,
                            },
                            Expr::Ident(h) if !frame.env.contains_key(h) => self.handler(frame.cell, h).map(|on| (frame.cell, on))
                                .or_else(|| self.first(h)),
                            _ => None,
                        };
                        return match target {
                            Some((cell, on)) => self.repeat(s, |me, st| me.call(cell, on, &[], frame, span, st, entry)),
                            None => { self.unsupported.get_or_insert("vote() over a handler this search cannot name".to_string()); s }
                        };
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
                            for on in &tools { out.extend(me.call(cell, on, &[], frame, span, st.clone(), entry)); }
                            out
                        });
                    }
                    "delegate" => {
                        return match (args.first().map(|a| &a.node), args.get(1).map(|a| &a.node)) {
                            (Some(Expr::Literal(Literal::String(c))), Some(Expr::Literal(Literal::String(h)))) => {
                                match self.cell(c).and_then(|cell| self.handler(cell, h).map(|on| (cell, on))) {
                                    Some((cell, on)) => self.call(cell, on, &args[2..], frame, span, s, entry),
                                    None => s,
                                }
                            }
                            _ => { self.unsupported.get_or_insert("delegate() with a computed cell or handler: what it runs is not known".to_string()); s }
                        };
                    }
                    _ => {}
                }
                // a handler: this cell's own first, then the program's
                if !forced && !super::names::builtin_names().contains(name) {
                    let found = self.first(name);
                    if let Some((cell, on)) = found { return self.call(cell, on, args, frame, span, s, entry); }
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
            // a hole's model: the instance `id` of a cell's machine is in one of these states
            Expr::FnCall { name, args } if name == super::flesh::STATUS_IN && args.len() >= 2 => {
                let wanted: Vec<(String, String)> = args[1..].iter().filter_map(|a| match &a.node {
                    Expr::Literal(Literal::String(t)) => t.split_once('.').map(|(c, s)| (c.to_string(), s.to_string())),
                    _ => None,
                }).collect();
                let Some(m) = wanted.first().and_then(|(c, _)| self.machines.iter().position(|x| &x.cell == c)) else { return (states.clone(), states) };
                let ok: Vec<usize> = wanted.iter().filter_map(|(_, s)| self.machines[m].states.iter().position(|x| x == s)).collect();
                let key = self.key_of(&args[0].node, args[0].span, frame);
                let (hit, other) = self.split_on(&states, &key, m);
                let (mut yes, mut no) = (other.clone(), other);
                for s in hit { if ok.contains(&(s.st.cur[m] as usize)) { yes.insert(s); } else { no.insert(s); } }
                (yes, no)
            }
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
            // `p.error == ()` / `p.error != ()` of a `let p = try { … }`: whether it succeeded
            Expr::CmpOp { left, op: op @ (CmpOp::Eq | CmpOp::Ne), right }
                if matches!(&right.node, Expr::Literal(Literal::Unit))
                && matches!(&left.node, Expr::FieldAccess { target, field } if field == "error" && matches!(&target.node, Expr::Ident(v) if frame.env.get(v).map_or(false, |k| k.starts_with("l:")))) => {
                let Expr::FieldAccess { target, .. } = &left.node else { unreachable!() };
                let Expr::Ident(v) = &target.node else { unreachable!() };
                let key = format!("ok:{}", frame.env[v]);
                let (mut yes, mut no) = (Set::new(), Set::new());
                let (key, either) = (kid(&key), kid(&either_key(&key)));
                for mut x in states {
                    match x.bind.get(key) {
                        Some(true) => { yes.insert(x); }
                        Some(false) => { no.insert(x); }
                        // "either": it is decided here, one copy each way
                        None if x.bind.remove(either).is_some() => {
                            let mut a = x.clone(); a.bind.insert(key, true); yes.insert(a);
                            x.bind.insert(key, false); no.insert(x);
                        }
                        None => { yes.insert(x.clone()); no.insert(x); }
                    }
                }
                if *op == CmpOp::Eq { (yes, no) } else { (no, yes) }
            }
            Expr::CmpOp { left, op: op @ (CmpOp::Eq | CmpOp::Ne), right } => {
                let (read, lit) = match (&left.node, &right.node) {
                    (l, Expr::Literal(Literal::String(s))) => (self.status_of(l, frame), s),
                    (Expr::Literal(Literal::String(s)), r) => (self.status_of(r, frame), s),
                    _ => return (states.clone(), states),
                };
                let Some((m, key, fallback)) = read else { return (states.clone(), states) };
                let fallback = fallback.as_deref();
                let Some(state) = self.machines[m].states.iter().position(|x| x == lit) else { return (states.clone(), states) };
                let initial = self.machines[m].initial;
                // An id never moved is in the initial state — where edition
                // 2027 answers `()` (so `?? "x"` answers "x"): what is read
                // there is then the initial state OR that other value.
                let unborn: Option<Option<usize>> = if !crate::interpreter::EDITION_2027.load(std::sync::atomic::Ordering::Relaxed) { None } else {
                    match fallback {
                        None => Some(None),
                        Some(f) => match self.machines[m].states.iter().position(|x| x == f) {
                            Some(i) if i == initial => None,
                            other => Some(other),
                        },
                    }
                };
                let (hit, other) = self.split_on(&states, &key, m);
                let (mut yes, mut no) = (other.clone(), other);
                for s in hit {
                    let cur = s.st.cur[m] as usize;
                    let eq = *op == CmpOp::Eq;
                    match unborn {
                        Some(alt) if cur == initial && alt != Some(initial) => {
                            // the initial state, or the unborn reading
                            for is in [state == initial, alt == Some(state)] {
                                if is == eq { yes.insert(s.clone()); } else { no.insert(s.clone()); }
                            }
                        }
                        _ => if (cur == state) == eq { yes.insert(s); } else { no.insert(s); },
                    }
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
                            self.first(name) }),
                };
                let Some((cell, on)) = callee else { return (states.clone(), states) };
                let [only] = on.body.as_slice() else { return (states.clone(), states) };
                let Statement::Return { value } = &only.node else { return (states.clone(), states) };
                let mut env = HashMap::new();
                for (i, p) in on.params.iter().enumerate() {
                    if let Some(a) = args.get(i) { env.insert(p.name.clone(), self.key_of(&a.node, a.span, frame)); }
                }
                let inner = Frame { cell, env: Rc::new(env), uid: format!("{}/b{}", frame.uid, only.span.start).into(), depth: frame.depth + 1, status: Rc::default() };
                self.branch(&value.node, &inner, states)
            }
            _ => (states.clone(), states),
        }
    }

    fn constraint(&mut self, c: &Constraint, frame: &Frame<'a>, states: Set, entry: &str) -> Set {
        match c {
            Constraint::Comparison { left, op, right } => {
                let before: BTreeSet<Abs> = states.iter().map(|x| x.st.clone()).collect();
                let moves_before = self.moves;
                let s = self.exprs(&[left, right], frame, states, entry);
                // `require get_status(id) == "x" else …`: only those states go on
                let cond = Expr::CmpOp { left: Box::new(left.clone()), op: *op, right: Box::new(right.clone()) };
                self.narrow(&cond, frame, &before, s, moves_before).0
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
        let mut local = Frame { cell: frame.cell, env: frame.env.clone(), uid: frame.uid.clone(), depth: frame.depth, status: frame.status.clone() };
        // The outcome of a `let p = try { … }` is remembered while `p.error`
        // can still be read: up to the last statement of this block that
        // reads it (ok key, "either" key, that statement's index). Kept to
        // the end of the handler, each `try` of a sweep doubled the states
        // the next one started from.
        let mut tried: Vec<(u32, u32, usize)> = Vec::new();
        let forget_tries = |set: &mut Set, keys: &[(u32, u32)]| {
            if keys.is_empty() || !set.iter().any(|x| x.bind.0.iter().any(|(k, _)| keys.iter().any(|(a, b)| a == k || b == k))) { return; }
            *set = std::mem::take(set).into_iter().map(|mut x| { x.bind.0.retain(|(k, _)| !keys.iter().any(|(a, b)| a == k || b == k)); x }).collect();
        };
        for (at, st) in stmts.iter().enumerate() {
            if flow.normal.is_empty() { break; }
            // a lambda that may move a machine, anywhere it does not run at once
            // (what it finds does not depend on the state: a statement is read once)
            if !self.facts.contains_key(&(st as *const Spanned<Statement> as usize)) {
                match &st.node {
                    Statement::Let { value: e, .. } | Statement::Assign { value: e, .. } | Statement::Return { value: e } | Statement::Ensure { condition: e }
                    | Statement::ExprStmt { expr: e } | Statement::If { condition: e, .. } | Statement::While { condition: e, .. } | Statement::For { iter: e, .. } => self.scan_lambdas(&e.node, false),
                    Statement::IndexSet { index, value, .. } => { self.scan_lambdas(&index.node, false); self.scan_lambdas(&value.node, false); }
                    Statement::MethodCall { args, .. } | Statement::Emit { args, .. } => for a in args { self.scan_lambdas(&a.node, false); },
                    _ => {}
                }
            }
            let facts = self.stmt_facts(st);
            if facts.head_calls && !local.status.is_empty() { local.status = Rc::default(); }
            // a variable assigned inside an expression of this statement may be
            // read after that, in the same statement: unknown from the start
            for n in &facts.assigned {
                if matches!(&st.node, Statement::Let { name, .. } | Statement::Assign { name, .. } if name == n) { continue; }
                Rc::make_mut(&mut local.env).insert(n.clone(), format!("l:{}:{}:{}:b", n, local.uid, st.span.start));
            }
            let cur = std::mem::take(&mut flow.normal);
            match &st.node {
                Statement::Let { name, value } | Statement::Assign { name, value } => {
                    // `let p = try { … }`: the states where it succeeded and those where it
                    // failed (and undid what it did) are told apart — `p.error == ()` reads it
                    let try_key = format!("ok:l:{}:{}:{}", name, local.uid, st.span.start);
                    let mut s = if let Expr::Try(inner) = &value.node {
                        // A state the `try` left as it was is the same whether it
                        // succeeded or failed: it is kept ONCE, marked "either"
                        // (`either_key`), and told apart only where `p.error` is
                        // read. Twenty `try` in a row that move nothing gave 2^20
                        // copies of each state.
                        let (try_key, either) = (kid(&try_key), kid(&either_key(&try_key)));
                        let mut last = at;
                        for (j, later) in stmts.iter().enumerate().skip(at + 1) {
                            if self.stmt_facts(later).errors.as_ref().map_or(true, |read| read.contains(name)) { last = j; }
                        }
                        tried.retain(|t| t.0 != try_key);
                        tried.push((try_key, either, last));
                        let bare = |mut x: Exec| { x.bind.remove(try_key); x.bind.remove(either); x };
                        let ok: Set = self.expr(inner, &local, cur.clone(), entry).into_iter().map(bare).collect();
                        let failed: Set = cur.into_iter().map(bare).collect();
                        let mut out = Set::new();
                        for x in &ok {
                            let mut y = x.clone();
                            if failed.contains(x) { y.bind.insert(either, true); } else { y.bind.insert(try_key, true); }
                            out.insert(y);
                        }
                        for mut x in failed {
                            if !ok.contains(&x) { x.bind.insert(try_key, false); out.insert(x); }
                        }
                        out
                    } else { self.expr(value, &local, cur, entry) };
                    let key = match &value.node {
                        Expr::Ident(v) if local.env.contains_key(v) => local.env[v].clone(),
                        Expr::Literal(Literal::String(_) | Literal::Int(_)) => self.key_of(&value.node, value.span, &local),
                        // `let c = consult_of.get(id)`: the instance `id` names
                        _ if self.key_of(&value.node, value.span, &local).starts_with("link:") => self.key_of(&value.node, value.span, &local),
                        _ => {
                            // a new value: the choice made for the old one ends here
                            let key = format!("l:{}:{}:{}", name, local.uid, st.span.start);
                            let (k, sk) = (kid(&key), kid(&format!("§{}", key)));
                            let was_tried = if matches!(&value.node, Expr::Try(_)) { None } else { let t = format!("ok:{}", key); Some((kid(&t), kid(&either_key(&t)))) };
                            s = s.into_iter().map(|mut x| {
                                x.bind.remove(k); x.bind.remove(sk);
                                if let Some((ok, either)) = was_tried { x.bind.remove(either); x.bind.remove(ok); }
                                x
                            }).collect();
                            key
                        }
                    };
                    Rc::make_mut(&mut local.env).insert(name.clone(), key);
                    flow.normal = s;
                }
                Statement::Return { value } => { flow.returned.extend(self.expr(value, &local, cur, entry)); }
                Statement::Ensure { condition } => { flow.normal = self.expr(condition, &local, cur, entry); }
                Statement::If { condition, then_body, else_body } => {
                    let before: BTreeSet<Abs> = cur.iter().map(|x| x.st.clone()).collect();
                    let moves_before = self.moves;
                    let s = self.expr(condition, &local, cur, entry);
                    let (yes, no) = self.narrow(&condition.node, &local, &before, s, moves_before);
                    let a = self.block(then_body, &local, yes, entry);
                    let b = self.block(else_body, &local, no, entry);
                    flow.normal.extend(a.normal); flow.normal.extend(b.normal);
                    flow.returned.extend(a.returned); flow.returned.extend(b.returned);
                    flow.broke.extend(a.broke); flow.broke.extend(b.broke);
                    flow.continued.extend(a.continued); flow.continued.extend(b.continued);
                    let mut read = Vec::new();
                    tries_read(&condition.node, &local, &mut read);
                    for k in read { rejoin(&mut flow.normal, &k); }
                }
                Statement::For { var, iter, body, .. } => {
                    let s = self.expr(iter, &local, cur, entry);
                    // at each turn the variable — and what the body assigns — is another value
                    let mut names = assigned_in(body);
                    names.push(var.clone());
                    let (inner, keys) = self.rebound(&local, names, st.span.start, "t");
                    let (mut returned, mut broke) = (Set::new(), Set::new());
                    let all = self.repeat(s, |me, x| {
                        let f = me.block(body, &inner, Self::forget(x, &keys, &inner), entry);
                        returned.extend(f.returned); broke.extend(f.broke);
                        // (`continue` goes to the next turn, like the end of the body)
                        let mut next = f.normal; next.extend(f.continued);
                        next
                    });
                    flow.normal = all; flow.normal.extend(broke);
                    flow.returned.extend(returned);
                }
                Statement::While { condition, body, .. } => {
                    let (mut returned, mut broke) = (Set::new(), Set::new());
                    let (inner, keys) = self.rebound(&local, assigned_in(body), st.span.start, "t");
                    let all = self.repeat(cur, |me, x| {
                        let x = me.expr(condition, &inner, Self::forget(x, &keys, &inner), entry);
                        let f = me.block(body, &inner, x, entry);
                        returned.extend(f.returned); broke.extend(f.broke);
                        // (`continue` goes to the next turn, like the end of the body)
                        let mut next = f.normal; next.extend(f.continued);
                        next
                    });
                    flow.normal = self.expr(condition, &inner, all, entry); flow.normal.extend(broke);
                    flow.returned.extend(returned);
                }
                Statement::Break => { flow.broke.extend(cur); }
                Statement::Continue => { flow.continued.extend(cur); }
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
                    // who hears it: the interpreter's one rule (`Interpreter::hears`) —
                    // raised by an interior cell, its siblings and its parent;
                    // raised by a top-level cell, the top-level cells and its own interior
                    let me = local.cell as *const CellDef as usize;
                    let parent = |c: usize| self.parent_of.get(&c).copied();
                    let hears = |c: usize| match (parent(me), parent(c)) {
                        (Some(p), Some(q)) => p == q,
                        (Some(p), None) => p == c,
                        (None, Some(q)) => q == me,
                        (None, None) => true,
                    };
                    let listeners: Vec<(&'a CellDef, &'a OnSection)> = self.cells.iter().copied()
                        .filter(|c| hears(*c as *const CellDef as usize))
                        .filter_map(|c| self.handler(c, signal_name).map(|on| (c, on))).collect();
                    let _ = &parent;
                    for (cell, on) in listeners {
                        // a listener in the emitter's own interior runs only when that
                        // interior was started (`soma run app.cell`, for the cell that
                        // drives the program): it may run, or not
                        let inside = self.parent_of.get(&(cell as *const CellDef as usize)).copied() == Some(me);
                        let after = self.call(cell, on, args, &local, st.span, s.clone(), entry);
                        if inside { s.extend(after); } else { s = after; }
                    }
                    flow.normal = s;
                }
            }
            flow.returned.extend(std::mem::take(&mut self.returned));
            flow.broke.extend(std::mem::take(&mut self.broke));
            flow.continued.extend(std::mem::take(&mut self.continued));
            let over: Vec<(u32, u32)> = tried.iter().filter(|t| t.2 == at).map(|t| (t.0, t.1)).collect();
            forget_tries(&mut flow.normal, &over);
            // a variable assigned in a block inside this statement (a
            // branch, a loop body, a match arm) holds another value from here
            for n in &facts.assigned {
                if matches!(&st.node, Statement::Let { name, .. } | Statement::Assign { name, .. } if name == n) { continue; }
                Rc::make_mut(&mut local.env).insert(n.clone(), format!("l:{}:{}:{}:a", n, local.uid, st.span.start));
                if local.status.contains_key(n) { Rc::make_mut(&mut local.status).remove(n); }
            }
            // a status held in a variable is the status until something is
            // called: any call may move the machine
            if facts.calls && !local.status.is_empty() { local.status = Rc::default(); }
            if let Statement::Let { name, value } | Statement::Assign { name, value } = &st.node {
                match self.status_of(&value.node, &local).filter(|_| !matches!(value.node, Expr::Ident(_))) {
                    Some(read) => { Rc::make_mut(&mut local.status).insert(name.clone(), read); }
                    None => if local.status.contains_key(name) { Rc::make_mut(&mut local.status).remove(name); },
                }
            }
        }
        // (whatever leaves the block leaves its variables)
        let all: Vec<(u32, u32)> = tried.iter().map(|t| (t.0, t.1)).collect();
        for set in [&mut flow.normal, &mut flow.returned, &mut flow.broke, &mut flow.continued] { forget_tries(set, &all); }
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
                // bounds are kept doubled: 2n is "n itself", 2n ± 1 "just
                // above / below n" — the variable may be a Float (`> 5` and
                // `< 6` leave 5.5), and `> 5` with `<= 5` still leaves nothing
                // (a literal whose double does not fit: not read — "may hold")
                _ if n.checked_mul(2).and_then(|d| d.checked_add(1)).and_then(|_| n.checked_mul(2)?.checked_sub(1)).is_none() => return false,
                CmpOp::Lt => slot.1 = slot.1.min(n.saturating_mul(2).saturating_sub(1)),
                CmpOp::Le => slot.1 = slot.1.min(n.saturating_mul(2)),
                CmpOp::Gt => slot.0 = slot.0.max(n.saturating_mul(2).saturating_add(1)),
                CmpOp::Ge => slot.0 = slot.0.max(n.saturating_mul(2)),
                CmpOp::Eq => { slot.0 = slot.0.max(n.saturating_mul(2)); slot.1 = slot.1.min(n.saturating_mul(2)); }
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
#[derive(Clone)]
pub struct Spec {
    pub target: String,
    pub requires: Vec<String>,
    pub all: bool,
    pub assuming: Option<String>,
    /// `while = [...]`: the states in `requires` are where the other
    /// machines must be NOW, when the target is entered
    pub now: bool,
    /// `via = "Cell.slot"`: the required states are those of the instance the
    /// target's instance names in that slot
    pub via: Option<String>,
    /// a rule stated on an edge of a frame (`moves a -> b when …`): only the
    /// entries into the target FROM this state
    pub from: Option<String>,
}

/// The rules the skeletons state on their edges (`moves a -> b when Other.s
/// [via slot]`), as rules between machines: `verify` judges them like those
/// of soma.toml — on the bodies, and on the holes, whose model takes the
/// edge only under its condition.
pub fn frame_specs(program: &Program) -> Vec<Spec> {
    let mut out: Vec<Spec> = Vec::new();
    for cell in super::names::collect_cells(program) {
        if !cell.skeleton { continue; }
        for s in &cell.sections {
            let Section::Face(f) = &s.node else { continue };
            for d in &f.declarations {
                let frame = match &d.node { FaceDecl::Signal(sig) => sig.frame.as_ref(), FaceDecl::Tool(t) => t.frame.as_ref(), _ => None };
                for c in frame.map(|f| f.conditions.as_slice()).unwrap_or(&[]) {
                    if !c.machine.is_empty() || c.to == "*" { continue; }
                    let spec = Spec { target: format!("{}.{}", cell.name, c.to), requires: c.states.clone(), all: false, assuming: None, now: true,
                        via: c.via.as_ref().map(|v| format!("{}.{}", cell.name, v)), from: (c.from != "*").then(|| c.from.clone()) };
                    if !out.iter().any(|o| o.target == spec.target && o.requires == spec.requires && o.via == spec.via && o.from == spec.from) { out.push(spec); }
                }
            }
        }
    }
    out
}

/// The handlers ("Cell.handler") whose run may move a machine owned by one of
/// `kept` (cell addresses) — directly, through what they call, or by doing what
/// this reading cannot follow (an emit, a model, a delegate, a call it cannot
/// resolve). A cone search skips every other handler: it cannot change the
/// product state followed, and its failing only leaves that state as it was.
/// Resolution is by name, so homonyms in other cells only widen the set.
fn touching(cells: &[&CellDef], kept: &HashSet<usize>) -> HashSet<String> {
    const OPAQUE: &[&str] = &["think", "think_json", "delegate", "horde", "vote", "horde_cancel", "await_approve", "await_signal",
        "await_sleep", "resume", "deliver", "cancel_wait", "_external_tool", "_std", "approve"];
    let handlers: HashMap<String, HashSet<String>> = cells.iter().map(|c| (c.name.clone(), c.sections.iter()
        .filter_map(|s| match &s.node { Section::OnSignal(on) => Some(on.signal_name.clone()), _ => None }).collect())).collect();
    let builtins = super::names::builtin_names();
    let mut touch: HashSet<String> = HashSet::new();
    let mut edges: Vec<(String, Vec<String>)> = Vec::new();
    for c in cells {
        let owns = kept.contains(&(*c as *const CellDef as usize));
        let own: HashSet<String> = handlers.get(&c.name).cloned().unwrap_or_default();
        for s in &c.sections {
            let Section::OnSignal(on) = &s.node else { continue };
            let me = format!("{}.{}", c.name, on.signal_name);
            let mut direct = false;
            let mut callees: Vec<String> = Vec::new();
            super::literals::for_each_expr(&on.body, &mut |e| match e {
                Expr::FnCall { name, .. } => {
                    if name == "transition" { if owns { direct = true; } }
                    else if OPAQUE.contains(&name.as_str()) { direct = true; }
                    else if own.contains(name) { callees.push(format!("{}.{}", c.name, name)); }
                    else {
                        let mut found = false;
                        for (cn, hs) in &handlers { if hs.contains(name) { callees.push(format!("{}.{}", cn, name)); found = true; } }
                        // a function value (it may be a handler); not a type or
                        // variant constructor, not an internal builtin
                        let lower = name.chars().next().map_or(false, |ch| ch.is_lowercase());
                        if !found && lower && !builtins.contains(name.as_str()) { direct = true; }
                    }
                }
                Expr::MethodCall { target, method, .. } => {
                    if method == "transition" { if owns { direct = true; } }
                    else if let Expr::Ident(t) = &target.node { if handlers.contains_key(t) { callees.push(format!("{}.{}", t, method)); } }
                }
                _ => {}
            });
            super::literals::for_each_stmt_deep(&on.body, &mut |st| match st {
                Statement::Emit { .. } => direct = true,
                Statement::MethodCall { target, method, .. } if handlers.contains_key(target) => callees.push(format!("{}.{}", target, method)),
                _ => {}
            });
            if direct { touch.insert(me.clone()); }
            edges.push((me, callees));
        }
    }
    loop {
        let mut changed = false;
        for (n, cs) in &edges { if !touch.contains(n) && cs.iter().any(|c| touch.contains(c)) { touch.insert(n.clone()); changed = true; } }
        if !changed { break; }
    }
    touch
}

const OPEN_MARK: &str = "\u{1f}open:";

/// `via` is sound only if the link of an entry never changes: the slot is
/// written by ONE handler, once, when the entry is created (after a
/// `require slot.get(k) == ()` on the same key), and never deleted.
fn link_written_once(cells: &[&CellDef], cell: &str, slot: &str) -> Result<(), String> {
    let Some(c) = cells.iter().find(|c| c.name == cell) else { return Err(format!("there is no cell `{}`", cell)) };
    let declared = c.sections.iter().any(|s| matches!(&s.node, Section::Memory(m) if m.slots.iter().any(|sl| sl.node.name == slot)));
    if !declared { return Err(format!("`{}` has no memory slot `{}`", cell, slot)); }
    let writes = |m: &str| matches!(m, "set" | "put" | "delete" | "remove" | "push" | "append");
    let mut writers: Vec<(String, Vec<(String, String)>)> = Vec::new();   // handler → (method, key text)
    for on in super::bodies::entries(c) {
        let mut found: Vec<(String, String)> = Vec::new();
        super::literals::for_each_stmt_deep(&on.body, &mut |st| match st {
            Statement::MethodCall { target, method, args } if target == slot && writes(method) =>
                found.push((method.clone(), args.first().map(|a| render_expr(&a.node)).unwrap_or_default())),
            Statement::IndexSet { name, index, .. } if name.split(|ch| ch == '.' || ch == '[').next() == Some(slot) =>
                found.push(("set".to_string(), render_expr(&index.node))),
            _ => {}
        });
        super::literals::for_each_expr(&on.body, &mut |e| if let Expr::MethodCall { target, method, args } = e {
            if matches!(&target.node, Expr::Ident(t) if t == slot) && writes(method) {
                found.push((method.clone(), args.first().map(|a| render_expr(&a.node)).unwrap_or_default()));
            }
        });
        // …and inside a string's `{…}` (`"{consult_of.set(id, c)}"`)
        super::literals::for_each_expr(&on.body, &mut |e| if let Expr::Literal(Literal::String(t)) = e {
            for seg in super::desugar::segments(t) {
                if let Some(x) = super::desugar::parse_segment(&seg) {
                    super::literals::for_each_in_expr(&x, &mut |y| if let Expr::MethodCall { target, method, args } = y {
                        if matches!(&target.node, Expr::Ident(t) if t == slot) && writes(method) {
                            found.push((format!("{} (in a string)", method), args.first().map(|a| render_expr(&a.node)).unwrap_or_default()));
                        }
                    });
                }
            }
        });
        if !found.is_empty() { writers.push((on.signal_name.clone(), found)); }
    }
    // a writer that is still a hole: written once is judged on its body
    if let Some((h, _)) = writers.iter().find(|(h, _)| c.sections.iter().any(|s| matches!(&s.node, Section::OnSignal(o) if &o.signal_name == h && o.properties.iter().any(|p| p == HOLE)))) {
        return Err(format!("{}{} is written by the hole `{}` — that it is written once (one `set`, after `require {}.get(id) == () else …`, never deleted) is judged when it has a body", OPEN_MARK, slot, h, slot));
    }
    let rule = format!("`via = \"{}.{}\"` needs the link written once — by ONE signal, once, when the entry is created (`require {}.get(id) == () else Exists` before `{}.set(id, …)`), never rewritten nor deleted: keep the link in a slot of its own", cell, slot, slot, slot);
    let [(handler, ws)] = writers.as_slice() else {
        return Err(format!("{} — `{}` is written by {}", rule, slot, if writers.is_empty() { "no signal".to_string() } else { writers.iter().map(|(h, _)| format!("`{}`", h)).collect::<Vec<_>>().join(", ") }));
    };
    let [(method, key)] = ws.as_slice() else { return Err(format!("{} — `{}` writes it {} times", rule, handler, ws.len())) };
    if !matches!(method.as_str(), "set" | "put") { return Err(format!("{} — `{}` calls `{}.{}`", rule, handler, slot, method)); }
    let on = c.sections.iter().find_map(|s| match &s.node { Section::OnSignal(o) if &o.signal_name == handler => Some(o), _ => None });
    // the key is a VALUE that does not change between the guard and the write:
    // a parameter the handler never rebinds (`let id = k` between them made
    // the guard about one key and the write about another)
    let rebound = on.is_some_and(|o| {
        let mut names: HashSet<String> = HashSet::new();
        super::invariants::collect_binders_stmts(&o.body, &mut names);
        super::literals::for_each_stmt_deep(&o.body, &mut |st| match st {
            Statement::Let { name, .. } | Statement::Assign { name, .. } => { names.insert(name.clone()); }
            Statement::For { var, .. } => { names.insert(var.clone()); }
            _ => {}
        });
        names.contains(key)
    });
    // or a local bound ONCE, by a `let` at the top of the body, before the guard
    // (`let id = "AD-{n}"`, an id the handler makes) and never assigned again:
    // the guard and the write read the same value
    let local_once = on.is_some_and(|o| {
        if o.params.iter().any(|p| &p.name == key) { return false; }
        let mut binders: HashSet<String> = HashSet::new();
        super::invariants::collect_binders_stmts(&o.body, &mut binders);
        if binders.contains(key) { return false; }
        let mut count = 0usize; let mut assigned = false;
        super::literals::for_each_stmt_deep(&o.body, &mut |st| match st {
            Statement::Let { name, .. } if name == key => { count += 1; }
            Statement::Assign { name, .. } if name == key => { assigned = true; }
            Statement::For { var, .. } if var == key => { assigned = true; }
            _ => {}
        });
        count == 1 && !assigned && o.body.iter().any(|st| matches!(&st.node, Statement::Let { name, .. } if name == key))
    });
    if !local_once && (!on.is_some_and(|o| o.params.iter().any(|p| &p.name == key)) || rebound) {
        return Err(format!("{} — `{}` writes it at the key `{}`, which is not one of its parameters left as it came", rule, handler, key));
    }
    // nothing between the guard and the write lets another run in: not a
    // `[task]` (an `await`, a `think()` end its step and let a second task
    // pass the same guard)
    if on.is_some_and(|o| o.properties.iter().any(|p| p == "task")) {
        return Err(format!("{} — `{}` is a `[task]`: another run may pass the guard while it waits, and write the link again", rule, handler));
    }
    // the creation guard, at the top level of the writer, before the write
    let guarded = on.is_some_and(|o| o.body.iter().take_while(|st| !matches!(&st.node, Statement::MethodCall { target, .. } if target == slot)).any(|st| {
        let Statement::Require { constraint, .. } = &st.node else { return false };
        let Constraint::Comparison { left, op: CmpOp::Eq, right } = &constraint.node else { return false };
        let reads_key = |e: &Expr| matches!(e, Expr::MethodCall { target, method, args } if method == "get" && args.len() == 1
            && matches!(&target.node, Expr::Ident(t) if t == slot) && render_expr(&args[0].node) == *key);
        let absent = |l: &Expr, r: &Expr| (reads_key(l) && matches!(r, Expr::Literal(Literal::Unit))) || (reads_key(r) && matches!(l, Expr::Literal(Literal::Unit)));
        // `require slot.get(k) == () else …` is the comparison `(slot.get(k) == ()) == true`
        absent(&left.node, &right.node) || matches!((&left.node, &right.node), (Expr::CmpOp { left: l, op: CmpOp::Eq, right: r }, Expr::Literal(Literal::Bool(true))) if absent(&l.node, &r.node))
    }));
    if !guarded { return Err(format!("{} — `{}` writes it without refusing an existing entry first", rule, handler)); }
    Ok(())
}

/// One search: `group`'s rules over the machines of their cone (`cone`) or
/// over every machine. `shared`: the rules are searched together because
/// they have the same cone — each is then answered as its own search would.
#[allow(clippy::too_many_arguments)]
fn attempt(exposed: &Program, cells: &[&CellDef], group: &[&Spec], cone: bool, shared: bool,
           assumption: Option<&Expr>, assuming: Option<&String>, via: Option<&String>, link: Option<&(String, String)>) -> Vec<super::verify::VerifyCheck> {
    use super::verify::VerifyCheck;
    // (the keys are this search's own: none is left for the next, none kept after)
    struct Keys;
    impl Drop for Keys { fn drop(&mut self) { KEYS.with(|k| *k.borrow_mut() = Default::default()); } }
    KEYS.with(|k| *k.borrow_mut() = Default::default());
    let _keys = Keys;
    let mut out: Vec<VerifyCheck> = Vec::new();
        let mut machines: Vec<Machine> = Vec::new();
        let mut dropped: Vec<String> = Vec::new();
        for cell in cells {
            for section in &cell.sections {
                let Section::State(sm) = &section.node else { continue };
                let sm = match assumption {
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
                machines.push(Machine { owner: *cell as *const CellDef as usize, cell: cell.name.clone(), name: sm.name.clone(), initial: index(&sm.initial).unwrap_or(0), states, edges: edges.into_iter().map(|(k, v)| (k, Rc::new(v))).collect() });
            }
        }
        // CONE OF INFLUENCE: first, only the machines the rules name (plus any
        // machine whose edges run effects — those may call handlers). The others
        // are left out: their status reads as unknown (a condition on it goes
        // both ways) and their transitions succeed. That is MORE behaviours than
        // the program has — a failing transition aborts the handler (nothing
        // moves) and a `try` already keeps the state before it — so a ✓ here
        // holds for the program. Anything short of a ✓ on every rule of the
        // group: the search is run again over every machine, as before.
        let mut sliced: HashSet<usize> = HashSet::new();
        if cone {
            let named: HashSet<&str> = group.iter()
                .flat_map(|s| std::iter::once(s.target.as_str()).chain(s.requires.iter().map(|r| r.as_str())))
                .filter_map(|n| n.split_once('.').map(|(c, _)| c)).collect();
            let unqualified = group.iter().any(|s| std::iter::once(&s.target).chain(s.requires.iter()).any(|n| !n.contains('.')));
            if unqualified { return out; }
            machines.retain(|m| {
                let keep = named.contains(m.cell.as_str()) || m.edges.values().any(|e| !e.is_empty());
                if !keep { sliced.insert(m.owner); }
                keep
            });
            // (a cone that leaves nothing out is the full search for its rule)
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
        for s in group {
            let said = format!("{}{}", via.as_ref().map(|v| format!(" via {}", v)).unwrap_or_default(), assuming.as_ref().map(|a| format!(", assuming {}", a)).unwrap_or_default());
            let label = if s.now { format!("entering {}{} while {}{}", s.target, s.from.as_ref().map(|f| format!(" from {}", f)).unwrap_or_default(), s.requires.join(" | "), said) }
                else { format!("before {} requires {}{}{}", s.target, if s.all { "each of " } else { "" }, s.requires.join(if s.all { ", " } else { " | " }), said) };
            let parts: Result<Vec<(usize, usize)>, String> = s.requires.iter().map(|r| resolve(r)).collect();
            match (resolve(&s.target), parts) {
                // every instance is in its initial state from the start, by
                // no transition: nothing can come before it, and a ✓ there
                // would say nothing
                (Ok(target), Ok(_)) if machines[target.0].initial == target.1 => out.push(VerifyCheck::Fail(format!(
                    "{}: `{}` is the initial state of {}.{} — an instance is there from the start, by no transition, so nothing can be required before it. State the rule on the state the guarded signal moves to (give that signal an edge to take)",
                    label, s.target, machines[target.0].cell, machines[target.0].name), None)),
                (Ok(target), Ok(parts)) if s.now => {
                    if parts.iter().any(|(m, _)| *m == target.0) {
                        out.push(VerifyCheck::Fail(format!("{}: `while` names a state of the target's own machine — its edges already say where it comes from", label), None));
                    } else {
                        let from = s.from.as_ref().and_then(|f| machines[target.0].states.iter().position(|x| x == f));
                        props.push(Property { label, target, needs: Vec::new(), all: false, now: parts, from });
                    }
                }
                (Ok(target), Ok(parts)) => {
                    let needs = parts.into_iter().map(|p| { let n = watched.len() as u32; *watched.entry(p).or_insert(n) }).collect();
                    let from = s.from.as_ref().and_then(|f| machines[target.0].states.iter().position(|x| x == f));
                    props.push(Property { label, target, needs, all: s.all, now: Vec::new(), from });
                }
                (Err(e), _) | (_, Err(e)) => out.push(VerifyCheck::Fail(format!("{}: {}", label, e), None)),
            }
        }
        if props.is_empty() { return out; }
        // with a link, the machines a rule requires states of follow the instance
        // the target's instance names; the target's own machine, the target's
        let mut secondary: HashSet<usize> = HashSet::new();
        if let Some((lc, _)) = link {
            for p in &props {
                if machines[p.target.0].cell != *lc {
                    for s in group { out.push(VerifyCheck::Fail(format!("before {}: `via` names a slot of `{}`, and the target is a state of `{}` — the link is read from the target's cell", s.target, lc, machines[p.target.0].cell), None)); }
                    continue;
                }
                let required: Vec<usize> = if p.now.is_empty() { watched.iter().filter(|(_, b)| p.needs.contains(b)).map(|((m, _), _)| *m).collect() } else { p.now.iter().map(|(m, _)| *m).collect() };
                for m in required {
                    if machines[m].cell == *lc { continue; }
                    secondary.insert(m);
                }
            }
        }
        if machines.len() > 12 || machines.iter().any(|m| m.states.len() > 250) || watched.len() > 60 {
            for p in &props { out.push(VerifyCheck::Fail(format!("{}: not explored — more than 12 machines in this search (the rule's cone, or the whole program), 250 states in one machine or 60 required states", p.label), None)); }
            return out;
        }

        // entry points: (label, cell, body, parameters, each transition a step of its own?)
        struct Entry<'e> { label: String, cell: &'e CellDef, body: &'e [Spanned<Statement>], params: &'e [Param], task: bool }
        let mut entries: Vec<Entry> = Vec::new();
        for cell in cells {
            for (i, section) in cell.sections.iter().enumerate() {
                match &section.node {
                    Section::OnSignal(on) => entries.push(Entry { label: format!("{}.{}", cell.name, on.signal_name), cell, body: &on.body, params: &on.params,
                        task: on.properties.iter().any(|p| p == "task") }),
                    Section::Every(e) | Section::After(e) => entries.push(Entry { label: format!("{} timer #{}", cell.name, i), cell, body: &e.body, params: &[], task: e.task }),
                    _ => {}
                }
            }
        }
        let prune = if sliced.is_empty() { None } else { Some(touching(cells, &machines.iter().map(|m| m.owner).collect())) };
        let mut engine = Engine { cells: cells.to_vec(), machines, sliced: sliced.clone(), prune, watched, props, broken: BTreeMap::new(), unsupported: None, names: super::desugar::program_names(exposed), link: link.cloned(), secondary: secondary.clone(), moves: 0, forced: false, variants: variant_names(exposed), top: exposed.cells.iter().map(|c| &c.node as *const CellDef as usize).collect(), parent_of: {
            fn walk(c: &CellDef, out: &mut HashMap<usize, usize>) {
                for s in &c.sections { if let Section::Interior(i) = &s.node { for x in &i.cells { out.insert(&x.node as *const CellDef as usize, c as *const CellDef as usize); walk(&x.node, out); } } }
            }
            let mut out = HashMap::new();
            for c in &exposed.cells { walk(&c.node, &mut out); }
            out
        }, returned: Set::new(), broke: Set::new(), continued: Set::new(),
            cell_ix: HashMap::new(), handler_ix: HashMap::new(), first_ix: HashMap::new(), slots_ix: HashMap::new(), facts: HashMap::new(), segs: HashMap::new() };
        engine.index();
        let mut start = Abs { cur: [0; 12], seen: 0 };
        for (m, mach) in engine.machines.iter().enumerate() { start.cur[m] = mach.initial as u8; }
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
                if !entry.label.contains(" timer #") {
                    if let Some(p) = &engine.prune { if !p.contains(&entry.label) { continue; } }
                }
                let mut env = HashMap::new();
                for p in entry.params { env.insert(p.name.clone(), format!("e:{}", p.name)); }
                let frame = Frame { cell: entry.cell, env: Rc::new(env), uid: "".into(), depth: 0, status: Rc::default() };
                let mut init = Set::new();
                init.insert(Exec { st: state.clone(), bind: Bind::default() });
                let before: BTreeSet<usize> = engine.broken.keys().copied().collect();
                let flow = engine.block(entry.body, &frame, init, &entry.label);
                let mut ends: Vec<Abs> = flow.normal.into_iter().chain(flow.returned).chain(flow.broke).chain(flow.continued).map(|e| e.st).collect();
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
                out.push(VerifyCheck::Fail(format!("{}: not proven — {}", p.label, why), None));
            } else if let Some((at, entry)) = witness.get(&i) {
                // the invocations that lead there, then the one that breaks it
                let mut steps = vec![format!("{}  ← enters {} here", entry, engine.machines[p.target.0].states[p.target.1])];
                let mut cur = at.clone();
                while let Some((prev, by)) = from.get(&cur) { steps.push(by.clone()); cur = prev.clone(); }
                steps.reverse();
                let show = |a: &Abs| a.cur.iter().take(engine.machines.len()).enumerate().map(|(m, s)| format!("{}={}", engine.machines[m].cell, engine.machines[m].states[*s as usize])).collect::<Vec<_>>().join(", ");
                out.push(VerifyCheck::Fail(
                    format!("{}: a run of the program's handlers enters `{}` without it (from {}){}", p.label, engine.machines[p.target.0].states[p.target.1], show(at), assumed),
                    Some(steps)));
            } else if refused {
                out.push(VerifyCheck::Fail(format!("{}: refused — more than {} product states over {} (the search is exhaustive or it is not a proof)", p.label, LIMIT, shape), None));
            } else {
                // (searched with other rules: the states this rule's own search explores —
                // those of the shared search, without the other rules' required states)
                let explored = if !shared { seen.len() } else {
                    let mask: u64 = p.needs.iter().fold(0, |m, b| m | (1 << b));
                    seen.iter().map(|a| (&a.cur, a.seen & mask)).collect::<HashSet<_>>().len()
                };
                out.push(VerifyCheck::Pass(format!("{} — every run of the handlers, {} product state(s) explored over {}{}", p.label, explored, shape, assumed)));
            }
        }
        out
}

/// The cells a rule names: what its cone is made from.
fn cone_key(s: &Spec) -> Option<BTreeSet<&str>> {
    let names = || std::iter::once(&s.target).chain(s.requires.iter());
    if names().any(|n| !n.contains('.')) { return None; }
    Some(names().filter_map(|n| n.split_once('.').map(|(c, _)| c)).collect())
}

/// Check the cross-machine properties. Empty when there is none.
pub fn check(program: &Program, specs: &[Spec]) -> Vec<super::verify::VerifyResult> {
    use super::verify::{VerifyCheck, VerifyResult};
    if specs.is_empty() { return Vec::new(); }
    let exposed = super::desugar::expose_in_place(program);
    let mut cells: Vec<&CellDef> = super::names::collect_cells(&exposed).into_iter()
        .filter(|c| matches!(c.kind, CellKind::Cell | CellKind::Agent)).collect();
    // top-level cells first: a bare call goes to a top-level handler, never
    // to an interior cell's homonym (the search took the first in file order)
    cells.sort_by_key(|c| !exposed.cells.iter().any(|t| std::ptr::eq(&t.node, *c)));
    let mut result = VerifyResult {
        machine_name: "product".to_string(), states: vec![], initial: String::new(), terminal_states: vec![], transitions: vec![], checks: vec![],
    };
    // properties that share an assumption share a search
    // (…and a link: `via` changes which instance of the other machines is followed)
    let mut groups: Vec<((Option<String>, Option<String>), Vec<&Spec>)> = Vec::new();
    for s in specs {
        let k = (s.assuming.clone(), s.via.clone());
        match groups.iter_mut().find(|g| g.0 == k) { Some(g) => g.1.push(s), None => groups.push((k, vec![s])) }
    }
    // Each group is prepared (what can be said of it before any search), then
    // the searches of ALL the groups run, then each group is concluded, in order.
    struct Prep<'s> { assuming: Option<String>, via: Option<String>, group: Vec<&'s Spec>, link: Option<(String, String)>, assumption: Option<Expr>, cones: Vec<Vec<usize>> }
    let mut said: Vec<Vec<VerifyCheck>> = Vec::new();
    let mut preps: Vec<Option<Prep>> = Vec::new();
    for ((assuming, via), group) in groups {
        let mut pre: Vec<VerifyCheck> = Vec::new();
        let prep = 'prep: {
            // soma.toml states its rules for every program of its folder: a rule
            // that names only cells this program does not have is about another
            // program (a neighbour file failed on `there is no cell Tir`) — a
            // note here. A rule naming one of its cells and a missing one is a
            // typo, and stays an error below.
            let group: Vec<&Spec> = group.into_iter().filter(|s| {
                let named: Vec<&str> = std::iter::once(s.target.as_str()).chain(s.requires.iter().map(|r| r.as_str())).chain(via.as_deref())
                    .filter_map(|n| n.split_once('.').map(|(c, _)| c)).collect();
                if named.is_empty() || named.iter().any(|c| cells.iter().any(|x| x.name == *c)) { return true; }
                let mut names: Vec<&str> = named.clone();
                names.sort();
                names.dedup();
                pre.push(VerifyCheck::Note(format!(
                    "soma.toml: the rule on `{}` names {} — no cell of this program; it applies to another program of this folder, so it is not checked here",
                    s.target, names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", "))));
                false
            }).collect();
            if group.is_empty() { break 'prep None; }
            let link: Option<(String, String)> = match via.as_deref() {
                None => None,
                Some(v) => match v.split_once('.') {
                    Some((c, sl)) => match link_written_once(&cells, c, sl) {
                        Ok(()) => Some((c.to_string(), sl.to_string())),
                        Err(e) if e.starts_with(OPEN_MARK) => { for s in &group { pre.push(VerifyCheck::Warning(format!("open obligation — before {} via {}: {}", s.target, v, e.trim_start_matches(OPEN_MARK)))); } break 'prep None; }
                        Err(e) => { for s in &group { pre.push(VerifyCheck::Fail(format!("before {}: not proven — {}", s.target, e), None)); } break 'prep None; }
                    },
                    None => { for s in &group { pre.push(VerifyCheck::Fail(format!("before {}: `via = \"{}\"` names a slot as `Cell.slot`", s.target, v), None)); } break 'prep None; }
                },
            };
            let assumption = match assuming.as_deref().map(parse_assumption) {
                Some(Err(e)) => { for s in &group { pre.push(VerifyCheck::Fail(format!("before {}: {}", s.target, e), None)); } break 'prep None; }
                Some(Ok(e)) => Some(e),
                None => None,
            };
            // first one cone per rule (each as small as the rule); the rules whose cone
            // falls short of a ✓ — and only they — are then searched together over
            // every machine. (Per rule: in a large program one rule left open by a
            // hole sent every other rule of the group to the full product.)
            //
            // Rules that name the same cells have the same cone, and their searches
            // differ only in the required states each remembers: they are searched
            // ONCE, together, and each is given the answer its own search gives
            // (when all of them are proven — otherwise each is searched alone, as
            // before).
            let mut cones: Vec<(Option<BTreeSet<&str>>, Vec<usize>)> = Vec::new();
            for (i, s) in group.iter().enumerate() {
                let k = cone_key(s);
                match cones.iter_mut().find(|c| k.is_some() && c.0 == k) { Some(c) => c.1.push(i), None => cones.push((k, vec![i])) }
            }
            let cones = cones.into_iter().map(|c| c.1).collect();
            Some(Prep { assuming, via, group, link, assumption, cones })
        };
        said.push(pre);
        preps.push(prep);
    }
    // The cone searches are independent of one another: they run on every core.
    let jobs: Vec<(usize, &Vec<usize>)> = preps.iter().enumerate()
        .flat_map(|(g, p)| p.iter().flat_map(move |p| p.cones.iter().map(move |members| (g, members)))).collect();
    let (cells_ref, exposed_ref, preps_ref) = (&cells[..], &exposed, &preps);
    let search = |g: usize, members: &Vec<usize>| -> Vec<(usize, usize, Vec<VerifyCheck>)> {
        let p = preps_ref[g].as_ref().expect("a job of a prepared group");
        let run = |specs: &[&Spec], shared: bool| attempt(exposed_ref, cells_ref, specs, true, shared, p.assumption.as_ref(), p.assuming.as_ref(), p.via.as_ref(), p.link.as_ref());
        if members.len() > 1 {
            let specs: Vec<&Spec> = members.iter().map(|i| p.group[*i]).collect();
            let all = run(&specs, true);
            if all.len() == members.len() && all.iter().all(|c| matches!(c, VerifyCheck::Pass(_))) {
                return members.iter().zip(all).map(|(i, c)| (g, *i, vec![c])).collect();
            }
        }
        members.iter().map(|i| (g, *i, run(&[p.group[*i]], false))).collect()
    };
    let mut per_rule: Vec<Vec<Option<Vec<VerifyCheck>>>> = preps.iter().map(|p| p.as_ref().map_or(Vec::new(), |p| p.group.iter().map(|_| None).collect())).collect();
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(jobs.len());
    if workers <= 1 {
        for (g, members) in &jobs { for (g, i, out) in search(*g, members) { per_rule[g][i] = Some(out); } }
    } else {
        let next = std::sync::atomic::AtomicUsize::new(0);
        let done: std::sync::Mutex<Vec<(usize, usize, Vec<VerifyCheck>)>> = std::sync::Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for _ in 0..workers {
                // (a search recurses through the handlers it follows, as deep as they call)
                std::thread::Builder::new().stack_size(512 * 1024 * 1024).spawn_scoped(scope, || {
                    // ("is this name a cell?" is asked of the thread's own program)
                    super::names::set_program_cells(super::desugar::program_names(exposed_ref).1);
                    loop {
                        let n = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((g, members)) = jobs.get(n) else { break };
                        let found = search(*g, members);
                        done.lock().unwrap().extend(found);
                    }
                }).expect("spawn a search thread");
            }
        });
        for (g, i, out) in done.into_inner().unwrap() { per_rule[g][i] = Some(out); }
    }
    for ((pre, prep), found) in said.into_iter().zip(&preps).zip(per_rule) {
        result.checks.extend(pre);
        let Some(Prep { assuming, via, group, link, assumption, .. }) = prep else { continue };
        let mut kept: Vec<VerifyCheck> = Vec::new();
        let mut rest: Vec<&Spec> = Vec::new();
        // what the cone said of a rule it did not prove: shown in place of a
        // full search that could not be explored (it may be spurious — the
        // machines outside the cone read as unknown — but it names the run)
        let mut cone_said: Vec<VerifyCheck> = Vec::new();
        for (i, out) in found.into_iter().enumerate() {
            let out = out.unwrap_or_default();
            // a cone that proves its rule keeps its verdict; one that does not sends
            // its rule to the search over every machine
            let proved = !out.is_empty() && out.iter().all(|c| matches!(c, VerifyCheck::Pass(_)));
            if proved { kept.extend(out); } else { rest.push(group[i]); cone_said.extend(out.into_iter().filter(|c| !matches!(c, VerifyCheck::Pass(_)))); }
        }
        if !rest.is_empty() {
            let out = attempt(&exposed, &cells, &rest, false, false, assumption.as_ref(), assuming.as_ref(), via.as_ref(), link.as_ref());
            for c in out {
                let unexplored = matches!(&c, VerifyCheck::Fail(m, _) if m.contains(": not explored — more than 12 machines"));
                let label = match &c { VerifyCheck::Fail(m, _) => m.split(": not explored").next().unwrap_or("").to_string(), _ => String::new() };
                let said = if unexplored { cone_said.iter().position(|k| matches!(k, VerifyCheck::Fail(m, _) if m.starts_with(&label) && !m.contains(": not explored"))) } else { None };
                match said {
                    Some(i) => {
                        let VerifyCheck::Fail(m, steps) = cone_said.remove(i) else { unreachable!() };
                        kept.push(VerifyCheck::Fail(format!("{} [in the rule's cone; the search over every machine was not explored — this run may be spurious: machines outside the cone read as unknown]", m), steps));
                    }
                    None => kept.push(c),
                }
            }
        }
        result.checks.extend(kept);
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
        // (the sites are read as expressions: one written inside a string is not among them)
        let mut hidden = false;
        super::literals::for_each_expr(body, &mut |x| if let Expr::Literal(Literal::String(t)) = x { if super::desugar::segments(t).iter().any(|seg| seg.contains("transition") || seg.contains('(')) { hidden = true; } });
        super::literals::for_each_expr(body, &mut |x| if let Expr::Match { arms, .. } = x {
            for arm in arms { let mut texts = Vec::new(); super::desugar::pattern_texts(&arm.pattern, &mut texts); if texts.iter().any(|t| !super::desugar::segments(t).is_empty()) { hidden = true; } }
        });
        if hidden { self.unsupported.get_or_insert("a `[task]` handler calls something inside a string's `{…}`: its steps are not followed there".to_string()); }
        let mut out = Vec::new();
        for (cell, call) in sites {
            // (what the handler binds is a variable there, whatever state bears the name)
            let mut env: HashMap<String, String> = HashMap::new();
            let mut bound: HashSet<String> = frame.env.keys().cloned().collect();
            super::invariants::collect_binders_stmts(body, &mut bound);
            super::literals::for_each_stmt_deep(body, &mut |st| match st {
                Statement::Let { name, .. } | Statement::Assign { name, .. } => { bound.insert(name.clone()); }
                Statement::For { var, .. } => { bound.insert(var.clone()); }
                _ => {}
            });
            for n in bound { env.insert(n.clone(), format!("l:{}:t{}", n, call.span.start)); }
            let inner = Frame { cell, env: Rc::new(env), uid: format!("t{}", call.span.start).into(), depth: 1, status: Rc::default() };
            let mut init = Set::new();
            init.insert(Exec { st: state.clone(), bind: Bind::default() });
            out.extend(self.expr(call, &inner, init, entry).into_iter().map(|e| e.st));
            out.extend(std::mem::take(&mut self.returned).into_iter().map(|e| e.st));
            out.extend(std::mem::take(&mut self.broke).into_iter().map(|e| e.st));
            out.extend(std::mem::take(&mut self.continued).into_iter().map(|e| e.st));
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
                        if let Some(found) = self.first(name) { callees.push(found); }
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
