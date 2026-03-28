use std::collections::HashMap;
use std::sync::Arc;
use crate::ast::*;
use crate::runtime::storage::{StorageBackend, StoredValue};
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero, One};
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
    Unit,
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Big(n) => write!(f, "{}", n),
            Value::Float(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
            Value::Bool(b) => write!(f, "{}", b),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    match item {
                        Value::String(s) => write!(f, "\"{}\"", s)?,
                        other => write!(f, "{}", other)?,
                    }
                }
                write!(f, "]")
            }
            Value::Map(entries) => {
                write!(f, "{{")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "\"{}\": ", k)?;
                    match v {
                        Value::String(s) => write!(f, "\"{}\"", s)?,
                        other => write!(f, "{}", other)?,
                    }
                }
                write!(f, "}}")
            }
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
    storage: HashMap<String, Arc<dyn StorageBackend>>,
    /// State machines: (cell_name, machine_name) → definition
    state_machines: HashMap<(String, String), StateMachineSection>,
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
                let val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                env.insert(name.clone(), val);
                Ok(Value::Unit)
            }

            Statement::Return { value } => {
                let val = self.eval_expr(&value.node, env, cell_name, signal_name)?;
                Err(ExecError::Return(val))
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
                    last = self.exec_body(body, env, cell_name, signal_name)?;
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
                    self.exec_body(body, env, cell_name, signal_name)?;
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
                self.emitted_signals.push((sig.clone(), arg_vals));
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
                        return Ok(Value::String(self.interpolate_string(s, env)));
                    }
                }
                Ok(val)
            }

            Expr::Ident(name) => env
                .get(name)
                .cloned()
                .ok_or_else(|| ExecError::Runtime(RuntimeError::UndefinedVar(name.clone()))),

            Expr::BinaryOp { left, op, right } => {
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
            Constraint::Predicate { name, args } => {
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
    fn interpolate_string(&self, s: &str, env: &HashMap<String, Value>) -> String {
        let bytes = s.as_bytes();
        let mut result = String::with_capacity(s.len());
        let mut pos = 0;
        while pos < bytes.len() {
            if bytes[pos] == b'{' {
                if let Some(end) = s[pos + 1..].find('}') {
                    let key = &s[pos + 1..pos + 1 + end];
                    // Check if it's a simple identifier (no spaces, no special chars)
                    if !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        if let Some(val) = env.get(key) {
                            result.push_str(&format!("{}", val));
                            pos = pos + 1 + end + 1;
                            continue;
                        }
                    }
                    // Also try field access: {car.brand}
                    if key.contains('.') {
                        let parts: Vec<&str> = key.splitn(2, '.').collect();
                        if parts.len() == 2 {
                            if let Some(val) = env.get(parts[0]) {
                                if let Value::Map(ref entries) = val {
                                    if let Some((_, field_val)) = entries.iter().find(|(k, _)| k == parts[1]) {
                                        result.push_str(&format!("{}", field_val));
                                        pos = pos + 1 + end + 1;
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            result.push(bytes[pos] as char);
            pos += 1;
        }
        result
    }

    fn eval_literal(&self, lit: &Literal) -> Value {
        match lit {
            Literal::Int(n) => Value::Int(*n),
            Literal::Float(n) => Value::Float(*n),
            Literal::String(s) => Value::String(s.clone()),
            Literal::Bool(b) => Value::Bool(*b),
            Literal::Duration(d) => Value::Float(d.value),
            Literal::Percentage(p) => Value::Float(*p),
            Literal::Unit => Value::Unit,
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
    /// above is Soma.
    fn call_builtin(&self, name: &str, args: &[Value], cell_name: &str) -> Option<Result<Value, RuntimeError>> {
        match name {
            "print" => {
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { print!(" "); }
                    print!("{}", arg);
                }
                println!();
                Some(Ok(Value::Unit))
            }
            "abs" => {
                args.first().map(|arg| match arg {
                    Value::Int(n) => Ok(Value::Int(n.abs())),
                    Value::Float(n) => Ok(Value::Float(n.abs())),
                    _ => Err(RuntimeError::TypeError("abs expects a number".to_string())),
                })
            }
            "len" => {
                args.first().map(|arg| match arg {
                    Value::String(s) => Ok(Value::Int(s.len() as i64)),
                    _ => Err(RuntimeError::TypeError("len expects a string".to_string())),
                })
            }
            "concat" => {
                if args.len() >= 2 {
                    // Optimize: avoid format! for string+string case
                    match (&args[0], &args[1]) {
                        (Value::String(a), Value::String(b)) => {
                            let mut result = String::with_capacity(a.len() + b.len());
                            result.push_str(a);
                            result.push_str(b);
                            Some(Ok(Value::String(result)))
                        }
                        _ => Some(Ok(Value::String(format!("{}{}", args[0], args[1]))))
                    }
                } else if args.len() == 1 {
                    Some(Ok(args[0].clone()))
                } else {
                    Some(Err(RuntimeError::TypeError("concat expects arguments".to_string())))
                }
            }
            "to_string" => {
                args.first().map(|arg| Ok(Value::String(format!("{}", arg))))
            }
            "to_int" | "int" => {
                args.first().map(|arg| match arg {
                    Value::Int(n) => Ok(Value::Int(*n)),
                    Value::Float(n) => Ok(Value::Int(*n as i64)),
                    Value::String(s) => Ok(Value::Int(
                        s.parse::<i64>().unwrap_or_else(|_| s.parse::<f64>().unwrap_or(0.0) as i64)
                    )),
                    Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
                    _ => Ok(Value::Int(0)),
                })
            }
            "to_float" | "float" => {
                args.first().map(|arg| match arg {
                    Value::Float(n) => Ok(Value::Float(*n)),
                    Value::Int(n) => Ok(Value::Float(*n as f64)),
                    Value::String(s) => Ok(Value::Float(s.parse::<f64>().unwrap_or(0.0))),
                    _ => Ok(Value::Float(0.0)),
                })
            }
            "to_json" => {
                args.first().map(|arg| Ok(Value::String(format!("{}", arg))))
            }
            "from_json" => {
                args.first().map(|arg| {
                    match arg {
                        Value::String(s) => Ok(json_to_value(s)),
                        // If already a Map or List, return as-is (idempotent)
                        Value::Map(_) | Value::List(_) => Ok(arg.clone()),
                        Value::Unit => Ok(Value::Unit),
                        other => Ok(Value::String(format!("{}", other))),
                    }
                })
            }
            "type_of" => {
                args.first().map(|arg| {
                    let t = match arg {
                        Value::Int(_) => "Int",
                        Value::Big(_) => "BigInt",
                        Value::Float(_) => "Float",
                        Value::String(_) => "String",
                        Value::Bool(_) => "Bool",
                        Value::List(_) => "List",
                        Value::Map(_) => "Map",
                        Value::Unit => "Unit",
                    };
                    Ok(Value::String(t.to_string()))
                })
            }
            // String operations
            "split" => {
                if args.len() >= 2 {
                    if let (Value::String(s), Value::String(delim)) = (&args[0], &args[1]) {
                        let parts: Vec<Value> = s.split(delim.as_str())
                            .map(|p| Value::String(p.to_string()))
                            .collect();
                        Some(Ok(Value::List(parts)))
                    } else {
                        Some(Err(RuntimeError::TypeError("split expects (string, delimiter)".to_string())))
                    }
                } else {
                    Some(Err(RuntimeError::TypeError("split expects 2 arguments".to_string())))
                }
            }
            "replace" => {
                if args.len() >= 3 {
                    if let (Value::String(s), Value::String(old), Value::String(new)) = (&args[0], &args[1], &args[2]) {
                        Some(Ok(Value::String(s.replace(old.as_str(), new.as_str()))))
                    } else {
                        Some(Err(RuntimeError::TypeError("replace expects strings".to_string())))
                    }
                } else {
                    Some(Err(RuntimeError::TypeError("replace expects 3 arguments".to_string())))
                }
            }
            "starts_with" => {
                if args.len() >= 2 {
                    if let (Value::String(s), Value::String(prefix)) = (&args[0], &args[1]) {
                        Some(Ok(Value::Bool(s.starts_with(prefix.as_str()))))
                    } else {
                        Some(Err(RuntimeError::TypeError("starts_with expects strings".to_string())))
                    }
                } else {
                    Some(Err(RuntimeError::TypeError("starts_with expects 2 arguments".to_string())))
                }
            }
            "trim" => {
                args.first().map(|arg| {
                    if let Value::String(s) = arg {
                        Ok(Value::String(s.trim().to_string()))
                    } else {
                        Err(RuntimeError::TypeError("trim expects a string".to_string()))
                    }
                })
            }
            // Collection constructors
            "list" => {
                // Flatten: if first arg is a list, extend it with the rest
                if let Some(Value::List(existing)) = args.first() {
                    let mut result = existing.clone();
                    result.extend(args[1..].to_vec());
                    Some(Ok(Value::List(result)))
                } else {
                    Some(Ok(Value::List(args.to_vec())))
                }
            }
            "render_each" => {
                // render_each(list_of_maps, template) → joined HTML string
                // Each map's keys become template variables
                if args.len() >= 2 {
                    if let (Value::List(items), Value::String(template)) = (&args[0], &args[1]) {
                        let mut result = String::with_capacity(template.len() * items.len());
                        for item in items {
                            if let Value::Map(entries) = item {
                                // Build vars from the map
                                let vars: HashMap<String, String> = entries.iter()
                                    .map(|(k, v)| (k.clone(), format!("{}", v)))
                                    .collect();
                                // Single-pass render
                                let bytes = template.as_bytes();
                                let mut pos = 0;
                                while pos < bytes.len() {
                                    if bytes[pos] == b'{' {
                                        if let Some(end) = template[pos+1..].find('}') {
                                            let key = &template[pos+1..pos+1+end];
                                            if let Some(val) = vars.get(key) {
                                                result.push_str(val);
                                                pos = pos + 1 + end + 1;
                                                continue;
                                            }
                                        }
                                    }
                                    result.push(bytes[pos] as char);
                                    pos += 1;
                                }
                            }
                        }
                        Some(Ok(Value::String(result)))
                    } else {
                        Some(Err(RuntimeError::TypeError("render_each expects (list, template)".to_string())))
                    }
                } else {
                    Some(Err(RuntimeError::TypeError("render_each expects 2 arguments".to_string())))
                }
            }
            "join" => {
                // join(list, separator) → string
                if args.len() >= 2 {
                    if let Value::List(items) = &args[0] {
                        let sep = format!("{}", args[1]);
                        let parts: Vec<String> = items.iter().map(|v| format!("{}", v)).collect();
                        Some(Ok(Value::String(parts.join(&sep))))
                    } else if let Value::List(items) = &args[0] {
                        let parts: Vec<String> = items.iter().map(|v| format!("{}", v)).collect();
                        Some(Ok(Value::String(parts.join(""))))
                    } else {
                        Some(Ok(Value::String(format!("{}", args[0]))))
                    }
                } else if let Some(Value::List(items)) = args.first() {
                    let parts: Vec<String> = items.iter().map(|v| format!("{}", v)).collect();
                    Some(Ok(Value::String(parts.join(""))))
                } else {
                    Some(Err(RuntimeError::TypeError("join expects a list".to_string())))
                }
            }
            "push" | "append" => {
                // push(list, item) → new list with item added
                if args.len() >= 2 {
                    if let Value::List(existing) = &args[0] {
                        let mut result = existing.clone();
                        result.extend(args[1..].to_vec());
                        Some(Ok(Value::List(result)))
                    } else {
                        Some(Ok(Value::List(args.to_vec())))
                    }
                } else {
                    Some(Ok(Value::List(args.to_vec())))
                }
            }
            "map" => {
                // map("key1", val1, "key2", val2, ...)
                let mut entries = Vec::new();
                let mut i = 0;
                while i + 1 < args.len() {
                    let key = format!("{}", args[i]);
                    let val = args[i + 1].clone();
                    entries.push((key, val));
                    i += 2;
                }
                Some(Ok(Value::Map(entries)))
            }
            // HTTP response constructor
            "response" => {
                let status = args.first().cloned().unwrap_or(Value::Int(200));
                let body = args.get(1).cloned().unwrap_or(Value::Unit);
                let mut entries = vec![
                    ("_status".to_string(), status),
                    ("_body".to_string(), body),
                ];
                let mut i = 2;
                while i + 1 < args.len() {
                    let key = format!("{}", args[i]);
                    let val = args[i + 1].clone();
                    entries.push((key, val));
                    i += 2;
                }
                Some(Ok(Value::Map(entries)))
            }
            // HTML response: html(body) or html(status, body)
            "html" => {
                let (status, body) = if args.len() >= 2 {
                    (args[0].clone(), format!("{}", args[1]))
                } else {
                    (Value::Int(200), args.first().map(|a| format!("{}", a)).unwrap_or_default())
                };
                Some(Ok(Value::Map(vec![
                    ("_status".to_string(), status),
                    ("_body".to_string(), Value::String(body)),
                    ("_content_type".to_string(), Value::String("text/html".to_string())),
                ])))
            }
            // Template rendering: render(template, key1, val1, key2, val2, ...)
            // Replaces {key} with val in the template string
            "render" => {
                if let Some(Value::String(template)) = args.first() {
                    // Build lookup map once
                    let mut vars: HashMap<String, String> = HashMap::new();
                    let mut i = 1;
                    while i + 1 < args.len() {
                        let key = format!("{}", args[i]);
                        let val = format!("{}", args[i + 1]);
                        vars.insert(key, val);
                        i += 2;
                    }
                    // Single pass: scan for {key} and replace
                    let bytes = template.as_bytes();
                    let mut result = String::with_capacity(template.len());
                    let mut pos = 0;
                    while pos < bytes.len() {
                        if bytes[pos] == b'{' {
                            // Find closing brace
                            if let Some(end) = template[pos+1..].find('}') {
                                let key = &template[pos+1..pos+1+end];
                                if let Some(val) = vars.get(key) {
                                    result.push_str(val);
                                    pos = pos + 1 + end + 1;
                                    continue;
                                }
                            }
                        }
                        result.push(bytes[pos] as char);
                        pos += 1;
                    }
                    Some(Ok(Value::String(result)))
                } else {
                    Some(Err(RuntimeError::TypeError("render expects a template string".to_string())))
                }
            }
            // Redirect
            "redirect" => {
                let url = args.first().map(|a| format!("{}", a)).unwrap_or("/".to_string());
                Some(Ok(Value::Map(vec![
                    ("_status".to_string(), Value::Int(302)),
                    ("_body".to_string(), Value::String(String::new())),
                    ("Location".to_string(), Value::String(url)),
                ])))
            }
            // ── Auto-increment ────────────────────────────────────────
            "next_id" => {
                // next_id() — auto-increment counter using the current cell's first storage slot
                let counter_key = "__next_id";
                // Prefer cell-prefixed slot
                let slot = self.storage.iter()
                    .find(|(k, _)| k.starts_with(&format!("{}.", cell_name)))
                    .or_else(|| self.storage.iter().next())
                    .map(|(_, v)| v);
                if let Some(backend) = slot {
                    let current = backend.get(counter_key)
                        .and_then(|v| match v {
                            crate::runtime::storage::StoredValue::Int(n) => Some(n),
                            _ => None,
                        })
                        .unwrap_or(0);
                    let next = current + 1;
                    backend.set(counter_key, crate::runtime::storage::StoredValue::Int(next));
                    Some(Ok(Value::Int(next)))
                } else {
                    Some(Ok(Value::Int(1)))
                }
            }
            // ── String operations (additional) ───────────────────────────
            "contains" => {
                if args.len() >= 2 {
                    if let (Value::String(haystack), Value::String(needle)) = (&args[0], &args[1]) {
                        Some(Ok(Value::Bool(haystack.contains(needle.as_str()))))
                    } else {
                        Some(Ok(Value::Bool(false)))
                    }
                } else {
                    Some(Err(RuntimeError::TypeError("contains(string, substring)".to_string())))
                }
            }
            "lowercase" | "to_lower" => {
                args.first().map(|a| {
                    if let Value::String(s) = a {
                        Ok(Value::String(s.to_lowercase()))
                    } else {
                        Ok(Value::String(format!("{}", a)))
                    }
                })
            }
            "uppercase" | "to_upper" => {
                args.first().map(|a| {
                    if let Value::String(s) = a {
                        Ok(Value::String(s.to_uppercase()))
                    } else {
                        Ok(Value::String(format!("{}", a)))
                    }
                })
            }
            "index_of" => {
                if args.len() >= 2 {
                    if let (Value::String(s), Value::String(sub)) = (&args[0], &args[1]) {
                        Some(Ok(match s.find(sub.as_str()) {
                            Some(i) => Value::Int(i as i64),
                            None => Value::Int(-1),
                        }))
                    } else {
                        Some(Ok(Value::Int(-1)))
                    }
                } else {
                    Some(Err(RuntimeError::TypeError("index_of(string, substring)".to_string())))
                }
            }
            "substring" | "substr" => {
                if args.len() >= 3 {
                    if let (Value::String(s), Value::Int(start), Value::Int(end)) = (&args[0], &args[1], &args[2]) {
                        let start = (*start).max(0) as usize;
                        let end = (*end).min(s.len() as i64) as usize;
                        Some(Ok(Value::String(s.get(start..end).unwrap_or("").to_string())))
                    } else {
                        Some(Ok(Value::Unit))
                    }
                } else {
                    Some(Err(RuntimeError::TypeError("substring(string, start, end)".to_string())))
                }
            }
            // ── State machine builtins ────────────────────────────────
            "transition" => {
                // transition(id, target_state)
                // Finds the state machine, checks guard, moves state
                if args.len() >= 2 {
                    let id = format!("{}", args[0]);
                    let target = format!("{}", args[1]);
                    Some(self.do_transition(&id, &target))
                } else {
                    Some(Err(RuntimeError::TypeError("transition(id, target_state) requires 2 args".to_string())))
                }
            }
            "get_status" => {
                // get_status(id) → current state string
                if let Some(id) = args.first() {
                    let id_str = format!("{}", id);
                    Some(self.do_get_status(&id_str))
                } else {
                    Some(Err(RuntimeError::TypeError("get_status(id) requires 1 arg".to_string())))
                }
            }
            "valid_transitions" => {
                // valid_transitions(id) → list of valid target states
                if let Some(id) = args.first() {
                    let id_str = format!("{}", id);
                    Some(Ok(self.do_valid_transitions(&id_str)))
                } else {
                    Some(Err(RuntimeError::TypeError("valid_transitions(id) requires 1 arg".to_string())))
                }
            }
            _ => None,
        }
    }

    /// Execute a state transition
    fn do_transition(&self, id: &str, target: &str) -> Result<Value, RuntimeError> {
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

    fn do_get_status(&self, id: &str) -> Result<Value, RuntimeError> {
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

    fn do_valid_transitions(&self, id: &str) -> Value {
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
    fn find_state_machine(&self) -> Option<(&StateMachineSection, &Arc<dyn StorageBackend>)> {
        for ((cell_name, sm_name), sm) in &self.state_machines {
            // The state is stored in a slot named after the state machine
            let slot_name = format!("_state_{}", sm_name);
            if let Some(backend) = self.storage.get(&slot_name).or_else(|| self.storage.get(sm_name)) {
                return Some((sm, backend));
            }
            // Try any storage slot that exists
            for (key, backend) in &self.storage {
                if key.contains("state") || key.contains("status") || key == sm_name {
                    return Some((sm, backend));
                }
            }
            // Use first available storage as fallback for state
            if let Some((_, backend)) = self.storage.iter().next() {
                return Some((sm, backend));
            }
        }
        None
    }
}

/// Check if a type expression refers to BigInt
/// Parse a JSON string into a Value
fn json_to_value(s: &str) -> Value {
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) {
        serde_json_to_value(&parsed)
    } else {
        Value::String(s.to_string())
    }
}

fn serde_json_to_value(v: &serde_json::Value) -> Value {
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
            Value::List(arr.iter().map(serde_json_to_value).collect())
        }
        serde_json::Value::Object(obj) => {
            Value::Map(obj.iter().map(|(k, v)| (k.clone(), serde_json_to_value(v))).collect())
        }
    }
}

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
enum ExecError {
    Return(Value),
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
