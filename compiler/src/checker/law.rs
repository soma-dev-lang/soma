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

/// `state frozen(Holder): Bool "…"` — a fact of the law about one entity (RFC 0003).
#[derive(Debug, Clone)]
pub struct StateDecl {
    pub name: String,
    pub entity: String,
    /// `Bool` or `Int`
    pub ty: String,
    pub definition: String,
    pub line: usize,
}

/// `action Payment(h: Holder, n: Int) "…"` — something that happens (RFC 0003).
#[derive(Debug, Clone)]
pub struct ActionDecl {
    pub name: String,
    /// (name, type): an entity, `Int`, `String` or `Bool`
    pub params: Vec<(String, String)>,
    pub definition: String,
    pub line: usize,
}

/// What a condition compares: a state applied to a parameter, a parameter, a number.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand { State(String, String), Param(String), Int(i64) }

/// The closed condition language of a rule: `not`, `and`, `or`, a state of a
/// parameter, a comparison. No arithmetic, no call, no quantifier — the
/// owner of the law reads it.
#[derive(Debug, Clone, PartialEq)]
pub enum Cond {
    Not(Box<Cond>),
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
    /// a `Bool` state of a parameter
    Holds(String, String),
    Cmp(Operand, String, Operand),
}

/// `requires Payment(h, n): not frozen(h)` — the formal reading of a rule.
#[derive(Debug, Clone)]
pub struct Formal {
    pub action: String,
    pub params: Vec<String>,
    pub cond: Cond,
    /// the condition as the owner wrote it
    pub text: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub text: String,
    pub about: Vec<String>,
    pub source: String,
    pub owner: String,
    pub law: String,
    pub requires: Option<Formal>,
}

#[derive(Debug, Clone)]
pub struct Law {
    pub name: String,
    /// as written in soma.toml
    pub file: String,
    pub terms: Vec<Term>,
    pub rules: Vec<Rule>,
    pub entities: Vec<Term>,
    pub states: Vec<StateDecl>,
    pub actions: Vec<ActionDecl>,
}

/// The laws a program is under, as read from disk.
#[derive(Debug, Default)]
pub struct Loaded {
    pub laws: Vec<Law>,
    /// (file as written in soma.toml, its text)
    pub files: Vec<(String, String)>,
    /// a file that is missing or does not parse: `file:line: message`
    pub errors: Vec<String>,
    /// what the formal part of a law gets wrong (RFC 0003): (kind, `file:line: message`)
    pub formal_errors: Vec<(&'static str, String)>,
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

// ── the formal part (RFC 0003) ───────────────────────────────────────

/// The pieces of a formal line: names, numbers, strings and punctuation.
#[derive(Debug, Clone, PartialEq)]
enum F { Name(String), Num(i64), Text(String), P(String) }

fn formal_tokens(line: &str) -> Result<Vec<F>, String> {
    let c: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() { i += 1; continue; }
        if ch == '/' && c.get(i + 1) == Some(&'/') { break; }
        if ch == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                match c.get(i) {
                    None => return Err("a string is not closed (`\"`)".to_string()),
                    Some('\\') if c.get(i + 1) == Some(&'"') => { s.push('"'); i += 2; }
                    Some('"') => { i += 1; break; }
                    Some(x) => { s.push(*x); i += 1; }
                }
            }
            out.push(F::Text(s));
            continue;
        }
        if ch.is_ascii_digit() {
            let mut s = String::new();
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '_') { if c[i] != '_' { s.push(c[i]); } i += 1; }
            out.push(F::Num(s.parse().map_err(|_| format!("`{}` is not a number a law can hold", s))?));
            continue;
        }
        if ch.is_alphabetic() || ch == '_' {
            let mut s = String::new();
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') { s.push(c[i]); i += 1; }
            out.push(F::Name(s));
            continue;
        }
        let two: String = c[i..(i + 2).min(c.len())].iter().collect();
        if ["==", "!=", "<=", ">="].contains(&two.as_str()) { out.push(F::P(two)); i += 2; continue; }
        out.push(F::P(ch.to_string()));
        i += 1;
    }
    Ok(out)
}

const CLOSED: &str = "a condition is made of `not`, `and`, `or`, parentheses, a state of a parameter (`frozen(h)`) and a comparison (`==`, `!=`, `<`, `<=`, `>`, `>=`) between an Int state or an Int parameter and a number or a parameter";

struct CondParser<'a> { t: &'a [F], i: usize }

impl CondParser<'_> {
    fn peek(&self) -> Option<&F> { self.t.get(self.i) }
    fn word(&self, w: &str) -> bool { matches!(self.peek(), Some(F::Name(n)) if n == w) }
    fn punct(&self, p: &str) -> bool { matches!(self.peek(), Some(F::P(x)) if x == p) }

    fn or(&mut self) -> Result<Cond, String> {
        let mut left = self.and()?;
        while self.word("or") { self.i += 1; left = Cond::Or(Box::new(left), Box::new(self.and()?)); }
        Ok(left)
    }
    fn and(&mut self) -> Result<Cond, String> {
        let mut left = self.not()?;
        while self.word("and") { self.i += 1; left = Cond::And(Box::new(left), Box::new(self.not()?)); }
        Ok(left)
    }
    fn not(&mut self) -> Result<Cond, String> {
        if self.word("not") { self.i += 1; return Ok(Cond::Not(Box::new(self.not()?))); }
        if self.punct("(") {
            self.i += 1;
            let inner = self.or()?;
            if !self.punct(")") { return Err(format!("a `(` is not closed — {}", CLOSED)); }
            self.i += 1;
            return Ok(inner);
        }
        let left = self.operand()?;
        if let Some(F::P(op)) = self.peek().cloned() {
            if ["==", "!=", "<", "<=", ">", ">="].contains(&op.as_str()) {
                self.i += 1;
                let right = self.operand()?;
                return Ok(Cond::Cmp(left, op, right));
            }
            if ["+", "-", "*", "/", "%"].contains(&op.as_str()) {
                return Err(format!("`{}` is arithmetic, which a law does not hold: compare a state or a parameter with a number or a parameter, and let a state carry the sum (`state exposure(Holder): Int`) — {}", op, CLOSED));
            }
        }
        match left {
            Operand::State(s, p) => Ok(Cond::Holds(s, p)),
            // a bare parameter or number is judged with the declarations (it has no Bool reading)
            other => Ok(Cond::Cmp(other, "bare".to_string(), Operand::Int(0))),
        }
    }
    fn operand(&mut self) -> Result<Operand, String> {
        match self.peek().cloned() {
            Some(F::Num(n)) => { self.i += 1; Ok(Operand::Int(n)) }
            Some(F::P(m)) if m == "-" && matches!(self.t.get(self.i + 1), Some(F::Num(_))) => {
                let Some(F::Num(n)) = self.t.get(self.i + 1).cloned() else { unreachable!() };
                self.i += 2;
                Ok(Operand::Int(-n))
            }
            Some(F::Name(n)) if ["forall", "exists", "all", "any", "some"].contains(&n.as_str()) =>
                Err(format!("`{}` is a quantifier, which a law does not hold: a rule speaks of the parameters of ONE action — {}", n, CLOSED)),
            Some(F::Name(n)) => {
                self.i += 1;
                if self.punct("(") {
                    let (Some(F::Name(arg)), Some(F::P(close))) = (self.t.get(self.i + 1).cloned(), self.t.get(self.i + 2).cloned()) else {
                        return Err(format!("`{}(` is followed by ONE parameter of the action and `)`: `{}(h)` — {}", n, n, CLOSED));
                    };
                    if close != ")" { return Err(format!("`{}(…)` takes one parameter: a state is a fact about one entity — {}", n, CLOSED)); }
                    self.i += 3;
                    Ok(Operand::State(n, arg))
                } else { Ok(Operand::Param(n)) }
            }
            Some(F::Text(_)) => Err(format!("a string is compared with nothing in a law: a state is `Bool` or `Int` — write the fact as a `Bool` state (`state frozen(Holder): Bool`) — {}", CLOSED)),
            Some(F::P(p)) => Err(format!("`{}` does not start a condition — {}", p, CLOSED)),
            None => Err(format!("the condition stops short — {}", CLOSED)),
        }
    }
}

