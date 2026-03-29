# Soma v1 Edge Case & JIT Test Report
## Date: 2026-03-29

## Summary: 51 PASS / 2 FAIL (53 total) + 1 Missing Feature

---

## Section 1: Type Coercion (10/10 PASS)

| Test | Description | Result | Output |
|------|-------------|--------|--------|
| T01 | Int + Float -> Float | PASS | `3 + 2.5 = 5.5`, type: Float |
| T02 | Int == Float (5 == 5.0) | PASS | `true` |
| T03 | to_int(3.7) truncate | PASS | `3` (negative: to_int(-2.9) = -2) |
| T04 | to_int("abc") -> null | PASS | Returns `()` |
| T05 | to_float("xyz") -> null | PASS | Returns `()` |
| T06 | to_string variants | PASS | 42->"42", true->"true", ()->"null" |
| T07 | type_of all types | PASS | Int, Float, String, Bool, Unit, List, Map |
| T08 | Truthiness | PASS | 0, "", (), false are falsy; 1, "hello", true are truthy |
| T09 | Empty list falsy | PASS | `list()` is falsy, `list(1)` is truthy |
| T10 | Mixed arithmetic | PASS | Int/Int=Int (truncate), Int*Float=Float |

## Section 2: Overflow/Underflow (5/5 PASS)

| Test | Description | Result | Output |
|------|-------------|--------|--------|
| T11 | i64 MAX + 1 | PASS | Caught: "integer overflow: 9223372036854775807 + 1 (use BigInt for large numbers)" |
| T12 | abs(i64::MIN) | PASS | Caught: "abs: integer overflow (i64::MIN has no positive equivalent)" |
| T13 | Large float 1e308 | PASS | 1e308 works, 1e308*2 = inf |
| T14 | Division by zero | PASS | Int: error "division by zero"; Float: returns inf |
| T15 | Modulo by zero | PASS | Error: "modulo by zero" |

## Section 3: Unicode/UTF-8 (5/5 PASS)

| Test | Description | Result | Output |
|------|-------------|--------|--------|
| T16 | len("héllo") | PASS | 5 (char count, not bytes); len("日本語") = 3 |
| T17 | Unicode interpolation | PASS | "Welcome to café!" renders correctly |
| T18 | substring() multibyte | PASS | substring("café", 0, 4) = "café", substring("café", 3, 4) = "é" |
| T19 | escape_html() Unicode | PASS | `&lt;b&gt;héllo&lt;/b&gt; &amp; &quot;world&quot;` |
| T20 | Unicode contains/uppercase | PASS | contains works, uppercase("héllo wörld") = "HÉLLO WÖRLD" |

## Section 4: Recursion Limits (3/3 PASS)

| Test | Description | Result | Output |
|------|-------------|--------|--------|
| T21 | fib(30) | PASS | 832040 |
| T22 | Deep recursion (50000) | PASS | Caught: "stack overflow (recursion depth exceeded)" |
| T23 | try catches stackoverflow | PASS | Error caught in try block, handler continues |

## Section 5: JIT Correctness (13/15 PASS, 2 FAIL)

| Test | Description | Result | Notes |
|------|-------------|--------|-------|
| T24 | Arithmetic 2+3*4 | PASS | Both: 14 |
| T25 | Comparisons | PASS | All match |
| T26 | If/else | PASS | Both match |
| T27 | While loop sum 1..100 | PASS | Both: 5050 |
| T28 | For loop | PASS | Both: 150 |
| T29 | String operations | PASS | All match |
| T30 | fib(20) recursive | PASS | Both: 6765 |
| T31 | Cross-handler calls | PASS | Both: 50 |
| T32 | Lambda map(x => x*2) | PASS | Both: [2, 4, 6, 8, 10] |
| T33 | Lambda filter(x => x>3) | PASS | Both: [4, 5, 6] |
| T34 | Match expression | **FAIL** | JIT: "match expressions are not supported in the bytecode VM" |
| T35 | Nested function calls | PASS | Both: 26 |
| T36 | Short-circuit && \|\| | PASS | false && (1/0) = false, no crash |
| T37 | Null coalescing | PASS | All match |
| T38 | Try/catch | **FAIL** | JIT: try returns raw value (null/15) instead of {"value":..., "error":...} |

