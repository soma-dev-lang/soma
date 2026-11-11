//! 3.0: tools from outside. A `tool` of a face may run on a server instead
//! of in a handler:
//!
//!     tool search(query: String) -> String [mcp: "docs", capability: "https://docs.example/*"] "Search the docs"
//!     tool pet(petId: Int) -> Map [openapi: "pets/getPetById"] "Fetch a pet"
//!
//!     # soma.toml
//!     [tools.docs]
//!     mcp = "https://docs.example/mcp"        # Model Context Protocol, Streamable HTTP
//!     [tools.pets]
//!     openapi = "petstore.json"               # OpenAPI 3 (JSON): a file here, or a URL
//!
//! `soma tools sync` asks each server for the tool's contract and pins it
//! in `soma.lock`; `soma check` holds the declaration to the pin, offline;
//! at run time a server whose contract no longer matches the pin is refused
//! (kind `tool_schema`) before anything is sent to it. The call itself is an
//! HTTP request like any other: inside a model's tool call it passes the
//! tool's capabilities, without following redirects.
//!
//! This is Soma CONSUMING tools. It is not a Soma tool server.

use crate::ast::{CellDef, FaceDecl, Param, Section, ToolDecl, TypeExpr};
use crate::pkg::lock::{LockFile, LockedTool};
use crate::pkg::manifest::{Manifest, ToolServer};
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The directory holding soma.toml / soma.lock at run time: where the
/// program is (`.soma_data` sits beside it).
pub fn project_dir() -> PathBuf {
    let data = crate::runtime::storage::data_dir();
    data.parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

/// The bound tools of a cell.
pub fn bound_tools(cell: &CellDef) -> Vec<(&crate::ast::Spanned<FaceDecl>, &ToolDecl)> {
    let mut out = Vec::new();
    for section in &cell.sections {
        let Section::Face(face) = &section.node else { continue };
        for d in &face.declarations {
            if let FaceDecl::Tool(t) = &d.node { if t.binding.is_some() { out.push((d, t)); } }
        }
    }
    out
}

pub fn lock_key(server: &str, remote: &str) -> String { format!("{}/{}", server, remote) }

// ── canonical JSON and the pin ─────────────────────────────────────────

fn canonical(v: &Json) -> Json {
    match v {
        Json::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            Json::Object(keys.into_iter().map(|k| (k.clone(), canonical(&o[k]))).collect())
        }
        Json::Array(a) => Json::Array(a.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

fn sha256(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

fn pin(kind: &str, description: &str, schema: &Json) -> LockedTool {
    let schema = canonical(schema).to_string();
    LockedTool { kind: kind.to_string(), sha256: sha256(&schema), description: description.to_string(), schema }
}

// ── HTTP ───────────────────────────────────────────────────────────────

struct Reply { status: u16, session: Option<String>, text: String }

fn bearer(server: &ToolServer) -> Result<Option<String>, String> {
    match &server.token_env {
        None => Ok(None),
        Some(var) => match std::env::var(var) {
            Ok(t) if !t.is_empty() => Ok(Some(t)),
            _ => Err(format!("the server's token is read from ${} (token_env in soma.toml), which is not set", var)),
        },
    }
}

fn http(method: &str, url: &str, server: &ToolServer, body: Option<&Json>, headers: &[(String, String)]) -> Result<Reply, String> {
    use std::io::Read;
    // a redirect is never followed: it would leave the URL the capability (and the pin) names
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_millis(server.timeout_ms.unwrap_or(30_000)))
        .redirects(0).build();
    let mut req = agent.request(method, url).set("Accept", "application/json, text/event-stream");
    if let Some(token) = bearer(server)? { req = req.set("Authorization", &format!("Bearer {}", token)); }
    for (k, v) in headers { req = req.set(k, v); }
    let result = match body {
        Some(b) => req.set("Content-Type", "application/json").send_string(&b.to_string()),
        None => req.call(),
    };
    let resp = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(t)) => return Err(format!("{} {}: {}", method, crate::interpreter::builtins::http::redact_url(url), t.kind())),
    };
    let status = resp.status();
    let session = resp.header("Mcp-Session-Id").map(|s| s.to_string());
    let mut buf = Vec::new();
    resp.into_reader().take(16 * 1024 * 1024).read_to_end(&mut buf).map_err(|e| format!("{} {}: reading the answer failed: {}", method, crate::interpreter::builtins::http::redact_url(url), e))?;
    Ok(Reply { status, session, text: String::from_utf8_lossy(&buf).into_owned() })
}

// ── MCP (Streamable HTTP) ──────────────────────────────────────────────

/// The JSON-RPC answer in a body that is JSON, or a server-sent event stream.
fn rpc_answer(text: &str, id: u64) -> Option<Json> {
    let t = text.trim_start();
    if t.starts_with('{') {
        return serde_json::from_str::<Json>(t).ok();
    }
    // `data: {…}` lines; the answer is the message carrying our id
    let mut last = None;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else { continue };
        if let Ok(v) = serde_json::from_str::<Json>(data.trim()) {
            if v.get("id").and_then(|i| i.as_u64()) == Some(id) { return Some(v); }
            last = Some(v);
        }
    }
    last
}

