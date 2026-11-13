//! `soma refs app.cell Cell.signal`: where a signal is declared, where its
//! body is, and every place that calls or emits it — in the program and in
//! every other `.cell` file of its directory (the tests), as `file:line`.
//!
//! It reads each file on its own, by syntax: a call is `Cell.signal(…)`
//! anywhere, `signal(…)` inside `Cell` (or in a test, where a bare name is
//! any cell's), `emit signal(…)`, and the same inside a string's `{…}`.

use std::path::{Path, PathBuf};
use crate::ast::*;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports};

pub struct Ref { pub what: &'static str, pub by: String, pub file: String, pub line: usize }

fn walk_expr(e: &Spanned<Expr>, f: &mut dyn FnMut(&Expr, Span)) {
    f(&e.node, e.span);
    let mut ex = |x: &Spanned<Expr>| walk_expr(x, f);
    match &e.node {
        Expr::FnCall { args, .. } => { for a in args { ex(a); } }
        Expr::MethodCall { target, args, .. } => { ex(target); for a in args { ex(a); } }
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => { ex(left); ex(right); }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => ex(i),
        Expr::FieldAccess { target, .. } => ex(target),
        Expr::Index { target, index } => { ex(target); ex(index); }
        Expr::ListLiteral(items) => { for it in items { ex(it); } }
        Expr::Record { fields, .. } => { for (_, v) in fields { ex(v); } }
        Expr::Lambda { body, .. } => ex(body),
        Expr::LambdaBlock { stmts, result, .. } => { walk_stmts(stmts, f); walk_expr(result, f); }
        Expr::Match { subject, arms } => {
            walk_expr(subject, f);
            for arm in arms {
                if let Some(g) = &arm.guard { walk_expr(g, f); }
                walk_stmts(&arm.body, f);
                walk_expr(&arm.result, f);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            walk_expr(condition, f);
            walk_stmts(then_body, f); walk_expr(then_result, f);
            walk_stmts(else_body, f); walk_expr(else_result, f);
        }
        _ => {}
    }
}

fn walk_constraint(c: &Constraint, span: Span, f: &mut dyn FnMut(&Expr, Span)) {
    match c {
        Constraint::Comparison { left, right, .. } => { walk_expr(left, f); walk_expr(right, f); }
        Constraint::Predicate { name, args } => { f(&Expr::FnCall { name: name.clone(), args: args.clone() }, span); for a in args { walk_expr(a, f); } }
        Constraint::And(a, b) | Constraint::Or(a, b) => { walk_constraint(&a.node, a.span, f); walk_constraint(&b.node, b.span, f); }
        Constraint::Not(i) => walk_constraint(&i.node, i.span, f),
        Constraint::Descriptive(_) => {}
    }
}

/// Every expression of a body with its span; a statement-level call or
/// emit is handed over as the expression it amounts to.
fn walk_stmts(stmts: &[Spanned<Statement>], f: &mut dyn FnMut(&Expr, Span)) {
    for st in stmts {
        match &st.node {
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } | Statement::Ensure { condition: value } => walk_expr(value, f),
            Statement::ExprStmt { expr } => walk_expr(expr, f),
            Statement::IndexSet { index, value, .. } => { walk_expr(index, f); walk_expr(value, f); }
            Statement::If { condition, then_body, else_body } => { walk_expr(condition, f); walk_stmts(then_body, f); walk_stmts(else_body, f); }
            Statement::For { iter, body, .. } => { walk_expr(iter, f); walk_stmts(body, f); }
            Statement::While { condition, body, .. } => { walk_expr(condition, f); walk_stmts(body, f); }
            Statement::MethodCall { target, method, args } => {
                let t = Spanned::new(Expr::Ident(target.clone()), st.span);
                f(&Expr::MethodCall { target: Box::new(t), method: method.clone(), args: args.clone() }, st.span);
                for a in args { walk_expr(a, f); }
            }
            Statement::Emit { signal_name, args } => {
                f(&Expr::FnCall { name: format!("emit {}", signal_name), args: Vec::new() }, st.span);
                for a in args { walk_expr(a, f); }
            }
            Statement::Require { constraint, else_signal } => {
                walk_constraint(&constraint.node, constraint.span, f);
                f(&Expr::Literal(Literal::String(else_signal.clone())), st.span);
            }
            Statement::Break | Statement::Continue => {}
        }
    }
}

/// What the files of a directory say of a name: which cells exist, and
/// whether `cell` alone has a handler of that name (a bare `signal(…)` or
/// `x.signal()` written in another cell then reaches it).
pub struct Names { pub cells: std::collections::HashSet<String>, pub unique: bool, pub definers: std::collections::HashSet<String> }

fn parse_alone(text: &str) -> Option<Program> {
    let tokens = crate::lexer::Lexer::new(text).tokenize().ok()?;
    crate::parser::Parser::new(tokens).parse_program().ok()
}

/// The references to `cell.signal` in one file, read on its own.
fn refs_in(text: &str, shown: &str, cell: &str, signal: &str, names: &Names, out: &mut Vec<Ref>) {
    let Some(program) = parse_alone(text) else { return };
    let line = |span: Span| crate::interpreter::span_to_location(text, span.start).0;
    fn visit<'a>(cells: &'a [Spanned<CellDef>], out: &mut Vec<&'a Spanned<CellDef>>) {
        for c in cells {
            out.push(c);
            for s in &c.node.sections { if let Section::Interior(i) = &s.node { visit(&i.cells, out); } }
        }
    }
    let mut all = Vec::new();
    visit(&program.cells, &mut all);
    for c in all {
        let here = c.node.name == cell;
        let test = c.node.kind == CellKind::Test;
        // does `e` call (or emit) the signal, written in a cell named `c`?
        // a bare name reaches `cell.signal` from the cell itself, from a test,
        // and from any cell when no other cell has a handler of that name
        let bare = here || test || names.unique;
        let hit = |e: &Expr| -> Option<&'static str> {
            match e {
                Expr::MethodCall { target, method, .. } if method == signal && matches!(&target.node, Expr::Ident(t) if t == cell) => Some("call"),
                // `x.signal()`: the handler, by its first argument
                Expr::MethodCall { target, method, .. } if method == signal && bare && !matches!(&target.node, Expr::Ident(t) if names.cells.contains(t)) => Some("call"),
                Expr::FnCall { name, .. } if name == signal && bare => Some("call"),
                // `vote(Cell.signal, …)`, `horde(Cell.signal, …)`: the handler, named
                Expr::FieldAccess { target, field } if field == signal && matches!(&target.node, Expr::Ident(t) if t == cell) => Some("call"),
                // `vote("Cell.signal", …)`, `horde("Cell.signal", …)`, `std.delegate("Cell", "signal", …)`
                Expr::FnCall { name, args } if matches!(name.as_str(), "vote" | "horde") && args.iter().any(|a| {
                    let mut named = false;
                    crate::checker::literals::for_each_in_expr(&a.node, &mut |x| if let Expr::Literal(Literal::String(t)) = x { if *t == format!("{}.{}", cell, signal) { named = true; } });
                    named
                }) => Some("call"),
                Expr::FnCall { name, args } if name == "_std" && matches!((args.first().map(|a| &a.node), args.get(1).map(|a| &a.node), args.get(2).map(|a| &a.node)),
                    (Some(Expr::Literal(Literal::String(f))), Some(Expr::Literal(Literal::String(c))), Some(Expr::Literal(Literal::String(h)))) if f == "delegate" && c == cell && h == signal) => Some("call"),
                // …and by a bare name or a string: `horde(signal, …)`, `map("on_result", "signal")`, `delegate(c, "signal", …)`
                Expr::FnCall { name, args } if matches!(name.as_str(), "vote" | "horde" | "delegate") && bare && args.iter().any(|a| {
                    let mut named = false;
                    crate::checker::literals::for_each_in_expr(&a.node, &mut |x| match x {
                        Expr::Ident(n) if n == signal => named = true,
                        Expr::Literal(Literal::String(t)) if t == signal => named = true,
                        _ => {}
                    });
                    named
                }) => Some("call"),
                Expr::FnCall { name, .. } if name.strip_prefix("emit ") == Some(signal) => Some("emit"),
                Expr::FnCall { name, args } if name == "delegate" && matches!((args.first().map(|a| &a.node), args.get(1).map(|a| &a.node)),
                    (Some(Expr::Literal(Literal::String(c))), Some(Expr::Literal(Literal::String(h)))) if c == cell && h == signal) => Some("call"),
                _ => None,
            }
        };
        // the calls a string makes in its `{…}`, strings inside them included
        fn in_strings(e: &Expr, depth: usize, f: &mut dyn FnMut(&Expr)) {
            if depth > 6 { return; }
            if let Expr::Literal(Literal::String(s)) = e {
                for seg in crate::checker::desugar::segments(s) {
                    if let Some(x) = crate::checker::desugar::parse_segment(&seg) {
                        let inner = Spanned::new(x, Span::new(0, 0));
                        walk_expr(&inner, &mut |ie, _| { f(ie); in_strings(ie, depth + 1, f); });
                    }
                }
            }
        }
        let scan = |by: &str, e: &Expr, span: Span, out: &mut Vec<Ref>| {
            let mut push = |what: &'static str| {
                let r = Ref { what, by: by.to_string(), file: shown.to_string(), line: line(span) };
                if !out.iter().any(|o| o.what == r.what && o.by == r.by && o.file == r.file && o.line == r.line) { out.push(r); }
            };
            if let Some(what) = hit(e) { push(what); }
            in_strings(e, 0, &mut |ie| if let Some(what) = hit(ie) { push(what); });
        };
        for s in &c.node.sections {
            match &s.node {
                Section::Face(face) if here && c.node.kind != CellKind::Flesh => for d in &face.declarations {
                    let named = match &d.node { FaceDecl::Signal(sig) => Some(&sig.name), FaceDecl::Tool(t) => Some(&t.name), _ => None };
                    if named == Some(&signal.to_string()) { out.push(Ref { what: "declared", by: String::new(), file: shown.to_string(), line: line(d.span) }); }
                },
                Section::OnSignal(on) => {
                    if here && on.signal_name == signal && !on.properties.iter().any(|p| p == HOLE) {
                        out.push(Ref { what: "body", by: String::new(), file: shown.to_string(), line: line(s.span) });
                    }
                    let by = if test { format!("test {} (helper {})", c.node.name, on.signal_name) } else { format!("{}.{}", c.node.name, on.signal_name) };
                    walk_stmts(&on.body, &mut |e, span| scan(&by, e, span, out));
                }
                Section::Every(e) | Section::After(e) => {
                    let by = format!("{} (a schedule)", c.node.name);
                    walk_stmts(&e.body, &mut |x, span| scan(&by, x, span, out));
                }
                Section::Runtime(rt) => {
                    let by = format!("{} (runtime {{ }})", c.node.name);
                    let body = crate::checker::runtime_section::as_statements(rt);
                    walk_stmts(&body, &mut |x, span| scan(&by, x, span, out));
                }
                Section::Rules(rules) => {
                    let by = format!("test {}", c.node.name);
                    for r in &rules.rules {
                        match &r.node {
                            Rule::Assert(e) | Rule::AssertFails(e) | Rule::AssertFailsMatching(e, _) | Rule::Let { value: e, .. }
                            | Rule::MockThink { reply: e, .. } | Rule::MockApprove { reply: e } | Rule::Property { body: e, .. } =>
                                walk_expr(e, &mut |x, span| scan(&by, x, span, out)),
                            Rule::MockHandler { name, reply, .. } => {
                                if name == &format!("{}.{}", cell, signal) || name == signal { out.push(Ref { what: "mock", by: by.clone(), file: shown.to_string(), line: line(r.span) }); }
                                walk_expr(reply, &mut |x, span| scan(&by, x, span, out));
                            }
                            Rule::Check(stmts) => walk_stmts(stmts, &mut |x, span| scan(&by, x, span, out)),
                            _ => {}
                        }
                    }
                }
                Section::State(sm) => for t in &sm.transitions {
                    let by = format!("{} (effect of {} -> {})", c.node.name, t.node.from, t.node.to);
                    walk_stmts(&t.node.effect, &mut |x, span| scan(&by, x, span, out));
                },
                _ => {}
            }
        }
    }
}

