use std::path::PathBuf;
use std::process;

use crate::ast;
use crate::interpreter;
use crate::registry::Registry;
use crate::runtime;
use crate::vm;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports, load_meta_cells_from_program};

/// `soma run --fresh`: the reset, run once the program passed its check
pub static FRESH_HOOK: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>> = std::sync::Mutex::new(None);

pub fn cmd_run(path: &PathBuf, args: &[String], use_jit: bool, signal_flag: Option<&str>, record_all: bool, registry: &mut Registry) {
    if use_jit {
        eprintln!("warning: --jit is deprecated and ignored. Mark hot numeric handlers [native] instead (see `soma docs agent`, Performance).");
        eprintln!("  on simulate(n: Int) [native] {{ ... }}");
        eprintln!("  See: https://soma-lang.dev#native");
        eprintln!();
    }
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);

    load_meta_cells_from_program(&program, registry, path);

    // a program that fails `soma check` does not run (operations.md): two
    // state machines in one cell, an undefined name… used to run anyway
    {
        let mut chk = crate::checker::Checker::new(registry);
        chk.source = Some((file_str.clone(), source.clone()));
        chk.check(&program);
        if chk.has_errors() {
            eprintln!("{} fails `soma check` — fix these before running it:", path.display());
            let mut in_error = false;
            for l in chk.report().lines() {
                if l.starts_with("error") { in_error = true; }
                else if l.starts_with("warning") || l.starts_with("advisory") || l.starts_with("✓") || l.starts_with("✗") || l.starts_with("note") { in_error = false; }
                if in_error { eprintln!("  {}", l); }
            }
            std::process::exit(1);
        }
    }
    if let Some(hook) = FRESH_HOOK.lock().unwrap_or_else(|e| e.into_inner()).take() { hook(); }

    // the handler name is matched on the RAW first token, before numeric
    // parsing: `soma run z.cell nan` called another handler with NaN, and
    // `soma run app.cell Cell.handler` passed "Cell.handler" as an argument
    let all_handlers: Vec<(String, String)> = program.cells.iter().flat_map(|c| c.node.sections.iter().filter_map(move |s| match &s.node {
        ast::Section::OnSignal(on) => Some((c.node.name.clone(), on.signal_name.clone())),
        _ => None,
    })).collect();
    let mut signal_owned: Option<String> = None;
    let mut args: &[String] = args;
    if signal_flag.is_none() {
        if let Some(first) = args.first() {
            let named = if all_handlers.iter().any(|(_, h)| h == first) {
                Some(first.clone())
            } else if let Some((c, h)) = first.split_once('.') {
                all_handlers.iter().any(|(cn, hn)| cn == c && hn == h).then(|| first.clone())
            } else { None };
            if let Some(h) = named { signal_owned = Some(h); args = &args[1..]; }
        }
    }
    let signal_flag: Option<&str> = signal_flag.or(signal_owned.as_deref());

    let arg_values: Vec<interpreter::Value> = args
        .iter()
        .map(|a| {
            // a number only in its canonical spelling ("007", "1_000", " 5"
            // are text; a declared Int parameter still converts them)
            if let Some(n) = a.parse::<i64>().ok().filter(|n| &n.to_string() == a) {
                interpreter::Value::Int(crate::interpreter::soma_int::SomaInt::from_i64(n))
            } else if let Some(big) = a.parse::<rug::Integer>().ok().filter(|b| &b.to_string() == a) {
                // 99999999999999999999 is an Int, not a Float saturated to i64
                interpreter::Value::Int(crate::interpreter::soma_int::SomaInt::from_rug(big))
            } else if let Some(n) = a.parse::<f64>().ok().filter(|n| n.is_finite() && (format!("{}", n) == *a || format!("{:?}", n) == *a)) {
                // "NaN" / "inf" stay Strings: a NaN passes no comparison
                interpreter::Value::Float(n)
            } else if a == "true" {
                interpreter::Value::Bool(true)
            } else if a == "false" {
                interpreter::Value::Bool(false)
            } else {
                interpreter::Value::String(a.clone())
            }
        })
        .collect();

    let regular_cells: Vec<&ast::CellDef> = program
        .cells
        .iter()
        .filter(|c| matches!(c.node.kind, ast::CellKind::Cell | ast::CellKind::Agent))
        .map(|c| &c.node)
        .collect();

    let has_interior = regular_cells.iter().any(|c| {
        c.sections.iter().any(|s| matches!(s.node, ast::Section::Interior(_)))
    });

    let has_runtime = regular_cells.iter().any(|c| {
        c.sections.iter().any(|s| matches!(s.node, ast::Section::Runtime(_)))
    });

    if has_interior || has_runtime {
        run_with_runtime(program, &arg_values);
    } else if use_jit {
        run_with_vm(program, arg_values, registry, &source, signal_flag.map(|s| s.rsplit('.').next().unwrap_or(s)));
    } else {
        run_single_cell(program, arg_values, registry, signal_flag, record_all, path, &source);
    }
}

