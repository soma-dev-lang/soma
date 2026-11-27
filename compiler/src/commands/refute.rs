//! The gesture of refutation: `soma fingerprint` names a program, `soma
//! monitor` prints the predicate derived from a rule, `soma refute` plays a
//! trace of calls and says whether a rule was broken.
//!
//! A trace is calls and nothing else — no assertion, no monitor written by
//! hand. The monitors are derived from the law (`always …`) and read the
//! memory through the skeleton's `maps`: nobody writes them, so nobody can
//! write a lenient one. The verdict is one of four, each with its own exit
//! status, and it names the program by its fingerprint.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ast;
use crate::checker::law;
use crate::interpreter::{self, Value};
use crate::lexer::Token;
use crate::registry::Registry;
use crate::runtime;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports, load_meta_cells_from_program};

fn sha(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

fn sha_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

fn cell_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') || name == "node_modules" || name == "target" { continue; }
        if p.is_dir() { cell_files(&p, out); } else if name.ends_with(".cell") { out.push(p); }
    }
}

/// One element of what is affirmed, and its digest.
pub struct Print { pub kind: &'static str, pub name: String, pub digest: String }

/// The law, each skeleton, each flesh (and each plain cell), the manifest —
/// then the binary, and `program`: one digest over everything but the binary.
/// A skeleton, a flesh or a cell is hashed by its TOKENS: comments and
/// layout are not the program. Test cells are not part of it.
pub fn fingerprint(path: &PathBuf, program: &ast::Program) -> Vec<Print> {
    let root = crate::project::root(path);
    let mut out: Vec<Print> = Vec::new();
    let manifest_path = root.join("soma.toml");
    let manifest = crate::pkg::manifest::Manifest::load(&manifest_path).ok();
    let loaded = law::load(manifest.as_ref(), &root);
    for (file, text) in &loaded.files { out.push(Print { kind: "law", name: file.clone(), digest: sha(text) }); }
    // the cells of the program, as the loader sees them
    let mut wanted: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    fn names(cells: &[ast::Spanned<ast::CellDef>], out: &mut std::collections::BTreeSet<String>) {
        for c in cells {
            if c.node.kind == ast::CellKind::Test { continue; }
            out.insert(c.node.name.clone());
        }
    }
    names(&program.cells, &mut wanted);
    // (kind, name) -> the tokens of its blocks, in the order of the files
    let mut blocks: BTreeMap<(&'static str, String), String> = BTreeMap::new();
    let mut files = Vec::new();
    cell_files(&root, &mut files);
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let Ok(tokens) = crate::lexer::Lexer::new(&text).tokenize() else { continue };
        let word = |t: &Token| match t { Token::Ident(w) | Token::TypeIdent(w) => Some(w.clone()), _ => None };
        let mut i = 0;
        let mut depth = 0i32;
        while i < tokens.len() {
            let start = i;
            let head = if depth == 0 { match &tokens[i].token {
                Token::Ident(w) if w == "skeleton" || w == "flesh" => Some(if w == "skeleton" { "skeleton" } else { "flesh" }),
                Token::Cell => Some("cell"),
                _ => None,
            } } else { None };
            match tokens[i].token { Token::LBrace => depth += 1, Token::RBrace => depth -= 1, _ => {} }
            let Some(kind) = head else { i += 1; continue };
            // up to the block's `{`: its kind words and its name
            let Some(open) = (i..tokens.len()).find(|k| tokens[*k].token == Token::LBrace) else { break };
            let is_test = tokens[i + 1..open].iter().any(|t| matches!(&t.token, Token::Ident(w) if w == "test") || format!("{:?}", t.token) == "Test");
            let name = tokens[i + 1..open].iter().filter_map(|t| word(&t.token)).find(|w| !["agent", "test", "type", "property", "checker", "backend", "builtin", "under"].contains(&w.as_str())).unwrap_or_default();
            let mut d = 0i32;
            let mut close = open;
            for (k, t) in tokens.iter().enumerate().skip(open) {
                match t.token { Token::LBrace => d += 1, Token::RBrace => { d -= 1; if d == 0 { close = k; break; } } _ => {} }
            }
            if !is_test && wanted.contains(&name) {
                let text: String = tokens[start..=close].iter().map(|t| format!("{:?}\u{1f}", t.token)).collect();
                blocks.entry((kind, name)).or_default().push_str(&text);
            }
            i = close + 1;
            depth = 0;
        }
    }
    for ((kind, name), text) in &blocks { if *kind == "skeleton" { out.push(Print { kind: "skeleton", name: name.clone(), digest: sha(text) }); } }
    for ((kind, name), text) in &blocks { if *kind == "flesh" { out.push(Print { kind: "flesh", name: name.clone(), digest: sha(text) }); } }
    for ((kind, name), text) in &blocks { if *kind == "cell" { out.push(Print { kind: "cell", name: name.clone(), digest: sha(text) }); } }
    if let Ok(text) = std::fs::read_to_string(&manifest_path) { out.push(Print { kind: "manifest", name: "soma.toml".to_string(), digest: sha(&text) }); }
    let whole: String = out.iter().map(|p| format!("{} {} {}\n", p.kind, p.name, p.digest)).collect();
    let exe = std::env::current_exe().ok().and_then(|p| std::fs::read(p).ok()).map(|b| sha_bytes(&b)).unwrap_or_else(|| "unknown".to_string());
    out.push(Print { kind: "binary", name: format!("soma {}", env!("CARGO_PKG_VERSION")), digest: exe });
    out.push(Print { kind: "program", name: String::new(), digest: sha(&whole) });
    out
}

fn load(path: &PathBuf, registry: &mut Registry) -> (ast::Program, String) {
    crate::interpreter::set_edition_beside(path);
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    load_meta_cells_from_program(&program, registry, path);
    (program, source)
}

/// `soma fingerprint app.cell [--json]`
pub fn cmd_fingerprint(path: &PathBuf, json: bool, registry: &mut Registry) {
    let (program, _) = load(path, registry);
    let prints = fingerprint(path, &program);
    if json {
        let program_digest = prints.iter().find(|p| p.kind == "program").map(|p| p.digest.clone()).unwrap_or_default();
        let elements: Vec<serde_json::Value> = prints.iter().filter(|p| p.kind != "program").map(|p| serde_json::json!({"kind": p.kind, "name": p.name, "sha256": p.digest})).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "schema": "soma.fingerprint/1", "program": program_digest, "elements": elements })).unwrap_or_default());
    } else {
        for p in &prints { println!("{:<9} {:<28} {}", p.kind, p.name, p.digest); }
    }
}

