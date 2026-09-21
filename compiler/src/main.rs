#![allow(dead_code, unused_imports, unused_variables)]

mod ast;
mod checker;
mod codegen;
mod commands;
mod interpreter;
mod lexer;
mod parser;
mod pkg;
mod provider;
mod registry;
mod runtime;
mod vm;

use clap::{Parser as ClapParser, Subcommand};
use std::path::PathBuf;

use registry::Registry;

#[derive(ClapParser)]
#[command(name = "soma")]
#[command(version)]
#[command(about = "The Soma language — fractal, declarative, agent-native")]
#[command(long_about = "The Soma language — fractal, declarative, agent-native\n\n\
    Soma is a language where every system is a cell: state, interface, lifecycle,\n\
    and distribution — in one model, from function to datacenter.\n\n\
    Quick start:\n  \
      soma init myapp && cd myapp\n  \
      soma serve app.cell            # HTTP on :8080\n  \
      soma check app.cell            # verify contracts\n  \
      soma verify app.cell           # prove state machines\n\n\
    Cluster (same code, multiple nodes):\n  \
      soma serve app.cell -p 8080\n  \
      soma serve app.cell -p 8081 --join localhost:8082\n\n\
    For coding agents:\n  \
      soma docs agent                # the language, offline, for your context\n  \
      soma describe --builtins --json   # exact signatures — never guess\n  \
      soma example invariant state_machine    # verified programs to start from\n\n\
    Docs: https://soma-lang.dev   (agents: https://soma-lang.dev/llms-full.txt)\n\
    Paper: https://soma-lang.dev/paper")]
#[command(after_help = "Examples:\n  \
    soma serve app.cell                     Start web server\n  \
    soma serve app.cell --join host:8082    Join a cluster\n  \
    soma check app.cell --json              Errors as JSON (for agents)\n  \
    soma verify app.cell --json             Proofs as JSON (for agents)\n  \
    soma describe app.cell                  Cell structure as JSON\n  \
    soma run app.cell 42 \"hello\"            Execute a handler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to stdlib directory (default: auto-detect)
    #[arg(long, global = true)]
    stdlib: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    // ── Core ──────────────────────────────────────────────────────
    /// Run a handler: soma run app.cell [args]
    Run {
        /// Path to the .cell source file
        file: PathBuf,
        /// Handler name, then its arguments (Int, Float, Bool, JSON for a List or Map, else String); without a name: `main`/`run`, or the only handler that fits
        // negative numbers are values, not flags: `soma run f.cell -7 2`;
        // and after the handler name everything is an argument (`soma run
        // app.cell put --fresh` wiped the database)
        #[arg(allow_negative_numbers = true, trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        /// Deprecated: use [native] on handlers instead
        #[arg(long, hide = true)]
        jit: bool,
        /// Signal handler to call (default: auto-detect)
        #[arg(long)]
        signal: Option<String>,
        /// V1.1: record every handler invocation to a .somalog file
        /// (use `soma replay` to deterministically re-execute it).
        /// Default off — no perf overhead unless asked.
        #[arg(long)]
        record: bool,
        /// Start from empty storage: delete `.soma_data/` beside the program first (only this program's tables when other programs' cells also keep data there)
        #[arg(long)]
        fresh: bool,
    },
    /// Start HTTP server: soma serve app.cell [-p 8080] [--join host:port]
    Serve {
        /// Path to the .cell source file
        file: PathBuf,
        /// Port to listen on (default: 8080)
        #[arg(short, long, default_value = "8080")]
        port: u16,
        /// Address to bind (default: loopback only; 0.0.0.0 exposes the service to the network)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Serve even when `soma check` reports errors
        #[arg(long)]
        no_check: bool,
        /// Do not run `every` / `after` blocks (HTTP handlers only — for tests and debugging)
        #[arg(long)]
        no_schedule: bool,
        /// Watch for changes and auto-reload
        #[arg(short, long)]
        watch: bool,
        /// Show parsed parameters and response body
        #[arg(long)]
        verbose: bool,
        /// Join an existing cluster node (host:port of its bus)
        #[arg(long)]
        join: Option<String>,
    },

    // ── Verify ────────────────────────────────────────────────────
    /// Check contracts, properties, and scale coherence
    Check {
        /// Path to the .cell source file
        file: PathBuf,
        /// Output as JSON (for agents)
        #[arg(long)]
        json: bool,
    },
    /// Lint for anti-patterns and suggest improvements
    Lint {
        /// Path to the .cell source file
        file: PathBuf,
        /// Output as JSON (for agents)
        #[arg(long)]
        json: bool,
    },
    /// Auto-fix common errors found by check
    Fix {
        /// Path to the .cell source file
        file: PathBuf,
        /// Output as JSON (for agents)
        #[arg(long)]
        json: bool,
        /// Rewrite `a / b` on two Ints inside [native] handlers to
        /// idiv(a, b) — for code written when native `/` truncated
        #[arg(long)]
        native_idiv: bool,
    },
    /// Prove state machines, temporal logic, CAP properties, quorum
    Verify {
        /// Path to the .cell source file(s)
        files: Vec<PathBuf>,
        /// Output as JSON (for agents)
        #[arg(long)]
        json: bool,
        /// Fail when anything is only runtime-checked or unprovable (every ⚠ is an error) — for CI gates
        #[arg(long)]
        strict: bool,
    },
    /// Run test assertions in a .cell file. Storage is isolated: every
    /// memory slot uses a fresh in-memory backend, so runs never touch
    /// persistent state and always start clean. Supports `assert expr`,
    /// `assert_fails expr` (passes when expr errors), and `property`.
    Test {
        /// Path to the .cell file containing test cells
        file: PathBuf,
        /// Output as JSON (for agents): one record per rule, then the totals
        #[arg(long)]
        json: bool,
    },
    /// V1: replay a .somalog file deterministically and report divergences
    Replay {
        /// Path to the .cell source file
        file: PathBuf,
        /// Path to the .somalog file (default: alongside the .cell)
        #[arg(long)]
        log: Option<PathBuf>,
        /// Replay only entries up to this timestamp (epoch ms or ISO-8601)
        #[arg(long)]
        at: Option<String>,
    },

    // ── Agent ─────────────────────────────────────────────────────
    /// Describe a cell as structured JSON: signals, memory, state machines, scale, routes
    Describe {
        /// Path to the .cell source file (not required with --builtins)
        file: Option<PathBuf>,
        /// List every builtin: signature + brief, grouped by category (✗ = nondeterministic)
        #[arg(long)]
        builtins: bool,
        /// Print only cell contracts: face signals, tools, memory slots, state machines, sum types
        #[arg(long)]
        faces: bool,
        /// Output as JSON (for agents)
        #[arg(long)]
        json: bool,
    },
    /// Render generated reference docs: soma docs builtins
    Docs {
        /// agent | reference | gotchas | builtins | agents-md | all (omit to list)
        #[arg(default_value = "")]
        topic: String,
    },
    /// Find a verified example program: soma example invariant state_machine
    Example {
        /// Search terms (domain, feature, word) — or an exact id to print its source
        terms: Vec<String>,
        /// Output as JSON (for agents)
        #[arg(long)]
        json: bool,
        /// List every match (default: the first 20)
        #[arg(long)]
        all: bool,
    },

    // ── Project ───────────────────────────────────────────────────
    /// Create a new Soma project
    Init {
        /// Project name (default: current directory name)
        name: Option<String>,
    },
    /// Add a dependency: soma add pkg --git url
    Add {
        /// Package name or git URL
        package: String,
        /// Version or branch
        #[arg(long)]
        version: Option<String>,
        /// Git URL
        #[arg(long)]
        git: Option<String>,
        /// Local path
        #[arg(long)]
        path: Option<String>,
    },
    /// Install all dependencies from soma.toml
    Install,
    /// List installed packages
    Env,
    /// Start an interactive REPL
    Repl,
    /// List all registered properties and their rules
    Props,

    // ── Deploy ────────────────────────────────────────────────────
    /// Deploy to a cloud provider: soma deploy app.cell --target cloudflare
    Deploy {
        /// Path to the .cell source file
        file: PathBuf,
        /// Target provider: cloudflare, fly, aws
        #[arg(long)]
        target: String,
        /// Cloud region (for AWS)
        #[arg(long)]
        region: Option<String>,
    },

    // ── Advanced ──────────────────────────────────────────────────
    /// Compile a .cell file and generate Rust code
    Build {
        /// Path to the .cell source file
        file: PathBuf,
        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Dump the AST (debug)
    #[command(hide = true)]
    Ast {
        file: PathBuf,
    },
    /// Dump tokens (debug)
    #[command(hide = true)]
    Tokens {
        file: PathBuf,
    },
    /// Add a storage provider
    #[command(hide = true)]
    AddProvider {
        /// Provider name (e.g., aws, gcp, cloudflare)
        name: String,
    },
    /// Test a storage provider
    #[command(hide = true)]
    TestProvider {
        /// Provider name
        name: String,
    },
    /// Migrate data between providers
    #[command(hide = true)]
    Migrate {
        /// Source provider
        #[arg(long)]
        from: String,
        /// Target provider
        #[arg(long)]
        to: String,
    },
}

