---
name: state-machine
description: Explicit lifecycle that the compiler model-checks for temporal properties.
type: concept
since: V1.0
related: [cell, handler, refinement, ctl-model-checking, sum-types]
---

# State Machine

A cell can declare an explicit state machine — a named graph of states
and transitions that instances move through. The compiler:

1. Reads the graph as a spec.
2. Verifies CTL temporal properties on it ([[ctl-model-checking]]).
3. Checks that every handler's `transition()` calls land on a declared
   target ([[refinement]]).
4. With V1.5 [[sum-types]] annotations, checks that every state name is
   a variant of a declared sum type.

Together: the state machine is **not just a picture next to the code**.
It is checked.

## Basic syntax

```soma
cell Order {
    state order {
        initial: pending
        pending -> validated
        validated -> filled
        filled -> settled
        * -> cancelled              // wildcard: from any state
    }

    on submit(id: String) { transition(id, "validated") }
    on fill(id: String)   { transition(id, "filled") }
    on cancel(id: String) { transition(id, "cancelled") }
}
```

State names are bare identifiers. The compiler builds the universe of
valid state names from `initial:`, every `from` (except `*`), and
every `to`.

## Typed state machines (V1.5)

If a `cell type` declares the state set as a sum type, the state block
can be annotated with it:

```soma
cell type OrderState {
    variants { Pending; Validated; Filled; Cancelled }
}

cell Order {
    state order: OrderState {        // ← annotation
        initial: Pending
        Pending -> Validated
        Validated -> Filled
        * -> Cancelled
    }

    on submit(id: String) {
        transition(id, Validated)    // typed; typo = compile error
    }
}
```

Now `transition(id, Shipd)` is a compile error referencing the type:

```
error: handler 'submit' transitions to 'Shipd', which is not a variant of 'OrderState'
```

See [[sum-types]] for the full story.

## Guards and effects on transitions

```soma
state order {
    initial: pending
    pending -> validated { guard { amount > 0 } }
    validated -> filled  { effect {
        emit fill_event(id)
        record_fill(id)
    } }
}
```

- `guard { expr }` — must be true for the transition to proceed.
- `effect { stmts }` — runs after a successful transition.

Both are inert at the language level today; the runtime ignores them
in V1.5. They are *declarative spec*. The intent is V1.6+ SMT
integration that proves handler path-conditions imply the guard.
See [[whats-missing]].

## Transition annotations

State blocks can carry `[max_instances(N)]` for the [[budget-proof]]:

```soma
state trade [max_instances(50000)] {
    initial: queued
    queued -> confirmed -> executed -> settled
}
```

The checker bounds memory used by tracking up to 50k live instance
IDs.

## Chained transitions

`a -> b -> c -> d` desugars to three transitions:

```soma
state foo {
    initial: a
    a -> b -> c -> d         // same as: a -> b, b -> c, c -> d
}
```

## What it gets you

Three properties the compiler verifies:

1. **Reachability** — every state declared is reachable from `initial`
   (`soma verify`).
2. **Liveness** — properties like `eventually(settled)` are
   model-checked exhaustively over the graph.
3. **Refinement** — every `transition(id, X)` call in any handler must
   target a declared state. With sum-type annotation, X must be a
   variant of the declared type. See [[refinement]].

In `soma.toml`:

```toml
[verify]
deadlock_free = true
eventually = ["settled", "cancelled"]

[verify.after.executed]
never = ["cancelled"]
eventually = ["settled", "failed"]
```

## Examples

The rebalancer's state machine (15 states):

```soma
state rebalance {
    initial: requested
    requested -> signal_pending
    signal_pending -> optimizing
    signal_pending -> failed
    optimizing -> compliance_pending
    compliance_pending -> approved
    compliance_pending -> flagged
    compliance_pending -> blocked
    approved -> commentary_pending -> finalized
    flagged -> human_review -> approved
    flagged -> human_review -> denied
    * -> failed
}
```

Pairs with `[verify] eventually = ["finalized", "denied", "failed"]`
to prove every rebalance reaches a terminal state.

## Edge cases

- The wildcard `*` does NOT include the target itself (no self-loop
  unless explicit).
- A state with no outgoing transitions is **terminal**. Liveness
  properties target the set of terminal states.
- `transition(id, dynamic_var)` — runtime-computed targets escape
  refinement. The verifier reports them as "dynamic" and weakens
  the per-handler effect summary. See [[refinement]].

## What this does NOT cover

- Source-state checking. A handler doesn't know which state the
  machine was in when it was called; the refinement check only
  validates the *target* of every `transition()`. SMT-based source
  reasoning is V1.6+.
- Guard implication proof — the V1.3 refinement records guards as
  text but doesn't try to prove handler conditions entail them.
