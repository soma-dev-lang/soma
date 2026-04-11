---
name: soma
description: Author, verify, and serve Soma programs — the fractal cell language for verified distributed systems and AI agents. Covers syntax, cell model, memory properties, state machines + CTL verification, agent builtins (think/delegate/budget), pattern matching, pipes, scale/distribution, face contracts, HTTP serving, and the generate→fix→check→verify→serve workflow. Use whenever editing `.cell` files, writing `soma.toml`, running `soma` CLI or `mcp__soma__*` tools, or reasoning about Soma state machines.
---

# Soma — Agent Skill

This is the authoritative agent-facing guide to the Soma language, toolchain, and idioms. Read it in full before writing or modifying any `.cell` file. The guide is dense on purpose: Soma has a small core but a lot of *subtleties* — places where the obvious thing is wrong, or where a small annotation changes distribution semantics.

Source-of-truth documents in this repo, in decreasing order of formality:

- `SOMA_SPEC.md` — machine-readable spec
- `SOMA_REFERENCE.md` — language reference (heavy on examples)
- `AGENT.md` — the agent-workflow cheat sheet
- `spec/grammar.md` — EBNF + property algebra
- `PAPER.md` — the "Scale as a Type" thesis
- `VISION.md` — where the language is heading (don't rely on aspirational features)
- `stdlib/*.cell` — builtin declarations and property definitions
- `examples/*.cell` — idiomatic usage (83+ cells)

If this SKILL.md and those docs disagree, the spec/grammar files win. This file is a distillation, not a replacement.

---

## 1. The one-paragraph thesis

Soma is a declarative, fractal language whose single construct — the **cell** — simultaneously describes a function, a service, a database, and a cluster. A cell has five sections: `face` (contract), `memory` (state, with distribution/durability properties), `state` (lifecycle state machines), `scale` (distribution requirements), and `on` handlers (behavior). A built-in CTL model checker proves temporal properties (deadlock-freedom, liveness, safety) at compile time. The same source runs locally or as a cluster node with `--join`. Agent primitives (`think`, `delegate`, `set_budget`, `approve`) are builtins, not libraries, which lets the compiler *prove that an LLM-driven agent terminates*. That last bullet is Soma's unique selling point.

## 2. Mental model — everything is a cell

```
cell <Name> {                       // kind can be: (plain) | agent | property | type | checker | backend | builtin | test
    face     { ... }                // contract: signals, awaits, tools, promises, given/where
    memory   { ... }                // state slots with property annotations
    state <machine> { ... }         // one or more state machines
    scale    { ... }                // distribution declaration
    every 30s { ... }               // periodic task (one per interval allowed)
    after  5s { ... }               // one-shot delayed task
    on <signal>(...) { ... }        // signal handler
    interior { cell Child { ... } } // nested cells (composition)
    runtime  { start Child; connect Producer.data -> Consumer }
    rules    { native "name" }      // (builtin cells only) FFI bridge
}
```

Cell *kinds* are modifiers on the keyword:

| Keyword | Purpose |
|---|---|
| `cell Foo { }` | Regular cell |
| `cell agent Foo { }` | Agent cell — unlocks `think`, `set_budget`, `tool` declarations |
| `cell property Foo { }` | Define a new memory property (see `stdlib/durability.cell`) |
| `cell type Foo<T> { }` | Define a custom type |
| `cell checker Foo { }` | Custom validation rule the checker will run |
| `cell backend Foo { }` | Storage backend definition |
| `cell builtin Foo { }` | FFI bridge to Rust (stdlib only) |
| `cell test Foo { }` | Test cell — `rules { assert ... }` |

The *fractal* claim is: the same five sections describe a one-line function and a 50-replica cluster. Don't invent new abstractions — reach for another cell.

## 3. Workflow — the loop you MUST run

```
soma_generate(file, code)    → write source
soma fix file.cell           → auto-repair missing handlers, bad properties
soma_check(file)             → contract + property + signal checking
soma_verify(file)            → CTL model checking against soma.toml [verify] section
soma_serve(file, port)       → serve (HTTP :PORT, WS :PORT+1, bus :PORT+2)
```

Rules:

- **Always run `soma fix` before `soma_check`.** It auto-repairs many trivial issues agents generate (missing handlers referenced from `face`, contradictory property combos, misspelled builtins).
- **Always write a `soma.toml` alongside the `.cell` file** when there is a state machine. `soma verify` without a `[verify]` section is a no-op.
- If `verify` fails, read the `counter_example` field — it is an ordered list of states forming the bad trace. The diagnosis is almost always one of: unreachable terminal, liveness cycle, or a property that contradicts the machine shape.
- `soma check --json` returns `{kind, fix}` fields — use them; they tell you the shape of the repair.

MCP tools available (`mcp__soma__*`): `soma_generate`, `soma_generate_toml`, `soma_check`, `soma_verify`, `soma_run`, `soma_test`, `soma_serve`, `soma_stop`, `soma_describe`. Prefer these over CLI when already inside Claude Code — they return structured JSON.

## 4. Syntax — the gotchas that will trip you every time

Soma looks Rust-ish at a glance but diverges sharply. These are NOT Soma, even though other languages use them:

```text
function foo() { }              // WRONG — use `on foo() { }`
def foo(): ...                  // WRONG — same
null / nil / None               // WRONG — use `()` (unit)
[1, 2, 3]                       // WRONG — use `list(1, 2, 3)`
{key: val}                      // WRONG — use `map("key", val)`   (MUST have even number of args)
import x                        // WRONG — use `use lib::x` / `use std::x` / `use pkg::x`
console.log(x)                  // WRONG — use `print(x)`
x === y                         // WRONG — use `x == y`
foo(x);                         // WRONG — no semicolons, ever
class Foo { }                   // WRONG — no classes, no inheritance
async fn / await                // WRONG — Soma is synchronous; use `every Ns {}` / `after Ns {}`
```

These ARE Soma:

```soma
on handler(x: Int) { return x + 1 }
let nothing = ()
let xs = list(1, 2, 3)
let m = map("name", "Alice", "age", 30)    // MUST be even arg count
"hello {name}"                              // string interpolation with {braces}
"""raw
multiline
no escapes needed"""                        // triple-quoted raw string
data |> filter(x => x.active) |> top(10)
let y = if cond { a } else { b }            // if IS an expression (else is required)
```

### Subtleties that bite

- **Integer division auto-promotes to Float when non-exact.** `7 / 2 == 3.5`, not `3`. Use `floor(7 / 2)` if you want integer division. `Int` is 64-bit; `BigInt` exists for arbitrary precision. The native backend has a dual-mode dispatch that will use GMP (via `rug`) for whole BigInt loops; single BigInt ops go through FFI and are slower.
- **`map()` must have an even number of arguments.** `map("a", 1, "b")` is a compile error — the checker will flag it, but `soma fix` won't always auto-repair.
- **`if` is an expression, but only with an `else` branch.** `let x = if cond { a }` is invalid. `let x = if cond { a } else { b }` is valid.
- **`return` inside a `for` loop exits the whole handler.** Don't use `return` to break out of a loop — use a variable and `break`.
- **`match` is an expression. Don't write `return match ...`** — just write the match (its value is the handler's return value). Writing `return` after a match is a lint warning.
- **Multi-line pipes inside `match` arms don't work.** Put the pipe on one line, or wrap the arm body in `{ ... }` and assign to a `let`.
- **Don't wrap stored values in `to_json()` when the value is already a map or list.** The storage layer (`slot.set`) auto-serializes `Map`/`List` values. Wrapping them manually works but produces double-encoded strings on read. Use `to_json` only when the caller explicitly wants a string.
- **`slot.get(key)` returns `()` for missing keys**, not an error. Always check `if raw == ()` before calling `from_json(raw)`.
- **`.keys`, `.values`, `.len` on a memory slot have NO parentheses.** They are properties, not methods. `items.keys()` is wrong; `items.keys` is right. `slot.get(k)` / `slot.set(k, v)` / `slot.delete(k)` / `slot.has(k)` DO take parentheses.
- **Private handlers start with `_`.** `on _helper()` is not exposed as an HTTP route. Use this to keep internal functions off the wire when serving.
- **`unique()` does not exist. Use `distinct()`.** Same for other "obvious name" mistakes: use `push` not `append_to`, `nth` not `at`, `len` not `length`.
- **`data.field ?? "default"` is null-coalescing** — the idiomatic guard against missing map fields.

