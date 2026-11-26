//! The law (RFC 0002): the business rules, written outside the code, and
//! the program held to them.
//!
//! A `.law` file declares terms and rules and names no cell, no handler, no
//! test. The program says which laws it is under (`[law] files = […]` in
//! soma.toml) and is aligned on them in the code:
//!
//! - `realizes Term` on a signal of a skeleton: this action is an instance
//!   of a term of the law;
//! - `under "ID"` on a `require`, on a frame with a guarded edge, on a test
//!   cell: it serves that rule;
//! - `exempt "ID" "why"` on a signal: that rule does not apply to it.
//!
//! The check refuses a rule nothing serves, and a rule not served in a
//! signal that realizes one of the terms it is about — the forgotten path.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::ast::*;

/// The error kind of the `require` that the statement `under "ID"` reads as
/// (parser): it always holds; it marks the branch where the rule's case is
/// handled by what the code does, not by a refusal.
pub const LAW_HANDLED: &str = "LawHandled";

#[derive(Debug, Clone)]
pub struct Term {
    pub name: String,
    pub definition: String,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub text: String,
    pub about: Vec<String>,
    pub source: String,
    pub owner: String,
    pub law: String,
}

#[derive(Debug, Clone)]
pub struct Law {
    pub name: String,
    /// as written in soma.toml
    pub file: String,
    pub terms: Vec<Term>,
    pub rules: Vec<Rule>,
}

/// The laws a program is under, as read from disk.
#[derive(Debug, Default)]
pub struct Loaded {
    pub laws: Vec<Law>,
    /// (file as written in soma.toml, its text)
    pub files: Vec<(String, String)>,
    /// a file that is missing or does not parse: `file:line: message`
    pub errors: Vec<String>,
}

enum Tok { Word(String), Str(String), Comma }

fn tokens(line: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() { i += 1; continue; }
        if c == '/' && chars.get(i + 1) == Some(&'/') { break; }
        if c == ',' { out.push(Tok::Comma); i += 1; continue; }
        if c == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err("a string is not closed (`\"`)".to_string()),
                    Some('\\') if chars.get(i + 1) == Some(&'"') => { s.push('"'); i += 2; }
                    Some('"') => { i += 1; break; }
                    Some(ch) => { s.push(*ch); i += 1; }
                }
            }
            out.push(Tok::Str(s));
            continue;
        }
        let mut w = String::new();
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != ',' && chars[i] != '"' {
            w.push(chars[i]);
            i += 1;
        }
        out.push(Tok::Word(w));
    }
    Ok(out)
}

