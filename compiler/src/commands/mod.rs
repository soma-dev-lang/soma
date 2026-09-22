pub mod run;
pub mod serve;
pub mod check;
pub mod fix;
pub mod test_cmd;
pub mod build;
pub mod init;
pub mod props;
pub mod repl;
pub mod provider;
pub mod describe;
pub mod docs;
pub mod example;
pub mod deploy;
pub mod lint;
pub mod replay;
pub mod dashboard;

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use crate::ast;
use crate::lexer;
use crate::parser;
use crate::registry::{self, Registry};

// ── Shared utility functions ─────────────────────────────────────────

pub fn find_stdlib() -> PathBuf {
    let mut candidates = vec![
        PathBuf::from("stdlib"),
        PathBuf::from("../stdlib"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("../stdlib"));
            candidates.push(parent.join("stdlib"));
        }
    }
    candidates.push(PathBuf::from(".soma_env/stdlib"));
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".soma/stdlib"));
    }

    // An EMPTY stdlib directory must not win: `.soma_env/stdlib` created by
    // an older `soma init` hid ~/.soma/stdlib and made `persistent` unknown.
    for candidate in &candidates {
        if crate::pkg::env::dir_has_cells(candidate) {
            return candidate.clone();
        }
    }

    // Nothing on disk: materialize the copy embedded in the binary.
    if let Some(home) = std::env::var_os("HOME") {
        let dir = PathBuf::from(home).join(".soma/stdlib");
        if crate::pkg::env::write_embedded_stdlib(&dir).is_ok() {
            return dir;
        }
    }
    let dir = std::env::temp_dir().join("soma-stdlib");
    let _ = crate::pkg::env::write_embedded_stdlib(&dir);
    dir
}

/// A soma.toml beside the program that does not parse is a hard error.
/// Every consumer used to swallow the failure (`.ok()`), so a manifest with
/// a typo — or, before 2.5, just without `[package]` — silently lost its
/// `[verify]` properties and `[agent]` settings while `soma verify` went on
/// printing "passed".
fn validate_manifest_beside(path: &PathBuf) {
    let toml_path = path.parent().unwrap_or(std::path::Path::new(".")).join("soma.toml");
    // present but unreadable (not UTF-8, a directory, a dangling link):
    // its [verify] properties were dropped and verify said OK — fail closed
    if fs::symlink_metadata(&toml_path).is_err() { return; }
    let content = match fs::read_to_string(&toml_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {} cannot be read ({}) — its [verify] properties and [agent] settings would be ignored; save it as UTF-8 text", toml_path.display(), e);
            fatal_exit();
        }
    };
    if let Ok(m) = toml::from_str::<crate::pkg::manifest::Manifest>(&content) {
        if !m.peers.is_empty() { HAS_PEERS.store(true, std::sync::atomic::Ordering::Relaxed); }
    }
    if let Err(e) = toml::from_str::<crate::pkg::manifest::Manifest>(&content) {
        eprintln!("error: {} does not parse — its [verify] properties and [agent] settings would be ignored", toml_path.display());
        for line in e.to_string().lines() {
            eprintln!("  {}", line);
        }
        // the [verify] key list is for a [verify] error: printed under a
        // `[cluster]` or `[package]` typo it only pointed the wrong way
        let msg = e.to_string();
        // an untagged enum reports only that no variant matched, so the
        // mistyped key is never named: say what a dependency may hold
        if msg.contains("untagged enum Dependency") {
            eprintln!("  a dependency is a version string (`lib = \"1.0\"`) or a table with git, path, version, branch or subdir");
        }
        if msg.contains("verify") || msg.contains("deadlock_free") || msg.contains("requires_all") {
            eprintln!("  valid [verify] keys: cells, deadlock_free, eventually, never, always, [verify.after.<state>] with eventually / never, [verify.before.<state>] with requires / requires_all");
        }
        fatal_exit();
    }
}

