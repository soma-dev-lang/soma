//! `soma describe` — machine-readable cell description.
//! Outputs structured JSON that agents can read, understand, and act on.

use std::path::PathBuf;
use crate::ast::*;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports};

pub fn cmd_describe(path: &PathBuf) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);

    let mut cells = Vec::new();

    for cell in &program.cells {
        if cell.node.kind != CellKind::Cell { continue; }
        cells.push(describe_cell(&cell.node));
    }

    let output = serde_json::json!({
        "file": file_str,
        "cells": cells,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

fn describe_cell(cell: &CellDef) -> serde_json::Value {
    let mut signals = Vec::new();
    let mut memory = Vec::new();
    let mut state_machines = Vec::new();
    let mut scale = serde_json::Value::Null;
    let mut every_intervals = Vec::new();
    let mut has_request = false;
    let mut routes = Vec::new();

    for section in &cell.sections {
        match &section.node {
            Section::OnSignal(on) => {
                let params: Vec<serde_json::Value> = on.params.iter()
                    .map(|p| serde_json::json!({
                        "name": p.name,
                        "type": format_type(&p.ty.node),
                    }))
                    .collect();

                let mut sig = serde_json::json!({
                    "name": on.signal_name,
                    "params": params,
                });

                if on.signal_name == "request" {
                    has_request = true;
                }

                // Detect properties like [native]
                if !on.properties.is_empty() {
                    sig["properties"] = serde_json::json!(on.properties);
                }

                signals.push(sig);
            }
            Section::Memory(mem) => {
                for slot in &mem.slots {
                    let props: Vec<String> = slot.node.properties.iter()
                        .map(|p| p.node.name().to_string())
                        .collect();
                    memory.push(serde_json::json!({
                        "name": slot.node.name,
                        "type": format_type(&slot.node.ty.node),
                        "properties": props,
                    }));
                }
            }
            Section::State(sm) => {
                let transitions: Vec<serde_json::Value> = sm.transitions.iter()
                    .map(|t| serde_json::json!({
                        "from": t.node.from,
                        "to": t.node.to,
                        "guarded": t.node.guard.is_some(),
                    }))
                    .collect();

                let states: Vec<String> = {
                    let mut s = std::collections::HashSet::new();
                    s.insert(sm.initial.clone());
                    for t in &sm.transitions {
                        if t.node.from != "*" { s.insert(t.node.from.clone()); }
                        s.insert(t.node.to.clone());
                    }
                    let mut v: Vec<String> = s.into_iter().collect();
                    v.sort();
                    v
                };

                state_machines.push(serde_json::json!({
                    "name": sm.name,
                    "initial": sm.initial,
                    "states": states,
                    "transitions": transitions,
                }));
            }
            Section::Scale(sc) => {
                let mut s = serde_json::json!({
                    "replicas": sc.replicas,
                    "consistency": format!("{}", sc.consistency),
                    "tolerance": sc.tolerance,
                });
                if let Some(ref shard) = sc.shard {
                    s["shard"] = serde_json::json!(shard);
                }
                if let Some(cpu) = sc.cpu {
                    s["cpu"] = serde_json::json!(cpu);
                }
                if let Some(ref mem) = sc.memory {
                    s["memory"] = serde_json::json!(mem);
                }
                if let Some(ref disk) = sc.disk {
                    s["disk"] = serde_json::json!(disk);
                }
                scale = s;
            }
            Section::Every(ev) => {
                every_intervals.push(serde_json::json!({
                    "interval_ms": ev.interval_ms,
                }));
            }
            Section::Face(face) => {
                for decl in &face.declarations {
                    match &decl.node {
                        FaceDecl::Signal(sig) => {
                            let params: Vec<serde_json::Value> = sig.params.iter()
                                .map(|p| serde_json::json!({"name": p.name, "type": format_type(&p.ty.node)}))
                                .collect();
                            let mut entry = serde_json::json!({
                                "name": sig.name,
                                "kind": "signal",
                                "params": params,
                            });
                            if let Some(ref ret) = sig.return_type {
                                entry["return_type"] = serde_json::json!(format_type(&ret.node));
                            }
                            // Don't duplicate — face signals are the contract
                        }
                        FaceDecl::Given(g) => {
                            // requirements
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    // Detect HTTP routes from request handler body (heuristic: look for path comparisons)
    // This is a best-effort analysis for describe output
    let source_text = std::fs::read_to_string(
        std::env::current_dir().unwrap_or_default().join("app.cell")
    ).unwrap_or_default();
    for line in source_text.lines() {
        let trimmed = line.trim();
        if trimmed.contains("path ==") || trimmed.contains("path==\"") {
            if let Some(start) = trimmed.find('"') {
                if let Some(end) = trimmed[start+1..].find('"') {
                    routes.push(trimmed[start+1..start+1+end].to_string());
                }
            }
        }
    }

    let mut result = serde_json::json!({
        "name": cell.name,
        "signals": signals,
        "memory": memory,
    });

    if !state_machines.is_empty() {
        result["state_machines"] = serde_json::json!(state_machines);
    }
    if scale != serde_json::Value::Null {
        result["scale"] = scale;
    }
    if !every_intervals.is_empty() {
        result["scheduled"] = serde_json::json!(every_intervals);
    }
    if has_request {
        result["web"] = serde_json::json!(true);
        if !routes.is_empty() {
            result["routes"] = serde_json::json!(routes);
        }
    }

    result
}

fn format_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Simple(name) => name.clone(),
        TypeExpr::Generic { name, args } => {
            let arg_strs: Vec<String> = args.iter().map(|a| format_type(&a.node)).collect();
            format!("{}<{}>", name, arg_strs.join(", "))
        }
        TypeExpr::CellRef { cell, member } => format!("{}.{}", cell, member),
        _ => "unknown".to_string(),
    }
}
