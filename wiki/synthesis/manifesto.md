---
name: manifesto
description: The thesis — the specification is the program. Why Soma exists.
type: synthesis
since: V1.0
related: [verification-overview, refinement, sum-types, architecture]
---

# Manifesto

> **The language where handler bodies cannot lie to the state machine.**

That's the Soma tagline. This page unpacks what it means and why
Soma exists.

## The problem

In production systems, three artifacts coexist:

1. **The spec.** A diagram, a Confluence page, a sequence chart.
2. **The code.** What actually runs.
3. **The infrastructure config.** Dockerfiles, Helm charts, IAM
   policies.

These three drift apart. The spec says "every order eventually reaches
`settled`"; the code has a bug that loops `pending → running →
failed → pending`; the K8s config tries to scale a service that
shouldn't be replicated.

The drift is invisible until production. By then it's expensive.

## The Soma thesis

**Collapse the three artifacts into one.** Make the spec, the code,
and the infrastructure *all expressions in the same language*. Make
the compiler enforce their consistency.

```soma
cell PricingEngine {
    face    { signal book_trade(data: Map) -> Map }     // contract
    memory  { trades: Map [persistent, consistent] }     // state
    state   { queued -> confirmed -> executed -> settled } // spec
    scale   { replicas: 50, shard: trades, consistency: strong } // infra
    on book_trade(data: Map) { trades.set(data.id, data) } // code
}
```

The compiler reads all five sections and verifies:

- `face` signals have handlers.
- Handler bodies (`on book_trade`) only call `transition()` with
  targets that appear in the `state` block ([[refinement]]).
- `scale.shard` references a real slot; the consistency level is
  compatible with the slot's [[memory]] properties.
- `scale.memory` budget is met by the actual peak memory usage
  ([[budget-proof]]).
- CTL properties of `state` hold ([[ctl-model-checking]]).

The spec, the code, the infrastructure: one document, mechanically
linked.

## Three claims, each provable

### Claim 1: The spec is the program

Before V1.3, this was half-true. The state block was decorative; the
handlers could ignore it. V1.3's [[refinement]] check made it true
for transition targets. V1.5's [[sum-types]] state-machine
annotation made it true at the *type* level.

```
$ soma check engine.cell
error: handler 'advance' transitions to 'Validtaed', which is not a variant of 'OrderState'
```

A typo in `transition(id, Validtaed)` is now a compile error citing
the type. There is no longer a meaningful sense in which the spec and
the code can diverge.

### Claim 2: The compiler is the supervisor

Where most production systems run a supervisor (PM2, systemd, K8s
controllers) to catch problems after deployment, Soma catches them
before. The supervisor's job — "make sure the system never reaches a
bad state" — is partly *implementable as a type system*:

- "no deadlocks" → [[ctl-model-checking]]
- "memory ≤ 128 MiB" → [[budget-proof]]
- "no double-charge" → state machine + [[refinement]]
- "handlers terminate" → [[termination]]
- "safety holds under adversarial LLM" → [[think-isolation]]

Each of these is a Coq theorem on the abstract semantics. The
compiler enforces the preconditions; the theorem gives you the
guarantee. The supervisor is the compiler.

### Claim 3: Agents are first-class

LLM-driven systems are not bolted on. `cell agent` is a kind of cell.
[[think]] is a bounded builtin with a [[budget-proof]] story.
[[think-isolation]] is a mechanically-verified theorem: CTL safety
holds *regardless of LLM output*.

This is not metaphor. The Coq proof in `Soma_Isolation.v` constructs
a labelled transition system where the LLM is treated as an
adversary, and proves the state-machine cannot be driven off the
declared graph.

The combination — verification + agent-native — is what no other
production-aimed language has.

## What this is not

A few clarifications:

- **Not a magic full-stack solution.** Soma doesn't replace your
  database, your monitoring, or your business logic. It expresses
  them in a more constrained way that the compiler can reason about.
- **Not a theorem prover.** The Coq proofs are about the abstract
  semantics, not your specific business logic. You write `ensure
  balance >= 0` for that.
- **Not a quantum language.** The "quantum-inspired" linalg builtins
  ([[stdlib-linalg]]) are classical algorithms motivated by Tang's
  dequantization work. No qubits.
- **Not finished.** V1.5 has real gaps — see [[whats-missing]].
  Effects in the type system, formal concurrency semantics, full
  Coq coverage of new features are all open work.

## Five bets (from VISION.md)

The V1 vision identifies five bets for the next phase:

1. **Intent compilation.** Express requirements in natural language;
   the compiler produces and verifies a cell.
2. **Diagnostic repair plans.** Every error includes a structured
   patch, not just a message.
3. **Cell composition protocol.** A package registry of verified
   cells with face-contract integration checks.
4. **Live verification.** Continuous proof maintenance as code
   changes, not just at compile time.
5. **Behavioral reflection.** `soma describe --behavior` produces a
   data-flow and intent summary an agent can consume without reading
   source.

These are the "next decade" goals. V1.5 has the foundations; the bets
build on them.

## The honest cut

What Soma actually delivers in V1.5:

- ✓ CTL model checking on state machines, with Coq backing.
- ✓ Refinement: handlers can't lie to the spec.
- ✓ Budget proofs: peak memory bounded at compile time.
- ✓ Think-isolation: LLM-as-adversary, safety preserved.
- ✓ Sum types: typed state machines, exhaustive matching.
- ✓ Fractal cell model: function → cluster, same syntax.

What it doesn't deliver:

- ✗ Effects in the type system.
- ✗ Formal cross-cell concurrency semantics.
- ✗ CompCert-style proof chain to the running Rust interpreter.
- ✗ Named consensus protocol for `consistency: strong`.

See [[coq-scorecard]] and [[whats-missing]] for the full ledger.

## Why this matters

The bet: agent-driven and ML-driven systems are going to multiply.
Every major framework today (LangChain, AutoGen, CrewAI) treats agents
as a glue layer over arbitrary code with no compiler-level guarantees.
When one of those systems fails in production — wrong state, leaked
memory, runaway LLM call — there's no theorem to fall back on.

Soma's bet is that the right level for guarantees is the language
itself. State machines, budgets, isolation, contracts — all in the
same artifact, all compile-checked.

If the bet is right, every production AI agent should eventually be
written in something like Soma.

## Related

- [[architecture]] — the fractal cell model.
- [[verification-overview]] — what the compiler proves.
- [[coq-scorecard]] — what's mechanically verified.
- [[vs-langchain]] / [[vs-kubernetes]] / [[vs-erlang-pony]] — how
  Soma compares to existing tools.
- [[whats-missing]] — honest gaps.