/// Sessions, per endpoint: `initialize` once per process.
static SESSIONS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

fn mcp_initialize(url: &str, server: &ToolServer) -> Result<Option<String>, String> {
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-03-26", "capabilities": {},
        "clientInfo": {"name": "soma", "version": env!("CARGO_PKG_VERSION")}}});
    let reply = http("POST", url, server, Some(&init), &[])?;
    if !(200..300).contains(&reply.status) {
        return Err(format!("the MCP server answered `initialize` with HTTP {}", reply.status));
    }
    let answer = rpc_answer(&reply.text, 1).ok_or("the MCP server's answer to `initialize` is not JSON-RPC")?;
    if let Some(e) = answer.get("error") {
        return Err(format!("the MCP server refused `initialize`: {}", e.get("message").and_then(|m| m.as_str()).unwrap_or("error")));
    }
    let mut headers = Vec::new();
    if let Some(s) = &reply.session { headers.push(("Mcp-Session-Id".to_string(), s.clone())); }
    let _ = http("POST", url, server, Some(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})), &headers)?;
    Ok(reply.session)
}

fn mcp_request(url: &str, server: &ToolServer, method: &str, params: Json) -> Result<Json, String> {
    for attempt in 0..2 {
        let known = SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).get(url).cloned();
        let session = match known {
            Some(s) => s,
            None => {
                let s = mcp_initialize(url, server)?;
                SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(url.to_string(), s.clone());
                s
            }
        };
        let mut headers = Vec::new();
        if let Some(s) = &session { headers.push(("Mcp-Session-Id".to_string(), s.clone())); }
        let reply = http("POST", url, server, Some(&json!({"jsonrpc": "2.0", "id": 2, "method": method, "params": params})), &headers)?;
        // the session ended on the server: one new `initialize`
        if reply.status == 404 && session.is_some() && attempt == 0 {
            SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).remove(url);
            continue;
        }
        if !(200..300).contains(&reply.status) {
            return Err(format!("the MCP server answered `{}` with HTTP {}", method, reply.status));
        }
        let answer = rpc_answer(&reply.text, 2).ok_or_else(|| format!("the MCP server's answer to `{}` is not JSON-RPC", method))?;
        if let Some(e) = answer.get("error") {
            return Err(format!("the MCP server refused `{}`: {}", method, e.get("message").and_then(|m| m.as_str()).unwrap_or("error")));
        }
        return Ok(answer.get("result").cloned().unwrap_or(Json::Null));
    }
    Err(format!("the MCP server ended the session twice during `{}`", method))
}

