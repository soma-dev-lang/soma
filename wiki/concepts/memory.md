---
name: memory
description: State slots annotated with distribution-type properties that the compiler enforces.
type: concept
since: V1.0
related: [cell, scale, budget-proof, verification-overview]
---

# Memory

Cells declare state in a `memory` section. Each slot has a name, a
type, and a list of **properties** in square brackets. The properties
are the entire distribution story — the compiler reads them and picks
the storage backend automatically.

## Syntax

```soma
memory {
    trades:  Map<String, String> [persistent, consistent]    // → SQLite
    cache:   Map<String, String> [ephemeral, local]           // → in-memory HashMap
    audit:   Map<String, String> [persistent, immutable]      // → append-only SQLite
}
```

## The distribution-type lattice

Three axes; exactly one value from each axis per slot:

- **Durability** — does data survive restart?
  - `[persistent]` — yes, durable storage
  - `[ephemeral]` — no, in-memory only
- **Consistency** — what do readers see?
  - `[consistent]` — linearizable, all readers see the latest write
  - `[eventual]` — readers may see stale data
  - `[local]` — per-instance state, no cross-instance guarantees
- **Other**
  - `[immutable]` — append-only, implies `[consistent]`, contradicts `[evict]`
  - `[evict(policy)]` — eviction policy, contradicts `[immutable]`
  - `[retain(duration: Int)]` — implies `[persistent]`
  - `[encrypted]` — encryption at rest
  - `[sampled]` — V1.5; supports ℓ²-norm sampling (no backend yet)

Contradictions are compile errors:

```soma
data: Map [ephemeral, persistent]    // ERROR: contradictory durability
```

## Why properties, not configuration

In K8s/Helm the equivalent is a YAML file that the compiler doesn't
see. Soma's bet: the distribution requirements *are* a type system, so
they should be checked alongside the program.

The compiler enforces, for example:

```soma
memory { data: Map [ephemeral] }
scale  { shard: data, consistency: strong }
// ERROR: shard 'data' uses [ephemeral] but scale declares consistency: strong
```

You cannot declare strong consistency over ephemeral storage; the
distribution story is internally inconsistent and the program won't
compile.

## Backend selection

A `cell backend` registers what property combinations it serves:

```soma
cell backend sqlite {
    rules {
        matches [persistent, consistent]
        matches [persistent, immutable]
        matches [persistent]
        native "sqlite"
    }
}
```

The runtime walks the memory section, looks up each slot's properties,
and picks the most-specific matching backend. SQLite matches "more
properties" than the file backend, so it wins ties. See
`stdlib/backends.cell`.

## Capacity annotations (for [[budget-proof]])

Optional bracketed annotations bound slot size:

```soma
trades: Map<String, String> [
    persistent,
    consistent,
    capacity(10000),                  // ≤ 10k entries
    max_key_bytes(64),
    max_value_bytes(2048),
]
```

The [[budget-proof]] checker uses these to compute a closed-form upper
bound on memory consumption. Without them it uses conservative
defaults (`DEFAULT_CAPACITY = 10_000`, `DEFAULT_MAX_VALUE_BYTES = 4096`).

## Slot API

Memory slots expose method-style access:

```soma
data.set("key", "value")      // O(log n) for [persistent], O(1) for [ephemeral]
let v = data.get("key")        // returns () if missing
data.delete("key")
let keys = data.keys           // List<String> — no parentheses
let vals = data.values
let n = data.len               // count

// Invariants:
memory {
    balance: Map<String, String> [persistent, consistent]
    invariant balance.len >= 0     // checked on every .set()
}
```

## Examples

Counter:

```soma
cell Counter {
    memory { n: Map<String, String> [ephemeral, local] }
    on inc() {
        let cur = to_int(n.get("v") ?? "0") + 1
        n.set("v", to_string(cur))
        return cur
    }
}
```

Persistent ledger:

```soma
cell Ledger {
    memory {
        entries: Map<String, String> [persistent, immutable, capacity(1000000), max_value_bytes(1024)]
    }
    on append(id: String, entry: String) {
        if entries.get(id) != () { return map("error", "id exists") }
        entries.set(id, entry)
        map("ok", id)
    }
}
```

## Edge cases

- `Map<K, V>` keys and values are always serialized to JSON-able primitives
  on `[persistent]` slots. Records and Lists need `to_json`/`from_json`
  round-tripping.
- `data.values` materializes the entire collection — bounded by `capacity`
  for the [[budget-proof]] but expensive on large slots.
- `[ephemeral, local]` is the only combination that guarantees zero
  network round-trips in cluster mode.

## What this does NOT cover

- Schema evolution — adding a field to a slot in V2 has no story yet.
  See [[whats-missing]].
- Backup / migration — handled per-backend by the runtime, not in the
  language.
