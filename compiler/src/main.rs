#![allow(dead_code, unused_imports, unused_variables)]

mod ast;
mod checker;
mod codegen;
mod interpreter;
mod lexer;
mod parser;
mod pkg;
mod provider;
mod registry;
mod runtime;
mod vm;

use clap::{Parser as ClapParser, Subcommand};
use std::fs;
use std::io::Read as IoRead;
use std::path::{Path, PathBuf};
use std::process;

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

    // Load registry from stdlib
    let mut registry = Registry::new();
    let stdlib_path = cli.stdlib.clone().unwrap_or_else(find_stdlib);
    if let Err(e) = registry.load_dir(&stdlib_path) {
        eprintln!("warning: failed to load stdlib: {}", e);
    }

    match cli.command {
        Commands::Check { file } => cmd_check(&file, &mut registry),
        Commands::Build { file, output } => cmd_build(&file, output.as_deref(), &mut registry),
        Commands::Ast { file } => cmd_ast(&file),
        Commands::Tokens { file } => cmd_tokens(&file),
        Commands::Run { file, args, jit, signal } => cmd_run(&file, &args, jit, signal.as_deref(), &mut registry),
        Commands::Serve { file, port, watch, verbose } => {
            if watch {
                cmd_serve_watch(&file, port, &mut registry);
            } else {
                cmd_serve(&file, port, verbose, &mut registry);
            }
        }
        Commands::Test { file } => cmd_test(&file, &mut registry),
        Commands::Init { name } => cmd_init(name.as_deref()),
        Commands::Add { package, version, git, path } => cmd_add(&package, version.as_deref(), git.as_deref(), path.as_deref()),
        Commands::Install => cmd_install(),
        Commands::Env => cmd_env(),
        Commands::Repl => cmd_repl(&mut registry),
        Commands::AddProvider { name } => cmd_add_provider(&name),
        Commands::TestProvider { name } => cmd_test_provider(&name),
        Commands::Migrate { from, to } => cmd_migrate(&from, &to),
        Commands::Props => cmd_props(&registry),
    }
}

/// Find the stdlib directory by looking relative to the source file or binary
fn find_stdlib() -> PathBuf {
    // Try relative to current directory, then relative to binary, then user home
    let mut candidates = vec![
        PathBuf::from("stdlib"),
        PathBuf::from("../stdlib"),
    ];
    // Relative to the binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("../stdlib"));
            candidates.push(parent.join("stdlib"));
        }
    }
    // Local .soma_env/stdlib
    candidates.push(PathBuf::from(".soma_env/stdlib"));
    // User home: ~/.soma/stdlib/
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".soma/stdlib"));
    }

    for candidate in &candidates {
        if candidate.exists() {
            return candidate.clone();
        }
    }

    // Default
    PathBuf::from("stdlib")
}

fn read_source(path: &PathBuf) -> String {
    match fs::read_to_string(path) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("error: cannot read '{}': {}", path.display(), e);
            process::exit(1);
        }
    }
}

fn lex(source: &str) -> Vec<lexer::SpannedToken> {
    let mut lex = lexer::Lexer::new(source);
    match lex.tokenize() {
        Ok(tokens) => tokens,
        Err(e) => {
            eprintln!("lexer error: {}", e);
            process::exit(1);
        }
    }
}

fn parse(tokens: Vec<lexer::SpannedToken>) -> ast::Program {
    let mut p = parser::Parser::new(tokens);
    match p.parse_program() {
        Ok(program) => program,
        Err(e) => {
            eprintln!("parse error: {}", e);
            process::exit(1);
        }
    }
}

/// Resolve `use` imports: load referenced files and merge their cells
fn resolve_imports(program: &mut ast::Program, base_path: &PathBuf) {
    let base_dir = base_path.parent().unwrap_or(std::path::Path::new("."));

    for import_path in &program.imports.clone() {
        let full_path = if import_path.starts_with("pkg:") {
            // Package: use pkg::math → .soma_env/packages/math/
            let pkg_name = &import_path[4..];
            resolve_pkg_path(base_dir, pkg_name)
        } else if import_path.starts_with("std:") {
            // Stdlib: use std::builtins → .soma_env/stdlib/ or stdlib/
            let mod_name = &import_path[4..];
            let candidates = [
                base_dir.join(".soma_env/stdlib").join(format!("{}.cell", mod_name)),
                base_dir.join("stdlib").join(format!("{}.cell", mod_name)),
                PathBuf::from("stdlib").join(format!("{}.cell", mod_name)),
            ];
            candidates.into_iter().find(|p| p.exists())
                .unwrap_or_else(|| {
                    eprintln!("error: stdlib module '{}' not found", mod_name);
                    process::exit(1);
                })
        } else if import_path.starts_with("lib:") {
            // Local lib: use lib::helpers → lib/helpers.cell or lib/helpers/
            let mod_name = &import_path[4..];
            let as_file = base_dir.join("lib").join(format!("{}.cell", mod_name));
            let as_dir = base_dir.join("lib").join(mod_name);
            if as_file.exists() { as_file } else { as_dir }
        } else {
            // Direct path or relative
            let with_ext = if !import_path.ends_with(".cell") {
                format!("{}.cell", import_path)
            } else {
                import_path.clone()
            };
            let as_path = base_dir.join(&with_ext);
            let as_dir = base_dir.join(import_path);
            if as_path.exists() { as_path } else { as_dir }
        };

        // If it's a directory, import all .cell files in it
        if full_path.is_dir() {
            if let Ok(entries) = fs::read_dir(&full_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map_or(false, |e| e == "cell") {
                        import_file(program, &path);
                    }
                }
            }
        } else {
            import_file(program, &full_path);
        }
    }
}

fn resolve_pkg_path(base_dir: &Path, pkg_name: &str) -> PathBuf {
    let candidates = [
        base_dir.join(".soma_env/packages").join(pkg_name),
        PathBuf::from(".soma_env/packages").join(pkg_name),
        base_dir.join("packages").join(pkg_name),
    ];
    for c in &candidates {
        if c.exists() { return c.clone(); }
    }
    eprintln!("error: package '{}' not installed (run `soma install`)", pkg_name);
    process::exit(1);
}

fn import_file(program: &mut ast::Program, path: &PathBuf) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot import '{}': {}", path.display(), e);
            process::exit(1);
        }
    };
    let tokens = lex(&source);
    let mut imported = parse(tokens);
    resolve_imports(&mut imported, path);
    program.cells.extend(imported.cells);
}

fn cmd_check(path: &PathBuf, registry: &mut Registry) {
    let source = read_source(path);
    let tokens = lex(&source);
    let mut program = parse(tokens);
    resolve_imports(&mut program, path);

    // Load any meta-cells from the source file itself into the registry
    load_meta_cells_from_program(&program, registry, path);

    let mut chk = checker::Checker::new(registry);
    chk.check(&program);

    print!("{}", chk.report());

    if chk.has_errors() {
        process::exit(1);
    }
}

