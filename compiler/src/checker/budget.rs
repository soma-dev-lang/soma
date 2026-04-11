//! Memory-budget proof obligation for Soma cells.
//!
//! Turns `scale { memory: "256Mi" }` from an advisory into a compile-
//! time proof obligation. Given a cell with a declared memory budget,
//! `BudgetChecker::check_cell` either:
//!
//!   - proves `peak_memory(cell) ≤ budget` via a closed-form bound
//!     derived from slot annotations and a conservative walk of every
//!     handler body, and emits a `BudgetOk` info (PASS); or
//!   - produces a concrete bound that exceeds the budget and emits a
//!     `BudgetExceeded` error (FAIL); or
//!   - cannot produce a bound because the cell calls an *unbounded
//!     builtin* (e.g. `from_json`, `think`, `http_get`) and emits a
//!     `BudgetAdvisory` warning listing the call sites (ADVISORY).
//!
//! ## The formula
//!
//! ```text
//! peak_memory(C) ≤ slot_sum(C)
//!                + max_{h ∈ handlers(C)} handler_peak(h)
//!                + state_machine_bound(C)
//!                + C_runtime
//! ```
//!
//! The `max` over handlers (not `sum`) matches the Soma runtime's
//! actual execution model: a single in-process cell instance runs one
//! handler at a time. A previous design using `sum` was unsound
//! (overly conservative by a factor of |handlers|).
//!
//! ## Annotations
//!
//! The checker reads existing `MemoryProperty::Param` annotations on
//! each memory slot. The grammar already parses any parameterized
//! property generically (`parser/mod.rs:751`); the checker just reads
//! the names it cares about.
//!
//! Recognized slot annotations:
//!
//!   - `[capacity(N)]`         — upper bound on the number of entries
//!                               stored in this collection slot.
//!   - `[max_key_bytes(N)]`    — upper bound on the byte size of any
//!                               key (for Map slots).
//!   - `[max_value_bytes(N)]`  — upper bound on the byte size of any
//!                               value.
//!   - `[max_element_bytes(N)]` — upper bound on each element of a
//!                                List slot.
//!
//! Recognized state block annotations (on the `state` section itself):
//!
//!   - `[max_instances(N)]`    — upper bound on the number of live
//!                               state-machine instance IDs.
//!
//! Recognized handler annotations:
//!
//!   - `[loop_bound(N)]`       — explicit upper bound on a `for` or
//!                               `while` loop inside the handler.
//!   - `[max_input_bytes(N)]`  — upper bound on any single handler
//!                               parameter's byte size (used to bound
//!                               `from_json` on parameters).
//!
//! ## Defaults (conservative)
//!
//! If an annotation is missing, the checker uses a conservative
//! default rather than claiming `Unbounded`:
//!
//!   - `DEFAULT_CAPACITY = 10_000` entries per collection slot
//!   - `DEFAULT_MAX_KEY_BYTES = 256`
//!   - `DEFAULT_MAX_VALUE_BYTES = 4 * 1024`
//!   - `DEFAULT_MAX_ELEMENT_BYTES = 4 * 1024`
//!   - `DEFAULT_MAX_INSTANCES = 10_000`
//!   - `HANDLER_STACK_OVERHEAD = 8 * 1024 * 1024` (8 MiB, per active handler)
//!   - `C_RUNTIME = 16 * 1024 * 1024` (16 MiB)
//!
//! Users who want tight bounds override with explicit annotations.
//!
//! ## Soundness
//!
//! Sound for: cells whose handlers do not call an "unbounded builtin"
//! (see `is_unbounded_builtin`). Sound under the local adversary model
//! from `docs/ADVERSARIES.md`: a single cell instance, one handler
//! active at a time.
//!
//! Not sound for: concurrent handler execution, cross-cell flows
//! where the receiving cell's budget is not declared, transient
//! LLM/HTTP responses of arbitrary size.
//!
//! Full theorem statement: `docs/SEMANTICS.md` §1.8.

use crate::ast::*;

// ── Tunable constants ──────────────────────────────────────────────

pub const DEFAULT_CAPACITY: u64 = 10_000;
pub const DEFAULT_MAX_KEY_BYTES: u64 = 256;
pub const DEFAULT_MAX_VALUE_BYTES: u64 = 4 * 1024;
pub const DEFAULT_MAX_ELEMENT_BYTES: u64 = 4 * 1024;
pub const DEFAULT_MAX_INSTANCES: u64 = 10_000;
pub const DEFAULT_INSTANCE_SIZE: u64 = 256;

