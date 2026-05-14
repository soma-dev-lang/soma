---
name: sum-types
description: V1.5 tagged unions with exhaustiveness checking and state-machine integration.
type: feature
since: V1.5
related: [pattern-matching, state-machine, refinement, verification-overview]
---

# Sum Types

A **sum type** declares a value that is "exactly one of N alternatives,
each with its own fields." Soma calls them `cell type` declarations
with a `variants` section. With sum types, the compiler enforces:

- **Exhaustive pattern matching** — a `match` that doesn't cover every
  variant is a compile error.
- **Typed state machines** — `state X: T { … }` annotation means
  every transition target is a variant of `T`. Typo → compile error.
- **Tagged values** — no more stringly-typed `Map<String, String>` with
  a hidden `_type` field.

## Syntax

```soma
cell type OrderStatus {
    variants {
        Pending
        Validated
        Filled
        Cancelled
    }
}
```

Three variant shapes:

```soma
cell type PaymentResult {
    variants {
        Charged { transaction_id: String, amount: Int }      // struct variant
        Declined { reason: String, code: Int }
        Pending                                               // unit variant
    }
}

cell type Move {
    variants {
        Up(Int)                                               // tuple variant
        Down(Int)
        Wait                                                  // unit
    }
}
```

## Construction

```soma
let s = Pending                                       // unit
let r = Charged { transaction_id: "T-1", amount: 100 }  // struct
let m = Up(3)                                          // tuple
```

The expression position resolves the constructor by looking it up in
the program's variant registry (see `compiler/src/interpreter/mod.rs`).

## Pattern matching

```soma
match r {
    Charged { transaction_id, amount } -> "ok {transaction_id} ${amount}"
    Declined { reason, code }           -> "rejected ({code}): {reason}"
    Pending                              -> "..."
}
```

Patterns:

- `Pending` — matches the unit variant
- `Charged { id, amount }` — destructures both fields (binding shorthand)
- `Charged { amount, .. }` — partial; `..` allows unmatched fields
- `Up(n)` — tuple destructure
- `Charged { code: 503, reason }` — match a specific field value plus
  bind another

See [[pattern-matching]] for the full pattern grammar.

## Exhaustiveness check (the headline feature)

```soma
match status {
    Pending   -> "waiting"
    Validated -> "ready"
    Filled    -> "done"
}
// ERROR: non-exhaustive match on 'OrderStatus': missing variant `Cancelled`
```

The check fires at `soma check` time. To opt out, add `_ -> default`:

```soma
match status {
    Pending -> "waiting"
    _        -> "moving"        // catches everything else
}
```

Guards (`pattern if cond -> …`) make an arm conditional; the checker
treats guarded arms as non-exhaustive contributors.

## Typed state machines (the killer feature)

The `state` section can be annotated with a sum type:

```soma
cell type OrderState {
    variants { Pending; Validated; Filled; Cancelled }
}

cell Order {
    state order: OrderState {            // ← typed annotation
        initial: Pending
        Pending -> Validated
        Validated -> Filled
        * -> Cancelled
    }

    on advance(id: String) {
        transition(id, Validated)        // typed; typo = compile error
    }
}
```

With the `: OrderState` annotation, three things are now checked at
compile time:

1. **Every state name** in the block is a variant of `OrderState`.
2. **Initial** is a variant of `OrderState`.
3. **Every `transition(id, X)` call** in any handler uses a valid
   variant of `OrderState`.

Typo demo:

```
$ soma check engine.cell
error: handler 'advance' transitions to 'Shipd', which is not a variant of 'OrderState'
```

The error names *both the bad identifier and the type* — `OrderState`
is in the message. Before V1.5, the V1.3 refinement check could only
say "undeclared state" if it caught the typo at all.

## Examples

A `PaymentGateway` returning a sum-type result that callers must
exhaustively handle:

```soma
cell type PaymentResult {
    variants {
        Charged { transaction_id: String, amount: Int }
        Declined { reason: String, code: Int }
        Pending
    }
}

cell PaymentGateway {
    face { signal charge(amount: Int, card: String) -> PaymentResult }
    on charge(amount: Int, card: String) {
        if amount > 0 { return Charged { transaction_id: "TX-X", amount } }
        return Declined { reason: "negative amount", code: 400 }
    }
}

cell Caller {
    on pay() {
        let res = delegate("PaymentGateway", "charge", 100, "4111")
        match res {
            Charged { transaction_id, amount } -> log_ok(transaction_id)
            Declined { reason, code }          -> log_error(code, reason)
            // Compiler: missing variant `Pending`
        }
    }
}
```

## Coexistence with records

Record literals (`User { name: "Alice", age: 30 }`) and struct-variant
constructors (`Accepted { id: "X" }`) share syntax. The interpreter
disambiguates by checking the type name against the variant registry:
known variant name → variant value; otherwise → record literal.

## Internal representation

A variant value is `Value::Variant { type_name, variant, fields }`
where fields is one of:

- `VariantValue::Unit`
- `VariantValue::Tuple(Vec<Value>)`
- `VariantValue::Struct(IndexMap<String, Value>)`

Display, equality, JSON serialization, and storage are all defined.

## Edge cases

- Two `cell type` declarations defining the same variant name → compile
  error (duplicate variant). Resolution must be unambiguous.
- Qualified form `OrderState::Pending` is supported in patterns but the
  unqualified form usually wins by registry lookup.
- The `__sampled__` field convention (used by `to_sampled` in
  [[stdlib-linalg]]) is a Map-based handle, not a variant — different
  mechanism, same idea of tagged values.

## What this does NOT cover (deferred to V1.6+)

- **`Option<T>` and `Result<T, E>` built-ins.** Designed in
  `SUM_TYPES_DESIGN.md` but not yet hardcoded.
- **Methods on sum types** — `impl OrderStatus { … }` is not in V1.5.
- **Parametric polymorphism** for user-defined generics.
- **VM bytecode dispatch** for variant patterns — V1.5 the VM stub
  falls through to wildcard; the interpreter has the full
  implementation.
- **Coq proof** (`Soma_SumTypes.v`) — designed, not landed.

See the full RFC: `SUM_TYPES_DESIGN.md`.
