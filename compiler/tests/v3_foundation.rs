// 3.0 foundation: static types for expressions (A1), parser recovery (A2),
// stable diagnostic codes (A3), editions (D1).

use std::process::Command;

fn soma_env(args: &[&str], env: &[(&str, &str)]) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soma"));
    cmd.args(args).current_dir(env!("CARGO_MANIFEST_DIR")).env_remove("SOMA_EDITION");
    for (k, v) in env { cmd.env(k, v); }
    let output = cmd.output().expect("failed to run soma");
    (String::from_utf8_lossy(&output.stdout).to_string(), String::from_utf8_lossy(&output.stderr).to_string(), output.status.code().unwrap_or(-1))
}

fn soma(args: &[&str]) -> (String, String, i32) { soma_env(args, &[]) }

/// A program in its own directory (a soma.toml may sit beside it).
fn program(name: &str, source: &str, manifest: Option<&str>) -> String {
    let dir = std::env::temp_dir().join(format!("soma_v3_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    if let Some(m) = manifest { std::fs::write(dir.join("soma.toml"), m).unwrap(); }
    let path = dir.join("app.cell");
    std::fs::write(&path, source).unwrap();
    path.display().to_string()
}

fn check_json(path: &str, env: &[(&str, &str)]) -> serde_json::Value {
    let (out, err, _) = soma_env(&["check", "--json", path], env);
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("not JSON ({e}): {out}\n{err}"))
}

fn kinds(doc: &serde_json::Value) -> Vec<String> {
    doc["errors"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap_or("").to_string()).collect()
}

fn messages(doc: &serde_json::Value) -> String {
    doc["errors"].as_array().unwrap().iter().map(|e| e["message"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

// ── A1: types ──────────────────────────────────────────────────────────

#[test]
fn a1_field_typo_on_a_record_is_a_check_error() {
    let p = program("field", r#"
cell type Line { variants { Line { sku: String, qty: Int } } }
cell A {
    on total(lines: List<Line>) {
        let t = 0
        for l in lines { t += l.qty + l.quantity }
        return t
    }
}"#, None);
    let d = check_json(&p, &[]);
    assert_eq!(kinds(&d), vec!["unknown_field"], "{d}");
    let m = messages(&d);
    assert!(m.contains("record type 'Line' has no field 'quantity'") && m.contains("[sku, qty]"), "{m}");
    assert_eq!(d["errors"][0]["line"], 6);
}

#[test]
fn a1_inferred_operand_types_are_checked() {
    let p = program("ops", r#"
cell A {
    on f(n: Int, s: String) {
        let a = s * 2
        let b = n + s
        let c = [1, 2] % 2
        let d = n == s
        let e = s < n
        let g = map("k", 1) + 1
        return 0
    }
}"#, None);
    let d = check_json(&p, &[]);
    assert_eq!(d["error_count"], 6, "{}", messages(&d));
    assert!(kinds(&d).iter().all(|k| k == "type_mismatch"));
    assert!(messages(&d).contains("cannot multiply a String and an Int: `s * 2`"));
}

/// What the interpreter accepts stays accepted (measured operator table).
#[test]
fn a1_valid_mixed_operations_still_check() {
    let p = program("valid", r#"
cell A {
    on f(n: Int, x: Float, s: String, xs: List<Int>, m: Map) {
        let a = n + x
        let b = xs + n
        let c = xs * xs
        let d = s + s
        let e = xs > 2
        let g = n == x
        let h = m == ()
        let i = n / 2
        let j = i + 1
        let k = m.anything
        let l = k == ()
        let acc = 0
        acc = "now a string"
        let z = acc + "x"
        return [a, b, c, d, e, g, h, j, l, z]
    }
}"#, None);
    let (out, err, code) = soma(&["check", &p]);
    assert_eq!(code, 0, "{out}{err}");
}

#[test]
fn a1_try_result_fields_are_known() {
    let p = program("tryf", r#"
cell A {
    on g() { return map("url", "x") }
    on f() {
        let r = try { g() }
        return r.url
    }
}"#, None);
    let d = check_json(&p, &[]);
    assert_eq!(kinds(&d), vec!["unknown_field"], "{d}");
    assert!(messages(&d).contains("`r.value.url`"), "{}", messages(&d));
}

#[test]
fn a1_inferred_argument_types_are_checked() {
    let p = program("args", r#"
cell A {
    on num(n: Int) { return n + 1 }
    on send(data: Map) { return data }
    on f(body: String) {
        let x = 2.5
        let a = num(x)
        let b = num(body)
        let c = send(body)
        let ok = num(len(body))
        return [a, b, c, ok]
    }
}"#, None);
    let d = check_json(&p, &[]);
    assert_eq!(kinds(&d), vec!["argument_type"; 3], "{}", messages(&d));
    assert!(messages(&d).contains("num(): argument 1 (`x`) is a Float but the handler declares `n: Int`"), "{}", messages(&d));
}

/// A failure inside `try { }` is a value the program asked for.
#[test]
fn a1_nothing_is_refused_inside_try() {
    let p = program("intry", r#"
cell A {
    on f(s: String) {
        let r = try { s * 2 }
        return r.kind
    }
}"#, None);
    let (out, err, code) = soma(&["check", &p]);
    assert_eq!(code, 0, "{out}{err}");
}

#[test]
fn a1_absence_is_an_error_in_edition_2027_only() {
    let src = r#"
cell A {
    memory { counts: Map<String, Int> [persistent] }
    on bump(k: String) {
        let c = counts.get(k)
        counts.set(k, c + 1)
        return c
    }
    on safe(k: String) {
        let c = counts.get(k) ?? 0
        let d = counts.get(k)
        if d == () { return 0 }
        counts.set(k, c + d + 1)
        return c
    }
    on guarded(k: String) {
        let d = counts.get(k)
        require d != () else Missing
        return d + 1
    }
}"#;
    let p = program("absent26", src, None);
    assert_eq!(check_json(&p, &[])["error_count"], 0);
    let p = program("absent27", src, Some("[package]\nedition = \"2027\"\n"));
    let d = check_json(&p, &[]);
    assert_eq!(kinds(&d), vec!["maybe_absent"], "{}", messages(&d));
    assert!(messages(&d).contains("(c ?? 0)"), "{}", messages(&d));
    assert_eq!(d["errors"][0]["line"], 6);
}

#[test]
fn a1_face_return_mismatch_warns_then_errors() {
    let src = r#"
cell A {
    face { signal f(id: String) -> String }
    on f(id: String) {
        let m = map("id", id)
        return m
    }
}"#;
    let p = program("ret26", src, None);
    let d = check_json(&p, &[]);
    assert_eq!(d["error_count"], 0, "{d}");
    assert!(d["warnings"][0]["message"].as_str().unwrap().contains("an error in edition 2027"), "{d}");
    assert_eq!(d["warnings"][0]["kind"], "type_advisory");
    let d = check_json(&p, &[("SOMA_EDITION", "2027")]);
    assert_eq!(kinds(&d), vec!["type_mismatch"], "{d}");
}

// ── A2: parser recovery ────────────────────────────────────────────────

const FIVE_SYNTAX_ERRORS: &str = r#"cell A {
    on f(x: Int) {
        let y = (a, 1) => a + 1
        let z = x + 1
        let w = xs[1:2]
        return z
    }
    on g(x: Int) {
        if x > 1 and x < 3 { return 1 }
        return 2
    }
    let K = 3
    on h() { return 1 }
}
cell B {
    on k() { let q = {a: 1}  return q }
}"#;

#[test]
fn a2_every_syntax_error_in_one_pass() {
    let p = program("five", FIVE_SYNTAX_ERRORS, None);
    let (_, err, code) = soma(&["check", &p]);
    assert_eq!(code, 1);
    assert_eq!(err.matches("error: ").count(), 5, "{err}");
    assert!(err.contains("5 syntax error(s)"), "{err}");
    let d = check_json(&p, &[]);
    assert_eq!(d["error_count"], 5);
    let lines: Vec<i64> = d["errors"].as_array().unwrap().iter().map(|e| e["line"].as_i64().unwrap()).collect();
    assert_eq!(lines, vec![3, 5, 9, 12, 16]);
    for e in d["errors"].as_array().unwrap() {
        assert_eq!(e["kind"], "syntax");
        for key in ["file", "line", "col", "fix", "code", "source_line"] {
            assert!(!e[key].is_null(), "syntax error without `{key}`: {e}");
        }
    }
}

/// A fix-it that leaves the AST complete does not hide the semantic errors.
#[test]
fn a2_complete_fixits_are_reported_with_semantic_errors() {
    let p = program("mixed", r#"cell A {
    face { signal f(x: Int) -> Int }
    on f(x: Int) -> Int {
        let z = x + undefinedd
        return match z { 1 => 1  _ => 2 }
    }
}"#, None);
    let d = check_json(&p, &[]);
    let codes: Vec<&str> = d["errors"].as_array().unwrap().iter().map(|e| e["code"].as_str().unwrap()).collect();
    assert_eq!(&codes[..3], &["E1003", "E1004", "E1004"], "{d}");
    assert!(messages(&d).contains("undefined variable 'undefinedd'"), "{d}");
}

/// Recovery never makes `run` execute a program that did not parse.
#[test]
fn a2_a_program_with_syntax_errors_does_not_run() {
    let p = program("norun", "cell A { on main() { return match 1 { 1 => 2 } } }", None);
    let (out, err, code) = soma(&["run", &p, "main"]);
    assert_ne!(code, 0, "{out}{err}");
    assert!(err.contains("match arms use '->'"), "{err}");
}

// ── A3: codes ──────────────────────────────────────────────────────────

#[test]
fn a3_every_diagnostic_carries_a_code_that_explains() {
    let p = program("codes", r#"
cell A {
    memory { n: Int }
    on f(x: Int) {
        let m = map("a", 1)
        let c = m.a ?? 0 + 1
        print("v={zz}")
        return lenn(x) + c
    }
}"#, None);
    let d = check_json(&p, &[]);
    let all: Vec<&serde_json::Value> = d["errors"].as_array().unwrap().iter().chain(d["warnings"].as_array().unwrap()).collect();
    assert!(all.len() >= 4, "{d}");
    for e in all {
        let code = e["code"].as_str().unwrap_or_else(|| panic!("no code: {e}"));
        assert!(code != "E0000" && code != "W0000", "unregistered kind: {e}");
        let (out, _, rc) = soma(&["explain", code]);
        assert_eq!(rc, 0, "soma explain {code}");
        assert!(out.starts_with(code), "{out}");
    }
    let (_, err, rc) = soma(&["explain", "E9999"]);
    assert_eq!(rc, 1);
    assert!(err.contains("no diagnostic code"));
}

#[test]
fn a3_text_report_names_the_code() {
    let p = program("codetext", "cell A { on f() { return nope(1) } }", None);
    let (out, _, _) = soma(&["check", &p]);
    assert!(out.contains("  = code: E"), "{out}");
}

// ── D1: editions ───────────────────────────────────────────────────────

#[test]
fn d1_unknown_edition_is_refused() {
    let p = program("edition", "cell A { on f() { return 1 } }", Some("[package]\nedition = \"2031\"\n"));
    let d = check_json(&p, &[]);
    assert_eq!(kinds(&d), vec!["edition"], "{d}");
    assert!(messages(&d).contains("\"2026\", \"2027\""));
}

#[test]
fn d1_init_writes_the_current_edition() {
    let dir = std::env::temp_dir().join(format!("soma_v3_init_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_soma")).args(["init", "proj"]).current_dir(&dir).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let toml = std::fs::read_to_string(dir.join("proj/soma.toml")).unwrap();
    assert!(toml.contains("edition = \"2027\""), "{toml}");
    let (o, e, code) = soma(&["check", &dir.join("proj/app.cell").display().to_string()]);
    assert_eq!(code, 0, "{o}{e}");
    let (o, e, code) = soma(&["test", &dir.join("proj/app.cell").display().to_string()]);
    assert_eq!(code, 0, "{o}{e}");
}
