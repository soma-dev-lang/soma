//! Shared name tables for the static call/interpolation checkers.
//!
//! The interpolation checker (interpolation_check.rs) and the dispatch
//! resolver (dispatch.rs) both need to know which bare names are
//! meaningful at runtime: builtins, handler names, sum-type variants,
//! memory slots, … This module centralizes that knowledge so the two
//! passes can never drift apart.

use crate::ast::*;
use std::collections::{HashMap, HashSet};

/// Every builtin callable by bare name at runtime. This mirrors the
/// dispatch tables in interpreter/builtins/*.rs plus the special-cased
/// names handled inline in interpreter/mod.rs (ws_connect, link, …).
/// Extra entries are harmless (they only create false negatives); a
/// MISSING entry creates false positives, so err on the side of more.
pub const BUILTIN_NAMES: &[&str] = &[
    // io / output
    "print", "echo", "read_file", "write_file", "read_files", "read_csv",
    "write_csv", "par_read_files", "par_word_count", "word_count", "include",
    "load_template", "render", "render_each",
    // string
    "contains", "starts_with", "ends_with", "replace", "split", "trim",
    "join", "uppercase", "lowercase", "substring", "index_of", "concat",
    "len", "to_string", "to_int", "to_float", "from_json", "to_json",
    "escape_html", "format_date", "type_of", "is_a", "is_type",
    // math
    "abs", "round", "floor", "ceil", "min", "max", "clamp", "pow", "sqrt",
    "random", "rand", "exp", "ln", "log", "log10", "idiv",
    "band", "bor", "bxor", "bnot", "shl", "shr",
    "bit_set", "bit_clr", "bit_test", "bit_len", "bit_next",
    "gcd", "pow_mod", "sqrt_int", "str_at", "str_eq", "str_len",
    // collections
    "map", "list", "push", "append", "filter", "find", "any", "all",
    "count", "reduce", "range", "reverse", "sort", "sort_by", "filter_by",
    "flatten", "zip", "enumerate", "nth", "merge", "with", "without",
    "pluck", "select", "distinct", "group_by", "top", "bottom", "agg",
    "sum", "sum_by", "avg", "avg_by", "min_by", "max_by", "count_by",
    "keys", "values", "describe", "inner_join", "left_join", "_coalesce",
    // http / serving
    "http_get", "http_post", "http_put", "http_delete", "http_patch",
    "html", "raw", "response", "redirect", "sse", "publish",
    "ws_connect", "ws_send", "link", "subscribe",
    // time
    "now", "now_ms", "today", "timestamp", "date_now", "sleep",
    // storage / state machines
    "load", "next_id", "transition", "get_status", "valid_transitions",
    // llm / agents
    "think", "think_json", "delegate", "anthropic", "openai", "approve",
    "remember", "recall", "clear_context", "set_budget", "tokens_used",
    "tokens_remaining", "trace", "clear_trace",
    // linalg / quant
    "mat", "matrix", "rows", "cols", "diag", "eye", "ones", "zeros",
    "clip", "quantile", "regress_sgd", "svd_lowrank", "clean_covariance",
    "var_gaussian", "var_historical", "expected_shortfall_historical",
    "impact_sqrt", "rie", "to_sampled", "drop_sampled", "sample_row",
    "importance_sample_rows", "indices", "row_indices", "col_indices",
];

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

        for b in BUILTIN_NAMES {
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

        for cell in collect_cells(program) {
            known.insert(cell.name.clone());
            for section in &cell.sections {
                match &section.node {
                    Section::OnSignal(on) => {
                        known.insert(on.signal_name.clone());
                        if matches!(cell.kind, CellKind::Cell | CellKind::Agent) {
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
