// 3.0 — B2: face contracts. `requires` / `ensures` on a signal are checked
// around every call, and `soma verify` proves them: each call site against
// `requires`, each returned value against `ensures` — and reads `ensures`
// at call sites in any cell.

use std::process::Command;

fn soma_in(dir: &std::path::Path, args: &[&str]) -> (String, String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(dir)
        .env_remove("SOMA_EDITION").output().expect("failed to run soma");
    (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.code().unwrap_or(-1))
}

fn project(name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_v3c_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), source).unwrap();
    dir
}

const LEDGER: &str = r#"cell Ledger {
    const LIMIT = 500

    face {
        signal balance() -> Int
            ensures result >= 0 && result <= 1000000
        signal debit(amount: Int) -> Int
            requires amount > 0 && amount <= LIMIT
            ensures result >= 0
        signal fact(n: Int) -> Int
            requires n >= 0
        signal bad(n: Int) -> Int
            ensures result > n
    }

    memory {
        account: Map<String, Int> [persistent]
        invariant account >= 0 && account <= 1000000
    }

    on balance() {
        return account.get("balance") ?? 0
    }

    on debit(amount: Int) {
        let current = balance()
        require amount <= current else "insufficient"
        account.set("balance", current - amount)
        return current - amount
    }

    on fact(n: Int) {
        if n == 0 { return 1 }
        return n * fact(n - 2)
    }

    on bad(n: Int) {
        return n - 1
    }

    on seed() {
        account.set("balance", 100)
        return 100
    }
}
"#;

