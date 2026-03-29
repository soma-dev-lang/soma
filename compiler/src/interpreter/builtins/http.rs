use super::super::{Value, RuntimeError};
use super::serde_json_to_value;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "http_get" | "fetch" => {
            if let Some(Value::String(url)) = args.first() {
                match ureq::get(url).call() {
                    Ok(resp) => {
                        let body = resp.into_string().unwrap_or_default();
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
                    Err(e) => Some(Ok(Value::Map(vec![
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
                        Err(e) => Some(Ok(Value::Map(vec![
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
                        Some(Ok(Value::Map(vec![
                            ("status".to_string(), Value::String("connected".to_string())),
                            ("url".to_string(), Value::String(url.clone())),
                        ])))
                    }
                    Err(e) => Some(Ok(Value::Map(vec![
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
