use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use crate::checker;
use crate::codegen;
use crate::registry::Registry;
use super::{read_source, lex, parse, resolve_imports, load_meta_cells_from_program};

pub fn cmd_build(path: &PathBuf, output: Option<&Path>, registry: &mut Registry) {
    let source = read_source(path);
    let tokens = lex(&source);
    let mut program = parse(tokens);
    resolve_imports(&mut program, path);

    load_meta_cells_from_program(&program, registry, path);

    let mut chk = checker::Checker::new(registry);
    chk.check(&program);

    for w in &chk.warnings {
        eprintln!("{}", w);
    }

    if chk.has_errors() {
        eprint!("{}", chk.report());
        process::exit(1);
    }

    eprintln!("note: codegen is experimental and generates skeleton code only — runtime behavior requires `soma run` or `soma serve`");

    let mut gen = codegen::CodeGen::new();
    let rust_code = gen.generate(&program);

    match output {
        Some(out_path) => {
            fs::write(out_path, &rust_code).unwrap_or_else(|e| {
                eprintln!("error: cannot write '{}': {}", out_path.display(), e);
                process::exit(1);
            });
            eprintln!("generated {}", out_path.display());
        }
        None => {
            print!("{}", rust_code);
        }
    }
}
