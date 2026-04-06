pub mod builtins;
pub mod native_ffi;
pub mod soma_int;

use std::collections::HashMap;
use std::sync::Arc;
use rustc_hash::FxHashMap;
use indexmap::IndexMap;

/// Fast environment map — uses FxHash (no crypto overhead) for variable lookups
type Env = FxHashMap<String, Value>;
use crate::ast::*;
use crate::runtime::storage::{StorageBackend, StoredValue};
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("undefined variable: {0}")]
    UndefinedVar(String),
    #[error("undefined function: {0}")]
    UndefinedFn(String),
    #[error("{0}")]
    TypeError(String),
    #[error("no handler found for signal '{0}' in cell '{1}'")]
    NoHandler(String, String),
    #[error("require failed: {0}")]
    RequireFailed(String),
    #[error("stack overflow (recursion depth exceeded)")]
    StackOverflow,
}

/// Return a human-readable type name for a Value (e.g. "String", "Int").
fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "Int",
        Value::Big(_) => "BigInt",
        Value::Float(_) => "Float",
        Value::String(_) => "String",
        Value::Bool(_) => "Bool",
        Value::List(_) => "List",
        Value::Map(_) => "Map",
        Value::Lambda { .. } | Value::LambdaBlock { .. } => "Function",
        Value::Unit => "Null",
    }
}

/// Human-readable name for a BinOp verb (e.g. "add", "subtract").
fn binop_verb(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "add",
        BinOp::Sub => "subtract",
        BinOp::Mul => "multiply",
        BinOp::Div => "divide",
        BinOp::Mod => "modulo",
        BinOp::And => "logical-and",
        BinOp::Or => "logical-or",
    }
}

/// Convert a byte offset to line:col using source text
pub fn span_to_location(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.chars().enumerate() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Build a source-context snippet with a caret line pointing at the error position.
/// Returns an empty string if no source is available.
pub fn format_error_context(source: &str, span_start: usize) -> String {
    let (line_num, col) = span_to_location(source, span_start);
    // Extract the source line
    let line_text = source.split('\n').nth(line_num - 1).unwrap_or("");
    let line_num_str = format!("{}", line_num);
    let gutter_width = line_num_str.len();
    let padding = " ".repeat(gutter_width);
    let caret_offset = " ".repeat(col.saturating_sub(1));
    format!(
        "{} |\n{} | {}\n{} | {}^",
        padding, line_num_str, line_text, padding, caret_offset
    )
}

/// Format an error with file location and source context if available.
pub fn format_runtime_error(
    err: &RuntimeError,
    source_file: Option<&str>,
    source_text: Option<&str>,
    span: Option<crate::ast::Span>,
) -> String {
    let (location, context) = match (source_file, source_text, span) {
        (Some(file), Some(text), Some(sp)) => {
            let (line, col) = span_to_location(text, sp.start);
            let loc = format!("  --> {}:{}:{}\n", file, line, col);
            let ctx = format_error_context(text, sp.start);
            (loc, ctx)
        }
        (Some(file), _, _) => (format!("  --> {}\n", file), String::new()),
        _ => (String::new(), String::new()),
    };
    if context.is_empty() {
        format!("error: {}\n{}", err, location)
    } else {
        format!("error: {}\n{}{}", err, location, context)
    }
}

/// Compute Levenshtein edit distance between two strings.
fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (m, n) = (a.len(), b.len());
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Check if a Value is truthy (false for Bool(false), Unit, Int(0); true otherwise)
pub fn is_truthy(val: &Value) -> bool {
    match val {
        Value::Bool(b) => *b,
        Value::Unit => false,
        Value::Int(n) => *n != 0,
        Value::Big(n) => !n.is_zero(),
        Value::String(s) => !s.is_empty(),
        Value::List(l) => !l.is_empty(),
        _ => true,
    }
}

/// Runtime values
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Big(BigInt),
    Float(f64),
    String(String),
    Bool(bool),
    List(Vec<Value>),
    Map(IndexMap<String, Value>),
    /// Lambda: captured param name + body expression + closure environment
    Lambda {
        param: std::string::String,
        body: Box<Spanned<Expr>>,
        env: HashMap<std::string::String, Value>,
    },
    /// Block lambda: with statements before result
    LambdaBlock {
        param: std::string::String,
        stmts: Vec<Spanned<Statement>>,
        result: Box<Spanned<Expr>>,
        env: HashMap<std::string::String, Value>,
    },
    Unit,
}

/// Build a Value::Map from a Vec of (String, Value) pairs.
/// Preserves insertion order; duplicate keys overwrite.
pub fn map_from_pairs(pairs: Vec<(String, Value)>) -> Value {
    Value::Map(pairs.into_iter().collect())
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Big(n) => write!(f, "{}", n),
            Value::Float(n) => {
                if n.fract() == 0.0 && n.is_finite() {
                    write!(f, "{:.1}", n)
                } else {
                    write!(f, "{}", n)
                }
            }
            Value::String(s) => write!(f, "{}", s),
            Value::Bool(b) => write!(f, "{}", b),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    match item {
                        Value::String(s) => {
                            let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                            write!(f, "\"{}\"", escaped)?
                        }
                        other => write!(f, "{}", other)?,
                    }
                }
                write!(f, "]")
            }
            Value::Map(entries) => {
                write!(f, "{{")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    let escaped_k = k.replace('\\', "\\\\").replace('"', "\\\"");
                    write!(f, "\"{}\": ", escaped_k)?;
                    match v {
                        Value::String(s) => {
                            let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                            write!(f, "\"{}\"", escaped)?
                        }
                        other => write!(f, "{}", other)?,
                    }
                }
                write!(f, "}}")
            }
            Value::Lambda { param, .. } => write!(f, "<lambda({})>", param),
            Value::LambdaBlock { param, .. } => write!(f, "<lambda({})>", param),
            Value::Unit => write!(f, "null"),
        }
    }
}

impl Value {
    pub fn as_int(&self) -> Result<i64, RuntimeError> {
        match self {
            Value::Int(n) => Ok(*n),
            Value::Big(n) => n.to_i64().ok_or_else(|| RuntimeError::TypeError("BigInt too large for i64".to_string())),
            other => Err(RuntimeError::TypeError(format!("expected Int, got {:?}", other))),
        }
    }

    fn as_bigint(&self) -> Result<BigInt, RuntimeError> {
        match self {
            Value::Big(n) => Ok(n.clone()),
            Value::Int(n) => Ok(BigInt::from(*n)),
            other => Err(RuntimeError::TypeError(format!("expected BigInt, got {:?}", other))),
        }
    }

    fn as_float(&self) -> Result<f64, RuntimeError> {
        match self {
            Value::Float(n) => Ok(*n),
            Value::Int(n) => Ok(*n as f64),
            Value::Big(n) => Ok(n.to_string().parse::<f64>().unwrap_or(f64::INFINITY)),
            other => Err(RuntimeError::TypeError(format!("expected Float, got {:?}", other))),
        }
    }

    fn as_bool(&self) -> Result<bool, RuntimeError> {
        match self {
            Value::Bool(b) => Ok(*b),
            Value::Int(n) => Ok(*n != 0),
            Value::Big(n) => Ok(!n.is_zero()),
            other => Err(RuntimeError::TypeError(format!("expected Bool, got {:?}", other))),
        }
    }

    /// Promote Int to Big if needed for mixed operations
    fn is_big(&self) -> bool {
        matches!(self, Value::Big(_))
    }
}

/// Signal that a return statement was hit
#[derive(Debug)]
struct ReturnSignal(Value);

/// Tree-walking interpreter for Soma programs
/// Pre-computed handler lookup: (cell_name, signal_name) → (params, body)
type HandlerKey = (String, String);
type HandlerValue = (Arc<Vec<Param>>, Arc<Vec<Spanned<Statement>>>);

/// Check whether a slice of statements contains any `let` bindings (used to
/// decide whether we need full scoping overhead in exec_body_scoped).
fn body_has_let(body: &[Spanned<Statement>]) -> bool {
    body.iter().any(|s| matches!(s.node, Statement::Let { .. }))
}

/// A broadcast event emitted by `emit` — sent to all SSE clients and connected cells
#[derive(Debug, Clone)]
pub struct BusEvent {
    pub stream: String,
    pub data: Value,
}

/// Shared broadcast bus for real-time event distribution
pub type EventBus = Arc<std::sync::Mutex<Vec<std::sync::mpsc::Sender<BusEvent>>>>;

/// TCP peer connections for inter-process signal bus
pub type PeerBus = Arc<std::sync::Mutex<Vec<std::sync::mpsc::Sender<String>>>>;

pub fn new_event_bus() -> EventBus {
    Arc::new(std::sync::Mutex::new(Vec::new()))
}

pub fn new_peer_bus() -> PeerBus {
    Arc::new(std::sync::Mutex::new(Vec::new()))
}

pub struct Interpreter {
    /// All cells in the program, by name
    pub(crate) cells: HashMap<String, CellDef>,
    /// Pre-computed handler lookup — avoids scanning sections on every call
    handler_cache: HashMap<HandlerKey, HandlerValue>,
    /// Maximum recursion depth
    max_depth: usize,
    pub(crate) current_depth: usize,
    /// Signals emitted during execution (collected for runtime dispatch)
    emitted_signals: Vec<(String, Vec<Value>)>,
    /// Storage backends for memory slots, keyed by "cell_name.slot_name"
    pub(crate) storage: HashMap<String, Arc<dyn StorageBackend>>,
    /// State machines: (cell_name, machine_name) → definition
    pub(crate) state_machines: HashMap<(String, String), StateMachineSection>,
    /// Broadcast bus for SSE/real-time events
    pub event_bus: Option<EventBus>,
    /// Peer bus for inter-process signal delivery
    pub peer_bus: Option<PeerBus>,
    /// WebSocket client outgoing channel (for ws_send)
    pub ws_out: Option<Arc<std::sync::Mutex<std::sync::mpsc::Sender<String>>>>,
    /// Source file path for error reporting
    pub source_file: Option<String>,
    /// Source text for line:col conversion
    pub source_text: Option<String>,
    /// Last known span (set before eval, used for error reporting)
    pub last_span: Option<crate::ast::Span>,
    /// Cached handler for the current recursive call (avoids repeated HashMap lookups)
    current_handler: Option<(String, String, Arc<Vec<Param>>, Arc<Vec<Spanned<Statement>>>)>,
    /// Loaded [native] handler FFI function pointers, keyed by (cell_name, signal_name)
    pub native_handlers: HashMap<(String, String), native_ffi::LoadedNative>,
    /// Cluster node for distributed storage (None = standalone mode)
    pub cluster: Option<Arc<crate::runtime::cluster::ClusterNode>>,
    /// Which memory slots are sharded (slot_name → true)
    pub sharded_slots: HashMap<String, bool>,
    /// Memory invariants: slot_name → list of expressions to check on .set()
    pub(crate) invariants: HashMap<String, Vec<Expr>>,
    // ── Agent-specific state ────────────────────────────────────────
    /// Token usage tracking: total tokens consumed by think() calls
    pub(crate) agent_tokens_used: i64,
    /// Token budget: max tokens allowed (0 = unlimited)
    pub(crate) agent_token_budget: i64,
    /// Conversation history for multi-turn think() within a handler
    pub(crate) agent_conversation: Vec<serde_json::Value>,
    /// Structured trace log: every think, tool call, transition, delegate
    pub(crate) agent_trace: Vec<Value>,
    /// Pending approval gates (for human-in-the-loop)
    pub(crate) agent_pending_approval: Option<String>,
    /// Agent LLM config (from soma.toml [agent] section)
    pub agent_config: Option<crate::pkg::manifest::AgentConfig>,
    /// Named model configs (from soma.toml [models.*] sections)
    pub agent_models: std::collections::HashMap<String, crate::pkg::manifest::AgentConfig>,
}

impl Interpreter {
    pub fn new(program: &Program) -> Self {
        let mut cells = HashMap::new();
        let mut handler_cache = HashMap::new();
        let mut state_machines = HashMap::new();
        for cell in &program.cells {
            cells.insert(cell.node.name.clone(), cell.node.clone());
            for section in &cell.node.sections {
                if let Section::OnSignal(ref on) = section.node {
                    let key = (cell.node.name.clone(), on.signal_name.clone());
                    if handler_cache.contains_key(&key) {
                        eprintln!("warning: duplicate handler '{}' in cell '{}' (last definition wins)", on.signal_name, cell.node.name);
                    }
                    let value = (Arc::new(on.params.clone()), Arc::new(on.body.clone()));
                    handler_cache.insert(key, value);
                }
                if let Section::State(ref sm) = section.node {
                    state_machines.insert(
                        (cell.node.name.clone(), sm.name.clone()),
                        sm.clone(),
                    );
                }
            }
        }
        // Collect memory invariants from all cells
        let mut invariants: HashMap<String, Vec<Expr>> = HashMap::new();
        for cell in &program.cells {
            for section in &cell.node.sections {
                if let Section::Memory(ref mem) = section.node {
                    if !mem.invariants.is_empty() {
                        let exprs: Vec<Expr> = mem.invariants.iter().map(|i| i.node.clone()).collect();
                        // Associate invariants with all slots in this memory section
                        for slot in &mem.slots {
                            let key = format!("{}.{}", cell.node.name, slot.node.name);
                            invariants.entry(key).or_default().extend(exprs.clone());
                            invariants.entry(slot.node.name.clone()).or_default().extend(exprs.clone());
                        }
                    }
                }
            }
        }
        Self {
            cells,
            handler_cache,
            max_depth: 512,
            current_depth: 0,
            emitted_signals: Vec::new(),
            storage: HashMap::new(),
            state_machines,
            event_bus: None,
            peer_bus: None,
            ws_out: None,
            source_file: None,
            source_text: None,
            last_span: None,
            current_handler: None,
            native_handlers: HashMap::new(),
            cluster: None,
            sharded_slots: HashMap::new(),
            invariants,
            agent_tokens_used: 0,
            agent_token_budget: 0,
            agent_conversation: Vec::new(),
            agent_trace: Vec::new(),
            agent_pending_approval: None,
            agent_config: None,
            agent_models: std::collections::HashMap::new(),
        }
    }

    /// Register an additional cell definition (used by runtime to inject interior cells)
    pub fn register_cell(&mut self, cell: CellDef) {
        // Update handler cache
        for section in &cell.sections {
            if let Section::OnSignal(ref on) = section.node {
                let key = (cell.name.clone(), on.signal_name.clone());
                let value = (Arc::new(on.params.clone()), Arc::new(on.body.clone()));
                self.handler_cache.insert(key, value);
            }
        }
        self.cells.insert(cell.name.clone(), cell);
    }

    /// Inject storage backends for memory slots
    pub fn set_storage(&mut self, cell_name: &str, slots: &HashMap<String, Arc<dyn StorageBackend>>) {
        for (slot_name, backend) in slots {
            let key = format!("{}.{}", cell_name, slot_name);
            self.storage.insert(key, backend.clone());
        }
        // Also register without cell prefix for direct access in handlers
        for (slot_name, backend) in slots {
            self.storage.insert(slot_name.clone(), backend.clone());
        }
    }

    /// Inject pre-keyed storage backends (keys already include cell prefix)
    pub fn set_storage_raw(&mut self, slots: &HashMap<String, Arc<dyn StorageBackend>>) {
        for (key, backend) in slots {
            self.storage.insert(key.clone(), backend.clone());
        }
    }

