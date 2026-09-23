//! V1.6: `[deterministic]` handler contract.
//!
//! A handler annotated `[deterministic]` must produce the same output for
//! the same input. This pass walks every handler body and flags calls
//! that would make the handler depend on state outside its arguments.
//!
//! Required for: golden tests, replay-as-spec, A/B comparisons, and the
//! property-based test machinery (#8) that quantifies over arbitrary inputs.

use crate::ast::*;

#[derive(Debug)]
pub struct DeterminismError {
    pub handler_name: String,
    pub bad_call: String,
    pub reason: String,
}

impl std::fmt::Display for DeterminismError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "handler '{}' is [deterministic] but uses '{}' — {}",
            self.handler_name, self.bad_call, self.reason
        )
    }
}

/// Builtins forbidden in `[deterministic]` handlers, grouped by why.
fn forbid_reason(name: &str) -> Option<&'static str> {
    match name {
        "now" | "now_ms" | "timestamp" | "today" | "date_now" =>
            Some("reads wall-clock time"),
        "random" | "rand" | "uuid" | "next_id" | "random_token"
        // they draw from a PRNG too
        | "sample_row" | "importance_sample_rows" | "svd_lowrank" | "regress_sgd" =>
            Some("reads a non-deterministic source"),
        "think" | "think_json" =>
            Some("calls the LLM (output is not a function of the prompt)"),
        "http_get" | "http_post" | "http_put" | "http_delete" | "http_patch" |
        "fetch" | "fetch_json" =>
            Some("performs network I/O"),
        "read_file" | "write_file" | "append_file" | "delete_file" =>
            Some("performs file I/O"),
        "delegate" =>
            Some("invokes another cell (state outside this handler)"),
        "transition" | "get_status" | "has_state" | "valid_transitions" =>
            Some("reads or moves a state machine (state outside its arguments)"),
        // printing and sleeping change no value the handler returns
        "print" | "sleep" => None,
        _ if super::names::unstable_builtin(name) || super::names::EFFECT_BUILTINS.contains(&name) =>
            Some("depends on state outside its arguments (I/O, storage, the model or another run)"),
        _ => None,
    }
}

/// The slot methods that write (`put` is `set`, `remove` is `delete`).
fn mutating_method(name: &str) -> bool {
    matches!(name, "set" | "put" | "delete" | "remove" | "append" | "push" | "clear")
}

/// Every call, storage read and storage write of the body, blocks
/// included, and — transitively — of every handler it calls (in this cell,
/// or `Other.h(…)`): "same inputs, same output" is refused by a slot read
/// (`counts.get("n")`), a state-machine read or move, file / network I/O,
/// and a call of a handler that does any of these.
pub fn check_deterministic_handler(
    handler: &OnSection,
    cell: &CellDef,
    cells: &std::collections::HashMap<String, CellDef>,
) -> Vec<DeterminismError> {
    // handler → its first violation, once scanned (None while in progress:
    // a recursive call adds nothing the body does not already do)
    let mut visited: std::collections::HashMap<String, Option<(String, String)>> = std::collections::HashMap::new();
    visited.insert(format!("{}.{}", cell.name, handler.signal_name), None);
    scan(handler, cell, cells, &mut visited, 0).into_iter()
        .map(|(bad_call, reason)| DeterminismError { handler_name: handler.signal_name.clone(), bad_call, reason })
        .collect()
}

fn slots_of(cell: &CellDef) -> std::collections::HashSet<String> {
    cell.sections.iter()
        .filter_map(|s| if let Section::Memory(m) = &s.node { Some(m) } else { None })
        .flat_map(|m| m.slots.iter().map(|sl| sl.node.name.clone()))
        .collect()
}

fn find_handler<'c>(cell: &'c CellDef, name: &str) -> Option<&'c OnSection> {
    cell.sections.iter().find_map(|s| match &s.node { Section::OnSignal(o) if o.signal_name == name => Some(o), _ => None })
}