### JIT Bug Details:

**BUG 1 - T34: Match expressions not supported in JIT**
- Interpreted: Returns correct match result ("one", "two", "other")
- JIT: Returns string "match expressions are not supported in the bytecode VM"
- Severity: Medium - graceful degradation (no crash) but wrong result

**BUG 2 - T38: Try/catch returns different structure in JIT**
- Interpreted: `try { 10/0 }` -> `{"value": null, "error": "division by zero"}`
- JIT: `try { 10/0 }` -> `null`
- Interpreted: `try { 10+5 }` -> `{"value": 15, "error": null}`
- JIT: `try { 10+5 }` -> `15`
- Severity: High - code that checks `result.error` or `result.value` will break under JIT

## Section 6: Error Message Quality (10/10 PASS)

| Test | Description | Result | Error Output |
|------|-------------|--------|--------------|
| T39 | Parse error | PASS | `error: expected '}', found end of file` at line:col with caret |
| T40 | Undefined variable | PASS | `error: undefined variable: y` with source + caret |
| T41 | Type mismatch | PASS | `error: cannot add String and Int: hello + 5` with location |
| T42 | Arity error | PASS | `error: add() expected 2 arguments, got 3` with location |
| T43 | "Did you mean?" | PASS* | Shows `undefined variable: mesage` but NO suggestion. **Missing feature.** |
| T44 | filter_by invalid op | PASS | `error: filter_by: unknown operator '~' (use >, >=, <, <=, ==, !=)` |
| T45 | map() odd args | PASS | `error: map() requires an even number of arguments (key-value pairs), got 3` |
| T46 | break outside loop | PASS | `error: break outside of loop` with location |
| T47 | Division by zero location | PASS | `error: division by zero` at exact line:col |
| T48 | Parse error unexpected token | PASS | `error: expected expression, found 'return'` with location |

### Error Quality Notes:
- All errors include file path, line:col, source snippet, and caret
- Format: `error: <message>\n  --> file:line:col\n  |\n  | <source>\n  | ^`
- **Missing**: "Did you mean?" suggestion for misspelled variables (T43)

## Section 7: Multi-File Programs (5/5 PASS)

| Test | Description | Result | Output |
|------|-------------|--------|--------|
| T49 | use lib::module import | PASS | Imports and calls work |
| T50 | Imported handler chain | PASS | double(double(5)) = 20 |
| T51 | soma check multi-file | PASS | "All checks passed" |
| T52 | soma test multi-file | PASS | 2 tests: 2 passed, 0 failed |
| T53 | soma verify multi-file | PASS | State machine verified: 3 states |

---

## Final Tally

| Section | Pass | Fail | Total |
|---------|------|------|-------|
| Type Coercion | 10 | 0 | 10 |
| Overflow/Underflow | 5 | 0 | 5 |
| Unicode/UTF-8 | 5 | 0 | 5 |
| Recursion Limits | 3 | 0 | 3 |
| JIT Correctness | 13 | 2 | 15 |
| Error Messages | 10 | 0 | 10 |
| Multi-File | 5 | 0 | 5 |
| **TOTAL** | **51** | **2** | **53** |

## Bugs Found

1. **JIT: Match expressions not compiled** - JIT returns a placeholder string instead of evaluating match
2. **JIT: Try/catch unwraps result** - JIT returns the raw value/null instead of the `{value, error}` map that the interpreter returns. Code relying on `.error` or `.value` fields will silently break under `--jit`

## Missing Feature

3. **"Did you mean?" suggestions** - Undefined variable errors do not suggest similar variable names (e.g., `mesage` near `message`)

## Notable Observations

- Float division by zero returns `inf` (IEEE 754 compliant), not an error
- Integer division truncates toward zero (10/3 = 3)
- `to_string(())` returns `"null"` (not `"()"`)
- `1e308` prints as fully expanded decimal (no scientific notation in display)
- `index_of` on Unicode strings returns char index (6 for "wörld" in "héllo wörld"), correctly character-based
- Overflow errors include helpful hints: "use BigInt for large numbers"
- Error messages are exemplary: file path, line:col, source snippet, and caret on every error
