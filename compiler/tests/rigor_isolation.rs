//! Think-isolation regression tests.
//!
//! Verifies that `soma verify` reports the isolation finding correctly:
//!   - Cells with all literal transition targets → think-isolated
//!   - Cells with dynamic targets → NOT think-isolated
//!   - Cells without state machines → no isolation finding
//!
//!   cargo test --test rigor_isolation --release

use std::process::Command;

fn soma_verify(source: &str, name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("soma_isolation_{}", name));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), source).unwrap();
    std::fs::write(dir.join("soma.toml"), "[package]\nname = \"test\"\n[verify]\ndeadlock_free = true\n").unwrap();

    let output = Command::new("./target/release/soma")
        .args(["verify", dir.join("app.cell").to_str().unwrap()])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run soma");
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn isolated_cell_with_literal_targets() {
    let out = soma_verify(r#"
cell Agent {
    state wf {
        initial: idle
        idle -> working -> done
        * -> failed
    }
    on start(id: String) {
        transition(id, "working")
        let result = think("do work")
        if contains(result, "success") {
            transition(id, "done")
        } else {
            transition(id, "failed")
        }
    }
}
"#, "isolated");
    assert!(
        out.contains("think-isolated"),
        "expected think-isolated, got:\n{out}"
    );
    assert!(
        !out.contains("NOT think-isolated"),
        "should not be NOT think-isolated:\n{out}"
    );
}

#[test]
fn not_isolated_with_dynamic_target() {
    let out = soma_verify(r#"
cell Dangerous {
    state wf {
        initial: idle
        idle -> done
        * -> failed
    }
    on process(id: String) {
        let next = think("which state?")
        transition(id, next)
    }
}
"#, "dynamic");
    assert!(
        out.contains("NOT think-isolated") || out.contains("non-literal target"),
        "expected NOT think-isolated or dynamic target warning, got:\n{out}"
    );
}

#[test]
fn no_isolation_finding_without_state_machine() {
    let out = soma_verify(r#"
cell Pure {
    on run() {
        return 42
    }
}
"#, "no_sm");
    assert!(
        !out.contains("think-isolated"),
        "should not emit isolation finding for cell without state machine:\n{out}"
    );
}

#[test]
fn isolated_with_try_think_pattern() {
    // The canonical agent pattern: try { think(...) } then literal transitions
    let out = soma_verify(r#"
cell Reviewer {
    state review {
        initial: pending
        pending -> approved
        pending -> rejected
        * -> failed
    }
    on review(id: String, payload: Map) {
        transition(id, "approved")
    }
}
"#, "try_think");
    assert!(
        out.contains("think-isolated"),
        "try+think pattern with literal targets should be isolated:\n{out}"
    );
}

#[test]
fn rebalancer_is_isolated() {
    // The real rebalancer: 5 handlers, 20 transitions, all literal
    let output = Command::new("./target/release/soma")
        .args(["verify", "../rebalancer/app.cell"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run soma");
    let out = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        out.contains("think-isolated"),
        "the rebalancer should be think-isolated:\n{out}"
    );
}
