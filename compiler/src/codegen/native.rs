//! Generates Rust source from [native] handler AST.
//!
//! Two codegen modes:
//!   - **Direct**: Int=i64, Float=f64, Bool=bool. Direct C ABI. Fast for
//!     bounded-range numerics (Monte Carlo, fib(90), counters, etc.).
//!   - **Rug**: Int=rug::Integer. Shared buffer FFI. For BigInt-heavy
//!     workloads (PI spigot, arbitrary precision).
//!
//! Mode selection: handlers that return `String` use Rug mode (the canonical
//! "BigInt-as-string" output pattern). Everything else uses Direct mode.

use crate::ast::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

// ── Public API ──────────────────────────────────────────────────────

/// Type inferred for a local variable
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NativeType {
    Int,
    Float,
    Bool,
    String,
}

impl NativeType {
    fn rust_str(&self) -> &'static str {
        match self {
            NativeType::Int => "i64",
            NativeType::Float => "f64",
            NativeType::Bool => "bool",
            NativeType::String => "String",
        }
    }
}

/// Information about a native handler to be compiled
pub struct NativeHandler {
    pub cell_name: String,
    pub signal_name: String,
    pub params: Vec<Param>,
    pub body: Vec<Spanned<Statement>>,
    /// Handler properties (e.g. ["native"])
    pub properties: Vec<String>,
}

/// FFI signature info: param types + return type
#[derive(Debug, Clone)]
pub struct NativeSig {
    pub fn_name: String,
    pub param_types: Vec<NativeType>,
    pub return_type: NativeType,
    /// If true, args are passed via shared buffer (_soma_push_*), not C params
    pub uses_shared_args: bool,
}

/// Parallel configuration for code generation
#[derive(Debug, Clone, Default)]
pub struct ParallelConfig {
    pub enabled: bool,
    pub handlers: Vec<String>,
    pub threads: usize,
}

/// Codegen mode for a single handler
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    /// Int=i64, Float=f64, Bool=bool. Direct C ABI.
    Direct,
    /// Int=rug::Integer. Shared buffer FFI.
    Rug,
}

/// Generate Rust source for a set of native handlers in one cell.
pub fn generate_native_source(handlers: &[NativeHandler]) -> (String, Vec<NativeSig>) {
    generate_native_source_with_config(handlers, &ParallelConfig::default())
}

pub fn generate_native_source_with_config(
    handlers: &[NativeHandler],
    parallel: &ParallelConfig,
) -> (String, Vec<NativeSig>) {
    let mut out = String::new();
    let mut sigs = Vec::new();

    let siblings: HashSet<String> = handlers.iter().map(|h| h.signal_name.clone()).collect();

    // Pre-compute rough sibling return types BEFORE mode selection so that
    // select_mode's classifier sees the correct type for sibling FnCalls
    // (otherwise unknown sibling calls default to Float, masking Int-overflow
    // patterns like `n * fact_rec(n - 1)`). Mode is unknown at this stage so
    // we use Direct as a placeholder.
    let mut pre_sibling_info: HashMap<String, SiblingInfo> = HashMap::new();
    for handler in handlers {
        let param_types: Vec<NativeType> = handler.params.iter()
            .map(|p| type_expr_to_native(&p.ty.node))
            .collect();
        pre_sibling_info.insert(handler.signal_name.clone(), SiblingInfo {
            param_types,
            return_type: NativeType::Float,
            mode: Mode::Direct,
        });
    }
    loop {
        let prev = pre_sibling_info.clone();
        for handler in handlers {
            let mut tmp = FnGenerator::new(&handler.params, &siblings, Mode::Direct)
                .with_sibling_info(pre_sibling_info.clone());
            for p in &handler.params {
                tmp.var_types.insert(p.name.clone(), type_expr_to_native(&p.ty.node));
            }
            tmp.infer_body_types(&handler.body);
            let return_type = tmp.infer_return_type(&handler.body);
            if let Some(info) = pre_sibling_info.get_mut(&handler.signal_name) {
                info.return_type = return_type;
            }
        }
        if prev == pre_sibling_info { break; }
    }

    // Decide mode for each handler
    let mut modes: Vec<Mode> = handlers.iter()
        .map(|h| select_mode(h, &siblings, &pre_sibling_info))
        .collect();
    // Propagate Rug-mode through sibling-call chains. Two directions:
    //   (1) Direct handler calling a Rug sibling returning Int → could receive
    //       a BigInt result that doesn't fit i64 → promote caller to Rug.
    //   (2) Rug handler calling a Direct sibling with an Int argument → could
    //       pass a BigInt that doesn't fit i64 → promote callee to Rug.
    // Iterate both to fixpoint.
    let int_param_handlers: HashSet<String> = handlers.iter()
        .filter(|h| h.params.iter().any(|p| type_expr_to_native(&p.ty.node) == NativeType::Int))
        .map(|h| h.signal_name.clone())
        .collect();
    loop {
        let mut changed = false;
        let mode_of: HashMap<&str, Mode> = handlers.iter().enumerate()
            .map(|(j, h)| (h.signal_name.as_str(), modes[j]))
            .collect();
        // (1) Direct → calls-Rug-Int-sibling → promote
        for i in 0..handlers.len() {
            if modes[i] == Mode::Rug { continue; }
            if calls_rug_int_sibling(&handlers[i].body, &mode_of, &siblings) {
                modes[i] = Mode::Rug;
                changed = true;
            }
        }
        // (2) For each Direct handler that takes Int params: if any Rug
        //     handler calls it, promote it to Rug.
        for i in 0..handlers.len() {
            if modes[i] == Mode::Rug { continue; }
            let name = &handlers[i].signal_name;
            if !int_param_handlers.contains(name) { continue; }
            let called_by_rug = handlers.iter().enumerate().any(|(j, h)| {
                modes[j] == Mode::Rug && body_calls(&h.body, name)
            });
            if called_by_rug {
                modes[i] = Mode::Rug;
                changed = true;
            }
        }
        // (3) Recursive Fibonacci-style: any handler that passes a 'a + b'
        //     style expression to a sibling as an Int argument → that sibling
        //     receives a potentially-unbounded value → promote it to Rug.
        for i in 0..handlers.len() {
            if modes[i] == Mode::Rug { continue; }
            let name = &handlers[i].signal_name;
            if !int_param_handlers.contains(name) { continue; }
            let called_with_unbounded_arg = handlers.iter().any(|h| {
                body_calls_with_unbounded_arg(&h.body, name)
            });
            if called_with_unbounded_arg {
                modes[i] = Mode::Rug;
                changed = true;
            }
        }
        if !changed { break; }
    }

    // ── Dual-mode eligibility ────────────────────────────────────────
    //
    // The philosophy: a type is a promise about behavior, not a choice of
    // representation. The user writes `Int` and the compiler picks i64 or
    // BigInt. So we generate BOTH versions for every handler: a fast
    // Direct one (i64/f64/bool throughout) and a Rug fallback (Integer
    // for unbounded ints). The dispatch wrapper tries fast first; on
    // overflow (which panics with overflow-checks=true), it catches the
    // panic and re-runs in the Rug version transparently.
    //
    // Universal: every handler is dual-mode. There are no exceptions.
    // Strings flow through the shared buffer either way; Direct codegen
    // handles String params/returns natively. The classifier no longer
    // needs to be perfect — when it's wrong, the runtime catches it.
    let dualmode: HashSet<String> = handlers.iter()
        .map(|h| h.signal_name.clone())
        .collect();

    // For dual-mode handlers, force the primary (fallback) mode to Rug.
    // The fast path is generated separately and tried first at runtime.
    // (A handler classified Direct that's now dualmode just has the
    // classifier's old result as its fast path.)
    for (i, h) in handlers.iter().enumerate() {
        if dualmode.contains(&h.signal_name) {
            modes[i] = Mode::Rug;
        }
    }

    let any_rug = modes.contains(&Mode::Rug);
    let uses_random = handlers.iter().any(|h| body_uses_random(&h.body));

    // Pre-compute sibling info: param types, return type, mode.
    // Iterate to fixpoint so handlers that call siblings get correct return types.
    let mut sibling_info: HashMap<String, SiblingInfo> = HashMap::new();
    for (handler, mode) in handlers.iter().zip(modes.iter()) {
        let param_types: Vec<NativeType> = handler.params.iter()
            .map(|p| type_expr_to_native(&p.ty.node))
            .collect();
        sibling_info.insert(handler.signal_name.clone(), SiblingInfo {
            param_types,
            return_type: NativeType::Float,  // placeholder, refined below
            mode: *mode,
        });
    }
    // Fixpoint pass to refine return types — handles mutual recursion / sibling chains
    loop {
        let prev = sibling_info.clone();
        for (handler, mode) in handlers.iter().zip(modes.iter()) {
            let mut tmp = FnGenerator::new(&handler.params, &siblings, *mode)
                .with_sibling_info(sibling_info.clone());
            for p in &handler.params {
                tmp.var_types.insert(p.name.clone(), type_expr_to_native(&p.ty.node));
            }
            tmp.infer_body_types(&handler.body);
            let return_type = tmp.infer_return_type(&handler.body);
            if let Some(info) = sibling_info.get_mut(&handler.signal_name) {
                info.return_type = return_type;
            }
        }
        if prev == sibling_info { break; }
    }

    // ── Preamble ──
    out.push_str("// Generated by Soma compiler — [native] handlers\n");
    out.push_str("// Do not edit — regenerate from .cell source\n\n");

    if any_rug {
        out.push_str("use rug::Integer;\n");
        out.push_str("use rug::Assign;\n\n");
    }

    if uses_random {
        out.push_str("static mut _SOMA_RNG_STATE: u64 = 0x12345678_9abcdef0;\n\n");
        out.push_str("#[inline(always)]\nunsafe fn _soma_random() -> f64 {\n");
        out.push_str("    _SOMA_RNG_STATE ^= _SOMA_RNG_STATE >> 12;\n");
        out.push_str("    _SOMA_RNG_STATE ^= _SOMA_RNG_STATE << 25;\n");
        out.push_str("    _SOMA_RNG_STATE ^= _SOMA_RNG_STATE >> 27;\n");
        out.push_str("    let r = _SOMA_RNG_STATE.wrapping_mul(0x2545F4914F6CDD1D);\n");
        out.push_str("    (r >> 11) as f64 / ((1u64 << 53) as f64)\n");
        out.push_str("}\n\n");
    }

    // ── Shared buffer for Rug mode ──
    if any_rug {
        out.push_str(SHARED_BUFFER_RUG);
    }

    // ── Quiet panic hook for dual-mode fast-path overflows ──
    //
    // The fast path uses i64 arithmetic with overflow-checks=true. When
    // overflow happens it panics, gets caught by catch_unwind in the
    // dispatch wrapper, and we transparently fall back to Rug. The user
    // shouldn't see anything — but the default panic hook prints a
    // backtrace to stderr. Install a no-op hook on first call so the
    // fallback is silent.
    if !dualmode.is_empty() {
        out.push_str("static _SOMA_PANIC_HOOK_INSTALLED: std::sync::Once = std::sync::Once::new();\n");
        out.push_str("fn _soma_install_quiet_hook() {\n");
        out.push_str("    _SOMA_PANIC_HOOK_INSTALLED.call_once(|| {\n");
        out.push_str("        std::panic::set_hook(Box::new(|_info| {}));\n");
        out.push_str("    });\n");
        out.push_str("}\n\n");
    }

    // ── Per-handler codegen ──
    let mut all_errors: Vec<String> = Vec::new();
    for (handler, mode) in handlers.iter().zip(modes.iter()) {
        let is_dual = dualmode.contains(&handler.signal_name);

        let mut gen = FnGenerator::new(&handler.params, &siblings, *mode)
            .with_sibling_info(sibling_info.clone())
            .with_handler_name(handler.signal_name.clone())
            .with_dualmode_siblings(dualmode.clone());
        for p in &handler.params {
            gen.var_types.insert(p.name.clone(), type_expr_to_native(&p.ty.node));
        }
        gen.infer_body_types(&handler.body);
        let int_params: HashSet<String> = handler.params.iter()
            .filter(|p| type_expr_to_native(&p.ty.node) == NativeType::Int)
            .map(|p| p.name.clone())
            .collect();
        gen.classify_int_vars(&handler.body, &int_params);

        let fn_name = format!("handler_{}", handler.signal_name);
        // Compute the return type using the FULLY populated FnGenerator
        // (with sibling_info) so Return statements get the right coercion target.
        let ret_type = gen.infer_return_type(&handler.body);
        gen.fn_return_type = ret_type;

        match mode {
            Mode::Direct => {
                gen_direct_handler(&mut out, &gen, handler, &fn_name, ret_type);
            }
            Mode::Rug if is_dual => {
                // Dual-mode handler: emit BOTH a fast Direct inner function
                // and the standard Rug inner+wrapper, then a custom dispatch
                // wrapper that tries fast first and falls back on overflow.
                //
                // Fast inner: an independent FnGenerator in Direct mode that
                // sees the dualmode_siblings set so its sibling calls go to
                // the _fast variants. Its body is the same AST.
                let mut fast_gen = FnGenerator::new(&handler.params, &siblings, Mode::Direct)
                    .with_sibling_info(sibling_info.clone())
                    .with_handler_name(handler.signal_name.clone())
                    .with_dualmode_siblings(dualmode.clone())
                    .with_fast_variant(true);
                for p in &handler.params {
                    fast_gen.var_types.insert(p.name.clone(), type_expr_to_native(&p.ty.node));
                }
                fast_gen.infer_body_types(&handler.body);
                fast_gen.fn_return_type = ret_type;
                let fast_inner_name = format!("{}_fast", fn_name);
                gen_direct_inner_fn(&mut out, &fast_gen, handler, &fast_inner_name, ret_type);

                // Rug inner — the safe fallback
                gen_rug_inner_fn(&mut out, &gen, handler, &fn_name, ret_type);

                // Custom dispatch wrapper: try fast, fall back to rug
                emit_dualmode_wrapper(&mut out, handler, &fn_name, ret_type);

                // Collect fast-gen errors too
                all_errors.extend(fast_gen.errors.into_inner());
            }
            Mode::Rug => {
                gen_rug_handler(&mut out, &gen, handler, &fn_name, ret_type);
            }
        }

        // Build sig + optional parallel/array wrappers
        let param_types: Vec<NativeType> = handler.params.iter()
            .map(|p| type_expr_to_native(&p.ty.node))
            .collect();

        let is_parallel = parallel.enabled && parallel.handlers.contains(&handler.signal_name);
        if is_parallel && !handler.params.is_empty() && *mode == Mode::Direct {
            emit_parallel_wrapper(&mut out, handler, &fn_name, ret_type, parallel);
        }

        match mode {
            Mode::Rug => {
                // Rug handlers always go through shared buffer
                sigs.push(NativeSig {
                    fn_name,
                    param_types,
                    return_type: ret_type,
                    uses_shared_args: true,
                });
            }
            Mode::Direct => {
                if is_parallel {
                    if param_types.len() > 3 {
                        emit_array_wrapper(&mut out, handler, &fn_name, ret_type, true);
                        sigs.push(NativeSig {
                            fn_name: format!("{}_par_arr", fn_name),
                            param_types,
                            return_type: ret_type,
                            uses_shared_args: false,
                        });
                    } else {
                        sigs.push(NativeSig {
                            fn_name: format!("{}_par", fn_name),
                            param_types,
                            return_type: ret_type,
                            uses_shared_args: false,
                        });
                    }
                } else if param_types.len() > 3 {
                    emit_array_wrapper(&mut out, handler, &fn_name, ret_type, false);
                    sigs.push(NativeSig {
                        fn_name: format!("{}_arr", fn_name),
                        param_types,
                        return_type: ret_type,
                        uses_shared_args: false,
                    });
                } else {
                    sigs.push(NativeSig {
                        fn_name,
                        param_types,
                        return_type: ret_type,
                        uses_shared_args: false,
                    });
                }
            }
        }

        // Collect errors from this handler
        all_errors.extend(gen.errors.into_inner());
    }

    // If any handler had errors, embed them as compile_error! macros so the
    // cargo build fails with a clear message that surfaces to the soma user.
    if !all_errors.is_empty() {
        let mut err_block = String::new();
        err_block.push_str("\n// Codegen errors:\n");
        err_block.push_str("const _: () = {\n");
        for e in &all_errors {
            // Escape quotes and backslashes
            let escaped: String = e.replace('\\', "\\\\").replace('"', "\\\"");
            err_block.push_str(&format!("    compile_error!(\"[soma codegen] {}\");\n", escaped));
        }
        err_block.push_str("};\n");
        out.push_str(&err_block);
    }

    (out, sigs)
}