fn cell_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') || name == "node_modules" { continue; }
        if p.is_dir() { cell_files(&p, out); } else if name.ends_with(".cell") { out.push(p); }
    }
}

/// The files a question about `cell.signal` is asked of: the program of
/// `entry` and the files of its directory that import it (its tests) —
/// not a `.cell` file that merely lies there (a backup, another program).
/// With what they say of the name.
fn survey(entry: &Path, cell: &str, signal: &str) -> (PathBuf, Vec<(PathBuf, String)>, Names) {
    let beside = entry.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    // the project: the folder of the entry, or — when the entry sits in a
    // sub-folder — the nearest folder above it that holds a soma.toml
    let dir = {
        let start = std::fs::canonicalize(&beside).unwrap_or_else(|_| beside.clone());
        let above = start.ancestors().skip(1).take(4).find(|d| d.join("soma.toml").is_file()).map(|d| d.to_path_buf());
        if start.join("soma.toml").is_file() { beside.clone() } else { above.unwrap_or_else(|| beside.clone()) }
    };
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut files = Vec::new();
    cell_files(&dir, &mut files);
    let texts: Vec<(PathBuf, String)> = files.into_iter().filter_map(|f| std::fs::read_to_string(&f).ok().map(|t| (f, t))).collect();
    let parsed: Vec<Option<Program>> = texts.iter().map(|(_, t)| parse_alone(t)).collect();
    // who imports whom (beside the importer, then from the entry's directory;
    // `use lib::flesh` may name a directory: every file in it)
    let known: std::collections::HashSet<PathBuf> = texts.iter().map(|(f, _)| real(f)).collect();
    let mut imports: std::collections::HashMap<PathBuf, Vec<PathBuf>> = std::collections::HashMap::new();
    for ((f, _), p) in texts.iter().zip(&parsed) {
        let Some(p) = p else { continue };
        let base = f.parent().unwrap_or(Path::new("."));
        for u in &p.imports {
            for t in import_targets(u, base, &dir) { if known.contains(&t) { imports.entry(real(f)).or_default().push(t); } }
        }
    }
    let closure = |from: &PathBuf| -> std::collections::HashSet<PathBuf> {
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![from.clone()];
        while let Some(x) = stack.pop() {
            if !seen.insert(x.clone()) { continue; }
            for y in imports.get(&x).into_iter().flatten() { stack.push(y.clone()); }
        }
        seen
    };
    let me = real(entry);
    let program = closure(&me);
    let related = |f: &PathBuf| { let f = real(f); program.contains(&f) || closure(&f).iter().any(|x| program.contains(x)) };
    let mut cells = std::collections::HashSet::new();
    let mut definers = std::collections::HashSet::new();
    let mut out = Vec::new();
    for ((f, text), p) in texts.iter().zip(&parsed) {
        let Some(p) = p else {
            // it may be a test of this program: say it was not read
            if text.contains(signal) {
                eprintln!("warning: {} does not parse — its references, if any, are not listed (`soma check {}`)", f.strip_prefix(&dir).unwrap_or(f).display(), f.display());
            }
            continue;
        };
        if !related(f) { continue; }
        fn visit(cs: &[Spanned<CellDef>], signal: &str, cells: &mut std::collections::HashSet<String>, definers: &mut std::collections::HashSet<String>) {
            for c in cs {
                if c.node.kind == CellKind::Test { continue; }
                cells.insert(c.node.name.clone());
                for s in &c.node.sections {
                    match &s.node {
                        Section::OnSignal(on) if on.signal_name == signal => { definers.insert(c.node.name.clone()); }
                        Section::Interior(i) => visit(&i.cells, signal, cells, definers),
                        _ => {}
                    }
                }
            }
        }
        visit(&p.cells, signal, &mut cells, &mut definers);
        out.push((f.clone(), text.clone()));
    }
    let unique = definers.iter().all(|d| d == cell) && !crate::checker::names::builtin_names().contains(signal);
    (dir, out, Names { cells, unique, definers })
}

