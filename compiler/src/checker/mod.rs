mod properties;
mod signals;
pub mod verify;
pub mod temporal;
pub mod native;

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
                write!(f, "warning: promise on '{cell}' is descriptive only, not machine-verifiable: \"{promise}\"")
            }
            Self::AwaitWithoutHandler { cell, signal, .. } => {
                write!(f, "warning: cell '{cell}' declares await '{signal}' but has no handler for it (will it be delivered via bus?)")
            }
            Self::ScaleEventualConsistency { cell, slot, .. } => {
                write!(f, "warning: cell '{cell}' uses eventual consistency on shard '{slot}' — reads after writes may return stale data")
            }
        }
    }
}

/// Top-level checker that runs all verification passes.
/// Uses the Registry for data-driven property checking.
pub struct Checker<'a> {
    pub registry: &'a Registry,
    pub errors: Vec<CheckError>,
    pub warnings: Vec<CheckWarning>,
}

impl<'a> Checker<'a> {
    pub fn new(registry: &'a Registry) -> Self {
        Self {
            registry,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn check(&mut self, program: &Program) {
        for cell in &program.cells {
            // Skip meta-cells (they define the language, not the program)
            if cell.node.kind != CellKind::Cell {
                continue;
            }
            self.check_cell(&cell.node);
        }
    }

    fn check_cell(&mut self, cell: &CellDef) {
        // 1. Structural checks
        self.check_structure(cell);

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

        // 5. Verify face contracts: signals have handlers, param counts match
        self.check_face_contracts(cell);

        // 6. Verify structural promises
        self.check_promises(cell);

        // 7. Run custom checkers from registry
        self.run_custom_checkers(cell);

        // 8. Verify scale section
        self.check_scale(cell);
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
        match predicate {
            "all_persistent" => self.all_slots_have_property(cell, "persistent"),
            "all_encrypted" => self.all_slots_have_property(cell, "encrypted"),
            "all_consistent" => self.all_slots_have_property(cell, "consistent"),
            "has_memory" => cell.sections.iter().any(|s| matches!(s.node, Section::Memory(_))),
            "has_face" => cell.sections.iter().any(|s| matches!(s.node, Section::Face(_))),
            "has_signals" => self.cell_has_signals(cell),
            "has_auth" => self.cell_has_given(cell, "auth") || self.cell_has_given(cell, "token"),
            _ => true, // Unknown predicates pass (permissive)
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

    fn run_custom_checkers(&mut self, cell: &CellDef) {
        for checker_def in &self.registry.checkers {
            // For each checker, evaluate its check body against this cell
            // For now, we support a simple pattern: check statements that
            // look at the cell's face for specific patterns
            for stmt in &checker_def.check_body {
                if let Statement::Require { constraint, else_signal } = &stmt.node {
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
            Constraint::Predicate { name, .. } => {
                match name.as_str() {
                    // Built-in checker predicates
                    "has_auth" => self.cell_has_given(cell, "auth") || self.cell_has_given(cell, "token"),
                    "has_face" => cell.sections.iter().any(|s| matches!(s.node, Section::Face(_))),
                    "has_memory" => cell.sections.iter().any(|s| matches!(s.node, Section::Memory(_))),
                    "has_signals" => self.cell_has_signals(cell),
                    _ => true, // Unknown predicates pass (permissive)
                }
            }
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
        }

        for error in &self.errors {
            output.push_str(&format!("error: {}\n", error));
        }

        if self.errors.is_empty() && self.warnings.is_empty() {
            output.push_str("✓ All checks passed.\n");
        } else if self.errors.is_empty() {
            output.push_str(&format!(
                "✓ {} warning(s), no errors.\n",
                self.warnings.len()
            ));
        } else {
            output.push_str(&format!(
                "✗ {} error(s), {} warning(s).\n",
                self.errors.len(),
                self.warnings.len()
            ));
        }

        output
    }
}
