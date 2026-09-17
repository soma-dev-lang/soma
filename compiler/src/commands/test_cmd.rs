//! `soma test` — run the assertions declared in `cell test` blocks.
//!
//! Test isolation: every memory slot of every cell is rebound to a fresh
//! in-memory backend (`MemoryBackend`) before any assertion runs — see
//! the slot-binding loop in `cmd_test`. Persistent storage configured on
//! the slots (`[persistent]`, providers, .soma_data) is never read or
//! written, so every `soma test` run starts from a clean slate and
//! leaves no state behind for the next run.
//!
//! Assertion forms:
//!   assert expr            — passes when expr is true
//!   assert_fails expr      — passes when evaluating expr produces a
//!                            runtime error (pin negative paths, e.g.
//!                            invalid state transitions MUST fail)
//!   property "n" forall …  — randomized property over an Int range

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

    // Assertions are reported with their exact source text and file:line —
    // spans count characters (the lexer walks a Vec<char>).
    let src_chars: Vec<char> = source.chars().collect();
    let text_of = |span: ast::Span, fallback: String| -> String {
        if span.end > span.start && span.end <= src_chars.len() {
            let raw: String = src_chars[span.start..span.end].iter().collect();
            raw.split_whitespace().collect::<Vec<_>>().join(" ")
        } else {
            fallback
        }
    };
    let line_of = |span: ast::Span| -> usize { interpreter::span_to_location(&source, span.start).0 };
    let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("").to_string();

    let mut interp = interpreter::Interpreter::new(&program);

    // Read agent config from soma.toml [agent] / [models.*], like `soma run`
    // — otherwise `mock = "..."` and per-cell models are inert under test.
    {
        let soma_toml = path.parent().unwrap_or(std::path::Path::new(".")).join("soma.toml");
        if let Ok(content) = std::fs::read_to_string(&soma_toml) {
            if let Ok(manifest) = toml::from_str::<crate::pkg::manifest::Manifest>(&content) {
                interp.agent_config = Some(manifest.agent);
                interp.agent_models = manifest.models;
            }
        }
    }

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
                }
            }
        }
    }
    // State machines need their instance storage even when the cell has
    // no memory section (a kill switch with no slots is still a machine).
    interp.ensure_state_machine_storage();

    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;

    for test_cell in &test_cells {
        println!("test {} ...", test_cell.name);

        for section in &test_cell.sections {
            if let ast::Section::Rules(ref rules) = section.node {
                for rule in &rules.rules {
                    match &rule.node {
                        ast::Rule::Assert(expr) => {
                            total += 1;

                            let shown = text_of(expr.span, format_expr(&expr.node));
                            match eval_test_assertion(&mut interp, &expr.node) {
                                Ok((true, _)) => {
                                    passed += 1;
                                    println!("  ✓ assert {}", shown);
                                }
                                Ok((false, detail)) => {
                                    failed += 1;
                                    println!("  ✗ {}:{}  assert {} — FAILED", file_name, line_of(expr.span), shown);
                                    for line in detail {
                                        println!("         {}", line);
                                    }
                                }
                                Err(e) => {
                                    failed += 1;
                                    println!("  ✗ {}:{}  assert {} — ERROR: {}", file_name, line_of(expr.span), shown, e);
                                }
                            }
                        }
                        ast::Rule::AssertFails(expr) => {
                            total += 1;

                            match eval_test_expr(&mut interp, &expr.node) {
                                // A typo'd signal name is NOT the failure
                                // under test — a vacuously green assert_fails
                                // would hide it forever. (UndefinedVar stays a
                                // passing domain error: it can arise inside
                                // the handler being asserted on.)
                                Err(e) if e.contains("UndefinedFn") => {
                                    failed += 1;
                                    println!("  ✗ assert_fails {} — FAILED: the expression itself is invalid ({}), not a domain error; fix the test",
                                             format_expr(&expr.node), e);
                                }
                                Err(e) => {
                                    passed += 1;
                                    println!("  ✓ assert_fails {} — raised {}", format_expr(&expr.node), e);
                                }
                                Ok(v) => {
                                    failed += 1;
                                    println!("  ✗ assert_fails {} — FAILED: expected a runtime error, but it succeeded with {}",
                                             format_expr(&expr.node), v);
                                }
                            }
                        }
                        ast::Rule::Property { name, var, ty, lo, hi, count, body } => {
                            total += 1;
                            match run_property(&mut interp, name, var, ty, *lo, *hi, *count, &body.node) {
                                Ok(None) => {
                                    passed += 1;
                                    println!("  ✓ property \"{}\" (forall {} in {}..{}, {} samples)",
                                             name, var, lo, hi, count);
                                }
                                Ok(Some(cex)) => {
                                    failed += 1;
                                    println!("  ✗ property \"{}\" — FAILED with counter-example {} = {}",
                                             name, var, cex);
                                }
                                Err(e) => {
                                    failed += 1;
                                    println!("  ✗ property \"{}\" — ERROR: {}", name, e);
                                }
                            }
                        }
                        _ => {}
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
) -> Result<(bool, Vec<String>), String> {
    match expr {
        ast::Expr::CmpOp { left, op, right } => {
            let left_val = eval_test_expr(interp, &left.node)?;
            let right_val = eval_test_expr(interp, &right.node)?;

            let result = interp.eval_cmpop_values(&left_val, op.clone(), &right_val)
                .unwrap_or(false);

            let mut detail = Vec::new();
            if !result {
                detail.push(format!("left:  {}", left_val));
                detail.push(format!("right: {}", right_val));
                for side in [&left.node, &right.node] {
                    if let Some(why) = explain_absent_field(interp, side) {
                        detail.push(why);
                    }
                }
            }

            Ok((result, detail))
        }
        _ => {
            let val = eval_test_expr(interp, expr)?;
            Ok((val.is_truthy(), Vec::new()))
        }
    }
}

/// `x.field` that evaluates to `()`: reading a missing key is not an error
/// in Soma, so a wrong field name shows up as a bare `null`. Say which
/// fields the value actually has — that is almost always the whole fix
/// (`.status` vs `._status`, `.url` on a try-result vs `.value.url`).
fn explain_absent_field(interp: &mut interpreter::Interpreter, expr: &ast::Expr) -> Option<String> {
    let ast::Expr::FieldAccess { target, field } = expr else { return None };
    if !matches!(eval_test_expr(interp, expr), Ok(interpreter::Value::Unit)) {
        return None;
    }
    let interpreter::Value::Map(entries) = eval_test_expr(interp, &target.node).ok()? else { return None };
    let keys: Vec<String> = entries.iter().map(|(k, _)| k.clone()).collect();
    if keys.iter().any(|k| k == field) {
        return None; // the field exists and really is ()
    }
    let near = keys
        .iter()
        .find(|k| k.trim_start_matches('_') == field.as_str() || **k == format!("_{field}"))
        .map(|k| format!(" — did you mean '.{k}'?"))
        .unwrap_or_default();
    Some(format!("note:  '.{field}' is absent; the value has fields [{}]{near}", keys.join(", ")))
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

/// V1.6: run a property-based test. Draw `count` random integers from
/// `[lo, hi]`, bind `var`, evaluate the postcondition, expect Bool(true).
/// Returns Ok(None) on universal pass, Ok(Some(cex)) on first failure.
fn run_property(
    interp: &mut interpreter::Interpreter,
    _name: &str,
    var: &str,
    ty: &str,
    lo: i64,
    hi: i64,
    count: u32,
    body: &ast::Expr,
) -> Result<Option<String>, String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Simple linear congruential RNG seeded by the wall clock, kept
    // small so we don't bring in a `rand` dependency.
    let mut state: u64 = SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64).unwrap_or(1);
    let next = |state: &mut u64| -> u64 {
        *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *state
    };
    if ty != "Int" {
        return Err(format!("only Int properties supported in V1.6 (got {})", ty));
    }
    if hi <= lo { return Err(format!("range {}..{} is empty", lo, hi)); }
    let span = (hi - lo) as u64;
    for _ in 0..count {
        let r = (next(&mut state) % span) as i64 + lo;
        let mut env = std::collections::HashMap::new();
        env.insert(var.to_string(), interpreter::Value::Int(interpreter::SomaInt::from_i64(r)));
        let v = interp.eval_expr_with_env(body, &env, "", "")
            .map_err(|e| format!("{:?}", e))?;
        if !v.is_truthy() {
            return Ok(Some(r.to_string()));
        }
    }
    Ok(None)
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
