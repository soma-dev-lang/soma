use super::super::{Value, RuntimeError};
use super::{val_to_i64, val_to_f64, map_field_i64, map_field_f64};
use crate::interpreter::soma_int::SomaInt;
use indexmap::IndexMap;


/// A number from a row field: exact Int (BigInt included) or Float. The
/// aggregations went through i64 (9.99 summed as 9, 2^70 as 0).
#[derive(Clone)]
enum Num { I(rug::Integer), F(f64) }

fn num_of(v: &Value) -> Option<Num> {
    match v {
        Value::Int(si) => Some(Num::I(si.to_rug())),
        Value::Float(f) => Some(Num::F(*f)),
        // "1 2" is not a number (rug's parser read it as 12)
        Value::String(s) => super::string::parse_int_text(s.trim()).map(Num::I)
            .or_else(|| s.trim().parse::<f64>().ok().filter(|f| f.is_finite()).map(Num::F)),
        _ => None,
    }
}

fn num_f(n: &Num) -> f64 { match n { Num::I(i) => SomaInt::from_rug(i.clone()).to_f64(), Num::F(f) => *f } }

fn num_value(n: Num) -> Value { match n { Num::I(i) => Value::Int(SomaInt::from_rug(i)), Num::F(f) => Value::Float(f) } }

fn num_cmp(a: &Num, b: &Num) -> std::cmp::Ordering {
    num_partial_cmp(a, b).unwrap_or_else(|| {
        let nan = |n: &Num| matches!(n, Num::F(f) if f.is_nan());
        nan(a).cmp(&nan(b))
    })
}

fn num_partial_cmp(a: &Num, b: &Num) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Num::I(x), Num::I(y)) => Some(x.cmp(y)),
        (Num::I(x), Num::F(y)) => x.partial_cmp(y),
        (Num::F(x), Num::I(y)) => y.partial_cmp(x).map(std::cmp::Ordering::reverse),
        (Num::F(x), Num::F(y)) => x.partial_cmp(y),
    }
}

fn num_sum(xs: &[Num]) -> Num {
    if xs.iter().all(|n| matches!(n, Num::I(_))) {
        let mut t = rug::Integer::new();
        for n in xs { if let Num::I(i) = n { t += i; } }
        Num::I(t)
    } else {
        Num::F(xs.iter().map(num_f).sum())
    }
}

/// the mean: an Int when it is exact, else a Float (like 7 / 2 = 3.5)
fn num_avg(xs: &[Num]) -> Value {
    let values: Vec<Value> = xs.iter().cloned().map(num_value).collect();
    super::math::numeric_mean(&values)
}

fn field_num(item: &Value, field: &str) -> Option<Num> {
    row_fields(item).and_then(|e| e.get(field)).and_then(num_of)
}

