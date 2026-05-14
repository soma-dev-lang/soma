---
name: coq-scorecard
description: What's mechanically verified in Coq, what's trusted by inspection, and the gaps.
type: reference
since: V1.4
related: [verification-overview, refinement, budget-proof, think-isolation, ctl-model-checking, whats-missing]
---

# Coq scorecard

`docs/rigor/coq/` contains the mechanical proofs backing Soma's
verification claims. This page enumerates exactly what's proven, what's
trusted, and what's still asterisked.

Reproduce all proofs: `make -C docs/rigor/coq check`.

## What's mechanically proven

### `Soma_CTL.v` — CTL safety and liveness

**Safety properties** (`deadlock_free`, `never`, `mutex`):
- **Sound and complete** on the abstract state machine.
- Counter-examples in the model checker correspond to actual paths.

**Liveness properties** (`eventually`, `after`):
- **Sound** after a depth-bound fix discovered during the rigor pass.
- Incompleteness intentional — depth bound is configurable.

### `Soma_Isolation.v` — think-isolation

If all `transition()` calls use literal target states, CTL safety
holds **regardless of what the LLM returns from `think()`**.
- Includes the tool-call side channel: tool handlers' transitions
  are also required to be literal.
- The reachable state set at runtime equals the source-graph
  reachable set.

### `Soma_RuntimeFidelity.v` — runtime preserves safety

The end-to-end chain from "runtime guards transitions against the
declared graph G" to "the safety invariant holds on the runtime
trace" is mechanised.
- No unproven gap in the chain.
- This is the *hard part* of the abstraction theorem — bridging the
  abstract state machine to the operational semantics.

### `Soma_Budget.v` + `Soma_BudgetOps.v` — memory budget

- Cost lattice laws (associativity, commutativity, monotonicity) are
  proven.
- The `plus` / `max` operations on `Cost::Bytes`, `Cost::Unbounded`,
  and so on are sound.
- Sum-of-allocations stays inside declared bounds.

### `Soma_Refinement.v` — V1.3 refinement

Handler-body refinement theorem: every `transition()` call with
literal targets matches the declared state machine.

## What's verified by handler structure (in the compiler, not Coq)

These checks are syntactic passes, but their **proof obligation** is
in Coq:

- **Termination.** Every handler is structurally bounded:
  - `for var in expr` has `[loop_bound(N)]` or a literal-range iterator.
  - `while cond` requires `[loop_bound(N)]`.
  - Recursion is depth-capped at 512.
  - The checker rejects unbounded loops.
- **Composition.** Every `emit` has a matching `on handler`; every
  `await` has a matching emitter. Cross-cell signal types are
  arity-compatible.

The static checks are correct by construction; the soundness of the
*model* they enforce is in `Soma_CTL.v`.

## What's trusted by inspection (not yet in Coq)

These items work but have an asterisk:

### V1.4 per-builtin cost assignments

`Soma_BudgetOps.v` proves the cost lattice is sound. The mapping from
"this expression / this builtin call" to "this Cost value" is a Rust
AST walker — currently trusted by inspection, not mechanised.

**Implication:** if the walker is wrong (e.g. misses a hidden
allocation in `from_json`), the proven bound is wrong. The walker is
short (~500 LOC, `compiler/src/checker/budget.rs`) and reviewed but
not formally connected to the Coq proof.

### V1.5 sum-type checks

The exhaustiveness check (`sum_types.rs`) and the typed-state-machine
refinement extension (`refinement.rs` typed-target branch) are
trusted by inspection. The Coq proofs (`Soma_SumTypes.v`,
`Soma_RefinementSum.v`) are **designed** in `SUM_TYPES_DESIGN.md`
but not yet landed.

### V1.5 linalg cost rules

The cost rules for `regress_sgd`, `svd_lowrank`, `clean_covariance`,
etc. (in `budget.rs`) are read by inspection. The bounds are
straightforward (sum-of-vectors × 8 bytes per float), but the
soundness isn't tied to a Coq theorem.

### Coq → Rust mapping

There's **no CompCert-style mechanical link** from the verified Coq
proofs to the running Rust interpreter. The proofs assume an abstract
operational semantics (`Soma_RuntimeFidelity.v`) that the interpreter
is *believed* to implement. The interpreter is trusted by inspection.

This is a real gap. Closing it would require either:
- Extracting an OCaml/Rust runtime from Coq (CompCert-style), or
- Writing a relational proof between Coq's semantics and the Rust
  source.

Neither is in scope for V1.5.

## Top-level theorem statements

`docs/SOUNDNESS.md` is the long-form companion to this scorecard.
Each theorem has:

- A precise English statement.
- The adversary model (`docs/ADVERSARIES.md`).
- The Coq theorem name + file.
- The compiler check that enforces the precondition.

## Zero axioms

Every proof file ends with `Closed under the global context.` — i.e.
no axioms beyond the Coq standard library.

## How big is the proof base

As of V1.5:

- **6 proof files**: `Soma_CTL.v`, `Soma_Isolation.v`,
  `Soma_RuntimeFidelity.v`, `Soma_Budget.v`, `Soma_BudgetOps.v`,
  `Soma_Refinement.v`.
- **50+ theorems and lemmas** across them.
- Roughly **3000 lines** of Coq.

## What's missing for "fully verified"

1. **Refinement→implementation gap.** No CompCert link.
2. **Per-builtin cost is trusted.** Walker correctness not proven.
3. **Sum-type proofs.** Designed, not landed (V1.6 target).
4. **Inter-cell composition theorems.** Per-cell CTL is proven; the
   cross-cell signal-bus semantics has no Coq spec.
5. **Effects in the type system.** Coq doesn't model `think()` /
   `http_get()` as effects.

See [[whats-missing]] for the full set of open work items.

## Implementation files

```
docs/rigor/coq/
├── Soma_CTL.v
├── Soma_Isolation.v
├── Soma_RuntimeFidelity.v
├── Soma_Budget.v
├── Soma_BudgetOps.v
├── Soma_Refinement.v
├── Makefile                  # `make check` rebuilds and verifies all
└── README.md                 # top-level rigor scorecard
```

## Honest cut

The V1.5 Coq scorecard:

| Theorem | Mechanised | Implementation trusted by |
|---|---|---|
| CTL safety + liveness | ✓ `Soma_CTL.v` | ✓ proof drives the checker |
| Think-isolation | ✓ `Soma_Isolation.v` | ✓ syntactic check matches the proof |
| Runtime fidelity | ✓ `Soma_RuntimeFidelity.v` | ⚠ Rust interpreter trusted by inspection |
| Budget (cost lattice) | ✓ `Soma_Budget.v`, `Soma_BudgetOps.v` | ⚠ per-builtin rules trusted |
| Refinement (V1.3) | ✓ `Soma_Refinement.v` | ✓ syntactic checker matches proof |
| Refinement (V1.5 typed) | ✗ not landed | ⚠ syntactic only |
| Sum types | ✗ not landed | ⚠ syntactic only |

Without sum-type Coq proofs, V1.5 introduces an **asterisk** on the
"zero axioms" claim. Closing it is the next focused Coq session.
