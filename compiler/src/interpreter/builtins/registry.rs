//! Builtin registry — the single source of truth for every builtin the
//! interpreter dispatches anywhere (the match arms in `builtins/*.rs`, the
//! direct dispatches in `interpreter/mod.rs`, and the lambda builtins).
//!
//! Three consumers:
//!   - `soma describe --builtins [--json]` — agent-queryable signature table
//!   - `soma docs builtins`                — generated SOMA_BUILTINS.md
//!   - `record_log::NONDET_BUILTINS`      — derived from `deterministic: false`
//!
//! Adding a builtin? Add its match arm AND a `BuiltinDoc` entry here.
//! The unit tests below enforce no duplicate names and that the replay
//! nondeterminism set stays in sync.

/// One documented builtin. `signature` is written in Soma surface syntax;
/// `deterministic: false` means every call is a potential source of replay
/// divergence (the set feeds `record_log::NONDET_BUILTINS`).
pub struct BuiltinDoc {
    pub name: &'static str,
    pub category: &'static str,
    pub signature: &'static str,
    pub brief: &'static str,
    pub deterministic: bool,
}

/// Shorthand for the common (deterministic) case.
const fn doc(name: &'static str, category: &'static str, signature: &'static str, brief: &'static str) -> BuiltinDoc {
    BuiltinDoc { name, category, signature, brief, deterministic: true }
}

/// Shorthand for the nondeterministic case (replay-divergence tracked).
const fn nondet(name: &'static str, category: &'static str, signature: &'static str, brief: &'static str) -> BuiltinDoc {
    BuiltinDoc { name, category, signature, brief, deterministic: false }
}

/// Display order for categories in `describe --builtins` / `docs builtins`.
pub const CATEGORY_ORDER: &[&str] = &[
    "string", "types", "math", "collection", "pipeline", "lambda",
    "io", "template", "web", "http", "time", "state", "memory",
    "agent", "linalg", "internal", "reserved",
];