fn scan(
    handler: &OnSection,
    cell: &CellDef,
    cells: &std::collections::HashMap<String, CellDef>,
    visited: &mut std::collections::HashMap<String, Option<(String, String)>>,
    depth: usize,
) -> Vec<(String, String)> {
    let body = &handler.body;
    // the names the body BINDS (lets, loop / lambda / match binders) — not
    // the ones it assigns (`bal[k] = 2` writes the slot)
    let mut locals: std::collections::HashSet<String> = handler.params.iter().map(|p| p.name.clone()).collect();
    super::invariants::collect_binders_stmts(body, &mut locals);
    crate::checker::literals::for_each_stmt_deep(body, &mut |st| match st {
        Statement::Let { name, .. } => { locals.insert(name.clone()); }
        Statement::For { var, .. } => { locals.insert(var.clone()); }
        _ => {}
    });
    // a parameter or local named like a slot hides it
    let slots: std::collections::HashSet<String> = slots_of(cell).into_iter()
        .filter(|n| !handler.params.iter().any(|p| &p.name == n)).collect();
    let mut errs: Vec<(String, String)> = Vec::new();
    let mut push = |bad_call: String, reason: String| {
        if !errs.iter().any(|e| e.0 == bad_call) { errs.push((bad_call, reason)); }
    };
    let mut callees: Vec<(String, String)> = Vec::new();
    crate::checker::literals::for_each_expr(body, &mut |e| match e {
        Expr::FnCall { name, .. } => {
            // before the locals: a DATA local named like the builtin
            // (`let random = 0  random()`) leaves the builtin to run, and
            // a function local of that name is refused by `soma check`
            if let Some(reason) = forbid_reason(name) { push(name.clone(), reason.to_string()); return; }
            if locals.contains(name) { return; }
            // a handler of this cell (it wins over a builtin), or a bare
            // call of another cell's handler
            if find_handler(cell, name).is_some() {
                callees.push((cell.name.clone(), name.clone()));
            } else if !super::names::builtin_names().contains(name.as_str()) {
                if let Some(c) = cells.values().find(|c| find_handler(c, name).is_some()) {
                    callees.push((c.name.clone(), name.clone()));
                }
            }
        }
        Expr::MethodCall { target, method, .. } => {
            if let Expr::Ident(t) = &target.node {
                if !locals.contains(t) && cells.contains_key(t) {
                    callees.push((t.clone(), method.clone()));
                    return;
                }
            }
            if mutating_method(method) { push(format!(".{}()", method), "mutates persistent storage".to_string()); }
            // `x.random()` / `ts.now()`: the builtin, called in method form
            else if let Some(reason) = forbid_reason(method) { push(method.clone(), reason.to_string()); }
        }
        Expr::Ident(n) if slots.contains(n) && !locals.contains(n) =>
            push(format!("the memory slot {}", n), "reads persistent storage (state outside its arguments)".to_string()),
        _ => {}
    });
    crate::checker::literals::for_each_stmt_deep(body, &mut |st| match st {
        Statement::Emit { .. } => push("emit".to_string(), "raises a signal (cross-cell effect)".to_string()),
        Statement::MethodCall { method, .. } if mutating_method(method) =>
            push(format!(".{}()", method), "mutates persistent storage".to_string()),
        Statement::IndexSet { name, .. } | Statement::Assign { name, .. }
            if { let root = name.split(|c| c == '.' || c == '[').next().unwrap_or(""); slots.contains(root) && !locals.contains(root) } =>
            push(format!("{} = …", name), "mutates persistent storage".to_string()),
        _ => {}
    });
    for (c, h) in callees {
        let key = format!("{}.{}", c, h);
        let Some(callee_cell) = cells.get(&c) else { continue };
        let Some(callee) = find_handler(callee_cell, &h) else {
            // `Other.h` names no handler: the proof cannot follow it
            push(format!("{}()", key), "calls something the check cannot follow".to_string());
            continue;
        };
        let first = match visited.get(&key) {
            Some(done) => done.clone(),
            None => {
                visited.insert(key.clone(), None);
                let inner = if depth > 16 { vec![("…".to_string(), "a call chain too deep to follow".to_string())] } else { scan(callee, callee_cell, cells, visited, depth + 1) };
                let first = inner.into_iter().next();
                visited.insert(key.clone(), first.clone());
                first
            }
        };
        if let Some((bad, why)) = first {
            let shown = if c == cell.name { format!("{}()", h) } else { format!("{}()", key) };
            push(shown, format!("calls a handler that is not deterministic (it uses `{}`, which {})", bad, why));
        }
    }
    errs
}