fn main() {
    // a shell that starts `soma serve` in the background leaves SIGINT
    // IGNORED (POSIX): `kill -INT` then did nothing, and a supervisor
    // believed it had stopped the server — take the default back
    #[cfg(unix)]
    unsafe {
        extern "C" { fn signal(sig: i32, handler: usize) -> usize; }
        const SIGINT: i32 = 2;
        const SIG_DFL: usize = 0;
        const SIG_IGN: usize = 1;
        if signal(SIGINT, SIG_DFL) != SIG_IGN { signal(SIGINT, SIG_DFL); }
    }
    // `soma docs builtins | head`: stdout closed by the reader is the end of
    // the output, not a crash (it panicked "failed printing to stdout")
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info.payload().downcast_ref::<String>().map(|s| s.as_str())
            .or_else(|| info.payload().downcast_ref::<&str>().copied()).unwrap_or("");
        if msg.contains("failed printing to stdout") && msg.contains("Broken pipe") {
            std::process::exit(0);
        }
        prev(info);
    }));
    // Run on a thread with an 8 MB stack to prevent SIGABRT on deep recursion
    // before the interpreter's own depth guard (max_depth: 512) can fire.
    // 512 MB of (virtual, lazily committed) stack: a value nested 45,000
    // deep aborted `soma run` in equality / to_json (exit 134), where serve
    // (64 MB request threads) survived
    let builder = std::thread::Builder::new().stack_size(512 * 1024 * 1024);
    let handler = builder.spawn(main_inner).expect("failed to spawn main thread");
    if let Err(e) = handler.join() {
        eprintln!("fatal: {:?}", e);
        std::process::exit(1);
    }
}