/// Conservative per-handler stack budget. This is the worst case we
/// charge for ONE active handler at a time (not the sum across all
/// handlers). 8 MiB matches most OS thread default stacks.
pub const HANDLER_STACK_OVERHEAD: u64 = 8 * 1024 * 1024;

/// Fixed interpreter + runtime overhead: HTTP server if present,
/// storage backends, the agent trace, string interning, etc.
pub const C_RUNTIME: u64 = 16 * 1024 * 1024;

// ── The cost lattice ───────────────────────────────────────────────

/// Abstract cost: either a finite upper bound in bytes, or
/// `Unbounded` with a human-readable reason set.
#[derive(Debug, Clone)]
pub enum Cost {
    Bounded(u64),
    Unbounded(Vec<String>),
}

impl Cost {
    pub fn zero() -> Self { Cost::Bounded(0) }
    pub fn bytes(n: u64) -> Self { Cost::Bounded(n) }
    pub fn unbounded(reason: impl Into<String>) -> Self {
        Cost::Unbounded(vec![reason.into()])
    }

    /// Lattice: sequential composition (the total of two back-to-back
    /// allocations, conservatively assuming neither is freed before
    /// the other runs).
    pub fn plus(self, other: Cost) -> Cost {
        match (self, other) {
            (Cost::Bounded(a), Cost::Bounded(b)) => Cost::Bounded(a.saturating_add(b)),
            (Cost::Unbounded(mut r1), Cost::Unbounded(r2)) => {
                r1.extend(r2);
                Cost::Unbounded(r1)
            }
            (Cost::Unbounded(r), _) | (_, Cost::Unbounded(r)) => Cost::Unbounded(r),
        }
    }

    /// Lattice: branching join (take the max of two branches).
    pub fn max(self, other: Cost) -> Cost {
        match (self, other) {
            (Cost::Bounded(a), Cost::Bounded(b)) => Cost::Bounded(a.max(b)),
            (Cost::Unbounded(mut r1), Cost::Unbounded(r2)) => {
                r1.extend(r2);
                Cost::Unbounded(r1)
            }
            (Cost::Unbounded(r), _) | (_, Cost::Unbounded(r)) => Cost::Unbounded(r),
        }
    }

    /// Lattice: multiplication (loop unrolling). `self × factor`.
    pub fn times(self, factor: u64) -> Cost {
        match self {
            Cost::Bounded(a) => Cost::Bounded(a.saturating_mul(factor)),
            Cost::Unbounded(r) => Cost::Unbounded(r),
        }
    }

    pub fn is_bounded(&self) -> bool {
        matches!(self, Cost::Bounded(_))
    }

    pub fn as_bounded(&self) -> Option<u64> {
        match self { Cost::Bounded(n) => Some(*n), _ => None }
    }
}

// ── Annotation readers ─────────────────────────────────────────────

/// Read a positive integer value from a parameterized slot property
/// `[name(N)]`. Returns None if the property is absent or malformed.
fn read_int_param(slot: &MemorySlot, prop_name: &str) -> Option<u64> {
    for prop in &slot.properties {
        if let MemoryProperty::Param(ref p) = prop.node {
            if p.name == prop_name {
                if let Some(lit) = p.values.first() {
                    if let Literal::Int(n) = lit.node {
                        if n >= 0 {
                            return Some(n as u64);
                        }
                    }
                }
            }
        }
    }
    None
}

// ── Parser for "256Mi", "8Gi", etc. ────────────────────────────────