fn run_with_vm(program: ast::Program, arg_values: Vec<interpreter::Value>, registry: &Registry, source: &str, signal_flag: Option<&str>) {
    eprintln!("note: --jit mode does not support all features yet (e.g., string interpolation)");
    let chunks = if let Some(cached) = vm::load_cached(source) {
        cached
    } else {
        let mut compiler = vm::BytecodeCompiler::new();
        compiler.compile_program(&program);
        vm::save_cache(source, &compiler.chunks);
        compiler.chunks
    };

    let cell = program.cells.iter()
        .find(|c| matches!(c.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) && c.node.sections.iter().any(|s| {
            if let ast::Section::OnSignal(ref on) = s.node { on.signal_name == "run" } else { false }
        }))
        .or_else(|| program.cells.iter().find(|c| matches!(c.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) && c.node.sections.iter().any(|s| {
            if let ast::Section::OnSignal(ref on) = s.node { on.signal_name == "request" } else { false }
        })))
        .or_else(|| program.cells.iter().find(|c| matches!(c.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) && c.node.sections.iter().any(|s| matches!(s.node, ast::Section::OnSignal(_)))))
        .or_else(|| program.cells.iter().find(|c| matches!(c.node.kind, ast::CellKind::Cell | ast::CellKind::Agent)))
        .unwrap_or_else(|| { eprintln!("error: no cell found"); process::exit(1); });
    let cell_name = cell.node.name.clone();

    let handler_names: Vec<String> = cell.node.sections.iter()
        .filter_map(|s| if let ast::Section::OnSignal(ref on) = s.node { Some(on.signal_name.clone()) } else { None })
        .collect();

    // Collect handler param counts for smart auto-dispatch
    let handler_params: Vec<(String, usize)> = cell.node.sections.iter()
        .filter_map(|s| if let ast::Section::OnSignal(ref on) = s.node {
            Some((on.signal_name.clone(), on.params.len()))
        } else { None })
        .collect();

    let (signal_name, actual_args) = if let Some(sig) = signal_flag {
        (sig.to_string(), arg_values)
    } else if let Some(interpreter::Value::String(ref name)) = arg_values.first() {
        if handler_names.contains(name) {
            (name.clone(), arg_values[1..].to_vec())
        } else {
            let name = name.clone();
            unknown_handler_or_default(&name, &handler_names, &handler_params, arg_values)
        }
    } else {
        // No explicit signal: dispatch by best match.
        //   1. `run` with exactly the right arity wins (the canonical entry point)
        //   2. any handler with the right arity (e.g. `compute` for `soma run fact.cell 5`)
        //   3. fall back to a zero-arg `run`
        //   4. any zero-arg handler
        // This used to put step 3 ahead of step 2, which made
        // `soma run fact.cell 5` try to call `run(5)` and error out
        // even when `compute(n: Int)` was right there.
        //   `main` / `run` first, and never a `_private` handler: `soma run
        //   app.cell` ran `_wipe`, the first one declared
        let n_args = arg_values.len();
        let public: Vec<&(String, usize)> = handler_params.iter().filter(|(h, _)| !h.starts_with('_')).collect();
        if public.is_empty() {
            eprintln!("error: cell '{}' has only private (`_`) handlers — name the one to run: soma run <file> <handler> …", cell_name);
            process::exit(1);
        }
        let default = public.iter().find(|(h, p)| (h == "main" || h == "run") && *p == n_args)
            .or_else(|| public.iter().find(|(_, p)| *p == n_args))
            .or_else(|| public.iter().find(|(h, _)| h == "main" || h == "run"))
            .or_else(|| public.iter().find(|(_, p)| *p == 0))
            .copied()
            .unwrap_or(public[0]);
        (default.0.clone(), arg_values)
    };

    let mut vm = vm::VM::new(chunks);

    for section in &cell.node.sections {
        if let ast::Section::Memory(ref mem) = section.node {
            let mut slots = std::collections::HashMap::new();
            for slot in &mem.slots {
                let props: Vec<String> = slot.node.properties.iter()
                    .map(|p| p.node.name().to_string()).collect();
                let backend = runtime::storage::resolve_backend_from_registry(
                    &cell_name, &slot.node.name, &props, registry);
                slots.insert(slot.node.name.clone(), backend);
            }
            vm.set_storage(&cell_name, &slots);
        }
    }

    let actual_args = coerce_cli_args(&cell.node, &signal_name, actual_args);

    match vm.call_signal(&cell_name, &signal_name, actual_args) {
        // the handler's value is the command's output; `()` prints nothing
        // (a `main` that only prints used to end with a stray `null`)
        Ok(interpreter::Value::Unit) => {}
        Ok(val) => println!("{}", run_output(&val)),
        Err(e) => { eprintln!("vm error: {}", e); process::exit(1); }
    }
}

