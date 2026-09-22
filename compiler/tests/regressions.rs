//! Reproductions from the September 2026 language audit.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_regression_{}_{}", std::process::id(), name));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn soma(dir: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn passes(name: &str, src: &str) {
    let dir = scratch(name);
    std::fs::write(dir.join("app.cell"), src).unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn ranges_and_collection_counts_reject_invalid_arguments() {
    passes(
        "counts",
        r#"
cell P {
    on zero_step() { let xs = [] for i in range(0, 5, 0) { xs = push(xs, i) } return xs }
    on descending() { let xs = [] for i in range(5, -1, -2) { xs = push(xs, i) } return xs }
    on short_range() { for i in range(0, 1000000000, 2) { return i } return -1 }
}
cell test T { rules {
    assert_fails range(0, 5, 0) matching "range"
    assert_fails zero_step() matching "range"
    assert descending() == [5, 3, 1]
    assert short_range() == 0
    assert_fails range(0, 20000001, 2) matching "limit"
    assert range(-9223372036854775808, 9223372036854775807, 9223372036854775807) == [-9223372036854775808, -1, 9223372036854775806]
    assert_fails top([1, 2], -1) matching "range"
    assert_fails bottom([1, 2], -1) matching "range"
    assert_fails top([1, 2], 1.5) matching "type"
    assert_fails bottom([1, 2], "1") matching "type"
    assert_fails top(42, 1) matching "type"
    assert_fails slice([1, 2], 0.5) matching "type"
    assert_fails slice("abc", 0, 1.5) matching "type"
    assert_fails min("oops", 1) matching "type"
    assert_fails max(1.0, "oops") matching "type"
    assert top([1, 2], 0) == []
    assert bottom([1, 2], 0) == []
    assert top([1, 2], 9999999999999999999999) == [1, 2]
} }
"#,
    );
}