// ── Mode selection ──────────────────────────────────────────────────

/// Decide whether a handler should use Direct or Rug mode.
///
/// `Int` is the only integer type the user sees. The choice of i64 vs
/// arbitrary-precision is the compiler's job, never the user's. Rules:
///   1. If the return type is String → Rug mode (canonical BigInt-as-string).
///   2. Else, run the classifier: if every Int local provably fits i64
///      across the whole function → Direct, else Rug.
///
/// There are deliberately no user-facing escape hatches. A type is a
/// promise about behavior, not a choice of representation. When the
/// classifier picks the wrong mode, that is a compiler bug, not a
/// missing annotation.
fn select_mode(
    handler: &NativeHandler,
    siblings: &HashSet<String>,
    pre_sibling_info: &HashMap<String, SiblingInfo>,
) -> Mode {
    // String parameters can't go through the Direct C ABI — they need the
    // shared-buffer FFI, which only Rug-mode handlers use.
    if handler.params.iter().any(|p| type_expr_to_native(&p.ty.node) == NativeType::String) {
        return Mode::Rug;
    }
    let mut gen = FnGenerator::new(&handler.params, siblings, Mode::Direct)
        .with_sibling_info(pre_sibling_info.clone());
    for p in &handler.params {
        gen.var_types.insert(p.name.clone(), type_expr_to_native(&p.ty.node));
    }
    gen.infer_body_types(&handler.body);
    let ret = gen.infer_return_type(&handler.body);
    if ret == NativeType::String {
        return Mode::Rug;
    }

    // Check if all Int locals (and params) can fit in i64.
    // The mode-select classifier puts params in the optimistic set and
    // assumes sibling calls return bounded values (Direct-mode-compatible).
    let int_vars: HashSet<String> = gen.var_types.iter()
        .filter(|(_, t)| **t == NativeType::Int)
        .map(|(n, _)| n.clone())
        .collect();
    let mut small = int_vars.clone();
    loop {
        let prev = small.clone();
        FnGenerator::run_classifier(&handler.body, &mut small, siblings, Some(&gen.var_types));
        if small == prev { break; }
    }

    if small.len() != int_vars.len() {
        return Mode::Rug;
    }

    // Also check Return statements: a return value that's a var*var
    // product (or otherwise not-bounded) means the result could overflow i64.
    if return_expr_needs_bigint(&handler.body, &small, &gen, siblings) {
        return Mode::Rug;
    }

    Mode::Direct
}

/// True if `body` contains a call to `target` where any argument is an
/// additive expression of two distinct Int identifiers (the Fibonacci
/// recurrence pattern that grows without bound across recursion).
fn body_calls_with_unbounded_arg(body: &[Spanned<Statement>], target: &str) -> bool {
    fn arg_is_unbounded(e: &Expr) -> bool {
        match e {
            Expr::BinaryOp { left, op, right } if matches!(op, BinOp::Add | BinOp::Sub) => {
                let l_ident = matches!(&left.node, Expr::Ident(_));
                let r_ident = matches!(&right.node, Expr::Ident(_));
                if l_ident && r_ident {
                    if let (Expr::Ident(a), Expr::Ident(b)) = (&left.node, &right.node) {
                        if a != b { return true; }
                    }
                }
                arg_is_unbounded(&left.node) || arg_is_unbounded(&right.node)
            }
            _ => false,
        }
    }
    fn expr_walk(e: &Expr, target: &str) -> bool {
        match e {
            Expr::FnCall { name, args } => {
                if name == target && args.iter().any(|a| arg_is_unbounded(&a.node)) {
                    return true;
                }
                args.iter().any(|a| expr_walk(&a.node, target))
            }
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
                expr_walk(&left.node, target) || expr_walk(&right.node, target)
            }
            Expr::Not(inner) => expr_walk(&inner.node, target),
            _ => false,
        }
    }
    fn stmt_walk(s: &Statement, target: &str) -> bool {
        match s {
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => {
                expr_walk(&value.node, target)
            }
            Statement::ExprStmt { expr } => expr_walk(&expr.node, target),
            Statement::If { condition, then_body, else_body } => {
                expr_walk(&condition.node, target)
                    || body_calls_with_unbounded_arg(then_body, target)
                    || body_calls_with_unbounded_arg(else_body, target)
            }
            Statement::While { condition, body } => {
                expr_walk(&condition.node, target)
                    || body_calls_with_unbounded_arg(body, target)
            }
            Statement::For { iter, body, .. } => {
                expr_walk(&iter.node, target)
                    || body_calls_with_unbounded_arg(body, target)
            }
            _ => false,
        }
    }
    body.iter().any(|s| stmt_walk(&s.node, target))
}

/// Collect the names of all sibling handlers called anywhere in `body`.
fn body_siblings_called(body: &[Spanned<Statement>], siblings: &HashSet<String>) -> HashSet<String> {
    let mut acc = HashSet::new();
    fn expr_walk(e: &Expr, siblings: &HashSet<String>, acc: &mut HashSet<String>) {
        match e {
            Expr::FnCall { name, args } => {
                if siblings.contains(name) {
                    acc.insert(name.clone());
                }
                for a in args { expr_walk(&a.node, siblings, acc); }
            }
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
                expr_walk(&left.node, siblings, acc);
                expr_walk(&right.node, siblings, acc);
            }
            Expr::Not(inner) => expr_walk(&inner.node, siblings, acc),
            _ => {}
        }
    }
    fn stmt_walk(s: &Statement, siblings: &HashSet<String>, acc: &mut HashSet<String>) {
        match s {
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => {
                expr_walk(&value.node, siblings, acc);
            }
            Statement::ExprStmt { expr } => expr_walk(&expr.node, siblings, acc),
            Statement::If { condition, then_body, else_body } => {
                expr_walk(&condition.node, siblings, acc);
                for s in then_body { stmt_walk(&s.node, siblings, acc); }
                for s in else_body { stmt_walk(&s.node, siblings, acc); }
            }
            Statement::While { condition, body } => {
                expr_walk(&condition.node, siblings, acc);
                for s in body { stmt_walk(&s.node, siblings, acc); }
            }
            Statement::For { iter, body, .. } => {
                expr_walk(&iter.node, siblings, acc);
                for s in body { stmt_walk(&s.node, siblings, acc); }
            }
            _ => {}
        }
    }
    for s in body { stmt_walk(&s.node, siblings, &mut acc); }
    acc
}

/// True if `body` contains any call to a function named `target`.
fn body_calls(body: &[Spanned<Statement>], target: &str) -> bool {
    fn expr_has_call(e: &Expr, target: &str) -> bool {
        match e {
            Expr::FnCall { name, args } => {
                name == target || args.iter().any(|a| expr_has_call(&a.node, target))
            }
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
                expr_has_call(&left.node, target) || expr_has_call(&right.node, target)
            }
            Expr::Not(inner) => expr_has_call(&inner.node, target),
            _ => false,
        }
    }
    fn stmt_has_call(s: &Statement, target: &str) -> bool {
        match s {
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => {
                expr_has_call(&value.node, target)
            }
            Statement::ExprStmt { expr } => expr_has_call(&expr.node, target),
            Statement::If { condition, then_body, else_body } => {
                expr_has_call(&condition.node, target)
                    || body_calls(then_body, target)
                    || body_calls(else_body, target)
            }
            Statement::While { condition, body } => {
                expr_has_call(&condition.node, target) || body_calls(body, target)
            }
            Statement::For { iter, body, .. } => {
                expr_has_call(&iter.node, target) || body_calls(body, target)
            }
            _ => false,
        }
    }
    body.iter().any(|s| stmt_has_call(&s.node, target))
}

/// True if `body` contains a call (anywhere) to a sibling handler that is
/// currently classified as Rug-mode and whose static return type is Int.
/// Such a result may be a BigInt that doesn't fit i64, so the caller can't
/// safely stay in Direct mode.
fn calls_rug_int_sibling(
    body: &[Spanned<Statement>],
    mode_of: &HashMap<&str, Mode>,
    siblings: &HashSet<String>,
) -> bool {
    fn expr_calls(
        expr: &Expr,
        mode_of: &HashMap<&str, Mode>,
        siblings: &HashSet<String>,
    ) -> bool {
        match expr {
            Expr::FnCall { name, args } => {
                if siblings.contains(name) && mode_of.get(name.as_str()).copied() == Some(Mode::Rug) {
                    return true;
                }
                args.iter().any(|a| expr_calls(&a.node, mode_of, siblings))
            }
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
                expr_calls(&left.node, mode_of, siblings) || expr_calls(&right.node, mode_of, siblings)
            }
            Expr::Not(inner) => expr_calls(&inner.node, mode_of, siblings),
            _ => false,
        }
    }
    fn stmt_calls(
        stmt: &Statement,
        mode_of: &HashMap<&str, Mode>,
        siblings: &HashSet<String>,
    ) -> bool {
        match stmt {
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => {
                expr_calls(&value.node, mode_of, siblings)
            }
            Statement::ExprStmt { expr } => expr_calls(&expr.node, mode_of, siblings),
            Statement::If { condition, then_body, else_body } => {
                expr_calls(&condition.node, mode_of, siblings)
                    || calls_rug_int_sibling(then_body, mode_of, siblings)
                    || calls_rug_int_sibling(else_body, mode_of, siblings)
            }
            Statement::While { condition, body } => {
                expr_calls(&condition.node, mode_of, siblings)
                    || calls_rug_int_sibling(body, mode_of, siblings)
            }
            Statement::For { iter, body, .. } => {
                expr_calls(&iter.node, mode_of, siblings)
                    || calls_rug_int_sibling(body, mode_of, siblings)
            }
            _ => false,
        }
    }
    body.iter().any(|s| stmt_calls(&s.node, mode_of, siblings))
}

/// Walk the body looking for Return statements whose value isn't bounded.
fn return_expr_needs_bigint(
    body: &[Spanned<Statement>],
    small: &HashSet<String>,
    gen: &FnGenerator,
    siblings: &HashSet<String>,
) -> bool {
    for stmt in body {
        match &stmt.node {
            Statement::Return { value } => {
                let ty = gen.infer_expr_type(&value.node);
                if ty == NativeType::Int
                    && !FnGenerator::is_bounded_expr(
                        &value.node, small, siblings, Some(&gen.var_types))
                {
                    return true;
                }
            }
            Statement::If { then_body, else_body, .. } => {
                if return_expr_needs_bigint(then_body, small, gen, siblings) { return true; }
                if return_expr_needs_bigint(else_body, small, gen, siblings) { return true; }
            }
            Statement::While { body, .. } | Statement::For { body, .. } => {
                if return_expr_needs_bigint(body, small, gen, siblings) { return true; }
            }
            _ => {}
        }
    }
    false
}

// ── Direct mode handler ─────────────────────────────────────────────
//
// Direct mode generates two functions:
//   inner_handler_X(typed args) -> typed return  — pure Rust, sibling-call target
//   handler_X(...)              -> ...           — FFI entry point, just calls inner_X

fn gen_direct_handler(
    out: &mut String,
    gen: &FnGenerator,
    handler: &NativeHandler,
    fn_name: &str,
    ret_type: NativeType,
) {
    gen_direct_inner_fn(out, gen, handler, fn_name, ret_type);

    // FFI entry — thin shim
    let param_str: String = handler.params.iter()
        .map(|p| {
            let ty = type_expr_to_native(&p.ty.node);
            format!("{}: {}", p.name, ty.rust_str())
        })
        .collect::<Vec<_>>()
        .join(", ");
    let arg_names: String = handler.params.iter()
        .map(|p| p.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "#[no_mangle]\npub extern \"C\" fn {}({}) -> {} {{\n    inner_{}({})\n}}\n\n",
        fn_name, param_str, ret_type.rust_str(), fn_name, arg_names
    ));
}

/// Emit only the typed inner function for a Direct-mode handler — no FFI
/// wrapper. Used both by gen_direct_handler and by the dual-mode emitter
/// that needs a fast Direct version alongside a separate Rug fallback.
fn gen_direct_inner_fn(
    out: &mut String,
    gen: &FnGenerator,
    handler: &NativeHandler,
    fn_name: &str,
    ret_type: NativeType,
) {
    let param_str: String = handler.params.iter()
        .map(|p| {
            let ty = type_expr_to_native(&p.ty.node);
            // Params are always declared `mut` — Soma allows assigning to a
            // parameter, and the body codegen emits it as a normal variable.
            format!("mut {}: {}", p.name, ty.rust_str())
        })
        .collect::<Vec<_>>()
        .join(", ");

    out.push_str(&format!(
        "fn inner_{}({}) -> {} {{\n",
        fn_name, param_str, ret_type.rust_str()
    ));
    for stmt in &handler.body {
        out.push_str(&gen.gen_stmt_direct(&stmt.node, 1));
    }
    let last_is_return = handler.body.last()
        .map(|s| matches!(s.node, Statement::Return { .. }))
        .unwrap_or(false);
    if !last_is_return {
        let default = match ret_type {
            NativeType::Float => "    0.0f64\n",
            NativeType::Bool => "    false\n",
            NativeType::String => "    String::new()\n",
            NativeType::Int => "    0i64\n",
        };
        out.push_str(default);
    }
    out.push_str("}\n\n");
}

// ── Rug mode handler ────────────────────────────────────────────────
//
// Rug mode generates two functions:
//   inner_handler_X(Integer args, f64, ...) -> Integer/String/f64/bool
//                                         — pure Rust, sibling-call target
//   handler_X() -> i64                    — FFI entry point with shared buffer

fn gen_rug_handler(
    out: &mut String,
    gen: &FnGenerator,
    handler: &NativeHandler,
    fn_name: &str,
    ret_type: NativeType,
) {
    gen_rug_inner_fn(out, gen, handler, fn_name, ret_type);
    gen_rug_ffi_wrapper(out, handler, fn_name, ret_type);
}

/// Emit just the typed Rug inner function (no FFI wrapper). Used by both
/// gen_rug_handler and the dual-mode emitter that generates a Rug fallback
/// alongside a fast Direct path.
fn gen_rug_inner_fn(
    out: &mut String,
    gen: &FnGenerator,
    handler: &NativeHandler,
    fn_name: &str,
    ret_type: NativeType,
) {
    let inner_ret = match ret_type {
        NativeType::Int => "Integer".to_string(),
        NativeType::String => "String".to_string(),
        other => other.rust_str().to_string(),
    };

    // For Int params we need them mutable in case the body assigns to them.
    let mut_param_str: String = handler.params.iter()
        .map(|p| {
            let ty = type_expr_to_native(&p.ty.node);
            let rust_ty = if ty == NativeType::Int { "Integer".to_string() } else { ty.rust_str().to_string() };
            format!("mut {}: {}", p.name, rust_ty)
        })
        .collect::<Vec<_>>()
        .join(", ");

    out.push_str(&format!("fn inner_{}({}) -> {} {{\n", fn_name, mut_param_str, inner_ret));
    let ctx = RugCtx::new(ret_type);
    out.push_str(&gen.gen_body_rug(&handler.body, 1, &ctx));
    let last_is_return = handler.body.last()
        .map(|s| matches!(s.node, Statement::Return { .. }))
        .unwrap_or(false);
    if !last_is_return {
        match ret_type {
            NativeType::Int => out.push_str("    Integer::new()\n"),
            NativeType::String => out.push_str("    String::new()\n"),
            NativeType::Float => out.push_str("    0.0f64\n"),
            NativeType::Bool => out.push_str("    false\n"),
        }
    }
    out.push_str("}\n\n");
}

