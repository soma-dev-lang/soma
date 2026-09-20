# Soma — instructions for coding agents

Paste this block into the `AGENTS.md` / `CLAUDE.md` of a project that uses
Soma (`soma init` writes it for you). It is short on purpose: the toolchain
carries the rest.

## What this project is written in

`.cell` files are [Soma](https://soma-lang.dev): one *cell* holds a contract
(`face`), storage with invariants (`memory`), a model-checked lifecycle
(`state`), handlers (`on`), HTTP routes and tests.

## Implementation scope — Soma 2.8.8

The language core is implemented; recent audits cover numeric boundaries,
typed inputs and storage, evaluation order, collections, dispatch, constructors,
SQLite failures and transaction boundaries.
Release validation passed 629 Rust tests, 118 CLI checks, 321 corpus programs
and 1,280 independent numeric comparisons. Evidence and current status:
https://soma-lang.dev/status.

Cluster replication is experimental and eventual: typed Map entries replicate
after commit, reconnect and state exchange repair missed updates, and per-key
logical versions resolve conflicts. Reads may be stale and concurrent updates
can overwrite each other. Consensus, physical sharding and exactly-once delivery
are not implemented; `strong` and `causal` declarations are rejected.

Static proofs are per cell and cover the inputs represented by the supported
analysis: state graphs, memory invariants, termination and cost bounds. Graph
liveness does not ensure handler invocation. Cluster behavior, arbitrary handler
semantics, cross-cell rules and LLM answers are outside these proofs. Unproved
checks are reported; `soma verify --strict` rejects warnings. Exact scope:
https://soma-lang.dev/docs/guarantees.md.

## The loop — run it after every edit, in this order

```
soma check  app.cell     # static gates: contracts, undefined names, dispatch
soma verify app.cell     # proves supported properties; reports runtime checks
soma test   app.cell     # runs `cell test` assertions
```

- Fix `check` first: it is cheap and its errors name their fix
  (`invalid transition: Placed → Delivered. Valid targets: [Accepted, Cancelled]`).
- A `verify` failure is an error, not a warning. Do not work around it by
  deleting the property — change the machine or the handler.
- When you report the work, paste the `verify` summary line. That is the
  evidence; "should be correct" is not.

## Never guess

```
soma describe --builtins --json      # exact signature of every builtin
soma describe app.cell --faces       # the contract of every cell, token-cheap
soma docs agent                      # the language summary, offline
```
Online: https://soma-lang.dev/llms-full.txt (everything, one fetch),
https://soma-lang.dev/gotchas.json (mistakes with their diagnostics),
https://soma-lang.dev/corpus/index.json (verified programs — fetch one
with the `features` you need before writing a new cell).

## The rules models get wrong

- `match` arms use `->`; lambdas use `=>`. Return types go on the `signal`
  in `face`, never on `on`.
- No semicolons. Interpolation supports expressions and quoted arguments:
  `"{split(text, \":\")[0]}"`. Double braces produce literal braces.
- Lists: `[1, 2]` or `list(1, 2)`. `if` is an expression when it has an
  `else`: `let x = if ok { 1 } else { 0 }`. `return` exits the handler,
  including from a loop.
- `7 / 2` is `3.5`; `idiv(7, 2)` is `3`.
- `transition(id, "state")` returns `{id, from, to}`; read the state with
  `get_status(id)`. Wrap a transition that may be illegal in `try { }`.
- `.get/.set/.delete` are for `memory` slots; local maps use `m["k"]`.
- `==` is structural on lists/maps; null is `()` (`x == ()`, `x ?? 0`); `push`/`concat`
  return NEW lists (`xs = push(xs, x)`); `+` on numeric lists adds element-wise.
- Guards (`a -> b { guard { amount < 100 } }`) read the locals of the handler that
  calls `transition()`.
- Agents: `cell agent` + a `state` machine + `set_budget(N)`. Test offline
  with `[agent] mock = "echo"` in `soma.toml`.

## Persistence corrections in Soma 2.8.8 (2026-09-20)

Soma 2.8.8 checks SQLite transaction boundaries and propagates refused writes
from `remember()`, `next_id()` and transitions. Failed commits withhold queued
events and replication; failed task boundaries stop execution. Auxiliary tables
open before the transaction. List writes handle failures atomically, indexed
reads handle gaps, and normal returns from scheduled ticks commit their writes.
`next_id()` rejects counter exhaustion/corruption and migrates only its own
cell's legacy counter. Twenty-nine new regression tests cover these paths.
Compatibility: a lost transaction aborts the invocation even inside `try`;
already committed task steps and external effects are not undone.
Details: https://soma-lang.dev/CHANGELOG.md.

## Scope and memory corrections in Soma 2.8.7 (2026-09-20)

Soma 2.8.7 fixes `if` expression scope, lambda capture inside interpolations
and error details, and collection evaluation when interpolations assign locals.
Static analyses now see calls containing quoted colons: recursion, invariant
writes and token costs cannot disappear from the proof. `recall()` stays within
its cell and preserves JSON-shaped Strings; `remember()` rejects functions and
excessive encoded nesting before writing. Twenty-four integration regressions
reproduce the defects on 2.8.6; a unit test covers legacy memory migration.
Compatibility: parse remembered JSON text explicitly with `from_json()`; keep
branch locals inside their branch. Details: https://soma-lang.dev/CHANGELOG.md.

