//! The gesture of refutation ("Soma — prochaines directions", D2, D3, D4): a
//! rule of the law that says what always holds has a monitor DERIVED from it;
//! `soma fingerprint` names a program; `soma refute` plays a trace of calls
//! against it and answers one of four verdicts.

use std::process::Command;

fn soma(d: &std::path::Path, args: &[&str]) -> (String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(d).output().expect("failed to run soma");
    (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
}

const LAW: &str = r#"// The source is fictional: this file shows a mechanism.
law Payments

entity Payment "A payment taken from a payer."
state captured(Payment): Int "The total captured on the payment."
state refunded(Payment): Int "The total refunded on the payment."

action Capture(p: Payment, n: Int) "Money taken from the payer."
action Refund(p: Payment, n: Int) "Money returned to the payer."

rule "RMB-01" "The total refunded on a payment never exceeds the total captured"
    about  Refund, Payment
    source "Example payment policy, art. 1 (fictional)"
    owner  "finance"
    always p: refunded(p) <= captured(p)
"#;

const SKELETON: &str = r#"use lib::till

skeleton Till {
    memory {
        capture: Map<String, Int> [persistent]
        refund: Map<String, Int> [persistent]
    }

    maps captured(p) = capture.get(p) ?? 0
    maps refunded(p) = refund.get(p) ?? 0

    face {
        signal take(id: String, n: Int) -> Int
            writes capture
            realizes Capture(id, n)
        signal give_back(id: String, n: Int) -> Int
            reads capture
            writes refund
            realizes Refund(id, n)
    }
}
"#;

/// The flesh of the white paper's example: the guard forgets what was already refunded.
const FAULTY: &str = r#"flesh Till {
    on take(id, n) {
        require n > 0 else Amount "n must be positive"
        capture.set(id, (capture.get(id) ?? 0) + n)
        return capture.get(id) ?? 0
    }

    on give_back(id, n) {
        require n > 0 else Amount "n must be positive"
        require n <= (capture.get(id) ?? 0) else Exceeds "refund exceeds capture"
        refund.set(id, (refund.get(id) ?? 0) + n)
        return refund.get(id) ?? 0
    }
}
"#;

const TRACE: &str = "# three calls the program accepts\nTill.take(\"p2\", 100)\nTill.give_back(\"p2\", 60)\nTill.give_back(\"p2\", 60)\n";

fn project(name: &str, flesh: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("soma_refute_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("lois")).unwrap();
    std::fs::create_dir_all(d.join("lib")).unwrap();
    std::fs::write(d.join("soma.toml"), "[package]\nname = \"till\"\n\n[law]\nfiles = [\"lois/payments.law\"]\n").unwrap();
    std::fs::write(d.join("lois/payments.law"), LAW).unwrap();
    std::fs::write(d.join("app.cell"), SKELETON).unwrap();
    std::fs::write(d.join("lib/till.cell"), flesh).unwrap();
    std::fs::write(d.join("counter.trace"), TRACE).unwrap();
    d
}

fn correct() -> String {
    FAULTY.replace("require n <= (capture.get(id) ?? 0) else", "require (refund.get(id) ?? 0) + n <= (capture.get(id) ?? 0) else")
}

fn fingerprint(d: &std::path::Path) -> String {
    let (out, code) = soma(d, &["fingerprint", "app.cell", "--json"]);
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    v["program"].as_str().unwrap().to_string()
}

#[test]
fn the_monitor_of_a_rule_is_derived_from_it_and_nobody_writes_it() {
    let d = project("monitor", FAULTY);
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "a rule that says what always holds is served by its monitor: {out}");
    let (out, code) = soma(&d, &["monitor", "app.cell", "RMB-01"]);
    assert_eq!(code, 0, "{out}");
    // the sentence, the form, the correspondence, and the predicate over memory
    assert!(out.contains("The total refunded on a payment never exceeds the total captured"), "{out}");
    assert!(out.contains("always p: refunded(p) <= captured(p)"), "{out}");
    assert!(out.contains("refunded(p) = (refund.get(p) ?? 0)") && out.contains("captured(p) = (capture.get(p) ?? 0)"), "{out}");
    assert!(out.contains("Till"), "it says in which cell each state is read: {out}");
    let (out, code) = soma(&d, &["monitor", "app.cell", "RMB-01", "--json"]);
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert_eq!(v["rule"], "RMB-01");
    assert_eq!(v["form"], "always p: refunded(p) <= captured(p)");
    assert_eq!(v["maps"].as_array().unwrap().len(), 2);
    // the table of the law says the rule is monitored, and which rules have no monitor
    let (out, _) = soma(&d, &["law", "app.cell"]);
    assert!(out.contains("status   monitored"), "{out}");
    // a rule with no form has no monitor, and the command says so
    std::fs::write(d.join("lois/payments.law"), LAW.replace("    always p: refunded(p) <= captured(p)\n", "")).unwrap();
    let (out, code) = soma(&d, &["monitor", "app.cell", "RMB-01"]);
    assert_ne!(code, 0);
    assert!(out.contains("no monitor") && out.contains("always"), "{out}");
}

