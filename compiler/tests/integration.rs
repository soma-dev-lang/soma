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