fn cmd_build(path: &PathBuf, output: Option<&Path>, registry: &mut Registry) {
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

fn cmd_ast(path: &PathBuf) {
    let source = read_source(path);
    let tokens = lex(&source);
    let program = parse(tokens);
    println!("{:#?}", program);
}

fn cmd_tokens(path: &PathBuf) {
    let source = read_source(path);
    let tokens = lex(&source);
    for tok in &tokens {
        println!("{:?}  @ {:?}", tok.token, tok.span);
    }
}

fn cmd_run(path: &PathBuf, args: &[String], use_jit: bool, signal_flag: Option<&str>, registry: &mut Registry) {
    let source = read_source(path);
    let tokens = lex(&source);
    let mut program = parse(tokens);
    resolve_imports(&mut program, path);

    load_meta_cells_from_program(&program, registry, path);

    // Parse CLI args into values
    let arg_values: Vec<interpreter::Value> = args
        .iter()
        .map(|a| {
            if let Ok(n) = a.parse::<i64>() {
                interpreter::Value::Int(n)
            } else if let Ok(n) = a.parse::<f64>() {
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

    // Check if this is a multi-cell program (has interior) or single-cell
    let regular_cells: Vec<&ast::CellDef> = program
        .cells
        .iter()
        .filter(|c| c.node.kind == ast::CellKind::Cell)
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
        run_with_vm(program, arg_values, registry, &source, signal_flag);
    } else {
        run_single_cell(program, arg_values, registry, signal_flag, path, &source);
    }
}

fn run_with_vm(program: ast::Program, arg_values: Vec<interpreter::Value>, registry: &Registry, source: &str, signal_flag: Option<&str>) {
    eprintln!("note: --jit mode does not support all features yet (e.g., string interpolation)");
    // Try loading compiled bytecode from cache
    let chunks = if let Some(cached) = vm::load_cached(source) {
        cached
    } else {
        let mut compiler = vm::BytecodeCompiler::new();
        compiler.compile_program(&program);
        vm::save_cache(source, &compiler.chunks);
        compiler.chunks
    };

    let cell = program.cells.iter()
        .find(|c| c.node.kind == ast::CellKind::Cell && c.node.sections.iter().any(|s| matches!(s.node, ast::Section::OnSignal(_))))
        .or_else(|| program.cells.iter().find(|c| c.node.kind == ast::CellKind::Cell))
        .unwrap_or_else(|| { eprintln!("error: no cell found"); process::exit(1); });
    let cell_name = cell.node.name.clone();

    // Find handler names
    let handler_names: Vec<String> = cell.node.sections.iter()
        .filter_map(|s| if let ast::Section::OnSignal(ref on) = s.node { Some(on.signal_name.clone()) } else { None })
        .collect();

    let (signal_name, actual_args) = if let Some(sig) = signal_flag {
        (sig.to_string(), arg_values)
    } else if let Some(interpreter::Value::String(ref name)) = arg_values.first() {
        if handler_names.contains(name) {
            (name.clone(), arg_values[1..].to_vec())
        } else {
            (handler_names[0].clone(), arg_values)
        }
    } else {
        (handler_names[0].clone(), arg_values)
    };

    // Create VM
    let mut vm = vm::VM::new(chunks);

    // Set up storage
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

    match vm.call_signal(&cell_name, &signal_name, actual_args) {
        Ok(val) => println!("{}", val),
        Err(e) => { eprintln!("vm error: {}", e); process::exit(1); }
    }
}

fn run_single_cell(program: ast::Program, arg_values: Vec<interpreter::Value>, registry: &Registry, signal_flag: Option<&str>, source_path: &PathBuf, source: &str) {
    // Find the main cell based on the signal being called
    // If first arg matches a handler name in a specific cell, use that cell
    let requested_signal = arg_values.first().and_then(|v| {
        if let interpreter::Value::String(s) = v { Some(s.clone()) } else { None }
    });
    let cell = requested_signal.as_ref().and_then(|sig| {
        program.cells.iter().find(|c| c.node.kind == ast::CellKind::Cell && c.node.sections.iter().any(|s| {
            if let ast::Section::OnSignal(ref on) = s.node { on.signal_name == *sig } else { false }
        }))
    })
    .or_else(|| program.cells.iter().find(|c| c.node.kind == ast::CellKind::Cell && c.node.sections.iter().any(|s| {
        if let ast::Section::OnSignal(ref on) = s.node { on.signal_name == "request" } else { false }
    })))
    .or_else(|| program.cells.iter().find(|c| c.node.kind == ast::CellKind::Cell && c.node.sections.iter().any(|s| matches!(s.node, ast::Section::OnSignal(_)))))
        .unwrap_or_else(|| {
            eprintln!("error: no runnable cell found");
            process::exit(1);
        });
    let cell_name = cell.node.name.clone();

    // Collect all signal handler names from the main cell
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

    // If the first arg matches a handler name, use it as the signal
    // Otherwise, use the first (or only) handler
    let (signal_name, actual_args) = if let Some(sig) = signal_flag {
        (sig.to_string(), arg_values)
    } else if let Some(interpreter::Value::String(ref name)) = arg_values.first() {
        if handler_names.contains(name) {
            (name.clone(), arg_values[1..].to_vec())
        } else {
            (handler_names[0].clone(), arg_values)
        }
    } else {
        (handler_names[0].clone(), arg_values)
    };

    let mut interp = interpreter::Interpreter::new(&program);

    // Set up storage for ALL cells (including imported ones)
    for prog_cell in &program.cells {
        if prog_cell.node.kind != ast::CellKind::Cell { continue; }
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
                interp.ensure_state_machine_storage();
            }
        }
    }

    interp.source_file = Some(source_path.display().to_string());
    interp.source_text = Some(source.to_string());

    match interp.call_signal(&cell_name, &signal_name, actual_args) {
        Ok(val) => println!("{}", val),
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

fn run_with_runtime(program: ast::Program, args: &[interpreter::Value]) {
    let mut rt = runtime::Runtime::new(program);

    // Print instantiation info
    eprintln!("soma runtime v0.1.0");
    eprintln!("---");
    rt.dump_state();
    eprintln!("---");

    // Find the first top-level cell with a runtime section or interior
    let main_cell = rt.cells.keys().next().cloned().unwrap_or_else(|| {
        eprintln!("error: no runnable cell found");
        process::exit(1);
    });

    // If the cell has a runtime section, execute it
    if let Err(e) = rt.run_cell(&main_cell) {
        eprintln!("runtime error: {}", e);
        process::exit(1);
    }

    // If args were provided, try to emit them as a signal to the first child
    if !args.is_empty() {
        // Find the first child that has an on-handler
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

    // Print signal log
    if !rt.signal_log.is_empty() {
        eprintln!("---");
        eprintln!("signal log:");
        for entry in &rt.signal_log {
            eprintln!("  {}", entry);
        }
    }
}

fn cmd_serve_watch(path: &PathBuf, port: u16, _registry: &mut Registry) {
    eprintln!("soma serve --watch");
    eprintln!("watching: {}", path.display());
    eprintln!("---");

    let exe = std::env::current_exe().unwrap();
    let mut last_modified = fs::metadata(path).ok()
        .and_then(|m| m.modified().ok());

    loop {
        // Start server as child process
        let mut child = std::process::Command::new(&exe)
            .args(["serve", path.to_str().unwrap(), "-p", &port.to_string()])
            .spawn()
            .expect("failed to start server");

        // Watch for changes
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));

            let current = fs::metadata(path).ok()
                .and_then(|m| m.modified().ok());

            if current != last_modified {
                last_modified = current;
                eprintln!("\n--- file changed, reloading... ---\n");
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
}

fn cmd_serve(path: &PathBuf, port: u16, verbose: bool, registry: &mut Registry) {
    let source = read_source(path);
    let tokens = lex(&source);
    let mut program = parse(tokens);
    resolve_imports(&mut program, path);
    load_meta_cells_from_program(&program, registry, path);

    // Find the main cell
    let cell = program
        .cells
        .iter()
        .find(|c| c.node.kind == ast::CellKind::Cell && c.node.sections.iter().any(|s| {
            if let ast::Section::OnSignal(ref on) = s.node { on.signal_name == "request" } else { false }
        }))
        .or_else(|| program.cells.iter().find(|c| c.node.kind == ast::CellKind::Cell && c.node.sections.iter().any(|s| matches!(s.node, ast::Section::OnSignal(_)))))
        .unwrap_or_else(|| {
            eprintln!("error: no cell found");
            process::exit(1);
        });
    let cell_name = cell.node.name.clone();

    // Collect handler names and their parameter lists
    let handler_names: Vec<String> = cell.node.sections.iter()
        .filter_map(|s| {
            if let ast::Section::OnSignal(ref on) = s.node {
                Some(on.signal_name.clone())
            } else {
                None
            }
        })
        .collect();

    // Map handler name -> list of param names (for JSON body extraction)
    let handler_params: std::collections::HashMap<String, Vec<String>> = cell.node.sections.iter()
        .filter_map(|s| {
            if let ast::Section::OnSignal(ref on) = s.node {
                Some((on.signal_name.clone(), on.params.iter().map(|p| p.name.clone()).collect()))
            } else {
                None
            }
        })
        .collect();

    // Set up storage for ALL cells (including imported ones)
    let mut storage_slots = std::collections::HashMap::new();
    for prog_cell in &program.cells {
        if prog_cell.node.kind != ast::CellKind::Cell { continue; }
        for section in &prog_cell.node.sections {
            if let ast::Section::Memory(ref mem) = section.node {
                for slot in &mem.slots {
                    let props: Vec<String> = slot.node.properties.iter()
                        .map(|p| p.node.name().to_string())
                        .collect();
                    let backend = runtime::storage::resolve_backend_from_registry(
                        &prog_cell.node.name, &slot.node.name, &props, registry);
                    // Register with AND without prefix — both work
                    storage_slots.insert(
                        format!("{}.{}", prog_cell.node.name, slot.node.name), backend.clone());
                    storage_slots.insert(slot.node.name.clone(), backend);
                }
            }
        }
    }

    let addr = format!("0.0.0.0:{}", port);
    let server = tiny_http::Server::http(&addr).unwrap_or_else(|e| {
        eprintln!("error: cannot start server on {}: {}", addr, e);
        process::exit(1);
    });

    eprintln!("soma serve v{}", env!("CARGO_PKG_VERSION"));
    eprintln!("cell: {}", cell_name);
    eprintln!("handlers: [{}]", handler_names.join(", "));
    eprintln!("database: {}", std::path::Path::new(".soma_data/soma.db").canonicalize()
        .unwrap_or_else(|_| std::path::PathBuf::from(".soma_data/soma.db")).display());
    eprintln!("listening on http://localhost:{}", port);
    eprintln!("---");

    // Wrap shared state in Arc for thread safety
    let program = std::sync::Arc::new(program);
    let storage_slots = std::sync::Arc::new(storage_slots);
    let handler_names = std::sync::Arc::new(handler_names);
    let handler_params = std::sync::Arc::new(handler_params);
    let cell_name = std::sync::Arc::new(cell_name);
    let base_dir = std::sync::Arc::new(
        path.parent().unwrap_or(std::path::Path::new(".")).to_path_buf()
    );

    // Spawn scheduler threads for `every` sections
    for cell_spanned in &program.cells {
        if cell_spanned.node.kind != ast::CellKind::Cell { continue; }
        for section in &cell_spanned.node.sections {
            if let ast::Section::Every(ref every) = section.node {
                let interval = every.interval_ms;
                let body = every.body.clone();
                let prog = program.clone();
                let slots = storage_slots.clone();
                let cname = cell_name.clone();
                eprintln!("scheduler: every {}ms", interval);

                std::thread::spawn(move || {
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(interval));
                        let mut interp = interpreter::Interpreter::new(&prog);
                        interp.set_storage_raw(&slots);
                        interp.ensure_state_machine_storage();
                        let mut env = std::collections::HashMap::new();
                        let _ = interp.exec_every(&body, &mut env, &cname);
                    }
                });
            }
        }
    }

    for mut request in server.incoming_requests() {
        let program = program.clone();
        let storage_slots = storage_slots.clone();
        let handler_names = handler_names.clone();
        let handler_params = handler_params.clone();
        let cell_name = cell_name.clone();
        let base_dir = base_dir.clone();

        std::thread::spawn(move || {
        let method = request.method().to_string();
        let url = request.url().to_string();

        // Read body and parse JSON if applicable
        let mut body_raw = String::new();
        let _ = request.as_reader().read_to_string(&mut body_raw);

        // Parse JSON body into a Value::Map if it looks like JSON
        let body_value = if body_raw.starts_with('{') || body_raw.starts_with('[') {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body_raw) {
                Some(json_request_to_value(&parsed))
            } else {
                None
            }
        } else {
            None
        };
        let body = body_raw;

        // Serve static files: /static/path → base_dir/static/path
        if url.starts_with("/static/") {
            let file_path = base_dir.join(&url[1..]); // strip leading /
            if file_path.exists() && file_path.is_file() {
                let content = std::fs::read(&file_path).unwrap_or_default();
                let mime = match file_path.extension().and_then(|e| e.to_str()) {
                    Some("css") => "text/css",
                    Some("js") => "application/javascript",
                    Some("html") => "text/html",
                    Some("png") => "image/png",
                    Some("jpg") | Some("jpeg") => "image/jpeg",
                    Some("svg") => "image/svg+xml",
                    Some("ico") => "image/x-icon",
                    Some("woff2") => "font/woff2",
                    Some("json") => "application/json",
                    _ => "application/octet-stream",
                };
                let resp = tiny_http::Response::from_data(content)
                    .with_header(
                        tiny_http::Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).unwrap()
                    );
                let _ = request.respond(resp);
                return;
            } else {
                let resp = tiny_http::Response::from_string("not found")
                    .with_status_code(404);
                let _ = request.respond(resp);
                return;
            }
        }

        // Create a fresh interpreter per request (shares storage via Arc)
        let mut interp = interpreter::Interpreter::new(&program);
        interp.set_storage_raw(&storage_slots);
        interp.ensure_state_machine_storage();

        // Determine which signal to call
        // Priority: /signal/ prefix > path-based routing > generic request handler
        let (signal_name, args) = if url.starts_with("/signal/") {
            // Direct signal call: /signal/put?key=foo&value=bar
            let signal = url.trim_start_matches("/signal/");
            let (sig_name, query) = signal.split_once('?').unwrap_or((signal, ""));
            let args: Vec<interpreter::Value> = if query.is_empty() {
                vec![]
            } else {
                query.split('&')
                    .filter_map(|pair| {
                        let (_, v) = pair.split_once('=')?;
                        Some(interpreter::Value::String(
                            urlencoding_decode(v)
                        ))
                    })
                    .collect()
            };
            (sig_name.to_string(), args)
        } else {
            // Split URL into path and query string
            let (url_path, query_string) = url.split_once('?').unwrap_or((&url, ""));
            let path = url_path.trim_start_matches('/');
            let (sig, rest) = path.split_once('/').unwrap_or((path, ""));
            if handler_names.contains(&sig.to_string()) {
                // Combine path segments and query params as args
                let mut args: Vec<interpreter::Value> = if rest.is_empty() {
                    vec![]
                } else {
                    rest.split('/')
                        .map(|s| {
                            let decoded = urlencoding_decode(s);
                            if let Ok(n) = decoded.parse::<i64>() {
                                interpreter::Value::Int(n)
                            } else {
                                interpreter::Value::String(decoded)
                            }
                        })
                        .collect()
                };
                // Add query parameters as additional args
                if !query_string.is_empty() {
                    for pair in query_string.split('&') {
                        if let Some((_, v)) = pair.split_once('=') {
                            let decoded = urlencoding_decode(v).replace('+', " ");
                            args.push(interpreter::Value::String(decoded));
                        }
                    }
                }
                // If POST with JSON body, extract fields to match handler params
                if method == "POST" && !body.is_empty() {
                    if let Some(ref bv) = body_value {
                        // If handler has named params, extract fields from JSON
                        if let Some(param_names) = handler_params.get(sig) {
                            if let interpreter::Value::Map(ref entries) = bv {
                                // Extract fields in param order, skipping params already filled by path/query
                                let remaining_params = &param_names[args.len()..];
                                for pname in remaining_params {
                                    let val = entries.iter()
                                        .find(|(k, _)| k == pname)
                                        .map(|(_, v)| v.clone())
                                        .unwrap_or(interpreter::Value::Unit);
                                    args.push(val);
                                }
                            } else {
                                // Non-object JSON (array, scalar) — pass as single arg
                                args.push(bv.clone());
                            }
                        } else {
                            args.push(bv.clone());
                        }
                    } else {
                        args.push(interpreter::Value::String(body.clone()));
                    }
                }
                (sig.to_string(), args)
            } else if handler_names.contains(&"request".to_string()) {
                // Fall back to generic request handler
                // Parse query params for all routes
                let (req_path, req_query) = url.split_once('?').unwrap_or((&url, ""));
                let query_map: Vec<(String, interpreter::Value)> = if req_query.is_empty() {
                    vec![]
                } else {
                    req_query.split('&')
                        .filter_map(|pair| {
                            let (k, v) = pair.split_once('=')?;
                            Some((
                                urlencoding_decode(k),
                                interpreter::Value::String(urlencoding_decode(v).replace('+', " ")),
                            ))
                        })
                        .collect()
                };
                // Pass parsed JSON body if available, otherwise raw string
                let body_arg = body_value.clone()
                    .unwrap_or(interpreter::Value::String(body.clone()));

                // Build args: method, path (without query), body, query_params map
                let mut req_args = vec![
                    interpreter::Value::String(method.clone()),
                    interpreter::Value::String(req_path.to_string()),
                    body_arg,
                ];
                if !query_map.is_empty() {
                    req_args.push(interpreter::Value::Map(query_map));
                }
                (
                    "request".to_string(),
                    req_args,
                )
            } else {
                // 404
                let resp = tiny_http::Response::from_string(
                    format!("{{\"error\": \"no handler for '{}'\", \"available\": [{:?}]}}",
                        url, handler_names.join(", "))
                )
                .with_status_code(404)
                .with_header(
                    tiny_http::Header::from_bytes(
                        &b"Content-Type"[..], &b"application/json"[..]
                    ).unwrap()
                );
                let _ = request.respond(resp);
                return;
            }
        };

        if verbose {
            eprintln!("  signal: {}", signal_name);
            eprintln!("  args: {:?}", args);
        }

        let start_time = std::time::Instant::now();

        match interp.call_signal(&cell_name, &signal_name, args) {
            Ok(val) => {
                // Check if the value is a structured response (from response() builtin)
                let is_response = if let interpreter::Value::Map(ref entries) = val {
                    entries.iter().any(|(k, _)| k == "_status")
                } else {
                    false
                };
                let (status_code, body_str, content_type, extra_headers) = if is_response {
                    let entries = if let interpreter::Value::Map(ref e) = val { e } else { unreachable!() };
                    let status = entries.iter()
                        .find(|(k, _)| k == "_status")
                        .and_then(|(_, v)| if let interpreter::Value::Int(n) = v { Some(*n as u16) } else { None })
                        .unwrap_or(200);
                    let content_type = entries.iter()
                        .find(|(k, _)| k == "_content_type")
                        .and_then(|(_, v)| if let interpreter::Value::String(s) = v { Some(s.clone()) } else { None })
                        .unwrap_or("application/json".to_string());
                    let body_val = entries.iter()
                        .find(|(k, _)| k == "_body")
                        .map(|(_, v)| v.clone())
                        .unwrap_or(interpreter::Value::Unit);
                    let headers: Vec<(String, String)> = entries.iter()
                        .filter(|(k, _)| !k.starts_with('_'))
                        .map(|(k, v)| (k.clone(), format!("{}", v)))
                        .collect();
                    let is_html = content_type.contains("html");
                    let body_str = if is_html {
                        // HTML: return raw string, no JSON wrapping
                        match &body_val {
                            interpreter::Value::String(s) => s.clone(),
                            interpreter::Value::Unit => String::new(),
                            other => format!("{}", other),
                        }
                    } else {
                        match &body_val {
                            interpreter::Value::Unit => "{}".to_string(),
                            interpreter::Value::Map(_) | interpreter::Value::List(_) => format!("{}", body_val),
                            interpreter::Value::String(s) => {
                                if s.starts_with('{') || s.starts_with('[') { s.clone() }
                                else { format!("{{\"result\": \"{}\"}}", s) }
                            }
                            other => format!("{{\"result\": {}}}", other),
                        }
                    };
                    (status, body_str, content_type, headers)
                } else {
                    let body = match &val {
                        interpreter::Value::Unit => "{}".to_string(),
                        interpreter::Value::List(_) | interpreter::Value::Map(_) => format!("{}", val),
                        interpreter::Value::String(s) => {
                            if s.starts_with('{') || s.starts_with('[') { s.clone() }
                            else { format!("{{\"result\": \"{}\"}}", s) }
                        }
                        other => format!("{{\"result\": {}}}", other),
                    };
                    (200u16, body, "application/json".to_string(), vec![])
                };

                let verbose_body = if verbose { Some(body_str.clone()) } else { None };
                let mut resp = tiny_http::Response::from_string(body_str)
                    .with_status_code(tiny_http::StatusCode(status_code))
                    .with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..], content_type.as_bytes()
                        ).unwrap()
                    );
                for (key, val) in &extra_headers {
                    if let Ok(h) = tiny_http::Header::from_bytes(key.as_bytes(), val.as_bytes()) {
                        resp.add_header(h);
                    }
                }
                // CORS
                resp.add_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
                let elapsed = start_time.elapsed();
                eprintln!("{} {} → {} {}ms", method, url, status_code, elapsed.as_millis());
                if let Some(ref vb) = verbose_body {
                    eprintln!("  response body: {}", vb);
                }
                let _ = request.respond(resp);
            }
            Err(e) => {
                let body = format!("{{\"error\": \"{}\"}}", e);
                let mut resp = tiny_http::Response::from_string(body)
                    .with_status_code(500)
                    .with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..], &b"application/json"[..]
                        ).unwrap()
                    );
                resp.add_header(tiny_http::Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap());
                let elapsed = start_time.elapsed();
                eprintln!("{} {} → 500 {}ms {}", method, url, elapsed.as_millis(), e);
                let _ = request.respond(resp);
            }
        }

        }); // end thread::spawn
    }
}

