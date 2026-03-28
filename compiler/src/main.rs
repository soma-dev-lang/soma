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
#[command(about = "The Soma language compiler — fractal, declarative, agent-native")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to stdlib directory (default: auto-detect)
    #[arg(long, global = true)]
    stdlib: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Check a .cell file for errors
    Check {
        /// Path to the .cell source file
        file: PathBuf,
    },
    /// Compile a .cell file and generate Rust code
    Build {
        /// Path to the .cell source file
        file: PathBuf,
        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Parse a .cell file and dump the AST
    Ast {
        /// Path to the .cell source file
        file: PathBuf,
    },
    /// Tokenize a .cell file and dump tokens
    Tokens {
        /// Path to the .cell source file
        file: PathBuf,
    },
    /// Run a .cell file: execute a signal handler with arguments
    Run {
        /// Path to the .cell source file
        file: PathBuf,
        /// Arguments to pass (parsed as integers or strings)
        args: Vec<String>,
        /// Use the bytecode VM instead of the tree-walking interpreter
        #[arg(long)]
        jit: bool,
        /// Signal handler to call (default: auto-detect from first arg or first handler)
        #[arg(long)]
        signal: Option<String>,
    },
    /// Serve a .cell file as a web application
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
    },
    /// Initialize a new Soma project
    Init {
        /// Project name (default: current directory name)
        name: Option<String>,
    },
    /// Add a dependency
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
    /// Install all dependencies
    Install,
    /// Run tests in a .cell file
    Test {
        /// Path to the .cell file containing test cells
        file: PathBuf,
    },
    /// List installed packages in the environment
    Env,
    /// Start an interactive REPL
    Repl,
    /// Add a storage provider
    AddProvider {
        /// Provider name (e.g., aws, gcp, cloudflare)
        name: String,
    },
    /// Test a storage provider against conformance suite
    TestProvider {
        /// Provider name
        name: String,
    },
    /// Migrate data between providers
    Migrate {
        /// Source provider
        #[arg(long)]
        from: String,
        /// Target provider
        #[arg(long)]
        to: String,
    },
    /// List all registered properties and their rules
    Props,
}

fn main() {
    let cli = Cli::parse();

    let mut registry = Registry::new();
    let stdlib_path = cli.stdlib.clone().unwrap_or_else(commands::find_stdlib);
    if let Err(e) = registry.load_dir(&stdlib_path) {
        eprintln!("warning: failed to load stdlib: {}", e);
    }

    match cli.command {
        Commands::Check { file } => commands::check::cmd_check(&file, &mut registry),
        Commands::Build { file, output } => commands::build::cmd_build(&file, output.as_deref(), &mut registry),
        Commands::Ast { file } => cmd_ast(&file),
        Commands::Tokens { file } => cmd_tokens(&file),
        Commands::Run { file, args, jit, signal } => commands::run::cmd_run(&file, &args, jit, signal.as_deref(), &mut registry),
        Commands::Serve { file, port, watch, verbose } => {
            if watch {
                commands::serve::cmd_serve_watch(&file, port, &mut registry);
            } else {
                commands::serve::cmd_serve(&file, port, verbose, &mut registry);
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