/// Every tool of an MCP server: name → pin.
fn mcp_tools(url: &str, server: &ToolServer) -> Result<HashMap<String, LockedTool>, String> {
    let mut out = HashMap::new();
    let mut cursor: Option<String> = None;
    for _ in 0..100 {
        let params = match &cursor { Some(c) => json!({"cursor": c}), None => json!({}) };
        let result = mcp_request(url, server, "tools/list", params)?;
        for t in result.get("tools").and_then(|t| t.as_array()).cloned().unwrap_or_default() {
            let Some(name) = t.get("name").and_then(|n| n.as_str()) else { continue };
            let schema = t.get("inputSchema").cloned().unwrap_or(json!({"type": "object"}));
            out.insert(name.to_string(), pin("mcp", t.get("description").and_then(|d| d.as_str()).unwrap_or(""), &schema));
        }
        cursor = result.get("nextCursor").and_then(|c| c.as_str()).map(|c| c.to_string());
        if cursor.is_none() { break; }
    }
    Ok(out)
}

// ── OpenAPI ────────────────────────────────────────────────────────────

fn openapi_document(dir: &Path, server: &ToolServer) -> Result<Json, String> {
    let source = server.openapi.as_deref().unwrap_or("");
    let text = if source.starts_with("http://") || source.starts_with("https://") {
        let reply = http("GET", source, server, None, &[])?;
        if !(200..300).contains(&reply.status) { return Err(format!("the OpenAPI document answered HTTP {}", reply.status)); }
        reply.text
    } else {
        std::fs::read_to_string(dir.join(source)).map_err(|e| format!("cannot read the OpenAPI document {}: {}", source, e))?
    };
    serde_json::from_str::<Json>(&text).map_err(|_| format!("the OpenAPI document {} is not JSON (a YAML document is not read: convert it)", source))
}

/// `{"$ref": "#/components/…"}` → what it names (local references only).
fn deref<'a>(doc: &'a Json, v: &'a Json, depth: usize) -> &'a Json {
    if depth > 16 { return v; }
    match v.get("$ref").and_then(|r| r.as_str()).and_then(|r| r.strip_prefix("#/")) {
        Some(path) => {
            let mut cur = doc;
            for seg in path.split('/') {
                let seg = seg.replace("~1", "/").replace("~0", "~");
                match cur.get(&seg) { Some(next) => cur = next, None => return v }
            }
            deref(doc, cur, depth + 1)
        }
        None => v,
    }
}

fn schema_type(doc: &Json, schema: &Json) -> String {
    deref(doc, schema, 0).get("type").and_then(|t| t.as_str()).unwrap_or("any").to_string()
}

