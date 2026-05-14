---
name: stdlib-strings
description: String builtins — length, search, transform, format.
type: reference
since: V1.0
related: [stdlib-collections]
---

# Stdlib: strings

All Soma strings are UTF-8. `len` returns character count (not byte
count).

## Length & inspection

- `len(s)` — character count.
- `contains(s, sub)` — Bool: does `s` contain `sub`?
- `starts_with(s, prefix)` / `ends_with(s, suffix)` — Bool.
- `index_of(s, sub)` — first byte index of `sub`, or `-1`.

## Transform

- `uppercase(s)` / `lowercase(s)`
- `trim(s)` — strip leading/trailing whitespace.
- `replace(s, old, new)` — replace all occurrences.
- `substring(s, from, to)` — half-open range `[from, to)`.

## Split & join

- `split(s, delim)` — returns `List<String>`.
- `join(lst, delim)` — returns `String`.

## Format & encode

- `to_string(v)` — convert any value to its String representation.
- `escape_html(s)` — HTML entity-encode.
- `to_json(v)` — serialize any value to JSON string. **Unbounded** —
  output size scales with input.
- `from_json(s)` — parse JSON to a Soma value. **Unbounded** — input
  size is uncontrolled.

## Interpolation

Soma strings support `{expr}` interpolation:

```soma
let name = "Alice"
let greeting = "Hello, {name}!"
let amount = 100
let msg = "Charged ${amount} on card {card_number}"
```

The expression inside `{ … }` can be any Soma expression — field
access, function call, arithmetic, even `match`.

## Multi-line strings

```soma
let prompt = """
Analyze this trade:
  symbol: {symbol}
  quantity: {qty}
  price: {price}
"""
```

Triple-quoted strings are raw — no escape sequences, embedded `"`
allowed.

## Examples

```soma
let s = "Hello, World"
len(s)                          // 12
uppercase(s)                    // "HELLO, WORLD"
substring(s, 7, 12)             // "World"
split("a,b,c,d", ",")           // ["a", "b", "c", "d"]
join(list("x", "y", "z"), "-")  // "x-y-z"
replace("hello", "l", "r")     // "herro"

let user = map("name", "Alice", "age", 30)
to_json(user)                   // "{\"name\":\"Alice\",\"age\":30}"
let parsed = from_json("{\"k\":42}")
parsed.k                        // 42
```

## Edge cases

- `len` returns character count, but `substring` indices and
  `index_of` are byte-based. For ASCII this is the same; for Unicode
  there's a mismatch — this is a documented inconsistency.
- `from_json` returns `()` on parse error. Always check.
- `to_json` of a `Value::Variant` produces a tagged Object with
  `_type` and `_variant` keys. See [[sum-types]] for the
  serialization convention.

## Related

- [[stdlib-collections]] — List/Map operations.
- [[stdlib-storage]] — `.set/.get` on Map slots typically uses
  `to_json/from_json` for complex values.
