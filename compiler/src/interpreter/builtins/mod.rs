pub mod string;
pub mod math;
pub mod collection;
pub mod pipeline;
pub mod http;
pub mod time;
pub mod io;
pub mod storage;
pub mod record;
pub mod llm;
pub mod linalg;
pub mod registry;

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

    // V1.6: tool-capability enforcement. If the LLM dispatched into a tool
    // with declared capabilities, the http/* builtins refuse URLs that do
    // not match any declared scope.
    // a capability-scoped tool reaches only its URLs: no file access, no raw
    // sockets (read_file / write_file / ws_connect bypassed the capability)
    if interp.current_tool_caps.is_some() && matches!(name, "read_file" | "write_file" | "read_csv" | "write_csv" | "read_files" | "par_read_files" | "read_stdin"
        // templates read files too, and link() routes every later emit to a peer
        | "load" | "include" | "load_template" | "link" | "ws_send"
        | "ws_connect" | "connect" | "subscribe" | "append_file") {
        let caps = interp.current_tool_caps.clone().unwrap_or_default();
        if !caps.iter().any(|c| c == "*") || interp.outer_tool_caps.iter().any(|o| !o.iter().any(|c| c == "*")) {
            return Some(Err(RuntimeError::TypeError(format!("capability denied: {}() is outside this tool's capabilities {:?}", name, caps))));
        }
    }
    http::NO_REDIRECTS.with(|c| c.set(interp.current_tool_caps.as_ref().map_or(false, |caps| !caps.iter().any(|c| c == "*"))));
    if matches!(name, "http_get" | "http_post" | "http_put" | "http_patch" | "http_delete") {
        if let Some(caps) = interp.current_tool_caps.clone() {
            if let Some(Value::String(url)) = args.first() {
                if !url_matches_any(url, &caps) || interp.outer_tool_caps.iter().any(|o| !url_matches_any(url, o)) {
                    return Some(Err(RuntimeError::TypeError(format!(
                        "capability denied: '{}' does not match any of {:?}", url, caps
                    ))));
                }
            }
        }
    }

    // counts, indexes, widths and code points are 64-bit: a BigInt there was
    // read as 0 (range(2^70, 2^70 + 3) == [], substring(s, 1, 2^70) == "",
    // random(2^70) == 0) — say so instead
    if matches!(name, "range" | "random" | "chr" | "substring" | "slice" | "pad_left" | "pad_right" | "repeat" | "take" | "drop" | "with" | "nth" | "days_in_month" | "sleep" | "str_at" | "left" | "right")
        // with(xs, i, v): only the index is a count — the stored value may be big
        && args.iter().enumerate().any(|(n, a)| !(name == "with" && n != 1) && matches!(a, Value::Int(i) if i.to_i64().is_none()))
    {
        return Some(Err(RuntimeError::Domain { kind: "range".to_string(), message: format!("{}(): an Int argument past 64 bits (a count, index or width is at most 2^63 - 1)", name) }));
    }
    // Try each category in order. A panic inside a builtin (a capacity
    // overflow the size checks missed) is an ordinary `try`-catchable error
    // of kind `internal`, not the end of the process.
    let interp_ptr = std::panic::AssertUnwindSafe(&mut *interp);
    let outcome = std::panic::catch_unwind(move || {
        let interp = interp_ptr;
        let interp: &mut super::Interpreter = interp.0;
        call_categories(interp, name, args, cell_name)
    });
    match outcome {
        Ok(r) => r,
        Err(p) => {
            let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "a builtin failed".to_string());
            Some(Err(RuntimeError::Domain { kind: "internal".to_string(), message: format!("{}(): {}", name, msg) }))
        }
    }
}

fn call_categories(interp: &mut super::Interpreter, name: &str, args: &[Value], cell_name: &str) -> Option<Result<Value, RuntimeError>> {
    None
        .or_else(|| io::call_builtin(name, args))
        .or_else(|| string::call_builtin(name, args))
        .or_else(|| math::call_builtin(name, args))
        .or_else(|| collection::call_builtin(name, args))
        .or_else(|| pipeline::call_builtin(name, args))
        .or_else(|| http::call_builtin(name, args))
        .or_else(|| time::call_builtin(name, args))
        .or_else(|| record::call_builtin(name, args))
        .or_else(|| linalg::call_builtin(name, args))
        .or_else(|| storage::call_builtin(interp, name, args, cell_name))
}

