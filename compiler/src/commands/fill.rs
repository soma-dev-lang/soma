//! `soma next` and `soma fill`: the unit of an agent's work is the hole.
//!
//! `next` answers one question — what do I do now? — with ONE item of an
//! ordered queue. `fill` writes the body of one signal in the `flesh` of its
//! cell and can change nothing else: the layers a body may not touch (a
//! skeleton, another handler, a law) are out of the command's reach, not
//! forbidden in prose.

use std::path::{Path, PathBuf};
use crate::ast::Program;
use crate::checker::frame;
use crate::lexer::Token;

fn fail(msg: String) -> ! {
    eprintln!("error: {}", msg);
    std::process::exit(1);
}

fn load(path: &PathBuf) -> (Program, String) {
    let source = super::read_source(path);
    let file_str = path.display().to_string();
    let tokens = super::lex_with_location(&source, Some(&file_str));
    let mut program = super::parse_with_location(tokens, Some(&source), Some(&file_str));
    super::resolve_imports(&mut program, path);
    (program, source)
}

/// This binary, run on the program: the commands are the one source.
fn run_json(dir: &Path, args: &[&str]) -> Option<serde_json::Value> {
    let me = std::env::current_exe().ok()?;
    let out = std::process::Command::new(me).args(args).current_dir(dir).stdin(std::process::Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let at = text.find('{')?;
    serde_json::from_str(&text[at..]).ok()
}

fn dir_and_name(path: &Path) -> (PathBuf, String) {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    (dir, path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
}

// ── fill ────────────────────────────────────────────────────────────────

/// `(open, close)`: the byte offsets of a block's `{` and its matching `}`,
/// from the token at `from` (which is the `{`).
fn block(tokens: &[crate::lexer::SpannedToken], from: usize) -> Option<(usize, usize, usize)> {
    let mut depth = 0i32;
    for (i, t) in tokens.iter().enumerate().skip(from) {
        match t.token {
            Token::LBrace => depth += 1,
            Token::RBrace => { depth -= 1; if depth == 0 { return Some((tokens[from].span.start, t.span.start, i)); } }
            _ => {}
        }
    }
    None
}

struct Flesh { file: PathBuf, text: String, open: usize, close: usize, handler: Option<(usize, usize)> }

/// The `flesh cell { … }` blocks of one file, with the span of `on name(…) { … }` in each.
fn flesh_in(file: &Path, text: &str, cell: &str, name: &str) -> Vec<Flesh> {
    let Ok(mut tokens) = crate::lexer::Lexer::new(text).tokenize() else { return Vec::new() };
    // the lexer counts characters; the text is cut in bytes
    let bytes: Vec<usize> = text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect();
    for t in tokens.iter_mut() { t.span.start = bytes[t.span.start.min(bytes.len() - 1)]; t.span.end = bytes[t.span.end.min(bytes.len() - 1)]; }
    let word = |t: &Token| match t { Token::Ident(w) | Token::TypeIdent(w) => Some(w.clone()), _ => None };
    let mut found = Vec::new();
    let mut i = 0;
    let mut depth = 0i32;
    while i + 2 < tokens.len() {
        match tokens[i].token { Token::LBrace => depth += 1, Token::RBrace => depth -= 1, _ => {} }
        if depth == 0 && word(&tokens[i].token).as_deref() == Some("flesh") && word(&tokens[i + 1].token).as_deref() == Some(cell) && tokens[i + 2].token == Token::LBrace {
            let Some((open, close, last)) = block(&tokens, i + 2) else { break };
            let mut handler = None;
            let mut j = i + 3;
            let mut d = 0i32;
            while j < last {
                match tokens[j].token { Token::LBrace => d += 1, Token::RBrace => d -= 1, _ => {} }
                if d == 0 && tokens[j].token == Token::On && word(&tokens[j + 1].token).as_deref() == Some(name) {
                    if let Some(b) = (j + 2..last).find(|k| tokens[*k].token == Token::LBrace).and_then(|k| block(&tokens, k)) {
                        handler = Some((tokens[j].span.start, b.1 + 1));
                    }
                    break;
                }
                j += 1;
            }
            found.push(Flesh { file: file.to_path_buf(), text: text.to_string(), open, close, handler });
            i = last;
            depth = 0;
        }
        i += 1;
    }
    found
}

/// The lines of a body, at the indentation of a handler's inside.
fn indented(body: &str, by: &str) -> String {
    let lines: Vec<&str> = body.trim_matches('\n').lines().collect();
    let common = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
    lines.iter().map(|l| if l.trim().is_empty() { String::new() } else { format!("{}{}", by, &l[common.min(l.len())..].trim_end()) }).collect::<Vec<_>>().join("\n")
}

/// Braces that close what the body did not open: the body would end its
/// handler and write beside it.
fn balanced(text: &str) -> Result<(), String> {
    let tokens = crate::lexer::Lexer::new(text).tokenize().map_err(|e| format!("{:?}", e))?;
    let mut depth = 0i32;
    for t in &tokens {
        match t.token {
            Token::LBrace => depth += 1,
            Token::RBrace => { depth -= 1; if depth < 0 { return Err("a `}` closes what the body did not open — a body is the inside of ONE handler".to_string()); } }
            _ => {}
        }
    }
    if depth != 0 { return Err(format!("{} `{{` left open", depth)); }
    Ok(())
}

pub fn cmd_fill(path: &PathBuf, target: &str, body_file: Option<&PathBuf>, reject: bool, json: bool) {
    if path.extension().map(|e| e != "cell").unwrap_or(true) {
        fail(format!("`soma fill` writes a handler body in the flesh of a program: `{}` is not a .cell file — a law, a lock or a manifest is not written by it", path.display()));
    }
    let Some((cell, name)) = target.split_once('.') else {
        fail(format!("`{}` is not a signal: write it `Cell.signal` — `soma holes {}` lists the ones with no body", target, path.display()));
    };
    let body = match body_file {
        Some(f) => std::fs::read_to_string(f).unwrap_or_else(|e| fail(format!("{}: {}", f.display(), e))),
        None => { use std::io::Read; let mut s = String::new(); let _ = std::io::stdin().read_to_string(&mut s); s }
    };
    if body.trim().is_empty() {
        fail(format!("no body: give it on stdin or with --body-file — `soma fill {} {} < body.txt`", path.display(), target));
    }
    let (program, _) = load(path);
    let skeletons = frame::build(&program);
    let Some(sk) = skeletons.iter().find(|s| s.cell.name == cell) else {
        if program.cells.iter().any(|c| c.node.name == cell) {
            fail(format!("`{}` is not a skeleton: `soma fill` writes the body of a signal a skeleton declares — a test cell or a cell written whole is edited as a file", cell));
        }
        fail(format!("no cell `{}` in {} — `soma skeleton {}` shows the cells and their signals", cell, path.display(), path.display()));
    };
    if !sk.cell.skeleton {
        fail(format!("`{}` is not a skeleton: `soma fill` writes the body of a signal a skeleton declares. A cell written whole is edited as a file; `soma skeleton {} --split` turns it into a skeleton and its flesh", cell, path.display()));
    }
    let private = name.starts_with('_');
    let whole = body.trim_start().starts_with("on ");
    let bone = sk.bones.iter().find(|b| b.name == name && b.in_face);
    if !private && bone.is_none() {
        let declared: Vec<&str> = sk.bones.iter().filter(|b| b.in_face).map(|b| b.name.as_str()).collect();
        fail(format!("`{}` is not a signal of `skeleton {}`'s face (it declares {}) — a new signal is a design change: add it to the skeleton, then `soma skeleton {} --amend`. A helper of the flesh is named `_{}`", name, cell, declared.join(", "), path.display(), name));
    }
    if private && !whole {
        fail(format!("a helper is written whole, with its parameters and their types: `on {}(a: Int) {{ … }}`", name));
    }
    // the handler, as text
    let handler = if whole {
        let b = body.trim();
        let head: String = b.chars().take_while(|c| *c != '(').collect();
        if head.trim_start_matches("on").trim() != name {
            fail(format!("the body is `{}(…`, the target is `{}`: one fill writes one handler — nothing was written", head.trim(), target));
        }
        let lines: Vec<&str> = b.lines().collect();
        if lines.len() == 1 { format!("    {}", b) } else {
            format!("    {}\n{}\n    {}", lines[0].trim_end(), indented(&lines[1..lines.len() - 1].join("\n"), "        "), lines[lines.len() - 1].trim())
        }
    } else {
        let params: Vec<&str> = bone.map(|b| b.params.iter().map(|p| p.name.as_str()).collect()).unwrap_or_default();
        format!("    on {}({}) {{\n{}\n    }}", name, params.join(", "), indented(&body, "        "))
    };
    if let Err(why) = balanced(&handler).and_then(|_| if whole {
        // one handler: its braces close once, at the end
        let inner = &handler[handler.find('{').map(|i| i + 1).unwrap_or(0)..handler.rfind('}').unwrap_or(handler.len())];
        balanced(inner)
    } else { balanced(&body) }) {
        fail(format!("the body of `{}` does not read as one handler body: {} — nothing was written", target, why));
    }
    // where the flesh of the cell lives
    let root = crate::project::root(path);
    let mut files = Vec::new();
    collect_cells(&root, &mut files);
    let mut blocks: Vec<Flesh> = files.iter().filter_map(|f| std::fs::read_to_string(f).ok().map(|t| flesh_in(f, &t, cell, name))).flatten().collect();
    let chosen = blocks.iter().position(|b| b.handler.is_some()).or(if blocks.is_empty() { None } else { Some(0) });
    let (file, before, after) = match chosen {
        Some(i) => {
            let b = blocks.swap_remove(i);
            let new = match b.handler {
                Some((s, e)) => {
                    let line_start = b.text[..s].rfind('\n').map(|i| i + 1).unwrap_or(0);
                    format!("{}{}{}", &b.text[..line_start], handler, &b.text[e..])
                }
                None => {
                    let head = b.text[..b.close].trim_end();
                    let empty = head.len() == b.open + 1;
                    format!("{}{}{}\n{}", head, if empty { "\n" } else { "\n\n" }, handler, &b.text[b.close..])
                }
            };
            (b.file, b.text, new)
        }
        None => {
            // no flesh yet for this cell: it starts beside its skeleton
            let text = std::fs::read_to_string(path).unwrap_or_default();
            let new = format!("{}{}\nflesh {} {{\n{}\n}}\n", text, if text.ends_with('\n') { "" } else { "\n" }, cell, handler);
            (path.clone(), text, new)
        }
    };
    // it must still read as a program
    let parses = |t: &str| crate::lexer::Lexer::new(t).tokenize().ok().and_then(|tk| crate::parser::Parser::new(tk).parse_program().ok()).is_some();
    if !parses(&after) {
        let detail = crate::lexer::Lexer::new(&after).tokenize().map_err(|e| format!("{:?}", e)).and_then(|tk| crate::parser::Parser::new(tk).parse_program().map(|_| ()).map_err(|e| format!("{}", e))).err().unwrap_or_default();
        fail(format!("the body of `{}` does not parse: {} — nothing was written (`soma docs flesh`: the syntax a body needs)", target, detail));
    }
    if let Err(e) = std::fs::write(&file, &after) { fail(format!("{}: {}", file.display(), e)); }

    // the diagnostics of THIS signal
    let (dir, entry) = dir_and_name(path);
    let checked = run_json(&dir, &["check", &entry, "--json"]).unwrap_or(serde_json::json!({}));
    let shown_file = file.strip_prefix(&dir).unwrap_or(&file).display().to_string();
    let start = after.find(&handler).map(|i| after[..i].lines().count() + 1).unwrap_or(0);
    let lines = start..start + handler.lines().count();
    let qualified = format!("`{}.{}`", cell, name);
    let all: Vec<serde_json::Value> = checked["errors"].as_array().cloned().unwrap_or_default();
    let (mine, elsewhere): (Vec<_>, Vec<_>) = all.into_iter().partition(|e| {
        let here = Path::new(e["file"].as_str().unwrap_or("")).file_name() == Path::new(&shown_file).file_name()
            && e["line"].as_u64().is_some_and(|l| lines.contains(&(l as usize)));
        here || e["message"].as_str().is_some_and(|m| m.contains(&qualified))
    });
    let frame_broken = mine.iter().any(|e| e["kind"] == "frame");
    let rejected = reject && !mine.is_empty();
    if rejected { let _ = std::fs::write(&file, &before); }
    let holes_left = if rejected { None } else { run_json(&dir, &["holes", &entry, "--json"]).and_then(|h| h["total_holes"].as_u64()) };
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "schema": "soma.fill/1", "signal": target, "file": shown_file, "written": !rejected,
            "frame": if frame_broken { "broken" } else { "kept" },
            "diagnostics": mine, "elsewhere": elsewhere.len(), "holes_left": holes_left,
        })).unwrap_or_default());
    } else {
        for e in &mine {
            eprintln!("error: {}", e["message"].as_str().unwrap_or(""));
            eprintln!("  --> {}:{}:{}", e["file"].as_str().unwrap_or(""), e["line"], e["col"]);
            eprintln!("  = code: {}", e["code"].as_str().unwrap_or(""));
        }
        if rejected {
            eprintln!("error: the body of `{}` was rejected ({} diagnostic(s)) — nothing was written", target, mine.len());
        } else {
            println!("{} `{}` in {} — frame {}, {} diagnostic(s) on this signal{}",
                if mine.is_empty() { "✓" } else { "✗" }, target, shown_file, if frame_broken { "BROKEN" } else { "kept" }, mine.len(),
                if elsewhere.is_empty() { String::new() } else { format!(" ({} elsewhere in the program: `soma check {}`)", elsewhere.len(), entry) });
            match holes_left {
                Some(n) => println!("{} hole{} left — `soma next {}`", n, if n == 1 { "" } else { "s" }, entry),
                None => println!("`soma next {}` says what is left", entry),
            }
        }
    }
    if !mine.is_empty() { std::process::exit(1); }
}

