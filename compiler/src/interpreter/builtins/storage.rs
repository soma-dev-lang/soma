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

/// Core agent think loop with tool calling.
/// 1. Reads tool declarations from the cell's face section
/// 2. Sends them as OpenAI function-calling tools
/// 3. When LLM returns tool_calls, dispatches to the cell's handlers
/// 4. Feeds results back, loops until final text response
fn agent_think(
    interp: &mut Interpreter,
    cell_name: &str,
    prompt: &str,
    system: Option<&str>,
    json_mode: bool,
) -> Result<Value, RuntimeError> {
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

    let system_msg = system.unwrap_or("You are a helpful AI agent. Be concise. Use tools when available.");

    // Build tool definitions from the cell's face declarations
    let tools = build_tool_definitions(interp, cell_name);

    // Conversation messages for the loop
    let mut messages = vec![
        serde_json::json!({"role": "system", "content": system_msg}),
        serde_json::json!({"role": "user", "content": prompt}),
    ];

    // Tool-calling loop (max 10 iterations to prevent runaway)
    for _iteration in 0..10 {
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "max_tokens": 2048,
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools);
        }
        if json_mode {
            body["response_format"] = serde_json::json!({"type": "json_object"});
        }

        let resp = ureq::post(&api_url)
            .set("Authorization", &format!("Bearer {}", api_key))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| RuntimeError::TypeError(format!("think() HTTP error: {}", e)))?;

        let text = resp.into_string()
            .map_err(|e| RuntimeError::TypeError(format!("think() response error: {}", e)))?;
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| RuntimeError::TypeError(format!("think() JSON error: {}", e)))?;

        if let Some(err) = json["error"]["message"].as_str() {
            return Err(RuntimeError::TypeError(format!("LLM error: {}", err)));
        }

        let choice = &json["choices"][0];
        let message = &choice["message"];
        let finish_reason = choice["finish_reason"].as_str().unwrap_or("");

        // Check for tool calls
        if finish_reason == "tool_calls" || message["tool_calls"].is_array() {
            if let Some(tool_calls) = message["tool_calls"].as_array() {
                // Add assistant message with tool calls to conversation
                messages.push(message.clone());

                // Execute each tool call
                for tc in tool_calls {
                    let tool_name = tc["function"]["name"].as_str().unwrap_or("");
                    let tool_args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                    let tool_id = tc["id"].as_str().unwrap_or("");

                    // Parse tool arguments and dispatch to the cell's handler
                    let result = dispatch_tool_call(interp, cell_name, tool_name, tool_args_str);

                    // Add tool result to conversation
                    messages.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tool_id,
                        "content": format!("{}", result),
                    }));
                }
                // Continue the loop — LLM will see tool results
                continue;
            }
        }

        // Final response — extract content
        if let Some(content) = message["content"].as_str() {
            if json_mode {
                // Parse JSON response into a Map
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