/// Every operation of the document that has an operationId: id → pin. The
/// pinned schema is what a call needs and nothing else: method, path, where
/// each parameter goes, the body's fields, and the same as an input schema.
fn openapi_operations(doc: &Json) -> HashMap<String, LockedTool> {
    let mut out = HashMap::new();
    let Some(paths) = doc.get("paths").and_then(|p| p.as_object()) else { return out };
    for (path, item) in paths {
        let item = deref(doc, item, 0);
        let shared: Vec<Json> = item.get("parameters").and_then(|p| p.as_array()).cloned().unwrap_or_default();
        for method in ["get", "post", "put", "patch", "delete"] {
            let Some(op) = item.get(method) else { continue };
            let Some(id) = op.get("operationId").and_then(|i| i.as_str()) else { continue };
            let mut properties = serde_json::Map::new();
            let mut required: Vec<String> = Vec::new();
            let mut places = serde_json::Map::new();
            let own: Vec<Json> = op.get("parameters").and_then(|p| p.as_array()).cloned().unwrap_or_default();
            for p in shared.iter().chain(own.iter()) {
                let p = deref(doc, p, 0);
                let (Some(name), Some(place)) = (p.get("name").and_then(|n| n.as_str()), p.get("in").and_then(|i| i.as_str())) else { continue };
                if !matches!(place, "path" | "query" | "header") { continue; }
                properties.insert(name.to_string(), json!({"type": p.get("schema").map(|s| schema_type(doc, s)).unwrap_or("string".to_string())}));
                places.insert(name.to_string(), json!(place));
                if place == "path" || p.get("required").and_then(|r| r.as_bool()) == Some(true) { required.push(name.to_string()); }
            }
            // a JSON object body: its fields are arguments; any other body is one argument, `body`
            let mut body_mode = "none";
            if let Some(body) = op.get("requestBody").map(|b| deref(doc, b, 0)) {
                let schema = body.get("content").and_then(|c| c.get("application/json")).and_then(|j| j.get("schema")).map(|s| deref(doc, s, 0));
                let body_required = body.get("required").and_then(|r| r.as_bool()) == Some(true);
                match schema.and_then(|s| s.get("properties")).and_then(|p| p.as_object()) {
                    Some(fields) if !fields.keys().any(|k| properties.contains_key(k)) => {
                        body_mode = "fields";
                        for (name, field) in fields {
                            properties.insert(name.clone(), json!({"type": schema_type(doc, field)}));
                            places.insert(name.clone(), json!("body"));
                        }
                        if body_required {
                            for r in schema.and_then(|s| s.get("required")).and_then(|r| r.as_array()).cloned().unwrap_or_default() {
                                if let Some(r) = r.as_str() { required.push(r.to_string()); }
                            }
                        }
                    }
                    _ => {
                        body_mode = "whole";
                        properties.insert("body".to_string(), json!({"type": schema.map(|s| schema_type(doc, s)).unwrap_or("any".to_string())}));
                        places.insert("body".to_string(), json!("body"));
                        if body_required { required.push("body".to_string()); }
                    }
                }
            }
            required.sort(); required.dedup();
            let schema = json!({
                "method": method.to_uppercase(), "path": path, "in": places, "body": body_mode,
                "type": "object", "properties": properties, "required": required,
            });
            let description = op.get("summary").or(op.get("description")).and_then(|d| d.as_str()).unwrap_or("");
            out.insert(id.to_string(), pin("openapi", description, &schema));
        }
    }
    out
}

fn openapi_base(doc: &Json, server: &ToolServer) -> Result<String, String> {
    let base = server.base_url.clone()
        .or_else(|| doc.get("servers").and_then(|s| s.get(0)).and_then(|s| s.get("url")).and_then(|u| u.as_str()).map(|u| u.to_string()))
        .unwrap_or_default();
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("no absolute server URL: the OpenAPI document names none — set `base_url = \"https://…\"` in the [tools.*] table".to_string());
    }
    Ok(base.trim_end_matches('/').to_string())
}

// ── what a server offers ───────────────────────────────────────────────

/// The URL a call to this server goes to (a prefix, for OpenAPI): what a
/// capability has to allow.
pub fn server_url(dir: &Path, server: &ToolServer) -> Option<String> {
    if let Some(u) = &server.mcp { return Some(u.clone()); }
    if let Some(b) = &server.base_url { return Some(b.clone()); }
    openapi_document(dir, server).ok().and_then(|doc| openapi_base(&doc, server).ok())
}

/// Ask a server for all its tools (the network, or the OpenAPI file).
pub fn fetch(dir: &Path, server: &ToolServer) -> Result<HashMap<String, LockedTool>, String> {
    match (&server.mcp, &server.openapi) {
        (Some(url), None) => mcp_tools(url, server),
        (None, Some(_)) => Ok(openapi_operations(&openapi_document(dir, server)?)),
        _ => Err("a [tools.*] table names one server: `mcp = \"https://…\"` or `openapi = \"file-or-url\"`".to_string()),
    }
}

// ── check ──────────────────────────────────────────────────────────────

