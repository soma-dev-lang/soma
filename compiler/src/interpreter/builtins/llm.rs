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
    /// Hard cap on one HTTP round-trip to the provider. Handlers run one
    /// at a time: a provider that hangs used to hang the whole service.
    pub timeout_ms: u64,
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

/// Build the HTTP request body for the given provider.
/// If `max_tokens_override` is Some, it replaces the default 2048.
pub fn build_request_body(
    config: &LlmConfig,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    json_mode: bool,
    max_tokens_override: Option<u64>,
) -> serde_json::Value {
    let mt = max_tokens_override.unwrap_or(2048);
    if config.provider == "anthropic" {
        let msgs: Vec<serde_json::Value> = messages.iter()
            .filter(|m| m["role"].as_str() != Some("system"))
            .cloned().collect();
        let mut body = serde_json::json!({
            "model": config.model,
            "messages": msgs,
            "max_tokens": mt,
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
            "max_tokens": mt,
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
    // `timeout` bounds the WHOLE call, retries and back-off included: the
    // proven latency bound counted one timeout while three retries of a
    // failing provider took 6.7 s against a declared 1 s (and a floor of
    // 1 s made `timeout: 900` wait 1000)
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(config.timeout_ms.max(1) as u64);

    for retry in 0..=config.max_retries {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(RuntimeError::TypeError(format!("think() request timed out after {} ms (the timeout covers retries): {}", config.timeout_ms, last_error)));
        }
        let mut req = ureq::post(&config.api_url)
            .timeout(left)
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
                // not a timeout: the provider may still be generating (and
                // billing) that reply — a retry made one think() cost up to
                // 4 × max_tokens past the proven bound, and held the handler
                // lock 4 × timeout
                let timed_out = { let l = last_error.to_lowercase(); l.contains("timed out") || l.contains("timeout") };
                let retryable = !timed_out && ["429", "500", "502", "503", "529"]
                    .iter().any(|code| last_error.contains(code));
                let delay = std::time::Duration::from_millis(500 * (1 << retry));
                if last_error.contains("429") { limiter::pause(delay); }
                if retryable && retry < config.max_retries && std::time::Instant::now() + delay < deadline {
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
                    // every text block counts (the last one alone was kept, and
                    // measured, so the rest passed max_tokens unseen)
                    if let Some(t) = block["text"].as_str() { text.push_str(t); }
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
    // `content` may be a list of parts (`[{"type":"text","text":…}]`, as
    // some OpenAI-compatible servers send): it read as "" — never measured
    // against max_tokens — and think() returned the raw JSON instead
    let content = match &msg["content"] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str().or_else(|| p.as_str())).collect::<Vec<_>>().concat(),
        _ => String::new(),
    };
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

/// V1.6: trace entries are now sum-type variants. The user matches on
/// `Think { .. } | ToolCall { .. } | Approval { .. }` exhaustively.
fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn variant_struct(name: &str, fields: Vec<(&str, Value)>) -> Value {
    use crate::interpreter::VariantValue;
    use indexmap::IndexMap;
    let mut m = IndexMap::new();
    for (k, v) in fields { m.insert(k.to_string(), v); }
    Value::Variant {
        type_name: "TraceStep".to_string(),
        variant: name.to_string(),
        fields: VariantValue::Struct(m),
    }
}

/// Create a trace entry for a think() call
pub fn trace_think(iteration: i64, prompt: &str, tokens: i64, total: i64, finish: &str) -> Value {
    trace_think_with(iteration, prompt, "", tokens, total, finish)
}

/// A Think step with its system prompt (an audit of "what did the model
/// see" needs both).
pub fn trace_think_with(iteration: i64, prompt: &str, system: &str, tokens: i64, total: i64, finish: &str) -> Value {
    variant_struct("Think", vec![
        ("iteration", Value::Int(SomaInt::from_i64(iteration))),
        ("prompt", Value::String(prompt.to_string())),
        ("system", Value::String(system.to_string())),
        ("tokens", Value::Int(SomaInt::from_i64(tokens))),
        ("total_tokens", Value::Int(SomaInt::from_i64(total))),
        ("finish_reason", Value::String(finish.to_string())),
        ("timestamp", Value::Int(SomaInt::from_i64(now_ts()))),
    ])
}

/// Create a trace entry for a tool call. Field is `name` (not `tool`),
/// because `tool` is a face-block keyword and would clash in match patterns.
pub fn trace_tool_call(name: &str, args: &str, result: &str) -> Value {
    variant_struct("ToolCall", vec![
        ("name", Value::String(name.to_string())),
        ("args", Value::String(args.to_string())),
        ("result", Value::String(result.to_string())),
        ("timestamp", Value::Int(SomaInt::from_i64(now_ts()))),
    ])
}

/// Create a trace entry for an approval gate
pub fn trace_approval(action: &str, result: &str) -> Value {
    variant_struct("Approval", vec![
        ("action", Value::String(action.to_string())),
        ("result", Value::String(result.to_string())),
        ("timestamp", Value::Int(SomaInt::from_i64(now_ts()))),
    ])
}

/// Create a trace entry for a state transition
pub fn trace_transition(instance: &str, from: &str, to: &str) -> Value {
    variant_struct("Transition", vec![
        ("instance", Value::String(instance.to_string())),
        ("from", Value::String(from.to_string())),
        ("to", Value::String(to.to_string())),
        ("timestamp", Value::Int(SomaInt::from_i64(now_ts()))),
    ])
}


/// Provider rate limits (`[agent] rpm` / `tpm`, or SOMA_LLM_RPM /
/// SOMA_LLM_TPM): token buckets shared by every thread of the process, so
/// 500 horde workers stay under the provider's quota; a 429 makes every
/// caller wait, not only the one that got it.
pub mod limiter {
    //! Exact sliding windows: at most `rpm` requests and `tpm` tokens in ANY
    //! 60 s window (a token bucket that starts full let 2 × rpm through in
    //! the first minute).
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    struct Window { reqs: VecDeque<Instant>, toks: VecDeque<(Instant, f64)>, tok_sum: f64, paused_until: Option<Instant> }
    static W: Mutex<Option<Window>> = Mutex::new(None);
    const MINUTE: Duration = Duration::from_secs(60);

    fn limits(cfg_rpm: u64, cfg_tpm: u64) -> (f64, f64) {
        let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<u64>().ok());
        (env("SOMA_LLM_RPM").unwrap_or(cfg_rpm) as f64, env("SOMA_LLM_TPM").unwrap_or(cfg_tpm) as f64)
    }

    /// Wait until one request of ~`tokens` tokens fits both limits.
    pub fn acquire(cfg_rpm: u64, cfg_tpm: u64, tokens: u64) {
        let (rpm, tpm) = limits(cfg_rpm, cfg_tpm);
        if rpm <= 0.0 && tpm <= 0.0 { return; }
        loop {
            let wait = {
                let mut g = W.lock().unwrap_or_else(|e| e.into_inner());
                let w = g.get_or_insert_with(|| Window { reqs: VecDeque::new(), toks: VecDeque::new(), tok_sum: 0.0, paused_until: None });
                let now = Instant::now();
                while w.reqs.front().map_or(false, |t| now.duration_since(*t) >= MINUTE) { w.reqs.pop_front(); }
                while w.toks.front().map_or(false, |(t, _)| now.duration_since(*t) >= MINUTE) { let (_, n) = w.toks.pop_front().unwrap(); w.tok_sum -= n; }
                // a request larger than a whole minute's tokens waits for an
                // empty window, then goes
                let need = if tpm > 0.0 { (tokens as f64).min(tpm) } else { 0.0 };
                match w.paused_until {
                    Some(t) if t > now => Some(t - now),
                    _ => {
                        let ok_r = rpm <= 0.0 || (w.reqs.len() as f64) < rpm;
                        let ok_t = tpm <= 0.0 || w.tok_sum + need <= tpm;
                        if ok_r && ok_t {
                            if rpm > 0.0 { w.reqs.push_back(now); }
                            if tpm > 0.0 { w.toks.push_back((now, need)); w.tok_sum += need; }
                            None
                        } else {
                            let wr = if ok_r { Duration::ZERO } else { w.reqs.front().map_or(Duration::ZERO, |t| (*t + MINUTE).saturating_duration_since(now)) };
                            let wt = if ok_t { Duration::ZERO } else {
                                // until enough old tokens leave the window
                                let mut left = w.tok_sum + need - tpm;
                                let mut until = Duration::ZERO;
                                for (t, n) in w.toks.iter() { left -= n; until = (*t + MINUTE).saturating_duration_since(now); if left <= 0.0 { break; } }
                                until
                            };
                            Some(wr.max(wt).max(Duration::from_millis(1)))
                        }
                    }
                }
            };
            match wait { None => return, Some(d) => std::thread::sleep(d.min(Duration::from_secs(5))) }
        }
    }

    /// A 429: every caller pauses for `d`.
    pub fn pause(d: Duration) {
        let mut g = W.lock().unwrap_or_else(|e| e.into_inner());
        let w = g.get_or_insert_with(|| Window { reqs: VecDeque::new(), toks: VecDeque::new(), tok_sum: 0.0, paused_until: None });
        let until = Instant::now() + d;
        if w.paused_until.map_or(true, |t| t < until) { w.paused_until = Some(until); }
    }
}
