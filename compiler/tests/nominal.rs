//! Nominal types (docs/NOMINAL_TYPES_DESIGN.md). Phase 0: the two ways an
//! agent writes a unit today — a type name declared nowhere (`q: Lots`) and
//! a parameterised scalar (`q: Int<lots>`) — were accepted in silence and
//! checked LESS than a plain `Int` (`Lots` read as Any, `Int<lots>` turned
//! the argument checks off). They are a warning in edition 2026 and an
//! error in 2027.

use std::process::Command;

fn check(dir: &std::path::Path, file: &str) -> (String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["check", file])
        .current_dir(dir)
        .output()
        .expect("failed to run soma");
    (format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)), out.status.success())
}

fn project(name: &str, src: &str, edition: Option<&str>) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_nominal_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), src).unwrap();
    if let Some(e) = edition {
        std::fs::write(dir.join("soma.toml"), format!("[package]\nname = \"app\"\nedition = \"{e}\"\n")).unwrap();
    }
    dir
}

const UNDECLARED: &str = r#"
cell Risk {
    memory { position: Map<String, Int> [persistent] }
    on order(sym: String, lots: Lots) {
        position.set(sym, lots)
        return lots
    }
}
"#;

const PARAMETERISED: &str = r#"
cell Risk {
    on label(x: String) { return x }
    on order(lots: Int<lots>) {
        return label(lots)
    }
}
"#;

#[test]
fn an_undeclared_type_name_is_a_warning_in_2026_and_an_error_in_2027() {
    let (out, ok) = check(&project("undeclared_2026", UNDECLARED, None), "app.cell");
    assert!(ok, "edition 2026: a warning, not an error\n{out}");
    assert!(out.contains("type `Lots` is declared nowhere") && out.contains("accepts any value"), "{out}");
    assert!(out.contains("an error in edition 2027"), "{out}");
    let (out, ok) = check(&project("undeclared_2027", UNDECLARED, Some("2027")), "app.cell");
    assert!(!ok, "edition 2027: an error\n{out}");
    assert!(out.contains("type `Lots` is declared nowhere") && out.contains("E0046"), "{out}");
}

#[test]
fn a_parameterised_scalar_is_a_warning_in_2026_and_an_error_in_2027() {
    let (out, ok) = check(&project("param_2026", PARAMETERISED, None), "app.cell");
    assert!(ok, "edition 2026: a warning, not an error\n{out}");
    assert!(out.contains("`Int<lots>`") && out.contains("Int takes no parameter"), "{out}");
    // the type argument is not ALSO reported as an undeclared type
    assert!(!out.contains("type `lots` is declared nowhere"), "{out}");
    let (out, ok) = check(&project("param_2027", PARAMETERISED, Some("2027")), "app.cell");
    assert!(!ok, "edition 2027: an error\n{out}");
    assert!(out.contains("`Int<lots>`") && out.contains("E0046"), "{out}");
}

#[test]
fn declared_and_base_types_are_not_reported() {
    let src = r#"
cell type Lots { variants { Lots { n: Int } } }
cell type Book<T> {
    face { signal append(entry: T) }
    on append(entry: T) { return entry }
}
cell Risk {
    memory {
        position: Map<String, Int> [persistent]
        seen: List<String> [persistent]
        events: Log<String> [persistent]
    }
    on order(sym: String, q: Lots, tags: List<String>, meta: Map<String, Any>, ok: Bool, px: Float) {
        position.set(sym, q.n)
        return q.n
    }
}
"#;
    let (out, ok) = check(&project("declared", src, Some("2027")), "app.cell");
    assert!(ok, "{out}");
    assert!(!out.contains("declared nowhere") && !out.contains("takes no parameter"), "{out}");
}

// ── phase 1: `cell type Lots = Int` ─────────────────────────────────

fn soma_in(dir: &std::path::Path, args: &[&str]) -> (String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(dir).output().expect("failed to run soma");
    (format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)), out.status.success())
}

/// The bug of the report: a cap in cents applied to lots, lots added to
/// cents. With Int everywhere, check passes and verify --strict PROVES it.
const RISK_INT: &str = r#"
cell Risk {
    const MAX_EXPOSURE = 5000000
    memory {
        position: Map<String, Int> [persistent]
        invariant position >= 0 && position <= MAX_EXPOSURE
    }
    on order(sym: String, lots: Int, price: Int) {
        require lots > 0 else BadQty
        let held = position.get(sym) ?? 0
        require held + lots <= MAX_EXPOSURE else OverCap
        position.set(sym, held + lots)
        return held + lots * price
    }
}
"#;

