//! RFC 0006, part A: the verdict says what was not judged.

use std::process::Command;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("soma_fidelity_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("lois")).unwrap();
    d
}

fn soma(d: &std::path::Path, args: &[&str]) -> (String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(d).output().expect("failed to run soma");
    (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
}

const LAW: &str = r#"law Stock

entity Sku "A stock keeping unit."
term Count "An inventory count."
state units(Sku): Int "Units on the shelf."
action Move(s: Sku, n: Int) "Units of a sku are moved."

rule "S-1" "A move is of at least one unit"
    about Move
    requires Move(s, n): n > 0

rule "S-2" "Every count is signed by two people"
    about Count
"#;

const APP: &str = r#"
skeleton Shelf {
    memory {
        on_shelf: Map<String, Int> [persistent]
        invariant on_shelf >= 0
        trail: Map<String, String> [persistent]
    }
    maps units(s) = on_shelf.get(s) ?? 0
    face {
        // Moves `n` units of `sku`.
        signal move(sku: String, n: Int) -> Int
            writes on_shelf, trail
            calls level
            emits moved, lost
            realizes Move(sku, n)
        // The units of `sku` on the shelf.
        signal level(sku: String) -> Int
            reads on_shelf
        // Hears a move.
        signal moved(sku: String) -> Int
            reads on_shelf
        // Sets the shelf after a count.
        signal recount(sku: String, n: Int) -> Int
            writes on_shelf
            silent "a count sets the figure; no unit moves"
            realizes Count
    }
}
"#;

#[test]
fn the_last_line_says_what_was_not_judged() {
    let d = dir("line");
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n\n[law]\nfiles = [\"lois/s.law\"]\n").unwrap();
    std::fs::write(d.join("lois/s.law"), LAW).unwrap();
    std::fs::write(d.join("app.cell"), APP).unwrap();
    let (out, _) = soma(&d, &["law", "app.cell", "--lock"]);
    assert!(d.join("law.lock").exists(), "{out}");
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let last = out.lines().last().unwrap();
    // every number, zero included; what no tool judges first, what the frames show after
    assert_eq!(last, "conforms to its frames and to the formal rules of its law — fidelity not judged: 1 rule in a sentence (no tool reads them), 1 silent signal, 4 signals neither entry nor internal, 2 signals nobody calls, 1 event nobody hears, 1 slot read only by its writers, second paths not computed (no signal says entry), held states not computed (no slot says held)", "{out}");
    let (out, _) = soma(&d, &["check", "app.cell", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    let n = &v["not_judged"];
    // `move` and `recount` are in no `calls`; `moved` is heard, `level` is called
    assert_eq!(n["signals_nobody_calls"], serde_json::json!(["Shelf.move", "Shelf.recount"]), "{out}");
    assert_eq!(n["events_nobody_hears"], serde_json::json!(["lost"]), "{out}");
    // `trail` is written and read by nothing; `on_shelf` is read by a signal, a `maps` and an invariant
    assert_eq!(n["slots_read_only_by_their_writers"], serde_json::json!(["Shelf.trail"]), "{out}");
    assert_eq!(n["silent_signals"], serde_json::json!(["Shelf.recount"]), "{out}");
    // the verdict of verify carries it too, after the law it was given under
    let (out, code) = soma(&d, &["verify", "app.cell", "--summary"]);
    assert_eq!(code, 0, "{out}");
    let last = out.lines().last().unwrap();
    assert!(last.starts_with("VERIFY OK") && last.contains(" — under law.lock ") && last.contains("1 slot read only by its writers"), "{last}");
    let (out, _) = soma(&d, &["verify", "app.cell", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..=out.rfind('}').unwrap()]).expect(&out);
    assert_eq!(v["not_judged"]["rules_in_sentences"], 1, "{out}");
}

#[test]
fn a_program_with_no_law_and_no_skeleton_keeps_its_verdict() {
    let d = dir("plain");
    std::fs::write(d.join("app.cell"), "cell Counter {\n    memory { n: Map<String, Int> [persistent] }\n    on add(k: String) { n.set(k, (n.get(k) ?? 0) + 1)  return n.get(k) ?? 0 }\n}\n").unwrap();
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.trim_end().ends_with("All checks passed.") && !out.contains("fidelity"), "{out}");
    let (out, _) = soma(&d, &["verify", "app.cell"]);
    assert!(!out.contains("fidelity"), "{out}");
}

#[test]
fn a_program_that_does_not_check_gets_no_line_that_says_it_conforms() {
    let d = dir("errors");
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n").unwrap();
    std::fs::write(d.join("app.cell"), "skeleton S {\n    memory { a: Map<String, Int> [persistent] }\n    face {\n        // Reads.\n        signal get(k: String) -> Int\n            reads nothing_here\n    }\n}\n").unwrap();
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(!out.contains("conforms"), "{out}");
}

// ── step 2: `entry`, `internal`, `held` ──────────────────────────────

const LAW2: &str = r#"law Billing

entity Account "An account, named by its id."
state known(Account): Bool "The account exists."
action Lookup(a: Account) "An account is looked up by its id."

rule "A-1" "Only an account that exists is looked up"
    about Lookup
    requires Lookup(a): known(a)
"#;

fn declared(second_entry: &str, adjust_calls: &str, extra_memory: &str) -> String {
    format!(r#"
cell type Account = String

skeleton Accounts {{
    memory {{
        cards: Map<Account, String> [persistent]
        lines: Map<Account, Int> [persistent]
        balance: Map<Account, Int> [persistent]
        invariant balance >= 0
        held balance from lines by recompute
{extra_memory}    }}
    maps known(a) = cards.get(a) != ()
    face {{
        // Finds an account by its id.
        signal find(a: Account) -> String
            entry
            reads cards
            realizes Lookup(a)

        // The card of an account the caller already holds.
        signal card(a: Account) -> String
            internal "the caller holds the account"
            reads cards

        // What the account owes.
        signal owed(a: Account) -> Int
            {second_entry}
            reads balance

        // Adds a line to the account.
        signal adjust(a: Account, n: Int) -> Int
            entry
            writes lines
            calls {adjust_calls}
            realizes Lookup(a)

        // Rebuilds the balance of an account from its lines.
        signal recompute(a: Account) -> Int
            internal "called by whoever writes a line"
            reads lines
            writes balance

        // An audit nobody asks for.
        signal audit(a: Account) -> Int
            internal "meant for the nightly job"
            reads lines
    }}
}}
"#)
}

fn project2(name: &str, source: &str) -> std::path::PathBuf {
    let d = dir(name);
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n\n[law]\nfiles = [\"lois/b.law\"]\n").unwrap();
    std::fs::write(d.join("lois/b.law"), LAW2).unwrap();
    std::fs::write(d.join("app.cell"), source).unwrap();
    let (out, _) = soma(&d, &["law", "app.cell", "--lock"]);
    assert!(d.join("law.lock").exists(), "{out}");
    d
}

#[test]
fn a_skeleton_says_who_calls_a_signal_and_what_a_slot_is_held_from() {
    // `owed` is an entry that takes an Account and neither looks it up nor calls `find`;
    // `adjust` writes a source of `balance` and does not call its holder
    let d = project2("declared", &declared("entry", "card", ""));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "warnings in this edition: {out}");
    assert!(out.contains("`Accounts.owed` is an `entry` that takes an entity the law guards") && out.contains("a second path"), "{out}");
    assert!(out.contains("`Accounts.adjust` writes a source of a `held` slot and does not have its holder in its `calls`"), "{out}");
    assert!(out.contains("`Accounts.audit` is `internal` and no signal of the program has it in its `calls`"), "{out}");
    let last = out.lines().last().unwrap();
    assert!(last.contains("0 signals neither entry nor internal") && last.contains("2 signals nobody calls")
        && last.contains("1 second path") && last.contains("1 held state that can go stale"), "{last}");
    // (`recompute` is internal and nobody calls it either: that is the second of the two)
    let (out, _) = soma(&d, &["check", "app.cell", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert_eq!(v["not_judged"]["second_paths"], serde_json::json!(["Accounts.owed"]), "{out}");
    assert_eq!(v["not_judged"]["held_states_that_can_go_stale"], serde_json::json!(["Accounts.adjust"]), "{out}");

    // the same skeleton, put right: `owed` goes through the guarded signal, `adjust` calls the holder
    let d = project2("right", &declared("entry\n            calls find", "card, recompute", ""));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("second path —") && !out.contains("goes stale"), "{out}");
    let last = out.lines().last().unwrap();
    assert!(last.contains("0 second paths") && last.contains("0 held states that can go stale") && last.contains("1 signal nobody calls"), "{last}");

    // the lock holds the three clauses; the brief shows who calls the signal
    let (out, _) = soma(&d, &["skeleton", "app.cell"]);
    assert!(out.contains("held balance from lines by recompute") && out.contains("internal \"the caller holds the account\"") && out.lines().any(|l| l.trim() == "entry"), "{out}");
    let (out, _) = soma(&d, &["skeleton", "app.cell", "Accounts.card"]);
    assert!(out.contains("internal \"the caller holds the account\"   // called only by signals of the program"), "{out}");
}

#[test]
fn the_declarations_are_held_to_what_exists() {
    let d = project2("wrong", &declared("entry\n            internal \"both\"", "card, recompute", "        held nothing from lines by recompute\n        held balance from elsewhere by find\n"));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Accounts.owed` says both `entry` and `internal`") && out.contains("E0142"), "{out}");
    assert!(out.contains("`nothing` is not a slot of `Accounts`") && out.contains("E0144"), "{out}");
    assert!(out.contains("`elsewhere` is not a slot") && out.contains("`find` does not have `balance` in its `writes`"), "{out}");
    let d = project2("noreason", &declared("internal", "card, recompute", ""));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("`Accounts.owed` is `internal` without a reason") && out.contains("E0143"), "{out}");
}

#[test]
fn a_program_that_declares_nothing_says_what_it_could_not_compute() {
    let d = dir("undeclared");
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n").unwrap();
    std::fs::write(d.join("app.cell"), "skeleton S {\n    memory { a: Map<String, Int> [persistent] }\n    face {\n        // Reads.\n        signal get(k: String) -> Int\n            reads a\n    }\n}\n").unwrap();
    let (out, _) = soma(&d, &["check", "app.cell"]);
    let last = out.lines().last().unwrap();
    assert!(last.ends_with("second paths not computed (no signal says entry), held states not computed (no slot says held)"), "{last}");
}

// ── part B: `soma next` gives one thing, and can say "this is not yours" ──

const LAW3: &str = r#"law Shop

entity Order "An order, named by its id."
state closed(Order): Bool "Nothing more happens on the order."
action Refund(o: Order, n: Int) "Money given back on an order."

rule "R-1" "Nothing is refunded on a closed order"
    about Refund
    requires Refund(o, n): not closed(o)
"#;

const SHOP: &str = r#"
skeleton Shop {
    memory {
        status: Map<String, String> [persistent]
        back: Map<String, Int> [persistent]
        invariant back >= 0
        notes: Map<String, String> [persistent]
    }
    maps closed(o) = (status.get(o) ?? "open") == "closed"
    face {
        // Closes order `o`.
        signal close(o: String) -> String
            writes status
            silent "closing is bookkeeping"
        // Gives `n` back on order `o`.
        signal refund(o: String, n: Int) -> Int
            writes back
            realizes Refund(o, n)
        // Keeps a note on order `o`.
        signal note(o: String, text: String) -> String
            writes notes
    }
}
"#;

fn shop(name: &str) -> std::path::PathBuf {
    let d = dir(name);
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n\n[law]\nfiles = [\"lois/shop.law\"]\n").unwrap();
    std::fs::write(d.join("lois/shop.law"), LAW3).unwrap();
    std::fs::write(d.join("app.cell"), SHOP).unwrap();
    let (out, _) = soma(&d, &["law", "app.cell", "--lock"]);
    assert!(d.join("law.lock").exists(), "{out}");
    let (out, _) = soma(&d, &["init", "--agent"]);
    assert!(out.contains("AGENTS.md"), "{out}");
    d
}

#[test]
fn next_hands_out_what_waits_for_no_answer_and_stops_when_all_that_is_left_is_the_owners() {
    let d = shop("next");
    // the question is asked, and is not a task: `soma law --questions` exits 3
    let (out, code) = soma(&d, &["law", "app.cell", "--questions", "--no-run"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("silent-state:closed") && out.contains("`Shop.close`") && out.contains("waits for it: \"R-1\"") && out.contains("[[decision]]"), "{out}");
    // `note` owes nothing under the question: it is handed out, and the question is said
    let (out, code) = soma(&d, &["next", "app.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with("hole  Shop.note") && out.contains("1 question open for the owner of the law"), "{out}");
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::write(d.join("app.cell"), format!("{SHOP}\nflesh Shop {{\n    on note(o, text) {{\n        notes.set(o, text)\n        return text\n    }}\n}}\n")).unwrap();
    // all that is left — `close`, `refund`, the test of R-1 — waits for the answer: exit 3
    let (out, code) = soma(&d, &["next", "app.cell"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.starts_with("question  silent-state:closed") && out.contains("not a task"), "{out}");
    let (out, _) = soma(&d, &["next", "app.cell", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert_eq!(v["kind"], "question");
    assert_eq!(v["for"], "owner");
    assert_eq!(v["open_questions"], 1);

    // the owner answers "it is meant", and locks: the lock holds the decision
    std::fs::write(d.join("lois/decisions.toml"), "[[decision]]\nquestion = \"silent-state:closed\"\nanswer = \"Closing is bookkeeping: no action of the law.\"\nby = \"finance\"\ndate = \"2026-11-28\"\nfact = \"Shop.close\"\n").unwrap();
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 1, "a decision is not in force until the lock holds it: {out}");
    assert!(out.contains("lois/decisions.toml (new)"), "{out}");
    let (out, code) = soma(&d, &["law", "app.cell", "--lock"]);
    assert_eq!(code, 0, "{out}");
    let lock = std::fs::read_to_string(d.join("law.lock")).unwrap();
    assert!(lock.contains("lois/decisions.toml"), "{lock}");
    // the question is closed: what waited for it is handed out again
    let (out, code) = soma(&d, &["next", "app.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(!out.contains("question") && (out.starts_with("law  R-1") || out.starts_with("hole")), "{out}");
    // …and reopens when the fact it answered changes
    std::fs::write(d.join("app.cell"), std::fs::read_to_string(d.join("app.cell")).unwrap().replace("            writes notes\n", "            writes notes, status\n            silent \"notes may close\"\n")).unwrap();
    let (out, code) = soma(&d, &["law", "app.cell", "--questions", "--no-run"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("silent-state:closed") && out.contains("`Shop.note`"), "{out}");
}

#[test]
fn a_program_that_does_not_load_is_not_something_to_do_like_another() {
    let d = shop("load");
    std::fs::write(d.join("app.cell"), "skeleton Shop {\n    memory { a: Map<String Int> }\n").unwrap();
    let (out, code) = soma(&d, &["next", "app.cell"]);
    assert_eq!(code, 2, "{out}");
    assert!(out.starts_with("load  "), "{out}");
}

// ── part C: a perimeter — one law at a time ──────────────────────────

const PAY_LAW: &str = r#"law Paying

entity Invoice "An invoice, named by its id."
term Reminder "A reminder sent about an invoice."
state paid(Invoice): Bool "The invoice is paid."
action Payment(i: Invoice, n: Int) "An invoice is paid."

rule "P-1" "An invoice is paid once"
    about Payment
    requires Payment(i, n): not paid(i)

rule "P-2" "A payment is written to the journal"
    about Payment
"#;

const STOCK_LAW: &str = r#"law Stocking

term Count "An inventory count."

rule "K-1" "Every count is signed by two people"
    about Count
"#;

const TWO: &str = r#"
skeleton Invoices {
    memory {
        settled: Map<String, Bool> [persistent]
        journal: Map<String, String> [persistent]
    }
    maps paid(i) = settled.get(i) ?? false
    face {
        // Pays invoice `i`.
        signal pay(i: String, n: Int) -> Int
            writes settled, journal
            calls Ledger.post, stamp
            realizes Payment(i, n)
        // The stamp of the day.
        signal stamp() -> String
    }
}
skeleton Ledger {
    memory {
        entries: Map<String, Int> [persistent]
        invariant entries >= 0
    }
    face {
        // Posts `n` under `key`.
        signal post(key: String, n: Int) -> Int
            writes entries
    }
}
skeleton Shelf {
    memory {
        units: Map<String, Int> [persistent]
        invariant units >= 0
    }
    face {
        // Sets the shelf after a count.
        signal recount(sku: String, n: Int) -> Int
            writes units
            realizes Count
    }
}
"#;

fn two_laws(name: &str) -> std::path::PathBuf {
    let d = dir(name);
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n\n[law]\nfiles = [\"lois/paying.law\", \"lois/stocking.law\"]\n").unwrap();
    std::fs::write(d.join("lois/paying.law"), PAY_LAW).unwrap();
    std::fs::write(d.join("lois/stocking.law"), STOCK_LAW).unwrap();
    std::fs::write(d.join("app.cell"), TWO).unwrap();
    let (out, _) = soma(&d, &["law", "app.cell", "--lock"]);
    assert!(d.join("law.lock").exists(), "{out}");
    let (out, _) = soma(&d, &["init", "--agent"]);
    assert!(out.contains("AGENTS.md"), "{out}");
    d
}

#[test]
fn the_perimeter_of_a_law_is_its_rules_its_signals_and_its_frontier() {
    let d = two_laws("perimeter");
    let (out, code) = soma(&d, &["skeleton", "app.cell", "--index", "--law", "lois/paying.law"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("the perimeter of the law Paying (lois/paying.law): 2 rules (1 formal), 1 signal in 1 cell"), "{out}");
    // what it calls outside: the signature, and whether there is a body — `stamp` is of its cell but not of the law
    assert!(out.contains("frontier: 2 signals called outside the perimeter, in 1 other cell"), "{out}");
    assert!(out.contains("Ledger.post(key: String, n: Int) -> Int   [hole]") && out.contains("Invoices.stamp() -> String   [hole]"), "{out}");
    let (out, _) = soma(&d, &["skeleton", "app.cell", "--index", "--law", "stocking.law", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert_eq!(v["schema"], "soma.scope/1");
    assert_eq!(v["signals"], serde_json::json!(["Shelf.recount"]));
    assert_eq!(v["frontier"].as_array().unwrap().len(), 0, "{out}");
    // a law the program is not under restricts nothing: said, exit 4
    let (out, code) = soma(&d, &["skeleton", "app.cell", "--index", "--law", "lois/other.law"]);
    assert_eq!(code, 4, "{out}");
    assert!(out.contains("is not a law of this program") && out.contains("lois/paying.law, lois/stocking.law"), "{out}");
}

#[test]
fn every_command_takes_the_same_perimeter() {
    let d = two_laws("commands");
    // check: the verdict of the perimeter alone
    let (out, code) = soma(&d, &["check", "app.cell", "--law", "lois/stocking.law"]);
    assert_eq!(code, 0, "{out}");
    let last = out.lines().last().unwrap();
    assert!(last.contains("in this perimeter, fidelity not judged: 1 rule in a sentence") && last.contains("1 signal neither entry nor internal") && last.contains("1 signal nobody calls"), "{last}");
    assert!(out.contains("the perimeter of the law Stocking (lois/stocking.law): 1 rule, 1 signal in 1 cell, 1 of them with no body yet"), "{out}");
    // law: its rules only
    let (out, _) = soma(&d, &["law", "app.cell", "--law", "lois/stocking.law", "--no-run"]);
    assert!(out.contains("\"K-1\"") && !out.contains("\"P-1\"") && out.contains("1 rule(s)"), "{out}");
    // next: a thing of the perimeter and nothing else
    let (out, _) = soma(&d, &["next", "app.cell", "--law", "lois/stocking.law", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert_eq!(v["target"], "Shelf.recount", "{out}");
    let (out, _) = soma(&d, &["next", "app.cell", "--law", "lois/paying.law", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert!(["Invoices.pay", "P-1", "P-2"].contains(&v["target"].as_str().unwrap()), "{out}");
    // verify: the verdict names the perimeter
    let (out, code) = soma(&d, &["verify", "app.cell", "--law", "lois/stocking.law"]);
    assert_eq!(code, 0, "{out}");
    let last = out.lines().last().unwrap();
    assert!(last.starts_with("VERIFY OK") && last.contains("in the perimeter of the law Stocking, fidelity not judged: 1 rule in a sentence"), "{last}");
    assert!(!out.contains("Ledger"), "{out}");
    // the mirror: what the skeletons of the perimeter do not state
    let (out, _) = soma(&d, &["doctor", "--program", "app.cell", "--law", "lois/paying.law"]);
    assert!(out.contains("in the perimeter of the law Paying (1 signals in 1 cells)") || out.contains("in the perimeter of the law Paying"), "{out}");
    assert!(!out.contains("Shelf"), "{out}");
}