/// Set by main when `--json` was asked: fatal errors then also go to
/// stdout as one JSON object, so a machine reader never sees an empty
/// stdout with exit 1.
pub static JSON_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// soma.toml beside the program lists [peers]: an `emit` nobody handles
/// here is meant for another process (not a lost event)
pub static HAS_PEERS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set by `soma verify`: a fatal error before any proof (unreadable file,
/// bad soma.toml, lex/parse/import error) still ends with the one verdict
/// line verify promises.
pub static VERIFY_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The message of the error that stops a program from loading (for the
/// `--json` object: a parse error printed nothing on stdout).
pub static FATAL_MSG: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
static JSON_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_fatal(msg: String) {
    if let Ok(mut m) = FATAL_MSG.lock() { if m.is_none() { *m = Some(msg); } }
}

pub fn fatal_exit() -> ! {
    if VERIFY_MODE.load(std::sync::atomic::Ordering::Relaxed) && !JSON_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        println!("VERIFY FAILED — the program does not load (fix the error above, then verify)");
    }
    if JSON_MODE.load(std::sync::atomic::Ordering::Relaxed) && !JSON_DONE.load(std::sync::atomic::Ordering::Relaxed) {
        let msg = FATAL_MSG.lock().ok().and_then(|m| m.clone()).unwrap_or_else(|| "the program does not load (details on stderr)".to_string());
        println!("{}", serde_json::json!({
            "errors": [{"level": "error", "kind": "load", "message": msg}],
            "warnings": [], "notes": [], "error_count": 1, "warning_count": 0, "passed": false
        }));
    }
    process::exit(1)
}

pub fn read_source(path: &PathBuf) -> String {
    validate_manifest_beside(path);
    match fs::read_to_string(path) {
        // a UTF-8 BOM (some editors on Windows) used to be "unexpected
        // character" at 1:1 — an invisible one
        Ok(source) => source.strip_prefix('\u{feff}').map(|s| s.to_string()).unwrap_or(source),
        Err(e) => {
            eprintln!("error: cannot read '{}': {}", path.display(), e);
            if JSON_MODE.load(std::sync::atomic::Ordering::Relaxed) {
                println!("{}", serde_json::json!({
                    "errors": [{"level": "error", "kind": "io", "message": format!("cannot read '{}': {}", path.display(), e), "fix": "check the path"}],
                    "warnings": [], "notes": [], "error_count": 1, "warning_count": 0, "passed": false
                }));
                JSON_DONE.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            fatal_exit();
        }
    }
}

pub(crate) fn lex_error_position(e: &lexer::LexError) -> Option<usize> {
    match e {
        lexer::LexError::UnexpectedChar { pos, .. } => Some(*pos),
        lexer::LexError::UnterminatedString { pos } => Some(*pos),
        lexer::LexError::UnterminatedComment { pos } => Some(*pos),
        lexer::LexError::InvalidNumber { pos } => Some(*pos),
        lexer::LexError::InvalidEscape { pos, .. } => Some(*pos),
        lexer::LexError::UnknownUnit { pos, .. } => Some(*pos),
    }
}

pub fn lex(source: &str) -> Vec<lexer::SpannedToken> {
    lex_with_location(source, None)
}

/// A hint for syntax carried over from Python / TypeScript / Rust. The
/// parser reports what it expected; this says what Soma writes instead.
/// Matched on the offending source line, so it is cheap and never wrong
/// about the grammar — at worst it stays silent.
fn foreign_syntax_hint(message: &str, source: &str, offset: usize) -> Option<String> {
    let (line_no, col) = crate::interpreter::span_to_location(source, offset);
    let line = source.split('\n').nth(line_no.saturating_sub(1)).unwrap_or("");
    let t = line.trim();
    let at: String = line.chars().skip(col.saturating_sub(1)).collect();
    let at = at.trim_start();
    let has = |needle: &str| t.contains(needle);

    let hint = if message.contains("unexpected character '@'") {
        "annotations go after the parameter list: `on f(n: Int) [native] { … }`"
    } else if message.contains("unexpected character ';'") || (t.ends_with(';') && message.contains("';'")) {
        "Soma has no semicolons — one statement per line"
    } else if t.starts_with("for (") || t.starts_with("for(") {
        "iterate a map with `for k in keys(m) { let v = m[k] }` or `for e in entries(m) { e.key  e.value }`; \
         a list with `for x in xs { }`"
    } else if t.starts_with("def ") || t.starts_with("fn ") || t.starts_with("function ") || t.starts_with("func ") {
        "Soma has no `def` / `fn` / `function` — a function is a handler inside a cell: `on name(x: Int) { return x + 1 }` \
         (its return type goes on `signal name(x: Int) -> Int` in `face { }`)"
    } else if t.starts_with("const ") || t.starts_with("var ") {
        "bindings are `let x = …`; reassign with `x = …`"
    } else if t.starts_with("elif ") || has("} elif ") || has("} elsif ") {
        "write `else if`"
    } else if t.starts_with("cell test {") || t.starts_with("cell test{") {
        "a test cell needs a name and a rules block: `cell test MyTests { rules { assert f(1) == 2 } }`"
    } else if message.contains("expected ']'") && at.starts_with(':') {
        "no slice syntax — `slice(xs, start, end)` (end exclusive; negative indexes count from the end, \
         `slice(xs, -2)` = last two)"
    } else if message.contains("expected ')'") && at.starts_with(',') && has("=>") {
        "a lambda takes exactly ONE parameter: `x => …`. To sort: `sort_by(rows, r => [0 - r.total, r.name])`; \
         to fold: `reduce(xs, 0, p => p.acc + p.val)`"
    } else if message.contains("expected expression") && at.starts_with('{') {
        "Soma has no `{k: v}` literal — a map is `map(\"k\", v)` (empty: `map()`), a record is `Name { k: v }`"
    } else if at.starts_with("=>") && has("match") || (message.contains("'=>'") && has("->") == false && has("match")) {
        "match arms use `->`; `=>` is for lambdas"
    } else if quote_inside_interpolation(t) && message.contains("expected") {
        "an unescaped `\"` inside `{…}` ends the string: escape it (`\"{pad_left(s, 4, \\\"0\\\")}\"`) or bind the value first (`let v = …`, then `\"{v}\"`)"
    } else if message.contains("found 'on'") && (t.contains("->") || t.contains("initial")) {
        "`on` is a keyword (it starts a handler) — name the state differently (`on_shift`, `active`)"
    } else if has("forall") && message.contains("expected ensures") {
        "a property takes ONE variable: encode pairs in it (`forall n: Int in 0..10000 ensures f(idiv(n, 100), n % 100) …`)"
    } else if has("**") {
        "no `**` operator — `pow(a, b)` is the Float power, `ipow(a, b)` the exact Int power"
    } else if has("===") || has("!==") {
        "no `===` — Soma's `==` is already structural (`!=` for not-equal)"
    } else if t.starts_with("throw ") || has(" throw ") {
        "no `throw` — raise with `fail(\"kind\", \"detail\")`; catch with `let r = try { … }` and read `r.kind` / `r.detail`"
    } else if t.starts_with("new ") || has(" new ") || t.starts_with("class ") {
        "no classes or `new` — a `cell` is a singleton service: its fields are `memory` slots, its methods are `on` handlers, a constructor is a `configure(…)` handler writing a `config` slot"
    } else if has(" ? ") && has(" : ") && message.contains("expected") {
        "no ternary — `if` is an expression: `let x = if cond { a } else { b }`"
    } else if (has(" and ") || has(" or ") || t.starts_with("not ") || has(" not ")) && message.contains("expected") {
        "boolean operators are `&&`, `||`, `!`"
    } else {
        return None;
    };
    Some(format!("  hint: {}", hint))
}

pub fn lex_with_location(source: &str, file: Option<&str>) -> Vec<lexer::SpannedToken> {
    let mut lex = lexer::Lexer::new(source);
    match lex.tokenize() {
        Ok(tokens) => tokens,
        Err(e) => {
            if let Some(pos) = lex_error_position(&e) {
                let (line, col) = crate::interpreter::span_to_location(source, pos);
                let location = if let Some(f) = file {
                    format!("  --> {}:{}:{}", f, line, col)
                } else {
                    format!("  --> {}:{}", line, col)
                };
                let context = crate::interpreter::format_error_context(source, pos);
                eprintln!("error: {}\n{}\n{}", e, location, context);
                if let Some(h) = foreign_syntax_hint(&e.to_string(), source, pos) {
                    eprintln!("{}", h);
                }
            } else {
                eprintln!("error: {}", e);
            }
            fatal_exit();
        }
    }
}

pub fn parse(tokens: Vec<lexer::SpannedToken>) -> ast::Program {
    parse_with_location(tokens, None, None)
}

pub fn parse_with_location(tokens: Vec<lexer::SpannedToken>, source: Option<&str>, file: Option<&str>) -> ast::Program {
    let mut p = parser::Parser::new(tokens);
    match p.parse_program() {
        Ok(program) => program,
        Err(e) => {
            match (e.span(), source) {
                (Some(span), Some(src)) => {
                    let (line, col) = crate::interpreter::span_to_location(src, span.start);
                    let location = if let Some(f) = file {
                        format!("  --> {}:{}:{}", f, line, col)
                    } else {
                        format!("  --> {}:{}", line, col)
                    };
                    let context = crate::interpreter::format_error_context(src, span.start);
                    eprintln!("error: {}\n{}\n{}", e, location, context);
                    set_fatal(format!("{} ({})", e, location.trim_start_matches("  --> ")));
                    if let Some(h) = foreign_syntax_hint(&e.to_string(), src, span.start) {
                        eprintln!("{}", h);
                    }
                }
                _ => {
                    eprintln!("error: {}", e);
                    set_fatal(format!("{}", e));
                }
            }
            fatal_exit();
        }
    }
}

thread_local! {
    /// files already brought in by this program's `use` graph: an import
    /// cycle (`a` uses `b` uses `a`, or `use self`) recursed until the stack
    /// overflowed, and a diamond (`b` and `c` both use `d`) defined D twice
    static IMPORTED: std::cell::RefCell<std::collections::HashSet<PathBuf>> = std::cell::RefCell::new(std::collections::HashSet::new());
    static IMPORT_DEPTH: std::cell::Cell<usize> = std::cell::Cell::new(0);
    /// directory of the file that started the `use` graph: `use lib::x`
    /// inside lib/scoring.cell names the PROJECT's lib/x, not lib/lib/x
    static ROOT_DIR: std::cell::RefCell<PathBuf> = std::cell::RefCell::new(PathBuf::from("."));
}

/// Where a `use` path is looked up, in order: beside the importing file, at
/// the project root (the entry file's directory), then at the nearest
/// ancestor holding a soma.toml (a lib file checked on its own).
fn import_roots(base_dir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![base_dir.to_path_buf(), ROOT_DIR.with(|r| r.borrow().clone())];
    // absolute: `.` (or the empty parent of a bare file name) has no parent to walk up to
    let mut dir = canonical(if base_dir.as_os_str().is_empty() { Path::new(".") } else { base_dir });
    for _ in 0..6 {
        if dir.join("soma.toml").exists() { roots.push(dir.clone()); break; }
        match dir.parent() { Some(p) if p != dir => dir = p.to_path_buf(), _ => break }
    }
    let mut seen = std::collections::HashSet::new();
    roots.retain(|r| seen.insert(canonical(r)));
    roots
}

fn canonical(p: &Path) -> PathBuf { fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()) }

pub fn resolve_imports(program: &mut ast::Program, base_path: &PathBuf) {
    let base_dir = base_path.parent().unwrap_or(Path::new("."));
    if IMPORT_DEPTH.with(|d| d.get()) == 0 {
        IMPORTED.with(|s| { let mut s = s.borrow_mut(); s.clear(); s.insert(canonical(base_path)); });
        ROOT_DIR.with(|r| *r.borrow_mut() = base_dir.to_path_buf());
    }

    for import_path in &program.imports.clone() {
        let full_path = if import_path.starts_with("pkg:") {
            let pkg_name = &import_path[4..];
            resolve_pkg_path(base_dir, pkg_name)
        } else if import_path.starts_with("std:") {
            let mod_name = &import_path[4..];
            let candidates = [
                base_dir.join(".soma_env/stdlib").join(format!("{}.cell", mod_name)),
                base_dir.join("stdlib").join(format!("{}.cell", mod_name)),
                PathBuf::from("stdlib").join(format!("{}.cell", mod_name)),
            ];
            candidates.into_iter().find(|p| p.exists())
                .unwrap_or_else(|| {
                    eprintln!("error: stdlib module '{}' not found", mod_name);
                    fatal_exit();
                })
        } else if import_path.starts_with("lib:") {
            let mod_name = &import_path[4..];
            import_roots(base_dir).iter()
                .flat_map(|root| [root.join("lib").join(format!("{}.cell", mod_name)), root.join("lib").join(mod_name)])
                .find(|p| p.exists())
                .unwrap_or_else(|| base_dir.join("lib").join(format!("{}.cell", mod_name)))
        } else {
            let with_ext = if !import_path.ends_with(".cell") {
                format!("{}.cell", import_path)
            } else {
                import_path.clone()
            };
            import_roots(base_dir).iter()
                .flat_map(|root| [root.join(&with_ext), root.join(import_path)])
                .find(|p| p.exists())
                .unwrap_or_else(|| base_dir.join(&with_ext))
        };

        if full_path.is_dir() {
            if let Ok(entries) = fs::read_dir(&full_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map_or(false, |e| e == "cell") {
                        import_file(program, &path);
                    }
                }
            }
        } else {
            import_file(program, &full_path);
        }
    }
}