## 5. Types

| Type | Literal | Notes |
|---|---|---|
| `Int` | `42`, `-1`, `0` | 64-bit |
| `BigInt` | (promoted from `Int` ops or literals that overflow) | arbitrary precision |
| `Float` | `3.14`, `1.5e3` | 64-bit, scientific notation |
| `String` | `"hi {name}"`, `"""raw"""` | interpolation with `{}`; triple-quote is raw |
| `Bool` | `true`, `false` | |
| `List<T>` | `list(1, 2, 3)` | ordered |
| `Map<K, V>` | `map("k", v, "k2", v2)` | MUST have even args |
| `Unit` | `()` | null equivalent |
| `Duration` | `5s`, `1min`, `500ms`, `1h`, `1d`, `2years` | converts to ms internally |
| `Record` | `User { name: "Alice", age: 30 }` | typed map with `_type` field |
| `Option` / `Result` | stdlib generic types | rarely needed — `()` and `try { }` cover most cases |

Type names are uppercase; value identifiers lowercase. `type_of(x)` returns a runtime string name.

**Conversion:** `to_int("abc")` returns `()`, NOT `0`. This is the single most important type-conversion rule — always null-check the result of `to_int` / `to_float` on user input.

## 6. Memory — the property algebra

Memory slots are the heart of Soma. Properties on memory are **distribution types** — the compiler checks their combination for contradictions.

```soma
memory {
    items:  Map<String, String>  [persistent, consistent]       // durable, linearizable → SQLite
    cache:  Map<String, String>  [ephemeral, local]             // per-node, in-memory
    log:    List<String>         [persistent, capacity(10000), ttl(7d), evict(lru)]
    users:  Map<String, String>  [persistent, consistent, encrypted]
    votes:  Map<String, Int>     [persistent, eventual, replicated(3)]
    audit:  List<String>         [persistent, immutable, retain(5years)]
    invariant items.len >= 0                                    // checked on every .set()
}
```

