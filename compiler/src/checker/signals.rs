use crate::ast::*;
use super::{CheckError, CheckWarning};
use std::collections::HashMap;

/// Checks signal wiring between sibling cells:
/// - Every `await` has a matching `signal` from a sibling
/// - Every `on` handler has a matching `signal` from a sibling
/// - Warns on signals with no handler
pub struct SignalChecker {
    pub errors: Vec<CheckError>,
    pub warnings: Vec<CheckWarning>,
}

#[derive(Debug)]
struct SignalInfo {
    cell_name: String,
    params: Vec<Param>,
    return_type: Option<Spanned<TypeExpr>>,
    span: Span,
}

#[derive(Debug)]
struct AwaitInfo {
    cell_name: String,
    params: Vec<Param>,
    span: Span,
}

#[derive(Debug)]
struct HandlerInfo {
    cell_name: String,
    params: Vec<Param>,
    span: Span,
}

impl SignalChecker {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Check that signals, awaits, and handlers among sibling cells are properly matched.
    pub fn check_siblings(&mut self, cells: &[Spanned<CellDef>]) {
        // Collect all signals, awaits, and handlers from sibling cells
        let mut emitted: HashMap<String, Vec<SignalInfo>> = HashMap::new();
        let mut awaited: HashMap<String, Vec<AwaitInfo>> = HashMap::new();
        let mut handled: HashMap<String, Vec<HandlerInfo>> = HashMap::new();

        // Check for duplicate cell names
        let mut cell_names: HashMap<String, Span> = HashMap::new();
        for cell in cells {
            if let Some(prev_span) = cell_names.get(&cell.node.name) {
                self.errors.push(CheckError::DuplicateCellName {
                    name: cell.node.name.clone(),
                    span: cell.span,
                });
            } else {
                cell_names.insert(cell.node.name.clone(), cell.span);
            }
        }

        for cell in cells {
            let cell_name = &cell.node.name;

            for section in &cell.node.sections {
                match &section.node {
                    Section::Face(face) => {
                        for decl in &face.declarations {
                            match &decl.node {
                                FaceDecl::Signal(sig) => {
                                    emitted
                                        .entry(sig.name.clone())
                                        .or_default()
                                        .push(SignalInfo {
                                            cell_name: cell_name.clone(),
                                            params: sig.params.clone(),
                                            return_type: sig.return_type.clone(),
                                            span: decl.span,
                                        });
                                }
                                FaceDecl::Await(aw) => {
                                    awaited
                                        .entry(aw.name.clone())
                                        .or_default()
                                        .push(AwaitInfo {
                                            cell_name: cell_name.clone(),
                                            params: aw.params.clone(),
                                            span: decl.span,
                                        });
                                }
                                _ => {}
                            }
                        }
                    }
                    Section::OnSignal(on) => {
                        handled
                            .entry(on.signal_name.clone())
                            .or_default()
                            .push(HandlerInfo {
                                cell_name: cell_name.clone(),
                                params: on.params.clone(),
                                span: section.span,
                            });
                    }
                    _ => {}
                }
            }
        }

        // ── Check: every await has a matching signal ─────────────────
        for (signal_name, awaits) in &awaited {
            if !emitted.contains_key(signal_name) {
                for aw in awaits {
                    self.errors.push(CheckError::UnmatchedAwait {
                        cell: aw.cell_name.clone(),
                        signal: signal_name.clone(),
                        span: aw.span,
                    });
                }
            } else {
                // Check type compatibility
                let emitters = &emitted[signal_name];
                for aw in awaits {
                    for em in emitters {
                        // Don't match against self
                        if em.cell_name == aw.cell_name {
                            continue;
                        }
                        if !self.params_compatible(&em.params, &aw.params) {
                            self.errors.push(CheckError::SignalTypeMismatch {
                                signal: signal_name.clone(),
                                span: aw.span,
                            });
                        }
                    }
                }
            }
        }

        // ── Check: every handler has a matching signal ───────────────
        for (signal_name, handlers) in &handled {
            if !emitted.contains_key(signal_name) {
                for handler in handlers {
                    self.errors.push(CheckError::UnmatchedHandler {
                        cell: handler.cell_name.clone(),
                        signal: signal_name.clone(),
                        span: handler.span,
                    });
                }
            } else {
                let emitters = &emitted[signal_name];
                for handler in handlers {
                    for em in emitters {
                        if em.cell_name == handler.cell_name {
                            continue;
                        }
                        if !self.params_compatible(&em.params, &handler.params) {
                            self.errors.push(CheckError::SignalTypeMismatch {
                                signal: signal_name.clone(),
                                span: handler.span,
                            });
                        }
                    }
                }
            }
        }

        // ── Warn: signals with no handler and no awaiter ─────────────
        for (signal_name, emitters) in &emitted {
            let has_handler = handled.contains_key(signal_name);
            let has_awaiter = awaited.contains_key(signal_name);

            if !has_handler && !has_awaiter {
                for em in emitters {
                    self.warnings.push(CheckWarning::UnhandledSignal {
                        cell: em.cell_name.clone(),
                        signal: signal_name.clone(),
                        span: em.span,
                    });
                }
            }
        }
    }

