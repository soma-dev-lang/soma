//! V1.8: memory-invariant checking.
//!
//! `invariant expr` inside a `memory { }` section declares a property of
//! the DATA: every write to a guarded slot must satisfy it. The runtime
//! enforces it before any `.set()`/`.push()` commits (interpreter
//! check_invariants); this module adds the static side:
//!
//!   1. validate_program — check-time validation of the invariant
//!      expressions themselves (unknown names are check errors, not
//!      silent runtime surprises).
//!   2. verify_program_invariants — a refinement-style proof pass for
//!      `soma verify`: writes of statically-known values (literals,
//!      constant folds, clamp(..) ranges) are PROVEN against constant
//!      bounds; a provably-violating write is a verify FAILURE; everything
//!      else is reported as runtime-checked.
//!
//! Bindings an invariant may use: the section's slot names (each bound to
//! the value being written), `value` (alias), `key`, `size` (entry count
//! after the write), the legacy `_slot_len`/`_slot_name`/`_key`, and
//! builtins.

use crate::ast::*;
use std::collections::{HashMap, HashSet};

use super::names::{builtin_names, suggest};
use super::verify::{VerifyCheck, VerifyResult};

pub struct InvariantIssue {
    pub message: String,
    pub span: Span,
}

const GENERIC_BINDINGS: &[&str] = &[
    "value", "key", "size", "status", "_slot_len", "_slot_name", "_key", "true", "false",
];

/// Check-time validation: every name an invariant references must be
/// resolvable when the runtime evaluates it.
pub fn validate_program(program: &Program) -> Vec<InvariantIssue> {
    let mut issues = Vec::new();
    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        set_slot_int(&cell.node);
        for section in &cell.node.sections {
            let Section::Memory(mem) = &section.node else { continue };
            let slot_names: HashSet<&str> =
                mem.slots.iter().map(|s| s.node.name.as_str()).collect();
            for inv in &mem.invariants {
                let mut idents = HashSet::new();
                collect_idents(&inv.node, &mut idents);
                // `status` is the machine state of the written key: it needs
                // a `state` section in this cell
                if deep_idents(&inv.node).contains("status")
                    && !cell.node.sections.iter().any(|s| matches!(&s.node, Section::State(_)))
                {
                    issues.push(InvariantIssue {
                        message: format!("memory invariant reads `status` (the machine state of the written key), but cell '{}' declares no `state {{ }}` machine", cell.node.name),
                        span: inv.span,
                    });
                }
                // An invariant is evaluated per write, with only the slot
                // being written in scope. Naming two slots can never
                // evaluate — every write to either would be rejected.
                // (counted deep: a slot named inside `"{b}"`, a lambda or a
                // match arm is read at run time too)
                let deep = deep_idents(&inv.node);
                let mut named: Vec<&str> = slot_names
                    .iter()
                    .copied()
                    .filter(|s| deep.contains(*s))
                    .collect();
                // Several slots of the CELL: a rule between them
                // (`invariant reserved <= stock`), checked on every write to
                // any of them — the written one is its new value, the others
                // are read at the same key. It cannot be proven by induction:
                // verify reports it runtime-checked.
                if named.len() > 1 {
                    named.sort();
                    let text = crate::ast::render_expr(&inv.node);
                    // a Map / List slot read at a key it has no entry for is
                    // () — the bare form would fail on the first write
                    // only a BARE reference to a keyed slot reads at the key
                    // (`b.size <= a.size` counts entries: nothing to default)
                    let counted: HashSet<String> = {
                        let mut c: HashSet<String> = HashSet::new();
                        crate::checker::literals::for_each_in_expr(&inv.node, &mut |e| match e {
                            Expr::FieldAccess { target, field } if matches!(field.as_str(), "size" | "len" | "count" | "length") =>
                                if let Expr::Ident(n) = &target.node { c.insert(n.clone()); },
                            Expr::MethodCall { target, method, args } if args.is_empty() && matches!(method.as_str(), "size" | "len" | "count") =>
                                if let Expr::Ident(n) = &target.node { c.insert(n.clone()); },
                            Expr::FnCall { name, args } if matches!(name.as_str(), "len" | "size") && args.len() == 1 =>
                                if let Expr::Ident(n) = &args[0].node { c.insert(n.clone()); },
                            _ => {}
                        });
                        c
                    };
                    let keyed = named.iter().any(|n| !counted.contains(*n) && mem.slots.iter().any(|sl| sl.node.name == **n
                        && matches!(&sl.node.ty.node, crate::ast::TypeExpr::Generic { name, .. } if name == "Map" || name == "List")));
                    // `stock.get(key)` in a rule between slots is the value
                    // BEFORE the write: on a write to `stock` it tests the old
                    // one, so that slot is not guarded at all
                    for n in &named {
                        if text.contains(&format!("{}.get(", n)) || text.contains(&format!("{}[key]", n)) {
                            issues.push(InvariantIssue {
                                message: format!("memory invariant between slots reads `{}.get(key)` — that is the value BEFORE the write, so a write to '{}' compares the OLD value and is not guarded: name the slot bare (`({} ?? 0)`), which is its new value on its own writes and its value at the same key on the others'", n, n, n),
                                span: inv.span,
                            });
                            break;
                        }
                    }
                    if keyed && !text.contains("??") {
                        issues.push(InvariantIssue {
                            message: format!(
                                "memory invariant between slots ({}) — the other slot is read at the SAME key, and a key it has no entry for reads as (), which no comparison accepts: default the sides (`(reserved ?? 0) <= (stock ?? 0)`). It is checked on every write to either slot, and stays runtime-checked (verify cannot prove a rule between two slots)",
                                named.join(", ")
                            ),
                            span: inv.span,
                            // a warning, not an error
                        });
                    }
                }
                for name in idents {
                    if slot_names.contains(name.as_str())
                        || GENERIC_BINDINGS.contains(&name.as_str())
                    {
                        continue;
                    }
                    let candidates: Vec<String> = slot_names
                        .iter()
                        .map(|s| s.to_string())
                        .chain(GENERIC_BINDINGS.iter().map(|s| s.to_string()))
                        .collect();
                    let suggestion = suggest(&name, candidates.iter())
                        .map(|s| format!(" (did you mean '{}'?)", s))
                        .unwrap_or_default();
                    issues.push(InvariantIssue {
                        message: format!(
                            "memory invariant references unknown name '{name}'{suggestion} — \
                             invariants may use the section's slot names, 'value', 'key', \
                             'size', and builtins"
                        ),
                        span: inv.span,
                    });
                }
                // called functions must be builtins — handlers are not
                // callable from an invariant context
                // every call the invariant can make — inside lambdas,
                // interpolation, match guards, UFCS (`"{_side()}"`,
                // `all(x => _side() > 0)`, `1._nope()` ran code or passed)
                let mut fns = HashSet::new();
                let mut effects: Vec<String> = Vec::new();
                crate::checker::desugar::for_each_deep(&inv.node, &mut |e| match e {
                    Expr::FnCall { name, .. } => { fns.insert(name.clone()); }
                    Expr::MethodCall { method, .. } if !matches!(method.as_str(), "get" | "has" | "contains" | "keys" | "values" | "len" | "size" | "length") => { fns.insert(method.clone()); }
                    _ => {}
                });
                for f in &fns {
                    if crate::checker::names::EFFECT_BUILTINS.contains(&f.as_str()) || crate::checker::names::IO_BUILTINS.contains(&f.as_str()) { effects.push(f.clone()); }
                }
                // a statement in a block of the invariant writes / emits
                // (`all(q => { other["inv"] = 7  true })` ran on every write)
                let mut stmt_effect: Option<String> = None;
                crate::checker::literals::for_each_stmt_in_expr(&inv.node, &mut |st| if stmt_effect.is_none() {
                    match st {
                        Statement::IndexSet { name, .. } => stmt_effect = Some(format!("writes `{}[…]`", name)),
                        Statement::Assign { name, .. } if name.contains('.') => stmt_effect = Some(format!("assigns `{}`", name)),
                        Statement::Emit { signal_name, .. } => stmt_effect = Some(format!("emits `{}`", signal_name)),
                        Statement::MethodCall { target, method, .. } => stmt_effect = Some(format!("calls {}.{}()", target, method)),
                        _ => {}
                    }
                });
                if let Some(w) = stmt_effect {
                    issues.push(InvariantIssue {
                        message: format!("memory invariant {w} — an invariant is a pure condition checked on every write: no writes, emits or calls, even inside a block lambda"),
                        span: inv.span,
                    });
                }
                for f in effects {
                    issues.push(InvariantIssue {
                        message: format!("memory invariant calls {f}() — an invariant is a condition checked on every write: it may not call think(), transition(), I/O or the network (it would run unseen by the cost, termination and refinement proofs)"),
                        span: inv.span,
                    });
                }
                for f in fns {
                    if !builtin_names().contains(f.as_str()) {
                        issues.push(InvariantIssue {
                            message: format!(
                                "memory invariant calls '{f}' which is not a builtin — \
                                 invariants run outside handler context and may only call builtins"
                            ),
                            span: inv.span,
                        });
                    }
                }
            }
        }
    }
    issues
}

// ── Static proof pass for `soma verify` ─────────────────────────────

/// What we can know about a written value at check time.
#[derive(Clone, Copy, Debug)]
enum Known {
    Exact(f64),
    Range(f64, f64),
    Unknown,
}

/// Three-valued proof outcome.
#[derive(Clone, Copy, PartialEq)]
enum Proof {
    Holds,
    Violated,
    Unknown,
}

/// Record which slots of `cell` hold Ints (Map<K, Int>, List<Int>):
/// untyped / Any / Float slots may hold 9.5, so a read of them gets no
/// integer narrowing
fn set_slot_int(cell: &CellDef) {
    SLOT_INT.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        for sec in &cell.sections {
            if let Section::Memory(mem) = &sec.node {
                for sl in &mem.slots {
                    let int = match &sl.node.ty.node {
                        TypeExpr::Generic { args, .. } => args.last().map_or(false, |a| matches!(&a.node, TypeExpr::Simple(t) if t == "Int" || t == "BigInt")),
                        _ => false,
                    };
                    m.insert(sl.node.name.clone(), int);
                }
            }
        }
    });
}

