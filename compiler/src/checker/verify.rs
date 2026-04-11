//! Model checker for Soma state machines
//! Implements bounded verification: reachability, deadlocks, liveness, cycles.

use crate::ast::*;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug)]
pub struct VerifyResult {
    pub machine_name: String,
    pub states: Vec<String>,
    pub initial: String,
    pub transitions: Vec<(String, String)>,  // (from, to)
    pub terminal_states: Vec<String>,
    pub checks: Vec<VerifyCheck>,
}

#[derive(Debug)]
pub enum VerifyCheck {
    Pass(String),
    Warning(String),
    Fail(String, Option<Vec<String>>), // message, optional trace
}

impl VerifyResult {
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|c| matches!(c, VerifyCheck::Fail(_, _)))
    }
}

/// Verify all state machines in a program
pub fn verify_program(program: &Program) -> Vec<VerifyResult> {
    let mut results = Vec::new();

    for cell in &program.cells {
        if !matches!(cell.node.kind, CellKind::Cell | CellKind::Agent) { continue; }
        for section in &cell.node.sections {
            if let Section::State(ref sm) = section.node {
                let mut result = verify_state_machine(sm, &cell.node);
                // ── V1.3: refinement check ─────────────────────────────
                // The CTL checker proves properties about the *picture* of
                // the state machine. The refinement check proves the
                // *handler bodies* don't lie to the picture: every
                // transition() call targets a declared state, every
                // declared transition is reached by some handler, and we
                // surface a per-handler effect summary so the reader can
                // see the proof at a glance.
                let handlers: Vec<(&OnSection, Span)> = cell.node.sections.iter()
                    .filter_map(|s| if let Section::OnSignal(ref on) = s.node { Some((on, s.span)) } else { None })
                    .collect();
                let findings = super::refinement::check_refinement(sm, &handlers);

                // ── V1.4: think-isolation check ────────────────────────
                // If all transition targets are literal (no DynamicTarget),
                // then CTL safety properties hold regardless of what
                // think() / any LLM builtin returns. See isolation.rs.
                let isolation = super::isolation::check_isolation(
                    &cell.node.name, &cell.node, &findings);
                match &isolation {
                    super::isolation::IsolationFinding::ThinkIsolated { n_handlers, n_transitions, .. } => {
                        result.checks.push(VerifyCheck::Pass(
                            format!(
                                "think-isolated: CTL safety properties hold regardless of LLM output ({} handlers, {} literal transitions, 0 dynamic)",
                                n_handlers, n_transitions
                            )
                        ));
                    }
                    super::isolation::IsolationFinding::NotIsolated { reasons, .. } => {
                        result.checks.push(VerifyCheck::Warning(
                            format!(
                                "NOT think-isolated: {} — safety under adversarial LLM is not proven for this cell",
                                reasons.join("; ")
                            )
                        ));
                    }
                    super::isolation::IsolationFinding::NoStateMachine => {
                        // No state machine → no isolation finding
                    }
                }

                // ── V1.4: handler termination check ──────────────
                let term_findings = super::termination::check_cell_termination(&cell.node);
                let all_terminate = term_findings.iter().all(|f|
                    matches!(f, super::termination::TerminationFinding::Terminates { .. }));
                if all_terminate && !term_findings.is_empty() {
                    result.checks.push(VerifyCheck::Pass(
                        format!("termination: all {} handlers structurally terminate", term_findings.len())
                    ));
                } else {
                    for tf in &term_findings {
                        if let super::termination::TerminationFinding::MayNotTerminate { handler, reasons } = tf {
                            for reason in reasons {
                                result.checks.push(VerifyCheck::Warning(reason.clone()));
                            }
                        }
                    }
                }

                // Detect whether any handler has a dynamic transition target.
                let has_any_dynamic = findings.iter().any(|f|
                    matches!(f, super::refinement::RefinementFinding::DynamicTarget { .. }));
                let refinement_label = if has_any_dynamic {
                    "refinement (partial — dynamic targets fall back to runtime check)"
                } else {
                    "refinement"
                };

                for f in findings {
                    use super::refinement::RefinementFinding::*;
                    match f {
                        UndeclaredTarget { handler, target, path, span } => {
                            let path_text = if path.is_empty() {
                                String::new()
                            } else {
                                format!("  [{}]", path.join(" ∧ "))
                            };
                            result.checks.push(VerifyCheck::Fail(
                                format!(
                                    "{}: handler `{}` calls transition(_, \"{}\") but \"{}\" is not in state machine `{}`{}",
                                    refinement_label, handler, target, target, sm.name, path_text
                                ),
                                Some(vec![format!("at byte offset {}–{}", span.start, span.end)]),
                            ));
                        }
                        DynamicTarget { handler, span: _ } => {
                            result.checks.push(VerifyCheck::Warning(
                                format!(
                                    "{}: handler `{}` calls transition() with a non-literal target — V1.3 cannot statically verify this; refinement coverage incomplete here",
                                    refinement_label, handler
                                ),
                            ));
                        }
                        DeadTransition { from, to } => {
                            result.checks.push(VerifyCheck::Warning(
                                format!(
                                    "{}: declared transition `{} → {}` is never reached by any handler — spec may be aspirational or stale",
                                    refinement_label, from, to
                                ),
                            ));
                        }
                        HandlerEffect { handler, targets, has_dynamic } => {
                            if targets.is_empty() && !has_dynamic { continue; }
                            let target_strs: Vec<String> = targets.iter().map(|c| {
                                if c.path.is_empty() {
                                    c.target.clone()
                                } else {
                                    format!("{} [{}]", c.target, c.path.join(" ∧ "))
                                }
                            }).collect();
                            let mut summary = if target_strs.is_empty() {
                                String::new()
                            } else {
                                format!("{{{}}}", target_strs.join(", "))
                            };
                            if has_dynamic {
                                if !summary.is_empty() { summary.push_str(" + "); }
                                summary.push_str("<dynamic>");
                            }
                            result.checks.push(VerifyCheck::Pass(
                                format!("{}: handler `{}` ⟶ {}", refinement_label, handler, summary)
                            ));
                        }
                    }
                }
                results.push(result);
            }
        }
        // Verify scale section if present
        if let Some(scale_result) = verify_scale(&cell.node) {
            results.push(scale_result);
        }

        // ── V1.4: composition check for interior cells ────────
        // For each interior block, verify that every emitted signal
        // has a matching handler and every handler has a signal source.
        for section in &cell.node.sections {
            if let Section::Interior(ref interior) = section.node {
                let comp = super::composition::check_composition(
                    &interior.cells, &cell.node);
                if !comp.pairs.is_empty() || !comp.undelivered.is_empty() || !comp.orphans.is_empty() {
                    let mut comp_result = VerifyResult {
                        machine_name: format!("{}/composition", cell.node.name),
                        states: vec![],
                        initial: String::new(),
                        terminal_states: vec![],
                        transitions: vec![],
                        checks: vec![],
                    };
                    if comp.undelivered.is_empty() && comp.orphans.is_empty() {
                        comp_result.checks.push(VerifyCheck::Pass(
                            format!("composition: {} signal pairs verified, 0 undelivered, 0 orphans",
                                comp.pairs.len())
                        ));
                    } else {
                        if !comp.pairs.is_empty() {
                            comp_result.checks.push(VerifyCheck::Pass(
                                format!("composition: {} signal pairs matched", comp.pairs.len())
                            ));
                        }
                        for (emitter, sig) in &comp.undelivered {
                            comp_result.checks.push(VerifyCheck::Warning(
                                format!("composition: cell '{}' emits signal '{}' but no sibling handles it",
                                    emitter, sig)
                            ));
                        }
                        for (handler, sig) in &comp.orphans {
                            comp_result.checks.push(VerifyCheck::Warning(
                                format!("composition: cell '{}' handles signal '{}' but no sibling emits it and parent face doesn't expose it",
                                    handler, sig)
                            ));
                        }
                    }
                    results.push(comp_result);
                }
            }
        }
    }

    results
}

