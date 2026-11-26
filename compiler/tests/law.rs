//! RFC 0002 — the law: business rules written outside the code, and the
//! program held to them.

use std::process::Command;

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("soma_law_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("lois")).unwrap();
    d
}

fn soma(d: &std::path::Path, args: &[&str]) -> (String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(d).output().expect("failed to run soma");
    (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
}

const LAW: &str = r#"// Ce qu'un fonds ne fait pas avec un porteur visé par des sanctions.
law Sanctions

term Porteur  "Celui qui détient des parts, inscrit au registre."
term Paiement "Un versement d'espèces du fonds."

rule "SAN-02" "Aucun paiement vers un porteur bloqué"
    about  Paiement, Porteur
    source "Règlement (UE) 269/2014, art. 2"
    owner  "conformite"
"#;

const TOML: &str = "[package]\nname = \"essai\"\n\n[law]\nfiles = [\"lois/sanctions.law\"]\n";

/// `payer`, `frais` (exempt) and — with `extra` — one more signal of the skeleton.
fn skeleton(extra: &str) -> String {
    format!(r#"
skeleton Caisse {{
    memory {{
        etat_de: Map<String, String> [persistent]
        solde: Map<String, Int> [persistent]
    }}
    face {{
        signal payer(porteur: String, n: Int) -> Int
            reads etat_de
            writes solde
            realizes Paiement
        signal frais(n: Int) -> Int
            writes solde
            realizes Paiement
            exempt "SAN-02" "paie la société de gestion, pas un porteur"
        signal bloquer(porteur: String) -> String
            writes etat_de
{extra}
    }}
}}
"#)
}

fn flesh(extra: &str, tests: &str) -> String {
    format!(r#"
use app
flesh Caisse {{
    on payer(porteur, n) {{
        require (etat_de.get(porteur) ?? "valide") != "bloque" else Bloque "{{porteur}} est bloqué" under "SAN-02"
        solde.set(porteur, (solde.get(porteur) ?? 0) + n)
        return n
    }}
    on frais(n) {{
        solde.set("sgp", (solde.get("sgp") ?? 0) + n)
        return n
    }}
    on bloquer(porteur) {{
        etat_de.set(porteur, "bloque")
        return "bloque"
    }}
{extra}
}}
{tests}
"#)
}

const TEST: &str = r#"
cell test PasDePaiementAuBloque under "SAN-02" {
    rules {
        assert Caisse.payer("a", 10) == 10
        let _ = Caisse.bloquer("a")
        assert_fails Caisse.payer("a", 10) matching "Bloque"
    }
}
"#;

fn project(name: &str, law: &str, skel: &str, prod: &str) -> std::path::PathBuf {
    let d = dir(name);
    std::fs::write(d.join("soma.toml"), TOML).unwrap();
    std::fs::write(d.join("lois/sanctions.law"), law).unwrap();
    std::fs::write(d.join("app.cell"), skel).unwrap();
    std::fs::write(d.join("prod.cell"), prod).unwrap();
    d
}

#[test]
fn a_program_aligned_on_its_law_passes_and_the_table_says_how() {
    let d = project("aligned", LAW, &skeleton(""), &flesh("", TEST));
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma(&d, &["test", "prod.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");

    let (out, code) = soma(&d, &["law", "prod.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("law Sanctions (lois/sanctions.law) — not locked yet"), "{out}");
    assert!(out.contains("status   checked"), "{out}");
    assert!(out.contains("served   Caisse.payer (checked)"), "{out}");
    assert!(out.contains("exempt   Caisse.frais — paie la société de gestion, pas un porteur"), "{out}");
    assert!(out.contains("tests    PasDePaiementAuBloque"), "{out}");
    assert!(out.contains("1 of the program's 1 `require` serve a rule"), "{out}");

    // the file of one rule: its terms, and every signal that realizes them
    let (out, code) = soma(&d, &["law", "prod.cell", "SAN-02"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("term     Paiement: Un versement d'espèces du fonds."), "{out}");
    assert!(out.contains("realized Paiement: Caisse.payer, Caisse.frais"), "{out}");

    let (out, _) = soma(&d, &["law", "prod.cell", "--todo"]);
    assert!(out.contains("nothing to do"), "{out}");
    let (out, _) = soma(&d, &["law", "prod.cell", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    assert_eq!(v["rules"][0]["status"], "checked");
    assert_eq!(v["rules"][0]["served"][0]["signal"], "Caisse.payer");
}

/// The defect the law exists for: a second way to pay, written later, that
/// forgets the rule. Three scenarios of Gérance found three such paths one
/// at a time (S104–S106); the check names each as soon as it is declared a
/// payment.
#[test]
fn a_signal_that_realizes_a_term_and_forgets_the_rule_is_refused() {
    let d = project("forgotten", LAW,
        &skeleton("        signal indemniser(porteur: String, n: Int) -> Int\n            writes solde\n            realizes Paiement"),
        &flesh("    on indemniser(porteur, n) {\n        solde.set(porteur, (solde.get(porteur) ?? 0) + n)\n        return n\n    }", TEST));
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("the rule \"SAN-02\" (Aucun paiement vers un porteur bloqué — Règlement (UE) 269/2014, art. 2) is not served in `Caisse.indemniser`, which realizes `Paiement`"), "{out}");
    assert!(out.contains("add a refusal `under \"SAN-02\"` to its handler") && out.contains("or `exempt \"SAN-02\" \"<why it does not apply>\"` to its signal"), "{out}");
    assert!(out.contains("E0130"), "{out}");

    let (out, code) = soma(&d, &["law", "prod.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("status   incomplete") && out.contains("MISSING  Caisse.indemniser"), "{out}");
    let (out, _) = soma(&d, &["law", "prod.cell", "--todo"]);
    assert!(out.starts_with("1. the rule \"SAN-02\""), "{out}");

    // served through a helper of the same cell: that is served
    let d = project("helper", LAW,
        &skeleton("        signal indemniser(porteur: String, n: Int) -> Int\n            reads etat_de\n            writes solde\n            realizes Paiement"),
        &flesh("    on indemniser(porteur, n) {\n        let _ = _libre(porteur)\n        solde.set(porteur, (solde.get(porteur) ?? 0) + n)\n        return n\n    }\n    on _libre(porteur: String) {\n        require (etat_de.get(porteur) ?? \"valide\") != \"bloque\" else Bloque \"{porteur} est bloqué\" under \"SAN-02\"\n        return true\n    }", TEST));
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_eq!(code, 0, "{out}");
    let (out, _) = soma(&d, &["law", "prod.cell"]);
    assert!(out.contains("served   Caisse.indemniser (checked)"), "{out}");
}

#[test]
fn a_rule_nothing_serves_and_a_rule_nobody_declared_are_refused() {
    // the law gains a rule the program does not know
    let law = format!("{LAW}\nrule \"SAN-06\" \"Le déblocage est motivé et fait par un autre\"\n    about Porteur\n    owner \"conformite\"\n");
    let d = project("unserved", &law, &skeleton(""), &flesh("", TEST));
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("the rule \"SAN-06\" (Le déblocage est motivé et fait par un autre) of the law Sanctions is served by nothing in the program"), "{out}");
    assert!(out.contains("E0129"), "{out}");

    // the code cites a rule that is in no law: an agent does not invent rules
    let d = project("invented", LAW, &skeleton(""), &flesh("", TEST).replace("under \"SAN-02\"\n        solde", "under \"SAN-99\"\n        solde"));
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("`Caisse.payer` has a refusal `under \"SAN-99\"`, a rule no law of the program declares"), "{out}");
    assert!(out.contains("a new rule is added to the law by the person who owns it, not in the code"), "{out}");

    // a term nobody declared; an exemption without its reason
    let d = project("term", LAW, &skeleton("").replace("realizes Paiement\n        signal frais", "realizes Versement\n        signal frais"), &flesh("", TEST));
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("`Caisse.payer` realizes `Versement`, a term no law of the program declares"), "{out}");
    let d = project("reason", LAW, &skeleton("").replace(" \"paie la société de gestion, pas un porteur\"", ""), &flesh("", TEST));
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("`Caisse.frais` is exempt from the rule \"SAN-02\" (Aucun paiement vers un porteur bloqué) without a reason"), "{out}");
}

#[test]
fn a_rule_without_a_test_is_a_warning() {
    let d = project("untested", LAW, &skeleton(""), &flesh("", ""));
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("warning: the rule \"SAN-02\" (Aucun paiement vers un porteur bloqué) has no test"), "{out}");
    let (out, _) = soma(&d, &["law", "prod.cell"]);
    assert!(out.contains("status   checked, no test"), "{out}");
}

#[test]
fn the_law_is_locked_and_a_change_of_it_is_refused_until_relocked() {
    let d = project("lock", LAW, &skeleton(""), &flesh("", TEST));
    let (out, code) = soma(&d, &["law", "prod.cell", "--lock"]);
    assert_eq!(code, 0, "{out}");
    assert!(d.join("law.lock").is_file());
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(soma(&d, &["law", "prod.cell"]).0.contains("— as locked"));

    // the text of a rule is softened: the program no longer checks
    std::fs::write(d.join("lois/sanctions.law"), LAW.replace("Aucun paiement vers", "Peu de paiements vers")).unwrap();
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("the law differs from law.lock: lois/sanctions.law (changed)"), "{out}");
    assert!(out.contains("E0124"), "{out}");
    let (_, code) = soma(&d, &["law", "prod.cell", "--lock"]);
    assert_eq!(code, 0);
    assert_eq!(soma(&d, &["check", "prod.cell"]).1, 0);
}

#[test]
fn a_law_names_no_code_and_says_so() {
    let d = project("words", &format!("{LAW}\nrefuses Bloque in Caisse.payer\n"), &skeleton(""), &flesh("", TEST));
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("lois/sanctions.law:12: `refuses` is not a word of a law") && out.contains("it names no cell, no handler and no test"), "{out}");
    assert!(out.contains("E0123"), "{out}");

    // a rule about a term the law does not define; a missing file
    let d = project("about", &LAW.replace("about  Paiement, Porteur", "about  Paiement, Souscription"), &skeleton(""), &flesh("", TEST));
    let (out, _) = soma(&d, &["check", "prod.cell"]);
    assert!(out.contains("the rule \"SAN-02\" is about `Souscription`, a term no law of the program declares"), "{out}");
    let d = project("missing", LAW, &skeleton(""), &flesh("", TEST));
    std::fs::remove_file(d.join("lois/sanctions.law")).unwrap();
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("lois/sanctions.law: the law soma.toml names cannot be read"), "{out}");
}

#[test]
fn a_program_under_no_law_is_unchanged_and_the_clauses_are_inert() {
    // `realizes`, `under`, `exempt` without a `[law]` table: accepted, no effect
    let d = project("nolaw", LAW, &skeleton(""), &flesh("", TEST));
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"essai\"\n").unwrap();
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma(&d, &["test", "prod.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("2 passed, 0 failed"), "{out}");
    let (out, code) = soma(&d, &["law", "prod.cell"]);
    assert_ne!(code, 0);
    assert!(out.contains("this program is under no law"), "{out}");

    // `under` wants a rule's identifier, a string
    std::fs::write(d.join("bad.cell"), "cell C {\n  on f(n: Int) {\n    require n > 0 else Neg under SAN\n    return n\n  }\n}\n").unwrap();
    let (out, code) = soma(&d, &["check", "bad.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("`under` names a rule of the law by its identifier, a string: `under \"SAN-02\"`"), "{out}");
}

/// A rule served by what the code does, not by a refusal: the share of a
/// frozen holder is withheld and the others are paid. The statement
/// `under "ID"` marks the branch; the table says `handled`, the weakest
/// status, and nothing changes at run time.
#[test]
fn a_branch_that_handles_the_case_of_a_rule_is_marked_and_said_handled() {
    let d = project("handled", LAW,
        &skeleton("        signal distribuer(porteurs: List<String>, n: Int) -> Int\n            reads etat_de\n            writes solde\n            realizes Paiement"),
        &flesh("    on distribuer(porteurs, n) {\n        let payes = 0\n        for p in porteurs {\n            if (etat_de.get(p) ?? \"valide\") == \"bloque\" {\n                under \"SAN-02\"\n            } else {\n                solde.set(p, (solde.get(p) ?? 0) + n)\n                payes = payes + 1\n            }\n        }\n        return payes\n    }",
            &format!("{TEST}\ncell test PartRetenue under \"SAN-02\" {{\n    rules {{\n        let _ = Caisse.bloquer(\"b\")\n        assert Caisse.distribuer([\"a\", \"b\", \"c\"], 5) == 2\n    }}\n}}\n")));
    let (out, code) = soma(&d, &["check", "prod.cell"]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma(&d, &["test", "prod.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("3 passed, 0 failed"), "{out}");
    let (out, _) = soma(&d, &["law", "prod.cell"]);
    assert!(out.contains("served   Caisse.payer (checked)") && out.contains("served   Caisse.distribuer (handled)"), "{out}");
    assert!(out.contains("status   handled"), "{out}");
    // the mark is not counted as a refusal of the program
    assert!(out.contains("1 of the program's 1 `require` serve a rule"), "{out}");
}