pub fn verify_program_invariants(program: &Program) -> Vec<VerifyResult> {
    let mut results = Vec::new();

    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        set_slot_int(&cell.node);

        // invariant → the slots it guards (same scoping rule as runtime)
        let mut guarded: Vec<(Expr, Vec<String>, String)> = Vec::new();
        let mut status_notes: Vec<String> = Vec::new();
        for section in &cell.node.sections {
            let Section::Memory(mem) = &section.node else { continue };
            let slot_names: Vec<String> =
                mem.slots.iter().map(|s| s.node.name.clone()).collect();
            for inv in &mem.invariants {
                let refs = deep_idents(&inv.node);
                let named: Vec<String> = slot_names
                    .iter()
                    .filter(|n| refs.contains(*n))
                    .cloned()
                    .collect();
                // a rule between the lifecycle and the data (`status`): the
                // prover has no model of which state a handler writes in — it
                // is checked on every write of the slot and every transition
                // of the instance, and said so (a Note, not a proof)
                if refs.contains("status") {
                    let targets = if named.is_empty() { slot_names.clone() } else { named };
                    status_notes.push(format!(
                        "invariant {} — reads `status`: checked at run time on every write to {} and on every transition() of the written key (a rule between lifecycle and data is not proven by induction)",
                        render_expr(&inv.node), targets.join(", ")
                    ));
                    continue;
                }
                let targets = if named.is_empty() { slot_names.clone() } else { named };
                guarded.push((normalize_invariant(&inv.node, &slot_names), targets, render_expr(&inv.node)));
            }
        }
        if guarded.is_empty() && status_notes.is_empty() {
            continue;
        }

        // Induction hypothesis: a value READ from a guarded slot satisfied
        // that slot's invariants when it was written (every write is
        // checked before it commits; storage starts empty). So
        // `counts.get(k) ?? 0` lies in the interval the invariants allow.
        let mut hyp: HashMap<String, (f64, f64)> = HashMap::new();
        for (inv, targets, _) in &guarded {
            for slot in targets {
                let (mut lo, mut hi) = hyp.get(slot).copied().unwrap_or((f64::NEG_INFINITY, f64::INFINITY));
                for c in conjuncts(inv) {
                    if let Some((l, h)) = interval_of(c, slot) {
                        lo = lo.max(l);
                        hi = hi.min(h);
                    }
                }
                hyp.insert(slot.clone(), (lo, hi));
            }
        }

        // `every` / `after` blocks write slots too: they are writers (their
        // writes were invisible — "no handler writes to guarded slots")
        let ticks: Vec<OnSection> = cell.node.sections.iter().enumerate().filter_map(|(i, s)| match &s.node {
            Section::Every(e) => Some(OnSection { signal_name: format!("every {}ms #{}", e.interval_ms, i), params: vec![], body: e.body.clone(), properties: if e.task { vec!["task".to_string()] } else { vec![] } }),
            Section::After(e) => Some(OnSection { signal_name: format!("after {}ms #{}", e.interval_ms, i), params: vec![], body: e.body.clone(), properties: if e.task { vec!["task".to_string()] } else { vec![] } }),
            _ => None,
        }).collect();
        let mut handlers: HashMap<String, &OnSection> = cell
            .node
            .sections
            .iter()
            .filter_map(|s| if let Section::OnSignal(on) = &s.node { Some((on.signal_name.clone(), on)) } else { None })
            .collect();
        for t in &ticks { handlers.insert(t.signal_name.clone(), t); }
        let handlers = handlers;
        // every handler of the program, for growth that goes through
        // another cell (`B.relay(x)` → `A.extra(x)`, which pushes)
        let mut all_handlers: Vec<(String, &OnSection)> = program.cells.iter()
            .flat_map(|c| c.node.sections.iter().filter_map(move |s| match &s.node {
                Section::OnSignal(on) => Some((c.node.name.clone(), on)),
                _ => None,
            }))
            .collect();
        // this cell's ticks too (a `[task]` tick's think() ends a step)
        for t in &ticks { all_handlers.push((cell.node.name.clone(), t)); }
        let all_handlers = all_handlers;
        let cell_names: HashSet<String> = program.cells.iter().map(|c| c.node.name.clone()).collect();
        // every handler of each cell: a computed `delegate` may run any
        let cell_handlers: HashMap<String, Vec<String>> = program.cells.iter().map(|c| (c.node.name.clone(),
            c.node.sections.iter().filter_map(|s| match &s.node { Section::OnSignal(o) => Some(o.signal_name.clone()), _ => None }).collect())).collect();
        // the face tools of each cell: a think() there may run any of them
        // (a tool that pushed to the slot broke a "proven" size bound)
        let cell_tools: HashMap<String, Vec<String>> = program.cells.iter().map(|c| (c.node.name.clone(),
            c.node.sections.iter().filter_map(|s| match &s.node {
                Section::Face(f) => Some(f.declarations.iter().filter_map(|d| match &d.node { FaceDecl::Tool(t) => Some(t.name.clone()), _ => None }).collect::<Vec<_>>()),
                _ => None,
            }).flatten().collect())).collect();

        // every write site in every handler: (handler, slot, value expr)
        let mut writes: Vec<(String, String, Expr, bool, Vec<usize>, Option<String>)> = Vec::new();
        for on in handlers.values() {
            collect_writes_stmts(&on.body, &on.signal_name, false, &mut writes);
        }
        writes.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));

        // local variable ranges, per (handler, write site): the facts that
        // hold differ by block
        let mut locals: HashMap<(String, Vec<usize>), HashMap<String, Known>> = HashMap::new();
        for (name, _, _, _, path, _) in &writes {
            if let Some(on) = handlers.get(name) {
                locals.entry((name.clone(), path.clone())).or_insert_with(|| local_ranges(on, &hyp_for(on, &hyp), &handlers, path));
            }
        }

        let mut result = VerifyResult {
            machine_name: format!("{}/invariants", cell.node.name),
            states: vec![],
            initial: String::new(),
            terminal_states: vec![],
            transitions: vec![],
            checks: vec![],
        };
        for note in status_notes.drain(..) {
            result.checks.push(VerifyCheck::Note(note));
        }

        for (inv, targets, inv_text) in &guarded {
            let relevant: Vec<&(String, String, Expr, bool, Vec<usize>, Option<String>)> = writes
                .iter()
                .filter(|(_, slot, _, _, _, _)| targets.contains(slot))
                .collect();
            if relevant.is_empty() {
                result.checks.push(VerifyCheck::Pass(format!(
                    "invariant {inv_text} — no handler writes to guarded slots"
                )));
                continue;
            }
            let parts = conjuncts(inv);
            // a handler that also sets the slot is judged on its sets: its
            // delete is no "only deletes" ✓
            let setters: HashSet<(&str, &str)> = relevant.iter()
                .filter(|(_, _, v, _, _, _)| !matches!(v, Expr::Ident(n) if n == "<deleted entry>"))
                .map(|(h, sl, _, _, _, _)| (h.as_str(), sl.as_str())).collect();
            let mut runtime_checked: Vec<String> = Vec::new();
            for (handler, slot, value_expr, in_try, wpath, wkey) in relevant {
                // a delete removes an entry that already satisfied a VALUE
                // invariant; only a `size` clause can flip on it
                if matches!(value_expr, Expr::Ident(n) if n == "<deleted entry>") {
                    if setters.contains(&(handler.as_str(), slot.as_str())) { continue; }
                    let mut names = HashSet::new();
                    collect_idents(inv, &mut names);
                    let mut fns = HashSet::new();
                    collect_fn_names(inv, &mut fns);
                    // a List delete SHIFTS the later elements: an invariant
                    // about `key` (the index) can break without a write
                    let list_slot = cell.node.sections.iter().any(|sec| matches!(&sec.node, Section::Memory(m)
                        if m.slots.iter().any(|sl| sl.node.name == *slot && matches!(&sl.node.ty.node, TypeExpr::Simple(t) | TypeExpr::Generic { name: t, .. } if t == "List"))));
                    let keyed = names.contains("key") || names.contains("_key");
                    // a rule BETWEEN slots CAN break on a delete: the entry
                    // it drops takes that side to () (a "proven" ✓ let a
                    // delete leave 3 reserved against 0 in stock)
                    let cross = {
                        let all: Vec<String> = cell.node.sections.iter().filter_map(|sec| match &sec.node {
                            Section::Memory(m) => Some(m.slots.iter().map(|sl| sl.node.name.clone()).collect::<Vec<_>>()), _ => None })
                            .flatten().collect();
                        all.iter().any(|n| n != slot && names.contains(n.as_str()))
                    };
                    if cross {
                        runtime_checked.push(format!("{handler} → {slot} (a rule between slots: this delete takes the other side to () — checked at run time)"));
                        continue;
                    }
                    if list_slot && keyed {
                        runtime_checked.push(format!("{handler} → {slot} (a List delete shifts the later elements to new indexes; `key` changes without a write — checked at run time)"));
                        continue;
                    }
                    if !names.contains("size") && !names.contains("len") && !fns.contains("len") && !fns.contains("size") {
                        result.checks.push(VerifyCheck::Pass(format!(
                            "invariant {inv_text} — writer '{handler}' only deletes from '{slot}' (a value invariant cannot break on a delete)"
                        )));
                        continue;
                    }
                    // `size <= K` cannot break on a delete either: it shrinks
                    // (the bounded-queue pop was "may grow past 64")
                    let sized = |c: &Expr| { let mut n = HashSet::new(); collect_idents(c, &mut n); let mut f = HashSet::new(); collect_fn_names(c, &mut f);
                        n.contains("size") || n.contains("len") || f.contains("len") || f.contains("size") };
                    if conjuncts(inv).iter().all(|c| !sized(c) || size_upper_bound(c, slot).is_some()) {
                        result.checks.push(VerifyCheck::Pass(format!(
                            "invariant {inv_text} — writer '{handler}' only deletes from '{slot}' (a delete cannot grow it)"
                        )));
                        continue;
                    }
                }
                let hyp_h = handlers.get(handler).map(|on| hyp_for(on, &hyp)).unwrap_or_else(|| hyp.clone());
                // a `require` about a READ of this slot (`(used.get(u) ?? 0) + n
                // <= 100`) no longer describes the slot once it was written
                // again — by a second write here, or by a handler this one
                // reaches (a helper, emit, delegate, itself): drop such facts
                let rewritten = {
                    let own = writes.iter().filter(|(h, sl, e, _, _, _)| h == handler && sl == slot && !matches!(e, Expr::Ident(n) if n == "<deleted entry>")).count() > 1;
                    let me = cell.node.name.clone();
                    let mut seen: HashSet<(String, String)> = HashSet::new();
                    let mut stack: Vec<(String, String)> = vec![(me.clone(), handler.clone())];
                    let mut other = false;
                    while let Some((c, h)) = stack.pop() {
                        if c == me && h == *handler && !seen.is_empty() { other = true; }
                        if !seen.insert((c.clone(), h.clone())) { continue; }
                        if (c != me || h != *handler) && c == me
                            && writes.iter().any(|(wh, sl, e, _, _, _)| *wh == h && sl == slot && !matches!(e, Expr::Ident(n) if n == "<deleted entry>")) { other = true; }
                        let Some((_, on)) = all_handlers.iter().find(|(cn, o)| *cn == c && o.signal_name == h) else { continue };
                        for (target, name) in calls_of(&on.body, &cell_names, cell_tools.get(&c).map(|v| v.as_slice()).unwrap_or(&[]), &cell_handlers) {
                            match target {
                                None => {
                                    if all_handlers.iter().any(|(cn, o)| *cn == c && o.signal_name == name) { stack.push((c.clone(), name)); }
                                    else { for (cn, o) in &all_handlers { if o.signal_name == name { stack.push((cn.clone(), name.clone())); } } }
                                }
                                Some(t) if t == "*" => { for (cn, o) in &all_handlers { if o.signal_name == name { stack.push((cn.clone(), name.clone())); } } }
                                Some(t) => stack.push((t, name)),
                            }
                        }
                    }
                    own || other
                };
                let mut vars_w = locals.get(&(handler.clone(), wpath.clone())).cloned().unwrap_or_default();
                // a `[task]` handler's think() ends a step: other requests and
                // tasks may write between a read / require before it and this
                // write — no fact about locals holds across it
                // …also a think() in a handler it reaches (a helper, delegate,
                // an emit listener): it ends the step all the same
                let task_with_think = handlers.get(handler).map_or(false, |on| on.properties.iter().any(|p| p == "task")) && {
                    let me = cell.node.name.clone();
                    let mut seen: HashSet<(String, String)> = HashSet::new();
                    let mut stack: Vec<(String, String)> = vec![(me.clone(), handler.clone())];
                    let mut t = false;
                    while let Some((c, h)) = stack.pop() {
                        if t || !seen.insert((c.clone(), h.clone())) { continue; }
                        let Some((_, on)) = all_handlers.iter().find(|(cn, o)| *cn == c && o.signal_name == h) else { continue };
                        crate::checker::literals::for_each_expr(&on.body, &mut |e| if matches!(e, Expr::FnCall { name, .. } if name == "think" || name == "think_json" || name == "vote") { t = true; });
                        for (target, name) in calls_of(&on.body, &cell_names, cell_tools.get(&c).map(|v| v.as_slice()).unwrap_or(&[]), &cell_handlers) {
                            match target {
                                None => {
                                    if all_handlers.iter().any(|(cn, o)| *cn == c && o.signal_name == name) { stack.push((c.clone(), name)); }
                                    else { for (cn, o) in &all_handlers { if o.signal_name == name { stack.push((cn.clone(), name.clone())); } } }
                                }
                                Some(tg) if tg == "*" => { for (cn, o) in &all_handlers { if o.signal_name == name { stack.push((cn.clone(), name.clone())); } } }
                                Some(tg) => stack.push((tg, name)),
                            }
                        }
                    }
                    t
                };
                if task_with_think { vars_w.clear(); }
                if rewritten {
                    let get_pat = format!("{}.get(", slot);
                    let idx_pat = format!("{}[", slot);
                    vars_w.retain(|k, _| !(k.starts_with("__") && (k.contains(&get_pat) || k.contains(&idx_pat))));
                }
                let ctx = RangeCtx {
                    hyp: &hyp_h,
                    vars: vars_w,
                    handlers: &handlers,
                    depth: 0,
                };
                // a bare `c.get(k)` / `c[k]` WRITTEN as is may be () (a missing
                // key): the invariant cannot hold of it (the runtime refuses the
                // write, kind invariant) — `c.get(k) + 1` raises before writing
                fn may_be_unit(e: &Expr) -> bool {
                    match e {
                        Expr::MethodCall { method, .. } if method == "get" => true,
                        Expr::Index { .. } => true,
                        Expr::IfExpr { then_result, else_result, .. } => may_be_unit(&then_result.node) || may_be_unit(&else_result.node),
                        Expr::Match { arms, .. } => arms.iter().any(|a| may_be_unit(&a.result.node)),
                        _ => false,
                    }
                }
                let known = if may_be_unit(value_expr) { Known::Unknown } else { ctx.range_of(value_expr) };
                let inductive = uses_slot_read(value_expr, &ctx);
                let mut verdicts: Vec<Proof> = parts.iter().map(|c| prove(c, slot, known)).collect();
                // a monotone counter, the shape the docs recommend:
                // `invariant value >= (seq.get(key) ?? 0)` written by
                // `seq.set(k, (seq.get(k) ?? 0) + n)` with n ≥ 0, or by
                // `max(seq.get(k) ?? 0, x)` — the new value is the old one
                // or more, whatever the old one is
                for (c, v) in parts.iter().zip(verdicts.iter_mut()) {
                    // …not across a think(): the value read before it may be stale
                    if *v == Proof::Holds || task_with_think { continue; }
                    let Expr::CmpOp { left, op, right } = c else { continue };
                    if !matches!(op, CmpOp::Ge | CmpOp::Gt) { continue; }
                    if !matches!(&left.node, Expr::Ident(n) if n == "value" || n == slot) { continue; }
                    // right side: this slot read at `key` (?? 0 allowed)
                    let reads_own_key = |e: &Expr| -> bool {
                        let inner = match e { Expr::FnCall { name, args } if name == "_coalesce" && args.len() == 2 => &args[0].node, other => other };
                        match inner {
                            Expr::MethodCall { target, method, args } if method == "get" && args.len() == 1 =>
                                matches!(&target.node, Expr::Ident(t) if t == slot) && matches!(&args[0].node, Expr::Ident(k) if k == "key"),
                            Expr::Index { target, index } =>
                                matches!(&target.node, Expr::Ident(t) if t == slot) && matches!(&index.node, Expr::Ident(k) if k == "key"),
                            _ => false,
                        }
                    };
                    if !reads_own_key(&right.node) { continue; }
                    // the written value: old + n (n ≥ 0, and > 0 for a strict >), or max(old, …)
                    let same_key = |e: &Expr| -> bool {
                        let inner = match e { Expr::FnCall { name, args } if name == "_coalesce" && args.len() == 2 => &args[0].node, other => other };
                        let key_expr = match inner {
                            Expr::MethodCall { target, method, args } if method == "get" && args.len() == 1 && matches!(&target.node, Expr::Ident(t) if t == slot) => Some(&args[0].node),
                            Expr::Index { target, index } if matches!(&target.node, Expr::Ident(t) if t == slot) => Some(&index.node),
                            _ => None,
                        };
                        match (key_expr, wkey.as_deref()) {
                            (Some(Expr::Ident(k)), Some(wk)) => k == wk,
                            _ => false,
                        }
                    };
                    let grows = match value_expr {
                        Expr::BinaryOp { left: a, op: BinOp::Add, right: b } => {
                            let (inc, other) = if same_key(&a.node) { (Some(&b.node), true) } else if same_key(&b.node) { (Some(&a.node), true) } else { (None, false) };
                            other && match (inc.map(|e| ctx.range_of(e)).and_then(bounds), op) {
                                (Some((lo, _)), CmpOp::Ge) => lo >= 0.0,
                                (Some((lo, _)), CmpOp::Gt) => lo > 0.0,
                                _ => false,
                            }
                        }
                        Expr::FnCall { name, args } if name == "max" && args.len() == 2 && matches!(op, CmpOp::Ge) =>
                            args.iter().any(|a| same_key(&a.node)),
                        _ => false,
                    };
                    if grows { *v = Proof::Holds; }
                }
                // `size <= K` after a write that adds at most one entry:
                // proven when the writer required the slot's size < K
                // (`require rows.size < 500`, `require len(rows) < 500`)
                // …and only for the handler's ONE adding write to this slot,
                // outside any loop (two pushes after one require add two)
                // a set on a key the handler knows exists does not grow the slot
                // a delete of this slot in the handler (or in a handler it
                // reaches) can remove the required key before the write:
                // `require m.get(k) != ()  m.delete(k)  m.set(k, 2)` grew it
                let deletes_reached = {
                    let me = cell.node.name.clone();
                    let mut seen: HashSet<(String, String)> = HashSet::new();
                    let mut stack: Vec<(String, String)> = vec![(me.clone(), handler.clone())];
                    let mut hit = false;
                    while let Some((c, h)) = stack.pop() {
                        if hit || !seen.insert((c.clone(), h.clone())) { continue; }
                        if c == me && writes.iter().any(|(wh, sl, e, _, _, _)| *wh == h && sl == slot && matches!(e, Expr::Ident(n) if n == "<deleted entry>")) { hit = true; break; }
                        let Some((_, on)) = all_handlers.iter().find(|(cn, o)| *cn == c && o.signal_name == h) else { continue };
                        for (target, name) in calls_of(&on.body, &cell_names, cell_tools.get(&c).map(|v| v.as_slice()).unwrap_or(&[]), &cell_handlers) {
                            match target {
                                None => {
                                    if all_handlers.iter().any(|(cn, o)| *cn == c && o.signal_name == name) { stack.push((c.clone(), name)); }
                                    else { for (cn, o) in &all_handlers { if o.signal_name == name { stack.push((cn.clone(), name.clone())); } } }
                                }
                                Some(t) if t == "*" => { for (cn, o) in &all_handlers { if o.signal_name == name { stack.push((cn.clone(), name.clone())); } } }
                                Some(t) => stack.push((t, name)),
                            }
                        }
                    }
                    hit
                };
                let key_exists = |path: &Vec<usize>, key: &Option<String>| -> bool {
                    let Some(k) = key else { return false };
                    if deletes_reached { return false; }
                    let Some(on) = handlers.get(handler) else { return false };
                    // the key expression must name only once-bound values:
                    // `let key = k  require m.get(key) != ()  key = k2  m.set(key, …)`
                    // sets a DIFFERENT key than the one required
                    let mut assigns: Vec<(&str, &Expr)> = Vec::new();
                    collect_assigns(&on.body, &mut assigns);
                    let rebound = |n: &str| assigns.iter().filter(|(m, _)| *m == n).count() > 1
                        || (on.params.iter().any(|p| p.name == n) && assigns.iter().any(|(m, _)| *m == n));
                    let mut shadow: HashSet<String> = HashSet::new();
                    collect_binders_stmts(&on.body, &mut shadow);
                    // a `for` variable bound ONCE in the handler (no other
                    // loop, lambda, match, let or parameter of that name) is
                    // one binding, not a shadow
                    let mut for_counts: HashMap<String, usize> = HashMap::new();
                    count_for_vars(&on.body, &mut for_counts);
                    let mut non_for: HashSet<String> = HashSet::new();
                    collect_non_for_binders(&on.body, &mut non_for);
                    let single_for = |w: &str| for_counts.get(w) == Some(&1) && !non_for.contains(w)
                        && !on.params.iter().any(|p| p.name == w)
                        && !assigns.iter().any(|(m, e)| *m == w && !std::ptr::eq(*e, &UNKNOWN_EXPR));
                    // the key must be a PURE text over names: no call (random),
                    // no field / index read (`p.k`, `ids[0]`, `cur.get("k")` —
                    // their value can change between the require and the set)
                    let unquoted: String = { let mut out = String::new(); let mut q = false; for ch in k.chars() { if ch == '"' { q = !q; continue; } if !q { out.push(ch); } } out };
                    if unquoted.contains('(') || unquoted.contains('.') || unquoted.contains('[') { return false; }
                    fn index_sets(stmts: &[Spanned<Statement>], out: &mut HashSet<String>) {
                        for st in stmts {
                            match &st.node {
                                Statement::IndexSet { name, .. } => { out.insert(name.clone()); }
                                Statement::If { then_body, else_body, .. } => { index_sets(then_body, out); index_sets(else_body, out); }
                                Statement::For { body, .. } | Statement::While { body, .. } => index_sets(body, out),
                                _ => {}
                            }
                        }
                    }
                    let mut mutated: HashSet<String> = HashSet::new();
                    index_sets(&on.body, &mut mutated);
                    if unquoted.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| !w.is_empty() && (rebound(w) || mutated.contains(w) || (shadow.contains(w) && !single_for(w)))) { return false; }
                    // `let r = rows.get(k)` bound once: `r != ()` says k exists
                    let aliases: HashMap<String, String> = assigns.iter()
                        .filter(|(n, _)| assigns.iter().filter(|(m, _)| m == n).count() == 1 && !shadow.contains(*n))
                        .filter_map(|(n, e)| match e {
                            Expr::MethodCall { target, method, args } if method == "get" && args.len() == 1
                                && matches!(&target.node, Expr::Ident(t) if t == slot) => Some((n.to_string(), render_expr(&args[0].node))),
                            _ => None,
                        }).collect();
                    existing_key_facts(&on.body, &[], slot, &aliases).iter().any(|(fp, fk, excl, after)| path.starts_with(fp) && fk == k && excl.as_ref().map_or(true, |e| !path.starts_with(e))
                        // a require / early exit covers only the writes AFTER it
                        && after.map_or(true, |j| path.get(fp.len()).map_or(false, |s| *s / 4 > j)))
                };
                if size_upper_bound_any(&parts, slot) && key_exists(wpath, wkey) {
                    for (c, v) in parts.iter().zip(verdicts.iter_mut()) {
                        if size_upper_bound(c, slot).is_some() { *v = Proof::Holds; }
                    }
                }
                // a clause that IS a require at this write (`require next >=
                // (versions.get(id) ?? 0)` for `value >= (versions.get(key) ?? 0)`):
                // it holds when nothing between can change what it reads — the
                // handler's ONE write to this slot, and no handler it calls,
                // emits to or dispatches (think tools) could write it either
                {
                    let one_write = writes.iter().filter(|(h, sl, _, _, _, _)| h == handler && sl == slot).count() == 1;
                    // a callee blocks the rule only when it (transitively, via
                    // calls, emits and think tools) can write THIS slot — a pure
                    // helper call (`let x = _pure(id)`) defeated the proof
                    let calls_handlers = {
                        let me = cell.node.name.clone();
                        let mut seen: HashSet<(String, String)> = HashSet::new();
                        let mut stack: Vec<(String, String)> = Vec::new();
                        let push_calls = |c: &str, on: &OnSection, stack: &mut Vec<(String, String)>| {
                            for (target, name) in calls_of(&on.body, &cell_names, cell_tools.get(c).map(|v| v.as_slice()).unwrap_or(&[]), &cell_handlers) {
                                match target {
                                    None => {
                                        if all_handlers.iter().any(|(cn, o)| cn == c && o.signal_name == name) { stack.push((c.to_string(), name)); }
                                        else { for (cn, o) in &all_handlers { if o.signal_name == name { stack.push((cn.clone(), name.clone())); } } }
                                    }
                                    Some(t) if t == "*" => { for (cn, o) in &all_handlers { if o.signal_name == name { stack.push((cn.clone(), name.clone())); } } }
                                    Some(t) => stack.push((t, name)),
                                }
                            }
                        };
                        if let Some(on) = handlers.get(handler) { push_calls(&me, on, &mut stack); }
                        let mut hit = handlers.get(handler).is_none();
                        while let Some((c, h)) = stack.pop() {
                            if hit || !seen.insert((c.clone(), h.clone())) { continue; }
                            if c == me && h == *handler { hit = true; break; }   // recursion back into the writer
                            if c == me && writes.iter().any(|(wh, sl, _, _, _, _)| *wh == h && sl == slot) { hit = true; break; }
                            match all_handlers.iter().find(|(cn, o)| *cn == c && o.signal_name == h) {
                                Some((_, on)) => push_calls(&c, on, &mut stack),
                                None => {}
                            }
                        }
                        hit
                    };
                    // outside any loop: a second pass writes again after the
                    // first changed what the clause reads (`value > old`)
                    let looped = wpath.iter().any(|step| step % 4 == 2 || *step == usize::MAX / 2);
                    // the clause and the written value are PURE over locals and
                    // THIS slot's `.get`: no other call (next_id / random /
                    // now / think / read_file / recall / get_status / len of
                    // another slot were read twice, differently), no other
                    // slot (`c.set(…)` between changed `c.get(…)`), and a key
                    // that is a plain name or literal
                    fn pure_here(e: &Expr, slot: &str) -> bool {
                        match e {
                            Expr::Literal(_) | Expr::Ident(_) => true,
                            Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => pure_here(&left.node, slot) && pure_here(&right.node, slot),
                            Expr::Not(i) => pure_here(&i.node, slot),
                            Expr::FnCall { name, args } if name == "_coalesce" => args.iter().all(|a| pure_here(&a.node, slot)),
                            Expr::MethodCall { target, method, args } => method == "get"
                                && matches!(&target.node, Expr::Ident(t) if t == slot)
                                && args.iter().all(|a| pure_here(&a.node, slot)),
                            _ => false,
                        }
                    }
                    let key_plain = wkey.as_deref().map_or(true, |k| {
                        let k = k.trim();
                        (k.starts_with('"') && k.ends_with('"') && !k.contains('{'))
                            || (!k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || c == '_') && !k.starts_with(|c: char| c.is_ascii_digit()))
                            || k.parse::<i64>().is_ok()
                    });
                    let pure = pure_here(value_expr, slot) && key_plain;
                    if one_write && !calls_handlers && !looped && pure {
                        for (c, v) in parts.iter().zip(verdicts.iter_mut()) {
                            if *v != Proof::Holds && pure_here(c, slot) && ctx.vars.contains_key(&format!("__req__{}", subst_render(c, slot, value_expr, wkey.as_deref()))) {
                                *v = Proof::Holds;
                            }
                        }
                    }
                }
                let mut adds_here = writes.iter().filter(|(h, sl, e, _, p, k)| h == handler && sl == slot
                    && !matches!(e, Expr::Ident(n) if n == "<deleted entry>") && !key_exists(p, k)).count();
                // a handler reachable from here (a sibling, another cell's
                // handler that calls back, an `emit` listener) that also adds
                // to the slot grows it past the one require (transitively)
                {
                    let me = cell.node.name.clone();
                    let mut seen: HashSet<(String, String)> = HashSet::new();
                    let mut stack: Vec<(String, String)> = vec![(me.clone(), handler.clone())];
                    let mut recursive = false;
                    while let Some((c, h)) = stack.pop() {
                        // reached again: the handler calls itself (directly or
                        // through others) — every nested run adds too
                        // (`w` recursing before its write was "proven")
                        if c == me && h == *handler && !seen.is_empty() { recursive = true; }
                        if !seen.insert((c.clone(), h.clone())) { continue; }
                        let Some((_, on)) = all_handlers.iter().find(|(cn, o)| *cn == c && o.signal_name == h) else { continue };
                        if (c != me || h != *handler) && c == me
                            && writes.iter().any(|(wh, sl, e, _, _, _)| *wh == h && sl == slot && !matches!(e, Expr::Ident(n) if n == "<deleted entry>"))
                        {
                            adds_here += 1;
                        }
                        for (target, name) in calls_of(&on.body, &cell_names, cell_tools.get(&c).map(|v| v.as_slice()).unwrap_or(&[]), &cell_handlers) {
                            match target {
                                // a bare call: the calling cell's own handler, else any definer
                                None => {
                                    if all_handlers.iter().any(|(cn, o)| *cn == c && o.signal_name == name) {
                                        stack.push((c.clone(), name));
                                    } else {
                                        for (cn, o) in &all_handlers { if o.signal_name == name { stack.push((cn.clone(), name.clone())); } }
                                    }
                                }
                                // `emit ev` reaches every `on ev` of the program
                                Some(t) if t == "*" => {
                                    for (cn, o) in &all_handlers { if o.signal_name == name { stack.push((cn.clone(), name.clone())); } }
                                }
                                Some(t) => stack.push((t, name)),
                            }
                        }
                    }
                    if recursive { adds_here += 1; }
                }
                // a loop body, or a lambda (`xs |> map(i => rows.push(i))`) may
                // run the write many times
                let in_loop = wpath.iter().any(|step| step % 4 == 2 || *step == usize::MAX / 2);
                for (c, v) in parts.iter().zip(verdicts.iter_mut()) {
                    // a [task] handler that thinks: the size read by the require may be stale
                    if *v == Proof::Holds || adds_here != 1 || in_loop || task_with_think { continue; }
                    let Some(k) = size_upper_bound(c, slot) else { continue };
                    let before = [format!("{}.size", slot), format!("{}.len", slot), format!("len({})", slot), format!("size({})", slot)]
                        .iter()
                        .filter_map(|r| ctx.vars.get(&format!("__expr__{}", r)).copied().and_then(bounds))
                        .map(|(_, hi)| hi)
                        .fold(f64::INFINITY, f64::min);
                    if before + 1.0 <= k { *v = Proof::Holds; }
                }
                // a Float slot: NaN (sqrt(-1.0), 0.0 / 0.0, inf - inf) fails
                // every comparison, and no interval says a value is not NaN —
                // `x >= 0.0` "proven" for `abs(v)` rejected abs(sqrt(-1.0))
                let float_slot = cell.node.sections.iter().any(|s| match &s.node {
                    Section::Memory(m) => m.slots.iter().any(|sl| sl.node.name == *slot && type_mentions_float(&sl.node.ty.node)),
                    _ => false,
                });
                let mut nan_open = false;
                if float_slot && !matches!(known, Known::Exact(_)) && !is_int_valued(value_expr) && !nan_free_float(value_expr, slot)
                    && !ctx.vars.contains_key(&format!("__cmp__{}", render_expr(value_expr))) {
                    for (c, v) in parts.iter().zip(verdicts.iter_mut()) {
                        if *v == Proof::Holds && size_upper_bound(c, slot).is_none() { *v = Proof::Unknown; nan_open = true; }
                    }
                }
                let how = if inductive { "proven by induction" } else { "proven" };

                if verdicts.iter().any(|v| *v == Proof::Violated) && *in_try {
                    // Always rejected AND caught: the author is showing that
                    // this write is unrepresentable. That is a result, not a bug.
                    result.checks.push(VerifyCheck::Pass(format!(
                        "invariant {inv_text} — writer '{handler}' can never commit {} to '{slot}': the write is \
                         always rejected (and the handler catches it)",
                        render_expr(value_expr)
                    )));
                } else if verdicts.iter().any(|v| *v == Proof::Violated) {
                    result.checks.push(VerifyCheck::Fail(
                        format!(
                            "invariant {inv_text} — writer '{handler}' writes {} to '{slot}': statically violated",
                            render_expr(value_expr)
                        ),
                        None,
                    ));
                } else if verdicts.iter().all(|v| *v == Proof::Holds) {
                    result.checks.push(VerifyCheck::Pass(format!(
                        "invariant {inv_text} — writer '{handler}' {how} (writes {})",
                        render_expr(value_expr)
                    )));
                } else {
                    // part proven, part not: say exactly which
                    for (c, v) in parts.iter().zip(&verdicts) {
                        if *v == Proof::Holds {
                            result.checks.push(VerifyCheck::Pass(format!(
                                "invariant {} — writer '{handler}' {how} (writes {})",
                                render_expr(c),
                                render_expr(value_expr)
                            )));
                        }
                    }
                    let open: Vec<String> = parts
                        .iter()
                        .zip(&verdicts)
                        .filter(|(_, v)| **v != Proof::Holds)
                        .map(|(c, _)| render_expr(c))
                        .collect();
                    // say WHY: the names in the written value the prover
                    // could not bound, and where each comes from
                    let mut names: HashSet<String> = HashSet::new();
                    collect_idents(value_expr, &mut names);
                    let on = handlers.get(handler);
                    let mut why: Vec<String> = names.iter().filter(|n| bounds(ctx.range_of(&Expr::Ident((*n).clone()))).is_none()).map(|n| {
                        let params: Vec<&str> = on.map(|o| o.params.iter().map(|p| p.name.as_str()).collect()).unwrap_or_default();
                        if params.contains(&n.as_str()) {
                            let numeric = on.map(|o| o.params.iter().any(|p| &p.name == n && matches!(&p.ty.node, TypeExpr::Simple(t) if t == "Int" || t == "Float" || t == "BigInt"))).unwrap_or(false);
                            if !numeric { return String::new(); }
                            // the bound to require is the one the open clause needs
                            let open_txt: Vec<String> = parts.iter().zip(&verdicts)
                                .filter(|(_, v)| **v != Proof::Holds)
                                // the clause over the WRITTEN expression: `require room - amount >= 0`
                                // is exactly the fact that proves `room - amount` (a bound on the
                                // parameter alone was the wrong advice for an upper bound)
                                .map(|(c, _)| subst_render(c, slot, value_expr, wkey.as_deref())).collect();
                            if matches!(value_expr, Expr::FnCall { name, .. } if name == "map" || name == "with") {
                                // a field of a built map: a require over it proves nothing
                                return format!("`{n}` is a parameter and the invariant reads a field of a Map value — record-field invariants are checked at run time");
                            }
                            if !handler_writable(&open_txt.join(" && ")) {
                                return format!("`{n}` is a parameter and the open clause reads the slot's size or key — no require in the handler can name those; it is checked at run time");
                            }
                            if open_txt.iter().all(|t| ctx.vars.contains_key(&format!("__req__{}", t))) {
                                return format!("`{n}` is a parameter; the matching `require {}` is there, but it proves the write only when it is the handler's one write to the slot, outside loops, with nothing between that could write the slot (put the require and the write in a handler of their own)", open_txt.join(" && "));
                            }
                            format!("`{n}` is a parameter (narrow it: `require {} else …`)", open_txt.join(" && "))
                        } else {
                            let mut assigns: Vec<(&str, &Expr)> = Vec::new();
                            if let Some(o) = on { collect_assigns(&o.body, &mut assigns); }
                            let bound: Vec<&&Expr> = assigns.iter().filter(|(m, _)| *m == n.as_str()).map(|(_, e)| e).collect();
                            match bound.as_slice() {
                                [] => format!("`{n}` has no known range"),
                                [one] => match one {
                                    // `??` is shown as written (it leaked the internal `_coalesce()`)
                                    Expr::FnCall { name: f, .. } if f == "_coalesce" => format!("`{n}` = {} — not bounded (give both sides of `??` a bound)", render_expr(one)),
                                    Expr::FnCall { name: f, .. } if !handlers.contains_key(f) => format!("`{n}` = {}() — a builtin the prover does not bound", f),
                                    Expr::MethodCall { .. } | Expr::Index { .. } => format!("`{n}` reads a slot with no interval invariant"),
                                    // a loop / lambda / match binding: no single
                                    // expression (it rendered as "`e` = ()")
                                    _ if std::ptr::eq(**one, &UNKNOWN_EXPR) => format!("`{n}` is bound by a loop or a pattern — its values (and their fields) are not bounded; `require` a bound on the value you write"),
                                    _ => format!("`{n}` = {} — not bounded", render_expr(one)),
                                },
                                _ => format!("`{n}` gets {} values (its binding and its later assignments — a loop accumulator?) — bind it once, or read the slot", bound.len()),
                            }
                        }
                    }).collect();
                    why.retain(|w| !w.is_empty());
                    why.sort();
                    // every name has SOME range, just not a tight enough one:
                    // say what is known and the require that would prove it
                    // a built map's field cannot be narrowed by a require
                    // (the suggested `require map(…).taken <= …` changed nothing)
                    let built = matches!(value_expr, Expr::FnCall { name, .. } if name == "map" || name == "with");
                    if why.is_empty() && !built {
                        if let Some((lo, hi)) = bounds(ctx.range_of(value_expr)) {
                            let num = |x: f64| if x == f64::INFINITY { "∞".to_string() } else if x == f64::NEG_INFINITY { "-∞".to_string() } else if x == 0.0 { "0".to_string() } else { format!("{}", x) };
                            let open_txt: Vec<String> = parts.iter().zip(&verdicts)
                                .filter(|(_, v)| **v != Proof::Holds)
                                .map(|(c, _)| subst_render(c, slot, value_expr, wkey.as_deref())).collect();
                            let (l, r) = (if lo.is_finite() { "[" } else { "(" }, if hi.is_finite() { "]" } else { ")" });
                            if !open_txt.is_empty() && open_txt.iter().all(|t| ctx.vars.contains_key(&format!("__req__{}", t))) {
                                why.push(format!("the matching `require {}` is there, but it proves the write only when it is the handler's one write to the slot, outside loops, with nothing between that could write the slot and a pure written value (put the require and the write in a handler of their own)", open_txt.join(" && ")));
                            } else if handler_writable(&open_txt.join(" && ")) {
                                why.push(format!("`{}` is only known to lie in {}{}, {}{} — narrow it: `require {} else …`",
                                    render_expr(value_expr), l, num(lo), num(hi), r, open_txt.join(" && ")));
                            } else {
                                why.push(format!("`{}` is only known to lie in {}{}, {}{} (the open clause reads the slot's size or key — checked at run time)",
                                    render_expr(value_expr), l, num(lo), num(hi), r));
                            }
                        }
                    }
                    if may_be_unit(value_expr) {
                        why = vec![format!("`{}` may be () (a missing key), which no invariant holds of — write `{} ?? <default>`", render_expr(value_expr), render_expr(value_expr))];
                    }
                    if nan_open {
                        why = vec![format!("'{}' holds Floats and `{}` may be NaN (sqrt(-1.0), 0.0 / 0.0, inf - inf), which fails every comparison — the write is checked at run time", slot, render_expr(value_expr))];
                    }
                    let tag = if open.len() == parts.len() {
                        format!("{handler} → {slot}")
                    } else {
                        format!("{handler} → {slot} [{}]", open.join(" && "))
                    };
                    // a `size` clause does not depend on the written value
                    let open_size: Vec<f64> = parts.iter().zip(&verdicts)
                        .filter(|(_, v)| **v != Proof::Holds)
                        .filter_map(|(c, _)| size_upper_bound(c, slot))
                        .collect();
                    let all_size = !open_size.is_empty() && open_size.len() == parts.iter().zip(&verdicts).filter(|(_, v)| **v != Proof::Holds).count();
                    if all_size && task_with_think {
                        why = vec![format!("'{}' is a [task] handler: the prover carries no fact across its think() (other requests may add to '{}' meanwhile), so this write stays checked at run time — keep a `require len({}) < {}` AFTER the last think(), just before the write, so the refusal is a clean error", handler, slot, slot, open_size[0])];
                    } else if all_size {
                        why = vec![format!("the slot may grow past {} — put `require len({}) < {}` before the handler's one write that adds to it (not in a loop), or, to update an entry that exists, `require {}.get(k) != ()` with `k` a plain local (`let k = r.id`, not `r.id`)", open_size[0], slot, open_size[0], slot)];
                    }
                    let tag = if why.is_empty() { tag } else { format!("{tag} because {}", why.join("; ")) };
                    if !runtime_checked.contains(&tag) {
                        runtime_checked.push(tag);
                    }
                }
            }
            if !runtime_checked.is_empty() {
                // a rule BETWEEN slots is enforced on every write to either
                // slot and never proven by induction — by design, so it is a
                // note, not a ⚠ that `--strict` fails on (and no `require`
                // suggestion: in a handler the slot name is the whole Map,
                // so the suggested line could not even evaluate)
                let cross = {
                    let all: Vec<String> = cell.node.sections.iter().filter_map(|sec| match &sec.node {
                        Section::Memory(m) => Some(m.slots.iter().map(|sl| sl.node.name.clone()).collect::<Vec<_>>()), _ => None })
                        .flatten().collect();
                    let mut names: HashSet<String> = HashSet::new();
                    collect_idents(inv, &mut names);
                    all.iter().filter(|n| names.contains(n.as_str())).count() > 1
                };
                if cross {
                    let mut writers: Vec<String> = runtime_checked.iter()
                        .map(|t| t.split(" because ").next().unwrap_or(t).split(" (").next().unwrap_or(t).trim().to_string())
                        .collect();
                    writers.sort(); writers.dedup();
                    result.checks.push(VerifyCheck::Note(format!(
                        "invariant {inv_text} — a rule BETWEEN slots: enforced on every write to either ({}) and on deletes, never proven by induction (by design; `--strict` passes)",
                        writers.join(", ")
                    )));
                } else {
                    result.checks.push(VerifyCheck::Warning(format!(
                        "invariant {inv_text} — runtime-checked (computed values): {}",
                        runtime_checked.join(", ")
                    )));
                }
            }
        }

        results.push(result);
    }

    results
}

