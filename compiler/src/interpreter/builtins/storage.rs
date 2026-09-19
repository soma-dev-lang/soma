use super::super::{Value, RuntimeError, Interpreter};
use crate::interpreter::soma_int::SomaInt;

pub fn call_builtin(interp: &mut Interpreter, name: &str, args: &[Value], cell_name: &str) -> Option<Result<Value, RuntimeError>> {
    match name {
        "next_id" => {
            // the cell's own counter table (it lived in the user's first Map
            // slot under "__next_id": a user key of that name reset it, and a
            // List first slot answered 1 forever); journaled, so a refused
            // request burns no id. The legacy counter is carried over once.
            let counter_key = "next_id";
            let slot_key = format!("{}.__counters", cell_name);
            if !interp.storage.contains_key(&slot_key) {
                let persistent = crate::interpreter::PERSIST_MACHINES.load(std::sync::atomic::Ordering::Relaxed)
                    || interp.storage.values().any(|b| b.backend_name() == "sqlite");
                let backend: std::sync::Arc<dyn crate::runtime::storage::StorageBackend> = if persistent {
                    std::sync::Arc::new(crate::runtime::storage::SqliteBackend::new(cell_name, "_counters"))
                } else {
                    std::sync::Arc::new(crate::runtime::storage::MemoryBackend::new())
                };
                interp.storage.insert(slot_key.clone(), backend);
            }
            let backend = interp.storage.get(&slot_key).cloned().unwrap();
            let as_int = |v: Option<crate::runtime::storage::StoredValue>| v.and_then(|v| match v {
                crate::runtime::storage::StoredValue::Int(n) => Some(n),
                _ => None,
            });
            let current = as_int(backend.get(counter_key)).unwrap_or_else(|| {
                interp.next_id_backend(cell_name).and_then(|legacy| as_int(legacy.get("__next_id"))).unwrap_or(0)
            });
            let next = current + 1;
            if let Some(j) = interp.journal.as_mut() {
                j.push(crate::interpreter::UndoOp::Counter {
                    backend: backend.clone(),
                    key: counter_key.to_string(),
                    prev: backend.get(counter_key),
                });
            }
            backend.set(counter_key, crate::runtime::storage::StoredValue::Int(next));
            Some(Ok(Value::Int(SomaInt::from_i64(next))))
        }
        "transition" => {
            if args.len() >= 2 {
                // a Variant target must be a declared variant of the MACHINE's
                // type (from_json built `Zzz.Qqq` / an undeclared `Sent` and it
                // moved a typed machine)
                if let Value::Variant { type_name, variant, .. } = &args[1] {
                    let declared_ok = interp.type_variants.get(type_name).map_or(false, |vs| vs.iter().any(|v| v == variant));
                    let machine_type = interp.find_state_machine_for(cell_name).and_then(|(sm, _)| sm.state_type.clone());
                    let type_ok = machine_type.as_deref().map_or(true, |t| t == type_name);
                    if !declared_ok || !type_ok {
                        return Some(Err(RuntimeError::TypeError(format!(
                            "transition(): {}.{} is not a declared state of this machine{}", type_name, variant,
                            machine_type.map(|t| format!(" (its states are {} variants)", t)).unwrap_or_default()))));
                    }
                }
                let id = match instance_id(&args[0], "transition") { Ok(i) => i, Err(e) => return Some(Err(e)) };
                let target = format!("{}", args[1]);
                Some(interp.do_transition_for(cell_name, &id, &target))
            } else {
                Some(Err(RuntimeError::TypeError("transition(id, target_state) requires 2 args".to_string())))
            }
        }
        "get_status" => {
            if let Some(id) = args.first() {
                let id_str = match instance_id(id, "get_status") { Ok(i) => i, Err(e) => return Some(Err(e)) };
                Some(interp.do_get_status_for(cell_name, &id_str))
            } else {
                Some(Err(RuntimeError::TypeError("get_status(id) requires 1 arg".to_string())))
            }
        }
        "has_state" => {
            if let Some(id) = args.first() {
                let id_str = match instance_id(id, "has_state") { Ok(i) => i, Err(e) => return Some(Err(e)) };
                Some(Ok(Value::Bool(interp.do_has_state_for(cell_name, &id_str))))
            } else {
                Some(Err(RuntimeError::TypeError("has_state(id) requires 1 arg".to_string())))
            }
        }
        "valid_transitions" => {
            if let Some(id) = args.first() {
                let id_str = match instance_id(id, "valid_transitions") { Ok(i) => i, Err(e) => return Some(Err(e)) };
                Some(Ok(interp.do_valid_transitions_for(cell_name, &id_str)))
            } else {
                Some(Err(RuntimeError::TypeError("valid_transitions(id) requires 1 arg".to_string())))
            }
        }
        // ── Agent memory: remember(key, value), recall(key) ─────────
        "remember" => {
            if args.len() >= 2 {
                // a horde task running as an agent instance: its own keys
                let key = match &interp.horde_instance { Some(i) => format!("{}/{}", i, args[0]), None => format!("{}", args[0]) };
                let val = &args[1];

                // the cell's own agent memory (it used to be written into
                // whichever user slot a HashMap walk found first — past its
                // type and invariants — and was never rolled back)
                let backend = agent_memory(interp, cell_name);
                if let Some(j) = interp.journal.as_mut() {
                    j.push(crate::interpreter::UndoOp::Restore { backend: backend.clone(), key: key.clone(), prev: backend.get(&key) });
                }
                backend.set(&key, super::super::value_to_stored(val));
                Some(Ok(Value::Unit))
            } else {
                Some(Err(RuntimeError::TypeError("remember(key, value)".to_string())))
            }
        }
        "recall" => {
            if let Some(Value::String(key)) = args.first() {
                let scoped = interp.horde_instance.as_ref().map(|i| format!("{}/{}", i, key));
                let key = scoped.as_ref().unwrap_or(key);
                // opened here too: a recall in a new process (or another
                // serve thread) found no table and answered null
                if let Some(v) = agent_memory(interp, cell_name).get(key) {
                    return Some(Ok(super::super::auto_deserialize(super::super::stored_to_value(v))));
                }
                // legacy: values an older version wrote into a user slot
                for (name, backend) in interp.storage.iter() {
                    // never a user slot's own entry that happens to share the key
                    let hit = if name.ends_with("__agent_memory") { backend.get(key) } else { backend.get(&format!("__mem_{}", key)) };
                    if let Some(val) = hit {
                        return Some(Ok(super::super::auto_deserialize(super::super::stored_to_value(val))));
                    }
                }
                Some(Ok(Value::Unit))
            } else {
                Some(Err(RuntimeError::TypeError("recall(key: String)".to_string())))
            }
        }
        // ── publish(stream_name, data) — push to SSE on a dynamic stream ─
        // Soma's `emit X(...)` uses static signal names baked into the AST.
        // For per-thread / per-user real-time channels we need a runtime-
        // chosen stream name. publish() pushes a BusEvent with the supplied
        // string as the stream name; any SSE connection whose `sse-swap`
        // attribute matches will receive the data.
        "publish" => {
            if args.len() < 2 {
                return Some(Err(RuntimeError::TypeError(
                    "publish(stream_name, data) requires 2 args".to_string(),
                )));
            }
            let stream = format!("{}", args[0]);
            let data = args[1].clone();
            if interp.event_bus.is_some() {
                // held until the handler commits
                interp.send_bus(crate::interpreter::BusEvent { stream, data, internal: false });
            }
            return Some(Ok(Value::Unit));
        }
        // ── Agent delegation: delegate(cell, signal, args...) ──────
        "delegate" => {
            if args.len() >= 2 {
                let target_cell = format!("{}", args[0]);
                let signal_name = format!("{}", args[1]);
                let signal_args: Vec<Value> = args[2..].to_vec();
                Some(interp.call_signal(&target_cell, &signal_name, signal_args)
                    // the callee's error AS IS: wrapping it made every kind
                    // `type` (a not_found answered 400, invalid_transition
                    // was not catchable by kind) and nested recursion
                    // printed "delegate error: " 500 times
                    )
            } else {
                Some(Err(RuntimeError::TypeError("delegate(cell_name, signal_name, ...args) requires at least 2 args".to_string())))
            }
        }
        // ── Hordes: one handler over many inputs, a bounded pool ─────
        "horde" if (2..=3).contains(&args.len()) => {
            let (Value::String(target), Value::List(inputs)) = (&args[0], &args[1]) else {
                return Some(Err(RuntimeError::TypeError("horde(handler, inputs: List, opts?: Map) — handler is `Cell.handler` or \"Cell.handler\"".to_string())));
            };
            Some(interp.horde_start(cell_name, target, inputs.clone(), args.get(2)))
        }
        "vote" if args.len() == 3 => {
            let (Value::String(target), Value::Int(k)) = (&args[0], &args[2]) else {
                return Some(Err(RuntimeError::TypeError("vote(handler, input, k: Int) — handler is `Cell.handler` or \"Cell.handler\"".to_string())));
            };
            Some(interp.horde_vote(cell_name, target, args[1].clone(), k.to_i64().unwrap_or(0)))
        }
        "horde_status" | "horde_results" | "horde_cancel" if args.len() == 1 => {
            let Value::String(id) = &args[0] else {
                return Some(Err(RuntimeError::TypeError(format!("{}(id: String) — the id horde() returned", name))));
            };
            Some(match name {
                "horde_status" => interp.horde_status(cell_name, id),
                "horde_results" => interp.horde_results(cell_name, id),
                _ => interp.horde_cancel(cell_name, id),
            })
        }
        // ── Agent: set_budget(max_tokens) ──────────────────────────
        "set_budget" => {
            if let Some(Value::Int(si)) = args.first() {
                let n = si.to_i64().unwrap_or(0).max(0);
                if interp.tool_depth > 0 {
                    // reached from a model's tool call (a delegated agent's own
                    // set_budget): it may only LOWER what is left — it replaced
                    // the caller's 300 with 8000 and reset tokens_used, so the
                    // model bought itself 19 provider calls
                    // (0 is "no limit": it cannot lift the caller's)
                    if n == 0 { return Some(Ok(Value::Unit)); }
                    let used = interp.agent_tokens_used;
                    let left = if interp.agent_token_budget > 0 { (interp.agent_token_budget - used).max(0) } else { n };
                    interp.agent_token_budget = used + n.min(left);
                    return Some(Ok(Value::Unit));
                }
                interp.agent_token_budget = n;
                interp.agent_tokens_used = 0;
                Some(Ok(Value::Unit))
            } else {
                Some(Err(RuntimeError::TypeError("set_budget(max_tokens: Int)".to_string())))
            }
        }
        "tokens_used" => {
            Some(Ok(Value::Int(SomaInt::from_i64(interp.agent_tokens_used))))
        }
        "tokens_remaining" => {
            if interp.agent_token_budget > 0 {
                Some(Ok(Value::Int(SomaInt::from_i64((interp.agent_token_budget - interp.agent_tokens_used).max(0)))))
            } else {
                Some(Ok(Value::Int(SomaInt::from_i64(-1)))) // unlimited
            }
        }
        // ── Agent: trace() — get execution log ──────────────────────
        "trace" => {
            if crate::interpreter::IN_SERVE.load(std::sync::atomic::Ordering::Relaxed) {
                let mut all: Vec<Value> = SERVE_TRACE.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
                all.extend(interp.agent_trace.iter().cloned());
                return Some(Ok(Value::List(all)));
            }
            Some(Ok(Value::List(interp.agent_trace.clone())))
        }
        "clear_trace" => {
            interp.agent_trace.clear();
            if crate::interpreter::IN_SERVE.load(std::sync::atomic::Ordering::Relaxed) {
                SERVE_TRACE.lock().unwrap_or_else(|e| e.into_inner()).clear();
            }
            Some(Ok(Value::Unit))
        }
        // ── Agent: context() — get/clear conversation history ───────
        "clear_context" => {
            interp.agent_conversation.clear();
            interp.agent_conversations.remove(cell_name);
            Some(Ok(Value::Unit))
        }
        // ── Agent: approve(action) — human-in-the-loop ─────────────
        "approve" => {
            let Some(Value::String(raw_action)) = args.first() else {
                return Some(Err(RuntimeError::TypeError("approve(action: String)".to_string())));
            };
            // the person must see what the program wrote: a model-written
            // `\r\x1b[2K` erased the real prompt ("Refund 5000") and drew
            // a fake one ("Refund 5") — control and bidi characters are shown
            // as escapes, never interpreted by the terminal
            let action_owned: String = raw_action.chars().map(|c| {
                if c.is_control() || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}') {
                    c.escape_unicode().to_string()
                } else { c.to_string() }
            }).collect();
            let action = &action_owned;
            // 1. scripted by `mock approve …` in a test cell
            if let Some(answer) = interp.approve_queue.pop_front() {
                interp.agent_trace.push(super::llm::trace_approval(action, if answer { "approved" } else { "refused" }));
                return Some(Ok(Value::Bool(answer)));
            }
            // 2. an explicit policy for unattended runs
            match std::env::var("SOMA_APPROVE").ok().as_deref() {
                Some("always") => {
                    eprintln!("[agent] approval requested: {} — approved by SOMA_APPROVE=always", action);
                    interp.agent_trace.push(super::llm::trace_approval(action, "auto_approved"));
                    return Some(Ok(Value::Bool(true)));
                }
                Some("never") => {
                    eprintln!("[agent] approval requested: {} — refused by SOMA_APPROVE=never", action);
                    interp.agent_trace.push(super::llm::trace_approval(action, "refused"));
                    return Some(Ok(Value::Bool(false)));
                }
                _ => {}
            }
            // 3. a human at the terminal (soma run, interactive)
            let in_serve = crate::interpreter::IN_SERVE.load(std::sync::atomic::Ordering::Relaxed);
            let tty = std::io::IsTerminal::is_terminal(&std::io::stdin())
                && std::io::IsTerminal::is_terminal(&std::io::stderr());
            if !in_serve && !interp.test_auto_mock && tty {
                eprint!("[agent] approve? {} [y/N] ", action);
                let mut line = String::new();
                let _ = std::io::stdin().read_line(&mut line);
                let yes = matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes");
                interp.agent_trace.push(super::llm::trace_approval(action, if yes { "approved" } else { "refused" }));
                return Some(Ok(Value::Bool(yes)));
            }
            // 4. nobody can answer: fail closed. A gate that silently
            // approves is worse than no gate.
            let how = if interp.test_auto_mock {
                "script it in the test: `mock approve true` / `mock approve false` before the call"
            } else if in_serve {
                "under soma serve the decision must arrive as data (a handler parameter or a route), or set SOMA_APPROVE=always|never for an explicit unattended policy"
            } else {
                "run interactively at a terminal, or set SOMA_APPROVE=always|never"
            };
            interp.agent_trace.push(super::llm::trace_approval(action, "unanswered"));
            Some(Err(RuntimeError::Domain {
                kind: "approval_required".to_string(),
                message: format!("approval_required: approve(\"{}\") has no one to answer it — {}", action, how),
            }))
        }
        // ── AI Agent: think() with tool-calling loop ──────────────
        "think" => {
            if let Some(Value::String(prompt)) = args.first() {
                if let Some(Value::Map(m)) = args.last() {
                    // a typo'd option (`max_token`) was ignored: 2048 tokens were sent
                    if let Some(k) = m.keys().find(|k| !matches!(k.as_str(), "max_tokens" | "timeout" | "timeout_ms" | "max_rounds" | "tools_allowed" | "requires")) {
                        return Some(Err(RuntimeError::TypeError(format!("think(): unknown option '{}' — the options are max_tokens, timeout (ms), max_rounds, tools_allowed, requires", k))));
                    }
                }
                let (system, max_tokens, timeout_ms) = extract_think_opts(args);
                interp.think_rounds = if cell_has_tools(interp, cell_name) { think_rounds(args) } else { 1 };
                if let Some(Value::Map(m)) = args.last() {
                    if let Some(v) = m.get("max_tokens") {
                        if !matches!(v, Value::Int(n) if n.to_i64().map_or(false, |x| x > 0)) {
                            return Some(Err(RuntimeError::TypeError(format!("think(): max_tokens must be a positive Int, got {} (a non-positive value sent the provider default of 2048)", v))));
                        }
                    }
                }
                let allowed = match tools_allowed(args) { Ok(a) => a, Err(e) => return Some(Err(e)) };
                Some(with_tools_allowed(interp, allowed, |interp| with_cell_conversation(interp, cell_name, |interp| agent_think(interp, cell_name, prompt, system.as_deref(), false, max_tokens, timeout_ms))))
            } else {
                Some(Err(RuntimeError::TypeError("think(prompt: String) requires a string argument".to_string())))
            }
        }
        "think_json" => {
            if let Some(Value::String(prompt)) = args.first() {
                if let Some(Value::Map(m)) = args.last() {
                    // a typo'd option (`max_token`) was ignored: 2048 tokens were sent
                    if let Some(k) = m.keys().find(|k| !matches!(k.as_str(), "max_tokens" | "timeout" | "timeout_ms" | "max_rounds" | "tools_allowed" | "requires")) {
                        return Some(Err(RuntimeError::TypeError(format!("think(): unknown option '{}' — the options are max_tokens, timeout (ms), max_rounds, tools_allowed, requires", k))));
                    }
                }
                let (system, max_tokens, timeout_ms) = extract_think_opts(args);
                interp.think_rounds = if cell_has_tools(interp, cell_name) { think_rounds(args) } else { 1 };
                if let Some(Value::Map(m)) = args.last() {
                    if let Some(v) = m.get("max_tokens") {
                        if !matches!(v, Value::Int(n) if n.to_i64().map_or(false, |x| x > 0)) {
                            return Some(Err(RuntimeError::TypeError(format!("think(): max_tokens must be a positive Int, got {} (a non-positive value sent the provider default of 2048)", v))));
                        }
                    }
                }
                let allowed = match tools_allowed(args) { Ok(a) => a, Err(e) => return Some(Err(e)) };
                Some(with_tools_allowed(interp, allowed, |interp| with_cell_conversation(interp, cell_name, |interp| agent_think(interp, cell_name, prompt, system.as_deref(), true, max_tokens, timeout_ms))))
            } else {
                Some(Err(RuntimeError::TypeError("think_json(prompt: String) requires a string argument".to_string())))
            }
        }
        _ => None,
    }
}

