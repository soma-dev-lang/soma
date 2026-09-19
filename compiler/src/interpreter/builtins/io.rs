use super::super::{Value, RuntimeError, map_from_pairs};
use crate::interpreter::soma_int::SomaInt;
use std::collections::HashMap;
use indexmap::IndexMap;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "print" => {
            let line = args.iter().map(|a| format!("{}", a)).collect::<Vec<_>>().join(" ");
            // under --json stdout is the one JSON document: print() goes to
            // stderr (`soma test --json` was invalid JSON)
            if crate::commands::JSON_MODE.load(std::sync::atomic::Ordering::Relaxed) { eprintln!("{}", line); } else { println!("{}", line); }
            Some(Ok(Value::Unit))
        }
        // every builtin that takes a path: `load("templates/" + name)` and
        // `read_files("../..", 3)` read any file past the project (only
        // read_file / write_file / read_csv / write_csv were guarded)
        "read_file" | "write_file" | "read_csv" | "write_csv" | "load_template" | "load" | "include"
        | "read_files" | "par_read_files" | "word_count" | "par_word_count"
            if matches!(args.first(), Some(Value::String(p)) if path_refused(p, name).is_some()) => {
            let Some(Value::String(p)) = args.first() else { unreachable!() };
            Some(Err(path_refused(p, name).unwrap()))
        }
        "load_template" | "load" | "include" => {
            if let Some(Value::String(path)) = args.first() {
                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        if args.len() > 1 {
                            // one pass, as render(): a value `{token}` is text,
                            // not a placeholder the next pair fills (it leaked
                            // the secret passed after it)
                            Some(Ok(Value::String(substitute_once(&content, &args[1..]))))
                        } else {
                            Some(Ok(Value::String(content)))
                        }
                    }
                    Err(e) => Some(Err(RuntimeError::TypeError(format!("cannot load '{}': {}", path, e)))),
                }
            } else {
                Some(Err(RuntimeError::TypeError("load_template expects a file path string".to_string())))
            }
        }
        "render" => {
            if let Some(Value::String(template)) = args.first() {
                Some(Ok(Value::String(substitute_once(template, &args[1..]))))
            } else {
                Some(Err(RuntimeError::TypeError("render expects a template string".to_string())))
            }
        }
        "html" => {
            let (status, mut body) = if args.len() >= 2 {
                (args[0].clone(), format!("{}", args[1]))
            } else {
                (Value::Int(SomaInt::from_i64(200)), args.first().map(|a| format!("{}", a)).unwrap_or_default())
            };
            // Only inject HTMX on full pages, not fragments
            let inject_htmx = body.contains("<html") || body.contains("<!DOCTYPE") || body.contains("<!doctype");
            // an hx- ATTRIBUTE inside a tag, not the text "hx-" (escaped user
            // content "say hx-get please" loaded a third-party script)
            let uses_htmx = regex::Regex::new(r#"<[a-zA-Z][^<>]*\shx-[a-z-]+\s*="#).map(|r| r.is_match(&body)).unwrap_or(false);
            if inject_htmx && uses_htmx && !body.contains("htmx.org") {
                // pinned by integrity: whoever controls the CDN cannot swap
                // the script run on every page (it had no SRI)
                let htmx_tag = "<script src=\"https://unpkg.com/htmx.org@2.0.4/dist/htmx.min.js\" integrity=\"sha384-HGfztofotfshcF7+8n44JQL2oJmowVChPTg48S+jvZoztPfvwD79OC/LTtG6dMp+\" crossorigin=\"anonymous\"></script>";
                if let Some(pos) = body.find("</head>") {
                    body.insert_str(pos, htmx_tag);
                } else if let Some(pos) = body.find("<body") {
                    body.insert_str(pos, htmx_tag);
                } else {
                    body = format!("{}{}", htmx_tag, body);
                }
            }
            // html(status, body, "Set-Cookie", "sid=1", …): header pairs
            // (they were dropped silently)
            let mut entries = map_from_pairs(vec![
                ("_status".to_string(), status),
                ("_body".to_string(), Value::String(body)),
                ("_content_type".to_string(), Value::String("text/html; charset=utf-8".to_string())),
                ("_response".to_string(), crate::interpreter::http_marker()),
            ]);
            if let Value::Map(m) = &mut entries {
                let mut i = 2;
                while i + 1 < args.len() {
                    m.insert(format!("{}", args[i]), args[i + 1].clone());
                    i += 2;
                }
            }
            Some(Ok(entries))
        }
        "response" => {
            let status = args.first().cloned().unwrap_or(Value::Int(SomaInt::from_i64(200)));
            match &status {
                // 1xx are protocol-level (101 left the client hanging)
                Value::Int(si) if si.to_i64().map_or(false, |n| (200..=599).contains(&n)) => {}
                // a server bug, not a client error: kind `response` → 500
                other => return Some(Err(RuntimeError::Domain { kind: "response".to_string(), message: format!(
                    "response(status, body): the status must be an HTTP status Int 200–599, got {}", other) })),
            }
            let body = args.get(1).cloned().unwrap_or(Value::Unit);
            let mut entries = IndexMap::new();
            entries.insert("_status".to_string(), status);
            entries.insert("_body".to_string(), body);
            entries.insert("_response".to_string(), crate::interpreter::http_marker());
            let mut i = 2;
            while i + 1 < args.len() {
                let key = format!("{}", args[i]);
                let val = args[i + 1].clone();
                entries.insert(key, val);
                i += 2;
            }
            Some(Ok(Value::Map(entries)))
        }
        "redirect" => {
            let url = args.first().map(|a| format!("{}", a)).unwrap_or("/".to_string());
            Some(Ok(map_from_pairs(vec![
                ("_status".to_string(), Value::Int(SomaInt::from_i64(302))),
                ("_body".to_string(), Value::String(String::new())),
                ("Location".to_string(), Value::String(url)),
                ("_response".to_string(), crate::interpreter::http_marker()),
            ])))
        }
        "sse" => {
            // sse("stream1", "stream2", ...) — returns a marker value
            // The server detects _sse and opens a persistent SSE connection
            let streams: Vec<String> = args.iter().map(|a| format!("{}", a)).collect();
            Some(Ok(map_from_pairs(vec![
                ("_sse".to_string(), Value::Bool(true)),
                ("_response".to_string(), crate::interpreter::http_marker()),
                ("_streams".to_string(), Value::List(
                    streams.iter().map(|s| Value::String(s.clone())).collect()
                )),
            ])))
        }
        "read_file" => {
            if let Some(Value::String(path)) = args.first() {
                match std::fs::read_to_string(path) {
                    Ok(content) => Some(Ok(Value::String(content))),
                    Err(e) => Some(Ok(map_from_pairs(vec![
                        ("error".to_string(), Value::String(format!("{}", e))),
                    ]))),
                }
            } else {
                Some(Err(RuntimeError::TypeError("read_file(path)".to_string())))
            }
        }
        "write_file" => {
            if args.len() >= 2 {
                if let (Value::String(path), content) = (&args[0], &args[1]) {
                    let text = format!("{}", content);
                    match write_creating_dirs(path, &text) {
                        Ok(_) => Some(Ok(Value::Bool(true))),
                        Err(e) => Some(Ok(map_from_pairs(vec![
                            ("error".to_string(), Value::String(format!("{}: {}", path, e))),
                        ]))),
                    }
                } else {
                    Some(Err(RuntimeError::TypeError("write_file(path, content)".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("write_file(path, content)".to_string())))
            }
        }
        // CSV text already in memory (an upload): no temp file
        "from_csv" => {
            let Some(Value::String(text)) = args.first() else {
                return Some(Err(RuntimeError::TypeError("from_csv(text: String, opts: Map?)".to_string())));
            };
            let opts = match csv_opts(args.get(1), "from_csv") { Ok(o) => o, Err(e) => return Some(Err(e)) };
            Some(csv_rows(text, opts, "from_csv"))
        }
        "read_csv" => {
            if let Some(Value::String(path)) = args.first() {
                let opts = match csv_opts(args.get(1), "read_csv") { Ok(o) => o, Err(e) => return Some(Err(e)) };
                match std::fs::read_to_string(path) {
                    Ok(content) => Some(csv_rows(&content, opts, path)),
                    Err(e) => Some(Ok(map_from_pairs(vec![
                        ("error".to_string(), Value::String(format!("{}", e))),
                    ]))),
                }
            } else {
                Some(Err(RuntimeError::TypeError("read_csv(path)".to_string())))
            }
        }
        "read_files" | "par_read_files" | "word_count" | "par_word_count" => {
            return call_bulk_io(name, args);
        }
        // CSV as a String (an HTTP download): no temp file
        "to_csv" => {
            match args.first() {
                Some(Value::List(items)) if args.len() == 1 => Some(Ok(Value::String(csv_text(items)))),
                _ => Some(Err(RuntimeError::TypeError("to_csv(rows: List<Map>) -> String".to_string()))),
            }
        }
        "write_csv" => {
            // write_csv(path, list_of_maps)
            if args.len() >= 2 {
                if let (Value::String(path), Value::List(items)) = (&args[0], &args[1]) {
                    let output = csv_text(items);
                    match write_creating_dirs(path, &output) {
                        Ok(_) => Some(Ok(Value::Bool(true))),
                        Err(e) => Some(Ok(map_from_pairs(vec![
                            ("error".to_string(), Value::String(format!("{}: {}", path, e))),
                        ]))),
                    }
                } else {
                    Some(Err(RuntimeError::TypeError("write_csv(path, list_of_maps)".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("write_csv(path, list_of_maps)".to_string())))
            }
        }
        _ => None,
    }
}

// ── Bulk I/O: read_files, word_count ────────────────────────────────
// These are implemented in Rust for performance on large-scale I/O.

fn call_bulk_io(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "read_files" => {
            // read_files(glob_pattern) → list of {path, content}
            // read_files(dir, count) → read first N files from dir
            if args.len() >= 2 {
                if let (Value::String(dir), Value::Int(count_si)) = (&args[0], &args[1]) {
                    let count_limit = count_si.to_i64().unwrap_or(0);
                    let mut results = Vec::new();
                    if let Ok(entries) = std::fs::read_dir(dir) {
                        // the first N BY NAME (the directory order was the
                        // file system's, different between runs)
                        let mut paths: Vec<std::path::PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
                        paths.sort();
                        let mut n = 0i64;
                        for path in paths {
                            if n >= count_limit { break; }
                            {
                                if path.is_file() {
                                    if let Ok(content) = std::fs::read_to_string(&path) {
                                        results.push(map_from_pairs(vec![
                                            ("path".to_string(), Value::String(path.display().to_string())),
                                            ("content".to_string(), Value::String(content)),
                                        ]));
                                        n += 1;
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Value::List(results)))
                } else {
                    Some(Err(RuntimeError::TypeError("read_files(dir, count)".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("read_files(dir, count)".to_string())))
            }
        }
        "word_count" => {
            // word_count(text) → map of word → count
            // word_count(list_of_strings) → aggregated map
            // Implemented in Rust for speed — 10-50x faster than interpreted loop
            match args.first() {
                Some(Value::String(text)) => {
                    let mut counts: HashMap<String, i64> = HashMap::new();
                    for word in text.split_whitespace() {
                        let w = word.to_lowercase();
                        *counts.entry(w).or_insert(0) += 1;
                    }
                    let map: IndexMap<String, Value> = counts.into_iter()
                        .map(|(k, v)| (k, Value::Int(SomaInt::from_i64(v))))
                        .collect();
                    Some(Ok(Value::Map(map)))
                }
                Some(Value::List(items)) => {
                    // Aggregate word counts across all strings in the list
                    let mut counts: HashMap<String, i64> = HashMap::new();
                    for item in items {
                        let text = match item {
                            Value::String(s) => s.clone(),
                            Value::Map(entries) => {
                                // If it's a {content: "..."} map, extract content
                                entries.get("content")
                                    .and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None })
                                    .unwrap_or_default()
                            }
                            _ => continue,
                        };
                        for word in text.split_whitespace() {
                            let w = word.to_lowercase();
                            *counts.entry(w).or_insert(0) += 1;
                        }
                    }
                    let map: IndexMap<String, Value> = counts.into_iter()
                        .map(|(k, v)| (k, Value::Int(SomaInt::from_i64(v))))
                        .collect();
                    Some(Ok(Value::Map(map)))
                }
                _ => Some(Err(RuntimeError::TypeError("word_count(text) or word_count(list)".to_string())))
            }
        }
        "par_read_files" => {
            // Parallel file reading using threads
            if args.len() >= 2 {
                if let (Value::String(dir), Value::Int(count_si)) = (&args[0], &args[1]) {
                    let count_limit = count_si.to_i64().unwrap_or(0) as usize;
                    let paths: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
                        .map(|entries| {
                            entries.filter_map(|e| e.ok())
                                .filter(|e| e.path().is_file())
                                .take(count_limit)
                                .map(|e| e.path())
                                .collect()
                        })
                        .unwrap_or_default();

                    let n_threads = std::thread::available_parallelism()
                        .map(|n| n.get()).unwrap_or(1);
                    let chunk = (paths.len() + n_threads - 1) / n_threads;

                    let results: Vec<Value> = std::thread::scope(|s| {
                        let handles: Vec<_> = paths.chunks(chunk).map(|chunk_paths| {
                            s.spawn(move || {
                                chunk_paths.iter().filter_map(|path| {
                                    std::fs::read_to_string(path).ok().map(|content| {
                                        map_from_pairs(vec![
                                            ("path".to_string(), Value::String(path.display().to_string())),
                                            ("content".to_string(), Value::String(content)),
                                        ])
                                    })
                                }).collect::<Vec<_>>()
                            })
                        }).collect();
                        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
                    });

                    Some(Ok(Value::List(results)))
                } else {
                    Some(Err(RuntimeError::TypeError("par_read_files(dir, count)".to_string())))
                }
            } else {
                Some(Err(RuntimeError::TypeError("par_read_files(dir, count)".to_string())))
            }
        }
        "par_word_count" => {
            // Parallel word count: split list across threads, merge counts
            if let Some(Value::List(items)) = args.first() {
                let n_threads = std::thread::available_parallelism()
                    .map(|n| n.get()).unwrap_or(1);
                let chunk = (items.len() + n_threads - 1) / n_threads;

                let merged: HashMap<String, i64> = std::thread::scope(|s| {
                    let handles: Vec<_> = items.chunks(chunk).map(|chunk_items| {
                        s.spawn(move || {
                            let mut local: HashMap<String, i64> = HashMap::new();
                            for item in chunk_items {
                                let text = match item {
                                    Value::String(s) => s.as_str(),
                                    Value::Map(entries) => {
                                        entries.get("content")
                                            .and_then(|v| if let Value::String(s) = v { Some(s.as_str()) } else { None })
                                            .unwrap_or("")
                                    }
                                    _ => "",
                                };
                                for word in text.split_whitespace() {
                                    *local.entry(word.to_lowercase()).or_insert(0) += 1;
                                }
                            }
                            local
                        })
                    }).collect();

                    let mut merged: HashMap<String, i64> = HashMap::new();
                    for handle in handles {
                        for (k, v) in handle.join().unwrap() {
                            *merged.entry(k).or_insert(0) += v;
                        }
                    }
                    merged
                });

                let map: IndexMap<String, Value> = merged.into_iter()
                    .map(|(k, v)| (k, Value::Int(SomaInt::from_i64(v))))
                    .collect();
                Some(Ok(Value::Map(map)))
            } else {
                Some(Err(RuntimeError::TypeError("par_word_count(list)".to_string())))
            }
        }
        // think() and think_json() are handled in storage.rs (needs interpreter access for tool calling)

        _ => None,
    }
}

/// Call an OpenAI-compatible LLM API.
/// Config: SOMA_LLM_KEY (or OPENAI_API_KEY), SOMA_LLM_URL, SOMA_LLM_MODEL
fn call_llm(prompt: &str, extra_args: &[Value]) -> Result<Value, RuntimeError> {
    let api_url = std::env::var("SOMA_LLM_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".to_string());
    let api_key = std::env::var("SOMA_LLM_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
        .map_err(|_| RuntimeError::TypeError(
            "think() requires SOMA_LLM_KEY or OPENAI_API_KEY environment variable".to_string()
        ))?;
    let model = std::env::var("SOMA_LLM_MODEL")
        .unwrap_or_else(|_| "gpt-4o-mini".to_string());

    let system = if let Some(Value::String(sys)) = extra_args.first() {
        sys.clone()
    } else {
        "You are a helpful AI agent. Respond concisely.".to_string()
    };

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": prompt}
        ],
        "max_tokens": 2048
    });

    match ureq::post(&api_url)
        .set("Authorization", &format!("Bearer {}", api_key))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
    {
        Ok(response) => {
            let text = response.into_string()
                .map_err(|e| RuntimeError::TypeError(format!("think() response error: {}", e)))?;
            let json: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| RuntimeError::TypeError(format!("think() JSON error: {}", e)))?;
            if let Some(content) = json["choices"][0]["message"]["content"].as_str() {
                Ok(Value::String(content.to_string()))
            } else if let Some(err) = json["error"]["message"].as_str() {
                Err(RuntimeError::TypeError(format!("LLM error: {}", err)))
            } else {
                Ok(Value::String(text))
            }
        }
        Err(e) => Err(RuntimeError::TypeError(format!("think() HTTP error: {}", e))),
    }
}


/// RFC 4180 records: `"a,b"` is one field, `""` inside quotes is a quote,
/// a quoted field may span lines; CRLF or LF. Each field carries whether it
/// was quoted.
/// `read_csv` / `from_csv` options: `raw` (Bool) and `delimiter` (one
/// character). Anything else is refused — `map("delimiter", ";")` was
/// ignored and every `;` file came back as one column.
fn csv_opts(opts: Option<&Value>, builtin: &str) -> Result<(bool, char), RuntimeError> {
    let mut raw = false;
    let mut delim = ',';
    match opts {
        None | Some(Value::Unit) => {}
        Some(Value::Map(m)) => {
            for (k, v) in m {
                match (k.as_str(), v) {
                    ("raw", Value::Bool(b)) => raw = *b,
                    ("delimiter", Value::String(d)) if d.chars().count() == 1 && !matches!(d.as_str(), "\"" | "\n" | "\r") => delim = d.chars().next().unwrap(),
                    ("raw" | "delimiter", other) => return Err(RuntimeError::TypeError(format!("{}: option '{}' got {} — raw is a Bool, delimiter one character (not a quote or newline)", builtin, k, other))),
                    _ => return Err(RuntimeError::TypeError(format!("{}: unknown option '{}' (options: raw, delimiter)", builtin, k))),
                }
            }
        }
        Some(other) => return Err(RuntimeError::TypeError(format!("{}: options must be a Map, got {}", builtin, crate::interpreter::value_type_name(other)))),
    }
    Ok((raw, delim))
}

fn csv_rows(content: &str, (raw, delim): (bool, char), source: &str) -> Result<Value, RuntimeError> {
    let path = source;
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut records = match parse_csv(content, delim) {
        Ok(r) => r.into_iter(),
        Err(rec) => return Err(RuntimeError::Domain { kind: "csv".to_string(), message: format!("csv: {}: record {} opens a quote that is never closed — the rest of the input would be one cell", path, rec) }),
    };
    let headers: Vec<String> = match records.next() {
        Some(h) => h.into_iter().map(|(t, _)| t.trim().to_string()).collect(),
        None => return Ok(Value::List(vec![])),
    };
    // `sku,qty,qty`: the second `qty` overwrote the first in
    // every row, silently — name the duplicate instead
    // `_type,_variant` columns forged a record / variant from client text
    // (is_a(row, "Role") was true) — refused as in a JSON body
    if let Some(r) = headers.iter().find(|h| matches!(h.as_str(), "_type" | "_variant" | "_values")) {
        return Err(RuntimeError::Domain { kind: "csv".to_string(), message: format!("csv: {}: column '{}' is reserved (it marks a record or a variant) — rename it", path, r) });
    }
    // two EMPTY header names dropped a column too
    if let Some(dup) = headers.iter().enumerate().find(|(i, h)| headers[..*i].contains(h)).map(|(_, h)| h.clone()) {
        return Err(RuntimeError::Domain { kind: "csv".to_string(), message: format!("csv: {}: the header names column '{}' twice — a row is a map, so one would overwrite the other; rename one", path, dup) });
    }
    let mut rows = Vec::new();
    for rec in records {
        if rec.len() == 1 && rec[0].0.trim().is_empty() && !rec[0].1 { continue; }
        let mut entries = IndexMap::new();
        for (i, header) in headers.iter().enumerate() {
            let (text, quoted) = rec.get(i).cloned().unwrap_or((String::new(), false));
            // raw keeps the text exactly (spaces included)
            let val = if quoted || raw { text.as_str() } else { text.trim() };
            // a quoted cell is text; so is `007` (an id,
            // not seven); raw mode keeps everything text
            let leading_zero = val.len() > 1 && val.starts_with('0') && val.as_bytes()[1].is_ascii_digit();
            let typed = if raw || quoted || leading_zero {
                Value::String(val.to_string())
            } else if let Ok(n) = val.parse::<i64>() {
                Value::Int(SomaInt::from_i64(n))
            } else if val.len() > 1 && val.trim_start_matches('-').len() == val.len() - usize::from(val.starts_with('-')) && val.trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) && !val.trim_start_matches('-').is_empty() {
                // an Int past i64 (shl(1, 70)) came back a Float
                Value::Int(SomaInt::from_decimal_str(val))
            } else if let Some(n) = val.parse::<f64>().ok().filter(|f| f.is_finite()) {
                Value::Float(n)
            } else if matches!(val, "NaN" | "inf" | "-inf") {
                // what write_csv writes for a NaN / infinite Float
                Value::Float(val.parse::<f64>().unwrap_or(f64::NAN))
            } else {
                Value::String(val.to_string())
            };
            entries.insert(header.clone(), typed);
        }
        rows.push(Value::Map(entries));
    }
    Ok(Value::List(rows))
}

/// The CSV text write_csv writes (header from the first row, quoting that
/// read_csv reads back exactly).
fn csv_text(items: &[Value]) -> String {
    let mut output = String::new();
    // Extract headers from first row
    // a cell a reader would split, trim or re-type is quoted:
    // separators, quotes, newlines, edge spaces — and a String
    // that reads as a number ("12", "1e5", "-0") — and a List or
    // Map is its JSON text (["a", "b"] shifted the row)
    let quote = |s: &str, text: bool| -> String {
        let numeric = text && !s.is_empty() && (s.parse::<f64>().is_ok() || s.trim_start_matches('-').chars().all(|c| c.is_ascii_digit()));
        // an empty String is `""`: a one-column row of it was a
        // blank line, which read_csv skips (24 rows in, 23 out)
        if numeric || (text && s.is_empty()) || s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r')
            || s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    if let Some(Value::Map(_)) = items.first() {
        // every key of every row, in first-seen order (a key only in a
        // later row was dropped with its value)
        let mut headers: Vec<&str> = Vec::new();
        for it in items {
            if let Value::Map(m) = it { for k in m.keys() { if !headers.contains(&k.as_str()) { headers.push(k.as_str()); } } }
        }
        output.push_str(&headers.iter().map(|h| quote(h, false)).collect::<Vec<_>>().join(","));
        output.push('\n');
        // Write rows
        for item in items {
            if let Value::Map(entries) = item {
                let vals: Vec<String> = headers.iter().map(|h| {
                    entries.get(*h)
                        .map(|v| match v {
                            Value::String(s) => quote(s, true),
                            // `()` is an empty cell (it was the text "null")
                            Value::Unit => String::new(),
                            Value::List(_) | Value::Map(_) | Value::Variant { .. } => quote(&super::string::to_json_string(v), false),
                            other => quote(&format!("{}", other), false),
                        })
                        .unwrap_or_default()
                }).collect();
                let line = vals.join(",");
                // a row of only empty cells is `""`, not a blank
                // line read_csv would skip (the row was lost)
                output.push_str(if line.is_empty() && !headers.is_empty() { "\"\"" } else { &line });
                output.push('\n');
            }
        }
    }
    output
}

fn parse_csv(text: &str, delim: char) -> Result<Vec<Vec<(String, bool)>>, usize> {
    let mut records = Vec::new();
    let mut rec: Vec<(String, bool)> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        any = true;
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') { field.push('"'); chars.next(); } else { in_quotes = false; }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.trim().is_empty() && !quoted => { field.clear(); quoted = true; in_quotes = true; }
            c if c == delim => { rec.push((std::mem::take(&mut field), quoted)); quoted = false; }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                rec.push((std::mem::take(&mut field), quoted));
                quoted = false;
                records.push(std::mem::take(&mut rec));
                any = false;
            }
            _ => field.push(c),
        }
    }
    // an unclosed quote swallowed the rest of the file into one cell,
    // silently (rows 3 and 4 became part of row 2's name)
    if in_quotes {
        return Err(records.len() + 1);
    }
    if any || !field.is_empty() || !rec.is_empty() {
        rec.push((field, quoted));
        records.push(rec);
    }
    Ok(records)
}

