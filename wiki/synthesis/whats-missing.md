---
name: whats-missing
description: Honest enumeration of V1.5 gaps. What experts would flag.
type: synthesis
since: V1.5
related: [verification-overview, coq-scorecard, manifesto]
---

# What's missing

Honest enumeration of V1.5 gaps. Some are research projects, some are
simple work that hasn't been prioritized, some are conceptual holes
that experts in adjacent fields would flag in a serious review.

The Karpathy LLM-wiki principle says: document what you can't yet do,
not just what you can.

## Tier 1 — actually blocking adoption

### Sum types & exhaustive matching — ✓ shipped V1.5

See [[sum-types]]. Was the #1 gap before V1.5.

### Effects in the type system

`think()`, `http_get()`, `now()`, `random()`, `delegate()` all change
the world. The type system doesn't track them.

Consequence: a "pure" handler can silently call `random()` and break
replay determinism. The type system can't refactor across effect
boundaries safely.

Standard solutions: Koka algebraic effects, OCaml 5 effect handlers,
Haskell's `IO`. Soma has `try { … }` and `?` for failure but no
effect tracking.

This is the highest-leverage **conceptual** gap.

### Formal concurrency semantics

Soma claims "verified distributed systems" — the formal semantics
covers per-cell state machines but not inter-cell signal passing.
Cross-cell ordering, causality, back-pressure are documented as
"best-effort" but lack a CSP / π-calculus level spec.

Without this, the "verified distributed" claim is only intra-cell.

### Named consensus protocol

`consistency: strong` must mean one specific thing (e.g.
"linearizable, backed by Raft with N=3 quorum, see RFC-001"). Today
it's a marketing string.

Distributed-systems people read `strong` and ask "Raft? Paxos?
EPaxos?". Without a named protocol with safety + liveness proofs,
the [[scale]] claim is incomplete.

## Tier 2 — meaningful improvements

### Generics (parametric polymorphism)

`Map<String, String>` works but you can't write a generic library.
Every helper that takes a `List<T>` is monomorphic.

`Option<T>` and `Result<T, E>` are designed (in `SUM_TYPES_DESIGN.md`)
as hardcoded builtins for V1.5.1; user-defined generics are V2.

### First-class `Option<T>` / `Result<T, E>`

Today, `()` overloaded as null is a smell. `data.get(k)` returns `()`
on miss; you compare with `!= ()`. A real `Option<T>` would surface
the distinction in the type system.

### Capability / permissions

`delegate("CellB", "method")` is unrestricted. Any cell can call any
other cell's handlers. No capability model.

Mark Miller (E-rights, object-capability security) would flag this.
Production deployments will eventually need it.

### Schema evolution

A `[persistent]` slot's structure is implicit (you put JSON in,
parse it out). Adding a field in V2 has no migration story. Older
records won't have the new field.

Standard solutions: explicit schema versions, lazy migration on
read, online migrations.

### OpenTelemetry / structured observability

`trace()` exists for agent debugging. No first-class OTel export, no
Prometheus metrics, no structured-log integration.

For production at scale this is the gap between "we have logs" and
"we have observability."

### Type inference

Most things are annotated. Hindley-Milner-style inference would
dramatically improve ergonomics and reduce visual noise.

## Tier 3 — ecosystem / tooling

### LSP server

No language server. Editing Soma in VS Code / JetBrains is plain
markdown highlighting. Every error squiggle, every autocompletion,
every jump-to-definition is missing.

This is probably the single most impactful tool improvement.

### Code formatter

No `soma fmt`. The syntax is consistent enough that one is feasible.

### Package manager + registry

`VISION.md` describes a cell-composition protocol. Today, `use lib::x`
loads from a relative directory; there's no central registry.

### Stable IR + incremental compilation

The compiler goes `AST → interpreter` or `AST → Rust source`. No IR
means no optimization passes, no incremental recompilation. Build
times for large projects will eventually hurt.

### Debugger

`trace()` and `print` are the debugger. No step-through, no
breakpoints.

### Tutorial / book

`SOMA_REFERENCE.md` is a cheat sheet. `AGENT.md` is for LLMs. No
"Learn You A Soma" narrative tutorial.

### Migration guides

Coming from LangChain, K8s, Erlang. [[vs-langchain]] starts this,
but it's one page.

### Standard library breadth

- No regex.
- No date types beyond `now()` (and `today()` returns a string).
- No channels / futures (sync world).
- No iterators / lazy sequences (fully-materialized Lists only).

## Tier 4 — V1.5 introduced

### `[sampled]` storage backend

`[sampled]` is declared in `stdlib/access.cell` but no backend
implements it. A user writing `Map [persistent, sampled]` today gets
nothing.

Promise unkept. Either land the BST-backed storage in
`runtime/storage.rs` or remove the property declaration.

### `[native]` codegen for linalg builtins

`svd_lowrank`, `regress_sgd`, `clean_covariance` run in the
interpreter at ~100-300× slower than hand-written Rust. For real
quant workloads they're unusable at scale.

`[native]` codegen integration is the structural fix.

### Bun-Bouchaud-Potters 2017 cross-validated RIE

The `clean_covariance` implementation uses the textbook Ledoit-Péché
RIE, not the cross-validated 2017 variant. Real-world cleaning
accuracy is 10–30% worse than the paper's optimum.

### Coq proof of V1.5 cost rules

The 14+ new linalg/risk cost-lattice rules in `budget.rs` are
trusted by source inspection. Coq proof was the standard before V1.5;
the asterisk is new and should be closed.

### VM bytecode dispatch for sum types

The bytecode VM compiler falls through to wildcard for variant
patterns (`compiler/src/vm/compiler.rs`). The tree-walking interpreter
handles them correctly; the VM doesn't.

## What "perfect" would look like

In priority order:

1. **Effects in the type system.** The single largest conceptual
   improvement.
2. **Formal concurrency semantics.** Required for "verified
   distributed" to be meaningful inter-cell.
3. **Named consensus protocol.** Required for "scale as a type" to
   be precise.
4. **LSP server.** Quality-of-life force multiplier.
5. **`[native]` codegen for linalg.** Makes the V1.5 algorithms
   actually fast.
6. **Coq proof of V1.5 cost rules.** Restore the asterisk-free claim.
7. **`Option<T>` / `Result<T, E>` builtins.** Plus user-defined
   generics roadmap.
8. **`[sampled]` storage backend.** Make the property real.
9. **Capability model.** Per-cell permission gates.
10. **OpenTelemetry export.** Production observability.

This is the realistic V1.6 / V2 roadmap. Don't pretend it's all
shipped.

## Related

- [[verification-overview]] — what IS proven today.
- [[coq-scorecard]] — the mechanically-verified subset.
- [[manifesto]] — the design philosophy that explains why some of
  these gaps matter more than others.