/// Emit the dual-mode dispatch wrapper for a handler that has both a fast
/// Direct inner (`inner_handler_X_fast`) and a Rug fallback inner
/// (`inner_handler_X`). The wrapper:
///   1. Reads args from the shared buffer.
///   2. Tries to convert each Int arg to i64. If any doesn't fit, skip the
///      fast path entirely and call Rug.
///   3. Calls the fast inner inside `catch_unwind` so an i64 overflow
///      panic doesn't crash the dylib.
///   4. On panic OR if any arg didn't fit, calls the Rug inner.
///   5. Packs the result the same way the Rug FFI wrapper would.
fn emit_dualmode_wrapper(
    out: &mut String,
    handler: &NativeHandler,
    fn_name: &str,
    ret_type: NativeType,
) {
    // Build expressions for fast-path arg decoding (each as Option<T>) and
    // rug-path arg expressions (each unconditionally typed).
    let mut fast_decode_lines: Vec<String> = Vec::new();
    let mut fast_call_args: Vec<String> = Vec::new();
    let mut rug_call_args: Vec<String> = Vec::new();
    let mut int_idx = 0;
    let mut str_idx = 0;
    for p in &handler.params {
        let ty = type_expr_to_native(&p.ty.node);
        let local = format!("_fast_arg_{}", p.name);
        match ty {
            NativeType::Int => {
                fast_decode_lines.push(format!(
                    "        let {} = match unsafe {{ _SOMA_ARGS[{}].to_i64() }} {{ Some(v) => v, None => return None }};",
                    local, int_idx
                ));
                fast_call_args.push(local);
                rug_call_args.push(format!("unsafe {{ _SOMA_ARGS[{}].clone() }}", int_idx));
                int_idx += 1;
            }
            NativeType::Float => {
                fast_decode_lines.push(format!(
                    "        let {} = unsafe {{ f64::from_bits(_SOMA_ARGS[{}].to_i64().unwrap_or(0) as u64) }};",
                    local, int_idx
                ));
                fast_call_args.push(local);
                rug_call_args.push(format!(
                    "unsafe {{ f64::from_bits(_SOMA_ARGS[{}].to_i64().unwrap_or(0) as u64) }}",
                    int_idx
                ));
                int_idx += 1;
            }
            NativeType::Bool => {
                fast_decode_lines.push(format!(
                    "        let {} = unsafe {{ _SOMA_ARGS[{}].to_i64().unwrap_or(0) != 0 }};",
                    local, int_idx
                ));
                fast_call_args.push(local);
                rug_call_args.push(format!(
                    "unsafe {{ _SOMA_ARGS[{}].to_i64().unwrap_or(0) != 0 }}",
                    int_idx
                ));
                int_idx += 1;
            }
            NativeType::String => {
                // Dual-mode handlers shouldn't have String params (eligibility
                // forbids it). Defensive: emit something compilable.
                fast_decode_lines.push(format!(
                    "        let {} = unsafe {{ _SOMA_STRING_ARGS[{}].clone() }};",
                    local, str_idx
                ));
                fast_call_args.push(local);
                rug_call_args.push(format!("unsafe {{ _SOMA_STRING_ARGS[{}].clone() }}", str_idx));
                str_idx += 1;
            }
        }
    }

    // Fast-path return type encoding for the inner Option closure.
    let fast_inner_ty = ret_type.rust_str();

    out.push_str(&format!("#[no_mangle]\npub extern \"C\" fn {}() -> i64 {{\n", fn_name));
    out.push_str("    _soma_install_quiet_hook();\n");
    out.push_str("    // Fast path: try Direct (i64/f64). Bail out via None if\n");
    out.push_str("    // any arg doesn't fit i64; catch overflow panics from\n");
    out.push_str("    // checked arithmetic and fall back to the Rug version.\n");
    out.push_str(&format!(
        "    let _fast: Result<Option<{}>, _> = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<{}> {{\n",
        fast_inner_ty, fast_inner_ty
    ));
    for line in &fast_decode_lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&format!(
        "        Some(inner_{}_fast({}))\n",
        fn_name, fast_call_args.join(", ")
    ));
    out.push_str("    }));\n");

    // If fast path returned a value, encode it directly. Otherwise fall back.
    out.push_str("    if let Ok(Some(_fast_v)) = _fast {\n");
    match ret_type {
        NativeType::Int => {
            out.push_str("        return _fast_v;\n");
        }
        NativeType::Float => {
            out.push_str("        return f64::to_bits(_fast_v) as i64;\n");
        }
        NativeType::Bool => {
            out.push_str("        return if _fast_v { 1 } else { 0 };\n");
        }
        NativeType::String => {
            // Shouldn't happen — eligibility excludes String. Handle anyway.
            out.push_str("        unsafe { _SOMA_RESULT = Some(_fast_v); }\n");
            out.push_str("        return i64::MIN + 1;\n");
        }
    }
    out.push_str("    }\n");

    // Rug fallback
    let rug_call = rug_call_args.join(", ");
    match ret_type {
        NativeType::Int => {
            out.push_str(&format!(
                "    let _ret_val: Integer = inner_{}({});\n",
                fn_name, rug_call
            ));
            out.push_str("    if let Some(v) = _ret_val.to_i64() {\n");
            out.push_str("        v\n");
            out.push_str("    } else {\n");
            out.push_str("        unsafe { _SOMA_RESULT = Some(_ret_val.to_string()); }\n");
            out.push_str("        i64::MIN\n");
            out.push_str("    }\n");
        }
        NativeType::Float => {
            out.push_str(&format!(
                "    let _ret_val: f64 = inner_{}({});\n",
                fn_name, rug_call
            ));
            out.push_str("    f64::to_bits(_ret_val) as i64\n");
        }
        NativeType::Bool => {
            out.push_str(&format!(
                "    let _ret_val: bool = inner_{}({});\n",
                fn_name, rug_call
            ));
            out.push_str("    if _ret_val { 1 } else { 0 }\n");
        }
        NativeType::String => {
            out.push_str(&format!(
                "    let _ret_val: String = inner_{}({});\n",
                fn_name, rug_call
            ));
            out.push_str("    unsafe { _SOMA_RESULT = Some(_ret_val); }\n");
            out.push_str("    i64::MIN + 1\n");
        }
    }
    out.push_str("}\n\n");
}

/// Emit the shared-buffer FFI wrapper that calls the Rug inner function
/// and packs its result for the FFI return.
fn gen_rug_ffi_wrapper(
    out: &mut String,
    handler: &NativeHandler,
    fn_name: &str,
    ret_type: NativeType,
) {
    // FFI entry — reads from shared buffers, calls inner, packs result.
    // Int args come from _SOMA_ARGS as Integer.
    // String args come from _SOMA_STRING_ARGS.
    // Float args are bit-encoded as i64 in _SOMA_ARGS (matches push side).
    // Bool args are 0/1 in _SOMA_ARGS.
    out.push_str(&format!("#[no_mangle]\npub extern \"C\" fn {}() -> i64 {{\n", fn_name));
    let mut arg_exprs: Vec<String> = Vec::new();
    let mut int_idx = 0;
    let mut str_idx = 0;
    for p in &handler.params {
        let ty = type_expr_to_native(&p.ty.node);
        match ty {
            NativeType::Int => {
                arg_exprs.push(format!("unsafe {{ _SOMA_ARGS[{}].clone() }}", int_idx));
                int_idx += 1;
            }
            NativeType::String => {
                arg_exprs.push(format!("unsafe {{ _SOMA_STRING_ARGS[{}].clone() }}", str_idx));
                str_idx += 1;
            }
            NativeType::Float => {
                // The interpreter pushes f64::to_bits as i64 via _soma_push_f64
                arg_exprs.push(format!(
                    "unsafe {{ f64::from_bits(_SOMA_ARGS[{}].to_i64().unwrap_or(0) as u64) }}",
                    int_idx
                ));
                int_idx += 1;
            }
            NativeType::Bool => {
                arg_exprs.push(format!(
                    "unsafe {{ _SOMA_ARGS[{}].to_i64().unwrap_or(0) != 0 }}",
                    int_idx
                ));
                int_idx += 1;
            }
        }
    }
    let call_args = arg_exprs.join(", ");
    match ret_type {
        NativeType::Int => {
            out.push_str(&format!("    let _ret_val: Integer = inner_{}({});\n", fn_name, call_args));
            out.push_str("    if let Some(v) = _ret_val.to_i64() {\n");
            out.push_str("        v\n");
            out.push_str("    } else {\n");
            out.push_str("        unsafe { _SOMA_RESULT = Some(_ret_val.to_string()); }\n");
            out.push_str("        i64::MIN\n");
            out.push_str("    }\n");
        }
        NativeType::String => {
            out.push_str(&format!("    let _ret_val: String = inner_{}({});\n", fn_name, call_args));
            out.push_str("    unsafe { _SOMA_RESULT = Some(_ret_val); }\n");
            out.push_str("    i64::MIN + 1\n");
        }
        NativeType::Float => {
            out.push_str(&format!("    let _ret_val: f64 = inner_{}({});\n", fn_name, call_args));
            out.push_str("    f64::to_bits(_ret_val) as i64\n");
        }
        NativeType::Bool => {
            out.push_str(&format!("    let _ret_val: bool = inner_{}({});\n", fn_name, call_args));
            out.push_str("    if _ret_val { 1 } else { 0 }\n");
        }
    }
    out.push_str("}\n\n");
}

// ── Wrappers for parallel + many-arg handlers ───────────────────────

fn emit_parallel_wrapper(
    out: &mut String,
    handler: &NativeHandler,
    fn_name: &str,
    ret_type: NativeType,
    parallel: &ParallelConfig,
) {
    let n_threads = parallel.threads;
    let threads_expr = if n_threads > 0 {
        format!("{}usize", n_threads)
    } else {
        "std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)".to_string()
    };

    let first_param = &handler.params[0].name;
    let other_args: Vec<String> = handler.params[1..].iter().map(|p| p.name.clone()).collect();
    let full_param_str: String = handler.params.iter()
        .map(|p| format!("{}: {}", p.name, type_expr_to_native(&p.ty.node).rust_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let other_args_str = if other_args.is_empty() {
        String::new()
    } else {
        format!(", {}", other_args.join(", "))
    };

    out.push_str(&format!(
        "#[no_mangle]\npub extern \"C\" fn {fn_name}_par({full_param_str}) -> {} {{\n\
        \tlet _n_threads = {threads_expr};\n\
        \tlet _total = {first_param} as usize;\n\
        \tif _total < _n_threads * 1000 {{\n\
        \t\treturn {fn_name}({first_param}{other_args_str});\n\
        \t}}\n\
        \tlet _chunk = (_total + _n_threads - 1) / _n_threads;\n\
        \tlet _result: f64 = std::thread::scope(|_s| {{\n\
        \t\tlet _handles: Vec<_> = (0.._n_threads).map(|_t| {{\n\
        \t\t\tlet _start = _t * _chunk;\n\
        \t\t\tlet _end = std::cmp::min((_t + 1) * _chunk, _total);\n\
        \t\t\tlet _chunk_n = (_end - _start) as i64;\n\
        \t\t\t_s.spawn(move || {{\n\
        \t\t\t\t{fn_name}(_chunk_n{other_args_str}) * _chunk_n as f64\n\
        \t\t\t}})\n\
        \t\t}}).collect();\n\
        \t\t_handles.into_iter().map(|h| h.join().unwrap()).sum::<f64>()\n\
        \t}});\n\
        \t(_result / {first_param} as f64) as {}\n\
        }}\n\n",
        ret_type.rust_str(), ret_type.rust_str()
    ));
}

fn emit_array_wrapper(
    out: &mut String,
    handler: &NativeHandler,
    fn_name: &str,
    ret_type: NativeType,
    is_parallel: bool,
) {
    let suffix = if is_parallel { "_par_arr" } else { "_arr" };
    let inner = if is_parallel { format!("{}_par", fn_name) } else { fn_name.to_string() };
    // Args are passed as raw u64 bits to preserve full i64 / f64 precision
    // (a previous f64-array design lost precision for i64 values > 2^53).
    out.push_str(&format!("#[no_mangle]\npub extern \"C\" fn {}{}(args: *const u64, _count: i64) -> u64 {{\n", fn_name, suffix));
    out.push_str("    unsafe {\n");
    let call_args: Vec<String> = handler.params.iter().enumerate().map(|(i, p)| {
        let ty = type_expr_to_native(&p.ty.node);
        match ty {
            NativeType::Int => format!("*args.add({}) as i64", i),
            NativeType::Float => format!("f64::from_bits(*args.add({}))", i),
            NativeType::Bool => format!("*args.add({}) != 0", i),
            NativeType::String => format!("*args.add({}) as i64", i),
        }
    }).collect();
    out.push_str(&format!("        let r = {}({});\n", inner, call_args.join(", ")));
    match ret_type {
        NativeType::Float => out.push_str("        r.to_bits()\n"),
        NativeType::Int => out.push_str("        r as u64\n"),
        NativeType::Bool => out.push_str("        if r { 1 } else { 0 }\n"),
        NativeType::String => out.push_str("        0\n"),
    }
    out.push_str("    }\n}\n\n");
}

// ── Type inference helpers ──────────────────────────────────────────

/// True if any statement in `body` (recursively) references the variable `name`.
/// Used by gen_assign_rug to decide whether the swap optimization is safe:
/// `name = src` can swap iff src is not read after this point.
///
/// Conservative: returns true on the first sighting of `name` anywhere
/// (doesn't model overwrite-before-read inside If/While bodies).
fn body_references(body: &[Spanned<Statement>], name: &str) -> bool {
    body.iter().any(|s| stmt_references(&s.node, name))
}

fn stmt_references(stmt: &Statement, name: &str) -> bool {
    match stmt {
        Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => {
            FnGenerator::expr_references(&value.node, name)
        }
        Statement::If { condition, then_body, else_body } => {
            FnGenerator::expr_references(&condition.node, name)
                || body_references(then_body, name)
                || body_references(else_body, name)
        }
        Statement::While { condition, body } => {
            FnGenerator::expr_references(&condition.node, name)
                || body_references(body, name)
        }
        Statement::For { iter, body, .. } => {
            FnGenerator::expr_references(&iter.node, name) || body_references(body, name)
        }
        Statement::ExprStmt { expr } => FnGenerator::expr_references(&expr.node, name),
        _ => false,
    }
}

/// Const-fold an Int literal-literal arithmetic expression. Returns the
/// Rust source for the result, or None if either operand isn't a literal.
/// On i64 overflow, emits a runtime panic (the dispatch wrapper catches
/// it and falls back to Rug). Used in Direct-mode codegen to avoid Rust's
/// const-evaluator refusing to compile e.g. `100000000000i64 * 1000000000i64`.
fn try_fold_int_literal_arith(left: &Expr, op: BinOp, right: &Expr) -> Option<String> {
    let (a, b) = match (left, right) {
        (Expr::Literal(Literal::Int(a)), Expr::Literal(Literal::Int(b))) => (*a, *b),
        _ => return None,
    };
    let folded: Option<i64> = match op {
        BinOp::Add => a.checked_add(b),
        BinOp::Sub => a.checked_sub(b),
        BinOp::Mul => a.checked_mul(b),
        BinOp::Div => if b == 0 { None } else { a.checked_div(b) },
        BinOp::Mod => if b == 0 { None } else { a.checked_rem(b) },
        _ => return None,
    };
    Some(match folded {
        Some(v) => format!("({}i64)", v),
        None => "{ panic!(\"i64 overflow in literal arithmetic\") }".to_string(),
    })
}

/// Rust source for a Soma comparison operator.
fn cmp_op_str(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Lt => "<", CmpOp::Gt => ">", CmpOp::Le => "<=",
        CmpOp::Ge => ">=", CmpOp::Eq => "==", CmpOp::Ne => "!=",
    }
}

/// Rust source for a Soma binary arithmetic operator.
fn arith_op_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*",
        BinOp::Div => "/", BinOp::Mod => "%", _ => "+",
    }
}

