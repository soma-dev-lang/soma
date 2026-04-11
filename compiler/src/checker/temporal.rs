//! Temporal property verification for Soma state machines
//!
//! Supports:
//!   always(P)          — P holds in every reachable state
//!   never(P)           — P never holds in any reachable state
//!   eventually(P)      — every path from initial reaches a state where P holds
//!   after(S, P)        — in every state reachable from S, P eventually holds
//!   reachable(S)       — state S is reachable from initial
//!   deadlock_free      — no reachable state has zero outgoing transitions (unless terminal)
//!
//! The checker exhaustively explores all paths in the state machine graph.
//! Soma state machines are small (5-15 states), so this is instantaneous.

use std::collections::{HashMap, HashSet, VecDeque};

/// A temporal property to verify
#[derive(Debug, Clone)]
pub enum Property {
    /// P holds in every reachable state
    Always(StatePredicate),
    /// P never holds in any reachable state
    Never(StatePredicate),
    /// Every execution path eventually reaches a state where P holds
    Eventually(StatePredicate),
    /// After reaching state S, P eventually holds on all subsequent paths
    After(String, StatePredicate),
    /// No reachable state is a deadlock (has outgoing transitions or is terminal)
    DeadlockFree,
    /// States S1 and S2 are never simultaneously reachable in a composed system
    Mutex(String, String),
}

/// A predicate on a state
#[derive(Debug, Clone)]
pub enum StatePredicate {
    /// Current state equals a specific value
    InState(String),
    /// Current state is NOT a specific value
    NotInState(String),
    /// Current state is one of a set
    InSet(Vec<String>),
    /// A guard expression holds (for future: symbolic evaluation)
    GuardHolds(String),
    /// Conjunction
    And(Box<StatePredicate>, Box<StatePredicate>),
    /// Disjunction
    Or(Box<StatePredicate>, Box<StatePredicate>),
    /// Negation
    Not(Box<StatePredicate>),
}

impl StatePredicate {
    pub fn eval(&self, current_state: &str) -> bool {
        match self {
            StatePredicate::InState(s) => current_state == s,
            StatePredicate::NotInState(s) => current_state != s,
            StatePredicate::InSet(set) => set.iter().any(|s| s == current_state),
            StatePredicate::GuardHolds(_) => true, // future: symbolic eval
            StatePredicate::And(a, b) => a.eval(current_state) && b.eval(current_state),
            StatePredicate::Or(a, b) => a.eval(current_state) || b.eval(current_state),
            StatePredicate::Not(inner) => !inner.eval(current_state),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            StatePredicate::InState(s) => format!("state == '{}'", s),
            StatePredicate::NotInState(s) => format!("state != '{}'", s),
            StatePredicate::InSet(set) => format!("state in [{}]", set.join(", ")),
            StatePredicate::GuardHolds(g) => format!("guard: {}", g),
            StatePredicate::And(a, b) => format!("({} && {})", a.describe(), b.describe()),
            StatePredicate::Or(a, b) => format!("({} || {})", a.describe(), b.describe()),
            StatePredicate::Not(inner) => format!("!({})", inner.describe()),
        }
    }
}

/// Result of checking a single property
#[derive(Debug)]
pub struct PropertyResult {
    pub property: String,
    pub passed: bool,
    pub message: String,
    pub counter_example: Option<Vec<String>>,
}

/// State machine graph for verification
pub struct StateMachineGraph {
    pub name: String,
    pub initial: String,
    pub states: HashSet<String>,
    pub adj: HashMap<String, Vec<(String, Option<String>)>>, // state -> [(target, guard_desc)]
    pub terminals: HashSet<String>,
}

impl StateMachineGraph {
    pub fn from_ast(sm: &crate::ast::StateMachineSection) -> Self {
        let mut states = HashSet::new();
        let mut adj: HashMap<String, Vec<(String, Option<String>)>> = HashMap::new();
        let mut wildcard_targets = Vec::new();

        states.insert(sm.initial.clone());

        for t in &sm.transitions {
            let from = &t.node.from;
            let to = &t.node.to;
            let guard_desc = t.node.guard.as_ref().map(|_| "guarded".to_string());

            states.insert(to.clone());

            if from == "*" {
                wildcard_targets.push((to.clone(), guard_desc));
            } else {
                states.insert(from.clone());
                adj.entry(from.clone()).or_default().push((to.clone(), guard_desc));
            }
        }

        // Expand wildcards
        for (target, guard) in &wildcard_targets {
            for state in &states.clone() {
                if state != target {
                    adj.entry(state.clone()).or_default().push((target.clone(), guard.clone()));
                }
            }
        }

        // Initialize empty adj for states with no outgoing
        for state in &states {
            adj.entry(state.clone()).or_default();
        }

        let terminals: HashSet<String> = states.iter()
            .filter(|s| adj.get(*s).map_or(true, |v| v.is_empty()))
            .cloned()
            .collect();

        StateMachineGraph {
            name: sm.name.clone(),
            initial: sm.initial.clone(),
            states,
            adj,
            terminals,
        }
    }

