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
        /// Arguments to pass (parsed as integers or strings)
        // negative numbers are values, not flags: `soma run f.cell -7 2`
        #[arg(allow_negative_numbers = true)]
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
    },
    /// Run test assertions in a .cell file. Storage is isolated: every
    /// memory slot uses a fresh in-memory backend, so runs never touch
    /// persistent state and always start clean. Supports `assert expr`,
    /// `assert_fails expr` (passes when expr errors), and `property`.
    Test {
        /// Path to the .cell file containing test cells
        file: PathBuf,
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
    // Run on a thread with an 8 MB stack to prevent SIGABRT on deep recursion
    // before the interpreter's own depth guard (max_depth: 512) can fire.
    let builder = std::thread::Builder::new().stack_size(16 * 1024 * 1024);
    let handler = builder.spawn(main_inner).expect("failed to spawn main thread");
    if let Err(e) = handler.join() {
        eprintln!("fatal: {:?}", e);
        std::process::exit(1);
    }
}

fn main_inner() {
    let cli = Cli::parse();

    let mut registry = Registry::new();
    let stdlib_path = cli.stdlib.clone().unwrap_or_else(commands::find_stdlib);
    if let Err(e) = registry.load_dir(&stdlib_path) {
        eprintln!("warning: failed to load stdlib: {}", e);
    }

    match cli.command {
        Commands::Check { file, json } => commands::check::cmd_check(&file, json, &mut registry),
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
        Commands::Run { file, args, jit, signal, record } => commands::run::cmd_run(&file, &args, jit, signal.as_deref(), record, &mut registry),
        Commands::Serve { file, port, host, no_check, watch, verbose, join } => {
            if watch {
                commands::serve::cmd_serve_watch(&file, port, &mut registry);
            } else {
                commands::serve::cmd_serve(&file, port, &host, verbose, join.as_deref(), no_check, &mut registry);
            }
        }
        Commands::Test { file } => commands::test_cmd::cmd_test(&file, &mut registry),
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
        Commands::Verify { files, json } => cmd_verify(&files, json),
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

fn cmd_verify(files: &[PathBuf], json: bool) {
    use checker::temporal::*;

    let mut all_results = Vec::new();
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
    let mut check_failed = false;

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
                eprintln!("{} fails `soma check` — fix these before verifying:", path.display());
                for line in chk.report().lines().filter(|l| !l.starts_with("warning") && !l.starts_with("advisory") && !l.starts_with("✓")) {
                    eprintln!("  {}", line);
                }
                check_failed = true;
            }
        }

        // every state any machine of this file declares
        let all_states: std::collections::HashSet<String> = program.cells.iter()
            .flat_map(|c| c.node.sections.iter())
            .filter_map(|s| if let ast::Section::State(sm) = &s.node { Some(sm) } else { None })
            .flat_map(|sm| StateMachineGraph::from_ast(sm).states.into_iter())
            .collect();

        eprintln!("Verifying {}...", path.display());
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
                                    unknown_states.push(format!("{} names state '{}', which no state machine declares", what, state));
                                }
                            }
                        }
                    }

                    let results: Vec<PropertyResult> = props.iter()
                        .map(|p| check_property(&graph, p))
                        .collect();

                    all_temporal.push((sm.name.clone(), results));
                }
            }
        }
    }

    if all_results.is_empty() && check_failed {
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
            if orphan_props {
                eprintln!("note: the soma.toml beside this file declares [verify] properties; none applies here (no state machine)");
            }
            eprintln!("VERIFY OK (vacuous — no state machine)");
        }
        return;
    }

    unknown_states.sort();
    unknown_states.dedup();
    let has_failures = all_results.iter().any(|r| r.has_failures())
        || all_temporal.iter().any(|(_, rs)| rs.iter().any(|r| !r.passed))
        || !unknown_states.is_empty()
        || check_failed;

    if json {
        // Machine-readable JSON output for agents
        let sm_results: Vec<serde_json::Value> = all_results.iter().map(|r| {
            let checks: Vec<serde_json::Value> = r.checks.iter().map(|c| {
                match c {
                    checker::verify::VerifyCheck::Pass(msg) => serde_json::json!({"status": "pass", "message": msg}),
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
            if user_props > 0 {
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
            eprintln!("error: soma.toml: {} — a property about a state that does not exist proves nothing", u);
        }
        // ONE verdict, last, so nobody stops reading at an early "0 failures"
        let structural = all_results.iter().filter(|r| r.has_failures()).count();
        let temporal = all_temporal.iter().flat_map(|(_, rs)| rs.iter()).filter(|r| !r.passed).count();
        if has_failures {
            let mut why: Vec<String> = Vec::new();
            if check_failed { why.push("soma check failed".to_string()); }
            if structural > 0 { why.push(format!("{} state machine{} with failed checks (see ✗ lines)", structural, if structural == 1 { "" } else { "s" })); }
            if temporal > 0 { why.push(format!("{} temporal propert{} failed", temporal, if temporal == 1 { "y" } else { "ies" })); }
            if !unknown_states.is_empty() { why.push(format!("{} propert{} on unknown states", unknown_states.len(), if unknown_states.len() == 1 { "y" } else { "ies" })); }
            eprintln!("VERIFY FAILED — {}", why.join("; "));
        } else {
            eprintln!("VERIFY OK");
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
