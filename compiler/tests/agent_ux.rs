//! Regression tests for what fresh agents found while learning Soma cold
//! (docs/agent-ux/LEDGER.md). Each test pins one finding.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("soma_agent_ux_{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn soma_in(d: &std::path::Path, args: &[&str]) -> (String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(args)
        .current_dir(d)
        .env_remove("SOMA_LLM_MOCK")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("SOMA_LLM_KEY")
        .output()
        .expect("failed to run soma");
    (
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
        o.status.code().unwrap_or(-1),
    )
}

/// Write `src` as app.cell, expect `soma test` to pass.
fn passes(name: &str, src: &str) -> String {
    let d = dir(name);
    std::fs::write(d.join("app.cell"), src).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{name}: soma test must pass:\n{out}");
    out
}

#[test]
fn handlers_are_atomic_and_try_is_a_savepoint() {
    passes("atomic", r#"
cell Bank {
    face { signal seed(n: Int) -> Int  signal wire(id: String, amount: Int) -> String
           signal careful(id: String, amount: Int) -> String  signal status(id: String) -> String }
    memory { account: Map<String, Int>  audit: Map<String, String>  invariant account >= 0 }
    state payment { initial: proposed  proposed -> drafted  drafted -> paid  proposed -> rejected }
    on seed(n: Int) { account.set("cash", n)  return n }
    on status(id: String) { return get_status(id) }
    on wire(id: String, amount: Int) {
        transition(id, "drafted")
        audit.set(id, "drafted")
        account.set("cash", (account.get("cash") ?? 0) - amount)
        transition(id, "paid")
        return "sent"
    }
    on careful(id: String, amount: Int) {
        let r = try { wire(id, amount) }
        if r.error != () { transition(id, "rejected")  return "rejected" }
        return "sent"
    }
}
cell test T {
    rules {
        assert seed(100) == 100
        assert_fails wire("w1", 500) matching "invariant"
        assert status("w1") == "proposed"
        assert audit.get("w1") == ()
        assert careful("w2", 500) == "rejected"
        assert status("w2") == "rejected"
        assert audit.get("w2") == ()
        assert wire("w3", 40) == "sent"
        assert account.get("cash") == 60
    }
}
"#);
}

#[test]
fn errors_have_a_kind_a_detail_and_can_be_reraised() {
    let out = passes("errors", r#"
cell Res {
    face { signal find(id: String) -> Map  signal relay(id: String) -> String  signal ship(id: String) -> String }
    memory { book: Map<String, Int>  invariant book >= 0 }
    state life { initial: requested  requested -> held  held -> shipped }
    on find(id: String) {
        if book.get(id) == () { fail("not_found", "reservation {id}") }
        return map("id", id)
    }
    on relay(id: String) { let g = try { find(id) }  if g.error != () { fail(g) }  return "found" }
    on ship(id: String) { transition(id, "shipped")  return "ok" }
}
cell test T {
    rules {
        assert_fails find("zz") matching "not_found: reservation zz"
        let r = try { relay("zz") }
        assert r.kind == "not_found"
        assert r.detail == "reservation zz"
        let t = try { ship("a") }
        assert t.kind == "invalid_transition"
        let w = try { book.set("k", -1) }
        assert w.kind == "invariant"
    }
}
"#);
    assert!(!out.contains("Runtime("), "errors are shown in the language's words: {out}");
    assert!(!out.contains("require failed: invalid transition"), "{out}");
}

#[test]
fn data_code_behaves_as_python_habits_expect() {
    passes("data", r#"
cell D {
    face { signal rows() -> List  signal bump() -> Map }
    on rows() { return [map("c", "bob", "t", 50), map("c", "al", "t", 50), map("c", "cy", "t", 90)] }
    on bump() {
        let counts = map("a", 1)
        counts["a"] += 2
        let acc = Account { balance: 10 }
        acc.balance -= 4
        return map("a", counts["a"], "balance", acc.balance)
    }
}
cell type R { variants { P { record: Map }  Bad } }
cell test T {
    rules {
        assert [] == []
        assert map("US", 1, "EU", 2) == map("EU", 2, "US", 1)
        assert P { record: map("a", 1) } == P { record: map("a", 1) }
        assert contains([1, 2, 3], 2)
        assert contains(["paid"], "paid")
        assert slice([1, 2, 3, 4, 5], 1, 3) == [2, 3]
        assert slice([1, 2, 3], -2) == [2, 3]
        assert keys(map("x", 1, "y", 2)) == ["x", "y"]
        assert map(sort_by(rows(), r => [0 - r.t, r.c]), r => r.c) == ["cy", "al", "bob"]
        assert map(sort_by(rows(), "c"), r => r.c) == ["al", "bob", "cy"]
        assert round(2.345, 2) == 2.35
        assert bump() == map("a", 3, "balance", 6)
        assert_fails rows()[0].missing >= 5 matching "cannot order ()"
    }
}
"#);
}

#[test]
fn check_redirects_foreign_habits() {
    let d = dir("habits");
    std::fs::write(d.join("app.cell"), r#"
cell H {
    face { signal f(orders: List) -> List  signal g(xs: List) -> Bool }
    on f(orders: List) {
        let rows = []
        for o in orders {
            if o.status == null { return [] }
            rows.push(o)
        }
        let t = lefft + 1
        return [orders[0]] + rows
    }
    on g(xs: List) { return xs.includes("paid") }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    for needle in [
        "'null' does not exist in Soma",
        "no method 'includes' — in Soma: contains(xs, x)",
        "the result of `push` is discarded",
        "`+` with a list literal",
        "undefined variable 'lefft'",
        "app.cell:", // diagnostics carry file:line:col
    ] {
        assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
    }
}

#[test]
fn parser_hints_name_the_soma_form() {
    let d = dir("hints");
    for (body, hint) in [
        ("let t = {}", "no `{k: v}` literal"),
        ("let a = xs[1:3]", "no slice syntax"),
        ("let s = sort_by(xs, (a, b) => a - b)", "exactly ONE parameter"),
        ("for (k, v) in xs { }", "entries(m)"),
        ("let a = 1;", "no semicolons"),
    ] {
        let src = format!("cell A {{\n    face {{ signal f(xs: List) -> Int }}\n    on f(xs: List) {{\n        {body}\n        return 1\n    }}\n}}\n");
        std::fs::write(d.join("app.cell"), src).unwrap();
        let (out, code) = soma_in(&d, &["check", "app.cell"]);
        assert_ne!(code, 0);
        assert!(out.contains("hint:") && out.contains(hint), "{body:?} should hint {hint:?}:\n{out}");
    }
}

#[test]
fn llm_mocks_are_scriptable_per_test_and_need_no_key() {
    passes("mocks", r#"
cell agent Triage {
    face { signal classify(text: String) -> String }
    cost {
        tokens: 200
    }
    state t { initial: received  received -> done  * -> failed }
    on classify(text: String) {
        let r = try { think("Classify: {text}", "You are a triage bot.", map("max_tokens", 50)) }
        if r.error != () { return "failed" }
        return lowercase(trim(r.value))
    }
}
cell test T {
    rules {
        let tickets = ["refund please", "it crashes"]
        mock think ["billing", "BUG"]
        assert classify(tickets[0]) == "billing"
        assert classify(tickets[1]) == "bug"
        mock think error "timeout"
        assert classify("x") == "failed"
    }
}
"#);
    // think(prompt, system, opts): the options are the LAST argument
    let d = std::env::temp_dir().join("soma_agent_ux_mocks");
    let (out, _) = soma_in(&d, &["check", "app.cell"]);
    assert!(out.contains("'tokens' bound proven — peak 50 tokens"), "{out}");
}

#[test]
fn guards_see_the_calling_handler_and_check_verifies_their_scope() {
    let src = |binding: &str| format!(r#"
cell Exp {{
    face {{ signal submit(id: String, amount: Int) -> String  signal ok_it(id: String) -> String  signal pay(id: String) -> String }}
    memory {{ amounts: Map<String, Int> }}
    state expense {{ initial: submitted  submitted -> approved  approved -> paid {{ guard {{ amount < 10000 }} }} }}
    on submit(id: String, amount: Int) {{ amounts.set(id, amount)  return get_status(id) }}
    on ok_it(id: String) {{ transition(id, "approved")  return get_status(id) }}
    on pay(id: String) {{
        let {binding} = amounts.get(id) ?? 0
        transition(id, "paid")
        return get_status(id)
    }}
}}
cell test T {{
    rules {{
        assert submit("small", 500) == "submitted"
        assert ok_it("small") == "approved"
        assert pay("small") == "paid"
        assert submit("big", 50000) == "submitted"
        assert ok_it("big") == "approved"
        assert_fails pay("big") matching "guard failed"
    }}
}}
"#);
    passes("guards", &src("amount"));
    let d = dir("guards_bad");
    std::fs::write(d.join("app.cell"), src("montant")).unwrap();
    let (out, code) = soma_in(&d, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("guard on `approved -> paid` reads 'amount', but handler `pay`"), "{out}");
}

#[test]
fn verify_properties_are_sound_and_typos_are_errors() {
    let d = dir("props");
    std::fs::write(d.join("app.cell"), r#"
cell P {
    face { signal go(id: String) -> String }
    state pay { initial: submitted  submitted -> rejected  rejected -> paid  submitted -> paid }
    on go(id: String) { transition(id, "paid")  return "ok" }
}
"#).unwrap();

    // `after rejected never paid` is FALSE here (rejected -> paid); it used to pass
    std::fs::write(d.join("soma.toml"), "[verify.after.rejected]\nnever = [\"paid\"]\n\n[verify.before.paid]\nrequires = [\"rejected\"]\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("'paid' is reachable after 'rejected'"), "{out}");
    assert!(out.contains("'paid' is reachable without passing through any of [rejected]"), "{out}");
    assert!(out.contains("VERIFY FAILED"), "one final verdict: {out}");

    // a property about a state that does not exist proves nothing
    std::fs::write(d.join("soma.toml"), "[verify]\nnever = [\"piad\"]\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("never names state 'piad'"), "{out}");

    // a typo'd key is an error, and [package] is optional
    std::fs::write(d.join("soma.toml"), "[verify]\nevetually = [\"paid\"]\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "app.cell"]);
    assert_ne!(code, 0);
    assert!(out.contains("soma.toml does not parse"), "{out}");
}

#[test]
fn wildcard_unterminating_a_state_is_reported_and_except_keeps_it_final() {
    let d = dir("wildcard");
    let machine = |wild: &str| format!("cell W {{\n    face {{ signal go(id: String) -> String }}\n    state life {{ initial: received  received -> decided  decided -> paid  decided -> denied  {wild} }}\n    on go(id: String) {{ transition(id, \"decided\")  return \"ok\" }}\n}}\n");
    std::fs::write(d.join("app.cell"), machine("* -> failed")).unwrap();
    let (out, _) = soma_in(&d, &["verify", "app.cell"]);
    assert!(out.contains("`* -> failed` also adds denied -> failed, paid -> failed"), "{out}");
    std::fs::write(d.join("app.cell"), machine("* -> failed except [paid, denied]")).unwrap();
    let (out, code) = soma_in(&d, &["verify", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("terminal states: [failed, denied, paid]") || out.contains("denied") && !out.contains("also adds"), "{out}");
}

#[test]
fn invariants_on_computed_values_are_proven_by_induction_when_they_can_be() {
    let d = dir("induction");
    std::fs::write(d.join("app.cell"), r#"
cell C {
    face { signal bump(k: String) -> Int  signal drop_one(k: String) -> Int  signal add(n: Int) -> Int }
    memory { counts: Map<String, Int>  invariant counts >= 0 }
    on bump(k: String)     { counts.set(k, (counts.get(k) ?? 0) + 1)  return 1 }
    on drop_one(k: String) { counts.set(k, (counts.get(k) ?? 0) - 1)  return 1 }
    on add(n: Int)         { counts.set("n", (counts.get("n") ?? 0) + n)  return 1 }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["verify", "app.cell"]);
    assert!(out.contains("writer 'bump' proven by induction"), "{out}");
    // these two must NOT be proven: a decrement and an unknown parameter
    assert!(out.contains("runtime-checked") && out.contains("drop_one → counts") && out.contains("add → counts"), "{out}");
    assert!(!out.contains("writer 'drop_one' proven") && !out.contains("writer 'add' proven"), "{out}");
}

#[test]
fn test_and_verify_refuse_a_program_that_fails_check() {
    let d = dir("gate");
    std::fs::write(d.join("app.cell"), "cell G {\n    face { signal f() -> Int }\n    state s { initial: a  a -> b }\n    on f() { return nope + 1 }\n}\ncell test T { rules { assert 1 == 1 } }\n").unwrap();
    for cmd in ["test", "verify"] {
        let (out, code) = soma_in(&d, &[cmd, "app.cell"]);
        assert_ne!(code, 0, "soma {cmd} must refuse: {out}");
        assert!(out.contains("fails `soma check`") && out.contains("undefined variable 'nope'"), "{out}");
    }
}

fn http(port: u16, method: &str, path: &str) -> String {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(std::time::Duration::from_secs(30))).unwrap();
    write!(s, "{method} {path} HTTP/1.0\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n").unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

/// The evaluator's experiment: 300 payments of 10 against a balance of 1000,
/// in parallel. Before handlers were atomic, 122–153 were paid. Also pins
/// route precedence: `POST /pay/<id>` is `request`'s route, not the
/// auto-exposed `pay` handler.
#[test]
fn concurrent_handlers_are_serialized_and_explicit_routes_win() {
    let d = dir("serve");
    std::fs::write(d.join("app.cell"), r#"
cell Bank {
    face {
        signal seed(n: Int) -> Int
        signal pay(id: String, amount: Int) -> Map
        signal request(method: String, path: String, body: String) -> Map
    }
    memory { account: Map<String, Int>  invariant account >= 0 }
    on seed(n: Int) { account.set("cash", n)  account.set("paid", 0)  return n }
    on pay(id: String, amount: Int) {
        let b = account.get("cash") ?? 0
        if b < amount { return map("ok", false) }
        account.set("cash", b - amount)
        account.set("paid", (account.get("paid") ?? 0) + 1)
        return map("ok", true)
    }
    on request(method: String, path: String, body: String) {
        match map("method", method, "path", path) {
            {method: "POST", path: "/pay/" + id} -> pay(id, 10)
            {method: "GET", path: "/stats"} -> map("cash", account.get("cash") ?? 0, "paid", account.get("paid") ?? 0)
            _ -> response(404, map("error", "not found"))
        }
    }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["check", "app.cell"]);
    assert!(out.contains("handler `pay` and a route of `request` share the path /pay"), "{out}");

    let port = 19100 + (std::process::id() % 700) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "app.cell", "-p", &port.to_string()])
        .current_dir(&d)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("soma serve");
    let mut up = false;
    for _ in 0..80 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            up = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut stats = String::new();
    let mut one = String::new();
    if up {
        let _ = http(port, "GET", "/seed/1000");
        one = http(port, "POST", "/pay/first");
        let workers: Vec<_> = (0..30)
            .map(|w| std::thread::spawn(move || {
                for i in 0..10 {
                    let _ = http(port, "POST", &format!("/pay/p{w}_{i}"));
                }
            }))
            .collect();
        for w in workers {
            let _ = w.join();
        }
        stats = http(port, "GET", "/stats");
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(up, "server did not start");
    assert!(one.contains("\"ok\": true") || one.contains("\"ok\":true"), "explicit route must win over the auto-exposed handler: {one}");
    // 301 attempts of 10 against 1000: exactly 100 can succeed
    assert!(stats.contains("\"paid\": 100") || stats.contains("\"paid\":100"), "lost updates: {stats}");
    assert!(stats.contains("\"cash\": 0") || stats.contains("\"cash\":0"), "{stats}");
}

#[test]
fn cli_args_take_json_for_map_and_list_params_and_unknown_ids_are_detectable() {
    let d = dir("cli_json");
    std::fs::write(d.join("app.cell"), r#"
cell Orders {
  memory { orders: Map<String, Map<String, Any>> }
  state flow {
    initial: new
    new -> paid
  }
  on validate(o: Map<String, Any>) {
    orders[o.id] = o
    o.qty * 2
  }
  on tags(xs: List<String>) { len(xs) }
  on pay_then_probe(id: String) {
    let before = has_state(id)
    transition(id, "paid")
    map("before", before, "after", has_state(id), "state", get_status(id), "fresh", has_state("nobody"))
  }
}
cell test Probe {
  rules {
    let r = pay_then_probe("x")
    assert r.before == false
    assert r.after == true
    assert r.state == "paid"
    assert r.fresh == false
    assert get_status("nobody") == "new"
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["run", "app.cell", "validate", r#"{"id":"a","qty":21}"#]);
    assert_eq!(code, 0, "{out}");
    assert!(out.trim().ends_with("42"), "{out}");
    let (out, code) = soma_in(&d, &["run", "app.cell", "tags", r#"["a","b"]"#]);
    assert_eq!(code, 0, "{out}");
    assert!(out.trim().ends_with('2'), "{out}");
    let (out, code) = soma_in(&d, &["run", "app.cell", "validate", "garbage"]);
    assert_ne!(code, 0);
    assert!(out.contains("expects Map") && out.contains("not valid JSON"), "{out}");
    let (out, code) = soma_in(&d, &["run", "app.cell", "validate", "[1]"]);
    assert_ne!(code, 0);
    assert!(out.contains("not a Map"), "{out}");
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn state_block_rejects_final_declarations_with_a_fix() {
    let d = dir("final_hint");
    std::fs::write(d.join("app.cell"), "cell A {\n  state s {\n    initial: a\n    a -> b\n    final: b\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "app.cell"]);
    assert_ne!(code, 0);
    assert!(out.contains("do not declare 'final'") && out.contains("no outgoing transition is final"), "{out}");
}

#[test]
fn other_cells_are_called_by_qualified_name_and_check_knows_their_handlers() {
    let d = dir("qualified_call");
    let src = r#"
cell Ledger {
  memory { bal: Map<String, Int> }
  on deposit(a: String, n: Int) {
    bal[a] = (bal.get(a) ?? 0) + n
    bal[a]
  }
}
cell Api {
  on request(method: String, path: String, body: Map<String, Any>) {
    Ledger.deposit("x", 5)
  }
}
cell test T {
  rules {
    assert Ledger.deposit("a", 3) == 3
    assert Api.request("POST", "/", map()) == 5
    assert deposit("a", 1) == 4
  }
}
"#;
    std::fs::write(d.join("app.cell"), src).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    std::fs::write(d.join("bad.cell"), src.replace("Ledger.deposit(\"x\", 5)", "Ledger.depositt(\"x\", 5)")).unwrap();
    let (out, code) = soma_in(&d, &["check", "bad.cell"]);
    assert_ne!(code, 0);
    assert!(out.contains("cell 'Ledger' has no handler 'depositt'") && out.contains("did you mean 'deposit'"), "{out}");
}
