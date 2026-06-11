//! V1.7 static checks: interpolation check, dispatch resolution and
//! the cross-machine composition lint. These close the gap between
//! what `soma check`/`soma verify` accept and what the runtime does.

use std::process::Command;

fn soma(args: &[&str]) -> (String, String, i32) {
    let output = Command::new("./target/debug/soma")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run soma");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, stderr, code)
}

fn check_src(name: &str, src: &str) -> (String, i32) {
    let tmp = std::env::temp_dir().join(name);
    std::fs::write(&tmp, src).unwrap();
    let (out, _, code) = soma(&["check", tmp.to_str().unwrap()]);
    (out, code)
}

// ── A1: static interpolation check ──────────────────────────────────

#[test]
fn interpolation_undefined_var_fails_check() {
    let (out, code) = check_src(
        "v17_interp_undef.cell",
        r#"
        cell Greeter {
            face { signal greet(customer: String) -> String }
            on greet(customer: String) {
                return "hello {customr}"
            }
        }
        "#,
    );
    assert_eq!(code, 1, "expected check failure, got: {out}");
    assert!(
        out.contains("string interpolation references undefined variable 'customr'"),
        "missing interpolation error: {out}"
    );
    assert!(
        out.contains("did you mean 'customer'?"),
        "missing did-you-mean suggestion: {out}"
    );
    assert!(
        out.contains("{{customr}}"),
        "missing escape correction: {out}"
    );
}

#[test]
fn interpolation_known_names_pass() {
    // Params, lets, loop vars, lambda params, match bindings, memory
    // slots, builtins and escaped braces must all stay silent.
    let (out, code) = check_src(
        "v17_interp_ok.cell",
        r#"
        cell Ok {
            face { signal go(user: String) -> String }
            memory { counter: Int [persistent] }
            on go(user: String) {
                let greeting = "hi {user}"
                let total = 0
                for item in list(1, 2, 3) {
                    let total = total + item
                    print("item {item} total {total}")
                }
                let f = x => "lambda sees {x}"
                print("escaped {{not_a_var}} and css {color: red;} ok")
                print("expr {1 + 2} and call {len(user)}")
                return "done {greeting}"
            }
        }
        "#,
    );
    assert_eq!(code, 0, "expected clean check, got: {out}");
}

#[test]
fn interpolation_use_before_definition_fails() {
    let (out, code) = check_src(
        "v17_interp_order.cell",
        r#"
        cell Order {
            face { signal go() -> String }
            on go() {
                let early = "value is {late}"
                let late = 42
                return early
            }
        }
        "#,
    );
    assert_eq!(code, 1, "use-before-definition should fail: {out}");
    assert!(out.contains("undefined variable 'late'"), "got: {out}");
}

#[test]
fn interpolation_nonparsing_segment_is_literal() {
    // Segments that don't parse as expressions render literally at
    // runtime, so they must not be errors.
    let (out, code) = check_src(
        "v17_interp_literal.cell",
        r#"
        cell Lit {
            face { signal go() -> String }
            on go() {
                return "json-ish {) not an expr} and {a,b,c} stay literal"
            }
        }
        "#,
    );
    assert_eq!(code, 0, "literal segments must pass: {out}");
}

#[test]
fn interpolation_broken_delivery_copy_fails_with_suggestion() {
    // The headline scenario: one {customer} typo'd to {customr} in the
    // Mesa delivery app must fail check with a suggestion.
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../delivery/app.cell"),
    )
    .expect("delivery/app.cell missing");
    assert!(src.contains("{customer}"), "fixture drifted: no {{customer}} in app.cell");
    let broken = src.replacen("{customer}", "{customr}", 1);
    let tmp = std::env::temp_dir().join("v17_mesa_broken.cell");
    std::fs::write(&tmp, broken).unwrap();
    let (out, _, code) = soma(&["check", tmp.to_str().unwrap()]);
    assert_eq!(code, 1, "broken Mesa copy must fail check: {out}");
    assert!(
        out.contains("undefined variable 'customr'") && out.contains("did you mean"),
        "missing suggestion: {out}"
    );

    // And the pristine app still checks clean.
    let (out_ok, _, code_ok) = soma(&["check", "../delivery/app.cell"]);
    assert_eq!(code_ok, 0, "pristine delivery/app.cell must pass: {out_ok}");
}

// ── A2: static dispatch resolution ──────────────────────────────────

