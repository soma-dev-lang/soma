//! Hordes (docs/design/horde.md, phase 2): one handler run over many inputs
//! by a bounded pool of workers.
//!
//! - The queue is persisted like the program's data: two tables per owner
//!   cell (`<Cell>__horde-meta`, `<Cell>__horde-tasks`) in soma.db when the
//!   program has persistent storage, in memory otherwise.
//! - `horde()` writes the horde and its tasks inside the caller's atomic
//!   unit; the workers start only when that unit COMMITS (a rolled-back
//!   caller leaves no horde behind).
//! - A task's result is recorded, and `on_result` called, in the same unit
//!   as the target handler's LAST step: a crash either keeps both or
//!   neither — re-running the task after a restart writes its result once.
//! - Without a worker factory (`soma test`, or a program not started by
//!   `soma serve` / `soma run`) the horde runs inline, synchronously.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use super::{map_from_pairs, stored_to_value, value_to_stored, Interpreter, RuntimeError, SomaInt, UndoOp, Value};
use crate::runtime::storage::{MemoryBackend, SqliteBackend, StorageBackend};

/// Builds a worker interpreter over the program's shared storage. Set by
/// `soma serve` and `soma run`.
pub type Factory = Box<dyn Fn() -> Interpreter + Send + Sync>;
pub static FACTORY: OnceLock<Factory> = OnceLock::new();

/// Most workers one horde may start (each is a thread and an interpreter).
pub const MAX_CONCURRENCY: usize = 1000;

#[derive(Clone, Debug)]
pub struct Spec {
    pub id: String,
    /// the cell that called horde(): its tables, its callbacks
    pub owner: String,
    pub target_cell: String,
    pub handler: String,
    pub on_result: Option<String>,
    pub on_done: Option<String>,
    pub on_error: Option<String>,
    pub concurrency: usize,
    pub max_attempts: u32,
    pub total: usize,
    pub budget_tokens: Option<i64>,
    /// the same value for every task, passed as the target's 2nd argument
    /// (a round of a simulation: every agent sees the same world)
    pub snapshot: Option<Value>,
    /// results applied at the END, in input order (not as they arrive)
    pub apply: Option<String>,
    /// random() in task i is seeded from (seed, i, attempt): reproducible
    pub seed: Option<i64>,
    /// the input field naming an agent INSTANCE: its remember()/recall()
    /// and its conversation persist across hordes (rounds)
    pub instance: Option<String>,
    /// the horde whose task / callback started this one, and its budget
    pub parent: Option<String>,
    pub parent_budget: Option<Arc<Budget>>,
}

/// `budget_tokens`: a hard ceiling for the whole horde. Every think() of a
/// task (and of on_result / on_done) RESERVES an upper bound of its cost —
/// the request's bytes (a token is at least one byte) plus max_tokens —
/// before calling the model, and settles the real count after. A call that
/// does not fit is refused (kind `budget`) and the horde stops starting
/// tasks: spent + reserved never passes the limit.
#[derive(Debug)]
pub struct Budget {
    pub limit: i64,
    /// the budget of the horde whose task (or callback) started this one:
    /// every reservation is made on both — a nested horde cannot spend
    /// past its parent's ceiling
    parent: Option<Arc<Budget>>,
    /// (spent, reserved)
    state: Mutex<(i64, i64)>,
    settled: Condvar,
    pub exhausted: std::sync::atomic::AtomicBool,
}

impl Budget {
    pub fn new(limit: i64, spent: i64) -> Self {
        Budget { limit, parent: None, state: Mutex::new((spent, 0)), settled: Condvar::new(), exhausted: std::sync::atomic::AtomicBool::new(false) }
    }
    pub fn with_parent(limit: i64, spent: i64, parent: Option<Arc<Budget>>) -> Self {
        Budget { parent, ..Budget::new(limit, spent) }
    }
    /// The budget a horde runs under: its own (chained to its parent's),
    /// else its parent's, else none.
    pub fn for_horde(own: Option<i64>, spent: i64, parent: Option<Arc<Budget>>) -> Option<Arc<Budget>> {
        match (own, parent) {
            (Some(b), p) => Some(Arc::new(Budget::with_parent(b, spent, p))),
            (None, Some(p)) => Some(p),
            (None, None) => None,
        }
    }
    fn is_exhausted(&self) -> bool {
        self.exhausted.load(std::sync::atomic::Ordering::SeqCst) || self.parent.as_ref().map_or(false, |p| p.is_exhausted())
    }
    /// Reserve `want` tokens. Calls in flight hold reservations larger than
    /// what they will spend: when `can_wait` (outside the handler lock) a
    /// call that does not fit YET waits for them to settle; it is refused
    /// only when it cannot fit even after they have.
    pub fn reserve(&self, want: i64, can_wait: bool) -> Result<(), RuntimeError> {
        self.reserve_own(want, can_wait)?;
        if let Some(p) = &self.parent {
            if let Err(e) = p.reserve(want, can_wait) {
                // the parent's ceiling stops this horde too
                self.settle_own(want, 0);
                self.exhausted.store(true, std::sync::atomic::Ordering::SeqCst);
                return Err(e);
            }
        }
        Ok(())
    }
    fn reserve_own(&self, want: i64, can_wait: bool) -> Result<(), RuntimeError> {
        let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if self.exhausted.load(std::sync::atomic::Ordering::SeqCst) || g.0 + want > self.limit || (g.0 + g.1 + want > self.limit && (!can_wait || g.1 == 0)) {
                self.exhausted.store(true, std::sync::atomic::Ordering::SeqCst);
                self.settled.notify_all();
                return Err(RuntimeError::TypeError(format!(
                    "token budget exhausted: horde budget_tokens {} — {} spent, {} reserved by calls in flight, this call needs up to {}",
                    self.limit, g.0, g.1, want)));
            }
            if g.0 + g.1 + want <= self.limit { break; }
            g = self.settled.wait(g).unwrap_or_else(|e| e.into_inner());
        }
        g.1 += want;
        Ok(())
    }
    /// The call is over: `reserved` back, `actual` spent.
    pub fn settle(&self, reserved: i64, actual: i64) {
        self.settle_own(reserved, actual);
        if let Some(p) = &self.parent { p.settle(reserved, actual); }
    }
    fn settle_own(&self, reserved: i64, actual: i64) {
        let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
        g.1 -= reserved;
        g.0 += actual;
        self.settled.notify_all();
    }
    pub fn spent(&self) -> i64 { self.state.lock().unwrap_or_else(|e| e.into_inner()).0 }
}

/// One think()'s reservation; given back (with 0 spent) if the call fails.
pub struct Reservation { budget: Arc<Budget>, want: i64, open: bool }

impl Reservation {
    /// Reserve `want` on a horde budget, if there is one (`can_wait`: the
    /// caller is outside the handler lock, see `Budget::reserve`).
    pub fn take(budget: Option<Arc<Budget>>, want: i64, can_wait: bool) -> Result<Option<Reservation>, RuntimeError> {
        match budget {
            None => Ok(None),
            Some(b) => { b.reserve(want, can_wait)?; Ok(Some(Reservation { budget: b, want, open: true })) }
        }
    }
    pub fn settle(mut self, actual: i64) { self.open = false; self.budget.settle(self.want, actual); }
}

impl Drop for Reservation {
    fn drop(&mut self) { if self.open { self.budget.settle(self.want, 0); } }
}

/// An upper bound of a request's tokens: a token is at least one byte, plus
/// a few per message for the chat format, plus the reply's cap.
pub fn request_bound(request_bytes: usize, messages: usize, max_tokens: u64) -> i64 {
    request_bytes as i64 + 8 * messages as i64 + 16 + max_tokens as i64
}