    /// Configure cluster mode — enables distributed storage operations
    pub fn set_cluster(&mut self, cluster: Arc<crate::runtime::cluster::ClusterNode>, sharded: &HashMap<String, bool>) {
        self.cluster = Some(cluster);
        self.sharded_slots = sharded.clone();
    }

    /// Execute an `every` block's body
    pub fn exec_every(&mut self, body: &[Spanned<Statement>], env: &mut Env, cell_name: &str) -> Result<Value, RuntimeError> {
        match self.exec_body(body, env, cell_name, "_every") {
            Ok(val) => Ok(val),
            Err(ExecError::Return(val)) => Ok(val),
            Err(ExecError::Break) => {
                let e = RuntimeError::TypeError("break outside of loop".to_string());
                eprintln!("[scheduler] error: {}", e);
                Err(e)
            }
            Err(ExecError::Continue) => {
                let e = RuntimeError::TypeError("continue outside of loop".to_string());
                eprintln!("[scheduler] error: {}", e);
                Err(e)
            }
            Err(ExecError::Runtime(e)) => {
                eprintln!("[scheduler] error: {}", e);
                Err(e)
            }
        }
    }

    /// Ensure state machine storage slots exist
    /// Uses persistent backend (SQLite) if any existing slot is persistent, otherwise memory
    pub fn ensure_state_machine_storage(&mut self) {
        let has_persistent = self.storage.values().any(|b| b.backend_name() == "sqlite" || b.backend_name() == "file");
        for ((cell_name, sm_name), _) in self.state_machines.clone() {
            // Use cell-scoped key to prevent collisions between agents
            let key = format!("__sm_{}_{}", cell_name, sm_name);
            let legacy_key = format!("__sm_{}", sm_name);
            if !self.storage.contains_key(&key) && !self.storage.contains_key(&legacy_key) {
                let backend: Arc<dyn crate::runtime::storage::StorageBackend> = if has_persistent {
                    Arc::new(crate::runtime::storage::SqliteBackend::new(&cell_name, &format!("_sm_{}", sm_name)))
                } else {
                    Arc::new(crate::runtime::storage::MemoryBackend::new())
                };
                self.storage.insert(key, backend);
            }
        }
    }

    /// Take all emitted signals (drains the buffer)
    /// Find which cell has a handler for the given signal and call it
    /// Same as find_and_call but with a different name for pipe operator
    pub fn find_and_call_with_args(&mut self, signal_name: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        self.find_and_call(signal_name, args)
    }

    pub fn find_and_call(&mut self, signal_name: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        // Search handler cache for matching signal
        let cell_name = self.handler_cache.keys()
            .find(|(_, sig)| sig == signal_name)
            .map(|(cell, _)| cell.clone());

        if let Some(cell) = cell_name {
            self.call_signal(&cell, signal_name, args)
        } else {
            // Try as a builtin
            if let Some(result) = self.call_builtin(signal_name, &args, "") {
                result
            } else {
                Err(RuntimeError::UndefinedFn(signal_name.to_string()))
            }
        }
    }

    pub fn take_emitted_signals(&mut self) -> Vec<(String, Vec<Value>)> {
        std::mem::take(&mut self.emitted_signals)
    }

    /// Run a signal handler on a cell with the given arguments.
    /// Only clones the handler's params and body — not the whole CellDef.
    pub fn call_signal(
        &mut self,
        cell_name: &str,
        signal_name: &str,
        args: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        // Check for [native] FFI handler first — fast path
        let native_key = (cell_name.to_string(), signal_name.to_string());
        if self.native_handlers.contains_key(&native_key) {
            let native = self.native_handlers.get(&native_key).unwrap();
            match native_ffi::call_native(native, &args) {
                Ok(val) => return Ok(val),
                Err(e) if e.contains("overflow_rerun") => {
                    // i128 overflow — fall through to interpreted path for BigInt
                    eprintln!("[native] i128 overflow, falling back to interpreted BigInt");
                }
                Err(e) => return Err(RuntimeError::TypeError(e)),
            }
        }

        // Lookup from pre-computed cache — O(1) instead of scanning sections
        let key = (cell_name.to_string(), signal_name.to_string());
        let (params, body) = {
            let entry = self.handler_cache.get(&key)
                .ok_or_else(|| RuntimeError::NoHandler(signal_name.to_string(), cell_name.to_string()))?;
            // Arc::clone is cheap — just increments a refcount (no deep copy)
            (Arc::clone(&entry.0), Arc::clone(&entry.1))
        };

        // Cache this handler for fast recursive lookups
        let prev_handler = self.current_handler.take();
        self.current_handler = Some((
            cell_name.to_string(),
            signal_name.to_string(),
            Arc::clone(&params),
            Arc::clone(&body),
        ));

        let result = self.call_signal_resolved(cell_name, signal_name, args, &params, &body);

        self.current_handler = prev_handler;
        result
    }

    /// Fast path: execute a signal handler with pre-resolved params/body.
    /// Avoids the HashMap lookup when we already know which handler to call.
    fn call_signal_resolved(
        &mut self,
        cell_name: &str,
        signal_name: &str,
        args: Vec<Value>,
        params: &[Param],
        body: &[Spanned<Statement>],
    ) -> Result<Value, RuntimeError> {
        self.current_depth += 1;
        if self.current_depth > self.max_depth {
            self.current_depth -= 1;
            return Err(RuntimeError::StackOverflow);
        }

        // Check arity
        if args.len() != params.len() {
            self.current_depth -= 1;
            return Err(RuntimeError::TypeError(format!(
                "{}() expected {} argument{}, got {}",
                signal_name,
                params.len(),
                if params.len() == 1 { "" } else { "s" },
                args.len()
            )));
        }

        // Bind parameters, promoting Int → BigInt if the type declares BigInt
        let mut env = FxHashMap::with_capacity_and_hasher(params.len() + 4, Default::default());
        for (param, val) in params.iter().zip(args) {
            let val = if is_bigint_type(&param.ty.node) {
                match val {
                    Value::Int(n) => Value::Big(BigInt::from(n)),
                    other => other,
                }
            } else {
                val
            };
            env.insert(param.name.clone(), val);
        }

        let result = self.exec_body(body, &mut env, cell_name, signal_name);

        self.current_depth -= 1;

        match result {
            Ok(val) => Ok(val),
            Err(ExecError::Return(val)) => Ok(val),
            Err(ExecError::Break) => Err(RuntimeError::TypeError("break outside of loop".to_string())),
            Err(ExecError::Continue) => Err(RuntimeError::TypeError("continue outside of loop".to_string())),
            Err(ExecError::Runtime(e)) => Err(e),
        }
    }

