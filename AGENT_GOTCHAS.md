# Soma for agents: verified wrong → right

The mistakes an LLM makes writing Soma, each with the **actual compiler
error** and the fix. Every pair below is verified against the current
`soma` binary. This is the self-correction corpus: when `soma check` /
`soma run` emits one of these errors, apply the paired fix.

A rule of thumb: **`soma check` catches most of these before you run.**
Write the file, run `soma check app.cell`, fix what it reports, repeat.

---

## 1. Nested string literals inside `{...}` interpolation

```soma
// WRONG — a string literal inside an interpolation segment
return "len: {len(\"hi\")}"
// error: string interpolation cannot evaluate a nested string literal
//        in '{len("hi")}' — bind the value with a let first
```
```soma
// RIGHT — bind it, then interpolate the variable
let n = len("hi")
return "len: {n}"
```

## 2. `match` arms use `->`, not `=>`

```soma
return match x { 1 => "a"  * => "b" }
// error: match arms use '->', not '=>'
```
```soma
return match x { 1 -> "a"  * -> "b" }
```
`=>` is **lambda** syntax (`p => p + 1`). `->` is match arms and signal
return types. Don't cross them.

## 3. Handlers don't declare return types

```soma
on add(a: Int, b: Int) -> Int { return a + b }
// error: handlers do not declare return types — put '-> Int' on the
//        signal declaration inside face { }
```
```soma
face { signal add(a: Int, b: Int) -> Int }
on add(a: Int, b: Int) { return a + b }
```

## 4. Adjacent string literals do NOT concatenate

```soma
return "hello " "world"
// error: in G.hello: a string literal follows `return` and is never
//        evaluated — adjacent string literals do not concatenate
```
```soma
return "hello world"        // one literal
let name = "world"
return "hello {name}"       // or interpolate
```
(`soma check` also warns on any other unreachable statement after
`return` / `break` / `continue`.)

## 5. `==` IS structural on lists, maps and variants

```soma
[1, 2] == [1, 2]                                  // true
map("US", 1, "EU", 2) == map("EU", 2, "US", 1)    // true — key order is irrelevant
[] == []                                          // true
```
`<`, `>` on lists or maps is an error (compare a field or `len()`), and values
of different kinds are an error to compare (`1 == "1"`), not `false`.

## 6. Float equality needs a tolerance

```soma
return 0.1 + 0.2 == 0.3      // false — floating point
```
```soma
return abs((0.1 + 0.2) - 0.3) < 0.0001
```

## 7. `transition()` returns a map, not the target string

```soma
on advance(id: String) {
    return transition(id, "next")   // returns {id, from, to}, not "next"
}
```
```soma
on advance(id: String) {
    transition(id, "next")
    return get_status(id)           // the new state as a string
}
```
Guard fallible transitions with `try`:
```soma
let r = try { transition(id, "next") }
if r.error != () { return map("error", r.error) }
```

## 8. `is_a` does not recognize sum-type VARIANTS — match them

```soma
let b = Box { w: 3 }         // Box is a `variants` constructor
return is_a(b, "Box")        // false — variants aren't tagged records
```
```soma
// extract the kind with an exhaustive match handler
on kind(s: Map) {
    return match s {
        Box { w } -> "Box"
        // ... every variant
    }
}
```
(`is_a` / `is_type` DO work on record literals: `is_a(Game { x: 1 }, "Game")` is `true`.)

## 9. There is no `cell type X { fields { ... } }`

Records are plain map-shaped values. Construct them with a literal:
```soma
let g = Game { bet: 10, pot: 0 }   // a field-accessible value (a Map)
g.bet = 20                          // mutate fields in place
let b = g.bet
return is_a(g, "Game")              // true — record literals carry _type
```
Use `cell type X { variants { ... } }` only for *sum types* (tagged unions).

## 10. `soma serve` routes only the cell that owns `request`

```soma
// other cells' signals are NOT auto-routed as HTTP endpoints
```
```soma
// put every routable signal on the request-owning cell, delegating
// to domain cells:
cell Api {
    face { signal request(...) -> String  signal place(...) -> Map }
    on place(...) { return place_order(...) }   // delegate to Orders
    on request(method, path, body) { ... }
}
```

---

## These USED to be limitations and now WORK — use them freely

Older Soma code worked around these; the current language supports them
directly. Prefer the direct form.