/// A change to the live registry, applied when the unit that made it commits.
pub(crate) enum Commit {
    Start { spec: Spec, queue: Vec<(usize, Value)>, done: usize, failed: usize, tokens: i64, closed: bool, cancelled: bool },
    Cancel(String),
    /// no workers (`soma test`, a program without serve / run): the horde
    /// runs right after the caller's unit commits, one task per unit — the
    /// caller's next statements run first, as under serve
    Sync { spec: Spec, inputs: Vec<Value> },
}

struct Live {
    spec: Spec,
    inputs: HashMap<usize, Value>,
    queue: VecDeque<usize>,
    attempts: HashMap<usize, u32>,
    running: usize,
    done: usize,
    failed: usize,
    /// tasks stopped by the budget (counted cancelled)
    stopped: usize,
    tokens: i64,
    cancelled: bool,
    /// one worker closes it (apply, on_done)…
    closing: bool,
    /// …and it is closed once that committed (a horde started by on_done
    /// is registered by then: `soma run` does not exit between rounds)
    closed: bool,
    budget: Option<Arc<Budget>>,
}

fn live() -> &'static (Mutex<HashMap<String, Live>>, Condvar) {
    static LIVE: OnceLock<(Mutex<HashMap<String, Live>>, Condvar)> = OnceLock::new();
    LIVE.get_or_init(|| (Mutex::new(HashMap::new()), Condvar::new()))
}

type Pair = (Arc<dyn StorageBackend>, Arc<dyn StorageBackend>);

/// The owner cell's two tables: hordes, tasks.
pub(crate) fn backends(owner: &str) -> Pair {
    static B: OnceLock<Mutex<HashMap<String, Pair>>> = OnceLock::new();
    let mut m = B.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap_or_else(|e| e.into_inner());
    m.entry(owner.to_string()).or_insert_with(|| {
        // under serve / run a horde is always persisted (a program with no
        // [persistent] slot lost its hordes at a restart); `soma test`: memory
        let persistent = (crate::runtime::storage::shared_connection().is_some() || PERSIST.load(std::sync::atomic::Ordering::Relaxed))
            && !super::IN_TEST.load(std::sync::atomic::Ordering::Relaxed);
        if persistent {
            // `-` cannot appear in a slot name: a slot `_hordes` used to BE
            // this table (the program could rewrite its own queue)
            (Arc::new(SqliteBackend::new(owner, "_horde-meta")) as Arc<dyn StorageBackend>,
             Arc::new(SqliteBackend::new(owner, "_horde-tasks")) as Arc<dyn StorageBackend>)
        } else {
            (Arc::new(MemoryBackend::new()) as Arc<dyn StorageBackend>,
             Arc::new(MemoryBackend::new()) as Arc<dyn StorageBackend>)
        }
    }).clone()
}

/// Create the tables of every cell that calls horde(), before any unit
/// opens (a CREATE TABLE inside a rolled-back unit would vanish with it).
/// Set by `prepare` (serve / run): horde tables live in soma.db.
static PERSIST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn prepare(program: &crate::ast::Program) {
    let owners = owners(program);
    if !owners.is_empty() { PERSIST.store(true, std::sync::atomic::Ordering::Relaxed); }
    for owner in owners { let _ = backends(&owner); }
}

/// The cells that call horde() (the only ones with horde tables).
fn owners(program: &crate::ast::Program) -> Vec<String> {
    let mut out = Vec::new();
    for c in &program.cells {
        let mut calls = false;
        for sec in &c.node.sections {
            let body = match &sec.node {
                crate::ast::Section::OnSignal(on) => &on.body,
                crate::ast::Section::Every(e) | crate::ast::Section::After(e) => &e.body,
                _ => continue,
            };
            crate::checker::literals::for_each_expr(body, &mut |e| if matches!(e, crate::ast::Expr::FnCall { name, .. } if name == "horde") { calls = true; });
        }
        if calls { out.push(c.node.name.clone()); }
    }
    out
}

fn int(n: i64) -> Value { Value::Int(SomaInt::from_i64(n)) }
fn s(x: &str) -> Value { Value::String(x.to_string()) }
fn field<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    if let Value::Map(m) = v { m.get(k) } else { None }
}
fn field_str(v: &Value, k: &str) -> Option<String> {
    match field(v, k) { Some(Value::String(x)) => Some(x.clone()), _ => None }
}
fn field_int(v: &Value, k: &str) -> i64 {
    match field(v, k) { Some(Value::Int(i)) => i.to_i64().unwrap_or(0), _ => 0 }
}
fn task_key(id: &str, idx: usize) -> String { format!("{}/{:07}", id, idx) }

/// An id names its owner cell (`Audit:h3`): status / results / cancel work
/// from any cell, a test rule included.
fn owner_of<'a>(id: &'a str, fallback: &'a str) -> &'a str {
    id.split_once(':').map(|(o, _)| o).unwrap_or(fallback)
}

impl Interpreter {
    /// A write to a horde table, undone with the unit (or the `try`).
    fn horde_set(&mut self, b: &Arc<dyn StorageBackend>, key: &str, v: &Value) {
        if let Some(j) = self.journal.as_mut() {
            j.push(UndoOp::Restore { backend: b.clone(), key: key.to_string(), prev: b.get(key) });
        }
        b.set(key, value_to_stored(v));
    }

    /// Apply `c` when the current unit commits (now, outside any unit).
    fn horde_on_commit(&mut self, c: Commit) {
        match (self.journal.as_mut(), c) {
            (Some(j), c) => j.push(UndoOp::Horde(c)),
            (None, Commit::Sync { spec, inputs }) => { self.deferred_hordes.push((spec, inputs)); self.run_deferred_hordes(); }
            (None, c) => apply_commit(c),
        }
    }

    /// Which (cell, handler) a horde target names: `"Cell.h"`, or `"h"` in
    /// the calling cell, else the one cell that defines it.
    fn horde_target(&self, owner: &str, target: &str) -> Result<(String, String), RuntimeError> {
        let defines = |c: &str, h: &str| self.cells.get(c).map_or(false, |cell| cell.sections.iter().any(|s| matches!(&s.node, crate::ast::Section::OnSignal(on) if on.signal_name == h)));
        if let Some((c, h)) = target.split_once('.') {
            if defines(c, h) { return Ok((c.to_string(), h.to_string())); }
            return Err(RuntimeError::TypeError(format!("horde(): no handler `{}` in cell `{}`", h, c)));
        }
        if defines(owner, target) { return Ok((owner.to_string(), target.to_string())); }
        let mut found: Vec<String> = self.cells.keys().filter(|c| defines(c, target)).cloned().collect();
        found.sort();
        match found.len() {
            1 => Ok((found.remove(0), target.to_string())),
            0 => Err(RuntimeError::TypeError(format!("horde(): no handler `{}` — name it `Cell.handler`", target))),
            _ => Err(RuntimeError::TypeError(format!("horde(): `{}` is defined in {} — name it `Cell.handler`", target, found.join(", ")))),
        }
    }

    fn handler_params(&self, cell: &str, h: &str) -> Option<usize> {
        self.cells.get(cell).and_then(|c| c.sections.iter().find_map(|s| match &s.node {
            crate::ast::Section::OnSignal(on) if on.signal_name == h => Some(on.params.len()),
            _ => None,
        }))
    }

