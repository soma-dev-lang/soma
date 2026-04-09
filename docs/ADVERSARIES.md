# Soma — Adversary Models for Verified Properties

> Companion to `docs/SEMANTICS.md` and `docs/SOUNDNESS.md`.
>
> Every guarantee Soma's verifier prints is true *under some
> assumption about what an adversary can do*. This document names that
> assumption explicitly for each property class. The principle from
> `TARGET_SOMA_V1.md §0`:
>
> > **Adversary-quantified statements of every existing guarantee:
> > "deadlock-free *under what network model*?"**
>
> Until each guarantee is paired to its adversary, it's marketing.
> This is the table that turns it into a theorem with a quantifier.

A property and its adversary always come together. `eventually(closed)`
"holds" only against an adversary that obeys certain rules — and
*against* an adversary that breaks those rules, the same statement
may be false.

The adversaries in this document are stratified by the **boundary**
they cross, from least to most powerful:

  - **Local adversary**: chooses the order of handler invocations on
    a single in-process cell instance, but cannot reorder operations
    *within* a handler (handlers are atomic from the model checker's
    perspective).
  - **Storage adversary**: above + can pause writes, can crash and
    restart the cell between handler invocations, can lose
    unflushed buffers.
  - **Concurrent adversary**: above + multiple handler invocations
    can interleave at the AST-statement granularity.
  - **Distributed adversary**: above + drops, reorders, delays, and
    duplicates messages between cell instances on different nodes.
  - **Byzantine adversary**: above + colluding nodes can return
    arbitrary responses.

V1 verification is sound only against the **local adversary**. The
other rows are aspirational and explicitly marked.

---

## Table of properties × adversaries

| Property                                       | Local | Storage | Concurrent | Distributed | Byzantine | Notes |
|------------------------------------------------|:-----:|:-------:|:----------:|:-----------:|:---------:|-------|
| `deadlock_free`                                |   ✅   |    ⚠️    |     ❌      |      ❌      |     ❌     | A1, A6 — single instance, no concurrency model |
| `always(P)`, `never(P)` *(safety)*             |   ✅   |    ✅    |     ❌      |      ❌      |     ❌     | Pure-state predicates; preserved by storage round-trip |
| `eventually(P)` *(liveness)*                    |   ✅   |    ⚠️    |     ❌      |      ❌      |     ❌     | Requires fair scheduling; storage crashes can re-replay |
| `after(s, P)` *(liveness)*                      |   ✅   |    ⚠️    |     ❌      |      ❌      |     ❌     | Same caveats as `eventually` |
| `mutex(s₁, s₂)`                                |   ✅   |    ✅    |     ❌      |      ❌      |     ❌     | Per-instance only; doesn't compose across instances |
| **V1.3 refinement** (handler-target check)     |   ✅   |    ✅    |     ✅      |      ✅      |    n/a     | Static, holds regardless of runtime adversary |
| **Replay determinism** (`[record]` cells)      |   ✅   |    ✅    |     n/a    |     n/a     |    n/a     | Single-instance bit-determinism; concurrency would need an event log |
| **Integer correctness** (no silent overflow)   |   ✅   |    ✅    |     ✅      |      ✅      |     ✅     | Pure arithmetic; the dual-mode dispatch is unconditionally correct |

Legend: ✅ = sound and tested; ⚠️ = sound under stated additional
assumption; ❌ = not yet proven, and an adversary in this column can
falsify the property; n/a = property doesn't make sense at this level.

---

## 1. Local adversary

This is the default V1 verification adversary. It can:

  - Choose any handler the cell exposes via `face`
  - Pass any well-typed arguments
  - Repeat handler invocations any number of times in any order
  - Observe storage between calls

