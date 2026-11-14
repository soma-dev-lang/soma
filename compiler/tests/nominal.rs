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
