---
name: interior
description: Nested cells inside a parent — the composition mechanism.
type: concept
since: V1.0
related: [cell, signals, composition, budget-proof]
---

# Interior

A cell can contain other cells in an `interior { }` block. Interior
children:

- run in the same process as the parent
- share the parent's runtime (signal bus, scheduler, storage)
- can declare their own [[memory]], [[handler]]s, and [[state-machine]]s
- inherit the parent's [[budget-proof]] (the parent's budget must
  cover the sum of children's peaks)

## Syntax

```soma
cell System {
    memory { … }

    interior {
        cell Alpha {
            face { signal score(input: Map) -> Map }
            on score(input: Map) { … }
        }

        cell Optimizer {
            face { signal optimize(scores: Map) -> Map }
            on optimize(scores: Map) { … }
        }
    }

    on rebalance(input: Map) {
        let scores = delegate("Alpha", "score", input)
        let weights = delegate("Optimizer", "optimize", scores)
        map("weights", weights)
    }
}
```

`delegate("ChildName", "signal", args)` is how the parent calls into
an interior child.

## Why interior cells

Three reasons:

1. **Locality.** Children run in the parent's process — no network
   round-trip for delegation. Calls are direct in-process dispatches.
2. **Scoped composition.** A child's memory and state machine are
   private to the parent. The parent doesn't expose them via its own
   face.
3. **[[budget-proof]] aggregation.** The parent's `scale.memory: …`
   bound must cover the sum of its children's peak memory. The checker
   reports per-child contributions.

## What the compiler verifies

For interior children:

- **Face contracts** — every signal declared in a child's face has an
  `on` handler in that child (same as top-level cells).
- **Signal composition** — every `emit` in the parent that targets a
  child must match a handler. Every child's `await` must be matched
  by a parent or sibling emit. See [[composition]].
- **Budget aggregation** — `parent.peak + children.peaks ≤
  parent.declared_budget`. Reported in `soma check`.

## Example: the rebalancer

```soma
cell PortfolioSystem {
    scale { memory: "512Mi" }

    memory { lifecycle: Map [persistent, consistent] }

    state rebalance { initial: requested … }

    interior {
        cell Alpha     { memory { factors: Map [ephemeral] }  on score(…) { … } }
        cell Optimizer { scale  { memory: "128Mi" }            on optimize(…) { … } }
        cell Compliance [model: claude] { … }                 # agent cell
        cell Commentary [model: claude] { … }                 # agent cell
    }

    on rebalance(input: Map) {
        let scores = delegate("Alpha",     "score",    input)
        let opt    = delegate("Optimizer", "optimize", scores)
        let approval = delegate("Compliance", "review", opt)
        if approval.verdict != "APPROVE" { return blocked }
        let report = delegate("Commentary", "write", opt)
        finalize(input, opt, report)
    }
}
```

Children compose into a verified pipeline. The parent owns the state
machine; children own their own (sub-)state machines.

## Signal flow

Interior cells participate in the signal bus:

- `emit signal_name(args)` in a parent or sibling can target any
  child's matching `on` handler.
- `await signal_name -> handler_name` waits for an external emit.
- `delegate("Cell", "signal", …)` is **synchronous request-response**;
  `emit` is **fire-and-forget**.

The [[composition]] checker verifies every emit has a matching handler
and every handler has at least one emit source, across the
parent/children scope.

## Examples

A pipeline with three stages:

```soma
cell Pipeline {
    interior {
        cell Validator  { on validate(d: Map) -> Map { … } }
        cell Enricher   { on enrich(d: Map) -> Map { … } }
        cell Writer     { on write(d: Map) -> Map { … } }
    }
    on process(data: Map) {
        let v = delegate("Validator", "validate", data)
        if v.error != () { return v }
        let e = delegate("Enricher", "enrich", v.data)
        delegate("Writer", "write", e.data)
    }
}
```

A parent with an agent child:

```soma
cell Desk {
    memory { trades: Map [persistent, consistent] }
    interior {
        cell agent Triage {
            face { signal classify(t: Map) -> String }
            on classify(t: Map) {
                think("Classify this trade: {t}", map("max_tokens", 100))
            }
        }
    }
    on submit(t: Map) {
        let cls = delegate("Triage", "classify", t)
        trades.set(t.id, to_json(t |> with("class", cls)))
    }
}
```

The parent's budget must cover the agent's `think()` allocation.

## Edge cases

- Interior children **cannot** have their own `scale { replicas: > 1
  }` — they live in the parent's process. They CAN have their own
  `scale { memory: "..." }` for budget sub-allocation.
- A child can have its own interior children (recursive nesting).
- `delegate` traverses the cell tree by name; a name collision across
  interior scopes is a compile error.

## What this does NOT cover

- Cross-process composition — `delegate` does not call into another
  process. For that, use [[signals]] over the inter-process signal bus
  (with `link("host:port")` or `[peers]` in `soma.toml`).
- Capability isolation — any cell can `delegate` to any sibling. No
  permission gates yet. See [[whats-missing]].
