# Soma Language Reference — for AI Agents

> Give this file to an AI agent as context when asking it to write Soma code.

## Quick rules

- No semicolons. Newlines separate statements.
- No `function`/`def`. Use `on handler_name(params) { }`.
- No `null`. Use `()` for null/unit.
- No `[1,2,3]`. Use `list(1, 2, 3)`.
- No `{key: val}`. Use `map("key", val, "key2", val2)`.
- No `import`. Use `use lib::module`.
- No `console.log`. Use `print(value)`.
- No `===`. Use `==`.
- Strings: `"hello {name}"` (interpolation with `{}`).
- Multi-line strings: `"""..."""` (raw, no escape needed, quotes work inside).
- Integer division: `7 / 2 = 3.5` (auto-promotes to float when non-exact).

## Cell structure

```soma
cell AppName {
    memory {
        data: Map<String, String> [persistent, consistent]    // → SQLite
        cache: Map<String, String> [ephemeral, local]         // → in-memory
    }

    state workflow {
        initial: draft
        draft -> review
        review -> approved
        review -> rejected
        * -> cancelled
    }

    every 30s {
        // runs periodically
    }

    on handler_name(param1: Type, param2: Type) {
        // handler body
        return value
    }

    on request(method: String, path: String, body: String) {
        match path {
            "/" -> html(dashboard())
            "/api/data" -> get_data()
            _ -> response(404, map("error", "not found"))
        }
    }
}
```

## Types

| Type | Example | Notes |
|------|---------|-------|
| Int | `42`, `-1`, `0` | 64-bit |
| Float | `3.14`, `1.5e3` | 64-bit, scientific notation |
| String | `"hello {name}"` | interpolation with `{}` |
| Bool | `true`, `false` | |
| List | `list(1, 2, 3)` | ordered |
| Map | `map("key", val)` | key-value pairs, MUST have even args |
| Unit | `()` | null equivalent |
| Duration | `5s`, `1min`, `500ms`, `1h` | converts to milliseconds |
| Record | `User { name: "Alice", age: 30 }` | typed map with `_type` field |

## Variables

```soma
let x = 42
x = x + 1                    // reassignment
x += 10                      // compound assignment
let name = "world"
let greeting = "hello {name}" // interpolation
```

## Control flow

```soma
if condition {
    // ...
} else if other {
    // ...
} else {
    // ...
}

while condition {
    if done { break }
    if skip { continue }
}

for item in list(1, 2, 3) {
    print(item)
}

for i in range(0, 10) {
    // 0 to 9
}

match value {
    "a" -> expr1
    "b" -> { stmts; expr2 }
    42 -> expr3
    () -> expr4              // match null
    _ -> default_expr        // wildcard
}
```

## Functions (handlers)

```soma
on add(a: Int, b: Int) {
    return a + b
}

on _private_helper() {
    // underscore prefix = not exposed as HTTP endpoint
    return "internal"
}

// Call: let result = add(1, 2)
```

## Lambdas

```soma
let doubled = list(1, 2, 3) |> map(x => x * 2)
let evens = data |> filter(x => x % 2 == 0)
let found = data |> find(x => x.id == target)
let has = data |> any(x => x.active)
let ok = data |> all(x => x.valid)
let n = data |> count(x => x.score > 80)

// Block lambda
let enriched = data |> map(s => {
    let score = s.x * 2 + s.y
    s |> with("score", score)
})

// Reduce
let sum = list(1, 2, 3) |> reduce(0, p => p.acc + p.val)
```

## Collections

```soma
// List
let items = list(1, 2, 3)
let items = push(items, 4)          // append
let first = nth(items, 0)           // index access
let rev = reverse(items)
let r = range(0, 10)                // [0..9]
let sorted = sort(items)            // ascending
let sorted = sort(items, "desc")    // descending
let n = len(items)

// Map
let m = map("name", "Alice", "age", 30)
let name = m.name                   // field access
let age = m.get("age")              // method access
let keys = m.keys                   // no parentheses
let vals = m.values
let updated = m |> with("email", "a@b.com")
```

