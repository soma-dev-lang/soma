// 3.0 — A6: the grammar artifacts. `spec/soma.grammar` is the one source;
// `tools/gen_grammar.py` writes `spec/soma.gbnf`, `spec/grammar.md` and the
// tree-sitter grammar from it. This test reads the GBNF as a constrained
// decoder would and holds it to the corpus: every `.cell` file of the
// repository that `soma` parses is in the grammar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

// ── a GBNF reader ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Node {
    Lit(Vec<char>),
    Class { negated: bool, ranges: Vec<(char, char)> },
    Ref(usize),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Star(Box<Node>),
    Plus(Box<Node>),
    Opt(Box<Node>),
}

struct Grammar {
    names: Vec<String>,
    rules: Vec<Node>,
    lexical: Vec<bool>,
}

struct Reader<'a> { chars: Vec<char>, pos: usize, index: &'a HashMap<String, usize> }

impl Reader<'_> {
    fn skip(&mut self) { while self.pos < self.chars.len() && self.chars[self.pos] == ' ' { self.pos += 1; } }
    fn escape(&mut self) -> char {
        self.pos += 1;
        let c = self.chars[self.pos];
        self.pos += 1;
        match c {
            'n' => '\n', 't' => '\t', 'r' => '\r',
            'x' => { let h: String = self.chars[self.pos..self.pos + 2].iter().collect(); self.pos += 2; char::from_u32(u32::from_str_radix(&h, 16).unwrap()).unwrap() }
            other => other,
        }
    }
    fn alt(&mut self) -> Node {
        let mut branches = vec![self.seq()];
        loop {
            self.skip();
            if self.pos < self.chars.len() && self.chars[self.pos] == '|' { self.pos += 1; branches.push(self.seq()); } else { break; }
        }
        if branches.len() == 1 { branches.pop().unwrap() } else { Node::Alt(branches) }
    }
    fn seq(&mut self) -> Node {
        let mut parts = Vec::new();
        loop {
            self.skip();
            if self.pos >= self.chars.len() || matches!(self.chars[self.pos], '|' | ')') { break; }
            parts.push(self.post());
        }
        if parts.len() == 1 { parts.pop().unwrap() } else { Node::Seq(parts) }
    }
    fn post(&mut self) -> Node {
        let mut node = match self.chars[self.pos] {
            '"' => {
                self.pos += 1;
                let mut text = Vec::new();
                while self.chars[self.pos] != '"' {
                    if self.chars[self.pos] == '\\' { text.push(self.escape()); } else { text.push(self.chars[self.pos]); self.pos += 1; }
                }
                self.pos += 1;
                Node::Lit(text)
            }
            '[' => {
                self.pos += 1;
                let negated = self.chars[self.pos] == '^';
                if negated { self.pos += 1; }
                let mut ranges = Vec::new();
                while self.chars[self.pos] != ']' {
                    let lo = if self.chars[self.pos] == '\\' { self.escape() } else { self.pos += 1; self.chars[self.pos - 1] };
                    if self.chars[self.pos] == '-' && self.chars[self.pos + 1] != ']' {
                        self.pos += 1;
                        let hi = if self.chars[self.pos] == '\\' { self.escape() } else { self.pos += 1; self.chars[self.pos - 1] };
                        ranges.push((lo, hi));
                    } else {
                        ranges.push((lo, lo));
                    }
                }
                self.pos += 1;
                Node::Class { negated, ranges }
            }
            '(' => { self.pos += 1; let inner = self.alt(); self.skip(); assert_eq!(self.chars[self.pos], ')'); self.pos += 1; inner }
            _ => {
                let start = self.pos;
                while self.pos < self.chars.len() && (self.chars[self.pos].is_ascii_alphanumeric() || self.chars[self.pos] == '-') { self.pos += 1; }
                let name: String = self.chars[start..self.pos].iter().collect();
                Node::Ref(*self.index.get(&name).unwrap_or_else(|| panic!("undefined rule `{name}`")))
            }
        };
        while self.pos < self.chars.len() && matches!(self.chars[self.pos], '*' | '+' | '?') {
            node = match self.chars[self.pos] { '*' => Node::Star(Box::new(node)), '+' => Node::Plus(Box::new(node)), _ => Node::Opt(Box::new(node)) };
            self.pos += 1;
        }
        node
    }
}

