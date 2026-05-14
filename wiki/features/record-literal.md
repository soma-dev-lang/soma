---
name: record-literal
description: `User { name: "Alice", age: 30 }` — typed record literals.
type: feature
since: V1.0
related: [sum-types, pattern-matching]
---

# Record literal

A record literal constructs a tagged Map with a `_type` field. The
syntax mirrors struct-variant construction (see [[sum-types]]) but
doesn't require a `cell type` declaration.

## Syntax

```soma
let u = User { name: "Alice", age: 30 }
// internally: Map("_type", "User", "name", "Alice", "age", 30)
```

The PascalCase identifier becomes the `_type` value. Fields can be
in any order.

## Pattern matching

Records destructure the same way as Maps:

```soma
match u {
    {name, age} if age >= 18 -> "adult {name}"
    {name}                    -> "minor {name}"
}
```

You can also match by `_type`:

```soma
match v {
    {_type: "User", name}    -> use(name)
    {_type: "Order", id}     -> proc(id)
    _                         -> default
}
```

## Coexistence with sum types

In V1.5, struct-variant constructors share this syntax. If the
identifier matches a known variant name (from a `cell type`
declaration), the expression is a [[sum-types]] variant value with
typed semantics. Otherwise it's a plain record (Map with `_type`).

```soma
cell type PaymentResult {
    variants { Charged { id: String, amount: Int }; Pending }
}

let v = Charged { id: "X", amount: 100 }    // variant — typed
let r = User { name: "Alice", age: 30 }     // record — untyped Map
```

## Examples

A record passed via the bus:

```soma
on submit(order: Map) {
    let normalized = Order {
        id: to_string(next_id()),
        symbol: order.symbol,
        qty: to_int(order.qty),
        side: order.side
    }
    emit place(normalized)
}

on place(order: Map) {
    match order {
        {_type: "Order", symbol, qty, side} -> execute(symbol, qty, side)
        _ -> reject(order)
    }
}
```

## Edge cases

- Records are Maps under the hood. `to_json(record)` includes the
  `_type` field. Useful for tagged serialization.
- Field access uses the standard Map syntax: `r.name`, `r.get("age")`,
  `r |> with("score", 95)`.
- For algebraically-meaningful tagging — variants of an enumeration
  with exhaustive matching — use [[sum-types]] instead.

## Related

- [[sum-types]] — typed variants with the same syntax.
- [[pattern-matching]] — destructuring patterns.