// ── Interval reasoning ──────────────────────────────────────────────

/// Split `a && b && c` into its conjuncts.
fn conjuncts(inv: &Expr) -> Vec<&Expr> {
    match inv {
        Expr::BinaryOp { left, op: BinOp::And, right } => {
            let mut v = conjuncts(&left.node);
            v.extend(conjuncts(&right.node));
            v
        }
        other => vec![other],
    }
}

/// The interval a single comparison allows for `slot` (closed superset:
/// `x > 5` contributes [5, ∞) — wider is sound for a hypothesis).
fn interval_of(cmp: &Expr, slot: &str) -> Option<(f64, f64)> {
    let Expr::CmpOp { left, op, right } = cmp else { return None };
    let (subject_is_abs, op, bound) = match (subject_of(&left.node, slot), const_of(&right.node)) {
        (Some(s), Some(c)) => (s, *op, c),
        _ => match (const_of(&left.node), subject_of(&right.node, slot)) {
            (Some(c), Some(s)) => (s, flip(*op), c),
            _ => return None,
        },
    };
    let inf = f64::INFINITY;
    Some(match (subject_is_abs, op) {
        (false, CmpOp::Ge | CmpOp::Gt) => (bound, inf),
        (false, CmpOp::Le | CmpOp::Lt) => (-inf, bound),
        (false, CmpOp::Eq) => (bound, bound),
        (true, CmpOp::Le | CmpOp::Lt) => (-bound, bound),
        _ => return None,
    })
}

