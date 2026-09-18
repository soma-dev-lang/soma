//! Validates that [native] handlers only use the allowed numeric subset.
//!
//! Allowed: Int, Float, Bool params/returns. Arithmetic, comparison, logic ops.
//! if/else, while, for, break, continue. let, assignment. Math builtins.
//! Calls to other [native] handlers in the same cell.
//!
//! Forbidden: String, Map, pipes, storage access, print, HTTP, signals, lambdas.

use crate::ast::*;
use std::collections::HashSet;

/// Error from native handler validation
#[derive(Debug)]
pub struct NativeCheckError {
    pub handler_name: String,
    pub reason: String,
}

impl std::fmt::Display for NativeCheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error: handler '{}' is marked [native] but {}\n  → [native] handlers can only use Int, Float, Bool and numeric operations",
            self.handler_name, self.reason
        )
    }
}

/// Set of handler names that are also [native] in the same cell.
/// Used to allow calls between native handlers.
pub type NativeSiblings = std::collections::HashSet<String>;

/// Validate a [native] handler's parameter types.
fn check_params(handler_name: &str, params: &[Param]) -> Result<(), NativeCheckError> {
    for p in params {
        let is_list = matches!(&p.ty.node, TypeExpr::Generic { name, .. } | TypeExpr::Simple(name) if name == "List");
        if is_list {
            // the codegen lowered a List<Float> parameter to a bare f64 and
            // the failure was a rustc dump — say what the boundary takes
            return Err(NativeCheckError {
                handler_name: handler_name.to_string(),
                reason: format!(
                    "parameter '{}' is a List — [native] parameters are Int, Float, Bool or String (a list cannot cross the boundary): keep the loop interpreted and make the per-element kernel [native], or build the data inside the handler with buffer(n) / buf_set / buf_get",
                    p.name
                ),
            });
        }
        if !is_native_type(&p.ty.node) {
            return Err(NativeCheckError {
                handler_name: handler_name.to_string(),
                reason: format!("parameter '{}' has type {} — [native] parameters are Int, Float, Bool or String", p.name, crate::commands::describe::format_type(&p.ty.node)),
            });
        }
    }
    Ok(())
}

fn is_native_type(ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Simple(name) => matches!(name.as_str(), "Int" | "Float" | "Bool" | "String"),
        TypeExpr::Generic { name, args } => {
            name == "List" && args.len() == 1 && matches!(&args[0].node, TypeExpr::Simple(s) if s == "Int" || s == "Float")
        }
        _ => false,
    }
}

pub(crate) const ALLOWED_BUILTINS: &[&str] = &[
    "sqrt", "log", "exp", "pow", "abs", "min", "max", "random",
    "len", "nth", "range", "floor", "ceil", "round", "sin", "cos",
    // Pipe operations (generate parallel native code)
    "map", "filter", "reduce", "fold",
    // Type conversions
    "to_float", "to_int", "to_string",
    // Bit operations (Int)
    "band", "bor", "bxor", "bnot", "shl", "shr", "bit_len",
    "bit_test", "bit_set", "bit_clr", "bit_next",
    // Number theory
    "pow_mod", "gcd", "sqrt_int", "idiv",
    // String introspection
    "str_len", "str_at", "str_eq",
    // Fixed-size i64 buffer (random-access array primitive for [native])
    // — `buffer(N)` creates a Vec<i64> of size N initialized to 0;
    //   `buf_get(buf, i)` reads buf[i]; `buf_set(buf, i, v)` writes buf[i] = v.
    // Closes the array-primitive gap with Numba/Cython.
    "buffer", "buf_get", "buf_set",
    "buffer_f", "buf_get_f", "buf_set_f",
    // HashMap<Int, Int> primitive — compiles to std::collections::HashMap
    "hashmap", "hm_get", "hm_set", "hm_inc", "hm_len", "hm_has",
    // Regex builtins — compile to Rust `regex` crate calls
    "regex_count", "regex_replace", "regex_match",
    // StringBuf primitive — preallocated growable string
    "strbuf", "sb_push", "sb_push_int", "sb_push_char", "sb_finish", "sb_len",
    // File I/O — for [native] handlers that need to read files / stdin
    "read_file", "read_stdin", "write_str",
];

