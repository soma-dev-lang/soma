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

// ── Factorial ────────────────────────────────────────────────────────

#[test]
fn test_fact_5() {
    let (out, _, code) = soma(&["run", "../examples/fact.cell", "5"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "120");
}

#[test]
fn test_fact_20() {
    let (out, _, code) = soma(&["run", "../examples/fact.cell", "20"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "2432902008176640000");
}

#[test]
fn test_fact_auto_promotes_bigint() {
    // 30! overflows i64 but auto-promotes to BigInt
    let (out, _, code) = soma(&["run", "../examples/fact.cell", "30"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "265252859812191058636308480000000");
}

#[test]
fn test_fact_bigint() {
    let (out, _, code) = soma(&["run", "../examples/fact_big.cell", "30"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "265252859812191058636308480000000");
}

#[test]
fn test_fact_bigint_100() {
    let (out, _, code) = soma(&["run", "../examples/fact_big.cell", "100"]);
    assert_eq!(code, 0);
    assert!(out.trim().starts_with("933262154439441"));
    assert!(out.trim().len() > 100); // 100! has 158 digits
}

// ── Quantum-inspired linalg builtins (Tang et al.) ───────────────────

#[test]
fn test_linalg_qi_rebalancer_budget_proven() {
    // The headline test: soma check on a cell that uses the linalg
    // builtins with declared bounds must emit "budget proven".
    let (out, _, code) = soma(&["check", "../examples/qi_rebalancer.cell"]);
    assert_eq!(code, 0, "soma check failed: {out}");
    assert!(
        out.contains("budget proven for cell 'QiOptimizer'"),
        "no budget proof in output: {out}"
    );
    // The proof must include the breakdown line.
    assert!(out.contains("breakdown"), "no breakdown: {out}");
}

#[test]
fn test_linalg_qi_rebalancer_runs() {
    let (out, _, code) = soma(&["run", "../examples/qi_rebalancer.cell"]);
    assert_eq!(code, 0, "stdout = {out}");
    assert!(out.contains("Factor loadings"));
    assert!(out.contains("Factor weights"));
    assert!(out.contains("iterations = 10000"));
    // RMT covariance cleaning section.
    assert!(out.contains("RMT-cleaned covariance"));
    assert!(out.contains("raw eigs"));
    assert!(out.contains("cleaned eigs"));
}

// ── Sum types ────────────────────────────────────────────────────────

#[test]
fn test_sum_types_example_runs() {
    let (out, _, code) = soma(&["run", "../examples/sum_types.cell"]);
    assert_eq!(code, 0, "stdout = {out}");
    assert!(out.contains("ok tx-100 $100"));
    assert!(out.contains("rejected (400): non-positive amount"));
    assert!(out.contains("up 3"));
    assert!(out.contains("idle"));
    assert!(out.contains("voided"));
}

#[test]
fn test_sum_types_example_checks() {
    let (out, _, code) = soma(&["check", "../examples/sum_types.cell"]);
    assert_eq!(code, 0, "soma check failed: {out}");
    assert!(out.contains("All checks passed"));
}

#[test]
fn test_sum_types_non_exhaustive_match_fails_check() {
    // Build a temporary cell with a non-exhaustive match and verify
    // soma check rejects it.
    let dir = std::env::temp_dir().join("soma_sum_nonex_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("nonex.cell");
    std::fs::write(&path, r#"
cell type Status {
    variants {
        A
        B
        C
    }
}
cell Demo {
    on classify() {
        let v = A
        return match v {
            A -> 1
            B -> 2
        }
    }
    on run() { classify() }
}
"#).unwrap();
    let (out, _, code) = soma(&["check", path.to_str().unwrap()]);
    assert_ne!(code, 0, "expected non-exhaustive match to fail check: {out}");
    assert!(
        out.contains("non-exhaustive") && out.contains("`C`"),
        "missing variant message: {out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_sum_types_wildcard_makes_match_exhaustive() {
    let dir = std::env::temp_dir().join("soma_sum_wild_test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("wild.cell");
    std::fs::write(&path, r#"
cell type Status {
    variants {
        A
        B
        C
        D
    }
}
cell Demo {
    on classify() {
        let v = B
        return match v {
            A -> "a"
            _ -> "other"
        }
    }
    on run() { print(classify()) }
}
"#).unwrap();
    let (out, _, code) = soma(&["check", path.to_str().unwrap()]);
    assert_eq!(code, 0, "wildcard should make match exhaustive: {out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_typed_state_machine_passes_when_variants_are_correct() {
    let dir = std::env::temp_dir().join("soma_typed_state_ok");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("ok.cell");
    std::fs::write(&path, r#"
cell type OrderState {
    variants {
        Pending
        Validated
        Filled
        Cancelled
    }
}
cell Engine {
    state order: OrderState {
        initial: Pending
        Pending -> Validated
        Validated -> Filled
        * -> Cancelled
    }
    on advance(id: String) {
        transition(id, Validated)
    }
    on run() { advance("x") }
}
"#).unwrap();
    let (out, _, code) = soma(&["check", path.to_str().unwrap()]);
    assert_eq!(code, 0, "typed state machine should pass check: {out}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_typed_state_machine_rejects_unknown_variant() {
    let dir = std::env::temp_dir().join("soma_typed_state_bad");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("bad.cell");
    std::fs::write(&path, r#"
cell type OrderState {
    variants {
        Pending
        Validated
        Filled
        Cancelled
    }
}
cell Engine {
    state order: OrderState {
        initial: Pending
        Pending -> Validated
        Validated -> Filled
        * -> Cancelled
    }
    on advance(id: String) {
        transition(id, Shipped)
    }
    on run() { advance("x") }
}
"#).unwrap();
    let (out, _, code) = soma(&["check", path.to_str().unwrap()]);
    assert_ne!(code, 0, "expected typed state machine to reject bad target: {out}");
    assert!(
        out.contains("'Shipped'") && out.contains("OrderState"),
        "error must name both the bad variant and the state type: {out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_mft_still_checks_after_impact_gate() {
    // After wiring the Bouchaud impact gate into mft/lib/risk.cell,
    // the engine must still pass `soma check`.  (Note: `soma verify`
    // returns non-zero on the mft engine because the cell has known
    // liveness violations by design — it's a long-running server.)
    let (out_c, _, code_c) = soma(&["check", "../mft/app.cell"]);
    assert_eq!(code_c, 0, "soma check failed: {out_c}");
    // Temporal property count comes from verify, which prints to stderr.
    // We just confirm the engine still compiles cleanly.
    assert!(out_c.contains("no errors") || out_c.contains("All checks passed"));
}

#[test]
fn test_risk_check_budget_proven() {
    let (out, _, code) = soma(&["check", "../examples/risk_check.cell"]);
    assert_eq!(code, 0, "stdout = {out}");
    assert!(
        out.contains("budget proven for cell 'RiskCheck'"),
        "no budget proof: {out}"
    );
}

#[test]
fn test_risk_check_runs_and_rejects() {
    let (out, _, code) = soma(&["run", "../examples/risk_check.cell"]);
    assert_eq!(code, 0, "stdout = {out}");
    // Order 1 must succeed.
    assert!(out.contains("\"ok\": true"), "order 1 didn't pass: {out}");
    assert!(out.contains("estimated_bps"), "no bps in output: {out}");
    // Order 2 must be rejected by the ensure precondition.
    assert!(out.contains("REJECTED"), "order 2 wasn't rejected: {out}");
    // VaR metrics must be reported.
    assert!(out.contains("VaR  95% (hist)"));
    assert!(out.contains("ES   95% (hist)"));
}

#[test]
fn test_linalg_qi_regression_runs() {
    let (out, _, code) = soma(&["run", "../examples/quantum_inspired_regression.cell"]);
    assert_eq!(code, 0, "stdout = {out}");
    // svd_lowrank should produce two meaningful singular values.
    assert!(out.contains("sigma_1"));
    assert!(out.contains("sigma_2"));
    // regress_sgd should produce a finite residual that the example
    // prints; we don't pin a numerical value (stochastic), but we
    // check the headline is reached.
    assert!(out.contains("||Ax - b||"), "no regression line: {out}");
    // Sampled rows come from importance_sample_rows.
    assert!(out.contains("sampled rows"));
}

// ── Checker ──────────────────────────────────────────────────────────

#[test]
fn test_check_counter() {
    let (out, _, code) = soma(&["check", "../examples/counter.cell"]);
    assert_eq!(code, 0);
    assert!(out.contains("All checks passed"));
}

#[test]
fn test_check_bad_properties() {
    let (_, _, code) = soma(&["check", "../examples/bad_properties.cell"]);
    assert_ne!(code, 0); // should fail
}

#[test]
fn test_check_promises() {
    let (_, _, code) = soma(&["check", "../examples/promises.cell"]);
    assert_ne!(code, 0); // BrokenVault violates all_encrypted
}

// ── Imports ──────────────────────────────────────────────────────────

#[test]
fn test_app_with_imports() {
    // Clean up any stale data
    let _ = std::fs::remove_dir_all("../.soma_data");
    let (out, _, code) = soma(&["run", "../examples/app.cell", "5"]);
    let _ = std::fs::remove_dir_all("../.soma_data");
    assert_eq!(code, 0);
    assert!(out.contains("35")); // square(5) + double(5) = 25 + 10
}

// ── Store ────────────────────────────────────────────────────────────

#[test]
fn test_store_put_get() {
    let _ = std::fs::remove_dir_all("../.soma_data");
    let (out, _, code) = soma(&["run", "../examples/store.cell", "put", "testkey", "testval"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "testval");

    let (out, _, code) = soma(&["run", "../examples/store.cell", "get", "testkey"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "testval");

    let _ = std::fs::remove_dir_all("../.soma_data");
}

// ── Props ────────────────────────────────────────────────────────────

#[test]
fn test_props_lists_all() {
    let (out, _, code) = soma(&["props"]);
    assert_eq!(code, 0);
    assert!(out.contains("persistent"));
    assert!(out.contains("ephemeral"));
    assert!(out.contains("sqlite"));
    assert!(out.contains("Builtins"));
    assert!(out.contains("print"));
    assert!(out.contains("render"));
}

// ── New features: modulo, while, assignment ──────────────────────────

#[test]
fn test_modulo() {
    let _ = std::fs::write("/tmp/test_mod.cell", r#"
        cell T { on run(n: Int) { return n % 3 } }
    "#);
    let (out, _, code) = soma(&["run", "/tmp/test_mod.cell", "10"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "1"); // 10 % 3 = 1
}

#[test]
fn test_while_loop() {
    let _ = std::fs::write("/tmp/test_while.cell", r#"
        cell T {
            on run(n: Int) {
                let sum = 0
                let i = 1
                while i <= n {
                    sum = sum + i
                    i = i + 1
                }
                return sum
            }
        }
    "#);
    let (out, _, code) = soma(&["run", "/tmp/test_while.cell", "10"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "55"); // 1+2+...+10 = 55
}

#[test]
fn test_assignment() {
    let _ = std::fs::write("/tmp/test_assign.cell", r#"
        cell T {
            on run() {
                let x = 1
                x = x + 10
                x = x * 2
                return x
            }
        }
    "#);
    let (out, _, code) = soma(&["run", "/tmp/test_assign.cell"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "22"); // (1+10)*2
}

// ── Fix command ─────────────────────────────────────────────────────

#[test]
fn test_fix_missing_handler() {
    let tmp = std::env::temp_dir().join("test_fix_missing_handler.cell");
    // A cell that declares a signal in face but has no handler for it
    std::fs::write(&tmp, r#"
        cell Broken {
            face {
                signal greet(name: String) -> String
            }
        }
    "#).unwrap();

    // Check should fail (missing handler)
    let (_, _, code) = soma(&["check", tmp.to_str().unwrap()]);
    assert_ne!(code, 0);

    // Fix should auto-generate the missing handler
    let (_, _, fix_code) = soma(&["fix", tmp.to_str().unwrap()]);
    assert_eq!(fix_code, 0);

    // After fix, check should pass
    let (out, _, code) = soma(&["check", tmp.to_str().unwrap()]);
    assert_eq!(code, 0, "check should pass after fix, got: {}", out);
    assert!(out.contains("All checks passed"));

    let _ = std::fs::remove_file(&tmp);
}

// ── Lint command ────────────────────────────────────────────────────

#[test]
fn test_lint_redundant_to_json() {
    let tmp = std::env::temp_dir().join("test_lint_redundant.cell");
    std::fs::write(&tmp, r#"
        cell Store {
            memory {
                items: Map<String, String> [persistent]
            }
            on save(id: String) {
                let data = map("name", "alice")
                items.set(id, to_json(data))
            }
        }
    "#).unwrap();

    let (out, _, code) = soma(&["lint", "--json", tmp.to_str().unwrap()]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&out)
        .expect("lint --json should produce valid JSON");
    let lints = parsed["lints"].as_array().expect("should have lints array");
    let found = lints.iter().any(|l| {
        l["rule"].as_str().unwrap_or("").contains("redundant_to_json")
    });
    assert!(found, "should detect redundant to_json lint, got: {}", out);

    let _ = std::fs::remove_file(&tmp);
}

// ── Describe command ────────────────────────────────────────────────

#[test]
fn test_describe_agent_cell() {
    let tmp = std::env::temp_dir().join("test_describe_agent.cell");
    std::fs::write(&tmp, r#"
        cell agent Helper {
            face {
                signal run(query: String) -> String
                tool search(q: String) -> String "Search the web"
            }
            memory {
                log: Map<String, String> [ephemeral]
            }
            on run(query: String) {
                return "done"
            }
        }
    "#).unwrap();

    let (out, _, code) = soma(&["describe", tmp.to_str().unwrap()]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&out)
        .expect("describe should produce valid JSON");
    // Agent cells should have kind: "agent"
    let cells = parsed["cells"].as_array().expect("describe returns object with cells array");
    assert!(!cells.is_empty());
    let cell = &cells[0];
    assert_eq!(cell["kind"].as_str(), Some("agent"), "agent cell should have kind=agent");
    // Should have tools in face
    let tools = &cell["face"]["tools"];
    assert!(tools.is_array(), "agent face should have tools array");
    assert!(!tools.as_array().unwrap().is_empty(), "tools should not be empty");

    let _ = std::fs::remove_file(&tmp);
}

// ── Check --json ────────────────────────────────────────────────────

#[test]
fn test_check_json_has_fix_field() {
    let tmp = std::env::temp_dir().join("test_check_json_fix.cell");
    std::fs::write(&tmp, r#"
        cell Broken {
            face {
                signal greet(name: String) -> String
            }
        }
    "#).unwrap();

    let (out, _, code) = soma(&["check", "--json", tmp.to_str().unwrap()]);
    assert_ne!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&out)
        .expect("check --json should produce valid JSON");
    let errors = parsed["errors"].as_array().expect("should have errors array");
    assert!(!errors.is_empty());
    for err in errors {
        assert!(err.get("fix").is_some(), "error should have 'fix' field: {:?}", err);
        assert!(err.get("kind").is_some(), "error should have 'kind' field: {:?}", err);
    }

    let _ = std::fs::remove_file(&tmp);
}

// ── Verify command ──────────────────────────────────────────────────

#[test]
fn test_verify_agent_state_machine() {
    let dir = std::env::temp_dir().join("test_verify_agent");
    let _ = std::fs::create_dir_all(&dir);

    let cell_path = dir.join("agent.cell");
    std::fs::write(&cell_path, r#"
        cell agent Searcher {
            face {
                signal research(topic: String) -> String
                tool search(query: String) -> String "Search the web"
            }
            memory {
                findings: Map<String, String> [persistent]
            }
            state workflow {
                initial: idle
                idle -> researching
                researching -> done
                * -> failed
            }
            on research(topic: String) {
                return "researched"
            }
        }
    "#).unwrap();

    let toml_path = dir.join("soma.toml");
    std::fs::write(&toml_path, r#"
        [package]
        name = "test-verify"
        version = "0.1.0"
    "#).unwrap();

    let (_, _, code) = soma(&["verify", cell_path.to_str().unwrap()]);
    assert_eq!(code, 0, "verify should pass for well-formed agent state machine");

    let _ = std::fs::remove_dir_all(&dir);
}

// ── Refinement (V1.3) ────────────────────────────────────────────────
//
// `soma verify` now proves that handler bodies don't lie to the state
// machine they live next to. These tests pin the three core checks:
//
//   1. correct case → exit 0, "✓ refinement: handler X ⟶ {…}" emitted
//   2. undeclared transition target → exit 1, error message names handler
//   3. dead transition (declared but unreached) → exit 0, warning emitted

#[test]
fn test_refinement_correct_case_passes() {
    let (out, _, code) = soma(&["verify", "../examples/refinement/01_payment_correct.cell"]);
    assert_eq!(code, 0, "01_payment_correct.cell must verify cleanly");
    assert!(out.contains("refinement: handler `authorize` ⟶"),
        "expected per-handler effect summary in output: {}", out);
    assert!(out.contains("authorized"));
    assert!(out.contains("captured"));
    assert!(out.contains("settled"));
    assert!(out.contains("refunded"));
}

#[test]
fn test_refinement_undeclared_target_fails() {
    let (out, _, code) = soma(&["verify", "../examples/refinement/02_payment_undeclared_target.cell"]);
    assert_eq!(code, 1, "02_payment_undeclared_target.cell must fail verify (exit 1)");
    assert!(out.contains("\"completed\""),
        "error message must name the offending target literal: {}", out);
    assert!(out.contains("settle"),
        "error message must name the offending handler: {}", out);
    assert!(out.contains("refinement"),
        "failure must be tagged as a refinement check: {}", out);
}

#[test]
fn test_refinement_dead_transition_warns_only() {
    let (out, _, code) = soma(&["verify", "../examples/refinement/03_payment_dead_transition.cell"]);
    assert_eq!(code, 0, "03_payment_dead_transition.cell must verify (warning, not error)");
    assert!(out.contains("never reached by any handler"),
        "expected dead-transition warning in output: {}", out);
    assert!(out.contains("refunded"),
        "warning should name the unused state: {}", out);
}

#[test]
fn test_refinement_path_conditions_surfaced() {
    let (out, _, code) = soma(&["verify", "../examples/refinement/04_path_conditions.cell"]);
    assert_eq!(code, 0, "04_path_conditions.cell must verify cleanly");
    assert!(out.contains("if amount > 0"),
        "path condition must appear in handler effect summary: {}", out);
    assert!(out.contains("if not (amount > 0)"),
        "negated path condition must appear in else branch: {}", out);
}

#[test]
fn test_refinement_dispatch_undeclared_at_top_level_in_loop() {
    // A handler with a transition() inside a for-loop must still be
    // analyzed (the walker recurses into For/While/If bodies).
    let dir = std::env::temp_dir().join("test_refinement_loop");
    let _ = std::fs::create_dir_all(&dir);
    let cell_path = dir.join("loop.cell");
    std::fs::write(&cell_path, r#"
        cell loopy {
            state s {
                initial: a
                a -> b
            }
            on run() {
                let i = 0
                while i < 3 {
                    transition("t", "ZZZ")
                    i = i + 1
                }
            }
        }
    "#).unwrap();
    let (out, _, code) = soma(&["verify", cell_path.to_str().unwrap()]);
    assert_eq!(code, 1, "verify should fail: ZZZ is not a declared state");
    assert!(out.contains("\"ZZZ\""), "must name the bad target: {}", out);
    let _ = std::fs::remove_dir_all(&dir);
}