/// Reads one `.law` file. A law names no code: its only words are `law`,
/// `term`, `rule`, and under a rule `about`, `source`, `owner`.
pub fn parse(file: &str, text: &str) -> (Law, Vec<String>) {
    let mut law = Law { name: String::new(), file: file.to_string(), terms: Vec::new(), rules: Vec::new() };
    let mut errors = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let at = |m: String| format!("{}:{}: {}", file, n + 1, m);
        let toks = match tokens(line) { Ok(t) => t, Err(m) => { errors.push(at(m)); continue; } };
        let Some(Tok::Word(head)) = toks.first() else {
            if !toks.is_empty() { errors.push(at("a line of a law starts with `law`, `term`, `rule`, `about`, `source` or `owner`".to_string())); }
            continue;
        };
        let strs: Vec<&String> = toks[1..].iter().filter_map(|t| if let Tok::Str(s) = t { Some(s) } else { None }).collect();
        let names: Vec<&String> = toks[1..].iter().filter_map(|t| if let Tok::Word(s) = t { Some(s) } else { None }).collect();
        match head.as_str() {
            "law" => match (names.first(), law.name.is_empty()) {
                (Some(name), true) => law.name = (*name).clone(),
                (Some(_), false) => errors.push(at("a file holds one law: a second `law` line".to_string())),
                (None, _) => errors.push(at("`law` is followed by its name: `law Sanctions`".to_string())),
            },
            "term" => match (names.first(), strs.first()) {
                (Some(name), Some(def)) if !def.trim().is_empty() => {
                    if law.terms.iter().any(|t| &t.name == *name) { errors.push(at(format!("the term `{}` is declared twice", name))); }
                    else { law.terms.push(Term { name: (*name).clone(), definition: (*def).clone() }); }
                }
                _ => errors.push(at("a term is a name and its definition in one sentence: `term Porteur \"Celui qui détient des parts.\"`".to_string())),
            },
            "rule" => match (strs.first(), strs.get(1)) {
                (Some(id), Some(text)) if !id.trim().is_empty() && !text.trim().is_empty() => {
                    if law.rules.iter().any(|r| &r.id == *id) { errors.push(at(format!("the rule \"{}\" is declared twice", id))); }
                    else { law.rules.push(Rule { id: (*id).clone(), text: (*text).clone(), about: Vec::new(), source: String::new(), owner: String::new(), law: String::new() }); }
                }
                _ => errors.push(at("a rule is an identifier and a sentence: `rule \"SAN-02\" \"Aucun paiement vers un porteur bloqué\"`".to_string())),
            },
            "about" | "source" | "owner" => {
                let Some(rule) = law.rules.last_mut() else {
                    errors.push(at(format!("`{}` belongs to a rule: write it under a `rule` line", head)));
                    continue;
                };
                match head.as_str() {
                    "about" if !names.is_empty() => rule.about.extend(names.iter().map(|s| (*s).clone())),
                    "source" if !strs.is_empty() => rule.source = strs[0].clone(),
                    "owner" if !strs.is_empty() => rule.owner = strs[0].clone(),
                    "about" => errors.push(at("`about` lists the terms the rule speaks of: `about Paiement, Porteur`".to_string())),
                    _ => errors.push(at(format!("`{}` is followed by a string: `{} \"…\"`", head, head))),
                }
            }
            other => errors.push(at(format!("`{}` is not a word of a law — a law has `law`, `term`, `rule`, and under a rule `about`, `source`, `owner`; it names no cell, no handler and no test", other))),
        }
    }
    if law.name.is_empty() { errors.push(format!("{}: the file does not name its law: start it with `law Name`", file)); }
    let name = law.name.clone();
    for r in law.rules.iter_mut() { r.law = name.clone(); }
    (law, errors)
}

/// The laws `[law] files = […]` of soma.toml names, read beside it.
pub fn load(manifest: Option<&crate::pkg::manifest::Manifest>, root: &Path) -> Loaded {
    let mut out = Loaded::default();
    let Some(m) = manifest else { return out };
    for file in &m.law.files {
        match std::fs::read_to_string(root.join(file)) {
            Ok(text) => {
                let (law, errors) = parse(file, &text);
                out.errors.extend(errors);
                out.laws.push(law);
                out.files.push((file.clone(), text));
            }
            Err(e) => out.errors.push(format!("{}: the law soma.toml names cannot be read ({})", file, e)),
        }
    }
    // across the laws of the program: one identifier, one rule; one name, one term
    let mut seen_rules: BTreeMap<String, String> = BTreeMap::new();
    let mut terms: BTreeSet<String> = BTreeSet::new();
    for law in &out.laws {
        for t in &law.terms { terms.insert(t.name.clone()); }
        for r in &law.rules {
            if let Some(other) = seen_rules.insert(r.id.clone(), law.file.clone()) {
                if other != law.file { out.errors.push(format!("{}: the rule \"{}\" is also declared in {}", law.file, r.id, other)); }
            }
        }
    }
    for law in &out.laws {
        for r in &law.rules {
            for t in &r.about {
                if !terms.contains(t) { out.errors.push(format!("{}: the rule \"{}\" is about `{}`, a term no law of the program declares — add `term {} \"…\"`", law.file, r.id, t, t)); }
            }
        }
    }
    out
}

