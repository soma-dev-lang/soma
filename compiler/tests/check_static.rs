//! V1.7 static checks: interpolation check, dispatch resolution and
//! the cross-machine composition lint. These close the gap between
//! what `soma check`/`soma verify` accept and what the runtime does.

use std::process::Command;

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
            memory { counter: Map<String, Int> [persistent] }
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

// ── Silent footguns promoted to diagnostics ─────────────────────────

#[test]
fn adjacent_string_literals_after_return_fail_check() {
    let (out, code) = check_src(
        "dead_adjacent_strings.cell",
        r#"
        cell G {
            face { signal hello() -> String }
            on hello() {
                return "hello " "world"
            }
        }
        "#,
    );
    assert_ne!(code, 0, "half the string is silently dropped — must fail: {out}");
    assert!(out.contains("adjacent string literals do not concatenate"), "got: {out}");
}

#[test]
fn unreachable_code_after_return_warns_but_passes() {
    let (out, code) = check_src(
        "dead_unreachable.cell",
        r#"
        cell G {
            face { signal f() -> Int }
            on f() {
                return 1
                let x = 2
            }
        }
        "#,
    );
    assert_eq!(code, 0, "unreachable code is a warning, not an error: {out}");
    assert!(out.contains("unreachable code after `return`"), "got: {out}");
}

