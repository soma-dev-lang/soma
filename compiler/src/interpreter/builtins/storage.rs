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
                Some(interp.do_transition(&id, &target))
            } else {
                Some(Err(RuntimeError::TypeError("transition(id, target_state) requires 2 args".to_string())))
            }
        }
        "get_status" => {
            if let Some(id) = args.first() {
                let id_str = format!("{}", id);
                Some(interp.do_get_status(&id_str))
            } else {
                Some(Err(RuntimeError::TypeError("get_status(id) requires 1 arg".to_string())))
            }
        }
        "valid_transitions" => {
            if let Some(id) = args.first() {
                let id_str = format!("{}", id);
                Some(Ok(interp.do_valid_transitions(&id_str)))
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
    // Check token budget before calling
    if interp.agent_token_budget > 0 && interp.agent_tokens_used >= interp.agent_token_budget {
        return Err(RuntimeError::TypeError(format!(
            "token budget exhausted: used {}/{}", interp.agent_tokens_used, interp.agent_token_budget
        )));
    }

    let api_url = std::env::var("SOMA_LLM_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".to_string());
    let api_key = std::env::var("SOMA_LLM_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
        .map_err(|_| RuntimeError::TypeError(
            "think() requires SOMA_LLM_KEY or OPENAI_API_KEY env var".to_string()
        ))?;
    let model = std::env::var("SOMA_LLM_MODEL")
        .unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let max_retries: usize = std::env::var("SOMA_LLM_RETRIES")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(3);

    let system_msg = system.unwrap_or("You are a helpful AI agent. Be concise. Use tools when available.");
    let tools = build_tool_definitions(interp, cell_name);

    // Multi-turn: if conversation exists, append; otherwise start fresh
    if interp.agent_conversation.is_empty() {
        interp.agent_conversation.push(serde_json::json!({"role": "system", "content": system_msg}));
    }
    interp.agent_conversation.push(serde_json::json!({"role": "user", "content": prompt}));

    let ts = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;

    // Tool-calling loop (max 10 iterations)
    for iteration in 0..10 {
        // Budget check per iteration
        if interp.agent_token_budget > 0 && interp.agent_tokens_used >= interp.agent_token_budget {
            return Err(RuntimeError::TypeError(format!(
                "token budget exhausted during think loop: {}/{}", interp.agent_tokens_used, interp.agent_token_budget
            )));
        }

        let mut body = serde_json::json!({
            "model": model,
            "messages": interp.agent_conversation,
            "max_tokens": 2048,
        });
        if !tools.is_empty() { body["tools"] = serde_json::json!(tools); }
        if json_mode { body["response_format"] = serde_json::json!({"type": "json_object"}); }

        // Retry with exponential backoff
        let mut last_error = String::new();
        let mut resp_text = None;
        for retry in 0..=max_retries {
            match ureq::post(&api_url)
                .set("Authorization", &format!("Bearer {}", api_key))
                .set("Content-Type", "application/json")
                .send_string(&body.to_string())
            {
                Ok(response) => {
                    resp_text = Some(response.into_string()
                        .map_err(|e| RuntimeError::TypeError(format!("think() response error: {}", e)))?);
                    break;
                }
                Err(e) => {
                    last_error = format!("{}", e);
                    let is_retryable = last_error.contains("429") || last_error.contains("500")
                        || last_error.contains("502") || last_error.contains("503")
                        || last_error.contains("timeout");
                    if is_retryable && retry < max_retries {
                        let delay = std::time::Duration::from_millis(500 * (1 << retry));
                        eprintln!("[agent] retry {}/{} after {:?}: {}", retry + 1, max_retries, delay, last_error);
                        std::thread::sleep(delay);
                        continue;
                    }
                    return Err(RuntimeError::TypeError(format!("think() failed after {} retries: {}", max_retries, last_error)));
                }
            }
        }
        let text = resp_text.ok_or_else(|| RuntimeError::TypeError(format!("think() no response: {}", last_error)))?;
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| RuntimeError::TypeError(format!("think() JSON error: {}", e)))?;

        if let Some(err) = json["error"]["message"].as_str() {
            return Err(RuntimeError::TypeError(format!("LLM error: {}", err)));
        }

        // Track token usage
        let usage = &json["usage"];
        let tokens_this_call = usage["total_tokens"].as_i64().unwrap_or(0);
        interp.agent_tokens_used += tokens_this_call;

        let choice = &json["choices"][0];
        let message = &choice["message"];
        let finish_reason = choice["finish_reason"].as_str().unwrap_or("");

        // Trace this LLM call
        interp.agent_trace.push(super::super::map_from_pairs(vec![
            ("event".to_string(), Value::String("think".to_string())),
            ("iteration".to_string(), Value::Int(iteration as i64)),
            ("prompt".to_string(), Value::String(if iteration == 0 { prompt.to_string() } else { "(continuation)".to_string() })),
            ("tokens".to_string(), Value::Int(tokens_this_call)),
            ("total_tokens".to_string(), Value::Int(interp.agent_tokens_used)),
            ("finish_reason".to_string(), Value::String(finish_reason.to_string())),
            ("timestamp".to_string(), Value::Int(ts())),
        ]));

        // Handle tool calls
        if finish_reason == "tool_calls" || message["tool_calls"].is_array() {
            if let Some(tool_calls) = message["tool_calls"].as_array() {
                interp.agent_conversation.push(message.clone());

                for tc in tool_calls {
                    let tool_name = tc["function"]["name"].as_str().unwrap_or("");
                    let tool_args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                    let tool_id = tc["id"].as_str().unwrap_or("");

                    let result = dispatch_tool_call(interp, cell_name, tool_name, tool_args_str);

                    // Trace the tool call
                    interp.agent_trace.push(super::super::map_from_pairs(vec![
                        ("event".to_string(), Value::String("tool_call".to_string())),
                        ("tool".to_string(), Value::String(tool_name.to_string())),
                        ("args".to_string(), Value::String(tool_args_str.to_string())),
                        ("result".to_string(), Value::String(format!("{}", result))),
                        ("timestamp".to_string(), Value::Int(ts())),
                    ]));

                    interp.agent_conversation.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tool_id,
                        "content": format!("{}", result),
                    }));
                }
                continue; // LLM will see tool results
            }
        }

        // Final response
        if let Some(content) = message["content"].as_str() {
            // Add to conversation for multi-turn
            interp.agent_conversation.push(serde_json::json!({"role": "assistant", "content": content}));

            if json_mode {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
                    return Ok(super::super::json_to_value(&parsed));
                }
            }
            return Ok(Value::String(content.to_string()));
        }
        return Ok(Value::String(text));
    }

    Err(RuntimeError::TypeError("think() exceeded max tool-calling iterations (10)".to_string()))
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
