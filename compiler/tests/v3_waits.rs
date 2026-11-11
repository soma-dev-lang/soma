// 3.0 — C3: durable waits. `await approve(…)`, `await signal(name, key)` and
// `await sleep(…)` in a `[task]` handler end the step when nobody can answer
// now: the call returns a 202 `waiting` response, the handler's position and
// locals are stored with the data, and the rest runs when the answer
// arrives — after a restart of `soma serve` too.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

fn soma_in(dir: &std::path::Path, args: &[&str]) -> (String, String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(dir)
        .env_remove("SOMA_EDITION").env_remove("SOMA_APPROVE").env_remove("SOMA_WAIT_TOKEN")
        .output().expect("failed to run soma");
    (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.code().unwrap_or(-1))
}

fn project(name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_v3w_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), source).unwrap();
    dir
}

const ORDERS: &str = r#"cell Orders {
    face {
        signal refund(id: String, amount: Int) -> Map
        signal ship(id: String) -> String
        signal nap(id: String) -> String
        signal seen() -> List
        signal two(id: String) -> String
        signal captured(n: Int) -> Int
    }

    memory {
        paid: Map<String, Int> [persistent]
        log: List<String> [persistent]
    }

    on refund(id: String, amount: Int) [task] {
        log.append("asked {id}")
        let note = "outer"
        if 100 < amount {
            let note = "inner"
            let ok = await approve("refund {amount} for {id}")
            if !ok {
                log.append("refused {id} {note}")
                return response(403, map("refused", id))
            }
            log.append("approved {id} {note}")
        }
        paid.set(id, amount)
        return map("paid", amount, "note", note)
    }

    on ship(id: String) [task] {
        let carrier = await signal("picked_up", id)
        log.append("shipped {id} by {carrier.name}")
        return "done"
    }

    on nap(id: String) [task] {
        log.append("nap {id}")
        await sleep(300ms)
        log.append("woke {id}")
        return "woke"
    }

    on two(id: String) [task] {
        let first = await approve("first {id}")
        log.append("first {first}")
        let second = await approve("second {id}")
        return "{first} {second}"
    }

    on captured(n: Int) [task] {
        let f = x => x + n
        let ok = await approve("go")
        return f(1)
    }

    on seen() { return log.values() }
}
"#;

