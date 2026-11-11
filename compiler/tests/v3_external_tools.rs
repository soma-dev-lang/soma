// 3.0 — C5: tools from outside. A `tool` bound with `[mcp: "server"]` or
// `[openapi: "server/operationId"]` runs on a server of soma.toml's
// `[tools.*]`; `soma tools sync` pins its contract in soma.lock, `soma check`
// holds the declaration to the pin offline, and a server whose contract
// drifted is refused at run time before anything is sent to it.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn soma_in(dir: &std::path::Path, args: &[&str]) -> (String, String, i32) {
    let o = Command::new(env!("CARGO_BIN_EXE_soma")).args(args).current_dir(dir)
        .env_remove("SOMA_EDITION").env("DOCS_TOKEN", "t0ken").output().expect("failed to run soma");
    (String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string(), o.status.code().unwrap_or(-1))
}

fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("soma_v3x_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A fake server: an MCP endpoint at /mcp (Streamable HTTP, answers as an
/// event stream, with a session) and a pet API under /api.
struct Fake {
    port: u16,
    /// the server's `search` tool grew a required argument
    drifted: Arc<AtomicBool>,
    /// every request: "METHOD path | authorization | body"
    seen: Arc<Mutex<Vec<String>>>,
}

fn fake() -> Fake {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let drifted = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (d, s) = (drifted.clone(), seen.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() { continue; }
            let mut parts = line.split_whitespace();
            let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
            let (mut length, mut auth, mut session) = (0usize, String::new(), String::new());
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).is_err() || h.trim().is_empty() { break; }
                let (k, v) = h.split_once(':').map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).unwrap_or_default();
                match k.as_str() { "content-length" => length = v.parse().unwrap_or(0), "authorization" => auth = v, "mcp-session-id" => session = v, _ => {} }
            }
            let mut body = vec![0u8; length];
            let _ = reader.read_exact(&mut body);
            let body = String::from_utf8_lossy(&body).to_string();
            s.lock().unwrap().push(format!("{} {} | {} | {}", method, path, auth, body));
            let msg: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
            let id = msg.get("id").cloned().unwrap_or(serde_json::Value::Null);
            let rpc = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let (status, kind, extra, out): (u16, &str, String, String) = if path == "/mcp" {
                match rpc {
                    "initialize" => (200, "application/json", "Mcp-Session-Id: s-1\r\n".to_string(),
                        serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": "2025-03-26", "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1"}}}).to_string()),
                    _ if session != "s-1" => (400, "application/json", String::new(), "{}".to_string()),
                    "notifications/initialized" => (202, "application/json", String::new(), String::new()),
                    "tools/list" => {
                        let (props, required) = if d.load(Ordering::SeqCst) {
                            (serde_json::json!({"query": {"type": "string"}, "limit": {"type": "integer"}, "lang": {"type": "string"}}), serde_json::json!(["query", "lang"]))
                        } else {
                            (serde_json::json!({"query": {"type": "string"}, "limit": {"type": "integer"}}), serde_json::json!(["query"]))
                        };
                        let result = serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {"tools": [
                            {"name": "search", "description": "Search the docs", "inputSchema": {"type": "object", "properties": props, "required": required}},
                            {"name": "boom", "description": "fails", "inputSchema": {"type": "object", "properties": {}}},
                        ]}});
                        (200, "text/event-stream", String::new(), format!("event: message\ndata: {}\n\n", result))
                    }
                    "tools/call" => {
                        let p = &msg["params"];
                        let result = if p["name"] == "boom" {
                            serde_json::json!({"content": [{"type": "text", "text": "kaput"}], "isError": true})
                        } else {
                            serde_json::json!({"content": [{"type": "text", "text": format!("found {} (limit {})", p["arguments"]["query"].as_str().unwrap_or("?"), p["arguments"]["limit"])}]})
                        };
                        (200, "application/json", String::new(), serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string())
                    }
                    _ => (200, "application/json", String::new(), serde_json::json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "unknown method"}}).to_string()),
                }
            } else if method == "GET" && path.starts_with("/api/pets/") {
                let rest = &path["/api/pets/".len()..];
                let (pid, query) = rest.split_once('?').unwrap_or((rest, ""));
                (200, "application/json", String::new(), serde_json::json!({"id": pid.parse::<i64>().unwrap_or(-1), "name": "rex", "query": query}).to_string())
            } else if method == "POST" && path == "/api/pets" {
                (201, "application/json", String::new(), serde_json::json!({"created": msg}).to_string())
            } else {
                (404, "application/json", String::new(), "{}".to_string())
            };
            let _ = write!(stream, "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}", status, kind, out.len(), extra, out);
        }
    });
    Fake { port, drifted, seen }
}