fn run_single_cell(program: ast::Program, arg_values: Vec<interpreter::Value>, registry: &Registry, signal_flag: Option<&str>, record_all: bool, source_path: &PathBuf, source: &str) {
    // `Cell.handler` names the cell; a handler name picks the first cell
    // that defines it
    let (preferred_cell, signal_flag): (Option<String>, Option<String>) = match signal_flag {
        Some(sf) => match sf.split_once('.') {
            Some((c, h)) => (Some(c.to_string()), Some(h.to_string())),
            None => (None, Some(sf.to_string())),
        },
        None => (None, None),
    };
    let signal_flag: Option<&str> = signal_flag.as_deref();
    let requested_signal = signal_flag.map(|s| s.to_string()).or_else(|| arg_values.first().and_then(|v| {
        if let interpreter::Value::String(s) = v { Some(s.clone()) } else { None }
    }));
    let cell = preferred_cell.as_ref().and_then(|pc| program.cells.iter().find(|c| &c.node.name == pc))
    .or_else(|| requested_signal.as_ref().and_then(|sig| {
        program.cells.iter().find(|c| matches!(c.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) && c.node.sections.iter().any(|s| {
            if let ast::Section::OnSignal(ref on) = s.node { on.signal_name == *sig } else { false }
        }))
    }))
    .or_else(|| program.cells.iter().find(|c| matches!(c.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) && c.node.sections.iter().any(|s| {
        if let ast::Section::OnSignal(ref on) = s.node { on.signal_name == "run" } else { false }
    })))
    .or_else(|| program.cells.iter().find(|c| matches!(c.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) && c.node.sections.iter().any(|s| {
        if let ast::Section::OnSignal(ref on) = s.node { on.signal_name == "request" } else { false }
    })))
    .or_else(|| program.cells.iter().find(|c| matches!(c.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) && c.node.sections.iter().any(|s| matches!(s.node, ast::Section::OnSignal(_)))))
        .unwrap_or_else(|| {
            eprintln!("error: no runnable cell found");
            process::exit(1);
        });
    let cell_name = cell.node.name.clone();

    let handler_names: Vec<String> = cell.node.sections.iter()
        .filter_map(|s| {
            if let ast::Section::OnSignal(ref on) = s.node {
                Some(on.signal_name.clone())
            } else {
                None
            }
        })
        .collect();

    if handler_names.is_empty() {
        eprintln!("error: cell '{}' has no signal handlers", cell_name);
        process::exit(1);
    }

    // Collect handler param counts for smart auto-dispatch
    let handler_params: Vec<(String, usize)> = cell.node.sections.iter()
        .filter_map(|s| if let ast::Section::OnSignal(ref on) = s.node {
            Some((on.signal_name.clone(), on.params.len()))
        } else { None })
        .collect();

    let (signal_name, actual_args) = if let Some(sig) = signal_flag {
        (sig.to_string(), arg_values)
    } else if let Some(interpreter::Value::String(ref name)) = arg_values.first() {
        if handler_names.contains(name) {
            (name.clone(), arg_values[1..].to_vec())
        } else {
            let name = name.clone();
            unknown_handler_or_default(&name, &handler_names, &handler_params, arg_values)
        }
    } else {
        // No explicit signal: dispatch by best match.
        //   1. `run` with exactly the right arity wins (the canonical entry point)
        //   2. any handler with the right arity (e.g. `compute` for `soma run fact.cell 5`)
        //   3. fall back to a zero-arg `run`
        //   4. any zero-arg handler
        // This used to put step 3 ahead of step 2, which made
        // `soma run fact.cell 5` try to call `run(5)` and error out
        // even when `compute(n: Int)` was right there.
        //   `main` / `run` first, and never a `_private` handler: `soma run
        //   app.cell` ran `_wipe`, the first one declared
        let n_args = arg_values.len();
        let public: Vec<&(String, usize)> = handler_params.iter().filter(|(h, _)| !h.starts_with('_')).collect();
        if public.is_empty() {
            eprintln!("error: cell '{}' has only private (`_`) handlers — name the one to run: soma run <file> <handler> …", cell_name);
            process::exit(1);
        }
        let default = public.iter().find(|(h, p)| (h == "main" || h == "run") && *p == n_args)
            .or_else(|| public.iter().find(|(_, p)| *p == n_args))
            .or_else(|| public.iter().find(|(h, _)| h == "main" || h == "run"))
            .or_else(|| public.iter().find(|(_, p)| *p == 0))
            .copied()
            .unwrap_or(public[0]);
        (default.0.clone(), arg_values)
    };

    let mut interp = interpreter::Interpreter::new(&program);

    // V1.1: --record flag turns on recording for *every* handler in the
    // program. The user no longer annotates handlers; opt-in is at the
    // command line. Per-handler [record] still works as a compat shim.
    if record_all {
        for prog_cell in &program.cells {
            for section in &prog_cell.node.sections {
                if let ast::Section::OnSignal(ref on) = section.node {
                    interp.record_handlers.insert((prog_cell.node.name.clone(), on.signal_name.clone()));
                }
            }
        }
    }
    if !interp.record_handlers.is_empty() {
        let log_path = interpreter::record_log::default_log_path(source_path);
        eprintln!("[record] writing replay log → {}", log_path.display());
        interp.record_log_path = Some(log_path);
    }

    for prog_cell in &program.cells {
        if !matches!(prog_cell.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) { continue; }
        for section in &prog_cell.node.sections {
            if let ast::Section::Memory(ref mem) = section.node {
                let mut slots = std::collections::HashMap::new();
                for slot in &mem.slots {
                    let props: Vec<String> = slot.node.properties.iter()
                        .map(|p| p.node.name().to_string())
                        .collect();
                    let backend = runtime::storage::resolve_backend_from_registry(
                        &prog_cell.node.name, &slot.node.name, &props, registry,
                    );
                    slots.insert(slot.node.name.clone(), backend);
                }
                interp.set_storage(&prog_cell.node.name, &slots);
            }
        }
    }
    // Always ensure state machine storage exists (even without memory
    // section) — on disk: an instance must survive the run
    crate::interpreter::PERSIST_MACHINES.store(true, std::sync::atomic::Ordering::Relaxed);
    interp.ensure_state_machine_storage();

    interp.source_file = Some(source_path.display().to_string());
    interp.source_text = Some(source.to_string());

    // Read compute config from soma.toml
    let parallel_config = {
        let soma_toml = source_path.parent().unwrap_or(std::path::Path::new(".")).join("soma.toml");
        if soma_toml.exists() {
            if let Ok(content) = std::fs::read_to_string(&soma_toml) {
                if let Ok(manifest) = toml::from_str::<crate::pkg::manifest::Manifest>(&content) {
                    let c = &manifest.compute;
                    crate::codegen::native::ParallelConfig {
                        enabled: c.backend == "threads" && !c.parallel.handlers.is_empty(),
                        handlers: c.parallel.handlers.clone(),
                        threads: c.threads,
                    }
                } else { crate::codegen::native::ParallelConfig::default() }
            } else { crate::codegen::native::ParallelConfig::default() }
        } else { crate::codegen::native::ParallelConfig::default() }
    };

    // Read agent config from soma.toml [agent] section
    {
        let soma_toml = source_path.parent().unwrap_or(std::path::Path::new(".")).join("soma.toml");
        if soma_toml.exists() {
            if let Ok(content) = std::fs::read_to_string(&soma_toml) {
                if let Ok(manifest) = toml::from_str::<crate::pkg::manifest::Manifest>(&content) {
                    interp.agent_config = Some(manifest.agent);
                    interp.agent_models = manifest.models;
                }
            }
        }
    }

    // Compile and load [native] handlers
    match interpreter::native_ffi::compile_and_load_natives_with_config(&program, &parallel_config) {
        Ok(natives) => {
            interp.native_handlers = natives;
        }
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    }

    let actual_args = coerce_cli_args(&cell.node, &signal_name, actual_args);

    // stored data older than the program (an invariant added since, an
    // instance in a removed state): say so, like serve does
    // a damaged database read back as `()` and `soma run` exited 0 (serve
    // already refused it): the same quick_check, before anything runs
    if let Err(why) = crate::runtime::storage::integrity_check() {
        eprintln!("error: .soma_data/soma.db is damaged ({}) — restore it from a backup; nothing was run", why);
        process::exit(1);
    }
    for line in interp.audit_stored_data() {
        eprintln!("warning: stored data: {}", line);
    }
    let result = interp.call_signal(&cell_name, &signal_name, actual_args);
    // move the committed rows from the WAL into the database file before
    // exiting: left in the WAL, one damaged frame silently dropped every
    // later commit (an [immutable] log read back empty, quick_check "ok")
    if let Some(conn) = crate::runtime::storage::shared_connection() {
        let c = conn.lock().unwrap_or_else(|e| e.into_inner());
        let _ = c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)");
    }
    // `soma run` has no bus: an emit meant for [peers] committed its
    // writes and reached nobody, without a word (an admin fix skipped the
    // notification service)
    if result.is_ok() && super::HAS_PEERS.load(std::sync::atomic::Ordering::Relaxed) {
        let mut names: Vec<String> = interp.take_emitted_signals().into_iter().map(|(n, _)| n).collect();
        names.sort();
        names.dedup();
        if !names.is_empty() {
            eprintln!("warning: emit {} NOT delivered to [peers] — `soma run` opens no bus (the writes are committed; send the event from the served program, or reconcile)",
                names.iter().map(|n| format!("'{}'", n)).collect::<Vec<_>>().join(", "));
        }
    }
    match result {
        // the handler's value is the command's output; `()` prints nothing
        // (a `main` that only prints used to end with a stray `null`)
        Ok(interpreter::Value::Unit) => {}
        Ok(val) => println!("{}", run_output(&val)),
        Err(e) => {
            eprintln!("{}", interpreter::format_runtime_error(
                &e,
                interp.source_file.as_deref(),
                interp.source_text.as_deref(),
                interp.last_span,
            ));
            process::exit(1);
        }
    }
}

