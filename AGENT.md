# Soma — Agent Guide

> You are an AI agent building systems in Soma.
> Read this file completely before generating any code.
> Use the MCP tools (`soma_generate`, `soma_check`, `soma_verify`) or CLI equivalents.

## What Soma is

A language where every system is a **cell**. A cell has:
- **memory** — state (persistent or ephemeral)
- **state machines** — lifecycle with verified transitions
- **handlers** (`on`) — functions that process signals
- **scale** — distribution declaration (replicas, consistency, sharding)
- **face** — contracts (compile-time checked)

The same code runs on 1 machine or 1000. No Docker, no YAML, no config files.

## Your workflow

```
1. Generate the .cell file       → soma_generate("app.cell", code)
2. Auto-fix errors               → soma fix app.cell (adds missing handlers, fixes properties)
3. Check for remaining errors    → soma_check("app.cell")
4. Verify state machines         → soma_verify("app.cell")
5. If verify fails: read counter-example, fix state machine, goto 4
6. Serve                         → soma_serve("app.cell", 8080)
```

**`soma fix` is your best friend.** It auto-repairs missing handlers, contradictory properties, and more. Use it before manual fixing.

## Syntax rules — memorize these

```
# These are WRONG in Soma:
function foo() { }          # WRONG — use: on foo() { }
null                        # WRONG — use: ()
[1, 2, 3]                  # WRONG — use: list(1, 2, 3)
{key: val}                 # WRONG — use: map("key", val)
import x                   # WRONG — use: use lib::x
console.log(x)             # WRONG — use: print(x)
x === y                    # WRONG — use: x == y
;                          # WRONG — no semicolons
let x = if a { b } else { c }  # WRONG — if is not an expression
```

```
# These are RIGHT:
on handler_name(param: Type) { }     # handler
let x = ()                           # null
let items = list(1, 2, 3)            # list
let m = map("name", "Alice", "age", 30)  # map (MUST have even args)
"hello {name}"                       # string interpolation
data |> filter(x => x.score > 50)    # pipe + lambda
data |> with("field", value)         # immutable update
```

## Cell template — start from this

```soma
cell AppName {
    memory {
        items: Map<String, String> [persistent, consistent]
    }

    state workflow {
        initial: draft
        draft -> active
        active -> completed
        active -> cancelled
        * -> archived
    }

    on create(data: Map) {
        let id = to_string(now()) + "_" + to_string(random(10000))
        let item = map(
            "id", id,
            "name", data.name ?? "untitled",
            "status", "draft",
            "created_at", to_string(now())
        )
        items.set(id, to_json(item))
        return map("status", "created", "id", id)
    }

    on request(method: String, path: String, body: String) {
        let req = map("method", method, "path", path)
        match req {
            {method: "POST", path: "/create"} -> create(body)
            {method: "GET", path: "/items"} -> {
                items.values() |> map(s => from_json(s)) |> filter(i => i.id != ())
            }
            {method: "GET", path: "/"} -> map("name", "AppName", "items", items.len())
            _ -> response(404, map("error", "not found"))
        }
    }
}
```

## DO and DON'T

### DO

- **DO** store complex values directly: `items.set(id, map(...))` — storage auto-serializes maps and lists
- **DO** read them back directly: `let user = items.get(id)` then `user.name` — auto-deserialized
- **DO** use `data.field ?? "default"` for null-safe access
- **DO** use `to_string()` when storing numbers in maps: `"qty", to_string(qty)`
- **DO** create a `soma.toml` with `[verify]` properties for every state machine
- **DO** use `[persistent, consistent]` for data that matters, `[ephemeral, local]` for caches
- **DO** use pipes: `data |> filter(x => x.active) |> sort_by("name") |> top(10)`
- **DO** handle the `on request(method, path, body)` pattern for web apps
- **DO** use `_prefix` for private handlers (not exposed as HTTP endpoints)
- **DO** check `if raw == ()` before calling `from_json(raw)` on storage gets

### DON'T

- **DO** use `if` as an expression: `let x = if a { b } else { c }` (else branch required)
- **DON'T** use multi-line pipes after `->` in match arms — put the pipe on one line or use if/else
- **DON'T** use `to_json()` when storing maps — storage auto-serializes: just `items.set(id, data)`
- **DON'T** use `unique()` — use `distinct()`
- **DON'T** use `async/await` — Soma is synchronous, use `every Ns {}` for periodic tasks and `after Ns {}` for one-shot delays
- **DON'T** use classes or inheritance — everything is a cell with handlers
- **DON'T** return inside a `for` loop expecting it to return from the handler — `return` exits the handler, use a variable instead
- **DON'T** put `return` after `match` — match is an expression, just use it directly or assign it