/// Extract system message and options from think()/think_json() args.
///
/// Accepted call shapes:
///   think("prompt")
///   think("prompt", "system")
///   think("prompt", map("max_tokens", 500, "timeout", 10000))
///   think("prompt", "system", map("max_tokens", 500))
/// `think(p, map("max_rounds", 1))`: at most N provider rounds (tool calls
/// included) — 10 by default; the cost bound multiplies by it.
/// An agent without `tool`s gets ONE provider round: a model that answers
/// with tool calls anyway was asked again up to 10 times (a cost bound
/// proven at ×1 spent ×6).
fn cell_has_tools(interp: &Interpreter, cell_name: &str) -> bool {
    interp.cells.get(cell_name).map_or(false, |c| c.sections.iter().any(|s| matches!(&s.node,
        crate::ast::Section::Face(f) if f.declarations.iter().any(|d| matches!(d.node, crate::ast::FaceDecl::Tool(_))))))
}

pub(crate) fn think_rounds(args: &[Value]) -> usize {
    match args.last() {
        Some(Value::Map(m)) => match m.get("max_rounds") {
            Some(Value::Int(n)) => n.to_i64().unwrap_or(10).clamp(1, 10) as usize,
            _ => 10,
        },
        _ => 10,
    }
}

fn extract_think_opts(args: &[Value]) -> (Option<String>, Option<u64>, Option<u64>) {
    let mut system: Option<String> = None;
    let mut max_tokens: Option<u64> = None;
    let mut timeout_ms: Option<u64> = None;

    // Check if last arg is a Map (options)
    let opts_map = args.last().and_then(|v| {
        if let Value::Map(m) = v { Some(m) } else { None }
    });

    if let Some(m) = opts_map {
        if let Some(Value::Int(n)) = m.get("max_tokens") {
            let v = n.to_i64().unwrap_or(0);
            if v > 0 { max_tokens = Some(v as u64); }
        }
        if let Some(Value::Int(n)) = m.get("timeout").or_else(|| m.get("timeout_ms")) {
            let v = n.to_i64().unwrap_or(0);
            if v > 0 { timeout_ms = Some(v as u64); }
        }
        // System message is arg[1] only if it's a String (not the Map)
        if args.len() >= 3 {
            if let Some(Value::String(s)) = args.get(1) {
                system = Some(s.clone());
            }
        }
    } else {
        // No options map — old-style call
        if let Some(Value::String(s)) = args.get(1) {
            system = Some(s.clone());
        }
    }

    (system, max_tokens, timeout_ms)
}