/// The tests of the project that say which rules they show at work: every
/// `cell test Name under "ID", …` in a `.cell` file beside soma.toml (and
/// in its `tests/` directory). A program and its tests are often separate
/// files (`prod.cell`, `test_passif.cell`): the law is checked on the
/// program, and its tests are found here. (rule -> tests)
pub fn tests_beside(root: &Path) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for dir in [root.to_path_buf(), root.join("tests")] {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().map_or(false, |x| x == "cell") { files.push(p); }
        }
    }
    files.sort();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        for line in text.lines() {
            let line = line.trim_start();
            let Some(rest) = line.strip_prefix("cell test ") else { continue };
            let Some((name, after)) = rest.split_once(" under ") else { continue };
            let head = after.split('{').next().unwrap_or("");
            let Ok(toks) = tokens(head) else { continue };
            for t in toks {
                if let Tok::Str(id) = t {
                    let list = out.entry(id).or_default();
                    if !list.contains(&name.trim().to_string()) { list.push(name.trim().to_string()); }
                }
            }
        }
    }
    out
}

// ── the lock ─────────────────────────────────────────────────────────

fn sha(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

/// `law.lock`: one line per law file, `sha256  file`.
pub fn lock_text(loaded: &Loaded) -> String {
    let mut out = String::from("# The laws this program is under, as they were reviewed. `soma law <file> --lock` rewrites it.\n");
    for (file, text) in &loaded.files { out.push_str(&format!("{}  {}\n", sha(text), file)); }
    out
}

/// `Some(message)` when a `law.lock` exists and the laws differ from it;
/// `None` when they agree. `Err(())` when there is no lock yet.
pub fn against_lock(loaded: &Loaded, root: &Path) -> Result<Option<String>, ()> {
    let Ok(locked) = std::fs::read_to_string(root.join("law.lock")) else { return Err(()) };
    let then: BTreeMap<String, String> = locked.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once("  ").map(|(h, f)| (f.trim().to_string(), h.trim().to_string()))).collect();
    let now: BTreeMap<String, String> = loaded.files.iter().map(|(f, t)| (f.clone(), sha(t))).collect();
    if then == now { return Ok(None); }
    let mut changed: Vec<String> = Vec::new();
    for (f, h) in &now {
        match then.get(f) {
            None => changed.push(format!("{} (new)", f)),
            Some(old) if old != h => changed.push(format!("{} (changed)", f)),
            _ => {}
        }
    }
    for f in then.keys() { if !now.contains_key(f) { changed.push(format!("{} (removed)", f)); } }
    Ok(Some(format!("the law differs from law.lock: {} — a change of the law is reviewed by the person who owns it, then `soma law <file> --lock` records it", changed.join(", "))))
}

// ── the program against the law ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Issue {
    pub kind: &'static str,
    pub message: String,
    pub span: Span,
    pub warning: bool,
}

