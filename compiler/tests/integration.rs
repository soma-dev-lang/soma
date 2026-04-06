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