/// Builtins whose Int result is bounded by their inputs (i64-safe).
/// Used by the small-int classifier to allow these as bounded sub-expressions.
fn is_bounded_builtin(name: &str) -> bool {
    matches!(name,
        "band" | "bor" | "bxor" | "bnot" | "shl" | "shr" | "bit_len"
        | "gcd" | "sqrt_int" | "pow_mod"
        | "abs" | "min" | "max"
        | "floor" | "ceil" | "round"
        | "to_int" | "len"
    )
}

fn type_expr_to_native(ty: &TypeExpr) -> NativeType {
    match ty {
        TypeExpr::Simple(name) => match name.as_str() {
            "Int" => NativeType::Int,
            "Float" => NativeType::Float,
            "Bool" => NativeType::Bool,
            "String" => NativeType::String,
            _ => NativeType::Float,
        },
        _ => NativeType::Float,
    }
}

// ── FnGenerator ─────────────────────────────────────────────────────

/// Information about a sibling handler — needed for cross-handler calls.
#[derive(Clone, Debug, PartialEq)]
struct SiblingInfo {
    param_types: Vec<NativeType>,
    return_type: NativeType,
    mode: Mode,
}

/// Context for the Rug-mode statement walker. Captures everything that
/// used to differ between the four `gen_stmt_rug*` variants.
#[derive(Clone, Debug)]
struct RugCtx {
    /// Function's declared return type — controls how `return value` is lowered.
    fn_ret_type: NativeType,
    /// Integer locals already declared in an enclosing scope. A `let` for one
    /// of these turns into an `assign` instead of a fresh allocation.
    hoisted: HashSet<String>,
}

impl RugCtx {
    fn new(fn_ret_type: NativeType) -> Self {
        Self { fn_ret_type, hoisted: HashSet::new() }
    }
}

struct FnGenerator {
    var_types: HashMap<String, NativeType>,
    siblings: HashSet<String>,
    sibling_info: HashMap<String, SiblingInfo>,
    mode: Mode,
    /// In Rug mode, Int variables that can be safely represented as i64
    /// (assigned only from literals or `self ± literal` patterns).
    small_int_vars: HashSet<String>,
    /// Errors encountered during codegen — checked at the end of generation.
    errors: RefCell<Vec<String>>,
    /// Name of the handler being compiled (for error context).
    handler_name: String,
    /// Declared/inferred return type of the handler being compiled.
    fn_return_type: NativeType,
    /// Names of siblings that have a fast (Direct) path. When in Direct
    /// codegen, sibling calls to one of these use the `_fast` suffix.
    /// In Rug codegen, this is unused (Rug calls always go to the Rug
    /// inner function regardless of whether a fast path exists).
    dualmode_siblings: HashSet<String>,
    /// True if THIS handler is itself the fast path of a dual-mode handler.
    /// (Doesn't change codegen of the body — only matters for the inner
    /// function name when emitting.)
    is_fast_variant: bool,
}

impl FnGenerator {
    fn new(params: &[Param], siblings: &HashSet<String>, mode: Mode) -> Self {
        let mut var_types = HashMap::new();
        for p in params {
            var_types.insert(p.name.clone(), type_expr_to_native(&p.ty.node));
        }
        Self {
            var_types,
            siblings: siblings.clone(),
            sibling_info: HashMap::new(),
            mode,
            small_int_vars: HashSet::new(),
            errors: RefCell::new(Vec::new()),
            handler_name: String::new(),
            fn_return_type: NativeType::Float,
            dualmode_siblings: HashSet::new(),
            is_fast_variant: false,
        }
    }

    fn with_return_type(mut self, ty: NativeType) -> Self {
        self.fn_return_type = ty;
        self
    }

    fn with_sibling_info(mut self, info: HashMap<String, SiblingInfo>) -> Self {
        self.sibling_info = info;
        self
    }

    fn with_handler_name(mut self, name: String) -> Self {
        self.handler_name = name;
        self
    }

    fn with_dualmode_siblings(mut self, set: HashSet<String>) -> Self {
        self.dualmode_siblings = set;
        self
    }

    fn with_fast_variant(mut self, fast: bool) -> Self {
        self.is_fast_variant = fast;
        self
    }

    fn err(&self, msg: impl Into<String>) {
        let m = format!("[{}] {}", self.handler_name, msg.into());
        self.errors.borrow_mut().push(m);
    }

    /// Classify Int variables: small (i64-safe) vs big (need rug::Integer).
    /// Only relevant in Rug mode.
    ///
    /// Fixpoint analysis: start with all Int locals in `small`. Repeatedly
    /// exclude any variable whose assignments use non-small variables or
    /// `var * var` (which can grow unboundedly). Iterate to fixpoint.
    ///
    /// Allowed expressions for a "small" variable:
    ///   - integer literal
    ///   - reference to another small variable (or self)
    ///   - small ± small, small / small, small % small
    ///   - small * literal, literal * small
    ///   - NOT small * small (potential unbounded growth)
    fn classify_int_vars(&mut self, _body: &[Spanned<Statement>], _int_params: &HashSet<String>) {
        // Universal dual-mode: every handler has a fast Direct path that
        // uses i64 throughout, AND a Rug fallback that uses Integer
        // throughout. The Rug fallback exists precisely to handle the
        // cases where i64 doesn't suffice — using i64 inside it (via the
        // small_int_var optimization) defeats its purpose: the same
        // overflow that would have crashed Direct now crashes Rug too,
        // escaping the dispatch wrapper's catch_unwind.
        //
        // So small_int_vars is always empty. The Rug fallback is slower
        // (Integer arithmetic for everything) but correct, and it only
        // runs when the fast path failed — by definition the case where
        // we needed BigInt anyway.
        self.small_int_vars = HashSet::new();
    }

    /// One classifier pass: walk the body and remove variables from `small`
    /// whose RHS expression is no longer i64-safe given the current state.
    /// `var_types`: Some when called from per-handler codegen (lets the
    /// bounded check treat Float idents as automatically bounded);
    /// None when called from the mode-select preview (we don't have full
    /// type info there yet).
    fn run_classifier(
        body: &[Spanned<Statement>],
        small: &mut HashSet<String>,
        siblings: &HashSet<String>,
        var_types: Option<&HashMap<String, NativeType>>,
    ) {
        let mut to_remove: Vec<String> = Vec::new();
        Self::collect_non_small(body, small, &mut to_remove, siblings, var_types);
        for name in to_remove {
            small.remove(&name);
        }
    }

    fn collect_non_small(
        body: &[Spanned<Statement>],
        small: &HashSet<String>,
        to_remove: &mut Vec<String>,
        siblings: &HashSet<String>,
        var_types: Option<&HashMap<String, NativeType>>,
    ) {
        Self::collect_non_small_inner(body, small, to_remove, siblings, var_types, false);
    }

    fn collect_non_small_inner(
        body: &[Spanned<Statement>],
        small: &HashSet<String>,
        to_remove: &mut Vec<String>,
        siblings: &HashSet<String>,
        var_types: Option<&HashMap<String, NativeType>>,
        in_loop: bool,
    ) {
        for stmt in body {
            match &stmt.node {
                Statement::Let { name, value } | Statement::Assign { name, value } => {
                    if !small.contains(name) { continue; }
                    if !Self::is_small_expr(name, &value.node, small, siblings, var_types) {
                        to_remove.push(name.clone());
                        continue;
                    }
                    // Loop-context check: Fibonacci-style `t = a + b` where both
                    // a and b are distinct loop-mutated Int vars accumulates
                    // unbounded growth across iterations even though each step
                    // looks individually bounded. The classifier doesn't model
                    // iterations, so reject this pattern conservatively when
                    // inside any loop.
                    if in_loop && Self::has_two_var_arith(&value.node, name, var_types) {
                        to_remove.push(name.clone());
                    }
                }
                Statement::If { then_body, else_body, .. } => {
                    Self::collect_non_small_inner(then_body, small, to_remove, siblings, var_types, in_loop);
                    Self::collect_non_small_inner(else_body, small, to_remove, siblings, var_types, in_loop);
                }
                Statement::While { body, .. } => {
                    Self::collect_non_small_inner(body, small, to_remove, siblings, var_types, true);
                }
                Statement::For { body, .. } => {
                    Self::collect_non_small_inner(body, small, to_remove, siblings, var_types, true);
                }
                _ => {}
            }
        }
    }

    /// True if `expr` contains a +/- of two distinct Int identifiers, neither
    /// of which is `target` (i.e. it's not a self-additive recurrence).
    /// This is the Fibonacci-style accumulation pattern that overflows i64
    /// without bound across loop iterations.
    fn has_two_var_arith(
        expr: &Expr,
        target: &str,
        var_types: Option<&HashMap<String, NativeType>>,
    ) -> bool {
        let is_int_ident = |e: &Expr| -> Option<String> {
            if let Expr::Ident(n) = e {
                if n == target { return None; }
                if let Some(types) = var_types {
                    if types.get(n).copied() == Some(NativeType::Int) {
                        return Some(n.clone());
                    }
                } else {
                    // No type info — assume Int (conservative).
                    return Some(n.clone());
                }
            }
            None
        };
        match expr {
            Expr::BinaryOp { left, op, right } if matches!(op, BinOp::Add | BinOp::Sub) => {
                if let (Some(a), Some(b)) = (is_int_ident(&left.node), is_int_ident(&right.node)) {
                    if a != b { return true; }
                }
                Self::has_two_var_arith(&left.node, target, var_types)
                    || Self::has_two_var_arith(&right.node, target, var_types)
            }
            Expr::BinaryOp { left, right, .. } => {
                Self::has_two_var_arith(&left.node, target, var_types)
                    || Self::has_two_var_arith(&right.node, target, var_types)
            }
            _ => false,
        }
    }

    /// True iff `expr` is a "bounded" RHS for assigning to `target`.
    /// Splits on whether expr self-references target.
    fn is_small_expr(
        target: &str,
        expr: &Expr,
        small: &HashSet<String>,
        siblings: &HashSet<String>,
        var_types: Option<&HashMap<String, NativeType>>,
    ) -> bool {
        if Self::expr_references(expr, target) {
            Self::is_additive_expr(target, expr, small, siblings, var_types)
        } else {
            Self::is_bounded_expr(expr, small, siblings, var_types)
        }
    }

    /// Self-referential expression: bounded growth patterns when iterated.
    /// Allowed:
    ///   - literal, target itself, small var
    ///   - target ± bounded  (linear growth per iteration)
    ///   - target / bounded  (shrinks toward zero)
    ///   - target % bounded  (bounded by modulus)
    ///   - bounded sub-expressions that don't reference target
    /// Disallowed:
    ///   - target * anything (exponential)
    ///   - target + target    (doubles → exponential)
    ///   - bounded / target   (target in denominator → could blow up)
    fn is_additive_expr(
        target: &str,
        expr: &Expr,
        small: &HashSet<String>,
        siblings: &HashSet<String>,
        var_types: Option<&HashMap<String, NativeType>>,
    ) -> bool {
        // If expr doesn't reference target, the bounded check is enough.
        if !Self::expr_references(expr, target) {
            return Self::is_bounded_expr(expr, small, siblings, var_types);
        }
        match expr {
            Expr::Ident(n) if n == target => true,
            Expr::BinaryOp { left, op, right } => {
                let l_has = Self::expr_references(&left.node, target);
                let r_has = Self::expr_references(&right.node, target);
                match op {
                    BinOp::Add => {
                        if l_has && r_has { return false; }  // target + target = doubling
                        Self::is_additive_expr(target, &left.node, small, siblings, var_types)
                            && Self::is_additive_expr(target, &right.node, small, siblings, var_types)
                    }
                    BinOp::Sub => {
                        Self::is_additive_expr(target, &left.node, small, siblings, var_types)
                            && Self::is_additive_expr(target, &right.node, small, siblings, var_types)
                    }
                    BinOp::Div | BinOp::Mod => {
                        if r_has { return false; }  // target in denominator
                        Self::is_additive_expr(target, &left.node, small, siblings, var_types)
                            && Self::is_bounded_expr(&right.node, small, siblings, var_types)
                    }
                    _ => false,  // Mul is exponential when self-referential
                }
            }
            _ => false,
        }
    }

