---
name: stdlib-storage
description: Memory-slot API — `.set/.get/.delete/.keys/.values/.len`.
type: reference
since: V1.0
related: [memory, budget-proof]
---

# Stdlib: storage

Memory slots expose method-style access. All methods are O(log n) on
`[persistent]` slots (backed by SQLite), O(1) on `[ephemeral]`.

## Operations

```soma
data.set(key, value)                   // insert or update
data.delete(key)                        // remove key
let v = data.get(key)                   // returns () if missing
let has = data.has(key)                  // Bool
let n = data.len                         // count of entries
let keys = data.keys                     // List<String>
let vals = data.values                   // List<String>
```

Note: `len`, `keys`, `values` have no parentheses — they're property
access, not function calls.

## Common patterns

JSON round-trip for complex values:

```soma
memory { users: Map<String, String> [persistent, consistent] }

on save_user(u: Map) {
    let id = u.id ?? to_string(next_id())
    users.set(id, to_json(u |> with("id", id)))
    return map("id", id)
}

on load_user(id: String) {
    let raw = users.get(id)
    if raw == () { return map("error", "not found") }
    from_json(raw)
}
```

Existence check:

```soma
if data.has("key") {
    return data.get("key")
}
```

Iterating:

```soma
for [loop_bound(10000)] k in data.keys {
    let v = data.get(k)
    process(k, v)
}
```

Note the `[loop_bound(N)]` — required for [[termination]] checking.

## Invariants

A memory slot can declare invariants checked on every `.set()`:

```soma
memory {
    balance: Map<String, String> [persistent, consistent]
    invariant balance.len >= 0
    invariant balance.len <= 1000
}
```

Violation throws a runtime error (catchable by `try { … }`).

## Method costs (budget)

The [[budget-proof]] charges per-method:

- `.set/.get/.delete/.has/.len` — constant, fits in arg_cost.
- `.keys` — `DEFAULT_CAPACITY × DEFAULT_MAX_KEY_BYTES` bytes
  (the worst case if not annotated). Pass `[capacity(N), max_key_bytes(K)]`
  to tighten.
- `.values` — same with `max_value_bytes`.

## Examples

A KV cache with annotations:

```soma
memory {
    cache: Map<String, String> [
        ephemeral, local,
        capacity(1000),
        max_value_bytes(4096)
    ]
}

on lookup(k: String) {
    let v = cache.get(k)
    if v != () { return v }
    let fresh = expensive_compute(k)
    cache.set(k, fresh)
    fresh
}
```

A persistent ledger:

```soma
memory {
    entries: Map<String, String> [
        persistent, immutable,
        capacity(1000000),
        max_value_bytes(1024)
    ]
}

on append(id: String, entry: String) {
    if entries.has(id) { return map("error", "duplicate id") }
    entries.set(id, entry)
    map("ok", id)
}
```

## Edge cases

- `data.set("X", ())` stores the Unit value. `data.get("X")` then
  returns `()` — indistinguishable from "key not present". Use
  `data.has("X")` to disambiguate.
- `data.delete(k)` on a missing key is a no-op (no error).
- `data.keys` and `data.values` materialize the entire collection.
  Cost = `capacity × max_*_bytes`. On large slots this is expensive.

## Related

- [[memory]] — the property language (persistent, ephemeral, etc.).
- [[budget-proof]] — how slot bounds compose into the cell budget.
