//! Stable diagnostic codes. A code never changes meaning and is never
//! reused: new kinds are APPENDED to the tables below (the position is the
//! number). `soma explain E0142` prints what a code means; `--json`
//! diagnostics carry it as `code`.
//!
//! E0xxx: check errors (by `kind`), E1xxx: syntax errors, W0xxx: warnings,
//! T0xxx: type errors (the inference pass, `checker/types.rs`).

/// Check-error kinds, in the order they were given a code. APPEND ONLY.
const ERROR_KINDS: &[(&str, &str)] = &[
    ("property_contradiction", "two properties of one memory slot exclude each other"),
    ("invalid_properties", "a combination of slot properties that cannot hold"),
    ("missing_handler", "a face signal has no `on` handler"),
    ("param_mismatch", "a handler's parameter count differs from its face signal"),
    ("duplicate_cell", "two cells share a name"),
    ("duplicate_slot", "two memory slots of one cell share a name"),
    ("duplicate_signal", "two handlers of one cell share a name"),
    ("shard_not_found", "the scale section shards a slot the cell does not declare"),
    ("consistency_mismatch", "a slot property contradicts the scale section's consistency"),
    ("promise_violation", "the cell breaks a `promise` of its face"),
    ("interpolation_undefined", "a string interpolation names a variable that is not in scope"),
    ("dispatch", "a call resolves to no handler, to several, or with the wrong argument count"),
    ("unmatched_await", "an `await` names a signal nothing sends"),
    ("unmatched_handler", "a handler answers a signal the face does not declare"),
    ("signal_type_mismatch", "a handler's parameter types differ from its face signal"),
    ("custom_checker_failed", "a `cell checker` rule failed"),
    ("sum_type_issue", "a `match` over a sum type is not exhaustive, or names an unknown variant"),
    ("determinism_violation", "a [deterministic] handler uses a nondeterministic builtin"),
    ("model_capability_missing", "the configured model lacks a capability the agent needs"),
    ("cost_exceeded", "a declared `cost` bound is lower than what the handler can spend"),
    ("budget_exceeded", "a declared memory budget is lower than the proven peak"),
    ("invariant_issue", "a memory invariant is malformed or names something it cannot read"),
    ("dead_code", "statements that can never run"),
    ("guard_scope", "a transition guard reads a name the calling handler does not bind"),
    ("habit", "an idiom from another language that means something else in Soma"),
    ("foreign_slot", "a handler touches a memory slot of another cell"),
    ("unknown_transition_target", "transition() names a state the machine does not declare"),
    ("undefined_variable", "a name with no let, parameter, loop variable, match binding or slot"),
    ("function_as_value", "a handler or builtin used as a value instead of being called"),
    ("ws_placement", "`on ws` outside the cell that owns `request`"),
    ("unknown_promise", "a `promise` no checker implements"),
    ("undefined_function", "a call to a name that is neither a handler nor a builtin"),
    ("think_option", "an unknown or mistyped think() option"),
    ("storage_collision", "two slots would share one storage table"),
    ("slot_method_on_local", "a slot method (.set / .push / .delete) on a local value"),
    ("runtime_unknown_cell", "the runtime section names a cell that does not exist"),
    ("runtime_signal_args", "the runtime section sends a signal with the wrong arguments"),
    ("native_vocabulary", "a [native] handler uses something native code does not have"),
    ("int_as_bool", "a number, string or list where a condition needs a Bool"),
    ("face_return_type", "a handler returns a value its face signal does not declare"),
    ("arity", "a call with the wrong number of arguments"),
    ("write_to_undefined", "an assignment to a name that was never bound"),
    ("write_to_loop_copy", "a write to a loop variable, which is a copy"),
    ("variant_shape", "a variant built with the wrong fields"),
    ("unused_value", "an expression statement whose value is dropped"),
    ("unknown_type", "a type name nothing declares"),
    ("unknown_state", "a state name the machine does not declare"),
    ("unknown_handler", "a handler name nothing defines"),
    ("unknown_handler_property", "an unknown handler annotation"),
    ("unknown_checker_predicate", "a checker rule names a predicate nothing implements"),
    ("undefined_delegate", "a route delegates to a handler that does not exist"),
    ("transition_no_machine", "transition() in a program without a state machine"),
    ("transition_effect", "an effect block on a transition does something it may not"),
    ("test_machine_ambiguous", "a test rule calls transition() with several machines in scope"),
    ("test_helper_shadows", "a test helper named like a program handler or a builtin"),
    ("task_without_think", "a [task] handler that never calls think()"),
    ("task_try_write", "a [task] handler writes inside a try that spans a think()"),
    ("task_transition_before_think", "a [task] handler transitions before its last think()"),
    ("task_stale_read", "a [task] handler uses a value read before a think()"),
    ("task_native", "[task] and [native] on one handler"),
    ("task_ensure", "`ensure` in a [task] handler undoes the current step only"),
    ("task_called_atomically", "a [task] handler called from a plain handler"),
    ("state_annotation", "an unknown annotation on a state or transition"),
    ("slot_assignment", "`slot = value` — a slot is written through its methods"),
    ("scale_memory", "the scale section contradicts the memory section"),
    ("scalar_slot", "a memory slot that is not a Map or a List"),
    ("rules_outside_test", "a `rules` block outside a `cell test`"),
    ("return_in_lambda", "`return` inside a lambda"),
    ("reserved_handler", "a handler named like a reserved builtin"),
    ("request_signature", "`on request` with a signature serve cannot call"),
    ("refusal_discarded", "a refusal() value that is not returned"),
    ("quadratic_concat", "a list or string grown by concatenation in a loop"),
    ("pipe_right_side", "the right side of `|>` is not a call"),
    ("param_shadows_slot", "a parameter named like a memory slot"),
    ("no_initial_state", "a state machine without `initial:`"),
    ("nested_quote", "an unescaped quote inside an interpolation"),
    ("native_primitive_shadowed", "a handler named like a native primitive"),
    ("multiple_state_machines", "more than one state machine in a cell"),
    ("mock_target", "a `mock` naming no handler"),
    ("local_shadows_slot", "a `let` named like a memory slot"),
    ("lambda_scope", "a lambda reads a name that is not in scope where it runs"),
    ("lambda_assign", "a lambda assigns an outer local"),
    ("invariant_not_bool", "an invariant that is not a condition"),
    ("interpolation_trailing", "text after the expression of an interpolation"),
    ("interpolation_malformed", "an interpolation that does not parse"),
    ("interpolation_format_spec", "a format spec in an interpolation (use format())"),
    ("interior_timer", "`every` / `after` in an interior cell"),
    ("interior_call", "a call into an interior cell from outside its parent"),
    ("immutable_write", "an overwrite or delete on an [immutable] slot"),
    ("guard_effect", "a guard that calls a handler, think() or writes a slot"),
    ("foreign_idiom", "syntax carried over from another language"),
    ("face_mismatch", "a handler and its face signal disagree"),
    ("every_zero", "`every 0` — an interval must be positive"),
    ("ensure_after_return", "`ensure` after a `return` never runs"),
    ("empty_test_cell", "a test cell that asserts nothing"),
    ("empty_program", "a file with no cell"),
    ("empty_match", "a `match` with no arm"),
    ("duplicate_edge", "the same transition declared twice"),
    ("count_field_default", "`.len` / `.size` read as a field with a default"),
    ("cost_unprovable", "a declared `cost` bound that cannot be proven"),
    ("builtin_arity", "a builtin called with too few or too many arguments"),
    ("budget_annotation", "a malformed budget annotation"),
    ("break_outside_loop", "`break` / `continue` outside a loop"),
    ("assignment_without_let", "an assignment to a name that has no `let`"),
    ("argument_type", "an argument whose type differs from the parameter's"),
    ("argument_count", "a handler called with the wrong number of arguments"),
    ("ambiguous_slot", "a slot name two cells declare"),
    // 3.0.0
    ("syntax", "a syntax error"),
    ("type_mismatch", "an expression whose inferred type cannot be used where it stands"),
    ("unknown_field", "a field the record type does not declare"),
    ("maybe_absent", "a value that may be absent `()` used where a value is required"),
    ("edition", "an unknown `edition` in soma.toml"),
    ("untrusted_value", "a model's answer or an HTTP reply decides a state, a URL, a file or a budget without a check"),
    ("load", "the program does not load: an import, a manifest or a file it needs"),
    ("io", "a file the command was given cannot be read"),
    ("contract", "a face contract (`requires` / `ensures`) says something a caller could not evaluate, or sits on a handler that cannot carry one"),
    ("await", "an `await` stands where its handler could not be stopped and resumed: outside a `[task]` handler, or not as a whole statement of its body or of an `if` branch there"),
    ("tool_binding", "a `tool` bound to a server (`[mcp: …]` / `[openapi: …]`) that soma.toml does not define, soma.lock does not pin, or whose declaration does not fit the pinned contract"),
    ("flesh", "a `flesh` body that cannot go into a skeleton: no `skeleton` of that name, a second body for one signal, parameters that differ from the signal's, or a helper with an untyped parameter"),
    ("frame", "a skeleton's frame that names something the skeleton does not have, or a flesh that does something its signal's frame does not grant (a write, a read, a call, an emit or a machine edge)"),
    ("skeleton_lock", "a skeleton differs from the one `skeleton.lock` holds (frozen with `soma skeleton --freeze`)"),
];

