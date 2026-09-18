use super::super::{Value, RuntimeError, VariantValue};
use super::json_to_value;
use crate::interpreter::soma_int::SomaInt;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "concat" => {
            if args.len() >= 2 {
                match (&args[0], &args[1]) {
                    (Value::String(a), Value::String(b)) => {
                        let mut result = String::with_capacity(a.len() + b.len());
                        result.push_str(a);
                        result.push_str(b);
                        Some(Ok(Value::String(result)))
                    }
                    // The explicit list concatenation (numeric `a + b` is
                    // elementwise, so this is how you join two lists).
                    (Value::List(a), Value::List(b)) => {
                        let mut out = a.clone();
                        out.extend(b.iter().cloned());
                        Some(Ok(Value::List(out)))
                    }
                    _ => Some(Ok(Value::String(format!("{}{}", args[0], args[1]))))
                }
            } else if args.len() == 1 {
                Some(Ok(args[0].clone()))
            } else {
                Some(Err(RuntimeError::TypeError("concat expects arguments".to_string())))
            }
        }
        "split" => {
            if args.len() >= 2 {
                if let (Value::String(s), Value::String(delim)) = (&args[0], &args[1]) {
                    let parts: Vec<Value> = s.split(delim.as_str())
                        .map(|p| Value::String(p.to_string()))
                        .collect();
                    Some(Ok(Value::List(parts)))
                } else {
                    Some(Err(RuntimeError::TypeError("split expects (string, delimiter)".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("split expects 2 arguments".to_string())))
            }
        }
        "replace" => {
            if args.len() >= 3 {
                if let (Value::String(s), Value::String(old), Value::String(new)) = (&args[0], &args[1], &args[2]) {
                    Some(Ok(Value::String(s.replace(old.as_str(), new.as_str()))))
                } else {
                    Some(Err(RuntimeError::TypeError("replace expects strings".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("replace expects 3 arguments".to_string())))
            }
        }
        "contains" => {
            if args.len() >= 2 {
                if let (Value::String(haystack), Value::String(needle)) = (&args[0], &args[1]) {
                    Some(Ok(Value::Bool(haystack.contains(needle.as_str()))))
                } else if matches!(args[0], Value::List(_) | Value::Map(_)) {
                    // list / map membership: collection.rs
                    None
                } else if matches!(args[0], Value::String(_)) {
                    // contains("abc", 1): compare against the text form
                    let Value::String(haystack) = &args[0] else { return None };
                    Some(Ok(Value::Bool(haystack.contains(format!("{}", args[1]).as_str()))))
                } else {
                    Some(Err(RuntimeError::TypeError(format!(
                        "contains(haystack, needle): haystack must be a String, List or Map, got {}",
                        crate::interpreter::value_type_name(&args[0])
                    ))))
                }
            } else {
                Some(Err(RuntimeError::TypeError("contains(string, substring)".to_string())))
            }
        }
        "starts_with" => {
            if args.len() >= 2 {
                if let (Value::String(s), Value::String(prefix)) = (&args[0], &args[1]) {
                    Some(Ok(Value::Bool(s.starts_with(prefix.as_str()))))
                } else {
                    Some(Err(RuntimeError::TypeError("starts_with expects strings".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("starts_with expects 2 arguments".to_string())))
            }
        }
        "ends_with" => {
            if args.len() >= 2 {
                if let (Value::String(s), Value::String(suffix)) = (&args[0], &args[1]) {
                    Some(Ok(Value::Bool(s.ends_with(suffix.as_str()))))
                } else {
                    Some(Err(RuntimeError::TypeError("ends_with expects strings".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("ends_with expects 2 arguments".to_string())))
            }
        }
        "lowercase" => {
            args.first().map(|a| {
                if let Value::String(s) = a {
                    Ok(Value::String(s.to_lowercase()))
                } else {
                    Ok(Value::String(format!("{}", a)))
                }
            })
        }
        "uppercase" => {
            args.first().map(|a| {
                if let Value::String(s) = a {
                    Ok(Value::String(s.to_uppercase()))
                } else {
                    Ok(Value::String(format!("{}", a)))
                }
            })
        }
        // Same functions the [native] backend compiles to the `regex` crate.
        "regex_count" | "regex_match" | "regex_replace" => {
            let (Some(Value::String(text)), Some(Value::String(pat))) = (args.first(), args.get(1)) else {
                return Some(Err(RuntimeError::TypeError(format!("{}(text: String, pattern: String{})", name, if name == "regex_replace" { ", replacement: String" } else { "" }))));
            };
            let rx = match regex::Regex::new(pat) {
                Ok(r) => r,
                Err(e) => return Some(Err(RuntimeError::TypeError(format!("{}: invalid pattern {:?}: {}", name, pat, e)))),
            };
            Some(Ok(match name {
                "regex_count" => Value::Int(SomaInt::from_i64(rx.find_iter(text).count() as i64)),
                "regex_match" => Value::Int(SomaInt::from_i64(if rx.is_match(text) { 1 } else { 0 })),
                _ => {
                    let Some(Value::String(rep)) = args.get(2) else {
                        return Some(Err(RuntimeError::TypeError("regex_replace(text, pattern, replacement)".to_string())));
                    };
                    Value::String(rx.replace_all(text, rep.as_str()).into_owned())
                }
            }))
        }
        "read_stdin" => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf).ok();
            Some(Ok(Value::String(buf)))
        }
        "write_str" => {
            use std::io::Write;
            let text = args.first().map(|v| format!("{}", v)).unwrap_or_default();
            let mut out = std::io::stdout();
            out.write_all(text.as_bytes()).ok();
            out.flush().ok();
            Some(Ok(Value::Int(SomaInt::from_i64(text.len() as i64))))
        }
        // printf subset: %d %s %f %.Nf %Nd %-Ns %0Nd %%
        "format" if !args.is_empty() => {
            let Value::String(fmt) = &args[0] else {
                return Some(Err(RuntimeError::TypeError("format(fmt: String, args...) — the first argument is the format".to_string())));
            };
            Some(printf_subset(fmt, &args[1..]))
        }
        // strings.Fields: split on any run of whitespace, no empty pieces
        "fields" => {
            args.first().map(|arg| match arg {
                Value::String(s) => Ok(Value::List(s.split_whitespace().map(|w| Value::String(w.to_string())).collect())),
                other => Err(RuntimeError::TypeError(format!("fields(s: String) -> List<String>, got {}", super::super::value_type_name(other)))),
            })
        }
        "trim" if args.len() == 2 => {
            // trim(s, chars): strip any of `chars` from both ends
            match (&args[0], &args[1]) {
                (Value::String(s), Value::String(set)) => {
                    let cs: Vec<char> = set.chars().collect();
                    Some(Ok(Value::String(s.trim_matches(|c| cs.contains(&c)).to_string())))
                }
                _ => Some(Err(RuntimeError::TypeError("trim(s: String, chars: String) -> String".to_string()))),
            }
        }
        "trim" => {
            args.first().map(|arg| {
                if let Value::String(s) = arg {
                    Ok(Value::String(s.trim().to_string()))
                } else {
                    Err(RuntimeError::TypeError("trim expects a string".to_string()))
                }
            })
        }
        "index_of" => {
            if args.len() >= 2 {
                if let (Value::String(s), Value::String(sub)) = (&args[0], &args[1]) {
                    Some(Ok(match s.find(sub.as_str()) {
                        Some(byte_pos) => {
                            // Convert byte offset to char offset
                            let char_pos = s[..byte_pos].chars().count();
                            Value::Int(SomaInt::from_i64(char_pos as i64))
                        }
                        None => Value::Int(SomaInt::from_i64(-1)),
                    }))
                } else if let Value::List(xs) = &args[0] {
                    // position of the first element equal to x (-1 if absent)
                    // — on a list it answered -1 for everything
                    let pos = xs.iter().position(|v| crate::interpreter::deep_equal(v, &args[1]) || match (v, &args[1]) {
                        (Value::Int(a), Value::Float(b)) => a.to_f64() == *b,
                        (Value::Float(a), Value::Int(b)) => *a == b.to_f64(),
                        _ => false,
                    });
                    Some(Ok(Value::Int(SomaInt::from_i64(pos.map_or(-1, |p| p as i64)))))
                } else {
                    Some(Err(RuntimeError::TypeError(format!("index_of(s: String, sub: String) or index_of(xs: List, x) — got {} as the first argument", crate::interpreter::value_type_name(&args[0])))))
                }
            } else {
                Some(Err(RuntimeError::TypeError("index_of(string, substring)".to_string())))
            }
        }
        "substring" => {
            if args.len() >= 3 {
                if let (Value::String(s), Value::Int(start_si), Value::Int(end_si)) = (&args[0], &args[1], &args[2]) {
                    let start = start_si.to_i64().unwrap_or(0).max(0) as usize;
                    let char_count = s.chars().count();
                    let end = end_si.to_i64().unwrap_or(0).min(char_count as i64) as usize;
                    let result: String = s.chars().skip(start).take(end.saturating_sub(start)).collect();
                    Some(Ok(Value::String(result)))
                } else {
                    Some(Ok(Value::Unit))
                }
            } else {
                Some(Err(RuntimeError::TypeError("substring(string, start, end)".to_string())))
            }
        }
        "len" => {
            args.first().map(|arg| match arg {
                Value::String(s) => Ok(Value::Int(SomaInt::from_i64(s.chars().count() as i64))),
                Value::List(items) => Ok(Value::Int(SomaInt::from_i64(items.len() as i64))),
                Value::Map(entries) => Ok(Value::Int(SomaInt::from_i64(entries.len() as i64))),
                _ => Err(RuntimeError::TypeError("len expects a string, list, or map".to_string())),
            })
        }
        "to_string" => {
            args.first().map(|arg| Ok(Value::String(format!("{}", arg))))
        }
        "to_int" => {
            // a Float outside i64 (1e19, inf, NaN) used to saturate silently
            let float_to_int = |n: f64| -> Result<Value, RuntimeError> {
                if !n.is_finite() {
                    return Err(RuntimeError::Domain {
                        kind: "range".to_string(),
                        message: format!("to_int({}) has no integer value — keep it a Float", n),
                    });
                }
                // truncation toward zero, BigInt-exact beyond i64 (to_int(1e20) raised)
                if n.abs() >= 9.223372036854775e18 {
                    return Ok(Value::Int(SomaInt::from_rug(rug::Integer::from_f64(n.trunc()).unwrap())));
                }
                Ok(Value::Int(SomaInt::from_i64(n as i64)))
            };
            args.first().map(|arg| match arg {
                Value::Int(si) => Ok(Value::Int(si.clone())),
                Value::Float(n) => float_to_int(*n),
                Value::String(s) => {
                    if let Ok(n) = s.parse::<i64>() {
                        Ok(Value::Int(SomaInt::from_i64(n)))
                    } else if let Ok(big) = s.trim().parse::<rug::Integer>() {
                        // integers beyond i64 stay exact instead of clamping
                        Ok(Value::Int(SomaInt::from_rug(big)))
                    } else if let Ok(f) = s.parse::<f64>() {
                        float_to_int(f)
                    } else {
                        Ok(Value::Unit)
                    }
                },
                Value::Bool(b) => Ok(Value::Int(SomaInt::from_i64(if *b { 1 } else { 0 }))),
                _ => Ok(Value::Unit),
            })
        }
        "to_float" => {
            args.first().map(|arg| match arg {
                Value::Float(n) => Ok(Value::Float(*n)),
                Value::Int(si) => Ok(Value::Float(si.to_f64())),
                Value::String(s) => {
                    if let Ok(f) = s.parse::<f64>() {
                        Ok(Value::Float(f))
                    } else {
                        Ok(Value::Unit)
                    }
                },
                _ => Ok(Value::Unit),
            })
        }
        "to_json" => {
            args.first().map(|arg| {
                let mut out = String::new();
                write_json(arg, &mut out);
                Ok(Value::String(out))
            })
        }
        // pad_left("7", 4, "0") = "0007"; pad_right("ab", 4) = "ab  "
        "pad_left" | "pad_right" if args.len() >= 2 => {
            let text = format!("{}", args[0]);
            let width = match &args[1] {
                Value::Int(i) => i.to_i64().unwrap_or(0).max(0) as usize,
                _ => return Some(Err(RuntimeError::TypeError(format!("{name}(s, width, fill?): width must be an Int")))),
            };
            let fill = args.get(2).map(|f| format!("{}", f)).filter(|f| !f.is_empty()).unwrap_or_else(|| " ".to_string());
            let have = text.chars().count();
            if have >= width {
                return Some(Ok(Value::String(text)));
            }
            let pad: String = fill.chars().cycle().take(width - have).collect();
            Some(Ok(Value::String(if name == "pad_left" { format!("{pad}{text}") } else { format!("{text}{pad}") })))
        }
        "from_json" => {
            args.first().map(|arg| {
                match arg {
                    // invalid JSON raises (kind "json") — it used to come back
                    // as the input string, so `try { from_json(s) }` never failed
                    Value::String(s) if serde_json::from_str::<serde_json::Value>(s).is_err() => {
                        let shown: String = s.chars().take(60).collect();
                        Err(RuntimeError::Domain {
                            kind: "json".to_string(),
                            message: format!("json: not valid JSON: {}", shown),
                        })
                    }
                    Value::String(s) => Ok(json_to_value(s)),
                    Value::Map(_) | Value::List(_) => Ok(arg.clone()),
                    Value::Unit => Ok(Value::Unit),
                    other => Ok(Value::String(format!("{}", other))),
                }
            })
        }
        "type_of" => {
            args.first().map(|arg| {
                let t = match arg {
                    Value::Int(_) => "Int",
                    Value::Float(_) => "Float",
                    Value::String(_) => "String",
                    Value::Bool(_) => "Bool",
                    Value::List(_) => "List",
                    Value::Map(_) => "Map",
                    Value::Lambda { .. } | Value::LambdaBlock { .. } => "Lambda",
                    Value::Variant { .. } => "Variant",
                    Value::Unit => "Unit",
                };
                Ok(Value::String(t.to_string()))
            })
        }
        "escape_html" => {
            if let Some(Value::String(s)) = args.first() {
                let escaped = s
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;")
                    .replace('\'', "&#39;");
                Some(Ok(Value::String(escaped)))
            } else {
                Some(Err(RuntimeError::TypeError("escape_html(string)".to_string())))
            }
        }
        _ => None,
    }
}

/// Serialize a Value as valid JSON. Unlike Display formatting, this escapes
/// strings properly and maps NaN/inf (which JSON cannot represent) to null.
/// BigInts are written as bare arbitrary-precision numbers.
/// The JSON `to_json` writes (used by `soma serve` for response bodies).
pub fn to_json_string(v: &Value) -> String {
    let mut out = String::new();
    write_json_with(v, &mut out, false);
    out
}

/// Same JSON, laid out like the interpreter's Display (`{"k": v}`, `[a, b]`)
/// — what `soma run` prints.
pub fn to_json_string_spaced(v: &Value) -> String {
    let mut out = String::new();
    write_json_with(v, &mut out, true);
    out
}

fn write_json(v: &Value, out: &mut String) { write_json_with(v, out, false) }

fn write_json_with(v: &Value, out: &mut String, spaced: bool) {
    let sep = if spaced { ", " } else { "," };
    let colon = if spaced { ": " } else { ":" };
    match v {
        Value::Unit => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(si) => out.push_str(&si.to_string()),
        Value::Float(f) => {
            if f.is_finite() {
                out.push_str(&format!("{:?}", f));
            } else {
                out.push_str("null");
            }
        }
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())),
        Value::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 { out.push_str(sep); }
                write_json_with(item, out, spaced);
            }
            out.push(']');
        }
        Value::Map(entries) => {
            out.push('{');
            // the HTTP-response mark is not data
            for (i, (k, val)) in entries.iter().filter(|(k, v)| !(k.as_str() == "_response" && matches!(v, Value::Lambda { param, .. } if param == crate::interpreter::HTTP_MARK))).enumerate() {
                if i > 0 { out.push_str(sep); }
                out.push_str(&serde_json::to_string(k).unwrap_or_else(|_| "\"\"".to_string()));
                out.push_str(colon);
                write_json_with(val, out, spaced);
            }
            out.push('}');
        }
        // a variant is a tagged object (it used to be its Display text as a
        // JSON string, which from_json could not bring back)
        Value::Variant { type_name, variant, fields } => {
            out.push_str("{\"_type\""); out.push_str(colon);
            out.push_str(&serde_json::to_string(type_name).unwrap_or_default());
            out.push_str(sep); out.push_str("\"_variant\""); out.push_str(colon);
            out.push_str(&serde_json::to_string(variant).unwrap_or_default());
            match fields {
                VariantValue::Unit => {}
                VariantValue::Tuple(items) => {
                    out.push_str(sep); out.push_str("\"_values\""); out.push_str(colon);
                    write_json_with(&Value::List(items.clone()), out, spaced);
                }
                VariantValue::Struct(entries) => {
                    for (k, val) in entries {
                        out.push_str(sep);
                        out.push_str(&serde_json::to_string(k).unwrap_or_else(|_| "\"\"".to_string()));
                        out.push_str(colon);
                        write_json_with(val, out, spaced);
                    }
                }
            }
            out.push('}');
        }
        other => out.push_str(&serde_json::to_string(&format!("{}", other)).unwrap_or_else(|_| "\"\"".to_string())),
    }
}