/// The files an import may name, as the loader looks them up: beside the
/// importer, then from the directory; a directory of that name is all its
/// files; `use name` is a package when one is installed under that name.
pub fn import_targets(u: &str, base: &Path, root: &Path) -> Vec<PathBuf> {
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if u.starts_with("std:") { return Vec::new(); }
    let rel = match (u.strip_prefix("lib:"), u.strip_prefix("pkg:")) {
        (Some(r), _) => format!("lib/{}", r),
        (_, Some(r)) => {
            if [root.join("packages").join(r), root.join(".soma_env/packages").join(r)].iter().any(|p| p.exists()) { return Vec::new(); }
            r.to_string()
        }
        _ => u.to_string(),
    };
    let bare = rel.strip_suffix(".cell").unwrap_or(&rel).to_string();
    for from in [base, root] {
        let file = from.join(format!("{}.cell", bare));
        if file.is_file() { return vec![real(&file)]; }
        let folder = from.join(&bare);
        if folder.is_dir() {
            // (one level, as the loader reads an imported folder: a file deeper
            // down is not part of the program)
            let mut inside: Vec<PathBuf> = std::fs::read_dir(&folder).map(|d| d.flatten().map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "cell")).collect()).unwrap_or_default();
            inside.sort();
            if !inside.is_empty() { return inside.iter().map(|f| real(f)).collect(); }
        }
    }
    Vec::new()
}

