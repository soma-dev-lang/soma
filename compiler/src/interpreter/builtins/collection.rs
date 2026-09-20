use super::super::{Value, RuntimeError, map_from_pairs};
use std::collections::HashMap;
use indexmap::IndexMap;
use crate::interpreter::soma_int::SomaInt;

/// Total order over values, for sorting: () < Bool < numbers < String <
/// List (lexicographic) < everything else. Numbers compare by value, so
/// 2 < 2.5 < 3. Used by sort_by; stable sorts keep ties in input order.
pub fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    fn rank(v: &Value) -> u8 {
        match v {
            Value::Unit => 0,
            Value::Bool(_) => 1,
            Value::Int(_) | Value::Float(_) => 2,
            Value::String(_) => 3,
            Value::List(_) => 4,
            _ => 5,
        }
    }
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Int(x), Value::Int(y)) => x.cmp(y).cmp(&0),
        (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => {
            // NaN sorts after every number (Equal broke the sort's total
            // order: sort_by over a column with a NaN came back unsorted)
            let nan = |v: &Value| matches!(v, Value::Float(f) if f.is_nan());
            match (nan(a), nan(b)) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                _ => crate::interpreter::numeric_cmp(a, b).unwrap_or(Ordering::Equal),
            }
        }
        (Value::String(x), Value::String(y)) => x.cmp(y),
        (Value::List(x), Value::List(y)) => {
            for (p, q) in x.iter().zip(y.iter()) {
                let o = compare_values(p, q);
                if o != Ordering::Equal {
                    return o;
                }
            }
            x.len().cmp(&y.len())
        }
        _ => rank(a).cmp(&rank(b)),
    }
}

/// Validate both materialized ranges and the interpreter's allocation-free loop.
pub(crate) fn range_spec(args: &[Value]) -> Result<(i64, i64, i64, u128), RuntimeError> {
    if !(2..=3).contains(&args.len()) {
        return Err(RuntimeError::TypeError("range expects (start, end) or (start, end, step)".into()));
    }
    let integer = |v: &Value| match v {
        Value::Int(n) => n.to_i64().ok_or_else(|| RuntimeError::Domain {
            kind: "range".into(), message: "range(): bounds and step must fit in 64 bits".into(),
        }),
        _ => Err(RuntimeError::TypeError(format!("range(start: Int, end: Int, step?: Int), got {} {}", crate::interpreter::value_type_name(v), v))),
    };
    let (start, end) = (integer(&args[0])?, integer(&args[1])?);
    let step = args.get(2).map(integer).transpose()?.unwrap_or(1);
    if step == 0 {
        return Err(RuntimeError::Domain { kind: "range".into(), message: "range(): step must not be zero".into() });
    }
    let distance = if step > 0 { end as i128 - start as i128 } else { start as i128 - end as i128 };
    let stride = (step as i128).abs();
    let count = if distance <= 0 { 0 } else { ((distance + stride - 1) / stride) as u128 };
    Ok((start, end, step, count))
}

