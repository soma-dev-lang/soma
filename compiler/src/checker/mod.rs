mod properties;
mod signals;
pub mod verify;
pub mod desugar;
pub mod temporal;
pub mod native;
pub mod refinement;
pub mod budget;
pub mod isolation;
pub mod termination;
pub mod composition;
pub mod sum_types;
pub mod determinism;
pub mod capabilities;
pub mod cost;
pub mod effects;
pub mod protocol;
pub mod names;
pub mod literals;
pub mod interpolation_check;
pub mod invariants;
pub mod dispatch;
pub mod dead_code;
pub mod guards;
pub mod habits;
pub mod routes;
pub mod cross_machine;

pub use properties::PropertyChecker;
pub use signals::SignalChecker;

use crate::ast::*;
use crate::registry::Registry;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CheckError {
    #[error("contradictory memory properties on '{slot}': {a} and {b} cannot coexist")]
    PropertyContradiction {
        slot: String,
        a: String,
        b: String,
        span: Span,
    },

    #[error("invalid property combination on '{slot}': {reason}")]
    InvalidPropertyCombination {
        slot: String,
        reason: String,
        span: Span,
    },

    #[error("unmatched await: cell '{cell}' awaits signal '{signal}' but no sibling emits it")]
    UnmatchedAwait {
        cell: String,
        signal: String,
        span: Span,
    },

    #[error("unmatched handler: cell '{cell}' handles signal '{signal}' but no sibling emits it")]
    UnmatchedHandler {
        cell: String,
        signal: String,
        span: Span,
    },

    #[error("signal type mismatch: signal '{signal}' has incompatible parameter types between emitter and handler")]
    SignalTypeMismatch {
        signal: String,
        span: Span,
    },

    #[error("duplicate cell name '{name}' in same scope")]
    DuplicateCellName {
        name: String,
        span: Span,
    },

    #[error("duplicate memory slot '{name}' in cell '{cell}'")]
    DuplicateSlot {
        cell: String,
        name: String,
        span: Span,
    },

    #[error("duplicate signal '{name}' in cell '{cell}'")]
    DuplicateSignal {
        cell: String,
        name: String,
        span: Span,
    },

    #[error("face contract: signal '{signal}' declared in cell '{cell}' has no handler")]
    MissingHandler {
        cell: String,
        signal: String,
        span: Span,
    },

    #[error("face contract: signal '{signal}' in cell '{cell}' declares {expected} params, handler has {actual}")]
    ParamCountMismatch {
        cell: String,
        signal: String,
        expected: usize,
        actual: usize,
        span: Span,
    },

    #[error("checker '{checker}' failed: {reason}")]
    CustomCheckerFailed {
        checker: String,
        reason: String,
        span: Span,
    },

    #[error("scale: shard '{slot}' is not a declared memory slot in cell '{cell}'")]
    ScaleShardNotFound {
        cell: String,
        slot: String,
        span: Span,
    },

    #[error("{message}")]
    SumTypeIssue {
        message: String,
        span: Span,
    },

    /// V1.6: a `[deterministic]` handler made a call that breaks determinism.
    #[error("{message}")]
    DeterminismViolation {
        message: String,
        span: Span,
    },

    /// V1.6: a `think(..., map("requires", [...]))` asks for a capability
    /// not declared by the configured model.
    #[error("{message}")]
    ModelCapabilityMissing {
        message: String,
        span: Span,
    },

    /// V1.6: cost-budget proof obligation failed.
    #[error("{message}")]
    CostExceeded {
        message: String,
        span: Span,
    },

    #[error("scale: shard '{slot}' uses [{prop}] but scale declares consistency: {consistency} — contradictory")]
    ScaleConsistencyMismatch {
        slot: String,
        prop: String,
        consistency: String,
        span: Span,
    },

    #[error("structural promise violated in cell '{cell}': promise '{promise}' is not satisfied")]
    PromiseViolation {
        cell: String,
        promise: String,
        span: Span,
    },

    /// Memory-budget proof failure: the cell's statically computed bound
    /// exceeds the declared `scale { memory: ... }` budget.
    #[error("budget exceeded in cell '{cell}': proven peak {proven} > declared budget {budget}")]
    BudgetExceeded {
        cell: String,
        proven: String,
        budget: String,
        breakdown: String,
        span: Span,
    },

    /// V1.7: a string literal interpolates an identifier that is
    /// provably unknown at that point in the handler.
    #[error("{message}")]
    InterpolationUndefined {
        message: String,
        span: Span,
    },
    /// A static error whose message already names its fix (the part
    /// after " — "); `kind` is the stable machine-readable class.
    #[error("{message}")]
    Static {
        kind: &'static str,
        message: String,
        span: Span,
    },

    /// V1.7: static dispatch resolution failed — undefined function or
    /// ambiguous cross-cell call.
    #[error("{message}")]
    DispatchIssue {
        message: String,
        span: Span,
    },

    /// V1.8: a memory invariant expression is itself invalid (unknown
    /// name, non-builtin call) — it would fail at every runtime check.
    #[error("{message}")]
    InvariantIssue {
        message: String,
        span: Span,
    },

    /// A habit from another language the runtime would reject.
    #[error("{message}")]
    Habit {
        message: String,
        span: Span,
    },

    /// A transition guard reads a name its calling handler never binds.
    #[error("{message}")]
    GuardScope {
        message: String,
        span: Span,
    },

    /// A statement that can never run and changes the result — e.g. the
    /// second half of `return "a" "b"`.
    #[error("{message}")]
    DeadCode {
        message: String,
        span: Span,
    },
}

#[derive(Debug)]
pub enum CheckWarning {
    UnhandledSignal {
        cell: String,
        signal: String,
        span: Span,
    },
    PropertyImplication {
        slot: String,
        flag: String,
        implied: String,
        span: Span,
    },
    UnknownProperty {
        slot: String,
        property: String,
        span: Span,
    },
    UnverifiablePromise {
        cell: String,
        promise: String,
        span: Span,
    },
    AwaitWithoutHandler {
        cell: String,
        signal: String,
        span: Span,
    },
    ScaleEventualConsistency {
        cell: String,
        slot: String,
        span: Span,
    },
    AgentMissingStateMachine {
        cell: String,
        span: Span,
    },
    /// Memory-budget proof obligation: the cell DOES have a declared
    /// `scale { memory: ... }` budget but the static analyser cannot
    /// produce a closed-form bound because some handler calls a builtin
    /// classified as unbounded (think, from_json, http_get, …). The
    /// declared budget is left as an advisory rather than a proof.
    BudgetAdvisory {
        cell: String,
        budget: String,
        bounded_portion: String,
        unbounded_reasons: Vec<String>,
    },
    /// Memory-budget proof success: the cell's statically computed
    /// bound fits within the declared budget. Emitted as an info-level
    /// note so the user can see the proven number.
    BudgetOk {
        cell: String,
        proven: String,
        budget: String,
        breakdown: String,
    },
    /// V1.6: a cost axis (tokens/latency/usd) is advisory because of
    /// an unbounded think() or unknown model.
    CostAdvisory {
        message: String,
        span: Span,
    },
    /// V1.6: a cost axis was proven within budget.
    CostProven {
        message: String,
        span: Span,
    },
    /// V1.7: a call resolves to the caller's own handler while another
    /// cell defines a handler with the same name — the silent-shadowing
    /// trap.
    DispatchShadow {
        message: String,
        span: Span,
    },
    /// A [native] handler whose result depends on the backend.
    NativeSemantics {
        message: String,
        span: Span,
    },
    /// A habit from another language that silently does the wrong thing.
    HabitWarning {
        message: String,
        span: Span,
    },
    /// Statements after return/break/continue in the same block.
    Unreachable {
        message: String,
        span: Span,
    },
    /// V1.7: an interpolation issue inside `try { }` — the error is
    /// catchable by design, so it warns instead of failing the gate.
    InterpolationRecoverable {
        message: String,
        span: Span,
    },
}

impl CheckWarning {
    /// True for informational notes (BudgetOk) that should not be
    /// counted as warnings in the human-readable tally.
    pub fn is_note(&self) -> bool {
        // a descriptive promise is documentation by design: a note, not a
        // warning that nags on every check
        // …and an implied property ('immutable' implies 'consistent'):
        // "7 warnings" was 1 warning and 6 of these
        matches!(self, CheckWarning::BudgetOk { .. } | CheckWarning::CostProven { .. } | CheckWarning::UnverifiablePromise { .. } | CheckWarning::PropertyImplication { .. })
    }
}

impl std::fmt::Display for CheckWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnhandledSignal { cell, signal, .. } => {
                write!(f, "warning: signal '{signal}' emitted by '{cell}' has no handler (signal will be lost)")
            }
            Self::PropertyImplication { slot, flag, implied, .. } => {
                write!(f, "note: '{flag}' on '{slot}' implies '{implied}' (added automatically)")
            }
            Self::UnknownProperty { slot, property, .. } => {
                write!(f, "warning: unknown property '{property}' on '{slot}' (not defined in any loaded cell property)")
            }
            Self::UnverifiablePromise { cell, promise, .. } => {
                write!(f, "note: promise on '{cell}' is documentation (not machine-verifiable): \"{promise}\"")
            }
            Self::AwaitWithoutHandler { cell, signal, .. } => {
                write!(f, "warning: cell '{cell}' declares await '{signal}' but has no handler for it (will it be delivered via bus?)")
            }
            Self::ScaleEventualConsistency { cell, slot, .. } => {
                write!(f, "warning: cell '{cell}' uses eventual consistency on shard '{slot}' — reads after writes may return stale data")
            }
            Self::AgentMissingStateMachine { cell, .. } => {
                write!(f, "warning: agent cell '{cell}' has no state machine — add a state section for verified behavior")
            }
            Self::BudgetAdvisory { cell, budget, bounded_portion, unbounded_reasons } => {
                let reasons = unbounded_reasons.iter()
                    .take(3)
                    .map(|r| format!("\n      → {r}"))
                    .collect::<String>();
                let more = if unbounded_reasons.len() > 3 {
                    format!("\n      → … ({} more)", unbounded_reasons.len() - 3)
                } else {
                    String::new()
                };
                write!(
                    f,
                    "advisory: cell '{cell}' declares budget {budget}; bounded portion is {bounded_portion}, but the following handlers call unbounded builtins so the proof is incomplete:{reasons}{more}"
                )
            }
            Self::BudgetOk { cell, proven, budget, breakdown } => {
                write!(f, "✓ budget proven for cell '{cell}': peak ≤ {proven} ≤ declared {budget}\n    breakdown: {breakdown}")
            }
            Self::CostAdvisory { message, .. } => write!(f, "advisory: {message}"),
            Self::CostProven { message, .. } => write!(f, "✓ {message}"),
            Self::DispatchShadow { message, .. } => write!(f, "warning: {message}"),
            Self::Unreachable { message, .. } => write!(f, "warning: {message}"),
            Self::HabitWarning { message, .. } => write!(f, "warning: {message}"),
            Self::NativeSemantics { message, .. } => write!(f, "warning: {message}"),
            Self::InterpolationRecoverable { message, .. } => {
                write!(f, "warning: {message} (recoverable here — inside try {{ }}, or in a handler a test expects to fail — so not an error)")
            }
        }
    }
}

impl CheckError {
    /// Where in the source this diagnostic points, when it points somewhere.
    pub fn span(&self) -> Option<Span> {
        match self {
            Self::PropertyContradiction { span, .. }
            | Self::InvalidPropertyCombination { span, .. }
            | Self::UnmatchedAwait { span, .. }
            | Self::UnmatchedHandler { span, .. }
            | Self::SignalTypeMismatch { span, .. }
            | Self::DuplicateCellName { span, .. }
            | Self::DuplicateSlot { span, .. }
            | Self::DuplicateSignal { span, .. }
            | Self::MissingHandler { span, .. }
            | Self::ParamCountMismatch { span, .. }
            | Self::CustomCheckerFailed { span, .. }
            | Self::ScaleShardNotFound { span, .. }
            | Self::SumTypeIssue { span, .. }
            | Self::DeterminismViolation { span, .. }
            | Self::ModelCapabilityMissing { span, .. }
            | Self::CostExceeded { span, .. }
            | Self::ScaleConsistencyMismatch { span, .. }
            | Self::PromiseViolation { span, .. }
            | Self::BudgetExceeded { span, .. }
            | Self::InterpolationUndefined { span, .. }
            | Self::Static { span, .. }
            | Self::DispatchIssue { span, .. }
            | Self::InvariantIssue { span, .. }
            | Self::DeadCode { span, .. }
            | Self::GuardScope { span, .. }
            | Self::Habit { span, .. } => Some(*span),
        }
    }
}