```soma
// bracket indexing (read + write) on lists, maps, strings
let x = xs[2]      xs[2] = 99      let v = m["key"]   m["key"] = 1   let c = s[0]
// negative literals
let n = -1                         // not `0 - 1`
// descending / stepped ranges
for r in range(10, 0, -1) { }      // not build-then-reverse
// numeric reductions over a list
sum(xs)   product(xs)   avg(xs)   min(xs)   max(xs)
// UFCS — any builtin is a method
xs.sum()   xs.sort()   xs.reverse()   m.det()   m.transpose()
// nested record/list mutation
g.board[0] = 99    g.meta.turn = 5    xs[i][j] = v
// matrices are first-class, with vectorized (numpy-style) operators
let M = [1,0,0,1].reshape(2,2)     let P = A * B      let t = M.T      det(M)
A + 10    A / 2    1 - A           // scalar broadcast on matrices
v * 2     v - 1    v * v   v + v   // vector broadcast + elementwise (+ * / -)
A > 2     v >= 2.0                 // comparison masks (0/1)
// list CONCATENATION is concat(a, b); non-numeric lists keep + = concat
// `with` is functional copy-update for maps AND lists
let m2 = with(m, "k", 9)           let l2 = with(xs, 0, 9)
```

---

## The agent loop, in commands

```
soma check  app.cell        # contracts, interpolation, dispatch — fix these first
soma verify app.cell        # PROVE state machines + memory invariants
soma test   app.cell        # run `cell test` assertions (assert / assert_fails)
soma run    app.cell sig a  # execute a handler
soma serve  app.cell -p 8080
soma describe app.cell --faces      # token-cheap contract summary of every cell
soma describe --builtins --json     # the exact builtin signatures (never guess)
```

When unsure of a builtin's signature, run `soma describe --builtins` —
do not guess. When unsure of a cell's API, run `soma describe --faces`.

---

## More verified footguns (found generating 168 programs)

## 11. Your handler shadows a builtin of the same name — when the argument count matches

`f(args)` calls the program's handler `f` if one takes that many arguments,
the builtin `f` otherwise. User code shadows the library, as everywhere else.

```soma
on merge(a: Int, b: Int) { return a + b + 1000 }
on use_it()  { return merge(1, 2) }      // 1003 — your handler
on list()    { return list(1, 2) }       // [1, 2] — 2 ≠ 0 arguments: the builtin
```
`soma check` warns on the two confusing cases:
```soma
on merge(a: Int, b: Int, c: Int) { … }
on use_it() { return merge(m1, m2) }
// warning: call to 'merge' with 2 argument(s) … resolves to the BUILTIN merge():
//          the handler G.merge takes [3]
on list() { let items = list() … }
// warning: inside G.list, `list(…)` with 0 argument(s) calls the handler ITSELF
//          (recursion), not the builtin list(). For an empty list write []
```
Method calls (`xs.count(p)`) always go to builtins.

## 12. `assert_fails` needs an expression that RAISES, not a falsy bool

```soma
assert_fails 1 == 2          // FAILS the test: 1==2 is just `false`, no error
```
```soma
assert_fails xs[99]          // passes: out-of-bounds RAISES
assert_fails transition(id, "illegal")   // passes: invalid transition raises
assert !(1 == 2)             // for a falsy predicate, use plain assert + !
```

## 13. Slot methods work on declared `memory` slots, not local maps

```soma
let seen = map()
seen.set("k", 1)             // error: 'seen' is not a memory slot
```
```soma
let seen = map()
seen["k"] = 1                // local maps use bracket indexing
let v = seen["k"] ?? 0
```
`.get`/`.set`/`.has`/`.delete`/`.keys` are for `memory { slot: ... }` slots.

## 14. `on` is a reserved keyword

It can't be a parameter name or a map field read as `.on`. Use `enabled`,
`active`, etc.

## 15. No semicolons; statements are newline-separated

```soma
{ a = 1; b = 2 }             // lex error
```
```soma
{
    a = 1
    b = 2
}
```

## 16. `given` is reserved (like `on`)

It's a face-declaration keyword — can't be a state name, param, or
identifier. `error: expected name, found Given`. Use `granted`, `input`, etc.

## 17. What a test cell's `rules { }` accepts

`assert`, `assert_fails` (optionally `… matching "text"`), `let name = expr`
(a fixture for the rules below), `mock think "reply"` / `mock think ["a", "b"]`
/ `mock think error "timeout"`, and `property`. No bare statements: put logic
in a handler and call it.

## 18. One invariant, one slot

