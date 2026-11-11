//! `soma embed <file>` (3.0, C9): a program as a library of another one.
//!
//! The host (Python, TypeScript, anything that can start a process) writes
//! one JSON object per line on stdin and reads one per line on stdout:
//!
//!     → {"id": 1, "call": "refund", "args": ["o1", 40]}
//!     ← {"id": 1, "ok": true, "value": {"paid": 40}}
//!     → {"id": 2, "call": "Ledger.debit", "args": {"amount": 900}}
//!     ← {"id": 2, "ok": false, "error": {"kind": "requires", "message": "…"}}
//!
//! The first line out is `{"ready": true, …}` with the handlers and their
//! typed parameters (or `{"ready": false, "errors": …}` and exit 1 when the
//! program fails `soma check`). Each call is one atomic invocation, with the
//! program's data beside it, exactly as under `soma run`. Values keep their
//! types: an Int past 2^53 is `{"__soma_int__": "digits"}`, a variant is
//! `{"_type", "_variant", …}`; `print()` goes to stderr.
//!
//! The clients are `packages/python` and `packages/npm`.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use crate::interpreter::{self, Value};
use crate::registry::Registry;
use crate::{ast, checker, runtime};
use super::{read_source, lex_with_location, parse_collecting, resolve_imports, load_meta_cells_from_program};

fn to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Object(o) if o.len() == 1 && o.get("__soma_int__").is_some_and(|d| d.is_string()) => interpreter::record_log::json_to_value(v),
        // a record or a variant of the program's own types (the handler's
        // parameter type decides whether it is one)
        serde_json::Value::Object(o) if o.get("_type").is_some_and(|t| t.is_string()) && o.get("_variant").is_some_and(|t| t.is_string()) => {
            let fields = match o.get("_values") {
                Some(serde_json::Value::Array(vs)) => interpreter::VariantValue::Tuple(vs.iter().map(to_value).collect()),
                _ => {
                    let named: indexmap::IndexMap<String, Value> = o.iter().filter(|(k, _)| !matches!(k.as_str(), "_type" | "_variant" | "_values"))
                        .map(|(k, x)| (k.clone(), to_value(x))).collect();
                    if named.is_empty() { interpreter::VariantValue::Unit } else { interpreter::VariantValue::Struct(named) }
                }
            };
            Value::Variant { type_name: o["_type"].as_str().unwrap_or("").to_string(), variant: o["_variant"].as_str().unwrap_or("").to_string(), fields }
        }
        serde_json::Value::Object(o) => Value::Map(o.iter().map(|(k, x)| (k.clone(), to_value(x))).collect()),
        serde_json::Value::Array(a) => Value::List(a.iter().map(to_value).collect()),
        other => interpreter::record_log::json_to_value(other),
    }
}

fn to_json(v: &Value) -> serde_json::Value {
    match v {
        // a response map: its status and body, without the runtime's mark
        Value::Map(m) if interpreter::is_http_response(v) =>
            serde_json::Value::Object(m.iter().filter(|(k, _)| k.as_str() != "_response").map(|(k, x)| (k.clone(), to_json(x))).collect()),
        Value::Map(m) => serde_json::Value::Object(m.iter().map(|(k, x)| (k.clone(), to_json(x))).collect()),
        Value::List(items) => serde_json::Value::Array(items.iter().map(to_json).collect()),
        // an Int a JavaScript number cannot hold exactly is tagged, like one past 64 bits
        Value::Int(n) if n.to_i64().is_some_and(|i| i.unsigned_abs() > (1u64 << 53) - 1) => serde_json::json!({"__soma_int__": n.to_string()}),
        Value::Variant { type_name, variant, fields } => {
            let mut o = serde_json::Map::new();
            o.insert("_type".to_string(), serde_json::json!(type_name));
            o.insert("_variant".to_string(), serde_json::json!(variant));
            match fields {
                interpreter::VariantValue::Unit => {}
                interpreter::VariantValue::Tuple(vs) => { o.insert("_values".to_string(), serde_json::Value::Array(vs.iter().map(to_json).collect())); }
                interpreter::VariantValue::Struct(fs) => { for (k, x) in fs { o.insert(k.clone(), to_json(x)); } }
            }
            serde_json::Value::Object(o)
        }
        other => interpreter::record_log::value_to_json(other),
    }
}