/// `%[-0][width][.prec](d|s|f|%)`; other letters are an error naming them.
fn printf_subset(fmt: &str, args: &[Value]) -> Result<Value, RuntimeError> {
    let chars: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut next = 0usize;
    while i < chars.len() {
        if chars[i] != '%' { out.push(chars[i]); i += 1; continue; }
        i += 1;
        if i < chars.len() && chars[i] == '%' { out.push('%'); i += 1; continue; }
        let mut left = false; let mut zero = false;
        while i < chars.len() && (chars[i] == '-' || chars[i] == '0') {
            if chars[i] == '-' { left = true } else { zero = true }
            i += 1;
        }
        let mut width = String::new();
        while i < chars.len() && chars[i].is_ascii_digit() { width.push(chars[i]); i += 1; }
        let mut prec: Option<usize> = None;
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            let mut p = String::new();
            while i < chars.len() && chars[i].is_ascii_digit() { p.push(chars[i]); i += 1; }
            prec = Some(p.parse().unwrap_or(0));
        }
        let Some(&conv) = chars.get(i) else {
            return Err(RuntimeError::TypeError("format(): the format ends inside a % directive".to_string()));
        };
        i += 1;
        let arg = args.get(next).cloned().ok_or_else(|| RuntimeError::TypeError(format!(
            "format(): directive %{} needs argument {} but only {} were given", conv, next + 1, args.len())))?;
        next += 1;
        let body = match conv {
            'd' => match &arg {
                Value::Int(n) => n.to_string(),
                Value::Float(f) => format!("{}", f.trunc() as i64),
                other => return Err(RuntimeError::TypeError(format!("format(): %d needs an Int, got {}", super::super::value_type_name(other)))),
            },
            'f' => {
                let x = match &arg { Value::Float(f) => *f, Value::Int(n) => n.to_f64(), other => return Err(RuntimeError::TypeError(format!("format(): %f needs a number, got {}", super::super::value_type_name(other)))) };
                super::math::fixed_string(x, prec.unwrap_or(6))
            }
            's' => {
                let t = format!("{}", arg);
                match prec { Some(p) => t.chars().take(p).collect(), None => t }
            }
            other => return Err(RuntimeError::TypeError(format!("format(): unsupported directive %{} (supported: %d %s %f %.Nf, widths, - and 0 flags)", other))),
        };
        let w: usize = width.parse().unwrap_or(0);
        let len = body.chars().count();
        if len >= w { out.push_str(&body); continue; }
        let pad = w - len;
        if left {
            out.push_str(&body); out.push_str(&" ".repeat(pad));
        } else if zero && conv != 's' {
            let (sign, digits) = if body.starts_with('-') { ("-", &body[1..]) } else { ("", body.as_str()) };
            out.push_str(sign); out.push_str(&"0".repeat(pad)); out.push_str(digits);
        } else {
            out.push_str(&" ".repeat(pad)); out.push_str(&body);
        }
    }
    Ok(Value::String(out))
}
