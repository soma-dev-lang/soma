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
        _ => None,
    }
}
