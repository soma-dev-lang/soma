//! A panic inside a [native] handler must surface as an ordinary Soma
//! runtime error — never abort the process (it used to: SIGABRT on `1 / 0`).

use std::process::Command;

#[test]
fn native_division_by_zero_is_a_catchable_error() {
    let dir = std::env::temp_dir().join("soma_native_guard");
    let _ = std::fs::create_dir_all(&dir);
    let cell = dir.join("g.cell");
    std::fs::write(&cell, r#"
cell G {
    face {
        signal div(a: Int, b: Int) -> Int
        signal safe(a: Int, b: Int) -> Int
    }
    on div(a: Int, b: Int) [native] {
        return a / b
    }
    on safe(a: Int, b: Int) {
        let r = try { div(a, b) }
        if r.error != () { return -1 }
        return r.value
    }
}
"#).unwrap();
    let run = |sig: &str, a: &str, b: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["run", cell.to_str().unwrap(), "--signal", sig, a, b])
            .current_dir(&dir)
            .output()
            .expect("failed to run soma");
        (
            format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
            o.status.code(),
        )
    };

    let (out, code) = run("div", "1", "0");
    assert_eq!(code, Some(1), "must exit with a runtime error, not a signal: {out}");
    assert!(out.contains("division by zero"), "got: {out}");

    let (out, code) = run("safe", "1", "0");
    assert_eq!(code, Some(0), "try must catch the native error: {out}");
    assert!(out.lines().any(|l| l.trim() == "-1"), "got: {out}");

    // the error buffer is cleared: a later good call is unaffected
    let (out, code) = run("div", "8", "2");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.lines().any(|l| l.trim() == "4"), "got: {out}");
}

/// idiv() is the backend-independent integer division: same result in
/// the interpreter and in [native] code, negatives and BigInts included.
#[test]
fn idiv_agrees_across_backends() {
    let dir = std::env::temp_dir().join("soma_native_idiv");
    let _ = std::fs::create_dir_all(&dir);
    let cell = dir.join("i.cell");
    std::fs::write(&cell, r#"
cell I {
    face {
        signal q_i(a: Int, b: Int) -> Int
        signal q_n(a: Int, b: Int) -> Int
        signal big_i(a: Int) -> Int
        signal big_n(a: Int) -> Int
    }
    on q_i(a: Int, b: Int) { return idiv(a, b) }
    on q_n(a: Int, b: Int) [native] { return idiv(a, b) }
    on big_i(a: Int) {
        let x = a * 1000
        return idiv(x, 7)
    }
    on big_n(a: Int) [native] {
        let x = a * 1000
        return idiv(x, 7)
    }
}
"#).unwrap();
    let run = |sig: &str, args: &[&str]| {
        let mut full = vec!["run", cell.to_str().unwrap(), "--signal", sig];
        full.extend_from_slice(args);
        let o = Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(&full)
            .current_dir(&dir)
            .output()
            .expect("failed to run soma");
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.starts_with("[native]"))
            .last()
            .unwrap_or("")
            .trim()
            .to_string()
    };
    // negative CLI arguments must be accepted as values
    for (a, b, want) in [("7", "2", "3"), ("-7", "2", "-3"), ("7", "-2", "-3")] {
        assert_eq!(run("q_i", &[a, b]), want, "interpreter idiv({a}, {b})");
        assert_eq!(run("q_n", &[a, b]), want, "native idiv({a}, {b})");
    }
    let want = "1317624576693539401000"; // i64::MAX * 1000 / 7
    assert_eq!(run("big_i", &["9223372036854775807"]), want, "interpreter BigInt idiv");
    assert_eq!(run("big_n", &["9223372036854775807"]), want, "native BigInt idiv");
}

