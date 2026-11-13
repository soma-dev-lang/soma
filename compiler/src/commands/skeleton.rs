//! `soma skeleton` — a program as its skeleton (SKELETON_DESIGN.md, phase 1).
//!
//! Memory, invariants, state machines, and a face whose signals each carry
//! their *frame* (what the handler reads, writes, calls, emits and which
//! machine edges it takes) — no handler bodies. The frames are inferred
//! from the bodies by `checker::frame`.
//!
//!   soma skeleton app.cell               the whole map
//!   soma skeleton app.cell Cell.signal   the brief for one handler:
//!                                        what it may touch, and the
//!                                        contracts of what it may call

use std::path::PathBuf;
use crate::ast::*;
use crate::checker::frame::{self, Bone, CellSkeleton, Frame};
use super::describe::{format_literal, format_params, format_promise, format_return, format_type, format_variant};
use super::{read_source, lex_with_location, parse_with_location, resolve_imports};

const WIDTH: usize = 100;

thread_local! {
    /// The entry file of the brief being printed: its directory is where
    /// `file:line` references are looked up.
    static BRIEF_ENTRY: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

pub fn cmd_skeleton(path: &PathBuf, signal: Option<&str>, json: bool, split: bool) {
    BRIEF_ENTRY.with(|p| *p.borrow_mut() = Some(path.clone()));
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    if split {
        let skeletons = frame::build_faithful(&program);
        let (text, unframed) = render_split(&program, &skeletons, &source);
        print!("{}", text);
        for u in &unframed {
            eprintln!("warning: {} — no frame can declare it; `soma check` will refuse that body until the target is written out", u);
        }
        return;
    }
    let skeletons = frame::build(&program);
    // a skeleton that left its lock: the map still prints — it is how one
    // looks — but a brief is what a body is written from, and is refused
    let broken = crate::checker::lock::check(&program, path, &source);
    if !broken.is_empty() {
        for b in &broken { eprintln!("{}: {}", if signal.is_some() { "error" } else { "warning" }, b); }
        if signal.is_some() {
            eprintln!("error: no brief is given for a program its skeleton.lock refuses");
            std::process::exit(1);
        }
    }
    // `soma skeleton app.cell Orders`: one cell of the map
    if let Some(sk) = signal.filter(|t| !t.contains('.') && !skeletons.iter().any(|sk| sk.bones.iter().any(|b| b.name == **t)))
        .and_then(|t| skeletons.iter().find(|sk| sk.cell.name == t)) {
        if json {
            let whole = map_json(&program, &skeletons, &file_str);
            let one = whole.get("cells").and_then(|c| c.as_array()).and_then(|cs| cs.iter().find(|c| c.get("name").and_then(|n| n.as_str()) == Some(sk.cell.name.as_str()))).cloned();
            println!("{}", serde_json::to_string_pretty(&one.unwrap_or(whole)).unwrap());
        } else {
            print!("{}", render_cell_view(&program, &source, &skeletons, sk));
        }
        return;
    }
    if let Some(target) = signal {
        match find_bone(&skeletons, target) {
            Ok((sk, bone)) => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&brief_json(&program, &source, &skeletons, sk, bone)).unwrap());
                } else {
                    print!("{}", render_brief(&program, &source, &skeletons, sk, bone));
                }
            }
            Err(msg) => {
                eprintln!("error: {}", msg);
                std::process::exit(1);
            }
        }
        return;
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&map_json(&program, &skeletons, &file_str)).unwrap());
    } else {
        print!("{}", render_map(&program, &skeletons, &file_str));
    }
}

/// `soma skeleton app.cell --index`: one line per cell, then who calls whom.
pub fn cmd_index(path: &PathBuf) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    let skeletons = frame::build(&program);
    for b in crate::checker::lock::check(&program, path, &source) { eprintln!("warning: {}", b); }
    print!("{}", render_index(&program, &source, &skeletons));
}

pub fn render_index(program: &Program, source: &str, skeletons: &[CellSkeleton]) -> String {
    let width = skeletons.iter().map(|sk| sk.cell.name.chars().count()).max().unwrap_or(0);
    let mut out = String::new();
    for sk in skeletons {
        // (a plain cell has no face to speak of: its handlers are what it offers)
        let faced = sk.bones.iter().filter(|b| b.in_face).count();
        let (signals, word) = if faced == 0 && !sk.cell.skeleton { (sk.bones.len(), "handler") } else { (faced, "signal") };
        let holes = sk.bones.iter().filter(|b| b.hole).count();
        let mut parts = vec![format!("{} {}{}", signals, word, if signals == 1 { "" } else { "s" })];
        if holes > 0 { parts.push(format!("{} hole{}", holes, if holes == 1 { "" } else { "s" })); }
        for section in &sk.cell.sections {
            if let Section::State(sm) = &section.node {
                let mut states: Vec<&str> = vec![sm.initial.as_str()];
                for t in &sm.transitions { for s in [t.node.from.as_str(), t.node.to.as_str()] { if s != "*" && !states.contains(&s) { states.push(s); } } }
                parts.push(format!("machine {} ({} states)", sm.name, states.len()));
            }
        }
        let what = cell_span(program, &sk.cell.name).map(|sp| intent_of(sp, source, false)).unwrap_or_default();
        let what = what.first().map(|c| format!("   // {}", c)).unwrap_or_default();
        out.push_str(&format!("{:<w$}  {}{}\n", sk.cell.name, parts.join(", "), what, w = width));
    }
    // cell -> cell, with the signals called and the events heard
    let mut links: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut link = |from: &str, to: &str, what: String| {
        if from == to { return; }
        match links.iter_mut().find(|(f, t, _)| f == from && t == to) {
            Some((_, _, ws)) => if !ws.contains(&what) { ws.push(what) },
            None => links.push((from.to_string(), to.to_string(), vec![what])),
        }
    };
    for sk in skeletons {
        let frames = sk.bones.iter().map(|b| &b.frame).chain(sk.schedules.iter().map(|s| &s.frame));
        for f in frames {
            for c in &f.calls {
                if let Some((cell, name)) = c.split_once('.') {
                    if skeletons.iter().any(|o| o.cell.name == cell) { link(&sk.cell.name, cell, name.to_string()); }
                }
            }
            for e in &f.emits {
                for o in skeletons.iter().filter(|o| o.bones.iter().any(|b| &b.name == e && !b.no_handler)) { link(&sk.cell.name, &o.cell.name, format!("emit {}", e)); }
            }
        }
    }
    if !links.is_empty() { out.push('\n'); }
    for (from, to, what) in &links {
        out.push_str(&wrap(&format!("{} -> {}: ", from, to), what, "    "));
    }
    out
}

/// One cell of the map, as its author wrote it: the comments above the
/// cell, its slots and its signals, and who calls each signal from outside.
pub fn render_cell_view(program: &Program, source: &str, skeletons: &[CellSkeleton], sk: &CellSkeleton) -> String {
    let mut out = String::new();
    if let Some(sp) = cell_span(program, &sk.cell.name) {
        for c in intent_of(sp, source, false) { out.push_str(&format!("// {}\n", c)); }
    }
    for line in render_one(sk).lines() {
        let body = line.trim_start();
        let indent = &line[..line.len() - body.len()];
        // a signal: its comments above it, its callers under it
        let bone = sk.bones.iter().find(|b| body.starts_with(&format!("signal {}(", b.name)) || body.starts_with(&format!("tool {}(", b.name)));
        if let Some(b) = bone {
            for c in intent_of(b.decl_span, source, true) { out.push_str(&format!("{}// {}\n", indent, c)); }
            out.push_str(line); out.push('\n');
            let callers: Vec<String> = callers_of(skeletons, sk, b).into_iter().filter(|c| !c.starts_with(&format!("{}.", sk.cell.name))).collect();
            if !callers.is_empty() { out.push_str(&wrap(&format!("{}    // called by ", indent), &callers, &format!("{}    //   ", indent))); }
            continue;
        }
        // the helpers, with what they take (a body that calls one needs it)
        if body.starts_with("// helpers (folded into the frames above)") {
            out.push_str(&format!("{}// helpers (their effects are in the frames of the signals that call them):\n", indent));
            for h in &sk.helpers {
                let on = sk.cell.sections.iter().find_map(|s| match &s.node { Section::OnSignal(on) if &on.signal_name == h => Some(on), _ => None });
                if let Some(on) = on { out.push_str(&format!("{}//   on {}({})\n", indent, h, format_params(&on.params))); }
            }
            continue;
        }
        if body.starts_with("//   ") && sk.helpers.iter().any(|h| body.contains(h.as_str())) { continue; }
        // a slot: the comment on its line
        let slot = body.split_once(':').map(|(n, _)| n).filter(|n| !n.contains(' ')).and_then(|n| slot_span(sk.cell, n));
        let note = slot.map(|sp| intent_of(sp, source, true)).unwrap_or_default();
        out.push_str(line);
        if !note.is_empty() { out.push_str(&format!("   // {}", note.join(" "))); }
        out.push('\n');
    }
    out
}