fn urlencoding_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        match b {
            b'%' => {
                let hi = chars.next().unwrap_or(b'0');
                let lo = chars.next().unwrap_or(b'0');
                let byte = (hex_val(hi) << 4) | hex_val(lo);
                result.push(byte as char);
            }
            b'+' => result.push(' '),
            _ => result.push(b as char),
        }
    }
    result
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

fn json_request_to_value(v: &serde_json::Value) -> interpreter::Value {
    match v {
        serde_json::Value::Null => interpreter::Value::Unit,
        serde_json::Value::Bool(b) => interpreter::Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                interpreter::Value::Int(i)
            } else {
                interpreter::Value::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => interpreter::Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            interpreter::Value::List(arr.iter().map(json_request_to_value).collect())
        }
        serde_json::Value::Object(obj) => {
            interpreter::Value::Map(
                obj.iter().map(|(k, v)| (k.clone(), json_request_to_value(v))).collect()
            )
        }
    }
}

// ── Test runner ──────────────────────────────────────────────────────

fn cmd_test(path: &PathBuf, registry: &mut Registry) {
    let source = read_source(path);
    let tokens = lex(&source);
    let mut program = parse(tokens);
    resolve_imports(&mut program, path);
    load_meta_cells_from_program(&program, registry, path);

    // Find all test cells
    let test_cells: Vec<&ast::CellDef> = program.cells.iter()
        .filter(|c| c.node.kind == ast::CellKind::Test)
        .map(|c| &c.node)
        .collect();

    if test_cells.is_empty() {
        eprintln!("no test cells found (use `cell test MyTests {{ ... }}`)");
        process::exit(1);
    }

    // Build an interpreter with all cells (so tests can call signal handlers)
    let mut interp = interpreter::Interpreter::new(&program);

    // Set up storage for regular cells — TESTS USE IN-MEMORY ONLY
    // This ensures tests never pollute the persistent database
    for cell in &program.cells {
        if cell.node.kind == ast::CellKind::Cell {
            for section in &cell.node.sections {
                if let ast::Section::Memory(ref mem) = section.node {
                    let mut slots = std::collections::HashMap::new();
                    for slot in &mem.slots {
                        // Force in-memory backend for test isolation
                        let backend: std::sync::Arc<dyn runtime::storage::StorageBackend> =
                            std::sync::Arc::new(runtime::storage::MemoryBackend::new());
                        slots.insert(slot.node.name.clone(), backend);
                    }
                    interp.set_storage(&cell.node.name, &slots);
                    interp.ensure_state_machine_storage();
                }
            }
        }
    }

    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;

    for test_cell in &test_cells {
        println!("test {} ...", test_cell.name);

        // Collect assert rules
        for section in &test_cell.sections {
            if let ast::Section::Rules(ref rules) = section.node {
                for rule in &rules.rules {
                    if let ast::Rule::Assert(ref expr) = rule.node {
                        total += 1;

                        match eval_test_assertion(&mut interp, &expr.node) {
                            Ok(true) => {
                                passed += 1;
                                println!("  ✓ assert {}", format_expr(&expr.node));
                            }
                            Ok(false) => {
                                failed += 1;
                                println!("  ✗ assert {} — FAILED", format_expr(&expr.node));
                            }
                            Err(e) => {
                                failed += 1;
                                println!("  ✗ assert {} — ERROR: {}", format_expr(&expr.node), e);
                            }
                        }
                    }
                }
            }
        }
    }

    println!("\n{} tests: {} passed, {} failed", total, passed, failed);

    if failed > 0 {
        process::exit(1);
    }
}

