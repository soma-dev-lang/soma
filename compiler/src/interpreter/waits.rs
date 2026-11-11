//! 3.0: durable waits. `await approve(msg)`, `await signal(name, key)` and
//! `await sleep(ms)` in a `[task]` handler: when nobody can answer now, the
//! handler's locals and its position are stored in the cell's `_waits`
//! table — in the same atomic step as what it wrote — and the call returns
//! a 202 `waiting` response. `resume` / `deliver` (or `soma serve`'s
//! `/_approvals`, `/_signals` and its sleep timer) run the rest.
//!
//! Where a wait may stand, and how it is found again, is in
//! `checker/waits.rs`.

use super::*;
use crate::checker::waits as place;
use crate::runtime::storage::{StorageBackend, StoredValue};

/// A year: `await sleep` is a reminder, not a calendar.
const MAX_SLEEP_MS: i64 = 366 * 86_400_000;

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn wait_error(message: String) -> RuntimeError {
    RuntimeError::Domain { kind: "await".to_string(), message }
}

fn not_waiting(what: String) -> RuntimeError {
    RuntimeError::Domain { kind: "not_waiting".to_string(), message: format!("not_waiting: {} — it was answered already, cancelled, or never asked (`waiting()` lists what is pending)", what) }
}

/// A local that cannot be written down: a lambda (a response map's own
/// provenance mark is stored with it and is not one).
fn holds_lambda(v: &Value) -> bool {
    match v {
        Value::Lambda { .. } | Value::LambdaBlock { .. } => true,
        Value::List(xs) => xs.iter().any(holds_lambda),
        Value::Map(m) => {
            let response = is_http_response(v);
            m.iter().any(|(k, x)| !(response && k == "_response") && holds_lambda(x))
        }
        Value::Variant { fields: VariantValue::Tuple(xs), .. } => xs.iter().any(holds_lambda),
        Value::Variant { fields: VariantValue::Struct(m), .. } => m.values().any(holds_lambda),
        _ => false,
    }
}

fn text(rec: &indexmap::IndexMap<String, StoredValue>, key: &str) -> String {
    match rec.get(key) { Some(StoredValue::String(s)) => s.clone(), _ => String::new() }
}

fn int(rec: &indexmap::IndexMap<String, StoredValue>, key: &str) -> i64 {
    match rec.get(key) { Some(StoredValue::Int(n)) => *n, _ => 0 }
}

/// What a caller (and `waiting()`) is told about a stored wait.
fn public_fields(id: &str, rec: &indexmap::IndexMap<String, StoredValue>) -> Vec<(String, Value)> {
    let kind = text(rec, "kind");
    let mut out = vec![
        ("waiting".to_string(), Value::String(id.to_string())),
        ("kind".to_string(), Value::String(kind.clone())),
    ];
    match kind.as_str() {
        "approve" => out.push(("message".to_string(), Value::String(text(rec, "message")))),
        "signal" => {
            out.push(("name".to_string(), Value::String(text(rec, "name"))));
            out.push(("key".to_string(), Value::String(text(rec, "key"))));
        }
        _ => out.push(("until_ms".to_string(), Value::Int(SomaInt::from_i64(int(rec, "due_ms"))))),
    }
    out
}

impl Interpreter {
    /// The cell's table of stored waits (persistent under run/serve).
    pub(crate) fn waits_backend(&mut self, cell_name: &str) -> Arc<dyn StorageBackend> {
        let slot_key = format!("{}.__waits", cell_name);
        if !self.storage.contains_key(&slot_key) {
            let persistent = PERSIST_MACHINES.load(std::sync::atomic::Ordering::Relaxed)
                || self.storage.values().any(|b| b.backend_name() == "sqlite");
            let backend: Arc<dyn StorageBackend> = if persistent {
                Arc::new(crate::runtime::storage::SqliteBackend::new(cell_name, "_waits"))
            } else {
                Arc::new(crate::runtime::storage::MemoryBackend::new())
            };
            self.storage.insert(slot_key.clone(), backend);
        }
        self.storage.get(&slot_key).cloned().unwrap()
    }

