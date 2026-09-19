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
    "agent", "linalg", "native", "internal", "reserved",
];

pub static BUILTINS: &[BuiltinDoc] = &[
    // ── string ──────────────────────────────────────────────────────
    doc("concat", "string", "concat(a, b) -> String | concat(a: List, b: List) -> List",
        "Concatenate strings, or join two lists (numeric list `+` is elementwise, so this is THE list concat)."),
    doc("pad_left", "string", "pad_left(s, width: Int, fill?: String) -> String",
        "Left-pad to `width` characters: pad_left(\"7\", 4, \"0\") = \"0007\". Default fill is a space."),
    doc("pad_right", "string", "pad_right(s, width: Int, fill?: String) -> String",
        "Right-pad to `width` characters."),
    doc("split", "string", "split(s: String, delim: String) -> List<String>",
        "Split a string on a delimiter into a list of substrings."),
    doc("replace", "string", "replace(s: String, old: String, new: String) -> String",
        "Replace every occurrence of `old` with `new`."),
    doc("contains", "string", "contains(haystack: String, needle: String) -> Bool | contains(list: List, x) -> Bool | contains(m: Map, key) -> Bool",
        "Substring test; list membership (structural equality); map key membership."),
    doc("starts_with", "string", "starts_with(s: String, prefix: String) -> Bool",
        "True if `s` begins with `prefix`."),
    doc("ends_with", "string", "ends_with(s: String, suffix: String) -> Bool",
        "True if `s` ends with `suffix`."),
    doc("lowercase", "string", "lowercase(s: String) -> String",
        "Lowercase the string (non-strings are stringified first)."),
    doc("uppercase", "string", "uppercase(s: String) -> String",
        "Uppercase the string (non-strings are stringified first)."),
    doc("trim", "string", "trim(s: String) -> String | trim(s: String, chars: String) -> String",
        "Strip leading and trailing whitespace — or any of the characters in `chars` (Go's strings.Trim(s, cutset))."),
    doc("sha256", "string", "sha256(s: String) -> String",
        "SHA-256 of the text, as 64 hex characters (store `sha256(salt + password)` or better an hmac_sha256 with a server secret, never the password)."),
    doc("hmac_sha256", "string", "hmac_sha256(key: String, message: String) -> String",
        "HMAC-SHA-256 as hex: sign a session cookie or a webhook payload with a server secret."),
    nondet("random_token", "string", "random_token(bytes?: Int) -> String",
        "Cryptographically secure random bytes from the OS, as hex (default 32 bytes = 64 characters): session tokens, API keys, salts. random() is NOT for secrets."),
    doc("secure_eq", "string", "secure_eq(a: String, b: String) -> Bool",
        "Constant-time equality for secrets (tokens, signatures): `==` returns early and leaks a prefix by timing."),
    doc("format", "string", "format(fmt: String, args...) -> String",
        "printf subset: %d %s %f %.2f %8.2f %e %.3e %3d %-8s %05d %% — %e is C-style scientific (6.022e+23); widths, precision (rounded half away from zero on the decimal text), left-align with '-', zero-pad with '0'."),
    doc("to_fixed", "math", "to_fixed(x: Float, digits: Int) -> String",
        "x with exactly `digits` decimals (\"%.2f\"), rounded half away from zero on the decimal text: to_fixed(1.005, 2) = \"1.01\"."),
    doc("div_round", "math", "div_round(n: Int, d: Int) -> Int",
        "Exact integer division rounded to the nearest, half away from zero (BigDecimal HALF_UP): div_round(10125 * 600, 120000) == 51, div_round(-7, 2) == -4. Money in cents stays exact."),
    doc("floor_div", "math", "floor_div(a: Int, b: Int) -> Int",
        "Division rounded toward -∞ (Ruby/Python `//`): floor_div(-150, 100) = -2. `idiv` truncates toward zero; `/` is exact."),
    doc("mod", "math", "mod(a: Int, b: Int) -> Int",
        "Modulo with the DIVISOR's sign (Ruby/Python `%`): mod(-150, 100) = 50. The `%` operator keeps the dividend's sign (C/Rust): -150 % 100 = -50."),
    doc("divmod", "math", "divmod(a: Int, b: Int) -> [q, r]",
        "[floor_div(a, b), mod(a, b)] — q * b + r == a with 0 <= r < |b|."),
    doc("parse_date", "time", "parse_date(s: \"YYYY-MM-DD\") -> {year, month, day, weekday, epoch_day}",
        "Strict ISO date to its parts (weekday 1 = Monday); raises kind \"date\" otherwise."),
    doc("add_days", "time", "add_days(date: String, n: Int) -> String",
        "The ISO date n days later (negative n goes back), across month and year ends."),
    doc("add_months", "time", "add_months(date: String, n: Int) -> String",
        "Same day n months later, clamped to the month's length (Ruby's Date >> n): add_months(\"2026-01-31\", 1) = \"2026-02-28\"."),
    doc("days_between", "time", "days_between(a: String, b: String) -> Int",
        "Days from a to b (negative when b is earlier)."),
    doc("months_between", "time", "months_between(a: String, b: String) -> Int",
        "Whole months from a to b (\"YYYY-MM-DD\"), day-of-month aware like java.time MONTHS.between: 2026-01-15 → 2026-04-14 is 2, → 2026-04-20 is 3."),
    doc("days_in_month", "time", "days_in_month(year: Int, month: Int) -> Int",
        "28–31, leap years included."),
    doc("fields", "string", "fields(s: String) -> List<String>",
        "Split on any run of whitespace, no empty pieces (Go's strings.Fields; split(s, \" \") keeps empties)."),
    doc("index_of", "string", "index_of(s: String, sub: String) -> Int  |  index_of(xs: List, x) -> Int",
        "Character index of the first occurrence of `sub` in a String, or the position of the first element equal to `x` in a List; -1 if absent."),
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
        "Parse a JSON string into a Map/List/scalar; maps and lists pass through. Invalid JSON RAISES (kind \"json\") — wrap LLM output in try { from_json(s) }."),
    doc("type_of", "types", "type_of(x) -> String",
        "Type name: \"Int\" (any size), \"Float\", \"String\", \"Bool\", \"List\", \"Map\", \"Function\", \"Variant\", or \"Unit\"."),
    doc("is_type", "types", "is_type(value: Map, type_name: String) -> Bool",
        "True if a record's `_type` field equals `type_name`."),
    doc("is_a", "types", "is_a(value: Map, type_name: String) -> Bool",
        "Alias of is_type."),

    // ── math ────────────────────────────────────────────────────────
    doc("abs", "math", "abs(x: Int|Float) -> Int|Float",
        "Absolute value, arbitrary precision (abs(-9223372036854775808) is 9223372036854775808)."),
    doc("round", "math", "round(x: Float) -> Int | round(x: Float, digits: Int) -> Float",
        "Round half away from zero to the nearest integer, or keep `digits` decimals: round(2.345, 2) = 2.35."),
    doc("floor", "math", "floor(x: Float) -> Int",
        "Largest integer <= x."),
    doc("ceil", "math", "ceil(x: Float) -> Int",
        "Smallest integer >= x."),
    doc("sqrt", "math", "sqrt(x: Int|Float) -> Float",
        "Square root."),
    doc("sin", "math", "sin(x: Int|Float) -> Float", "Sine (radians). Also cos, tan, atan, atan2(y, x)."),
    doc("cos", "math", "cos(x: Int|Float) -> Float", "Cosine (radians)."),
    doc("tan", "math", "tan(x: Int|Float) -> Float", "Tangent (radians)."),
    doc("atan", "math", "atan(x: Int|Float) -> Float", "Arc tangent."),
    doc("atan2", "math", "atan2(y: Float, x: Float) -> Float", "Arc tangent of y/x, quadrant-aware."),
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
    doc("min", "math", "min(a, b) -> Int|Float | min(list: List) -> Int|Float",
        "Smaller of two numbers, or the minimum of a list (Float if any element is)."),
    doc("max", "math", "max(a, b) -> Int|Float | max(list: List) -> Int|Float",
        "Larger of two numbers, or the maximum of a list (Float if any element is)."),
    doc("sum", "math", "sum(list: List) -> Int|Float",
        "Sum of a list of numbers (Int-exact unless any element is a Float); 0 when empty. Floats are added left to right without compensation (NumPy's pairwise / Python's fsum can differ in the last bits)."),
    doc("product", "math", "product(list: List) -> Int|Float",
        "Product of a list of numbers; 1 when empty."),
    doc("avg", "math", "avg(list: List) -> Int|Float",
        "Mean of a list of numbers, by the rule of `/`: avg([1, 2]) = 1.5, an exact mean of Ints stays an Int; () when empty."),
    doc("parse_int", "math", "parse_int(s: String) -> Int | ()",
        "Strict integer parse: () unless the WHOLE string is an integer (\"1.5\", \"12abc\", \"\" → ()). to_int() is lenient and truncates."),
    doc("parse_float", "math", "parse_float(s: String) -> Float | ()",
        "Strict float parse: () unless the whole string is a finite number."),
    doc("idiv", "math", "idiv(a: Int, b: Int) -> Int",
        "Integer division truncating toward zero; errors on division by zero."),
    doc("clamp", "math", "clamp(v, lo, hi) -> Int|Float",
        "Constrain v to [lo, hi]; errors if lo > hi."),
    nondet("random", "math", "random() -> Float | random(max: Int) -> Int | random(min: Int, max: Int) -> Int",
        "Time-seeded PRNG: float in [0,1), or int in [0,max) / [min,max). There is no seed: for reproducible runs write your own generator (an LCG over Ints), and for secrets use random_token()."),
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
        "Exact left shift (a * 2^n), arbitrary precision like every Int op. For a 64-bit wrapping shift (xorshift), mask: band(shl(x, 13), M) with M = shl(1, 64) - 1 bound once (a literal beyond 64 bits is not allowed in [native]); values past 2^63 run [native] code in BigInt mode — prefer 32-bit xorshift masks for speed."),
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
    doc("range", "collection", "range(start: Int, end: Int, step?: Int) -> List<Int>",
        "Integers from start toward end (exclusive); optional step may be negative to count down."),
    doc("sort", "collection", "sort(list: List, order?: \"desc\") -> List",
        "Sort scalars ascending (or \"desc\"); errors on incomparable element types."),
    doc("flatten", "collection", "flatten(list: List) -> List",
        "Flatten one level of nested lists."),
    doc("zip", "collection", "zip(a: List, b: List) -> List<{left, right}>",
        "Pair elements positionally; stops at the shorter list."),
    doc("enumerate", "collection", "enumerate(list: List) -> List<{index, value}>",
        "Attach a 0-based index to each element."),
    doc("with", "collection", "with(m: Map, key, value, ...) -> Map | with(list: List, i: Int, value) -> List",
        "Copy of the map with key-value pairs inserted, or copy of the list with element i replaced."),
    doc("without", "collection", "without(m: Map, keys...) -> Map",
        "Return a copy of the map with the given keys removed."),
    doc("merge", "collection", "merge(a: Map, b: Map) -> Map",
        "Copy of `a` with all entries of `b` inserted (b wins on conflict)."),
    doc("join", "collection", "join(list: List, sep: String) -> String | join(left: List, right: List, key) -> List",
        "Join list elements into a string — or, with two lists, an inner data join on `key`."),

    // ── pipeline ────────────────────────────────────────────────────
    doc("filter_by", "pipeline", "filter_by(rows: List<Map>, field, op: \">\"|\">=\"|\"<\"|\"<=\"|\"==\"|\"!=\", value) -> List<Map>",
        "Keep rows whose `field` compares true against `value` (op defaults to == with 3 args)."),
    doc("fail", "types", "fail(kind: String, detail?) -> never | fail(r: TryResult) -> never",
        "Raise a domain error. `try { f() }` yields {value, error, kind, detail}: branch on r.kind (\"not_found\", \"invalid_transition\", \"guard_failed\", \"invariant\", a `require … else Tag` tag, …); fail(r) re-raises a caught error unchanged."),
    doc("slice", "collection", "slice(xs: List|String, start: Int, end?: Int) -> List|String",
        "Sub-list / substring, end exclusive; negative indexes count from the end (slice(xs, -2) = last two). Clamped, never raises."),
    doc("keys", "collection", "keys(m: Map) -> List<String>",
        "Keys of a map VALUE, in insertion order. (Memory slots: slot.keys().)"),
    doc("values", "collection", "values(m: Map) -> List",
        "Values of a map VALUE, in insertion order. (Memory slots: slot.values().)"),
    doc("entries", "collection", "entries(m: Map) -> List<{key, value}>",
        "Key/value records of a map VALUE: for e in entries(m) { e.key  e.value }."),
    doc("sort_by", "pipeline", "sort_by(rows: List<Map>, field, order?: \"desc\") -> List<Map> | sort_by(list, x => key, order?: \"desc\") -> List",
        "Stable sort by a field (numbers by value, strings lexicographically) or by a key function; a list key sorts on several keys: sort_by(rows, r => [0 - r.total, r.name])."),
    doc("top", "pipeline", "top(rows: List, n: Int) -> List",
        "First n elements."),
    doc("bottom", "pipeline", "bottom(rows: List, n: Int) -> List",
        "Last n elements."),
    doc("sum_by", "pipeline", "sum_by(rows: List<Map>, field) -> Int | Float",
        "Sum of a field across rows: exact Int when every value is an Int, else a Float; a numeric String (\"5\") counts as its number."),
    doc("avg_by", "pipeline", "avg_by(rows: List<Map>, field) -> Int|Float",
        "Mean of a field; Int when whole, () on an empty list."),
    doc("min_by", "pipeline", "min_by(rows: List<Map>, field) -> Map",
        "Row with the smallest integer value of `field`, or ()."),
    doc("max_by", "pipeline", "max_by(rows: List<Map>, field) -> Map",
        "Row with the largest integer value of `field`, or ()."),
    doc("pluck", "pipeline", "pluck(rows: List<Map>, field) -> List",
        "Extract one field from every row (missing fields become ())."),
    doc("group_by", "pipeline", "group_by(rows: List<Map>, field) -> Map<String, List>",
        "Group rows into a map keyed by the field's stringified value: 1 and \"1\" (true and \"true\") share a group, a row without the field goes to \"unknown\" and a () value to \"null\" — normalise the field first when those differ in your data."),
    doc("distinct", "pipeline", "distinct(rows: List, field?) -> List",
        "Unique elements — or, with `field`, the unique VALUES of that field (distinct_by keeps the rows)."),
    doc("distinct_by", "pipeline", "distinct_by(rows: List<Map>, field: String) -> List<Map>",
        "The first row per distinct value of `field` (lodash uniqBy / dedup by id). Alias: unique_by."),
    doc("count_by", "pipeline", "count_by(rows: List<Map>, field, value) -> Int",
        "Number of rows whose `field` stringifies equal to `value`."),
    doc("select", "pipeline", "select(rows: List<Map>, fields...) -> List<Map>",
        "Project each row down to the named fields."),
    doc("agg", "pipeline", "agg(rows: List<Map>, group_field, \"col:func\"...) -> List<Map>",
        "Group + aggregate: func is sum|avg|min|max|count; every group also gets a `count`. Groups are keyed like group_by (the field's text)."),
    doc("inner_join", "pipeline", "inner_join(left: List<Map>, right: List<Map>, key) -> List<Map>",
        "Merge rows whose `key` matches in both lists (left fields win)."),
    doc("left_join", "pipeline", "left_join(left: List<Map>, right: List<Map>, key) -> List<Map>",
        "Keep every left row, merging matching right-row fields when found."),

    // ── lambda (higher-order) ───────────────────────────────────────
    doc("filter", "lambda", "filter(list: List, x => Bool) -> List",
        "Keep elements where the lambda returns true (it answers a Bool; () counts as false)."),
    doc("find", "lambda", "find(list: List, x => Bool) -> Any",
        "First element where the lambda returns true, or ()."),
    doc("any", "lambda", "any(list: List, x => Bool) -> Bool",
        "True if the lambda returns true for at least one element."),
    doc("all", "lambda", "all(list: List, x => Bool) -> Bool",
        "True if the lambda returns true for every element (true on empty)."),
    doc("count", "lambda", "count(list: List, x => Bool) -> Int",
        "Number of elements where the lambda returns true."),
    doc("reduce", "lambda", "reduce(list: List, initial, p => expr) -> Any",
        "Fold the list; the lambda receives {acc, val} and returns the next acc."),

    // ── io ──────────────────────────────────────────────────────────
    doc("print", "io", "print(args...) -> ()",
        "Print arguments space-separated, then a newline."),
    doc("read_file", "io", "read_file(path: String) -> String | {error}",
        "Read a file as a string; returns {error: ...} on failure."),
    doc("write_file", "io", "write_file(path: String, content) -> Bool | {error}",
        "Write content (stringified) to a file; true on success."),
    doc("read_csv", "io", "read_csv(path: String, opts: Map?) -> List<Map> | {error}",
        "Parse an RFC 4180 CSV (quoted fields, \"\" escapes, multi-line quoted cells, CRLF) with a header row into maps. Unquoted cells are auto-typed Int/Float/String; a quoted cell and a leading-zero id (007) stay Strings; a short row is padded with \"\", extra fields are dropped. map(\"raw\", true) keeps every cell as text (exact money: \"1.00\")."),
    doc("write_csv", "io", "write_csv(path: String, rows: List<Map>) -> Bool | {error}",
        "Write rows as CSV using the first row's keys as the header. A cell read_csv would split, trim or re-type is quoted (separators, quotes, newlines, edge spaces, and a String that reads as a number: \"12\" comes back \"12\"); a List/Map is its JSON text, () an empty cell; true comes back as the String \"true\"."),
    doc("read_files", "io", "read_files(dir: String, count: Int) -> List<{path, content}>",
        "Read the first `count` files of a directory, in file-name order, with their content (a file that is not UTF-8 text is skipped). There is no listing, move or delete: remember the names you processed in a slot."),
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
        "Open a Server-Sent-Events connection that receives only the named streams (no name: every stream)."),
    doc("publish", "web", "publish(stream: String, data) -> ()",
        "Push data to a runtime-chosen SSE stream name on the event bus."),

    // ── http (outbound) ─────────────────────────────────────────────
    doc("http_get", "http", "http_get(url: String, opts?: {timeout, max_bytes, headers}) -> Map|List|String",
        "GET a URL. 2xx: the body (JSON parsed). Never raises: otherwise {error, kind, status, body} — kind http_status (status + the upstream body), timeout, refused or network. timeout defaults to 30000 ms."),
    doc("http_post", "http", "http_post(url: String, body, opts?: {timeout, max_bytes, headers}) -> Map|List|String",
        "POST body (a Map/List is sent as JSON, a String as is). Same result shape and default timeout as http_get. Also http_put, http_patch, http_delete(url, opts?)."),
    doc("http_put", "http", "http_put(url: String, body, opts?) -> Map|List|String", "PUT; same shape as http_post."),
    doc("http_patch", "http", "http_patch(url: String, body, opts?) -> Map|List|String", "PATCH; same shape as http_post."),
    doc("http_delete", "http", "http_delete(url: String, opts?) -> Map|List|String", "DELETE; same shape as http_get."),
    doc("ws_connect", "http", "ws_connect(url: String) -> Map",
        "Open a WebSocket connection; incoming messages dispatch as signals."),
    doc("ws_send", "http", "ws_send(msg) -> ()",
        "Send a message on the current WebSocket connection; errors if not connected."),
    doc("link", "http", "link(addr: \"host:port\") -> ()",
        "Open a TCP signal-bus link to a peer node."),
    doc("subscribe", "http", "subscribe(url: String) -> ()",
        "Subscribe to a remote event stream; an {\"event\", \"data\"} message runs on event(data) only for an event this program emits or soma.toml [bus] accept lists (else refused); other text runs on ws(msg)."),

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
        "Block the current handler for `ms` milliseconds (0 to 86400000; anything else raises kind range). Under serve it holds the handler lock the whole time."),

    // ── state machines ──────────────────────────────────────────────
    doc("next_id", "state", "next_id() -> Int",
        "Monotonic per-cell counter, in its own table (persistent under run/serve; per test cell in tests); journaled — a refused request burns no id; the ids drawn inside a `try` that fails are kept (they may have escaped into a local), so ids are unique, not always dense."),
    doc("transition", "state", "transition(id, target_state: String) -> {id, from, to}",
        "Move instance `id` to `target_state` (read the new state with get_status(id)); raises kind \"invalid_transition\" with the valid targets, or \"guard_failed\". Rolled back if the handler later fails."),
    doc("get_status", "state", "get_status(id) -> String",
        "Current state of instance `id` — the INITIAL state when `id` was never transitioned (an unknown id looks like a fresh instance; use has_state(id) to tell them apart)."),
    doc("has_state", "state", "has_state(id) -> Bool",
        "True when instance `id` was transitioned at least once (a recorded state exists). get_status(id) alone cannot distinguish an unknown id from a fresh one."),
    doc("valid_transitions", "state", "valid_transitions(id) -> List<String>",
        "States reachable from instance `id`'s current state."),

    // ── memory ──────────────────────────────────────────────────────
    doc("remember", "memory", "remember(key, value) -> ()",
        "Persist a value in the cell's agent memory slot."),
    doc("recall", "memory", "recall(key: String) -> Any",
        "The value this cell remember()ed under the key, or ()."),
    doc("append", "memory", "slot.append(value) -> ()",
        "Memory-slot method: append a value to a list-backed slot (alias: slot.push)."),

    // ── agent ───────────────────────────────────────────────────────
    doc("think", "agent", "think(prompt: String, system?: String, opts?: {max_tokens, timeout, max_rounds, tools_allowed, requires}) -> String",
        "Call the configured LLM with tool-calling, multi-turn context, and budget enforcement. A think() offers the model every tool of the cell's face, or only those named in map(\"tools_allowed\", [\"lookup\"]) (a call to another is refused and told to the model); `requires` lists model capabilities checked by `soma check`. A timeout is not retried (429/5xx are, up to 3 times)."),
    doc("think_json", "agent", "think_json(prompt: String, system?: String, opts?: {max_tokens, timeout, max_rounds, tools_allowed, requires}) -> Map",
        "Like think(), but parses the response as JSON into a Map (tools are offered too: pass max_rounds 1 to keep the cost bound at one round). There is no schema option — check the fields yourself."),
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
        "Human-in-the-loop gate. Answered by `mock approve true|false` in tests, by SOMA_APPROVE=always|never, or by a person at the terminal under `soma run`; otherwise (e.g. under soma serve) it RAISES kind \"approval_required\" — it never approves on its own."),

    // ── linalg / risk ───────────────────────────────────────────────
    doc("matrix", "linalg", "matrix(\"1 2; 3 4\") -> List<List<Float>>",
        "MATLAB-style matrix literal: ';' separates rows, whitespace/',' separates entries."),
    doc("mat", "linalg", "mat(rows: Int, cols: Int, values: List<Float>) -> List<List<Float>>",
        "Reshape a flat list into an r×c matrix; errors if the count mismatches."),
    doc("reshape", "linalg", "reshape(values, rows: Int, cols: Int) -> Matrix",
        "Lay a flat list or matrix out row-major as rows×cols; also m.reshape(r,c)."),
    doc("transpose", "linalg", "transpose(m: Matrix) -> Matrix",
        "Transpose; also m.transpose()."),
    doc("shape", "linalg", "shape(m) -> List<Int>",
        "[rows, cols] for a matrix, [n] for a vector; also m.shape()."),
    doc("matmul", "linalg", "matmul(a: Matrix, b: Matrix) -> Matrix",
        "Matrix product (also the `*` operator on two matrices); inner dims must agree."),
    doc("det", "linalg", "det(m: Matrix) -> Float",
        "Determinant of a square matrix (LU with partial pivoting)."),
    doc("diag_sum", "linalg", "diag_sum(m: Matrix) -> Float",
        "Matrix trace: sum of the diagonal."),
    doc("identity", "linalg", "identity(n: Int) -> Matrix",
        "n×n identity matrix (alias of eye)."),
    doc("scale", "linalg", "scale(m: Matrix, k) -> Matrix",
        "Scalar-multiply every entry (also `k * m`)."),
    doc("flatten_mat", "linalg", "flatten_mat(m: Matrix) -> List<Float>",
        "Flatten a matrix to a row-major vector."),
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
        "RMT (Bouchaud-Potters) covariance cleaning: rows are observations (T), columns assets (N); the sample covariance divides by T (population — numpy cov uses T-1); center (default true) subtracts each column mean; clip replaces the eigenvalues inside the Marchenko-Pastur bulk by their mean; .matrix is the cleaned N×N, .eigenvalues the cleaned spectrum (no eigenvectors). max_obs / max_assets past the data raise kind range."),
    doc("impact_sqrt", "linalg", "impact_sqrt(qty: Float, daily_volume: Float, sigma: Float, opts?: {Y}) -> Map",
        "Bouchaud square-root market-impact law; .bps is expected slippage."),
    doc("quantile", "linalg", "quantile(values: List<Float>, q: Float) -> Float",
        "q-th quantile with linear interpolation between the two nearest sorted values (numpy's default): quantile(xs, 0.5) == median(xs)."),
    doc("median", "math", "median(xs: List) -> Int | Float",
        "Middle value of the sorted list (mean of the two middles for even n, exact Int when it is one) — statistics.median."),
    doc("pstdev", "math", "pstdev(xs: List) -> Float",
        "Population standard deviation (divide by n) — statistics.pstdev."),
    doc("stddev", "math", "stddev(xs: List) -> Float", "Same as pstdev (population)."),
    doc("stdev", "math", "stdev(xs: List) -> Float",
        "SAMPLE standard deviation (divide by n - 1) — statistics.stdev / pandas .std(); needs two values."),
    doc("variance", "math", "variance(xs: List) -> Float",
        "SAMPLE variance (divide by n - 1) — statistics.variance; pvariance is the population form."),
    doc("pvariance", "math", "pvariance(xs: List) -> Float", "Population variance (divide by n) — statistics.pvariance."),
    doc("chr", "string", "chr(n: Int) -> String", "The character with code point n: chr(65) == \"A\"."),
    doc("ord", "string", "ord(s: String) -> Int", "Code point of the first character: ord(\"A\") == 65."),
    doc("var_historical", "linalg", "var_historical(returns: List<Float>, opts?: {alpha, max_obs}) -> Float",
        "Historical Value-at-Risk as a POSITIVE loss: -quantile(returns, 1 - alpha) (returns positive for gains); alpha in (0, 1), else kind range; more observations than max_obs raise kind range."),
    doc("expected_shortfall_historical", "linalg", "expected_shortfall_historical(returns: List<Float>, opts?: {alpha, max_obs}) -> Float",
        "Historical expected shortfall (CVaR) as a positive loss: minus the mean of the returns at or below the (1 - alpha) quantile; alpha in (0, 1); max_obs as for var_historical."),
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

    doc("regex_count", "string", "regex_count(text: String, pattern: String) -> Int",
        "Number of non-overlapping matches (Rust regex syntax). Same in [native] (pattern must be a literal there)."),
    doc("regex_match", "string", "regex_match(text: String, pattern: String) -> Int",
        "1 when the pattern matches anywhere in text, else 0."),
    doc("regex_replace", "string", "regex_replace(text: String, pattern: String, replacement: String) -> String",
        "Replace every match; $1 refers to the first capture group."),
    doc("read_stdin", "io", "read_stdin() -> String", "The whole standard input (for `soma run` filters)."),
    doc("write_str", "io", "write_str(s: String) -> Int", "Write s to stdout without a newline; returns the byte count."),

    // ── [native] handlers only ──────────────────────────────────────
    doc("buffer", "native", "buffer(n: Int) -> Buf   [native] only",
        "Array of n Ints, zeroed. Random access with buf_get / buf_set. Not available in interpreted handlers."),
    doc("buf_get", "native", "buf_get(b: Buf, i: Int) -> Int   [native] only", "Read b[i]."),
    doc("buf_set", "native", "buf_set(b: Buf, i: Int, v: Int) -> ()   [native] only", "Write b[i] = v."),
    doc("buffer_f", "native", "buffer_f(n: Int) -> BufF   [native] only", "Array of n Floats, zeroed (buf_get_f / buf_set_f)."),
    doc("buf_get_f", "native", "buf_get_f(b: BufF, i: Int) -> Float   [native] only", "Read b[i]."),
    doc("buf_set_f", "native", "buf_set_f(b: BufF, i: Int, v: Float) -> ()   [native] only", "Write b[i] = v."),
    doc("hashmap", "native", "hashmap() -> HMap   [native] only", "Int → Int hash map (hm_get / hm_set / hm_inc / hm_len / hm_has)."),
    doc("hm_get", "native", "hm_get(m: HMap, k: Int) -> Int   [native] only", "Value at k, 0 when absent."),
    doc("hm_set", "native", "hm_set(m: HMap, k: Int, v: Int) -> ()   [native] only", "m[k] = v."),
    doc("hm_inc", "native", "hm_inc(m: HMap, k: Int) -> ()   [native] only", "m[k] += 1 (inserting 1)."),
    doc("hm_len", "native", "hm_len(m: HMap) -> Int   [native] only", "Number of keys."),
    doc("hm_has", "native", "hm_has(m: HMap, k: Int) -> Bool   [native] only", "Whether k is present."),
    doc("strbuf", "native", "strbuf(capacity?: Int) -> SBuf   [native] only", "Growable string builder (sb_push / sb_push_int / sb_push_char / sb_len / sb_finish)."),
    doc("sb_push", "native", "sb_push(b: SBuf, s: String) -> ()   [native] only", "Append a string."),
    doc("sb_push_int", "native", "sb_push_int(b: SBuf, n: Int) -> ()   [native] only", "Append an Int's decimal digits."),
    doc("sb_push_char", "native", "sb_push_char(b: SBuf, c: Int) -> ()   [native] only", "Append one character by code point."),
    doc("sb_len", "native", "sb_len(b: SBuf) -> Int   [native] only", "Bytes so far."),
    doc("sb_finish", "native", "sb_finish(b: SBuf) -> String   [native] only", "The built String."),
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
            "random", "rand", "random_token",
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

