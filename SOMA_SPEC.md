# SOMA Language Specification

> Machine-readable specification for AI agents and tooling.
> Version: 0.1.0

## Overview

Soma is a fractal, declarative, agent-native language. Programs are composed of **cells** -- self-contained units with contracts (faces), state (memory), behavior (signal handlers), and composition (interior cells). The compiler binary is `soma`.

## File Format

- Extension: `.cell`
- Encoding: UTF-8
- Comments: `//` line comments

## Cell Definition

```
cell <Name> {
    face { ... }       // optional: contract/interface
    memory { ... }     // optional: state slots
    on <signal>(...) { ... }  // signal handlers
    interior { ... }   // optional: nested cells
    runtime { ... }    // optional: orchestration
    state <name> { ... } // optional: state machine
}
```

### Cell Kinds

| Syntax | Kind | Purpose |
|--------|------|---------|
| `cell Name { }` | Cell | Regular cell |
| `cell property Name { }` | Property | Define memory property (e.g. persistent) |
| `cell type Name<T> { }` | Type | Define custom type |
| `cell checker Name { }` | Checker | Custom validation rule |
| `cell backend Name { }` | Backend | Storage backend definition |
| `cell builtin Name { }` | Builtin | Native function bridge |
| `cell test Name { }` | Test | Test cell with assertions |

## Face Section (Contract)

```
face {
    given param: Type [where constraint]
    signal handler_name(param: Type, ...) -> ReturnType
    await external_signal(param: Type, ...) -> ReturnType
    promise "descriptive guarantee"
    promise constraint_expression
}
```

## Memory Section (State)

```
memory {
    name: Type [property1, property2, parameterized_prop(value)]
}
```

### Types

| Type | Description |
|------|-------------|
| `Int` | 64-bit integer |
| `BigInt` | Arbitrary precision integer |
| `Float` | 64-bit float |
| `String` | UTF-8 string |
| `Bool` | Boolean |
| `Map<K, V>` | Key-value map |
| `List<T>` | Ordered list |

### Built-in Memory Properties

Properties are extensible via `cell property` definitions. Common ones from stdlib:

- `persistent` -- data survives restarts (backed by SQLite)
- `consistent` -- strong consistency guarantees
- `ephemeral` -- in-memory only
- `encrypted` -- data at rest encryption
- `capacity(n)` -- max entries
- `ttl(duration)` -- time-to-live
- `partitioned(key, n)` -- partitioning

## Signal Handlers

```
on signal_name(param1: Type, param2: Type) {
    // statements
}
```

## Statements

| Statement | Syntax |
|-----------|--------|
| Let binding | `let x = expr` |
| Assignment | `x = expr` |
| Return | `return expr` |
| If/Else | `if cond { ... } else { ... }` (also works as expression) |
| For loop | `for item in collection { ... }` |
| While loop | `while cond { ... }` |
| Emit signal | `emit signal_name(args)` |
| Method call | `target.method(args)` |
| Expression | `fn_call(args)` |
| Require | `require constraint else signal` |

## Expressions

| Expression | Syntax |
|------------|--------|
| Literal | `42`, `3.14`, `"string"`, `true`, `false`, `()` |
| Identifier | `name` |
| Field access | `obj.field` |
| Method call | `obj.method(args)` |
| Function call | `fn(args)` |
| Binary ops | `+`, `-`, `*`, `/`, `%` |
| Comparison | `==`, `!=`, `<`, `>`, `<=`, `>=` |
| Logical | `&&`, `||`, `!` |
| String interp | `"hello {name}"` |
| List literal | `[1, 2, 3]` |
| Map literal | `map("key1", val1, "key2", val2)` |

## Built-in Functions

### Core

| Function | Signature | Description |
|----------|-----------|-------------|
| `print(args...)` | `(...) -> Unit` | Print to stdout |
| `concat(a, b)` | `(String, String) -> String` | String concatenation |
| `to_string(x)` | `(Any) -> String` | Convert to string |
| `to_int(x)` | `(String) -> Int` | Parse integer |
| `to_float(x)` | `(String) -> Float` | Parse float |
| `to_json(x)` | `(Any) -> String` | Serialize to JSON |
| `from_json(s)` | `(String) -> Any` | Parse JSON |
| `type_of(x)` | `(Any) -> String` | Runtime type name |
| `abs(n)` | `(Num) -> Num` | Absolute value |
| `len(x)` | `(Collection) -> Int` | Length/size |
| `next_id()` | `() -> Int` | Auto-incrementing ID |

### String Functions

| Function | Description |
|----------|-------------|
| `split(s, delim)` | Split string by delimiter |
| `replace(s, old, new)` | Replace substring |
| `starts_with(s, prefix)` | Check prefix |
| `trim(s)` | Trim whitespace |
| `lowercase(s)` | Convert to lowercase |
| `uppercase(s)` | Convert to uppercase  |
| `contains(s, sub)` | Check substring |
| `index_of(s, sub)` | Find substring position |
| `substring(s, start, end)` | Extract substring |
| `join(list, sep)` | Join list into string |

### List Functions

