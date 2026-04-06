use super::super::{Value, RuntimeError, Interpreter};

pub fn call_builtin(interp: &mut Interpreter, name: &str, args: &[Value], cell_name: &str) -> Option<Result<Value, RuntimeError>> {
    match name {
        "next_id" => {
            let counter_key = "__next_id";
            let slot = interp.storage.iter()
                .find(|(k, _)| k.starts_with(&format!("{}.", cell_name)))
                .or_else(|| interp.storage.iter().next())
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
        // ── Agent: gather(items, cell, signal) — fan-out pattern ────
        // Calls cell.signal(item) for each item, collects results into a list
        "gather" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let target_cell = format!("{}", args[1]);
                    let signal_name = format!("{}", args[2]);
                    let mut results = Vec::new();
                    for item in items {
                        match interp.call_signal(&target_cell, &signal_name, vec![item.clone()]) {
                            Ok(val) => results.push(val),
                            Err(e) => results.push(Value::String(format!("error: {}", e))),
                        }
                    }
                    Some(Ok(Value::List(results)))
                } else {
                    Some(Err(RuntimeError::TypeError("gather(list, cell_name, signal_name)".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("gather(list, cell_name, signal_name) requires 3 args".to_string())))
            }
        }
        // ── Agent: broadcast(signal, data) — emit to all cells ──────
        "broadcast" => {
            if args.len() >= 2 {
                let signal_name = format!("{}", args[0]);
                let data = args[1].clone();
                // Find all cells with matching handler
                let matching: Vec<String> = interp.handler_cache.keys()
                    .filter(|(_, s)| s.as_str() == signal_name.as_str())
                    .map(|(c, _)| c.clone())
                    .collect();
                let mut count = 0;
                for target in matching {
                    let _ = interp.call_signal(&target, &signal_name, vec![data.clone()]);
                    count += 1;
                }
                Some(Ok(Value::Int(count)))
            } else {
                Some(Err(RuntimeError::TypeError("broadcast(signal_name, data)".to_string())))
            }
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
            if let Some(Value::Int(n)) = args.first() {
                interp.agent_token_budget = *n;
                interp.agent_tokens_used = 0;
                Some(Ok(Value::Unit))
            } else {
                Some(Err(RuntimeError::TypeError("set_budget(max_tokens: Int)".to_string())))
            }
        }
        "tokens_used" => {
            Some(Ok(Value::Int(interp.agent_tokens_used)))
        }
        "tokens_remaining" => {
            if interp.agent_token_budget > 0 {
                Some(Ok(Value::Int(interp.agent_token_budget - interp.agent_tokens_used)))
            } else {
                Some(Ok(Value::Int(-1))) // unlimited
            }
        }
        // ── Agent: trace() — get execution log ──────────────────────
        "trace" => {
            Some(Ok(Value::List(interp.agent_trace.clone())))
        }
        "clear_trace" => {
            interp.agent_trace.clear();
            Some(Ok(Value::Unit))
        }
        // ── Agent: context() — get/clear conversation history ───────
        "clear_context" => {
            interp.agent_conversation.clear();
            Some(Ok(Value::Unit))
        }
        // ── Agent: approve(action) — human-in-the-loop ─────────────
        "approve" => {
            if let Some(Value::String(action)) = args.first() {
                // In serve mode: pause and wait for HTTP approval
                // In run mode: auto-approve with a warning
                eprintln!("[agent] approval requested: {}", action);
                eprintln!("[agent] auto-approved (use soma serve for interactive approval)");
                // Log to trace
                interp.agent_trace.push(super::super::map_from_pairs(vec![
                    ("event".to_string(), Value::String("approval".to_string())),
                    ("action".to_string(), Value::String(action.clone())),
                    ("result".to_string(), Value::String("auto_approved".to_string())),
                    ("timestamp".to_string(), Value::Int(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64)),
                ]));
                Some(Ok(Value::Bool(true)))
            } else {
                Some(Err(RuntimeError::TypeError("approve(action: String)".to_string())))
            }
        }
        // ── AI Agent: think() with tool-calling loop ──────────────
        "think" => {
            if let Some(Value::String(prompt)) = args.first() {
                let system = args.get(1).and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None });
                Some(agent_think(interp, cell_name, prompt, system.as_deref(), false))
            } else {
                Some(Err(RuntimeError::TypeError("think(prompt: String) requires a string argument".to_string())))
            }
        }
        "think_json" => {
            if let Some(Value::String(prompt)) = args.first() {
                let system = args.get(1).and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None });
                Some(agent_think(interp, cell_name, prompt, system.as_deref(), true))
            } else {
                Some(Err(RuntimeError::TypeError("think_json(prompt: String) requires a string argument".to_string())))
            }
        }
        _ => None,
    }
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
    let mock_val = std::env::var("SOMA_LLM_MOCK").ok()
        .or_else(|| if cfg_mock.is_empty() { None } else { Some(cfg_mock) });
    if let Some(mock) = mock_val {
        let response = match mock.as_str() {
            "echo" => prompt.to_string(),
            s if s.starts_with("fixed:") => s[6..].to_string(),
            _ => format!("[mock] {}", prompt),
        };
        interp.agent_trace.push(super::super::map_from_pairs(vec![
            ("event".to_string(), Value::String("think".to_string())),
            ("prompt".to_string(), Value::String(prompt.to_string())),
            ("mock".to_string(), Value::Bool(true)),
        ]));
        if interp.agent_conversation.is_empty() {
            interp.agent_conversation.push(serde_json::json!({"role": "system", "content": "mock"}));
        }
        interp.agent_conversation.push(serde_json::json!({"role": "user", "content": prompt}));
        interp.agent_conversation.push(serde_json::json!({"role": "assistant", "content": &response}));
        if json_mode {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&response) {
                return Ok(super::super::json_to_value(&parsed));
            }
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
        .map_err(|_| RuntimeError::TypeError(format!(
            "think() requires API key. In soma.toml:\n\n    [agent]\n    provider = \"{}\"\n    key = \"${{ANTHROPIC_API_KEY}}\"\n\nOr set SOMA_LLM_KEY env var. Or SOMA_LLM_MOCK=echo for offline.",
            if provider.is_empty() { "anthropic" } else { &provider }
        )))?;
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
    };

    let tools = build_tool_definitions(interp, cell_name);

    // Multi-turn setup
    if interp.agent_conversation.is_empty() && config.provider != "anthropic" {
        interp.agent_conversation.push(serde_json::json!({"role": "system", "content": system_msg}));
    }
    interp.agent_conversation.push(serde_json::json!({"role": "user", "content": prompt}));

    // Tool-calling loop
    for iteration in 0..10 {
        if interp.agent_token_budget > 0 && interp.agent_tokens_used >= interp.agent_token_budget {
            return Err(RuntimeError::TypeError(format!("token budget exhausted: {}/{}", interp.agent_tokens_used, interp.agent_token_budget)));
        }

        let body = llm::build_request_body(&config, &interp.agent_conversation, &tools, json_mode);
        let raw_json = llm::send_with_retry(&config, &body)?;
        let resp = llm::parse_response(&config, &raw_json);

        interp.agent_tokens_used += resp.tokens;
        interp.agent_trace.push(llm::trace_think(
            iteration as i64, if iteration == 0 { prompt } else { "(cont)" }, resp.tokens, interp.agent_tokens_used, &resp.finish_reason,
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
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&resp.content) {
                    return Ok(super::super::json_to_value(&parsed));
                }
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
    if let Some(cell) = interp.cells.get(cell_name).cloned() {
        for section in &cell.sections {
            if let crate::ast::Section::OnSignal(on) = &section.node {
                if on.signal_name == tool_name {
                    for param in &on.params {
                        let val = args_val.get(&param.name);
                        arg_values.push(match val {
                            Some(serde_json::Value::String(s)) => Value::String(s.clone()),
                            Some(serde_json::Value::Number(n)) => {
                                if let Some(i) = n.as_i64() { Value::Int(i) }
                                else { Value::Float(n.as_f64().unwrap_or(0.0)) }
                            }
                            Some(serde_json::Value::Bool(b)) => Value::Bool(*b),
                            _ => Value::String(val.map(|v| v.to_string()).unwrap_or_default()),
                        });
                    }
                    break;
                }
            }
        }
    }

    // Call the handler
    match interp.call_signal(cell_name, tool_name, arg_values) {
        Ok(val) => val,
        Err(e) => Value::String(format!("tool error: {}", e)),
    }
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