// ── the whole map ────────────────────────────────────────────────────

pub fn render_map(program: &Program, skeletons: &[CellSkeleton], file: &str) -> String {
    let declared = skeletons.iter().filter(|sk| sk.cell.skeleton).count();
    let frames = if declared == skeletons.len() && declared > 0 { "frames as declared" }
        else if declared == 0 { "frames inferred from the bodies" }
        else { "frames declared for `skeleton`s, inferred for `cell`s" };
    let mut out = format!("// {} — skeleton: structure and frames, no handler bodies ({})\n", file, frames);
    for (cell, _) in frame::cells_with_parents(program) {
        if cell.kind != CellKind::Type { continue; }
        for section in &cell.sections {
            if let Section::Variants(v) = &section.node {
                let variants: Vec<String> = v.variants.iter().map(|x| format_variant(&x.node)).collect();
                out.push_str(&format!("\ncell type {} {{ variants {{ {} }} }}\n", cell.name, variants.join("  ")));
            }
        }
    }
    for sk in skeletons {
        out.push('\n');
        render_cell(sk, true, &mut out);
    }
    out
}

/// One cell as its skeleton, with the notes (`// hole`, the helpers).
pub fn render_one(sk: &CellSkeleton) -> String {
    let mut out = String::new();
    render_cell(sk, true, &mut out);
    out
}

/// A signal's frame, and whether it is a hole, for `describe --json`.
pub fn signal_json(b: &Bone) -> serde_json::Value {
    serde_json::json!({ "hole": b.hole, "frame": frame_json(&b.frame) })
}

/// One skeleton as text, without what changes as the flesh is written
/// (which signals are holes, which helpers exist): what `skeleton.lock` keeps.
pub fn render_frozen(sk: &CellSkeleton) -> String {
    let mut out = String::new();
    render_cell(sk, false, &mut out);
    out
}

fn render_cell(sk: &CellSkeleton, notes: bool, out: &mut String) {
    let cell = sk.cell;
    let kind = if cell.kind == CellKind::Agent { "skeleton agent" } else { "skeleton" };
    match sk.parent {
        Some(p) => out.push_str(&format!("{} {} {{   // interior of {}\n", kind, cell.name, p)),
        None => out.push_str(&format!("{} {} {{\n", kind, cell.name)),
    }
    for section in &cell.sections {
        match &section.node {
            Section::Memory(mem) => {
                out.push_str("    memory {\n");
                for slot in &mem.slots { out.push_str(&format!("        {}\n", slot_line(&slot.node))); }
                for inv in &mem.invariants { out.push_str(&format!("        invariant {}\n", render_expr(&inv.node))); }
                out.push_str("    }\n");
            }
            Section::State(sm) => {
                let ty = sm.state_type.as_deref().map(|t| format!(": {}", t)).unwrap_or_default();
                out.push_str(&format!("    state {}{} {{\n        initial: {}\n", sm.name, ty, sm.initial));
                for t in &sm.transitions { out.push_str(&format!("        {}\n", edge_line(&t.node))); }
                out.push_str("    }\n");
            }
            _ => {}
        }
    }
    let promises: Vec<String> = cell.sections.iter()
        .filter_map(|s| if let Section::Face(f) = &s.node { Some(f) } else { None })
        .flat_map(|f| f.declarations.iter())
        .filter_map(|d| if let FaceDecl::Promise(p) = &d.node { Some(promise_line(&p.constraint.node)) } else { None })
        .collect();
    if !sk.bones.is_empty() || !promises.is_empty() {
        out.push_str("    face {\n");
        for b in sk.bones.iter().filter(|b| notes || b.in_face) { render_bone(b, notes, out); }
        for p in &promises { out.push_str(&format!("        promise {}\n", p)); }
        out.push_str("    }\n");
    }
    // a schedule is a body: in a skeleton it is `every 5s { tick() }`,
    // calling a signal that carries the frame
    for s in &sk.schedules {
        let inline = frame_inline(&s.frame);
        if notes { out.push_str(&format!("    // {} {{ … }}  {}\n", s.label, if inline.is_empty() { "pure" } else { inline.as_str() })); }
        else { out.push_str(&format!("    {} {{ {} }}\n", s.label, s.frame.calls.iter().map(|c| format!("{}()", c)).collect::<Vec<_>>().join("  "))); }
    }
    if notes && !sk.helpers.is_empty() {
        out.push_str(&wrap("    // helpers (folded into the frames above): ", &sk.helpers, "    //   "));
    }
    out.push_str("}\n");
}

fn slot_line(slot: &MemorySlot) -> String {
    let props: Vec<String> = slot.properties.iter().map(|p| match &p.node {
        MemoryProperty::Flag(n) => n.clone(),
        MemoryProperty::Param(pp) => format!("{}({})", pp.name,
            pp.values.iter().map(|v| match &v.node {
                // as the lexer reads them back
                Literal::Duration(d) => format!("{}{}", d.value, match d.unit {
                    DurationUnit::Milliseconds => "ms", DurationUnit::Seconds => "s", DurationUnit::Minutes => "min",
                    DurationUnit::Hours => "h", DurationUnit::Days => "d", DurationUnit::Years => "years",
                }),
                other => format_literal(other),
            }).collect::<Vec<_>>().join(", ")),
    }).collect();
    let props = if props.is_empty() { String::new() } else { format!(" [{}]", props.join(", ")) };
    format!("{}: {}{}", slot.name, format_type(&slot.ty.node), props)
}

fn edge_line(t: &Transition) -> String {
    let except = if t.except.is_empty() { String::new() } else { format!(" except [{}]", t.except.join(", ")) };
    let guard = t.guard.as_ref().map(|g| format!(" {{ guard {{ {} }} }}", render_expr(&g.node))).unwrap_or_default();
    // an edge's effect block is a body: it belongs to the flesh
    let effect = if t.effect.is_empty() { "" } else { "   // has an effect block" };
    format!("{} -> {}{}{}{}", t.from, t.to, except, guard, effect)
}

fn promise_line(c: &Constraint) -> String {
    match c {
        Constraint::Descriptive(s) => format!("\"{}\"", s),
        _ => format_promise(c),
    }
}

fn bone_head(b: &Bone) -> String {
    let ret = b.return_type.map(|r| format!(" -> {}", format_type(&r.node))).unwrap_or_default();
    format!("{} {}({}){}", if b.tool { "tool" } else { "signal" }, b.name, format_params(b.params), ret)
}

fn bone_note(b: &Bone) -> &'static str {
    if b.hole { "   // hole" } else if b.no_handler { "   // no handler" } else if !b.in_face { "   // not in face" } else { "" }
}

pub fn frame_inline(f: &Frame) -> String {
    f.clauses().iter().map(|(kw, items)| format!("{} {}", kw, items.join(", "))).collect::<Vec<_>>().join("  ")
}

/// The part of an inferred frame a skeleton can declare, and a comment
/// for the part it cannot: a transition whose target (or machine) is only
/// known at run time has no `moves` clause.
fn declarable(f: &Frame) -> (Frame, &'static str) {
    let mut frame = f.clone();
    let unknown = frame.dynamic_moves || frame.moves.iter().any(|(m, from, _)| m == "?" || from == "?");
    frame.dynamic_moves = false;
    frame.moves.retain(|(m, from, _)| m != "?" && from != "?");
    (frame, if unknown { "   // + a transition() the analysis cannot name (target or machine known at run time)" } else { "" })
}

