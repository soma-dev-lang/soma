//! `soma eval` (3.0): a regression gate for what a handler answers.
//!
//! A dataset is a JSON-lines file, one case per line:
//!   {"input": "refund please", "expected": "billing"}
//!   {"input": ["acme", 200], "expected": {"state": "paid"}, "name": "small invoice"}
//! `input` is the handler's argument (a list: its arguments). Each case is
//! graded — `exact` (==, structural), `contains` (the answer's text holds
//! the expected text) or `handler:Cell.judge` (your own grader, called as
//! judge(input, answer, expected), which answers a Bool) — and the pass
//! rate is held to `--threshold`. The run is stored beside the program
//! (`.soma_eval/<handler>.json`) and compared with the previous one: a case
//! that passed and now fails is named.
//!
//! Cases run as generated `cell test` rules, so storage is isolated and
//! think() follows the same rule as `soma test`: the configured model when
//! a key is set, the mock otherwise.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// A JSON value as Soma source.
fn literal(v: &Value) -> String {
    match v {
        Value::Null => "()".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            let s = n.to_string();
            if s.starts_with('-') { format!("({s})") } else { s }
        }
        Value::String(s) => {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    // a brace would start an interpolation
                    '{' => out.push_str("{{"),
                    '}' => out.push_str("}}"),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        Value::Array(items) => format!("[{}]", items.iter().map(literal).collect::<Vec<_>>().join(", ")),
        Value::Object(m) => format!("map({})", m.iter().map(|(k, v)| format!("{}, {}", literal(&Value::String(k.clone())), literal(v))).collect::<Vec<_>>().join(", ")),
    }
}

struct Case {
    name: String,
    args: String,
    input: String,
    expected: String,
}

fn load_cases(path: &Path) -> Result<Vec<Case>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;
    let mut cases = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") { continue; }
        let v: Value = serde_json::from_str(line).map_err(|e| format!("{}:{}: not a JSON object ({})", path.display(), i + 1, e))?;
        let Some(obj) = v.as_object() else { return Err(format!("{}:{}: a case is a JSON object {{\"input\": …, \"expected\": …}}", path.display(), i + 1)) };
        if let Some(k) = obj.keys().find(|k| !matches!(k.as_str(), "input" | "expected" | "name")) {
            return Err(format!("{}:{}: unknown key \"{}\" — a case has input, expected and (optionally) name", path.display(), i + 1, k));
        }
        let (Some(input), Some(expected)) = (obj.get("input"), obj.get("expected")) else {
            return Err(format!("{}:{}: a case needs \"input\" and \"expected\"", path.display(), i + 1));
        };
        let args = match input {
            Value::Array(items) => items.iter().map(literal).collect::<Vec<_>>().join(", "),
            one => literal(one),
        };
        cases.push(Case {
            name: obj.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()).unwrap_or_else(|| format!("case {}", cases.len() + 1)),
            args,
            input: literal(input),
            expected: literal(expected),
        });
    }
    if cases.is_empty() { return Err(format!("{}: no case", path.display())); }
    Ok(cases)
}

