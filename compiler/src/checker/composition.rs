//! Session-type-inspired composition checker.
//!
//! Proves that when cells communicate via signals inside an `interior`
//! block, every signal emitted has a matching handler and every handler
//! has a signal source. This is the duality property from session types,
//! applied to the Soma signal bus.
//!
//! ## What this checks
//!
//! For each `interior { ... }` block in a cell:
//!
//!   1. Every `emit Sig(args)` in any handler body has a matching
//!      `on Sig(...)` handler in some sibling cell (or in the parent's
//!      runtime section for downward signals).
//!   2. Every `on Sig(...)` handler in a child cell either:
//!      (a) has a matching `emit Sig(...)` from a sibling, or
//!      (b) is exposed in the parent's `face { signal Sig(...) }` for
//!          external delivery.
//!
//! ## Output
//!
//! - **Verified**: N signal pairs matched, 0 undelivered, 0 orphans
//! - **Undelivered**: cell A emits Sig but no sibling handles it
//! - **Orphan**: cell B handles Sig but no sibling emits it and the
//!   parent doesn't expose it
//!
//! ## Scope
//!
//! Per-interior-block. Signals between cells in *different* interior
//! blocks are not matched (they flow through different scopes).

use crate::ast::*;

#[derive(Debug, Clone)]
pub struct SignalPair {
    pub signal: String,
    pub emitter: String,
    pub handler: String,
}

#[derive(Debug, Clone)]
pub struct CompositionResult {
    pub pairs: Vec<SignalPair>,
    pub undelivered: Vec<(String, String)>,   // (emitter_cell, signal)
    pub orphans: Vec<(String, String)>,       // (handler_cell, signal)
}

/// Collect all signal names emitted by `emit Sig(...)` statements
/// inside a cell's handler bodies.
fn collect_emits(cell: &CellDef) -> Vec<String> {
    let mut emits = Vec::new();
    for section in &cell.sections {
        match &section.node {
            Section::OnSignal(on) => {
                collect_emits_from_stmts(&on.body, &mut emits);
            }
            Section::Every(every) => {
                collect_emits_from_stmts(&every.body, &mut emits);
            }
            Section::After(after) => {
                collect_emits_from_stmts(&after.body, &mut emits);
            }
            _ => {}
        }
    }
    emits.sort();
    emits.dedup();
    emits
}

fn collect_emits_from_stmts(stmts: &[Spanned<Statement>], out: &mut Vec<String>) {
    for stmt in stmts {
        match &stmt.node {
            Statement::Emit { signal_name, .. } => {
                out.push(signal_name.clone());
            }
            Statement::If { then_body, else_body, .. } => {
                collect_emits_from_stmts(then_body, out);
                collect_emits_from_stmts(else_body, out);
            }
            Statement::For { body, .. } | Statement::While { body, .. } => {
                collect_emits_from_stmts(body, out);
            }
            Statement::ExprStmt { expr } => {
                collect_emits_from_expr(&expr.node, out);
            }
            Statement::Let { value, .. } | Statement::Assign { value, .. }
            | Statement::Return { value } => {
                collect_emits_from_expr(&value.node, out);
            }
            _ => {}
        }
    }
}

fn collect_emits_from_expr(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Match { arms, subject } => {
            collect_emits_from_expr(&subject.node, out);
            for arm in arms {
                collect_emits_from_stmts(&arm.body, out);
                collect_emits_from_expr(&arm.result.node, out);
            }
        }
        Expr::IfExpr { then_body, else_body, then_result, else_result, .. } => {
            collect_emits_from_stmts(then_body, out);
            collect_emits_from_stmts(else_body, out);
            collect_emits_from_expr(&then_result.node, out);
            collect_emits_from_expr(&else_result.node, out);
        }
        Expr::Pipe { left, right } => {
            collect_emits_from_expr(&left.node, out);
            collect_emits_from_expr(&right.node, out);
        }
        Expr::LambdaBlock { stmts, result, .. } => {
            collect_emits_from_stmts(stmts, out);
            collect_emits_from_expr(&result.node, out);
        }
        _ => {}
    }
}

