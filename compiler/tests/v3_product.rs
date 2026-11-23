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
    // (it names the three machines: its cone is the whole product)
    let dir = project("limit2", "[verify.before.\"A.s59\"]\nrequires = [\"A.s58\", \"B.s58\", \"C.s58\"]\n", &src);
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("refused — more than 200000 product states"), "{out}");

    // a rule that names one machine is searched over its cone first: B and C
    // are left out (their status unknown, their moves free) and the 60 states
    // of A prove it — the full product was past the limit
    let dir = project("limit3", "[verify.before.\"A.s59\"]\nrequires = [\"A.s58\"]\n", &src);
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("✓ before A.s59 requires A.s58 — every run of the handlers, 60 product state(s) explored over A.m (60)"), "{out}");
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


// ── 3.2.4: what Orbite (156 machines) found ────────────────────────────

const TRY_TOML: &str = "[package]\nname = \"r\"\n[verify.before.\"Dossier.arretee\"]\nwhile = [\"Verrou.tenu\"]\n";

#[test]
fn v324_the_success_of_a_try_is_followed_into_its_branch() {
    // the lock is taken in a `try`, the transition follows only `p.error == ()`
    let d = project("try_ok", TRY_TOML, r#"// La règle : on n'entre dans Dossier.arretee qu'avec le verrou tenu. Le corps pose le verrou
// dans un `try` et ne transitionne que si la pose a réussi.
cell Verrou {
    memory { n: Map<String, Int> [persistent] }
    state verrou {
        initial: libre
        libre -> tenu
        tenu -> libre
    }
    on poser(r: String, ok: Bool) {
        require ok else Conflit "refusé"
        transition(r, "libre", "tenu")
        return "tenu"
    }
    on lever(r: String) {
        transition(r, "tenu", "libre")
        return "libre"
    }
}
cell Dossier {
    memory { n: Map<String, Int> [persistent] }
    state dossier {
        initial: accord
        accord -> arretee
        arretee -> accord
    }
    on arreter(r: String, ok: Bool) {
        let p = try { Verrou.poser(r, ok) }
        if p.error == () {
            transition(r, "accord", "arretee")
            return "arretee"
        }
        return "refuse"
    }
    on rouvrir(r: String) {
        transition(r, "arretee", "accord")
        let _ = Verrou.lever(r)
        return "accord"
    }
}
"#);
    let (out, _) = soma_in(&d, &["verify", "app.cell"]);
    assert!(out.contains("✓ entering Dossier.arretee") && out.contains("VERIFY OK"), "{out}");
    // transitioning on FAILURE still breaks the rule
    let d = project("try_bad", TRY_TOML, r#"// La règle : on n'entre dans Dossier.arretee qu'avec le verrou tenu. Le corps pose le verrou
// dans un `try` et ne transitionne que si la pose a réussi.
cell Verrou {
    memory { n: Map<String, Int> [persistent] }
    state verrou {
        initial: libre
        libre -> tenu
        tenu -> libre
    }
    on poser(r: String, ok: Bool) {
        require ok else Conflit "refusé"
        transition(r, "libre", "tenu")
        return "tenu"
    }
    on lever(r: String) {
        transition(r, "tenu", "libre")
        return "libre"
    }
}
cell Dossier {
    memory { n: Map<String, Int> [persistent] }
    state dossier {
        initial: accord
        accord -> arretee
        arretee -> accord
    }
    on arreter(r: String, ok: Bool) {
        let p = try { Verrou.poser(r, ok) }
        if p.error != () {
            transition(r, "accord", "arretee")
            return "arretee"
        }
        return "refuse"
    }
    on rouvrir(r: String) {
        transition(r, "arretee", "accord")
        let _ = Verrou.lever(r)
        return "accord"
    }
}
"#);
    let (out, _) = soma_in(&d, &["verify", "app.cell"]);
    assert!(out.contains("✗ entering Dossier.arretee"), "{out}");
}

const LINK_TOML: &str = "[package]\nname = \"r\"\n[verify.before.\"Ordre.execute\"]\nwhile = [\"Compte.ouvert\"]\nvia = \"Ordre.compte_de\"\n";

#[test]
fn v324_a_link_key_made_once_by_the_signal_is_accepted() {
    let d = project("lien_ok", LINK_TOML, r#"// Le lien est écrit une fois, à la création, à une clé que le signal fabrique lui-même.
cell Compte {
    memory { n: Map<String, Int> [persistent] }
    state compte {
        initial: ouvert
        ouvert -> ferme
    }
    on ouvrir(c: String) { n.set(c, 1)  return "ouvert" }
    on fermer(c: String) { transition(c, "ouvert", "ferme")  return "ferme" }
    on etat(c: String) { return get_status(c) ?? "ouvert" }
}
cell Ordre {
    memory {
        compte_de: Map<String, String> [persistent]
        compteur: Map<String, Int> [persistent]
    }
    state ordre {
        initial: recu
        recu -> execute
    }
    on creer(c: String) {
        let k = (compteur.get("n") ?? 0) + 1
        compteur.set("n", k)
        let id = "OR-{k}"
        require compte_de.get(id) == () else Existe "ordre {id}"
        compte_de.set(id, c)
        return id
    }
    on executer(id: String) {
        let c = compte_de.get(id) ?? ""
        require (Compte.etat(c)) == "ouvert" else EtatInvalide "compte fermé"
        transition(id, "recu", "execute")
        return "execute"
    }
}
"#);
    let (out, _) = soma_in(&d, &["verify", "app.cell"]);
    assert!(out.contains("✓ entering Ordre.execute"), "{out}");
    for (name, src) in [("lien_double", r#"// Le lien est écrit une fois, à la création, à une clé que le signal fabrique lui-même.
cell Compte {
    memory { n: Map<String, Int> [persistent] }
    state compte {
        initial: ouvert
        ouvert -> ferme
    }
    on ouvrir(c: String) { n.set(c, 1)  return "ouvert" }
    on fermer(c: String) { transition(c, "ouvert", "ferme")  return "ferme" }
    on etat(c: String) { return get_status(c) ?? "ouvert" }
}
cell Ordre {
    memory {
        compte_de: Map<String, String> [persistent]
        compteur: Map<String, Int> [persistent]
    }
    state ordre {
        initial: recu
        recu -> execute
    }
    on creer(c: String) {
        let k = (compteur.get("n") ?? 0) + 1
        compteur.set("n", k)
        let id = "OR-{k}"
        require compte_de.get(id) == () else Existe "ordre {id}"
        compte_de.set(id, c)
        compte_de.set(id, "autre")
        return id
    }
    on executer(id: String) {
        let c = compte_de.get(id) ?? ""
        require (Compte.etat(c)) == "ouvert" else EtatInvalide "compte fermé"
        transition(id, "recu", "execute")
        return "execute"
    }
}
"#), ("lien_reaffecte", r#"// Le lien est écrit une fois, à la création, à une clé que le signal fabrique lui-même.
cell Compte {
    memory { n: Map<String, Int> [persistent] }
    state compte {
        initial: ouvert
        ouvert -> ferme
    }
    on ouvrir(c: String) { n.set(c, 1)  return "ouvert" }
    on fermer(c: String) { transition(c, "ouvert", "ferme")  return "ferme" }
    on etat(c: String) { return get_status(c) ?? "ouvert" }
}
cell Ordre {
    memory {
        compte_de: Map<String, String> [persistent]
        compteur: Map<String, Int> [persistent]
    }
    state ordre {
        initial: recu
        recu -> execute
    }
    on creer(c: String) {
        let k = (compteur.get("n") ?? 0) + 1
        compteur.set("n", k)
        let id = "OR-{k}"
        id = "OR-0"
        require compte_de.get(id) == () else Existe "ordre {id}"
        compte_de.set(id, c)
        return id
    }
    on executer(id: String) {
        let c = compte_de.get(id) ?? ""
        require (Compte.etat(c)) == "ouvert" else EtatInvalide "compte fermé"
        transition(id, "recu", "execute")
        return "execute"
    }
}
"#)] {
        let d = project(name, LINK_TOML, src);
        let (out, _) = soma_in(&d, &["verify", "app.cell"]);
        assert!(out.contains("needs the link written once"), "{name}: {out}");
    }
}

#[test]
fn v324_homonymous_handlers_of_two_cells_are_no_recursion() {
    let d = project("homonymes", "[package]\nname = \"r\"\n", r#"// NE DEVRAIT PAS être signalé : Alertes.ouvrir appelle Escalades.ouvrir, qui
// n'appelle rien. Aujourd'hui : « recursive call through … a qualified
// `Cell.ouvrir(…)` », puis « X calls `ouvrir`, which may not terminate » pour
// chaque appelant de n'importe quel `ouvrir` du programme (19 avertissements
// dans Rempart, 22 cells).
cell Escalades {
    on ouvrir(id: String) { return "manager" }
}
cell Alertes {
    on ouvrir(id: String) { return Escalades.ouvrir(id) }
}
cell Confirmations {
    on ouvrir(id: String) { return "envoyee" }
    on recevoir(id: String) { return Alertes.ouvrir(id) }
}
"#);
    let (out, _) = soma_in(&d, &["verify", "app.cell"]);
    assert!(!out.contains("recursive call") && !out.contains("may not terminate"), "{out}");
    // a real cycle across cells is named as one
    let d = project("cycle", "[package]\nname = \"r\"\n", r#"// DOIT être signalé : A.f -> B.f -> A.f ne termine jamais.
// Aujourd'hui signalé, mais seulement grâce à hidden_self_call qui prend
// tout `X.f(…)` appelé depuis `on f` pour un auto-appel. call_graph ne voit
// pas le cycle : il indexe par nom nu et filtre `name != on.signal_name`.
cell A {
    on f(n: Int) { return B.f(n) }
}
cell B {
    on f(n: Int) { return A.f(n) }
}
"#);
    let (out, _) = soma_in(&d, &["verify", "app.cell"]);
    assert!(out.contains("mutual recursion A.f → B.f → A.f"), "{out}");
}

// ── the search at the size of a large application (Orbite) ──────────────────

/// A sweep: N handlers tried one after the other, each outcome read at once
/// and never again. The outcome of each `try` was remembered to the end of
/// the handler: every state of the search was held 2^N times (Orbite's
/// `Horloge.battre`, N = 20: `soma verify` of its test files did not finish).
#[test]
fn a_sweep_of_tries_does_not_double_the_search_at_each_try() {
    let n = 24;
    let mut sweep = String::new();
    for i in 0..n {
        sweep += &format!("        let r{i} = try {{ Log.note(id, \"step {i}\") }}\n        if r{i}.error != () {{ total = total + 1 }} else {{ Log.note(id, \"failed: {{r{i}.error}}\") }}\n");
    }
    let source = format!(r#"{TWO}
cell Log {{
    face {{ signal note(id: String, what: String) -> Int }}
    memory {{ lines: Map<String, String> }}
    on note(id: String, what: String) {{ lines.set(id, what)  return 1 }}
}}

cell Sweep {{
    face {{ signal run(id: String) -> Int }}
    on run(id: String) {{
        let total = 0
{sweep}        Pay.settle(id)
        return total
    }}
}}
"#);
    let dir = project("sweep", "[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n", &source);
    let started = std::time::Instant::now();
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    let took = started.elapsed();
    assert!(out.contains("✓ before Pay.paid requires Approval.signed_off"), "{out}");
    assert_eq!(code, 0, "{out}");
    assert!(took.as_secs() < 20, "24 tries in a row: verify took {took:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Rules that name the same cells are searched together: each must be given
/// the words its own search gives — the same count of explored states.
#[test]
fn rules_of_one_cone_searched_together_answer_as_each_alone() {
    let rules = [
        "[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n",
        "[verify.before.\"Pay.open\"]\nrequires = [\"Approval.asked\"]\n",
    ];
    let line = |out: &str, what: &str| out.lines().find(|l| l.contains(what)).unwrap_or_else(|| panic!("no line `{what}` in:\n{out}")).to_string();
    let together = project("cone_together", &format!("{}{}", rules[0], rules[1]), TWO);
    let (both, _) = soma_in(&together, &["verify", "app.cell"]);
    for (rule, what) in [(rules[0], "before Pay.paid requires"), (rules[1], "before Pay.open requires")] {
        let alone = project("cone_alone", rule, TWO);
        let (one, _) = soma_in(&alone, &["verify", "app.cell"]);
        assert_eq!(line(&both, what), line(&one, what), "together:\n{both}\nalone:\n{one}");
        assert!(line(&one, what).contains("product state(s) explored"), "{one}");
        let _ = std::fs::remove_dir_all(&alone);
    }
    let _ = std::fs::remove_dir_all(&together);
}

/// A handler that reads the status of N ids it does not move: each read
/// decides "this id is the followed one" or not, and both answers went on —
/// 2^N copies of every state. Two states that differ only in such a choice
/// are the one state where it is not decided.
#[test]
fn reading_the_status_of_many_ids_does_not_double_the_search_at_each_read() {
    let n = 22;
    let params: Vec<String> = (0..n).map(|i| format!("a{i}: String")).collect();
    let mut body = String::new();
    for i in 0..n { body += &format!("        if Approval.approved(a{i}) {{ total = total + 1 }}\n"); }
    let source = format!(r#"{TWO}
cell Census {{
    face {{ signal count(id: String, {params}) -> Int }}
    on count(id: String, {params}) {{
        let total = 0
{body}        Pay.settle(id)
        return total
    }}
}}
"#, params = params.join(", "));
    let dir = project("census", "[verify.before.\"Pay.paid\"]\nrequires = [\"Approval.signed_off\"]\n", &source);
    let started = std::time::Instant::now();
    let (out, code) = soma_in(&dir, &["verify", "app.cell"]);
    let took = started.elapsed();
    assert!(out.contains("✓ before Pay.paid requires Approval.signed_off"), "{out}");
    assert_eq!(code, 0, "{out}");
    assert!(took.as_secs() < 20, "the status of {n} ids read in a row: verify took {took:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