/// `Action(p, q): condition` → the formal reading, or what is wrong with it.
fn parse_requires(toks: &[F], line: usize, raw: &str) -> Result<Formal, String> {
    let colon = toks.iter().position(|t| *t == F::P(":".to_string()));
    let (Some(F::Name(action)), Some(colon)) = (toks.first().cloned(), colon) else {
        return Err("`requires` is `requires Action(params): condition` — the `:` separates the action from what it needs: `requires Payment(h, n): not frozen(h)`".to_string());
    };
    let mut params = Vec::new();
    if toks.get(1) != Some(&F::P("(".to_string())) || toks.get(colon - 1) != Some(&F::P(")".to_string())) {
        return Err(format!("the action of a `requires` carries its parameters: `requires {}(h, n): …`", action));
    }
    for t in &toks[2..colon - 1] {
        match t { F::Name(p) => params.push(p.clone()), F::P(c) if c == "," => {}, _ => return Err(format!("the parameters of `{}` are names: `{}(h, n)`", action, action)) }
    }
    let mut p = CondParser { t: &toks[colon + 1..], i: 0 };
    let cond = p.or()?;
    if let Some(rest) = p.peek() {
        let what = match rest { F::Name(n) => format!("`{}`", n), F::P(x) => format!("`{}`", x), F::Num(n) => format!("`{}`", n), F::Text(_) => "a string".to_string() };
        if matches!(rest, F::P(x) if x == "(") {
            if let Some(F::Name(f)) = p.t.get(p.i.saturating_sub(1)) { return Err(format!("`{}` is called like a function, which a law does not hold — {}", f, CLOSED)); }
        }
        return Err(format!("{} does not continue a condition — {}", what, CLOSED));
    }
    let text = raw.split_once(':').map(|(_, c)| c.split("//").next().unwrap_or("").trim().to_string()).unwrap_or_default();
    Ok(Formal { action, params, cond, text, line })
}

/// The formal lines of a law against its own declarations (E0133).
fn check_formal(law: &Law, file: &str) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    let entity = |n: &str| law.entities.iter().any(|e| e.name == n);
    for s in &law.states {
        let at = |m: String| ("law_requires_unknown", format!("{}:{}: {}", file, s.line, m));
        if !entity(&s.entity) { out.push(at(format!("the state `{}` is about `{}`, which the law does not declare — add `entity {} \"…\"` (ask the owner of the law)", s.name, s.entity, s.entity))); }
        if s.ty != "Bool" && s.ty != "Int" { out.push(at(format!("the state `{}` is `{}`: a state of the law is `Bool` or `Int`", s.name, s.ty))); }
    }
    for a in &law.actions {
        for (p, t) in &a.params {
            if !entity(t) && !["Int", "String", "Bool"].contains(&t.as_str()) {
                out.push(("law_requires_unknown", format!("{}:{}: the parameter `{}` of the action `{}` is a `{}`, which the law does not declare — a parameter is an entity of the law, `Int`, `String` or `Bool`; add `entity {} \"…\"` if it is a thing the law speaks of", file, a.line, p, a.name, t, t)));
            }
        }
    }
    for r in &law.rules {
        let Some(f) = &r.requires else { continue };
        let at = |m: String| ("law_requires_unknown", format!("{}:{}: the rule \"{}\": {}", file, f.line, r.id, m));
        let Some(action) = law.actions.iter().find(|a| a.name == f.action) else {
            out.push(at(format!("`requires {}(…)` names an action the law does not declare — add `action {}({}) \"…\"` (ask the owner of the law)", f.action, f.action, f.params.iter().map(|p| format!("{}: …", p)).collect::<Vec<_>>().join(", "))));
            continue;
        };
        let signature = format!("{}({})", action.name, action.params.iter().map(|(p, t)| format!("{}: {}", p, t)).collect::<Vec<_>>().join(", "));
        if action.params.len() != f.params.len() {
            out.push(at(format!("`requires {}({})` gives {} parameter(s), the action is `{}`", f.action, f.params.join(", "), f.params.len(), signature)));
            continue;
        }
        // the names of the `requires` bind by position to the action's types
        let ty = |p: &str| f.params.iter().position(|x| x == p).map(|i| action.params[i].1.clone());
        let mut problems: Vec<String> = Vec::new();
        fn walk(c: &Cond, f: &mut dyn FnMut(&Cond)) {
            f(c);
            match c { Cond::Not(a) => walk(a, f), Cond::And(a, b) | Cond::Or(a, b) => { walk(a, f); walk(b, f); } _ => {} }
        }
        let state_of = |s: &str, p: &str, want: &str, problems: &mut Vec<String>| {
            let Some(decl) = law.states.iter().find(|d| d.name == s) else {
                problems.push(format!("`{}(…)` is not a state of the law — add `state {}(…): {} \"…\"` (ask the owner of the law)", s, s, want));
                return;
            };
            match ty(p) {
                None => problems.push(format!("`{}` in `{}({})` is not a parameter of `{}`", p, s, p, signature)),
                Some(t) if t != decl.entity => problems.push(format!("`{}({})`: `{}` is a state of a {}, and `{}` is a {} in `{}`", s, p, s, decl.entity, p, t, signature)),
                _ => {}
            }
            if decl.ty != want {
                problems.push(if want == "Bool" { format!("`{}({})` stands alone as a condition, and `{}` is an Int state: compare it (`{}({}) < 100`)", s, p, s, s, p) }
                              else { format!("`{}({})` is compared, and `{}` is a Bool state: write `{}({})` or `not {}({})`", s, p, s, s, p, s, p) });
            }
        };
        walk(&f.cond, &mut |c| match c {
            Cond::Holds(s, p) => state_of(s, p, "Bool", &mut problems),
            Cond::Cmp(l, op, r) => {
                for o in [l, r] {
                    match o {
                        Operand::State(s, p) => state_of(s, p, "Int", &mut problems),
                        Operand::Param(p) => match ty(p) {
                            None => problems.push(format!("`{}` is not a parameter of `{}`", p, signature)),
                            Some(t) if t != "Int" => problems.push(format!("`{}` is a {} in `{}`: a comparison is between Int values — a fact about `{}` is a state (`state …({}): Bool`)", p, t, signature, p, t)),
                            _ => {}
                        },
                        Operand::Int(_) => {}
                    }
                    if op == "bare" { break; }
                }
                if op == "bare" && !matches!(l, Operand::Param(p) if ty(p).is_none()) && problems.is_empty() {
                    problems.push("a parameter or a number alone is not a condition: compare it (`n > 0`)".to_string());
                }
            }
            _ => {}
        });
        problems.dedup();
        for p in problems { out.push(at(p)); }
    }
    out
}