fn render_bone(b: &Bone, notes: bool, out: &mut String) {
    let head = bone_head(b);
    let (frame, unknown) = declarable(&b.frame);
    let inline = frame_inline(&frame);
    let note = if notes { format!("{}{}", bone_note(b), unknown) } else { String::new() };
    if !notes {
        // the lock: one line per signal, so a difference names the signal
        let mut line = format!("        {}", head);
        for c in b.requires { line.push_str(&format!("  requires {}", render_expr(&c.node))); }
        for c in b.ensures { line.push_str(&format!("  ensures {}", render_expr(&c.node))); }
        if !inline.is_empty() { line.push_str(&format!("  {}", inline)); }
        out.push_str(&line);
        out.push('\n');
        return;
    }
    if b.requires.is_empty() && b.ensures.is_empty() && 8 + head.len() + 2 + inline.len() <= WIDTH {
        if inline.is_empty() { out.push_str(&format!("        {}{}\n", head, note)); }
        else { out.push_str(&format!("        {}  {}{}\n", head, inline, note)); }
        return;
    }
    out.push_str(&format!("        {}{}\n", head, note));
    for c in b.requires { out.push_str(&format!("            requires {}\n", render_expr(&c.node))); }
    for c in b.ensures { out.push_str(&format!("            ensures {}\n", render_expr(&c.node))); }
    for (kw, items) in frame.clauses() {
        out.push_str(&wrap(&format!("            {} ", kw), &items, "                "));
    }
}

/// `first` then the items, comma-separated, broken at WIDTH onto lines
/// that start with `cont`.
fn wrap(first: &str, items: &[String], cont: &str) -> String {
    let mut out = String::new();
    let mut line = first.to_string();
    let mut fresh = true;
    for (i, item) in items.iter().enumerate() {
        let piece = if i + 1 < items.len() { format!("{}, ", item) } else { item.clone() };
        if !fresh && line.len() + piece.trim_end().len() > WIDTH {
            out.push_str(line.trim_end());
            out.push('\n');
            line = cont.to_string();
        }
        line.push_str(&piece);
        fresh = false;
    }
    out.push_str(line.trim_end());
    out.push('\n');
    out
}

// ── the brief for one handler ────────────────────────────────────────

/// `Cell.signal` (or `signal`) as the (cell, signal) it names.
pub fn locate(skeletons: &[CellSkeleton], target: &str) -> Result<(String, String), String> {
    find_bone(skeletons, target).map(|(sk, b)| (sk.cell.name.clone(), b.name.clone()))
}

fn find_bone<'s, 'a>(skeletons: &'s [CellSkeleton<'a>], target: &str) -> Result<(&'s CellSkeleton<'a>, &'s Bone<'a>), String> {
    let (cell, name) = match target.split_once('.') {
        Some((c, n)) => (Some(c), n),
        None => (None, target),
    };
    let found: Vec<(&CellSkeleton, &Bone)> = skeletons.iter()
        .filter(|sk| cell.map_or(true, |c| sk.cell.name == c))
        .flat_map(|sk| sk.bones.iter().filter(|b| b.name == name).map(move |b| (sk, b)))
        .collect();
    match found.len() {
        1 => Ok(found[0]),
        0 => {
            if let Some(sk) = skeletons.iter().find(|sk| cell.map_or(true, |c| sk.cell.name == c) && sk.helpers.iter().any(|h| h == name)) {
                return Err(format!("`{}` is a helper of `{}`: it has no frame of its own, its effects are in the frames of the signals that call it", name, sk.cell.name));
            }
            let all: Vec<String> = skeletons.iter()
                .flat_map(|sk| sk.bones.iter().map(move |b| format!("{}.{}", sk.cell.name, b.name))).collect();
            let hint = crate::checker::names::suggest(target, all.iter())
                .map(|s| format!(" Did you mean `{}`?", s)).unwrap_or_default();
            Err(format!("no signal `{}`.{}\n  signals: {}", target, hint, all.join(", ")))
        }
        _ => Err(format!("`{}` is a signal of {}: write `Cell.{}`", name,
            found.iter().map(|(sk, _)| format!("`{}`", sk.cell.name)).collect::<Vec<_>>().join(" and "), name)),
    }
}

/// The machine a frame's `moves` label names, seen from `sk`.
fn machine_of<'s>(skeletons: &'s [CellSkeleton], sk: &'s CellSkeleton, label: &str) -> Option<&'s StateMachineSection> {
    let (cell, name) = match label.split_once('.') {
        Some((c, n)) => (skeletons.iter().find(|s| s.cell.name == c)?.cell, Some(n)),
        None => (sk.cell, if label.is_empty() { None } else { Some(label) }),
    };
    cell.sections.iter()
        .filter_map(|s| if let Section::State(sm) = &s.node { Some(sm) } else { None })
        .find(|sm| name.map_or(true, |n| sm.name == n))
}

fn slot_of<'s>(sk: &'s CellSkeleton, name: &str) -> Option<(&'s MemorySlot, Vec<String>)> {
    for s in &sk.cell.sections {
        if let Section::Memory(mem) = &s.node {
            if let Some(slot) = mem.slots.iter().find(|sl| sl.node.name == name) {
                let invariants = mem.invariants.iter().map(|i| render_expr(&i.node))
                    .filter(|text| text.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| w == name))
                    .collect();
                return Some((&slot.node, invariants));
            }
        }
    }
    None
}

/// The bone a `calls` entry names (`f`, `Cell.f`), seen from `sk`.
fn callee_of<'s, 'a>(skeletons: &'s [CellSkeleton<'a>], sk: &'s CellSkeleton<'a>, call: &str) -> Option<&'s Bone<'a>> {
    let (cell, name) = match call.split_once('.') {
        Some((c, n)) => (c, n),
        None => (sk.cell.name.as_str(), call),
    };
    skeletons.iter().find(|s| s.cell.name == cell)?.bones.iter().find(|b| b.name == name)
}

// ── what the source says and the AST does not keep ───────────────────

/// The text of the file a span belongs to (the main file or an imported
/// one), as characters, with the span's bounds in it.
fn file_of(span: Span, main: &str) -> Option<(Vec<char>, usize, usize)> {
    let (text, start): (String, usize) = match crate::interpreter::resolve_import_pos(span.start) {
        Some((_, text, local)) => (text.to_string(), local),
        None => (main.to_string(), span.start),
    };
    let chars: Vec<char> = text.chars().collect();
    let end = start + span.end.saturating_sub(span.start);
    if start > end || end > chars.len() { return None; }
    Some((chars, start, end))
}

fn comment_text(line: &str) -> Option<String> {
    let at = line.find("//")?;
    // `//` inside a string is not a comment
    if line[..at].matches('"').count() % 2 == 1 { return None; }
    // rules and boxes around a heading (`// ── Orders: … ───`) are not words
    let text = line[at + 2..].trim().trim_matches(|c: char| matches!(c, '─' | '-' | '=' | '*' | '#' | '│' | '═') || c.is_whitespace()).to_string();
    if text.chars().any(|c| c.is_alphanumeric()) { Some(text) } else { None }
}

/// A comment that only makes sense next to the one above it.
fn leans_on_neighbour(comment: &str) -> bool {
    let c = comment.trim().to_lowercase();
    ["the same", "same as", "same, ", "as above", "like the above", "likewise", "idem", "ditto"].iter().any(|p| c.starts_with(p))
}

/// The comments written for a declaration: the `//` lines right above it
/// and the ones that trail its own lines. They are the only place a
/// skeleton says what a signal is FOR and what its answer means.
fn intent_of(span: Span, main: &str, trailing: bool) -> Vec<String> {
    let Some((chars, start, end)) = file_of(span, main) else { return Vec::new() };
    let line_start = |i: usize| chars[..i].iter().rposition(|c| *c == '\n').map_or(0, |p| p + 1);
    let line_end = |i: usize| chars[i..].iter().position(|c| *c == '\n').map_or(chars.len(), |p| i + p);
    let mut above: Vec<String> = Vec::new();
    let mut at = line_start(start);
    // a `/* … */` block that ends right above the declaration
    if at > 0 {
        let before: String = chars[..at].iter().collect();
        let trimmed = before.trim_end();
        if trimmed.ends_with("*/") {
            if let Some(open) = trimmed.rfind("/*") {
                let block: Vec<String> = trimmed[open + 2..trimmed.len() - 2].lines()
                    .map(|l| l.trim().trim_start_matches('*').trim().to_string()).filter(|l| l.chars().any(|c| c.is_alphanumeric())).collect();
                // only a block that follows a line break (not one trailing other code)
                if trimmed[..open].trim_end_matches(|c: char| c == ' ' || c == '\t').ends_with('\n') || open == 0 {
                    let mut out = block;
                    if trailing {
                        let text: String = chars[line_start(start)..line_end(end.max(start + 1) - 1)].iter().collect();
                        out.extend(text.lines().filter_map(comment_text));
                    }
                    return out;
                }
            }
        }
    }
    // a cell's heading comment may stand one blank line above it
    if !trailing && at > 0 {
        let prev = line_start(at - 1);
        if chars[prev..at - 1].iter().all(|c| c.is_whitespace()) { at = prev; }
    }
    while at > 0 {
        let prev = line_start(at - 1);
        let line: String = chars[prev..at - 1].iter().collect();
        if !line.trim_start().starts_with("//") { break; }
        if let Some(t) = comment_text(&line) { above.push(t); }
        at = prev;
    }
    above.reverse();
    if trailing {
        let text: String = chars[line_start(start)..line_end(end.max(start + 1) - 1)].iter().collect();
        above.extend(text.lines().filter_map(comment_text));
    } else {
        let first: String = chars[start..line_end(start)].iter().collect();
        above.extend(comment_text(&first));
    }
    above
}

