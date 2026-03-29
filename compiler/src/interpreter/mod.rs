pub mod builtins;

use std::collections::HashMap;
use std::sync::Arc;
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
    #[error("type error: {0}")]
    TypeError(String),
    #[error("no handler found for signal '{0}' in cell '{1}'")]
    NoHandler(String, String),
    #[error("require failed: {0}")]
    RequireFailed(String),
    #[error("stack overflow (recursion depth exceeded)")]
    StackOverflow,
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

/// Format an error with file location if available
pub fn format_runtime_error(
    err: &RuntimeError,
    source_file: Option<&str>,
    source_text: Option<&str>,
    span: Option<crate::ast::Span>,
) -> String {
    let location = match (source_file, source_text, span) {
        (Some(file), Some(text), Some(sp)) => {
            let (line, col) = span_to_location(text, sp.start);
            format!("{}:{}:{}: ", file, line, col)
        }
        (Some(file), _, _) => format!("{}: ", file),
        _ => String::new(),
    };
    format!("{}runtime error: {}", location, err)
}

/// Check if a Value is truthy (false for Bool(false), Unit, Int(0); true otherwise)
pub fn is_truthy(val: &Value) -> bool {
    match val {
        Value::Bool(b) => *b,
        Value::Unit => false,
        Value::Int(n) => *n != 0,
        Value::Big(n) => !n.is_zero(),
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
    Map(Vec<(String, Value)>),
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
type HandlerValue = (Vec<Param>, Vec<Spanned<Statement>>);

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
    cells: HashMap<String, CellDef>,
    /// Pre-computed handler lookup — avoids scanning sections on every call
    handler_cache: HashMap<HandlerKey, HandlerValue>,
    /// Maximum recursion depth
    max_depth: usize,
    current_depth: usize,
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
                    let value = (on.params.clone(), on.body.clone());
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
        Self {
            cells,
            handler_cache,
            max_depth: 10_000,
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
        }
    }

    /// Register an additional cell definition (used by runtime to inject interior cells)
    pub fn register_cell(&mut self, cell: CellDef) {
        // Update handler cache
        for section in &cell.sections {
            if let Section::OnSignal(ref on) = section.node {
                let key = (cell.name.clone(), on.signal_name.clone());
                let value = (on.params.clone(), on.body.clone());
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

    /// Execute an `every` block's body
    pub fn exec_every(&mut self, body: &[Spanned<Statement>], env: &mut HashMap<String, Value>, cell_name: &str) -> Result<Value, RuntimeError> {
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
        // Check if any existing slot uses a persistent backend
        let has_persistent = self.storage.values().any(|b| b.backend_name() == "sqlite" || b.backend_name() == "file");
        for ((cell_name, sm_name), _) in self.state_machines.clone() {
            let key = format!("__sm_{}", sm_name);
            if !self.storage.contains_key(&key) {
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
        // Lookup from pre-computed cache — O(1) instead of scanning sections
        let key = (cell_name.to_string(), signal_name.to_string());
        let (params, body) = self.handler_cache.get(&key)
            .cloned()
            .ok_or_else(|| RuntimeError::NoHandler(signal_name.to_string(), cell_name.to_string()))?;

        self.current_depth += 1;
        if self.current_depth > self.max_depth {
            self.current_depth -= 1;
            return Err(RuntimeError::StackOverflow);
        }

        // Check arity
        if args.len() != params.len() {
            self.current_depth -= 1;
            return Err(RuntimeError::TypeError(format!(
                "expected {} arguments, got {}",
                params.len(),
                args.len()
            )));
        }

        // Bind parameters, promoting Int → BigInt if the type declares BigInt
        let mut env = HashMap::with_capacity(params.len() + 8);
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

        let result = self.exec_body(&body, &mut env, cell_name, signal_name);

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
        env: &mut HashMap<String, Value>,
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

    fn exec_stmt(
        &mut self,
        stmt: &Statement,
        env: &mut HashMap<String, Value>,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<Value, ExecError> {
        match stmt {
            Statement::Let { name, value } => {
                let val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                env.insert(name.clone(), val);
                Ok(Value::Unit)
            }

            Statement::Assign { name, value } => {
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
                // += optimization: items += val → in-place append for lists, in-place concat for strings
                // (handled by BinaryOp::Add desugaring, but we can optimize the common case)
                let val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                env.insert(name.clone(), val);
                Ok(Value::Unit)
            }

            Statement::Return { value } => {
                let val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                Err(ExecError::Return(val))
            }

            Statement::Break => {
                Err(ExecError::Break)
            }

            Statement::Continue => {
                Err(ExecError::Continue)
            }

            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                let cond = self.eval_expr(&condition.node, env, cell_name, signal_name)?;
                if cond.as_bool().map_err(ExecError::Runtime)? {
                    self.exec_body(then_body, env, cell_name, signal_name)
                } else if !else_body.is_empty() {
                    self.exec_body(else_body, env, cell_name, signal_name)
                } else {
                    Ok(Value::Unit)
                }
            }

            Statement::For { var, iter, body } => {
                // Evaluate the iterator expression
                let iter_val = self.eval_expr(&iter.node, env, cell_name, signal_name)?;

                // Convert to iterable items
                let items = match iter_val {
                    Value::List(items) => items,
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
                    match self.exec_body(body, env, cell_name, signal_name) {
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
                let mut iterations = 0;
                loop {
                    let cond = self.eval_expr(&condition.node, env, cell_name, signal_name)?;
                    if !cond.as_bool().map_err(ExecError::Runtime)? {
                        break;
                    }
                    match self.exec_body(body, env, cell_name, signal_name) {
                        Ok(_) => {}
                        Err(ExecError::Break) => break,
                        Err(ExecError::Continue) => {}
                        Err(e) => return Err(e),
                    }
                    iterations += 1;
                    if iterations > 1_000_000 {
                        return Err(ExecError::Runtime(RuntimeError::StackOverflow));
                    }
                }
                Ok(Value::Unit)
            }

            Statement::ExprStmt { expr } => {
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
        env: &mut HashMap<String, Value>,
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
                // Then check for recursive call to current signal
                else if name == signal_name {
                    self.call_signal(cell_name, signal_name, arg_vals)
                        .map_err(ExecError::Runtime)
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
                        Err(ExecError::Runtime(RuntimeError::UndefinedFn(name.clone())))
                    }
                }
            }

            Expr::Not(inner) => {
                let val = self.eval_expr(&inner.node, env, cell_name, signal_name)?;
                let b = val.as_bool().map_err(ExecError::Runtime)?;
                Ok(Value::Bool(!b))
            }

            Expr::Record { type_name, fields } => {
                // Record literal: User { name: "Alice", age: 30 }
                // Evaluates to a Map with a _type field for runtime type checking
                let mut entries = vec![("_type".to_string(), Value::String(type_name.clone()))];
                for (field_name, field_expr) in fields {
                    let val = self.eval_expr(&field_expr.node, env, cell_name, signal_name)?;
                    entries.push((field_name.clone(), val));
                }
                Ok(Value::Map(entries))
            }

            Expr::Try(inner) => {
                // try { expr } → returns map("value", result) or map("error", message)
                match self.eval_expr(&inner.node, env, cell_name, signal_name) {
                    Ok(val) => Ok(Value::Map(vec![
                        ("value".to_string(), val),
                        ("error".to_string(), Value::Unit),
                    ])),
                    Err(ExecError::Runtime(e)) => Ok(Value::Map(vec![
                        ("value".to_string(), Value::Unit),
                        ("error".to_string(), Value::String(format!("{}", e))),
                    ])),
                    Err(ExecError::Return(val)) => Err(ExecError::Return(val)),
                    Err(ExecError::Break) => Err(ExecError::Break),
                    Err(ExecError::Continue) => Err(ExecError::Continue),
                }
            }

            Expr::Lambda { param, body } => {
                // Capture current environment
                Ok(Value::Lambda {
                    param: param.clone(),
                    body: body.clone(),
                    env: env.clone(),
                })
            }

            Expr::LambdaBlock { param, stmts, result } => {
                // Capture current environment + statements
                // Store stmts as a serialized form inside the lambda
                // We'll handle this in apply_lambda
                Ok(Value::LambdaBlock {
                    param: param.clone(),
                    stmts: stmts.clone(),
                    result: result.clone(),
                    env: env.clone(),
                })
            }

            Expr::Match { subject, arms } => {
                let val = self.eval_expr(&subject.node, env, cell_name, signal_name)?;
                for arm in arms {
                    let matches = match &arm.pattern {
                        MatchPattern::Wildcard => true,
                        MatchPattern::Literal(lit) => {
                            let lit_val = self.eval_literal(lit);
                            self.values_equal(&val, &lit_val)
                        }
                    };
                    if matches {
                        // Execute body statements
                        for stmt in &arm.body {
                            self.last_span = Some(stmt.span);
                            self.exec_stmt(&stmt.node, env, cell_name, signal_name)?;
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
                            "keys" => return Ok(Value::List(entries.iter().map(|(k, _)| Value::String(k.clone())).collect())),
                            "values" => return Ok(Value::List(entries.iter().map(|(_, v)| v.clone()).collect())),
                            "length" | "len" | "size" => return Ok(Value::Int(entries.len() as i64)),
                            _ => {}
                        }
                        let val = entries.iter()
                            .find(|(k, _)| k == field)
                            .map(|(_, v)| v.clone())
                            .unwrap_or(Value::Unit);
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
                            "length" | "len" => Ok(Value::Int(s.len() as i64)),
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
                            Ok(entries.iter()
                                .find(|(k, _)| k == &key_str)
                                .map(|(_, v)| v.clone())
                                .unwrap_or(Value::Unit))
                        } else {
                            Ok(Value::Unit)
                        }
                    }
                    (Value::Map(entries), "keys") => {
                        Ok(Value::List(entries.iter().map(|(k, _)| Value::String(k.clone())).collect()))
                    }
                    (Value::Map(entries), "values") => {
                        Ok(Value::List(entries.iter().map(|(_, v)| v.clone()).collect()))
                    }
                    (Value::Map(entries), "has") => {
                        if let Some(key) = arg_vals.first() {
                            let key_str = format!("{}", key);
                            Ok(Value::Bool(entries.iter().any(|(k, _)| k == &key_str)))
                        } else {
                            Ok(Value::Bool(false))
                        }
                    }
                    (Value::String(s), "len" | "length") => Ok(Value::Int(s.len() as i64)),
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
        &self,
        cell_name: &str,
        slot_name: &str,
        method: &str,
        args: &[Value],
    ) -> Result<Value, ExecError> {
        // Look up the storage backend: cell-prefixed FIRST (avoids cross-cell collisions)
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

        match method {
            "get" => {
                let key = args.first()
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "get() requires a key argument".to_string()
                    )))?;
                let key_str = format!("{}", key);
                match backend.get(&key_str) {
                    Some(stored) => Ok(stored_to_value(stored)),
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
                backend.set(&key_str, value_to_stored(val));
                Ok(Value::Unit)
            }
            "delete" | "remove" => {
                let key = args.first()
                    .ok_or_else(|| ExecError::Runtime(RuntimeError::TypeError(
                        "delete() requires a key argument".to_string()
                    )))?;
                let key_str = format!("{}", key);
                let removed = backend.delete(&key_str);
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
                Ok(Value::String(backend.backend_name().to_string()))
            }
            _ => {
                Err(ExecError::Runtime(RuntimeError::TypeError(
                    format!("unknown method '{}' on memory slot '{}'", method, slot_name),
                )))
            }
        }
    }

    /// Evaluate a constraint expression, returning true/false
    fn eval_constraint(
        &mut self,
        constraint: &Constraint,
        env: &mut HashMap<String, Value>,
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
    fn interpolate_string(&mut self, s: &str, env: &mut HashMap<String, Value>, cell_name: &str, signal_name: &str) -> String {
        let bytes = s.as_bytes();
        let mut result = String::with_capacity(s.len());
        let mut pos = 0;
        while pos < bytes.len() {
            if bytes[pos] == b'{' {
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
            result.push(bytes[pos] as char);
            pos += 1;
        }
        result
    }

    /// Parse and evaluate an expression string from interpolation
    fn eval_interpolation_expr(&mut self, expr_str: &str, env: &mut HashMap<String, Value>, cell_name: &str, signal_name: &str) -> Option<Value> {
        // Fast path: simple variable name
        if expr_str.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return env.get(expr_str).cloned();
        }
        // Fast path: var.field
        if expr_str.contains('.') && !expr_str.contains('(') && !expr_str.contains(' ') {
            let parts: Vec<&str> = expr_str.splitn(2, '.').collect();
            if parts.len() == 2 {
                if let Some(val) = env.get(parts[0]) {
                    if let Value::Map(ref entries) = val {
                        if let Some((_, field_val)) = entries.iter().find(|(k, _)| k == parts[1]) {
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
                }
            }
        }
        // Full expression: parse and eval
        let wrapped = format!("cell _T {{ on _e() {{ return {} }} }}", expr_str);
        let mut lexer = crate::lexer::Lexer::new(&wrapped);
        let tokens = lexer.tokenize().ok()?;
        let mut parser = crate::parser::Parser::new(tokens);
        let program = parser.parse_program().ok()?;
        let cell = program.cells.first()?;
        let section = cell.node.sections.first()?;
        if let crate::ast::Section::OnSignal(ref on) = section.node {
            if let Some(stmt) = on.body.first() {
                if let crate::ast::Statement::Return { ref value } = stmt.node {
                    match self.eval_expr(&value.node, env, cell_name, signal_name) {
                        Ok(val) => return Some(val),
                        Err(_) => return None,
                    }
                }
            }
        }
        None
    }

    fn eval_literal(&self, lit: &Literal) -> Value {
        match lit {
            Literal::Int(n) => Value::Int(*n),
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
                    DurationUnit::Years => d.value * 365.0 * 86_400_000.0,
                };
                Value::Int(ms as i64)
            }
            Literal::Percentage(p) => Value::Float(*p),
            Literal::Unit => Value::Unit,
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
        Ok(Value::Map(vec![
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
        Ok(Value::Map(vec![
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

        Ok(Value::Map(vec![
            ("status".to_string(), Value::String("subscribed".to_string())),
            ("url".to_string(), Value::String(url.to_string())),
        ]))
    }

    pub(crate) fn apply_lambda(&mut self, lambda: &Value, arg: Value, cell_name: &str) -> Result<Value, ExecError> {
        match lambda {
            Value::Lambda { param, body, env: closed_env } => {
                let mut env = closed_env.clone();
                env.insert(param.clone(), arg);
                self.eval_expr(&body.node, &mut env, cell_name, "")
            }
            Value::LambdaBlock { param, stmts, result, env: closed_env } => {
                let mut env = closed_env.clone();
                env.insert(param.clone(), arg);
                for stmt in stmts {
                    self.exec_stmt(&stmt.node, &mut env, cell_name, "")?;
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
            (Value::Float(x), Value::Float(y)) => x == y,
            (Value::Int(x), Value::Float(y)) => (*x as f64) == *y,
            (Value::Float(x), Value::Int(y)) => *x == (*y as f64),
            (Value::String(x), Value::String(y)) => x == y,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::Unit, Value::Unit) => true,
            _ => false,
        }
    }

    fn eval_binop(&self, l: &Value, op: BinOp, r: &Value) -> Result<Value, RuntimeError> {
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
                BinOp::Add => a.checked_add(*b).map(Value::Int)
                    .ok_or_else(|| RuntimeError::TypeError(format!("integer overflow: {} + {} (use BigInt for large numbers)", a, b))),
                BinOp::Sub => a.checked_sub(*b).map(Value::Int)
                    .ok_or_else(|| RuntimeError::TypeError(format!("integer overflow: {} - {} (use BigInt for large numbers)", a, b))),
                BinOp::Mul => a.checked_mul(*b).map(Value::Int)
                    .ok_or_else(|| RuntimeError::TypeError(format!("integer overflow: {} * {} (use BigInt for large numbers)", a, b))),
                BinOp::Div => {
                    if *b == 0 {
                        Err(RuntimeError::TypeError("division by zero".to_string()))
                    } else {
                        Ok(Value::Int(a / b))
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
                "cannot apply {:?} to {:?} and {:?}",
                op, l, r
            ))),
        }
    }

    fn eval_cmpop(&self, l: &Value, op: CmpOp, r: &Value) -> Result<Value, RuntimeError> {
        // BigInt comparisons
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
                "cannot compare {:?} and {:?}",
                l, r
            ))),
        }
    }

    /// Native function boundary. The names here correspond to `native "name"`
    /// in `cell builtin` definitions. This is the thin kernel — everything
    /// above is Soma. Delegates to sub-modules in builtins/.
    pub fn call_builtin(&self, name: &str, args: &[Value], cell_name: &str) -> Option<Result<Value, RuntimeError>> {
        builtins::call_builtin(self, name, args, cell_name)
    }

    /// Execute a state transition
    pub(crate) fn do_transition(&self, id: &str, target: &str) -> Result<Value, RuntimeError> {
        // Find the state machine and its storage
        let (sm, status_slot) = self.find_state_machine()
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

        let _transition = match transition {
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

        // TODO: evaluate guard expression (would need env context)
        // For now, guards are checked if they're simple comparisons

        // Perform transition
        status_slot.set(id, crate::runtime::storage::StoredValue::String(target.to_string()));

        Ok(Value::Map(vec![
            ("id".to_string(), Value::String(id.to_string())),
            ("from".to_string(), Value::String(current)),
            ("to".to_string(), Value::String(target.to_string())),
        ]))
    }

    pub(crate) fn do_get_status(&self, id: &str) -> Result<Value, RuntimeError> {
        let (sm, status_slot) = self.find_state_machine()
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
        let Some((sm, status_slot)) = self.find_state_machine() else {
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
        for ((_cell_name, sm_name), sm) in &self.state_machines {
            // State machines use a DEDICATED slot: __sm_{name}
            // This prevents confusion with user data slots
            let slot_name = format!("__sm_{}", sm_name);
            if let Some(backend) = self.storage.get(&slot_name) {
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
fn value_to_stored(val: &Value) -> StoredValue {
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

fn stored_to_value(stored: StoredValue) -> Value {
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
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program().unwrap();
        let mut interp = Interpreter::new(&program);
        interp.call_signal(cell, signal, args)
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
    fn test_factorial_int_overflow() {
        // Int overflows cleanly with a helpful error
        let source = r#"
            cell Fact {
                on compute(n: Int) {
                    if n <= 1 { return 1 }
                    return n * compute(n - 1)
                }
            }
        "#;
        let result = run(source, "Fact", "compute", vec![Value::Int(30)]);
        assert!(result.is_err()); // overflow
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
}