    /// Every stored wait: (cell, id, record), oldest first.
    fn stored_waits(&mut self) -> Vec<(String, String, indexmap::IndexMap<String, StoredValue>)> {
        let mut cells: Vec<String> = self.cells.keys().cloned().collect();
        cells.sort();
        let mut out = Vec::new();
        for cell in cells {
            let backend = self.waits_backend(&cell);
            for id in backend.keys() {
                if let Some(StoredValue::Map(rec)) = backend.get(&id) { out.push((cell.clone(), id, rec)); }
            }
        }
        out.sort_by(|a, b| (int(&a.2, "created_ms"), &a.1).cmp(&(int(&b.2, "created_ms"), &b.1)));
        out
    }

    /// `let x = await what(args)` / `await what(args)` as a statement of a
    /// handler: the answer when there is one now, otherwise the handler
    /// stops here (`ExecError::Return` of the `waiting` response).
    pub(super) fn await_stmt(
        &mut self,
        what: &str,
        args: &[Spanned<Expr>],
        bind: Option<&str>,
        span: Span,
        env: &mut Env,
        cell_name: &str,
        signal_name: &str,
    ) -> Result<Value, ExecError> {
        let mut vals = Vec::with_capacity(args.len());
        for a in args { vals.push(self.eval_expr(&a.node, env, cell_name, signal_name)?); }
        let mut rec: indexmap::IndexMap<String, StoredValue> = indexmap::IndexMap::new();
        rec.insert("kind".to_string(), StoredValue::String(what.to_string()));
        match what {
            "approve" => {
                let [Value::String(message)] = vals.as_slice() else {
                    return Err(ExecError::Runtime(RuntimeError::TypeError("await approve(message: String)".to_string())));
                };
                // a scripted answer, an explicit policy, a person at the terminal
                let before = self.agent_trace.len();
                match self.call_builtin("approve", &vals, cell_name) {
                    Some(Err(RuntimeError::Domain { kind, .. })) if kind == "approval_required" => self.agent_trace.truncate(before),
                    Some(r) => return r.map_err(ExecError::Runtime),
                    None => return Err(ExecError::Runtime(RuntimeError::UndefinedFn("approve".to_string()))),
                }
                rec.insert("message".to_string(), StoredValue::String(message.clone()));
            }
            "sleep" => {
                let ms = match vals.as_slice() {
                    [Value::Int(ms)] => ms.to_i64().filter(|ms| (0..=MAX_SLEEP_MS).contains(ms)),
                    _ => return Err(ExecError::Runtime(RuntimeError::TypeError("await sleep(duration) takes one duration (`30s`, `2h`) or a count of milliseconds".to_string()))),
                };
                let Some(ms) = ms else {
                    return Err(ExecError::Runtime(RuntimeError::Domain { kind: "range".to_string(), message: "await sleep(): the duration is 0 ms to 366 days".to_string() }));
                };
                // a test does not wait; `soma run` has one process and one
                // caller: it sleeps here, between two steps
                if IN_TEST.load(std::sync::atomic::Ordering::Relaxed) || self.test_auto_mock { return Ok(Value::Unit); }
                if !IN_SERVE.load(std::sync::atomic::Ordering::Relaxed) {
                    self.outside_unit(|| std::thread::sleep(std::time::Duration::from_millis(ms as u64))).map_err(ExecError::Runtime)?;
                    return Ok(Value::Unit);
                }
                rec.insert("due_ms".to_string(), StoredValue::Int(now_ms().saturating_add(ms)));
            }
            _ => {
                let (name, key) = match vals.as_slice() {
                    [Value::String(name), key @ (Value::String(_) | Value::Int(_))] if !name.is_empty() => (name.clone(), key_text(key)),
                    _ => return Err(ExecError::Runtime(RuntimeError::TypeError("await signal(name: String, key) takes the signal's name and the key that tells this wait from the others (a String or an Int)".to_string()))),
                };
                rec.insert("name".to_string(), StoredValue::String(name));
                rec.insert("key".to_string(), StoredValue::String(key));
            }
        }

        // where to come back to: the n-th wait of this handler
        let handler = self.handler_cache.get(&(cell_name.to_string(), signal_name.to_string())).map(|e| (Arc::clone(&e.0), Arc::clone(&e.1)));
        let place = handler.as_ref().and_then(|(_, body)| place::waits(body).iter()
            .position(|p| p.last().and_then(|(b, i)| place::wait_of(&b[*i].node)).map_or(false, |w| w.3 == span)));
        let (Some((params, body)), Some(nth)) = (handler, place) else {
            return Err(ExecError::Runtime(wait_error(format!(
                "await {}(…) has nobody to answer it now and cannot stop here: a wait is stored as a whole statement of an `on …() [task]` handler's body, or of an `if` branch there", what))));
        };
        if let Some((name, _)) = env.iter().find(|(_, v)| holds_lambda(v)) {
            return Err(ExecError::Runtime(wait_error(format!(
                "await {}(…): the local `{}` holds a lambda, which cannot be stored with the wait — compute what it gives before the wait, or define it after", what, name))));
        }
        if let Some(name) = bind { record_shadow(env, name); }
        let env_id = &*env as *const Env as usize;
        let frames: Vec<StoredValue> = LEX_FRAMES.with(|f| f.borrow().iter().filter(|fr| fr.0 == env_id).map(|fr| {
            StoredValue::List(fr.2.iter().map(|(name, old)| {
                let mut m = indexmap::IndexMap::new();
                m.insert("name".to_string(), StoredValue::String(name.clone()));
                if let Some(v) = old { m.insert("value".to_string(), value_to_stored(v)); }
                StoredValue::Map(m)
            }).collect())
        }).collect());
        let mut locals: Vec<(&String, &Value)> = env.iter().collect();
        locals.sort_by(|a, b| a.0.cmp(b.0));

        // unguessable: under `soma serve` the id is what an answer names
        let mut raw = [0u8; 12];
        if getrandom::getrandom(&mut raw).is_err() {
            return Err(ExecError::Runtime(wait_error("await: the system gave no random bytes for the wait's identifier".to_string())));
        }
        let id = format!("w_{}", raw.iter().map(|b| format!("{:02x}", b)).collect::<String>());
        rec.insert("cell".to_string(), StoredValue::String(cell_name.to_string()));
        rec.insert("handler".to_string(), StoredValue::String(signal_name.to_string()));
        rec.insert("nth".to_string(), StoredValue::Int(nth as i64));
        rec.insert("shape".to_string(), StoredValue::String(place::fingerprint(&params, &body)));
        rec.insert("bind".to_string(), StoredValue::String(bind.unwrap_or("").to_string()));
        rec.insert("created_ms".to_string(), StoredValue::Int(now_ms()));
        rec.insert("env".to_string(), StoredValue::Map(locals.into_iter().map(|(k, v)| (k.clone(), value_to_stored(v))).collect()));
        rec.insert("frames".to_string(), StoredValue::List(frames));

        let backend = self.waits_backend(cell_name);
        backend.set(&id, StoredValue::Map(rec.clone()));
        if let Some(j) = self.journal.as_mut() {
            j.push(UndoOp::Restore { backend: backend.clone(), key: id.clone(), prev: None });
        }
        self.check_storage_write().map_err(ExecError::Runtime)?;
        if what == "approve" {
            self.agent_trace.push(builtins::llm::trace_approval(&text(&rec, "message"), "waiting"));
        }
        self.suspended_now = true;
        // an HTTP 202 whose body is what a client needs to answer; the same
        // fields sit beside it for the program (`r.waiting`)
        let mut fields = vec![("_status".to_string(), Value::Int(SomaInt::from_i64(202)))];
        fields.push(("_body".to_string(), map_from_pairs(public_fields(&id, &rec))));
        fields.extend(public_fields(&id, &rec));
        fields.push(("_response".to_string(), http_marker()));
        Err(ExecError::Return(map_from_pairs(fields)))
    }

