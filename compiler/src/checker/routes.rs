//! The routes a cell's `request` handler spells out.
//!
//! `soma serve` exposes every public handler of the request-owning cell at
//! `/<handler>/<args>` AND calls `request(method, path, body)` for anything
//! else. A domain handler named after one of `request`'s own routes used to
//! shadow that route: with `on hold(id, qty)` and a route `"/hold/" + id`,
//! `POST /hold/r1` went to the handler with the wrong arguments (HTTP 500)
//! while check, verify, test and `soma run … request` were all green.
//!
//! Rule: a path that `request` matches explicitly goes to `request`. This
//! module extracts those paths; `soma serve` applies the rule and `soma
//! check` reports the collision.

use crate::ast::*;

#[derive(Debug, Clone, Default)]
pub struct ExplicitRoutes {
    pub exact: Vec<String>,
    pub prefixes: Vec<String>,
}

impl ExplicitRoutes {
    pub fn matches(&self, path: &str) -> bool {
        self.exact.iter().any(|p| p == path) || self.prefixes.iter().any(|p| path.starts_with(p.as_str()))
    }

    /// First path segment of every explicit route ("/hold/" → "hold").
    pub fn first_segments(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .exact
            .iter()
            .chain(self.prefixes.iter())
            .filter_map(|p| p.trim_start_matches('/').split('/').next().map(str::to_string))
            .filter(|s| !s.is_empty())
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

pub fn explicit_routes(cell: &CellDef) -> ExplicitRoutes {
    let mut routes = ExplicitRoutes::default();
    for section in &cell.sections {
        let Section::OnSignal(on) = &section.node else { continue };
        if on.signal_name != "request" {
            continue;
        }
        for stmt in &on.body {
            super::termination::walk_stmt(&stmt.node, &mut |e| {
                if let Expr::Match { arms, .. } = e {
                    for arm in arms {
                        collect(&arm.pattern, false, &mut routes);
                    }
                }
            });
        }
    }
    routes
}

/// `in_path`: are we looking at the pattern bound to the `path` field (or at
/// a bare pattern matched against the path)?
fn collect(pattern: &MatchPattern, in_path: bool, out: &mut ExplicitRoutes) {
    match pattern {
        MatchPattern::MapDestructure(fields) => {
            for (name, p) in fields {
                if name == "path" {
                    collect(p, true, out);
                }
            }
        }
        MatchPattern::Or(alts) => {
            for a in alts {
                collect(a, in_path, out);
            }
        }
        MatchPattern::Literal(Literal::String(s)) if s.starts_with('/') && (in_path || s.len() > 1) => {
            out.exact.push(s.clone());
        }
        MatchPattern::StringPrefix { prefix, .. } if prefix.starts_with('/') => {
            out.prefixes.push(prefix.clone());
        }
        _ => {}
    }
}

/// Collisions between `request`'s explicit routes and public handler names.
pub fn check_program(program: &Program) -> Vec<(String, Span)> {
    let mut out = Vec::new();
    for cell in super::names::collect_cells(program) {
        let routes = explicit_routes(cell);
        let segments = routes.first_segments();
        if segments.is_empty() {
            continue;
        }
        for section in &cell.sections {
            let Section::OnSignal(on) = &section.node else { continue };
            let name = &on.signal_name;
            if name == "request" || name.starts_with('_') || !segments.contains(name) {
                continue;
            }
            // `{path: "/add/" + id} -> add(id)` is the documented shape: the
            // route delegates to the handler of the same name — no warning
            if request_calls(cell, name) {
                continue;
            }
            out.push((
                format!(
                    "handler `{name}` and a route of `request` share the path /{name}: `soma serve` also exposes every \
                     public handler of this cell at /{name}/<args>. The route written in `request` wins for the paths \
                     it matches, but any other /{name}/… still calls the handler directly. Prefix handlers that are \
                     not meant to be HTTP endpoints with `_` (`on _{name}(…)`)"
                ),
                section.span,
            ));
        }
    }
    out
}

/// Does the cell's `request` handler call `name` anywhere in its body?
fn request_calls(cell: &CellDef, name: &str) -> bool {
    let Some(req) = cell.sections.iter().find_map(|s| match &s.node {
        Section::OnSignal(on) if on.signal_name == "request" => Some(on),
        _ => None,
    }) else { return false };
    let mut found = false;
    super::literals::for_each_call(&req.body, &mut |called, _, _| { if called == name { found = true; } });
    found
}
