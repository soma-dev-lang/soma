---
name: handler
description: The only mechanism for adding behavior to a cell. `on signal_name(...) { ... }`.
type: concept
since: V1.0
related: [cell, face, signals, ensure, termination]
---

# Handler

A **handler** is the only mechanism for adding executable behavior to a
cell. Every operation a cell performs happens inside an `on` block.

There is no top-level code. No `main` function. No constructors. The
cell exists; handlers execute in response to signals.

## Syntax

```soma
on handler_name(param: Type, param: Type) {
    // statements
    return value     // optional; last expression is the result
}
```

## Concurrency model

**One handler at a time per cell instance.** No locks needed inside
handler bodies — the runtime serializes invocations on a single cell
instance. This is the Erlang/Pony actor pattern adapted: each cell is
its own actor.

(Concurrency *across* cells happens via the [[signals]] bus, which has
its own ordering story — see [[whats-missing]] for the open formal
semantics work.)

## Special handlers

- `on setup(p: Map)` — runs once at cell startup with the config.
- `on request(method: String, path: String, body: String)` — HTTP
  dispatch entry point.
- `every 30s { ... }` — scheduled execution (every interval).
- `after 5s { ... }` — one-shot delay.
- `on _name(...)` — underscore prefix marks the handler as internal;
  not in [[face]], not an HTTP endpoint.

## Handler properties

Optional `[bracketed]` annotations:

```soma
on rebalance(input: Map) [record] {       // log every call for replay
    ...
}

on optimize() [native] {                  // compile to Rust cdylib
    ...
}

on critical() [record, native] {          // combinable
    ...
}
```

Recognized properties:

- `[record]` — log inputs/outputs to `.somalog` for deterministic
  replay (`soma run --record`, `soma replay`).
- `[native]` — handler body compiles to a Rust cdylib for ~100–300×
  speedup on numeric loops. Subject to `compiler/src/checker/native.rs`
  restrictions (no `think()`, bounded loops, etc.).

## Termination guarantee

The [[termination]] checker proves every handler structurally
terminates:

- `for var in expr { ... }` with bounded `expr` or `[loop_bound(N)]`.
- `while cond { ... }` with explicit `[loop_bound(N)]`.
- No unbounded recursion (recursion depth capped at 512).

`soma verify` reports per-cell: `✓ termination: all 31 handlers
structurally terminate`.

## Postconditions

`ensure expr` inside a handler is a runtime-checked postcondition:

```soma
on withdraw(balance: Int, amount: Int) {
    let result = balance - amount
    ensure result >= 0           // fails the handler if false
    return result
}
```

See [[ensure]] for the full story and the verified pre-trade pattern.

## Handler effect summary

For each handler, the [[refinement]] checker computes the set of
transitions it can produce, with path conditions:

```
✓ refinement: handler `rebalance` ⟶ {
    signal_pending,
    failed [if alpha_cfg != () ∧ alpha_result.error != ()],
    blocked [if verdict == "BLOCK"],
    approved [if verdict == "APPROVE"],
    flagged
}
```

This is the V1.3 *theorem*: the picture next to the code is now the
code.

## Examples

A pure computation:

```soma
on factorial(n: Int) {
    if n <= 1 { return 1 }
    return n * factorial(n - 1)
}
```

A handler with state and a transition:

```soma
on submit_order(data: Map) {
    let id = "ORD-" + to_string(next_id())
    orders.set(id, to_json(data |> with("id", id)))
    transition(id, "validated")
    map("id", id, "status", "validated")
}
```

A handler with delegation:

```soma
on rebalance(input: Map) {
    let alpha = delegate("Alpha", "score", input)
    if alpha.error != () { return map("status", "failed", "reason", alpha.error) }
    let opt = delegate("Optimizer", "optimize", alpha.scores)
    map("status", "approved", "weights", opt.weights)
}
```

## Edge cases

- A handler that does not return an explicit value evaluates to the
  result of its last expression. Empty handler body → `Unit`.
- `return` exits the handler. There is no `goto`, `throw`, or
  exception ladder; failure flows through `try { ... }` and `?`.
- Calling `transition()` inside a handler does not automatically
  short-circuit — the handler continues. To return immediately after a
  transition: `transition(id, "X"); return map("status", "X")`.
- Recursion is allowed but capped at depth 512 (interpreter limit).
  See [[termination]] for the analysis.

## What this does NOT cover

- Async handlers / await — not in V1.5. All builtins are synchronous.
  See [[whats-missing]].
- Capability checks on `delegate` — any handler can call any other
  cell's handler today. Security model is open work.