/// Collect all signal names handled by `on Sig(...)` handlers.
fn collect_handlers(cell: &CellDef) -> Vec<String> {
    let mut handlers = Vec::new();
    for section in &cell.sections {
        if let Section::OnSignal(on) = &section.node {
            if !on.signal_name.starts_with('_') {
                handlers.push(on.signal_name.clone());
            }
        }
    }
    handlers.sort();
    handlers.dedup();
    handlers
}

/// Collect signal names from a cell's face declarations.
fn collect_face_signals(cell: &CellDef) -> Vec<String> {
    let mut signals = Vec::new();
    for section in &cell.sections {
        if let Section::Face(face) = &section.node {
            for decl in &face.declarations {
                if let FaceDecl::Signal(sig) = &decl.node {
                    signals.push(sig.name.clone());
                }
            }
        }
    }
    signals
}

/// Check composition for one interior block.
pub fn check_composition(
    children: &[Spanned<CellDef>],
    parent: &CellDef,
) -> CompositionResult {
    // 1. For each child, collect what it emits and what it handles.
    let mut cell_emits: Vec<(String, Vec<String>)> = Vec::new();
    let mut cell_handles: Vec<(String, Vec<String>)> = Vec::new();

    for child in children {
        let name = child.node.name.clone();
        cell_emits.push((name.clone(), collect_emits(&child.node)));
        cell_handles.push((name, collect_handlers(&child.node)));
    }

    // Also collect emits from the parent's runtime section (downward signals).
    let mut parent_emits: Vec<String> = Vec::new();
    for section in &parent.sections {
        if let Section::Runtime(rt) = &section.node {
            for entry in &rt.entries {
                if let RuntimeEntry::Emit { signal_name, .. } = &entry.node {
                    parent_emits.push(signal_name.clone());
                }
            }
        }
    }

    // Parent face signals count as "externally delivered" to children.
    let parent_face_signals = collect_face_signals(parent);

    // 2. Build the all-emitters set (siblings + parent runtime).
    let mut all_emitted: Vec<(String, String)> = Vec::new(); // (cell, signal)
    for (cell, sigs) in &cell_emits {
        for sig in sigs {
            all_emitted.push((cell.clone(), sig.clone()));
        }
    }
    for sig in &parent_emits {
        all_emitted.push(("(parent)".to_string(), sig.clone()));
    }

    // 3. Build the all-handlers set.
    let mut all_handled: Vec<(String, String)> = Vec::new(); // (cell, signal)
    for (cell, sigs) in &cell_handles {
        for sig in sigs {
            all_handled.push((cell.clone(), sig.clone()));
        }
    }

    // 4. Match emits → handlers.
    let mut pairs = Vec::new();
    let mut undelivered = Vec::new();

    for (emitter, signal) in &all_emitted {
        let has_handler = all_handled.iter().any(|(_, s)| s == signal);
        if has_handler {
            let handler_cell = all_handled.iter()
                .find(|(_, s)| s == signal)
                .map(|(c, _)| c.clone())
                .unwrap_or_default();
            pairs.push(SignalPair {
                signal: signal.clone(),
                emitter: emitter.clone(),
                handler: handler_cell,
            });
        } else {
            undelivered.push((emitter.clone(), signal.clone()));
        }
    }

    // 5. Find orphan handlers (handled but never emitted by any sibling
    //    or parent, and not in the parent's face).
    let mut orphans = Vec::new();
    for (handler_cell, signal) in &all_handled {
        let has_emitter = all_emitted.iter().any(|(_, s)| s == signal);
        let in_parent_face = parent_face_signals.contains(signal);
        if !has_emitter && !in_parent_face {
            orphans.push((handler_cell.clone(), signal.clone()));
        }
    }

    // Deduplicate pairs (same signal may be emitted by multiple paths).
    pairs.sort_by(|a, b| a.signal.cmp(&b.signal));
    pairs.dedup_by(|a, b| a.signal == b.signal && a.emitter == b.emitter);

    CompositionResult { pairs, undelivered, orphans }
}
