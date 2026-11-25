//! `list_dir`, `file_info`, `append_file`, `read_line`: a program that works on a directory
//! of files (an agent's workspace) and talks with whoever runs it, one line at a time.
//!
//! Like `read_file` and `run_process` they never raise on what the file system does: a
//! missing path, a refusal, a failure is `{error, kind}` with kind `not_found`, `refused` or
//! `io`. A wrong argument type is a program bug and raises kind `type`.

use super::super::{map_from_pairs, RuntimeError, Value};
use crate::interpreter::soma_int::SomaInt;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "list_dir" => Some(match args {
            [Value::String(p)] => Ok(list_dir(p)),
            _ => Err(RuntimeError::TypeError("list_dir(path: String) — the directory to list".to_string())),
        }),
        "file_info" => Some(match args {
            [Value::String(p)] => Ok(file_info(p)),
            _ => Err(RuntimeError::TypeError("file_info(path: String)".to_string())),
        }),
        "append_file" => Some(match args {
            [Value::String(p), content] => Ok(append_file(p, &match content { Value::String(s) => s.clone(), other => format!("{}", other) })),
            _ => Err(RuntimeError::TypeError("append_file(path: String, content)".to_string())),
        }),
        "read_line" => Some(match args {
            [] => Ok(read_line(None)),
            [Value::String(prompt)] => Ok(read_line(Some(prompt))),
            _ => Err(RuntimeError::TypeError("read_line(prompt?: String)".to_string())),
        }),
        _ => None,
    }
}

fn int(n: i64) -> Value { Value::Int(SomaInt::from_i64(n)) }

fn failure(e: &std::io::Error, path: &str) -> Value {
    let kind = match e.kind() {
        std::io::ErrorKind::NotFound => "not_found",
        std::io::ErrorKind::PermissionDenied => "refused",
        _ => "io",
    };
    map_from_pairs(vec![
        ("error".to_string(), Value::String(format!("{}: {}", path, e))),
        ("kind".to_string(), Value::String(kind.to_string())),
    ])
}

fn kind_of(t: &std::fs::FileType) -> &'static str {
    if t.is_symlink() { "link" } else if t.is_dir() { "dir" } else if t.is_file() { "file" } else { "other" }
}

/// Seconds since the epoch of the last modification, 0 when the platform has none.
fn mtime(m: &std::fs::Metadata) -> i64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The entries of a directory, sorted by name: `{name, kind, size, mtime}` with kind
/// `file`, `dir`, `link` (a symbolic link is not followed) or `other`.
fn list_dir(path: &str) -> Value {
    let rd = match std::fs::read_dir(path) { Ok(r) => r, Err(e) => return failure(&e, path) };
    let mut rows: Vec<(String, Value)> = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let (kind, size, mt) = match entry.metadata() {
            Ok(m) => {
                let k = entry.file_type().map(|t| kind_of(&t)).unwrap_or("other");
                (k, if m.is_file() { m.len() as i64 } else { 0 }, mtime(&m))
            }
            Err(_) => ("other", 0, 0),
        };
        rows.push((name.clone(), map_from_pairs(vec![
            ("name".to_string(), Value::String(name)),
            ("kind".to_string(), Value::String(kind.to_string())),
            ("size".to_string(), int(size)),
            ("mtime".to_string(), int(mt)),
        ])));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    Value::List(rows.into_iter().map(|r| r.1).collect::<Vec<_>>().into())
}

/// `{exists, kind, size, mtime}` of a path (a symbolic link is followed); `{exists: false}`
/// when nothing is there.
fn file_info(path: &str) -> Value {
    match std::fs::metadata(path) {
        Ok(m) => map_from_pairs(vec![
            ("exists".to_string(), Value::Bool(true)),
            ("kind".to_string(), Value::String(kind_of(&m.file_type()).to_string())),
            ("size".to_string(), int(m.len() as i64)),
            ("mtime".to_string(), int(mtime(&m))),
        ]),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => map_from_pairs(vec![
            ("exists".to_string(), Value::Bool(false)),
        ]),
        Err(e) => failure(&e, path),
    }
}

/// Add `content` at the end of the file, creating it (and its directories) when needed.
fn append_file(path: &str, content: &str) -> Value {
    use std::io::Write;
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) { return failure(&e, path); }
        }
    }
    let f = std::fs::OpenOptions::new().create(true).append(true).open(path);
    match f.and_then(|mut f| f.write_all(content.as_bytes())) {
        Ok(()) => Value::Bool(true),
        Err(e) => failure(&e, path),
    }
}

/// One line of standard input without its line end, or () at the end of the input. The
/// prompt, when given, is written to standard output first (no newline).
fn read_line(prompt: Option<&str>) -> Value {
    use std::io::{BufRead, Write};
    if let Some(p) = prompt {
        let mut out = std::io::stdout();
        let _ = out.write_all(p.as_bytes());
        let _ = out.flush();
    }
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => Value::Unit,
        Ok(_) => {
            if line.ends_with('\n') { line.pop(); }
            if line.ends_with('\r') { line.pop(); }
            Value::String(line)
        }
    }
}