#[test]
fn dispatch_unknown_function_fails_check() {
    let (out, code) = check_src(
        "v17_dispatch_unknown.cell",
        r#"
        cell P {
            face { signal go() -> Int }
            on go() { return unknown_fn(3) }
        }
        "#,
    );
    assert_eq!(code, 1, "unknown function must fail: {out}");
    assert!(out.contains("undefined function 'unknown_fn'"), "got: {out}");
}

#[test]
fn dispatch_ambiguous_call_fails_check() {
    let (out, code) = check_src(
        "v17_dispatch_ambig.cell",
        r#"
        cell A {
            face { signal foo() -> Int }
            on foo() { return 1 }
        }
        cell B {
            face { signal foo() -> Int }
            on foo() { return 2 }
        }
        cell C {
            face { signal go() -> Int }
            on go() { return foo() }
        }
        "#,
    );
    assert_eq!(code, 1, "ambiguous call must fail: {out}");
    assert!(
        out.contains("ambiguous call 'foo': defined by cells A and B"),
        "got: {out}"
    );
    assert!(out.contains("rename one handler"), "got: {out}");
}

#[test]
fn dispatch_recursive_shadow_warns_but_passes() {
    let (out, code) = check_src(
        "v17_dispatch_shadow.cell",
        r#"
        cell D {
            face { signal foo(n: Int) -> Int }
            on foo(n: Int) {
                if n <= 0 { return 0 }
                return foo(n - 1)
            }
        }
        cell Y {
            face { signal foo() -> Int }
            on foo() { return 7 }
        }
        "#,
    );
    assert_eq!(code, 0, "shadowed recursion is a warning, not an error: {out}");
    assert!(
        out.contains("recursive self-call") && out.contains("Y also defines 'foo'"),
        "missing shadow warning: {out}"
    );
}

#[test]
fn dispatch_clean_recursion_and_lambdas_stay_silent() {
    let (out, code) = check_src(
        "v17_dispatch_silent.cell",
        r#"
        cell F {
            face { signal fact(n: Int) -> Int }
            on fact(n: Int) {
                if n <= 1 { return 1 }
                let g = x => x + 1
                let bump = g(0)
                return n * fact(n - 1) + bump - 1
            }
        }
        "#,
    );
    assert_eq!(code, 0, "got: {out}");
    assert!(
        !out.contains("warning: call to"),
        "single-definer recursion must not warn: {out}"
    );
}

// ── A3: cross-machine composition lint (soma verify) ────────────────

#[test]
fn composition_warns_on_delivery_pickup_order() {
    let (out, _, code) = soma(&["verify", "../delivery/app.cell"]);
    assert_eq!(code, 0, "composition is warning-only; verify must pass");
    assert!(
        out.contains(
            "composition: 'pickup_order' transitions machine 'order' and then calls 'assign' (cell Couriers) which can fail"
        ),
        "missing composition warning: {out}"
    );
    assert!(
        out.contains("Pre-check the callee's guard or compensate on failure"),
        "missing correction text: {out}"
    );
}

#[test]
fn composition_warning_in_verify_json() {
    let (out, _, code) = soma(&["verify", "../delivery/app.cell", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).expect("verify --json must be JSON");
    let machines = v["state_machines"].as_array().unwrap();
    let comp = machines
        .iter()
        .find(|m| m["name"] == "Orders/composition")
        .expect("Orders/composition section missing from --json");
    let found = comp["checks"].as_array().unwrap().iter().any(|c| {
        c["status"] == "warning"
            && c["message"]
                .as_str()
                .unwrap_or("")
                .contains("'pickup_order' transitions machine 'order'")
    });
    assert!(found, "composition warning missing from JSON: {out}");
}

#[test]
fn composition_silent_without_cross_cell_call() {
    let tmp = std::env::temp_dir().join("v17_comp_silent.cell");
    std::fs::write(
        &tmp,
        r#"
        cell Solo {
            face { signal advance(id: String) -> String }
            state flow {
                initial: start
                start -> middle
                middle -> done
            }
            on advance(id: String) {
                let r = try { transition(id, "middle") }
                if r.error != () {
                    return "blocked"
                }
                return "advanced"
            }
        }
        "#,
    )
    .unwrap();
    let (out, _, code) = soma(&["verify", tmp.to_str().unwrap()]);
    assert_eq!(code, 0, "got: {out}");
    assert!(
        !out.contains("which can fail"),
        "no composition warning expected for same-cell transitions: {out}"
    );
}