fn read_gbnf(text: &str) -> Grammar {
    let mut defs: Vec<(String, String, bool)> = Vec::new();
    let mut lexical = false;
    for line in text.lines() {
        if line.trim() == "# @lexical" { lexical = true; continue; }
        if line.starts_with('#') || line.trim().is_empty() { continue; }
        let (name, body) = line.split_once("::=").expect("a rule");
        defs.push((name.trim().to_string(), body.trim().to_string(), lexical));
        lexical = false;
    }
    let index: HashMap<String, usize> = defs.iter().enumerate().map(|(i, d)| (d.0.clone(), i)).collect();
    let rules = defs.iter().map(|(_, body, _)| {
        let mut r = Reader { chars: body.chars().collect(), pos: 0, index: &index };
        let node = r.alt();
        assert_eq!(r.pos, r.chars.len(), "trailing text in `{body}`");
        node
    }).collect();
    Grammar { names: defs.iter().map(|d| d.0.clone()).collect(), rules, lexical: defs.iter().map(|d| d.2).collect() }
}

// ── recognition: every end position a node can reach ───────────────────

struct Run<'a> {
    g: &'a Grammar,
    text: &'a [char],
    memo: HashMap<(u32, u32), Rc<Vec<u32>>>,
    furthest: u32,
}

impl Run<'_> {
    fn ends(&mut self, node: &Node, pos: u32) -> Rc<Vec<u32>> {
        match node {
            Node::Lit(lit) => {
                let p = pos as usize;
                if self.text.len() >= p + lit.len() && self.text[p..p + lit.len()] == lit[..] {
                    let e = (p + lit.len()) as u32;
                    self.furthest = self.furthest.max(e);
                    Rc::new(vec![e])
                } else { Rc::new(vec![]) }
            }
            Node::Class { negated, ranges } => match self.text.get(pos as usize) {
                Some(c) if ranges.iter().any(|(lo, hi)| lo <= c && c <= hi) != *negated => { self.furthest = self.furthest.max(pos + 1); Rc::new(vec![pos + 1]) }
                _ => Rc::new(vec![]),
            },
            Node::Ref(rule) => {
                let key = (*rule as u32, pos);
                if let Some(hit) = self.memo.get(&key) { return hit.clone(); }
                let g = self.g;
                let mut found = self.ends(&g.rules[*rule], pos);
                // a token takes the longest text it can
                if g.lexical[*rule] && found.len() > 1 { found = Rc::new(vec![*found.iter().max().unwrap()]); }
                self.memo.insert(key, found.clone());
                found
            }
            Node::Seq(parts) => {
                let mut at = vec![pos];
                for part in parts {
                    let mut next: Vec<u32> = Vec::new();
                    for p in &at { next.extend(self.ends(part, *p).iter()); }
                    next.sort_unstable();
                    next.dedup();
                    if next.is_empty() { return Rc::new(next); }
                    at = next;
                }
                Rc::new(at)
            }
            Node::Alt(branches) => {
                let mut all: Vec<u32> = Vec::new();
                for b in branches { all.extend(self.ends(b, pos).iter()); }
                all.sort_unstable();
                all.dedup();
                Rc::new(all)
            }
            Node::Opt(inner) => {
                let mut all = vec![pos];
                all.extend(self.ends(inner, pos).iter());
                all.sort_unstable();
                all.dedup();
                Rc::new(all)
            }
            Node::Star(inner) | Node::Plus(inner) => {
                let mut seen: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
                let mut work = vec![pos];
                let mut reached: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
                if matches!(node, Node::Star(_)) { reached.insert(pos); }
                while let Some(p) = work.pop() {
                    if !seen.insert(p) { continue; }
                    for e in self.ends(inner, p).iter() {
                        if *e > p { reached.insert(*e); work.push(*e); }
                    }
                }
                Rc::new(reached.into_iter().collect())
            }
        }
    }
}

/// Is `text` in the grammar? Otherwise the furthest character any rule reached.
fn accepts(g: &Grammar, text: &str) -> Result<(), usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut run = Run { g, text: &chars, memo: HashMap::new(), furthest: 0 };
    let root = g.names.iter().position(|n| n == "root").expect("a `root` rule");
    let ends = run.ends(&Node::Ref(root), 0);
    if ends.contains(&(chars.len() as u32)) { Ok(()) } else { Err(run.furthest as usize) }
}