struct RangeCtx<'a> {
    hyp: &'a HashMap<String, (f64, f64)>,
    vars: HashMap<String, Known>,
    handlers: &'a HashMap<String, &'a OnSection>,
    depth: usize,
}

fn bounds(k: Known) -> Option<(f64, f64)> {
    match k {
        Known::Exact(v) => Some((v, v)),
        Known::Range(lo, hi) => Some((lo, hi)),
        Known::Unknown => None,
    }
}

fn mk(lo: f64, hi: f64) -> Known {
    const EXACT: f64 = 9007199254740992.0; // 2^53: beyond it f64 rounds (i64::MAX + 10 == i64::MAX)
    // `>=`: a result that ROUNDED to 2^53 is past it too (`v + 1` after
    // `require v <= 2^53` computed 2^53 and "proved" `<= 2^53`)
    if lo.is_nan() || hi.is_nan() {
        return Known::Unknown;
    }
    // past 2^53 an f64 end may be rounded: widen it soundly (outward) instead
    // of losing the whole range — `?? 9223372036854775807` keeps its `>= 0`
    let lo = if lo.is_finite() && lo.abs() >= EXACT { if lo > 0.0 { EXACT - 1.0 } else { f64::NEG_INFINITY } } else { lo };
    let hi = if hi.is_finite() && hi.abs() >= EXACT { if hi < 0.0 { -(EXACT - 1.0) } else { f64::INFINITY } } else { hi };
    if lo.is_infinite() && hi.is_infinite() {
        Known::Unknown
    } else if lo == hi {
        Known::Exact(lo)
    } else {
        Known::Range(lo, hi)
    }
}

fn join(a: Known, b: Known) -> Known {
    match (bounds(a), bounds(b)) {
        (Some((al, ah)), Some((bl, bh))) => mk(al.min(bl), ah.max(bh)),
        _ => Known::Unknown,
    }
}

impl RangeCtx<'_> {
    /// A read of a guarded slot: `slot.get(k)` or `slot[k]`.
    fn slot_read(&self, expr: &Expr) -> Option<Known> {
        let slot = match expr {
            Expr::MethodCall { target, method, .. } if method == "get" => match &target.node {
                Expr::Ident(n) => n,
                _ => return None,
            },
            Expr::Index { target, .. } => match &target.node {
                Expr::Ident(n) => n,
                _ => return None,
            },
            _ => return None,
        };
        let (lo, hi) = self.hyp.get(slot)?;
        if lo.is_infinite() && hi.is_infinite() {
            return None;
        }
        Some(mk(*lo, *hi))
    }

    fn range_of(&self, expr: &Expr) -> Known {
        if let Some(k) = self.slot_read(expr) {
            return k;
        }
        let computed = self.range_of_inner(expr);
        // a `require` on this exact expression narrows it
        if matches!(expr, Expr::BinaryOp { .. } | Expr::FnCall { .. }) {
            if let Some(fact) = self.vars.get(&format!("__expr__{}", render_expr(expr))).copied().and_then(bounds) {
                return match bounds(computed) {
                    Some((l, h)) => mk(l.max(fact.0), h.min(fact.1)),
                    None => mk(fact.0, fact.1),
                };
            }
        }
        computed
    }

    fn range_of_inner(&self, expr: &Expr) -> Known {
        match expr {
            // `if p == "pro" { 50000 } else { 1000 }`: either arm
            Expr::IfExpr { then_result, else_result, .. } =>
                join(self.range_of(&then_result.node), self.range_of(&else_result.node)),
            // a match whose arms all have bounded results
            Expr::Match { arms, .. } if !arms.is_empty() =>
                arms.iter().map(|a| self.range_of(&a.result.node)).reduce(join).unwrap_or(Known::Unknown),
            Expr::Literal(Literal::Int(n)) => mk(*n as f64, *n as f64),
            Expr::Literal(Literal::Float(f)) => Known::Exact(*f),
            Expr::Ident(n) => self.vars.get(n).copied().unwrap_or(Known::Unknown),
            Expr::BinaryOp { left, op, right } => {
                // `a - b` after `require b <= a`: at least 0, whatever b is
                // (the bounds of `b` alone are often unknown — a parameter)
                if matches!(op, BinOp::Sub) {
                    if let (Expr::Ident(a), Expr::Ident(b)) = (&left.node, &right.node) {
                        if self.vars.contains_key(&format!("__le__{}__{}", b, a))
                            && (bounds(self.range_of(&left.node)).is_none() || bounds(self.range_of(&right.node)).is_none())
                        {
                            return mk(0.0, f64::INFINITY);
                        }
                    }
                }
                // `v * v` is never negative, whatever v is
                let same_var = matches!((&left.node, &right.node), (Expr::Ident(a), Expr::Ident(b)) if a == b);
                if matches!(op, BinOp::Mul) && same_var && bounds(self.range_of(&left.node)).is_none() {
                    return mk(0.0, f64::INFINITY);
                }
                // `x % n` with a literal n: |x % n| < n (and ≥ 0 when x is)
                if matches!(op, BinOp::Mod) {
                    if let Some((nl, nh)) = bounds(self.range_of(&right.node)) {
                        if nl == nh && nl != 0.0 {
                            let n = nl.abs();
                            let lo = match bounds(self.range_of(&left.node)) { Some((l, _)) if l >= 0.0 => 0.0, _ => -(n - 1.0) };
                            return mk(lo, n - 1.0);
                        }
                        let _ = nh;
                    }
                }
                let (Some((al, ah)), Some((bl, bh))) =
                    (bounds(self.range_of(&left.node)), bounds(self.range_of(&right.node)))
                else {
                    return Known::Unknown;
                };
                match op {
                    BinOp::Add => mk(al + bl, ah + bh),
                    BinOp::Sub => {
                        // `require b <= a` earlier: a - b is never negative
                        let nonneg = matches!((&left.node, &right.node), (Expr::Ident(a), Expr::Ident(b))
                            if self.vars.contains_key(&format!("__le__{}__{}", b, a)));
                        let lo = al - bh;
                        mk(if nonneg { lo.max(0.0) } else { lo }, ah - bl)
                    }
                    BinOp::Mul => {
                        let c = [al * bl, al * bh, ah * bl, ah * bh];
                        if c.iter().any(|x| x.is_nan()) {
                            return Known::Unknown; // 0 × ∞
                        }
                        mk(c.iter().cloned().fold(f64::INFINITY, f64::min), c.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
                    }
                    BinOp::Div if bl == bh && bl != 0.0 && al == ah => Known::Exact(al / bl),
                    _ => Known::Unknown,
                }
            }
            Expr::FnCall { name, args } => match (name.as_str(), args.len()) {
                // a handler of the cell named like a builtin IS what runs
                // (`on clamp(x, lo, hi) { return x }` was trusted as clamp)
                (_, _) if self.depth < 3 && self.handlers.contains_key(name) => match self.handlers.get(name) {
                    Some(on) => {
                        let inner = RangeCtx {
                            hyp: self.hyp,
                            vars: local_ranges_at(on, self.hyp, self.handlers, self.depth + 1, &[]),
                            handlers: self.handlers,
                            depth: self.depth + 1,
                        };
                        let mut rets: Vec<&Expr> = Vec::new();
                        collect_return_values(&on.body, &mut rets);
                        if rets.is_empty() {
                            return Known::Unknown;
                        }
                        rets.iter().map(|e| inner.range_of(e)).reduce(join).unwrap_or(Known::Unknown)
                    }
                    None => Known::Unknown,
                },
                (_, _) if self.handlers.contains_key(name) => Known::Unknown,
                // a ?? b: a when present, else b
                ("_coalesce", 2) => join(self.range_of(&args[0].node), self.range_of(&args[1].node)),
                ("clamp", 3) => match (bounds(self.range_of(&args[1].node)), bounds(self.range_of(&args[2].node))) {
                    (Some((lo, _)), Some((_, hi))) if lo <= hi => mk(lo, hi),
                    _ => Known::Unknown,
                },
                ("max", 2) | ("min", 2) => {
                    let (Some((al, ah)), Some((bl, bh))) =
                        (bounds(self.range_of(&args[0].node)), bounds(self.range_of(&args[1].node)))
                    else {
                        // max(x, 0) ≥ 0 even when x is unknown
                        let known = [&args[0], &args[1]].iter().find_map(|a| bounds(self.range_of(&a.node)));
                        return match (name.as_str(), known) {
                            ("max", Some((l, _))) => mk(l, f64::INFINITY),
                            ("min", Some((_, h))) => mk(f64::NEG_INFINITY, h),
                            _ => Known::Unknown,
                        };
                    };
                    if name == "max" { mk(al.max(bl), ah.max(bh)) } else { mk(al.min(bl), ah.min(bh)) }
                }
                ("abs", 1) => match bounds(self.range_of(&args[0].node)) {
                    Some((lo, hi)) => {
                        let low = if lo <= 0.0 && hi >= 0.0 { 0.0 } else { lo.abs().min(hi.abs()) };
                        mk(low, lo.abs().max(hi.abs()))
                    }
                    None => mk(0.0, f64::INFINITY),
                },
                // `len("abc")` / `len([1, 2])`: exactly what it is
                ("len", 1) => match &args[0].node {
                    Expr::Literal(Literal::String(t)) => { let n = t.chars().count() as f64; mk(n, n) }
                    Expr::ListLiteral(items) => { let n = items.len() as f64; mk(n, n) }
                    _ => mk(0.0, f64::INFINITY),
                },
                // `mod(a, n)` / `idiv(a, n)` / `sqrt_int(x)` / `band(x, mask)`
                ("mod", 2) => match bounds(self.range_of(&args[1].node)) {
                    Some((nl, nh)) if nl == nh && nl != 0.0 => {
                        let n = nl.abs();
                        let lo = match bounds(self.range_of(&args[0].node)) { Some((l, _)) if l >= 0.0 => 0.0, _ => -(n - 1.0) };
                        mk(lo, n - 1.0)
                    }
                    _ => Known::Unknown,
                },
                ("idiv", 2) => match (bounds(self.range_of(&args[0].node)), bounds(self.range_of(&args[1].node))) {
                    (Some((al, ah)), Some((bl, _))) if al >= 0.0 && bl >= 1.0 => mk(0.0, ah / bl),
                    _ => Known::Unknown,
                },
                ("sqrt_int", 1) => match bounds(self.range_of(&args[0].node)) {
                    Some((lo, hi)) if lo >= 0.0 => mk(0.0, hi.sqrt().ceil()),
                    _ => mk(0.0, f64::INFINITY),
                },
                ("band", 2) => match (bounds(self.range_of(&args[0].node)), bounds(self.range_of(&args[1].node))) {
                    (Some((al, _)), Some((ml, mh))) if al >= 0.0 && ml == mh && ml >= 0.0 => mk(0.0, ml),
                    _ => Known::Unknown,
                },
                // a sibling handler: `on total() { return counts.get("n") ?? 0 }`,
                // or `_drop_hold(id, sku)` returning a slot read — its
                // parameters are unknown inside, its returns are joined
                _ => Known::Unknown,
            },
            _ => Known::Unknown,
        }
    }
}

/// Did the proof lean on the induction hypothesis — a read of a guarded
/// slot, directly, through a local, or through an accessor handler?
fn uses_slot_read(expr: &Expr, ctx: &RangeCtx) -> bool {
    let mut found = false;
    super::termination::walk_expr(expr, &mut |e| {
        let via_local_or_accessor = match e {
            Expr::Ident(n) => ctx.vars.get(n).is_some_and(|k| bounds(*k).is_some_and(|(l, h)| l != h)),
            Expr::FnCall { name, args } => args.is_empty() && ctx.handlers.contains_key(name),
            _ => false,
        };
        if ctx.slot_read(e).is_some() || via_local_or_accessor {
            found = true;
        }
    });
    found
}

fn collect_return_values<'e>(stmts: &'e [Spanned<Statement>], out: &mut Vec<&'e Expr>) {
    for (i, stmt) in stmts.iter().enumerate() {
        match &stmt.node {
            Statement::Return { value } => out.push(&value.node),
            Statement::ExprStmt { expr } if i + 1 == stmts.len() => out.push(&expr.node),
            Statement::If { then_body, else_body, .. } => {
                collect_return_values(then_body, out);
                collect_return_values(else_body, out);
            }
            Statement::While { body, .. } | Statement::For { body, .. } => collect_return_values(body, out),
            _ => {}
        }
    }
}