/// The fields of a row: a Map, or a record (`Line { sku, qty }`, the row
/// type `List<Line>` parameters decode to — every field builtin answered
/// empty on them).
pub(crate) fn row_fields(item: &Value) -> Option<&IndexMap<String, Value>> {
    match item {
        Value::Map(m) => Some(m),
        Value::Variant { fields: crate::interpreter::VariantValue::Struct(m), .. } => Some(m),
        _ => None,
    }
}

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    // a field is named by a String: a lambda or a number was turned into
    // text and every row missed it (`sum_by(rows, r => r.v)` was 0)
    if matches!(name, "filter_by" | "sum_by" | "avg_by" | "min_by" | "max_by" | "pluck" | "group_by" | "count_by" | "agg" | "distinct" | "sort_by")
        && args.len() >= 2 && matches!(args[0], Value::List(_)) && !matches!(args[1], Value::String(_))
        && !(name == "sort_by" && matches!(args[1], Value::Lambda { .. } | Value::LambdaBlock { .. }))
    {
        return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!(
            "{}(rows, field): the field is a String name, got {} — for a computed value, map the rows first (`rows |> map(r => …)`)",
            name, crate::interpreter::value_type_name(&args[1])) }));
    }
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
                    let Some(entries) = row_fields(item) else { return false };
                    let Some(val) = entries.get(&field) else { return false };
                    // numbers compare exactly (BigInt included); other values as text
                    // against a NUMBER threshold, numeric text compares as a
                    // number ("99" > 100 was true as text) and a value that
                    // is no number passes no ordering ((), true were kept)
                    let numeric_threshold = matches!(threshold, Value::Int(_) | Value::Float(_));
                    if numeric_threshold && num_of(val).is_none() {
                        return op == "!=";
                    }
                    match (num_of(val), num_of(threshold)) {
                        (Some(a), Some(b)) if !matches!(val, Value::String(_)) || numeric_threshold => {
                            let Some(o) = num_partial_cmp(&a, &b) else { return op == "!="; };
                            match op.as_str() {
                                ">" => o.is_gt(), ">=" => o.is_ge(), "<" => o.is_lt(), "<=" => o.is_le(),
                                "==" | "=" => o.is_eq(), "!=" => !o.is_eq(), _ => false,
                            }
                        }
                        // text compares lexicographically, like `<` on Strings
                        // (an ISO date range answered [] silently)
                        _ => {
                            let (a, b) = (format!("{}", val), format!("{}", threshold));
                            match op.as_str() {
                                "==" | "=" => a == b,
                                "!=" => a != b,
                                ">" => a > b, ">=" => a >= b, "<" => a < b, "<=" => a <= b,
                                _ => false,
                            }
                        }
                    }
                }).cloned().collect();
                Some(Ok(Value::List(result)))
            } else {
                Some(Err(RuntimeError::TypeError("filter_by expects (list, field, op, value)".to_string())))
            }
        }
        "sort_by" => {
            if let Some(Value::List(items)) = args.first() {
                let field = if args.len() >= 2 { format!("{}", args[1]) } else { return Some(Ok(Value::List(items.clone()))); };
                let desc = match super::sort_descending("sort_by", args.get(2)) { Ok(d) => d, Err(e) => return Some(Err(e)) };
                // Generic, stable ordering of the field values: numbers by
                // value, strings lexicographically (they used to all compare
                // as 0, i.e. not sort at all).
                let field_of = |row: &Value| -> Value {
                    row_fields(row).and_then(|e| e.get(&field)).cloned().unwrap_or(Value::Unit)
                };
                let mut sorted = items.clone();
                let nan = |v: &Value| matches!(v, Value::Float(f) if f.is_nan());
                sorted.sort_by(|a, b| {
                    let (x, y) = (field_of(a), field_of(b));
                    // NaN last in "desc" too
                    match (nan(&x), nan(&y)) {
                        (true, false) => return std::cmp::Ordering::Greater,
                        (false, true) => return std::cmp::Ordering::Less,
                        _ => {}
                    }
                    let o = super::collection::compare_values(&x, &y);
                    if desc { o.reverse() } else { o }
                });
                Some(Ok(Value::List(sorted)))
            } else {
                Some(Err(RuntimeError::TypeError("sort_by expects (list, field)".to_string())))
            }
        }
        "top" | "bottom" => {
            Some((|| {
                let [Value::List(items), Value::Int(count)] = args else {
                    return Err(RuntimeError::TypeError(format!("{} expects (rows: List, n: Int)", name)));
                };
                if count.cmp(&SomaInt::from_i64(0)) < 0 {
                    return Err(RuntimeError::Domain { kind: "range".into(), message: format!("{}(): n must be nonnegative", name) });
                }
                let n = count.to_rug().to_usize().unwrap_or(usize::MAX).min(items.len());
                Ok(Value::List(if name == "top" { items[..n].to_vec() } else { items[items.len() - n..].to_vec() }))
            })())
        }
        "sum_by" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    if let Some(bad) = non_numeric(items, &field) {
                        return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("sum_by(): field '{}' holds {} {} — not a number (sum_by counts numbers and numeric text only; a missing field counts 0)", field, crate::interpreter::value_type_name(&bad), crate::interpreter::builtins::string::to_json_string(&bad)) }));
                    }
                    let xs: Vec<Num> = items.iter().filter_map(|it| field_num(it, &field)).collect();
                    Some(Ok(num_value(num_sum(&xs))))
                } else if matches!(args[0], Value::Unit) { Some(Ok(Value::Int(SomaInt::from_i64(0)))) }
                // `sum_by("abc", "a")` answered 0
                else { Some(Err(RuntimeError::TypeError(format!("sum_by(rows, field) needs a List, got {}", crate::interpreter::value_type_name(&args[0]))))) }
            } else {
                Some(Err(RuntimeError::TypeError("sum_by expects (list, field)".to_string())))
            }
        }
        "avg_by" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    if let Some(bad) = non_numeric(items, &field) {
                        return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("avg_by(): field '{}' holds {} {} — not a number (avg_by counts numbers and numeric text only)", field, crate::interpreter::value_type_name(&bad), crate::interpreter::builtins::string::to_json_string(&bad)) }));
                    }
                    let xs: Vec<Num> = items.iter().filter_map(|it| field_num(it, &field)).collect();
                    Some(Ok(num_avg(&xs)))
                } else if matches!(args[0], Value::Unit) { Some(Ok(Value::Unit)) }
                // a Map or a String answered an empty result (`distinct(m)` was [])
                else { Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("avg_by(): needs a List of rows, got {}", crate::interpreter::value_type_name(&args[0])) })) }
            } else {
                Some(Err(RuntimeError::TypeError("avg_by expects (list, field)".to_string())))
            }
        }
        "min_by" | "max_by" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let is_max = name == "max_by";
                    let mut best: Option<(Num, &Value)> = None;
                    // a value that is no number is refused, as agg does (it
                    // was skipped: max_by over "n/a", 3, 1 gave the 3 row)
                    if let Some(bad) = non_numeric(items, &field) {
                        return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("{}(): field '{}' holds {} {} — not a number (a missing field is skipped)", name, field, crate::interpreter::value_type_name(&bad), crate::interpreter::builtins::string::to_json_string(&bad)) }));
                    }
                    for it in items {
                        let Some(n) = field_num(it, &field) else { continue };
                        let better = match &best { None => true, Some((b, _)) => if is_max { num_cmp(&n, b).is_gt() } else { num_cmp(&n, b).is_lt() } };
                        if better { best = Some((n, it)); }
                    }
                    Some(Ok(best.map(|(_, v)| v.clone()).unwrap_or(Value::Unit)))
                } else if matches!(args[0], Value::Unit) { Some(Ok(Value::Unit)) }
                // a Map or a String answered an empty result (`distinct(m)` was [])
                else { Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("min_by/max_by(): needs a List of rows, got {}", crate::interpreter::value_type_name(&args[0])) })) }
            } else {
                Some(Err(RuntimeError::TypeError("min_by/max_by expects (list, field)".to_string())))
            }
        }
        "pluck" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let result: Vec<Value> = items.iter().map(|item| {
                        if let Some(entries) = row_fields(item) {
                            entries.get(&field).cloned().unwrap_or(Value::Unit)
                        } else { Value::Unit }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else if matches!(args[0], Value::Unit) { Some(Ok(Value::List(vec![]))) }
                // a Map or a String answered an empty result (`distinct(m)` was [])
                else { Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("pluck(): needs a List of rows, got {}", crate::interpreter::value_type_name(&args[0])) })) }
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
                        let key = if let Some(entries) = row_fields(item) {
                            entries.get(&field)
                                .map(|v| format!("{}", v)).unwrap_or("unknown".to_string())
                        } else { "unknown".to_string() };
                        groups.entry(key).or_default().push(item.clone());
                    }
                    let result: IndexMap<String, Value> = groups.into_iter()
                        .map(|(k, v)| (k, Value::List(v))).collect();
                    Some(Ok(Value::Map(result)))
                } else if matches!(args[0], Value::Unit) { Some(Ok(Value::Map(IndexMap::new()))) }
                // a Map or a String answered an empty result (`distinct(m)` was [])
                else { Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("group_by(): needs a List of rows, got {}", crate::interpreter::value_type_name(&args[0])) })) }
            } else {
                Some(Err(RuntimeError::TypeError("group_by expects (list, field)".to_string())))
            }
        }
        // uniqBy: the first ROW per distinct value of `field`
        "distinct_by" | "unique_by" => {
            if let (Some(Value::List(items)), Some(field)) = (args.first(), args.get(1)) {
                let field = format!("{}", field);
                let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut result: Vec<Value> = Vec::new();
                for item in items {
                    let v = if let Some(e) = row_fields(item) { e.get(&field).cloned().unwrap_or(Value::Unit) } else { item.clone() };
                    if distinct_key(&v).map_or(true, |key| seen.insert(key)) {
                        result.push(item.clone());
                    }
                }
                Some(Ok(Value::List(result)))
            } else {
                Some(Err(RuntimeError::TypeError("distinct_by(rows: List<Map>, field: String) -> List<Map>".to_string())))
            }
        }
        "distinct" => {
            if let Some(Value::List(items)) = args.first() {
                if let Some(field) = args.get(1) {
                    let field = format!("{}", field);
                    // keyed by kind AND text: "1" and 1 (or () and "null") are
                    // different values (the Int was dropped as a duplicate)
                    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                    let mut result: Vec<Value> = Vec::new();
                    for item in items {
                        let v = if let Some(e) = row_fields(item) {
                            e.get(&field).cloned().unwrap_or(Value::Unit)
                        } else { item.clone() };
                        if distinct_key(&v).map_or(true, |key| seen.insert(key)) {
                            result.push(v);
                        }
                    }
                    Some(Ok(Value::List(result)))
                } else {
                    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                    let result: Vec<Value> = items.iter().filter(|item| distinct_key(item).map_or(true, |key| seen.insert(key))).cloned().collect();
                    Some(Ok(Value::List(result)))
                }
            } else if matches!(args[0], Value::Unit) { Some(Ok(Value::List(vec![]))) }
            // a Map or a String answered an empty result (`distinct(m)` was [])
            else { Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("distinct(): needs a List of rows, got {}", crate::interpreter::value_type_name(&args[0])) })) }
        }
        "count_by" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let target = format!("{}", args[2]);
                    let count = items.iter().filter(|item| {
                        if let Some(entries) = row_fields(item) {
                            entries.get(&field).map(|v| format!("{}", v) == target).unwrap_or(false)
                        } else { false }
                    }).count() as i64;
                    Some(Ok(Value::Int(SomaInt::from_i64(count))))
                } else if matches!(args[0], Value::Unit) { Some(Ok(Value::Int(SomaInt::from_i64(0)))) }
                // a Map or a String answered an empty result (`distinct(m)` was [])
                else { Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("count_by(): needs a List of rows, got {}", crate::interpreter::value_type_name(&args[0])) })) }
            } else {
                Some(Err(RuntimeError::TypeError("count_by expects (list, field, value)".to_string())))
            }
        }
        "select" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    // names, or one list of them (`select(rows, ["id", "a"])` read
                    // the list's text as one name and gave `{}` per row)
                    let fields: Vec<String> = args[1..].iter().flat_map(|a| match a {
                        Value::List(xs) => xs.iter().map(|x| format!("{}", x)).collect::<Vec<_>>(),
                        other => vec![format!("{}", other)],
                    }).collect();
                    let result: Vec<Value> = items.iter().map(|item| {
                        if let Some(entries) = row_fields(item) {
                            let filtered: IndexMap<String, Value> = fields.iter()
                                .filter_map(|f| entries.get(f).map(|v| (f.clone(), v.clone())))
                                .collect();
                            Value::Map(filtered)
                        } else { item.clone() }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else if matches!(args[0], Value::Unit) { Some(Ok(Value::List(vec![]))) }
                // a Map or a String answered an empty result (`distinct(m)` was [])
                else { Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("select(): needs a List of rows, got {}", crate::interpreter::value_type_name(&args[0])) })) }
            } else {
                Some(Err(RuntimeError::TypeError("select expects (list, field1, field2, ...)".to_string())))
            }
        }
        "agg" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let group_field = format!("{}", args[1]);
                    let ops: Vec<String> = args[2..].iter().map(|a| format!("{}", a)).collect();
                    // an op without a colon, or a func outside the five, used
                    // to be dropped or answer () in silence ("v:median",
                    // "v:SUM"); a text value was skipped where sum_by refuses it
                    for op_str in &ops {
                        let Some((col, func)) = op_str.split_once(':') else {
                            return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                                "agg(): \"{}\" names no function — write \"{}:sum\" (sum, avg, min, max or count)", op_str, op_str) }));
                        };
                        if !matches!(func, "sum" | "avg" | "min" | "max" | "count") {
                            return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                                "agg(): unknown function '{}' in \"{}\" — the function is sum, avg, min, max or count", func, op_str) }));
                        }
                        if func != "count" {
                            if let Some(bad) = non_numeric(items, col) {
                                return Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!(
                                    "agg(): field '{}' holds {} {} — not a number ({} counts numbers and numeric text only)",
                                    col, crate::interpreter::value_type_name(&bad), crate::interpreter::builtins::string::to_json_string(&bad), func) }));
                            }
                        }
                    }
                    let mut groups: IndexMap<String, Vec<&Value>> = IndexMap::new();
                    for item in items {
                        let key = if let Some(entries) = row_fields(item) {
                            entries.get(&group_field).map(|v| format!("{}", v)).unwrap_or("unknown".to_string())
                        } else { "unknown".to_string() };
                        groups.entry(key).or_default().push(item);
                    }
                    let result: Vec<Value> = groups.into_iter().map(|(key, group)| {
                        let mut row: IndexMap<String, Value> = IndexMap::new();
                        row.insert(group_field.clone(), Value::String(key));
                        row.insert("count".to_string(), Value::Int(SomaInt::from_i64(group.len() as i64)));
                        for op_str in &ops {
                            if let Some(colon) = op_str.find(':') {
                                let col = &op_str[..colon];
                                let func = &op_str[colon+1..];
                                let vals: Vec<Num> = group.iter().filter_map(|item| field_num(item, col)).collect();
                                let agg_val = match func {
                                    "sum" => num_value(num_sum(&vals)),
                                    "avg" => num_avg(&vals),
                                    "min" => vals.iter().cloned().min_by(num_cmp).map(num_value).unwrap_or(Value::Unit),
                                    "max" => vals.iter().cloned().max_by(num_cmp).map(num_value).unwrap_or(Value::Unit),
                                    // every present value counts (a String id too)
                                    "count" => Value::Int(SomaInt::from_i64(group.iter().filter(|item| row_fields(item).map_or(false, |e| e.get(col).map_or(false, |v| !matches!(v, Value::Unit)))).count() as i64)),
                                    _ => Value::Unit,
                                };
                                row.insert(format!("{}_{}", col, func), agg_val);
                            }
                        }
                        Value::Map(row)
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else if matches!(args[0], Value::Unit) { Some(Ok(Value::List(vec![]))) }
                // a Map or a String answered an empty result (`distinct(m)` was [])
                else { Some(Err(RuntimeError::Domain { kind: "type".to_string(), message: format!("agg(): needs a List of rows, got {}", crate::interpreter::value_type_name(&args[0])) })) }
            } else {
                Some(Err(RuntimeError::TypeError("agg expects (list, group_field, \"col:func\", ...)".to_string())))
            }
        }
        _ => None,
    }
}

