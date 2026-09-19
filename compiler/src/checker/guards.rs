//! Transition guards must be evaluable.
//!
//! `a -> b { guard { amount < 10000 } }` is evaluated when a handler calls
//! `transition(id, "b")`, in this scope: the locals of THAT handler at the
//! call, the cell's memory slots, builtins, and `_id` / `_from` / `_to`.
//! A name the calling handler never binds is an `UndefinedVar` at runtime —
//! and an `assert_fails transition(...)` then passes for the wrong reason.
//! This pass reports it at check time, per (guard, calling handler).

use crate::ast::*;
use std::collections::HashSet;

pub struct GuardIssue {
    pub message: String,
    pub span: Span,
}

const GUARD_BINDINGS: &[&str] = &["_id", "_from", "_to", "true", "false"];

pub fn check_program(program: &Program) -> Vec<GuardIssue> {
    let mut issues = Vec::new();
    for cell in super::names::collect_cells(program) {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        let slots: HashSet<String> = cell
            .sections
            .iter()
            .filter_map(|s| if let Section::Memory(m) = &s.node { Some(m) } else { None })
            .flat_map(|m| m.slots.iter().map(|sl| sl.node.name.clone()))
            .collect();

        for section in &cell.sections {
            let Section::State(sm) = &section.node else { continue };
            for tr in &sm.transitions {
                let Some(guard) = &tr.node.guard else { continue };
                let mut names: Vec<String> = Vec::new();
                super::termination::walk_expr(&guard.node, &mut |e| {
                    if let Expr::Ident(n) = e {
                        if !names.contains(n) {
                            names.push(n.clone());
                        }
                    }
                });
                let free: Vec<&String> = names
                    .iter()
                    .filter(|n| {
                        !slots.contains(*n)
                            && !GUARD_BINDINGS.contains(&n.as_str())
                            && !super::names::builtin_names().contains(n.as_str())
                    })
                    .collect();
                if free.is_empty() {
                    continue;
                }

                // every handler that takes this transition with a literal target
                let mut callers = 0usize;
                // `every` / `after` blocks take transitions too (a tick with no
                // `amount` raised undefined_variable every tick)
                let ticks: Vec<OnSection> = cell.sections.iter().filter_map(|s| match &s.node {
                    Section::Every(e) => Some(OnSection { signal_name: format!("every {}ms", e.interval_ms), params: vec![], body: e.body.clone(), properties: vec![] }),
                    Section::After(e) => Some(OnSection { signal_name: format!("after {}ms", e.interval_ms), params: vec![], body: e.body.clone(), properties: vec![] }),
                    _ => None,
                }).collect();
                // handlers of cells WITHOUT a machine take this machine's
                // edges too when it is the program's only one (`N.force`
                // calling transition() passed check, then raised
                // undefined_variable in the guard)
                let all_cells = super::names::collect_cells(program);
                let machines = all_cells.iter().filter(|c| c.sections.iter().any(|s| matches!(s.node, Section::State(_)))).count();
                let foreign_cells: Vec<&CellDef> = if machines == 1 {
                    all_cells.iter().copied().filter(|c| c.name != cell.name && matches!(c.kind, CellKind::Cell | CellKind::Agent)
                        && !c.sections.iter().any(|s| matches!(s.node, Section::State(_)))).collect()
                } else { Vec::new() };
                let foreign: Vec<&OnSection> = foreign_cells.iter()
                    .flat_map(|c| c.sections.iter().filter_map(|s| match &s.node { Section::OnSignal(on) => Some(on), _ => None }))
                    .collect();
                // …and their every / after ticks
                let foreign_ticks: Vec<OnSection> = foreign_cells.iter().flat_map(|c| c.sections.iter().filter_map(|s| match &s.node {
                    Section::Every(e) => Some(OnSection { signal_name: format!("{} every {}ms", c.name, e.interval_ms), params: vec![], body: e.body.clone(), properties: vec![] }),
                    Section::After(e) => Some(OnSection { signal_name: format!("{} after {}ms", c.name, e.interval_ms), params: vec![], body: e.body.clone(), properties: vec![] }),
                    _ => None,
                })).collect();
                let handlers_and_ticks: Vec<&OnSection> = cell.sections.iter().filter_map(|s| match &s.node { Section::OnSignal(on) => Some(on), _ => None })
                    .chain(ticks.iter()).chain(foreign.into_iter()).chain(foreign_ticks.iter()).collect();
                for on in handlers_and_ticks {
                    let mut takes = false;
                    for stmt in &on.body {
                        super::termination::walk_stmt(&stmt.node, &mut |e| {
                            if let Expr::FnCall { name, args } = e {
                                if name == "transition" && args.len() >= 2 {
                                    match &args[1].node {
                                        Expr::Literal(Literal::String(s)) => {
                                            if s == &tr.node.to {
                                                takes = true;
                                            }
                                        }
                                        // a Variant name used as the target
                                        Expr::Ident(v) if v == &tr.node.to => takes = true,
                                        // a computed target can be ANY state:
                                        // this handler may take this edge
                                        _ => takes = true,
                                    }
                                }
                            }
                        });
                    }
                    if !takes {
                        continue;
                    }
                    callers += 1;
                    let mut bound: HashSet<String> = on.params.iter().map(|p| p.name.clone()).collect();
                    super::dispatch::bind_all(&on.body, &mut bound);
                    // bound only inside a branch (`if big { let amount = 50 }`)
                    // is not bound on every path to the transition: the
                    // runtime raised undefined_variable, not guard_failed
                    // …and only the top-level lets BEFORE the statement that
                    // calls transition() (`transition(…)  let amount = 5`
                    // passed check and raised undefined_variable)
                    let mut unconditional: HashSet<String> = on.params.iter().map(|p| p.name.clone()).collect();
                    for st in &on.body {
                        let mut has_transition = false;
                        super::termination::walk_stmt(&st.node, &mut |e| {
                            if matches!(e, Expr::FnCall { name, .. } if name == "transition") { has_transition = true; }
                        });
                        if has_transition { break; }
                        if let Statement::Let { name, .. } | Statement::Assign { name, .. } = &st.node {
                            unconditional.insert(name.clone());
                        }
                    }
                    for n in &free {
                        if bound.contains(*n) && !unconditional.contains(*n) {
                            issues.push(GuardIssue {
                                message: format!(
                                    "guard on `{} -> {}` reads '{}', which handler `{}` binds only inside a branch or after transition() — bind `let {} = …` at the top level of the handler, before transition(), so every path defines it",
                                    tr.node.from, tr.node.to, n, on.signal_name, n
                                ),
                                span: guard.span,
                            });
                            continue;
                        }
                        if !bound.contains(*n) {
                            issues.push(GuardIssue {
                                message: format!(
                                    "guard on `{} -> {}` reads '{}', but handler `{}` — which takes that \
                                     transition — has no variable '{}'. A guard sees the locals of the handler \
                                     calling transition(), the cell's memory slots, and _id / _from / _to: bind \
                                     `let {} = …` before the call",
                                    tr.node.from, tr.node.to, n, on.signal_name, n, n
                                ),
                                span: guard.span,
                            });
                        }
                    }
                }
                if callers == 0 {
                    issues.push(GuardIssue {
                        message: format!(
                            "guard on `{} -> {}` reads {} but no handler calls transition() toward `{}`, \
                             so nothing is known to bind {}. A guard sees the locals of the handler \
                             calling transition(), the cell's memory slots, and _id / _from / _to",
                            tr.node.from,
                            tr.node.to,
                            free.iter().map(|n| format!("'{}'", n)).collect::<Vec<_>>().join(", "),
                            tr.node.to,
                            if free.len() == 1 { "it" } else { "them" }
                        ),
                        span: guard.span,
                    });
                }
            }
        }
    }
    issues
}
