use std::collections::HashMap;
use std::sync::Arc;

use super::bytecode::*;
use crate::interpreter::Value;
use crate::runtime::storage::StorageBackend;

/// A call frame on the VM stack
struct CallFrame {
    chunk_idx: usize,
    ip: usize,
    base: usize, // base index into locals
}

/// The Soma Virtual Machine — executes compiled bytecode.
pub struct VM {
    /// All compiled chunks
    chunks: Vec<Chunk>,
    /// Chunk lookup: (cell, signal) → chunk index
    chunk_map: HashMap<(String, String), usize>,
    /// Value stack
    stack: Vec<Value>,
    /// Local variable slots (flat array, frames index into it)
    locals: Vec<Value>,
    /// Call frame stack
    frames: Vec<CallFrame>,
    /// Storage backends
    storage: HashMap<String, Arc<dyn StorageBackend>>,
    /// Builtin function dispatcher
    emitted_signals: Vec<(String, Vec<Value>)>,
    /// Iterator state stack (for for..in loops)
    iter_stack: Vec<IterState>,
    /// Max call depth
    max_frames: usize,
}

struct IterState {
    items: Vec<Value>,
    index: usize,
}

impl VM {
    pub fn new(chunks: Vec<Chunk>) -> Self {
        let mut chunk_map = HashMap::new();
        for (i, chunk) in chunks.iter().enumerate() {
            chunk_map.insert((chunk.cell_name.clone(), chunk.signal_name.clone()), i);
        }
        Self {
            chunks,
            chunk_map,
            stack: Vec::with_capacity(256),
            locals: Vec::with_capacity(64),
            frames: Vec::with_capacity(32),
            storage: HashMap::new(),
            emitted_signals: Vec::new(),
            iter_stack: Vec::new(),
            max_frames: 10_000,
        }
    }

    pub fn set_storage(&mut self, name: &str, slots: &HashMap<String, Arc<dyn StorageBackend>>) {
        for (slot_name, backend) in slots {
            self.storage.insert(slot_name.clone(), backend.clone());
        }
    }

    pub fn take_emitted_signals(&mut self) -> Vec<(String, Vec<Value>)> {
        std::mem::take(&mut self.emitted_signals)
    }

    /// Execute a signal handler, return the result
    pub fn call_signal(
        &mut self,
        cell_name: &str,
        signal_name: &str,
        args: Vec<Value>,
    ) -> Result<Value, String> {
        let key = (cell_name.to_string(), signal_name.to_string());
        let chunk_idx = *self.chunk_map.get(&key)
            .ok_or_else(|| format!("no compiled handler for {}.{}", cell_name, signal_name))?;

        if self.frames.len() >= self.max_frames {
            return Err("stack overflow".to_string());
        }

        let base = self.locals.len();

        // Reserve local slots
        let num_locals = self.chunks[chunk_idx].locals.len();
        self.locals.resize(base + num_locals, Value::Unit);

        // Bind arguments to first N locals
        for (i, arg) in args.into_iter().enumerate() {
            if base + i < self.locals.len() {
                self.locals[base + i] = arg;
            }
        }

        self.frames.push(CallFrame {
            chunk_idx,
            ip: 0,
            base,
        });

        let result = self.run();

        // Pop frame and clean up locals
        self.frames.pop();
        self.locals.truncate(base);

        result
    }

