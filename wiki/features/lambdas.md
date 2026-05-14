---
name: lambdas
description: First-class anonymous functions for pipe-style data processing.
type: feature
since: V1.0
related: [pipes, pattern-matching]
---

# Lambdas

A **lambda** is an anonymous function expression: `param => body`. The
primary use case is feeding higher-order builtins through the
[[pipes]] operator.

## Syntax

```soma
let double = x => x * 2
let doubled = list(1, 2, 3) |> map(x => x * 2)
let evens = data |> filter(x => x % 2 == 0)
let found = items |> find(x => x.id == target)
let any_active = users |> any(u => u.active)
```

## Block lambdas

For multi-statement bodies:

```soma
let enriched = data |> map(s => {
    let score = s.x * 2 + s.y
    s |> with("score", score)
})
```

The last expression is the result.

## Reduce / fold

```soma
let sum = list(1, 2, 3, 4) |> reduce(0, p => p.acc + p.val)
//                                    ^   ^ Map with `acc` and `val`
//                                    initial accumulator
```

`reduce(initial, f)` calls `f(map("acc", acc, "val", current))` and
threads the result as the new `acc`.

## Higher-order pipes — full list

```soma
data |> map(x => …)              // transform each element
data |> filter(x => …)           // keep elements where predicate is true
data |> find(x => …)             // first matching element (or Unit if none)
data |> any(x => …)              // Bool
data |> all(x => …)              // Bool
data |> count(x => …)            // Int
data |> reduce(initial, p => p.acc + p.val)
data |> each(x => …)             // side-effect every element; returns Unit
data |> partition(x => …)        // returns map { yes: List, no: List }
```

## Closures

A lambda captures its surrounding scope (read-only):

```soma
on filter_above(items: List, threshold: Int) {
    return items |> filter(x => x > threshold)
    //                          ^ captures `threshold` from outer scope
}
```

The capture is by **value** (snapshot at lambda construction). Mutating
`threshold` after the lambda is built doesn't affect the lambda.

## Examples

A data pipeline using only lambdas:

```soma
on top_scores(students: List) {
    students
        |> filter(s => s.active)
        |> map(s => s |> with("score", s.x * 2 + s.y))
        |> sort_by("score", "desc")
        |> top(10)
        |> map(s => s.name)
}
```

A reducer to compute statistics:

```soma
let stats = scores |> reduce(map("sum", 0, "count", 0), p => map(
    "sum",   p.acc.sum + p.val,
    "count", p.acc.count + 1
))
let avg = stats.sum / stats.count
```

## Edge cases

- A lambda **cannot** call `transition()`, `emit`, or other
  side-effecting builtins that the [[refinement]] / [[think-isolation]]
  checkers track. The checkers will warn if you try.
- Lambda bodies are not separately type-checked or memory-bounded —
  they're inlined into the caller's analysis.
- A lambda used as a non-pipe builtin's argument (e.g.
  `my_fn(x => x + 1)`) works at runtime but the higher-order builtin
  must accept lambdas; check the [[stdlib-pipes]] reference.

## What this does NOT cover

- **Curried functions** — not in V1.5. Each lambda takes exactly one
  parameter (possibly a Map for multiple values).
- **Tail-call optimization** — recursion in a lambda counts against
  the 512-depth call stack.
