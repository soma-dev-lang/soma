pub mod string;
pub mod math;
pub mod collection;
pub mod pipeline;
pub mod http;
pub mod time;
pub mod indicators;
pub mod io;
pub mod storage;
pub mod record;
pub mod llm;

use super::{Value, RuntimeError, map_from_pairs};
use std::collections::HashMap;

/// Dispatch builtin calls to sub-modules.
/// Returns None if the name is not a known builtin.
pub fn call_builtin(interp: &mut super::Interpreter, name: &str, args: &[Value], cell_name: &str) -> Option<Result<Value, RuntimeError>> {
    // V1: track nondeterminism for [record] / replay divergence detection.
    // If a recorded handler calls now()/random()/..., we want to know.
    if super::record_log::NONDET_BUILTINS.iter().any(|n| *n == name) {
        if !interp.record_nondet_called.iter().any(|s| s == name) {
            interp.record_nondet_called.push(name.to_string());
        }
    }
    // Try each category in order
    None
        .or_else(|| io::call_builtin(name, args))
        .or_else(|| string::call_builtin(name, args))
        .or_else(|| math::call_builtin(name, args))
        .or_else(|| collection::call_builtin(name, args))
        .or_else(|| pipeline::call_builtin(name, args))
        .or_else(|| http::call_builtin(name, args))
        .or_else(|| time::call_builtin(name, args))
        .or_else(|| indicators::call_builtin(name, args))
        .or_else(|| record::call_builtin(name, args))
        .or_else(|| storage::call_builtin(interp, name, args, cell_name))
}

/// Higher-order builtins: map, filter, find, any, each — require mutable interpreter
/// Called from eval_expr directly (not through call_builtin) because we need &mut self
pub fn call_lambda_builtin(interp: &mut super::Interpreter, name: &str, args: &[Value], cell_name: &str) -> Option<Result<Value, RuntimeError>> {
    // reduce/fold: (list, initial, lambda)
    if matches!(name, "reduce" | "fold") {
        if args.len() >= 3 {
            if let Some(Value::List(items)) = args.first() {
                let initial = &args[1];
                let lambda = match &args[2] {
                    v @ Value::Lambda { .. } => v,
                    v @ Value::LambdaBlock { .. } => v,
                    _ => return Some(Err(RuntimeError::TypeError("reduce: third argument must be a lambda".to_string()))),
                };
                let mut acc = initial.clone();
                for item in items {
                    let pair = map_from_pairs(vec![
                        ("acc".to_string(), acc),
                        ("val".to_string(), item.clone()),
                    ]);
                    match interp.apply_lambda(lambda, pair, cell_name) {
                        Ok(v) => acc = v,
                        Err(e) => return Some(Err(RuntimeError::TypeError(format!("{:?}", e)))),
                    }
                }
                return Some(Ok(acc));
            } else {
                return Some(Err(RuntimeError::TypeError("reduce: first argument must be a list".to_string())));
            }
        } else {
            return Some(Err(RuntimeError::TypeError("reduce expects (list, initial, lambda)".to_string())));
        }
    }

    // All these expect (list, lambda) as args
    let list = match args.first() {
        Some(Value::List(items)) => items,
        _ => return None,
    };
    let lambda = match args.get(1) {
        Some(v @ Value::Lambda { .. }) => v,
        Some(v @ Value::LambdaBlock { .. }) => v,
        _ => return None,
    };

    match name {
        "map" | "each" => {
            let mut result = Vec::with_capacity(list.len());
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(v) => result.push(v),
                    Err(e) => return Some(Err(RuntimeError::TypeError(format!("{:?}", e)))),
                }
            }
            Some(Ok(Value::List(result)))
        }
        "filter" => {
            let mut result = Vec::new();
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(v) => {
                        if super::is_truthy(&v) {
                            result.push(item.clone());
                        }
                    }
                    Err(e) => return Some(Err(RuntimeError::TypeError(format!("{:?}", e)))),
                }
            }
            Some(Ok(Value::List(result)))
        }
        "find" => {
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(v) => {
                        if super::is_truthy(&v) {
                            return Some(Ok(item.clone()));
                        }
                    }
                    Err(e) => return Some(Err(RuntimeError::TypeError(format!("{:?}", e)))),
                }
            }
            Some(Ok(Value::Unit))
        }
        "any" => {
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(v) => {
                        if super::is_truthy(&v) {
                            return Some(Ok(Value::Bool(true)));
                        }
                    }
                    Err(e) => return Some(Err(RuntimeError::TypeError(format!("{:?}", e)))),
                }
            }
            Some(Ok(Value::Bool(false)))
        }
        "all" => {
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(v) => {
                        if !super::is_truthy(&v) {
                            return Some(Ok(Value::Bool(false)));
                        }
                    }
                    Err(e) => return Some(Err(RuntimeError::TypeError(format!("{:?}", e)))),
                }
            }
            Some(Ok(Value::Bool(true)))
        }
        "count" => {
            let mut n = 0i64;
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(v) => {
                        if super::is_truthy(&v) { n += 1; }
                    }
                    Err(e) => return Some(Err(RuntimeError::TypeError(format!("{:?}", e)))),
                }
            }
            Some(Ok(Value::Int(crate::interpreter::soma_int::SomaInt::from_i64(n))))
        }
        _ => None,
    }
}

// ── Shared helper functions ──────────────────────────────────────────

/// Extract an i64 from a Value
pub fn val_to_i64(v: &Value) -> i64 {
    match v {
        Value::Int(si) => si.to_i64().unwrap_or(0),
        Value::Float(n) => *n as i64,
        Value::String(s) => s.parse::<i64>().unwrap_or_else(|_| s.parse::<f64>().unwrap_or(0.0) as i64),
        Value::Bool(b) => if *b { 1 } else { 0 },
        _ => 0,
    }
}

/// Extract an f64 from a Value
pub fn val_to_f64(v: &Value) -> f64 {
    match v {
        Value::Float(n) => *n,
        Value::Int(si) => si.to_f64(),
        Value::String(s) => s.parse::<f64>().unwrap_or(0.0),
        Value::Bool(b) => if *b { 1.0 } else { 0.0 },
        _ => 0.0,
    }
}

/// Get a field from a Map as i64
pub fn map_field_i64(item: &Value, field: &str) -> i64 {
    if let Value::Map(entries) = item {
        entries.get(field).map(val_to_i64).unwrap_or(0)
    } else { 0 }
}

/// Get a field from a Map as f64
pub fn map_field_f64(item: &Value, field: &str) -> f64 {
    if let Value::Map(entries) = item {
        entries.get(field).map(val_to_f64).unwrap_or(0.0)
    } else { 0.0 }
}

/// Parse a JSON string into a Value
pub fn json_to_value(s: &str) -> Value {
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) {
        serde_json_to_value(&parsed)
    } else {
        Value::String(s.to_string())
    }
}

pub fn serde_json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Unit,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(crate::interpreter::soma_int::SomaInt::from_i64(i))
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Value::List(arr.iter().map(serde_json_to_value).collect())
        }
        serde_json::Value::Object(obj) => {
            Value::Map(obj.iter().map(|(k, v)| (k.clone(), serde_json_to_value(v))).collect())
        }
    }
}