/// Resolution rule: `f(args)` is the program's handler `f` when one takes
/// that many arguments, the builtin otherwise. User code shadows the
/// library, so a handler named after a builtin is called like any other.
#[test]
fn a_handler_shadows_a_builtin_of_the_same_arity() {
    let tmp = std::env::temp_dir().join("dispatch_handler_wins.cell");
    std::fs::write(&tmp, r#"
        cell G {
            face {
                signal merge(a: Int, b: Int) -> Int
                signal use_it() -> Int
                signal list() -> List
            }
            on merge(a: Int, b: Int) { return a + b + 1000 }
            on use_it() { return merge(1, 2) }
            on list() { return list(1, 2) }
        }
        cell test GT {
            rules {
                assert use_it() == 1003
                assert merge(1, 2) == 1003
                assert list() == [1, 2]
            }
        }
    "#).unwrap();
    let (out, _, code) = soma(&["check", tmp.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("BUILTIN"), "matching arity goes to the handler, silently: {out}");
    let (out, _, code) = soma(&["test", tmp.to_str().unwrap()]);
    assert_eq!(code, 0, "handler must win when the argument count matches: {out}");
}

#[test]
fn a_mismatching_argument_count_falls_to_the_builtin_with_a_warning() {
    let (out, code) = check_src(
        "dispatch_builtin_arity.cell",
        r#"
        cell G {
            face {
                signal merge(a: Int, b: Int, c: Int) -> Int
                signal use_it() -> Map
            }
            on merge(a: Int, b: Int, c: Int) { return a + b + c }
            on use_it() { return merge(map("a", 1), map("b", 2)) }
        }
        "#,
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("resolves to the BUILTIN merge()"), "got: {out}");
}

#[test]
fn a_same_arity_self_call_under_a_builtin_name_warns() {
    let (out, code) = check_src(
        "dispatch_self_call.cell",
        r#"
        cell G {
            face { signal list() -> List }
            on list() {
                let items = list()
                return items
            }
        }
        "#,
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("calls the handler ITSELF"), "got: {out}");
}

#[test]
fn native_int_division_warns_and_idiv_is_silent() {
    let (out, code) = check_src(
        "native_int_div.cell",
        r#"
        cell N {
            face {
                signal mid(lo: Int, hi: Int) -> Int
                signal mid_ok(lo: Int, hi: Int) -> Int
                signal avg(a: Float, b: Float) -> Float
            }
            on mid(lo: Int, hi: Int) [native] { return (lo + hi) / 2 }
            on mid_ok(lo: Int, hi: Int) [native] { return idiv(lo + hi, 2) }
            on avg(a: Float, b: Float) [native] { return (a + b) / 2.0 }
        }
        "#,
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("in N.mid [native]"), "Int / Int must warn: {out}");
    assert!(!out.contains("N.mid_ok"), "idiv must not warn: {out}");
    assert!(!out.contains("N.avg"), "Float division must not warn: {out}");
}

#[test]
fn non_exhaustive_match_inside_a_lambda_fails_check() {
    let (out, code) = check_src(
        "sum_lambda_match.cell",
        r#"
        cell type Pay { variants { Charged { tx: String }  Declined { reason: String }  Cash } }
        cell M {
            face { signal go() -> String }
            on go() {
                let f = x => match x {
                    Charged { tx } -> "paid"
                    Cash -> "cash"
                }
                return f(Cash)
            }
        }
        "#,
    );
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("missing variant `Declined`"), "got: {out}");
}

const SPENDER: &str = r#"
        cell agent Spender {
            face {
                signal one(x: String) -> String
                signal HANDLER(items: List) -> Int
            }
            cost {
                tokens: 100000
            }
            state flow {
                initial: idle
                idle -> done
            }
            on one(x: String) {
                return think("s {x}", map("max_tokens", 500, "timeout", 5000))
            }
            on HANDLER(items: List) {
                BODY
                return len(items)
            }
        }
"#;

/// A token bound is only "proven" when every think() runs a known number
/// of times. Through a sibling call in a loop over a list, or inside a
/// lambda, the count is unknown: the bound must be advisory.
#[test]
fn cost_bound_is_not_proven_through_helpers_loops_or_lambdas() {
    for (name, body) in [
        ("via_helper", "for i in items { one(i) }"),
        ("via_lambda", r#"let r = map(items, x => think("s {x}", map("max_tokens", 500)))"#),
    ] {
        let src = SPENDER.replace("HANDLER", name).replace("BODY", body);
        let (out, _) = check_src(&format!("cost_{name}.cell"), &src);
        assert!(out.contains("bound is advisory"), "{name}: {out}");
        assert!(!out.contains("'tokens' bound proven"), "{name} must not be proven: {out}");
    }
    // a literal range IS a known count: 3 x 500 tokens, proven
    let src = SPENDER
        .replace("HANDLER", "via_range")
        .replace("BODY", r#"for i in range(0, 3) { one("x") }"#);
    let (out, code) = check_src("cost_via_range.cell", &src);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("'tokens' bound proven — peak 1500 tokens"), "got: {out}");
}

#[test]
fn fix_native_idiv_rewrites_integer_divisions_only() {
    let tmp = std::env::temp_dir().join("fix_native_idiv.cell");
    // the em dash makes byte offsets ≠ character offsets
    std::fs::write(&tmp, r#"// migration test — spans count characters
cell F {
    face {
        signal a(lo: Int, hi: Int) -> Int
        signal b(x: Float) -> Float
        signal c(lo: Int, hi: Int) -> Int
    }
    on a(lo: Int, hi: Int) [native] {
        let mid = (lo + hi) / 2
        let tri = hi * (hi + 1) / 2
        let q = hi / (lo + 1) / 3
        let w = ((2 * hi + 1) * lo + 3 * (hi - 1)) / (hi + 2)
        return mid + tri + q + w
    }
    on b(x: Float) [native] { return x / 2.0 }
    on c(lo: Int, hi: Int) { return (lo + hi) / 2 }
}
"#).unwrap();
    let (out, _, code) = soma(&["fix", tmp.to_str().unwrap(), "--native-idiv"]);
    assert_eq!(code, 0, "{out}");
    let fixed = std::fs::read_to_string(&tmp).unwrap();
    assert!(fixed.contains("let mid = idiv(lo + hi, 2)"), "{fixed}");
    assert!(fixed.contains("let tri = idiv(hi * (hi + 1), 2)"), "{fixed}");
    assert!(fixed.contains("let q = idiv(idiv(hi, lo + 1), 3)"), "{fixed}");
    assert!(fixed.contains("let w = idiv((2 * hi + 1) * lo + 3 * (hi - 1), hi + 2)"), "{fixed}");
    assert!(fixed.contains("return x / 2.0"), "Float division must stay: {fixed}");
    assert!(fixed.contains("on c(lo: Int, hi: Int) { return (lo + hi) / 2 }"), "interpreted handler must stay: {fixed}");
    let (out, _, code) = soma(&["check", tmp.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("is a Float (7 / 2"), "nothing left to warn about: {out}");
}
