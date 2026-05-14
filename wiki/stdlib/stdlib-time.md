---
name: stdlib-time
description: Time builtins — `now`, `now_ms`, `today`, `format_date`.
type: reference
since: V1.0
related: [handler]
---

# Stdlib: time

```soma
let ts = now()                  // Int — unix timestamp (seconds)
let ms = now_ms()               // Int — unix timestamp (milliseconds)
let day = today()               // String — "2026-05-14"
let formatted = format_date(ts) // String — "2026-05-14"
```

## Duration literals

In source, durations are first-class:

```soma
every 30s    { ... }            // every 30 seconds
after 5min   { ... }            // one-shot 5 minutes from now
every 1h     { ... }            // every hour
let timeout = 500ms              // → 500 (Int milliseconds)
```

Recognized units: `ms`, `s`, `min`, `h`, `d`, `y`. All convert to
milliseconds internally.

## Nondeterminism

`now`, `now_ms`, `today`, `format_date(now())`, and the duration-based
scheduler are **nondeterministic**. Inside a `[record]` handler, the
runtime logs every call so `soma replay` can reproduce deterministically.

If a `[record]` handler calls `now()` and the replayed log contains a
different timestamp, the replay reports the divergence with the call
site.

## Examples

A periodic check:

```soma
every 60s {
    let ts = now()
    let snapshot = compute_metrics()
    log.set(to_string(ts), to_json(snapshot))
}
```

A timeout pattern:

```soma
on with_timeout(work: String) {
    let start = now_ms()
    let r = try { expensive_call(work) }
    if now_ms() - start > 5000 {
        return map("error", "exceeded 5s budget", "value", r.value)
    }
    r.value
}
```

## Edge cases

- `now()` resolution depends on the OS. Typically microseconds; not
  guaranteed monotonic.
- `now_ms()` is `now() × 1000` plus subsec_millis; same monotonicity
  caveat.
- `today()` uses the local timezone. For UTC, format `now()` directly.
- Duration literals in source are parsed to Int milliseconds at
  compile time — `5s` and `5000` are not the same value? Actually
  they are: `5s` parses to `5000`. The literal carries the unit only
  for readability.

## Related

- [[handler]] — `every Ns { }` and `after Ns { }` wrap handlers.
