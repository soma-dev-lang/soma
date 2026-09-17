//! `soma serve` static files: confined to <project>/static.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

fn get(port: u16, path: &str) -> String {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    write!(s, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn static_files_cannot_escape_the_static_dir() {
    let dir = std::env::temp_dir().join("soma_serve_static");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("static")).unwrap();
    std::fs::write(dir.join("static/a.css"), "body{}").unwrap();
    std::fs::write(
        dir.join("soma.toml"),
        "[package]\nname = \"w\"\nversion = \"0.1.0\"\n[agent]\nkey = \"sk-SECRET\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("app.cell"),
        r#"
cell W {
    face { signal request(method: String, path: String, body: String) -> String }
    on request(method: String, path: String, body: String) { return "ok" }
}
"#,
    )
    .unwrap();

    let port = 18500 + (std::process::id() % 400) as u16;
    let mut child = Command::new(env!("CARGO_BIN_EXE_soma"))
        .args(["serve", "app.cell", "-p", &port.to_string()])
        .current_dir(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start soma serve");
    let mut up = false;
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            up = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let results = if up {
        Some((
            get(port, "/static/a.css"),
            get(port, "/static/a.css?v=2"),
            get(port, "/static/../soma.toml"),
            get(port, "/static/../app.cell"),
        ))
    } else {
        None
    };
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);

    let (css, css_q, toml, cell) = results.expect("server did not start");
    assert!(css.contains("body{}"), "static file must be served: {css}");
    assert!(css_q.contains("body{}"), "?query must be ignored: {css_q}");
    assert!(!toml.contains("sk-SECRET"), "soma.toml leaked through /static/..: {toml}");
    assert!(toml.starts_with("HTTP/1.0 403") || toml.starts_with("HTTP/1.1 403"), "got: {toml}");
    assert!(!cell.contains("cell W"), "source leaked through /static/..: {cell}");
}
