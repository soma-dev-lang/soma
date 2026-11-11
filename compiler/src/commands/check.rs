use std::path::PathBuf;
use std::process;

use crate::checker;
use crate::registry::Registry;
use super::{read_source, lex_with_location, parse_collecting, resolve_imports, load_meta_cells_from_program};

pub fn cmd_check(path: &PathBuf, json: bool, registry: &mut Registry) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    // every syntax error in one pass; the ones that left the AST complete
    // (`=>` in a match arm, `-> T` on a handler) are reported beside the
    // semantic errors instead of hiding them
    let (mut program, syntax_errors) = parse_collecting(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);

    load_meta_cells_from_program(&program, registry, path);

    // Load the project manifest, if present alongside the cell file.
    // It's used by V1.6 model-capability checks; ignored when absent.
    let manifest = path.parent().and_then(|d| {
        let toml = d.join("soma.toml");
        if toml.exists() {
            crate::pkg::manifest::Manifest::load(&toml).ok()
        } else { None }
    });

    let mut chk = checker::Checker::new(registry);
    chk.manifest = manifest.as_ref();
    chk.source = Some((file_str.clone(), source.clone()));
    chk.check(&program);
    for e in syntax_errors.iter().rev() {
        chk.errors.insert(0, checker::CheckError::Static {
            kind: "syntax",
            message: e.to_string(),
            span: e.span().unwrap_or(crate::ast::Span { start: 0, end: 0 }),
        });
    }

    if json {
        println!("{}", chk.report_json());
    } else {
        print!("{}", chk.report());
    }

    if chk.has_errors() {
        process::exit(1);
    }
}