fn resolve_pkg_path(base_dir: &Path, pkg_name: &str) -> PathBuf {
    let candidates = [
        base_dir.join(".soma_env/packages").join(pkg_name),
        PathBuf::from(".soma_env/packages").join(pkg_name),
        base_dir.join("packages").join(pkg_name),
    ];
    for (i, c) in candidates.iter().enumerate() {
        if c.exists() {
            // an installed package must be what soma.lock pinned (a
            // tampered .soma_env copy ran; the lock's hash was decorative)
            if i < 2 {
                let lock_path = base_dir.join("soma.lock");
                if let Ok(lock) = crate::pkg::lock::LockFile::load(&lock_path) {
                    // the name as the lock knows it: `use MATHX` opened the same
                    // files on a case-insensitive disk and skipped the check
                    let entry = lock.get(pkg_name).or_else(|| lock.packages.values().find(|l| l.name.eq_ignore_ascii_case(pkg_name)));
                    if lock_path.exists() && entry.is_none() {
                        eprintln!("error: package '{}' is installed in {} but not in soma.lock — run `soma install`", pkg_name, c.display());
                        fatal_exit();
                    }
                    if let Some(want) = entry.and_then(|l| l.content_sha256.clone().map(|h| (h, l.files.clone()))) {
                        let got = crate::pkg::resolver::content_sha256(c, &want.1);
                        let mut listed = want.1.clone();
                        listed.sort();
                        let present = crate::pkg::resolver::installed_cell_files(c);
                        if present != listed {
                            eprintln!("error: package '{}' in {} holds files soma.lock does not list ({}) — run `soma install` to restore it",
                                pkg_name, c.display(), present.iter().filter(|f| !listed.contains(f)).cloned().collect::<Vec<_>>().join(", "));
                            fatal_exit();
                        }
                        if got != want.0 {
                            eprintln!("error: package '{}' in {} differs from soma.lock (sha256 {}… recorded, {}… on disk) — it was modified after `soma install`; run `soma install` to restore it, or re-lock on purpose",
                                pkg_name, c.display(), &want.0[..12.min(want.0.len())], &got[..12]);
                            fatal_exit();
                        }
                    }
                }
            }
            return c.clone();
        }
    }
    // `use helper` with helper.cell beside the program: a local file
    let sibling = base_dir.join(format!("{}.cell", pkg_name));
    if sibling.exists() { return sibling; }
    eprintln!("error: `use {}` — no file {}.cell beside the program, no lib/{}.cell (`use lib::{}`), and no installed package '{}' (`soma install`)",
        pkg_name, pkg_name, pkg_name, pkg_name, pkg_name);
    fatal_exit();
}