fn monitors_of(path: &PathBuf, program: &ast::Program) -> (Vec<law::Monitor>, Vec<law::Law>) {
    let root = crate::project::root(path);
    let manifest = crate::pkg::manifest::Manifest::load(&root.join("soma.toml")).ok();
    let loaded = law::load(manifest.as_ref(), &root);
    (law::monitors(program, &loaded.laws), loaded.laws)
}

/// `soma monitor app.cell ["RULE"] [--json]`: the predicate derived from a rule.
pub fn cmd_monitor(path: &PathBuf, rule: Option<&str>, json: bool, registry: &mut Registry) {
    let (program, _) = load(path, registry);
    let (monitors, laws) = monitors_of(path, &program);
    let rules: Vec<&law::Rule> = laws.iter().flat_map(|l| l.rules.iter()).collect();
    if let Some(id) = rule {
        if !rules.iter().any(|r| r.id == id) {
            eprintln!("no rule \"{}\" in the laws of this program — the rules are {}", id, rules.iter().map(|r| format!("\"{}\"", r.id)).collect::<Vec<_>>().join(", "));
            std::process::exit(1);
        }
        if !monitors.iter().any(|m| m.rule == id) {
            let r = rules.iter().find(|r| r.id == id).unwrap();
            eprintln!("the rule \"{}\" ({}) has no monitor: {} — a monitor is derived from what a rule says ALWAYS holds (`always p: refunded(p) <= captured(p)`, written by the owner of the law) and from the `maps` of its states; nobody writes one by hand",
                r.id, r.text, if r.always.is_some() { "a state it speaks of has no `maps` in the program (`soma check` gives the line to write)" } else { "it says nothing that always holds" });
            std::process::exit(1);
        }
    }
    let shown: Vec<&law::Monitor> = monitors.iter().filter(|m| rule.is_none_or(|id| m.rule == id)).collect();
    if json {
        let list: Vec<serde_json::Value> = shown.iter().map(|m| serde_json::json!({
            "rule": m.rule, "sentence": m.sentence, "source": m.source, "form": m.form,
            "maps": m.maps.iter().map(|(st, cell, p, e)| serde_json::json!({"state": st, "cell": cell, "param": p, "expression": e})).collect::<Vec<_>>(),
        })).collect();
        let doc = if rule.is_some() && list.len() == 1 { list[0].clone() } else { serde_json::json!({ "schema": "soma.monitor/1", "monitors": list }) };
        println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
        return;
    }
    if shown.is_empty() {
        println!("no monitor: no rule of this program's laws says what always holds (`always p: …`) with every state mapped");
        return;
    }
    for m in shown {
        println!("rule \"{}\"   {}{}", m.rule, m.sentence, if m.source.is_empty() { String::new() } else { format!("   ({})", m.source) });
        println!("  form      {}", m.form);
        for (st, cell, p, e) in &m.maps { println!("  maps      {}({}) = {}   — read in `{}`", st, p, e, cell); }
        println!("  judged    after every accepted call of a trace, for each value the trace names (`soma refute`); derived from the rule, written by no one");
    }
}

