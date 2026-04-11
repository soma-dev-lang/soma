use super::super::{Value, RuntimeError};
use super::{val_to_i64, val_to_f64, map_field_i64, map_field_f64};
use crate::interpreter::soma_int::SomaInt;
use indexmap::IndexMap;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "filter_by" => {
            if let Some(Value::List(items)) = args.first() {
                let field = if args.len() >= 2 { format!("{}", args[1]) } else { return Some(Ok(Value::List(items.clone()))); };
                let (op, threshold) = if args.len() >= 4 {
                    (format!("{}", args[2]), &args[3])
                } else if args.len() >= 3 {
                    ("==".to_string(), &args[2])
                } else {
                    return Some(Ok(Value::List(items.clone())));
                };
                match op.as_str() {
                    ">" | ">=" | "<" | "<=" | "==" | "=" | "!=" => {}
                    _ => return Some(Err(RuntimeError::TypeError(
                        format!("filter_by: unknown operator '{}' (use >, >=, <, <=, ==, !=)", op)
                    ))),
                }
                let result: Vec<Value> = items.iter().filter(|item| {
                    if let Value::Map(entries) = item {
                        let val = entries.get(&field);
                        if let Some(val) = val {
                            let use_float = matches!(val, Value::Float(_)) || matches!(threshold, Value::Float(_));
                            if use_float {
                                let a = val_to_f64(val);
                                let b = val_to_f64(threshold);
                                match op.as_str() {
                                    ">" => a > b,
                                    ">=" => a >= b,
                                    "<" => a < b,
                                    "<=" => a <= b,
                                    "==" | "=" => (a - b).abs() < f64::EPSILON,
                                    "!=" => (a - b).abs() >= f64::EPSILON,
                                    _ => false,
                                }
                            } else {
                                let a = val_to_i64(val);
                                let b = val_to_i64(threshold);
                                match op.as_str() {
                                    ">" => a > b,
                                    ">=" => a >= b,
                                    "<" => a < b,
                                    "<=" => a <= b,
                                    "==" | "=" => format!("{}", val) == format!("{}", threshold),
                                    "!=" => format!("{}", val) != format!("{}", threshold),
                                    _ => false,
                                }
                            }
                        } else { false }
                    } else { false }
                }).cloned().collect();
                Some(Ok(Value::List(result)))
            } else {
                Some(Err(RuntimeError::TypeError("filter_by expects (list, field, op, value)".to_string())))
            }
        }
        "sort_by" => {
            if let Some(Value::List(items)) = args.first() {
                let field = if args.len() >= 2 { format!("{}", args[1]) } else { return Some(Ok(Value::List(items.clone()))); };
                let desc = args.get(2).map(|v| format!("{}", v) == "desc").unwrap_or(false);
                let has_float = items.iter().any(|item| {
                    if let Value::Map(entries) = item {
                        entries.get(&field).map(|v| matches!(v, Value::Float(_))).unwrap_or(false)
                    } else { false }
                });
                let mut sorted = items.clone();
                if has_float {
                    sorted.sort_by(|a, b| {
                        let av = map_field_f64(a, &field);
                        let bv = map_field_f64(b, &field);
                        if desc { bv.partial_cmp(&av).unwrap_or(std::cmp::Ordering::Equal) }
                        else { av.partial_cmp(&bv).unwrap_or(std::cmp::Ordering::Equal) }
                    });
                } else {
                    sorted.sort_by(|a, b| {
                        let av = map_field_i64(a, &field);
                        let bv = map_field_i64(b, &field);
                        if desc { bv.cmp(&av) } else { av.cmp(&bv) }
                    });
                }
                Some(Ok(Value::List(sorted)))
            } else {
                Some(Err(RuntimeError::TypeError("sort_by expects (list, field)".to_string())))
            }
        }
        "top" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let n = val_to_i64(&args[1]) as usize;
                    Some(Ok(Value::List(items.iter().take(n).cloned().collect())))
                } else {
                    Some(Ok(args[0].clone()))
                }
            } else {
                Some(Err(RuntimeError::TypeError("top expects (list, n)".to_string())))
            }
        }
        "bottom" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let n = val_to_i64(&args[1]) as usize;
                    let start = if n >= items.len() { 0 } else { items.len() - n };
                    Some(Ok(Value::List(items[start..].to_vec())))
                } else {
                    Some(Ok(args[0].clone()))
                }
            } else {
                Some(Err(RuntimeError::TypeError("bottom expects (list, n)".to_string())))
            }
        }
        "sum_by" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let total: i64 = items.iter().map(|item| map_field_i64(item, &field)).sum();
                    Some(Ok(Value::Int(SomaInt::from_i64(total))))
                } else { Some(Ok(Value::Int(SomaInt::from_i64(0)))) }
            } else {
                Some(Err(RuntimeError::TypeError("sum_by expects (list, field)".to_string())))
            }
        }
        "avg_by" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    if items.is_empty() { return Some(Ok(Value::Unit)); }
                    let total: f64 = items.iter().map(|item| map_field_f64(item, &field)).sum();
                    let avg = total / items.len() as f64;
                    // Return Int if whole number, Float otherwise
                    if avg == (avg as i64) as f64 {
                        Some(Ok(Value::Int(SomaInt::from_i64(avg as i64))))
                    } else {
                        Some(Ok(Value::Float(avg)))
                    }
                } else { Some(Ok(Value::Unit)) }
            } else {
                Some(Err(RuntimeError::TypeError("avg_by expects (list, field)".to_string())))
            }
        }
        "min_by" | "max_by" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let is_max = name == "max_by";
                    let result = items.iter().max_by_key(|item| {
                        let v = map_field_i64(item, &field);
                        if is_max { v } else { -v }
                    });
                    Some(Ok(result.cloned().unwrap_or(Value::Unit)))
                } else { Some(Ok(Value::Unit)) }
            } else {
                Some(Err(RuntimeError::TypeError("min_by/max_by expects (list, field)".to_string())))
            }
        }
        "pluck" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let result: Vec<Value> = items.iter().map(|item| {
                        if let Value::Map(entries) = item {
                            entries.get(&field).cloned().unwrap_or(Value::Unit)
                        } else { Value::Unit }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(Value::List(vec![]))) }
            } else {
                Some(Err(RuntimeError::TypeError("map_by expects (list, field)".to_string())))
            }
        }
        "group_by" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let mut groups: IndexMap<String, Vec<Value>> = IndexMap::new();
                    for item in items {
                        let key = if let Value::Map(entries) = item {
                            entries.get(&field)
                                .map(|v| format!("{}", v)).unwrap_or("unknown".to_string())
                        } else { "unknown".to_string() };
                        groups.entry(key).or_default().push(item.clone());
                    }
                    let result: IndexMap<String, Value> = groups.into_iter()
                        .map(|(k, v)| (k, Value::List(v))).collect();
                    Some(Ok(Value::Map(result)))
                } else { Some(Ok(Value::Map(IndexMap::new()))) }
            } else {
                Some(Err(RuntimeError::TypeError("group_by expects (list, field)".to_string())))
            }
        }
        "distinct" => {
            if let Some(Value::List(items)) = args.first() {
                if let Some(field) = args.get(1) {
                    let field = format!("{}", field);
                    let mut seen = Vec::new();
                    let mut result: Vec<Value> = Vec::new();
                    for item in items {
                        let v = if let Value::Map(e) = item {
                            e.get(&field).cloned().unwrap_or(Value::Unit)
                        } else { item.clone() };
                        let key = format!("{}", v);
                        if !seen.contains(&key) {
                            seen.push(key);
                            result.push(v);
                        }
                    }
                    Some(Ok(Value::List(result)))
                } else {
                    let mut seen = Vec::new();
                    let result: Vec<Value> = items.iter().filter(|item| {
                        let v = format!("{}", item);
                        if seen.contains(&v) { false } else { seen.push(v); true }
                    }).cloned().collect();
                    Some(Ok(Value::List(result)))
                }
            } else { Some(Ok(Value::List(vec![]))) }
        }
        _ => None,
    }
}
