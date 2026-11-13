//! Where is the project of an entry file? — ONE answer, for the loader's
//! manifest, the checker, the lock, `verify`, `test`, `run` and the tools.
//!
//! It was four: the loader looked imports up beside the importer, at the
//! entry's folder and at the nearest folder above holding a soma.toml; the
//! manifest was read beside the entry only; the lock was looked for in every
//! folder up to the root; the tools took the nearest soma.toml above. A test
//! file in `tests/unit/` that imports the program ran in the default edition
//! and without the project's `[verify]` tables, while its imports came from
//! the project.
//!
//! The rule: the entry's folder, when it holds a soma.toml; else the nearest
//! folder above that holds one AND that the entry's imports are found in —
//! the folder the loader resolved them in; else the entry's folder.

use std::path::{Path, PathBuf};

fn beside(entry: &Path) -> PathBuf {
    entry.parent().filter(|d| !d.as_os_str().is_empty()).map(|d| d.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

/// The project folder of `entry`.
pub fn root(entry: &Path) -> PathBuf {
    let here = beside(entry);
    if here.join("soma.toml").is_file() { return here; }
    let start = std::fs::canonicalize(&here).unwrap_or_else(|_| here.clone());
    let Some(above) = start.ancestors().skip(1).take(6).find(|d| d.join("soma.toml").is_file()).map(|d| d.to_path_buf()) else { return here };
    // the entry belongs to it when one of its imports is found there and not beside it
    let Ok(text) = std::fs::read_to_string(entry) else { return here };
    let Ok(tokens) = crate::lexer::Lexer::new(&text).tokenize() else { return here };
    let Ok(program) = crate::parser::Parser::new(tokens).parse_program() else { return here };
    // …that is: an import resolves to a file outside the entry's folder and
    // within the folder above (`use "../../app.cell"`, `use lib::x` found at
    // the project root)
    let canon = |p: PathBuf| std::fs::canonicalize(&p).ok();
    let found_above = program.imports.iter().any(|u| {
        if u.starts_with("std:") { return false; }
        let rel = match (u.strip_prefix("lib:"), u.strip_prefix("pkg:")) { (Some(r), _) => format!("lib/{}", r), (_, Some(r)) => r.to_string(), _ => u.clone() };
        let file = if rel.ends_with(".cell") { rel.clone() } else { format!("{}.cell", rel) };
        let bare = rel.strip_suffix(".cell").unwrap_or(&rel).to_string();
        [start.join(&file), start.join(&bare), above.join(&file), above.join(&bare)].into_iter().filter_map(canon)
            .any(|f| f.starts_with(&above) && !f.starts_with(&start))
    });
    if found_above { above } else { here }
}

/// The manifest of `entry`'s project (it may not exist).
pub fn manifest(entry: &Path) -> PathBuf {
    root(entry).join("soma.toml")
}