const RISK_TYPED: &str = r#"
cell type Lots = Int
cell type Cents = Int

cell Risk {
    const MAX_EXPOSURE = Cents(5000000)
    memory {
        position: Map<String, Lots> [persistent]
        invariant position >= 0 && position <= MAX_EXPOSURE
    }
    on order(sym: String, lots: Lots, price: Cents) {
        require lots > 0 else BadQty
        let held = position.get(sym) ?? 0
        require held + lots <= MAX_EXPOSURE else OverCap
        position.set(sym, held + lots)
        return held + lots * price
    }
}
"#;

/// The same program written right: the cap counts lots, the notional is a
/// conversion written out.
const RISK_RIGHT: &str = r#"
cell type Lots = Int
cell type Cents = Int

cell Risk {
    face {
        signal order(sym: String, lots: Lots, price: Cents) -> Cents
    }
    const MAX_LOTS = Lots(50)
    memory {
        position: Map<String, Lots> [persistent]
        invariant position >= 0 && position <= MAX_LOTS
    }
    on order(sym: String, lots: Lots, price: Cents) {
        require lots > 0 else BadQty
        let held = position.get(sym) ?? 0
        require held + lots <= MAX_LOTS else OverCap
        position.set(sym, held + lots)
        return Cents(to_int(held + lots) * to_int(price))
    }
}
"#;

#[test]
fn mixing_two_nominal_types_is_refused_where_int_proved_it() {
    let (out, ok) = soma_in(&project("risk_int", RISK_INT, None), &["check", "app.cell"]);
    assert!(ok, "the Int program passes today (the bug):\n{out}");
    let dir = project("risk_typed", RISK_TYPED, None);
    let (out, ok) = soma_in(&dir, &["check", "app.cell"]);
    assert!(!ok, "Lots against Cents must be refused:\n{out}");
    assert!(out.contains("E0122"), "{out}");
    // the cap: Lots compared with Cents, in the require AND in the invariant
    assert!(out.contains("compares Lots with Cents"), "{out}");
    assert!(out.contains("invariant"), "the invariant's clause is typed too:\n{out}");
    // the notional: Lots times Cents
    assert!(out.contains("multiplies Lots by Cents"), "{out}");
    // each message names the fix
    assert!(out.contains("Lots(") && out.contains("to_int("), "{out}");
}

#[test]
fn a_program_with_nominal_types_proves_what_the_int_program_proves() {
    let typed = project("risk_right", RISK_RIGHT, None);
    let (out, ok) = soma_in(&typed, &["check", "app.cell"]);
    assert!(ok, "{out}");
    let (v_typed, ok) = soma_in(&typed, &["verify", "app.cell"]);
    assert!(ok, "{v_typed}");
    let erased = RISK_RIGHT.replace("cell type Lots = Int\n", "").replace("cell type Cents = Int\n", "")
        .replace("Lots(50)", "50").replace(": Lots", ": Int").replace(": Cents", ": Int").replace("-> Cents", "-> Int")
        .replace("Map<String, Lots>", "Map<String, Int>").replace("return Cents(to_int(held + lots) * to_int(price))", "return (held + lots) * price");
    let (v_int, _) = soma_in(&project("risk_right_int", &erased, None), &["verify", "app.cell"]);
    let proofs = |s: &str| s.lines().filter(|l| l.contains('✓') || l.contains('⚠')).map(|l| l.trim().to_string()).collect::<Vec<_>>();
    assert_eq!(proofs(&v_typed), proofs(&v_int), "same proofs with Lots as with Int:\n{v_typed}\n---\n{v_int}");
    assert!(v_typed.contains("✓ invariant position >= 0 && position <= 50"), "{v_typed}");
    // and it runs: a Lots is an Int at run time
    let (out, ok) = soma_in(&typed, &["run", "app.cell", "order", "ES", "3", "500"]);
    assert!(ok && out.trim().ends_with("1500"), "{out}");
}