fn repo() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf() }

fn grammar() -> Grammar {
    read_gbnf(&std::fs::read_to_string(repo().join("spec/soma.gbnf")).expect("spec/soma.gbnf — run tools/gen_grammar.py"))
}

fn cell_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if matches!(name.as_str(), ".claude" | "node_modules" | "target" | ".git" | ".soma_env" | ".soma_data") { continue; }
            cell_files(&path, out);
        } else if name.ends_with(".cell") {
            out.push(path);
        }
    }
}

fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(256 * 1024 * 1024).spawn(f).unwrap().join().unwrap()
}

#[test]
fn a6_every_cell_file_soma_parses_is_in_the_grammar() {
    let mut files = Vec::new();
    cell_files(&repo(), &mut files);
    files.sort();
    assert!(files.len() > 300, "the corpus is the repository's .cell files: found {}", files.len());
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    let chunks: Vec<Vec<PathBuf>> = (0..workers).map(|w| files.iter().skip(w).step_by(workers).cloned().collect()).collect();
    let handles: Vec<_> = chunks.into_iter().map(|chunk| std::thread::Builder::new().stack_size(256 * 1024 * 1024).spawn(move || {
        let g = grammar();
        let (mut parsed, mut failures) = (0usize, Vec::new());
        for file in chunk {
            // the corpus is what `soma` parses: a fixture written to fail is not in it
            let ok = Command::new(env!("CARGO_BIN_EXE_soma")).arg("ast").arg(&file).env("SOMA_AST_QUIET", "1")
                .output().map(|o| o.status.success()).unwrap_or(false);
            if !ok { continue; }
            parsed += 1;
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            if let Err(at) = accepts(&g, &text) {
                let chars: Vec<char> = text.chars().collect();
                let line = chars[..at.min(chars.len())].iter().filter(|c| **c == '\n').count() + 1;
                let from = at.saturating_sub(60);
                let near: String = chars[from..(at + 40).min(chars.len())].iter().collect();
                failures.push(format!("{}:{}  …{}…", file.display(), line, near.replace('\n', "⏎")));
            }
        }
        (parsed, failures)
    }).unwrap()).collect();
    let (mut parsed, mut failures) = (0usize, Vec::new());
    for h in handles { let (p, f) = h.join().unwrap(); parsed += p; failures.extend(f); }
    failures.sort();
    if std::env::var("SOMA_GRAMMAR_REPORT").is_ok() {
        for f in &failures { eprintln!("{f}"); }
        eprintln!("{} of {} parsed files are not in the grammar", failures.len(), parsed);
    }
    assert!(parsed > 300, "soma parsed only {parsed} files of the corpus");
    assert!(failures.is_empty(), "{} of {} files `soma` parses are not in spec/soma.gbnf — first: {}", failures.len(), parsed, failures[0]);
}

#[test]
fn a6_the_grammar_refuses_what_is_not_soma() {
    on_big_stack(|| {
        let g = grammar();
        let good = "cell A {\n    face { signal f(n: Int) -> Int }\n    on f(n: Int) {\n        let k = n * 2 + 1\n        if k > 3 { return k }\n        return match k { 1 -> \"one\"  _ -> \"other\" }\n    }\n}\n";
        assert!(accepts(&g, good).is_ok());
        for (what, bad) in [
            ("an unclosed cell", good.trim_end().strip_suffix('}').unwrap().to_string()),
            ("a handler without a body", good.replace("on f(n: Int) {", "on f(n: Int)")),
            ("an operator with no right side", good.replace("return k }", "return k + }")),
            ("a statement at the top of a cell", good.replace("    face {", "    return 1\n    face {")),
            ("an unclosed call", good.replace("return k }", "return f(k }")),
            ("a signal without parentheses", good.replace("signal f(n: Int) -> Int", "signal f -> Int")),
            ("text after the last cell", format!("{good}}}")),
        ] {
            assert!(accepts(&g, &bad).is_err(), "{what} is in the grammar:\n{bad}");
        }
    });
}

#[test]
fn a6_the_artifacts_are_generated_from_the_one_source() {
    let Ok(out) = Command::new("python3").arg(repo().join("tools/gen_grammar.py")).arg("--check").output() else { return };
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}
