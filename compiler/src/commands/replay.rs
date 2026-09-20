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

    let (entries, bad_lines) = match record_log::read_all_counting(&log_path) {
        Ok(e) => e,
        Err(e) => { eprintln!("error: failed to read {}: {}", log_path.display(), e); process::exit(1); }
    };
    // a replay that checked nothing, or skipped lines, is not a pass
    if bad_lines > 0 {
        eprintln!("error: {}: {} line(s) are not log entries (truncated or edited) — the replay cannot vouch for them", log_path.display(), bad_lines);
    }
    if entries.is_empty() {
        eprintln!("error: {} contains no entries — nothing was replayed", log_path.display());
        process::exit(1);
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
    // the [agent] of soma.toml, as for run/serve (replay sent the prompts
    // and the key to api.openai.com instead of the configured provider)
    {
        let soma_toml = file.parent().unwrap_or(std::path::Path::new(".")).join("soma.toml");
        if let Ok(content) = std::fs::read_to_string(&soma_toml) {
            if let Ok(manifest) = toml::from_str::<crate::pkg::manifest::Manifest>(&content) {
                interp.agent_config = Some(manifest.agent);
                interp.agent_models = manifest.models;
            }
        }
    }
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
    interp.native_handlers = interpreter::native_ffi::compile_and_load_natives_with_config(&program, &parallel_config)
        .unwrap_or_else(|e| {
            eprintln!("error: cannot replay: native compilation failed: {}", e);
            process::exit(1);
        });

    let mut diverged = 0usize;
    let mut ok = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        match interp.call_signal(&entry.cell, &entry.handler, entry.args.clone()) {
            Ok(live) => {
                if entry.error_kind.is_none() && values_equivalent(&live, &entry.result) {
                    println!("  #{:<4}  {}.{}({})  ok", i + 1, entry.cell, entry.handler, fmt_args(&entry.args));
                    ok += 1;
                } else {
                    println!();
                    println!("  divergence at entry #{}: {}.{}", i + 1, entry.cell, entry.handler);
                    println!("      args:     {}", fmt_args(&entry.args));
                    if let Some(kind) = &entry.error_kind { println!("      recorded: raised {}", kind); }
                    else { println!("      recorded: {}", entry.result); }
                    println!("      replayed: {}", live);
                    let source_changed = entry
                        .src
                        .as_ref()
                        .is_some_and(|recorded| *recorded != record_log::source_fingerprint(&source));
                    if source_changed {
                        println!("      cause:    the source changed since this entry was recorded");
                        println!("      fix:      replay against the recorded version, or re-record with `soma run --record`");
                        if !entry.nondet.is_empty() {
                            println!("      note:     the handler also calls nondeterministic builtins: {}",
                                entry.nondet.join(", "));
                        }
                    } else if !entry.nondet.is_empty() {
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
            // the recorded call raised the same kind: as recorded
            Err(e) if entry.error_kind.as_deref() == Some(e.kind().as_str()) => {
                println!("  #{:<4}  {}.{}({})  ok (raised {} as recorded)", i + 1, entry.cell, entry.handler, fmt_args(&entry.args), e.kind());
                ok += 1;
            }
            Err(e) => {
                println!("  #{:<4}  {}.{}  ERROR: {}", i + 1, entry.cell, entry.handler, e);
                diverged += 1;
            }
        }
    }

    println!("--------------------------------------------------------------");
    println!("replayed {} entries: {} ok, {} diverged", entries.len(), ok, diverged);
    if bad_lines > 0 {
        println!("  and {} unreadable line(s)", bad_lines);
        process::exit(1);
    }
    if diverged > 0 {
        process::exit(1);
    }
}

/// `--at`: epoch milliseconds, or ISO 8601 `YYYY-MM-DD[THH:MM[:SS]][Z]` (UTC).
/// Anything else is an error (it silently meant "replay everything").
fn parse_at(s: &str) -> i64 {
    if let Ok(n) = s.parse::<i64>() { return n; }
    let t = s.trim().trim_end_matches('Z');
    let (date, time) = t.split_once(['T', ' ']).unwrap_or((t, "00:00:00"));
    if let Some((y, m, d)) = crate::interpreter::builtins::time::parse_iso_date(date) {
        let parts: Vec<&str> = time.split(':').collect();
        let num = |i: usize| parts.get(i).map(|p| p.parse::<i64>()).unwrap_or(Ok(0));
        if let (Ok(h), Ok(mi), Ok(se)) = (num(0), num(1), num(2)) {
            if (0..24).contains(&h) && (0..60).contains(&mi) && (0..61).contains(&se) && parts.len() <= 3 {
                let days = crate::interpreter::builtins::time::days_from_civil(y, m, d);
                return ((days * 86400) + h * 3600 + mi * 60 + se) * 1000;
            }
        }
    }
    eprintln!("error: --at '{}' is neither epoch milliseconds nor an ISO 8601 time (2026-09-18 or 2026-09-18T12:30:00Z)", s);
    process::exit(1)
}

fn fmt_args(args: &[interpreter::Value]) -> String {
    args.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(", ")
}

fn suggest_fix(nondet: &[String]) -> String {
    if nondet.iter().any(|s| s == "now" || s == "now_ms" || s == "timestamp") {
        return "pass the clock in as a handler argument (now()/now_ms() are read again on replay; [pure] does not freeze them)".to_string();
    }
    if nondet.iter().any(|s| s == "random" || s == "rand") {
        return "pass random draws in as handler arguments (there is no seed builtin)".to_string();
    }
    if nondet.iter().any(|s| s == "today" || s == "date_now") {
        return "pass the date as an input parameter so replay sees the same value".to_string();
    }
    "remove the nondeterministic call from the handler body".to_string()
}