/// A call of a trace: `Cell.signal(literal, …)`.
struct Call { cell: String, signal: String, args: Vec<Value>, text: String }

fn parse_trace(text: &str, program: &ast::Program) -> Result<Vec<Call>, String> {
    let mut calls = Vec::new();
    let signals = |cell: &str| -> Vec<String> {
        program.cells.iter().filter(|c| c.node.name == cell && c.node.kind != ast::CellKind::Test).flat_map(|c| c.node.sections.iter())
            .filter_map(|s| if let ast::Section::Face(f) = &s.node { Some(f) } else { None }).flat_map(|f| f.declarations.iter())
            .filter_map(|d| if let ast::FaceDecl::Signal(sig) = &d.node { Some(sig.name.clone()) } else { None }).collect()
    };
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let at = |m: String| format!("line {}: `{}` — {}", n + 1, line, m);
        let only_calls = "a trace is calls and nothing else: one `Cell.signal(\"literal\", 42)` per line — no `assert`, no `let`, no expression";
        let (Some(open), true) = (line.find('('), line.ends_with(')')) else { return Err(at(only_calls.to_string())) };
        let head = line[..open].trim();
        if head.contains(' ') || head.is_empty() { return Err(at(only_calls.to_string())); }
        let (cell, signal) = match head.split_once('.') {
            Some((c, s)) => (c.to_string(), s.to_string()),
            None => {
                let owners: Vec<String> = program.cells.iter().filter(|c| c.node.kind != ast::CellKind::Test && signals(&c.node.name).contains(&head.to_string())).map(|c| c.node.name.clone()).collect();
                match owners.as_slice() {
                    [one] => (one.clone(), head.to_string()),
                    [] => return Err(at(format!("no cell of the program has a signal `{}`", head))),
                    _ => return Err(at(format!("`{}` is a signal of {}: name the cell (`{}.{}(…)`)", head, owners.join(" and "), owners[0], head))),
                }
            }
        };
        if !program.cells.iter().any(|c| c.node.name == cell && c.node.kind != ast::CellKind::Test) {
            return Err(at(format!("no cell `{}` in the program", cell)));
        }
        if !signals(&cell).contains(&signal) {
            return Err(at(format!("`{}` is not a signal of `{}`'s face (its signals: {}) — a trace calls what the outside may call", signal, cell, signals(&cell).join(", "))));
        }
        // the arguments: literals only
        let inner = &line[open + 1..line.len() - 1];
        let mut args = Vec::new();
        let mut cur = String::new();
        let mut in_str = false;
        let mut parts: Vec<String> = Vec::new();
        let mut chars = inner.chars().peekable();
        while let Some(c) = chars.next() {
            if in_str { cur.push(c); if c == '\\' { if let Some(x) = chars.next() { cur.push(x); } } else if c == '"' { in_str = false; } }
            else if c == '"' { in_str = true; cur.push(c); }
            else if c == ',' { parts.push(cur.trim().to_string()); cur.clear(); }
            else { cur.push(c); }
        }
        if !cur.trim().is_empty() || !parts.is_empty() { parts.push(cur.trim().to_string()); }
        for p in parts {
            let v = if p.len() >= 2 && p.starts_with('"') && p.ends_with('"') { Value::String(p[1..p.len() - 1].replace("\\\"", "\"")) }
                else if let Ok(i) = p.replace('_', "").parse::<i64>() { Value::Int(interpreter::soma_int::SomaInt::from_i64(i)) }
                else if p == "true" || p == "false" { Value::Bool(p == "true") }
                else if let Ok(f) = p.parse::<f64>() { Value::Float(f) }
                else { return Err(at(format!("`{}` is not a literal: an argument of a trace is a string, a number or a boolean — the outside (a clock, a model, another call) is given by the trace as a value, or the call is not in it", p))) };
            args.push(v);
        }
        calls.push(Call { cell, signal, args, text: line.to_string() });
    }
    if calls.is_empty() { return Err("the trace is empty: it holds no call".to_string()); }
    Ok(calls)
}