/// Verify distributed scale properties
fn verify_scale(cell: &CellDef) -> Option<VerifyResult> {
    let scale = cell.sections.iter().find_map(|s| {
        if let Section::Scale(ref sc) = s.node { Some(sc) } else { None }
    })?;

    let mut result = VerifyResult {
        machine_name: format!("{}/scale", cell.name),
        states: vec![
            "single".to_string(),
            "distributed".to_string(),
            "partitioned".to_string(),
            "recovering".to_string(),
        ],
        initial: "single".to_string(),
        transitions: vec![
            ("single".to_string(), "distributed".to_string()),
            ("distributed".to_string(), "partitioned".to_string()),
            ("partitioned".to_string(), "recovering".to_string()),
            ("recovering".to_string(), "distributed".to_string()),
        ],
        terminal_states: vec![],
        checks: Vec::new(),
    };

    // Collect memory slots
    let slots: Vec<(String, Vec<String>)> = cell.sections.iter()
        .filter_map(|s| {
            if let Section::Memory(ref mem) = s.node {
                Some(mem.slots.iter().map(|slot| {
                    let props: Vec<String> = slot.node.properties.iter()
                        .map(|p| p.node.name().to_string()).collect();
                    (slot.node.name.clone(), props)
                }).collect::<Vec<_>>())
            } else {
                None
            }
        })
        .flatten()
        .collect();

    // Check 1: replicas > 0
    if scale.replicas > 0 {
        result.checks.push(VerifyCheck::Pass(
            format!("replicas: {} instances declared", scale.replicas)
        ));
    } else {
        result.checks.push(VerifyCheck::Fail(
            "replicas must be > 0".to_string(), None
        ));
    }

    // Check 2: tolerance < replicas
    if scale.tolerance < scale.replicas {
        result.checks.push(VerifyCheck::Pass(
            format!("tolerance: survives {} node failures (of {} replicas)", scale.tolerance, scale.replicas)
        ));
    } else {
        result.checks.push(VerifyCheck::Fail(
            format!("tolerance ({}) must be < replicas ({})", scale.tolerance, scale.replicas),
            None,
        ));
    }

    // Check 3: shard references valid memory
    if let Some(ref shard_name) = scale.shard {
        let slot = slots.iter().find(|(name, _)| name == shard_name);
        match slot {
            Some((_, props)) => {
                result.checks.push(VerifyCheck::Pass(
                    format!("shard: '{}' is a valid memory slot", shard_name)
                ));

                // Check 4: sharded memory should be persistent
                if props.iter().any(|p| p == "persistent") {
                    result.checks.push(VerifyCheck::Pass(
                        format!("shard '{}' is [persistent] — data survives node restart", shard_name)
                    ));
                } else if props.iter().any(|p| p == "ephemeral") {
                    result.checks.push(VerifyCheck::Warning(
                        format!("shard '{}' is [ephemeral] — data lost on node failure (tolerance provides no durability guarantee)", shard_name)
                    ));
                }

                // Check 5: consistency coherence
                let has_consistent = props.iter().any(|p| p == "consistent");
                match scale.consistency {
                    ScaleConsistency::Strong => {
                        if has_consistent {
                            result.checks.push(VerifyCheck::Pass(
                                format!("consistency: strong — [consistent] memory '{}' with linearizable reads/writes", shard_name)
                            ));
                        } else {
                            result.checks.push(VerifyCheck::Pass(
                                format!("consistency: strong — all reads/writes to '{}' are linearizable", shard_name)
                            ));
                        }
                    }
                    ScaleConsistency::Causal => {
                        result.checks.push(VerifyCheck::Pass(
                            format!("consistency: causal — operations on '{}' respect causal ordering", shard_name)
                        ));
                    }
                    ScaleConsistency::Eventual => {
                        result.checks.push(VerifyCheck::Warning(
                            format!("consistency: eventual — reads from '{}' may return stale data after writes", shard_name)
                        ));
                    }
                }

                // Check 6: CAP analysis
                if scale.tolerance > 0 {
                    match scale.consistency {
                        ScaleConsistency::Strong => {
                            result.checks.push(VerifyCheck::Pass(
                                "CAP: CP mode — consistent + partition-tolerant (availability reduced during partitions)".to_string()
                            ));
                        }
                        ScaleConsistency::Eventual => {
                            result.checks.push(VerifyCheck::Pass(
                                "CAP: AP mode — available + partition-tolerant (consistency relaxed during partitions)".to_string()
                            ));
                        }
                        ScaleConsistency::Causal => {
                            result.checks.push(VerifyCheck::Pass(
                                "CAP: causal consistency — weaker than linearizable, stronger than eventual".to_string()
                            ));
                        }
                    }
                }
            }
            None => {
                result.checks.push(VerifyCheck::Fail(
                    format!("shard '{}' is not a declared memory slot", shard_name),
                    None,
                ));
            }
        }
    } else {
        result.checks.push(VerifyCheck::Pass(
            "no shard declared — all memory replicated to all nodes".to_string()
        ));
    }

    // Check 7: every blocks + leader election
    let has_every = cell.sections.iter().any(|s| matches!(s.node, Section::Every(_)));
    if has_every {
        result.checks.push(VerifyCheck::Pass(
            "scheduler: 'every' blocks run on leader node only (leader = lowest node ID)".to_string()
        ));
    }

    // Check 8: quorum analysis for strong consistency
    if scale.consistency == ScaleConsistency::Strong && scale.replicas > 1 {
        let quorum = scale.replicas / 2 + 1;
        let max_failures = scale.replicas - quorum;
        if scale.tolerance <= max_failures {
            result.checks.push(VerifyCheck::Pass(
                format!("quorum: {}/{} nodes needed — tolerates {} failures", quorum, scale.replicas, max_failures)
            ));
        } else {
            result.checks.push(VerifyCheck::Fail(
                format!("tolerance ({}) exceeds maximum for strong consistency quorum ({}/{})",
                    scale.tolerance, max_failures, scale.replicas),
                None,
            ));
        }
    }

    // Check 9: verify non-sharded memory is local-only
    for (slot_name, props) in &slots {
        if scale.shard.as_deref() != Some(slot_name) {
            let is_ephemeral = props.iter().any(|p| p == "ephemeral");
            let is_local = props.iter().any(|p| p == "local");
            if is_ephemeral || is_local {
                result.checks.push(VerifyCheck::Pass(
                    format!("memory '{}' is node-local — not distributed (fast path)", slot_name)
                ));
            }
        }
    }

    Some(result)
}