/// `const NAME = literal` lines of a cell, from its source (the parser
/// replaces a constant by its literal everywhere).
fn consts_of(cell_span: Span, main: &str) -> Vec<String> {
    let Some((chars, start, end)) = file_of(cell_span, main) else { return Vec::new() };
    let text: String = chars[start..end].iter().collect();
    let mut depth = 0i32;
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if depth == 1 {
            if let Some(rest) = t.strip_prefix("const ") {
                out.push(rest.split("//").next().unwrap_or(rest).trim().to_string());
            }
        }
        depth += t.matches('{').count() as i32 - t.matches('}').count() as i32;
    }
    out
}

fn cell_span(program: &Program, name: &str) -> Option<Span> {
    fn rec(cells: &[Spanned<CellDef>], name: &str) -> Option<Span> {
        for c in cells {
            if c.node.name == name && c.node.kind != CellKind::Flesh { return Some(c.span); }
            for s in &c.node.sections {
                if let Section::Interior(i) = &s.node { if let Some(sp) = rec(&i.cells, name) { return Some(sp); } }
            }
        }
        None
    }
    rec(&program.cells, name)
}

fn slot_span(cell: &CellDef, name: &str) -> Option<Span> {
    cell.sections.iter().find_map(|s| match &s.node {
        Section::Memory(m) => m.slots.iter().find(|sl| sl.node.name == name).map(|sl| sl.span),
        _ => None,
    })
}

/// The sum / record types these texts name, and the types THOSE name, as
/// `Name = Variant { … } | …`, in declaration order.
pub fn types_closure(program: &Program, texts: &[String]) -> Vec<String> {
    let all: Vec<(&CellDef, String)> = frame::cells_with_parents(program).into_iter().map(|(c, _)| c).filter(|c| c.kind == CellKind::Type)
        .filter_map(|c| c.sections.iter().find_map(|s| match &s.node {
            Section::Variants(v) => Some((c, v.variants.iter().map(|x| format_variant(&x.node)).collect::<Vec<_>>().join(" | "))),
            _ => None,
        })).collect();
    let names = |t: &str, name: &str| t.split(|ch: char| !(ch.is_alphanumeric() || ch == '_')).any(|w| w == name);
    let mut picked: Vec<usize> = Vec::new();
    let mut frontier: Vec<String> = texts.to_vec();
    while let Some(text) = frontier.pop() {
        for (i, (c, body)) in all.iter().enumerate() {
            if !picked.contains(&i) && names(&text, &c.name) {
                picked.push(i);
                frontier.push(body.clone());
            }
        }
    }
    picked.sort();
    picked.into_iter().map(|i| format!("{} = {}", all[i].0.name, all[i].1)).collect()
}

fn types_named(program: &Program, texts: &[String]) -> Vec<String> {
    types_closure(program, texts)
}

/// A machine, whole: a body that moves or reads it needs every state.
fn machine_lines(label: &str, sm: &StateMachineSection, source: &str) -> Vec<String> {
    let name = if label.is_empty() { sm.name.clone() } else { label.to_string() };
    let mut lines = vec![format!("{} — one instance per id (the String or Int you pass); initial `{}`", name, sm.initial)];
    lines.push(format!("    get_status(id) is () until the id's first transition (edition 2026: `{}`) — write `get_status(id) ?? \"{}\"`", sm.initial, sm.initial));
    for t in &sm.transitions {
        let note = intent_of(t.span, source, true);
        lines.push(format!("    {}{}", edge_line(&t.node).replace("   // has an effect block", "   (runs an effect block)"),
            if note.is_empty() { String::new() } else { format!("   // {}", note.join(" ")) }));
    }
    lines
}

