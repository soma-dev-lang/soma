//! The first five minutes of an agent (or a person) with Soma must work
//! with nothing but the binary: `soma init` → check → verify → test, the
//! docs offline, and a verified example one command away.

use std::io::{Read, Write};
use std::process::Command;

fn soma_in(dir: &std::path::Path, home: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> (String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soma"));
    // HOME is empty: no ~/.soma/stdlib to lean on
    cmd.args(args).current_dir(dir).env("HOME", home).env_remove("SOMA_LLM_MOCK");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let o = cmd.output().expect("failed to run soma");
    (
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
        o.status.code().unwrap_or(-1),
    )
}

fn scratch(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&root);
    let (work, home) = (root.join("work"), root.join("home"));
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    (work, home)
}

#[test]
fn init_produces_a_project_that_passes_the_whole_loop() {
    let (work, home) = scratch("soma_onboarding_init");
    let (out, code) = soma_in(&work, &home, &["init", "myapp"], &[]);
    assert_eq!(code, 0, "{out}");
    let app = work.join("myapp");
    // the name every doc uses — `soma check app.cell` must not be "file not found"
    assert!(app.join("app.cell").exists(), "init must create app.cell: {out}");
    let agents = std::fs::read_to_string(app.join("AGENTS.md")).expect("AGENTS.md");
    assert!(agents.contains("soma check") && agents.contains("soma verify"), "{agents}");

    let (out, code) = soma_in(&app, &home, &["check", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}");
    // the stdlib is embedded: `persistent` is known without any install
    assert!(!out.contains("unknown property"), "stdlib must resolve from the binary alone: {out}");
    let (out, code) = soma_in(&app, &home, &["verify", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("0 failures"), "{out}");
    let (out, code) = soma_in(&app, &home, &["test", "app.cell"], &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("0 failed"), "{out}");
}

#[test]
fn docs_are_embedded_in_the_binary() {
    let (work, home) = scratch("soma_onboarding_docs");
    let (out, code) = soma_in(&work, &home, &["docs"], &[]);
    assert_eq!(code, 0, "{out}");
    for topic in ["agent", "reference", "gotchas", "builtins", "agents-md"] {
        assert!(out.contains(topic), "`soma docs` must list {topic}: {out}");
        let (body, code) = soma_in(&work, &home, &["docs", topic], &[]);
        assert_eq!(code, 0, "soma docs {topic}");
        assert!(body.len() > 1500, "soma docs {topic} is suspiciously short ({} bytes)", body.len());
    }
    let (out, code) = soma_in(&work, &home, &["docs", "nope"], &[]);
    assert_ne!(code, 0);
    assert!(out.contains("Valid topics"), "{out}");
}

/// Serve two canned documents, then stop.
fn serve_once(listener: std::net::TcpListener, routes: Vec<(String, String)>) {
    std::thread::spawn(move || {
        for stream in listener.incoming().take(8) {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (status, body) = match routes.iter().find(|(p, _)| *p == path) {
                Some((_, b)) => ("200 OK", b.clone()),
                None => ("404 Not Found", "nope".to_string()),
            };
            let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
}

#[test]
fn example_searches_the_corpus_index_and_prints_sources() {
    let (work, home) = scratch("soma_onboarding_example");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let index = r#"{"soma_version":"x","count":2,"domains":{"finance":1,"games":1},
      "features":["invariant","http"],
      "programs":[
        {"id":"finance/ledger","domain":"finance","title":"A ledger that cannot overdraw","summary":"escrow","features":["invariant"]},
        {"id":"games/dice","domain":"games","title":"Dice","summary":"rolls","features":["http"]}]}"#;
    serve_once(listener, vec![
        ("/corpus/full.json".to_string(), index.to_string()),
        ("/corpus/finance/ledger.cell".to_string(), "cell Ledger { }\n".to_string()),
    ]);
    let env = [("SOMA_SITE", base.as_str())];

    let (out, code) = soma_in(&work, &home, &["example", "invariant"], &env);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("finance/ledger") && !out.contains("games/dice"), "{out}");

    let (out, code) = soma_in(&work, &home, &["example", "finance/ledger"], &env);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "cell Ledger { }\n");

    let (out, code) = soma_in(&work, &home, &["example", "zzz"], &env);
    assert_ne!(code, 0);
    assert!(out.contains("no verified program matches"), "{out}");
}