/// Core agent think loop with:
/// - Tool calling (auto-dispatch to cell handlers)
/// - Multi-turn context (conversation persists across think() calls)
/// - Token budget enforcement (hard cap on LLM spend)
/// - Retry with exponential backoff (rate limits, transient errors)
/// - Structured tracing (every LLM call and tool dispatch logged)
fn agent_think(
    interp: &mut Interpreter,
    cell_name: &str,
    prompt: &str,
    system: Option<&str>,
    json_mode: bool,
    max_tokens: Option<u64>,
    timeout_ms: Option<u64>,
) -> Result<Value, RuntimeError> {
    use super::llm;

    // Budget check
    if interp.agent_token_budget > 0 && interp.agent_tokens_used >= interp.agent_token_budget {
        return Err(RuntimeError::TypeError(format!(
            "token budget exhausted: used {}/{}", interp.agent_tokens_used, interp.agent_token_budget
        )));
    }
    // the prompt's size is known before it is sent (~4 characters per
    // token): one that alone overruns what is left is refused here — a
    // 100 KB document spent 25 328 tokens under set_budget(3000)
    if interp.agent_token_budget > 0 {
        let prompt_est = ((prompt.chars().count() + system.map_or(0, |s| s.chars().count())) as i64 + 3) / 4;
        if interp.agent_tokens_used + prompt_est > interp.agent_token_budget {
            return Err(RuntimeError::TypeError(format!(
                "token budget exhausted: the prompt alone is ~{} tokens, {} of {} are left — shorten it (slice / summarize in parts) or raise set_budget",
                prompt_est, interp.agent_token_budget - interp.agent_tokens_used, interp.agent_token_budget
            )));
        }
    }

    // Resolve config: cell [model: x] → soma.toml [models.x] → [agent] → env vars
    let cell_model = interp.cells.get(cell_name).and_then(|c| c.agent_model.clone());
    let cell_skill = interp.cells.get(cell_name).and_then(|c| c.agent_skill.clone());
    let cfg = if let Some(ref model_name) = cell_model {
        interp.agent_models.get(model_name)
    } else {
        interp.agent_config.as_ref()
    };

    // Mock mode: soma.toml mock or SOMA_LLM_MOCK env
    let cfg_mock = cfg.map(|c| c.mock.clone()).unwrap_or_default();
    let mut mock_val = std::env::var("SOMA_LLM_MOCK").ok()
        .or_else(|| if cfg_mock.is_empty() { None } else { Some(cfg_mock) });

    // `soma test` with no key anywhere: mock instead of hitting the network.
    if mock_val.is_none() && interp.test_auto_mock && interp.mock_queue.is_empty() {
        let has_key = ["SOMA_LLM_KEY", "ANTHROPIC_API_KEY", "OPENAI_API_KEY"].iter().any(|k| std::env::var(k).is_ok_and(|v| !v.is_empty()))
            || cfg.is_some_and(|c| !resolve_env_vars(&c.key).is_empty());
        if !has_key {
            if !interp.auto_mock_noted {
                eprintln!("note: no LLM key configured — think() is mocked (echo: the reply is the prompt). \
                           Script replies with `mock think \"…\"`, or set [agent] mock in soma.toml.");
                interp.auto_mock_noted = true;
            }
            mock_val = Some("echo".to_string());
        }
    }

    // Scripted replies (`mock think …`) come first, whatever the mode.
    let scripted = interp.mock_queue.pop_front();
    // a scripted failure happens AFTER the step boundary, as a provider
    // error would (the test rolled back what production had committed)
    if let Some(Err(msg)) = scripted.clone() {
        interp.outside_unit(|| ());
        interp.agent_trace.push(super::llm::trace_think(0, prompt, 0, 0, "error"));
        return Err(RuntimeError::TypeError(format!("think() failed: {} (scripted by `mock think error`)", msg)));
    }
    if scripted.is_some() && mock_val.is_none() {
        mock_val = Some("echo".to_string());
    }

    if let Some(mock) = mock_val {
        // SOMA_LLM_MOCK_LATENCY_MS: a mocked model that takes time, to test
        // concurrency offline (inside a `[task]` it waits outside the lock)
        // …and ALWAYS ends a `[task]` step, latency or not: the step
        // semantics depended on the mock's latency (soma test rolled the
        // whole handler back where production kept the first step)
        let ms = std::env::var("SOMA_LLM_MOCK_LATENCY_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        // the rate limits apply to a mock too (a horde's pacing is testable offline)
        let (rpm, tpm) = cfg.map(|c| (c.rpm, c.tpm)).unwrap_or((0, 0));
        let want = ((prompt.chars().count() as u64 + 3) / 4) + max_tokens.unwrap_or(1000);
        // a horde's budget: reserve the bound BEFORE the (mocked) call —
        // outside the lock in a [task], where it may wait for calls in flight
        let bound = crate::interpreter::horde::request_bound(prompt.len() + system.map_or(0, |s| s.len()), 2, max_tokens.unwrap_or(2048));
        let (budget, can_wait) = (interp.horde_budget.clone(), interp.task_unit.is_some());
        let reservation = interp.outside_unit(|| -> Result<_, RuntimeError> {
            let r = crate::interpreter::horde::Reservation::take(budget, bound, can_wait)?;
            super::llm::limiter::acquire(rpm, tpm, want);
            if ms > 0 { std::thread::sleep(std::time::Duration::from_millis(ms.min(600_000))) }
            Ok(r)
        })?;
        let response = match (&scripted, mock.as_str()) {
            // a scripted reply longer than max_tokens (~4 characters per
            // token) is what a real provider refuses (kind llm): the test
            // exercises that path instead of under-counting tokens
            // a `fixed:` reply is scripted text too: `soma serve` cut it
            // short while `soma test` raised, so the two disagreed
            (Some(Ok(text)), _) if max_tokens.map_or(false, |m| (text.chars().count() as u64 + 3) / 4 > m) => {
                return Err(RuntimeError::Domain { kind: "llm".to_string(), message: format!(
                    "llm: the scripted reply is ~{} tokens for max_tokens {} — a provider reply over the cap raises (raise max_tokens, or script a shorter reply)",
                    (text.chars().count() + 3) / 4, max_tokens.unwrap_or(0)) });
            }
            (Some(Ok(text)), _) => text.clone(),
            // a real provider stops at max_tokens: so does the mock (~4
            // characters per token) — a scripted reply is kept as written
            (_, "echo") => cap_reply(prompt, max_tokens),
            // `rules:mocks.json` — [{"match": "risk", "reply": "{\"risk\": 3}"},
            // {"cell": "Judge", "reply": "yes"}, …]: the first rule whose
            // `match` is in the prompt (and `cell` is the calling agent)
            // answers; none: an echo
            (_, s) if s.starts_with("rules:") => {
                let rules = mock_rules(&s[6..]).map_err(|e| RuntimeError::TypeError(format!("think() mock rules: {}", e)))?;
                let hit = rules.iter().find(|r| {
                    r.get("match").and_then(|m| m.as_str()).map_or(true, |m| prompt.contains(m))
                        && r.get("cell").and_then(|c| c.as_str()).map_or(true, |c| c == cell_name)
                });
                match hit.and_then(|r| r.get("reply")) {
                    Some(serde_json::Value::String(t)) => {
                        let tokens = (t.chars().count() as u64 + 3) / 4;
                        if let Some(m) = max_tokens.filter(|&m| tokens > m) {
                            return Err(RuntimeError::Domain { kind: "llm".to_string(), message: format!(
                                "llm: the mock rule's reply is ~{} tokens for max_tokens {} — a provider reply over the cap raises", tokens, m) });
                        }
                        t.clone()
                    }
                    // a JSON reply written as JSON
                    Some(other) => other.to_string(),
                    None => cap_reply(prompt, max_tokens),
                }
            }
            (_, s) if s.starts_with("fixed:") => {
                let text = &s[6..];
                let tokens = (text.chars().count() as u64 + 3) / 4;
                if let Some(m) = max_tokens.filter(|&m| tokens > m) {
                    return Err(RuntimeError::Domain { kind: "llm".to_string(), message: format!(
                        "llm: the fixed mock reply is ~{} tokens for max_tokens {} — a provider reply over the cap raises (raise max_tokens, or use a shorter fixed: reply)",
                        tokens, m) });
                }
                text.to_string()
            }
            (_, other) => {
                // a configuration mistake, not a domain error: a handler's
                // `try { think(..) }` must not be able to swallow it
                eprintln!(
                    "error: unknown LLM mock mode '{}' (SOMA_LLM_MOCK or [agent] mock) — expected `echo` \
                     (reply = the prompt), `fixed:<text>` or `rules:<file.json>`",
                    other
                );
                std::process::exit(2);
            }
        };
        // a mocked call still costs: ~4 characters per token, prompt and
        // reply, so `set_budget` exhaustion (kind `budget`) is testable offline
        // the REPLY part is capped at max_tokens like a provider's (a long
        // scripted reply counted 41 against a proven bound of 10)
        let reply_tokens = (response.chars().count() as i64 + 3) / 4;
        let reply_tokens = match max_tokens { Some(m) if m > 0 => reply_tokens.min(m as i64), _ => reply_tokens };
        let est = (prompt.chars().count() as i64 + 3) / 4 + reply_tokens;
        let est = est.max(1);
        if let Some(r) = reservation { r.settle(est); }
        interp.agent_tokens_used += est;
        interp.agent_trace.push(super::llm::trace_think_with(0, prompt, system.unwrap_or(""), est, interp.agent_tokens_used, "stop"));
        if interp.agent_conversation.is_empty() {
            interp.agent_conversation.push(serde_json::json!({"role": "system", "content": "mock"}));
        }
        interp.agent_conversation.push(serde_json::json!({"role": "user", "content": prompt}));
        interp.agent_conversation.push(serde_json::json!({"role": "assistant", "content": &response}));
        if json_mode {
            return json_object_reply(&response);
        }
        return Ok(Value::String(response));
    }

    // Resolve LLM config
    let provider = cfg.map(|c| c.provider.clone()).unwrap_or_default();
    let api_key = std::env::var("SOMA_LLM_KEY")
        .or_else(|_| cfg.map(|c| resolve_env_vars(&c.key)).filter(|k| !k.is_empty()).ok_or(std::env::VarError::NotPresent))
        .or_else(|_| match provider.as_str() {
            "anthropic" => std::env::var("ANTHROPIC_API_KEY"),
            "openai" => std::env::var("OPENAI_API_KEY"),
            _ => std::env::var("OPENAI_API_KEY").or_else(|_| std::env::var("ANTHROPIC_API_KEY")),
        })
        .or_else(|_| if provider == "ollama" { Ok("ollama".to_string()) } else { Err(std::env::VarError::NotPresent) })
        .map_err(|_| {
            // said ONCE on stderr even when the program catches the error:
            // an operator saw "auto_approved" in 7 ms and no sign the model
            // was never reached
            static SAID: std::sync::Once = std::sync::Once::new();
            SAID.call_once(|| eprintln!("note: think() has no LLM key — every call raises kind \"llm\" (set SOMA_LLM_KEY, or SOMA_LLM_MOCK=echo to mock offline)"));
            RuntimeError::TypeError(format!(
            "think() requires API key. In soma.toml:\n\n    [agent]\n    provider = \"{}\"\n    key = \"${{ANTHROPIC_API_KEY}}\"\n\nOr set SOMA_LLM_KEY env var. Or SOMA_LLM_MOCK=echo for offline.",
            if provider.is_empty() { "anthropic" } else { &provider }
        ))})?;
    if api_key.is_empty() {
        return Err(RuntimeError::TypeError("think() API key empty.".to_string()));
    }

    let skill_content = cell_skill.and_then(|path| std::fs::read_to_string(&path).ok());
    let system_msg = system.or(skill_content.as_deref())
        .unwrap_or("You are a helpful AI agent. Be concise. Use tools when available.");

    let config = llm::LlmConfig {
        api_url: std::env::var("SOMA_LLM_URL").unwrap_or_else(|_| cfg.map(|c| c.resolve_url()).unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string())),
        api_key,
        model: std::env::var("SOMA_LLM_MODEL").unwrap_or_else(|_| cfg.map(|c| c.resolve_model()).unwrap_or_else(|| "gpt-4o-mini".to_string())),
        provider: provider.clone(),
        max_retries: std::env::var("SOMA_LLM_RETRIES").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| cfg.map(|c| c.retries).unwrap_or(3)),
        system_msg: system_msg.to_string(),
        timeout_ms: timeout_ms.unwrap_or_else(|| std::env::var("SOMA_LLM_TIMEOUT_MS").ok().and_then(|s| s.parse().ok()).unwrap_or(60_000)),
    };

    let (limit_rpm, limit_tpm) = cfg.map(|c| (c.rpm, c.tpm)).unwrap_or((0, 0));
    let tools = build_tool_definitions(interp, cell_name);

    // Multi-turn setup — an explicit system prompt replaces the previous
    // one (a second think() with its own system was sent the first's)
    if config.provider != "anthropic" {
        match interp.agent_conversation.first_mut() {
            None => interp.agent_conversation.push(serde_json::json!({"role": "system", "content": system_msg})),
            Some(first) if system.is_some() && first.get("role").and_then(|r| r.as_str()) == Some("system") => {
                first["content"] = serde_json::json!(system_msg);
            }
            _ => {}
        }
    }
    interp.agent_conversation.push(serde_json::json!({"role": "user", "content": prompt}));

    // Timeout tracking
    let start = std::time::Instant::now();

    // Tool-calling loop
    let rounds = interp.think_rounds.max(1);
    for iteration in 0..rounds {
        // Timeout check
        if let Some(tms) = timeout_ms {
            if start.elapsed().as_millis() as u64 > tms {
                return Err(RuntimeError::TypeError(format!(
                    "think() timed out after {}ms", tms
                )));
            }
        }

        if interp.agent_token_budget > 0 && interp.agent_tokens_used >= interp.agent_token_budget {
            return Err(RuntimeError::TypeError(format!("token budget exhausted: {}/{}", interp.agent_tokens_used, interp.agent_token_budget)));
        }

        let body = llm::build_request_body(&config, &interp.agent_conversation, &tools, json_mode, max_tokens);
        // a `[task]` step boundary: the wait for the model runs outside the
        // handler lock (other requests and tasks proceed meanwhile)
        let body_bytes = body.to_string().len();
        let want = (body_bytes as u64 + 3) / 4 + max_tokens.unwrap_or(1000);
        let (rpm, tpm) = (limit_rpm, limit_tpm);
        let bound = crate::interpreter::horde::request_bound(body_bytes, interp.agent_conversation.len(), max_tokens.unwrap_or(2048));
        let (budget, can_wait) = (interp.horde_budget.clone(), interp.task_unit.is_some());
        let (raw_json, reservation) = interp.outside_unit(|| -> Result<_, RuntimeError> {
            let r = crate::interpreter::horde::Reservation::take(budget, bound, can_wait)?;
            llm::limiter::acquire(rpm, tpm, want);
            // a failed call gives its reservation back (Reservation's Drop)
            Ok((llm::send_with_retry(&config, &body)?, r))
        })?;
        let mut resp = llm::parse_response(&config, &raw_json);
        // a provider that omits `usage` (or reports a negative count) spent
        // tokens all the same: estimate ~4 characters per token, as the mock
        // does — the budget was never charged
        if resp.tokens <= 0 {
            let chars = prompt.chars().count() + resp.content.chars().count()
                + resp.tool_calls.iter().map(|t| t.arguments_json.len() + t.name.len()).sum::<usize>();
            resp.tokens = ((chars as i64) + 3) / 4;
            resp.tokens = resp.tokens.max(1);
        }

        interp.agent_tokens_used += resp.tokens;
        interp.agent_trace.push(llm::trace_think_with(
            iteration as i64, if iteration == 0 { prompt } else { "(cont)" }, if iteration == 0 { system.unwrap_or("") } else { "" },
            resp.tokens, interp.agent_tokens_used, &resp.finish_reason,
        ));
        // the cost proof counts each reply at its max_tokens: a provider that
        // ignores the cap (10 000 tokens for max_tokens 60) is refused — the
        // tokens are charged, the reply is not used
        // the provider's COUNT is not trusted alone: a reply of 15 000
        // characters reported as 20 tokens passed the cap and the "proven"
        // cost bound — the reply is measured too (~4 characters per token,
        // as the mocks count)
        let reported = raw_json["usage"]["completion_tokens"].as_i64().or_else(|| raw_json["usage"]["output_tokens"].as_i64()).unwrap_or(0);
        // the tool call's id too: it is sent back twice on the next round
        let reply_chars = resp.content.chars().count() + resp.tool_calls.iter().map(|t| t.arguments_json.len() + t.name.len() + t.id.len()).sum::<usize>();
        let measured = ((reply_chars as i64) + 3) / 4;
        let out = reported.max(measured);
        // an under-reported reply is charged at its measured size too
        // (set_budget / tokens_used relied on the provider's count)
        if measured > reported { interp.agent_tokens_used += measured - reported; }
        // the horde's budget: the real charge replaces the reservation (a
        // provider that ignored max_tokens is charged all of it, and raises)
        if let Some(r) = reservation { r.settle(resp.tokens + (measured - reported).max(0)); }
        let cap = max_tokens.unwrap_or(2048) as i64;
        if out > cap {
            return Err(RuntimeError::Domain { kind: "llm".to_string(), message: format!("llm: the provider returned {} reply tokens for max_tokens {} — it ignored the cap the cost bound relies on", out, cap) });
        }

        // Tool calls
        if !resp.tool_calls.is_empty() {
            llm::push_assistant_tool_message(&config, &mut interp.agent_conversation, &raw_json);
            for tc in &resp.tool_calls {
                interp.tool_depth += 1;
                let result = dispatch_tool_call(interp, cell_name, &tc.name, &tc.arguments_json);
                interp.tool_depth -= 1;
                interp.agent_trace.push(llm::trace_tool_call(&tc.name, &tc.arguments_json, &format!("{}", result)));
                llm::push_tool_result(&config, &mut interp.agent_conversation, &tc.id, &format!("{}", result));
            }
            continue;
        }

        // Final response
        if !resp.content.is_empty() {
            interp.agent_conversation.push(serde_json::json!({"role": "assistant", "content": &resp.content}));
            if json_mode {
                return json_object_reply(&resp.content);
            }
            return Ok(Value::String(resp.content));
        }
        // no text and no tool call: the provider's JSON is not the model's
        // answer (it was returned as the reply — unmeasured, any size)
        return Err(RuntimeError::Domain { kind: "llm".to_string(), message:
            "llm: the provider's reply has neither text nor a tool call (empty, null or an unknown content shape)".to_string() });
    }

    if tools.is_empty() {
        // no tool to call: the reply was a tool call the cell cannot serve
        return Err(RuntimeError::TypeError("think(): the model answered with a tool call, but this cell declares no tools (face { tool … }) — the reply has no text".to_string()));
    }
    Err(RuntimeError::TypeError(format!("think() exceeded max rounds ({}) of tool calls — raise map(\"max_rounds\", N) (≤ 10) or give the model fewer steps", rounds)))
}