    /// `horde(handler, inputs, opts)`: returns the horde's id.
    pub(crate) fn horde_start(&mut self, owner: &str, target: &str, inputs: Vec<Value>, opts: Option<&Value>) -> Result<Value, RuntimeError> {
        let (target_cell, handler) = self.horde_target(owner, target)?;
        let mut spec = Spec {
            id: String::new(), owner: owner.to_string(), target_cell, handler,
            on_result: None, on_done: None, on_error: None,
            concurrency: 8, max_attempts: 1, total: inputs.len(), budget_tokens: None,
            snapshot: None, apply: None, seed: None, instance: None,
            parent: self.horde_current.clone(), parent_budget: self.horde_budget.clone(),
        };
        if let Some(o) = opts {
            let Value::Map(m) = o else { return Err(RuntimeError::TypeError("horde(): opts must be a map, e.g. map(\"concurrency\", 50)".to_string())) };
            for (k, v) in m.iter() {
                match (k.as_str(), v) {
                    ("concurrency", Value::Int(i)) => {
                        let n = i.to_i64().unwrap_or(0);
                        if n < 1 || n as usize > MAX_CONCURRENCY {
                            return Err(RuntimeError::TypeError(format!("horde(): concurrency must be 1..{} (got {})", MAX_CONCURRENCY, n)));
                        }
                        spec.concurrency = n as usize;
                    }
                    ("max_attempts", Value::Int(i)) => {
                        let n = i.to_i64().unwrap_or(0);
                        if !(1..=10).contains(&n) { return Err(RuntimeError::TypeError(format!("horde(): max_attempts must be 1..10 (got {})", n))); }
                        spec.max_attempts = n as u32;
                    }
                    ("budget_tokens", Value::Int(i)) => {
                        let n = i.to_i64().unwrap_or(0);
                        if n < 1 { return Err(RuntimeError::TypeError(format!("horde(): budget_tokens must be positive (got {})", n))); }
                        spec.budget_tokens = Some(n);
                    }
                    ("snapshot", v) => spec.snapshot = Some(v.clone()),
                    ("seed", Value::Int(i)) => spec.seed = Some(i.to_i64().unwrap_or(0)),
                    ("instance", Value::String(f)) => spec.instance = Some(f.clone()),
                    ("on_result" | "on_done" | "on_error" | "apply", Value::String(h)) => {
                        let want: &[usize] = match k.as_str() { "on_result" | "apply" => &[1, 2], "on_done" => &[1], _ => &[2] };
                        match self.handler_params(owner, h) {
                            Some(n) if want.contains(&n) => {}
                            Some(n) => return Err(RuntimeError::TypeError(format!(
                                "horde(): {} handler `{}` takes {} parameter(s); it is called with {}", k, h, n,
                                match k.as_str() { "on_result" | "apply" => "(result) or (input, result)", "on_done" => "(horde_id)", _ => "(input, error)" }))),
                            None => return Err(RuntimeError::TypeError(format!("horde(): {} names `{}`, which is not a handler of cell `{}`", k, h, owner))),
                        }
                        match k.as_str() { "on_result" => spec.on_result = Some(h.clone()), "apply" => spec.apply = Some(h.clone()), "on_done" => spec.on_done = Some(h.clone()), _ => spec.on_error = Some(h.clone()) }
                    }
                    (key, _) => return Err(RuntimeError::TypeError(format!(
                        "horde(): unknown or mistyped option `{}` — options: concurrency (Int), max_attempts (Int), budget_tokens (Int), seed (Int), snapshot (any value), instance (field name), on_result, apply, on_done, on_error (handler names)", key))),
                }
            }
        }
        if spec.on_result.is_some() && spec.apply.is_some() {
            return Err(RuntimeError::TypeError("horde(): on_result (as results arrive) and apply (in input order, at the end) exclude each other — pick one".to_string()));
        }
        let want_params = if spec.snapshot.is_some() { 2 } else { 1 };
        if self.handler_params(&spec.target_cell, &spec.handler) != Some(want_params) {
            return Err(RuntimeError::TypeError(if want_params == 2 {
                format!("horde(): with a snapshot, `{}.{}` takes two parameters (input, snapshot)", spec.target_cell, spec.handler)
            } else {
                format!("horde(): `{}.{}` must take exactly one parameter (the input) — or two (input, snapshot) with a `snapshot` option", spec.target_cell, spec.handler)
            }));
        }
        if let Some(f) = &spec.instance {
            if let Some(bad) = inputs.iter().position(|v| !matches!(field(v, f), Some(Value::String(_)) | Some(Value::Int(_)))) {
                return Err(RuntimeError::TypeError(format!("horde(): instance `{}`: input {} has no String/Int field `{}` naming its agent", f, bad, f)));
            }
        }
        let (hordes, tasks) = backends(owner);
        let seq = hordes.get("__seq").map(stored_to_value).map(|v| match v { Value::Int(i) => i.to_i64().unwrap_or(0), _ => 0 }).unwrap_or(0) + 1;
        if let Some(j) = self.journal.as_mut() {
            j.push(UndoOp::Counter { backend: hordes.clone(), key: "__seq".to_string(), prev: hordes.get("__seq") });
        }
        hordes.set("__seq", value_to_stored(&int(seq)));
        spec.id = format!("{}:h{}", owner, seq);
        let meta = map_from_pairs(vec![
            ("target".to_string(), s(&format!("{}.{}", spec.target_cell, spec.handler))),
            ("total".to_string(), int(spec.total as i64)),
            ("concurrency".to_string(), int(spec.concurrency as i64)),
            ("max_attempts".to_string(), int(spec.max_attempts as i64)),
            ("budget_tokens".to_string(), spec.budget_tokens.map(int).unwrap_or(Value::Unit)),
            ("snapshot".to_string(), spec.snapshot.clone().unwrap_or(Value::Unit)),
            ("has_snapshot".to_string(), Value::Bool(spec.snapshot.is_some())),
            ("apply".to_string(), spec.apply.clone().map(Value::String).unwrap_or(Value::Unit)),
            ("seed".to_string(), spec.seed.map(int).unwrap_or(Value::Unit)),
            ("instance".to_string(), spec.instance.clone().map(Value::String).unwrap_or(Value::Unit)),
            ("parent".to_string(), spec.parent.clone().map(Value::String).unwrap_or(Value::Unit)),
            // a nested horde without its own ceiling runs under its parent's
            ("parent_budget".to_string(), spec.parent_budget.as_ref().map(|b| int(b.limit)).unwrap_or(Value::Unit)),
            ("on_result".to_string(), spec.on_result.clone().map(Value::String).unwrap_or(Value::Unit)),
            ("on_done".to_string(), spec.on_done.clone().map(Value::String).unwrap_or(Value::Unit)),
            ("on_error".to_string(), spec.on_error.clone().map(Value::String).unwrap_or(Value::Unit)),
            ("state".to_string(), s("running")),
            ("closed".to_string(), Value::Bool(false)),
        ]);
        self.horde_set(&hordes, &spec.id, &meta);
        for (i, input) in inputs.iter().enumerate() {
            let rec = map_from_pairs(vec![("input".to_string(), input.clone()), ("state".to_string(), s("queued")), ("attempts".to_string(), int(0))]);
            self.horde_set(&tasks, &task_key(&spec.id, i), &rec);
        }
        let id = spec.id.clone();
        let workers = FACTORY.get().is_some() && !super::IN_TEST.load(std::sync::atomic::Ordering::Relaxed);
        if workers {
            self.horde_claim(owner, &id);
            let queue = inputs.into_iter().enumerate().collect();
            self.horde_on_commit(Commit::Start { spec, queue, done: 0, failed: 0, tokens: 0, closed: false, cancelled: false });
        } else {
            // (its status is read from the tables: nothing to register)
            self.horde_on_commit(Commit::Sync { spec, inputs });
        }
        Ok(Value::String(id))
    }