/// A signal of the program that realizes a term of the law.
#[derive(Debug, Clone)]
pub struct Realization {
    pub cell: String,
    pub signal: String,
    pub terms: Vec<String>,
    /// rules a `require … under` in its handler (or in a helper of its cell
    /// it calls) serves
    pub checked: BTreeSet<String>,
    /// rules a statement `under "ID"` in a branch of its handler handles
    pub handled: BTreeSet<String>,
    /// rules its frame serves by a guarded edge
    pub proved: BTreeSet<String>,
    pub exempt: Vec<(String, String)>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RuleStatus {
    pub rule: Rule,
    /// `Cell.signal` and how: "proved" | "checked" | "handled"
    pub served: Vec<(String, &'static str)>,
    pub exempt: Vec<(String, String)>,
    /// signals that realize a term of the rule and neither serve it nor are exempt
    pub missing: Vec<String>,
    pub tests: Vec<String>,
}

impl RuleStatus {
    /// `unserved` | `incomplete` | `proved` | `checked` | `handled` — the
    /// weakest way it is served anywhere: a guarded edge proves, a refusal
    /// checks at run time, a marked branch only says where it is handled.
    pub fn status(&self) -> &'static str {
        if self.served.is_empty() { "unserved" }
        else if !self.missing.is_empty() { "incomplete" }
        else if self.served.iter().any(|(_, how)| *how == "handled") { "handled" }
        else if self.served.iter().all(|(_, how)| *how == "proved") { "proved" }
        else { "checked" }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub rules: Vec<RuleStatus>,
    pub realizations: Vec<Realization>,
    pub issues: Vec<Issue>,
    /// `require` statements of the program, and how many serve a rule
    pub requires: usize,
    pub requires_under: usize,
}

struct CellView<'a> {
    name: String,
    span: Span,
    signals: Vec<(&'a SignalDecl, Span)>,
    /// handler name -> the Debug text of its body
    bodies: BTreeMap<String, (String, Span)>,
}

/// The rules cited in a body (its Debug text), each with how: `true` for a
/// refusal (`require … under`), `false` for a marked branch (`under "ID"`).
fn unders(text: &str) -> BTreeSet<(String, bool)> {
    let mut out = BTreeSet::new();
    let mark = "under: Some(\"";
    let handled = format!("else_signal: \"{}\", ", LAW_HANDLED);
    let mut rest = text;
    while let Some(i) = rest.find(mark) {
        let refusal = !rest[..i].ends_with(&handled);
        rest = &rest[i + mark.len()..];
        if let Some(j) = rest.find('"') { out.insert((rest[..j].to_string(), refusal)); }
    }
    out
}

fn views<'a>(cells: &'a [Spanned<CellDef>], out: &mut Vec<CellView<'a>>, tests: &mut Vec<(String, Vec<String>, Span)>) {
    for cell in cells {
        if matches!(cell.node.kind, CellKind::Test) {
            if !cell.node.under.is_empty() { tests.push((cell.node.name.clone(), cell.node.under.clone(), cell.span)); }
            continue;
        }
        // a skeleton and its flesh are two definitions of one cell
        let at = match out.iter().position(|v| v.name == cell.node.name) {
            Some(i) => i,
            None => { out.push(CellView { name: cell.node.name.clone(), span: cell.span, signals: Vec::new(), bodies: BTreeMap::new() }); out.len() - 1 }
        };
        for section in &cell.node.sections {
            match &section.node {
                Section::Face(face) => for d in &face.declarations {
                    if let FaceDecl::Signal(sig) = &d.node { out[at].signals.push((sig, d.span)); }
                },
                Section::OnSignal(on) => { out[at].bodies.insert(on.signal_name.clone(), (format!("{:?}", on.body), section.span)); }
                Section::Interior(interior) => views(&interior.cells, out, tests),
                _ => {}
            }
        }
    }
}

/// The rules the handler `signal` of a cell serves — by a refusal (`true`)
/// or a marked branch (`false`) — in its own body or in a handler of the
/// same cell it calls.
fn served_by(view: &CellView, signal: &str) -> BTreeSet<(String, bool)> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut todo = vec![signal.to_string()];
    let mut out = BTreeSet::new();
    while let Some(h) = todo.pop() {
        if !seen.insert(h.clone()) { continue; }
        let Some((text, _)) = view.bodies.get(&h) else { continue };
        out.extend(unders(text));
        for other in view.bodies.keys() {
            if !seen.contains(other) && text.contains(&format!("FnCall {{ name: \"{}\"", other)) { todo.push(other.clone()); }
        }
    }
    out
}