/// Build OpenAI function-calling tool definitions from a cell's face tool declarations
fn build_tool_definitions(interp: &Interpreter, cell_name: &str) -> Vec<serde_json::Value> {
    let mut tools = build_all_tool_definitions(interp, cell_name);
    // `map("tools_allowed", ["lookup"])`: only those are offered
    if let Some(allowed) = &interp.think_tools_allowed {
        tools.retain(|t| t["function"]["name"].as_str().map_or(false, |n| allowed.iter().any(|a| a == n)));
    }
    tools
}

fn build_all_tool_definitions(interp: &Interpreter, cell_name: &str) -> Vec<serde_json::Value> {
    let mut tools = Vec::new();
    if let Some(cell) = interp.cells.get(cell_name) {
        for section in &cell.sections {
            if let crate::ast::Section::Face(face) = &section.node {
                for decl in &face.declarations {
                    if let crate::ast::FaceDecl::Tool(tool) = &decl.node {
                        let mut properties = serde_json::Map::new();
                        let mut required = Vec::new();
                        for param in &tool.params {
                            // the JSON-schema type the parameter really takes (a Map
                            // advertised as "string" made the model send text the
                            // type check then refused)
                            let t = format!("{}", crate::commands::describe::format_type(&param.ty.node));
                            let param_type = match t.as_str() {
                                "Int" => "integer",
                                "Float" => "number",
                                "Bool" => "boolean",
                                x if x == "Map" || x.starts_with("Map<") => "object",
                                x if x == "List" || x.starts_with("List<") => "array",
                                _ => "string",
                            };
                            properties.insert(param.name.clone(), serde_json::json!({
                                "type": param_type,
                                "description": format!("Parameter: {}", param.name),
                            }));
                            required.push(serde_json::json!(param.name));
                        }
                        tools.push(serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description.as_deref().unwrap_or(&tool.name),
                                "parameters": {
                                    "type": "object",
                                    "properties": properties,
                                    "required": required,
                                }
                            }
                        }));
                    }
                }
            }
        }
    }
    tools
}