/// Warning classes (the `CheckWarning` variant, snake_case). APPEND ONLY.
const WARNING_KINDS: &[(&str, &str)] = &[
    ("unhandled_signal", "an emitted signal no cell handles"),
    ("property_implication", "a slot property implies another that is missing"),
    ("unknown_property", "a slot property nothing defines"),
    ("unknown_property_ref", "a `cell property` refers to an unknown property"),
    ("unverifiable_promise", "a descriptive promise the compiler cannot check"),
    ("await_without_handler", "an `await` with no handler for its signal"),
    ("scale_eventual_consistency", "eventual consistency on a slot that reads its own writes"),
    ("agent_missing_state_machine", "an agent cell without a state machine"),
    ("cost_advisory", "a cost bound that is an estimate, not a proof"),
    ("cost_proven", "a cost bound that is proven (a note)"),
    ("dispatch_shadow", "a bare call that two definitions could answer"),
    ("native_semantics", "native code that would differ from the interpreter"),
    ("unreachable", "code or states that cannot be reached"),
    ("habit_warning", "an idiom that probably does not mean what it does elsewhere"),
    ("interpolation_recoverable", "an undefined interpolation variable inside try / assert_fails"),
    ("budget_advisory", "a memory budget whose proof is incomplete"),
    ("budget_ok", "a memory budget that holds (a note)"),
    // 3.0.0
    ("type_advisory", "a type the inference could not settle; checked at run time"),
    // 3.1.0
    ("frame_warning", "a skeleton that says more than the program does: a frame wider than its body, a machine edge or a slot no frame touches, a helper nothing calls"),
];

