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
    pub reason: &'static str,
}

impl std::fmt::Display for DeterminismError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "handler '{}' is [deterministic] but calls '{}' — {}",
            self.handler_name, self.bad_call, self.reason
        )
    }
}

/// Builtins forbidden in `[deterministic]` handlers, grouped by why.
fn forbid_reason(name: &str) -> Option<&'static str> {
    match name {
        "now" | "now_ms" | "timestamp" | "today" | "date_now" =>
            Some("reads wall-clock time"),
        "random" | "rand" | "uuid" | "next_id" =>
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
        _ => None,
    }
}

/// The slot methods that write (`put` is `set`, `remove` is `delete`).
fn mutating_method(name: &str) -> bool {
    matches!(name, "set" | "put" | "delete" | "remove" | "append" | "push" | "clear")
}

/// Every call and storage write of the body, blocks included: a `require`
/// condition, `slot.put(k, v)`, `slot.remove(k)` and `slot[k] = v` passed
/// the contract (only `.set/.delete/.push` were refused).
pub fn check_deterministic_handler(
    handler_name: &str,
    body: &[Spanned<Statement>],
    slots: &std::collections::HashSet<String>,
) -> Vec<DeterminismError> {
    let mut errs: Vec<DeterminismError> = Vec::new();
    let mut push = |bad_call: String, reason: &'static str| {
        if !errs.iter().any(|e| e.bad_call == bad_call) {
            errs.push(DeterminismError { handler_name: handler_name.to_string(), bad_call, reason });
        }
    };
    crate::checker::literals::for_each_expr(body, &mut |e| match e {
        Expr::FnCall { name, .. } => {
            if let Some(reason) = forbid_reason(name) { push(name.clone(), reason); }
        }
        Expr::MethodCall { method, .. } if mutating_method(method) =>
            push(format!(".{}()", method), "mutates persistent storage"),
        _ => {}
    });
    crate::checker::literals::for_each_stmt_deep(body, &mut |st| match st {
        Statement::Emit { .. } => push("emit".to_string(), "raises a signal (cross-cell effect)"),
        Statement::MethodCall { method, .. } if mutating_method(method) =>
            push(format!(".{}()", method), "mutates persistent storage"),
        Statement::IndexSet { name, .. } | Statement::Assign { name, .. }
            if slots.contains(name.split('.').next().unwrap_or("")) =>
            push(format!("{} = …", name), "mutates persistent storage"),
        _ => {}
    });
    errs
}