    /// Non-self bounded expression: i64-safe assuming all referenced vars are bounded.
    ///
    /// Rules:
    ///   - literals (Int, Float, Bool) are bounded
    ///   - Idents: Int idents must be in `small`; Float/Bool idents are
    ///     automatically bounded (when `var_types` is supplied)
    ///   - +, -, /, % on bounded operands → bounded
    ///   - * on bounded operands → bounded ONLY IF at least one is a literal
    ///     OR at least one operand is a Float (Int*Int could overflow)
    ///   - FnCall to a known-bounded builtin or a sibling handler → bounded
    ///     if all args are bounded
    fn is_bounded_expr(
        expr: &Expr,
        small: &HashSet<String>,
        siblings: &HashSet<String>,
        var_types: Option<&HashMap<String, NativeType>>,
    ) -> bool {
        match expr {
            Expr::Literal(Literal::Int(_))
            | Expr::Literal(Literal::Float(_))
            | Expr::Literal(Literal::Bool(_)) => true,
            Expr::Ident(n) => {
                // When we have type info, non-Int idents are automatically bounded.
                if let Some(types) = var_types {
                    if types.get(n).copied().unwrap_or(NativeType::Float) != NativeType::Int {
                        return true;
                    }
                }
                small.contains(n)
            }
            Expr::BinaryOp { left, op, right } => {
                if !Self::is_bounded_expr(&left.node, small, siblings, var_types) { return false; }
                if !Self::is_bounded_expr(&right.node, small, siblings, var_types) { return false; }
                if matches!(op, BinOp::Mul) {
                    // var*var is an Int-overflow concern only.
                    let is_float_operand = |e: &Expr| -> bool {
                        if matches!(e, Expr::Literal(Literal::Float(_))) { return true; }
                        if let (Some(types), Expr::Ident(n)) = (var_types, e) {
                            return types.get(n).copied() == Some(NativeType::Float);
                        }
                        false
                    };
                    let is_int_lit = |e: &Expr| matches!(e, Expr::Literal(Literal::Int(_)));
                    let any_float = is_float_operand(&left.node) || is_float_operand(&right.node);
                    let any_int_lit = is_int_lit(&left.node) || is_int_lit(&right.node);
                    if !any_float && !any_int_lit { return false; }
                }
                matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod)
            }
            Expr::FnCall { name, args } => {
                if !is_bounded_builtin(name) && !siblings.contains(name) { return false; }
                // shl is i64-safe only when the shift amount is a small literal.
                // shl(1, n) for n>=63 overflows i64; treat any shl with a
                // non-literal (or large literal) shift count as unbounded.
                if name == "shl" && args.len() >= 2 {
                    match &args[1].node {
                        Expr::Literal(Literal::Int(k)) if *k < 60 && *k >= 0 => {}
                        _ => return false,
                    }
                }
                args.iter().all(|a| Self::is_bounded_expr(&a.node, small, siblings, var_types))
            }
            _ => false,
        }
    }

    fn infer_body_types(&mut self, body: &[Spanned<Statement>]) {
        for stmt in body {
            self.infer_stmt_types(&stmt.node);
        }
    }

    fn infer_stmt_types(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Let { name, value } => {
                let ty = self.infer_expr_type(&value.node);
                self.var_types.insert(name.clone(), ty);
            }
            Statement::Assign { name, value } => {
                let ty = self.infer_expr_type(&value.node);
                if let Some(existing) = self.var_types.get(name) {
                    if *existing == NativeType::Int && ty == NativeType::Float {
                        self.var_types.insert(name.clone(), NativeType::Float);
                    }
                } else {
                    self.var_types.insert(name.clone(), ty);
                }
            }
            Statement::If { then_body, else_body, .. } => {
                for s in then_body { self.infer_stmt_types(&s.node); }
                for s in else_body { self.infer_stmt_types(&s.node); }
            }
            Statement::While { body, .. } => {
                for s in body { self.infer_stmt_types(&s.node); }
            }
            Statement::For { var, iter, body, .. } => {
                self.var_types.insert(var.clone(), NativeType::Int);
                let _ = self.infer_expr_type(&iter.node);
                for s in body { self.infer_stmt_types(&s.node); }
            }
            _ => {}
        }
    }

    fn infer_expr_type(&self, expr: &Expr) -> NativeType {
        match expr {
            Expr::Literal(Literal::Int(_)) => NativeType::Int,
            Expr::Literal(Literal::Float(_)) => NativeType::Float,
            Expr::Literal(Literal::Bool(_)) => NativeType::Bool,
            Expr::Literal(Literal::String(_)) => NativeType::String,
            Expr::Ident(name) => self.var_types.get(name).copied().unwrap_or(NativeType::Float),
            Expr::BinaryOp { left, op, right } => {
                let lt = self.infer_expr_type(&left.node);
                let rt = self.infer_expr_type(&right.node);
                match op {
                    BinOp::Div => {
                        if lt == NativeType::Int && rt == NativeType::Int {
                            NativeType::Int
                        } else {
                            NativeType::Float
                        }
                    }
                    BinOp::And | BinOp::Or => NativeType::Bool,
                    BinOp::Add if lt == NativeType::String || rt == NativeType::String => NativeType::String,
                    _ => {
                        if lt == NativeType::Float || rt == NativeType::Float {
                            NativeType::Float
                        } else {
                            lt
                        }
                    }
                }
            }
            Expr::CmpOp { .. } => NativeType::Bool,
            Expr::Not(_) => NativeType::Bool,
            Expr::FnCall { name, args } => {
                match name.as_str() {
                    "random" | "sqrt" | "log" | "exp" | "pow" | "sin" | "cos" => NativeType::Float,
                    "to_string" => NativeType::String,
                    "to_float" => NativeType::Float,
                    "to_int" | "len" | "floor" | "ceil" | "round" => NativeType::Int,
                    "band" | "bor" | "bxor" | "bnot" | "shl" | "shr" | "bit_len" => NativeType::Int,
                    "gcd" | "pow_mod" | "sqrt_int" => NativeType::Int,
                    "str_len" | "str_at" => NativeType::Int,
                    "str_eq" => NativeType::Bool,
                    "abs" | "min" | "max" => {
                        if args.is_empty() { NativeType::Float }
                        else { self.infer_expr_type(&args[0].node) }
                    }
                    other => {
                        // Sibling native handler call — use the precomputed sibling info if available.
                        if let Some(info) = self.sibling_info.get(other) {
                            info.return_type
                        } else {
                            NativeType::Float
                        }
                    }
                }
            }
            _ => NativeType::Float,
        }
    }

    fn infer_return_type(&self, body: &[Spanned<Statement>]) -> NativeType {
        for stmt in body {
            match &stmt.node {
                Statement::Return { value } => {
                    return self.infer_expr_type(&value.node);
                }
                Statement::If { then_body, else_body, .. } => {
                    if let Some(ty) = self.find_return_type(then_body) { return ty; }
                    if let Some(ty) = self.find_return_type(else_body) { return ty; }
                }
                Statement::While { body, .. } => {
                    if let Some(ty) = self.find_return_type(body) { return ty; }
                }
                _ => {}
            }
        }
        if let Some(last) = body.last() {
            if let Statement::ExprStmt { expr } = &last.node {
                return self.infer_expr_type(&expr.node);
            }
        }
        NativeType::Float
    }

    fn find_return_type(&self, body: &[Spanned<Statement>]) -> Option<NativeType> {
        for stmt in body {
            match &stmt.node {
                Statement::Return { value } => return Some(self.infer_expr_type(&value.node)),
                Statement::If { then_body, else_body, .. } => {
                    if let Some(t) = self.find_return_type(then_body) { return Some(t); }
                    if let Some(t) = self.find_return_type(else_body) { return Some(t); }
                }
                Statement::While { body, .. } => {
                    if let Some(t) = self.find_return_type(body) { return Some(t); }
                }
                _ => {}
            }
        }
        None
    }

    fn indent(level: usize) -> String {
        "    ".repeat(level)
    }

    // ═══════════════════════════════════════════════════════════════
    // Direct mode codegen (Int=i64, Float=f64, Bool=bool)
    // ═══════════════════════════════════════════════════════════════

    fn gen_stmt_direct(&self, stmt: &Statement, indent: usize) -> String {
        let ind = Self::indent(indent);
        match stmt {
            Statement::Let { name, value } => {
                let ty = self.var_types.get(name).copied().unwrap_or(NativeType::Float);
                let expr = self.gen_expr_direct(&value.node, ty);
                format!("{}let mut {}: {} = {};\n", ind, name, ty.rust_str(), expr)
            }
            Statement::Assign { name, value } => {
                let ty = self.var_types.get(name).copied().unwrap_or(NativeType::Float);
                let expr = self.gen_expr_direct(&value.node, ty);
                format!("{}{} = {};\n", ind, name, expr)
            }
            Statement::Return { value } => {
                // Use the function's return type, not the expression's inferred type,
                // so that mixed-type returns (e.g. `return 0` from a Float function) get coerced.
                let ret_ty = self.fn_return_type;
                let expr = self.gen_expr_direct(&value.node, ret_ty);
                format!("{}return {};\n", ind, expr)
            }
            Statement::If { condition, then_body, else_body } => {
                let cond = self.gen_expr_direct(&condition.node, NativeType::Bool);
                let mut s = format!("{}if {} {{\n", ind, cond);
                for st in then_body {
                    s.push_str(&self.gen_stmt_direct(&st.node, indent + 1));
                }
                if else_body.is_empty() {
                    s.push_str(&format!("{}}}\n", ind));
                } else {
                    s.push_str(&format!("{}}} else {{\n", ind));
                    for st in else_body {
                        s.push_str(&self.gen_stmt_direct(&st.node, indent + 1));
                    }
                    s.push_str(&format!("{}}}\n", ind));
                }
                s
            }
            Statement::While { condition, body } => {
                let cond = self.gen_expr_direct(&condition.node, NativeType::Bool);
                let mut s = format!("{}while {} {{\n", ind, cond);
                for st in body {
                    s.push_str(&self.gen_stmt_direct(&st.node, indent + 1));
                }
                s.push_str(&format!("{}}}\n", ind));
                s
            }
            Statement::For { var, iter, body } => {
                let iter_code = self.gen_for_iter_direct(&iter.node);
                let mut s = format!("{}for {} in {} {{\n", ind, var, iter_code);
                for st in body {
                    s.push_str(&self.gen_stmt_direct(&st.node, indent + 1));
                }
                s.push_str(&format!("{}}}\n", ind));
                s
            }
            Statement::Break => format!("{}break;\n", ind),
            Statement::Continue => format!("{}continue;\n", ind),
            Statement::ExprStmt { expr } => {
                // Statement-level expression (e.g. a fn call for side effects).
                // Drop the result via `let _ =` to silence unused-result warnings.
                let code = self.gen_expr_direct(&expr.node, NativeType::Float);
                format!("{}let _ = {};\n", ind, code)
            }
            other => {
                self.err(format!("unsupported statement in Direct mode: {:?}", other));
                format!("{}// codegen error\n", ind)
            }
        }
    }

    fn gen_for_iter_direct(&self, expr: &Expr) -> String {
        if let Expr::FnCall { name, args } = expr {
            if name == "range" && args.len() == 2 {
                // Range bounds must be i64. In Rug mode, an Int parameter or
                // local could be `Integer` — convert via to_i64().
                let a = self.gen_for_bound(&args[0].node);
                let b = self.gen_for_bound(&args[1].node);
                return format!("{}..{}", a, b);
            }
        }
        self.err("unsupported for-loop iterator (only range(a, b) is supported)");
        "0..0".to_string()
    }

    /// Lower a range bound expression to i64. Works in both Direct and Rug
    /// modes — in Direct mode every Int local is already i64; in Rug mode,
    /// non-small Int locals/params are `Integer` and need .to_i64().unwrap().
    fn gen_for_bound(&self, expr: &Expr) -> String {
        if self.mode == Mode::Rug {
            return self.gen_int_to_i64_rug(expr);
        }
        self.gen_expr_direct(expr, NativeType::Int)
    }

    fn gen_expr_direct(&self, expr: &Expr, target_ty: NativeType) -> String {
        match expr {
            Expr::Literal(Literal::Int(n)) => {
                match target_ty {
                    NativeType::Float => format!("{}.0f64", n),
                    _ => format!("{}i64", n),
                }
            }
            Expr::Literal(Literal::Float(f)) => format!("{}f64", f),
            Expr::Literal(Literal::Bool(b)) => format!("{}", b),
            Expr::Literal(Literal::String(s)) => {
                format!("\"{}\".to_string()", s.replace('\\', "\\\\").replace('"', "\\\""))
            }
            Expr::Ident(name) => {
                let var_ty = self.var_types.get(name).copied().unwrap_or(NativeType::Float);
                self.coerce_direct(name.clone(), var_ty, target_ty)
            }
            Expr::BinaryOp { left, op, right } => {
                self.gen_binop_direct(&left.node, *op, &right.node, target_ty)
            }
            Expr::CmpOp { left, op, right } => {
                let lt = self.infer_expr_type(&left.node);
                let rt = self.infer_expr_type(&right.node);
                let common = if lt == NativeType::Float || rt == NativeType::Float {
                    NativeType::Float
                } else if lt == NativeType::String || rt == NativeType::String {
                    NativeType::String
                } else if lt == NativeType::Bool && rt == NativeType::Bool {
                    NativeType::Bool
                } else {
                    NativeType::Int
                };
                let l = self.gen_expr_direct(&left.node, common);
                let r = self.gen_expr_direct(&right.node, common);
                format!("({} {} {})", l, cmp_op_str(*op), r)
            }
            Expr::Not(inner) => {
                let e = self.gen_expr_direct(&inner.node, NativeType::Bool);
                format!("(!{})", e)
            }
            Expr::FnCall { name, args } => self.gen_fn_call_direct(name, args, target_ty),
            other => {
                self.err(format!("unsupported expression in Direct mode: {:?}", other));
                "0i64".to_string()
            }
        }
    }

    /// Coerce a value of `from` type to `to` type in Direct mode.
    fn coerce_direct(&self, expr: String, from: NativeType, to: NativeType) -> String {
        if from == to { return expr; }
        match (from, to) {
            (NativeType::Int, NativeType::Float) => format!("({} as f64)", expr),
            (NativeType::Float, NativeType::Int) => format!("({} as i64)", expr),
            (NativeType::Bool, NativeType::Int) => format!("({} as i64)", expr),
            (NativeType::Int, NativeType::Bool) => format!("({} != 0)", expr),
            (_, NativeType::String) => format!("format!(\"{{}}\", {})", expr),
            _ => expr,
        }
    }

    fn gen_binop_direct(&self, left: &Expr, op: BinOp, right: &Expr, target_ty: NativeType) -> String {
        let lt = self.infer_expr_type(left);
        let rt = self.infer_expr_type(right);

        // Logical
        if matches!(op, BinOp::And | BinOp::Or) {
            let l = self.gen_expr_direct(left, NativeType::Bool);
            let r = self.gen_expr_direct(right, NativeType::Bool);
            let op_str = if matches!(op, BinOp::And) { "&&" } else { "||" };
            return format!("({} {} {})", l, op_str, r);
        }

        // String concat
        if matches!(op, BinOp::Add) && (lt == NativeType::String || rt == NativeType::String) {
            let l = self.gen_expr_direct(left, NativeType::String);
            let r = self.gen_expr_direct(right, NativeType::String);
            return format!("format!(\"{{}}{{}}\", {}, {})", l, r);
        }

        // Numeric: pick common type (Float dominates)
        let common = if lt == NativeType::Float || rt == NativeType::Float {
            NativeType::Float
        } else {
            NativeType::Int
        };

        // Literal-literal arithmetic: const-fold ourselves to avoid Rust's
        // const-evaluator catching overflow at compile time.
        if common == NativeType::Int {
            if let Some(inner) = try_fold_int_literal_arith(left, op, right) {
                return self.coerce_direct(inner, common, target_ty);
            }
        }

        let l = self.gen_expr_direct(left, common);
        let r = self.gen_expr_direct(right, common);
        let inner = format!("({} {} {})", l, arith_op_str(op), r);
        self.coerce_direct(inner, common, target_ty)
    }

    fn gen_fn_call_direct(&self, name: &str, args: &[Spanned<Expr>], target_ty: NativeType) -> String {
        match name {
            "random" => "unsafe { _soma_random() }".to_string(),
            "sqrt" | "log" | "exp" | "sin" | "cos" => {
                let a = self.gen_expr_direct(&args[0].node, NativeType::Float);
                let method = match name { "log" => "ln", other => other };
                format!("({}).{}()", a, method)
            }
            "pow" => {
                let a = self.gen_expr_direct(&args[0].node, NativeType::Float);
                let b = self.gen_expr_direct(&args[1].node, NativeType::Float);
                format!("({}).powf({})", a, b)
            }
            "abs" => {
                let a_ty = self.infer_expr_type(&args[0].node);
                let a = self.gen_expr_direct(&args[0].node, a_ty);
                format!("({}).abs()", a)
            }
            "min" | "max" if args.len() == 2 => {
                let a_ty = self.infer_expr_type(&args[0].node);
                let b_ty = self.infer_expr_type(&args[1].node);
                let common = if a_ty == NativeType::Float || b_ty == NativeType::Float {
                    NativeType::Float
                } else {
                    NativeType::Int
                };
                let a = self.gen_expr_direct(&args[0].node, common);
                let b = self.gen_expr_direct(&args[1].node, common);
                if common == NativeType::Float {
                    format!("({}).{}({})", a, name, b)
                } else {
                    format!("std::cmp::{}({}, {})", name, a, b)
                }
            }
            "floor" | "ceil" | "round" => {
                let a = self.gen_expr_direct(&args[0].node, NativeType::Float);
                format!("(({}).{}() as i64)", a, name)
            }
            "to_int" => {
                let a_ty = self.infer_expr_type(&args[0].node);
                let a = self.gen_expr_direct(&args[0].node, a_ty);
                format!("({} as i64)", a)
            }
            "to_float" => {
                let a_ty = self.infer_expr_type(&args[0].node);
                // In Rug mode an Int Ident may be a `rug::Integer` (no `as f64`
                // cast). Detect the case and emit `.to_f64()` instead.
                if a_ty == NativeType::Int && self.mode == Mode::Rug {
                    if let Expr::Ident(name) = &args[0].node {
                        if !self.small_int_vars.contains(name) {
                            return format!("({}.to_f64())", name);
                        }
                    }
                }
                let a = self.gen_expr_direct(&args[0].node, a_ty);
                format!("({} as f64)", a)
            }
            "to_string" => {
                let a_ty = self.infer_expr_type(&args[0].node);
                let a = self.gen_expr_direct(&args[0].node, a_ty);
                format!("format!(\"{{}}\", {})", a)
            }
            "len" | "str_len" => {
                let arg_ty = self.infer_expr_type(&args[0].node);
                if arg_ty == NativeType::String {
                    let a = self.gen_expr_direct(&args[0].node, NativeType::String);
                    format!("({}.len() as i64)", a)
                } else {
                    {
                        self.err("len() is only supported on String values in [native]");
                        "0i64".to_string()
                    }
                }
            }
            "str_at" if args.len() == 2 => {
                let s = self.gen_expr_direct(&args[0].node, NativeType::String);
                let i = self.gen_expr_direct(&args[1].node, NativeType::Int);
                format!("({}.as_bytes()[({}) as usize] as i64)", s, i)
            }
            "str_eq" if args.len() == 2 => {
                let a = self.gen_expr_direct(&args[0].node, NativeType::String);
                let b = self.gen_expr_direct(&args[1].node, NativeType::String);
                format!("(({}) == ({}))", a, b)
            }
            "range" if args.len() == 2 => {
                let a = self.gen_expr_direct(&args[0].node, NativeType::Int);
                let b = self.gen_expr_direct(&args[1].node, NativeType::Int);
                format!("({}..{})", a, b)
            }
            // Bit operations (Int)
            "band" | "bor" | "bxor" if args.len() == 2 => {
                let a = self.gen_expr_direct(&args[0].node, NativeType::Int);
                let b = self.gen_expr_direct(&args[1].node, NativeType::Int);
                let op = match name { "band" => "&", "bor" => "|", _ => "^" };
                format!("(({}) {} ({}))", a, op, b)
            }
            "bnot" if args.len() == 1 => {
                let a = self.gen_expr_direct(&args[0].node, NativeType::Int);
                format!("(!({}))", a)
            }
            "shl" | "shr" if args.len() == 2 => {
                let a = self.gen_expr_direct(&args[0].node, NativeType::Int);
                let b = self.gen_expr_direct(&args[1].node, NativeType::Int);
                let op = if name == "shl" { "<<" } else { ">>" };
                format!("(({}) {} ({}))", a, op, b)
            }
            "bit_len" if args.len() == 1 => {
                let a = self.gen_expr_direct(&args[0].node, NativeType::Int);
                format!("(64 - ({} as i64).leading_zeros() as i64)", a)
            }
            // Number theory
            "gcd" if args.len() == 2 => {
                let a = self.gen_expr_direct(&args[0].node, NativeType::Int);
                let b = self.gen_expr_direct(&args[1].node, NativeType::Int);
                format!("{{ let (mut _a, mut _b): (i64, i64) = (({}).abs(), ({}).abs()); while _b != 0 {{ let _t = _b; _b = _a % _b; _a = _t; }} _a }}", a, b)
            }
            "sqrt_int" if args.len() == 1 => {
                let a = self.gen_expr_direct(&args[0].node, NativeType::Int);
                format!("(({} as f64).sqrt() as i64)", a)
            }
            "pow_mod" if args.len() == 3 => {
                // Direct mode pow_mod: square-and-multiply on i64
                let base = self.gen_expr_direct(&args[0].node, NativeType::Int);
                let exp = self.gen_expr_direct(&args[1].node, NativeType::Int);
                let m = self.gen_expr_direct(&args[2].node, NativeType::Int);
                format!("{{ let mut _r: i128 = 1; let mut _b: i128 = ({}) as i128 % ({}) as i128; let mut _e: i64 = {}; let _m: i128 = ({}) as i128; while _e > 0 {{ if _e & 1 == 1 {{ _r = (_r * _b) % _m; }} _e >>= 1; _b = (_b * _b) % _m; }} _r as i64 }}",
                    base, m, exp, m)
            }
            other => {
                // Sibling native handler call — go through inner_handler_X
                if let Some(info) = self.sibling_info.get(other).cloned() {
                    return self.gen_sibling_call_from_direct(other, args, &info, target_ty);
                }
                {
                    self.err(format!("unknown function call: {} (not a sibling, not a builtin)", other));
                    "0i64".to_string()
                }
            }
        }
    }

    /// Generate a sibling call from a Direct-mode caller.
    /// Marshals args based on the sibling's parameter types and mode.
    fn gen_sibling_call_from_direct(
        &self,
        name: &str,
        args: &[Spanned<Expr>],
        sibling: &SiblingInfo,
        target_ty: NativeType,
    ) -> String {
        // If the sibling is a dual-mode handler AND this caller is itself
        // in the fast path (Mode::Direct or the fast variant of a dual
        // handler), call its fast Direct variant. This keeps the fast path
        // entirely within i64/f64 land. If overflow happens in the inner
        // sibling, the panic propagates up to the outermost dispatch
        // wrapper which catches it and re-runs in Rug mode.
        //
        // We deliberately do NOT use _fast siblings from Rug-mode callers
        // (even when they're emitting Direct code for a small_int_var
        // assignment), because the Rug fallback path is not wrapped in
        // catch_unwind — a panic from a fast sibling would escape out the
        // FFI boundary.
        let calling_fast_variant =
            self.mode == Mode::Direct && self.dualmode_siblings.contains(name);
        let effective_mode = if calling_fast_variant { Mode::Direct } else { sibling.mode };
        let arg_strs: Vec<String> = args.iter().zip(sibling.param_types.iter())
            .map(|(arg, &expected)| {
                if effective_mode == Mode::Rug && expected == NativeType::Int {
                    // Sibling expects Integer, we have i64 → Integer::from(...)
                    let e = self.gen_expr_direct(&arg.node, NativeType::Int);
                    format!("Integer::from({})", e)
                } else if expected == NativeType::String {
                    // Always clone String args to avoid move-then-borrow conflicts
                    if let Expr::Ident(name) = &arg.node {
                        format!("{}.clone()", name)
                    } else {
                        self.gen_expr_direct(&arg.node, NativeType::String)
                    }
                } else {
                    self.gen_expr_direct(&arg.node, expected)
                }
            })
            .collect();
        let inner_name = if calling_fast_variant {
            format!("inner_handler_{}_fast", name)
        } else {
            format!("inner_handler_{}", name)
        };
        let call = format!("{}({})", inner_name, arg_strs.join(", "));
        // Convert sibling return value to caller's representation
        let from_ty = if effective_mode == Mode::Rug && sibling.return_type == NativeType::Int {
            // Sibling returns Integer; we need an i64 (or coerced)
            return self.coerce_direct(format!("({}).to_i64().expect(\"BigInt overflow in sibling call\")", call), NativeType::Int, target_ty);
        } else {
            sibling.return_type
        };
        self.coerce_direct(call, from_ty, target_ty)
    }

    // ═══════════════════════════════════════════════════════════════
    // Rug mode codegen (Int=rug::Integer, Float=f64)
    // ═══════════════════════════════════════════════════════════════

    /// Statement walker for Rug mode.
    ///
    /// One unified function replaces what used to be four near-identical
    /// variants (`gen_stmt_rug`, `gen_stmt_rug_inner`, `gen_stmt_rug_hoisted`,
    /// `gen_stmt_rug_hoisted_inner`). The behavioural differences are now
    /// captured in `RugCtx`:
    ///   - `fn_ret_type`: how Return statements should produce their value
    ///   - `hoisted`: which Integer locals have been hoisted to function scope
    ///     (so their `let` statements become `assign` instead of fresh allocs)
    /// Walk a body, generating code for each statement. Each statement is
    /// passed the *remaining* statements in the same body so that
    /// liveness-sensitive optimizations (e.g. swap-on-assign) can check
    /// whether a variable is still in use.
    fn gen_body_rug(&self, body: &[Spanned<Statement>], indent: usize, ctx: &RugCtx) -> String {
        let mut out = String::new();
        for (i, stmt) in body.iter().enumerate() {
            let rest = &body[i + 1..];
            out.push_str(&self.gen_stmt_rug(&stmt.node, indent, ctx, rest));
        }
        out
    }

    fn gen_stmt_rug(
        &self,
        stmt: &Statement,
        indent: usize,
        ctx: &RugCtx,
        rest: &[Spanned<Statement>],
    ) -> String {
        let ind = Self::indent(indent);
        match stmt {
            Statement::Let { name, value } => {
                let ty = self.var_types.get(name).copied().unwrap_or(NativeType::Float);
                if ty == NativeType::Int
                    && !self.small_int_vars.contains(name)
                    && ctx.hoisted.contains(name)
                {
                    return self.gen_init_rug(name, &value.node, &ind);
                }
                self.gen_let_rug(name, ty, &value.node, &ind)
            }
            Statement::Assign { name, value } => {
                let ty = self.var_types.get(name).copied().unwrap_or(NativeType::Float);
                self.gen_assign_rug_typed(name, ty, &value.node, &ind, rest)
            }
            Statement::Return { value } => {
                self.gen_return_rug(&value.node, ctx.fn_ret_type, &ind)
            }
            Statement::If { condition, then_body, else_body } => {
                let cond = self.gen_cond_rug(&condition.node);
                let mut s = format!("{}if {} {{\n", ind, cond);
                s.push_str(&self.gen_body_rug(then_body, indent + 1, ctx));
                if else_body.is_empty() {
                    s.push_str(&format!("{}}}\n", ind));
                } else {
                    s.push_str(&format!("{}}} else {{\n", ind));
                    s.push_str(&self.gen_body_rug(else_body, indent + 1, ctx));
                    s.push_str(&format!("{}}}\n", ind));
                }
                s
            }
            Statement::While { condition, body } => {
                let cond = self.gen_cond_rug(&condition.node);
                let mut s = String::new();

                // Hoist Integer locals declared anywhere in the loop body
                // (recursively) to this scope so GMP buffers persist across
                // iterations.
                let mut new_hoisted: HashSet<String> = HashSet::new();
                self.collect_loop_int_vars(body, &mut new_hoisted);
                for v in &ctx.hoisted { new_hoisted.remove(v); }
                for var in &new_hoisted {
                    s.push_str(&format!("{}let mut {}: Integer = Integer::new();\n", ind, var));
                }

                let mut child = ctx.clone();
                child.hoisted.extend(new_hoisted.iter().cloned());

                s.push_str(&format!("{}while {} {{\n", ind, cond));
                s.push_str(&self.gen_body_rug(body, indent + 1, &child));
                s.push_str(&format!("{}}}\n", ind));
                s
            }
            Statement::For { var, iter, body } => {
                let iter_code = self.gen_for_iter_direct(&iter.node);
                let mut s = format!("{}for {} in {} {{\n", ind, var, iter_code);
                s.push_str(&self.gen_body_rug(body, indent + 1, ctx));
                s.push_str(&format!("{}}}\n", ind));
                s
            }
            Statement::Break => format!("{}break;\n", ind),
            Statement::Continue => format!("{}continue;\n", ind),
            Statement::ExprStmt { expr } => {
                let code = self.gen_expr_rug(&expr.node);
                format!("{}let _ = {};\n", ind, code)
            }
            other => {
                self.err(format!("unsupported statement in Rug mode: {:?}", other));
                format!("{}// codegen error\n", ind)
            }
        }
    }

    /// Generate the body of a `let mut name: T = ...` for a non-hoisted Let.
    fn gen_let_rug(&self, name: &str, ty: NativeType, value: &Expr, ind: &str) -> String {
        match ty {
            NativeType::Int if self.small_int_vars.contains(name) => {
                let expr = self.gen_expr_direct(value, NativeType::Int);
                format!("{}let mut {}: i64 = {};\n", ind, name, expr)
            }
            NativeType::Int => {
                let expr = self.gen_expr_rug(value);
                format!("{}let mut {}: Integer = {};\n", ind, name, expr)
            }
            NativeType::String => {
                let expr = self.gen_expr_rug_string(value);
                format!("{}let mut {}: String = {};\n", ind, name, expr)
            }
            NativeType::Float => {
                let expr = self.gen_expr_direct(value, NativeType::Float);
                format!("{}let mut {}: f64 = {};\n", ind, name, expr)
            }
            NativeType::Bool => {
                let expr = self.gen_expr_direct(value, NativeType::Bool);
                format!("{}let mut {}: bool = {};\n", ind, name, expr)
            }
        }
    }

    /// Type-dispatched `name = value` assignment in Rug mode.
    /// `rest` is the slice of statements that follow this Assign in the
    /// same body — used by the swap optimization to check whether the
    /// source variable is still live after this point.
    fn gen_assign_rug_typed(
        &self,
        name: &str,
        ty: NativeType,
        value: &Expr,
        ind: &str,
        rest: &[Spanned<Statement>],
    ) -> String {
        match ty {
            NativeType::Int if self.small_int_vars.contains(name) => {
                let expr = self.gen_expr_direct(value, NativeType::Int);
                format!("{}{} = {};\n", ind, name, expr)
            }
            NativeType::Int => self.gen_assign_rug(name, value, ind, rest),
            NativeType::String => self.gen_assign_string_rug(name, value, ind),
            NativeType::Float | NativeType::Bool => {
                let expr = self.gen_expr_direct(value, ty);
                format!("{}{} = {};\n", ind, name, expr)
            }
        }
    }

    /// String assignment with the `result = result + to_string(int)` →
    /// `write!(result, "{}", int)` and `result = result + str` → `push_str`
    /// peephole optimizations.
    fn gen_assign_string_rug(&self, name: &str, value: &Expr, ind: &str) -> String {
        if let Expr::BinaryOp { left, op: BinOp::Add, right } = value {
            if let Expr::Ident(ref lname) = left.node {
                if lname == name {
                    // result = result + to_string(int_var) → write!(result, "{}", int_var)
                    if let Expr::FnCall { name: fname, args } = &right.node {
                        if fname == "to_string" && args.len() == 1 {
                            let arg_ty = self.infer_expr_type(&args[0].node);
                            if arg_ty == NativeType::Int {
                                if let Expr::Ident(ref vname) = args[0].node {
                                    return format!(
                                        "{}{{ use std::fmt::Write; write!({}, \"{{}}\", {}).unwrap(); }}\n",
                                        ind, name, vname
                                    );
                                }
                            }
                        }
                    }
                    // General: result.push_str(rhs)
                    let rhs = self.gen_expr_rug_string(&right.node);
                    return format!("{}{}.push_str(&{});\n", ind, name, rhs);
                }
            }
        }
        let expr = self.gen_expr_rug_string(value);
        format!("{}{} = {};\n", ind, name, expr)
    }

    /// Generate a Return statement in Rug mode.
    /// `fn_ret_type` is the function's declared return type — Return statements
    /// must produce a value of THIS type, regardless of what the value expression
    /// happens to be.
    fn gen_return_rug(&self, value: &Expr, fn_ret_type: NativeType, ind: &str) -> String {
        match fn_ret_type {
            NativeType::Int => {
                let expr = self.gen_expr_rug(value);
                format!("{}return {};\n", ind, expr)
            }
            NativeType::String => {
                let expr = self.gen_expr_rug_string(value);
                format!("{}return {};\n", ind, expr)
            }
            NativeType::Float | NativeType::Bool => {
                let expr = self.gen_expr_direct(value, fn_ret_type);
                format!("{}return {};\n", ind, expr)
            }
        }
    }

    fn collect_loop_int_vars(&self, body: &[Spanned<Statement>], vars: &mut HashSet<String>) {
        for stmt in body {
            match &stmt.node {
                Statement::Let { name, .. } => {
                    let ty = self.var_types.get(name).copied().unwrap_or(NativeType::Float);
                    // Don't hoist small Int vars — they're plain i64, not Integer
                    if ty == NativeType::Int && !self.small_int_vars.contains(name) {
                        vars.insert(name.clone());
                    }
                }
                Statement::If { then_body, else_body, .. } => {
                    self.collect_loop_int_vars(then_body, vars);
                    self.collect_loop_int_vars(else_body, vars);
                }
                // Recurse into nested loops too — their Let-bound Integers
                // can also be hoisted to the outer scope so GMP buffers
                // are reused across all iterations.
                Statement::While { body, .. } => {
                    self.collect_loop_int_vars(body, vars);
                }
                Statement::For { body, .. } => {
                    self.collect_loop_int_vars(body, vars);
                }
                _ => {}
            }
        }
    }

    /// Rug-mode expression returning rug::Integer.
    fn gen_expr_rug(&self, expr: &Expr) -> String {
        match expr {
            Expr::Literal(Literal::Int(n)) => format!("Integer::from({}i64)", n),
            Expr::Ident(name) => {
                let var_ty = self.var_types.get(name).copied().unwrap_or(NativeType::Float);
                match var_ty {
                    NativeType::Int if self.small_int_vars.contains(name) => {
                        format!("Integer::from({})", name)
                    }
                    NativeType::Int => format!("{}.clone()", name),
                    NativeType::Float => format!("Integer::from({} as i64)", name),
                    NativeType::Bool => {
                        format!("Integer::from(if {} {{ 1i64 }} else {{ 0i64 }})", name)
                    }
                    NativeType::String => {
                        self.err(format!("cannot use String variable '{}' in an Integer context", name));
                        "Integer::from(0i64)".to_string()
                    }
                }
            }
            Expr::BinaryOp { left, op, right } => {
                let lt = self.infer_expr_type(&left.node);
                let rt = self.infer_expr_type(&right.node);
                if lt == NativeType::Int && rt == NativeType::Int {
                    self.gen_binop_rug(&left.node, *op, &right.node)
                } else {
                    {
                        self.err("mixed Int/Float binary operation in Rug-mode integer expression");
                        "Integer::from(0i64)".to_string()
                    }
                }
            }
            Expr::FnCall { name, args } => self.gen_fn_call_rug(name, args),
            other => {
                self.err(format!("unsupported expression in Rug mode: {:?}", other));
                "Integer::from(0i64)".to_string()
            }
        }
    }

    /// Rug-mode binop returning rug::Integer.
    fn gen_binop_rug(&self, left: &Expr, op: BinOp, right: &Expr) -> String {
        let op_str = arith_op_str(op);
        // Literal-literal: const-fold to avoid Rust's const-evaluator
        // catching overflow at compile time. We're in Rug-mode, so a folded
        // overflow can't be wrapped in catch_unwind — but the Rug fallback
        // doesn't need to handle the overflow path here, since the user
        // wrote literal arithmetic that overflows i64 in source. We emit
        // an Integer literal directly using Integer::from_str (i128 first,
        // BigInt-string second) so the result is correct.
        if let (Expr::Literal(Literal::Int(a)), Expr::Literal(Literal::Int(b))) = (left, right) {
            // Try i64 fold
            let folded: Option<i64> = match op {
                BinOp::Add => a.checked_add(*b),
                BinOp::Sub => a.checked_sub(*b),
                BinOp::Mul => a.checked_mul(*b),
                BinOp::Div => if *b == 0 { None } else { a.checked_div(*b) },
                BinOp::Mod => if *b == 0 { None } else { a.checked_rem(*b) },
                _ => None,
            };
            if let Some(v) = folded {
                return format!("Integer::from({}i64)", v);
            }
            // Folded to BigInt: compute via i128, format as a decimal string,
            // and emit `<rug::Integer as std::str::FromStr>::from_str(...)`.
            // Wrapped in unwrap() — the literal is always well-formed.
            let big: Option<i128> = match op {
                BinOp::Add => Some((*a as i128) + (*b as i128)),
                BinOp::Sub => Some((*a as i128) - (*b as i128)),
                BinOp::Mul => Some((*a as i128) * (*b as i128)),
                _ => None,
            };
            if let Some(v) = big {
                return format!(
                    "<Integer as std::str::FromStr>::from_str(\"{}\").unwrap()",
                    v
                );
            }
        }
        // If both operands are small (i64), do pure i64 arithmetic
        if self.is_small_int_expr(left) && self.is_small_int_expr(right) {
            let l = self.gen_expr_direct(left, NativeType::Int);
            let r = self.gen_expr_direct(right, NativeType::Int);
            return format!("Integer::from({} {} {})", l, op_str, r);
        }
        // Mixed: pick representation that rug supports
        // Rug supports: Integer op &Integer, Integer op u32/i32/u64/i64
        // For "small * Integer" we want: small as i64 * &big = Integer
        let l = self.gen_expr_rug_operand(left);
        let r = self.gen_expr_rug_operand(right);
        format!("Integer::from({} {} {})", l, op_str, r)
    }

    /// Operand for a Rug-mode binop. Same as `gen_expr_rug_ref` but emits
    /// i64 literals (which compose cleanly with both Integer and i64 sides).
    fn gen_expr_rug_operand(&self, expr: &Expr) -> String {
        if let Expr::Literal(Literal::Int(n)) = expr {
            return format!("{}i64", n);
        }
        self.gen_expr_rug_ref(expr)
    }

    /// Generate an "incomplete" rug expression suitable for x.assign(...).
    fn gen_expr_rug_incomplete(&self, expr: &Expr) -> String {
        match expr {
            Expr::Literal(Literal::Int(n)) => format!("{}i64", n),
            Expr::BinaryOp { left, op, right } => {
                let lt = self.infer_expr_type(&left.node);
                let rt = self.infer_expr_type(&right.node);
                if lt == NativeType::Int && rt == NativeType::Int {
                    let op_str = arith_op_str(*op);
                    if self.is_small_int_expr(&left.node) && self.is_small_int_expr(&right.node) {
                        let l = self.gen_expr_direct(&left.node, NativeType::Int);
                        let r = self.gen_expr_direct(&right.node, NativeType::Int);
                        return format!("{} {} {}", l, op_str, r);
                    }
                    let l = self.gen_expr_rug_operand(&left.node);
                    let r = self.gen_expr_rug_operand(&right.node);
                    format!("{} {} {}", l, op_str, r)
                } else {
                    self.gen_expr_rug(expr)
                }
            }
            _ => self.gen_expr_rug(expr),
        }
    }

    /// Convert a Rug-mode Int expression to an i64 (for use as a usize index, etc.)
    fn gen_int_to_i64_rug(&self, expr: &Expr) -> String {
        match expr {
            Expr::Literal(Literal::Int(n)) => format!("{}i64", n),
            Expr::Ident(name) => {
                let var_ty = self.var_types.get(name).copied().unwrap_or(NativeType::Float);
                if var_ty == NativeType::Int {
                    if self.small_int_vars.contains(name) {
                        name.clone()
                    } else {
                        format!("{}.to_i64().unwrap()", name)
                    }
                } else {
                    format!("{} as i64", name)
                }
            }
            Expr::BinaryOp { left, op, right } => {
                let l = self.gen_int_to_i64_rug(&left.node);
                let r = self.gen_int_to_i64_rug(&right.node);
                format!("({} {} {})", l, arith_op_str(*op), r)
            }
            _ => format!("{}.to_i64().unwrap()", self.gen_expr_rug(expr)),
        }
    }

    /// Borrow-form `&Integer` expression for rug API methods that take `&Integer`.
    /// For Ident vars: emit `&name` (no copy). For literals or compound exprs:
    /// fall back to `&Integer::from(...)` (one alloc, but unavoidable).
    fn gen_int_borrow_rug(&self, expr: &Expr) -> String {
        match expr {
            Expr::Ident(name) => {
                let var_ty = self.var_types.get(name).copied().unwrap_or(NativeType::Float);
                if var_ty == NativeType::Int {
                    if self.small_int_vars.contains(name) {
                        // i64 variable — wrap in Integer (allocates)
                        format!("&Integer::from({})", name)
                    } else {
                        // Already an Integer — just borrow
                        format!("&{}", name)
                    }
                } else {
                    format!("&Integer::from({} as i64)", name)
                }
            }
            Expr::Literal(Literal::Int(n)) => format!("&Integer::from({}i64)", n),
            _ => format!("&{}", self.gen_expr_rug(expr)),
        }
    }

    /// Reference-form Integer expression for use as operand.
    /// For "big" Integer vars: emit &name. For "small" i64 vars: emit name (rug ops support i64).
    fn gen_expr_rug_ref(&self, expr: &Expr) -> String {
        match expr {
            Expr::Literal(Literal::Int(n)) => {
                if *n >= 0 && *n <= u32::MAX as i64 {
                    format!("{}u32", n)
                } else if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 {
                    format!("{}i32", n)
                } else {
                    format!("&Integer::from({}i64)", n)
                }
            }
            Expr::Ident(name) => {
                let var_ty = self.var_types.get(name).copied().unwrap_or(NativeType::Float);
                match var_ty {
                    NativeType::Int if self.small_int_vars.contains(name) => name.clone(),
                    NativeType::Int => format!("&{}", name),
                    NativeType::Float => format!("({} as i64)", name),
                    NativeType::Bool => format!("(if {} {{ 1i64 }} else {{ 0i64 }})", name),
                    NativeType::String => {
                        self.err(format!("cannot use String variable '{}' as an Integer operand", name));
                        "0i64".to_string()
                    }
                }
            }
            Expr::BinaryOp { left, op, right } => {
                let inner = self.gen_binop_rug(&left.node, *op, &right.node);
                format!("&{}", inner)
            }
            Expr::FnCall { name, args } => {
                let inner = self.gen_fn_call_rug(name, args);
                format!("&{}", inner)
            }
            _ => format!("&{}", self.gen_expr_rug(expr)),
        }
    }

    /// Initialize a hoisted Integer with `name = value`.
    /// Unlike gen_assign_rug, this does NOT swap (Let statements must
    /// preserve the source value for later reads).
    fn gen_init_rug(&self, name: &str, value: &Expr, ind: &str) -> String {
        // Literal: assign(N)
        if let Expr::Literal(Literal::Int(n)) = value {
            return format!("{}{}.assign({}i64);\n", ind, name, n);
        }
        // Ident: clone_from (no swap — source must remain valid)
        if let Expr::Ident(src) = value {
            let var_ty = self.var_types.get(src).copied().unwrap_or(NativeType::Float);
            if var_ty == NativeType::Int {
                if self.small_int_vars.contains(src) {
                    return format!("{}{}.assign({});\n", ind, name, src);
                }
                return format!("{}{}.clone_from(&{});\n", ind, name, src);
            }
        }
        // General: assign from incomplete expression
        let expr = self.gen_expr_rug_incomplete(value);
        format!("{}{}.assign({});\n", ind, name, expr)
    }

    /// Efficient in-place Integer assignment for `name = value` (Rug mode).
    ///
    /// Dispatch ladder, in priority order:
    ///   1. Peephole: `name = name * name`            → `name.square_mut()`
    ///   2. Peephole: `name = name OP rhs`            → `name OP= rhs`
    ///   3. Peephole: `name = lhs OP name` (+, *)     → `name OP= lhs`
    ///   4. Literal:  `name = N`                       → `name.assign(N)`
    ///   5. Ident:    `name = src` (different)
    ///        → `mem::swap(name, src)` if src is dead in the rest of the body
    ///        → `name.clone_from(&src)` otherwise
    ///   6. Self-ref: RHS reads `name` somewhere       → temp + swap (avoid borrow)
    ///   7. General:                                   → `name.assign(incomplete)`
    fn gen_assign_rug(
        &self,
        name: &str,
        value: &Expr,
        ind: &str,
        rest: &[Spanned<Statement>],
    ) -> String {
        if let Some(s) = self.try_square_mut(name, value, ind) { return s; }
        if let Some(s) = self.try_inplace_op(name, value, ind) { return s; }

        if let Expr::Literal(Literal::Int(n)) = value {
            return format!("{}{}.assign({}i64);\n", ind, name, n);
        }

        if let Expr::Ident(src) = value {
            let var_ty = self.var_types.get(src).copied().unwrap_or(NativeType::Float);
            if var_ty == NativeType::Int && src != name {
                // Swap is correct only if `src` is not read in the rest of
                // the current body. Otherwise the read would see `name`'s
                // OLD value (which the swap put into `src`).
                if !body_references(rest, src) {
                    return format!("{}std::mem::swap(&mut {}, &mut {});\n", ind, name, src);
                }
                return format!("{}{}.clone_from(&{});\n", ind, name, src);
            }
        }

        if Self::expr_references(value, name) {
            let expr = self.gen_expr_rug(value);
            return format!(
                "{}{{ let mut _t: Integer = {}; std::mem::swap(&mut {}, &mut _t); }}\n",
                ind, expr, name
            );
        }

        let expr = self.gen_expr_rug_incomplete(value);
        format!("{}{}.assign({});\n", ind, name, expr)
    }

    /// Match `name = name * name` and produce `name.square_mut()`.
    fn try_square_mut(&self, name: &str, value: &Expr, ind: &str) -> Option<String> {
        if let Expr::BinaryOp { left, op: BinOp::Mul, right } = value {
            if let (Expr::Ident(ln), Expr::Ident(rn)) = (&left.node, &right.node) {
                if ln == name && rn == name {
                    return Some(format!("{}{}.square_mut();\n", ind, name));
                }
            }
        }
        None
    }

    /// Match `name = name OP rhs` (or `name = lhs OP name` for commutative ops)
    /// and produce `name OP= rhs`. Bails out if the *other* side references
    /// `name` too — that would cause a borrow conflict on `name`.
    fn try_inplace_op(&self, name: &str, value: &Expr, ind: &str) -> Option<String> {
        let Expr::BinaryOp { left, op, right } = value else { return None; };
        let op_assign = match op {
            BinOp::Add => "+=", BinOp::Sub => "-=", BinOp::Mul => "*=",
            BinOp::Div => "/=", BinOp::Mod => "%=", _ => return None,
        };

        // Form 1: name = name OP rhs
        if let Expr::Ident(lname) = &left.node {
            if lname == name && !Self::expr_references(&right.node, name) {
                let r = self.gen_expr_rug_ref(&right.node);
                return Some(format!("{}{} {} {};\n", ind, name, op_assign, r));
            }
        }

        // Form 2: name = lhs OP name (only valid for commutative + and *)
        if matches!(op, BinOp::Add | BinOp::Mul) {
            if let Expr::Ident(rname) = &right.node {
                if rname == name && !Self::expr_references(&left.node, name) {
                    let l = self.gen_expr_rug_ref(&left.node);
                    return Some(format!("{}{} {} {};\n", ind, name, op_assign, l));
                }
            }
        }

        None
    }

    /// Boolean condition for if/while in Rug mode.
    fn gen_cond_rug(&self, expr: &Expr) -> String {
        match expr {
            Expr::CmpOp { left, op, right } => {
                let op_str = cmp_op_str(*op);
                let lt = self.infer_expr_type(&left.node);
                let rt = self.infer_expr_type(&right.node);
                if lt == NativeType::Int && rt == NativeType::Int {
                    // Both Int — emit either pure-i64 or Integer-aware comparison.
                    if self.is_small_int_expr(&left.node) && self.is_small_int_expr(&right.node) {
                        let l = self.gen_expr_direct(&left.node, NativeType::Int);
                        let r = self.gen_expr_direct(&right.node, NativeType::Int);
                        return format!("({} {} {})", l, op_str, r);
                    }
                    let l = self.gen_cmp_operand_rug(&left.node);
                    let r = self.gen_cmp_operand_rug(&right.node);
                    format!("{} {} {}", l, op_str, r)
                } else if lt == NativeType::Bool && rt == NativeType::Bool {
                    // Bool == Bool — keep both sides as bool, no i64 coercion.
                    let l = self.gen_expr_direct(&left.node, NativeType::Bool);
                    let r = self.gen_expr_direct(&right.node, NativeType::Bool);
                    format!("({} {} {})", l, op_str, r)
                } else {
                    // Float / mixed: lower via the direct-mode helpers.
                    let common = if lt == NativeType::Float || rt == NativeType::Float {
                        NativeType::Float
                    } else {
                        NativeType::Int
                    };
                    let l = self.gen_expr_direct(&left.node, common);
                    let r = self.gen_expr_direct(&right.node, common);
                    format!("({} {} {})", l, op_str, r)
                }
            }
            Expr::Not(inner) => {
                let e = self.gen_cond_rug(&inner.node);
                format!("(!{})", e)
            }
            Expr::BinaryOp { left, op, right } if matches!(op, BinOp::And | BinOp::Or) => {
                let l = self.gen_cond_rug(&left.node);
                let r = self.gen_cond_rug(&right.node);
                let op_str = if matches!(op, BinOp::And) { "&&" } else { "||" };
                format!("({} {} {})", l, op_str, r)
            }
            Expr::Literal(Literal::Bool(b)) => format!("{}", b),
            Expr::Ident(name) => name.clone(),
            other => {
                self.err(format!("unsupported boolean condition in Rug mode: {:?}", other));
                "true".to_string()
            }
        }
    }

    /// True if expr references the given variable name anywhere.
    fn expr_references(expr: &Expr, name: &str) -> bool {
        match expr {
            Expr::Ident(n) => n == name,
            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
                Self::expr_references(&left.node, name) || Self::expr_references(&right.node, name)
            }
            Expr::Not(inner) => Self::expr_references(&inner.node, name),
            Expr::FnCall { args, .. } => args.iter().any(|a| Self::expr_references(&a.node, name)),
            _ => false,
        }
    }

    /// True if expr only references small_int_vars or i64 literals.
    fn is_small_int_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Literal(Literal::Int(_)) => true,
            Expr::Ident(name) => self.small_int_vars.contains(name),
            Expr::BinaryOp { left, right, .. } => {
                self.is_small_int_expr(&left.node) && self.is_small_int_expr(&right.node)
            }
            _ => false,
        }
    }

    fn gen_cmp_operand_rug(&self, expr: &Expr) -> String {
        match expr {
            Expr::Literal(Literal::Int(n)) => {
                // rug::Integer compares with i64 directly
                format!("{}i64", n)
            }
            Expr::Ident(name) => name.clone(),
            _ => self.gen_expr_rug(expr),
        }
    }

    /// String expression in Rug mode.
    fn gen_expr_rug_string(&self, expr: &Expr) -> String {
        match expr {
            Expr::Literal(Literal::String(s)) => {
                format!("\"{}\".to_string()", s.replace('\\', "\\\\").replace('"', "\\\""))
            }
            Expr::BinaryOp { left, op: BinOp::Add, right } => {
                let l = self.gen_expr_rug_string(&left.node);
                let r = self.gen_expr_rug_string(&right.node);
                format!("format!(\"{{}}{{}}\", {}, {})", l, r)
            }
            Expr::FnCall { name, args } if name == "to_string" && args.len() == 1 => {
                let arg_ty = self.infer_expr_type(&args[0].node);
                if arg_ty == NativeType::Int {
                    if let Expr::Ident(ref vname) = args[0].node {
                        format!("{}.to_string()", vname)
                    } else {
                        let a = self.gen_expr_rug(&args[0].node);
                        format!("{}.to_string()", a)
                    }
                } else {
                    let a = self.gen_expr_direct(&args[0].node, arg_ty);
                    format!("format!(\"{{}}\", {})", a)
                }
            }
            // Sibling call returning String — call directly without forcing
            // Integer context (which would error). Marshals args based on the
            // sibling's parameter types.
            Expr::FnCall { name, args }
                if self.siblings.contains(name)
                    && self.sibling_info.get(name).map(|i| i.return_type) == Some(NativeType::String) =>
            {
                let info = self.sibling_info.get(name).cloned().unwrap();
                let arg_strs: Vec<String> = args.iter().zip(info.param_types.iter())
                    .map(|(arg, &expected)| self.marshal_arg_to_sibling(&arg.node, expected, info.mode))
                    .collect();
                format!("inner_handler_{}({})", name, arg_strs.join(", "))
            }
            Expr::Ident(name) => {
                let var_ty = self.var_types.get(name).copied().unwrap_or(NativeType::Float);
                if var_ty == NativeType::String {
                    format!("{}.clone()", name)
                } else if var_ty == NativeType::Int {
                    format!("{}.to_string()", name)
                } else {
                    format!("format!(\"{{}}\", {})", name)
                }
            }
            _ => {
                let inner = self.gen_expr_rug(expr);
                format!("{}.to_string()", inner)
            }
        }
    }

    fn gen_fn_call_rug(&self, name: &str, args: &[Spanned<Expr>]) -> String {
        match name {
            "abs" => {
                let a = self.gen_expr_rug(&args[0].node);
                format!("({}).abs()", a)
            }
            // String introspection — return Int
            "str_len" | "len" if args.len() == 1 => {
                let arg_ty = self.infer_expr_type(&args[0].node);
                if arg_ty == NativeType::String {
                    if let Expr::Ident(name) = &args[0].node {
                        return format!("Integer::from({}.len() as i64)", name);
                    }
                    let a = self.gen_expr_direct(&args[0].node, NativeType::String);
                    format!("Integer::from(({}).len() as i64)", a)
                } else {
                    self.err("len() / str_len() only supported on String");
                    "Integer::from(0i64)".to_string()
                }
            }
            "str_at" if args.len() == 2 => {
                let s_name = if let Expr::Ident(name) = &args[0].node {
                    name.clone()
                } else {
                    self.gen_expr_direct(&args[0].node, NativeType::String)
                };
                // The index is an Int expression in Rug mode (could be Integer)
                // — produce code that yields a usize.
                let i_int = self.gen_int_to_i64_rug(&args[1].node);
                format!("Integer::from({}.as_bytes()[({}) as usize] as i64)", s_name, i_int)
            }
            "min" | "max" if args.len() == 2 => {
                let a = self.gen_expr_rug(&args[0].node);
                let b = self.gen_expr_rug(&args[1].node);
                let cmp = if name == "min" { "<" } else { ">" };
                format!("{{ let _a = {}; let _b = {}; if _a {} _b {{ _a }} else {{ _b }} }}", a, b, cmp)
            }
            "to_int" => {
                let a_ty = self.infer_expr_type(&args[0].node);
                if a_ty == NativeType::Int {
                    self.gen_expr_rug(&args[0].node)
                } else {
                    let a = self.gen_expr_direct(&args[0].node, a_ty);
                    format!("Integer::from({} as i64)", a)
                }
            }
            "floor" | "ceil" | "round" => {
                let a = self.gen_expr_direct(&args[0].node, NativeType::Float);
                format!("Integer::from(({}).{}() as i64)", a, name)
            }
            // Bit operations on Integer — need owned operands so the result
            // can be a BigInt regardless of operand sizes.
            "band" | "bor" | "bxor" if args.len() == 2 => {
                let a = self.gen_expr_rug(&args[0].node);
                let b = self.gen_expr_rug(&args[1].node);
                let op = match name { "band" => "&", "bor" => "|", _ => "^" };
                format!("Integer::from(({}) {} ({}))", a, op, b)
            }
            "bnot" if args.len() == 1 => {
                let a = self.gen_expr_rug(&args[0].node);
                format!("Integer::from(!({}))", a)
            }
            "shl" if args.len() == 2 => {
                // Wrap the base in Integer so the shift can produce a BigInt
                // (u32 << u32 in plain Rust would panic for shift > 31).
                let a = self.gen_expr_rug(&args[0].node);
                let b = self.gen_int_to_i64_rug(&args[1].node);
                format!("Integer::from(({}) << (({}) as u32))", a, b)
            }
            "shr" if args.len() == 2 => {
                let a = self.gen_expr_rug(&args[0].node);
                let b = self.gen_int_to_i64_rug(&args[1].node);
                format!("Integer::from(({}) >> (({}) as u32))", a, b)
            }
            "bit_len" if args.len() == 1 => {
                let a = self.gen_expr_rug(&args[0].node);
                format!("Integer::from(({}).significant_bits() as i64)", a)
            }
            // Number theory: rug has these as methods. All args must be &Integer.
            "gcd" if args.len() == 2 => {
                let a = self.gen_expr_rug(&args[0].node);
                // Second arg as a borrowed Integer reference where possible.
                let b_ref = self.gen_int_borrow_rug(&args[1].node);
                format!("({}).gcd({})", a, b_ref)
            }
            "pow_mod" if args.len() == 3 => {
                // pow_mod(self, &exp, &modulus) -> Result<Integer, _>
                let base = self.gen_expr_rug(&args[0].node);
                let exp = self.gen_int_borrow_rug(&args[1].node);
                let m = self.gen_int_borrow_rug(&args[2].node);
                format!("({}).pow_mod({}, {}).expect(\"pow_mod failed (modulus 0?)\")", base, exp, m)
            }
            "sqrt_int" if args.len() == 1 => {
                let a = self.gen_expr_rug(&args[0].node);
                format!("({}).sqrt()", a)
            }
            other => {
                // Sibling call from Rug mode — return type Integer
                if let Some(info) = self.sibling_info.get(other).cloned() {
                    return self.gen_sibling_call_from_rug_int(other, args, &info);
                }
                {
                    self.err(format!("unknown function call: {} (not a sibling, not a builtin)", other));
                    "Integer::from(0i64)".to_string()
                }
            }
        }
    }

    /// Generate a sibling call from a Rug-mode caller, returning Integer.
    /// Used when we need an Integer result (e.g., inside a rug expression).
    fn gen_sibling_call_from_rug_int(
        &self,
        name: &str,
        args: &[Spanned<Expr>],
        sibling: &SiblingInfo,
    ) -> String {
        let arg_strs: Vec<String> = args.iter().zip(sibling.param_types.iter())
            .map(|(arg, &expected)| self.marshal_arg_to_sibling(&arg.node, expected, sibling.mode))
            .collect();
        let call = format!("inner_handler_{}({})", name, arg_strs.join(", "));
        // Convert return to Integer
        match (sibling.mode, sibling.return_type) {
            (Mode::Rug, NativeType::Int) => call,  // already Integer
            (Mode::Direct, NativeType::Int) => format!("Integer::from({})", call),
            (_, NativeType::Float) => format!("Integer::from({} as i64)", call),
            (_, NativeType::Bool) => format!("Integer::from(if {} {{ 1i64 }} else {{ 0i64 }})", call),
            (_, NativeType::String) => {
                self.err("cannot use a String-returning sibling in an Integer context");
                "Integer::from(0i64)".to_string()
            }
        }
    }

    /// Marshal an arg expression for a sibling call.
    /// `expected`: the parameter's NativeType
    /// `callee_mode`: the sibling's mode
    fn marshal_arg_to_sibling(&self, expr: &Expr, expected: NativeType, callee_mode: Mode) -> String {
        if expected == NativeType::Int && callee_mode == Mode::Rug {
            // Callee expects Integer
            self.gen_expr_rug(expr)
        } else if expected == NativeType::Int && callee_mode == Mode::Direct {
            // Callee expects i64
            let arg_ty = self.infer_expr_type(expr);
            if arg_ty == NativeType::Int && self.mode == Mode::Rug {
                if let Expr::Ident(name) = expr {
                    if self.small_int_vars.contains(name) {
                        return name.clone();
                    }
                    return format!("({}.to_i64().expect(\"BigInt overflow in sibling call\"))", name);
                }
                let e = self.gen_expr_rug(expr);
                format!("({}).to_i64().expect(\"BigInt overflow in sibling call\")", e)
            } else {
                self.gen_expr_direct(expr, NativeType::Int)
            }
        } else if expected == NativeType::String {
            // String arg: always clone at the call site to avoid move-then-borrow
            // conflicts when the same variable appears in multiple positions.
            if let Expr::Ident(name) = expr {
                format!("{}.clone()", name)
            } else {
                self.gen_expr_direct(expr, NativeType::String)
            }
        } else {
            self.gen_expr_direct(expr, expected)
        }
    }
}

