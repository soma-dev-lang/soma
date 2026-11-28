//! A skeleton under a law, before any body exists: what a hole owes to a rule
//! in a sentence is an open obligation, not an error; a derived guard whose
//! state awaits a body does not pass for a refusal; a refusal names every
//! rule the call breaks.

use std::process::Command;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("soma_lawskel_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("lois")).unwrap();
    d
}

fn soma(d: &std::path::Path, args: &[&str]) -> (String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(d).output().expect("failed to run soma");
    (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
}

const TOML: &str = "[package]\nname = \"essai\"\n\n[law]\nfiles = [\"lois/billing.law\"]\n";

const LAW: &str = r#"law Billing

entity Line "A line of an invoice."
term Adjustment "A change of the amount of a line, after the invoice is committed."

state known(Line): Bool "The invoice has a line of this name."
state system_credit(Line): Bool "The line is a credit the system created."

action Removal(l: Line, n: Int) "The removal of an amount from a line."

rule "B-01" "Only a line that exists is changed"
    about    Removal
    requires Removal(l, n): known(l)

rule "B-02" "An amount removed is positive"
    about    Removal
    requires Removal(l, n): n > 0

rule "B-03" "A credit of the system is never removed"
    about    Removal
    requires Removal(l, n): not system_credit(l)

rule "B-04" "Every adjustment is written to the journal"
    about    Adjustment
"#;

/// `system_credit`: how the skeleton reads the state — the memory, or a signal.
fn app(system_credit: &str, flesh: &str) -> String {
    format!(r#"
skeleton Invoices {{
    memory {{
        lines: Map<String, Int> [persistent]
        credits: Map<String, Bool> [persistent]
        journal: Map<String, String> [persistent]
    }}
    maps known(l) = lines.get(l) != ()
    maps system_credit(l) = {system_credit}
    face {{
        signal is_system(l: String) -> Bool
            reads credits
        signal remove(l: String, n: Int) -> Int
            reads lines, credits
            writes lines
            realizes Removal(l, n)
        signal adjust(l: String, n: Int) -> Int
            writes journal
            realizes Adjustment
        signal close(l: String) -> String
            moves open -> closed
    }}
    state flow {{
        initial: open
        open -> closed
    }}
}}
{flesh}
"#)
}

fn project(name: &str, source: &str) -> std::path::PathBuf {
    let d = dir(name);
    std::fs::write(d.join("soma.toml"), TOML).unwrap();
    std::fs::write(d.join("lois/billing.law"), LAW).unwrap();
    std::fs::write(d.join("app.cell"), source).unwrap();
    let (out, code) = soma(&d, &["law", "app.cell", "--lock"]);
    assert!(d.join("law.lock").exists(), "{code}: {out}");
    d
}

const MEMORY: &str = "credits.get(l) ?? false";

#[test]
fn a_rule_in_a_sentence_is_owed_by_a_hole_and_the_skeleton_still_runs() {
    let d = project("owed", &app(MEMORY, ""));
    // check: no error, and the debt is said
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("E0130"), "{out}");
    assert!(out.contains("1 rule of the law still owed by the flesh"), "{out}");
    let (out, _) = soma(&d, &["check", "app.cell", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert_eq!(v["owed"][0]["rule"], "B-04", "{out}");
    assert_eq!(v["owed"][0]["signal"], "Invoices.adjust", "{out}");
    // verify: the structure is proven, and the verdict counts what is owed
    let (out, code) = soma(&d, &["verify", "app.cell", "--summary"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("VERIFY OK (skeleton — the holes above are open obligations, and 1 rule of the law still owed by the flesh)"), "{out}");
    // the law: the rule is not served yet, the work list names who owes it
    let (out, code) = soma(&d, &["law", "app.cell", "--todo", "--no-run"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("Invoices.adjust owes \"B-04\""), "{out}");
    let (out, _) = soma(&d, &["law", "app.cell", "--json", "--no-run"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert_eq!(v["owed"][0]["signal"], "Invoices.adjust", "{out}");
    assert!(!v["todo"].as_array().unwrap().iter().any(|t| t.as_str().unwrap().contains("B-04")), "what a hole owes is not a fault: {out}");
    // the brief of the hole says what its body owes
    let (out, _) = soma(&d, &["skeleton", "app.cell", "Invoices.adjust"]);
    assert!(out.contains("owes \"B-04\""), "{out}");
    // a test of a derived guard runs on the skeleton
    std::fs::write(d.join("test_guards.cell"), "use app\ncell test Guards under \"B-01\" {\n    rules { assert_fails Invoices.remove(\"x\", 5) matching \"B-01\" }\n}\n").unwrap();
    let (out, code) = soma(&d, &["test", "test_guards.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("1 passed"), "{out}");
}

#[test]
fn a_body_that_does_not_serve_the_rule_is_still_the_forgotten_path() {
    let flesh = "flesh Invoices {\n    on adjust(l, n) {\n        journal.set(l, \"adjusted\")\n        return n\n    }\n}\n";
    let d = project("forgotten", &app(MEMORY, flesh));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("E0130") && out.contains("B-04"), "{out}");
    assert!(!out.contains("owed by the flesh"), "{out}");
}

#[test]
fn a_refusal_names_every_rule_the_call_breaks() {
    let d = project("all", &app(MEMORY, ""));
    std::fs::write(d.join("t.cell"), "use app\ncell test T under \"B-02\" {\n    rules {\n        assert_fails Invoices.remove(\"x\", -1) matching \"B-01\"\n        assert_fails Invoices.remove(\"x\", -1) matching \"B-02\"\n    }\n}\n").unwrap();
    let (out, code) = soma(&d, &["test", "t.cell"]);
    assert_eq!(code, 0, "{out}");
    let line = out.lines().find(|l| l.contains("matching \"B-02\"")).expect(&out);
    // in the order of the law, and the rule it keeps is not among them
    let (a, b) = (line.find("rule \"B-01\"").expect(line), line.find("rule \"B-02\"").expect(line));
    assert!(a < b && !line.contains("rule \"B-03\""), "{line}");
}

#[test]
fn a_state_that_awaits_a_body_is_not_a_refusal_by_the_law() {
    let d = project("awaits", &app("is_system(l)", ""));
    // on a line that exists nowhere the state is false and the rule holds:
    // what fails is the reading, and it says so without naming the rule
    std::fs::write(d.join("lois/billing.law"), LAW.replace("    requires Removal(l, n): known(l)\n", "").replace("    requires Removal(l, n): n > 0\n", "")).unwrap();
    let (out, code) = soma(&d, &["law", "app.cell", "--lock"]);
    assert_eq!(code, 0, "{out}");
    std::fs::write(d.join("t.cell"), "use app\ncell test T {\n    rules {\n        assert_fails Invoices.remove(\"x\", 5) matching \"B-03\"\n        assert_fails Invoices.remove(\"x\", 5) matching \"cannot be read\"\n    }\n}\n").unwrap();
    let (out, _) = soma(&d, &["test", "t.cell"]);
    assert!(out.contains("1 passed, 1 failed"), "{out}");
    let line = out.lines().find(|l| l.contains("cannot be read\" —")).expect(&out);
    assert!(line.contains("the state `system_credit` of the law cannot be read") && line.contains("Invoices.is_system is a hole"), "{line}");
    // check warns, where the `maps` is written
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("`maps system_credit`") && out.contains("`Invoices.is_system`") && out.contains("no body yet"), "{out}");
    // the law says the rule is not judged yet
    let (out, _) = soma(&d, &["law", "app.cell", "B-03", "--no-run"]);
    assert!(out.contains("awaiting Invoices.is_system"), "{out}");
    let (out, _) = soma(&d, &["law", "app.cell", "--map"]);
    assert!(out.contains("Invoices.is_system") && out.contains("no body yet"), "{out}");
}

#[test]
fn a_rule_broken_for_sure_is_a_refusal_even_beside_a_state_that_awaits_a_body() {
    let d = project("both", &app("is_system(l)", ""));
    std::fs::write(d.join("t.cell"), "use app\ncell test T under \"B-02\" {\n    rules { assert_fails Invoices.remove(\"x\", -1) matching \"B-02\" }\n}\n").unwrap();
    let (out, code) = soma(&d, &["test", "t.cell"]);
    assert_eq!(code, 0, "{out}");
    let line = out.lines().find(|l| l.contains("matching \"B-02\"")).unwrap();
    // B-03 is not among the rules broken — and the message says it was not judged
    assert!(!line.contains("it requires `not system_credit") && line.contains("not judged: rule \"B-03\""), "{out}");
}

#[test]
fn a_maps_that_reads_another_cell_through_a_hole_and_a_field_is_seen() {
    let source = r#"
skeleton Ledger {
    memory { cards: Map<String, Map> [persistent] }
    face {
        signal card(l: String) -> Map
            reads cards
    }
}
skeleton Invoices {
    memory { lines: Map<String, Int> [persistent] }
    maps known(l) = lines.get(l) != ()
    maps system_credit(l) = Ledger.card(l).system == true
    face {
        signal remove(l: String, n: Int) -> Int
            reads lines
            writes lines
            calls Ledger.card
            realizes Removal(l, n)
        signal adjust(l: String, n: Int) -> Int
            writes lines
            realizes Removal(l, n)
    }
}
"#;
    let d = project("across", source);
    let (out, _) = soma(&d, &["check", "app.cell"]);
    assert!(out.contains("`maps system_credit` in `Invoices` reads through `Ledger.card`") && out.contains("no body yet"), "{out}");
    let (out, _) = soma(&d, &["law", "app.cell", "B-03", "--no-run"]);
    assert!(out.contains("awaiting Ledger.card"), "{out}");
}