/// Validate all statements in a [native] handler body.
pub fn check_native_handler(
    handler_name: &str,
    params: &[Param],
    body: &[Spanned<Statement>],
    siblings: &NativeSiblings,
) -> Result<(), NativeCheckError> {
    check_params(handler_name, params)?;
    for stmt in body {
        check_stmt(handler_name, &stmt.node, siblings)?;
    }
    // what codegen refuses, said here (each used to pass check and die in
    // rustc behind a 600-line dump)
    let mut bufs: HashSet<String> = HashSet::new();
    check_codegen_limits(handler_name, body, &mut bufs)?;
    // String values in native code: text is read (str_* builtins, `==`),
    // not parsed, ordered or grown by `+` (each was a rustc error)
    let mut texts: HashSet<String> = params.iter()
        .filter(|p| matches!(&p.ty.node, TypeExpr::Simple(t) if t == "String"))
        .map(|p| p.name.clone()).collect();
    for st in body {
        if let Statement::Let { name, value } = &st.node {
            if matches!(value.node, Expr::Literal(Literal::String(_))) || matches!(&value.node, Expr::Ident(n) if texts.contains(n)) { texts.insert(name.clone()); }
        }
    }
    let is_text = |e: &Expr| matches!(e, Expr::Literal(Literal::String(_))) || matches!(e, Expr::Ident(n) if texts.contains(n));
    let mut problem: Option<String> = None;
    crate::checker::literals::for_each_expr(body, &mut |e| {
        if problem.is_some() { return; }
        match e {
            Expr::FnCall { name, args } if matches!(name.as_str(), "to_int" | "to_float") && args.first().map_or(false, |a| is_text(&a.node)) =>
                problem = Some(format!("{}() of a String is not native (parse text in an interpreted handler and pass the number)", name)),
            Expr::CmpOp { left, op, right } if matches!(op, CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge) && (is_text(&left.node) || is_text(&right.node)) =>
                problem = Some("orders Strings with < / > — native code compares text only with == / !=".to_string()),
            _ => {}
        }
    });
    fn grows_text(stmts: &[Spanned<Statement>], texts: &HashSet<String>) -> Option<String> {
        stmts.iter().find_map(|st| match &st.node {
            Statement::Assign { name, value } if texts.contains(name) && matches!(value.node, Expr::BinaryOp { op: BinOp::Add, .. }) => Some(name.clone()),
            Statement::If { then_body, else_body, .. } => grows_text(then_body, texts).or_else(|| grows_text(else_body, texts)),
            Statement::For { body, .. } | Statement::While { body, .. } => grows_text(body, texts),
            _ => None,
        })
    }
    // only a local that ALIASES a String parameter (`let u = s`, a &str in
    // Rust) cannot grow; `let r = ""  r = r + …` compiles
    let param_aliases: HashSet<String> = body.iter().filter_map(|st| match &st.node {
        Statement::Let { name, value } if matches!(&value.node, Expr::Ident(n) if params.iter().any(|p| &p.name == n && matches!(&p.ty.node, TypeExpr::Simple(t) if t == "String"))) => Some(name.clone()),
        _ => None,
    }).collect();
    if problem.is_none() {
        if let Some(n) = grows_text(body, &param_aliases) {
            problem = Some(format!("grows the String `{}` with + — build text with strbuf() / sb_push / sb_finish", n));
        }
    }
    // a Float buffer index was truncated silently (buf_set(b, 1.7, 9) wrote b[1])
    if problem.is_none() {
        let mut floats: HashSet<String> = params.iter()
            .filter(|p| matches!(&p.ty.node, TypeExpr::Simple(t) if t == "Float"))
            .map(|p| p.name.clone()).collect();
        for st in body {
            if let Statement::Let { name, value } = &st.node {
                if has_float_literal(&value.node) || matches!(&value.node, Expr::FnCall { name, args } if name == "random" && args.is_empty()) {
                    floats.insert(name.clone());
                }
            }
        }
        let floaty = |e: &Expr| has_float_literal(e) || matches!(e, Expr::Ident(n) if floats.contains(n))
            || matches!(e, Expr::FnCall { name, args } if name == "random" && args.is_empty());
        crate::checker::literals::for_each_expr(body, &mut |e| {
            if problem.is_some() { return; }
            if let Expr::FnCall { name, args } = e {
                if matches!(name.as_str(), "buf_get" | "buf_set" | "buf_get_f" | "buf_set_f") && args.get(1).map_or(false, |a| floaty(&a.node)) {
                    problem = Some(format!("{}() with a Float index — an index is an Int (use floor() / to_int() explicitly)", name));
                }
            }
        });
    }
    // names Rust reserves: `let fn = 1` / `on f(loop: Int)` passed check and
    // rustc refused the generated code
    if problem.is_none() {
        const RUST_WORDS: &[&str] = &["as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen", "union", "None", "Some", "Ok", "Err", "Vec", "String", "Option", "Result", "Box"];
        let mut names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        fn binders(stmts: &[Spanned<Statement>], out: &mut Vec<String>) {
            for st in stmts {
                match &st.node {
                    Statement::Let { name, .. } => out.push(name.clone()),
                    Statement::For { var, body, .. } => { out.push(var.clone()); binders(body, out); }
                    Statement::If { then_body, else_body, .. } => { binders(then_body, out); binders(else_body, out); }
                    Statement::While { body, .. } => binders(body, out),
                    _ => {}
                }
            }
        }
        binders(body, &mut names);
        if let Some(n) = names.iter().find(|n| RUST_WORDS.contains(&n.as_str())) {
            problem = Some(format!("names a value `{}`, a word Rust reserves — rename it (`{}_`)", n, n));
        }
    }
    // a Bool in arithmetic: native code turned `b * 10 + 1` into a Bool
    // (true), the interpreter raises "cannot add Bool and Int"
    if problem.is_none() {
        let mut bools: HashSet<String> = params.iter().filter(|p| matches!(&p.ty.node, TypeExpr::Simple(t) if t == "Bool")).map(|p| p.name.clone()).collect();
        fn is_bool(e: &Expr, bools: &HashSet<String>) -> bool {
            match e {
                Expr::CmpOp { .. } | Expr::Not(_) | Expr::Literal(Literal::Bool(_)) => true,
                Expr::BinaryOp { op: BinOp::And | BinOp::Or, .. } => true,
                Expr::Ident(n) => bools.contains(n),
                _ => false,
            }
        }
        fn lets(stmts: &[Spanned<Statement>], bools: &mut HashSet<String>) {
            for st in stmts {
                match &st.node {
                    Statement::Let { name, value } => { if is_bool(&value.node, bools) { bools.insert(name.clone()); } else { bools.remove(name); } }
                    Statement::If { then_body, else_body, .. } => { lets(then_body, bools); lets(else_body, bools); }
                    Statement::For { body, .. } | Statement::While { body, .. } => lets(body, bools),
                    _ => {}
                }
            }
        }
        lets(body, &mut bools);
        crate::checker::literals::for_each_expr(body, &mut |e| {
            if problem.is_some() { return; }
            if let Expr::BinaryOp { left, op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod, right } = e {
                if is_bool(&left.node, &bools) || is_bool(&right.node, &bools) {
                    problem = Some("does arithmetic on a Bool (the interpreter raises \"cannot add Bool and Int\"; native code gave a Bool) — write `if b { 1 } else { 0 }`".to_string());
                }
            }
        });
    }
    // `let x = n + 1  …  x = to_string(x)`: a native local keeps the Rust
    // type of its `let` (the error blamed to_string's "BigInt mode")
    if problem.is_none() {
        fn kind(e: &Expr, kinds: &std::collections::HashMap<String, char>) -> Option<char> {
            match e {
                Expr::Literal(Literal::String(_)) => Some('s'),
                Expr::Literal(Literal::Int(_) | Literal::BigInt(_) | Literal::Float(_)) => Some('n'),
                Expr::Literal(Literal::Bool(_)) => Some('b'),
                Expr::FnCall { name, .. } if matches!(name.as_str(), "to_string" | "concat" | "format" | "substring" | "upper" | "lower" | "trim") => Some('s'),
                Expr::BinaryOp { op: BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod, .. } => Some('n'),
                Expr::BinaryOp { op: BinOp::Add, left, right } => kind(&left.node, kinds).filter(|k| *k == 'n').or_else(|| kind(&right.node, kinds).filter(|k| *k == 'n')),
                Expr::Ident(n) => kinds.get(n).copied(),
                _ => None,
            }
        }
        fn walk(stmts: &[Spanned<Statement>], kinds: &mut std::collections::HashMap<String, char>) -> Option<String> {
            for st in stmts {
                let hit = match &st.node {
                    Statement::Let { name, value } => { match kind(&value.node, kinds) { Some(k) => { kinds.insert(name.clone(), k); } None => { kinds.remove(name); } } None }
                    Statement::Assign { name, value } => match (kinds.get(name), kind(&value.node, kinds)) {
                        (Some(a), Some(b)) if *a != b => {
                            let w = |k: char| match k { 's' => "a String", 'n' => "a number", _ => "a Bool" };
                            Some(format!("the local `{}` is bound to {} by its `let`, then assigned {} — a native local keeps the type of its `let`; give the new value its own name (`let {}_text = …`)", name, w(*a), w(b), name))
                        }
                        _ => None,
                    },
                    Statement::If { then_body, else_body, .. } => walk(then_body, kinds).or_else(|| walk(else_body, kinds)),
                    Statement::For { body, .. } | Statement::While { body, .. } => walk(body, kinds),
                    _ => None,
                };
                if hit.is_some() { return hit; }
            }
            None
        }
        let mut kinds: std::collections::HashMap<String, char> = params.iter().filter_map(|p| match &p.ty.node {
            TypeExpr::Simple(t) if t == "String" => Some((p.name.clone(), 's')),
            TypeExpr::Simple(t) if t == "Int" || t == "Float" => Some((p.name.clone(), 'n')),
            TypeExpr::Simple(t) if t == "Bool" => Some((p.name.clone(), 'b')),
            _ => None,
        }).collect();
        problem = walk(body, &mut kinds);
        // one return type per native handler: `return a / b` (a Float) on
        // one path and `return "neg"` on another passed check, then rustc
        // refused the generated code (E0308)
        if problem.is_none() {
            let mut kinds2 = kinds.clone();
            fn all_lets(stmts: &[Spanned<Statement>], k: &mut std::collections::HashMap<String, char>) {
                for st in stmts {
                    match &st.node {
                        Statement::Let { name, value } => { if let Some(c) = kind(&value.node, k) { k.insert(name.clone(), c); } }
                        Statement::If { then_body, else_body, .. } => { all_lets(then_body, k); all_lets(else_body, k); }
                        Statement::For { body, .. } | Statement::While { body, .. } => all_lets(body, k),
                        _ => {}
                    }
                }
            }
            all_lets(body, &mut kinds2);
            let mut rets: Vec<&Expr> = Vec::new();
            collect_returns(body, &mut rets);
            let seen: HashSet<char> = rets.iter().filter_map(|e| kind(e, &kinds2)).collect();
            if seen.contains(&'s') && seen.len() > 1 {
                problem = Some("returns a String on one path and a number or Bool on another — a native handler has one return type (return to_string(x), or split the handler)".to_string());
            }
        }
    }
    // a condition is a Bool, as interpreted: `if k { … }` on an Int ran in
    // native code and raised in the interpreter
    if problem.is_none() {
        let nums: HashSet<String> = params.iter().filter(|p| matches!(&p.ty.node, TypeExpr::Simple(t) if t == "Int" || t == "Float")).map(|p| p.name.clone()).collect();
        let numeric = |e: &Expr| matches!(e, Expr::Literal(Literal::Int(_) | Literal::Float(_)))
            || matches!(e, Expr::Ident(n) if nums.contains(n))
            || matches!(e, Expr::BinaryOp { op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod, .. });
        fn conds<'a>(stmts: &'a [Spanned<Statement>], out: &mut Vec<&'a Expr>) {
            for st in stmts {
                match &st.node {
                    Statement::If { condition, then_body, else_body } => { out.push(&condition.node); conds(then_body, out); conds(else_body, out); }
                    Statement::While { condition, body, .. } => { out.push(&condition.node); conds(body, out); }
                    Statement::For { body, .. } => conds(body, out),
                    _ => {}
                }
            }
        }
        let mut cs: Vec<&Expr> = Vec::new();
        conds(body, &mut cs);
        if cs.iter().any(|c| numeric(c)) {
            problem = Some("uses a number as a condition — a condition is a Bool, as in interpreted code: compare it (`if k != 0`)".to_string());
        }
    }
    if let Some(reason) = problem {
        return Err(NativeCheckError { handler_name: handler_name.to_string(), reason });
    }
    // a buffer handed to a sibling [native] handler: siblings take Int,
    // Float, Bool or String only (rustc: "non-primitive cast: Vec<i64> as i64")
    let mut hit: Option<(String, String)> = None;
    crate::checker::literals::for_each_call(body, &mut |name, args, _| {
        if hit.is_none() && siblings.contains(name) {
            for a in args {
                if let Expr::Ident(n) = &a.node {
                    if bufs.contains(n) { hit = Some((name.to_string(), n.clone())); break; }
                }
            }
        }
    });
    if let Some((callee, buf)) = hit {
        return Err(NativeCheckError { handler_name: handler_name.to_string(), reason: format!(
            "passes the buffer `{}` to the sibling handler `{}` — only Int, Float, Bool or String cross between [native] handlers; do the work in one handler, or pass the values one by one", buf, callee) });
    }
    Ok(())
}