/// The slot bounds a handler may use: none for a slot whose name the
/// handler also binds (a match binding, a lambda parameter, a `let` in an
/// expression block) — `c.get(..)` may read that binding, not the slot.
fn hyp_for(on: &OnSection, hyp: &HashMap<String, (f64, f64)>) -> HashMap<String, (f64, f64)> {
    let mut binders: HashSet<String> = HashSet::new();
    collect_binders_stmts(&on.body, &mut binders);
    // a `let` of a slot's name anywhere in the handler hides the slot too
    // (`let b = map("x", 1000)  a.set(k, b.get("x"))` took b's bound)
    super::literals::for_each_stmt_deep(&on.body, &mut |st| {
        if let Statement::Let { name, .. } = st { binders.insert(name.clone()); }
    });
    hyp.iter().filter(|(k, _)| !binders.contains(k.as_str())).map(|(k, v)| (k.clone(), *v)).collect()
}

fn local_ranges(
    on: &OnSection,
    hyp: &HashMap<String, (f64, f64)>,
    handlers: &HashMap<String, &OnSection>,
    write_path: &[usize],
) -> HashMap<String, Known> {
    local_ranges_at(on, hyp, handlers, 0, write_path)
}

/// Range of every local: the join of all its assignments. Three rounds;
/// a bound still moving after that (an accumulator in a loop) is widened
/// to ±∞. Parameters are unknown.
fn local_ranges_at(
    on: &OnSection,
    hyp: &HashMap<String, (f64, f64)>,
    handlers: &HashMap<String, &OnSection>,
    depth: usize,
    write_path: &[usize],
) -> HashMap<String, Known> {
    let mut assigns: Vec<(&str, &Expr)> = Vec::new();
    collect_assigns(&on.body, &mut assigns);
    let params: HashSet<&str> = on.params.iter().map(|p| p.name.as_str()).collect();
    // names whose PARTS are written (`a.x = 500`, `a["x"] = …`, `xs[0] = …`,
    // `a.x += …`): a require over `a.x` says nothing after such a write
    let mut part_written: HashSet<String> = HashSet::new();
    crate::checker::literals::for_each_stmt_deep(&on.body, &mut |st| match st {
        Statement::IndexSet { name, .. } => { part_written.insert(name.split('.').next().unwrap_or(name).to_string()); }
        Statement::Assign { name, .. } if name.contains('.') || name.contains('[') => {
            part_written.insert(name.split(|c| c == '.' || c == '[').next().unwrap_or(name).to_string());
        }
        _ => {}
    });

    let mut vars: HashMap<String, Known> = HashMap::new();
    for round in 0..4 {
        let mut next: HashMap<String, Known> = HashMap::new();
        for (name, value) in &assigns {
            if params.contains(name) {
                next.insert((*name).to_string(), Known::Unknown);
                continue;
            }
            let ctx = RangeCtx { hyp, vars: vars.clone(), handlers, depth };
            let r = ctx.range_of(value);
            let merged = match next.get(*name) {
                Some(prev) => join(*prev, r),
                None => r,
            };
            next.insert((*name).to_string(), merged);
        }
        if round == 3 {
            // widen whatever is still growing
            for (name, k) in next.iter_mut() {
                if let (Some((pl, ph)), Some((l, h))) = (vars.get(name).copied().and_then(bounds), bounds(*k)) {
                    let lo = if l < pl { f64::NEG_INFINITY } else { l };
                    let hi = if h > ph { f64::INFINITY } else { h };
                    *k = mk(lo, hi);
                }
            }
        }
        vars = next;
    }
    // `require open < 3 else …` narrows a local that is bound ONCE (a
    // `let`, never reassigned): the handler is atomic, so a write it makes
    // only commits when every unconditional require held. Two agents
    // wanted `open_count <= 3` proven from exactly this shape.
    let mut single: HashMap<&str, &Expr> = HashMap::new();
    for (name, value) in &assigns {
        if params.contains(name) { continue; }
        single.insert(*name, value);
    }
    // a name bound more than once — by a second `let`, or by a lambda
    // parameter / match-arm pattern that SHADOWS it (`match raw { n -> …
    // used.set(k, n) }` wrote the arm's n, not the narrowed parameter)
    let mut shadowed: HashSet<String> = HashSet::new();
    collect_binders_stmts(&on.body, &mut shadowed);
    let dup: HashSet<&str> = assigns.iter().map(|(n, _)| *n)
        .filter(|n| assigns.iter().filter(|(m, _)| m == n).count() > 1 || shadowed.contains(*n))
        .chain(shadowed.iter().map(|s| s.as_str()))
        .collect();
    // a parameter is bound once too — unless the body reassigns it
    let int_params: HashSet<&str> = on.params.iter()
        .filter(|p| matches!(&p.ty.node, TypeExpr::Simple(t) if t == "Int" || t == "BigInt"))
        .map(|p| p.name.as_str()).collect();
    // this handler's parameters while it is analysed — restored after, as
    // the analysis descends into callees (a callee's `a: Float` made the
    // caller's local `a` non-Int)
    struct RestoreParams(Option<HashSet<String>>);
    impl Drop for RestoreParams {
        fn drop(&mut self) { if let Some(p) = self.0.take() { NON_INT_PARAMS.with(|np| *np.borrow_mut() = p); } }
    }
    let _restore = RestoreParams(Some(NON_INT_PARAMS.with(|np| std::mem::replace(&mut *np.borrow_mut(),
        on.params.iter().filter(|p| !int_params.contains(p.name.as_str())).map(|p| p.name.clone()).collect()))));
    let reassigned_param = |n: &str| params.contains(n) && assigns.iter().any(|(m, _)| *m == n);
    // requires that always run when the handler commits: the top-level
    // ones, and those directly inside a loop body (a per-iteration `let`
    // narrowed by a per-iteration `require` — the handler is atomic, so a
    // require failing in ANY iteration rolls every write back). A loop with
    // break/continue can skip its require: not narrowed.
    let mut collected: Vec<Fact> = Vec::new();
    collect_facts(&on.body, &[], &mut collected);
    // the facts that hold at THIS write: a require in the same block or an
    // enclosing one (on that path it has run, or will — the handler is
    // atomic), plus the NEGATION of an early exit `if n >= 1 { … return … }`
    // for the statements after it (never for writes inside its branch)
    // (left, op, right, scope, negated): a NEGATED comparison (`if x > 10.0
    // { return }` → x <= 10.0) is false for NaN, which fails every
    // comparison — it only narrows values that cannot be NaN
    let mut facts: Vec<(&Expr, CmpOp, &Expr, Option<HashSet<String>>, bool)> = Vec::new();
    for f in &collected {
        if !write_path.starts_with(&f.path) { continue; }
        if let Some(ex) = &f.exclude { if write_path.starts_with(ex) { continue; } }
        // `if fast { bal.set(k, n)  return }  require n <= 10 …`: the write
        // before the require can commit without it
        // an invariant is checked AT the write, not at commit: a require
        // narrows only the writes that come AFTER it (`bal.set(k, n)` then
        // `require n <= 10` — the write is refused before the require runs)
        if matches!(f.stmt.node, Statement::Require { .. })
            && write_path.get(f.path.len()).map_or(true, |s| *s / 4 <= f.index) { continue; }
        // an early exit `if n > 10 { return }` says nothing about a write
        // that comes BEFORE it (that write already happened)
        if f.exclude.is_some() && write_path.get(f.path.len()).map_or(true, |s| *s / 4 <= f.index) { continue; }
        match &f.stmt.node {
            Statement::Require { constraint, .. } => facts.extend(constraint_comparisons(&constraint.node).into_iter().map(|(l, o, r)| (l, o, r, None, false))),
            Statement::If { condition, .. } if f.branch == 1 => facts.extend(positive_comparisons(&condition.node).into_iter().map(|(l, o, r)| (l, o, r, None, false))),
            Statement::If { condition, .. } => facts.extend(negated_comparisons(&condition.node).into_iter().map(|(l, o, r)| (l, o, r, None, true))),
            _ => {}
        }
    }
    {
        {
            for (left, op, right, scope, negated) in facts {
                if negated {
                    // NaN-free: a constant, an Int parameter, or a local whose
                    // range is already known (it came from a bounded source)
                    // an Int expression cannot be NaN: Int parameters, Int
                    // literals, and once-bound locals over them (`let x = amt`
                    // lost the `else` narrowing a parameter got)
                    fn int_expr(e: &Expr, single: &HashMap<&str, &Expr>, int_params: &HashSet<&str>, depth: usize) -> bool {
                        if depth > 8 { return false; }
                        match e {
                            Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::BigInt(_)) => true,
                            Expr::Ident(n) => int_params.contains(n.as_str()) || single.get(n.as_str()).map_or(false, |v| int_expr(v, single, int_params, depth + 1)),
                            Expr::BinaryOp { left, op, right } => matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Mod)
                                && int_expr(&left.node, single, int_params, depth + 1) && int_expr(&right.node, single, int_params, depth + 1),
                            _ => false,
                        }
                    }
                    let nan_free = |e: &Expr, vars: &HashMap<String, Known>| const_of(e).is_some() || int_expr(e, &single, &int_params, 0) || match e {
                        Expr::Ident(n) => vars.get(n.as_str()).copied().and_then(bounds).is_some(),
                        _ => false,
                    };
                    if !nan_free(left, &vars) || !nan_free(right, &vars) { continue; }
                }
                // inside a loop: every name the fact mentions must be a
                // per-iteration local of that loop
                if let Some(scope) = &scope {
                    let mut used = HashSet::new();
                    collect_idents(left, &mut used);
                    collect_idents(right, &mut used);
                    if !used.iter().all(|u| scope.contains(u)) { continue; }
                }
                // the whole comparison, as text: an invariant clause that IS
                // this require (over the written value) holds at the write —
                // only when every name in it is bound once (see the use site)
                if !negated {
                    // every name, Index / field targets included (`xs[0]`:
                    // collect_idents saw none, so the check passed vacuously)
                    let mut used: HashSet<String> = HashSet::new();
                    crate::interpreter::free_names_expr(left, &mut used);
                    crate::interpreter::free_names_expr(right, &mut used);
                    if used.iter().all(|u| !dup.contains(u.as_str()) && !reassigned_param(u) && !part_written.contains(u.as_str())) {
                        let cmp = Expr::CmpOp { left: Box::new(Spanned::new(left.clone(), Span::new(0, 0))), op, right: Box::new(Spanned::new(right.clone(), Span::new(0, 0))) };
                        vars.insert(format!("__req__{}", render_expr(&cmp)), Known::Exact(1.0));
                    }
                }
                // `require b <= a` (two once-bound names): a - b >= 0 — kept
                // as a fact the subtraction rule reads
                if let (Expr::Ident(a), Expr::Ident(b)) = (left, right) {
                    if !dup.contains(a.as_str()) && !dup.contains(b.as_str())
                        && !reassigned_param(a) && !reassigned_param(b)
                    {
                        match op {
                            CmpOp::Le | CmpOp::Lt => { vars.insert(format!("__le__{}__{}", a, b), Known::Exact(1.0)); }
                            CmpOp::Ge | CmpOp::Gt => { vars.insert(format!("__le__{}__{}", b, a), Known::Exact(1.0)); }
                            _ => {}
                        }
                        // `require n <= limit` where limit came out of a
                        // guarded slot (`limits.get(id) ?? 1`, invariant
                        // limits <= 5): n inherits that slot's bound —
                        // one slot's invariant chained into another's proof
                        let narrow = |vars: &mut HashMap<String, Known>, name: &str, lo: f64, hi: f64| {
                            let cur = vars.get(name).copied().unwrap_or(Known::Unknown);
                            let n = match bounds(cur) { Some((cl, ch)) => mk(cl.max(lo), ch.min(hi)), None => mk(lo, hi) };
                            vars.insert(name.to_string(), n);
                        };
                        let ra = vars.get(a.as_str()).copied().and_then(bounds);
                        let rb = vars.get(b.as_str()).copied().and_then(bounds);
                        match op {
                            CmpOp::Le | CmpOp::Lt => {
                                if let Some((_, bh)) = rb { narrow(&mut vars, a, f64::NEG_INFINITY, bh); }
                                if let Some((al, _)) = ra { narrow(&mut vars, b, al, f64::INFINITY); }
                            }
                            CmpOp::Ge | CmpOp::Gt => {
                                if let Some((bl, _)) = rb { narrow(&mut vars, a, bl, f64::INFINITY); }
                                if let Some((_, ah)) = ra { narrow(&mut vars, b, f64::NEG_INFINITY, ah); }
                            }
                            _ => {}
                        }
                    }
                    continue;
                }
                let (name, op, bound) = match (left, const_of(right)) {
                    (Expr::Ident(n), Some(c)) => (n.as_str(), op, c),
                    _ => match (const_of(left), right) {
                        (Some(c), Expr::Ident(n)) => (n.as_str(), flip(op), c),
                        // `require cur + n <= 1000000`: a fact about the whole
                        // expression, matched later by its rendering when the
                        // very same expression is written
                        (_, _) => {
                            let (e, op, c) = match (const_of(right), const_of(left)) {
                                (Some(c), None) => (left, op, c),
                                (None, Some(c)) => (right, flip(op), c),
                                _ => continue,
                            };
                            // every name in it must be bound once (else the
                            // written expression is not the required one)
                            let mut used = HashSet::new();
                            collect_idents(e, &mut used);
                            if used.iter().any(|u| dup.contains(u.as_str()) || reassigned_param(u)) { continue; }
                            let (lo, hi) = match op {
                                CmpOp::Lt => (f64::NEG_INFINITY, if looks_int(e) && c.fract() == 0.0 { c - 1.0 } else { c.next_down() }),
                                CmpOp::Le => (f64::NEG_INFINITY, c),
                                CmpOp::Gt => (if looks_int(e) && c.fract() == 0.0 { c + 1.0 } else { c.next_up() }, f64::INFINITY),
                                CmpOp::Ge => (c, f64::INFINITY),
                                CmpOp::Eq => (c, c),
                                CmpOp::Ne => continue,
                            };
                            let key = format!("__expr__{}", render_expr(e));
                            let cur = vars.get(&key).copied().unwrap_or(Known::Unknown);
                            let narrowed = match bounds(cur) { Some((cl, ch)) => mk(cl.max(lo), ch.min(hi)), None => mk(lo, hi) };
                            vars.insert(key, narrowed);
                            // a comparison that held: not NaN
                            if !negated { vars.insert(format!("__cmp__{}", render_expr(e)), Known::Exact(1.0)); }
                            continue;
                        }
                    },
                };
                if dup.contains(name) || reassigned_param(name) { continue; }
                let integral = bound.fract() == 0.0 && match single.get(name) {
                    Some(value) => looks_int(value),
                    None if params.contains(name) => int_params.contains(name),
                    None => continue,
                };
                let (lo, hi) = match op {
                    CmpOp::Lt => (f64::NEG_INFINITY, if integral { bound - 1.0 } else { bound.next_down() }),
                    CmpOp::Le => (f64::NEG_INFINITY, bound),
                    CmpOp::Gt => (if integral { bound + 1.0 } else { bound.next_up() }, f64::INFINITY),
                    CmpOp::Ge => (bound, f64::INFINITY),
                    CmpOp::Eq => (bound, bound),
                    CmpOp::Ne => continue,
                };
                let cur = vars.get(name).copied().unwrap_or(Known::Unknown);
                let narrowed = match bounds(cur) {
                    Some((cl, ch)) => mk(cl.max(lo), ch.min(hi)),
                    None => mk(lo, hi),
                };
                vars.insert(name.to_string(), narrowed);
                // `require rate > 0.0` held: `rate` is not NaN (NaN fails
                // every comparison) — "may be NaN" looped back to that require
                // (a negated fact — the else of `if x < 0.0` — lets NaN through)
                if !negated { vars.insert(format!("__cmp__{}", name), Known::Exact(1.0)); }
            }
        }
    }
    // one more pass for once-bound locals, now that the facts are known:
    // `let x = if amount > 0 { left - amount } else { left }` needs the
    // `amount <= left` fact the first pass did not have. Both ranges are
    // sound over-approximations of the same value: keep their intersection.
    for (name, value) in &assigns {
        if params.contains(name) || dup.contains(name) { continue; }
        let ctx = RangeCtx { hyp, vars: vars.clone(), handlers, depth };
        let fresh = ctx.range_of(value);
        let merged = match (vars.get(*name).copied().and_then(bounds), bounds(fresh)) {
            (Some((a, b)), Some((c, d))) => mk(a.max(c), b.min(d)),
            (None, Some(_)) => fresh,
            _ => continue,
        };
        vars.insert((*name).to_string(), merged);
    }
    // a name ALSO bound by a lambda / match / nested block may be that
    // binding at the write (`let v = …  xs |> map(v => c.set(k, v))`)
    let mut binders: HashSet<String> = HashSet::new();
    collect_binders_stmts(&on.body, &mut binders);
    for b in &binders { vars.insert(b.clone(), Known::Unknown); }
    vars
}