    /// Run the rest of a stopped handler with `answer` as the value of its
    /// wait. A step like any other: it commits or rolls back whole, and a
    /// failed one leaves the wait where it was (the answer can come again).
    pub fn resume_wait(&mut self, id: &str, answer: Value) -> Result<Value, RuntimeError> {
        if self.journal.is_none() && self.task_unit.is_none() {
            self.run_task(|me| me.resume_body(id, answer))
        } else {
            self.atomically(|me| me.resume_body(id, answer))
        }
    }

    fn resume_body(&mut self, id: &str, answer: Value) -> Result<Value, RuntimeError> {
        let Some((cell, _, rec)) = self.stored_waits().into_iter().find(|(_, i, _)| i == id) else {
            return Err(not_waiting(format!("no wait `{}`", id)));
        };
        let (kind, handler) = (text(&rec, "kind"), text(&rec, "handler"));
        let answer = match kind.as_str() {
            "approve" => match answer {
                Value::Bool(_) => answer,
                other => return Err(RuntimeError::TypeError(format!("wait `{}` is an approval: its answer is true or false, got {}", id, short_value(&other)))),
            },
            "sleep" => Value::Unit,
            _ => answer,
        };
        let entry = self.handler_cache.get(&(cell.clone(), handler.clone())).map(|e| (Arc::clone(&e.0), Arc::clone(&e.1)));
        let stale = || RuntimeError::Domain { kind: "wait_stale".to_string(), message: format!(
            "wait_stale: wait `{}` was taken in handler '{}' of cell '{}', which has changed since (or is gone): its stored position and locals no longer describe this program — `cancel_wait(\"{}\")` drops it; what the handler wrote before the wait stays", id, handler, cell, id) };
        let Some((params, body)) = entry else { return Err(stale()) };
        if place::fingerprint(&params, &body) != text(&rec, "shape") { return Err(stale()); }
        let all = place::waits(&body);
        let Some(path) = all.get(int(&rec, "nth") as usize) else { return Err(stale()) };

        // the wait is consumed with the step it starts
        let backend = self.waits_backend(&cell);
        backend.delete(id);
        if let Some(j) = self.journal.as_mut() {
            j.push(UndoOp::Restore { backend: backend.clone(), key: id.to_string(), prev: Some(StoredValue::Map(rec.clone())) });
        }
        self.check_storage_write()?;
        if kind == "approve" {
            let said = if matches!(answer, Value::Bool(true)) { "approved" } else { "refused" };
            self.agent_trace.push(builtins::llm::trace_approval(&text(&rec, "message"), said));
        }

        let mut env: Env = FxHashMap::default();
        if let Some(StoredValue::Map(locals)) = rec.get("env") {
            for (k, v) in locals { env.insert(k.clone(), stored_to_value(v.clone())); }
        }
        let bind = text(&rec, "bind");
        if !bind.is_empty() { env.insert(bind, answer); }
        let mut frames: Vec<Vec<(String, Option<Value>)>> = match rec.get("frames") {
            Some(StoredValue::List(fs)) => fs.iter().map(|f| match f {
                StoredValue::List(saved) => saved.iter().filter_map(|s| match s {
                    StoredValue::Map(m) => Some((text(m, "name"), m.get("value").cloned().map(stored_to_value))),
                    _ => None,
                }).collect(),
                _ => Vec::new(),
            }).collect(),
            _ => Vec::new(),
        };

        self.current_depth += 1;
        if self.current_depth > self.max_depth {
            self.current_depth -= 1;
            return Err(RuntimeError::StackOverflow);
        }
        let prev_handler = self.current_handler.replace((cell.clone(), handler.clone(), Arc::clone(&params), Arc::clone(&body)));
        // the rest of the innermost block, then of each block around it
        let mut result: Result<Value, ExecError> = Ok(Value::Unit);
        for (level, (block, idx)) in path.iter().enumerate().rev() {
            let rest = &block[idx + 1..];
            if result.is_ok() && !rest.is_empty() {
                result = if level > 0 {
                    with_lexical_scope(rest, &mut env, |env| self.exec_body(rest, env, &cell, &handler))
                } else {
                    self.exec_body(rest, &mut env, &cell, &handler)
                };
            }
            // leaving an `if` branch: the `let`s it made before the wait end here
            if level > 0 && body_has_let(block) {
                for (name, old) in frames.pop().unwrap_or_default() {
                    match old {
                        Some(value) => { env.insert(name, value); }
                        None => { env.remove(&name); }
                    }
                }
            }
        }
        self.current_depth -= 1;
        self.current_handler = prev_handler;
        let out = match result {
            Ok(val) | Err(ExecError::Return(val)) => Ok(val),
            Err(ExecError::Break) => Err(RuntimeError::TypeError("break outside of loop".to_string())),
            Err(ExecError::Continue) => Err(RuntimeError::TypeError("continue outside of loop".to_string())),
            Err(ExecError::Runtime(e)) => Err(e),
        };
        // it stopped at its next wait: the `waiting` response, not the face's type
        if std::mem::take(&mut self.suspended_now) { return out; }
        out.and_then(|val| self.check_face_return(&cell, &handler, val))
    }