    /// All states reachable from initial
    pub fn reachable(&self) -> HashSet<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        visited.insert(self.initial.clone());
        queue.push_back(self.initial.clone());
        while let Some(s) = queue.pop_front() {
            if let Some(nexts) = self.adj.get(&s) {
                for (next, _) in nexts {
                    if visited.insert(next.clone()) {
                        queue.push_back(next.clone());
                    }
                }
            }
        }
        visited
    }

    /// Find a path from start to a state satisfying predicate
    fn find_path_to(&self, start: &str, pred: &StatePredicate) -> Option<Vec<String>> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut parent: HashMap<String, String> = HashMap::new();

        visited.insert(start.to_string());
        queue.push_back(start.to_string());

        while let Some(s) = queue.pop_front() {
            if pred.eval(&s) {
                // Reconstruct path
                let mut path = vec![s.clone()];
                let mut current = s;
                while let Some(p) = parent.get(&current) {
                    path.push(p.clone());
                    current = p.clone();
                }
                path.reverse();
                return Some(path);
            }
            if let Some(nexts) = self.adj.get(&s) {
                for (next, _) in nexts {
                    if visited.insert(next.clone()) {
                        parent.insert(next.clone(), s.clone());
                        queue.push_back(next.clone());
                    }
                }
            }
        }
        None
    }

    /// Find a path from start that NEVER satisfies predicate (counter-example for eventually)
    fn find_path_avoiding(&self, start: &str, pred: &StatePredicate, max_depth: usize) -> Option<Vec<String>> {
        // DFS: find a path that reaches a terminal or cycles without ever hitting pred
        let mut best_path: Option<Vec<String>> = None;

        fn dfs(
            graph: &StateMachineGraph,
            current: &str,
            pred: &StatePredicate,
            path: &mut Vec<String>,
            visited: &mut HashSet<String>,
            max_depth: usize,
            best: &mut Option<Vec<String>>,
        ) {
            if path.len() > max_depth { return; }

            if pred.eval(current) { return; } // predicate satisfied, not a counter-example

            // Terminal state without satisfying predicate → counter-example
            if graph.terminals.contains(current) {
                *best = Some(path.clone());
                return;
            }

            // Cycle without satisfying predicate → counter-example
            if visited.contains(current) {
                let mut p = path.clone();
                p.push(format!("... cycle back to '{}'", current));
                *best = Some(p);
                return;
            }

            visited.insert(current.to_string());

            if let Some(nexts) = graph.adj.get(current) {
                for (next, _) in nexts {
                    path.push(next.clone());
                    dfs(graph, next, pred, path, visited, max_depth, best);
                    path.pop();
                    if best.is_some() { return; } // found one, stop
                }
            }

            visited.remove(current);
        }

        let mut path = vec![start.to_string()];
        let mut visited = HashSet::new();
        dfs(self, start, pred, &mut path, &mut visited, max_depth, &mut best_path);
        best_path
    }
}

