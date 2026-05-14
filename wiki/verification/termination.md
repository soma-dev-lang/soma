---
name: termination
description: Every handler structurally terminates — bounded loops, finite recursion.
type: feature
since: V1.0
related: [handler, verification-overview, budget-proof]
---

# Termination

The termination checker proves every handler in every cell
structurally terminates. There are no infinite loops in well-formed
Soma code.

`soma verify` output:

```
✓ termination: all 31 handlers structurally terminate
```

## What's checked

For each handler, the checker walks the AST and verifies:

1. **`for var in expr { ... }`** — the iterable must be statically
   bounded. Either:
   - The iterable is `range(N)` or `range(from, to)` with literal
     bounds, OR
   - The for-loop has `[loop_bound(N)]` annotation:
     ```soma
     for [loop_bound(10000)] item in unknown_size_list { ... }
     ```

2. **`while cond { ... }`** — requires `[loop_bound(N)]`:
   ```soma
   while [loop_bound(1000)] not done { tick() }
   ```

3. **Recursion** — handlers can call themselves or each other, but
   the global recursion depth is capped at 512 by the interpreter.
   Beyond that, a `StackOverflow` runtime error fires.

4. **`break` and `continue`** — fine within bounded loops; don't
   help with unbounded ones.

5. **Calls to other handlers** — analyzed transitively. A handler
   calling an unbounded handler inherits the warning.

## What's NOT checked

- **Logical termination via decreasing measure.** A `for` loop with
  `[loop_bound(100)]` is "proven" terminating up to 100 iterations,
  regardless of whether the inner body always halts sooner. Soma's
  termination is *resource-bounded*, not measure-decreasing.
- **Cross-cell mutual recursion via `delegate`.** Each cell's
  handlers are checked locally. A pathological `delegate("Other",
  "ping")` chain back to the original cell can stack-overflow at
  runtime.

## Examples

A bounded loop, OK:

```soma
on tally(xs: List) {
    let sum = 0
    for x in xs { sum = sum + x }      // OK if xs is bounded
    return sum
}
```

If `xs`'s size isn't statically known, the checker requires:

```soma
on tally(xs: List) {
    let sum = 0
    for [loop_bound(10000)] x in xs { sum = sum + x }
    return sum
}
```

A while loop requires bound:

```soma
on poll() {
    let done = false
    while [loop_bound(100)] not done {
        done = check_state()
    }
}
```

Recursion is fine at moderate depth:

```soma
on factorial(n: Int) {
    if n <= 1 { return 1 }
    return n * factorial(n - 1)     // OK; depth ≤ 512
}
```

## Why this matters

Two things compose:

1. **No unbounded computation reaches production.** Every loop has a
   declared ceiling.
2. **[[budget-proof]] can sum them.** A `for [loop_bound(100)]` lets
   the cost checker multiply the body's cost by 100. Without the
   bound, the cost is `Unbounded` and the proof degrades to advisory.

This is part of why Soma agents can have provable memory budgets even
when calling `think()`: the *outer* loops are bounded.

## Edge cases

- `every Ns { ... }` and `after Ns { ... }` are scheduling
  primitives; their bodies are checked as ordinary handlers (no
  unbounded looping inside).
- A lambda body inside a `|> map(…)` or `|> filter(…)` is itself
  checked.
- `transition(id, X)` does not affect termination — it's a
  state-machine step, not a control-flow loop.

## Related

- [[handler]] — the construct that's checked.
- [[budget-proof]] — uses termination bounds for cost composition.
- [[verification-overview]] — where this fits in the verification
  scorecard.
