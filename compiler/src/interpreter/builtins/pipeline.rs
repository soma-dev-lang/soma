use super::super::{Value, RuntimeError};
use super::{val_to_i64, val_to_f64, map_field_i64, map_field_f64};

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
                        let val = entries.iter().find(|(k, _)| k == &field).map(|(_, v)| v);
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
                        entries.iter().any(|(k, v)| k == &field && matches!(v, Value::Float(_)))
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
        "top" | "take" => {
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
                    Some(Ok(Value::Int(total)))
                } else { Some(Ok(Value::Int(0))) }
            } else {
                Some(Err(RuntimeError::TypeError("sum_by expects (list, field)".to_string())))
            }
        }
        "avg_by" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    if items.is_empty() { return Some(Ok(Value::Unit)); }
                    let total: i64 = items.iter().map(|item| map_field_i64(item, &field)).sum();
                    Some(Ok(Value::Int(total / items.len() as i64)))
                } else { Some(Ok(Value::Int(0))) }
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
        "map_by" | "pluck" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let result: Vec<Value> = items.iter().map(|item| {
                        if let Value::Map(entries) = item {
                            entries.iter().find(|(k, _)| k == &field)
                                .map(|(_, v)| v.clone()).unwrap_or(Value::Unit)
                        } else { Value::Unit }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(Value::List(vec![]))) }
            } else {
                Some(Err(RuntimeError::TypeError("map_by expects (list, field)".to_string())))
            }
        }
        "count_by" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let target = format!("{}", args[2]);
                    let count = items.iter().filter(|item| {
                        if let Value::Map(entries) = item {
                            entries.iter().any(|(k, v)| k == &field && format!("{}", v) == target)
                        } else { false }
                    }).count();
                    Some(Ok(Value::Int(count as i64)))
                } else { Some(Ok(Value::Int(0))) }
            } else {
                Some(Err(RuntimeError::TypeError("count_by expects (list, field, value)".to_string())))
            }
        }
        "group_by" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
                    for item in items {
                        let key = if let Value::Map(entries) = item {
                            entries.iter().find(|(k, _)| k == &field)
                                .map(|(_, v)| format!("{}", v)).unwrap_or("unknown".to_string())
                        } else { "unknown".to_string() };
                        if let Some(group) = groups.iter_mut().find(|(k, _)| k == &key) {
                            group.1.push(item.clone());
                        } else {
                            groups.push((key, vec![item.clone()]));
                        }
                    }
                    let result: Vec<(String, Value)> = groups.into_iter()
                        .map(|(k, v)| (k, Value::List(v))).collect();
                    Some(Ok(Value::Map(result)))
                } else { Some(Ok(Value::Map(vec![]))) }
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
                            e.iter().find(|(k,_)| k == &field).map(|(_,v)| v.clone()).unwrap_or(Value::Unit)
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
        "select" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let fields: Vec<String> = args[1..].iter().map(|a| format!("{}", a)).collect();
                    let result: Vec<Value> = items.iter().map(|item| {
                        if let Value::Map(entries) = item {
                            let picked: Vec<(String, Value)> = entries.iter()
                                .filter(|(k,_)| fields.contains(k))
                                .cloned().collect();
                            Value::Map(picked)
                        } else { item.clone() }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(args[0].clone())) }
            } else { Some(Err(RuntimeError::TypeError("select expects (list, fields...)".to_string()))) }
        }
        "agg" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let group_field = format!("{}", args[1]);
                    let agg_specs: Vec<(String, String)> = args[2..].iter().map(|a| {
                        let s = format!("{}", a);
                        let parts: Vec<&str> = s.splitn(2, ':').collect();
                        if parts.len() == 2 { (parts[0].to_string(), parts[1].to_string()) }
                        else { (s.clone(), "count".to_string()) }
                    }).collect();

                    let mut groups: Vec<(String, Vec<&Value>)> = Vec::new();
                    for item in items {
                        let gk = if let Value::Map(e) = item {
                            e.iter().find(|(k,_)| k == &group_field).map(|(_,v)| format!("{}", v)).unwrap_or("null".to_string())
                        } else { "null".to_string() };
                        if let Some(g) = groups.iter_mut().find(|(k,_)| k == &gk) {
                            g.1.push(item);
                        } else {
                            groups.push((gk, vec![item]));
                        }
                    }

                    let result: Vec<Value> = groups.iter().map(|(gk, items)| {
                        let mut row = vec![(group_field.clone(), Value::String(gk.clone())), ("count".to_string(), Value::Int(items.len() as i64))];
                        for (col, func) in &agg_specs {
                            let vals: Vec<i64> = items.iter().map(|i| map_field_i64(i, col)).collect();
                            let agg_val = match func.as_str() {
                                "sum" => Value::Int(vals.iter().sum()),
                                "avg" => Value::Int(if vals.is_empty() { 0 } else { vals.iter().sum::<i64>() / vals.len() as i64 }),
                                "min" => Value::Int(vals.iter().copied().min().unwrap_or(0)),
                                "max" => Value::Int(vals.iter().copied().max().unwrap_or(0)),
                                "count" => Value::Int(vals.len() as i64),
                                _ => Value::Int(vals.iter().sum()),
                            };
                            row.push((format!("{}_{}", col, func), agg_val));
                        }
                        Value::Map(row)
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Err(RuntimeError::TypeError("agg expects (list, group_field, specs...)".to_string()))) }
            } else { Some(Err(RuntimeError::TypeError("agg expects at least 3 args".to_string()))) }
        }
        "with_column" | "add_field" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let new_field = format!("{}", args[1]);
                    let source = format!("{}", args[2]);
                    let op = if args.len() >= 4 { format!("{}", args[3]) } else { "=".to_string() };
                    let operand = if args.len() >= 5 { val_to_i64(&args[4]) } else { 0 };
                    let result: Vec<Value> = items.iter().map(|item| {
                        if let Value::Map(entries) = item {
                            let src_val = entries.iter().find(|(k,_)| k == &source).map(|(_,v)| val_to_i64(v)).unwrap_or(0);
                            let computed = match op.as_str() { "*" => src_val * operand, "+" => src_val + operand, "-" => src_val - operand, "/" => if operand != 0 { src_val / operand } else { 0 }, _ => src_val };
                            let mut e = entries.clone(); e.push((new_field.clone(), Value::Int(computed))); Value::Map(e)
                        } else { item.clone() }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(args[0].clone())) }
            } else { Some(Err(RuntimeError::TypeError("with_column(list, name, source, op, value)".to_string()))) }
        }
        "describe" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let vals: Vec<i64> = items.iter().map(|i| map_field_i64(i, &field)).collect();
                    let n = vals.len() as i64; let sum: i64 = vals.iter().sum();
                    let avg = if n > 0 { sum / n } else { 0 };
                    Some(Ok(Value::Map(vec![("count".into(), Value::Int(n)), ("sum".into(), Value::Int(sum)), ("avg".into(), Value::Int(avg)),
                        ("min".into(), Value::Int(vals.iter().copied().min().unwrap_or(0))), ("max".into(), Value::Int(vals.iter().copied().max().unwrap_or(0)))])))
                } else { Some(Ok(Value::Unit)) }
            } else { Some(Err(RuntimeError::TypeError("describe(list, field)".to_string()))) }
        }
        "sample" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let n = val_to_i64(&args[1]) as usize;
                    if n >= items.len() { return Some(Ok(Value::List(items.clone()))); }
                    let step = items.len() / n;
                    Some(Ok(Value::List((0..n).map(|i| items[i * step].clone()).collect())))
                } else { Some(Ok(args[0].clone())) }
            } else { Some(Err(RuntimeError::TypeError("sample(list, n)".to_string()))) }
        }
        "window" | "rolling" => {
            if args.len() >= 4 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]); let size = val_to_i64(&args[2]) as usize; let func = format!("{}", args[3]);
                    let out = format!("{}_{}{}", field, func, size);
                    let vals: Vec<i64> = items.iter().map(|i| map_field_i64(i, &field)).collect();
                    let result: Vec<Value> = items.iter().enumerate().map(|(i, item)| {
                        let start = if i >= size { i - size + 1 } else { 0 };
                        let w = &vals[start..=i];
                        let v = match func.as_str() { "avg" => w.iter().sum::<i64>() / w.len() as i64, "sum" => w.iter().sum(), "min" => w.iter().copied().min().unwrap_or(0), "max" => w.iter().copied().max().unwrap_or(0), _ => 0 };
                        if let Value::Map(e) = item { let mut ne = e.clone(); ne.push((out.clone(), Value::Int(v))); Value::Map(ne) } else { item.clone() }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(args[0].clone())) }
            } else { Some(Err(RuntimeError::TypeError("window(list, field, size, func)".to_string()))) }
        }
        "cumsum" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]); let out = format!("{}_cumsum", field);
                    let mut cum = 0i64;
                    let result: Vec<Value> = items.iter().map(|item| {
                        cum += map_field_i64(item, &field);
                        if let Value::Map(e) = item { let mut ne = e.clone(); ne.push((out.clone(), Value::Int(cum))); Value::Map(ne) } else { item.clone() }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(args[0].clone())) }
            } else { Some(Err(RuntimeError::TypeError("cumsum(list, field)".to_string()))) }
        }
        "add_rank" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]); let desc = args.get(2).map(|v| format!("{}", v) == "desc").unwrap_or(true);
                    let mut idx: Vec<(usize, i64)> = items.iter().enumerate().map(|(i, item)| (i, map_field_i64(item, &field))).collect();
                    idx.sort_by(|a, b| if desc { b.1.cmp(&a.1) } else { a.1.cmp(&b.1) });
                    let mut ranks = vec![0usize; items.len()];
                    for (r, (i, _)) in idx.iter().enumerate() { ranks[*i] = r + 1; }
                    let result: Vec<Value> = items.iter().enumerate().map(|(i, item)| {
                        if let Value::Map(e) = item { let mut ne = e.clone(); ne.push(("_rank".into(), Value::Int(ranks[i] as i64))); Value::Map(ne) } else { item.clone() }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(args[0].clone())) }
            } else { Some(Err(RuntimeError::TypeError("add_rank(list, field)".to_string()))) }
        }
        "percentile" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let pct = match &args[2] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.5 };
                    let mut vals: Vec<f64> = items.iter().map(|i| map_field_f64(i, &field)).collect();
                    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    if vals.is_empty() { return Some(Ok(Value::Unit)); }
                    let idx = ((vals.len() as f64 - 1.0) * pct).round() as usize;
                    let idx = idx.min(vals.len() - 1);
                    Some(Ok(Value::Float(vals[idx])))
                } else { Some(Ok(Value::Unit)) }
            } else { Some(Err(RuntimeError::TypeError("percentile(list, field, pct)".to_string()))) }
        }
        "median" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let mut vals: Vec<f64> = items.iter().map(|i| map_field_f64(i, &field)).collect();
                    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    if vals.is_empty() { return Some(Ok(Value::Unit)); }
                    let idx = ((vals.len() as f64 - 1.0) * 0.5).round() as usize;
                    let idx = idx.min(vals.len() - 1);
                    Some(Ok(Value::Float(vals[idx])))
                } else { Some(Ok(Value::Unit)) }
            } else { Some(Err(RuntimeError::TypeError("median(list, field)".to_string()))) }
        }
        "std_by" | "stdev" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let vals: Vec<f64> = items.iter().map(|i| map_field_f64(i, &field)).collect();
                    let n = vals.len() as f64;
                    if n == 0.0 { return Some(Ok(Value::Unit)); }
                    let mean = vals.iter().sum::<f64>() / n;
                    let variance = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
                    Some(Ok(Value::Float(variance.sqrt())))
                } else { Some(Ok(Value::Unit)) }
            } else { Some(Err(RuntimeError::TypeError("std_by(list, field)".to_string()))) }
        }
        "zscore" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let vals: Vec<f64> = items.iter().map(|i| map_field_f64(i, &field)).collect();
                    let n = vals.len() as f64;
                    if n == 0.0 { return Some(Ok(Value::List(vec![]))); }
                    let mean = vals.iter().sum::<f64>() / n;
                    let std = (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
                    let result: Vec<Value> = items.iter().enumerate().map(|(i, item)| {
                        let z = if std > 0.0 { (vals[i] - mean) / std } else { 0.0 };
                        if let Value::Map(entries) = item {
                            let mut new_entries = entries.clone();
                            new_entries.push((format!("{}_z", field), Value::Float(z)));
                            Value::Map(new_entries)
                        } else { item.clone() }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(Value::List(vec![]))) }
            } else { Some(Err(RuntimeError::TypeError("zscore(list, field)".to_string()))) }
        }
        "rank" => {
            if args.len() >= 2 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let mut indexed: Vec<(usize, f64)> = items.iter().enumerate()
                        .map(|(i, item)| (i, map_field_f64(item, &field))).collect();
                    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    let mut ranks = vec![0i64; items.len()];
                    for (rank_pos, (idx, _)) in indexed.iter().enumerate() {
                        ranks[*idx] = (rank_pos + 1) as i64;
                    }
                    let result: Vec<Value> = items.iter().enumerate().map(|(i, item)| {
                        if let Value::Map(entries) = item {
                            let mut new = entries.clone();
                            new.push((format!("{}_rank", field), Value::Int(ranks[i])));
                            Value::Map(new)
                        } else { item.clone() }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(Value::List(vec![]))) }
            } else { Some(Err(RuntimeError::TypeError("rank(list, field)".to_string()))) }
        }
        "normalize" => {
            if args.len() >= 4 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let min_val = match &args[2] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.0 };
                    let max_val = match &args[3] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 1.0 };
                    let vals: Vec<f64> = items.iter().map(|i| map_field_f64(i, &field)).collect();
                    let data_min = vals.iter().cloned().fold(f64::INFINITY, f64::min);
                    let data_max = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                    let range = data_max - data_min;
                    let result: Vec<Value> = items.iter().enumerate().map(|(i, item)| {
                        let scaled = if range > 0.0 {
                            (vals[i] - data_min) / range * (max_val - min_val) + min_val
                        } else { min_val };
                        if let Value::Map(entries) = item {
                            let mut new = entries.clone();
                            new.push((format!("{}_norm", field), Value::Float(scaled)));
                            Value::Map(new)
                        } else { item.clone() }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(Value::List(vec![]))) }
            } else { Some(Err(RuntimeError::TypeError("normalize(list, field, min, max)".to_string()))) }
        }
        "winsorize" => {
            if args.len() >= 4 {
                if let Value::List(items) = &args[0] {
                    let field = format!("{}", args[1]);
                    let lo_pct = match &args[2] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.05 };
                    let hi_pct = match &args[3] { Value::Float(n) => *n, Value::Int(n) => *n as f64, _ => 0.95 };
                    let mut sorted_vals: Vec<f64> = items.iter().map(|i| map_field_f64(i, &field)).collect();
                    sorted_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    if sorted_vals.is_empty() { return Some(Ok(Value::List(vec![]))); }
                    let lo_idx = ((sorted_vals.len() as f64 - 1.0) * lo_pct).round() as usize;
                    let hi_idx = ((sorted_vals.len() as f64 - 1.0) * hi_pct).round() as usize;
                    let lo_val = sorted_vals[lo_idx.min(sorted_vals.len() - 1)];
                    let hi_val = sorted_vals[hi_idx.min(sorted_vals.len() - 1)];
                    let result: Vec<Value> = items.iter().map(|item| {
                        if let Value::Map(entries) = item {
                            let new_entries: Vec<(String, Value)> = entries.iter().map(|(k, v)| {
                                if k == &field {
                                    let val = map_field_f64(item, &field);
                                    let clamped = val.max(lo_val).min(hi_val);
                                    (k.clone(), Value::Float(clamped))
                                } else { (k.clone(), v.clone()) }
                            }).collect();
                            Value::Map(new_entries)
                        } else { item.clone() }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(Value::List(vec![]))) }
            } else { Some(Err(RuntimeError::TypeError("winsorize(list, field, lo_pct, hi_pct)".to_string()))) }
        }
        "rename" => {
            if args.len() >= 3 {
                if let Value::List(items) = &args[0] {
                    let old = format!("{}", args[1]);
                    let new_name = format!("{}", args[2]);
                    let result: Vec<Value> = items.iter().map(|item| {
                        if let Value::Map(entries) = item {
                            let renamed: Vec<(String, Value)> = entries.iter().map(|(k, v)| {
                                if k == &old { (new_name.clone(), v.clone()) } else { (k.clone(), v.clone()) }
                            }).collect();
                            Value::Map(renamed)
                        } else { item.clone() }
                    }).collect();
                    Some(Ok(Value::List(result)))
                } else { Some(Ok(args[0].clone())) }
            } else { Some(Err(RuntimeError::TypeError("rename(list, old_name, new_name)".to_string()))) }
        }
        _ => None,
    }
}
