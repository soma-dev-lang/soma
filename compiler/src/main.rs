#![allow(dead_code, unused_imports, unused_variables)]

mod ast;
mod checker;
mod codegen;
mod commands;
mod external;
mod interpreter;
mod lexer;
mod parser;
mod pkg;
mod project;
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
        /// Migrate the program to an edition: the mechanical rewrites, soma.toml, then what is left to decide (`--edition 2027`)
        #[arg(long, value_name = "EDITION")]
        edition: Option<String>,
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
        /// Only what needs attention: every ✗ and ⚠ under its heading, the count of ✓, the holes, the verdict
        #[arg(long)]
        summary: bool,
        /// Write the proof as an artifact: sources and toolchain hashed, every property with its verdict class, one digest
        #[arg(long, value_name = "OUT.json")]
        attest: Option<PathBuf>,
        /// Re-run the verification an attestation records and compare digests
        #[arg(long, value_name = "ATTESTATION.json")]
        check_attestation: Option<PathBuf>,
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
        /// Ask the model again instead of replaying its recorded answers
        #[arg(long)]
        live: bool,
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
    /// Print a program as its skeleton: memory, state machines, and each signal's frame
    /// (reads / writes / moves / calls) — no handler bodies
    Skeleton {
        /// Path to the .cell source file
        file: PathBuf,
        /// `Cell.signal` (or `signal`): the brief for that one handler. A cell's name
        /// (`Orders`): that cell alone, with its comments and who calls it
        signal: Option<String>,
        /// Output as JSON (for agents)
        #[arg(long)]
        json: bool,
        /// One line per cell (signals, holes, machine, what it is for) and who calls whom:
        /// the way into a large program
        #[arg(long)]
        index: bool,
        /// Rewrite the file as `skeleton` + `flesh`: the structure with the frames its
        /// bodies have today, and the bodies unchanged (to stdout)
        #[arg(long)]
        split: bool,
        /// Write the skeletons to skeleton.lock: `soma check` then refuses any change to them
        #[arg(long)]
        freeze: bool,
        /// Accept the current skeletons as the new skeleton.lock, printing what changed
        #[arg(long)]
        amend: bool,
    },
    /// Where a signal is declared, where its body is, and every call of it — program and tests — as file:line
    Refs {
        /// Path to the program's entry file
        file: PathBuf,
        /// `Cell.signal` (or `signal`)
        signal: String,
        /// Output as JSON (for agents)
        #[arg(long)]
        json: bool,
    },
    /// Rename a signal everywhere it is written — declaration, body, calls, frames, tests — under the entry's directory
    Rename {
        /// Path to the program's entry file
        file: PathBuf,
        /// `Cell.signal`
        signal: String,
        /// The new name
        new_name: String,
        /// Print the lines that would change, write nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// List the holes of a program's skeletons — signals with no body yet — in the order to fill them
    Holes {
        /// Path to the .cell source file
        file: PathBuf,
        /// A cell (`Orders`) or one signal (`Orders.place`): only those
        only: Option<String>,
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
    /// Format .cell files to the canonical layout: soma fmt app.cell | soma fmt . --check
    Fmt {
        /// Files or directories (every .cell under a directory)
        paths: Vec<PathBuf>,
        /// List the files that are not canonical and exit 1 — writes nothing (for CI)
        #[arg(long)]
        check: bool,
        /// Output as JSON (for agents)
        #[arg(long)]
        json: bool,
    },
    /// Grade a handler on a dataset: soma eval app.cell --handler classify --dataset cases.jsonl
    Eval {
        /// Path to the .cell source file
        file: PathBuf,
        /// The handler to call with each case's input (`classify`, or `Cell.classify`)
        #[arg(long)]
        handler: String,
        /// JSON lines: {"input": …, "expected": …, "name"?: …} — input is the argument (a list: the arguments)
        #[arg(long)]
        dataset: PathBuf,
        /// exact | contains | handler:Cell.judge (called as judge(input, answer, expected) -> Bool)
        #[arg(long, default_value = "exact")]
        grade: String,
        /// The pass rate the run must reach (0 to 1)
        #[arg(long, default_value_t = 1.0)]
        threshold: f64,
        /// Output as JSON (for agents)
        #[arg(long)]
        json: bool,
    },
    /// Language server over stdio: diagnostics on open / save, formatting
    Lsp,
    /// Explain a diagnostic code: soma explain E0028 (omit the code to list them)
    Explain {
        /// A code from a diagnostic (E0028, W0014); omit to list every code
        #[arg(default_value = "")]
        code: String,
        /// Output as JSON (for agents)
        #[arg(long)]
        json: bool,
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
        /// Start skeleton-first: app.cell holds the structure (`skeleton`), lib/ the bodies (`flesh`)
        #[arg(long)]
        skeleton: bool,
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
    /// Serve a program to a host process over stdin / stdout, one JSON object per line (the Python and TypeScript packages use it)
    Embed {
        /// The program
        file: PathBuf,
    },
    /// External tools (`tool … [mcp: "server"]` / `[openapi: "server/op"]`): pin their contracts in soma.lock
    Tools {
        /// `sync` asks each server and writes soma.lock; `list` shows the bound tools and their pins
        action: String,
        /// The program (default: app.cell)
        file: Option<PathBuf>,
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
            if file.is_dir() { run_over_dir("check", &file, json, &[]); }
            commands::check::cmd_check(&file, json, &mut registry)
        }
        Commands::Lint { file, json } => {
            *commands::JSON_SCHEMA.lock().unwrap_or_else(|e| e.into_inner()) = "lint";
            commands::lint::cmd_lint(&file, json)
        }
        Commands::Fix { file, json, native_idiv, edition } => {
            *commands::JSON_SCHEMA.lock().unwrap_or_else(|e| e.into_inner()) = "fix";
            if let Some(edition) = edition {
                commands::fix::cmd_fix_edition(&file, &edition, json, &mut registry);
            } else if native_idiv {
                commands::fix::cmd_fix_native_idiv(&file, json);
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
                    if data.exists() {
                        let (own, mut foreign, kept) = cells_in_db(&file, &data.join("soma.db"));
                        // the JSON slot files (interior cells' [persistent]
                        // slots, `Sys.A_cnt.json`): this program's are reset
                        // with its tables — they survived a partial reset
                        let (own_files, foreign_files) = slot_files_in(&file, &data);
                        foreign.extend(foreign_files);
                        if !foreign.is_empty() || !kept.is_empty() {
                            if let Ok(conn) = rusqlite::Connection::open(data.join("soma.db")) {
                                for t in &own { let _ = conn.execute_batch(&format!("DROP TABLE IF EXISTS \"{}\"", t.replace('"', "\"\""))); }
                            }
                            for f in &own_files { let _ = std::fs::remove_file(f); }
                            let own: Vec<String> = own.iter().cloned().chain(own_files.iter().filter_map(|f| f.file_name().map(|n| n.to_string_lossy().to_string()))).collect();
                            if foreign.is_empty() {
                                eprintln!("fresh: reset {} table(s) of this program", own.len());
                            } else {
                                eprintln!("fresh: reset {} table(s) of this program — {} also holds tables {} (other programs in this directory), kept",
                                    own.len(), data.display(), foreign.join(", "));
                            }
                            if !kept.is_empty() {
                                eprintln!("fresh: kept {} too — data of a cell another program here also declares (its shared slots, next_id() counters, remember() memory)",
                                    kept.join(", "));
                            }
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
                commands::serve::cmd_serve_watch(&file, &mut registry);
            } else {
                if no_schedule { commands::serve::NO_SCHEDULE.store(true, std::sync::atomic::Ordering::Relaxed); }
                commands::serve::cmd_serve(&file, port, &host, verbose, join.as_deref(), no_check, &mut registry);
            }
        }
        Commands::Test { file, json } => {
            if file.is_dir() { run_over_dir("test", &file, json, &[]); }
            commands::test_cmd::cmd_test(&file, json, &mut registry)
        }
        Commands::Replay { file, log, at, live } => commands::replay::cmd_replay(&file, log.as_ref(), at.as_deref(), live, &mut registry),
        Commands::Init { name, skeleton } => commands::init::cmd_init(name.as_deref(), skeleton),
        Commands::Add { package, version, git, path } => commands::init::cmd_add(&package, version.as_deref(), git.as_deref(), path.as_deref()),
        Commands::Embed { file } => {
            runtime::storage::set_data_dir_beside(&file);
            commands::embed::cmd_embed(&file, &mut registry)
        }
        Commands::Tools { action, file } => {
            let file = file.unwrap_or_else(|| PathBuf::from("app.cell"));
            match action.as_str() {
                "sync" => commands::tools::cmd_sync(&file),
                "list" => commands::tools::cmd_list(&file),
                other => { eprintln!("soma tools: `{}` is not an action — `soma tools sync [file]` or `soma tools list [file]`", other); std::process::exit(2); }
            }
        }
        Commands::Install => commands::init::cmd_install(),
        Commands::Env => commands::init::cmd_env(),
        Commands::Repl => commands::repl::cmd_repl(&mut registry),
        Commands::AddProvider { name } => commands::provider::cmd_add_provider(&name),
        Commands::TestProvider { name } => commands::provider::cmd_test_provider(&name),
        Commands::Migrate { from, to } => commands::provider::cmd_migrate(&from, &to),
        Commands::Props => commands::props::cmd_props(&registry),
        Commands::Verify { files, json, strict, summary, attest, check_attestation } => {
            if summary && !json { verify_summary(&files, strict); }
            if let Some(a) = check_attestation {
                commands::attest::check(&a);
            }
            if let Some(out) = attest {
                if files.is_empty() {
                    eprintln!("error: `soma verify --attest out.json app.cell` needs the program to verify");
                    std::process::exit(1);
                }
                commands::attest::write(&files, strict, &out);
            }
            // a directory, as check and test take one (it answered "cannot
            // read '.': Is a directory")
            if files.len() == 1 && files[0].is_dir() {
                run_over_dir("verify", &files[0], json, if strict { &["--strict"] } else { &[] });
            }
            cmd_verify(&files, json, strict)
        }
        Commands::Deploy { file, target, region } => commands::deploy::cmd_deploy(&file, &target, region.as_deref()),
        Commands::Describe { file, builtins, faces, json } => {
            if builtins {
                commands::describe::cmd_describe_builtins(json);
            } else if let Some(ref file) = file {
                if faces {
                    commands::describe::cmd_describe_faces(file, json);
                } else {
                    *commands::JSON_SCHEMA.lock().unwrap_or_else(|e| e.into_inner()) = "describe";
                    *commands::JSON_FILE.lock().unwrap_or_else(|e| e.into_inner()) = file.display().to_string();
                    commands::describe::cmd_describe(file);
                }
            } else {
                eprintln!("error: missing file argument.\n  soma describe <file.cell>            full structure as JSON\n  soma describe <file.cell> --faces    contracts only (token-cheap)\n  soma describe --builtins [--json]    builtin signature table");
                std::process::exit(1);
            }
        }
        Commands::Skeleton { file, signal, json, index, split, freeze, amend } => {
            if freeze || amend { commands::skeleton::cmd_freeze(&file, amend) }
            else if index { commands::skeleton::cmd_index(&file) }
            else { commands::skeleton::cmd_skeleton(&file, signal.as_deref(), json, split) }
        }
        Commands::Rename { file, signal, new_name, dry_run } => commands::refs::cmd_rename(&file, &signal, &new_name, dry_run),
        Commands::Refs { file, signal, json } => commands::refs::cmd_refs(&file, &signal, json),
        Commands::Holes { file, only, json } => commands::skeleton::cmd_holes(&file, only.as_deref(), json),
        Commands::Docs { topic } => commands::docs::cmd_docs(&topic),
        Commands::Fmt { paths, check, json } => commands::fmt::cmd_fmt(&paths, check, json),
        Commands::Eval { file, handler, dataset, grade, threshold, json } => commands::eval::cmd_eval(&file, &handler, &dataset, &grade, threshold, json),
        Commands::Lsp => commands::lsp::cmd_lsp(),
        Commands::Explain { code, json } => commands::explain::cmd_explain(&code, json),
        Commands::Example { terms, json, all } => commands::example::cmd_example(&terms, json, all),
    }
}

/// `soma verify --summary`: the same verification, and of its report only
/// what a reader must act on. (The full report of a program of a dozen
/// cells runs to hundreds of lines, nearly all of them `✓`.)
fn verify_summary(files: &[PathBuf], strict: bool) -> ! {
    let exe = std::env::current_exe().unwrap_or_else(|e| { eprintln!("error: cannot locate the soma binary: {e}"); std::process::exit(1) });
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("verify").args(files);
    if strict { cmd.arg("--strict"); }
    let out = cmd.output().unwrap_or_else(|e| { eprintln!("error: cannot run soma verify: {e}"); std::process::exit(1) });
    let text = format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
    let (mut proven, mut noted, mut waiting) = (0usize, 0usize, 0usize);
    let mut heading = String::new();
    let mut printed_heading = String::new();
    let mut tail: Vec<&str> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('✓') { proven += 1; }
        else if t.starts_with('·') { noted += 1; }
        // a hole's own contracts: true of every hole, said once
        else if t.starts_with('⚠') && (t.contains("not proven for `?`") || (t.contains("runtime-checked at") && t.contains("(?"))) { waiting += 1; }
        else if t.starts_with('✗') || t.starts_with('⚠') {
            if heading != printed_heading {
                println!("{}", heading);
                printed_heading = heading.clone();
            }
            println!("{}", line);
        }
        else if t.starts_with('○') || t.starts_with("VERIFY") || t.starts_with("error") || t.contains("fails `soma check`") { tail.push(line); }
        // a counter-example trace under a ✗, a location under an error
        else if line.starts_with("      ") && !printed_heading.is_empty() && heading == printed_heading { println!("{}", line); }
        else if !t.is_empty() && !line.starts_with(' ') && (t.starts_with("State machine") || t.ends_with(':')) { heading = line.to_string(); }
    }
    println!("{} proven (✓), {} note{} (·) — `soma verify` without --summary prints them", proven, noted, if noted == 1 { "" } else { "s" });
    if waiting > 0 {
        println!("○ {} contract{} of holes wait{} for a body (an `ensures` of a hole, a `requires` a hole must meet when it calls) — judged as the holes are filled", waiting, if waiting == 1 { "" } else { "s" }, if waiting == 1 { "s" } else { "" });
    }
    for l in tail { println!("{}", l); }
    std::process::exit(out.status.code().unwrap_or(1));
}

fn cmd_verify(files: &[PathBuf], json: bool, strict: bool) {
    use checker::temporal::*;
    commands::VERIFY_MODE.store(true, std::sync::atomic::Ordering::Relaxed);

    let mut all_results = Vec::new();
    // a skeleton's unfilled signals, per file: open obligations
    let mut hole_notes: Vec<String> = Vec::new();
    let mut all_cell_names: Vec<String> = Vec::new();
    // horde cost bounds, printed when no state machine gives verify more to say
    let mut horde_lines: Vec<(bool, String)> = Vec::new();
    let mut cross_cell: Vec<String> = Vec::new();
    let mut machine_cells: Vec<String> = Vec::new();
    let mut all_temporal = Vec::new();
    let mut total_cells: usize = 0;

    // Try to read soma.toml for user-defined properties
    let manifest = files.first()
        .map(|f| project::manifest(f))
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read_to_string(&p).ok())
        .and_then(|content| toml::from_str::<pkg::manifest::Manifest>(&content).ok());

    let verify_config = manifest.as_ref().map(|m| &m.verify);

    let mut unknown_states: Vec<String> = Vec::new();
    let check_failed = false;

    for path in files {
        // (the edition decides what `get_status` answers for an id never moved)
        interpreter::set_edition_beside(path);
        let source = commands::read_source(path);
        let file_str = path.display().to_string();
        let tokens = commands::lex_with_location(&source, Some(&file_str));
        let mut program = commands::parse_with_location(tokens, Some(&source), Some(&file_str));
        commands::resolve_imports(&mut program, path);
        hole_notes.extend(checker::flesh::holes_note(&program, &file_str));
        // from here on a hole is read as its frame: whatever is proven
        // holds for every flesh that conforms
        let checked = program.clone();
        checker::flesh::model_holes(&mut program);
        checker::runtime_section::as_handler(&mut program);

        // A proof about a program that does not pass `soma check` is a proof
        // about nothing: run the static gates first.
        {
            let mut registry = registry::Registry::new();
            commands::load_meta_cells_from_program(&checked, &mut registry, path);
            let mut chk = checker::Checker::new(&registry);
            chk.source = Some((file_str.clone(), source.clone()));
            chk.check(&checked);
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
                    let mut doc = commands::verify_json(false, "VERIFY FAILED — soma check failed");
                    doc["check_errors"] = serde_json::json!(errors);
                    println!("{}", doc);
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
            // (not for a cell whose machine soma.toml already ties to another's:
            // that rule is stated, and judged under `product:`)
            let tied: Vec<String> = verify_config.map(|cfg| cfg.before.iter().filter(|(t, b)| product_property(t, b))
                .filter_map(|(t, _)| t.split_once('.').map(|(c, _)| format!("cross-cell: `{}.", c))).collect()).unwrap_or_default();
            cross_cell.extend(checker::verify::cross_cell_notes(&program).into_iter().filter(|n| !tied.iter().any(|t| n.starts_with(t))));
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
                            // 3.0: `Cell.state` names are a property of the
                            // PRODUCT of the machines; one under `assuming`
                            // is checked on this machine without the edges
                            // the assumption rules out (both below)
                            if product_property(target, before_cfg) || before_cfg.assuming.is_some() { continue; }
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
                                if product_property(target, b) { continue; }
                                named.push(("[verify.before.<state>]", target));
                                named.extend(b.requires.iter().map(|s| ("before … requires", s)));
                                named.extend(b.requires_all.iter().map(|s| ("before … requires_all", s)));
                            }
                            // with several machines in scope a state may belong
                            // to another one: only complain when NO machine of
                            // the verified files declares it
                            for (what, state) in named {
                                if !graph.states.contains(state) && !all_states.contains(state.as_str()) {
                                    // `Cell.state` is the form of a rule between machines
                                    let qualified = state.split_once('.').is_some_and(|(_, st)| all_states.contains(st) || graph.states.iter().any(|x| x == st));
                                    unknown_states.push(format!("{} names state '{}', which no verified state machine declares{}{}", what, state,
                                        if qualified { " — `Cell.state` is read by `[verify.before.\"Cell.state\"] requires = [...]` only (a rule between two machines); `after`, `never`, `eventually` and `always` are judged per machine: write the bare state name" } else { "" },
                                        if targeted.is_empty() { "" } else { " (cells = [...] in soma.toml limits verification to those cells)" }));
                                }
                            }
                        }
                    }

                    let mut results: Vec<PropertyResult> = props.iter()
                        .map(|p| check_property(&graph, p))
                        .collect();
                    // 3.0: `assuming = "amount > 500"` — the same precedence,
                    // on the machine without the edges whose guard cannot
                    // hold then
                    if let Some(cfg) = verify_config.filter(|_| applies) {
                        for (target, b) in &cfg.before {
                            let Some(text) = b.assuming.as_ref().filter(|_| !product_property(target, b)) else { continue };
                            if !graph.states.contains(target) { continue; }
                            match checker::product::parse_assumption(text) {
                                Err(e) => results.push(PropertyResult { property: format!("before {} assuming {}", target, text), passed: false, message: e, counter_example: None }),
                                Ok(assumption) => {
                                    let (narrowed, gone) = checker::product::machine_assuming(sm, &assumption);
                                    let g2 = StateMachineGraph::from_ast(&narrowed);
                                    let mut reqs: Vec<Vec<String>> = Vec::new();
                                    if !b.requires.is_empty() { reqs.push(b.requires.clone()); }
                                    reqs.extend(b.requires_all.iter().map(|s| vec![s.clone()]));
                                    for req in reqs {
                                        let mut r = check_property(&g2, &Property::Requires(target.clone(), req));
                                        r.property = format!("{}, assuming {}", r.property, text);
                                        r.message = format!("{} — assuming `{}`: {}", r.message, text,
                                            if gone.is_empty() { "no guard of this machine is ruled out by it (the property is the unconditional one)".to_string() }
                                            else { format!("the edge(s) whose guard cannot hold then are left out ({})", gone.join(", ")) });
                                        results.push(r);
                                    }
                                }
                            }
                        }
                    }

                    let machine_cells = program.cells.iter().filter(|c| c.node.sections.iter().any(|s| matches!(s.node, ast::Section::State(_)))).count();
                    let label = if machine_cells > 1 { format!("{}.{}", cell.node.name, sm.name) } else { sm.name.clone() };
                    all_temporal.push((label, results));
                }
            }
        }
    }

    // 3.0 (B2): properties over the product of the machines
    if let Some(cfg) = verify_config {
        let specs: Vec<checker::product::Spec> = cfg.before.iter().filter(|(t, b)| product_property(t, b)).flat_map(|(target, b)| {
            let mut out = Vec::new();
            if !b.requires.is_empty() { out.push(checker::product::Spec { target: target.clone(), requires: b.requires.clone(), all: false, assuming: b.assuming.clone(), via: b.via.clone(), now: false }); }
            if !b.requires_all.is_empty() { out.push(checker::product::Spec { target: target.clone(), requires: b.requires_all.clone(), all: true, assuming: b.assuming.clone(), via: b.via.clone(), now: false }); }
            if !b.while_in.is_empty() { out.push(checker::product::Spec { target: target.clone(), requires: b.while_in.clone(), all: false, assuming: b.assuming.clone(), via: b.via.clone(), now: true }); }
            out
        }).collect();
        if !specs.is_empty() {
            for path in files {
                let source = commands::read_source(path);
                let file_str = path.display().to_string();
                let tokens = commands::lex_with_location(&source, Some(&file_str));
                let mut program = commands::parse_with_location(tokens, Some(&source), Some(&file_str));
                commands::resolve_imports(&mut program, path);
                // a hole is read as its frame. A property a hole's frame can
                // break, and the bodies written so far do not, is an open
                // obligation: neither proven nor refuted until the hole is filled
                let written = program.clone();
                let has_holes = !checker::flesh::holes(&program).is_empty();
                checker::flesh::model_holes(&mut program);
        checker::runtime_section::as_handler(&mut program);
                let mut modeled = checker::product::check(&program, &specs);
                if has_holes {
                    let plain = checker::product::check(&written, &specs);
                    for (m, p) in modeled.iter_mut().zip(plain.iter()) {
                        if m.checks.len() != p.checks.len() { continue; }
                        for (mc, pc) in m.checks.iter_mut().zip(p.checks.iter()) {
                            if let (checker::verify::VerifyCheck::Fail(msg, steps), checker::verify::VerifyCheck::Pass(_)) = (&*mc, pc) {
                                // the run, in a line: the last call is the one to write (or to frame) so that it cannot
                                let run = steps.as_ref().filter(|s| !s.is_empty()).map(|s| format!(" Run: {}.", s.iter().map(|x| x.split("  ←").next().unwrap_or(x).trim().to_string()).collect::<Vec<_>>().join(" → "))).unwrap_or_default();
                                *mc = checker::verify::VerifyCheck::Warning(format!(
                                    "open obligation — {} — the bodies written so far respect it; a hole's frame allows this run.{} It is proven when the holes on the run have bodies that rule it out, or when their frames no longer allow it", msg, run));
                            }
                        }
                    }
                }
                all_results.extend(modeled);
            }
        }
    }

    if all_results.is_empty() && check_failed {
        // still ONE verdict line, the one every caller greps for
        if json {
            println!("{}", commands::verify_json(false, "VERIFY FAILED — soma check failed"));
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
            // --strict: an unbounded horde is no proof, under --json too
            let unbounded = horde_lines.iter().filter(|(ok, _)| !ok).count();
            let ok = !(strict && unbounded > 0);
            let verdict = if ok { "VERIFY OK (vacuous — no state machine)".to_string() } else { format!("VERIFY FAILED — --strict: {} horde cost bound(s) unprovable", unbounded) };
            let mut doc = commands::verify_json(ok, &verdict);
            doc["note"] = serde_json::json!("no state machine: nothing beyond soma check was proven");
            doc["hordes"] = serde_json::json!(horde_lines.iter().map(|(ok, l)| serde_json::json!({"status": if *ok { "pass" } else { "warning" }, "message": format!("cost: {}", l)})).collect::<Vec<_>>());
            doc["cells"] = serde_json::json!(all_cell_names);
            println!("{}", doc);
            if !ok { std::process::exit(1); }
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
            for n in &hole_notes { println!("{}", n); }
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
            if b.requires.is_empty() && b.requires_all.is_empty() && b.while_in.is_empty() {
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

        let mut output = commands::verify_json(!has_failures, if has_failures { "VERIFY FAILED" } else { "VERIFY OK" });
        output["state_machines"] = serde_json::json!(sm_results);
        output["temporal"] = serde_json::json!(temporal_results);
        output["hordes"] = serde_json::json!(horde_lines.iter().map(|(ok, l)| serde_json::json!({"status": if *ok { "pass" } else { "warning" }, "message": format!("cost: {}", l)})).collect::<Vec<_>>());
        output["errors"] = serde_json::json!(unknown_states.iter().map(|u| serde_json::json!({"level": "error", "kind": "verify", "message": u})).collect::<Vec<_>>());
        output["cells"] = serde_json::json!(all_cell_names);
        if !hole_notes.is_empty() { output["holes"] = serde_json::json!(hole_notes); }
        if total_cells > 1 {
            output["note"] = serde_json::json!(
                "state machines are verified per cell. A value crosses a cell boundary by a face contract (`requires` / `ensures` on the signal); a rule between two cells' machines is proven when soma.toml states it — `[verify.before.\"Pay.paid\"] requires = [\"Approval.signed_off\"]` explores the product of the machines."
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
            eprintln!("note: state machines are verified per cell. A value crosses a cell boundary by a face contract (`requires` / `ensures` on the signal); a rule between two cells' machines is proven when soma.toml states it — `[verify.before.\"Pay.paid\"] requires = [\"Approval.signed_off\"]` explores the product of the machines.");
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
            for n in &hole_notes { println!("{}", n); }
            println!("VERIFY FAILED — {}", why.join("; "));
        } else if hole_notes.is_empty() {
            println!("VERIFY OK");
        } else {
            // the structure is proven; the bodies that are missing are not
            for n in &hole_notes { println!("{}", n); }
            println!("VERIFY OK (skeleton — the holes above are open obligations)");
        }
    }

    if has_failures {
        std::process::exit(1);
    }
}

// Small commands kept in main.rs — not worth a separate file
/// A `[verify.before.…]` entry that names a state as `Cell.state`: it is
/// about several machines at once (checker/product.rs).
fn product_property(target: &str, b: &pkg::manifest::BeforeConfig) -> bool {
    target.contains('.') || !b.while_in.is_empty() || b.via.is_some() || b.requires.iter().chain(b.requires_all.iter()).any(|s| s.contains('.'))
}

fn cmd_ast(path: &PathBuf) {
    let source = commands::read_source(path);
    let tokens = commands::lex(&source);
    let program = commands::parse(tokens);
    // (SOMA_AST_QUIET: only whether it parses — the grammar tests ask for 1,400 files)
    if std::env::var("SOMA_AST_QUIET").is_ok() { return; }
    println!("{:#?}", program);
}

fn cmd_tokens(path: &PathBuf) {
    let source = commands::read_source(path);
    let tokens = commands::lex(&source);
    for tok in &tokens {
        println!("{:?}  @ {:?}", tok.token, tok.span);
    }
}

/// The JSON slot files (`{cell path}_{slot}.json`) in the data directory
/// `data`: those of the cells of `file` (interior cells included, by their
/// qualified path), and the others — which belong to other programs beside it.
fn slot_files_in(file: &std::path::Path, data: &std::path::Path) -> (Vec<std::path::PathBuf>, Vec<String>) {
    fn files_of(p: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let Some(program) = std::fs::read_to_string(p).ok()
            .and_then(|src| lexer::Lexer::new(&src).tokenize().ok())
            .and_then(|t| parser::Parser::new(t).parse_program().ok()) else { return names };
        fn rec(c: &ast::CellDef, path: &str, names: &mut Vec<String>) {
            let cell = if path.is_empty() { c.name.clone() } else { format!("{}.{}", path, c.name) };
            for sec in &c.sections {
                match &sec.node {
                    ast::Section::Memory(m) => for slot in &m.slots { names.push(format!("{}_{}.json", cell, slot.node.name)); },
                    ast::Section::Interior(i) => for ch in &i.cells { rec(&ch.node, &cell, names); },
                    _ => {}
                }
            }
        }
        for c in &program.cells { rec(&c.node, "", &mut names); }
        names
    }
    let names = files_of(file);
    // a file another program beside this one declares too is its data as well
    let mut sibling: Vec<String> = Vec::new();
    let base = file.parent().filter(|b| !b.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    let me = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if let Ok(es) = std::fs::read_dir(base) {
        for e in es.flatten() {
            let p = e.path();
            if p.extension().map_or(true, |x| x != "cell") || p.canonicalize().map_or(false, |c| c == me) { continue; }
            sibling.extend(files_of(&p));
        }
    }
    let (mut own, mut foreign) = (Vec::new(), Vec::new());
    if let Ok(es) = std::fs::read_dir(data) {
        for e in es.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if !n.ends_with(".json") { continue; }
            if names.contains(&n) && !sibling.contains(&n) { own.push(e.path()); } else { foreign.push(n); }
        }
    }
    (own, foreign)
}

/// The tables of `db` owned by the cells of `file` (and of its lib/ and
/// installed packages), and the cell prefixes of the other tables.
fn cells_in_db(file: &std::path::Path, db: &std::path::Path) -> (Vec<String>, Vec<String>, Vec<String>) {
    // the EXACT tables a program owns (a name prefix took cell `A_b`'s
    // tables for cell `A`'s and reset another program's data), and its cell
    // names — parsed, never scanned with a regex (a `cell X` inside a
    // multi-line string counted as a cell). None: the file does not parse.
    fn tables_of(p: &std::path::Path) -> Option<(Vec<String>, Vec<String>)> {
        let src = std::fs::read_to_string(p).ok()?;
        let tokens = lexer::Lexer::new(&src).tokenize().ok()?;
        let program = parser::Parser::new(tokens).parse_program().ok()?;
        fn rec(c: &ast::CellDef, path: &str, names: &mut Vec<String>, cells: &mut Vec<String>) {
            let cell = if path.is_empty() { c.name.clone() } else { format!("{}.{}", path, c.name) };
            cells.push(cell.to_ascii_lowercase());
            names.push(format!("{}__counters", cell).to_ascii_lowercase());
            names.push(format!("{}__agent_memory", cell).to_ascii_lowercase());
            names.push(format!("{}__waits", cell).to_ascii_lowercase());
            names.push(format!("{}__horde-meta", cell).to_ascii_lowercase());
            names.push(format!("{}__horde-tasks", cell).to_ascii_lowercase());
            for sec in &c.sections {
                match &sec.node {
                    ast::Section::Memory(m) => for slot in &m.slots { names.push(format!("{}_{}", cell, slot.node.name).to_ascii_lowercase()); },
                    ast::Section::State(sm) => names.push(format!("{}__sm_{}", cell, sm.name).to_ascii_lowercase()),
                    ast::Section::Interior(i) => for ch in &i.cells { rec(&ch.node, &cell, names, cells); },
                    _ => {}
                }
            }
        }
        let (mut names, mut cells) = (Vec::new(), Vec::new());
        for c in &program.cells { rec(&c.node, "", &mut names, &mut cells); }
        Some((names, cells))
    }
    let mut names: Vec<String> = Vec::new();
    let mut my_cells: Vec<String> = Vec::new();
    if let Some((n, c)) = tables_of(file) { names.extend(n); my_cells.extend(c); }
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
    for d in deps { if let Some((n, c)) = tables_of(&d) { names.extend(n); my_cells.extend(c); } }
    // what the OTHER .cell files beside this one declare: they share this
    // database. A file that does not parse keeps every table whose name
    // mentions a cell it seems to declare (the regex fallback: keeping
    // data is the safe error)
    let (mut sibling_tables, mut sibling_cells): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    // cells of a sibling that does not parse: every table named after them
    // stays (its slot tables cannot be told apart — `--fresh a.cell` dropped
    // the `App_items` a broken b.cell also declares)
    let (mut unparsed_cells, mut unparsed): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    {
        let re = regex::Regex::new(r"(?m)^\s*cell\s+(?:[a-z_]+\s+)?([A-Za-z_][A-Za-z0-9_]*)").unwrap();
        let me = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        if let Ok(es) = std::fs::read_dir(if base.as_os_str().is_empty() { std::path::Path::new(".") } else { base }) {
            for e in es.flatten() {
                let p = e.path();
                if p.extension().map_or(true, |x| x != "cell") || p.canonicalize().map_or(false, |c| c == me) { continue; }
                match tables_of(&p) {
                    Some((n, c)) => { sibling_tables.extend(n); sibling_cells.extend(c); }
                    None => if let Ok(src) = std::fs::read_to_string(&p) {
                        let mut seen = Vec::new();
                        for c in re.captures_iter(&src) {
                            sibling_cells.push(c[1].to_ascii_lowercase());
                            unparsed_cells.push(c[1].to_ascii_lowercase());
                            seen.push(c[1].to_string());
                        }
                        if !seen.is_empty() {
                            unparsed.push(format!("{} (cell {})", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), seen.join(", ")));
                        }
                    },
                }
            }
        }
    }
    let Ok(conn) = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) else { return (Vec::new(), Vec::new(), Vec::new()) };
    let Ok(mut st) = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'") else { return (Vec::new(), Vec::new(), Vec::new()) };
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
    // the cell a foreign table belongs to: the LONGEST known cell name it
    // starts with (`App_x_items` is cell `App_x`'s when a program here
    // declares `App_x`, not a slot `x_items` of `App`)
    let known: Vec<&String> = my_cells.iter().chain(sibling_cells.iter()).collect();
    let owner_of = |f: &str| -> Option<String> {
        let lf = f.to_ascii_lowercase();
        known.iter().filter(|c| lf.starts_with(&format!("{}_", c))).max_by_key(|c| c.len()).map(|c| c.to_string())
    };
    // a cell's runtime tables (next_id() counters, remember() memory, a
    // horde's queue) are SHARED by every program that names the cell so:
    // when another program here has tables of the same cell, they stay
    // (`--fresh a2.cell` restarted `App`'s ids under a.cell's records)
    // …and when ANOTHER .cell file beside this one declares the same cell
    // (with no slots of its own there are no tables to tell: `--fresh
    // a.cell` wiped b.cell's remember() memory and next_id() counters)
    // …and a SLOT table another .cell file here declares too (the same
    // cell and slot) is that program's data as well: kept
    let mut kept: Vec<String> = Vec::new();
    own.retain(|t| {
        let lt = t.to_ascii_lowercase();
        let base = lt.strip_suffix("_log").unwrap_or(&lt);
        if sibling_tables.iter().any(|n| n == &lt || n == base) { kept.push(t.clone()); return false; }
        if unparsed_cells.iter().any(|c| lt.starts_with(&format!("{}_", c))) { kept.push(t.clone()); return false; }
        let Some(cell) = ["__counters", "__agent_memory", "__waits", "__horde-meta", "__horde-tasks"].iter().find_map(|sfx| lt.strip_suffix(sfx)) else { return true };
        if foreign.iter().any(|f| match owner_of(f) { Some(o) => o == cell, None => f.to_ascii_lowercase().starts_with(&format!("{}_", cell)) }) || sibling_cells.iter().any(|c| c == cell) { kept.push(t.clone()); false } else { true }
    });
    if !unparsed.is_empty() && !kept.is_empty() {
        eprintln!("fresh: {} beside this program does not parse — every table of the cell(s) it seems to declare is kept (fix it, then run --fresh again to reset them)", unparsed.join(", "));
    }
    (own, foreign, kept)
}