fn show(v: &Value) -> String {
    match v { Value::String(s) => format!("\"{}\"", s), other => format!("{}", other) }
}

/// `soma refute app.cell --trace t [--expect <fingerprint>] [--json]`.
/// Exit status: 0 `holds`, 1 `refuted`, 2 `rejected`, 3 `mismatch`, 4 when
/// the question itself does not stand (no monitor, a trace that is not one).
pub fn cmd_refute(path: &PathBuf, trace: &PathBuf, expect: Option<&str>, json: bool, registry: &mut Registry) {
    let stop = |m: String| -> ! { eprintln!("error: {}", m); std::process::exit(4) };
    // the run starts from nothing, keeps nothing, and is told about no test double
    runtime::storage::EPHEMERAL.store(true, std::sync::atomic::Ordering::Relaxed);
    let (program, source) = load(path, registry);
    {
        let mut chk = crate::checker::Checker::new(registry);
        chk.source = Some((path.display().to_string(), source.clone()));
        chk.check(&program);
        if chk.has_errors() { stop(format!("{} fails `soma check`: a program that does not check is refuted by nothing — `soma check {}`", path.display(), path.display())); }
    }
    refuse_the_outside(&program);
    let prints = fingerprint(path, &program);
    let program_print = prints.iter().find(|p| p.kind == "program").map(|p| p.digest.clone()).unwrap_or_default();
    let binary = prints.iter().find(|p| p.kind == "binary").map(|p| format!("{} {}", p.name, p.digest)).unwrap_or_default();
    let (monitors, _) = monitors_of(path, &program);
    let text = std::fs::read_to_string(trace).unwrap_or_else(|e| stop(format!("{}: {}", trace.display(), e)));
    let calls = parse_trace(&text, &program).unwrap_or_else(|m| stop(format!("{}: {}", trace.display(), m)));
    if monitors.is_empty() {
        stop("no monitor: no rule of this program's laws says what always holds (`always p: …`) with every state mapped — there is nothing a trace could refute (`soma monitor` lists them)".to_string());
    }
    let finish = |verdict: &str, code: i32, lines: Vec<String>, extra: serde_json::Value| -> ! {
        if json {
            let mut doc = serde_json::json!({ "schema": "soma.refute/1", "verdict": verdict, "program": program_print, "binary": binary, "trace": trace.display().to_string(), "calls": calls.len(),
                "monitors": monitors.iter().map(|m| m.rule.clone()).collect::<Vec<_>>() });
            if let (Some(d), Some(e)) = (doc.as_object_mut(), extra.as_object()) { for (k, v) in e { d.insert(k.clone(), v.clone()); } }
            println!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
        } else {
            for l in &lines { println!("{}", l); }
            println!("  program   {}", program_print);
            println!("  binary    {}", binary);
        }
        std::process::exit(code)
    };
    if let Some(want) = expect {
        if want != program_print {
            finish("mismatch", 3, vec![
                format!("mismatch  the trace aims at another program than this one"),
                format!("  expected  {}", want),
                "  the trace proves nothing of this program: `soma fingerprint` says which element differs".to_string(),
            ], serde_json::json!({ "expected": want }));
        }
    }
    let mut interp = fresh(&program);
    // the values the trace names: what a monitor is judged on
    let mut seen: Vec<Value> = Vec::new();
    for (i, call) in calls.iter().enumerate() {
        for a in &call.args { if matches!(a, Value::String(_) | Value::Int(_)) && !seen.contains(a) { seen.push(a.clone()); } }
        if let Err(e) = interp.call_signal(&call.cell, &call.signal, call.args.clone()) {
            finish("rejected", 2, vec![
                format!("rejected  step {} of {}: {}", i + 1, calls.len(), call.text),
                format!("  the program refused the call: {}", e),
                "  a trace whose call is refused proves nothing: neither that a rule is broken, nor that it holds".to_string(),
            ], serde_json::json!({ "step": i + 1, "call": call.text, "error": format!("{}", e) }));
        }
        for m in &monitors {
            for v in &seen {
                if let Ok(false) = interp.law_monitor(&m.cond, &m.var, v) {
                    let readings = interp.law_readings(&m.cond, &m.var, v);
                    let values: Vec<String> = m.maps.iter().zip(readings.iter()).map(|((st, _, _, _), r)| format!("{}({}) = {}", st, show(v), r)).collect();
                    finish("refuted", 1, vec![
                        format!("refuted   rule \"{}\" — {}{}", m.rule, m.sentence, if m.source.is_empty() { String::new() } else { format!(" ({})", m.source) }),
                        format!("  step {} of {}: {}   — accepted by the program", i + 1, calls.len(), call.text),
                        format!("  the monitor `{}` fails for {} = {}: {}", m.form, m.var, show(v), values.join(", ")),
                    ], serde_json::json!({ "rule": m.rule, "step": i + 1, "call": call.text, "entity": show(v), "values": values }));
                }
            }
        }
    }
    finish("holds", 0, vec![
        format!("holds     {} call(s) played, {} monitor(s) judged after each ({}): none failed", calls.len(), monitors.len(), monitors.iter().map(|m| format!("\"{}\"", m.rule)).collect::<Vec<_>>().join(", ")),
        "  nothing was proven: a rule holds on this trace, not on every trace".to_string(),
    ], serde_json::json!({}));
}