/// `if a || b { return … }` after which ¬a ∧ ¬b holds: the negated leaves.
/// `&&` cannot be split (¬(a ∧ b) is a disjunction) and yields nothing.
/// The comparisons an `if` condition guarantees inside its branch (the
/// `&&` spine only).
fn positive_comparisons(c: &Expr) -> Vec<(&Expr, CmpOp, &Expr)> {
    match c {
        Expr::CmpOp { left, op, right } => vec![(&left.node, *op, &right.node)],
        Expr::BinaryOp { left, op: BinOp::And, right } => {
            let mut v = positive_comparisons(&left.node);
            v.extend(positive_comparisons(&right.node));
            v
        }
        _ => vec![],
    }
}

fn negated_comparisons(c: &Expr) -> Vec<(&Expr, CmpOp, &Expr)> {
    match c {
        Expr::CmpOp { left, op, right } => {
            let neg = match op {
                CmpOp::Lt => CmpOp::Ge, CmpOp::Ge => CmpOp::Lt,
                CmpOp::Le => CmpOp::Gt, CmpOp::Gt => CmpOp::Le,
                CmpOp::Eq => CmpOp::Ne, CmpOp::Ne => CmpOp::Eq,
            };
            vec![(&left.node, neg, &right.node)]
        }
        Expr::BinaryOp { left, op: BinOp::Or, right } => {
            let mut v = negated_comparisons(&left.node);
            v.extend(negated_comparisons(&right.node));
            v
        }
        _ => vec![],
    }
}

/// Does this block leave the handler on every path (a trailing `return`
/// or a bare `fail(…)`)?
fn exits(stmts: &[Spanned<Statement>]) -> bool {
    match stmts.last().map(|s| &s.node) {
        Some(Statement::Return { .. }) => true,
        Some(Statement::ExprStmt { expr }) => matches!(&expr.node, Expr::FnCall { name, .. } if name == "fail"),
        _ => false,
    }
}

/// A fact and, when it sits inside a loop body, the names `let`-bound in
/// that body: a loop can run ZERO times, so its `require` says nothing
/// about a parameter or an outer local — only about the per-iteration
/// locals that every write in the same iteration reads.
/// A `require` (or an early-exit `if`) and the block path it holds on.
struct Fact<'e> {
    stmt: &'e Spanned<Statement>,
    path: Vec<usize>,
    /// the early exit's own branch: the negated condition does not hold there
    exclude: Option<Vec<usize>>,
    /// an `if`'s own branches: 1 = inside `then` (the condition holds),
    /// 2 = inside `else` (its negation holds); 0 = a require / early exit
    branch: u8,
    /// a require: its statement index, and whether a `return` / `break` /
    /// `continue` before it in its block can skip it — then it says nothing
    /// about the writes that come BEFORE it (they may commit without it)
    index: usize,
    exit_before: bool,
}

fn collect_facts<'e>(stmts: &'e [Spanned<Statement>], path: &[usize], out: &mut Vec<Fact<'e>>) {
    let sub = |i: usize, b: usize| -> Vec<usize> { let mut v = path.to_vec(); v.push(block_step(i, b)); v };
    for (i, st) in stmts.iter().enumerate() {
        match &st.node {
            Statement::Require { .. } => out.push(Fact { stmt: st, path: path.to_vec(), exclude: None, branch: 0, index: i, exit_before: stmts[..i].iter().any(|s| can_exit(s)) }),
            Statement::If { then_body, else_body, .. } => {
                if else_body.is_empty() && exits(then_body) {
                    out.push(Fact { stmt: st, path: path.to_vec(), exclude: Some(sub(i, 0)), branch: 0, index: i, exit_before: false });
                }
                // `if n < 5 { c.set(k, n + 1) }`: the condition holds for the
                // writes of its branch, its negation for those of `else`
                out.push(Fact { stmt: st, path: sub(i, 0), exclude: None, branch: 1, index: 0, exit_before: false });
                if !else_body.is_empty() {
                    out.push(Fact { stmt: st, path: sub(i, 1), exclude: None, branch: 2, index: 0, exit_before: false });
                }
                collect_facts(then_body, &sub(i, 0), out);
                collect_facts(else_body, &sub(i, 1), out);
            }
            // a loop body's require holds for the writes of the same body
            // (same iteration) — unless break/continue can skip it
            Statement::For { body, .. } | Statement::While { body, .. } => {
                if !has_break_or_continue(body) { collect_facts(body, &sub(i, 2), out); }
            }
            _ => {}
        }
    }
}

fn has_break_or_continue(stmts: &[Spanned<Statement>]) -> bool {
    stmts.iter().any(|st| match &st.node {
        Statement::Break | Statement::Continue => true,
        Statement::If { then_body, else_body, .. } => has_break_or_continue(then_body) || has_break_or_continue(else_body),
        Statement::For { body, .. } | Statement::While { body, .. } => has_break_or_continue(body),
        _ => false,
    })
}

/// The comparison leaves of a require constraint (only the `&&` spine —
/// an `||` branch constrains nothing on its own).
fn constraint_comparisons(c: &Constraint) -> Vec<(&Expr, CmpOp, &Expr)> {
    // the parser wraps a bare boolean expression as `expr == true`
    fn from_expr(e: &Expr) -> Vec<(&Expr, CmpOp, &Expr)> {
        match e {
            Expr::CmpOp { left, op, right } => vec![(&left.node, *op, &right.node)],
            Expr::BinaryOp { left, op: BinOp::And, right } => {
                let mut v = from_expr(&left.node);
                v.extend(from_expr(&right.node));
                v
            }
            _ => Vec::new(),
        }
    }
    match c {
        Constraint::Comparison { left, op: CmpOp::Eq, right }
            if matches!(right.node, Expr::Literal(Literal::Bool(true))) => from_expr(&left.node),
        Constraint::Comparison { left, op, right } => vec![(&left.node, *op, &right.node)],
        Constraint::And(a, b) => {
            let mut v = constraint_comparisons(&a.node);
            v.extend(constraint_comparisons(&b.node));
            v
        }
        _ => Vec::new(),
    }
}

// An expression that cannot produce a fraction: no Float literal, no `/`,
// no float-producing builtin. Used to tighten strict bounds on integers.
thread_local! {
    /// slots of the cell being verified: name → its values are Ints
    static SLOT_INT: std::cell::RefCell<HashMap<String, bool>> = std::cell::RefCell::new(HashMap::new());
    /// parameters of the handler being analysed that are NOT Int
    static NON_INT_PARAMS: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new());
}

fn looks_int(expr: &Expr) -> bool {
    // a read of an untyped / Any / Float slot, or a Float parameter, is not
    // an Int: `require c < 10` does not make `c + 1 <= 10` (9.5 + 1)
    let slot_read_int = |target: &Expr| -> Option<bool> {
        let Expr::Ident(n) = target else { return None };
        SLOT_INT.with(|m| m.borrow().get(n).copied())
    };
    match expr {
        Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::BigInt(_)) => true,
        Expr::Literal(Literal::Float(_)) => false,
        Expr::Literal(_) => true,
        Expr::Ident(n) if NON_INT_PARAMS.with(|p| p.borrow().contains(n)) => false,
        Expr::MethodCall { target, method, .. } if method == "get" && slot_read_int(&target.node).is_some() => slot_read_int(&target.node).unwrap_or(false),
        Expr::Index { target, .. } if slot_read_int(&target.node).is_some() => slot_read_int(&target.node).unwrap_or(false),
        Expr::Ident(_) => true,
        Expr::BinaryOp { left, op, right } => !matches!(op, BinOp::Div) && looks_int(&left.node) && looks_int(&right.node),
        Expr::FnCall { name, args } => !matches!(name.as_str(), "to_float" | "avg" | "sqrt" | "pow" | "log" | "exp" | "sin" | "cos" | "quantile" | "median" | "pstdev" | "variance")
            && args.iter().all(|a| looks_int(&a.node)),
        Expr::MethodCall { args, .. } => args.iter().all(|a| looks_int(&a.node)),
        Expr::FieldAccess { .. } | Expr::Index { .. } => true,
        _ => true,
    }
}

fn collect_assigns<'e>(stmts: &'e [Spanned<Statement>], out: &mut Vec<(&'e str, &'e Expr)>) {
    for stmt in stmts {
        match &stmt.node {
            Statement::Let { name, value } | Statement::Assign { name, value } => {
                out.push((name.as_str(), &value.node));
                expr_block_assigns(&value.node, out);
            }
            Statement::If { condition, then_body, else_body } => {
                expr_block_assigns(&condition.node, out);
                collect_assigns(then_body, out);
                collect_assigns(else_body, out);
            }
            Statement::While { condition, body, .. } => { expr_block_assigns(&condition.node, out); collect_assigns(body, out) }
            Statement::For { var, iter, body, .. } => {
                expr_block_assigns(&iter.node, out);
                // the loop variable is unknown
                out.push((var.as_str(), &UNKNOWN_EXPR));
                collect_assigns(body, out);
            }
            Statement::Return { value } | Statement::ExprStmt { expr: value } | Statement::Ensure { condition: value } => expr_block_assigns(&value.node, out),
            Statement::IndexSet { index, value, .. } => { expr_block_assigns(&index.node, out); expr_block_assigns(&value.node, out); }
            Statement::MethodCall { args, .. } | Statement::Emit { args, .. } => { for a in args { expr_block_assigns(&a.node, out); } }
            _ => {}
        }
    }
}

/// `v = n` inside a match arm, a `try { }`, an if-expression or a lambda
/// block reassigns the outer `v` (it was invisible: `let v = 5  match n {
/// _ -> { v = n } }  bal.set(k, v)` was "proven" with v = 5)
fn expr_block_assigns<'e>(e: &'e Expr, out: &mut Vec<(&'e str, &'e Expr)>) {
    fn flat<'e>(stmts: &'e [Spanned<Statement>], out: &mut Vec<(&'e str, &'e Expr)>) {
        for st in stmts {
            match &st.node {
                Statement::Assign { name, value } => out.push((name.as_str(), &value.node)),
                Statement::If { then_body, else_body, .. } => { flat(then_body, out); flat(else_body, out); }
                Statement::For { body, .. } | Statement::While { body, .. } => flat(body, out),
                _ => {}
            }
        }
    }
    let mut found: Vec<&'e str> = Vec::new();
    crate::checker::literals::for_each_in_expr(e, &mut |x| {
        let mut local: Vec<(&'e str, &'e Expr)> = Vec::new();
        match x {
            Expr::Match { arms, .. } => for a in arms { flat(&a.body, &mut local) },
            Expr::IfExpr { then_body, else_body, .. } => { flat(then_body, &mut local); flat(else_body, &mut local); }
            Expr::LambdaBlock { stmts, .. } => flat(stmts, &mut local),
            _ => {}
        }
        found.extend(local.into_iter().map(|(n, _)| n));
    });
    // the value assigned inside the block is not tracked: unknown
    for n in found { out.push((n, &UNKNOWN_EXPR)); }
}

static UNKNOWN_EXPR: Expr = Expr::Literal(Literal::Unit);

/// Constant-fold a value expression where possible.
fn static_value(expr: &Expr) -> Known {
    match expr {
        Expr::Literal(Literal::Int(n)) => mk(*n as f64, *n as f64),
        Expr::Literal(Literal::Float(f)) => Known::Exact(*f),
        Expr::BinaryOp { left, op, right } => {
            let (Known::Exact(a), Known::Exact(b)) =
                (static_value(&left.node), static_value(&right.node))
            else {
                return Known::Unknown;
            };
            match op {
                BinOp::Add => mk(a + b, a + b),
                BinOp::Sub => mk(a - b, a - b),
                BinOp::Mul => mk(a * b, a * b),
                BinOp::Div if b != 0.0 => mk(a / b, a / b),
                _ => Known::Unknown,
            }
        }
        // clamp(x, lo, hi) with constant bounds → range [lo, hi]
        Expr::FnCall { name, args } if name == "clamp" && args.len() == 3 => {
            let (Known::Exact(lo), Known::Exact(hi)) =
                (static_value(&args[1].node), static_value(&args[2].node))
            else {
                return Known::Unknown;
            };
            if lo <= hi { mk(lo, hi) } else { Known::Unknown }
        }
        _ => Known::Unknown,
    }
}

/// Prove an invariant against a known written value. Supports the
/// shapes that cover real risk limits: comparisons of the slot value
/// (optionally through abs()) against constants, joined with &&.
fn prove(inv: &Expr, slot: &str, value: Known) -> Proof {
    match inv {
        // `n == 0 || n == 5`: holds when either side holds for every value
        // in range (a literal write of 0 was "runtime-checked")
        Expr::BinaryOp { left, op: BinOp::Or, right } => {
            // only over the written value: a side reading `key` (or another
            // name) can RAISE (`key != 0` on a String key) before the other
            // side is evaluated, and the write is refused
            let only_value = |e: &Expr| deep_idents(e).iter().all(|n| n == slot || n == "value");
            if !only_value(&left.node) || !only_value(&right.node) { return Proof::Unknown; }
            let a = prove(&left.node, slot, value);
            let b = prove(&right.node, slot, value);
            if a == Proof::Holds || b == Proof::Holds {
                Proof::Holds
            } else if a == Proof::Violated && b == Proof::Violated {
                Proof::Violated
            } else {
                Proof::Unknown
            }
        }
        Expr::BinaryOp { left, op: BinOp::And, right } => {
            let a = prove(&left.node, slot, value);
            let b = prove(&right.node, slot, value);
            if a == Proof::Violated || b == Proof::Violated {
                Proof::Violated
            } else if a == Proof::Holds && b == Proof::Holds {
                Proof::Holds
            } else {
                Proof::Unknown
            }
        }
        Expr::CmpOp { left, op, right } => {
            // normalize to subject(value) OP constant
            let (subject, op, bound) = match (subject_of(&left.node, slot), const_of(&right.node)) {
                (Some(s), Some(c)) => (s, *op, c),
                _ => match (const_of(&left.node), subject_of(&right.node, slot)) {
                    (Some(c), Some(s)) => (s, flip(*op), c),
                    _ => return Proof::Unknown,
                },
            };
            let range = match value {
                Known::Exact(v) => (v, v),
                Known::Range(lo, hi) => (lo, hi),
                Known::Unknown => return Proof::Unknown,
            };
            // apply abs() to the written-value range if the subject does
            let (lo, hi) = if subject {
                // abs over [lo, hi]
                let alo = if range.0 <= 0.0 && range.1 >= 0.0 {
                    0.0
                } else {
                    range.0.abs().min(range.1.abs())
                };
                (alo, range.0.abs().max(range.1.abs()))
            } else {
                range
            };
            let holds_at = |v: f64| match op {
                CmpOp::Lt => v < bound,
                CmpOp::Gt => v > bound,
                CmpOp::Le => v <= bound,
                CmpOp::Ge => v >= bound,
                CmpOp::Eq => v == bound,
                CmpOp::Ne => v != bound,
            };
            // monotone predicates: checking the endpoints decides the range
            let (at_lo, at_hi) = (holds_at(lo), holds_at(hi));
            match (at_lo, at_hi) {
                (true, true) if op != CmpOp::Ne && op != CmpOp::Eq => Proof::Holds,
                (false, false) if op != CmpOp::Ne && op != CmpOp::Eq => Proof::Violated,
                _ if lo == hi => {
                    if at_lo { Proof::Holds } else { Proof::Violated }
                }
                _ => Proof::Unknown,
            }
        }
        _ => Proof::Unknown,
    }
}

/// Is this expression the guarded subject? Returns Some(true) for
/// abs(slot|value), Some(false) for a bare slot|value reference.
fn subject_of(expr: &Expr, slot: &str) -> Option<bool> {
    match expr {
        Expr::Ident(n) if n == slot || n == "value" => Some(false),
        Expr::FnCall { name, args } if name == "abs" && args.len() == 1 => {
            match &args[0].node {
                Expr::Ident(n) if n == slot || n == "value" => Some(true),
                _ => None,
            }
        }
        _ => None,
    }
}

fn const_of(expr: &Expr) -> Option<f64> {
    match static_value(expr) {
        Known::Exact(v) => Some(v),
        _ => None,
    }
}

fn flip(op: CmpOp) -> CmpOp {
    match op {
        CmpOp::Lt => CmpOp::Gt,
        CmpOp::Gt => CmpOp::Lt,
        CmpOp::Le => CmpOp::Ge,
        CmpOp::Ge => CmpOp::Le,
        other => other,
    }
}

// ── Write-site collection ───────────────────────────────────────────

/// Block paths: a statement's enclosing blocks, each encoded as
/// `index * 4 + branch` (0 = then, 1 = else, 2 = loop body). A `require`
/// at path P is a fact for every write whose path starts with P — the same
/// block or one nested in it: on that path the require has run (or will,
/// and the atomic handler rolls the write back).
pub(crate) fn block_step(index: usize, branch: usize) -> usize { index * 4 + branch }

fn collect_writes_stmts(stmts: &[Spanned<Statement>], handler: &str, in_try: bool, out: &mut Vec<(String, String, Expr, bool, Vec<usize>, Option<String>)>) {
    collect_writes_stmts_at(stmts, handler, in_try, &[], out)
}