An invariant is checked per write, with only the written slot in scope.
```soma
memory { a: Map<String, Int>  b: Map<String, Int>  invariant a + b <= 100 }
// error: memory invariant references several slots (a, b) — ... Write
//        one invariant per slot
```
`size` invariants are enforced on `delete` too: `invariant size >= 1`
rejects removing the last entry.

## 19. `7 / 2 = 3.5` everywhere — say `idiv` when you mean the integer quotient

`/` on two Ints is 3.5 (an Int only when exact) in the interpreter AND in
`[native]` handlers. Native code is statically typed, so where the quotient
must be an Int it is checked instead of truncated:

```soma
on mid(lo: Int, hi: Int) [native] {
    let m = lo
    m = (lo + hi) / 2        // m is an Int slot
    return m
}
// mid(1, 2) → error: Int / Int is not exact here, and this spot can only
//             hold an Int (7 / 2 is 3.5) — write idiv(a, b) ...
```
```soma
on mid(lo: Int, hi: Int) [native] { return idiv(lo + hi, 2) }   // 1, everywhere
```
`soma check` warns on Int / Int in native handlers; `soma fix f.cell --native-idiv`
rewrites them (for code written when native `/` truncated). A `[native]`
division by zero is an ordinary, `try`-catchable runtime error.

## 20. A cost bound is only *proven* when every `think()` count is known

`think()` reached through a loop over a list, a lambda (`map(xs, x => think(..))`)
or a recursive helper makes the bound **advisory**. Give the loop a literal
`range(0, N)` or `[loop_bound(N)]` to get `bound proven` back. Calls to sibling
handlers are composed: `for i in range(0, 3) { helper() }` costs 3 × helper.

## 21. Habits from Python / TypeScript that `soma check` now redirects

```soma
if x == null { }          // error: 'null' does not exist — Soma's null is `()`; also `x ?? default`
xs.includes(v)            // error: no method 'includes' — in Soma: contains(xs, v)
rows.push(o)              // warning: result discarded — push returns a NEW list: rows = push(rows, o)
[a[0]] + rest             // warning: `+` ADDS numeric lists element-wise — concat(a, b) / push(xs, x)
let t = lefft + 1         // error: undefined variable 'lefft' (did you mean 'left'?)
```
Everyday collection builtins: `contains(list|map|string, x)`, `slice(xs, start, end?)`
(negative indexes count from the end), `keys(m)` / `values(m)` / `entries(m)`,
`sort_by(rows, "field")` or `sort_by(rows, r => [0 - r.total, r.name], "desc"?)`
(stable; a list key sorts on several keys), `round(x, digits)`.

## 22. Transition guards see the calling handler's locals

```soma
state expense { initial: approved   approved -> paid { guard { amount < 10000 } } }
on pay(id: String) {
    let amount = amounts.get(id) ?? 0      // the guard reads THIS `amount`
    transition(id, "paid")                 // raises "guard failed…" when false
}
```
A guard sees: the locals of the handler calling `transition()`, the cell's
memory slots, `_id`, `_from`, `_to`. `soma check` rejects a guard that reads a
name its calling handler never binds. Guards are enforced at runtime; the
model checker keeps the edge (an over-approximation, so safety results hold).

## 23. `soma.toml` is validated

A `soma.toml` that does not parse — or has an unknown key under `[verify]` — is
an error for every command (it used to be ignored silently, so `[verify]`
properties were never checked). `[package]` is optional.

## 24. Handlers are atomic; errors have kinds

A handler that raises leaves nothing behind: its writes and transitions are
rolled back (a failing `try { }` block too, to where it began). Do not write
compensation code. Raise with `fail("kind", "detail")` or
`require cond else Tag`; catch with `let r = try { … }` and branch on `r.kind`
(`"not_found"`, `"invalid_transition"`, `"guard_failed"`, `"invariant"`, …);
`fail(r)` re-raises.

## 25. `* -> failed` leaves EVERY state, final ones included

```soma
state s { initial: a   a -> paid   * -> failed }               // paid -> failed exists: paid is not final
state s { initial: a   a -> paid   * -> failed except [paid] }  // paid stays final
```
`soma verify` warns about the first form and prints the second.

## 26. A route in `request` and a handler of the same name

`soma serve` exposes every public handler at `/<name>/<args>`. With
`on hold(id, qty)` and a route `"/hold/" + id`, the explicit route wins — but
other `/hold/…` shapes still reach the handler. Prefix internal handlers with
`_`. `soma check` warns.
