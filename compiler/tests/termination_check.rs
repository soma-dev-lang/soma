//! Handler termination proof: recursion shapes that must NOT be reported
//! as "structurally terminate".

use std::process::Command;

fn verify(name: &str, handlers: &str) -> String {
    let src = format!(
        r#"
cell T {{
    face {{
        signal a(n: Int) -> Int
        signal b(n: Int) -> Int
    }}
    state flow {{
        initial: s0
        s0 -> s1
    }}
{handlers}
}}
"#
    );
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, src).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["verify", path.to_str().unwrap()])
        .output()
        .expect("failed to run soma");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn mutual_recursion_is_flagged() {
    let out = verify(
        "term_mutual.cell",
        "    on a(n: Int) { return b(n + 1) }\n    on b(n: Int) { return a(n + 1) }",
    );
    assert!(out.contains("mutual recursion a → b → a"), "got: {out}");
    assert!(!out.contains("structurally terminate"), "got: {out}");
}

#[test]
fn recursion_hidden_in_an_operand_is_flagged() {
    let out = verify(
        "term_hidden.cell",
        "    on a(n: Int) { return 1 + a(n + 1) }\n    on b(n: Int) { return n }",
    );
    assert!(out.contains("recursive call without provable decreasing argument"), "got: {out}");
}

#[test]
fn decreasing_recursion_needs_a_base_case() {
    let out = verify(
        "term_nobase.cell",
        "    on a(n: Int) { return a(n - 1) }\n    on b(n: Int) { return n }",
    );
    assert!(out.contains("no base case"), "got: {out}");
}

#[test]
fn structural_recursion_and_plain_calls_still_prove() {
    let out = verify(
        "term_ok.cell",
        "    on a(n: Int) {\n        if n <= 1 { return 1 }\n        return n * a(n - 1)\n    }\n    on b(n: Int) { return a(n) + 1 }",
    );
    assert!(out.contains("all 2 handlers structurally terminate"), "got: {out}");
}
