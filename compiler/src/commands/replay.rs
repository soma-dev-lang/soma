//! V1: `soma replay` — deterministic time-travel replay.
//!
//! Reads a `.somalog` file produced by a previous run of a [record]
//! handler and re-executes each entry against a fresh interpreter.
//! For each entry we compare the live result against the recorded one;
//! if they differ, we print a divergence report naming the
//! nondeterministic builtin(s) the original handler called.

use std::path::PathBuf;
use std::process;

use crate::ast;
use crate::interpreter;
use crate::registry::Registry;
use crate::runtime;
use crate::interpreter::record_log::{self, RecordEntry, values_equivalent};
use super::{read_source, lex_with_location, parse_with_location, resolve_imports, load_meta_cells_from_program};

pub fn cmd_replay(
    file: &PathBuf,
    log: Option<&PathBuf>,
    at: Option<&str>,
    registry: &mut Registry,
) {
    let source = read_source(file);
    let file_str = file.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, file);
    load_meta_cells_from_program(&program, registry, file);

    let log_path: PathBuf = log.cloned().unwrap_or_else(|| record_log::default_log_path(file));
    if !log_path.exists() {
        eprintln!("error: no replay log at {}", log_path.display());
        eprintln!("hint: run `soma run {}` first with a [record] handler.", file.display());
        process::exit(1);
    }

    let entries = match record_log::read_all(&log_path) {
        Ok(e) => e,
        Err(e) => { eprintln!("error: failed to read {}: {}", log_path.display(), e); process::exit(1); }
    };
    if entries.is_empty() {
        eprintln!("warning: {} contains no entries", log_path.display());
        return;
    }

    // If --at is provided, slice the entries to those at-or-before that timestamp.
    let entries: Vec<RecordEntry> = if let Some(ts_str) = at {
        let ts = parse_at(ts_str);
        entries.into_iter().filter(|e| e.ts_ms <= ts).collect()
    } else {
        entries
    };

    println!("soma replay: {} entries from {}", entries.len(), log_path.display());
    println!("--------------------------------------------------------------");

    let mut interp = interpreter::Interpreter::new(&program);
    interp.replay_mode = true;
    interp.source_file = Some(file.display().to_string());
    interp.source_text = Some(source.clone());

    // Storage backends (memory only — replay must be hermetic)
    for prog_cell in &program.cells {
        if !matches!(prog_cell.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) { continue; }
        for section in &prog_cell.node.sections {
            if let ast::Section::Memory(ref mem) = section.node {
                let mut slots = std::collections::HashMap::new();
                for slot in &mem.slots {
                    let backend: std::sync::Arc<dyn runtime::storage::StorageBackend> =
                        std::sync::Arc::new(runtime::storage::MemoryBackend::new());
                    slots.insert(slot.node.name.clone(), backend);
                }
                interp.set_storage(&prog_cell.node.name, &slots);
            }
        }
    }
    interp.ensure_state_machine_storage();

    // Compile [native] handlers (so [native] cells can replay too)
    let parallel_config = crate::codegen::native::ParallelConfig::default();
    if let Ok(natives) = interpreter::native_ffi::compile_and_load_natives_with_config(&program, &parallel_config) {
        interp.native_handlers = natives;
    }

    let mut diverged = 0usize;
    let mut ok = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        match interp.call_signal(&entry.cell, &entry.handler, entry.args.clone()) {
            Ok(live) => {
                if values_equivalent(&live, &entry.result) {
                    println!("  #{:<4}  {}.{}({})  ok", i + 1, entry.cell, entry.handler, fmt_args(&entry.args));
                    ok += 1;
                } else {
                    println!();
                    println!("  divergence at entry #{}: {}.{}", i + 1, entry.cell, entry.handler);
                    println!("      args:     {}", fmt_args(&entry.args));
                    println!("      recorded: {}", entry.result);
                    println!("      replayed: {}", live);
                    if !entry.nondet.is_empty() {
                        println!("      cause:    nondeterminism in handler — calls to {}",
                            entry.nondet.join(", "));
                        let suggestion = suggest_fix(&entry.nondet);
                        println!("      fix:      {}", suggestion);
                    } else {
                        println!("      cause:    unknown (no nondet builtins recorded — check storage or external state)");
                    }
                    println!();
                    diverged += 1;
                }
            }
            Err(e) => {
                println!("  #{:<4}  {}.{}  ERROR: {}", i + 1, entry.cell, entry.handler, e);
                diverged += 1;
            }
        }
    }

    println!("--------------------------------------------------------------");
    println!("replayed {} entries: {} ok, {} diverged", entries.len(), ok, diverged);
    if diverged > 0 {
        process::exit(1);
    }
}

fn parse_at(s: &str) -> i64 {
    if let Ok(n) = s.parse::<i64>() { return n; }
    // ISO 8601: very simple parser, accept yyyy-mm-ddThh:mm:ssZ
    // For V1 we just accept epoch ms or fall through to "all".
    i64::MAX
}

fn fmt_args(args: &[interpreter::Value]) -> String {
    args.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(", ")
}

fn suggest_fix(nondet: &[String]) -> String {
    if nondet.iter().any(|s| s == "now" || s == "now_ms" || s == "timestamp") {
        return "mark the handler [pure] or pass the clock as an explicit input parameter".to_string();
    }
    if nondet.iter().any(|s| s == "random" || s == "rand") {
        return "seed the PRNG explicitly or pass random draws in via the handler args".to_string();
    }
    if nondet.iter().any(|s| s == "today" || s == "date_now") {
        return "pass the date as an input parameter so replay sees the same value".to_string();
    }
    "remove the nondeterministic call from the handler body".to_string()
}