/// Dispatch a tool call from the LLM to the cell's handler
fn dispatch_tool_call(interp: &mut Interpreter, cell_name: &str, tool_name: &str, args_json: &str) -> Value {
    // Parse the JSON arguments: an object, or the call is refused (garbage
    // ran the tool with "" for every parameter)
    let args_val: serde_json::Value = match serde_json::from_str::<serde_json::Value>(args_json) {
        Ok(v @ serde_json::Value::Object(_)) => v,
        _ => return Value::String(format!("tool error: the arguments of '{}' are not a JSON object", tool_name)),
    };

    // Convert JSON args to Soma Value args (positional, matching handler params)
    let mut arg_values = Vec::new();
    let mut tool_caps: Option<Vec<String>> = None;
    if let Some(cell) = interp.cells.get(cell_name).cloned() {
        for section in &cell.sections {
            // V1.6: pull the declared capabilities for THIS tool (from face).
            if let crate::ast::Section::Face(ref face) = &section.node {
                for decl in &face.declarations {
                    if let crate::ast::FaceDecl::Tool(ref td) = decl.node {
                        if td.name == tool_name && !td.capabilities.is_empty() {
                            tool_caps = Some(td.capabilities.clone());
                        }
                    }
                }
            }
            if let crate::ast::Section::OnSignal(on) = &section.node {
                if on.signal_name == tool_name {
                    for param in &on.params {
                        // declared types are enforced like any call; a missing
                        // argument, a forged record/variant or a wrong type is
                        // a tool error the model sees (not a silent "")
                        let Some(val) = args_val.get(&param.name) else {
                            return Value::String(format!("tool error: '{}' needs the argument '{}'", tool_name, param.name));
                        };
                        fn forged(v: &serde_json::Value) -> bool {
                            match v {
                                serde_json::Value::Object(m) => m.contains_key("_type") || m.contains_key("_variant") || m.contains_key("_values") || m.values().any(forged),
                                serde_json::Value::Array(xs) => xs.iter().any(forged),
                                _ => false,
                            }
                        }
                        if forged(val) {
                            return Value::String(format!("tool error: argument '{}' carries _type / _variant", param.name));
                        }
                        let v = super::serde_json_to_value(val);
                        match crate::interpreter::check_param_type(param, v) {
                            Ok(v) => arg_values.push(v),
                            Err(m) => return Value::String(format!("tool error: {}(): {}", tool_name, m)),
                        }
                    }
                }
            }
        }
    }

    // Only a DECLARED `tool` is callable by the model: it could name any
    // handler of the cell (a private `_admin`, a state-changing `pay`) and
    // escape the capability list through it
    let declared = interp.cells.get(cell_name).map_or(false, |cell| cell.sections.iter().any(|s| matches!(&s.node,
        crate::ast::Section::Face(face) if face.declarations.iter().any(|d| matches!(&d.node, crate::ast::FaceDecl::Tool(td) if td.name == tool_name)))));
    if !declared {
        return Value::String(format!("tool error: '{}' is not a tool of this agent", tool_name));
    }
    if let Some(allowed) = &interp.think_tools_allowed {
        if !allowed.iter().any(|a| a == tool_name) {
            return Value::String(format!("tool error: '{}' is not allowed in this step (tools_allowed: {:?})", tool_name, allowed));
        }
    }
    // Scope the capability set for the duration of the call.
    let prev_caps = interp.current_tool_caps.take();
    let nested = prev_caps.is_some();
    if let Some(p) = prev_caps.clone() {
        interp.outer_tool_caps.push(p.clone());
        interp.current_tool_caps = Some(tool_caps.unwrap_or(p));
    } else {
        interp.current_tool_caps = tool_caps;
    }
    // a tool call that raises leaves nothing behind (its writes were kept
    // while the model was told it failed)
    let mark = interp.journal.as_ref().map(|j| j.len());
    // a think() inside the tool is its own conversation (it inserted a user
    // message between the assistant's tool_calls and the tool results)
    let saved_conversation = std::mem::take(&mut interp.agent_conversation);
    let saved_rounds = interp.think_rounds;
    let outcome = interp.call_signal(cell_name, tool_name, arg_values);
    interp.agent_conversation = saved_conversation;
    interp.think_rounds = saved_rounds;
    let result = match outcome {
        Ok(val) => val,
        Err(e) => {
            if let Some(m) = mark { interp.rollback_to(m); }
            Value::String(format!("tool error: {}", e))
        }
    };
    if nested { interp.outer_tool_caps.pop(); }
    interp.current_tool_caps = prev_caps;
    result
}

