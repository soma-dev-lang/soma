use super::super::{Value, RuntimeError, Interpreter};
use crate::interpreter::soma_int::SomaInt;

pub fn call_builtin(interp: &mut Interpreter, name: &str, args: &[Value], cell_name: &str) -> Option<Result<Value, RuntimeError>> {
    match name {
        "next_id" => {
            // The counter lives in the cell's FIRST declared memory slot
            // (declaration order — never HashMap order, which differs per
            // thread under `soma serve` and handed the same id out twice),
            // and the write is journaled: a refused request burns no id.
            let counter_key = "__next_id";
            let backend = interp.next_id_backend(cell_name);
            if let Some(backend) = backend {
                let current = backend.get(counter_key)
                    .and_then(|v| match v {
                        crate::runtime::storage::StoredValue::Int(n) => Some(n),
                        _ => None,
                    })
                    .unwrap_or(0);
                let next = current + 1;
                if let Some(j) = interp.journal.as_mut() {
                    j.push(crate::interpreter::UndoOp::Restore {
                        backend: backend.clone(),
                        key: counter_key.to_string(),
                        prev: backend.get(counter_key),
                    });
                }
                backend.set(counter_key, crate::runtime::storage::StoredValue::Int(next));
                Some(Ok(Value::Int(SomaInt::from_i64(next))))
            } else {
                Some(Ok(Value::Int(SomaInt::from_i64(1))))
            }
        }
        "transition" => {
            if args.len() >= 2 {
                let id = format!("{}", args[0]);
                let target = format!("{}", args[1]);
                Some(interp.do_transition_for(cell_name, &id, &target))
            } else {
                Some(Err(RuntimeError::TypeError("transition(id, target_state) requires 2 args".to_string())))
            }
        }
        "get_status" => {
            if let Some(id) = args.first() {
                let id_str = format!("{}", id);
                Some(interp.do_get_status_for(cell_name, &id_str))
            } else {
                Some(Err(RuntimeError::TypeError("get_status(id) requires 1 arg".to_string())))
            }
        }
        "has_state" => {
            if let Some(id) = args.first() {
                let id_str = format!("{}", id);
                Some(Ok(Value::Bool(interp.do_has_state_for(cell_name, &id_str))))
            } else {
                Some(Err(RuntimeError::TypeError("has_state(id) requires 1 arg".to_string())))
            }
        }
        "valid_transitions" => {
            if let Some(id) = args.first() {
                let id_str = format!("{}", id);
                Some(Ok(interp.do_valid_transitions_for(cell_name, &id_str)))
            } else {
                Some(Err(RuntimeError::TypeError("valid_transitions(id) requires 1 arg".to_string())))
            }
        }
        // ── Agent memory: remember(key, value), recall(key) ─────────
        "remember" => {
            if args.len() >= 2 {
                let key = format!("{}", args[0]);
                let val = &args[1];
                // Store in __agent_memory slot
                let slot_key = format!("{}.__agent_memory", cell_name);
                if let Some(backend) = interp.storage.get(&slot_key).or_else(|| {
                    interp.storage.iter().find(|(k, _)| k.ends_with(".__agent_memory") || k.as_str() == "__agent_memory").map(|(_, v)| v)
                }) {
                    backend.set(&key, super::super::value_to_stored(val));
                    Some(Ok(Value::Unit))
                } else {
                    // Auto-create memory in first available storage
                    if let Some((_, backend)) = interp.storage.iter().next() {
                        backend.set(&format!("__mem_{}", key), super::super::value_to_stored(val));
                        Some(Ok(Value::Unit))
                    } else {
                        Some(Err(RuntimeError::TypeError("remember() requires a memory slot".to_string())))
                    }
                }
            } else {
                Some(Err(RuntimeError::TypeError("remember(key, value)".to_string())))
            }
        }
        "recall" => {
            if let Some(Value::String(key)) = args.first() {
                // Recall from any storage slot
                for (_, backend) in interp.storage.iter() {
                    if let Some(val) = backend.get(key).or_else(|| backend.get(&format!("__mem_{}", key))) {
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
            if let Some(ref bus) = interp.event_bus {
                let event = crate::interpreter::BusEvent { stream, data };
                if let Ok(senders) = bus.lock() {
                    for sender in senders.iter() {
                        let _ = sender.send(event.clone());
                    }
                }
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
                    .map_err(|e| RuntimeError::TypeError(format!("delegate error: {}", e))))
            } else {
                Some(Err(RuntimeError::TypeError("delegate(cell_name, signal_name, ...args) requires at least 2 args".to_string())))
            }
        }
        // ── Agent: set_budget(max_tokens) ──────────────────────────
        "set_budget" => {
            if let Some(Value::Int(si)) = args.first() {
                interp.agent_token_budget = si.to_i64().unwrap_or(0);
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
            Some(Ok(Value::Unit))
        }
        // ── Agent: approve(action) — human-in-the-loop ─────────────
        "approve" => {
            let Some(Value::String(action)) = args.first() else {
                return Some(Err(RuntimeError::TypeError("approve(action: String)".to_string())));
            };
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
                let (system, max_tokens, timeout_ms) = extract_think_opts(args);
                Some(agent_think(interp, cell_name, prompt, system.as_deref(), false, max_tokens, timeout_ms))
            } else {
                Some(Err(RuntimeError::TypeError("think(prompt: String) requires a string argument".to_string())))
            }
        }
        "think_json" => {
            if let Some(Value::String(prompt)) = args.first() {
                let (system, max_tokens, timeout_ms) = extract_think_opts(args);
                Some(agent_think(interp, cell_name, prompt, system.as_deref(), true, max_tokens, timeout_ms))
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
        if let Some(Value::Int(n)) = m.get("timeout") {
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
    if let Some(Err(msg)) = &scripted {
        interp.agent_trace.push(super::llm::trace_think(0, prompt, 0, 0, "error"));
        return Err(RuntimeError::TypeError(format!("think() failed: {} (scripted by `mock think error`)", msg)));
    }
    if scripted.is_some() && mock_val.is_none() {
        mock_val = Some("echo".to_string());
    }

    if let Some(mock) = mock_val {
        let response = match (&scripted, mock.as_str()) {
            (Some(Ok(text)), _) => text.clone(),
            // a real provider stops at max_tokens: so does the mock (~4
            // characters per token) — a scripted reply is kept as written
            (_, "echo") => cap_reply(prompt, max_tokens),
            (_, s) if s.starts_with("fixed:") => cap_reply(&s[6..], max_tokens),
            (_, other) => {
                // a configuration mistake, not a domain error: a handler's
                // `try { think(..) }` must not be able to swallow it
                eprintln!(
                    "error: unknown LLM mock mode '{}' (SOMA_LLM_MOCK or [agent] mock) — expected `echo` \
                     (reply = the prompt) or `fixed:<text>`",
                    other
                );
                std::process::exit(2);
            }
        };
        // a mocked call still costs: ~4 characters per token, prompt and
        // reply, so `set_budget` exhaustion (kind `budget`) is testable offline
        let est = ((prompt.chars().count() + response.chars().count()) as i64 + 3) / 4;
        let est = est.max(1);
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

    let tools = build_tool_definitions(interp, cell_name);

    // Multi-turn setup
    if interp.agent_conversation.is_empty() && config.provider != "anthropic" {
        interp.agent_conversation.push(serde_json::json!({"role": "system", "content": system_msg}));
    }
    interp.agent_conversation.push(serde_json::json!({"role": "user", "content": prompt}));

    // Timeout tracking
    let start = std::time::Instant::now();

    // Tool-calling loop
    for iteration in 0..10 {
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
        let raw_json = llm::send_with_retry(&config, &body)?;
        let resp = llm::parse_response(&config, &raw_json);

        interp.agent_tokens_used += resp.tokens;
        interp.agent_trace.push(llm::trace_think_with(
            iteration as i64, if iteration == 0 { prompt } else { "(cont)" }, if iteration == 0 { system.unwrap_or("") } else { "" },
            resp.tokens, interp.agent_tokens_used, &resp.finish_reason,
        ));

        // Tool calls
        if !resp.tool_calls.is_empty() {
            llm::push_assistant_tool_message(&config, &mut interp.agent_conversation, &raw_json);
            for tc in &resp.tool_calls {
                let result = dispatch_tool_call(interp, cell_name, &tc.name, &tc.arguments_json);
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
        return Ok(Value::String(serde_json::to_string(&raw_json).unwrap_or_default()));
    }

    Err(RuntimeError::TypeError("think() exceeded max iterations (10)".to_string()))
}

/// Build OpenAI function-calling tool definitions from a cell's face tool declarations
fn build_tool_definitions(interp: &Interpreter, cell_name: &str) -> Vec<serde_json::Value> {
    let mut tools = Vec::new();
    if let Some(cell) = interp.cells.get(cell_name) {
        for section in &cell.sections {
            if let crate::ast::Section::Face(face) = &section.node {
                for decl in &face.declarations {
                    if let crate::ast::FaceDecl::Tool(tool) = &decl.node {
                        let mut properties = serde_json::Map::new();
                        let mut required = Vec::new();
                        for param in &tool.params {
                            let param_type = match format!("{}", crate::commands::describe::format_type(&param.ty.node)).as_str() {
                                "Int" | "Float" => "number",
                                "Bool" => "boolean",
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
    // Parse the JSON arguments
    let args_val: serde_json::Value = serde_json::from_str(args_json).unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

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
                        let val = args_val.get(&param.name);
                        arg_values.push(match val {
                            Some(serde_json::Value::String(s)) => Value::String(s.clone()),
                            Some(serde_json::Value::Number(n)) => {
                                if let Some(i) = n.as_i64() { Value::Int(SomaInt::from_i64(i)) }
                                else { Value::Float(n.as_f64().unwrap_or(0.0)) }
                            }
                            Some(serde_json::Value::Bool(b)) => Value::Bool(*b),
                            _ => Value::String(val.map(|v| v.to_string()).unwrap_or_default()),
                        });
                    }
                }
            }
        }
    }

    // Scope the capability set for the duration of the call.
    let prev_caps = interp.current_tool_caps.take();
    interp.current_tool_caps = tool_caps;
    let result = match interp.call_signal(cell_name, tool_name, arg_values) {
        Ok(val) => val,
        Err(e) => Value::String(format!("tool error: {}", e)),
    };
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