fn nominal_check(name: &str, body: &str) -> (String, bool) {
    let src = format!(r#"
cell type Lots = Int
cell type Cents = Int
cell type Sym = String

cell Desk {{
    face {{
        signal lots_of(s: Sym) -> Lots
        signal plain(n: Int) -> Int
    }}
    memory {{
        pos: Map<String, Lots> [persistent]
        notes: Map<String, String> [persistent]
    }}
    on lots_of(s: Sym) {{ return pos.get(s) ?? 0 }}
    on plain(n: Int) {{ return n }}
    on keep(q: Lots) {{ return q }}
    on probe(a: Lots, b: Lots, c: Cents, n: Int, s: Sym, m: Map) {{
        {body}
    }}
}}
"#);
    soma_in(&project(name, &src, None), &["check", "app.cell"])
}

#[test]
fn what_a_nominal_type_accepts() {
    for (i, body) in [
        "return a + b",                    // L + L
        "return a - b",
        "return a * 2",                    // scaled by a literal
        "return idiv(a, 2)",
        "return abs(a - b)",
        "if a <= 50 { return a + 1 } return a",   // literals take the other side's type
        "let t = pos.get(\"x\") ?? 0\n        return t + a",
        "return Lots(n)",                  // the explicit wrap
        "return to_int(a) + n",            // leaving the type
        "pos.set(\"k\", a + b)\n        return a",
        "return lots_of(s)",
        "return plain(to_int(a))",
        "return a == b",
        "return max(a, b)",
    ].iter().enumerate() {
        let (out, ok) = nominal_check(&format!("accept_{i}"), body);
        assert!(ok && !out.contains("E0122"), "should pass: `{body}`\n{out}");
    }
}

#[test]
fn what_a_nominal_type_refuses() {
    for (i, (body, says)) in [
        ("return a + c", "adds Cents to Lots"),
        ("return a == c", "compares Lots with Cents"),
        ("return a < c", "compares Lots with Cents"),
        ("return a * b", "multiplies Lots by Lots"),
        ("return a * c", "multiplies Lots by Cents"),
        ("return a + n", "adds an Int to Lots"),
        ("return a * n", "multiplies Lots by an Int"),
        ("return a <= n", "compares Lots with an Int"),
        ("return plain(a)", "is Lots but the handler declares"),
        ("return keep(n)", "is an Int but the handler declares `q: Lots`"),
        ("return keep(c)", "is Cents but the handler declares `q: Lots`"),
        ("pos.set(\"k\", c)\n        return a", "writes Cents"),
        ("pos.set(\"k\", n)\n        return a", "writes an Int"),
        ("return Lots(c)", "Lots(c)"),
        ("return Lots(\"3\")", "Lots(\"3\")"),
        ("return idiv(a, n)", "idiv"),
    ].iter().enumerate() {
        let (out, ok) = nominal_check(&format!("refuse_{i}"), body);
        assert!(!ok && out.contains("E0122") && out.contains(says), "should refuse `{body}` saying `{says}`:\n{out}");
    }
}

#[test]
fn a_value_from_a_map_meeting_a_nominal_type_is_a_warning() {
    let (out, ok) = nominal_check("from_map", "return a + m.qty");
    assert!(ok, "a warning, not an error:\n{out}");
    assert!(out.contains("make it a record"), "{out}");
}

#[test]
fn one_nominal_name_cannot_have_two_bases() {
    let src = "cell type Lots = Int\ncell type Lots = Float\ncell A { on f(q: Lots) { return q } }\n";
    let (out, ok) = soma_in(&project("two_bases", src, None), &["check", "app.cell"]);
    assert!(!ok && out.contains("Lots") && out.contains("Int") && out.contains("Float"), "{out}");
}

#[test]
fn a_nominal_parameter_is_a_native_int() {
    let src = "cell type Lots = Int\ncell A {\n    on twice(q: Lots) [native] {\n        return q * 2\n    }\n}\n";
    let (out, ok) = soma_in(&project("native", src, None), &["check", "app.cell"]);
    assert!(ok, "{out}");
}

// ── phase 2: `cell type Lots = Int where …` ─────────────────────────
// The bound is written once, on the type, and becomes what verify already
// proves: a `requires` on each face parameter of that type, an `ensures` on
// each result, a memory invariant on each slot that holds it, a `require`
// on each parameter of a handler with no face signal.

const BOUNDED: &str = r#"
cell type Lots = Int where value >= 0 && value <= 50

cell Desk {
    face {
        signal order(sym: String, q: Lots) -> Lots
        signal double(q: Lots) -> Lots
    }
    memory {
        pos: Map<String, Lots> [persistent]
    }
    on order(sym: String, q: Lots) {
        let held = pos.get(sym) ?? 0
        require held + q <= 50 else Full
        pos.set(sym, held + q)
        return held + q
    }
    on double(q: Lots) {
        return q * 2
    }
    on probe(q: Lots) {
        return q
    }
}
"#;

#[test]
fn a_bound_on_a_nominal_type_is_checked_at_every_boundary() {
    let d = project("bounded", BOUNDED, None);
    let (out, ok) = soma_in(&d, &["check", "app.cell"]);
    assert!(ok, "{out}");
    // a face parameter: the caller's fault (kind requires)
    let (out, ok) = soma_in(&d, &["run", "app.cell", "order", "ES", "51"]);
    assert!(!ok && out.contains("requires"), "{out}");
    let (out, ok) = soma_in(&d, &["run", "app.cell", "order", "ES", "7"]);
    assert!(ok && out.trim().ends_with('7'), "{out}");
    // a result: the handler's fault (kind ensures)
    let (out, ok) = soma_in(&d, &["run", "app.cell", "double", "40"]);
    assert!(!ok && out.contains("ensures"), "{out}");
    // a handler with no face signal: a require
    let (out, ok) = soma_in(&d, &["run", "app.cell", "probe", "-1"]);
    assert!(!ok && out.contains("Lots"), "{out}");
}

#[test]
fn verify_proves_a_bound_where_the_program_narrows_it() {
    let d = project("bounded_verify", BOUNDED, None);
    let (out, _) = soma_in(&d, &["verify", "app.cell"]);
    // the slot's invariant comes from the type, and the require proves it
    assert!(out.contains("✓ invariant pos >= 0 && pos <= 50"), "{out}");
    // the parameter's bound is a requires (a fact inside order)
    assert!(out.contains("requires") || out.contains("contracts"), "{out}");
    // double can return 100: its ensures is not proven
    assert!(out.lines().any(|l| l.contains("double") && (l.contains('⚠') || l.contains('✗'))), "{out}");
}

#[test]
fn a_literal_outside_the_bound_is_refused_by_check() {
    for (body, says) in [
        ("const MAX = Lots(60)\n    on f() { return MAX }", "Lots(60)"),
        ("on f() { return Lots(-1) }", "Lots(-1)"),
    ] {
        let src = format!("cell type Lots = Int where value >= 0 && value <= 50\ncell A {{\n    {body}\n}}\n");
        let (out, ok) = soma_in(&project("bound_lit", &src, None), &["check", "app.cell"]);
        assert!(!ok && out.contains("E0122") && out.contains(says) && out.contains("value >= 0 && value <= 50"), "`{body}`:\n{out}");
    }
    let src = "cell type Lots = Int where value >= 0 && value <= 50\ncell A {\n    const MAX = Lots(50)\n    on f() { return MAX }\n}\n";
    let (out, ok) = soma_in(&project("bound_lit_ok", src, None), &["check", "app.cell"]);
    assert!(ok, "{out}");
}

#[test]
fn a_bound_reads_only_value() {
    let src = "cell type Lots = Int where value <= cap\ncell A { on f(q: Lots) { return q } }\n";
    let (out, ok) = soma_in(&project("bound_names", src, None), &["check", "app.cell"]);
    assert!(!ok && out.contains("value") && out.contains("cap"), "{out}");
}

#[test]
fn the_brief_and_the_lock_show_the_nominal_type_as_written() {
    let src = "skeleton Desk {\n    memory { pos: Map<String, Int> [persistent] }\n    face {\n        // Buys q lots.\n        signal buy(sym: String, q: Int) -> Int\n            writes pos\n    }\n}\nflesh Desk {\n    on buy(sym: String, q: Int) { pos.set(sym, (pos.get(sym) ?? 0) + q)  return q }\n}\n";
    let d = project("brief", src, None);
    let (out, ok) = soma_in(&d, &["skeleton", "app.cell", "--freeze"]);
    assert!(ok, "{out}");
    let typed = format!("cell type Lots = Int where value >= 0\n{}", src.replace("q: Int) -> Int", "q: Lots) -> Lots").replace("q: Int)", "q: Lots)").replace("Map<String, Int>", "Map<String, Lots>"));
    std::fs::write(d.join("app.cell"), typed).unwrap();
    // --amend names the change of type, line by line
    let (out, ok) = soma_in(&d, &["skeleton", "app.cell", "--amend"]);
    assert!(ok && out.contains("- signal buy(sym: String, q: Int) -> Int") && out.contains("+ signal buy(sym: String, q: Lots) -> Lots")
        && out.contains("+ pos: Map<String, Lots>"), "{out}");
    // the brief: the signature as written and what Lots is
    let (out, ok) = soma_in(&d, &["skeleton", "app.cell", "Desk.buy"]);
    assert!(ok && out.contains("q: Lots) -> Lots") && out.contains("Lots = Int where value >= 0 (nominal:"), "{out}");
    // describe too
    let (out, _) = soma_in(&d, &["describe", "app.cell"]);
    assert!(out.contains("\"Lots\""), "{out}");
}