#[test]
fn b2_contracts_are_checked_around_every_call() {
    let src = format!("{LEDGER}{}", r#"
cell test T {
    rules {
        assert seed() == 100
        assert debit(30) == 70
        assert_fails debit(0 - 5) matching "the face requires `amount > 0 && amount <= 500` — called with amount = -5"
        assert_fails debit(501)
        assert_fails bad(3) matching "the face ensures `result > n` — the handler returned 2 (called with n = 3)"
        // a recursive call is a call: fact(3) reaches fact(-1)
        assert_fails fact(3) matching "called with n = -1"
        assert fact(4) == 8
        // a scripted answer is held to the contract as the handler is
        mock balance 0 - 1
        assert_fails balance() matching "the face ensures"
    }
}
"#);
    let d = project("runtime", &src);
    let (out, err, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("8 passed, 0 failed"), "{out}");
    // the kinds a caller can branch on, and nothing written by a refused call
    std::fs::write(d.join("app.cell"), format!("{LEDGER}{}", r#"
cell Main {
    on run() {
        seed()
        let a = try { debit(0) }
        let b = try { bad(1) }
        return [a.kind, b.kind, balance()]
    }
}
"#)).unwrap();
    let (out, err, _) = soma_in(&d, &["run", "--fresh", "app.cell"]);
    assert!(out.contains("requires") && out.contains("ensures") && out.contains("100"), "{out}{err}");
}

#[test]
fn b2_verify_proves_ensures_and_call_sites() {
    let d = project("verify", LEDGER);
    let (out, err, code) = soma_in(&d, &["verify", "app.cell"]);
    let all = format!("{out}{err}");
    assert_eq!(code, 0, "{all}");
    assert!(all.contains("Ledger/contracts"), "{all}");
    // from the slot's invariant, by induction
    assert!(all.contains("✓ balance: ensures result >= 0 — proven for every returned value"), "{all}");
    assert!(all.contains("✓ balance: ensures result <= 1000000 — proven"), "{all}");
    // from `require amount <= current` in the handler
    assert!(all.contains("✓ debit: ensures result >= 0 — proven"), "{all}");
    // no caller in the program: said, not claimed
    assert!(all.contains("debit: requires amount > 0 — no call site in the program"), "{all}");
    // the recursive call is the one that is not proven — and it is the bug
    assert!(all.contains("⚠ fact: requires n >= 0 — runtime-checked at 1 of 1 call site: fact → fact(n - 2)"), "{all}");
    assert!(all.contains("⚠ bad: ensures result > n — runtime-checked"), "{all}");
}

#[test]
fn b2_a_contract_crosses_the_cell_boundary() {
    let src = format!("{LEDGER}{}", r#"
cell Desk {
    memory {
        seen: Map<String, Int> [persistent]
        paid: Map<String, Int> [persistent]
        invariant seen >= 0 && seen <= 1000000
        invariant paid >= 0 && paid <= 500
    }

    on snapshot() {
        seen.set("bare", balance())
        seen.set("qualified", Ledger.balance())
        return 1
    }

    on pay(amount: Int) {
        require amount > 0 && amount <= 500 else "over the limit"
        let left = debit(amount)
        paid.set("last", amount)
        return left
    }

    on pay_unchecked(amount: Int) {
        return debit(amount)
    }
}
"#);
    let d = project("cross", &src);
    let (out, err, code) = soma_in(&d, &["verify", "app.cell"]);
    let all = format!("{out}{err}");
    assert_eq!(code, 0, "{all}");
    // a number read through another cell's handler was "unbounded"
    assert!(all.contains("✓ invariant seen >= 0 && seen <= 1000000 — writer 'snapshot' proven (writes balance())"), "{all}");
    assert!(all.contains("writer 'snapshot' proven (writes Ledger.balance())"), "{all}");
    // one caller proves the precondition, the other does not: named
    assert!(all.contains("⚠ debit: requires amount > 0 — runtime-checked at 1 of 2 call sites: pay_unchecked → debit(amount)"), "{all}");
    assert!(all.contains("⚠ debit: requires amount <= 500 — runtime-checked at 1 of 2 call sites: pay_unchecked → debit(amount)"), "{all}");
    // a call that can never satisfy the contract is a failure of verify
    std::fs::write(d.join("app.cell"), format!("{src}{}", r#"
cell Rogue {
    on wrong() {
        return debit(501)
    }
}
"#)).unwrap();
    let (out, err, code) = soma_in(&d, &["verify", "app.cell"]);
    let all = format!("{out}{err}");
    assert_ne!(code, 0, "{all}");
    assert!(all.contains("✗ debit: requires amount <= 500 — `wrong` calls debit(501), which can never satisfy it"), "{all}");
}

#[test]
fn b2_requires_is_a_fact_for_the_handler_and_nothing_more() {
    let d = project("facts", r#"cell Stock {
    face {
        signal put(sku: String, qty: Int) -> Int
            requires qty >= 0 && qty <= 1000
            ensures result >= 0
        signal shift(n: Int) -> Int
            requires n >= 0
        signal level(sku: String) -> Int
            ensures result >= 0
        signal loose(sku: String) -> Int
            ensures result >= 0
        signal twice(n: Int) -> Int
            requires n >= 1 && n <= 10
            ensures result >= 2 && result <= 20
        signal never(n: Int) -> Int
            ensures result >= 0
    }
    memory {
        stock: Map<String, Int> [persistent]
        invariant stock >= 0 && stock <= 1000
    }
    on put(sku: String, qty: Int) {
        stock.set(sku, qty)
        return qty
    }
    on shift(n: Int) {
        n = n - 100
        stock.set("x", n)
        return n
    }
    on level(sku: String) {
        let v = stock.get(sku)
        if v == () { return 0 }
        return v
    }
    on loose(sku: String) {
        let v = stock.get(sku)
        return v
    }
    on twice(n: Int) {
        if n > 5 {
            return n * 2
        }
        n + n
    }
    on never(n: Int) {
        return 0 - 1
    }
}
"#);
    let (out, err, code) = soma_in(&d, &["verify", "app.cell"]);
    let all = format!("{out}{err}");
    // the write of a parameter the face bounds
    assert!(all.contains("✓ invariant stock >= 0 && stock <= 1000 — writer 'put' proven"), "{all}");
    // a parameter the handler rebinds is not the one the face spoke of
    assert!(all.contains("⚠ invariant stock >= 0 && stock <= 1000 — runtime-checked (computed values): shift"), "{all}");
    // a slot read is bounded once `()` is ruled out, and not before
    assert!(all.contains("✓ level: ensures result >= 0 — proven for every returned value (2 returns)"), "{all}");
    assert!(all.contains("⚠ loose: ensures result >= 0 — runtime-checked: not proven for `v` (may be ()"), "{all}");
    // path by path, the last expression included
    assert!(all.contains("✓ twice: ensures result >= 2 — proven for every returned value (2 returns)"), "{all}");
    assert!(all.contains("✓ twice: ensures result <= 20 — proven"), "{all}");
    // a return that can never satisfy the clause fails verify
    assert_ne!(code, 0, "{all}");
    assert!(all.contains("✗ never: ensures result >= 0 — the handler returns `0 - 1`, which can never satisfy it"), "{all}");
}

#[test]
fn b2_check_refuses_a_clause_a_caller_could_not_evaluate() {
    let d = project("check", r#"cell A {
    face {
        signal f(n: Int) -> Int
            requires n > limit
            requires result > 0
            ensures helper(n) > 0
            ensures result >= now()
        signal g(x: Int) -> Int
            requires x > 0
        signal h(a: Int) -> Int
            ensures result > 0
        signal ok(a: Int, b: Int) -> Int
            requires a <= b && abs(a) < 100
            ensures result == max(a, b)
    }
    memory { limit: Map<String, Int> }
    on f(n: Int) { return n }
    on helper(n: Int) { return n }
    on g(x: Int) [native] { return x + 1 }
    on h(b: Int) { return b }
    on ok(a: Int, b: Int) { return b }
}
"#);
    let (out, _, code) = soma_in(&d, &["check", "--json", "app.cell"]);
    assert_ne!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&out).expect(&out);
    let errors = v["errors"].as_array().unwrap();
    let msgs: Vec<&str> = errors.iter().map(|e| e["message"].as_str().unwrap()).collect();
    assert_eq!(errors.len(), 6, "{msgs:#?}");
    assert!(errors.iter().all(|e| e["kind"] == "contract" && e["code"] == "E0116"), "{out}");
    assert!(msgs.iter().any(|m| m.contains("reads `limit`")), "{msgs:#?}");
    assert!(msgs.iter().any(|m| m.contains("`result` is the returned value")), "{msgs:#?}");
    assert!(msgs.iter().any(|m| m.contains("calls `helper()`, a handler")), "{msgs:#?}");
    assert!(msgs.iter().any(|m| m.contains("calls `now()`")), "{msgs:#?}");
    assert!(msgs.iter().any(|m| m.contains("its handler is `[native]`")), "{msgs:#?}");
    assert!(msgs.iter().any(|m| m.contains("the handler calls parameter 1 `b`")), "{msgs:#?}");
    let (out, _, _) = soma_in(&d, &["explain", "E0116"]);
    assert!(out.contains("contract"), "{out}");
}

#[test]
fn b2_describe_and_fmt_know_the_clauses() {
    let d = project("tools", LEDGER);
    let (out, _, _) = soma_in(&d, &["describe", "app.cell"]);
    assert!(out.contains("\"requires\"") && out.contains("amount > 0 && amount <= 500") && out.contains("\"ensures\""), "{out}");
    let (out, err, code) = soma_in(&d, &["fmt", "--check", "app.cell"]);
    assert_eq!(code, 0, "{out}{err}");
    // a clause written flush is hung under its signal
    std::fs::write(d.join("app.cell"), "cell A {\n    face {\n        signal f(n: Int) -> Int\n        requires n > 0\n        ensures result > 0\n    }\n    on f(n: Int) { return n }\n}\n").unwrap();
    soma_in(&d, &["fmt", "app.cell"]);
    let text = std::fs::read_to_string(d.join("app.cell")).unwrap();
    assert!(text.contains("        signal f(n: Int) -> Int\n            requires n > 0\n            ensures result > 0\n"), "{text}");
}
