//! Integration tests for the builtin registry surface:
//!   soma describe --builtins [--json]
//!   soma describe <file.cell> --faces [--json]
//!   soma docs builtins
//!
//! The registry's internal invariants (no duplicate names, NONDET derivation)
//! are unit-tested in src/interpreter/builtins/registry.rs; these tests pin
//! the CLI contract that agents consume.

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

// ── describe --builtins ──────────────────────────────────────────────

#[test]
fn describe_builtins_json_is_complete_and_unique() {
    let (out, _, code) = soma(&["describe", "--builtins", "--json"]);
    assert_eq!(code, 0);
    let entries: Vec<serde_json::Value> = serde_json::from_str(&out)
        .expect("--json must emit a JSON array");
    assert!(entries.len() >= 140, "expected ~160 builtins, got {}", entries.len());

    // No duplicate names.
    let mut seen = std::collections::HashSet::new();
    for e in &entries {
        let name = e["name"].as_str().expect("entry has name");
        assert!(seen.insert(name.to_string()), "duplicate builtin: {}", name);
        assert!(e["signature"].as_str().is_some_and(|s| !s.is_empty()),
            "{}: empty signature", name);
        assert!(e["brief"].as_str().is_some_and(|s| !s.is_empty()),
            "{}: empty brief", name);
        assert!(e["category"].as_str().is_some_and(|s| !s.is_empty()),
            "{}: empty category", name);
    }

    // Every name in the historical NONDET list is deterministic:false.
    let nondet: Vec<&str> = entries.iter()
        .filter(|e| e["deterministic"] == serde_json::json!(false))
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    for name in ["now", "now_ms", "timestamp", "today", "date_now", "random", "rand"] {
        assert!(nondet.contains(&name), "'{}' must be deterministic:false", name);
    }
    assert_eq!(nondet.len(), 7,
        "nondeterministic set changed — this changes replay behavior: {:?}", nondet);
}

#[test]
fn describe_builtins_text_groups_by_category() {
    let (out, _, code) = soma(&["describe", "--builtins"]);
    assert_eq!(code, 0);
    for cat in ["string", "math", "collection", "pipeline", "agent", "linalg"] {
        assert!(out.contains(&format!("── {} ", cat)), "missing category '{}'", cat);
    }
    // Nondeterministic builtins carry the ✗ marker.
    assert!(out.lines().any(|l| l.contains('✗') && l.contains("now()")),
        "now() must be marked nondeterministic with ✗");
    // A representative signature an agent would query.
    assert!(out.contains("agg(rows: List<Map>, group_field, \"col:func\"...) -> List<Map>"));
}

#[test]
fn describe_without_file_or_builtins_says_how_to_fix() {
    let (_, err, code) = soma(&["describe"]);
    assert_ne!(code, 0);
    assert!(err.contains("--builtins"), "error must show the correction: {}", err);
}

// ── describe --faces ─────────────────────────────────────────────────

#[test]
fn describe_faces_delivery_app_is_compact_contract() {
    let (out, _, code) = soma(&["describe", "../delivery/app.cell", "--faces"]);
    assert_eq!(code, 0);

    // Sum types with variants.
    assert!(out.contains("type OrderState = Placed | Accepted | Cooking | Ready | PickedUp | Delivered | Cancelled"));
    assert!(out.contains("type PayResult = Charged { tx: String, amount: Int } | Declined { reason: String } | CashOnDelivery"));

    // Face signals with full signatures.
    assert!(out.contains("signal charge(method: String, amount: Int) -> Map"));
    assert!(out.contains("signal pickup_order(id: String, courier: String) -> String"));

    // Promises render as written.
    assert!(out.contains("promise all_persistent"));

    // Memory slots with type + properties.
    assert!(out.contains("memory jobs: Map<String, String> [persistent, consistent]"));

    // State machines: name, sum-type annotation, initial, compact transitions.
    assert!(out.contains("state order: OrderState (initial Placed)"));
    assert!(out.contains("Placed -> Accepted, Accepted -> Cooking"));
    assert!(out.contains("state duty (initial idle): idle -> assigned, assigned -> delivering, delivering -> idle"));

    // NO handler bodies — and token-cheap: ~60 lines for the 7-cell app.
    assert!(!out.contains("transition("), "handler bodies must not leak");
    assert!(!out.contains("let "), "handler bodies must not leak");
    let lines = out.lines().count();
    assert!(lines <= 80, "faces output should be compact, got {} lines", lines);
}

#[test]
fn describe_faces_json_is_structured() {
    let (out, _, code) = soma(&["describe", "../delivery/app.cell", "--faces", "--json"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    let cells = v["cells"].as_array().expect("cells array");
    assert!(cells.len() >= 7, "expected >= 7 cells (incl. 2 types), got {}", cells.len());

    let orders = cells.iter().find(|c| c["name"] == "Orders").expect("Orders cell");
    assert!(orders["signals"].as_array().unwrap().iter()
        .any(|s| s == "place_order(customer: String, rest: String, dishes: String, pay: String) -> Map"));
    assert_eq!(orders["states"][0]["type"], "OrderState");
    assert!(orders["states"][0]["transitions"].as_array().unwrap()
        .iter().any(|t| t == "Placed -> Cancelled"));

    let pay_result = cells.iter().find(|c| c["name"] == "PayResult").expect("PayResult type");
    assert_eq!(pay_result["kind"], "type");
    assert!(pay_result["variants"].as_array().unwrap()
        .iter().any(|t| t == "Declined { reason: String }"));
}

// ── docs builtins ────────────────────────────────────────────────────

#[test]
fn docs_builtins_renders_generated_markdown() {
    let (out, _, code) = soma(&["docs", "builtins"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("# Soma Builtins"));
    assert!(out.contains("GENERATED by `soma docs builtins`"));
    assert!(out.contains("## string"));
    assert!(out.contains("| Builtin | Signature | Description |"));
    // Pipes inside signatures must be escaped so the table stays intact.
    assert!(out.contains("random() -> Float \\| random(max: Int) -> Int"));
    // Nondeterministic marker present in the table.
    assert!(out.contains("| `now` ✗ |"));
}

#[test]
fn docs_unknown_topic_lists_valid_topics() {
    let (_, err, code) = soma(&["docs", "nope"]);
    assert_ne!(code, 0);
    assert!(err.contains("Valid topics: [builtins]"), "got: {}", err);
}
