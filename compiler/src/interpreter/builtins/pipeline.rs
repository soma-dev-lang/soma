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
        Value::String(s) => s.trim().parse::<rug::Integer>().ok().map(Num::I)
            .or_else(|| s.trim().parse::<f64>().ok().filter(|f| f.is_finite()).map(Num::F)),
        _ => None,
    }
}

fn num_f(n: &Num) -> f64 { match n { Num::I(i) => SomaInt::from_rug(i.clone()).to_f64(), Num::F(f) => *f } }

fn num_value(n: Num) -> Value { match n { Num::I(i) => Value::Int(SomaInt::from_rug(i)), Num::F(f) => Value::Float(f) } }

fn num_cmp(a: &Num, b: &Num) -> std::cmp::Ordering {
    match (a, b) {
        (Num::I(x), Num::I(y)) => x.cmp(y),
        _ => num_f(a).partial_cmp(&num_f(b)).unwrap_or(std::cmp::Ordering::Equal),
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
    if xs.is_empty() { return Value::Unit; }
    match num_sum(xs) {
        Num::I(t) => {
            let n = rug::Integer::from(xs.len());
            let (q, r) = t.clone().div_rem(n.clone());
            if r == 0 { Value::Int(SomaInt::from_rug(q)) } else { Value::Float(num_f(&Num::I(t)) / xs.len() as f64) }
        }
        Num::F(f) => Value::Float(f / xs.len() as f64),
    }
}

fn field_num(item: &Value, field: &str) -> Option<Num> {
    if let Value::Map(e) = item { e.get(field).and_then(num_of) } else { None }
}

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
                    let Value::Map(entries) = item else { return false };
                    let Some(val) = entries.get(&field) else { return false };
                    // numbers compare exactly (BigInt included); other values as text
                    match (num_of(val), num_of(threshold)) {
                        (Some(a), Some(b)) if !matches!(val, Value::String(_)) => {
                            let o = num_cmp(&a, &b);
                            match op.as_str() {
                                ">" => o.is_gt(), ">=" => o.is_ge(), "<" => o.is_lt(), "<=" => o.is_le(),
                                "==" | "=" => o.is_eq(), "!=" => !o.is_eq(), _ => false,
                            }
                        }
                        _ => match op.as_str() {
                            "==" | "=" => format!("{}", val) == format!("{}", threshold),
                            "!=" => format!("{}", val) != format!("{}", threshold),
                            _ => false,
                        },
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
                let desc = args.get(2).map(|v| format!("{}", v) == "desc").unwrap_or(false);
                // Generic, stable ordering of the field values: numbers by
                // value, strings lexicographically (they used to all compare
                // as 0, i.e. not sort at all).
                let field_of = |row: &Value| -> Value {
                    match row {
                        Value::Map(entries) => entries.get(&field).cloned().unwrap_or(Value::Unit),
                        _ => Value::Unit,
                    }
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
                    let mut best: Option<(Num, &Value)> = None;
                    for it in items {
                        let Some(n) = field_num(it, &field) else { continue };
                        let better = match &best { None => true, Some((b, _)) => if is_max { num_cmp(&n, b).is_gt() } else { num_cmp(&n, b).is_lt() } };
                        if better { best = Some((n, it)); }
                    }
                    Some(Ok(best.map(|(_, v)| v.clone()).unwrap_or(Value::Unit)))
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
        // uniqBy: the first ROW per distinct value of `field`
        "distinct_by" | "unique_by" => {
            if let (Some(Value::List(items)), Some(field)) = (args.first(), args.get(1)) {
                let field = format!("{}", field);
                let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut result: Vec<Value> = Vec::new();
                for item in items {
                    let v = if let Value::Map(e) = item { e.get(&field).cloned().unwrap_or(Value::Unit) } else { item.clone() };
                    if seen.insert(format!("{}", v)) {
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
                        let v = if let Value::Map(e) = item {
                            e.get(&field).cloned().unwrap_or(Value::Unit)
                        } else { item.clone() };
                        if seen.insert(distinct_key(&v)) {
                            result.push(v);
                        }
                    }
                    Some(Ok(Value::List(result)))
                } else {
                    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
                    let result: Vec<Value> = items.iter().filter(|item| seen.insert(distinct_key(item))).cloned().collect();
                    Some(Ok(Value::List(result)))
                }
            } else { Some(Ok(Value::List(vec![]))) }
        }
        "count_by" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let target = format!("{}", args[2]);
                    let count = items.iter().filter(|item| {
                        if let Value::Map(entries) = item {
                            entries.get(&field).map(|v| format!("{}", v) == target).unwrap_or(false)
                        } else { false }
                    }).count() as i64;
                    Some(Ok(Value::Int(SomaInt::from_i64(count))))
                } else { Some(Ok(Value::Int(SomaInt::from_i64(0)))) }
            } else {
                Some(Err(RuntimeError::TypeError("count_by expects (list, field, value)".to_string())))
            }
        }
        "select" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let fields: Vec<String> = args[1..].iter().map(|a| format!("{}", a)).collect();
                    let result: Vec<Value> = items.iter().map(|item| {
                        if let Value::Map(entries) = item {
                            let filtered: IndexMap<String, Value> = fields.iter()
                                .filter_map(|f| entries.get(f).map(|v| (f.clone(), v.clone())))
                                .collect();
                            Value::Map(filtered)
                        } else { item.clone() }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(Value::List(vec![]))) }
            } else {
                Some(Err(RuntimeError::TypeError("select expects (list, field1, field2, ...)".to_string())))
            }
        }
        "agg" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let group_field = format!("{}", args[1]);
                    let ops: Vec<String> = args[2..].iter().map(|a| format!("{}", a)).collect();
                    let mut groups: IndexMap<String, Vec<&Value>> = IndexMap::new();
                    for item in items {
                        let key = if let Value::Map(entries) = item {
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
                                    "count" => Value::Int(SomaInt::from_i64(group.iter().filter(|item| matches!(item, Value::Map(e) if e.get(col).map_or(false, |v| !matches!(v, Value::Unit)))).count() as i64)),
                                    _ => Value::Unit,
                                };
                                row.insert(format!("{}_{}", col, func), agg_val);
                            }
                        }
                        Value::Map(row)
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(Value::List(vec![]))) }
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
        Value::Map(m) => m.get(field).filter(|v| !matches!(v, Value::Unit) && !matches!(v, Value::String(s) if s.trim().is_empty()) && num_of(v).is_none()).cloned(),
        _ => None,
    })
}

fn distinct_key(v: &Value) -> String {
    format!("{}\u{1f}{}", crate::interpreter::value_type_name(v), crate::interpreter::builtins::string::to_json_string(v))
}