fn is_buffer_ctor(e: &Expr) -> bool {
    matches!(e, Expr::FnCall { name, .. } if matches!(name.as_str(), "buffer" | "buffer_f" | "hashmap" | "strbuf"))
}

fn has_float_literal(e: &Expr) -> bool {
    match e {
        Expr::Literal(Literal::Float(_)) => true,
        Expr::BinaryOp { left, right, .. } => has_float_literal(&left.node) || has_float_literal(&right.node),
        Expr::FnCall { name, args } => matches!(name.as_str(), "sqrt" | "to_float" | "sin" | "cos" | "exp" | "log") || args.iter().any(|a| has_float_literal(&a.node)),
        _ => false,
    }
}

fn check_codegen_limits(handler_name: &str, body: &[Spanned<Statement>], bufs: &mut HashSet<String>) -> Result<(), NativeCheckError> {
    let err = |reason: String| Err(NativeCheckError { handler_name: handler_name.to_string(), reason });
    // `let x = 0 … x = 1.5`: one native variable has one type (rustc
    // "mismatched types" used to be the message)
    let int_lets: HashSet<String> = body.iter().filter_map(|st| match &st.node {
        Statement::Let { name, value } if matches!(value.node, Expr::Literal(Literal::Int(_))) => Some(name.clone()),
        _ => None,
    }).collect();
    fn assigns_float(stmts: &[Spanned<Statement>], ints: &HashSet<String>) -> Option<String> {
        stmts.iter().find_map(|st| match &st.node {
            Statement::Assign { name, value } if ints.contains(name) && has_float_literal(&value.node) => Some(name.clone()),
            Statement::If { then_body, else_body, .. } => assigns_float(then_body, ints).or_else(|| assigns_float(else_body, ints)),
            Statement::For { body, .. } | Statement::While { body, .. } => assigns_float(body, ints),
            _ => None,
        })
    }
    if let Some(n) = assigns_float(body, &int_lets) {
        return err(format!("`{n}` starts as an Int and is later given a Float — a native variable has one type: start it as a Float (`let {n} = 0.0`)"));
    }
    // one type per variable and per return: Bool / String / number do not
    // mix (each passed check and failed in rustc with "mismatched types")
    fn lit_kind(e: &Expr) -> Option<&'static str> {
        match e {
            Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::Float(_)) => Some("number"),
            Expr::Literal(Literal::String(_)) => Some("String"),
            Expr::Literal(Literal::Bool(_)) => Some("Bool"),
            _ => None,
        }
    }
    let mut kinds: std::collections::HashMap<String, &'static str> = std::collections::HashMap::new();
    let mut ret_kinds: Vec<&'static str> = Vec::new();
    fn walk<'b>(stmts: &'b [Spanned<Statement>], kinds: &mut std::collections::HashMap<String, &'static str>, rets: &mut Vec<&'static str>) -> Option<String> {
        for st in stmts {
            match &st.node {
                Statement::Let { name, value } => { if let Some(k) = lit_kind(&value.node) { kinds.insert(name.clone(), k); } }
                Statement::Assign { name, value } => {
                    if let (Some(prev), Some(k)) = (kinds.get(name).copied(), lit_kind(&value.node)) {
                        if prev != k { return Some(format!("`{}` holds a {} and is later given a {} — a native variable has one type", name, prev, k)); }
                    }
                }
                Statement::Return { value } => { if let Some(k) = lit_kind(&value.node) { rets.push(k); } }
                Statement::If { then_body, else_body, .. } => {
                    if let Some(m) = walk(then_body, kinds, rets) { return Some(m); }
                    if let Some(m) = walk(else_body, kinds, rets) { return Some(m); }
                }
                Statement::For { body, .. } | Statement::While { body, .. } => { if let Some(m) = walk(body, kinds, rets) { return Some(m); } }
                _ => {}
            }
        }
        None
    }
    if let Some(m) = walk(body, &mut kinds, &mut ret_kinds) { return err(m); }
    ret_kinds.sort(); ret_kinds.dedup();
    if ret_kinds.len() > 1 {
        return err(format!("returns a {} on one path and a {} on another — a native handler returns one type", ret_kinds[0], ret_kinds[1]));
    }
    for st in body {
        match &st.node {
            Statement::Let { name, value } => {
                if is_buffer_ctor(&value.node) { bufs.insert(name.clone()); }
                else if let Expr::Ident(src) = &value.node {
                    if bufs.contains(src) {
                        return err(format!("`let {} = {}` re-binds a buffer — a Buf/HMap/SBuf lives in one variable; copy it element by element (`for k in range(0, n) {{ buf_set(v, k, buf_get(u, k)) }}`)", name, src));
                    }
                }
                check_codegen_limits_expr(handler_name, &value.node)?;
            }
            Statement::Assign { name, value } => {
                if bufs.contains(name) {
                    return err(format!("`{} = …` re-assigns a buffer — a Buf/HMap/SBuf is bound once; copy elements instead of swapping variables", name));
                }
                check_codegen_limits_expr(handler_name, &value.node)?;
            }
            Statement::Return { value } => {
                match &value.node {
                    Expr::Ident(n) if bufs.contains(n) => return err(format!("returns the buffer `{}` — only Int, Float, Bool or String cross the [native] boundary; pack the values into a String (strbuf) and split it in an interpreted handler", n)),
                    Expr::ListLiteral(_) => return err("returns a list — only Int, Float, Bool or String cross the [native] boundary".to_string()),
                    _ => {}
                }
                check_codegen_limits_expr(handler_name, &value.node)?;
            }
            Statement::For { iter, body, .. } => {
                if let Expr::FnCall { name, args } = &iter.node {
                    if name == "range" && args.len() != 2 {
                        return err("a native `for` takes `range(a, b)` only — write a `while` loop for a step".to_string());
                    }
                }
                check_codegen_limits(handler_name, body, bufs)?;
            }
            Statement::While { condition, body, .. } => {
                check_codegen_limits_expr(handler_name, &condition.node)?;
                check_codegen_limits(handler_name, body, bufs)?;
            }
            Statement::If { condition, then_body, else_body } => {
                check_codegen_limits_expr(handler_name, &condition.node)?;
                check_codegen_limits(handler_name, then_body, bufs)?;
                check_codegen_limits(handler_name, else_body, bufs)?;
            }
            Statement::ExprStmt { expr } | Statement::Ensure { condition: expr } => check_codegen_limits_expr(handler_name, &expr.node)?,
            _ => {}
        }
    }
    Ok(())
}