pub fn render_brief(program: &Program, source: &str, skeletons: &[CellSkeleton], sk: &CellSkeleton, b: &Bone) -> String {
    let mut out = String::new();
    // what the cell is, in its author's words
    if let Some(sp) = cell_span(program, &sk.cell.name) {
        for c in intent_of(sp, source, false).iter().rev().take(3).collect::<Vec<_>>().into_iter().rev() {
            let own = format!("{}:", sk.cell.name);
            out.push_str(&format!("// {}: {}\n", sk.cell.name, c.strip_prefix(&own).unwrap_or(c).trim()));
        }
    }
    out.push_str(&format!("{}   // {}{}\n", bone_head(b), sk.cell.name,
        if b.hole { ", hole" } else if b.no_handler { ", no handler" } else if !b.in_face { ", not in face" } else { "" }));
    let mut intent = intent_of(b.decl_span, source, true);
    // "The same, paid with points." says nothing without the comment it
    // leans on: the declaration just above brings its own
    if intent.first().is_some_and(|c| leans_on_neighbour(c)) {
        let above = sk.bones.iter().filter(|x| x.in_face && x.decl_span.start < b.decl_span.start).max_by_key(|x| x.decl_span.start);
        if let Some(prev) = above {
            let theirs = intent_of(prev.decl_span, source, true);
            if !theirs.is_empty() {
                intent.push(format!("(`{}`, declared just above, says:)", prev.name));
                intent.extend(theirs.into_iter().map(|c| format!("    {}", c)));
            }
        }
    }
    // a tool's description is what the model is told it does
    if b.tool {
        let described = sk.cell.sections.iter().find_map(|s| match &s.node {
            Section::Face(f) => f.declarations.iter().find_map(|d| match &d.node { FaceDecl::Tool(t) if t.name == b.name => t.description.clone(), _ => None }),
            _ => None,
        });
        if let Some(text) = described { intent.insert(0, text); }
    }
    for c in &intent { out.push_str(&format!("    // {}\n", c)); }
    // where it is written: the declaration, and the body when there is one
    if let Some(path) = BRIEF_ENTRY.with(|p| p.borrow().clone()) {
        let refs = super::refs::collect(&path, &sk.cell.name, &b.name);
        let places: Vec<String> = refs.iter().filter(|r| matches!(r.what, "declared" | "body")).map(|r| format!("{} {}:{}", r.what, r.file, r.line)).collect();
        if !places.is_empty() { out.push_str(&format!("    // ({})\n", places.join(", "))); }
    }
    if intent.is_empty() && b.hole {
        // the brief can only pass on what the skeleton says
        out.push_str("    // (no comment on this signal in the skeleton: what it must do and what its answer means are not written down — ask, or read its callers)\n");
    }
    for c in b.requires { out.push_str(&format!("    requires {}\n", render_expr(&c.node))); }
    for c in b.ensures { out.push_str(&format!("    ensures {}\n", render_expr(&c.node))); }
    let label = |out: &mut String, kw: &str, lines: Vec<String>| {
        for (i, l) in lines.iter().enumerate() { out.push_str(&format!("{:<10} {}\n", if i == 0 { kw } else { "" }, l)); }
    };
    if let Some(sp) = cell_span(program, &sk.cell.name) {
        let consts = consts_of(sp, source);
        if !consts.is_empty() { label(&mut out, "const", consts); }
    }
    // the types its signature and its callees' signatures name
    let mut texts: Vec<String> = vec![bone_head(b)];
    texts.extend(b.frame.calls.iter().filter_map(|c| callee_of(skeletons, sk, c)).map(bone_head));
    // …and the types of the slots it reads or writes: what it stores must be one
    for section in &sk.cell.sections {
        if let Section::Memory(m) = &section.node {
            texts.extend(m.slots.iter().filter(|sl| b.frame.reads.contains(&sl.node.name) || b.frame.writes.contains(&sl.node.name)).map(|sl| slot_line(&sl.node)));
        }
    }
    let mut types = types_named(program, &texts);
    if !types.is_empty() {
        types.push("    (build one: `Name { field: value }` or `Name(value)`; take one apart: `match v { Name { field } -> … }`)".to_string());
        label(&mut out, "types", types);
    }
    if b.frame.is_pure() {
        out.push_str("pure: reads and writes no memory, calls no handler, moves no machine\n");
        let callers = callers_of(skeletons, sk, b);
        if !callers.is_empty() {
            for (i, c) in callers.iter().enumerate() { out.push_str(&format!("{:<10} {}\n", if i == 0 { "called by" } else { "" }, c)); }
        }
        let emitters: Vec<String> = skeletons.iter().flat_map(|o| o.bones.iter().filter(|x| x.frame.emits.contains(&b.name)).map(move |x| format!("{}.{}", o.cell.name, x.name))).collect();
        for (i, c) in emitters.iter().enumerate() { out.push_str(&format!("{:<10} {}\n", if i == 0 { "emitted by" } else { "" }, c)); }
        out.push_str(&stub_line(sk, b));
        return out;
    }
    let mut section = |kw: &str, lines: Vec<String>| {
        for (i, l) in lines.iter().enumerate() {
            out.push_str(&format!("{:<10} {}\n", if i == 0 { kw } else { "" }, l));
        }
    };
    let slots = |names: &std::collections::BTreeSet<String>| -> Vec<String> {
        let mut lines = Vec::new();
        for n in names {
            match slot_of(sk, n) {
                Some((slot, invariants)) => {
                    let note = slot_span(sk.cell, n).map(|sp| intent_of(sp, source, true)).unwrap_or_default();
                    lines.push(if note.is_empty() { slot_line(slot) } else { format!("{}   // {}", slot_line(slot), note.join(" ")) });
                    for inv in invariants { lines.push(format!("    invariant {}", inv)); }
                }
                None => match machine_of(skeletons, sk, n) {
                    // (the machine itself is printed below, whole)
                    Some(_) => lines.push(format!("{}   (state machine: see `machine`)", n)),
                    None => lines.push(n.clone()),
                },
            }
        }
        lines
    };
    if !b.frame.reads.is_empty() { section("reads", slots(&b.frame.reads)); }
    if !b.frame.writes.is_empty() { section("writes", slots(&b.frame.writes)); }
    if !b.frame.moves.is_empty() || b.frame.dynamic_moves {
        let mut lines = Vec::new();
        for (label, from, to) in &b.frame.moves {
            let guard = machine_of(skeletons, sk, label)
                .and_then(|sm| sm.transitions.iter().find(|t| &t.node.from == from && &t.node.to == to))
                .and_then(|t| t.node.guard.as_ref())
                .map(|g| format!("   guard {}", render_expr(&g.node))).unwrap_or_default();
            let prefix = if label.is_empty() { String::new() } else { format!("{}: ", label) };
            lines.push(format!("{}{} -> {}{}", prefix, from, to, guard));
        }
        if b.frame.dynamic_moves { lines.push("?   (a transition whose target is computed at run time)".to_string()); }
        section("moves", lines);
    }
    // every machine the body reads or moves, whole
    let mut labels: Vec<String> = b.frame.moves.iter().map(|(l, _, _)| l.clone()).collect();
    labels.extend(b.frame.reads.iter().filter(|r| slot_of(sk, r).is_none()).cloned());
    let mut shown: Vec<String> = Vec::new();
    for l in labels {
        let Some(sm) = machine_of(skeletons, sk, &l) else { continue };
        let key = if l.contains('.') { l.clone() } else { sm.name.clone() };
        if shown.contains(&key) { continue; }
        shown.push(key.clone());
        section("machine", machine_lines(&key, sm, source));
    }
    if !b.frame.calls.is_empty() {
        // (signature, frame, contract lines)
        let rows: Vec<(String, String, Vec<String>)> = b.frame.calls.iter().map(|c| match callee_of(skeletons, sk, c) {
            Some(callee) => {
                let ret = format_return(&callee.return_type.cloned());
                let frame = if callee.frame.is_pure() { "pure".to_string() } else { frame_inline(&callee.frame) };
                // what the caller owes, and what it gets
                let contract: Vec<String> = callee.requires.iter().map(|x| format!("    requires {}", render_expr(&x.node)))
                    .chain(callee.ensures.iter().map(|x| format!("    ensures {}", render_expr(&x.node)))).collect();
                let mut contract: Vec<String> = contract;
                // what the callee is for, in its skeleton's words
                let intent = intent_of(callee.decl_span, source, true);
                if !intent.is_empty() { contract.insert(0, format!("    // {}", intent.join(" "))); }
                (format!("{}({}){}{}", c, format_params(callee.params), ret, if callee.hole { "   [hole]" } else { "" }), frame, contract)
            }
            // a builtin: its signature, and what it does in one sentence
            None => match crate::interpreter::builtins::registry::BUILTINS.iter().find(|d| d.name == c) {
                Some(doc) => (doc.signature.to_string(), String::new(), vec![format!("    {}", doc.brief.split(". ").next().unwrap_or(doc.brief).trim_end_matches('.'))]),
                None => (c.clone(), String::new(), Vec::new()),
            },
        }).collect();
        let pad = rows.iter().map(|(sig, _, _)| sig.len()).filter(|n| *n <= 60).max().unwrap_or(0);
        section("calls", rows.iter().flat_map(|(sig, frame, contract)| {
            let head = if frame.is_empty() { sig.clone() } else { format!("{:<pad$}   {}", sig, frame, pad = pad) };
            std::iter::once(head).chain(contract.iter().cloned())
        }).collect());
    }
    if !b.frame.emits.is_empty() {
        // an emit runs every signal of that name: who listens, and what each is for
        let mut lines = Vec::new();
        for e in &b.frame.emits {
            let listeners: Vec<(&CellSkeleton, &Bone)> = skeletons.iter().flat_map(|o| o.bones.iter().filter(|x| &x.name == e).map(move |x| (o, x))).collect();
            if listeners.is_empty() { lines.push(format!("{}   (no signal of that name listens in this program)", e)); }
            for (o, x) in listeners {
                let ret = format_return(&x.return_type.cloned());
                lines.push(format!("{}({}){}   in {}{}", e, format_params(x.params), ret, o.cell.name, if x.frame.is_pure() { String::new() } else { format!(": {}", frame_inline(&x.frame)) }));
                let intent = intent_of(x.decl_span, source, true);
                if !intent.is_empty() { lines.push(format!("    // {}", intent.join(" "))); }
            }
        }
        section("emits", lines);
    }
    let callers = callers_of(skeletons, sk, b);
    if !callers.is_empty() { section("called by", callers); }
    let emitters: Vec<String> = skeletons.iter().flat_map(|o| o.bones.iter().filter(|x| x.frame.emits.contains(&b.name)).map(move |x| format!("{}.{}", o.cell.name, x.name))).collect();
    if !emitters.is_empty() { section("emitted by", emitters); }
    out.push_str(&stub_line(sk, b));
    out
}

/// For a hole: where its body goes, ready to fill.
fn stub_line(sk: &CellSkeleton, b: &Bone) -> String {
    if !b.hole { return String::new(); }
    format!("write      flesh {} {{ on {}({}) {{ … }} }}      (the syntax a body needs, on one page: soma docs flesh)\n", sk.cell.name, b.name,
        b.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", "))
}

/// The signals whose frame may call `b` (`Cell.signal`, a schedule as
/// `Cell: every 5s`): who relies on its contract.
fn callers_of(skeletons: &[CellSkeleton], sk: &CellSkeleton, b: &Bone) -> Vec<String> {
    let qualified = format!("{}.{}", sk.cell.name, b.name);
    let mut out = Vec::new();
    for other in skeletons {
        let names = |f: &Frame| f.calls.iter().any(|c| c == &qualified || (other.cell.name == sk.cell.name && c == &b.name));
        for x in &other.bones {
            if names(&x.frame) && !(other.cell.name == sk.cell.name && x.name == b.name) { out.push(format!("{}.{}", other.cell.name, x.name)); }
        }
        for s in &other.schedules {
            if names(&s.frame) { out.push(format!("{}: {}", other.cell.name, s.label)); }
        }
    }
    out
}

