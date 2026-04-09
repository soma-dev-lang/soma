//! V1: Session-typed signal protocol checker.
//!
//! A `protocol` block declares a linearised script of role-to-role messages.
//! For every step `from -> to : Msg(args)`, the cell whose name matches `to`
//! (case-insensitive) must have a handler `on Msg(args)`. If any step is
//! missing a handler, we reject the program *at compile time* — this is
//! deadlock-by-construction, not deadlock-by-model-checking.
//!
//! Loop and Choice steps recursively expand into their inner steps for
//! the exhaustiveness check.
//!
//! V1 limitations (documented in docs/SEMANTICS.md):
//!   - We check exhaustiveness, not ordering. The ordering check requires
//!     a real session-type unification with Loop/Choice, deferred to V1.1.
//!   - Roles match cell names case-insensitively. Explicit `uses P as role`
//!     syntax is also deferred to V1.1.

use crate::ast::*;
use super::CheckError;

pub struct ProtocolChecker {
    pub errors: Vec<CheckError>,
}

impl ProtocolChecker {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    /// Check protocols at the same scope as the cells that play their roles.
    /// `cells` is the set of sibling cells (e.g. interior children, or top-level).
    pub fn check_scope(&mut self, protocols: &[(&ProtocolSection, Span)], cells: &[&CellDef]) {
        for (proto, span) in protocols {
            self.check_protocol(proto, *span, cells);
        }
    }

    fn check_protocol(&mut self, proto: &ProtocolSection, span: Span, cells: &[&CellDef]) {
        // 1. Every role must be the name of some cell in scope.
        for role in &proto.roles {
            if !cells.iter().any(|c| c.name.eq_ignore_ascii_case(role)) {
                self.errors.push(CheckError::ProtocolRoleMissingCell {
                    protocol: proto.name.clone(),
                    role: role.clone(),
                    span,
                });
            }
        }
        // 2. Flatten loop/choice into a list of (to, msg, arity) triples
        //    and check each receiver has a matching handler.
        let mut steps: Vec<(String, String, usize, Span)> = Vec::new();
        Self::flatten(&proto.steps, &mut steps);

        for (to, msg, arity, step_span) in &steps {
            let cell = cells.iter().find(|c| c.name.eq_ignore_ascii_case(to));
            let Some(cell) = cell else { continue }; // role error already reported
            let handler = cell.sections.iter().find_map(|s| {
                if let Section::OnSignal(ref on) = s.node {
                    if on.signal_name == *msg { Some(on) } else { None }
                } else { None }
            });
            match handler {
                None => self.errors.push(CheckError::ProtocolStepNotHandled {
                    protocol: proto.name.clone(),
                    role: to.clone(),
                    message: msg.clone(),
                    span: *step_span,
                }),
                Some(on) if on.params.len() != *arity => {
                    self.errors.push(CheckError::ProtocolArityMismatch {
                        protocol: proto.name.clone(),
                        role: to.clone(),
                        message: msg.clone(),
                        expected: *arity,
                        actual: on.params.len(),
                        span: *step_span,
                    });
                }
                Some(_) => {}
            }
        }
    }

    fn flatten(steps: &[Spanned<ProtocolStep>], out: &mut Vec<(String, String, usize, Span)>) {
        for s in steps {
            match &s.node {
                ProtocolStep::Send { to, message, params, .. } => {
                    out.push((to.clone(), message.clone(), params.len(), s.span));
                }
                ProtocolStep::Loop(body) => Self::flatten(body, out),
                ProtocolStep::Choice(branches) => {
                    for b in branches { Self::flatten(b, out); }
                }
            }
        }
    }
}