// ── Shared buffer source for Rug mode ───────────────────────────────

const SHARED_BUFFER_RUG: &str = r#"
// Shared arg/result buffers for Rug-mode handlers.
// Int args go in _SOMA_ARGS; String args go in _SOMA_STRING_ARGS.
// Each buffer is a flat positional array; the handler param-reader knows
// which positional index in each buffer corresponds to which parameter.
static mut _SOMA_ARGS: Vec<Integer> = Vec::new();
static mut _SOMA_STRING_ARGS: Vec<String> = Vec::new();
static mut _SOMA_RESULT: Option<String> = None;

#[no_mangle]
pub extern "C" fn _soma_clear_args() {
    unsafe {
        _SOMA_ARGS.clear();
        _SOMA_STRING_ARGS.clear();
    }
}

#[no_mangle]
pub extern "C" fn _soma_push_i64(v: i64) {
    unsafe { _SOMA_ARGS.push(Integer::from(v)); }
}

#[no_mangle]
pub extern "C" fn _soma_push_bigint(ptr: *const u8, len: i64) {
    unsafe {
        let bytes = std::slice::from_raw_parts(ptr, len as usize);
        let s = std::str::from_utf8_unchecked(bytes);
        let val = Integer::parse(s).unwrap();
        _SOMA_ARGS.push(Integer::from(val));
    }
}