    /// Main execution loop
    fn run(&mut self) -> Result<Value, String> {
        loop {
            let frame = self.frames.last().unwrap();
            let chunk_idx = frame.chunk_idx;
            let ip = frame.ip;
            let base = frame.base;

            if ip >= self.chunks[chunk_idx].code.len() {
                return Ok(Value::Unit);
            }

            let op = self.chunks[chunk_idx].code[ip];
            // Advance IP
            self.frames.last_mut().unwrap().ip += 1;

            match op {
                x if x == Op::Const as u8 => {
                    let idx = self.read_u16() as usize;
                    let val = match &self.chunks[chunk_idx].constants[idx] {
                        Constant::Int(n) => Value::Int(*n),
                        Constant::Float(n) => Value::Float(*n),
                        Constant::String(s) => Value::String(s.clone()),
                        Constant::Name(s) => Value::String(s.clone()),
                    };
                    self.stack.push(val);
                }
                x if x == Op::Unit as u8 => self.stack.push(Value::Unit),
                x if x == Op::True as u8 => self.stack.push(Value::Bool(true)),
                x if x == Op::False as u8 => self.stack.push(Value::Bool(false)),

                x if x == Op::GetLocal as u8 => {
                    let slot = self.read_u16() as usize;
                    let val = self.locals[base + slot].clone();
                    self.stack.push(val);
                }
                x if x == Op::SetLocal as u8 => {
                    let slot = self.read_u16() as usize;
                    let val = self.stack.pop().unwrap_or(Value::Unit);
                    let idx = base + slot;
                    if idx >= self.locals.len() {
                        self.locals.resize(idx + 1, Value::Unit);
                    }
                    self.locals[idx] = val;
                }

                // Arithmetic
                x if x == Op::Add as u8 => self.binary_op(|a, b| match (a, b) {
                    (Value::Int(a), Value::Int(b)) => a.checked_add(b).map(Value::Int).unwrap_or(Value::Float(a as f64 + b as f64)),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a + b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(a as f64 + b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a + b as f64),
                    (Value::String(a), Value::String(b)) => { let mut s = a; s.push_str(&b); Value::String(s) }
                    (a, b) => Value::String(format!("{}{}", a, b)),
                }),
                x if x == Op::Sub as u8 => self.binary_op(|a, b| match (a, b) {
                    (Value::Int(a), Value::Int(b)) => Value::Int(a - b),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a - b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(a as f64 - b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a - b as f64),
                    _ => Value::Unit,
                }),
                x if x == Op::Mul as u8 => self.binary_op(|a, b| match (a, b) {
                    (Value::Int(a), Value::Int(b)) => a.checked_mul(b).map(Value::Int).unwrap_or(Value::Float(a as f64 * b as f64)),
                    (Value::Float(a), Value::Float(b)) => Value::Float(a * b),
                    (Value::Int(a), Value::Float(b)) => Value::Float(a as f64 * b),
                    (Value::Float(a), Value::Int(b)) => Value::Float(a * b as f64),
                    _ => Value::Unit,
                }),
                x if x == Op::Div as u8 => self.binary_op(|a, b| match (a, b) {
                    (Value::Int(a), Value::Int(b)) => if b != 0 { Value::Int(a / b) } else { Value::Unit },
                    (Value::Float(a), Value::Float(b)) => Value::Float(a / b),
                    _ => Value::Unit,
                }),
                x if x == Op::Mod as u8 => self.binary_op(|a, b| match (a, b) {
                    (Value::Int(a), Value::Int(b)) => if b != 0 { Value::Int(a % b) } else { Value::Unit },
                    _ => Value::Unit,
                }),

                // Comparison
                x if x == Op::Eq as u8 => self.cmp_op(|a, b| a == b),
                x if x == Op::Ne as u8 => self.cmp_op(|a, b| a != b),
                x if x == Op::Lt as u8 => self.cmp_op(|a, b| a < b),
                x if x == Op::Gt as u8 => self.cmp_op(|a, b| a > b),
                x if x == Op::Le as u8 => self.cmp_op(|a, b| a <= b),
                x if x == Op::Ge as u8 => self.cmp_op(|a, b| a >= b),

                x if x == Op::Not as u8 => {
                    let val = self.stack.pop().unwrap_or(Value::Unit);
                    self.stack.push(Value::Bool(!val.is_truthy()));
                }

                x if x == Op::Concat as u8 => {
                    let b = self.stack.pop().unwrap_or(Value::Unit);
                    let a = self.stack.pop().unwrap_or(Value::Unit);
                    self.stack.push(Value::String(format!("{}{}", a, b)));
                }

                // Control flow
                x if x == Op::Jump as u8 => {
                    let target = self.read_u16() as usize;
                    self.frames.last_mut().unwrap().ip = target;
                }
                x if x == Op::JumpIfFalse as u8 => {
                    let target = self.read_u16() as usize;
                    let val = self.stack.pop().unwrap_or(Value::Unit);
                    if !val.is_truthy() {
                        self.frames.last_mut().unwrap().ip = target;
                    }
                }
                x if x == Op::Return as u8 => {
                    let val = self.stack.pop().unwrap_or(Value::Unit);
                    return Ok(val);
                }
                x if x == Op::Pop as u8 => {
                    self.stack.pop();
                }

                // Function calls
                x if x == Op::CallBuiltin as u8 => {
                    let name_idx = self.read_u16() as usize;
                    let argc = self.read_u8() as usize;
                    let name = self.chunks[chunk_idx].constants[name_idx].as_str().to_string();
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.stack.pop().unwrap_or(Value::Unit));
                    }
                    args.reverse();
                    let result = self.call_builtin_fn(&name, args);
                    self.stack.push(result);
                }

                x if x == Op::CallSignal as u8 => {
                    let cell_idx = self.read_u16() as usize;
                    let sig_idx = self.read_u16() as usize;
                    let argc = self.read_u8() as usize;
                    let cell = self.chunks[chunk_idx].constants[cell_idx].as_str().to_string();
                    let sig = self.chunks[chunk_idx].constants[sig_idx].as_str().to_string();
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.stack.pop().unwrap_or(Value::Unit));
                    }
                    args.reverse();
                    let result = self.call_signal(&cell, &sig, args)?;
                    self.stack.push(result);
                }

                x if x == Op::CallStorage as u8 => {
                    let slot_idx = self.read_u16() as usize;
                    let argc = self.read_u8() as usize;
                    let method_idx = self.read_u16() as usize;
                    let slot = self.chunks[chunk_idx].constants[slot_idx].as_str().to_string();
                    let method = self.chunks[chunk_idx].constants[method_idx].as_str().to_string();
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.stack.pop().unwrap_or(Value::Unit));
                    }
                    args.reverse();
                    let result = self.call_storage_fn(&slot, &method, args);
                    self.stack.push(result);
                }

                x if x == Op::GetField as u8 => {
                    let name_idx = self.read_u16() as usize;
                    let field = self.chunks[chunk_idx].constants[name_idx].as_str().to_string();
                    let obj = self.stack.pop().unwrap_or(Value::Unit);
                    let val = match obj {
                        Value::Map(ref entries) => {
                            entries.iter()
                                .find(|(k, _)| *k == field)
                                .map(|(_, v)| v.clone())
                                .unwrap_or(Value::Unit)
                        }
                        _ => Value::Unit,
                    };
                    self.stack.push(val);
                }

                x if x == Op::CallMethod as u8 => {
                    let name_idx = self.read_u16() as usize;
                    let argc = self.read_u8() as usize;
                    let method = self.chunks[chunk_idx].constants[name_idx].as_str().to_string();
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.stack.pop().unwrap_or(Value::Unit));
                    }
                    args.reverse();
                    let obj = self.stack.pop().unwrap_or(Value::Unit);
                    // Dispatch method on obj
                    let result = self.dispatch_method(obj, &method, args);
                    self.stack.push(result);
                }

                // Iteration
                x if x == Op::IterInit as u8 => {
                    let val = self.stack.pop().unwrap_or(Value::Unit);
                    let items = match val {
                        Value::List(items) => items,
                        Value::String(s) if s.contains('\n') => {
                            s.split('\n').filter(|l| !l.is_empty())
                                .map(|l| Value::String(l.to_string())).collect()
                        }
                        other => vec![other],
                    };
                    self.iter_stack.push(IterState { items, index: 0 });
                }
                x if x == Op::IterNext as u8 => {
                    let end_offset = self.read_u16() as usize;
                    let local_slot = self.read_u16() as usize;
                    if let Some(iter) = self.iter_stack.last_mut() {
                        if iter.index < iter.items.len() {
                            let val = iter.items[iter.index].clone();
                            iter.index += 1;
                            self.locals[base + local_slot] = val;
                        } else {
                            self.iter_stack.pop();
                            self.frames.last_mut().unwrap().ip = end_offset;
                        }
                    } else {
                        self.frames.last_mut().unwrap().ip = end_offset;
                    }
                }

                _ => return Err(format!("unknown opcode: {}", op)),
            }
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────

    fn read_u16(&mut self) -> u16 {
        let frame = self.frames.last_mut().unwrap();
        let chunk = &self.chunks[frame.chunk_idx];
        let val = ((chunk.code[frame.ip] as u16) << 8) | (chunk.code[frame.ip + 1] as u16);
        frame.ip += 2;
        val
    }

    fn read_u8(&mut self) -> u8 {
        let frame = self.frames.last_mut().unwrap();
        let val = self.chunks[frame.chunk_idx].code[frame.ip];
        frame.ip += 1;
        val
    }

    fn binary_op<F: Fn(Value, Value) -> Value>(&mut self, f: F) {
        let b = self.stack.pop().unwrap_or(Value::Unit);
        let a = self.stack.pop().unwrap_or(Value::Unit);
        self.stack.push(f(a, b));
    }

    fn cmp_op<F: Fn(i64, i64) -> bool>(&mut self, f: F) {
        let b = self.stack.pop().unwrap_or(Value::Unit);
        let a = self.stack.pop().unwrap_or(Value::Unit);
        let result = match (&a, &b) {
            (Value::Int(a), Value::Int(b)) => f(*a, *b),
            (Value::Unit, Value::Unit) => f(0, 0),
            (Value::Unit, _) => f(0, 1), // Unit != anything
            (_, Value::Unit) => f(1, 0),
            (Value::String(a), Value::String(b)) => f(a.cmp(b) as i64, 0),
            (Value::Bool(a), Value::Bool(b)) => f(*a as i64, *b as i64),
            _ => false,
        };
        self.stack.push(Value::Bool(result));
    }

    fn call_builtin_fn(&mut self, name: &str, args: Vec<Value>) -> Value {
        // Delegate to the interpreter's builtin registry
        // For speed, handle the most common ones inline
        match name {
            "concat" => {
                if args.len() >= 2 {
                    match (&args[0], &args[1]) {
                        (Value::String(a), Value::String(b)) => {
                            let mut s = String::with_capacity(a.len() + b.len());
                            s.push_str(a);
                            s.push_str(b);
                            Value::String(s)
                        }
                        _ => Value::String(format!("{}{}", args[0], args[1])),
                    }
                } else {
                    Value::Unit
                }
            }
            "to_string" => {
                args.first().map(|a| Value::String(format!("{}", a))).unwrap_or(Value::Unit)
            }
            "print" => {
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { print!(" "); }
                    print!("{}", arg);
                }
                println!();
                Value::Unit
            }
            "render" => {
                if let Some(Value::String(template)) = args.first() {
                    let mut vars: HashMap<String, String> = HashMap::new();
                    let mut i = 1;
                    while i + 1 < args.len() {
                        vars.insert(format!("{}", args[i]), format!("{}", args[i + 1]));
                        i += 2;
                    }
                    let bytes = template.as_bytes();
                    let mut result = String::with_capacity(template.len());
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
                    Value::String(result)
                } else {
                    Value::Unit
                }
            }
            "html" => {
                let body = args.first().map(|a| format!("{}", a)).unwrap_or_default();
                Value::Map(vec![
                    ("_status".to_string(), Value::Int(200)),
                    ("_body".to_string(), Value::String(body)),
                    ("_content_type".to_string(), Value::String("text/html".to_string())),
                ])
            }
            "response" => {
                let status = args.first().cloned().unwrap_or(Value::Int(200));
                let body = args.get(1).cloned().unwrap_or(Value::Unit);
                Value::Map(vec![
                    ("_status".to_string(), status),
                    ("_body".to_string(), body),
                ])
            }
            "redirect" => {
                let url = args.first().map(|a| format!("{}", a)).unwrap_or("/".to_string());
                Value::Map(vec![
                    ("_status".to_string(), Value::Int(302)),
                    ("_body".to_string(), Value::String(String::new())),
                    ("Location".to_string(), Value::String(url)),
                ])
            }
            "map" => {
                let mut entries = Vec::new();
                let mut i = 0;
                while i + 1 < args.len() {
                    entries.push((format!("{}", args[i]), args[i + 1].clone()));
                    i += 2;
                }
                Value::Map(entries)
            }
            "list" => {
                if let Some(Value::List(existing)) = args.first() {
                    let mut result = existing.clone();
                    result.extend(args[1..].to_vec());
                    Value::List(result)
                } else {
                    Value::List(args)
                }
            }
            "from_json" => {
                match args.first() {
                    Some(Value::String(s)) => {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
                            json_to_value(&v)
                        } else {
                            Value::String(s.clone())
                        }
                    }
                    Some(v @ Value::Map(_)) | Some(v @ Value::List(_)) => v.clone(),
                    _ => Value::Unit,
                }
            }
            "to_json" => {
                args.first().map(|a| Value::String(format!("{}", a))).unwrap_or(Value::Unit)
            }
            "split" => {
                if args.len() >= 2 {
                    if let (Value::String(s), Value::String(d)) = (&args[0], &args[1]) {
                        Value::List(s.split(d.as_str()).map(|p| Value::String(p.to_string())).collect())
                    } else { Value::Unit }
                } else { Value::Unit }
            }
            _ => Value::Unit,
        }
    }

    fn call_storage_fn(&self, slot: &str, method: &str, args: Vec<Value>) -> Value {
        let backend = match self.storage.get(slot) {
            Some(b) => b,
            None => return Value::Unit,
        };
        match method {
            "get" => {
                let key = args.first().map(|a| format!("{}", a)).unwrap_or_default();
                backend.get(&key).map(|v| stored_to_value(v)).unwrap_or(Value::Unit)
            }
            "set" | "put" => {
                if args.len() >= 2 {
                    let key = format!("{}", args[0]);
                    backend.set(&key, value_to_stored(&args[1]));
                }
                Value::Unit
            }
            "delete" => {
                let key = args.first().map(|a| format!("{}", a)).unwrap_or_default();
                Value::Bool(backend.delete(&key))
            }
            "append" => {
                if let Some(val) = args.first() {
                    backend.append(value_to_stored(val));
                }
                Value::Unit
            }
            "keys" => Value::List(backend.keys().into_iter().map(Value::String).collect()),
            "values" => Value::List(backend.values().into_iter().map(stored_to_value).collect()),
            "len" | "size" => Value::Int(backend.len() as i64),
            "has" => {
                let key = args.first().map(|a| format!("{}", a)).unwrap_or_default();
                Value::Bool(backend.has(&key))
            }
            "backend" => Value::String(backend.backend_name().to_string()),
            _ => Value::Unit,
        }
    }

    fn dispatch_method(&self, obj: Value, method: &str, args: Vec<Value>) -> Value {
        match (&obj, method) {
            (Value::List(items), "len" | "length") => Value::Int(items.len() as i64),
            (Value::List(items), "first") => items.first().cloned().unwrap_or(Value::Unit),
            (Value::List(items), "last") => items.last().cloned().unwrap_or(Value::Unit),
            (Value::Map(entries), "keys") => {
                Value::List(entries.iter().map(|(k, _)| Value::String(k.clone())).collect())
            }
            (Value::Map(entries), "get") => {
                let key = args.first().map(|a| format!("{}", a)).unwrap_or_default();
                entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone()).unwrap_or(Value::Unit)
            }
            (Value::String(s), "len" | "length") => Value::Int(s.len() as i64),
            _ => Value::Unit,
        }
    }
}