It **cannot**:

  - Interleave handler bodies (each `on Sig(...) { body }` is atomic
    from the model checker's perspective)
  - Mutate storage directly (only through handler calls)
  - Run two instances of the same cell in parallel
  - Drop or reorder signals already in flight
  - Inspect or alter the program source

**Theorem 1.1 (Local soundness).** *Every property marked ✅ in the
"Local" column of the table holds against any local adversary.*

*Proof.* See `docs/SOUNDNESS.md` §3.1, §3.2, §3.3 for the model
checker proof, and §2 for the runtime trust base. The local adversary
maps directly onto the model checker's exhaustive enumeration of
`Reach(G)`. □

This is the adversary the rebalancer demo defends against
(`rebalancer/app.cell`). The 16 verified properties for the rebalance
state machine all assume a local adversary calling
`POST /rebalance`, `POST /approve`, etc. in any order.

---

## 2. Storage adversary

Adds: the storage backend can crash, restart, lose unflushed writes,
and reorder commits across handler boundaries.

**What still holds (✅):**

  - **Safety properties.** `Always(P)` and `Never(P)` are predicates
    on the *current state*. A crash that loses the most recent write
    rolls back to a previous valid state, which is also in `Reach(G)`,
    and the predicate must still hold there (or it would have failed
    on a smaller execution).
  - **Refinement.** Static check, immune to runtime crashes.
  - **Mutex.** Same argument as Always/Never.

**What needs an extra assumption (⚠️):**

  - **`deadlock_free`** holds *if* the backend's recovery is
    transactional — i.e., the recovered state is some state in
    `Reach(G)`, not an arbitrary intermediate. The current `FileBackend`
    writes the entire JSON map per `set()`, so this holds. The
    `SqliteBackend` uses sqlite's WAL, also transactional.
  - **`eventually(P)`** requires that after a crash, the cell
    eventually receives the same handler invocations again. Without
    fair re-delivery, an `eventually` claim degrades to a "would-be
    eventually if the workload resumes" claim.

**What doesn't hold (❌):**

  - Nothing in the current property set is broken by the storage
    adversary alone. The dangers are upstream: a workload generator
    that doesn't replay lost work after a crash will visibly violate
    `eventually(P)` from the user's perspective.

---

## 3. Concurrent adversary

Adds: multiple handler invocations on the same cell instance can
interleave at the AST-statement granularity.

**What changes (❌):**

  - **`deadlock_free`** is no longer guaranteed. Two handlers that
    each acquire memory in different orders can deadlock — the model
    checker doesn't see the implicit memory locks because they're
    per-storage-backend, not part of the abstract graph.
  - **`eventually(P)`** can be falsified: handler A starts a state
    transition, handler B observes the intermediate, transitions
    elsewhere, and the system reaches a state outside `Reach(G)` from
    B's perspective.
  - **`always(P)`/`never(P)`** can be falsified for the same reason:
    interleaved writes can produce intermediate states the model
    checker never considered.

**What still holds (✅):**

  - **Refinement** (V1.3 static check). Independent of runtime.
  - **Integer correctness.** Per-operation contract.

**Status:** V1 has no story here. The current interpreter is
single-threaded per instance, so this adversary doesn't materialise
in practice — but the verifier does *not* prove safety under
interleaving, and any future multi-threaded backend must either keep
handlers atomic or re-prove the properties.

---

## 4. Distributed adversary

Adds: cells run on multiple nodes; messages between them can be
dropped, reordered, duplicated, or arbitrarily delayed (but not
forged — that's the byzantine layer).

**Status:** `soma serve --join` exists for cluster mode but verification
of distributed properties was the v1 `protocol` and `adversary`
features that were **subtracted in V1.2** (see `SEMANTICS.md` §4)
because the implementation didn't carry the claim. The current
verifier proves nothing about cell-to-cell composition across nodes.

This is the largest open gap in the verification story. Examples
that would need it but currently don't have it:

  - Two-phase commit between cells
  - Leader election in `--join` clusters
  - Cross-node consistency of `[persistent, consistent]` memory

Anybody using Soma in distributed mode today is relying on the
*runtime* (the storage backend, the cluster glue) to provide what
the verifier does not. The honest framing: V1 Soma is a verified
single-cell language, and a non-verified distributed runtime around
it.

---

## 5. Byzantine adversary

Adds: colluding nodes can return arbitrary responses, sign with
arbitrary keys, equivocate.

**Status:** Out of scope for V1 entirely. No property in the table
is byzantine-tolerant. This is correctly marked ❌ everywhere except
"Integer correctness", which is a per-operation contract that does
not depend on any other party.

---

## 6. The LLM as an adversary (the agent angle)

A practical question Soma's pitch must answer: when a `cell agent`
calls `think()` and the LLM returns garbage, does any of this still
hold?

The honest answer:

  - **The state-machine properties still hold** as long as the
    handler that called `think()` does not pass the LLM output
    directly into `transition()` as a dynamic target. The rebalancer's
    `Compliance.review()` returns a string that the orchestrator
    parses with `_parse_verdict()` into a small enum (`APPROVE`,
    `BLOCK`, `FLAG`); only the enum is used in the state-machine
    decision. The verifier proves the state machine is sound for any
    enum value, regardless of what the LLM said.
  - **The LLM can still cause outcomes the user dislikes.** A
    hallucinated `APPROVE` from the model doesn't violate any
    *verified* property of the rebalancer — but it may violate the
    user's *intended* policy. The verifier can't catch that. This is
    a fundamental limit of any verification system that has an LLM
    in the loop and treats the LLM as a black box.

The architectural defence in the rebalancer is **not** to verify the
LLM, but to confine it: the LLM is a *reviewer* and a *narrator*, not
a *decider*. Every investment decision is in `lib/optimizer.cell`,
which is pure deterministic Soma. The LLM can flag, suggest, and
explain, but cannot move money. This pattern generalises: the LLM
adversary is contained by architecture, not by verification.

---

## 7. The single sentence

> Every property `soma verify` prints is sound against a local
> adversary; the safety properties extend to a storage adversary; no
> property is yet sound against concurrent, distributed, or byzantine
> adversaries — and the LLM is an adversary you contain by
> architecture, not by proof.