### 6.1 Axes

| Axis | Values | Default |
|---|---|---|
| Durability | `persistent` ⊕ `ephemeral` | `ephemeral` |
| Consistency | `consistent` ⊕ `eventual` ⊕ `local` | `local` |
| Mutability | `immutable` or `versioned` or (neither = mutable) | mutable |
| Redundancy | `replicated(n)` | none |
| Lifecycle | `ttl(dur)`, `retain(dur)`, `capacity(n)`, `evict(lru|lfu|fifo|random)` | none |
| Security | `encrypted` | none |

⊕ = mutually exclusive. Violating exclusivity is a compile error.

### 6.2 Contradictions the compiler rejects

| Combo | Why |
|---|---|
| `persistent` + `ephemeral` | mutually exclusive durability |
| `consistent` + `eventual` | mutually exclusive consistency |
| `consistent` + `local` | linearizability requires non-local |
| `eventual` + `local` | both are non-strong — ambiguous |
| `immutable` + `evict(*)` | can't evict from append-only store |
| `ttl(X)` + `retain(Y)` where X < Y | retention would be violated |
| `ephemeral` + `retain(*)` | can't guarantee retention without persistence |
| `ephemeral` + `replicated(n>1)` | replication implies durability |
| `[ephemeral]` memory named in `scale { shard: ... }` | sharding requires durability |
| `[ephemeral]` slot + `scale { consistency: strong }` | contradicts — ephemeral has no cross-node story |

### 6.3 Implications the compiler applies

| If you wrote | Compiler assumes |
|---|---|
| `immutable` | `consistent` (no mutation ⇒ no staleness) |
| `replicated(n>1)` | `persistent` |
| `retain(*)` | `persistent` |

### 6.4 Slot API (methods vs properties)

Properties (NO parentheses): `slot.keys`, `slot.values`, `slot.len`, `slot.entries`, `slot.all`.

Methods (WITH parentheses): `slot.get(k)`, `slot.set(k, v)`, `slot.delete(k)`, `slot.has(k)`, `slot.contains(k)`, `slot.push(v)` (for list-shaped slots).

Storage auto-serialization: if you `slot.set(k, some_map_or_list)` the runtime serializes to JSON under the hood and you get the structured value back on `slot.get(k)`. You can still manually `to_json` / `from_json` when you explicitly want strings on the wire — but inside handlers, prefer direct storage of maps/lists.

### 6.5 Custom properties

You can define new memory properties:

```soma
cell property rate_limited {
    face { promise "at most N writes per second" }
    params { n: Int }
    rules {
        on write { require writes_per_sec <= n }
    }
}
```

Then use it: `counter: Int [rate_limited(100)]`. See `stdlib/lifecycle.cell` and `stdlib/durability.cell` for the stdlib versions.

## 7. Signal handlers (the `on` block)

Handlers are the only place behavior lives. A handler's return value is the result of the last expression OR an explicit `return`.

```soma
on add(a: Int, b: Int) -> Int {
    return a + b
}

on _internal(x: Int) {          // underscore prefix → not exposed as HTTP
    x * 2
}
```

### Statements

| Form | Example |
|---|---|
| Let | `let x = expr` |
| Reassign | `x = expr`, `x += 1`, `x -= 1`, `x *= 2`, `x /= 2` |
| Return | `return expr` |
| If | `if cond { ... } else if { ... } else { ... }` (also expression with `else`) |
| While | `while cond { ... }` with `break` / `continue` |
| For | `for item in list(1,2,3) { ... }` or `for i in range(0, 10)` |
| Match | `match expr { pat -> result, ... }` (expression) |
| Emit | `emit signal_name(args)` or `signal name(args)` |
| Require | `require cond else error_signal` — precondition |
| Ensure | `ensure cond` — postcondition, throws on failure |
| Try | `let r = try { risky() }` → `r.value` or `r.error` |
| `?` propagate | `let v = try { risky() }?` — early-return on error |
| Transition | `transition("instance_id", "new_state")` |

### Timers

```soma
every 30s { check_stale() }       // periodic, runs on leader in cluster mode
after 5s { init_once() }          // one-shot delay
```

Only one `every` block per interval per cell. `every` runs on the leader node in a cluster; `after` runs on whichever node the cell started on.

## 8. Pattern matching — deep

`match` is an expression with full pattern support:

```soma
match value {
    "a"                              -> expr1             // literal
    42                               -> expr2
    ()                               -> "null"            // match unit
    "x" || "y"                       -> "one of"          // or-pattern
    200..299                         -> "success"         // range
    n if n >= 90                     -> "A"               // guard
    name                             -> use(name)          // bind to variable
    "/api/" + rest                   -> api(rest)         // string prefix
    {method: "GET", path: "/"}       -> home()            // map destructure
    {method: "POST", path: "/api/" + r} -> create(r)      // nested
    _                                -> default           // wildcard (always last)
}
```

Rules and gotchas:

- Arms separated by newlines (no commas needed, commas tolerated).
- Multi-statement arm: wrap in braces `{ stmt; stmt; expr }`.
- Guards (`if ...`) bind to the pattern variable, not outer scope.
- Map destructuring is partial — unmentioned keys are ignored.
- Range patterns are inclusive on both ends: `200..299` includes 200 and 299.
- String-prefix `"/api/" + rest` captures `rest` as everything after the prefix.
- Multi-line pipes as arm body do NOT work — put the pipe on one line or into a `let` before the match.

## 9. Expressions, pipes, collections

Pipe `|>` is the backbone of data work. It's left-to-right function application: `x |> f(y)` is `f(x, y)`.

```soma
// Higher-order with lambdas
data |> map(s => s.name)
data |> filter(s => s.score > 50)
data |> find(s => s.id == target)        // returns first match or ()
data |> any(s => s.active)
data |> all(s => s.valid)
data |> count(s => s.score > 80)
data |> reduce(0, p => p.acc + p.val)    // lambda gets {acc, val}

// Block lambda — for multi-statement transforms
data |> map(s => {
    let score = s.x * 2 + s.y
    s |> with("score", score)
})

// Field-based (no lambda needed)
data |> filter_by("price", ">", 100)      // ops: > >= < <= == !=
data |> sort_by("score", "desc")           // "asc" default
data |> top(10)
data |> bottom(5)
data |> group_by("dept")
data |> distinct("category")
// Joins
data |> inner_join(other, "key")
data |> left_join(other, "key")

// Utility
data |> flatten()
data |> reverse()
data |> zip(other)
list("a", "b", "c") |> join(", ")         // "a, b, c"
```

`reduce`'s lambda parameter is a pair record: `p => p.acc + p.val` — do NOT write `(acc, val) => ...`.

## 10. State machines — Soma's reason to exist

State machines are the *object of verification*. Design them assuming every state will be exhaustively explored by the model checker.

```soma
state order {
    initial: pending
    pending   -> validated { guard { amount > 0 } }   // optional guard
    validated -> sent
    sent      -> filled
    sent      -> rejected
    filled    -> settled
    failed    -> pending                              // retry — CAREFUL: cycle
    *         -> cancelled                             // from any state
}
```

Rules:

- One `initial:` line per machine.
- Multiple `state <name> { }` blocks per cell are allowed.
- `*` as source = "from any state" (catch-all).
- Guards are boolean expressions over handler-scope variables — only evaluated when a `transition()` call would enter that arrow.
- Transitions happen via `transition("instance_id", "target")` inside a handler. `get_status("id")` returns the current state. `valid_transitions("id")` returns the list of reachable next states.
- Reaching an undeclared state = runtime error; `try { }` catches it.
- *Any cycle that doesn't reach a terminal state breaks liveness.* This is the most common verify failure — see §12.

Multiple machines are independent unless you declare cross-machine constraints in `[verify]`.

## 11. `scale` — distribution as types

```soma
scale {
    replicas: 10              // total instances
    shard: items              // MUST reference a declared memory slot
    consistency: eventual     // strong | causal | eventual
    tolerance: 2              // survives N simultaneous node failures
    cpu: 2                    // per instance
    memory: "1Gi"             // per instance
}
```

Compiler checks:

- `shard` exists as a memory slot in the same cell.
- The sharded slot is NOT `[ephemeral]` / `[local]`.
- Only one slot may be sharded.
- `consistency: strong` + `tolerance > 0` → CP mode (warns about availability under partition).
- Quorum for strong: `⌈(N+1)/2⌉`; max tolerable failures = `N - quorum`. The compiler prints this and rejects impossible `tolerance`.
- `[persistent, consistent]` + `consistency: eventual` → warning (declared strong storage but running eventual).

Runtime behavior: `soma serve` with no `--join` runs standalone. `soma serve --join host:port` joins a cluster over the signal bus. Memory writes become `_cluster_set` broadcasts on the bus; the same mechanism replicates state and carries inter-cell signals. Consistent hashing assigns keys to nodes (FNV, 128 vnodes/node).

Current runtime limitations (from `PAPER.md`): full replication (not true sharding), eventual replication even under `strong` (no Raft/Paxos yet), 15s heartbeat timeout. Treat `strong` as a contract you declared, not a guarantee the runtime enforces cryptographically. Verification catches CAP contradictions; the runtime is best-effort.

## 12. Verification — reading and fixing `soma verify`

`soma.toml` drives verification:

```toml
[verify]
deadlock_free = true
eventually   = ["delivered", "cancelled"]
never        = ["invalid"]

[verify.after.confirmed]
eventually = ["delivered", "cancelled"]
never      = ["pending"]

[verify.after.shipped]
eventually = ["delivered", "failed"]
```

CTL properties supported:

- `deadlock_free` — every reachable state has an outgoing transition (or is explicitly terminal).
- `eventually = [s1, s2, ...]` — on every execution path, eventually one of `s1..sn` is reached.
- `never = [...]` — no reachable state is in the list.
- `[verify.after.<state>]` — conditional: after first entering `<state>`, the sub-properties hold.

Reading a failure:

```json
{
  "passed": false,
  "temporal": [{
    "property": "eventually(delivered)",
    "passed": false,
    "counter_example": ["pending", "shipped", "failed", "pending", "shipped", "...cycle"]
  }]
}
```

**Interpretation:** the trace found a cycle that never reaches `delivered`. Fix *one of*:

1. **Break the cycle** — remove the arrow that closes the loop (e.g. drop `failed -> pending`).
2. **Add a terminal escape** — introduce `failed_permanent` and a bounded retry (requires runtime counter; usually a memory slot).
3. **Weaken the property** — change `eventually = ["delivered"]` to `eventually = ["delivered", "cancelled", "expired"]` so at least one terminal is always reachable.
4. **Add a catch-all** — `* -> expired` makes `expired` reachable from any state; combine with `eventually = [..., "expired"]`.

Common failure classes:

| Symptom | Root cause | Typical fix |
|---|---|---|
| `deadlock_free` fails | A state has no outgoing transitions and isn't in `eventually` | Add a transition OR mark it terminal by listing it in `eventually` |
| `eventually(X)` fails | Cycle exists that avoids `X` | Break the cycle, add a retry bound, or weaken the property |
| `never(X)` fails | Direct or transitive arrow reaches `X` | Remove the arrow |
| `after.S eventually T` fails | After `S`, the sub-machine can loop | Same as top-level fix, but scoped to the sub-machine |
| `distribution contradiction` | Scale says strong, slot is ephemeral | Change slot props or change `consistency` |

Rule of thumb: **every state machine should have at least one terminal state reachable from every other state**, and `eventually` should list exactly the terminal set.

## 13. Face contracts (`face { }`)

Face is the cell's public contract. It's structural and checked at compile time.

```soma
face {
    given threshold: Int where { threshold > 0 }     // parameter declaration
    signal create(name: String) -> Map                // MUST have `on create(...)` handler
    signal delete(id: String)                         // same
    await  trade(data: Map)                           // MUST be emitted by a sibling
    tool   search(q: String) -> String "Search the web"  // agent-only: LLM tool-calling spec
    promise all_persistent                            // structural check: all memory is [persistent]
    promise "human-readable description"              // generates doc warning
    promise latency < 100ms                           // bounded-latency promise (structural)
}
```

Semantics:

- `signal foo(...)` without a matching `on foo` handler → compile error.
- `await foo(...)` with no sibling `emit foo` → compile error (cell would block).
- `on foo` with no matching `signal foo` anywhere → warning (dead handler).
- Unmatched `emit` → warning (signal is lost).
- `promise` forms: structural (`all_persistent`, `exactly_once`, `latency < Xms`) are checked; string form is a doc annotation.
- Parent `promise` composes downward — if the parent promises `all state encrypted`, every descendant memory slot must be `[encrypted]`.
- `given ... where { ... }` declares a parameter with a constraint that the compiler verifies at call sites.

## 14. Interior cells and runtime

```soma
cell System {
    interior {
        cell Worker { ... }
        cell Cache  { ... }
    }
    runtime {
        start Worker                         // bring up interior cell
        connect Worker.done -> Cache         // wire a signal
        emit initialize()                    // fire a startup signal
    }
}
```

- Interior cells are composed children. Their faces are checked for signal matching with each other and the parent.
- `runtime { }` is orchestration: `start`, `connect producer.signal -> consumer`, `emit initial_signal(args)`.
- `soma describe` dumps the full interior graph as JSON.

## 15. Agents (`cell agent ...`) and LLM builtins

Agent cells unlock LLM primitives. The unique pitch: **the compiler proves your agent terminates**, because the agent's lifecycle is a verified state machine and `think()` transitions are just handler calls inside that machine.

```soma
cell agent Researcher {
    face {
        signal research(topic: String) -> Map
        tool search(q: String) -> String "Search the web"
        tool summarize(text: String) -> String "Summarize text concisely"
        promise "agent always reaches done or failed state"
    }

    memory {
        findings: Map<String, String> [persistent]
        log:      Map<String, String> [ephemeral]
    }

    state workflow {
        initial: idle
        idle        -> researching
        researching -> analyzing
        analyzing   -> done
        *           -> failed
    }

    on search(q: String)      { "stub for: {q}" }   // tool implementation
    on summarize(text: String){ "summary: {text}" }

    on research(topic: String) {
        set_budget(5000)                                  // hard token cap
        transition("task", "researching")
        let r = try { think("Research {topic}") }
        if r.error != () {
            transition("task", "failed")
            return map("status", "failed", "error", r.error)
        }
        transition("task", "analyzing")
        let s = try { think("Synthesize: {r.value}") }?   // ? propagates
        findings.set(topic, map("topic", topic, "summary", s))
        transition("task", "done")
        map("status", "done", "tokens", tokens_used())
    }
}
```

### Agent builtins

| Builtin | Signature | Notes |
|---|---|---|
| `think(prompt)` | `(String) -> String` | LLM call with auto tool dispatch + retry; uses `[agent]` in `soma.toml` |
| `think_json(prompt)` | `(String) -> Map` | LLM returns structured Map, parsed |
| `delegate(cell, signal, args...)` | cross-cell task dispatch | Target cell name is a string |
| `set_budget(n)` | `(Int) -> Unit` | Hard cap; further `think` calls error when exceeded |
| `tokens_used()` | `() -> Int` | Tokens consumed in this handler |
| `remember(k, v)` / `recall(k)` | Persistent agent memory | Backed by `[persistent]` slot under the hood |
| `approve(action)` | human-in-the-loop gate | Blocks until human approves; returns Bool |
| `trace()` | `() -> List<Map>` | Full execution log for this invocation |