// ── Value helpers ────────────────────────────────────────────────────

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Unit => false,
            Value::String(s) => !s.is_empty(),
            _ => true,
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Unit, Value::Unit) => true,
            _ => false,
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
            (Value::String(a), Value::String(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

use crate::runtime::storage::StoredValue;

fn value_to_stored(val: &Value) -> StoredValue {
    match val {
        Value::Int(n) => StoredValue::Int(*n),
        Value::Float(n) => StoredValue::Float(*n),
        Value::String(s) => StoredValue::String(s.clone()),
        Value::Bool(b) => StoredValue::Bool(*b),
        Value::Unit => StoredValue::Null,
        Value::List(items) => StoredValue::List(items.iter().map(value_to_stored).collect()),
        Value::Map(entries) => StoredValue::Map(entries.iter().map(|(k, v)| (k.clone(), value_to_stored(v))).collect()),
        Value::Big(n) => StoredValue::String(n.to_string()),
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
        StoredValue::Map(map) => Value::Map(map.into_iter().map(|(k, v)| (k, stored_to_value(v))).collect()),
    }
}

fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Unit,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() { Value::Int(i) }
            else { Value::Float(n.as_f64().unwrap_or(0.0)) }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(a) => Value::List(a.iter().map(json_to_value).collect()),
        serde_json::Value::Object(o) => Value::Map(o.iter().map(|(k, v)| (k.clone(), json_to_value(v))).collect()),
    }
}