#[test]
fn c3_a_wait_ends_the_step_and_resume_runs_the_rest() {
    let src = format!("{}{}", ORDERS, r#"
cell test Small {
    rules {
        let r = refund("a", 5)
        assert r.paid == 5
        assert len(waiting()) == 0
    }
}

cell test Approved {
    rules {
        let r = refund("b", 500)
        assert r._status == 202
        assert r.kind == "approve"
        assert r.message == "refund 500 for b"
        assert paid.get("b") == ()
        assert seen() == ["asked b"]
        let w = waiting()
        assert len(w) == 1
        assert w[0].handler == "refund"
        let done = resume(r.waiting, true)
        assert done.paid == 500
        assert done.note == "outer"
        assert paid.get("b") == 500
        assert seen() == ["asked b", "approved b inner"]
        assert len(waiting()) == 0
        assert_fails resume(r.waiting, true) matching "not_waiting"
    }
}

cell test Refused {
    rules {
        let r = refund("c", 500)
        let done = resume(r.waiting, false)
        assert done._status == 403
        assert paid.get("c") == ()
        assert_fails resume(r.waiting, "yes") matching "not_waiting"
    }
}

cell test WrongAnswer {
    rules {
        let r = refund("c", 500)
        assert_fails resume(r.waiting, "yes") matching "true or false"
        assert len(waiting()) == 1
    }
}

cell test Scripted {
    rules {
        mock approve true
        let r = refund("d", 500)
        assert r.paid == 500
    }
}

cell test Signal {
    rules {
        let r = ship("p1")
        assert r.kind == "signal"
        assert r.name == "picked_up"
        assert r.key == "p1"
        assert_fails deliver("picked_up", "other", map("name", "ups")) matching "not_waiting"
        assert deliver("picked_up", "p1", map("name", "ups")) == "done"
        assert seen() == ["shipped p1 by ups"]
    }
}

cell test TwoWaits {
    rules {
        let a = two("t")
        assert a.message == "first t"
        let b = resume(a.waiting, true)
        assert b._status == 202
        assert b.message == "second t"
        assert resume(b.waiting, false) == "true false"
    }
}

cell test Cancel {
    rules {
        let r = refund("e", 500)
        assert cancel_wait(r.waiting)
        assert !cancel_wait(r.waiting)
        assert_fails resume(r.waiting, true) matching "not_waiting"
        assert seen() == ["asked e"]
    }
}

cell test SleepInATest {
    rules {
        assert nap("n") == "woke"
    }
}

cell test Lambda {
    rules {
        assert_fails captured(1) matching "holds a lambda"
        mock approve true
        assert captured(1) == 2
    }
}
"#);
    let dir = project("flow", &src);
    let (out, err, code) = soma_in(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("0 failed"), "{out}");
}

#[test]
fn c3_check_refuses_a_wait_it_could_not_resume() {
    let dir = project("check", r#"cell Bad {
    face {
        signal a(n: Int) -> Int
        signal b(n: Int) -> Int
            ensures result >= 0
        signal c(xs: List<Int>) -> Int
    }

    on a(n: Int) {
        let ok = await approve("x")
        return n
    }

    on b(n: Int) [task] {
        await sleep(1s)
        return n
    }

    on c(xs: List<Int>) [task] {
        for x in xs {
            await sleep(1s)
        }
        let y = len(to_string(await approve("q")))
        return y
    }

    every 5s {
        await sleep(1s)
    }
}
"#);
    let (out, err, code) = soma_in(&dir, &["check", "app.cell"]);
    let all = format!("{out}{err}");
    assert_ne!(code, 0, "{all}");
    assert!(all.contains("which is not `[task]`"), "{all}");
    assert!(all.contains("its face states an `ensures`"), "{all}");
    assert_eq!(all.matches("here cannot be resumed").count(), 2, "{all}");
    assert!(all.contains("in a timer block"), "{all}");
    assert_eq!(all.matches("E0117").count(), 5, "{all}");
    let (out, _, _) = soma_in(&dir, &["explain", "E0117"]);
    assert!(out.contains("await"), "{out}");

    // `await` takes one of the three waits, nothing else
    std::fs::write(dir.join("other.cell"), "cell A {\n    face { signal w() -> Int }\n    on w() [task] {\n        let x = await foo(1)\n        return 1\n    }\n}\n").unwrap();
    let (out, err, code) = soma_in(&dir, &["check", "other.cell"]);
    assert_ne!(code, 0);
    assert!(format!("{out}{err}").contains("waits for one of three things"), "{out}{err}");
}

#[test]
fn c3_verify_sees_where_an_instance_rests() {
    let dir = project("verify", r#"cell Orders {
    face {
        signal pay(id: String) -> Map
        signal expire(id: String) -> String
        signal hold(id: String) -> Map
    }

    state order {
        initial: draft
        draft -> pending
        pending -> paid
        pending -> expired
        draft -> held
        held -> released
    }

    on pay(id: String) [task] {
        transition(id, "pending")
        let ok = await approve("pay {id}")
        if ok { transition(id, "paid") }
        return map("status", get_status(id))
    }

    on expire(id: String) {
        transition(id, "expired")
        return "expired"
    }

    on hold(id: String) [task] {
        transition(id, "held")
        let ok = await approve("release {id}")
        if ok { transition(id, "released") }
        return map("status", get_status(id))
    }
}
"#);
    let (out, err, code) = soma_in(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}{err}");
    // entering `pending` before the wait is the pattern, not a replay hazard
    assert!(!format!("{out}{err}").contains("BEFORE a think()"), "{out}{err}");
    let (out, err, _) = soma_in(&dir, &["verify", "app.cell"]);
    let all = format!("{out}{err}");
    assert!(all.contains("Orders/waits"), "{all}");
    assert!(all.contains("rests in `pending` of machine 'order'; besides the answer, expire() takes a transition out of it"), "{all}");
    assert!(all.contains("rests in `held` of machine 'order' until the answer comes, and nothing else leaves that state"), "{all}");
    let (out, _, _) = soma_in(&dir, &["describe", "app.cell"]);
    assert!(out.contains("\"waits\""), "{out}");
    assert!(out.contains("await approve"), "{out}");
}

// ── under `soma serve` ───────────────────────────────────────────────

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Server(std::process::Child);
impl Drop for Server {
    fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
}

fn serve(dir: &std::path::Path, port: u16, extra: &[&str], env: &[(&str, &str)]) -> Server {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soma"));
    cmd.args(["serve", "app.cell", "-p", &port.to_string()]).args(extra).current_dir(dir)
        .env_remove("SOMA_EDITION").env_remove("SOMA_APPROVE").env_remove("SOMA_WAIT_TOKEN")
        .stdout(Stdio::null()).stderr(Stdio::null());
    for (k, v) in env { cmd.env(k, v); }
    let server = Server(cmd.spawn().unwrap());
    for _ in 0..100 {
        // an HTTP answer, not only an open port: `soma serve` binds before its accept loop runs
        if http(port, "GET", "/seen", "", &[]).0 != 0 { return server; }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("soma serve did not start on {port}");
}

/// One request: (status, body).
fn http(port: u16, method: &str, path: &str, body: &str, headers: &[&str]) -> (u16, String) {
    let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) else { return (0, String::new()) };
    s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    if !body.is_empty() { req.push_str("Content-Type: application/json\r\n"); }
    for h in headers { req.push_str(h); req.push_str("\r\n"); }
    req.push_str("\r\n");
    req.push_str(body);
    if s.write_all(req.as_bytes()).is_err() { return (0, String::new()); }
    let mut raw = String::new();
    let _ = s.read_to_string(&mut raw);
    let status = raw.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, raw.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default())
}

