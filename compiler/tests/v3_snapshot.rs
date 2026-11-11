// 3.0 — C8: concurrency beyond one global lock. Under `soma serve` a handler
// that only reads runs on a snapshot of the last commit, beside whatever
// holds the handler lock: readers run in parallel, never wait for a writer,
// and never see half of one. An invocation taken for a reader that turns out
// to write keeps nothing of that attempt and runs again under the lock.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn project(name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_v3s_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), source).unwrap();
    dir
}

struct Server { child: std::process::Child, log: std::path::PathBuf }
impl Drop for Server {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}
impl Server {
    fn log(&self) -> String { std::fs::read_to_string(&self.log).unwrap_or_default() }
}

fn serve(dir: &std::path::Path, env: &[(&str, &str)]) -> (Server, u16) {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let log = dir.join("serve.log");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soma"));
    cmd.args(["serve", "app.cell", "-p", &port.to_string()]).current_dir(dir)
        .env_remove("SOMA_EDITION").env_remove("SOMA_SERIAL")
        .stdout(Stdio::null()).stderr(std::fs::File::create(&log).unwrap());
    for (k, v) in env { cmd.env(k, v); }
    let server = Server { child: cmd.spawn().unwrap(), log };
    for _ in 0..200 {
        if http(port, "GET", "/ping", "").0 == 200 { return (server, port); }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("soma serve did not start: {}", server.log());
}

fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) else { return (0, String::new()) };
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    if !body.is_empty() { req.push_str("Content-Type: application/json\r\n"); }
    req.push_str("\r\n");
    req.push_str(body);
    if s.write_all(req.as_bytes()).is_err() { return (0, String::new()); }
    let mut raw = String::new();
    let _ = s.read_to_string(&mut raw);
    let status = raw.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, raw.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default())
}

const SHOP: &str = r#"cell Shop {
    face {
        signal ping() -> String
        signal slow_read(k: String) -> Map
        signal pair() -> List
        signal move_both(v: Int) -> String
        signal bump(k: String) -> Int
        signal peek(k: String) -> Int
        signal sneaky(k: String) -> Int
        signal state_of(id: String) -> String
        signal ship(id: String) -> String
    }

    memory {
        a: Map<String, Int> [persistent]
        b: Map<String, Int> [persistent]
        hits: Map<String, Int> [persistent]
    }

    state order {
        initial: open
        open -> shipped
    }

    on ping() { return "pong" }

    on slow_read(k: String) {
        let before = a.get(k) ?? 0
        sleep(400)
        return map("before", before, "after", a.get(k) ?? 0)
    }

    on pair() { return [a.get("x") ?? 0, b.get("x") ?? 0] }

    on move_both(v: Int) {
        a.set("x", v)
        sleep(400)
        b.set("x", v)
        return "moved"
    }

    on bump(k: String) {
        a.set(k, (a.get(k) ?? 0) + 1)
        return a.get(k)
    }

    on peek(k: String) { return a.get(k) ?? 0 }

    on state_of(id: String) { return get_status(id) }

    on ship(id: String) {
        transition(id, "shipped")
        return get_status(id)
    }

    on request(method: String, path: String, body: Map) {
        return match map("method", method, "path", path) {
            {method: "GET", path: "/items/" + k} -> peek(k)
            {method: "GET", path: "/counted/" + k} -> sneaky(k)
            {method: "POST", path: "/items/" + k} -> bump(k)
            _ -> response(404, map("error", "no route"))
        }
    }

    on sneaky(k: String) {
        hits.set(k, (hits.get(k) ?? 0) + 1)
        return hits.get(k)
    }
}
"#;

fn parallel(n: usize, port: u16, path: &'static str) -> (Duration, Vec<(u16, String)>) {
    let start = Instant::now();
    let handles: Vec<_> = (0..n).map(|_| std::thread::spawn(move || http(port, "GET", path, ""))).collect();
    let out = handles.into_iter().map(|h| h.join().unwrap()).collect();
    (start.elapsed(), out)
}