    /// Run the hordes whose caller has committed (no workers): outermost
    /// only — a horde started by an on_done waits for the one running.
    pub(crate) fn run_deferred_hordes(&mut self) {
        if self.running_deferred || self.journal.is_some() || self.task_unit.is_some() { return; }
        self.running_deferred = true;
        while !self.deferred_hordes.is_empty() {
            let (spec, inputs) = self.deferred_hordes.remove(0);
            self.horde_run_sync(spec, inputs);
        }
        self.running_deferred = false;
    }

    fn horde_is_cancelled(&self, spec: &Spec) -> bool {
        let (hordes, _) = backends(&spec.owner);
        hordes.get(&spec.id).map(stored_to_value).and_then(|m| field_str(&m, "state")).as_deref() == Some("cancelled")
    }

    /// Every task in input order, each (its last step and its result) in
    /// its own units — as a worker runs it.
    fn horde_run_sync(&mut self, spec: Spec, inputs: Vec<Value>) {
        let saved_budget = self.horde_budget.take();
        let saved_current = self.horde_current.replace(spec.id.clone());
        self.horde_budget = Budget::for_horde(spec.budget_tokens, 0, spec.parent_budget.clone());
        let convs = std::mem::take(&mut self.agent_conversations);
        let conv = std::mem::take(&mut self.agent_conversation);
        let (used, cap) = (self.agent_tokens_used, self.agent_token_budget);
        let exhausted = |me: &Interpreter| me.horde_budget.as_ref().map_or(false, |b| b.is_exhausted());
        for (idx, input) in inputs.into_iter().enumerate() {
            if exhausted(self) || self.horde_is_cancelled(&spec) { break; }
            let mut attempt = 0u32;
            loop {
                attempt += 1;
                match run_one(self, &spec, idx, &input, attempt) {
                    Outcome::Retry(_) => continue,
                    _ => break,
                }
            }
        }
        if exhausted(self) { let _ = self.atomically(|me| -> Result<(), RuntimeError> { me.horde_mark_exhausted(&spec); Ok(()) }); }
        self.agent_conversations.clear();
        self.agent_conversation.clear();
        self.horde_apply_all(&spec);
        if let Err(e) = self.atomically(|me| me.horde_close(&spec)) {
            eprintln!("[horde {}] on_done `{}` failed: {}", spec.id, spec.on_done.clone().unwrap_or_default(), e);
            let _ = self.atomically(|me| { let mut sp = spec.clone(); sp.on_done = None; me.horde_close(&sp) });
        }
        self.agent_conversations = convs;
        self.agent_conversation = conv;
        self.agent_tokens_used = used;
        self.agent_token_budget = cap;
        self.horde_budget = saved_budget;
        self.horde_current = saved_current;
    }