/// Syntax-error classes, recognised from the parser's message. APPEND ONLY.
const SYNTAX_KINDS: &[(&str, &str)] = &[
    ("expected", "the parser expected one token and found another"),
    ("unexpected_eof", "the file ends inside a construct"),
    ("handler_return_type", "a return type on a handler — it goes on the face signal"),
    ("match_fat_arrow", "`=>` in a match arm — arms use `->`"),
    ("reserved_word", "a keyword used as a name"),
    ("nesting_too_deep", "expressions or blocks nested too deeply"),
    ("cell_constant", "a `let` / `const` directly inside a cell"),
    ("statement_as_value", "a statement where a value is expected"),
    ("fix_it", "a near-miss the parser knows the correction for"),
    ("lexical", "a character, string or number the lexer cannot read"),
];

fn lookup(table: &[(&str, &str)], kind: &str) -> Option<usize> {
    table.iter().position(|(k, _)| *k == kind)
}

/// `E0001`… for a check-error kind; `E0000` for a kind without a code (a
/// test refuses those: every new kind is appended to ERROR_KINDS).
pub fn error_code(kind: &str) -> String {
    match lookup(ERROR_KINDS, kind) {
        Some(i) => format!("E{:04}", i + 1),
        None => "E0000".to_string(),
    }
}

pub fn warning_code(kind: &str) -> String {
    match lookup(WARNING_KINDS, kind) {
        Some(i) => format!("W{:04}", i + 1),
        None => "W0000".to_string(),
    }
}

