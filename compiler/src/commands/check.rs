use std::path::PathBuf;
use std::process;

use crate::checker;
use crate::registry::Registry;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports, load_meta_cells_from_program};

pub fn cmd_check(path: &PathBuf, registry: &mut Registry) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);

    load_meta_cells_from_program(&program, registry, path);

    let mut chk = checker::Checker::new(registry);
    chk.check(&program);

    print!("{}", chk.report());

    if chk.has_errors() {
        process::exit(1);
    }
}