#[test]
fn range_loop_respects_user_dispatch_and_evaluates_arguments_once() {
    passes(
        "range_dispatch",
        r#"
cell P {
    on range(a: Int, b: Int) { return [42] }
    on run() { let xs = [] for i in range(0, 2) { xs = push(xs, i) } return xs }
}
cell test T { rules { assert run() == [42] } }
"#,
    );
    let dir = scratch("range_once");
    std::fs::write(
        dir.join("app.cell"),
        r#"
cell P {
    on bound() { print("bound-called") return "bad" }
    on run() { for i in range(bound(), 2) { print(i) } }
}
"#,
    )
    .unwrap();
    let (code, out) = soma(&dir, &["run", "app.cell", "run"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(
        out.lines().filter(|line| *line == "bound-called").count(),
        1,
        "{out}"
    );
}

#[test]
fn mixed_numbers_compare_exactly_in_operators_collections_and_pipelines() {
    passes(
        "numbers",
        r#"
cell test T { rules {
    assert 9007199254740993 > 9007199254740992.0
    assert 9007199254740992.0 < 9007199254740993
    assert 9007199254740993 != 9007199254740992.0
    assert ([9007199254740993] > 9007199254740992.0) == [1.0]
    assert ([[9007199254740993]] > 9007199254740992.0) == [[1.0]]
    assert ([9007199254740992.0] < 9007199254740993) == [1.0]
    assert_fails ([[1], [2, 3]] > 0) matching "ragged"
    assert [9007199254740993] != [9007199254740992.0]
    assert !contains([9007199254740993], 9007199254740992.0)
    assert len(distinct([9007199254740993, 9007199254740992.0])) == 2
    assert len(distinct([[1], [1.0]])) == 1
    assert len(distinct([map("a", 1, "b", 2), map("b", 2, "a", 1.0)])) == 1
    assert len(distinct([map("n", sqrt(-1.0)), map("n", sqrt(-1.0))], "n")) == 2
    assert len(distinct_by([map("n", 1), map("n", "1"), map("n", 1.0)], "n")) == 2
    assert sort([9007199254740993, 9007199254740992.0]) == [9007199254740992.0, 9007199254740993]
    assert sort_by([map("n", 9007199254740993), map("n", 9007199254740992.0)], "n")[0].n == 9007199254740992.0
    assert filter_by([map("n", 9007199254740993)], "n", ">", 9007199254740992.0).len == 1
    assert filter_by([map("n", sqrt(-1.0))], "n", "==", 1).len == 0
    assert filter_by([map("n", sqrt(-1.0))], "n", ">=", 1).len == 0
    assert filter_by([map("n", sqrt(-1.0))], "n", "!=", 1).len == 1
    assert 9999999999999999999999999999 < 1e100
    assert -9007199254740993 < -9007199254740992.0
    assert 1 == 1.0
} }
"#,
    );
}

#[test]
fn numeric_comparisons_agree_with_legacy_flag_and_native_code() {
    let dir = scratch("vm_numbers");
    for (expr, expected) in [
        ("9007199254740993 > 9007199254740992.0", "true"),
        ("9007199254740993 == 9007199254740992.0", "false"),
        ("sqrt(-1.0) == 1.0", "false"),
        ("sqrt(-1.0) != 1.0", "true"),
        ("sqrt(-1.0) <= 1.0", "false"),
        ("sqrt(-1.0) >= 1.0", "false"),
    ] {
        std::fs::write(
            dir.join("app.cell"),
            format!("cell P {{ on run() {{ return {expr} }} }}"),
        )
        .unwrap();
        let (code, out) = soma(&dir, &["run", "--jit", "app.cell"]);
        assert_eq!(code, 0, "{out}");
        assert_eq!(out.lines().next(), Some(expected), "{expr}: {out}");
    }
    passes(
        "native_numbers",
        r#"
cell P {
    on greater(a: Int, b: Float) [native] { return a > b }
    on lesser(a: Float, b: Int) [native] { return a < b }
    on collision(l: Float, r: Int) [native] { return r > l }
    on rank(a: Int, b: Float) [native] {
        if a < b { return -1 }
        if a > b { return 1 }
        if a == b { return 0 }
        return 2
    }
}
cell test T { rules {
    assert greater(9007199254740993, 9007199254740992.0)
    assert lesser(9007199254740992.0, 9007199254740993)
    assert collision(9007199254740992.0, 9007199254740993)
    assert rank(9007199254740993, 9007199254740992.0) == 1
    assert rank(9223372036854775807, 9223372036854775808.0) == -1
    assert rank(-9223372036854775808, -9223372036854775808.0) == 0
    assert rank(-9007199254740993, -9007199254740992.0) == -1
    assert rank(1, 1.5) == -1
    assert rank(-1, -1.5) == 1
    assert rank(1000000000000000000000000000001, 1e30) == -1
    assert rank(1, sqrt(-1.0)) == 2
} }
"#,
    );
}

struct Server(std::process::Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(port: u16, path: &str) -> String {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(format!("GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").as_bytes())
        .unwrap();
    let mut out = String::new();
    loop {
        let mut byte = [0];
        assert_eq!(
            stream.read(&mut byte).unwrap(),
            1,
            "incomplete HTTP headers: {out}"
        );
        out.push(byte[0] as char);
        if out.ends_with("\r\n\r\n") {
            break;
        }
    }
    let length: usize = out
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .expect("HTTP response must have Content-Length");
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    out.push_str(std::str::from_utf8(&body).unwrap());
    out
}

#[test]
fn query_flags_and_encoded_plus_survive_http_decoding() {
    let dir = scratch("query");
    std::fs::write(
        dir.join("app.cell"),
        r#"
cell App {
    face { signal echo(text: String) -> String }
    on echo(text: String) { return text }
    on request(method: String, path: String, body: String, query: Map) { return query }
}
"#,
    )
    .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_soma"))
            .args([
                "serve",
                "app.cell",
                "--no-schedule",
                "-p",
                &port.to_string(),
            ])
            .current_dir(&dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let out = get(port, "/query?flag&text=a%2Bb+c&%2Bkey=%2B&&");
    assert!(out.contains("200 OK"), "{out}");
    let body: serde_json::Value =
        serde_json::from_str(out.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["flag"], "", "{out}");
    assert_eq!(body["text"], "a+b c", "{out}");
    assert_eq!(body["+key"], "+", "{out}");
    assert!(body.get("").is_none(), "{out}");
    assert!(get(port, "/echo?text=a%2Bb+c").contains("a+b c"));
    assert!(get(port, "/echo?text").contains("200 OK"));
}

#[test]
fn fix_reports_syntax_repairs_in_json() {
    let dir = scratch("fix");
    std::fs::write(
        dir.join("app.cell"),
        "cell P { on run() -> Int { return 1 } }",
    )
    .unwrap();
    let (code, out) = soma(&dir, &["fix", "app.cell", "--json"]);
    assert_eq!(code, 0, "{out}");
    let result: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(result["fix_count"], 1, "{out}");
    assert_eq!(result["fixes"].as_array().unwrap().len(), 1);
    assert_eq!(result["passed"], true);
}

#[test]
fn replay_distinguishes_an_error_from_a_returned_map() {
    for (name, before, after) in [
        (
            "return_to_error",
            "return map(\"__error__\", \"rejected\")",
            "fail(\"rejected\")",
        ),
        (
            "error_to_return",
            "fail(\"rejected\")",
            "return map(\"__error__\", \"rejected\")",
        ),
    ] {
        let dir = scratch(name);
        let log = dir.join("app.somalog");
        let _ = std::fs::remove_file(&log);
        std::fs::write(
            dir.join("app.cell"),
            format!("cell P {{ on run() {{ {before} }} }}"),
        )
        .unwrap();
        soma(&dir, &["run", "--record", "app.cell"]);
        assert!(log.exists());
        let (code, out) = soma(&dir, &["replay", "app.cell"]);
        assert_eq!(code, 0, "unchanged program must replay: {out}");
        std::fs::write(
            dir.join("app.cell"),
            format!("cell P {{ on run() {{ {after} }} }}"),
        )
        .unwrap();
        let (code, out) = soma(&dir, &["replay", "app.cell"]);
        assert_ne!(
            code, 0,
            "a return changed into an error, or vice versa: {out}"
        );
    }
}

#[test]
fn native_failures_are_recorded_and_replay_logs_validate_the_outcome() {
    let dir = scratch("native_record");
    let log = dir.join("app.somalog");
    let _ = std::fs::remove_file(&log);
    std::fs::write(
        dir.join("app.cell"),
        "cell P { on run(a: Int, b: Int) [native] { return idiv(a, b) } }",
    )
    .unwrap();
    let (code, out) = soma(&dir, &["run", "--record", "app.cell", "8", "0"]);
    assert_eq!(code, 1, "{out}");
    let text = std::fs::read_to_string(&log).expect("native errors must be recorded");
    assert_eq!(text.lines().count(), 1);
    let entry: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert!(entry["error_kind"].is_string());
    let (code, out) = soma(&dir, &["replay", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("raised"), "{out}");
    for version in [1, 2, 99] {
        let mut edited = entry.clone();
        edited["v"] = serde_json::json!(version);
        edited.as_object_mut().unwrap().remove("error_kind");
        std::fs::write(&log, format!("{}\n", edited)).unwrap();
        let (code, out) = soma(&dir, &["replay", "app.cell"]);
        if version == 1 {
            assert_eq!(code, 0, "legacy v1 must still replay: {out}");
        } else {
            assert_ne!(code, 0, "invalid or unknown log format must fail: {out}");
        }
    }
}

#[test]
fn cost_bounds_cannot_wrap_or_assume_a_shadowed_range_is_builtin() {
    let dir = scratch("costs");
    for source in [
        r#"cell agent A { cost { tokens: 10 } on run() { for i in range(-1, 9223372036854775807) { think("x", map("max_tokens", 1)) } } }"#,
        r#"cell agent A { cost { tokens: 1 } on range(a: Int, b: Int) { return [1, 2, 3] } on run() { for i in range(0, 0) { think("x", map("max_tokens", 1)) } } }"#,
        r#"cell agent A { cost { tokens: 10 } on run() { for [loop_bound(9223372036854775808)] i in [1] { think("x", map("max_tokens", 1)) } } }"#,
        r#"cell agent A { cost { tokens: 10 } on run() { for [loop_bound(9223372036854775807)] i in [1] { think("x", map("max_tokens", 2)) } think("x", map("max_tokens", 2)) } }"#,
        r#"cell agent A { cost { tokens: 10 } on run() { while [loop_bound(9223372036854775808)] true { think("x", map("max_tokens", 1)) } } }"#,
    ] {
        std::fs::write(dir.join("app.cell"), source).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_eq!(
            code, 1,
            "an unknown or overflowing bound must fail check: {source}\n{out}"
        );
        assert!(out.contains("cost") || out.contains("loop_bound"), "{out}");
    }
    std::fs::write(
        dir.join("soma.toml"),
        "[models.priced]\nmodel = \"gpt-4o\"\nprovider = \"openai\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("app.cell"),
        r#"
cell agent A [model: priced] {
    cost { usd: 100000000000000000000.0 }
    on run() { for i in range(0, 2000000000000000000) { think("x", map("max_tokens", 1)) } }
}
"#,
    )
    .unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 1, "a saturated USD estimate is not a proof: {out}");
    assert!(out.contains("usd") && out.contains("overflows"), "{out}");
}

#[test]
fn environment_does_not_list_resolver_git_checkouts_as_packages() {
    let dir = scratch("env_packages");
    std::fs::create_dir_all(dir.join(".soma_env/packages/matrix")).unwrap();
    std::fs::create_dir_all(dir.join(".soma_env/packages/_git_matrix/.git")).unwrap();
    let (code, out) = soma(&dir, &["env"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("matrix"), "{out}");
    assert!(!out.contains("_git_matrix"), "{out}");
}

#[test]
fn dates_validate_types_and_invalid_mock_clocks_fail_tests() {
    passes(
        "dates",
        r#"
cell test T { rules {
    assert_fails days_in_month("oops", 2) matching "type"
    assert_fails days_in_month(2024, 2.5) matching "type"
    assert days_in_month(2024, 2) == 29
    assert days_in_month(1900, 2) == 28
    assert_fails days_in_month(2024, 13) matching "date"
} }
"#,
    );
    let dir = scratch("bad_clock");
    for clock in ["99999999999999999999999999999", "1e100", "sqrt(-1.0)"] {
        std::fs::write(
            dir.join("app.cell"),
            format!("cell test T {{ rules {{ mock now {clock} assert true }} }}"),
        )
        .unwrap();
        let (code, out) = soma(&dir, &["test", "app.cell"]);
        assert_ne!(
            code, 0,
            "invalid clock silently used the real clock: {clock}\n{out}"
        );
        assert!(out.contains("mock now") && out.contains("range"), "{out}");
    }
}

#[test]
fn unicode_slices_and_list_search_preserve_their_contracts() {
    passes(
        "unicode_contracts",
        r#"
cell test T { rules {
    assert chr(65) == "A"
    assert chr(128512) == "😀"
    assert ord(chr(1114111)) == 1114111
    assert_fails chr(4294967361) matching "range"
    assert_fails chr(-4294967231) matching "range"
    assert_fails chr(55296) matching "range"
    assert_fails chr(1114112) matching "range"
    assert_fails chr(65.5) matching "type"
    assert_fails chr("65") matching "type"
    assert substring("abc", 0, -1) == ""
    assert substring("a😀éz", -4, 3) == "a😀é"
    assert substring("abc", 2, 1) == ""
    assert substring("abc", 0, 999) == "abc"
    assert index_of([9007199254740993], 9007199254740992.0) == -1
    assert index_of([9007199254740992.0], 9007199254740993) == -1
    assert index_of([1], 1.0) == 0
    assert index_of([sqrt(-1.0)], sqrt(-1.0)) == -1
    assert pad_left(100000000000000000000, 23, "0") == "00100000000000000000000"
    assert_fails pad_left("x", 100000000000000000000) matching "range"
} }
"#,
    );
}

#[test]
fn math_builtins_reject_silent_type_coercions() {
    let expressions = [
        "round(\"bad\")",
        "floor(\"bad\")",
        "ceil(false)",
        "sqrt(\"bad\")",
        "log(\"bad\")",
        "ln(false)",
        "exp([])",
        "log10(\"bad\")",
        "band(\"3\", 2)",
        "bor(1, 2.5)",
        "bxor(false, 3)",
        "bnot(\"1\")",
        "shl(1, 2.5)",
        "shr(\"4\", 1)",
        "bit_set(1, \"2\")",
        "bit_test(1.5, 0)",
        "bit_clr(1, false)",
        "bit_next(1, 0.5)",
        "bit_len(\"3\")",
        "gcd(4.5, 2)",
        "sqrt_int(4.5)",
        "pow_mod(3, 2.5, 7)",
        "str_at(\"abc\", 1.5)",
        "random(2.5)",
    ];
    let mut src = String::from("cell test T { rules {\n");
    for expression in expressions {
        src.push_str(&format!("assert_fails {expression} matching \"type\"\n"));
    }
    src.push_str("assert round(1.5) == 2\nassert floor(-1.5) == -2\nassert ceil(-1.5) == -1\n} }");
    passes("math_types", &src);
}

#[test]
fn bit_counts_do_not_become_zero_when_they_exceed_i64() {
    passes(
        "bit_counts",
        r#"
cell test T { rules {
    assert shr(7, 999999999999999999999) == 0
    assert shr(-7, 999999999999999999999) == -1
    assert_fails shr(7, -999999999999999999999) matching "out of range"
    assert_fails shl(7, 999999999999999999999) matching "range"
    assert_fails bit_set(0, 999999999999999999999) matching "range"
    assert_fails bit_test(1, 999999999999999999999) matching "range"
    assert_fails bit_clr(-1, 999999999999999999999) matching "range"
    assert_fails bit_next(1, 999999999999999999999) matching "range"
    assert_fails bit_set(0, 16777216) matching "range"
    assert_fails bit_clr(-1, 16777216) matching "range"
    assert bit_test(-1, 16777216) == 1
    assert bit_next(-1, 64) == 64
    assert bit_len(shl(1, 80)) == 81
} }
"#,
    );
}

#[test]
fn deprecated_jit_flag_preserves_results_errors_and_dispatch() {
    let dir = scratch("jit_compatibility");
    for (expr, expected) in [
        ("7 / 2", "3.5"),
        ("[1, 2] == [1, 2]", "true"),
        ("[1, 2] + [3, 4]", "[4, 6]"),
        ("to_json(\"a\\n\\\"b\")", "\"a\\n\\\"b\""),
    ] {
        std::fs::write(
            dir.join("app.cell"),
            format!("cell P {{ on run() {{ return {expr} }} }}"),
        )
        .unwrap();
        let (code, out) = soma(&dir, &["run", "--jit", "app.cell"]);
        assert_eq!(code, 0, "{out}");
        assert_eq!(out.lines().next(), Some(expected), "{expr}: {out}");
    }
    std::fs::write(
        dir.join("app.cell"),
        "cell P { on run() { return idiv(1, 0) } }",
    )
    .unwrap();
    let (code, out) = soma(&dir, &["run", "--jit", "app.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("division by zero"), "{out}");
    std::fs::write(
        dir.join("app.cell"),
        "cell A { on value() { return 1 } } cell B { on value() { return 2 } }",
    )
    .unwrap();
    let (code, out) = soma(&dir, &["run", "--jit", "app.cell", "B.value"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.lines().next(), Some("2"), "{out}");
}

#[test]
fn native_shifts_and_random_support_wide_bounds() {
    passes(
        "native_wide_bounds",
        r#"
cell P {
    on shift(a: Int, n: Int) [native] { return shr(a, n) }
    on draw(lo: Int, hi: Int) [native] { return random(lo, hi) }
    on next_negative(n: Int) [native] { let a = -1 return bit_next(a, n) }
    on next_positive(n: Int) [native] { let a = 1 return bit_next(a, n) }
}
cell test T { rules {
    assert next_negative(63) == 63
    assert next_negative(64) == 64
    assert next_negative(80) == 80
    assert next_positive(64) == -1
    assert shift(7, 999999999999999999999) == 0
    assert shift(-7, 999999999999999999999) == -1
    assert_fails shift(7, -999999999999999999999) matching "out of range"
    assert draw(-9223372036854775808, 9223372036854775807) >= -9223372036854775808
    assert draw(-9223372036854775808, 9223372036854775807) < 9223372036854775807
    assert random(-9223372036854775808, 9223372036854775807) >= -9223372036854775808
    assert random(-9223372036854775808, 9223372036854775807) < 9223372036854775807
} }
"#,
    );
}

#[test]
fn while_optimization_preserves_inputs_iteration_order_and_errors() {
    passes(
        "while_semantics",
        r#"
cell P {
    on outer() { let i = 0 let total = 0 let k = 5 while i < 3 { total += k i += 1 } return total }
    on overflow() { let i = 0 let a = 9223372036854775807 while i < 2 { i += 1 a += 1 } return a }
    on early() { let i = 0 while i < 3 { i += 1 return i } return -1 }
    on fraction() { let i = 0 let a = 5 while i < 2 { a /= 2 i += 1 } return a }
    on divide_zero() { let i = 0 let a = 5 while i < 2 { a /= 0 i += 1 } return a }
    on float_counter() { let i = 0.0 let a = 0 while i < 3 { a += 1 i += 0.5 } return a }
    on changed_type() { let i = 0 while i < 3 { i = "bad" } }
    on nested() { let i = 0 let a = 0 while i < 3 { a += i * i i += 1 } return a }
    on breaking() { let i = 0 while i < 3 { i += 1 break } return i }
    on size_limit() { let i = 0 let a = 2 while i < 30 { a *= a i += 1 } return i }
}
cell test T { rules {
    assert outer() == 15
    assert overflow() == 9223372036854775809
    assert early() == 1
    assert fraction() == 1.25
    assert_fails divide_zero() matching "division by zero"
    assert float_counter() == 6
    assert_fails changed_type() matching "type"
    assert nested() == 5
    assert breaking() == 1
    assert_fails size_limit() matching "range"
} }
"#,
    );
}

#[test]
fn native_temporaries_do_not_shadow_user_arguments() {
    passes(
        "native_names",
        r#"
cell P {
    on rs(_a: Int, _k: Int) [native] { return shr(_k, _a) }
    on ls(_a: Int, _k: Int) [native] { return shl(_k, _a) }
    on bt(_a: Int, _k: Int) [native] { return bit_test(_k, _a) }
    on bs(_a: Int, _k: Int) [native] { return bit_set(_k, _a) }
    on bc(_a: Int, _k: Int) [native] { return bit_clr(_k, _a) }
    on pm(_m: Int, _e: Int) [native] { return pow_mod(_m, _e, 5) }
    on bs_big(_t: Int, n: Int) [native] { return bit_set(n, _t) }
    on lo(_a: Int, _b: Int) [native] { return min(_b, _a) }
    on mask(_x: Int, _y: Int) [native] { return band(_y, bnot(_x)) }
    on bounded(_lb_n: Int, _soma_tmp_0: Int) [native] {
        let i = 0
        while [loop_bound(2)] i < 2 { i += 1 }
        for [loop_bound(2)] j in range(0, 2) { i += j }
        return _lb_n + _soma_tmp_0
    }
}
cell test T { rules {
    assert rs(1, 8) == 4
    assert ls(1, 8) == 16
    assert bt(1, 2) == 1
    assert bs(1, 8) == 10
    assert bc(1, 8) == 8
    assert pm(3, 2) == 4
    assert rs(1, 100000000000000000000) == 50000000000000000000
    assert ls(1, 100000000000000000000) == 200000000000000000000
    assert bs_big(3, 100000000000000000000) == 100000000000000000008
    assert_fails bs_big(16777216, 0) matching "range"
    assert lo(100000000000000000000, 200000000000000000000) == 100000000000000000000
    assert mask(1, 100000000000000000000) == 100000000000000000000
    assert bounded(40, 2) == 42
    assert bounded(100000000000000000000, 2) == 100000000000000000002
} }
"#,
    );
}

#[test]
fn integer_ratios_and_statistics_do_not_round_operands_first() {
    passes(
        "exact_ratios",
        r#"
cell P {
    on ratio() { let a = ipow(10, 400) return (a + 1) / a }
    on near_variance() { let a = ipow(10, 400) return variance([a, a + 1]) }
}
cell test T { rules {
    assert ratio() == 1.0
    assert type_of(ratio()) == "Float"
    assert 1 / shl(1, 1074) == 5e-324
    assert -1 / shl(1, 1074) == -5e-324
    assert 1 / shl(1, 1075) == 0.0
    assert 3 / shl(1, 1075) == 1e-323
    assert 9007199254740993 / 2 == 4503599627370496.0
    assert variance([9007199254740992, 9007199254740993]) == 0.5
    assert pvariance([9007199254740992, 9007199254740993]) == 0.25
    assert pstdev([9007199254740992, 9007199254740993]) == 0.5
    assert near_variance() == 0.5
    assert variance([10000000000000000.0, 10000000000000002.0]) == 2.0
    assert variance([9007199254740993, 9007199254740992.0]) == 0.5
    assert variance([1e308, 1e308]) == 0.0
    assert median([1e308, 1e308]) == 1e308
    assert median([5e-324, 5e-324]) == 5e-324
    assert variance([1, 2, 3]) == 1.0
    assert pvariance([1, 1, 1]) == 0.0
    assert_fails variance([1]) matching "empty"
} }
"#,
    );
}

/// `(f)(2)` and `(x => x + 1)(2)` parsed as two statements: `return` gave
/// back the lambda and `(2)` was an unreachable value. They are refused like
/// `f(a)(b)`; parentheses that merely group still parse.
#[test]
fn calling_a_parenthesized_expression_is_refused_like_a_call_result() {
    let dir = scratch("paren_call");
    for (name, body) in [
        ("lambda", "return (x => x + 1)(2)"),
        ("ident", "let f = x => x + 1\n        return (f)(2)"),
    ] {
        let src = format!("cell P {{\n    on run() {{\n        {body}\n    }}\n}}\n");
        std::fs::write(dir.join("app.cell"), src).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "{name}: {out}");
        assert!(out.contains("parenthesized expression directly"), "{name}: {out}");
        assert!(out.contains("bind it first"), "{name}: {out}");
    }
    passes(
        "paren_ok",
        r#"
cell P {
    on go() {
        let f = x => x + 1
        let g = (f)
        let a = (1)
        return (g(2)) + (f(1)) + (a) * (2)
    }
}
cell test T {
    rules {
        assert P.go() == 7
    }
}
"#,
    );
}

/// `"{0x1F}"`, `"{1e3}"` and `"{1_000}"` passed `soma check` and raised
/// "undefined variable" at run time: the interpolation fast path took any
/// alphanumeric segment for a variable name. `{42}` stays literal text (a
/// regex quantifier), as the checker mirrors.
#[test]
fn interpolated_numeric_literals_evaluate_like_code() {
    passes(
        "interp_numeric_literals",
        r#"
cell P {
    on go(x: Int) {
        return "{0x1F} {1e3} {1_000} {0b11} {42} {true} {x} {1.5}"
    }
}
cell test T {
    rules {
        assert P.go(2) == "31 1000.0 1000 3 {42} true 2 1.5"
    }
}
"#,
    );
}

/// An Int past 64 bits as a List-slot index was element 0: `rows.delete(2^63)`
/// removed the first row, `rows.set(2^63, v)` overwrote it, `rows.get(2^63)`
/// read it (the `rows[i]` form refused it). It is out of bounds like any
/// other index past the end; a local list's `.get` answers `()`.
#[test]
fn bigint_list_indices_are_out_of_bounds_not_element_zero() {
    passes(
        "bigint_list_index",
        r#"
cell A {
    memory { rows: List<Int> [persistent] }
    on seed() { rows.push(10)
        rows.push(20)
        return rows.values }
    on del_big() { return [rows.delete(9223372036854775808), rows.values] }
    on set_big() { let r = try { rows.set(9223372036854775808, 99) }
        return [r.kind, rows.values] }
    on get_big() { return rows.get(9223372036854775808) }
    on local_get() { let xs = [10, 20]
        return xs.get(9223372036854775808) }
}
cell test T {
    rules {
        assert A.seed() == [10, 20]
        assert A.del_big() == [false, [10, 20]]
        assert A.set_big() == ["index", [10, 20]]
        assert A.get_big() == ()
        assert A.local_get() == ()
    }
}
"#,
    );
}

/// A Map key that serde_json gives a meaning to (`$serde_json::private::Number`
/// under `arbitrary_precision`) read back as an Int, or turned the whole Map
/// into a String, after a round trip through SQLite. It is escaped on disk.
#[test]
fn map_keys_serde_reserves_survive_storage() {
    passes(
        "serde_private_key",
        r#"
cell K {
    memory { prefs: Map<String, Map> [persistent] }
    on put(field: String, v: String) { prefs.set("u", map(field, v))
        return prefs.get("u") }
    on read() { return prefs.get("u") }
}
cell test T {
    rules {
        assert K.put("$serde_json::private::Number", "42") == map("$serde_json::private::Number", "42")
        assert K.read() == map("$serde_json::private::Number", "42")
        assert K.put("$serde_json::private::Number", "abc") == map("$serde_json::private::Number", "abc")
        assert type_of(K.read()) == "Map"
        assert K.put("__key__x", "1") == map("__key__x", "1")
        assert K.read() == map("__key__x", "1")
        assert K.put("__map__", "1") == map("__map__", "1")
    }
}
"#,
    );
}

/// `set_budget(-10^21)` (an Int past 64 bits) escaped the negative check and
/// became an unlimited budget; a huge positive one saturates.
#[test]
fn set_budget_refuses_a_negative_bigint() {
    passes(
        "budget_bigint",
        r#"
cell agent B {
    on neg() { let r = try { set_budget(-1000000000000000000000) }
        return r.kind }
    on huge() { set_budget(99999999999999999999999)
        return tokens_remaining() > 0 }
}
cell test T {
    rules {
        assert B.neg() == "range"
        assert B.huge() == true
    }
}
"#,
    );
}

/// `inner_join` / `left_join` scanned the right list once per left row: a
/// 20 000-row join took 8 s. The right side is indexed once; the first right
/// row with an equal key (as text) still wins, and rows without the key are
/// kept by left_join and dropped by inner_join.
#[test]
fn joins_index_the_right_side_and_keep_first_match_semantics() {
    passes(
        "join_index",
        r#"
cell J {
    on rows(n: Int) {
        let out = []
        for i in range(0, n) { out = push(out, map("id", i, "v", i * 3 % 101)) }
        return out
    }
    on big(n: Int) {
        let a = rows(n)
        let b = rows(n) |> map(r => map("id", r.id, "w", 1))
        return [len(inner_join(a, b, "id")), len(left_join(a, b, "id"))]
    }
    on small() {
        let a = [map("k", 1, "a", 1), map("k", 2, "a", 2), map("k", 3, "a", 3), map("x", 1)]
        let b = [map("k", 2, "b", "first"), map("k", 2, "b", "second"), map("k", 1, "b", "one"), map("k", "1", "b", "str")]
        return [inner_join(a, b, "k"), left_join(a, b, "k"), join(a, b, "k")]
    }
}
cell test T {
    rules {
        assert J.big(30000) == [30000, 30000]
        let s = J.small()
        assert s[0] == [map("k", 1, "a", 1, "b", "one"), map("k", 2, "a", 2, "b", "first")]
        assert s[1] == [map("k", 1, "a", 1, "b", "one"), map("k", 2, "a", 2, "b", "first"), map("k", 3, "a", 3), map("x", 1)]
        assert s[2] == s[0]
    }
}
"#,
    );
}

/// `m = with(m, k, v)` and `m = without(m, k)` on a local map copied the whole
/// map at every step (20 000 removals took 26 s; the pipe form `m = m |> with(…)`
/// was already in place). They update in place; aliases made before the
/// assignment keep the old value.
#[test]
fn self_assigned_with_and_without_update_a_local_map_in_place() {
    passes(
        "with_without_in_place",
        r#"
cell W {
    on many(n: Int) {
        let m = map()
        for i in range(0, n) { m = with(m, "k" + to_string(i), i) }
        let full = len(m)
        for i in range(0, n) { m = without(m, "k" + to_string(i)) }
        return [full, len(m)]
    }
    on alias() {
        let m = map("a", 1, "b", 2, "c", 3)
        let keep = m
        m = without(m, "b", "zz")
        m = with(m, "d", 4, 1, "int-key")
        let f = m
        f = with(f, "a", len(f))
        return [m, keep, f, without(m, "c")]
    }
}
cell test T {
    rules {
        assert W.many(30000) == [30000, 0]
        let a = W.alias()
        assert a[0] == map("a", 1, "c", 3, "d", 4, "1", "int-key")
        assert a[1] == map("a", 1, "b", 2, "c", 3)
        assert a[2] == map("a", 4, "c", 3, "d", 4, "1", "int-key")
        assert a[3] == map("a", 1, "d", 4, "1", "int-key")
    }
}
"#,
    );
}

/// `rows[i] = v` on a List slot read the whole log three times and rewrote
/// it (23 ms per write on 20 000 rows). It updates one row; a failing `try`
/// or handler puts the one old element back; gaps after a delete, negative
/// indices, invariants, immutable slots and bounds behave as before.
#[test]
fn list_slot_element_writes_update_one_row() {
    passes(
        "list_slot_set",
        r#"
cell L {
    memory {
        rows: List<Int> [persistent]
        fixed: List<Int> [persistent, immutable]
        invariant rows >= 0
    }
    on seed() { rows.push(1)
        rows.push(2)
        rows.push(3)
        fixed.push(9)
        return rows.values }
    on set_ok() { rows[1] = 20
        rows[-1] = 30
        return rows.values }
    on set_try() { let r = try { rows[0] = 100
            fail("abort") }
        return [r.kind, rows.values] }
    on set_then_fail() { rows[0] = 111
        fail("boom") }
    on set_neg() { let r = try { rows[1] = -5 }
        return [r.kind, rows.values] }
    on set_imm() { let r = try { fixed[0] = 1 }
        return [r.kind, fixed.values] }
    on set_oob() { let r = try { rows[3] = 1 }
        return [r.kind, rows.values] }
    on after_delete() { rows.delete(0)
        rows[0] = 22
        rows.push(4)
        rows[2] = 44
        return rows.values }
    on read() { return [rows.values, rows.len, rows[0], rows.get(-1)] }
}
cell test T {
    rules {
        assert L.seed() == [1, 2, 3]
        assert L.set_ok() == [1, 20, 30]
        assert L.set_try() == ["abort", [1, 20, 30]]
        assert_fails L.set_then_fail()
        assert L.read() == [[1, 20, 30], 3, 1, 30]
        assert L.set_neg() == ["invariant", [1, 20, 30]]
        assert L.set_imm() == ["invariant", [9]]
        assert L.set_oob() == ["index", [1, 20, 30]]
        assert L.after_delete() == [22, 30, 44]
        assert L.read() == [[22, 30, 44], 3, 22, 44]
    }
}
"#,
    );
}

/// `rows.delete(i)` on a List slot rewrote the whole log (23 ms per delete on
/// 20 000 rows). It deletes one row by id; a failing `try` or handler puts
/// the element back at its place. Negative and out-of-range indices,
/// key-shift and size invariants behave as before.
#[test]
fn list_slot_element_deletes_remove_one_row() {
    passes(
        "list_slot_delete",
        r#"
cell D {
    memory { rows: List<Int> [persistent] }
    on seed() { for i in [10, 20, 30, 40, 50] { rows.push(i) }
        return rows.values }
    on del_mid() { return [rows.delete(2), rows.values, rows[2], rows.len] }
    on del_try() { let r = try { rows.delete(1)
            rows.delete(0)
            fail("abort") }
        return [r.kind, rows.values, rows[1]] }
    on del_then_fail() { rows.delete(0)
        rows.delete(0)
        fail("boom") }
    on del_neg_oob() { return [rows.delete(-1), rows.values, rows.delete(99), rows.delete(-99), rows.values] }
    on del_then_push() { rows.delete(0)
        rows.push(60)
        rows[0] = 11
        return [rows.values, rows[0], rows.get(-1), rows.len] }
    on read() { return [rows.values, rows.len] }
}
cell K {
    memory { keyed: List<Int> [persistent]
        invariant key != 0 || keyed == 0 }
    on seed() { keyed.push(0)
        keyed.push(5)
        return keyed.values }
    on del0() { let r = try { keyed.delete(0) }
        return [r.kind, keyed.values] }
    on del1() { let r = try { keyed.delete(1) }
        return [r.value, keyed.values] }
}
cell Z {
    memory { sized: List<Int> [persistent]
        invariant size >= 1 }
    on seed() { sized.push(1)
        return sized.values }
    on del0() { let r = try { sized.delete(0) }
        return [r.kind, sized.values] }
}
cell test T {
    rules {
        assert D.seed() == [10, 20, 30, 40, 50]
        assert D.del_mid() == [true, [10, 20, 40, 50], 40, 4]
        assert D.del_try() == ["abort", [10, 20, 40, 50], 20]
        assert_fails D.del_then_fail()
        assert D.read() == [[10, 20, 40, 50], 4]
        assert D.del_neg_oob() == [true, [10, 20, 40], false, false, [10, 20, 40]]
        assert D.del_then_push() == [[11, 40, 60], 11, 60, 3]
        assert K.seed() == [0, 5]
        assert K.del0() == ["invariant", [0, 5]]
        assert K.del1() == [true, [0]]
        assert Z.seed() == [1]
        assert Z.del0() == ["invariant", [1]]
    }
}
"#,
    );
}

/// `subscribe(url)` ended for good when the publisher closed the socket: the
/// reader thread logged the error and stopped. It reconnects with a backoff;
/// events published after the publisher's restart reach the subscriber.
#[test]
fn subscribe_reconnects_after_the_publisher_restarts() {
    use std::net::TcpListener;
    // a free HTTP/WebSocket/bus port triple at or after `from` (the listeners
    // are dropped again, so two calls need distinct starting points)
    fn triple(from: u16) -> u16 {
        for p in (from..15000).step_by(4) {
            if (0..3).all(|i| TcpListener::bind(("127.0.0.1", p + i)).is_ok()) { return p; }
        }
        panic!("no free port triple");
    }
    let root = scratch("subscribe_reconnect");
    let (pub_dir, sub_dir) = (root.join("pub"), root.join("sub"));
    std::fs::create_dir_all(&pub_dir).unwrap();
    std::fs::create_dir_all(&sub_dir).unwrap();
    let pub_port = triple(14000);
    let sub_port = triple(pub_port + 4);
    // the WebSocket port (HTTP port + 1) opens only with an `on ws` handler
    std::fs::write(pub_dir.join("app.cell"), "cell Pub {\n    on ws(msg: String) { return msg }\n    on beat(n: Int) { publish(\"tick\", map(\"n\", n))\n        return n }\n}\n").unwrap();
    std::fs::write(sub_dir.join("app.cell"), format!(
        "cell Sub {{\n    memory {{ got: Map<String, Int> [persistent] }}\n    on start() {{ subscribe(\"ws://127.0.0.1:{}\") }}\n    on tick(data: Map) {{ got.set(\"n\" + to_string(data.n), data.n) }}\n    on state() {{ return sort(got.keys) }}\n}}\n", pub_port + 1)).unwrap();
    std::fs::write(sub_dir.join("soma.toml"), "[bus]\naccept = [\"tick\"]\n").unwrap();
    let spawn = |dir: &Path, port: u16| {
        let log = std::fs::File::create(dir.join("serve.log")).unwrap();
        Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["serve", "app.cell", "-p", &port.to_string(), "--no-schedule"])
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap()
    };
    let ready = |port: u16| {
        for _ in 0..100 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { return; }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("server on {port} did not start");
    };
    let post = |port: u16, path: &str| {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(format!("POST {path} HTTP/1.0\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n").as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    };
    let mut publisher = spawn(&pub_dir, pub_port);
    ready(pub_port);
    ready(pub_port + 1); // the WebSocket port the subscriber connects to
    let mut subscriber = spawn(&sub_dir, sub_port);
    ready(sub_port);
    std::thread::sleep(std::time::Duration::from_millis(800));
    post(pub_port, "/beat/1");
    let wait_for = |want: &str| {
        for _ in 0..60 {
            if get(sub_port, "/state").contains(want) { return; }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        panic!("subscriber never saw {want}: {}\n--- subscriber log\n{}\n--- publisher log\n{}", get(sub_port, "/state"),
            std::fs::read_to_string(sub_dir.join("serve.log")).unwrap_or_default(),
            std::fs::read_to_string(pub_dir.join("serve.log")).unwrap_or_default());
    };
    wait_for("\"n1\"");
    // the publisher goes away and comes back: the subscription must follow
    let _ = publisher.kill();
    let _ = publisher.wait();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let mut publisher = spawn(&pub_dir, pub_port);
    ready(pub_port);
    ready(pub_port + 1);
    std::thread::sleep(std::time::Duration::from_millis(2500));
    post(pub_port, "/beat/2");
    wait_for("\"n2\"");
    let _ = publisher.kill();
    let _ = subscriber.kill();
    let _ = publisher.wait();
    let _ = subscriber.wait();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn strict_ignores_a_shared_manifest_scoped_to_another_entry() {
    // examples/corpus/web/soma.toml targets todo_state.cell; `verify
    // url_shortener.cell --strict` in that directory failed with "[verify]
    // cells names no state machine of this file" for a gate meant for the
    // other program
    let dir = scratch("strict_entry");
    let machine = r#"
cell Todo {
    memory { n: Map<String, Int> }
    state todo { initial: open  open -> done }
    on finish(id: String) { transition(id, "open", "done") return get_status(id) }
}
"#;
    std::fs::write(dir.join("todo.cell"), machine).unwrap();
    std::fs::write(dir.join("other.cell"), r#"
cell Counter {
    memory { n: Map<String, Int> }
    state c { initial: idle  idle -> busy  busy -> idle }
    on go(id: String) { transition(id, "idle", "busy") return get_status(id) }
    on stop(id: String) { transition(id, "busy", "idle") return get_status(id) }
}
"#).unwrap();
    std::fs::write(dir.join("soma.toml"), r#"
[package]
name = "shared"
version = "0.1.0"
entry = "todo.cell"

[verify]
cells = ["Todo"]
eventually = ["done"]
"#).unwrap();
    let (code, out) = soma(&dir, &["verify", "other.cell", "--strict"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("names no state machine"), "{out}");
    assert!(out.contains("its properties are not checked here"), "{out}");
    // the entry itself still carries the gate
    let (code, out) = soma(&dir, &["verify", "todo.cell", "--strict"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("eventually"), "{out}");
    // an entry that does not exist: the manifest is about THIS directory's
    // program, and a cell it names but the file lacks is still refused
    std::fs::write(dir.join("soma.toml"), r#"
[package]
name = "shared"
version = "0.1.0"

[verify]
cells = ["Todo"]
eventually = ["done"]
"#).unwrap();
    let (code, out) = soma(&dir, &["verify", "other.cell", "--strict"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("names no state machine"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_assertions_report_a_refused_comparison_as_an_error() {
    // `assert 5 != "5"` read as `false` (the interpreter's "cannot compare"
    // was swallowed): the report showed two identical-looking sides, and
    // `assert_fails` on such a comparison passed for the wrong reason
    let dir = scratch("assert_cmp");
    std::fs::write(dir.join("app.cell"), r#"
cell C {
    on lst() { return [1, 2] }
}
cell test T {
    rules {
        assert 5 != "5"
        assert C.lst() != "x"
        assert C.lst() != [3]
        assert C.lst() == [1, 2]
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("ERROR: cannot compare Int and String — left: Int 5, right: String 5"), "{out}");
    assert!(out.contains("ERROR: cannot compare List and String"), "{out}");
    assert!(out.contains("4 tests: 2 passed, 2 failed"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn identity_is_eye_and_refuses_bad_sizes() {
    // identity(n) had its own code: a negative or Float size read as 0 and
    // there was no cell limit (identity(100000) tried to build 10^10 cells)
    let dir = scratch("identity");
    std::fs::write(dir.join("app.cell"), r#"
cell M {
    on same() { return identity(3) == eye(3) }
    on neg() { return identity(-1) }
    on huge() { return identity(100000) }
    on flt() { return identity(2.5) }
    on reshape_neg() { return reshape([1, 2], -1, 2) }
}
cell test T {
    rules {
        assert M.same() == true
        assert_fails M.neg() matching "non-negative"
        assert_fails M.huge() matching "past the limit"
        assert_fails M.flt() matching "expected integer"
        assert_fails M.reshape_neg() matching "non-negative"
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("5 tests: 5 passed"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn map_pattern_needs_the_key_and_float_string_add_is_refused_plainly() {
    // `{kind} -> …` bound kind = () for a map WITHOUT the key: the next arm
    // was unreachable and the body failed later with "cannot add String and
    // Unit". `"a" + 1.5` said "expected Float, got String" while `"a" + 1`
    // said "cannot add String and Int".
    let dir = scratch("map_pattern");
    std::fs::write(dir.join("app.cell"), r#"
cell S {
    on m(v: Map) {
        return match v {
            {kind: "a", n} -> "a:" + to_string(n)
            {kind} -> "other:" + kind
            _ -> "none"
        }
    }
    on present_unit() { return match map("kind", ()) { {kind} -> "present" _ -> "absent" } }
    on f() { return "a" + 1.5 }
}
cell test T {
    rules {
        assert S.m(map("kind", "a", "n", 3)) == "a:3"
        assert S.m(map("kind", "z")) == "other:z"
        assert S.m(map("x", 1)) == "none"
        assert S.m(map()) == "none"
        assert S.present_unit() == "present"
        assert_fails S.f() matching "cannot add String and Float"
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("6 tests: 6 passed"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn generic_key_invariant_typed_for_the_wrong_slot_kind_is_refused() {
    // `invariant key != ""` names no slot, so it guards every slot of the
    // section — and on a List slot `key` is the index: every push failed at
    // run time with "cannot compare Int and String"
    let dir = scratch("generic_key");
    let mixed = r#"
cell A {
    memory {
        names: Map<String, String>
        xs: List<Int>
        invariant key != ""
    }
    on add(v: Int) { xs.push(v) return len(xs) }
}
"#;
    std::fs::write(dir.join("app.cell"), mixed).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("on a List slot (xs) `key` is the index"), "{out}");
    // the Int form beside a Map slot
    std::fs::write(dir.join("app.cell"), mixed.replace(r#"key != """#, "key >= 0")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("on a Map slot (names) `key` is a String"), "{out}");
    // each kind in its own section: fine, and the rule holds at run time
    std::fs::write(dir.join("app.cell"), r#"
cell A {
    memory {
        names: Map<String, String>
        invariant key != ""
    }
    memory {
        xs: List<Int>
        invariant key >= 0
    }
    on add(v: Int) { xs.push(v) return len(xs) }
    on name(k: String, v: String) { names.set(k, v) return v }
}
cell test T {
    rules {
        assert A.add(1) == 1
        assert A.name("a", "b") == "b"
        assert_fails A.name("", "b") matching "invariant"
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("3 tests: 3 passed"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn describe_lists_the_routes_the_server_matches() {
    // describe found routes by grepping `path ==` in ./app.cell — the file of
    // the current directory, whatever file was described — and missed every
    // `match path { "/x" -> … }` arm
    let dir = scratch("describe_routes");
    std::fs::write(dir.join("svc.cell"), r#"
cell Svc {
    memory { items: Map<String, Int> }
    on add(id: String, n: Int) { items.set(id, n) return n }
    on request(method: String, path: String, body: Map) {
        if path == "/health" { return map("ok", true) }
        return match path {
            "/stats" -> map("n", len(items))
            "/items/" + id -> items.get(id)
            _ -> response(404, map("error", "no"))
        }
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["describe", "svc.cell"]);
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let cell = &v["cells"][0];
    assert_eq!(cell["web"], serde_json::json!(true));
    let routes: Vec<&str> = cell["routes"].as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
    assert!(routes.contains(&"/health") && routes.contains(&"/stats") && routes.contains(&"/items/*"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn run_without_a_handler_name_never_guesses_among_several() {
    // `soma run app.cell` ran the first zero-arg public handler (a `reset`);
    // `soma run app.cell 5` ran `close("5")` — declared before echo(s) and
    // num(x) — and committed a transition. The guess must be unambiguous.
    let dir = scratch("run_default");
    std::fs::write(dir.join("svc.cell"), r#"
cell Svc {
    memory { log: List<String> }
    on reset() { log.push("reset") return len(log) }
    on close(id: String) { log.push("close " + id) return id }
    on echo(s: String) { return s }
    on show() { return log }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["run", "svc.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("several public handlers and no `main`/`run`"), "{out}");
    let (code, out) = soma(&dir, &["run", "svc.cell", "5"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("1 argument(s) fit 2 handlers") && out.contains("[close, echo]"), "{out}");
    // nothing ran
    let (_, out) = soma(&dir, &["run", "svc.cell", "show"]);
    assert_eq!(out.trim(), "[]", "{out}");
    // still convenient: the only handler taking these arguments, main/run, a lone handler
    std::fs::write(dir.join("fact.cell"), "cell F { on compute(n: Int) { return n * 2 }  on run() { return compute(20) } }\n").unwrap();
    let (_, out) = soma(&dir, &["run", "fact.cell", "5"]);
    assert_eq!(out.trim(), "10", "{out}");
    let (_, out) = soma(&dir, &["run", "fact.cell"]);
    assert_eq!(out.trim(), "40", "{out}");
    std::fs::write(dir.join("one.cell"), "cell O { on only() { return 7 } }\n").unwrap();
    let (_, out) = soma(&dir, &["run", "one.cell"]);
    assert_eq!(out.trim(), "7", "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn use_lib_inside_a_lib_file_resolves_from_the_project_root() {
    // lib/scoring.cell saying `use lib::helpers` looked for lib/lib/helpers
    // (relative to the importing file): nested imports never resolved, and
    // `soma check lib/scoring.cell` on its own failed too
    let dir = scratch("use_lib_nested");
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/helpers.cell"), "cell Helpers { on double(n: Int) { return n * 2 } }\n").unwrap();
    std::fs::write(dir.join("lib/scoring.cell"), "use lib::helpers\ncell Scoring { on score(n: Int) { return double(n) + 1 } }\n").unwrap();
    std::fs::write(dir.join("app.cell"), r#"
use lib::scoring
cell App { on run(n: Int) { return score(n) } }
cell test T { rules { assert App.run(3) == 7 } }
"#).unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("1 tests: 1 passed"), "{out}");
    // a lib file checked on its own finds the project through its soma.toml
    std::fs::write(dir.join("soma.toml"), "[package]\nname = \"p\"\nversion = \"0.1.0\"\nentry = \"app.cell\"\n").unwrap();
    let (code, out) = soma(&dir.join("lib"), &["check", "scoring.cell"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = soma(&dir, &["check", "lib/scoring.cell"]);
    assert_eq!(code, 0, "{out}");
    // a missing module is still the plain error
    std::fs::write(dir.join("bad.cell"), "use lib::nothere\ncell B { on go() { return 1 } }\n").unwrap();
    let (code, out) = soma(&dir, &["check", "bad.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("cannot import"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn is_a_recognizes_variants_as_the_gotchas_now_say() {
    // AGENT_GOTCHAS §8 claimed `is_a(Box { w: 3 }, "Box")` is false; it is
    // true, by variant name and by sum-type name, and a struct variant's
    // field reads with `.` (what the rewritten gotcha shows)
    let dir = scratch("is_a_variants");
    std::fs::write(dir.join("app.cell"), r#"
cell type Shape { variants { Box { w: Int } Dot } }
cell G {
    on t() { let b = Box { w: 3 } return [is_a(b, "Box"), is_a(b, "Shape"), is_a(Dot, "Shape"), is_a(b, "Dot"), type_of(b), b.w] }
    on k(s: Map) { return match s { Box { w } -> "Box of {w}"  Dot -> "Dot" } }
}
cell test T {
    rules {
        assert G.t() == [true, true, true, false, "Variant", 3]
        assert G.k(Box { w: 3 }) == "Box of 3"
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("2 tests: 2 passed"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn count_is_a_field_on_values_and_length_counts_on_slots() {
    // `.count` on a local Map read the field (a JSON record's count) while
    // check warned "on a Map `.count` is the number of entries (never ())";
    // `.length` counted on a value but read as a missing field on a slot
    let dir = scratch("count_length");
    std::fs::write(dir.join("app.cell"), r#"
cell P {
    memory { m: Map<String, Int>  xs: List<Int> }
    on rec() { let r = from_json("{\"count\": 7}") let e = map("a", 1) return [r.count ?? 0, e.count ?? 0, e.length, e.len] }
    on slots() { m.set("count", 5) m.set("b", 1) xs.push(1) return [m.count, m.length, m.len, xs.count, xs.length, xs.len] }
}
cell test T {
    rules {
        assert P.rec() == [7, 0, 1, 1]
        assert P.slots() == [2, 2, 2, 1, 1, 1]
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("number of entries"), "{out}");
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("2 tests: 2 passed"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn slot_aliases_do_not_swallow_a_predicate() {
    // `rows.all(r => …)` on a List slot returned the slot's CONTENT (a
    // truthy list), so `if rows.all(p)` was always taken and a `require`
    // on it never refused; `rows.count(p)` returned the entry count
    let dir = scratch("slot_alias");
    std::fs::write(dir.join("app.cell"), r#"
cell P {
    memory { rows: List<Int> [persistent]  m: Map<String, Int> [persistent] }
    on seed() { rows.push(20) rows.push(90) m.set("a", 1) m.set("b", 2) return len(rows) }
    on preds() { return [rows.all(x => x > 100), rows.any(x => x > 100), rows.count(x => x > 100), rows.all(x => x > 1), rows.count(x => x > 1)] }
    on bare() { return [rows.all, rows.len, rows.count, rows.first, rows.last, m.all, m.count, m.len] }
    on local() { let l = [20, 90] return [l.all(x => x > 100), l.any(x => x > 100), l.count(x => x > 100), l.all(x => x > 1), l.count(x => x > 1)] }
}
cell test T {
    rules {
        assert P.seed() == 2
        assert P.preds() == [false, false, 0, true, 2]
        assert P.preds() == P.local()
        assert P.bare() == [[20, 90], 2, 2, 20, 90, [1, 2], 2, 2]
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("4 tests: 4 passed"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn every_and_after_bodies_are_checked_for_exhaustive_match() {
    // a non-exhaustive match inside `every` / `after` passed `soma check`
    // (only `on` bodies were walked) and failed EVERY tick at run time,
    // rolled back, visible only in the server log
    let dir = scratch("exhaustive_tick");
    let head = "cell type Shape { variants { Box { w: Int } Dot Pair(Int, Int) } }\ncell Y {\n    memory { n: Map<String, Int> }\n";
    for block in ["    every 10s {", "    after 10s {"] {
        std::fs::write(dir.join("app.cell"), format!(
            "{head}{block} let v = Pair(1, 2) let r = match v {{ Box {{ w }} -> w  Dot -> 0 }} n.set(\"e\", r) }}\n}}\n")).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "{out}");
        assert!(out.contains("non-exhaustive match on 'Shape': missing variant `Pair`"), "{out}");
    }
    // an exhaustive tick still passes
    std::fs::write(dir.join("app.cell"), format!(
        "{head}    every 10s {{ let v = Dot let r = match v {{ Box {{ w }} -> w  Dot -> 0  Pair(a, b) -> a }} n.set(\"e\", r) }}\n}}\n")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unknown_structural_promise_is_refused() {
    // `promise all_persistemt` passed with "All checks passed" on a cell
    // whose slots were not persistent: an unknown predicate was silently
    // true, so a typo turned a checked guarantee into a no-op
    let dir = scratch("promise_typo");
    let cell = |promise: &str, prop: &str| format!(
        "cell A {{\n    face {{\n        signal go() -> Int\n        promise {promise}\n    }}\n    memory {{ n: Map<String, Int>{prop} }}\n    on go() {{ return 1 }}\n}}\n");
    std::fs::write(dir.join("app.cell"), cell("all_persistemt", "")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("which nothing checks") && out.contains("did you mean 'all_persistent'?"), "{out}");
    // an invented one, with no near match, is refused too and lists the known ones
    std::fs::write(dir.join("app.cell"), cell("never_loses_data", "")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("The structural promises are: all_persistent"), "{out}");
    // the real predicate still both fails when violated and passes when met
    std::fs::write(dir.join("app.cell"), cell("all_persistent", "")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("promise 'all_persistent' is not satisfied"), "{out}");
    std::fs::write(dir.join("app.cell"), cell("all_persistent", " [persistent]")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    // a quoted promise stays a note
    std::fs::write(dir.join("app.cell"), cell("\"backed up hourly\"", " [persistent]")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_checker_predicate_nothing_implements_is_refused() {
    // `cell checker … rules { check { require has_auht else Tag } }` was
    // silently true for every cell (the project rule enforced nothing), and
    // under `!` it failed every cell instead; `require all_persistent`, a
    // real structural promise, was not implemented here either
    let dir = scratch("checker_pred");
    let prog = |pred: &str, prop: &str| format!(
        "cell checker rule_x {{\n    face {{ promise \"a project rule\" }}\n    rules {{ check {{ require {pred} else Tag }} }}\n}}\ncell A {{\n    face {{ signal go() -> Int }}\n    memory {{ n: Map<String, Int>{prop} }}\n    on go() {{ return 1 }}\n}}\n");
    std::fs::write(dir.join("app.cell"), prog("has_auht", "")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("which nothing implements") && out.contains("did you mean 'has_auth'?"), "{out}");
    // under `!` the unknown predicate is reported once and fires no rule
    std::fs::write(dir.join("app.cell"), prog("!has_memry", "")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert_eq!(out.lines().filter(|l| l.starts_with("error")).count(), 1, "{out}");
    // the whole promise vocabulary now works in a checker, and really checks
    std::fs::write(dir.join("app.cell"), prog("all_persistent", "")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("failed check 'Tag'"), "{out}");
    std::fs::write(dir.join("app.cell"), prog("all_persistent", " [persistent]")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_variant_pattern_of_the_wrong_shape_is_refused() {
    // `check` called these matches exhaustive and the runtime then raised
    // "variant 'Box' not handled" on every value: the arm could never match
    let dir = scratch("variant_shape");
    let head = "cell type Shape { variants { Box { w: Int } Dot Pair(Int, Int) } }\ncell X {\n";
    let bad = [
        ("match v { Nosuch(x) -> x  Box { w } -> w  Dot -> 0  Pair(a, b) -> a }", "which no `cell type` declares as a variant"),
        ("match v { Box { z } -> z  Dot -> 0  Pair(a, b) -> a }", "`z` is not a field of `Box`"),
        ("match v { Box { w } -> w  Dot -> 0  Pair(a) -> a }", "`Pair` declares 2 positional fields"),
        ("match v { Box(w) -> w  Dot -> 0  Pair(a, b) -> a }", "`Box` declares the field `w`"),
        ("match v { Dot(x) -> x  Box { w } -> w  Pair(a, b) -> a }", "`Dot` declares no payload"),
    ];
    for (m, want) in bad {
        std::fs::write(dir.join("app.cell"), format!("{head}    on a(v: Shape) {{ return {m} }}\n}}\n")).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "{m}\n{out}");
        assert!(out.contains(want), "{m}\nwant: {want}\n{out}");
    }
    // the shapes that do fit, including `..`, an or-pattern and a literal
    std::fs::write(dir.join("app.cell"), format!(
        "{head}    on a(v: Shape) {{ return match v {{ Box {{ w }} -> w  Dot -> 0  Pair(a, b) -> a + b }} }}\n\
         \x20   on b(v: Shape) {{ return match v {{ Box {{ .. }} -> 1  Dot -> 0  Pair(a, b) -> a }} }}\n\
         \x20   on c(v: Shape) {{ return match v {{ Box {{ w }} || Dot -> 1  Pair(a, b) -> a }} }}\n\
         \x20   on d(v: Shape) {{ return match v {{ Pair(1, b) -> b  _ -> 0 }} }}\n}}\n")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}