    // We use a separate error type internally to handle early returns
    fn exec_body(
        &mut self,
        body: &[Spanned<Statement>],
        env: &mut Env,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<Value, ExecError> {
        let mut last_value = Value::Unit;

        for stmt in body {
            self.last_span = Some(stmt.span);
            last_value = self.exec_stmt(&stmt.node, env, cell_name, signal_name)?;
        }

        Ok(last_value)
    }

    /// Execute a body with proper block scoping:
    /// - `let` declarations are local to the block
    /// - assignments to pre-existing variables propagate to outer scope
    fn exec_body_scoped(
        &mut self,
        body: &[Spanned<Statement>],
        env: &mut Env,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<Value, ExecError> {
        // Fast path: if the body contains no `let` bindings, no scoping is needed
        if !body_has_let(body) {
            return self.exec_body(body, env, cell_name, signal_name);
        }
        // Collect which names will be `let`-bound in this block and save their
        // original values (if any) for restoration. This is O(k) where k = number
        // of let bindings in the block, NOT O(n) where n = total env size.
        let mut shadowed: Vec<(String, Option<Value>)> = Vec::new();
        let mut new_keys: Vec<String> = Vec::new();
        for stmt in body {
            if let Statement::Let { name, .. } = &stmt.node {
                if let Some(existing) = env.get(name) {
                    shadowed.push((name.clone(), Some(existing.clone())));
                } else {
                    new_keys.push(name.clone());
                }
            }
        }

        let result = self.exec_body(body, env, cell_name, signal_name);

        // Remove new bindings that should not leak out of this scope
        for key in &new_keys {
            env.remove(key);
        }
        // Restore shadowed variables to their original values
        for (name, original) in shadowed {
            if let Some(val) = original {
                env.insert(name, val);
            } else {
                env.remove(&name);
            }
        }
        result
    }

    fn exec_stmt(
        &mut self,
        stmt: &Statement,
        env: &mut Env,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<Value, ExecError> {
        match stmt {
            Statement::Let { name, value } => {
                self.last_span = Some(value.span);
                let val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                env.insert(name.clone(), val);
                Ok(Value::Unit)
            }

            Statement::Assign { name, value } => {
                self.last_span = Some(value.span);
                // Optimization: items = list(items, x) → in-place append (avoids O(n²) clone)
                if let Expr::FnCall { name: fn_name, args: fn_args } = &value.node {
                    if (fn_name == "list" || fn_name == "push" || fn_name == "append") && fn_args.len() >= 2 {
                        if let Expr::Ident(ref first_arg_name) = fn_args[0].node {
                            if first_arg_name == name {
                                // Take the list from env, append in place, put back
                                let mut existing = env.remove(name).unwrap_or(Value::List(vec![]));
                                if let Value::List(ref mut vec) = existing {
                                    for arg in &fn_args[1..] {
                                        let val = self.eval_expr(&arg.node, env, cell_name, signal_name)?;
                                        vec.push(val);
                                    }
                                    env.insert(name.clone(), existing);
                                    return Ok(Value::Unit);
                                }
                                env.insert(name.clone(), existing);
                            }
                        }
                    }
                }
                // Fast path: x = x + LITERAL or x = x - LITERAL (compound assignment on ints)
                if let Expr::BinaryOp { left, op, right } = &value.node {
                    if let Expr::Ident(ref lhs_name) = left.node {
                        if lhs_name == name {
                            // x = x OP rhs → in-place update
                            if let Some(Value::Int(current)) = env.get(name) {
                                let current = *current;
                                let rhs_val = self.eval_expr(&right.node, env, cell_name, signal_name)?;
                                if let Value::Int(rhs_int) = rhs_val {
                                    let result = match op {
                                        BinOp::Add => match current.checked_add(rhs_int) {
                                            Some(v) => v,
                                            None => { let val = self.eval_expr(&value.node, env, cell_name, signal_name)?; env.insert(name.clone(), val); return Ok(Value::Unit); }
                                        },
                                        BinOp::Sub => match current.checked_sub(rhs_int) {
                                            Some(v) => v,
                                            None => { let val = self.eval_expr(&value.node, env, cell_name, signal_name)?; env.insert(name.clone(), val); return Ok(Value::Unit); }
                                        },
                                        BinOp::Mul => match current.checked_mul(rhs_int) {
                                            Some(v) => v,
                                            None => { let val = self.eval_expr(&value.node, env, cell_name, signal_name)?; env.insert(name.clone(), val); return Ok(Value::Unit); }
                                        },
                                        BinOp::Div if rhs_int != 0 => current / rhs_int,
                                        _ => {
                                            let val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                                            env.insert(name.clone(), val);
                                            return Ok(Value::Unit);
                                        }
                                    };
                                    env.insert(name.clone(), Value::Int(result));
                                    return Ok(Value::Unit);
                                }
                            }
                        }
                    }
                }
                // Fast path: m = m |> with(k, v) → in-place map insert
                if let Expr::Pipe { left, right } = &value.node {
                    if let Expr::Ident(ref pipe_name) = left.node {
                        if pipe_name == name {
                            if let Expr::FnCall { name: fn_name, args } = &right.node {
                                if fn_name == "with" && args.len() >= 2 {
                                    let key = self.eval_expr(&args[0].node, env, cell_name, signal_name)?;
                                    let val = self.eval_expr(&args[1].node, env, cell_name, signal_name)?;
                                    let key_str = format!("{}", key);
                                    if let Some(Value::Map(ref mut entries)) = env.get_mut(name) {
                                        entries.insert(key_str, val);
                                        return Ok(Value::Unit);
                                    }
                                }
                            }
                        }
                    }
                }
                let val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                env.insert(name.clone(), val);
                Ok(Value::Unit)
            }

            Statement::Return { value } => {
                self.last_span = Some(value.span);
                let val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                Err(ExecError::Return(val))
            }

            Statement::Break => {
                Err(ExecError::Break)
            }

            Statement::Continue => {
                Err(ExecError::Continue)
            }

            Statement::Ensure { condition } => {
                // Postcondition: store condition for checking after handler returns.
                // For now, evaluate eagerly — if false at this point, fail.
                // This is useful for pre+post style: ensure at end of handler body.
                let val = self.eval_expr(&condition.node, env, cell_name, signal_name)?;
                if !val.is_truthy() {
                    Err(ExecError::Runtime(RuntimeError::RequireFailed(
                        format!("ensure postcondition failed")
                    )))
                } else {
                    Ok(Value::Unit)
                }
            }

            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                self.last_span = Some(condition.span);
                let cond = self.eval_expr(&condition.node, env, cell_name, signal_name)?;
                if is_truthy(&cond) {
                    self.exec_body_scoped(then_body, env, cell_name, signal_name)
                } else if !else_body.is_empty() {
                    self.exec_body_scoped(else_body, env, cell_name, signal_name)
                } else {
                    Ok(Value::Unit)
                }
            }

            Statement::For { var, iter, body } => {
                // Fast path: for i in range(start, end) — no allocation
                if let Expr::FnCall { name: fn_name, args: fn_args } = &iter.node {
                    if fn_name == "range" && fn_args.len() >= 2 {
                        let start_val = self.eval_expr(&fn_args[0].node, env, cell_name, signal_name)?;
                        let end_val = self.eval_expr(&fn_args[1].node, env, cell_name, signal_name)?;
                        if let (Value::Int(start), Value::Int(end)) = (&start_val, &end_val) {
                            let (start, end) = (*start, *end);
                            let mut last = Value::Unit;
                            let mut i = start;
                            let needs_scope = body_has_let(body);
                            while i < end {
                                env.insert(var.clone(), Value::Int(i));
                                let result = if needs_scope {
                                    self.exec_body_scoped(body, env, cell_name, signal_name)
                                } else {
                                    self.exec_body(body, env, cell_name, signal_name)
                                };
                                match result {
                                    Ok(val) => last = val,
                                    Err(ExecError::Break) => break,
                                    Err(ExecError::Continue) => { i += 1; continue; }
                                    Err(e) => { env.remove(var); return Err(e); }
                                }
                                i += 1;
                            }
                            env.remove(var);
                            return Ok(last);
                        }
                    }
                }

                // General path: evaluate iterator
                let iter_val = self.eval_expr(&iter.node, env, cell_name, signal_name)?;

                let items = match iter_val {
                    Value::List(items) => items,
                    Value::Map(entries) => {
                        entries.into_iter().map(|(k, v)| {
                            map_from_pairs(vec![
                                ("key".to_string(), Value::String(k)),
                                ("value".to_string(), v),
                            ])
                        }).collect()
                    }
                    Value::String(s) => {
                        if s.contains('\n') {
                            s.split('\n')
                                .filter(|l| !l.is_empty())
                                .map(|l| Value::String(l.to_string()))
                                .collect()
                        } else {
                            vec![Value::String(s)]
                        }
                    }
                    other => vec![other],
                };

                let mut last = Value::Unit;
                for item in items {
                    env.insert(var.clone(), item);
                    match self.exec_body_scoped(body, env, cell_name, signal_name) {
                        Ok(val) => last = val,
                        Err(ExecError::Break) => break,
                        Err(ExecError::Continue) => continue,
                        Err(e) => { env.remove(var); return Err(e); }
                    }
                }
                env.remove(var);
                Ok(last)
            }

            Statement::While { condition, body } => {
                // Ultra-fast path: while i < N { ... i += K ... }
                // Detect: condition is i < literal, body is all Assign with += on ints
                // Run entirely with Rust locals, zero HashMap access per iteration
                if let Expr::CmpOp { left, op: CmpOp::Lt, right } = &condition.node {
                    if let (Expr::Ident(ref counter_name), Expr::Literal(Literal::Int(limit))) = (&left.node, &right.node) {
                        let limit = *limit;
                        let cn = counter_name.clone();

                        // Check if body is purely int-assignable (all stmts are x += expr on ints)
                        // If so, we can run the entire loop with a local variable snapshot
                        let needs_scope = body_has_let(body);

                        // Snapshot approach: pull all referenced int vars into a local vec,
                        // run the loop, push them back. Eliminates HashMap per-iteration.
                        // Collect all variable names referenced in the body
                        let mut var_names: Vec<String> = vec![cn.clone()];
                        for stmt in body.iter() {
                            if let Statement::Assign { name, .. } = &stmt.node {
                                if !var_names.contains(name) {
                                    var_names.push(name.clone());
                                }
                            }
                        }

                        // Check if ALL referenced vars are ints and body is only int assigns
                        let all_ints = var_names.iter().all(|n| {
                            matches!(env.get(n), Some(Value::Int(_)) | None)
                        });

                        if all_ints && !needs_scope && var_names.len() <= 8 {
                            // Pull vars into local Rust Vec
                            let mut locals: Vec<i64> = var_names.iter().map(|n| {
                                match env.get(n) { Some(Value::Int(v)) => *v, _ => 0 }
                            }).collect();

                            // Run the loop with direct array access
                            'fast_while: loop {
                                if locals[0] >= limit { break; }

                                // Execute each statement with local vars
                                for stmt in body.iter() {
                                    if let Statement::Assign { name, value } = &stmt.node {
                                        if let Expr::BinaryOp { left: lhs, op, right: rhs } = &value.node {
                                            if let Expr::Ident(ref lhs_name) = lhs.node {
                                                if lhs_name == name {
                                                    let lhs_idx = var_names.iter().position(|n| n == name);
                                                    if let Some(idx) = lhs_idx {
                                                        let rhs_val = match &rhs.node {
                                                            Expr::Literal(Literal::Int(n)) => *n,
                                                            Expr::Ident(ref rn) => {
                                                                var_names.iter().position(|n| n == rn)
                                                                    .map(|ri| locals[ri]).unwrap_or(0)
                                                            }
                                                            _ => { break 'fast_while; } // can't handle, fall through
                                                        };
                                                        let checked = match op {
                                                            BinOp::Add => locals[idx].checked_add(rhs_val),
                                                            BinOp::Sub => locals[idx].checked_sub(rhs_val),
                                                            BinOp::Mul => locals[idx].checked_mul(rhs_val),
                                                            _ => None,
                                                        };
                                                        match checked {
                                                            Some(v) => locals[idx] = v,
                                                            None => {
                                                                // Overflow — promote to BigInt register loop
                                                                let mut vals: Vec<Value> = locals.iter()
                                                                    .map(|v| Value::Int(*v)).collect();
                                                                // Apply this operation with BigInt
                                                                let lhs_big = BigInt::from(locals[idx]);
                                                                let rhs_big = BigInt::from(rhs_val);
                                                                let result_big = match op {
                                                                    BinOp::Add => lhs_big + rhs_big,
                                                                    BinOp::Sub => lhs_big - rhs_big,
                                                                    BinOp::Mul => lhs_big * rhs_big,
                                                                    _ => { break 'fast_while; }
                                                                };
                                                                vals[idx] = Value::Big(result_big);
                                                                // Continue with Value-based register loop
                                                                // Push to env and use the standard fast path
                                                                for (ii, n) in var_names.iter().enumerate() {
                                                                    env.insert(n.clone(), vals[ii].clone());
                                                                }
                                                                break 'fast_while;
                                                            }
                                                        }
                                                        continue;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    // Non-optimizable statement — fall back to general path
                                    // Push locals back to env first
                                    for (i, n) in var_names.iter().enumerate() {
                                        env.insert(n.clone(), Value::Int(locals[i]));
                                    }
                                    // Run remaining body via general path
                                    break 'fast_while;
                                }
                            }

                            // Push final values back to env
                            for (i, n) in var_names.iter().enumerate() {
                                env.insert(n.clone(), Value::Int(locals[i]));
                            }
                            // Check if loop completed (counter >= limit)
                            if locals[0] >= limit {
                                return Ok(Value::Unit);
                            }

                            // Overflowed to BigInt — run Value-based register loop (no HashMap per iteration)
                            let mut vals: Vec<Value> = var_names.iter()
                                .map(|n| env.get(n).cloned().unwrap_or(Value::Int(0)))
                                .collect();

                            'bigint_loop: loop {
                                // Check counter < limit
                                let counter_done = match &vals[0] {
                                    Value::Int(v) => *v >= limit,
                                    Value::Big(v) => *v >= BigInt::from(limit),
                                    _ => true,
                                };
                                if counter_done { break; }

                                for stmt in body.iter() {
                                    if let Statement::Assign { name, value: val_expr } = &stmt.node {
                                        if let Expr::BinaryOp { left: lhs, op, right: rhs } = &val_expr.node {
                                            if let Expr::Ident(ref lhs_name) = lhs.node {
                                                if lhs_name == name {
                                                    let idx = var_names.iter().position(|n| n == name);
                                                    if let Some(idx) = idx {
                                                        // Resolve RHS from registers
                                                        let rhs_val = match &rhs.node {
                                                            Expr::Literal(Literal::Int(n)) => Value::Int(*n),
                                                            Expr::Ident(ref rn) => {
                                                                var_names.iter().position(|n| n == rn)
                                                                    .map(|ri| vals[ri].clone())
                                                                    .unwrap_or(Value::Int(0))
                                                            }
                                                            Expr::BinaryOp { left: inner_l, op: inner_op, right: inner_r } => {
                                                                // Handle i * i, i * i * i etc
                                                                let l = match &inner_l.node {
                                                                    Expr::Ident(ref n) => var_names.iter().position(|vn| vn == n).map(|i| vals[i].clone()).unwrap_or(Value::Int(0)),
                                                                    Expr::Literal(Literal::Int(n)) => Value::Int(*n),
                                                                    _ => { break 'bigint_loop; }
                                                                };
                                                                let r = match &inner_r.node {
                                                                    Expr::Ident(ref n) => var_names.iter().position(|vn| vn == n).map(|i| vals[i].clone()).unwrap_or(Value::Int(0)),
                                                                    Expr::Literal(Literal::Int(n)) => Value::Int(*n),
                                                                    _ => { break 'bigint_loop; }
                                                                };
                                                                self.eval_binop_val(&l, inner_op.clone(), &r)
                                                            }
                                                            _ => { break 'bigint_loop; }
                                                        };
                                                        vals[idx] = self.eval_binop_val(&vals[idx], op.clone(), &rhs_val);
                                                        continue;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    // Can't optimize this stmt — push back and fall through
                                    for (ii, n) in var_names.iter().enumerate() {
                                        env.insert(n.clone(), vals[ii].clone());
                                    }
                                    break 'bigint_loop;
                                }
                            }

                            // Push final values back
                            for (i, n) in var_names.iter().enumerate() {
                                env.insert(n.clone(), vals[i].clone());
                            }
                            // Check if done
                            let counter_done = match &vals[0] {
                                Value::Int(v) => *v >= limit,
                                Value::Big(v) => *v >= BigInt::from(limit),
                                _ => true,
                            };
                            if counter_done {
                                return Ok(Value::Unit);
                            }
                        }

                        // Standard fast path: direct int comparison, skip eval_expr on condition
                        loop {
                            if let Some(Value::Int(current)) = env.get(&cn) {
                                if *current >= limit { break; }
                            } else { break; }
                            let result = if needs_scope {
                                self.exec_body_scoped(body, env, cell_name, signal_name)
                            } else {
                                self.exec_body(body, env, cell_name, signal_name)
                            };
                            match result {
                                Ok(_) => {}
                                Err(ExecError::Break) => break,
                                Err(ExecError::Continue) => {}
                                Err(e) => return Err(e),
                            }
                        }
                        return Ok(Value::Unit);
                    }
                }
                // General path
                loop {
                    let cond = self.eval_expr(&condition.node, env, cell_name, signal_name)?;
                    if !is_truthy(&cond) {
                        break;
                    }
                    match self.exec_body_scoped(body, env, cell_name, signal_name) {
                        Ok(_) => {}
                        Err(ExecError::Break) => break,
                        Err(ExecError::Continue) => {}
                        Err(e) => return Err(e),
                    }
                }
                Ok(Value::Unit)
            }

            Statement::ExprStmt { expr } => {
                self.last_span = Some(expr.span);
                self.eval_expr(&expr.node, env, cell_name, signal_name)
            }

            Statement::Require {
                constraint,
                else_signal,
            } => {
                let result = self.eval_constraint(&constraint.node, env, cell_name, signal_name)?;
                if !result {
                    Err(ExecError::Runtime(RuntimeError::RequireFailed(
                        format!("{}: constraint violated", else_signal)
                    )))
                } else {
                    Ok(Value::Unit)
                }
            }

            Statement::Emit { signal_name: sig, args } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(&arg.node, env, cell_name, signal_name)?);
                }
                let dispatch_args = arg_vals.clone();
                self.emitted_signals.push((sig.clone(), arg_vals.clone()));
                // Prepare data for broadcast
                let broadcast_data = if arg_vals.len() == 1 { arg_vals[0].clone() } else { Value::List(arg_vals) };
                // Broadcast to event bus (SSE clients)
                if let Some(ref bus) = self.event_bus {
                    let event = BusEvent {
                        stream: sig.clone(),
                        data: broadcast_data.clone(),
                    };
                    if let Ok(senders) = bus.lock() {
                        if !senders.is_empty() {
                        }
                        for sender in senders.iter() {
                            let _ = sender.send(event.clone());
                        }
                    }
                }
                // Send to peer bus (inter-process)
                if let Some(ref peers) = self.peer_bus {
                    let line = format!("EVENT {} {}\n", sig, broadcast_data);
                    if let Ok(senders) = peers.lock() {
                        for sender in senders.iter() {
                            let _ = sender.send(line.clone());
                        }
                    }
                }
                // Dispatch to sibling cells with matching handler (intra-process)
                let matching_cells: Vec<String> = self.handler_cache.keys()
                    .filter(|(c, s)| s == sig && c != cell_name)
                    .map(|(c, _)| c.clone())
                    .collect();
                for target_cell in matching_cells {
                    let _ = self.call_signal(&target_cell, sig, dispatch_args.clone());
                }
                Ok(Value::Unit)
            }

            Statement::MethodCall { target, method, args } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(&arg.node, env, cell_name, signal_name)?);
                }
                // Check if target is a memory slot with a storage backend
                self.call_storage_method(cell_name, target, method, &arg_vals)
            }
        }
    }

    fn eval_expr(
        &mut self,
        expr: &Expr,
        env: &mut Env,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<Value, ExecError> {
        match expr {
            Expr::Literal(lit) => {
                let val = self.eval_literal(lit);
                // Auto-interpolate strings: "Hello {name}" → "Hello Alice"
                if let Value::String(ref s) = val {
                    if s.contains('{') {
                        return Ok(Value::String(self.interpolate_string(s, env, cell_name, signal_name)));
                    }
                }
                Ok(val)
            }

            Expr::Ident(name) => env
                .get(name)
                .cloned()
                .ok_or_else(|| ExecError::Runtime(RuntimeError::UndefinedVar(name.clone()))),

            Expr::BinaryOp { left, op, right } => {
                // Short-circuit for logical And/Or
                if *op == BinOp::And {
                    let l = self.eval_expr(&left.node, env, cell_name, signal_name)?;
                    if !is_truthy(&l) {
                        return Ok(Value::Bool(false));
                    }
                    let r = self.eval_expr(&right.node, env, cell_name, signal_name)?;
                    return Ok(Value::Bool(is_truthy(&r)));
                }
                if *op == BinOp::Or {
                    let l = self.eval_expr(&left.node, env, cell_name, signal_name)?;
                    if is_truthy(&l) {
                        return Ok(Value::Bool(true));
                    }
                    let r = self.eval_expr(&right.node, env, cell_name, signal_name)?;
                    return Ok(Value::Bool(is_truthy(&r)));
                }
                let l = self.eval_expr(&left.node, env, cell_name, signal_name)?;
                let r = self.eval_expr(&right.node, env, cell_name, signal_name)?;
                self.eval_binop(&l, *op, &r).map_err(ExecError::Runtime)
            }

            Expr::CmpOp { left, op, right } => {
                let l = self.eval_expr(&left.node, env, cell_name, signal_name)?;
                let r = self.eval_expr(&right.node, env, cell_name, signal_name)?;
                self.eval_cmpop(&l, *op, &r).map_err(ExecError::Runtime)
            }

            Expr::FnCall { name, args } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(&arg.node, env, cell_name, signal_name)?);
                }

                // WebSocket builtins — need &mut self
                if name == "ws_connect" {
                    if let Some(Value::String(url)) = arg_vals.first() {
                        return self.do_ws_connect(url, cell_name)
                            .map_err(ExecError::Runtime);
                    }
                    return Err(ExecError::Runtime(RuntimeError::TypeError("ws_connect(url)".to_string())));
                }
                if name == "ws_send" {
                    if let Some(ref out) = self.ws_out {
                        let msg = match arg_vals.first() {
                            Some(Value::String(s)) => s.clone(),
                            Some(v) => format!("{}", v),
                            None => "{}".to_string(),
                        };
                        if let Ok(sender) = out.lock() {
                            let _ = sender.send(msg);
                        }
                        return Ok(Value::Unit);
                    }
                    return Err(ExecError::Runtime(RuntimeError::TypeError("ws_send: not connected".to_string())));
                }
                // connect(host:port) — open a TCP signal bus link
                if name == "link" {
                    if let Some(Value::String(addr)) = arg_vals.first() {
                        return self.do_connect(addr, cell_name)
                            .map_err(ExecError::Runtime);
                    }
                    return Err(ExecError::Runtime(RuntimeError::TypeError("connect(\"host:port\")".to_string())));
                }
                if name == "subscribe" {
                    if let Some(Value::String(url)) = arg_vals.first() {
                        return self.do_subscribe(url, cell_name)
                            .map_err(ExecError::Runtime);
                    }
                    return Err(ExecError::Runtime(RuntimeError::TypeError("subscribe(url)".to_string())));
                }

                // Check lambda builtins first (map, filter, find, etc.) — need &mut self
                if arg_vals.iter().any(|v| matches!(v, Value::Lambda { .. })) {
                    if let Some(val) = builtins::call_lambda_builtin(self, name, &arg_vals, cell_name) {
                        return val.map_err(ExecError::Runtime);
                    }
                }
                // Check builtins FIRST (before recursive calls)
                // This ensures list() calls the builtin even inside a "list" handler
                if let Some(val) = self.call_builtin(name, &arg_vals, cell_name) {
                    val.map_err(ExecError::Runtime)
                }
                // Then check for recursive call to current signal — use cached handler
                else if name == signal_name {
                    // Fast path: use cached handler to avoid HashMap lookup + key allocation
                    let cached = self.current_handler.as_ref().and_then(|(ch_cell, ch_sig, ch_params, ch_body)| {
                        if ch_cell == cell_name && ch_sig == signal_name {
                            Some((Arc::clone(ch_params), Arc::clone(ch_body)))
                        } else {
                            None
                        }
                    });
                    if let Some((params, body)) = cached {
                        self.call_signal_resolved(cell_name, signal_name, arg_vals, &params, &body)
                            .map_err(ExecError::Runtime)
                    } else {
                        self.call_signal(cell_name, signal_name, arg_vals)
                            .map_err(ExecError::Runtime)
                    }
                }
                // Is it a call to another cell's signal?
                else {
                    // Try to find a cell with a matching on-handler
                    let found_cell = self.cells.keys().find(|cn| {
                        self.cells[*cn].sections.iter().any(|s| {
                            if let Section::OnSignal(ref on) = s.node {
                                on.signal_name == *name
                            } else {
                                false
                            }
                        })
                    }).cloned();

                    if let Some(target_cell) = found_cell {
                        self.call_signal(&target_cell, name, arg_vals)
                            .map_err(ExecError::Runtime)
                    } else {
                        // Collect known names for "did you mean?" suggestion
                        let mut all_names: Vec<String> = self.handler_cache.keys()
                            .map(|(_, sig)| sig.clone())
                            .collect();
                        let builtins = [
                            "print", "len", "push", "map", "list", "filter", "sort_by", "filter_by",
                            "to_string", "to_int", "to_float", "from_json", "to_json", "reverse", "range",
                            "random", "abs", "round", "floor", "ceil", "min", "max", "clamp", "pow", "sqrt",
                            "contains", "starts_with", "ends_with", "replace", "split", "trim", "join",
                            "uppercase", "lowercase", "substring", "index_of", "concat", "type_of",
                            "now", "now_ms", "http_get", "http_post", "html", "response", "redirect",
                            "next_id", "transition", "get_status", "valid_transitions", "is_type",
                            "top", "bottom", "agg", "group_by", "distinct", "describe", "flatten", "zip",
                            "sum_by", "avg_by", "min_by", "max_by", "count_by", "escape_html",
                            "each", "find", "any", "all", "count", "sse", "link", "ws_connect", "ws_send",
                            "subscribe", "keys", "values", "sort", "append", "sleep",
                            "nth", "at", "read_file", "write_file", "read_csv",
                        ];
                        for b in &builtins { all_names.push(b.to_string()); }

                        let suggestion = all_names.iter()
                            .filter(|n| levenshtein(n, name) <= 3)
                            .min_by_key(|n| levenshtein(n, name))
                            .cloned()
                            .or_else(|| {
                                // Fallback: prefix match (e.g. "length" starts with "len")
                                all_names.iter()
                                    .find(|n| name.starts_with(n.as_str()) || n.starts_with(name))
                                    .cloned()
                            });

                        if let Some(did_you_mean) = suggestion {
                            Err(ExecError::Runtime(RuntimeError::UndefinedFn(
                                format!("{} (did you mean '{}'?)", name, did_you_mean)
                            )))
                        } else {
                            Err(ExecError::Runtime(RuntimeError::UndefinedFn(name.clone())))
                        }
                    }
                }
            }

            Expr::Not(inner) => {
                let val = self.eval_expr(&inner.node, env, cell_name, signal_name)?;
                let b = val.as_bool().map_err(ExecError::Runtime)?;
                Ok(Value::Bool(!b))
            }

            Expr::ListLiteral(elements) => {
                let mut items = Vec::with_capacity(elements.len());
                for elem in elements {
                    items.push(self.eval_expr(&elem.node, env, cell_name, signal_name)?);
                }
                Ok(Value::List(items))
            }

            Expr::Record { type_name, fields } => {
                // Record literal: User { name: "Alice", age: 30 }
                // Evaluates to a Map with a _type field for runtime type checking
                let mut entries = IndexMap::new();
                entries.insert("_type".to_string(), Value::String(type_name.clone()));
                for (field_name, field_expr) in fields {
                    let val = self.eval_expr(&field_expr.node, env, cell_name, signal_name)?;
                    entries.insert(field_name.clone(), val);
                }
                Ok(Value::Map(entries))
            }

            Expr::Try(inner) => {
                // try { expr } → returns map("value", result) or map("error", message)
                match self.eval_expr(&inner.node, env, cell_name, signal_name) {
                    Ok(val) => Ok(map_from_pairs(vec![
                        ("value".to_string(), val),
                        ("error".to_string(), Value::Unit),
                    ])),
                    Err(ExecError::Runtime(e)) => Ok(map_from_pairs(vec![
                        ("value".to_string(), Value::Unit),
                        ("error".to_string(), Value::String(format!("{}", e))),
                    ])),
                    Err(ExecError::Return(val)) => Err(ExecError::Return(val)),
                    Err(ExecError::Break) => Err(ExecError::Break),
                    Err(ExecError::Continue) => Err(ExecError::Continue),
                }
            }

            Expr::TryPropagate(inner) => {
                // expr? — evaluate inner, if result is a map with a non-unit error field, propagate it
                let val = self.eval_expr(&inner.node, env, cell_name, signal_name)?;
                match &val {
                    Value::Map(entries) => {
                        if let Some(err) = entries.get("error") {
                            if !matches!(err, Value::Unit) {
                                // Has an error — propagate by returning the error map
                                return Err(ExecError::Return(val));
                            }
                        }
                        // No error — unwrap value
                        Ok(entries.get("value").cloned().unwrap_or(val))
                    }
                    _ => Ok(val) // Not a result map, return as-is
                }
            }

            Expr::Lambda { param, body } => {
                // Capture current environment (convert FxHashMap → HashMap for storage in Value)
                let captured: HashMap<String, Value> = env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                Ok(Value::Lambda {
                    param: param.clone(),
                    body: body.clone(),
                    env: captured,
                })
            }

            Expr::LambdaBlock { param, stmts, result } => {
                // Capture current environment + statements
                let captured: HashMap<String, Value> = env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                Ok(Value::LambdaBlock {
                    param: param.clone(),
                    stmts: stmts.clone(),
                    result: result.clone(),
                    env: captured,
                })
            }

            Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
                let cond_val = self.eval_expr(&condition.node, env, cell_name, signal_name)?;
                if cond_val.is_truthy() {
                    for stmt in then_body {
                        self.last_span = Some(stmt.span);
                        self.exec_stmt(&stmt.node, env, cell_name, signal_name)?;
                    }
                    self.eval_expr(&then_result.node, env, cell_name, signal_name)
                } else {
                    for stmt in else_body {
                        self.last_span = Some(stmt.span);
                        self.exec_stmt(&stmt.node, env, cell_name, signal_name)?;
                    }
                    self.eval_expr(&else_result.node, env, cell_name, signal_name)
                }
            }

            Expr::Match { subject, arms } => {
                let val = self.eval_expr(&subject.node, env, cell_name, signal_name)?;
                for arm in arms {
                    let (matches, bindings) = self.match_pattern(&arm.pattern, &val);
                    if matches {
                        // Bind all variables captured by the pattern
                        for (name, bound_val) in &bindings {
                            env.insert(name.clone(), bound_val.clone());
                        }
                        // Evaluate guard clause if present
                        if let Some(ref guard) = arm.guard {
                            let guard_val = self.eval_expr(&guard.node, env, cell_name, signal_name)?;
                            if !guard_val.is_truthy() {
                                // Guard failed — unbind and try next arm
                                for (name, _) in &bindings {
                                    env.remove(name);
                                }
                                continue;
                            }
                        }
                        // Execute body statements, capturing last value
                        let mut last_val = Value::Unit;
                        for stmt in &arm.body {
                            self.last_span = Some(stmt.span);
                            last_val = self.exec_stmt(&stmt.node, env, cell_name, signal_name)?;
                        }
                        // If the result expression is Unit (parser couldn't extract it),
                        // use the last body statement's value instead
                        if matches!(arm.result.node, Expr::Literal(Literal::Unit)) && !arm.body.is_empty() {
                            return Ok(last_val);
                        }
                        return self.eval_expr(&arm.result.node, env, cell_name, signal_name);
                    }
                }
                // No match found
                Ok(Value::Unit)
            }

            Expr::Pipe { left, right } => {
                // Evaluate left side
                let left_val = self.eval_expr(&left.node, env, cell_name, signal_name)?;

                // Right side must be a FnCall — prepend left_val as first arg
                self.last_span = Some(right.span);
                match &right.node {
                    Expr::FnCall { name, args } => {
                        let mut all_args = vec![left_val];
                        for arg in args {
                            all_args.push(self.eval_expr(&arg.node, env, cell_name, signal_name)?);
                        }
                        // Check lambda builtins first (map, filter, etc.)
                        if all_args.iter().any(|v| matches!(v, Value::Lambda { .. } | Value::LambdaBlock { .. })) {
                            if let Some(val) = builtins::call_lambda_builtin(self, name, &all_args, cell_name) {
                                return val.map_err(ExecError::Runtime);
                            }
                        }
                        // Call as builtin first, then signal
                        if let Some(val) = self.call_builtin(name, &all_args, cell_name) {
                            val.map_err(ExecError::Runtime)
                        } else {
                            self.find_and_call_with_args(name, all_args)
                                .map_err(ExecError::Runtime)
                        }
                    }
                    Expr::Ident(name) => {
                        // Bare function: expr |> fn → fn(expr)
                        let all_args = vec![left_val];
                        if let Some(val) = self.call_builtin(name, &all_args, cell_name) {
                            val.map_err(ExecError::Runtime)
                        } else {
                            self.find_and_call_with_args(name, all_args)
                                .map_err(ExecError::Runtime)
                        }
                    }
                    Expr::FieldAccess { target, field } => {
                        // expr |> obj.method → method call with pipe value
                        let target_val = self.eval_expr(&target.node, env, cell_name, signal_name)?;
                        self.call_storage_method(cell_name, &format!("{}", target_val), field, &[left_val])
                    }
                    _ => Err(ExecError::Runtime(RuntimeError::TypeError(
                        "pipe (|>) right side must be a function call".to_string()
                    )))
                }
            }

            Expr::FieldAccess { target, field } => {
                // Check if target is an ident referring to a storage slot
                if let Expr::Ident(ref slot_name) = target.node {
                    // Try storage first, fall back to env variable
                    if self.storage.contains_key(slot_name)
                        || self.storage.contains_key(&format!("{}.{}", cell_name, slot_name))
                    {
                        return self.call_storage_method(cell_name, slot_name, field, &[]);
                    }
                }
                // Evaluate target and access field on the value
                let target_val = self.eval_expr(&target.node, env, cell_name, signal_name)?;
                match target_val {
                    Value::Map(ref entries) => {
                        // Built-in pseudo-fields for maps
                        match field.as_str() {
                            "keys" => return Ok(Value::List(entries.keys().map(|k| Value::String(k.clone())).collect())),
                            "values" => return Ok(Value::List(entries.values().cloned().collect())),
                            "length" | "len" | "size" => return Ok(Value::Int(entries.len() as i64)),
                            _ => {}
                        }
                        let val = entries.get(field).cloned().unwrap_or(Value::Unit);
                        Ok(val)
                    }
                    Value::List(ref items) => {
                        // list.length, list.len, list.first, list.last
                        match field.as_str() {
                            "length" | "len" => Ok(Value::Int(items.len() as i64)),
                            "first" => Ok(items.first().cloned().unwrap_or(Value::Unit)),
                            "last" => Ok(items.last().cloned().unwrap_or(Value::Unit)),
                            _ => {
                                // Try numeric index
                                if let Ok(idx) = field.parse::<usize>() {
                                    Ok(items.get(idx).cloned().unwrap_or(Value::Unit))
                                } else {
                                    Ok(Value::Unit)
                                }
                            }
                        }
                    }
                    Value::String(ref s) => {
                        match field.as_str() {
                            "length" | "len" => Ok(Value::Int(s.chars().count() as i64)),
                            _ => Ok(Value::Unit),
                        }
                    }
                    _ => Err(ExecError::Runtime(RuntimeError::TypeError(
                        format!("cannot access field '{}' on {:?}", field, target_val),
                    )))
                }
            }

            Expr::MethodCall { target, method, args } => {
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(&arg.node, env, cell_name, signal_name)?);
                }
                // Check if target is a storage slot
                if let Expr::Ident(ref slot_name) = target.node {
                    if self.storage.contains_key(slot_name)
                        || self.storage.contains_key(&format!("{}.{}", cell_name, slot_name))
                    {
                        return self.call_storage_method(cell_name, slot_name, method, &arg_vals);
                    }
                }
                // Evaluate target and call method on the value
                let target_val = self.eval_expr(&target.node, env, cell_name, signal_name)?;
                match (&target_val, method.as_str()) {
                    (Value::List(items), "get") => {
                        if let Some(Value::Int(idx)) = arg_vals.first() {
                            Ok(items.get(*idx as usize).cloned().unwrap_or(Value::Unit))
                        } else {
                            Ok(Value::Unit)
                        }
                    }
                    (Value::List(items), "len" | "length") => {
                        Ok(Value::Int(items.len() as i64))
                    }
                    (Value::Map(entries), "get") => {
                        if let Some(key) = arg_vals.first() {
                            let key_str = format!("{}", key);
                            Ok(entries.get(&key_str).cloned().unwrap_or(Value::Unit))
                        } else {
                            Ok(Value::Unit)
                        }
                    }
                    (Value::Map(entries), "keys") => {
                        Ok(Value::List(entries.keys().map(|k| Value::String(k.clone())).collect()))
                    }
                    (Value::Map(entries), "values") => {
                        Ok(Value::List(entries.values().cloned().collect()))
                    }
                    (Value::Map(entries), "has") => {
                        if let Some(key) = arg_vals.first() {
                            let key_str = format!("{}", key);
                            Ok(Value::Bool(entries.contains_key(&key_str)))
                        } else {
                            Ok(Value::Bool(false))
                        }
                    }
                    (Value::String(s), "len" | "length") => Ok(Value::Int(s.chars().count() as i64)),
                    (Value::String(s), "split") => {
                        if let Some(Value::String(delim)) = arg_vals.first() {
                            Ok(Value::List(s.split(delim.as_str()).map(|p| Value::String(p.to_string())).collect()))
                        } else {
                            Ok(Value::Unit)
                        }
                    }
                    _ => {
                        // Try storage as fallback
                        if let Expr::Ident(ref name) = target.node {
                            return self.call_storage_method(cell_name, name, method, &arg_vals);
                        }
                        Err(ExecError::Runtime(RuntimeError::TypeError(
                            format!("no method '{}' on {:?}", method, target_val),
                        )))
                    }
                }
            }

        }
    }

    /// Dispatch a method call to a storage backend.
    /// Handles: get(key), set(key, val), delete(key), append(val), len(), list()
    fn call_storage_method(
        &mut self,
        cell_name: &str,
        slot_name: &str,
        method: &str,
        args: &[Value],
    ) -> Result<Value, ExecError> {
        let prefixed = format!("{}.{}", cell_name, slot_name);
        let backend = self.storage.get(&prefixed)
            .or_else(|| self.storage.get(slot_name));

        let backend = match backend {
            Some(b) => b,
            None => {
                return Err(ExecError::Runtime(RuntimeError::TypeError(
                    format!("'{}' is not a memory slot (no storage backend)", slot_name),
                )));
            }
        };

        // Is this slot sharded across the cluster?
        let is_sharded = self.cluster.is_some()
            && (self.sharded_slots.contains_key(slot_name) || self.sharded_slots.contains_key(&prefixed));

        match method {
            "get" => {
                let key = args.first()
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "get() requires a key argument".to_string()
                    )))?;
                let key_str = format!("{}", key);

                // In cluster mode: check local first, then ask peers if not found
                if is_sharded {
                    if let Some(stored) = backend.get(&key_str) {
                        return Ok(auto_deserialize(stored_to_value(stored)));
                    }
                    if let Some(ref cluster) = self.cluster {
                        if !cluster.owns_key(&key_str) {
                            if let Some(val) = self.cluster_remote_get(slot_name, &key_str) {
                                return Ok(auto_deserialize(val));
                            }
                        }
                    }
                    return Ok(Value::Unit);
                }

                match backend.get(&key_str) {
                    Some(stored) => Ok(auto_deserialize(stored_to_value(stored))),
                    None => Ok(Value::Unit),
                }
            }
            "set" | "put" => {
                let key = args.first()
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "set() requires key and value arguments".to_string()
                    )))?;
                let val = args.get(1)
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "set() requires key and value arguments".to_string()
                    )))?;
                let key_str = format!("{}", key);
                let val_str = format!("{}", val);

                // Always write locally
                backend.set(&key_str, value_to_stored(val));

                // In cluster mode: broadcast to peers via EVENT bus
                if is_sharded {
                    self.cluster_broadcast_set(slot_name, &key_str, &val_str);
                }

                // Check memory invariants after set
                let prefixed_key = format!("{}.{}", cell_name, slot_name);
                let invs = self.invariants.get(&prefixed_key)
                    .or_else(|| self.invariants.get(slot_name))
                    .cloned()
                    .unwrap_or_default();
                if !invs.is_empty() {
                    // Re-fetch backend to get current state
                    let inv_backend = self.storage.get(&prefixed_key)
                        .or_else(|| self.storage.get(slot_name))
                        .cloned();
                    if let Some(inv_backend) = inv_backend {
                        for inv in &invs {
                            // Provide slot metadata as env variables for invariant evaluation
                            let mut env = FxHashMap::default();
                            env.insert("_slot_len".to_string(), Value::Int(inv_backend.len() as i64));
                            env.insert("_slot_name".to_string(), Value::String(slot_name.to_string()));
                            env.insert("_key".to_string(), Value::String(key_str.clone()));
                            let result = self.eval_expr(inv, &mut env, cell_name, "");
                            match result {
                                Ok(Value::Bool(true)) => {}
                                Ok(Value::Bool(false)) => {
                                    return Err(ExecError::Runtime(RuntimeError::RequireFailed(
                                        format!("memory invariant violated on '{}' after set(\"{}\")", slot_name, key_str)
                                    )));
                                }
                                _ => {}
                            }
                        }
                    }
                }

                Ok(Value::Unit)
            }
            "delete" | "remove" => {
                let key = args.first()
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "delete() requires a key argument".to_string()
                    )))?;
                let key_str = format!("{}", key);
                let removed = backend.delete(&key_str);

                // Broadcast delete to cluster
                if is_sharded {
                    self.cluster_broadcast_del(slot_name, &key_str);
                }
                Ok(Value::Bool(removed))
            }
            "append" | "push" => {
                let val = args.first()
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "append() requires a value argument".to_string()
                    )))?;
                backend.append(value_to_stored(val));
                Ok(Value::Unit)
            }
            "len" | "size" | "count" => {
                Ok(Value::Int(backend.len() as i64))
            }
            "list" | "all" | "entries" => {
                let items = backend.list();
                Ok(Value::List(items.into_iter().map(stored_to_value).collect()))
            }
            "keys" => {
                let keys = backend.keys();
                Ok(Value::List(keys.into_iter().map(Value::String).collect()))
            }
            "values" => {
                // In cluster mode: fan-out to all peers, merge with local
                if is_sharded {
                    return self.cluster_fan_out_values(slot_name, backend);
                }
                let vals = backend.values();
                Ok(Value::List(vals.into_iter().map(stored_to_value).collect()))
            }
            "has" | "contains" => {
                let key = args.first()
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "has() requires a key argument".to_string()
                    )))?;
                let key_str = format!("{}", key);
                Ok(Value::Bool(backend.has(&key_str)))
            }
            "backend" => {
                if is_sharded {
                    Ok(Value::String("cluster".to_string()))
                } else {
                    Ok(Value::String(backend.backend_name().to_string()))
                }
            }
            _ => {
                Err(ExecError::Runtime(RuntimeError::TypeError(
                    format!("unknown method '{}' on memory slot '{}'", method, slot_name),
                )))
            }
        }
    }

    // ── Cluster storage helpers ──────────────────────────────────────

    /// Broadcast a set operation to all peers via EVENT bus
    fn cluster_broadcast_set(&self, slot: &str, key: &str, value: &str) {
        if let Some(ref peers) = self.peer_bus {
            let msg = format!("EVENT _cluster_set {}\n",
                serde_json::json!({"slot": slot, "key": key, "value": value}));
            if let Ok(senders) = peers.lock() {
                for tx in senders.iter() {
                    let _ = tx.send(msg.clone());
                }
            }
        }
    }

    /// Broadcast a delete operation to all peers
    fn cluster_broadcast_del(&self, slot: &str, key: &str) {
        if let Some(ref peers) = self.peer_bus {
            let msg = format!("EVENT _cluster_del {}\n",
                serde_json::json!({"slot": slot, "key": key}));
            if let Ok(senders) = peers.lock() {
                for tx in senders.iter() {
                    let _ = tx.send(msg.clone());
                }
            }
        }
    }

    /// Request a value from the cluster (blocking with timeout)
    fn cluster_remote_get(&self, slot: &str, key: &str) -> Option<Value> {
        let cluster = self.cluster.as_ref()?;
        let req_id = cluster.next_req_id();
        let (tx, rx) = std::sync::mpsc::channel();

        cluster.pending.lock().unwrap().insert(req_id.clone(), tx);

        if let Some(ref peers) = self.peer_bus {
            let msg = format!("EVENT _cluster_get {}\n",
                serde_json::json!({"slot": slot, "key": key, "req_id": req_id}));
            if let Ok(senders) = peers.lock() {
                for sender in senders.iter() {
                    let _ = sender.send(msg.clone());
                }
            }
        }

        match rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(value) => {
                cluster.pending.lock().unwrap().remove(&req_id);
                if value.is_empty() { None } else { Some(Value::String(value)) }
            }
            Err(_) => {
                cluster.pending.lock().unwrap().remove(&req_id);
                None
            }
        }
    }

    /// Fan-out values() to all peers, merge with local
    fn cluster_fan_out_values(&self, slot: &str, local_backend: &Arc<dyn StorageBackend>) -> Result<Value, ExecError> {
        // Start with local values
        let mut all_values: Vec<Value> = local_backend.values()
            .into_iter().map(stored_to_value).collect();

        // Fan-out to peers
        if let (Some(ref cluster), Some(ref peers)) = (&self.cluster, &self.peer_bus) {
            let req_id = cluster.next_req_id();
            let (tx, rx) = std::sync::mpsc::channel();
            cluster.pending.lock().unwrap().insert(req_id.clone(), tx);

            let msg = format!("EVENT _cluster_values {}\n",
                serde_json::json!({"slot": slot, "req_id": req_id}));
            let peer_count = if let Ok(senders) = peers.lock() {
                for sender in senders.iter() {
                    let _ = sender.send(msg.clone());
                }
                senders.len()
            } else {
                0
            };

            // Collect replies with timeout (best-effort)
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let mut replies = 0;
            while replies < peer_count {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() { break; }
                match rx.recv_timeout(remaining) {
                    Ok(json_str) => {
                        replies += 1;
                        // Parse the values array from the reply
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
                            if let Some(arr) = parsed.as_array() {
                                for v in arr {
                                    if let Some(s) = v.as_str() {
                                        all_values.push(Value::String(s.to_string()));
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            cluster.pending.lock().unwrap().remove(&req_id);
        }

        Ok(Value::List(all_values))
    }

    /// Evaluate a constraint expression, returning true/false
    fn eval_constraint(
        &mut self,
        constraint: &Constraint,
        env: &mut Env,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<bool, ExecError> {
        match constraint {
            Constraint::Comparison { left, op, right } => {
                let l = self.eval_expr(&left.node, env, cell_name, signal_name)?;
                let r = self.eval_expr(&right.node, env, cell_name, signal_name)?;
                let result = self.eval_cmpop(&l, *op, &r).map_err(ExecError::Runtime)?;
                result.as_bool().map_err(ExecError::Runtime)
            }
            Constraint::Predicate { name, args: _ } => {
                // Evaluate as a boolean expression
                if let Some(val) = env.get(name) {
                    return val.as_bool().map_err(ExecError::Runtime);
                }
                // Unknown predicates pass
                Ok(true)
            }
            Constraint::And(a, b) => {
                let ra = self.eval_constraint(&a.node, env, cell_name, signal_name)?;
                let rb = self.eval_constraint(&b.node, env, cell_name, signal_name)?;
                Ok(ra && rb)
            }
            Constraint::Or(a, b) => {
                let ra = self.eval_constraint(&a.node, env, cell_name, signal_name)?;
                let rb = self.eval_constraint(&b.node, env, cell_name, signal_name)?;
                Ok(ra || rb)
            }
            Constraint::Not(inner) => {
                let r = self.eval_constraint(&inner.node, env, cell_name, signal_name)?;
                Ok(!r)
            }
            Constraint::Descriptive(_) => Ok(true),
        }
    }

    /// Interpolate {var} in a string from the local scope.
    /// If var is not found in scope, leave {var} as-is (for render() compatibility).
    fn interpolate_string(&mut self, s: &str, env: &mut Env, cell_name: &str, signal_name: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut pos = 0;
        while pos < s.len() {
            if s.as_bytes()[pos] == b'{' {
                if let Some(end) = s[pos + 1..].find('}') {
                    let expr_str = &s[pos + 1..pos + 1 + end];

                    // Skip empty or HTML-like content (class names, CSS)
                    if expr_str.is_empty() || expr_str.contains(':') || expr_str.contains(';') {
                        result.push('{');
                        pos += 1;
                        continue;
                    }

                    // Try to parse and evaluate the expression
                    let eval_result = self.eval_interpolation_expr(expr_str, env, cell_name, signal_name);
                    if let Some(val) = eval_result {
                        result.push_str(&format!("{}", val));
                        pos = pos + 1 + end + 1;
                        continue;
                    }
                }
            }
            // Properly handle multi-byte UTF-8 characters
            let ch = &s[pos..];
            if let Some(c) = ch.chars().next() {
                result.push(c);
                pos += c.len_utf8();
            } else {
                pos += 1;
            }
        }
        result
    }

    /// Parse and evaluate an expression string from interpolation
    fn eval_interpolation_expr(&mut self, expr_str: &str, env: &mut Env, cell_name: &str, signal_name: &str) -> Option<Value> {
        // Fast path: simple variable name
        if expr_str.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return match env.get(expr_str) {
                Some(val) => Some(val.clone()),
                None => Some(Value::String(format!("<error: undefined variable: {}>", expr_str))),
            };
        }
        // Fast path: var.field
        if expr_str.contains('.') && !expr_str.contains('(') && !expr_str.contains(' ') {
            let parts: Vec<&str> = expr_str.splitn(2, '.').collect();
            if parts.len() == 2 {
                if let Some(val) = env.get(parts[0]) {
                    if let Value::Map(ref entries) = val {
                        if let Some(field_val) = entries.get(parts[1]) {
                            return Some(field_val.clone());
                        }
                    }
                    // Try .length etc
                    match parts[1] {
                        "length" | "len" => {
                            if let Value::List(items) = val { return Some(Value::Int(items.len() as i64)); }
                            if let Value::String(s) = val { return Some(Value::Int(s.len() as i64)); }
                        }
                        _ => {}
                    }
                    return Some(Value::String(format!("<error: no field '{}' on {}>", parts[1], parts[0])));
                } else {
                    return Some(Value::String(format!("<error: undefined variable: {}>", parts[0])));
                }
            }
        }
        // Full expression: parse and eval
        let wrapped = format!("cell _T {{ on _e() {{ return {} }} }}", expr_str);
        let mut lexer = crate::lexer::Lexer::new(&wrapped);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(e) => return Some(Value::String(format!("<error: parse error in '{{{}}}': {}>", expr_str, e))),
        };
        let mut parser = crate::parser::Parser::new(tokens);
        let program = match parser.parse_program() {
            Ok(p) => p,
            Err(e) => return Some(Value::String(format!("<error: parse error in '{{{}}}': {}>", expr_str, e))),
        };
        let cell = match program.cells.first() {
            Some(c) => c,
            None => return Some(Value::String(format!("<error: failed to parse '{{{}}}'>", expr_str))),
        };
        let section = match cell.node.sections.first() {
            Some(s) => s,
            None => return Some(Value::String(format!("<error: failed to parse '{{{}}}'>", expr_str))),
        };
        if let crate::ast::Section::OnSignal(ref on) = section.node {
            if let Some(stmt) = on.body.first() {
                if let crate::ast::Statement::Return { ref value } = stmt.node {
                    match self.eval_expr(&value.node, env, cell_name, signal_name) {
                        Ok(val) => return Some(val),
                        Err(e) => return Some(Value::String(format!("<error: {:?}>", e))),
                    }
                }
            }
        }
        Some(Value::String(format!("<error: failed to evaluate '{{{}}}'>", expr_str)))
    }

    fn eval_literal(&self, lit: &Literal) -> Value {
        match lit {
            Literal::Int(n) => Value::Int(*n),
            Literal::BigInt(s) => Value::Big(s.parse::<BigInt>().unwrap_or_default()),
            Literal::Float(n) => Value::Float(*n),
            Literal::String(s) => Value::String(s.clone()),
            Literal::Bool(b) => Value::Bool(*b),
            Literal::Duration(d) => {
                let ms = match d.unit {
                    DurationUnit::Milliseconds => d.value,
                    DurationUnit::Seconds => d.value * 1000.0,
                    DurationUnit::Minutes => d.value * 60_000.0,
                    DurationUnit::Hours => d.value * 3_600_000.0,
                    DurationUnit::Days => d.value * 86_400_000.0,
                    DurationUnit::Years => d.value * 365.25 * 86_400_000.0,
                };
                Value::Int(ms as i64)
            }
            Literal::Percentage(p) => Value::Float(*p),
            Literal::Unit => Value::Unit,
        }
    }

    /// Match a pattern against a value, returning (matches, bindings)
    fn match_pattern(&self, pattern: &MatchPattern, val: &Value) -> (bool, Vec<(String, Value)>) {
        match pattern {
            MatchPattern::Wildcard => (true, vec![]),
            MatchPattern::Literal(lit) => {
                let lit_val = self.eval_literal(lit);
                (self.values_equal(val, &lit_val), vec![])
            }
            MatchPattern::Variable(name) => {
                (true, vec![(name.clone(), val.clone())])
            }
            MatchPattern::Or(alternatives) => {
                for alt in alternatives {
                    let (m, bindings) = self.match_pattern(alt, val);
                    if m { return (true, bindings); }
                }
                (false, vec![])
            }
            MatchPattern::MapDestructure(fields) => {
                if let Value::Map(entries) = val {
                    let mut bindings = Vec::new();
                    for (field_name, sub_pattern) in fields {
                        let field_val = entries.get(field_name).cloned().unwrap_or(Value::Unit);
                        let (m, sub_bindings) = self.match_pattern(sub_pattern, &field_val);
                        if !m { return (false, vec![]); }
                        bindings.extend(sub_bindings);
                    }
                    (true, bindings)
                } else {
                    (false, vec![])
                }
            }
            MatchPattern::StringPrefix { prefix, rest } => {
                if let Value::String(s) = val {
                    if s.starts_with(prefix.as_str()) {
                        let remainder = s[prefix.len()..].to_string();
                        (true, vec![(rest.clone(), Value::String(remainder))])
                    } else {
                        (false, vec![])
                    }
                } else {
                    (false, vec![])
                }
            }
            MatchPattern::Range { from, to } => {
                match val {
                    Value::Int(i) => (*from <= *i && *i <= *to, vec![]),
                    Value::Float(f) => ((*from as f64) <= *f && *f <= (*to as f64), vec![]),
                    _ => (false, vec![]),
                }
            }
        }
    }

    /// Open a persistent WebSocket client connection
    fn do_ws_connect(&mut self, url: &str, cell_name: &str) -> Result<Value, RuntimeError> {
        use tungstenite::connect;

        // Connect raw TcpStream and do WS handshake
        let parsed = url::Url::parse(url).map_err(|e| {
            RuntimeError::TypeError(format!("ws_connect: bad URL: {}", e))
        })?;
        let host = parsed.host_str().unwrap_or("localhost");
        let port = parsed.port().unwrap_or(80);

        let stream = std::net::TcpStream::connect(format!("{}:{}", host, port)).map_err(|e| {
            RuntimeError::TypeError(format!("ws_connect: {}", e))
        })?;
        // Set read timeout so read() doesn't block forever — allows checking outgoing channel
        stream.set_read_timeout(Some(std::time::Duration::from_millis(50))).ok();

        let (ws, _) = tungstenite::client::client(url, stream).map_err(|e| {
            RuntimeError::TypeError(format!("ws handshake: {}", e))
        })?;

        // Outgoing channel
        let (out_tx, out_rx) = std::sync::mpsc::channel::<String>();
        self.ws_out = Some(Arc::new(std::sync::Mutex::new(out_tx)));

        // Writer thread: owns the WS, sends outgoing messages
        // Incoming messages are handled via SSE (separate channel)
        std::thread::spawn(move || {
            let mut ws = ws;
            for msg in out_rx {
                if ws.send(tungstenite::Message::Text(msg)).is_err() { break; }
                let _ = ws.flush();
            }
            eprintln!("ws: writer thread ended");
        });

        // ws_connect is send-only. Use subscribe() for receiving.
        // This separation avoids the read/write deadlock in tungstenite.

        eprintln!("ws: connected to {}", url);
        Ok(map_from_pairs(vec![
            ("status".to_string(), Value::String("connected".to_string())),
            ("url".to_string(), Value::String(url.to_string())),
        ]))
    }

    /// Apply a lambda to a value: bind param, eval body
    /// Connect to a remote cell via TCP signal bus
    pub fn do_connect(&mut self, addr: &str, cell_name: &str) -> Result<Value, RuntimeError> {
        let stream = std::net::TcpStream::connect(addr).map_err(|e| {
            RuntimeError::TypeError(format!("connect: {}", e))
        })?;
        let read_stream = stream.try_clone().map_err(|e| {
            RuntimeError::TypeError(format!("connect clone: {}", e))
        })?;

        // Writer: sends outgoing signals to this peer
        let (tx, rx) = std::sync::mpsc::channel::<String>();

        // Register this peer's sender in the peer bus
        if let Some(ref peers) = self.peer_bus {
            if let Ok(mut senders) = peers.lock() {
                senders.push(tx);
            }
        }

        // Writer thread
        let mut write_stream = stream;
        std::thread::spawn(move || {
            use std::io::Write;
            for line in rx {
                if write_stream.write_all(line.as_bytes()).is_err() { break; }
                if write_stream.flush().is_err() { break; }
            }
        });

        // Reader thread: reads EVENT lines, dispatches to handlers
        let cells = self.cells.clone();
        let storage: HashMap<String, Arc<dyn StorageBackend>> = self.storage.iter()
            .map(|(k, v)| (k.clone(), v.clone())).collect();
        let state_machines = self.state_machines.clone();
        let event_bus = self.event_bus.clone();
        let peer_bus = self.peer_bus.clone();
        let ws_out = self.ws_out.clone();
        let cname = cell_name.to_string();

        std::thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(read_stream);
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };
                // Parse: EVENT name json_data
                if line.starts_with("EVENT ") {
                    let rest = &line[6..];
                    if let Some(space) = rest.find(' ') {
                        let event_name = &rest[..space];
                        let json_data = &rest[space+1..];

                        let data = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_data) {
                            builtins::serde_json_to_value(&parsed)
                        } else {
                            Value::String(json_data.to_string())
                        };

                        // Dispatch to on event_name(data) handler
                        let prog = Program { imports: vec![], cells: cells.values().map(|c| {
                            Spanned::new(c.clone(), Span::new(0, 0))
                        }).collect() };
                        let mut interp = Interpreter::new(&prog);
                        for (k, v) in &storage {
                            interp.storage.insert(k.clone(), v.clone());
                        }
                        interp.state_machines = state_machines.clone();
                        interp.event_bus = event_bus.clone();
                        interp.peer_bus = peer_bus.clone();
                        interp.ws_out = ws_out.clone();

                        // Try all cells to find the handler
                        let mut handled = false;
                        for cn in interp.cells.keys().cloned().collect::<Vec<_>>() {
                            if interp.handler_cache.contains_key(&(cn.clone(), event_name.to_string())) {
                                match interp.call_signal(&cn, event_name, vec![data.clone()]) {
                                    Ok(_) => { handled = true; break; }
                                    Err(_) => {}
                                }
                            }
                        }
                        if !handled {
                            eprintln!("[bus] no handler for '{}'", event_name);
                        }
                    }
                }
            }
            eprintln!("connect: peer disconnected");
        });

        eprintln!("connect: linked to {}", addr);
        Ok(map_from_pairs(vec![
            ("status".to_string(), Value::String("connected".to_string())),
            ("peer".to_string(), Value::String(addr.to_string())),
        ]))
    }

    /// Subscribe to a remote WS stream — dedicated read-only connection
    /// Incoming messages are parsed as {"event":"name","data":{...}} and dispatched to on name() handlers
    fn do_subscribe(&mut self, url: &str, cell_name: &str) -> Result<Value, RuntimeError> {
        let parsed = url::Url::parse(url).map_err(|e| {
            RuntimeError::TypeError(format!("subscribe: bad URL: {}", e))
        })?;
        let host = parsed.host_str().unwrap_or("localhost");
        let port = parsed.port().unwrap_or(80);

        let stream = std::net::TcpStream::connect(format!("{}:{}", host, port)).map_err(|e| {
            RuntimeError::TypeError(format!("subscribe: {}", e))
        })?;

        let (ws, _) = tungstenite::client::client(url, stream).map_err(|e| {
            RuntimeError::TypeError(format!("subscribe handshake: {}", e))
        })?;

        // Reader thread: blocks on read, dispatches to handlers
        let cells = self.cells.clone();
        let storage: HashMap<String, Arc<dyn StorageBackend>> = self.storage.iter()
            .map(|(k, v)| (k.clone(), v.clone())).collect();
        let state_machines = self.state_machines.clone();
        let event_bus = self.event_bus.clone();
        let ws_out = self.ws_out.clone();
        let cname = cell_name.to_string();

        let url_owned = url.to_string();
        std::thread::spawn(move || {
            let mut ws = ws;
            eprintln!("subscribe: listening on {}", url_owned);
            loop {
                match ws.read() {
                    Ok(tungstenite::Message::Text(text)) => {
                        // Parse {"event":"trade","data":{...}} format
                        let prog = Program { imports: vec![], cells: cells.values().map(|c| {
                            Spanned::new(c.clone(), Span::new(0, 0))
                        }).collect() };
                        let mut interp = Interpreter::new(&prog);
                        for (k, v) in &storage {
                            interp.storage.insert(k.clone(), v.clone());
                        }
                        interp.state_machines = state_machines.clone();
                        interp.event_bus = event_bus.clone();
                        interp.ws_out = ws_out.clone();

                        // Try to parse as bus event format
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                            if let Some(event_name) = parsed.get("event").and_then(|e| e.as_str()) {
                                let data = parsed.get("data")
                                    .map(|d| builtins::serde_json_to_value(d))
                                    .unwrap_or(Value::String(text.clone()));
                                // Dispatch to on event_name(data)
                                let _ = interp.call_signal(&cname, event_name, vec![data]);
                                continue;
                            }
                        }
                        // Fallback: dispatch to on ws(message)
                        let _ = interp.call_signal(&cname, "ws", vec![Value::String(text)]);
                    }
                    Ok(tungstenite::Message::Close(_)) => {
                        eprintln!("subscribe: connection closed");
                        break;
                    }
                    Err(e) => {
                        eprintln!("subscribe: error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });

        Ok(map_from_pairs(vec![
            ("status".to_string(), Value::String("subscribed".to_string())),
            ("url".to_string(), Value::String(url.to_string())),
        ]))
    }

    /// Evaluate an expression with the given environment. Used by the VM for
    /// interpreter fallback (e.g. try expressions).
    pub fn eval_expr_with_env(
        &mut self,
        expr: &Expr,
        env: &HashMap<String, Value>,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<Value, ExecError> {
        let mut fx_env: Env = env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        self.eval_expr(expr, &mut fx_env, cell_name, signal_name)
    }

    pub(crate) fn apply_lambda(&mut self, lambda: &Value, arg: Value, cell_name: &str) -> Result<Value, ExecError> {
        match lambda {
            Value::Lambda { param, body, env: closed_env } => {
                let mut env: Env = closed_env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                env.insert(param.clone(), arg);
                self.eval_expr(&body.node, &mut env, cell_name, "")
            }
            Value::LambdaBlock { param, stmts, result, env: closed_env } => {
                let mut env: Env = closed_env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                env.insert(param.clone(), arg);
                let mut last_val = Value::Unit;
                for stmt in stmts {
                    last_val = self.exec_stmt(&stmt.node, &mut env, cell_name, "")?;
                }
                if matches!(result.node, Expr::Literal(Literal::Unit)) && !stmts.is_empty() {
                    return Ok(last_val);
                }
                self.eval_expr(&result.node, &mut env, cell_name, "")
            }
            _ => Err(ExecError::Runtime(RuntimeError::TypeError(
                format!("expected lambda, got {}", lambda)
            )))
        }
    }

    fn values_equal(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => x == y,
            (Value::Big(x), Value::Big(y)) => x == y,
            (Value::Big(x), Value::Int(y)) => *x == BigInt::from(*y),
            (Value::Int(x), Value::Big(y)) => BigInt::from(*x) == *y,
            (Value::Float(x), Value::Float(y)) => x == y,
            (Value::Int(x), Value::Float(y)) => (*x as f64) == *y,
            (Value::Float(x), Value::Int(y)) => *x == (*y as f64),
            (Value::String(x), Value::String(y)) => x == y,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::Unit, Value::Unit) => true,
            _ => false,
        }
    }

    /// Convenience wrapper that returns Value (panics on error — for register loop only)
    fn eval_binop_val(&self, l: &Value, op: BinOp, r: &Value) -> Value {
        self.eval_binop(l, op, r).unwrap_or(Value::Unit)
    }

    fn eval_binop(&self, l: &Value, op: BinOp, r: &Value) -> Result<Value, RuntimeError> {
        // BigInt + Float → promote both to Float
        if (l.is_big() && matches!(r, Value::Float(_))) || (matches!(l, Value::Float(_)) && r.is_big()) {
            let a = l.as_float()?;
            let b = r.as_float()?;
            return match op {
                BinOp::Add => Ok(Value::Float(a + b)),
                BinOp::Sub => Ok(Value::Float(a - b)),
                BinOp::Mul => Ok(Value::Float(a * b)),
                BinOp::Div => Ok(Value::Float(a / b)),
                BinOp::Mod => Ok(Value::Float(a % b)),
                _ => Err(RuntimeError::TypeError("invalid op for floats".to_string())),
            };
        }

        // If either operand is BigInt, promote both
        if l.is_big() || r.is_big() {
            let a = l.as_bigint()?;
            let b = r.as_bigint()?;
            return match op {
                BinOp::Add => Ok(Value::Big(a + b)),
                BinOp::Sub => Ok(Value::Big(a - b)),
                BinOp::Mul => Ok(Value::Big(a * b)),
                BinOp::Div => {
                    if b.is_zero() {
                        Err(RuntimeError::TypeError("division by zero".to_string()))
                    } else {
                        Ok(Value::Big(a / b))
                    }
                }
                BinOp::Mod => {
                    if b.is_zero() {
                        Err(RuntimeError::TypeError("modulo by zero".to_string()))
                    } else {
                        Ok(Value::Big(a % b))
                    }
                }
                BinOp::And => Ok(Value::Bool(!a.is_zero() && !b.is_zero())),
                BinOp::Or => Ok(Value::Bool(!a.is_zero() || !b.is_zero())),
            };
        }

        match (l, r) {
            (Value::Int(a), Value::Int(b)) => match op {
                BinOp::Add => Ok(a.checked_add(*b).map(Value::Int)
                    .unwrap_or_else(|| Value::Big(BigInt::from(*a) + BigInt::from(*b)))),
                BinOp::Sub => Ok(a.checked_sub(*b).map(Value::Int)
                    .unwrap_or_else(|| Value::Big(BigInt::from(*a) - BigInt::from(*b)))),
                BinOp::Mul => Ok(a.checked_mul(*b).map(Value::Int)
                    .unwrap_or_else(|| Value::Big(BigInt::from(*a) * BigInt::from(*b)))),
                BinOp::Div => {
                    if *b == 0 {
                        Err(RuntimeError::TypeError("division by zero".to_string()))
                    } else if a % b == 0 {
                        Ok(Value::Int(a / b))
                    } else {
                        Ok(Value::Float(*a as f64 / *b as f64))
                    }
                }
                BinOp::Mod => {
                    if *b == 0 {
                        Err(RuntimeError::TypeError("modulo by zero".to_string()))
                    } else {
                        Ok(Value::Int(a % b))
                    }
                }
                BinOp::And => Ok(Value::Bool(*a != 0 && *b != 0)),
                BinOp::Or => Ok(Value::Bool(*a != 0 || *b != 0)),
            },
            (Value::Float(_), _) | (_, Value::Float(_)) => {
                let a = l.as_float()?;
                let b = r.as_float()?;
                match op {
                    BinOp::Add => Ok(Value::Float(a + b)),
                    BinOp::Sub => Ok(Value::Float(a - b)),
                    BinOp::Mul => Ok(Value::Float(a * b)),
                    BinOp::Div => Ok(Value::Float(a / b)),
                    BinOp::Mod => Ok(Value::Float(a % b)),
                    _ => Err(RuntimeError::TypeError("invalid op for floats".to_string())),
                }
            }
            (Value::String(a), Value::String(b)) => match op {
                BinOp::Add => Ok(Value::String(format!("{}{}", a, b))),
                _ => Err(RuntimeError::TypeError("invalid op for strings".to_string())),
            },
            (Value::List(a), Value::List(b)) => match op {
                BinOp::Add => {
                    let mut result = a.clone();
                    result.extend(b.clone());
                    Ok(Value::List(result))
                }
                _ => Err(RuntimeError::TypeError("invalid op for lists".to_string())),
            },
            (Value::Bool(a), Value::Bool(b)) => match op {
                BinOp::And => Ok(Value::Bool(*a && *b)),
                BinOp::Or => Ok(Value::Bool(*a || *b)),
                _ => Err(RuntimeError::TypeError("invalid op for bools".to_string())),
            },
            _ => Err(RuntimeError::TypeError(format!(
                "cannot {} {} and {}: {} {} {}",
                binop_verb(op), value_type_name(l), value_type_name(r), l, op, r
            ))),
        }
    }

    /// Public comparison for use by the test runner
    pub fn eval_cmpop_values(&self, l: &Value, op: CmpOp, r: &Value) -> Result<bool, RuntimeError> {
        match self.eval_cmpop(l, op, r)? {
            Value::Bool(b) => Ok(b),
            _ => Ok(false),
        }
    }

    fn eval_cmpop(&self, l: &Value, op: CmpOp, r: &Value) -> Result<Value, RuntimeError> {
        // BigInt + Float comparison → promote to Float
        if (l.is_big() && matches!(r, Value::Float(_))) || (matches!(l, Value::Float(_)) && r.is_big()) {
            let a = l.as_float()?;
            let b = r.as_float()?;
            let result = match op {
                CmpOp::Lt => a < b,
                CmpOp::Gt => a > b,
                CmpOp::Le => a <= b,
                CmpOp::Ge => a >= b,
                CmpOp::Eq => a == b,
                CmpOp::Ne => a != b,
            };
            return Ok(Value::Bool(result));
        }

        // BigInt comparisons (both BigInt or Int+BigInt)
        if l.is_big() || r.is_big() {
            let a = l.as_bigint()?;
            let b = r.as_bigint()?;
            let result = match op {
                CmpOp::Lt => a < b,
                CmpOp::Gt => a > b,
                CmpOp::Le => a <= b,
                CmpOp::Ge => a >= b,
                CmpOp::Eq => a == b,
                CmpOp::Ne => a != b,
            };
            return Ok(Value::Bool(result));
        }

        // Handle Unit comparisons first (before type coercion)
        if matches!(l, Value::Unit) || matches!(r, Value::Unit) {
            let result = match op {
                CmpOp::Eq => matches!((l, r), (Value::Unit, Value::Unit)),
                CmpOp::Ne => !matches!((l, r), (Value::Unit, Value::Unit)),
                _ => false,
            };
            return Ok(Value::Bool(result));
        }

        match (l, r) {
            (Value::Int(a), Value::Int(b)) => {
                let result = match op {
                    CmpOp::Lt => a < b,
                    CmpOp::Gt => a > b,
                    CmpOp::Le => a <= b,
                    CmpOp::Ge => a >= b,
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                };
                Ok(Value::Bool(result))
            }
            (Value::Float(_), _) | (_, Value::Float(_)) => {
                let a = l.as_float()?;
                let b = r.as_float()?;
                let result = match op {
                    CmpOp::Lt => a < b,
                    CmpOp::Gt => a > b,
                    CmpOp::Le => a <= b,
                    CmpOp::Ge => a >= b,
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                };
                Ok(Value::Bool(result))
            }
            // String comparison
            (Value::String(a), Value::String(b)) => {
                let result = match op {
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                    CmpOp::Lt => a < b,
                    CmpOp::Gt => a > b,
                    CmpOp::Le => a <= b,
                    CmpOp::Ge => a >= b,
                };
                Ok(Value::Bool(result))
            }
            // Bool comparison
            (Value::Bool(a), Value::Bool(b)) => {
                let result = match op {
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                    _ => return Err(RuntimeError::TypeError("cannot order booleans".to_string())),
                };
                Ok(Value::Bool(result))
            }
            // Unit comparison (null checks)
            (Value::Unit, Value::Unit) => {
                Ok(Value::Bool(matches!(op, CmpOp::Eq | CmpOp::Le | CmpOp::Ge)))
            }
            // Anything compared to Unit (null check)
            (Value::Unit, _) | (_, Value::Unit) => {
                Ok(Value::Bool(matches!(op, CmpOp::Ne)))
            }
            _ => Err(RuntimeError::TypeError(format!(
                "cannot compare {} and {}",
                value_type_name(l), value_type_name(r)
            ))),
        }
    }

    /// Native function boundary. The names here correspond to `native "name"`
    /// in `cell builtin` definitions. This is the thin kernel — everything
    /// above is Soma. Delegates to sub-modules in builtins/.
    pub fn call_builtin(&mut self, name: &str, args: &[Value], cell_name: &str) -> Option<Result<Value, RuntimeError>> {
        builtins::call_builtin(self, name, args, cell_name)
    }

    /// Execute a state transition
    pub(crate) fn do_transition(&mut self, id: &str, target: &str) -> Result<Value, RuntimeError> {
        self.do_transition_for("", id, target)
    }

    pub(crate) fn do_transition_for(&mut self, cell_name: &str, id: &str, target: &str) -> Result<Value, RuntimeError> {
        let (sm, status_slot) = self.find_state_machine_for(cell_name)
            .ok_or_else(|| RuntimeError::TypeError("no state machine found".to_string()))?;

        // Get current state
        let current = status_slot.get(id)
            .map(|v| match v {
                crate::runtime::storage::StoredValue::String(s) => s,
                _ => format!("{}", v),
            })
            .unwrap_or(sm.initial.clone());

        // Find matching transition
        let transition = sm.transitions.iter().find(|t| {
            (t.node.from == current || t.node.from == "*") && t.node.to == target
        });

        let transition = match transition {
            Some(t) => t,
            None => {
                let valid: Vec<String> = sm.transitions.iter()
                    .filter(|t| t.node.from == current || t.node.from == "*")
                    .map(|t| t.node.to.clone())
                    .collect();
                return Err(RuntimeError::RequireFailed(format!(
                    "invalid transition: {} → {}. Current state: '{}'. Valid targets: [{}]",
                    current, target, current, valid.join(", ")
                )));
            }
        };

        // Clone guard if present (to release borrow on self before eval_expr)
        let guard_clone = transition.node.guard.as_ref().map(|g| g.node.clone());

        // Evaluate guard expression if present
        if let Some(guard_expr) = guard_clone {
            let mut env = FxHashMap::default();
            // Provide the current state and target as variables in guard scope
            env.insert("_from".to_string(), Value::String(current.clone()));
            env.insert("_to".to_string(), Value::String(target.to_string()));
            env.insert("_id".to_string(), Value::String(id.to_string()));
            let result = self.eval_expr(&guard_expr, &mut env, "", "")
                .map_err(|e| match e {
                    ExecError::Runtime(r) => r,
                    ExecError::Return(v) => RuntimeError::TypeError(format!("guard returned {:?}", v)),
                    _ => RuntimeError::TypeError("guard evaluation failed".to_string()),
                })?;
            match result {
                Value::Bool(true) => {} // guard passed
                Value::Bool(false) => {
                    return Err(RuntimeError::RequireFailed(format!(
                        "guard failed for transition {} → {}: condition is false",
                        current, target
                    )));
                }
                _ => {
                    return Err(RuntimeError::TypeError(format!(
                        "guard must return Bool, got {}",
                        value_type_name(&result)
                    )));
                }
            }
        }

        // Re-find state machine storage after potential mutation from eval_expr
        let (_sm, status_slot) = self.find_state_machine_for(cell_name)
            .ok_or_else(|| RuntimeError::TypeError("no state machine found".to_string()))?;

        // Perform transition
        status_slot.set(id, crate::runtime::storage::StoredValue::String(target.to_string()));

        Ok(map_from_pairs(vec![
            ("id".to_string(), Value::String(id.to_string())),
            ("from".to_string(), Value::String(current)),
            ("to".to_string(), Value::String(target.to_string())),
        ]))
    }

    pub(crate) fn do_get_status(&self, id: &str) -> Result<Value, RuntimeError> {
        self.do_get_status_for("", id)
    }

    pub(crate) fn do_get_status_for(&self, cell_name: &str, id: &str) -> Result<Value, RuntimeError> {
        let (sm, status_slot) = self.find_state_machine_for(cell_name)
            .ok_or_else(|| RuntimeError::TypeError("no state machine found".to_string()))?;

        let current = status_slot.get(id)
            .map(|v| match v {
                crate::runtime::storage::StoredValue::String(s) => s,
                _ => format!("{}", v),
            })
            .unwrap_or(sm.initial.clone());

        Ok(Value::String(current))
    }

    pub(crate) fn do_valid_transitions(&self, id: &str) -> Value {
        self.do_valid_transitions_for("", id)
    }

    pub(crate) fn do_valid_transitions_for(&self, cell_name: &str, id: &str) -> Value {
        let Some((sm, status_slot)) = self.find_state_machine_for(cell_name) else {
            return Value::List(vec![]);
        };

        let current = status_slot.get(id)
            .map(|v| match v {
                crate::runtime::storage::StoredValue::String(s) => s,
                _ => format!("{}", v),
            })
            .unwrap_or(sm.initial.clone());

        let targets: Vec<Value> = sm.transitions.iter()
            .filter(|t| t.node.from == current || t.node.from == "*")
            .map(|t| Value::String(t.node.to.clone()))
            .collect();

        Value::List(targets)
    }

    /// Find the first state machine and its backing storage slot
    pub(crate) fn find_state_machine(&self) -> Option<(&StateMachineSection, &Arc<dyn StorageBackend>)> {
        self.find_state_machine_for("")
    }

    pub(crate) fn find_state_machine_for(&self, cell_name: &str) -> Option<(&StateMachineSection, &Arc<dyn StorageBackend>)> {
        // First: try cell-scoped key (multi-cell programs)
        if !cell_name.is_empty() {
            for ((cn, sm_name), sm) in &self.state_machines {
                if cn == cell_name {
                    let scoped_key = format!("__sm_{}_{}", cn, sm_name);
                    if let Some(backend) = self.storage.get(&scoped_key) {
                        return Some((sm, backend));
                    }
                    // Fallback to legacy key
                    let legacy_key = format!("__sm_{}", sm_name);
                    if let Some(backend) = self.storage.get(&legacy_key) {
                        return Some((sm, backend));
                    }
                }
            }
        }
        // Fallback: any state machine (backwards compat)
        for ((cn, sm_name), sm) in &self.state_machines {
            let scoped_key = format!("__sm_{}_{}", cn, sm_name);
            let legacy_key = format!("__sm_{}", sm_name);
            if let Some(backend) = self.storage.get(&scoped_key).or_else(|| self.storage.get(&legacy_key)) {
                return Some((sm, backend));
            }
        }
        None
    }
}

/// Check if a type expression refers to BigInt
fn is_bigint_type(ty: &TypeExpr) -> bool {
    matches!(ty, TypeExpr::Simple(name) if name == "BigInt")
}

/// Convert a runtime Value to a StoredValue
pub(crate) fn value_to_stored(val: &Value) -> StoredValue {
    match val {
        Value::Int(n) => StoredValue::Int(*n),
        Value::Big(n) => StoredValue::String(n.to_string()),
        Value::Float(n) => StoredValue::Float(*n),
        Value::String(s) => StoredValue::String(s.clone()),
        Value::Bool(b) => StoredValue::Bool(*b),
        Value::List(items) => StoredValue::List(items.iter().map(value_to_stored).collect()),
        Value::Map(entries) => StoredValue::Map(
            entries.iter().map(|(k, v)| (k.clone(), value_to_stored(v))).collect()
        ),
        Value::Lambda { .. } | Value::LambdaBlock { .. } => StoredValue::String("<lambda>".to_string()),
        Value::Unit => StoredValue::Null,
    }
}

pub(crate) fn stored_to_value(stored: StoredValue) -> Value {
    match stored {
        StoredValue::Int(n) => Value::Int(n),
        StoredValue::Float(n) => Value::Float(n),
        StoredValue::String(s) => Value::String(s),
        StoredValue::Bool(b) => Value::Bool(b),
        StoredValue::Null => Value::Unit,
        StoredValue::List(items) => Value::List(items.into_iter().map(stored_to_value).collect()),
        StoredValue::Map(map) => Value::Map(
            map.into_iter().map(|(k, v)| (k, stored_to_value(v))).collect()
        ),
    }
}

/// Auto-deserialize: if a Value::String looks like JSON (starts with { or [),
/// parse it into a Map or List. This handles the common case where old code
/// used to_json() before .set(), making .get() return a raw JSON string.
pub(crate) fn auto_deserialize(val: Value) -> Value {
    if let Value::String(ref s) = val {
        let trimmed = s.trim();
        if (trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']'))
        {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
                return json_to_value(&parsed);
            }
        }
    }
    val
}

/// Convert serde_json::Value to interpreter Value
pub fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Unit,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Value::List(arr.iter().map(json_to_value).collect())
        }
        serde_json::Value::Object(obj) => {
            let entries: indexmap::IndexMap<String, Value> = obj.iter()
                .map(|(k, v)| (k.clone(), json_to_value(v)))
                .collect();
            Value::Map(entries)
        }
    }
}