fn check_codegen_limits_expr(handler_name: &str, e: &Expr) -> Result<(), NativeCheckError> {
    let err = |reason: String| Err(NativeCheckError { handler_name: handler_name.to_string(), reason });
    match e {
        Expr::BinaryOp { left, op, right } => {
            if matches!(op, BinOp::Div | BinOp::Mod) && matches!(&right.node, Expr::Literal(Literal::Int(0))) {
                return err("divides by the literal 0 — rustc refuses to compile it (an unconditional panic); pass the divisor as a value if you want the runtime error".to_string());
            }
            check_codegen_limits_expr(handler_name, &left.node)?;
            check_codegen_limits_expr(handler_name, &right.node)
        }
        Expr::FnCall { name, args } => {
            if name == "idiv" && matches!(args.get(1).map(|a| &a.node), Some(Expr::Literal(Literal::Int(0)))) {
                return err("idiv by the literal 0 — rustc refuses to compile it; pass the divisor as a value".to_string());
            }
            for a in args { check_codegen_limits_expr(handler_name, &a.node)?; }
            Ok(())
        }
        Expr::CmpOp { left, right, .. } => { check_codegen_limits_expr(handler_name, &left.node)?; check_codegen_limits_expr(handler_name, &right.node) }
        Expr::Not(i) => check_codegen_limits_expr(handler_name, &i.node),
        _ => Ok(()),
    }
}

