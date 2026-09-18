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
    "value", "key", "size", "_slot_len", "_slot_name", "_key", "true", "false",
];

/// Check-time validation: every name an invariant references must be
/// resolvable when the runtime evaluates it.
pub fn validate_program(program: &Program) -> Vec<InvariantIssue> {
    let mut issues = Vec::new();
    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.node.sections {
            let Section::Memory(mem) = &section.node else { continue };
            let slot_names: HashSet<&str> =
                mem.slots.iter().map(|s| s.node.name.as_str()).collect();
            for inv in &mem.invariants {
                let mut idents = HashSet::new();
                collect_idents(&inv.node, &mut idents);
                // An invariant is evaluated per write, with only the slot
                // being written in scope. Naming two slots can never
                // evaluate — every write to either would be rejected.
                let mut named: Vec<&str> = slot_names
                    .iter()
                    .copied()
                    .filter(|s| idents.contains(*s))
                    .collect();
                if named.len() > 1 {
                    named.sort();
                    issues.push(InvariantIssue {
                        message: format!(
                            "memory invariant references several slots ({}) — an invariant is \
                             checked per write, with only the written slot's value in scope, so \
                             this could never evaluate and every write would be rejected. Write \
                             one invariant per slot",
                            named.join(", ")
                        ),
                        span: inv.span,
                    });
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
                let mut fns = HashSet::new();
                collect_fn_names(&inv.node, &mut fns);
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

pub fn verify_program_invariants(program: &Program) -> Vec<VerifyResult> {
    let mut results = Vec::new();

    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }

        // invariant → the slots it guards (same scoping rule as runtime)
        let mut guarded: Vec<(Expr, Vec<String>, String)> = Vec::new();
        for section in &cell.node.sections {
            let Section::Memory(mem) = &section.node else { continue };
            let slot_names: Vec<String> =
                mem.slots.iter().map(|s| s.node.name.clone()).collect();
            for inv in &mem.invariants {
                let mut refs = HashSet::new();
                collect_idents(&inv.node, &mut refs);
                let named: Vec<String> = slot_names
                    .iter()
                    .filter(|n| refs.contains(*n))
                    .cloned()
                    .collect();
                let targets = if named.is_empty() { slot_names.clone() } else { named };
                guarded.push((normalize_invariant(&inv.node, &slot_names), targets, render_expr(&inv.node)));
            }
        }
        if guarded.is_empty() {
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

        let handlers: HashMap<String, &OnSection> = cell
            .node
            .sections
            .iter()
            .filter_map(|s| if let Section::OnSignal(on) = &s.node { Some((on.signal_name.clone(), on)) } else { None })
            .collect();

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
                locals.entry((name.clone(), path.clone())).or_insert_with(|| local_ranges(on, &hyp, &handlers, path));
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
            let mut runtime_checked: Vec<String> = Vec::new();
            for (handler, slot, value_expr, in_try, wpath, wkey) in relevant {
                // a delete removes an entry that already satisfied a VALUE
                // invariant; only a `size` clause can flip on it
                if matches!(value_expr, Expr::Ident(n) if n == "<deleted entry>") {
                    let mut names = HashSet::new();
                    collect_idents(inv, &mut names);
                    let mut fns = HashSet::new();
                    collect_fn_names(inv, &mut fns);
                    if !names.contains("size") && !names.contains("len") && !fns.contains("len") && !fns.contains("size") {
                        result.checks.push(VerifyCheck::Pass(format!(
                            "invariant {inv_text} — writer '{handler}' only deletes from '{slot}' (a value invariant cannot break on a delete)"
                        )));
                        continue;
                    }
                }
                let ctx = RangeCtx {
                    hyp: &hyp,
                    vars: locals.get(&(handler.clone(), wpath.clone())).cloned().unwrap_or_default(),
                    handlers: &handlers,
                    depth: 0,
                };
                let known = ctx.range_of(value_expr);
                let inductive = uses_slot_read(value_expr, &ctx);
                let mut verdicts: Vec<Proof> = parts.iter().map(|c| prove(c, slot, known)).collect();
                // `size <= K` after a write that adds at most one entry:
                // proven when the writer required the slot's size < K
                // (`require rows.size < 500`, `require len(rows) < 500`)
                // …and only for the handler's ONE adding write to this slot,
                // outside any loop (two pushes after one require add two)
                // a set on a key the handler knows exists does not grow the slot
                let key_exists = |path: &Vec<usize>, key: &Option<String>| -> bool {
                    let Some(k) = key else { return false };
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
                    if k.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| !w.is_empty() && (rebound(w) || shadow.contains(w))) { return false; }
                    existing_key_facts(&on.body, &[], slot).iter().any(|(fp, fk, excl)| path.starts_with(fp) && fk == k && excl.as_ref().map_or(true, |e| !path.starts_with(e)))
                };
                if size_upper_bound_any(&parts, slot) && key_exists(wpath, wkey) {
                    for (c, v) in parts.iter().zip(verdicts.iter_mut()) {
                        if size_upper_bound(c, slot).is_some() { *v = Proof::Holds; }
                    }
                }
                let mut adds_here = writes.iter().filter(|(h, sl, e, _, p, k)| h == handler && sl == slot
                    && !matches!(e, Expr::Ident(n) if n == "<deleted entry>") && !key_exists(p, k)).count();
                // a sibling handler called from here that also adds to the
                // slot grows it past the one require (transitively)
                if let Some(on) = handlers.get(handler) {
                    let mut seen: HashSet<String> = HashSet::new();
                    let mut stack: Vec<String> = Vec::new();
                    crate::checker::literals::for_each_call(&on.body, &mut |n, _, _| stack.push(n.to_string()));
                    // `emit ev(…)` runs every `on ev` of the process: a call too
                    fn emits(stmts: &[Spanned<Statement>], out: &mut Vec<String>) {
                        for st in stmts {
                            match &st.node {
                                Statement::Emit { signal_name, .. } => out.push(signal_name.clone()),
                                Statement::If { then_body, else_body, .. } => { emits(then_body, out); emits(else_body, out); }
                                Statement::For { body, .. } | Statement::While { body, .. } => emits(body, out),
                                _ => {}
                            }
                        }
                    }
                    emits(&on.body, &mut stack);
                    while let Some(callee) = stack.pop() {
                        if callee == *handler || !seen.insert(callee.clone()) { continue; }
                        let Some(c_on) = handlers.get(&callee) else { continue };
                        if writes.iter().any(|(h, sl, e, _, _, _)| *h == callee && sl == slot && !matches!(e, Expr::Ident(n) if n == "<deleted entry>")) {
                            adds_here += 1;
                        }
                        crate::checker::literals::for_each_call(&c_on.body, &mut |n, _, _| stack.push(n.to_string()));
                        emits(&c_on.body, &mut stack);
                    }
                }
                // a loop body, or a lambda (`xs |> map(i => rows.push(i))`) may
                // run the write many times
                let in_loop = wpath.iter().any(|step| step % 4 == 2 || *step == usize::MAX / 2);
                for (c, v) in parts.iter().zip(verdicts.iter_mut()) {
                    if *v == Proof::Holds || adds_here != 1 || in_loop { continue; }
                    let Some(k) = size_upper_bound(c, slot) else { continue };
                    let before = [format!("{}.size", slot), format!("{}.len", slot), format!("len({})", slot), format!("size({})", slot)]
                        .iter()
                        .filter_map(|r| ctx.vars.get(&format!("__expr__{}", r)).copied().and_then(bounds))
                        .map(|(_, hi)| hi)
                        .fold(f64::INFINITY, f64::min);
                    if before + 1.0 <= k { *v = Proof::Holds; }
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
                                .map(|(c, _)| render_expr(c).replace(slot.as_str(), &render_expr(value_expr))).collect();
                            format!("`{n}` is a parameter (narrow it: `require {} else …`)", open_txt.join(" && "))
                        } else {
                            let mut assigns: Vec<(&str, &Expr)> = Vec::new();
                            if let Some(o) = on { collect_assigns(&o.body, &mut assigns); }
                            let bound: Vec<&&Expr> = assigns.iter().filter(|(m, _)| *m == n.as_str()).map(|(_, e)| e).collect();
                            match bound.as_slice() {
                                [] => format!("`{n}` has no known range"),
                                [one] => match one {
                                    Expr::FnCall { name: f, .. } if !handlers.contains_key(f) => format!("`{n}` = {}() — a builtin the prover does not bound", f),
                                    Expr::MethodCall { .. } | Expr::Index { .. } => format!("`{n}` reads a slot with no interval invariant"),
                                    _ => format!("`{n}` = {} — not bounded", render_expr(one)),
                                },
                                _ => format!("`{n}` is reassigned {} times (a loop accumulator?) — bind it once, or read the slot", bound.len()),
                            }
                        }
                    }).collect();
                    why.retain(|w| !w.is_empty());
                    why.sort();
                    // every name has SOME range, just not a tight enough one:
                    // say what is known and the require that would prove it
                    if why.is_empty() {
                        if let Some((lo, hi)) = bounds(ctx.range_of(value_expr)) {
                            let num = |x: f64| if x == f64::INFINITY { "∞".to_string() } else if x == f64::NEG_INFINITY { "-∞".to_string() } else { format!("{}", x) };
                            let open_txt: Vec<String> = parts.iter().zip(&verdicts)
                                .filter(|(_, v)| **v != Proof::Holds)
                                .map(|(c, _)| render_expr(c).replace(slot.as_str(), &render_expr(value_expr))).collect();
                            let (l, r) = (if lo.is_finite() { "[" } else { "(" }, if hi.is_finite() { "]" } else { ")" });
                            why.push(format!("`{}` is only known to lie in {}{}, {}{} — narrow it: `require {} else …`",
                                render_expr(value_expr), l, num(lo), num(hi), r, open_txt.join(" && ")));
                        }
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
                    if all_size {
                        why = vec![format!("the slot may grow past {} — put `require len({}) < {}` before the handler's one write that adds to it (not in a loop)", open_size[0], slot, open_size[0])];
                    }
                    let tag = if why.is_empty() { tag } else { format!("{tag} because {}", why.join("; ")) };
                    if !runtime_checked.contains(&tag) {
                        runtime_checked.push(tag);
                    }
                }
            }
            if !runtime_checked.is_empty() {
                result.checks.push(VerifyCheck::Warning(format!(
                    "invariant {inv_text} — runtime-checked (computed values): {}",
                    runtime_checked.join(", ")
                )));
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
    if lo.is_nan() || hi.is_nan() || (lo.is_finite() && lo.abs() > EXACT) || (hi.is_finite() && hi.abs() > EXACT) {
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
            Expr::Literal(Literal::Int(n)) => Known::Exact(*n as f64),
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
                ("len", 1) => mk(0.0, f64::INFINITY),
                // a sibling handler: `on total() { return counts.get("n") ?? 0 }`,
                // or `_drop_hold(id, sku)` returning a slot read — its
                // parameters are unknown inside, its returns are joined
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
    let mut facts: Vec<(&Expr, CmpOp, &Expr, Option<HashSet<String>>)> = Vec::new();
    for f in &collected {
        if !write_path.starts_with(&f.path) { continue; }
        if let Some(ex) = &f.exclude { if write_path.starts_with(ex) { continue; } }
        match &f.stmt.node {
            Statement::Require { constraint, .. } => facts.extend(constraint_comparisons(&constraint.node).into_iter().map(|(l, o, r)| (l, o, r, None))),
            Statement::If { condition, .. } => facts.extend(negated_comparisons(&condition.node).into_iter().map(|(l, o, r)| (l, o, r, None))),
            _ => {}
        }
    }
    {
        {
            for (left, op, right, scope) in facts {
                // inside a loop: every name the fact mentions must be a
                // per-iteration local of that loop
                if let Some(scope) = &scope {
                    let mut used = HashSet::new();
                    collect_idents(left, &mut used);
                    collect_idents(right, &mut used);
                    if !used.iter().all(|u| scope.contains(u)) { continue; }
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
                                CmpOp::Lt => (f64::NEG_INFINITY, if looks_int(e) && c.fract() == 0.0 { c - 1.0 } else { c }),
                                CmpOp::Le => (f64::NEG_INFINITY, c),
                                CmpOp::Gt => (if looks_int(e) && c.fract() == 0.0 { c + 1.0 } else { c }, f64::INFINITY),
                                CmpOp::Ge => (c, f64::INFINITY),
                                CmpOp::Eq => (c, c),
                                CmpOp::Ne => continue,
                            };
                            let key = format!("__expr__{}", render_expr(e));
                            let cur = vars.get(&key).copied().unwrap_or(Known::Unknown);
                            let narrowed = match bounds(cur) { Some((cl, ch)) => mk(cl.max(lo), ch.min(hi)), None => mk(lo, hi) };
                            vars.insert(key, narrowed);
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
                    CmpOp::Lt => (f64::NEG_INFINITY, if integral { bound - 1.0 } else { bound }),
                    CmpOp::Le => (f64::NEG_INFINITY, bound),
                    CmpOp::Gt => (if integral { bound + 1.0 } else { bound }, f64::INFINITY),
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
    vars
}

/// `if a || b { return … }` after which ¬a ∧ ¬b holds: the negated leaves.
/// `&&` cannot be split (¬(a ∧ b) is a disjunction) and yields nothing.
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
}

fn collect_facts<'e>(stmts: &'e [Spanned<Statement>], path: &[usize], out: &mut Vec<Fact<'e>>) {
    let sub = |i: usize, b: usize| -> Vec<usize> { let mut v = path.to_vec(); v.push(block_step(i, b)); v };
    for (i, st) in stmts.iter().enumerate() {
        match &st.node {
            Statement::Require { .. } => out.push(Fact { stmt: st, path: path.to_vec(), exclude: None }),
            Statement::If { then_body, else_body, .. } => {
                if else_body.is_empty() && exits(then_body) {
                    out.push(Fact { stmt: st, path: path.to_vec(), exclude: Some(sub(i, 0)) });
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

/// An expression that cannot produce a fraction: no Float literal, no `/`,
/// no float-producing builtin. Used to tighten strict bounds on integers.
fn looks_int(expr: &Expr) -> bool {
    match expr {
        Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::BigInt(_)) => true,
        Expr::Literal(Literal::Float(_)) => false,
        Expr::Literal(_) => true,
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
            Statement::Let { name, value } | Statement::Assign { name, value } => out.push((name.as_str(), &value.node)),
            Statement::If { then_body, else_body, .. } => {
                collect_assigns(then_body, out);
                collect_assigns(else_body, out);
            }
            Statement::While { body, .. } => collect_assigns(body, out),
            Statement::For { var, body, .. } => {
                // the loop variable is unknown
                out.push((var.as_str(), &UNKNOWN_EXPR));
                collect_assigns(body, out);
            }
            _ => {}
        }
    }
}

static UNKNOWN_EXPR: Expr = Expr::Literal(Literal::Unit);

/// Constant-fold a value expression where possible.
fn static_value(expr: &Expr) -> Known {
    match expr {
        Expr::Literal(Literal::Int(n)) => Known::Exact(*n as f64),
        Expr::Literal(Literal::Float(f)) => Known::Exact(*f),
        Expr::BinaryOp { left, op, right } => {
            let (Known::Exact(a), Known::Exact(b)) =
                (static_value(&left.node), static_value(&right.node))
            else {
                return Known::Unknown;
            };
            match op {
                BinOp::Add => Known::Exact(a + b),
                BinOp::Sub => Known::Exact(a - b),
                BinOp::Mul => Known::Exact(a * b),
                BinOp::Div if b != 0.0 => Known::Exact(a / b),
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
            if lo <= hi { Known::Range(lo, hi) } else { Known::Unknown }
        }
        _ => Known::Unknown,
    }
}

/// Prove an invariant against a known written value. Supports the
/// shapes that cover real risk limits: comparisons of the slot value
/// (optionally through abs()) against constants, joined with &&.
fn prove(inv: &Expr, slot: &str, value: Known) -> Proof {
    match inv {
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
            | Statement::Return { value } => collect_writes_expr(&value.node, handler, in_try, path, out),
            Statement::ExprStmt { expr } => collect_writes_expr(&expr.node, handler, in_try, path, out),
            Statement::If { condition, then_body, else_body } => {
                collect_writes_expr(&condition.node, handler, in_try, path, out);
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
                out.push((handler.to_string(), name.clone(), value.node.clone(), in_try, path.to_vec(), Some(render_expr(&index.node))));
                collect_writes_expr(&index.node, handler, in_try, path, out);
                collect_writes_expr(&value.node, handler, in_try, path, out);
            }
            Statement::MethodCall { target, method, args } => {
                push_slot_write(target, method, args, handler, in_try, path, out);
                for a in args {
                    collect_writes_expr(&a.node, handler, in_try, path, out);
                }
            }
            Statement::Emit { args, .. } => {
                for a in args {
                    collect_writes_expr(&a.node, handler, in_try, path, out);
                }
            }
            Statement::Ensure { condition } => collect_writes_expr(&condition.node, handler, in_try, path, out),
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
                let mut hits: Vec<(String, String)> = Vec::new();
                find_len_of_slot(&inv.node, &slots, &mut hits);
                for (func, slot) in hits {
                    out.push(InvariantIssue {
                        message: format!(
                            "`{func}({slot})` in an invariant measures the VALUE being written to '{slot}', not how \
                             many entries '{slot}' holds. For an entry-count bound write `{slot}.size <= N`; if you \
                             do mean the value's length, write `{func}(value)`"
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

fn collect_binders_expr(e: &Expr, out: &mut HashSet<String>) {
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

fn pattern_binders(p: &MatchPattern, out: &mut HashSet<String>) {
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
fn key_presence(e: &Expr, slot: &str, negate: bool) -> Option<(String, bool)> {
    let get_key = |t: &Expr| -> Option<String> {
        match t {
            Expr::MethodCall { target, method, args } if (method == "get" || method == "has") && args.len() == 1
                && matches!(&target.node, Expr::Ident(n) if n == slot) => Some(render_expr(&args[0].node)),
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
        Expr::Not(i) => key_presence(&i.node, slot, !negate),
        _ => None,
    }
}

/// Where a key of `slot` is known to exist: (block path, key rendering,
/// excluded sub-path).
fn existing_key_facts(stmts: &[Spanned<Statement>], path: &[usize], slot: &str) -> Vec<(Vec<usize>, String, Option<Vec<usize>>)> {
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
                    if let Some((k, true)) = key_presence(&e, slot, false) { out.push((path.to_vec(), k, None)); }
                }
            }
            Statement::If { condition, then_body, else_body } => {
                if let Some((k, present)) = key_presence(&condition.node, slot, false) {
                    if present {
                        out.push((sub(i, 0), k, None));
                    } else {
                        out.push((sub(i, 1), k.clone(), None));
                        if else_body.is_empty() && exits(then_body) { out.push((path.to_vec(), k, Some(sub(i, 0)))); }
                    }
                }
                out.extend(existing_key_facts(then_body, &sub(i, 0), slot));
                out.extend(existing_key_facts(else_body, &sub(i, 1), slot));
            }
            Statement::For { body, .. } | Statement::While { body, .. } => out.extend(existing_key_facts(body, &sub(i, 2), slot)),
            _ => {}
        }
    }
    out
}