#[test]
fn c8_readers_run_in_parallel_and_never_see_half_a_write() {
    let dir = project("par", SHOP);
    let (server, port) = serve(&dir, &[]);

    // six readers of 400 ms each: together, not one after the other (2.4 s)
    let (took, out) = parallel(6, port, "/slow_read?k=q");
    assert!(out.iter().all(|(s, _)| *s == 200), "{out:?}");
    assert!(took < Duration::from_millis(1600), "6 readers of 400 ms took {took:?} — they ran one at a time");

    // a writer between its two writes: a reader does not wait for it, and
    // sees neither of them — then both
    let writer = std::thread::spawn(move || http(port, "POST", "/move_both", r#"{"v": 7}"#));
    std::thread::sleep(Duration::from_millis(150));
    let start = Instant::now();
    let (status, during) = http(port, "GET", "/pair", "");
    assert_eq!(status, 200);
    assert!(start.elapsed() < Duration::from_millis(250), "the reader waited for the writer: {:?}", start.elapsed());
    assert_eq!(during.trim(), "[0,0]", "a reader saw half of an uncommitted write");
    assert_eq!(writer.join().unwrap().0, 200);
    assert_eq!(http(port, "GET", "/pair", "").1.trim(), "[7,7]");

    // a reader keeps ONE snapshot: a write committed while it runs does not show in it
    let reader = std::thread::spawn(move || http(port, "GET", "/slow_read?k=r", ""));
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(http(port, "POST", "/items/r", "").0, 200);
    let (_, seen) = reader.join().unwrap();
    assert!(seen.contains("\"before\":0") && seen.contains("\"after\":0"), "{seen}");
    assert_eq!(http(port, "GET", "/items/r", "").1.trim(), "{\"result\": 1}");

    // a machine's state is read on the snapshot too
    assert!(http(port, "GET", "/state_of?id=o1", "").1.contains("open"));
    assert!(http(port, "POST", "/ship", r#"{"id": "o1"}"#).1.contains("shipped"));
    assert!(http(port, "GET", "/state_of?id=o1", "").1.contains("shipped"));
    drop(server);
}

#[test]
fn c8_an_invocation_taken_for_a_reader_that_writes_runs_again_as_a_writer() {
    let dir = project("fallback", SHOP);
    let (server, port) = serve(&dir, &[]);
    // GET routes through `request` are tried on a snapshot
    assert_eq!(http(port, "POST", "/items/q", "").1.trim(), "{\"result\": 1}");
    assert_eq!(http(port, "GET", "/items/q", "").1.trim(), "{\"result\": 1}");
    // this GET route writes: counted once per call, not twice, not zero times
    assert_eq!(http(port, "GET", "/counted/z", "").1.trim(), "{\"result\": 1}");
    assert_eq!(http(port, "GET", "/counted/z", "").1.trim(), "{\"result\": 2}");
    assert_eq!(http(port, "GET", "/counted/z", "").1.trim(), "{\"result\": 3}");
    // …and it is remembered by its route, once; the other GET route still reads beside
    let log = server.log();
    assert_eq!(log.matches("Shop.request (GET /counted) writes").count(), 1, "{log}");
    assert_eq!(http(port, "GET", "/items/q", "").1.trim(), "{\"result\": 1}");
    assert!(!server.log().contains("GET /items) writes"), "{}", server.log());
}

#[test]
fn c8_soma_serial_keeps_the_one_lock() {
    let dir = project("serial", SHOP);
    let (_server, port) = serve(&dir, &[("SOMA_SERIAL", "1")]);
    let (took, out) = parallel(4, port, "/slow_read?k=q");
    assert!(out.iter().all(|(s, _)| *s == 200), "{out:?}");
    assert!(took >= Duration::from_millis(1500), "SOMA_SERIAL=1: 4 readers of 400 ms took {took:?}");
}

#[test]
fn c8_a_slot_kept_in_memory_has_no_snapshot() {
    // `cache` is not persistent: a writer's uncommitted value would be read — the cell stays under the lock
    let src = SHOP.replace("        hits: Map<String, Int> [persistent]", "        hits: Map<String, Int> [persistent]\n        cache: Map<String, Int>");
    let dir = project("memory", &src);
    let (_server, port) = serve(&dir, &[]);
    let (took, out) = parallel(4, port, "/slow_read?k=q");
    assert!(out.iter().all(|(s, _)| *s == 200), "{out:?}");
    assert!(took >= Duration::from_millis(1500), "4 readers of 400 ms took {took:?} with an in-memory slot");
}
