//! V1: deterministic record / replay log.
//!
//! Every cell handler annotated `[record]` gets each invocation logged
//! to a `.somalog` JSON-lines file. `soma replay` later re-runs the
//! handlers in the original order and bit-compares the result; any
//! observable divergence is reported with the *cause* (e.g. a call to
//! `now()` that wasn't covered by the recorded clock).
//!
//! File format (one JSON object per line):
//! ```text
//! {"v":1,"ts":1738000000,"cell":"trader","handler":"tick",
//!  "args":[{"price":187.42}],"result":412,"nondet":["now_ms"]}
//! ```

use super::{Value, soma_int::SomaInt};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// Names of builtins whose return value is *not* a function of their
/// arguments — every call is a potential source of replay divergence.
pub const NONDET_BUILTINS: &[&str] = &[
    "now", "now_ms", "timestamp", "today", "date_now",
    "random", "rand",
];

#[derive(Debug, Clone)]
pub struct RecordEntry {
    pub ts_ms: i64,
    pub cell: String,
    pub handler: String,
    pub args: Vec<Value>,
    pub result: Value,
    pub nondet: Vec<String>,
}

impl RecordEntry {
    pub fn to_json_line(&self) -> String {
        let args_json: Vec<serde_json::Value> = self.args.iter().map(value_to_json).collect();
        let result_json = value_to_json(&self.result);
        let nondet_json: Vec<serde_json::Value> = self.nondet.iter()
            .map(|s| serde_json::Value::String(s.clone()))
            .collect();
        let obj = serde_json::json!({
            "v": 1,
            "ts": self.ts_ms,
            "cell": self.cell,
            "handler": self.handler,
            "args": args_json,
            "result": result_json,
            "nondet": nondet_json,
        });
        obj.to_string()
    }

    pub fn from_json_line(line: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let cell = v.get("cell")?.as_str()?.to_string();
        let handler = v.get("handler")?.as_str()?.to_string();
        let ts_ms = v.get("ts").and_then(|x| x.as_i64()).unwrap_or(0);
        let args: Vec<Value> = v.get("args")?.as_array()?
            .iter().map(json_to_value).collect();
        let result = json_to_value(v.get("result").unwrap_or(&serde_json::Value::Null));
        let nondet: Vec<String> = v.get("nondet")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        Some(Self { ts_ms, cell, handler, args, result, nondet })
    }
}

pub fn append(path: &Path, entry: &RecordEntry) -> std::io::Result<()> {
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", entry.to_json_line())?;
    Ok(())
}

pub fn read_all(path: &Path) -> std::io::Result<Vec<RecordEntry>> {
    let f = std::fs::File::open(path)?;
    let mut entries = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        if let Some(e) = RecordEntry::from_json_line(&line) {
            entries.push(e);
        }
    }
    Ok(entries)
}

/// Default log path next to a source file.
pub fn default_log_path(source: &Path) -> PathBuf {
    let mut p = source.to_path_buf();
    p.set_extension("somalog");
    p
}

// ── Value <-> serde_json conversion ──────────────────────────────────

pub fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Int(si) => match si.to_i64() {
            Some(n) => serde_json::Value::from(n),
            None => serde_json::Value::String(si.to_string()),
        },
        Value::Float(n) => serde_json::Value::from(*n),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::List(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
        Value::Map(entries) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in entries {
                obj.insert(k.clone(), value_to_json(v));
            }
            serde_json::Value::Object(obj)
        }
        Value::Lambda { .. } | Value::LambdaBlock { .. } => serde_json::Value::String("<lambda>".to_string()),
        Value::Unit => serde_json::Value::Null,
    }
}

pub fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Unit,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(SomaInt::from_i64(i))
            } else {
                Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => {
            // BigInt encoded as decimal string
            if let Ok(i) = s.parse::<i64>() {
                Value::Int(SomaInt::from_i64(i))
            } else {
                Value::String(s.clone())
            }
        }
        serde_json::Value::Array(arr) => Value::List(arr.iter().map(json_to_value).collect()),
        serde_json::Value::Object(obj) => {
            let mut map = indexmap::IndexMap::new();
            for (k, v) in obj { map.insert(k.clone(), json_to_value(v)); }
            Value::Map(map)
        }
    }
}

/// Bit-equivalence for replay divergence detection.
/// Floats use exact equality (replay should be deterministic — any drift is divergence).
pub fn values_equivalent(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.to_string() == y.to_string(),
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::List(x), Value::List(y)) =>
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| values_equivalent(a, b)),
        (Value::Map(x), Value::Map(y)) =>
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| values_equivalent(v, w))),
        (Value::Unit, Value::Unit) => true,
        _ => false,
    }
}