fn write_project(dir: &std::path::Path, port: u16, capability: &str) {
    std::fs::write(dir.join("soma.toml"), format!("[tools.docs]\nmcp = \"http://127.0.0.1:{port}/mcp\"\ntoken_env = \"DOCS_TOKEN\"\n\n[tools.pets]\nopenapi = \"petstore.json\"\n")).unwrap();
    std::fs::write(dir.join("petstore.json"), serde_json::json!({
        "openapi": "3.0.0", "servers": [{"url": format!("http://127.0.0.1:{port}/api")}],
        "paths": {
            "/pets/{petId}": {"get": {"operationId": "getPetById", "summary": "Fetch a pet", "parameters": [
                {"name": "petId", "in": "path", "required": true, "schema": {"type": "integer"}},
                {"name": "verbose", "in": "query", "schema": {"type": "boolean"}}]}},
            "/pets": {"post": {"operationId": "addPet", "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/NewPet"}}}}}}
        },
        "components": {"schemas": {"NewPet": {"type": "object", "required": ["name"], "properties": {"name": {"type": "string"}, "age": {"type": "integer"}}}}}
    }).to_string()).unwrap();
    std::fs::write(dir.join("app.cell"), format!(r#"cell agent Helper {{
    face {{
        signal find(q: String) -> String
        signal pet(id: Int) -> Map
        signal add(name: String) -> Map
        signal fail_it() -> String
        tool search(query: String, limit: Int) -> String [mcp: "docs", capability: "{capability}"] "Search the docs"
        tool boom() -> String [mcp: "docs"] "Always fails"
        tool get_pet(petId: Int, verbose: Bool) -> Map [openapi: "pets/getPetById"] "Fetch a pet"
        tool add_pet(name: String, age: Int) -> Map [openapi: "pets/addPet"] "Add a pet"
    }}

    state task {{
        initial: idle
        idle -> done
    }}

    on find(q: String) {{ return search(q, 3) }}
    on pet(id: Int) {{ return get_pet(id, true) }}
    on add(name: String) {{ return add_pet(name, 4) }}
    on fail_it() {{ return boom() }}
}}

cell test T {{
    rules {{
        assert_fails find("x") matching "a test does not call it"
        mock search "scripted"
        assert find("x") == "scripted"
    }}
}}
"#)).unwrap();
}

