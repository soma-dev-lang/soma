---
name: verification-overview
description: What `soma check` / `soma verify` actually prove about a cell. The four classes of guarantee.
type: synthesis
since: V1.0
related: [ctl-model-checking, refinement, budget-proof, think-isolation, termination, composition, coq-scorecard]
---

# Verification overview

Soma's pitch is **the compiler is the supervisor**. This page enumerates
exactly what's proven, when, and under what adversary model.

## The four classes

1. **Temporal (CTL) properties on state machines.**
   `soma verify` does model-checking on the `state` block. Proves
   `deadlock_free`, `eventually(X | Y)`, `after(A, never B)`, etc.
   See [[ctl-model-checking]].

2. **Refinement: handler bodies match the spec.**
   V1.3. Every `transition()` call in any handler targets a declared
   state. With V1.5 [[sum-types]] annotation, targets are checked
   against a type. See [[refinement]].

3. **Memory budget proofs.**
   V1.4. If a cell declares `scale { memory: "128Mi" }`, the compiler
   proves peak ≤ declared, or fails with a breakdown, or downgrades to
   advisory. See [[budget-proof]].

4. **Think-isolation: safety holds regardless of LLM output.**
   When every transition target is a literal (no dynamic
   `transition(id, var)`), the CTL safety theorem in
   `Soma_Isolation.v` proves the state-machine cannot be broken by an
   adversarial LLM. See [[think-isolation]].

Two more, supporting:

5. **Termination.** Every handler structurally terminates (bounded
   loops, decreasing recursion). See [[termination]].

6. **Composition.** Every `await` has a matching `emit` and vice
   versa, with arity-compatible signal types. See [[composition]].

## What runs when

`soma check`:
- Structural checks (face contracts, memory properties, scale consistency)
- Refinement (V1.3)
- Budget proof (V1.4)
- Sum-type exhaustiveness + state-type checks (V1.5)
- Custom checkers (user-defined `cell checker`s)

`soma verify`:
- All of `soma check`
- CTL model checking on every state machine
- Termination check
- Think-isolation analysis
- Composition checks across interior cells

Production-ish runtime check:
- `[record]` handlers append to `.somalog` for replay
- `soma replay` is bit-deterministic against the log

## What's verified mechanically (Coq)

`docs/rigor/coq/` contains 50+ theorems and lemmas. Closed under the
global context (zero axioms). Top-level results:

- **`Soma_CTL.v`** — CTL safety and liveness are sound on the
  abstract state machine.
- **`Soma_Isolation.v`** — when transition targets are literal,
  safety holds regardless of LLM output (think-isolation).
- **`Soma_RuntimeFidelity.v`** — the runtime preserves the safety
  invariant when guards block prohibited transitions.
- **`Soma_Budget.v`** + **`Soma_BudgetOps.v`** — cost lattice laws
  and per-builtin allocation bounds are sound.
- **`Soma_Refinement.v`** (V1.3) — handler-body refinement theorem.

See [[coq-scorecard]] for the full list and known gaps.

## What's NOT proven (honestly)

- **Refinement→implementation gap.** The Coq proofs are about the
  abstract state machine and the cost lattice, not about the running
  Rust interpreter. The CompCert-style end-to-end chain doesn't
  exist; we trust the interpreter implementation by inspection.
- **Concurrent semantics across cells.** Inter-cell signal ordering
  lacks a formal CSP-level spec. The temporal properties are
  *per-cell*. See [[whats-missing]].
- **Source-state correctness.** A handler doesn't know which state the
  machine was in when it was called; refinement checks targets, not
  legality of the call from the current state. SMT-backed source
  reasoning is V1.6+.
- **Named consensus protocol.** `consistency: strong` doesn't specify
  Raft / Paxos / etc. as the underlying algorithm.
- **Effects in the type system.** `think()` and `http_get()` are
  effects; the type system doesn't track them. See [[whats-missing]].

## How adversary models are documented

`docs/ADVERSARIES.md` enumerates each property and its adversary:

- **Think-isolation** — adversary: arbitrary LLM output and arbitrary
  tool-call response.
- **Refinement** — adversary: handler bodies (assumed to compile but
  with arbitrary control flow).
- **Budget** — adversary: arbitrary input data within declared
  bounds.

Every claim names what it does and doesn't survive. That's the spirit:
verify what you can, document what you can't.

## Example: a fully-verified cell

```soma
cell Rebalancer {
    scale { memory: "256Mi" }

    memory { trades: Map<String, String> [persistent, consistent, capacity(10000)] }

    state trade {
        initial: pending
        pending -> validated -> settled
        * -> failed
    }

    on submit(d: Map) {
        ensure to_int(d.qty) > 0
        let id = "T-" + to_string(next_id())
        trades.set(id, to_json(d))
        transition(id, "validated")
        map("id", id)
    }

    on settle(id: String) {
        transition(id, "settled")
    }
}
```

`soma verify` output:

```
✓ no deadlocks
✓ eventually(settled | failed)
✓ refinement: handler `submit`  ⟶ {validated}
✓ refinement: handler `settle`  ⟶ {settled}
✓ think-isolated: CTL safety holds regardless of LLM output
   (2 handlers, 2 literal transitions, 0 dynamic)
✓ termination: all 2 handlers structurally terminate
✓ budget proven for cell 'Rebalancer': peak ≤ 38.21 MiB ≤ 256.00 MiB
Temporal: 1 passed, 0 failed
```

Six classes of guarantee in one verify run.

## What to read next

- [[ctl-model-checking]] — how state machines are model-checked
- [[refinement]] — V1.3 spec-matches-code
- [[budget-proof]] — V1.4 cost lattice
- [[think-isolation]] — V1.0 + adversary model
- [[coq-scorecard]] — what's mechanically verified