    /// Before a task (each attempt): its seed, its agent instance (memory and
    /// conversation), its arguments.
    fn horde_task_begin(&mut self, spec: &Spec, idx: usize, input: &Value, attempt: u32) -> Vec<Value> {
        if let Some(seed) = spec.seed {
            // splitmix of (seed, index, attempt): the same draws on every run
            let mut z = (seed as u64) ^ (idx as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ (attempt as u64).wrapping_mul(0xD1B54A32D192ED03);
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            super::builtins::math::set_rng_state((z ^ (z >> 31)) | 1);
        }
        self.horde_instance = spec.instance.as_ref().and_then(|f| field(input, f)).map(|v| match v { Value::String(x) => x.clone(), other => format!("{}", other) });
        if let Some(inst) = self.horde_instance.clone() {
            let mem = super::builtins::storage::agent_memory(self, &spec.target_cell);
            if let Some(Value::List(msgs)) = mem.get(&format!("{}/__conversation", inst)).map(stored_to_value) {
                let conv: Vec<serde_json::Value> = msgs.iter().filter_map(|m| match m { Value::String(j) => serde_json::from_str(j).ok(), _ => None }).collect();
                self.agent_conversations.insert(spec.target_cell.clone(), conv);
            }
        }
        match &spec.snapshot { Some(snap) => vec![input.clone(), snap.clone()], None => vec![input.clone()] }
    }

    /// Record a task's result and call `on_result` — inside the unit of the
    /// target's last step. `Ok(false)`: another process already did.
    fn horde_finish(&mut self, spec: &Spec, idx: usize, input: &Value, result: &Value, tokens: i64, attempts: u32) -> Result<bool, RuntimeError> {
        let (_, tasks) = backends(&spec.owner);
        let key = task_key(&spec.id, idx);
        if let Some(prev) = tasks.get(&key).map(stored_to_value) {
            if field_str(&prev, "state").as_deref() == Some("done") { return Ok(false); }
        }
        if let Some(h) = spec.on_result.clone() {
            let args = if self.handler_params(&spec.owner, &h) == Some(2) { vec![input.clone(), result.clone()] } else { vec![result.clone()] };
            self.call_signal(&spec.owner, &h, args)?;
        }
        // the agent instance's conversation, for its next round
        if let Some(inst) = self.horde_instance.clone() {
            let conv: Vec<Value> = self.agent_conversations.get(&spec.target_cell).map(|c| c.iter().map(|m| Value::String(m.to_string())).collect()).unwrap_or_default();
            let mem = super::builtins::storage::agent_memory(self, &spec.target_cell);
            self.horde_set(&mem, &format!("{}/__conversation", inst), &Value::List(conv));
        }
        let rec = map_from_pairs(vec![
            ("input".to_string(), input.clone()), ("state".to_string(), s("done")),
            ("result".to_string(), result.clone()), ("tokens".to_string(), int(tokens)),
            ("attempts".to_string(), int(attempts as i64)),
            // `apply`: called at the end, in input order (then true)
            ("applied".to_string(), Value::Bool(spec.apply.is_none())),
        ]);
        self.horde_set(&tasks, &key, &rec);
        Ok(true)
    }

    /// A task out of attempts: `on_error(input, error)` (its failure does not
    /// stop the record) and the task marked failed.
    fn horde_fail(&mut self, spec: &Spec, idx: usize, input: &Value, e: &RuntimeError, attempts: u32) {
        if self.horde_task_done(spec, idx) { return; }
        let msg = format!("{}", e);
        if let Some(h) = spec.on_error.clone() {
            let mark = self.journal.as_ref().map_or(0, |j| j.len());
            // what `try` gives: {error, kind, detail}
            let err_v = map_from_pairs(vec![
                ("error".to_string(), Value::String(msg.clone())),
                ("kind".to_string(), Value::String(e.kind())),
                ("detail".to_string(), Value::String(e.detail())),
            ]);
            if let Err(err) = self.call_signal(&spec.owner, &h, vec![input.clone(), err_v]) {
                self.rollback_to(mark);
                eprintln!("[horde {}] on_error `{}` failed: {}", spec.id, h, err);
            }
        }
        let (_, tasks) = backends(&spec.owner);
        let rec = map_from_pairs(vec![
            ("input".to_string(), input.clone()), ("state".to_string(), s("failed")),
            ("error".to_string(), Value::String(msg)), ("kind".to_string(), Value::String(e.kind())),
            ("attempts".to_string(), int(attempts as i64)),
        ]);
        self.horde_set(&tasks, &task_key(&spec.id, idx), &rec);
    }

    /// Recorded done (by this process or another): never overwritten.
    fn horde_task_done(&self, spec: &Spec, idx: usize) -> bool {
        let (_, tasks) = backends(&spec.owner);
        tasks.get(&task_key(&spec.id, idx)).map(stored_to_value).and_then(|t| field_str(&t, "state")).as_deref() == Some("done")
    }

    /// A task the budget stopped: counted cancelled, no on_error.
    fn horde_mark_stopped(&mut self, spec: &Spec, idx: usize, input: &Value) {
        if self.horde_task_done(spec, idx) { return; }
        let (_, tasks) = backends(&spec.owner);
        let rec = map_from_pairs(vec![("input".to_string(), input.clone()), ("state".to_string(), s("cancelled")), ("error".to_string(), s("stopped: budget_tokens exhausted"))]);
        self.horde_set(&tasks, &task_key(&spec.id, idx), &rec);
    }

    /// The budget ran out: the horde's state says so (the rest never runs).
    fn horde_mark_exhausted(&mut self, spec: &Spec) {
        let (hordes, _) = backends(&spec.owner);
        let Some(Value::Map(mut m)) = hordes.get(&spec.id).map(stored_to_value) else { return };
        if matches!(m.get("state"), Some(Value::String(st)) if st == "running") {
            m.insert("state".to_string(), s("exhausted"));
            self.horde_set(&hordes, &spec.id, &Value::Map(m));
        }
    }

    /// `apply`: every done task's result, in INPUT order, each in its own
    /// unit with its `applied` mark (exactly once across a restart). An
    /// apply that raises marks its task failed.
    fn horde_apply_all(&mut self, spec: &Spec) {
        let Some(h) = spec.apply.clone() else { return };
        let (_, tasks) = backends(&spec.owner);
        let two = self.handler_params(&spec.owner, &h) == Some(2);
        for idx in 0..spec.total {
            let key = task_key(&spec.id, idx);
            let Some(rec) = tasks.get(&key).map(stored_to_value) else { continue };
            if field_str(&rec, "state").as_deref() != Some("done") || matches!(field(&rec, "applied"), Some(Value::Bool(true))) { continue; }
            let input = field(&rec, "input").cloned().unwrap_or(Value::Unit);
            let result = field(&rec, "result").cloned().unwrap_or(Value::Unit);
            let args = if two { vec![input.clone(), result] } else { vec![result] };
            let step = |me: &mut Interpreter| -> Result<(), RuntimeError> {
                // re-read inside the unit: another process may have applied it
                let Some(Value::Map(mut m)) = tasks.get(&key).map(stored_to_value) else { return Ok(()) };
                if matches!(m.get("applied"), Some(Value::Bool(true))) { return Ok(()); }
                me.call_signal(&spec.owner, &h, args.clone())?;
                m.insert("applied".to_string(), Value::Bool(true));
                me.horde_set(&tasks, &key, &Value::Map(m));
                Ok(())
            };
            let r = self.atomically(step);
            if let Err(e) = r {
                eprintln!("[horde {}] apply `{}` failed for task {}: {}", spec.id, h, idx, e);
                let fail = |me: &mut Interpreter| -> Result<(), RuntimeError> {
                    if let Some(Value::Map(mut m)) = tasks.get(&key).map(stored_to_value) {
                        m.insert("state".to_string(), s("failed"));
                        m.insert("error".to_string(), Value::String(format!("apply: {}", e)));
                        me.horde_set(&tasks, &key, &Value::Map(m));
                    }
                    Ok(())
                };
                let _ = self.atomically(fail);
            }
        }
    }

    /// Nothing left to run: mark the horde closed and call `on_done` — once.
    fn horde_close(&mut self, spec: &Spec) -> Result<(), RuntimeError> {
        let (hordes, _) = backends(&spec.owner);
        let Some(meta) = hordes.get(&spec.id).map(stored_to_value) else { return Ok(()) };
        if matches!(field(&meta, "closed"), Some(Value::Bool(true))) { return Ok(()); }
        let mut m = match meta { Value::Map(m) => m, _ => return Ok(()) };
        if matches!(m.get("state"), Some(Value::String(st)) if st == "running") { m.insert("state".to_string(), s("done")); }
        m.insert("closed".to_string(), Value::Bool(true));
        self.horde_set(&hordes, &spec.id, &Value::Map(m));
        if let Some(h) = spec.on_done.clone() {
            self.call_signal(&spec.owner, &h, vec![Value::String(spec.id.clone())])?;
        }
        Ok(())
    }

    /// `horde_status(id)`: counts, tokens and state.
    pub(crate) fn horde_status(&mut self, owner: &str, id: &str) -> Result<Value, RuntimeError> {
        let caller = owner;
        let owner = owner_of(id, owner);
        // a horde is its cell's data: another cell asks one of its handlers
        // (test rules read anything)
        if caller != owner && !caller.is_empty() && !self.is_test_cell(caller) {
            return Err(RuntimeError::Domain { kind: "not_found".to_string(), message: format!("horde `{}` belongs to cell `{}` — ask a handler of `{}`", id, owner, owner) });
        }
        // cancelled by this very unit (applied at its commit)
        let pending_cancel = self.journal.as_ref().map_or(false, |j| j.iter().any(|u| matches!(u, UndoOp::Horde(Commit::Cancel(x)) if x == id)));
        {
            let reg = live().0.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(l) = reg.get(id) {
                let over = l.budget.as_ref().map_or(false, |b| b.is_exhausted());
                let stopped = l.cancelled || over || pending_cancel;
                let queued = if stopped { 0 } else { l.queue.len() };
                let cancelled = if stopped { l.queue.len() } else { 0 } + l.stopped;
                let state = match (l.closed, l.cancelled, over) {
                    (true, true, _) => "cancelled", (true, _, true) => "exhausted", (true, _, _) => "done",
                    (false, true, _) => "cancelling", (false, _, true) => "exhausting",
                    _ if pending_cancel => "cancelling", _ => "running",
                };
                // the budget's own count: think()s of callbacks included
                let tokens = l.budget.as_ref().map_or(l.tokens, |b| b.spent());
                let mut m = status_map(state, l.spec.total, queued, l.running, l.done, l.failed, cancelled, tokens);
                if let (Value::Map(mm), Some(b)) = (&mut m, l.spec.budget_tokens) { mm.insert("budget_tokens".to_string(), int(b)); }
                return Ok(m);
            }
        }
        // not live in this process: read the tables
        let (hordes, tasks) = backends(owner);
        let Some(meta) = hordes.get(id).map(stored_to_value) else {
            return Err(RuntimeError::Domain { kind: "not_found".to_string(), message: format!("no horde `{}` in cell `{}`", id, owner) });
        };
        let total = field_int(&meta, "total") as usize;
        let (mut queued, mut done, mut failed, mut tokens, mut stopped) = (0, 0, 0, 0i64, 0);
        for i in 0..total {
            let Some(t) = tasks.get(&task_key(id, i)).map(stored_to_value) else { continue };
            match field_str(&t, "state").as_deref() {
                Some("done") => { done += 1; tokens += field_int(&t, "tokens"); }
                Some("failed") => failed += 1,
                Some("cancelled") => stopped += 1,
                _ => queued += 1,
            }
        }
        let mut state = field_str(&meta, "state").unwrap_or_default();
        // unclosed, with workers here, yet not live: a restart refused to
        // resume it (the program changed) — it waits
        let elsewhere = hordes.get(&lease_key(id)).map(stored_to_value)
            .map_or(false, |l| field_str(&l, "runner").unwrap_or_default() != me() && now_ms() - field_int(&l, "beat") < LEASE_STALE_MS);
        if state == "running" && !elsewhere && !matches!(field(&meta, "closed"), Some(Value::Bool(true))) && FACTORY.get().is_some() && !super::IN_TEST.load(std::sync::atomic::Ordering::Relaxed) {
            state = "paused".to_string();
        }
        let (queued, cancelled) = if state == "cancelled" || state == "exhausted" { (0, queued + stopped) } else { (queued, stopped) };
        let mut m = status_map(&state, total, queued, 0, done, failed, cancelled, tokens);
        if let (Value::Map(mm), Some(Value::Int(b))) = (&mut m, field(&meta, "budget_tokens")) { mm.insert("budget_tokens".to_string(), Value::Int(b.clone())); }
        Ok(m)
    }

    /// `horde_results(id)`: the results in input order (() for a task not done).
    pub(crate) fn horde_results(&mut self, owner: &str, id: &str) -> Result<Value, RuntimeError> {
        let caller = owner;
        let owner = owner_of(id, owner);
        // a horde is its cell's data: another cell asks one of its handlers
        // (test rules read anything)
        if caller != owner && !caller.is_empty() && !self.is_test_cell(caller) {
            return Err(RuntimeError::Domain { kind: "not_found".to_string(), message: format!("horde `{}` belongs to cell `{}` — ask a handler of `{}`", id, owner, owner) });
        }
        let (hordes, tasks) = backends(owner);
        let Some(meta) = hordes.get(id).map(stored_to_value) else {
            return Err(RuntimeError::Domain { kind: "not_found".to_string(), message: format!("no horde `{}` in cell `{}`", id, owner) });
        };
        let total = field_int(&meta, "total") as usize;
        Ok(Value::List((0..total).map(|i| tasks.get(&task_key(id, i)).map(stored_to_value)
            .and_then(|t| if field_str(&t, "state").as_deref() == Some("done") { field(&t, "result").cloned() } else { None })
            .unwrap_or(Value::Unit)).collect()))
    }

    /// `horde_cancel(id)`: no new task starts; running ones finish.
    pub(crate) fn horde_cancel(&mut self, owner: &str, id: &str) -> Result<Value, RuntimeError> {
        let caller = owner;
        let owner = owner_of(id, owner);
        // a horde is its cell's data: another cell asks one of its handlers
        // (test rules read anything)
        if caller != owner && !caller.is_empty() && !self.is_test_cell(caller) {
            return Err(RuntimeError::Domain { kind: "not_found".to_string(), message: format!("horde `{}` belongs to cell `{}` — ask a handler of `{}`", id, owner, owner) });
        }
        let (hordes, _) = backends(owner);
        let Some(meta) = hordes.get(id).map(stored_to_value) else {
            return Err(RuntimeError::Domain { kind: "not_found".to_string(), message: format!("no horde `{}` in cell `{}`", id, owner) });
        };
        if field_str(&meta, "state").as_deref() != Some("running") { return Ok(Value::Bool(false)); }
        let Value::Map(mut m) = meta else { return Ok(Value::Bool(false)) };
        m.insert("state".to_string(), s("cancelled"));
        self.horde_set(&hordes, id, &Value::Map(m));
        self.horde_on_commit(Commit::Cancel(id.to_string()));
        Ok(Value::Bool(true))
    }
}

#[allow(clippy::too_many_arguments)]
fn status_map(state: &str, total: usize, queued: usize, running: usize, done: usize, failed: usize, cancelled: usize, tokens: i64) -> Value {
    map_from_pairs(vec![
        ("state".to_string(), s(state)), ("total".to_string(), int(total as i64)),
        ("queued".to_string(), int(queued as i64)), ("running".to_string(), int(running as i64)),
        ("done".to_string(), int(done as i64)), ("failed".to_string(), int(failed as i64)),
        ("cancelled".to_string(), int(cancelled as i64)), ("tokens".to_string(), int(tokens)),
    ])
}

/// Called by `unit_end` for each committed `Commit`.
pub(crate) fn apply_commit(c: Commit) {
    match c {
        Commit::Sync { .. } => {}
        Commit::Start { spec, queue, done, failed, tokens, closed, cancelled } => {
            let n = queue.len();
            let id = spec.id.clone();
            let workers = spec.concurrency.min(n).max(if closed { 0 } else { 1 });
            {
                let mut reg = live().0.lock().unwrap_or_else(|e| e.into_inner());
                reg.insert(id.clone(), Live {
                    spec: spec.clone(),
                    queue: queue.iter().map(|(i, _)| *i).collect(),
                    inputs: queue.into_iter().collect(),
                    attempts: HashMap::new(), running: 0, done, failed, stopped: 0, tokens,
                    cancelled, closing: closed, closed,
                    budget: Budget::for_horde(spec.budget_tokens, tokens, spec.parent_budget.clone()),
                });
            }
            if closed { live().1.notify_all(); return; }
            for _ in 0..workers {
                let id = id.clone();
                super::spawn_handler_thread(move || worker(&id));
            }
        }
        Commit::Cancel(id) => {
            let mut reg = live().0.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(l) = reg.get_mut(&id) { l.cancelled = true; }
        }
    }
}

enum Outcome { Done(i64), Retry(i64), Failed(i64), Stopped(i64) }

fn worker(id: &str) {
    let Some(factory) = FACTORY.get() else { return };
    let mut interp = factory();
    interp.horde_budget = live().0.lock().unwrap_or_else(|e| e.into_inner()).get(id).and_then(|l| l.budget.clone());
    interp.horde_current = Some(id.to_string());
    let exhausted = |l: &Live| l.budget.as_ref().map_or(false, |b| b.is_exhausted());
    loop {
        let next = {
            let mut reg = live().0.lock().unwrap_or_else(|e| e.into_inner());
            let Some(l) = reg.get_mut(id) else { return };
            if l.cancelled || exhausted(l) { None } else {
                l.queue.pop_front().map(|i| {
                    l.running += 1;
                    let a = *l.attempts.get(&i).unwrap_or(&0) + 1;
                    (l.spec.clone(), i, l.inputs.get(&i).cloned().unwrap_or(Value::Unit), a)
                })
            }
        };
        let Some((spec, idx, input, attempt)) = next else { break };
        let outcome = run_one(&mut interp, &spec, idx, &input, attempt);
        let mut reg = live().0.lock().unwrap_or_else(|e| e.into_inner());
        let Some(l) = reg.get_mut(id) else { return };
        l.running -= 1;
        match outcome {
            Outcome::Done(t) => { l.done += 1; l.tokens += t; l.inputs.remove(&idx); }
            Outcome::Retry(t) => { l.tokens += t; l.attempts.insert(idx, attempt); l.queue.push_back(idx); }
            Outcome::Failed(t) => { l.failed += 1; l.tokens += t; l.inputs.remove(&idx); }
            Outcome::Stopped(t) => { l.stopped += 1; l.tokens += t; l.inputs.remove(&idx); }
        }
    }
    // the last worker out closes the horde
    let spec = {
        let mut reg = live().0.lock().unwrap_or_else(|e| e.into_inner());
        let Some(l) = reg.get_mut(id) else { return };
        if l.running > 0 || l.closing || (!l.cancelled && !exhausted(l) && !l.queue.is_empty()) { return; }
        l.closing = true;
        (l.spec.clone(), exhausted(l))
    };
    let (spec, out_of_budget) = spec;
    if out_of_budget { let _ = interp.atomically(|me| -> Result<(), RuntimeError> { me.horde_mark_exhausted(&spec); Ok(()) }); }
    interp.agent_conversations.clear();
    interp.agent_conversation.clear();
    interp.horde_apply_all(&spec);
    if let Err(e) = interp.atomically(|me| me.horde_close(&spec)) {
        eprintln!("[horde {}] on_done `{}` failed: {}", spec.id, spec.on_done.clone().unwrap_or_default(), e);
        // the close is recorded even when on_done raises
        let _ = interp.atomically(|me| { let mut sp = spec.clone(); sp.on_done = None; me.horde_close(&sp) });
    }
    if let Some(l) = live().0.lock().unwrap_or_else(|e| e.into_inner()).get_mut(id) { l.closed = true; }
    live().1.notify_all();
}

/// One task: the target handler as `[task]` steps, its result recorded in
/// the unit of its last step.
fn run_one(interp: &mut Interpreter, spec: &Spec, idx: usize, input: &Value, attempt: u32) -> Outcome {
    interp.agent_tokens_used = 0;
    interp.agent_token_budget = 0;
    interp.current_depth = 0;
    // each task starts a fresh model context: a worker's interpreter runs
    // many tasks, and one input's conversation must not reach the next
    interp.agent_conversations.clear();
    interp.agent_conversation.clear();
    interp.agent_trace.clear();
    let task = interp.handler_is_task(&spec.target_cell, &spec.handler);
    let args = interp.horde_task_begin(spec, idx, input, attempt);
    let body = |me: &mut Interpreter| -> Result<bool, RuntimeError> {
        let v = me.call_signal_inner(&spec.target_cell, &spec.handler, args)?;
        let spent = me.agent_tokens_used;
        me.horde_finish(spec, idx, input, &v, spent, attempt)
    };
    let r = if task { interp.run_task(body) } else { interp.atomically(body) };
    interp.horde_instance = None;
    let spent = interp.agent_tokens_used;
    match r {
        Ok(true) => Outcome::Done(spent),
        // another process recorded it: this run's last step was not kept
        Ok(false) => Outcome::Done(0),
        // stopped by the budget: not a failure of the input (no on_error)
        Err(e) if e.kind() == "budget" && interp.horde_budget.as_ref().map_or(false, |b| b.is_exhausted()) => {
            let _ = interp.atomically(|me| -> Result<(), RuntimeError> { me.horde_mark_stopped(spec, idx, input); Ok(()) });
            Outcome::Stopped(spent)
        }
        Err(e) if attempt < spec.max_attempts && !interp.horde_budget.as_ref().map_or(false, |b| b.is_exhausted()) => {
            eprintln!("[horde {}] task {} attempt {} failed: {}", spec.id, idx, attempt, e);
            Outcome::Retry(spent)
        }
        Err(e) => {
            let _ = interp.atomically(|me| -> Result<(), RuntimeError> { me.horde_fail(spec, idx, input, &e, attempt); Ok(()) });
            Outcome::Failed(spent)
        }
    }
}

/// `soma run`: wait until every horde started in this process is closed.
pub fn wait_all() {
    let (m, cv) = live();
    let mut reg = m.lock().unwrap_or_else(|e| e.into_inner());
    while reg.values().any(|l| !l.closed) {
        reg = cv.wait_timeout(reg, std::time::Duration::from_millis(200)).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
    }
}

/// Start-up: hordes left running by a stopped process resume — tasks not
/// recorded done/failed run again (their result is written once).
pub fn recover(program: &crate::ast::Program) {
    if FACTORY.get().is_none() || crate::runtime::storage::shared_connection().is_none() { return; }
    scan(program);
    // another process on the same data may stop: its hordes' leases go
    // stale and this one takes them over; ours are kept fresh
    let prog = program.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        heartbeat();
        scan(&prog);
    });
}