fn type_name(t: &ast::TypeExpr) -> String { super::describe::format_type(t) }

/// The handlers a host may call: cell → handler → parameters and result.
fn interface(program: &ast::Program) -> serde_json::Value {
    let mut cells = serde_json::Map::new();
    for cell in &program.cells {
        if !matches!(cell.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) { continue; }
        let returns = |name: &str| cell.node.sections.iter().find_map(|s| match &s.node {
            ast::Section::Face(f) => f.declarations.iter().find_map(|d| match &d.node {
                ast::FaceDecl::Signal(sig) if sig.name == name => sig.return_type.as_ref().map(|t| type_name(&t.node)),
                ast::FaceDecl::Tool(t) if t.name == name => t.return_type.as_ref().map(|t| type_name(&t.node)),
                _ => None,
            }),
            _ => None,
        });
        let mut handlers = serde_json::Map::new();
        for section in &cell.node.sections {
            let ast::Section::OnSignal(on) = &section.node else { continue };
            handlers.insert(on.signal_name.clone(), serde_json::json!({
                "params": on.params.iter().map(|p| serde_json::json!({"name": p.name, "type": type_name(&p.ty.node)})).collect::<Vec<_>>(),
                "returns": returns(&on.signal_name),
            }));
        }
        cells.insert(cell.node.name.clone(), serde_json::Value::Object(handlers));
    }
    serde_json::Value::Object(cells)
}

fn fail(id: &serde_json::Value, kind: &str, message: String) -> serde_json::Value {
    serde_json::json!({"id": id, "ok": false, "error": {"kind": kind, "message": message}})
}

