---
name: architecture
description: The fractal cell model — function, service, database, cluster, all expressed as a cell.
type: synthesis
since: V1.0
related: [cell, interior, scale, memory]
---

# Architecture

Soma's only top-level construct is the [[cell]]. The same construct
expresses:

- a function (a cell with handlers)
- a service (a cell with `on request`)
- a database (a cell with `[persistent]` memory)
- a cluster (a cell with `scale { replicas: N }`)

This is what the `Scale as a Type` paper means by **fractal**: the
model is self-similar at every scale.

## The five sections of a cell

```soma
cell PricingEngine {
    face    { signal book_trade(data: Map) -> Map }
    memory  { trades: Map [persistent, consistent] }
    state   { queued -> confirmed -> executed -> settled }
    scale   { replicas: 50, shard: trades, consistency: strong }
    on book_trade(data: Map) { trades.set(data.id, data) }
}
```

Five orthogonal axes:

- [[face]] — contract (compile-checked)
- [[memory]] — state (with distribution-type properties)
- [[state-machine]] — lifecycle (with CTL verification)
- [[scale]] — distribution (replicas, shard, consistency)
- [[handler]] — behavior (the only place code runs)

No infrastructure layer. No YAML. No Helm. No Dockerfile. The cell IS
the deployment artifact.

## Composition via [[interior]]

A cell can contain other cells:

```soma
cell System {
    interior {
        cell Alpha     { ... }
        cell Optimizer { ... }
        cell agent Compliance [model: claude] { ... }
    }
    on rebalance(input: Map) {
        let s = delegate("Alpha", "score", input)
        let o = delegate("Optimizer", "optimize", s)
        let v = delegate("Compliance", "review", o)
        ...
    }
}
```

Interior children:
- run in the parent's process (zero network for `delegate`)
- share the parent's signal bus, scheduler, storage
- contribute to the parent's [[budget-proof]] (peaks aggregate)
- can have their own state machines, memory, handlers

This is the **fractal** part: a child cell is structurally identical
to the parent. Nesting is unlimited.

## Standalone vs cluster

The same source runs both ways:

```
soma serve mycell.cell -p 8080                       # standalone
soma serve mycell.cell -p 8080 --join coord:9000    # cluster member
```

In cluster mode:
- `[persistent, consistent]` slots are replicated via consensus.
- `[ephemeral, local]` slots stay node-local.
- `[persistent]` + `eventual` slots use async replication.
- Sharded slots route writes by key hash to one of N replicas.

The flag flips behavior, not the language. There is no "code-with-K8s"
and "code-without-K8s" — just code.

## Where the compiler enforces consistency

Cross-section checks (compile time):

- `scale.shard: foo` requires `foo` to exist in `memory`.
- `scale.consistency: strong` requires the sharded slot to be
  `[persistent, consistent]`, not `[ephemeral]`.
- `face.signal X(...) -> ...` requires `on X(...) { ... }` to exist
  with matching arity.
- `state X { … }` with `: T` annotation requires every state name to
  be a variant of `T` ([[sum-types]]).
- `scale.memory: "256Mi"` requires the [[budget-proof]] to prove peak
  ≤ 256MiB.
- Every `await` matched by an `emit`; every `on handler` matched by
  an `emit` source ([[composition]]).

The point: **the type system, the distribution system, and the
verification system are the same system**. The compiler sees them all.

## Comparison: traditional layered architecture

| Layer | Traditional | Soma |
|---|---|---|
| Business logic | App code (Python/Node/JS) | Cell handlers |
| State | DB (Postgres, Redis) | `memory` with properties |
| Service contract | OpenAPI / proto | `face` |
| Deployment | Dockerfile + YAML | `scale` |
| Orchestration | K8s + Helm | `--join` flag |
| Lifecycle | None (or ad-hoc state field) | `state` with CTL |
| Verification | Tests, hopefully | `soma verify` |

The collapse is the point. There's exactly one mental model at every
scale.

## Trade-offs

The fractal model has real costs:

- **No "schema-vs-code" separation.** Adding a new asset to a
  `Map<String, String>` slot doesn't require a DB migration — but
  also doesn't have a migration story. See [[whats-missing]].
- **No named consensus protocol.** `strong` is a hint; the actual
  algorithm isn't in the language. Distributed systems experts find
  this loose.
- **Tight coupling to the runtime.** The `--join` flag is a Soma
  runtime feature; you can't deploy a Soma cell on bare K8s without
  the Soma sidecar.
- **Single-process per cell instance.** No multi-thread parallelism
  within a cell. Use `scale { replicas: N }` for parallelism, not
  threads.

These are conscious trade-offs to keep the language tractable.

## Examples

A function:

```soma
cell Math {
    on square(x: Int) { x * x }
}
```

A service:

```soma
cell Api {
    memory { items: Map<String, String> [persistent, consistent] }
    on request(method: String, path: String, body: String) {
        match path {
            "/api/items" -> items.values
            _ -> response(404, map("error", "not found"))
        }
    }
}
```

A multi-cell system with verification:

```soma
cell PortfolioSystem {
    scale { memory: "256Mi" }
    memory { lifecycle: Map [persistent, consistent] }
    state rebalance { initial: pending  pending -> approved -> closed  * -> failed }
    interior {
        cell Alpha     { on score(d: Map) -> Map { ... } }
        cell Optimizer { on optimize(s: Map) -> Map { ... } }
        cell agent Compliance [model: claude] { on review(o: Map) -> Map { ... } }
    }
    on rebalance(id: String, input: Map) {
        transition(id, "pending")
        let s = delegate("Alpha", "score", input)
        let o = delegate("Optimizer", "optimize", s)
        let v = delegate("Compliance", "review", o)
        if v.verdict == "APPROVE" { transition(id, "approved") }
        else { transition(id, "failed") }
        finalize(id)
    }
}
```

One cell, with three sub-cells (one of which is an LLM agent), with
budget proven, state machine verified, refinement enforced.

## Related

- [[cell]] — the construct itself.
- [[interior]] — composition mechanics.
- [[scale]] — the distribution declarations.
- [[manifesto]] — why this design.
- [[vs-kubernetes]] — comparison with infrastructure-as-YAML.