### Agent config in `soma.toml`

```toml
[agent]
provider = "ollama"                # ollama | openai | anthropic
model    = "gemma3:12b"

# OpenAI
# provider = "openai"
# model    = "gpt-4o-mini"
# key      = "${OPENAI_API_KEY}"    # ${VAR} expands env vars

# Anthropic (Claude)
# provider = "anthropic"
# model    = "claude-opus-4-6"      # use the latest Claude 4.6 model IDs
# key      = "${ANTHROPIC_API_KEY}"

# Custom endpoint
# url   = "https://your-api.com/v1/chat/completions"
# model = "your-model"
# key   = "your-key"
```

Env vars always override `soma.toml`. `SOMA_LLM_KEY`, `SOMA_LLM_URL`, `SOMA_LLM_MODEL` are the universal overrides. **Never put raw keys in `soma.toml` — use `${ENV_VAR}`.**

### Proving agent termination — the killer feature

The state machine + `set_budget` + `[verify]` combo is what lets you write:

```toml
[verify]
deadlock_free = true
eventually    = ["done", "failed"]
[verify.after.researching]
eventually    = ["analyzing", "failed"]
[verify.after.analyzing]
eventually    = ["done", "failed"]
```

and have `soma verify` PROVE the agent can't livelock. Every agent cell must have (a) a state machine covering its lifecycle, (b) a `* -> failed` catch-all, (c) `eventually = [terminal_set]` in `soma.toml`. If these are absent, the verification story is a lie.

## 16. HTTP serving

`soma serve file.cell [-p 8080]` maps handlers to HTTP routes. The conventions:

| URL | Handler |
|---|---|
| `POST /signal/handler?k=v` | `handler(v)` |
| `GET /handler/arg1/arg2` | `handler(arg1, arg2)` |
| `GET /handler?k=v` | `handler(v)` |
| `POST /handler` + JSON body | `handler(field1, field2, ...)` — body fields extracted by param name |
| `GET /any/path` | falls through to `on request(method, path, body)` if defined |
| `GET /static/path` | serves from `static/` |

**Idiom: one fat `on request` handler.** Most real apps use a single `on request(method, path, body)` with a `match` on `(method, path)`:

```soma
on request(method: String, path: String, body: String) {
    let req = map("method", method, "path", path)
    match req {
        {method: "GET",  path: "/"}                  -> html(home())
        {method: "GET",  path: "/api/items"}         -> items.values |> map(s => from_json(s))
        {method: "POST", path: "/api/items"}         -> create(from_json(body))
        {method: "POST", path: "/api/items/" + id}   -> update(id, from_json(body))
        {method: "DELETE", path: "/api/items/" + id} -> delete_item(id)
        _                                            -> response(404, map("error", "not found"))
    }
}
```

### Response helpers

| Builtin | Purpose |
|---|---|
| `response(status, body)` | Custom HTTP response |
| `html(body)` / `html(status, body)` | `text/html` response, auto-injects HTMX if body contains `hx-*` attrs |
| `redirect(url)` / `redirect(status, url)` | 302 (default) or custom |
| Return a `Map` directly | Auto-serializes as JSON with 200 |
| `sse(event, data)` | Server-sent-events frame |

### Server flags

| Flag | Purpose |
|---|---|
| `-p 8080` | HTTP port (WS = +1, bus = +2) |
| `--verbose` | Log every request |
| `--watch` | Auto-reload on `.cell` changes |
| `--join host:port` | Join cluster via seed's bus port |

## 17. Inter-cell and inter-process signals

```soma
// Emit inside a handler
emit trade(map("ticker", "BTC", "qty", 1))
signal trade(map(...))                  // alias for emit

// Handle
on trade(data: Map) { record_fill(data) }
```

For inter-process, configure peers in `soma.toml`:

```toml
[peers]
exchange = "localhost:8082"
```

Signals sent via `emit` reach all peers on the bus. Use `delegate(cell, signal, args)` for targeted cross-cell calls within the same process.

## 18. File I/O, time, utility builtins

```soma
let content = read_file("data.txt")
write_file("out.txt", content)
let rows = read_csv("data.csv")        // list of maps, auto-typed columns

let ts   = now()                       // unix seconds
let ms   = now_ms()
let date = today()                     // "2026-04-08"
let fmt  = format_date(ts)

let id = next_id()                     // monotonic auto-increment (per cell)
let r  = random()                      // Float 0..1
let r  = random(100)                   // Int 0..99
let r  = random(10, 20)                // Int 10..19
```

## 19. Project layout

```
myapp/
  soma.toml         # manifest + [verify] + [agent] + [peers] + [dependencies]
  main.cell         # entry point (usually)
  lib/
    helpers.cell    # `use lib::helpers`
    scoring.cell
  static/           # served at /static/
  .soma_env/        # isolated env
    stdlib/         # property defs
    packages/       # dependencies
    cache/          # bytecode cache
  .soma_data/
    soma.db         # SQLite for [persistent]
```

`soma.toml`:

```toml
[package]
name        = "my-app"
version     = "0.1.0"
description = ""
author      = ""
entry       = "main.cell"

[dependencies]
# foo = "0.1.0"
# foo = { git = "https://...", version = "main" }
# foo = { path = "../local" }

[agent]                  # optional — only for agent cells
provider = "ollama"
model    = "gemma3:12b"

[peers]                  # optional — for inter-process signals
exchange = "localhost:8082"

[verify]                 # optional — drives `soma verify`
deadlock_free = true
eventually    = ["done", "failed"]

[verify.after.researching]
eventually    = ["analyzing", "failed"]
```

Imports:

```soma
use other_file          // sibling .cell file (no extension)
use lib::helpers        // lib/helpers.cell
use std::builtins       // stdlib
use pkg::math           // installed package
```

## 20. Testing

```soma
cell test MathTests {
    rules {
        assert 1 + 1 == 2
        assert len("hello") == 5
        assert round(3.7) == 4
        assert add(2, 3) == 5               // can call handlers from other cells
    }
}
```

Run: `soma test file.cell`. The test harness exposes handlers in the same file. Test cells can declare their own `memory` for fixture data.

## 21. CLI command reference

| Command | Purpose |
|---|---|
| `soma run file.cell [args]` | Execute entry handler |
| `soma run file.cell --signal name [args]` | Execute a specific handler |
| `soma run file.cell --jit [args]` | Use bytecode VM backend |
| `soma serve file.cell [-p 8080]` | HTTP server |
| `soma serve file.cell --verbose` | Verbose logging |
| `soma serve file.cell --watch` | Hot reload |
| `soma serve file.cell --join host:port` | Cluster mode |
| `soma check file.cell` | Contract + property + signal checking |
| `soma check --json file.cell` | Machine-readable `{kind, fix}` errors |
| `soma fix file.cell` | Auto-repair |
| `soma verify file.cell` | CTL model checking |
| `soma build file.cell [-o out.rs]` | Generate Rust skeleton (native codegen) |
| `soma test file.cell` | Run test cells |
| `soma init [name]` | Initialize project |
| `soma add pkg [--git URL] [--path DIR]` | Add dependency |
| `soma install` | Install dependencies |
| `soma repl` | Interactive REPL |
| `soma ast file.cell` | Dump AST |
| `soma tokens file.cell` | Dump tokens |
| `soma describe file.cell` | JSON description (handlers, memory, state, tools) |
| `soma lint file.cell` | Anti-pattern checks |
| `soma env` | Environment info |
| `soma props` | List registered properties |

Three execution backends co-exist:

1. **Tree-walking interpreter** — default; best for development.
2. **Bytecode VM** — `--jit`; compiled once, faster for hot loops.
3. **Native Rust codegen** — via `soma build`; the current frontier, with dual-mode `Int`/`BigInt` dispatch. Whole-loop BigInt via `rug` (GMP) is near-native speed; single BigInt ops go through FFI and are slower.

All three backends must agree on semantics. If you see divergence, the interpreter is the reference.

## 22. MCP tools (`mcp__soma__*`)

When running inside Claude Code, prefer these over CLI — they return structured JSON and integrate with the workflow loop:

| Tool | Purpose |
|---|---|
| `soma_generate` | Write a `.cell` file |
| `soma_generate_toml` | Write a `soma.toml` |
| `soma_check` | Run checker, return JSON diagnostics |
| `soma_verify` | Run model checker, return temporal results |
| `soma_run` | Execute a handler |
| `soma_test` | Run test cells |
| `soma_serve` | Start HTTP server |
| `soma_stop` | Stop a running server |
| `soma_describe` | Introspect a cell (structure + behavior) |

The workflow prompt from `AGENT.md`: **generate → check → verify → serve**, each a gate. Don't skip verify.

## 23. Common mistakes (ranked by frequency)

1. **Using `[1, 2, 3]` instead of `list(1, 2, 3)`** — square brackets are for property annotations only.
2. **Odd-arg `map(...)`** — `map("a", 1, "b")` is wrong.
3. **`function foo()` / `def foo()`** — use `on foo()`.
4. **`null` / `None` / `nil`** — use `()`.
5. **`import x`** — use `use ...`.
6. **Forgetting `else` on `if`-as-expression.**
7. **`return match ...`** — just write the match.
8. **`return` inside a `for` loop** — exits the handler, not the loop.
9. **Wrapping a map in `to_json()` before `slot.set()`** — storage auto-serializes.
10. **Calling `from_json` on a `()` result** — always null-check `slot.get(k)` first.
11. **`items.keys()` instead of `items.keys`** — no parentheses on slot properties.
12. **`unique()` instead of `distinct()`.**
13. **State machine with a cycle but `eventually = ["terminal"]`** — liveness fails.
14. **Memory `[ephemeral]` + `scale { shard: that_slot }`** — contradiction.
15. **Multi-line pipe inside a `match` arm** — put pipe on one line.
16. **Missing handler for a `face { signal foo }` declaration** — compile error; `soma fix` repairs.
17. **Agent cell without a state machine** — verification story collapses.
18. **Raw API keys in `soma.toml`** — use `${ENV_VAR}`.
19. **`to_int("abc") == 0`** assumption — it's `()`, null-check it.
20. **Integer division surprise** — `7 / 2 == 3.5`, use `floor` for truncation.

