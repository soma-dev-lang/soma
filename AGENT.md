<!-- Generated from site/agent.md by tools/build_site.py. -->

# Soma — instructions for coding agents

Paste this block into the `AGENTS.md` / `CLAUDE.md` of a project that uses
Soma (`soma init` writes it for you). It is short on purpose: the toolchain
carries the rest.

## What this project is written in

`.cell` files are [Soma](https://soma-lang.dev): one *cell* holds a contract
(`face`), storage with invariants (`memory`), a model-checked lifecycle
(`state`), handlers (`on`), HTTP routes and tests.

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

## Cluster corrections in Soma 2.8.2 (2026-09-20)

Use [Soma 2.8.2](https://github.com/soma-dev-lang/soma/releases/tag/v2.8.2).
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
