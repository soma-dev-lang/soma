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
    PROGRAM_HANDLERS.with(|h| *h.borrow_mut() = super::desugar::program_names(program).0);
    let mut issues = Vec::new();
    // interior cells too: an invariant naming an unknown slot, or reading
    // `status` without a machine, was unchecked inside `interior { }`
    for cell in crate::checker::names::collect_cells(program) {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        set_slot_int(cell);
        for section in &cell.sections {
            let Section::Memory(mem) = &section.node else { continue };
            let slot_names: HashSet<&str> =
                mem.slots.iter().map(|s| s.node.name.as_str()).collect();
            for inv in &mem.invariants {
                let mut idents = HashSet::new();
                collect_idents(&inv.node, &mut idents);
                // `status` is the machine state of the written key: it needs
                // a `state` section in this cell
                if deep_idents(&inv.node).contains("status")
                    && !cell.sections.iter().any(|s| matches!(&s.node, Section::State(_)))
                {
                    issues.push(InvariantIssue {
                        message: format!("memory invariant reads `status` (the machine state of the written key), but cell '{}' declares no `state {{ }}` machine", cell.name),
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
                // A rule with only generic bindings guards EVERY slot of the
                // section, and `key` is a String on a Map but the index (an
                // Int) on a List: `key != ""` beside a List slot failed every
                // push at run time ("cannot compare Int and String").
                if named.is_empty() && deep.contains("key") {
                    let kind_of = |sl: &crate::ast::Spanned<crate::ast::MemorySlot>| match &sl.node.ty.node {
                        crate::ast::TypeExpr::Generic { name, .. } if name == "List" => Some("List"),
                        // a Map<Int, V>'s key is an Int
                        crate::ast::TypeExpr::Generic { name, args } if name == "Map"
                            && args.len() == 2 && matches!(&args[0].node, crate::ast::TypeExpr::Simple(k) if k == "Int") => Some("IntMap"),
                        crate::ast::TypeExpr::Generic { name, .. } if name == "Map" => Some("Map"),
                        _ => None,
                    };
                    let lists: Vec<&str> = mem.slots.iter().filter(|sl| kind_of(sl) == Some("List")).map(|sl| sl.node.name.as_str()).collect();
                    let maps: Vec<&str> = mem.slots.iter().filter(|sl| kind_of(sl) == Some("Map")).map(|sl| sl.node.name.as_str()).collect();
                    let (mut as_string, mut as_int) = (false, false);
                    crate::checker::literals::for_each_in_expr(&inv.node, &mut |e| match e {
                        Expr::CmpOp { left, right, .. } => {
                            let is_key = |x: &Expr| matches!(x, Expr::Ident(n) if n == "key");
                            for (a, b) in [(&left.node, &right.node), (&right.node, &left.node)] {
                                if is_key(a) {
                                    match b {
                                        Expr::Literal(Literal::String(_)) => as_string = true,
                                        Expr::Literal(Literal::Int(_)) => as_int = true,
                                        _ => {}
                                    }
                                }
                            }
                        }
                        Expr::FnCall { name, args } if matches!(name.as_str(), "len" | "str_len" | "starts_with" | "ends_with" | "contains" | "trim" | "lowercase" | "uppercase" | "regex_match")
                            && args.iter().any(|a| matches!(&a.node, Expr::Ident(n) if n == "key")) => as_string = true,
                        _ => {}
                    });
                    if as_string && !lists.is_empty() {
                        issues.push(InvariantIssue {
                            message: format!(
                                "memory invariant compares `key` with a String, but it names no slot so it guards every slot of this section, and on a List slot ({}) `key` is the index (an Int): every push would fail at run time. Put the List slot{} in {} own `memory {{ }}` section",
                                lists.join(", "), if lists.len() == 1 { "" } else { "s" }, if lists.len() == 1 { "its" } else { "their" }
                            ),
                            span: inv.span,
                        });
                    }
                    if as_int && !maps.is_empty() {
                        issues.push(InvariantIssue {
                            message: format!(
                                "memory invariant compares `key` with an Int, but it names no slot so it guards every slot of this section, and on a Map slot ({}) `key` is a String: every set would fail at run time. Put the Map slot{} in {} own `memory {{ }}` section",
                                maps.join(", "), if maps.len() == 1 { "" } else { "s" }, if maps.len() == 1 { "its" } else { "their" }
                            ),
                            span: inv.span,
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
    CELL_HANDLERS.with(|h| {
        let mut h = h.borrow_mut();
        h.clear();
        for sec in &cell.sections {
            if let Section::OnSignal(on) = &sec.node { h.insert(on.signal_name.clone()); }
        }
    });
    SLOT_INT.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        for sec in &cell.sections {
            if let Section::Memory(mem) = &sec.node {
                for sl in &mem.slots {
                    let int = match &sl.node.ty.node {
                        TypeExpr::Generic { args, .. } => args.last().map_or(false, |a| matches!(&a.node, TypeExpr::Simple(t) if t == "Int")),
                        _ => false,
                    };
                    m.insert(sl.node.name.clone(), int);
                }
            }
        }
    });
}

pub fn verify_program_invariants(program: &Program) -> Vec<VerifyResult> {
    PROGRAM_HANDLERS.with(|h| *h.borrow_mut() = super::desugar::program_names(program).0);
    // 3.0: a face `requires` is a fact for the handler's writes
    let inlined = super::contracts::inline_requires(program);
    let program = &inlined;
    let mut results = Vec::new();

    // interior cells too: an invariant inside `interior { }` was never
    // proven — a statically violated one passed as VERIFY OK
    for cell in crate::checker::names::collect_cells(program) {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        set_slot_int(&cell);
        set_contract_results(program, cell);

        // invariant → the slots it guards (same scoping rule as runtime)
        let mut guarded: Vec<(Expr, Vec<String>, String)> = Vec::new();
        let mut status_notes: Vec<String> = Vec::new();
        for section in &cell.sections {
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
                // a slot named like a binding an invariant reads (`size`, `key`…):
                // `invariant size >= 0` is then about the COUNT, at run time —
                // it bounds no value read from that slot
                if matches!(slot.as_str(), "size" | "key" | "status" | "_slot_len") { continue; }
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
        let ticks: Vec<OnSection> = cell.sections.iter().enumerate().filter_map(|(i, s)| match &s.node {
            Section::Every(e) => Some(OnSection { signal_name: format!("every {}ms #{}", e.interval_ms, i), params: vec![], body: e.body.clone(), properties: if e.task { vec!["task".to_string()] } else { vec![] } }),
            Section::After(e) => Some(OnSection { signal_name: format!("after {}ms #{}", e.interval_ms, i), params: vec![], body: e.body.clone(), properties: if e.task { vec!["task".to_string()] } else { vec![] } }),
            _ => None,
        }).collect();
        let mut handlers: HashMap<String, &OnSection> = cell
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
        for t in &ticks { all_handlers.push((cell.name.clone(), t)); }
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
        for (name, slot_at, _, _, path, _) in &writes {
            if let Some(on) = handlers.get(name) {
                locals.entry((name.clone(), path.clone())).or_insert_with(|| local_ranges(on, &hyp_for(on, &hyp), &handlers, path, Some(slot_at.as_str())));
            }
        }

        let mut result = VerifyResult {
            machine_name: format!("{}/invariants", cell.name),
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
                    let list_slot = cell.sections.iter().any(|sec| matches!(&sec.node, Section::Memory(m)
                        if m.slots.iter().any(|sl| sl.node.name == *slot && matches!(&sl.node.ty.node, TypeExpr::Simple(t) | TypeExpr::Generic { name: t, .. } if t == "List"))));
                    let keyed = names.contains("key") || names.contains("_key");
                    // a rule BETWEEN slots CAN break on a delete: the entry
                    // it drops takes that side to () (a "proven" ✓ let a
                    // delete leave 3 reserved against 0 in stock)
                    let cross = {
                        let all: Vec<String> = cell.sections.iter().filter_map(|sec| match &sec.node {
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
                    if !names.contains("size") && !names.contains("len") && !names.contains("_slot_len") && !fns.contains("len") && !fns.contains("size") {
                        result.checks.push(VerifyCheck::Pass(format!(
                            "invariant {inv_text} — writer '{handler}' only deletes from '{slot}' (a value invariant cannot break on a delete)"
                        )));
                        continue;
                    }
                    // `size <= K` cannot break on a delete either: it shrinks
                    // (the bounded-queue pop was "may grow past 64")
                    let sized = |c: &Expr| { let mut n = HashSet::new(); collect_idents(c, &mut n); let mut f = HashSet::new(); collect_fn_names(c, &mut f);
                        n.contains("size") || n.contains("len") || n.contains("_slot_len") || f.contains("len") || f.contains("size") };
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
                    let me = cell.name.clone();
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
                // …and, in ANY handler, a resume() / deliver(): the rest of a
                // waiting handler runs inside this one, with its writes
                let is_task = handlers.get(handler).map_or(false, |on| on.properties.iter().any(|p| p == "task"));
                let mut resumes = false;
                let task_with_think = {
                    let me = cell.name.clone();
                    let mut seen: HashSet<(String, String)> = HashSet::new();
                    let mut stack: Vec<(String, String)> = vec![(me.clone(), handler.clone())];
                    let mut t = false;
                    while let Some((c, h)) = stack.pop() {
                        if (t && resumes) || !seen.insert((c.clone(), h.clone())) { continue; }
                        let Some((_, on)) = all_handlers.iter().find(|(cn, o)| *cn == c && o.signal_name == h) else { continue };
                        crate::checker::literals::for_each_expr(&on.body, &mut |e| if matches!(e, Expr::FnCall { name, .. } if matches!(name.as_str(), "think" | "think_json" | "vote" | "await_approve" | "await_signal" | "await_sleep")) { t = true; });
                        crate::checker::literals::for_each_expr(&on.body, &mut |e| if matches!(e, Expr::FnCall { name, .. } if name == "resume" || name == "deliver") { resumes = true; });
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
                    (is_task && t) || resumes
                };
                // …except about the PARAMETERS, which nothing but the handler
                // rebinds (`require n >= 0 && n <= 100` was cleared too, a
                // spurious ⚠ on `scores.set(k, n)`)
                if task_with_think {
                    let params: HashSet<&str> = handlers.get(handler).map(|on| on.params.iter()
                        .filter(|p| { let mut a = Vec::new(); collect_assigns(&on.body, &mut a); !a.iter().any(|(m, _)| *m == p.name) })
                        .map(|p| p.name.as_str()).collect()).unwrap_or_default();
                    vars_w.retain(|k, _| {
                        let text = match k.strip_prefix("__") { Some(rest) => rest.split_once("__").map_or(rest, |(_, t)| t), None => k.as_str() };
                        // only names (no call, field or index), each a parameter
                        let mut q = false;
                        let mut plain = String::new();
                        for c in text.chars() { if c == '"' { q = !q; plain.push(' '); } else if !q { plain.push(c); } }
                        !plain.contains(|c| matches!(c, '(' | '.' | '['))
                            && plain.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty() && !w.starts_with(|c: char| c.is_ascii_digit()))
                                .all(|w| params.contains(w) || w == "true" || w == "false")
                    });
                }
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
                        // `get(k, default)` gives the default on a missing key
                        Expr::MethodCall { method, args, .. } if method == "get" => args.get(1).map_or(true, |d| may_be_unit(&d.node) || matches!(d.node, Expr::Literal(Literal::Unit))),
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
                    // (the default of a `?? d`, as text: the invariant's and the write's must be the same)
                    let default_of = |e: &Expr| -> String { match e { Expr::FnCall { name, args } if name == "_coalesce" && args.len() == 2 => render_expr(&args[1].node), _ => String::new() } };
                    let inv_default = default_of(&right.node);
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
                    // the value must not call what can write the slot between
                    // its read and the write: `(seq.get(k) ?? 0) + bump(k)`
                    // wrote old + 1 after bump(k) stored old + 100
                    if value_may_write(value_expr, &handlers) { continue; }
                    // …nor bind the key's name itself (`if true { let k = "other"  seq.get(k) } …`)
                    if let Some(wk) = wkey.as_deref() {
                        let mut inner: HashSet<String> = HashSet::new();
                        collect_binders_expr(value_expr, &mut inner);
                        crate::checker::literals::for_each_in_expr(value_expr, &mut |x| collect_binders_expr(x, &mut inner));
                        let mut reassigned = false;
                        crate::checker::literals::for_each_stmt_in_expr(value_expr, &mut |st| if matches!(st, Statement::Assign { name, .. } | Statement::Let { name, .. } if name == wk) { reassigned = true; });
                        if inner.contains(wk) || reassigned { continue; }
                    }
                    // the written value: old + n (n ≥ 0, and > 0 for a strict >), or max(old, …)
                    let same_key = |e: &Expr| -> bool {
                        if default_of(e) != inv_default { return false; }
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
                                (Some((lo, _)), CmpOp::Gt) => lo > 0.0 && SLOT_INT.with(|m| m.borrow().get(slot).copied().unwrap_or(false)),
                                _ => false,
                            }
                        }
                        // (the builtin: a handler `max` of the cell is what runs otherwise)
                        Expr::FnCall { name, args } if name == "max" && args.len() == 2 && matches!(op, CmpOp::Ge) && !handlers.contains_key("max") =>
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
                    let me = cell.name.clone();
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
                    if k.contains('{') { return false; }
                    let mut mutated: HashSet<String> = HashSet::new();
                    index_sets(&on.body, &mut mutated);
                    if unquoted.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| !w.is_empty() && (rebound(w) || mutated.contains(w) || (shadow.contains(w) && !single_for(w)))) { return false; }
                    // `let r = rows.get(k)` bound once: `r != ()` says k exists;
                    // `let r = rows.get(k) ?? ""`: `r != ""` says so too (a
                    // missing key gives the default)
                    let aliases: HashMap<String, (String, Option<String>)> = assigns.iter()
                        .filter(|(n, _)| assigns.iter().filter(|(m, _)| m == n).count() == 1 && !shadow.contains(*n))
                        .filter_map(|(n, e)| {
                            let (get, default) = match e {
                                Expr::FnCall { name, args } if name == "_coalesce" && args.len() == 2
                                    && matches!(args[1].node, Expr::Literal(_)) => (&args[0].node, Some(render_expr(&args[1].node))),
                                other => (*other, None),
                            };
                            match get {
                                Expr::MethodCall { target, method, args } if method == "get" && args.len() == 1
                                    && matches!(&target.node, Expr::Ident(t) if t == slot) => Some((n.to_string(), (render_expr(&args[0].node), default))),
                                _ => None,
                            }
                        }).collect();
                    existing_key_facts(&on.body, &[], slot, &aliases).iter().any(|(fp, fk, excl, after)| path.starts_with(fp) && fk == k && excl.as_ref().map_or(true, |e| !path.starts_with(e))
                        // a require / early exit covers only the writes AFTER it
                        && after.map_or(true, |j| path.get(fp.len()).map_or(false, |s| *s / 4 > j)))
                };
                // not across a think() of a [task]: other requests run during
                // it and may delete the key (a `require m.get(k) != ()` then
                // think then m.set(k, …) grew the slot past `size <= 2`)
                if size_upper_bound_any(&parts, slot) && key_exists(wpath, wkey) && !task_with_think {
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
                        let me = cell.name.clone();
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
                            // (by its text: not when a local of the handler bears a name the
                            // clause reads — `let size = 0  require size <= 1` is no bound on
                            // the slot's size — nor when the text leaves a body out)
                            let shadowed = deep_idents(c).iter().any(|n| ctx.vars.contains_key(&format!("__local__{}", n)));
                            let text = subst_render(c, slot, value_expr, wkey.as_deref());
                            if *v != Proof::Holds && !shadowed && !text.contains('…') && pure_here(c, slot) && ctx.vars.contains_key(&format!("__req__{}", text)) {
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
                    let me = cell.name.clone();
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
                let float_slot = cell.sections.iter().any(|s| match &s.node {
                    Section::Memory(m) => m.slots.iter().any(|sl| sl.node.name == *slot && may_hold_float(&sl.node.ty.node)),
                    _ => false,
                });
                let mut nan_open = false;
                if float_slot && !matches!(known, Known::Exact(_)) && !is_int_valued(value_expr) && !nan_free_float(value_expr, slot)
                    && !ctx.vars.contains_key(&format!("__cmp__{}", render_expr(value_expr))) {
                    for (c, v) in parts.iter().zip(verdicts.iter_mut()) {
                        if *v == Proof::Holds && size_upper_bound(c, slot).is_none() { *v = Proof::Unknown; nan_open = true; }
                    }
                }
                // 3.0 (B4): what the intervals leave open, as linear
                // arithmetic over the facts at the write (`require b >=
                // amount` proves `b - amount >= 0`). Not for a Float slot,
                // a [task] that thinks, or a size clause.
                let mut linear = false;
                if !float_slot && !task_with_think {
                    if let Some(on) = handlers.get(handler) {
                        let int_returns = int_returning(cell);
                        for (c, v) in parts.iter().zip(verdicts.iter_mut()) {
                            if *v != Proof::Unknown || size_upper_bound(c, slot).is_some() { continue; }
                            let Expr::CmpOp { left, op, right } = c else { continue };
                            // the clause speaks of the stored value only
                            if !deep_idents(c).iter().all(|n| n == slot) { continue; }
                            if linear_entails(on, wpath, &left.node, *op, &right.node, Some((slot.as_str(), value_expr)), ctx.hyp, &int_returns) {
                                *v = Proof::Holds;
                                linear = true;
                            }
                        }
                    }
                }
                let how = if linear { "proven (linear arithmetic over the handler's requires and conditions)" } else if inductive { "proven by induction" } else { "proven" };

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
                    // a hole's model writes `?`: say what that is, not "`?` has no known range"
                    let hole = on.map(|o| super::flesh::is_hole(o)).unwrap_or(false);
                    let mut why: Vec<String> = if hole {
                        vec!["it is a hole (its frame lets a body write any value — judged when the body exists)".to_string()]
                    } else { names.iter().filter(|n| bounds(ctx.range_of(&Expr::Ident((*n).clone()))).is_none()).map(|n| {
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
                            // the handler rebinds the parameter, so a require
                            // on it describes a value that is gone by the
                            // write: telling the author to add one (or that
                            // theirs is missing) sent them after a fix that
                            // cannot work
                            let mut rebinds: Vec<(&str, &Expr)> = Vec::new();
                            if let Some(o) = on { collect_assigns(&o.body, &mut rebinds); }
                            if rebinds.iter().any(|(m, _)| *m == n.as_str()) {
                                return format!(
                                    "`{n}` is a parameter the handler rebinds before the write, so no `require` on it describes the value written — write the value you checked, or require the one you write"
                                );
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
                    }).collect() };
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
                    // `invariant len(rows) <= N` reads like a count and is not one
                    // (an agent wrote it, got a bare ⚠ and no reason)
                    if why.is_empty() {
                        let mut lens = Vec::new();
                        for c in &parts { find_len_of_slot(c, std::slice::from_ref(slot), &mut lens); }
                        if !lens.is_empty() {
                            why = vec![format!("`len({slot})` in an invariant measures each VALUE written to '{slot}', not how many entries it holds — for a bound on the entries write `{slot}.size <= N`")];
                        }
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
                    let all: Vec<String> = cell.sections.iter().filter_map(|sec| match &sec.node {
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
    /// `e` names a memory slot, and nothing in the handler hides it.
    fn is_slot(&self, e: &Expr) -> bool {
        matches!(e, Expr::Ident(n) if self.hyp.contains_key(&format!("{}{}", SLOT_MARK, n)) && !self.vars.contains_key(n))
    }

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
        // `slot.get(k, default)`: a missing key gives the DEFAULT, which no
        // invariant bounds (`vals.get(k, -50)` under `vals >= 0` is -50)
        if let Expr::MethodCall { args, .. } = expr {
            if let Some(d) = args.get(1) {
                return Some(join(mk(*lo, *hi), self.range_of(&d.node)));
            }
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
            // (a text that leaves a body out — `if c { … } else { … }` — names no one expression)
            if let Some(fact) = Some(render_expr(expr)).filter(|t| !t.contains('…')).and_then(|t| self.vars.get(&format!("__expr__{}", t)).copied()).and_then(bounds) {
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
            // — with a catch-all arm: without one a value no arm matches
            // gives () (`match x { 1 -> 5  2 -> 6 }` on 3); a match on
            // variants is exhaustive (`soma check` refuses a missing one)
            Expr::Match { arms, .. } if !arms.is_empty() && arms.iter().any(|a| (a.guard.is_none() && matches!(a.pattern, MatchPattern::Wildcard | MatchPattern::Variable(_)))
                || matches!(a.pattern, MatchPattern::Variant { .. })) =>
                arms.iter().map(|a| self.range_of(&a.result.node)).reduce(join).unwrap_or(Known::Unknown),
            Expr::Literal(Literal::Int(n)) => mk(*n as f64, *n as f64),
            Expr::Literal(Literal::Float(f)) => Known::Exact(*f),
            Expr::Ident(n) => self.vars.get(n).copied().unwrap_or(Known::Unknown),
            // `slot.size`, `slot.len`: a count is never negative
            Expr::FieldAccess { target, field } if matches!(field.as_str(), "size" | "len") && self.is_slot(&target.node) => mk(0.0, f64::INFINITY),
            // `slot.count(x => …)`, `slot.keys.count(x => …)`: so many of them
            Expr::MethodCall { target, method, args } if method == "count" && args.len() == 1 && !self.handlers.contains_key("count")
                && matches!(args[0].node, Expr::Lambda { .. } | Expr::LambdaBlock { .. })
                && (self.is_slot(&target.node) || matches!(&target.node, Expr::FieldAccess { target: t, field } if matches!(field.as_str(), "keys" | "values" | "entries") && self.is_slot(&t.node))) => mk(0.0, f64::INFINITY),
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
                // `x % n` with a literal n: |x % n| < n (and ≥ 0 when x is);
                // for an Int x that is at most n - 1, for a Float only below n
                if matches!(op, BinOp::Mod) {
                    if let Some((nl, nh)) = bounds(self.range_of(&right.node)) {
                        if nl == nh && nl != 0.0 {
                            let n = nl.abs();
                            let top = if may_be_float(&left.node, &self.vars) || nl.fract() != 0.0 { n } else { n - 1.0 };
                            let lo = match bounds(self.range_of(&left.node)) { Some((l, _)) if l >= 0.0 => 0.0, _ => -top };
                            return mk(lo, top);
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
            // 3.0: `Ledger.balance()` — a call into another cell is known by
            // what that cell's face ensures of the result, and by nothing else
            Expr::MethodCall { target, method, .. } => match &target.node {
                Expr::Ident(t) if !self.vars.contains_key(t) && !self.hyp.contains_key(t) && !self.vars.contains_key(&format!("__local__{}", t))
                    && !SLOT_INT.with(|m| m.borrow().contains_key(t)) => with_contract(Known::Unknown, &format!("{}.{}", t, method)),
                _ => Known::Unknown,
            },
            Expr::FnCall { name, args } => {
            // a handler named like a builtin runs only for an arity it
            // accepts: `max(x, 0)` beside `on max(a, b, c)` is the builtin
            // (the handler's `return 5` was taken for it)
            let builtin_call = self.handlers.get(name).map_or(super::names::builtin_names().contains(name.as_str()), |on| super::names::builtin_names().contains(name.as_str())
                && !crate::ast::accepted_arities(&on.params).contains(&args.len()));
            // 3.0: what the callee's face ensures narrows whatever its body shows
            let base = (|| -> Known { match (name.as_str(), args.len()) {
                // a handler of the cell named like a builtin IS what runs
                // (`on clamp(x, lo, hi) { return x }` was trusted as clamp)
                (_, _) if !builtin_call && self.depth < 3 && self.handlers.contains_key(name) => match self.handlers.get(name) {
                    Some(on) => {
                        // a body that can end without a value returns ():
                        // `if n > 0 { return 5 }` is not "5"
                        if may_fall_off(&on.body) { return Known::Unknown; }
                        // the CALLEE's slot bounds: a `let limits = map(…)` there
                        // hides the slot `limits` (the caller's were used)
                        let hyp = hyp_for(on, self.hyp);
                        let inner = RangeCtx {
                            hyp: &hyp,
                            vars: local_ranges_at(on, &hyp, self.handlers, self.depth + 1, &[], None),
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
                (_, _) if !builtin_call && self.handlers.contains_key(name) => Known::Unknown,
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
                ("count", 2) if matches!(args[1].node, Expr::Lambda { .. } | Expr::LambdaBlock { .. }) => mk(0.0, f64::INFINITY),
                // `sum(slot.values)` of a slot whose values are never negative
                ("sum", 1) => match &args[0].node {
                    Expr::FieldAccess { target, field } if field == "values" && self.is_slot(&target.node) => match &target.node {
                        Expr::Ident(n) if SLOT_INT.with(|m| m.borrow().get(n).copied().unwrap_or(false)) && self.hyp.get(n).is_some_and(|(lo, _)| *lo >= 0.0) => mk(0.0, f64::INFINITY),
                        _ => Known::Unknown,
                    },
                    _ => Known::Unknown,
                },
                ("abs", 1) => match bounds(self.range_of(&args[0].node)) {
                    Some((lo, hi)) => {
                        let low = if lo <= 0.0 && hi >= 0.0 { 0.0 } else { lo.abs().min(hi.abs()) };
                        mk(low, lo.abs().max(hi.abs()))
                    }
                    None => mk(0.0, f64::INFINITY),
                },
                // `len("abc")` / `len([1, 2])`: exactly what it is — a
                // literal with a brace is interpolated (`"{s}"`) or escaped
                // (`"{{}}"` is 2 characters): only `>= 0` is known then
                ("len", 1) => match &args[0].node {
                    Expr::Literal(Literal::String(t)) if !t.contains(['{', '}']) => { let n = t.chars().count() as f64; mk(n, n) }
                    Expr::ListLiteral(items) => { let n = items.len() as f64; mk(n, n) }
                    _ => mk(0.0, f64::INFINITY),
                },
                // `mod(a, n)` / `idiv(a, n)` / `sqrt_int(x)` / `band(x, mask)`
                ("mod", 2) => match bounds(self.range_of(&args[1].node)) {
                    // floored: the result takes the DIVISOR's sign
                    // (mod(7, -3) == -2, mod(-7, 3) == 2)
                    Some((nl, nh)) if nl == nh && nl > 0.0 => mk(0.0, nl - 1.0),
                    Some((nl, nh)) if nl == nh && nl < 0.0 => mk(nl + 1.0, 0.0),
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
            } })();
            if builtin_call { base } else { with_contract(base, name) }
            }
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
    // a `return` inside a match arm, a `try` block or an if-expression
    // returns from the handler too: `let r = match n { 1 -> { return 0 - 5 } … }`
    // was left out, and `c.set(k, score(n))` was proven `c >= 0`
    super::literals::for_each_stmt_deep(stmts, &mut |st| {
        if let Statement::Return { value } = st { out.push(&value.node); }
    });
    collect_tail_values(stmts, out);
}

/// Can the body end without a value — no `return` or tail expression on some
/// path (an `if` without `else`, a loop that runs zero times)? The handler
/// then returns ().
fn may_fall_off(stmts: &[Spanned<Statement>]) -> bool {
    match stmts.last().map(|s| &s.node) {
        Some(Statement::Return { .. }) | Some(Statement::ExprStmt { .. }) => false,
        Some(Statement::If { then_body, else_body, .. }) => may_fall_off(then_body) || may_fall_off(else_body),
        _ => true,
    }
}

/// The value of a body that ends without `return` (its last expression).
fn collect_tail_values<'e>(stmts: &'e [Spanned<Statement>], out: &mut Vec<&'e Expr>) {
    for (i, stmt) in stmts.iter().enumerate() {
        match &stmt.node {
            Statement::ExprStmt { expr } if i + 1 == stmts.len() => out.push(&expr.node),
            Statement::If { then_body, else_body, .. } => {
                collect_tail_values(then_body, out);
                collect_tail_values(else_body, out);
            }
            Statement::While { body, .. } | Statement::For { body, .. } => collect_tail_values(body, out),
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
    let mut out: HashMap<String, (f64, f64)> = hyp.iter().filter(|(k, _)| !binders.contains(k.as_str())).map(|(k, v)| (k.clone(), *v)).collect();
    // the slots this handler reads as slots (no local, no parameter of the
    // name): `slot.size` is then a count — of a local it may be a field
    SLOT_INT.with(|m| for slot in m.borrow().keys() {
        if !binders.contains(slot) && !on.params.iter().any(|p| &p.name == slot) { out.insert(format!("{}{}", SLOT_MARK, slot), (0.0, f64::INFINITY)); }
    });
    out
}

/// In a handler's hypotheses: `SLOT_MARK + name` says `name` is a memory
/// slot there, hidden by nothing.
const SLOT_MARK: &str = "\u{1f}slot:";

fn local_ranges(
    on: &OnSection,
    hyp: &HashMap<String, (f64, f64)>,
    handlers: &HashMap<String, &OnSection>,
    write_path: &[usize],
    target: Option<&str>,
) -> HashMap<String, Known> {
    local_ranges_at(on, hyp, handlers, 0, write_path, target)
}

/// Does `e` evaluate to the same value at the `require` and at the write
/// under proof? Not when it calls an unstable builtin, reads state that
/// the handler also writes (other than the one read-modify-write of the
/// target slot), or calls a handler while this handler has other effects.
fn expr_stable(e: &Expr, on: &OnSection, handlers: &HashMap<String, &OnSection>, locals: &HashSet<&str>, target: Option<&str>, in_loop: bool) -> bool {
    let params: HashSet<&str> = on.params.iter().map(|p| p.name.as_str()).collect();
    let is_state = |t: &str| !params.contains(t) && !locals.contains(t);
    let mut ok = !interpolates(e);
    let mut reads: Vec<String> = Vec::new();
    let mut calls = false;
    // a call of a LOCAL function is as stable as the lambda's body: `let f =
    // v => random()` then `require f(1) <= 0.5` said nothing about the
    // second f(1). The bodies are scanned with the expression itself; a
    // name bound to anything but one `let f = <lambda>` is not followed.
    let mut lambdas: HashMap<&str, Vec<&Expr>> = HashMap::new();
    super::literals::for_each_stmt_deep(&on.body, &mut |st| {
        if let Statement::Let { name, value } | Statement::Assign { name, value } = st {
            lambdas.entry(name.as_str()).or_default().push(&value.node);
        }
    });
    let mut work: Vec<&Expr> = vec![e];
    let mut seen_fns: HashSet<&str> = HashSet::new();
    while let Some(e) = work.pop() {
    // the names the expression binds itself (a lambda parameter, a match
    // binding) are no state
    let mut bound_here: HashSet<String> = HashSet::new();
    collect_binders_expr(e, &mut bound_here);
    super::literals::for_each_in_expr(e, &mut |x| match x {
        // a slot passed whole is read too (`len(c) + n`, `sum(c)`, `c |> …`):
        // only `c.get(..)` / `c[..]` counted, and `require len(c) + n <= 5`
        // held for every later `c.set(k, len(c) + n)`
        Expr::Ident(n) if is_state(n) && !bound_here.contains(n.as_str())
            && n.chars().next().map_or(false, |c| c.is_ascii_lowercase() || c == '_') => reads.push(n.clone()),
        Expr::FnCall { name, .. } if super::names::unstable_builtin(name) => ok = false,
        Expr::FnCall { name, .. } if !handlers.contains_key(name) && name != "_coalesce"
            && (lambdas.contains_key(name.as_str()) || params.contains(name.as_str()) || !super::names::builtin_names().contains(name.as_str())) => {
            match lambdas.get(name.as_str()).map(|v| v.as_slice()) {
                Some([Expr::Lambda { body, .. }]) if !params.contains(name.as_str()) => {
                    if let Some((k, _)) = lambdas.get_key_value(name.as_str()) {
                        if seen_fns.insert(k) { work.push(&body.node); }
                    }
                }
                _ => ok = false,
            }
        }
        // a handler that draws a random number (or reads the clock, the
        // model…) answers differently at the write: `require (c.get(k) ?? 0)
        // - price() >= 0` said nothing about the second price()
        Expr::FnCall { name, .. } if handlers.contains_key(name) => { calls = true; if callee_unstable(name, handlers, 0) { ok = false; } }
        Expr::MethodCall { target: t, .. } | Expr::Index { target: t, .. } | Expr::FieldAccess { target: t, .. } => {
            if let Expr::Ident(n) = &t.node {
                if super::names::cellish(n) { ok = false; }
                else if is_state(n) { reads.push(n.clone()); }
            }
        }
        _ => {}
    });
    }
    if !ok { return false; }
    // a local or parameter whose PARTS the handler writes (`cur.amount = 0`,
    // `cur["amount"] = 0`): `cur.amount` at the require is not `cur.amount`
    // at the write
    let mut part_written: HashSet<String> = HashSet::new();
    super::literals::for_each_stmt_deep(&on.body, &mut |st| match st {
        Statement::IndexSet { name, .. } => { part_written.insert(name.split(|c| c == '.' || c == '[').next().unwrap_or(name).to_string()); }
        Statement::Assign { name, .. } if name.contains('.') || name.contains('[') => {
            part_written.insert(name.split(|c| c == '.' || c == '[').next().unwrap_or(name).to_string());
        }
        _ => {}
    });
    let mut reads_part = false;
    super::literals::for_each_in_expr(e, &mut |x| match x {
        Expr::MethodCall { target: t, .. } | Expr::Index { target: t, .. } | Expr::FieldAccess { target: t, .. } => {
            // (a slot's `c[k] = v` is its one read-modify-write, judged below)
            if matches!(&t.node, Expr::Ident(n) if part_written.contains(n) && !is_state(n)) { reads_part = true; }
        }
        _ => {}
    });
    if reads_part { return false; }
    // every write of the handler body, by target name; emits; handler calls
    let mut writes: HashMap<String, usize> = HashMap::new();
    let mut other_effects = false;
    let write_method = |m: &str| matches!(m, "set" | "put" | "delete" | "remove" | "push" | "append" | "clear");
    super::literals::for_each_stmt_deep(&on.body, &mut |st| match st {
        Statement::MethodCall { target: t, method, .. } if write_method(method) && is_state(t) => *writes.entry(t.clone()).or_default() += 1,
        Statement::IndexSet { name, .. } | Statement::Assign { name, .. } => {
            let root = name.split(|c| c == '.' || c == '[').next().unwrap_or(name);
            if is_state(root) { *writes.entry(root.to_string()).or_default() += 1; }
        }
        Statement::Emit { .. } => other_effects = true,
        _ => {}
    });
    super::literals::for_each_expr(&on.body, &mut |x| match x {
        Expr::MethodCall { target: t, method, .. } if write_method(method) => {
            if let Expr::Ident(n) = &t.node { if is_state(n) { *writes.entry(n.clone()).or_default() += 1; } }
        }
        // a call of another handler, or of another cell, may write anything
        Expr::FnCall { name, .. } if handlers.contains_key(name) && name != &on.signal_name => {
            other_effects |= callee_writes(name, handlers, 0);
        }
        // a bare name that is no builtin and no handler of this cell is
        // another cell's handler (`sabotage(k)` → B.sabotage)
        Expr::FnCall { name, .. } if !handlers.contains_key(name) && !super::names::builtin_names().contains(name.as_str()) && name != "_coalesce" => other_effects = true,
        Expr::MethodCall { target: t, .. } if matches!(&t.node, Expr::Ident(n) if super::names::cellish(n)) => other_effects = true,
        _ => {}
    });
    // a called handler (or another cell) may write what is read; and a
    // write in a loop runs again after itself (`require (c.get(k) ?? 0) - n
    // >= 0` held for the first pass only)
    if !reads.is_empty() && (other_effects || in_loop) { return false; }
    for r in &reads {
        let w = writes.get(r).copied().unwrap_or(0);
        if w > 1 || (w == 1 && Some(r.as_str()) != target) { return false; }
    }
    if calls {
        if other_effects { return false; }
        // a handler call re-evaluated in a loop or a lambda (`for … {
        // c.set("a", total() + n) }`) reads what the previous pass wrote
        if in_loop && !writes.is_empty() { return false; }
        let n: usize = writes.iter().map(|(k, v)| if Some(k.as_str()) == target { v.saturating_sub(1) } else { *v }).sum();
        if n > 0 { return false; }
    }
    true
}

/// Can two calls of handler `name` answer differently: does it (or a
/// handler it calls) call an unstable builtin, another cell, or a name the
/// prover cannot follow?
fn callee_unstable(name: &str, handlers: &HashMap<String, &OnSection>, depth: usize) -> bool {
    let Some(on) = handlers.get(name) else { return true };
    if depth > 6 { return true; }
    let mut hit = false;
    super::literals::for_each_expr(&on.body, &mut |x| match x {
        Expr::FnCall { name: n, .. } if handlers.contains_key(n) => { if n != name && callee_unstable(n, handlers, depth + 1) { hit = true; } }
        Expr::FnCall { name: n, .. } if super::names::unstable_builtin(n) => hit = true,
        Expr::FnCall { name: n, .. } if !super::names::builtin_names().contains(n.as_str()) && n != "_coalesce" => hit = true,
        Expr::MethodCall { target, .. } if matches!(&target.node, Expr::Ident(t) if super::names::cellish(t)) => hit = true,
        _ => {}
    });
    hit
}

/// Can evaluating `e` write state: a slot write in it, a handler that writes
/// (or one of another cell, or a name the prover cannot follow), an effect
/// builtin (think() may run a tool that writes)?
/// A string with `{…}` somewhere in `e`: what it calls runs where it
/// stands, which the rules that reason on the order of reads and writes
/// do not follow.
fn interpolates(e: &Expr) -> bool {
    let mut hit = false;
    super::literals::for_each_in_expr(e, &mut |x| if let Expr::Literal(Literal::String(s)) = x { if !super::desugar::segments(s).is_empty() { hit = true; } });
    hit
}

fn value_may_write(e: &Expr, handlers: &HashMap<String, &OnSection>) -> bool {
    let mut hit = interpolates(e);
    super::literals::for_each_in_expr(e, &mut |x| match x {
        Expr::FnCall { name, .. } if handlers.contains_key(name) => { if callee_writes(name, handlers, 0) { hit = true; } }
        Expr::FnCall { name, .. } if super::names::EFFECT_BUILTINS.contains(&name.as_str()) => hit = true,
        Expr::FnCall { name, .. } if !super::names::builtin_names().contains(name.as_str()) && name != "_coalesce" => hit = true,
        Expr::MethodCall { method, .. } if matches!(method.as_str(), "set" | "put" | "delete" | "remove" | "push" | "append" | "clear") => hit = true,
        Expr::MethodCall { target, .. } if matches!(&target.node, Expr::Ident(t) if super::names::cellish(t)) => hit = true,
        _ => {}
    });
    super::literals::for_each_stmt_in_expr(e, &mut |st| match st {
        Statement::MethodCall { .. } | Statement::IndexSet { .. } | Statement::Emit { .. } => hit = true,
        _ => {}
    });
    hit
}

/// Does handler `name` (or a handler it calls) write state or emit?
fn callee_writes(name: &str, handlers: &HashMap<String, &OnSection>, depth: usize) -> bool {
    let Some(on) = handlers.get(name) else { return true };
    if depth > 6 { return true; }
    let mut hit = false;
    super::literals::for_each_stmt_deep(&on.body, &mut |st| match st {
        Statement::MethodCall { method, .. } if matches!(method.as_str(), "set" | "put" | "delete" | "remove" | "push" | "append" | "clear") => hit = true,
        Statement::IndexSet { .. } | Statement::Emit { .. } => hit = true,
        _ => {}
    });
    if hit { return true; }
    super::literals::for_each_expr(&on.body, &mut |x| match x {
        Expr::MethodCall { method, .. } if matches!(method.as_str(), "set" | "put" | "delete" | "remove" | "push" | "append" | "clear") => hit = true,
        Expr::FnCall { name: n, .. } if n == "transition" || n == "remember" || n == "delegate" => hit = true,
        // another cell's handler (`B.h()`), or a bare name that is one
        Expr::MethodCall { target, .. } if matches!(&target.node, Expr::Ident(t) if super::names::cellish(t)) => hit = true,
        Expr::FnCall { name: n, .. } if !handlers.contains_key(n) && !super::names::builtin_names().contains(n.as_str()) && n != "_coalesce" => hit = true,
        Expr::FnCall { name: n, .. } if handlers.contains_key(n) && n != name => { if callee_writes(n, handlers, depth + 1) { hit = true; } }
        _ => {}
    });
    hit
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
    target: Option<&str>,
) -> HashMap<String, Known> {
    let mut assigns: Vec<(&str, &Expr)> = Vec::new();
    collect_assigns(&on.body, &mut assigns);
    let local_names: HashSet<&str> = assigns.iter().map(|(n, _)| *n).collect();
    let in_loop = write_path.iter().any(|step| step % 4 == 2 || *step == usize::MAX / 2);
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

    // Known BEFORE any range is computed (a local's range was first worked
    // out with the outer `v`, then `v` was declared a binder: `let v = 5
    // let u = match x { v -> v }` kept [5, 5] for u):
    // — the names a lambda / match / nested block ALSO binds, and the
    //   `let`s of expression blocks (`if c { let v = x  v }`): unknown;
    // — the names that may hold a Float (`x % 100` is below 100, not 99).
    let mut early: HashSet<String> = HashSet::new();
    collect_binders_stmts(&on.body, &mut early);
    crate::checker::literals::for_each_expr(&on.body, &mut |e| crate::checker::literals::for_each_stmt_in_expr(e, &mut |st| {
        if let Statement::Let { name, .. } = st { early.insert(name.clone()); }
    }));
    let mut marks: HashMap<String, Known> = HashMap::new();
    for p in &on.params {
        if !matches!(&p.ty.node, TypeExpr::Simple(t) if t == "Int") { marks.insert(format!("__float__{}", p.name), Known::Exact(1.0)); }
    }
    for b in &early { marks.insert(format!("__float__{}", b), Known::Exact(1.0)); }
    loop {
        let before = marks.len();
        for (name, value) in &assigns {
            if !params.contains(name) && may_be_float(value, &marks) { marks.insert(format!("__float__{}", name), Known::Exact(1.0)); }
        }
        if marks.len() == before { break; }
    }

    // every name the handler binds (`__local__x`): `L.bal()` on a local or
    // a parameter `L` is a method on that value, not the cell `L`
    for p in &on.params { marks.insert(format!("__local__{}", p.name), Known::Exact(1.0)); }
    for (name, _) in &assigns { marks.insert(format!("__local__{}", name), Known::Exact(1.0)); }
    for b in &early { marks.insert(format!("__local__{}", b), Known::Exact(1.0)); }
    crate::checker::literals::for_each_stmt_deep(&on.body, &mut |st| if let Statement::For { var, .. } = st { marks.insert(format!("__local__{}", var), Known::Exact(1.0)); });

    let mut vars: HashMap<String, Known> = marks.clone();
    for round in 0..4 {
        let mut next: HashMap<String, Known> = marks.clone();
        for (name, value) in &assigns {
            if params.contains(name) || early.contains(*name) {
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
        if params.contains(name) || early.contains(*name) { continue; }
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
        // (`BigInt` is a name the run does not check: a value of that parameter may be 0.5)
        .filter(|p| matches!(&p.ty.node, TypeExpr::Simple(t) if t == "Int"))
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
                    // …and a count (`len(rows)`, `rows.size`): an Int, so the
                    // early exit `if len(rows) >= 50 { return }` narrows it
                    // for the writes after, as `require len(rows) < 50` does
                    let nan_free = |e: &Expr, vars: &HashMap<String, Known>| const_of(e).is_some() || int_expr(e, &single, &int_params, 0)
                        || ((is_count(e) || matches!(e, Expr::Ident(_) | Expr::FnCall { .. })) && known_int(e, &single, &int_params, &dup, 0)) || match e {
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
                    if used.iter().all(|u| !dup.contains(u.as_str()) && !reassigned_param(u) && !part_written.contains(u.as_str()))
                        && expr_stable(left, on, handlers, &local_names, target, in_loop) && expr_stable(right, on, handlers, &local_names, target, in_loop)
                    {
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
                            // Field / Index targets included, and none whose parts
                            // are written (`require cur.amount - n >= 0` then
                            // `cur.amount = 0` was still a fact at the write)
                            let mut used: HashSet<String> = HashSet::new();
                            crate::interpreter::free_names_expr(e, &mut used);
                            if used.iter().any(|u| dup.contains(u.as_str()) || reassigned_param(u)
                                || (part_written.contains(u.as_str()) && (params.contains(u.as_str()) || local_names.contains(u.as_str())))) { continue; }
                            // re-evaluated at the write: it must give the same value
                            if !expr_stable(e, on, handlers, &local_names, target, in_loop) { continue; }
                            let (lo, hi) = match op {
                                CmpOp::Lt => (f64::NEG_INFINITY, if known_int(e, &single, &int_params, &dup, 0) && c.fract() == 0.0 { c - 1.0 } else { c.next_down() }),
                                CmpOp::Le => (f64::NEG_INFINITY, c),
                                CmpOp::Gt => (if known_int(e, &single, &int_params, &dup, 0) && c.fract() == 0.0 { c + 1.0 } else { c.next_up() }, f64::INFINITY),
                                CmpOp::Ge => (c, f64::INFINITY),
                                CmpOp::Eq => (c, c),
                                CmpOp::Ne => continue,
                            };
                            let key = format!("__expr__{}", render_expr(e));
                            // (a text that leaves a body out is no one expression's)
                            if key.contains('…') { continue; }
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
                    Some(value) => known_int(value, &single, &int_params, &dup, 0),
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
    binders.extend(early.iter().cloned());
    for b in &binders { vars.insert(b.clone(), Known::Unknown); }
    // names that may hold a Float (`__float__x`): `x % 3` is below 3, not
    // below 2, when x is 2.5 — a parameter not typed Int, a binder, and a
    // local any of whose values may be a Float
    for p in &on.params {
        if !matches!(&p.ty.node, TypeExpr::Simple(t) if t == "Int") { vars.insert(format!("__float__{}", p.name), Known::Exact(1.0)); }
    }
    for b in &binders { vars.insert(format!("__float__{}", b), Known::Exact(1.0)); }
    loop {
        let before = vars.len();
        for (name, value) in &assigns {
            if !params.contains(name) && may_be_float(value, &vars) { vars.insert(format!("__float__{}", name), Known::Exact(1.0)); }
        }
        if vars.len() == before { break; }
    }
    vars
}

/// Can `e` be a Float, given the names already marked `__float__`? Int
/// literals, Int-typed names and `+ - * %` over them cannot; a Float
/// literal, `/`, a call or a field read may.
fn may_be_float(e: &Expr, vars: &HashMap<String, Known>) -> bool {
    match e {
        Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::BigInt(_)) | Expr::Literal(Literal::Bool(_)) | Expr::Literal(Literal::String(_)) => false,
        Expr::Ident(n) => vars.contains_key(&format!("__float__{}", n)),
        Expr::BinaryOp { left, op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Mod, right } => may_be_float(&left.node, vars) || may_be_float(&right.node, vars),
        Expr::FnCall { name, .. } => !matches!(name.as_str(), "len" | "idiv" | "mod" | "to_int" | "floor" | "ceil" | "round" | "ipow" | "abs_int" | "count" | "size"),
        _ => true,
    }
}

/// `if a || b { return … }` after which ¬a ∧ ¬b holds: the negated leaves.
/// `&&` cannot be split (¬(a ∧ b) is a disjunction) and yields nothing.
/// The comparisons an `if` condition guarantees inside its branch (the
/// `&&` spine only).
/// `len(x)` / `size(x)` / `count(x)` / `x.size` / `x.len` / `x.size()`: a count
fn is_count(e: &Expr) -> bool {
    match e {
        Expr::FnCall { name, args } => matches!(name.as_str(), "len" | "size" | "count") && args.len() == 1,
        Expr::FieldAccess { field, .. } => matches!(field.as_str(), "size" | "len" | "count" | "length"),
        Expr::MethodCall { method, args, .. } => matches!(method.as_str(), "size" | "len" | "count") && args.is_empty(),
        _ => false,
    }
}

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
    /// handlers of the cell being verified (a call of one is no builtin)
    static CELL_HANDLERS: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new());
    /// Every handler name of the program: `x.count()` runs a handler
    /// `count` of ANY cell (method form), whatever the builtin of that name answers.
    static PROGRAM_HANDLERS: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new());
    /// parameters of the handler being analysed that are NOT Int
    static NON_INT_PARAMS: std::cell::RefCell<HashSet<String>> = std::cell::RefCell::new(HashSet::new());
}

/// Is `expr` KNOWN to be an Int — so `require e < 10` means `e <= 9`?
/// Int literals, Int parameters, once-bound locals over them, reads of an
/// Int slot, Int-returning builtins and `+ - * %` of known Ints. Anything
/// else (a parse_float(), a field of a Map, a list element, a random()…)
/// may be 9.5: no integer tightening.
fn known_int(expr: &Expr, single: &HashMap<&str, &Expr>, int_params: &HashSet<&str>, dup: &HashSet<&str>, depth: usize) -> bool {
    if depth > 8 { return false; }
    let ki = |e: &Expr| known_int(e, single, int_params, dup, depth + 1);
    let slot_int = |target: &Expr| -> bool {
        let Expr::Ident(n) = target else { return false };
        SLOT_INT.with(|m| m.borrow().get(n).copied()).unwrap_or(false) && !single.contains_key(n.as_str()) && !int_params.contains(n.as_str())
    };
    match expr {
        Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::BigInt(_)) => true,
        Expr::Ident(n) if dup.contains(n.as_str()) => false,
        Expr::Ident(n) => int_params.contains(n.as_str()) || single.get(n.as_str()).is_some_and(|v| ki(v)),
        Expr::MethodCall { target, method, args } if method == "get" => slot_int(&target.node) && args.get(1).map_or(true, |d| ki(&d.node)),
        Expr::MethodCall { method, .. } => matches!(method.as_str(), "len" | "size" | "count")
            && !PROGRAM_HANDLERS.with(|h| h.borrow().contains(method.as_str())),
        // `rows.size` / `rows.len` of a memory slot counts its entries, like
        // `len(rows)` (`require rows.size < 50` gave the bound 49.99…, and
        // 49.99… + 1 > 50 left the size invariant unproven)
        Expr::FieldAccess { target, field } if matches!(field.as_str(), "size" | "len" | "count" | "length") => match &target.node {
            Expr::Ident(n) => SLOT_INT.with(|m| m.borrow().contains_key(n.as_str())) && !single.contains_key(n.as_str()) && !int_params.contains(n.as_str()) && !dup.contains(n.as_str()),
            _ => false,
        },
        Expr::Index { target, .. } => slot_int(&target.node),
        Expr::BinaryOp { left, op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Mod, right } => ki(&left.node) && ki(&right.node),
        // a handler of the cell named like a builtin may return anything
        Expr::FnCall { name, .. } if CELL_HANDLERS.with(|h| h.borrow().contains(name.as_str())) => false,
        Expr::FnCall { name, .. } if PROGRAM_HANDLERS.with(|h| h.borrow().contains(name.as_str())) => false,
        Expr::FnCall { name, args } => match name.as_str() {
            // round(x, digits) is a Float (round(9.95, 1) == 10.0 but
            // round(9.94, 1) == 9.9): only the one-argument form is an Int
            "round" => args.len() == 1,
            "len" | "count" | "size" | "str_len" | "to_int" | "floor" | "ceil" | "idiv" | "mod" | "floor_div" | "ipow" | "abs_int" => true,
            "abs" | "min" | "max" | "clamp" | "_coalesce" => !args.is_empty() && args.iter().all(|a| ki(&a.node)),
            _ => false,
        },
        Expr::IfExpr { then_result, else_result, .. } => ki(&then_result.node) && ki(&else_result.node),
        Expr::Match { arms, .. } => !arms.is_empty() && arms.iter().all(|a| ki(&a.result.node)),
        _ => false,
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
            // `require len(try { v = n }) > 0 else Never` reassigns v too
            Statement::Require { constraint, .. } => {
                fn con<'e>(c: &'e Constraint, out: &mut Vec<(&'e str, &'e Expr)>) {
                    match c {
                        Constraint::Comparison { left, right, .. } => { expr_block_assigns(&left.node, out); expr_block_assigns(&right.node, out); }
                        Constraint::And(a, b) | Constraint::Or(a, b) => { con(&a.node, out); con(&b.node, out); }
                        Constraint::Not(i) => con(&i.node, out),
                        _ => {}
                    }
                }
                con(&constraint.node, out);
            }
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
        // (a slot named like a binding — `key`, `size`, `status` — is that
        // binding in its invariants at run time: only `value` is the value then)
        Expr::Ident(n) if (n == slot && !matches!(slot, "key" | "size" | "status" | "_slot_len")) || n == "value" => Some(false),
        Expr::FnCall { name, args } if name == "abs" && args.len() == 1 => {
            match &args[0].node {
                Expr::Ident(n) if (n == slot && !matches!(slot, "key" | "size" | "status" | "_slot_len")) || n == "value" => Some(true),
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
                // the condition runs on every pass: a write there is in the loop
                collect_writes_expr(&condition.node, handler, in_try, &sub(i, 2), out);
                collect_writes_stmts_at(body, handler, in_try, &sub(i, 2), out);
            }
            Statement::For { iter, body, .. } => {
                collect_writes_expr(&iter.node, handler, in_try, &sub(i, 3), out);
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
        // `xs[bal.set(k, -5) ?? 0]`: the index runs too ("no handler writes")
        Expr::Index { target, index } => {
            collect_writes_expr(&target.node, handler, in_try, path, out);
            collect_writes_expr(&index.node, handler, in_try, path, out);
        }
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
                            "`invariant {}` names no slot, so it guards EVERY slot of this memory ({}) — name the slot it is about (`{}` is the value being written — `get(key)` inside an invariant reads the value BEFORE the write — and `len({})` its size), or move it to a memory of its own",
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

pub(crate) fn collect_binders_stmts(stmts: &[Spanned<Statement>], out: &mut HashSet<String>) {
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
            // (a `require`'s constraint holds expressions too)
            Statement::Require { .. } => crate::checker::literals::for_each_expr(std::slice::from_ref(st), &mut |e| {
                if matches!(e, Expr::LambdaBlock { .. } | Expr::Lambda { .. } | Expr::Match { .. } | Expr::IfExpr { .. }) { collect_binders_expr(e, out); }
            }),
            _ => {}
        }
    }
}

/// `let`s of a block nested INSIDE an expression (an if-expression, a match
/// arm, a lambda block): they shadow outer names — a slot included — for
/// the rest of that block.
fn nested_lets(stmts: &[Spanned<Statement>], out: &mut HashSet<String>) {
    // at any depth: `if c { if d { let x = … } }` inside an expression
    // block hides the outer `x` as well
    crate::checker::literals::for_each_stmt_deep(stmts, &mut |st| {
        if let Statement::Let { name, .. } = st { out.insert(name.clone()); }
    });
}

pub(crate) fn collect_binders_expr(e: &Expr, out: &mut HashSet<String>) {
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
fn key_presence(e: &Expr, slot: &str, negate: bool, aliases: &HashMap<String, (String, Option<String>)>) -> Option<(String, bool)> {
    let get_key = |t: &Expr| -> Option<String> {
        match t {
            Expr::MethodCall { target, method, args } if (method == "get" || method == "has") && args.len() == 1
                && matches!(&target.node, Expr::Ident(n) if n == slot) => Some(render_expr(&args[0].node)),
            // `let r = rows.get(k)  require r != ()` (r bound once)
            Expr::Ident(n) => aliases.get(n).filter(|(_, d)| d.is_none()).map(|(k, _)| k.clone()),
            _ => None,
        }
    };
    // `let r = rows.get(k) ?? d` (d a literal, r bound once): `r != d` means
    // get gave a value, so k exists. `r == d` does NOT mean k is absent (an
    // entry may hold d) — but the callers only ever use the PRESENT side:
    // (k, false) puts the fact on the `else` branch / after an early exit,
    // where `r == d` is false
    let coalesced = |l: &Expr, r: &Expr| -> Option<String> {
        let (Expr::Ident(n), Expr::Literal(_)) = (l, r) else { return None };
        match aliases.get(n) { Some((k, Some(d))) if *d == render_expr(r) => Some(k.clone()), _ => None }
    };
    match e {
        Expr::CmpOp { left, op, right } if coalesced(&left.node, &right.node).or_else(|| coalesced(&right.node, &left.node)).is_some() => {
            let k = coalesced(&left.node, &right.node).or_else(|| coalesced(&right.node, &left.node))?;
            match op {
                CmpOp::Ne => Some((k, !negate)),
                CmpOp::Eq => Some((k, negate)),
                _ => None,
            }
        }
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
fn existing_key_facts(stmts: &[Spanned<Statement>], path: &[usize], slot: &str, aliases: &HashMap<String, (String, Option<String>)>) -> Vec<(Vec<usize>, String, Option<Vec<usize>>, Option<usize>)> {
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
                    Expr::FnCall { name, args } => name == "keys" && args.len() == 1 && matches!(&args[0].node, Expr::Ident(n) if n == slot)
                        && !CELL_HANDLERS.with(|h| h.borrow().contains("keys")),
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
    // the names these statements bind: one of them where a handler's or a
    // cell's name is expected is a VALUE (`vote(h, …)` with a local `h`,
    // `Aux.f` with a local `Aux`) — any handler, then
    let mut bound: HashSet<String> = HashSet::new();
    collect_binders_stmts(stmts, &mut bound);
    crate::checker::literals::for_each_stmt_deep(stmts, &mut |st| match st {
        Statement::Let { name, .. } | Statement::Assign { name, .. } => { bound.insert(name.clone()); }
        Statement::For { var, .. } => { bound.insert(var.clone()); }
        _ => {}
    });
    // `x.h()` on such a local is `h(x)`, whatever slot or cell bears the name `x`
    crate::checker::literals::for_each_expr(stmts, &mut |e| if let Expr::MethodCall { target, method, .. } = e {
        if matches!(&target.node, Expr::Ident(t) if bound.contains(t)) && cell_handlers.values().any(|hs| hs.contains(method)) { out.push((None, method.clone())); }
    });
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
        // (a size bound was "proven" past a vote whose target wrote the slot);
        // `horde(h, inputs)` runs h once per input, and its callbacks — a
        // horde whose task deleted the required key "proved" a size bound
        if (name == "vote" && args.len() == 3) || name == "horde" {
            let mut target = |e: &Expr| match e {
                Expr::FieldAccess { target, field } => match &target.node {
                    Expr::Ident(c) if !bound.contains(c) => out.push((Some(c.clone()), field.clone())),
                    _ => for (c, hs) in cell_handlers { for h in hs { out.push((Some(c.clone()), h.clone())); } },
                },
                // a bare name that is no handler is a local holding the
                // target's name: any handler
                Expr::Ident(h) if !bound.contains(h) && cell_handlers.values().any(|hs| hs.contains(h)) => out.push((None, h.clone())),
                Expr::Literal(Literal::String(t)) => match t.split_once('.') {
                    Some((c, h)) => out.push((Some(c.to_string()), h.to_string())),
                    None => out.push((None, t.clone())),
                },
                _ => for (c, hs) in cell_handlers { for h in hs { out.push((Some(c.clone()), h.clone())); } },
            };
            if let Some(a) = args.first() { target(&a.node); }
            if name == "horde" {
                match args.get(2).map(|a| &a.node) {
                    Some(Expr::FnCall { name: m, args: kv }) if m == "map" => for c in kv.chunks(2) {
                        match (&c[0].node, c.get(1).map(|v| &v.node)) {
                            (Expr::Literal(Literal::String(k)), Some(v)) if matches!(k.as_str(), "on_result" | "apply" | "on_done" | "on_error") => target(v),
                            (Expr::Literal(Literal::String(_)), _) => {}
                            // a computed key may name a callback
                            (_, Some(v)) => target(v),
                            _ => {}
                        }
                    },
                    // computed options: any callback
                    Some(_) => for (c, hs) in cell_handlers { for h in hs { out.push((Some(c.clone()), h.clone())); } },
                    None => {}
                }
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

/// The values a slot type admits include a Float: `Map<String, Float>`, and
/// also a slot with no value type (`Map`, `Any`, `List`) — its NaN write
/// (`abs(sqrt(-1.0))`) passed a `>= 0` "proven" by the interval alone.
fn may_hold_float(t: &TypeExpr) -> bool {
    match t {
        TypeExpr::Simple(n) => matches!(n.as_str(), "Float" | "Any" | "Map" | "List"),
        TypeExpr::Generic { name, args } if name == "Map" => args.last().map_or(true, |a| may_hold_float(&a.node)),
        TypeExpr::Generic { name, args } if name == "List" => args.first().map_or(true, |a| may_hold_float(&a.node)),
        _ => type_mentions_float(t),
    }
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

// ── 3.0: face contracts (`requires` / `ensures`) ────────────────────
//
// The interpreter evaluates a contract around every call, so both halves
// are facts wherever the call returned: the handler may assume `requires`
// (it is inlined as a leading `require`, see contracts::inline_requires),
// and a caller — in this cell or another — may assume `ensures` of the
// result. What is left to PROVE is that neither check can fail: each
// `return` against `ensures`, each call site against `requires`.

thread_local! {
    /// For the cell being analysed: what a call is known to return, from
    /// the callee's `ensures` — keyed `name` for a bare call (resolved as
    /// the runtime resolves it) and `Cell.name` for a qualified one.
    static CONTRACT_RESULTS: std::cell::RefCell<HashMap<String, (f64, f64)>> = std::cell::RefCell::new(HashMap::new());
}

/// The interval the `ensures` clauses of `sig` give its result.
fn ensured_interval(sig: &SignalDecl) -> Option<(f64, f64)> {
    // a parameter named like the subject would be read as the result
    if sig.params.iter().any(|p| p.name == "result" || p.name == "value") { return None; }
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for clause in &sig.ensures {
        for c in conjuncts(&clause.node) {
            if !deep_idents(c).iter().all(|n| n == "result") { continue; }
            if let Some((l, h)) = interval_of(c, "result") {
                lo = lo.max(l);
                hi = hi.min(h);
            }
        }
    }
    if lo.is_infinite() && hi.is_infinite() || lo > hi { None } else { Some((lo, hi)) }
}

/// The contracted signal a call from `cell` reaches: its own handler first,
/// else the one other cell that handles the name (two would be ambiguous).
fn callee_of<'p>(program: &'p Program, cell: &CellDef, target: Option<&str>, name: &str) -> Option<(&'p CellDef, &'p SignalDecl)> {
    let cells = crate::checker::names::collect_cells(program);
    let handles = |c: &CellDef| c.sections.iter().any(|s| matches!(&s.node, Section::OnSignal(on) if on.signal_name == name));
    let owner: &CellDef = match target {
        Some(t) => *cells.iter().find(|c| c.name == t)?,
        None => match cells.iter().find(|c| c.name == cell.name && handles(c)) {
            Some(c) => *c,
            None => {
                // (a bare call goes to a top-level cell: an interior cell's homonym is not its target)
                let top = |c: &CellDef| program.cells.iter().any(|t| std::ptr::eq(&t.node, c));
                let mut others = cells.iter().filter(|c| matches!(c.kind, CellKind::Cell | CellKind::Agent) && handles(c) && top(c));
                let first = *others.next()?;
                if others.next().is_some() { return None; }
                first
            }
        },
    };
    let (_, sig, on) = super::contracts::contracted(owner).into_iter().find(|(_, s, _)| s.name == name)?;
    on?;
    Some((owner, sig))
}

/// Load CONTRACT_RESULTS for `cell` (cleared first: it is per cell).
fn set_contract_results(program: &Program, cell: &CellDef) {
    let mut table: HashMap<String, (f64, f64)> = HashMap::new();
    let refused: HashSet<usize> = super::contracts::check_program(program).iter().map(|i| i.span.start).collect();
    let cells = crate::checker::names::collect_cells(program);
    let mut names: HashSet<&str> = HashSet::new();
    for c in &cells {
        for (decl, sig, on) in super::contracts::contracted(c) {
            if on.is_none() || refused.contains(&decl.span.start) || sig.ensures.iter().any(|e| refused.contains(&e.span.start)) { continue; }
            let Some(iv) = ensured_interval(sig) else { continue };
            table.insert(format!("{}.{}", c.name, sig.name), iv);
            names.insert(sig.name.as_str());
        }
    }
    for name in names {
        // a builtin's name is the builtin for the arities the handler does not take
        if builtin_names().contains(name) { continue; }
        if let Some((owner, sig)) = callee_of(program, cell, None, name) {
            if let Some(iv) = table.get(&format!("{}.{}", owner.name, sig.name)).copied() { table.insert(name.to_string(), iv); }
        }
    }
    CONTRACT_RESULTS.with(|t| *t.borrow_mut() = table);
}

/// `base`, narrowed by what the callee's face ensures of the result.
fn with_contract(base: Known, key: &str) -> Known {
    match CONTRACT_RESULTS.with(|t| t.borrow().get(key).copied()) {
        Some((lo, hi)) => match bounds(base) {
            Some((l, h)) if l.max(lo) <= h.min(hi) => mk(l.max(lo), h.min(hi)),
            _ => mk(lo, hi),
        },
        None => base,
    }
}

/// The slot bounds of `cell` (the induction hypothesis of the invariant
/// proof): a value read from a guarded slot satisfied its invariants.
fn slot_hypotheses(cell: &CellDef) -> HashMap<String, (f64, f64)> {
    let mut hyp: HashMap<String, (f64, f64)> = HashMap::new();
    for section in &cell.sections {
        let Section::Memory(mem) = &section.node else { continue };
        let slot_names: Vec<String> = mem.slots.iter().map(|s| s.node.name.clone()).collect();
        for inv in &mem.invariants {
            let refs = deep_idents(&inv.node);
            if refs.contains("status") { continue; }
            let named: Vec<String> = slot_names.iter().filter(|n| refs.contains(*n)).cloned().collect();
            let targets = if named.is_empty() { slot_names.clone() } else { named };
            let norm = normalize_invariant(&inv.node, &slot_names);
            for slot in &targets {
                let (mut lo, mut hi) = hyp.get(slot).copied().unwrap_or((f64::NEG_INFINITY, f64::INFINITY));
                for c in conjuncts(&norm) {
                    if let Some((l, h)) = interval_of(c, slot) {
                        lo = lo.max(l);
                        hi = hi.min(h);
                    }
                }
                hyp.insert(slot.clone(), (lo, hi));
            }
        }
    }
    hyp
}

/// The values a handler can return, each with the block path it sits on.
/// `tail`: the block's last expression is the handler's value.
fn collect_returns_at<'e>(stmts: &'e [Spanned<Statement>], path: &[usize], tail: bool, out: &mut Vec<(&'e Expr, Vec<usize>, bool)>) {
    let sub = |i: usize, b: usize| -> Vec<usize> { let mut v = path.to_vec(); v.push(block_step(i, b)); v };
    for (i, st) in stmts.iter().enumerate() {
        let last = i + 1 == stmts.len();
        match &st.node {
            Statement::Return { value } => out.push((&value.node, sub(i, 3), true)),
            Statement::ExprStmt { expr } if last && tail => out.push((&expr.node, sub(i, 3), false)),
            Statement::If { then_body, else_body, .. } => {
                collect_returns_at(then_body, &sub(i, 0), tail && last, out);
                collect_returns_at(else_body, &sub(i, 1), tail && last, out);
            }
            Statement::While { body, .. } | Statement::For { body, .. } => collect_returns_at(body, &sub(i, 2), false, out),
            _ => {}
        }
    }
}

/// A call found in a handler body: where, to whom, with what, and whether a
/// `try` holds it (a refusal there is the caller's intent).
struct CallSite<'e> {
    target: Option<String>,
    name: String,
    args: Vec<&'e Expr>,
    path: Vec<usize>,
    in_try: bool,
}

fn collect_calls_at<'e>(stmts: &'e [Spanned<Statement>], path: &[usize], out: &mut Vec<CallSite<'e>>) {
    let sub = |i: usize, b: usize| -> Vec<usize> { let mut v = path.to_vec(); v.push(block_step(i, b)); v };
    fn in_expr<'e>(e: &'e Expr, path: &[usize], out: &mut Vec<CallSite<'e>>) {
        // the calls a `try { }` holds, by address
        let mut tried: HashSet<*const Expr> = HashSet::new();
        crate::checker::literals::for_each_in_expr(e, &mut |x| if let Expr::Try(inner) = x {
            crate::checker::literals::for_each_in_expr(&inner.node, &mut |y| { tried.insert(y as *const Expr); });
        });
        crate::checker::literals::for_each_in_expr(e, &mut |x| match x {
            Expr::FnCall { name, args } => out.push(CallSite { target: None, name: name.clone(), args: args.iter().map(|a| &a.node).collect(), path: path.to_vec(), in_try: tried.contains(&(x as *const Expr)) }),
            Expr::MethodCall { target, method, args } => if let Expr::Ident(t) = &target.node {
                out.push(CallSite { target: Some(t.clone()), name: method.clone(), args: args.iter().map(|a| &a.node).collect(), path: path.to_vec(), in_try: tried.contains(&(x as *const Expr)) });
            },
            _ => {}
        });
    }
    for (i, st) in stmts.iter().enumerate() {
        match &st.node {
            Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => in_expr(&value.node, &sub(i, 3), out),
            Statement::ExprStmt { expr } => in_expr(&expr.node, &sub(i, 3), out),
            Statement::Ensure { condition } => in_expr(&condition.node, &sub(i, 3), out),
            Statement::If { condition, then_body, else_body } => {
                in_expr(&condition.node, &sub(i, 3), out);
                collect_calls_at(then_body, &sub(i, 0), out);
                collect_calls_at(else_body, &sub(i, 1), out);
            }
            Statement::While { condition, body, .. } => {
                in_expr(&condition.node, &sub(i, 2), out);
                collect_calls_at(body, &sub(i, 2), out);
            }
            Statement::For { iter, body, .. } => {
                in_expr(&iter.node, &sub(i, 3), out);
                collect_calls_at(body, &sub(i, 2), out);
            }
            Statement::IndexSet { index, value, .. } => { in_expr(&index.node, &sub(i, 3), out); in_expr(&value.node, &sub(i, 3), out); }
            Statement::MethodCall { target, method, args } => {
                out.push(CallSite { target: Some(target.clone()), name: method.clone(), args: args.iter().map(|a| &a.node).collect(), path: sub(i, 3), in_try: false });
                for a in args { in_expr(&a.node, &sub(i, 3), out); }
            }
            Statement::Emit { args, .. } => for a in args { in_expr(&a.node, &sub(i, 3), out); },
            Statement::Require { constraint, .. } => for (l, _, r) in constraint_comparisons(&constraint.node) { in_expr(l, &sub(i, 3), out); in_expr(r, &sub(i, 3), out); },
            _ => {}
        }
    }
}

/// Can `e` be `()` where it is returned — a slot read with no default, or a
/// local bound to one that the handler never compares with `()`?
fn unit_possible(e: &Expr, on: &OnSection) -> bool {
    fn direct(e: &Expr) -> bool {
        match e {
            Expr::MethodCall { method, args, .. } if method == "get" => args.get(1).map_or(true, |d| direct(&d.node) || matches!(d.node, Expr::Literal(Literal::Unit))),
            Expr::Index { .. } | Expr::Literal(Literal::Unit) => true,
            Expr::IfExpr { then_result, else_result, .. } => direct(&then_result.node) || direct(&else_result.node),
            Expr::Match { arms, .. } => arms.iter().any(|a| direct(&a.result.node)),
            _ => false,
        }
    }
    if direct(e) { return true; }
    let Expr::Ident(n) = e else { return false };
    let mut assigns: Vec<(&str, &Expr)> = Vec::new();
    collect_assigns(&on.body, &mut assigns);
    if !assigns.iter().any(|(m, v)| m == n && direct(v)) { return false; }
    let mut tested = false;
    crate::checker::literals::for_each_expr(&on.body, &mut |x| if let Expr::CmpOp { left, right, .. } = x {
        let is = |a: &Expr, b: &Expr| matches!(a, Expr::Ident(m) if m == n) && matches!(b, Expr::Literal(Literal::Unit));
        if is(&left.node, &right.node) || is(&right.node, &left.node) { tested = true; }
    });
    !tested
}

/// The proof pass: one block per cell whose face carries a contract.
/// `program` is the source as written; the analysis runs on the copy with
/// each `requires` inlined, so a handler's own writes and returns see it.
pub fn verify_contracts(program: &Program) -> Vec<VerifyResult> {
    PROGRAM_HANDLERS.with(|h| *h.borrow_mut() = super::desugar::program_names(program).0);
    let inlined = super::contracts::inline_requires(program);
    let refused: HashSet<usize> = super::contracts::check_program(program).iter().map(|i| i.span.start).collect();
    let cells = crate::checker::names::collect_cells(&inlined);
    if !cells.iter().any(|c| !super::contracts::contracted(c).is_empty()) { return Vec::new(); }

    // every call site of the program that reaches a contracted signal:
    // (callee cell, callee signal) → (caller, clause text, verdict, detail)
    let mut sites: HashMap<(String, String), Vec<(String, String, Proof, String)>> = HashMap::new();
    for cell in &cells {
        if !matches!(cell.kind, CellKind::Cell | CellKind::Agent) { continue; }
        set_slot_int(cell);
        set_contract_results(&inlined, cell);
        let hyp = slot_hypotheses(cell);
        let handlers: HashMap<String, &OnSection> = cell.sections.iter()
            .filter_map(|s| if let Section::OnSignal(on) = &s.node { Some((on.signal_name.clone(), on)) } else { None })
            .collect();
        // sites this reading does not judge: an `emit` (every listener of that
        // name runs with its arguments) and the calls of `every` / `after` blocks
        let unjudged = |caller: &str, body: &[Spanned<Statement>], sites: &mut HashMap<(String, String), Vec<(String, String, Proof, String)>>, all_calls: bool| {
            crate::checker::literals::for_each_stmt_deep(body, &mut |st| if let Statement::Emit { signal_name, args } = st {
                for other in &cells {
                    for (_, sig, _) in super::contracts::contracted(other) {
                        if &sig.name != signal_name || sig.requires.is_empty() { continue; }
                        let shown = format!("emit {}({})", signal_name, args.iter().map(|a| render_expr(&a.node)).collect::<Vec<_>>().join(", "));
                        for clause in &sig.requires { for c in conjuncts(&clause.node) {
                            sites.entry((other.name.clone(), sig.name.clone())).or_default().push((caller.to_string(), render_expr(c), Proof::Unknown, shown.clone()));
                        } }
                    }
                }
            });
            if all_calls {
                let mut calls: Vec<CallSite> = Vec::new();
                collect_calls_at(body, &[], &mut calls);
                for call in calls {
                    let Some((owner, sig)) = callee_of(&inlined, cell, call.target.as_deref(), &call.name) else { continue };
                    if sig.requires.is_empty() { continue; }
                    let shown = format!("{}({})", call.name, call.args.iter().map(|a| render_expr(a)).collect::<Vec<_>>().join(", "));
                    for clause in &sig.requires { for c in conjuncts(&clause.node) {
                        sites.entry((owner.name.clone(), sig.name.clone())).or_default().push((caller.to_string(), render_expr(c), Proof::Unknown, shown.clone()));
                    } }
                }
            }
        };
        for section in &cell.sections {
            if let Section::Every(e) | Section::After(e) = &section.node { unjudged("a schedule", &e.body, &mut sites, true); }
        }
        for on in handlers.values() {
            unjudged(&on.signal_name, &on.body, &mut sites, false);
            let mut calls: Vec<CallSite> = Vec::new();
            collect_calls_at(&on.body, &[], &mut calls);
            let locals: HashSet<String> = { let mut a = Vec::new(); collect_assigns(&on.body, &mut a); a.iter().map(|(n, _)| n.to_string()).chain(on.params.iter().map(|p| p.name.clone())).collect() };
            for call in calls {
                // `m.get(k)` on a local or a slot is no call into a cell — but
                // `x.f()` on a local IS the call `f(x)` when a handler is named f:
                // a site, not judged
                if call.target.as_ref().is_some_and(|t| locals.contains(t)) {
                    for other in &cells {
                        for (_, sig, _) in super::contracts::contracted(other) {
                            if sig.name != call.name || sig.requires.is_empty() { continue; }
                            let shown = format!("{}.{}(…)", call.target.as_deref().unwrap_or(""), call.name);
                            for clause in &sig.requires { for c in conjuncts(&clause.node) {
                                sites.entry((other.name.clone(), sig.name.clone())).or_default().push((on.signal_name.clone(), render_expr(c), Proof::Unknown, shown.clone()));
                            } }
                        }
                    }
                    continue;
                }
                // `vote(A.f, …)`, `horde(A.f, …, map("on_result", "f"))`: the handler
                // they name runs with arguments this reading does not follow
                if call.target.is_none() && matches!(call.name.as_str(), "vote" | "horde" | "delegate" | "resume" | "deliver") {
                    let mut named: Vec<String> = Vec::new();
                    for a in &call.args {
                        crate::checker::literals::for_each_in_expr(a, &mut |x| match x {
                            Expr::FieldAccess { field, .. } => named.push(field.clone()),
                            Expr::Ident(n) => named.push(n.clone()),
                            Expr::Literal(Literal::String(s)) => named.push(s.clone()),
                            _ => {}
                        });
                    }
                    for other in &cells {
                        for (_, sig, _) in super::contracts::contracted(other) {
                            if sig.requires.is_empty() || !named.contains(&sig.name) { continue; }
                            let shown = format!("{}(… {} …)", call.name, sig.name);
                            for clause in &sig.requires { for c in conjuncts(&clause.node) {
                                sites.entry((other.name.clone(), sig.name.clone())).or_default().push((on.signal_name.clone(), render_expr(c), Proof::Unknown, shown.clone()));
                            } }
                        }
                    }
                }
                if call.target.is_none() && builtin_names().contains(call.name.as_str())
                    && !handlers.get(&call.name).is_some_and(|h| crate::ast::accepted_arities(&h.params).contains(&call.args.len())) { continue; }
                let Some((owner, sig)) = callee_of(&inlined, cell, call.target.as_deref(), &call.name) else { continue };
                if sig.requires.is_empty() { continue; }
                // a call that leaves trailing parameters out is a call site too:
                // not judged, so not "proven at every site"
                if call.args.len() != sig.params.len() {
                    let shown = format!("{}({})", call.name, call.args.iter().map(|a| render_expr(a)).collect::<Vec<_>>().join(", "));
                    for clause in &sig.requires { for c in conjuncts(&clause.node) {
                        sites.entry((owner.name.clone(), sig.name.clone())).or_default().push((on.signal_name.clone(), render_expr(c), Proof::Unknown, shown.clone()));
                    } }
                    continue;
                }
                let hyp_h = hyp_for(on, &hyp);
                let ctx = RangeCtx { hyp: &hyp_h, vars: local_ranges_at(on, &hyp_h, &handlers, 0, &call.path, None), handlers: &handlers, depth: 0 };
                let shown = format!("{}({})", call.name, call.args.iter().map(|a| render_expr(a)).collect::<Vec<_>>().join(", "));
                for clause in &sig.requires {
                    for c in conjuncts(&clause.node) {
                        let names = deep_idents(c);
                        // one parameter against a constant: the interval domain decides it
                        let verdict = match sig.params.iter().position(|p| names.len() == 1 && names.contains(&p.name)) {
                            Some(i) if matches!(&sig.params[i].ty.node, TypeExpr::Simple(t) if t == "Int") => prove(c, &sig.params[i].name, ctx.range_of(call.args[i])),
                            _ => Proof::Unknown,
                        };
                        // a `[task]` handler's facts may be stale after a think(): not claimed
                        let verdict = if verdict == Proof::Holds && on.properties.iter().any(|p| p == "task") { Proof::Unknown } else { verdict };
                        let verdict = if verdict == Proof::Violated && call.in_try { Proof::Unknown } else { verdict };
                        sites.entry((owner.name.clone(), sig.name.clone())).or_default()
                            .push((on.signal_name.clone(), render_expr(c), verdict, shown.clone()));
                    }
                }
            }
        }
    }

    let mut results = Vec::new();
    for cell in &cells {
        let contracted = super::contracts::contracted(cell);
        if contracted.is_empty() { continue; }
        set_slot_int(cell);
        set_contract_results(&inlined, cell);
        let hyp = slot_hypotheses(cell);
        let handlers: HashMap<String, &OnSection> = cell.sections.iter()
            .filter_map(|s| if let Section::OnSignal(on) = &s.node { Some((on.signal_name.clone(), on)) } else { None })
            .collect();
        let mut result = VerifyResult {
            machine_name: format!("{}/contracts", cell.name),
            states: vec![],
            initial: String::new(),
            terminal_states: vec![],
            transitions: vec![],
            checks: vec![],
        };
        for (decl, sig, on) in contracted {
            let Some(on) = on else { continue };
            if refused.contains(&decl.span.start) || sig.requires.iter().chain(sig.ensures.iter()).any(|c| refused.contains(&c.span.start)) {
                result.checks.push(VerifyCheck::Warning(format!("{}: the contract is refused by `soma check` — nothing is proven of it", sig.name)));
                continue;
            }
            // ── requires: every call site in the program ──
            let mut by_clause: Vec<(String, Vec<&(String, String, Proof, String)>)> = Vec::new();
            let empty = Vec::new();
            let all = sites.get(&(cell.name.clone(), sig.name.clone())).unwrap_or(&empty);
            for clause in &sig.requires {
                for c in conjuncts(&clause.node) {
                    let text = render_expr(c);
                    by_clause.push((text.clone(), all.iter().filter(|s| s.1 == text).collect()));
                }
            }
            for (text, found) in by_clause {
                let failed: Vec<&&(String, String, Proof, String)> = found.iter().filter(|s| s.2 == Proof::Violated).collect();
                let open: Vec<&&(String, String, Proof, String)> = found.iter().filter(|s| s.2 == Proof::Unknown).collect();
                if let Some(f) = failed.first() {
                    result.checks.push(VerifyCheck::Fail(format!(
                        "{}: requires {} — `{}` calls {}, which can never satisfy it (the call raises kind `requires` every time)", sig.name, text, f.0, f.3), None));
                } else if found.is_empty() {
                    result.checks.push(VerifyCheck::Note(format!(
                        "{}: requires {} — no call site in the program; checked at run time on every call from outside (kind `requires`)", sig.name, text)));
                } else if open.is_empty() {
                    result.checks.push(VerifyCheck::Pass(format!(
                        "{}: requires {} — proven at {} call site{} ({}); a call from outside is checked at run time", sig.name, text, found.len(),
                        if found.len() == 1 { "" } else { "s" },
                        { let mut who: Vec<&str> = found.iter().map(|s| s.0.as_str()).collect(); who.sort(); who.dedup(); who.join(", ") })));
                } else {
                    let mut list: Vec<String> = open.iter().map(|s| format!("{} → {}", s.0, s.3)).collect();
                    list.sort();
                    list.dedup();
                    result.checks.push(VerifyCheck::Warning(format!(
                        "{}: requires {} — runtime-checked at {} of {} call site{}: {} (narrow the argument there: `require … else …` before the call; an early `if … {{ return … }}` counts only for an Int — a typed parameter or `to_int(…)` — since a value read from a Map may be a Float or NaN, which no comparison excludes)",
                        sig.name, text, open.len(), found.len(), if found.len() == 1 { "" } else { "s" }, list.join(", "))));
                }
            }
            // ── ensures: every value the handler returns ──
            if sig.ensures.is_empty() { continue; }
            // the handler as the prover reads it (its `requires` inlined)
            let mut returns: Vec<(&Expr, Vec<usize>, bool)> = Vec::new();
            collect_returns_at(&on.body, &[], true, &mut returns);
            let mut deep_returns = 0usize;
            crate::checker::literals::for_each_stmt_deep(&on.body, &mut |st| if matches!(st, Statement::Return { .. }) { deep_returns += 1; });
            let tracked = returns.iter().filter(|r| r.2).count();
            let int_result = matches!(sig.return_type.as_ref().map(|t| &t.node), Some(TypeExpr::Simple(t)) if t == "Int");
            let blocked: Option<String> = if !int_result {
                Some("only a result the face types `-> Int` is proven".to_string())
            } else if on.properties.iter().any(|p| p == "task") {
                Some("a `[task]` handler commits step by step: what it read before a think() may have changed".to_string())
            } else if deep_returns > tracked {
                Some("a `return` inside a match arm, a `try` or a lambda is not followed".to_string())
            } else if may_fall_off(&on.body) {
                Some("the handler can end without a value, and returns () then".to_string())
            } else if returns.is_empty() {
                Some("the handler returns nothing".to_string())
            } else { None };
            for clause in &sig.ensures {
                for c in conjuncts(&clause.node) {
                    let text = render_expr(c);
                    if let Some(why) = &blocked {
                        result.checks.push(VerifyCheck::Warning(format!("{}: ensures {} — runtime-checked: {}", sig.name, text, why)));
                        continue;
                    }
                    let mut open: Vec<String> = Vec::new();
                    let mut failed: Option<String> = None;
                    for (value, path, _) in &returns {
                        let hyp_h = hyp_for(on, &hyp);
                        let ctx = RangeCtx { hyp: &hyp_h, vars: local_ranges_at(on, &hyp_h, &handlers, 0, path, None), handlers: &handlers, depth: 0 };
                        let absent = unit_possible(value, on);
                        let known = if absent { Known::Unknown } else { ctx.range_of(value) };
                        // only `result` against a constant is decided here
                        let verdict = if deep_idents(c).iter().all(|n| n == "result") && !sig.params.iter().any(|p| p.name == "result" || p.name == "value") {
                            prove(c, "result", known)
                        } else { Proof::Unknown };
                        set_slot_int(cell);
                        // 3.0 (B4): a clause the intervals leave open — one
                        // that relates `result` to a parameter too — as
                        // linear arithmetic over the facts at the return
                        let verdict = match (verdict, c) {
                            (Proof::Unknown, Expr::CmpOp { left, op, right }) if !absent && !sig.params.iter().any(|p| p.name == "result")
                                && linear_entails(on, path, &left.node, *op, &right.node, Some(("result", *value)), &hyp_h, &int_returning(cell)) => Proof::Holds,
                            (v, _) => v,
                        };
                        match verdict {
                            Proof::Holds => {}
                            Proof::Violated => { failed.get_or_insert(render_expr(value)); }
                            Proof::Unknown => open.push(match bounds(known) {
                                _ if absent => format!("`{}` (may be () — a missing key)", render_expr(value)),
                                Some((lo, hi)) => format!("`{}` (known: {} to {})", render_expr(value), fmt_bound(lo), fmt_bound(hi)),
                                None => format!("`{}` (unbounded)", render_expr(value)),
                            }),
                        }
                    }
                    if let Some(v) = failed {
                        result.checks.push(VerifyCheck::Fail(format!(
                            "{}: ensures {} — the handler returns `{}`, which can never satisfy it (the call raises kind `ensures` on that path)", sig.name, text, v), None));
                    } else if open.is_empty() {
                        result.checks.push(VerifyCheck::Pass(format!(
                            "{}: ensures {} — proven for every returned value ({} return{}); callers in any cell may rely on it",
                            sig.name, text, returns.len(), if returns.len() == 1 { "" } else { "s" })));
                    } else {
                        open.sort();
                        open.dedup();
                        result.checks.push(VerifyCheck::Warning(format!(
                            "{}: ensures {} — runtime-checked: not proven for {} (callers may still rely on it: a result that breaks it is refused, kind `ensures`)",
                            sig.name, text, open.join(", "))));
                    }
                }
            }
        }
        if !result.checks.is_empty() { results.push(result); }
    }
    CONTRACT_RESULTS.with(|t| t.borrow_mut().clear());
    results
}

fn fmt_bound(v: f64) -> String {
    if v == f64::INFINITY { "+∞".to_string() } else if v == f64::NEG_INFINITY { "-∞".to_string() } else if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{}", v) }
}

// ── 3.0 (B4): linear integer arithmetic behind the intervals ───────────
//
// An interval knows `b >= 0` and `amount >= 1`; it does not know
// `b >= amount`, so `acct.set(id, b - amount)` after `require b >= amount`
// stayed "runtime-checked". Here the facts that hold at a write — the
// `require`s before it, the conditions of the `if`s around it, the negation
// of an early exit — are kept as linear constraints between the handler's
// Int values, and a clause is proven when the facts together with its
// NEGATION have no solution (Fourier–Motzkin elimination; exact over the
// integers' relaxation, so "no solution" is a proof).
//
// What is a value here: an `Int` parameter the handler never reassigns; a
// local bound once to `Slot.get(k) ?? <Int literal>` on an Int slot (it
// then satisfies the slot's interval invariant, or is the default) or to a
// call of a handler whose face says `-> Int`; and the sums, differences
// and constant multiples of those. Anything else — a Float, a division, a
// reassigned local, a value read inline — is not a value, a fact over it
// is dropped, and a goal over it is not proven. The search is bounded;
// past the bound nothing is proven.

/// Σ coef·var + constant
#[derive(Clone, Debug, Default)]
struct Linear { coef: std::collections::BTreeMap<String, i128>, constant: i128 }

impl Linear {
    fn constant(n: i128) -> Self { Linear { coef: Default::default(), constant: n } }
    fn var(name: &str) -> Self { let mut l = Linear::default(); l.coef.insert(name.to_string(), 1); l }
    fn scaled(&self, k: i128) -> Option<Self> {
        let mut out = Linear { coef: Default::default(), constant: self.constant.checked_mul(k)? };
        for (v, c) in &self.coef { let x = c.checked_mul(k)?; if x != 0 { out.coef.insert(v.clone(), x); } }
        Some(out)
    }
    fn plus(&self, other: &Linear) -> Option<Self> {
        let mut out = self.clone();
        out.constant = out.constant.checked_add(other.constant)?;
        for (v, c) in &other.coef {
            let x = out.coef.get(v).copied().unwrap_or(0).checked_add(*c)?;
            if x == 0 { out.coef.remove(v); } else { out.coef.insert(v.clone(), x); }
        }
        Some(out)
    }
    fn minus(&self, other: &Linear) -> Option<Self> { self.plus(&other.scaled(-1)?) }
}

struct LinearScope<'e> {
    /// once-bound locals and what they are bound to
    single: HashMap<&'e str, &'e Expr>,
    int_params: HashSet<&'e str>,
    /// slot → the interval its invariant gives a stored value
    hyp: &'e HashMap<String, (f64, f64)>,
    /// handlers of the cell whose face declares `-> Int`
    int_returns: &'e HashSet<String>,
    /// a name standing for an expression (`result` → the returned value, the slot → the written one)
    subst: Option<(&'e str, &'e Expr)>,
    /// opaque values met, with the bounds assumed of each
    atoms: std::cell::RefCell<std::collections::BTreeMap<String, (Option<i128>, Option<i128>)>>,
}

impl<'e> LinearScope<'e> {
    /// The bounds of a value read as `Slot.get(k) ?? default` / `Slot[k] ?? default`.
    fn slot_read(&self, e: &Expr) -> Option<(Option<i128>, Option<i128>)> {
        let Expr::FnCall { name, args } = e else { return None };
        if name != "_coalesce" || args.len() != 2 { return None; }
        let Expr::Literal(Literal::Int(default)) = &args[1].node else { return None };
        let slot = match &args[0].node {
            Expr::MethodCall { target, method, args } if method == "get" && args.len() == 1 => match &target.node { Expr::Ident(s) => s, _ => return None },
            Expr::Index { target, .. } => match &target.node { Expr::Ident(s) => s, _ => return None },
            _ => return None,
        };
        if !SLOT_INT.with(|m| m.borrow().get(slot).copied().unwrap_or(false)) { return None; }
        let d = *default as i128;
        let (lo, hi) = self.hyp.get(slot).copied().unwrap_or((f64::NEG_INFINITY, f64::INFINITY));
        let as_int = |x: f64| if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.0e15 { Some(x as i128) } else { None };
        // the stored value satisfies the invariant; a missing key gives the default
        let lo = if lo == f64::NEG_INFINITY { None } else { as_int(lo.ceil()).map(|l| l.min(d)) };
        let hi = if hi == f64::INFINITY { None } else { as_int(hi.floor()).map(|h| h.max(d)) };
        Some((lo, hi))
    }

    fn linear(&self, e: &Expr, depth: usize) -> Option<Linear> {
        if depth > 12 { return None; }
        match e {
            Expr::Literal(Literal::Int(n)) => Some(Linear::constant(*n as i128)),
            Expr::Ident(n) => {
                if let Some((name, value)) = self.subst { if n == name { return self.linear(value, depth + 1); } }
                if self.int_params.contains(n.as_str()) { return Some(Linear::var(n)); }
                let def = *self.single.get(n.as_str())?;
                if let Some(l) = self.linear(def, depth + 1) { return Some(l); }
                // an opaque Int: a slot read with its default, or a call of an `-> Int` handler
                let bounds = match def {
                    Expr::FnCall { name, .. } if self.int_returns.contains(name) => Some((None, None)),
                    other => self.slot_read(other),
                }?;
                self.atoms.borrow_mut().insert(n.clone(), bounds);
                Some(Linear::var(n))
            }
            Expr::BinaryOp { left, op: BinOp::Add, right } => self.linear(&left.node, depth + 1)?.plus(&self.linear(&right.node, depth + 1)?),
            Expr::BinaryOp { left, op: BinOp::Sub, right } => self.linear(&left.node, depth + 1)?.minus(&self.linear(&right.node, depth + 1)?),
            Expr::BinaryOp { left, op: BinOp::Mul, right } => {
                let (l, r) = (self.linear(&left.node, depth + 1)?, self.linear(&right.node, depth + 1)?);
                if l.coef.is_empty() { r.scaled(l.constant) } else if r.coef.is_empty() { l.scaled(r.constant) } else { None }
            }
            _ => None,
        }
    }

    /// `l op r` as constraints `t >= 0` (all values are Ints: `t > 0` is `t - 1 >= 0`).
    fn constraints(&self, l: &Expr, op: CmpOp, r: &Expr) -> Option<Vec<Linear>> {
        let d = self.linear(l, 0)?.minus(&self.linear(r, 0)?)?;      // l - r
        let neg = d.scaled(-1)?;                                       // r - l
        let minus_one = Linear::constant(-1);
        Some(match op {
            CmpOp::Ge => vec![d],
            CmpOp::Gt => vec![d.plus(&minus_one)?],
            CmpOp::Le => vec![neg],
            CmpOp::Lt => vec![neg.plus(&minus_one)?],
            CmpOp::Eq => vec![d, neg],
            CmpOp::Ne => return None,
        })
    }
}

/// Have these constraints (each `>= 0`) no solution? Fourier–Motzkin over
/// the rationals: `true` is a proof; `false` is "not shown" (a solution may
/// exist, or the bound on the work was reached).
fn infeasible(mut system: Vec<Linear>) -> bool {
    let contradiction = |s: &[Linear]| s.iter().any(|c| c.coef.is_empty() && c.constant < 0);
    for _ in 0..24 {
        if contradiction(&system) { return true; }
        system.retain(|c| !c.coef.is_empty());
        // the variable whose elimination makes the fewest new constraints
        let mut vars: Vec<String> = system.iter().flat_map(|c| c.coef.keys().cloned()).collect();
        vars.sort(); vars.dedup();
        let Some(var) = vars.into_iter().min_by_key(|v| {
            let pos = system.iter().filter(|c| c.coef.get(v).is_some_and(|k| *k > 0)).count();
            let neg = system.iter().filter(|c| c.coef.get(v).is_some_and(|k| *k < 0)).count();
            pos * neg
        }) else { return false };
        let (mut lower, mut upper, mut rest) = (Vec::new(), Vec::new(), Vec::new());
        for c in system {
            match c.coef.get(&var).copied() {
                Some(k) if k > 0 => lower.push(c),
                Some(_) => upper.push(c),
                None => rest.push(c),
            }
        }
        if lower.len() * upper.len() > 400 { return false; }
        for lo in &lower {
            for up in &upper {
                // a·x + p >= 0 (a > 0) and -b·x + q >= 0 (b > 0)  ⇒  b·p + a·q >= 0
                let (a, b) = (lo.coef[&var], -up.coef[&var]);
                let Some(combined) = lo.scaled(b).and_then(|x| up.scaled(a).and_then(|y| x.plus(&y))) else { return false };
                rest.push(combined);
            }
        }
        if rest.len() > 600 { return false; }
        system = rest;
    }
    contradiction(&system)
}

/// Is `lhs op rhs` implied, at the statement on `path` of `on`, by the facts
/// that hold there? `subst` names an expression (`result`, or the slot being
/// written).
fn linear_entails(
    on: &OnSection, path: &[usize], lhs: &Expr, op: CmpOp, rhs: &Expr, subst: Option<(&str, &Expr)>,
    hyp: &HashMap<String, (f64, f64)>, int_returns: &HashSet<String>,
) -> bool {
    let mut assigns: Vec<(&str, &Expr)> = Vec::new();
    collect_assigns(&on.body, &mut assigns);
    let params: HashSet<&str> = on.params.iter().map(|p| p.name.as_str()).collect();
    let mut shadowed: HashSet<String> = HashSet::new();
    collect_binders_stmts(&on.body, &mut shadowed);
    // names whose parts are written are no scalars
    let mut part_written: HashSet<String> = HashSet::new();
    crate::checker::literals::for_each_stmt_deep(&on.body, &mut |st| match st {
        Statement::IndexSet { name, .. } => { part_written.insert(name.split('.').next().unwrap_or(name).to_string()); }
        Statement::Assign { name, .. } if name.contains('.') || name.contains('[') => {
            part_written.insert(name.split(|c| c == '.' || c == '[').next().unwrap_or(name).to_string());
        }
        _ => {}
    });
    let once = |n: &str| assigns.iter().filter(|(m, _)| *m == n).count() == 1 && !shadowed.contains(n) && !part_written.contains(n) && !params.contains(n);
    let single: HashMap<&str, &Expr> = assigns.iter().filter(|(n, v)| once(n) && !std::ptr::eq(*v, &UNKNOWN_EXPR)).map(|(n, v)| (*n, *v)).collect();
    let int_params: HashSet<&str> = on.params.iter()
        // (`BigInt` is a name the run does not check: a value of that parameter may be 0.5)
        .filter(|p| matches!(&p.ty.node, TypeExpr::Simple(t) if t == "Int"))
        .map(|p| p.name.as_str())
        .filter(|n| !assigns.iter().any(|(m, _)| m == n) && !shadowed.contains(*n) && !part_written.contains(*n))
        .collect();
    let scope = LinearScope { single, int_params, hyp, int_returns, subst, atoms: Default::default() };

    // the goal's negation
    let negated = match op {
        CmpOp::Ge => vec![(CmpOp::Lt, false)], CmpOp::Gt => vec![(CmpOp::Le, false)],
        CmpOp::Le => vec![(CmpOp::Gt, false)], CmpOp::Lt => vec![(CmpOp::Ge, false)],
        // `a == b` fails as `a < b` or as `a > b`: both must be impossible
        CmpOp::Eq => vec![(CmpOp::Lt, false), (CmpOp::Gt, false)],
        CmpOp::Ne => return false,
    };
    // the facts at this statement — the same ones the interval pass takes
    // (see `local_ranges_at`): a require BEFORE it in its block or an
    // enclosing one, the condition of an `if` around it, the negation of an
    // early exit before it
    let mut collected: Vec<Fact> = Vec::new();
    collect_facts(&on.body, &[], &mut collected);
    let no_subst = LinearScope { single: scope.single.clone(), int_params: scope.int_params.clone(), hyp, int_returns, subst: None, atoms: Default::default() };
    let mut facts: Vec<Linear> = Vec::new();
    for f in &collected {
        if !path.starts_with(&f.path) { continue; }
        if let Some(ex) = &f.exclude { if path.starts_with(ex) { continue; } }
        if matches!(f.stmt.node, Statement::Require { .. }) && path.get(f.path.len()).map_or(true, |s| *s / 4 <= f.index) { continue; }
        if f.exclude.is_some() && path.get(f.path.len()).map_or(true, |s| *s / 4 <= f.index) { continue; }
        let comparisons: Vec<(&Expr, CmpOp, &Expr)> = match &f.stmt.node {
            Statement::Require { constraint, .. } => constraint_comparisons(&constraint.node),
            Statement::If { condition, .. } if f.branch == 1 => positive_comparisons(&condition.node),
            Statement::If { condition, .. } => negated_comparisons(&condition.node),
            _ => Vec::new(),
        };
        for (l, o, r) in comparisons {
            // (a fact names the handler's values, never `result` or the slot)
            if let Some(cs) = no_subst.constraints(l, o, r) { facts.extend(cs); }
        }
    }
    for (o, _) in negated {
        let Some(goal) = scope.constraints(lhs, o, rhs) else { return false };
        let mut system = facts.clone();
        system.extend(goal);
        // what is assumed of each opaque value
        let atoms: Vec<(String, (Option<i128>, Option<i128>))> = scope.atoms.borrow().iter().map(|(k, v)| (k.clone(), *v))
            .chain(no_subst.atoms.borrow().iter().map(|(k, v)| (k.clone(), *v))).collect();
        for (name, (lo, hi)) in atoms {
            if let Some(lo) = lo { if let Some(c) = Linear::var(&name).plus(&Linear::constant(-lo)) { system.push(c); } }
            if let Some(hi) = hi { if let Some(c) = Linear::var(&name).scaled(-1).and_then(|v| v.plus(&Linear::constant(hi))) { system.push(c); } }
        }
        if !infeasible(system) { return false; }
    }
    true
}

/// The handlers of `cell` whose face declares `-> Int` (the runtime holds
/// every value they return to it).
fn int_returning(cell: &CellDef) -> HashSet<String> {
    cell.sections.iter().flat_map(|s| match &s.node {
        Section::Face(f) => f.declarations.iter().filter_map(|d| match &d.node {
            FaceDecl::Signal(sig) if matches!(sig.return_type.as_ref().map(|t| &t.node), Some(TypeExpr::Simple(t)) if t == "Int") => Some(sig.name.clone()),
            _ => None,
        }).collect::<Vec<_>>(),
        _ => Vec::new(),
    }).collect()
}

#[cfg(test)]
mod linear_tests {
    use super::*;

    fn c(pairs: &[(&str, i128)], constant: i128) -> Linear {
        Linear { coef: pairs.iter().map(|(v, k)| (v.to_string(), *k)).collect(), constant }
    }

    #[test]
    fn fourier_motzkin_finds_the_contradiction_and_only_it() {
        // b >= amount, amount >= 1, and (negated goal) b - amount <= -1
        assert!(infeasible(vec![c(&[("b", 1), ("amount", -1)], 0), c(&[("amount", 1)], -1), c(&[("b", -1), ("amount", 1)], -1)]));
        // without the first fact there is a solution (b = 0, amount = 1)
        assert!(!infeasible(vec![c(&[("amount", 1)], -1), c(&[("b", -1), ("amount", 1)], -1)]));
        // a chain: x <= y, y <= z, z <= 5, x >= 6
        assert!(infeasible(vec![c(&[("y", 1), ("x", -1)], 0), c(&[("z", 1), ("y", -1)], 0), c(&[("z", -1)], 5), c(&[("x", 1)], -6)]));
        assert!(!infeasible(vec![c(&[("y", 1), ("x", -1)], 0), c(&[("z", 1), ("y", -1)], 0), c(&[("z", -1)], 5), c(&[("x", 1)], -5)]));
        // coefficients: 2x + 3y <= 6, x >= 2, y >= 1
        assert!(infeasible(vec![c(&[("x", -2), ("y", -3)], 6), c(&[("x", 1)], -2), c(&[("y", 1)], -1)]));
        assert!(!infeasible(vec![]));
    }
}
