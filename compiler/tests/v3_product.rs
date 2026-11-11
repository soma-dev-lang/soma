// 3.0 — B2: a property over the PRODUCT of the program's machines.
// `[verify.before."Pay.paid"] requires = ["Approval.signed_off"]` follows one
// id through every machine at once, a handler invocation being one atomic
// step; `assuming = "amount > 500"` leaves out the edges whose guard cannot
// hold then. The search is exhaustive, or it is refused.

use std::process::Command;

fn soma_in(dir: &std::path::Path, args: &[&str]) -> (String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(dir)
        .env_remove("SOMA_EDITION").output().expect("failed to run soma");
    (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
}

fn project(name: &str, toml: &str, source: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_v3p_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("soma.toml"), toml).unwrap();
    std::fs::write(dir.join("app.cell"), source).unwrap();
    dir
}

const TWO: &str = r#"cell Approval {
    face {
        signal ask(id: String) -> String
        signal sign(id: String) -> String
        signal approved(id: String) -> Bool
    }
    state review {
        initial: none
        none -> asked
        asked -> signed_off
    }
    on ask(id: String) { transition(id, "asked")  return "asked" }
    on sign(id: String) { transition(id, "signed_off")  return "signed" }
    on approved(id: String) { return get_status(id) == "signed_off" }
}

cell Pay {
    face {
        signal open(id: String) -> String
        signal settle(id: String) -> String
    }
    state payment {
        initial: draft
        draft -> open
        open -> paid
    }
    on open(id: String) {
        Approval.ask(id)
        transition(id, "open")
        return "open"
    }
    on settle(id: String) {
        if Approval.approved(id) {
            transition(id, "paid")
            return "paid"
        }
        return "waiting"
    }
}
"#;

const PROPERTY: &str = "[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n";

#[test]
fn b2_a_rule_between_two_machines_is_proven_on_their_product() {
    let dir = project("holds", PROPERTY, TWO);
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("✓ before Pay.paid requires Approval.signed_off — every run of the handlers, 6 product state(s) explored over Approval.review (3) × Pay.payment (3)"), "{out}");
    assert!(out.contains("VERIFY OK"), "{out}");

    // the handler stops asking: the run that breaks the rule is shown
    let dir = project("broken", PROPERTY, &TWO.replace("if Approval.approved(id) {", "if id != \"\" {"));
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("✗ before Pay.paid requires Approval.signed_off: a run of the program's handlers enters `paid` without it"), "{out}");
    assert!(out.contains("Pay.open → Pay.settle  ← enters paid here"), "{out}");
    assert!(out.contains("VERIFY FAILED"), "{out}");

    // a `require` on the other machine's state is read as the `if` is
    let dir = project("require", PROPERTY, &TWO.replace("        if Approval.approved(id) {\n            transition(id, \"paid\")\n            return \"paid\"\n        }\n        return \"waiting\"",
        "        require Approval.approved(id) else NotSigned\n        transition(id, \"paid\")\n        return \"paid\""));
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("✓ before Pay.paid requires Approval.signed_off"), "{out}");

    // a `return` inside a `match` arm ends the handler THERE: the state it
    // leaves is reachable, whatever the statements after the match would do
    let early = TWO.replace("    on open(id: String) {\n        Approval.ask(id)\n        transition(id, \"open\")\n        return \"open\"\n    }",
        "    on open(id: String, fast: Bool) {\n        Approval.ask(id)\n        transition(id, \"open\")\n        let r = match fast {\n            true -> {\n                transition(id, \"paid\")\n                return \"fast\"\n            }\n            _ -> \"slow\"\n        }\n        return r\n    }")
        .replace("signal open(id: String) -> String", "signal open(id: String, fast: Bool) -> String");
    let dir = project("early", PROPERTY, &early);
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("Pay.open  ← enters paid here"), "{out}");

    // another id's approval is not this id's: a constant id in the test is caught
    let dir = project("other_id", PROPERTY, &TWO.replace("if Approval.approved(id) {", "if Approval.approved(\"vip\") {"));
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("✗ before Pay.paid requires Approval.signed_off"), "{out}");
}

