//! RFC 0003 — the law as an abstract machine, the skeleton as its refinement.
//! Step 1: the law gets entities, states, actions and a formal reading of a
//! rule in a closed condition language; `law.lock` holds the list of laws.

use std::process::Command;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("soma_lawref_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("lois")).unwrap();
    d
}

fn soma(d: &std::path::Path, args: &[&str]) -> (String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(d).output().expect("failed to run soma");
    (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
}

const TOML: &str = "[package]\nname = \"essai\"\n\n[law]\nfiles = [\"lois/sanctions.law\"]\n";

const HEAD: &str = r#"law Sanctions

entity Holder "Whoever holds units of the fund, on its register."

state frozen(Holder): Bool "The holder is under an asset freeze."
state exposure(Holder): Int "What the fund has paid the holder, in cents."

action Payment(h: Holder, n: Int)
    "Cash paid by the fund to a holder."
action Freeze(h: Holder) "The holder is placed under an asset freeze."
"#;

/// A 3.3-style program: it realizes the term and serves the rule by hand.
const APP: &str = r#"
skeleton Fund {
    memory {
        status: Map<String, String> [persistent]
        paid: Map<String, Int> [persistent]
    }
    face {
        signal redeem(holder: String, n: Int) -> Int
            reads status
            writes paid
            realizes Payment
    }
}
flesh Fund {
    on redeem(holder, n) {
        require (status.get(holder) ?? "valid") != "frozen" else Frozen "{holder} is frozen" under "SAN-02"
        paid.set(holder, (paid.get(holder) ?? 0) + n)
        return n
    }
}
cell test T under "SAN-02" {
    rules { assert Fund.redeem("ana", 5) == 5 }
}
"#;

fn project(name: &str, rule: &str) -> std::path::PathBuf {
    let d = dir(name);
    std::fs::write(d.join("soma.toml"), TOML).unwrap();
    std::fs::write(d.join("lois/sanctions.law"), format!("{HEAD}\n{rule}\n")).unwrap();
    std::fs::write(d.join("app.cell"), APP).unwrap();
    d
}

fn rule(requires: &str) -> String {
    format!("rule \"SAN-02\" \"No payment to a frozen holder\"\n    about    Payment, Holder\n    source   \"Regulation (EU) 269/2014, art. 2\"\n    owner    \"compliance\"\n    {requires}\n")
}

#[test]
fn a_law_declares_entities_states_actions_and_the_formal_reading_of_a_rule() {
    let d = project("reads", &rule("requires Payment(h, n): not frozen(h) and (exposure(h) < 1000000 or n <= 0)"));
    let (out, _) = soma(&d, &["law", "app.cell", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    let law = &v["laws"][0];
    assert_eq!(law["entities"][0]["name"], "Holder", "{out}");
    assert_eq!(law["states"][0]["name"], "frozen");
    assert_eq!(law["states"][0]["entity"], "Holder");
    assert_eq!(law["states"][0]["type"], "Bool");
    assert_eq!(law["states"][1]["type"], "Int");
    assert_eq!(law["actions"][0]["name"], "Payment");
    assert_eq!(law["actions"][0]["params"][0]["type"], "Holder");
    assert_eq!(law["actions"][0]["definition"], "Cash paid by the fund to a holder.", "a definition may stand on the next line");
    let r = &v["rules"][0];
    assert_eq!(r["requires"]["action"], "Payment", "{out}");
    assert_eq!(r["requires"]["condition"], "not frozen(h) and (exposure(h) < 1000000 or n <= 0)");
    // an entity, a state and an action are terms: `about` may name them, `realizes Payment` still reads
    let (out, _) = soma(&d, &["law", "app.cell", "SAN-02"]);
    assert!(out.contains("requires Payment(h, n): not frozen(h)"), "{out}");
}

#[test]
fn a_condition_outside_the_closed_language_is_refused_with_the_nearest_form() {
    for (requires, says) in [
        ("requires Payment(h, n): exposure(h) + n < 1000000", "arithmetic"),
        ("requires Payment(h, n): max(n, 3) > 0", "one parameter"),
        ("requires Payment(h, n): forall x: frozen(x)", "quantifier"),
        ("requires Payment(h, n): frozen(h) == \"yes\"", "Bool"),
        ("requires Payment(h, n): not", "condition"),
        ("requires Payment(h, n) not frozen(h)", "`:`"),
    ] {
        let d = project("closed", &rule(requires));
        let (out, code) = soma(&d, &["check", "app.cell"]);
        assert_ne!(code, 0, "{requires}: {out}");
        assert!(out.contains("E0132"), "{requires}: {out}");
        assert!(out.contains(says), "{requires} — expected the message to speak of {says}: {out}");
        assert!(out.contains("lois/sanctions.law:"), "the message gives the file and the line: {out}");
    }
}

#[test]
fn a_formal_rule_names_only_what_the_law_declares() {
    for (requires, says) in [
        ("requires Refund(h, n): not frozen(h)", "action Refund("),
        ("requires Payment(h): not frozen(h)", "Payment(h: Holder, n: Int)"),
        ("requires Payment(h, n): not blocked(h)", "state blocked("),
        // a function of another language reads as a state nobody declared
        ("requires Payment(h, n): len(h) > 0", "state len("),
        ("requires Payment(h, n): not frozen(x)", "`x`"),
        ("requires Payment(h, n): not frozen(n)", "Holder"),
        ("requires Payment(h, n): exposure(h)", "Int"),
        ("requires Payment(h, n): h < 3", "`h`"),
    ] {
        let d = project("declared", &rule(requires));
        let (out, code) = soma(&d, &["check", "app.cell"]);
        assert_ne!(code, 0, "{requires}: {out}");
        assert!(out.contains("E0133"), "{requires}: {out}");
        assert!(out.contains(says), "{requires} — expected {says}: {out}");
    }
    // a state of an entity nobody declared, an action over a type the law does not have
    let d = dir("declared2");
    std::fs::write(d.join("soma.toml"), TOML).unwrap();
    std::fs::write(d.join("app.cell"), APP).unwrap();
    std::fs::write(d.join("lois/sanctions.law"), "law S\nstate frozen(Holder): Bool \"x\"\naction Payment(h: Holder, n: Money) \"y\"\nstate level(Holder): Float \"z\"\n").unwrap();
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_ne!(code, 0);
    assert!(out.contains("entity Holder"), "{out}");
    assert!(out.contains("Money"), "{out}");
    assert!(out.contains("Bool") && out.contains("Float"), "{out}");
}

#[test]
fn a_program_cannot_leave_a_law_its_lock_holds() {
    let d = project("leave", &rule(""));
    let (out, code) = soma(&d, &["law", "app.cell", "--lock"]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    // the table is removed from soma.toml: the program is no longer under its law
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n").unwrap();
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_ne!(code, 0, "a program that left its law passed: {out}");
    assert!(out.contains("E0140"), "{out}");
    assert!(out.contains("lois/sanctions.law") && out.contains("[law]"), "the message says what to restore: {out}");
    // a second law, locked, then dropped from the list
    std::fs::write(d.join("lois/autre.law"), "law Autre\nterm X \"x\"\n").unwrap();
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n\n[law]\nfiles = [\"lois/sanctions.law\", \"lois/autre.law\"]\n").unwrap();
    let _ = soma(&d, &["law", "app.cell", "--lock"]);
    std::fs::write(d.join("soma.toml"), TOML).unwrap();
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("E0140") && out.contains("lois/autre.law"), "{out}");
    // a project that never had a law and has no lock is untouched
    let e = dir("nolaw");
    std::fs::write(e.join("app.cell"), "cell A { on f() { return 1 } }\n").unwrap();
    let (out, code) = soma(&e, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
}

// ── step 2: the correspondence, in the skeleton ─────────────────────────

const FORMAL: &str = "requires Payment(h, n): not frozen(h)";

/// The program of the RFC: `maps`, `realizes Action(args)`, `silent`.
fn refined(maps: &str, redeem: &str, more: &str) -> String {
    format!(r#"
skeleton Fund {{
    memory {{
        status: Map<String, String> [persistent]
        paid: Map<String, Int> [persistent]
    }}

    {maps}

    face {{
        signal redeem(holder: String, n: Int) -> Int
            reads status
            writes paid
            {redeem}
        signal freeze(holder: String) -> String
            writes status
            realizes Freeze(holder)
        signal purge() -> Int
            writes paid
            silent "removes zero balances; no holder's state changes"
        {more}
    }}
}}
"#)
}

const MAPS: &str = r#"maps frozen(h) = (status.get(h) ?? "valid") == "frozen"
    maps exposure(h) = paid.get(h) ?? 0"#;

fn refined_project(name: &str, app: &str) -> std::path::PathBuf {
    let d = project(name, &rule(FORMAL));
    std::fs::write(d.join("app.cell"), app).unwrap();
    d
}

#[test]
fn a_skeleton_maps_the_states_of_the_law_and_binds_its_signals_to_actions() {
    let d = refined_project("corr", &refined(MAPS, "realizes Payment(holder, n)", ""));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    // the skeleton shows the correspondence, and the lock holds it
    let (out, _) = soma(&d, &["skeleton", "app.cell"]);
    assert!(out.contains("maps frozen(h) = (status.get(h) ?? \"valid\") == \"frozen\""), "{out}");
    assert!(out.contains("realizes Payment(holder, n)"), "{out}");
    assert!(out.contains("silent \"removes zero balances; no holder's state changes\""), "{out}");
    let (out, code) = soma(&d, &["skeleton", "app.cell", "--freeze"]);
    assert_eq!(code, 0, "{out}");
    std::fs::write(d.join("app.cell"), refined(MAPS, "realizes Payment(holder, n)", "").replace("== \"frozen\"", "== \"gel\"")).unwrap();
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_ne!(code, 0, "a changed `maps` passed the lock: {out}");
    assert!(out.contains("skeleton.lock"), "{out}");
}

#[test]
fn a_maps_defines_a_state_of_the_law_once_and_purely() {
    for (maps, code, says) in [
        ("maps blocked(h) = status.get(h) == \"frozen\"", "E0134", "frozen"),
        ("maps frozen(h) = status.get(h) == \"frozen\"\n    maps frozen(x) = false", "E0134", "twice"),
        ("maps frozen(h) = paid.set(h, 0) == ()", "E0135", "paid"),
        ("maps frozen(h) = now() > 0", "E0135", "now"),
        ("maps frozen(h) = paid.get(h) ?? 0", "E0135", "Bool"),
        ("maps exposure(h) = status.get(h) == \"x\"", "E0135", "Int"),
    ] {
        let d = refined_project("maps", &refined(maps, "realizes Payment(holder, n)", ""));
        let (out, exit) = soma(&d, &["check", "app.cell"]);
        assert_ne!(exit, 0, "{maps}: {out}");
        assert!(out.contains(code), "{maps} — expected {code}: {out}");
        assert!(out.contains(says), "{maps} — expected `{says}`: {out}");
    }
}

#[test]
fn a_binding_fits_the_action_and_a_silent_signal_says_why() {
    for (redeem, code, says) in [
        ("realizes Payment(holder)", "E0137", "Payment(h: Holder, n: Int)"),
        ("realizes Payment(n, holder)", "E0137", "Payment(h: Holder, n: Int)"),
        ("realizes Payment(holder, amount)", "E0137", "`amount`"),
        ("realizes Refund(holder, n)", "E0126", "Refund"),
        ("silent", "E0139", "silent \""),
        ("realizes Payment(holder, n)\n            silent \"both\"", "E0137", "silent"),
    ] {
        let d = refined_project("bind", &refined(MAPS, redeem, ""));
        let (out, exit) = soma(&d, &["check", "app.cell"]);
        assert_ne!(exit, 0, "{redeem}: {out}");
        assert!(out.contains(code), "{redeem} — expected {code}: {out}");
        assert!(out.contains(says), "{redeem} — expected `{says}`: {out}");
    }
    // a literal is an argument too
    let d = refined_project("bind_lit", &refined(MAPS, "realizes Payment(holder, 100)", ""));
    let (out, exit) = soma(&d, &["check", "app.cell"]);
    assert_eq!(exit, 0, "{out}");
}

#[test]
fn the_correspondence_belongs_to_a_skeleton() {
    let d = refined_project("plain", r#"
cell Fund {
    memory { status: Map<String, String> [persistent] }
    maps frozen(h) = status.get(h) == "frozen"
    on f() { return 1 }
}
"#);
    let (out, exit) = soma(&d, &["check", "app.cell"]);
    assert_ne!(exit, 0, "{out}");
    assert!(out.contains("skeleton") && out.contains("--split"), "{out}");
}

// ── steps 3 and 4: closure by effects, and the derived guard ────────────

const FLESH: &str = r#"
flesh Fund {
    on redeem(holder, n) {
        paid.set(holder, (paid.get(holder) ?? 0) + n)
        return n
    }
    on freeze(holder) {
        status.set(holder, "frozen")
        return "frozen"
    }
    on purge() { return 0 }
}
"#;

const TESTS: &str = r#"
cell test Frozen under "SAN-02" {
    rules {
        assert Fund.redeem("ana", 5) == 5
        assert Fund.freeze("ana") == "frozen"
        assert_fails Fund.redeem("ana", 7) matching "SAN-02"
        assert Fund.redeem("bob", 1) == 1
    }
}
"#;

/// Acceptance 1 of the design note: the body holds no guard, and the law is kept.
#[test]
fn the_guard_of_a_formal_rule_is_derived_and_refuses_before_any_write() {
    let app = format!("{}{}{}", refined(MAPS, "realizes Payment(holder, n)", ""), FLESH, TESTS);
    assert!(!FLESH.contains("require"), "the flesh writes no guard");
    let d = refined_project("derived", &app);
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma(&d, &["verify", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    // from the command line: allowed, frozen, refused — with the rule, its sentence and its source
    let (out, code) = soma(&d, &["run", "app.cell", "redeem", "ana", "5"]);
    assert_eq!(code, 0, "{out}");
    let (_, code) = soma(&d, &["run", "app.cell", "freeze", "ana"]);
    assert_eq!(code, 0);
    let (out, code) = soma(&d, &["run", "app.cell", "redeem", "ana", "7"]);
    assert_ne!(code, 0, "a payment to a frozen holder passed: {out}");
    assert!(out.contains("SAN-02") && out.contains("No payment to a frozen holder") && out.contains("Regulation (EU) 269/2014"), "{out}");
    assert!(out.contains("not frozen(holder)"), "the message shows the condition over the signal's own names: {out}");
    // nothing was written by the refused call
    let (out, _) = soma(&d, &["run", "app.cell", "redeem", "ana", "0"]);
    assert!(out.contains("SAN-02"), "{out}");
    let app2 = app.replace("on purge() { return 0 }", "on purge() { return paid.get(\"ana\") ?? 0 }");
    std::fs::write(d.join("app.cell"), app2).unwrap();
    let (out, _) = soma(&d, &["run", "app.cell", "purge"]);
    assert!(out.trim().ends_with('5'), "the refused payments left no write: {out}");
    // the table says the rule is checked by a derived guard, and by which signal
    let (out, _) = soma(&d, &["law", "app.cell", "SAN-02"]);
    assert!(out.contains("status   checked"), "{out}");
    assert!(out.contains("Fund.redeem (derived)"), "{out}");
}

/// A condition with `and`, `or`, an Int state and a parameter.
#[test]
fn a_derived_guard_follows_the_whole_condition() {
    let app = format!("{}{}", refined(MAPS, "realizes Payment(holder, n)", ""), FLESH);
    let d = project("cond", &rule("requires Payment(h, n): not frozen(h) and (exposure(h) < 10 or n <= 2)"));
    std::fs::write(d.join("app.cell"), app).unwrap();
    for (args, ok) in [(["ana", "9"], true), (["ana", "2"], true), (["ana", "3"], false), (["ana", "1"], true)] {
        let (out, code) = soma(&d, &["run", "app.cell", "redeem", args[0], args[1]]);
        assert_eq!(code == 0, ok, "redeem {args:?}: {out}");
    }
}

/// Acceptance 2 and 5: the forgotten path is closed on the frames.
#[test]
fn a_signal_that_writes_under_the_law_is_an_action_or_says_it_is_none() {
    let d = refined_project("closure", &refined(MAPS, "realizes Payment(holder, n)", "signal indemnify(holder: String, n: Int) -> Int\n            writes paid"));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("E0138"), "{out}");
    assert!(out.contains("Fund.indemnify") && out.contains("paid") && out.contains("exposure"), "it names the signal, the slot and the state: {out}");
    assert!(out.contains("realizes ") && out.contains("silent \""), "it gives the two clauses: {out}");
    // a signal that only reads, or writes a slot no `maps` reads, owes nothing
    let app = refined(MAPS, "realizes Payment(holder, n)", "signal look(holder: String) -> Int\n            reads paid").replace("paid: Map<String, Int> [persistent]", "paid: Map<String, Int> [persistent]\n        notes: Map<String, String> [persistent]")
        .replace("signal look(", "signal note(holder: String) -> Int\n            writes notes\n        signal look(");
    let d = refined_project("closure_ok", &app);
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    // a 3.3 `realizes Term` without arguments is not a binding
    let d = refined_project("closure_33", &refined(MAPS, "realizes Payment(holder, n)", "signal indemnify(holder: String, n: Int) -> Int\n            writes paid\n            realizes Payment"));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("realizes Payment(<h: Holder>, <n: Int>)"), "{out}");
}

/// Acceptance 3: a state a rule needs must be mapped, and the message gives the line.
#[test]
fn a_state_a_rule_needs_has_its_maps() {
    let d = refined_project("unmapped", &refined("maps exposure(h) = paid.get(h) ?? 0", "realizes Payment(holder, n)", ""));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("E0136"), "{out}");
    assert!(out.contains("maps frozen(h) = ") && out.contains("skeleton Fund") && out.contains("SAN-02"), "{out}");
}

/// The first program of the RFC's problem: a guard that refuses nothing no longer passes.
#[test]
fn the_empty_guard_of_phase_0_no_longer_serves_the_rule() {
    let app = format!("{}{}", refined(MAPS, "realizes Payment", ""), FLESH.replace("on redeem(holder, n) {", "on redeem(holder, n) {\n        require true else Frozen \"{holder} is frozen\" under \"SAN-02\""));
    let d = refined_project("empty", &app);
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("no longer serves it"), "{out}");
    // …and a test `under` a formal rule that shows no refusal is reported
    let app = format!("{}{}{}", refined(MAPS, "realizes Payment(holder, n)", ""), FLESH, "\ncell test Lax under \"SAN-02\" {\n    rules { assert Fund.redeem(\"ana\", 5) == 5 }\n}\n");
    let d = refined_project("lax", &app);
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("warning") && out.contains("Lax") && out.contains("assert_fails"), "{out}");
}

// ── step 5: what the owner of the law and the agent each read ───────────

/// Acceptance 4: the dictionary the owner reviews, and a brief that spares the agent the law.
#[test]
fn the_map_is_what_the_owner_reads_and_the_brief_what_the_agent_reads() {
    let app = format!("{}{}", refined(MAPS, "realizes Payment(holder, n)", ""), FLESH);
    let d = refined_project("map", &app);
    let (out, code) = soma(&d, &["law", "app.cell", "--map"]);
    assert_eq!(code, 0, "{out}");
    // each state and its definition in the program
    assert!(out.contains("frozen(Holder): Bool") && out.contains("The holder is under an asset freeze."), "{out}");
    assert!(out.contains("Fund: (status.get(h) ?? \"valid\") == \"frozen\""), "{out}");
    // each action and the signals that are it
    assert!(out.contains("Payment(h: Holder, n: Int)") && out.contains("Fund.redeem(holder, n)"), "{out}");
    assert!(out.contains("Freeze(h: Holder)") && out.contains("Fund.freeze(holder)"), "{out}");
    // each silent signal and its reason
    assert!(out.contains("Fund.purge") && out.contains("removes zero balances; no holder's state changes"), "{out}");
    let (out, code) = soma(&d, &["law", "app.cell", "--map", "--json"]);
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert_eq!(v["states"][0]["name"], "frozen");
    assert_eq!(v["states"][0]["maps"]["cell"], "Fund");
    assert_eq!(v["actions"][0]["realized_by"][0]["signal"], "Fund.redeem");
    assert_eq!(v["silent"][0]["signal"], "Fund.purge");
    // the brief: the action, the rule owed, the guard — and that it is not the agent's to write
    let (out, code) = soma(&d, &["skeleton", "app.cell", "Fund.redeem"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("realizes Payment(holder, n)"), "{out}");
    assert!(out.contains("\"SAN-02\"") && out.contains("No payment to a frozen holder") && out.contains("not frozen(holder)"), "{out}");
    assert!(out.contains("do not write"), "the brief says the guard is derived: {out}");
    // a signal the law does not concern says nothing of it
    let (out, _) = soma(&d, &["skeleton", "app.cell", "Fund.purge"]);
    assert!(out.contains("silent") && !out.contains("SAN-02"), "{out}");
    // an action nobody realizes, a state nobody maps: said to the owner
    let d2 = refined_project("map2", &app.replace("\n            realizes Freeze(holder)", "\n            silent \"an operator's switch\""));
    let (out, _) = soma(&d2, &["law", "app.cell", "--map"]);
    assert!(out.contains("Freeze(h: Holder)") && out.contains("no signal"), "{out}");
}

// ── step 6: the state of the law lives in another cell (the owner's option 3) ──

const TWO_LAW: &str = r#"law Sanctions
entity Holder "Whoever holds units."
state frozen(Holder): Bool "Under a freeze."
action Payment(h: Holder, n: Int) "Cash paid to a holder."
action Freeze(h: Holder) "Placed under a freeze."
action Thaw(h: Holder) "The freeze is lifted."
rule "SAN-02" "No payment to a frozen holder"
    about Payment, Holder
    source "Regulation (EU) 269/2014, art. 2"
    owner "compliance"
    requires Payment(h, n): not frozen(h)
"#;

/// A register holds who is frozen, as the state of a machine; another cell
/// pays, and knows the holder only through the order it settles.
fn two_cells(maps: &str, thaw: &str, settle: &str) -> String {
    format!(r#"
skeleton Register {{
    memory {{ log: Map<String, String> [persistent] }}
    state holder {{
        initial: valid
        valid -> frozen
        frozen -> valid
    }}
    {maps}
    face {{
        signal state_of(h: String) -> String
            reads holder
        signal note(h: String) -> String
            writes log
        signal freeze(h: String) -> String
            moves valid -> frozen
            realizes Freeze(h)
        signal thaw(h: String) -> String
            moves frozen -> valid
            {thaw}
    }}
}}
flesh Register {{
    on state_of(h) {{ return get_status(h) ?? "valid" }}
    on note(h) {{
        log.set(h, "seen")
        return h
    }}
    on freeze(h) {{
        transition(h, "valid", "frozen")
        return "frozen"
    }}
    on thaw(h) {{
        transition(h, "frozen", "valid")
        return "valid"
    }}
}}
skeleton Orders {{
    memory {{
        holder_of: Map<String, String> [persistent]
        paid: Map<String, Int> [persistent]
    }}
    face {{
        signal place(o: String, h: String) -> String
            writes holder_of
        signal settle(o: String, n: Int) -> Int
            reads holder_of
            writes paid
            {settle}
    }}
}}
flesh Orders {{
    on place(o, h) {{
        holder_of.set(o, h)
        return o
    }}
    on settle(o, n) {{
        let h = holder_of.get(o) ?? ""
        paid.set(h, (paid.get(h) ?? 0) + n)
        return n
    }}
}}
cell test AcrossCells under "SAN-02" {{
    rules {{
        assert Orders.place("o1", "ana") == "o1"
        assert Orders.place("o2", "bob") == "o2"
        assert Orders.settle("o1", 5) == 5
        assert Register.freeze("ana") == "frozen"
        assert_fails Orders.settle("o1", 5) matching "SAN-02"
        assert Orders.settle("o2", 5) == 5
        assert Register.thaw("ana") == "valid"
        assert Orders.settle("o1", 5) == 5
    }}
}}
"#)
}

fn two_project(name: &str, app: &str) -> std::path::PathBuf {
    let d = dir(name);
    std::fs::write(d.join("soma.toml"), TOML).unwrap();
    std::fs::write(d.join("lois/sanctions.law"), TWO_LAW).unwrap();
    std::fs::write(d.join("app.cell"), app).unwrap();
    d
}

const VIA: &str = "realizes Payment(o via holder_of, n)";
const STATE_OF: &str = "maps frozen(h) = state_of(h) == \"frozen\"";

#[test]
fn a_state_of_the_law_is_read_in_the_cell_that_holds_it_and_an_argument_through_a_link() {
    let d = two_project("across", &two_cells(STATE_OF, "realizes Thaw(h)", VIA));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("not frozen(o via holder_of)"), "the refusal shows the link: {out}");
    // the same state read directly from the machine, in the cell that owns it
    let d = two_project("across_direct", &two_cells("maps frozen(h) = (get_status(h) ?? \"valid\") == \"frozen\"", "realizes Thaw(h)", VIA));
    let (out, code) = soma(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    // the dictionary and the brief say where the state is held
    let (out, _) = soma(&d, &["law", "app.cell", "--map"]);
    assert!(out.contains("Register: ") && out.contains("Orders.settle(o via holder_of, n)"), "{out}");
    let (out, _) = soma(&d, &["skeleton", "app.cell", "Orders.settle"]);
    assert!(out.contains("not frozen(o via holder_of)"), "{out}");
}

#[test]
fn the_closure_follows_the_signals_a_maps_reads_through() {
    // `thaw` moves the machine `state_of` reads, which `maps frozen` reads: it owes a binding
    let d = two_project("closure2", &two_cells(STATE_OF, "", VIA));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("E0138") && out.contains("Register.thaw") && out.contains("frozen"), "{out}");
    assert!(!out.contains("Register.note"), "a signal that writes a slot no state reads owes nothing: {out}");
    // a `maps` may read through a signal only when that signal changes nothing
    let d = two_project("impure2", &two_cells("maps frozen(h) = note(h) == \"frozen\"", "realizes Thaw(h)", VIA));
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("E0135") && out.contains("note") && out.contains("writes"), "{out}");
    // a link that is not a slot of the signal's cell, a key that is not a parameter
    for (settle, says) in [("realizes Payment(o via nowhere, n)", "nowhere"), ("realizes Payment(x via holder_of, n)", "`x`")] {
        let d = two_project("via", &two_cells(STATE_OF, "realizes Thaw(h)", settle));
        let (out, code) = soma(&d, &["check", "app.cell"]);
        assert_ne!(code, 0, "{settle}: {out}");
        assert!(out.contains("E0137") && out.contains(says), "{settle}: {out}");
    }
}