/// Every reference to `cell.signal` in the program of `entry` and its tests.
pub fn collect(entry: &Path, cell: &str, signal: &str) -> Vec<Ref> {
    let (dir, files, names) = survey(entry, cell, signal);
    let mut out = Vec::new();
    for (f, text) in &files {
        let shown = f.strip_prefix(&dir).unwrap_or(f).display().to_string();
        refs_in(text, &shown, cell, signal, &names, &mut out);
    }
    out
}

pub fn cmd_refs(path: &PathBuf, target: &str, json: bool) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    let skeletons = crate::checker::frame::build(&program);
    let (cell, signal) = match super::skeleton::locate(&skeletons, target) {
        Ok(found) => found,
        Err(msg) => { eprintln!("error: {}", msg); std::process::exit(1); }
    };
    let refs = collect(path, &cell, &signal);
    if json {
        let rows: Vec<serde_json::Value> = refs.iter().map(|r| serde_json::json!({ "what": r.what, "by": r.by, "file": r.file, "line": r.line })).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "signal": format!("{}.{}", cell, signal), "refs": rows })).unwrap());
        return;
    }
    println!("{}.{}", cell, signal);
    let width = refs.iter().map(|r| r.by.chars().count()).max().unwrap_or(0);
    for what in ["declared", "body", "call", "emit", "mock"] {
        let rows: Vec<&Ref> = refs.iter().filter(|r| r.what == what).collect();
        let label = match what { "call" => "called by", "emit" => "emitted by", "mock" => "mocked by", w => w };
        for (i, r) in rows.iter().enumerate() {
            println!("{:<10} {:<w$}  {}:{}", if i == 0 { label } else { "" }, r.by, r.file, r.line, w = width);
        }
    }
    if !refs.iter().any(|r| matches!(r.what, "call" | "emit")) {
        println!("called by  nothing under {} — an entry point (a route, the command line), or dead", path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new(".")).display());
    }
}

