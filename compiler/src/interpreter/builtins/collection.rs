use super::super::{Value, RuntimeError, map_from_pairs};
use std::collections::HashMap;
use indexmap::IndexMap;
use crate::interpreter::soma_int::SomaInt;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "list" => {
            if args.len() > 1 {
                if let Some(Value::List(existing)) = args.first() {
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
        "map" => {
            if args.len() % 2 != 0 {
                return Some(Err(RuntimeError::TypeError(
                    format!("map() requires an even number of arguments (key-value pairs), got {}", args.len())
                )));
            }
            let mut entries = IndexMap::with_capacity(args.len() / 2);
            let mut i = 0;
            while i + 1 < args.len() {
                let key = match &args[i] {
                    Value::String(s) => s.clone(),
                    other => format!("{}", other),
                };
                let val = args[i + 1].clone();
                entries.insert(key, val);
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
                    result.insert(key, val);
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
                let result: IndexMap<String, Value> = entries.iter()
                    .filter(|(k, _)| !keys_to_remove.contains(k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
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
                        result.insert(key.clone(), val.clone());
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
                        map_from_pairs(vec![("left".to_string(), l.clone()), ("right".to_string(), r.clone())])
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(Value::List(vec![]))) }
            } else { Some(Err(RuntimeError::TypeError("zip expects (list, list)".to_string()))) }
        }
        "enumerate" => {
            if let Some(Value::List(items)) = args.first() {
                let result: Vec<Value> = items.iter().enumerate().map(|(i, v)| {
                    map_from_pairs(vec![("index".to_string(), Value::Int(SomaInt::from_i64(i as i64))), ("value".to_string(), v.clone())])
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
                        let lk = if let Value::Map(e) = l { e.get(&key).map(|v| format!("{}", v)) } else { None };
                        lk.and_then(|lk_val| {
                            right.iter().find(|r| {
                                if let Value::Map(e) = r { e.get(&key).map(|v| format!("{}", v) == lk_val).unwrap_or(false) } else { false }
                            }).map(|r| {
                                let mut merged = if let Value::Map(e) = l { e.clone() } else { IndexMap::new() };
                                if let Value::Map(re) = r {
                                    for (rk, rv) in re {
                                        if rk != &key && !merged.contains_key(rk) {
                                            merged.insert(rk.clone(), rv.clone());
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
                        let lk = if let Value::Map(e) = l { e.get(&key).map(|v| format!("{}", v)) } else { None };
                        let r_match = lk.and_then(|lk_val| {
                            right.iter().find(|r| {
                                if let Value::Map(e) = r { e.get(&key).map(|v| format!("{}", v) == lk_val).unwrap_or(false) } else { false }
                            })
                        });
                        let mut merged = if let Value::Map(e) = l { e.clone() } else { IndexMap::new() };
                        if let Some(Value::Map(re)) = r_match {
                            for (rk, rv) in re {
                                if rk != &key && !merged.contains_key(rk) {
                                    merged.insert(rk.clone(), rv.clone());
                                }
                            }
                        }
                        Value::Map(merged)
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Err(RuntimeError::TypeError("left_join expects (list, list, key)".to_string()))) }
            } else { Some(Err(RuntimeError::TypeError("left_join expects (list, list, key)".to_string()))) }
        }
        "reverse" => {
            if let Some(Value::List(items)) = args.first() {
                let mut result = items.clone();
                result.reverse();
                Some(Ok(Value::List(result)))
            } else {
                Some(Err(RuntimeError::TypeError("reverse expects a list".to_string())))
            }
        }
        "range" => {
            if args.len() >= 2 {
                let start = match &args[0] { Value::Int(si) => si.to_i64().unwrap_or(0), Value::Float(n) => *n as i64, _ => 0 };
                let end = match &args[1] { Value::Int(si) => si.to_i64().unwrap_or(0), Value::Float(n) => *n as i64, _ => 0 };
                let result: Vec<Value> = (start..end).map(|i| Value::Int(SomaInt::from_i64(i))).collect();
                Some(Ok(Value::List(result)))
            } else {
                Some(Err(RuntimeError::TypeError("range expects (start, end)".to_string())))
            }
        }
        "sort" => {
            if let Some(Value::List(items)) = args.first() {
                let mut sorted = items.clone();
                let desc = args.get(1).map(|a| format!("{}", a) == "desc").unwrap_or(false);
                sorted.sort_by(|a, b| {
                    let ordering = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => { let c = x.cmp(*y); c.cmp(&0) }
                        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
                        (Value::String(x), Value::String(y)) => x.cmp(y),
                        _ => std::cmp::Ordering::Equal,
                    };
                    if desc { ordering.reverse() } else { ordering }
                });
                Some(Ok(Value::List(sorted)))
            } else {
                Some(Err(RuntimeError::TypeError("sort(list) or sort(list, \"desc\")".to_string())))
            }
        }
        "nth" | "at" | "get_at" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let idx = match &args[1] {
                        Value::Int(si) => si.to_i64().unwrap_or(0) as usize,
                        _ => return Some(Err(RuntimeError::TypeError("nth: index must be Int".to_string()))),
                    };
                    if idx < items.len() {
                        Some(Ok(items[idx].clone()))
                    } else {
                        Some(Ok(Value::Unit)) // out of bounds -> null
                    }
                } else {
                    Some(Err(RuntimeError::TypeError("nth: first argument must be a list".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("nth(list, index)".to_string())))
            }
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