    /// Check if two parameter lists are structurally compatible.
    /// For now, we check that they have the same number of params and
    /// that type names match. Full type checking would require a type environment.
    fn params_compatible(&self, emitter: &[Param], receiver: &[Param]) -> bool {
        if emitter.len() != receiver.len() {
            return false;
        }

        for (ep, rp) in emitter.iter().zip(receiver.iter()) {
            if !self.types_compatible(&ep.ty.node, &rp.ty.node) {
                return false;
            }
        }

        true
    }

    fn types_compatible(&self, a: &TypeExpr, b: &TypeExpr) -> bool {
        match (a, b) {
            (TypeExpr::Simple(a_name), TypeExpr::Simple(b_name)) => a_name == b_name,
            (
                TypeExpr::Generic {
                    name: a_name,
                    args: a_args,
                },
                TypeExpr::Generic {
                    name: b_name,
                    args: b_args,
                },
            ) => {
                a_name == b_name
                    && a_args.len() == b_args.len()
                    && a_args
                        .iter()
                        .zip(b_args.iter())
                        .all(|(a, b)| self.types_compatible(&a.node, &b.node))
            }
            (
                TypeExpr::CellRef {
                    cell: a_cell,
                    member: a_member,
                },
                TypeExpr::CellRef {
                    cell: b_cell,
                    member: b_member,
                },
            ) => a_cell == b_cell && a_member == b_member,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_signal_cell(name: &str, signals: Vec<&str>, awaits: Vec<&str>) -> Spanned<CellDef> {
        let mut decls: Vec<Spanned<FaceDecl>> = Vec::new();

        for sig in signals {
            decls.push(Spanned::new(
                FaceDecl::Signal(SignalDecl {
                    name: sig.to_string(),
                    params: vec![],
                    return_type: None,
                }),
                Span::new(0, 0),
            ));
        }

        for aw in awaits {
            decls.push(Spanned::new(
                FaceDecl::Await(AwaitDecl {
                    name: aw.to_string(),
                    params: vec![],
                    return_type: None,
                }),
                Span::new(0, 0),
            ));
        }

        Spanned::new(
            CellDef {
                kind: CellKind::Cell,
                name: name.to_string(),
                type_params: vec![],
                sections: vec![Spanned::new(
                    Section::Face(FaceSection { declarations: decls }),
                    Span::new(0, 0),
                )],
            },
            Span::new(0, 0),
        )
    }

    #[test]
    fn test_matched_signals() {
        let cells = vec![
            make_signal_cell("Producer", vec!["data_ready"], vec![]),
            make_signal_cell("Consumer", vec![], vec!["data_ready"]),
        ];
        let mut checker = SignalChecker::new();
        checker.check_siblings(&cells);
        assert_eq!(checker.errors.len(), 0);
    }

    #[test]
    fn test_unmatched_await() {
        let cells = vec![
            make_signal_cell("Consumer", vec![], vec!["data_ready"]),
            make_signal_cell("Other", vec!["other_signal"], vec![]),
        ];
        let mut checker = SignalChecker::new();
        checker.check_siblings(&cells);
        assert_eq!(checker.errors.len(), 1);
        assert!(matches!(
            checker.errors[0],
            CheckError::UnmatchedAwait { .. }
        ));
    }

    #[test]
    fn test_unhandled_signal_warning() {
        let cells = vec![
            make_signal_cell("Producer", vec!["data_ready"], vec![]),
            make_signal_cell("Other", vec![], vec![]),
        ];
        let mut checker = SignalChecker::new();
        checker.check_siblings(&cells);
        assert_eq!(checker.errors.len(), 0);
        assert_eq!(checker.warnings.len(), 1);
    }

    #[test]
    fn test_signal_wiring_complete() {
        // Producer emits data_ready, Consumer awaits data_ready
        // Consumer emits processed, Reporter awaits processed
        let cells = vec![
            make_signal_cell("Producer", vec!["data_ready"], vec![]),
            make_signal_cell("Consumer", vec!["processed"], vec!["data_ready"]),
            make_signal_cell("Reporter", vec![], vec!["processed"]),
        ];
        let mut checker = SignalChecker::new();
        checker.check_siblings(&cells);
        assert_eq!(checker.errors.len(), 0);
        assert_eq!(checker.warnings.len(), 0);
    }

    #[test]
    fn test_duplicate_cell_names() {
        let cells = vec![
            make_signal_cell("Worker", vec![], vec![]),
            make_signal_cell("Worker", vec![], vec![]),
        ];
        let mut checker = SignalChecker::new();
        checker.check_siblings(&cells);
        assert_eq!(checker.errors.len(), 1);
        assert!(matches!(
            checker.errors[0],
            CheckError::DuplicateCellName { .. }
        ));
    }
}