/// Resolve ${ENV_VAR} references in a string.
/// Examples:
///   "${ANTHROPIC_API_KEY}" → value of ANTHROPIC_API_KEY
///   "sk-${SUFFIX}" → "sk-" + value of SUFFIX
///   "literal" → "literal" (unchanged)
fn resolve_env_vars(s: &str) -> String {
    let mut result = s.to_string();
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find('}') {
            let var_name = &result[start + 2..start + end];
            let value = std::env::var(var_name).unwrap_or_default();
            result = format!("{}{}{}", &result[..start], value, &result[start + end + 1..]);
        } else {
            break;
        }
    }
    result
}

/// `think_json` promises a Map: a reply that is not a JSON object is an
/// error of kind `json` (it used to hand back the raw String, and a
/// classifier read `answer.category` as `()` without noticing). A ```json
/// fence or prose around the object is tolerated.
/// Under `soma serve` each request has its own interpreter: the trace of a
/// request used to vanish with it. The last 1000 steps, process-wide.
static SERVE_TRACE: std::sync::Mutex<std::collections::VecDeque<Value>> = std::sync::Mutex::new(std::collections::VecDeque::new());

pub fn serve_trace_extend(steps: &[Value]) {
    if steps.is_empty() { return; }
    let mut t = SERVE_TRACE.lock().unwrap_or_else(|e| e.into_inner());
    for s in steps { t.push_back(s.clone()); }
    while t.len() > 1000 { t.pop_front(); }
}

