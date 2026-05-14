---
name: face
description: A cell's public contract — signals, promises, tools. Compile-checked.
type: concept
since: V1.0
related: [cell, handler, signals, refinement]
---

# Face

A **face** is a cell's public API contract. It declares:

- **signals** — functions the cell exposes (must have matching `on`
  handlers)
- **promises** — structural or human-readable claims
- **tools** — for agent cells, external functions the LLM can call
- **given** clauses — required parameters from the runtime

The compiler verifies that every signal declared in the face has a
handler with matching parameter count.

## Syntax

```soma
cell PaymentGateway {
    face {
        signal charge(amount: Int, card: String) -> Map
        signal refund(transaction_id: String) -> Map
        promise "no double-charges"
        promise all_persistent           // structural: all slots [persistent]
    }
    on charge(amount: Int, card: String) { ... }    // required
    on refund(transaction_id: String)    { ... }    // required
}
```

If the cell forgets to declare `on refund`, `soma check` fails with:

```
face contract: signal 'refund' declared in cell 'PaymentGateway' has no handler
```

## Why it exists

Two reasons:

1. **The contract is in the source.** A reader (human or LLM) sees the
   face block and knows the API without reading the handler bodies.
2. **The compiler enforces it.** No drift between "what we advertise"
   and "what we implement." See [[refinement]] for the full story.

## Promise types

Three flavors:

- **String promise**: `promise "no double-charges"` — human-readable,
  generates a warning if no test asserts something equivalent.
- **Structural promise**: `promise all_persistent` — checked by walking
  the memory section. The current list of recognized structural
  promises is in `compiler/src/checker/mod.rs::check_promises`.
- **Identifier promise**: `promise <verifies_term>` — looked up in the
  registry; a [[verification-overview]] check is triggered.

## Tools (agent cells)

For `cell agent`, the face declares tools the LLM can invoke during a
`think()` call:

```soma
cell agent Researcher {
    face {
        signal research(topic: String) -> Map
        tool search(query: String) -> String "Search the web"
        tool calculate(expr: String) -> Float "Evaluate a math expression"
    }
    on search(query: String) { http_get("https://api.search.com?q={query}") }
    on calculate(expr: String) { ... }
    on research(topic: String) {
        think("Research {topic}", map("max_tokens", 2000))
    }
}
```

When the LLM emits a tool-call in its response, the runtime dispatches
to the corresponding `on` handler. See [[think]] for the full
mechanism.

## `given` clauses

A signal can declare parameters required from the runtime that don't
come from the caller:

```soma
face {
    signal authorize(amount: Int) -> Bool {
        given user_id: String
    }
}
```

`given` parameters are populated from the request context (auth header,
trace, etc.). They're invisible to the caller.

## Examples

Minimal contract:

```soma
cell Counter {
    face { signal inc() -> Int }
    memory { n: Map<String, String> [ephemeral, local] }
    on inc() {
        let cur = to_int(n.get("v") ?? "0") + 1
        n.set("v", to_string(cur))
        return cur
    }
}
```

The face on the rebalancer's Optimizer cell:

```soma
face {
    signal optimize(input: Map) -> Map
    promise "all math is deterministic"
    promise "no LLM in the optimization path"
    promise no_think
}
```

`no_think` is a structural promise the checker verifies by scanning all
handlers for `think()` calls.

## Edge cases

- A signal *not* in the face but with an `on` handler is **internal** —
  callable via `delegate` from inside the cell or its interior, but not
  via HTTP/RPC.
- An `on` handler whose name starts with `_` (e.g. `on _helper`) is
  always internal regardless of the face.
- Face contracts are checked per-cell. Cross-cell composition (signals
  matched across `interior {}` boundaries) is checked separately —
  see [[composition]].

## What this does NOT cover

- Promise *semantic* verification beyond a small structural set —
  string promises are documentation, not theorems.
- Type compatibility across the bus — signal parameter types are
  syntactically matched (arity), not deeply unified. See
  [[whats-missing]] for the open work.