/// `{key}` placeholders filled from (key, value) pairs in ONE left-to-right
/// pass: a substituted value is never rescanned.
fn substitute_once(template: &str, pairs: &[Value]) -> String {
    let mut vars: HashMap<String, String> = HashMap::new();
    let mut i = 0;
    while i + 1 < pairs.len() {
        vars.insert(format!("{}", pairs[i]), format!("{}", pairs[i + 1]));
        i += 2;
    }
    let mut result = String::with_capacity(template.len());
    let mut pos = 0;
    while pos < template.len() {
        if template.as_bytes()[pos] == b'{' {
            if let Some(end) = template[pos + 1..].find('}') {
                if let Some(val) = vars.get(&template[pos + 1..pos + 1 + end]) {
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
    result
}

/// File paths built from client input: a `..` segment walked out of the
/// intended directory (`read_file("uploads/" + name)` with name
/// `../../secrets/admin_token.txt` returned the secret), and a write could
/// replace the program's own source (`write_file("uploads/" + "../f.cell", …)`
/// ran on the next restart).
fn path_refused(path: &str, builtin: &str) -> Option<RuntimeError> {
    let parts: Vec<&str> = path.split(['/', '\\']).collect();
    if parts.iter().any(|p| *p == "..") {
        return Some(RuntimeError::Domain { kind: "path".to_string(), message: format!("path: {}(\"{}\") has a `..` segment — build paths from a fixed directory and a validated name", builtin, path) });
    }
    if builtin.starts_with("write") {
        // compared case-insensitively: macOS / Windows file systems are
        // (`APP.CELL` overwrote app.cell, `.SOMA_DATA/soma.db` the live db)
        let lower: Vec<String> = parts.iter().map(|p| p.to_ascii_lowercase()).collect();
        let file = lower.last().map(|s| s.as_str()).unwrap_or("");
        if file.ends_with(".cell") || file == "soma.toml" || file == "soma.lock" || lower.iter().any(|p| p == ".soma_data") {
            return Some(RuntimeError::Domain { kind: "path".to_string(), message: format!("path: {}(\"{}\") would overwrite the program, its configuration or its storage", builtin, path) });
        }
    }
    None
}

/// `write_csv("zreports/z.csv", …)`: the directory is created (it answered
/// "No such file or directory" and an end-of-day job committed without its
/// report). The path guard ran before.
fn write_creating_dirs(path: &str, content: &str) -> std::io::Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() { std::fs::create_dir_all(parent)?; }
    }
    std::fs::write(path, content)
}