/// Reads one `.law` file. A law names no code: its words are `law`, `term`,
/// `entity`, `state`, `action`, `rule`, and under a rule `about`, `source`,
/// `owner`, `requires`.
pub fn parse(file: &str, text: &str) -> (Law, Vec<String>) {
    let (law, errors, _) = parse_all(file, text);
    (law, errors)
}

/// `parse`, with what the formal part gets wrong: (kind, message).
pub fn parse_all(file: &str, text: &str) -> (Law, Vec<String>, Vec<(&'static str, String)>) {
    let mut law = Law { name: String::new(), file: file.to_string(), terms: Vec::new(), rules: Vec::new(), entities: Vec::new(), states: Vec::new(), actions: Vec::new() };
    let mut errors = Vec::new();
    let mut formal: Vec<(&'static str, String)> = Vec::new();
    // the declaration a definition on the next line belongs to
    #[derive(PartialEq)]
    enum Last { None, State, Action }
    let mut last = Last::None;
    for (n, line) in text.lines().enumerate() {
        let at = |m: String| format!("{}:{}: {}", file, n + 1, m);
        let first = line.split_whitespace().next().unwrap_or("");
        if ["entity", "state", "action", "requires"].contains(&first) || (line.trim_start().starts_with('"') && last != Last::None) {
            let ft = match formal_tokens(line) { Ok(t) => t, Err(m) => { errors.push(at(m)); continue; } };
            let texts: Vec<String> = ft.iter().filter_map(|t| if let F::Text(s) = t { Some(s.clone()) } else { None }).collect();
            let def = texts.first().cloned().unwrap_or_default();
            if line.trim_start().starts_with('"') {
                // a definition on its own line, under what it defines
                match last {
                    Last::State => if let Some(s) = law.states.last_mut() { if s.definition.is_empty() { s.definition = def; } },
                    Last::Action => if let Some(a) = law.actions.last_mut() { if a.definition.is_empty() { a.definition = def; } },
                    Last::None => {}
                }
                continue;
            }
            last = Last::None;
            let name = match ft.get(1) { Some(F::Name(s)) => s.clone(), _ => String::new() };
            let taken = law.terms.iter().chain(law.entities.iter()).any(|t| t.name == name) || law.states.iter().any(|s| s.name == name) || law.actions.iter().any(|a| a.name == name);
            match first {
                "entity" => {
                    if name.is_empty() || def.trim().is_empty() { errors.push(at("an entity is a name and its definition in one sentence: `entity Holder \"Whoever holds units of the fund.\"`".to_string())); }
                    else if taken { errors.push(at(format!("`{}` is declared twice", name))); }
                    else { law.entities.push(Term { name, definition: def }); }
                }
                "state" => {
                    // state name ( Entity ) : Type "…"
                    let shape = (ft.get(2), ft.get(3), ft.get(4), ft.get(5), ft.get(6));
                    match shape {
                        (Some(F::P(o)), Some(F::Name(entity)), Some(F::P(c)), Some(F::P(colon)), Some(F::Name(ty))) if o == "(" && c == ")" && colon == ":" && !name.is_empty() => {
                            if taken { errors.push(at(format!("`{}` is declared twice", name))); }
                            else { law.states.push(StateDecl { name, entity: entity.clone(), ty: ty.clone(), definition: def, line: n + 1 }); last = Last::State; }
                        }
                        _ => errors.push(at("a state is a fact about one entity: `state frozen(Holder): Bool \"The holder is under an asset freeze.\"` (its type is `Bool` or `Int`)".to_string())),
                    }
                }
                "action" => {
                    let close = ft.iter().position(|t| *t == F::P(")".to_string()));
                    if name.is_empty() || ft.get(2) != Some(&F::P("(".to_string())) || close.is_none() {
                        errors.push(at("an action is a name and its parameters: `action Payment(h: Holder, n: Int) \"Cash paid to a holder.\"`".to_string()));
                        continue;
                    }
                    let inside = &ft[3..close.unwrap_or(3)];
                    let mut params = Vec::new();
                    let mut ok = true;
                    for part in inside.split(|t| *t == F::P(",".to_string())) {
                        match part {
                            [F::Name(p), F::P(colon), F::Name(t)] if colon == ":" => params.push((p.clone(), t.clone())),
                            [] if inside.is_empty() => {}
                            _ => ok = false,
                        }
                    }
                    if !ok { errors.push(at(format!("each parameter of an action is `name: Type`: `action {}(h: Holder, n: Int)`", name))); }
                    else if taken { errors.push(at(format!("`{}` is declared twice", name))); }
                    else { law.actions.push(ActionDecl { name, params, definition: def, line: n + 1 }); last = Last::Action; }
                }
                _ => {
                    let Some(rule) = law.rules.last_mut() else {
                        errors.push(at("`requires` belongs to a rule: write it under a `rule` line".to_string()));
                        continue;
                    };
                    if rule.requires.is_some() { errors.push(at(format!("the rule \"{}\" has two `requires`: a rule has one formal reading — join them with `and`, or write two rules", rule.id))); continue; }
                    let raw = line.trim_start().strip_prefix("requires").unwrap_or("");
                    match parse_requires(&ft[1..], n + 1, raw) {
                        Ok(f) => rule.requires = Some(f),
                        Err(m) => formal.push(("law_condition", at(format!("the rule \"{}\": {}", rule.id, m)))),
                    }
                }
            }
            continue;
        }
        last = Last::None;
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
                    else { law.rules.push(Rule { id: (*id).clone(), text: (*text).clone(), about: Vec::new(), source: String::new(), owner: String::new(), law: String::new(), requires: None }); }
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
            other => errors.push(at(format!("`{}` is not a word of a law — a law has `law`, `term`, `entity`, `state`, `action`, `rule`, and under a rule `about`, `source`, `owner`, `requires`; it names no cell, no handler and no test", other))),
        }
    }
    if law.name.is_empty() { errors.push(format!("{}: the file does not name its law: start it with `law Name`", file)); }
    let name = law.name.clone();
    for r in law.rules.iter_mut() { r.law = name.clone(); }
    formal.extend(check_formal(&law, file));
    (law, errors, formal)
}

/// The laws `[law] files = […]` of soma.toml names, read beside it.
pub fn load(manifest: Option<&crate::pkg::manifest::Manifest>, root: &Path) -> Loaded {
    let mut out = Loaded::default();
    let Some(m) = manifest else { return out };
    for file in &m.law.files {
        match std::fs::read_to_string(root.join(file)) {
            Ok(text) => {
                let (law, errors, formal) = parse_all(file, &text);
                out.errors.extend(errors);
                out.formal_errors.extend(formal);
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
        // an entity, a state and an action are terms: `about` may name them
        for t in &law.entities { terms.insert(t.name.clone()); }
        for s in &law.states { terms.insert(s.name.clone()); }
        for a in &law.actions { terms.insert(a.name.clone()); }
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

/// The laws `law.lock` holds that soma.toml no longer names: a program does
/// not leave a law by deleting a line of its manifest (RFC 0003, E0140).
pub fn left_behind(loaded: &Loaded, root: &Path) -> Option<String> {
    let locked = std::fs::read_to_string(root.join("law.lock")).ok()?;
    let named: BTreeSet<&str> = loaded.files.iter().map(|(f, _)| f.as_str()).collect();
    let gone: Vec<String> = locked.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once("  ").map(|(_, f)| f.trim().to_string())).filter(|f| !named.contains(f.as_str())).collect();
    if gone.is_empty() { return None; }
    Some(format!("law.lock holds {} that soma.toml no longer names: the program left a law it was reviewed under — restore `[law] files = [{}]` in soma.toml; when the owner of the law has withdrawn it, `soma law <file> --lock` records that",
        gone.iter().map(|f| format!("`{}`", f)).collect::<Vec<_>>().join(", "),
        loaded.files.iter().map(|(f, _)| f.clone()).chain(gone.iter().cloned()).map(|f| format!("\"{}\"", f)).collect::<Vec<_>>().join(", ")))
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
    // (a law the lock holds and soma.toml dropped is `left_behind`'s, said once)
    if changed.is_empty() { return Ok(None); }
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
    /// RFC 0003: the actions of the law this signal is bound to (`realizes A(args)`)
    pub bindings: Vec<(String, Vec<String>)>,
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
    /// RFC 0003: the states of the law this cell defines
    maps: Vec<&'a Spanned<MapsDecl>>,
    slots: Vec<String>,
    skeleton: bool,
    signals: Vec<(&'a SignalDecl, Span)>,
    /// handler name -> the Debug text of its body
    bodies: BTreeMap<String, (String, Span)>,
}

/// The states a condition speaks of.
fn cond_states(c: &Cond, out: &mut Vec<String>) {
    let operand = |o: &Operand, out: &mut Vec<String>| if let Operand::State(s, _) = o { if !out.contains(s) { out.push(s.clone()); } };
    match c {
        Cond::Not(a) => cond_states(a, out),
        Cond::And(a, b) | Cond::Or(a, b) => { cond_states(a, out); cond_states(b, out); }
        Cond::Holds(s, _) => if !out.contains(s) { out.push(s.clone()); },
        Cond::Cmp(l, _, r) => { operand(l, out); operand(r, out); }
    }
}

/// A condition of the law over the names of a signal: `not frozen(holder)`.
fn cond_text(c: &Cond, bind: &dyn Fn(&str) -> String) -> String {
    let operand = |o: &Operand| match o { Operand::State(s, p) => format!("{}({})", s, bind(p)), Operand::Param(p) => bind(p), Operand::Int(n) => n.to_string() };
    match c {
        Cond::Not(a) => format!("not {}", match **a { Cond::And(..) | Cond::Or(..) => format!("({})", cond_text(a, bind)), _ => cond_text(a, bind) }),
        Cond::And(a, b) => format!("{} and {}", match **a { Cond::Or(..) => format!("({})", cond_text(a, bind)), _ => cond_text(a, bind) }, match **b { Cond::Or(..) => format!("({})", cond_text(b, bind)), _ => cond_text(b, bind) }),
        Cond::Or(a, b) => format!("{} or {}", cond_text(a, bind), cond_text(b, bind)),
        Cond::Holds(s, p) => format!("{}({})", s, bind(p)),
        Cond::Cmp(l, op, r) => format!("{} {} {}", operand(l), op, operand(r)),
    }
}

/// RFC 0003, obligation 1 — the derived guard. For each signal bound to an
/// action (`realizes A(args)`) and each rule `requires A(params): C`: C over
/// the signal's arguments, each state replaced by its `maps`. Set on the
/// program when it is loaded, so that every command runs it. What cannot be
/// derived (a state with no `maps`, a binding that does not fit) is left
/// out here and said by `soma check`.
pub fn derive(program: &mut Program, root: &Path) {
    let Ok(manifest) = crate::pkg::manifest::Manifest::load(&root.join("soma.toml")) else { return };
    if manifest.law.files.is_empty() { return; }
    let loaded = load(Some(&manifest), root);
    let formal: Vec<&Rule> = loaded.laws.iter().flat_map(|l| l.rules.iter()).filter(|r| r.requires.is_some()).collect();
    if formal.is_empty() { return; }
    let actions: Vec<&ActionDecl> = loaded.laws.iter().flat_map(|l| l.actions.iter()).collect();
    // state -> (the cell that maps it, the parameter, the expression)
    let mut maps: BTreeMap<String, (String, String, Spanned<Expr>)> = BTreeMap::new();
    fn collect(cells: &[Spanned<CellDef>], maps: &mut BTreeMap<String, (String, String, Spanned<Expr>)>) {
        for c in cells {
            for m in &c.node.maps { maps.entry(m.node.state.clone()).or_insert((c.node.name.clone(), m.node.param.clone(), m.node.expr.clone())); }
            for s in &c.node.sections { if let Section::Interior(i) = &s.node { collect(&i.cells, maps); } }
        }
    }
    collect(&program.cells, &mut maps);
    fn build(c: &Cond, arg: &dyn Fn(&str) -> Option<GuardOperand>, maps: &BTreeMap<String, (String, String, Spanned<Expr>)>) -> Option<GuardCond> {
        let state = |s: &str, p: &str| -> Option<GuardOperand> {
            let (cell, param, expr) = maps.get(s)?;
            Some(GuardOperand::State { cell: cell.clone(), param: param.clone(), expr: expr.clone(), arg: Box::new(arg(p)?) })
        };
        let operand = |o: &Operand| -> Option<GuardOperand> {
            match o { Operand::State(s, p) => state(s, p), Operand::Param(p) => arg(p), Operand::Int(n) => Some(GuardOperand::Int(*n)) }
        };
        Some(match c {
            Cond::Not(a) => GuardCond::Not(Box::new(build(a, arg, maps)?)),
            Cond::And(a, b) => GuardCond::And(Box::new(build(a, arg, maps)?), Box::new(build(b, arg, maps)?)),
            Cond::Or(a, b) => GuardCond::Or(Box::new(build(a, arg, maps)?), Box::new(build(b, arg, maps)?)),
            Cond::Holds(s, p) => GuardCond::Holds(state(s, p)?),
            Cond::Cmp(l, op, r) => GuardCond::Cmp(operand(l)?, op.clone(), operand(r)?),
        })
    }
    fn attach(cells: &mut [Spanned<CellDef>], formal: &[&Rule], actions: &[&ActionDecl], maps: &BTreeMap<String, (String, String, Spanned<Expr>)>) {
        for c in cells.iter_mut() {
            let mut guards: Vec<LawGuard> = Vec::new();
            for s in &c.node.sections {
                let Section::Face(face) = &s.node else { continue };
                for d in &face.declarations {
                    let FaceDecl::Signal(sig) = &d.node else { continue };
                    let Some(frame) = &sig.frame else { continue };
                    for (name, args) in &frame.bindings {
                        let Some(action) = actions.iter().find(|a| &a.name == name) else { continue };
                        if action.params.len() != args.len() { continue; }
                        for rule in formal.iter().filter(|r| r.requires.as_ref().is_some_and(|f| &f.action == name)) {
                            let Some(f) = &rule.requires else { continue };
                            if f.params.len() != args.len() { continue; }
                            // the rule's own names for the action's parameters, bound by position
                            let arg_text = |p: &str| f.params.iter().position(|x| x == p).map(|i| args[i].clone());
                            let cell_name = c.node.name.clone();
                            let arg = |p: &str| -> Option<GuardOperand> {
                                let a = arg_text(p)?;
                                // `o via holder_of`: what the slot of the signal's cell holds for the parameter
                                if let Some((key, slot)) = a.split_once(" via ") {
                                    let nowhere = Span { start: 0, end: 0 };
                                    let get = Expr::MethodCall {
                                        target: Box::new(Spanned::new(Expr::Ident(slot.to_string()), nowhere)), method: "get".to_string(),
                                        args: vec![Spanned::new(Expr::Ident(key.to_string()), nowhere)],
                                    };
                                    return Some(GuardOperand::State { cell: cell_name.clone(), param: key.to_string(), expr: Spanned::new(get, nowhere), arg: Box::new(GuardOperand::Param(key.to_string())) });
                                }
                                Some(if let Some(t) = a.strip_prefix('"') { GuardOperand::Str(t.trim_end_matches('"').to_string()) }
                                     else if let Ok(n) = a.parse::<i64>() { GuardOperand::Int(n) }
                                     else { GuardOperand::Param(a) })
                            };
                            let Some(cond) = build(&f.cond, &arg, maps) else { continue };
                            guards.push(LawGuard {
                                signal: sig.name.clone(), rule: rule.id.clone(), sentence: rule.text.clone(), source: rule.source.clone(),
                                shown: cond_text(&f.cond, &|p| arg_text(p).unwrap_or_else(|| p.to_string())), cond,
                            });
                        }
                    }
                }
            }
            c.node.law_guards = guards;
            for s in c.node.sections.iter_mut() { if let Section::Interior(i) = &mut s.node { attach(&mut i.cells, formal, actions, maps); } }
        }
    }
    attach(&mut program.cells, &formal, &actions, &maps);
}

/// `soma law --map`: the correspondence between the law and the program,
/// for the owner of the law — each state and its `maps`, each action and
/// the signals that realize it, each `silent` signal and its reason.
pub fn dictionary(program: &Program, laws: &[Law]) -> serde_json::Value {
    let mut cells: Vec<CellView> = Vec::new();
    let mut tests = Vec::new();
    views(&program.cells, &mut cells, &mut tests);
    let states: Vec<serde_json::Value> = laws.iter().flat_map(|l| l.states.iter()).map(|s| {
        let found = cells.iter().find_map(|v| v.maps.iter().find(|m| m.node.state == s.name).map(|m| (v.name.clone(), m)));
        serde_json::json!({
            "name": s.name, "entity": s.entity, "type": s.ty, "definition": s.definition,
            "maps": found.map(|(cell, m)| serde_json::json!({"cell": cell, "param": m.node.param, "expression": crate::ast::render_expr(&m.node.expr.node)})),
        })
    }).collect();
    let actions: Vec<serde_json::Value> = laws.iter().flat_map(|l| l.actions.iter()).map(|a| {
        let by: Vec<serde_json::Value> = cells.iter().flat_map(|v| v.signals.iter().map(move |(sig, _)| (v, sig)))
            .filter_map(|(v, sig)| sig.frame.as_ref().and_then(|f| f.bindings.iter().find(|(n, _)| n == &a.name)).map(|(_, args)| serde_json::json!({"signal": format!("{}.{}", v.name, sig.name), "arguments": args})))
            .collect();
        let rules: Vec<String> = laws.iter().flat_map(|l| l.rules.iter()).filter(|r| r.requires.as_ref().is_some_and(|f| f.action == a.name))
            .map(|r| format!("\"{}\" requires {}", r.id, r.requires.as_ref().map(|f| f.text.clone()).unwrap_or_default())).collect();
        serde_json::json!({
            "name": a.name, "definition": a.definition, "rules": rules,
            "params": a.params.iter().map(|(p, t)| serde_json::json!({"name": p, "type": t})).collect::<Vec<_>>(),
            "realized_by": by,
        })
    }).collect();
    let silent: Vec<serde_json::Value> = cells.iter().flat_map(|v| v.signals.iter().map(move |(sig, _)| (v, sig)))
        .filter_map(|(v, sig)| sig.frame.as_ref().and_then(|f| f.silent.as_ref()).map(|why| serde_json::json!({
            "signal": format!("{}.{}", v.name, sig.name), "reason": why,
            "writes": sig.frame.as_ref().map(|f| f.writes.clone()).unwrap_or_default(),
        }))).collect();
    serde_json::json!({ "schema": "soma.law.map/1", "states": states, "actions": actions, "silent": silent })
}

pub fn render_dictionary(d: &serde_json::Value) -> String {
    let text = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();
    let mut out = String::from("The law and the program, side by side — for the owner of the law to review.\n\nstates — a fact of the law, and how the program holds it\n");
    for s in d["states"].as_array().into_iter().flatten() {
        out.push_str(&format!("  {}({}): {}   {}\n", text(&s["name"]), text(&s["entity"]), text(&s["type"]), text(&s["definition"])));
        match s["maps"].as_object() {
            Some(m) => out.push_str(&format!("      {}: {}\n", m["cell"].as_str().unwrap_or(""), m["expression"].as_str().unwrap_or(""))),
            None => out.push_str("      NOT MAPPED: no `maps` of the program defines it\n"),
        }
    }
    if d["states"].as_array().is_none_or(|a| a.is_empty()) { out.push_str("  (the law declares no state)\n"); }
    out.push_str("\nactions — what the law says happens, and the signals that are it\n");
    for a in d["actions"].as_array().into_iter().flatten() {
        let params: Vec<String> = a["params"].as_array().into_iter().flatten().map(|p| format!("{}: {}", text(&p["name"]), text(&p["type"]))).collect();
        out.push_str(&format!("  {}({})   {}\n", text(&a["name"]), params.join(", "), text(&a["definition"])));
        for r in a["rules"].as_array().into_iter().flatten() { out.push_str(&format!("      rule {}\n", text(r))); }
        let by = a["realized_by"].as_array().cloned().unwrap_or_default();
        if by.is_empty() { out.push_str("      realized by no signal of the program\n"); }
        for b in by {
            let args: Vec<String> = b["arguments"].as_array().into_iter().flatten().map(text).collect();
            out.push_str(&format!("      {}({})\n", text(&b["signal"]), args.join(", ")));
        }
    }
    if d["actions"].as_array().is_none_or(|a| a.is_empty()) { out.push_str("  (the law declares no action)\n"); }
    let silent = d["silent"].as_array().cloned().unwrap_or_default();
    if !silent.is_empty() {
        out.push_str("\nsilent — signals that write under the law and are said to be no action of it\n");
        for s in silent {
            let writes: Vec<String> = s["writes"].as_array().into_iter().flatten().map(text).collect();
            out.push_str(&format!("  {}   writes {}\n      \"{}\"\n", text(&s["signal"]), writes.join(", "), text(&s["reason"])));
        }
    }
    out
}

/// The type a `maps` expression has on its face, when it shows one.
fn static_type(e: &Expr) -> Option<&'static str> {
    match e {
        Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::BigInt(_)) => Some("Int"),
        Expr::Literal(Literal::Bool(_)) => Some("Bool"),
        Expr::Literal(Literal::String(_)) => Some("String"),
        Expr::CmpOp { .. } | Expr::Not(_) => Some("Bool"),
        Expr::BinaryOp { op: BinOp::And | BinOp::Or, .. } => Some("Bool"),
        Expr::BinaryOp { .. } => Some("Int"),
        // `a ?? b`: what stands in for a missing value says the type
        Expr::FnCall { name, args } if name == "_coalesce" && args.len() == 2 => static_type(&args[1].node),
        _ => None,
    }
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

fn views<'a>(cells: &'a [Spanned<CellDef>], out: &mut Vec<CellView<'a>>, tests: &mut Vec<(String, Vec<String>, Span, bool)>) {
    for cell in cells {
        if matches!(cell.node.kind, CellKind::Test) {
            if !cell.node.under.is_empty() { tests.push((cell.node.name.clone(), cell.node.under.clone(), cell.span, format!("{:?}", cell.node.sections).contains("AssertFails"))); }
            continue;
        }
        // a skeleton and its flesh are two definitions of one cell
        let at = match out.iter().position(|v| v.name == cell.node.name) {
            Some(i) => i,
            None => { out.push(CellView { name: cell.node.name.clone(), span: cell.span, maps: Vec::new(), slots: Vec::new(), skeleton: false, signals: Vec::new(), bodies: BTreeMap::new() }); out.len() - 1 }
        };
        out[at].maps.extend(cell.node.maps.iter());
        out[at].skeleton |= cell.node.skeleton;
        for section in &cell.node.sections {
            match &section.node {
                Section::Face(face) => for d in &face.declarations {
                    if let FaceDecl::Signal(sig) = &d.node { out[at].signals.push((sig, d.span)); }
                },
                Section::Memory(mem) => for slot in &mem.slots { out[at].slots.push(slot.node.name.clone()); },
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
    let terms: BTreeSet<&str> = laws.iter().flat_map(|l| l.terms.iter().chain(l.entities.iter()).map(|t| t.name.as_str())
        .chain(l.states.iter().map(|s| s.name.as_str())).chain(l.actions.iter().map(|a| a.name.as_str()))).collect();
    let known = |id: &str| rules.iter().any(|r| r.id == id);
    let said = |id: &str| rules.iter().find(|r| r.id == id).map(|r| format!("\"{}\" ({})", r.id, r.text)).unwrap_or_else(|| format!("\"{}\"", id));

    let mut cells: Vec<CellView> = Vec::new();
    let mut tests: Vec<(String, Vec<String>, Span, bool)> = Vec::new();
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

    // RFC 0003 — the correspondence: each `maps` defines a state of the law,
    // once, purely, at its type; each binding fits its action
    let states: Vec<&StateDecl> = laws.iter().flat_map(|l| l.states.iter()).collect();
    let actions: Vec<&ActionDecl> = laws.iter().flat_map(|l| l.actions.iter()).collect();
    let mut mapped: BTreeMap<String, String> = BTreeMap::new();
    for view in &cells {
        for m in &view.maps {
            let d = &m.node;
            let shown = format!("maps {}({}) = {}", d.state, d.param, crate::ast::render_expr(&d.expr.node));
            let Some(decl) = states.iter().find(|s| s.name == d.state) else {
                let known: Vec<String> = states.iter().map(|s| format!("{}({}): {}", s.name, s.entity, s.ty)).collect();
                report.issues.push(Issue { kind: "law_maps_unknown", warning: false, span: m.span,
                    message: format!("`{}` in `{}` defines `{}`, a state no law of the program declares — the states of the law are {}; a new state is added to the law by the person who owns it", shown, view.name, d.state,
                        if known.is_empty() { "none (the law declares `state name(Entity): Bool \"…\"`)".to_string() } else { known.join(", ") }) });
                continue;
            };
            if let Some(other) = mapped.insert(d.state.clone(), view.name.clone()) {
                report.issues.push(Issue { kind: "law_maps_unknown", warning: false, span: m.span,
                    message: format!("the state `{}` is mapped twice: in `{}` and in `{}` — a state of the law has one definition in a program; keep one `maps {}(…)`", d.state, other, view.name, d.state) });
                continue;
            }
            // pure: no write, no effect, nothing that changes from one reading to the next
            let text = format!("{:?}", d.expr);
            let mut why: Vec<String> = Vec::new();
            for (at, _) in text.match_indices("method: \"") {
                let method = text[at + 9..].split('"').next().unwrap_or("");
                if !["set", "delete", "push", "clear", "remove", "insert", "append"].contains(&method) { continue; }
                // the slot: the name the call is made on
                let head = &text[text[..at].rfind("MethodCall { target:").unwrap_or(0)..at];
                let slot = head.rfind("Ident(\"").map(|i| head[i + 7..].split('"').next().unwrap_or("")).unwrap_or("a slot");
                why.push(format!("it writes `{}` (`.{}`)", slot, method));
            }
            for part in text.split("FnCall { name: \"").skip(1) {
                let f = part.split('"').next().unwrap_or("");
                if f == "_coalesce" { continue; }
                match crate::interpreter::builtins::registry::lookup(f) {
                    Some(b) if !b.deterministic || crate::checker::names::EFFECT_BUILTINS.contains(&f) =>
                        why.push(format!("it calls `{}`, whose answer or effect is not a reading of the program's memory", f)),
                    Some(_) => {}
                    // a signal — of this cell, or `Cell.signal`: only one that changes nothing
                    None => {
                        let (cell, name) = f.split_once('.').unwrap_or((view.name.as_str(), f));
                        match cells.iter().find(|v| v.name == cell).and_then(|v| v.signals.iter().find(|(s, _)| s.name == name)) {
                            Some((callee, _)) => if let Some(fr) = &callee.frame {
                                let mut does: Vec<String> = Vec::new();
                                if !fr.writes.is_empty() { does.push(format!("writes {}", fr.writes.join(", "))); }
                                if !fr.moves.is_empty() { does.push("moves a machine".to_string()); }
                                if !fr.emits.is_empty() { does.push(format!("emits {}", fr.emits.join(", "))); }
                                if !does.is_empty() { why.push(format!("it reads through `{}.{}`, whose frame {}", cell, name, does.join(" and "))); }
                            },
                            None => {}
                        }
                    }
                }
            }
            let ty = static_type(&d.expr.node);
            if let Some(t) = ty { if t != decl.ty { why.push(format!("it is {} {}, and the law declares `{}` a {} state", if t == "Int" { "an" } else { "a" }, t, d.state, decl.ty)); } }
            if !why.is_empty() {
                report.issues.push(Issue { kind: "law_maps_impure", warning: false, span: m.span,
                    message: format!("`{}` in `{}`: {} — a `maps` is a pure {} reading of the program: the slots and machines of its cell, or another cell through a read-only signal", shown, view.name, why.join("; "), decl.ty) });
            }
        }
    }
    for view in &cells {
        for (sig, span) in &view.signals {
            let Some(frame) = &sig.frame else { continue };
            let who = format!("{}.{}", view.name, sig.name);
            if let Some(why) = &frame.silent {
                if why.trim().is_empty() {
                    report.issues.push(Issue { kind: "law_silent_reason", warning: false, span: *span,
                        message: format!("`{}` is `silent` without a reason — say why it is no action of the law: `silent \"<why no state of the law changes>\"`", who) });
                }
                if let Some((action, _)) = frame.bindings.first() {
                    report.issues.push(Issue { kind: "law_realizes_signature", warning: false, span: *span,
                        message: format!("`{}` both `realizes {}(…)` and is `silent`: a signal is an action of the law or it is none — keep one of the two clauses", who, action) });
                }
            }
            for (name, args) in &frame.bindings {
                let Some(action) = actions.iter().find(|a| &a.name == name) else { continue };   // said as an unknown term
                let signature = format!("{}({})", action.name, action.params.iter().map(|(p, t)| format!("{}: {}", p, t)).collect::<Vec<_>>().join(", "));
                let mut wrong: Vec<String> = Vec::new();
                if args.len() != action.params.len() {
                    wrong.push(format!("it gives {} argument(s)", args.len()));
                } else {
                    for (arg, (p, want)) in args.iter().zip(&action.params) {
                        let got = if arg.starts_with('"') { Some("String".to_string()) }
                            else if arg.trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) { Some("Int".to_string()) }
                            else if let Some((key, slot)) = arg.split_once(" via ") {
                                if !sig.params.iter().any(|q| q.name == key) {
                                    wrong.push(format!("`{}` in `{}` is not a parameter of `{}`", key, arg, sig.name));
                                } else if !view.slots.iter().any(|s| s == slot) {
                                    wrong.push(format!("`{}` in `{}` is not a slot of `{}` (its slots: {}) — a link is a Map of the signal's own cell", slot, arg, view.name, view.slots.join(", ")));
                                }
                                continue;
                            }
                            else { sig.params.iter().find(|q| &q.name == arg).map(|q| crate::commands::describe::format_type(&q.ty.node)) };
                        match got {
                            None => wrong.push(format!("`{}` is not a parameter of `{}` (its parameters: {})", arg, sig.name, sig.params.iter().map(|q| q.name.clone()).collect::<Vec<_>>().join(", "))),
                            Some(t) => {
                                let fits = match want.as_str() { "Int" | "String" | "Bool" => &t == want, _ => t == "String" || t == "Int" || !["Bool", "Float"].contains(&t.as_str()) && !t.contains('<') };
                                if !fits { wrong.push(format!("`{}` is a {}, and `{}` is `{}: {}`", arg, t, p, p, want)); }
                            }
                        }
                    }
                }
                if !wrong.is_empty() {
                    report.issues.push(Issue { kind: "law_realizes_signature", warning: false, span: *span,
                        message: format!("`{}` realizes `{}({})`: {} — the action is `{}`", who, name, args.join(", "), wrong.join("; "), signature) });
                }
            }
        }
    }

    // RFC 0003, obligation 2 — closure by effects: a slot or a machine a
    // `maps` reads is under the law, and a signal that writes it is an
    // action of the law or says it is none. Judged on the frames.
    // cell -> what of it is under the law (a slot, a machine by name, or ""
    // for any machine of the cell) -> the states that read it. A `maps`
    // reads its own cell directly, and other cells — or its own — through
    // the read-only signals it calls: what those read is under the law too.
    let mut under_law: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for view in &cells {
        for m in &view.maps {
            let text = format!("{:?}", m.node.expr);
            let mut put = |cell: &str, what: &str| {
                let list = under_law.entry(cell.to_string()).or_default().entry(what.to_string()).or_default();
                if !list.contains(&m.node.state) { list.push(m.node.state.clone()); }
            };
            for slot in &view.slots {
                if text.contains(&format!("Ident(\"{}\")", slot)) { put(&view.name, slot); }
            }
            if text.contains("FnCall { name: \"get_status\"") || text.contains("FnCall { name: \"has_state\"") { put(&view.name, ""); }
            for part in text.split("FnCall { name: \"").skip(1) {
                let f = part.split('"').next().unwrap_or("");
                if crate::interpreter::builtins::registry::lookup(f).is_some() || f == "_coalesce" { continue; }
                let (cell, name) = f.split_once('.').unwrap_or((view.name.as_str(), f));
                let Some(frame) = cells.iter().find(|v| v.name == cell).and_then(|v| v.signals.iter().find(|(sg, _)| sg.name == name)).and_then(|(sg, _)| sg.frame.as_ref()) else { continue };
                for r in &frame.reads { put(cell, r); }
            }
        }
    }
    for view in &cells {
        let Some(under) = under_law.get(&view.name) else { continue };
        for (sig, span) in &view.signals {
            let Some(frame) = &sig.frame else { continue };
            if !frame.bindings.is_empty() || frame.silent.is_some() { continue; }
            let mut touched: Vec<String> = frame.writes.iter().filter_map(|w| under.get(w).map(|st| format!("writes `{}`, which `maps {}` reads", w, st.join("`, `maps ")))).collect();
            for (machine, _, _) in &frame.moves {
                // a machine named in a frame, or the cell's own (""): under the law by its name or as "any"
                let states: Vec<String> = under.iter().filter(|(what, _)| what.is_empty() || what.as_str() == machine || (machine.is_empty() && !view.slots.contains(*what)))
                    .flat_map(|(_, st)| st.iter().cloned()).collect();
                if !states.is_empty() {
                    let line = format!("moves a machine of `{}`, whose state `maps {}` reads", view.name, states.join("`, `maps "));
                    if !touched.contains(&line) { touched.push(line); }
                }
            }
            if touched.is_empty() { continue; }
            let hint = frame.realizes.iter().find_map(|t| actions.iter().find(|a| &a.name == t))
                .map(|a| format!("realizes {}({})", a.name, a.params.iter().map(|(p, t)| format!("<{}: {}>", p, t)).collect::<Vec<_>>().join(", ")))
                .unwrap_or_else(|| "realizes <Action>(…)".to_string());
            let which = if hint.contains("<Action>") { format!(" (the actions of the law: {})", actions.iter().map(|a| format!("{}({})", a.name, a.params.iter().map(|(p, t)| format!("{}: {}", p, t)).collect::<Vec<_>>().join(", "))).collect::<Vec<_>>().join("; ")) } else { String::new() };
            report.issues.push(Issue { kind: "law_unbound_effect", warning: false, span: *span,
                message: format!("`{}.{}` {}: it changes a state of the law and does not say which action of the law it is — write `{}` on the signal{}, or `silent \"<why no state of the law changes>\"` if it is none",
                    view.name, sig.name, touched.join(" and "), hint, which) });
        }
    }
    // …and obligation 1's precondition: every state a rule needs is mapped
    for view in &cells {
        for (sig, span) in &view.signals {
            let Some(frame) = &sig.frame else { continue };
            for (name, _) in &frame.bindings {
                for rule in rules.iter().filter(|r| r.requires.as_ref().is_some_and(|f| &f.action == name)) {
                    let Some(f) = &rule.requires else { continue };
                    let mut needed: Vec<String> = Vec::new();
                    cond_states(&f.cond, &mut needed);
                    for st in needed {
                        if mapped.contains_key(&st) { continue; }
                        let decl = states.iter().find(|s| s.name == st);
                        report.issues.push(Issue { kind: "law_state_unmapped", warning: false, span: *span,
                            message: format!("the rule \"{}\" ({}) requires `{}` of `{}.{}`, and no `maps` of the program defines the state `{}`{} — write `maps {}({}) = <{} expression over the memory>` in `skeleton {}` (or in the skeleton that holds that fact)",
                                rule.id, rule.text, f.text, view.name, sig.name, st,
                                decl.map(|d| format!(" ({})", d.definition)).unwrap_or_default(), st,
                                decl.map(|d| d.entity.chars().next().unwrap_or('x').to_lowercase().to_string()).unwrap_or_else(|| "x".to_string()),
                                decl.map(|d| d.ty.clone()).unwrap_or_else(|| "Bool".to_string()), view.name) });
                    }
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
                proved, exempt: frame.exempt.clone(), bindings: frame.bindings.clone(), span: *span,
            });
        }
    }

    // the tests: those of the project's files, and those of this program
    let mut tested: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (id, names) in tests_beside {
        if known(id) { tested.insert(id.clone(), names.clone()); }
    }
    for (name, ids, span, refuses) in &tests {
        for id in ids {
            // a rule with a formal reading is kept by a refusal: a test that
            // shows only the allowed case shows nothing of the rule
            if !refuses && rules.iter().any(|r| &r.id == id && r.requires.is_some()) {
                report.issues.push(Issue { kind: "law_untested_rule", warning: true, span: *span,
                    message: format!("the test `{}` is `under \"{}\"` and refuses nothing: a test of a rule shows a case the rule forbids — add `assert_fails <the forbidden call> matching \"{}\"`", name, id, id) });
            }
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
        // a rule with a formal reading is served by the guard derived from
        // it (RFC 0003): a refusal or a branch tagged by hand no longer counts
        let formal = rule.requires.as_ref();
        if formal.is_none() {
            for who in checked_anywhere.get(&rule.id).into_iter().flatten() {
                if !status.served.iter().any(|(w, _)| w == who) { status.served.push((who.clone(), "checked")); }
            }
            for who in handled_anywhere.get(&rule.id).into_iter().flatten() {
                if !status.served.iter().any(|(w, _)| w == who) { status.served.push((who.clone(), "handled")); }
            }
        }
        for r in &report.realizations {
            if !r.terms.iter().any(|t| rule.about.contains(t)) { continue; }
            let who = format!("{}.{}", r.cell, r.signal);
            if let Some(f) = formal {
                if r.proved.contains(&rule.id) {
                    if !status.served.iter().any(|(w, _)| w == &who) { status.served.push((who, "proved")); }
                } else if r.bindings.iter().any(|(a, _)| a == &f.action) {
                    if !status.served.iter().any(|(w, _)| w == &who) { status.served.push((who, "derived")); }
                } else if r.terms.contains(&f.action) {
                    let action = laws.iter().flat_map(|l| l.actions.iter()).find(|a| a.name == f.action);
                    let shape = action.map(|a| a.params.iter().map(|(p, t)| format!("<{}: {}>", p, t)).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                    report.issues.push(Issue { kind: "law_unserved_realization", warning: false, span: r.span,
                        message: format!("the rule \"{}\" ({}{}) has a formal reading — `requires {}({}): {}` — and `{}` realizes `{}` without saying with which arguments: write `realizes {}({})` on its signal, and the guard is derived from the rule (a refusal written by hand `under \"{}\"` no longer serves it)",
                            rule.id, rule.text, if rule.source.is_empty() { String::new() } else { format!(" — {}", rule.source) }, f.action, f.params.join(", "), f.text, who, f.action, f.action, shape, rule.id) });
                    status.missing.push(who);
                }
                continue;
            }
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
    if let Some(f) = &s.rule.requires { out.push_str(&format!("             formal   requires {}({}): {}\n", f.action, f.params.join(", "), f.text)); }
    if full {
        for t in &s.rule.about {
            if let Some(term) = laws.iter().flat_map(|l| l.terms.iter().chain(l.entities.iter())).find(|x| &x.name == t) {
                out.push_str(&format!("             term     {}: {}\n", term.name, term.definition));
            }
            if let Some(a) = laws.iter().flat_map(|l| l.actions.iter()).find(|x| &x.name == t) {
                out.push_str(&format!("             action   {}({}): {}\n", a.name, a.params.iter().map(|(p, ty)| format!("{}: {}", p, ty)).collect::<Vec<_>>().join(", "), a.definition));
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
            "entities": l.entities.iter().map(|t| serde_json::json!({"name": t.name, "definition": t.definition})).collect::<Vec<_>>(),
            "states": l.states.iter().map(|s| serde_json::json!({"name": s.name, "entity": s.entity, "type": s.ty, "definition": s.definition})).collect::<Vec<_>>(),
            "actions": l.actions.iter().map(|a| serde_json::json!({"name": a.name, "definition": a.definition,
                "params": a.params.iter().map(|(p, t)| serde_json::json!({"name": p, "type": t})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "lock": match lock { Ok(None) => "as_locked", Ok(Some(_)) => "differs", Err(()) => "absent" },
        "rules": report.rules.iter().map(|s| serde_json::json!({
            "id": s.rule.id, "text": s.rule.text, "law": s.rule.law, "source": s.rule.source, "owner": s.rule.owner,
            "about": s.rule.about, "status": s.status(),
            "served": s.served.iter().map(|(w, h)| serde_json::json!({"signal": w, "how": h})).collect::<Vec<_>>(),
            "exempt": s.exempt.iter().map(|(w, y)| serde_json::json!({"signal": w, "why": y})).collect::<Vec<_>>(),
            "missing": s.missing, "tests": s.tests,
            "requires": s.rule.requires.as_ref().map(|f| serde_json::json!({"action": f.action, "params": f.params, "condition": f.text})),
        })).collect::<Vec<_>>(),
        "todo": loaded.errors.iter().cloned().chain(report.issues.iter().map(|i| i.message.clone())).collect::<Vec<_>>(),
        "requires": report.requires, "requires_under": report.requires_under,
    })
}