fn main_inner() {
    if std::env::args().any(|a| a == "--json") {
        commands::JSON_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let cli = Cli::parse();

    let mut registry = Registry::new();
    let stdlib_path = cli.stdlib.clone().unwrap_or_else(commands::find_stdlib);
    if let Err(e) = registry.load_dir(&stdlib_path) {
        eprintln!("warning: failed to load stdlib: {}", e);
    }

    match cli.command {
        Commands::Check { file, json } => {
            if file.is_dir() { run_over_dir("check", &file, json); }
            commands::check::cmd_check(&file, json, &mut registry)
        }
        Commands::Lint { file, json } => commands::lint::cmd_lint(&file, json),
        Commands::Fix { file, json, native_idiv } => {
            if native_idiv {
                commands::fix::cmd_fix_native_idiv(&file);
            } else {
                commands::fix::cmd_fix(&file, json, &mut registry);
            }
        }
        Commands::Build { file, output } => commands::build::cmd_build(&file, output.as_deref(), &mut registry),
        Commands::Ast { file } => cmd_ast(&file),
        Commands::Tokens { file } => cmd_tokens(&file),
        Commands::Run { file, args, jit, signal, record, fresh } => {
            runtime::storage::set_data_dir_beside(&file);
            // --fresh deletes data only once the program passes `soma check`
            // (a program with an error wiped the database, then refused to run)
            if fresh {
                let file = file.clone();
                let hook: Box<dyn FnOnce() + Send> = Box::new(move || {
                // `--fresh --record` starts a new log: the old one described runs
                // from storage `--fresh` just deleted (replay then "diverged"). It
                // is kept beside, renamed.
                if fresh && record {
                    let log = interpreter::record_log::default_log_path(&file);
                    if log.exists() {
                        let prev = log.with_extension("somalog.prev");
                        let _ = std::fs::rename(&log, &prev);
                        eprintln!("fresh: started a new record log ({} kept as {})", log.display(), prev.display());
                    }
                }
                if fresh {
                    // the database lives beside the program (see operations.md)
                    let data = runtime::storage::data_dir();
                    // not under a running `soma serve` (it holds a shared lock)
                    if let Ok(conn) = rusqlite::Connection::open(data.join("serve.lock")) {
                        let _ = conn.busy_timeout(std::time::Duration::from_millis(0));
                        if data.join("serve.lock").exists() && conn.execute_batch("BEGIN EXCLUSIVE").is_err() {
                            eprintln!("error: --fresh: a `soma serve` is running on {} — stop it first (deleting its database under it loses everything it writes next)", data.display());
                            std::process::exit(1);
                        }
                    }
                    // the database is per DIRECTORY: `--fresh other.cell` wiped
                    // the app's data beside it — when other programs' cells have
                    // tables there, only THIS program's tables are reset
                    if data.join("soma.db").exists() {
                        let (own, foreign) = cells_in_db(&file, &data.join("soma.db"));
                        if !foreign.is_empty() {
                            if let Ok(conn) = rusqlite::Connection::open(data.join("soma.db")) {
                                for t in &own { let _ = conn.execute_batch(&format!("DROP TABLE IF EXISTS \"{}\"", t.replace('"', "\"\""))); }
                            }
                            eprintln!("fresh: reset {} table(s) of this program — {} also holds tables {} (other programs in this directory), kept",
                                own.len(), data.display(), foreign.join(", "));
                            return;
                        }
                    }
                    if data.exists() {
                        if let Err(e) = std::fs::remove_dir_all(&data) {
                            eprintln!("error: --fresh: cannot remove {}: {}", data.display(), e);
                            std::process::exit(1);
                        }
                        eprintln!("fresh: removed {}", data.display());
                    }
                }
                });
                *commands::run::FRESH_HOOK.lock().unwrap_or_else(|e| e.into_inner()) = Some(hook);
            }
            commands::run::cmd_run(&file, &args, jit, signal.as_deref(), record, &mut registry)
        }
        Commands::Serve { file, port, host, no_check, no_schedule, watch, verbose, join } => {
            runtime::storage::set_data_dir_beside(&file);
            if watch {
                commands::serve::cmd_serve_watch(&file, port, &mut registry);
            } else {
                if no_schedule { commands::serve::NO_SCHEDULE.store(true, std::sync::atomic::Ordering::Relaxed); }
                commands::serve::cmd_serve(&file, port, &host, verbose, join.as_deref(), no_check, &mut registry);
            }
        }
        Commands::Test { file, json } => {
            if file.is_dir() { run_over_dir("test", &file, json); }
            commands::test_cmd::cmd_test(&file, json, &mut registry)
        }
        Commands::Replay { file, log, at } => commands::replay::cmd_replay(&file, log.as_ref(), at.as_deref(), &mut registry),
        Commands::Init { name } => commands::init::cmd_init(name.as_deref()),
        Commands::Add { package, version, git, path } => commands::init::cmd_add(&package, version.as_deref(), git.as_deref(), path.as_deref()),
        Commands::Install => commands::init::cmd_install(),
        Commands::Env => commands::init::cmd_env(),
        Commands::Repl => commands::repl::cmd_repl(&mut registry),
        Commands::AddProvider { name } => commands::provider::cmd_add_provider(&name),
        Commands::TestProvider { name } => commands::provider::cmd_test_provider(&name),
        Commands::Migrate { from, to } => commands::provider::cmd_migrate(&from, &to),
        Commands::Props => commands::props::cmd_props(&registry),
        Commands::Verify { files, json, strict } => cmd_verify(&files, json, strict),
        Commands::Deploy { file, target, region } => commands::deploy::cmd_deploy(&file, &target, region.as_deref()),
        Commands::Describe { file, builtins, faces, json } => {
            if builtins {
                commands::describe::cmd_describe_builtins(json);
            } else if let Some(ref file) = file {
                if faces {
                    commands::describe::cmd_describe_faces(file, json);
                } else {
                    commands::describe::cmd_describe(file);
                }
            } else {
                eprintln!("error: missing file argument.\n  soma describe <file.cell>            full structure as JSON\n  soma describe <file.cell> --faces    contracts only (token-cheap)\n  soma describe --builtins [--json]    builtin signature table");
                std::process::exit(1);
            }
        }
        Commands::Docs { topic } => commands::docs::cmd_docs(&topic),
        Commands::Example { terms, json, all } => commands::example::cmd_example(&terms, json, all),
    }
}

fn cmd_verify(files: &[PathBuf], json: bool, strict: bool) {
    use checker::temporal::*;
    commands::VERIFY_MODE.store(true, std::sync::atomic::Ordering::Relaxed);

    let mut all_results = Vec::new();
    let mut all_cell_names: Vec<String> = Vec::new();
    // horde cost bounds, printed when no state machine gives verify more to say
    let mut horde_lines: Vec<(bool, String)> = Vec::new();
    let mut cross_cell: Vec<String> = Vec::new();
    let mut machine_cells: Vec<String> = Vec::new();
    let mut all_temporal = Vec::new();
    let mut total_cells: usize = 0;

    // Try to read soma.toml for user-defined properties
    let manifest = files.first()
        .and_then(|f| f.parent())
        .map(|dir| dir.join("soma.toml"))
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read_to_string(&p).ok())
        .and_then(|content| toml::from_str::<pkg::manifest::Manifest>(&content).ok());

    let verify_config = manifest.as_ref().map(|m| &m.verify);

    let mut unknown_states: Vec<String> = Vec::new();
    let check_failed = false;

    for path in files {
        let source = commands::read_source(path);
        let file_str = path.display().to_string();
        let tokens = commands::lex_with_location(&source, Some(&file_str));
        let mut program = commands::parse_with_location(tokens, Some(&source), Some(&file_str));
        commands::resolve_imports(&mut program, path);

        // A proof about a program that does not pass `soma check` is a proof
        // about nothing: run the static gates first.
        {
            let mut registry = registry::Registry::new();
            commands::load_meta_cells_from_program(&program, &mut registry, path);
            let mut chk = checker::Checker::new(&registry);
            chk.source = Some((file_str.clone(), source.clone()));
            chk.check(&program);
            if chk.has_errors() {
                // the error blocks only (a warning's `-->` / caret lines used
                // to be printed without the warning), then STOP: a proof
                // about a program that does not check proves nothing
                // on stdout with the verdict: verify's whole report is stdout
                // under --json stdout is ONE JSON document, with the errors
                // in it (a text line came first and the errors were dropped)
                if !json { println!("{} fails `soma check` — fix these before verifying:", path.display()); }
                let mut in_error = false;
                let mut errors: Vec<String> = Vec::new();
                for line in chk.report().lines() {
                    if line.starts_with("error") { in_error = true; errors.push(line.to_string()); }
                    else if line.starts_with("warning") || line.starts_with("advisory") || line.starts_with("✓") || line.starts_with("✗") || line.starts_with("note") { in_error = false; }
                    else if in_error { if let Some(last) = errors.last_mut() { last.push('\n'); last.push_str(line); } }
                    if in_error && !json { println!("  {}", line); }
                }
                if json {
                    println!("{}", serde_json::json!({"ok": false, "verdict": "VERIFY FAILED — soma check failed", "cells": [], "check_errors": errors}));
                } else {
                    println!("VERIFY FAILED — soma check failed (fix the errors above, then verify)");
                }
                std::process::exit(1);
            }
        }

        // every state a TARGETED machine declares (`cells = [...]` in
        // soma.toml, else every machine of the file). A property naming
        // another cell's state used to pass vacuously ("never reached").
        let targeted: Vec<String> = verify_config.as_ref().map(|c| c.cells.clone()).unwrap_or_default();
        let all_states: std::collections::HashSet<String> = program.cells.iter()
            .filter(|c| targeted.is_empty() || targeted.contains(&c.node.name))
            .flat_map(|c| c.node.sections.iter())
            .filter_map(|s| if let ast::Section::State(sm) = &s.node { Some(sm) } else { None })
            .flat_map(|sm| StateMachineGraph::from_ast(sm).states.into_iter())
            .collect();

        eprintln!("Verifying {}...", path.display());
        all_cell_names.extend(program.cells.iter().map(|c| c.node.name.clone()));
        machine_cells.extend(program.cells.iter()
            .filter(|c| c.node.sections.iter().any(|s| matches!(s.node, ast::Section::State(_))))
            .map(|c| c.node.name.clone()));
        total_cells += program.cells.iter()
            .filter(|c| matches!(c.node.kind, ast::CellKind::Cell | ast::CellKind::Agent))
            .count();
        // Also count interior cells
        for cell in &program.cells {
            for section in &cell.node.sections {
                if let ast::Section::Interior(ref interior) = section.node {
                    total_cells += interior.cells.len();
                }
            }
        }
        let results = checker::verify::verify_program(&program);
        all_results.extend(results);
        {
            let all = checker::cost::all_handlers_of(&program);
            for c in &program.cells { horde_lines.extend(checker::cost::horde_bounds(&c.node, &all)); }
            cross_cell.extend(checker::verify::cross_cell_notes(&program));
        }

        // Run temporal property checks on each state machine
        for cell in &program.cells {
            if !matches!(cell.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) { continue; }
            for section in &cell.node.sections {
                if let ast::Section::State(ref sm) = section.node {
                    let graph = StateMachineGraph::from_ast(sm);
                    let mut props = Vec::new();

                    // Auto-derive: deadlock_free
                    props.push(Property::DeadlockFree);

                    // User-defined properties from soma.toml [verify].
                    // If [verify] names specific cells, only those cells get
                    // the user properties — a shared soma.toml must not impose
                    // one cell's temporal properties on unrelated cells.
                    let applies = verify_config.map_or(false, |cfg| {
                        cfg.cells.is_empty() || cfg.cells.contains(&cell.node.name)
                    });
                    if let Some(cfg) = verify_config.filter(|_| applies) {
                        if cfg.deadlock_free {
                            // already added above
                        }

                        // eventually = ["settled", "cancelled"]
                        if !cfg.eventually.is_empty() {
                            props.push(Property::Eventually(StatePredicate::InSet(
                                cfg.eventually.clone()
                            )));
                        }

                        // never = ["error_state"]
                        for state in &cfg.never {
                            props.push(Property::Never(StatePredicate::InState(state.clone())));
                        }

                        // always = ["a", "b"]: the machine is only ever in one
                        // of these states (one property over the SET — a
                        // per-state `always` could only hold for a machine
                        // with a single state).
                        if !cfg.always.is_empty() {
                            props.push(Property::Always(StatePredicate::InSet(cfg.always.clone())));
                        }

                        // [verify.before.paid] requires = ["manager_approved"]
                        for (target, before_cfg) in &cfg.before {
                            // requires = [a, b]: at least ONE of them
                            if !before_cfg.requires.is_empty() {
                                props.push(Property::Requires(target.clone(), before_cfg.requires.clone()));
                            }
                            // requires_all = [a, b]: EACH of them
                            for state in &before_cfg.requires_all {
                                props.push(Property::Requires(target.clone(), vec![state.clone()]));
                            }
                        }

                        // [verify.after.sent]
                        // eventually = ["filled", "rejected"]
                        for (trigger, after_cfg) in &cfg.after {
                            if !after_cfg.eventually.is_empty() {
                                props.push(Property::After(
                                    trigger.clone(),
                                    StatePredicate::InSet(after_cfg.eventually.clone()),
                                ));
                            }
                            for state in &after_cfg.never {
                                props.push(Property::AfterNever(trigger.clone(), state.clone()));
                            }
                        }
                    }

                    // A property naming a state the machine does not have is
                    // a typo, not a theorem: `never = ["piad"]` used to pass
                    // as "never reached in any execution".
                    if applies {
                        if let Some(cfg) = verify_config {
                            let mut named: Vec<(&str, &String)> = Vec::new();
                            named.extend(cfg.eventually.iter().map(|s| ("eventually", s)));
                            named.extend(cfg.never.iter().map(|s| ("never", s)));
                            named.extend(cfg.always.iter().map(|s| ("always", s)));
                            for (trigger, a) in &cfg.after {
                                named.push(("[verify.after.<state>]", trigger));
                                named.extend(a.eventually.iter().map(|s| ("after … eventually", s)));
                                named.extend(a.never.iter().map(|s| ("after … never", s)));
                            }
                            for (target, b) in &cfg.before {
                                named.push(("[verify.before.<state>]", target));
                                named.extend(b.requires.iter().map(|s| ("before … requires", s)));
                                named.extend(b.requires_all.iter().map(|s| ("before … requires_all", s)));
                            }
                            // with several machines in scope a state may belong
                            // to another one: only complain when NO machine of
                            // the verified files declares it
                            for (what, state) in named {
                                if !graph.states.contains(state) && !all_states.contains(state.as_str()) {
                                    unknown_states.push(format!("{} names state '{}', which no verified state machine declares{}", what, state,
                                        if targeted.is_empty() { "" } else { " (cells = [...] in soma.toml limits verification to those cells)" }));
                                }
                            }
                        }
                    }

                    let results: Vec<PropertyResult> = props.iter()
                        .map(|p| check_property(&graph, p))
                        .collect();

                    let machine_cells = program.cells.iter().filter(|c| c.node.sections.iter().any(|s| matches!(s.node, ast::Section::State(_)))).count();
                    let label = if machine_cells > 1 { format!("{}.{}", cell.node.name, sm.name) } else { sm.name.clone() };
                    all_temporal.push((label, results));
                }
            }
        }
    }

    if all_results.is_empty() && check_failed {
        // still ONE verdict line, the one every caller greps for
        if json {
            println!("{}", serde_json::json!({"ok": false, "verdict": "VERIFY FAILED — soma check failed", "cells": []}));
        } else {
            println!("VERIFY FAILED — soma check failed (fix the errors above, then verify)");
        }
        std::process::exit(1);
    }
    if all_results.is_empty() {
        // Nothing to prove is not a proof: say so, but still end with the
        // one verdict line every caller greps for. Properties in soma.toml
        // about a machine that does not exist are an error.
        let orphan_props = verify_config.as_ref().map(|cfg| {
            !cfg.eventually.is_empty() || !cfg.never.is_empty() || !cfg.always.is_empty()
                || !cfg.after.is_empty() || !cfg.before.is_empty()
        }).unwrap_or(false);
        if json {
            println!("{{\"state_machines\":[], \"temporal\":[], \"passed\": true, \"note\": \"no state machine: nothing beyond soma check was proven\"}}");
        } else {
            eprintln!("No state machine in this program: nothing to prove beyond `soma check` (invariants and lifecycles are what verify proves).");
            for (ok, line) in &horde_lines { println!("  {} cost: {}", if *ok { "✓" } else { "⚠" }, line); }
            for line in &cross_cell { eprintln!("note: {}", line); }
            // --strict: an unbounded horde is no proof
            if strict && horde_lines.iter().any(|(ok, _)| !ok) {
                println!("VERIFY FAILED — --strict: {} horde cost bound(s) unprovable", horde_lines.iter().filter(|(ok, _)| !ok).count());
                std::process::exit(1);
            }
            if orphan_props {
                eprintln!("note: the soma.toml beside this file declares [verify] properties; none applies here (no state machine)");
            }
            println!("VERIFY OK (vacuous — no state machine)");
        }
        return;
    }

    // properties declared, no machine to check them against: an error, not
    // "Temporal: 0 passed" (a renamed cell silently lost the gate)
    let declares_props = verify_config.as_ref().map(|cfg| {
        !cfg.eventually.is_empty() || !cfg.never.is_empty() || !cfg.always.is_empty()
            || !cfg.after.is_empty() || !cfg.before.is_empty()
    }).unwrap_or(false);
    // a shared soma.toml scoped with `cells = [...]` to a cell this file
    // does not contain is not an error (examples/ holds many programs)
    let targeted_present = verify_config.as_ref().map(|cfg| {
        cfg.cells.is_empty() || cfg.cells.iter().any(|c| all_cell_names.contains(c))
    }).unwrap_or(false);
    // the manifest's `entry` is another file of the directory that exists:
    // its properties belong to that program, and --strict on this one must
    // not fail for a gate that was never meant to apply here
    let entry_is_another_file = manifest.as_ref().zip(files.first()).map(|(m, first)| {
        let dir = first.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        let entry = dir.join(&m.package.entry);
        let same = |p: &PathBuf| std::fs::canonicalize(p).ok() == std::fs::canonicalize(&entry).ok();
        entry.exists() && !files.iter().any(|f| same(f))
    }).unwrap_or(false);
    // `cells = [...]` naming a cell: a near-miss of a cell of this file is a
    // typo (every property was silently skipped and verify said OK); a cell
    // of this file without a state machine cannot carry the properties
    if let Some(cfg) = verify_config.filter(|_| declares_props) {
        for c in &cfg.cells {
            if machine_cells.contains(c) { continue; }
            if let Some(real) = all_cell_names.iter().find(|n| n.eq_ignore_ascii_case(c) && *n != c) {
                unknown_states.push(format!("soma.toml [verify] cells names '{c}', which this file does not define (did you mean '{real}'? names are case-sensitive) — its properties were not checked"));
            } else if all_cell_names.contains(c) {
                unknown_states.push(format!("soma.toml [verify] cells names '{c}', which has no `state {{ }}` — its properties apply to nothing"));
            } else if let Some(near) = checker::names::suggest(c, all_cell_names.iter()) {
                unknown_states.push(format!("soma.toml [verify] cells names '{c}', which this file does not define (did you mean '{near}'?) — its properties were not checked"));
            } else if strict && !targeted_present && !entry_is_another_file {
                // --strict: properties that apply to NOTHING are no proof (a
                // renamed cell silently disabled every property)
                unknown_states.push(format!("soma.toml [verify] cells names '{c}', which this file does not define — none of its properties were checked"));
            } else if !json {
                eprintln!("note: soma.toml [verify] cells names '{c}', which this file does not define — its properties are not checked here");
            }
        }
    }
    // `[verify.before.paid] requires = []` promised nothing and passed
    if let Some(cfg) = verify_config {
        for (st, b) in &cfg.before {
            if b.requires.is_empty() && b.requires_all.is_empty() {
                unknown_states.push(format!("soma.toml [verify.before.{st}] lists no state (`requires = [\"approved\"]` or `requires_all = [...]`) — it checks nothing"));
            }
        }
        for (st, a) in &cfg.after {
            if a.eventually.is_empty() && a.never.is_empty() {
                unknown_states.push(format!("soma.toml [verify.after.{st}] lists no state (`eventually = [...]` or `never = [...]`) — it checks nothing"));
            }
        }
    }
    // …unless the FILE simply has no machine: a helper or migration cell
    // beside a project soma.toml is not a failed proof (it used to exit 1)
    if declares_props && all_temporal.is_empty() && targeted_present {
        if machine_cells.is_empty() {
            eprintln!("note: the soma.toml beside this file declares [verify] properties; this file has no `state {{ }}`, so none applies to it");
        } else {
            unknown_states.push("[verify] declares properties but no state machine received them (no `state { }` in the targeted cells — check `cells = [...]`)".to_string());
        }
    }
    unknown_states.sort();
    unknown_states.dedup();
    // --strict: a proof that degraded to "runtime-checked" (a removed
    // require, an unprovable termination) is a failure, not a ⚠ — a CI
    // gate on the exit code could not see the downgrade otherwise
    let strict_warnings: usize = if strict {
        all_results.iter().map(|r| r.checks.iter().filter(|c| matches!(c, checker::verify::VerifyCheck::Warning(_))).count()).sum::<usize>()
            + horde_lines.iter().filter(|(ok, _)| !ok).count()
    } else { 0 };
    let has_failures = all_results.iter().any(|r| r.has_failures())
        || all_temporal.iter().any(|(_, rs)| rs.iter().any(|r| !r.passed))
        || !unknown_states.is_empty()
        || check_failed
        || strict_warnings > 0;

    if json {
        // Machine-readable JSON output for agents
        let sm_results: Vec<serde_json::Value> = all_results.iter().map(|r| {
            let checks: Vec<serde_json::Value> = r.checks.iter().map(|c| {
                match c {
                    checker::verify::VerifyCheck::Pass(msg) => serde_json::json!({"status": "pass", "message": msg}),
                    checker::verify::VerifyCheck::Note(msg) => serde_json::json!({"status": "note", "message": msg}),
                    checker::verify::VerifyCheck::Warning(msg) => serde_json::json!({"status": "warning", "message": msg}),
                    checker::verify::VerifyCheck::Fail(msg, trace) => {
                        let mut v = serde_json::json!({"status": "fail", "message": msg});
                        if let Some(t) = trace { v["counter_example"] = serde_json::json!(t); }
                        v
                    }
                }
            }).collect();
            serde_json::json!({
                "name": r.machine_name,
                "states": r.states,
                "initial": r.initial,
                "terminal_states": r.terminal_states,
                "checks": checks,
            })
        }).collect();

        let temporal_results: Vec<serde_json::Value> = all_temporal.iter().map(|(name, results)| {
            let props: Vec<serde_json::Value> = results.iter().map(|r| {
                let mut v = serde_json::json!({
                    "property": r.property,
                    "passed": r.passed,
                    "message": r.message,
                });
                if let Some(ref ce) = r.counter_example {
                    v["counter_example"] = serde_json::json!(ce);
                }
                v
            }).collect();
            serde_json::json!({"state_machine": name, "properties": props})
        }).collect();

        let mut output = serde_json::json!({
            "passed": !has_failures,
            "state_machines": sm_results,
            "temporal": temporal_results,
            "hordes": horde_lines.iter().map(|(ok, l)| serde_json::json!({"status": if *ok { "pass" } else { "warning" }, "message": format!("cost: {}", l)})).collect::<Vec<_>>(),
        });
        if total_cells > 1 {
            output["note"] = serde_json::json!(
                "verification is per-cell. Cross-cell signal composition is not yet verified."
            );
        }
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        // Human-readable output
        print!("{}", checker::verify::format_results(&all_results));
        // each horde's cost bound, once
        for (ok, line) in &horde_lines { println!("  {} cost: {}", if *ok { "✓" } else { "⚠" }, line); }
        // what verify does NOT cover, named (it is per-cell)
        for line in &cross_cell { eprintln!("note: {}", line); }

        for (name, results) in &all_temporal {
            print!("{}", format_property_results(name, results));
        }

        let total_temporal = all_temporal.iter().map(|(_, rs)| rs.len()).sum::<usize>();
        let passed_temporal = all_temporal.iter()
            .flat_map(|(_, rs)| rs.iter())
            .filter(|r| r.passed)
            .count();
        let failed_temporal = total_temporal - passed_temporal;

        if let Some(cfg) = verify_config {
            // count PROPERTIES, the way they are reported below (a list of
            // states is one property), so "N loaded" matches the tally
            let user_props = (!cfg.eventually.is_empty()) as usize
                + cfg.never.len()
                + (!cfg.always.is_empty()) as usize
                + cfg.after.values().map(|a| (!a.eventually.is_empty()) as usize + a.never.len()).sum::<usize>()
                + cfg.before.values().map(|b| (!b.requires.is_empty()) as usize + b.requires_all.len()).sum::<usize>();
            if user_props > 0 && !targeted_present {
                // `cells = ["Nope"]`: loaded, and applied to nothing here
                eprintln!("soma.toml: {} user-defined properties loaded — NONE applies to this file ([verify] cells = {:?} names no cell of it)", user_props, cfg.cells);
            } else if user_props > 0 {
                eprintln!("soma.toml: {} user-defined properties loaded", user_props);
            }
        }

        eprintln!("Temporal: {} passed, {} failed", passed_temporal, failed_temporal);

        if total_cells > 1 {
            eprintln!("note: verification is per-cell. Cross-cell signal composition is not yet verified.");
        }
    }

    if !json {
        for u in &unknown_states {
            if u.starts_with("soma.toml") { eprintln!("error: {}", u); }
            else { eprintln!("error: soma.toml: {} — a property about a state that does not exist proves nothing", u); }
        }
        // ONE verdict, last, so nobody stops reading at an early "0 failures"
        let structural = all_results.iter().filter(|r| r.has_failures()).count();
        let temporal = all_temporal.iter().flat_map(|(_, rs)| rs.iter()).filter(|r| !r.passed).count();
        if has_failures {
            let mut why: Vec<String> = Vec::new();
            if check_failed { why.push("soma check failed".to_string()); }
            if structural > 0 { why.push(format!("{} cell{} with failed checks — state machine or invariants (see ✗ lines)", structural, if structural == 1 { "" } else { "s" })); }
            if temporal > 0 { why.push(format!("{} temporal propert{} failed", temporal, if temporal == 1 { "y" } else { "ies" })); }
            let bad_cells = unknown_states.iter().filter(|u| u.starts_with("soma.toml [verify] cells")).count();
            let bad_before = unknown_states.iter().filter(|u| u.starts_with("soma.toml [verify.before")).count();
            let unknown = unknown_states.len() - bad_cells - bad_before;
            if bad_before > 0 { why.push("a [verify.before] entry lists no state".to_string()); }
            if unknown > 0 { why.push(format!("{} propert{} on unknown states", unknown, if unknown == 1 { "y" } else { "ies" })); }
            if bad_cells > 0 { why.push("soma.toml [verify] cells names no state machine of this file".to_string()); }
            if strict_warnings > 0 {
                why.push(format!("--strict: {} ⚠ line{} (runtime-checked or unprovable)", strict_warnings, if strict_warnings == 1 { "" } else { "s" }));
                // the ⚠ lines sit among dozens of ✓: repeat them by the verdict
                println!("--strict failures:");
                for r in &all_results {
                    for c in &r.checks {
                        if let checker::verify::VerifyCheck::Warning(m) = c { println!("  ⚠ {}", m); }
                    }
                }
            }
            println!("VERIFY FAILED — {}", why.join("; "));
        } else {
            println!("VERIFY OK");
        }
    }

    if has_failures {
        std::process::exit(1);
    }
}

