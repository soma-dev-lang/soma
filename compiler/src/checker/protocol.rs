//! V1.6: protocol verification.
//!
//! For each top-level `protocol Foo { ... }`, verify that every
//! `from -> to: msg(args)` step names a real cell bound by `roles` and
//! that the target cell has a handler matching `msg(args)`. The check
//! is structural: think of it as session types' linearity property,
//! restricted to single-shot request/reply pairs.

use crate::ast::*;

#[derive(Debug)]
pub enum ProtocolFinding {
    Ok { protocol: String, steps: usize },
    UnknownRole { protocol: String, role: String },
    MissingHandler { protocol: String, role: String, cell: String, msg: String },
    ParamCountMismatch { protocol: String, msg: String, expected: usize, actual: usize },
}

impl std::fmt::Display for ProtocolFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolFinding::Ok { protocol, steps } =>
                write!(f, "protocol '{}': {} steps all match", protocol, steps),
            ProtocolFinding::UnknownRole { protocol, role } =>
                write!(f, "protocol '{}': role '{}' is not declared in `roles:`", protocol, role),
            ProtocolFinding::MissingHandler { protocol, role, cell, msg } =>
                write!(f, "protocol '{}': role '{}' (cell '{}') has no handler for '{}'",
                       protocol, role, cell, msg),
            ProtocolFinding::ParamCountMismatch { protocol, msg, expected, actual } =>
                write!(f, "protocol '{}': message '{}' declares {} arg(s), handler expects {}",
                       protocol, msg, actual, expected),
        }
    }
}

pub fn check_program(program: &Program) -> Vec<ProtocolFinding> {
    let mut findings = Vec::new();
    for p in &program.protocols {
        let proto = &p.node;
        let roles_map: std::collections::HashMap<&str, &str> = proto.roles.iter()
            .map(|(role, cell)| (role.as_str(), cell.as_str()))
            .collect();

        let mut ok_steps = 0;
        let mut step_failed = false;

        for step in &proto.steps {
            // Verify both roles exist.
            for r in [&step.from, &step.to] {
                if !roles_map.contains_key(r.as_str()) {
                    findings.push(ProtocolFinding::UnknownRole {
                        protocol: proto.name.clone(), role: r.clone(),
                    });
                    step_failed = true;
                }
            }
            if step_failed { continue; }

            // Find the target cell and verify it has a handler for the message.
            let cell_name = roles_map[step.to.as_str()];
            let target_cell = program.cells.iter().find(|c| c.node.name == cell_name);
            let target_cell = match target_cell {
                Some(c) => c,
                None => {
                    findings.push(ProtocolFinding::MissingHandler {
                        protocol: proto.name.clone(),
                        role: step.to.clone(),
                        cell: cell_name.to_string(),
                        msg: step.message.clone(),
                    });
                    step_failed = true;
                    continue;
                }
            };
            // Check the cell has an `on <msg>` handler with matching arity.
            let mut handler = None;
            for section in &target_cell.node.sections {
                if let Section::OnSignal(ref h) = section.node {
                    if h.signal_name == step.message {
                        handler = Some(h);
                        break;
                    }
                }
            }
            match handler {
                None => {
                    findings.push(ProtocolFinding::MissingHandler {
                        protocol: proto.name.clone(),
                        role: step.to.clone(),
                        cell: cell_name.to_string(),
                        msg: step.message.clone(),
                    });
                    step_failed = true;
                }
                Some(h) => {
                    if h.params.len() != step.args.len() {
                        findings.push(ProtocolFinding::ParamCountMismatch {
                            protocol: proto.name.clone(),
                            msg: step.message.clone(),
                            expected: h.params.len(),
                            actual: step.args.len(),
                        });
                        step_failed = true;
                    } else {
                        ok_steps += 1;
                    }
                }
            }
        }
        if !step_failed {
            findings.push(ProtocolFinding::Ok {
                protocol: proto.name.clone(),
                steps: ok_steps,
            });
        }
    }
    findings
}
