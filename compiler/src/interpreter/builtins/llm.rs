//! LLM provider abstraction for think().
//! Handles OpenAI, Anthropic, and Ollama API differences.

use super::super::{Value, RuntimeError, map_from_pairs};
use crate::interpreter::soma_int::SomaInt;

/// Resolved LLM configuration
pub struct LlmConfig {
    pub api_url: String,
    pub api_key: String,
    pub model: String,
    pub provider: String,
    pub max_retries: usize,
    pub system_msg: String,
}

/// Unified LLM response
pub struct LlmResponse {
    pub content: String,
    pub finish_reason: String,
    pub tool_calls: Vec<ToolCall>,
    pub tokens: i64,
}

/// A tool call from the LLM
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
}

/// Build the HTTP request body for the given provider
pub fn build_request_body(
    config: &LlmConfig,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    json_mode: bool,
) -> serde_json::Value {
    if config.provider == "anthropic" {
        let msgs: Vec<serde_json::Value> = messages.iter()
            .filter(|m| m["role"].as_str() != Some("system"))
            .cloned().collect();
        let mut body = serde_json::json!({
            "model": config.model,
            "messages": msgs,
            "max_tokens": 2048,
            "system": config.system_msg,
        });
        if !tools.is_empty() {
            let anthropic_tools: Vec<serde_json::Value> = tools.iter().map(|t| {
                serde_json::json!({
                    "name": t["function"]["name"],
                    "description": t["function"]["description"],
                    "input_schema": t["function"]["parameters"],
                })
            }).collect();
            body["tools"] = serde_json::json!(anthropic_tools);
        }
        body
    } else {
        let mut body = serde_json::json!({
            "model": config.model,
            "messages": messages,
            "max_tokens": 2048,
        });
        if !tools.is_empty() { body["tools"] = serde_json::json!(tools); }
        if json_mode { body["response_format"] = serde_json::json!({"type": "json_object"}); }
        body
    }
}

/// Send HTTP request with retry and exponential backoff
pub fn send_with_retry(
    config: &LlmConfig,
    body: &serde_json::Value,
) -> Result<serde_json::Value, RuntimeError> {
    let mut last_error = String::new();

    for retry in 0..=config.max_retries {
        let mut req = ureq::post(&config.api_url)
            .set("Content-Type", "application/json");

        if config.provider == "anthropic" {
            req = req.set("x-api-key", &config.api_key)
                .set("anthropic-version", "2023-06-01");
        } else {
            req = req.set("Authorization", &format!("Bearer {}", config.api_key));
        }

        match req.send_string(&body.to_string()) {
            Ok(response) => {
                let text = response.into_string()
                    .map_err(|e| RuntimeError::TypeError(format!("think() response error: {}", e)))?;
                let json: serde_json::Value = serde_json::from_str(&text)
                    .map_err(|e| RuntimeError::TypeError(format!("think() JSON error: {}", e)))?;
                if let Some(err) = json["error"]["message"].as_str() {
                    return Err(RuntimeError::TypeError(format!("LLM error: {}", err)));
                }
                return Ok(json);
            }
            Err(e) => {
                last_error = format!("{}", e);
                let retryable = ["429", "500", "502", "503", "529", "timeout"]
                    .iter().any(|code| last_error.contains(code));
                if retryable && retry < config.max_retries {
                    let delay = std::time::Duration::from_millis(500 * (1 << retry));
                    eprintln!("[agent] retry {}/{} after {:?}: {}", retry + 1, config.max_retries, delay, last_error);
                    std::thread::sleep(delay);
                    continue;
                }
            }
        }
    }
    Err(RuntimeError::TypeError(format!("think() failed after {} retries: {}", config.max_retries, last_error)))
}

/// Parse a provider-specific response into a unified LlmResponse
pub fn parse_response(config: &LlmConfig, json: &serde_json::Value) -> LlmResponse {
    let (content, finish_reason, tool_calls, tokens) = if config.provider == "anthropic" {
        parse_anthropic_response(json)
    } else {
        parse_openai_response(json)
    };
    LlmResponse { content, finish_reason, tool_calls, tokens }
}