/// Internal error type to handle return-as-control-flow
#[derive(Debug)]
pub(crate) enum ExecError {
    Return(Value),
    Break,
    Continue,
    Runtime(RuntimeError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn run(source: &str, cell: &str, signal: &str, args: Vec<Value>) -> Result<Value, RuntimeError> {
        // Run on a thread with 8 MB stack so recursive tests don't SIGABRT
        let source = source.to_string();
        let cell = cell.to_string();
        let signal = signal.to_string();
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let mut lexer = Lexer::new(&source);
                let tokens = lexer.tokenize().unwrap();
                let mut parser = Parser::new(tokens);
                let program = parser.parse_program().unwrap();
                let mut interp = Interpreter::new(&program);
                // Set up storage for memory sections
                for prog_cell in &program.cells {
                    for section in &prog_cell.node.sections {
                        if let crate::ast::Section::Memory(ref mem) = section.node {
                            let mut slots = std::collections::HashMap::new();
                            for slot in &mem.slots {
                                let backend: std::sync::Arc<dyn crate::runtime::storage::StorageBackend> =
                                    std::sync::Arc::new(crate::runtime::storage::MemoryBackend::new());
                                slots.insert(slot.node.name.clone(), backend);
                            }
                            interp.set_storage(&prog_cell.node.name, &slots);
                        }
                    }
                }
                interp.ensure_state_machine_storage();
                interp.call_signal(&cell, &signal, args)
            })
            .expect("failed to spawn test thread")
            .join()
            .expect("test thread panicked")
    }

    #[test]
    fn test_factorial() {
        let source = r#"
            cell Fact {
                on compute(n: Int) {
                    if n <= 1 {
                        return 1
                    }
                    return n * compute(n - 1)
                }
            }
        "#;
        let result = run(source, "Fact", "compute", vec![Value::Int(5)]).unwrap();
        assert_eq!(result.as_int().unwrap(), 120);
    }

    #[test]
    fn test_factorial_int_auto_promotes_to_bigint() {
        // Int overflow auto-promotes to BigInt instead of erroring
        let source = r#"
            cell Fact {
                on compute(n: Int) {
                    if n <= 1 { return 1 }
                    return n * compute(n - 1)
                }
            }
        "#;
        let result = run(source, "Fact", "compute", vec![Value::Int(30)]);
        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(matches!(val, Value::Big(_)));
        assert_eq!(val.to_string(), "265252859812191058636308480000000");
    }

    #[test]
    fn test_factorial_bigint() {
        // BigInt handles arbitrary precision
        let source = r#"
            cell Fact {
                on compute(n: BigInt) {
                    if n <= 1 { return 1 }
                    return n * compute(n - 1)
                }
            }
        "#;
        let result = run(source, "Fact", "compute", vec![Value::Int(30)]).unwrap();
        assert!(matches!(result, Value::Big(_)));
        assert_eq!(
            result.to_string(),
            "265252859812191058636308480000000"
        );
    }

    #[test]
    fn test_fibonacci() {
        let source = r#"
            cell Fib {
                on compute(n: Int) {
                    if n <= 1 {
                        return n
                    }
                    return compute(n - 1) + compute(n - 2)
                }
            }
        "#;
        let result = run(source, "Fib", "compute", vec![Value::Int(10)]).unwrap();
        assert_eq!(result.as_int().unwrap(), 55);
    }

    #[test]
    fn test_let_and_arithmetic() {
        let source = r#"
            cell Math {
                on add(a: Int, b: Int) {
                    let sum = a + b
                    return sum * 2
                }
            }
        "#;
        let result = run(source, "Math", "add", vec![Value::Int(3), Value::Int(4)]).unwrap();
        assert_eq!(result.as_int().unwrap(), 14);
    }

    #[test]
    fn test_if_else() {
        let source = r#"
            cell Logic {
                on max(a: Int, b: Int) {
                    if a > b {
                        return a
                    } else {
                        return b
                    }
                }
            }
        "#;
        let result = run(source, "Logic", "max", vec![Value::Int(3), Value::Int(7)]).unwrap();
        assert_eq!(result.as_int().unwrap(), 7);
    }

    // ── Match patterns ──────────────────────────────────────────────

    #[test]
    fn test_match_variable_binding() {
        let source = r#"
            cell T {
                on run(x: Int) {
                    return match x {
                        name -> name + 100
                    }
                }
            }
        "#;
        let result = run(source, "T", "run", vec![Value::Int(7)]).unwrap();
        assert_eq!(result.as_int().unwrap(), 107);
    }

    #[test]
    fn test_match_or_pattern() {
        let source = r#"
            cell T {
                on run(s: String) {
                    return match s {
                        "a" || "b" -> "matched"
                        _ -> "no"
                    }
                }
            }
        "#;
        let a = run(source, "T", "run", vec![Value::String("a".into())]).unwrap();
        assert_eq!(a.to_string(), "matched");
        let b = run(source, "T", "run", vec![Value::String("b".into())]).unwrap();
        assert_eq!(b.to_string(), "matched");
        let c = run(source, "T", "run", vec![Value::String("z".into())]).unwrap();
        assert_eq!(c.to_string(), "no");
    }

    #[test]
    fn test_match_map_destructure() {
        let source = r#"
            cell T {
                on run() {
                    let req = map("method", "GET", "path", "/home")
                    return match req {
                        {method: "GET", path} -> path
                        _ -> "other"
                    }
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "/home");
    }

    #[test]
    fn test_match_string_prefix() {
        let source = r#"
            cell T {
                on run(s: String) {
                    return match s {
                        "/api/" + rest -> rest
                        _ -> "no match"
                    }
                }
            }
        "#;
        let result = run(source, "T", "run", vec![Value::String("/api/users".into())]).unwrap();
        assert_eq!(result.to_string(), "users");
        let miss = run(source, "T", "run", vec![Value::String("/home".into())]).unwrap();
        assert_eq!(miss.to_string(), "no match");
    }

    #[test]
    fn test_match_guard_clause() {
        let source = r#"
            cell T {
                on run(n: Int) {
                    return match n {
                        x if x > 0 -> "positive"
                        x if x < 0 -> "negative"
                        _ -> "zero"
                    }
                }
            }
        "#;
        let pos = run(source, "T", "run", vec![Value::Int(5)]).unwrap();
        assert_eq!(pos.to_string(), "positive");
        let neg = run(source, "T", "run", vec![Value::Int(-3)]).unwrap();
        assert_eq!(neg.to_string(), "negative");
        let zero = run(source, "T", "run", vec![Value::Int(0)]).unwrap();
        assert_eq!(zero.to_string(), "zero");
    }

    #[test]
    fn test_match_range_pattern() {
        let source = r#"
            cell T {
                on run(n: Int) {
                    return match n {
                        0..10 -> "small"
                        _ -> "big"
                    }
                }
            }
        "#;
        let small = run(source, "T", "run", vec![Value::Int(5)]).unwrap();
        assert_eq!(small.to_string(), "small");
        let big = run(source, "T", "run", vec![Value::Int(15)]).unwrap();
        assert_eq!(big.to_string(), "big");
    }

    #[test]
    fn test_match_nested_destructure() {
        let source = r#"
            cell T {
                on run() {
                    let req = map("method", "POST", "path", "/api/orders")
                    return match req {
                        {method: "POST", path: "/api/" + r} -> r
                        _ -> "no"
                    }
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "orders");
    }

    // ── Expressions ─────────────────────────────────────────────────

    #[test]
    fn test_if_expression() {
        let source = r#"
            cell T {
                on run() {
                    let x = if true { 42 } else { 0 }
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 42);
    }

    #[test]
    fn test_if_expression_else_if() {
        let source = r#"
            cell T {
                on run() {
                    let x = if false { 1 } else if true { 2 } else { 3 }
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 2);
    }

    #[test]
    fn test_match_as_expression() {
        let source = r#"
            cell T {
                on run() {
                    let x = match "a" {
                        "a" -> 1
                        _ -> 0
                    }
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 1);
    }

    // ── Operators ───────────────────────────────────────────────────

    #[test]
    fn test_compound_minus_eq() {
        let source = r#"
            cell T {
                on run() {
                    let x = 10
                    x -= 3
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 7);
    }

    #[test]
    fn test_compound_star_eq() {
        let source = r#"
            cell T {
                on run() {
                    let x = 5
                    x *= 2
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 10);
    }

    #[test]
    fn test_compound_slash_eq() {
        let source = r#"
            cell T {
                on run() {
                    let x = 20
                    x /= 4
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 5);
    }

    #[test]
    fn test_try_propagate() {
        let source = r#"
            cell T {
                on run() {
                    let v = try { 42 }?
                    return v
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 42);
    }

    #[test]
    fn test_try_propagate_error() {
        let source = r#"
            cell T {
                on run() {
                    let v = try { 1 / 0 }?
                    return v
                }
            }
        "#;
        // Division by zero causes try to wrap an error, then ? propagates it
        // The result should be a map with an error field (returned via early return)
        let result = run(source, "T", "run", vec![]).unwrap();
        // ? propagates by returning the error map
        assert!(matches!(result, Value::Map(_)));
        if let Value::Map(ref entries) = result {
            assert!(entries.get("error").is_some());
            let err = entries.get("error").unwrap();
            assert!(!matches!(err, Value::Unit));
        }
    }

    #[test]
    fn test_null_coalesce_precedence() {
        // () ?? 5 should evaluate to 5, and then == 5 should be true
        // This tests that ?? binds tighter than == (or at least correctly)
        let source = r#"
            cell T {
                on run() {
                    let x = () ?? 5
                    return x
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 5);
    }

    // ── Statements ──────────────────────────────────────────────────

    #[test]
    fn test_ensure_pass() {
        let source = r#"
            cell T {
                on run() {
                    ensure true
                    return "ok"
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "ok");
    }

    #[test]
    fn test_ensure_fail() {
        let source = r#"
            cell T {
                on run() {
                    ensure false
                    return "ok"
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_implicit_return() {
        // Handler without explicit return should return the last expression
        let source = r#"
            cell T {
                on run() {
                    let x = 10
                    let y = 20
                    x + y
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 30);
    }

    // ── Storage auto-deserialize ────────────────────────────────────

    #[test]
    fn test_auto_deserialize_json_string() {
        let json_str = r#"{"name": "alice", "age": 30}"#;
        let val = Value::String(json_str.to_string());
        let result = auto_deserialize(val);
        assert!(matches!(result, Value::Map(_)));
        if let Value::Map(ref entries) = result {
            assert_eq!(entries.get("name").unwrap().to_string(), "alice");
            assert_eq!(entries.get("age").unwrap().as_int().unwrap(), 30);
        }
    }

    // ── Inter-agent communication ──────────────────────────────────

    #[test]
    fn test_emit_dispatches_to_sibling_cell() {
        let source = r#"
            cell A {
                on run() {
                    emit ping(map("from", "A"))
                    return "emitted"
                }
            }
            cell B {
                memory { log: Map<String, String> [ephemeral] }
                on ping(data: Map) {
                    log.set("got", data.from)
                }
            }
        "#;
        let result = run(source, "A", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "emitted");
    }

    #[test]
    fn test_gather_fan_out() {
        let source = r#"
            cell Worker {
                on process(item: String) {
                    return "done:" + item
                }
            }
            cell Main {
                on run() {
                    return gather(list("x", "y", "z"), "Worker", "process")
                }
            }
        "#;
        let result = run(source, "Main", "run", vec![]).unwrap();
        if let Value::List(items) = result {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0].to_string(), "done:x");
            assert_eq!(items[2].to_string(), "done:z");
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn test_broadcast_to_multiple_cells() {
        let source = r#"
            cell A {
                on alert(msg: String) { return "A:" + msg }
            }
            cell B {
                on alert(msg: String) { return "B:" + msg }
            }
            cell Main {
                on run() {
                    return broadcast("alert", "fire")
                }
            }
        "#;
        let result = run(source, "Main", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 2);
    }

    #[test]
    fn test_delegate_cross_cell() {
        let source = r#"
            cell Helper {
                on double(n: Int) { return n * 2 }
            }
            cell Main {
                on run() {
                    return delegate("Helper", "double", 21)
                }
            }
        "#;
        let result = run(source, "Main", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 42);
    }

    // ── Agent cell features ────────────────────────────────────────

    #[test]
    fn test_cell_agent_with_state_machine() {
        let source = r#"
            cell agent Bot {
                state w { initial: idle  idle -> done  * -> failed }
                on run() {
                    transition("t", "done")
                    return get_status("t")
                }
            }
        "#;
        let result = run(source, "Bot", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "done");
    }

    #[test]
    fn test_cell_agent_emit_and_delegate() {
        // Agent A delegates to agent B, B transitions and returns
        let source = r#"
            cell agent Worker {
                state w { initial: idle  idle -> done  * -> failed }
                on process(x: Int) {
                    transition("t", "done")
                    return x * 10
                }
            }
            cell Main {
                on run() {
                    return delegate("Worker", "process", 5)
                }
            }
        "#;
        let result = run(source, "Main", "run", vec![]).unwrap();
        assert_eq!(result.as_int().unwrap(), 50);
    }

    #[test]
    fn test_state_machine_isolation_between_cells() {
        let source = r#"
            cell agent A {
                state sa { initial: idle  idle -> doneA  * -> fail }
                on go() {
                    transition("t", "doneA")
                    return get_status("t")
                }
            }
            cell agent B {
                state sb { initial: idle  idle -> doneB  * -> fail }
                on go() {
                    transition("t", "doneB")
                    return get_status("t")
                }
            }
            cell Main {
                on run() {
                    let a = delegate("A", "go")
                    let b = delegate("B", "go")
                    return list(a, b)
                }
            }
        "#;
        let result = run(source, "Main", "run", vec![]).unwrap();
        if let Value::List(items) = result {
            assert_eq!(items[0].to_string(), "doneA");
            assert_eq!(items[1].to_string(), "doneB");
        } else {
            panic!("expected list");
        }
    }

    // ── Mock mode ──────────────────────────────────────────────────

    #[test]
    fn test_think_mock_and_budget_and_trace() {
        // Combined test to avoid env var races in parallel test execution
        std::env::set_var("SOMA_LLM_MOCK", "echo");

        // Echo mode
        let source = r#"
            cell agent Bot {
                state w { initial: idle  idle -> done  * -> failed }
                on run() {
                    transition("t", "done")
                    return think("hello world")
                }
            }
        "#;
        let result = run(source, "Bot", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "hello world");

        // Fixed mode
        std::env::set_var("SOMA_LLM_MOCK", "fixed:42");
        let source2 = r#"
            cell agent Bot2 {
                state w { initial: idle  idle -> done  * -> failed }
                on run() {
                    transition("t", "done")
                    return think("anything")
                }
            }
        "#;
        let result2 = run(source2, "Bot2", "run", vec![]).unwrap();
        assert_eq!(result2.to_string(), "42");

        // Token tracking in mock mode
        std::env::set_var("SOMA_LLM_MOCK", "echo");
        let source3 = r#"
            cell agent Bot3 {
                state w { initial: idle  idle -> done  * -> failed }
                on run() {
                    set_budget(1000)
                    transition("t", "done")
                    think("test")
                    return tokens_used()
                }
            }
        "#;
        let result3 = run(source3, "Bot3", "run", vec![]).unwrap();
        assert_eq!(result3.as_int().unwrap(), 0);

        // Trace records think calls
        let source4 = r#"
            cell agent Bot4 {
                state w { initial: idle  idle -> done  * -> failed }
                on run() {
                    transition("t", "done")
                    think("test prompt")
                    return trace()
                }
            }
        "#;
        let result4 = run(source4, "Bot4", "run", vec![]).unwrap();
        if let Value::List(entries) = result4 {
            assert!(!entries.is_empty(), "trace should have entries");
        } else {
            panic!("expected list from trace()");
        }

        std::env::remove_var("SOMA_LLM_MOCK");
    }

    // ── Transition error messages ──────────────────────────────────

    #[test]
    fn test_transition_typo_shows_valid_states() {
        let source = r#"
            cell T {
                state w { initial: idle  idle -> done }
                memory { x: Map<String, String> [ephemeral] }
                on run() {
                    return transition("t", "doen")
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("done"), "error should show valid state 'done': {}", err);
    }

    #[test]
    fn test_transition_without_memory_section() {
        let source = r#"
            cell T {
                state w { initial: idle  idle -> done }
                on run() {
                    transition("t", "done")
                    return get_status("t")
                }
            }
        "#;
        let result = run(source, "T", "run", vec![]).unwrap();
        assert_eq!(result.to_string(), "done");
    }
}