## 24. Idiomatic templates

### 24.1 CRUD web app

```soma
cell App {
    memory {
        items: Map<String, String> [persistent, consistent]
    }

    state item_lifecycle {
        initial: draft
        draft     -> active
        active    -> archived
        *         -> deleted
    }

    on create(data: Map) {
        let id = to_string(next_id())
        let item = data |> with("id", id) |> with("status", "draft")
        items.set(id, item)
        transition(id, "draft")
        return map("status", "created", "id", id)
    }

    on request(method: String, path: String, body: String) {
        let req = map("method", method, "path", path)
        match req {
            {method: "GET",  path: "/"}                -> html("<h1>App</h1>")
            {method: "GET",  path: "/api/items"}       -> items.values |> map(s => s)
            {method: "POST", path: "/api/items"}       -> create(from_json(body))
            {method: "GET",  path: "/api/items/" + id} -> items.get(id) ?? response(404, map("error","nf"))
            _                                           -> response(404, map("error", "not found"))
        }
    }
}
```

```toml
[package] name = "app"
[verify]
deadlock_free = true
eventually = ["deleted", "archived"]
```

### 24.2 Data pipeline

```soma
cell Pipeline {
    on run() {
        let data = read_csv("input.csv")
        let result = data
            |> filter(s => s.score > 50)
            |> sort_by("score", "desc")
            |> top(10)
        print(result)
        result
    }
}
```

### 24.3 Verified agent

```soma
cell agent Writer {
    face {
        signal write(topic: String) -> Map
        tool outline(topic: String) -> String "Make an outline"
        promise "eventually reaches done or failed"
    }
    memory { drafts: Map<String, String> [persistent] }
    state workflow {
        initial: idle
        idle      -> outlining
        outlining -> drafting
        drafting  -> done
        *         -> failed
    }
    on outline(t: String) { "## {t}\n1.\n2.\n3." }
    on write(topic: String) {
        set_budget(3000)
        transition("task", "outlining")
        let o = try { think("Outline {topic}") }?
        transition("task", "drafting")
        let d = try { think("Draft from: {o}") }?
        drafts.set(topic, d)
        transition("task", "done")
        map("ok", true, "tokens", tokens_used())
    }
}
```

```toml
[agent] provider = "ollama" model = "gemma3:12b"
[verify]
deadlock_free = true
eventually = ["done", "failed"]
[verify.after.outlining] eventually = ["drafting", "failed"]
[verify.after.drafting]  eventually = ["done", "failed"]
```

### 24.4 Distributed cell (cluster)

```soma
cell PricingEngine {
    face { signal book_trade(data: Map) -> Map }
    memory {
        trades: Map<String, String> [persistent, consistent]
        cache:  Map<String, String> [ephemeral, local]
    }
    state trade {
        initial: queued
        queued    -> confirmed
        confirmed -> executed
        executed  -> settled
        *         -> cancelled
    }
    scale {
        replicas:    50
        shard:       trades
        consistency: strong
        tolerance:   2
    }
    on book_trade(data: Map) {
        let id = to_string(next_id())
        trades.set(id, data |> with("id", id) |> with("status", "queued"))
        transition(id, "queued")
        map("id", id)
    }
}
```

```toml
[verify]
deadlock_free = true
eventually = ["settled", "cancelled"]
[verify.after.executed]
never      = ["cancelled"]
eventually = ["settled"]
```

Run: `soma serve pricing.cell -p 8080` (seed), then `soma serve pricing.cell -p 8081 --join localhost:8082` (join).

## 25. Final pre-serve checklist

Before calling `soma_serve`:

1. `soma fix` has run (or there were no fixable issues).
2. `soma check` returns `{"passed": true}`.
3. `soma verify` returns `{"passed": true}` OR you've consciously weakened properties and documented why in `soma.toml` comments.
4. Every state machine has a terminal set reachable from every state, and `eventually` lists that set.
5. Every POST route has a matching arm in `on request`.
6. Every `face { signal foo }` has an `on foo` handler.
7. Agent cells have `set_budget(N)` at the top of every LLM-entry handler.
8. No raw API keys in `soma.toml` — only `${ENV_VAR}`.
9. Storage reads (`slot.get`) are null-checked before `from_json`.
10. `soma describe` returns the shape you expect (sanity check).

## 26. When in doubt

- Read the relevant stdlib cell under `stdlib/` — properties, builtins, backends are all defined there in Soma itself.
- Look for the pattern in `examples/` (83+ files covering agents, pipelines, CRUD, chat, math, etc.).
- Prefer the interpreter's behavior as the reference when backends disagree.
- Run `soma describe file.cell` to get a JSON view of what the compiler understood — if it disagrees with your mental model, trust the compiler.
- If a feature sounds aspirational (intent compilation, repair plans, live re-verification, behavioral reflection), check `VISION.md` — it may not be implemented yet.

The Soma bet is: **if the model is regular enough, the compiler can verify what matters before anything runs.** Your job as an agent is to keep the program inside that regular shape — cells, signals, memory with property annotations, state machines with terminal sets — so the compiler can do its job.