pub fn analyse(program: &Program, laws: &[Law], tests_beside: &BTreeMap<String, Vec<String>>) -> Report {
    let mut report = Report::default();
    let none = Span { start: 0, end: 0 };
    let rules: Vec<&Rule> = laws.iter().flat_map(|l| l.rules.iter()).collect();
    let terms: BTreeSet<&str> = laws.iter().flat_map(|l| l.terms.iter().map(|t| t.name.as_str())).collect();
    let known = |id: &str| rules.iter().any(|r| r.id == id);
    let said = |id: &str| rules.iter().find(|r| r.id == id).map(|r| format!("\"{}\" ({})", r.id, r.text)).unwrap_or_else(|| format!("\"{}\"", id));

    let mut cells: Vec<CellView> = Vec::new();
    let mut tests: Vec<(String, Vec<String>, Span)> = Vec::new();
    views(&program.cells, &mut cells, &mut tests);

    // every `require`, and those under a rule that exists
    let mut checked_anywhere: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut handled_anywhere: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for view in &cells {
        for (handler, (text, span)) in &view.bodies {
            let marks = text.matches(&format!("else_signal: \"{}\"", LAW_HANDLED)).count();
            report.requires += text.matches("Require {").count() - marks;
            report.requires_under += text.matches("under: Some(\"").count() - marks;
            for (id, refusal) in unders(text) {
                if !known(&id) {
                    report.issues.push(Issue { kind: "law_unknown_rule", warning: false, span: *span,
                        message: format!("`{}.{}` has a refusal `under \"{}\"`, a rule no law of the program declares — the rules are {}; a new rule is added to the law by the person who owns it, not in the code", view.name, handler, id, rules.iter().map(|r| format!("\"{}\"", r.id)).collect::<Vec<_>>().join(", ")) });
                } else if refusal {
                    checked_anywhere.entry(id).or_default().push(format!("{}.{}", view.name, handler));
                } else {
                    handled_anywhere.entry(id).or_default().push(format!("{}.{}", view.name, handler));
                }
            }
        }
    }

    // the signals that realize a term, and what each serves
    let mut proved_anywhere: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for view in &cells {
        for (sig, span) in &view.signals {
            let Some(frame) = &sig.frame else { continue };
            let who = format!("{}.{}", view.name, sig.name);
            for t in &frame.realizes {
                if !terms.contains(t.as_str()) {
                    report.issues.push(Issue { kind: "law_unknown_term", warning: false, span: *span,
                        message: format!("`{}` realizes `{}`, a term no law of the program declares — the terms are {}", who, t, terms.iter().cloned().collect::<Vec<_>>().join(", ")) });
                }
            }
            let mut proved = BTreeSet::new();
            for id in &frame.under {
                if !known(id) {
                    report.issues.push(Issue { kind: "law_unknown_rule", warning: false, span: *span,
                        message: format!("`{}` is `under \"{}\"`, a rule no law of the program declares", who, id) });
                } else if frame.conditions.is_empty() {
                    report.issues.push(Issue { kind: "law_unguarded_under", warning: false, span: *span,
                        message: format!("`{}` is `under {}` but its frame has no guarded edge (`moves a -> b when Other.state`): nothing in it serves the rule — guard the edge, or put `under \"{}\"` on the `require` of its handler", who, said(id), id) });
                } else {
                    proved.insert(id.clone());
                    proved_anywhere.entry(id.clone()).or_default().push(who.clone());
                }
            }
            for (id, why) in &frame.exempt {
                if !known(id) {
                    report.issues.push(Issue { kind: "law_unknown_rule", warning: false, span: *span,
                        message: format!("`{}` is `exempt \"{}\"`, a rule no law of the program declares", who, id) });
                } else if why.trim().is_empty() {
                    report.issues.push(Issue { kind: "law_exempt_reason", warning: false, span: *span,
                        message: format!("`{}` is exempt from the rule {} without a reason — say why it does not apply: `exempt \"{}\" \"…\"`", who, said(id), id) });
                }
            }
            if frame.realizes.is_empty() { continue; }
            let served = served_by(view, &sig.name);
            report.realizations.push(Realization {
                cell: view.name.clone(), signal: sig.name.clone(), terms: frame.realizes.clone(),
                checked: served.iter().filter(|(id, refusal)| *refusal && known(id)).map(|(id, _)| id.clone()).collect(),
                handled: served.iter().filter(|(id, refusal)| !*refusal && known(id)).map(|(id, _)| id.clone()).collect(),
                proved, exempt: frame.exempt.clone(), span: *span,
            });
        }
    }

    // the tests: those of the project's files, and those of this program
    let mut tested: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, names) in tests_beside {
        if known(id) { tested.insert(id.clone(), names.clone()); }
    }
    for (name, ids, span) in &tests {
        for id in ids {
            if !known(id) {
                report.issues.push(Issue { kind: "law_unknown_rule", warning: false, span: *span,
                    message: format!("the test `{}` is `under \"{}\"`, a rule no law of the program declares", name, id) });
            } else {
                let list = tested.entry(id.clone()).or_default();
                if !list.contains(name) { list.push(name.clone()); }
            }
        }
    }

    // rule by rule, in the order of the law
    for rule in &rules {
        let mut status = RuleStatus { rule: (*rule).clone(), served: Vec::new(), exempt: Vec::new(), missing: Vec::new(), tests: tested.get(&rule.id).cloned().unwrap_or_default() };
        for who in proved_anywhere.get(&rule.id).into_iter().flatten() { status.served.push((who.clone(), "proved")); }
        for who in checked_anywhere.get(&rule.id).into_iter().flatten() {
            if !status.served.iter().any(|(w, _)| w == who) { status.served.push((who.clone(), "checked")); }
        }
        for who in handled_anywhere.get(&rule.id).into_iter().flatten() {
            if !status.served.iter().any(|(w, _)| w == who) { status.served.push((who.clone(), "handled")); }
        }
        for r in &report.realizations {
            if !r.terms.iter().any(|t| rule.about.contains(t)) { continue; }
            let who = format!("{}.{}", r.cell, r.signal);
            if let Some((_, why)) = r.exempt.iter().find(|(id, _)| id == &rule.id) {
                status.exempt.push((who, why.clone()));
            } else if r.proved.contains(&rule.id) || r.checked.contains(&rule.id) || r.handled.contains(&rule.id) {
                // served through a helper of the cell: say it under the signal's name too
                let how = if r.proved.contains(&rule.id) { "proved" } else if r.checked.contains(&rule.id) { "checked" } else { "handled" };
                if !status.served.iter().any(|(w, _)| w == &who) { status.served.push((who, how)); }
            } else {
                let term = r.terms.iter().find(|t| rule.about.contains(t)).cloned().unwrap_or_default();
                report.issues.push(Issue { kind: "law_unserved_realization", warning: false, span: r.span,
                    message: format!("the rule \"{}\" ({}{}) is not served in `{}`, which realizes `{}` — add a refusal `under \"{}\"` to its handler (or the statement `under \"{}\"` in the branch that handles the case), or `exempt \"{}\" \"<why it does not apply>\"` to its signal",
                        rule.id, rule.text, if rule.source.is_empty() { String::new() } else { format!(" — {}", rule.source) }, who, term, rule.id, rule.id, rule.id) });
                status.missing.push(who);
            }
        }
        // (said once: when a signal that realizes one of its terms is named
        // above as not serving it, that is the place to serve it)
        if status.served.is_empty() && status.missing.is_empty() {
            report.issues.push(Issue { kind: "law_unserved_rule", warning: false, span: none,
                message: format!("the rule \"{}\" ({}{}) of the law {} is served by nothing in the program — a refusal `require … else Tag \"…\" under \"{}\"` where it applies, or a guarded edge in a frame `under \"{}\"`",
                    rule.id, rule.text, if rule.source.is_empty() { String::new() } else { format!(" — {}", rule.source) }, rule.law, rule.id, rule.id) });
        } else if !status.served.is_empty() && status.tests.is_empty() {
            report.issues.push(Issue { kind: "law_untested_rule", warning: true, span: none,
                message: format!("the rule \"{}\" ({}) has no test: `cell test Name under \"{}\" {{ … }}` with a case it refuses", rule.id, rule.text, rule.id) });
        }
        report.rules.push(status);
    }
    report
}