fn check_stmt(handler_name: &str, stmt: &Statement, siblings: &NativeSiblings) -> Result<(), NativeCheckError> {
    match stmt {
        Statement::Let { value, .. } => check_expr(handler_name, &value.node, siblings),
        Statement::Assign { value, .. } => check_expr(handler_name, &value.node, siblings),
        Statement::Return { value } => check_expr(handler_name, &value.node, siblings),
        Statement::IndexSet { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "index assignment is not allowed in native handlers".to_string(),
        }),
        Statement::If { condition, then_body, else_body } => {
            check_expr(handler_name, &condition.node, siblings)?;
            for s in then_body { check_stmt(handler_name, &s.node, siblings)?; }
            for s in else_body { check_stmt(handler_name, &s.node, siblings)?; }
            Ok(())
        }
        Statement::While { condition, body, .. } => {
            check_expr(handler_name, &condition.node, siblings)?;
            for s in body { check_stmt(handler_name, &s.node, siblings)?; }
            Ok(())
        }
        Statement::For { iter, body, .. } => {
            check_expr(handler_name, &iter.node, siblings)?;
            for s in body { check_stmt(handler_name, &s.node, siblings)?; }
            Ok(())
        }
        Statement::Break | Statement::Continue => Ok(()),
        Statement::ExprStmt { expr } => check_expr(handler_name, &expr.node, siblings),
        Statement::Emit { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses 'emit' (signals not allowed in native handlers)".to_string(),
        }),
        Statement::Require { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses 'require' (not allowed in native handlers)".to_string(),
        }),
        Statement::MethodCall { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses method call (not allowed in native handlers)".to_string(),
        }),
        Statement::Ensure { condition } => check_expr(handler_name, &condition.node, siblings),
    }
}

