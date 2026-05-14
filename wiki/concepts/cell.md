---
name: cell
description: The unit of computation, distribution, and verification — fractal at every scale.
type: concept
since: V1.0
related: [face, memory, state-machine, scale, handler, interior, architecture]
---

# Cell

A **cell** is Soma's only top-level construct. It is simultaneously:

- a function (when it only has handlers)
- a service (when it has `on request`)
- a database (when it has `[persistent]` memory)
- a cluster (when it has a `scale { replicas: N }` section)

The point of the design: there is exactly **one mental model** at every
level of the system. Going from one machine to a thousand is changing a
number in `scale`, not learning Kubernetes.

## Anatomy

```soma
cell PricingEngine {
    face    { signal book_trade(data: Map) -> Map }         // contract
    memory  { trades: Map [persistent, consistent] }         // state
    state   { queued -> confirmed -> executed -> settled }   // lifecycle
    scale   { replicas: 50, shard: trades, consistency: strong }
    on book_trade(data: Map) { trades.set(data.id, data) }   // behavior
}
```

Five sections, all optional:

- [[face]] — public contract that the compiler checks against handlers
- [[memory]] — state slots with distribution-type properties
- [[state-machine]] — explicit lifecycle for instance IDs
- [[scale]] — replica count, shard, consistency level, resources
- [[handler]] — `on signal_name(...)` blocks

Plus the wrappers:

- `every Ns { ... }` — scheduled execution
- `after Ns { ... }` — one-shot delay
- [[interior]] `{ cell A { ... }  cell B { ... } }` — composition

## Cell kinds

The keyword can be specialized:

- `cell Foo { ... }` — regular cell (default)
- `cell agent Foo [model: opus] { ... }` — agent cell with `think()` and tools
- `cell type Foo { variants { ... } }` — [[sum-types]]
- `cell property persistent { rules { ... } }` — defines a memory property
- `cell backend sqlite { rules { matches [persistent, consistent] ... } }` — storage backend
- `cell builtin print { rules { native "print" } }` — bridges a Rust function
- `cell checker my_rule { rules { check { ... } } }` — project-specific lint
- `cell test MathTests { rules { assert 1 + 1 == 2 } }` — test cell

Every section is parsed the same way regardless of kind; what changes is
which sections are *required* (an agent needs `think()`-calling handlers;
a backend needs `matches`).

## Fractal property

The *same* cell can be:

- standalone: `soma serve mycell.cell -p 8080`
- a cluster node: `soma serve mycell.cell -p 8080 --join other:8081`

with **zero code changes**. The `--join` flag flips the runtime from
"standalone process" to "cluster member"; `[persistent, consistent]`
slots become CP-replicated; `[ephemeral, local]` stays node-local.

This is what the `Scale as a Type` paper means by *fractal*: there is no
"infrastructure layer" separate from "application layer." Both are
expressed in the cell.

## Examples

A function:

```soma
cell Math {
    on square(x: Int) { return x * x }
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

A cluster:

```soma
cell Pricer {
    memory { quotes: Map<String, Float> [persistent, consistent] }
    scale  { replicas: 100, shard: quotes, consistency: strong }
    on quote(symbol: String) { return quotes.get(symbol) ?? 0.0 }
}
// soma serve pricer.cell -p 8080 --join coordinator:9000
```

## Edge cases

- A cell with no `face` has implicitly public handlers — `on x()` is
  visible to callers if the handler name doesn't start with `_`.
- `on _private()` is callable from within the cell (and via `delegate`
  from interior children) but is not an HTTP endpoint and is not in the
  face contract.
- `cell type` declarations cannot have `memory`, `state`, or handlers —
  they're pure type-level declarations.

## What this does NOT cover

- How handler bodies dispatch (lambda capture, recursion limits) — see
  [[handler]].
- The signal bus's ordering guarantees — see [[signals]] and the open
  question in [[whats-missing]].
