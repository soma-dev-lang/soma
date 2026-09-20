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
