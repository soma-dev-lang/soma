//! `env_var` and `run_process`: the environment of the running program, and the programs it
//! starts.
//!
//! `run_process` never goes through a shell: the program is looked up on PATH (or given by
//! its path) and the arguments are handed to it as they are — `"$HOME"` stays `$HOME`, `;`
//! separates nothing. Like the http builtins it never raises on what the process does: a
//! program that is missing, a timeout, a refusal of the operator's policy, a failure to start
//! are `{error, kind}`; an option of the wrong type is a program bug and raises kind `type`.
//!
//! The operator's policy, read at each call: `SOMA_PROCESS=deny` refuses every process,
//! `SOMA_PROCESS_ALLOW=git,ls` only the programs named (by name, or by the last segment of
//! the path given).

use super::super::{map_from_pairs, RuntimeError, Value};
use crate::interpreter::soma_int::SomaInt;
use std::io::{Read, Write};

const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const MAX_TIMEOUT_MS: u64 = 3_600_000;
const DEFAULT_MAX_BYTES: usize = 1_000_000;

pub fn call_builtin(name: &str, args: &[Value]) -> Option<Result<Value, RuntimeError>> {
    match name {
        "env_var" => Some(match args {
            [Value::String(n)] => Ok(std::env::var(n).map(Value::String).unwrap_or(Value::Unit)),
            _ => Err(RuntimeError::TypeError("env_var(name: String) — the variable's name".to_string())),
        }),
        "run_process" => Some(run_process(args)),
        _ => None,
    }
}

fn failure(kind: &str, message: String) -> Value {
    map_from_pairs(vec![
        ("error".to_string(), Value::String(message)),
        ("kind".to_string(), Value::String(kind.to_string())),
    ])
}

fn refused_by_policy(program: &str) -> Option<String> {
    if std::env::var("SOMA_PROCESS").is_ok_and(|v| v.trim().eq_ignore_ascii_case("deny")) {
        return Some(format!("run_process(\"{}\"): processes are refused here (SOMA_PROCESS=deny)", program));
    }
    if let Ok(allow) = std::env::var("SOMA_PROCESS_ALLOW") {
        let base = program.rsplit(['/', '\\']).next().unwrap_or(program);
        if !allow.split(',').map(|a| a.trim()).any(|a| !a.is_empty() && (a == program || a == base)) {
            return Some(format!("run_process(\"{}\"): not among the programs allowed here (SOMA_PROCESS_ALLOW={})", program, allow));
        }
    }
    None
}