fn check_expr(handler_name: &str, expr: &Expr, siblings: &NativeSiblings) -> Result<(), NativeCheckError> {
    match expr {
        Expr::Literal(lit) => {
            match lit {
                Literal::Int(_) | Literal::Float(_) | Literal::Bool(_) => Ok(()),
                Literal::String(text) => {
                    // `"v {n}"` compiled to the literal text — no
                    // interpolation natively; say so instead of returning it
                    let has_hole = text.replace("{{", "").replace("}}", "").contains('{');
                    if has_hole {
                        return Err(NativeCheckError {
                            handler_name: handler_name.to_string(),
                            reason: format!("uses string interpolation ({}) — not available natively: build the text with to_string() and + , or in an interpreted handler", text.chars().take(30).collect::<String>()),
                        });
                    }
                    Ok(())
                }
                Literal::BigInt(b) => Err(NativeCheckError {
                    handler_name: handler_name.to_string(),
                    reason: format!("uses the Int literal {} beyond 64 bits — build it at run time (`shl(1, 64) - 1`, `shl(0x9E3779B9, 32) + 0x7F4A7C15`), or keep this code interpreted", b),
                }),
                _ => Err(NativeCheckError {
                    handler_name: handler_name.to_string(),
                    reason: format!("uses unsupported literal type {:?}", lit),
                }),
            }
        }
        Expr::Ident(_) => Ok(()),
        Expr::BinaryOp { left, right, .. } => {
            check_expr(handler_name, &left.node, siblings)?;
            check_expr(handler_name, &right.node, siblings)
        }
        Expr::CmpOp { left, right, .. } => {
            check_expr(handler_name, &left.node, siblings)?;
            check_expr(handler_name, &right.node, siblings)
        }
        Expr::Not(inner) => check_expr(handler_name, &inner.node, siblings),
        Expr::FnCall { name, args } => {
            // `map(k, v)` / `filter(xs, f)` are pipeline ops natively only in
            // `xs |> map(x => …)` form: a map LITERAL or a list call cannot
            // exist in native code (rustc used to be the one to say so)
            let is_pipeline_op = matches!(name.as_str(), "map" | "filter" | "reduce" | "fold");
            let has_lambda = args.iter().any(|a| matches!(a.node, Expr::Lambda { .. } | Expr::LambdaBlock { .. }));
            if is_pipeline_op && !has_lambda {
                return Err(NativeCheckError {
                    handler_name: handler_name.to_string(),
                    reason: format!("builds a {} with `{}(…)` — no maps or lists natively: return one scalar per handler (or a String), and build the record in an interpreted handler", if name == "map" { "map" } else { "list" }, name),
                });
            }
            // Allow known math builtins and calls to other native handlers
            if !ALLOWED_BUILTINS.contains(&name.as_str())
                && !siblings.contains(name)
                && name != handler_name
            {
                return Err(NativeCheckError {
                    handler_name: handler_name.to_string(),
                    reason: format!("calls '{}', which is not a [native] handler of this cell (a [native] handler calls only builtins of the native vocabulary and [native] handlers of its OWN cell — move '{}' here or drop [native])", name, name),
                });
            }
            for arg in args { check_expr(handler_name, &arg.node, siblings)?; }
            Ok(())
        }
        Expr::Pipe { left, right } => {
            // Pipes are allowed in native — they generate parallel code
            check_expr(handler_name, &left.node, siblings)?;
            check_expr(handler_name, &right.node, siblings)
        }
        Expr::Lambda { param: _, body } => {
            // Lambdas allowed for pipe operations (map, filter, reduce)
            check_expr(handler_name, &body.node, siblings)
        }
        Expr::LambdaBlock { param: _, stmts, result } => {
            for stmt in stmts { check_stmt(handler_name, &stmt.node, siblings)?; }
            check_expr(handler_name, &result.node, siblings)
        }
        Expr::FieldAccess { target, .. } => {
            // Field access allowed for lambda params (p.acc, p.val, s.price)
            check_expr(handler_name, &target.node, siblings)
        }
        Expr::MethodCall { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses method call (not allowed in native handlers)".to_string(),
        }),
        Expr::Index { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "bracket indexing is not allowed in native handlers (use buf_get)".to_string(),
        }),
        Expr::Record { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses record literal (not allowed in native handlers)".to_string(),
        }),
        Expr::Try(_) => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses try expression (not allowed in native handlers)".to_string(),
        }),
        Expr::Match { .. } => Err(NativeCheckError {
            handler_name: handler_name.to_string(),
            reason: "uses match expression (not allowed in native handlers)".to_string(),
        }),
        Expr::ListLiteral(elements) => {
            for elem in elements { check_expr(handler_name, &elem.node, siblings)?; }
            Ok(())
        }
        Expr::TryPropagate(inner) => check_expr(handler_name, &inner.node, siblings),
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            check_expr(handler_name, &condition.node, siblings)?;
            for stmt in then_body { check_stmt(handler_name, &stmt.node, siblings)?; }
            check_expr(handler_name, &then_result.node, siblings)?;
            for stmt in else_body { check_stmt(handler_name, &stmt.node, siblings)?; }
            check_expr(handler_name, &else_result.node, siblings)?;
            Ok(())
        }
    }
}

// ── Backend-dependent semantics ─────────────────────────────────────