fn wait_id(body: &str) -> String {
    let at = body.find("\"waiting\":\"").expect(body) + "\"waiting\":\"".len();
    body[at..].split('"').next().unwrap().to_string()
}

#[test]
fn c3_serve_answers_202_and_resumes_after_a_restart() {
    let dir = project("serve", ORDERS);
    let port = free_port();
    let server = serve(&dir, port, &[], &[]);

    let (status, body) = http(port, "POST", "/refund", r#"{"id":"r1","amount":500}"#, &[]);
    assert_eq!(status, 202, "{body}");
    assert!(body.contains("\"kind\":\"approve\"") && body.contains("refund 500 for r1"), "{body}");
    let approval = wait_id(&body);
    let (status, body) = http(port, "POST", "/ship", r#"{"id":"s1"}"#, &[]);
    assert_eq!(status, 202, "{body}");
    let (status, body) = http(port, "POST", "/nap", r#"{"id":"n1"}"#, &[]);
    assert_eq!(status, 202, "{body}");
    assert!(body.contains("until_ms"), "{body}");
    let (_, listed) = http(port, "GET", "/_approvals", "", &[]);
    assert!(listed.contains(&approval) && listed.contains("\"handler\":\"refund\""), "{listed}");

    // the process ends: the waits are data
    drop(server);
    let _server = serve(&dir, port, &[], &[]);

    // the sleep came due while the server was down, or just after
    let mut woke = false;
    for _ in 0..60 {
        let (_, seen) = http(port, "GET", "/seen", "", &[]);
        if seen.contains("woke n1") { woke = true; break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(woke, "the sleep did not resume after the restart");

    let (status, body) = http(port, "POST", &format!("/_approvals/{approval}"), r#"{"approved":"yes"}"#, &[]);
    assert_eq!(status, 400, "{body}");
    let (status, body) = http(port, "POST", &format!("/_approvals/{approval}"), r#"{"approved":true}"#, &[]);
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"paid\":500") && body.contains("\"note\":\"outer\""), "{body}");
    // answered once
    let (status, body) = http(port, "POST", &format!("/_approvals/{approval}"), r#"{"approved":true}"#, &[]);
    assert_eq!(status, 404, "{body}");
    assert!(body.contains("not_waiting"), "{body}");

    let (status, body) = http(port, "POST", "/_signals/picked_up/s1", r#"{"name":"ups"}"#, &[]);
    assert_eq!(status, 200, "{body}");
    let (_, seen) = http(port, "GET", "/seen", "", &[]);
    assert!(seen.contains("approved r1 inner") && seen.contains("shipped s1 by ups"), "{seen}");
    let (_, listed) = http(port, "GET", "/_approvals", "", &[]);
    assert_eq!(listed.trim(), "[]", "{listed}");

    // a refusal is the handler's own answer
    let (_, body) = http(port, "POST", "/refund", r#"{"id":"r2","amount":900}"#, &[]);
    let (status, body) = http(port, "POST", &format!("/_approvals/{}", wait_id(&body)), r#"{"approved":false}"#, &[]);
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("\"refused\":\"r2\""), "{body}");
}

#[test]
fn c3_the_wait_routes_answer_loopback_or_the_token_only() {
    let dir = project("guard", ORDERS);
    // bound beyond loopback, no token: whoever can post would approve
    let port = free_port();
    let server = serve(&dir, port, &["--host", "0.0.0.0"], &[]);
    let (_, body) = http(port, "POST", "/refund", r#"{"id":"r1","amount":500}"#, &[]);
    let id = wait_id(&body);
    let (status, body) = http(port, "GET", "/_approvals", "", &[]);
    assert_eq!(status, 403, "{body}");
    let (status, _) = http(port, "POST", &format!("/_approvals/{id}"), r#"{"approved":true}"#, &[]);
    assert_eq!(status, 403);
    drop(server);

    let port = free_port();
    let _server = serve(&dir, port, &["--host", "0.0.0.0"], &[("SOMA_WAIT_TOKEN", "s3cret")]);
    let (status, _) = http(port, "POST", &format!("/_approvals/{id}"), r#"{"approved":true}"#, &[]);
    assert_eq!(status, 401);
    let (status, _) = http(port, "POST", &format!("/_approvals/{id}"), r#"{"approved":true}"#, &["Authorization: Bearer s3creT"]);
    assert_eq!(status, 401);
    let (status, body) = http(port, "POST", &format!("/_approvals/{id}"), r#"{"approved":true}"#, &["Authorization: Bearer s3cret"]);
    assert_eq!(status, 200, "{body}");
}

#[test]
fn c3_a_wait_is_not_resumed_into_a_handler_that_changed() {
    let dir = project("stale", ORDERS);
    let port = free_port();
    let server = serve(&dir, port, &[], &[]);
    let (_, body) = http(port, "POST", "/refund", r#"{"id":"r1","amount":500}"#, &[]);
    let id = wait_id(&body);
    drop(server);

    // an edit ELSEWHERE in the file moves every position: the wait still resumes
    std::fs::write(dir.join("app.cell"), ORDERS.replace("    on seen() { return log.values() }", "    on seen() {\n        let all = log.values()\n        return all\n    }").replace("cell Orders {", "// orders\n\ncell Orders {")).unwrap();
    let server = serve(&dir, port, &[], &[]);
    let (_, listed) = http(port, "GET", "/_approvals", "", &[]);
    assert!(listed.contains(&id), "{listed}");
    drop(server);

    // an edit of the handler itself: its stored position describes another program
    std::fs::write(dir.join("app.cell"), ORDERS.replace("log.append(\"asked {id}\")", "log.append(\"asked {id}\")\n        log.append(\"again\")")).unwrap();
    let _server = serve(&dir, port, &[], &[]);
    let (status, body) = http(port, "POST", &format!("/_approvals/{id}"), r#"{"approved":true}"#, &[]);
    assert_eq!(status, 409, "{body}");
    assert!(body.contains("wait_stale"), "{body}");
    let (_, seen) = http(port, "GET", "/seen", "", &[]);
    assert!(!seen.contains("approved r1"), "{seen}");
}

// found while writing the tests above: a constant before a block's brace
#[test]
fn a_constant_before_a_block_is_still_the_constant() {
    let dir = project("const_brace", r#"cell A {
    const LIMIT = 100
    face { signal big(n: Int) -> String }
    on big(n: Int) {
        if n > LIMIT {
            return "big"
        }
        let k = match LIMIT {
            100 -> "hundred"
            _ -> "other"
        }
        while n > LIMIT { return k }
        return "small {k}"
    }
}

cell test T {
    rules {
        assert big(500) == "big"
        assert big(5) == "small hundred"
    }
}
"#);
    let (out, err, code) = soma_in(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}{err}");
    let (out, err, code) = soma_in(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}{err}");
}
