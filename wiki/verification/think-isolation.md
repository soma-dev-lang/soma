---
name: think-isolation
description: CTL safety holds regardless of what the LLM returns from `think()`. Mechanically verified.
type: feature
since: V1.0
related: [think, ctl-model-checking, refinement, coq-scorecard]
---

# Think-isolation

**Theorem.** If every `transition()` call in a cell uses a literal
target state (no `transition(id, dynamic_var)`), then CTL safety
properties of the state machine hold **regardless of what the LLM
returns from `think()`**.

This is what makes Soma "verified AI agents" non-vacuous. An LLM can
hallucinate, fabricate, refuse, or be adversarial — and the state
machine's safety theorems still hold.

## What "safety" means here

Safety = properties of the form "no bad state is ever reached":

- `never = ["invalid"]` — invalid is unreachable.
- `mutex = [A, B]` — A and B never hold simultaneously.
- `deadlock_free` — every reachable state has progress.

These compose into stronger guarantees. They do **not** include
liveness (`eventually(X)`) — for liveness, the LLM could in principle
keep stalling the system. Liveness has a separate adversary model
(see `docs/ADVERSARIES.md`).

## Why literals matter

The Coq proof in `Soma_Isolation.v` constructs a labelled transition
system where transition labels are *literal state names from the source
program*. The reasoning:

- Handler bodies can call `think()`, parse the result, use it in
  conditions, branch on it.
- But `transition(id, target)` always takes a target that the
  *compiler* picked, not the LLM.
- So the graph of reachable states is fully determined by the source
  program, not by the LLM's output.

The proof: for any safety property `P` proven on the source graph,
the same `P` holds at runtime — no LLM output can drive the system off
the graph.

## The escape hatch — and its cost

`transition(id, target_var)` where `target_var` is a runtime variable
(possibly LLM-derived) **breaks** the literal-only invariant. The
isolation theorem no longer applies for that handler.

`soma verify` reports this explicitly:

```
⚠ think-isolated: CTL safety holds regardless of LLM output
  (3 handlers, 7 literal transitions, 1 dynamic)
```

The "1 dynamic" tells the reviewer: one handler has an LLM-influenced
transition target. The strong claim weakens to that handler.

## Tool-call side channel — closed

Adversarial review found and the team closed: a `cell agent` that
calls `think()` may have the LLM invoke tools (declared in [[face]]).
A tool handler could itself call `transition()`. If the tool's
transition target were dynamic, the isolation theorem would be
violated indirectly.

V1.0 closed this by detecting tool handlers and including their
transitions in the isolation analysis. From `compiler/src/checker/isolation.rs`:

```
✓ think-isolated: CTL safety properties hold regardless of LLM output
  (5 handlers, 20 literal transitions, 0 dynamic)
```

The check includes tool-handler transitions in the count.

## Example: a verified agent

```soma
cell agent OrderClassifier [model: claude] {
    face {
        signal classify(order: Map) -> String
        tool lookup(symbol: String) -> Map "Look up market data"
    }

    state classification {
        initial: pending
        pending -> classified -> done
        * -> failed
    }

    on lookup(symbol: String) {
        // Tool body. May call transition() ONLY with literals.
        transition("inst", "pending")     // ✓ literal
    }

    on classify(order: Map) {
        transition("inst", "pending")
        let label = think("Classify: {order}", map("max_tokens", 100))
        transition("inst", "classified")
        let r = handle_label(label)        // pure function, no transitions
        transition("inst", "done")
        map("label", label, "result", r)
    }
}
```

`soma verify`:

```
✓ deadlock_free
✓ eventually(done | failed)
✓ refinement: handler `classify` ⟶ {pending, classified, done}
✓ refinement: handler `lookup`   ⟶ {pending}
✓ think-isolated: CTL safety properties hold regardless of LLM output
  (2 handlers, 4 literal transitions, 0 dynamic)
✓ termination: all 2 handlers structurally terminate
```

The LLM can return *any string* from `think("Classify: …")` and the
state machine still reaches `done` or `failed`. Soundness is mechanical.

## Counter-example: dynamic transition

```soma
on classify(order: Map) {
    let label = think("Pick a target state: …")  // LLM controls the string
    transition("inst", label)                     // ✗ dynamic — escapes isolation
}
```

`soma verify`:

```
⚠ think-isolated: 1 dynamic transition found
  → handler `classify` calls transition() with runtime-computed target

CTL safety claim weakens — handler `classify` is not isolated.
```

## What the proof covers

`Soma_Isolation.v` in `docs/rigor/coq/`:

- Labelled transition system parameterised over the state set.
- Safety theorem: for any source program where all
  `transition()` calls have literal targets, the reachability set is
  *exactly* the set computed by [[ctl-model-checking]] on the source
  graph.
- Tool-call side-channel closure: handler reachability includes tool
  handlers reachable via `think()`.

Closed under the global context. Zero axioms.

## Edge cases

- A handler calling `delegate("Other", "sig")` is fine — the *other*
  cell's transitions are checked in its own isolation analysis.
- `emit signal_name(…)` is fine — emit doesn't trigger transitions
  directly. The receiver's transitions are checked separately.
- A handler that constructs a literal-but-computed string like
  `transition("inst", "validated" + "")` — the parser does **not** see
  this as literal; it's parsed as `Expr::BinaryOp`. So this triggers
  the dynamic-target warning even though semantically it's a literal.
  Workaround: use the bare string.

## What this does NOT cover

- **Liveness under adversarial LLM.** If the LLM keeps refusing /
  stalling, `eventually(X)` may fail at runtime even though the proof
  says it holds. The proof assumes the handler completes; the LLM
  refusing to respond is a different adversary.
- **Output integrity.** Think-isolation says the state machine is
  safe, NOT that the LLM's response is correct. A handler returning
  the LLM's response to a caller carries no guarantee.
- **Cross-cell isolation.** If cell A's `think()` output is passed to
  cell B's transition via `delegate`, the isolation theorem doesn't
  cover the bridge. B should be analysed in its own right.