fn collect_writes_stmts_at(stmts: &[Spanned<Statement>], handler: &str, in_try: bool, path: &[usize], out: &mut Vec<(String, String, Expr, bool, Vec<usize>, Option<String>)>) {
    let sub = |i: usize, b: usize| -> Vec<usize> { let mut v = path.to_vec(); v.push(block_step(i, b)); v };
    for (i, stmt) in stmts.iter().enumerate() {
        match &stmt.node {
            Statement::Let { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Return { value } => collect_writes_expr(&value.node, handler, in_try, &sub(i, 3), out),
            Statement::ExprStmt { expr } => collect_writes_expr(&expr.node, handler, in_try, &sub(i, 3), out),
            Statement::If { condition, then_body, else_body } => {
                collect_writes_expr(&condition.node, handler, in_try, &sub(i, 3), out);
                collect_writes_stmts_at(then_body, handler, in_try, &sub(i, 0), out);
                collect_writes_stmts_at(else_body, handler, in_try, &sub(i, 1), out);
            }
            Statement::While { condition, body, .. } => {
                collect_writes_expr(&condition.node, handler, in_try, path, out);
                collect_writes_stmts_at(body, handler, in_try, &sub(i, 2), out);
            }
            Statement::For { iter, body, .. } => {
                collect_writes_expr(&iter.node, handler, in_try, path, out);
                collect_writes_stmts_at(body, handler, in_try, &sub(i, 2), out);
            }
            // `slot[k] = v` — same write path as slot.set(k, v) at runtime.
            // A local of the same name is filtered out later (only guarded
            // slot names are kept).
            Statement::IndexSet { name, index, value } => {
                out.push((handler.to_string(), name.clone(), value.node.clone(), in_try, sub(i, 3), Some(render_expr(&index.node))));
                collect_writes_expr(&index.node, handler, in_try, &sub(i, 3), out);
                collect_writes_expr(&value.node, handler, in_try, &sub(i, 3), out);
            }
            Statement::MethodCall { target, method, args } => {
                push_slot_write(target, method, args, handler, in_try, &sub(i, 3), out);
                for a in args {
                    collect_writes_expr(&a.node, handler, in_try, &sub(i, 3), out);
                }
            }
            Statement::Emit { args, .. } => {
                for a in args {
                    collect_writes_expr(&a.node, handler, in_try, &sub(i, 3), out);
                }
            }
            Statement::Ensure { condition } => collect_writes_expr(&condition.node, handler, in_try, &sub(i, 3), out),
            _ => {}
        }
    }
}

/// Record a slot mutation. set/put and push/append write a known value
/// expression; delete/remove changes `size`, which is never statically
/// known — it is recorded with an opaque value so the invariant is
/// reported as runtime-checked rather than "no handler writes".
fn push_slot_write(
    slot: &str,
    method: &str,
    args: &[Spanned<Expr>],
    handler: &str,
    in_try: bool,
    path: &[usize],
    out: &mut Vec<(String, String, Expr, bool, Vec<usize>, Option<String>)>,
) {
    match method {
        "set" | "put" if args.len() >= 2 => {
            out.push((handler.to_string(), slot.to_string(), args[1].node.clone(), in_try, path.to_vec(), Some(render_expr(&args[0].node))));
        }
        "push" | "append" if !args.is_empty() => {
            out.push((handler.to_string(), slot.to_string(), args[0].node.clone(), in_try, path.to_vec(), None));
        }
        "delete" | "remove" => {
            out.push((handler.to_string(), slot.to_string(), Expr::Ident("<deleted entry>".to_string()), in_try, path.to_vec(), None));
        }
        _ => {}
    }
}

fn collect_writes_expr(expr: &Expr, handler: &str, in_try: bool, path: &[usize], out: &mut Vec<(String, String, Expr, bool, Vec<usize>, Option<String>)>) {
    match expr {
        Expr::MethodCall { target, method, args } => {
            if let Expr::Ident(slot) = &target.node {
                push_slot_write(slot, method, args, handler, in_try, path, out);
            }
            collect_writes_expr(&target.node, handler, in_try, path, out);
            for a in args {
                collect_writes_expr(&a.node, handler, in_try, path, out);
            }
        }
        Expr::FnCall { args, .. } => {
            for a in args {
                collect_writes_expr(&a.node, handler, in_try, path, out);
            }
        }
        Expr::FieldAccess { target, .. } => collect_writes_expr(&target.node, handler, in_try, path, out),
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            collect_writes_expr(&left.node, handler, in_try, path, out);
            collect_writes_expr(&right.node, handler, in_try, path, out);
        }
        Expr::Try(i) => collect_writes_expr(&i.node, handler, true, path, out),
        Expr::Not(i) | Expr::TryPropagate(i) => collect_writes_expr(&i.node, handler, in_try, path, out),
        Expr::ListLiteral(items) => {
            for i in items {
                collect_writes_expr(&i.node, handler, in_try, path, out);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                collect_writes_expr(&v.node, handler, in_try, path, out);
            }
        }
        // a lambda may run many times (`xs |> map(i => rows.push(i))`)
        Expr::Lambda { body, .. } => collect_writes_expr(&body.node, handler, in_try, &{ let mut v = path.to_vec(); v.push(usize::MAX / 2); v }, out),
        Expr::LambdaBlock { stmts, result, .. } => {
            let lam = { let mut v = path.to_vec(); v.push(usize::MAX / 2); v };
            collect_writes_stmts_at(stmts, handler, in_try, &lam, out);
            collect_writes_expr(&result.node, handler, in_try, &lam, out);
        }
        Expr::Match { subject, arms } => {
            collect_writes_expr(&subject.node, handler, in_try, path, out);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_writes_expr(&g.node, handler, in_try, path, out);
                }
                collect_writes_stmts_at(&arm.body, handler, in_try, &{ let mut v = path.to_vec(); v.push(usize::MAX / 2); v }, out);
                collect_writes_expr(&arm.result.node, handler, in_try, path, out);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            collect_writes_expr(&condition.node, handler, in_try, path, out);
            collect_writes_stmts_at(then_body, handler, in_try, &{ let mut v = path.to_vec(); v.push(usize::MAX / 2); v }, out);
            collect_writes_expr(&then_result.node, handler, in_try, path, out);
            collect_writes_stmts_at(else_body, handler, in_try, &{ let mut v = path.to_vec(); v.push(usize::MAX / 2); v }, out);
            collect_writes_expr(&else_result.node, handler, in_try, path, out);
        }
        _ => {}
    }
}

// ── Small AST utilities ─────────────────────────────────────────────

fn collect_idents(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::Ident(n) => {
            out.insert(n.clone());
        }
        Expr::FieldAccess { target, .. } => collect_idents(&target.node, out),
        Expr::MethodCall { target, args, .. } => {
            collect_idents(&target.node, out);
            for a in args {
                collect_idents(&a.node, out);
            }
        }
        Expr::FnCall { args, .. } => {
            for a in args {
                collect_idents(&a.node, out);
            }
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            collect_idents(&left.node, out);
            collect_idents(&right.node, out);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => collect_idents(&i.node, out),
        Expr::ListLiteral(items) => {
            for i in items {
                collect_idents(&i.node, out);
            }
        }
        _ => {}
    }
}

fn collect_fn_names(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::FnCall { name, args } => {
            out.insert(name.clone());
            for a in args {
                collect_fn_names(&a.node, out);
            }
        }
        Expr::FieldAccess { target, .. } => collect_fn_names(&target.node, out),
        Expr::MethodCall { target, args, .. } => {
            collect_fn_names(&target.node, out);
            for a in args {
                collect_fn_names(&a.node, out);
            }
        }
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            collect_fn_names(&left.node, out);
            collect_fn_names(&right.node, out);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => collect_fn_names(&i.node, out),
        Expr::ListLiteral(items) => {
            for i in items {
                collect_fn_names(&i.node, out);
            }
        }
        _ => {}
    }
}

// ── Per-slot entry count: `slot.size` ───────────────────────────────

/// Rewrite `slot.size` / `slot.len` / `slot.count` (and their `()` method
/// forms) to the generic `size` binding. Scoping must be computed on the
/// ORIGINAL expression, which still names the slot — that is what makes
/// `invariant links.size <= 1000` bound `links` only, not every slot of the
/// section the way a bare `size` does.
pub fn normalize_invariant(expr: &Expr, slots: &[String]) -> Expr {
    let is_count = |f: &str| matches!(f, "size" | "len" | "count" | "length");
    let is_slot = |e: &Expr| matches!(e, Expr::Ident(n) if slots.iter().any(|s| s == n));
    let sub = |e: &Spanned<Expr>| Box::new(Spanned::new(normalize_invariant(&e.node, slots), e.span));
    match expr {
        Expr::FieldAccess { target, field } if is_count(field) && is_slot(&target.node) => {
            Expr::Ident("size".to_string())
        }
        Expr::MethodCall { target, method, args } if args.is_empty() && is_count(method) && is_slot(&target.node) => {
            Expr::Ident("size".to_string())
        }
        Expr::BinaryOp { left, op, right } => Expr::BinaryOp { left: sub(left), op: *op, right: sub(right) },
        Expr::CmpOp { left, op, right } => Expr::CmpOp { left: sub(left), op: *op, right: sub(right) },
        Expr::Not(inner) => Expr::Not(sub(inner)),
        Expr::FnCall { name, args } => Expr::FnCall {
            name: name.clone(),
            args: args.iter().map(|a| Spanned::new(normalize_invariant(&a.node, slots), a.span)).collect(),
        },
        other => other.clone(),
    }
}

/// `invariant len(links) <= 1000` reads like "at most 1000 links" and means
/// "every written VALUE is at most 1000 long". Returns (message, span).
pub fn lint_program(program: &Program) -> Vec<InvariantIssue> {
    let mut out = Vec::new();
    for cell in &program.cells {
        for section in &cell.node.sections {
            let Section::Memory(mem) = &section.node else { continue };
            let slots: Vec<String> = mem.slots.iter().map(|s| s.node.name.clone()).collect();
            for inv in &mem.invariants {
                // a rule BETWEEN slots: the absent side reads as 0, so the
                // first write to the left at a key the right slot does not
                // have yet is refused (an event whose `house` was written
                // before `sellable` never opened)
                {
                    let names = deep_idents(&inv.node);
                    let mut both: Vec<&String> = slots.iter().filter(|n| names.contains(n.as_str())).collect();
                    both.sort();
                    let text = crate::ast::render_expr(&inv.node);
                    // …only when no handler seeds both slots together (a
                    // program that writes the right-hand slot first is fine)
                    let seeds_both = program.cells.iter().filter(|c| c.node.name == cell.node.name).any(|c| c.node.sections.iter().any(|sec| {
                        let Section::OnSignal(on) = &sec.node else { return false };
                        let mut written: HashSet<String> = HashSet::new();
                        crate::checker::literals::for_each_stmt_deep(&on.body, &mut |st| match st {
                            Statement::MethodCall { target, method, .. } if matches!(method.as_str(), "set" | "push" | "put") => { written.insert(target.clone()); }
                            Statement::IndexSet { name, .. } => { written.insert(name.clone()); }
                            _ => {}
                        });
                        crate::checker::literals::for_each_expr(&on.body, &mut |e| if let Expr::MethodCall { target, method, .. } = e {
                            if matches!(method.as_str(), "set" | "push" | "put") {
                                if let Expr::Ident(t) = &target.node { written.insert(t.clone()); }
                            }
                        });
                        both.iter().all(|n| written.contains(n.as_str()))
                    }));
                    if both.len() > 1 && text.contains("??") && !seeds_both {
                        out.push(InvariantIssue {
                            message: format!("invariant between slots ({}) — at a key the other slot has no entry for, `?? 0` makes that side 0: write the right-hand slot first (or seed both in one handler), or the first write to the other is refused",
                                both.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")),
                            span: inv.span,
                        });
                    }
                }
                // an invariant naming no slot guards EVERY slot of the memory
                // (`invariant get_status(key) != "paid"` refused every write
                // to an unrelated counter) — say so when there are several
                let refs = deep_idents(&inv.node);
                if slots.len() > 1 && !slots.iter().any(|sl| refs.contains(sl.as_str())) {
                    out.push(InvariantIssue {
                        message: format!(
                            "`invariant {}` names no slot, so it guards EVERY slot of this memory ({}) — name the slot it is about (e.g. `{}.get(key)` / `len({})`), or move it to a memory of its own",
                            render_expr(&inv.node), slots.join(", "), slots[0], slots[0]),
                        span: inv.span,
                    });
                }
                let mut hits: Vec<(String, String)> = Vec::new();
                find_len_of_slot(&inv.node, &slots, &mut hits);
                for (func, slot) in hits {
                    out.push(InvariantIssue {
                        message: format!(
                            "`{func}({slot})` in an invariant measures the VALUE being written to '{slot}', not how \
                             many entries '{slot}' holds. For an entry-count bound write `{slot}.size <= N`; if you \
                             do mean the length of each value written to '{slot}', keep `{func}({slot})` (a bare \
                             `{func}(value)` names no slot, so it would bound the values of EVERY slot of this memory)"
                        ),
                        span: inv.span,
                    });
                }
            }
        }
    }
    out
}

fn find_len_of_slot(expr: &Expr, slots: &[String], out: &mut Vec<(String, String)>) {
    match expr {
        Expr::FnCall { name, args } => {
            if matches!(name.as_str(), "len" | "size" | "count") && args.len() == 1 {
                if let Expr::Ident(n) = &args[0].node {
                    if slots.iter().any(|s| s == n) {
                        out.push((name.clone(), n.clone()));
                    }
                }
            }
            for a in args {
                find_len_of_slot(&a.node, slots, out);
            }
        }
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
            find_len_of_slot(&left.node, slots, out);
            find_len_of_slot(&right.node, slots, out);
        }
        Expr::Not(i) => find_len_of_slot(&i.node, slots, out),
        _ => {}
    }
}

/// Names introduced by lambda parameters and match-arm patterns anywhere in
/// a body (they shadow, so a narrowed outer name of the same spelling is
/// not what a write inside reads).
fn count_for_vars(stmts: &[Spanned<Statement>], out: &mut HashMap<String, usize>) {
    for st in stmts {
        match &st.node {
            Statement::For { var, body, .. } => { *out.entry(var.clone()).or_default() += 1; count_for_vars(body, out); }
            Statement::While { body, .. } => count_for_vars(body, out),
            Statement::If { then_body, else_body, .. } => { count_for_vars(then_body, out); count_for_vars(else_body, out); }
            _ => {}
        }
    }
}

/// Binders other than `for` variables: lambda parameters, match bindings
/// (anywhere, including inside loop iterators and bodies).
fn collect_non_for_binders(stmts: &[Spanned<Statement>], out: &mut HashSet<String>) {
    for st in stmts {
        match &st.node {
            Statement::For { iter, body, .. } => { collect_binders_expr(&iter.node, out); collect_non_for_binders(body, out); }
            Statement::While { condition, body, .. } => { collect_binders_expr(&condition.node, out); collect_non_for_binders(body, out); }
            Statement::If { condition, then_body, else_body } => {
                collect_binders_expr(&condition.node, out); collect_non_for_binders(then_body, out); collect_non_for_binders(else_body, out);
            }
            _ => collect_binders_stmts(std::slice::from_ref(st), out),
        }
    }
}

fn collect_binders_stmts(stmts: &[Spanned<Statement>], out: &mut HashSet<String>) {
    for st in stmts {
        match &st.node {
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value }
            | Statement::Ensure { condition: value } | Statement::ExprStmt { expr: value } => collect_binders_expr(&value.node, out),
            Statement::If { condition, then_body, else_body } => {
                collect_binders_expr(&condition.node, out); collect_binders_stmts(then_body, out); collect_binders_stmts(else_body, out);
            }
            Statement::For { var, iter, body, .. } => { out.insert(var.clone()); collect_binders_expr(&iter.node, out); collect_binders_stmts(body, out); }
            Statement::While { condition, body, .. } => { collect_binders_expr(&condition.node, out); collect_binders_stmts(body, out); }
            Statement::Emit { args, .. } | Statement::MethodCall { args, .. } => { for a in args { collect_binders_expr(&a.node, out); } }
            Statement::IndexSet { index, value, .. } => { collect_binders_expr(&index.node, out); collect_binders_expr(&value.node, out); }
            _ => {}
        }
    }
}

/// `let`s of a block nested INSIDE an expression (an if-expression, a match
/// arm, a lambda block): they shadow outer names — a slot included — for
/// the rest of that block.
fn nested_lets(stmts: &[Spanned<Statement>], out: &mut HashSet<String>) {
    for st in stmts {
        if let Statement::Let { name, .. } = &st.node { out.insert(name.clone()); }
    }
}