    /// Sleeps whose time has come (for `soma serve`'s timer).
    pub fn due_sleeps(&mut self) -> Vec<String> {
        let now = now_ms();
        self.stored_waits().into_iter()
            .filter(|(_, _, rec)| text(rec, "kind") == "sleep" && int(rec, "due_ms") <= now)
            .map(|(_, id, _)| id).collect()
    }

    /// `resume`, `deliver`, `waiting`, `cancel_wait`.
    pub(crate) fn wait_builtin(&mut self, name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
        // an answer does not come from the model: inside a tool call these
        // would let it approve its own request
        if matches!(name, "resume" | "cancel_wait") && (self.tool_depth > 0 || self.current_tool_caps.is_some()) {
            return Err(RuntimeError::TypeError(format!("capability denied: {}() inside a tool — the answer to a wait comes from a person or the program, not from the model that is waiting", name)));
        }
        match (name, args) {
            ("resume", [Value::String(id), answer]) => self.resume_wait(id, answer.clone()),
            ("resume", _) => Err(RuntimeError::TypeError("resume(wait_id: String, answer)".to_string())),
            ("deliver", [Value::String(signal), key @ (Value::String(_) | Value::Int(_)), value]) => {
                let key = key_text(key);
                let found = self.stored_waits().into_iter()
                    .find(|(_, _, rec)| text(rec, "kind") == "signal" && &text(rec, "name") == signal && text(rec, "key") == key);
                match found {
                    Some((_, id, _)) => self.resume_wait(&id, value.clone()),
                    None => Err(not_waiting(format!("nothing waits for signal `{}` with key `{}`", signal, key))),
                }
            }
            ("deliver", _) => Err(RuntimeError::TypeError("deliver(name: String, key, value)".to_string())),
            ("waiting", []) => Ok(Value::List(self.stored_waits().into_iter().map(|(cell, id, rec)| {
                let mut fields = public_fields(&id, &rec);
                fields.push(("cell".to_string(), Value::String(cell)));
                fields.push(("handler".to_string(), Value::String(text(&rec, "handler"))));
                fields.push(("since_ms".to_string(), Value::Int(SomaInt::from_i64(int(&rec, "created_ms")))));
                map_from_pairs(fields)
            }).collect())),
            ("waiting", _) => Err(RuntimeError::TypeError("waiting() takes no argument".to_string())),
            ("cancel_wait", [Value::String(id)]) => {
                let Some((cell, _, rec)) = self.stored_waits().into_iter().find(|(_, i, _)| i == id) else { return Ok(Value::Bool(false)) };
                let backend = self.waits_backend(&cell);
                backend.delete(id);
                if let Some(j) = self.journal.as_mut() {
                    j.push(UndoOp::Restore { backend: backend.clone(), key: id.clone(), prev: Some(StoredValue::Map(rec)) });
                }
                self.check_storage_write()?;
                Ok(Value::Bool(true))
            }
            _ => Err(RuntimeError::TypeError("cancel_wait(wait_id: String)".to_string())),
        }
    }
}