pub static BUILTINS: &[BuiltinDoc] = &[
    // ── string ──────────────────────────────────────────────────────
    doc("concat", "string", "concat(a, b) -> String",
        "Concatenate two values as a string; with one arg, returns it unchanged."),
    doc("split", "string", "split(s: String, delim: String) -> List<String>",
        "Split a string on a delimiter into a list of substrings."),
    doc("replace", "string", "replace(s: String, old: String, new: String) -> String",
        "Replace every occurrence of `old` with `new`."),
    doc("contains", "string", "contains(haystack: String, needle: String) -> Bool",
        "True if `needle` occurs anywhere in `haystack`."),
    doc("starts_with", "string", "starts_with(s: String, prefix: String) -> Bool",
        "True if `s` begins with `prefix`."),
    doc("ends_with", "string", "ends_with(s: String, suffix: String) -> Bool",
        "True if `s` ends with `suffix`."),
    doc("lowercase", "string", "lowercase(s: String) -> String",
        "Lowercase the string (non-strings are stringified first)."),
    doc("uppercase", "string", "uppercase(s: String) -> String",
        "Uppercase the string (non-strings are stringified first)."),
    doc("trim", "string", "trim(s: String) -> String",
        "Strip leading and trailing whitespace."),
    doc("index_of", "string", "index_of(s: String, sub: String) -> Int",
        "Character index of the first occurrence of `sub`, or -1 if absent."),
    doc("substring", "string", "substring(s: String, start: Int, end: Int) -> String",
        "Character-based slice [start, end) — end is exclusive and clamped."),
    doc("escape_html", "string", "escape_html(s: String) -> String",
        "Escape &, <, >, double and single quotes for safe HTML embedding."),
    doc("str_len", "string", "str_len(s: String) -> Int",
        "Byte length of a string (cf. len(), which counts characters)."),
    doc("str_at", "string", "str_at(s: String, i: Int) -> Int",
        "Byte value at index `i`; errors if out of range."),
    doc("str_eq", "string", "str_eq(a: String, b: String) -> Bool",
        "Exact string equality (fast path for [native] code)."),

    // ── types / conversion ──────────────────────────────────────────
    doc("len", "types", "len(x: String|List|Map) -> Int",
        "Characters of a string, elements of a list, or entries of a map."),
    doc("to_string", "types", "to_string(x) -> String",
        "Render any value with its display formatting."),
    doc("to_int", "types", "to_int(x) -> Int",
        "Convert to Int (floats truncate, strings parse, BigInt-exact); returns () on failure."),
    doc("to_float", "types", "to_float(x) -> Float",
        "Convert to Float; returns () if a string fails to parse."),
    doc("to_json", "types", "to_json(x) -> String",
        "Serialize a value as JSON (strings escaped, NaN/inf become null)."),
    doc("from_json", "types", "from_json(s: String) -> Any",
        "Parse a JSON string into a Map/List/scalar; maps and lists pass through."),
    doc("type_of", "types", "type_of(x) -> String",
        "Type name: \"Int\", \"BigInt\", \"Float\", \"String\", \"Bool\", \"List\", \"Map\", \"Lambda\", \"Variant\", or \"Unit\"."),
    doc("is_type", "types", "is_type(value: Map, type_name: String) -> Bool",
        "True if a record's `_type` field equals `type_name`."),
    doc("is_a", "types", "is_a(value: Map, type_name: String) -> Bool",
        "Alias of is_type."),

    // ── math ────────────────────────────────────────────────────────
    doc("abs", "math", "abs(x: Int|Float) -> Int|Float",
        "Absolute value; errors on i64::MIN overflow."),
    doc("round", "math", "round(x: Float) -> Int",
        "Round half away from zero to the nearest integer."),
    doc("floor", "math", "floor(x: Float) -> Int",
        "Largest integer <= x."),
    doc("ceil", "math", "ceil(x: Float) -> Int",
        "Smallest integer >= x."),
    doc("sqrt", "math", "sqrt(x: Int|Float) -> Float",
        "Square root."),
    doc("log", "math", "log(x: Int|Float) -> Float",
        "Natural logarithm."),
    doc("ln", "math", "ln(x: Int|Float) -> Float",
        "Alias of log (natural logarithm)."),
    doc("exp", "math", "exp(x: Int|Float) -> Float",
        "e raised to the power x."),
    doc("log10", "math", "log10(x: Int|Float) -> Float",
        "Base-10 logarithm."),
    doc("pow", "math", "pow(base: Int|Float, exp: Int|Float) -> Float",
        "base raised to exp (always a Float)."),
    doc("min", "math", "min(a, b) -> Int|Float",
        "Smaller of two numbers (Float if either is a Float)."),
    doc("max", "math", "max(a, b) -> Int|Float",
        "Larger of two numbers (Float if either is a Float)."),
    doc("idiv", "math", "idiv(a: Int, b: Int) -> Int",
        "Integer division truncating toward zero; errors on division by zero."),
    doc("clamp", "math", "clamp(v, lo, hi) -> Int|Float",
        "Constrain v to [lo, hi]; errors if lo > hi."),
    nondet("random", "math", "random() -> Float | random(max: Int) -> Int | random(min: Int, max: Int) -> Int",
        "Time-seeded PRNG: float in [0,1), or int in [0,max) / [min,max)."),
    doc("gcd", "math", "gcd(a: Int, b: Int) -> Int",
        "Greatest common divisor (Euclid, absolute values)."),
    doc("sqrt_int", "math", "sqrt_int(n: Int) -> Int",
        "Integer square root; errors on negative input."),
    doc("pow_mod", "math", "pow_mod(base: Int, exp: Int, m: Int) -> Int",
        "Modular exponentiation base^exp mod m; errors if m is zero."),
    doc("band", "math", "band(a: Int, b: Int) -> Int",
        "Bitwise AND."),
    doc("bor", "math", "bor(a: Int, b: Int) -> Int",
        "Bitwise OR."),
    doc("bxor", "math", "bxor(a: Int, b: Int) -> Int",
        "Bitwise XOR."),
    doc("bnot", "math", "bnot(a: Int) -> Int",
        "Bitwise NOT."),
    doc("shl", "math", "shl(a: Int, n: Int) -> Int",
        "Shift left by n bits (wrapping)."),
    doc("shr", "math", "shr(a: Int, n: Int) -> Int",
        "Arithmetic shift right by n bits (wrapping)."),
    doc("bit_test", "math", "bit_test(a: Int, i: Int) -> Int",
        "1 if bit i of a is set, else 0."),
    doc("bit_set", "math", "bit_set(a: Int, i: Int) -> Int",
        "a with bit i set."),
    doc("bit_clr", "math", "bit_clr(a: Int, i: Int) -> Int",
        "a with bit i cleared."),
    doc("bit_next", "math", "bit_next(a: Int, i: Int) -> Int",
        "Index of the lowest set bit at or above i, or -1 if none."),
    doc("bit_len", "math", "bit_len(a: Int) -> Int",
        "Number of significant bits (estimated for BigInt)."),

    // ── collection ──────────────────────────────────────────────────
    doc("list", "collection", "list(items...) -> List",
        "Build a list; list(existing_list, more...) appends to a copy."),
    doc("map", "collection", "map(key, value, ...) -> Map | list |> map(x => expr) -> List",
        "Build a map from key-value pairs (even arg count), or — with a lambda — transform each list element."),
    doc("push", "collection", "push(list: List, items...) -> List",
        "Return a new list with the items appended (the original is unchanged)."),
    doc("nth", "collection", "nth(list: List, i: Int) -> Any",
        "Element at index i, or () when out of bounds."),
    doc("reverse", "collection", "reverse(list: List) -> List",
        "Return the list in reverse order."),
    doc("range", "collection", "range(start: Int, end: Int) -> List<Int>",
        "Integers from start up to but excluding end."),
    doc("sort", "collection", "sort(list: List, order?: \"desc\") -> List",
        "Sort scalars ascending (or \"desc\"); errors on incomparable element types."),
    doc("flatten", "collection", "flatten(list: List) -> List",
        "Flatten one level of nested lists."),
    doc("zip", "collection", "zip(a: List, b: List) -> List<{left, right}>",
        "Pair elements positionally; stops at the shorter list."),
    doc("enumerate", "collection", "enumerate(list: List) -> List<{index, value}>",
        "Attach a 0-based index to each element."),
    doc("with", "collection", "with(m: Map, key, value, ...) -> Map",
        "Return a copy of the map with the given key-value pairs inserted."),
    doc("without", "collection", "without(m: Map, keys...) -> Map",
        "Return a copy of the map with the given keys removed."),
    doc("merge", "collection", "merge(a: Map, b: Map) -> Map",
        "Copy of `a` with all entries of `b` inserted (b wins on conflict)."),
    doc("join", "collection", "join(list: List, sep: String) -> String | join(left: List, right: List, key) -> List",
        "Join list elements into a string — or, with two lists, an inner data join on `key`."),

    // ── pipeline ────────────────────────────────────────────────────
    doc("filter_by", "pipeline", "filter_by(rows: List<Map>, field, op: \">\"|\">=\"|\"<\"|\"<=\"|\"==\"|\"!=\", value) -> List<Map>",
        "Keep rows whose `field` compares true against `value` (op defaults to == with 3 args)."),
    doc("sort_by", "pipeline", "sort_by(rows: List<Map>, field, order?: \"desc\") -> List<Map>",
        "Sort rows by a numeric field, ascending unless \"desc\"."),
    doc("top", "pipeline", "top(rows: List, n: Int) -> List",
        "First n elements."),
    doc("bottom", "pipeline", "bottom(rows: List, n: Int) -> List",
        "Last n elements."),
    doc("sum_by", "pipeline", "sum_by(rows: List<Map>, field) -> Int",
        "Sum of a field across rows (integer arithmetic)."),
    doc("avg_by", "pipeline", "avg_by(rows: List<Map>, field) -> Int|Float",
        "Mean of a field; Int when whole, () on an empty list."),
    doc("min_by", "pipeline", "min_by(rows: List<Map>, field) -> Map",
        "Row with the smallest integer value of `field`, or ()."),
    doc("max_by", "pipeline", "max_by(rows: List<Map>, field) -> Map",
        "Row with the largest integer value of `field`, or ()."),
    doc("pluck", "pipeline", "pluck(rows: List<Map>, field) -> List",
        "Extract one field from every row (missing fields become ())."),
    doc("group_by", "pipeline", "group_by(rows: List<Map>, field) -> Map<String, List>",
        "Group rows into a map keyed by the field's stringified value."),
    doc("distinct", "pipeline", "distinct(rows: List, field?) -> List",
        "Unique elements — or, with `field`, the unique values of that field."),
    doc("count_by", "pipeline", "count_by(rows: List<Map>, field, value) -> Int",
        "Number of rows whose `field` stringifies equal to `value`."),
    doc("select", "pipeline", "select(rows: List<Map>, fields...) -> List<Map>",
        "Project each row down to the named fields."),
    doc("agg", "pipeline", "agg(rows: List<Map>, group_field, \"col:func\"...) -> List<Map>",
        "Group + aggregate: func is sum|avg|min|max|count; every group also gets a `count`."),
    doc("inner_join", "pipeline", "inner_join(left: List<Map>, right: List<Map>, key) -> List<Map>",
        "Merge rows whose `key` matches in both lists (left fields win)."),
    doc("left_join", "pipeline", "left_join(left: List<Map>, right: List<Map>, key) -> List<Map>",
        "Keep every left row, merging matching right-row fields when found."),

    // ── lambda (higher-order) ───────────────────────────────────────
    doc("filter", "lambda", "filter(list: List, x => Bool) -> List",
        "Keep elements where the lambda returns truthy."),
    doc("find", "lambda", "find(list: List, x => Bool) -> Any",
        "First element where the lambda is truthy, or ()."),
    doc("any", "lambda", "any(list: List, x => Bool) -> Bool",
        "True if the lambda is truthy for at least one element."),
    doc("all", "lambda", "all(list: List, x => Bool) -> Bool",
        "True if the lambda is truthy for every element (true on empty)."),
    doc("count", "lambda", "count(list: List, x => Bool) -> Int",
        "Number of elements where the lambda is truthy."),
    doc("reduce", "lambda", "reduce(list: List, initial, p => expr) -> Any",
        "Fold the list; the lambda receives {acc, val} and returns the next acc."),

    // ── io ──────────────────────────────────────────────────────────
    doc("print", "io", "print(args...) -> ()",
        "Print arguments space-separated, then a newline."),
    doc("read_file", "io", "read_file(path: String) -> String | {error}",
        "Read a file as a string; returns {error: ...} on failure."),
    doc("write_file", "io", "write_file(path: String, content) -> Bool | {error}",
        "Write content (stringified) to a file; true on success."),
    doc("read_csv", "io", "read_csv(path: String) -> List<Map>",
        "Parse a CSV with header row into maps; cells auto-typed to Int/Float/String."),
    doc("write_csv", "io", "write_csv(path: String, rows: List<Map>) -> Bool | {error}",
        "Write rows as CSV using the first row's keys as the header."),
    doc("read_files", "io", "read_files(dir: String, count: Int) -> List<{path, content}>",
        "Read up to `count` files from a directory."),
    doc("par_read_files", "io", "par_read_files(dir: String, count: Int) -> List<{path, content}>",
        "Thread-parallel variant of read_files."),
    doc("word_count", "io", "word_count(text: String | docs: List) -> Map<String, Int>",
        "Lowercased word frequency of a string or of {content} docs (Rust-speed)."),
    doc("par_word_count", "io", "par_word_count(docs: List) -> Map<String, Int>",
        "Thread-parallel variant of word_count over a list."),

    // ── template ────────────────────────────────────────────────────
    doc("load_template", "template", "load_template(path: String, key, value, ...) -> String",
        "Read a file and substitute each {key} placeholder with its value."),
    doc("load", "template", "load(path: String, key, value, ...) -> String",
        "Alias of load_template."),
    doc("include", "template", "include(path: String, key, value, ...) -> String",
        "Alias of load_template."),
    doc("render", "template", "render(template: String, key, value, ...) -> String",
        "Substitute {key} placeholders in an in-memory template string."),
    doc("render_each", "template", "render_each(rows: List<Map>, template: String) -> String",
        "Render the template once per row, substituting {field} from each map."),

    // ── web (HTTP responses) ────────────────────────────────────────
    doc("html", "web", "html(body) -> Response | html(status: Int, body) -> Response",
        "text/html response; auto-injects HTMX on full pages that use hx- attributes."),
    doc("response", "web", "response(status: Int, body, header_key, header_value, ...) -> Response",
        "Response with explicit status, body, and optional headers."),
    doc("redirect", "web", "redirect(url: String) -> Response",
        "302 redirect to `url`."),
    doc("sse", "web", "sse(streams...) -> Response",
        "Open a Server-Sent-Events connection subscribed to the named streams."),
    doc("publish", "web", "publish(stream: String, data) -> ()",
        "Push data to a runtime-chosen SSE stream name on the event bus."),

    // ── http (outbound) ─────────────────────────────────────────────
    doc("http_get", "http", "http_get(url: String, opts?: {max_bytes, timeout}) -> Map|String",
        "GET a URL; JSON bodies parse to a Map/List, errors return {error}."),
    doc("http_post", "http", "http_post(url: String, body) -> Map|String",
        "POST a JSON body to a URL; JSON responses parse to a Map/List."),
    doc("ws_connect", "http", "ws_connect(url: String) -> Map",
        "Open a WebSocket connection; incoming messages dispatch as signals."),
    doc("ws_send", "http", "ws_send(msg) -> ()",
        "Send a message on the current WebSocket connection; errors if not connected."),
    doc("link", "http", "link(addr: \"host:port\") -> ()",
        "Open a TCP signal-bus link to a peer node."),
    doc("subscribe", "http", "subscribe(url: String) -> ()",
        "Subscribe to a remote event stream; events dispatch as signals."),

    // ── time ────────────────────────────────────────────────────────
    nondet("now", "time", "now() -> Int",
        "Current Unix timestamp in seconds."),
    nondet("now_ms", "time", "now_ms() -> Int",
        "Current Unix timestamp in milliseconds."),
    nondet("today", "time", "today() -> String",
        "Today's date as \"YYYY-MM-DD\" (UTC)."),
    doc("format_date", "time", "format_date(ts: Int) -> String",
        "Format a Unix-seconds timestamp as \"YYYY-MM-DD\" (UTC)."),
    doc("sleep", "time", "sleep(ms: Int) -> ()",
        "Block the current handler for `ms` milliseconds."),

    // ── state machines ──────────────────────────────────────────────
    doc("next_id", "state", "next_id() -> Int",
        "Monotonic per-cell counter backed by storage; starts at 1."),
    doc("transition", "state", "transition(id, target_state: String) -> String",
        "Move instance `id` to `target_state`; errors with the valid targets on an invalid move."),
    doc("get_status", "state", "get_status(id) -> String",
        "Current state of instance `id` (initial state if never transitioned)."),
    doc("valid_transitions", "state", "valid_transitions(id) -> List<String>",
        "States reachable from instance `id`'s current state."),

    // ── memory ──────────────────────────────────────────────────────
    doc("remember", "memory", "remember(key, value) -> ()",
        "Persist a value in the cell's agent memory slot."),
    doc("recall", "memory", "recall(key: String) -> Any",
        "Fetch a remembered value from any storage slot, or ()."),
    doc("append", "memory", "slot.append(value) -> ()",
        "Memory-slot method: append a value to a list-backed slot (alias: slot.push)."),

    // ── agent ───────────────────────────────────────────────────────
    doc("think", "agent", "think(prompt: String, system?: String, opts?: {max_tokens, timeout}) -> String",
        "Call the configured LLM with tool-calling, multi-turn context, and budget enforcement."),
    doc("think_json", "agent", "think_json(prompt: String, system?: String, opts?: {max_tokens, timeout}) -> Map",
        "Like think(), but parses the response as JSON into a Map."),
    doc("delegate", "agent", "delegate(cell: String, signal: String, args...) -> Any",
        "Invoke another cell's handler and return its result."),
    doc("set_budget", "agent", "set_budget(max_tokens: Int) -> ()",
        "Hard cap on LLM tokens; think() fails once exhausted."),
    doc("tokens_used", "agent", "tokens_used() -> Int",
        "LLM tokens consumed since the budget was set."),
    doc("tokens_remaining", "agent", "tokens_remaining() -> Int",
        "Tokens left in the budget, or -1 if unlimited."),
    doc("trace", "agent", "trace() -> List",
        "Structured execution log: every think(), tool call, and approval."),
    doc("clear_trace", "agent", "clear_trace() -> ()",
        "Empty the agent trace log."),
    doc("clear_context", "agent", "clear_context() -> ()",
        "Reset the multi-turn LLM conversation history."),
    doc("approve", "agent", "approve(action: String) -> Bool",
        "Human-in-the-loop gate; interactive in serve mode, auto-approved in run mode."),

    // ── linalg / risk ───────────────────────────────────────────────
    doc("matrix", "linalg", "matrix(\"1 2; 3 4\") -> List<List<Float>>",
        "MATLAB-style matrix literal: ';' separates rows, whitespace/',' separates entries."),
    doc("mat", "linalg", "mat(rows: Int, cols: Int, values: List<Float>) -> List<List<Float>>",
        "Reshape a flat list into an r×c matrix; errors if the count mismatches."),
    doc("rows", "linalg", "rows(r1: List<Float>, r2: List<Float>, ...) -> List<List<Float>>",
        "Build a matrix from row vectors."),
    doc("cols", "linalg", "cols(c1: List<Float>, c2: List<Float>, ...) -> List<List<Float>>",
        "Build a matrix from column vectors (transposes)."),
    doc("eye", "linalg", "eye(n: Int) -> List<List<Float>>",
        "n×n identity matrix."),
    doc("zeros", "linalg", "zeros(r: Int, c: Int) -> List<List<Float>>",
        "r×c matrix of zeros."),
    doc("ones", "linalg", "ones(r: Int, c: Int) -> List<List<Float>>",
        "r×c matrix of ones."),
    doc("diag", "linalg", "diag(values: List<Float>) -> List<List<Float>>",
        "Square diagonal matrix from a list."),
    doc("to_sampled", "linalg", "to_sampled(A: List<List<Float>>, opts?: {max_rows, max_cols}) -> Map",
        "Build a BST-backed length-squared sampling handle (Tang); O(log n) per sample after."),
    doc("sample_row", "linalg", "sample_row(A) -> Map",
        "Draw one row index by ℓ²-norm importance sampling (time-seeded PRNG)."),
    doc("drop_sampled", "linalg", "drop_sampled(handle: Map) -> Bool",
        "Free a to_sampled() registry entry; true if it existed."),
    doc("importance_sample_rows", "linalg", "importance_sample_rows(A, opts: {samples}) -> Map",
        "Sample rows by squared-norm importance (time-seeded PRNG)."),
    doc("svd_lowrank", "linalg", "svd_lowrank(A, opts: {row_samples, col_samples, rank, max_dim}) -> Map",
        "Sublinear randomized low-rank SVD with declared sampling bounds."),
    doc("regress_sgd", "linalg", "regress_sgd(A, b: List<Float>, opts: {eps, lambda, max_iter, max_dim}) -> Map",
        "Ridge regression via stochastic gradient descent with declared bounds."),
    doc("clean_covariance", "linalg", "clean_covariance(returns: List<List<Float>>, opts: {method: \"rie\"|\"clip\"|\"raw\", eta, center, max_assets, max_obs}) -> Map",
        "RMT (Bouchaud-Potters) covariance cleaning; .matrix is the cleaned N×N."),
    doc("impact_sqrt", "linalg", "impact_sqrt(qty: Float, daily_volume: Float, sigma: Float, opts?: {Y}) -> Map",
        "Bouchaud square-root market-impact law; .bps is expected slippage."),
    doc("quantile", "linalg", "quantile(values: List<Float>, q: Float) -> Float",
        "Empirical q-quantile of a sample."),
    doc("var_historical", "linalg", "var_historical(returns: List<Float>, opts?: {alpha, max_obs}) -> Float",
        "Historical Value-at-Risk — no distributional assumption."),
    doc("expected_shortfall_historical", "linalg", "expected_shortfall_historical(returns: List<Float>, opts?: {alpha, max_obs}) -> Float",
        "Historical expected shortfall (CVaR) beyond the VaR threshold."),
    doc("var_gaussian", "linalg", "var_gaussian(returns: List<Float>, opts?: {alpha, mu, sigma}) -> Float",
        "Gaussian VaR assuming N(mu, sigma^2); moments inferred unless overridden."),

    // ── internal ────────────────────────────────────────────────────
    doc("_coalesce", "internal", "_coalesce(a, b) -> Any",
        "Desugared form of `a ?? b`: returns b only when a is ()."),

    // ── reserved (replay nondeterminism guards) ─────────────────────
    nondet("timestamp", "reserved", "timestamp() -> Int",
        "Reserved nondeterministic name — tracked for replay divergence; not currently dispatched."),
    nondet("date_now", "reserved", "date_now() -> String",
        "Reserved nondeterministic name — tracked for replay divergence; not currently dispatched."),
    nondet("rand", "reserved", "rand() -> Float",
        "Reserved nondeterministic name — tracked for replay divergence; not currently dispatched."),
];

