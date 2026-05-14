//! Sum-types exhaustiveness checker.
//!
//! Walks every `match` expression in the program.  When all arms are
//! variant patterns of the same sum type — or all arms reference
//! variants belonging to a single declared type — the checker
//! verifies the set of covered variants equals the full set declared
//! for that type, unless a wildcard arm catches the rest.
//!
//! Limitations of this first pass:
//!   - We don't yet flow-track the subject's type; the check fires
//!     only when arms themselves disambiguate.
//!   - Or-patterns over variants are flattened.
//!   - Guards on arms are ignored for the purposes of coverage.

use crate::ast::*;
use std::collections::{HashMap, HashSet};

/// Diagnostics produced by the check.
#[derive(Debug, Clone)]
pub struct SumTypeIssue {
    pub kind: SumTypeIssueKind,
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SumTypeIssueKind {
    /// `match` doesn't cover every variant of the inferred sum type.
    NonExhaustive,
    /// A variant referenced in a pattern doesn't exist in any
    /// declared sum type.
    UnknownVariant,
    /// Two `cell type` declarations both define a variant with the
    /// same name — variant lookup becomes ambiguous.
    DuplicateVariant,
}

/// Lookup: variant name → type name + variant list of the parent type.
pub struct VariantRegistry {
    pub variant_to_type: HashMap<String, String>,
    pub type_to_variants: HashMap<String, Vec<String>>,
    pub duplicates: Vec<(String, Vec<String>)>, // (variant, types that defined it)
}

impl VariantRegistry {
    pub fn build(program: &Program) -> Self {
        let mut variant_to_type: HashMap<String, String> = HashMap::new();
        let mut type_to_variants: HashMap<String, Vec<String>> = HashMap::new();
        let mut collisions: HashMap<String, Vec<String>> = HashMap::new();
        for cell in &program.cells {
            if !matches!(cell.node.kind, CellKind::Type) {
                continue;
            }
            for section in &cell.node.sections {
                if let Section::Variants(ref vs) = section.node {
                    let mut names = Vec::with_capacity(vs.variants.len());
                    for vd in &vs.variants {
                        names.push(vd.node.name.clone());
                        if let Some(existing) = variant_to_type.get(&vd.node.name) {
                            collisions
                                .entry(vd.node.name.clone())
                                .or_insert_with(|| vec![existing.clone()])
                                .push(cell.node.name.clone());
                        } else {
                            variant_to_type.insert(vd.node.name.clone(), cell.node.name.clone());
                        }
                    }
                    type_to_variants.insert(cell.node.name.clone(), names);
                }
            }
        }
        let duplicates = collisions
            .into_iter()
            .map(|(v, types)| (v, types))
            .collect();
        Self { variant_to_type, type_to_variants, duplicates }
    }
}

pub fn check_program(program: &Program) -> Vec<SumTypeIssue> {
    let mut issues = Vec::new();
    let registry = VariantRegistry::build(program);

    // Duplicate variant names across types.
    for (variant, types) in &registry.duplicates {
        issues.push(SumTypeIssue {
            kind: SumTypeIssueKind::DuplicateVariant,
            span: Span::new(0, 0),
            message: format!(
                "variant '{}' is declared in multiple sum types: {}",
                variant,
                types.join(", ")
            ),
        });
    }

    // Walk every handler body looking for match expressions.
    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.node.sections {
            if let Section::OnSignal(ref on) = section.node {
                for stmt in &on.body {
                    walk_stmt(&stmt.node, &registry, &mut issues);
                }
            }
        }
    }