// ── JSON ─────────────────────────────────────────────────────────────

fn frame_json(f: &Frame) -> serde_json::Value {
    serde_json::json!({
        "reads": f.reads, "writes": f.writes, "calls": f.calls, "emits": f.emits,
        "moves": f.moves.iter().map(|(m, from, to)| serde_json::json!({ "machine": m, "from": from, "to": to })).collect::<Vec<_>>(),
        "dynamic_moves": f.dynamic_moves,
        "pure": f.is_pure(),
    })
}

fn bone_json(b: &Bone) -> serde_json::Value {
    serde_json::json!({
        "name": b.name,
        "kind": if b.tool { "tool" } else { "signal" },
        "params": b.params.iter().map(|p| serde_json::json!({ "name": p.name, "type": format_type(&p.ty.node) })).collect::<Vec<_>>(),
        "returns": b.return_type.map(|r| format_type(&r.node)),
        "requires": b.requires.iter().map(|c| render_expr(&c.node)).collect::<Vec<_>>(),
        "ensures": b.ensures.iter().map(|c| render_expr(&c.node)).collect::<Vec<_>>(),
        "in_face": b.in_face,
        "has_handler": !b.no_handler,
        "hole": b.hole,
        "frame_declared": b.declared,
        "frame": frame_json(&b.frame),
    })
}

fn map_json(program: &Program, skeletons: &[CellSkeleton], file: &str) -> serde_json::Value {
    let types: Vec<serde_json::Value> = frame::cells_with_parents(program).into_iter()
        .filter(|(c, _)| c.kind == CellKind::Type)
        .flat_map(|(c, _)| c.sections.iter().filter_map(move |s| match &s.node {
            Section::Variants(v) => Some(serde_json::json!({
                "name": c.name,
                "variants": v.variants.iter().map(|x| format_variant(&x.node)).collect::<Vec<_>>(),
            })),
            _ => None,
        })).collect();
    let cells: Vec<serde_json::Value> = skeletons.iter().map(|sk| {
        let mut slots = Vec::new();
        let mut invariants = Vec::new();
        let mut machines = Vec::new();
        for s in &sk.cell.sections {
            match &s.node {
                Section::Memory(mem) => {
                    for slot in &mem.slots {
                        slots.push(serde_json::json!({
                            "name": slot.node.name, "type": format_type(&slot.node.ty.node),
                            "properties": slot.node.properties.iter().map(|p| p.node.name().to_string()).collect::<Vec<_>>(),
                        }));
                    }
                    for inv in &mem.invariants { invariants.push(render_expr(&inv.node)); }
                }
                Section::State(sm) => machines.push(serde_json::json!({
                    "name": sm.name, "initial": sm.initial,
                    "transitions": sm.transitions.iter().map(|t| serde_json::json!({
                        "from": t.node.from, "to": t.node.to,
                        "guard": t.node.guard.as_ref().map(|g| render_expr(&g.node)),
                    })).collect::<Vec<_>>(),
                })),
                _ => {}
            }
        }
        serde_json::json!({
            "name": sk.cell.name,
            "kind": if sk.cell.kind == CellKind::Agent { "agent" } else { "cell" },
            "interior_of": sk.parent,
            "memory": slots, "invariants": invariants, "machines": machines,
            "signals": sk.bones.iter().map(bone_json).collect::<Vec<_>>(),
            "schedules": sk.schedules.iter().map(|s| serde_json::json!({ "label": s.label, "frame": frame_json(&s.frame) })).collect::<Vec<_>>(),
            "helpers": sk.helpers,
        })
    }).collect();
    serde_json::json!({ "file": file, "types": types, "cells": cells })
}

fn brief_json(program: &Program, source: &str, skeletons: &[CellSkeleton], sk: &CellSkeleton, b: &Bone) -> serde_json::Value {
    let callees: Vec<serde_json::Value> = b.frame.calls.iter().map(|c| match callee_of(skeletons, sk, c) {
        Some(callee) => { let mut v = bone_json(callee); v["name"] = serde_json::json!(c); v }
        None => serde_json::json!({ "name": c, "kind": "builtin" }),
    }).collect();
    let slots: Vec<serde_json::Value> = b.frame.reads.iter().map(|n| (n, "reads"))
        .chain(b.frame.writes.iter().map(|n| (n, "writes")))
        .filter_map(|(n, access)| slot_of(sk, n).map(|(slot, invariants)| serde_json::json!({
            "name": n, "access": access, "type": format_type(&slot.ty.node), "invariants": invariants,
        }))).collect();
    let mut texts: Vec<String> = vec![bone_head(b)];
    texts.extend(b.frame.calls.iter().filter_map(|c| callee_of(skeletons, sk, c)).map(bone_head));
    // …and the types of the slots it reads or writes: what it stores must be one
    for section in &sk.cell.sections {
        if let Section::Memory(m) = &section.node {
            texts.extend(m.slots.iter().filter(|sl| b.frame.reads.contains(&sl.node.name) || b.frame.writes.contains(&sl.node.name)).map(|sl| slot_line(&sl.node)));
        }
    }
    let mut labels: Vec<String> = b.frame.moves.iter().map(|(l, _, _)| l.clone()).collect();
    labels.extend(b.frame.reads.iter().filter(|r| slot_of(sk, r).is_none()).cloned());
    let mut machines: Vec<serde_json::Value> = Vec::new();
    for l in labels {
        let Some(sm) = machine_of(skeletons, sk, &l) else { continue };
        let name = if l.contains('.') { l.clone() } else { sm.name.clone() };
        if machines.iter().any(|m| m["name"] == name.as_str()) { continue; }
        machines.push(serde_json::json!({
            "name": name, "initial": sm.initial,
            "transitions": sm.transitions.iter().map(|t| serde_json::json!({ "from": t.node.from, "to": t.node.to, "guard": t.node.guard.as_ref().map(|g| render_expr(&g.node)) })).collect::<Vec<_>>(),
        }));
    }
    let span = cell_span(program, &sk.cell.name);
    serde_json::json!({
        "cell": sk.cell.name,
        "cell_intent": span.map(|sp| intent_of(sp, source, false)).unwrap_or_default(),
        "intent": intent_of(b.decl_span, source, true),
        "signal": bone_json(b), "slots": slots, "callees": callees, "called_by": callers_of(skeletons, sk, b),
        "consts": span.map(|sp| consts_of(sp, source)).unwrap_or_default(),
        "types": types_named(program, &texts),
        "machines": machines,
    })
}

// ── holes ────────────────────────────────────────────────────────────

// ── freeze: skeleton.lock ────────────────────────────────────────────

/// `soma skeleton FILE --freeze` / `--amend`: write the program's skeletons
/// to `skeleton.lock`; from then on `soma check` (and run, test, serve,
/// verify) refuse a program whose skeletons differ from it.
pub fn cmd_freeze(path: &PathBuf, amend: bool) {
    use crate::checker::lock;
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    let now = lock::fingerprint(&program, path, &source);
    let lock_path = lock::path_for(path);
    if now.is_empty() {
        eprintln!("error: {} has no `skeleton` to freeze — its cells hold their own handlers (`soma skeleton {} --split` rewrites them as skeleton + flesh)", file_str, file_str);
        std::process::exit(1);
    }
    let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let existing = lock::read(&lock_path);
    let replaced = !matches!(existing, Ok(None));
    match (&existing, amend) {
        // a lock is there: only --amend writes over it — readable or not,
        // for this program or for another one of the directory
        (Err(why), false) => {
            eprintln!("error: {} {} — `--freeze` does not write over a lock: restore it (version control), or replace it on purpose: soma skeleton {} --amend", lock_path.display(), why, file_str);
            std::process::exit(1);
        }
        (Ok(Some(old)), false) => {
            let diff = lock::compare(old, &now, old.file == file_name);
            if diff.is_empty() && old.file == file_name {
                println!("{} already holds these {} skeleton{}", lock_path.display(), skeleton_count(&now), if skeleton_count(&now) == 1 { "" } else { "s" });
                return;
            }
            if old.file != file_name {
                eprintln!("error: {} was written for {} — one lock per directory. To make it the lock of {} instead: soma skeleton {} --amend", lock_path.display(), old.file, file_name, file_str);
            } else {
                eprintln!("error: {} exists and the structure has changed since — to accept the change on purpose: soma skeleton {} --amend", lock_path.display(), file_str);
            }
            for d in &diff { eprintln!("{}", d); }
            std::process::exit(1);
        }
        (Err(why), true) => println!("replacing {}, which {}", lock_path.display(), why),
        (Ok(Some(old)), true) => {
            let diff = lock::compare(old, &now, true);
            if diff.is_empty() && old.file == file_name { println!("nothing to amend: the structure is the one {} holds", lock_path.display()); return; }
            if old.file != file_name { println!("{} was the lock of {}: it becomes the lock of {}", lock_path.display(), old.file, file_name); }
            for d in &diff { println!("{}", d); }
        }
        (Ok(None), _) => {}
    }
    if let Err(e) = std::fs::write(&lock_path, lock::render(&file_name, &now)) {
        eprintln!("error: cannot write {}: {}", lock_path.display(), e);
        std::process::exit(1);
    }
    let names: Vec<String> = now.iter().filter(|e| e.name != lock::VERIFY_ENTRY).map(|e| e.name.clone()).collect();
    println!("{} {} — {} skeleton{}: {}{}", if replaced { "amended" } else { "froze" }, lock_path.display(), names.len(),
        if names.len() == 1 { "" } else { "s" }, names.join(", "),
        if now.iter().any(|e| e.name == lock::VERIFY_ENTRY) { ", with the edition and the [verify] tables of soma.toml" } else { "" });
}