fn import_file(program: &mut ast::Program, path: &PathBuf) {
    if !IMPORTED.with(|s| s.borrow_mut().insert(canonical(path))) { return; }
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot import '{}': {}", path.display(), e);
            fatal_exit();
        }
    };
    let file_str = path.display().to_string();
    let mut tokens = lex_with_location(&source, Some(&file_str));
    // its spans point into ITS text (see IMPORT_SPAN_BASE)
    let base = crate::interpreter::register_import_source(&file_str, &source);
    for t in tokens.iter_mut() { t.span.start += base; t.span.end += base; }
    let mut imported = parse_with_location(tokens, Some(&source), Some(&file_str));
    IMPORT_DEPTH.with(|d| d.set(d.get() + 1));
    resolve_imports(&mut imported, path);
    IMPORT_DEPTH.with(|d| d.set(d.get() - 1));
    // an imported file's own test cells are ITS tests (`soma test lib/m.cell`):
    // run from the importer they reported the importer's file and lines
    imported.cells.retain(|c| !matches!(c.node.kind, ast::CellKind::Test));
    program.cells.extend(imported.cells);
}

/// Load meta-cells (property, checker, type) from the user's program into the registry.
pub fn load_meta_cells_from_program(program: &ast::Program, registry: &mut Registry, _path: &PathBuf) {
    for cell in &program.cells {
        match cell.node.kind {
            ast::CellKind::Property | ast::CellKind::Checker | ast::CellKind::Type
            | ast::CellKind::Backend | ast::CellKind::Builtin | ast::CellKind::Test => {
                if let Err(e) = register_cell_from_ast(registry, &cell.node) {
                    eprintln!("warning: failed to register {}: {}", cell.node.name, e);
                }
            }
            ast::CellKind::Cell | ast::CellKind::Agent => {}
        }
    }
}

