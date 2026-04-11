use super::super::{Value, RuntimeError, map_from_pairs};
use super::serde_json_to_value;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "http_get" => {
            if let Some(Value::String(url)) = args.first() {
                // Extract optional options map as second arg
                let opts = args.get(1).and_then(|v| if let Value::Map(m) = v { Some(m) } else { None });
                let max_bytes = opts.and_then(|m| m.get("max_bytes")).and_then(|v| {
                    if let Value::Int(n) = v { let v = n.to_i64().unwrap_or(0); if v > 0 { Some(v as usize) } else { None } } else { None }
                });
                let timeout_ms = opts.and_then(|m| m.get("timeout")).and_then(|v| {
                    if let Value::Int(n) = v { let v = n.to_i64().unwrap_or(0); if v > 0 { Some(v as u64) } else { None } } else { None }
                });

                let result = if let Some(tms) = timeout_ms {
                    ureq::AgentBuilder::new()
                        .timeout(std::time::Duration::from_millis(tms))
                        .build()
                        .get(url)
                        .call()
                } else {
                    ureq::get(url).call()
                };

                match result {
                    Ok(resp) => {
                        let mut body = resp.into_string().unwrap_or_default();
                        if let Some(mb) = max_bytes {
                            if body.len() > mb {
                                body.truncate(mb);
                            }
                        }
                        if body.starts_with('{') || body.starts_with('[') {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                                Some(Ok(serde_json_to_value(&v)))
                            } else {
                                Some(Ok(Value::String(body)))
                            }
                        } else {
                            Some(Ok(Value::String(body)))
                        }
                    }
                    Err(e) => Some(Ok(map_from_pairs(vec![
                        ("error".to_string(), Value::String(format!("{}", e))),
                    ])))
                }
            } else {
                Some(Err(RuntimeError::TypeError("http_get(url)".to_string())))
            }
        }
        "http_post" => {
            if args.len() >= 2 {
                if let Value::String(url) = &args[0] {
                    let body = format!("{}", args[1]);
                    match ureq::post(url)
                        .set("Content-Type", "application/json")
                        .send_string(&body)
                    {
                        Ok(resp) => {
                            let text = resp.into_string().unwrap_or_default();
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                                Some(Ok(serde_json_to_value(&v)))
                            } else {
                                Some(Ok(Value::String(text)))
                            }
                        }
                        Err(e) => Some(Ok(map_from_pairs(vec![
                            ("error".to_string(), Value::String(format!("{}", e))),
                        ])))
                    }
                } else {
                    Some(Err(RuntimeError::TypeError("http_post(url, body)".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("http_post(url, body)".to_string())))
            }
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
