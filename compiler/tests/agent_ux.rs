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
            got.push(http(port, "GET", path));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(up, "server did not start");
    let status = |r: &String| r.lines().next().unwrap_or("").to_string();
    assert!(status(&got[0]).contains("500") && got[0].contains("stack_overflow"), "{}", got[0]);
    assert!(status(&got[1]).contains("422") && got[1].contains("\"kind\":\"invariant\""), "{}", got[1]);
    assert!(status(&got[2]).contains("409") && got[2].contains("invalid_transition"), "{}", got[2]);
    assert!(status(&got[3]).contains("404") && got[3].contains("\"kind\":\"not_found\""), "{}", got[3]);
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
    assert q.second == "mock"
    mock price_check [1, 2]
    assert price_check("a") == 1
    assert price_check("b") == 2
  }
}
"#).unwrap();
    let (out, code) = soma_in(&d, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}");
}
