use std::path::PathBuf;
use std::process;

use crate::checker;
use crate::registry::Registry;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports, load_meta_cells_from_program};

pub fn cmd_check(path: &PathBuf, json: bool, registry: &mut Registry) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
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
    chk.check(&program);

    if json {
        println!("{}", chk.report_json());
    } else {
        print!("{}", chk.report());
    }

    if chk.has_errors() {
        process::exit(1);
    }
}
