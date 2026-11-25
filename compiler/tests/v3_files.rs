//! What a program needs to work on a directory of files and to talk with whoever runs it:
//! `list_dir`, `file_info`, `append_file`, `read_line` — and the headers of an HTTP
//! failure (`retry-after`). None of them raises on what the file system does: a failure is
//! `{error, kind}`.

use std::io::Write;
use std::process::{Command, Stdio};

fn soma(dir: &std::path::Path, args: &[&str], stdin: &str) -> (String, i32) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_soma"));
    c.args(args).current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().expect("soma runs");
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    (format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)), o.status.code().unwrap_or(-1))
}

fn project(name: &str, source: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_files_{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.cell"), source).unwrap();
    dir
}

const PROG: &str = r#"cell F {
    on lister(d: String) { return list_dir(d) }
    on info(p: String) { return file_info(p) }
    on ajouter(p: String) {
        let a = append_file(p, "un\n")
        let b = append_file(p, "deux\n")
        return [a, b, read_file(p)]
    }
    on dialogue() {
        let a = read_line()
        let b = read_line()
        let c = read_line()
        return [a, b, c]
    }
}
"#;

#[test]
fn list_dir_names_each_entry_with_its_kind() {
    let dir = project("list", PROG);
    std::fs::create_dir_all(dir.join("d/sous")).unwrap();
    std::fs::write(dir.join("d/b.txt"), "12345").unwrap();
    std::fs::write(dir.join("d/a.txt"), "x").unwrap();
    let (out, code) = soma(&dir, &["run", "app.cell", "lister", "d"], "");
    assert_eq!(code, 0, "{out}");
    // sorted by name, with kind and size
    let a = out.find("a.txt").expect(&out);
    let b = out.find("b.txt").expect(&out);
    let s = out.find("sous").expect(&out);
    assert!(a < b && b < s, "{out}");
    assert!(out.contains("\"dir\"") || out.contains("dir"), "{out}");
    assert!(out.contains('5'), "{out}");
    // a missing directory: {error, kind: not_found}, no raise
    let (out, code) = soma(&dir, &["run", "app.cell", "lister", "absent"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("not_found"), "{out}");
}

#[test]
fn file_info_says_whether_a_path_exists_and_what_it_is() {
    let dir = project("info", PROG);
    std::fs::write(dir.join("f.txt"), "abc").unwrap();
    let (out, code) = soma(&dir, &["run", "app.cell", "info", "f.txt"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("file") && out.contains('3') && out.contains("mtime"), "{out}");
    let (out, _) = soma(&dir, &["run", "app.cell", "info", "nulle-part"], "");
    assert!(out.contains("false"), "{out}");
}

#[test]
fn append_file_adds_to_the_end() {
    let dir = project("append", PROG);
    let (out, code) = soma(&dir, &["run", "app.cell", "ajouter", "journal/t.log"], "");
    assert_eq!(code, 0, "{out}");
    assert_eq!(std::fs::read_to_string(dir.join("journal/t.log")).unwrap(), "un\ndeux\n");
    assert!(out.contains("true"), "{out}");
}

#[test]
fn read_line_reads_one_line_at_a_time_and_unit_at_the_end() {
    let dir = project("line", PROG);
    let (out, code) = soma(&dir, &["run", "app.cell", "dialogue"], "bonjour\nau revoir\n");
    assert_eq!(code, 0, "{out}");
    // the newline is not part of the line; the end of input is ()
    assert!(out.contains("bonjour") && out.contains("au revoir"), "{out}");
    assert!(!out.contains("bonjour\\n"), "{out}");
    assert!(out.contains("()") || out.contains("null"), "{out}");
}

#[test]
fn file_builtins_are_effects_a_frame_names() {
    let dir = project("frame", r#"skeleton S {
    face {
        signal voir(d: String) -> List
        signal noter(p: String) -> Bool
    }
}
flesh S {
    on voir(d) { return list_dir(d) }
    on noter(p) { return append_file(p, "x") == true }
}
"#);
    let (out, code) = soma(&dir, &["check", "app.cell"], "");
    assert_ne!(code, 0, "{out}");
    assert!(out.contains("list_dir"), "{out}");
    assert!(out.contains("append_file"), "{out}");
}

#[test]
fn an_http_failure_carries_the_response_headers() {
    // a server that answers 429 with retry-after
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = listener.accept() {
            use std::io::Read;
            // read the whole request (headers and the JSON body) before answering
            let mut got: Vec<u8> = Vec::new();
            let mut buf = [0u8; 4096];
            while !String::from_utf8_lossy(&got).ends_with('}') {
                match s.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => got.extend_from_slice(&buf[..n]) }
            }
            let body = r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#;
            let _ = write!(s, "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 7\r\nRequest-Id: req_abc\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
        }
    });
    let dir = project("headers", &format!(r#"cell H {{
    on essayer() {{
        let r = http_post("http://127.0.0.1:{port}/v1/messages", map("a", 1))
        return [r.status, r.headers["retry-after"], r.headers["request-id"], r.body.error.type]
    }}
}}
"#));
    let (out, code) = soma(&dir, &["run", "app.cell", "essayer"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("429") && out.contains('7') && out.contains("req_abc") && out.contains("rate_limit_error"), "{out}");
}
