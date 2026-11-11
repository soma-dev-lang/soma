//! `soma tools sync | list` — the external tools of a program (3.0, C5).

use std::path::PathBuf;
use std::process;

use super::{read_source, lex_with_location, parse_collecting, resolve_imports};

fn load(path: &PathBuf) -> (crate::ast::Program, PathBuf) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let (mut program, errors) = parse_collecting(tokens, Some(&source), Some(&file_str));
    if !errors.is_empty() {
        eprintln!("{} does not parse — run `soma check {}`", path.display(), path.display());
        process::exit(1);
    }
    resolve_imports(&mut program, path);
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    (program, dir)
}

/// Ask each server for the contract of the tools bound to it; pin them in soma.lock.
pub fn cmd_sync(path: &PathBuf) {
    let (program, dir) = load(path);
    match crate::external::sync(&program, &dir) {
        Ok(lines) => {
            for l in &lines { println!("{}", l); }
            if lines.iter().any(|l| l.starts_with("CHANGED")) {
                println!("a contract changed: `soma check {}` says whether the tool declarations still fit", path.display());
            }
        }
        Err(errors) => {
            for e in errors { eprintln!("error: {}", e); }
            eprintln!("soma.lock was not changed");
            process::exit(1);
        }
    }
}

/// The bound tools, their server and their pin — from the files, no network.
pub fn cmd_list(path: &PathBuf) {
    let (program, dir) = load(path);
    let lock = crate::pkg::lock::LockFile::load(&dir.join("soma.lock")).unwrap_or_default();
    let mut any = false;
    for cell in crate::checker::names::collect_cells(&program) {
        for (_, tool) in crate::external::bound_tools(cell) {
            any = true;
            let b = tool.binding.as_ref().unwrap();
            let key = crate::external::lock_key(&b.server, &b.remote);
            let pin = match lock.tools.get(&key) {
                Some(p) => format!("pinned {}", &p.sha256[..12.min(p.sha256.len())]),
                None => "NOT PINNED (soma tools sync)".to_string(),
            };
            println!("{}.{}  {} {}  {}", cell.name, tool.name, b.kind, key, pin);
        }
    }
    if !any { println!("no external tool: no `tool` of this program is bound with `[mcp: …]` or `[openapi: …]`"); }
}
