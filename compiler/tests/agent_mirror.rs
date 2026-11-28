//! What a skeleton does not state, said by the toolchain: the mirror of a
//! program (`soma doctor --program`), the `maps` read as bodies, the type of
//! a parameter against the entity of the law, a project under a law to
//! imitate (`soma init --law`), and the guard rails of the method.

use std::process::Command;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("soma_mirror_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("lois")).unwrap();
    d
}

fn soma(d: &std::path::Path, args: &[&str]) -> (String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(d).output().expect("failed to run soma");
    (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
}

const BARE: &str = r#"
skeleton Stock {
    memory { units: Map<String, Int> [persistent] }
    state item {
        initial: listed
        listed -> sold
    }
    face {
        signal card(sku: String) -> Map
            reads units
        // Sells one unit of `sku`.
        signal sell(sku: String) -> Int
            writes units
            moves listed -> sold
    }
}
"#;

#[test]
fn the_mirror_says_what_a_skeleton_does_not_state() {
    let d = dir("bare");
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n").unwrap();
    std::fs::write(d.join("app.cell"), BARE).unwrap();
    let (out, code) = soma(&d, &["doctor", "--program", "app.cell"]);
    assert_eq!(code, 0, "nothing here is an error: {out}");
    for needle in ["`Stock.card` — returns `Map`", "the slot `units` of `Stock` holds numbers and no invariant", "1 state machine and no property", "`Stock.card` has no comment", "soma docs skeleton --brief"] {
        assert!(out.contains(needle), "missing {needle:?} in {out}");
    }
    assert!(!out.contains("`Stock.sell` has no comment"), "{out}");
    let (out, _) = soma(&d, &["doctor", "--program", "app.cell", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert_eq!(v["schema"], "soma.mirror/1");
    assert_eq!(v["not_stated"].as_array().unwrap().len(), 4, "{out}");
    // check passes, and no longer in silence
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("○ not stated by the skeleton: 1 signal with a bare Map or List, 1 slot of numbers with no invariant, 1 program with no property to prove, 1 signal with no comment"), "{out}");
    // next says it once, before the bodies; freezing as it is closes it
    let (out, _) = soma(&d, &["init", "--agent"]);
    assert!(out.contains("AGENTS.md"), "{out}");
    let (out, _) = soma(&d, &["next", "app.cell"]);
    assert!(out.starts_with("skeleton") && out.contains("soma doctor --program app.cell") && out.contains("--freeze"), "{out}");
    let (out, code) = soma(&d, &["skeleton", "app.cell", "--freeze"]);
    assert_eq!(code, 0, "{out}");
    let (out, _) = soma(&d, &["next", "app.cell"]);
    assert!(out.starts_with("hole"), "{out}");
}

#[test]
fn the_starters_state_what_a_skeleton_states() {
    let d = dir("starters");
    let (out, code) = soma(&d, &["init", "--skeleton", "demo"]);
    assert_eq!(code, 0, "{out}");
    let (out, _) = soma(&d.join("demo"), &["doctor", "--program", "app.cell"]);
    assert!(out.contains("Nothing a skeleton usually states is missing"), "{out}");
    let (out, _) = soma(&d.join("demo"), &["next", "app.cell"]);
    assert!(out.starts_with("hole  Counter.reset"), "{out}");
}

#[test]
fn init_law_writes_a_whole_project_under_a_law_and_it_holds() {
    let d = dir("lawstarter");
    let (out, code) = soma(&d, &["init", "--law", "shop"]);
    assert_eq!(code, 0, "{out}");
    let p = d.join("shop");
    for f in ["lois/payments.law", "law.lock", "app.cell", "lib/shop.cell"] { assert!(p.join(f).exists(), "{f} missing: {out}"); }
    let (out, code) = soma(&p, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("All checks passed") && !out.contains("not stated"), "{out}");
    // the verdict names the law it was given under
    let (out, code) = soma(&p, &["verify", "app.cell", "--summary"]);
    assert_eq!(code, 0, "{out}");
    let last = out.lines().last().unwrap();
    assert!(last.starts_with("VERIFY OK — under law.lock ") && last.len() == "VERIFY OK — under law.lock ".len() + 8, "{last}");
    let (out, code) = soma(&p, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma(&p, &["law", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("4 rule(s): 1 monitored, 0 proved, 2 checked, 1 handled, 0 incomplete, 0 unserved; 0 without a test"), "{out}");
    let (out, _) = soma(&p, &["doctor", "--program", "app.cell"]);
    assert!(out.contains("Nothing a skeleton usually states is missing") && out.contains("3 of those 3 have a test"), "{out}");
    // without its law, the same program does not get the same last line
    std::fs::write(p.join("soma.toml"), std::fs::read_to_string(p.join("soma.toml")).unwrap().replace("files = [\"lois/payments.law\"]", "files = []")).unwrap();
    let (out, _) = soma(&p, &["verify", "app.cell", "--summary"]);
    assert!(!out.contains("under law.lock"), "{out}");
}

const TOML: &str = "[package]\nname = \"essai\"\n\n[law]\nfiles = [\"lois/f.law\"]\n";
const LAW: &str = r#"law F

entity Invoice "An invoice."
entity Account "An account."
state delegated(Invoice): Bool "Paid by the parent account."
state shut(Invoice): Bool "Closed."
state balance(Invoice): Int "What is left to pay."
action Payment(f: Invoice, n: Int) "The payment of an invoice."

rule "P-1" "A delegated or closed invoice is not paid, nor more than its balance"
    about Payment
    requires Payment(f, n): not delegated(f) and not shut(f) and balance(f) >= n
"#;

fn typed(maps: &str, pay: &str, card_frame: &str) -> String {
    format!(r#"
cell type Invoice = String
cell type Account = String
cell type Card {{ variants {{ Card {{ account: String, balance: Int }} }} }}
cell type Life {{ variants {{ Open  Shut }} }}

skeleton Invoices {{
    memory {{ cards: Map<Invoice, Card> [persistent] }}
    state life: Life {{
        initial: Open
        Open -> Shut
    }}
    face {{
        // The card of invoice `f`.
        signal card(f: Invoice) -> Card
            {card_frame}
        // Closes invoice `f`.
        signal shut(f: Invoice) -> String
            moves Open -> Shut
            silent "closing is no action of this law"
    }}
}}
skeleton Payments {{
    memory {{
        paid: Map<Invoice, Int> [persistent]
        invariant paid >= 0
    }}
{maps}
    face {{
        // Pays `n` on an invoice.
        {pay}
            writes paid
    }}
}}
"#)
}

fn project(name: &str, source: &str) -> std::path::PathBuf {
    let d = dir(name);
    std::fs::write(d.join("soma.toml"), TOML).unwrap();
    std::fs::write(d.join("lois/f.law"), LAW).unwrap();
    std::fs::write(d.join("app.cell"), source).unwrap();
    let (out, _) = soma(&d, &["law", "app.cell", "--lock"]);
    assert!(d.join("law.lock").exists(), "{out}");
    d
}

const GOOD_MAPS: &str = "    maps delegated(f) = Invoices.card(f).account != \"\"\n    maps shut(f) = Invoices.card(f).balance == 0\n    maps balance(f) = Invoices.card(f).balance";
const GOOD_PAY: &str = "signal pay(f: Invoice, n: Int) -> Int\n            realizes Payment(f, n)";

#[test]
fn a_maps_is_read_as_a_body() {
    let maps = "    maps delegated(f) = Invoices.card(f).parent != \"\"\n    maps shut(f) = get_status(f) == Shut\n    maps balance(f) = Invoices.card(42).balance";
    let d = project("mapsbody", &typed(maps, GOOD_PAY, "reads cards"));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "warnings in this edition: {out}");
    assert!(out.contains("in `maps delegated` of `Payments`") && out.contains("record type 'Card' has no field 'parent'"), "{out}");
    assert!(out.contains("in `maps shut` of `Payments`") && out.contains("cannot compare String and Variant"), "{out}");
    assert!(out.contains("in `maps balance` of `Payments`: Invoices.card(): argument 1 is a Int literal"), "{out}");
    // and a `maps` that is right says nothing
    let d = project("mapsgood", &typed(GOOD_MAPS, GOOD_PAY, "reads cards"));
    let (out, _) = soma(&d, &["check", "app.cell"]);
    assert!(!out.contains("in `maps"), "{out}");
}

#[test]
fn a_parameter_bound_to_an_entity_has_its_type() {
    let pay = "signal pay(a: Account, n: Int) -> Int\n            realizes Payment(a, n)";
    let d = project("entity", &typed(GOOD_MAPS, pay, "reads cards"));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("`Payments.pay` realizes `Payment(a, n)`: `a` is a `Account`, and the action's `f` is a `Invoice`"), "{out}");
    let d = project("entityok", &typed(GOOD_MAPS, GOOD_PAY, "reads cards"));
    let (out, _) = soma(&d, &["check", "app.cell"]);
    assert!(!out.contains("and the action's"), "{out}");
}

#[test]
fn a_maps_that_reads_through_a_signal_of_another_cell_that_writes_is_said() {
    let d = project("impure", &typed(GOOD_MAPS, GOOD_PAY, "writes cards\n            silent \"a cache\""));
    let (out, _) = soma(&d, &["check", "app.cell"]);
    assert!(out.contains("reads through `Invoices.card`, whose frame writes cards") && out.contains("E0135 in the next edition"), "{out}");
}

#[test]
fn a_state_only_silent_signals_change_is_said_to_the_owner_of_the_law() {
    let maps = "    maps delegated(f) = false\n    maps shut(f) = false\n    maps balance(f) = paid.get(f) ?? 0";
    let source = typed(maps, "signal pay(f: Invoice, n: Int) -> Int\n            silent \"bookkeeping\"", "reads cards");
    let d = project("silent", &source);
    let (out, _) = soma(&d, &["doctor", "--program", "app.cell"]);
    assert!(out.contains("For the owner of the law (1)") && out.contains("`balance` — changed only by `silent` signals: `Payments.pay`"), "{out}");
    let (out, _) = soma(&d, &["law", "app.cell", "--json", "--no-run"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert_eq!(v["hints"].as_array().unwrap().len(), 1, "{out}");
}

#[test]
fn a_skeleton_is_frozen_once_it_checks_and_verifies() {
    let d = dir("freeze");
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n\n[verify]\neventually = [\"lost\"]\n").unwrap();
    std::fs::write(d.join("app.cell"), BARE).unwrap();
    let (out, code) = soma(&d, &["skeleton", "app.cell", "--freeze"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("what is frozen is what was proven") && !d.join("skeleton.lock").exists(), "{out}");
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n").unwrap();
    std::fs::write(d.join("app.cell"), BARE.replace("reads units", "reads nothing_of_the_kind")).unwrap();
    let (out, code) = soma(&d, &["skeleton", "app.cell", "--freeze"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("does not check") && !d.join("skeleton.lock").exists(), "{out}");
}

#[test]
fn exemptions_that_share_their_words_are_a_template() {
    let d = dir("exempt");
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n\n[law]\nfiles = [\"lois/t.law\"]\n").unwrap();
    std::fs::write(d.join("lois/t.law"), "law T\n\nterm Movement \"Anything that moves stock.\"\n\nrule \"T-1\" \"Every movement is counted\"\n    about Movement\n").unwrap();
    let signals: String = (1..=6).map(|i| format!("        // Moves stock, way {i}.\n        signal move{i}(sku: String) -> Int\n            writes units\n            realizes Movement\n            exempt \"T-1\" \"does not apply to move{i}\"\n")).collect();
    std::fs::write(d.join("app.cell"), format!("skeleton Stock {{\n    memory {{\n        units: Map<String, Int> [persistent]\n        invariant units >= 0\n    }}\n    face {{\n{signals}    }}\n}}\n")).unwrap();
    let (out, _) = soma(&d, &["law", "app.cell", "--lock"]);
    assert!(d.join("law.lock").exists(), "{out}");
    let (out, _) = soma(&d, &["check", "app.cell"]);
    assert!(out.contains("6 of the program's 6 `exempt` share their reason with at least four others") && out.contains("the term is too broad"), "{out}");
}

#[test]
fn each_layer_has_its_page_to_read_first() {
    let d = dir("briefs");
    let (out, code) = soma(&d, &["docs", "skeleton", "--brief"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("# Before you write a skeleton") && out.lines().count() <= 60, "{}", out.lines().count());
    let (out, code) = soma(&d, &["docs", "law", "--brief"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("# Before you write a law") && out.lines().count() <= 60);
}