/// What the program reads of the outside: `(Cell.handler, builtin)`. A trace
/// gives literals; a clock, chance, a model or the network would make two
/// runs differ.
fn outside(program: &ast::Program) -> Vec<(String, String)> {
    let mut out = Vec::new();
    fn walk(cells: &[ast::Spanned<ast::CellDef>], out: &mut Vec<(String, String)>) {
        for c in cells {
            if c.node.kind == ast::CellKind::Test { continue; }
            for s in &c.node.sections {
                match &s.node {
                    ast::Section::OnSignal(on) => {
                        let text = format!("{:?}", on.body);
                        for part in text.split("FnCall { name: \"").skip(1) {
                            let f = part.split('"').next().unwrap_or("");
                            let reads = crate::interpreter::builtins::registry::lookup(f).is_some_and(|b| !b.deterministic)
                                || ["think", "think_json", "http_get", "http_post", "http_put", "http_patch", "http_delete", "run_process", "env_var", "read_file", "read_line", "read_stdin", "list_dir", "file_info", "delegate"].contains(&f);
                            let who = format!("{}.{}", c.node.name, on.signal_name);
                            if reads && !out.contains(&(who.clone(), f.to_string())) { out.push((who, f.to_string())); }
                        }
                    }
                    ast::Section::Interior(i) => walk(&i.cells, out),
                    _ => {}
                }
            }
        }
    }
    walk(&program.cells, &mut out);
    out
}

fn refuse_the_outside(program: &ast::Program) {
    let reads = outside(program);
    if reads.is_empty() { return; }
    eprintln!("error: this program reads the outside — {} — and a trace gives it literals only: two runs could give two verdicts. A program judged by a trace takes what it needs of the outside as a parameter of its signal (the trace then gives it), or the handler is left out of what is attested",
        reads.iter().map(|(who, f)| format!("`{}` calls `{}`", who, f)).collect::<Vec<_>>().join(", "));
    std::process::exit(4);
}

