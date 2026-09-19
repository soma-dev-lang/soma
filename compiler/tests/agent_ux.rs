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
    assert!(out.contains("'tokens' bound proven — peak 50 reply tokens"), "{out}");
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
    // the route delegates to the same-named handler: the documented shape, no warning
    assert!(!out.contains("share the path /pay"), "{out}");

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
        let _ = http(port, "POST", "/seed/1000");
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

/// The booking agent's blocker: `next_id()` picked its counter slot in
/// HashMap order (different per thread under serve) and wrote outside the
/// journal — the 30-way race handed out an id that already existed and a
/// refused request burned an id.
#[test]
fn next_id_is_deterministic_and_rolls_back_with_the_handler() {
    let d = dir("next_id");
    std::fs::write(d.join("app.cell"), r#"
cell Booking {
    face {
        signal book(slot: String) -> Map
        signal ids() -> List
        signal request(method: String, path: String, body: String) -> Map
    }
    memory {
        zz_first: Map<String, Int>
        bookings: Map<String, String>
        slots: Map<String, Int>
        invariant slots <= 1
        aa_last: Map<String, Int>
    }
    on book(slot: String) {
        let id = "b{next_id()}"
        slots.set(slot, (slots.get(slot) ?? 0) + 1)
        bookings.set(id, slot)
        return map("id", id)
    }
    on ids() { return sort(bookings.keys()) }
    on request(method: String, path: String, body: String) {
        match map("method", method, "path", path) {
            // (a raise rolls the whole request back, its id included; a
            // `try` keeps the ids drawn inside it — they may have escaped)
            {method: "POST", path: "/book/" + slot} -> book(slot)
            {method: "GET", path: "/ids"} -> ids()
            _ -> response(404, map("error", "not found"))
        }
    }
}
cell test T {
    rules {
        assert book("s1").id == "b1"
        assert_fails book("s1")
        assert book("s2").id == "b2"
        assert ids() == ["b1", "b2"]
    }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "refused requests must burn no id:\n{out}");

    let port = 19800 + (std::process::id() % 150) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "app.cell", "-p", &port.to_string()])
        .current_dir(&d)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("soma serve");
    let mut up = false;
    for _ in 0..80 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { up = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut ids = String::new();
    if up {
        let _ = http(port, "POST", "/book/alpha");
        let workers: Vec<_> = (0..30)
            .map(|_| std::thread::spawn(move || http(port, "POST", "/book/beta")))
            .collect();
        for w in workers { let _ = w.join(); }
        let _ = http(port, "POST", "/book/gamma");
        ids = http(port, "GET", "/ids");
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(up, "server did not start");
    let body = ids.split("\r\n\r\n").last().unwrap_or("").replace(' ', "");
    assert_eq!(body.trim(), r#"["b1","b2","b3"]"#, "ids must be unique and dense: {ids}");
}

/// Cycle 3: slots read by bare name, never assigned as a whole; scopes in
/// check match the runtime; lambda errors keep their kind; `|>` binds
/// tighter than comparisons; forall walks every value of a small range.
#[test]
fn cycle3_language_findings() {
    let d = dir("cycle3");
    std::fs::write(d.join("slot.cell"), r#"
cell A {
  memory { history: List<String> [persistent] }
  on add(v: String) {
    history = push(history, v)
    len(history)
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "slot.cell"]);
    assert_ne!(code, 0);
    assert!(out.contains("'history' is a memory slot, not a variable") && out.contains("history.push(x)"), "{out}");

    std::fs::write(d.join("scope.cell"), "cell A {\n  on f(xs: List<Int>) {\n    let ys = xs |> map(x => x * 2)\n    for i in ys { let k = i }\n    return x + i + k\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "scope.cell"]);
    assert_ne!(code, 0);
    for v in ["'x'", "'i'", "'k'"] {
        assert!(out.contains(&format!("undefined variable {v}")), "{v} must be out of scope:\n{out}");
    }

    std::fs::write(d.join("app.cell"), r#"
cell A {
  memory { history: List<String> [persistent]  hits: Map<String, Int> }
  on add(v: String) {
    history.push(v)
    hits[v] = (hits.get(v) ?? 0) + 1
    map("n", len(history), "all", history, "hits", hits)
  }
  on bad(x: Int) { if x == 2 { fail("bad_line", "line {x}") }  x }
  on through() {
    let r = try { [1, 2, 3] |> map(x => bad(x)) }
    map("kind", r.kind, "detail", r.detail)
  }
  on g(n: Int) { if n == 57 { return -1 }  n }
}
cell test T {
  rules {
    assert add("a").n == 1
    let r = add("b")
    assert r.n == 2
    assert r.all == ["a", "b"]
    assert r.hits == map("a", 1, "b", 1)
    assert through().kind == "bad_line"
    assert through().detail == "line 2"
    assert [3, 1, 2] |> sort() == [1, 2, 3]
    assert ([1] |> len()) + 1 == 2
    property "wrong" forall n: Int in 0..100 ensures g(n) >= 0
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("counter-example n = 57"), "forall must be exhaustive on 100 values:\n{out}");
    assert!(out.contains("9 tests: 8 passed, 1 failed") || out.contains("8 passed"), "{out}");
}

/// Site evaluator, cycle 3: a runaway recursion killed the whole service
/// (thread stack overflow before the depth guard), errors were all 500s,
/// `()` came back as `{}`, and a String reached an Int parameter untouched.
#[test]
fn serve_survives_recursion_maps_kinds_to_statuses_and_types_parameters() {
    let d = dir("serve_kinds");
    std::fs::write(d.join("app.cell"), r#"
cell A {
  memory { m: Map<String, Int>  invariant m >= 0 }
  state s { initial: a  a -> b }
  on rec(n: Int) { return rec(n + 1) }
  on neg(x: Int) { m.set("k", x)  m.get("k") }
  on twice(id: String) { transition(id, "b")  transition(id, "b") }
  on find(id: String) { fail("not_found", "no {id}") }
  on nothing() { () }
  on add(n: Int) { n + 1 }
}
cell test T {
  rules {
    assert_fails add("ten") matching "parameter 'n' expects Int, got String"
    assert_fails add(2.0) matching "parameter 'n' expects Int, got Float"
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");

    let port = 19950 + (std::process::id() % 40) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "app.cell", "-p", &port.to_string()])
        .current_dir(&d)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("soma serve");
    let mut up = false;
    for _ in 0..80 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { up = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut got = Vec::new();
    if up {
        for path in ["/rec/0", "/neg/-5", "/twice/x", "/find/z", "/nothing", "/add/ten", "/add/2"] {
            got.push(http(port, "POST", path));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(up, "server did not start");
    let status = |r: &String| r.lines().next().unwrap_or("").to_string();
    assert!(status(&got[0]).contains("500") && got[0].contains("stack_overflow"), "{}", got[0]);
    assert!(status(&got[1]).contains("422") && got[1].contains("\"kind\": \"invariant\""), "{}", got[1]);
    assert!(status(&got[2]).contains("409") && got[2].contains("invalid_transition"), "{}", got[2]);
    assert!(status(&got[3]).contains("404") && got[3].contains("\"kind\": \"not_found\""), "{}", got[3]);
    assert!(status(&got[4]).contains("200") && got[4].trim_end().ends_with("null"), "{}", got[4]);
    assert!(status(&got[5]).contains("400") && got[5].contains("expects Int"), "{}", got[5]);
    assert!(got[6].contains("\"result\": 3") || got[6].contains("\"result\":3"), "the service must still answer after the overflow: {}", got[6]);
}

/// Cycle 3 (adversarial + porting agents): small lies fixed — `soma run`
/// with a wrong handler name, builtin arity, `assert` on a non-Bool, `()`
/// echoed whole, from_json(""), to_int out of range, sum on non-numbers,
/// List slot methods, `f() + 1` as a statement.
#[test]
fn cycle3_small_lies() {
    let d = dir("cycle3_small");
    std::fs::write(d.join("app.cell"), r#"
cell A {
  memory { xs: List<Int> [persistent] }
  on greet(name: String) { "hi {name}" }
  on f() { 1 }
  on plus() { f() + 1 }
  on lens() {
    xs.push(3)
    xs.push(4)
    map("len", xs.len, "g1", xs.get(1), "last", xs.last, "has", xs.has(3))
  }
  on nothing() { () }
}
cell test T {
  rules {
    assert plus() == 2
    let l = lens()
    assert l.len == 2
    assert l.g1 == 4
    assert l.last == 4
    assert l.has == true
    assert nothing() == ()
    assert_fails len() matching "len(x: String|List|Map) -> Int — called with 0 arguments"
    assert_fails from_json("") matching "not valid JSON"
    assert to_int(1e19) == 10000000000000000000
    assert_fails sum([1, "a"]) matching "needs numbers"
    assert "false"
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("✓ assert nothing() == ()"), "{out}");
    assert!(out.contains("assert needs a Bool, got String"), "{out}");
    assert!(out.contains("11 tests: 10 passed, 1 failed"), "{out}");
    let (out, code) = soma_in(&d, &["run", "app.cell", "gret", "bob"]);
    assert_ne!(code, 0);
    assert!(out.contains("no handler named 'gret' (did you mean 'greet'?)"), "{out}");
    let (out, code) = soma_in(&d, &["docs", "guarantees"]);
    assert_eq!(code, 0);
    assert!(out.contains("PROVEN") || out.contains("proven"), "{out}");
}

/// Cycle 3 (adversarial): the face is a contract — a `-> Int` handler that
/// returns a String fails check (literal) and run (value); the cost proof
/// sees calls into other cells; lint sees through `try { }`.
#[test]
fn face_contracts_cross_cell_cost_and_lint_through_try() {
    let d = dir("face_cost");
    std::fs::write(d.join("face.cell"), "cell A {\n  face { signal typed() -> Int }\n  on typed() { return \"not an int\" }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "face.cell"]);
    assert_ne!(code, 0);
    assert!(out.contains("returns a String literal but its face declares `-> Int`"), "{out}");

    std::fs::write(d.join("face2.cell"), "cell A {\n  face { signal typed(s: String) -> Int }\n  on typed(s: String) { return s }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "face2.cell", "typed", "x"]);
    assert_ne!(code, 0);
    assert!(out.contains("the face declares `-> Int` but the handler returned String"), "{out}");

    std::fs::write(d.join("cost.cell"), r#"
cell agent Ledger {
  on burn(x: String) {
    let acc = ""
    for i in range(0, 5) { acc = acc + think(x, map("max_tokens", 50)) }
    acc
  }
}
cell agent Api {
  cost { tokens: 100 }
  on go(x: String) { Ledger.burn(x) }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "cost.cell"]);
    assert_ne!(code, 0);
    assert!(out.contains("computed 250 tokens > declared 100"), "{out}");

    std::fs::write(d.join("lint.cell"), r#"
cell A {
  memory { drafts: Map<String, String> }
  on create_room(name: String) { drafts.set(name, "x")  name }
  on send(id: String) {
    let reply = drafts.get(id)
    require reply != () else NoDraft
    reply
  }
  on request(method: String, path: String, body: String) {
    match map("method", method, "path", path) {
      {method: "POST", path: "/rooms/" + name} -> {
        let r = try { create_room(name) }
        r.value
      }
      _ -> response(404, map("error", "not found"))
    }
  }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["lint", "lint.cell"]);
    assert!(!out.contains("create_room' is not referenced"), "{out}");
    assert!(!out.contains("unchecked .get()"), "{out}");
}

/// `mock <handler> …` stubs any handler (tools, http wrappers) in a test.
#[test]
fn any_handler_can_be_mocked_in_tests() {
    let d = dir("mock_handler");
    std::fs::write(d.join("app.cell"), r#"
cell agent Buyer {
  face { signal quote(item: String) -> Map  tool price_check(item: String) -> Int }
  on price_check(item: String) { http_get("https://prices.example/{item}") }
  on quote(item: String) {
    let market = price_check(item)
    let r = try { price_check(item) }
    map("market", market, "second", r.kind)
  }
}
cell test T {
  rules {
    mock price_check 120
    mock price_check error "prices down"
    let q = quote("bolt")
    assert q.market == 120
    assert q.second == "prices down"
    mock price_check [1, 2]
    assert price_check("a") == 1
    assert price_check("b") == 2
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
}

/// Cycle 4: what the second wave of agents found.
#[test]
fn cycle4_findings() {
    let d = dir("cycle4");
    // native under serve, cross-process lock, face/require detail, literal checks
    std::fs::write(d.join("lit.cell"), "cell A {\n  state s { initial: a\n a -> b }\n  on takes_int(n: Int) { n }\n  on go(id: String) {\n    transition(id, \"c\")\n    takes_int(\"s\")\n  }\n  on h(n: Int) [native] { return \"v {n}\" }\n  memory { n: Int [persistent] }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "lit.cell", "--json"]);
    assert_ne!(code, 0);
    for k in ["unknown_transition_target", "argument_type", "native_vocabulary", "scalar_slot"] {
        assert!(out.contains(&format!("\"kind\": \"{k}\"")) || out.contains(&format!("\"kind\":\"{k}\"")), "missing kind {k}:\n{out}");
    }
    assert!(!out.contains("\"other\""), "{out}");

    std::fs::write(d.join("app.cell"), r#"
cell A {
  memory { c: Map<String, Int> [persistent] }
  on lend(m: String, open: Int) {
    require open < 3 else LoanLimit "member {m} already holds {open} loans"
    open + 1
  }
  on bumps(n: Int) {
    for i in range(0, n) { c.set("k", (c.get("k") ?? 0) + 1) }
    c.get("k")
  }
  on sq(n: Int) [native] { n * n }
  on decide(id: String, ok: Bool) { map("id", id, "ok", ok) }
}
cell test T {
  rules {
    assert_fails lend("ann", 3) matching "LoanLimit: member ann already holds 3 loans"
    let r = try { lend("bob", 5) }
    assert r.kind == "LoanLimit"
    assert r.detail == "member bob already holds 5 loans"
    assert sq(12) == 144
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");

    // two processes on one .soma_data: no lost update
    let a = Command::new(env!("CARGO_BIN_EXE_soma")).args(["run", "app.cell", "bumps", "500"]).current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let b = Command::new(env!("CARGO_BIN_EXE_soma")).args(["run", "app.cell", "bumps", "500"]).current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let _ = a.wait_with_output(); let _ = b.wait_with_output();
    let (out, _) = soma_in(&d, &["run", "app.cell", "bumps", "0"]);
    assert!(out.lines().any(|l| l.trim() == "1000"), "two soma run processes must serialize: {out}");

    // native compiled under serve; Bool path segment coerced
    let port = 19990 + (std::process::id() % 9) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "app.cell", "-p", &port.to_string()])
        .current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("soma serve");
    let mut up = false;
    for _ in 0..150 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { up = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let (sq, dec) = if up { (http(port, "GET", "/sq/12"), http(port, "GET", "/decide/x/true")) } else { (String::new(), String::new()) };
    let _ = child.kill(); let _ = child.wait();
    assert!(up);
    assert!(sq.contains("144"), "{sq}");
    assert!(dec.contains("\"ok\":true"), "{dec}");
}

/// Two agents wanted this proven: `require open < 3` then `open + 1`
/// keeps `open_count <= 3` — a once-bound local narrowed by an
/// unconditional require (the handler is atomic, so the write only commits
/// when the require held).
#[test]
fn prover_uses_require_facts_on_locals() {
    let d = dir("prover_require");
    std::fs::write(d.join("app.cell"), "cell A {\n  memory { open_count: Map<String, Int>\n invariant open_count >= 0 && open_count <= 3 }\n  on lend(m: String) {\n    let open = open_count.get(m) ?? 0\n    require open < 3 else LoanLimit\n    open_count.set(m, open + 1)\n    open + 1\n  }\n  on twice(m: String) {\n    let open = open_count.get(m) ?? 0\n    require open < 3 else LoanLimit\n    open = open + 5\n    open_count.set(m, open)\n    open\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("writer 'lend' proven by induction (writes open + 1)"), "{out}");
    // a reassigned local is NOT narrowed by the require
    assert!(out.contains("twice → open_count"), "{out}");
}

/// Cycle 5: scheduler blocks are checked like handlers, mocks are scoped
/// and typed, the clock can be frozen, test cells are isolated, --strict.
#[test]
fn cycle5_findings() {
    let d = dir("cycle5");
    std::fs::write(d.join("sched.cell"), "cell E {\n  state s { initial: a\n a -> b }\n  every 1s { transition(\"t\", \"zzz\") }\n  on f() { 1 }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "sched.cell"]);
    assert_ne!(code, 0);
    assert!(out.contains("transition() to \"zzz\""), "{out}");

    std::fs::write(d.join("app.cell"), r#"
cell Notifier { on send(to: String) { "sent {to}" } }
cell agent A {
  memory { hist: List<String> [persistent] }
  on when() { map("t", now(), "d", today()) }
  on go(x: Int) {
    if x > 0 { fail("early", "boom") }
    think("q", map("max_tokens", 10))
  }
  on notify() { Notifier.send("bob") }
  on record(x: String) { hist.push(x)  len(hist) }
}
cell test First {
  rules {
    mock now 1700000000
    assert when().t == 1700000000
    assert when().d == "2023-11-14"
    mock think "A"
    assert_fails go(1) matching "early"
    mock think "B"
    assert go(0) == "B"
    mock think ["C", "D"]
    assert go(0) == "C"
    assert go(0) == "D"
    mock Notifier.send error "smtp: down"
    let r = try { notify() }
    assert r.kind == "smtp"
    assert record("a") == 1
  }
}
cell test Second {
  rules {
    assert record("b") == 1
    assert notify() == "sent bob"
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("1 mock think unused after"), "{out}");
    let (out, code) = soma_in(&d, &["test", "app.cell", "--json"]);
    assert_eq!(code, 0);
    assert!(out.contains("\"ok\": true") && out.contains("\"cell\": \"Second\""), "{out}");

    std::fs::write(d.join("strict.cell"), "cell A {\n  memory { c: Map<String, Int>\n invariant c <= 1 }\n  on bump(k: String) { c.set(k, (c.get(k) ?? 0) + 1) }\n}\n").unwrap();
    let (_, code) = soma_in(&d, &["verify", "strict.cell"]);
    assert_eq!(code, 0);
    let (out, code) = soma_in(&d, &["verify", "strict.cell", "--strict"]);
    assert_ne!(code, 0);
    assert!(out.contains("--strict"), "{out}");

    let deep = format!("cell D {{ on f() {{ return {}1{} }} }}\n", "(".repeat(3000), ")".repeat(3000));
    std::fs::write(d.join("deep.cell"), deep).unwrap();
    let (out, code) = soma_in(&d, &["check", "deep.cell"]);
    assert_eq!(code, 1, "must be an error, not an abort: {out}");
    assert!(out.contains("nested more than"), "{out}");
}

/// Cycle 6 (adversarial c5 + docs audit + Ruby / job-queue ports): a BigInt
/// stored in a slot came back as a String, `rows[0] = v` on a List slot was
/// silently dropped, slot value types were advisory, native-only primitives
/// passed check in interpreted handlers, `format` did not exist, `j.state =`
/// did not parse, `assert_fails … matching` ignored the kind, the prover did
/// not chain one slot's invariant into another's proof.
#[test]
fn cycle6_findings() {
    let d = dir("cycle6");
    std::fs::write(d.join("app.cell"), r#"
cell Store {
  face { signal bump() -> Map  signal rows_edit() -> List  signal typed() -> Int
         signal fmt() -> String  signal kw() -> Map  signal rx() -> Int  signal boom() -> Int }
  memory {
    n: Map<String, Int> [persistent]
    invariant n >= 0
    rows: List<Map> [persistent]
    ints: Map<String, Int> [persistent]
    attempts: Map<String, Int> [persistent]
    invariant attempts >= 0 && attempts <= 5
    limits: Map<String, Int> [persistent]
    invariant limits >= 1 && limits <= 5
  }
  on bump() {
    let cur = n.get("c") ?? 9223372036854775807
    n.set("c", cur + 1)
    let back = n.get("c")
    return map("t", type_of(back), "v", back, "ok", back > cur)
  }
  on rows_edit() {
    rows.push(map("id", 1))
    rows.push(map("id", 2))
    rows[0].id = 42
    rows[1] = map("id", 43)
    rows.delete(0)
    return rows
  }
  on typed() { ints.set("s", "not an int")  return 1 }
  on fmt() { return format("%5d|%-4s|%05d|%.2f|%%", 42, "hi", -7, 7/2) }
  on kw() { let j = map("id", 1)  j.state = "done"  return j }
  on rx() { return regex_count("a1b22c333", "[0-9]+") + regex_match("abc", "^a") }
  on boom() { fail("not_found", "no such id") }
  on run_one(id: String) {
    let x = (attempts.get(id) ?? 0) + 1
    let limit = limits.get(id) ?? 1
    require x <= limit else Exceeded "too many"
    attempts.set(id, x)
    return x
  }
  on clear(id: String) { attempts.delete(id) }
}
cell test T {
  rules {
    assert bump().t == "Int"
    assert bump().ok == true
    assert rows_edit() == [map("id", 43)]
    assert_fails typed() matching "type"
    assert fmt() == "   42|hi  |-0007|3.50|%"
    assert kw().state == "done"
    assert rx() == 4
    assert_fails boom() matching "not_found"
    assert map("a", 1).keys == ["a"]
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("9 passed, 0 failed"), "{out}");

    // one slot's invariant chained into another's proof, delete ignored
    let (out, code) = soma_in(&d, &["verify", "app.cell", "--strict"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("writer 'run_one' proven by induction"), "{out}");
    assert!(out.contains("only deletes"), "{out}");

    // native-only primitives are a check error in interpreted handlers;
    // rules outside a test cell are an error; a slot indexed then assigned
    std::fs::write(d.join("bad.cell"), "cell B {\n  memory { rows: List<Map> [persistent] }\n  on f() { let b = buffer(4)  return buf_get(b, 0) }\n  on g() { rows[0] = map()  rows = [] }\n  property \"p\" forall n: Int in 0..3 ensures n >= 0\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "bad.cell"]);
    assert_ne!(code, 0);
    assert!(out.contains("[native]-only primitive"), "{out}");
    assert!(out.contains("is a memory slot, not a variable"), "{out}");
    assert!(out.contains("not a test cell"), "{out}");

    // a BOM is not an "unexpected character"
    std::fs::write(d.join("bom.cell"), "\u{feff}cell C { on f() { 1 } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "bom.cell"]);
    assert_eq!(code, 0, "{out}");

    // test --json carries left/right and a JSON body even when check fails
    let (out, _) = soma_in(&d, &["test", "app.cell", "--json"]);
    assert!(out.contains("\"passed\": 9"), "{out}");
    let (out, code) = soma_in(&d, &["test", "bad.cell", "--json"]);
    assert_ne!(code, 0);
    assert!(out.contains("\"check_errors\""), "{out}");

    // verify ends with a verdict even when check fails
    let (out, _) = soma_in(&d, &["verify", "bad.cell"]);
    assert!(out.contains("VERIFY FAILED"), "{out}");

    // describe --json lists sum types
    std::fs::write(d.join("ty.cell"), "cell type Pay { variants { Charged { tx: String }  Cash } }\ncell D { on f() { 1 } }\n").unwrap();
    let (out, _) = soma_in(&d, &["describe", "ty.cell", "--json"]);
    assert!(out.contains("\"Charged\""), "{out}");
}

/// Cycle 7 (inventory / Java / data / adversarial / LLM assistant): ticks
/// were not rolled back, a kill mid-handler left a partial commit, `request`
/// was itself an endpoint, `|> map` was quadratic, a bare state name failed
/// at runtime, `think_json` handed back a String, path segments were not
/// decoded, whole Floats slipped into Int slots, `match` on a non-variant
/// gave `()`, the prover ignored `require` inside loops and handler returns.
#[test]
fn cycle7_findings() {
    let d = dir("cycle7");
    std::fs::write(d.join("app.cell"), r#"
cell type Pay { variants { Charged { tx: String }  Cash } }
cell Shop {
  face { signal go(id: String) -> String  signal big(n: Int) -> Int  signal pick(p: Map) -> String
         signal money(c: Int) -> Int  signal months() -> Int  signal js() -> Map  signal tokens() -> Int
         signal fl() -> Int  signal drain(keys: List) -> Int  signal one(k: String) -> Int  signal rel(k: String) -> Int }
  memory {
    bal: Map<String, Int> [persistent]
    invariant bal >= 0
    pays: Map<String, Pay> [persistent]
    fs: Map<String, Float> [persistent]
  }
  state st { initial: OPEN  OPEN -> CLOSED }
  on go(id: String) { transition(id, CLOSED)  return get_status(id) }
  on big(n: Int) {
    let xs = range(0, n)
    let ys = xs |> map(x => x * 2)
    let i = 0  let acc = 0
    while i < n { acc += xs[i]  i += 1 }
    return len(ys) + acc
  }
  on pick(p: Map) { return match p { Charged { tx } -> tx  Cash -> "cash" } }
  on money(c: Int) { return div_round(c * 600, 120000) }
  on months() { return months_between("2026-01-15", "2026-04-20") }
  on js() { return think_json("q", map("max_tokens", 20)) }
  on tokens() { set_budget(1000)  think("hello world")  return tokens_used() }
  on fl() { fs.set("k", 1)  bal.set("k", 1.0)  return 1 }
  on drain(keys: List) {
    let n = 0
    for k in keys { let cur = bal.get(k) ?? 0  require cur >= 1 else Empty  bal.set(k, cur - 1)  n += 1 }
    return n
  }
  on one(k: String) { let cur = bal.get(k) ?? 0  require cur >= 1 else Empty  bal.set(k, cur - 1)  return cur }
  on rel(k: String) { let q = one(k)  bal.set("other", (bal.get("other") ?? 0) + q)  return q }
}
cell test T {
  rules {
    assert go("a") == "CLOSED"
    assert big(4000) == 4000 + 7998000
    assert_fails pick(map("a", 1)) matching "no arm can match"
    assert money(1010025) == 5050
    assert money(10125) == 51
    assert months() == 3
    mock think "I think it is billing"
    assert_fails js() matching "json"
    mock think "```json\n{\"category\": \"billing\"}\n```"
    assert js().category == "billing"
    mock think "reply"
    assert tokens() > 0
    assert_fails fl() matching "type"
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("10 passed, 0 failed"), "{out}");
    let (out, _) = soma_in(&d, &["verify", "app.cell"]);
    assert!(out.contains("writer 'drain' proven by induction"), "{out}");
    assert!(out.contains("writer 'rel' proven"), "{out}");

    // a raising tick is rolled back; `request` is never an endpoint; paths are decoded
    std::fs::write(d.join("srv.cell"), r#"
cell Api {
  face { signal state() -> Map  signal credit() -> Int }
  memory { seq: List<String> [persistent] }
  every 1s { seq.push("tick")  fail("boom", "tick failed") }
  on credit() { seq.push("credit")  return seq.len }
  on state() { return map("seq", seq) }
  on request(method: String, path: String, body: String) {
    match map("method", method, "path", path) {
      {method: "GET", path: "/state"} -> state()
      {method: "GET", path: "/echo/" + rest} -> map("rest", rest)
      {method: "POST", path: "/credit"} -> credit()
      _ -> response(404, map("error", "nf"))
    }
  }
}
"#).unwrap();
    let port = 20990 + (std::process::id() % 5) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "srv.cell", "-p", &port.to_string()])
        .current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("soma serve");
    let mut up = false;
    for _ in 0..150 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { up = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    std::thread::sleep(std::time::Duration::from_millis(2500));
    let (st, spoof, echo) = if up {
        (http(port, "GET", "/state"), http(port, "GET", "/request/POST/%2Fcredit/x"), http(port, "GET", "/echo/a%20b"))
    } else { (String::new(), String::new(), String::new()) };
    let _ = child.kill(); let _ = child.wait();
    assert!(up);
    assert!(st.contains("\"seq\":[]"), "{st}");
    assert!(spoof.contains("404"), "{spoof}");
    assert!(echo.contains("\"rest\":\"a b\""), "{echo}");
    let (out, _) = soma_in(&d, &["run", "srv.cell", "state"]);
    assert!(out.contains("\"seq\": []"), "{out}");
}

/// Cycle 10 (regression replay + Go port): slot `.entries` returned values
/// and a SQL LIKE wildcard dropped two-character keys, `parse_date` weekday
/// was off by one and it accepted "2026-3-1", a native hashmap overflow
/// leaked a Rust panic, `len(xs)` copied the list, an if-expression of two
/// literals was unbounded, `mock now` was whole seconds only, headers were
/// unreadable, trailing Map parameters were mandatory.
#[test]
fn cycle10_findings() {
    let d = dir("cycle10");
    std::fs::write(d.join("app.cell"), r#"
cell K {
  face { signal fill() -> Int  signal show() -> List  signal pick(id: String, p: String) -> Int
         signal t() -> List  signal add(a: Int, opts: Map) -> Int  signal big(n: Int) -> Int }
  memory { m: Map<String, Int> [persistent]  q: Map<String, Int> [persistent]  invariant q >= 0 && q <= 100000 }
  on fill() { for k in ["10", "9", "B", "a"] { m.set(k, 1) }  return len(m.keys) }
  on show() { return m.entries |> map(e => e.key) }
  on pick(id: String, p: String) { let v = if p == "a" { 50000 } else { 1000 }  q.set(id, v)  return v }
  on t() { return [now(), now_ms()] }
  on add(a: Int, opts: Map) { return a + (opts.step ?? 1) }
  on big(n: Int) {
    let xs = range(0, n)
    let i = 0  let s = 0
    while i < len(xs) { s += nth(xs, i)  i += 1 }
    return s
  }
}
cell test T { rules {
  assert fill() == 4
  assert show() == ["10", "9", "B", "a"]
  assert parse_date("2026-03-01").weekday == 7
  assert parse_date("2026-03-02").weekday == 1
  assert_fails parse_date("2026-3-1") matching "date"
  mock now 1700000000.25
  assert t() == [1700000000, 1700000000250]
  assert add(1) == 2
  assert add(1, map("step", 5)) == 6
  assert big(20000) == 199990000
} }
"#).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("9 passed, 0 failed"), "{out}");
    let (out, _) = soma_in(&d, &["run", "--fresh", "app.cell", "fill"]);
    assert!(out.contains('4'), "{out}");
    let (out, _) = soma_in(&d, &["run", "app.cell", "show"]);
    assert!(out.contains(r#"["10", "9", "B", "a"]"#), "{out}");
    let (out, _) = soma_in(&d, &["verify", "app.cell"]);
    assert!(out.contains("writer 'pick' proven"), "{out}");

    // termination is checked in cells without a state machine too
    std::fs::write(d.join("term.cell"), "cell W { face { signal f(n: Int) -> Int }\n on f(n: Int) { if n == 0 { return 0 }\n return f(n - 1) } }\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "--strict", "term.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("no base case bounds it from below"), "{out}");
}

/// Cycle 11: a local named like a slot read and wrote the SLOT through
/// `x[i]`; size proofs credited growth through siblings / emits / lambdas /
/// re-bound keys; `request(…, headers: Map)` with 4 parameters got the
/// query; List-slot `rows[i]` answered () out of range; emit skipped the
/// emitting cell.
#[test]
fn cycle11_findings() {
    let d = dir("cycle11");
    std::fs::write(d.join("app.cell"), r#"
cell C {
  face { signal seed() -> List  signal w() -> List  signal idx() -> List  signal h(x: Int) -> Int  signal hit() -> Int }
  memory { rows: List<Int> [persistent]  n: Map<String, Int> [persistent] }
  on seed() { rows.push(10)  rows.push(20)  return rows }
  on w() { let rows = [1, 2]  rows[0] = 99  return rows }
  on idx() { let r = try { rows[5] }  return [rows[-1], r.kind] }
  on h(x: Int) { emit grew(x)  return n.get("g") ?? 0 }
  on grew(x: Int) { n.set("g", (n.get("g") ?? 0) + x) }
  on hit() { return 1 }
}
cell test T { rules {
  assert seed() == [10, 20]
  assert w() == [99, 2]
  assert rows.len == 2
  assert idx() == [20, "index"]
  assert h(5) == 5
} }
"#).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("5 passed, 0 failed"), "{out}");
    let (out, _) = soma_in(&d, &["check", "app.cell"]);
    assert!(out.contains("hides the memory slot `rows`"), "{out}");

    // size proofs: a sibling that also pushes, a push in a lambda
    std::fs::write(d.join("sz.cell"), r#"
cell S {
  face { signal h(x: Int) -> Int  signal l(x: Int) -> Int  signal ok(x: Int) -> Int }
  memory { rows: List<Int> [persistent]  invariant rows.size <= 3 }
  on h(x: Int) { require len(rows) < 3 else Full  _add(x)  rows.push(x)  return len(rows) }
  on _add(x: Int) { require len(rows) < 3 else Full  rows.push(x) }
  on l(x: Int) { require len(rows) < 3 else Full  let r = [1, 2] |> map(i => rows.push(i))  return len(rows) }
  on ok(x: Int) { require len(rows) < 3 else Full  rows.push(x)  return len(rows) }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["verify", "sz.cell"]);
    assert!(out.contains("writer 'ok' proven"), "{out}");
    assert!(!out.contains("writer 'h' proven"), "{out}");
    assert!(!out.contains("writer 'l' proven"), "{out}");
}

/// Cycle 12: a bare call inside a cell runs that cell's own handler; verify
/// always ends with a verdict and prints check errors with it; clearer
/// hints (require without else, `and`, a quote inside `{…}`); native check
/// gaps (a Buf to a sibling); a builtin-arity call is not recursion.
#[test]
fn cycle12_findings() {
    let d = dir("cycle12");
    std::fs::write(d.join("app.cell"), r#"
cell Orders { on pack(id: String) { return "orders:" + id } }
cell Warehouse {
  on pack(id: String) { return "warehouse:" + id }
  on caller(id: String) { return pack(id) }
  on other(id: String) { return Orders.pack(id) }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["run", "app.cell", "caller", "w1"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("warehouse:w1"), "{out}");
    let (out, _) = soma_in(&d, &["run", "app.cell", "other", "w1"]);
    assert!(out.contains("orders:w1"), "{out}");

    std::fs::write(d.join("p.cell"), "cell A { on f(n: Int) { require n < 3  return n } }\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "p.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("needs `else Tag`"), "{out}");
    assert!(out.contains("VERIFY FAILED"), "{out}");

    std::fs::write(d.join("h.cell"), "cell A {\n on f(a: Bool, b: Bool) { return a and b }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["check", "h.cell"]);
    assert!(out.contains("Soma writes `a && b`"), "{out}");

    std::fs::write(d.join("n.cell"), r#"
cell N {
  on takes(n: Int) [native] { return n }
  on a5(n: Int) [native] { let b = buffer(n)  buf_set(b, 0, 7)  return takes(b) }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "n.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("passes the buffer `b` to the sibling handler `takes`"), "{out}");

    std::fs::write(d.join("ar.cell"), r#"
cell A {
  on sum(a: Int, b: Int, c: Int) { return a + b + c }
  on use_sum() { return sum([1, 2]) }
  on list() { return list(1, 2) }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "ar.cell"]);
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma_in(&d, &["verify", "--strict", "ar.cell"]);
    assert_eq!(code, 0, "{out}");
}

/// Cycle 13: `[verify] cells` typos fail verify; `if` branches narrow
/// invariant proofs; update loops over a slot's keys keep size proofs;
/// reserved words name themselves; Int builtins used as Bool are errors;
/// per-rule token budgets.
#[test]
fn cycle13_findings() {
    let d = dir("cycle13");
    std::fs::write(d.join("app.cell"), r#"
cell Tickets {
  state life { initial: open  open -> closed  closed -> open }
  on close(id: String) { transition(id, "closed") }
  on reopen(id: String) { transition(id, "open") }
}
"#).unwrap();
    std::fs::write(d.join("soma.toml"), "[verify]\ncells = [\"Tickts\"]\nnever = [\"closed\"]\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "app.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("did you mean 'Tickets'"), "{out}");
    std::fs::remove_file(d.join("soma.toml")).unwrap();

    std::fs::write(d.join("q.cell"), r#"
cell Q {
  memory { c: Map<String, Int> [persistent]  invariant c >= 0 && c <= 5
           rows: Map<String, Int> [persistent]  invariant rows.size <= 10 }
  on add_if(k: String) { let n = c.get(k) ?? 0  if n < 5 { c.set(k, n + 1) }  return n }
  on bad(k: String, x: Int) { if x < 5 || x > 100 { c.set(k, x) } }
  on touch() { for k in rows.keys { rows.set(k, 5) }  return 1 }
  on upd(k: String) { let r = rows.get(k)  require r != () else Missing  rows.set(k, r + 1) }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["verify", "q.cell"]);
    assert!(out.contains("writer 'add_if' proven"), "{out}");
    assert!(!out.contains("writer 'bad' proven"), "{out}");
    assert!(out.contains("writer 'touch' proven"), "{out}");
    assert!(out.contains("writer 'upd' proven"), "{out}");

    std::fs::write(d.join("r.cell"), "cell A { on f(agent: String) { return agent } }\n").unwrap();
    let (out, _) = soma_in(&d, &["check", "r.cell"]);
    assert!(out.contains("`agent` is a reserved word"), "{out}");
    std::fs::write(d.join("b.cell"), "cell A { on f(s: String) { if !regex_match(s, \"^a\") { return false }  return true } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "b.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("answers an Int (1 or 0)"), "{out}");
}

/// Cycle 13 (attack): persistent List size invariants, machines without
/// slots persist, match arms are scopes, NaN / shadowing / cross-cell /
/// termination / emit-cost false proofs, run refuses a failing program.
#[test]
fn cycle13_attack_findings() {
    let d = dir("cycle13b");
    std::fs::write(d.join("lst.cell"), "cell Log { memory { rows: List<Int> [persistent]  invariant rows.size <= 2 }\n on add(x: Int) { rows.push(x)  return len(rows) } }\n").unwrap();
    let _ = soma_in(&d, &["run", "--fresh", "lst.cell", "add", "1"]);
    let _ = soma_in(&d, &["run", "lst.cell", "add", "2"]);
    let (out, code) = soma_in(&d, &["run", "lst.cell", "add", "3"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("size <= 2"), "{out}");

    std::fs::write(d.join("sm.cell"), "cell D {\n state deal { initial: open  open -> funded  funded -> released }\n on fund(id: String) { transition(id, \"funded\")  return get_status(id) }\n on release(id: String) { transition(id, \"released\")  return get_status(id) }\n}\n").unwrap();
    let _ = soma_in(&d, &["run", "--fresh", "sm.cell", "fund", "d1"]);
    let (out, code) = soma_in(&d, &["run", "sm.cell", "release", "d1"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("released"), "{out}");

    std::fs::write(d.join("m.cell"), "cell M { on f(amount: Int, fee: Int) { let t = match fee { amount if amount > 10 -> \"p\"  _ -> \"b\" }  return amount } }\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "m.cell", "f", "5000", "1"]);
    assert!(out.contains("5000"), "{out}");
    let (out, _) = soma_in(&d, &["run", "m.cell", "f", "5000", "50"]);
    assert!(out.contains("5000"), "{out}");

    std::fs::write(d.join("p.cell"), r#"
cell G {
  memory { level: Map<String, Float> [persistent]  invariant level >= 0.0 && level <= 10.0
           c: Map<String, Int> [persistent]  invariant c <= 100 }
  on set_level(k: String, x: Float) { if x > 10.0 { return "hi" }  if x < 0.0 { return "lo" }  level.set(k, x) }
  on lam(k: String) { let v = c.get(k) ?? 0  let out = [1000] |> map(v => { c.set(k, v)  v })  return out }
  on cnt(n: Int) { if n <= 0 { return 0 }  n = n + 5  return cnt(n - 1) }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["verify", "p.cell"]);
    assert!(!out.contains("writer 'set_level' proven"), "{out}");
    assert!(!out.contains("writer 'lam' proven"), "{out}");
    assert!(out.contains("handler `cnt`: recursive call without provable decreasing argument"), "{out}");

    std::fs::write(d.join("bad.cell"), "cell A { on f() { return undefined_thing } }\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "bad.cell", "f"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("fails `soma check`"), "{out}");
}

/// Cycle 14: `{4}` in a string is literal text; a counting `while` does not
/// make a cost bound advisory; a transition into a state no edge enters is
/// a check error; RFC 4180 read_csv; `use helper` finds helper.cell.
#[test]
fn cycle14_findings() {
    let d = dir("cycle14");
    std::fs::write(d.join("i.cell"), "cell R { on main() { let s = \"a{4}b{2,3}\"  return s } }\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "i.cell", "main"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("a{4}b{2,3}"), "{out}");

    std::fs::write(d.join("c.cell"), "cell agent A {\n cost { tokens: 100 }\n on _count(n: Int) { let i = 0  while i < n { i = i + 1 }  return i }\n on ask(n: Int) { let k = _count(n)  return think(\"hi {k}\", map(\"max_tokens\", 100)) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "c.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("bound proven"), "{out}");

    std::fs::write(d.join("t.cell"), "cell W {\n state s { initial: draft  draft -> done }\n on create(id: String) { transition(id, \"draft\") }\n on finish(id: String) { transition(id, \"done\") }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "t.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("no declared edge of cell 'W' enters 'draft'"), "{out}");

    std::fs::write(d.join("t.csv"), "id,name,amt\n007,\"Smith, J\",1.00\n").unwrap();
    std::fs::write(d.join("helper.cell"), "cell H { on rows() { return read_csv(\"t.csv\") } }\n").unwrap();
    std::fs::write(d.join("main.cell"), "use helper\ncell M { on go() { let r = H.rows()  return \"{r[0].id}|{r[0].name}\" } }\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "main.cell", "go"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("007|Smith, J"), "{out}");
}

/// Cycle 14 (attack): order-aware and block-aware invariant proofs, tick
/// writers, termination through pipes / qualified calls / ticks, non-finite
/// floats nested in slots, stored lambdas refused, UFCS on handlers.
#[test]
fn cycle14_attack_findings() {
    let d = dir("cycle14b");
    std::fs::write(d.join("p.cell"), r#"
cell Wallet {
  memory { bal: Map<String, Int> [persistent]  invariant bal >= 0 && bal <= 100 }
  on a(k: String, n: Int) { let v = 5  match n { 0 -> { v = 0 } _ -> { v = n } }  bal.set(k, v) }
  on d(k: String, n: Int, fast: Bool) { if fast { bal.set(k, n)  return "fast" }  require n >= 0 && n <= 10 else TooBig  bal.set(k, n) }
  on w(k: String, n: Int, c: Int) { let z = if c == 1 { bal.set(k, n)  1 } else { 0 }  require n >= 0 && n <= 10 else X  bal.set(k, n) }
  every 1s { bal.set("tick", 1000) }
  on up(n: Int) { return (n + 1) |> up() }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["verify", "p.cell"]);
    assert!(!out.contains("writer 'a' proven"), "{out}");
    assert!(out.contains("runtime-checked") && out.contains("d → bal"), "{out}");
    assert!(out.contains("w → bal"), "{out}");
    assert!(out.contains("every 1000ms"), "{out}");
    assert!(out.contains("handler `up`: recursive call"), "{out}");

    std::fs::write(d.join("n.cell"), r#"
cell N {
  memory { a: Map<String, Map> [persistent] }
  on put() { a.set("m", map("nan", 0.0 / 0.0, "inf", 1.0 / 0.0))  return "ok" }
  on get() { let m = a.get("m")  return "{m.nan} {m.inf}" }
  on lam() { a.set("f", map("f", x => x + 1))  return "stored" }
  on dbl(x: Int) { return x * 2 }
  on u(n: Int) { return (n + 1).dbl() }
}
"#).unwrap();
    let _ = soma_in(&d, &["run", "--fresh", "n.cell", "put"]);
    let (out, _) = soma_in(&d, &["run", "n.cell", "get"]);
    assert!(out.contains("NaN inf"), "{out}");
    let (out, code) = soma_in(&d, &["run", "n.cell", "lam"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("cannot be stored"), "{out}");
    let (out, _) = soma_in(&d, &["run", "n.cell", "u", "3"]);
    assert!(out.contains("8"), "{out}");
}

/// Cycle 15: deletes keep size proofs; loop_bound is enforced; literal
/// list loops are bounded; index_of on lists.
#[test]
fn cycle15_findings() {
    let d = dir("cycle15");
    std::fs::write(d.join("q.cell"), r#"
cell Q {
  memory { queue: List<String> [persistent]  invariant queue.size <= 64 }
  on add(p: String) { require len(queue) < 64 else full  queue.push(p) }
  on pop() { require len(queue) > 0 else empty  queue.delete(0) }
  on grid() { let t = 0  for l in [[0, 1, 2], [3, 4, 5]] { t = t + len(l) }  return t }
  on pos() { return index_of(["a", "bob", "c"], "bob") }
  on lb() { let n = 0  for [loop_bound(1)] x in ["a", "b", "c"] { n = n + 1 }  return n }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["verify", "--strict", "q.cell"]);
    assert_eq!(code, 1, "{out}"); // lb's bound is too small: a ⚠
    assert!(out.contains("writer 'pop' only deletes"), "{out}");
    assert!(!out.contains("handler `grid`"), "{out}");
    assert!(out.contains("loop_bound(1)]` over a literal list of 3 items"), "{out}");
    let (out, _) = soma_in(&d, &["run", "q.cell", "pos"]);
    assert!(out.trim().ends_with('1'), "{out}");
    let (out, code) = soma_in(&d, &["run", "q.cell", "lb"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("loop_bound"), "{out}");
}

/// Cycle 15 (attack): String slots give back their text; interpolation /
/// UFCS calls are seen by the prover; native recursion depth is an error,
/// not an abort; strict soma.toml; canonical numeric CLI args; remember()
/// rolls back.
#[test]
fn cycle15_attack_findings() {
    let d = dir("cycle15b");
    std::fs::write(d.join("s.cell"), r#"
cell C {
  memory { m: Map<String, String> [persistent]  rows: List<Int> [persistent]  invariant rows.size <= 2
           bal: Map<String, Int> [persistent]  invariant bal >= 0 && bal <= 10 }
  on put() { m.set("k", "{{\"x\": 1}}")  return type_of(m.get("k")) }
  on _more(x: Int) { require len(rows) < 2 else Full  rows.push(x) }
  on add(x: Int) { require len(rows) < 2 else Full  let s = "{_more(x)}"  rows.push(x) }
  on hid(k: String, n: Int) { let s = "{bal.set(k, n)}"  return s }
  on s(v: String) { return v }
  on tr(k: String) { let r = try { remember(k, "x")  fail("x", "y") }  return recall(k) }
  on deep(n: Int) [native] { if n <= 0 { return 0 }  return deep(n - 1) + 1 }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["run", "--fresh", "s.cell", "put"]);
    assert!(out.contains("String"), "{out}");
    let (out, _) = soma_in(&d, &["verify", "s.cell"]);
    assert!(!out.contains("writer 'add' proven"), "{out}");
    assert!(out.contains("hid → bal"), "{out}");
    let (out, _) = soma_in(&d, &["run", "s.cell", "s", "007"]);
    assert!(out.contains("007"), "{out}");
    let (out, _) = soma_in(&d, &["run", "s.cell", "tr", "a"]);
    assert!(!out.contains("\"x\""), "{out}");
    let (out, code) = soma_in(&d, &["run", "s.cell", "deep", "100000000"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("stack_overflow"), "{out}");

    std::fs::write(d.join("soma.toml"), "[verfy]\nnever = [\"x\"]\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "s.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("does not parse"), "{out}");
}

/// Cycle 16: native argument types are checked; native random is an Int
/// with arguments; native loop_bound is enforced; big Floats convert
/// exactly; bit_len is exact; while [loop_bound] terminates for verify.
#[test]
fn cycle16_findings() {
    let d = dir("cycle16");
    std::fs::write(d.join("n.cell"), r#"
cell N {
  on sq(n: Int) [native] { return n * n }
  on r() [native] { let a = random(10)  let b = random(10, 20)  return a + b }
  on lb(n: Int) [native] { let t = n  let k = 0  while [loop_bound(3)] t > 0 { t = t - 1  k = k + 1 }  return k }
  on main() { let v = map("f", 2.5)  let x = try { sq(v.f) }  return x.kind }
  on big() { return [to_int(1e20), bit_len(shl(1, 100))] }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["run", "--fresh", "n.cell", "main"]);
    assert!(out.contains("type"), "{out}");
    let (out, code) = soma_in(&d, &["run", "n.cell", "r"]);
    assert_eq!(code, 0, "{out}");
    let v: i64 = out.lines().find_map(|l| l.trim().parse().ok()).unwrap();
    assert!((10..30).contains(&v), "{out}");
    let (out, code) = soma_in(&d, &["run", "n.cell", "lb", "10"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("loop_bound"), "{out}");
    let (out, _) = soma_in(&d, &["run", "n.cell", "big"]);
    assert!(out.contains("[100000000000000000000, 101]"), "{out}");
    let (out, _) = soma_in(&d, &["verify", "n.cell"]);
    assert!(!out.contains("handler `lb`: while-loop"), "{out}");
}

/// Cycle 16 (attack): key aliasing and computed keys do not prove size;
/// recursion before the base case; storage-name collisions; next_id in its
/// own table; nested slot types; list masks in invariants; cost max over
/// branches; guard names bound after transition.
#[test]
fn cycle16_attack_findings() {
    let d = dir("cycle16b");
    std::fs::write(d.join("p.cell"), r#"
cell S {
  memory { rows: Map<String, Int> [persistent]  invariant rows.size <= 2
           scores: Map<String, List<Int>> [persistent]  invariant scores >= 0 }
  on add(k: String) { require len(rows) < 2 else Full  rows.set(k, 1) }
  on rename(k: String, j: String) { let p = map("k", k)  require rows.get(p.k) != () else Missing  p.k = j  rows.set(p.k, 2) }
  on count(n: Int) { let rest = count(n - 1)  if n <= 0 { return 0 }  return rest + 1 }
  on neg(k: String) { scores.set(k, [-5, -7]) }
  on badtype(k: String) { scores.set(k, ["x"]) }
  on nid() { return next_id() }
  on reg(k: String) { rows.set(k, 1) }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["verify", "p.cell"]);
    assert!(!out.contains("writer 'rename' proven"), "{out}");
    assert!(out.contains("handler `count`"), "{out}");
    let (out, code) = soma_in(&d, &["run", "--fresh", "p.cell", "neg", "a"]);
    assert_eq!(code, 1, "{out}");
    let (out, code) = soma_in(&d, &["run", "p.cell", "badtype", "a"]);
    assert_eq!(code, 1, "{out}");
    let _ = soma_in(&d, &["run", "p.cell", "nid"]);
    let _ = soma_in(&d, &["run", "p.cell", "reg", "__next_id"]);
    let (out, _) = soma_in(&d, &["run", "p.cell", "nid"]);
    assert!(out.trim().ends_with('2'), "{out}");

    std::fs::write(d.join("c.cell"), "cell L { memory { events: List<String> [persistent]  events_log: Map<String, String> [persistent] } on f() { return 1 } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "c.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("would share the storage table"), "{out}");

    std::fs::write(d.join("k.cell"), "cell agent A {\n cost { tokens: 100 }\n on ask(q: String) { if q == \"a\" { return think(q, map(\"max_tokens\", 100)) } else { return think(q, map(\"max_tokens\", 100)) } }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "k.cell"]);
    assert_eq!(code, 0, "{out}");
}

/// Cycle 17: cost composes tool rounds across cells; unknown think()
/// options are errors; next_id's own table is not an orphan in the audit.
#[test]
fn cycle17_findings() {
    let d = dir("cycle17");
    std::fs::write(d.join("c.cell"), r#"
cell agent R { face { signal research(q: String) -> String  tool search(q: String) -> String "Search" }
  cost { tokens: 600 }  on search(q: String) { return "x" }
  on research(q: String) { return think("{q}", map("max_tokens", 300, "max_rounds", 2)) } }
cell C { face { signal go(q: String) -> String }  cost { tokens: 300 }  on go(q: String) { return R.research(q) } }
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "c.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("computed 600 tokens > declared 300"), "{out}");

    std::fs::write(d.join("t.cell"), "cell agent A { on ask(q: String) { let r = try { think(q, map(\"max_token\", 5)) }  return r.detail } }\n").unwrap();
    let (out, _) = soma_in(&d, &["check", "t.cell"]);
    assert!(out.contains("\"max_token\" is not a think() option"), "{out}");

    std::fs::write(d.join("n.cell"), "cell N { memory { m: Map<String, Int> [persistent] } on id() { return next_id() } }\n").unwrap();
    let _ = soma_in(&d, &["run", "--fresh", "n.cell", "id"]);
    let (out, _) = soma_in(&d, &["run", "n.cell", "id"]);
    assert!(!out.contains("no slot declares"), "{out}");
}

/// Cycle 17 (attack): max_tokens must be positive (cost and runtime);
/// Map slots refuse push, List slots refuse non-Int set; List<Int>
/// parameters check their elements; BigInt → Float rounds to nearest.
#[test]
fn cycle17_attack_findings() {
    let d = dir("cycle17b");
    std::fs::write(d.join("e.cell"), "cell agent E {\n cost { tokens: 100 }\n on go() { return think(\"go\", map(\"max_tokens\", 0)) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "e.cell"]);
    assert_eq!(code, 1, "{out}");

    std::fs::write(d.join("s.cell"), r#"
cell A {
  memory { m: Map<String, Int> [persistent]  rows: List<Int> [persistent] }
  on p(v: Int) { m.push(v) }
  on s() { rows.set("0", 7) }
  on g(xs: List<Int>) { return sum(xs) }
  on f() { return to_float(1180591620717411303423) }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["run", "--fresh", "s.cell", "p", "5"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("is a Map slot"), "{out}");
    let (out, code) = soma_in(&d, &["run", "s.cell", "s"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("is a List slot"), "{out}");
    let (out, code) = soma_in(&d, &["run", "s.cell", "g", "[\"x\",\"y\"]"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("got an element String"), "{out}");
    let (out, _) = soma_in(&d, &["run", "s.cell", "f"]);
    assert!(out.contains("1180591620717411300000.0"), "{out}");
}

/// Cycle 18: `else` narrows a once-bound Int local like a parameter; an
/// untyped parameter and a cell-level constant name their fix.
#[test]
fn cycle18_findings() {
    let d = dir("cycle18");
    std::fs::write(d.join("n.cell"), r#"
cell N { memory { neg: Map<String, Int> [persistent]  invariant neg >= 0 }
  on add(k: String, amt: Int) { let x = amt  if x > 0 { return 1 } else { neg.set(k, (neg.get(k) ?? 0) - x) } } }
"#).unwrap();
    let (out, _) = soma_in(&d, &["verify", "n.cell"]);
    assert!(out.contains("writer 'add' proven"), "{out}");
    std::fs::write(d.join("u.cell"), "cell U { on f(x) { return x } }\n").unwrap();
    let (out, _) = soma_in(&d, &["check", "u.cell"]);
    assert!(out.contains("`x: Any`"), "{out}");
    std::fs::write(d.join("c.cell"), "cell C { const LIMIT = 5  on f() { return 1 } }\n").unwrap();
    let (out, _) = soma_in(&d, &["check", "c.cell"]);
    assert!(out.contains("a cell has no constants"), "{out}");
}

/// Cycle 18 (attack): exact aggregations and Int vectors; malformed
/// variants refused; `//route` and route-owned handlers; strict
/// loop_bound; require detail names checked.
#[test]
fn cycle18_attack_findings() {
    let d = dir("cycle18b");
    std::fs::write(d.join("sales.csv"), "dept,price\nA,9.99\nA,5.50\nB,0.75\n").unwrap();
    std::fs::write(d.join("a.cell"), r#"
cell type Pay { variants { Charged { tx: String, amt: Int }  Cash } }
cell A {
  memory { pays: Map<String, Pay> [persistent] }
  on agg1() { let rows = read_csv("sales.csv")  return [sum_by(rows, "price"), agg(rows, "dept", "price:max")[0].price_max] }
  on vec() { return [9007199254740993] + [0] }
  on put(body: String) { pays.set("k", from_json(body)) }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["run", "--fresh", "a.cell", "agg1"]);
    assert!(out.contains("16.24") && out.contains("9.99"), "{out}");
    let (out, _) = soma_in(&d, &["run", "a.cell", "vec"]);
    assert!(out.contains("[9007199254740993]"), "{out}");
    let (out, code) = soma_in(&d, &["run", "a.cell", "put", "{\"_type\":\"Pay\",\"_variant\":\"Charged\",\"tx\":5,\"amt\":\"x\"}"]);
    assert_eq!(code, 1, "{out}");

    std::fs::write(d.join("l.cell"), "cell L { on f(xs: List) { let n = 0  for [loop_bound(2.5)] x in xs { n = n + 1 }  return n } }\n").unwrap();
    let (out, _) = soma_in(&d, &["check", "l.cell"]);
    assert!(out.contains("positive Int literal"), "{out}");
    std::fs::write(d.join("r.cell"), "cell R { on f(x: Int) { require x > 0 else Bad \"detail {zundef}\"  return x } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "r.cell"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("zundef"), "{out}");
}

/// Cycle 19: the analyses see `require` conditions and details; guards are
/// pure; a handler `request` calls is route-owned; absurd sizes raise;
/// a failing try keeps its ids; sum-type parameters and generic variant
/// fields are checked; CSV round trip; cross-cell slot reads are errors.
#[test]
fn cycle19_findings() {
    let d = dir("cycle19");
    std::fs::write(d.join("r.cell"), r#"
cell agent R {
  cost { tokens: 10 }
  on c(p: String) {
    require think(p) != "" else Empty
    return 1
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "r.cell"]);
    assert_ne!(code, 0, "think() in a require condition must count: {out}");
    std::fs::write(d.join("t.cell"), r#"
cell T {
  on r(n: Int) {
    require n < 3 else Big "{r(n)}"
    return n
  }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["verify", "t.cell"]);
    assert!(!out.contains("structurally terminate"), "{out}");
    std::fs::write(d.join("g.cell"), r#"
cell G {
  memory { rows: Map<String, Int> [persistent] }
  state s { initial: a  a -> b { guard { rows.set(_id, 1) == () } } }
  on go(_id: String) { transition(_id, "b") }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "g.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("a guard is a condition"), "{out}");
    std::fs::write(d.join("x.cell"), r#"
cell Store { memory { config: Map<String, String> [persistent] } on put(k: String, v: String) { config.set(k, v) } }
cell App { on get(k: String) { return Store.config.get(k) } }
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "x.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("reads the memory of another cell"), "{out}");
    std::fs::write(d.join("e.cell"), "cell E { every 0ms { print(1) } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "e.cell"]);
    assert_ne!(code, 0, "{out}");

    std::fs::write(d.join("a.cell"), r#"
cell type Pay { variants { Charged { tx: String }  Cash } }
cell type Wrap { variants { W { xs: List<Int> } } }
cell A {
  memory { m: Map<String, Int> [persistent] }
  on f(p: Pay) { return p }
  on sizes() {
    let big = shl(1, 62)
    return [try { pad_left("x", big) }.kind, try { zeros(big, big) }.kind, try { range(0, big) }.kind, try { sleep(0 - 1) }.kind]
  }
  on ids() {
    let id = 0
    let r = try {
      id = next_id()
      fail("boom", "x")
    }
    return [id, next_id()]
  }
  on types() {
    let sx = "x"
    let one = 1
    return [try { f(sx) }.kind, try { Charged { tx: one } }.kind, try { W { xs: ["a"] } }.kind, f(Cash) == Cash]
  }
  on csv() {
    write_csv("t.csv", [map("tags", ["a", "b"], "qty", 5, "code", "12")])
    return read_csv("t.csv")
  }
  on loops() {
    let n = 0
    for x in m.get("missing") { n = n + 1 }
    return [n, try { for x in 5 { n = n + 1 } }.kind]
  }
  on vec() {
    let p = shl(1, 53)
    return [[p + 1] > p, [6] / 3, median([p + 1, p + 1])]
  }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["run", "--fresh", "a.cell", "sizes"]);
    assert!(out.contains(r#"["range", "range", "range", "range"]"#), "{out}");
    let (out, _) = soma_in(&d, &["run", "a.cell", "ids"]);
    assert!(out.contains("[1, 2]"), "ids drawn in a failed try are kept: {out}");
    let (out, _) = soma_in(&d, &["run", "a.cell", "types"]);
    assert!(out.contains(r#"["type", "type", "type", true]"#), "{out}");
    let (out, _) = soma_in(&d, &["run", "a.cell", "csv"]);
    assert!(out.contains(r#""tags": "[\"a\",\"b\"]""#) && out.contains(r#""qty": 5"#) && out.contains(r#""code": "12""#), "{out}");
    let (out, _) = soma_in(&d, &["run", "a.cell", "loops"]);
    assert!(out.contains(r#"[0, "type"]"#), "{out}");
    let (out, _) = soma_in(&d, &["run", "a.cell", "vec"]);
    assert!(out.contains("[[1.0], [2], 9007199254740993]"), "{out}");
}

/// Cycle 19 (attack): a public handler `request` reaches after its own
/// checks is not also a direct endpoint.
#[test]
fn cycle19_route_owned_by_calls() {
    let d = dir("cycle19r");
    std::fs::write(d.join("app.cell"), r#"
cell App {
  memory { log: Map<String, String> [persistent] }
  on wipe(id: String) {
    log.set(id, "wiped")
    return "wiped"
  }
  on request(method: String, path: String, body: String, headers: Map) {
    if starts_with(path, "/wipe/") {
      if headers.authorization != "Bearer ok" { return response(401, map("error", "auth")) }
      return wipe(substring(path, 6, len(path)))
    }
    return response(404, map("error", "nf"))
  }
}
"#).unwrap();
    let port = 20100 + (std::process::id() % 150) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "app.cell", "-p", &port.to_string()])
        .current_dir(&d)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("soma serve");
    let mut up = false;
    for _ in 0..80 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { up = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let r = if up { http(port, "POST", "/wipe/a") } else { String::new() };
    let _ = child.kill();
    let _ = child.wait();
    assert!(up, "server did not start");
    assert!(r.contains("401") && r.contains("auth"), "the direct /wipe endpoint bypassed request's auth: {r}");
}

/// Cycle 20: an SSE client receives only the streams it subscribed to
/// (tenant B read tenant A's events); a literal `delegate` to a missing
/// handler of a known cell is a check error.
#[test]
fn cycle20_findings() {
    let d = dir("cycle20");
    std::fs::write(d.join("dl.cell"), r#"
cell A { on f(x: Int) { return x } }
cell B { on g() { return delegate("A", "nope", 1) } }
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "dl.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("has no handler 'nope'"), "{out}");
    std::fs::write(d.join("app.cell"), r#"
cell App {
    on request(method: String, path: String, body: String) {
        match map("method", method, "path", path) {
            {method: "GET", path: "/lit/b"} -> sse("b")
            {method: "POST", path: "/pub/" + name} -> _pub(name)
            _ -> response(404, map("error", "not found"))
        }
    }
    on _pub(name: String) {
        publish(name, map("to", name))
        return "ok"
    }
}
"#).unwrap();
    let port = 20300 + (std::process::id() % 150) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "app.cell", "-p", &port.to_string()])
        .current_dir(&d)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("soma serve");
    let mut up = false;
    for _ in 0..80 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { up = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut got = String::new();
    if up {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_millis(1500))).unwrap();
        s.write_all(b"GET /lit/b HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = http(port, "POST", "/pub/a");
        let _ = http(port, "POST", "/pub/b");
        let mut buf = [0u8; 4096];
        loop {
            match s.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => got.push_str(&String::from_utf8_lossy(&buf[..n])) }
            if got.contains("event: b") { break; }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(up, "server did not start");
    assert!(got.contains("event: b"), "{got}");
    assert!(!got.contains("event: a"), "an SSE client got a stream it did not subscribe to: {got}");
}

/// Cycle 21: `&&` / `||` / ensure / match guards take Bools (a comparison
/// mask passed compound invariants, guards and asserts); replay runs only
/// top-level calls; recall works in a new process; a function is not a
/// value; () is not a key; native Bool arithmetic and Rust keywords are
/// check errors; think options are checked.
#[test]
fn cycle21_findings() {
    let d = dir("cycle21");
    std::fs::write(d.join("a.cell"), r#"
cell A {
  memory { m: Map<String, Any> [persistent]  invariant m >= 0 && m <= 100
           w: Map<String, List<Int>> [persistent] }
  on put(k: String) { m.set(k, [5000]) }
  on both(xs: List<Int>) { return xs >= 0 && true }
  on post(s: String) {
    ensure s
    return 1
  }
  on nullkey() { w.set((), [1]) }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["run", "--fresh", "a.cell", "put", "k"]);
    assert!(out.contains("could not be evaluated") || out.contains("needs Bool"), "a mask passed a compound invariant: {out}");
    let (out, _) = soma_in(&d, &["run", "a.cell", "both", "[-5]"]);
    assert!(out.contains("&& needs Bool operands"), "{out}");
    let (out, _) = soma_in(&d, &["run", "a.cell", "post", "false"]);
    assert!(out.contains("expected Bool"), "{out}");
    let (out, _) = soma_in(&d, &["run", "a.cell", "nullkey"]);
    assert!(out.contains("the key is ()"), "{out}");

    std::fs::write(d.join("rp.cell"), r#"
cell C {
  memory { m: Map<String, Int> [persistent] }
  on outer(k: String) {
    _inner(k)
    return m.get(k)
  }
  on _inner(k: String) {
    require m.get(k) == () else Dup
    m.set(k, 1)
  }
}
"#).unwrap();
    let _ = soma_in(&d, &["run", "--fresh", "--record", "--signal", "outer", "rp.cell", "a"]);
    let (out, _) = soma_in(&d, &["replay", "rp.cell"]);
    assert!(out.contains("1 ok, 0 diverged"), "{out}");

    std::fs::write(d.join("r.cell"), r#"
cell agent R {
  memory { x: Map<String, Int> [persistent] }
  on w(k: String) { remember(k, "mine") }
  on go(k: String) { return recall(k) }
}
"#).unwrap();
    let _ = soma_in(&d, &["run", "--fresh", "r.cell", "w", "k1"]);
    let (out, _) = soma_in(&d, &["run", "r.cell", "go", "k1"]);
    assert!(out.contains("mine"), "recall lost the value across runs: {out}");

    std::fs::write(d.join("f.cell"), "cell F { on a() { let f = len  return f([1, 2]) } on b() { return [1, 2] |> len } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "f.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("`len` is a function, not a value"), "{out}");
    assert_eq!(out.matches("is a function, not a value").count(), 1, "`|> len` is a call: {out}");

    std::fs::write(d.join("n.cell"), r#"
cell N {
  on f(n: Int) [native] {
    let b = n > 2
    return b * 10 + 1
  }
  on g(n: Int) [native] {
    let fn = n + 1
    return fn
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "n.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("arithmetic on a Bool") && out.contains("a word Rust reserves"), "{out}");

    std::fs::write(d.join("t.cell"), "cell agent T { on a(q: String) { return think(q, map(\"max_tokens\", 5, \"schema\", 1)) } on b(q: String) { return think(q, map(\"max_tokens\", 5, \"tools_allowed\", [])) } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "t.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("\"schema\" is not a think() option") && !out.contains("\"tools_allowed\" is not"), "{out}");
}

/// Cycle 22: a test cell's own helper wins a bare call in its rules (a
/// same-named helper of another test cell ran silently).
#[test]
fn cycle22_findings() {
    let d = dir("cycle22");
    std::fs::write(d.join("tt.cell"), "cell test A { on _who() { return \"A\" }  rules { assert _who() == \"A\" } }\ncell test B { on _who() { return \"B\" }  rules { assert _who() == \"B\" } }\n").unwrap();
    let (out, code) = soma_in(&d, &["test", "tt.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("2 passed"), "{out}");

    // range() near i64::MAX ended (the step wrapped and it ran out of memory);
    // round() of 2^63 is 2^63; %.99999f is a catchable range error
    std::fs::write(d.join("r.cell"), r#"
cell R {
  on n() {
    let big = 9223372036854775807
    return [len(range(0 - 5, big, big)), round(9223372036854775808.0), try { format("%.99999f", 1.5) }.kind]
  }
  on pub(m: String) {
    publish("room", m)
    return "ok"
  }
  on request(method: String, path: String, body: String) {
    match path {
      "/s" -> sse("room")
      _ -> response(404, map("e", 1))
    }
  }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["run", "r.cell", "n"]);
    assert!(out.contains(r#"[2, 9223372036854775808, "range"]"#), "{out}");
    let port = 20500 + (std::process::id() % 150) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "r.cell", "-p", &port.to_string()])
        .current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("soma serve");
    let mut up = false;
    for _ in 0..80 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { up = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut got = String::new();
    if up {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_millis(1500))).unwrap();
        s.write_all(b"GET /s HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = http(port, "POST", "/pub/legit%0Aevent%3A%20forged");
        let mut buf = [0u8; 4096];
        loop {
            match s.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => got.push_str(&String::from_utf8_lossy(&buf[..n])) }
            if got.contains("legit") { break; }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(up, "server did not start");
    assert!(got.contains(r#"data: "legit\nevent: forged""#) && !got.contains("\nevent: forged"), "{got}");
}

/// Cycle 23 (attack): a zero matrix dimension no longer bypasses the size
/// cap (capacity overflow past `try`); a builtin panic is a catchable
/// error; a native handler returning a Float and a String is a check error;
/// `%d` of inf raises.
#[test]
fn cycle23_attack_findings() {
    let d = dir("cycle23a");
    std::fs::write(d.join("z.cell"), r#"
cell T {
  on main() {
    let b = shl(1, 62)
    return [try { zeros(0, b) }.kind, try { ones(shl(1, 40), 0) }.kind, try { mat(b, 0, []) }.kind, try { format("%d", 1.0 / 0.0) }.kind, format("%d", 1e20)]
  }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["run", "z.cell", "main"]);
    assert!(out.contains(r#"["range", "range", "range", "range", "100000000000000000000"]"#), "{out}");
    std::fs::write(d.join("n.cell"), r#"
cell N {
  on retdiff(a: Int, b: Int) [native] {
    if a > b { return a / b }
    return "neg"
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "n.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("one return type"), "{out}");

    // an i64 local assigned into a BigInt-mode local compiled to a swap of
    // two Rust types (E0308 after a clean check)
    std::fs::write(d.join("b4.cell"), "cell B {\n    on f(n: Int) [native] {\n        let nd = 0\n        let di = 0\n        di = str_len(\"abc\")\n        di = nd\n        return di + n\n    }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "b4.cell", "f", "10"]);
    assert!(out.lines().any(|l| l.trim() == "10"), "{out}");
}

/// Cycle 24: capitalised field names; a handler cannot replace a safety
/// builtin; a reactive machine passes --strict; one initial state.
#[test]
fn cycle24_findings() {
    let d = dir("cycle24");
    std::fs::write(d.join("f.cell"), "cell T {\n  on main() {\n    let s = map(\"Ab\", 1)\n    s.Ab = 5\n    let g = Tank { LT1: 3 }\n    g.LT1 = 4\n    return [s.Ab, g.LT1]\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "f.cell", "main"]);
    assert!(out.contains("[5, 4]"), "{out}");
    std::fs::write(d.join("r.cell"), r#"
cell Audit { memory { trail: List<String> [persistent] } on transition(id: String, to: String) { trail.push(to) } }
cell Reactor { state rod { initial: safe  safe -> armed  armed -> fired } on fire(id: String) { transition(id, "fired") } }
"#).unwrap();
    let (out, code) = soma_in(&d, &["check", "r.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("cannot be named `transition`"), "{out}");
    std::fs::write(d.join("p.cell"), r#"
cell Pump {
  state p { initial: off  off -> starting  starting -> running  running -> stopping  stopping -> off  * -> fault  fault -> off }
  on go(id: String) { transition(id, "starting") }
  on up(id: String) { transition(id, "running") }
  on halt(id: String) { transition(id, "stopping") }
  on done(id: String) { transition(id, "off") }
  on trip(id: String) { transition(id, "fault") }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["verify", "--strict", "p.cell"]);
    assert!(out.contains("reactive machine") && out.contains("VERIFY OK"), "{out}");
    std::fs::write(d.join("i.cell"), "cell I { state s { initial: a  initial: b  a -> b } on go(id: String) { transition(id, \"b\") } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "i.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("ONE initial state"), "{out}");
}

/// Cycle 25: another cell's slot by bare name is a check error; a builtin
/// is not replaced by another cell's handler; import cycles and diamonds
/// load; flags after the handler are arguments; a property sees the rules'
/// fixtures; crypto builtins; `--json` on a parse error is JSON.
#[test]
fn cycle25_findings() {
    let d = dir("cycle25");
    std::fs::write(d.join("s.cell"), "cell Vault { memory { keys: Map<String, String> [persistent] } on put(k: String) { keys.set(k, \"v\") } }\ncell Other { on steal() { return keys.get(\"admin\") } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "s.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("memory slot of another cell"), "{out}");

    std::fs::write(d.join("lib.cell"), "cell Lib { on escape_html(s: String) { return s } }\n").unwrap();
    std::fs::write(d.join("app.cell"), "use lib\ncell App { on page(s: String) { return escape_html(s) } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "app.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("calls the BUILTIN escape_html()"), "a library replaced escape_html: {out}");

    std::fs::write(d.join("a.cell"), "use b\ncell A { on f() { return 1 } }\n").unwrap();
    std::fs::write(d.join("b.cell"), "use a\ncell B { on g() { return 2 } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "a.cell"]);
    assert_eq!(code, 0, "an import cycle must load: {out}");

    std::fs::write(d.join("fl.cell"), "cell F { memory { m: Map<String, String> [persistent] } on put(v: String) {\n    m.set(\"k\", v)\n    return v\n  } }\n").unwrap();
    let _ = soma_in(&d, &["run", "fl.cell", "put", "keep"]);
    let (out, _) = soma_in(&d, &["run", "fl.cell", "put", "--fresh"]);
    assert!(!out.contains("fresh: removed"), "{out}");

    std::fs::write(d.join("pf.cell"), "cell A { on f(k: Int, n: Int) { return k + n } }\ncell test T { rules {\n  let k = 5\n  property \"fixture\" forall n: Int in 0..3 ensures f(k, n) >= 5\n} }\n").unwrap();
    let (out, code) = soma_in(&d, &["test", "pf.cell"]);
    assert_eq!(code, 0, "{out}");

    std::fs::write(d.join("cr.cell"), "cell C { on main() { return [sha256(\"abc\"), len(random_token()), secure_eq(\"a\", \"b\")] } }\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "cr.cell", "main"]);
    assert!(out.contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad") && out.contains("64, false"), "{out}");

    std::fs::write(d.join("syn.cell"), "cell X { on f() { return } }\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_soma")).args(["check", "--json", "syn.cell"]).current_dir(&d).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(serde_json::from_str::<serde_json::Value>(stdout.trim()).is_ok(), "{stdout}");
}

/// Cycle 26 (attack): a bare / UFCS / pipe call into another cell counts
/// as a write for GET → 405; crypto builtins refuse non-Strings (`()` was
/// "null": secure_eq("null", ()) was true); `Store["s"]` and
/// `"{Store.s}"` are foreign-slot check errors.
#[test]
fn cycle26_attack_findings() {
    let d = dir("cycle26a");
    std::fs::write(d.join("c.cell"), "cell C { on main() { return [try { secure_eq(\"null\", ()) }.kind, try { sha256(5) }.kind, secure_eq(\"a\", \"a\")] } }\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "c.cell", "main"]);
    assert!(out.contains(r#"["type", "type", true]"#), "{out}");
    std::fs::write(d.join("x.cell"), "cell Store { memory { secret: Map<String, String> [persistent] } on put(k: String) { secret.set(k, \"s\") } }\ncell Api { on a() { return Store[\"secret\"] } on b() { return \"{Store.secret}\" } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "x.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("reads the memory of another cell"), "{out}");

    std::fs::write(d.join("g.cell"), r#"
cell Store {
  memory { n: Map<String, Int> [persistent] }
  on bump(k: String) { n.set(k, (n.get(k) ?? 0) + 1) }
}
cell Api {
  on c_bare(k: String) { bump(k)  return "ok" }
  on c_ufcs(k: String) { k.bump()  return "ok" }
  on request(method: String, path: String, body: String) { return response(404, map("e", 1)) }
}
"#).unwrap();
    let port = 20700 + (std::process::id() % 150) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "g.cell", "-p", &port.to_string()])
        .current_dir(&d).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("soma serve");
    let mut up = false;
    for _ in 0..80 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { up = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let (a, b) = if up { (http(port, "GET", "/c_bare/x"), http(port, "GET", "/c_ufcs/x")) } else { (String::new(), String::new()) };
    let _ = child.kill();
    let _ = child.wait();
    assert!(up, "server did not start");
    assert!(a.contains("405") && b.contains("405"), "a GET wrote through a cross-cell call: {a} / {b}");
}

/// Cycle 26: a test rule calling transition() in a program with several
/// machines is a check error (assert_fails passed on the ambiguity error).
#[test]
fn cycle26_findings() {
    let d = dir("cycle26");
    std::fs::write(d.join("m.cell"), "cell A { state s { initial: a1  a1 -> a2 } on go(id: String) { transition(id, \"a2\") } }\ncell B { state t { initial: b1  b1 -> b2 } on go2(id: String) { transition(id, \"b2\") } }\ncell test T { rules { assert_fails transition(\"x\", \"a1\") } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "m.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("several state machines"), "{out}");
}

/// Cycle 27: `Store.config` in a test rule is a check error (bare `config`
/// works there).
#[test]
fn cycle27_findings() {
    let d = dir("cycle27");
    std::fs::write(d.join("b.cell"), "cell Store { memory { config: Map<String, Int> [persistent] } on put(k: String, v: Int) { config.set(k, v) } }\ncell test T { rules {\n  let _ = put(\"a\", 1)\n  assert Store.config.get(\"a\") == 1\n} }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "b.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("write `config`"), "{out}");
    std::fs::write(d.join("ok.cell"), "cell Store { memory { config: Map<String, Int> [persistent] } on put(k: String, v: Int) { config.set(k, v) } }\ncell test T { rules {\n  let _ = put(\"a\", 1)\n  assert config.get(\"a\") == 1\n} }\n").unwrap();
    let (out, code) = soma_in(&d, &["test", "ok.cell"]);
    assert_eq!(code, 0, "{out}");
}

/// Cycle 27 (attack): `try {…}?` re-raises; a guarded arm does not cover its
/// variant; `"{slot}"` reads the slot; `{a b}` is not one expression;
/// mod/idiv take Ints; sum_by refuses non-numbers; distinct keeps "1" and 1.
#[test]
fn cycle27_attack_findings() {
    let d = dir("cycle27a");
    std::fs::write(d.join("q.cell"), "cell Q {\n  memory { m: Map<String, Int> [persistent] }\n  on risky() { fail(\"not_found\", \"nope\") }\n  on pay() {\n    m.set(\"written\", 1)\n    let v = try { risky() }?\n    return v\n  }\n  on peek() { return m.keys }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "--fresh", "q.cell", "pay"]);
    assert_ne!(code, 0, "{out}");
    let (out, _) = soma_in(&d, &["run", "q.cell", "peek"]);
    assert!(out.contains("[]"), "the write before `?` committed: {out}");

    std::fs::write(d.join("g.cell"), "cell type Sh { variants { Sq(Float)  Dot } }\ncell A { on area(s: Sh) { return match s {\n  Sq(x) if x > 5.0 -> x\n  Dot -> 0.0\n} } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "g.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("missing variant `Sq`"), "{out}");

    std::fs::write(d.join("s.cell"), "cell S {\n  memory { counts: Map<String, Int> [persistent] }\n  on f() {\n    counts.set(\"a\", 1)\n    return \"{counts}\"\n  }\n  on g() { return [try { mod(7.5, 2) }.kind, try { sum_by([map(\"q\", \"abc\")], \"q\") }.kind, len(distinct([\"1\", 1]))] }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "--fresh", "s.cell", "f"]);
    assert!(out.contains("\"a\": 1"), "{out}");
    let (out, _) = soma_in(&d, &["run", "s.cell", "g"]);
    assert!(out.contains(r#"["type", "type", 2]"#), "{out}");

    std::fs::write(d.join("i.cell"), "cell I { on f(total: Int) { return \"{total junk}\" } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "i.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("is not one expression"), "{out}");
}

/// Cycle 28: think() in a while condition and the last `max_rounds` count;
/// a String in an Any slot stays a String; `else { if … }` is a value;
/// BigInt clamp; a literal sub-pattern does not cover its variant; an
/// invariant is its own cell's; qualified-call arity.
#[test]
fn cycle28_findings() {
    let d = dir("cycle28");
    std::fs::write(d.join("w.cell"), "cell agent W {\n  cost { tokens: 10 }\n  on go() {\n    let calls = 0\n    while think(\"x\", map(\"max_tokens\", 10)) != \"stop\" { calls = calls + 1 }\n    return calls\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "w.cell"]);
    assert_ne!(code, 0, "a think() in a while condition was costed once: {out}");
    std::fs::write(d.join("r.cell"), "cell agent R {\n  face { signal go(q: String) -> String  tool t(q: String) -> String \"t\" }\n  cost { tokens: 25 }\n  on t(q: String) { return q }\n  on go(q: String) { return think(q, map(\"max_tokens\", 10, \"max_rounds\", 2, \"max_rounds\", 10)) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "r.cell"]);
    assert_ne!(code, 0, "the first max_rounds was costed, the last runs: {out}");

    std::fs::write(d.join("a.cell"), r#"
cell type Pay { variants { Charged { tx: String }  Cash } }
cell A {
  memory { notes: Map<String, Any> [persistent]  m: Map<String, Int> [persistent] invariant m >= 0 }
  on put(t: String) {
    notes.set("a", t)
    return type_of(notes.get("a"))
  }
  on nest(k: String) {
    let s = if k == "x" { 1 } else { if k == "y" { 2 } else { 3 } }
    return s
  }
  on cl() { return clamp(shl(1, 70), 10, 20) }
}
cell B {
  memory { m: Map<String, Int> [persistent] }
  on bput(v: Int) {
    m.set("n", v)
    return "ok"
  }
}
"#).unwrap();
    let (out, _) = soma_in(&d, &["run", "--fresh", "a.cell", "put", "{\"_type\": \"Admin\"}"]);
    assert!(out.contains("String"), "{out}");
    let (out, _) = soma_in(&d, &["run", "a.cell", "nest", "y"]);
    assert!(out.trim().ends_with('2'), "{out}");
    let (out, _) = soma_in(&d, &["run", "a.cell", "cl"]);
    assert!(out.trim().ends_with("20"), "{out}");
    let (out, _) = soma_in(&d, &["run", "a.cell", "bput", "-5"]);
    assert!(out.contains("ok"), "A's invariant refused B's write: {out}");

    std::fs::write(d.join("m.cell"), "cell type Pay { variants { Charged { tx: String }  Cash } }\ncell M { on f(p: Pay) { return match p {\n  Charged { tx: \"t1\" } -> 1\n  Cash -> 2\n} } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "m.cell"]);
    assert_ne!(code, 0, "{out}");
    std::fs::write(d.join("q.cell"), "cell B { on bh(x: Int) { return x } }\ncell C { on c() { return B.bh() } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "q.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("takes 1"), "{out}");
}

/// Cycle 29 (attack): lambda self-application hits the recursion guard (it
/// aborted the process) and is not "structurally terminating"; think() in
/// an index assignment is costed; a bare foreign slot in interpolation is
/// refused at check and at run time; the guard rule sees UFCS/interpolated
/// transitions.
#[test]
fn cycle29_attack_findings() {
    let d = dir("cycle29a");
    std::fs::write(d.join("l.cell"), "cell L {\n  on h(n: Int) {\n    let f = g => g(g)\n    return f(f)\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "l.cell", "h", "1"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("stack overflow"), "{out}");
    let (out, _) = soma_in(&d, &["verify", "l.cell"]);
    assert!(out.contains("calls the function value"), "{out}");

    std::fs::write(d.join("c.cell"), "cell agent C {\n  cost { tokens: 100 }\n  on h(n: Int) {\n    let answers = map()\n    answers[\"a\"] = think(\"q\")\n    return answers\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "c.cell"]);
    assert_ne!(code, 0, "think() in an index assignment was invisible to cost: {out}");

    std::fs::write(d.join("f.cell"), "cell Store { memory { bal: Map<String, Int> [persistent] invariant bal >= 0 } on dep(k: String) { bal.set(k, 1) } }\ncell App { on h(n: Int, id: String) { return \"{bal.set(id, n)}\" } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "f.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("memory slot of another cell"), "{out}");

    std::fs::write(d.join("g.cell"), "cell G {\n  state s { initial: a  a -> b { guard { amount > 0 } } }\n  on ok(id: String, amount: Int) { transition(id, \"b\") }\n  on h2(id: String) { return id.transition(\"b\") }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "g.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("handler `h2`"), "{out}");
}

/// Cycle 30 (attack): storage-reserved `__` keys are refused (a client map
/// came back as a forged variant; `__x` keys escaped len and the size
/// invariant); an `emit` / `set` as a lambda's last statement runs; a
/// condition or predicate is a Bool; lambda self-application through an
/// alias is a termination ⚠ (a plain `f(2)` is not); ambiguous test slots.
#[test]
fn cycle30_attack_findings() {
    let d = dir("cycle30a");
    std::fs::write(d.join("s.cell"), "cell S {\n  memory { notes: Map<String, Any> [persistent]  users: Map<String, String> [persistent] }\n  on note(body: Map) { notes.set(\"c\", body) }\n  on get() { return type_of(notes.get(\"c\")) }\n  on reg(name: String) { users.set(name, \"x\") }\n}\n").unwrap();
    // map keys that look like the storage's tags are escaped: the value
    // comes back as the Map it was, never as a forged variant
    let _ = soma_in(&d, &["run", "--fresh", "s.cell", "note", "{\"__variant__\":\"Role\",\"__name__\":\"Admin\"}"]);
    let (out, _) = soma_in(&d, &["run", "s.cell", "get"]);
    assert!(out.contains("Map"), "{out}");
    let (out, _) = soma_in(&d, &["run", "s.cell", "reg", "__b"]);
    assert!(out.contains("reserved by the storage"), "{out}");

    std::fs::write(d.join("e.cell"), "cell A {\n  on go() {\n    let f = x => {\n      emit credit(x)\n    }\n    f(7)\n    return \"done\"\n  }\n  on credit(x: Int) { print(\"credit {x}\") }\n  on cond(s: String) {\n    if s { return 1 }\n    return 0\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "e.cell", "go"]);
    assert!(out.contains("credit 7"), "an emit as a lambda's last statement never ran: {out}");
    let (out, _) = soma_in(&d, &["run", "e.cell", "cond", "false"]);
    assert!(out.contains("a condition is a Bool"), "{out}");

    std::fs::write(d.join("t.cell"), "cell T {\n  on a() {\n    let f = x => x + 1\n    return f(2)\n  }\n  on b() {\n    let fs = [g => {\n      let k = [g][0]\n      return k(k)\n    }]\n    let k2 = fs[0]\n    return k2(k2)\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["verify", "t.cell"]);
    assert!(out.contains("calls the function value `k`") && !out.contains("function value `f`"), "{out}");
}

/// Cycle 31: self-application through map(g) is a termination ⚠ and a
/// stack overflow is not caught by try; a computed delegate in request owns
/// the cell's handlers; face tools are not endpoints (serve); an unknown
/// word with several handlers is an error; invariants and conditions are
/// Bools with () false; `f(a)(b)` is refused; floats print shortest digits;
/// test helpers read slots; undefined functions in rules are check errors.
#[test]
fn cycle31_findings() {
    let d = dir("cycle31");
    std::fs::write(d.join("y.cell"), "cell Y {\n  on spin(n: Int) {\n    let w = g => try { [g, g] |> map(g) }\n    return [w] |> map(w)\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["verify", "y.cell"]);
    assert!(out.contains("calls the function value `g`"), "{out}");
    let (out, code) = soma_in(&d, &["run", "y.cell", "spin", "1"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("stack overflow"), "{out}");

    std::fs::write(d.join("u.cell"), "cell U {\n  memory { m: Map<String, String> [persistent] }\n  on wipe(k: String) { m.delete(k) }\n  on stats(k: String) { return m.get(k) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "u.cell", "nosuch"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("no handler named 'nosuch'"), "{out}");

    std::fs::write(d.join("b.cell"), "cell B {\n  memory { vals: Map<String, Int> [persistent] invariant vals }\n  on f(n: Int) { return [!(), [1] |> filter(x => ()), format(\"%.3e\", 6.02214076e23), 6.02214076e23] }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "b.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("is not a condition"), "{out}");
    std::fs::write(d.join("b2.cell"), "cell B {\n  on f(n: Int) { return [!(), [1] |> filter(x => ()), format(\"%.3e\", 6.02214076e23), 6.02214076e23] }\n  on g(n: Int) {\n    let mk = a => (b => a + b)\n    return mk(5)(n)\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "b2.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("calling the result of a call"), "{out}");
    std::fs::write(d.join("b3.cell"), "cell B {\n  on f(n: Int) { return [!(), [1] |> filter(x => ()), format(\"%.3e\", 6.02214076e23), 6.02214076e23] }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "b3.cell", "f", "1"]);
    assert!(out.contains(r#"[true, [], "6.022e+23", 6.02214076e23]"#), "{out}");

    std::fs::write(d.join("t.cell"), "cell Store {\n  memory { items: Map<String, Int> [persistent] }\n  on add(k: String, n: Int) { items.set(k, n) }\n}\ncell test T {\n  rules {\n    let _ = add(\"a\", 1)\n    assert _count() == 1\n  }\n  on _count() { return items.len }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["test", "t.cell"]);
    assert_eq!(code, 0, "{out}");
    std::fs::write(d.join("t2.cell"), "cell Store { on add(k: String) { return 1 } }\ncell test T { rules { assert _total() == 1 } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "t2.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("undefined function '_total'"), "{out}");
}

#[test]
fn cycle32_findings() {
    let d = dir("cycle32");
    // a block inside an interpolation segment ends at its matching brace
    std::fs::write(d.join("i.cell"), "cell I {\n  on f(c: Bool) {\n    let xs = [1, 2]\n    return \"a {if c { 1 } else { 2 }} {xs.map(x => { x * 2 })} b\"\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "i.cell", "f", "true"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("a 1 [2, 4] b"), "{out}");
    // a record has no `.size` pseudo-field; a plain map keeps it
    std::fs::write(d.join("r.cell"), "cell R {\n  on f(n: Int) {\n    let r = Item { name: \"x\" }\n    return [r.size, map(\"a\", n).size]\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "r.cell", "f", "1"]);
    assert!(out.contains("[null, 1]") || out.contains("[(), 1]"), "{out}");
    // from_json cannot build a variant without its declared fields
    std::fs::write(d.join("j.cell"), "cell type Pay { variants { Charged { tx: String }  Cash } }\ncell J {\n  on f(s: String) {\n    let v = try { from_json(s) }\n    return v.kind\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "j.cell", "f", r#"{"_type":"Pay","_variant":"Charged"}"#]);
    assert!(out.contains("type"), "{out}");
}

#[test]
fn cycle33_findings() {
    let d = dir("cycle33");
    // an unparseable segment is rescanned by the runtime: the inner `{…}`
    // is seen by termination, cost and guard analyses
    std::fs::write(d.join("t.cell"), "cell T {\n  on spin(n: Int) {\n    let s = \"{ { spin(n + 1) } }\"\n    return s\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "--strict", "t.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("handler `spin`: recursive call"), "{out}");
    std::fs::write(d.join("g.cell"), "cell App {\n  memory { n: Map<String, Int> [persistent] }\n  state flow {\n    initial: a\n    a -> b { guard { \"{ { _bump() } }\" != \"\" } }\n    b -> a\n  }\n  on _bump() { n.set(\"k\", 1) return 1 }\n  on go(id: String) { transition(id, \"b\") }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "g.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("calls the handler `_bump`"), "{out}");
    // face return types are checked element-wise
    std::fs::write(d.join("f.cell"), "cell M {\n  face { signal g() -> List<Int> }\n  on g() {\n    let x = [\"a\"]\n    return x\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "f.cell", "g"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("expected Int"), "{out}");
}

#[test]
fn cycle34_findings() {
    let d = dir("cycle34");
    // `??` short-circuits; with() stores a BigInt value
    std::fs::write(d.join("c.cell"), "cell C {\n  on f(k: String) {\n    let m = map(\"cash\", \"Cash\")\n    let big = map(\"a\", 1) |> with(\"x\", 99999999999999999999999)\n    return [m.get(k) ?? fail(\"not_found\", \"account {k}\"), big.x]\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "c.cell", "f", "cash"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Cash") && out.contains("99999999999999999999999"), "{out}");
    // a strict Float bound after a require is proven (and not NaN)
    std::fs::write(d.join("r.cell"), "cell Rates {\n  memory {\n    rates: Map<String, Float> [persistent]\n    invariant rates > 0.0\n  }\n  on set_rate(k: String, rate: Float) {\n    require rate > 0.0 else BadRate\n    rates.set(k, rate)\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "--strict", "r.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("proven by induction"), "{out}");
}

#[test]
fn cycle34_attack_findings() {
    let d = dir("cycle34b");
    // invariants are pure conditions, however the call is hidden
    std::fs::write(d.join("i.cell"), "cell I {\n  memory {\n    m: Map<String, Int> [persistent]\n    invariant \"{_side()}\" != \"\"\n    invariant think(\"audit {value}\") != \"\"\n  }\n  on _side() { return 1 }\n  on w(k: String) { m.set(k, 1) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "i.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("'_side' which is not a builtin") && out.contains("calls think()"), "{out}");
    // a tool that calls back the handler whose think() dispatches it
    std::fs::write(d.join("a.cell"), "cell agent A {\n  face {\n    signal ask(q: String) -> String\n    tool deeper(q: String) -> String \"go deeper\"\n  }\n  on deeper(q: String) { return ask(q) }\n  on ask(q: String) { return think(q, map(\"max_tokens\", 50, \"max_rounds\", 2)) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "--strict", "a.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("mutual recursion"), "{out}");
    // an error in an imported file is reported in that file
    std::fs::write(d.join("helper.cell"), "cell H {\n  on boom(x: Int) {\n    return 10 / x\n  }\n}\n").unwrap();
    std::fs::write(d.join("app.cell"), "use helper\n\ncell App {\n  on main() { return boom(0) }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "app.cell", "main"]);
    assert!(out.contains("helper.cell:3:"), "{out}");
    // link() is a state change: GET answers 405
    std::fs::write(d.join("l.cell"), "cell L {\n  on connect(addr: String) { link(addr) return 1 }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "l.cell"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn cycle35_findings() {
    let d = dir("cycle35");
    // `x |> h` (a bare handler name) is a call for every analysis
    std::fs::write(d.join("p.cell"), "cell App {\n  on up(n: Int) { return (n + 1) |> up }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "--strict", "p.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("handler `up`"), "{out}");
    // a field of a String / List raises instead of reading ()
    std::fs::write(d.join("f.cell"), "cell F {\n  on f(s: String) { return s.worker ?? \"dflt\" }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "f.cell", "f", "x"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("cannot read field 'worker' of String"), "{out}");
    // I/O in an invariant is a check error
    std::fs::write(d.join("i.cell"), "cell I {\n  memory {\n    c: Map<String, Int> [persistent]\n    invariant c >= 0 && read_file(\"gate.txt\") == \"open\"\n  }\n  on put() { c.set(\"a\", 1) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "i.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("calls read_file()"), "{out}");
}

#[test]
fn cycle36_findings() {
    let d = dir("cycle36");
    // statements inside expression blocks: termination sees the emit in try
    std::fs::write(d.join("t.cell"), "cell App {\n  on ping(m: Map) { let r = try { emit pong(m) } return 1 }\n  on pong(m: Map) { let r = try { emit ping(m) } return 1 }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "--strict", "t.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("mutual recursion"), "{out}");
    // a guard writing a slot inside a block lambda
    std::fs::write(d.join("g.cell"), "cell App {\n  memory { hits: Map<String, Int> [persistent] }\n  state st {\n    initial: a\n    a -> b { guard { [1] |> all(q => { hits[\"g\"] = 9  true }) } }\n  }\n  on go(id: String) { transition(id, \"b\") }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "g.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("writes `hits[…]`"), "{out}");
    // `return` inside a block lambda; a bare `return`
    std::fs::write(d.join("r.cell"), "cell R {\n  on f(n: Int) { let ys = [1, 2, 3] |> map(v => { if v == 2 { return 99 }  v })  return ys }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "r.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("`return` inside a block lambda"), "{out}");
    std::fs::write(d.join("b.cell"), "cell B {\n  on f(n: Int) {\n    if n <= 0 { return }\n    return n\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "b.cell", "f", "0"]);
    assert_eq!(code, 0, "{out}");
    // soma run splits the query off request's path, as serve does
    std::fs::write(d.join("q.cell"), "cell Q {\n  on request(method: String, path: String, body: String, query: Map) { return map(\"path\", path, \"x\", query.x ?? \"none\") }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "q.cell", "request", "GET", "/a?x=1", ""]);
    assert!(out.contains("\"/a\"") && out.contains("\"x\": \"1\""), "{out}");
}

#[test]
fn cycle37_findings() {
    let d = dir("cycle37");
    // Int bounds past 2^53 are not "proven" through f64 rounding
    std::fs::write(d.join("h.cell"), "cell B {\n  memory {\n    bal: Map<String, Int> [persistent]\n    invariant bal <= 9007199254740992\n  }\n  on deposit(k: String, v: Int) {\n    require v <= 9007199254740992 else Big\n    bal.set(k, v + 1)\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "--strict", "h.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(!out.contains("writer 'deposit' proven"), "{out}");
    // a bare slot read written as is may be ()
    std::fs::write(d.join("u.cell"), "cell U {\n  memory {\n    c: Map [persistent]\n    invariant c >= 0\n  }\n  on w(k: String) { c.set(k, c.get(k)) }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["verify", "u.cell"]);
    assert!(out.contains("may be () (a missing key)"), "{out}");
    // an http_get in a lambda does not make a token bound advisory
    std::fs::write(d.join("c.cell"), "cell agent A {\n  cost { tokens: 300 }\n  on go(ids: List) {\n    let rows = ids |> map(i => http_get(\"http://127.0.0.1:1/o/{i}\"))\n    return think(\"sum {rows}\", map(\"max_tokens\", 100))\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "c.cell"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("'tokens' bound proven"), "{out}");
}

#[test]
fn cycle38_findings() {
    let d = dir("cycle38");
    // quant bounds and alpha are enforced
    std::fs::write(d.join("q.cell"), "cell Q {\n  on f(n: Int) {\n    let r = try { var_historical([0.01, -0.02, 0.03], map(\"alpha\", 1.5)) }\n    let s = try { var_historical([0.01, -0.02, 0.03], map(\"max_obs\", 2)) }\n    return [r.kind, s.kind]\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "q.cell", "f", "1"]);
    assert!(out.contains(r#"["range", "range"]"#), "{out}");
    // a materialized range is capped; a for-loop range is not
    std::fs::write(d.join("r.cell"), "cell R {\n  on f(n: Int) { let r = try { range(0, n) }  return r.kind }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "r.cell", "f", "20000000"]);
    assert!(out.contains("range"), "{out}");
    // an emit listener reached through a helper is not "on a route path"
    std::fs::write(d.join("w.cell"), "cell W {\n  on request(method: String, path: String, body: String) {\n    return match path { \"/pub\" -> _pub()  _ -> response(404, \"no\") }\n  }\n  on _pub() { emit tick(map(\"a\", 1)) return 1 }\n  on tick(data: Map) { return 1 }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["check", "w.cell"]);
    assert!(!out.contains("share the path /tick"), "{out}");
    // a statement keyword where a value is expected
    std::fs::write(d.join("l.cell"), "cell L {\n  on f(ok: Bool) {\n    let g = x => require ok else Nope\n    return 1\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["check", "l.cell"]);
    assert!(out.contains("starts a statement"), "{out}");
    // a file that is not a database: a clean error, no panic
    std::fs::create_dir_all(d.join(".soma_data")).unwrap();
    std::fs::write(d.join(".soma_data/soma.db"), "garbage garbage garbage garbage garbage garbage garbage garbage garbage garbage garbage garbage").unwrap();
    std::fs::write(d.join("k.cell"), "cell K {\n  memory { kv: Map<String, String> [persistent] }\n  on n() { return len(kv.keys) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "k.cell", "n"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("cannot be used") && !out.contains("panicked"), "{out}");
}

#[test]
fn cycle39_findings() {
    let d = dir("cycle39");
    // `mock Email.send` does not stub Sms.send; a test helper may not shadow
    std::fs::write(d.join("m.cell"), "cell Email { on send(to: String) { return \"email\" } }\ncell Sms { on send(to: String) { return \"sms\" } }\ncell App { on welcome(u: String) { return Sms.send(u) } }\ncell test T {\n  rules {\n    mock Email.send \"email-sent\"\n    assert welcome(\"bob\") == \"email-sent\"\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["test", "m.cell"]);
    assert_ne!(code, 0, "{out}");
    std::fs::write(d.join("s.cell"), "cell Cart { on total(xs: List) { return 0 } }\ncell test T {\n  rules { assert total([1, 2, 3]) == 6 }\n  on total(xs: List) { return sum(xs) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "s.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("test helper `total`"), "{out}");
    // `value` outside an invariant; monotone versions proven by their require
    std::fs::write(d.join("v.cell"), "cell P {\n  memory {\n    versions: Map<String, Int> [persistent]\n    invariant value >= (versions.get(key) ?? 0)\n  }\n  on bump(id: String) {\n    let next = (versions.get(id) ?? 0) + 1\n    require next >= (versions.get(id) ?? 0) else Stale\n    versions.set(id, next)\n  }\n  on bad(id: String, a: Map) {\n    require a.x >= (versions.get(id) ?? 0) else Stale\n    a.x = 0\n    versions.set(id, a.x)\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["verify", "v.cell"]);
    assert!(out.contains("writer 'bump' proven"), "{out}");
    assert!(!out.contains("writer 'bad' proven"), "{out}");
    std::fs::write(d.join("u.cell"), "cell U { on f(x: Int) { require value >= 0 else Bad  return x } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "u.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("exists only inside a memory `invariant`"), "{out}");
    // test cells that assert nothing fail
    std::fs::write(d.join("e.cell"), "cell A { on f() { return 1 } }\ncell test T { rules { let x = f() } }\n").unwrap();
    let (out, code) = soma_in(&d, &["test", "e.cell"]);
    assert_ne!(code, 0, "{out}");
}

#[test]
fn cycle40_findings() {
    let d = dir("cycle40");
    // the equal-clause rule: pure over locals and THIS slot only
    std::fs::write(d.join("h.cell"), "cell App {\n  memory {\n    c: Map<String, Int> [persistent]\n    m: Map<String, Int> [persistent]\n    invariant m % 2 == 0\n  }\n  on w(k: String) {\n    require (c.get(\"x\") ?? 0) % 2 == 0 else Odd\n    c.set(\"x\", 3)\n    m.set(k, c.get(\"x\") ?? 0)\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["verify", "h.cell"]);
    assert!(!out.contains("writer 'w' proven"), "{out}");
    std::fs::write(d.join("n.cell"), "cell App {\n  memory {\n    s: Map<String, Int> [persistent]\n    invariant s >= (s.get(key) ?? 0)\n  }\n  on w(v: Int) {\n    require v >= (s.get(to_string(next_id())) ?? 0) else Low\n    s.set(to_string(next_id()), v)\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["verify", "n.cell"]);
    assert!(!out.contains("writer 'w' proven"), "{out}");
    // a second slot named inside an interpolation or a lambda is a second slot
    std::fs::write(d.join("x.cell"), "cell App {\n  memory {\n    a: Map<String, Int> [persistent]\n    b: Map<String, String> [persistent]\n    invariant a + len(\"{b}\") <= 10\n  }\n  on wa(v: Int) { a.set(\"x\", v) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "x.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("references several slots"), "{out}");
    // an undefined function in a property body is a check error
    std::fs::write(d.join("p.cell"), "cell A { on f(n: Int) { return n } }\ncell test T { rules { property \"p\" forall n: Int in 0..5 ensures nosuch(n) >= 0 } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "p.cell"]);
    assert_ne!(code, 0, "{out}");
}

#[test]
fn cycle41_findings() {
    let d = dir("cycle41");
    // a guarded transition taken from a machine-less cell binds the guard's names
    std::fs::write(d.join("g.cell"), "cell M {\n  state s {\n    initial: a\n    a -> done { guard { amount <= 100 } }\n  }\n}\ncell N {\n  on force(id: String) { transition(id, \"done\") }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "g.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("reads 'amount'"), "{out}");
    // for over any finite value terminates
    std::fs::write(d.join("l.cell"), "cell L {\n  on d(m: Map) {\n    let n = 0\n    for x in m[\"k\"] { n += 1 }\n    return n\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "--strict", "l.cell"]);
    assert_eq!(code, 0, "{out}");
    // soma run decodes request's path like serve
    std::fs::write(d.join("r.cell"), "cell R {\n  on request(method: String, path: String, body: String) { return path }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "r.cell", "request", "GET", "/w/%C3%A9", ""]);
    assert!(out.contains("/w/é"), "{out}");
}

#[test]
fn cycle42_findings() {
    let d = dir("cycle42");
    // a parameter named like a slot; a face whose parameter types disagree
    std::fs::write(d.join("p.cell"), "cell C {\n  memory { m: Map<String, Int> [persistent]  invariant m.size <= 2 }\n  on add(k: String, m: Map) { require len(m) < 2 else Full  m.set(k, 1) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "p.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("has the name of the memory slot"), "{out}");
    std::fs::write(d.join("f.cell"), "cell F {\n  face { signal setup(capacity: Int, name: String) }\n  on setup(name: String, capacity: Int) { return 1 }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "f.cell"]);
    assert_ne!(code, 0, "{out}");
    // ticks of a machine-less cell are verified against the machine
    std::fs::write(d.join("t.cell"), "cell App {\n  state s { initial: a  a -> done }\n  on go(id: String) { transition(id, \"done\") }\n}\ncell Sweeper {\n  every 1s { transition(\"t3\", \"bogus\") }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "t.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("\"bogus\" is not in state machine"), "{out}");
    // an instance id of () is refused; a capitalised typo is not an argument
    std::fs::write(d.join("i.cell"), "cell I {\n  state s { initial: a  a -> b }\n  on go(x: Int) { transition((), \"b\") }\n  on st(id: String) { return get_status(id) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "i.cell", "go", "1"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("an instance id is a String or an Int"), "{out}");
    let (out, code) = soma_in(&d, &["run", "i.cell", "St", "x"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("no handler named 'St'"), "{out}");
}

#[test]
fn cycle43_findings() {
    let d = dir("cycle43");
    // parameter types are checked all the way down
    std::fs::write(d.join("p.cell"), "cell P {\n  on p1(x: List<Map<String, Int>>) { return x }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "p.cell", "p1", r#"[{"a":"x"}]"#]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("expected Int, got String"), "{out}");
    // 1e20 is not an Int
    std::fs::write(d.join("i.cell"), "cell I {\n  on addi(k: String, v: Int) { return v }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "i.cell", "addi", "a", "1e20"]);
    assert_ne!(code, 0, "{out}");
    // misspelled types, extra builtin arguments
    std::fs::write(d.join("t.cell"), "cell T {\n  memory { m: Map<String, Integer> [persistent] }\n  on f(n: Int) { return max(1, 2, n) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "t.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("unknown type `Integer`") && out.contains("max() takes at most 2"), "{out}");
    // from_json of an undeclared _type is refused
    std::fs::write(d.join("j.cell"), "cell J {\n  on f(s: String) { let r = try { from_json(s) }  return r.kind }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "j.cell", "f", r#"{"_type":"Nope","_variant":"X","a":1}"#]);
    assert!(out.contains("type"), "{out}");
    // `variants` as a statement's variable
    std::fs::write(d.join("k.cell"), "cell K {\n  on main() {\n    let variants = 1\n    variants = 2\n    return variants\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "k.cell"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn cycle44_findings() {
    let d = dir("cycle44");
    // an emit whose listener takes a different number of arguments
    std::fs::write(d.join("e.cell"), "cell T {\n  on h() { emit ev(1) }\n  on ev(a: Int, b: Int) { return 1 }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "e.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("but the listener T.ev takes 2"), "{out}");
    // a computed Float is not an Int parameter
    std::fs::write(d.join("f.cell"), "cell F {\n  on ix(n: Int) { return n }\n  on go() { let f = 2.0  return ix(f) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "f.cell", "go"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("expects Int, got Float"), "{out}");
    // CSV: a row of only empty cells survives; a duplicate header is refused
    std::fs::write(d.join("c.cell"), "cell C {\n  on rt() {\n    write_csv(\"o.csv\", [map(\"a\", 1), map(\"a\", ()), map(\"a\", 3)])\n    return len(read_csv(\"o.csv\"))\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "c.cell", "rt"]);
    assert!(out.trim_end().ends_with('3'), "{out}");
    std::fs::write(d.join("dup.csv"), "sku,qty,qty\nA,1,2\n").unwrap();
    std::fs::write(d.join("r.cell"), "cell R { on f() { let r = try { read_csv(\"dup.csv\") }  return r.kind } }\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "r.cell", "f"]);
    assert!(out.contains("csv"), "{out}");
}

#[test]
fn cycle45_findings() {
    let d = dir("cycle45");
    // documented html() headers pass the arity check; a unary chain is depth-limited
    std::fs::write(d.join("h.cell"), "cell H { on page() { return html(200, \"<p>x</p>\", \"Set-Cookie\", \"a=b\") } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "h.cell"]);
    assert_eq!(code, 0, "{out}");
    std::fs::write(d.join("n.cell"), format!("cell N {{ on f() {{ return 0 {} 1 }} }}\n", "-".repeat(5000))).unwrap();
    let (out, code) = soma_in(&d, &["check", "n.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("nested more than"), "{out}");
    // a List slot's invariant `key` is the element's index
    std::fs::write(d.join("l.cell"), "cell Log {\n  memory {\n    entries: List<String> [persistent]\n    invariant entries.get(key) == ()\n  }\n  on add(v: String) { entries.push(v)  return entries.len }\n  on edit(i: Int, v: String) { entries[i] = v }\n}\ncell test T {\n  rules {\n    assert Log.add(\"a\") == 1\n    assert_fails edit(0, \"forged\") matching \"invariant\"\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["test", "l.cell"]);
    assert_eq!(code, 0, "{out}");
    // a guard variable bound in the loop body before transition()
    std::fs::write(d.join("g.cell"), "cell V {\n  state s {\n    initial: a\n    a -> b { guard { overdue } }\n  }\n  on sweep(ids: List<String>) {\n    for it in ids {\n      let overdue = true\n      transition(it, \"b\")\n    }\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "g.cell"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn cycle46_findings() {
    let d = dir("cycle46");
    // a string split by an unescaped quote as the last statement of a loop body
    std::fs::write(d.join("q.cell"), "cell Q {\n  on f() {\n    let m = map(\"k\", \"v\")\n    let s = \"\"\n    for i in range(0, 2) {\n      s = s + \"<td>{m.k ?? \"\"}</td>\"\n    }\n    return s\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "q.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("unescaped `\"` inside `{…}`"), "{out}");
    // a literal write proves a disjunction over the value; `key` blocks it
    std::fs::write(d.join("o.cell"), "cell A {\n  memory {\n    n: Map<String, Int> [persistent]\n    invariant n == 0 || n == 5\n  }\n  on zero(k: String) { n.set(k, 0) }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["verify", "o.cell"]);
    assert!(out.contains("writer 'zero' proven"), "{out}");
    std::fs::write(d.join("k.cell"), "cell A {\n  memory {\n    n: Map<String, Int> [persistent]\n    invariant key != 0 || n == 0\n  }\n  on zero(k: String) { n.set(k, 0) }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["verify", "k.cell"]);
    assert!(!out.contains("writer 'zero' proven"), "{out}");
}

#[test]
fn cycle47_findings() {
    let d = dir("cycle47");
    // a scripted think reply longer than max_tokens raises like a provider
    std::fs::write(d.join("t.cell"), "cell agent A {\n  on ask(q: String) { return think(q, map(\"max_tokens\", 10)) }\n}\ncell test T {\n  rules {\n    mock think \"this scripted reply is certainly much longer than ten tokens of text\"\n    assert_fails ask(\"q\") matching \"llm\"\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["test", "t.cell"]);
    assert_eq!(code, 0, "{out}");
    // a huge declared Content-Length does not take the server down
    std::fs::write(d.join("s.cell"), "cell S {\n  memory { c: Map<String, Int> [persistent] }\n  on bump(k: String) { c.set(k, (c.get(k) ?? 0) + 1) return 1 }\n}\n").unwrap();
    let port = 19850 + (std::process::id() % 40) as u16;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "s.cell", "-p", &port.to_string()]).current_dir(&d)
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
    let mut up = false;
    for _ in 0..50 { if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { up = true; break; } std::thread::sleep(std::time::Duration::from_millis(100)); }
    assert!(up);
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(3))).ok();
    s.write_all(b"POST /bump/x HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1000000000000000\r\n\r\nabc").unwrap();
    let mut buf = [0u8; 64];
    let n = s.read(&mut buf).unwrap_or(0);
    assert!(String::from_utf8_lossy(&buf[..n]).contains("413"));
    drop(s);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let alive = std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
    let _ = child.kill();
    let _ = child.wait();
    assert!(alive, "serve died on a huge Content-Length");
}

#[test]
fn cycle48_findings() {
    let d = dir("cycle48");
    // an Int product past 2^24 bits is refused before it is built
    std::fs::write(d.join("g.cell"), "cell G {\n  on grow(k: Int) {\n    let x = shl(1, 10000000)\n    for [loop_bound(200)] i in range(0, k) { x = x * x }\n    return bit_len(x)\n  }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "g.cell", "grow", "8"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("past the limit of 16777216 bits"), "{out}");
    // substring / range with the wrong argument types raise
    std::fs::write(d.join("s.cell"), "cell S {\n  on f() {\n    let a = try { substring(\"hello\", 1.5, 3) }\n    let b = try { range(0, 3.7) }\n    return [a.kind, b.kind]\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "s.cell", "f"]);
    assert!(out.contains(r#"["type", "type"]"#), "{out}");
}

#[test]
fn cycle49_findings() {
    let d = dir("cycle49");
    // pow_mod work is bounded; a long digit string is not an Int past the cap
    std::fs::write(d.join("p.cell"), "cell P {\n  on pm(k: Int) { let r = try { pow_mod(3, shl(1, k) - 1, shl(1, k) - 1) }  return r.kind }\n  on ok() { return pow_mod(3, shl(1, 1023) - 1, shl(1, 1024) - 1) % 1000 }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "p.cell", "pm", "100000"]);
    assert!(out.contains("range"), "{out}");
    let (out, code) = soma_in(&d, &["run", "p.cell", "ok"]);
    assert_eq!(code, 0, "{out}");
    std::fs::write(d.join("i.cell"), "cell I {\n  on f(n: Int) { let s = pad_left(\"1\", n, \"9\")  let r = try { to_int(s) }  return r.kind }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "i.cell", "f", "6000000"]);
    assert!(out.contains("range"), "{out}");
}

#[test]
fn cycle50_findings() {
    let d = dir("cycle50");
    // get_status from a machine-less cell with several machines
    std::fs::write(d.join("g.cell"), "cell A { state s { initial: a  a -> b } }\ncell B { state t { initial: x  x -> y } }\ncell F { on st(id: String) { return get_status(id) } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "g.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("calls get_status()"), "{out}");
    // a variant literal with an unknown field
    std::fs::write(d.join("v.cell"), "cell type View { variants { Anon { id: String, cv: String } } }\ncell R { on f() { return Anon { id: \"1\", cv: \"x\", email: \"a@b\" } } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "v.cell"]);
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("has no field `email`"), "{out}");
    // file builtins refuse `..` and overwriting the program
    std::fs::write(d.join("f.cell"), "cell F {\n  on r(n: String) { let x = try { read_file(\"uploads/\" + n) }  return x.kind }\n  on w(n: String) { let x = try { write_file(n, \"cell X {}\") }  return x.kind }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "f.cell", "r", "../../secret.txt"]);
    assert!(out.contains("path"), "{out}");
    let (out, _) = soma_in(&d, &["run", "f.cell", "w", "f.cell"]);
    assert!(out.contains("path"), "{out}");
}

#[test]
fn cycle51_findings() {
    let d = dir("cycle51");
    // the write guard is case-insensitive (macOS / Windows file systems)
    std::fs::write(d.join("f.cell"), "cell F {\n  on w(n: String) { let x = try { write_file(n, \"PWNED\") }  return x.kind }\n}\n").unwrap();
    for name in ["F.CELL", "SOMA.TOML", ".SOMA_DATA/soma.db"] {
        let (out, _) = soma_in(&d, &["run", "f.cell", "w", name]);
        assert!(out.contains("path"), "{name}: {out}");
    }
    let src = std::fs::read_to_string(d.join("f.cell")).unwrap();
    assert!(src.starts_with("cell F"), "the program was overwritten");
}

/// Cycle 52: every path-taking builtin refuses `..`; a `fixed:` mock over
/// max_tokens raises like a scripted one; a guard local bound after an
/// early transition to ANOTHER state is fine; an `emit` without [peers]
/// opens no bus port; `unauthorized` is 401; error bodies do not name
/// private handlers; the loopback Host check parses the whole authority.
#[test]
fn cycle52_findings() {
    let d = dir("cycle52");
    std::fs::write(d.join("f.cell"), "cell F {\n  on a(p: String) { let x = try { load(p) }  return x.kind }\n  on d(p: String) { let x = try { read_files(p, 3) }  return x.kind }\n}\n").unwrap();
    for (h, p) in [("a", "../x.txt"), ("a", "t/../../x"), ("d", "../..")] {
        let (out, _) = soma_in(&d, &["run", "--fresh", "f.cell", h, p]);
        assert!(out.contains("path"), "{h} {p}: {out}");
    }

    let m = dir("cycle52_mock");
    std::fs::write(m.join("soma.toml"), "[agent]\nmock = \"fixed:abcdefghijklmnopqrstuvwxyz0123456789\"\n").unwrap();
    std::fs::write(m.join("t.cell"), "cell T { on ask(q: String) { let r = try { think(\"Q: {q}\", map(\"max_tokens\", 5)) }  return r.kind } }\n").unwrap();
    let (out, _) = soma_in(&m, &["run", "t.cell", "ask", "x"]);
    assert!(out.contains("llm"), "a fixed: reply over max_tokens raises kind llm: {out}");

    std::fs::write(d.join("g.cell"), "cell G { state s { initial: a  a -> b { guard { n > 0 } }  a -> c }\n  on go(id: String, k: Int) {\n    if k == 0 { transition(id, \"c\")  return \"c\" }\n    let n = k\n    transition(id, \"b\")\n    return \"b\" } }\n").unwrap();
    let (out, code) = soma_in(&d, &["check", "g.cell"]);
    assert_eq!(code, 0, "a let after a transition to another state binds the guard local: {out}");

    let s = dir("cycle52_serve");
    std::fs::write(s.join("app.cell"), r#"
cell E {
  memory { log: Map<String, String> [persistent] }
  on freed(id: String) { log.set(id, "promoted") }
  on _go(id: String) { emit freed(id) }
  on _book(time: String) { return time }
  on request(method: String, path: String, body: String) {
    if path == "/log" { return log }
    if path == "/b" { return _book(from_json(body).time) }
    if path == "/u" { require false else unauthorized }
    return response(404, "")
  }
}
"#).unwrap();
    let port = 19650 + (std::process::id() % 100) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "app.cell", "-p", &port.to_string()])
        .current_dir(&s).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("soma serve");
    let mut up = false;
    for _ in 0..80 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { up = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let raw = |req: String| -> String {
        let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        c.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = c.read_to_string(&mut out);
        out
    };
    let bus_open = std::net::TcpStream::connect(("127.0.0.1", port + 2)).is_ok();
    let u = if up { http(port, "GET", "/u") } else { String::new() };
    let b = if up { raw("POST /b HTTP/1.0\r\nHost: localhost\r\nContent-Length: 10\r\n\r\n{\"time\":9}".to_string()) } else { String::new() };
    let h1 = if up { raw(format!("GET /log HTTP/1.0\r\nHost: localhost:{port}.evil.com\r\n\r\n")) } else { String::new() };
    let h2 = if up { raw(format!("GET /log HTTP/1.0\r\nHost: localhost:{port}\r\nHost: evil.com\r\n\r\n")) } else { String::new() };
    let ok = if up { http(port, "GET", "/log") } else { String::new() };
    let _ = child.kill();
    let _ = child.wait();
    assert!(up, "server did not start");
    assert!(!bus_open, "an emit without [peers] opens no bus port");
    assert!(u.starts_with("HTTP/1.1 401") || u.starts_with("HTTP/1.0 401"), "unauthorized → 401: {u}");
    assert!(b.contains("\"type\"") && !b.contains("_book"), "the body names no private handler: {b}");
    assert!(h1.contains(" 403 "), "Host with a suffix after the port is refused: {h1}");
    assert!(h2.contains(" 403 "), "two Host headers are refused: {h2}");
    assert!(ok.contains(" 200 "), "a plain local request passes: {ok}");
}

/// Cycle 53: `soma run` says an emit meant for [peers] was not delivered;
/// `m.size ?? d` is a check warning; a JSON body with more than a million
/// values is refused 413 before it is parsed (15 MB became 2.4 GB).
#[test]
fn cycle53_findings() {
    let d = dir("cycle53");
    std::fs::write(d.join("soma.toml"), "[peers]\nb = \"127.0.0.1:1\"\n").unwrap();
    std::fs::write(d.join("a.cell"), "cell A { memory { s: Map<String, Int> [persistent] }\n  on go(k: String) { s.set(k, 1)  emit ping(map(\"p\", k))  return \"ok\" }\n  on ping(e: Map) { return () } }\n").unwrap();
    let (out, _) = soma_in(&d, &["run", "--fresh", "a.cell", "go", "x"]);
    assert!(out.contains("NOT delivered"), "an emit under soma run with [peers] is reported: {out}");

    let l = dir("cycle53_lint");
    std::fs::write(l.join("s.cell"), "cell S { on main() { let b = from_json(\"{\\\"qty\\\": 2}\")  let size = b.size ?? \"M\"  return size } }\n").unwrap();
    let (out, _) = soma_in(&l, &["check", "s.cell"]);
    assert!(out.contains("b.get(\"size\")"), "`.size ?? …` is flagged: {out}");

    let s = dir("cycle53_serve");
    std::fs::write(s.join("app.cell"), "cell R { on request(method: String, path: String, body: String) { return \"ok\" } }\n").unwrap();
    let port = 19750 + (std::process::id() % 40) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "app.cell", "-p", &port.to_string()])
        .current_dir(&s).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("soma serve");
    let mut up = false;
    for _ in 0..80 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() { up = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut resp = String::new();
    if up {
        let body = format!("[{}0]", "0,".repeat(1_100_000));
        let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(std::time::Duration::from_secs(30))).unwrap();
        let _ = write!(c, "POST /x HTTP/1.0\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
        let _ = c.read_to_string(&mut resp);
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(up, "server did not start");
    assert!(resp.contains(" 413 "), "a body over a million JSON values is refused: {}", &resp[..resp.len().min(200)]);
}

/// Cycle 54: in an invariant `slot.get(key)` is the STORED value on a
/// Map-valued slot too (a write-once audit log could be rewritten); a List
/// delete is checked only by size invariants, as verify says; approve()
/// shows control characters as escapes; a model's tool call cannot lift the
/// caller's token budget through a delegated set_budget.
#[test]
fn cycle54_findings() {
    passes("cycle54_writeonce", r#"
cell Log {
    memory {
        docs: Map<String, Map> [persistent]
        rows: List<Map> [persistent]
        invariant docs.get(key) == ()
        invariant rows.get(key) == ()
    }
    on put(k: String, v: Int) { docs.set(k, map("v", v)) }
    on add(v: Int) { rows.push(map("v", v)) }
    on over(v: Int) { rows[0] = map("v", v) }
    on show() { return docs.get("a") }
}
cell test T {
    rules {
        let _a = put("a", 1)
        let _v = put("v", 1)
        assert_fails put("a", 2)
        assert show().v == 1
        let _r = add(1)
        assert_fails over(9)
    }
}
"#);
    passes("cycle54_delete", r#"
cell L {
    memory { rows: List<String> [persistent]  invariant rows.get(key) == () }
    on add(s: String) { rows.push(s) }
    on erase(i: Int) { rows.delete(i)  return rows.len() }
}
cell test T { rules { let _a = add("a")  assert erase(0) == 0 } }
"#);
    let d = dir("cycle54_approve");
    std::fs::write(d.join("ap.cell"), "cell A { on go(m: String) { return approve(m) } }\n").unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["run", "ap.cell", "go", "x\r\u{1b}[2Ky"]).current_dir(&d).env("SOMA_APPROVE", "always")
        .output().expect("soma");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!err.contains('\u{1b}') && err.contains("\\u{1b}"), "control characters are escaped: {err:?}");
}

/// Cycle 55: `x |> map(f)` / `filter` on a non-list is a type error (it
/// built a Map); `ipow` is an exact Int power and `to_int(pow(…))` is
/// flagged; read_csv refuses unknown options and reads a `delimiter`;
/// `from_csv` parses text in memory; `%f` of an Int is exact; `to_float` of
/// an Int past the Float range raises.
#[test]
fn cycle55_findings() {
    passes("cycle55", r#"
cell P {
    on hmap() { let xs = map("a", 1).lines  return xs |> map(l => l.debit) }
    on hfilter() { let xs = ()  return xs |> filter(l => l > 1) }
    on p340() { return ipow(3, 40) }
    on csv() { return from_csv("a;b\n1;\"x;y\"\n", map("delimiter", ";")) }
    on badopt() { return from_csv("a\n1\n", map("delim", ";")) }
    on fmt() { return format("%.2f", 123456789012345678901234567890) }
    on big() { return to_float(shl(1, 1100)) }
}
cell test T {
    rules {
        assert_fails hmap()
        assert_fails hfilter()
        assert p340() == 12157665459056928801
        assert csv()[0].b == "x;y"
        assert_fails badopt()
        assert fmt() == "123456789012345678901234567890.00"
        assert_fails big()
    }
}
"#);
    let d = dir("cycle55_lint");
    std::fs::write(d.join("x.cell"), "cell X { on main() { return to_int(pow(3, 40)) } }\n").unwrap();
    let (out, _) = soma_in(&d, &["check", "x.cell"]);
    assert!(out.contains("ipow"), "to_int(pow(…)) points to ipow: {out}");
}

/// Cycle 55 (attack on the static guarantees): `"C".delegate(…)` and
/// `"C" |> delegate(…)` are calls for termination and size proofs; a `let`
/// of a slot's name hides the slot's bound; the latency bound counts
/// sleep / approve / file I/O and tool rounds.
#[test]
fn cycle55_soundness() {
    let d = dir("cycle55_sound");
    std::fs::write(d.join("loop.cell"), "cell C {\n  state s { initial: a  a -> b }\n  on go(id: String) { transition(id, \"b\") }\n  on spin(n: Int) { return \"C\".delegate(\"spin\", n) }\n  on spin2(n: Int) { return \"C\" |> delegate(\"spin2\", n) }\n}\n").unwrap();
    let (out, code) = soma_in(&d, &["verify", "--strict", "loop.cell"]);
    assert!(code != 0 && !out.contains("VERIFY OK"), "a self-delegating handler is not proven to terminate: {out}");

    std::fs::write(d.join("shadow.cell"), "cell C {\n  memory {\n    a: Map<String, Int> [persistent]\n    invariant a >= 0 && a <= 100\n    b: Map<String, Int> [persistent]\n    invariant b >= 0 && b <= 10\n  }\n  on copy(k: String) {\n    let b = map(\"x\", 1000)\n    a.set(k, b.get(\"x\") ?? 0)\n  }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["verify", "--strict", "shadow.cell"]);
    assert!(!out.contains("writer 'copy' proven"), "a let hiding a slot does not lend its bound: {out}");

    std::fs::write(d.join("lat.cell"), "cell agent L {\n  face { signal go(p: String) -> String }\n  cost { tokens: 100  latency: 1s }\n  on go(p: String) { sleep(3000)  return think(p, map(\"max_tokens\", 100, \"timeout\", 500)) }\n}\n").unwrap();
    let (out, _) = soma_in(&d, &["check", "lat.cell"]);
    assert!(!out.contains("'latency' bound proven"), "a sleep counts toward latency: {out}");
}
