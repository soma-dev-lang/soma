//! `soma serve`: what runs beside a long request.

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("soma_beside_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// GET `path`: (seconds it took, body).
fn get(port: u16, path: &str) -> (f64, String) {
    let t = Instant::now();
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    (t.elapsed().as_secs_f64(), out)
}

struct Server { child: Child, port: u16, log: std::path::PathBuf }
impl Drop for Server { fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); } }

fn serve(d: &std::path::Path) -> Server {
    let port = free_port();
    let log = d.join("serve.log");
    let child = Command::new(env!("CARGO_BIN_EXE_soma")).args(["serve", "app.cell", "-p", &port.to_string()]).current_dir(d)
        .stdout(Stdio::null()).stderr(std::fs::File::create(&log).unwrap()).spawn().expect("failed to run soma serve");
    let server = Server { child, port, log };
    // (the first start of a program with a [native] handler compiles it)
    let until = Instant::now() + Duration::from_secs(180);
    loop {
        if let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let _ = write!(s, "GET /fixe HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            if out.contains("200") { break; }
        }
        assert!(Instant::now() < until, "soma serve did not answer: {}", std::fs::read_to_string(&server.log).unwrap_or_default());
        std::thread::sleep(Duration::from_millis(100));
    }
    server
}

fn program(slots: &str, native: &str) -> String {
    format!(r#"
cell Api {{
    memory {{
{slots}
    }}
{native}
    on calcul(ms: Int) {{
        let t0 = now_ms()
        let x = 0
        while now_ms() - t0 < ms {{ x = x + 1 }}
        return x
    }}
    on request(method: String, path: String, body: String) {{
        if path == "/lent" {{ return map("fait", calcul(2500)) }}
        if path == "/lit" {{ return map("n", vus.get("n") ?? 0) }}
        if path == "/ecrit" {{ vus.set("n", (vus.get("n") ?? 0) + 1)  return map("n", vus.get("n")) }}
        if path == "/fixe" {{ return map("ok", true) }}
        return map("_status", 404)
    }}
}}
"#)
}

/// While `/lent` computes, how long the next requests take: (a route that reads
/// nothing, one that reads a slot, one that writes).
fn during_a_long_request(server: &Server) -> (f64, f64, f64) {
    let port = server.port;
    let slow = std::thread::spawn(move || get(port, "/lent"));
    std::thread::sleep(Duration::from_millis(400));
    let (fixe, lit, ecrit) = (get(port, "/fixe").0, get(port, "/lit").0, get(port, "/ecrit").0);
    let (took, body) = slow.join().unwrap();
    assert!(body.contains("fait") && took >= 2.4, "the long request ran: {took} s, {body}");
    (fixe, lit, ecrit)
}

/// One `[native]` handler in the program — never called here — and every
/// request waited for the one before it: a read-only request ran beside the
/// others only in a program with no compiled handler, because a library of
/// native handlers is entered by one caller at a time and the handler lock
/// saw to that. Found by a rewrite of a Python application: a computation of
/// 20 to 50 seconds held the whole server. Native calls have their own lock.
#[test]
fn a_native_handler_in_the_program_does_not_serialize_the_server() {
    let d = dir("native");
    std::fs::write(d.join("app.cell"), program(
        "        vus: Map<String, Int> [persistent]",
        "    on carre(n: Int) [native] { return n * n }")).unwrap();
    let server = serve(&d);
    let (fixe, lit, ecrit) = during_a_long_request(&server);
    assert!(fixe < 1.0 && lit < 1.0 && ecrit < 1.0, "beside a long read-only request: /fixe {fixe:.2} s, /lit {lit:.2} s, /ecrit {ecrit:.2} s");
}

/// A slot kept in memory has no snapshot: the server takes one request at a
/// time, as before — and now says so, once, naming the slot.
#[test]
fn a_slot_without_persistent_serializes_the_server_and_serve_says_so() {
    let d = dir("memory");
    std::fs::write(d.join("app.cell"), program("        vus: Map<String, Int>", "")).unwrap();
    let server = serve(&d);
    let log = std::fs::read_to_string(&server.log).unwrap_or_default();
    assert!(log.contains("[serve] one request at a time: `Api.vus` is not `[persistent]`"), "{log}");
    assert_eq!(log.matches("one request at a time").count(), 1, "{log}");

    // every slot `[persistent]`: nothing to say, and the others are not held
    let d = dir("persistent");
    std::fs::write(d.join("app.cell"), program("        vus: Map<String, Int> [persistent]", "")).unwrap();
    let server = serve(&d);
    let (fixe, lit, ecrit) = during_a_long_request(&server);
    assert!(fixe < 1.0 && lit < 1.0 && ecrit < 1.0, "/fixe {fixe:.2} s, /lit {lit:.2} s, /ecrit {ecrit:.2} s");
    assert!(!std::fs::read_to_string(&server.log).unwrap_or_default().contains("one request at a time"));
}

/// A computation started in the background — a horde whose target only reads
/// — held the handler lock from its first line to its last: every request
/// that writes waited for it. It runs on a snapshot of the last commit; its
/// result is recorded, and `on_result` called, in a unit of their own.
#[test]
fn a_computation_in_the_background_does_not_hold_the_requests_that_write() {
    let d = dir("horde");
    std::fs::write(d.join("app.cell"), r#"
cell Api {
    memory {
        resultats: Map<String, Int> [persistent]
        vus: Map<String, Int> [persistent]
    }
    on calcul(cle: String) {
        let t0 = now_ms()
        let x = 0
        while now_ms() - t0 < 2500 { x = x + 1 }
        return (vus.get("n") ?? 0) + 1000
    }
    on _range(input: String, result: Int) { resultats.set(input, result) }
    on request(method: String, path: String, body: String) {
        if path == "/lance" {
            let h = horde(Api.calcul, ["a"], map("concurrency", 1, "on_result", "_range"))
            return map("horde", h)
        }
        if path == "/resultat" { return map("a", resultats.get("a") ?? 0) }
        if path == "/ecrit" { vus.set("n", (vus.get("n") ?? 0) + 1)  return map("n", vus.get("n")) }
        if path == "/fixe" { return map("ok", true) }
        return map("_status", 404)
    }
}
"#).unwrap();
    let server = serve(&d);
    let (started, body) = get(server.port, "/lance");
    assert!(body.contains("Api:h1") && started < 1.0, "the horde answers at once: {started} s, {body}");
    std::thread::sleep(Duration::from_millis(400));
    let (ecrit, body) = get(server.port, "/ecrit");
    assert!(body.contains("\"n\":1"), "{body}");
    assert!(ecrit < 1.0, "a request that writes waited {ecrit:.2} s for the computation in the background");
    // the computation read the snapshot it started on (n was absent), and its result is recorded
    std::thread::sleep(Duration::from_millis(3200));
    let (_, body) = get(server.port, "/resultat");
    assert!(body.contains("\"a\":1000"), "{body}");
}
