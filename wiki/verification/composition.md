---
name: composition
description: Inter-cell signal matching — every `await` has an `emit`, every `on` has a source.
type: feature
since: V1.0
related: [signals, interior, face, verification-overview]
---

# Composition

The composition checker verifies that signals flowing between cells
match up. For every `await`, there's an `emit` somewhere reachable;
for every `on signal_name`, there's at least one cell that can emit
it; arity and types are consistent.

`soma verify` output:

```
✓ composition: all signals matched
  10 emit sites, 12 handlers, 0 orphans
```

## What's checked

Per [[interior]] block (parent + children), three classes of signal:

1. **Local emits** — `emit X(args)` from within the cell. The emit
   site is the source; matched against any `on X(...)` in the same
   cell or interior children.

2. **External emits** — `emit X(args)` going to a peer (via
   `soma.toml` `[peers]`) or to a parent. The checker can't see
   the receiver, so it records a "potential external receiver"
   without erroring.

3. **`await` blocks in `runtime { }`** — when a cell explicitly
   awaits a signal it doesn't emit itself. Composition requires a
   sibling or parent to emit it.

Diagnostics:

- **`UnmatchedAwait { cell, signal }`** — a cell awaits `X` but no
  reachable peer emits `X`. Reported as warning (might be external).
- **`UnmatchedHandler { cell, signal }`** — a cell has `on X(...)`
  but no cell in scope emits `X`. Reported as warning.
- **`SignalTypeMismatch { signal }`** — an emit and a handler agree
  on signal name but disagree on parameter arity. Reported as error.

## What's NOT checked

- **Deep type compatibility.** Parameter types are matched by name
  (`Map` vs `Map`) and arity, not deeply unified. A `Map<String,
  String>` and `Map<String, Int>` are interchangeable from the
  composition checker's perspective.
- **Causal ordering across cells.** "Cell A emits X, then cell B
  emits Y" — composition doesn't track this. The runtime delivers
  signals in source-emit order, but cross-cell ordering has no
  formal semantics. See [[whats-missing]].
- **Cross-process composition.** When a peer is reachable only via
  `[peers]` in `soma.toml`, the checker can't introspect the
  peer cell. Composition checking stops at the process boundary.

## How signals flow

```
parent cell
├── emit foo(args)           ──┐
│                              │
└── interior {                 ▼
    cell A {
        on foo(args) { ... }   ◀── matched
        emit bar(args)         ─┐
    }                           │
    cell B {                    ▼
        on bar(args) { ... }   ◀── matched
    }
}
```

The checker walks the tree from parent to interior children, building
the emit and handler indices in scope. Cross-scope matches require
explicit cell-to-cell visibility.

## Examples

A clean composition (everything matched):

```soma
cell Pipeline {
    on process(data: Map) {
        emit validate(data)
    }
    interior {
        cell Validator {
            on validate(data: Map) {
                emit enrich(data)
            }
        }
        cell Enricher {
            on enrich(data: Map) {
                emit store(data)
            }
        }
        cell Store {
            on store(data: Map) { ... }
        }
    }
}

// soma verify:
// ✓ composition: 3 emits, 3 handlers, 0 orphans
```

An orphan (handler with no emitter):

```soma
cell Listener {
    on never_called(data: Map) { ... }    // nobody emits never_called
}

// soma verify:
// ⚠ UnmatchedHandler: 'Listener' handles 'never_called' but no sibling emits it
```

An arity mismatch (compile error):

```soma
cell A {
    on event(a: String, b: Int) { ... }
}
cell B {
    on run() {
        emit event("hello")                // wrong arity — 1 arg, expected 2
    }
}

// soma verify:
// ✗ SignalTypeMismatch: signal 'event' arity differs between emitter and handler
```

## Cross-process composition

`soma.toml`:

```toml
[peers]
exchange = "localhost:8082"
```

```soma
emit trade(map("ticker", "BTC", "qty", 1))
// → delivered locally + to the cell at localhost:8082
```

Composition checking on the local side: the emit is recorded, but
since the peer cell isn't in scope, no orphan warning is emitted.

If the peer cell does NOT have an `on trade(...)` handler, this is
silently ignored at runtime (no error, no log). For critical signals,
prefer `delegate("CellAtPeer", "method")` which gives a structured
error map.

## Why this matters

Without composition checking:

- A handler typo (`on tarade` instead of `on trade`) silently fails
  at runtime. No callers reach the handler; no errors.
- Removing an `emit` leaves dead handlers nobody notices.
- Adding a new signal forgets to wire a recipient.

With it:

- Signal-routing bugs are surfaced at compile time.
- Refactoring is safer — moving a handler between cells produces
  diagnostics.

## Edge cases

- Wildcard handlers (`on _` for catchalls) — not in V1.5.
- Self-emits (`cell A { on tick { emit tick(...) }} `) — fine; the
  emit and handler are in the same cell.
- An `await` inside an `on handler` — checked the same way as
  `await` in a `runtime` block.

## Related

- [[signals]] — the `emit` / `await` / `on` semantics.
- [[interior]] — the composition scope.
- [[face]] — the public contract that drives most signals.
- [[verification-overview]] — how composition fits in.
