---
name: scale
description: How a cell distributes — replicas, sharding, consistency, resource budgets.
type: concept
since: V1.0
related: [cell, memory, budget-proof, architecture]
---

# Scale

The `scale` section is a cell's distribution declaration. It encodes
replica count, what's sharded, consistency level, and per-instance
resource budgets. The compiler verifies internal consistency at
compile time — before deployment.

## Syntax

```soma
scale {
    replicas: 50            // number of cell instances
    shard: trades           // memory slot to distribute across replicas
    consistency: strong     // strong | causal | eventual
    tolerance: 2            // survives N node failures
    cpu: 4                  // cores per instance
    memory: "8Gi"           // RAM budget per instance
    disk: "100Gi"           // disk budget
}
```

## What the compiler checks

Four classes of cross-section consistency:

1. **Shard validity** — the named slot exists in `memory`.
2. **Consistency coherence** — no `[ephemeral]` slot can be sharded with
   strong consistency; no `[local]` slot can be sharded at all.
3. **CAP analysis** — `strong + tolerance > 0` implies CP mode (reduced
   availability under partition). Reported as info, not error.
4. **Quorum sizing** — for strong + N replicas, quorum = N/2 + 1,
   maximum tolerable failures = N - quorum. Mismatch is a compile
   error.

The error messages are concrete:

```
memory { data: Map [ephemeral] }
scale  { shard: data, consistency: strong }
// ERROR: shard 'data' uses [ephemeral] but scale declares consistency: strong
```

## Memory budget (V1.4)

`memory: "8Gi"` is **not advisory**. The [[budget-proof]] checker walks
every handler, counts every allocation, and proves peak ≤ declared
budget. Output:

```
✓ budget proven for cell 'Optimizer':
  peak ≤ 69.89 MiB ≤ declared 128.00 MiB
  breakdown: slots 0 B + max-handler 53.89 MiB + state 0 B + runtime 16.00 MiB
```

Three outcomes:

- **Proven** — closed-form bound fits. Cell will not OOM.
- **Exceeded** — bound exceeds budget. Compile error with breakdown.
- **Advisory** — handler calls an unbounded builtin (like `think()`
  without `max_tokens`). The checker names the call sites.

See [[budget-proof]] for the cost-lattice details.

## Cluster vs standalone

The same source runs in two modes:

```
soma serve mycell.cell -p 8080                   # standalone
soma serve mycell.cell -p 8080 --join coord:9000 # cluster member
```

In cluster mode:
- `[persistent, consistent]` slots are replicated via consensus.
- `[ephemeral, local]` slots stay node-local.
- `[persistent]` + `eventual` slots use async replication.
- Sharded slots route writes by key hash.

The `--join` flag is a *runtime* decision. The source code is
identical.

## Examples

Single-instance:

```soma
cell Tracker {
    memory { counts: Map [persistent, consistent] }
    scale  { replicas: 1, memory: "128Mi" }
    on inc(k: String) { ... }
}
```

Sharded multi-instance:

```soma
cell Pricer {
    memory { quotes: Map<String, Float> [persistent, consistent] }
    scale {
        replicas: 100
        shard: quotes               // hash on key, route to one instance
        consistency: strong
        tolerance: 2                // survives 2 failed nodes (quorum = 51)
        cpu: 2
        memory: "4Gi"
    }
    on get(symbol: String) { quotes.get(symbol) ?? 0.0 }
}
```

Read-heavy with eventual consistency:

```soma
cell Recommendations {
    memory { feed: Map<String, String> [persistent] }
    scale {
        replicas: 50
        consistency: eventual       // not strong — read-anywhere, write-anywhere
        tolerance: 5
    }
}
```

## Edge cases

- `replicas: 1` with `shard: …` is contradictory — a one-replica cell
  can't shard. Compile error.
- `consistency: strong` with `replicas: 1` works but degenerates to
  "single source of truth" — no consensus needed.
- The `memory: "8Gi"` field is parsed by `parse_budget_bytes` in
  `compiler/src/checker/budget.rs`. Suffixes: `Ki`, `Mi`, `Gi`, `Ti`
  (binary), `KB`, `MB`, `GB` (decimal), plus suffix-less bytes.

## What this does NOT cover

- **Named consensus protocol.** `consistency: strong` doesn't name a
  specific algorithm (Raft? Paxos? EPaxos?). This is the largest gap
  flagged by distributed-systems experts in [[whats-missing]].
- **Schema evolution.** Adding a field to a sharded slot in V2 has no
  story; the runtime assumes schema stability.
- **Cross-cluster** federation — Soma has no notion of "another Soma
  cluster" today.
