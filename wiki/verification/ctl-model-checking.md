---
name: ctl-model-checking
description: Temporal properties (eventually, never, after) on state machines, verified at compile time.
type: feature
since: V1.0
related: [state-machine, verification-overview, think-isolation, coq-scorecard]
---

# CTL model checking

`soma verify` includes a model checker for **Computation Tree Logic**
(CTL) properties on each cell's state machine. The properties are
declared in `soma.toml` (or `[verify]` blocks alongside the cell) and
exhaustively explored at compile time.

## Property syntax (in `soma.toml`)

```toml
[verify]
deadlock_free = true                         # no state can deadlock
eventually = ["settled", "cancelled"]        # every path reaches one of these
never = ["invalid"]                           # no state ever equals this

[verify.after.executed]
never = ["cancelled"]                         # after `executed`, never `cancelled`
eventually = ["settled", "failed"]            # after `executed`, eventually one of these
```

Supported keys:

- `deadlock_free` — every reachable state has at least one outgoing
  transition (or is explicitly terminal).
- `eventually = ["X", "Y", ...]` — from every reachable state, every
  path reaches a state in the list.
- `never = ["X", "Y", ...]` — no reachable state matches the list.
- `after.S` — properties that hold once state `S` is reached.

## What the checker does

For each property, the model checker:

1. Builds the state graph from the `state { … }` block.
2. Performs exhaustive reachability / fixpoint analysis over the
   graph.
3. Emits ✓ on proof, ✗ with a **counter-example trace** on failure.

Example success output:

```
✓ deadlock_free — no deadlocks in any reachable state
✓ eventually(settled | cancelled) — every reachable state reaches one
✓ never(invalid) — invalid is unreachable
```

Example failure with counter-example:

```
✗ eventually(settled) — counter-example:
  queued → running → failed → queued → running → failed → ... (cycle)
```

The cycle means there's a reachable trajectory that never reaches
`settled` — the failure condition for `eventually`.

## Common properties for production cells

```toml
# A trade lifecycle:
[verify]
deadlock_free = true
eventually = ["settled", "cancelled"]

# After execution, no rollback:
[verify.after.executed]
never = ["cancelled", "pending"]
eventually = ["settled"]

# A risk system:
[verify]
eventually = ["passed", "blocked", "needs_review"]
never = ["unknown"]

# An agent workflow:
[verify]
deadlock_free = true
eventually = ["done", "failed"]
```

## How exhaustive is "exhaustive"?

For finite state machines (always the case in Soma — states are a
finite set), the model checker explores every reachable state. Loops
are handled by fixpoint convergence. The complexity is polynomial in
the size of the graph; in practice all real Soma cells (≤ tens of
states) check in milliseconds.

## Coq backing

`Soma_CTL.v` proves:

- **Safety properties** (`never`, `mutex`, `deadlock_free`) — sound and
  complete on the abstract state machine.
- **Liveness** (`eventually`, `after`) — sound after a depth-bound
  fix discovered during the rigor pass.

50+ theorems in `docs/rigor/coq/`, all closed under the global
context. See [[coq-scorecard]].

## How CTL combines with refinement

The CTL check operates on the **declared** state machine (the
picture). The [[refinement]] check operates on the **handler bodies**
(the code). Together:

- CTL proves the picture has the right shape.
- Refinement proves the handlers can only produce transitions that
  fit the picture.

Without refinement, CTL is decorative — the handlers could ignore the
picture entirely. Without CTL, refinement is shallow — it doesn't
check whether the picture itself satisfies the desired properties.

Both are necessary; both are now V1.5-shipped.

## Examples

A minimal lifecycle:

```soma
cell Pipeline {
    state run {
        initial: pending
        pending -> running
        running -> done
        * -> failed
    }
    on start() { transition("inst", "running") }
    on finish() { transition("inst", "done") }
}
```

```toml
[verify]
deadlock_free = true
eventually = ["done", "failed"]
```

`soma verify` output:

```
State machine 'run': 4 states, initial 'pending'
  ✓ no deadlocks
  ✓ liveness: every state can eventually reach a terminal state
  ✓ wildcard transitions: * -> [failed]

Temporal properties for 'run':
  ✓ deadlock_free
  ✓ eventually(done | failed)
```

A state machine that *fails* a property:

```soma
state buggy {
    initial: pending
    pending -> running
    running -> running          // cycle without exit
}
```

`soma verify`:

```
✗ liveness violation: states [pending, running] cannot reach any terminal state
  trace: pending → running → running → running → ...
```

## Edge cases

- A state with no outgoing transitions is **terminal** by definition.
  Liveness properties target the set of terminal states.
- Wildcard transitions (`* -> X`) are expanded to "from every
  non-terminal state".
- Guards on transitions are **ignored** by CTL (they're runtime
  conditions, not graph topology). Source-state legality is V1.6+.

## What this does NOT cover

- **Cross-cell temporal properties.** "After cell A signals X, cell B
  eventually transitions to Y" — not in V1.5. Composition checks are
  per-cell.
- **Real-time properties.** "X happens within 30s" — Soma has no
  timed automata.
- **Stochastic CTL.** No probability annotations.