/// `a / b` on two Ints is 3.5 for 7 / 2 on every backend. Native code is
/// statically typed, though: where the quotient lands in a slot that can
/// only hold an Int (an Int variable, an index, an Int argument) a
/// non-exact division is a RUNTIME ERROR there, while the interpreter
/// would retype the variable to Float. Code that means "integer quotient"
/// should say idiv(a, b). Returns one warning per handler dividing two
/// Int-typed operands.
pub fn int_division_warnings(program: &Program) -> Vec<(String, Span)> {
    load_int_handlers(program);
    let mut out = Vec::new();
    for cell in super::names::collect_cells(program) {
        for section in &cell.sections {
            let Section::OnSignal(on) = &section.node else { continue };
            if !on.properties.iter().any(|p| p == "native") {
                continue;
            }
            let mut env: std::collections::HashMap<String, Num> = std::collections::HashMap::new();
            for p in &on.params {
                env.insert(p.name.clone(), match &p.ty.node {
                    TypeExpr::Simple(t) if t == "Int" => Num::Int,
                    TypeExpr::Simple(t) if t == "Float" => Num::Float,
                    _ => Num::Unknown,
                });
            }
            let mut sites: Vec<(Span, Span)> = Vec::new();
            scan_stmts(&on.body, &mut env, &mut sites);
            let hits = sites.len();
            if hits > 0 {
                out.push((
                    format!(
                        "in {}.{} [native]: `/` on two Ints is a Float (7 / 2 = 3.5); where the result must \
                         be an Int a non-exact quotient is a runtime error — {} occurrence{}. Write \
                         idiv(a, b) for an integer quotient (`soma fix --native-idiv` rewrites them)",
                        cell.name,
                        on.signal_name,
                        hits,
                        if hits == 1 { "" } else { "s" }
                    ),
                    section.span,
                ));
            }
        }
    }
    out
}