/// An interpreter on the program, with empty memory that is kept nowhere.
fn fresh(program: &ast::Program) -> interpreter::Interpreter {
    let mut interp = interpreter::Interpreter::new(program);
    for cell in &program.cells {
        if matches!(cell.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) {
            for section in &cell.node.sections {
                if let ast::Section::Memory(ref mem) = section.node {
                    let mut slots = std::collections::HashMap::new();
                    for slot in &mem.slots {
                        let backend: std::sync::Arc<dyn runtime::storage::StorageBackend> = std::sync::Arc::new(runtime::storage::MemoryBackend::new());
                        slots.insert(slot.node.name.clone(), backend);
                    }
                    interp.set_storage(&cell.node.name, &slots);
                }
            }
        }
    }
    interp.ensure_state_machine_storage();
    interp
}

/// `soma attack app.cell ["RULE"] [--depth n] [--out file]`: a bounded
/// search for a trace that refutes a rule — every sequence of at most
/// `depth` calls of the program's signals, over small domains of
/// arguments, each played from an empty state. It writes nothing in the
/// program. Exit status: 1 when a trace refutes (it is printed, and
/// written to `--out`), 0 when none does — which proves nothing.
pub fn cmd_attack(path: &PathBuf, rule: Option<&str>, depth: usize, out_file: Option<&PathBuf>, json: bool, registry: &mut Registry) {
    let stop = |m: String| -> ! { eprintln!("error: {}", m); std::process::exit(4) };
    runtime::storage::EPHEMERAL.store(true, std::sync::atomic::Ordering::Relaxed);
    let (program, source) = load(path, registry);
    {
        let mut chk = crate::checker::Checker::new(registry);
        chk.source = Some((path.display().to_string(), source.clone()));
        chk.check(&program);
        if chk.has_errors() { stop(format!("{} fails `soma check`: `soma check {}` first", path.display(), path.display())); }
    }
    refuse_the_outside(&program);
    let (all, laws) = monitors_of(path, &program);
    if let Some(id) = rule {
        if !laws.iter().flat_map(|l| l.rules.iter()).any(|r| r.id == id) { stop(format!("no rule \"{}\" in the laws of this program", id)); }
    }
    let monitors: Vec<law::Monitor> = all.into_iter().filter(|m| rule.is_none_or(|id| m.rule == id)).collect();
    if monitors.is_empty() {
        stop(format!("no monitor{}: a rule can be attacked when it says what always holds (`always p: …`) and each of its states is mapped — `soma monitor {}` lists them", rule.map(|r| format!(" for the rule \"{}\"", r)).unwrap_or_default(), path.display()));
    }
    // the calls the outside may make, over small domains
    let strings = ["p1", "p2"];
    let ints: [i64; 3] = [1, 2, 3];
    let mut moves: Vec<(String, String, Vec<Value>, String)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for cell in program.cells.iter().filter(|c| c.node.kind != ast::CellKind::Test) {
        for section in &cell.node.sections {
            let ast::Section::Face(face) = &section.node else { continue };
            for d in &face.declarations {
                let ast::FaceDecl::Signal(sig) = &d.node else { continue };
                if sig.name == "request" { continue; }
                let mut combos: Vec<Vec<Value>> = vec![Vec::new()];
                let mut ok = true;
                for p in &sig.params {
                    let ty = super::describe::format_type(&p.ty.node);
                    let domain: Vec<Value> = match ty.as_str() {
                        "Int" => ints.iter().map(|n| Value::Int(interpreter::soma_int::SomaInt::from_i64(*n))).collect(),
                        "String" => strings.iter().map(|s| Value::String(s.to_string())).collect(),
                        "Bool" => vec![Value::Bool(true), Value::Bool(false)],
                        _ => { ok = false; Vec::new() }
                    };
                    combos = combos.into_iter().flat_map(|c| domain.iter().map(move |v| { let mut n = c.clone(); n.push(v.clone()); n })).collect();
                }
                if !ok { skipped.push(format!("{}.{}", cell.node.name, sig.name)); continue; }
                for args in combos {
                    let text = format!("{}.{}({})", cell.node.name, sig.name, args.iter().map(show).collect::<Vec<_>>().join(", "));
                    moves.push((cell.node.name.clone(), sig.name.clone(), args, text));
                }
            }
        }
    }
    if moves.is_empty() { stop("no signal of the program takes only Int, String or Bool parameters: there is no call this search can make".to_string()); }
    let limit = 200_000usize;
    let mut played = 0usize;
    // breadth first: the shortest trace that refutes is the one to show
    let mut frontier: Vec<Vec<usize>> = vec![Vec::new()];
    let mut found: Option<(Vec<usize>, String, String, Vec<String>)> = None;
    'search: for _level in 0..depth {
        let mut next: Vec<Vec<usize>> = Vec::new();
        for prefix in &frontier {
            for m in 0..moves.len() {
                if played >= limit { break 'search; }
                played += 1;
                let mut seq = prefix.clone();
                seq.push(m);
                let mut interp = fresh(&program);
                let mut seen: Vec<Value> = Vec::new();
                let mut accepted = true;
                for (k, i) in seq.iter().enumerate() {
                    let (cell, signal, args, _) = &moves[*i];
                    for a in args { if matches!(a, Value::String(_) | Value::Int(_)) && !seen.contains(a) { seen.push(a.clone()); } }
                    if interp.call_signal(cell, signal, args.clone()).is_err() { accepted = false; break; }
                    // only the last call is new: the prefix was judged at its own level
                    if k + 1 < seq.len() { continue; }
                    for mon in &monitors {
                        for v in &seen {
                            if let Ok(false) = interp.law_monitor(&mon.cond, &mon.var, v) {
                                let readings = interp.law_readings(&mon.cond, &mon.var, v);
                                let values: Vec<String> = mon.maps.iter().zip(readings.iter()).map(|((st, _, _, _), r)| format!("{}({}) = {}", st, show(v), r)).collect();
                                found = Some((seq.clone(), mon.rule.clone(), mon.sentence.clone(), values));
                                break 'search;
                            }
                        }
                    }
                }
                if accepted { next.push(seq); }
            }
        }
        frontier = next;
        if frontier.is_empty() { break; }
    }
    let prints = fingerprint(path, &program);
    let program_print = prints.iter().find(|p| p.kind == "program").map(|p| p.digest.clone()).unwrap_or_default();
    let domains = format!("String in {{{}}}, Int in {{{}}}", strings.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(", "), ints.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", "));
    match found {
        Some((seq, rule_id, sentence, values)) => {
            let lines: Vec<String> = seq.iter().map(|i| moves[*i].3.clone()).collect();
            let trace = format!("# refutes \"{}\" ({}) — found by `soma attack`, {} sequence(s) played\n# program {}\n{}\n", rule_id, sentence, played, program_print, lines.join("\n"));
            if let Some(f) = out_file { if let Err(e) = std::fs::write(f, &trace) { stop(format!("{}: {}", f.display(), e)); } }
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "schema": "soma.attack/1", "verdict": "refuted", "rule": rule_id, "trace": lines, "values": values, "played": played, "program": program_print })).unwrap_or_default());
            } else {
                println!("refuted   rule \"{}\" — {}", rule_id, sentence);
                for (k, l) in lines.iter().enumerate() { println!("  {}. {}", k + 1, l); }
                println!("  then {}", values.join(", "));
                println!("  {} sequence(s) played; program {}", played, program_print);
                match out_file { Some(f) => println!("  the trace is in {} — `soma refute {} --trace {}` replays it; kept in refutations/, `soma law` shows it", f.display(), path.display(), f.display()),
                    None => println!("  `--out refutations/<name>.trace` keeps it: `soma law` then shows the rule as refuted") }
            }
            std::process::exit(1);
        }
        None => {
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "schema": "soma.attack/1", "verdict": "not refuted", "depth": depth, "played": played, "exhausted": played < limit, "skipped": skipped, "program": program_print })).unwrap_or_default());
            } else {
                println!("not refuted   no trace of at most {} call(s) refutes {} (depth {}, {} sequence(s) played{})", depth,
                    monitors.iter().map(|m| format!("\"{}\"", m.rule)).collect::<Vec<_>>().join(", "), depth, played, if played >= limit { ", stopped at the limit" } else { "" });
                println!("  searched: {} call(s) per step — {}", moves.len(), domains);
                if !skipped.is_empty() { println!("  not called (a parameter that is not an Int, a String or a Bool): {}", skipped.join(", ")); }
                println!("  this proves nothing: a longer trace, or other values, may refute the rule");
            }
        }
    }
}