## Pipe operators

```soma
// Higher-order (with lambdas)
data |> map(s => s.name)
data |> filter(s => s.score > 50)
data |> find(s => s.id == target)
data |> any(s => s.active)
data |> all(s => s.valid)
data |> count(s => s.score > 80)
data |> reduce(0, p => p.acc + p.val)

// Field-based
data |> filter_by("price", ">", 100)     // operators: > >= < <= == !=
data |> sort_by("score", "desc")
data |> top(10)
data |> bottom(5)
data |> agg("sector", "price:sum", "vol:avg")
data |> group_by("dept")
data |> distinct("category")              // unique values

// Column-wise (statistics)
data |> zscore("field")                   // adds field_z column
data |> rank("field")                     // adds field_rank column
data |> normalize("field", 0, 100)        // adds field_norm column
data |> winsorize("field", 0.05, 0.95)    // clamp at percentile bounds
percentile(data, "field", 0.9)            // terminal: returns value
median(data, "field")
std_by(data, "field")

// DataFrame
data |> select("field1", "field2")
data |> rename("old", "new")
data |> with("new_field", value)          // add/update field on each map
data |> join(other, "key")
data |> describe("field")                 // {count, sum, avg, min, max}

// Utilities
data |> flatten()
data |> reverse()
data |> zip(other)
list("a", "b", "c") |> join(", ")        // "a, b, c"
```

## String builtins

```soma
len("hello")                    // 5 (chars, not bytes)
contains("hello", "ell")        // true
starts_with("hello", "he")      // true
ends_with("hello.txt", ".txt")  // true
replace("hello", "l", "r")     // "herro"
split("a,b,c", ",")            // ["a", "b", "c"]
trim("  hi  ")                  // "hi"
uppercase("hello")              // "HELLO"
lowercase("HELLO")              // "hello"
substring("hello", 1, 3)        // "el"
index_of("hello", "ll")         // 2
escape_html("<b>x</b>")         // "&lt;b&gt;x&lt;/b&gt;"
to_json(map("a", 1))            // "{\"a\": 1}"
from_json("{\"a\": 1}")         // map
```

## Math builtins

```soma
abs(-5)             // 5
round(3.7)          // 4
floor(3.7)          // 3
ceil(3.2)           // 4
min(3, 7)           // 3
max(3, 7)           // 7
clamp(15, 0, 10)    // 10
pow(2, 10)          // 1024.0
sqrt(16.0)          // 4.0
log(2.718)          // ~1.0
exp(1.0)            // ~2.718
log10(100.0)        // 2.0
random()            // float 0.0..1.0
random(100)         // int 0..99
random(10, 20)      // int 10..19
```

## Type conversion

```soma
to_int("42")        // 42
to_int("abc")       // () — returns null, not 0
to_int(3.7)         // 3
to_float(42)        // 42.0
to_string(42)       // "42"
type_of(42)         // "Int"
```

## Error handling

```soma
let result = try { risky_operation() }
if result.error != () {
    print("Error: {result.error}")
    return response(500, map("error", result.error))
}
let value = result.value

// try catches: division by zero, undefined variables,
// type errors, stack overflow, invalid transitions
```

## Storage

```soma
memory {
    data: Map<String, String> [persistent, consistent]   // → SQLite
    cache: Map<String, String> [ephemeral, local]        // → in-memory
}

// In handlers:
data.set("key", "value")
let val = data.get("key")           // returns () if missing
data.delete("key")
let keys = data.keys                 // list of keys
let vals = data.values               // list of values
let n = data.len                     // count

// JSON roundtrip for complex values:
data.set("user", to_json(map("name", "Alice", "age", 30)))
let user = from_json(data.get("user"))
```

## State machines

