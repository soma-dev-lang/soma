---
name: stdlib-pipes
description: Pipe-form (`|>`) builtins — `map / filter / find / top / group_by / sort_by`.
type: reference
since: V1.0
related: [pipes, lambdas, stdlib-collections]
---

# Stdlib: pipes

Builtins designed to be used with the [[pipes]] operator. The
canonical reference is `stdlib/builtins.cell`.

## Higher-order (with lambdas)

```soma
data |> map(x => x.score * 2)
data |> filter(x => x.active)
data |> find(x => x.id == target)         // first match or ()
data |> any(x => x.valid)                  // Bool
data |> all(x => x.valid)                  // Bool
data |> count(x => x.score > 80)           // Int
data |> reduce(0, p => p.acc + p.val)
data |> each(x => print(x.name))            // for side effects; returns ()
data |> partition(x => x.active)            // { yes: List, no: List }
```

## Field-based (no lambda needed)

```soma
data |> filter_by("price", ">", 100)        // ops: > >= < <= == !=
data |> sort_by("score", "desc")             // "asc" or "desc"
data |> group_by("dept")                     // Map<String, List>
data |> distinct("category")                 // List of unique values
data |> top(10)                               // first N
data |> bottom(5)                             // last N
```

## Set-like

```soma
data |> reverse()
data |> flatten()                             // one-level flatten
data |> zip(other_list)                       // List of pairs
list("a", "b", "c") |> join(", ")           // "a, b, c"
```

## Examples

A scoring pipeline:

```soma
let top_10 = students
    |> filter(s => s.active)
    |> map(s => s |> with("score", s.x * 2 + s.y))
    |> sort_by("score", "desc")
    |> top(10)
    |> map(s => s.name)
```

Aggregation by group:

```soma
let by_dept = employees
    |> group_by("dept")
    |> map(g => map(
        "dept", g.key,
        "count", len(g.values),
        "total_salary", g.values |> reduce(0, p => p.acc + p.val.salary)
    ))
```

## Edge cases

- Field-based builtins use Map field access. If the field is missing
  the value is treated as `()`, which compares less than any other
  value.
- `top(0)` returns an empty List.
- `partition` returns a Map with exactly two keys, `yes` and `no`.
  Both are always present (possibly empty).

## Related

- [[pipes]] — the `|>` operator itself.
- [[lambdas]] — function literal syntax.
- [[stdlib-collections]] — non-pipe forms.