/// Evaluate a test assertion expression
fn eval_test_assertion(
    interp: &mut interpreter::Interpreter,
    expr: &ast::Expr,
) -> Result<bool, String> {
    match expr {
        ast::Expr::CmpOp { left, op, right } => {
            let left_val = eval_test_expr(interp, &left.node)?;
            let right_val = eval_test_expr(interp, &right.node)?;

            // Compare as floats for numeric comparisons (handles both Int and Float)
            let lf = match &left_val { interpreter::Value::Int(n) => *n as f64, interpreter::Value::Float(n) => *n, _ => 0.0 };
            let rf = match &right_val { interpreter::Value::Int(n) => *n as f64, interpreter::Value::Float(n) => *n, _ => 0.0 };
            let result = match op {
                ast::CmpOp::Eq => format!("{}", left_val) == format!("{}", right_val),
                ast::CmpOp::Ne => format!("{}", left_val) != format!("{}", right_val),
                ast::CmpOp::Lt => lf < rf,
                ast::CmpOp::Gt => lf > rf,
                ast::CmpOp::Le => lf <= rf,
                ast::CmpOp::Ge => lf >= rf,
            };

            if !result {
                eprintln!("         left:  {}", left_val);
                eprintln!("         right: {}", right_val);
            }

            Ok(result)
        }
        // Bare expression — truthy check
        _ => {
            let val = eval_test_expr(interp, expr)?;
            Ok(val.is_truthy())
        }
    }
}