fn parse_anthropic_response(json: &serde_json::Value) -> (String, String, Vec<ToolCall>, i64) {
    let stop = json["stop_reason"].as_str().unwrap_or("").to_string();
    let usage = &json["usage"];
    let tokens = usage["input_tokens"].as_i64().unwrap_or(0)
        + usage["output_tokens"].as_i64().unwrap_or(0);

    let mut text = String::new();
    let mut tools = Vec::new();

    if let Some(blocks) = json["content"].as_array() {
        for block in blocks {
            match block["type"].as_str() {
                Some("text") => {
                    if let Some(t) = block["text"].as_str() { text = t.to_string(); }
                }
                Some("tool_use") => {
                    tools.push(ToolCall {
                        id: block["id"].as_str().unwrap_or("").to_string(),
                        name: block["name"].as_str().unwrap_or("").to_string(),
                        arguments_json: block["input"].to_string(),
                    });
                }
                _ => {}
            }
        }
    }
    (text, stop, tools, tokens)
}

fn parse_openai_response(json: &serde_json::Value) -> (String, String, Vec<ToolCall>, i64) {
    let choice = &json["choices"][0];
    let msg = &choice["message"];
    let finish_reason = choice["finish_reason"].as_str().unwrap_or("").to_string();
    let content = msg["content"].as_str().unwrap_or("").to_string();
    let tokens = json["usage"]["total_tokens"].as_i64().unwrap_or(0);

    let mut tools = Vec::new();
    if let Some(tc_array) = msg["tool_calls"].as_array() {
        for tc in tc_array {
            tools.push(ToolCall {
                id: tc["id"].as_str().unwrap_or("").to_string(),
                name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                arguments_json: tc["function"]["arguments"].as_str().unwrap_or("{}").to_string(),
            });
        }
    }
    (content, finish_reason, tools, tokens)
}

/// Add tool result to conversation in provider-specific format
pub fn push_tool_result(
    config: &LlmConfig,
    conversation: &mut Vec<serde_json::Value>,
    tool_id: &str,
    result: &str,
) {
    if config.provider == "anthropic" {
        conversation.push(serde_json::json!({
            "role": "user",
            "content": [{"type": "tool_result", "tool_use_id": tool_id, "content": result}],
        }));
    } else {
        conversation.push(serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_id,
            "content": result,
        }));
    }
}

/// Add assistant message with tool calls to conversation
pub fn push_assistant_tool_message(
    config: &LlmConfig,
    conversation: &mut Vec<serde_json::Value>,
    raw_json: &serde_json::Value,
) {
    if config.provider == "anthropic" {
        conversation.push(serde_json::json!({"role": "assistant", "content": raw_json["content"]}));
    } else {
        conversation.push(raw_json["choices"][0]["message"].clone());
    }
}

/// Create a trace entry for a think() call
pub fn trace_think(iteration: i64, prompt: &str, tokens: i64, total: i64, finish: &str) -> Value {
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs() as i64;
    map_from_pairs(vec![
        ("event".to_string(), Value::String("think".to_string())),
        ("iteration".to_string(), Value::Int(SomaInt::from_i64(iteration))),
        ("prompt".to_string(), Value::String(prompt.to_string())),
        ("tokens".to_string(), Value::Int(SomaInt::from_i64(tokens))),
        ("total_tokens".to_string(), Value::Int(SomaInt::from_i64(total))),
        ("finish_reason".to_string(), Value::String(finish.to_string())),
        ("timestamp".to_string(), Value::Int(SomaInt::from_i64(ts))),
    ])
}

/// Create a trace entry for a tool call
pub fn trace_tool_call(name: &str, args: &str, result: &str) -> Value {
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs() as i64;
    map_from_pairs(vec![
        ("event".to_string(), Value::String("tool_call".to_string())),
        ("tool".to_string(), Value::String(name.to_string())),
        ("args".to_string(), Value::String(args.to_string())),
        ("result".to_string(), Value::String(result.to_string())),
        ("timestamp".to_string(), Value::Int(SomaInt::from_i64(ts))),
    ])
}