/// A horde runs in ONE process at a time: a lease {runner, beat} per horde,
/// renewed every second by the process that runs it; stale after 5 s.
const LEASE_STALE_MS: i64 = 5000;
fn now_ms() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) }
fn lease_key(id: &str) -> String { format!("__lease/{}", id) }
fn me() -> String { format!("{:016x}", super::process_nonce()) }

impl Interpreter {
    /// Take (or renew) the lease of `id` for this process — refused when
    /// another live process holds it.
    fn horde_claim(&mut self, owner: &str, id: &str) -> bool {
        let (hordes, _) = backends(owner);
        let key = lease_key(id);
        let mine = me();
        if let Some(l) = hordes.get(&key).map(stored_to_value) {
            let runner = field_str(&l, "runner").unwrap_or_default();
            if runner != mine && now_ms() - field_int(&l, "beat") < LEASE_STALE_MS { return false; }
        }
        let v = map_from_pairs(vec![("runner".to_string(), Value::String(mine)), ("beat".to_string(), int(now_ms()))]);
        self.horde_set(&hordes, &key, &v);
        true
    }
}

/// Renew the leases of the hordes running here.
fn heartbeat() {
    let ids: Vec<(String, String)> = live().0.lock().unwrap_or_else(|e| e.into_inner()).values()
        .filter(|l| !l.closed).map(|l| (l.spec.owner.clone(), l.spec.id.clone())).collect();
    if ids.is_empty() { return; }
    let Some(factory) = FACTORY.get() else { return };
    let mut interp = factory();
    let _ = interp.atomically(|me| -> Result<(), RuntimeError> { for (o, id) in &ids { me.horde_claim(o, id); } Ok(()) });
}