fn run_process(args: &[Value]) -> Result<Value, RuntimeError> {
    let usage = "run_process(program: String, args: List<String>, opts?: map(\"cwd\", dir, \"stdin\", text, \"env\", map(…), \"clear_env\", Bool, \"timeout\", ms, \"max_bytes\", n))";
    let (program, list, opts) = match args {
        [Value::String(p), Value::List(l)] => (p, l, None),
        [Value::String(p), Value::List(l), Value::Map(m)] => (p, l, Some(m)),
        _ => return Err(RuntimeError::TypeError(usage.to_string())),
    };
    let mut argv: Vec<String> = Vec::new();
    for a in list.iter() {
        match a {
            Value::String(s) => argv.push(s.clone()),
            other => return Err(RuntimeError::TypeError(format!("run_process: each argument is a String, got {} {}", super::super::value_type_name(other), other))),
        }
    }
    if program.is_empty() { return Err(RuntimeError::TypeError("run_process: the program is empty".to_string())); }
    // the options: their types are checked, an unknown one is refused
    if let Some(m) = opts {
        for (k, v) in m.iter() {
            let ok = match (k.as_str(), v) {
                ("timeout", Value::Int(n)) => n.to_i64().is_some_and(|x| x > 0 && x as u64 <= MAX_TIMEOUT_MS),
                ("max_bytes", Value::Int(n)) => n.to_i64().is_some_and(|x| x > 0),
                ("cwd" | "stdin", Value::String(_)) => true,
                ("clear_env", Value::Bool(_)) => true,
                ("env", Value::Map(e)) => e.values().all(|x| matches!(x, Value::String(_))),
                ("timeout" | "max_bytes" | "cwd" | "stdin" | "clear_env" | "env", _) => false,
                _ => return Err(RuntimeError::TypeError(format!("run_process: unknown option '{}' — the options are cwd, stdin, env, clear_env, timeout (ms), max_bytes", k))),
            };
            if !ok {
                let want = match k.as_str() {
                    "timeout" => format!("a positive Int of milliseconds, at most {}", MAX_TIMEOUT_MS),
                    "max_bytes" => "a positive Int".to_string(),
                    "cwd" | "stdin" => "a String".to_string(),
                    "clear_env" => "a Bool".to_string(),
                    _ => "a map of variable name → String".to_string(),
                };
                return Err(RuntimeError::TypeError(format!("run_process: option '{}' must be {}, got {} {}", k, want, super::super::value_type_name(v), v)));
            }
        }
    }
    if let Some(why) = refused_by_policy(program) { return Ok(failure("refused", why)); }
    let get = |k: &str| opts.and_then(|m| m.get(k));
    let timeout_ms = match get("timeout") { Some(Value::Int(n)) => n.to_i64().unwrap_or(0) as u64, _ => DEFAULT_TIMEOUT_MS };
    let max_bytes = match get("max_bytes") { Some(Value::Int(n)) => n.to_i64().unwrap_or(0) as usize, _ => DEFAULT_MAX_BYTES };

    let mut cmd = std::process::Command::new(program);
    cmd.args(&argv).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    if let Some(Value::String(d)) = get("cwd") { cmd.current_dir(d); }
    if let Some(Value::Bool(true)) = get("clear_env") { cmd.env_clear(); }
    if let Some(Value::Map(e)) = get("env") {
        for (k, v) in e.iter() { if let Value::String(s) = v { cmd.env(k, s); } }
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let kind = match e.kind() {
                std::io::ErrorKind::NotFound => "not_found",
                std::io::ErrorKind::PermissionDenied => "refused",
                _ => "io",
            };
            return Ok(failure(kind, format!("run_process(\"{}\"): {}", program, e)));
        }
    };
    // stdin is written from a thread (a large input would block against a full stdout)
    let input = match get("stdin") { Some(Value::String(s)) => s.clone(), _ => String::new() };
    let mut stdin = child.stdin.take();
    let writer = std::thread::spawn(move || { if let Some(w) = stdin.as_mut() { let _ = w.write_all(input.as_bytes()); } drop(stdin); });
    // each stream read to its end from a thread, kept up to max_bytes
    let reader = |stream: Option<Box<dyn Read + Send>>| std::thread::spawn(move || {
        let mut kept: Vec<u8> = Vec::new();
        let mut cut = false;
        if let Some(mut s) = stream {
            let mut buf = [0u8; 8192];
            loop {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let room = max_bytes.saturating_sub(kept.len());
                        if n > room { cut = true; }
                        kept.extend_from_slice(&buf[..n.min(room)]);
                    }
                }
            }
        }
        (kept, cut)
    });
    let out = reader(child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let err = reader(child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>));
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if start.elapsed().as_millis() as u64 >= timeout_ms => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
            Err(e) => return Ok(failure("io", format!("run_process(\"{}\"): {}", program, e))),
        }
    };
    let _ = writer.join();
    let (stdout, cut_out) = out.join().unwrap_or_default();
    let (stderr, cut_err) = err.join().unwrap_or_default();
    let Some(status) = status else {
        return Ok(map_from_pairs(vec![
            ("error".to_string(), Value::String(format!("run_process(\"{}\"): timeout after {} ms — the process was killed", program, timeout_ms))),
            ("kind".to_string(), Value::String("timeout".to_string())),
            ("stdout".to_string(), Value::String(String::from_utf8_lossy(&stdout).into_owned())),
            ("stderr".to_string(), Value::String(String::from_utf8_lossy(&stderr).into_owned())),
        ]));
    };
    // a process ended by a signal has no exit code: -1
    let code = status.code().unwrap_or(-1);
    Ok(map_from_pairs(vec![
        ("code".to_string(), Value::Int(SomaInt::from_i64(code as i64))),
        ("stdout".to_string(), Value::String(String::from_utf8_lossy(&stdout).into_owned())),
        ("stderr".to_string(), Value::String(String::from_utf8_lossy(&stderr).into_owned())),
        ("truncated".to_string(), Value::Bool(cut_out || cut_err)),
    ]))
}