// ── what `soma law` prints ───────────────────────────────────────────

fn rule_block(s: &RuleStatus, laws: &[Law], realizations: &[Realization], full: bool) -> String {
    let mut out = format!("  {:<10} {}\n", format!("\"{}\"", s.rule.id), s.rule.text);
    out.push_str(&format!("             status   {}{}\n", s.status(), if s.tests.is_empty() && !s.served.is_empty() { ", no test" } else { "" }));
    if !s.rule.source.is_empty() { out.push_str(&format!("             source   {}\n", s.rule.source)); }
    if !s.rule.owner.is_empty() { out.push_str(&format!("             owner    {}\n", s.rule.owner)); }
    if !s.rule.about.is_empty() { out.push_str(&format!("             about    {}\n", s.rule.about.join(", "))); }
    if full {
        for t in &s.rule.about {
            if let Some(term) = laws.iter().flat_map(|l| l.terms.iter()).find(|x| &x.name == t) {
                out.push_str(&format!("             term     {}: {}\n", term.name, term.definition));
            }
            let who: Vec<String> = realizations.iter().filter(|r| r.terms.contains(t)).map(|r| format!("{}.{}", r.cell, r.signal)).collect();
            out.push_str(&format!("             realized {}: {}\n", t, if who.is_empty() { "by no signal of the program".to_string() } else { who.join(", ") }));
        }
    }
    for (who, how) in &s.served { out.push_str(&format!("             served   {} ({})\n", who, how)); }
    for (who, why) in &s.exempt { out.push_str(&format!("             exempt   {} — {}\n", who, why)); }
    for who in &s.missing { out.push_str(&format!("             MISSING  {}\n", who)); }
    if !s.tests.is_empty() { out.push_str(&format!("             tests    {}\n", s.tests.join(", "))); }
    out
}