impl CheckWarning {
    /// Where in the source this diagnostic points, when it points somewhere.
    pub fn span(&self) -> Option<Span> {
        match self {
            Self::UnhandledSignal { span, .. }
            | Self::PropertyImplication { span, .. }
            | Self::UnknownProperty { span, .. }
            | Self::UnverifiablePromise { span, .. }
            | Self::AwaitWithoutHandler { span, .. }
            | Self::ScaleEventualConsistency { span, .. }
            | Self::AgentMissingStateMachine { span, .. }
            | Self::CostAdvisory { span, .. }
            | Self::CostProven { span, .. }
            | Self::DispatchShadow { span, .. }
            | Self::NativeSemantics { span, .. }
            | Self::Unreachable { span, .. }
            | Self::HabitWarning { span, .. }
            | Self::InterpolationRecoverable { span, .. } => Some(*span),
            Self::BudgetAdvisory { .. }
            | Self::BudgetOk { .. } => None,
        }
    }
}

/// Top-level checker that runs all verification passes.
/// Uses the Registry for data-driven property checking.
pub struct Checker<'a> {
    /// (file name, source text) — lets reports point at file:line:col.
    pub source: Option<(String, String)>,
    pub registry: &'a Registry,
    pub manifest: Option<&'a crate::pkg::manifest::Manifest>,
    pub errors: Vec<CheckError>,
    pub warnings: Vec<CheckWarning>,
    /// cell name → (handler name → body), every top-level cell
    pub all_handlers: cost::AllHandlers,
    /// cells with interpolation / UFCS / pipe calls made explicit (desugar)
    analysis_cells: std::collections::HashMap<String, CellDef>,
}

/// Every predicate name a checker constraint mentions that no structural
/// promise implements, with the span of its `require`.
fn collect_unknown_predicates(c: &Constraint, span: Span, checker: &str, out: &mut Vec<(String, String, Span)>) {
    match c {
        Constraint::Predicate { name, .. } => {
            if !STRUCTURAL_PROMISES.contains(&name.as_str()) {
                out.push((checker.to_string(), name.clone(), span));
            }
        }
        Constraint::Not(inner) => collect_unknown_predicates(&inner.node, span, checker, out),
        Constraint::And(a, b) | Constraint::Or(a, b) => {
            collect_unknown_predicates(&a.node, span, checker, out);
            collect_unknown_predicates(&b.node, span, checker, out);
        }
        _ => {}
    }
}

/// The promise predicates `face { promise <name> }` can name. One that is not
/// here is a typo or an invention: it is refused, never silently true.
const STRUCTURAL_PROMISES: &[&str] = &[
    "all_persistent", "all_encrypted", "all_consistent",
    "has_memory", "has_face", "has_signals", "has_auth",
];

impl<'a> Checker<'a> {
    pub fn new(registry: &'a Registry) -> Self {
        Self {
            source: None,
            registry,
            manifest: None,
            errors: Vec::new(),
            warnings: Vec::new(),
            all_handlers: Default::default(),
            analysis_cells: Default::default(),
        }
    }

