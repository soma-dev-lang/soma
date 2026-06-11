//! Shared name tables for the static call/interpolation checkers.
//!
//! The interpolation checker (interpolation_check.rs) and the dispatch
//! resolver (dispatch.rs) both need to know which bare names are
//! meaningful at runtime: builtins, handler names, sum-type variants,
//! memory slots, … This module centralizes that knowledge so the two
//! passes can never drift apart.

use crate::ast::*;
use std::collections::{HashMap, HashSet};

/// Names dispatched inline by the interpreter rather than through the
/// builtins tables: `append` only works through the in-place assignment
/// fast path (`items = append(items, x)`), and `_coalesce` backs `??`.
const INLINE_BUILTIN_NAMES: &[&str] = &["append", "_coalesce"];

/// Every builtin callable by bare name at runtime, DERIVED from the
/// builtin registry (interpreter/builtins/registry.rs — the single
/// source of truth) so this list can never drift from the dispatch
/// tables again. The registry's 'reserved' category (documented but
/// never dispatched) is excluded on purpose: calls to those names fail
/// at runtime and must therefore fail check too.
pub fn builtin_names() -> &'static HashSet<&'static str> {
    use std::sync::OnceLock;
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        crate::interpreter::builtins::registry::BUILTINS
            .iter()
            .filter(|b| b.category != "reserved")
            .map(|b| b.name)
            .chain(INLINE_BUILTIN_NAMES.iter().copied())
            .collect()
    })
}

/// Program-wide index of every name that can resolve at runtime.
/// Built once per `soma check` / `soma verify` invocation.
pub struct ProgramIndex {
    /// handler name → cells (in declaration order) that define it.
    pub handler_map: HashMap<String, Vec<String>>,
    /// Every name that is "known" in identifier position somewhere:
    /// handler names, face decls, given bindings, memory slots, cell
    /// names, sum-type names + variants, state names. Superset by
    /// design — extra names only cost false negatives.
    pub known: HashSet<String>,
    /// Sum-type variant names (constructor position).
    pub variants: HashSet<String>,
    /// Memory slot names across all cells.
    pub slots: HashSet<String>,
}

/// Collect all cells in the program, recursing into interior sections.
pub fn collect_cells(program: &Program) -> Vec<&CellDef> {
    fn rec<'a>(cell: &'a CellDef, out: &mut Vec<&'a CellDef>) {
        out.push(cell);
        for s in &cell.sections {
            if let Section::Interior(ref interior) = s.node {
                for child in &interior.cells {
                    rec(&child.node, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    for c in &program.cells {
        rec(&c.node, &mut out);
    }
    out
}

impl ProgramIndex {
    pub fn build(program: &Program) -> Self {
        let mut handler_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut known: HashSet<String> = HashSet::new();
        let mut variants: HashSet<String> = HashSet::new();
        let mut slots: HashSet<String> = HashSet::new();

        for b in builtin_names() {
            known.insert((*b).to_string());
        }
        // [native] handlers have their own builtin vocabulary (sin,
        // buffer, hashmap, …) — those names must resolve too.
        for b in super::native::ALLOWED_BUILTINS {
            known.insert((*b).to_string());
        }
        known.insert("true".to_string());
        known.insert("false".to_string());
        known.insert("_".to_string());

        // Only TOP-LEVEL cells' handlers are callable by bare name: the
        // interpreter's handler cache never registers interior cells
        // (they communicate via emit), so counting them here made the
        // checker bless calls that are UndefinedFn at runtime.
        let top_level: HashSet<&str> = program.cells.iter()
            .map(|c| c.node.name.as_str())
            .collect();

        for cell in collect_cells(program) {
            known.insert(cell.name.clone());
            let is_top_level = top_level.contains(cell.name.as_str());
            for section in &cell.sections {
                match &section.node {
                    Section::OnSignal(on) => {
                        known.insert(on.signal_name.clone());
                        if is_top_level && matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
                            let definers = handler_map
                                .entry(on.signal_name.clone())
                                .or_default();
                            if !definers.contains(&cell.name) {
                                definers.push(cell.name.clone());
                            }
                        }
                    }
                    Section::Face(face) => {
                        for decl in &face.declarations {
                            match &decl.node {
                                FaceDecl::Signal(s) => { known.insert(s.name.clone()); }
                                FaceDecl::Await(a) => { known.insert(a.name.clone()); }
                                FaceDecl::Tool(t) => { known.insert(t.name.clone()); }
                                FaceDecl::Given(g) => { known.insert(g.name.clone()); }
                                FaceDecl::Promise(_) => {}
                            }
                        }
                    }
                    Section::Memory(mem) => {
                        for slot in &mem.slots {
                            known.insert(slot.node.name.clone());
                            slots.insert(slot.node.name.clone());
                        }
                    }
                    Section::Variants(vs) => {
                        for v in &vs.variants {
                            known.insert(v.node.name.clone());
                            variants.insert(v.node.name.clone());
                        }
                    }
                    Section::State(sm) => {
                        known.insert(sm.name.clone());
                        for t in &sm.transitions {
                            known.insert(t.node.from.clone());
                            known.insert(t.node.to.clone());
                        }
                        known.insert(sm.initial.clone());
                    }
                    _ => {}
                }
            }
        }

        Self { handler_map, known, variants, slots }
    }
}

/// Levenshtein edit distance, used for "did you mean?" suggestions.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Best "did you mean?" candidate within edit distance 2.
/// Ties resolve to the lexicographically smallest candidate so the
/// suggestion is deterministic across runs.
pub fn suggest<'a, I>(name: &str, candidates: I) -> Option<String>
where
    I: IntoIterator<Item = &'a String>,
{
    candidates
        .into_iter()
        .filter(|c| c.as_str() != name)
        .map(|c| (levenshtein(name, c), c))
        .filter(|(d, c)| *d <= 2 && *d < name.len().max(c.len()))
        .min_by(|(da, ca), (db, cb)| da.cmp(db).then(ca.cmp(cb)))
        .map(|(_, c)| c.clone())
}
