use std::path::PathBuf;
use std::process;

use crate::ast;
use crate::interpreter;
use crate::registry::Registry;
use crate::runtime;
use crate::vm;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports, load_meta_cells_from_program};

pub fn cmd_run(path: &PathBuf, args: &[String], use_jit: bool, signal_flag: Option<&str>, registry: &mut Registry) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);

    load_meta_cells_from_program(&program, registry, path);

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
            (handler_names[0].clone(), arg_values)
        }
    } else {
        // No explicit signal: prefer handler matching arg count, then "run", then first zero-arg handler
        let n_args = arg_values.len();
        let default = handler_params.iter().find(|(h, p)| h == "run" && *p == n_args)
            .or_else(|| handler_params.iter().find(|(h, _)| h == "run"))
            .or_else(|| handler_params.iter().find(|(_, p)| *p == n_args))
            .or_else(|| handler_params.iter().find(|(_, p)| *p == 0))
            .unwrap_or(&handler_params[0]);
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

    match vm.call_signal(&cell_name, &signal_name, actual_args) {
        Ok(val) => println!("{}", val),
        Err(e) => { eprintln!("vm error: {}", e); process::exit(1); }
    }
}

fn run_single_cell(program: ast::Program, arg_values: Vec<interpreter::Value>, registry: &Registry, signal_flag: Option<&str>, source_path: &PathBuf, source: &str) {
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
            (handler_names[0].clone(), arg_values)
        }
    } else {
        // No explicit signal: prefer handler matching arg count, then "run", then first zero-arg handler
        let n_args = arg_values.len();
        let default = handler_params.iter().find(|(h, p)| h == "run" && *p == n_args)
            .or_else(|| handler_params.iter().find(|(h, _)| h == "run"))
            .or_else(|| handler_params.iter().find(|(_, p)| *p == n_args))
            .or_else(|| handler_params.iter().find(|(_, p)| *p == 0))
            .unwrap_or(&handler_params[0]);
        (default.0.clone(), arg_values)
    };

    let mut interp = interpreter::Interpreter::new(&program);

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