    pub fn check(&mut self, program: &Program) {
        // once per program, not per cell: a checker rule naming a predicate
        // nothing implements is refused before it silently holds everywhere
        self.validate_checker_predicates();
        // Structure: something to check, one machine per cell with a start
        // state, unique cell names. Each of these used to pass silently
        // (the runtime kept the LAST machine / merged same-named cells).
        if program.cells.is_empty() {
            self.errors.push(CheckError::Static {
                kind: "empty_program",
                message: "no cell in this file — a program is at least `cell Name { on run() { … } }`".to_string(),
                span: Span { start: 0, end: 0 },
            });
        }
        let mut seen_cells: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for cell in &program.cells {
            if !seen_cells.insert(cell.node.name.as_str()) {
                self.errors.push(CheckError::Static {
                    kind: "duplicate_cell",
                    message: format!("cell '{}' is defined twice — rename one; same-named cells are not merged", cell.node.name),
                    span: cell.span,
                });
            }
            // a slot is a keyed store: `n: Int` was accepted and behaved as a
            // Map, contradicting every doc
            for section in &cell.node.sections {
                if let Section::Memory(ref mem) = section.node {
                    for slot in &mem.slots {
                        let ok = matches!(&slot.node.ty.node,
                            TypeExpr::Generic { name, .. } | TypeExpr::Simple(name) if matches!(name.as_str(), "Map" | "List" | "Log" | "Ledger"));
                        if !ok {
                            self.errors.push(CheckError::Static {
                                kind: "scalar_slot",
                                message: format!(
                                    "slot '{}' is declared as {} — a memory slot is a Map<K, V> or a List<T>; keep a single value as one entry: `{}: Map<String, {}>` and `{}.get(\"value\")`",
                                    slot.node.name, crate::commands::describe::format_type(&slot.node.ty.node),
                                    slot.node.name, crate::commands::describe::format_type(&slot.node.ty.node), slot.node.name
                                ),
                                span: slot.span,
                            });
                        }
                    }
                }
            }
            let machines: Vec<&Spanned<Section>> = cell.node.sections.iter()
                .filter(|s| matches!(s.node, Section::State(_)))
                .collect();
            if machines.len() > 1 {
                self.errors.push(CheckError::Static {
                    kind: "multiple_state_machines",
                    message: format!("cell '{}' declares {} state machines — a cell has one lifecycle; put the second machine in its own cell", cell.node.name, machines.len()),
                    span: machines[1].span,
                });
            }
            for m in machines {
                if let Section::State(ref sm) = m.node {
                    if sm.initial.is_empty() {
                        let first = sm.transitions.first().map(|t| t.node.from.clone()).unwrap_or_else(|| "state".to_string());
                        self.errors.push(CheckError::Static {
                            kind: "no_initial_state",
                            message: format!("state machine '{}' has no start state — add `initial: {}` as its first line", sm.name, first),
                            span: m.span,
                        });
                    }
                }
            }
        }
        // Program-wide sum-type checks (duplicate variants,
        // non-exhaustive matches).
        for issue in sum_types::check_program(program) {
            self.errors.push(CheckError::SumTypeIssue {
                message: issue.message,
                span: issue.span,
            });
        }
        // V1.7: static interpolation check — every string literal in
        // every handler body, scanned with the runtime's segmentation
        // rules. Catches "hello {customr}" before it 500s at runtime.
        // Issues inside try { } are recoverable by design → warnings.
        for issue in interpolation_check::check_program(program) {
            if issue.habit {
                self.warnings.push(CheckWarning::HabitWarning { message: issue.message, span: issue.span });
            } else if issue.warning {
                self.warnings.push(CheckWarning::InterpolationRecoverable {
                    message: issue.message,
                    span: issue.span,
                });
            } else if issue.kind == "undefined_variable" || issue.kind == "undefined_function" {
                self.errors.push(CheckError::InterpolationUndefined {
                    message: issue.message,
                    span: issue.span,
                });
            } else {
                self.errors.push(CheckError::Static {
                    kind: issue.kind,
                    message: issue.message,
                    span: issue.span,
                });
            }
        }
        // V1.7: static dispatch resolution — unknown and ambiguous
        // bare-name calls, plus the recursive-shadowing trap.
        let (dispatch_errors, dispatch_warnings) = dispatch::check_program(program);
        for e in dispatch_errors {
            self.errors.push(CheckError::DispatchIssue {
                message: e.message,
                span: e.span,
            });
        }
        // V1.8: memory invariants must themselves be valid — an
        // invariant referencing an unknown name would reject every
        // write at runtime with an evaluation error.
        for issue in invariants::validate_program(program) {
            self.errors.push(CheckError::InvariantIssue {
                message: issue.message,
                span: issue.span,
            });
        }
        for (message, span) in native::int_division_warnings(program) {
            self.warnings.push(CheckWarning::NativeSemantics { message, span });
        }
        for (message, span) in routes::check_program(program) {
            self.warnings.push(CheckWarning::HabitWarning { message, span });
        }
        for w in invariants::lint_program(program) {
            self.warnings.push(CheckWarning::HabitWarning { message: w.message, span: w.span });
        }
        let (habit_errors, habit_warnings) = habits::check_program(program);
        for e in habit_errors {
            self.errors.push(CheckError::Habit { message: e.message, span: e.span });
        }
        for w in habit_warnings {
            self.warnings.push(CheckWarning::HabitWarning { message: w.message, span: w.span });
        }
        // on the desugared program: a transition in `"{…}"`, a UFCS
        // `id.transition("b")` or a `require` condition escaped the rule
        // a misspelled builtin type (`x: Integer`, `List<Strng>`) was read as
        // an unknown record type — Any — and every value passed
        {
            const BASE: &[&str] = &["Int", "Float", "String", "Bool", "List", "Map", "Any", "BigInt"];
            let declared: std::collections::HashSet<String> = program.cells.iter().map(|c| c.node.name.clone()).collect();
            fn names_in(t: &TypeExpr, out: &mut Vec<String>) {
                match t {
                    TypeExpr::Simple(n) => out.push(n.clone()),
                    TypeExpr::Generic { name, args } => { out.push(name.clone()); for a in args { names_in(&a.node, out); } }
                    _ => {}
                }
            }
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for cell in &program.cells {
                let mut types: Vec<(&TypeExpr, Span)> = Vec::new();
                for sec in &cell.node.sections {
                    match &sec.node {
                        Section::Memory(m) => for sl in &m.slots { types.push((&sl.node.ty.node, sl.span)); },
                        Section::OnSignal(on) => for p in &on.params { types.push((&p.ty.node, sec.span)); },
                        Section::Face(f) => for d in &f.declarations {
                            if let FaceDecl::Signal(sig) = &d.node {
                                for p in &sig.params { types.push((&p.ty.node, d.span)); }
                                if let Some(r) = &sig.return_type { types.push((&r.node, d.span)); }
                            }
                        },
                        _ => {}
                    }
                }
                for (t, span) in types {
                    let mut ns = Vec::new();
                    names_in(t, &mut ns);
                    for n in ns {
                        if BASE.contains(&n.as_str()) || declared.contains(&n) || !seen.insert(n.clone()) { continue; }
                        let lower = n.to_lowercase();
                        let alias = match lower.as_str() {
                            "integer" | "int64" | "int32" | "i64" | "long" => Some("Int"),
                            "str" | "text" => Some("String"),
                            "boolean" => Some("Bool"),
                            "double" | "float64" | "f64" | "number" | "decimal" => Some("Float"),
                            "array" | "vec" => Some("List"),
                            "dict" | "object" | "hashmap" => Some("Map"),
                            "json" | "value" | "anything" | "dynamic" => Some("Any"),
                            _ => None,
                        };
                        let near = alias.map(|a| a.to_string()).or_else(|| { let base: Vec<String> = BASE.iter().map(|b| b.to_string()).collect(); names::suggest(&n, base.iter()).map(|x| x.to_string()) });
                        if let Some(fix) = near {
                            self.errors.push(CheckError::Static {
                                kind: "unknown_type",
                                message: format!("unknown type `{}` — did you mean `{}`? (an undeclared type name would accept any value)", n, fix),
                                span,
                            });
                        }
                    }
                }
            }
        }
        // a builtin called with more arguments than any of its forms takes
        {
            let handler_names: std::collections::HashSet<String> = program.cells.iter().flat_map(|c| c.node.sections.iter().filter_map(|s| match &s.node {
                Section::OnSignal(on) => Some(on.signal_name.clone()), _ => None })).collect();
            let mut reported: std::collections::HashSet<String> = std::collections::HashSet::new();
            // interior cells too: a builtin called with too many arguments
            // was unchecked inside `interior { }`
            for cell in crate::checker::names::collect_cells(program) {
                for sec in &cell.sections {
                    let body = match &sec.node {
                        Section::OnSignal(on) => &on.body,
                        Section::Every(e) | Section::After(e) => &e.body,
                        _ => continue,
                    };
                    let mut piped: std::collections::HashSet<*const Expr> = std::collections::HashSet::new();
                    literals::for_each_expr(body, &mut |e| if let Expr::Pipe { right, .. } = e { piped.insert(&right.node as *const Expr); });
                    let mut bad: Vec<String> = Vec::new();
                    literals::for_each_expr(body, &mut |e| if let Expr::FnCall { name, args } = e {
                        if handler_names.contains(name) { return; }
                        let n = args.len() + if piped.contains(&(e as *const Expr)) { 1 } else { 0 };
                        if let Some(max) = crate::interpreter::builtins::registry::max_arity(name) {
                            if n > max && reported.insert(format!("{}:{}", name, n)) {
                                bad.push(format!("{}() takes at most {} argument{} ({} given) — the extra ones were silently ignored", name, max, if max == 1 { "" } else { "s" }, n));
                            }
                        }
                    });
                    for m in bad {
                        self.errors.push(CheckError::Static { kind: "builtin_arity", message: m, span: sec.span });
                    }
                }
            }
        }
        // a variant literal (`Anon { id: 5, email: … }`) with a missing,
        // extra or literally mistyped field passed check and failed at run
        // time — its declaration is known here
        {
            let mut shapes: std::collections::HashMap<String, Vec<(String, TypeExpr)>> = std::collections::HashMap::new();
            for c in &program.cells {
                if !matches!(c.node.kind, CellKind::Type) { continue; }
                for sec in &c.node.sections {
                    if let Section::Variants(vs) = &sec.node {
                        for vd in &vs.variants {
                            if let VariantFields::Struct(fs) = &vd.node.fields {
                                shapes.insert(vd.node.name.clone(), fs.iter().map(|(n, t)| (n.clone(), t.node.clone())).collect());
                            }
                        }
                    }
                }
            }
            if !shapes.is_empty() {
                for cell in &program.cells {
                    for sec in &cell.node.sections {
                        let body = match &sec.node {
                            Section::OnSignal(on) => &on.body,
                            Section::Every(e) | Section::After(e) => &e.body,
                            _ => continue,
                        };
                        let mut bad: Vec<String> = Vec::new();
                        literals::for_each_expr(body, &mut |e| if let Expr::Record { type_name, fields } = e {
                            let Some(decl) = shapes.get(type_name) else { return };
                            for (n, _) in decl {
                                if !fields.iter().any(|(f, _)| f == n) { bad.push(format!("`{type_name} {{ … }}` is missing the field `{n}`")); }
                            }
                            for (f, v) in fields {
                                match decl.iter().find(|(n, _)| n == f) {
                                    None => bad.push(format!("`{type_name} {{ … }}` has no field `{f}` (the variant declares {})", decl.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", "))),
                                    Some((_, TypeExpr::Simple(t))) => {
                                        let lit = match &v.node {
                                            Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::BigInt(_)) => Some("Int"),
                                            Expr::Literal(Literal::Float(_)) => Some("Float"),
                                            Expr::Literal(Literal::String(_)) => Some("String"),
                                            Expr::Literal(Literal::Bool(_)) => Some("Bool"),
                                            _ => None,
                                        };
                                        if let Some(l) = lit {
                                            let fits = l == t || (t == "Float" && l == "Int") || !matches!(t.as_str(), "Int" | "Float" | "String" | "Bool");
                                            if !fits { bad.push(format!("`{type_name} {{ {f}: … }}` is a {l} literal but the variant declares `{f}: {t}`")); }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        });
                        bad.dedup();
                        for m in bad {
                            self.errors.push(CheckError::Static { kind: "variant_shape", message: m, span: sec.span });
                        }
                    }
                }
            }
        }
        let exposed_for_guards = crate::checker::desugar::expose_for_analysis(program);
        for issue in guards::check_program(&exposed_for_guards) {
            self.errors.push(CheckError::GuardScope { message: issue.message, span: issue.span });
        }
        let (dead_errors, dead_warnings) = dead_code::check_program(program);
        for e in dead_errors {
            self.errors.push(CheckError::DeadCode { message: e.message, span: e.span });
        }
        for w in dead_warnings {
            self.warnings.push(CheckWarning::Unreachable { message: w.message, span: w.span });
        }
        for w in dispatch_warnings {
            self.warnings.push(CheckWarning::DispatchShadow {
                message: w.message,
                span: w.span,
            });
        }
        // handlers of every top-level cell, for cross-cell cost composition
        let analysis = desugar::expose_for_analysis(program);
        self.analysis_cells = analysis.cells.iter().map(|c| (c.node.name.clone(), c.node.clone())).collect();
        self.all_handlers = analysis.cells.iter().map(|c| {
            let mut hs: std::collections::HashMap<String, Vec<Spanned<Statement>>> = c.node.sections.iter().filter_map(|s| match &s.node {
                Section::OnSignal(h) => Some((h.signal_name.clone(), h.body.clone())),
                _ => None,
            }).collect();
            // marks an agent with tools: its think() makes up to 10 rounds,
            // also when another cell calls it
            if c.node.sections.iter().any(|s| matches!(&s.node, Section::Face(f) if f.declarations.iter().any(|d| matches!(d.node, FaceDecl::Tool(_))))) {
                hs.insert("__has_tools__".to_string(), Vec::new());
            }
            (c.node.name.clone(), hs)
        }).collect();
        // two cells with `request`: serve routes only the FIRST — say so
        {
            let routers: Vec<&Spanned<CellDef>> = program.cells.iter().filter(|c| matches!(c.node.kind, CellKind::Cell | CellKind::Agent)
                && c.node.sections.iter().any(|s| matches!(&s.node, Section::OnSignal(on) if on.signal_name == "request"))).collect();
            // the router (or `ws`) of an IMPORTED file: a dependency took the
            // HTTP surface — its public handlers became endpoints and the
            // program's own ones unreachable, with no word from check
            let base = crate::interpreter::IMPORT_SPAN_BASE;
            let main_has_router = routers.iter().any(|c| c.span.start < base);
            for c in program.cells.iter().filter(|c| c.span.start >= base && matches!(c.node.kind, CellKind::Cell | CellKind::Agent)) {
                // an imported `every` / `after` runs under soma serve too
                for sec in &c.node.sections {
                    if matches!(sec.node, Section::Every(_) | Section::After(_)) {
                        self.warnings.push(CheckWarning::DispatchShadow {
                            message: format!("imported cell `{}` declares an `every` / `after` job — it runs under soma serve like your own; if that is not intended, drop the import", c.node.name),
                            span: sec.span,
                        });
                        break;
                    }
                }
                for sec in &c.node.sections {
                    if let Section::OnSignal(on) = &sec.node {
                        if (on.signal_name == "request" && !main_has_router) || on.signal_name == "ws" {
                            self.warnings.push(CheckWarning::DispatchShadow {
                                message: format!("imported cell `{}` defines `{}` — under soma serve it owns {} (its public handlers become endpoints); if that is not intended, define `{}` in your own cell or drop the import",
                                    c.node.name, on.signal_name,
                                    if on.signal_name == "ws" { "the WebSocket port" } else { "HTTP routing, and your own cells' handlers are no longer routed" },
                                    on.signal_name),
                                span: sec.span,
                            });
                        }
                    }
                }
            }
            if routers.len() > 1 {
                self.warnings.push(CheckWarning::DispatchShadow {
                    message: format!("cells {} each define `request` — soma serve routes only the first ({}); merge the routes into one cell",
                        routers.iter().map(|c| c.node.name.as_str()).collect::<Vec<_>>().join(", "), routers[0].node.name),
                    span: routers[1].span,
                });
            }
        }
        // storage tables are named `<Cell>_<slot>` (and `<table>_log`,
        // `<Cell>__sm_<machine>`, `<Cell>__agent_memory`), case-insensitive
        // in SQLite: two names that meet share data (`acct`/`Acct`, cell
        // `User` slot `pass_hash` vs cell `User_pass` slot `hash`, a slot
        // `_sm_flow` rewriting machine instances)
        {
            let mut owners: std::collections::HashMap<String, (String, Span)> = std::collections::HashMap::new();
            let mut reported: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
            for cell in &program.cells {
                if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) { continue; }
                let c = &cell.node.name;
                let mut names: Vec<(String, String)> = vec![
                    (format!("{}__agent_memory", c), format!("{}'s agent memory", c)),
                    (format!("{}__counters", c), format!("{}'s next_id counter", c)),
                ];
                for sec in &cell.node.sections {
                    match &sec.node {
                        Section::Memory(m) => for sl in &m.slots {
                            let t = format!("{}_{}", c, sl.node.name);
                            names.push((t.clone(), format!("slot {}.{}", c, sl.node.name)));
                            names.push((format!("{}_log", t), format!("slot {}.{}", c, sl.node.name)));
                        },
                        Section::State(sm) => {
                            let t = format!("{}__sm_{}", c, sm.name);
                            names.push((t.clone(), format!("state machine {}.{}", c, sm.name)));
                            names.push((format!("{}_log", t), format!("state machine {}.{}", c, sm.name)));
                        }
                        _ => {}
                    }
                }
                for (t, what) in names {
                    let key = t.to_lowercase();
                    match owners.get(&key) {
                        Some((other, _)) if *other != what && reported.insert((other.clone(), what.clone())) => {
                            self.errors.push(CheckError::Static {
                                kind: "storage_collision",
                                message: format!("{} and {} would share the storage table \"{}\" (table names ignore case and `_` joins cell and slot names) — rename one of them", other, what, t),
                                span: cell.span,
                            });
                        }
                        Some(_) => {}
                        None => { owners.insert(key, (what, cell.span)); }
                    }
                }
            }
        }
        // `Store.config.get(k)` from another cell passed check and raised
        // "undefined variable: Store" at run time: a cell's memory is its own
        {
            let slots_of: std::collections::HashMap<String, Vec<String>> = program.cells.iter().map(|c| (c.node.name.clone(), c.node.sections.iter().filter_map(|s| match &s.node {
                Section::Memory(m) => Some(m.slots.iter().map(|sl| sl.node.name.clone()).collect::<Vec<_>>()),
                _ => None,
            }).flatten().collect())).collect();
            // a test rule naming `Store.config`: tests reach slots by their
            // bare name (it raised "undefined variable: Store" after check ✓)
            for cell in program.cells.iter().filter(|c| c.node.kind == CellKind::Test) {
                for sec in &cell.node.sections {
                    let Section::Rules(rules) = &sec.node else { continue };
                    for rule in &rules.rules {
                        let e = match &rule.node {
                            Rule::Assert(e) | Rule::AssertFails(e) | Rule::AssertFailsMatching(e, _) => e,
                            // a property's `ensures` body too (an undefined
                            // function there surfaced only at `soma test`)
                            Rule::Property { body, .. } => body,
                            Rule::Let { value, .. } => value,
                            _ => continue,
                        };
                        let mut hit: Option<(String, String)> = None;
                        literals::for_each_in_expr(&e.node, &mut |x| if let Expr::FieldAccess { target, field } = x {
                            if let Expr::Ident(c) = &target.node {
                                if slots_of.get(c).map_or(false, |v| v.contains(field)) { hit = Some((c.clone(), field.clone())); }
                            }
                        });
                        if let Some((c, f)) = hit {
                            self.errors.push(CheckError::Static {
                                kind: "foreign_slot",
                                message: format!("in a test cell a slot is named without its cell: write `{}` (not `{}.{}`) — test rules read every cell's slots by their bare name", f, c, f),
                                span: rule.span,
                            });
                        }
                    }
                }
            }
            // (interpolation segments exposed: `"{Store.secret}"` too)
            let exposed = crate::checker::desugar::expose_for_analysis(program);
            for cell in &exposed.cells {
                for sec in &cell.node.sections {
                    let body = match &sec.node {
                        Section::OnSignal(on) => &on.body,
                        Section::Every(e) | Section::After(e) => &e.body,
                        _ => continue,
                    };
                    let mut hits: Vec<(String, String)> = Vec::new();
                    literals::for_each_expr(body, &mut |e| {
                        let pair = match e {
                            Expr::FieldAccess { target, field } => match &target.node { Expr::Ident(c) => Some((c.clone(), field.clone())), _ => None },
                            // `Store["secret"]`
                            Expr::Index { target, index } => match (&target.node, &index.node) {
                                (Expr::Ident(c), Expr::Literal(Literal::String(f))) => Some((c.clone(), f.clone())),
                                _ => None,
                            },
                            _ => None,
                        };
                        if let Some((c, field)) = pair {
                            if c != cell.node.name && slots_of.get(&c).map_or(false, |v| v.contains(&field)) && !hits.contains(&(c.clone(), field.clone())) {
                                hits.push((c, field));
                            }
                        }
                    });
                    for (c, f) in hits {
                        self.errors.push(CheckError::Static {
                            kind: "foreign_slot",
                            message: format!("`{}.{}` reads the memory of another cell — a cell's slots are private to it (this raises \"undefined variable: {}\" at run time); add a handler to {} that returns what you need (`on get_{}(k: String) {{ return {}.get(k) }}`) and call `{}.get_{}(k)`", c, f, c, c, f, f, c, f),
                            span: sec.span,
                        });
                    }
                }
            }
        }
        // a transition guard is a CONDITION: a think(), a slot write or a
        // handler call in it ran on every transition() and no analysis saw
        // it (false termination / cost / invariant proofs); an undefined
        // function in it passed check
        {
            let handlers: std::collections::HashSet<String> = program.cells.iter().flat_map(|c| c.node.sections.iter().filter_map(|s| match &s.node {
                Section::OnSignal(on) => Some(on.signal_name.clone()),
                _ => None,
            })).collect();
            let builtins = names::builtin_names();
            for cell in &program.cells {
                for sec in &cell.node.sections {
                    let Section::State(sm) = &sec.node else { continue };
                    for t in &sm.transitions {
                        let Some(g) = &t.node.guard else { continue };
                        let mut bad: Option<String> = None;
                        desugar::for_each_deep(&g.node, &mut |e| {
                            if bad.is_some() { return; }
                            match e {
                                Expr::FnCall { name, .. } if handlers.contains(name) => bad = Some(format!("calls the handler `{}`", name)),
                                Expr::FnCall { name, .. } if names::EFFECT_BUILTINS.contains(&name.as_str()) || names::IO_BUILTINS.contains(&name.as_str()) => bad = Some(format!("calls {}()", name)),
                                Expr::MethodCall { method, .. } if names::EFFECT_BUILTINS.contains(&method.as_str()) || names::IO_BUILTINS.contains(&method.as_str()) => bad = Some(format!("calls .{}()", method)),
                                Expr::FnCall { name, .. } if !builtins.contains(name.as_str()) && !name.starts_with(|c: char| c.is_uppercase()) => bad = Some(format!("calls `{}`, which is not a builtin (undefined function)", name)),
                                Expr::MethodCall { method, .. } if matches!(method.as_str(), "set" | "put" | "delete" | "remove" | "push" | "append" | "clear") || handlers.contains(method) =>
                                    bad = Some(format!("calls .{}()", method)),
                                _ => {}
                            }
                        });
                        // a statement in a block of the guard (`all(q => { hits["g"] = 9  true })`)
                        if bad.is_none() {
                            literals::for_each_stmt_in_expr(&g.node, &mut |st| if bad.is_none() {
                                match st {
                                    Statement::IndexSet { name, .. } => bad = Some(format!("writes `{}[…]`", name)),
                                    Statement::Assign { name, .. } if name.contains('.') => bad = Some(format!("assigns `{}`", name)),
                                    Statement::Emit { signal_name, .. } => bad = Some(format!("emits `{}`", signal_name)),
                                    Statement::MethodCall { target, method, .. } => bad = Some(format!("calls {}.{}()", target, method)),
                                    _ => {}
                                }
                            });
                        }
                        if let Some(what) = bad {
                            self.errors.push(CheckError::Static {
                                kind: "guard_effect",
                                message: format!("the guard of {} -> {} {} — a guard is a condition over the calling handler's locals: it cannot call handlers, think(), or write slots (it would run on every transition(), unseen by termination, cost and invariant proofs); compute the value in the handler before transition() and test that local in the guard", t.node.from, t.node.to, what),
                                span: g.span,
                            });
                        }
                    }
                }
            }
        }
        // `delegate("Nope", "x", 1)` with literal names passed check and
        // raised "no handler found" at run time (a bare call is checked)
        {
            let defs: std::collections::HashMap<String, Vec<String>> = program.cells.iter().map(|c| (c.node.name.clone(), c.node.sections.iter().filter_map(|s| match &s.node {
                Section::OnSignal(on) => Some(on.signal_name.clone()),
                _ => None,
            }).collect())).collect();
            for cell in &program.cells {
                for sec in &cell.node.sections {
                    let body = match &sec.node {
                        Section::OnSignal(on) => &on.body,
                        Section::Every(e) | Section::After(e) => &e.body,
                        _ => continue,
                    };
                    let mut bad: Vec<(bool, String)> = Vec::new();
                    literals::for_each_expr(body, &mut |e| if let Expr::FnCall { name, args } = e {
                        if name == "delegate" && args.len() >= 2 {
                            if let (Expr::Literal(Literal::String(c)), Expr::Literal(Literal::String(h))) = (&args[0].node, &args[1].node) {
                                // a cell of another file composed at run time
                                // is legitimate: a warning; a missing handler
                                // of a cell this program defines is an error
                                let msg = match defs.get(c) {
                                    None => Some((false, format!("delegate(\"{}\", \"{}\", …): no cell named {} in this program — it raises \"no handler found\" unless that cell is loaded beside it", c, h, c))),
                                    Some(hs) if !hs.contains(h) => Some((true, format!("delegate(\"{}\", \"{}\", …): cell {} has no handler '{}'", c, h, c, h))),
                                    _ => None,
                                };
                                if let Some(m) = msg { if !bad.contains(&m) { bad.push(m); } }
                            }
                        }
                    });
                    for (err, m) in bad {
                        if err { self.errors.push(CheckError::Static { kind: "undefined_delegate", message: m, span: sec.span }); }
                        else { self.warnings.push(CheckWarning::HabitWarning { message: m, span: sec.span }); }
                    }
                }
            }
        }
        // `think(p, map("schema", …))`: an unknown option in a literal map
        // passed check and raised at run time
        for cell in &program.cells {
            for sec in &cell.node.sections {
                let body = match &sec.node {
                    Section::OnSignal(on) => &on.body,
                    Section::Every(e) | Section::After(e) => &e.body,
                    _ => continue,
                };
                let mut bad: Vec<String> = Vec::new();
                let tools: Vec<String> = cell.node.sections.iter().filter_map(|s| match &s.node {
                    Section::Face(f) => Some(f.declarations.iter().filter_map(|d| match &d.node { FaceDecl::Tool(t) => Some(t.name.clone()), _ => None }).collect::<Vec<_>>()),
                    _ => None,
                }).flatten().collect();
                let mut not_tools: Vec<String> = Vec::new();
                literals::for_each_expr(body, &mut |e| if let Expr::FnCall { name, args } = e {
                    if matches!(name.as_str(), "think" | "think_json") {
                        for a in args {
                            if let Expr::FnCall { name: m, args: kv } = &a.node {
                                if m == "map" {
                                    for k in kv.iter().step_by(2) {
                                        if let Expr::Literal(Literal::String(k)) = &k.node {
                                            if !matches!(k.as_str(), "max_tokens" | "timeout" | "timeout_ms" | "max_rounds" | "tools_allowed" | "requires") && !bad.contains(k) { bad.push(k.clone()); }
                                        }
                                    }
                                    // `tools_allowed: ["admin"]` naming a handler that is
                                    // not a face tool allowed nothing, silently
                                    for pair in kv.chunks(2) {
                                        if let [k, v] = pair {
                                            if matches!(&k.node, Expr::Literal(Literal::String(k)) if k == "tools_allowed") {
                                                if let Expr::ListLiteral(items) = &v.node {
                                                    for it in items {
                                                        if let Expr::Literal(Literal::String(t)) = &it.node {
                                                            if !tools.contains(t) && !not_tools.contains(t) { not_tools.push(t.clone()); }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
                for t in not_tools {
                    self.errors.push(CheckError::Static { kind: "think_option", message: format!("tools_allowed names \"{}\", which is not a tool of {} — declare it in the face (`tool {}(…) -> … \"what it does\"`) or drop it", t, cell.node.name, t), span: sec.span });
                }
                for k in bad {
                    self.errors.push(CheckError::Static { kind: "think_option", message: format!("\"{}\" is not a think() option — the options are max_tokens, max_rounds, timeout (ms), tools_allowed and requires; a JSON shape is checked by your own code after think_json()", k), span: sec.span });
                }
            }
        }
        // a handler named `transition` / `approve` / `fail` … replaced the
        // builtin in EVERY cell (bare calls resolve to a handler first): an
        // imported `on transition(id, to) { log }` turned every interlock
        // into a no-op while verify still proved the edges
        // (the builtins a proof or a safety gate rests on)
        const RESERVED: &[&str] = &["transition", "approve", "fail", "get_status", "has_state", "valid_transitions",
            "think", "think_json", "set_budget", "tokens_used"];
        for cell in &program.cells {
            for sec in &cell.node.sections {
                if let Section::OnSignal(on) = &sec.node {
                    if RESERVED.contains(&on.signal_name.as_str()) {
                        self.errors.push(CheckError::Static {
                            kind: "reserved_handler",
                            message: format!("a handler cannot be named `{}`: it would replace the builtin {}() for every bare call in the program (state machines, approvals and errors included) — rename it (`on {}_{}(…)`)", on.signal_name, on.signal_name, on.signal_name, cell.node.name.to_lowercase()),
                            span: sec.span,
                        });
                    }
                }
            }
        }
        // `buffer`, `hashmap`, `strbuf` and their operations exist only
        // inside a [native] handler, so a handler may carry one of those
        // names — and then the SAME call text means two things: inside
        // [native] it is the primitive, everywhere else it is the handler.
        // Both compiled, silently, and renaming a handler to `buffer` made
        // every native call stop reaching it.
        {
            let defined: std::collections::HashSet<String> = names::collect_cells(program)
                .iter()
                .flat_map(|c| c.sections.iter().filter_map(|s| match &s.node {
                    Section::OnSignal(on) => Some(on.signal_name.clone()),
                    _ => None,
                }))
                .collect();
            let native_only = names::native_only_names();
            for cell in names::collect_cells(program) {
                for sec in &cell.sections {
                    let Section::OnSignal(on) = &sec.node else { continue };
                    if !on.properties.iter().any(|p| p == "native") { continue }
                    let mut clash: Option<String> = None;
                    literals::for_each_expr(&on.body, &mut |e| {
                        if let Expr::FnCall { name, .. } = e {
                            if clash.is_none()
                                && native_only.contains(name.as_str())
                                && defined.contains(name)
                            {
                                clash = Some(name.clone());
                            }
                        }
                    });
                    if let Some(name) = clash {
                        self.errors.push(CheckError::Static {
                            kind: "native_primitive_shadowed",
                            message: format!(
                                "`{name}(…)` in the [native] handler `{}` is the primitive, not the handler `{name}` this program defines — the same call in an interpreted handler goes to the handler, so one name means two things. Rename the handler (`on {name}_of(…)`)",
                                on.signal_name
                            ),
                            span: sec.span,
                        });
                    }
                }
            }
        }
        // `transition()` in a cell with no state machine: it moved the
        // program's only machine unseen by refinement, think-isolation and
        // the guard rule (or raised at run time with several machines)
        {
            let has_machine = |c: &CellDef| c.sections.iter().any(|s| matches!(s.node, Section::State(_)));
            let machines = program.cells.iter().filter(|c| has_machine(&c.node)).count();
            let any_machine = machines > 0;
            // with exactly ONE machine the runtime moves it (and verify now
            // models those calls); with none or several it can only fail
            for cell in program.cells.iter().filter(|c| machines != 1 && matches!(c.node.kind, CellKind::Cell | CellKind::Agent) && !has_machine(&c.node)) {
                for sec in &cell.node.sections {
                    let body = match &sec.node {
                        Section::OnSignal(on) => &on.body,
                        Section::Every(e) | Section::After(e) => &e.body,
                        _ => continue,
                    };
                    // get_status / has_state / valid_transitions read the same
                    // machine and failed the same way ("no state machine found")
                    let mut hit: Option<String> = None;
                    literals::for_each_expr(body, &mut |e| if let Expr::FnCall { name, .. } = e {
                        if hit.is_none() && matches!(name.as_str(), "transition" | "get_status" | "has_state" | "valid_transitions") { hit = Some(name.clone()); }
                    });
                    if let Some(f) = hit {
                        self.errors.push(CheckError::Static {
                            kind: "transition_no_machine",
                            message: if any_machine {
                                format!("cell {} calls {}() but has no state machine, and the program has {} — it reads the machine of the CALLING cell: call a handler of the cell that owns the machine", cell.node.name, f, if machines > 1 { "several" } else { "none" })
                            } else {
                                format!("cell {} calls {}() but the program declares no state machine", cell.node.name, f)
                            },
                            span: sec.span,
                        });
                    }
                }
            }
        }
        // an undefined function in a test rule passed check and failed only
        // when the test ran
        {
            let mut defined: std::collections::HashSet<String> = std::collections::HashSet::new();
            for c in &program.cells {
                for s in &c.node.sections {
                    if let Section::OnSignal(on) = &s.node { defined.insert(on.signal_name.clone()); }
                }
            }
            let builtins = names::builtin_names();
            for cell in program.cells.iter().filter(|c| c.node.kind == CellKind::Test) {
                for sec in &cell.node.sections {
                    let Section::Rules(rules) = &sec.node else { continue };
                    let mut lets: std::collections::HashSet<String> = std::collections::HashSet::new();
                    for rule in &rules.rules {
                        if let Rule::Let { name, .. } = &rule.node { lets.insert(name.clone()); }
                        let e = match &rule.node {
                            Rule::Assert(e) | Rule::AssertFails(e) | Rule::AssertFailsMatching(e, _) => e,
                            // a property's `ensures` body too (an undefined
                            // function there surfaced only at `soma test`)
                            Rule::Property { body, .. } => body,
                            Rule::Let { value, .. } => value,
                            _ => continue,
                        };
                        let mut params: std::collections::HashSet<String> = std::collections::HashSet::new();
                        literals::for_each_in_expr(&e.node, &mut |x| match x {
                            Expr::Lambda { param, .. } | Expr::LambdaBlock { param, .. } => { params.insert(param.clone()); }
                            _ => {}
                        });
                        let mut bad: Option<String> = None;
                        literals::for_each_in_expr(&e.node, &mut |x| if let Expr::FnCall { name, .. } = x {
                            if bad.is_none() && !defined.contains(name) && !builtins.contains(name.as_str()) && !lets.contains(name)
                                && !params.contains(name) && !name.starts_with(|c: char| c.is_uppercase()) && name != "_coalesce" {
                                bad = Some(name.clone());
                            }
                        });
                        if let Some(n) = bad {
                            let near = names::suggest(&n, defined.iter()).map(|s| format!(" (did you mean '{}'?)", s)).unwrap_or_default();
                            self.errors.push(CheckError::Static {
                                kind: "undefined_function",
                                message: format!("test rule calls undefined function '{}'{} — no cell defines a handler of that name and it is not a builtin", n, near),
                                span: rule.span,
                            });
                        }
                    }
                }
            }
        }
        // `assert_fails transition(id, "x")` in a program with several
        // machines raised "which machine?" (kind type) and passed for the
        // wrong reason; a test names the machine by calling its cell's handler
        {
            let machines = program.cells.iter().filter(|c| c.node.sections.iter().any(|s| matches!(s.node, Section::State(_)))).count();
            if machines > 1 {
                for cell in program.cells.iter().filter(|c| c.node.kind == CellKind::Test) {
                    for sec in &cell.node.sections {
                        let Section::Rules(rules) = &sec.node else { continue };
                        for rule in &rules.rules {
                            let e = match &rule.node {
                                Rule::Assert(e) | Rule::AssertFails(e) | Rule::AssertFailsMatching(e, _) => e,
                                Rule::Property { body, .. } => body,
                                Rule::Let { value, .. } => value,
                                _ => continue,
                            };
                            let mut hit = false;
                            literals::for_each_in_expr(&e.node, &mut |x| if let Expr::FnCall { name, .. } = x {
                                if matches!(name.as_str(), "transition" | "get_status" | "has_state" | "valid_transitions") { hit = true; }
                            });
                            if hit {
                                self.errors.push(CheckError::Static {
                                    kind: "test_machine_ambiguous",
                                    message: "a test rule calls transition() / get_status() directly, but the program has several state machines — it cannot tell which one (and `assert_fails` would pass on that error): call a handler of the cell that owns the machine, and write `assert_fails … matching \"invalid_transition\"`".to_string(),
                                    span: rule.span,
                                });
                            }
                        }
                    }
                }
            }
        }
        // `on ws` is served only from the cell that owns `request` (another
        // cell's ran never, port+1 was not even opened), and a frame is text
        {
            let owner = program.cells.iter().find(|c| c.node.sections.iter().any(|s| matches!(&s.node, Section::OnSignal(on) if on.signal_name == "request"))).map(|c| c.node.name.clone());
            for cell in program.cells.iter().filter(|c| matches!(c.node.kind, CellKind::Cell | CellKind::Agent)) {
                for sec in &cell.node.sections {
                    let Section::OnSignal(on) = &sec.node else { continue };
                    if on.signal_name != "ws" { continue; }
                    if let Some(o) = &owner {
                        if *o != cell.node.name {
                            self.errors.push(CheckError::Static { kind: "ws_placement", message: format!("`on ws` of cell {} never runs: `soma serve` serves the cell that owns `request` ({}) — move `on ws` there", cell.node.name, o), span: sec.span });
                        }
                    }
                    if let Some(p) = on.params.first() {
                        if !matches!(&p.ty.node, TypeExpr::Simple(t) if t == "String" || t == "Any") {
                            self.errors.push(CheckError::Static { kind: "ws_placement", message: "`on ws(msg: String)`: a WebSocket frame arrives as text — parse it with from_json(msg)".to_string(), span: sec.span });
                        }
                    }
                }
            }
        }
        // an invariant is a condition: `invariant vals` / `invariant vals - 3`
        // were truthiness (5 accepted, 0 refused) and verify suggested
        // `require v - 3 else …`, itself invalid
        for cell in &program.cells {
            for sec in &cell.node.sections {
                let Section::Memory(m) = &sec.node else { continue };
                for inv in &m.invariants {
                    let not_bool = match &inv.node {
                        Expr::Ident(_) | Expr::Literal(Literal::Int(_) | Literal::Float(_) | Literal::String(_) | Literal::BigInt(_)) => true,
                        Expr::BinaryOp { op, .. } => !matches!(op, BinOp::And | BinOp::Or),
                        _ => false,
                    };
                    if not_bool {
                        self.errors.push(CheckError::Static {
                            kind: "invariant_not_bool",
                            message: format!("invariant `{}` is not a condition — compare it (`{} >= 0`, `size <= 100`)", crate::ast::render_expr(&inv.node), crate::ast::render_expr(&inv.node)),
                            span: inv.span,
                        });
                    }
                }
            }
        }
        // `every 0ms` ran its body back to back (77,865 commits in 3 s)
        for cell in &program.cells {
            for sec in &cell.node.sections {
                if let Section::Every(e) = &sec.node {
                    if e.interval_ms == 0 {
                        self.errors.push(CheckError::Static {
                            kind: "every_zero",
                            message: "`every 0ms` would run its body back to back, forever — give it a period of at least 1ms".to_string(),
                            span: sec.span,
                        });
                    }
                }
            }
        }
        for cell in &program.cells {
            // `cell test T { }` ran "0 tests: 0 passed" and exited 0
            if cell.node.kind == CellKind::Test && !cell.node.sections.iter().any(|s| matches!(s.node, Section::Rules(_))) {
                self.errors.push(CheckError::Static {
                    kind: "empty_test_cell",
                    message: format!("test cell '{}' has no `rules {{ }}` block — it would run zero assertions and pass", cell.node.name),
                    span: cell.span,
                });
            }
            // Skip meta-cells (they define the language, not the program)
            if cell.node.kind != CellKind::Cell && cell.node.kind != CellKind::Agent {
                continue;
            }
            self.check_cell(&cell.node);
        }
    }

    fn check_cell(&mut self, cell: &CellDef) {
        // 1. Structural checks
        self.check_structure(cell);
        self.check_face_return_literals(cell);
        self.check_native_vocabulary(cell);
        for issue in literals::check_cell(cell) {
            self.errors.push(CheckError::Static { kind: issue.kind, message: issue.message, span: issue.span });
        }

        // 2. Property checks (data-driven from registry)
        let mut prop_checker = PropertyChecker::new(self.registry);
        for section in &cell.sections {
            if let Section::Memory(ref mem) = section.node {
                for slot in &mem.slots {
                    prop_checker.check_slot(&slot.node, slot.span);
                }
            }
        }
        self.errors.extend(prop_checker.errors);
        self.warnings.extend(prop_checker.warnings);

        // 3. Signal checks within interior
        for section in &cell.sections {
            if let Section::Interior(ref interior) = section.node {
                let mut sig_checker = SignalChecker::new();
                sig_checker.check_siblings(&interior.cells);
                self.errors.extend(sig_checker.errors);
                self.warnings.extend(sig_checker.warnings);

                // 4. Recurse into children
                for child in &interior.cells {
                    if child.node.kind == CellKind::Cell {
                        self.check_cell(&child.node);
                    }
                }
            }
        }

        // 4b. V1.6: enforce [deterministic] handler contracts.
        for section in &cell.sections {
            if let Section::OnSignal(ref handler) = section.node {
                if handler.properties.iter().any(|p| p == "deterministic") {
                    let errs = determinism::check_deterministic_handler(
                        &handler.signal_name, &handler.body,
                    );
                    for e in errs {
                        self.errors.push(CheckError::DeterminismViolation {
                            message: e.to_string(),
                            span: section.span,
                        });
                    }
                }
            }
        }

        // 4d. V1.6: cost-budget proof. Walks think()/http_*/loop sites
        // and proves peak ≤ declared. Advisory if any think() is unbounded.
        let analysis_cell = self.analysis_cells.get(&cell.name).cloned().unwrap_or_else(|| cell.clone());
        for finding in cost::check_cell(&analysis_cell, self.manifest, &self.all_handlers) {
            match finding {
                cost::CostFinding::Exceeded { .. } => {
                    self.errors.push(CheckError::CostExceeded {
                        message: finding.to_string(),
                        span: Span::new(0, 0),
                    });
                }
                cost::CostFinding::Advisory { .. } => {
                    // a DECLARED bound that cannot be proven is a failed
                    // promise, not a note: a CI gate on the exit code let
                    // an unbounded think() through
                    self.errors.push(CheckError::Static {
                        kind: "cost_unprovable",
                        message: format!("{} — give every think() a literal max_tokens and every loop around it a literal bound, or remove the cost declaration", finding),
                        span: Span::new(0, 0),
                    });
                }
                cost::CostFinding::Proven { .. } => {
                    self.warnings.push(CheckWarning::CostProven {
                        message: finding.to_string(),
                        span: Span::new(0, 0),
                    });
                }
            }
        }

        // 4c. V1.6: model capability contracts on think(..., map("requires", [...])).
        for e in capabilities::check_cell(cell, self.manifest) {
            // Locate the span as the cell's first OnSignal section (best effort).
            let span = cell.sections.iter()
                .find_map(|s| match &s.node {
                    Section::OnSignal(h) if h.signal_name == e.handler_name => Some(s.span),
                    _ => None,
                })
                .unwrap_or(Span::new(0, 0));
            self.errors.push(CheckError::ModelCapabilityMissing {
                message: e.to_string(),
                span,
            });
        }

        // 5. Verify face contracts: signals have handlers, param counts match
        self.check_face_contracts(cell);

        // 6. Verify structural promises
        self.check_promises(cell);

        // 7. Run custom checkers from registry
        self.run_custom_checkers(cell);

        // 8. Verify scale section
        self.check_scale(cell);

        // 9. Agent-specific checks
        if cell.kind == CellKind::Agent {
            self.check_agent_contracts(cell);
        }

        // 10. Memory-budget proof obligation (V1.4).
        // If the cell declares scale { memory: "..." }, the budget
        // checker either proves the bound, fails, or downgrades to
        // an advisory if a handler calls an unbounded builtin.
        self.check_memory_budget(cell);

        // 11. Aggregate budget for interior children (V1.4).
        // Interior cells run in the same process as their parent.
        // The parent's declared budget must cover the sum of its own
        // peak plus all children's peaks. Children's own budgets are
        // sub-budgets within the parent's total.
        self.check_aggregate_budget(cell);
    }

    fn check_aggregate_budget(&mut self, cell: &CellDef) {
        // Only applies if the parent declares a budget AND has interior children.
        let parent_budget_bytes = cell.sections.iter().find_map(|s| {
            if let Section::Scale(ref sc) = s.node {
                sc.memory.as_ref().and_then(|m| budget::parse_budget_bytes(m))
            } else {
                None
            }
        });
        let parent_budget_bytes = match parent_budget_bytes {
            Some(b) => b,
            None => return,
        };

        let mut children: Vec<(String, budget::BudgetReport)> = Vec::new();
        for section in &cell.sections {
            if let Section::Interior(ref interior) = section.node {
                for child in &interior.cells {
                    if child.node.kind == CellKind::Cell || child.node.kind == CellKind::Agent {
                        let report = budget::check_cell(&child.node);
                        children.push((child.node.name.clone(), report));
                    }
                }
            }
        }

        if children.is_empty() {
            return;
        }

        let parent_report = budget::check_cell(cell);

        // Aggregate = parent's own peak + sum of children's peaks.
        let mut aggregate = parent_report.total.clone();
        let mut any_unbounded = false;
        let mut child_summaries = Vec::new();

        for (name, child_report) in &children {
            match &child_report.total {
                budget::Cost::Bounded(n) => {
                    child_summaries.push(format!("{}: {}", name, budget::format_bytes(*n)));
                    aggregate = aggregate.plus(budget::Cost::bytes(*n));
                }
                budget::Cost::Unbounded(reasons) => {
                    any_unbounded = true;
                    child_summaries.push(format!("{}: unbounded", name));
                    aggregate = aggregate.plus(budget::Cost::unbounded(
                        format!("interior cell '{}' is unbounded", name)
                    ));
                    let _ = reasons; // suppress unused
                }
            }
        }

        match &aggregate {
            budget::Cost::Bounded(total) => {
                if *total > parent_budget_bytes {
                    let span = cell.sections.iter()
                        .find(|s| matches!(s.node, Section::Scale(_)))
                        .map(|s| s.span)
                        .unwrap_or_else(|| Span::new(0, 0));
                    self.errors.push(CheckError::BudgetExceeded {
                        cell: cell.name.clone(),
                        proven: budget::format_bytes(*total),
                        budget: budget::format_bytes(parent_budget_bytes),
                        breakdown: format!(
                            "parent {} + interior children [{}]",
                            budget::format_cost(&parent_report.total),
                            child_summaries.join(", ")
                        ),
                        span,
                    });
                } else {
                    self.warnings.push(CheckWarning::BudgetOk {
                        cell: format!("{} (aggregate with interior)", cell.name),
                        proven: budget::format_bytes(*total),
                        budget: budget::format_bytes(parent_budget_bytes),
                        breakdown: format!(
                            "parent {} + interior [{}]",
                            budget::format_cost(&parent_report.total),
                            child_summaries.join(", ")
                        ),
                    });
                }
            }
            budget::Cost::Unbounded(reasons) => {
                if any_unbounded {
                    self.warnings.push(CheckWarning::BudgetAdvisory {
                        cell: format!("{} (aggregate)", cell.name),
                        budget: budget::format_bytes(parent_budget_bytes),
                        bounded_portion: budget::format_cost(&parent_report.total),
                        unbounded_reasons: reasons.clone(),
                    });
                }
            }
        }
    }

    fn check_memory_budget(&mut self, cell: &CellDef) {
        let report = budget::check_cell(cell);
        match report.verdict() {
            budget::BudgetVerdict::NoBudgetDeclared => { /* opt-in: silent */ }
            budget::BudgetVerdict::Pass => {
                let proven = budget::format_cost(&report.total);
                let budget_str = report
                    .budget
                    .map(budget::format_bytes)
                    .unwrap_or_else(|| "?".to_string());
                let breakdown = format!(
                    "slots {} + max-handler {} + state {} + runtime {}",
                    budget::format_cost(&report.slot_sum),
                    budget::format_cost(&report.handler_max),
                    budget::format_cost(&report.sm_bound),
                    budget::format_bytes(report.runtime),
                );
                self.warnings.push(CheckWarning::BudgetOk {
                    cell: cell.name.clone(),
                    proven,
                    budget: budget_str,
                    breakdown,
                });
            }
            budget::BudgetVerdict::Fail => {
                let proven = budget::format_cost(&report.total);
                let budget_str = report
                    .budget
                    .map(budget::format_bytes)
                    .unwrap_or_else(|| "?".to_string());
                let breakdown = format!(
                    "slots {} + max-handler {} + state {} + runtime {}",
                    budget::format_cost(&report.slot_sum),
                    budget::format_cost(&report.handler_max),
                    budget::format_cost(&report.sm_bound),
                    budget::format_bytes(report.runtime),
                );
                let span = cell
                    .sections
                    .iter()
                    .find(|s| matches!(s.node, Section::Scale(_)))
                    .map(|s| s.span)
                    .unwrap_or_else(|| Span::new(0, 0));
                self.errors.push(CheckError::BudgetExceeded {
                    cell: cell.name.clone(),
                    proven,
                    budget: budget_str,
                    breakdown,
                    span,
                });
            }
            budget::BudgetVerdict::Advisory => {
                // Total is Unbounded — collect the reasons.
                let reasons: Vec<String> = match &report.total {
                    budget::Cost::Unbounded(rs) => rs.clone(),
                    _ => vec![],
                };
                // Compute the bounded portion (slots + state + runtime;
                // skip handler_max which is the unbounded one).
                let bounded_only = report
                    .slot_sum
                    .clone()
                    .plus(report.sm_bound.clone())
                    .plus(budget::Cost::bytes(report.runtime));
                let bounded_str = budget::format_cost(&bounded_only);
                let budget_str = report
                    .budget
                    .map(budget::format_bytes)
                    .unwrap_or_else(|| "?".to_string());
                self.warnings.push(CheckWarning::BudgetAdvisory {
                    cell: cell.name.clone(),
                    budget: budget_str,
                    bounded_portion: bounded_str,
                    unbounded_reasons: reasons,
                });
            }
        }
    }

    /// Verify agent cells have required structure
    fn check_agent_contracts(&mut self, cell: &CellDef) {
        // Agent cells SHOULD have a state machine (warning, not error — for flexibility)
        let has_state = cell.sections.iter().any(|s| matches!(s.node, Section::State(_)));
        if !has_state {
            self.warnings.push(CheckWarning::AgentMissingStateMachine {
                cell: cell.name.clone(),
                span: Span { start: 0, end: 0 },
            });
        }

        // Every tool declaration MUST have a matching handler
        for section in &cell.sections {
            if let Section::Face(face) = &section.node {
                for decl in &face.declarations {
                    if let FaceDecl::Tool(tool) = &decl.node {
                        let has_handler = cell.sections.iter().any(|s| {
                            if let Section::OnSignal(on) = &s.node {
                                on.signal_name == tool.name
                            } else { false }
                        });
                        if !has_handler {
                            self.errors.push(CheckError::MissingHandler {
                                cell: cell.name.clone(),
                                signal: format!("{} (tool)", tool.name),
                                span: decl.span,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Verify face contracts: every declared signal has a handler with matching params
    fn check_face_contracts(&mut self, cell: &CellDef) {
        // Collect face signal declarations
        let mut face_signals: Vec<(&SignalDecl, Span)> = Vec::new();
        let mut face_awaits: Vec<(&AwaitDecl, Span)> = Vec::new();

        for section in &cell.sections {
            if let Section::Face(ref face) = section.node {
                for decl in &face.declarations {
                    match &decl.node {
                        FaceDecl::Signal(sig) => face_signals.push((sig, decl.span)),
                        FaceDecl::Await(aw) => face_awaits.push((aw, decl.span)),
                        _ => {}
                    }
                }
            }
        }

        // Collect handlers
        let handlers: Vec<(&OnSection, Span)> = cell.sections.iter()
            .filter_map(|s| {
                if let Section::OnSignal(ref on) = s.node { Some((on, s.span)) } else { None }
            })
            .collect();

        // Check: every face signal has a handler
        for (sig, span) in &face_signals {
            let handler = handlers.iter().find(|(h, _)| h.signal_name == sig.name);
            match handler {
                None => {
                    self.errors.push(CheckError::MissingHandler {
                        cell: cell.name.clone(),
                        signal: sig.name.clone(),
                        span: *span,
                    });
                }
                Some((h, _)) => {
                    // Check param count matches
                    if h.params.len() != sig.params.len() {
                        self.errors.push(CheckError::ParamCountMismatch {
                            cell: cell.name.clone(),
                            signal: sig.name.clone(),
                            expected: sig.params.len(),
                            actual: h.params.len(),
                            span: *span,
                        });
                    } else {
                        // …and each one, in order: `signal setup(capacity: Int,
                        // name: String)` over `on setup(name, capacity)` was
                        // published by describe and failed every caller
                        for (i, (fp, hp)) in sig.params.iter().zip(h.params.iter()).enumerate() {
                            let (ft, ht) = (crate::commands::describe::format_type(&fp.ty.node), crate::commands::describe::format_type(&hp.ty.node));
                            // positional: the TYPE at each position must agree
                            // (a face parameter's name is documentation)
                            // the NAME matters too: an HTTP JSON body and
                            // `soma describe` follow the face, the handler
                            // reads its own parameter names
                            if fp.name != hp.name {
                                self.warnings.push(CheckWarning::HabitWarning {
                                    message: format!("parameter {} of `{}`: the face calls it `{}`, the handler `{}` — an HTTP JSON body is matched by NAME (the handler's), and `soma describe` publishes the face's: use one name", i + 1, sig.name, fp.name, hp.name),
                                    span: *span,
                                });
                            }
                            if ft != ht && ft != "Any" && ht != "Any" {
                                self.errors.push(CheckError::Static {
                                    kind: "face_mismatch",
                                    message: format!("parameter {} of `{}`: the face declares `{}: {}` but the handler takes `{}: {}` — callers follow the face (describe publishes it); make them agree", i + 1, sig.name, fp.name, ft, hp.name, ht),
                                    span: *span,
                                });
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Check: every await has a handler (warning, not error — might come via bus)
        for (aw, span) in &face_awaits {
            let has_handler = handlers.iter().any(|(h, _)| h.signal_name == aw.name);
            if !has_handler {
                self.warnings.push(CheckWarning::AwaitWithoutHandler {
                    cell: cell.name.clone(),
                    signal: aw.name.clone(),
                    span: *span,
                });
            }
        }
    }

    /// Verify structural promises — promises that can be checked at compile time.
    /// Structural promises use known predicate names:
    ///   promise all_persistent   — every memory slot has [persistent]
    ///   promise all_encrypted    — every memory slot has [encrypted]
    ///   promise has_memory       — cell declares at least one memory slot
    ///   promise has_face         — cell has a face section
    /// Descriptive promises (strings) get a warning.
    fn check_promises(&mut self, cell: &CellDef) {
        for section in &cell.sections {
            if let Section::Face(ref face) = section.node {
                for decl in &face.declarations {
                    if let FaceDecl::Promise(ref p) = decl.node {
                        match &p.constraint.node {
                            Constraint::Descriptive(text) => {
                                self.warnings.push(CheckWarning::UnverifiablePromise {
                                    cell: cell.name.clone(),
                                    promise: text.clone(),
                                    span: p.constraint.span,
                                });
                            }
                            Constraint::Predicate { name, .. } => {
                                // an unknown predicate used to pass silently, so a
                                // typo turned a checked guarantee into a no-op
                                // (`promise all_persistemt` was green on a cell with
                                // no persistent slot at all)
                                if !STRUCTURAL_PROMISES.contains(&name.as_str()) {
                                    let near = crate::checker::names::suggest(name, STRUCTURAL_PROMISES.iter().map(|s| s.to_string()).collect::<Vec<_>>().iter())
                                        .map(|s| format!(" (did you mean '{}'?)", s))
                                        .unwrap_or_default();
                                    self.errors.push(CheckError::Static {
                                        kind: "unknown_promise",
                                        message: format!(
                                            "cell '{}' promises '{}'{}, which nothing checks — a promise no check backs proves nothing. The structural promises are: {}. For a note, quote it: `promise \"{}\"`",
                                            cell.name, name, near, STRUCTURAL_PROMISES.join(", "), name),
                                        span: p.constraint.span,
                                    });
                                    continue;
                                }
                                let ok = self.verify_structural_promise(cell, name);
                                if !ok {
                                    self.errors.push(CheckError::PromiseViolation {
                                        cell: cell.name.clone(),
                                        promise: name.clone(),
                                        span: p.constraint.span,
                                    });
                                }
                            }
                            // Comparison promises (value >= 0) are runtime-checked
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    /// Check a structural promise predicate against a cell's structure
    fn verify_structural_promise(&self, cell: &CellDef, predicate: &str) -> bool {
        debug_assert!(STRUCTURAL_PROMISES.contains(&predicate));
        match predicate {
            "all_persistent" => self.all_slots_have_property(cell, "persistent"),
            "all_encrypted" => self.all_slots_have_property(cell, "encrypted"),
            "all_consistent" => self.all_slots_have_property(cell, "consistent"),
            "has_memory" => cell.sections.iter().any(|s| matches!(s.node, Section::Memory(_))),
            "has_face" => cell.sections.iter().any(|s| matches!(s.node, Section::Face(_))),
            "has_signals" => self.cell_has_signals(cell),
            "has_auth" => self.cell_has_given(cell, "auth") || self.cell_has_given(cell, "token"),
            _ => true, // unreachable: check_promises refuses an unknown predicate
        }
    }

    fn all_slots_have_property(&self, cell: &CellDef, prop_name: &str) -> bool {
        for section in &cell.sections {
            if let Section::Memory(ref mem) = section.node {
                for slot in &mem.slots {
                    let has_prop = slot.node.properties.iter()
                        .any(|p| p.node.name() == prop_name);
                    if !has_prop {
                        return false;
                    }
                }
            }
        }
        // All slots checked (or none exist — vacuously true)
        true
    }

    /// Verify scale section: shard references valid memory, consistency matches properties
    fn check_scale(&mut self, cell: &CellDef) {
        // Collect memory slot names and their properties
        let mut slots: Vec<(String, Vec<String>, Span)> = Vec::new();
        for section in &cell.sections {
            if let Section::Memory(ref mem) = section.node {
                for slot in &mem.slots {
                    let props: Vec<String> = slot.node.properties.iter()
                        .map(|p| p.node.name().to_string())
                        .collect();
                    slots.push((slot.node.name.clone(), props, slot.span));
                }
            }
        }

        for section in &cell.sections {
            if let Section::Scale(ref scale) = section.node {
                // Check shard references a valid memory slot
                if let Some(ref shard_name) = scale.shard {
                    let slot = slots.iter().find(|(name, _, _)| name == shard_name);
                    match slot {
                        None => {
                            self.errors.push(CheckError::ScaleShardNotFound {
                                cell: cell.name.clone(),
                                slot: shard_name.clone(),
                                span: section.span,
                            });
                        }
                        Some((_, props, _slot_span)) => {
                            // Check consistency coherence
                            let has_consistent = props.iter().any(|p| p == "consistent");
                            let has_ephemeral = props.iter().any(|p| p == "ephemeral");

                            // [ephemeral] + strong consistency is contradictory
                            if has_ephemeral && scale.consistency == ScaleConsistency::Strong {
                                self.errors.push(CheckError::ScaleConsistencyMismatch {
                                    slot: shard_name.clone(),
                                    prop: "ephemeral".to_string(),
                                    consistency: "strong".to_string(),
                                    span: section.span,
                                });
                            }

                            // [consistent] + eventual is a warning (you declared consistent but accept stale reads)
                            if has_consistent && scale.consistency == ScaleConsistency::Eventual {
                                self.warnings.push(CheckWarning::ScaleEventualConsistency {
                                    cell: cell.name.clone(),
                                    slot: shard_name.clone(),
                                    span: section.span,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    /// Once per program: a `cell checker` rule naming a predicate nothing
    /// implements enforced nothing (`require has_auht` was silently true for
    /// every cell), and under a `!` it failed every cell instead.
    fn validate_checker_predicates(&mut self) {
        let mut bad: Vec<(String, String, Span)> = Vec::new();
        for def in &self.registry.checkers {
            for stmt in &def.check_body {
                if let Statement::Require { constraint, .. } = &stmt.node {
                    collect_unknown_predicates(&constraint.node, constraint.span, &def.name, &mut bad);
                }
            }
        }
        for (checker, name, span) in bad {
            let near = crate::checker::names::suggest(&name, STRUCTURAL_PROMISES.iter().map(|s| s.to_string()).collect::<Vec<_>>().iter())
                .map(|s| format!(" (did you mean '{}'?)", s))
                .unwrap_or_default();
            self.errors.push(CheckError::Static {
                kind: "unknown_checker_predicate",
                message: format!(
                    "checker '{}' requires '{}'{}, which nothing implements — the rule would hold for every cell (and fail every cell under `!`). The predicates are: {}",
                    checker, name, near, STRUCTURAL_PROMISES.join(", ")),
                span,
            });
        }
    }

    fn run_custom_checkers(&mut self, cell: &CellDef) {
        for checker_def in &self.registry.checkers {
            // For each checker, evaluate its check body against this cell
            // For now, we support a simple pattern: check statements that
            // look at the cell's face for specific patterns
            for stmt in &checker_def.check_body {
                if let Statement::Require { constraint, else_signal } = &stmt.node {
                    // an unknown predicate is already an error of its own:
                    // do not ALSO fire this rule against every cell
                    let mut unknown = Vec::new();
                    collect_unknown_predicates(&constraint.node, constraint.span, &checker_def.name, &mut unknown);
                    if !unknown.is_empty() { continue; }
                    let satisfied = self.evaluate_checker_constraint(cell, &constraint.node);
                    if !satisfied {
                        self.errors.push(CheckError::CustomCheckerFailed {
                            checker: checker_def.name.clone(),
                            reason: format!(
                                "cell '{}' failed check '{}' ({})",
                                cell.name,
                                else_signal,
                                checker_def.promises.first().map(|s| s.as_str()).unwrap_or(""),
                            ),
                            span: constraint.span,
                        });
                    }
                }
            }
        }
    }

    /// Evaluate a checker constraint against a cell.
    /// This is a simplified interpreter — it supports predicate names
    /// that map to structural checks on the cell.
    fn evaluate_checker_constraint(&self, cell: &CellDef, constraint: &Constraint) -> bool {
        match constraint {
            // one vocabulary with `face { promise … }`: a checker could only
            // read four of the seven (`require all_persistent` was silently
            // true), and an unknown name passed — refused up front now, so
            // reaching here with one is impossible
            Constraint::Predicate { name, .. } if STRUCTURAL_PROMISES.contains(&name.as_str()) =>
                self.verify_structural_promise(cell, name),
            Constraint::Predicate { .. } => true,
            Constraint::Not(inner) => !self.evaluate_checker_constraint(cell, &inner.node),
            Constraint::And(a, b) => {
                self.evaluate_checker_constraint(cell, &a.node)
                    && self.evaluate_checker_constraint(cell, &b.node)
            }
            Constraint::Or(a, b) => {
                self.evaluate_checker_constraint(cell, &a.node)
                    || self.evaluate_checker_constraint(cell, &b.node)
            }
            _ => true, // Comparison/descriptive constraints pass
        }
    }

    fn cell_has_given(&self, cell: &CellDef, name: &str) -> bool {
        for section in &cell.sections {
            if let Section::Face(ref face) = section.node {
                for decl in &face.declarations {
                    if let FaceDecl::Given(ref g) = decl.node {
                        if g.name == name {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    fn cell_has_signals(&self, cell: &CellDef) -> bool {
        for section in &cell.sections {
            if let Section::Face(ref face) = section.node {
                for decl in &face.declarations {
                    if matches!(decl.node, FaceDecl::Signal(_)) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// `[native]` bodies use a restricted vocabulary: report every handler
    /// that steps outside it HERE, not one per `soma run` after a rustc
    /// round-trip.
    fn check_native_vocabulary(&mut self, cell: &CellDef) {
        let natives: Vec<&OnSection> = cell.sections.iter().filter_map(|s| match &s.node {
            Section::OnSignal(h) if h.properties.iter().any(|p| p == "native") => Some(h),
            _ => None,
        }).collect();
        if natives.is_empty() { return; }
        let siblings: native::NativeSiblings = natives.iter().map(|h| h.signal_name.clone()).collect();
        let errors_before = self.errors.len();
        // `f` and `f_fast`: the generated `inner_handler_f_fast` of each
        // collided (rustc E0428)
        for h in natives.iter() {
            let twin = format!("{}_fast", h.signal_name);
            if natives.iter().any(|o| o.signal_name == twin) {
                self.errors.push(CheckError::Static {
                    kind: "native_vocabulary",
                    message: format!("[native] handlers '{}' and '{}' collide in the generated code (the first one's fast path is named '{}') — rename one", h.signal_name, twin, twin),
                    span: Span { start: 0, end: 0 },
                });
            }
        }
        for h in natives.iter().copied() {
            if let Err(e) = native::check_native_handler(&h.signal_name, &h.params, &h.body, &siblings) {
                let span = cell.sections.iter().find_map(|s| match &s.node {
                    Section::OnSignal(x) if x.signal_name == h.signal_name => Some(s.span),
                    _ => None,
                }).unwrap_or(Span { start: 0, end: 0 });
                self.errors.push(CheckError::Static {
                    kind: "native_vocabulary",
                    message: format!("handler '{}' is marked [native] but {} — the native vocabulary is numbers, buffer/hashmap/strbuf primitives and sibling [native] handlers (see `soma docs agent`, Performance); drop [native] or move the rest out", h.signal_name, e.reason),
                    span,
                });
            }
        }
        // the vocabulary is fine: run the code generator itself (no rustc)
        // and report what IT refuses — a mixed Int/Float, an if-expression…
        // used to pass check and fail at `soma run` time
        if self.errors.len() == errors_before {
            let handlers: Vec<crate::codegen::native::NativeHandler> = natives.iter().map(|h| crate::codegen::native::NativeHandler {
                cell_name: cell.name.clone(),
                signal_name: h.signal_name.clone(),
                params: h.params.clone(),
                body: h.body.clone(),
                properties: h.properties.clone(),
            }).collect();
            let (src, _) = crate::codegen::native::generate_native_source(&handlers);
            let mut seen_msgs: std::collections::HashSet<String> = std::collections::HashSet::new();
            for line in src.lines() {
                if !seen_msgs.insert(line.trim().to_string()) { continue; }
                let Some(rest) = line.trim().strip_prefix("compile_error!(\"[soma codegen] ") else { continue };
                let msg = rest.trim_end_matches("\");").replace("\\\"", "\"").replace("\\\\", "\\");
                let (hname, text) = match msg.strip_prefix('[').and_then(|m| m.split_once("] ")) {
                    Some((h, t)) => (h.to_string(), t.to_string()),
                    None => (String::new(), msg.clone()),
                };
                let span = cell.sections.iter().find_map(|s| match &s.node {
                    Section::OnSignal(x) if x.signal_name == hname => Some(s.span),
                    _ => None,
                }).unwrap_or(Span { start: 0, end: 0 });
                self.errors.push(CheckError::Static {
                    kind: "native_vocabulary",
                    message: format!("handler '{}' is marked [native] but the native compiler refuses it: {} — drop [native] or rewrite that part", hname, text),
                    span,
                });
            }
        }
    }

    /// `signal f() -> Int` with `return "text"` (or a trailing literal of
    /// the wrong kind): the contract is checkable without running.
    fn check_face_return_literals(&mut self, cell: &CellDef) {
        let declared: Vec<(String, String)> = cell.sections.iter().flat_map(|s| match &s.node {
            Section::Face(face) => face.declarations.iter().filter_map(|d| match &d.node {
                FaceDecl::Signal(sig) => sig.return_type.as_ref().map(|t| (sig.name.clone(), match &t.node {
                    TypeExpr::Simple(n) | TypeExpr::Generic { name: n, .. } => n.clone(),
                    _ => "Any".to_string(),
                })),
                _ => None,
            }).collect::<Vec<_>>(),
            _ => Vec::new(),
        }).collect();
        if declared.is_empty() { return; }
        fn literal_kind(e: &Expr) -> Option<&'static str> {
            match e {
                Expr::Literal(Literal::Int(_)) | Expr::Literal(Literal::BigInt(_)) => Some("Int"),
                Expr::Literal(Literal::Float(_)) => Some("Float"),
                Expr::Literal(Literal::String(_)) => Some("String"),
                Expr::Literal(Literal::Bool(_)) => Some("Bool"),
                Expr::ListLiteral(_) => Some("List"),
                Expr::Record { .. } => Some("Map"),
                Expr::FnCall { name, .. } if name == "map" => Some("Map"),
                Expr::FnCall { name, .. } if name == "list" => Some("List"),
                _ => None,
            }
        }
        fn compatible(declared: &str, got: &str) -> bool {
            declared == got || declared == "Any"
                || (declared == "Float" && got == "Int")
                || (declared == "Map" && got == "Map")
        }
        fn walk(stmts: &[Spanned<Statement>], last_is_value: bool, out: &mut Vec<(Span, &'static str)>) {
            let n = stmts.len();
            for (i, st) in stmts.iter().enumerate() {
                match &st.node {
                    Statement::Return { value } => {
                        if let Some(k) = literal_kind(&value.node) { out.push((value.span, k)); }
                    }
                    Statement::ExprStmt { expr } if last_is_value && i + 1 == n => {
                        if let Some(k) = literal_kind(&expr.node) { out.push((expr.span, k)); }
                    }
                    Statement::If { then_body, else_body, .. } => {
                        walk(then_body, last_is_value && i + 1 == n, out);
                        walk(else_body, last_is_value && i + 1 == n, out);
                    }
                    _ => {}
                }
            }
        }
        for section in &cell.sections {
            if let Section::OnSignal(ref h) = section.node {
                if h.signal_name == "request" { continue; } // the router answers with whatever the route returns
                let Some((_, ty)) = declared.iter().find(|(n, _)| n == &h.signal_name) else { continue };
                let mut found = Vec::new();
                walk(&h.body, true, &mut found);
                for (span, got) in found {
                    if !compatible(ty, got) {
                        self.errors.push(CheckError::Static {
                            kind: "face_return_type",
                            message: format!(
                                "handler '{}' returns a {} literal but its face declares `-> {}` — fix the value or the face",
                                h.signal_name, got, ty
                            ),
                            span,
                        });
                    }
                }
            }
        }
    }

    fn check_structure(&mut self, cell: &CellDef) {
        let mut signal_names: Vec<(String, Span)> = Vec::new();
        let mut slot_names: Vec<(String, Span)> = Vec::new();

        for section in &cell.sections {
            match &section.node {
                Section::Face(face) => {
                    for decl in &face.declarations {
                        let name = match &decl.node {
                            FaceDecl::Signal(s) => Some((&s.name, decl.span)),
                            FaceDecl::Await(a) => Some((&a.name, decl.span)),
                            _ => None,
                        };
                        if let Some((name, span)) = name {
                            if signal_names.iter().any(|(n, _)| n == name) {
                                self.errors.push(CheckError::DuplicateSignal {
                                    cell: cell.name.clone(),
                                    name: name.clone(),
                                    span,
                                });
                            } else {
                                signal_names.push((name.clone(), span));
                            }
                        }
                    }
                }
                // `assert` / `property` rules run under `soma test` for
                // `cell test` only: in an ordinary cell they never run
                Section::Rules(rules) if cell.kind == CellKind::Cell && !rules.rules.is_empty() => {
                    self.errors.push(CheckError::Static {
                        kind: "rules_outside_test",
                        message: format!(
                            "cell '{}' has {} test rule(s) (assert / property) but is not a test cell — they would never run: move them to `cell test {}Tests {{ rules {{ … }} }}`",
                            cell.name, rules.rules.len(), cell.name
                        ),
                        span: section.span,
                    });
                }
                Section::Memory(mem) => {
                    for slot in &mem.slots {
                        if slot_names.iter().any(|(n, _)| n == &slot.node.name) {
                            self.errors.push(CheckError::DuplicateSlot {
                                cell: cell.name.clone(),
                                name: slot.node.name.clone(),
                                span: slot.span,
                            });
                        } else {
                            slot_names.push((slot.node.name.clone(), slot.span));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn report(&self) -> String {
        let mut output = String::new();

        for warning in &self.warnings {
            output.push_str(&format!("{}\n", warning));
            output.push_str(&self.location_block(warning.span()));
        }

        for error in &self.errors {
            output.push_str(&format!("error: {}\n", error));
            output.push_str(&self.location_block(error.span()));
        }

        // Tally only real warnings, not informational notes (BudgetOk).
        let note_count = self.warnings.iter().filter(|w| w.is_note()).count();
        let real_warning_count = self.warnings.len() - note_count;

        if self.errors.is_empty() && real_warning_count == 0 && note_count == 0 {
            output.push_str("✓ All checks passed.\n");
        } else if self.errors.is_empty() {
            if note_count > 0 && real_warning_count == 0 {
                output.push_str(&format!(
                    "✓ All checks passed ({} note{} above).\n",
                    note_count,
                    if note_count == 1 { "" } else { "s" }
                ));
            } else {
                output.push_str(&format!(
                    "✓ {} warning(s), no errors.\n",
                    real_warning_count
                ));
            }
        } else {
            output.push_str(&format!(
                "✗ {} error(s), {} warning(s).\n",
                self.errors.len(),
                real_warning_count
            ));
        }

        output
    }

    /// Machine-readable JSON report for agent consumption.
    /// Each error includes a `fix` field with a concrete repair suggestion.
    /// `  --> file:line:col` plus the source line and a caret, like runtime
    /// errors — empty when the diagnostic has no position or no source.
    fn location_block(&self, span: Option<Span>) -> String {
        let (Some((file, text)), Some(span)) = (&self.source, span) else { return String::new() };
        if span.start == 0 && span.end == 0 {
            return String::new();
        }
        let (line, col) = crate::interpreter::span_to_location(text, span.start);
        let (file, _, _) = crate::interpreter::locate_pos(file, text, span.start);
        let mut block = format!(
            "  --> {}:{}:{}\n{}",
            file,
            line,
            col,
            crate::interpreter::format_error_context(text, span.start)
        );
        if !block.ends_with('\n') {
            block.push('\n');
        }
        block
    }

    /// file / line / col / source line for the JSON report.
    fn location_json(&self, span: Option<Span>, v: &mut serde_json::Value) {
        let (Some((file, text)), Some(span)) = (&self.source, span) else { return };
        if span.start == 0 && span.end == 0 {
            return;
        }
        let (line, col) = crate::interpreter::span_to_location(text, span.start);
        let (file, text, _) = crate::interpreter::locate_pos(file, text, span.start);
        v["file"] = serde_json::json!(file);
        v["line"] = serde_json::json!(line);
        v["col"] = serde_json::json!(col);
        if let Some(src) = text.split('\n').nth(line - 1) {
            v["source_line"] = serde_json::json!(src.trim_end());
        }
    }

    pub fn report_json(&self) -> String {
        let errors: Vec<serde_json::Value> = self.errors.iter().map(|e| {
            let (msg, fix, kind) = Self::error_with_fix(e);
            let mut v = serde_json::json!({
                "level": "error",
                "kind": kind,
                "message": msg,
                "fix": fix,
            });
            self.location_json(e.span(), &mut v);
            v
        }).collect();

        // A proven bound ("✓ cost: 'tokens' bound proven …") is good news,
        // not a warning to act on: it goes under "notes".
        let warnings: Vec<serde_json::Value> = self.warnings.iter().filter(|w| !w.is_note()).map(|w| {
            let (msg, fix) = Self::warning_with_fix(w);
            let mut v = serde_json::json!({
                "level": "warning",
                "message": msg,
                "fix": fix,
            });
            self.location_json(w.span(), &mut v);
            v
        }).collect();
        let notes: Vec<serde_json::Value> = self.warnings.iter().filter(|w| w.is_note()).map(|w| {
            serde_json::json!({ "level": "note", "message": format!("{}", w) })
        }).collect();

        let output = serde_json::json!({
            "passed": self.errors.is_empty(),
            "errors": errors,
            "warnings": warnings,
            "notes": notes,
            "error_count": self.errors.len(),
            "warning_count": self.warnings.iter().filter(|w| !w.is_note()).count(),
        });

        serde_json::to_string_pretty(&output).unwrap()
    }

    /// Generate error message + concrete fix suggestion for each error type
    fn error_with_fix(err: &CheckError) -> (String, String, &'static str) {
        match err {
            CheckError::PropertyContradiction { slot, a, b, .. } => (
                format!("{}", err),
                format!("Remove either [{a}] or [{b}] from memory slot '{slot}'. These properties are mutually exclusive."),
                "property_contradiction",
            ),
            CheckError::InvalidPropertyCombination { slot, reason, .. } => (
                format!("{}", err),
                format!("Fix the property combination on '{slot}': {reason}"),
                "invalid_properties",
            ),
            CheckError::MissingHandler { cell, signal, .. } => (
                format!("{}", err),
                format!("Add a handler to cell '{cell}':\n\n    on {signal}() {{\n        // TODO: implement\n        return map(\"status\", \"ok\")\n    }}"),
                "missing_handler",
            ),
            CheckError::ParamCountMismatch { cell, signal, expected, actual, .. } => (
                format!("{}", err),
                format!("Change the handler 'on {signal}(...)' in cell '{cell}' to accept {expected} parameter(s) (currently has {actual})."),
                "param_mismatch",
            ),
            CheckError::DuplicateCellName { name, .. } => (
                format!("{}", err),
                format!("Rename one of the duplicate cells named '{name}' to a unique name."),
                "duplicate_cell",
            ),
            CheckError::DuplicateSlot { cell, name, .. } => (
                format!("{}", err),
                format!("Remove the duplicate memory slot '{name}' in cell '{cell}'."),
                "duplicate_slot",
            ),
            CheckError::DuplicateSignal { cell, name, .. } => (
                format!("{}", err),
                format!("Remove the duplicate handler 'on {name}()' in cell '{cell}'."),
                "duplicate_signal",
            ),
            CheckError::ScaleShardNotFound { cell, slot, .. } => (
                format!("{}", err),
                format!("Either add a memory slot named '{slot}' to cell '{cell}', or change the shard target in the scale section to match an existing slot."),
                "shard_not_found",
            ),
            CheckError::ScaleConsistencyMismatch { slot, prop, consistency, .. } => (
                format!("{}", err),
                format!("Memory slot '{slot}' uses [{prop}] but scale declares consistency: {consistency}. Change either the memory property or the scale consistency level."),
                "consistency_mismatch",
            ),
            CheckError::PromiseViolation { cell, promise, .. } => (
                format!("{}", err),
                format!("Cell '{cell}' violates promise '{promise}'. Either satisfy the constraint or remove the promise from the face section."),
                "promise_violation",
            ),
            CheckError::InterpolationUndefined { .. } => (
                format!("{}", err),
                "Bind the variable before this string is evaluated, fix the spelling, or escape the braces as '{{...}}' if the text is literal.".to_string(),
                "interpolation_undefined",
            ),
            CheckError::DispatchIssue { .. } => (
                format!("{}", err),
                "Define the missing handler, fix the spelling, or rename one of the colliding handlers so the call resolves to exactly one cell.".to_string(),
                "dispatch",
            ),
            CheckError::Static { kind, message, .. } => {
                // the fix is the clause after the dash, when the message has one
                let fix = message.split(" — ").nth(1).map(|f| f.to_string())
                    .unwrap_or_else(|| message.clone());
                (message.clone(), fix, kind)
            }
            other => {
                // every remaining variant: a stable snake_case kind from its
                // name; the message names its own fix
                let name = format!("{:?}", other);
                let variant = name.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("error");
                let snake = variant.chars().fold(String::new(), |mut acc, c| {
                    if c.is_uppercase() && !acc.is_empty() { acc.push('_'); }
                    acc.push(c.to_ascii_lowercase()); acc
                });
                (format!("{}", err), format!("{}", err), Box::leak(snake.into_boxed_str()))
            }
        }
    }

    /// Generate warning message + suggestion
    fn warning_with_fix(warn: &CheckWarning) -> (String, String) {
        match warn {
            CheckWarning::UnhandledSignal { cell, signal, .. } => (
                format!("{}", warn),
                format!("Add a handler 'on {signal}(...)' to cell '{cell}', or remove the emit if it's unused."),
            ),
            CheckWarning::UnknownProperty { slot, property, .. } => (
                format!("{}", warn),
                format!("Check spelling of property '{property}' on slot '{slot}'. Define it with 'cell property {property} {{ }}' or remove it."),
            ),
            CheckWarning::UnverifiablePromise { promise, .. } => (
                format!("{}", warn),
                format!("Replace the descriptive promise \"{promise}\" with a machine-verifiable constraint, or accept this as documentation."),
            ),
            CheckWarning::DispatchShadow { .. } => (
                format!("{}", warn),
                "Rename one of the colliding handlers so the bare-name call is unambiguous.".to_string(),
            ),
            CheckWarning::AgentMissingStateMachine { cell, .. } => (
                format!("{}", warn),
                format!("Add a state machine to agent cell '{}' to enable verified behavior:\n\n    state workflow {{\n        initial: idle\n        idle -> active\n        active -> done\n        * -> failed\n    }}", cell),
            ),
            _ => (
                format!("{}", warn),
                "Review and address the reported warning.".to_string(),
            ),
        }
    }
}