// Small commands kept in main.rs — not worth a separate file
fn cmd_ast(path: &PathBuf) {
    let source = commands::read_source(path);
    let tokens = commands::lex(&source);
    let program = commands::parse(tokens);
    println!("{:#?}", program);
}

fn cmd_tokens(path: &PathBuf) {
    let source = commands::read_source(path);
    let tokens = commands::lex(&source);
    for tok in &tokens {
        println!("{:?}  @ {:?}", tok.token, tok.span);
    }
}

/// The tables of `db` owned by the cells of `file` (and of its lib/ and
/// installed packages), and the cell prefixes of the other tables.
fn cells_in_db(file: &std::path::Path, db: &std::path::Path) -> (Vec<String>, Vec<String>) {
    // the EXACT tables this program owns (a name prefix took cell `A_b`'s
    // tables for cell `A`'s and reset another program's data)
    let mut names: Vec<String> = Vec::new();
    let add_file = |p: &std::path::Path, names: &mut Vec<String>| {
        if let Ok(src) = std::fs::read_to_string(p) {
            let program = commands::parse(commands::lex(&src));
            for c in &program.cells {
                let cell = &c.node.name;
                names.push(format!("{}__counters", cell).to_ascii_lowercase());
                names.push(format!("{}__agent_memory", cell).to_ascii_lowercase());
                names.push(format!("{}__horde-meta", cell).to_ascii_lowercase());
                names.push(format!("{}__horde-tasks", cell).to_ascii_lowercase());
                for sec in &c.node.sections {
                    match &sec.node {
                        ast::Section::Memory(m) => for slot in &m.slots { names.push(format!("{}_{}", cell, slot.node.name).to_ascii_lowercase()); },
                        ast::Section::State(sm) => names.push(format!("{}__sm_{}", cell, sm.name).to_ascii_lowercase()),
                        _ => {}
                    }
                }
            }
        }
    };
    add_file(file, &mut names);
    let base = file.parent().unwrap_or(std::path::Path::new("."));
    fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        if let Ok(es) = std::fs::read_dir(d) {
            for e in es.flatten() {
                let p = e.path();
                if p.is_dir() { walk(&p, out); } else if p.extension().map_or(false, |x| x == "cell") { out.push(p); }
            }
        }
    }
    let mut deps = Vec::new();
    walk(&base.join("lib"), &mut deps);
    walk(&base.join(".soma_env").join("packages"), &mut deps);
    walk(&base.join("packages"), &mut deps);
    for d in deps { add_file(&d, &mut names); }
    let Ok(conn) = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else { return (Vec::new(), Vec::new()) };
    let Ok(mut st) = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'") else { return (Vec::new(), Vec::new()) };
    let tables: Vec<String> = st.query_map([], |r| r.get::<_, String>(0)).map(|it| it.flatten().collect()).unwrap_or_default();
    let mut foreign: Vec<String> = Vec::new();
    let mut own: Vec<String> = Vec::new();
    for t in tables {
        let lt = t.to_ascii_lowercase();
        if lt.starts_with('_') || lt.starts_with("sqlite_") || !lt.contains('_') { continue; }
        let base = lt.strip_suffix("_log").unwrap_or(&lt);
        if names.iter().any(|n| n == &lt || n == base) { own.push(t); continue; }
        let name = t.strip_suffix("_log").unwrap_or(&t).to_string();
        if !foreign.contains(&name) { foreign.push(name); }
    }
    (own, foreign)
}


/// `soma test lib/` (also check): every .cell file under the directory, one
/// after another — a project-wide gate used to need a shell loop.
fn run_over_dir(cmd: &str, dir: &std::path::Path, json: bool) -> ! {
    fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(d) else { return };
        let mut es: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        es.sort();
        for p in es {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            // not the installed packages, not the data directory, no dotfiles
            if name.starts_with('.') || name == "node_modules" { continue; }
            if p.is_dir() { walk(&p, out); } else if p.extension().is_some_and(|x| x == "cell") { out.push(p); }
        }
    }
    let mut files = Vec::new();
    walk(dir, &mut files);
    if files.is_empty() {
        eprintln!("no .cell file under {}", dir.display());
        std::process::exit(1);
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("soma"));
    let (mut failed, total) = (0usize, files.len());
    for f in &files {
        eprintln!("── {} ──", f.display());
        let mut c = std::process::Command::new(&exe);
        c.arg(cmd).arg(f);
        if json { c.arg("--json"); }
        let ok = c.status().map(|s| s.success()).unwrap_or(false);
        if !ok { failed += 1; }
    }
    eprintln!("{}: {} file(s), {} failed", cmd, total, failed);
    std::process::exit(if failed > 0 { 1 } else { 0 });
}
