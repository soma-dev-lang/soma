//! `soma serve`, tenth pass: HTTP framing and protocol edges, the accept
//! loop, loopback detection, WebSocket replies and logs, static files.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_pass10_serve_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Server(std::process::Child, PathBuf);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

/// `soma serve -p 0` (the OS picks a free port: fixed ports collided with
/// other test processes on the machine); the port is read from the log. The
/// WebSocket port is that port + 1, which a parallel server may hold: then
/// the server exits and is started again.
fn serve(dir: &Path, extra: &[&str]) -> (Server, u16) {
    for _attempt in 0..8 {
        let log = std::fs::File::create(dir.join("serve.log")).unwrap();
        let mut args = vec!["serve", "app.cell", "--no-schedule", "-p", "0"];
        args.extend_from_slice(extra);
        let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
            .args(&args)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline && child.try_wait().unwrap().is_none() {
            let text = std::fs::read_to_string(dir.join("serve.log")).unwrap_or_default();
            let port = text.lines().find_map(|l| l.strip_prefix("listening on http://"))
                .and_then(|rest| rest.rsplit(':').next()).and_then(|p| p.trim().parse::<u16>().ok());
            if let Some(port) = port.filter(|_| text.contains("\n---\n")) {
                // a moment for the WebSocket listener to bind (or fail to)
                std::thread::sleep(Duration::from_millis(150));
                let up = child.try_wait().unwrap().is_none()
                    && (extra.contains(&"--host") || raw(port, "GET /__soma/hordes HTTP/1.0\r\n\r\n", 1000).starts_with("HTTP/"));
                if up {
                    return (Server(child, dir.to_path_buf()), port);
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
        let text = std::fs::read_to_string(dir.join("serve.log")).unwrap_or_default();
        assert!(text.contains("cannot bind port"), "server did not start: {text}");
    }
    panic!("server did not start after 8 attempts");
}

/// Send raw bytes, read until EOF or `ms` of silence; `<TIMEOUT>` marks a
/// connection still open when reading stopped.
fn raw(port: u16, req: &str, ms: u64) -> String {
    let Ok(mut s) = std::net::TcpStream::connect(("127.0.0.1", port)) else { return String::new() };
    s.set_read_timeout(Some(Duration::from_millis(ms))).unwrap();
    let _ = s.write_all(req.as_bytes());
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(_) => {
                out.extend_from_slice(b"<TIMEOUT>");
                break;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn head_of(resp: &str) -> String {
    resp.split("\r\n\r\n").next().unwrap_or("").to_ascii_lowercase()
}

const APP: &str = r#"
cell App {
    memory { kv: Map<String, String> [persistent] }
    on hello(name: String) { return "hi {name}" }
    on stat(code: Int) { return response(code, "x") }
    on echo(data: Map) { return data }
    on stream() { return sse("s1") }
    on ws(message: String) {
        if message == "nan" { return map("n", 0.0 / 0.0, "q", "a\"b") }
        fail("bad", message)
    }
}
"#;

fn app(name: &str) -> PathBuf {
    let dir = scratch(name);
    std::fs::write(dir.join("app.cell"), APP).unwrap();
    dir
}

#[test]
fn a_client_that_resets_before_accept_does_not_end_the_server() {
    // a request followed at once by a RST: the connection had no peer
    // address by the time it was accepted, tiny_http unwrapped it, and the
    // panic hook ended `soma serve` (exit 70) — any client could stop it
    let dir = app("rst");
    let (mut server, port) = serve(&dir, &[]);
    for _ in 0..400 {
        let s = socket2_like_rst(port);
        drop(s);
    }
    std::thread::sleep(Duration::from_millis(300));
    assert!(server.0.try_wait().unwrap().is_none(), "the server exited: {}", std::fs::read_to_string(dir.join("serve.log")).unwrap_or_default());
    assert!(raw(port, "GET /hello/x HTTP/1.0\r\n\r\n", 3000).starts_with("HTTP/1.0 200"));
}

/// Connect, send a request, and close with SO_LINGER 0 (a RST).
fn socket2_like_rst(port: u16) -> std::net::TcpStream {
    let s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let _ = (&s).write_all(b"GET /hello/x HTTP/1.1\r\nHost: localhost\r\n\r\n");
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        #[repr(C)]
        struct Linger {
            l_onoff: i32,
            l_linger: i32,
        }
        extern "C" {
            fn setsockopt(fd: i32, level: i32, name: i32, val: *const std::ffi::c_void, len: u32) -> i32;
        }
        #[cfg(target_os = "linux")]
        const SOL_SOCKET: i32 = 1;
        #[cfg(target_os = "linux")]
        const SO_LINGER: i32 = 13;
        #[cfg(not(target_os = "linux"))]
        const SOL_SOCKET: i32 = 0xffff;
        #[cfg(not(target_os = "linux"))]
        const SO_LINGER: i32 = 0x0080;
        let l = Linger { l_onoff: 1, l_linger: 0 };
        unsafe {
            setsockopt(s.as_raw_fd(), SOL_SOCKET, SO_LINGER, &l as *const Linger as *const std::ffi::c_void, std::mem::size_of::<Linger>() as u32);
        }
    }
    s
}

#[test]
fn http_versions_other_than_1_0_and_1_1_get_505_and_the_connection_closes() {
    // `GET / HTTP/2.0` was never answered: the 505 waited behind the
    // request's own writer forever (a thread and a socket held); HTTP/0.9
    // was served as a keep-alive 1.x
    let dir = app("versions");
    let (_s, port) = serve(&dir, &[]);
    for v in ["HTTP/2.0", "HTTP/3.0", "HTTP/0.9"] {
        let out = raw(port, &format!("GET /hello/x {v}\r\nHost: localhost\r\n\r\nGET /hello/y HTTP/1.1\r\nHost: localhost\r\n\r\n"), 3000);
        assert!(out.starts_with("HTTP/1.1 505"), "{v}: {out}");
        // like every other answer, readable cross-origin
        assert!(out.to_ascii_lowercase().contains("access-control-allow-origin: *"), "{v}: {out}");
        assert!(!out.contains("<TIMEOUT>") && !out.contains("hi y"), "{v}: the connection must close: {out}");
    }
}

#[test]
fn bodiless_statuses_carry_no_framing_headers() {
    // RFC 9110 §8.6: no Content-Length in a 1xx or 204; a 304's described
    // an empty 200; the OPTIONS 204 said text/plain; a 205 had content
    let dir = app("bodiless");
    let (_s, port) = serve(&dir, &[]);
    for code in [204, 304] {
        let h = head_of(&raw(port, &format!("GET /stat/{code} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"), 3000));
        assert!(h.starts_with(&format!("http/1.1 {code}")), "{h}");
        assert!(!h.contains("content-length") && !h.contains("transfer-encoding"), "{code}: {h}");
    }
    let out = raw(port, "GET /stat/205 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", 3000);
    assert!(out.starts_with("HTTP/1.1 205") && out.to_ascii_lowercase().contains("content-length: 0") && out.ends_with("\r\n\r\n"), "{out}");
    let h = head_of(&raw(port, "OPTIONS /hello HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", 3000));
    assert!(h.starts_with("http/1.1 204") && !h.contains("\ncontent-type:") && !h.contains("content-length"), "{h}");
    // the interim 100 carries no Content-Length either
    let out = raw(port, "POST /echo HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nExpect: 100-continue\r\nContent-Length: 7\r\nConnection: close\r\n\r\n{\"a\":1}", 3000);
    let interim = out.split("\r\n\r\n").next().unwrap_or("").to_ascii_lowercase();
    assert!(interim.starts_with("http/1.1 100") && !interim.contains("content-length"), "{out}");
    assert!(out.contains("{\"a\":1}"), "{out}");
}

#[test]
fn an_http_1_0_expectation_is_ignored() {
    // a 1.0 client does not know `100 Continue`: it read it as the response
    let dir = app("expect10");
    let (_s, port) = serve(&dir, &[]);
    let out = raw(port, "POST /echo HTTP/1.0\r\nContent-Type: application/json\r\nExpect: 100-continue\r\nContent-Length: 7\r\n\r\n{\"a\":1}", 3000);
    assert!(out.starts_with("HTTP/1.0 200") && !out.contains(" 100 "), "{out}");
}

#[test]
fn malformed_targets_missing_host_and_1_0_chunked_are_400() {
    let dir = app("targets");
    let (_s, port) = serve(&dir, &[]);
    // a target that is not `/…` was routed (`GET hello/x` → hello("x"))
    let out = raw(port, "GET hello/x HTTP/1.1\r\nHost: localhost\r\n\r\n", 3000);
    assert!(out.starts_with("HTTP/1.1 400") && !out.contains("hi x") && !out.contains("<TIMEOUT>"), "{out}");
    // RFC 9112 §3.2: HTTP/1.1 without Host
    let out = raw(port, "GET /hello/x HTTP/1.1\r\n\r\n", 3000);
    assert!(out.starts_with("HTTP/1.1 400") && !out.contains("hi x"), "{out}");
    // …while HTTP/1.0 without Host, `OPTIONS *` and absolute-form still work
    assert!(raw(port, "GET /hello/x HTTP/1.0\r\n\r\n", 3000).starts_with("HTTP/1.0 200"));
    assert!(raw(port, "OPTIONS * HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", 3000).starts_with("HTTP/1.1 204"));
    assert!(raw(port, &format!("GET http://localhost:{port}/hello/x HTTP/1.1\r\nConnection: close\r\n\r\n"), 3000).starts_with("HTTP/1.1 200"));
    // the refusals tiny_http makes itself carry the CORS header too
    let out = raw(port, &format!("GET /hello/x HTTP/1.1\r\nHost: localhost\r\nCookie: {}\r\n\r\n", "a".repeat(17 * 1024)), 3000);
    assert!(out.starts_with("HTTP/1.1 431") && out.to_ascii_lowercase().contains("access-control-allow-origin: *"), "{out}");
    // RFC 9112 §6.1: Transfer-Encoding in HTTP/1.0 is faulty framing
    let out = raw(port, "POST /echo HTTP/1.0\r\nTransfer-Encoding: chunked\r\nContent-Type: application/json\r\n\r\n7\r\n{\"a\":1}\r\n0\r\n\r\n", 3000);
    assert!(out.starts_with("HTTP/1.0 400") && !out.contains("\"a\":1"), "{out}");
}

#[test]
fn a_head_request_on_an_sse_route_gets_headers_only() {
    let dir = app("sse_head");
    let (_s, port) = serve(&dir, &[]);
    let out = raw(port, "HEAD /stream HTTP/1.1\r\nHost: localhost\r\n\r\n", 1500);
    assert!(out.starts_with("HTTP/1.1 200") && out.to_ascii_lowercase().contains("text/event-stream"), "{out}");
    assert!(!out.contains("event: connected") && !out.contains("data:"), "a HEAD response has no content: {out}");
}

#[test]
fn a_refusal_pipelined_behind_a_stream_does_not_stall_other_clients() {
    // refusals are made in the accept loop, and a connection's answers go
    // out in order: a bad-Host request pipelined behind an SSE stream held
    // the accept loop (and so every client) until the stream ended
    let dir = app("stall");
    let (_s, port) = serve(&dir, &[]);
    let mut a = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    a.write_all(b"GET /stream HTTP/1.1\r\nHost: localhost\r\n\r\nGET /hello/x HTTP/1.1\r\nHost: evil.example\r\n\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let t0 = Instant::now();
    let out = raw(port, "GET /hello/y HTTP/1.0\r\n\r\n", 5000);
    assert!(out.starts_with("HTTP/1.0 200") && out.contains("hi y"), "another client was not served: {out}");
    assert!(t0.elapsed() < Duration::from_secs(3), "served only after {:?}", t0.elapsed());
    drop(a);
}

#[test]
fn a_bracketed_ipv6_loopback_host_keeps_the_loopback_checks() {
    // `--host [::1]` is loopback, but only the spellings 127.0.0.1 /
    // localhost / ::1 turned the Host and Origin checks on
    if std::net::TcpListener::bind("[::1]:0").is_err() {
        return; // no IPv6 loopback on this machine
    }
    let dir = app("v6");
    let (_s, port) = serve(&dir, &["--host", "[::1]"]);
    let send = |req: &str| -> String {
        let Ok(mut s) = std::net::TcpStream::connect(("::1", port)) else { return String::new() };
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let _ = s.write_all(req.as_bytes());
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    while !send("GET /hello/x HTTP/1.0\r\n\r\n").starts_with("HTTP/") {
        assert!(Instant::now() < deadline, "server did not answer on [::1]");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(send("GET /hello/x HTTP/1.0\r\nHost: [::1]\r\n\r\n").starts_with("HTTP/1.0 200"));
    let out = send("GET /hello/x HTTP/1.0\r\nHost: evil.example\r\n\r\n");
    assert!(out.starts_with("HTTP/1.0 403") && !out.contains("hi x"), "{out}");
    let log = std::fs::read_to_string(dir.join("serve.log")).unwrap_or_default();
    // the banner is a URL (it said `http://::1:…`)
    assert!(log.contains(&format!("listening on http://[::1]:{port}")), "{log}");
}

fn ws_client(port: u16) -> tungstenite::WebSocket<std::net::TcpStream> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(stream) = std::net::TcpStream::connect(("127.0.0.1", port + 1)) {
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            if let Ok((ws, _)) = tungstenite::client::client(format!("ws://127.0.0.1:{}/", port + 1), stream) {
                return ws;
            }
        }
        assert!(Instant::now() < deadline, "no websocket");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn websocket_replies_are_json_and_frame_text_cannot_forge_log_lines() {
    let dir = app("ws");
    let (_s, port) = serve(&dir, &[]);
    let mut ws = ws_client(port);
    // a Map reply is JSON (its display text sent `NaN`)
    ws.send(tungstenite::Message::Text("nan".into())).unwrap();
    let reply = ws.read().unwrap().into_text().unwrap();
    let v: serde_json::Value = serde_json::from_str(&reply).unwrap_or_else(|e| panic!("not JSON ({e}): {reply}"));
    assert!(v["n"].is_null() && v["q"] == "a\"b", "{reply}");
    // the error log line escapes the frame's text
    ws.send(tungstenite::Message::Text("x\nPOST /admin/grant → 200 1ms".into())).unwrap();
    let _ = ws.read();
    std::thread::sleep(Duration::from_millis(300));
    let log = std::fs::read_to_string(dir.join("serve.log")).unwrap_or_default();
    assert!(!log.lines().any(|l| l.starts_with("POST /admin/grant")), "forged log line: {log}");
}

#[test]
fn a_map_argument_from_the_query_or_path_refuses_numbers_beyond_float_range() {
    // the body refused 1e400; `?data={"a":1e400}` handed the handler an inf
    let dir = app("inf");
    let (_s, port) = serve(&dir, &[]);
    for target in ["/echo?data=%7B%22a%22%3A1e400%7D", "/echo/%7B%22a%22%3A%5B-1e400%5D%7D"] {
        let out = raw(port, &format!("GET {target} HTTP/1.0\r\n\r\n"), 3000);
        assert!(out.starts_with("HTTP/1.0 400"), "{target}: {out}");
    }
    let out = raw(port, "GET /echo?data=%7B%22a%22%3A1e300%7D HTTP/1.0\r\n\r\n", 3000);
    assert!(out.starts_with("HTTP/1.0 200") && out.contains("1e300"), "{out}");
}

#[test]
fn static_files_unreadable_are_404_and_upper_case_extensions_get_their_type() {
    let dir = app("static");
    std::fs::create_dir_all(dir.join("static")).unwrap();
    std::fs::write(dir.join("static/logo.PNG"), b"\x89PNG").unwrap();
    std::fs::write(dir.join("static/locked.txt"), b"secret").unwrap();
    let (_s, port) = serve(&dir, &[]);
    let h = head_of(&raw(port, "GET /static/logo.PNG HTTP/1.0\r\n\r\n", 3000));
    assert!(h.starts_with("http/1.0 200") && h.contains("content-type: image/png"), "{h}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("static/locked.txt");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
        // (root reads it anyway: nothing to check then)
        if std::fs::read(&p).is_err() {
            let out = raw(port, "GET /static/locked.txt HTTP/1.0\r\n\r\n", 3000);
            assert!(out.starts_with("HTTP/1.0 404"), "an unreadable file answered: {out}");
        }
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644));
    }
}
