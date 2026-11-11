//! `soma lsp` (3.0): a language server over stdio.
//!
//! What it does: diagnostics when a file is opened or saved — exactly what
//! `soma check --json` reports for the file on disk, codes and fixes
//! included — and whole-document formatting (`soma fmt`). What it does not
//! do yet: diagnostics for unsaved text, hover, completion, go-to-definition.

use std::collections::HashMap;
use std::io::{BufRead, Read, Write};

use serde_json::{json, Value};

fn send(out: &mut impl Write, msg: &Value) {
    let body = msg.to_string();
    let _ = write!(out, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = out.flush();
}

fn read_message(input: &mut impl BufRead) -> Option<Value> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line).ok()? == 0 { return None; }
        let line = line.trim_end();
        if line.is_empty() { break; }
        if let Some(v) = line.strip_prefix("Content-Length:") { length = v.trim().parse().ok(); }
    }
    let mut body = vec![0u8; length?];
    input.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn path_of(uri: &str) -> Option<std::path::PathBuf> {
    let p = uri.strip_prefix("file://")?;
    // percent-decoding of the bytes a path can hold
    let bytes = p.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&p[i + 1..i + 3], 16) { out.push(b); i += 3; continue; }
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(std::path::PathBuf::from(String::from_utf8(out).ok()?))
}

/// One check diagnostic as an LSP one. Positions: check counts lines and
/// columns from 1, in characters; LSP from 0 (UTF-16 units — the same for
/// the BMP text source files are made of).
fn diagnostic(e: &Value, severity: u8) -> Value {
    let line = e["line"].as_u64().unwrap_or(1).saturating_sub(1);
    let col = e["col"].as_u64().unwrap_or(1).saturating_sub(1);
    let end = e["source_line"].as_str().map_or(col + 1, |l| (l.chars().count() as u64).max(col + 1));
    let mut message = e["message"].as_str().unwrap_or("").to_string();
    if let Some(fix) = e["fix"].as_str() {
        if !fix.is_empty() && !message.contains(fix) { message.push_str(&format!("\nfix: {fix}")); }
    }
    let mut d = json!({
        "range": {"start": {"line": line, "character": col}, "end": {"line": line, "character": end}},
        "severity": severity,
        "source": "soma",
        "message": message,
    });
    if let Some(code) = e["code"].as_str() { d["code"] = json!(code); }
    d
}

/// `soma check --json` on the file as saved, in a fresh process (a check
/// that fails to load exits; the server must not).
fn diagnostics_for(path: &std::path::Path) -> Vec<Value> {
    let Ok(exe) = std::env::current_exe() else { return Vec::new() };
    let Ok(out) = std::process::Command::new(exe).arg("check").arg("--json").arg(path)
        .stdin(std::process::Stdio::null()).output() else { return Vec::new() };
    let Ok(doc) = serde_json::from_slice::<Value>(&out.stdout) else { return Vec::new() };
    let file = path.display().to_string();
    // an imported file's diagnostics belong to that file
    let here = |e: &&Value| e["file"].as_str().map_or(true, |f| f == file || std::path::Path::new(f).file_name() == path.file_name());
    let mut all: Vec<Value> = doc["errors"].as_array().into_iter().flatten().filter(here).map(|e| diagnostic(e, 1)).collect();
    all.extend(doc["warnings"].as_array().into_iter().flatten().filter(here).map(|e| diagnostic(e, 2)));
    all
}

pub fn cmd_lsp() {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut docs: HashMap<String, String> = HashMap::new();
    let mut shutdown = false;
    while let Some(msg) = read_message(&mut input) {
        let method = msg["method"].as_str().unwrap_or("");
        let id = msg.get("id").cloned();
        let uri = msg["params"]["textDocument"]["uri"].as_str().unwrap_or("").to_string();
        let publish = |out: &mut std::io::StdoutLock, uri: &str| {
            if let Some(path) = path_of(uri) {
                send(out, &json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
                    "params": {"uri": uri, "diagnostics": diagnostics_for(&path)}}));
            }
        };
        match method {
            "initialize" => send(&mut out, &json!({"jsonrpc": "2.0", "id": id, "result": {
                "capabilities": {
                    // full text on change (kept for formatting); diagnostics on open and save
                    "textDocumentSync": {"openClose": true, "change": 1, "save": {"includeText": false}},
                    "documentFormattingProvider": true,
                },
                "serverInfo": {"name": "soma", "version": env!("CARGO_PKG_VERSION")},
            }})),
            "textDocument/didOpen" => {
                docs.insert(uri.clone(), msg["params"]["textDocument"]["text"].as_str().unwrap_or("").to_string());
                publish(&mut out, &uri);
            }
            "textDocument/didChange" => {
                if let Some(text) = msg["params"]["contentChanges"].as_array().and_then(|c| c.last()).and_then(|c| c["text"].as_str()) {
                    docs.insert(uri.clone(), text.to_string());
                }
            }
            "textDocument/didSave" => publish(&mut out, &uri),
            "textDocument/didClose" => {
                docs.remove(&uri);
                send(&mut out, &json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": uri, "diagnostics": []}}));
            }
            "textDocument/formatting" => {
                let edits = match docs.get(&uri).map(|text| (text, super::fmt::format_checked(text))) {
                    Some((text, Some(Ok(formatted)))) if formatted != *text => {
                        let lines = text.split('\n').count() as u64;
                        json!([{"range": {"start": {"line": 0, "character": 0}, "end": {"line": lines, "character": 0}}, "newText": formatted}])
                    }
                    _ => json!([]),
                };
                send(&mut out, &json!({"jsonrpc": "2.0", "id": id, "result": edits}));
            }
            "shutdown" => {
                shutdown = true;
                send(&mut out, &json!({"jsonrpc": "2.0", "id": id, "result": null}));
            }
            "exit" => std::process::exit(if shutdown { 0 } else { 1 }),
            _ => {
                // a request this server does not implement gets an answer, not silence
                if let Some(id) = id {
                    send(&mut out, &json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("soma lsp does not implement {method}")}}));
                }
            }
        }
    }
}