fn skeleton_count(entries: &[crate::checker::lock::Entry]) -> usize {
    entries.iter().filter(|e| e.name != crate::checker::lock::VERIFY_ENTRY).count()
}

/// `soma holes`: the signals of the program's skeletons that have no body
/// yet, in the order to fill them — a hole comes after the holes it calls,
/// so each body can run as soon as it is written.
pub fn cmd_holes(path: &PathBuf, only: Option<&str>, json: bool) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    let skeletons = frame::build(&program);
    let broken = crate::checker::lock::check(&program, path, &source);
    if !broken.is_empty() {
        for b in &broken { eprintln!("error: {}", b); }
        eprintln!("error: the holes of a program its skeleton.lock refuses are not a work list");
        std::process::exit(1);
    }

    let declared: Vec<&CellSkeleton> = skeletons.iter().filter(|sk| sk.cell.skeleton).collect();
    let signals: usize = declared.iter().map(|sk| sk.bones.iter().filter(|b| b.in_face).count()).sum();
    // (name, the holes its frame calls, bone)
    let mut open: Vec<(String, Vec<String>, &Bone)> = Vec::new();
    for sk in &declared {
        for b in sk.bones.iter().filter(|b| b.hole) {
            let calls: Vec<String> = b.frame.calls.iter()
                .map(|c| if c.contains('.') { c.clone() } else { format!("{}.{}", sk.cell.name, c) })
                .collect();
            open.push((format!("{}.{}", sk.cell.name, b.name), calls, b));
        }
    }
    let names: std::collections::HashSet<String> = open.iter().map(|(n, _, _)| n.clone()).collect();
    for (_, calls, _) in open.iter_mut() { calls.retain(|c| names.contains(c)); }
    let mut ordered: Vec<(String, Vec<String>, &Bone)> = Vec::new();
    while !open.is_empty() {
        // a cycle of holes has no first: take them in source order
        let i = open.iter().position(|(n, calls, _)| calls.iter().all(|c| c == n || ordered.iter().any(|(o, _, _)| o == c))).unwrap_or(0);
        ordered.push(open.remove(i));
    }

    // `soma holes app.cell Orders` / `Orders.place`: that cell's, that one —
    // the numbers stay the ones of the whole order
    let total = ordered.len();
    let numbered: Vec<(usize, &(String, Vec<String>, &Bone))> = ordered.iter().enumerate()
        .filter(|(_, (name, _, _))| only.map_or(true, |o| name == o || name.split('.').next() == Some(o)))
        .map(|(i, h)| (i + 1, h)).collect();
    if let Some(o) = only {
        if !skeletons.iter().any(|sk| sk.cell.name == o || o.split_once('.').map_or(false, |(c, s)| sk.cell.name == c && sk.bones.iter().any(|b| b.name == s))) {
            eprintln!("error: `{}` names no cell (or `Cell.signal`) of {} — cells: {}", o, file_str, skeletons.iter().map(|sk| sk.cell.name.clone()).collect::<Vec<_>>().join(", "));
            std::process::exit(1);
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "file": file_str,
            "skeletons": declared.len(),
            "signals": signals,
            "total_holes": total,
            "holes": numbered.iter().map(|(n, (name, after, b))| serde_json::json!({
                "order": n, "cell": name.split('.').next().unwrap_or(""), "signal": name, "after": after, "frame": frame_json(&b.frame),
            })).collect::<Vec<_>>(),
        })).unwrap());
        return;
    }
    if declared.is_empty() {
        println!("no skeleton in {} — its cells hold their own handlers (`soma skeleton {}` prints the skeleton it would have)", file_str, file_str);
        return;
    }
    if ordered.is_empty() {
        println!("no holes — the {} signal{} of {} all have a body", signals, if signals == 1 { "" } else { "s" }, file_str);
        return;
    }
    if numbered.is_empty() {
        println!("no holes in `{}` — {} elsewhere in {}", only.unwrap_or(""), total, file_str);
        return;
    }
    println!("{} hole{} of {} signal{} in {} — numbered in the order to fill them (a hole comes after the holes it calls):",
        total, if total == 1 { "" } else { "s" }, signals, if signals == 1 { "" } else { "s" }, file_str);
    // one block per cell: a cell is what one agent takes
    let mut cells: Vec<&str> = Vec::new();
    for (_, (name, _, _)) in &numbered { let c = name.split('.').next().unwrap_or(""); if !cells.contains(&c) { cells.push(c); } }
    for cell in cells {
        let mine: Vec<&(usize, &(String, Vec<String>, &Bone))> = numbered.iter().filter(|(_, (n, _, _))| n.split('.').next() == Some(cell)).collect();
        println!("\n{} ({})", cell, mine.len());
        let pad = mine.iter().map(|(_, (n, _, _))| n.len() - cell.len() - 1).max().unwrap_or(0);
        for (n, (name, _, b)) in mine {
            let frame = if b.frame.is_pure() { "pure".to_string() } else { frame_inline(&b.frame) };
            println!("  {:>3}. {:<pad$}   {}", n, &name[cell.len() + 1..], frame, pad = pad);
        }
    }
    println!("\nthe brief for one: soma skeleton {} {}", file_str, numbered[0].1 .0);
}

// ── split: a cell as skeleton + flesh ────────────────────────────────

/// Frame clauses, one per line, indented by `indent`.
fn clause_lines(f: &Frame, indent: &str) -> String {
    let mut out = String::new();
    for (kw, items) in f.clauses() {
        // a continuation line hangs one level under its clause (`soma fmt`)
        out.push_str(&wrap(&format!("{}{} ", indent, kw), &items, &format!("{}    ", indent)));
    }
    out
}

fn line_indent(source: &str, at: usize) -> String {
    let start = source[..at].rfind('\n').map_or(0, |i| i + 1);
    source[start..at].chars().take_while(|c| c.is_whitespace()).collect()
}

/// The file rewritten: every `cell` / `cell agent` of it as a `skeleton`
/// (its structure verbatim, each signal with the frame its body has) and a
/// `flesh` (its handlers verbatim). Everything else — comments, types,
/// tests, imports — is kept as written. Also: the signals no frame can
/// describe.
pub fn render_split(program: &Program, skeletons: &[CellSkeleton], source: &str) -> (String, Vec<String>) {
    // spans count characters; the text is cut by bytes
    let idx: Vec<usize> = source.char_indices().map(|(i, _)| i).chain(std::iter::once(source.len())).collect();
    let local = |sp: Span| sp.start <= sp.end && sp.end < idx.len();
    let bytes = |sp: Span| Span { start: idx[sp.start], end: idx[sp.end] };
    let mut items: Vec<(Span, Option<&CellDef>)> = program.cells.iter().filter(|c| local(c.span))
        .map(|c| (bytes(c.span), if matches!(c.node.kind, CellKind::Cell | CellKind::Agent) && !c.node.skeleton { Some(&c.node) } else { None }))
        .collect();
    items.extend(program.protocols.iter().filter(|p| local(p.span)).map(|p| (bytes(p.span), None)));
    items.sort_by_key(|(sp, _)| sp.start);

    let mut out = String::new();
    let mut unframed = Vec::new();
    let mut at = 0usize;
    for (span, cell) in items {
        if span.start < at { continue; }
        out.push_str(&source[at..span.start]);
        match cell.and_then(|c| skeletons.iter().find(|sk| sk.cell.name == c.name && sk.parent.is_none())) {
            Some(sk) => split_cell(sk, source, &idx, span, &mut out, &mut unframed),
            None => out.push_str(&source[span.start..span.end]),
        }
        at = span.end;
    }
    out.push_str(&source[at..]);
    (out, unframed)
}

