---
name: pattern-matching
description: `match` expressions with literal, variable, destructuring, range, and variant patterns.
type: feature
since: V1.0
related: [sum-types, match-as-expression]
---

# Pattern matching

`match` is Soma's primary branching construct. It evaluates a subject
expression, picks the first arm whose pattern matches, and yields
the arm's result expression.

```soma
match value {
    "a"               -> 1
    "b"               -> 2
    42                -> 3
    ()                -> 4                      // match Unit
    "x" || "y"        -> 5                       // or-pattern
    name              -> use(name)               // variable binding
    "/api/" + rest    -> api(rest)               // string prefix
    {method, path}    -> dispatch(method, path)  // map destructure
    _                 -> default                  // wildcard
}
```

## Pattern grammar

- **Literal**: `42`, `"hello"`, `3.14`, `true`, `()`
- **Negative number**: `-5`, `-3.14`
- **Variable** (binds the matched value): `name`
- **Wildcard**: `_`
- **Or-pattern**: `"a" || "b" || "c"` (note: `||` not `|`)
- **Range**: `1..10` matches integers in [1, 10)
- **Map destructure**: `{key: pattern, key2}` (`key2` is shorthand for
  `key2: key2`)
- **String prefix**: `"prefix" + rest` matches and binds `rest`
- **Variant**: `Pending`, `Up(n)`, `Charged { id, .. }` — see [[sum-types]]

Guards extend any pattern:

```soma
match score {
    n if n >= 90 -> "A"
    n if n >= 80 -> "B"
    n if n >= 70 -> "C"
    _            -> "F"
}
```

## Match as expression

`match` returns a value:

```soma
let label = match status {
    "ok"    -> "alright"
    "error" -> "failed"
    _       -> "unknown"
}
```

Each arm can have a block of statements before the result expression:

```soma
match input {
    {kind: "trade", amount} -> {
        log_trade(amount)
        validate(amount)
    }
    _ -> response(400, map("error", "unknown"))
}
```

## Map destructure with nested patterns

The most common production pattern is HTTP request dispatch:

```soma
on request(method: String, path: String, body: String) {
    let req = map("method", method, "path", path)
    match req {
        {method: "GET",    path: "/"}                  -> home()
        {method: "GET",    path: "/api/" + r}          -> list(r)
        {method: "POST",   path: "/api/" + r}          -> create(r, body)
        {method: "DELETE", path: "/api/" + r}          -> remove(r)
        n if n.method == "OPTIONS"                      -> cors()
        _                                               -> response(404, map("error", "not found"))
    }
}
```

## Range patterns

```soma
match http_status {
    200..299 -> "success"
    300..399 -> "redirect"
    400..499 -> "client error"
    500..599 -> "server error"
    _        -> "unknown"
}
```

Ranges are half-open on the integer side; `200..299` matches 200
through 298 inclusive. (TODO: verify whether the upper bound is
inclusive or exclusive in the runtime; treating the ambiguity here
defensively.)

## Variant patterns (V1.5)

See [[sum-types]] for the full grammar. Quick reference:

```soma
match result {
    Pending                              -> "waiting"
    Charged { transaction_id, amount }   -> "ok"
    Declined { code: 503, .. }           -> "retry"
    Up(n) if n > 0                       -> "north {n}"
}
```

- `Pending` — unit variant, no fields
- `Charged { id, amount }` — struct variant, both fields bound
- `Charged { code: 503, .. }` — match `code == 503`, ignore other fields
- `Up(n)` — tuple variant, binds the inner value

## Or-patterns

Note: Soma uses `||`, not `|`, for or-patterns (to avoid conflict with
the bitwise / type-syntax `|`):

```soma
match x {
    "open" || "active" || "running" -> "live"
    "closed" || "done"               -> "finished"
    _                                  -> "unknown"
}
```

In variant patterns, `||` works the same way but may need separate
arms in current syntax:

```soma
match move {
    Up(n)   -> "north {n}"
    Down(n) -> "south {n}"        // currently can't or-pattern Up || Down
    Wait     -> "idle"
}
```

## Exhaustiveness check

For [[sum-types]] subjects, the compiler verifies every variant is
covered or a wildcard catches the rest. Missing arm:

```
error: non-exhaustive match on 'OrderStatus': missing variant `Cancelled`
```

This is one of the few static type-system properties Soma has;
extending it to other types (e.g. `Bool`'s `true`/`false`) is open
work.

## Edge cases

- Match arms are checked **top to bottom**. The first match wins.
- A bare lowercase identifier in pattern position is **always a
  variable binding**, never a constant. Capital-first names are
  variant patterns (matched against the variant registry).
- `_` is special: it's a wildcard, not a binding. To bind everything
  use `name`.
- An empty match (no arms) returns `Unit` and is legal but useless.

## What this does NOT cover

- **Nested variant patterns in records** — works but underdocumented.
- **List patterns** like `[head, ...tail]` — not in V1.5.
- **`@` bindings** (`Pending @ s -> use(s)`) — not in V1.5.