#[test]
fn b2_the_product_follows_emits_tasks_and_names_what_it_cannot_resolve() {
    // the approval arrives by an event: the listener in the other cell is part of the step
    let by_event = TWO.replace("    on sign(id: String) { transition(id, \"signed_off\")  return \"signed\" }",
        "    on sign(id: String) { transition(id, \"signed_off\")  emit signed(id)  return \"signed\" }")
        .replace("    on settle(id: String) {\n        if Approval.approved(id) {\n            transition(id, \"paid\")\n            return \"paid\"\n        }\n        return \"waiting\"\n    }",
                 "    on signed(id: String) { transition(id, \"paid\") }")
        .replace("        signal settle(id: String) -> String\n", "");
    let dir = project("emit", PROPERTY, &by_event);
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    // `signed` is a handler like any other: called directly, it pays without the sign-off
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("Pay.signed  ← enters paid here"), "{out}");

    // a [task] commits between its steps: the rule must hold for each transition alone
    let task = TWO.replace("    on settle(id: String) {\n        if Approval.approved(id) {",
        "    on settle(id: String) [task] {\n        let note = think(\"ok?\")\n        if Approval.approved(id) {");
    let dir = project("task", PROPERTY, &task);
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_ne!(code, 0, "a [task]'s transition is a step of its own, taken from any state: {out}");
    assert!(out.contains("✗ before Pay.paid requires Approval.signed_off"), "{out}");

    // a state no machine has, and a bare name two machines share
    let dir = project("names", "[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signd_off\"]\n", TWO);
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("`Approval.signd_off` is not a state of any machine"), "{out}");
}

#[test]
fn b2_the_search_is_exhaustive_or_refused() {
    // three machines of 60 states, each stepped on its own: 216,000 product states
    let mut src = String::new();
    for cell in ["A", "B", "C"] {
        let edges: String = (0..59).map(|i| format!("        s{} -> s{}\n", i, i + 1)).collect();
        // (the target is the caller's: any edge out of the current state)
        src.push_str(&format!("cell {cell} {{\n    face {{ signal step_{cell}(id: String, to: String) -> Int }}\n    state m {{\n        initial: s0\n{edges}    }}\n    on step_{cell}(id: String, to: String) {{\n        transition(id, to)\n        return 1\n    }}\n}}\n\n"));
    }
    let dir = project("limit", "[verify.before.\"A.s59\"]\nrequires = [\"B.s59\"]\n", &src);
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    // A reaches s59 on its own long before the limit: a counterexample is a proof of the failure
    assert!(out.contains("✗ before A.s59 requires B.s59"), "{out}");

    // a property that HOLDS on a space past the limit is refused, not passed
    let dir = project("limit2", "[verify.before.\"A.s59\"]\nrequires = [\"A.s58\"]\n", &src);
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("refused — more than 200000 product states"), "{out}");
}

#[test]
fn b2_assuming_leaves_out_the_edges_whose_guard_cannot_hold() {
    let src = r#"cell Payments {
    face {
        signal authorize(id: String, amount: Int) -> String
        signal park(id: String) -> String
        signal sign_off(id: String) -> String
        signal settle(id: String) -> String
    }
    state flow {
        initial: proposed
        proposed -> authorized { guard { amount <= 500 } }
        proposed -> pending_signoff
        pending_signoff -> signed_off
        signed_off -> authorized
        authorized -> paid
    }
    on authorize(id: String, amount: Int) { transition(id, "authorized")  return "authorized" }
    on park(id: String) { transition(id, "pending_signoff")  return "parked" }
    on sign_off(id: String) { transition(id, "signed_off")  return "signed_off" }
    on settle(id: String) { transition(id, "paid")  return "paid" }
}
"#;
    let toml = "[verify.before.paid]\nrequires = [\"signed_off\"]\nassuming = \"amount > 500\"\n";
    let dir = project("assume", toml, src);
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("✓ requires('paid' only after one of [signed_off]), assuming amount > 500"), "{out}");
    assert!(out.contains("left out (proposed -> authorized)"), "{out}");

    // without the assumption the small payment's path is a counterexample
    let dir = project("unassumed", "[verify.before.paid]\nrequires = [\"signed_off\"]\n", src);
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_ne!(code, 0, "{out}");

    // a guard the assumption does not rule out stays: `amount <= 900` can hold when amount > 500
    let dir = project("weak", toml, &src.replace("amount <= 500", "amount <= 900"));
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("no guard of this machine is ruled out"), "{out}");

    // an assumption is a conjunction of integer comparisons, and can hold
    for (bad, needle) in [("amount > 500 || vip", "compares variables with integers"), ("amount > 5 && amount < 3", "can never hold")] {
        let dir = project("badassume", &format!("[verify.before.paid]\nrequires = [\"signed_off\"]\nassuming = \"{bad}\"\n"), src);
        let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
        assert_ne!(code, 0, "{out}");
        assert!(out.contains(needle), "{bad}: {out}");
    }

    // the same assumption on a product property
    let toml = "[verify.before.\"Payments.paid\"]\nrequires = [\"Payments.signed_off\"]\nassuming = \"amount > 500\"\n";
    let dir = project("assume_product", toml, src);
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("✓ before Payments.paid requires Payments.signed_off, assuming amount > 500"), "{out}");
}

#[test]
fn b2_treasury_proves_its_headline_property() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let (out, code) = soma_in(&repo.join("treasury"), &["verify", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("✓ requires('paid' only after one of [signed_off]), assuming amount > 500"), "{out}");
    assert!(out.contains("VERIFY OK"), "{out}");
}
