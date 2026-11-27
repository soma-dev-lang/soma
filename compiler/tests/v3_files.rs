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

#[test]
fn run_exit_status_makes_an_int_result_the_status_of_the_process() {
    let dir = project("status", "cell R {\n    on main(n: Int) { print(\"sortie\")  return n }\n    on texte() { return \"t\" }\n}\n");
    // without the option: the value is printed, the status is 0
    let (out, code) = soma(&dir, &["run", "app.cell", "main", "3"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("sortie") && out.trim_end().ends_with('3'), "{out}");
    // with it: the Int is the status, and it is not printed
    let (out, code) = soma(&dir, &["run", "--exit-status", "app.cell", "main", "3"], "");
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("sortie") && !out.trim_end().ends_with('3'), "{out}");
    let (_, code) = soma(&dir, &["run", "--exit-status", "app.cell", "main", "0"], "");
    assert_eq!(code, 0);
    // a value that is not an Int is printed as before
    let (out, code) = soma(&dir, &["run", "--exit-status", "app.cell", "texte"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains('t'), "{out}");
}

#[test]
fn run_ephemeral_keeps_nothing_on_disk_between_runs() {
    let src = "cell E {\n    memory { vus: Map<String, Int> [persistent] }\n    state m {\n        initial: a\n        a -> b\n    }\n    on avancer(id: String) {\n        transition(id, \"a\", \"b\")\n        vus.set(id, (vus.get(id) ?? 0) + 1)\n        return \"{get_status(id)} {vus.get(id)} {next_id()}\"\n    }\n}\n";
    let dir = project("ephemeral", src);
    // without the option, the machine and the slot persist: the second run finds `x` in `b`
    let (out, code) = soma(&dir, &["run", "app.cell", "avancer", "x"], "");
    assert_eq!(code, 0, "{out}");
    let (out, code) = soma(&dir, &["run", "app.cell", "avancer", "x"], "");
    assert_ne!(code, 0, "{out}");
    // with it, every run starts from nothing, and nothing is written beside the program
    let dir = project("ephemeral2", src);
    for _ in 0..2 {
        let (out, code) = soma(&dir, &["run", "--ephemeral", "app.cell", "avancer", "x"], "");
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("b 1 1"), "{out}");
    }
    assert!(!dir.join(".soma_data").exists(), "--ephemeral wrote .soma_data");
}

// ── a file another program reads while this one writes it ───────────────

/// `write_file` truncated the file, then wrote it: a reader in between saw
/// an empty or a half-written file. A rewrite of a Python application kept a
/// Python service for that alone ("write a file atomically"). The content is
/// now written beside the file and renamed over it: a reader sees the old
/// file or the new one.
#[test]
fn write_file_replaces_the_file_in_one_step() {
    let dir = project("atomic", r#"cell W {
    on ecrire(p: String, fois: Int) {
        let a = read_file("a.txt")
        let b = read_file("b.txt")
        for k in range(0, fois) {
            let _ = write_file(p, if k % 2 == 0 { a } else { b })
        }
        return fois
    }
    on rate() { return write_file("pas/un/dossier\u{0}/x", "y") }
}
"#);
    let (a, b) = ("a".repeat(4_000_000), "b".repeat(4_000_000));
    std::fs::write(dir.join("a.txt"), &a).unwrap();
    std::fs::write(dir.join("b.txt"), &b).unwrap();
    std::fs::write(dir.join("cible.txt"), &a).unwrap();
    let target = dir.join("cible.txt");
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (stop2, target2, a2, b2) = (stop.clone(), target.clone(), a.clone(), b.clone());
    let reader = std::thread::spawn(move || {
        let (mut reads, mut torn) = (0u32, 0u32);
        while !stop2.load(std::sync::atomic::Ordering::Relaxed) {
            if let Ok(s) = std::fs::read_to_string(&target2) {
                reads += 1;
                if s != a2 && s != b2 { torn += 1; }
            }
        }
        (reads, torn)
    });
    let (out, code) = soma(&dir, &["run", "--ephemeral", "app.cell", "ecrire", "cible.txt", "60"], "");
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let (reads, torn) = reader.join().unwrap();
    assert_eq!(code, 0, "{out}");
    assert!(reads > 20, "the reader read the file {reads} times");
    assert_eq!(torn, 0, "{torn} of {reads} reads saw a file that was neither the old one nor the new one");
    // nothing is left beside the file
    let left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.contains("cible") && n != "cible.txt").collect();
    assert!(left.is_empty(), "{left:?}");
    // a failure is still a value
    let (out, _) = soma(&dir, &["run", "--ephemeral", "app.cell", "rate"], "");
    assert!(out.contains("error"), "{out}");
}

