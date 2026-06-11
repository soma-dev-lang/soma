//! V1.8 memory invariants: runtime rejection semantics, static proof
//! pass in `soma verify`, and check-time validation of the invariant
//! expressions themselves.

use std::process::Command;

fn soma(args: &[&str]) -> (String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("failed to run soma");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (text, output.status.code().unwrap_or(-1))
}

fn write_probe(name: &str, src: &str) -> String {
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, src).unwrap();
    path.to_str().unwrap().to_string()
}

const GUARDED: &str = r#"
cell G {
    face {
        signal set_it(v: Int) -> Map
        signal read_it() -> Int
        signal seed_it() -> Int
        signal corrupt_it() -> Int
    }
    memory {
        position: Map<String, Int> [persistent]
        invariant abs(position) <= 5
    }
    on set_it(v: Int) {
        let r = try { position.set("k", v) }
        if r.error != () { return map("ok", false) }
        return map("ok", true)
    }
    on read_it() {
        let p = position.get("k")
        if p == () { return 0 }
        return p
    }
    on seed_it() { position.set("k", 3) return 3 }
    on corrupt_it() { position.set("k", 99) return 99 }
}
cell test GTests {
    rules {
        assert set_it(4).ok == true
        assert read_it() == 4
        // rejected write: error raised, previous value intact
        assert set_it(9).ok == false
        assert read_it() == 4
        assert_fails position.set("k", 0 - 50)
    }
}
"#;

#[test]
fn runtime_rejects_and_preserves_previous_value() {
    let p = write_probe("v18_inv_runtime.cell", GUARDED);
    let (out, code) = soma(&["test", &p]);
    assert_eq!(code, 0, "guarded tests must pass: {out}");
    assert!(out.contains("5 passed"), "expected 5 passing assertions: {out}");
}

#[test]
fn violation_message_quotes_the_invariant() {
    let p = write_probe("v18_inv_msg.cell", GUARDED);
    let (out, code) = soma(&["run", &p, "corrupt_it"]);
    assert_eq!(code, 1, "direct breach must error: {out}");
    assert!(
        out.contains("memory invariant violated on 'position': abs(position) <= 5"),
        "message must quote slot and expression: {out}"
    );
    assert!(out.contains("the slot is unchanged"), "message must state rollback: {out}");
}

#[test]
fn verify_proves_literals_and_fails_static_violations() {
    let p = write_probe("v18_inv_verify.cell", GUARDED);
    let (out, code) = soma(&["verify", &p]);
    assert!(
        out.contains("invariant abs(position) <= 5 — writer 'seed_it' proven (writes 3)"),
        "literal within bounds must be proven: {out}"
    );
    assert!(
        out.contains("writer 'corrupt_it' writes 99 to 'position': statically violated"),
        "literal beyond bounds must fail: {out}"
    );
    assert_eq!(code, 1, "a statically violated invariant must fail verify: {out}");
}

#[test]
fn check_rejects_unknown_names_in_invariants() {
    let p = write_probe(
        "v18_inv_badname.cell",
        r#"
cell B {
    face { signal go() -> Int }
    memory {
        total: Map<String, Int> [persistent]
        invariant totl >= 0
    }
    on go() { total.set("a", 1) return 1 }
}
"#,
    );
    let (out, code) = soma(&["check", &p]);
    assert_eq!(code, 1, "unknown name in invariant must fail check: {out}");
    assert!(out.contains("unknown name 'totl'"), "got: {out}");
    assert!(out.contains("did you mean 'total'?"), "got: {out}");
}

#[test]
fn check_rejects_non_builtin_calls_in_invariants() {
    let p = write_probe(
        "v18_inv_badcall.cell",
        r#"
cell B {
    face { signal go() -> Int  signal helper(x: Int) -> Bool }
    memory {
        total: Map<String, Int> [persistent]
        invariant helper(total)
    }
    on go() { total.set("a", 1) return 1 }
    on helper(x: Int) { return x >= 0 }
}
"#,
    );
    let (out, code) = soma(&["check", &p]);
    assert_eq!(code, 1, "handler call in invariant must fail check: {out}");
    assert!(out.contains("not a builtin"), "got: {out}");
}

#[test]
fn invariant_scopes_to_named_slot_only() {
    // `invariant abs(position) <= 5` must not guard writes to `notes`
    let p = write_probe(
        "v18_inv_scope.cell",
        r#"
cell S {
    face { signal go() -> String }
    memory {
        position: Map<String, Int> [persistent]
        notes: Map<String, String> [persistent]
        invariant abs(position) <= 5
    }
    on go() {
        notes.set("a", "free text far beyond any numeric bound")
        return notes.get("a")
    }
}
"#,
    );
    let (out, code) = soma(&["run", &p, "go"]);
    assert_eq!(code, 0, "unrelated slot must not be guarded: {out}");
    assert!(out.contains("free text"), "got: {out}");
}

#[test]
fn clamp_writes_are_proven() {
    let p = write_probe(
        "v18_inv_clamp.cell",
        r#"
cell C {
    face { signal go(v: Int) -> Int }
    memory {
        position: Map<String, Int> [persistent]
        invariant abs(position) <= 5
    }
    on go(v: Int) {
        position.set("k", clamp(v, 0 - 5, 5))
        return 1
    }
}
"#,
    );
    let (out, code) = soma(&["verify", &p]);
    assert_eq!(code, 0, "clamped write must verify clean: {out}");
    assert!(
        out.contains("writer 'go' proven"),
        "clamp within bounds must be statically proven: {out}"
    );
}