fn collect_cells(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') || name == "node_modules" || name == "target" { continue; }
        if p.is_dir() { collect_cells(&p, out); } else if name.ends_with(".cell") { out.push(p); }
    }
}

// ── next ────────────────────────────────────────────────────────────────

struct Item { kind: &'static str, target: String, reason: String, command: String, then: Option<String> }

thread_local! {
    /// the questions open for the owner of the law, as the last `first_thing` saw them
    static OPEN_QUESTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn first_thing(path: &PathBuf, only: Option<&str>, scope: Option<&super::scope::Scope>) -> Item {
    let (dir, entry) = dir_and_name(path);
    // `--cell`, and the perimeter of a law (RFC 0006, part C)
    let in_cell = |text: &str| only.is_none_or(|c| text.contains(c)) && scope.is_none_or(|s| s.names(text));
    let in_scope = |who: &str| scope.is_none_or(|s| s.has_signal(who));
    // 0. the questions for the owner of the law, counted before anything is
    // handed out: every answer says how many are open (4.0.0 counted them at
    // rank 5, and an answer of an earlier rank said 0)
    let law = run_json(&dir, &["law", &entry, "--todo", "--json"]);
    let open: Vec<&serde_json::Value> = law.as_ref().and_then(|l| l["questions"].as_array()).map(|qs| qs.iter().filter(|q| q["open"] == true)
        .filter(|q| scope.is_none_or(|s| q["rules"].as_array().into_iter().flatten().any(|r| s.rules.iter().any(|x| Some(x.as_str()) == r.as_str()))
            || q["signals"].as_array().into_iter().flatten().any(|g| g.as_str().is_some_and(|g| s.has_signal(g))))).collect()).unwrap_or_default();
    OPEN_QUESTIONS.with(|n| n.set(open.len()));
    // 1. the agent's setup
    if let Some(d) = run_json(&dir, &["doctor", "--json"]) {
        if let Some(c) = d["checks"].as_array().and_then(|cs| cs.iter().find(|c| c["ok"] == false)) {
            return Item { kind: "doctor", target: c["name"].as_str().unwrap_or("").to_string(), reason: c["found"].as_str().unwrap_or("").to_string(),
                command: c["fix"].as_str().unwrap_or("soma doctor").to_string(), then: None };
        }
    }
    // 2. an error of check
    let checked = run_json(&dir, &["check", &entry, "--json"]);
    let Some(checked) = checked else {
        // exit 2: nothing can be said of a program that does not load
        return Item { kind: "load", target: entry.clone(), reason: "the program does not load: `soma check` gave no report".to_string(), command: format!("soma check {}", entry), then: None };
    };
    // a program that does not parse is not "something to do" like another: exit 2
    if let Some(e) = checked["errors"].as_array().and_then(|es| es.iter().find(|e| e["code"].as_str().is_some_and(|c| c.starts_with("E1")))) {
        return Item { kind: "load", target: format!("{}:{}", e["file"].as_str().unwrap_or(&entry), e["line"]),
            reason: format!("the program does not load: {} [{}]", e["message"].as_str().unwrap_or(""), e["code"].as_str().unwrap_or("")),
            command: format!("soma check {}", entry), then: None };
    }
    if let Some(e) = checked["errors"].as_array().and_then(|es| es.iter().find(|e| in_cell(e["message"].as_str().unwrap_or("")) || only.is_none()).or(es.first())) {
        let more = checked["error_count"].as_u64().unwrap_or(1).saturating_sub(1);
        return Item { kind: "check", target: format!("{}:{}", e["file"].as_str().unwrap_or(&entry), e["line"]),
            reason: format!("{} [{}]{}", e["message"].as_str().unwrap_or(""), e["code"].as_str().unwrap_or(""), if more > 0 { format!(" — and {} more", more) } else { String::new() }),
            command: format!("soma check {}", entry), then: Some(format!("soma explain {}", e["code"].as_str().unwrap_or(""))) };
    }
    // 3. a failure of verify
    let verified = run_json(&dir, &["verify", &entry, "--json"]);
    if let Some(v) = &verified {
        if v["passed"] == false {
            let failing = v["state_machines"].as_array().into_iter().flatten().find_map(|m| m["checks"].as_array().into_iter().flatten()
                .find(|c| c["status"] == "fail").map(|c| (m["name"].as_str().unwrap_or("").to_string(), c["message"].as_str().unwrap_or("").to_string())));
            let (target, reason) = failing.unwrap_or_else(|| (entry.clone(), v["errors"].as_array().and_then(|e| e.first()).map(|e| e.to_string()).unwrap_or_else(|| v["verdict"].as_str().unwrap_or("verify failed").to_string())));
            return Item { kind: "verify", target, reason, command: format!("soma verify {}", entry), then: None };
        }
    }
    // 3b. what the evidence says AGAINST a rule — a refutation the project
    // keeps, a test under a rule that fails: a failure like those of verify
    if let Some(t) = law.as_ref().and_then(|l| l["todo"].as_array()).and_then(|t| t.iter().filter_map(|x| x.as_str())
        .find(|t| (t.contains(" is refuted by ") || t.contains(" fails — `soma test`")) && in_cell(t))) {
        let rule = t.split('"').nth(1).unwrap_or("").to_string();
        return Item { kind: "law", target: rule.clone(), reason: t.to_string(), command: format!("soma law {} \"{}\"", entry, rule), then: None };
    }
    // 4. before the freeze: what the skeleton does not state, ONE at a time —
    // the mirror's, then what the frames show (RFC 0006, part A). Stating
    // it, or freezing the skeleton as it is, closes it
    if !crate::checker::lock::path_for(path).exists() {
        let m = super::mirror::mirror(path);
        let mut gaps: Vec<(String, String)> = m.gaps.iter().filter(|g| only.is_none_or(|c| g.target.starts_with(&format!("{}.", c)) || g.kind == "no_property"))
            .filter(|g| scope.is_none_or(|s| s.has_signal(&g.target) || (g.kind == "no_invariant" && g.target.split_once('.').is_some_and(|(c, _)| s.cells.contains(c)))))
            .map(|g| (g.target.clone(), g.message.clone())).collect();
        for w in checked["warnings"].as_array().into_iter().flatten() {
            let text = w["message"].as_str().unwrap_or("");
            if (text.contains("a second path") || text.contains("goes stale") || text.contains("is `internal` and no signal") || text.contains("a held slot has one writer")) && in_cell(text) {
                gaps.push((text.split('`').nth(1).unwrap_or("").to_string(), text.to_string()));
            }
        }
        if m.skeletons > 0 && !gaps.is_empty() {
            return Item { kind: "skeleton", target: gaps[0].0.clone(),
                reason: format!("{} — {} of {} thing{} the skeleton, not frozen yet, does not state", gaps[0].1, 1, gaps.len(), if gaps.len() == 1 { "" } else { "s" }),
                command: format!("soma doctor --program {}", entry),
                then: Some(format!("soma skeleton {} --freeze   # once it says what you mean: from then on the bodies are written against it", entry)) };
        }
    }
    // 5. the questions for the owner of the law: they are not tasks, and
    // what waits for their answer is not handed out
    let texts = |q: &serde_json::Value, key: &str| -> Vec<String> { q[key].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect() };
    let waiting_rules: std::collections::BTreeSet<String> = open.iter().flat_map(|q| texts(q, "rules")).collect();
    let mut waiting_signals: std::collections::BTreeSet<String> = open.iter().flat_map(|q| texts(q, "signals")).collect();
    if let Some(l) = &law {
        for o in l["owed"].as_array().into_iter().flatten() {
            if waiting_rules.contains(o["rule"].as_str().unwrap_or("")) { waiting_signals.insert(o["signal"].as_str().unwrap_or("").to_string()); }
        }
        for r in l["rules"].as_array().into_iter().flatten() {
            if !waiting_rules.contains(r["id"].as_str().unwrap_or("")) { continue; }
            for sv in r["served"].as_array().into_iter().flatten() { waiting_signals.insert(sv["signal"].as_str().unwrap_or("").to_string()); }
        }
    }
    // 6. what the law still asks, of what waits for no answer
    if let Some(l) = &law {
        if let Some(t) = l["todo"].as_array().and_then(|t| t.iter().filter_map(|x| x.as_str())
            .find(|t| in_cell(t) && !waiting_rules.contains(t.split('"').nth(1).unwrap_or("")))) {
            let rule = t.split('"').nth(1).unwrap_or("").to_string();
            return Item { kind: "law", target: if rule.is_empty() { entry.clone() } else { rule.clone() }, reason: t.to_string(),
                command: if rule.is_empty() { format!("soma law {} --todo", entry) } else { format!("soma law {} \"{}\"", entry, rule) }, then: None };
        }
    }
    // 7. the next hole, among those that owe nothing under a question
    if let Some(h) = run_json(&dir, &["holes", &entry, "--json"]) {
        let total = h["total_holes"].as_u64().unwrap_or(0);
        if let Some(hole) = h["holes"].as_array().and_then(|hs| hs.iter().find(|x| only.is_none_or(|c| x["cell"] == c)
            && in_scope(x["signal"].as_str().unwrap_or(""))
            && !waiting_signals.contains(x["signal"].as_str().unwrap_or("")))) {
            let signal = hole["signal"].as_str().unwrap_or("").to_string();
            return Item { kind: "hole", reason: format!("a signal with no body yet — hole {} of {} ({} signals)", hole["order"], total, h["signals"]),
                command: format!("soma skeleton {} {}", entry, signal), then: Some(format!("soma fill {} {}   # its body on stdin", entry, signal)), target: signal };
        }
    }
    // 8. a failing test
    if let Some(t) = run_json(&dir, &["test", &entry, "--json"]) {
        // in a perimeter: the tests under its rules
        let of_scope: Option<std::collections::BTreeSet<String>> = scope.map(|s| law.as_ref().and_then(|l| l["rules"].as_array()).into_iter().flatten()
            .filter(|r| s.rules.iter().any(|x| Some(x.as_str()) == r["id"].as_str()))
            .flat_map(|r| r["tests"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(str::to_string)).collect::<Vec<_>>()).collect());
        if let Some(r) = t["results"].as_array().and_then(|rs| rs.iter().find(|r| r["status"] != "pass" && only.is_none_or(|c| r["cell"] == c)
            && of_scope.as_ref().is_none_or(|names| names.contains(r["cell"].as_str().unwrap_or(""))))) {
            return Item { kind: "test", target: r["cell"].as_str().unwrap_or("").to_string(),
                reason: format!("{} — {}", r["rule"].as_str().unwrap_or(""), r["message"].as_str().unwrap_or("")), command: format!("soma test {}", entry), then: None };
        }
    }
    // all that is left waits for the owner: this is not yours (exit 3)
    if let Some(q) = open.first() {
        return Item { kind: "question", target: q["id"].as_str().unwrap_or("").to_string(),
            reason: format!("{} — a question for the owner of the law, not a task: do not answer it in the code, and do not write the bodies that wait for it ({} question{} open)",
                q["text"].as_str().unwrap_or(""), open.len(), if open.len() == 1 { "" } else { "s" }),
            command: format!("soma law {} --questions", entry), then: None };
    }
    // 7. nothing
    let verdict = verified.as_ref().and_then(|v| v["verdict"].as_str()).unwrap_or("verify did not answer").to_string();
    Item { kind: "complete", target: entry.clone(), reason: format!("no gap, no error, no hole, no failing test — {}", verdict), command: String::new(), then: None }
}

/// `soma next [app.cell] [--cell C] [--json]`: the one thing to do now.
/// Exits 0 when the program is complete, 1 while something is left for
/// whoever writes it, 2 when the program does not load, 3 when all that is
/// left waits for the owner of the law.
pub fn cmd_next(path: &PathBuf, only: Option<&str>, json: bool, law: Option<&str>) {
    if !path.is_file() { fail(format!("{}: no such program — `soma next app.cell`", path.display())); }
    let scope = law.map(|l| super::scope::scope_or_exit(path, l));
    let item = first_thing(path, only, scope.as_ref());
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "schema": "soma.next/1", "kind": item.kind, "target": item.target, "reason": item.reason,
            "command": if item.command.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(item.command.clone()) }, "then": item.then,
            "for": if item.kind == "question" { "owner" } else { "agent" },
            "open_questions": OPEN_QUESTIONS.with(|n| n.get()),
        })).unwrap_or_default());
    } else if item.kind == "complete" {
        println!("complete  {}", item.target);
        println!("  {}", item.reason);
    } else {
        println!("{}  {}", item.kind, item.target);
        println!("  why:  {}", item.reason);
        println!("  do:   {}", item.command);
        if let Some(t) = &item.then { println!("  then: {}", t); }
        let open = OPEN_QUESTIONS.with(|n| n.get());
        if open > 0 && item.kind != "question" {
            println!("  ({} question{} open for the owner of the law — `soma law {} --questions`; what waits for an answer is not handed out)", open, if open == 1 { "" } else { "s" }, path.display());
        }
    }
    std::process::exit(match item.kind { "complete" => 0, "load" => 2, "question" => 3, _ => 1 });
}