pub fn cmd_eval(file: &PathBuf, handler: &str, dataset: &PathBuf, grade: &str, threshold: f64, json: bool) {
    let fail = |msg: String| -> ! {
        if json { println!("{}", serde_json::json!({"ok": false, "error": msg})); } else { eprintln!("error: {msg}"); }
        std::process::exit(1)
    };
    if !(0.0..=1.0).contains(&threshold) { fail(format!("--threshold is a pass rate between 0 and 1, got {threshold}")); }
    let cases = load_cases(dataset).unwrap_or_else(|e| fail(e));
    let judge = grade.strip_prefix("handler:");
    if judge.is_none() && !matches!(grade, "exact" | "contains") {
        fail(format!("--grade is exact, contains or handler:Cell.judge — got \"{grade}\""));
    }
    let source = std::fs::read_to_string(file).unwrap_or_else(|e| fail(format!("cannot read '{}': {}", file.display(), e)));
    // one rule per case, in a test cell appended to a copy of the program
    let mut rules = String::new();
    for c in &cases {
        let call = format!("{}({})", handler, c.args);
        rules.push_str(&match (grade, judge) {
            (_, Some(j)) => format!("        assert {}({}, {}, {}) == true\n", j, c.input, call, c.expected),
            ("contains", _) => format!("        assert contains(to_string({}), to_string({}))\n", call, c.expected),
            _ => format!("        assert {} == {}\n", call, c.expected),
        });
    }
    let dir = file.parent().filter(|d| !d.as_os_str().is_empty()).map(|d| d.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
    // beside the program: its `use` paths and soma.toml resolve as usual
    let tmp = dir.join(format!(".{stem}.eval-{}.cell", std::process::id()));
    let program = format!("{}\n\ncell test SomaEvalCases {{\n    rules {{\n{}    }}\n}}\n", source.trim_end(), rules);
    std::fs::write(&tmp, program).unwrap_or_else(|e| fail(format!("cannot write '{}': {}", tmp.display(), e)));
    let exe = std::env::current_exe().unwrap_or_else(|e| fail(format!("cannot locate the soma binary: {e}")));
    let out = std::process::Command::new(exe).arg("test").arg("--json").arg(&tmp).output();
    let _ = std::fs::remove_file(&tmp);
    let out = out.unwrap_or_else(|e| fail(format!("cannot run soma test: {e}")));
    let doc: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| fail(format!("the program does not run under `soma test`: {}", String::from_utf8_lossy(&out.stderr).trim())));
    // the program's own test cells ran too: the cases are SomaEvalCases' rules
    let all: Vec<Value> = doc["results"].as_array().cloned().unwrap_or_default().into_iter()
        .filter(|r| r["cell"] == "SomaEvalCases").collect();
    if all.len() != cases.len() {
        let why = doc["error"].as_str().map(|s| s.to_string())
            .or_else(|| doc["check_errors"].as_array().and_then(|a| a.first()).and_then(|e| e.as_str().map(|s| s.to_string())))
            .or_else(|| doc["errors"].as_array().and_then(|a| a.first()).and_then(|e| e["message"].as_str().map(|s| s.to_string())))
            .unwrap_or_else(|| "soma test reported no result for the cases".to_string());
        fail(format!("the cases could not run: {why}"));
    }
    let results = &all[..];
    let mut rows = Vec::new();
    let mut passed = 0usize;
    for (c, r) in cases.iter().zip(results) {
        let ok = r["status"] == "pass";
        if ok { passed += 1; }
        // what it answered against what was expected, or what it raised
        let detail = match (r["left"].as_str(), r["right"].as_str()) {
            (Some(got), Some(want)) => format!("answered {got}, expected {want}"),
            _ => r["message"].as_str().unwrap_or("").to_string(),
        };
        rows.push(serde_json::json!({"name": c.name, "passed": ok, "detail": if ok { String::new() } else { detail }}));
    }
    let rate = passed as f64 / cases.len() as f64;

    // compare with the previous run of this handler, then store this one
    let store_dir = dir.join(".soma_eval");
    let store = store_dir.join(format!("{}.{}.{}.json", stem, handler.replace('.', "_"), grade.replace([':', '.'], "_")));
    let previous: Option<Value> = std::fs::read_to_string(&store).ok().and_then(|t| serde_json::from_str(&t).ok());
    let was_passing = |name: &str| previous.as_ref().and_then(|p| p["cases"].as_array().cloned()).unwrap_or_default().iter()
        .any(|c| c["name"] == name && c["passed"] == true);
    let regressions: Vec<String> = rows.iter().filter(|r| r["passed"] == false && was_passing(r["name"].as_str().unwrap_or("")))
        .map(|r| r["name"].as_str().unwrap_or("").to_string()).collect();
    let ok = rate + 1e-12 >= threshold;
    let report = serde_json::json!({
        "ok": ok, "handler": handler, "dataset": dataset.display().to_string(), "grade": grade,
        "cases": rows, "total": cases.len(), "passed": passed, "pass_rate": rate, "threshold": threshold,
        "previous_pass_rate": previous.as_ref().and_then(|p| p["pass_rate"].as_f64()),
        "regressions": regressions,
    });
    let _ = std::fs::create_dir_all(&store_dir);
    if let Err(e) = std::fs::write(&store, serde_json::to_string_pretty(&report).unwrap() + "\n") {
        eprintln!("warning: cannot store the run in '{}': {}", store.display(), e);
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        for r in report["cases"].as_array().unwrap() {
            let name = r["name"].as_str().unwrap_or("");
            if r["passed"] == true { println!("  \u{2713} {name}"); } else {
                println!("  \u{2717} {name}{}", r["detail"].as_str().filter(|d| !d.is_empty()).map(|d| format!(" — {d}")).unwrap_or_default());
            }
        }
        let before = report["previous_pass_rate"].as_f64().map(|p| format!(" (previous run: {:.0}%)", p * 100.0)).unwrap_or_default();
        println!("\n{} / {} cases pass — {:.0}%, threshold {:.0}%{}", passed, cases.len(), rate * 100.0, threshold * 100.0, before);
        if !regressions.is_empty() {
            println!("regressions since the previous run: {}", regressions.join(", "));
        }
        println!("{}", if ok { "EVAL OK" } else { "EVAL FAILED" });
    }
    if !ok { std::process::exit(1); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_values_become_soma_source() {
        let v: Value = serde_json::from_str(r#"{"a": [1, -2.5, "x {y} \"q\"", null, true], "b": {"c": 1}}"#).unwrap();
        assert_eq!(literal(&v), r#"map("a", [1, (-2.5), "x {{y}} \"q\"", (), true], "b", map("c", 1))"#);
    }
}
