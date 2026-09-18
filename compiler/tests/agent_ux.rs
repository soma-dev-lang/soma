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
            {method: "POST", path: "/book/" + slot} -> {
                let r = try { book(slot) }
                if r.error != () { return response(409, map("error", r.kind)) }
                r.value
            }
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
        assert zz_first.get("__next_id") == 2
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
    assert add(2.0) == 3
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
    assert_fails to_int(1e19) matching "outside the Int range"
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
    std::fs::write(d.join("main.cell"), "use helper\ncell M { on go() { let r = rows()  return \"{r[0].id}|{r[0].name}\" } }\n").unwrap();
    let (out, code) = soma_in(&d, &["run", "main.cell", "go"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("007|Smith, J"), "{out}");
}