| Function | Description |
|----------|-------------|
| `list(items...)` | Create list |
| `push(list, item)` | Append item |
| `flatten(list)` | Flatten nested lists |
| `zip(a, b)` | Zip two lists |
| `enumerate(list)` | Add indices |

### Map Functions

| Function | Description |
|----------|-------------|
| `map(k1, v1, k2, v2, ...)` | Create map from pairs |
| `with(map, key, value)` | Add/update key |
| `without(map, key)` | Remove key |
| `merge(map1, map2)` | Merge maps |

### Collection Query Functions (SQL-like)

| Function | Description |
|----------|-------------|
| `filter_by(list, key, value)` | Filter entries |
| `sort_by(list, key)` | Sort by field |
| `top(list, n)` | First N items |
| `sum_by(list, key)` | Sum field values |
| `avg_by(list, key)` | Average field values |
| `min_by(list, key)` / `max_by(list, key)` | Min/max by field |
| `pluck(list, key)` | Extract field values |
| `count_by(list, key, value)` | Count matching |
| `group_by(list, key)` | Group by field |
| `distinct(list, key)` | Unique values |
| `inner_join(a, b, key)` | Inner join |
| `left_join(a, b, key)` | Left join |

### Template Functions

| Function | Description |
|----------|-------------|
| `render(template, k1, v1, ...)` | Replace `{key}` with values |
| `render_each(template, list)` | Render template for each item |

### HTTP/Web Functions

| Function | Description |
|----------|-------------|
| `response(status, body)` | Create HTTP response with status |
| `html(body)` / `html(status, body)` | HTML response (auto-injects HTMX if needed) |
| `redirect(url)` / `redirect(status, url)` | HTTP redirect |

### Memory Slot Methods

| Method | Description |
|--------|-------------|
| `slot.get(key)` | Get value by key |
| `slot.set(key, value)` | Set key-value pair |
| `slot.delete(key)` | Delete by key |
| `slot.len()` | Count entries |
| `slot.keys()` | List all keys |
| `slot.values()` | List all values |
| `slot.entries()` / `slot.all()` | List all key-value pairs |
| `slot.has(key)` / `slot.contains(key)` | Check existence |
| `slot.push(value)` | Append to list storage |

## State Machines

```
state machine_name {
    initial = state_name

    state_a -> state_b when guard_expr {
        // effect statements
    }
    * -> error_state when error_condition { ... }
}
```

## Interior (Composition)

```
interior {
    cell Child1 { ... }
    cell Child2 { ... }
}
```

## Runtime Section

```
runtime {
    start ChildCell
    connect Producer.data -> Consumer
    emit initialize()
}
```

## Imports

```
use other_file          // relative file import
use lib::helpers        // lib/ directory import
use std::builtins       // stdlib import
use pkg::math           // package import
```

## Test Cells

```
cell test MyTests {
    assert handler(args) == expected
    assert condition_expression
}
```

Run tests: `soma test file.cell`

## CLI Commands

| Command | Description |
|---------|-------------|
| `soma run file.cell [args...]` | Execute signal handler |
| `soma run file.cell --signal name [args...]` | Execute specific handler |
| `soma run file.cell --jit [args...]` | Use bytecode VM |
| `soma serve file.cell [-p port]` | HTTP server |
| `soma serve file.cell --verbose` | Verbose HTTP logging |
| `soma serve file.cell --watch` | Auto-reload on changes |
| `soma check file.cell` | Type/property checking |
| `soma fix file.cell` | Auto-repair common errors (missing handlers, bad properties) |
| `soma build file.cell [-o out.rs]` | Generate Rust skeleton |
| `soma test file.cell` | Run test cells |
| `soma init [name]` | Initialize project (creates subdirectory if name given) |
| `soma add package [--git url] [--path dir]` | Add dependency |
| `soma install` | Install dependencies |
| `soma repl` | Interactive REPL |
| `soma ast file.cell` | Dump AST |
| `soma tokens file.cell` | Dump tokens |
| `soma env` | Show environment info |
| `soma props` | List registered properties |

## HTTP Server Routing

When using `soma serve`, routes map to signal handlers:

| URL Pattern | Handler Called |
|-------------|---------------|
| `POST /signal/handler?k=v` | `handler(v)` |
| `GET /handler/arg1/arg2` | `handler(arg1, arg2)` |
| `GET /handler?k=v` | `handler(v)` |
| `POST /handler` with JSON body | `handler(field1, field2, ...)` -- fields extracted by param names |
| `GET /any/path` | `request("GET", "/any/path", "", {query_params})` |
| `GET /static/path` | Serves static file from `static/` directory |

## Project Structure

```
myapp/
  soma.toml          # project manifest
  main.cell          # entry point
  lib/               # local library cells
  static/            # static assets (CSS, JS, images)
  .soma_env/         # isolated environment
    stdlib/          # property definitions
    packages/        # installed dependencies
    cache/           # compiled bytecode cache
  .soma_data/        # runtime data
    soma.db          # SQLite database for persistent memory
```

## soma.toml

```toml
[package]
name = "myapp"
version = "0.1.0"
description = ""
author = ""
entry = "main.cell"

[dependencies]
# package_name = "version"
# package_name = { git = "url", version = "branch" }
# package_name = { path = "../local/path" }
```