/// The conformity table, in the order of the law; `only`: one rule, in full.
pub fn render(loaded: &Loaded, report: &Report, lock: &Result<Option<String>, ()>, only: Option<&str>) -> String {
    let mut out = String::new();
    for law in &loaded.laws {
        let rules: Vec<&RuleStatus> = report.rules.iter().filter(|s| s.rule.law == law.name && only.map_or(true, |id| s.rule.id == id)).collect();
        if rules.is_empty() { continue; }
        out.push_str(&format!("law {} ({}) — {}\n", law.name, law.file, match lock { Ok(None) => "as locked", Ok(Some(_)) => "DIFFERS from law.lock", Err(()) => "not locked yet" }));
        for s in rules { out.push_str(&rule_block(s, &loaded.laws, &report.realizations, only.is_some())); }
        out.push('\n');
    }
    if only.is_none() {
        let count = |st: &str| report.rules.iter().filter(|s| s.status() == st).count();
        out.push_str(&format!("{} rule(s): {} proved, {} checked, {} handled, {} incomplete, {} unserved; {} without a test\n",
            report.rules.len(), count("proved"), count("checked"), count("handled"), count("incomplete"), count("unserved"),
            report.rules.iter().filter(|s| s.tests.is_empty()).count()));
        out.push_str(&format!("{} of the program's {} `require` serve a rule\n", report.requires_under, report.requires));
    }
    out
}

/// Only what fails, as a work list.
pub fn todo(loaded: &Loaded, report: &Report, lock: &Result<Option<String>, ()>) -> String {
    let mut lines: Vec<String> = loaded.errors.clone();
    if let Ok(Some(m)) = lock { lines.push(m.clone()); }
    lines.extend(report.issues.iter().map(|i| format!("{}{}", if i.warning { "(warning) " } else { "" }, i.message)));
    if lines.is_empty() { return "nothing to do: every rule of the law is served and tested\n".to_string(); }
    lines.iter().enumerate().map(|(n, l)| format!("{}. {}\n", n + 1, l)).collect()
}

pub fn json(loaded: &Loaded, report: &Report, lock: &Result<Option<String>, ()>) -> serde_json::Value {
    serde_json::json!({
        "laws": loaded.laws.iter().map(|l| serde_json::json!({
            "name": l.name, "file": l.file,
            "terms": l.terms.iter().map(|t| serde_json::json!({"name": t.name, "definition": t.definition})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "lock": match lock { Ok(None) => "as_locked", Ok(Some(_)) => "differs", Err(()) => "absent" },
        "rules": report.rules.iter().map(|s| serde_json::json!({
            "id": s.rule.id, "text": s.rule.text, "law": s.rule.law, "source": s.rule.source, "owner": s.rule.owner,
            "about": s.rule.about, "status": s.status(),
            "served": s.served.iter().map(|(w, h)| serde_json::json!({"signal": w, "how": h})).collect::<Vec<_>>(),
            "exempt": s.exempt.iter().map(|(w, y)| serde_json::json!({"signal": w, "why": y})).collect::<Vec<_>>(),
            "missing": s.missing, "tests": s.tests,
        })).collect::<Vec<_>>(),
        "todo": loaded.errors.iter().cloned().chain(report.issues.iter().map(|i| i.message.clone())).collect::<Vec<_>>(),
        "requires": report.requires, "requires_under": report.requires_under,
    })
}