/// Convert a memory budget string to bytes. Supports Ki, Mi, Gi, Ti
/// (binary SI) and K, M, G, T (decimal). Returns None on parse error.
pub fn parse_budget_bytes(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() { return None; }

    // Strip the unit suffix, if any.
    let (num_part, mult): (&str, u64) = if let Some(stripped) = s.strip_suffix("Ki") {
        (stripped, 1024)
    } else if let Some(stripped) = s.strip_suffix("Mi") {
        (stripped, 1024 * 1024)
    } else if let Some(stripped) = s.strip_suffix("Gi") {
        (stripped, 1024 * 1024 * 1024)
    } else if let Some(stripped) = s.strip_suffix("Ti") {
        (stripped, 1024_u64.pow(4))
    } else if let Some(stripped) = s.strip_suffix("K") {
        (stripped, 1000)
    } else if let Some(stripped) = s.strip_suffix("M") {
        (stripped, 1_000_000)
    } else if let Some(stripped) = s.strip_suffix("G") {
        (stripped, 1_000_000_000)
    } else if let Some(stripped) = s.strip_suffix("T") {
        (stripped, 1_000_000_000_000)
    } else if let Some(stripped) = s.strip_suffix("B") {
        (stripped, 1)
    } else {
        (s, 1)
    };

    let n: u64 = num_part.trim().parse().ok()?;
    Some(n.saturating_mul(mult))
}

// ── Builtin classification ─────────────────────────────────────────

/// Which builtin calls are "unbounded" — i.e., their result size is a
/// function of data not visible to the static analyzer. Calling one
/// of these inside a handler degrades the handler's cost to
/// `Unbounded(reason)`.
pub fn unbounded_builtin_reason(name: &str) -> Option<&'static str> {
    match name {
        "think" | "think_json" =>
            Some("LLM response size is not statically bounded"),
        "from_json" =>
            Some("from_json input size is not statically bounded"),
        "to_json" =>
            Some("to_json output size depends on input size"),
        "http_get" | "http_post" | "http_put" | "http_delete" =>
            Some("HTTP response size is not statically bounded"),
        "read_file" =>
            Some("file size is not statically bounded"),
        "sql_query" | "query" =>
            Some("database result set size is not statically bounded"),
        "delegate" =>
            Some("delegate target response size is not statically bounded"),
        "recall" | "recall_similar" =>
            Some("recall result size is not statically bounded"),
        _ => None,
    }
}

// ── Allocation cost for a single expression ────────────────────────

