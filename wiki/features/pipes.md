---
name: pipes
description: `|>` operator for left-to-right data flow. The Soma idiom for collection pipelines.
type: feature
since: V1.0
related: [lambdas, stdlib-pipes, stdlib-collections]
---

# Pipes

The `|>` operator threads a value through a sequence of transformations:

```soma
data |> filter(x => x > 0) |> map(x => x * 2) |> sort_by("score", "desc") |> top(10)
```

Desugars to: `top(sort_by(map(filter(data, x => x > 0), x => x * 2), "score", "desc"), 10)`.

Reads top-to-bottom; no nested parentheses; the value of interest is
always on the left.

## What pipes do

`x |> f(args)` becomes `f(x, args)`. The left-hand-side is prepended
as the first argument of the right-hand-side function call.

```soma
list(1, 2, 3) |> push(4)     // → push(list(1, 2, 3), 4) = [1, 2, 3, 4]
data |> with("key", "val")    // → with(data, "key", "val")
"hello" |> uppercase()        // → uppercase("hello") = "HELLO"
```

## Composition

Multiple pipes chain naturally:

```soma
data |> filter(x => x.active)
     |> map(x => x.name)
     |> sort()
     |> top(5)
```

Multi-line is fine. The compiler doesn't care about whitespace; the
chain ends when the next token isn't a pipe.

## Higher-order pipe builtins

The pipes-heavy builtins (in `stdlib/builtins.cell`):

```soma
data |> map(f)              // List<T> → List<U>
data |> filter(pred)        // List<T> → List<T>
data |> find(pred)          // List<T> → T or ()
data |> any(pred)           // List<T> → Bool
data |> all(pred)           // List<T> → Bool
data |> count(pred)         // List<T> → Int
data |> reduce(init, f)     // List<T> → U
data |> each(f)             // List<T> → Unit (for side effects)
```

Field-based versions don't need a lambda:

```soma
data |> filter_by("price", ">", 100)     // ops: > >= < <= == !=
data |> sort_by("score", "desc")
data |> group_by("dept")                  // Map<String, List>
data |> distinct("category")              // unique values
data |> top(10)
data |> bottom(5)
```

Utilities:

```soma
data |> flatten()
data |> reverse()
data |> zip(other)
list("a", "b", "c") |> join(", ")        // "a, b, c"
```

See [[stdlib-pipes]] for full signatures.

## Examples

A scoring pipeline (rebalancer-style):

```soma
on rank(candidates: List) {
    candidates
        |> filter(c => c.market_cap > 1000000000)
        |> map(c => c |> with("score", c.alpha * 0.6 + c.momentum * 0.4))
        |> sort_by("score", "desc")
        |> top(20)
        |> map(c => map("ticker", c.ticker, "weight", c.score / 100.0))
}
```

A HTTP request normalizer:

```soma
on classify(req: Map) {
    req
        |> with("method_upper", uppercase(req.method))
        |> with("path_norm", lowercase(req.path))
        |> with("has_body", len(req.body) > 0)
}
```

A reduce-fold for aggregations:

```soma
let totals = trades |> reduce(map("buy", 0, "sell", 0), p => {
    if p.val.side == "BUY" {
        map("buy", p.acc.buy + p.val.qty, "sell", p.acc.sell)
    } else {
        map("buy", p.acc.buy, "sell", p.acc.sell + p.val.qty)
    }
})
```

## Comparison with method chaining

Soma doesn't have method chaining for collections (no `data.filter(…).map(…)`).
The pipe is the way:

```soma
// NOT VALID:
let r = data.filter(p).map(f).top(5)

// VALID:
let r = data |> filter(p) |> map(f) |> top(5)
```

Memory slots DO have method calls (`data.set(…)`, `data.keys`), but
they're not chainable across types.

## Edge cases

- `|>` binds tighter than most operators except function call. To
  pipe into the result of an expression, parenthesize:
  ```soma
  let r = (a + b) |> double()
  ```
- A pipe into a builtin that doesn't accept the LHS as first arg is a
  runtime error (the value is just prepended).
- Pipes work with user-defined handlers too:
  ```soma
  let scored = input |> compute_score() |> persist()
  ```
  desugars to `persist(compute_score(input))`.

## What this does NOT cover

- **Concurrent pipes** — there's no `data |> par_map(…)`. All pipes
  are sequential.
- **Streaming** — pipes operate on fully materialized lists, not
  iterators. For huge lists this is a memory concern; see
  [[budget-proof]] for how to bound.