// ── rename ───────────────────────────────────────────────────────────

/// One file with `cell.old` written `new` wherever it stands: the
/// declaration, the body, calls (`Cell.old(…)` anywhere, `old(…)` inside
/// `Cell` and in tests), frame clauses (`calls old`, `calls Cell.old`),
/// mocks, and the calls inside a string's `{…}`. An EVENT (a signal some
/// `emit` raises) is renamed in every cell that declares or hears it.
/// Returns the new text and the lines changed.
fn rename_in(text: &str, cell: &str, old: &str, new: &str, event: bool, names: &Names) -> Option<(String, Vec<usize>)> {
    use crate::lexer::Token;
    let tokens = crate::lexer::Lexer::new(text).tokenize().ok()?;
    crate::parser::Parser::new(tokens.clone()).parse_program().ok()?;
    // the cells of the file, by their braces: (start, end, name, is a test)
    // — `cell Name {`, `skeleton Name {`, `flesh Name {`, `cell test Name {`
    let mut cells: Vec<(usize, usize, String, bool)> = Vec::new();
    {
        let mut open: Vec<(usize, Option<usize>)> = Vec::new();   // (brace depth marker, index into `cells`)
        let mut pending: Option<(String, bool, usize)> = None;
        let mut heading = false;
        let mut strict = false;
        let mut test = false;
        let mut start = 0;
        for t in tokens.iter() {
            match &t.token {
                // `cell` is a keyword: what follows, up to the brace, is its heading
                // (`cell agent Bot`, `cell test T`, `cell worker`)
                Token::Cell => { heading = true; strict = false; test = false; pending = None; start = t.span.start; }
                Token::Ident(w) if !heading && (w == "skeleton" || w == "flesh") => { heading = true; strict = true; test = false; pending = None; start = t.span.start; }
                Token::Test if heading => test = true,
                Token::TypeIdent(n) | Token::Ident(n) if heading => pending = Some((n.clone(), test, start)),
                Token::LBrace => {
                    match pending.take() {
                        Some((name, is_test, at)) if heading => { cells.push((at, usize::MAX, name, is_test)); open.push((0, Some(cells.len() - 1))); }
                        _ => open.push((0, None)),
                    }
                    heading = false;
                }
                Token::RBrace => {
                    if let Some((_, Some(k))) = open.pop() { cells[k].1 = t.span.end; }
                    heading = false; pending = None;
                }
                // (`let flesh = 1`: a word, not a heading)
                _ => { if strict || !heading { heading = false; pending = None; } }
            }
        }
    }
    let home = |pos: usize| -> (bool, bool) {
        // innermost cell holding the position: (is `cell`, is a test)
        cells.iter().filter(|(s, e, _, _)| *s <= pos && pos < *e).min_by_key(|(s, e, _, _)| e - s)
            .map_or((false, false), |(_, _, n, t)| (n == cell, *t))
    };
    let chars: Vec<char> = text.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    // (start, end, replacement) over the characters
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        let (here, test) = home(t.span.start);
        match &t.token {
            Token::Ident(w) if w == old => {
                let prev = i.checked_sub(1).map(|j| &tokens[j].token);
                let prev2 = i.checked_sub(2).map(|j| &tokens[j].token);
                let next = tokens.get(i + 1).map(|n| &n.token);
                // which clause of a frame is this name in, if any?
                let clause = || -> Option<&str> {
                    let mut j = i;
                    while j > 0 {
                        j -= 1;
                        match &tokens[j].token {
                            // (the clause word of a face: it follows a signature, a type or
                            // another clause — not `[calls, tick]`, where `calls` is a local)
                            Token::Ident(k) if k == "calls" || k == "emits" => {
                                let after_decl = j > 0 && matches!(&tokens[j - 1].token, Token::RParen | Token::Ident(_) | Token::TypeIdent(_) | Token::Gt | Token::IntLit(_) | Token::StringLit(_));
                                return if after_decl { Some(if k == "calls" { "calls" } else { "emits" }) } else { None };
                            }
                            // (another clause: `calls bump  reads total` — `total` is a slot)
                            Token::Ident(k) if matches!(k.as_str(), "reads" | "writes" | "moves" | "requires" | "ensures") => return None,
                            Token::Ident(_) | Token::TypeIdent(_) | Token::Dot | Token::Comma => continue,
                            _ => return None,
                        }
                    }
                    None
                };
                // a bare name reaches `cell.old` from the cell, from a test, and
                // from any cell when no other cell has a handler of that name
                let bare = here || test || names.unique;
                let call = matches!(next, Some(Token::LParen));
                let yes = match (prev2, prev) {
                    // `Cell.old`; for an event, `Listener.old` of every cell that hears it
                    (Some(Token::Ident(c) | Token::TypeIdent(c)), Some(Token::Dot)) if c == cell || (event && names.definers.contains(c)) => true,
                    (Some(Token::Ident(c) | Token::TypeIdent(c)), Some(Token::Dot)) if names.cells.contains(c) => false,
                    // `x.old(…)`: the handler, by its first argument
                    (_, Some(Token::Dot)) => call && bare,
                    (_, Some(Token::Signal)) | (_, Some(Token::On)) => here || event,
                    (_, Some(Token::Ident(k))) if k == "tool" => here,
                    (_, Some(Token::Emit)) => event,
                    (_, Some(Token::Ident(k))) if k == "mock" => test && names.unique,
                    _ if call => bare || event,
                    _ => match clause() { Some("calls") => here, Some("emits") => event, _ => false },
                };
                if yes { edits.push((t.span.start, t.span.end, new.to_string())); }
            }
            // `vote("Cell.old", …)`, `horde("Cell.old", …)`, a horde callback `"Cell.old"`: the handler, in a string
            Token::StringLit(v) if *v == format!("{}.{}", cell, old) && (1..=12).any(|b| i >= b && matches!(&tokens[i - b].token, Token::Ident(k) if k == "vote" || k == "horde")) => {
                let (s, e) = (t.span.start, t.span.end.min(chars.len()));
                let at = s + 1 + cell.chars().count() + 1;
                if e >= at + old.chars().count() && chars[at..at + old.chars().count()].iter().collect::<String>() == old { edits.push((at, at + old.chars().count(), new.to_string())); }
            }
            // `vote("old", …)`, `horde(…, map("on_result", "old"))` in the cell: its handler, by a bare name
            Token::StringLit(v) if v == old && here && (1..=24).any(|b| i >= b && matches!(&tokens[i - b].token, Token::Ident(k) if k == "vote" || k == "horde")) => {
                let (s, e) = (t.span.start, t.span.end.min(chars.len()));
                if e >= s + 2 && chars[s + 1..e - 1].iter().collect::<String>() == old { edits.push((s + 1, e - 1, new.to_string())); }
            }
            // `map("tools_allowed", ["old"])` in the cell: a tool, named
            Token::StringLit(v) if v == old && here && (1..=8).any(|b| i >= b && matches!(&tokens[i - b].token, Token::StringLit(k) if k == "tools_allowed")) => {
                let (s, e) = (t.span.start, t.span.end.min(chars.len()));
                if e >= s + 2 && chars[s + 1..e - 1].iter().collect::<String>() == old { edits.push((s + 1, e - 1, new.to_string())); }
            }
            // `delegate("Cell", "old", …)`: the handler's name, as a string
            Token::StringLit(v) if v == old && i >= 4 && matches!(&tokens[i - 1].token, Token::Comma)
                && matches!(&tokens[i - 2].token, Token::StringLit(c) if c == cell) && matches!(&tokens[i - 3].token, Token::LParen)
                && matches!(&tokens[i - 4].token, Token::Ident(d) if d == "delegate") => {
                let (s, e) = (t.span.start, t.span.end.min(chars.len()));
                if e >= s + 2 && chars[s + 1..e - 1].iter().collect::<String>() == old { edits.push((s + 1, e - 1, new.to_string())); }
            }
            // the calls a string makes in its `{…}` — never its text
            Token::StringLit(_) => {
                let (s, e) = (t.span.start, t.span.end.min(chars.len()));
                let raw: Vec<char> = chars[s..e].to_vec();
                let qualified: Vec<char> = format!("{}.{}(", cell, old).chars().collect();
                let bare_call: Vec<char> = format!("{}(", old).chars().collect();
                let bare = here || test || names.unique;
                let mut depth = 0usize;
                let mut k = 0;
                while k < raw.len() {
                    if raw[k] == '{' { depth += 1; k += 1; continue; }
                    if raw[k] == '}' { depth = depth.saturating_sub(1); k += 1; continue; }
                    if depth == 0 { k += 1; continue; }
                    let boundary = k == 0 || !is_word(raw[k - 1]);
                    if boundary && raw[k..].starts_with(&qualified) {
                        let at = s + k + cell.chars().count() + 1;
                        edits.push((at, at + old.chars().count(), new.to_string()));
                        k += qualified.len();
                    } else if bare && raw[k..].starts_with(&bare_call) && boundary
                        && (raw[k - 1] != '.' || { let mut b = k - 1; while b > 0 && is_word(raw[b - 1]) { b -= 1; } !names.cells.contains(&raw[b..k - 1].iter().collect::<String>()) }) {
                        edits.push((s + k, s + k + old.chars().count(), new.to_string()));
                        k += bare_call.len();
                    } else { k += 1; }
                }
            }
            _ => {}
        }
    }
    if edits.is_empty() { return None; }
    edits.sort();
    edits.dedup();
    let mut out = String::new();
    let mut lines = Vec::new();
    let mut at = 0;
    for (s, e, rep) in &edits {
        if *s < at { continue; }
        out.extend(chars[at..*s].iter());
        out.push_str(rep);
        lines.push(chars[..*s].iter().filter(|c| **c == '\n').count() + 1);
        at = *e;
    }
    out.extend(chars[at..].iter());
    lines.dedup();
    Some((out, lines))
}

