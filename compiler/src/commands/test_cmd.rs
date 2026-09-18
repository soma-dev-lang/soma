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

    // Tests of a program that fails `soma check` mislead: a duplicate handler
    // or a non-exhaustive match used to run anyway ("last definition wins").
    {
        let mut chk = crate::checker::Checker::new(registry);
        chk.source = Some((file_str.clone(), source.clone()));
        chk.check(&program);
        if chk.has_errors() {
            eprintln!("{} fails `soma check` — fix these before running its tests:", path.display());
            for line in chk.report().lines().filter(|l| !l.starts_with("warning") && !l.starts_with("advisory") && !l.starts_with("✓")) {
                eprintln!("  {}", line);
            }
            process::exit(1);
        }
    }

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

    interp.test_auto_mock = true;

    // [native] handlers run natively under test too — otherwise an assertion
    // comparing a native handler with its interpreted twin compares the
    // interpreter with itself and proves nothing about the compiled code.
    let parallel_config = crate::codegen::native::ParallelConfig::default();
    match interpreter::native_ffi::compile_and_load_natives_with_config(&program, &parallel_config) {
        Ok(natives) => interp.native_handlers = natives,
        Err(e) => {
            eprintln!("warning: [native] handlers could not be compiled — they run INTERPRETED in this test run");
            for line in e.lines().take(3) {
                eprintln!("  {}", line);
            }
        }
    }

    for test_cell in &test_cells {
        println!("test {} ...", test_cell.name);
        // `let` rules bind here; every later rule of the cell sees them
        let mut test_env: std::collections::HashMap<String, interpreter::Value> = std::collections::HashMap::new();
        interp.mock_queue.clear();
        interp.approve_queue.clear();

        for section in &test_cell.sections {
            if let ast::Section::Rules(ref rules) = section.node {
                for rule in &rules.rules {
                    match &rule.node {
                        ast::Rule::Let { name, value } => {
                            match eval_test_expr(&mut interp, &value.node, &test_env) {
                                Ok(v) => {
                                    test_env.insert(name.clone(), v);
                                }
                                Err(e) => {
                                    total += 1;
                                    failed += 1;
                                    println!("  ✗ {}:{}  let {} = … — ERROR: {}", file_name, line_of(rule.span), name, e);
                                }
                            }
                        }
                        ast::Rule::MockThink { reply, is_error } => {
                            match eval_test_expr(&mut interp, &reply.node, &test_env) {
                                Ok(interpreter::Value::List(items)) => {
                                    for it in items {
                                        let text = format!("{}", it);
                                        interp.mock_queue.push_back(if *is_error { Err(text) } else { Ok(text) });
                                    }
                                }
                                Ok(v) => {
                                    let text = format!("{}", v);
                                    interp.mock_queue.push_back(if *is_error { Err(text) } else { Ok(text) });
                                }
                                Err(e) => {
                                    total += 1;
                                    failed += 1;
                                    println!("  ✗ {}:{}  mock think … — ERROR: {}", file_name, line_of(rule.span), e);
                                }
                            }
                        }
                        ast::Rule::MockApprove { reply } => {
                            match eval_test_expr(&mut interp, &reply.node, &test_env) {
                                Ok(interpreter::Value::List(items)) => {
                                    for it in items {
                                        interp.approve_queue.push_back(it.is_truthy());
                                    }
                                }
                                Ok(v) => interp.approve_queue.push_back(v.is_truthy()),
                                Err(e) => {
                                    total += 1;
                                    failed += 1;
                                    println!("  ✗ {}:{}  mock approve … — ERROR: {}", file_name, line_of(rule.span), e);
                                }
                            }
                        }
                        ast::Rule::Assert(expr) => {
                            total += 1;

                            let shown = text_of(expr.span, format_expr(&expr.node));
                            match eval_test_assertion(&mut interp, &expr.node, &test_env) {
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
                        ast::Rule::AssertFails(_) | ast::Rule::AssertFailsMatching(..) => {
                            total += 1;
                            let (expr, wanted) = match &rule.node {
                                ast::Rule::AssertFails(e) => (e, None),
                                ast::Rule::AssertFailsMatching(e, t) => (e, Some(t.as_str())),
                                _ => unreachable!(),
                            };
                            let shown = text_of(expr.span, format_expr(&expr.node));
                            let at = format!("{}:{}", file_name, line_of(expr.span));

                            match eval_test_expr(&mut interp, &expr.node, &test_env) {
                                // A typo'd name is NOT the failure under test —
                                // a vacuously green assert_fails would hide it
                                // forever. Undefined functions and variables
                                // are bugs (`soma check` reports both), never
                                // the domain error a test means to prove.
                                Err(e) if e.starts_with("undefined function") || e.starts_with("undefined variable") => {
                                    failed += 1;
                                    println!("  ✗ {}  assert_fails {} — FAILED: it raised {}, a bug rather than the failure under test (run `soma check`)",
                                             at, shown, e);
                                }
                                Err(e) => match wanted {
                                    Some(text) if !e.contains(text) => {
                                        failed += 1;
                                        println!("  ✗ {}  assert_fails {} matching \"{}\" — FAILED: it raised something else: {}",
                                                 at, shown, text, e);
                                    }
                                    _ => {
                                        passed += 1;
                                        match wanted {
                                            Some(text) => println!("  ✓ assert_fails {} matching \"{}\" — raised {}", shown, text, e),
                                            None => println!("  ✓ assert_fails {} — raised {}", shown, e),
                                        }
                                    }
                                },
                                Ok(v) => {
                                    failed += 1;
                                    println!("  ✗ {}  assert_fails {} — FAILED: expected a runtime error, but it succeeded with {}",
                                             at, shown, v);
                                }
                            }
                        }
                        ast::Rule::Property { name, var, ty, lo, hi, count, body } => {
                            total += 1;
                            match run_property(&mut interp, name, var, ty, *lo, *hi, *count, &body.node) {
                                Ok((None, cov)) => {
                                    passed += 1;
                                    println!("  ✓ property \"{}\" (forall {} in {}..{}: {})",
                                             name, var, lo, hi, cov);
                                }
                                Ok((Some(cex), _)) => {
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
    env: &std::collections::HashMap<String, interpreter::Value>,
) -> Result<(bool, Vec<String>), String> {
    match expr {
        ast::Expr::CmpOp { left, op, right } => {
            let (left_val, left_note) = eval_side(interp, &left.node, env)?;
            let (right_val, right_note) = eval_side(interp, &right.node, env)?;

            let result = interp.eval_cmpop_values(&left_val, op.clone(), &right_val)
                .unwrap_or(false);

            let mut detail = Vec::new();
            if !result {
                detail.push(format!("left:  {}", left_val));
                detail.push(format!("right: {}", right_val));
                detail.extend(left_note);
                detail.extend(right_note);
            }

            Ok((result, detail))
        }
        _ => {
            let val = eval_test_expr(interp, expr, env)?;
            // `assert "false"` and `assert some_map` used to pass (truthy):
            // an assertion is a Bool or it is a mistake.
            match val {
                interpreter::Value::Bool(b) => Ok((b, Vec::new())),
                other => Err(format!(
                    "assert needs a Bool, got {} {} — compare it: `assert x == …`",
                    interpreter::value_type_name(&other), other
                )),
            }
        }
    }
}

/// Evaluate one side of a comparison ONCE (handlers have side effects). For
/// `x.field` on a map that lacks the field, also explain the resulting `()`:
/// reading a missing key is not an error in Soma, so a wrong field name shows
/// up as a bare `null`. Naming the fields that exist is almost always the
/// whole fix (`.status` vs `._status`, `.url` on a try-result vs `.value.url`).
fn eval_side(
    interp: &mut interpreter::Interpreter,
    expr: &ast::Expr,
    env: &std::collections::HashMap<String, interpreter::Value>,
) -> Result<(interpreter::Value, Option<String>), String> {
    let ast::Expr::FieldAccess { target, field } = expr else {
        return Ok((eval_test_expr(interp, expr, env)?, None));
    };
    // matrices answer .T / .shape, variants have payload fields: let the
    // interpreter handle anything that is not a plain map
    if matches!(field.as_str(), "T" | "shape") {
        return Ok((eval_test_expr(interp, expr, env)?, None));
    }
    let base = eval_test_expr(interp, &target.node, env)?;
    let interpreter::Value::Map(entries) = &base else {
        let mut scope = env.clone();
        scope.insert("__test_base".to_string(), base);
        let rebuilt = ast::Expr::FieldAccess {
            target: Box::new(ast::Spanned::new(ast::Expr::Ident("__test_base".to_string()), target.span)),
            field: field.clone(),
        };
        return Ok((eval_test_expr(interp, &rebuilt, &scope)?, None));
    };
    if let Some(v) = entries.get(field) {
        return Ok((v.clone(), None));
    }
    let keys: Vec<String> = entries.keys().cloned().collect();
    let near = keys
        .iter()
        .find(|k| k.trim_start_matches('_') == field.as_str())
        .map(|k| format!(" — did you mean '.{k}'?"))
        .unwrap_or_default();
    let note = format!("note:  '.{field}' is absent; the value has fields [{}]{near}", keys.join(", "));
    Ok((interpreter::Value::Unit, Some(note)))
}

fn eval_test_expr(
    interp: &mut interpreter::Interpreter,
    expr: &ast::Expr,
    env: &std::collections::HashMap<String, interpreter::Value>,
) -> Result<interpreter::Value, String> {
    // Delegate to the real interpreter for full expression support
    // (pipes, lambdas, match, field access, method calls, etc.)
    // errors are shown the way the language words them, not as a Rust
    // Debug dump (`Runtime(RequireFailed("…"))`)
    interp.eval_expr_with_env(expr, env, "", "").map_err(|e| describe_error(&e))
}

/// V1.6: run a property-based test. Draw `count` random integers from
/// `[lo, hi]`, bind `var`, evaluate the postcondition, expect Bool(true).
/// Returns Ok(None) on universal pass, Ok(Some(cex)) on first failure.
/// How a `forall` property was evaluated, for the report line.
enum Coverage { Exhaustive(u64), Sampled(u32, u64) }

impl std::fmt::Display for Coverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Coverage::Exhaustive(n) => write!(f, "all {} values", n),
            Coverage::Sampled(k, n) => write!(f, "{} of {} values sampled, both bounds included, fixed seed — NOT a proof", k, n),
        }
    }
}

fn run_property(
    interp: &mut interpreter::Interpreter,
    _name: &str,
    var: &str,
    ty: &str,
    lo: i64,
    hi: i64,
    count: u32,
    body: &ast::Expr,
) -> Result<(Option<String>, Coverage), String> {
    if ty != "Int" {
        return Err(format!("only Int properties supported in V1.6 (got {})", ty));
    }
    if hi <= lo { return Err(format!("range {}..{} is empty", lo, hi)); }
    let span = (hi - lo) as u64;
    // Every value when the range is small enough to walk: a universally
    // quantified claim over 100 values used to be 50 random draws that
    // could say ✓ on one run and ✗ on the next. Larger ranges are sampled
    // with a FIXED seed (same verdict on every run) and both bounds.
    const EXHAUSTIVE_MAX: u64 = 20_000;
    let exhaustive = span <= EXHAUSTIVE_MAX.max(count as u64);
    let values: Vec<i64> = if exhaustive {
        (lo..hi).collect()
    } else {
        let mut state: u64 = 0x5eed_0000_0000_0001 ^ (span.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut next = || -> u64 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            state >> 11
        };
        let mut v: Vec<i64> = vec![lo, hi - 1];
        while (v.len() as u32) < count.max(2) {
            v.push((next() % span) as i64 + lo);
        }
        v
    };
    let coverage = if exhaustive { Coverage::Exhaustive(span) } else { Coverage::Sampled(values.len() as u32, span) };
    for r in values {
        let mut env = std::collections::HashMap::new();
        env.insert(var.to_string(), interpreter::Value::Int(interpreter::SomaInt::from_i64(r)));
        let v = interp.eval_expr_with_env(body, &env, "", "")
            .map_err(|e| describe_error(&e))?;
        if !v.is_truthy() {
            return Ok((Some(r.to_string()), coverage));
        }
    }
    Ok((None, coverage))
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

/// An execution error in the language's own words.
fn describe_error(e: &interpreter::ExecError) -> String {
    match e {
        interpreter::ExecError::Runtime(r) => r.to_string(),
        other => format!("{:?}", other),
    }
}