/// The most arguments any documented form of `name` takes; None when a form
/// is variadic (`args...`) or the builtin is unknown. `max(1, 2, 3)`
/// silently answered 2 (the third argument was ignored).
pub fn max_arity(name: &str) -> Option<usize> {
    let sig = lookup(name)?.signature;
    let mut best = 0usize;
    let pat = format!("{}(", name);
    let mut rest = sig;
    let mut found = false;
    while let Some(i) = rest.find(&pat) {
        found = true;
        let after = &rest[i + pat.len()..];
        let mut depth = 0i32;
        let mut end = None;
        for (j, c) in after.char_indices() {
            match c {
                '(' | '<' | '[' | '{' => depth += 1,
                ')' if depth == 0 => { end = Some(j); break; }
                ')' | '>' | ']' | '}' => depth -= 1,
                _ => {}
            }
        }
        let inner = &after[..end?];
        if inner.contains("...") || inner.contains('…') { return None; }
        let mut n = 0usize;
        let mut d = 0i32;
        let mut cur = String::new();
        for c in inner.chars() {
            match c {
                '<' | '(' | '[' | '{' => { d += 1; cur.push(c); }
                '>' | ')' | ']' | '}' => { d -= 1; cur.push(c); }
                ',' if d == 0 => { if !cur.trim().is_empty() { n += 1; } cur.clear(); }
                _ => cur.push(c),
            }
        }
        if !cur.trim().is_empty() { n += 1; }
        best = best.max(n);
        rest = &after[end? ..];
    }
    if found { Some(best) } else { None }
}