## State machines — the differentiator

State machines are verified at compile time. Design them carefully:

```soma
state order {
    initial: pending
    pending -> confirmed
    confirmed -> shipped
    shipped -> delivered
    pending -> cancelled
    confirmed -> cancelled
    shipped -> failed
    failed -> pending          // retry
    * -> expired               // catch-all timeout
}
```

Then in `soma.toml`:
```toml
[verify]
deadlock_free = true
eventually = ["delivered", "cancelled", "expired"]

[verify.after.shipped]
eventually = ["delivered", "failed", "expired"]

[verify.after.confirmed]
never = ["pending"]
```

`soma verify` will prove these properties or give you a counter-example trace to fix.

**Common verify failure**: cycles like `failed -> pending -> confirmed -> shipped -> failed` mean "eventually delivered" is unprovable (the cycle can loop forever). Either:
- Accept it (use `eventually = ["confirmed", "cancelled", "expired"]` — a weaker but true property)
- Or add a retry limit in code and an `expired` terminal state

## Scale section — for distributed systems

```soma
scale {
    replicas: 10
    shard: items              // which memory to distribute
    consistency: eventual     // strong | causal | eventual
    tolerance: 2              // survives N node failures
    cpu: 2
    memory: "1Gi"
}
```

Rules:
- `shard` must reference a declared memory slot name
- `[ephemeral]` + `consistency: strong` = compile error
- `[persistent, consistent]` + `consistency: eventual` = warning (declared consistent but accepting stale reads)
- Only one memory slot can be sharded

## Types reference

| Type | Literal | Notes |
|------|---------|-------|
| Int | `42` | 64-bit |
| Float | `3.14`, `1.5e3` | 64-bit |
| String | `"hello {name}"` | interpolation |
| Bool | `true`, `false` | |
| List | `list(1, 2, 3)` | |
| Map | `map("k", v, "k2", v2)` | MUST have even number of args |
| Unit | `()` | null equivalent |

## Builtin functions — the ones you'll use most

```
# Storage
data.set("key", to_json(value))
data.get("key")                    → String or ()
data.delete("key")
data.keys()                        → List
data.values()                      → List
data.len()                         → Int

# JSON
to_json(map(...))                  → String
from_json(string)                  → Map or ()

# Collections
len(list), push(list, item), nth(list, index)
sort(list), reverse(list), range(0, 10)

# Pipes
data |> map(x => expr)
data |> filter(x => bool)
data |> find(x => bool)
data |> sort_by("field")
data |> top(N)
data |> reduce(init, p => p.acc + p.val)
data |> with("field", value)       # add/update field
data |> distinct("field")          # unique values
data |> group_by("field")

# Strings
len(s), contains(s, sub), split(s, sep), trim(s)
replace(s, old, new), uppercase(s), lowercase(s)
to_string(any), to_int(s), to_float(s)
escape_html(s)

# Math
abs, round, floor, ceil, min, max, sqrt, log, pow
random(), random(100), random(10, 20)

# Time
now(), now_ms(), date_now()

# HTTP responses
html("<h1>Hi</h1>")
response(201, map("id", 1))
redirect("/other")
```

## Reading verification output

When `soma_verify` returns JSON:

```json
{
  "passed": false,
  "temporal": [{
    "property": "after('shipped', eventually delivered|failed)",
    "passed": false,
    "counter_example": ["shipped", "failed", "pending", "confirmed", "shipped", "...cycle"]
  }]
}
```

This means: after reaching "shipped", the system can enter a cycle `shipped → failed → pending → confirmed → shipped` that never reaches "delivered". Fix: add a retry limit or weaken the property.

## Generating soma.toml

Always generate a `soma.toml` alongside the `.cell` file:

```toml
[package]
name = "my-app"

[verify]
deadlock_free = true
eventually = ["completed", "cancelled"]

[verify.after.active]
eventually = ["completed", "cancelled"]
```

## Final checklist before serving

1. `soma_check` returns `{"passed": true}` — no contract violations
2. `soma_verify` returns `{"passed": true}` — all temporal properties hold
3. Every state machine has a terminal state reachable from every other state
4. Every `POST` handler has a matching `if path == "/route"` in `on request()`
5. All stored values use `to_json()` / `from_json()` roundtrip