/// A cached dylib must prove it was built from the expected source
/// (`_soma_build_id`). A wrong library under the right name — what
/// concurrent builds used to produce — is rejected and rebuilt, instead of
/// silently answering with another program's code.
#[test]
fn poisoned_native_cache_entry_is_rejected_and_rebuilt() {
    let dir = std::env::temp_dir().join("soma_native_poison");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let program = |cell: &str, k: i64| {
        format!(
            "cell {cell} {{\n    face {{ signal f(n: Int) -> Int }}\n    on f(n: Int) [native] {{\n        return n + {k}\n    }}\n}}\n"
        )
    };
    std::fs::write(dir.join("a.cell"), program("A", 1)).unwrap();
    std::fs::write(dir.join("b.cell"), program("B", 1000)).unwrap();
    let run = |file: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["run", file, "--signal", "f", "1"])
            .current_dir(&dir)
            .output()
            .expect("failed to run soma");
        (
            String::from_utf8_lossy(&o.stdout).trim().to_string(),
            String::from_utf8_lossy(&o.stderr).to_string(),
        )
    };
    assert_eq!(run("a.cell").0, "2");
    assert_eq!(run("b.cell").0, "1001");

    // poison A's entry with B's library (both export `handler_f`)
    let cache = dir.join(".soma_cache/native");
    let find = |prefix: &str| {
        std::fs::read_dir(&cache)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| {
                let n = p.file_name().unwrap().to_string_lossy().to_string();
                n.starts_with(prefix) && (n.ends_with(".dylib") || n.ends_with(".so"))
            })
            .expect("cached dylib")
    };
    let (a, b) = (find("native_a_"), find("native_b_"));
    std::fs::remove_file(&a).unwrap();
    std::fs::copy(&b, &a).unwrap();

    let (out, err) = run("a.cell");
    assert!(err.contains("rejected"), "poisoned entry must be detected: {err}");
    assert_eq!(out, "2", "and rebuilt — never B's answer (1001): {err}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `/` on two Ints means the same thing in [native] code as in the
/// interpreter: 7 / 2 = 3.5, an exact quotient is an Int (BigInt-exact),
/// and only a slot that can hold nothing but an Int refuses a non-exact
/// quotient — loudly, never by truncating to 3.
#[test]
fn native_int_division_matches_the_interpreter() {
    let dir = std::env::temp_dir().join("soma_native_intdiv");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bodies = [
        ("ret", "return a / b"),
        ("avg", "let s = a + b\n        return s / 2"),
        ("mixed", "return a / b + 1"),
        ("slot", "let n = a\n        n = n / b\n        return n"),
        ("big", "let x = a * a * a * a\n        x = x / b\n        return x"),
    ];
    let mut src = String::from("cell D {\n    face {\n");
    for (n, _) in bodies {
        src += &format!("        signal {n}_i(a: Int, b: Int) -> Float\n        signal {n}_n(a: Int, b: Int) -> Float\n");
    }
    src += "    }\n";
    for (n, b) in bodies {
        src += &format!("    on {n}_i(a: Int, b: Int) {{\n        {b}\n    }}\n    on {n}_n(a: Int, b: Int) [native] {{\n        {b}\n    }}\n");
    }
    src += "}\n";
    std::fs::write(dir.join("d.cell"), src).unwrap();
    let run = |sig: &str, a: &str, b: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["run", "d.cell", "--signal", sig, a, b])
            .current_dir(&dir)
            .output()
            .expect("failed to run soma");
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
            .lines()
            .filter(|l| !l.starts_with("[native]"))
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    };
    for (sig, a, b, want) in [
        ("ret", "7", "2", "3.5"),
        ("ret", "6", "2", "3"),
        ("ret", "-7", "2", "-3.5"),
        ("avg", "3", "4", "3.5"),
        ("avg", "4", "4", "4"),
        ("mixed", "7", "2", "4.5"),
        ("mixed", "6", "2", "4"),
        ("slot", "6", "2", "3"),
        ("big", "1000000", "1000", "1000000000000000000000"),
    ] {
        assert_eq!(run(&format!("{sig}_i"), a, b), want, "interpreter {sig}({a}, {b})");
        assert_eq!(run(&format!("{sig}_n"), a, b), want, "native {sig}({a}, {b})");
    }
    // an Int-only slot cannot hold 3.5: runtime error, not a silent 3
    let out = run("slot_n", "7", "2");
    assert!(out.contains("not exact"), "got: {out}");
    assert!(out.contains("idiv"), "the error must name the fix: {out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn native_build_is_not_redirected_by_the_callers_cargo_target_directory() {
    let dir = std::env::temp_dir().join(format!("soma_native_target_{}",std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A per-test-process source forces a fresh cache entry.
    let n = std::process::id();
    std::fs::write(dir.join("app.cell"),format!(r#"
cell N {{
 face {{ signal calculate(n: Int) -> Int }}
 on calculate(n: Int) [native] {{ return n + {n} }}
}}
cell test T {{ rules {{ assert calculate(7) == {} }} }}
"#,n+7)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["test","app.cell"]).current_dir(&dir)
        .env("CARGO_TARGET_DIR",dir.join("external-target"))
        .output().unwrap();
    assert!(out.status.success(),"{}{}",String::from_utf8_lossy(&out.stdout),String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(dir);
}

/// `/` stays the exact Int when the divisor is a `%` expression, and the
/// Int-only builtins (band, gcd, pow_mod, bit_len…) refuse a Float like the
/// interpreter does instead of truncating it (bit_len(2.5) answered 2.0,
/// pow_mod(2.5, 2, 7) answered 4.0). An exact Int / Int quotient reaching
/// one of them is checked at run time; a BigInt `/` in an Int slot is no
/// longer an "internal" refusal.
#[test]
fn native_int_builtins_refuse_floats_and_keep_exact_quotients() {
    let dir = std::env::temp_dir().join("soma_native_int_args");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bodies = [
        ("modq", "return b / (c % 3)"),
        ("gcdq", "return b / (gcd(c, 9) % 2)"),
        ("remq", "return (a / 2) % 3"),
        ("bandq", "let x = a / 2\n        return band(x, 1)"),
        ("bigexp", "return sin(a) + pow_mod(2, b / c, 7)"),
    ];
    let mut src = String::from("cell D {\n    face {\n");
    for (n, _) in bodies {
        src += &format!("        signal {n}_i(a: Int, b: Int, c: Int) -> Float\n        signal {n}_n(a: Int, b: Int, c: Int) -> Float\n");
    }
    src += "    }\n";
    for (n, b) in bodies {
        src += &format!("    on {n}_i(a: Int, b: Int, c: Int) {{\n        {b}\n    }}\n    on {n}_n(a: Int, b: Int, c: Int) [native] {{\n        {b}\n    }}\n");
    }
    src += "}\n";
    std::fs::write(dir.join("d.cell"), src).unwrap();
    let run = |sig: &str, a: &str, b: &str, c: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["run", "d.cell", "--signal", sig, a, b, c])
            .current_dir(&dir)
            .output()
            .expect("failed to run soma");
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
            .lines()
            .filter(|l| !l.starts_with("[native]"))
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    };
    for (sig, a, b, c, want) in [
        ("modq", "1", "63", "4", "63"),
        ("gcdq", "1", "63", "4", "63"),
        ("remq", "6", "0", "0", "0"),
        ("remq", "7", "0", "0", "0.5"),
        ("bandq", "6", "0", "0", "1"),
        ("bigexp", "3", "6", "3", "4.141120008059867"),
    ] {
        assert_eq!(run(&format!("{sig}_i"), a, b, c), want, "interpreter {sig}({a}, {b}, {c})");
        assert_eq!(run(&format!("{sig}_n"), a, b, c), want, "native {sig}({a}, {b}, {c})");
    }
    // 7 / 2 is 3.5: band() raises in both backends, never band(3, 1)
    for sig in ["bandq_i", "bandq_n"] {
        let out = run(sig, "7", "0", "0");
        assert!(out.contains("band(): expected Int arguments, got Float 3.5"), "{sig}: {out}");
    }
    // a non-exact quotient in pow_mod's exponent slot is an error, not a value
    let out = run("bigexp_n", "3", "6", "4");
    assert!(out.contains("not exact") && out.contains("idiv"), "got: {out}");
    assert!(!out.contains("internal"), "got: {out}");

    // a Float literal or parameter in an Int-only builtin is refused statically
    std::fs::write(dir.join("f.cell"), r#"
cell F {
    face {
        signal lit(a: Int) -> Float
        signal par(x: Float) -> Float
    }
    on lit(a: Int) [native] { return sin(a) * 0.0 + pow_mod(2.5, 2, 7) }
    on par(x: Float) [native] { return sin(x) * 0.0 + bit_len(x) }
}
"#).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["check", "f.cell"])
        .current_dir(&dir)
        .output()
        .expect("failed to run soma");
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert_ne!(o.status.code(), Some(0), "check must refuse: {out}");
    assert!(out.contains("pow_mod() takes Ints, got the Float `2.5`"), "got: {out}");
    assert!(out.contains("bit_len() takes Ints, got the Float `x`"), "got: {out}");
    assert!(!out.contains("error[E0"), "no raw rustc output: {out}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A huge literal times a small local (`9223372036854775807 * i`) overflowed
/// i64 in the fast path AND in the BigInt fallback, which multiplied two
/// plain i64 operands: the handler answered a `range` error where the
/// interpreter promotes to BigInt.
#[test]
fn native_bigint_fallback_promotes_huge_literal_times_small_variable() {
    let dir = std::env::temp_dir().join("soma_native_huge_literal");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bodies = [
        ("mul", "let i = 11\n        return 9223372036854775807 * i"),
        ("mulr", "let i = 11\n        return i * 4611686018427387904"),
        ("add", "let i = 11\n        return 9223372036854775807 + i"),
        ("sub", "let i = 11\n        return i - 9223372036854775807"),
        ("bits", "let i = 11\n        return bit_len(9223372036854775807 * i)"),
        ("param", "return a * 9223372036854775807"),
    ];
    let mut src = String::from("cell D {\n    face {\n");
    for (n, _) in bodies {
        src += &format!("        signal {n}_i(a: Int) -> Int\n        signal {n}_n(a: Int) -> Int\n");
    }
    src += "    }\n";
    for (n, b) in bodies {
        src += &format!("    on {n}_i(a: Int) {{\n        {b}\n    }}\n    on {n}_n(a: Int) [native] {{\n        {b}\n    }}\n");
    }
    src += "}\n";
    std::fs::write(dir.join("d.cell"), src).unwrap();
    let run = |sig: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["run", "d.cell", "--signal", sig, "3"])
            .current_dir(&dir)
            .output()
            .expect("failed to run soma");
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
            .lines()
            .filter(|l| !l.starts_with("[native]"))
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    };
    for (sig, want) in [
        ("mul", "101457092405402533877"),
        ("mulr", "50728546202701266944"),
        ("add", "9223372036854775818"),
        ("sub", "-9223372036854775796"),
        ("bits", "67"),
        ("param", "27670116110564327421"),
    ] {
        assert_eq!(run(&format!("{sig}_i")), want, "interpreter {sig}");
        assert_eq!(run(&format!("{sig}_n")), want, "native {sig}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `buffer(-3)` allocated `(-3) as usize` elements and died with Rust's
/// "capacity overflow"; it is a `range` error naming the argument.
#[test]
fn native_buffer_refuses_a_negative_size_with_a_range_error() {
    let dir = std::env::temp_dir().join("soma_native_buffer_neg");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("b.cell"), r#"
cell B {
    face {
        signal mk(n: Int) -> Int
        signal mkf(n: Int) -> Float
    }
    on mk(n: Int) [native] { let b = buffer(n)
        return n }
    on mkf(n: Int) [native] { let b = buffer_f(n)
        buf_set_f(b, 0, 1.5)
        return buf_get_f(b, 0) }
}
"#).unwrap();
    let run = |sig: &str, n: &str| {
        let o = Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["run", "b.cell", "--signal", sig, n])
            .current_dir(&dir)
            .output()
            .expect("failed to run soma");
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
    };
    assert!(run("mk", "0").lines().any(|l| l.trim() == "0"), "buffer(0) is fine");
    assert!(run("mkf", "2").lines().any(|l| l.trim() == "1.5"));
    for sig in ["mk", "mkf"] {
        let out = run(sig, "-3");
        assert!(out.contains("buffer(n): n is a number of elements, 0 or more") && out.contains("-3"), "{sig}: {out}");
        assert!(!out.contains("capacity overflow"), "{sig}: {out}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `to_string(n / 2)` printed "2.0" natively where the interpreter prints
/// the exact quotient as an Int ("2"); an inexact one stays "3.5". The
/// inexact-division flag no longer leaks from one native call into the next.
/// `str_at` out of range raises the interpreter's message and kind, not
/// Rust's index panic, and native error texts carry no `kind: ` prefix.
#[test]
fn native_to_string_of_exact_quotient_and_str_at_match_the_interpreter() {
    let dir = std::env::temp_dir().join("soma_native_tostring_q");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("q.cell"), r#"
cell Q {
    face {
        signal show(n: Int) -> String
        signal show_n(n: Int) -> String
        signal at(s: String, i: Int) -> Int
        signal at_n(s: String, i: Int) -> Int
    }
    on show(n: Int) { return to_string(n / 2) + "|" + to_string(7 / 2) + "|" + to_string(n / 2) }
    on show_n(n: Int) [native] { return to_string(n / 2) + "|" + to_string(7 / 2) + "|" + to_string(n / 2) }
    on at(s: String, i: Int) { return str_at(s, i) }
    on at_n(s: String, i: Int) [native] { return str_at(s, i) }
    on both(n: Int, s: String, i: Int) {
        let a = try { show(n) }
        let b = try { show_n(n) }
        let c = try { at(s, i) }
        let d = try { at_n(s, i) }
        return [a.value == b.value, c.error == d.error, c.kind == d.kind, a.value, c.error]
    }
}
"#).unwrap();
    let run = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(["run", "q.cell", "--signal", "both"])
            .args(args)
            .current_dir(&dir)
            .output()
            .expect("failed to run soma");
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
            .lines()
            .filter(|l| !l.starts_with("[native]"))
            .last()
            .unwrap_or("")
            .trim()
            .to_string()
    };
    // exact quotient first, inexact, exact again — each call decides alone
    for (n, s, i) in [("4", "héllo", "9"), ("7", "héllo", "-1"), ("0", "", "0"), ("4", "abc", "1")] {
        let out = run(&[n, s, i]);
        assert!(out.starts_with("[true, true, true, "), "n={n} s={s} i={i}: {out}");
    }
    assert!(run(&["4", "abc", "1"]).contains("\"2|3.5|2\""));
    assert!(run(&["4", "abc", "9"]).contains("str_at: index 9 out of range for a string of 3 bytes"));
    let _ = std::fs::remove_dir_all(&dir);
}