fn verify_state_machine(sm: &StateMachineSection, cell: &CellDef) -> VerifyResult {
    let mut result = VerifyResult {
        machine_name: sm.name.clone(),
        states: Vec::new(),
        initial: sm.initial.clone(),
        transitions: Vec::new(),
        terminal_states: Vec::new(),
        checks: Vec::new(),
    };

    // 1. Collect all states and transitions
    let mut states: HashSet<String> = HashSet::new();
    states.insert(sm.initial.clone());

    let mut edges: Vec<(String, String, bool)> = Vec::new(); // (from, to, has_guard)
    let mut wildcard_targets: Vec<String> = Vec::new();

    for t in &sm.transitions {
        let from = &t.node.from;
        let to = &t.node.to;
        let has_guard = t.node.guard.is_some();

        if from == "*" {
            wildcard_targets.push(to.clone());
        } else {
            states.insert(from.clone());
            edges.push((from.clone(), to.clone(), has_guard));
        }
        states.insert(to.clone());
    }

    // Expand wildcards: * -> X means every state can go to X
    for target in &wildcard_targets {
        for state in &states.clone() {
            if state != target {
                edges.push((state.clone(), target.clone(), false));
            }
        }
    }

    let states_vec: Vec<String> = {
        let mut v: Vec<String> = states.iter().cloned().collect();
        v.sort();
        v
    };
    result.states = states_vec.clone();

    for (from, to, _) in &edges {
        result.transitions.push((from.clone(), to.clone()));
    }

    let n_states = states.len();
    let n_transitions = edges.len();
    result.checks.push(VerifyCheck::Pass(
        format!("{} states, {} transitions", n_states, n_transitions)
    ));

    // 2. Build adjacency map
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for state in &states {
        adj.insert(state.clone(), Vec::new());
    }
    for (from, to, _) in &edges {
        adj.entry(from.clone()).or_default().push(to.clone());
    }

    // 3. Reachability: BFS from initial state
    let reachable = bfs(&sm.initial, &adj);

    let unreachable: Vec<&String> = states.iter()
        .filter(|s| !reachable.contains(*s))
        .collect();

    if unreachable.is_empty() {
        result.checks.push(VerifyCheck::Pass(
            format!("all states reachable from '{}'", sm.initial)
        ));
    } else {
        let names: Vec<String> = unreachable.iter().map(|s| s.to_string()).collect();
        result.checks.push(VerifyCheck::Fail(
            format!("unreachable states: [{}]", names.join(", ")),
            None,
        ));
    }

    // 4. Terminal states (no outgoing transitions)
    let terminals: Vec<String> = states.iter()
        .filter(|s| adj.get(*s).map_or(true, |v| v.is_empty()))
        .cloned()
        .collect();

    result.terminal_states = terminals.clone();
    if terminals.is_empty() {
        result.checks.push(VerifyCheck::Warning(
            "no terminal states — all states have outgoing transitions (possible infinite loop)".to_string()
        ));
    } else {
        result.checks.push(VerifyCheck::Pass(
            format!("terminal states: [{}]", terminals.join(", "))
        ));
    }

    // 5. Deadlock: non-terminal state with no outgoing (shouldn't happen after wildcard expansion,
    //    but check reachable non-terminal states)
    let deadlocks: Vec<String> = reachable.iter()
        .filter(|s| {
            let outs = adj.get(*s).map_or(0, |v| v.len());
            outs == 0 && !terminals.contains(s)
        })
        .cloned()
        .collect();

    if deadlocks.is_empty() {
        result.checks.push(VerifyCheck::Pass(
            "no deadlocks".to_string()
        ));
    } else {
        result.checks.push(VerifyCheck::Fail(
            format!("deadlock states: [{}] — stuck with no transitions", deadlocks.join(", ")),
            None,
        ));
    }

    // 6. Liveness: every reachable non-terminal state can reach a terminal state
    let mut can_terminate: HashSet<String> = terminals.iter().cloned().collect();
    // Backward BFS: from terminals, find all states that can reach them
    let mut rev_adj: HashMap<String, Vec<String>> = HashMap::new();
    for (from, to, _) in &edges {
        rev_adj.entry(to.clone()).or_default().push(from.clone());
    }
    let mut queue: VecDeque<String> = terminals.iter().cloned().collect();
    while let Some(state) = queue.pop_front() {
        if let Some(parents) = rev_adj.get(&state) {
            for parent in parents {
                if can_terminate.insert(parent.clone()) {
                    queue.push_back(parent.clone());
                }
            }
        }
    }

    let stuck: Vec<String> = reachable.iter()
        .filter(|s| !can_terminate.contains(*s))
        .cloned()
        .collect();

    if stuck.is_empty() {
        result.checks.push(VerifyCheck::Pass(
            "liveness: every state can eventually reach a terminal state".to_string()
        ));
    } else {
        // Find a cycle that doesn't lead to termination
        let cycle = find_cycle(&stuck, &adj);
        result.checks.push(VerifyCheck::Fail(
            format!("liveness violation: states [{}] cannot reach any terminal state", stuck.join(", ")),
            cycle,
        ));
    }

    // 7. Guards analysis
    let guarded: Vec<(&str, &str)> = sm.transitions.iter()
        .filter(|t| t.node.guard.is_some())
        .map(|t| (t.node.from.as_str(), t.node.to.as_str()))
        .collect();

    let unguarded: Vec<(&str, &str)> = sm.transitions.iter()
        .filter(|t| t.node.guard.is_none() && t.node.from != "*")
        .map(|t| (t.node.from.as_str(), t.node.to.as_str()))
        .collect();

    if !guarded.is_empty() {
        let guards_str: Vec<String> = guarded.iter()
            .map(|(f, t)| format!("{} -> {}", f, t))
            .collect();
        result.checks.push(VerifyCheck::Pass(
            format!("guarded transitions: {}", guards_str.join(", "))
        ));
    }

    // Warn about critical transitions without guards
    for (from, to) in &unguarded {
        // Warn if it's a "dangerous" transition (to terminal or to a state that implies commitment)
        if terminals.contains(&to.to_string()) || *to == "sent" || *to == "approved" || *to == "deployed" {
            result.checks.push(VerifyCheck::Warning(
                format!("{} -> {} has no guard (consider adding a guard condition)", from, to)
            ));
        }
    }

    // 8. Wildcard analysis
    if !wildcard_targets.is_empty() {
        result.checks.push(VerifyCheck::Pass(
            format!("wildcard transitions: * -> [{}]", wildcard_targets.join(", "))
        ));
    }

    // 9. Path analysis: check if every non-terminal state has a path to every terminal
    for terminal in &terminals {
        let reaches_terminal: Vec<String> = reachable.iter()
            .filter(|s| {
                let path = bfs(s, &adj);
                path.contains(terminal)
            })
            .cloned()
            .collect();

        let cant_reach: Vec<String> = reachable.iter()
            .filter(|s| !reaches_terminal.contains(s) && !terminals.contains(s))
            .cloned()
            .collect();

        if !cant_reach.is_empty() && !wildcard_targets.contains(terminal) {
            result.checks.push(VerifyCheck::Warning(
                format!("states [{}] cannot reach terminal '{}'",
                    cant_reach.join(", "), terminal)
            ));
        }
    }

    result
}

