//! V1: declarative-threat-model checker.
//!
//! An `adversary` block names a class of failures (drop, reorder, delay,
//! partition, ...). A `scale { survives: net ∧ llm }` clause asserts that
//! all the verifier's safety/liveness claims hold *under* those adversaries.
//!
//! The checker's job at compile time is just to make sure every name in
//! `survives:` actually points at a declared adversary in scope, and to
//! propagate the model into VerifyResult so the verifier can stamp every
//! pass message with the qualifier.

use crate::ast::*;
use super::CheckError;

pub struct AdversaryChecker {
    pub errors: Vec<CheckError>,
}

impl AdversaryChecker {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    /// Check `survives:` clauses against in-scope adversaries.
    pub fn check_scope(
        &mut self,
        adversaries: &[&AdversarySection],
        cells: &[(&CellDef, Span)],
    ) {
        let names: Vec<&str> = adversaries.iter().map(|a| a.name.as_str()).collect();
        for (cell, span) in cells {
            for section in &cell.sections {
                if let Section::Scale(ref sc) = section.node {
                    for s in &sc.survives {
                        if !names.iter().any(|n| n.eq_ignore_ascii_case(s)) {
                            self.errors.push(CheckError::AdversaryUndeclared {
                                cell: cell.name.clone(),
                                name: s.clone(),
                                span: *span,
                            });
                        }
                    }
                }
            }
        }
    }
}

/// Format an adversary clause for inclusion in a verifier message.
/// `["network", "llm"]` -> "under network ∧ llm".
pub fn format_survives(names: &[String]) -> String {
    if names.is_empty() { return String::new(); }
    format!("under {}", names.join(" ∧ "))
}
