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
        if cell.node.kind != CellKind::Cell { continue; }
        for section in &cell.node.sections {
            if let Section::State(ref sm) = section.node {
                results.push(verify_state_machine(sm, &cell.node));
            }
        }
    }

    results
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