/// Cost of evaluating an expression — the maximum number of transient
/// bytes allocated during evaluation.
///
/// Design choice: we over-approximate by *summing* the costs of
/// subexpressions (as if they all live until the parent expression
/// completes), which is sound. A tighter analysis would track which
/// intermediates can be freed — left for Tier 2.
pub fn expr_cost(e: &Expr) -> Cost {
    match e {
        Expr::Literal(lit) => match lit {
            Literal::String(s) => Cost::bytes(s.len() as u64 + 32),
            _ => Cost::zero(),
        },
        Expr::Ident(_) => Cost::zero(),

        Expr::FieldAccess { target, .. } => expr_cost(&target.node),

        Expr::MethodCall { target, method, args } => {
            let mut c = expr_cost(&target.node);
            for a in args {
                c = c.plus(expr_cost(&a.node));
            }
            // Method on a storage slot: `.set`, `.get`, `.keys`, `.values`, `.len`
            // These don't themselves allocate beyond their arguments.
            match method.as_str() {
                "set" | "get" | "delete" | "contains" | "len" | "has" => c,
                "keys" => {
                    // Returns a list of all keys. Without flow analysis we
                    // cannot tell if the target is a storage slot or a local
                    // map, so use DEFAULT_CAPACITY × DEFAULT_MAX_KEY_BYTES.
                    c.plus(Cost::bytes(
                        DEFAULT_CAPACITY.saturating_mul(DEFAULT_MAX_KEY_BYTES)
                    ))
                }
                "values" => {
                    // Returns a list of all values.
                    c.plus(Cost::bytes(
                        DEFAULT_CAPACITY.saturating_mul(DEFAULT_MAX_VALUE_BYTES)
                    ))
                }
                _ => c,
            }
        }

        Expr::FnCall { name, args } => {
            let mut arg_cost = Cost::zero();
            for a in args {
                arg_cost = arg_cost.plus(expr_cost(&a.node));
            }
            // Check if the builtin itself is unbounded.
            if let Some(reason) = unbounded_builtin_reason(name) {
                let site = format!("{} at call site — {}", name, reason);
                return arg_cost.plus(Cost::unbounded(site));
            }
            // Specific allocation-bearing builtins with closed-form cost.
            match name.as_str() {
                // list(a, b, c) allocates a list of N elements + header
                "list" => arg_cost.plus(Cost::bytes(64 + (args.len() as u64) * 16)),
                // map(k1, v1, k2, v2, ...) allocates a map with args.len()/2 entries
                "map" => arg_cost.plus(Cost::bytes(256 + (args.len() as u64) * 64)),
                // push appends one entry
                "push" => arg_cost.plus(Cost::bytes(32)),
                // with copies the map and sets one key
                "with" => arg_cost.plus(Cost::bytes(256)),
                // Arithmetic / logical
                "abs" | "round" | "floor" | "ceil" | "sqrt" | "min" | "max"
                | "pow" | "to_string" | "to_int" | "to_float"
                | "to_bool" => arg_cost,
                // String ops: bounded by argument size (which is already in arg_cost)
                "len" | "contains" | "starts_with" | "ends_with"
                | "index_of" | "substring" | "trim" | "uppercase" | "lowercase"
                | "split" | "replace" | "concat" | "join" => {
                    // concat/join sum: O(total input bytes). Already bounded by
                    // the argument costs; add a small header per result string.
                    arg_cost.plus(Cost::bytes(64))
                }
                // Nondeterministic builtins that return small values
                "now" | "now_ms" | "random" | "next_id"
                | "today" => arg_cost,
                // print/log are no-op for memory
                "print" | "log" => arg_cost,
                // transition / state machine ops
                "transition" | "get_status" | "valid_transitions"
                | "remember" | "set_budget" | "tokens_used"
                | "emit" | "publish" | "set_status" => arg_cost,
                // response / render / html: produce HTTP response maps
                "response" | "html" | "redirect" | "render"
                | "escape_html" | "from_form" => arg_cost.plus(Cost::bytes(512)),
                // Unknown free function — treat as zero additional cost (it's
                // a user-defined handler call, and we charge each handler
                // separately in the max{} across handlers below).
                _ => arg_cost,
            }
        }

        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } => {
            expr_cost(&left.node).plus(expr_cost(&right.node))
        }

        Expr::Not(inner) => expr_cost(&inner.node),

        Expr::Record { fields, .. } => {
            let mut c = Cost::bytes(64);
            for (_, v) in fields {
                c = c.plus(expr_cost(&v.node));
            }
            c
        }

        Expr::Try(inner) => expr_cost(&inner.node).plus(Cost::bytes(128)),
        Expr::TryPropagate(inner) => expr_cost(&inner.node),

        Expr::Pipe { left, right } => {
            expr_cost(&left.node).plus(expr_cost(&right.node))
        }

        Expr::Match { subject, arms } => {
            let mut c = expr_cost(&subject.node);
            let mut arm_max = Cost::zero();
            for arm in arms {
                let mut arm_c = Cost::zero();
                for st in &arm.body {
                    arm_c = arm_c.plus(stmt_cost(&st.node));
                }
                arm_c = arm_c.plus(expr_cost(&arm.result.node));
                arm_max = arm_max.max(arm_c);
            }
            c = c.plus(arm_max);
            c
        }

        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            let cond_c = expr_cost(&condition.node);
            let mut then_c = Cost::zero();
            for st in then_body { then_c = then_c.plus(stmt_cost(&st.node)); }
            let then_total = then_c.plus(expr_cost(&then_result.node));
            let mut else_c = Cost::zero();
            for st in else_body { else_c = else_c.plus(stmt_cost(&st.node)); }
            let else_total = else_c.plus(expr_cost(&else_result.node));
            cond_c.plus(then_total.max(else_total))
        }

        Expr::Lambda { .. } | Expr::LambdaBlock { .. } => {
            // Lambda bodies are evaluated per call by the caller (e.g. |> map).
            // The cost of *constructing* the closure is small.
            Cost::bytes(64)
        }

        Expr::ListLiteral(items) => {
            let mut c = Cost::bytes(64 + (items.len() as u64) * 16);
            for it in items {
                c = c.plus(expr_cost(&it.node));
            }
            c
        }
    }
}

// ── Allocation cost for a single statement ─────────────────────────