/// Evaluate an expression in test context
fn eval_test_expr(
    interp: &mut interpreter::Interpreter,
    expr: &ast::Expr,
) -> Result<interpreter::Value, String> {
    match expr {
        ast::Expr::FnCall { name, args } => {
            // Try to call a signal handler
            let mut arg_vals = Vec::new();
            for arg in args {
                arg_vals.push(eval_test_expr(interp, &arg.node)?);
            }
            // Find which cell has this handler
            interp.find_and_call(name, arg_vals)
                .map_err(|e| format!("{}", e))
        }
        ast::Expr::Literal(lit) => Ok(match lit {
            ast::Literal::Int(n) => interpreter::Value::Int(*n),
            ast::Literal::Float(n) => interpreter::Value::Float(*n),
            ast::Literal::String(s) => interpreter::Value::String(s.clone()),
            ast::Literal::Bool(b) => interpreter::Value::Bool(*b),
            ast::Literal::Unit => interpreter::Value::Unit,
            _ => interpreter::Value::Unit,
        }),
        ast::Expr::BinaryOp { left, op, right } => {
            let l = eval_test_expr(interp, &left.node)?;
            let r = eval_test_expr(interp, &right.node)?;
            // Simple arithmetic for test expressions
            match (l, op, r) {
                (interpreter::Value::Int(a), ast::BinOp::Add, interpreter::Value::Int(b)) => Ok(interpreter::Value::Int(a + b)),
                (interpreter::Value::Int(a), ast::BinOp::Sub, interpreter::Value::Int(b)) => Ok(interpreter::Value::Int(a - b)),
                (interpreter::Value::Int(a), ast::BinOp::Mul, interpreter::Value::Int(b)) => Ok(interpreter::Value::Int(a * b)),
                _ => Ok(interpreter::Value::Unit),
            }
        }
        ast::Expr::Ident(name) => Ok(interpreter::Value::String(name.clone())),
        _ => Err(format!("unsupported expression in test")),
    }
}

