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
        "entries" if args.len() == 1 && matches!(args[0], Value::Map(_) | Value::Variant { fields: crate::interpreter::VariantValue::Struct(_), .. }) => {
            let (Value::Map(m) | Value::Variant { fields: crate::interpreter::VariantValue::Struct(m), .. }) = &args[0] else { return None };
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
                    // `map(m.nope, 5)` stored the key "null", as `with` refuses
                    Value::Unit => return Some(Err(RuntimeError::TypeError(format!(
                        "map(): key {} is () — an absent value (a mistyped field?) cannot be a key", i / 2 + 1)))),
                    other => format!("{}", other),
                };
                let val = args[i + 1].clone();
                entries.insert(key, val);
                i += 2;
            }
            Some(Ok(Value::Map(entries)))
        }
        "push" => {
            // push(list(1)) answered [[1]] and push(5, 6) the list [5, 6]:
            // the first argument is the list, the rest are appended
            match args.first() {
                Some(Value::List(existing)) => {
                    let mut result = existing.clone();
                    result.extend(args[1..].iter().cloned());
                    Some(Ok(Value::List(result)))
                }
                other => Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                    "push(list, items…) appends to a List, got {} — start from `[]`: push([], x)",
                    other.map(|v| format!("{} {}", crate::interpreter::value_type_name(v), super::string::to_json_string(v))).unwrap_or_else(|| "nothing".to_string())) })),
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
                        // `()` is no key (it became the entry "null")
                        if matches!(args[i], Value::Unit) {
                            return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message:
                                "with(map, (), …): the key is () — an absent value (a mistyped field?) cannot be a key".to_string() }));
                        }
                        let key = crate::interpreter::key_text(&args[i]);
                        let val = args[i + 1].clone();
                        result.insert(key, val);
                        i += 2;
                    }
                    let out = Value::Map(result);
                    if let Err(e) = crate::interpreter::check_response_status(&out) { return Some(Err(e)); }
                    Some(Ok(out))
                }
                // with(list, index, value) → new list with element replaced.
                Some(Value::List(items)) if args.len() == 3 => {
                    // negative from the end, as `xs[-1]` (a nested write
                    // `g[0][-1] = 8` goes through here); past 64 bits it is
                    // out of bounds
                    let idx = match &args[1] {
                        Value::Int(si) => si.to_i64().map(|i| if i < 0 { i + items.len() as i64 } else { i }).unwrap_or(i64::MAX),
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
                // a nested write `mm["x"]["y"] = 3` / `xs[0].y = 3` reaches
                // here with the missing (or scalar) container: name the key
                Some(Value::Unit) if args.len() == 3 => Some(Err(RuntimeError::TypeError(format!(
                    "cannot set key {} inside () — the enclosing key or field is missing (in `m[a][b] = v`, m[a] must exist: set `m[a] = map()` first, or write `m[a] = map(b, v)`)",
                    crate::interpreter::short_value(&args[1]))))),
                Some(other) if args.len() == 3 => Some(Err(RuntimeError::TypeError(format!(
                    "cannot set key {} inside {} {} — only a Map, a List or a record holds keys",
                    crate::interpreter::short_value(&args[1]), crate::interpreter::value_type_name(other), crate::interpreter::short_value(other))))),
                _ => Some(Err(RuntimeError::TypeError(
                    "with expects (map, key, value) or (list, index, value)".to_string()))),
            }
        }
        "without" => {
            // `()` is no key: it deleted the entry "null"
            if matches!(args.first(), Some(Value::Map(_))) && args[1..].iter().any(|a| matches!(a, Value::Unit)) {
                return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: "without(map, ()): the key is () — an absent value (a mistyped field?) cannot be a key".to_string() }));
            }
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
            // merge(5, m) answered 5 and merge(m, [1]) answered m unchanged;
            // a third map was ignored
            if args.len() < 2 {
                return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: "merge(a: Map, b: Map, …) takes at least two maps".to_string() }));
            }
            let mut result: IndexMap<String, Value> = IndexMap::new();
            for (i, a) in args.iter().enumerate() {
                let Value::Map(m) = a else {
                    return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                        "merge(a: Map, b: Map, …): argument {} is {} {}, not a Map", i + 1,
                        crate::interpreter::value_type_name(a), super::string::to_json_string(a)) }));
                };
                for (key, val) in m {
                    result.insert(key.clone(), val.clone());
                }
            }
            Some(Ok(Value::Map(result)))
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
                    // the separator is text (`join(xs, ())` put "null" between)
                    let Value::String(sep) = &args[1] else {
                        return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                            "join(list, sep): the separator is a String, got {} {}", crate::interpreter::value_type_name(&args[1]), args[1]) }));
                    };
                    let parts: Vec<String> = items.iter().map(|v| format!("{}", v)).collect();
                    Some(Ok(Value::String(parts.join(sep))))
                } else {
                    // `join(map("a", 1), ",")` printed the map, `join((), ",")`
                    // "null": the text of a non-list is not its elements joined
                    Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                        "join(list, sep): needs a List, got {}", crate::interpreter::value_type_name(&args[0])) }))
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
            } else { not_a_list("flatten(list)", args.first()) }
        }
        "zip" => {
            if args.len() >= 2 {
                if let (Value::List(a), Value::List(b)) = (&args[0], &args[1]) {
                    let result: Vec<Value> = a.iter().zip(b.iter()).map(|(l, r)| {
                        map_from_pairs(vec![("left".to_string(), l.clone()), ("right".to_string(), r.clone())])
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { not_a_list("zip(a, b)", args[..2].iter().find(|v| !matches!(v, Value::List(_)))) }
            } else { Some(Err(RuntimeError::TypeError("zip expects (list, list)".to_string()))) }
        }
        "enumerate" => {
            if let Some(Value::List(items)) = args.first() {
                let result: Vec<Value> = items.iter().enumerate().map(|(i, v)| {
                    map_from_pairs(vec![("index".to_string(), Value::Int(SomaInt::from_i64(i as i64))), ("value".to_string(), v.clone())])
                }).collect();
                Some(Ok(Value::List(result)))
            } else { not_a_list("enumerate(list)", args.first()) }
        }
        "render_each" => {
            if args.len() >= 2 {
                if let (Value::List(items), Value::String(template)) = (&args[0], &args[1]) {
                    let mut result = String::with_capacity(template.len() * items.len());
                    for item in items {
                        // records too (a List<Row> rendered ""); any other
                        // item was skipped without a word
                        let Some(entries) = super::pipeline::row_fields(item) else {
                            return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                                "render_each(rows, template): each row is a Map or a record, got {} {}", crate::interpreter::value_type_name(item), item) }));
                        };
                        {
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
        "inner_join" | "left_join" => {
            let what = if name == "inner_join" { "join" } else { "left_join" };
            if args.len() >= 3 {
                if let (Value::List(left), Value::List(right)) = (&args[0], &args[1]) {
                    let key = format!("{}", args[2]);
                    // the FIRST right row with an equal key (as text) wins;
                    // indexed once — a scan per left row made a 20 000-row
                    // join take 8 s (quadratic)
                    let mut index: std::collections::HashMap<String, &Value> = std::collections::HashMap::new();
                    for r in right {
                        if let Some(e) = super::pipeline::row_fields(r) {
                            if let Some(v) = e.get(&key) {
                                index.entry(format!("{}", v)).or_insert(r);
                            }
                        }
                    }
                    let merge = |l: &Value, r: Option<&&Value>| -> Value {
                        let mut merged = super::pipeline::row_fields(l).cloned().unwrap_or_default();
                        if let Some(re) = r.and_then(|r| super::pipeline::row_fields(r)) {
                            for (rk, rv) in re {
                                if rk != &key && !merged.contains_key(rk) {
                                    merged.insert(rk.clone(), rv.clone());
                                }
                            }
                        }
                        Value::Map(merged)
                    };
                    let result: Vec<Value> = if name == "inner_join" {
                        left.iter().filter_map(|l| {
                            let lk = super::pipeline::row_fields(l).and_then(|e| e.get(&key)).map(|v| format!("{}", v));
                            lk.and_then(|lk_val| index.get(&lk_val)).map(|r| merge(l, Some(r)))
                        }).collect()
                    } else {
                        left.iter().map(|l| {
                            let lk = super::pipeline::row_fields(l).and_then(|e| e.get(&key)).map(|v| format!("{}", v));
                            merge(l, lk.and_then(|lk_val| index.get(&lk_val)))
                        }).collect()
                    };
                    Some(Ok(Value::List(result)))
                } else { Some(Err(RuntimeError::TypeError(format!("{} expects (list, list, key)", what)))) }
            } else { Some(Err(RuntimeError::TypeError(format!("{} expects (list, list, key)", what)))) }
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
                let desc = match super::sort_descending("sort", args.get(1)) { Ok(d) => d, Err(e) => return Some(Err(e)) };
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

/// The error for a list builtin handed something else (`zip([1], "ab")`,
/// `enumerate("ab")` and `flatten(5)` answered [] in silence).
fn not_a_list(call: &str, got: Option<&Value>) -> Option<Result<Value, RuntimeError>> {
    Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!(
        "{} takes a List, got {}", call,
        got.map(|v| format!("{} {}", crate::interpreter::value_type_name(v), super::string::to_json_string(v))).unwrap_or_else(|| "nothing".to_string())) }))
}

/// The keys sort_by orders: numbers, texts, Bools, Lists of them, and ()
/// (the documented rank: () < Bool < number < text < List). A Map or record
/// key compared "equal" to every other and the rows came back unsorted
/// without a word — sort() refuses them.
pub(crate) fn sort_keys_ok(name: &str, keys: &[&Value]) -> Result<(), RuntimeError> {
    for k in keys {
        if matches!(k, Value::Map(_) | Value::Variant { .. } | Value::Lambda { .. } | Value::LambdaBlock { .. }) {
            return Err(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                "{}: a key is {} — keys are numbers, texts, Bools or Lists of them (order by one of its fields)", name, crate::interpreter::value_type_name(k)) });
        }
    }
    Ok(())
}
