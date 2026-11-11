// 3.0 — C9: a Soma program as a library. `soma embed app.cell` serves the
// program's handlers to a host process, one JSON object per line; the Python
// and npm packages (packages/python, packages/npm) are that host's side.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn repo() -> std::path::PathBuf { std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf() }

/// A scratch directory holding a copy of the fixture program.
fn fixture(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_v3e_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(repo().join("packages/fixture/app.cell"), dir.join("app.cell")).unwrap();
    dir
}

#[test]
fn c9_embed_speaks_one_json_object_per_line() {
    let dir = fixture("protocol");
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma")).args(["embed", "app.cell"]).current_dir(&dir)
        .env_remove("SOMA_EDITION").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut ask = |line: &str| -> serde_json::Value {
        if !line.is_empty() { writeln!(stdin, "{}", line).unwrap(); stdin.flush().unwrap(); }
        let mut reply = String::new();
        out.read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap_or_else(|e| panic!("not JSON ({e}): {reply:?}"))
    };
    // the first line: the handlers and their typed parameters
    let ready = ask("");
    assert_eq!(ready["ready"], true, "{ready}");
    assert_eq!(ready["cells"]["Ledger"]["debit"]["params"][0]["type"], "Int", "{ready}");
    assert_eq!(ready["cells"]["Ledger"]["debit"]["returns"], "Int", "{ready}");

    assert_eq!(ask(r#"{"id": 1, "call": "seed", "args": [1000]}"#), serde_json::json!({"id": 1, "ok": true, "value": 1000}));
    assert_eq!(ask(r#"{"id": "b", "call": "Ledger.debit", "args": {"amount": 300}}"#), serde_json::json!({"id": "b", "ok": true, "value": 700}));
    // an error keeps its kind; the failed call wrote nothing
    let refused = ask(r#"{"id": 3, "call": "debit", "args": [5000]}"#);
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"]["kind"], "invariant", "{refused}");
    assert_eq!(ask(r#"{"id": 4, "call": "balance"}"#)["value"], 700);
    assert_eq!(ask(r#"{"id": 5, "call": "debit", "args": [-1]}"#)["error"]["kind"], "requires");
    assert_eq!(ask(r#"{"id": 6, "call": "find", "args": [""]}"#)["error"]["kind"], "BadId");
    // an Int a JSON number cannot carry is tagged, both ways
    assert_eq!(ask(r#"{"id": 7, "call": "big"}"#)["value"], serde_json::json!({"__soma_int__": "1180591620717411303424"}));
    assert_eq!(ask(r#"{"id": 8, "call": "echo", "args": [{"__soma_int__": "1180591620717411303425"}]}"#)["value"]["__soma_int__"], "1180591620717411303425");
    assert_eq!(ask(r#"{"id": 9, "call": "echo", "args": [9007199254740993]}"#)["value"], serde_json::json!({"__soma_int__": "9007199254740993"}));
    // a variant of the program's types, out and in
    let pay = ask(r#"{"id": 10, "call": "classify", "args": [40]}"#);
    assert_eq!(pay["value"], serde_json::json!({"_type": "Decision", "_variant": "Pay", "amount": 40}));
    assert_eq!(ask(r#"{"id": 11, "call": "label", "args": [{"_type": "Decision", "_variant": "Hold", "reason": "manual"}]}"#)["value"], "hold: manual");
    assert_eq!(ask(r#"{"id": 12, "call": "label", "args": [{"_type": "Decision", "_variant": "Refund"}]}"#)["error"]["kind"], "type");
    // print() does not reach the protocol
    assert_eq!(ask(r#"{"id": 13, "call": "shout", "args": ["hi"]}"#)["value"], "HI");
    // what is not a call
    assert_eq!(ask(r#"{"id": 14, "call": "nope"}"#)["error"]["kind"], "no_handler");
    assert_eq!(ask(r#"{"id": 15, "call": "uppercase", "args": ["x"]}"#)["error"]["kind"], "no_handler", "a builtin is not a handler");
    assert_eq!(ask(r#"{"id": 16, "call": "debit", "args": {"sum": 1}}"#)["error"]["kind"], "type");
    assert_eq!(ask(r#"{"id": 17}"#)["error"]["kind"], "protocol");
    assert_eq!(ask("this is not json")["error"]["kind"], "protocol");
    assert_eq!(ask(r#"{"id": 18, "close": true}"#)["ok"], true);
    drop(stdin);
    assert!(child.wait().unwrap().success());

    // a program that fails `soma check` is refused on the first line
    std::fs::write(dir.join("broken.cell"), "cell B {\n    on f() { return undefined_name }\n}\n").unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(["embed", "broken.cell"]).current_dir(&dir).env_remove("SOMA_EDITION").stdin(Stdio::null()).output().unwrap();
    assert!(!o.status.success());
    let first: serde_json::Value = serde_json::from_str(String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("")).unwrap();
    assert_eq!(first["ready"], false, "{first}");
    assert!(first["errors"][0]["message"].as_str().unwrap_or("").contains("undefined"), "{first}");
}

/// Run a client's own test against the binary under test. Skipped when the
/// host language is not installed here.
fn client(program: &str, script: &str, name: &str) {
    if Command::new(program).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().map_or(true, |s| !s.success()) {
        eprintln!("{program} is not installed here: the {name} client was not run");
        return;
    }
    let dir = fixture(name);
    let o = Command::new(program).arg(repo().join(script)).arg(dir.join("app.cell"))
        .env("SOMA_BIN", env!("CARGO_BIN_EXE_soma")).env_remove("SOMA_EDITION").output().unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(o.status.success() && out.contains(&format!("{name}: ok")), "{out}");
}

#[test]
fn c9_the_python_package_calls_handlers_and_gets_errors_by_kind() {
    client("python3", "packages/python/tests/test_embed.py", "python");
}

#[test]
fn c9_the_npm_package_calls_handlers_and_gets_errors_by_kind() {
    client("node", "packages/npm/test.js", "node");
}
