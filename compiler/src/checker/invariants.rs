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
        let mut writes: Vec<(String, String, Expr, bool)> = Vec::new();
        for on in handlers.values() {
            collect_writes_stmts(&on.body, &on.signal_name, false, &mut writes);
        }
        writes.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));

        // local variable ranges, per handler
        let mut locals: HashMap<String, HashMap<String, Known>> = HashMap::new();
        for (name, on) in &handlers {
            locals.insert(name.clone(), local_ranges(on, &hyp, &handlers));
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
            let relevant: Vec<&(String, String, Expr, bool)> = writes
                .iter()
                .filter(|(_, slot, _, _)| targets.contains(slot))
                .collect();
            if relevant.is_empty() {
                result.checks.push(VerifyCheck::Pass(format!(
                    "invariant {inv_text} — no handler writes to guarded slots"
                )));
                continue;
            }
            let parts = conjuncts(inv);
            let mut runtime_checked: Vec<String> = Vec::new();
            for (handler, slot, value_expr, in_try) in relevant {
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
                    vars: locals.get(handler).cloned().unwrap_or_default(),
                    handlers: &handlers,
                    depth: 0,
                };
                let known = ctx.range_of(value_expr);
                let inductive = uses_slot_read(value_expr, &ctx);
                let verdicts: Vec<Proof> = parts.iter().map(|c| prove(c, slot, known)).collect();
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
                            format!("`{n}` is a parameter (narrow it: `require {n} >= 0 else …`)")
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
                    why.sort();
                    let tag = if open.len() == parts.len() {
                        format!("{handler} → {slot}")
                    } else {
                        format!("{handler} → {slot} [{}]", open.join(" && "))
                    };
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
    if lo.is_nan() || hi.is_nan() {
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
        match expr {
            Expr::Literal(Literal::Int(n)) => Known::Exact(*n as f64),
            Expr::Literal(Literal::Float(f)) => Known::Exact(*f),
            Expr::Ident(n) => self.vars.get(n).copied().unwrap_or(Known::Unknown),
            Expr::BinaryOp { left, op, right } => {
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
                            vars: local_ranges_at(on, self.hyp, self.handlers, self.depth + 1),
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
) -> HashMap<String, Known> {
    local_ranges_at(on, hyp, handlers, 0)
}

/// Range of every local: the join of all its assignments. Three rounds;
/// a bound still moving after that (an accumulator in a loop) is widened
/// to ±∞. Parameters are unknown.
fn local_ranges_at(
    on: &OnSection,
    hyp: &HashMap<String, (f64, f64)>,
    handlers: &HashMap<String, &OnSection>,
    depth: usize,
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
    let dup: HashSet<&str> = assigns.iter().map(|(n, _)| *n).filter(|n| assigns.iter().filter(|(m, _)| m == n).count() > 1).collect();
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
    let mut unconditional: Vec<&Spanned<Statement>> = Vec::new();
    collect_unconditional_requires(&on.body, &mut unconditional);
    for stmt in unconditional {
        if let Statement::Require { constraint, .. } = &stmt.node {
            for (left, op, right) in constraint_comparisons(&constraint.node) {
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
                        _ => continue,
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
    vars
}

fn collect_unconditional_requires<'e>(stmts: &'e [Spanned<Statement>], out: &mut Vec<&'e Spanned<Statement>>) {
    for st in stmts {
        match &st.node {
            Statement::Require { .. } => out.push(st),
            Statement::For { body, .. } | Statement::While { body, .. } => {
                if !has_break_or_continue(body) { collect_unconditional_requires(body, out); }
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

fn collect_writes_stmts(stmts: &[Spanned<Statement>], handler: &str, in_try: bool, out: &mut Vec<(String, String, Expr, bool)>) {
    for stmt in stmts {
        match &stmt.node {
            Statement::Let { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Return { value } => collect_writes_expr(&value.node, handler, in_try, out),
            Statement::ExprStmt { expr } => collect_writes_expr(&expr.node, handler, in_try, out),
            Statement::If { condition, then_body, else_body } => {
                collect_writes_expr(&condition.node, handler, in_try, out);
                collect_writes_stmts(then_body, handler, in_try, out);
                collect_writes_stmts(else_body, handler, in_try, out);
            }
            Statement::While { condition, body, .. } => {
                collect_writes_expr(&condition.node, handler, in_try, out);
                collect_writes_stmts(body, handler, in_try, out);
            }
            Statement::For { iter, body, .. } => {
                collect_writes_expr(&iter.node, handler, in_try, out);
                collect_writes_stmts(body, handler, in_try, out);
            }
            // `slot[k] = v` — same write path as slot.set(k, v) at runtime.
            // A local of the same name is filtered out later (only guarded
            // slot names are kept).
            Statement::IndexSet { name, index, value } => {
                out.push((handler.to_string(), name.clone(), value.node.clone(), in_try));
                collect_writes_expr(&index.node, handler, in_try, out);
                collect_writes_expr(&value.node, handler, in_try, out);
            }
            Statement::MethodCall { target, method, args } => {
                push_slot_write(target, method, args, handler, in_try, out);
                for a in args {
                    collect_writes_expr(&a.node, handler, in_try, out);
                }
            }
            Statement::Emit { args, .. } => {
                for a in args {
                    collect_writes_expr(&a.node, handler, in_try, out);
                }
            }
            Statement::Ensure { condition } => collect_writes_expr(&condition.node, handler, in_try, out),
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
    out: &mut Vec<(String, String, Expr, bool)>,
) {
    match method {
        "set" | "put" if args.len() >= 2 => {
            out.push((handler.to_string(), slot.to_string(), args[1].node.clone(), in_try));
        }
        "push" | "append" if !args.is_empty() => {
            out.push((handler.to_string(), slot.to_string(), args[0].node.clone(), in_try));
        }
        "delete" | "remove" => {
            out.push((handler.to_string(), slot.to_string(), Expr::Ident("<deleted entry>".to_string()), in_try));
        }
        _ => {}
    }
}

fn collect_writes_expr(expr: &Expr, handler: &str, in_try: bool, out: &mut Vec<(String, String, Expr, bool)>) {
    match expr {
        Expr::MethodCall { target, method, args } => {
            if let Expr::Ident(slot) = &target.node {
                push_slot_write(slot, method, args, handler, in_try, out);
            }
            collect_writes_expr(&target.node, handler, in_try, out);
            for a in args {
                collect_writes_expr(&a.node, handler, in_try, out);
            }
        }
        Expr::FnCall { args, .. } => {
            for a in args {
                collect_writes_expr(&a.node, handler, in_try, out);
            }
        }
        Expr::FieldAccess { target, .. } => collect_writes_expr(&target.node, handler, in_try, out),
        Expr::BinaryOp { left, right, .. }
        | Expr::CmpOp { left, right, .. }
        | Expr::Pipe { left, right } => {
            collect_writes_expr(&left.node, handler, in_try, out);
            collect_writes_expr(&right.node, handler, in_try, out);
        }
        Expr::Try(i) => collect_writes_expr(&i.node, handler, true, out),
        Expr::Not(i) | Expr::TryPropagate(i) => collect_writes_expr(&i.node, handler, in_try, out),
        Expr::ListLiteral(items) => {
            for i in items {
                collect_writes_expr(&i.node, handler, in_try, out);
            }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields {
                collect_writes_expr(&v.node, handler, in_try, out);
            }
        }
        Expr::Lambda { body, .. } => collect_writes_expr(&body.node, handler, in_try, out),
        Expr::LambdaBlock { stmts, result, .. } => {
            collect_writes_stmts(stmts, handler, in_try, out);
            collect_writes_expr(&result.node, handler, in_try, out);
        }
        Expr::Match { subject, arms } => {
            collect_writes_expr(&subject.node, handler, in_try, out);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_writes_expr(&g.node, handler, in_try, out);
                }
                collect_writes_stmts(&arm.body, handler, in_try, out);
                collect_writes_expr(&arm.result.node, handler, in_try, out);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            collect_writes_expr(&condition.node, handler, in_try, out);
            collect_writes_stmts(then_body, handler, in_try, out);
            collect_writes_expr(&then_result.node, handler, in_try, out);
            collect_writes_stmts(else_body, handler, in_try, out);
            collect_writes_expr(&else_result.node, handler, in_try, out);
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
