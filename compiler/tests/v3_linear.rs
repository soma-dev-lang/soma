// 3.0 — B4: linear integer arithmetic behind the interval prover. A clause
// the intervals leave open is proven when the facts at the write (requires,
// `if` conditions, early exits) together with its negation have no
// solution. Only over Int values the handler cannot change; anything else
// stays runtime-checked.

use std::process::Command;

fn verify(name: &str, source: &str) -> String {
    let dir = std::env::temp_dir().join(format!("soma_v3n_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), source).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(["verify", "app.cell"]).current_dir(&dir)
        .env_remove("SOMA_EDITION").output().expect("failed to run soma");
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

const LINEAR: &str = "proven (linear arithmetic over the handler's requires and conditions)";

fn bank(handlers: &str) -> String {
    format!(r#"cell Bank {{
    face {{
        signal h(a: String, b: String, amount: Int, fee: Int) -> Int
    }}

    memory {{
        acct: Map<String, Int> [persistent]
        invariant acct >= 0 && acct <= 1000
    }}

{handlers}
}}
"#)
}

#[test]
fn b4_a_relation_between_two_values_proves_the_write() {
    // room is 1000 - to; amount <= room; so to + amount <= 1000
    let out = verify("move", &bank(r#"    on h(a: String, b: String, amount: Int, fee: Int) {
        let from = acct.get(a) ?? 0
        let to = acct.get(b) ?? 0
        if amount <= 0 || amount > from { return 0 }
        let room = 1000 - to
        if amount > room { return 0 }
        acct.set(a, from - amount)
        acct.set(b, to + amount)
        return amount
    }"#));
    assert!(out.contains(&format!("writer 'h' {LINEAR} (writes to + amount)")), "{out}");
    assert!(!out.contains("runtime-checked"), "{out}");
    assert!(out.contains("VERIFY OK"), "{out}");

    // the condition of the branch the write is in, with a constant multiple
    let out = verify("branch", &bank(r#"    on h(a: String, b: String, amount: Int, fee: Int) {
        let now = acct.get(a) ?? 0
        if 2 * amount <= 1000 - now && amount >= 0 {
            acct.set(a, now + 2 * amount)
            return 1
        } else {
            return 0
        }
    }"#));
    assert!(out.contains(&format!("writer 'h' {LINEAR} (writes now + 2 * amount)")), "{out}");

    // three values in a chain
    let out = verify("chain", &bank(r#"    on h(a: String, b: String, amount: Int, fee: Int) {
        let bal = acct.get(a) ?? 0
        require fee >= 0 && amount >= fee else Bad
        require bal >= amount else Insufficient
        acct.set(a, bal - fee)
        return fee
    }"#));
    assert!(!out.contains("runtime-checked"), "{out}");
}

#[test]
fn b4_what_does_not_follow_stays_runtime_checked() {
    for (name, body, why) in [
        ("unrelated", r#"        let bal = acct.get(a) ?? 0
        require amount > 0 else Bad
        acct.set(a, bal - amount)
        return bal"#, "nothing relates bal and amount"),
        ("another_term", r#"        let bal = acct.get(a) ?? 0
        require bal >= amount else Insufficient
        require fee >= 0 else Bad
        acct.set(a, bal - amount - fee)
        return bal"#, "the require is about amount, the write also subtracts fee"),
        ("reassigned", r#"        let bal = acct.get(a) ?? 0
        let take = amount
        require bal >= take && take >= 0 else Insufficient
        take = take + 1
        acct.set(a, bal - take)
        return bal"#, "the local changes after the require"),
        ("require_after", r#"        let bal = acct.get(a) ?? 0
        acct.set(a, bal - amount)
        require bal >= amount && amount >= 0 else Insufficient
        return bal"#, "the require comes after the write"),
        ("other_branch", r#"        let bal = acct.get(a) ?? 0
        if bal >= amount && amount >= 0 { return 0 }
        acct.set(a, bal - amount)
        return bal"#, "the condition holds in the branch that returns, not at the write"),
        ("product", r#"        let bal = acct.get(a) ?? 0
        require amount >= 0 && fee >= 0 && bal >= amount * fee else Insufficient
        acct.set(a, bal - amount * fee + 1)
        return 0"#, "a product of two values is not linear, and the +1 breaks it anyway"),
    ] {
        let out = verify(name, &bank(&format!("    on h(a: String, b: String, amount: Int, fee: Int) {{\n{body}\n    }}")));
        assert!(out.contains("runtime-checked"), "{name} ({why}) was proven:\n{out}");
        assert!(!out.contains(LINEAR) || name == "another_term" || name == "unrelated", "{name}: {out}");
    }

    // Floats are not reasoned about: a + b - b is not a
    let out = verify("float", r#"cell F {
    face { signal h(k: String, x: Float, y: Float) -> Float }
    memory {
        level: Map<String, Float> [persistent]
        invariant level >= 0.0
    }
    on h(k: String, x: Float, y: Float) {
        require x >= y else Bad
        level.set(k, x - y)
        return x
    }
}
"#);
    assert!(!out.contains(LINEAR), "{out}");
}

#[test]
fn b4_a_clause_relating_the_result_to_a_parameter_is_proven() {
    let out = verify("ensures", r#"cell Bank {
    face {
        signal withdraw(id: String, amount: Int) -> Int
            requires amount > 0
            ensures result <= amount
            ensures result >= 1
        signal overdraw(id: String, amount: Int) -> Int
            ensures result <= amount
    }
    memory {
        acct: Map<String, Int> [persistent]
        invariant acct >= 0
    }
    on withdraw(id: String, amount: Int) {
        let bal = acct.get(id) ?? 0
        if bal >= amount {
            acct.set(id, bal - amount)
            return amount
        }
        require bal >= 1 else Empty
        acct.set(id, 0)
        return bal
    }
    on overdraw(id: String, amount: Int) {
        return amount + 1
    }
}
"#);
    assert!(out.contains("withdraw: ensures result <= amount — proven for every returned value (2 returns)"), "{out}");
    assert!(out.contains("withdraw: ensures result >= 1 — proven for every returned value (2 returns)"), "{out}");
    assert!(!out.contains("overdraw: ensures result <= amount — proven"), "{out}");
}

#[test]
fn b4_the_ratelimit_package_proves_its_bucket() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let dir = std::env::temp_dir().join(format!("soma_v3n_pkg_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(repo.join("packages/ratelimit/ratelimit.cell"), dir.join("app.cell")).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(["verify", "app.cell", "--strict"]).current_dir(&dir).env_remove("SOMA_EDITION").output().unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(out.contains(&format!("invariant tokens >= 0 — writer 'take' {LINEAR} (writes have - cost)")), "{out}");
    assert!(!out.contains("runtime-checked"), "{out}");
}