#[test]
fn refute_answers_one_of_four_verdicts_each_with_its_exit_status() {
    // refuted: three accepted calls, and the monitor fails after the third
    let d = project("verdicts", FAULTY);
    let (out, code) = soma(&d, &["refute", "app.cell", "--trace", "counter.trace"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with("refuted"), "{out}");
    assert!(out.contains("RMB-01") && out.contains("step 3") && out.contains("Till.give_back(\"p2\", 60)"), "{out}");
    assert!(out.contains("refunded(\"p2\") = 120") && out.contains("captured(\"p2\") = 100"), "it gives the values: {out}");
    assert!(out.contains(&fingerprint(&d)), "the verdict names the program by its fingerprint: {out}");
    // twice the same
    let (again, code2) = soma(&d, &["refute", "app.cell", "--trace", "counter.trace"]);
    assert_eq!((again.as_str(), code2), (out.as_str(), 1));
    // the trace aimed at THIS program
    let fp = fingerprint(&d);
    let (out, code) = soma(&d, &["refute", "app.cell", "--trace", "counter.trace", "--expect", &fp, "--json"]);
    assert_eq!(code, 1, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert_eq!(v["verdict"], "refuted");
    assert_eq!(v["rule"], "RMB-01");
    assert_eq!(v["step"], 3);
    assert_eq!(v["program"], fp);

    // the flesh is corrected: the same trace no longer refutes — and the binary says WHY it does not
    std::fs::write(d.join("lib/till.cell"), correct()).unwrap();
    let (out, code) = soma(&d, &["refute", "app.cell", "--trace", "counter.trace"]);
    assert_eq!(code, 2, "{out}");
    assert!(out.starts_with("rejected"), "{out}");
    assert!(out.contains("step 3") && out.contains("Exceeds") && out.contains("proves nothing"), "{out}");
    // …and it is no longer the program the trace aimed at
    let (out, code) = soma(&d, &["refute", "app.cell", "--trace", "counter.trace", "--expect", &fp]);
    assert_eq!(code, 3, "{out}");
    assert!(out.starts_with("mismatch"), "{out}");
    assert!(out.contains(&fp) && out.contains(&fingerprint(&d)), "{out}");
    // holds: every call is played and no monitor fails
    std::fs::write(d.join("ok.trace"), "Till.take(\"p1\", 100)\nTill.give_back(\"p1\", 40)\nTill.give_back(\"p1\", 60)\n").unwrap();
    let (out, code) = soma(&d, &["refute", "app.cell", "--trace", "ok.trace"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("holds"), "{out}");
    assert!(out.contains("3 call(s)") && out.contains("nothing was proven"), "holding on one trace is not a proof: {out}");
}

#[test]
fn a_trace_is_calls_and_nothing_else() {
    let d = project("trace", FAULTY);
    for (trace, says) in [
        ("assert Till.take(\"p\", 1) == 1\n", "calls"),
        ("Till.take(\"p\", now())\n", "literal"),
        ("Till.nowhere(\"p\", 1)\n", "nowhere"),
        ("Nulle.take(\"p\", 1)\n", "Nulle"),
        ("", "empty"),
    ] {
        std::fs::write(d.join("bad.trace"), trace).unwrap();
        let (out, code) = soma(&d, &["refute", "app.cell", "--trace", "bad.trace"]);
        assert_eq!(code, 4, "{trace:?}: {out}");
        assert!(out.contains(says), "{trace:?} — expected `{says}`: {out}");
    }
    // a program with no monitor has nothing a trace could refute: a precondition is not what always holds
    std::fs::write(d.join("lois/payments.law"), LAW.replace("    always p: refunded(p) <= captured(p)\n", "    requires Refund(p, n): refunded(p) <= captured(p)\n")).unwrap();
    let (out, code) = soma(&d, &["refute", "app.cell", "--trace", "counter.trace"]);
    assert_eq!(code, 4, "{out}");
    assert!(out.contains("no monitor"), "{out}");
}

#[test]
fn the_fingerprint_names_the_law_each_skeleton_each_flesh_the_manifest_and_the_binary() {
    let d = project("fp", FAULTY);
    let (out, code) = soma(&d, &["fingerprint", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    for what in ["law       lois/payments.law", "skeleton  Till", "flesh     Till", "manifest  soma.toml", "binary    soma ", "program   "] {
        assert!(out.contains(what), "{what}: {out}");
    }
    let line = |out: &str, key: &str| out.lines().find(|l| l.contains(key)).unwrap_or("").to_string();
    let before = out.clone();
    // a comment or a blank line in the flesh changes nothing…
    std::fs::write(d.join("lib/till.cell"), format!("// reviewed\n\n{}", FAULTY.replace("    on take", "\n    // takes money\n    on take"))).unwrap();
    let (out, _) = soma(&d, &["fingerprint", "app.cell"]);
    assert_eq!(out, before, "layout and comments are not the program");
    // …a changed body changes the flesh and the program, and nothing else
    std::fs::write(d.join("lib/till.cell"), correct()).unwrap();
    let (out, _) = soma(&d, &["fingerprint", "app.cell"]);
    assert_ne!(line(&out, "flesh     Till"), line(&before, "flesh     Till"));
    assert_ne!(line(&out, "program   "), line(&before, "program   "));
    for same in ["law       ", "skeleton  Till", "manifest  ", "binary    "] { assert_eq!(line(&out, same), line(&before, same), "{same}"); }
    // the law, the skeleton
    std::fs::write(d.join("lois/payments.law"), LAW.replace("never exceeds", "does not exceed")).unwrap();
    let (out2, _) = soma(&d, &["fingerprint", "app.cell"]);
    assert_ne!(line(&out2, "law       "), line(&out, "law       "));
    assert_eq!(line(&out2, "flesh     Till"), line(&out, "flesh     Till"));
}

/// `soma law` used to count the tests attached to a rule, not what they say:
/// a failing test was listed among those that back the rule, and nothing
/// recorded a refutation. Now each test carries its result, and a
/// refutation the project keeps makes the rule's status fall wherever it is
/// shown.
#[test]
fn the_table_of_the_law_carries_the_result_of_each_test_and_the_known_refutations() {
    let d = project("results", FAULTY);
    std::fs::write(d.join("tests.cell"), r#"use app

cell test RefundWithinCapture under "RMB-01" {
    rules {
        assert Till.take("p1", 100) == 100
        assert_fails Till.give_back("p1", 150) matching "Exceeds"
        assert Till.give_back("p1", 40) == 40
    }
}

cell test TwoRefunds under "RMB-01" {
    rules {
        assert Till.take("p2", 100) == 100
        assert Till.give_back("p2", 60) == 60
        assert_fails Till.give_back("p2", 60) matching "Exceeds"
    }
}
"#).unwrap();
    // one test passes, the other fails: the table says which
    let (out, code) = soma(&d, &["law", "app.cell"]);
    assert_ne!(code, 0, "a rule with a failing test is not in order: {out}");
    assert!(out.contains("RefundWithinCapture (passes)") && out.contains("TwoRefunds (FAILS)"), "{out}");
    assert!(out.contains("status   failing"), "{out}");
    let (out, _) = soma(&d, &["law", "app.cell", "--todo"]);
    assert!(out.contains("TwoRefunds") && out.contains("fails"), "{out}");
    // without running anything, the table says it did not
    let (out, _) = soma(&d, &["law", "app.cell", "--no-run"]);
    assert!(out.contains("TwoRefunds (not run)") && !out.contains("FAILS"), "{out}");
    // a refutation kept in the project: the status falls, and says by which trace
    std::fs::create_dir_all(d.join("refutations")).unwrap();
    std::fs::write(d.join("refutations/double-refund.trace"), TRACE).unwrap();
    let (out, code) = soma(&d, &["law", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("status   refuted"), "{out}");
    assert!(out.contains("refutations/double-refund.trace") && out.contains("step 3"), "{out}");
    let (out, _) = soma(&d, &["law", "app.cell", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out[out.find('{').unwrap()..]).expect(&out);
    assert_eq!(v["rules"][0]["status"], "refuted", "{out}");
    assert_eq!(v["rules"][0]["refuted_by"][0]["trace"], "refutations/double-refund.trace");
    assert_eq!(v["rules"][0]["test_results"]["TwoRefunds"], "fails");
    // `soma next` hands it out
    let _ = soma(&d, &["init", "--agent"]);
    let (out, _) = soma(&d, &["next", "app.cell", "--json"]);
    assert!(out.contains("\"law\"") && out.contains("double-refund"), "{out}");
    // the flesh is corrected: both tests pass, the trace no longer refutes — and stays, said as such
    std::fs::write(d.join("lib/till.cell"), correct()).unwrap();
    let (out, code) = soma(&d, &["law", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("status   monitored"), "{out}");
    assert!(out.contains("TwoRefunds (passes)"), "{out}");
    assert!(out.contains("double-refund.trace") && out.contains("no longer refutes"), "{out}");
}

/// D5 — the side that contests: a bounded search for a trace that refutes a
/// rule, over small domains. It finds the double refund without being told
/// where to look, and says what finding nothing means.
#[test]
fn attack_searches_small_traces_and_says_what_silence_is_worth() {
    let d = project("attack", FAULTY);
    let (out, code) = soma(&d, &["attack", "app.cell", "RMB-01", "--out", "found.trace"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with("refuted"), "{out}");
    assert!(out.contains("RMB-01") && out.contains("Till.give_back("), "{out}");
    // what it found is a trace like any other: replayed, it refutes
    let trace = std::fs::read_to_string(d.join("found.trace")).unwrap();
    assert!(trace.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).count() <= 3, "the shortest it met: {trace}");
    let (out, code) = soma(&d, &["refute", "app.cell", "--trace", "found.trace", "--expect", &fingerprint(&d)]);
    assert_eq!(code, 1, "{out}");
    // the corrected flesh: nothing found — and that is not a proof
    std::fs::write(d.join("lib/till.cell"), correct()).unwrap();
    let (out, code) = soma(&d, &["attack", "app.cell", "RMB-01", "--depth", "3"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with("not refuted"), "{out}");
    assert!(out.contains("proves nothing") && out.contains("depth 3"), "{out}");
    // it wrote nothing in the program
    assert_eq!(std::fs::read_to_string(d.join("lib/till.cell")).unwrap(), correct());
    let (out, code) = soma(&d, &["attack", "app.cell", "NOPE"]);
    assert_eq!(code, 4, "{out}");
}

/// Two runs give the same verdict: a program that reads the outside — a
/// clock, chance, a model, the network — cannot be judged by a trace of
/// literals, and the binary says which handler reads what.
#[test]
fn a_program_that_reads_the_outside_is_not_judged_by_a_trace() {
    let d = project("outside", &FAULTY.replace("capture.set(id, (capture.get(id) ?? 0) + n)", "capture.set(id, (capture.get(id) ?? 0) + n + now() % 2)"));
    std::fs::write(d.join("app.cell"), SKELETON.replace("            writes capture\n            realizes Capture(id, n)", "            writes capture\n            calls now\n            realizes Capture(id, n)")).unwrap();
    let (out, code) = soma(&d, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    for cmd in [vec!["refute", "app.cell", "--trace", "counter.trace"], vec!["attack", "app.cell", "RMB-01"]] {
        let (out, code) = soma(&d, &cmd);
        assert_eq!(code, 4, "{cmd:?}: {out}");
        assert!(out.contains("now") && out.contains("Till.take") && out.contains("outside"), "{cmd:?}: {out}");
    }
}