```soma
state order {
    initial: pending
    pending -> validated { guard { amount > 0 } }
    validated -> sent
    sent -> filled
    sent -> rejected
    filled -> settled
    * -> cancelled           // from any state
}

// In handlers:
transition("order_id", "validated")    // move state
let status = get_status("order_id")    // current state
let valid = valid_transitions("order_id") // available transitions
```

## HTTP server

```soma
// Run: soma serve app.cell
// Starts HTTP on :8080, WS on :8081, signal bus on :8082

on request(method: String, path: String, body: String) {
    match path {
        "/"         -> html(render_page())
        "/api/data" -> get_all_data()
        _           -> response(404, map("error", "not found"))
    }
}

// Response types:
html("<h1>Hello</h1>")                // text/html
map("key", "value")                    // application/json (auto)
response(201, map("id", 1))           // custom status code
redirect("/other")                     // 302 redirect
sse("trade", "update")                // SSE event stream
```

## Inter-process signals

```soma
// soma.toml
// [peers]
// exchange = "localhost:8082"

// Send (goes to all peers):
signal order(map("ticker", "BTC", "qty", 1))
// or:
emit trade(fill_data)

// Receive (auto-dispatched from bus):
on trade(data: Map) {
    record_fill(data)
}
```

## File I/O

```soma
let content = read_file("data.txt")
write_file("output.txt", content)
let rows = read_csv("data.csv")       // list of maps, auto-typed
```

## Time

```soma
let ts = now()                // unix timestamp (seconds)
let ms = now_ms()             // milliseconds
let today = date_now()        // "2026-03-29"
let formatted = format_date(ts) // "2026-03-29"
```

## Verification

```soma
// soma.toml
// [verify]
// deadlock_free = true
// eventually = ["settled", "cancelled"]
// never = ["invalid"]
//
// [verify.after.sent]
// eventually = ["filled", "rejected"]

// Run: soma verify app.cell
```

## Face contracts (compile-time checked)

```soma
cell API {
    face {
        signal create(name: String) -> Map    // MUST have matching handler
        signal delete(id: String)              // MUST have matching handler
        promise all_persistent                 // structural check
        promise "human-readable description"   // generates warning
    }
    // Missing handler for 'delete' → compile error
}
```

## Tests

```soma
cell test MathTests {
    rules {
        assert 1 + 1 == 2
        assert len("hello") == 5
        assert round(3.7) == 4
    }
}
// Run: soma test file.cell
```

## Multi-file projects

```
project/
    app.cell          // main file
    lib/
        helpers.cell  // use lib::helpers
        scoring.cell  // use lib::scoring
    soma.toml         // config + peers + verify
```

```soma
// app.cell
use lib::helpers
use lib::scoring

cell App {
    on run() {
        let result = helper_function()   // from helpers.cell
    }
}
```

## Common patterns

```soma
// CRUD web app
cell App {
    memory { items: Map<String, String> [persistent, consistent] }

    on request(method: String, path: String, body: String) {
        if method == "POST" && path == "/api/items" {
            let data = from_json(body)
            let id = to_string(next_id())
            items.set(id, to_json(data |> with("id", id)))
            return data |> with("id", id)
        }
        match path {
            "/api/items" -> items.values |> map(s => from_json(s))
            _ -> response(404, map("error", "not found"))
        }
    }
}

// Data pipeline
cell Pipeline {
    on run() {
        let data = read_csv("input.csv")
        let result = data
            |> filter(s => s.score > 50)
            |> zscore("score")
            |> rank("score")
            |> sort_by("score_rank", "asc")
            |> top(10)
            |> select("name", "score", "score_rank")
        print(result)
    }
}

// Real-time with state machine
cell OrderSystem {
    memory { orders: Map<String, String> [persistent, consistent] }
    state order { initial: pending  pending -> validated  validated -> shipped  * -> cancelled }
    every 30s { check_stale_orders() }

    on create(data: Map) {
        let id = to_string(next_id())
        orders.set(id, to_json(data |> with("id", id) |> with("status", "pending")))
        return map("id", id)
    }

    on advance(id: String, target: String) {
        return transition(id, target)
    }
}
```