    // Typed state-machine refinement: for `state X: T { … }`, every
    // state name appearing in the block must be a variant of `T`.
    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) {
            continue;
        }
        for section in &cell.node.sections {
            if let Section::State(ref sm) = section.node {
                if let Some(state_type) = &sm.state_type {
                    let variants = match registry.type_to_variants.get(state_type) {
                        Some(v) => v,
                        None => {
                            issues.push(SumTypeIssue {
                                kind: SumTypeIssueKind::UnknownVariant,
                                span: section.span,
                                message: format!(
                                    "state machine '{}' annotated with unknown type '{}'",
                                    sm.name, state_type
                                ),
                            });
                            continue;
                        }
                    };
                    // Check initial.
                    if !sm.initial.is_empty() && !variants.contains(&sm.initial) {
                        issues.push(SumTypeIssue {
                            kind: SumTypeIssueKind::UnknownVariant,
                            span: section.span,
                            message: format!(
                                "state '{}' is not a variant of '{}' (declared variants: {})",
                                sm.initial,
                                state_type,
                                variants.join(", ")
                            ),
                        });
                    }
                    // Check every transition's source and target.
                    for t in &sm.transitions {
                        if t.node.from != "*" && !variants.contains(&t.node.from) {
                            issues.push(SumTypeIssue {
                                kind: SumTypeIssueKind::UnknownVariant,
                                span: t.span,
                                message: format!(
                                    "transition source '{}' is not a variant of '{}'",
                                    t.node.from, state_type
                                ),
                            });
                        }
                        if !variants.contains(&t.node.to) {
                            issues.push(SumTypeIssue {
                                kind: SumTypeIssueKind::UnknownVariant,
                                span: t.span,
                                message: format!(
                                    "transition target '{}' is not a variant of '{}'",
                                    t.node.to, state_type
                                ),
                            });
                        }
                    }
                    // Check every transition() call in every handler.
                    for section in &cell.node.sections {
                        if let Section::OnSignal(ref on) = section.node {
                            for stmt in &on.body {
                                walk_typed_transitions(
                                    &stmt.node,
                                    state_type,
                                    variants,
                                    &on.signal_name,
                                    &mut issues,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    issues
}

fn walk_typed_transitions(
    stmt: &Statement,
    state_type: &str,
    variants: &[String],
    handler: &str,
    issues: &mut Vec<SumTypeIssue>,
) {
    match stmt {
        Statement::Let { value, .. } | Statement::Assign { value, .. } | Statement::Return { value } => {
            walk_typed_transitions_expr(&value.node, value.span, state_type, variants, handler, issues);
        }
        Statement::ExprStmt { expr } => {
            walk_typed_transitions_expr(&expr.node, expr.span, state_type, variants, handler, issues);
        }
        Statement::Ensure { condition } => {
            walk_typed_transitions_expr(&condition.node, condition.span, state_type, variants, handler, issues);
        }
        Statement::If { condition, then_body, else_body } => {
            walk_typed_transitions_expr(&condition.node, condition.span, state_type, variants, handler, issues);
            for s in then_body { walk_typed_transitions(&s.node, state_type, variants, handler, issues); }
            for s in else_body { walk_typed_transitions(&s.node, state_type, variants, handler, issues); }
        }
        Statement::For { iter, body, .. } => {
            walk_typed_transitions_expr(&iter.node, iter.span, state_type, variants, handler, issues);
            for s in body { walk_typed_transitions(&s.node, state_type, variants, handler, issues); }
        }
        Statement::While { condition, body, .. } => {
            walk_typed_transitions_expr(&condition.node, condition.span, state_type, variants, handler, issues);
            for s in body { walk_typed_transitions(&s.node, state_type, variants, handler, issues); }
        }
        _ => {}
    }
}

fn walk_typed_transitions_expr(
    expr: &Expr,
    span: Span,
    state_type: &str,
    variants: &[String],
    handler: &str,
    issues: &mut Vec<SumTypeIssue>,
) {
    match expr {
        Expr::FnCall { name, args } if name == "transition" => {
            if let Some(target_expr) = args.get(1) {
                let target_name = match &target_expr.node {
                    Expr::Ident(n) if n.chars().next().map_or(false, |c| c.is_ascii_uppercase()) => Some(n.clone()),
                    Expr::Literal(Literal::String(s)) => Some(s.clone()),
                    _ => None,
                };
                if let Some(name) = target_name {
                    if !variants.contains(&name) {
                        issues.push(SumTypeIssue {
                            kind: SumTypeIssueKind::UnknownVariant,
                            span,
                            message: format!(
                                "handler '{}' transitions to '{}', which is not a variant of '{}'",
                                handler, name, state_type
                            ),
                        });
                    }
                }
            }
            for a in args { walk_typed_transitions_expr(&a.node, a.span, state_type, variants, handler, issues); }
        }
        Expr::FnCall { args, .. } => {
            for a in args { walk_typed_transitions_expr(&a.node, a.span, state_type, variants, handler, issues); }
        }
        Expr::MethodCall { target, args, .. } => {
            walk_typed_transitions_expr(&target.node, target.span, state_type, variants, handler, issues);
            for a in args { walk_typed_transitions_expr(&a.node, a.span, state_type, variants, handler, issues); }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            walk_typed_transitions_expr(&condition.node, condition.span, state_type, variants, handler, issues);
            for s in then_body { walk_typed_transitions(&s.node, state_type, variants, handler, issues); }
            walk_typed_transitions_expr(&then_result.node, then_result.span, state_type, variants, handler, issues);
            for s in else_body { walk_typed_transitions(&s.node, state_type, variants, handler, issues); }
            walk_typed_transitions_expr(&else_result.node, else_result.span, state_type, variants, handler, issues);
        }
        Expr::Match { subject, arms } => {
            walk_typed_transitions_expr(&subject.node, subject.span, state_type, variants, handler, issues);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    walk_typed_transitions_expr(&g.node, g.span, state_type, variants, handler, issues);
                }
                for s in &arm.body { walk_typed_transitions(&s.node, state_type, variants, handler, issues); }
                walk_typed_transitions_expr(&arm.result.node, arm.result.span, state_type, variants, handler, issues);
            }
        }
        _ => {}
    }
}

fn walk_stmt(stmt: &Statement, reg: &VariantRegistry, issues: &mut Vec<SumTypeIssue>) {
    match stmt {
        Statement::Let { value, .. } => walk_expr(&value.node, value.span, reg, issues),
        Statement::Assign { value, .. } => walk_expr(&value.node, value.span, reg, issues),
        Statement::ExprStmt { expr } => walk_expr(&expr.node, expr.span, reg, issues),
        Statement::Return { value } => walk_expr(&value.node, value.span, reg, issues),
        Statement::Ensure { condition } => walk_expr(&condition.node, condition.span, reg, issues),
        Statement::If { condition, then_body, else_body } => {
            walk_expr(&condition.node, condition.span, reg, issues);
            for s in then_body { walk_stmt(&s.node, reg, issues); }
            for s in else_body { walk_stmt(&s.node, reg, issues); }
        }
        Statement::For { iter, body, .. } => {
            walk_expr(&iter.node, iter.span, reg, issues);
            for s in body { walk_stmt(&s.node, reg, issues); }
        }
        Statement::While { condition, body, .. } => {
            walk_expr(&condition.node, condition.span, reg, issues);
            for s in body { walk_stmt(&s.node, reg, issues); }
        }
        _ => {}
    }
}

fn walk_expr(expr: &Expr, span: Span, reg: &VariantRegistry, issues: &mut Vec<SumTypeIssue>) {
    match expr {
        Expr::Match { subject, arms } => {
            walk_expr(&subject.node, subject.span, reg, issues);
            check_match_exhaustiveness(arms, span, reg, issues);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    walk_expr(&g.node, g.span, reg, issues);
                }
                for s in &arm.body {
                    walk_stmt(&s.node, reg, issues);
                }
                walk_expr(&arm.result.node, arm.result.span, reg, issues);
            }
        }
        Expr::IfExpr { condition, then_body, then_result, else_body, else_result } => {
            walk_expr(&condition.node, condition.span, reg, issues);
            for s in then_body { walk_stmt(&s.node, reg, issues); }
            walk_expr(&then_result.node, then_result.span, reg, issues);
            for s in else_body { walk_stmt(&s.node, reg, issues); }
            walk_expr(&else_result.node, else_result.span, reg, issues);
        }
        Expr::BinaryOp { left, right, .. } | Expr::CmpOp { left, right, .. } | Expr::Pipe { left, right } => {
            walk_expr(&left.node, left.span, reg, issues);
            walk_expr(&right.node, right.span, reg, issues);
        }
        Expr::Not(inner) | Expr::Try(inner) | Expr::TryPropagate(inner) => {
            walk_expr(&inner.node, inner.span, reg, issues);
        }
        Expr::FnCall { args, .. } => {
            for a in args { walk_expr(&a.node, a.span, reg, issues); }
        }
        Expr::MethodCall { target, args, .. } => {
            walk_expr(&target.node, target.span, reg, issues);
            for a in args { walk_expr(&a.node, a.span, reg, issues); }
        }
        Expr::Record { fields, .. } => {
            for (_, v) in fields { walk_expr(&v.node, v.span, reg, issues); }
        }
        _ => {}
    }
}

fn check_match_exhaustiveness(
    arms: &[MatchArm],
    span: Span,
    reg: &VariantRegistry,
    issues: &mut Vec<SumTypeIssue>,
) {
    // Determine if every arm is a variant pattern of the same type.
    // A wildcard / variable / map-destructure arm catches "anything" and
    // makes the match trivially exhaustive.
    let mut covered: HashSet<String> = HashSet::new();
    let mut inferred_type: Option<String> = None;
    let mut has_wildcard = false;

    for arm in arms {
        if arm.guard.is_some() {
            // A guard might fail, so this arm doesn't actually exhaust
            // its pattern's variant on its own.  Treat it as a catch-all
            // for the exhaustiveness check (the value can still flow
            // through).
            has_wildcard = true;
            continue;
        }
        if !collect_variants(&arm.pattern, reg, &mut covered, &mut inferred_type) {
            // Arm doesn't have a variant pattern → wildcard for our purposes.
            has_wildcard = true;
        }
    }

    if has_wildcard { return; }
    let type_name = match inferred_type {
        Some(t) => t,
        None => return,  // no arm gave us a type to check against
    };
    let all_variants = match reg.type_to_variants.get(&type_name) {
        Some(v) => v,
        None => return,
    };
    let missing: Vec<&String> = all_variants
        .iter()
        .filter(|v| !covered.contains(v.as_str()))
        .collect();
    if !missing.is_empty() {
        let names: Vec<String> = missing.iter().map(|s| s.to_string()).collect();
        issues.push(SumTypeIssue {
            kind: SumTypeIssueKind::NonExhaustive,
            span,
            message: format!(
                "non-exhaustive match on '{}': missing variant{} {}",
                type_name,
                if names.len() == 1 { "" } else { "s" },
                names.iter().map(|n| format!("`{}`", n)).collect::<Vec<_>>().join(", ")
            ),
        });
    }
}

/// Walk a pattern and accumulate the variant names it covers.
/// Returns `true` if the pattern is a variant pattern (i.e., contributes
/// to coverage), `false` if it's a catch-all (wildcard/variable/etc).
fn collect_variants(
    pat: &MatchPattern,
    reg: &VariantRegistry,
    covered: &mut HashSet<String>,
    inferred: &mut Option<String>,
) -> bool {
    match pat {
        MatchPattern::Variant { type_name, name, .. } => {
            // Resolve type either from explicit qualifier or registry lookup.
            let resolved = type_name
                .clone()
                .or_else(|| reg.variant_to_type.get(name).cloned());
            match resolved {
                Some(t) => {
                    match inferred {
                        None => { *inferred = Some(t.clone()); }
                        Some(existing) => {
                            if existing != &t {
                                // Two different types in one match — skip exhaustiveness.
                                *inferred = None;
                            }
                        }
                    }
                    covered.insert(name.clone());
                    true
                }
                None => true, // unknown variant; report elsewhere
            }
        }
        MatchPattern::Or(alts) => {
            let mut any = false;
            for a in alts {
                if collect_variants(a, reg, covered, inferred) { any = true; }
            }
            any
        }
        _ => false,
    }
}