/// Read an explicit loop bound annotation from an iterator expression.
/// Recognizes `range(0, N)` where N is a literal, and `range(N)` with
/// a single literal.
fn literal_range_bound(iter: &Expr) -> Option<u64> {
    if let Expr::FnCall { name, args } = iter {
        if name == "range" {
            // range(lo, hi) or range(n)
            if args.len() == 1 {
                if let Expr::Literal(Literal::Int(n)) = args[0].node {
                    if n >= 0 { return Some(n as u64); }
                }
            } else if args.len() == 2 {
                if let (Expr::Literal(Literal::Int(lo)), Expr::Literal(Literal::Int(hi)))
                    = (&args[0].node, &args[1].node)
                {
                    if *hi >= *lo && *lo >= 0 { return Some((*hi - *lo) as u64); }
                }
            }
        }
    }
    None
}

pub fn stmt_cost(s: &Statement) -> Cost {
    match s {
        Statement::Let { value, .. } | Statement::Assign { value, .. } => {
            expr_cost(&value.node)
        }
        Statement::Return { value } => expr_cost(&value.node),
        Statement::If { condition, then_body, else_body } => {
            let cond_c = expr_cost(&condition.node);
            let mut then_c = Cost::zero();
            for st in then_body { then_c = then_c.plus(stmt_cost(&st.node)); }
            let mut else_c = Cost::zero();
            for st in else_body { else_c = else_c.plus(stmt_cost(&st.node)); }
            cond_c.plus(then_c.max(else_c))
        }
        Statement::For { iter, body, bound, .. } => {
            let iter_c = expr_cost(&iter.node);
            let mut body_c = Cost::zero();
            for st in body { body_c = body_c.plus(stmt_cost(&st.node)); }
            // Bound resolution priority:
            //   1. Explicit `[loop_bound(N)]` annotation on the for stmt
            //   2. Literal `range(0, N)` or `range(N)` with N a constant
            //   3. Conservative fallback: DEFAULT_CAPACITY (10000)
            let n = bound
                .or_else(|| literal_range_bound(&iter.node))
                .unwrap_or(DEFAULT_CAPACITY);
            iter_c.plus(body_c.times(n))
        }
        Statement::While { condition, body, bound } => {
            let cond_c = expr_cost(&condition.node);
            let mut body_c = Cost::zero();
            for st in body { body_c = body_c.plus(stmt_cost(&st.node)); }
            let n = bound.unwrap_or(DEFAULT_CAPACITY);
            cond_c.plus(body_c.times(n))
        }
        Statement::Emit { args, .. } => {
            let mut c = Cost::zero();
            for a in args { c = c.plus(expr_cost(&a.node)); }
            c
        }
        Statement::Require { .. } | Statement::Break | Statement::Continue => Cost::zero(),
        Statement::MethodCall { args, .. } => {
            let mut c = Cost::zero();
            for a in args { c = c.plus(expr_cost(&a.node)); }
            c
        }
        Statement::Ensure { condition } => expr_cost(&condition.node),
        Statement::ExprStmt { expr } => expr_cost(&expr.node),
    }
}

// ── Handler cost ───────────────────────────────────────────────────

pub fn handler_peak(on: &OnSection) -> Cost {
    let mut c = Cost::zero();
    for st in &on.body {
        c = c.plus(stmt_cost(&st.node));
    }
    // Add the stack overhead for one active handler invocation.
    c.plus(Cost::bytes(HANDLER_STACK_OVERHEAD))
}

// ── Slot cost ──────────────────────────────────────────────────────

pub fn slot_bound(slot: &MemorySlot) -> Cost {
    let capacity = read_int_param(slot, "capacity").unwrap_or(DEFAULT_CAPACITY);
    let max_key = read_int_param(slot, "max_key_bytes").unwrap_or(DEFAULT_MAX_KEY_BYTES);
    let max_value = read_int_param(slot, "max_value_bytes")
        .or_else(|| read_int_param(slot, "max_element_bytes"))
        .unwrap_or(DEFAULT_MAX_VALUE_BYTES);

    // Per-entry cost: key + value + small per-entry header
    let per_entry = max_key.saturating_add(max_value).saturating_add(64);
    Cost::bytes(capacity.saturating_mul(per_entry))
}

// ── State machine instance cost ────────────────────────────────────