fn split_cell(sk: &CellSkeleton, source: &str, idx: &[usize], span: Span, out: &mut String, unframed: &mut Vec<String>) {
    let cell = sk.cell;
    // a span of the AST (characters) as bytes of `source`; None: not in this file
    let bytes = |sp: Span| if sp.start <= sp.end && sp.end < idx.len() { Some(Span { start: idx[sp.start], end: idx[sp.end] }) } else { None };
    let text = &source[span.start..span.end];
    let Some(open) = text.find('{') else { out.push_str(text); return; };
    // `cell Name` / `cell agent Name [model: …]` → `skeleton …`
    let head = text[..open].trim_start();
    let head = head.strip_prefix("cell").unwrap_or(head);
    let base = line_indent(source, span.start);
    let inner = format!("{}    ", base);
    let mut skel = format!("skeleton{}{{", head);
    let mut flesh = String::new();

    let face_names: Vec<&str> = sk.bones.iter().filter(|b| b.in_face).map(|b| b.name.as_str()).collect();
    // signals the face gains: handlers called from outside the cell, and
    // one per schedule (a schedule has no frame: it calls a signal)
    let mut extra: Vec<String> = Vec::new();
    let note = |b: &Bone, unknown: &str, unframed: &mut Vec<String>| {
        if !unknown.is_empty() { unframed.push(format!("`{}.{}` calls transition() toward a target or a machine known only at run time", cell.name, b.name)); }
    };
    for b in sk.bones.iter().filter(|b| !b.in_face) {
        let (f, unknown) = declarable(&b.frame);
        note(b, unknown, unframed);
        extra.push(format!("{}signal {}({})\n{}", format!("{}    ", inner), b.name, format_params(b.params), clause_lines(&f, &format!("{}        ", inner))));
    }
    let mut ticks: Vec<(String, String)> = Vec::new(); // (signal, body text)
    let mut schedule = 0usize;

    let mut at = span.start + open + 1;
    let mut face_done = false;
    let face_extra = |extra: &[String]| extra.concat();
    for section in &cell.sections {
        // synthesized (a bound tool's handler) or out of this file
        let Some(sp) = bytes(section.span) else { continue };
        if sp.start < at || sp.end > span.end { continue; }
        let gap = &source[at..sp.start];
        let body = &source[sp.start..sp.end];
        match &section.node {
            Section::OnSignal(_) => { flesh.push_str(gap); flesh.push_str(body); }
            Section::Every(ev) | Section::After(ev) => {
                let f = sk.schedules.get(schedule).map(|s| s.frame.clone()).unwrap_or_default();
                schedule += 1;
                let calls_only = f.reads.is_empty() && f.writes.is_empty() && f.moves.is_empty() && f.emits.is_empty() && !f.dynamic_moves
                    && f.calls.iter().all(|c| c.contains('.') || face_names.contains(&c.as_str()) || sk.bones.iter().any(|b| &b.name == c));
                let brace = body.find('{').unwrap_or(body.len());
                if calls_only || brace == body.len() {
                    skel.push_str(gap); skel.push_str(body);
                } else {
                    let name = format!("_tick_{}", schedule);
                    let (shown, unknown) = declarable(&f);
                    if !unknown.is_empty() { unframed.push(format!("the `{}` block of `{}` calls transition() toward a target known only at run time", body[..brace].trim(), cell.name)); }
                    extra.push(format!("{}    signal {}()\n{}", inner, name, clause_lines(&shown, &format!("{}        ", inner))));
                    let task = if ev.task { " [task]" } else { "" };
                    ticks.push((format!("{}on {}(){} {}", inner, name, task, &body[brace..]), String::new()));
                    skel.push_str(gap);
                    skel.push_str(&format!("{}{{ {}() }}", &body[..brace], name));
                }
            }
            Section::Face(face) => {
                skel.push_str(gap);
                let mut fat = sp.start;
                for decl in &face.declarations {
                    let Some(dsp) = bytes(decl.span) else { continue };
                    if dsp.start < fat || dsp.end > sp.end { continue; }
                    skel.push_str(&source[fat..dsp.start]);
                    let name = match &decl.node { FaceDecl::Signal(s) => Some(&s.name), FaceDecl::Tool(t) => Some(&t.name), _ => None };
                    let bone = name.and_then(|n| sk.bones.iter().find(|b| &b.name == n && b.in_face));
                    let ind = line_indent(source, dsp.start);
                    match bone {
                        Some(b) => {
                            let (f, unknown) = declarable(&b.frame);
                            note(b, unknown, unframed);
                            // the handler's own parameter names, when they differ
                            let renamed = match &decl.node {
                                FaceDecl::Signal(sig) => sig.params.len() == b.params.len() && sig.params.iter().zip(b.params).any(|(a, c)| a.name != c.name || format_type(&a.ty.node) != format_type(&c.ty.node)),
                                _ => false,
                            };
                            if renamed {
                                skel.push_str(&bone_head(b));
                                for c in b.requires { skel.push_str(&format!("\n{}    requires {}", ind, render_expr(&c.node))); }
                                for c in b.ensures { skel.push_str(&format!("\n{}    ensures {}", ind, render_expr(&c.node))); }
                            } else {
                                skel.push_str(&source[dsp.start..dsp.end]);
                            }
                            let clauses = clause_lines(&f, &format!("{}    ", ind));
                            if !clauses.is_empty() { skel.push('\n'); skel.push_str(clauses.trim_end_matches('\n')); }
                        }
                        None => skel.push_str(&source[dsp.start..dsp.end]),
                    }
                    fat = dsp.end;
                }
                // the closing brace of the face: the gained signals go before it
                let tail = &source[fat..sp.end];
                let close = tail.rfind('}').unwrap_or(tail.len());
                if !face_done && !extra.is_empty() {
                    skel.push_str(tail[..close].trim_end());
                    skel.push('\n');
                    skel.push_str(face_extra(&extra).trim_end_matches('\n'));
                    skel.push_str(&format!("\n{}", inner));
                    skel.push_str(&tail[close..]);
                } else {
                    skel.push_str(tail);
                }
                face_done = true;
            }
            _ => { skel.push_str(gap); skel.push_str(body); }
        }
        at = sp.end;
    }
    // a schedule after the face, or no face at all: the gained signals
    // still need one
    let late: Vec<String> = if face_done { extra.iter().filter(|e| e.contains("signal _tick_") && !skel.contains(e.as_str())).cloned().collect() } else { extra.clone() };
    if !late.is_empty() {
        skel.push_str(&format!("\n{}face {{\n{}\n{}}}", inner, late.concat().trim_end_matches('\n'), inner));
    }
    let tail = &source[at..span.end];
    let close = tail.rfind('}').unwrap_or(tail.len());
    // trailing comments stay with the code they follow
    let (tail_gap, closing) = (&tail[..close], &tail[close..]);
    if flesh.is_empty() && ticks.is_empty() {
        skel.push_str(tail_gap);
    } else {
        skel.push_str(&format!("\n{}", base));
        flesh.push_str(tail_gap.trim_end());
    }
    skel.push_str(closing);
    out.push_str(&skel);
    if !flesh.trim().is_empty() || !ticks.is_empty() {
        out.push_str(&format!("\n\n{}flesh {} {{\n", base, cell.name));
        // no blank line after the opening brace (`soma fmt`)
        let body = flesh.trim_end();
        let first = body.find(|c: char| !c.is_whitespace()).unwrap_or(body.len());
        out.push_str(&body[body[..first].rfind('\n').map_or(0, |i| i + 1)..]);
        for (t, _) in &ticks { out.push_str(&format!("\n\n{}", t)); }
        out.push_str(&format!("\n{}}}", base));
    }
}
