---
name: refinement
description: V1.3 — handler bodies cannot lie to the state machine.
type: feature
since: V1.3
related: [state-machine, sum-types, verification-overview, ctl-model-checking]
---

# Refinement (V1.3)

The **refinement** check closes the gap between the `state { }` block
(the spec) and the handler bodies (the code). Before V1.3, the
verifier proved CTL properties about a *picture* of the state machine
and trusted that handlers actually called `transition()` consistently
with the picture. They could lie. V1.3 closes that.

## What it proves

For every cell with a state machine:

1. **Every `transition("inst", "X")` call** in any handler body must
   name a state `X` that exists in the cell's `state { }` block. Typo
   → compile error.

2. **Every transition declared in the state block** must be reached
   by at least one `transition()` call from some handler. Dead
   transitions in the spec → warning.

3. **Per-handler effect summary.** For each handler, the verifier
   computes the set of target states it can reach, with the path
   conditions (`if` guards) leading to each call. Surfaced in
   `soma verify` output:

```
✓ refinement: handler `rebalance` ⟶ {
    signal_pending,
    failed [if alpha_cfg != () ∧ alpha_result.error != ()],
    blocked [if verdict == "BLOCK"],
    approved [if verdict == "APPROVE"],
    flagged
}
```

## V1.5 typed extension

With [[sum-types]] state annotation `state order: OrderState { … }`,
the check additionally verifies:

- Every state name in the `state` block is a variant of `OrderState`.
- Every `transition(id, X)` call uses a valid variant of `OrderState`.

A typo in a variant name is a compile error citing the type:

```
error: handler 'advance' transitions to 'Validtaed', which is not a variant of 'OrderState'
```

Before V1.5, the error would say "undeclared state Validtaed" — the
*type* wasn't part of the message because there was no type.

## What it does NOT prove (V1.3 scope)

- **Source-state correctness.** A handler doesn't know which state the
  machine was in when it was called. The check only validates the
  *target* of every `transition()` — not that the call is legal from
  the current state. Catching that requires SMT-backed symbolic
  execution.

- **Guard implication.** If the state block says
  `pending -> authorized when amount > 0`, and the handler writes
  `if amount <= 0 { return }  transition("t", "authorized")`, V1.3
  doesn't try to prove the handler's path condition implies the
  state-machine's guard. Both are recorded as text and displayed.

- **Dynamic targets.** `transition(id, target_var)` with a runtime
  variable can't be statically analyzed. V1.3 records
  `has_dynamic_target = true` and emits a warning so the user knows
  the strong check didn't fire on that handler.

## Examples

A clean cell — full refinement:

```soma
cell type OrderState { variants { Pending; Validated; Filled } }

cell Order {
    state order: OrderState {
        initial: Pending
        Pending -> Validated -> Filled
    }

    on submit(id: String) { transition(id, Validated) }
    on fill(id: String)   { transition(id, Filled) }
}

// soma verify:
// ✓ refinement: handler `submit` ⟶ {Validated}
// ✓ refinement: handler `fill`   ⟶ {Filled}
```

A cell with a dynamic target (downgrades the check):

```soma
on advance(id: String, target_state: String) {
    transition(id, target_state)        // dynamic — escapes strong refinement
}

// soma verify:
// ⚠ refinement: handler `advance` has dynamic target — strong check skipped
```

A typo (compile-error path):

```soma
on submit(id: String) {
    transition(id, "Validtaed")          // typo
}

// soma verify:
// ✗ undeclared target: handler `submit` calls transition with 'Validtaed'
//   declared states are: pending, validated, filled
```

## Why this matters

Before V1.3, "the specification is the program" was half-true: the
spec block was decorative, the handler bodies were the truth. Now the
spec block has compile-time enforcement. Drift between them is a
build failure.

In Coq:

- `Soma_Refinement.v` proves the syntactic refinement theorem on the
  abstract state machine.
- With the V1.5 typed extension, `Soma_RefinementSum.v` (designed,
  not yet landed) parameterises the proof over a state type.

## Implementation

`compiler/src/checker/refinement.rs`. The walker is a syntactic
analysis over each handler's AST that:

1. Collects `Expr::FnCall { name: "transition", args }` instances.
2. Classifies each as literal-target or dynamic.
3. Pushes path conditions (`if cond`, `match arm`) onto a stack as it
   walks.
4. Records each transition with its target and path.

The whole pass is O(handler size) per cell. No fixpoint, no SMT.

## What this does NOT cover

- SMT-backed source-state and guard reasoning (V1.6+).
- Inter-cell refinement — when `delegate("Other", "sig")` triggers a
  state change in another cell, neither side's refinement check sees
  the full chain.
