---
name: try
description: `try { expr }` catches runtime errors and returns a Map; `?` propagates them.
type: feature
since: V1.0
related: [ensure, handler]
---

# `try` and `?`

`try { expr }` evaluates `expr` and:

- on success → returns `map("value", result, "error", ())`
- on failure → returns `map("value", (), "error", "message")`

```soma
let r = try { risky_operation() }
if r.error != () {
    log_error(r.error)
    return response(500, map("error", r.error))
}
let value = r.value
```

## `?` — propagate on error

`try { expr }?` is shorthand for "unwrap if OK, return-with-error if
not":

```soma
let value = try { risky_operation() }?
// If risky_operation failed, this handler returns its error map.
// Otherwise, `value` is the success value.
```

The `?` operator turns a possibly-failing expression into a
short-circuit: in the success case the handler continues; in the
failure case the handler returns immediately.

## What `try` catches

Any of these will be caught and rendered as an error string:

- `ensure` failures (postcondition violations) — see [[ensure]]
- Type errors (e.g. `to_int("abc")` then `.get()` on the result)
- Division by zero
- Undefined variables
- Stack overflow
- Invalid state machine transitions
- Builtin errors (`from_json("bad")`)

What `try` does **not** catch:

- `transition()` calls — they happen for real even if a later `ensure`
  fires.
- Side effects already executed inside `try { … }` (memory writes,
  emits) are not rolled back.

## Examples

Defensive parsing:

```soma
on parse_request(body: String) {
    let r = try { from_json(body) }
    if r.error != () {
        return response(400, map("error", "invalid JSON: {r.error}"))
    }
    let data = r.value
    process(data)
}
```

Pre-trade reject pattern:

```soma
on submit(qty: Float, vol: Float, sigma: Float) {
    let imp = impact_sqrt(qty, vol, sigma, map())
    ensure imp.bps <= 30.0
    ensure qty <= max_qty()
    emit place_order(qty)
}

on attempt(qty: Float, vol: Float, sigma: Float) {
    let r = try { submit(qty, vol, sigma) }
    if r.error != () {
        return map("status", "rejected", "reason", r.error)
    }
    map("status", "submitted")
}
```

Propagating with `?`:

```soma
on full_pipeline(input: Map) {
    let cleaned = try { sanitize(input) }?
    let scored  = try { score(cleaned) }?
    let saved   = try { persist(scored) }?
    map("status", "done", "id", saved.id)
}
```

If any step ensure-fails or throws, the handler returns the error
map. Reads top-to-bottom.

## Error-message format

When an `ensure` fails inside `try`, the error includes the span:

```
require failed: ensure postcondition failed
  --> file.cell:42:9
```

When a builtin fails, the message is the builtin's own diagnostic:

```
type error: to_int("abc"): not a valid integer literal
```

## `?? Map<...>` — null coalescing

A related operator: `??` provides a default for `Unit`:

```soma
let name = user.get("name") ?? "anonymous"
let port = to_int(config.get("port") ?? "8080")
```

`??` is not `try`/`?` — it handles `Unit` (Soma's null), not errors.
Use it when you have a missing-field case and a sensible default.

## Edge cases

- `try { expr }` returns a Map even on success. Always unwrap with
  `.value`. Don't compare to the original.
- `?` only works inside a handler — it generates a `return` for the
  enclosing handler. Calling it from inside a lambda is not supported.
- An `ensure` that fires inside a *nested* `try { try { … } }`
  propagates to the *innermost* `try`. The outer try sees a successful
  value (the inner error map).

## What this does NOT cover

- **Typed error variants.** When `Result<T, E>` lands in V1.5.1
  ([[sum-types]] roadmap), `try` becomes structurally typed. Today
  the error is always a String.
- **Resource cleanup** — Soma has no `finally` or RAII. Use explicit
  cleanup before `return`.