/// Format an expression for display
fn format_expr(expr: &ast::Expr) -> String {
    match expr {
        ast::Expr::CmpOp { left, op, right } => {
            format!("{} {} {}", format_expr(&left.node), op, format_expr(&right.node))
        }
        ast::Expr::FnCall { name, args } => {
            let args_str: Vec<String> = args.iter().map(|a| format_expr(&a.node)).collect();
            format!("{}({})", name, args_str.join(", "))
        }
        ast::Expr::Literal(lit) => match lit {
            ast::Literal::Int(n) => n.to_string(),
            ast::Literal::Float(n) => n.to_string(),
            ast::Literal::String(s) => format!("\"{}\"", s),
            ast::Literal::Bool(b) => b.to_string(),
            _ => "?".to_string(),
        },
        ast::Expr::Ident(name) => name.clone(),
        ast::Expr::BinaryOp { left, op, right } => {
            format!("{} {} {}", format_expr(&left.node), op, format_expr(&right.node))
        }
        _ => "...".to_string(),
    }
}

// ── REPL ────────────────────────────────────────────────────────────

fn cmd_repl(registry: &mut Registry) {
    eprintln!("soma repl v0.1.0 — type expressions to evaluate, :quit to exit");

    // Create a minimal program with an empty cell for the REPL
    let empty_program = ast::Program {
        imports: vec![],
        cells: vec![],
    };
    let mut interp = interpreter::Interpreter::new(&empty_program);

    let stdin = std::io::stdin();
    let mut line = String::new();

    loop {
        eprint!("soma> ");
        line.clear();
        match stdin.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => {
                eprintln!("error: {}", e);
                break;
            }
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if input == ":quit" || input == ":q" || input == "exit" {
            break;
        }

        // Try to parse as a cell definition first
        if input.starts_with("cell ") {
            let tokens = lex(input);
            match parser::Parser::new(tokens).parse_program() {
                Ok(program) => {
                    for cell in &program.cells {
                        interp.register_cell(cell.node.clone());
                        println!("defined cell: {}", cell.node.name);
                    }
                }
                Err(e) => eprintln!("parse error: {}", e),
            }
            continue;
        }

        // Try to parse as a signal call: handler_name(args)
        // Wrap it in a minimal cell handler so we can evaluate it
        let wrapper = format!(
            "cell _Repl {{ on _eval() {{ return {} }} }}",
            input
        );

        let tokens = lex(&wrapper);
        match parser::Parser::new(tokens).parse_program() {
            Ok(program) => {
                for cell in &program.cells {
                    interp.register_cell(cell.node.clone());
                }
                match interp.call_signal("_Repl", "_eval", vec![]) {
                    Ok(val) => println!("{}", val),
                    Err(e) => eprintln!("error: {}", e),
                }
            }
            Err(e) => eprintln!("parse error: {}", e),
        }
    }
}

// ── Package manager commands ─────────────────────────────────────────