/// `soma test lib/` (also check): every .cell file under the directory, one
/// after another — a project-wide gate used to need a shell loop.
fn run_over_dir(cmd: &str, dir: &std::path::Path, json: bool, extra: &[&str]) -> ! {
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
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    walk(dir, &mut files);
    if files.is_empty() {
        eprintln!("no .cell file under {}", dir.display());
        // --json answers one document here too (stdout was empty)
        if json {
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "command": cmd, "files": [], "total": 0, "failed": 0, "ok": false,
                "note": "no .cell file under the directory",
            })).unwrap());
        }
        std::process::exit(1);
    }
    // `test`: a file with no test cell (a `use lib::…` library, a helper
    // cell) has nothing to run — it was counted as a failed file, so every
    // multi-file project failed `soma test .`
    // the summary's `note` is always there (null when there is nothing to say)
    let mut skipped = 0usize;
    if cmd == "test" {
        // parsed, not matched by a line regex: `} cell test U {…}` on one
        // line was "skipped"; a file that does not parse is run (and fails)
        // instead of skipped with exit 0
        let has_tests = |text: &str| -> bool {
            let Ok(tokens) = lexer::Lexer::new(text).tokenize() else { return true };
            match parser::Parser::new(tokens).parse_program() {
                Ok(p) => p.cells.iter().any(|c| matches!(c.node.kind, ast::CellKind::Test)),
                Err(_) => true,
            }
        };
        let (with, without): (Vec<_>, Vec<_>) = files.into_iter()
            .partition(|f| std::fs::read_to_string(f).map_or(true, |t| has_tests(&t)));
        for f in &without { eprintln!("── {} ── no test cells, skipped", f.display()); }
        skipped = without.len();
        if with.is_empty() {
            eprintln!("no test cells in any .cell file under {}", dir.display());
            // --json still answers one document (stdout was empty)
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "command": cmd, "files": [], "total": 0, "failed": 0, "ok": false,
                    "note": "no test cells in any .cell file",
                })).unwrap());
            }
            std::process::exit(1);
        }
        files = with;
    }
    // `check`: a file that holds a skeleton or a flesh and is imported by
    // another file of the directory is a PART of that program — alone, a
    // flesh has no skeleton and a skeleton no bodies. It is checked through
    // the file that imports it.
    if cmd == "check" {
        let parsed: Vec<(std::path::PathBuf, Option<ast::Program>)> = files.iter().map(|f| {
            let p = std::fs::read_to_string(f).ok().and_then(|t| lexer::Lexer::new(&t).tokenize().ok()).and_then(|toks| parser::Parser::new(toks).parse_program().ok());
            (f.clone(), p)
        }).collect();
        let real = |p: &std::path::Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        // who imports whom, among the files of the directory (an import is
        // looked up beside the importer, then from the directory given)
        let known: std::collections::HashSet<std::path::PathBuf> = files.iter().map(|f| real(f)).collect();
        let mut imports: std::collections::HashMap<std::path::PathBuf, Vec<std::path::PathBuf>> = std::collections::HashMap::new();
        for (f, p) in &parsed {
            let Some(p) = p else { continue };
            let base = f.parent().unwrap_or(std::path::Path::new("."));
            for u in &p.imports {
                for target in commands::refs::import_targets(u, base, dir) { if known.contains(&target) { imports.entry(real(f)).or_default().push(target); } }
            }
        }
        let reaches = |from: &std::path::PathBuf, to: &std::path::PathBuf| -> bool {
            let mut seen = std::collections::HashSet::new();
            let mut stack = vec![from.clone()];
            while let Some(x) = stack.pop() {
                if !seen.insert(x.clone()) { continue; }
                for y in imports.get(&x).into_iter().flatten() { if y == to { return true; } stack.push(y.clone()); }
            }
            false
        };
        let has_tests = |p: &Option<ast::Program>| p.as_ref().is_some_and(|p| p.cells.iter().any(|c| c.node.kind == ast::CellKind::Test));
        // a part: it holds a skeleton or a flesh, no test of its own, and a
        // file that is no test file imports it without being imported by it
        // (a test file may sit elsewhere, away from the project's soma.toml;
        // two files importing each other are both checked)
        let part = |f: &std::path::PathBuf, p: &Option<ast::Program>| {
            let me = real(f);
            p.as_ref().is_some_and(|p| p.cells.iter().any(|c| c.node.skeleton || c.node.kind == ast::CellKind::Flesh)) && !has_tests(p)
                && parsed.iter().any(|(g, gp)| { let g = real(g); g != me && !gp.as_ref().is_some_and(|p| !p.cells.is_empty() && p.cells.iter().all(|c| c.node.kind == ast::CellKind::Test)) && gp.is_some() && imports.get(&g).is_some_and(|v| v.contains(&me)) && !reaches(&me, &g) })
        };
        let parts: Vec<std::path::PathBuf> = parsed.iter().filter(|(f, p)| part(f, p)).map(|(f, _)| f.clone()).collect();
        if parts.len() < files.len() {
            for f in &parts { eprintln!("── {} ── a part of a program (skeleton / flesh): checked through the file that imports it", f.display()); }
            files.retain(|f| !parts.contains(f));
        }
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("soma"));
    let (mut failed, total) = (0usize, files.len());
    // `test`: the assertions of every file, in one total
    let (mut passed_all, mut failed_all) = (0usize, 0usize);
    // --json: ONE document holding each file's report (the per-file objects
    // were written back to back, which no JSON reader accepts)
    let mut reports: Vec<serde_json::Value> = Vec::new();
    for f in &files {
        eprintln!("── {} ──", f.display());
        let mut c = std::process::Command::new(&exe);
        c.arg(cmd).arg(f).args(extra);
        let ok = if json {
            c.arg("--json");
            match c.stderr(std::process::Stdio::inherit()).output() {
                Ok(out) => {
                    let text = String::from_utf8_lossy(&out.stdout).to_string();
                    let report = serde_json::from_str::<serde_json::Value>(&text).unwrap_or(serde_json::Value::String(text));
                    reports.push(serde_json::json!({ "file": f.display().to_string(), "ok": out.status.success(), "report": report }));
                    out.status.success()
                }
                Err(_) => false,
            }
        } else if cmd == "test" {
            // (its output passes through; its last `N tests: P passed, F failed` is added up)
            match c.stderr(std::process::Stdio::inherit()).output() {
                Ok(out) => {
                    let text = String::from_utf8_lossy(&out.stdout).to_string();
                    print!("{}", text);
                    for l in text.lines() {
                        let w: Vec<&str> = l.split_whitespace().collect();
                        if w.len() >= 6 && w[1] == "tests:" && w[3] == "passed," && w[5].starts_with("failed") {
                            passed_all += w[2].parse::<usize>().unwrap_or(0);
                            failed_all += w[4].parse::<usize>().unwrap_or(0);
                        }
                    }
                    out.status.success()
                }
                Err(_) => false,
            }
        } else {
            c.status().map(|s| s.success()).unwrap_or(false)
        };
        if !ok { failed += 1; }
    }
    if cmd == "test" && !json && passed_all + failed_all > 0 {
        println!("{} assertions: {} passed, {} failed — in {} file(s)", passed_all + failed_all, passed_all, failed_all, total);
    }
    eprintln!("{}: {} file(s), {} failed", cmd, total, failed);
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "command": cmd, "files": reports, "total": total, "failed": failed, "ok": failed == 0,
            "note": if skipped > 0 { serde_json::json!(format!("{} file(s) with no test cells skipped", skipped)) } else { serde_json::Value::Null },
        })).unwrap());
    }
    std::process::exit(if failed > 0 { 1 } else { 0 });
}