fn collect_binders_expr(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::LambdaBlock { stmts, .. } => nested_lets(stmts, out),
        Expr::Match { arms, .. } => for arm in arms { nested_lets(&arm.body, out) },
        Expr::IfExpr { then_body, else_body, .. } => { nested_lets(then_body, out); nested_lets(else_body, out); }
        _ => {}
    }
    match e {
        Expr::Lambda { param, body } => { out.insert(param.clone()); collect_binders_expr(&body.node, out); }
        Expr::LambdaBlock { param, stmts, result } => { out.insert(param.clone()); collect_binders_stmts(stmts, out); collect_binders_expr(&result.node, out); }
        Expr::Match { subject, arms } => {
            collect_binders_expr(&subject.node, out);
            for arm in arms {
                pattern_binders(&arm.pattern, out);
                if let Some(g) = &arm.guard { collect_binders_expr(&g.node, out); }
                collect_binders_stmts(&arm.body, out);
                collect_binders_expr(&arm.result.node, out);
            }
        }
        Expr::FieldAccess { target, .. } => collect_binders_expr(&target.node, out),
        Expr::Index { target, index } => { collect_binders_expr(&target.node, out); collect_binders_expr(&index.node, out); }
        Expr::MethodCall { target, args, .. } => { collect_binders_expr(&target.node, out); for a in args { collect_binders_expr(&a.node, out); } }
        Expr::FnCall { args, .. } => { for a in args { collect_binders_expr(&a.node, out); } }
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => {
            collect_binders_expr(&left.node, out); collect_binders_expr(&right.node, out);
        }
        Expr::Not(i) | Expr::Try(i) | Expr::TryPropagate(i) => collect_binders_expr(&i.node, out),
        Expr::Record { fields, .. } => { for (_, v) in fields { collect_binders_expr(&v.node, out); } }
        Expr::ListLiteral(items) => { for i in items { collect_binders_expr(&i.node, out); } }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            collect_binders_expr(&condition.node, out);
            collect_binders_stmts(then_body, out); collect_binders_expr(&then_result.node, out);
            collect_binders_stmts(else_body, out); collect_binders_expr(&else_result.node, out);
        }
        _ => {}
    }
}

pub(crate) fn pattern_binders(p: &MatchPattern, out: &mut HashSet<String>) {
    match p {
        MatchPattern::Variable(n) => { out.insert(n.clone()); }
        MatchPattern::Or(ps) => { for q in ps { pattern_binders(q, out); } }
        MatchPattern::MapDestructure(fields) => { for (name, q) in fields { out.insert(name.clone()); pattern_binders(q, out); } }
        MatchPattern::StringPrefix { rest, .. } => { out.insert(rest.clone()); }
        MatchPattern::Variant { fields, .. } => match fields {
            VariantPatternFields::Tuple(ps) => { for q in ps { pattern_binders(q, out); } }
            VariantPatternFields::Struct { fields, .. } => { for (name, q) in fields { out.insert(name.clone()); pattern_binders(q, out); } }
            VariantPatternFields::Unit => {}
        },
        _ => {}
    }
}

/// `size <= K` / `rows.size < K` / `len(rows) <= K` as an invariant clause:
/// the largest size it allows.
fn size_upper_bound(c: &Expr, slot: &str) -> Option<f64> {
    let is_size = |e: &Expr| match e {
        Expr::Ident(n) => n == "size",
        Expr::FieldAccess { target, field } => matches!(&target.node, Expr::Ident(n) if n == slot) && (field == "size" || field == "len"),
        // NOT `len(slot)`: in an invariant the slot's name is the value being
        // written, so `len(rows)` measures that value, not the slot
        _ => false,
    };
    let Expr::CmpOp { left, op, right } = c else { return None };
    let (op, k) = match (is_size(&left.node), const_of(&right.node), const_of(&left.node), is_size(&right.node)) {
        (true, Some(k), _, _) => (*op, k),
        (_, _, Some(k), true) => (flip(*op), k),
        _ => return None,
    };
    match op { CmpOp::Le => Some(k), CmpOp::Lt => Some(k - 1.0), _ => None }
}

fn size_upper_bound_any(parts: &[&Expr], slot: &str) -> bool {
    parts.iter().any(|c| size_upper_bound(c, slot).is_some())
}

/// `slot.get(K) != ()` / `slot.get(K) == ()` / `slot.has(K)` → (K, key is present?)
fn key_presence(e: &Expr, slot: &str, negate: bool, aliases: &HashMap<String, String>) -> Option<(String, bool)> {
    let get_key = |t: &Expr| -> Option<String> {
        match t {
            Expr::MethodCall { target, method, args } if (method == "get" || method == "has") && args.len() == 1
                && matches!(&target.node, Expr::Ident(n) if n == slot) => Some(render_expr(&args[0].node)),
            // `let r = rows.get(k)  require r != ()` (r bound once)
            Expr::Ident(n) => aliases.get(n).cloned(),
            _ => None,
        }
    };
    match e {
        Expr::CmpOp { left, op, right } if matches!(&right.node, Expr::Literal(Literal::Unit)) => {
            let k = get_key(&left.node)?;
            let present = match op { CmpOp::Ne => true, CmpOp::Eq => false, _ => return None };
            Some((k, present != negate))
        }
        Expr::MethodCall { method, .. } if method == "has" => Some((get_key(e)?, !negate)),
        Expr::Not(i) => key_presence(&i.node, slot, !negate, aliases),
        _ => None,
    }
}

/// Where a key of `slot` is known to exist: (block path, key rendering,
/// excluded sub-path).
fn existing_key_facts(stmts: &[Spanned<Statement>], path: &[usize], slot: &str, aliases: &HashMap<String, String>) -> Vec<(Vec<usize>, String, Option<Vec<usize>>, Option<usize>)> {
    let mut out = Vec::new();
    let sub = |i: usize, b: usize| -> Vec<usize> { let mut v = path.to_vec(); v.push(block_step(i, b)); v };
    for (i, st) in stmts.iter().enumerate() {
        match &st.node {
            Statement::Require { constraint, .. } => {
                if let Constraint::Comparison { left, op, right } = &constraint.node {
                    // `require e` is stored as `e == true`
                    let e = if *op == CmpOp::Eq && matches!(&right.node, Expr::Literal(Literal::Bool(true))) {
                        left.node.clone()
                    } else {
                        Expr::CmpOp { left: Box::new(left.clone()), op: *op, right: Box::new(right.clone()) }
                    };
                    if let Some((k, true)) = key_presence(&e, slot, false, aliases) { out.push((path.to_vec(), k, None, Some(i))); }
                }
            }
            Statement::If { condition, then_body, else_body } => {
                if let Some((k, present)) = key_presence(&condition.node, slot, false, aliases) {
                    if present {
                        out.push((sub(i, 0), k, None, None));
                    } else {
                        out.push((sub(i, 1), k.clone(), None, None));
                        if else_body.is_empty() && exits(then_body) { out.push((path.to_vec(), k, Some(sub(i, 0)), Some(i))); }
                    }
                }
                out.extend(existing_key_facts(then_body, &sub(i, 0), slot, aliases));
                out.extend(existing_key_facts(else_body, &sub(i, 1), slot, aliases));
            }
            Statement::For { var, iter, body, .. } => {
                // `for k in rows.keys { rows.set(k, …) }`: k was a key when
                // the loop started — setting it again cannot add past the
                // size the slot had (a deleted k comes back, no more)
                let over_keys = match &iter.node {
                    Expr::FieldAccess { target, field } => field == "keys" && matches!(&target.node, Expr::Ident(n) if n == slot),
                    Expr::MethodCall { target, method, args } => method == "keys" && args.is_empty() && matches!(&target.node, Expr::Ident(n) if n == slot),
                    Expr::FnCall { name, args } => name == "keys" && args.len() == 1 && matches!(&args[0].node, Expr::Ident(n) if n == slot),
                    _ => false,
                };
                if over_keys { out.push((sub(i, 2), var.clone(), None, None)); }
                out.extend(existing_key_facts(body, &sub(i, 2), slot, aliases));
            }
            Statement::While { body, .. } => out.extend(existing_key_facts(body, &sub(i, 2), slot, aliases)),
            _ => {}
        }
    }
    out
}

/// The handlers a body can call: `f(..)` (cell None), `Cell.h(..)` (Some(cell)),
/// `emit ev(..)` (Some("*")).
fn calls_of(stmts: &[Spanned<Statement>], cells: &HashSet<String>, tools: &[String], cell_handlers: &HashMap<String, Vec<String>>) -> Vec<(Option<String>, String)> {
    let mut out: Vec<(Option<String>, String)> = Vec::new();
    // `delegate("A", op, x)` with a computed handler may run any handler of
    // A (a computed cell: any handler anywhere) — it "proved" a size bound
    crate::checker::literals::for_each_expr(stmts, &mut |e| if let Expr::FnCall { name, args } = e {
        if name == "delegate" && args.len() >= 2 && !matches!(args[1].node, Expr::Literal(Literal::String(_))) {
            match &args[0].node {
                Expr::Literal(Literal::String(c)) => for h in cell_handlers.get(c).into_iter().flatten() { out.push((Some(c.clone()), h.clone())); },
                _ => for (c, hs) in cell_handlers { for h in hs { out.push((Some(c.clone()), h.clone())); } },
            }
        }
        // `vote(Cell.h, input, k)` runs its target k times, right here
        // (a size bound was "proven" past a vote whose target wrote the slot)
        if name == "vote" && args.len() == 3 {
            match &args[0].node {
                Expr::FieldAccess { target, field } => if let Expr::Ident(c) = &target.node { out.push((Some(c.clone()), field.clone())); },
                Expr::Ident(h) => out.push((None, h.clone())),
                Expr::Literal(Literal::String(t)) => match t.split_once('.') {
                    Some((c, h)) => out.push((Some(c.to_string()), h.to_string())),
                    None => out.push((None, t.clone())),
                },
                _ => for (c, hs) in cell_handlers { for h in hs { out.push((Some(c.clone()), h.clone())); } },
            }
        }
        // a literal handler: that handler of the named cell — or of ANY
        // cell defining it when the cell is computed (`delegate(c, "helper",
        // x)` with `c = "App" + ""` was not followed)
        if name == "delegate" && args.len() >= 2 {
            if let Expr::Literal(Literal::String(h)) = &args[1].node {
                match &args[0].node {
                    Expr::Literal(Literal::String(c)) => out.push((Some(c.clone()), h.clone())),
                    _ => for (c, hs) in cell_handlers { if hs.contains(h) { out.push((Some(c.clone()), h.clone())); } },
                }
            }
        }
    });
    let mut thinks = false;
    crate::checker::literals::for_each_expr(stmts, &mut |e| if matches!(e, Expr::FnCall { name, .. } if name == "think" || name == "think_json" || name == "vote") { thinks = true; });
    if thinks { for t in tools { out.push((None, t.clone())); } }
    crate::checker::literals::for_each_expr(stmts, &mut |e| match e {
        Expr::FnCall { name, .. } => out.push((None, name.clone())),
        Expr::MethodCall { target, method, .. } => {
            if let Expr::Ident(c) = &target.node { if cells.contains(c) { out.push((Some(c.clone()), method.clone())); } }
        }
        _ => {}
    });
    // inside `try { }` / block lambdas / match arms too: `try { emit more(..) }`
    // grew the slot past a "proven" size bound
    crate::checker::literals::for_each_stmt_deep(stmts, &mut |st| match st {
        Statement::MethodCall { target, method, .. } if cells.contains(target) => out.push((Some(target.clone()), method.clone())),
        Statement::Emit { signal_name, .. } => out.push((Some("*".to_string()), signal_name.clone())),
        _ => {}
    });
    out
}

/// Can this statement leave the block early (a `return`, `break` or
/// `continue`, at any depth — `fail()` raises and rolls back, so it is not
/// an exit that commits)?
fn can_exit(st: &Spanned<Statement>) -> bool {
    match &st.node {
        Statement::Return { .. } | Statement::Break | Statement::Continue => true,
        Statement::If { then_body, else_body, .. } => then_body.iter().any(can_exit) || else_body.iter().any(can_exit),
        Statement::For { body, .. } | Statement::While { body, .. } => body.iter().any(can_exit),
        _ => {
            let mut hit = false;
            crate::checker::literals::for_each_expr(std::slice::from_ref(st), &mut |e| match e {
                Expr::Match { arms, .. } => { if arms.iter().any(|a| a.body.iter().any(can_exit)) { hit = true; } }
                Expr::IfExpr { then_body, else_body, .. } => { if then_body.iter().chain(else_body).any(can_exit) { hit = true; } }
                _ => {}
            });
            hit
        }
    }
}

/// `c` with the slot name replaced by the written value, rendered — on the
/// AST (a text replace turned `key != "admin"` into `"1dmin"`), with the
/// value parenthesised when it is an operation (`(n + 1) % 2`).
/// The invariant clause over the WRITTEN value, as a `require` a handler can
/// write: the slot name and `value` become the written expression, `key`
/// the written key (`require value >= …` was suggested — `value` does not
/// exist in a handler).
fn subst_render(c: &Expr, slot: &str, value: &Expr, key: Option<&str>) -> String {
    let shown = match value {
        Expr::BinaryOp { .. } => format!("({})", render_expr(value)),
        _ => render_expr(value),
    };
    fn go(e: &Expr, slot: &str, shown: &str, key: Option<&str>) -> Expr {
        let b = |x: &Spanned<Expr>| Box::new(Spanned::new(go(&x.node, slot, shown, key), x.span));
        match e {
            Expr::Ident(n) if n == slot || n == "value" => Expr::Ident(shown.to_string()),
            Expr::Ident(n) if n == "key" && key.is_some() => Expr::Ident(key.unwrap_or("key").to_string()),
            Expr::BinaryOp { left, op, right } => Expr::BinaryOp { left: b(left), op: *op, right: b(right) },
            Expr::CmpOp { left, op, right } => Expr::CmpOp { left: b(left), op: *op, right: b(right) },
            Expr::Not(i) => Expr::Not(b(i)),
            Expr::FnCall { name, args } => Expr::FnCall { name: name.clone(), args: args.iter().map(|a| Spanned::new(go(&a.node, slot, shown, key), a.span)).collect() },
            // `versions.get(key)` keeps its slot receiver, its key is the written one
            Expr::MethodCall { target, method, args } => Expr::MethodCall { target: target.clone(), method: method.clone(), args: args.iter().map(|a| Spanned::new(go(&a.node, slot, shown, key), a.span)).collect() },
            Expr::FieldAccess { target, field } => Expr::FieldAccess { target: b(target), field: field.clone() },
            other => other.clone(),
        }
    }
    render_expr(&go(c, slot, &shown, key))
}

fn type_mentions_float(t: &TypeExpr) -> bool {
    match t {
        TypeExpr::Simple(n) => n == "Float",
        TypeExpr::Generic { args, .. } => args.iter().any(|a| type_mentions_float(&a.node)),
        _ => false,
    }
}

/// No Float can come out of it: Int literals, and `+ - * %` over them and
/// over names (a Float slot written from an Int expression stays NaN-free
/// only when its names are Ints — the conservative case is a literal).
fn is_int_valued(e: &Expr) -> bool {
    match e {
        Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::BigInt(_)) => true,
        Expr::BinaryOp { left, op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Mod, right } => is_int_valued(&left.node) && is_int_valued(&right.node),
        _ => false,
    }
}

/// Cannot be NaN: a literal, a read of the slot itself (a stored value passed
/// the invariant, and NaN passes no comparison), `a ?? b` of those, and a
/// literal added to or subtracted from one of those.
fn nan_free_float(e: &Expr, slot: &str) -> bool {
    let lit = |x: &Expr| matches!(x, Expr::Literal(Literal::Int(_) | Literal::Float(_)));
    match e {
        _ if lit(e) => true,
        Expr::MethodCall { target, method, args } => method == "get" && args.len() == 1 && matches!(&target.node, Expr::Ident(t) if t == slot),
        Expr::FnCall { name, args } if name == "_coalesce" && args.len() == 2 => nan_free_float(&args[0].node, slot) && nan_free_float(&args[1].node, slot),
        Expr::BinaryOp { left, op: BinOp::Add | BinOp::Sub, right } =>
            (lit(&left.node) && nan_free_float(&right.node, slot)) || (lit(&right.node) && nan_free_float(&left.node, slot)),
        _ => false,
    }
}

/// Every identifier an expression can read at run time: through string
/// interpolation, lambda bodies and match arms too.
pub(crate) fn deep_idents(e: &Expr) -> HashSet<String> {
    let mut out = HashSet::new();
    crate::checker::desugar::for_each_deep(e, &mut |x| match x {
        Expr::Ident(n) => { out.insert(n.clone()); }
        Expr::MethodCall { target, .. } => { if let Expr::Ident(n) = &target.node { out.insert(n.clone()); } }
        _ => {}
    });
    out
}

/// A suggested `require` must be writable in a handler: `size` / `key` /
/// `value` exist only inside an invariant (the hint `require v + size <= 3`
/// was itself a check error).
fn handler_writable(txt: &str) -> bool {
    !txt.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| matches!(w, "size" | "key" | "value"))
}