fn soma_type(json_type: &str) -> &'static str {
    match json_type {
        "string" => "String", "integer" => "Int", "number" => "Float", "boolean" => "Bool",
        "array" => "List", "object" => "Map", _ => "Any",
    }
}

fn declared_type(p: &Param) -> String {
    match &p.ty.node {
        TypeExpr::Simple(n) => n.clone(),
        TypeExpr::Generic { name, .. } => name.clone(),
        _ => "Any".to_string(),
    }
}

/// Does the declaration fit the pinned contract? Every argument the tool
/// declares exists on the server with that type, and everything the server
/// requires is declared.
pub fn mismatch(tool: &ToolDecl, pinned: &LockedTool) -> Option<String> {
    let schema: Json = serde_json::from_str(&pinned.schema).ok()?;
    let empty = serde_json::Map::new();
    let props = schema.get("properties").and_then(|p| p.as_object()).unwrap_or(&empty);
    let offered = || if props.is_empty() { "no argument".to_string() } else {
        props.iter().map(|(k, v)| format!("{}: {}", k, soma_type(v.get("type").and_then(|t| t.as_str()).unwrap_or("any")))).collect::<Vec<_>>().join(", ")
    };
    for p in &tool.params {
        let Some(prop) = props.get(&p.name) else {
            return Some(format!("it declares `{}`, which the server's tool does not take (it takes {})", p.name, offered()));
        };
        let want = soma_type(prop.get("type").and_then(|t| t.as_str()).unwrap_or("any"));
        let have = declared_type(p);
        let fits = want == "Any" || have == "Any" || have == want || (want == "Float" && have == "Int");
        if !fits {
            return Some(format!("`{}` is declared {}, the server takes {} there", p.name, have, want));
        }
    }
    for r in schema.get("required").and_then(|r| r.as_array()).cloned().unwrap_or_default() {
        if let Some(r) = r.as_str() {
            if !tool.params.iter().any(|p| p.name == r) {
                return Some(format!("the server requires `{}`, which the tool does not declare (it takes {})", r, offered()));
            }
        }
    }
    None
}

pub struct BindingIssue { pub message: String, pub span: crate::ast::Span }

