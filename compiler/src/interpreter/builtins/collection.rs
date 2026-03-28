use super::super::{Value, RuntimeError};
use std::collections::HashMap;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "list" => {
            if let Some(Value::List(existing)) = args.first() {
                let mut result = existing.clone();
                result.extend(args[1..].to_vec());
                Some(Ok(Value::List(result)))
            } else {
                Some(Ok(Value::List(args.to_vec())))
            }
        }
        "map" => {
            let mut entries = Vec::with_capacity(args.len() / 2);
            let mut i = 0;
            while i + 1 < args.len() {
                let key = match &args[i] {
                    Value::String(s) => s.clone(),
                    other => format!("{}", other),
                };
                let val = args[i + 1].clone();
                entries.push((key, val));
                i += 2;
            }
            Some(Ok(Value::Map(entries)))
        }
        "push" | "append" => {
            if args.len() >= 2 {
                if let Value::List(existing) = &args[0] {
                    let mut result = existing.clone();
                    result.extend(args[1..].to_vec());
                    Some(Ok(Value::List(result)))
                } else {
                    Some(Ok(Value::List(args.to_vec())))
                }
            } else {
                Some(Ok(Value::List(args.to_vec())))
            }
        }
        "with" => {
            if let Some(Value::Map(entries)) = args.first() {
                let mut result = entries.clone();
                let mut i = 1;
                while i + 1 < args.len() {
                    let key = format!("{}", args[i]);
                    let val = args[i + 1].clone();
                    if let Some(entry) = result.iter_mut().find(|(k, _)| k == &key) {
                        entry.1 = val;
                    } else {
                        result.push((key, val));
                    }
                    i += 2;
                }
                Some(Ok(Value::Map(result)))
            } else {
                Some(Err(RuntimeError::TypeError("with expects (map, key, value)".to_string())))
            }
        }
        "without" => {
            if let Some(Value::Map(entries)) = args.first() {
                let keys_to_remove: Vec<String> = args[1..].iter().map(|a| format!("{}", a)).collect();
                let result: Vec<(String, Value)> = entries.iter()
                    .filter(|(k, _)| !keys_to_remove.contains(k))
                    .cloned().collect();
                Some(Ok(Value::Map(result)))
            } else {
                Some(Err(RuntimeError::TypeError("without expects (map, keys...)".to_string())))
            }
        }
        "merge" => {
            if args.len() >= 2 {
                if let (Value::Map(a), Value::Map(b)) = (&args[0], &args[1]) {
                    let mut result = a.clone();
                    for (key, val) in b {
                        if let Some(entry) = result.iter_mut().find(|(k, _)| k == key) {
                            entry.1 = val.clone();
                        } else {
                            result.push((key.clone(), val.clone()));
                        }
                    }
                    Some(Ok(Value::Map(result)))
                } else { Some(Ok(args[0].clone())) }
            } else {
                Some(Err(RuntimeError::TypeError("merge expects (map1, map2)".to_string())))
            }
        }
        "join" => {
            // Smart dispatch: join(list, list, key) → data join, join(list, sep) → string join
            if args.len() >= 3 {
                if let (Value::List(_), Value::List(_)) = (&args[0], &args[1]) {
                    return call_builtin("inner_join", args);
                }
            }
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let sep = format!("{}", args[1]);
                    let parts: Vec<String> = items.iter().map(|v| format!("{}", v)).collect();
                    Some(Ok(Value::String(parts.join(&sep))))
                } else if let Value::List(items) = &args[0] {
                    let parts: Vec<String> = items.iter().map(|v| format!("{}", v)).collect();
                    Some(Ok(Value::String(parts.join(""))))
                } else {
                    Some(Ok(Value::String(format!("{}", args[0]))))
                }
            } else if let Some(Value::List(items)) = args.first() {
                let parts: Vec<String> = items.iter().map(|v| format!("{}", v)).collect();
                Some(Ok(Value::String(parts.join(""))))
            } else {
                Some(Err(RuntimeError::TypeError("join expects a list".to_string())))
            }
        }
        "flatten" => {
            if let Some(Value::List(items)) = args.first() {
                let result: Vec<Value> = items.iter().flat_map(|item| {
                    if let Value::List(inner) = item { inner.clone() } else { vec![item.clone()] }
                }).collect();
                Some(Ok(Value::List(result)))
            } else { Some(Ok(Value::List(vec![]))) }
        }
        "zip" => {
            if args.len() >= 2 {
                if let (Value::List(a), Value::List(b)) = (&args[0], &args[1]) {
                    let result: Vec<Value> = a.iter().zip(b.iter()).map(|(l, r)| {
                        Value::Map(vec![("left".to_string(), l.clone()), ("right".to_string(), r.clone())])
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(Value::List(vec![]))) }
            } else { Some(Err(RuntimeError::TypeError("zip expects (list, list)".to_string()))) }
        }
        "enumerate" => {
            if let Some(Value::List(items)) = args.first() {
                let result: Vec<Value> = items.iter().enumerate().map(|(i, v)| {
                    Value::Map(vec![("index".to_string(), Value::Int(i as i64)), ("value".to_string(), v.clone())])
                }).collect();
                Some(Ok(Value::List(result)))
            } else { Some(Ok(Value::List(vec![]))) }
        }
        "render_each" => {
            if args.len() >= 2 {
                if let (Value::List(items), Value::String(template)) = (&args[0], &args[1]) {
                    let mut result = String::with_capacity(template.len() * items.len());
                    for item in items {
                        if let Value::Map(entries) = item {
                            let vars: HashMap<String, String> = entries.iter()
                                .map(|(k, v)| (k.clone(), format!("{}", v)))
                                .collect();
                            let bytes = template.as_bytes();
                            let mut pos = 0;
                            while pos < bytes.len() {
                                if bytes[pos] == b'{' {
                                    if let Some(end) = template[pos+1..].find('}') {
                                        let key = &template[pos+1..pos+1+end];
                                        if let Some(val) = vars.get(key) {
                                            result.push_str(val);
                                            pos = pos + 1 + end + 1;
                                            continue;
                                        }
                                    }
                                }
                                result.push(bytes[pos] as char);
                                pos += 1;
                            }
                        }
                    }
                    Some(Ok(Value::String(result)))
                } else {
                    Some(Err(RuntimeError::TypeError("render_each expects (list, template)".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("render_each expects 2 arguments".to_string())))
            }
        }
        // JOIN operations
        "inner_join" => {
            if args.len() >= 3 {
                if let (Value::List(left), Value::List(right)) = (&args[0], &args[1]) {
                    let key = format!("{}", args[2]);
                    let result: Vec<Value> = left.iter().filter_map(|l| {
                        let lk = if let Value::Map(e) = l { e.iter().find(|(k,_)| k == &key).map(|(_,v)| format!("{}", v)) } else { None };
                        lk.and_then(|lk_val| {
                            right.iter().find(|r| {
                                if let Value::Map(e) = r { e.iter().any(|(k,v)| k == &key && format!("{}", v) == lk_val) } else { false }
                            }).map(|r| {
                                let mut merged = if let Value::Map(e) = l { e.clone() } else { vec![] };
                                if let Value::Map(re) = r {
                                    for (rk, rv) in re {
                                        if rk != &key && !merged.iter().any(|(mk,_)| mk == rk) {
                                            merged.push((rk.clone(), rv.clone()));
                                        }
                                    }
                                }
                                Value::Map(merged)
                            })
                        })
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Err(RuntimeError::TypeError("join expects (list, list, key)".to_string()))) }
            } else { Some(Err(RuntimeError::TypeError("join expects (list, list, key)".to_string()))) }
        }
        "left_join" => {
            if args.len() >= 3 {
                if let (Value::List(left), Value::List(right)) = (&args[0], &args[1]) {
                    let key = format!("{}", args[2]);
                    let result: Vec<Value> = left.iter().map(|l| {
                        let lk = if let Value::Map(e) = l { e.iter().find(|(k,_)| k == &key).map(|(_,v)| format!("{}", v)) } else { None };
                        let r_match = lk.and_then(|lk_val| {
                            right.iter().find(|r| {
                                if let Value::Map(e) = r { e.iter().any(|(k,v)| k == &key && format!("{}", v) == lk_val) } else { false }
                            })
                        });
                        let mut merged = if let Value::Map(e) = l { e.clone() } else { vec![] };
                        if let Some(Value::Map(re)) = r_match {
                            for (rk, rv) in re {
                                if rk != &key && !merged.iter().any(|(mk,_)| mk == rk) {
                                    merged.push((rk.clone(), rv.clone()));
                                }
                            }
                        }
                        Value::Map(merged)
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Err(RuntimeError::TypeError("left_join expects (list, list, key)".to_string()))) }
            } else { Some(Err(RuntimeError::TypeError("left_join expects (list, list, key)".to_string()))) }
        }
        "_coalesce" => {
            if args.len() >= 2 {
                Some(Ok(if matches!(args[0], Value::Unit) { args[1].clone() } else { args[0].clone() }))
            } else {
                Some(Ok(args.first().cloned().unwrap_or(Value::Unit)))
            }
        }
        _ => None,
    }
}
