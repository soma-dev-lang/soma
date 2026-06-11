// Track C: fix-it diagnostics, assert_fails, test isolation, lint canonicalization.

use std::process::Command;

/// Run the soma binary built for this test profile (debug or release).
fn soma(args: &[&str]) -> (String, String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run soma");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

/// Write a temp .cell file and return its path as a String.
fn temp_cell(name: &str, source: &str) -> String {
    let path = std::env::temp_dir().join(format!("soma_track_c_{}.cell", name));
    std::fs::write(&path, source).expect("failed to write temp cell");
    path.display().to_string()
}

// ── C1: parser fix-it messages contain their own correction ─────────

#[test]
fn test_fixit_handler_return_type_via_check() {
    let path = temp_cell("fixit_a", "cell C { on add(a: Int) -> Int { return a } }");
    let (_, err, code) = soma(&["check", &path]);
    assert_ne!(code, 0);
    assert!(
        err.contains("handlers do not declare return types — put '-> Int' on the signal declaration inside face { }, then write: on add(a: Int) { ... }"),
        "got: {}", err
    );
}

#[test]
fn test_fixit_match_fat_arrow_via_check() {
    let path = temp_cell(
        "fixit_b",
        r#"cell C { on f(x: Int) { return match x { 1 => "one" _ -> "many" } } }"#,
    );
    let (_, err, code) = soma(&["check", &path]);
    assert_ne!(code, 0);
    assert!(err.contains("match arms use '->', not '=>'"), "got: {}", err);
}

#[test]
fn test_fixit_initial_equals_via_check() {
    let path = temp_cell(
        "fixit_c",
        "cell C { state doc { initial = draft\n draft -> published } }",
    );
    let (_, err, code) = soma(&["check", &path]);
    assert_ne!(code, 0);
    assert!(
        err.contains("state machines declare the start state as 'initial: draft' (colon, not equals)"),
        "got: {}", err
    );
}

#[test]
fn test_fixit_guard_when_via_check() {
    let path = temp_cell(
        "fixit_d",
        "cell C { state doc { initial: draft\n draft -> published when ready { } } }",
    );
    let (_, err, code) = soma(&["check", &path]);
    assert_ne!(code, 0);
    assert!(
        err.contains("guards are declared inside the transition block: draft -> published { guard { cond } }"),
        "got: {}", err
    );
}

// ── C2a: assert_fails pins negative paths ────────────────────────────

const MACHINE_FIXTURE: &str = r#"
cell Orders {
    face {
        signal noop() -> Int
    }
    memory {
        meta: Map<String, Int> [persistent, consistent]
    }
    state order {
        initial: Placed
        Placed -> Accepted
        Accepted -> Delivered
    }
    on noop() { return 1 }
}
"#;

#[test]
fn test_assert_fails_passes_on_runtime_error() {
    let source = format!(
        "{}\ncell test T {{ rules {{ assert_fails transition(\"1\", \"Delivered\") }} }}",
        MACHINE_FIXTURE
    );
    let (out, _, code) = soma(&["test", &temp_cell("af_pass", &source)]);
    assert_eq!(code, 0, "out: {}", out);
    assert!(out.contains("✓ assert_fails transition(\"1\", \"Delivered\")"), "out: {}", out);
    assert!(out.contains("1 passed, 0 failed"), "out: {}", out);
}

#[test]
fn test_assert_fails_fails_when_expr_succeeds() {
    // Placed -> Accepted is a VALID transition, so it succeeds and the
    // negative assertion must fail.
    let source = format!(
        "{}\ncell test T {{ rules {{ assert_fails transition(\"1\", \"Accepted\") }} }}",
        MACHINE_FIXTURE
    );
    let (out, _, code) = soma(&["test", &temp_cell("af_fail", &source)]);
    assert_eq!(code, 1, "out: {}", out);
    assert!(out.contains("✗ assert_fails"), "out: {}", out);
    assert!(out.contains("expected a runtime error"), "out: {}", out);
}

// ── C2b/C2c: delivery lifecycle tests + isolation between runs ──────

#[test]
fn test_delivery_app_tests_pass() {
    let (out, _, code) = soma(&["test", "../delivery/app.cell"]);
    assert_eq!(code, 0, "out: {}", out);
    assert!(out.contains("19 tests: 19 passed, 0 failed"), "out: {}", out);
    assert!(out.contains("✓ assert_fails transition(\"1\", \"Delivered\")"), "out: {}", out);
}

#[test]
fn test_runs_are_isolated_from_persistent_state() {
    // LifecycleTests asserts order_count() == 1 after placing exactly one
    // order. If `soma test` leaked state into the [persistent] slots,
    // the second run would see two orders and fail.
    let (out1, _, code1) = soma(&["test", "../delivery/app.cell"]);
    assert_eq!(code1, 0, "first run: {}", out1);
    let (out2, _, code2) = soma(&["test", "../delivery/app.cell"]);
    assert_eq!(code2, 0, "second run must start clean: {}", out2);
    assert!(out2.contains("19 tests: 19 passed, 0 failed"), "out: {}", out2);
}

// ── Verify: cyclic (reactive) machines are warnings, not failures ───

#[test]
fn test_verify_cyclic_machine_is_reactive_warning() {
    // No terminal states at all → reactive system, verify must pass.
    let path = temp_cell(
        "verify_cyclic",
        "cell C { state duty { initial: idle\n idle -> assigned\n assigned -> idle } }",
    );
    let (out, err, code) = soma(&["verify", &path]);
    assert_eq!(code, 0, "out: {} err: {}", out, err);
    assert!(
        out.contains("reactive/cyclic system"),
        "expected reactive warning, out: {}", out
    );
}

#[test]
fn test_verify_real_liveness_violation_still_fails() {
    // A terminal state exists (done), but the b <-> c cycle can never
    // reach it — that is a genuine liveness failure.
    let path = temp_cell(
        "verify_stuck",
        "cell C { state m { initial: a\n a -> done\n a -> b\n b -> c\n c -> b } }",
    );
    let (out, err, code) = soma(&["verify", &path]);
    assert_eq!(code, 1, "out: {} err: {}", out, err);
    assert!(out.contains("liveness violation"), "out: {}", out);
}

// ── C3: canonical alias lints ────────────────────────────────────────

#[test]
fn test_lint_flags_alias_spellings() {
    let path = temp_cell(
        "lint_alias",
        r#"
cell C {
    memory { items: List<String> [persistent] }
    on add(x: String) {
        items.append(x)
        let n = items.length
        return ln(2.0) + n
    }
}
"#,
    );
    let (out, _, code) = soma(&["lint", &path]);
    assert_eq!(code, 0, "lint must not hard-fail: {}", out);
    assert!(
        out.contains("'append' is an alias of 'push' — use 'push' (one canonical spelling keeps generated code consistent)"),
        "out: {}", out
    );
    assert!(out.contains("'ln' is an alias of 'log' — use 'log'"), "out: {}", out);
    assert!(out.contains("'length' is an alias of 'len' — use 'len'"), "out: {}", out);
}

#[test]
fn test_lint_alias_is_suggestion_not_check_error() {
    // The same source must pass `soma check` — canonicalization is a
    // lint suggestion, never a check error.
    let path = temp_cell(
        "lint_alias_check",
        r#"
cell C {
    memory { items: List<String> [persistent] }
    on add(x: String) {
        items.append(x)
        return items.length
    }
}
"#,
    );
    let (out, err, code) = soma(&["check", &path]);
    assert_eq!(code, 0, "out: {} err: {}", out, err);
}