fn register_cell_from_ast(registry: &mut Registry, cell: &ast::CellDef) -> Result<(), String> {
    match cell.kind {
        ast::CellKind::Property => {
            let mut def = registry::PropertyDef {
                name: cell.name.clone(),
                contradicts: std::collections::HashSet::new(),
                implies: std::collections::HashSet::new(),
                requires: std::collections::HashSet::new(),
                mutex_group: None,
                has_params: false,
                promises: Vec::new(),
            };

            for section in &cell.sections {
                match &section.node {
                    ast::Section::Face(face) => {
                        for decl in &face.declarations {
                            match &decl.node {
                                ast::FaceDecl::Promise(p) => {
                                    if let ast::Constraint::Descriptive(s) = &p.constraint.node {
                                        def.promises.push(s.clone());
                                    }
                                }
                                ast::FaceDecl::Given(_) => {
                                    def.has_params = true;
                                }
                                _ => {}
                            }
                        }
                    }
                    ast::Section::Rules(rules) => {
                        for rule in &rules.rules {
                            match &rule.node {
                                ast::Rule::Contradicts(names) => {
                                    def.contradicts.extend(names.iter().cloned());
                                }
                                ast::Rule::Implies(names) => {
                                    def.implies.extend(names.iter().cloned());
                                }
                                ast::Rule::Requires(names) => {
                                    def.requires.extend(names.iter().cloned());
                                }
                                ast::Rule::MutexGroup(group) => {
                                    def.mutex_group = Some(group.clone());
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }

            if let Some(ref group) = def.mutex_group {
                registry
                    .mutex_groups
                    .entry(group.clone())
                    .or_default()
                    .push(def.name.clone());
            }

            let name = def.name.clone();
            let contradicts: Vec<String> = def.contradicts.iter().cloned().collect();
            registry.properties.insert(name.clone(), def);

            for other_name in &contradicts {
                if let Some(other_def) = registry.properties.get_mut(other_name) {
                    other_def.contradicts.insert(name.clone());
                }
            }

            Ok(())
        }
        ast::CellKind::Checker => {
            let mut def = registry::CheckerDef {
                name: cell.name.clone(),
                promises: Vec::new(),
                check_body: Vec::new(),
            };

            for section in &cell.sections {
                match &section.node {
                    ast::Section::Face(face) => {
                        for decl in &face.declarations {
                            if let ast::FaceDecl::Promise(p) = &decl.node {
                                if let ast::Constraint::Descriptive(s) = &p.constraint.node {
                                    def.promises.push(s.clone());
                                }
                            }
                        }
                    }
                    ast::Section::Rules(rules) => {
                        for rule in &rules.rules {
                            if let ast::Rule::Check(body) = &rule.node {
                                def.check_body = body.clone();
                            }
                        }
                    }
                    _ => {}
                }
            }

            registry.checkers.push(def);
            Ok(())
        }
        ast::CellKind::Backend => {
            let mut def = registry::BackendDef {
                name: cell.name.clone(),
                matches: Vec::new(),
                native_impl: None,
                promises: Vec::new(),
            };
            for section in &cell.sections {
                if let ast::Section::Rules(rules) = &section.node {
                    for rule in &rules.rules {
                        match &rule.node {
                            ast::Rule::Matches(props) => def.matches.push(props.clone()),
                            ast::Rule::Native(name) => def.native_impl = Some(name.clone()),
                            _ => {}
                        }
                    }
                }
            }
            registry.backends.push(def);
            Ok(())
        }
        ast::CellKind::Builtin => {
            let mut def = registry::BuiltinDef {
                name: cell.name.clone(),
                native_impl: None,
                promises: Vec::new(),
            };
            for section in &cell.sections {
                if let ast::Section::Rules(rules) = &section.node {
                    for rule in &rules.rules {
                        if let ast::Rule::Native(name) = &rule.node {
                            def.native_impl = Some(name.clone());
                        }
                    }
                }
            }
            registry.builtins.insert(def.name.clone(), def);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// A `{` opened inside a string literal with a `"` before its `}`:
/// `"id-{pad_left(x, 4, "0")}"` — the inner quote ends the string.
fn quote_inside_interpolation(line: &str) -> bool {
    let b: Vec<char> = line.chars().collect();
    let mut in_str = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == '\\' { i += 2; continue; }
            if c == '"' { in_str = false; }
            else if c == '{' {
                if let Some(close) = b[i..].iter().position(|&x| x == '}') {
                    if b[i..i + close].contains(&'"') { return true; }
                }
            }
        } else if c == '"' { in_str = true; }
        i += 1;
    }
    false
}