#[no_mangle]
pub extern "C" fn _soma_push_string(ptr: *const u8, len: i64) {
    unsafe {
        let bytes = std::slice::from_raw_parts(ptr, len as usize);
        _SOMA_STRING_ARGS.push(std::str::from_utf8_unchecked(bytes).to_string());
    }
}

#[no_mangle]
pub extern "C" fn _soma_result_ptr() -> *const u8 {
    unsafe { _SOMA_RESULT.as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null()) }
}

#[no_mangle]
pub extern "C" fn _soma_result_len() -> i64 {
    unsafe { _SOMA_RESULT.as_ref().map(|s| s.len() as i64).unwrap_or(0) }
}
"#;

// ── random() detection ──────────────────────────────────────────────

fn body_uses_random(body: &[Spanned<Statement>]) -> bool {
    body.iter().any(|s| stmt_uses_random(&s.node))
}

fn stmt_uses_random(stmt: &Statement) -> bool {
    match stmt {
        Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => {
            expr_uses_random(&value.node)
        }
        Statement::If { condition, then_body, else_body } => {
            expr_uses_random(&condition.node) || body_uses_random(then_body) || body_uses_random(else_body)
        }
        Statement::While { condition, body } => {
            expr_uses_random(&condition.node) || body_uses_random(body)
        }
        Statement::For { iter, body, .. } => {
            expr_uses_random(&iter.node) || body_uses_random(body)
        }
        Statement::ExprStmt { expr } => expr_uses_random(&expr.node),
        _ => false,
    }
}

fn expr_uses_random(expr: &Expr) -> bool {
    match expr {
        Expr::FnCall { name, args } => {
            name == "random" || args.iter().any(|a| expr_uses_random(&a.node))
        }
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
            expr_uses_random(&left.node) || expr_uses_random(&right.node)
        }
        Expr::Not(inner) => expr_uses_random(&inner.node),
        _ => false,
    }
}
