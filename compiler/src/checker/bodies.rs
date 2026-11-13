//! The executable bodies of a cell — ONE answer to "what runs in this
//! program?". A handler, an `every` / `after` block and the statements of
//! `runtime { }` all run; each analysis used to walk the sections itself
//! and keep its own list (handlers only for the budget, the effects and
//! the capabilities; handlers and timers for the proofs; nothing read
//! `runtime { }`), so what one analysis judged another had never seen.
//!
//! `entries` gives them all as handlers: the real ones, and one synthetic
//! `OnSection` per timer (`every 1000ms`, `after 500ms`) and for the
//! `runtime { }` section (`runtime { }`). An analysis that reasons per
//! handler reads the synthetic ones like the others.

use crate::ast::*;

/// The name given to the `runtime { }` section when it is read as a handler.
pub const RUNTIME_ENTRY: &str = "runtime { }";

/// Every body of `cell` that runs, as a handler. Timers and the runtime
/// section have no parameters; a `[task]` timer keeps its property.
pub fn entries(cell: &CellDef) -> Vec<OnSection> {
    let mut out: Vec<OnSection> = Vec::new();
    let mut runtime: Vec<Spanned<Statement>> = Vec::new();
    let mut exposed = false;
    for s in &cell.sections {
        match &s.node {
            Section::OnSignal(on) => {
                if on.signal_name == RUNTIME_ENTRY { exposed = true; }
                out.push(on.clone());
            }
            Section::Every(e) => out.push(OnSection {
                signal_name: format!("every {}ms", e.interval_ms), params: Vec::new(), body: e.body.clone(),
                properties: if e.task { vec!["task".to_string()] } else { Vec::new() },
            }),
            Section::After(e) => out.push(OnSection {
                signal_name: format!("after {}ms", e.interval_ms), params: Vec::new(), body: e.body.clone(),
                properties: if e.task { vec!["task".to_string()] } else { Vec::new() },
            }),
            Section::Runtime(rt) => runtime.extend(super::runtime_section::as_statements(rt)),
            _ => {}
        }
    }
    // (a program already exposed for analysis carries the section as a handler)
    if !runtime.is_empty() && !exposed {
        out.push(OnSection { signal_name: RUNTIME_ENTRY.to_string(), params: Vec::new(), body: runtime, properties: Vec::new() });
    }
    out
}
