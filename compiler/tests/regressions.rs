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

/// Wait until the server answers a real HTTP request, not merely until the
/// port accepts a connection: `soma serve` binds long before its accept
/// loop, so a bare `connect()` succeeded while startup was still running and
/// the request that followed could be reset.
fn wait_http(port: u16) {
    // up to 20 s: under a full parallel test run a server can take more
    // than the 5 s this allowed (an intermittent "did not answer")
    for _ in 0..800 {
        if http_probe(port) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("server on {port} did not answer an HTTP request");
}

/// Wait until the port accepts a connection. For a listener that does not
/// speak HTTP (the WebSocket port a subscriber dials), which is all that can
/// be checked.
fn wait_port(port: u16) {
    for _ in 0..200 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("nothing listening on {port}");
}

fn http_probe(port: u16) -> bool {
    let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else { return false };
    if stream.set_read_timeout(Some(std::time::Duration::from_millis(500))).is_err() { return false }
    if stream.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n").is_err() { return false }
    let mut head = [0u8; 5];
    let mut got = 0;
    while got < head.len() {
        match stream.read(&mut head[got..]) {
            Ok(0) | Err(_) => return false,
            Ok(n) => got += n,
        }
    }
    &head == b"HTTP/"
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
    wait_http(port);
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
    let ready = |port: u16| wait_http(port);
    // the WebSocket port does not answer HTTP: an open socket is the signal
    let ready_socket = |port: u16| wait_port(port);
    let post = |port: u16, path: &str| {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(format!("POST {path} HTTP/1.0\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n").as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    };
    let mut publisher = spawn(&pub_dir, pub_port);
    ready(pub_port);
    ready_socket(pub_port + 1); // the WebSocket port the subscriber connects to
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
    ready_socket(pub_port + 1);
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

#[test]
fn comparing_a_state_name_with_a_variant_is_refused() {
    // in a typed machine `transition()` takes the variant but `get_status()`
    // answers the state's NAME, so `get_status(id) == Funded` raised "cannot
    // compare String and Variant" on every run — it can never be true
    let dir = scratch("state_vs_variant");
    let head = "cell type Status { variants { Open  Funded } }\ncell T {\n    state deal: Status { initial: Open  Open -> Funded }\n";
    for body in ["if get_status(id) == Funded { return 1 } return 0",
                 "if Funded != get_status(id) { return 1 } return 0"] {
        std::fs::write(dir.join("app.cell"), format!("{head}    on a(id: String) {{ {body} }}\n}}\n")).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "{body}\n{out}");
        assert!(out.contains("Compare the name: `== \"Funded\"`"), "{body}\n{out}");
    }
    // the string form, a real variant-to-variant test and transition() are fine
    std::fs::write(dir.join("app.cell"), format!(
        "cell type Pay {{ variants {{ Card {{ last4: String }}  Cash }} }}\n{head}\
         \x20   on a(id: String) {{ if get_status(id) == \"Funded\" {{ return 1 }} return 0 }}\n\
         \x20   on b(id: String) {{ transition(id, Open, Funded) return get_status(id) }}\n\
         \x20   on c(p: Pay) {{ return p == Cash }}\n}}\n")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_match_arm_that_can_never_run_is_reported() {
    // a case appended below `_ -> …`, or repeating an earlier pattern, never
    // ran: the match silently kept taking the earlier arm
    let dir = scratch("dead_arm");
    let head = "cell type Shape { variants { Box { w: Int } Dot } }\ncell D {\n";
    let dead = [
        "on a(x: Int) { return match x { 1 -> \"a\"  _ -> \"w\"  2 -> \"b\" } }",
        "on a(x: Int) { return match x { 1 -> \"a\"  1 -> \"b\"  _ -> \"w\" } }",
        "on a(s: String) { return match s { \"a\" -> 1  \"a\" -> 2  _ -> 0 } }",
        "on a(v: Shape) { return match v { Box { w } -> w  Dot -> 0  Box { w } -> 9 } }",
        "on a(x: Int) { return match x { n -> n  5 -> 0 } }",
        "on a(x: Int) { return match x { 1 || 2 -> \"a\"  2 -> \"b\"  _ -> \"w\" } }",
    ];
    for m in dead {
        std::fs::write(dir.join("app.cell"), format!("{head}    {m}\n}}\n")).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_eq!(code, 0, "a dead arm is a warning, not an error\n{m}\n{out}");
        assert!(out.contains("this match arm never runs"), "{m}\n{out}");
    }
    // a guard may fail, so a guarded arm kills nothing; a partly covered
    // or-pattern is still reachable
    std::fs::write(dir.join("app.cell"), format!(
        "{head}    on a(x: Int) {{ return match x {{ n if n > 5 -> 1  n if n > 2 -> 2  _ -> 0 }} }}\n\
         \x20   on b(x: Int) {{ return match x {{ 1 || 2 -> \"a\"  2 || 3 -> \"b\"  _ -> \"w\" }} }}\n\
         \x20   on c(v: Shape) {{ return match v {{ Box {{ w }} -> w  Dot -> 0 }} }}\n}}\n")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("never runs"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn interior_cells_get_the_same_static_checks() {
    // a cell nested in `interior { }` skipped the sum-type checks, the
    // invariant name/`status` validation and the builtin arity check: the
    // same code was refused at the top level and passed inside
    let dir = scratch("interior_checks");
    let head = "cell type Shape { variants { Box { w: Int } Dot Pair(Int, Int) } }\n";
    let cases: [(&str, &str); 5] = [
        ("    on h(v: Shape) { return match v { Box { w } -> w  Dot -> 0 } }", "non-exhaustive match"),
        ("    on h(v: Shape) { return match v { Box { z } -> z  Dot -> 0  Pair(a, b) -> a } }", "is not a field of `Box`"),
        ("    on h() { return len(1, 2, 3) }", "len() takes at most 1 argument"),
        ("    memory { n: Map<String, Int>\n        invariant nosuchname >= 0\n    }\n    on h() { return 1 }", "references unknown name 'nosuchname'"),
        ("    memory { n: Map<String, Int>\n        invariant status != \"x\"\n    }\n    on h() { return 1 }", "declares no `state"),
    ];
    for (body, want) in cases {
        std::fs::write(dir.join("app.cell"), format!("{head}cell T {{\n{body}\n}}\n")).unwrap();
        let (code, top) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "{body}\n{top}");
        assert!(top.contains(want), "at top level\n{body}\n{top}");
        let nested: String = body.lines().map(|l| format!("    {l}\n")).collect();
        std::fs::write(dir.join("app.cell"), format!(
            "{head}cell Outer {{\n    on run() {{ return 1 }}\n    interior {{\n        cell T {{\n{nested}        }}\n    }}\n}}\n")).unwrap();
        let (code, inner) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "inside interior\n{body}\n{inner}");
        assert!(inner.contains(want), "inside interior\n{body}\n{inner}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn verify_proves_interior_cells_too() {
    // `soma verify` walked only top-level cells: a machine, a loop and an
    // invariant inside `interior { }` were never proven, so a STATICALLY
    // violated invariant came out as VERIFY OK
    let dir = scratch("verify_interior");
    let worker = "    memory {\n        n: Map<String, Int>\n        invariant n >= 0\n    }\n\
        \x20   state m { initial: a  a -> b  b -> c  c -> b }\n\
        \x20   on go(id: String) {\n        n.set(\"k\", 0 - 1)\n        let i = 0\n\
        \x20       while i < 10 { i = i + 1 }\n        transition(id, \"a\", \"b\")\n        return get_status(id)\n    }\n";
    std::fs::write(dir.join("flat.cell"), format!("cell Worker {{\n{worker}}}\n")).unwrap();
    let (code, flat) = soma(&dir, &["verify", "flat.cell"]);
    assert_ne!(code, 0, "{flat}");
    let nested: String = worker.lines().map(|l| format!("        {l}\n")).collect();
    std::fs::write(dir.join("app.cell"), format!(
        "cell Outer {{\n    on run() {{ return 1 }}\n    interior {{\n\
         \x20       cell Driver {{\n            face {{ signal go(id: String) }}\n            on go(id: String) {{ return 1 }}\n        }}\n\
         \x20       cell Worker {{\n            face {{ await go(id: String) }}\n{nested}        }}\n    }}\n}}\n")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = soma(&dir, &["verify", "app.cell"]);
    assert_ne!(code, 0, "an interior cell must be proven like any other\n{out}");
    for want in ["invariant n >= 0", "statically violated",
                 "while-loop without provable termination bound",
                 "no terminal states"] {
        assert!(out.contains(want), "missing {want:?} for the interior cell\n{out}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn describe_shows_interior_cells() {
    // `describe --faces` is the contract summary of every cell, yet a cell
    // nested in `interior { }` was invisible: its face, memory and handlers
    // never appeared, in text or in JSON
    let dir = scratch("describe_interior");
    std::fs::write(dir.join("app.cell"), r#"
cell Outer {
    memory { top: Map<String, Int> [persistent] }
    on run() { top.set("t", 1) return 1 }
    interior {
        cell Driver {
            face { signal ping(id: String) }
            on ping(id: String) { return 1 }
        }
        cell Worker {
            face { await ping(id: String) }
            memory { n: Map<String, Int> [persistent] }
            on ping(id: String) { n.set(id, 1) return n.get(id) }
        }
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = soma(&dir, &["describe", "app.cell", "--faces"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("cell Driver   (interior of Outer)"), "{out}");
    assert!(out.contains("cell Worker   (interior of Outer)"), "{out}");
    assert!(out.contains("memory n: Map<String, Int>"), "{out}");
    for args in [vec!["describe", "app.cell"], vec!["describe", "app.cell", "--faces", "--json"]] {
        let (code, out) = soma(&dir, &args);
        assert_eq!(code, 0, "{out}");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let cells = v["cells"].as_array().unwrap();
        let named: Vec<(&str, Option<&str>)> = cells.iter()
            .map(|c| (c["name"].as_str().unwrap(), c["interior_of"].as_str()))
            .collect();
        assert!(named.contains(&("Outer", None)), "{out}");
        assert!(named.contains(&("Driver", Some("Outer"))), "{out}");
        assert!(named.contains(&("Worker", Some("Outer"))), "{out}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn calling_an_interior_cell_by_name_is_refused() {
    // the interpreter never registers a cell declared in `interior { }`, so
    // `Worker.ping(…)` passed `check` and raised "undefined variable:
    // Worker" when the rule or the handler ran
    let dir = scratch("interior_call");
    let interior = "    interior {\n\
        \x20       cell Driver {\n            face { signal ping(id: String) }\n            on ping(id: String) { return 1 }\n        }\n\
        \x20       cell Worker {\n            face { await ping(id: String) }\n            on ping(id: String) { return 2 }\n        }\n    }\n";
    // from a test rule
    std::fs::write(dir.join("app.cell"), format!(
        "cell Outer {{\n    on run() {{ return 1 }}\n{interior}}}\n\
         cell test T {{\n    rules {{\n        assert Worker.ping(\"a\") == 2\n    }}\n}}\n")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("is a cell inside the `interior { }` of 'Outer'"), "{out}");
    // and from a top-level handler
    std::fs::write(dir.join("app.cell"), format!(
        "cell Outer {{\n    on run() {{ return Worker.ping(\"a\") }}\n{interior}}}\n")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("never called by name"), "{out}");
    // the composition itself stays legal
    std::fs::write(dir.join("app.cell"), format!("cell Outer {{\n    on run() {{ return 1 }}\n{interior}}}\n")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn lint_reads_interior_cells() {
    // `soma lint` walked only top-level cells, so the handlers of a cell
    // nested in `interior { }` were never linted
    let dir = scratch("lint_interior");
    let body = "        let s = status.get(\"a\")\n        return s\n";
    std::fs::write(dir.join("flat.cell"), format!(
        "cell A {{\n    memory {{ status: Map<String, String> }}\n    on h() {{\n{body}    }}\n}}\n")).unwrap();
    let (code, flat) = soma(&dir, &["lint", "flat.cell"]);
    assert_eq!(code, 0, "{flat}");
    assert!(flat.contains("unchecked .get()"), "{flat}");
    std::fs::write(dir.join("app.cell"), format!(
        "cell Outer {{\n    on run() {{ return 1 }}\n    interior {{\n\
         \x20       cell A {{\n            face {{ signal h() }}\n            memory {{ status: Map<String, String> }}\n\
         \x20           on h() {{\n{body}            }}\n        }}\n    }}\n}}\n")).unwrap();
    let (code, out) = soma(&dir, &["lint", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("unchecked .get()"), "the interior handler must be linted too\n{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn lint_defaults_the_unchecked_get_to_the_slot_value_type() {
    // the `??` fallback was hardcoded to `map()` whatever the slot held:
    // a `Map<String, String>` was told to fall back on a Map, and applying
    // the suggestion put a Map where a String belonged
    let dir = scratch("lint_get_default");
    std::fs::write(dir.join("app.cell"),
        "cell A {\n    memory {\n        s: Map<String, String>\n        i: Map<String, Int>\n         \x20       f: Map<String, Float>\n        b: Map<String, Bool>\n         \x20       m: Map<String, Map<String, Int>>\n        l: Map<String, List<Int>>\n         \x20       a: Map<String, Any>\n        e: List<String>\n    }\n         \x20   on h(k: String) {\n        let v1 = s.get(k)\n        let v2 = i.get(k)\n         \x20       let v3 = f.get(k)\n        let v4 = b.get(k)\n        let v5 = m.get(k)\n         \x20       let v6 = l.get(k)\n        let v7 = a.get(k)\n        let v8 = e.get(0)\n         \x20       return [v1, v2, v3, v4, v5, v6, v7, v8]\n    }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["lint", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    for want in ["s.get(k) ?? \"\"", "i.get(k) ?? 0", "f.get(k) ?? 0.0",
                 "b.get(k) ?? false", "m.get(k) ?? map()", "l.get(k) ?? []",
                 "e.get(0) ?? \"\""] {
        assert!(out.contains(want), "missing suggestion `{want}`\n{out}");
    }
    // a type with no literal default is not given an invented one
    assert!(out.contains("Any has no literal default"), "{out}");
    assert!(!out.contains("a.get(k) ?? map()"), "{out}");

    // every suggested default is accepted where the `.get()` stood
    std::fs::write(dir.join("fixed.cell"),
        std::fs::read_to_string(dir.join("app.cell")).unwrap()
            .replace("= s.get(k)", "= s.get(k) ?? \"\"")
            .replace("= i.get(k)", "= i.get(k) ?? 0")
            .replace("= f.get(k)", "= f.get(k) ?? 0.0")
            .replace("= b.get(k)", "= b.get(k) ?? false")
            .replace("= m.get(k)", "= m.get(k) ?? map()")
            .replace("= l.get(k)", "= l.get(k) ?? []")
            .replace("= e.get(0)", "= e.get(0) ?? \"\"")).unwrap();
    let (code, out) = soma(&dir, &["check", "fixed.cell"]);
    assert_eq!(code, 0, "the suggested defaults must type-check\n{out}");
    let (_, out) = soma(&dir, &["lint", "fixed.cell"]);
    assert!(!out.contains("s.get(k)"), "the warning must be gone once applied\n{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn lint_names_the_method_that_reaches_an_unrouted_handler() {
    // the note said "reachable as POST /<h>/<args>" for every handler,
    // but `soma serve` answers a GET on one that does not write — and it
    // exposes no endpoint at all for a handler `request` reaches through
    // one of its own helpers, which the note still called reachable
    let dir = scratch("lint_unrouted_method");
    std::fs::write(dir.join("app.cell"),
        "cell A {\n    memory { n: Map<String, Int> }\n         \x20   on request(path: String) {\n        if path == \"/s\" { return submit(1) }\n         \x20       return map(\"ok\", false)\n    }\n         \x20   on submit(v: Int) { return check_it(v) }\n         \x20   on check_it(v: Int) { return v > 0 }\n         \x20   on bump(k: String) {\n        n.set(k, (n.get(k) ?? 0) + 1)\n        return 1\n    }\n         \x20   on peek(k: String) { return n.get(k) ?? 0 }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["lint", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    // a handler that writes is POST-only, a read-only one answers a GET too
    assert!(out.contains("'bump' is not referenced by `request` — it is still reachable as POST /bump/<args>"),
        "a writing handler is POST-only\n{out}");
    assert!(out.contains("'peek' is not referenced by `request` — it is still reachable as GET or POST /peek/<args>"),
        "a read-only handler answers a GET too\n{out}");
    // `request` -> `submit` -> `check_it`: both belong to `request`
    assert!(!out.contains("'submit' is not referenced"), "request calls submit\n{out}");
    assert!(!out.contains("'check_it' is not referenced"),
        "request reaches check_it through submit, so serve exposes no endpoint for it\n{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn lint_does_not_suggest_a_match_that_would_change_the_answer() {
    // `if s == "a" { s = "b" }` then `if s == "b" { … }` runs BOTH, on the
    // rewritten value; a `match s` takes one arm. The note suggested the
    // rewrite anyway, so following it changed what the handler returned
    let dir = scratch("lint_if_chain_rebind");
    std::fs::write(dir.join("rebind.cell"),
        "cell C {\n    on step(s: String) {\n        let cur = s\n        let trace = \"\"\n         \x20       if cur == \"a\" { cur = \"b\"  trace = trace + \"a\" }\n         \x20       if cur == \"b\" { cur = \"c\"  trace = trace + \"b\" }\n         \x20       if cur == \"c\" { cur = \"d\"  trace = trace + \"c\" }\n         \x20       return [cur, trace]\n    }\n}\n").unwrap();
    // the three ifs really do run in sequence
    let (code, out) = soma(&dir, &["run", "rebind.cell", "step", "a"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("\"d\"") && out.contains("\"abc\""),
        "the chain runs on the rewritten value\n{out}");
    let (code, out) = soma(&dir, &["lint", "rebind.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("if-chain"),
        "a branch that rewrites the compared variable rules the match out\n{out}");

    // a chain that leaves the compared variable alone is still reported
    std::fs::write(dir.join("plain.cell"),
        "cell C {\n    on step(s: String) {\n        let out = 0\n         \x20       if s == \"a\" { out = 1 }\n        if s == \"b\" { out = 2 }\n         \x20       if s == \"c\" { out = 3 }\n        return out\n    }\n}\n").unwrap();
    let (_, out) = soma(&dir, &["lint", "plain.cell"]);
    assert!(out.contains("3 branches on 's'"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_name_reached_for_out_of_habit_is_answered_the_same_as_a_call_and_a_method() {
    // `x.str()` named to_string, while `str(x)` fell through to edit
    // distance and answered "did you mean 'shr'?" — a bit shift. Same for
    // `size` (sin), `pop` (pow) and `add` (abs)
    let dir = scratch("habit_call_hint");
    for (name, want) in [("str", "to_string(x)"), ("size", "len(x)"),
                         ("toUpperCase", "uppercase(s)"), ("forEach", "for x in xs")] {
        std::fs::write(dir.join("app.cell"), format!(
            "cell A {{\n    on f(x: Int) {{\n        return {name}(x)\n    }}\n}}\n")).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "{out}");
        assert!(out.contains(&format!("in Soma: {want}")), "`{name}(x)` must name {want}\n{out}");
        assert!(!out.contains("did you mean"), "`{name}(x)` must not guess by edit distance\n{out}");
    }
    // inside a string interpolation too
    std::fs::write(dir.join("interp.cell"),
        "cell A {\n    on f(x: Int) {\n        return \"v={str(x)}\"\n    }\n}\n").unwrap();
    let (_, out) = soma(&dir, &["check", "interp.cell"]);
    assert!(out.contains("in Soma: to_string(x)"), "{out}");
    // a genuine typo still gets the near-miss
    std::fs::write(dir.join("typo.cell"),
        "cell A {\n    on f(x: Int) {\n        return lenn(x)\n    }\n}\n").unwrap();
    let (_, out) = soma(&dir, &["check", "typo.cell"]);
    assert!(out.contains("did you mean 'len'?"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn verify_names_a_rebound_parameter_as_the_reason_the_write_is_not_proven() {
    // the handler already had `require amount >= 0`, and verify still told
    // the author to add one. The require was not the problem: the handler
    // reassigns the parameter before the write, so no require on it can
    // describe what is written
    let dir = scratch("verify_rebind_reason");
    let head = "cell T {\n    memory {\n        bal: Map<String, Int> [persistent]\n                \x20       invariant bal >= 0\n    }\n                \x20   state flow {\n        initial: open\n        open -> closed\n    }\n                \x20   on close(id: String) { transition(id, \"closed\")  return 1 }\n";
    std::fs::write(dir.join("rebind.cell"), format!(
        "{head}    on put(id: String, amount: Int) {{\n         \x20       require amount >= 0 else BadAmount\n         \x20       amount = 0 - 5\n        bal.set(id, amount)\n        return 1\n    }}\n}}\n")).unwrap();
    let (code, out) = soma(&dir, &["check", "rebind.cell"]);
    assert_eq!(code, 0, "{out}");
    let (_, out) = soma(&dir, &["verify", "rebind.cell"]);
    assert!(out.contains("rebinds before the write"), "{out}");
    assert!(!out.contains("narrow it: `require"),
        "the require is already there; repeating the advice sends the author after a fix that cannot work\n{out}");

    // a parameter written straight through still gets the require advice
    std::fs::write(dir.join("plain.cell"), format!(
        "{head}    on put(id: String, amount: Int) {{\n         \x20       bal.set(id, amount)\n        return 1\n    }}\n}}\n")).unwrap();
    let (_, out) = soma(&dir, &["verify", "plain.cell"]);
    assert!(out.contains("narrow it: `require amount >= 0 else"), "{out}");
    assert!(!out.contains("rebinds before the write"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn refinement_does_not_count_a_transition_past_a_return() {
    // `soma check` warns the statement is unreachable, and verify still
    // listed it: the handler was reported as reaching a state that nothing
    // could reach through it — the opposite of what a safety audit needs
    let dir = scratch("refinement_dead_transition");
    std::fs::write(dir.join("app.cell"),
        "cell T {\n    memory { st: Map<String, Int> [persistent] }\n         \x20   state flow {\n        initial: draft\n        draft -> live\n        live -> done\n    }\n         \x20   on early(id: String) {\n        return get_status(id)\n        transition(id, \"live\")\n    }\n         \x20   on guarded(id: String, ok: Bool) {\n         \x20       if ok == false { return map(\"ok\", false) }\n         \x20       transition(id, \"live\")\n        return map(\"ok\", true)\n    }\n         \x20   on finish(id: String) { transition(id, \"done\")  return 1 }\n}\n").unwrap();
    let (_, out) = soma(&dir, &["verify", "app.cell"]);
    assert!(!out.contains("`early` ⟶"),
        "a transition past a return is not an edge the handler takes\n{out}");
    // a return inside an `if` does not end the handler: what follows counts
    assert!(out.contains("`guarded` ⟶ {live}"), "{out}");
    assert!(out.contains("`finish` ⟶ {done}"), "{out}");
    // and the runtime agrees: `early` leaves the instance where it was
    let (code, out) = soma(&dir, &["run", "app.cell", "early", "z"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("draft"), "early must not move the instance\n{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_handler_named_after_a_native_primitive_is_refused_where_it_is_ambiguous() {
    // `buffer`, `hashmap`, `strbuf` … exist only inside a [native] handler,
    // so a handler may carry one of those names. Then the same call text
    // meant two things — the primitive inside [native], the handler outside
    // — and both compiled silently
    let dir = scratch("native_primitive_shadow");
    let body = "    on native_use(n: Int) [native] {\n        let b = buffer(n)\n                \x20       buf_set(b, 0, 7)\n        return buf_get(b, 0)\n    }\n                \x20   on interp_use(n: Int) { return buffer(n) }\n";
    std::fs::write(dir.join("same.cell"), format!(
        "cell A {{\n    on buffer(n: Int) {{ return 0 - 1 }}\n{body}}}\n")).unwrap();
    let (code, out) = soma(&dir, &["check", "same.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("is the primitive, not the handler `buffer`"), "{out}");

    // the handler in another cell is the same ambiguity
    std::fs::write(dir.join("other.cell"), format!(
        "cell Other {{\n    face {{ signal buffer(n: Int) -> Int }}\n         \x20   on buffer(n: Int) {{ return 0 - 1 }}\n}}\ncell A {{\n{body}}}\n")).unwrap();
    let (code, out) = soma(&dir, &["check", "other.cell"]);
    assert_ne!(code, 0, "{out}");

    // a [native] handler using the primitives with no homonym is untouched
    std::fs::write(dir.join("clean.cell"),
        "cell A {\n    on native_use(n: Int) [native] {\n        let b = buffer(n)\n         \x20       buf_set(b, 0, 7)\n        return buf_get(b, 0)\n    }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "clean.cell"]);
    assert_eq!(code, 0, "{out}");

    // and so is a handler named `buffer` in a program with no native handler
    std::fs::write(dir.join("interp.cell"),
        "cell A {\n    on buffer(n: Int) { return n }\n    on call_it(n: Int) { return buffer(n) }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "interp.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_write_guard_follows_the_link_to_what_the_write_lands_on() {
    // the guard read the path as written, so a symlink walked straight past
    // it: `write_file("upload.csv", …)` with upload.csv -> app.cell replaced
    // the program, which the guard exists to prevent
    let dir = scratch("write_guard_symlink");
    std::fs::write(dir.join("app.cell"),
        "cell G {\n    on w(p: String, c: String) {\n         \x20       let r = try { write_file(p, c) }\n         \x20       if r.error != () { return map(\"refused\", true) }\n         \x20       return map(\"refused\", false)\n    }\n}\n").unwrap();
    std::fs::write(dir.join("victim.cell"), "ORIGINAL\n").unwrap();
    let _ = std::fs::remove_file(dir.join("link.csv"));
    #[cfg(unix)]
    std::os::unix::fs::symlink("victim.cell", dir.join("link.csv")).unwrap();

    #[cfg(unix)]
    {
        let (code, out) = soma(&dir, &["run", "app.cell", "w", "link.csv", "PWNED"]);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("\"refused\": true"), "a link to a .cell is a write to that .cell\n{out}");
        assert_eq!(std::fs::read_to_string(dir.join("victim.cell")).unwrap(), "ORIGINAL\n",
            "the program's source must be untouched");
    }
    // a trailing dot names the same file on Windows
    let (_, out) = soma(&dir, &["run", "app.cell", "w", "victim.cell.", "X"]);
    assert!(out.contains("\"refused\": true"), "{out}");
    // ordinary writes still go through, including into a directory that
    // does not exist yet
    for p in ["report.csv", "new/deep/x.csv"] {
        let (code, out) = soma(&dir, &["run", "app.cell", "w", p, "X"]);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("\"refused\": false"), "`{p}` must be written\n{out}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_mistyped_manifest_key_is_refused_in_every_section() {
    // `[verify]` and `[agent]` already refused an unknown key, five other
    // sections dropped it: `seedz` left the node standalone while the
    // operator believed it had joined a cluster, `entri` fell back to
    // main.cell, `pathh` resolved a local dependency through the registry
    let dir = scratch("manifest_typo");
    std::fs::write(dir.join("app.cell"), "cell A {\n    on run() { return 1 }\n}\n").unwrap();
    for (label, toml, field) in [
        ("compute", "[compute]\nthreadz = 4\n", "threadz"),
        ("cluster", "[cluster]\nseedz = [\"a:1\"]\n", "seedz"),
        ("parallel", "[compute.parallel]\nhandlerz = [\"h\"]\n", "handlerz"),
        ("package", "[package]\nentri = \"app.cell\"\n", "entri"),
        ("dependency", "[dependencies]\nlib = { pathh = \"./lib\" }\n", "pathh"),
    ] {
        std::fs::write(dir.join("soma.toml"), toml).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "[{label}] a mistyped key must not be dropped\n{out}");
        if label == "dependency" {
            // an untagged enum names no field, so the valid keys are spelled out
            assert!(out.contains("a dependency is a version string"), "[{label}]\n{out}");
        } else {
            assert!(out.contains(&format!("unknown field `{field}`")), "[{label}]\n{out}");
        }
        // the [verify] key list belongs to a [verify] error, not this one
        assert!(!out.contains("valid [verify] keys"), "[{label}]\n{out}");
    }
    // the spellings these sections actually take still parse
    for toml in [
        "[compute]\nbackend = \"threads\"\nthreads = 4\n",
        "[cluster]\nseeds = [\"a:1\"]\nnode_id = \"n1\"\n",
        "[package]\nname = \"x\"\nentry = \"app.cell\"\n",
        "[dependencies]\nlib = \"1.0\"\n",
    ] {
        std::fs::write(dir.join("soma.toml"), toml).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_eq!(code, 0, "{toml}\n{out}");
    }
    // a [verify] error still gets the key list
    std::fs::write(dir.join("soma.toml"), "[verify]\nbefor = []\n").unwrap();
    let (_, out) = soma(&dir, &["check", "app.cell"]);
    assert!(out.contains("valid [verify] keys"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_quant_builtin_refuses_an_option_it_does_not_read() {
    // http, csv, think and soma.toml all refuse an unknown option; these
    // nine dropped it, so `var_historical(r, map("alfa", 0.99))` quietly
    // computed the default confidence level and answered a different number
    let dir = scratch("quant_unknown_opt");
    let calls = [
        ("var_historical", "var_historical(rr, BAD)"),
        ("var_gaussian", "var_gaussian(rr, BAD)"),
        ("expected_shortfall_historical", "expected_shortfall_historical(rr, BAD)"),
        ("clean_covariance", "clean_covariance(cov, BAD)"),
        ("impact_sqrt", "impact_sqrt(100.0, 1000.0, 0.2, BAD)"),
        ("importance_sample_rows", "importance_sample_rows(A, BAD)"),
        ("svd_lowrank", "svd_lowrank(A, BAD)"),
        ("regress_sgd", "regress_sgd(A, [1.0, 2.0, 3.0, 4.0, 5.0], BAD)"),
        ("to_sampled", "to_sampled(A, BAD)"),
    ];
    let head = "cell Q {\n    on t() {\n        \x20       let A = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]\n        \x20       let rr = [0.01, 0.02, 0 - 0.01, 0.03, 0 - 0.02, 0.005]\n        \x20       let cov = [[1.0, 0.1], [0.1, 1.0]]\n";
    for (name, call) in calls {
        let body = call.replace("BAD", "map(\"alfa\", 0.99)");
        std::fs::write(dir.join("app.cell"), format!("{head}        return {body}\n    }}\n}}\n")).unwrap();
        let (_, out) = soma(&dir, &["run", "app.cell", "t"]);
        assert!(out.contains(&format!("{name}: unknown option 'alfa'")),
            "{name} must refuse an option it does not read\n{out}");
    }
    // every option these builtins do read is still accepted
    let good = "[var_historical(rr, map(\"alpha\", 0.95, \"max_obs\", 64, \"max_assets\", 8)), \
                var_gaussian(rr, map(\"alpha\", 0.95, \"mu\", 0.0, \"sigma\", 0.1, \"max_obs\", 64)), \
                expected_shortfall_historical(rr, map(\"alpha\", 0.95, \"max_obs\", 64)), \
                impact_sqrt(100.0, 1000.0, 0.2, map(\"Y\", 1.0))]";
    std::fs::write(dir.join("app.cell"), format!("{head}        return {good}\n    }}\n}}\n")).unwrap();
    let (code, out) = soma(&dir, &["run", "app.cell", "t"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("unknown option"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_unknown_slot_property_names_the_one_it_is_a_letter_from() {
    // a handler annotation already offered the near miss; a slot property
    // did not, so `[persistant]` read as a property of the author's own and
    // the slot silently kept nothing across a restart
    let dir = scratch("slot_property_typo");
    for (typo, want) in [("persistant", "persistent"), ("consistant", "consistent"),
                         ("ephemerale", "ephemeral"), ("encripted", "encrypted"),
                         ("imutable", "immutable"), ("capacty(10)", "capacity")] {
        std::fs::write(dir.join("app.cell"), format!(
            "cell A {{\n    memory {{ m: Map<String, Int> [{typo}] }}\n    on r() {{ return 1 }}\n}}\n")).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_eq!(code, 0, "an unknown property stays a warning\n{out}");
        assert!(out.contains(&format!("did you mean '{want}'?")), "[{typo}]\n{out}");
    }
    // a property of the author's own is not second-guessed
    std::fs::write(dir.join("app.cell"),
        "cell A {\n    memory { m: Map<String, Int> [my_custom_flag] }\n    on r() { return 1 }\n}\n").unwrap();
    let (_, out) = soma(&dir, &["check", "app.cell"]);
    assert!(out.contains("unknown property 'my_custom_flag'"), "{out}");
    assert!(!out.contains("did you mean"), "{out}");
    // the spelling it meant passes clean
    std::fs::write(dir.join("app.cell"),
        "cell A {\n    memory { m: Map<String, Int> [persistent] }\n    on r() { return 1 }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("unknown property"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_property_rule_naming_nothing_is_reported() {
    // `cell property my_flag { rules { implies [persistant] } }` was
    // accepted in silence, and the note even said the slot implied
    // `persistant` — a name nothing defines, so the slot got no persistence
    let dir = scratch("property_rule_ref");
    let tail = "cell A {\n    memory { m: Map<String, Int> [my_flag] }\n    on r() { return 1 }\n}\n";
    for (rule, name, want) in [
        ("implies", "persistant", Some("persistent")),
        ("requires", "ephemeralle", Some("ephemeral")),
        ("contradicts", "nosuchprop", None),
    ] {
        std::fs::write(dir.join("app.cell"), format!(
            "cell property my_flag {{\n    rules {{ {rule} [{name}] }}\n}}\n{tail}")).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        // `requires` also raises the pre-existing "not present" error, which
        // this warning is what explains
        if rule != "requires" { assert_eq!(code, 0, "it stays a warning\n{out}"); }
        assert!(out.contains(&format!("{rule} '{name}', which no loaded cell property defines")), "{out}");
        match want {
            Some(n) => assert!(out.contains(&format!("did you mean '{n}'?")), "{out}"),
            None => assert!(out.contains("as written the rule never applies"), "{out}"),
        }
    }
    // a name that resolves, even one defined further down the file, is quiet
    std::fs::write(dir.join("app.cell"), format!(
        "cell property my_flag {{\n    rules {{ implies [other_flag] }}\n}}\n         cell property other_flag {{\n    rules {{ implies [persistent] }}\n}}\n{tail}")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("no loaded cell property defines"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_malformed_budget_annotation_is_refused_rather_than_defaulted() {
    // `capacity("big")` and `capacity()` fell back to the DEFAULT capacity
    // and `soma check` still printed "budget proven" — on a number the
    // author never wrote (66 MiB proven where `capacity(10)` proves 24)
    let dir = scratch("budget_annotation");
    let cell = |prop: &str| format!(
        "cell A {{\n    memory {{ m: Map<String, Int> [persistent, {prop}] }}\n         \x20   scale {{\n        replicas: 1\n        memory: \"128Mi\"\n    }}\n         \x20   on r() {{ return 1 }}\n}}\n");
    for prop in ["capacity()", "capacity(\"big\")", "capacity(1.5)",
                 "capacity(10, 20)", "max_value_bytes()"] {
        std::fs::write(dir.join("app.cell"), cell(prop)).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "`{prop}` must not be proven on a default\n{out}");
        assert!(out.contains("the memory proof would silently use the default"), "[{prop}]\n{out}");
    }
    // a well-formed bound still proves, and a parameterised property that
    // feeds no proof is untouched
    for prop in ["capacity(10)", "max_key_bytes(64)", "ttl(30min)"] {
        std::fs::write(dir.join("app.cell"), cell(prop)).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_eq!(code, 0, "[{prop}]\n{out}");
    }
    // the same annotation on a state machine: `max_instances` is the only
    // one anything reads there, and a mistyped one left the proof on the
    // default instance count with nothing said
    let machine = |ann: &str| format!(
        "cell A {{\n    memory {{ st: Map<String, Int> [persistent, capacity(10)] }}\n         \x20   state flow {ann} {{\n        initial: a\n        a -> b\n    }}\n         \x20   scale {{\n        replicas: 1\n        memory: \"128Mi\"\n    }}\n         \x20   on go(id: String) {{ transition(id, \"b\")  return 1 }}\n}}\n");
    for ann in ["[max_instances()]", "[max_instances(\"big\")]", "[max_instances(10, 20)]"] {
        std::fs::write(dir.join("app.cell"), machine(ann)).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "[{ann}]\n{out}");
        assert!(out.contains("the memory proof would silently use the default"), "[{ann}]\n{out}");
    }
    for (ann, want) in [("[max_instanes(10)]", "did you mean `max_instances(N)`?"),
                        ("[persistent]", "is the only annotation a state machine takes")] {
        std::fs::write(dir.join("app.cell"), machine(ann)).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "[{ann}]\n{out}");
        assert!(out.contains("is read by nothing"), "[{ann}]\n{out}");
        assert!(out.contains(want), "[{ann}]\n{out}");
    }
    for ann in ["[max_instances(10)]", ""] {
        std::fs::write(dir.join("app.cell"), machine(ann)).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_eq!(code, 0, "[{ann}]\n{out}");
    }

    // a `memory:` the budget parser cannot read was indistinguishable from
    // declaring none: the proof was silently skipped and check said nothing
    let scaled = |mem: &str| format!(
        "cell A {{\n    memory {{ m: Map<String, Int> [persistent, capacity(10)] }}\n         \x20   scale {{\n        replicas: 1\n        memory: \"{mem}\"\n    }}\n         \x20   on r() {{ return 1 }}\n}}\n");
    for mem in ["128 Mo", "", "abc", "12.5Gi"] {
        std::fs::write(dir.join("app.cell"), scaled(mem)).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "[{mem}]\n{out}");
        assert!(out.contains("is not a size"), "[{mem}]\n{out}");
    }
    for mem in ["128Mi", "8Gi", "1Ti", "2G"] {
        std::fs::write(dir.join("app.cell"), scaled(mem)).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_eq!(code, 0, "[{mem}]\n{out}");
        assert!(out.contains("budget holds"), "[{mem}]\n{out}");
    }

    // and the bound that is written is the bound that is proven
    std::fs::write(dir.join("app.cell"), cell("capacity(10)")).unwrap();
    let (_, tight) = soma(&dir, &["check", "app.cell"]);
    std::fs::write(dir.join("app.cell"), cell("capacity(100)")).unwrap();
    let (_, loose) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(tight, loose, "the declared capacity must move the proven peak");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_cost_axis_speaks_only_for_the_bound_it_was_given() {
    // an empty `cost { }`, and `cost { usd: 1 }`, were both told their
    // 'tokens' bound was advisory — a bound the author never wrote. And a
    // declared usd bound that could not be proven said nothing at all
    let dir = scratch("cost_axis");
    let cell = |body: &str, handler: &str| format!(
        "cell A {{\n    cost {{\n{body}    }}\n    on r() {handler}\n}}\n");
    let unbounded = "[task] { return think(\"hi\") }";

    // nothing declared, nothing claimed
    let (code, out) = {
        std::fs::write(dir.join("app.cell"), cell("", unbounded)).unwrap();
        soma(&dir, &["check", "app.cell"])
    };
    assert_eq!(code, 0, "an empty cost block declares no bound\n{out}");
    assert!(!out.contains("advisory"), "{out}");

    // each axis reports on its own bound
    for (body, axis) in [("        tokens: 300\n", "tokens"), ("        usd: 1\n", "usd")] {
        std::fs::write(dir.join("app.cell"), cell(body, unbounded)).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "{out}");
        assert!(out.contains(&format!("'{axis}' bound is advisory")), "{out}");
        let other = if axis == "tokens" { "usd" } else { "tokens" };
        assert!(!out.contains(&format!("'{other}' bound")), "no axis speaks for a bound not given\n{out}");
    }

    // a bound that can be proven still is
    std::fs::write(dir.join("app.cell"),
        cell("        usd: 1\n", "{ return think(\"hi\", \"sys\", map(\"max_tokens\", 5)) }")).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("'usd' bound proven"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn fix_never_deletes_code_past_the_handler_line() {
    // removing a handler's `-> T` searched the WHOLE file for the opening
    // brace: with no body on that line it found the next cell's `{` and
    // deleted everything between — the closing brace and a whole cell
    // declaration — while reporting the fix as applied
    let dir = scratch("fix_return_type");
    let broken = "cell A {\n    on go(a: Int) -> Int\n}\ncell B {\n    on h() { return 1 }\n}\n";
    std::fs::write(dir.join("app.cell"), broken).unwrap();
    let (_, out) = soma(&dir, &["fix", "app.cell"]);
    assert_eq!(std::fs::read_to_string(dir.join("app.cell")).unwrap(), broken,
        "fix must leave the file alone when it cannot fix it\n{out}");
    assert!(out.contains("handlers do not declare return types"), "{out}");
    // the ordinary form, body on the same line, is still fixed
    std::fs::write(dir.join("app.cell"), "cell A {\n    on go(a: Int) -> Int { return a }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["fix", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("removed the return type"), "{out}");
    assert_eq!(std::fs::read_to_string(dir.join("app.cell")).unwrap(),
        "cell A {\n    on go(a: Int) { return a }\n}\n");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn add_keeps_the_manifest_the_author_wrote() {
    // `soma add` re-serialized the parsed manifest, so every comment the
    // author wrote disappeared and every default value was spelled out
    let dir = scratch("add_manifest");
    let hand = "# the project manifest — keep this order\n[package]\nname = \"demo\"\nversion = \"0.1.0\"\nentry = \"app.cell\"\n\n# what to prove\n[verify]\ndeadlock_free = true\neventually = [\"Done\"]\n\n[dependencies]\n";
    std::fs::write(dir.join("soma.toml"), hand).unwrap();
    let (code, out) = soma(&dir, &["add", "other", "--version", "1.2.3"]);
    assert_eq!(code, 0, "{out}");
    let after = std::fs::read_to_string(dir.join("soma.toml")).unwrap();
    assert_eq!(after, format!("{hand}other = \"1.2.3\"\n"), "only the dependency line may appear\n{after}");
    // a git dependency, then replacing an entry in place
    let (code, out) = soma(&dir, &["add", "mypkg", "--git", "https://example.com/x.git"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = soma(&dir, &["add", "other", "--version", "2.0.0"]);
    assert_eq!(code, 0, "{out}");
    let after = std::fs::read_to_string(dir.join("soma.toml")).unwrap();
    assert!(after.contains("# the project manifest — keep this order"), "{after}");
    assert!(after.contains("# what to prove"), "{after}");
    assert!(after.contains("other = \"2.0.0\"") && !after.contains("1.2.3"), "{after}");
    assert!(after.contains("[dependencies.mypkg]"), "{after}");
    assert!(!after.contains("author ="), "no default should be written out\n{after}");
    // and with no [dependencies] table at all
    std::fs::write(dir.join("soma.toml"), "# bare\n[package]\nname = \"demo\"\nversion = \"0.1.0\"\n").unwrap();
    let (code, out) = soma(&dir, &["add", "p", "--path", "../lib"]);
    assert_eq!(code, 0, "{out}");
    let after = std::fs::read_to_string(dir.join("soma.toml")).unwrap();
    assert!(after.starts_with("# bare\n") && after.contains("[dependencies.p]") && after.contains("path = \"../lib\""), "{after}");
    // a CRLF manifest keeps CRLF: rewriting every line ending is the same
    // whole-file diff this edit exists to avoid
    std::fs::write(dir.join("soma.toml"), "# windows\r\n[package]\r\nname = \"demo\"\r\nversion = \"0.1.0\"\r\n\r\n[dependencies]\r\n").unwrap();
    let (code, out) = soma(&dir, &["add", "w", "--version", "1.0"]);
    assert_eq!(code, 0, "{out}");
    let after = std::fs::read_to_string(dir.join("soma.toml")).unwrap();
    assert!(after.contains("w = \"1.0\""), "{after}");
    assert!(!after.replace("\r\n", "").contains('\n'), "every newline must stay CRLF\n{after:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_dashboard_is_same_origin_while_handlers_are_not() {
    // serving.md listed the dashboard among the responses carrying
    // `Access-Control-Allow-Origin: *`; the code deliberately withholds it
    // there so another origin cannot read the program's structure
    let dir = scratch("dashboard_cors");
    std::fs::write(dir.join("app.cell"), r#"
cell App {
    memory { n: Map<String, Int> [persistent] }
    on bump() { n.set("c", (n.get("c") ?? 0) + 1) return n.get("c") }
}
"#).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["serve", "app.cell", "--no-schedule", "-p", &port.to_string()])
            .current_dir(&dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_http(port);
    let header = "access-control-allow-origin";
    let handler = get(port, "/bump").to_lowercase();
    assert!(handler.contains(header), "a handler answers any origin\n{handler}");
    for path in ["/__soma/", "/__soma/hordes"] {
        let dash = get(port, path).to_lowercase();
        assert!(dash.contains("200 ok"), "{path}\n{dash}");
        assert!(!dash.contains(header), "{path} must stay same-origin\n{dash}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn duration_and_percentage_literals_render_as_written() {
    // the shared expression renderer had no arm for either, so they came out
    // as Rust's Debug — `promise latency < 200ms` printed
    // `Duration(Duration { value: 200.0, unit: Milliseconds })` in describe
    let dir = scratch("duration_render");
    std::fs::write(dir.join("app.cell"), "cell D {\n    face {\n        signal go() -> Int\n        promise latency < 200ms\n        promise error_rate < 5%\n    }\n    on go() { return 1 }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["describe", "app.cell", "--faces"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("promise latency < 200ms"), "{out}");
    assert!(out.contains("promise error_rate < 5%"), "{out}");
    assert!(!out.contains("Duration {") && !out.contains("Milliseconds"), "no Rust Debug\n{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_lambda_builtin_refuses_extra_arguments() {
    // the `>` of a lambda arrow was read as a closing generic when the
    // registry signature was parsed, so `filter(list: List, x => Bool)`
    // yielded no arity at all and every lambda-taking builtin accepted an
    // extra argument, silently ignored at run time
    let dir = scratch("lambda_arity");
    let bad = [
        ("filter([1, 2, 3], x => x > 1, 99)", "filter() takes at most 2 arguments (3 given)"),
        ("any([1, 2], x => x > 1, 1, 2)", "any() takes at most 2 arguments (4 given)"),
        ("count([1, 2], x => x > 0, 7)", "count() takes at most 2 arguments (3 given)"),
        ("find([1, 2], x => x > 1, 5)", "find() takes at most 2 arguments (3 given)"),
        ("all([1, 2], x => x > 0, 5)", "all() takes at most 2 arguments (3 given)"),
        ("reduce([1, 2], 0, p => p.acc + p.val, 9)", "reduce() takes at most 3 arguments (4 given)"),
        ("sort_by([map(\"k\", 2)], \"k\", \"desc\", 9)", "sort_by() takes at most 3 arguments (4 given)"),
    ];
    for (call, want) in bad {
        std::fs::write(dir.join("app.cell"), format!("cell L {{\n    on h() {{ return {call} }}\n}}\n")).unwrap();
        let (code, out) = soma(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "{call}\n{out}");
        assert!(out.contains(want), "{call}\nwant: {want}\n{out}");
    }
    // the right number of arguments still passes, piped form included
    std::fs::write(dir.join("app.cell"), "cell L {\n    on a() { return filter([1, 2, 3], x => x > 1) }\n    on b() { return sort_by([map(\"k\", 2)], \"k\") }\n    on c() { return reduce([1, 2], 0, p => p.acc + p.val) }\n    on d() { return [1, 2] |> filter(x => x > 1) }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn concat_needs_both_values() {
    // `concat(xs)` answered `xs` unchanged, so a forgotten second argument
    // produced a wrong result in silence — every sibling (split, replace,
    // contains…) refuses a missing argument with a `type` error
    let dir = scratch("concat_arity");
    std::fs::write(dir.join("app.cell"), r#"
cell C {
    on one() { let r = try { concat("a") } return if r.error != () { "ERR:" + r.kind } else { to_string(r.value) } }
    on none() { let r = try { concat() } return if r.error != () { "ERR:" + r.kind } else { to_string(r.value) } }
    on strings() { return concat("a", "b") }
    on lists() { return concat([1], [2]) }
    on mixed() { return concat("a", 1) }
}
cell test T {
    rules {
        assert C.one() == "ERR:type"
        assert C.none() == "ERR:type"
        assert C.strings() == "ab"
        assert C.lists() == [1, 2]
        assert C.mixed() == "a1"
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("5 tests: 5 passed"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn lowercase_and_uppercase_case_the_stringified_value() {
    // the doc says "non-strings are stringified first" — the stringified
    // form came back untouched, so `uppercase(true)` gave "true" and the
    // call quietly did nothing
    let dir = scratch("case_nonstring");
    std::fs::write(dir.join("app.cell"), r#"
cell U {
    on t() { return [uppercase(true), lowercase("AB"), uppercase("ab"), uppercase([1, "ab"]), uppercase(map("Ka", "Vb")), uppercase(12)] }
}
cell test T {
    rules {
        assert U.t() == ["TRUE", "ab", "AB", "[1, \"AB\"]", "{\"KA\": \"VB\"}", "12"]
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("1 tests: 1 passed"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn parallel_bulk_io_reads_what_the_sequential_form_reads() {
    // an empty list, an empty or missing directory, or a count of 0 made a
    // chunk size of 0 and panicked; par_read_files also took the first N in
    // directory order, not by name, and let a non-UTF-8 file use up a place
    let dir = scratch("par_bulk_io");
    let files = dir.join("files");
    std::fs::create_dir_all(&files).unwrap();
    for f in ["c", "a", "d", "b"] {
        std::fs::write(files.join(f), format!("w {f}")).unwrap();
    }
    std::fs::write(files.join("a0"), [0xffu8, 0xfe]).unwrap();
    std::fs::create_dir_all(dir.join("empty")).unwrap();
    std::fs::write(dir.join("app.cell"), r#"
cell P {
    on names(n: Int) { return par_read_files("files", n) |> map(f => f.content) }
    on seq(n: Int) { return read_files("files", n) |> map(f => f.content) }
}
cell test T {
    rules {
        assert par_word_count(list()) == map()
        assert par_read_files("empty", 5) == []
        assert par_read_files("missing", 5) == []
        assert P.names(0) == []
        assert P.names(2) == ["w a", "w b"]
        assert P.names(2) == P.seq(2)
        assert P.names(9) == P.seq(9)
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("7 tests: 7 passed"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_guard_is_not_proven_through_a_rebinding_the_require_does_not_cover() {
    // each handler narrows `amount` with require, then changes what the
    // transition() reads: inside a try block, as the loop variable, on a
    // later pass of the loop, as a lambda parameter, as a match binding.
    // verify said "proven" for all five and `soma run` failed the guard.
    let dir = scratch("guard_rebinding");
    std::fs::write(dir.join("app.cell"), r#"
cell Order {
    state order {
        initial: pending
        pending -> validated { guard { amount > 0 } }
    }
    on via_try(id: String, amount: Int) {
        require amount > 0 else "bad"
        let r = try { amount = 0 - 5 }
        transition(id, "pending", "validated")
        return 1
    }
    on via_for_var(id: String, amount: Int) {
        require amount > 0 else "bad"
        for amount in [0 - 5] { transition(id, "pending", "validated") }
        return 1
    }
    on via_loop(amount: Int) {
        require amount > 0 else "bad"
        for k in ["a", "b"] {
            transition(k, "pending", "validated")
            amount = 0 - 5
        }
        return 1
    }
    on via_lambda(id: String, amount: Int) {
        require amount > 0 else "bad"
        let r = [0 - 3] |> map(amount => transition(id, "pending", "validated"))
        return 1
    }
    on via_match(id: String, amount: Int, m: Map) {
        require amount > 0 else "bad"
        let r = match m { { amount } -> transition(id, "pending", "validated") }
        return 1
    }
    on kept(id: String, amount: Int) {
        require amount > 0 else "bad"
        let r = try { get_status(id) }
        for k in [1, 2] { let z = k }
        transition(id, "pending", "validated")
        return 1
    }
}
"#).unwrap();
    let (_, out) = soma(&dir, &["verify", "app.cell"]);
    let line = out.lines().find(|l| l.contains("guard `amount > 0`") && !l.contains("kept in the model")).unwrap_or_default().to_string();
    assert!(line.contains("runtime-checked in `via_try`, `via_for_var`, `via_loop`, `via_lambda`, `via_match`"), "{out}");
    assert!(line.contains("proven in `kept`"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn signal_route_binds_query_keys_by_name() {
    // `/signal/h?v=…&k=…` filled the parameters in query order, so the key
    // and the value of a write were swapped; `/h?…` already bound by name
    let dir = scratch("signal_query");
    std::fs::write(dir.join("app.cell"), r#"
cell App {
    on pair(k: String, v: String) { return k + "=" + v }
}
"#).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["serve", "app.cell", "--no-schedule", "-p", &port.to_string()])
            .current_dir(&dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_http(port);
    let named = get(port, "/signal/pair?v=VAL&k=KEY");
    assert!(named.contains("KEY=VAL"), "{named}");
    assert!(get(port, "/pair?v=VAL&k=KEY").contains("KEY=VAL"));
    let ordered = get(port, "/signal/pair?a=KEY&b=VAL");
    assert!(ordered.contains("KEY=VAL"), "{ordered}");
}

#[test]
fn a_sort_order_other_than_asc_or_desc_is_refused() {
    // "DESC", "descending" or `true` sorted ascending in silence
    passes("sort_order", r#"
cell S {
    on k(r: Map) { return if r.error != () { "ERR:" + r.kind } else { to_string(r.value) } }
}
cell test T {
    rules {
        assert sort([1, 3, 2], "desc") == [3, 2, 1]
        assert sort([1, 3, 2], "asc") == [1, 2, 3]
        assert sort_by([map("a", 1), map("a", 2)], "a", "desc") == [map("a", 2), map("a", 1)]
        assert sort_by([1, 2], x => x, "desc") == [2, 1]
        assert S.k(try { sort([1, 3, 2], "DESC") }) == "ERR:type"
        assert S.k(try { [1, 3, 2].sort(true) }) == "ERR:type"
        assert S.k(try { sort_by([map("a", 1)], "a", "descending") }) == "ERR:type"
        assert S.k(try { sort_by([1, 2], x => x, "DESC") }) == "ERR:type"
    }
}
"#);
}

#[test]
fn agg_refuses_a_function_it_does_not_compute() {
    // "v:median" and "v:SUM" answered v_median: () / v_SUM: (), a bare "v"
    // was dropped, and a text value was skipped where sum_by refuses it
    passes("agg_funcs", r#"
cell S {
    on k(r: Map) { return if r.error != () { "ERR:" + r.kind } else { to_string(r.value) } }
}
cell test T {
    rules {
        assert agg([map("g", 1, "v", 2), map("g", 1, "v", 4)], "g", "v:sum", "v:avg", "v:count")[0].v_avg == 3
        assert agg([map("g", 1, "v", "x")], "g", "v:count")[0].v_count == 1
        assert agg([map("g", 1, "v", "")], "g", "v:sum")[0].count == 1
        assert S.k(try { agg([map("g", 1, "v", 2)], "g", "v:median") }) == "ERR:type"
        assert S.k(try { agg([map("g", 1, "v", 2)], "g", "v:SUM") }) == "ERR:type"
        assert S.k(try { agg([map("g", 1, "v", 2)], "g", "v") }) == "ERR:type"
        assert S.k(try { agg([map("g", 1, "v", "abc"), map("g", 1, "v", 1)], "g", "v:sum") }) == "ERR:type"
    }
}
"#);
}

#[test]
fn a_list_literal_needs_its_commas() {
    // the comma was optional: `[1 2 3]` passed, and an unescaped quote
    // inside an interpolation turned one element into three in silence
    let dir = scratch("list_commas");
    for (i, src) in ["[1 2 3]", "[\"a\" \"b\"]", "[\"n={m.get(\"1\")}\"]"].iter().enumerate() {
        std::fs::write(dir.join(format!("l{i}.cell")), format!("cell L {{\n  on run() {{\n    let m = map(\"1\", 2)\n    print({src})\n  }}\n}}\n")).unwrap();
        let (code, out) = soma(&dir, &["check", &format!("l{i}.cell")]);
        assert_ne!(code, 0, "{src}: {out}");
        assert!(out.contains("separated by commas") || out.contains("quote"), "{src}: {out}");
    }
    passes("list_commas_ok", "cell L {\n  on t() { return [\n    1,\n    2,\n  ] }\n}\ncell test T {\n  rules {\n    assert L.t() == [1, 2]\n    assert [1 -1] == [0]\n  }\n}\n");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_native_comparison_on_a_bigint_answers_outside_an_if() {
    // `if x > 0` compared a BigInt fine; `return x > 0`, `let b = x > 0` and
    // `b = x > 0` lowered the Int to i64 and raised kind range
    passes("native_bigint_bool", r#"
cell N {
    on f(x: Int) [native] { return x > 0 }
    on g(x: Int) [native] {
        let b = x > 0
        let c = 0
        if b { c = 1 }
        return c
    }
    on h(x: Int, y: Int) [native] { return x == y }
    on k(n: Int) [native] {
        let s = 1
        for i in range(0, n) { s = s * 3 }
        return s % 2 == 1 && !(s < 0)
    }
    on m(x: Int) [native] {
        let b = false
        b = x >= 5 || x < 0 - 5
        return b
    }
}
cell test T {
    rules {
        assert N.f(shl(1, 70)) == true
        assert N.f(0 - shl(1, 70)) == false
        assert N.g(shl(1, 70)) == 1
        assert N.h(shl(1, 70), shl(1, 70)) == true
        assert N.k(60) == true
        assert N.m(0 - shl(1, 70)) == true
        assert N.m(1) == false
    }
}
"#);
}

#[test]
fn a_header_named_twice_sends_both_values() {
    // the second Set-Cookie overwrote the first: the session cookie was
    // dropped in silence, by response() and html() alike
    let dir = scratch("twice_headers");
    std::fs::write(dir.join("app.cell"), r#"
cell C {
    on request(method: String, path: String, body: String) {
        return match path {
            "/c" -> response(200, "ok", "Set-Cookie", "sid=1; HttpOnly", "Set-Cookie", "csrf=2", "X-One", "a")
            "/h" -> html(200, "<p>x</p>", "Set-Cookie", "a=1", "Set-Cookie", "b=2", "Set-Cookie", "c=3")
            _ -> response(404, "no")
        }
    }
}
"#).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["serve", "app.cell", "--no-schedule", "-p", &port.to_string()])
            .current_dir(&dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_http(port);
    let c = get(port, "/c");
    assert!(c.contains("Set-Cookie: sid=1; HttpOnly\r\n") && c.contains("Set-Cookie: csrf=2\r\n") && c.contains("X-One: a\r\n"), "{c}");
    let h = get(port, "/h");
    for v in ["a=1", "b=2", "c=3"] {
        assert!(h.contains(&format!("Set-Cookie: {v}\r\n")), "{h}");
    }
}

#[test]
fn html_with_one_argument_still_answers() {
    // the header pairs were read from args[2..] even with one argument
    passes("html_one_arg", "cell H {\n  on p() { return html(\"<p>x</p>\") }\n}\ncell test T {\n  rules {\n    assert H.p()._status == 200\n    assert html()._body == \"\"\n  }\n}\n");
}

#[test]
fn html_checks_its_status_like_response() {
    // the String "404" answered 200, and 1000 answered 500 after the
    // handler's writes had committed; response() refused both
    passes("html_status", r#"
cell H {
    memory { kv: Map<String, Int> [persistent] }
    on k(r: Map) { return if r.error != () { "ERR:" + r.kind } else { "ok" } }
    on page(code: Any) {
        kv.set("hit", 1)
        return html(code, "<p>x</p>")
    }
}
cell test T {
    rules {
        assert H.k(try { H.page("404") }) == "ERR:response"
        assert H.k(try { H.page(1000) }) == "ERR:response"
        assert kv.get("hit") == ()
        assert H.k(try { H.page(404) }) == "ok"
        assert H.k(try { html("<p>x</p>") }) == "ok"
    }
}
"#);
}

#[test]
fn word_count_keys_come_in_first_appearance_order() {
    // the counts went through a HashMap: another key order on every run
    passes("word_count_order", r#"
cell test T {
    rules {
        assert keys(word_count("zeta alpha Zeta beta gamma delta epsilon eta theta")) == ["zeta", "alpha", "beta", "gamma", "delta", "epsilon", "eta", "theta"]
        assert keys(word_count(["b a", "c b"])) == ["b", "a", "c"]
        assert keys(par_word_count(["b a", "c b", "d e", "f a", "g h", "i j", "k l", "m n", "o p", "q r", "s t", "u v"])) == ["b", "a", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v"]
        assert word_count("zeta alpha Zeta").zeta == 2
    }
}
"#);
}

#[test]
fn an_empty_top_level_verify_list_is_refused() {
    // `eventually = []`, `never = []`, `always = []` under [verify] checked
    // nothing and printed VERIFY OK; the after/before sections refused theirs
    for key in ["eventually", "never", "always"] {
        let dir = scratch(&format!("empty_verify_{key}"));
        std::fs::write(dir.join("app.cell"), "cell O {\n  state order {\n    initial: a\n    a -> b\n  }\n  on go(id: String) { transition(id, \"b\") }\n}\n").unwrap();
        std::fs::write(dir.join("soma.toml"), format!("[verify]\ndeadlock_free = true\n{key} = []\n")).unwrap();
        let (code, out) = soma(&dir, &["verify", "app.cell"]);
        assert_ne!(code, 0, "{key}: {out}");
        assert!(out.contains("names no state"), "{key}: {out}");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn a_wildcard_except_names_a_declared_state() {
    // `except [setled]` was accepted: it excepted nothing, and the wildcard
    // still left `settled`, the state the author meant to keep final
    let dir = scratch("except_unknown");
    std::fs::write(dir.join("app.cell"), "cell O {\n  state order {\n    initial: pending\n    pending -> settled\n    * -> cancelled except [setled]\n  }\n  on go(id: String) { transition(id, \"settled\") }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("names 'setled' (did you mean 'settled'?)"), "{out}");
    std::fs::write(dir.join("app.cell"), "cell O {\n  state order {\n    initial: pending\n    pending -> settled\n    * -> cancelled except [settled]\n  }\n  on go(id: String) { transition(id, \"settled\") }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn serve_watch_keeps_its_flags_and_its_server_stops_with_it() {
    // the watched server was started as `serve <file> -p <port>` only, so
    // --no-schedule (and --host, --verbose, …) were dropped; killing the
    // watcher left that server running, re-parented, holding the port
    let dir = scratch("serve_watch");
    std::fs::write(dir.join("app.cell"), r#"
cell W {
    memory { n: Map<String, Int> [persistent] }
    every 100ms { n.set("e", (n.get("e") ?? 0) + 1) }
    on x() { return n.get("e") ?? 0 }
}
"#).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut watcher = Server(
        Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["serve", "-w", "--no-schedule", "app.cell", "-p", &port.to_string()])
            .current_dir(&dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_http(port);
    std::thread::sleep(std::time::Duration::from_millis(600));
    let out = get(port, "/x");
    assert!(out.ends_with("{\"result\": 0}"), "timers ran under --no-schedule: {out}");
    let _ = watcher.0.kill();
    let _ = watcher.0.wait();
    let mut closed = false;
    for _ in 0..40 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_err() { closed = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(closed, "the watched server outlived its watcher");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn push_and_merge_refuse_what_is_not_their_collection() {
    // push(list(1)) answered [[1]], push(5, 6) the list [5, 6]; merge(5, m)
    // answered 5, merge(m, [1]) m unchanged, and a third map was ignored
    passes("push_merge_types", r#"
cell S {
    on k(r: Map) { return if r.error != () { "ERR:" + r.kind } else { to_string(r.value) } }
}
cell test T {
    rules {
        assert push([1]) == [1]
        assert push([1], 2, 3) == [1, 2, 3]
        assert [1].push(2) == [1, 2]
        assert S.k(try { push(5, 6) }) == "ERR:type"
        assert S.k(try { push("ab", "c") }) == "ERR:type"
        assert merge(map("a", 1), map("a", 2, "b", 3)) == map("a", 2, "b", 3)
        assert merge(map("a", 1), map("b", 2), map("c", 3)) == map("a", 1, "b", 2, "c", 3)
        assert S.k(try { merge(map("a", 1), [1]) }) == "ERR:type"
        assert S.k(try { merge(5, map("a", 1)) }) == "ERR:type"
    }
}
"#);
}

#[test]
fn a_native_float_to_int_past_64_bits_answers_like_the_interpreter() {
    // `let n = to_int(x)` (floor, ceil, round too) was kept an i64 local, so
    // x = 1e30 raised kind range with no BigInt path to fall back on
    passes("native_float_to_int", r#"
cell N {
    on f(x: Float) [native] {
        let n = to_int(x)
        return n + 1
    }
    on g(x: Float) [native] {
        let n = 0
        n = floor(x)
        return n
    }
    on h(x: Float) [native] {
        let s = 0
        for i in range(0, 3) { s = s + round(x) }
        return s
    }
    on i(x: Float) { return to_int(x) + 1 }
}
cell test T {
    rules {
        assert N.f(1.0e30) == N.i(1.0e30)
        assert N.g(0.0 - 1.0e25) == floor(0.0 - 1.0e25)
        assert N.h(1.0e20) == 3 * round(1.0e20)
        assert N.f(2.5) == 3
    }
}
"#);
}

#[test]
fn native_runaway_recursion_is_a_stack_overflow() {
    // the native depth guard came back as kind "type", which a try caught;
    // interpreted, runaway recursion is a stack_overflow that fails the
    // handler
    let dir = scratch("native_stack_overflow");
    std::fs::write(dir.join("app.cell"), r#"
cell R {
    on d(n: Int) [native] {
        if n == 0 { return 0 }
        return 1 + d(n - 1)
    }
    on run() {
        print(d(100))
        let r = try { d(30000) }
        print("caught {r.kind}")
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["run", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("100") && out.contains("stack overflow") && !out.contains("caught"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_native_bigint_division_rounds_once() {
    // the inexact BigInt / Int quotient divided the two f64 conversions,
    // rounding twice: 10^25 / 7 came out one ulp above the interpreter
    passes("native_bigint_div_round", r#"
cell D {
    on f(n: Int, d: Int) [native] {
        let s = 1
        for i in range(0, n) { s = s * 10 }
        return s / d
    }
    on g(n: Int, d: Int) {
        let s = 1
        for i in range(0, n) { s = s * 10 }
        return s / d
    }
}
cell test T {
    rules {
        assert D.f(25, 7) == D.g(25, 7)
        assert D.f(40, 0 - 7) == D.g(40, 0 - 7)
        assert D.f(33, 17) == D.g(33, 17)
        assert D.f(70, 6) == D.g(70, 6)
        assert D.f(20, 999999937) == D.g(20, 999999937)
    }
}
"#);
}

#[test]
fn a_compound_or_comparison_face_promise_is_checked_or_refused() {
    // `promise all_persistent && has_auth`, `promise all_persistemt ||
    // has_face` and `promise balance >= 0` passed check: only a bare
    // predicate was evaluated, and nothing enforced the rest at run time
    let dir = scratch("compound_promises");
    std::fs::write(dir.join("app.cell"), "cell API {\n  face {\n    signal put(v: Int)\n    promise all_persistent && has_auth\n    promise all_persistemt || has_face\n    promise balance >= 0\n  }\n  memory { balance: Map<String, Int> [ephemeral] }\n  on put(v: Int) { balance.set(\"a\", v) }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("promise 'all_persistent && has_auth' is not satisfied"), "{out}");
    assert!(out.contains("'all_persistemt' (did you mean 'all_persistent'?)"), "{out}");
    assert!(out.contains("promises `balance >= 0`, which nothing checks"), "{out}");
    std::fs::write(dir.join("app.cell"), "cell A {\n  face {\n    signal put(v: Int)\n    promise has_memory && !all_encrypted\n    promise all_encrypted || has_face\n  }\n  memory { x: Map<String, Int> [ephemeral] }\n  on put(v: Int) { x.set(\"a\", v) }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_edge_repeating_a_move_with_another_guard_is_refused() {
    // a transition takes the FIRST edge that matches: `* -> c` before
    // `a -> c { guard }` left the guard never evaluated, and `b -> d { guard }`
    // before `b -> d` refused a move the second edge allowed
    let dir = scratch("duplicate_edges");
    std::fs::write(dir.join("app.cell"), "cell O {\n  state s {\n    initial: a\n    a -> b\n    * -> c\n    a -> c { guard { x > 0 } }\n    b -> d { guard { x > 0 } }\n    b -> d\n  }\n  on go(id: String, x: Int) { transition(id, \"c\")  transition(id, \"d\") }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("`a -> c { guard … }` repeats the move a -> c that `* -> c` already declares"), "{out}");
    assert!(out.contains("`b -> d` repeats the move b -> d that `b -> d { guard … }` already declares"), "{out}");
    // the same guard on both (none here) answers the same either way
    std::fs::write(dir.join("app.cell"), "cell O {\n  state s {\n    initial: a\n    a -> b\n    b -> c\n    * -> c\n  }\n  on go(id: String) { transition(id, \"c\") }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_native_bigint_prints_through_to_string_and_sb_push_int() {
    // `str_len(to_string(s))` and `sb_push_int(sb, s)` lowered s to i64 and
    // raised kind range once s had grown past 64 bits; a returned
    // `to_string(s)` worked
    passes("native_bigint_print", r#"
cell N {
    on f(n: Int) [native] {
        let s = 1
        for i in range(0, n) { s = s * 3 }
        return str_len(to_string(s))
    }
    on g(n: Int) [native] {
        let s = 1
        for i in range(0, n) { s = s * 3 }
        let sb = strbuf()
        sb_push_int(sb, s)
        sb_push(sb, "!")
        return sb_finish(sb)
    }
}
cell test T {
    rules {
        assert N.f(50) == 24
        assert N.g(50) == "717897987691852588770249!"
        assert N.g(3) == "27!"
    }
}
"#);
}

#[test]
fn zip_enumerate_and_flatten_refuse_what_is_not_a_list() {
    // zip([1], "ab"), enumerate("ab") and flatten(5) answered [] in silence
    passes("list_builtins_types", r#"
cell S {
    on k(r: Map) { return if r.error != () { "ERR:" + r.kind } else { to_string(r.value) } }
}
cell test T {
    rules {
        assert zip([1, 2], ["a"]) == [map("left", 1, "right", "a")]
        assert enumerate(["x"]) == [map("index", 0, "value", "x")]
        assert flatten([[1], 2, [3, 4]]) == [1, 2, 3, 4]
        assert S.k(try { zip([1], "ab") }) == "ERR:type"
        assert S.k(try { enumerate("ab") }) == "ERR:type"
        assert S.k(try { flatten(5) }) == "ERR:type"
    }
}
"#);
}

#[test]
fn to_float_reads_what_to_int_reads() {
    // to_float(" 1.5 ") and a line with its "\n" gave () where to_int
    // trims; to_float(true) gave () where to_int(true) is 1
    passes("to_float_like_to_int", r#"
cell test T {
    rules {
        assert to_float(" 1.5 ") == 1.5
        assert to_float("2.5\n") == 2.5
        assert to_int(" 12 ") == 12
        assert to_float(true) == 1.0
        assert to_float(false) == 0.0
        assert to_float("x") == ()
    }
}
"#);
}

#[test]
fn soma_fix_keeps_annotations_and_the_meaning_of_arithmetic() {
    // removing `-> Int` took `[native]` / `[task]` with it; replacing `;`
    // by spaces turned `let d = a; -b` into `let d = a   -b`, i.e. a - b
    let dir = scratch("fix_meaning");
    std::fs::write(dir.join("rt.cell"), "cell A {\n    face {\n        signal sq(x: Int) -> Int\n    }\n    on sq(x: Int) -> Int [native] {\n        return x * x\n    }\n}\n").unwrap();
    let (_, out) = soma(&dir, &["fix", "rt.cell"]);
    let fixed = std::fs::read_to_string(dir.join("rt.cell")).unwrap();
    assert!(fixed.contains("on sq(x: Int) [native] {"), "{out}\n{fixed}");
    std::fs::write(dir.join("semi.cell"), "cell A {\n    on f(a: Int, b: Int) {\n        let xs = [a]; let ys = [b]\n        let d = a; -b\n        return d\n    }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["fix", "semi.cell"]);
    let fixed = std::fs::read_to_string(dir.join("semi.cell")).unwrap();
    assert!(fixed.contains("let d = a; -b"), "{out}\n{fixed}");
    assert!(!fixed.contains("let xs = [a];"), "{out}\n{fixed}");
    assert_ne!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn termination_needs_one_measure_and_a_base_case_at_the_halving_fixed_point() {
    // `log2(0)` (base cases n < 0 and n == 1) recursed forever on idiv(0, 2);
    // `walk` decreased n in one call and m in the other: (5,5) → (6,4) →
    // (5,5). verify --strict proved both.
    let dir = scratch("termination_measure");
    std::fs::write(dir.join("app.cell"), r#"
cell T {
    on log2(n: Int) {
        if n < 0 { return -1 }
        if n == 1 { return 0 }
        return 1 + log2(idiv(n, 2))
    }
    on walk(n: Int, m: Int) {
        if n <= 0 { return 0 }
        if m <= 0 { return 0 }
        if n > m { return walk(n - 1, m + 1) }
        return walk(n + 1, m - 1)
    }
    on ok_log(n: Int) {
        if n <= 1 { return 0 }
        return 1 + ok_log(idiv(n, 2))
    }
    on ok_digits(a: Int) {
        if a == 0 { return 0 }
        return 1 + ok_digits(idiv(a, 3))
    }
    on ok_req(n: Int) {
        require n >= 0 else "neg"
        if n == 0 { return 0 }
        return 1 + ok_req(n - 1)
    }
    on ok_two(n: Int, m: Int) {
        if n <= 0 { return m }
        if m > 3 { return ok_two(n - 1, m - 1) }
        return ok_two(n - 2, m + 1)
    }
}
"#).unwrap();
    let (_, out) = soma(&dir, &["verify", "app.cell"]);
    assert!(out.contains("handler `log2`: halving `n`"), "{out}");
    assert!(out.contains("handler `walk`: no single parameter"), "{out}");
    for ok in ["ok_log", "ok_digits", "ok_req", "ok_two"] {
        assert!(!out.contains(&format!("handler `{ok}`")), "{ok}: {out}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_invariant_is_not_proven_through_a_float_modulo_or_a_nan_into_an_untyped_slot() {
    // `x % 3` was bounded by 2 whatever x held (2.5 % 3 is 2.5), and the NaN
    // guard only looked at slots typed Float: `abs(sqrt(x))` into a `Map`
    // slot was "proven" >= 0, then NaN was rejected at run time
    let dir = scratch("invariant_float_mod_nan");
    std::fs::write(dir.join("app.cell"), r#"
cell T {
    memory {
        c: Map [persistent]
        d: Map [persistent]
        e: Map<String, Int> [persistent]
        invariant c <= 2
        invariant d >= 0
        invariant e <= 2
    }
    on rem(x: Float) { c.set("r", x % 3) }
    on mag(x: Float) { d.set("m", abs(sqrt(x))) }
    on irem(x: Int) { e.set("r", x % 3) }
}
"#).unwrap();
    let (_, out) = soma(&dir, &["verify", "app.cell"]);
    assert!(out.contains("⚠ invariant c <= 2"), "{out}");
    assert!(out.contains("⚠ invariant d >= 0"), "{out}");
    assert!(out.contains("writer 'irem' proven"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_literal_arm_does_not_make_a_variant_match_exhaustive() {
    // `"Yellow" -> …` or `() -> …` beside `Red -> …` switched the check off,
    // and Green raised "non-exhaustive match" at run time
    let dir = scratch("exhaustive_literal_arm");
    std::fs::write(dir.join("app.cell"), "cell type Light { variants { Red  Yellow  Green } }\ncell X {\n  on lit(l: Light) { return match l { Red -> \"stop\"  \"Yellow\" -> \"slow\" } }\n  on unit(l: Light) { return match l { Red -> \"stop\"  () -> \"none\" } }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert_eq!(out.matches("non-exhaustive match on 'Light'").count(), 2, "{out}");
    std::fs::write(dir.join("app.cell"), "cell type Light { variants { Red  Yellow  Green } }\ncell X {\n  on a(l: Light) { return match l { Red -> \"stop\"  \"Yellow\" -> \"slow\"  _ -> \"go\" } }\n  on b(l: Light) { return match l { Red -> \"stop\"  other -> \"go\" } }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_string_pattern_is_interpolated_like_any_string() {
    // `match v { "user-{id}" -> … }` compared the raw text, braces included,
    // while `v == "user-{id}"` was true; `"{nosuch}"` passed check
    passes("pattern_interpolation", r#"
cell P {
    on m(v: String, id: String) { return match v { "user-{id}" -> "matched"  _ -> "no" } }
    on esc(v: String) { return match v { "{{x}}" -> "escaped"  _ -> "no" } }
    on pre(v: String, base: String) { return match v { "{base}/" + rest -> rest  _ -> "none" } }
}
cell test T {
    rules {
        assert P.m("user-42", "42") == "matched"
        assert P.m("user-{{id}}", "42") == "no"
        assert P.esc("{{x}}") == "escaped"
        assert P.esc("{{{{x}}}}") == "no"
        assert P.pre("/api/users", "/api") == "users"
    }
}
"#);
    let dir = scratch("pattern_interpolation_check");
    std::fs::write(dir.join("app.cell"), "cell P {\n  on f(v: String) { return match v { \"{nosuch}\" -> 1  _ -> 2 } }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("undefined variable 'nosuch'"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_or_pattern_binds_only_what_every_alternative_binds() {
    // `Up(n) || Wait -> n` passed check and raised "undefined variable: n"
    // on Wait: the bindings of the alternatives were united
    let dir = scratch("or_pattern_bindings");
    std::fs::write(dir.join("app.cell"), "cell type Move { variants { Up(Int) Down(Int) Wait } }\ncell X {\n  on f(m: Move) { return match m { Up(n) || Wait -> n  Down(n) -> 0 } }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("undefined variable 'n'"), "{out}");
    std::fs::write(dir.join("app.cell"), "cell type Move { variants { Up(Int) Down(Int) Wait } }\ncell X {\n  on g(m: Move) { return match m { Up(n) || Down(n) -> n  Wait -> 0 } }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_record_pattern_matches_a_record() {
    // `Game { bet } -> …` only matched a struct variant: a record (a Map
    // carrying `_type`) always fell through to the next arm
    passes("record_pattern", r#"
cell X {
    on f(v: Any) { return match v { Game { bet } -> "game {bet}"  _ -> "other" } }
    on g(v: Any) { return match v { Game { bet, .. } -> "game {bet}"  _ -> "other" } }
}
cell test T {
    rules {
        assert X.f(Game { bet: 10 }) == "game 10"
        assert X.f(Game { bet: 10, who: "a" }) == "other"
        assert X.g(Game { bet: 10, who: "a" }) == "game 10"
        assert X.f(Bet { bet: 1 }) == "other"
        assert X.f(map("bet", 3)) == "other"
    }
}
"#);
}

#[test]
fn each_test_cell_starts_with_no_hordes() {
    // T1's horde and its results were visible to T2, and T2's first horde
    // was Audit:h2: the horde tables outlived the per-cell reset
    passes("horde_test_isolation", r#"
cell Audit {
    on work(x: Int) [task] { return x * 2 }
    on start() { return horde(Audit.work, [1, 2, 3], map("concurrency", 2)) }
    on seen(id: String) { let r = try { horde_status(id) }  return r.error == () }
}
cell test T1 {
    rules {
        let h = start()
        assert h == "Audit:h1"
        assert horde_status(h).done == 3
    }
}
cell test T2 {
    rules {
        assert Audit.seen("Audit:h1") == false
        assert start() == "Audit:h1"
    }
}
"#);
}

#[test]
fn lint_does_not_advise_dropping_json_around_a_string_slot() {
    // a `Map<String, String>` slot keeps JSON as text: dropping to_json /
    // from_json as advised made the write a type error and the read a String
    let dir = scratch("lint_json_text_slot");
    let src = "cell A {\n    memory {\n        docs: Map<String, String> [persistent]\n        maps: Map<String, Map> [persistent]\n    }\n    on save(k: String, n: Int) {\n        docs.set(k, to_json(map(\"n\", n)))\n        maps.set(k, to_json(map(\"n\", n)))\n        return k\n    }\n    on load(k: String) {\n        return from_json(docs.get(k)).n\n    }\n    on load2(k: String) {\n        return from_json(maps.get(k))\n    }\n}\n";
    std::fs::write(dir.join("app.cell"), src).unwrap();
    let (_, out) = soma(&dir, &["lint", "app.cell"]);
    assert_eq!(out.matches("redundant to_json()").count(), 1, "{out}");
    assert_eq!(out.matches("redundant from_json()").count(), 1, "{out}");
    assert!(out.contains("line 8:") && out.contains("line 15:"), "{out}");
    assert!(!out.contains("line 7:") && !out.contains("line 12:"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn mock_approve_takes_a_bool() {
    // `mock approve "false"` approved: the answer went through is_truthy,
    // and a non-empty String is truthy
    let dir = scratch("mock_approve_bool");
    std::fs::write(dir.join("app.cell"), "cell G {\n  on ask() { return approve(\"ship?\") }\n}\ncell test T {\n  rules {\n    mock approve \"false\"\n  }\n}\ncell test U {\n  rules {\n    mock approve [true, false]\n    assert G.ask() == true\n    assert G.ask() == false\n  }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("mock approve takes true / false"), "{out}");
    assert!(out.contains("✓ assert G.ask() == true") && out.contains("✓ assert G.ask() == false"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn soma_test_over_a_project_skips_libraries_and_answers_one_json_document() {
    // `soma test .` counted `lib/helpers.cell` (no test cell) as a failed
    // file, so every multi-file project failed; --json wrote one object per
    // file back to back
    let dir = scratch("test_dir_project");
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/helpers.cell"), "cell Helpers {\n  on double(x: Int) { return x * 2 }\n}\n").unwrap();
    std::fs::write(dir.join("app.cell"), "use lib::helpers\ncell App {\n  on run(x: Int) { return double(x) }\n}\ncell test AppTests {\n  rules {\n    assert run(2) == 4\n  }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["test", "."]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("no test cells, skipped"), "{out}");
    let json = Command::new(env!("CARGO_BIN_EXE_soma")).args(["test", "--json", "."]).current_dir(&dir).output().unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&json.stdout).expect("one JSON document");
    assert_eq!(doc["ok"], true, "{doc}");
    assert_eq!(doc["files"].as_array().map(|a| a.len()), Some(1), "{doc}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn format_d_reads_its_precision_as_a_minimum_of_digits() {
    // the precision of %d was parsed and dropped: format("%.3d", 7) was "7"
    passes("format_d_precision", r#"
cell test T {
    rules {
        assert format("%.3d", 7) == "007"
        assert format("%.3d", 0 - 7) == "-007"
        assert format("%6.3d|", 7) == "   007|"
        assert format("%-6.3d|", 7) == "007   |"
        assert format("%.2d", 1234) == "1234"
        assert format("%05d", 0 - 3) == "-0003"
    }
}
"#);
}

#[test]
fn a_variant_pattern_on_get_status_is_refused() {
    // `match get_status(id) { Pending -> … }` passed check and raised
    // "match expects a variant … got String" on every call
    let dir = scratch("get_status_variant_match");
    std::fs::write(dir.join("app.cell"), "cell type S { variants { Pending Done } }\ncell L {\n  state todo: S { initial: Pending  Pending -> Done }\n  on show(id: String) { return match get_status(id) { Pending -> \"p\"  Done -> \"d\" } }\n  on ok(id: String) { return match get_status(id) { \"Pending\" -> \"p\"  _ -> \"o\" } }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert_eq!(out.matches("is a variant pattern").count(), 1, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Start `soma serve app.cell` in `dir` on a free port.
fn serve_dir(dir: &Path) -> (Server, u16) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["serve", "app.cell", "--no-schedule", "-p", &port.to_string()])
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_http(port);
    (server, port)
}

/// Send one raw HTTP/1.0 request and read the whole answer.
fn raw_request(port: u16, request: &str) -> String {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = stream.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn a_content_type_value_is_checked_like_any_header() {
    // `response(200, x, "Content-Type", q.t)` sent a CR/LF in the value as a
    // second header (Set-Cookie injection) and panicked on a non-ASCII byte
    let dir = scratch("content_type_header");
    std::fs::write(dir.join("app.cell"), r#"
cell App {
    on request(method: String, path: String, body: String, q: Map) {
        return response(200, "hello", "Content-Type", q.t ?? "text/plain", "X-Name", q.n ?? "ok")
    }
}
"#).unwrap();
    let (_server, port) = serve_dir(&dir);
    let out = get(port, "/ct?t=text/plain%0D%0ASet-Cookie:%20pwn=1");
    assert!(out.starts_with("HTTP/1.0 200"), "{out}");
    assert!(!out.to_ascii_lowercase().contains("set-cookie"), "{out}");
    let out = get(port, "/ct?t=text/plain%3B%20charset%3D%C3%A9&n=%C3%A9");
    assert!(out.starts_with("HTTP/1.0 200"), "{out}");
    assert!(out.contains("hello"), "{out}");
    let out = get(port, "/ct?t=text/csv");
    assert!(out.contains("Content-Type: text/csv"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_handler_writing_through_put_is_not_run_on_get() {
    // `bal.put(k, v)` is `bal.set(k, v)`, but the GET gate did not count it
    // as a write: `GET /wipe/alice` zeroed the balance (CSRF)
    let dir = scratch("put_on_get");
    std::fs::write(dir.join("app.cell"), r#"
cell App {
    memory { bal: Map<String, Int> [persistent] }
    on wipe(k: String) { bal.put(k, 0)  return "ok" }
}
"#).unwrap();
    let (_server, port) = serve_dir(&dir);
    let out = get(port, "/wipe/alice");
    assert!(out.starts_with("HTTP/1.0 405"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_deterministic_handler_refuses_every_storage_write() {
    // only .set/.delete/.push were refused: put, remove, slot[k] = v and a
    // clock read in a `require` passed the contract
    let dir = scratch("deterministic_storage");
    std::fs::write(dir.join("app.cell"), r#"
cell T {
    memory { bal: Map<String, Int> [persistent] }
    on p(k: String) [deterministic] { bal.put(k, 1) return 1 }
    on i(k: String) [deterministic] { bal[k] = 2 return 1 }
    on r(k: String) [deterministic] { bal.remove(k) return 1 }
    on c(k: String) [deterministic] { require now() > 0 else Late  return 1 }
    on ok(bal: Int) [deterministic] { return bal + 1 }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    for h in ["'p'", "'i'", "'r'", "'c'"] {
        assert!(out.contains(&format!("handler {h} is [deterministic]")), "{h}: {out}");
    }
    assert!(!out.contains("handler 'ok'"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_slot_named_like_a_machine_store_is_refused() {
    // `__sm_Order_order.set(id, "shipped")` moved the machine with no
    // transition(): no edge, guard or status rule was checked
    let dir = scratch("reserved_slot_prefix");
    std::fs::write(dir.join("app.cell"), r#"
cell Order {
    memory { __sm_Order_order: Map<String, String> [persistent] }
    state order { initial: pending  pending -> paid }
    on hack(id: String) { __sm_Order_order.set(id, "paid") return get_status(id) }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("reserved for the runtime"), "{out}");
    let (code, _) = soma(&dir, &["run", "app.cell", "hack", "o1"]);
    assert_ne!(code, 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_method_form_transition_reads_the_callers_locals() {
    // `id.transition("approved")` never handed its locals to the guard: it
    // failed "undefined variable", or read the locals an earlier invalid
    // transition() had left behind and approved 5000 under `amount < 1000`
    passes("method_transition_guard", r#"
cell Orders {
    state o { initial: pending  pending -> approved { guard { amount < 1000 } } }
    on approve_order(id: String, amount: Int) { id.transition("approved")  return get_status(id) }
    on approve_plain(id: String, amount: Int) { let r = try { transition(id, "approved") }  return r.kind }
    on after_invalid() {
        approve_plain("small", 5)
        approve_plain("small", 5)
        let r = try { approve_order("big", 5000) }
        return r.kind
    }
}
cell test T { rules {
    assert approve_order("a", 5) == "approved"
    assert_fails approve_order("b", 5000) matching "guard"
    assert after_invalid() == "guard_failed"
} }
"#);
}

#[test]
fn a_failing_try_in_a_test_rule_rolls_back_like_in_a_handler() {
    // each handler called from the `try` committed on its own: the rule
    // saw writes and transitions a handler's `try` would have undone
    passes("test_rule_try_rollback", r#"
cell Bank {
    memory { bal: Map<String, Int> [persistent] }
    state acct { initial: open  open -> frozen }
    on dep(k: String, n: Int) { bal.set(k, n) }
    on getb(k: String) { return bal.get(k) }
}
cell test T { rules {
    let r = try { dep("x", 5)  transition("x", "frozen")  fail("boom", "") }
    assert r.kind == "boom"
    assert getb("x") == ()
    assert get_status("x") == "open"
    let ok = try { dep("y", 7) }
    assert getb("y") == 7
} }
"#);
}

#[test]
fn native_float_locals_print_as_floats() {
    // every Float local counted as a possible exact quotient: a sum of
    // 0.5s printed "2", -0.0 printed "0", a copy of a Float param "3"
    passes("native_float_locals", r#"
cell P {
    on acc() [native] { let t = 0.0  for i in range(0, 4) { t = t + 0.5 }  return to_string(t) }
    on negz() [native] { let z = -0.0  return to_string(z) }
    on fcopy(x: Float) [native] { let y = x  return to_string(y) }
    on prod() [native] { let b = 1.5 * 2.0  return to_string(b) }
    on quot(a: Int, b: Int) [native] { let q = a / b  return to_string(q) }
    on loopvar() [native] { let s = 0  for i in range(0, 5) { i = i + 2  s = s + i }  return s }
}
cell test T { rules {
    assert acc() == "2.0"
    assert negz() == "-0.0"
    assert fcopy(3.0) == "3.0"
    assert prod() == "3.0"
    assert quot(8, 4) == "2"
    assert quot(7, 2) == "3.5"
    assert loopvar() == 20
} }
"#);
}

#[test]
fn native_refuses_a_bool_or_number_return_and_string_plus_number() {
    // `if x < 0 { return false }  return x * 2` answered true for 5, and
    // `"n=" + n` joined the text where the interpreter raises
    let dir = scratch("native_mixed_types");
    std::fs::write(dir.join("app.cell"), r#"
cell P {
    on f(x: Int) [native] { if x < 0 { return false }  return x * 2 }
    on g(x: Int) [native] { if x < 0 { return x > -5 }  return x * 2 }
    on label(n: Int) [native] { return "n=" + (n + 1) }
    on and_mix(x: Int) [native] { if x < 0 { return 1 }  return x > 2 && x < 9 }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("handler 'f' is marked [native] but returns a Bool on one path"), "{out}");
    assert!(out.contains("handler 'g' is marked [native] but returns a Bool on one path"), "{out}");
    assert!(out.contains("handler 'and_mix' is marked [native] but returns a Bool on one path"), "{out}");
    // the diagnostic keeps the author's parentheses
    assert!(out.contains("`\"n=\" + (n + 1)`"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn native_quotients_keep_their_own_exactness() {
    // exactness was one handler-wide flag: an unrelated inexact division
    // made an exact quotient a Float; a sibling's exact quotient reached its
    // caller as a Float; `q * q` past 2^53 answered wrong digits; and
    // `s = s + q` truncated 10.5 to 9
    passes("native_quotient_exactness", r#"
cell P {
    on mean_and_half(total: Int, n: Int) [native] { let mean = total / n  let h = n / 2  return h }
    on show_half(total: Int, n: Int) [native] { let mean = total / n  let h = n / 2  return to_string(h) }
    on half(n: Int) [native] { return n / 2 }
    on sib_str(n: Int) [native] { return to_string(half(n)) }
    on sib_let(n: Int) [native] { let a = half(n)  return a }
    on sib_mul(n: Int) [native] { return half(n) * 2 }
    on sqmod(x: Int, y: Int) [native] { let q = x / y  return q * q % 1000 }
    on acc(x: Int, y: Int) [native] { let q = x / y  let s = 0  for i in range(0, 3) { s = s + q }  return s }
}
cell test T { rules {
    assert mean_and_half(7, 4) == 2
    assert type_of(mean_and_half(7, 4)) == "Int"
    assert show_half(7, 4) == "2"
    assert sib_str(8) == "4"
    assert sib_str(7) == "3.5"
    assert sib_let(8) == 4
    assert type_of(sib_let(8)) == "Int"
    assert sib_mul(8) == 8
    assert sqmod(1000, 10) == 0
    assert_fails sqmod(100000001, 1) matching "2^53"
    assert acc(8, 2) == 12
    assert_fails acc(7, 2) matching "never truncates"
} }
"#);
}

#[test]
fn native_bigint_products_of_small_locals_promote() {
    // `let x = 3037000500  return x * x` overflowed i64 inside the BigInt
    // fallback itself; a range bound past 64 bits leaked Rust's
    // "called `Option::unwrap()` on a `None` value"
    passes("native_small_products", r#"
cell P {
    on sq() [native] { let x = 3037000500  let y = x * x  return y }
    on sq_abs() [native] { let x = 3037000500  return abs(x * x) }
    on loop_sum(k: Int) [native] { let x = 3037000000 + k  let s = 0  for i in range(0, 3) { s = s + x * x }  return s }
    on r(b: Int) [native] { let s = 0  for i in range(b, b + 3) { s = s + 1 }  return s }
    on p(e: Int) [native] { return pow_mod(2, e, 7) }
}
cell test T { rules {
    assert sq() == 9223372037000250000
    assert sq_abs() == 9223372037000250000
    assert loop_sum(5) == 27670107091110000075
    assert_fails r(100000000000000000000) matching "64 bits"
    assert_fails p(0 - 1) matching "modular inverse"
} }
"#);
}

#[test]
fn a_unit_key_is_no_key_for_reads_and_deletes() {
    // set((), v) is refused, but get/has/[] read the "null" entry and
    // `users.delete(body.userid)` (a mistyped field) deleted it
    passes("unit_key_reads", r#"
cell Users {
    memory { users: Map<String, Int> [persistent] }
    on seed() { users.set("null", 7) }
    on read(body: Map) { return [users.get(body.user_id), users.has(body.user_id), users[body.user_id]] }
    on remove(body: Map) { return users.delete(body.user_id) }
    on count() { return users.len }
}
cell test T { rules {
    let s = seed()
    assert read(map("userid", "u1")) == [(), false, ()]
    assert_fails remove(map("userid", "u1")) matching "the key is ()"
    assert count() == 1
} }
"#);
}

#[test]
fn a_machine_state_with_fields_is_refused() {
    // `state s: St` with `Funded(Int)` passed check and verify, then
    // `transition(id, Funded(n))` raised "invalid transition" on every run
    // (and `transition(id, Funded)` "takes payload")
    let dir = scratch("machine_state_fields");
    std::fs::write(dir.join("app.cell"), r#"
cell type St { variants { Pending  Funded(Int)  Done { at: Int } } }
cell Escrow {
    state s: St { initial: Pending  Pending -> Funded  Funded -> Done }
    on fund(id: String, n: Int) { transition(id, Funded(n))  return get_status(id) }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("state 'Funded' of machine 's'") && out.contains("state 'Done' of machine 's'"), "{out}");
    assert!(out.contains("transitions to Funded(n) built with fields"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_int_in_a_nested_float_position_becomes_a_float() {
    // only a top-level Float slot promoted an Int: Map<String, List<Float>>
    // kept [1, 2] as Ints
    passes("nested_float_promotion", r#"
cell Prices {
    memory {
        hist: Map<String, List<Float>> [persistent]
        nested: Map<String, Map<String, Float>> [persistent]
    }
    on put() { hist.set("a", [1, 2.5])  nested.set("a", map("x", 3)) }
    on show() { return [type_of(hist.get("a")[0]), to_json(hist.get("a")), type_of(nested.get("a").x)] }
}
cell test T { rules {
    let p = put()
    assert show() == ["Float", "[1.0,2.5]", "Float"]
} }
"#);
}

#[test]
fn a_map_int_slot_keeps_int_keys() {
    // Map<Int, V> ignored its key type: keys came back as Strings sorted as
    // text (["10", "2"]), "not-a-number" was stored, and `invariant key >= 1`
    // was refused as comparing a String key
    passes("map_int_keys", r#"
cell Seats {
    memory { seats: Map<Int, String> [persistent]
        invariant key >= 1 }
    on book(n: Int, who: String) { seats.set(n, who)  return seats.keys }
    on bad() { seats.set("not-a-number", "x") }
    on total() { let t = 0  for k in seats.keys { t = t + k }  return t }
    on first_entry() { return seats.entries()[0].key }
}
cell test T { rules {
    assert book(2, "ada") == [2]
    assert book(10, "bob") == [2, 10]
    assert max(book(3, "cy")) == 10
    assert total() == 15
    assert first_entry() == 2
    assert_fails bad() matching "keys are Ints"
    assert_fails book(0, "zed") matching "invariant"
} }
"#);
}

/// `soma verify` on one program: its output.
fn verify_out(name: &str, src: &str) -> String {
    let dir = scratch(name);
    std::fs::write(dir.join("app.cell"), src).unwrap();
    let (_, out) = soma(&dir, &["verify", "app.cell"]);
    let _ = std::fs::remove_dir_all(dir);
    out
}

#[test]
fn verify_proves_no_invariant_through_a_changed_value() {
    // proven, then the run rejected the write: a callee's `return` in a
    // match arm was not one of its values; a require over a slot another
    // handler rewrites, over random() or now(), held "at the write"
    let cases = [
        ("inv_match_return", "cell A {\n memory { c: Map<String, Int> [persistent]\n  invariant c >= 0 }\n on score(n: Int) { let r = match n { 1 -> { return 0 - 5 } _ -> 0 }  return 1 }\n on put(k: String, n: Int) { c.set(k, score(n))  return \"ok\" }\n}\n"),
        ("inv_callee_write", "cell A {\n memory { c: Map<String, Int> [persistent]\n  limits: Map<String, Int> [persistent]\n  invariant c >= 0 && c <= 2 }\n on set_limit(k: String, v: Int) { limits.set(k, v) }\n on copy(k: String) {\n  require (limits.get(k) ?? 0) >= 0 && (limits.get(k) ?? 0) <= 2 else Bad\n  set_limit(k, 99)\n  c.set(k, limits.get(k) ?? 0)\n }\n}\n"),
        ("inv_random", "cell A {\n memory { c: Map<String, Int> [persistent]\n  invariant c <= 0 }\n on roll() { require random(2) <= 0 else Unlucky  c.set(\"r\", random(2)) }\n}\n"),
    ];
    for (name, src) in cases {
        let out = verify_out(name, src);
        assert!(out.contains("runtime-checked") && !out.contains("✓ invariant"), "{name}: {out}");
    }
}

#[test]
fn verify_proves_no_guard_through_a_changed_value() {
    let head = "cell O {\n state o { initial: pending  pending -> validated { guard { GUARD } }  validated -> done }\n";
    let cases = [
        // a field write after the require
        ("guard_field", "amount <= cap.max", " on validate(id: String, amount: Int) { let cap = map(\"max\", 100)  require amount <= cap.max else Big  cap.max = 0  transition(id, \"validated\") }\n"),
        // time passes
        ("guard_time", "now() <= expires", " on validate(id: String, expires: Int) { require now() <= expires else Late  sleep(1)  transition(id, \"validated\") }\n"),
        // NaN passes a negated require
        ("guard_nan", "amount >= 0", " on validate(id: String, x: Float) { let amount = sqrt(x)  require !(amount < 0) else Bad  transition(id, \"validated\") }\n"),
        // a String literal is not an Int literal
        ("guard_type", "code == 5", " on validate(id: String, code: String) { require code == \"5\" else Bad  transition(id, \"validated\") }\n"),
        // two Ints that are one f64
        ("guard_big", "amount > 9007199254740993", " on validate(id: String, amount: Int) { require amount > 9007199254740992 else Small  transition(id, \"validated\") }\n"),
        // an assignment inside an if condition
        ("guard_cond", "amount > 0", " on validate(id: String, amount: Int) { require amount > 0 else Bad  if len(try { amount = 0 - 5 }) >= 0 { print(1) }  transition(id, \"validated\") }\n"),
        // a computed target beside a narrowed caller
        ("guard_dynamic", "amount > 0", " on validate(id: String, amount: Int) { require amount > 0 else Bad  transition(id, \"validated\") }\n on sneaky(id: String, amount: Int, t: String) { transition(id, t) }\n"),
    ];
    for (name, guard, handlers) in cases {
        let src = format!("{}{}}}\n", head.replace("GUARD", guard), handlers);
        let out = verify_out(name, &src);
        assert!(out.contains("runtime-checked") && !out.contains("proven — every caller"), "{name}: {out}");
    }
    // still proven: a plain require on an Int parameter
    let src = format!("{} on validate(id: String, amount: Int) {{ require amount > 0 else Bad  transition(id, \"validated\") }}\n}}\n", head.replace("GUARD", "amount > 0"));
    assert!(verify_out("guard_plain", &src).contains("proven — every caller"));
}

#[test]
fn check_counts_what_it_was_missing() {
    // a think() inside an index was never counted (200 tokens "proved"
    // under a 100 bound); an assignment inside a `require` condition was
    // not a reassignment; delegate's argument count was not checked
    let dir = scratch("check_missing_counts");
    std::fs::write(dir.join("cost.cell"), r#"
cell agent Bot { cost { tokens: 100 }
  on classify(q: String) {
    let summary = think("summarize: {q}", map("max_tokens", 100))
    let labels = list("ham", "spam")
    return labels[to_int(think("0 or 1? {summary}", map("max_tokens", 100)))]
  }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["check", "cost.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("computed 200 tokens > declared 100"), "{out}");
    std::fs::write(dir.join("inv.cell"), "cell A {\n memory { c: Map<String, Int> [persistent]\n  invariant c >= 0 }\n on put(k: String, n: Int) { let v = 5  require len(try { v = n }) > 0 else Never  c.set(k, v) }\n}\n").unwrap();
    let (_, out) = soma(&dir, &["verify", "inv.cell"]);
    assert!(out.contains("runtime-checked") && !out.contains("✓ invariant"), "{out}");
    std::fs::write(dir.join("dl.cell"), "cell A { on h(x: Int, y: Int) { return x + y } }\ncell B { on go() { return delegate(\"A\", \"h\", 1) }\n on ok() { return delegate(\"A\", \"h\", 1, 2) } }\n").unwrap();
    let (code, out) = soma(&dir, &["check", "dl.cell"]);
    assert_ne!(code, 0, "{out}");
    assert_eq!(out.matches("passes 1 argument, but A.h takes 2").count(), 1, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_triple_quoted_string_dedents_around_a_non_breaking_space() {
    // the indentation was measured with Unicode whitespace and sliced in
    // bytes: a pasted U+00A0 panicked the lexer ("not a char boundary")
    let dir = scratch("triple_quote_nbsp");
    std::fs::write(dir.join("app.cell"), "cell P {\n  on run() {\n    let s = \"\"\"\n     ab\n    \u{a0}cd\n    \"\"\"\n    return s == \" ab\\n\u{a0}cd\"\n  }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["run", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.trim(), "true", "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_line_starting_with_a_bracket_or_paren_is_a_new_expression() {
    // `let xs = [10, 20, 30]` then `[i]` on the next line parsed as
    // `[10, 20, 30][i]`, and `let b = a` then `(1 + 2)` as the call a(1 + 2)
    passes("newline_bracket", r#"
cell P {
    on f(i: Int) {
        let xs = [10, 20, 30]
        [i]
    }
    on g() {
        let a = 5
        let b = a
        (1 + 2)
    }
    on h(i: Int) { let xs = [10, 20, 30]  return xs[i] }
}
cell test T { rules {
    assert f(1) == [1]
    assert g() == 3
    assert h(1) == 20
} }
"#);
}

#[test]
fn a_number_touching_percent_is_the_modulo_when_an_operand_follows() {
    // `10%-3` lexed as the percentage 10% then `- 3` (7.0); `10%3` and
    // `10%n` failed to parse
    passes("percent_or_modulo", r#"
cell P {
    on run() { let n = 3  return [10%-3, 10%3, 10%n, 7%2*2] }
}
cell test T { rules {
    assert run() == [1, 1, 1, 2]
} }
"#);
}

#[test]
fn interpolation_reads_quoted_braces_and_semicolons_as_code() {
    // a `}` or a `;` inside a quoted argument printed the source text; a
    // pattern "a}}" did not match the string "a}}"
    passes("interp_quotes", r#"
cell P {
    on parts() { let parts = ["a", "b"]  return "parts: {join(parts, \"}\")}" }
    on get() { let m = map("}", 7)  return "[{m.get(\"}\")}]" }
    on csv() { let xs = ["a", "b"]  return "csv: {join(xs, \";\")}" }
    on css() { return "css: {color: red; margin: 0}" }
    on pat() { let s = "a}}"  return match s { "a}}" -> "matched"  _ -> "fell through" } }
}
cell test T { rules {
    assert parts() == "parts: a}b"
    assert get() == "[7]"
    assert csv() == "csv: a;b"
    assert css() == "css: {color: red; margin: 0}"
    assert pat() == "matched"
} }
"#);
}

#[test]
fn integer_text_is_digits_and_decimals_truncate_exactly() {
    // rug's parser skipped spaces inside the digits ("1 2 3" was 123, also
    // in sum_by), and a decimal string went through f64 (…807.5 became …808)
    passes("int_text", r#"
cell test T { rules {
    assert to_int("1 2 3") == ()
    assert to_int("1_000") == 1000
    assert to_int("1__0") == ()
    assert to_int(" 12.5 ") == 12
    assert to_int("-7.9") == -7
    assert to_int("9223372036854775807.5") == 9223372036854775807
    assert to_int("99999999999999999999.9") == 99999999999999999999
    assert to_int("12345678901234567891e0") == 12345678901234567891
    assert to_int("1.25e3") == 1250
    assert to_float("1_000.5") == 1000.5
    assert_fails sum_by([map("v", "1 2"), map("v", "3")], "v") matching "not a number"
} }
"#);
}

#[test]
fn http_int_parameters_read_digits_and_big_integers() {
    // `?n=1+2` ("1 2") was the Int 12; `?n=100000000000000000000` became a
    // Float and failed an Int parameter, and a String parameter got ".0"
    let dir = scratch("http_int_text");
    std::fs::write(dir.join("app.cell"), "cell A {\n on echo_int(n: Int) { return n + 1 }\n on echo_str(s: String) { return s }\n}\n").unwrap();
    let (_server, port) = serve_dir(&dir);
    assert!(get(port, "/echo_int?n=1+2").starts_with("HTTP/1.0 400"));
    let out = get(port, "/echo_int?n=100000000000000000000");
    assert!(out.contains("100000000000000000001"), "{out}");
    let out = get(port, "/echo_str?s=100000000000000000000");
    assert!(out.ends_with("{\"result\":\"100000000000000000000\"}"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn small_builtin_and_literal_fixes() {
    // round capped digits at 15; to_fixed of an Int went through f64;
    // `-2.abs()` failed to parse; "%08.2f" padded inf with zeros
    passes("small_builtin_fixes", r#"
cell test T { rules {
    assert round(1e-16, 16) == 0.0000000000000001
    assert to_fixed(9007199254740993, 0) == "9007199254740993"
    assert to_fixed(123456789012345678901234567890, 2) == "123456789012345678901234567890.00"
    assert -2.abs() == -2
    assert format("%08.2f", 1.0 / 0.0) == "     inf"
    assert format("%08.2f", 0.0 - 3.5) == "-0003.50"
} }
"#);
    // a float literal past the range is refused; an arity error names
    // the line of each call
    let dir = scratch("literal_and_arity");
    std::fs::write(dir.join("big.cell"), "cell P { on run() { return 1e400 } }\n").unwrap();
    let (code, out) = soma(&dir, &["check", "big.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("past the Float range"), "{out}");
    std::fs::write(dir.join("ar.cell"), "cell P {\n on run() {\n  let a = 1\n  return abs(1, 2)\n }\n on other() { return abs(3, 4) }\n}\n").unwrap();
    let (_, out) = soma(&dir, &["check", "ar.cell"]);
    assert!(out.contains("ar.cell:4:") && out.contains("ar.cell:6:"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn serve_binds_every_body_and_keeps_a_path_plus() {
    // PUT / PATCH / DELETE (and a lowercase `post`) dropped their body, as
    // did every /signal/ route; a text/plain body with `=` was read as a
    // form; `/a+b` reached `request` as "/a b"; the /signal/ 404 was
    // invalid JSON for a URL holding a quote
    let dir = scratch("serve_bodies");
    std::fs::write(dir.join("app.cell"), r#"
cell App {
    memory { docs: Map<String, Map> [persistent] }
    on save(data: Map) { docs.set("last", data)  return data }
    on add(n: Int) { return n + 1 }
    on echo_str(s: String) { return s }
    on request(method: String, path: String, body: String) { return map("path", path) }
}
"#).unwrap();
    let (_server, port) = serve_dir(&dir);
    let send = |method: &str, path: &str, ct: &str, body: &str| raw_request(port, &format!(
        "{method} {path} HTTP/1.0\r\nHost: localhost\r\nContent-Type: {ct}\r\nContent-Length: {}\r\n\r\n{body}", body.len()));
    for m in ["PUT", "PATCH", "DELETE", "post"] {
        let out = send(m, "/save", "application/json", r#"{"title":"hello"}"#);
        assert!(out.ends_with(r#"{"title":"hello"}"#), "{m}: {out}");
    }
    let out = send("POST", "/signal/add", "application/json", r#"{"n":2}"#);
    assert!(out.contains(r#""result": 3"#) || out.contains(r#""result":3"#), "{out}");
    let out = send("POST", "/echo_str", "text/plain", "aGVsbG8=");
    assert!(out.ends_with(r#"{"result":"aGVsbG8="}"#), "{out}");
    let out = get(port, "/a+b");
    assert!(out.contains(r#""/a+b""#), "{out}");
    let out = raw_request(port, "GET /signal/a\"b HTTP/1.0\r\nHost: localhost\r\n\r\n");
    let body = out.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    let json: serde_json::Value = serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {out}"));
    assert_eq!(json["kind"], "not_found", "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn lint_and_fix_advice_keeps_the_program_working() {
    let dir = scratch("lint_fix_advice");
    // `.length` on a map is its key: no alias advice; an if-chain with
    // `else` branches is not a match
    std::fs::write(dir.join("l.cell"), "cell L {\n on f() { let r = map(\"length\", 5, \"w\", 2)  return r.length }\n on score(s: String) {\n  let n = 0\n  if s == \"a\" { n += 1 } else { n += 10 }\n  if s == \"b\" { n += 2 } else { n += 20 }\n  if s == \"c\" { n += 3 } else { n += 30 }\n  return n\n }\n}\n").unwrap();
    let (_, out) = soma(&dir, &["lint", "l.cell"]);
    assert!(!out.contains("'length' is an alias") && !out.contains("if_chain") && !out.contains("consider using match"), "{out}");
    // fix writes stubs of the declared type; check reads a map literal's values
    std::fs::write(dir.join("fx.cell"), "cell type Opt { variants { Some(Int)  Nothing } }\ncell type Only { variants { One(Int) } }\ncell A {\n face {\n  signal a(x: Int) -> Map<String, Int>\n  signal g() -> Opt\n  signal h() -> Only\n }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["fix", "fx.cell"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = soma(&dir, &["run", "fx.cell", "a", "1"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = soma(&dir, &["run", "fx.cell", "g"]);
    assert_eq!(code, 0, "{out}");
    std::fs::write(dir.join("fr.cell"), "cell A {\n face { signal a(x: Int) -> Map<String, Int> }\n on a(x: Int) { return map(\"status\", \"ok\") }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "fr.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("holding a String literal"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_json_keeps_a_rule_with_a_dash_and_says_what_raised() {
    // the record was re-read from the text: `"a — c"` in a rule was cut at
    // its dash, and an assert_fails that did NOT raise said raised: true
    let dir = scratch("test_json_records");
    std::fs::write(dir.join("app.cell"), "cell P {\n on s() { return \"a — b\" }\n on ok() { return 1 }\n}\ncell test T { rules {\n assert s() == \"a — c\"\n assert_fails ok()\n} }\n").unwrap();
    let (_, out) = soma(&dir, &["test", "app.cell", "--json"]);
    let json: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    let results = json["results"].as_array().unwrap();
    assert_eq!(results[0]["rule"], "assert s() == \"a — c\"", "{out}");
    assert_eq!(results[1]["raised"], false, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_map_int_slot_reads_the_same_through_every_spelling() {
    // values were in text order while keys were numeric; keys(s),
    // entries(s), `s |> keys` and `for e in s` gave text keys; a test rule
    // saw text keys
    passes("map_int_every_spelling", r#"
cell Seats {
    memory { seats: Map<Int, String> [persistent] }
    on book(n: Int, who: String) { seats.set(n, who) }
    on pairs() { return zip(seats.keys, seats.values) }
    on spelled() {
        let t = 0
        for e in seats { t = t + e.key }
        return [keys(seats), seats |> keys, entries(seats)[0].key, t, seats.values()]
    }
}
cell test T { rules {
    let a = book(2, "two")
    let b = book(10, "ten")
    assert pairs() == [map("left", 2, "right", "two"), map("left", 10, "right", "ten")]
    assert spelled() == [[2, 10], [2, 10], 2, 12, ["two", "ten"]]
    assert seats.keys == [2, 10]
} }
"#);
}

#[test]
fn core_evaluation_fixes_of_the_second_pass() {
    // an index that calls ran 2-3 times; a `return` in a `let x = if …`
    // branch became x; an outer assignment before a block's own `let` of
    // that name was undone; a negative index failed below the first level;
    // a lambda did not capture a name read by an interpolated pattern;
    // `m[()] = v` wrote the entry "null"
    passes("core_second_pass", r#"
cell App {
    memory { calls: Map<String, Int> [persistent] }
    on _next() { let n = calls.get("n") ?? 0  calls.set("n", n + 1)  return n }
    on nested() {
        calls.set("n", 0)
        let grid = [[10, 11], [20, 21]]
        grid[_next()][1] = 5
        let xs = [100, 200]
        xs[_next()] += 1
        return [grid, xs, calls.get("n")]
    }
    on early(xs: List<Int>) {
        let found = if len(xs) == 0 { return "empty" } else { xs[0] }
        return "first={found}"
    }
    on shadow() {
        let x = 1
        if true { x = 5  let x = 6 }
        let acc = 0
        for i in range(0, 3) { acc = acc + i  let acc = 1000 }
        return [x, acc]
    }
    on negative() { let g = [[1, 2], [3, 4]]  g[0][-1] = 8  return g }
    on captured() { let id = "42"  return ["user-42", "x"] |> map(v => match v { "user-{id}" -> "hit"  _ -> "miss" }) }
    on unit_key(body: Map) { let mm = map()  mm[body.userid] = "alice"  return mm }
    on unit_read() { let mm = map("null", 1)  return [mm[()], mm.get(()), mm.has(())] }
}
cell test T { rules {
    assert nested() == [[[10, 5], [20, 21]], [100, 201], 2]
    assert early([]) == "empty"
    assert early([7]) == "first=7"
    assert shadow() == [5, 3]
    assert negative() == [[1, 8], [3, 4]]
    assert captured() == ["hit", "miss"]
    assert_fails unit_key(map("user_id", "u")) matching "the key is ()"
    assert unit_read() == [(), (), false]
} }
"#);
    // a record pattern beside a declared sum type
    let dir = scratch("record_pattern_with_types");
    std::fs::write(dir.join("app.cell"), "cell type Color { variants { Red  Green } }\ncell App { on main() {\n let g = Game { bet: 10 }\n return match g { Game { bet } -> \"bet {bet}\"  _ -> \"fell through\" }\n} }\n").unwrap();
    let (code, out) = soma(&dir, &["run", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("bet 10"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_handler_passed_as_a_value_is_a_callee() {
    // horde(worker, xs) / vote(worker, q, 1) ran a writing handler on GET,
    // and a handler request owned only through horde() was reachable
    // directly, around request's auth
    let dir = scratch("handler_as_value");
    std::fs::write(dir.join("app.cell"), r#"
cell App {
    memory { bal: Map<String, Int> [persistent] }
    on worker(k: String) [task] { bal.set(k, 0)  return k }
    on wipe_all(ks: String) { return horde(worker, split(ks, ",")) }
    on vt(k: String) { return vote(worker, k, 1) }
    on reset_one(k: String) [task] { bal.set(k, 0)  return k }
    on request(method: String, path: String, body: List, headers: Map) {
        if headers.authorization != "Bearer s3cret" { return response(401, map("error", "auth")) }
        return match path {
            "/admin/reset/" + k -> horde(reset_one, [k])
            "/count" -> len(body)
            _ -> response(404, "nf")
        }
    }
}
"#).unwrap();
    let (_server, port) = serve_dir(&dir);
    assert!(get(port, "/wipe_all/a,b").starts_with("HTTP/1.0 405"));
    assert!(get(port, "/vt/a").starts_with("HTTP/1.0 405"));
    let out = raw_request(port, "POST /reset_one/a HTTP/1.0\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n");
    assert!(out.starts_with("HTTP/1.0 401"), "{out}");
    // an empty body is an empty List for `body: List`
    let out = raw_request(port, "GET /count HTTP/1.0\r\nHost: localhost\r\nAuthorization: Bearer s3cret\r\n\r\n");
    assert!(out.starts_with("HTTP/1.0 200"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn native_second_pass_answers_like_the_interpreter() {
    // the return type came from the first return (a later Float or
    // quotient raised); a Float handler printed its Int return as a Float;
    // abs(q) of a quotient; an i64 quotient past 2^53 rounded twice; a
    // BigInt local handed to a sibling raised
    passes("native_second_pass", r#"
cell P {
    on clamp0(x: Float) [native] { if x < 0.0 { return 0 }  return x }
    on halve(n: Int) [native] { if n == 0 { return 0 }  return n / 2 }
    on absq(n: Int) [native] { let q = n / 2  return abs(q) }
    on ratio(a: Int, b: Int) [native] { return a / b }
    on isev(n: Int) [native] { return n % 2 == 0 }
    on sq_even(n: Int) [native] { let x = n * n  return isev(x) }
}
cell test T { rules {
    assert clamp0(1.5) == 1.5
    assert type_of(clamp0(0.0 - 2.0)) == "Int"
    assert halve(5) == 2.5
    assert halve(4) == 2
    assert absq(8) == 4
    assert type_of(absq(8)) == "Int"
    assert ratio(2880067194370816120, 1779979416004714189) == 1.618033988749895
    assert sq_even(5000000000) == true
} }
"#);
    // refused at check: a tail if that ends in values, String + an Int
    // builtin, mismatched comparisons, to_int of a Bool
    let dir = scratch("native_second_pass_check");
    std::fs::write(dir.join("app.cell"), r#"
cell P {
    on pick(n: Int) [native] { if n > 0 { n * 10 } else { 0 - n } }
    on label(s: String) [native] { return s + str_len(s) }
    on same(b: Bool, n: Int) [native] { return b == n }
    on flag(n: Int) [native] { let b = n > 0  return to_int(b) }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    for h in ["'pick'", "'label'", "'same'", "'flag'"] {
        assert!(out.contains(&format!("handler {h} is marked [native]")), "{h}: {out}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn verify_second_pass_false_proofs() {
    // a require before a loop proved every pass's write; a bare call into
    // another cell was no effect; emitting its own event to another cell's
    // looping or echoing handler terminated; recursion in the base case's
    // condition terminated; http_patch took no time and post's body was read
    // as its options
    let inv_loop = "cell A {\n memory { c: Map<String, Int> [persistent]\n  invariant c >= 0 }\n on go(k: String, n: Int) {\n  require ((c.get(k) ?? 0) - n) >= 0 else \"low\"\n  for i in range(0, 3) { c.set(k, (c.get(k) ?? 0) - n) }\n  return c.get(k)\n }\n}\n";
    assert!(!verify_out("vr_loop", inv_loop).contains("writer 'go' proven"));
    let inv_cell = "cell A {\n memory { c: Map<String, Int> [persistent]\n  d: Map<String, Int> [persistent]\n  invariant c >= 0 }\n on setd(k: String, v: Int) { d.set(k, v)  return v }\n on go(k: String) {\n  require (d.get(k) ?? 0) >= 0 else \"neg\"\n  sabotage(k)\n  c.set(k, d.get(k) ?? 0)\n }\n}\ncell B { on sabotage(k: String) { A.setd(k, -5)  return 0 } }\n";
    assert!(!verify_out("vr_cell", inv_cell).contains("writer 'go' proven"));
    let echo = "cell A {\n state s { initial: a  a -> b }\n on ping(n: Int) { emit ping(n + 1)  return n }\n on go(id: String) { transition(id, \"b\")  return 1 }\n}\ncell B { on ping(n: Int) { emit ping(n)  return n } }\n";
    assert!(verify_out("vr_echo", echo).contains("emits it back") || verify_out("vr_echo", echo).contains("emits back"));
    let rec = "cell A {\n state s { initial: a  a -> b }\n on f(n: Int) { if f(n - 1) > 100 || n <= 0 { return 0 }  return 1 }\n on go(id: String) { transition(id, \"b\")  return 1 }\n}\n";
    assert!(!verify_out("vr_rec", rec).contains("all 2 handlers structurally terminate"));
    let dir = scratch("vr_latency");
    std::fs::write(dir.join("a.cell"), "cell A {\n cost { latency: 1s }\n on go(url: String) { return http_patch(url, map(\"q\", 1), map(\"timeout\", 5000)) }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "a.cell"]);
    assert!(code != 0 && out.contains("computed 5000 ms"), "{out}");
    std::fs::write(dir.join("b.cell"), "cell A {\n cost { latency: 45s }\n on go(url: String) { return http_post(url, map(\"q\", 1), map(\"timeout\", 60000)) }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "b.cell"]);
    assert!(code != 0 && out.contains("computed 60000 ms"), "{out}");
    // emitting its own event is counted once, not twice
    std::fs::write(dir.join("c.cell"), "cell A {\n cost { tokens: 100 }\n on ping(q: String) {\n  let a = think(q, map(\"max_tokens\", 100))\n  emit ping(q)\n  return a\n }\n}\ncell B { on ping(q: String) { print(\"B got {q}\")  return q } }\n").unwrap();
    let (code, out) = soma(&dir, &["check", "c.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_parameter_bound_by_name_keeps_its_position() {
    // `POST /two?b=B` with {"a":"A"} bound B to `a` and the body to `b`
    let dir = scratch("query_named_position");
    std::fs::write(dir.join("app.cell"), "cell App {\n on two(a: String, b: String) { return a + \"|\" + b }\n on pay(to: String, amount: Int, memo: String) { return to + \"|\" + to_string(amount) + \"|\" + memo }\n}\n").unwrap();
    let (_server, port) = serve_dir(&dir);
    let post = |path: &str, body: &str| raw_request(port, &format!("POST {path} HTTP/1.0\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()));
    assert!(post("/two?b=B", r#"{"a":"A"}"#).ends_with(r#"{"result":"A|B"}"#));
    assert!(post("/pay?amount=5", r#"{"to":"bob","memo":"rent"}"#).ends_with(r#"{"result":"bob|5|rent"}"#));
    let out = get(port, "/two?b=B");
    assert!(out.contains("parameter 'a'"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn parser_and_builtin_fixes_of_the_second_pass() {
    passes("builtins_second_pass", r#"
cell P {
    on refund(amount: Int) {
        let r = if amount > 0 {
            let fee = 2
            -amount
        } else { 0 }
        return r
    }
    on spaced(a: Int) {
        let x = a
            - 1
        return x
    }
}
cell test T { rules {
    assert refund(50) == -50
    assert spaced(5) == 4
    assert len(filter_by([map("p", "99"), map("p", "150")], "p", ">", 100)) == 1
    assert len(filter_by([map("s", ()), map("s", 3), map("s", true), map("s", 7)], "s", ">", 5)) == 1
    assert len(filter_by([map("d", "2024-01-02")], "d", ">", "2024-01-01")) == 1
    assert_fails max_by([map("p", "n/a"), map("p", 3)], "p") matching "not a number"
    assert_fails distinct(map("a", 1)) matching "type"
    assert_fails join(5, ",") matching "type"
    assert distinct(()) == []
    assert regex_replace("file_1.txt", "file_(\\d+)", "doc_$1_v2") == "doc_1_v2.txt"
    assert regex_replace("12", "(\\d)(\\d)", "$1a$2") == "1a2"
    assert regex_replace("cost: X", "X", "$5") == "cost: $5"
    assert sqrt(ipow(10, 400)) > 9.9e199
    assert log10(ipow(10, 400)) == 400.0
    assert_fails to_float("1e400") matching "range"
    assert_fails scale([[1, 2]], "3") matching "type"
    assert_fails shape([[1, 2], [3]]) matching "ragged"
} }
"#);
    // a triple-quoted string reads the same in a CRLF file
    let dir = scratch("crlf_triple");
    std::fs::write(dir.join("app.cell"), "cell P {\r\n on run() {\r\n  let s = \"\"\"\r\n  a\r\n    b\r\n  c\r\n  \"\"\"\r\n  return s == \"a\\n  b\\nc\"\r\n }\r\n}\r\n").unwrap();
    let (code, out) = soma(&dir, &["run", "app.cell"]);
    assert_eq!((code, out.trim()), (0, "true"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cli_fixes_of_the_second_pass() {
    let dir = scratch("cli_second_pass");
    // fix edits the slot and the cell it names, not one containing the name
    std::fs::write(dir.join("s.cell"), "cell A {\n    memory {\n        catalog: Map<String, String> [ephemeral, local]\n        log: Map<String, String> [persistent, ephemeral]\n    }\n    on r() { return 1 }\n}\n").unwrap();
    soma(&dir, &["fix", "s.cell"]);
    let fixed = std::fs::read_to_string(dir.join("s.cell")).unwrap();
    assert!(fixed.contains("catalog: Map<String, String> [ephemeral, local]"), "{fixed}");
    assert!(!fixed.contains("[persistent, ephemeral]"), "{fixed}");
    std::fs::write(dir.join("c.cell"), "cell AppStore {\n    on x() { return 1 }\n}\ncell App {\n    face { signal ping() -> Int }\n}\n").unwrap();
    soma(&dir, &["fix", "c.cell"]);
    let fixed = std::fs::read_to_string(dir.join("c.cell")).unwrap();
    let app = fixed.find("cell App {").unwrap();
    assert!(fixed[app..].contains("on ping()") && !fixed[..app].contains("on ping()"), "{fixed}");
    // two handlers of one name; a recursive local lambda
    std::fs::write(dir.join("d.cell"), "cell A {\n on ping() { return 1 }\n on ping() { return 2 }\n on f(n: Int) { let fib = k => if k < 2 { k } else { fib(k - 1) + fib(k - 2) }  return fib(n) }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "d.cell"]);
    assert!(code != 0 && out.contains("duplicate signal 'ping'") && out.contains("the lambda `fib` calls `fib`"), "{out}");
    // verify over a directory; `-0` is the text "-0"
    let sub = dir.join("progs");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("m.cell"), "cell M {\n state s { initial: a  a -> b }\n on go(id: String) { transition(id, \"b\")  return 1 }\n on txt(x: String) { return x }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["verify", "progs"]);
    assert!(code == 0 && out.contains("verify: 1 file(s), 0 failed"), "{out}");
    let (_, out) = soma(&sub, &["run", "m.cell", "txt", "-0"]);
    assert_eq!(out.trim(), "-0", "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn serve_third_pass_binding_and_gates() {
    let dir = scratch("serve_third_pass");
    std::fs::write(dir.join("app.cell"), r#"
cell Store {
    memory { docs: Map<String, Map> [persistent]  bal: Map<String, Int> [persistent] }
    on put_doc(key: String, data: Map) { docs.set(key, data)  return docs.get(key) }
    on put_note(key: String, text: String) { return text }
    on tag(name: String, meta: Map) { return [name, meta] }
    on add(n: Int, opts: Map) { return [n, opts] }
    on hello() { return "hi" }
    on reset_one(k: String) { bal.set(k, 0)  return k }
    on stop(h: String) { return horde_cancel(h) }
    on request(method: String, path: String, body: String, headers: Map) {
        if headers.get("authorization") != "Bearer s3cret" { return response(401, "no") }
        if starts_with(path, "/admin/reset/") { return horde(Store.reset_one, [replace(path, "/admin/reset/", "")]) }
        return response(404, "nf")
    }
}
"#).unwrap();
    let (_server, port) = serve_dir(&dir);
    let send = |m: &str, path: &str, ct: &str, body: &str| raw_request(port, &format!(
        "{m} {path} HTTP/1.0\r\nHost: localhost\r\nContent-Type: {ct}\r\nContent-Length: {}\r\n\r\n{body}", body.len()));
    let j = "application/json";
    // an unknown query key never takes the body's place
    assert!(send("PUT", "/put_doc/k1?v=2", j, r#"{"title":"Q3"}"#).ends_with(r#"{"title":"Q3"}"#));
    assert!(send("PUT", "/put_note/k1?utm_source=mail", "text/plain", "call the bank").ends_with(r#"{"result":"call the bank"}"#));
    // a missing trailing Map is empty; the body's other keys fill it
    assert!(send("POST", "/tag", j, r#"{"name":"x"}"#).ends_with(r#"["x",{}]"#));
    assert!(send("POST", "/add", j, r#"{"n":1,"step":7}"#).ends_with(r#"[1,{"step":7}]"#));
    // a body for a handler with no open parameter is ignored
    assert!(send("POST", "/hello", j, r#"{"x":1}"#).ends_with(r#"{"result":"hi"}"#));
    // `horde(Store.reset_one, …)` makes reset_one request's; horde_cancel writes
    assert!(send("POST", "/reset_one/alice", j, "").starts_with("HTTP/1.0 401"));
    assert!(get(port, "/stop?h=x").starts_with("HTTP/1.0 405"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn fix_edits_the_cell_the_error_names() {
    // a slot of the same name in another cell was edited; a comment naming
    // the cell received the handler stub
    let dir = scratch("fix_scoped");
    std::fs::write(dir.join("a.cell"), "cell A {\n    memory { log: List<String> [ephemeral] }\n    on r() { return 1 }\n}\ncell B {\n    memory { log: List<String> [persistent, ephemeral] }\n    on s() { return 1 }\n}\n").unwrap();
    soma(&dir, &["fix", "a.cell"]);
    let fixed = std::fs::read_to_string(dir.join("a.cell")).unwrap();
    assert!(fixed.contains("memory { log: List<String> [ephemeral] }") && !fixed.contains("[persistent, ephemeral]"), "{fixed}");
    std::fs::write(dir.join("b.cell"), "// Helper is used by cell App below\ncell Helper {\n    on x() { return 1 }\n}\ncell App {\n    face { signal go(n: Int) }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["fix", "b.cell"]);
    assert_eq!(code, 0, "{out}");
    let fixed = std::fs::read_to_string(dir.join("b.cell")).unwrap();
    let app = fixed.find("cell App {").unwrap();
    assert!(fixed[app..].contains("on go(") && !fixed[..app].contains("on go("), "{fixed}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn parser_and_builtins_third_pass() {
    passes("builtins_third_pass", r#"
cell P {
    on g(m: Map, n: Int) {
        let k = m.len
        (n * 2)
    }
    on h(a: Int) {
        let d = (a
          -2)
        return d
    }
    on price(p: Float) { let discount = 5%  return p * (1 - discount) }
}
cell test T { rules {
    assert g(map("a", 1), 5) == 10
    assert h(5) == 3
    assert price(100.0) == 95.0
    assert to_fixed(1.005, 16) == "1.0050000000000000"
    assert regex_replace("price: 10", "(\\d+)", "$1 $USD") == "price: 10 $USD"
    assert regex_replace("2024-05", "(?P<y>\\d{4})-(?P<m>\\d\\d)", "$m/$y_x") == "05/2024_x"
    assert_fails format("%d", 2.5) matching "fraction"
    assert format("%d", 3.0) == "3"
    assert format("%.2e", 2 * ipow(10, 400) - 1) == "2.00e+400"
    assert_fails matrix("1e400 1") matching "range"
} }
"#);
    // a Rust token name never reaches a message
    let dir = scratch("token_names");
    std::fs::write(dir.join("app.cell"), "cell P { on r() { for i in 1..3 { print(i) } } }\n").unwrap();
    let (_, out) = soma(&dir, &["check", "app.cell"]);
    assert!(!out.contains("DotDot") && out.contains("'..'"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn verify_third_pass() {
    let dir = scratch("verify_third_pass");
    // an interpolated delegate cell is refused (it was read as a fixed name)
    std::fs::write(dir.join("d.cell"), "cell App {\n memory { c: Map<String, Int> [persistent]  invariant c >= 0 }\n on drain(k: String) { c.set(k, 0)  return 0 }\n on w(k: String, who: String) { let r = delegate(\"{who}\", \"drain\", k)  return r }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "d.cell"]);
    assert!(code != 0 && out.contains("an interpolated cell name is computed"), "{out}");
    // the last timeout counts; link() makes latency advisory
    std::fs::write(dir.join("t.cell"), "cell A {\n cost { latency: 1s }\n on go(q: String) { return http_get(q, map(\"timeout\", 100, \"timeout\", 60000)) }\n}\n").unwrap();
    let (_, out) = soma(&dir, &["check", "t.cell"]);
    assert!(out.contains("computed 60000 ms"), "{out}");
    std::fs::write(dir.join("l.cell"), "cell A {\n cost { latency: 100ms }\n on go(u: String) { let r = link(u)  return 1 }\n}\n").unwrap();
    let (_, out) = soma(&dir, &["check", "l.cell"]);
    assert!(!out.contains("latency' bound proven"), "{out}");
    // a local named _from proves nothing about the machine's _from
    std::fs::write(dir.join("g.cell"), "cell O {\n state o { initial: pending  pending -> paid  * -> cancelled { guard { _from != \"paid\" } } }\n on pay(id: String) { transition(id, \"paid\") }\n on cancel(id: String) { let _from = \"pending\"  require _from != \"paid\" else nope  transition(id, \"cancelled\") }\n}\n").unwrap();
    let (_, out) = soma(&dir, &["verify", "g.cell"]);
    assert!(!out.contains("proven — every caller"), "{out}");
    // a guard variable bound in the branch that transitions is accepted
    std::fs::write(dir.join("b.cell"), "cell R {\n state s { initial: filed  filed -> approved { guard { reason != \"\" } }  filed -> escalated }\n on review(id: String, amount: Int) {\n  if amount > 100 { transition(id, \"filed\", \"escalated\") }\n  else { let reason = \"auto\"  transition(id, \"filed\", \"approved\") }\n }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "b.cell"]);
    assert_eq!(code, 0, "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn core_third_pass() {
    passes("core_third_pass", r#"
cell App {
    memory { calls: Map<String, Int> [persistent]  seats: Map<Int, String> [persistent] }
    on _next() { let n = calls.get("n") ?? 0  calls.set("n", n + 1)  return n }
    on arm() { let x = 1  match 1 { 1 -> { x = 5  let x = 6 }  _ -> 0 }  return x }
    on tryblk() { let x = 1  let r = try { x = 5  let x = 7  x }  return [x, r.value] }
    on sum() {
        let total = 0
        for i in [1, 2, 3] {
            let d = match i { 2 -> { let total = 100  total }  _ -> 0 }
            total = total + i
            let total = -1
        }
        return total
    }
    on interp_key() {
        calls.set("n", 0)
        let m = map("k0", 10, "k1", 20)
        m["k{_next()}"] += 1
        return [m, calls.get("n")]
    }
    on with_unit(body: Map) { let m = map()  m = with(m, body.userid, 1)  return m }
    on without_unit(body: Map) { let m = map("null", 1)  return [contains(m, body.userid), try { without(m, body.userid) }.kind] }
    on int_keys() { seats.set(1, "a")  return [seats.get("1"), seats.has("1"), try { seats.delete("1") }.kind, seats.len] }
}
cell test T { rules {
    assert arm() == 5
    assert tryblk() == [5, 7]
    assert sum() == 6
    assert interp_key() == [map("k0", 11, "k1", 20), 1]
    assert_fails with_unit(map("user_id", "u")) matching "the key is ()"
    assert without_unit(map("user_id", "u")) == [false, "type"]
    assert int_keys() == [(), false, "type", 1]
} }
"#);
}

#[test]
fn native_third_pass() {
    passes("native_third_pass", r#"
cell P {
    on sq(n: Int) [native] { return n * n }
    on c(n: Int, x: Float) [native] { let s = 1  for i in range(0, n) { s = s * 3 }  return s + sq(x) }
    on h(n: Int, x: Float) [native] { if x < 0.0 { return n }  return x }
    on g(x: Float) [native] { let t = 0  t = to_int(x * 100.0)  return t }
    on qb(n: Int) [native] { let q = n / 2  if n > 100 { q = q + 0.5 }  return q }
    on fl(n: Int) [native] { let q = n / 2  return q * 1000000000000 * 1.5 }
    on cmp(n: Int) [native] { let q = n / 2  if q * 1000000000000 > 1000000000000000 { return 1 }  return 0 }
    on isq(x: Float) [native] { return sq(x) }
}
cell test T { rules {
    assert_fails c(50, 2.5) matching "Int parameter got the Float"
    assert_fails isq(3.0) matching "Int parameter got the Float"
    assert h(5, 0.0 - 1.0) == 5
    assert_fails h(100000000000000001, 0.0 - 1.0) matching "2^53"
    assert g(2.5) == 250
    assert qb(4) == 2
    assert type_of(qb(4)) == "Int"
    assert qb(200) == 100.5
    assert fl(40000) == 30000000000000000.0
    assert_fails cmp(40000) matching "2^53"
} }
"#);
    let dir = scratch("native_third_pass_check");
    std::fs::write(dir.join("app.cell"), "cell P {\n on pos(n: Int) [native] { return n > 0 }\n on st(n: Int) [native] { return to_string(n) }\n on a(n: Int) [native] { return pos(n) + 1 }\n on b(n: Int) [native] { return st(n) + n }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert!(code != 0 && out.contains("handler 'a' is marked [native]") && out.contains("handler 'b' is marked [native]"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn native_recursion_and_exact_quotients_after_the_type_rework() {
    // regressions of the third pass: a recursive Int handler read its own
    // placeholder return type as Float; an exact quotient to an Int
    // parameter was refused; abs/min/max/to_int of an exact Int hid a loss
    // of digits
    passes("native_fourth_pass", r#"
cell P {
    on fact(n: Int) [native] { if n <= 1 { return 1 }  return n * fact(n - 1) }
    on f(n: Int) [native] { if n <= 1 { return 1 }  return g(n - 1) * n }
    on g(n: Int) [native] { if n <= 1 { return 1 }  return f(n - 1) * n }
    on even(n: Int) [native] { if n == 0 { return true }  return odd(n - 1) }
    on odd(n: Int) [native] { if n == 0 { return false }  return even(n - 1) }
    on sq(x: Int) [native] { return x * x }
    on a(n: Int) [native] { return sq(n / 2) }
    on m(x: Int, y: Int) [native] { let q = x / y  return abs(q * q) % 1000 }
}
cell test T { rules {
    assert fact(25) == 15511210043330985984000000
    assert f(30) == 265252859812191058636308480000000
    assert even(10) == true
    assert a(8) == 16
    assert_fails a(7) matching "not exact"
    assert m(1000, 10) == 0
    assert_fails m(100000001, 1) matching "2^53"
} }
"#);
}

#[test]
fn serve_fourth_pass() {
    let dir = scratch("serve_fourth_pass");
    std::fs::write(dir.join("app.cell"), r#"
cell App {
    memory { bal: Map<String, Int> [persistent] }
    on page(t: String) { return with(html("<b>hi</b>"), "_content_type", t) }
    on go(to: String) { return redirect(to) }
    on wipe(k: String) { bal.set(k, 0)  return "wiped" }
    on request(method: String, path: String, body: String) {
        if path == "/admin" && body == "secret" { let who = "App"  return delegate(who, "wipe", "a") }
        return response(404, "nf")
    }
}
"#).unwrap();
    let (_server, port) = serve_dir(&dir);
    let out = get(port, "/page?t=text%2Fhtml%0D%0ASet-Cookie%3A%20pwn%3D1");
    assert!(!out.to_ascii_lowercase().contains("set-cookie"), "{out}");
    assert!(get(port, "/page?t=text%2Fhtml%C3%A9").starts_with("HTTP/1.0 200"));
    let out = get(port, "/go?to=%2Fcaf%C3%A9");
    assert!(out.contains("Location: /caf%C3%A9"), "{out}");
    // `delegate(who, "wipe", …)` makes wipe request's
    let out = raw_request(port, "POST /wipe/a HTTP/1.0\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n");
    assert!(!out.contains("wiped"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
    // request(method, path) takes no body
    let dir = scratch("serve_request_two");
    std::fs::write(dir.join("app.cell"), "cell App { on request(method: String, path: String) { return path } }\n").unwrap();
    let (_server, port) = serve_dir(&dir);
    assert!(get(port, "/x").contains("/x"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn fix_fourth_pass() {
    let dir = scratch("fix_fourth_pass");
    // an idiom edit before the error no longer shifts which slot is edited
    std::fs::write(dir.join("a.cell"), "cell Z {\n    on z() { let a = None\n        let b = None\n        return 1 }\n}\ncell A {\n    memory { log: List<String> [persistent, ephemeral] }\n    on r() { return 1 }\n}\ncell B {\n    memory { log: List<String> [ephemeral, local] }\n    on s() { return 1 }\n}\n").unwrap();
    soma(&dir, &["fix", "a.cell"]);
    let fixed = std::fs::read_to_string(dir.join("a.cell")).unwrap();
    assert!(fixed.contains("log: List<String> [ephemeral, local]") && !fixed.contains("[persistent, ephemeral]"), "{fixed}");
    // two handlers of one name are not "fixed" by deleting a face line
    std::fs::write(dir.join("d.cell"), "cell B {\n    face { signal ping(x: Int) -> Int }\n    on ping(x: Int) { return 1 }\n    on ping(x: Int) { return 2 }\n}\n").unwrap();
    soma(&dir, &["fix", "d.cell"]);
    let fixed = std::fs::read_to_string(dir.join("d.cell")).unwrap();
    assert!(fixed.contains("signal ping(x: Int) -> Int"), "{fixed}");
    // a brace in a comment or a triple-quoted string is not the cell's end
    std::fs::write(dir.join("c.cell"), "cell H {\n    on x() { return \"\"\"see \"cell App\" docs\"\"\" }\n}\ncell App {\n    face { signal go(n: Int) }\n    on y() {\n        // closes early: }\n        return 1\n    }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["fix", "c.cell"]);
    assert_eq!(code, 0, "{out}");
    let fixed = std::fs::read_to_string(dir.join("c.cell")).unwrap();
    let app = fixed.find("cell App {").unwrap();
    assert!(fixed[app..].contains("on go(") && !fixed[..app].contains("on go("), "{fixed}");
    let _ = std::fs::remove_dir_all(dir);
}