/// `soma check`, offline: soma.toml names the server, soma.lock pins the
/// tool, the declaration fits the pin, the capabilities allow the server.
pub fn check_program(program: &crate::ast::Program, manifest: Option<&Manifest>, dir: &Path) -> Vec<BindingIssue> {
    let mut issues = Vec::new();
    let mut lock: Option<Result<LockFile, String>> = None;
    for cell in crate::checker::names::collect_cells(program) {
        for (decl, tool) in bound_tools(cell) {
            let b = tool.binding.as_ref().unwrap();
            let mut fail = |message: String| issues.push(BindingIssue { message, span: decl.span });
            // (the parser gives a bound tool the handler that makes the call)
            let own_handler = cell.sections.iter().any(|s| matches!(&s.node, Section::OnSignal(on) if on.signal_name == tool.name
                && !matches!(on.body.as_slice(), [st] if matches!(&st.node, crate::ast::Statement::Return { value } if matches!(&value.node, crate::ast::Expr::FnCall { name, .. } if name == "_external_tool")))));
            if own_handler {
                fail(format!("tool '{}' is bound to the server `{}` and has a handler too: one of them answers — drop `[{}: …]` or the `on {}(…)` handler", tool.name, b.server, b.kind, tool.name));
                continue;
            }
            let Some(server) = manifest.and_then(|m| m.tools.get(&b.server)) else {
                fail(format!("tool '{}' is bound to `{}`, which soma.toml does not define — add `[tools.{}]` with `{} = \"…\"`", tool.name, b.server, b.server,
                    if b.kind == "mcp" { "mcp" } else { "openapi" }));
                continue;
            };
            let kind_ok = if b.kind == "mcp" { server.mcp.is_some() && server.openapi.is_none() } else { server.openapi.is_some() && server.mcp.is_none() };
            if !kind_ok {
                fail(format!("tool '{}' is bound with `{}:` but `[tools.{}]` in soma.toml {} — a table names one server, of the kind the tool says",
                    tool.name, b.kind, b.server, if server.mcp.is_some() && server.openapi.is_some() { "names both an `mcp` and an `openapi` server" } else if b.kind == "mcp" { "is an OpenAPI document" } else { "is an MCP server" }));
                continue;
            }
            let locked = lock.get_or_insert_with(|| LockFile::load(&dir.join("soma.lock")));
            let pinned = match locked {
                Ok(l) => l.tools.get(&lock_key(&b.server, &b.remote)).filter(|p| p.kind == b.kind),
                Err(e) => { fail(format!("tool '{}': {}", tool.name, e)); continue; }
            };
            let Some(pinned) = pinned else {
                fail(format!("tool '{}' is bound to `{}` on `{}`, which soma.lock does not pin — run `soma tools sync`: it asks the server for the tool's contract and writes it down", tool.name, b.remote, b.server));
                continue;
            };
            if let Some(why) = mismatch(tool, pinned) {
                fail(format!("tool '{}' does not fit `{}` on `{}` as pinned in soma.lock: {}", tool.name, b.remote, b.server, why));
                continue;
            }
            // a scoped tool that cannot reach its own server would be refused on every call
            if !tool.capabilities.is_empty() {
                if let Some(url) = server.mcp.clone().or_else(|| server.base_url.clone()) {
                    let probe = if b.kind == "mcp" { url.clone() } else { format!("{}/", url.trim_end_matches('/')) };
                    let reaches = crate::interpreter::builtins::url_matches_any(&probe, &tool.capabilities)
                        || tool.capabilities.iter().any(|c| c.strip_prefix("net:").unwrap_or(c).trim_end_matches('*').starts_with(url.trim_end_matches('/')));
                    if !reaches {
                        fail(format!("tool '{}' runs on {} but its capabilities {:?} do not allow that URL: every call would be refused — add a capability for the server", tool.name, crate::interpreter::builtins::http::redact_url(&url), tool.capabilities));
                    }
                }
            }
        }
    }
    issues
}

// ── run time ───────────────────────────────────────────────────────────

/// Contracts already held to their pin in this process.
static VERIFIED: Mutex<Option<std::collections::HashSet<String>>> = Mutex::new(None);