/// Resume the unclosed hordes no live process runs.
fn scan(program: &crate::ast::Program) {
    static SAID: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let said = SAID.get_or_init(Default::default);
    for owner in owners(program) {
        let (hordes, tasks) = backends(&owner);
        for id in hordes.keys() {
            if id == "__seq" || id.starts_with("__lease/") { continue; }
            if live().0.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&id) { continue; }
            let Some(meta) = hordes.get(&id).map(stored_to_value) else { continue };
            if matches!(field(&meta, "closed"), Some(Value::Bool(true))) { continue; }
            // someone else runs it (a fresh lease)
            if let Some(l) = hordes.get(&lease_key(&id)).map(stored_to_value) {
                if field_str(&l, "runner").unwrap_or_default() != me() && now_ms() - field_int(&l, "beat") < LEASE_STALE_MS { continue; }
            }
            let target = field_str(&meta, "target").unwrap_or_default();
            let Some((tc, h)) = target.split_once('.') else { continue };
            let opt = |k: &str| field_str(&meta, k);
            let spec = Spec {
                id: id.clone(), owner: owner.clone(), target_cell: tc.to_string(), handler: h.to_string(),
                on_result: opt("on_result"), on_done: opt("on_done"), on_error: opt("on_error"),
                concurrency: (field_int(&meta, "concurrency").max(1) as usize).min(MAX_CONCURRENCY),
                max_attempts: field_int(&meta, "max_attempts").max(1) as u32,
                total: field_int(&meta, "total") as usize,
                budget_tokens: match field(&meta, "budget_tokens") { Some(Value::Int(i)) => i.to_i64(), _ => None },
                snapshot: if matches!(field(&meta, "has_snapshot"), Some(Value::Bool(true))) { Some(field(&meta, "snapshot").cloned().unwrap_or(Value::Unit)) } else { None },
                apply: opt("apply"),
                seed: match field(&meta, "seed") { Some(Value::Int(i)) => i.to_i64(), _ => None },
                instance: opt("instance"),
                parent: opt("parent"),
                // after a restart a nested horde gets its parent's ceiling
                // again, less its own recorded spend (documented)
                parent_budget: match field(&meta, "parent_budget") { Some(Value::Int(i)) => i.to_i64().map(|b| Arc::new(Budget::new(b, 0))), _ => None },
            };
            let (mut queue, mut done, mut failed, mut tokens) = (Vec::new(), 0, 0, 0i64);
            for i in 0..spec.total {
                let Some(t) = tasks.get(&task_key(&id, i)).map(stored_to_value) else { continue };
                match field_str(&t, "state").as_deref() {
                    Some("done") => { done += 1; tokens += field_int(&t, "tokens"); }
                    Some("failed") => failed += 1,
                    _ => queue.push((i, field(&t, "input").cloned().unwrap_or(Value::Unit))),
                }
            }
            let mut spec = spec;
            if let Some(pb) = spec.parent_budget.take() { spec.parent_budget = Some(Arc::new(Budget::new(pb.limit, tokens))); }
            let cancelled = matches!(field_str(&meta, "state").as_deref(), Some("cancelled") | Some("exhausted"));
            // the program changed since (a deploy renamed a callback): every
            // remaining task would pay the model, then fail — do not resume
            if let Some(why) = spec_mismatch(program, &spec) {
                if said.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone()) {
                    eprintln!("error: horde {} NOT resumed: {} — {} task(s) wait; restore it (or cancel the horde) and restart", id, why, queue.len());
                }
                continue;
            }
            // claim it in a unit (two processes starting together: one wins)
            let Some(factory) = FACTORY.get() else { return };
            let mut interp = factory();
            let (o, i) = (owner.clone(), id.clone());
            if !interp.atomically(|me| -> Result<bool, RuntimeError> { Ok(me.horde_claim(&o, &i)) }).unwrap_or(false) { continue; }
            eprintln!("[horde {}] resuming: {} task(s) to run, {} done, {} failed{}", id, if cancelled { 0 } else { queue.len() }, done, failed, if cancelled { " (cancelled)" } else { "" });
            // a cancelled horde keeps its unrun tasks queued (counted cancelled)
            apply_commit(Commit::Start { spec, queue, done, failed, tokens, closed: false, cancelled });
        }
    }
}