fn cmd_init(name: Option<&str>) {
    let cwd = std::env::current_dir().unwrap();

    // If a name is provided, create a subdirectory; otherwise init in cwd
    let (project_dir, project_name) = if let Some(n) = name {
        let dir = cwd.join(n);
        if dir.exists() {
            eprintln!("error: directory '{}' already exists", n);
            process::exit(1);
        }
        fs::create_dir_all(&dir).unwrap_or_else(|e| {
            eprintln!("error: cannot create directory '{}': {}", n, e);
            process::exit(1);
        });
        (dir, n.to_string())
    } else {
        let n = cwd.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("myapp")
            .to_string();
        (cwd.clone(), n)
    };

    let manifest_path = project_dir.join("soma.toml");
    if manifest_path.exists() {
        eprintln!("soma.toml already exists");
        process::exit(1);
    }

    // Create manifest
    let manifest = pkg::Manifest::new(&project_name);
    manifest.save(&manifest_path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    // Create environment
    let env = pkg::SomaEnv::init(&project_dir).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    // Create starter main.cell
    let main_path = project_dir.join("main.cell");
    if !main_path.exists() {
        fs::write(&main_path, r#"// Your Soma app starts here

cell App {
    face {
        signal hello(name: String) -> String
    }

    on hello(name: String) {
        return concat("Hello, ", concat(name, "!"))
    }
}
"#).ok();
    }

    let rel_prefix = if name.is_some() {
        format!("{}/", project_name)
    } else {
        String::new()
    };

    println!("initialized soma project: {}", project_name);
    println!("");
    println!("  {}soma.toml        project manifest", rel_prefix);
    println!("  {}main.cell        entry point", rel_prefix);
    println!("  {}.soma_env/       isolated environment", rel_prefix);
    println!("    stdlib/         {} property definitions", env.all_cell_paths().len());
    println!("    packages/      dependencies (empty)");
    println!("    cache/          compiled bytecode");
    println!("");
    println!("next steps:");
    if name.is_some() {
        println!("  cd {}", project_name);
    }
    println!("  soma run main.cell hello world");
    println!("  soma add mypackage --git https://github.com/user/repo");
    println!("  soma serve main.cell");
}

fn cmd_add(package: &str, version: Option<&str>, git: Option<&str>, path: Option<&str>) {
    let cwd = std::env::current_dir().unwrap();
    let manifest_path = cwd.join("soma.toml");

    if !manifest_path.exists() {
        eprintln!("error: no soma.toml found (run `soma init` first)");
        process::exit(1);
    }

    let mut manifest = pkg::Manifest::load(&manifest_path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    // Build dependency spec
    let dep = if let Some(git_url) = git {
        pkg::Dependency::Full(pkg::DependencySpec {
            git: Some(git_url.to_string()),
            path: None,
            version: version.map(|v| v.to_string()),
            branch: None,
        })
    } else if let Some(local_path) = path {
        pkg::Dependency::Full(pkg::DependencySpec {
            git: None,
            path: Some(local_path.to_string()),
            version: None,
            branch: None,
        })
    } else {
        pkg::Dependency::Version(version.unwrap_or("*").to_string())
    };

    manifest.dependencies.insert(package.to_string(), dep);
    manifest.save(&manifest_path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    println!("added {} to soma.toml", package);
    println!("run `soma install` to fetch it");
}

fn cmd_install() {
    let cwd = std::env::current_dir().unwrap();
    let manifest_path = cwd.join("soma.toml");
    let lock_path = cwd.join("soma.lock");

    if !manifest_path.exists() {
        eprintln!("error: no soma.toml found (run `soma init` first)");
        process::exit(1);
    }

    let manifest = pkg::Manifest::load(&manifest_path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    let mut lock = pkg::LockFile::load(&lock_path).unwrap_or_default();

    // Ensure environment exists
    let _env = pkg::SomaEnv::init(&cwd).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    println!("installing {} dependencies...", manifest.dependencies.len());

    // Resolve dependencies into .soma_env/packages/
    let _env_packages = cwd.join(".soma_env").join("packages");
    // Temporarily override cache dir to use env packages
    let installed = pkg::resolve_and_install(&cwd.join(".soma_env"), &manifest, &mut lock)
        .unwrap_or_else(|e| {
            eprintln!("error: {}", e);
            process::exit(1);
        });

    // Save lock file
    lock.save(&lock_path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    println!("");
    println!("installed {} packages", installed.len());
    for (name, path) in &installed {
        println!("  {} → {}", name, path.display());
    }
}

fn cmd_env() {
    let cwd = std::env::current_dir().unwrap();

    if let Some(env) = pkg::SomaEnv::load(&cwd) {
        let packages = env.list_packages();
        let cell_files = env.all_cell_paths();

        println!("soma environment: {}", env.root.display());
        println!("");
        println!("stdlib: {} files", cell_files.iter().filter(|p| p.starts_with(&env.stdlib_dir)).count());
        println!("packages: {}", packages.len());
        for pkg in &packages {
            println!("  {}", pkg);
        }
        println!("total .cell files: {}", cell_files.len());
    } else {
        println!("no environment found (run `soma init`)");
    }
}

fn cmd_add_provider(name: &str) {
    let cwd = std::env::current_dir().unwrap();
    let providers_dir = cwd.join(".soma_env/providers").join(name);

    if providers_dir.exists() {
        eprintln!("provider '{}' already installed at {}", name, providers_dir.display());
        return;
    }

    fs::create_dir_all(&providers_dir).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        process::exit(1);
    });

    // Create a skeleton manifest
    let manifest = format!(r#"[provider]
name = "{name}"
version = "0.1.0"
description = "{name} storage provider for Soma"

[provider.auth]
env = []

# Define backends here. Example:
# [[backend]]
# name = "default"
# requires = ["persistent", "consistent"]
# optional = ["encrypted"]
# native = "sqlite"
"#);

    fs::write(providers_dir.join("soma-provider.toml"), manifest).unwrap();

    // Update soma.toml if it exists
    let soma_toml = cwd.join("soma.toml");
    if soma_toml.exists() {
        let mut content = fs::read_to_string(&soma_toml).unwrap_or_default();
        if !content.contains("[storage]") {
            content.push_str(&format!("\n[storage]\nprovider = \"{}\"\n", name));
            fs::write(&soma_toml, content).unwrap();
        }
    }

    println!("provider '{}' added", name);
    println!("  manifest: {}/soma-provider.toml", providers_dir.display());
    println!("  edit the manifest to configure backends");
    println!("  test with: soma test-provider {}", name);
}

fn cmd_test_provider(name: &str) {
    let cwd = std::env::current_dir().unwrap();

    let resolver = if name == "local" {
        provider::ProviderResolver::local()
    } else {
        let config = provider::StorageConfig {
            provider: name.to_string(),
            config: std::collections::HashMap::new(),
            overrides: std::collections::HashMap::new(),
        };
        provider::ProviderResolver::from_config(config, &cwd).unwrap_or_else(|e| {
            eprintln!("error: {}", e);
            process::exit(1);
        })
    };

    println!("testing provider: {}", resolver.provider_name());
    println!("");

    // Test 1: persistent + consistent
    let mut pass = 0;
    let mut fail = 0;

    let test_cases = vec![
        ("persistent, consistent", vec!["persistent".to_string(), "consistent".to_string()]),
        ("ephemeral", vec!["ephemeral".to_string()]),
        ("persistent, encrypted", vec!["persistent".to_string(), "encrypted".to_string()]),
        ("ephemeral, local", vec!["ephemeral".to_string(), "local".to_string()]),
    ];

    for (label, props) in &test_cases {
        let request = provider::StorageRequest {
            cell_name: "Test".to_string(),
            field_name: "data".to_string(),
            field_type: "Map<String, String>".to_string(),
            properties: props.iter().map(|p| provider::Property::Flag(p.clone())).collect(),
        };
        match resolver.resolve(&request) {
            Ok(backend) => {
                // Basic CRUD test
                backend.set("test_key", runtime::storage::StoredValue::String("test_val".to_string()));
                let got = backend.get("test_key");
                let keys = backend.keys();
                backend.delete("test_key");
                let after = backend.get("test_key");

                let ok = got.is_some() && keys.contains(&"test_key".to_string()) && after.is_none();
                if ok {
                    println!("  ✓ [{}] → {} — CRUD passed", label, backend.backend_name());
                    pass += 1;
                } else {
                    println!("  ✗ [{}] → {} — CRUD failed", label, backend.backend_name());
                    fail += 1;
                }
            }
            Err(e) => {
                println!("  ✗ [{}] — resolve failed: {}", label, e);
                fail += 1;
            }
        }
    }

    // Test contradiction detection
    let bad_request = provider::StorageRequest {
        cell_name: "Test".to_string(),
        field_name: "bad".to_string(),
        field_type: "Map".to_string(),
        properties: vec![
            provider::Property::Flag("persistent".to_string()),
            provider::Property::Flag("ephemeral".to_string()),
        ],
    };
    // The compiler should catch this, but verify the resolver doesn't match
    match resolver.resolve(&bad_request) {
        Ok(_) => { println!("  ✗ [persistent, ephemeral] should not resolve"); fail += 1; }
        Err(_) => { println!("  ✓ [persistent, ephemeral] correctly rejected"); pass += 1; }
    }

    println!("\n{} tests: {} passed, {} failed", pass + fail, pass, fail);
    if fail > 0 { process::exit(1); }
}

fn cmd_migrate(from: &str, to: &str) {
    println!("soma migrate --from {} --to {}", from, to);
    println!("");
    println!("Migration reads all keys from source provider and writes to target.");
    println!("This uses the StorageBackend trait — works between any two providers.");
    println!("");

    let cwd = std::env::current_dir().unwrap();

    // Load source and target resolvers
    let source = if from == "local" {
        provider::ProviderResolver::local()
    } else {
        let config = provider::StorageConfig { provider: from.to_string(), ..Default::default() };
        provider::ProviderResolver::from_config(config, &cwd).unwrap_or_else(|e| {
            eprintln!("error loading source: {}", e); process::exit(1);
        })
    };

    let target = if to == "local" {
        provider::ProviderResolver::local()
    } else {
        let config = provider::StorageConfig { provider: to.to_string(), ..Default::default() };
        provider::ProviderResolver::from_config(config, &cwd).unwrap_or_else(|e| {
            eprintln!("error loading target: {}", e); process::exit(1);
        })
    };

    println!("source: {} → target: {}", source.provider_name(), target.provider_name());
    println!("note: migration requires a .cell file to know which fields to migrate.");
    println!("usage: soma migrate --from local --to aws (with soma.toml configured)");
}

fn cmd_props(registry: &Registry) {
    println!("Registered properties ({} total):\n", registry.properties.len());

    let mut names: Vec<&String> = registry.properties.keys().collect();
    names.sort();

    for name in names {
        let def = &registry.properties[name];
        println!("  {} {}", if def.has_params { "●" } else { "○" }, name);

        if !def.promises.is_empty() {
            for p in &def.promises {
                println!("    promise: {}", p);
            }
        }
        if !def.contradicts.is_empty() {
            let mut c: Vec<&String> = def.contradicts.iter().collect();
            c.sort();
            println!("    contradicts: [{}]", c.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        }
        if !def.implies.is_empty() {
            let mut i: Vec<&String> = def.implies.iter().collect();
            i.sort();
            println!("    implies: [{}]", i.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        }
        if !def.requires.is_empty() {
            let mut r: Vec<&String> = def.requires.iter().collect();
            r.sort();
            println!("    requires: [{}]", r.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        }
        if let Some(ref group) = def.mutex_group {
            println!("    mutex_group: {}", group);
        }
        println!();
    }

    if !registry.mutex_groups.is_empty() {
        println!("Mutex groups:");
        for (group, members) in &registry.mutex_groups {
            println!("  {} → [{}]", group, members.join(", "));
        }
    }

    if !registry.backends.is_empty() {
        println!("\nBackends ({} total):", registry.backends.len());
        for backend in &registry.backends {
            println!("  {} (native: {})", backend.name,
                backend.native_impl.as_deref().unwrap_or("?"));
            for m in &backend.matches {
                println!("    matches [{}]", m.join(", "));
            }
        }
    }

    if !registry.builtins.is_empty() {
        println!("\nBuiltins ({} total):", registry.builtins.len());
        let mut names: Vec<&String> = registry.builtins.keys().collect();
        names.sort();
        for name in names {
            let def = &registry.builtins[name];
            println!("  {} (native: {})", name,
                def.native_impl.as_deref().unwrap_or("?"));
        }
    }
}

/// Load meta-cells (property, checker, type) from the user's program into the registry.
/// This allows users to define custom properties in their own .cell files.
fn load_meta_cells_from_program(program: &ast::Program, registry: &mut Registry, _path: &PathBuf) {
    // Re-register meta-cells from the parsed program
    for cell in &program.cells {
        match cell.node.kind {
            ast::CellKind::Property | ast::CellKind::Checker | ast::CellKind::Type
            | ast::CellKind::Backend | ast::CellKind::Builtin | ast::CellKind::Test => {
                if let Err(e) = register_cell_from_ast(registry, &cell.node) {
                    eprintln!("warning: failed to register {}: {}", cell.node.name, e);
                }
            }
            ast::CellKind::Cell => {}
        }
    }
}

fn register_cell_from_ast(registry: &mut Registry, cell: &ast::CellDef) -> Result<(), String> {
    match cell.kind {
        ast::CellKind::Property => {
            let mut def = registry::PropertyDef {
                name: cell.name.clone(),
                contradicts: std::collections::HashSet::new(),
                implies: std::collections::HashSet::new(),
                requires: std::collections::HashSet::new(),
                mutex_group: None,
                has_params: false,
                promises: Vec::new(),
            };

            for section in &cell.sections {
                match &section.node {
                    ast::Section::Face(face) => {
                        for decl in &face.declarations {
                            match &decl.node {
                                ast::FaceDecl::Promise(p) => {
                                    if let ast::Constraint::Descriptive(s) = &p.constraint.node {
                                        def.promises.push(s.clone());
                                    }
                                }
                                ast::FaceDecl::Given(_) => {
                                    def.has_params = true;
                                }
                                _ => {}
                            }
                        }
                    }
                    ast::Section::Rules(rules) => {
                        for rule in &rules.rules {
                            match &rule.node {
                                ast::Rule::Contradicts(names) => {
                                    def.contradicts.extend(names.iter().cloned());
                                }
                                ast::Rule::Implies(names) => {
                                    def.implies.extend(names.iter().cloned());
                                }
                                ast::Rule::Requires(names) => {
                                    def.requires.extend(names.iter().cloned());
                                }
                                ast::Rule::MutexGroup(group) => {
                                    def.mutex_group = Some(group.clone());
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }

            if let Some(ref group) = def.mutex_group {
                registry
                    .mutex_groups
                    .entry(group.clone())
                    .or_default()
                    .push(def.name.clone());
            }

            let name = def.name.clone();
            let contradicts: Vec<String> = def.contradicts.iter().cloned().collect();
            registry.properties.insert(name.clone(), def);

            for other_name in &contradicts {
                if let Some(other_def) = registry.properties.get_mut(other_name) {
                    other_def.contradicts.insert(name.clone());
                }
            }

            Ok(())
        }
        ast::CellKind::Checker => {
            let mut def = registry::CheckerDef {
                name: cell.name.clone(),
                promises: Vec::new(),
                check_body: Vec::new(),
            };

            for section in &cell.sections {
                match &section.node {
                    ast::Section::Face(face) => {
                        for decl in &face.declarations {
                            if let ast::FaceDecl::Promise(p) = &decl.node {
                                if let ast::Constraint::Descriptive(s) = &p.constraint.node {
                                    def.promises.push(s.clone());
                                }
                            }
                        }
                    }
                    ast::Section::Rules(rules) => {
                        for rule in &rules.rules {
                            if let ast::Rule::Check(body) = &rule.node {
                                def.check_body = body.clone();
                            }
                        }
                    }
                    _ => {}
                }
            }

            registry.checkers.push(def);
            Ok(())
        }
        ast::CellKind::Backend => {
            let mut def = registry::BackendDef {
                name: cell.name.clone(),
                matches: Vec::new(),
                native_impl: None,
                promises: Vec::new(),
            };
            for section in &cell.sections {
                if let ast::Section::Rules(rules) = &section.node {
                    for rule in &rules.rules {
                        match &rule.node {
                            ast::Rule::Matches(props) => def.matches.push(props.clone()),
                            ast::Rule::Native(name) => def.native_impl = Some(name.clone()),
                            _ => {}
                        }
                    }
                }
            }
            registry.backends.push(def);
            Ok(())
        }
        ast::CellKind::Builtin => {
            let mut def = registry::BuiltinDef {
                name: cell.name.clone(),
                native_impl: None,
                promises: Vec::new(),
            };
            for section in &cell.sections {
                if let ast::Section::Rules(rules) = &section.node {
                    for rule in &rules.rules {
                        if let ast::Rule::Native(name) = &rule.node {
                            def.native_impl = Some(name.clone());
                        }
                    }
                }
            }
            registry.builtins.insert(def.name.clone(), def);
            Ok(())
        }
        _ => Ok(()),
    }
}
