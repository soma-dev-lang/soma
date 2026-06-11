//! `soma describe` — machine-readable cell description.
//! Outputs structured JSON that agents can read, understand, and act on.
//! An agent seeing this output should fully understand the cell without reading
//! the source.

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
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) { continue; }
        cells.push(describe_cell(&cell.node, &source));
    }

    let imports: Vec<&str> = program.imports.iter().map(|s| s.as_str()).collect();

    let output = serde_json::json!({
        "file": file_str,
        "imports": imports,
        "cells": cells,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

fn describe_cell(cell: &CellDef, source: &str) -> serde_json::Value {
    let mut handlers = Vec::new();
    let mut memory_slots = Vec::new();
    let mut state_machines = Vec::new();
    let mut scale = serde_json::Value::Null;
    let mut scheduled = Vec::new();
    let mut has_request = false;
    let mut routes = Vec::new();

    // Face section data
    let mut face_signals = Vec::new();
    let mut face_promises = Vec::new();
    let mut face_tools: Vec<serde_json::Value> = Vec::new();

    for section in &cell.sections {
        let section_line = span_to_line(source, section.span.start);

        match &section.node {
            Section::OnSignal(on) => {
                let params: Vec<serde_json::Value> = on.params.iter()
                    .map(|p| serde_json::json!({
                        "name": p.name,
                        "type": format_type(&p.ty.node),
                    }))
                    .collect();

                let is_public = !on.signal_name.starts_with('_');

                let mut handler = serde_json::json!({
                    "name": on.signal_name,
                    "params": params,
                    "is_public": is_public,
                    "line": section_line,
                });

                if on.signal_name == "request" {
                    has_request = true;
                }

                if !on.properties.is_empty() {
                    handler["properties"] = serde_json::json!(on.properties);
                }

                handlers.push(handler);
            }
            Section::Memory(mem) => {
                for slot in &mem.slots {
                    let props: Vec<String> = slot.node.properties.iter()
                        .map(|p| p.node.name().to_string())
                        .collect();

                    let invariants: Vec<String> = mem.invariants.iter()
                        .map(|inv| format_expr(&inv.node))
                        .collect();

                    memory_slots.push(serde_json::json!({
                        "name": slot.node.name,
                        "type": format_type(&slot.node.ty.node),
                        "properties": props,
                        "invariants": invariants,
                    }));
                }
            }
            Section::State(sm) => {
                let transitions: Vec<serde_json::Value> = sm.transitions.iter()
                    .map(|t| serde_json::json!({
                        "from": t.node.from,
                        "to": t.node.to,
                        "has_guard": t.node.guard.is_some(),
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
                scheduled.push(serde_json::json!({
                    "kind": "every",
                    "interval_ms": ev.interval_ms,
                }));
            }
            Section::After(af) => {
                scheduled.push(serde_json::json!({
                    "kind": "after",
                    "delay_ms": af.interval_ms,
                }));
            }
            Section::Face(face) => {
                for decl in &face.declarations {
                    match &decl.node {
                        FaceDecl::Signal(sig) => {
                            let params: Vec<String> = sig.params.iter()
                                .map(|p| format!("{}: {}", p.name, format_type(&p.ty.node)))
                                .collect();
                            let mut entry = serde_json::json!({
                                "name": sig.name,
                                "params": params,
                            });
                            if let Some(ref ret) = sig.return_type {
                                entry["returns"] = serde_json::json!(format_type(&ret.node));
                            }
                            face_signals.push(entry);
                        }
                        FaceDecl::Promise(p) => {
                            face_promises.push(format_constraint(&p.constraint.node));
                        }
                        FaceDecl::Given(_g) => {
                            // given declarations are requirements, not part of signals/promises
                        }
                        FaceDecl::Await(_a) => {}
                        FaceDecl::Tool(tool) => {
                            let params: Vec<String> = tool.params.iter()
                                .map(|p| format!("{}: {}", p.name, format_type(&p.ty.node)))
                                .collect();
                            let mut t = serde_json::json!({
                                "name": tool.name,
                                "params": params,
                            });
                            if let Some(ref ret) = tool.return_type {
                                t["returns"] = serde_json::json!(format_type(&ret.node));
                            }
                            if let Some(ref desc) = tool.description {
                                t["description"] = serde_json::json!(desc);
                            }
                            if !tool.capabilities.is_empty() {
                                t["capabilities"] = serde_json::json!(tool.capabilities);
                            }
                            face_tools.push(t);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Detect HTTP routes from request handler body (heuristic: look for path comparisons)
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
        "handlers": handlers,
        "memory": memory_slots,
    });

    if !state_machines.is_empty() {
        result["state_machines"] = serde_json::json!(state_machines);
    }
    if scale != serde_json::Value::Null {
        result["scale"] = scale;
    }
    if !scheduled.is_empty() {
        result["scheduled"] = serde_json::json!(scheduled);
    }
    if !face_signals.is_empty() || !face_promises.is_empty() || !face_tools.is_empty() {
        let mut face = serde_json::json!({
            "signals": face_signals,
            "promises": face_promises,
        });
        if !face_tools.is_empty() {
            face["tools"] = serde_json::json!(face_tools);
        }
        result["face"] = face;
    }
    if cell.kind == CellKind::Agent {
        result["kind"] = serde_json::json!("agent");
    }
    if has_request {
        result["web"] = serde_json::json!(true);
        if !routes.is_empty() {
            result["routes"] = serde_json::json!(routes);
        }
    }

    result
}

// ── `soma describe --builtins` ───────────────────────────────────────

/// Print the builtin registry, grouped by category.
/// Text mode is for humans; --json emits the table as a JSON array.
pub fn cmd_describe_builtins(json: bool) {
    use crate::interpreter::builtins::registry;

    if json {
        let entries: Vec<serde_json::Value> = registry::BUILTINS.iter()
            .map(|b| serde_json::json!({
                "name": b.name,
                "category": b.category,
                "signature": b.signature,
                "brief": b.brief,
                "deterministic": b.deterministic,
            }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&entries).unwrap());
        return;
    }

    let nondet_count = registry::BUILTINS.iter().filter(|b| !b.deterministic).count();
    println!("Soma builtins — {} total, {} nondeterministic (✗ = replay divergence source)",
        registry::BUILTINS.len(), nondet_count);
    println!("note: 'deterministic' here means NOT in the replay-divergence set tracked by");
    println!("`soma replay` — it is not a purity claim (think/http_*/read_* have effects).");

    for cat in registry::categories() {
        println!("\n── {} ─────", cat);
        for b in registry::BUILTINS.iter().filter(|b| b.category == cat) {
            let marker = if b.deterministic { " " } else { "✗" };
            println!("  {} {}", marker, b.signature);
            println!("      {}", b.brief);
        }
    }
}

// ── `soma describe <file.cell> --faces` ──────────────────────────────

/// Token-cheap contract pack: for each cell, ONLY its contract — face
/// signals, tools, promises, memory slots, state machines, and sum-type
/// declarations. No handler bodies. Paste this into an agent's context
/// before writing a new cell against these contracts.
pub fn cmd_describe_faces(path: &PathBuf, json: bool) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);

    if json {
        let mut out = Vec::new();
        for cell in &program.cells {
            match cell.node.kind {
                CellKind::Cell | CellKind::Agent | CellKind::Type => {
                    out.push(face_json(&cell.node));
                }
                _ => {}
            }
        }
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "file": file_str,
            "cells": out,
        })).unwrap());
        return;
    }

    println!("# {} — contracts only (no handler bodies)", file_str);

    // Sum types first: they are the vocabulary the cells speak.
    for cell in &program.cells {
        if cell.node.kind != CellKind::Type { continue; }
        for section in &cell.node.sections {
            if let Section::Variants(v) = &section.node {
                println!("type {} = {}", cell.node.name, format_variants(v));
            }
        }
    }

    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) { continue; }
        print_cell_face(&cell.node);
    }
}

fn print_cell_face(cell: &CellDef) {
    let kind = if cell.kind == CellKind::Agent { "agent" } else { "cell" };
    println!("\n{} {}", kind, cell.name);

    let mut has_face = false;
    for section in &cell.sections {
        match &section.node {
            Section::Face(face) => {
                has_face = true;
                for decl in &face.declarations {
                    match &decl.node {
                        FaceDecl::Signal(sig) => {
                            println!("  signal {}({}){}", sig.name,
                                format_params(&sig.params),
                                format_return(&sig.return_type));
                        }
                        FaceDecl::Tool(tool) => {
                            let caps = if tool.capabilities.is_empty() {
                                String::new()
                            } else {
                                format!(" [{}]", tool.capabilities.join(", "))
                            };
                            let desc = tool.description.as_deref()
                                .map(|d| format!(" — {}", d)).unwrap_or_default();
                            println!("  tool {}({}){}{}{}", tool.name,
                                format_params(&tool.params),
                                format_return(&tool.return_type), caps, desc);
                        }
                        FaceDecl::Promise(p) => {
                            println!("  promise {}", format_promise(&p.constraint.node));
                        }
                        FaceDecl::Given(_) | FaceDecl::Await(_) => {}
                    }
                }
            }
            Section::Memory(mem) => {
                for slot in &mem.slots {
                    let props: Vec<String> = slot.node.properties.iter()
                        .map(|p| p.node.name().to_string()).collect();
                    let props = if props.is_empty() { String::new() }
                        else { format!(" [{}]", props.join(", ")) };
                    println!("  memory {}: {}{}", slot.node.name,
                        format_type(&slot.node.ty.node), props);
                }
                for inv in &mem.invariants {
                    println!("  invariant {}", format_expr(&inv.node));
                }
            }
            Section::State(sm) => {
                let ty = sm.state_type.as_deref()
                    .map(|t| format!(": {}", t)).unwrap_or_default();
                let transitions: Vec<String> = sm.transitions.iter()
                    .map(|t| format!("{} -> {}{}", t.node.from, t.node.to,
                        if t.node.guard.is_some() { " (guarded)" } else { "" }))
                    .collect();
                println!("  state {}{} (initial {}): {}", sm.name, ty,
                    sm.initial, transitions.join(", "));
            }
            _ => {}
        }
    }

    // Cells without a face still have a contract: their public handlers.
    if !has_face {
        for section in &cell.sections {
            if let Section::OnSignal(on) = &section.node {
                if on.signal_name.starts_with('_') { continue; }
                println!("  on {}({})", on.signal_name, format_params(&on.params));
            }
        }
    }
}

fn face_json(cell: &CellDef) -> serde_json::Value {
    let kind = match cell.kind {
        CellKind::Agent => "agent",
        CellKind::Type => "type",
        _ => "cell",
    };
    let mut signals = Vec::new();
    let mut tools = Vec::new();
    let mut promises = Vec::new();
    let mut memory = Vec::new();
    let mut invariants = Vec::new();
    let mut states = Vec::new();
    let mut variants = Vec::new();

    for section in &cell.sections {
        match &section.node {
            Section::Face(face) => {
                for decl in &face.declarations {
                    match &decl.node {
                        FaceDecl::Signal(sig) => signals.push(format!(
                            "{}({}){}", sig.name, format_params(&sig.params),
                            format_return(&sig.return_type))),
                        FaceDecl::Tool(tool) => {
                            let mut t = serde_json::json!({
                                "signature": format!("{}({}){}", tool.name,
                                    format_params(&tool.params),
                                    format_return(&tool.return_type)),
                            });
                            if let Some(ref d) = tool.description {
                                t["description"] = serde_json::json!(d);
                            }
                            if !tool.capabilities.is_empty() {
                                t["capabilities"] = serde_json::json!(tool.capabilities);
                            }
                            tools.push(t);
                        }
                        FaceDecl::Promise(p) => promises.push(format_promise(&p.constraint.node)),
                        FaceDecl::Given(_) | FaceDecl::Await(_) => {}
                    }
                }
            }
            Section::Memory(mem) => {
                for slot in &mem.slots {
                    let props: Vec<String> = slot.node.properties.iter()
                        .map(|p| p.node.name().to_string()).collect();
                    memory.push(serde_json::json!({
                        "name": slot.node.name,
                        "type": format_type(&slot.node.ty.node),
                        "properties": props,
                    }));
                }
                for inv in &mem.invariants {
                    invariants.push(format_expr(&inv.node));
                }
            }
            Section::State(sm) => {
                let transitions: Vec<String> = sm.transitions.iter()
                    .map(|t| format!("{} -> {}", t.node.from, t.node.to))
                    .collect();
                let mut s = serde_json::json!({
                    "name": sm.name,
                    "initial": sm.initial,
                    "transitions": transitions,
                });
                if let Some(ref ty) = sm.state_type {
                    s["type"] = serde_json::json!(ty);
                }
                states.push(s);
            }
            Section::Variants(v) => {
                for var in &v.variants {
                    variants.push(format_variant(&var.node));
                }
            }
            _ => {}
        }
    }

    let mut result = serde_json::json!({ "name": cell.name, "kind": kind });
    if !signals.is_empty() { result["signals"] = serde_json::json!(signals); }
    if !tools.is_empty() { result["tools"] = serde_json::json!(tools); }
    if !promises.is_empty() { result["promises"] = serde_json::json!(promises); }
    if !memory.is_empty() { result["memory"] = serde_json::json!(memory); }
    if !invariants.is_empty() { result["invariants"] = serde_json::json!(invariants); }
    if !states.is_empty() { result["states"] = serde_json::json!(states); }
    if !variants.is_empty() { result["variants"] = serde_json::json!(variants); }
    result
}

/// Render a promise the way it was written: `promise all_persistent`
/// is a zero-arg predicate — print it without the empty parens.
fn format_promise(c: &Constraint) -> String {
    if let Constraint::Predicate { name, args } = c {
        if args.is_empty() {
            return name.clone();
        }
    }
    format_constraint(c)
}

fn format_params(params: &[Param]) -> String {
    params.iter()
        .map(|p| format!("{}: {}", p.name, format_type(&p.ty.node)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_return(ret: &Option<Spanned<TypeExpr>>) -> String {
    ret.as_ref().map(|r| format!(" -> {}", format_type(&r.node))).unwrap_or_default()
}

fn format_variants(v: &VariantsSection) -> String {
    v.variants.iter()
        .map(|var| format_variant(&var.node))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn format_variant(var: &VariantDecl) -> String {
    match &var.fields {
        VariantFields::Unit => var.name.clone(),
        VariantFields::Tuple(tys) => format!("{}({})", var.name,
            tys.iter().map(|t| format_type(&t.node)).collect::<Vec<_>>().join(", ")),
        VariantFields::Struct(fields) => format!("{} {{ {} }}", var.name,
            fields.iter()
                .map(|(n, t)| format!("{}: {}", n, format_type(&t.node)))
                .collect::<Vec<_>>()
                .join(", ")),
    }
}

/// Convert a byte offset to a 1-based line number.
fn span_to_line(source: &str, offset: usize) -> usize {
    let (line, _col) = crate::interpreter::span_to_location(source, offset);
    line
}

pub fn format_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Simple(name) => name.clone(),
        TypeExpr::Generic { name, args } => {
            let arg_strs: Vec<String> = args.iter().map(|a| format_type(&a.node)).collect();
            format!("{}<{}>", name, arg_strs.join(", "))
        }
        TypeExpr::CellRef { cell, member } => format!("{}.{}", cell, member),
    }
}

/// Best-effort rendering of a constraint as a human-readable string.
fn format_constraint(c: &Constraint) -> String {
    match c {
        Constraint::Comparison { left, op, right } => {
            format!("{} {} {}", format_expr(&left.node), op, format_expr(&right.node))
        }
        Constraint::Predicate { name, args } => {
            let arg_strs: Vec<String> = args.iter().map(|a| format_expr(&a.node)).collect();
            format!("{}({})", name, arg_strs.join(", "))
        }
        Constraint::And(a, b) => {
            format!("{} && {}", format_constraint(&a.node), format_constraint(&b.node))
        }
        Constraint::Or(a, b) => {
            format!("{} || {}", format_constraint(&a.node), format_constraint(&b.node))
        }
        Constraint::Not(c) => {
            format!("!{}", format_constraint(&c.node))
        }
        Constraint::Descriptive(s) => s.clone(),
    }
}

/// Best-effort rendering of an expression for display in describe output.
fn format_expr(e: &Expr) -> String {
    match e {
        Expr::Literal(lit) => format_literal(lit),
        Expr::Ident(name) => name.clone(),
        Expr::FieldAccess { target, field } => {
            format!("{}.{}", format_expr(&target.node), field)
        }
        Expr::BinaryOp { left, op, right } => {
            format!("{} {} {}", format_expr(&left.node), op, format_expr(&right.node))
        }
        Expr::CmpOp { left, op, right } => {
            format!("{} {} {}", format_expr(&left.node), op, format_expr(&right.node))
        }
        Expr::FnCall { name, args } => {
            let arg_strs: Vec<String> = args.iter().map(|a| format_expr(&a.node)).collect();
            format!("{}({})", name, arg_strs.join(", "))
        }
        Expr::MethodCall { target, method, args } => {
            let arg_strs: Vec<String> = args.iter().map(|a| format_expr(&a.node)).collect();
            format!("{}.{}({})", format_expr(&target.node), method, arg_strs.join(", "))
        }
        Expr::Not(inner) => format!("!{}", format_expr(&inner.node)),
        _ => "...".to_string(),
    }
}

fn format_literal(lit: &Literal) -> String {
    match lit {
        Literal::Int(n) => n.to_string(),
        Literal::BigInt(s) => s.clone(),
        Literal::Float(f) => f.to_string(),
        Literal::String(s) => format!("\"{}\"", s),
        Literal::Bool(b) => b.to_string(),
        Literal::Unit => "()".to_string(),
        Literal::Duration(d) => format!("{}{}", d.value, match d.unit {
            DurationUnit::Milliseconds => "ms",
            DurationUnit::Seconds => "s",
            DurationUnit::Minutes => "min",
            DurationUnit::Hours => "h",
            DurationUnit::Days => "d",
            DurationUnit::Years => "y",
        }),
        Literal::Percentage(p) => format!("{}%", p),
    }
}

/// Public re-export of `format_constraint` for the dashboard.
pub fn format_constraint_pub(c: &Constraint) -> String {
    format_constraint(c)
}

/// Public re-export of `format_expr` for the dashboard.
pub fn format_expr_pub(c: &Expr) -> String {
    format_expr(c)
}