pub struct CallError { pub kind: &'static str, pub message: String }

fn tool_error(message: String) -> CallError { CallError { kind: "tool", message } }

/// Run an external tool: `args` is the JSON object of its arguments.
/// `allow` is asked for every URL before it is requested (the capability
/// check of the tool call in progress).
pub fn call(tool: &ToolDecl, args: serde_json::Map<String, Json>, allow: &dyn Fn(&str) -> Result<(), String>) -> Result<Json, CallError> {
    let b = tool.binding.as_ref().expect("an external tool");
    let dir = project_dir();
    let name = &tool.name;
    let manifest = Manifest::load(&dir.join("soma.toml")).map_err(|e| tool_error(format!("tool '{}': {}", name, e)))?;
    let server = manifest.tools.get(&b.server)
        .ok_or_else(|| tool_error(format!("tool '{}' is bound to `{}`, which soma.toml does not define", name, b.server)))?;
    let lock = LockFile::load(&dir.join("soma.lock")).map_err(|e| tool_error(format!("tool '{}': {}", name, e)))?;
    let key = lock_key(&b.server, &b.remote);
    let pinned = lock.tools.get(&key).filter(|p| p.kind == b.kind)
        .ok_or_else(|| tool_error(format!("tool '{}' is not pinned in soma.lock — run `soma tools sync`", name)))?;
    let drift = |now: Option<&LockedTool>| -> Result<(), CallError> {
        match now {
            Some(now) if now.sha256 == pinned.sha256 => Ok(()),
            Some(_) => Err(CallError { kind: "tool_schema", message: format!(
                "tool_schema: `{}` on `{}` no longer has the contract pinned in soma.lock (the server changed its arguments) — nothing was sent; run `soma tools sync`, read what changed, then `soma check`", b.remote, b.server) }),
            None => Err(CallError { kind: "tool_schema", message: format!(
                "tool_schema: `{}` no longer exists on `{}` — nothing was sent; run `soma tools sync`", b.remote, b.server) }),
        }
    };
    let verified_key = format!("{}|{}|{}", dir.display(), key, pinned.sha256);
    let verified = VERIFIED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).contains(&verified_key);
    let mark = || { VERIFIED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).insert(verified_key.clone()); };

    if let Some(url) = &server.mcp {
        allow(url).map_err(tool_error)?;
        if !verified {
            let offered = mcp_tools(url, server).map_err(|e| tool_error(format!("tool '{}': {}", name, e)))?;
            drift(offered.get(&b.remote))?;
            mark();
        }
        let result = mcp_request(url, server, "tools/call", json!({"name": b.remote, "arguments": Json::Object(args)}))
            .map_err(|e| tool_error(format!("tool '{}': {}", name, e)))?;
        let texts: Vec<String> = result.get("content").and_then(|c| c.as_array()).map(|items| items.iter()
            .filter(|i| i.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|i| i.get("text").and_then(|t| t.as_str()).map(|t| t.to_string())).collect()).unwrap_or_default();
        if result.get("isError").and_then(|e| e.as_bool()) == Some(true) {
            return Err(tool_error(format!("tool '{}' failed on the server: {}", name, if texts.is_empty() { "no detail".to_string() } else { texts.join("\n") })));
        }
        return Ok(match result.get("structuredContent") {
            Some(s) if !s.is_null() => s.clone(),
            _ => Json::String(texts.join("\n")),
        });
    }

    // OpenAPI
    let doc = openapi_document(&dir, server).map_err(|e| tool_error(format!("tool '{}': {}", name, e)))?;
    if !verified {
        drift(openapi_operations(&doc).get(&b.remote))?;
        mark();
    }
    let schema: Json = serde_json::from_str(&pinned.schema).map_err(|_| tool_error(format!("tool '{}': its pin in soma.lock is damaged — run `soma tools sync`", name)))?;
    let places = schema.get("in").and_then(|p| p.as_object()).cloned().unwrap_or_default();
    let mut path = schema.get("path").and_then(|p| p.as_str()).unwrap_or("/").to_string();
    let method = schema.get("method").and_then(|m| m.as_str()).unwrap_or("GET").to_string();
    let plain = |v: &Json| match v { Json::String(s) => s.clone(), other => other.to_string() };
    let encode = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>().replace('+', "%20");
    let mut query: Vec<String> = Vec::new();
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut body = serde_json::Map::new();
    let mut whole: Option<Json> = None;
    for (arg, value) in &args {
        if value.is_null() { continue; }
        match places.get(arg).and_then(|p| p.as_str()) {
            Some("path") => path = path.replace(&format!("{{{}}}", arg), &encode(&plain(value))),
            Some("query") => query.push(format!("{}={}", encode(arg), encode(&plain(value)))),
            Some("header") => headers.push((arg.clone(), plain(value))),
            Some("body") if schema.get("body").and_then(|b| b.as_str()) == Some("whole") => whole = Some(value.clone()),
            Some("body") => { body.insert(arg.clone(), value.clone()); }
            _ => {}
        }
    }
    if path.contains('{') {
        return Err(tool_error(format!("tool '{}': the path `{}` still has an unfilled parameter (an argument was `()`)", name, path)));
    }
    let mut url = format!("{}{}", openapi_base(&doc, server).map_err(|e| tool_error(format!("tool '{}': {}", name, e)))?, path);
    if !query.is_empty() { url.push('?'); url.push_str(&query.join("&")); }
    allow(&url).map_err(tool_error)?;
    let payload = match schema.get("body").and_then(|b| b.as_str()) {
        Some("whole") => whole,
        Some("fields") => Some(Json::Object(body)),
        _ => None,
    };
    let reply = http(&method, &url, server, payload.as_ref(), &headers).map_err(|e| tool_error(format!("tool '{}': {}", name, e)))?;
    if !(200..300).contains(&reply.status) {
        return Err(tool_error(format!("tool '{}': the server answered HTTP {}", name, reply.status)));
    }
    let text = reply.text.trim();
    Ok(if text.is_empty() { Json::Null } else { serde_json::from_str::<Json>(text).unwrap_or(Json::String(reply.text.clone())) })
}