pub fn cmd_rename(path: &PathBuf, target: &str, new: &str, dry_run: bool) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    let skeletons = crate::checker::frame::build(&program);
    let (cell, old) = match super::skeleton::locate(&skeletons, target) {
        Ok(found) => found,
        Err(msg) => { eprintln!("error: {}", msg); std::process::exit(1); }
    };
    let fail = |msg: String| -> ! { eprintln!("error: {}", msg); std::process::exit(1) };
    if new.is_empty() || !new.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || new.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        fail(format!("`{}` is not a name (letters, digits, `_`)", new));
    }
    // a word of the language (`if`, `on`, `let`, `emit`…) is no name
    let lexed = crate::lexer::Lexer::new(new).tokenize().ok().map(|t| t.into_iter().map(|x| x.token).filter(|t| !matches!(t, crate::lexer::Token::Eof)).collect::<Vec<_>>());
    if !matches!(lexed.as_deref(), Some([crate::lexer::Token::Ident(_)])) || matches!(new, "skeleton" | "flesh" | "reads" | "writes" | "moves" | "calls" | "emits" | "tool" | "mock") {
        fail(format!("`{}` is a word of the language: a signal cannot bear it", new));
    }
    if crate::checker::names::builtin_names().contains(new) { fail(format!("`{}` is a builtin: a signal of that name would hide it", new)); }
    let owner = skeletons.iter().find(|sk| sk.cell.name == cell);
    let taken = owner.is_some_and(|sk| sk.bones.iter().any(|b| b.name == new) || sk.helpers.iter().any(|h| h == new));
    if taken { fail(format!("`{}` already has a signal or a helper `{}`", cell, new)); }
    let refs = collect(path, &cell, &old);
    let event = refs.iter().any(|r| r.what == "emit");
    if event {
        // every cell that hears the event is renamed with it: none may hold the new name already
        if let Some(sk) = skeletons.iter().find(|sk| sk.bones.iter().any(|b| b.name == old) && sk.bones.iter().any(|b| b.name == new)) {
            fail(format!("`{}` is an event (an `emit` raises it), heard by `{}`, which already has a signal `{}`", old, sk.cell.name, new));
        }
    }
    // the program and its tests: not another program lying in the directory
    let (dir, files, names) = survey(path, &cell, &old);
    let dir = dir.as_path();
    // the new name must be new to the whole program: a handler, a local or a
    // lambda that already bears it would hide the signal, or be taken for it
    for (f, text) in &files {
        let Ok(tokens) = crate::lexer::Lexer::new(text).tokenize() else { continue };
        if let Some(t) = tokens.iter().find(|t| matches!(&t.token, crate::lexer::Token::Ident(w) if w == new)) {
            let line = crate::interpreter::span_to_location(text, t.span.start).0;
            fail(format!("`{}` is already a name in the program ({}:{}) — a signal renamed to it could be hidden by it, or taken for it: pick a name the program does not use",
                new, f.strip_prefix(dir).unwrap_or(f).display(), line));
        }
    }
    let mut total = 0;
    for (f, text) in files {
        let Some((changed, lines)) = rename_in(&text, &cell, &old, new, event, &names) else { continue };
        let shown = f.strip_prefix(dir).unwrap_or(&f).display().to_string();
        for l in &lines { println!("{}:{}", shown, l); }
        total += lines.len();
        if !dry_run {
            if let Err(e) = std::fs::write(&f, changed) { fail(format!("cannot write {}: {}", f.display(), e)); }
        }
    }
    println!("{}.{} -> {}.{}: {} line(s){}{}", cell, old, cell, new, total,
        if event { " — an event: renamed in every cell that declares or hears it" } else { "" },
        if dry_run { " (dry run: nothing written)" } else { "" });
    // what a rename does not read
    if std::fs::read_to_string(dir.join("soma.toml")).is_ok_and(|t| t.contains(&format!("\"{}\"", old))) {
        println!("note: soma.toml names \"{}\" (a handler list?) — it is not rewritten: check it", old);
    }
    if !dry_run {
        if dir.join(crate::checker::lock::LOCK_FILE).exists() && owner.is_some_and(|sk| sk.cell.skeleton) {
            println!("the skeleton changed: accept it with `soma skeleton {} --amend`, then `soma check {}`", path.display(), dir.display());
        } else {
            println!("now: `soma check {}`", dir.display());
        }
    }
}