pub fn cmd_embed(path: &PathBuf, registry: &mut Registry) {
    // stdout is the protocol: whatever the program prints goes to stderr
    super::JSON_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
    interpreter::set_edition_beside(path);
    let out = std::io::stdout();
    let send = |v: serde_json::Value| {
        let mut o = out.lock();
        let _ = writeln!(o, "{}", v);
        let _ = o.flush();
    };
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let (mut program, syntax) = parse_collecting(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    load_meta_cells_from_program(&program, registry, path);
    {
        let mut chk = checker::Checker::new(registry);
        chk.source = Some((file_str.clone(), source.clone()));
        chk.check(&program);
        for e in syntax.iter().rev() {
            chk.errors.insert(0, checker::CheckError::Static { kind: "syntax", message: e.to_string(), span: e.span().unwrap_or(ast::Span { start: 0, end: 0 }) });
        }
        if chk.has_errors() {
            let report: serde_json::Value = serde_json::from_str(&chk.report_json()).unwrap_or(serde_json::json!({}));
            send(serde_json::json!({"ready": false, "file": file_str, "errors": report.get("errors").cloned().unwrap_or(serde_json::json!([]))}));
            std::process::exit(1);
        }
    }

    let mut interp = interpreter::Interpreter::new(&program);
    for cell in &program.cells {
        if !matches!(cell.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) { continue; }
        for section in &cell.node.sections {
            if let ast::Section::Memory(ref mem) = section.node {
                let mut slots = std::collections::HashMap::new();
                for slot in &mem.slots {
                    let props: Vec<String> = slot.node.properties.iter().map(|p| p.node.name().to_string()).collect();
                    slots.insert(slot.node.name.clone(), runtime::storage::resolve_backend_from_registry(&cell.node.name, &slot.node.name, &props, registry));
                }
                interp.set_storage(&cell.node.name, &slots);
            }
        }
    }
    interpreter::PERSIST_MACHINES.store(true, std::sync::atomic::Ordering::Relaxed);
    interp.ensure_state_machine_storage();
    interp.source_file = Some(file_str.clone());
    interp.source_text = Some(source.clone());
    let toml = path.parent().unwrap_or(std::path::Path::new(".")).join("soma.toml");
    if let Ok(manifest) = crate::pkg::manifest::Manifest::load(&toml) {
        interp.agent_config = Some(manifest.agent);
        interp.agent_models = manifest.models;
    }

    let cells = interface(&program);
    send(serde_json::json!({"ready": true, "soma": env!("CARGO_PKG_VERSION"), "file": file_str, "cells": cells}));

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() { continue; }
        let request: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => { send(fail(&serde_json::Value::Null, "protocol", format!("a request is one JSON object per line: {}", e))); continue; }
        };
        let id = request.get("id").cloned().unwrap_or(serde_json::Value::Null);
        if request.get("close").is_some() { send(serde_json::json!({"id": id, "ok": true, "value": null})); break; }
        let Some(target) = request.get("call").and_then(|c| c.as_str()) else {
            send(fail(&id, "protocol", "a request names a handler: {\"id\": 1, \"call\": \"handler\", \"args\": [...]}".to_string()));
            continue;
        };
        // `handler`, or `Cell.handler`
        let (cell, handler): (Option<&str>, &str) = match target.split_once('.') { Some((c, h)) => (Some(c), h), None => (None, target) };
        let owners: Vec<&str> = cells.as_object().map(|m| m.iter()
            .filter(|(c, hs)| cell.map_or(true, |want| want == c.as_str()) && hs.get(handler).is_some())
            .map(|(c, _)| c.as_str()).collect()).unwrap_or_default();
        let owner = match owners.as_slice() {
            [one] => one.to_string(),
            [] => { send(fail(&id, "no_handler", format!("no handler `{}` — the `ready` line lists them", target))); continue; }
            many => { send(fail(&id, "no_handler", format!("`{}` is a handler of {}: name the cell (`{}.{}`)", handler, many.join(" and "), many[0], handler))); continue; }
        };
        let params: Vec<String> = cells[&owner][handler]["params"].as_array()
            .map(|ps| ps.iter().filter_map(|p| p["name"].as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
        // positional (a list) or by name (an object)
        let args: Vec<Value> = match request.get("args") {
            None | Some(serde_json::Value::Null) => Vec::new(),
            Some(serde_json::Value::Array(a)) => a.iter().map(to_value).collect(),
            Some(serde_json::Value::Object(named)) => {
                if let Some(unknown) = named.keys().find(|k| !params.contains(k)) {
                    send(fail(&id, "type", format!("{}() has no parameter `{}` (it takes {})", handler, unknown, if params.is_empty() { "none".to_string() } else { params.join(", ") })));
                    continue;
                }
                if let Some(missing) = params.iter().find(|p| !named.contains_key(*p)) {
                    send(fail(&id, "type", format!("{}() needs `{}` (it takes {})", handler, missing, params.join(", "))));
                    continue;
                }
                params.iter().map(|p| to_value(&named[p])).collect()
            }
            Some(_) => { send(fail(&id, "protocol", "`args` is a list (by position) or an object (by parameter name)".to_string())); continue; }
        };
        interp.current_depth = 0;
        // each call is an invocation of its own: budget and count start fresh
        interp.agent_token_budget = 0;
        interp.agent_budget_zero = false;
        interp.agent_tokens_used = 0;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| interp.call_signal(&owner, handler, args)));
        send(match outcome {
            Ok(Ok(value)) => serde_json::json!({"id": id, "ok": true, "value": to_json(&value)}),
            Ok(Err(e)) => fail(&id, &e.kind(), e.to_string()),
            Err(_) => fail(&id, "internal", "the interpreter stopped on this call — the program's data is as it was before it".to_string()),
        });
    }
}