fn json_object_reply(text: &str) -> Result<Value, RuntimeError> {
    let candidates: Vec<&str> = {
        let mut v = vec![text.trim()];
        if let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) {
            if a < b { v.push(&text[a..=b]); }
        }
        v
    };
    for c in candidates {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(c) {
            if parsed.is_object() {
                return Ok(super::super::json_to_value(&parsed));
            }
        }
    }
    let shown: String = text.chars().take(80).collect();
    Err(RuntimeError::Domain {
        kind: "json".to_string(),
        message: format!("think_json(): the model did not answer a JSON object — got {:?}; catch it with `try`, or use think() and parse the text yourself", shown),
    })
}

fn cap_reply(text: &str, max_tokens: Option<u64>) -> String {
    match max_tokens {
        Some(n) => text.chars().take((n as usize).saturating_mul(4)).collect(),
        None => text.to_string(),
    }
}

/// Run a think() with the CALLING CELL's conversation: each agent cell has
/// its own multi-turn context (agent B was sent agent A's prompts and data).
fn with_cell_conversation(interp: &mut Interpreter, cell_name: &str, f: impl FnOnce(&mut Interpreter) -> Result<Value, RuntimeError>) -> Result<Value, RuntimeError> {
    let own = interp.agent_conversations.remove(cell_name).unwrap_or_default();
    let outer = std::mem::replace(&mut interp.agent_conversation, own);
    let result = f(interp);
    let own = std::mem::replace(&mut interp.agent_conversation, outer);
    interp.agent_conversations.insert(cell_name.to_string(), own);
    result
}

