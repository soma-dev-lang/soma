//! Checks of interior composition that the runtime depends on:
//!
//! - `every` / `after` inside an interior cell: never scheduled. Interior
//!   cells run only through the `runtime` section of `soma run` (a one-shot
//!   pass), and `soma serve` does not instantiate them — the block was
//!   silently dead. It is an error, with the fix (a top-level cell).
//! - the `runtime` section: `start` / `connect` naming no interior cell, and
//!   `signal go(…)` with the wrong number of arguments or a literal of the
//!   wrong type for the handlers' parameters (`signal go("5")` for
//!   `on go(n: Int)` failed only at run time). Undefined names in its
//!   statements and arguments are found by the interpolation walker.

use crate::ast::*;

/// The runtime section as the statements the runtime executes in order
/// (`signal` entries as emits; `start` / `connect` are declarations).
pub fn as_statements(rt: &RuntimeSection) -> Vec<Spanned<Statement>> {
    rt.entries.iter().filter_map(|e| match &e.node {
        RuntimeEntry::Emit { signal_name, args } => Some(Spanned { node: Statement::Emit { signal_name: signal_name.clone(), args: args.clone() }, span: e.span }),
        RuntimeEntry::Stmt(st) => Some(Spanned { node: st.clone(), span: e.span }),
        _ => None,
    }).collect()
}

pub struct Issue {
    pub kind: &'static str,
    pub message: String,
    pub span: Span,
}

fn interior_children(cell: &CellDef) -> Vec<&Spanned<CellDef>> {
    cell.sections.iter().filter_map(|s| match &s.node {
        Section::Interior(i) => Some(i.cells.iter().filter(|c| matches!(c.node.kind, CellKind::Cell | CellKind::Agent)).collect::<Vec<_>>()),
        _ => None,
    }).flatten().collect()
}

fn timers_in_interior(cell: &CellDef, path: &str, out: &mut Vec<Issue>) {
    for child in interior_children(cell) {
        let child_path = format!("{}.{}", path, child.node.name);
        for s in &child.node.sections {
            let what = match &s.node {
                Section::Every(_) => "every",
                Section::After(_) => "after",
                _ => continue,
            };
            out.push(Issue {
                kind: "interior_timer",
                message: format!(
                    "`{what}` block in the interior cell {child_path} never runs: interior cells run only through the runtime section of `soma run` (one pass), and `soma serve` does not schedule them — move the `{what}` block to a top-level cell (timers run in any top-level cell under `soma serve`) and have it call or `signal` what it needs"
                ),
                span: s.span,
            });
        }
        timers_in_interior(&child.node, &child_path, out);
    }
}

/// The literal kind of `e`, when it is a plain literal.
fn literal_kind(e: &Expr) -> Option<&'static str> {
    match e {
        Expr::Literal(Literal::Int(_)) => Some("Int"),
        Expr::Literal(Literal::Float(_)) => Some("Float"),
        Expr::Literal(Literal::String(_)) => Some("String"),
        Expr::Literal(Literal::Bool(_)) => Some("Bool"),
        _ => None,
    }
}

/// Whether a literal of `kind` can be passed where `ty` is declared.
fn accepts(ty: &TypeExpr, kind: &str) -> bool {
    let TypeExpr::Simple(t) = ty else { return true };
    match t.as_str() {
        "Int" => kind == "Int",
        "Float" => matches!(kind, "Int" | "Float"),
        "String" => kind == "String",
        "Bool" => kind == "Bool",
        "List" | "Map" => false,
        _ => true,
    }
}

pub fn check_cell(cell: &CellDef) -> Vec<Issue> {
    let mut out = Vec::new();
    timers_in_interior(cell, &cell.name, &mut out);

    let children = interior_children(cell);
    let names: Vec<&str> = children.iter().map(|c| c.node.name.as_str()).collect();
    let listed = || if names.is_empty() { "none".to_string() } else { names.join(", ") };
    for s in &cell.sections {
        let Section::Runtime(rt) = &s.node else { continue };
        for entry in &rt.entries {
            match &entry.node {
                RuntimeEntry::Start { cell_name } if !names.contains(&cell_name.as_str()) => {
                    out.push(Issue {
                        kind: "runtime_unknown_cell",
                        message: format!("`start {cell_name}`: {} has no interior cell named {cell_name} (interior cells: {})", cell.name, listed()),
                        span: entry.span,
                    });
                }
                RuntimeEntry::Connect { from_cell, signal, to_cell } => {
                    for c in [from_cell, to_cell] {
                        if !names.contains(&c.as_str()) {
                            out.push(Issue {
                                kind: "runtime_unknown_cell",
                                message: format!("`connect {from_cell}.{signal} -> {to_cell}`: {} has no interior cell named {c} (interior cells: {})", cell.name, listed()),
                                span: entry.span,
                            });
                        }
                    }
                }
                RuntimeEntry::Emit { signal_name, args } => {
                    let handlers: Vec<(&str, &OnSection)> = children.iter().flat_map(|c| c.node.sections.iter().filter_map(move |s| match &s.node {
                        Section::OnSignal(on) if &on.signal_name == signal_name => Some((c.node.name.as_str(), on)),
                        _ => None,
                    })).collect();
                    for (child, on) in handlers {
                        if !accepted_arities(&on.params).contains(&args.len()) {
                            out.push(Issue {
                                kind: "runtime_signal_args",
                                message: format!("`signal {signal_name}(…)` passes {} argument(s), and {child}.{signal_name} takes {}", args.len(), on.params.len()),
                                span: entry.span,
                            });
                            continue;
                        }
                        for (a, p) in args.iter().zip(on.params.iter()) {
                            if let Some(k) = literal_kind(&a.node) {
                                if !accepts(&p.ty.node, k) {
                                    let ty = match &p.ty.node { TypeExpr::Simple(t) => t.clone(), _ => "?".to_string() };
                                    out.push(Issue {
                                        kind: "runtime_signal_args",
                                        message: format!("`signal {signal_name}(…)`: parameter '{}' of {child}.{signal_name} is {ty}, and the runtime section passes a {k} literal", p.name),
                                        span: a.span,
                                    });
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}
