//! Two cases an agent reported on 3.2.0 (2026-11-14), reduced to their
//! minimum: what proves, and what must still not.

use std::process::Command;

fn verify(name: &str, src: &str, toml: Option<&str>) -> String {
    let dir = std::env::temp_dir().join(format!("soma_report_v32_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), src).unwrap();
    if let Some(t) = toml { std::fs::write(dir.join("soma.toml"), t).unwrap(); }
    let out = Command::new(env!("CARGO_BIN_EXE_soma")).args(["verify", "app.cell"]).current_dir(&dir).output().unwrap();
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

const GATEWAY: &str = r#"
cell Target {
    face {
        signal take(lots: Int) -> Int
            requires lots > 0 && lots <= 1000
    }
    on take(lots: Int) { return lots }
}
cell Gateway {
    on _bad(msg: String) { return response(400, map("error", msg)) }
    on _create(o: Map) {
        LOTS
        if lots <= 0 || lots > 1000 { return _bad("lots") }
        return Target.take(lots)
    }
    on request(method: String, path: String, body: String) {
        return _create(from_json(body))
    }
}
"#;

#[test]
fn an_early_return_proves_a_call_site_contract_for_an_int_read_from_json() {
    // to_int(…) is an Int: the early return excludes everything it tests
    let out = verify("to_int", &GATEWAY.replace("LOTS", "let lots = to_int(o.lots ?? 0)"), None);
    assert!(out.contains("✓ take: requires lots > 0") && out.contains("✓ take: requires lots <= 1000"), "{out}");
    // a raw Map value may be a Float or NaN, which `lots <= 0 || lots > 1000`
    // lets through — but `take(lots: Int)` refuses anything but an Int
    // before its requires: what reaches the clause passed the early exit
    let out = verify("raw", &GATEWAY.replace("LOTS", "let lots = o.lots ?? 0"), None);
    assert!(out.contains("✓ take: requires lots > 0") && out.contains("✓ take: requires lots <= 1000"), "{out}");
    // …only for that variable against constants: a bound borrowed from
    // another unchecked value (a NaN cap lets every lots through) proves nothing
    let borrowed = GATEWAY.replace("LOTS", "let lots = o.lots ?? 0\n        let cap = o.cap ?? 0\n        if cap > 1000 { return _bad(\"cap\") }\n        if lots > cap { return _bad(\"over\") }")
        .replace("if lots <= 0 || lots > 1000 { return _bad(\"lots\") }", "if lots <= 0 { return _bad(\"lots\") }");
    let out = verify("borrowed", &borrowed, None);
    assert!(out.contains("⚠ take: requires lots <= 1000") && out.contains("NaN"), "{out}");
    // and an expression passed (not the variable): not relaxed
    let expr = GATEWAY.replace("LOTS", "let lots = o.lots ?? 0").replace("return Target.take(lots)", "return Target.take(lots + 0)");
    let out = verify("expr", &expr, None);
    assert!(out.contains("⚠ take: requires lots > 0"), "{out}");
}

const VIA: &str = r#"
cell Risk {
    state cut {
        initial: live
        live -> cut
        cut -> live
    }
    on cut_desk(d: String) { transition(d, "live", "cut")  return "cut" }
    on rearm(d: String) { transition(d, "cut", "live")  return "live" }
    on is_live(d: String) { return (get_status(d) ?? "live") == "live" }
}
cell Fire {
    memory { desk_of: Map<String, String> [persistent] }
    state order {
        initial: idle
        idle -> armed
        armed -> done
    }
    on arm(id: String, desk: String) {
        require desk_of.get(id) == () else Duplicate
        desk_of.set(id, desk)
        transition(id, "idle", "armed")
        return "armed"
    }
    on fire(id: String) {
        READ
        CHECK
        transition(id, "armed", "done")
        return "done"
    }
}
"#;

const VIA_TOML: &str = "[verify.before.\"Fire.done\"]\nwhile = [\"Risk.live\"]\nvia = \"Fire.desk_of\"\n";

#[test]
fn a_link_read_with_a_default_still_names_the_linked_instance() {
    let with_default = VIA.replace("READ", "let desk = desk_of.get(id) ?? \"\"\n        require desk != \"\" else Unknown")
        .replace("CHECK", "require Risk.is_live(desk) else DeskCut");
    let out = verify("via_default", &with_default, Some(VIA_TOML));
    assert!(out.contains("✓ entering Fire.done while Risk.live via Fire.desk_of"), "{out}");
    // the check removed: the rule must still fail
    let unchecked = VIA.replace("READ", "let desk = desk_of.get(id) ?? \"\"").replace("CHECK", "");
    let out = verify("via_unchecked", &unchecked, Some(VIA_TOML));
    assert!(out.contains("✗ entering Fire.done"), "{out}");
    // checking ANOTHER desk than the linked one proves nothing
    let other = VIA.replace("READ", "let desk = desk_of.get(\"other\") ?? \"\"").replace("CHECK", "require Risk.is_live(desk) else DeskCut");
    let out = verify("via_other", &other, Some(VIA_TOML));
    assert!(out.contains("✗ entering Fire.done"), "{out}");
}

#[test]
fn a_rule_about_another_program_of_the_folder_is_a_note_not_a_failure() {
    // soma.toml's rule is about Fire; this program has no such cell
    let neighbour = "cell Counter {\n    memory { n: Map<String, Int> [persistent] }\n    on add(k: String) { n.set(k, (n.get(k) ?? 0) + 1) }\n}\n";
    let out = verify("neighbour", neighbour, Some(VIA_TOML));
    assert!(out.contains("VERIFY OK") && out.contains("no cell of this program"), "{out}");
    // …but a rule that names a cell of the program and a missing one is a typo: an error
    let typo = VIA.replace("READ", "let desk = desk_of.get(id) ?? \"\"").replace("CHECK", "");
    let out = verify("typo", &typo, Some("[verify.before.\"Fire.done\"]\nwhile = [\"Risky.live\"]\n"));
    assert!(out.contains("VERIFY FAILED"), "{out}");
}
