use std::path::PathBuf;
use std::process;

use crate::ast;
use crate::interpreter;
use crate::registry::Registry;
use crate::runtime;
use super::{read_source, lex_with_location, parse_with_location, resolve_imports, load_meta_cells_from_program};

pub fn cmd_test(path: &PathBuf, registry: &mut Registry) {
    let source = read_source(path);
    let file_str = path.display().to_string();
    let tokens = lex_with_location(&source, Some(&file_str));
    let mut program = parse_with_location(tokens, Some(&source), Some(&file_str));
    resolve_imports(&mut program, path);
    load_meta_cells_from_program(&program, registry, path);

    let test_cells: Vec<&ast::CellDef> = program.cells.iter()
        .filter(|c| c.node.kind == ast::CellKind::Test)
        .map(|c| &c.node)
        .collect();

    if test_cells.is_empty() {
        eprintln!("no test cells found (use `cell test MyTests {{ ... }}`)");
        process::exit(1);
    }

    let mut interp = interpreter::Interpreter::new(&program);

    for cell in &program.cells {
        if matches!(cell.node.kind, ast::CellKind::Cell | ast::CellKind::Agent) {
            for section in &cell.node.sections {
                if let ast::Section::Memory(ref mem) = section.node {
                    let mut slots = std::collections::HashMap::new();
                    for slot in &mem.slots {
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

fn eval_test_assertion(
    interp: &mut interpreter::Interpreter,
    expr: &ast::Expr,
) -> Result<bool, String> {
    match expr {
        ast::Expr::CmpOp { left, op, right } => {
            let left_val = eval_test_expr(interp, &left.node)?;
            let right_val = eval_test_expr(interp, &right.node)?;

            let result = interp.eval_cmpop_values(&left_val, op.clone(), &right_val)
                .unwrap_or(false);

            if !result {
                eprintln!("         left:  {}", left_val);
                eprintln!("         right: {}", right_val);
            }

            Ok(result)
        }
        _ => {
            let val = eval_test_expr(interp, expr)?;
            Ok(val.is_truthy())
        }
    }
}

fn eval_test_expr(
    interp: &mut interpreter::Interpreter,
    expr: &ast::Expr,
) -> Result<interpreter::Value, String> {
    // Delegate to the real interpreter for full expression support
    // (pipes, lambdas, match, field access, method calls, etc.)
    let env = std::collections::HashMap::new();
    interp.eval_expr_with_env(expr, &env, "", "")
        .map_err(|e| format!("{:?}", e))
}

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