#[test]
fn c5_a_bound_tool_is_pinned_checked_and_called() {
    let server = fake();
    let dir = project("flow");
    write_project(&dir, server.port, &format!("http://127.0.0.1:{}/*", server.port));

    // not pinned yet: check says how
    let (out, err, code) = soma_in(&dir, &["check", "app.cell"]);
    let all = format!("{out}{err}");
    assert_ne!(code, 0, "{all}");
    assert_eq!(all.matches("which soma.lock does not pin — run `soma tools sync`").count(), 4, "{all}");
    assert!(all.contains("E0118"), "{all}");

    let (out, err, code) = soma_in(&dir, &["tools", "sync", "app.cell"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("pinned   docs/search") && out.contains("pinned   pets/addPet"), "{out}");
    let lock = std::fs::read_to_string(dir.join("soma.lock")).unwrap();
    assert!(lock.contains("[tools.\"docs/search\"]") && lock.contains("sha256 ="), "{lock}");
    // the token went as a header, read from the environment
    assert!(server.seen.lock().unwrap().iter().any(|r| r.starts_with("POST /mcp | Bearer t0ken |")), "{:?}", server.seen.lock().unwrap());

    // check is offline from here on
    let before = server.seen.lock().unwrap().len();
    let (out, err, code) = soma_in(&dir, &["check", "app.cell"]);
    assert_eq!(code, 0, "{out}{err}");
    let (out, _, _) = soma_in(&dir, &["tools", "list", "app.cell"]);
    assert!(out.contains("Helper.search  mcp docs/search  pinned"), "{out}");
    let (out, _, _) = soma_in(&dir, &["describe", "app.cell"]);
    assert!(out.contains("\"bound_to\""), "{out}");
    assert_eq!(out.matches("\"name\": \"search\"").count(), 2, "the tool and its binding, not a handler: {out}");
    // a test never reaches the server: the answer is scripted
    let (out, err, code) = soma_in(&dir, &["test", "app.cell"]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(server.seen.lock().unwrap().len(), before, "check, list, describe and test stay offline");

    // MCP: the arguments under the server's names, the text content back
    let (out, err, code) = soma_in(&dir, &["run", "app.cell", "find", "hello"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("found hello (limit 3)"), "{out}");
    // OpenAPI: path and query parameters, a JSON body from the arguments
    let (out, err, code) = soma_in(&dir, &["run", "app.cell", "pet", "7"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("\"id\": 7") && out.contains("verbose=true"), "{out}");
    let (out, err, code) = soma_in(&dir, &["run", "app.cell", "add", "rex"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("\"name\": \"rex\"") && out.contains("\"age\": 4"), "{out}");
    assert!(server.seen.lock().unwrap().iter().any(|r| r.starts_with("POST /api/pets |")), "{:?}", server.seen.lock().unwrap());
    // a failure on the server is an error of kind `tool`
    let (out, err, code) = soma_in(&dir, &["run", "app.cell", "fail_it"]);
    assert_ne!(code, 0);
    assert!(format!("{out}{err}").contains("failed on the server: kaput"), "{out}{err}");

    // the server changes the tool's contract: nothing is sent to it
    server.drifted.store(true, Ordering::SeqCst);
    let calls = |s: &Fake| s.seen.lock().unwrap().iter().filter(|r| r.contains("tools/call")).count();
    let sent = calls(&server);
    let (out, err, code) = soma_in(&dir, &["run", "app.cell", "find", "hello"]);
    assert_ne!(code, 0);
    assert!(format!("{out}{err}").contains("tool_schema"), "{out}{err}");
    assert_eq!(calls(&server), sent, "no tools/call after the drift");
    // sync shows what changed; check then says the declaration no longer fits
    let (out, err, code) = soma_in(&dir, &["tools", "sync", "app.cell"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("CHANGED  docs/search") && out.contains("\"lang\""), "{out}");
    let (out, err, code) = soma_in(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0);
    assert!(format!("{out}{err}").contains("the server requires `lang`, which the tool does not declare"), "{out}{err}");
}

#[test]
fn c5_check_holds_the_declaration_to_soma_toml_and_the_pin() {
    let server = fake();
    let dir = project("check");
    // a capability that does not allow the tool's own server
    write_project(&dir, server.port, "https://elsewhere.example/*");
    let (_, _, code) = soma_in(&dir, &["tools", "sync", "app.cell"]);
    assert_eq!(code, 0);
    let (out, err, code) = soma_in(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0);
    assert!(format!("{out}{err}").contains("do not allow that URL"), "{out}{err}");

    let src = std::fs::read_to_string(dir.join("app.cell")).unwrap();
    let fixed = src.replace("https://elsewhere.example/*", &format!("http://127.0.0.1:{}/*", server.port));
    let variants: [(&str, String, &str); 5] = [
        ("a wrong type", fixed.replace("tool search(query: String, limit: Int)", "tool search(query: String, limit: String)"), "`limit` is declared String, the server takes Int there"),
        ("an argument the server does not take", fixed.replace("tool search(query: String, limit: Int)", "tool search(query: String, limit: Int, page: Int)").replace("search(q, 3)", "search(q, 3, 1)"), "it declares `page`, which the server's tool does not take"),
        ("a required argument left out", fixed.replace("tool get_pet(petId: Int, verbose: Bool)", "tool get_pet(verbose: Bool)").replace("get_pet(id, true)", "get_pet(true)"), "the server requires `petId`"),
        ("a server soma.toml does not define", fixed.replace("[mcp: \"docs\"] \"Always fails\"", "[mcp: \"nowhere\"] \"Always fails\""), "which soma.toml does not define"),
        ("the wrong kind of server", fixed.replace("[mcp: \"docs\"] \"Always fails\"", "[mcp: \"pets\"] \"Always fails\""), "is an OpenAPI document"),
    ];
    for (what, source, needle) in variants {
        std::fs::write(dir.join("app.cell"), source).unwrap();
        let (out, err, code) = soma_in(&dir, &["check", "app.cell"]);
        assert_ne!(code, 0, "{what}: {out}{err}");
        assert!(format!("{out}{err}").contains(needle), "{what}: {out}{err}");
    }
    // bound AND handled: one of them answers
    std::fs::write(dir.join("app.cell"), fixed.replace("    on fail_it() { return boom() }", "    on fail_it() { return boom() }\n    on boom() { return \"local\" }")).unwrap();
    let (out, err, code) = soma_in(&dir, &["check", "app.cell"]);
    assert_ne!(code, 0);
    assert!(format!("{out}{err}").contains("and has a handler too"), "{out}{err}");
    // sync names what the server offers when the tool is not there
    std::fs::write(dir.join("app.cell"), fixed.replace("[mcp: \"docs\"] \"Always fails\"", "[mcp: \"docs/explode\"] \"Always fails\"")).unwrap();
    let (out, err, code) = soma_in(&dir, &["tools", "sync", "app.cell"]);
    assert_ne!(code, 0);
    assert!(format!("{out}{err}").contains("offers no tool `explode` (it offers: boom, search)"), "{out}{err}");
    assert!(format!("{out}{err}").contains("soma.lock was not changed"), "{out}{err}");
}