/// Resolve a possibly negative index against `len` (-1 = last), clamped.
fn clamp_index(i: i64, len: usize) -> usize {
    let len = len as i64;
    let i = if i < 0 { len + i } else { i };
    i.clamp(0, len) as usize
}

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        // fail("kind", "detail") raises a domain error; fail(r) re-raises a
        // caught try-result with its original kind.
        "fail" => {
            match args.first() {
                Some(Value::Map(m)) if m.contains_key("error") => {
                    let kind = m.get("kind").map(|v| format!("{}", v)).unwrap_or_else(|| "error".to_string());
                    let message = m.get("error").map(|v| format!("{}", v)).unwrap_or_default();
                    Some(Err(RuntimeError::Domain { kind, message }))
                }
                Some(k) => {
                    let kind = format!("{}", k);
                    let message = match args.get(1) {
                        Some(d) => format!("{}: {}", kind, d),
                        None => kind.clone(),
                    };
                    Some(Err(RuntimeError::Domain { kind, message }))
                }
                None => Some(Err(RuntimeError::Domain { kind: "error".to_string(), message: "fail()".to_string() })),
            }
        }
        // contains(list, x) — membership by structural equality;
        // contains(map, key). (contains(string, sub) lives in string.rs.)
        "contains" if matches!(args.first(), Some(Value::List(_) | Value::Map(_))) && args.len() == 2 => {
            match &args[0] {
                Value::List(items) => Some(Ok(Value::Bool(
                    items.iter().any(|it| crate::interpreter::deep_equal(it, &args[1])),
                ))),
                Value::Map(entries) => Some(Ok(Value::Bool(entries.contains_key(&format!("{}", args[1]))))),
                _ => None,
            }
        }
        // slice(xs, start, end?) — end exclusive, negative indexes count from
        // the end; works on lists and strings. Out-of-range is clamped.
        "slice" if matches!(args.first(), Some(Value::List(_) | Value::String(_))) && (args.len() == 2 || args.len() == 3) => {
            let idx = |v: &Value| match v {
                Value::Int(i) => i.to_i64(),
                _ => None,
            };
            let Some(start) = idx(&args[1]) else {
                return Some(Err(RuntimeError::TypeError("slice(xs, start, end?): start must be an Int".to_string())));
            };
            let end = match args.get(2) {
                None | Some(Value::Unit) => None,
                Some(v) => match idx(v) {
                    Some(e) => Some(e),
                    None => return Some(Err(RuntimeError::TypeError("slice(xs, start, end?): end must be an Int".to_string()))),
                },
            };
            match &args[0] {
                Value::List(items) => {
                    let a = clamp_index(start, items.len());
                    let b = end.map(|e| clamp_index(e, items.len())).unwrap_or(items.len());
                    Some(Ok(Value::List(if a < b { items[a..b].to_vec() } else { Vec::new() })))
                }
                Value::String(text) => {
                    let chars: Vec<char> = text.chars().collect();
                    let a = clamp_index(start, chars.len());
                    let b = end.map(|e| clamp_index(e, chars.len())).unwrap_or(chars.len());
                    Some(Ok(Value::String(if a < b { chars[a..b].iter().collect() } else { String::new() })))
                }
                _ => None,
            }
        }
        // keys(m) / values(m) / entries(m) on a Map VALUE (memory slots use
        // the .keys() / .values() / .entries() methods). With any other
        // argument shape these fall through, so a handler named `entries`
        // keeps working.
        "keys" if args.len() == 1 && matches!(args[0], Value::Map(_)) => {
            let Value::Map(m) = &args[0] else { return None };
            Some(Ok(Value::List(m.keys().map(|k| Value::String(k.clone())).collect())))
        }
        // a record (struct variant): its declared field names / values
        "keys" if args.len() == 1 && matches!(args[0], Value::Variant { fields: crate::interpreter::VariantValue::Struct(_), .. }) => {
            let Value::Variant { fields: crate::interpreter::VariantValue::Struct(fs), .. } = &args[0] else { return None };
            Some(Ok(Value::List(fs.keys().map(|k| Value::String(k.clone())).collect())))
        }
        "values" if args.len() == 1 && matches!(args[0], Value::Variant { fields: crate::interpreter::VariantValue::Struct(_), .. }) => {
            let Value::Variant { fields: crate::interpreter::VariantValue::Struct(fs), .. } = &args[0] else { return None };
            Some(Ok(Value::List(fs.values().cloned().collect())))
        }
        "values" if args.len() == 1 && matches!(args[0], Value::Map(_)) => {
            let Value::Map(m) = &args[0] else { return None };
            Some(Ok(Value::List(m.values().cloned().collect())))
        }
        "entries" if args.len() == 1 && matches!(args[0], Value::Map(_)) => {
            let Value::Map(m) = &args[0] else { return None };
            Some(Ok(Value::List(
                m.iter()
                    .map(|(k, v)| map_from_pairs(vec![
                        ("key".to_string(), Value::String(k.clone())),
                        ("value".to_string(), v.clone()),
                    ]))
                    .collect(),
            )))
        }
        "list" => {
            // Construct a list of the arguments literally, so
            // list(list(1,2), list(3,4)) nests as [[1,2],[3,4]] — matching
            // the [a, b] literal. The `x = list(x, item)` append idiom is
            // handled in-place by the interpreter's assignment fast path
            // (it never reaches here); use push() or `+` to concatenate.
            Some(Ok(Value::List(args.to_vec())))
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
        "push" => {
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
            if args.len() % 2 == 0 {
                return Some(Err(RuntimeError::TypeError("with expects a collection followed by complete key-value pairs".to_string())));
            }
            match args.first() {
                // a record: its declared fields only (types checked where it lands)
                Some(Value::Variant { type_name, variant, fields: crate::interpreter::VariantValue::Struct(fs) }) => {
                    let mut out = fs.clone();
                    let mut i = 1;
                    while i + 1 < args.len() {
                        let key = format!("{}", args[i]);
                        if !out.contains_key(&key) {
                            return Some(Err(RuntimeError::TypeError(format!("{} has no field '{}' (fields: {})", variant, key, fs.keys().map(|x| x.as_str()).collect::<Vec<_>>().join(", ")))));
                        }
                        out.insert(key, args[i + 1].clone());
                        i += 2;
                    }
                    Some(Ok(Value::Variant { type_name: type_name.clone(), variant: variant.clone(), fields: crate::interpreter::VariantValue::Struct(out) }))
                }
                Some(Value::Map(entries)) => {
                    let mut result = entries.clone();
                    let mut i = 1;
                    while i + 1 < args.len() {
                        let key = format!("{}", args[i]);
                        let val = args[i + 1].clone();
                        result.insert(key, val);
                        i += 2;
                    }
                    Some(Ok(Value::Map(result)))
                }
                // with(list, index, value) → new list with element replaced.
                Some(Value::List(items)) if args.len() == 3 => {
                    let idx = match &args[1] {
                        Value::Int(si) => si.to_i64().unwrap_or(-1),
                        _ => return Some(Err(RuntimeError::TypeError(
                            "with(list, index, value): index must be an Int".to_string()))),
                    };
                    if idx < 0 || idx as usize >= items.len() {
                        return Some(Err(RuntimeError::TypeError(format!(
                            "with: list index {} out of bounds (length {})", idx, items.len()))));
                    }
                    let mut result = items.clone();
                    result[idx as usize] = args[2].clone();
                    Some(Ok(Value::List(result)))
                }
                _ => Some(Err(RuntimeError::TypeError(
                    "with expects (map, key, value) or (list, index, value)".to_string()))),
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
            Some((|| {
                let (start, end, step, count) = range_spec(args)?;
                if count > crate::interpreter::MAX_LIST_LEN as u128 {
                    return Err(RuntimeError::Domain { kind: "range".to_string(), message: format!("range({}, {}): {} elements is past the limit of {} for a List — loop with `for i in range(a, b)` (not materialized) or a while loop", start, end, count, crate::interpreter::MAX_LIST_LEN) });
                }
                let mut result = Vec::new();
                if step > 0 {
                    let mut i = start;
                    // checked: `i += step` near i64::MAX wrapped negative and
                    // the loop never ended (range(-5, 2^63 - 1, 2^63 - 1))
                    while i < end { result.push(Value::Int(SomaInt::from_i64(i))); match i.checked_add(step) { Some(n) => i = n, None => break } }
                } else if step < 0 {
                    let mut i = start;
                    while i > end { result.push(Value::Int(SomaInt::from_i64(i))); match i.checked_add(step) { Some(n) => i = n, None => break } }
                }
                Ok(Value::List(result))
            })())
        }
        "sort" => {
            if let Some(Value::List(items)) = args.first() {
                let mut sorted = items.clone();
                let desc = args.get(1).map(|a| format!("{}", a) == "desc").unwrap_or(false);
                let mut incomparable: Option<String> = None;
                sorted.sort_by(|a, b| {
                    let ordering = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => { let c = x.cmp(y); c.cmp(&0) }
                        // mixed Int/Float compare numerically; NaN gets a total
                        // order (sorts after all finite values) instead of
                        // silently corrupting the sort
                        (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => compare_values(a, b),
                        (Value::String(x), Value::String(y)) => x.cmp(y),
                        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
                        _ => {
                            if incomparable.is_none() {
                                incomparable = Some(format!(
                                    "sort: cannot compare {} and {}",
                                    crate::interpreter::value_type_name(a),
                                    crate::interpreter::value_type_name(b)
                                ));
                            }
                            std::cmp::Ordering::Equal
                        }
                    };
                    if desc { ordering.reverse() } else { ordering }
                });
                match incomparable {
                    Some(msg) => Some(Err(RuntimeError::TypeError(msg))),
                    None => Some(Ok(Value::List(sorted))),
                }
            } else {
                Some(Err(RuntimeError::TypeError("sort(list) or sort(list, \"desc\")".to_string())))
            }
        }
        "nth" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    // negative from the end, like xs[-1]; () out of range
                    let raw = match &args[1] {
                        Value::Int(si) => si.to_i64().unwrap_or(i64::MAX),
                        _ => return Some(Err(RuntimeError::TypeError("nth: index must be Int".to_string()))),
                    };
                    let k = if raw < 0 { raw + items.len() as i64 } else { raw };
                    if k >= 0 && (k as usize) < items.len() {
                        Some(Ok(items[k as usize].clone()))
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
