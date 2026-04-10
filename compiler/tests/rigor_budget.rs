//! Memory-budget proof obligation — executable tests.
//!
//! Exercises every path of the V1.4 budget checker against
//! synthetic cells written to a temp dir, then asserts on the
//! human-readable output of `soma check`.
//!
//! See `compiler/src/checker/budget.rs` and `docs/SEMANTICS.md` §1.8.
//!
//!   cargo test --test rigor_budget --release

use std::process::Command;

fn soma_check(source: &str, name: &str) -> (String, String, i32) {
    let dir = std::env::temp_dir().join(format!("soma_budget_test_{}", name));
    std::fs::create_dir_all(&dir).unwrap();
    let cell_path = dir.join("app.cell");
    std::fs::write(&cell_path, source).unwrap();

    let exe = "./target/release/soma";
    let output = Command::new(exe)
        .args(["check", cell_path.to_str().unwrap()])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run soma — did you cargo build --release?");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

fn assert_pass(source: &str, name: &str) -> String {
    let (out, err, code) = soma_check(source, name);
    assert_eq!(code, 0, "{name}: expected exit 0, got {code}\nstdout:\n{out}\nstderr:\n{err}");
    assert!(
        out.contains("budget proven"),
        "{name}: expected 'budget proven' in output, got:\n{out}"
    );
    out
}

fn assert_fail(source: &str, name: &str) -> String {
    let (out, _err, code) = soma_check(source, name);
    assert_eq!(code, 1, "{name}: expected exit 1, got {code}\nstdout:\n{out}");
    assert!(
        out.contains("budget exceeded"),
        "{name}: expected 'budget exceeded' in output, got:\n{out}"
    );
    out
}

fn assert_advisory(source: &str, name: &str) -> String {
    let (out, _err, code) = soma_check(source, name);
    assert_eq!(code, 0, "{name}: expected exit 0, got {code}\nstdout:\n{out}");
    assert!(
        out.contains("advisory:"),
        "{name}: expected 'advisory:' in output, got:\n{out}"
    );
    out
}

fn assert_silent(source: &str, name: &str) -> String {
    let (out, _err, code) = soma_check(source, name);
    assert_eq!(code, 0, "{name}: expected exit 0, got {code}\nstdout:\n{out}");
    assert!(
        !out.contains("budget proven") && !out.contains("budget exceeded") && !out.contains("advisory:"),
        "{name}: expected no budget output (no scale.memory declared), got:\n{out}"
    );
    out
}

// ── Path 1: PASS ───────────────────────────────────────────────────

#[test]
fn budget_pass_minimal_cell() {
    assert_pass(
        r#"
cell Trivial {
    memory {
        items: Map<String, String> [persistent, capacity(100), max_value_bytes(512)]
    }
    on get(k: String) { return items.get(k) }
    on put(k: String, v: String) { items.set(k, v) }
    scale {
        replicas: 1
        memory: "256Mi"
    }
}
"#,
        "minimal",
    );
}

#[test]
fn budget_pass_uses_default_when_unannotated() {
    // No capacity/max_value_bytes annotations — defaults to 10000 × 4KiB ≈ 40 MiB
    // for the slot, which fits comfortably in 256 MiB.
    assert_pass(
        r#"
cell Defaulty {
    memory {
        items: Map<String, String> [persistent]
    }
    on get(k: String) { return items.get(k) }
    scale {
        replicas: 1
        memory: "256Mi"
    }
}
"#,
        "defaulty",
    );
}

#[test]
fn budget_pass_state_machine_with_max_instances() {
    let out = assert_pass(
        r#"
cell Workflow {
    memory {
        runs: Map<String, String> [persistent, capacity(50), max_value_bytes(1024)]
    }
    state lifecycle [max_instances(50)] {
        initial: pending
        pending -> done
    }
    on start(id: String) { transition(id, "done") }
    scale {
        replicas: 1
        memory: "256Mi"
    }
}
"#,
        "workflow",
    );
    // Should reflect the small max_instances bound
    assert!(out.contains("state"), "expected state breakdown, got:\n{out}");
}

// ── Path 2: FAIL ───────────────────────────────────────────────────

#[test]
fn budget_fail_capacity_blows_budget() {
    assert_fail(
        r#"
cell Bloated {
    memory {
        items: Map<String, String> [persistent, capacity(1000000), max_value_bytes(8192)]
    }
    on get(k: String) { return items.get(k) }
    scale {
        replicas: 1
        memory: "256Mi"
    }
}
"#,
        "bloated",
    );
}

#[test]
fn budget_fail_state_machine_too_many_instances() {
    assert_fail(
        r#"
cell Pageant {
    state lc [max_instances(10000000)] {
        initial: a
        a -> b
    }
    on go(id: String) { transition(id, "b") }
    scale {
        replicas: 1
        memory: "16Mi"
    }
}
"#,
        "pageant",
    );
}

// ── Path 3: ADVISORY ───────────────────────────────────────────────

#[test]
fn budget_advisory_think_call() {
    let out = assert_advisory(
        r#"
cell Llmy {
    memory {
        runs: Map<String, String> [persistent, capacity(100), max_value_bytes(1024)]
    }
    on submit(payload: Map) {
        let result = think("analyze")
        return result
    }
    scale {
        replicas: 1
        memory: "256Mi"
    }
}
"#,
        "llmy",
    );
    assert!(out.contains("think"), "expected 'think' in advisory reasons");
}

#[test]
fn budget_advisory_from_json_call() {
    let out = assert_advisory(
        r#"
cell Parsey {
    memory {
        runs: Map<String, String> [persistent, capacity(100), max_value_bytes(1024)]
    }
    on parse(body: String) {
        let m = from_json(body)
        return m
    }
    scale {
        replicas: 1
        memory: "256Mi"
    }
}
"#,
        "parsey",
    );
    assert!(out.contains("from_json"), "expected 'from_json' in advisory reasons");
}

#[test]
fn budget_advisory_http_get() {
    let out = assert_advisory(
        r#"
cell Webby {
    memory {
        runs: Map<String, String> [persistent, capacity(100), max_value_bytes(1024)]
    }
    on fetch(url: String) {
        let r = http_get(url)
        return r
    }
    scale {
        replicas: 1
        memory: "256Mi"
    }
}
"#,
        "webby",
    );
    assert!(out.contains("http_get"), "expected 'http_get' in advisory reasons");
}

// ── Path 4: SILENT (opt-in: no scale.memory) ───────────────────────

#[test]
fn budget_silent_no_scale_section() {
    assert_silent(
        r#"
cell Plain {
    memory {
        items: Map<String, String> [persistent, capacity(1000000), max_value_bytes(8192)]
    }
    on get(k: String) { return items.get(k) }
}
"#,
        "plain",
    );
}

#[test]
fn budget_silent_scale_no_memory() {
    assert_silent(
        r#"
cell Plain2 {
    memory {
        items: Map<String, String> [persistent, capacity(1000000), max_value_bytes(8192)]
    }
    on get(k: String) { return items.get(k) }
    scale {
        replicas: 3
        cpu: 2
    }
}
"#,
        "plain2",
    );
}

// ── Helpers correctness: parse_budget_bytes ─────────────────────────

#[test]
fn budget_pass_with_byte_unit() {
    // "16Mi" = 16,777,216 bytes; check we parse it correctly via the
    // PASS path on a cell whose proven peak is below 16Mi.
    // (slot empty + 8 MiB stack + 0 state + 16 MiB runtime = 24 MiB ≥ 16 MiB → FAIL)
    // Actually 24 > 16 so this should FAIL. Use this as a unit test
    // for unit parsing on the FAIL path.
    assert_fail(
        r#"
cell Tiny {
    memory {
        items: Map<String, String> [persistent, capacity(10), max_value_bytes(64)]
    }
    on get(k: String) { return items.get(k) }
    scale {
        replicas: 1
        memory: "16Mi"
    }
}
"#,
        "tiny",
    );
}

#[test]
fn budget_pass_with_gi_unit() {
    // 1 GiB easily fits the default cell footprint.
    assert_pass(
        r#"
cell BigBudget {
    memory {
        items: Map<String, String> [persistent, capacity(1000), max_value_bytes(4096)]
    }
    on get(k: String) { return items.get(k) }
    scale {
        replicas: 1
        memory: "1Gi"
    }
}
"#,
        "bigbudget",
    );
}

// ── Multi-handler: max, not sum ─────────────────────────────────────

#[test]
fn budget_pass_many_handlers_uses_max_not_sum() {
    // 10 handlers × 8 MiB stack = 80 MiB if SUMMED.
    // If MAX is used (correct), the stack contribution is just 8 MiB.
    // Budget 64 MiB should fit: 8 MiB stack + 16 MiB runtime + tiny slot = 24 MiB.
    assert_pass(
        r#"
cell Multi {
    memory {
        items: Map<String, String> [persistent, capacity(10), max_value_bytes(64)]
    }
    on h1() { return 1 }
    on h2() { return 2 }
    on h3() { return 3 }
    on h4() { return 4 }
    on h5() { return 5 }
    on h6() { return 6 }
    on h7() { return 7 }
    on h8() { return 8 }
    on h9() { return 9 }
    on h10() { return 10 }
    scale {
        replicas: 1
        memory: "64Mi"
    }
}
"#,
        "multi",
    );
}