## Execution corrections in Soma 2.8.6 (2026-09-20)

Arithmetic assignments and optimized collection operations evaluate arguments
once, in order. Self-appends keep the source list readable; piped map updates
apply every pair and propagate later failures. `with` requires complete pairs.
Calls and pipes share local-lambda and cell-handler resolution. Optimized
operations honor builtin mocks, and block lambdas work in ordinary collection
calls. Struct/tuple constructors recursively coerce declared fields: Float fields
hold Floats, with out-of-range Ints rejected. Named-field syntax cannot construct
a tuple/unit variant. Twenty-four regression tests cover these corrections.
Details: https://soma-lang.dev/CHANGELOG.md.

## Input and storage corrections in Soma 2.8.5 (2026-09-20)

Implicit Int-to-Float promotion rejects values outside Float range. Typed maps
check all keys, including names beginning with `_`, and record/variant payloads.
Declared sum/record returns validate the variant and its fields. Memory writes
reject functions hidden inside variants and values exceeding 100 levels in the
storage encoding; record and escaped-map wrappers count toward this limit.
Local-list `nth` uses the same signed-64-bit index limit as the ordinary builtin.
`sleep` requires an Int duration (0 through 86400000 ms); other types raise `type`.
Eighteen integration regressions reproduce these defects on the previous release;
an additional unit test covers the minimum signed date count. Details:
https://soma-lang.dev/CHANGELOG.md.

## Numeric corrections in Soma 2.8.4 (2026-09-20)

Use the current release when handling wide numeric ranges. `avg`, `avg_by`
and grouped means average finite operands before Float rounding. Representable
standard deviations are preserved even when their variance is outside Float
range. Mixed medians keep the exact middle value; any NaN makes the median NaN.
`clamp` rejects nonnumeric operands and NaN bounds, compares numbers exactly,
and returns the selected operand with its type. A mixed median or clamp may
therefore return an Int. Nine regression tests and 320 independent reference
vectors cover these corrections; details: https://soma-lang.dev/CHANGELOG.md.

## Robotics robustness in Soma 2.8.3 (2026-09-20)

100 additional regression scenarios cover numeric boundaries, invalid sensor
values, mission transitions, storage invariants, rollback, native execution,
persistence across processes and replay. Run the suite in the language repository:
`cargo test --release --locked --manifest-path compiler/Cargo.toml --test robotics_robustness`.
The complete matrix and limits are at https://soma-lang.dev/docs/robotics.md
and offline with `soma docs robotics`. These are language tests, not hardware
qualification or hard real-time guarantees. Local rollback cannot undo physical
actuation. Eventual cluster replication does not establish distributed safety.

## Cluster corrections in Soma 2.8.2 (2026-09-20)

Use [Soma 2.8.8](https://github.com/soma-dev-lang/soma/releases/tag/v2.8.8), which includes these corrections.
`scale.shard` supports mutable Map slots with explicit `consistency: eventual`.
Updates preserve types and are published after commit. Reconnect and periodic
state exchange carry per-key logical versions and deletion markers. Use a
separate directory and reachable `SOMA_NODE_ID=host:bus-port` per replica.
Reads are local; concurrent writes can overwrite each other. This is full
replication, not physical sharding or consensus. `strong`, `causal`, replicated
Lists, immutable slots and memory invariants are refused. `verify --strict`
reports distribution as unproved. Scheduling is advisory during partitions;
signals are not replayed. Upgrade every node together (protocol v2).
Full setup and limits: https://soma-lang.dev/docs/cluster.md.

## Corrections included in Soma 2.8.1 (2026-09-20)

These fixes are included in [Soma 2.8.1](https://github.com/soma-dev-lang/soma/releases/tag/v2.8.1).
Check `soma --version`; use the installer below to upgrade. The full release
notes are at https://soma-lang.dev/CHANGELOG.md.

- Mixed Int/Float comparisons retain the integer's exact value, including
  beyond 2^53. Sorting, filtering and structural equality follow that rule.
  NaN is unordered and never equal; `filter_by` follows those semantics.
- `range(a, b, 0)` raises kind `range`. `top`/`bottom` require a List and a
  nonnegative Int count. A `for` over a user-defined `range` calls that handler.
- HTTP queries preserve `%2B` as `+`; a bare `?flag` becomes `flag: ""`.
- `soma fix --json` counts syntax repairs. New replay logs distinguish
  raised errors from returned maps containing `__error__`, and record native
  failures after the transaction finishes.
- Cost proofs reject overflowing loop bounds and do not infer builtin
  range lengths from a user-defined `range`. Use `loop_bound(N)` when a
  collection's size is unknown to the checker.
- Invalid or overflowing `mock now` / `mock now_ms` fails a test; it never
  falls back to the real clock. `days_in_month` requires Int arguments.
- Integer `while` optimization preserves outer operands, BigInt promotion
  and errors. Division rounds the exact ratio; nearby large numbers retain
  their differences in variance/stddev. `index_of` uses exact equality.
- `chr` accepts only valid Unicode scalar Ints. `substring` clamps both
  indexes to the string's character bounds; a negative end gives `""`.
  Bit counts cannot wrap to zero; numeric builtins enforce operand types.
- `--jit` is a deprecated compatibility no-op. Use `[native]` for numeric
  compilation; generated temporary names cannot replace user operands in
  bit operations or loop-bound counters.

## Install

```
curl -fsSL https://soma-lang.dev/install.sh | sh
```
