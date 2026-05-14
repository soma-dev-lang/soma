---
name: duration-literal
description: First-class duration syntax — `5s`, `1min`, `500ms`, `1h`.
type: feature
since: V1.0
related: [handler, stdlib-time]
---

# Duration literals

Soma has first-class duration syntax. Numbers followed by a unit
suffix parse to integer milliseconds at compile time.

## Units

- `ms` — milliseconds
- `s` — seconds
- `min` — minutes
- `h` — hours
- `d` — days
- `y` — years

## Use sites

```soma
every 30s { ... }              // schedule: every 30 seconds
after 5min { ... }             // one-shot: 5 minutes from now
let timeout = 500ms            // → 500 (Int milliseconds)
let day = 1d                   // → 86_400_000

http_get(url, map("timeout", 5s))      // 5s = 5000ms
think("...", map("timeout", 60s))
```

The compiler parses the unit and emits the equivalent milliseconds.
Mixed-unit arithmetic works:

```soma
let total = 1min + 30s        // 90_000 ms
```

## Examples

A periodic task:

```soma
cell Watcher {
    every 60s {
        let snapshot = check_health()
        events.set(to_string(now()), to_json(snapshot))
    }
}
```

A one-shot delay:

```soma
on start(id: String) {
    transition(id, "running")
    after 5min {
        if get_status(id) == "running" {
            transition(id, "timed_out")
        }
    }
}
```

A bounded LLM call:

```soma
let r = think("Long task", map(
    "max_tokens", 5000,
    "timeout", 2min        // 120000ms
))
```

## Edge cases

- Duration literals always evaluate to `Int` (milliseconds). They
  don't carry the unit at runtime — `5s` and `5000` are the same value.
- Fractional durations like `1.5s` are parsed but truncated to
  integer milliseconds.
- Mixing durations with non-duration ints in arithmetic works (it's
  just Int math), but the result loses any semantic unit hint.

## Related

- [[stdlib-time]] — `now()`, `now_ms()`, `today()`.
- [[handler]] — `every Ns { }` / `after Ns { }` wrappers.