/// Check a property against a state machine graph
pub fn check_property(graph: &StateMachineGraph, property: &Property) -> PropertyResult {
    let reachable = graph.reachable();

    match property {
        Property::Always(pred) => {
            // Check: pred holds in every reachable state
            for state in &reachable {
                if !pred.eval(state) {
                    // Find path to this violating state
                    let path = graph.find_path_to(&graph.initial, &StatePredicate::InState(state.clone()));
                    return PropertyResult {
                        property: format!("always({})", pred.describe()),
                        passed: false,
                        message: format!("violated in state '{}'", state),
                        counter_example: path,
                    };
                }
            }
            PropertyResult {
                property: format!("always({})", pred.describe()),
                passed: true,
                message: format!("holds in all {} reachable states", reachable.len()),
                counter_example: None,
            }
        }

        Property::Never(pred) => {
            // Check: pred never holds in any reachable state
            for state in &reachable {
                if pred.eval(state) {
                    let path = graph.find_path_to(&graph.initial, &StatePredicate::InState(state.clone()));
                    return PropertyResult {
                        property: format!("never({})", pred.describe()),
                        passed: false,
                        message: format!("violated: state '{}' is reachable", state),
                        counter_example: path,
                    };
                }
            }
            PropertyResult {
                property: format!("never({})", pred.describe()),
                passed: true,
                message: "never reached in any execution".to_string(),
                counter_example: None,
            }
        }

        Property::Eventually(pred) => {
            // Check: every path from initial eventually reaches a state where pred holds.
            //
            // SOUNDNESS NOTE: the depth bound for the counter-example search must be
            // ≥ |reachable| so that any acyclic path through the reachable subgraph
            // can be fully explored before we conclude "no counter-example exists".
            // The previous hard-coded `50` was a soundness gap: on a 60-state linear
            // chain ending in a non-satisfying terminal, the DFS gave up at depth 50
            // and reported the property as PASSING when in fact every path is a
            // counter-example. See `tests/rigor_eventually_long_chain.rs`.
            let bound = reachable.len() + 1;
            let counter = graph.find_path_avoiding(&graph.initial, pred, bound);
            if let Some(path) = counter {
                PropertyResult {
                    property: format!("eventually({})", pred.describe()),
                    passed: false,
                    message: "exists a path that never satisfies the predicate".to_string(),
                    counter_example: Some(path),
                }
            } else {
                PropertyResult {
                    property: format!("eventually({})", pred.describe()),
                    passed: true,
                    message: "all paths eventually satisfy the predicate".to_string(),
                    counter_example: None,
                }
            }
        }

        Property::After(state, pred) => {
            // Check: after reaching state S, pred eventually holds
            if !reachable.contains(state) {
                return PropertyResult {
                    property: format!("after('{}', {})", state, pred.describe()),
                    passed: true,
                    message: format!("state '{}' is unreachable (vacuously true)", state),
                    counter_example: None,
                };
            }
            // Same soundness fix as Eventually above: the bound must be ≥ |reachable|
            // so the DFS can explore every acyclic path through the subgraph.
            let bound = reachable.len() + 1;
            let counter = graph.find_path_avoiding(state, pred, bound);
            if let Some(mut path) = counter {
                // Prepend path from initial to the trigger state
                if let Some(prefix) = graph.find_path_to(&graph.initial, &StatePredicate::InState(state.clone())) {
                    let mut full = prefix;
                    full.extend(path.drain(1..)); // skip duplicate of trigger state
                    path = full;
                }
                PropertyResult {
                    property: format!("after('{}', {})", state, pred.describe()),
                    passed: false,
                    message: format!("after reaching '{}', predicate can be avoided", state),
                    counter_example: Some(path),
                }
            } else {
                PropertyResult {
                    property: format!("after('{}', {})", state, pred.describe()),
                    passed: true,
                    message: format!("after '{}', all paths eventually satisfy the predicate", state),
                    counter_example: None,
                }
            }
        }

        Property::DeadlockFree => {
            let deadlocks: Vec<String> = reachable.iter()
                .filter(|s| {
                    let outs = graph.adj.get(*s).map_or(0, |v| v.len());
                    outs == 0 && !graph.terminals.contains(*s)
                })
                .cloned()
                .collect();

            if deadlocks.is_empty() {
                PropertyResult {
                    property: "deadlock_free".to_string(),
                    passed: true,
                    message: "no deadlocks in any reachable state".to_string(),
                    counter_example: None,
                }
            } else {
                let path = graph.find_path_to(&graph.initial, &StatePredicate::InSet(deadlocks.clone()));
                PropertyResult {
                    property: "deadlock_free".to_string(),
                    passed: false,
                    message: format!("deadlock states: [{}]", deadlocks.join(", ")),
                    counter_example: path,
                }
            }
        }

        Property::Mutex(s1, s2) => {
            // Both states are never simultaneously reachable (for composed systems)
            // In a single machine: s1 and s2 are never on the same path at the same time
            // Simplified: check that there's no transition from s1 to s2 or s2 to s1
            // (in a single SM, you can only be in one state at a time, so this checks
            //  that you can't reach s2 from s1 without going through a reset)
            let s1_reachable = if reachable.contains(s1) {
                let mut visited = HashSet::new();
                let mut queue = VecDeque::new();
                visited.insert(s1.clone());
                queue.push_back(s1.clone());
                while let Some(s) = queue.pop_front() {
                    if let Some(nexts) = graph.adj.get(&s) {
                        for (next, _) in nexts {
                            if visited.insert(next.clone()) {
                                queue.push_back(next.clone());
                            }
                        }
                    }
                }
                visited
            } else {
                HashSet::new()
            };

            if s1_reachable.contains(s2) {
                let path = graph.find_path_to(s1, &StatePredicate::InState(s2.clone()));
                PropertyResult {
                    property: format!("mutex('{}', '{}')", s1, s2),
                    passed: false,
                    message: format!("'{}' can reach '{}' — they are not mutually exclusive", s1, s2),
                    counter_example: path,
                }
            } else {
                PropertyResult {
                    property: format!("mutex('{}', '{}')", s1, s2),
                    passed: true,
                    message: format!(
                        "'{}' and '{}' are mutually exclusive\n    \
                         warning: mutex({}, {}) is trivially satisfied for a single-cell state machine \
                         (a sequential machine is always in one state at a time). \
                         Mutex is meaningful only for composed cells.",
                        s1, s2, s1, s2
                    ),
                    counter_example: None,
                }
            }
        }
    }
}

/// Format property results for display
pub fn format_property_results(machine: &str, results: &[PropertyResult]) -> String {
    let mut output = String::new();

    output.push_str(&format!("\n  Temporal properties for '{}':\n", machine));

    for r in results {
        if r.passed {
            output.push_str(&format!("  \x1b[32m✓\x1b[0m {} — {}\n", r.property, r.message));
        } else {
            output.push_str(&format!("  \x1b[31m✗\x1b[0m {} — {}\n", r.property, r.message));
            if let Some(ref trace) = r.counter_example {
                output.push_str(&format!("    counter-example: {}\n", trace.join(" → ")));
            }
        }
    }

    output
}
