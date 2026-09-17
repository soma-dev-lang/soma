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
soma verify app.cell     # PROVES state machines and memory invariants
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
https://soma-lang.dev/corpus/index.json (300+ verified programs — fetch one
with the `features` you need before writing a new cell).

## The rules models get wrong

- `match` arms use `->`; lambdas use `=>`. Return types go on the `signal`
  in `face`, never on `on`.
- No semicolons. No nested `"..."` inside `{...}` interpolation — bind a
  `let` first.
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

## Install

```
curl -fsSL https://soma-lang.dev/install.sh | sh
```