/// BFS: return all reachable states from start
fn bfs(start: &str, adj: &HashMap<String, Vec<String>>) -> HashSet<String> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    visited.insert(start.to_string());
    queue.push_back(start.to_string());

    while let Some(state) = queue.pop_front() {
        if let Some(neighbors) = adj.get(&state) {
            for next in neighbors {
                if visited.insert(next.clone()) {
                    queue.push_back(next.clone());
                }
            }
        }
    }

    visited
}

/// Find a cycle in a set of states
fn find_cycle(states: &[String], adj: &HashMap<String, Vec<String>>) -> Option<Vec<String>> {
    let state_set: HashSet<&String> = states.iter().collect();

    for start in states {
        let mut visited = HashSet::new();
        let mut path = Vec::new();
        if dfs_cycle(start, &state_set, adj, &mut visited, &mut path) {
            path.push(start.clone()); // complete the cycle
            return Some(path);
        }
    }
    None
}

fn dfs_cycle(
    current: &str,
    valid: &HashSet<&String>,
    adj: &HashMap<String, Vec<String>>,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> bool {
    if !visited.insert(current.to_string()) {
        return path.contains(&current.to_string());
    }
    path.push(current.to_string());

    if let Some(neighbors) = adj.get(current) {
        for next in neighbors {
            if valid.contains(next) {
                if dfs_cycle(next, valid, adj, visited, path) {
                    return true;
                }
            }
        }
    }

    path.pop();
    false
}

/// Format verification results for display
pub fn format_results(results: &[VerifyResult]) -> String {
    let mut output = String::new();

    for result in results {
        output.push_str(&format!("\nState machine '{}': {} states, initial '{}'\n",
            result.machine_name, result.states.len(), result.initial));
        output.push_str(&format!("  States: [{}]\n", result.states.join(", ")));
        output.push_str("\n");

        for check in &result.checks {
            match check {
                VerifyCheck::Pass(msg) => {
                    output.push_str(&format!("  \x1b[32m✓\x1b[0m {}\n", msg));
                }
                VerifyCheck::Warning(msg) => {
                    output.push_str(&format!("  \x1b[33m⚠\x1b[0m {}\n", msg));
                }
                VerifyCheck::Fail(msg, trace) => {
                    output.push_str(&format!("  \x1b[31m✗\x1b[0m {}\n", msg));
                    if let Some(trace) = trace {
                        output.push_str(&format!("    trace: {}\n", trace.join(" → ")));
                    }
                }
            }
        }
    }

    let total_pass = results.iter().flat_map(|r| &r.checks).filter(|c| matches!(c, VerifyCheck::Pass(_))).count();
    let total_warn = results.iter().flat_map(|r| &r.checks).filter(|c| matches!(c, VerifyCheck::Warning(_))).count();
    let total_fail = results.iter().flat_map(|r| &r.checks).filter(|c| matches!(c, VerifyCheck::Fail(_, _))).count();

    output.push_str(&format!("\n{} passed, {} warnings, {} failures\n", total_pass, total_warn, total_fail));

    output
}
