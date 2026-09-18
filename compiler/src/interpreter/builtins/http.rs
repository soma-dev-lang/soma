use super::super::{Value, RuntimeError, map_from_pairs};
use super::serde_json_to_value;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "http_get" => {
            let Some(Value::String(url)) = args.first() else {
                return Some(Err(RuntimeError::TypeError("http_get(url: String, opts?: map(\"timeout\", ms, \"max_bytes\", n, \"headers\", map(...)))".to_string())));
            };
            let opts = match args.get(1) {
                None | Some(Value::Unit) => None,
                Some(Value::Map(m)) => Some(m),
                Some(other) => return Some(Err(RuntimeError::TypeError(format!("http_get(url, opts): opts must be a map(\"timeout\", ms, …), got {}", super::super::value_type_name(other))))),
            };
            Some(http_call("GET", url, None, opts))
        }
        "http_post" | "http_put" | "http_patch" | "http_delete" => {
            let method = match name { "http_post" => "POST", "http_put" => "PUT", "http_patch" => "PATCH", _ => "DELETE" };
            let Some(Value::String(url)) = args.first() else {
                return Some(Err(RuntimeError::TypeError(format!("{}(url: String, body, opts?)", name))));
            };
            if method != "DELETE" && args.len() < 2 {
                return Some(Err(RuntimeError::TypeError(format!("{}(url, body, opts?) — the body is required (map() for none)", name))));
            }
            let (body, opts_arg) = if method == "DELETE" { (args.get(1).filter(|v| !matches!(v, Value::Map(_))), args.get(1).filter(|v| matches!(v, Value::Map(_)))) } else { (args.get(1), args.get(2)) };
            let opts = match opts_arg {
                None | Some(Value::Unit) => None,
                Some(Value::Map(m)) => Some(m),
                Some(other) => return Some(Err(RuntimeError::TypeError(format!("{}(url, body, opts): opts must be a map(\"timeout\", ms, …), got {}", name, super::super::value_type_name(other))))),
            };
            let text = body.map(|b| match b {
                Value::String(s) => s.clone(),
                other => super::string::to_json_string(other),
            });
            Some(http_call(method, url, text, opts))
        }
        "ws_connect" => {
            // ws_connect(url) — open a WS connection, return connection ID
            // For now, synchronous: connect, return a map with the connection
            // The real async handling happens via the event bus
            if let Some(Value::String(url)) = args.first() {
                match tungstenite::connect(url) {
                    Ok((mut ws, _response)) => {
                        // Read messages in a background thread, push to stdout for now
                        // In a full implementation, this would feed into the event bus
                        // For now, return success
                        let _ = ws.close(None);
                        Some(Ok(map_from_pairs(vec![
                            ("status".to_string(), Value::String("connected".to_string())),
                            ("url".to_string(), Value::String(url.clone())),
                        ])))
                    }
                    Err(e) => Some(Ok(map_from_pairs(vec![
                        ("error".to_string(), Value::String(format!("{}", e))),
                    ])))
                }
            } else {
                Some(Err(RuntimeError::TypeError("ws_connect(url)".to_string())))
            }
        }
        _ => None,
    }
}

/// One outbound request. Never raises for the network or a status: the
/// result is the parsed body on 2xx, else a map
/// `{error, kind, status, body}` — kind `http_status` (with the status and
/// the upstream body parsed when it is JSON), `timeout`, `refused` or
/// `network`. The default timeout is 30 s (a hung upstream used to hang the
/// caller forever — and under `soma serve` every other request with it).
fn http_call(method: &str, url: &str, body: Option<String>, opts: Option<&indexmap::IndexMap<String, Value>>) -> Result<Value, RuntimeError> {
    let int_opt = |k: &str| opts.and_then(|m| m.get(k)).and_then(|v| match v {
        Value::Int(n) => n.to_i64().filter(|v| *v > 0).map(|v| v as u64),
        _ => None,
    });
    if let Some(m) = opts {
        for k in m.keys() {
            if !matches!(k.as_str(), "timeout" | "max_bytes" | "headers") {
                return Err(RuntimeError::TypeError(format!("http: unknown option '{}' — the options are timeout (ms), max_bytes, headers", k)));
            }
        }
    }
    let timeout_ms = int_opt("timeout").unwrap_or(30_000);
    let max_bytes = int_opt("max_bytes").map(|v| v as usize);
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_millis(timeout_ms)).build();
    let mut req = agent.request(method, url);
    if body.is_some() { req = req.set("Content-Type", "application/json"); }
    if let Some(Value::Map(h)) = opts.and_then(|m| m.get("headers")) {
        for (k, v) in h {
            let v = match v { Value::String(s) => s.clone(), other => format!("{}", other) };
            req = req.set(k, &v);
        }
    }
    let result = match &body { Some(b) => req.send_string(b), None => req.call() };
    let parse = |mut text: String| -> Value {
        if let Some(mb) = max_bytes { if text.len() > mb { text.truncate(mb); } }
        let t = text.trim_start();
        if t.starts_with('{') || t.starts_with('[') {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) { return serde_json_to_value(&v); }
        }
        Value::String(text)
    };
    let err = |kind: &str, msg: String, status: i64, body: Value| -> Value {
        map_from_pairs(vec![
            ("error".to_string(), Value::String(msg)),
            ("kind".to_string(), Value::String(kind.to_string())),
            ("status".to_string(), Value::Int(crate::interpreter::soma_int::SomaInt::from_i64(status))),
            ("body".to_string(), body),
        ])
    };
    Ok(match result {
        Ok(resp) => parse(resp.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, resp)) => {
            let b = parse(resp.into_string().unwrap_or_default());
            err("http_status", format!("{} {}: status code {}", method, url, code), code as i64, b)
        }
        Err(ureq::Error::Transport(t)) => {
            let msg = format!("{} {}: {}", method, url, t);
            let lower = msg.to_lowercase();
            let kind = if lower.contains("timed out") || lower.contains("timeout") { "timeout" }
                else if lower.contains("refused") { "refused" }
                else { "network" };
            err(kind, msg, 0, Value::Unit)
        }
    })
}