/// `soma run app.cell nosuch 1`: a first token that looks like a handler
/// name but is none is an error with the real names — it used to be fed
/// silently to the first handler as its String argument. A token that is
/// plainly data (`soma run app.cell hello` for `on greet(name: String)`)
/// still goes to the handler whose arity matches.
fn unknown_handler_or_default(
    name: &str,
    handler_names: &[String],
    handler_params: &[(String, usize)],
    arg_values: Vec<interpreter::Value>,
) -> (String, Vec<interpreter::Value>) {
    let n_args = arg_values.len();
    let by_arity = handler_params.iter().find(|(h, p)| (h == "run" || h == "main") && *p == n_args)
        .or_else(|| handler_params.iter().find(|(h, p)| *p == n_args && !h.starts_with('_')));
    let identifier_like = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_');
    // a handler takes the rest of the args → the token was meant as a name
    let rest_fits = handler_params.iter().any(|(_, p)| *p + 1 == n_args);
    // a near-miss of a handler name is a typo, never an argument:
    // `list_acounts` ran close_account("list_acounts")
    let near_miss = crate::checker::names::suggest(name, handler_names.iter()).is_some();
    // with SEVERAL public handlers a bare word is a handler name, never data
    // (`soma run app.cell help` wiped "help" through the first handler)
    let several = handler_names.iter().filter(|h| !h.starts_with('_') && h.as_str() != "request").count() > 1;
    // …and so is any token that is not plainly data (a number or JSON):
    // `Arch`, `pay-now`, `"st q"` ran another handler by arity and committed
    let data_like = name.trim().parse::<f64>().is_ok() || name.trim_start().starts_with(['[', '{', '"']);
    if (identifier_like && (by_arity.is_none() || rest_fits || near_miss || several))
        || near_miss || (several && !data_like) {
        let public: Vec<&String> = handler_names.iter().filter(|h| !h.starts_with('_')).collect();
        let near = crate::checker::names::suggest(name, public.iter().copied())
            .map(|h| format!(" (did you mean '{}'?)", h))
            .unwrap_or_default();
        eprintln!("error: no handler named '{}'{} — handlers: [{}]; usage: soma run app.cell <handler> [args…]",
            name, near, public.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        process::exit(1);
    }
    match by_arity {
        Some((h, _)) => (h.clone(), arg_values),
        None => (handler_names[0].clone(), arg_values),
    }
}

/// Validate and coerce CLI args against the handler's declared parameter
/// types, so a String passed where an Int is declared fails at the call
/// boundary instead of deep inside the handler body.
fn coerce_cli_args(cell: &ast::CellDef, signal_name: &str, args: Vec<interpreter::Value>) -> Vec<interpreter::Value> {
    let params = cell.sections.iter().find_map(|s| {
        if let ast::Section::OnSignal(ref on) = s.node {
            if on.signal_name == signal_name { return Some(&on.params); }
        }
        None
    });
    let Some(params) = params else { return args };
    // `soma run app.cell request GET "/a?x=1" ""` — split the query off the
    // path as `soma serve` does (the route got "/a?x=1" and an empty query)
    let mut args = args;
    if signal_name == "request" {
        let pi = params.iter().position(|p| p.name == "path");
        let qi = params.iter().position(|p| p.name == "query");
        if let Some(pi) = pi {
            if let Some(interpreter::Value::String(full)) = args.get(pi).cloned() {
                // path segments reach request percent-decoded, as under serve
                // (`/w/%C3%A9` was "é" served and "%C3%A9" under soma run)
                let raw_path = full.split('?').next().unwrap_or(&full).to_string();
                args[pi] = interpreter::Value::String(crate::commands::serve::urlencoding_decode(&raw_path));
                if let Some((_, qs)) = full.split_once('?') {
                    if let Some(qi) = qi {
                        let given_empty = match args.get(qi) { None => true, Some(interpreter::Value::Map(m)) => m.is_empty(), Some(interpreter::Value::String(t)) => t.is_empty() || t == "{}", _ => false };
                        if given_empty {
                            let pairs: Vec<(String, interpreter::Value)> = qs.split('&').filter(|p| !p.is_empty()).map(|p| {
                                let (k, v) = p.split_once('=').unwrap_or((p, ""));
                                (crate::commands::serve::urlencoding_decode(&k.replace('+', " ")), interpreter::Value::String(crate::commands::serve::urlencoding_decode(&v.replace('+', " "))))
                            }).collect();
                            while args.len() <= qi { args.push(interpreter::Value::Map(Default::default())); }
                            args[qi] = interpreter::map_from_pairs(pairs);
                        }
                    }
                }
            }
        }
    }
    args.into_iter().enumerate().map(|(i, arg)| {
        let Some(param) = params.get(i) else { return arg };
        // `request`'s headers map: lower-case names, as `soma serve` gives them
        if signal_name == "request" && param.name == "headers" {
            if let interpreter::Value::Map(m) = &arg {
                return interpreter::Value::Map(m.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.clone())).collect());
            }
        }
        let ty: String = match param.ty.node {
            ast::TypeExpr::Simple(ref ty) => ty.clone(),
            ast::TypeExpr::Generic { ref name, .. } => name.clone(),
            _ => return arg,
        };
        let fail = |got: &str| -> interpreter::Value {
            eprintln!("error: argument '{}' of signal '{}' expects {}, got {}",
                param.name, signal_name, ty, got);
            process::exit(1)
        };
        match (ty.as_str(), &arg) {
            ("Int", interpreter::Value::Int(_)) => arg,
            // "1.0" is not an Int (HTTP answers 400 type for the same path)
            // a non-canonical spelling ("007", "1_000") is text until a
            // declared Int / Float converts it
            ("Int", interpreter::Value::String(t)) if t.trim().replace('_', "").parse::<rug::Integer>().is_ok() =>
                interpreter::Value::Int(crate::interpreter::soma_int::SomaInt::from_rug(t.trim().replace('_', "").parse::<rug::Integer>().unwrap())),
            ("Int", other) => fail(&format!("'{}'", other)),
            ("Float", interpreter::Value::Float(_)) => arg,
            ("Float", interpreter::Value::Int(si)) => interpreter::Value::Float(si.to_f64()),
            ("Float", interpreter::Value::String(t)) if t.trim().parse::<f64>().map_or(false, |f| f.is_finite()) =>
                interpreter::Value::Float(t.trim().parse::<f64>().unwrap()),
            ("Float", other) => fail(&format!("'{}'", other)),
            ("Bool", interpreter::Value::Bool(_)) => arg,
            ("Bool", other) => fail(&format!("'{}'", other)),
            // a numeric/bool-looking CLI token passed to a String param is a string
            ("String", interpreter::Value::String(_)) => arg,
            ("String", other) => interpreter::Value::String(format!("{}", other)),
            // a Map/List parameter takes its CLI token as JSON
            ("Map" | "List", interpreter::Value::String(s)) => {
                if s.trim().is_empty() {
                    return if ty == "Map" { interpreter::Value::Map(Default::default()) } else { interpreter::Value::List(vec![]) };
                }
                if serde_json::from_str::<serde_json::Value>(s).is_err() {
                    return fail(&format!("a string that is not valid JSON: '{}'", s));
                }
                let v = crate::interpreter::builtins::json_to_value(s);
                match (ty.as_str(), &v) {
                    ("Map", interpreter::Value::Map(_)) | ("List", interpreter::Value::List(_)) => v,
                    _ => fail(&format!("JSON that is not a {}: '{}'", ty, s)),
                }
            }
            ("Map", interpreter::Value::Map(_)) | ("List", interpreter::Value::List(_)) => arg,
            ("Map" | "List", other) => fail(&format!("'{}'", other)),
            _ => arg,
        }
    }).collect()
}

