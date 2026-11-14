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

#[test]
fn delete_cannot_break_a_size_invariant() {
    let p = write_probe(
        "v18_inv_delete.cell",
        r#"
cell D {
    face {
        signal seed() -> Int
        signal drop_it() -> Int
        signal count() -> Int
    }
    memory {
        roster: Map<String, Int>
        invariant size >= 1
    }
    on seed() { roster.set("a", 1) return 1 }
    on drop_it() { roster.delete("a") return roster.len() }
    on count() { return roster.len() }
}
cell test DTests {
    rules {
        assert seed() == 1
        assert_fails drop_it()
        assert count() == 1
        // deleting a missing key is a no-op, not a violation
        assert roster.delete("nope") == false
    }
}
"#,
    );
    let (out, code) = soma(&["test", &p]);
    assert_eq!(code, 0, "delete must be invariant-checked: {out}");
    assert!(out.contains("rejected delete"), "got: {out}");

    let (out, _) = soma(&["verify", &p]);
    assert!(
        out.contains("drop_it → roster"),
        "verify must list the delete as a runtime-checked writer: {out}"
    );
}

#[test]
fn bracket_writes_are_seen_by_verify() {
    let p = write_probe(
        "v18_inv_bracket.cell",
        r#"
cell B {
    face { signal go() -> Int }
    memory {
        account: Map<String, Int>
        invariant account >= 0
    }
    on go() {
        account["balance"] = -5
        return 1
    }
}
"#,
    );
    let (out, code) = soma(&["verify", &p]);
    assert_ne!(code, 0, "a literal violating bracket write must fail verify: {out}");
    assert!(out.contains("statically violated"), "got: {out}");
}

#[test]
fn check_rejects_multi_slot_invariants() {
    let p = write_probe(
        "v18_inv_multislot.cell",
        r#"
cell M {
    face { signal go() -> Int }
    memory {
        a: Map<String, Int>
        b: Map<String, Int>
        invariant a + b <= 100
    }
    on go() { a.set("k", 1) return 1 }
}
"#,
    );
    let (out, code) = soma(&["check", &p]);
    assert_ne!(code, 0, "an invariant no write can satisfy must fail check: {out}");
    assert!(out.contains("invariant between slots (a, b)") && out.contains("?? 0"), "got: {out}");
}

// ── a size bound is proven whatever form the guard takes ────────────
// (an agent's report, 2026-11-14: correct code was left "runtime-checked"
// because of how it was written — `journal.size` where `len(journal)` was
// recognised, an early `if … { return }` where `require` was, and a local
// `let cur = notes.get(k) ?? ""` where `notes.get(k) != ()` was)

const SIZE_BASE: &str = r#"
cell Desk {
    memory {
        notes: Map<String, String> [persistent]
        invariant notes.size <= 50
    }
    on add(k: String, v: String) {
        ADD_GUARD
        notes.set(k, v)
        return "ok"
    }
    on edit(k: String, v: String) {
        EDIT_GUARD
        notes.set(k, v)
        return "ok"
    }
}
"#;

fn size_verdicts(name: &str, add: &str, edit: &str) -> (bool, bool, String) {
    let src = SIZE_BASE.replace("ADD_GUARD", add).replace("EDIT_GUARD", edit);
    let (out, _) = soma(&["verify", &write_probe(name, &src)]);
    let proven = |h: &str| out.lines().any(|l| l.contains("✓") && l.contains(&format!("writer '{h}'")));
    (proven("add"), proven("edit"), out)
}

#[test]
fn a_size_bound_is_proven_whatever_form_the_guard_takes() {
    let ok_add = "require len(notes) < 50 else Full";
    let ok_edit = "require notes.get(k) != () else Missing";
    // the forms the prover already knew
    let (a, e, out) = size_verdicts("size_form_known.cell", ok_add, ok_edit);
    assert!(a && e, "{out}");
    // `notes.size` / `notes.len` in the require: the same count as len(notes)
    for g in ["require notes.size < 50 else Full", "require notes.len < 50 else Full", "require 50 > notes.size else Full"] {
        let (a, _, out) = size_verdicts("size_form_field.cell", g, ok_edit);
        assert!(a, "`{g}` should prove the size bound:\n{out}");
    }
    // an early return is a guard for the writes after it
    for g in ["if len(notes) >= 50 { return \"full\" }", "if notes.size >= 50 { return \"full\" }", "if 50 <= len(notes) { return \"full\" }"] {
        let (a, _, out) = size_verdicts("size_form_return.cell", g, ok_edit);
        assert!(a, "`{g}` should prove the size bound:\n{out}");
    }
    // a local holding the slot's value with a default: different from the
    // default means the key was there (`get` gave () → the default)
    for g in ["let cur = notes.get(k) ?? \"\"\n        require cur != \"\" else Missing",
              "let cur = notes.get(k) ?? \"\"\n        if cur == \"\" { return \"missing\" }"] {
        let (_, e, out) = size_verdicts("size_form_coalesce.cell", ok_add, g);
        assert!(e, "`{g}` should prove the size bound:\n{out}");
    }
}

#[test]
fn a_size_bound_is_not_proven_by_a_guard_that_does_not_hold() {
    let ok_add = "require len(notes) < 50 else Full";
    let ok_edit = "require notes.get(k) != () else Missing";
    // one too many: 50 entries + 1
    for g in ["require notes.size <= 50 else Full", "if len(notes) > 50 { return \"full\" }",
              // the early exit does not exit
              "if len(notes) >= 50 { print(\"full\") }"] {
        let (a, _, out) = size_verdicts("size_bad_add.cell", g, ok_edit);
        assert!(!a, "`{g}` must not prove the size bound:\n{out}");
    }
    // the default is what a PRESENT key may hold too: equal to it says nothing
    for g in ["let cur = notes.get(k) ?? \"\"\n        require cur == \"\" else Missing",
              // the local is rebound between the test and the write
              "let cur = notes.get(k) ?? \"\"\n        require cur != \"\" else Missing\n        cur = \"x\"",
              // another key
              "let cur = notes.get(\"other\") ?? \"\"\n        require cur != \"\" else Missing"] {
        let (_, e, out) = size_verdicts("size_bad_edit.cell", ok_add, g);
        assert!(!e, "`{g}` must not prove the size bound:\n{out}");
    }
}

#[test]
fn verify_says_why_an_invariant_on_len_of_the_slot_is_not_a_count() {
    let src = r#"
cell Log {
    memory {
        journal: List<String> [persistent]
        invariant len(journal) <= 100
    }
    on add(line: String) {
        require len(journal) < 100 else Full
        journal.push(line)
    }
}
"#;
    let (out, _) = soma(&["verify", &write_probe("len_of_slot.cell", src)]);
    assert!(out.contains("measures each VALUE written") && out.contains("journal.size <= N"), "{out}");
}
