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
    let Ok(content) = fs::read_to_string(&toml_path) else { return };
    if let Err(e) = toml::from_str::<crate::pkg::manifest::Manifest>(&content) {
        eprintln!("error: {} does not parse — its [verify] properties and [agent] settings would be ignored", toml_path.display());
        for line in e.to_string().lines() {
            eprintln!("  {}", line);
        }
        eprintln!("  valid [verify] keys: cells, deadlock_free, eventually, never, always, [verify.after.<state>] with eventually / never, [verify.before.<state>] with requires");
        process::exit(1);
    }
}

pub fn read_source(path: &PathBuf) -> String {
    validate_manifest_beside(path);
    match fs::read_to_string(path) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("error: cannot read '{}': {}", path.display(), e);
            process::exit(1);
        }
    }
}

fn lex_error_position(e: &lexer::LexError) -> Option<usize> {
    match e {
        lexer::LexError::UnexpectedChar { pos, .. } => Some(*pos),
        lexer::LexError::UnterminatedString { pos } => Some(*pos),
        lexer::LexError::UnterminatedComment { pos } => Some(*pos),
        lexer::LexError::InvalidNumber { pos } => Some(*pos),
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
            process::exit(1);
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
                    if let Some(h) = foreign_syntax_hint(&e.to_string(), src, span.start) {
                        eprintln!("{}", h);
                    }
                }
                _ => {
                    eprintln!("error: {}", e);
                }
            }
            process::exit(1);
        }
    }
}

pub fn resolve_imports(program: &mut ast::Program, base_path: &PathBuf) {
    let base_dir = base_path.parent().unwrap_or(Path::new("."));

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
                    process::exit(1);
                })
        } else if import_path.starts_with("lib:") {
            let mod_name = &import_path[4..];
            let as_file = base_dir.join("lib").join(format!("{}.cell", mod_name));
            let as_dir = base_dir.join("lib").join(mod_name);
            if as_file.exists() { as_file } else { as_dir }
        } else {
            let with_ext = if !import_path.ends_with(".cell") {
                format!("{}.cell", import_path)
            } else {
                import_path.clone()
            };
            let as_path = base_dir.join(&with_ext);
            let as_dir = base_dir.join(import_path);
            if as_path.exists() { as_path } else { as_dir }
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
    for c in &candidates {
        if c.exists() { return c.clone(); }
    }
    eprintln!("error: package '{}' not installed (run `soma install`)", pkg_name);
    process::exit(1);
}

fn import_file(program: &mut ast::Program, path: &PathBuf) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot import '{}': {}", path.display(), e);
            process::exit(1);
        }
    };
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut imported = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut imported, path);
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