// ── `soma tools sync` ──────────────────────────────────────────────────

/// Pin every bound tool of the program. Returns the report lines, or the
/// reasons nothing was written.
pub fn sync(program: &crate::ast::Program, dir: &Path) -> Result<Vec<String>, Vec<String>> {
    let manifest = Manifest::load(&dir.join("soma.toml")).map_err(|e| vec![e])?;
    let mut lock = LockFile::load(&dir.join("soma.lock")).map_err(|e| vec![e])?;
    let mut wanted: Vec<(String, String, String, String)> = Vec::new();   // (tool, kind, server, remote)
    for cell in crate::checker::names::collect_cells(program) {
        for (_, tool) in bound_tools(cell) {
            let b = tool.binding.as_ref().unwrap();
            let entry = (tool.name.clone(), b.kind.clone(), b.server.clone(), b.remote.clone());
            if !wanted.contains(&entry) { wanted.push(entry); }
        }
    }
    if wanted.is_empty() {
        return Ok(vec!["no external tool: no `tool` of this program is bound with `[mcp: …]` or `[openapi: …]`".to_string()]);
    }
    let mut errors = Vec::new();
    let mut report = Vec::new();
    let mut offered: HashMap<String, Result<HashMap<String, LockedTool>, String>> = HashMap::new();
    let mut fresh: Vec<(String, LockedTool)> = Vec::new();
    for (tool, kind, server_name, remote) in &wanted {
        let Some(server) = manifest.tools.get(server_name) else {
            errors.push(format!("tool '{}': soma.toml has no `[tools.{}]`", tool, server_name));
            continue;
        };
        let tools = offered.entry(server_name.clone()).or_insert_with(|| fetch(dir, server));
        match tools {
            Err(e) => errors.push(format!("tool '{}': `{}` — {}", tool, server_name, e)),
            Ok(tools) => match tools.get(remote).filter(|t| &t.kind == kind) {
                None => {
                    let mut names: Vec<&String> = tools.keys().collect();
                    names.sort();
                    errors.push(format!("tool '{}': `{}` offers no {} `{}` (it offers: {})", tool, server_name,
                        if kind == "mcp" { "tool" } else { "operation with operationId" }, remote,
                        if names.is_empty() { "nothing".to_string() } else { names.iter().take(30).map(|n| n.as_str()).collect::<Vec<_>>().join(", ") }));
                }
                Some(now) => {
                    let key = lock_key(server_name, remote);
                    report.push(match lock.tools.get(&key) {
                        None => format!("pinned   {}  {}", key, &now.sha256[..12]),
                        Some(old) if old == now => format!("same     {}  {}", key, &now.sha256[..12]),
                        Some(old) => format!("CHANGED  {}  {} -> {}\n    was: {}\n    now: {}", key, &old.sha256[..12], &now.sha256[..12], old.schema, now.schema),
                    });
                    fresh.push((key, now.clone()));
                }
            },
        }
    }
    if !errors.is_empty() { return Err(errors); }
    // (a pin no tool of THIS program names stays: another program here may use it)
    for (k, t) in fresh { lock.tools.insert(k, t); }
    lock.save(&dir.join("soma.lock")).map_err(|e| vec![e])?;
    Ok(report)
}