fn syntax_kind(message: &str) -> &'static str {
    if message.starts_with("handlers do not declare return types") { "handler_return_type" }
    else if message.starts_with("match arms use '->'") { "match_fat_arrow" }
    else if message.contains("is a reserved word in Soma") { "reserved_word" }
    else if message.contains("starts a statement and cannot stand where a value is expected") { "statement_as_value" }
    else if message.contains("nested more than") || message.contains("nested too deep") { "nesting_too_deep" }
    else if message.contains("a cell has no constants") { "cell_constant" }
    else if message.starts_with("unexpected end of input") { "unexpected_eof" }
    else if message.starts_with("expected ") { "expected" }
    else { "fix_it" }
}

/// `E1001`… for a parser message.
pub fn syntax_code(message: &str) -> String {
    format!("E{:04}", 1001 + lookup(SYNTAX_KINDS, syntax_kind(message)).unwrap_or(8))
}

/// The code of a lexer error.
pub fn lexical_code() -> String {
    format!("E{:04}", 1001 + lookup(SYNTAX_KINDS, "lexical").unwrap_or(9))
}

/// `soma explain CODE`: (kind, one-line meaning).
pub fn explain(code: &str) -> Option<(&'static str, &'static str)> {
    let code = code.trim();
    let (prefix, digits) = code.split_at(code.len().min(1));
    let n: usize = digits.parse().ok()?;
    let (table, base) = match (prefix, n) {
        ("E" | "e", 1001..) => (SYNTAX_KINDS, 1001),
        ("E" | "e", _) => (ERROR_KINDS, 1),
        ("W" | "w", _) => (WARNING_KINDS, 1),
        _ => return None,
    };
    let (k, d) = table.get(n.checked_sub(base)?)?;
    Some((k, d))
}

/// Every (code, kind, meaning), for `soma explain --list` and the docs.
pub fn all() -> Vec<(String, &'static str, &'static str)> {
    let mut out = Vec::new();
    for (i, (k, d)) in ERROR_KINDS.iter().enumerate() { out.push((format!("E{:04}", i + 1), *k, *d)); }
    for (i, (k, d)) in SYNTAX_KINDS.iter().enumerate() { out.push((format!("E{:04}", 1001 + i), *k, *d)); }
    for (i, (k, d)) in WARNING_KINDS.iter().enumerate() { out.push((format!("W{:04}", i + 1), *k, *d)); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_explained() {
        let all = all();
        let mut seen = std::collections::HashSet::new();
        for (code, kind, meaning) in &all {
            assert!(seen.insert(code.clone()), "duplicate code {code}");
            assert!(!meaning.is_empty(), "{kind} has no meaning");
            assert_eq!(explain(code).map(|(k, _)| k), Some(*kind));
        }
        for table in [ERROR_KINDS, WARNING_KINDS, SYNTAX_KINDS] {
            let mut kinds = std::collections::HashSet::new();
            for (k, _) in table { assert!(kinds.insert(*k), "duplicate kind {k}"); }
        }
    }

    /// A diagnostic kind written anywhere in the checker has a code: adding
    /// `kind: "new_thing"` without appending it to ERROR_KINDS fails here.
    #[test]
    fn every_kind_in_the_checker_has_a_code() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/checker");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().map_or(true, |n| n == "codes.rs") { continue; }
            let text = std::fs::read_to_string(&path).unwrap();
            for part in text.split("kind: \"").skip(1) {
                let kind: String = part.chars().take_while(|c| c.is_ascii_lowercase() || *c == '_').collect();
                if kind.is_empty() || !part[kind.len()..].starts_with('"') { continue; }
                assert_ne!(error_code(&kind), "E0000", "kind \"{kind}\" ({}) has no code — append it to ERROR_KINDS", path.display());
                checked += 1;
            }
        }
        assert!(checked > 100, "the scan found only {checked} kinds");
    }

    /// The first codes are published: their numbers are frozen.
    #[test]
    fn published_codes_do_not_move() {
        assert_eq!(error_code("property_contradiction"), "E0001");
        assert_eq!(error_code("dispatch"), "E0012");
        assert_eq!(error_code("undefined_variable"), "E0028");
        assert_eq!(syntax_code("match arms use '->', not '=>'"), "E1004");
        assert_eq!(warning_code("habit_warning"), "W0014");
    }
}
