# Auto-optimizations: pre-implementation study

Goal: extend the "auto-memo" idea to other transparent compiler-driven
transformations that detect a syntactic pattern in `[native]` handlers
and apply a guaranteed-correct rewrite. **Zero false positives is a
hard requirement** — every rule must reject any handler whose semantics
the rewrite would change, even if that means rejecting some legitimate
candidates.

## Survey of the existing codebase

After comment-stripping and per-cell scoping (siblings only — calls
across cells with the same name don't count as self-recursion):

| Handler                                  | Shape                              | Eligible for           |
|------------------------------------------|------------------------------------|------------------------|
| `euclidean_alg.cell::gcd_basic(a,b)`     | `return gcd_basic(b, a%b)`         | **auto-iteration**     |
| `ext_gcd.cell::egcd_g(a,b)`              | `return egcd_g(b, a-(a/b)*b)`      | **auto-iteration**     |
| `ext_gcd.cell::egcd_y(a,b)`              | `return egcd_x(b,r) - q*egcd_y(b,r)` | NO — not tail, mutual rec |
| `crt.cell::egcd_y(a,b)`                  | same                                | NO — same              |
| `champernowne_words.cell::letters(n)`    | `l = l + 3 + letters(n%100)`        | NO — derived param, not tail |
| `nqueens.cell::solve(n,r,c1,c2,c3)`      | inside `while`, accumulator          | NO — not tail          |
| `stirling2.cell::s2_rec(n,k)`            | `k*s2_rec(n-1,k) + s2_rec(n-1,k-1)` | **auto-tabulation**    |
| `ackermann.cell::ackermann(m,n)`         | nested self-call                    | auto-memo only         |
| `karatsuba`, `mergesort`, `quicksort`    | derived split args                  | none                   |
| `levenshtein.cell::lev(a,b,i,j)`         | 4 params                            | none (arity)           |

Plus 20 stress cases in `examples/memo_corpus/m*.cell`, of which all
"yes"-eligible cases are also auto-tabulation candidates **except**
m17_ackermann_nested (nested call → no monotone fill order) and any
case with `param − param` shrinks.

## Candidate 1 — auto-iteration

**Idea**: a `[native]` handler whose final operation is `return self(args)`
(pure tail call) is rewritten as a `while true` loop that updates the
parameters in-place. Eliminates stack overflow risk and call overhead.

### Detection rule (strict)

A handler `f(p1..pn) [native]` is auto-iteration eligible iff:

1. It is `[native]`.
2. All parameters are scalar (`Int`, `Float`, `Bool`).  *No `String`* —
   string params are `&str` and rebinding inside a loop is awkward; we'd
   have to re-clone them on every iteration to satisfy borrow checker.
3. The body contains **exactly one** self-call.
4. That self-call appears in **tail position**: the body ends with
   `return f(args)` and no other code follows in the same control-flow
   path. Concretely:
   - The whole-body's last statement is `return f(args)`, OR
   - Every branch (`if`/`else if`/`else`) of the body's terminator is
     `return f(args)` or `return <const-or-param>`.
5. The argument expressions in the self-call only reference parameters
   and let-bound locals from the same body — they must not depend on
   the *result* of the self-call (which would be impossible by
   construction since the call IS in tail position, but we re-check).

### False-positive traps

| Trap                                                  | Guard                                                              |
|-------------------------------------------------------|--------------------------------------------------------------------|
| Same-named function from another cell (HTTP `get(k)`)| Use the `siblings` set, only count calls to *this* cell's handler |
| Self-call inside an expression (e.g. `n*f(n-1)`)      | Reject — self-call must be the entire return expression           |
| Self-call inside a loop                                | Reject — only the body-level last statement counts                |
| Self-call inside a `while`/`for`                      | Reject — even if it's the last stmt of the body                   |
| Two self-calls (one tail, one not)                    | Rule (3) requires *exactly one*                                   |
| Self-call with String args                             | Rule (2) excludes Strings                                         |
| Self-call where args include `f(...)` nested          | Rule (5) — args reference only params/locals/literals             |
| Mutually recursive (`f → g → f`)                       | Detected by counting *self*-calls only                            |

### Why we don't extend to "left-fold" form (`return acc * f(...)`)

The conservative version is "pure tail call only". One could imagine
also accepting `return p * f(args)` and rewriting to an accumulator
loop. **We reject this** because:

- The accumulator transformation requires the operator to be both
  associative and commutative *over the iteration order*, which is
  fragile to verify syntactically (consider `f(n) - g(n)` — looks
  like fold, but isn't).
- The user can write the loop themselves if they need it.
- Zero false positives is non-negotiable.

### Expected impact

- `euclidean_alg::gcd_basic` — already non-bottleneck (small inputs).
  Eliminates the 0.1% recursive overhead.
- `ext_gcd::egcd_g` — same.
- The real value is **risk reduction**: any user who writes a tail-
  recursive helper gets stack-safe iteration for free.

### Soundness sketch

A pure tail call `return f(g1, ..., gn)` evaluates `g1..gn` then jumps.
Replacing with `p1, ..., pn = g1, ..., gn; continue` is semantically
identical because:
- The frame about to be popped contains no live values past the call.
- Parameters are the only state the next iteration sees.
- We must evaluate all `gi` *before* any reassignment (else `g2` could
  see the new value of `p1`). Solution: bind to fresh locals first.

## Candidate 2 — auto-tabulation

**Idea**: a handler eligible for auto-memo whose self-call args are
all monotone shrinks toward zero is rewritten as a bottom-up `Vec`
fill. Eliminates the `HashMap` overhead, lets LLVM vectorize, and uses
strictly less memory.

### Detection rule (strict)

A handler `f(p1..pn) [native]` is auto-tabulation eligible iff:

1. It satisfies `auto_memo_eligible(f)`. (Reuse the existing rule.)
2. All self-call arguments are of the form:
   - `pi` (parameter unchanged), OR
   - `pi − k` where `k ≥ 0` is a small literal.

   In other words: **no `param − param`, no nested self-calls, no
   literal-only args**. Every arg must be one specific parameter,
   shifted down by a known amount.

3. There must exist **at least one** `pi − k` where `k ≥ 1` for at
   least one parameter — otherwise the recursion doesn't actually
   shrink, which would mean the handler diverges.
4. Return type is `Int`. (Same restriction as auto-memo.)

### Why this rule is safe

Conditions (1)+(2) imply that for every recursive call `f(args)`, each
arg is `≤` the corresponding caller arg. Therefore the call graph rooted
at `f(N1, ..., Nn)` only ever visits points in the box
`[0..N1] × ... × [0..Nn]`. We can pre-allocate a Vec of size
`(N1+1) × ... × (Nn+1)` and fill it in row-major order from `(0,...,0)`
upward. Each cell is filled exactly once. The recursive recurrence is
turned into a loop over the cell index.

### False positive traps

| Trap                                                  | Guard                                                              |
|-------------------------------------------------------|--------------------------------------------------------------------|
| `f(n - n)` (param-param) — base case zero, fine but breaks fill order | Excluded by rule (2)                                           |
| Nested call `f(n - 1, f(n - 1, k))` (Ackermann)        | Excluded — nested call is not "param − k"                          |
| Literal-only `f(0, k)` — base case but no shrink     | Allowed if SOME other call shrinks; excluded if it's the only call |
| Mixed-arity (handler takes `(n,k)` but recursive call passes only `(n-1)`) | Impossible — all sibling calls type-check        |
| Negative shrink amount `f(n + 1)`                      | Rule requires `k ≥ 0`                                              |
| Large literal in arg (e.g. `f(n - 1000000)`)          | Rule (2) limits literal to "small" (≤ 5 like memo); larger means table bound is huge → fall back to memo |
| Recursive call with `f(p, p)` where p is a param      | Allowed — both args are "param unchanged" form                     |
| Negative parameter at runtime (`n < 0` is base case)  | Use `if n < 0 { call_compute(n) }` bypass — same as memo's "doesn't fit i64" path |

### Implementation strategy

For 1-arg eligible handlers:
```rust
fn inner_handler_f_fast(n: i64) -> i64 {
    if n < 0 { return inner_handler_f_fast_compute(n); }
    let cap = (n as usize) + 1;
    let mut tab: Vec<i64> = Vec::with_capacity(cap);
    for i in 0..cap {
        // emit the body but each "f(i - k)" becomes "tab[i - k]"
        let v = compute_inline(i as i64, &tab);
        tab.push(v);
    }
    tab[n as usize]
}
```

For 2-arg, similar with 2D vec. Cap at e.g. 1024×1024 = 8MB to avoid
OOM on adversarial inputs; fall back to memo above the cap.

**Subtlety**: the body references `f(i - k)` for various k. We can't
just emit `tab[i - k]` blindly because the body may have other code
that uses `f` indirectly through let-bindings. **Solution for v1**:
auto-tabulation only fires when the body is structurally simple — a
sequence of base-case `if` returns followed by a single `return
<linear combination of self-calls>`. If the body has complex control
flow, fall back to auto-memo.

### Formal "simple body" rule

A body is *tab-simple* iff its top-level statements are:
1. Zero or more `if cond { return literal_expr }` guards (base cases)
2. Followed by exactly one `return <expression>` whose expression
   contains only: literals, parameter references, arithmetic operators
   (`+`, `*`, `-`), and self-calls of the form `f(p1±k1, ..., pn±kn)`.

Anything else (let bindings of intermediate values, while loops, nested
ifs, calls to siblings) → fall back to auto-memo.

### Expected impact

For 1D fills (m01_fib, m02_tribonacci, m05_binomial_rec, m07_motzkin,
m09_lucas, m20_branches): ~5-10× over memo.
For 2D fills (m03_lcs2d, m04_edit, m06_delannoy, m08_partition,
stirling2): ~10× over memo (HashMap is the bottleneck).

## Candidate 3 — auto-CSE for pure calls

**Idea**: within a single straight-line block, the same pure-function
call repeated 2+ times is hoisted into a `let`.

### Detection rule (strict)

Within a single statement sequence (no control flow between the
occurrences), find call expressions that:

1. Are calls to a `[native]` sibling handler **OR** to a known-pure
   builtin (`gcd`, `is_prime`, `bit_test`, etc. — explicit allowlist).
2. Have the same syntactic form (same function, same arg expressions).
3. Each arg expression references only:
   - Literals
   - Parameters or let-bound locals whose binding has not been
     reassigned between the two call sites.

If found 2+ matching occurrences, hoist the first into a fresh local
and replace the others with the local.

### False positive traps

| Trap                                                  | Guard                                                              |
|-------------------------------------------------------|--------------------------------------------------------------------|
| Sibling that mutates global state                      | Soma `[native]` handlers are pure by construction                  |
| Sibling that reads `now_ms()`                           | Reject any call to `now_ms`, `random`, etc. (impure builtins list) |
| Arg variable reassigned between calls                  | Track reassignments in the same block                              |
| Calls separated by an if-else                          | Restrict to the *same* block — don't hoist across control flow     |
| Calls inside a loop                                    | Hoist within the loop body only, not out of the loop               |

### Expected impact

Modest. The big wins are catching patterns like
`if is_prime(n) { f(is_prime(n)) ... }` which are rare in
hand-written code but show up in macro-generated/AI-generated code.

## Candidates rejected after analysis

### Auto-bitset (`Set<Int>` → `Vec<u64>` bitvec)

Requires knowing the value bound at the insertion site. Without
range/abstract interpretation across handler boundaries, this is
unsafe — any value > the bound corrupts the bitvector. **Skipped.**

### Auto-matrix-exponentiation for linear recurrences

The detection rule (constant linear combination of `f(n - i)`) is
narrow but the rewrite (compile-time matrix construction + log-time
exponentiation) is large and the win only matters for `n > 10^6`,
which the existing memo+tab already handles fast. **Skipped.**

### Auto-vectorization of accumulators

LLVM already does this on the codegen side. Anything we'd add at the
Soma level would be redundant. **Skipped.**

### Auto-string-builder

Already implemented as a peephole (`result = result + to_string(x)` →
`write!`). The remaining cases are too varied to pattern-match safely.
**Skipped.**

## Implementation order

1. **Auto-iteration** first — small, surgical, lowest risk. Touches
   only one new emit path. Validate against `iter_corpus`.
2. **Auto-tabulation** second — refinement of auto-memo, reuses the
   existing detection rule. Validate against `memo_corpus` (existing)
   plus `tab_corpus` for the boundary cases.
3. **Auto-CSE** third — orthogonal to the above, very small, only fires
   in narrow cases.

After each step: full 100-challenge regression must pass.

## Validation methodology

For each candidate:

1. Build `examples/<feat>_corpus/*.cell` with `// expected_fire: yes/no`
   annotations covering at minimum:
   - The textbook positive case
   - Each false-positive trap as a negative case
   - Edge cases (smallest/largest valid inputs, base case at 0/1)
2. Build a Python verifier `bench/check_<feat>_corpus.py` that runs
   the syntactic detection rule and asserts 100% match.
3. Implement the Rust detection in `compiler/src/codegen/native.rs`,
   matching the Python verifier 1:1.
4. After implementation: rerun the verifier (still passes), run the
   corpus through the actual `soma run` (results match), run the full
   100-challenge regression (no breakage).

Only after all four steps pass is the feature declared done.