/// The first present field value that is not a number (a numeric String
/// counts as its number; an absent field or `()` is skipped).
fn non_numeric(items: &[Value], field: &str) -> Option<Value> {
    items.iter().find_map(|it| match it {
        // a blank CSV cell ("") is a missing value, as agg() treats it
        Value::Map(_) | Value::Variant { .. } => row_fields(it).and_then(|m| m.get(field)).filter(|v| !matches!(v, Value::Unit) && !matches!(v, Value::String(s) if s.trim().is_empty()) && num_of(v).is_none()).cloned(),
        _ => None,
    })
}

fn distinct_key(v: &Value) -> Option<String> {
    // Structural keys preserve exact numbers and ignore map insertion order.
    // A value containing NaN is unequal even to itself: never deduplicate it.
    fn key(v: &Value) -> Option<serde_json::Value> {
        use serde_json::json;
        Some(match v {
            Value::Int(i) => json!(["number", i.to_string()]),
            Value::Float(f) if f.is_nan() => return None,
            Value::Float(f) if f.is_finite() && f.fract() == 0.0 => json!(["number", rug::Integer::from_f64(*f)?.to_string()]),
            Value::Float(f) => json!(["float", f.to_string()]),
            Value::String(s) => json!(["string", s]),
            Value::Bool(b) => json!(["bool", b]),
            Value::Unit => json!(["unit"]),
            Value::List(xs) => json!(["list", xs.iter().map(key).collect::<Option<Vec<_>>>()?]),
            Value::Map(m) => {
                let mut pairs = m.iter().collect::<Vec<_>>();
                pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
                let entries = pairs.into_iter().map(|(k, v)| Some(json!([k, key(v)?]))).collect::<Option<Vec<_>>>()?;
                json!(["map", entries])
            }
            Value::Variant { type_name, variant, fields } => {
                use crate::interpreter::VariantValue;
                let payload = match fields {
                    VariantValue::Unit => json!(["unit"]),
                    VariantValue::Tuple(xs) => json!(["tuple", xs.iter().map(key).collect::<Option<Vec<_>>>()?]),
                    VariantValue::Struct(m) => key(&Value::Map(m.clone()))?,
                };
                json!(["variant", type_name, variant, payload])
            }
            // Preserve values with no structural key rather than dropping a
            // distinct value because its display text happened to collide.
            _ => return None,
        })
    }
    key(v).map(|k| k.to_string())
}