/// Look up a builtin by name.
pub fn lookup(name: &str) -> Option<&'static BuiltinDoc> {
    BUILTINS.iter().find(|b| b.name == name)
}

/// All categories, in display order, with any stragglers appended.
pub fn categories() -> Vec<&'static str> {
    let mut cats: Vec<&'static str> = CATEGORY_ORDER.to_vec();
    for b in BUILTINS {
        if !cats.contains(&b.category) {
            cats.push(b.category);
        }
    }
    cats.retain(|c| BUILTINS.iter().any(|b| b.category == *c));
    cats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_no_duplicate_names() {
        let mut seen = std::collections::HashSet::new();
        for b in BUILTINS {
            assert!(seen.insert(b.name), "duplicate builtin in registry: {}", b.name);
        }
    }

    #[test]
    fn nondet_list_matches_registry() {
        // The historical replay nondeterminism set, verbatim. The derived
        // NONDET_BUILTINS must preserve it exactly — changing it changes
        // replay divergence detection.
        let expected: &[&str] = &[
            "now", "now_ms", "timestamp", "today", "date_now",
            "random", "rand",
        ];
        for name in expected {
            let doc = lookup(name)
                .unwrap_or_else(|| panic!("nondet builtin '{}' missing from registry", name));
            assert!(!doc.deterministic, "'{}' must be deterministic: false", name);
        }
        let derived: Vec<&str> = crate::interpreter::record_log::NONDET_BUILTINS
            .iter().copied().collect();
        let mut derived_sorted = derived.clone();
        derived_sorted.sort_unstable();
        let mut expected_sorted = expected.to_vec();
        expected_sorted.sort_unstable();
        assert_eq!(derived_sorted, expected_sorted,
            "derived NONDET_BUILTINS diverged from the historical set");
    }

    #[test]
    fn every_category_is_ordered() {
        for b in BUILTINS {
            assert!(CATEGORY_ORDER.contains(&b.category),
                "builtin '{}' has unlisted category '{}'", b.name, b.category);
        }
    }
}