/// `file_lock(path)`: one program at a time in a section, across processes —
/// the other half of "write a file atomically and under a lock". The lock is
/// held until the handler ends, whatever way it ends: there is nothing to
/// release and nothing to forget.
#[test]
fn file_lock_is_held_until_the_handler_ends() {
    let dir = project("lock", r#"cell L {
    on tenir(ms: Int) {
        let got = file_lock("donnees.lock")
        let _ = append_file("journal.txt", "tenir:pris\n")
        let _ = sleep(ms)
        let _ = append_file("journal.txt", "tenir:fin\n")
        return got
    }
    on essayer() { return file_lock("donnees.lock", map("timeout", 0)) }
    on attendre() {
        let got = file_lock("donnees.lock", map("timeout", 8000))
        let _ = append_file("journal.txt", "attendre:pris\n")
        return got
    }
    on rate() {
        let got = file_lock("donnees.lock")
        let x = 1 / (len("") * 1)
        return got
    }
    on deux() { return [file_lock("donnees.lock"), file_lock("donnees.lock")] }
}
"#);
    let d1 = dir.clone();
    let holder = std::thread::spawn(move || soma(&d1, &["run", "--ephemeral", "app.cell", "tenir", "1500"], ""));
    // wait for the first to hold it
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !std::fs::read_to_string(dir.join("journal.txt")).unwrap_or_default().contains("tenir:pris") {
        assert!(std::time::Instant::now() < until, "the first program never took the lock");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // another program does not get it now…
    let (out, code) = soma(&dir, &["run", "--ephemeral", "app.cell", "essayer"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("\"kind\": \"locked\"") || out.contains("kind: locked") || out.contains("locked"), "{out}");
    assert!(!out.trim().ends_with("true"), "{out}");
    // …and gets it when the first handler has ended
    let (out, code) = soma(&dir, &["run", "--ephemeral", "app.cell", "attendre"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.trim().ends_with("true"), "{out}");
    let (out, code) = holder.join().unwrap();
    assert_eq!(code, 0, "{out}");
    assert!(out.trim().ends_with("true"), "{out}");
    let journal = std::fs::read_to_string(dir.join("journal.txt")).unwrap();
    assert_eq!(journal, "tenir:pris\ntenir:fin\nattendre:pris\n", "the section of the second ran after the first's");
    // a handler that fails leaves no lock behind
    let (_, code) = soma(&dir, &["run", "--ephemeral", "app.cell", "rate"], "");
    assert_ne!(code, 0);
    let (out, _) = soma(&dir, &["run", "--ephemeral", "app.cell", "essayer"], "");
    assert!(out.trim().ends_with("true"), "{out}");
    // taking a lock it already holds is not a wait on itself
    let (out, code) = soma(&dir, &["run", "--ephemeral", "app.cell", "deux"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("[true, true]"), "{out}");
}

/// `http_get(url, map("proxy", "http://user:pass@host:port"))`, or
/// `SOMA_HTTP_PROXY` for every request of the process: the request goes
/// through the proxy with its credentials, and the password is in no message.
#[test]
fn an_http_request_goes_through_an_authenticated_proxy() {
    use std::io::{BufRead, BufReader};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen2 = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten().take(2) {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" { break; }
                head.push_str(&line);
            }
            seen2.lock().unwrap().push(head);
            let body = "{\"par\":\"le proxy\"}";
            let mut s = stream;
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        }
    });
    let dir = project("proxy", &format!(r#"cell H {{
    on par_option() {{ return http_get("http://service.invalid/prix", map("proxy", "http://jean:s3cret@127.0.0.1:{port}")) }}
    on par_environnement() {{ return http_get("http://service.invalid/cours") }}
    on mauvais() {{ return http_get("http://service.invalid/x", map("proxy", "http://jean:s3cret@127.0.0.1:1", "timeout", 2000)) }}
}}
"#));
    let (out, code) = soma(&dir, &["run", "--ephemeral", "app.cell", "par_option"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("le proxy"), "{out}");
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(["run", "--ephemeral", "app.cell", "par_environnement"]).current_dir(&dir)
        .env("SOMA_HTTP_PROXY", format!("http://jean:s3cret@127.0.0.1:{port}")).output().unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(out.contains("le proxy"), "{out}");
    let heads = seen.lock().unwrap().clone();
    assert_eq!(heads.len(), 2, "{heads:?}");
    assert!(heads[0].starts_with("GET http://service.invalid/prix "), "{}", heads[0]);
    assert!(heads[1].starts_with("GET http://service.invalid/cours "), "{}", heads[1]);
    // jean:s3cret
    for h in &heads { assert!(h.to_lowercase().contains("proxy-authorization: basic amvhbjpzm2nyzxq="), "{h}"); }
    // a proxy that does not answer: a value, and no password in it
    let (out, code) = soma(&dir, &["run", "--ephemeral", "app.cell", "mauvais"], "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("error") && !out.contains("s3cret"), "{out}");
}
