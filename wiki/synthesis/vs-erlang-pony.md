---
name: vs-erlang-pony
description: Comparison with actor-model languages — Erlang, Elixir, Pony.
type: synthesis
since: V1.0
related: [cell, signals, manifesto, vs-langchain, vs-kubernetes]
---

# Soma vs Erlang / Elixir / Pony

Erlang, Elixir, and Pony are the dominant actor-model languages. They
share with Soma the idea that a unit of computation (process, actor,
cell) is the unit of concurrency. They differ on what the compiler
proves.

## What's the same

- **Actor / cell as the primitive.** Erlang processes, Pony actors,
  Soma cells. Each runs sequentially within itself; concurrency is
  across instances.
- **Message-passing.** Erlang's `!` operator, Pony's behavior calls,
  Soma's `emit` and `delegate`. No shared mutable state.
- **"Let it crash" philosophy.** Erlang supervisors, Pony tracebacks,
  Soma's `try { … }` + state-machine rollback. Errors are localized.
- **Functional core.** All three discourage mutation in favor of
  immutable values and recursion.

## What's different

### 1. Soma has explicit state machines

Erlang has `gen_statem` (a behavior); Pony has nothing. Soma makes
the state machine a **language construct** with CTL verification.

```soma
cell Order {
    state order {
        initial: pending
        pending -> validated -> filled
    }
    on submit(id: String) { transition(id, "validated") }
}
```

`soma verify` proves CTL temporal properties on the graph. Erlang
programs have state machines, but they live in code, not in the type
system.

### 2. Soma's compiler proves termination + memory budget

Erlang: a process can have an infinite loop. The supervisor will
restart it but the loop runs first.

Pony: capabilities prevent data races but don't prove termination.

Soma: [[termination]] and [[budget-proof]] are compile-time checks
across every handler. Unbounded loops require explicit
`[loop_bound(N)]`.

### 3. Soma treats LLMs as first-class

None of Erlang / Elixir / Pony have built-in LLM support.
Integration is a library; safety is the user's problem.

Soma has `cell agent`, `think()`, [[think-isolation]] — a
mechanically-verified theorem that LLM output can't break state-
machine safety.

### 4. Soma's distribution is part of the cell

Erlang's distribution model (`nodes`, `epmd`, `:rpc`) is run-time.
You write code that *can* run distributed and configure it at start.

Pony has actors that work across machines via causality-typed
behaviors, but the distribution story is similar to Erlang.

Soma's `scale { replicas: N, shard: foo, consistency: strong }` is
**source code**, compile-checked. The same source runs standalone or
distributed via `--join`.

## What Erlang/Pony have that Soma doesn't

Be honest:

- **Decades of production hardening.** Erlang has run WhatsApp,
  Klarna, Riak, dozens of telecom switches. Pony is younger but
  more carefully designed for capability safety.
- **Pony's reference capabilities.** Soma has no equivalent of
  `iso`, `trn`, `ref`, `val`, `box`, `tag` — Pony's static
  guarantees against data races are stronger than Soma's "one
  handler at a time" rule.
- **BEAM ecosystem.** Erlang/Elixir have `mnesia`, `phoenix`,
  `nerves`, `:gen_tcp`. Soma's stdlib is a fraction of this.
- **OTP behaviors.** `gen_server`, `gen_statem`, `supervisor` are
  battle-tested patterns with library support. Soma has the building
  blocks but not the pattern catalog.
- **Hot code reload.** Erlang's signature feature — swap handler
  code in a running system. Soma has no equivalent.
- **Soft real-time guarantees.** Erlang has reduction-based fair
  scheduling. Soma has cooperative scheduling.

## Where Soma wins

- **Compile-time CTL verification.** Erlang/Pony have no equivalent.
- **Memory budget proofs.** [[budget-proof]] doesn't exist elsewhere.
- **LLM integration.** First-class.
- **Distribution as part of the type system.** Erlang/Pony express
  this in configuration, not types.

## Migration sketch

From Erlang/Elixir:

- `gen_server` → cell with `face { }` + `on call_name(...)`.
- `gen_statem` → cell with `state { }`.
- `:send` / `!` → `emit`. `:call` → `delegate`.
- Supervisor → state-machine transitions to a `failed` terminal +
  retry handler.
- Mnesia table → `memory { x: Map [persistent, consistent] }`.

The semantic gap is small. The verification gap (you go from "I have
tests" to "the compiler proves CTL") is large in your favor.

From Pony:

- Actors → cells, same idea.
- Reference capabilities → Soma's properties (`[ephemeral, local]`
  approximates `iso`; `[persistent, consistent]` approximates `val`).
  Soma's capabilities are coarser but typed.

## Honest cut

Erlang is **more mature**, **more flexible**, and **more proven** in
production. For traditional concurrent systems (telecom, real-time
data, fault-tolerant servers) Erlang is the obvious choice.

Soma's bet: as systems incorporate LLMs and as verification becomes
table stakes, the compile-time guarantee story matters more than
maturity. For new agent systems with verification requirements, Soma
is the right starting point even with its smaller ecosystem.

For everything else, Erlang's three-decade head start wins.

## Related

- [[cell]] — the actor analog.
- [[signals]] — message-passing.
- [[verification-overview]] — what's actually proven.
- [[vs-langchain]] — comparison with agent frameworks.
- [[vs-kubernetes]] — comparison with infrastructure-as-YAML.
