//! Lifecycle ↔ data: `status` invariants, declared transition sources and
//! guards proven from the calling handler's `require`.
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_lifecycle_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn soma(dir: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(dir).output().unwrap();
    (out.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

const ESCROW: &str = r#"
cell Escrow {
    memory {
        balances: Map<String, Int> [persistent]
        invariant balances >= 0
        invariant status != "released" || (balances ?? 0) == 0
    }
    state deal {
        initial: open
        open -> funded
        funded -> released { guard { amount >= 0 } }
        * -> cancelled
    }
    on fund(id: String, amount: Int) {
        require amount > 0 else BadAmount
        balances.set(id, amount)
        transition(id, "open", "funded")
        return get_status(id)
    }
    on release(id: String, amount: Int) {
        require amount == 0 else NotZero
        balances.set(id, amount)
        transition(id, "funded", "released")
        return get_status(id)
    }
    on fast_release(id: String, amount: Int) {
        transition(id, "released")
        return get_status(id)
    }
    on refund_after_release(id: String, amount: Int) {
        balances.set(id, amount)
        return balances.get(id)
    }
    on read(id: String) { return [get_status(id), balances.get(id)] }
}
cell test T {
    rules {
        assert Escrow.fund("a", 100) == "funded"
        assert_fails Escrow.fast_release("a", 0) matching "invariant"
        assert Escrow.read("a") == ["funded", 100]
        assert Escrow.release("a", 0) == "released"
        assert_fails Escrow.refund_after_release("a", 5) matching "invariant"
        assert Escrow.read("a") == ["released", 0]
        assert Escrow.fund("b", 10) == "funded"
        assert_fails Escrow.fund("b", 5) matching "invalid_transition"
        assert Escrow.read("b") == ["funded", 10]
        assert_fails Escrow.release("c", 0) matching "invalid_transition"
    }
}
"#;

/// A released escrow may not keep a balance: refused when the balance is
/// written under `released` AND when the machine moves to `released` with
/// a balance; the two-argument transition of `fast_release` is refused by
/// the invariant, the three-argument ones by their declared source.
#[test]
fn status_invariants_and_declared_sources_are_enforced_at_run_time() {
    let dir = scratch("escrow");
    std::fs::write(dir.join("app.cell"), ESCROW).unwrap();
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("10 tests: 10 passed"), "{out}");
    assert!(out.contains("rejected transition of 100"), "the transition itself is refused: {out}");
    assert!(out.contains("declares its source, and 'b' is in 'funded', not 'open'"), "{out}");
}

/// `verify` shows the edge a handler takes, notes the handlers that hide
/// their source, reports the `status` rule as runtime-checked and proves the
/// guard from the `require` of the one caller that narrows `amount`.
#[test]
fn verify_reports_sources_status_rules_and_guard_proofs() {
    let dir = scratch("escrow_verify");
    std::fs::write(dir.join("app.cell"), ESCROW).unwrap();
    let (_, out) = soma(&dir, &["verify", "app.cell"]);
    assert!(out.contains("handler `fund` ⟶ {open → funded}"), "{out}");
    assert!(out.contains("handler `fast_release` ⟶ {released}"), "{out}");
    assert!(out.contains("transition sources: `fast_release` calls transition(id, target) without declaring the source"), "{out}");
    assert!(out.contains("reads `status`: checked at run time on every write to balances and on every transition()"), "{out}");
    // `release` narrows amount (require amount == 0); `fast_release` does not
    assert!(out.contains("guard `amount >= 0` on funded -> released: runtime-checked in `fast_release`"), "{out}");
    assert!(out.contains("proven in `release`"), "{out}");
    let (code, strict) = soma(&dir, &["verify", "app.cell", "--strict"]);
    assert_ne!(code, 0, "an unproven guard fails --strict: {strict}");
}

/// A guard is proven by a `require` or an enclosing `if` that bounds every
/// variable it reads, and lost by a weaker bound or a reassignment.
#[test]
fn guards_are_proven_from_require_and_if_and_lost_on_reassignment() {
    let dir = scratch("guards");
    std::fs::write(dir.join("app.cell"), r#"
cell Pay {
    state order {
        initial: pending
        pending -> validated { guard { amount > 0 && amount < 1000 } }
        validated -> sent { guard { currency == "EUR" } }
        sent -> filled
    }
    on validate(id: String, amount: Int) {
        require amount > 0 && amount <= 999 else BadAmount
        transition(id, "pending", "validated")
        return get_status(id)
    }
    on validate_weak(id: String, amount: Int) {
        require amount > 0 else BadAmount
        transition(id, "validated")
        return get_status(id)
    }
    on validate_if(id: String, amount: Int) {
        if amount > 0 && amount < 500 { transition(id, "validated") }
        return get_status(id)
    }
    on validate_reassigned(id: String, amount: Int) {
        require amount > 0 && amount < 1000 else BadAmount
        amount = amount * 10
        transition(id, "validated")
        return get_status(id)
    }
    on send(id: String, currency: String) {
        require currency == "EUR" else Currency
        transition(id, "validated", "sent")
        return get_status(id)
    }
    on fill(id: String) { transition(id, "sent", "filled")
        return get_status(id) }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let (_, out) = soma(&dir, &["verify", "app.cell"]);
    assert!(out.contains("guard `amount > 0 && amount < 1000` on pending -> validated: runtime-checked in `validate_weak`, `validate_reassigned`"), "{out}");
    assert!(out.contains("proven in `validate`, `validate_if`"), "{out}");
    assert!(out.contains("guard `currency == \"EUR\"` on validated -> sent: proven"), "{out}");
}

/// `transition(id, from, to)`: the edge must exist and both states be
/// declared; the arity stays two or three.
#[test]
fn check_refuses_a_declared_source_without_its_edge() {
    let dir = scratch("sources");
    std::fs::write(dir.join("app.cell"), r#"
cell B {
    state m { initial: open  open -> funded  funded -> released  * -> cancelled }
    on a(id: String) { return transition(id, "open", "released") }
    on b(id: String) { return transition(id, "nope", "funded") }
    on c(id: String) { return transition(id, "funded", "cancelled") }
    on d(id: String) { return transition(id, "open", "funded", "x") }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("cell 'B' declares no edge open -> released; edges leaving 'open': [funded, cancelled]"), "{out}");
    assert!(out.contains("transition() from \"nope\" (did you mean \"open\"?)"), "{out}");
    assert!(!out.contains("funded -> cancelled"), "a wildcard edge is a declared edge: {out}");
    assert!(out.contains("takes 2 arguments (instance id, target state) or 3"), "{out}");
    // `status` needs a machine
    std::fs::write(dir.join("nomachine.cell"), "cell N {\n    memory { m: Map<String, Int> [persistent]\n        invariant status != \"x\" || m == 0 }\n    on w(k: String, v: Int) { m.set(k, v) }\n}\n").unwrap();
    let (code, out) = soma(&dir, &["check", "nomachine.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("reads `status`") && out.contains("declares no `state { }` machine"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Typed machines: a handler whose target is ANOTHER variant does not take a
/// guarded edge (every variant target was read as a computed one, so a guard
/// reading a local was refused for every handler of the cell).
#[test]
fn guard_variable_check_ignores_handlers_targeting_other_variants() {
    let dir = scratch("typed_guard");
    std::fs::write(dir.join("app.cell"), r#"
cell type Phase { variants { Open  Funded  Released } }
cell Typed {
    memory {
        bal: Map<String, Int> [persistent]
        invariant status != "Released" || (bal ?? 0) == 0
    }
    state deal: Phase {
        initial: Open
        Open -> Funded
        Funded -> Released { guard { amount == 0 } }
        * -> Open
    }
    on fund(id: String, amount: Int) { bal.set(id, amount)
        transition(id, Open, Funded)
        return get_status(id) }
    on release(id: String, amount: Int) { require amount == 0 else NotZero
        bal.set(id, amount)
        transition(id, Funded, Released)
        return get_status(id) }
    on reopen(id: String) { transition(id, Open)
        return get_status(id) }
    on read(id: String) { return [get_status(id), bal.get(id)] }
}
cell test T {
    rules {
        assert Typed.fund("a", 5) == "Funded"
        assert_fails Typed.release("a", 1) matching "NotZero"
        assert Typed.release("a", 0) == "Released"
        assert Typed.read("a") == ["Released", 0]
        assert Typed.reopen("a") == "Open"
        assert Typed.fund("b", 7) == "Funded"
        assert Typed.reopen("b") == "Open"
    }
}
"#).unwrap();
    let (code, out) = soma(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = soma(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("7 tests: 7 passed"), "{out}");
    let (_, out) = soma(&dir, &["verify", "app.cell"]);
    assert!(out.contains("guard `amount == 0` on Funded -> Released: proven"), "{out}");
    assert!(out.contains("handler `fund` ⟶ {Open → Funded}"), "{out}");
    let _ = std::fs::remove_dir_all(dir);
}
