---
name: stdlib-collections
description: Builtins for List, Map, and basic data manipulation.
type: reference
since: V1.0
related: [stdlib-pipes, pipes, lambdas]
---

# Stdlib: collections

Builtins for constructing and manipulating `List` and `Map` values. The
canonical reference is `stdlib/builtins.cell`.

## Construction

- `list(a, b, c, ...)` — variadic List literal. **Note**: if the first
  arg is itself a List, it's flattened. To nest, use [[stdlib-linalg]]
  `rows()` or build with `push(list(row1), row2)`.
- `map(k1, v1, k2, v2, ...)` — variadic Map literal. Even arg count
  required.
- `push(lst, item)` — append `item` to `lst`. Returns new List.
- `with(m, k, v)` — copy `m` with key `k` set to `v`. Returns new Map.

## Indexing & lookup

- `nth(lst, i)` — get the i-th element of `lst`. Returns `()` if out
  of bounds.
- `len(x)` — length of a List, Map, or String.
- `m.get(k)` — Map field access (method form). Returns `()` if missing.
- `m.k` — Map field access (sugar). Same semantics.
- `m.keys` — List of keys (no parens; not a function).
- `m.values` — List of values.

## Aggregation

- `first(lst)` / `last(lst)` — first / last element.
- `reverse(lst)` — reversed List.
- `flatten(lst_of_lst)` — flatten one level.
- `sort(lst)` — ascending. `sort(lst, "desc")` — descending.

## Range

- `range(n)` — `[0, 1, ..., n-1]`.
- `range(from, to)` — `[from, from+1, ..., to-1]`.

## Higher-order (with lambdas — see [[stdlib-pipes]] for pipe forms)

- `map(lst, x => f(x))` — transform each element.
- `filter(lst, x => p(x))` — keep matching.
- `reduce(lst, initial, p => p.acc + p.val)` — fold.
- `find(lst, x => p(x))` — first match or `()`.
- `any(lst, x => p(x))` / `all(lst, x => p(x))` — Bool.
- `count(lst, x => p(x))` — Int.

## Examples

```soma
let xs = list(1, 2, 3, 4, 5)
let doubled = xs |> map(x => x * 2)               // [2, 4, 6, 8, 10]
let evens = xs |> filter(x => x % 2 == 0)          // [2, 4]
let sum = xs |> reduce(0, p => p.acc + p.val)      // 15
let first_big = xs |> find(x => x > 3)             // 4

let m = map("name", "Alice", "age", 30, "active", true)
let name = m.name                                  // "Alice"
let age = m.get("age")                              // 30
let keys = m.keys                                  // ["name", "age", "active"]

let enriched = m |> with("score", 95)              // same map + score
```

## Edge cases

- `list(list(1, 2), 3, 4)` flattens to `[1, 2, 3, 4]`. The "first arg
  is a List" flattening is documented but surprising. Use
  [[stdlib-linalg]] `rows()` for nested-list construction.
- `m.get(k)` returns `()` (Unit) for missing keys. Compare with
  `!= ()` rather than `!= null`.
- `nth(lst, -1)` is out of bounds, not "last element". Use
  `nth(lst, len(lst) - 1)` or `last(lst)`.

## Related

- [[stdlib-pipes]] — chained pipe-form `|> map / filter / sort_by / top`.
- [[stdlib-strings]] — string operations.
- [[stdlib-storage]] — memory-slot method calls (`.set` / `.get`).