/// `SOMA_LLM_MOCK=rules:<file>`: the rules, read once per path (relative
/// to the working directory, else to the program's directory).
fn mock_rules(path: &str) -> Result<Vec<serde_json::Value>, String> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Vec<serde_json::Value>>>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(r) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(path) { return Ok(r.clone()); }
    let p = std::path::PathBuf::from(path);
    let text = std::fs::read_to_string(&p)
        .or_else(|_| {
            let dir = crate::runtime::storage::data_dir();
            std::fs::read_to_string(dir.parent().unwrap_or(std::path::Path::new(".")).join(&p))
        })
        .map_err(|e| format!("cannot read {}: {}", path, e))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{} is not JSON: {}", path, e))?;
    let rules = match v {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(ref o) if o.get("rules").map_or(false, |r| r.is_array()) => o["rules"].as_array().cloned().unwrap_or_default(),
        _ => return Err(format!("{}: expected a list of {{\"match\", \"cell\", \"reply\"}} rules", path)),
    };
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_string(), rules.clone());
    Ok(rules)
}

/// The cell's own agent-memory table (persistent under run/serve).
pub(crate) fn agent_memory(interp: &mut Interpreter, cell_name: &str) -> std::sync::Arc<dyn crate::runtime::storage::StorageBackend> {
    let slot_key = format!("{}.__agent_memory", cell_name);
    if !interp.storage.contains_key(&slot_key) {
        let persistent = crate::interpreter::PERSIST_MACHINES.load(std::sync::atomic::Ordering::Relaxed)
            || interp.storage.values().any(|b| b.backend_name() == "sqlite");
        let backend: std::sync::Arc<dyn crate::runtime::storage::StorageBackend> = if persistent {
            std::sync::Arc::new(crate::runtime::storage::SqliteBackend::new(cell_name, "_agent_memory"))
        } else {
            std::sync::Arc::new(crate::runtime::storage::MemoryBackend::new())
        };
        interp.storage.insert(slot_key.clone(), backend);
    }
    interp.storage.get(&slot_key).cloned().unwrap()
}

/// `map("tools_allowed", ["t1", …])` of a think() call.
fn tools_allowed(args: &[Value]) -> Result<Option<Vec<String>>, RuntimeError> {
    let Some(Value::Map(m)) = args.last() else { return Ok(None) };
    match m.get("tools_allowed") {
        None => Ok(None),
        Some(Value::List(xs)) if xs.iter().all(|x| matches!(x, Value::String(_))) =>
            Ok(Some(xs.iter().map(|x| format!("{}", x)).collect())),
        Some(other) => Err(RuntimeError::TypeError(format!("think(): tools_allowed must be a List of tool names, got {}", other))),
    }
}

/// Run a think() with its tool list narrowed (restored after: a tool's own
/// think() has its own list).
fn with_tools_allowed<T>(interp: &mut Interpreter, allowed: Option<Vec<String>>, f: impl FnOnce(&mut Interpreter) -> T) -> T {
    let prev = std::mem::replace(&mut interp.think_tools_allowed, allowed);
    let out = f(interp);
    interp.think_tools_allowed = prev;
    out
}

/// An instance id is a String or an Int (an Int is its decimal text, so
/// `42` and `"42"` are one instance). `()` — a missing record's `.id` — moved
/// one shared instance "null" for every caller; a Float / List / Map id
/// aliased by its printed text.
fn instance_id(v: &Value, f: &str) -> Result<String, RuntimeError> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Int(i) => Ok(format!("{}", i)),
        other => Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("type: {}(): an instance id is a String or an Int, got {} {}", f, crate::interpreter::value_type_name(other), other) }),
    }
}