impl Interpreter {
    /// `vote(handler, input, k)`: k agents on the same input; the most
    /// common answer wins (ties: the earliest voter's). In a `[task]` step
    /// under serve / run the k calls run at once, outside the lock; else
    /// one after the other. Each voter starts from a fresh model context.
    pub(crate) fn horde_vote(&mut self, owner: &str, target: &str, input: Value, k: i64) -> Result<Value, RuntimeError> {
        let (tc, h) = self.horde_target(owner, target)?;
        if self.handler_params(&tc, &h) != Some(1) {
            return Err(RuntimeError::TypeError(format!("vote(): `{}.{}` must take exactly one parameter (the input)", tc, h)));
        }
        if !(1..=25).contains(&k) {
            return Err(RuntimeError::TypeError(format!("vote(): k must be 1..25 (got {})", k)));
        }
        let k = k as usize;
        let concurrent = FACTORY.get().is_some() && !super::IN_TEST.load(std::sync::atomic::Ordering::Relaxed) && self.task_unit.is_some();
        let results: Vec<Result<Value, RuntimeError>> = if concurrent {
            let budget = self.horde_budget.clone();
            let (tc2, h2, input2) = (tc.clone(), h.clone(), input.clone());
            self.outside_unit(move || {
                let (tx, rx) = std::sync::mpsc::channel();
                for j in 0..k {
                    let (tx, tc, h, input, budget) = (tx.clone(), tc2.clone(), h2.clone(), input2.clone(), budget.clone());
                    super::spawn_handler_thread(move || {
                        let Some(factory) = FACTORY.get() else { return };
                        let mut w = factory();
                        w.horde_budget = budget;
                        let task = w.handler_is_task(&tc, &h);
                        let r = if task { w.run_task(|me| me.call_signal_inner(&tc, &h, vec![input.clone()])) } else { w.atomically(|me| me.call_signal_inner(&tc, &h, vec![input.clone()])) };
                        let _ = tx.send((j, r));
                    });
                }
                drop(tx);
                let mut out: Vec<(usize, Result<Value, RuntimeError>)> = rx.iter().collect();
                out.sort_by_key(|(j, _)| *j);
                out.into_iter().map(|(_, r)| r).collect()
            })
        } else {
            let convs = std::mem::take(&mut self.agent_conversations);
            let conv = std::mem::take(&mut self.agent_conversation);
            let mut out = Vec::new();
            for _ in 0..k {
                self.agent_conversations.clear();
                self.agent_conversation.clear();
                let mark = self.journal.as_ref().map_or(0, |j| j.len());
                let r = self.call_signal(&tc, &h, vec![input.clone()]);
                if r.is_err() { self.rollback_to(mark); }
                out.push(r);
            }
            self.agent_conversations = convs;
            self.agent_conversation = conv;
            out
        };
        let mut votes: Vec<Value> = Vec::new();
        let mut first_err: Option<RuntimeError> = None;
        let mut errors = 0i64;
        for r in results {
            match r {
                Ok(v) => votes.push(v),
                Err(e) => { errors += 1; if first_err.is_none() { first_err = Some(e); } }
            }
        }
        if votes.is_empty() {
            return Err(first_err.unwrap_or_else(|| RuntimeError::TypeError("vote(): no voter answered".to_string())));
        }
        // tally by value (its printed form), the earliest voter first on ties
        let keys: Vec<String> = votes.iter().map(|v| format!("{}", v)).collect();
        let mut best = 0usize;
        let mut best_n = 0usize;
        for (i, key) in keys.iter().enumerate() {
            let n = keys.iter().filter(|x| *x == key).count();
            if n > best_n { best = i; best_n = n; }
        }
        Ok(map_from_pairs(vec![
            ("winner".to_string(), votes[best].clone()),
            ("count".to_string(), int(best_n as i64)),
            ("k".to_string(), int(k as i64)),
            ("unanimous".to_string(), Value::Bool(best_n == k)),
            ("errors".to_string(), int(errors)),
            ("votes".to_string(), Value::List(votes)),
        ]))
    }
}

/// Every horde live in this process with its status (the dashboard's
/// `/__soma/hordes`), newest first.
pub fn live_statuses() -> serde_json::Value {
    let ids: Vec<(String, String, String)> = {
        let reg = live().0.lock().unwrap_or_else(|e| e.into_inner());
        reg.values().map(|l| (l.spec.id.clone(), l.spec.owner.clone(), format!("{}.{}", l.spec.target_cell, l.spec.handler))).collect()
    };
    let Some(factory) = FACTORY.get() else { return serde_json::json!([]) };
    let mut interp = factory();
    let mut out: Vec<(u64, serde_json::Value)> = Vec::new();
    for (id, owner, target) in ids {
        let Ok(st) = interp.horde_status(&owner, &id) else { continue };
        let n: u64 = id.rsplit('h').next().and_then(|x| x.parse().ok()).unwrap_or(0);
        out.push((n, serde_json::json!({"id": id, "target": target, "status": super::record_log::value_to_json(&st)})));
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    serde_json::Value::Array(out.into_iter().map(|(_, v)| v).collect())
}

/// Does the program still have the horde's target and callbacks, with
/// their arities? (checked before a persisted horde resumes)
fn spec_mismatch(program: &crate::ast::Program, spec: &Spec) -> Option<String> {
    let params = |c: &str, h: &str| program.cells.iter().find(|x| x.node.name == c).and_then(|x| x.node.sections.iter().find_map(|s| match &s.node {
        crate::ast::Section::OnSignal(on) if on.signal_name == h => Some(on.params.len()), _ => None }));
    let want = if spec.snapshot.is_some() { 2 } else { 1 };
    match params(&spec.target_cell, &spec.handler) {
        None => return Some(format!("its handler `{}.{}` no longer exists", spec.target_cell, spec.handler)),
        Some(n) if n != want => return Some(format!("`{}.{}` now takes {} parameter(s), the horde passes {}", spec.target_cell, spec.handler, n, want)),
        _ => {}
    }
    for (what, h, ok) in [("on_result", &spec.on_result, &[1usize, 2][..]), ("apply", &spec.apply, &[1, 2][..]), ("on_done", &spec.on_done, &[1][..]), ("on_error", &spec.on_error, &[2][..])] {
        let Some(h) = h else { continue };
        match params(&spec.owner, h) {
            None => return Some(format!("its {} handler `{}.{}` no longer exists", what, spec.owner, h)),
            Some(n) if !ok.contains(&n) => return Some(format!("its {} handler `{}.{}` now takes {} parameter(s)", what, spec.owner, h, n)),
            _ => {}
        }
    }
    None
}
