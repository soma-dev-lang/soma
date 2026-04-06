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
    Agent integration (MCP server):\n  \
      pip install mcp\n  \
      python3 mcp/soma_mcp.py\n\n\
    Docs: https://soma-lang.dev\n\
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
        args: Vec<String>,
        /// Deprecated: use [native] on handlers instead
        #[arg(long, hide = true)]
        jit: bool,
        /// Signal handler to call (default: auto-detect)
        #[arg(long)]
        signal: Option<String>,
    },
    /// Start HTTP server: soma serve app.cell [-p 8080] [--join host:port]
    Serve {
        /// Path to the .cell source file
        file: PathBuf,
        /// Port to listen on (default: 8080)
        #[arg(short, long, default_value = "8080")]
        port: u16,
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
    },
    /// Prove state machines, temporal logic, CAP properties, quorum
    Verify {
        /// Path to the .cell source file(s)
        files: Vec<PathBuf>,
        /// Output as JSON (for agents)
        #[arg(long)]
        json: bool,
    },
    /// Run test assertions in a .cell file
    Test {
        /// Path to the .cell file containing test cells
        file: PathBuf,
    },

    // ── Agent ─────────────────────────────────────────────────────
    /// Describe a cell as structured JSON: signals, memory, state machines, scale, routes
    Describe {
        /// Path to the .cell source file
        file: PathBuf,
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
        Commands::Fix { file, json } => commands::fix::cmd_fix(&file, json, &mut registry),
        Commands::Build { file, output } => commands::build::cmd_build(&file, output.as_deref(), &mut registry),
        Commands::Ast { file } => cmd_ast(&file),
        Commands::Tokens { file } => cmd_tokens(&file),
        Commands::Run { file, args, jit, signal } => commands::run::cmd_run(&file, &args, jit, signal.as_deref(), &mut registry),
        Commands::Serve { file, port, watch, verbose, join } => {
            if watch {
                commands::serve::cmd_serve_watch(&file, port, &mut registry);
            } else {
                commands::serve::cmd_serve(&file, port, verbose, join.as_deref(), &mut registry);
            }
        }
        Commands::Test { file } => commands::test_cmd::cmd_test(&file, &mut registry),
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
        Commands::Describe { file } => commands::describe::cmd_describe(&file),
    }
}

fn cmd_verify(files: &[PathBuf], json: bool) {
    use checker::temporal::*;

    let mut all_results = Vec::new();
    let mut all_temporal = Vec::new();

    // Try to read soma.toml for user-defined properties
    let manifest = files.first()
        .and_then(|f| f.parent())
        .map(|dir| dir.join("soma.toml"))
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read_to_string(&p).ok())
        .and_then(|content| toml::from_str::<pkg::manifest::Manifest>(&content).ok());

    let verify_config = manifest.as_ref().map(|m| &m.verify);

    for path in files {
        let source = commands::read_source(path);
        let tokens = commands::lex(&source);
        let mut program = commands::parse(tokens);
        commands::resolve_imports(&mut program, path);

        eprintln!("Verifying {}...", path.display());
        let results = checker::verify::verify_program(&program);
        all_results.extend(results);

        // Run temporal property checks on each state machine
        for cell in &program.cells {
            if cell.node.kind != ast::CellKind::Cell { continue; }
            for section in &cell.node.sections {
                if let ast::Section::State(ref sm) = section.node {
                    let graph = StateMachineGraph::from_ast(sm);
                    let mut props = Vec::new();

                    // Auto-derive: deadlock_free
                    props.push(Property::DeadlockFree);

                    // User-defined properties from soma.toml [verify]
                    if let Some(cfg) = verify_config {
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

                        // always = ["valid_state"]
                        for state in &cfg.always {
                            props.push(Property::Always(StatePredicate::InState(state.clone())));
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
                                props.push(Property::After(
                                    trigger.clone(),
                                    StatePredicate::NotInState(state.clone()),
                                ));
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

    if all_results.is_empty() {
        if json {
            println!("{{\"state_machines\":[], \"temporal\":[], \"passed\": true}}");
        } else {
            eprintln!("No state machines found.");
        }
        return;
    }

    let has_failures = all_results.iter().any(|r| r.has_failures())
        || all_temporal.iter().any(|(_, rs)| rs.iter().any(|r| !r.passed));

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

        let output = serde_json::json!({
            "passed": !has_failures,
            "state_machines": sm_results,
            "temporal": temporal_results,
        });
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
            let user_props = cfg.eventually.len() + cfg.never.len() + cfg.always.len()
                + cfg.after.values().map(|a| a.eventually.len() + a.never.len()).sum::<usize>()
                + if cfg.deadlock_free { 1 } else { 0 };
            if user_props > 0 {
                eprintln!("soma.toml: {} user-defined properties loaded", user_props);
            }
        }

        eprintln!("Temporal: {} passed, {} failed", passed_temporal, failed_temporal);
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