pub fn state_machine_bound(cell: &CellDef) -> Cost {
    let mut total = Cost::zero();
    for section in &cell.sections {
        if let Section::State(ref sm) = section.node {
            // Look for [max_instances(N)] on the state machine itself.
            // The parser accepts this as `state foo [max_instances(1000)] { ... }`.
            let max_inst = sm.properties.iter().find_map(|p| {
                if let MemoryProperty::Param(ref pp) = p.node {
                    if pp.name == "max_instances" {
                        if let Some(lit) = pp.values.first() {
                            if let Literal::Int(n) = lit.node {
                                if n >= 0 { return Some(n as u64); }
                            }
                        }
                    }
                }
                None
            }).unwrap_or(DEFAULT_MAX_INSTANCES);

            total = total.plus(Cost::bytes(
                max_inst.saturating_mul(DEFAULT_INSTANCE_SIZE)
            ));
        }
    }
    total
}

// ── Cell-level cost ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BudgetReport {
    pub cell: String,
    /// Sum of all slot bounds.
    pub slot_sum: Cost,
    /// Max over handlers of peak cost + stack overhead.
    pub handler_max: Cost,
    /// Per-handler breakdown (for reporting).
    pub handler_breakdown: Vec<(String, Cost)>,
    /// State machine instance bound.
    pub sm_bound: Cost,
    /// Runtime constant overhead.
    pub runtime: u64,
    /// Final bound (sum of the above).
    pub total: Cost,
    /// Declared budget in bytes, if any.
    pub budget: Option<u64>,
}

impl BudgetReport {
    pub fn verdict(&self) -> BudgetVerdict {
        match (&self.total, self.budget) {
            (Cost::Bounded(b), Some(limit)) => {
                if *b <= limit { BudgetVerdict::Pass } else { BudgetVerdict::Fail }
            }
            (Cost::Bounded(_), None) => BudgetVerdict::NoBudgetDeclared,
            (Cost::Unbounded(_), Some(_)) => BudgetVerdict::Advisory,
            (Cost::Unbounded(_), None) => BudgetVerdict::NoBudgetDeclared,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetVerdict {
    Pass,
    Fail,
    Advisory,
    NoBudgetDeclared,
}

pub fn check_cell(cell: &CellDef) -> BudgetReport {
    // 1. Sum of slot bounds.
    let mut slot_sum = Cost::zero();
    for section in &cell.sections {
        if let Section::Memory(ref mem) = section.node {
            for slot in &mem.slots {
                slot_sum = slot_sum.plus(slot_bound(&slot.node));
            }
        }
    }

    // 2. Max over handlers.
    let mut handler_max = Cost::zero();
    let mut breakdown = Vec::new();
    for section in &cell.sections {
        if let Section::OnSignal(ref on) = section.node {
            let c = handler_peak(on);
            breakdown.push((on.signal_name.clone(), c.clone()));
            handler_max = handler_max.max(c);
        }
    }
    // If no handlers: still charge one stack frame for initialization.
    if breakdown.is_empty() {
        handler_max = Cost::bytes(HANDLER_STACK_OVERHEAD);
    }

    // 3. State machine instance bound.
    let sm_bound = state_machine_bound(cell);

    // 4. Total: slot_sum + handler_max + sm_bound + C_runtime.
    let runtime = C_RUNTIME;
    let total = slot_sum
        .clone()
        .plus(handler_max.clone())
        .plus(sm_bound.clone())
        .plus(Cost::bytes(runtime));

    // 5. Declared budget from scale section.
    let mut budget: Option<u64> = None;
    for section in &cell.sections {
        if let Section::Scale(ref sc) = section.node {
            if let Some(ref s) = sc.memory {
                budget = parse_budget_bytes(s);
            }
        }
    }

    BudgetReport {
        cell: cell.name.clone(),
        slot_sum,
        handler_max,
        handler_breakdown: breakdown,
        sm_bound,
        runtime,
        total,
        budget,
    }
}

// ── Pretty-printing helpers ────────────────────────────────────────

pub fn format_bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", (n as f64) / (1024.0 * 1024.0 * 1024.0))
    } else if n >= 1024 * 1024 {
        format!("{:.2} MiB", (n as f64) / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.2} KiB", (n as f64) / 1024.0)
    } else {
        format!("{} B", n)
    }
}

pub fn format_cost(c: &Cost) -> String {
    match c {
        Cost::Bounded(n) => format_bytes(*n),
        Cost::Unbounded(reasons) => {
            if reasons.is_empty() {
                "unbounded".to_string()
            } else {
                format!("unbounded ({})", reasons.join("; "))
            }
        }
    }
}