fn run_with_runtime(program: ast::Program, args: &[interpreter::Value]) {
    let mut rt = runtime::Runtime::new(program);

    eprintln!("soma runtime v0.1.0");
    eprintln!("---");
    rt.dump_state();
    eprintln!("---");

    let main_cell = rt.cells.keys().next().cloned().unwrap_or_else(|| {
        eprintln!("error: no runnable cell found");
        process::exit(1);
    });

    if let Err(e) = rt.run_cell(&main_cell) {
        eprintln!("runtime error: {}", e);
        process::exit(1);
    }

    if !args.is_empty() {
        let handler_info: Option<(String, String)> = rt.cells.get(&main_cell)
            .and_then(|cell| {
                cell.children.values().find_map(|child| {
                    child.def.sections.iter().find_map(|s| {
                        if let ast::Section::OnSignal(ref on) = s.node {
                            Some((child.name.clone(), on.signal_name.clone()))
                        } else {
                            None
                        }
                    })
                })
            });

        if let Some((_child_name, signal_name)) = handler_info {
            match rt.emit_signal(&main_cell, &signal_name, args.to_vec()) {
                Ok(results) => {
                    for val in results {
                        println!("{}", val);
                    }
                }
                Err(e) => {
                    eprintln!("runtime error: {}", e);
                    process::exit(1);
                }
            }
        }
    }

    if !rt.signal_log.is_empty() {
        eprintln!("---");
        eprintln!("signal log:");
        for entry in &rt.signal_log {
            eprintln!("  {}", entry);
        }
    }
}

/// What `soma run` prints for a handler's value: valid JSON for maps, lists
/// and variants (NaN/inf → null, a variant → `{"_type", "_variant", …}`,
/// same writer as `to_json` and `soma serve`); scalars as themselves.
fn run_output(val: &interpreter::Value) -> String {
    match val {
        interpreter::Value::Map(_) | interpreter::Value::List(_) | interpreter::Value::Variant { .. } =>
            interpreter::builtins::string::to_json_string_spaced(val),
        other => format!("{}", other),
    }
}
