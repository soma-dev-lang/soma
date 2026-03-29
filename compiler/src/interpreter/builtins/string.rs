use super::super::{Value, RuntimeError};
use super::json_to_value;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "concat" => {
            if args.len() >= 2 {
                match (&args[0], &args[1]) {
                    (Value::String(a), Value::String(b)) => {
                        let mut result = String::with_capacity(a.len() + b.len());
                        result.push_str(a);
                        result.push_str(b);
                        Some(Ok(Value::String(result)))
                    }
                    _ => Some(Ok(Value::String(format!("{}{}", args[0], args[1]))))
                }
            } else if args.len() == 1 {
                Some(Ok(args[0].clone()))
            } else {
                Some(Err(RuntimeError::TypeError("concat expects arguments".to_string())))
            }
        }
        "split" => {
            if args.len() >= 2 {
                if let (Value::String(s), Value::String(delim)) = (&args[0], &args[1]) {
                    let parts: Vec<Value> = s.split(delim.as_str())
                        .map(|p| Value::String(p.to_string()))
                        .collect();
                    Some(Ok(Value::List(parts)))
                } else {
                    Some(Err(RuntimeError::TypeError("split expects (string, delimiter)".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("split expects 2 arguments".to_string())))
            }
        }
        "replace" => {
            if args.len() >= 3 {
                if let (Value::String(s), Value::String(old), Value::String(new)) = (&args[0], &args[1], &args[2]) {
                    Some(Ok(Value::String(s.replace(old.as_str(), new.as_str()))))
                } else {
                    Some(Err(RuntimeError::TypeError("replace expects strings".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("replace expects 3 arguments".to_string())))
            }
        }
        "contains" => {
            if args.len() >= 2 {
                if let (Value::String(haystack), Value::String(needle)) = (&args[0], &args[1]) {
                    Some(Ok(Value::Bool(haystack.contains(needle.as_str()))))
                } else {
                    Some(Ok(Value::Bool(false)))
                }
            } else {
                Some(Err(RuntimeError::TypeError("contains(string, substring)".to_string())))
            }
        }
        "starts_with" => {
            if args.len() >= 2 {
                if let (Value::String(s), Value::String(prefix)) = (&args[0], &args[1]) {
                    Some(Ok(Value::Bool(s.starts_with(prefix.as_str()))))
                } else {
                    Some(Err(RuntimeError::TypeError("starts_with expects strings".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("starts_with expects 2 arguments".to_string())))
            }
        }
        "ends_with" => {
            if args.len() >= 2 {
                if let (Value::String(s), Value::String(suffix)) = (&args[0], &args[1]) {
                    Some(Ok(Value::Bool(s.ends_with(suffix.as_str()))))
                } else {
                    Some(Err(RuntimeError::TypeError("ends_with expects strings".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("ends_with expects 2 arguments".to_string())))
            }
        }
        "lowercase" | "to_lower" => {
            args.first().map(|a| {
                if let Value::String(s) = a {
                    Ok(Value::String(s.to_lowercase()))
                } else {
                    Ok(Value::String(format!("{}", a)))
                }
            })
        }
        "uppercase" | "to_upper" => {
            args.first().map(|a| {
                if let Value::String(s) = a {
                    Ok(Value::String(s.to_uppercase()))
                } else {
                    Ok(Value::String(format!("{}", a)))
                }
            })
        }
        "trim" => {
            args.first().map(|arg| {
                if let Value::String(s) = arg {
                    Ok(Value::String(s.trim().to_string()))
                } else {
                    Err(RuntimeError::TypeError("trim expects a string".to_string()))
                }
            })
        }
        "index_of" => {
            if args.len() >= 2 {
                if let (Value::String(s), Value::String(sub)) = (&args[0], &args[1]) {
                    Some(Ok(match s.find(sub.as_str()) {
                        Some(byte_pos) => {
                            // Convert byte offset to char offset
                            let char_pos = s[..byte_pos].chars().count();
                            Value::Int(char_pos as i64)
                        }
                        None => Value::Int(-1),
                    }))
                } else {
                    Some(Ok(Value::Int(-1)))
                }
            } else {
                Some(Err(RuntimeError::TypeError("index_of(string, substring)".to_string())))
            }
        }
        "substring" | "substr" => {
            if args.len() >= 3 {
                if let (Value::String(s), Value::Int(start), Value::Int(end)) = (&args[0], &args[1], &args[2]) {
                    let start = (*start).max(0) as usize;
                    let char_count = s.chars().count();
                    let end = (*end).min(char_count as i64) as usize;
                    let result: String = s.chars().skip(start).take(end.saturating_sub(start)).collect();
                    Some(Ok(Value::String(result)))
                } else {
                    Some(Ok(Value::Unit))
                }
            } else {
                Some(Err(RuntimeError::TypeError("substring(string, start, end)".to_string())))
            }
        }
        "len" => {
            args.first().map(|arg| match arg {
                Value::String(s) => Ok(Value::Int(s.chars().count() as i64)),
                Value::List(items) => Ok(Value::Int(items.len() as i64)),
                Value::Map(entries) => Ok(Value::Int(entries.len() as i64)),
                _ => Err(RuntimeError::TypeError("len expects a string, list, or map".to_string())),
            })
        }
        "to_string" => {
            args.first().map(|arg| Ok(Value::String(format!("{}", arg))))
        }
        "to_int" | "int" => {
            args.first().map(|arg| match arg {
                Value::Int(n) => Ok(Value::Int(*n)),
                Value::Float(n) => Ok(Value::Int(*n as i64)),
                Value::String(s) => {
                    if let Ok(n) = s.parse::<i64>() {
                        Ok(Value::Int(n))
                    } else if let Ok(f) = s.parse::<f64>() {
                        Ok(Value::Int(f as i64))
                    } else {
                        Ok(Value::Unit)
                    }
                },
                Value::Bool(b) => Ok(Value::Int(if *b { 1 } else { 0 })),
                _ => Ok(Value::Unit),
            })
        }
        "to_float" | "float" => {
            args.first().map(|arg| match arg {
                Value::Float(n) => Ok(Value::Float(*n)),
                Value::Int(n) => Ok(Value::Float(*n as f64)),
                Value::String(s) => {
                    if let Ok(f) = s.parse::<f64>() {
                        Ok(Value::Float(f))
                    } else {
                        Ok(Value::Unit)
                    }
                },
                _ => Ok(Value::Unit),
            })
        }
        "to_json" => {
            args.first().map(|arg| Ok(Value::String(format!("{}", arg))))
        }
        "from_json" => {
            args.first().map(|arg| {
                match arg {
                    Value::String(s) => Ok(json_to_value(s)),
                    Value::Map(_) | Value::List(_) => Ok(arg.clone()),
                    Value::Unit => Ok(Value::Unit),
                    other => Ok(Value::String(format!("{}", other))),
                }
            })
        }
        "type_of" => {
            args.first().map(|arg| {
                let t = match arg {
                    Value::Int(_) => "Int",
                    Value::Big(_) => "BigInt",
                    Value::Float(_) => "Float",
                    Value::String(_) => "String",
                    Value::Bool(_) => "Bool",
                    Value::List(_) => "List",
                    Value::Map(_) => "Map",
                    Value::Lambda { .. } | Value::LambdaBlock { .. } => "Lambda",
                    Value::Unit => "Unit",
                };
                Ok(Value::String(t.to_string()))
            })
        }
        "escape_html" | "html_escape" => {
            if let Some(Value::String(s)) = args.first() {
                let escaped = s
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;")
                    .replace('\'', "&#39;");
                Some(Ok(Value::String(escaped)))
            } else {
                Some(Err(RuntimeError::TypeError("escape_html(string)".to_string())))
            }
        }
        _ => None,
    }
}