thread_local! {
    /// Handlers whose face signal declares `-> Int`, for the current scan.
    static INT_HANDLERS: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Record every `signal f(..) -> Int` of the program for scan_expr.
fn load_int_handlers(program: &Program) {
    let mut set = std::collections::HashSet::new();
    for cell in super::names::collect_cells(program) {
        for section in &cell.sections {
            if let Section::Face(face) = &section.node {
                for decl in &face.declarations {
                    if let FaceDecl::Signal(sig) = &decl.node {
                        if matches!(sig.return_type.as_ref().map(|t| &t.node), Some(TypeExpr::Simple(t)) if t == "Int") {
                            set.insert(sig.name.clone());
                        }
                    }
                }
            }
        }
    }
    INT_HANDLERS.with(|h| *h.borrow_mut() = set);

    // No face declaration (scripts, benchmarks): a [native] handler whose
    // returns all scan as Int is Int. Self-calls are assumed Int while
    // scanning; two rounds settle sibling chains.
    for _ in 0..2 {
        for cell in super::names::collect_cells(program) {
            for section in &cell.sections {
                let Section::OnSignal(on) = &section.node else { continue };
                if !on.properties.iter().any(|p| p == "native")
                    || INT_HANDLERS.with(|h| h.borrow().contains(&on.signal_name))
                {
                    continue;
                }
                INT_HANDLERS.with(|h| h.borrow_mut().insert(on.signal_name.clone()));
                let mut env: std::collections::HashMap<String, Num> = std::collections::HashMap::new();
                for p in &on.params {
                    env.insert(p.name.clone(), match &p.ty.node {
                        TypeExpr::Simple(t) if t == "Int" => Num::Int,
                        TypeExpr::Simple(t) if t == "Float" => Num::Float,
                        _ => Num::Unknown,
                    });
                }
                let mut sink = Vec::new();
                scan_stmts(&on.body, &mut env, &mut sink);
                let mut returns = Vec::new();
                collect_returns(&on.body, &mut returns);
                let all_int = !returns.is_empty()
                    && returns.iter().all(|e| {
                        // a bare Int / Int return is what we are trying to
                        // classify — it counts as Int-valued (pre-idiv code)
                        let t = scan_expr(e, &env, &mut sink);
                        t == Num::Int || matches!(e, Expr::BinaryOp { op: BinOp::Div, left, right }
                            if scan_expr(&left.node, &env, &mut sink) == Num::Int
                            && scan_expr(&right.node, &env, &mut sink) == Num::Int)
                    });
                if !all_int {
                    INT_HANDLERS.with(|h| h.borrow_mut().remove(&on.signal_name));
                }
            }
        }
    }
}

fn collect_returns<'e>(stmts: &'e [Spanned<Statement>], out: &mut Vec<&'e Expr>) {
    for stmt in stmts {
        match &stmt.node {
            Statement::Return { value } => out.push(&value.node),
            Statement::If { then_body, else_body, .. } => {
                collect_returns(then_body, out);
                collect_returns(else_body, out);
            }
            Statement::While { body, .. } | Statement::For { body, .. } => collect_returns(body, out),
            _ => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Num {
    Int,
    Float,
    Unknown,
}

fn scan_stmts(stmts: &[Spanned<Statement>], env: &mut std::collections::HashMap<String, Num>, hits: &mut Vec<(Span, Span)>) {
    for stmt in stmts {
        match &stmt.node {
            Statement::Let { name, value } | Statement::Assign { name, value } => {
                let t = scan_expr(&value.node, env, hits);
                // a variable that is ever Float stays Float (loops reassign)
                let merged = match (env.get(name), t) {
                    (Some(Num::Float), _) | (_, Num::Float) => Num::Float,
                    (Some(Num::Unknown), _) | (_, Num::Unknown) => Num::Unknown,
                    _ => Num::Int,
                };
                env.insert(name.clone(), merged);
            }
            Statement::Return { value } | Statement::Ensure { condition: value } => {
                scan_expr(&value.node, env, hits);
            }
            Statement::ExprStmt { expr } => {
                scan_expr(&expr.node, env, hits);
            }
            Statement::If { condition, then_body, else_body } => {
                scan_expr(&condition.node, env, hits);
                scan_stmts(then_body, env, hits);
                scan_stmts(else_body, env, hits);
            }
            Statement::While { condition, body, .. } => {
                scan_expr(&condition.node, env, hits);
                scan_stmts(body, env, hits);
            }
            Statement::For { var, iter, body, .. } => {
                scan_expr(&iter.node, env, hits);
                env.insert(var.clone(), Num::Unknown);
                scan_stmts(body, env, hits);
            }
            Statement::IndexSet { index, value, .. } => {
                scan_expr(&index.node, env, hits);
                scan_expr(&value.node, env, hits);
            }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => {
                for a in args {
                    scan_expr(&a.node, env, hits);
                }
            }
            Statement::Require { .. } | Statement::Break | Statement::Continue => {}
        }
    }
}

fn scan_expr(expr: &Expr, env: &std::collections::HashMap<String, Num>, hits: &mut Vec<(Span, Span)>) -> Num {
    match expr {
        Expr::Literal(Literal::Int(_)) => Num::Int,
        Expr::Literal(Literal::Float(_)) => Num::Float,
        Expr::Ident(n) => env.get(n).copied().unwrap_or(Num::Unknown),
        Expr::BinaryOp { left, op, right } => {
            let l = scan_expr(&left.node, env, hits);
            let r = scan_expr(&right.node, env, hits);
            if matches!(op, BinOp::And | BinOp::Or) {
                return Num::Unknown;
            }
            if matches!(op, BinOp::Div) && l == Num::Int && r == Num::Int {
                // literal / literal that divides exactly agrees everywhere
                let exact = matches!(
                    (&left.node, &right.node),
                    (Expr::Literal(Literal::Int(a)), Expr::Literal(Literal::Int(b))) if *b != 0 && a % b == 0
                );
                if !exact {
                    hits.push((left.span, right.span));
                }
            }
            match (l, r) {
                (Num::Float, _) | (_, Num::Float) => Num::Float,
                (Num::Int, Num::Int) => Num::Int,
                _ => Num::Unknown,
            }
        }
        Expr::FnCall { name, args } => {
            let tys: Vec<Num> = args.iter().map(|a| scan_expr(&a.node, env, hits)).collect();
            match name.as_str() {
                "to_float" | "sqrt" | "log" | "exp" | "pow" | "sin" | "cos" | "random" => Num::Float,
                "to_int" | "idiv" | "gcd" | "len" | "floor" | "ceil" | "round" | "pow_mod"
                | "sqrt_int" | "band" | "bor" | "bxor" | "bnot" | "shl" | "shr" | "bit_len" => Num::Int,
                "abs" | "min" | "max" => {
                    if tys.iter().any(|t| *t == Num::Float) {
                        Num::Float
                    } else if !tys.is_empty() && tys.iter().all(|t| *t == Num::Int) {
                        Num::Int
                    } else {
                        Num::Unknown
                    }
                }
                other if INT_HANDLERS.with(|h| h.borrow().contains(other)) => Num::Int,
                _ => Num::Unknown,
            }
        }
        Expr::CmpOp { left, right, .. } => {
            scan_expr(&left.node, env, hits);
            scan_expr(&right.node, env, hits);
            Num::Unknown
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => {
            scan_expr(&i.node, env, hits);
            Num::Unknown
        }
        Expr::IfExpr { condition, then_result, else_result, .. } => {
            scan_expr(&condition.node, env, hits);
            let a = scan_expr(&then_result.node, env, hits);
            let b = scan_expr(&else_result.node, env, hits);
            if a == b { a } else { Num::Unknown }
        }
        Expr::MethodCall { target, args, .. } => {
            scan_expr(&target.node, env, hits);
            for a in args {
                scan_expr(&a.node, env, hits);
            }
            Num::Unknown
        }
        Expr::Index { target, index } => {
            scan_expr(&target.node, env, hits);
            scan_expr(&index.node, env, hits);
            Num::Unknown
        }
        _ => Num::Unknown,
    }
}

/// Operand spans (left, right) of every Int / Int division inside [native]
/// handlers — the rewrite sites of `soma fix --native-idiv`.
pub fn int_division_sites(program: &Program) -> Vec<(Span, Span)> {
    load_int_handlers(program);
    let mut out = Vec::new();
    for cell in super::names::collect_cells(program) {
        for section in &cell.sections {
            let Section::OnSignal(on) = &section.node else { continue };
            if !on.properties.iter().any(|p| p == "native") {
                continue;
            }
            let mut env: std::collections::HashMap<String, Num> = std::collections::HashMap::new();
            for p in &on.params {
                env.insert(p.name.clone(), match &p.ty.node {
                    TypeExpr::Simple(t) if t == "Int" => Num::Int,
                    TypeExpr::Simple(t) if t == "Float" => Num::Float,
                    _ => Num::Unknown,
                });
            }
            scan_stmts(&on.body, &mut env, &mut out);
        }
    }
    out
}