/// Match a URL against capability scopes.
/// Recognized scope forms:
///   "net:host"          — host name (exact match against URL host)
///   "net:host/prefix"   — host + path prefix
///   "net:*"             — any network
///   "net:https://..."   — match prefix against full URL
/// `*` matches any run of characters (a URL pattern like the docs'
/// `https://api.x.com/*`, which was denied for every URL: only `net:` was read)
fn glob_match(pat: &str, text: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 { return pat == text; }
    let mut rest = text;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !rest.starts_with(part) { return false; }
            rest = &rest[part.len()..];
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) { Some(p) => rest = &rest[p + part.len()..], None => return false }
        }
    }
    true
}

/// `scheme://authority` and the rest (path, query) of a URL a tool may
/// fetch — None for one no capability can allow: userinfo (`a@b`), a
/// fragment, a backslash, whitespace, or a `.` / `..` path segment (plain or
/// percent-encoded): `/public/../admin` left a `/public/*` scope, and a `*`
/// used to match across the host (`http://127.0.0.1/a.x.com/` passed
/// `http://*.x.com/*`).
fn split_url(u: &str) -> Option<(&str, &str, String)> {
    let (scheme, after) = u.split_once("://")?;
    if u.contains('#') || u.contains('\\') || u.chars().any(|c| c.is_whitespace() || c.is_control()) { return None; }
    let end = after.find(|c| c == '/' || c == '?').unwrap_or(after.len());
    let (authority, rest) = after.split_at(end);
    if authority.is_empty() || authority.contains('@') || authority.contains('%') { return None; }
    let path = rest.split('?').next().unwrap_or("");
    let decoded = path.replace("%2e", ".").replace("%2E", ".").replace("%2f", "/").replace("%2F", "/").replace("%5c", "/").replace("%5C", "/");
    if decoded.split('/').any(|seg| seg == "." || seg == "..") { return None; }
    Some((scheme, authority, rest.to_string()))
}

fn url_matches_any(url: &str, caps: &[String]) -> bool {
    let Some((scheme, authority, rest)) = split_url(url) else {
        return caps.iter().any(|c| c == "net:*" || c == "*");
    };
    let host = authority.to_ascii_lowercase();
    for cap in caps {
        if cap == "net:*" || cap == "*" { return true; }
        let pattern = cap.strip_prefix("net:").unwrap_or(cap);
        if let Some((pscheme, pauth, prest)) = pattern.split_once("://").map(|(s, a)| {
            let end = a.find(|c| c == '/' || c == '?').unwrap_or(a.len());
            (s, &a[..end], &a[end..])
        }) {
            // the host part and the path part are matched SEPARATELY: a `*`
            // in the host cannot reach into the path, and the reverse
            if pscheme.eq_ignore_ascii_case(scheme) && glob_match(&pauth.to_ascii_lowercase(), &host) {
                let ok = if cap.starts_with("net:") && !prest.contains('*') {
                    rest.starts_with(prest) || prest.is_empty()
                } else if prest.is_empty() {
                    rest.is_empty() || rest == "/"
                } else {
                    glob_match(prest, &rest)
                };
                if ok { return true; }
            }
            continue;
        }
        // `net:host` / `net:host/prefix`
        if cap.starts_with("net:") {
            let (phost, ppath) = match pattern.find('/') { Some(k) => (&pattern[..k], &pattern[k..]), None => (pattern, "") };
            if glob_match(&phost.to_ascii_lowercase(), &host) && rest.starts_with(ppath) { return true; }
        }
    }
    false
}

