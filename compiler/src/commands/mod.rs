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
pub mod deploy;
pub mod lint;

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

    for candidate in &candidates {
        if candidate.exists() {
            return candidate.clone();
        }
    }

    PathBuf::from("stdlib")
}

pub fn read_source(path: &PathBuf) -> String {
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
            match (&e, source) {
                (parser::ParseError::Expected { span, .. }, Some(src)) => {
                    let (line, col) = crate::interpreter::span_to_location(src, span.start);
                    let location = if let Some(f) = file {
                        format!("  --> {}:{}:{}", f, line, col)
                    } else {
                        format!("  --> {}:{}", line, col)
                    };
                    let context = crate::interpreter::format_error_context(src, span.start);
                    eprintln!("error: {}\n{}\n{}", e, location, context);
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
            ast::CellKind::Cell => {}
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
