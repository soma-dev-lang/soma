use super::super::{Value, RuntimeError};
use std::collections::HashMap;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "print" => {
            for (i, arg) in args.iter().enumerate() {
                if i > 0 { print!(" "); }
                print!("{}", arg);
            }
            println!();
            Some(Ok(Value::Unit))
        }
        "load_template" | "load" | "include" => {
            if let Some(Value::String(path)) = args.first() {
                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        if args.len() > 1 {
                            let mut result = content;
                            let mut i = 1;
                            while i + 1 < args.len() {
                                let key = format!("{}", args[i]);
                                let val = format!("{}", args[i + 1]);
                                result = result.replace(&format!("{{{}}}", key), &val);
                                i += 2;
                            }
                            Some(Ok(Value::String(result)))
                        } else {
                            Some(Ok(Value::String(content)))
                        }
                    }
                    Err(e) => Some(Err(RuntimeError::TypeError(format!("cannot load '{}': {}", path, e)))),
                }
            } else {
                Some(Err(RuntimeError::TypeError("load_template expects a file path string".to_string())))
            }
        }
        "render" => {
            if let Some(Value::String(template)) = args.first() {
                let mut vars: HashMap<String, String> = HashMap::new();
                let mut i = 1;
                while i + 1 < args.len() {
                    let key = format!("{}", args[i]);
                    let val = format!("{}", args[i + 1]);
                    vars.insert(key, val);
                    i += 2;
                }
                let mut result = String::with_capacity(template.len());
                let mut pos = 0;
                while pos < template.len() {
                    if template.as_bytes()[pos] == b'{' {
                        if let Some(end) = template[pos+1..].find('}') {
                            let key = &template[pos+1..pos+1+end];
                            if let Some(val) = vars.get(key) {
                                result.push_str(val);
                                pos = pos + 1 + end + 1;
                                continue;
                            }
                        }
                    }
                    if let Some(c) = template[pos..].chars().next() {
                        result.push(c);
                        pos += c.len_utf8();
                    } else {
                        pos += 1;
                    }
                }
                Some(Ok(Value::String(result)))
            } else {
                Some(Err(RuntimeError::TypeError("render expects a template string".to_string())))
            }
        }
        "html" => {
            let (status, mut body) = if args.len() >= 2 {
                (args[0].clone(), format!("{}", args[1]))
            } else {
                (Value::Int(200), args.first().map(|a| format!("{}", a)).unwrap_or_default())
            };
            // Only inject HTMX on full pages, not fragments
            let inject_htmx = body.contains("<html") || body.contains("<!DOCTYPE") || body.contains("<!doctype");
            if inject_htmx && body.contains("hx-") && !body.contains("htmx.org") {
                let htmx_tag = "<script src=\"https://unpkg.com/htmx.org@2.0.4\"></script>";
                if let Some(pos) = body.find("</head>") {
                    body.insert_str(pos, htmx_tag);
                } else if let Some(pos) = body.find("<body") {
                    body.insert_str(pos, htmx_tag);
                } else {
                    body = format!("{}{}", htmx_tag, body);
                }
            }
            Some(Ok(Value::Map(vec![
                ("_status".to_string(), status),
                ("_body".to_string(), Value::String(body)),
                ("_content_type".to_string(), Value::String("text/html; charset=utf-8".to_string())),
            ])))
        }
        "response" => {
            let status = args.first().cloned().unwrap_or(Value::Int(200));
            let body = args.get(1).cloned().unwrap_or(Value::Unit);
            let mut entries = vec![
                ("_status".to_string(), status),
                ("_body".to_string(), body),
            ];
            let mut i = 2;
            while i + 1 < args.len() {
                let key = format!("{}", args[i]);
                let val = args[i + 1].clone();
                entries.push((key, val));
                i += 2;
            }
            Some(Ok(Value::Map(entries)))
        }
        "redirect" => {
            let url = args.first().map(|a| format!("{}", a)).unwrap_or("/".to_string());
            Some(Ok(Value::Map(vec![
                ("_status".to_string(), Value::Int(302)),
                ("_body".to_string(), Value::String(String::new())),
                ("Location".to_string(), Value::String(url)),
            ])))
        }
        "sse" => {
            // sse("stream1", "stream2", ...) — returns a marker value
            // The server detects _sse and opens a persistent SSE connection
            let streams: Vec<String> = args.iter().map(|a| format!("{}", a)).collect();
            Some(Ok(Value::Map(vec![
                ("_sse".to_string(), Value::Bool(true)),
                ("_streams".to_string(), Value::List(
                    streams.iter().map(|s| Value::String(s.clone())).collect()
                )),
            ])))
        }
        _ => None,
    }
}
