---
name: ensure
description: Runtime-checked postcondition with verification semantics — the Soma idiom for typed preconditions.
type: feature
since: V1.0
related: [handler, try, verified-pretrade]
---

# `ensure`

`ensure expr` is a runtime-checked postcondition. If `expr` evaluates
to falsey, the handler fails immediately with a `RequireFailed` error.

```soma
on withdraw(balance: Int, amount: Int) {
    let result = balance - amount
    ensure result >= 0           // fails if false
    return result
}
```

This is the Soma idiom for **invariants that the type system cannot
express**. Combined with `try { … }` and the `?` operator at the call
site, it gives you a structured-failure path that the [[budget-proof]]
and [[refinement]] checkers understand.

## Semantics

- `ensure cond` short-circuits: if `cond` is true, execution
  continues. If false, the handler aborts and returns an error map
  to the caller (via `try`) or propagates upward.
- `ensure` is **not** caught by an enclosing `try { }` in the *same*
  handler; it bypasses the local try-block and returns the error to
  the caller.
- An `ensure` failure includes the source span in the error message:
  `require failed: ensure postcondition failed at file.cell:42:9`.

## The "verified pre-trade" pattern

Where `ensure` shines: turning an empirical / domain model into a
compile-time-shaped, runtime-enforced contract.

```soma
on submit_order(qty: Float, daily_vol: Float, sigma: Float) {
    let imp = impact_sqrt(qty, daily_vol, sigma, map("Y", 1.0))
    ensure imp.bps <= 30.0         // Bouchaud square-root law as a precondition

    let var = var_historical(returns_for(symbol), map("alpha", 0.99))
    ensure var <= 0.05              // fat-tail VaR cap

    emit place_order(qty)
}
```

The handler **cannot** reach `emit place_order` unless both ensures
hold. The model (`impact_sqrt` from Tóth–Lemperière–Bouchaud,
`var_historical` from Bouchaud-Potters) is empirical, but the
**precondition is enforced**.

Caller side:

```soma
on attempt(qty: Float, vol: Float, sigma: Float) {
    let r = try { submit_order(qty, vol, sigma) }
    if r.error != () {
        log_rejection(r.error)
        return map("status", "rejected", "reason", r.error)
    }
    map("status", "submitted")
}
```

See [[verified-pretrade]] for the full case study and
`examples/risk_check.cell`.

## Why this matters

The standard programming-language idiom for "this condition must hold"
is one of:

- `assert` — compiled away in release builds.
- An `if … return error`, hand-rolled per call site.
- A type that the compiler enforces (refinement types, dependent
  types) — high effort.

`ensure` is the middle path: cheap at the call site, always-on,
structured-error-on-failure, model-checked by the [[refinement]]
output. Soma's verification machinery treats `ensure` as a load-bearing
construct.

## Examples

Domain invariant:

```soma
on add_item(items: List, item: Map) {
    let result = push(items, item)
    ensure len(result) == len(items) + 1
    return result
}
```

Multi-step gate:

```soma
on update_balance(account: String, delta: Int) {
    let current = balances.get(account) ?? "0"
    let new_bal = to_int(current) + delta
    ensure new_bal >= 0
    ensure new_bal <= max_balance()
    balances.set(account, to_string(new_bal))
}
```

Empirical model as gate (the Bouchaud square-root):

```soma
on place_order(qty: Float, vol: Float, sigma: Float) {
    let imp = impact_sqrt(qty, vol, sigma, map("Y", 1.0))
    ensure imp.bps <= 30.0
    submit_to_broker(qty)
}
```

## `try { }` and the `?` operator

`try { expr }` catches `ensure` failures (and other runtime errors)
and returns a Map with `value` or `error`:

```soma
let r = try { submit_order(qty, vol, sigma) }
if r.error != () {
    print("rejected: {r.error}")
    return map("status", "rejected")
}
let result = r.value
```

`?` is the short form — propagate the error if there is one,
otherwise unwrap:

```soma
let result = try { submit_order(qty, vol, sigma) }?
```

If `submit_order` ensure-failed, `?` returns the error map to the
caller of the *current* handler.

## Edge cases

- `ensure` evaluates its argument every time. Side-effects inside the
  expression run before the check.
- An `ensure` after a successful `transition()` does not roll back
  the transition — the state change has already happened. Order
  matters.
- `ensure (a && b)` and `ensure a; ensure b` differ only in error
  messages (the first reports the conjoined expression; the second
  reports which clause failed).

## What this does NOT cover

- Static verification of `ensure` predicates. SMT integration would
  let the compiler *prove* the ensure always holds, eliminating the
  runtime cost. See [[whats-missing]].
- Capability scoping — `ensure` doesn't check permissions; only
  conditions.