/// Higher-order builtins: map, filter, find, any, each — require mutable interpreter
/// Called from eval_expr directly (not through call_builtin) because we need &mut self
pub fn call_lambda_builtin(interp: &mut super::Interpreter, name: &str, args: &[Value], cell_name: &str) -> Option<Result<Value, RuntimeError>> {
    // reduce/fold: (list, initial, lambda)
    if matches!(name, "reduce") {
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
                        Err(e) => return Some(Err(crate::interpreter::lambda_error(e))),
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
        // sort_by(rows, r => r.total)            ascending by key
        // sort_by(rows, r => r.total, "desc")    descending
        // sort_by(rows, r => [0 - r.total, r.name])   several keys: a list
        // Stable: ties keep their input order.
        "sort_by" => {
            let desc = args.get(2).map(|v| format!("{}", v) == "desc").unwrap_or(false);
            let mut keyed: Vec<(Value, Value)> = Vec::with_capacity(list.len());
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(k) => keyed.push((k, item.clone())),
                    Err(e) => return Some(Err(crate::interpreter::lambda_error(e))),
                }
            }
            keyed.sort_by(|a, b| {
                let o = collection::compare_values(&a.0, &b.0);
                if desc { o.reverse() } else { o }
            });
            Some(Ok(Value::List(keyed.into_iter().map(|(_, v)| v).collect())))
        }
        "map" => {
            let mut result = Vec::with_capacity(list.len());
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(v) => result.push(v),
                    Err(e) => return Some(Err(crate::interpreter::lambda_error(e))),
                }
            }
            Some(Ok(Value::List(result)))
        }
        "filter" => {
            let mut result = Vec::new();
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(v) => {
                        if match pred_bool(name, &v) { Ok(b) => b, Err(e) => return Some(Err(e)) } {
                            result.push(item.clone());
                        }
                    }
                    Err(e) => return Some(Err(crate::interpreter::lambda_error(e))),
                }
            }
            Some(Ok(Value::List(result)))
        }
        "find" => {
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(v) => {
                        if match pred_bool(name, &v) { Ok(b) => b, Err(e) => return Some(Err(e)) } {
                            return Some(Ok(item.clone()));
                        }
                    }
                    Err(e) => return Some(Err(crate::interpreter::lambda_error(e))),
                }
            }
            Some(Ok(Value::Unit))
        }
        "any" => {
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(v) => {
                        if match pred_bool(name, &v) { Ok(b) => b, Err(e) => return Some(Err(e)) } {
                            return Some(Ok(Value::Bool(true)));
                        }
                    }
                    Err(e) => return Some(Err(crate::interpreter::lambda_error(e))),
                }
            }
            Some(Ok(Value::Bool(false)))
        }
        "all" => {
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(v) => {
                        if !match pred_bool(name, &v) { Ok(b) => b, Err(e) => return Some(Err(e)) } {
                            return Some(Ok(Value::Bool(false)));
                        }
                    }
                    Err(e) => return Some(Err(crate::interpreter::lambda_error(e))),
                }
            }
            Some(Ok(Value::Bool(true)))
        }
        "count" => {
            let mut n = 0i64;
            for item in list {
                match interp.apply_lambda(lambda, item.clone(), cell_name) {
                    Ok(v) => {
                        if match pred_bool(name, &v) { Ok(b) => b, Err(e) => return Some(Err(e)) } { n += 1; }
                    }
                    Err(e) => return Some(Err(crate::interpreter::lambda_error(e))),
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
            } else if let Ok(big) = n.to_string().parse::<rug::Integer>() {
                // integers beyond i64 roundtrip exactly (needs serde_json
                // arbitrary_precision, which preserves the raw digits)
                Value::Int(crate::interpreter::soma_int::SomaInt::from_rug(big))
            } else {
                // a number f64 cannot hold (1e400): infinity, not 0.0
                Value::Float(n.as_f64().unwrap_or_else(|| {
                    if n.to_string().starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY }
                }))
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            Value::List(arr.iter().map(serde_json_to_value).collect())
        }
        serde_json::Value::Object(obj) => {
            // `{"_type": "Pay", "_variant": "Charged", …}` is what to_json
            // writes for a variant: bring the variant back
            if let (Some(serde_json::Value::String(t)), Some(serde_json::Value::String(v))) = (obj.get("_type"), obj.get("_variant")) {
                use crate::interpreter::VariantValue;
                let fields = if let Some(serde_json::Value::Array(items)) = obj.get("_values").or_else(|| obj.get("_fields")) {
                    VariantValue::Tuple(items.iter().map(serde_json_to_value).collect())
                } else {
                    let entries: indexmap::IndexMap<String, Value> = obj.iter()
                        .filter(|(k, _)| k.as_str() != "_type" && k.as_str() != "_variant")
                        .map(|(k, v)| (k.clone(), serde_json_to_value(v))).collect();
                    if entries.is_empty() { VariantValue::Unit } else { VariantValue::Struct(entries) }
                };
                return Value::Variant { type_name: t.clone(), variant: v.clone(), fields };
            }
            Value::Map(obj.iter().map(|(k, v)| (k.clone(), serde_json_to_value(v))).collect())
        }
    }
}

/// The answer of a filter / find / any / all / count predicate: a Bool
/// (`()` is false), like an `if` condition — a String "false" counted as true.
fn pred_bool(name: &str, v: &Value) -> Result<bool, RuntimeError> {
    match v {
        Value::Bool(b) => Ok(*b),
        Value::Unit => Ok(false),
        other => Err(RuntimeError::TypeError(format!("{}(): the lambda answered {} {} — a predicate answers a Bool (compare: `x => x.n > 0`)", name, crate::interpreter::value_type_name(other), { let t: String = format!("{}", other).chars().take(30).collect(); t }))),
    }
}
