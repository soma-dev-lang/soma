---
name: signals
description: emit / await — fire-and-forget cross-cell messaging on the bus.
type: feature
since: V1.0
related: [handler, interior, composition, think]
---

# Signals

A **signal** is a fire-and-forget message between cells. Where
`delegate` is synchronous request-response, `emit` is asynchronous
broadcast.

## `emit` — send a signal

```soma
emit trade_filled(map("id", "T-123", "qty", 100))
```

The signal goes to:

- every handler in the current cell, interior children, or siblings
  with `on trade_filled(d: Map) { … }`
- every peer cell registered in `soma.toml`'s `[peers]` over the
  network signal bus

`emit` is non-blocking. The caller doesn't see results.

## `signal` — alternative emit syntax

```soma
signal trade_filled(map("id", "T-123", "qty", 100))
```

Identical semantics. `signal` is the preferred form inside `runtime {}`
blocks of meta-cells (`cell property`, `cell backend`, etc.).

## Receiving — `on` handlers

```soma
cell AuditLog {
    memory { events: Map<String, String> [persistent, immutable] }

    on trade_filled(data: Map) {
        events.set(to_string(now()), to_json(data))
    }
}
```

Any cell with a matching `on signal_name` receives the signal. Names
must match exactly; signal types are arity-checked (parameter count),
not deeply unified.

## Cross-process signals

`soma.toml`:

```toml
[peers]
exchange = "localhost:8082"
```

```soma
emit trade(map("ticker", "BTC", "qty", 1))
// → delivered to the local cell AND to the cell at exchange:8082
```

The signal bus serializes the payload as JSON and POSTs to the peer.

## `await` — explicit handler binding

```soma
runtime {
    await trade_filled -> record_fill
}
```

Inside a `runtime` section, `await` binds an external signal name to
a local handler. The [[composition]] checker verifies every `await`
has a matching `emit` somewhere reachable.

## What the compiler verifies

Three properties — see [[composition]]:

1. **Every `await` has a source.** If a cell awaits `trade_filled` and
   no sibling/parent emits it, `soma check` warns
   `UnmatchedAwait { cell, signal }`.
2. **Every `on handler` has a source.** Conversely, if a cell handles
   `trade_filled` and nobody emits it, `UnmatchedHandler` warns.
3. **Signal type compatibility.** Parameter count must match between
   emitter and receiver.

The third check is **arity only** — Soma's type system doesn't
unify parameter types deeply. See [[whats-missing]].

## Examples

A producer / consumer pair:

```soma
cell Producer {
    every 5s { emit tick(map("ts", now())) }
}

cell Consumer {
    memory { counts: Map [ephemeral] }
    on tick(data: Map) {
        let cur = to_int(counts.get("n") ?? "0") + 1
        counts.set("n", to_string(cur))
    }
}
```

Multi-recipient signal:

```soma
cell Source {
    on event(data: Map) {
        emit log_event(data)
        emit metric_event(data)
        emit audit_event(data)
    }
}

cell Logger    { on log_event(d: Map)    { … } }
cell Metrics   { on metric_event(d: Map) { … } }
cell Auditor   { on audit_event(d: Map)  { … } }
```

The single `emit event` doesn't fan out automatically; the producer
explicitly fans out to the three sinks. (No "topic" abstraction.)

## `delegate` vs `emit`

| Aspect | `delegate("Cell", "sig", args)` | `emit sig(args)` |
|--------|----------------------------------|------------------|
| Sync/async | synchronous (returns a value) | asynchronous (returns Unit) |
| Recipient | named cell only | every matching handler |
| Failure | returns error map | silent (no callback) |
| Cross-process | yes if peer configured | yes if peer configured |

Use `delegate` when you need a result. Use `emit` for events that fan
out, broadcasts, or fire-and-forget.

## Edge cases

- Signal handlers run **after** the emitter's handler completes — the
  emit is enqueued, not interleaved.
- A signal sent to oneself is delivered; cells can `emit own_signal(…)`
  to schedule work for their next dispatch tick.
- The signal bus is **not** durable in V1.5. A process crash loses
  in-flight signals. For durable messaging, use `[persistent]`
  memory + scheduled handlers.

## What this does NOT cover

- **Formal ordering semantics.** Cross-cell signal ordering is
  documented as "best-effort, source-order" but lacks a CSP /
  π-calculus level spec. See [[whats-missing]].
- **Back-pressure** — there's no rate limiting or queue depth control
  for emit in V1.5. Producers can outpace consumers silently.
- **Dead-letter handling** — a signal nobody handles is dropped.
