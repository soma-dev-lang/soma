//! Regression test for the depth-bound soundness gap in the CTL
//! `eventually` checker.
//!
//! Before this fix the DFS counter-example search hard-coded a depth
//! bound of 50. On a state machine with an acyclic path longer than
//! 50 from the initial state to a non-satisfying terminal, the DFS
//! gave up before reaching the terminal and the caller treated
//! "no counter-example found" as "property holds" — a false positive
//! on a liveness property.
//!
//! Repro: a 60-state linear chain s0 → s1 → ... → s59 → dead_end with
//! the property `eventually(NEVER_HIT)`. NEVER_HIT is unreachable, so
//! every execution falsifies the property; the verifier MUST report
//! `passed: false` and include a counter-example that ends in
//! `dead_end`.
//!
//! See `docs/SOUNDNESS.md` §3 for the full discussion.

use std::process::Command;

fn build_chain(n_states: usize) -> String {
    let mut s = String::from("cell C {\n    state lc {\n        initial: s0\n");
    for i in 0..n_states - 1 {
        s.push_str(&format!("        s{} -> s{}\n", i, i + 1));
    }
    s.push_str(&format!("        s{} -> dead_end\n", n_states - 1));
    s.push_str("    }\n    on run() { return 0 }\n}\n");
    s
}

fn write_proj(dir: &std::path::Path, n_states: usize) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("app.cell"), build_chain(n_states)).unwrap();
    std::fs::write(
        dir.join("soma.toml"),
        "[package]\nname = \"rigor_chain\"\n\n[verify]\ndeadlock_free = true\neventually = [\"NEVER_HIT\"]\n",
    )
    .unwrap();
}

fn run_verify(file: &std::path::Path) -> serde_json::Value {
    let exe = "./target/release/soma";
    let output = Command::new(exe)
        .args(["verify", file.to_str().unwrap(), "--json"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run soma — did you `cargo build --release`?");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    // The first line is "Verifying ..." — strip it before parsing JSON.
    let json_start = stdout.find('{').expect("no JSON in stdout");
    serde_json::from_str(&stdout[json_start..]).expect("invalid JSON")
}

#[test]
fn eventually_finds_counter_example_on_60_state_chain() {
    let dir = std::env::temp_dir().join("rigor_chain_60");
    write_proj(&dir, 61); // 61 chain states + dead_end terminal
    let result = run_verify(&dir.join("app.cell"));
    let temporal = &result["temporal"][0]["properties"];

    // Find the eventually result
    let eventually = temporal
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["property"].as_str().unwrap().starts_with("eventually"))
        .expect("eventually property missing — soma.toml not loaded?");

    assert_eq!(
        eventually["passed"], false,
        "soundness regression: eventually(NEVER_HIT) reported as PASSING on a 60-state chain. \
         The depth bound in temporal.rs::Property::Eventually probably regressed to a hard-coded \
         constant. It must be ≥ |reachable| so the DFS can fully explore acyclic paths."
    );

    let ce = eventually["counter_example"]
        .as_array()
        .expect("counter-example missing");
    assert!(
        ce.len() >= 60,
        "counter-example too short: {} states (expected ≥60)",
        ce.len()
    );
    assert_eq!(
        ce.last().unwrap().as_str().unwrap(),
        "dead_end",
        "counter-example must terminate at dead_end"
    );
}

#[test]
fn eventually_finds_counter_example_on_200_state_chain() {
    // Stress version: 200-state chain, well past the old 50 bound.
    let dir = std::env::temp_dir().join("rigor_chain_200");
    write_proj(&dir, 201);
    let result = run_verify(&dir.join("app.cell"));
    let temporal = &result["temporal"][0]["properties"];
    let eventually = temporal
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["property"].as_str().unwrap().starts_with("eventually"))
        .unwrap();

    assert_eq!(eventually["passed"], false);
    let ce = eventually["counter_example"].as_array().unwrap();
    assert!(ce.len() >= 200);
}

#[test]
fn eventually_passes_when_predicate_actually_holds() {
    // Sanity: same chain, but ask for a predicate that DOES hold on every path.
    let dir = std::env::temp_dir().join("rigor_chain_60_pass");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), build_chain(61)).unwrap();
    std::fs::write(
        dir.join("soma.toml"),
        "[package]\nname = \"rigor_chain_pass\"\n\n[verify]\ndeadlock_free = true\neventually = [\"dead_end\"]\n",
    )
    .unwrap();
    let result = run_verify(&dir.join("app.cell"));
    let temporal = &result["temporal"][0]["properties"];
    let eventually = temporal
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["property"].as_str().unwrap().starts_with("eventually"))
        .unwrap();
    assert_eq!(
        eventually["passed"], true,
        "false negative: eventually(dead_end) should hold on a chain that always reaches dead_end"
    );
}
