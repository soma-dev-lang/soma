<!-- Generated from site/agent.md by tools/build_site.py. -->

# Soma — instructions for coding agents

This block belongs in the `AGENTS.md` / `CLAUDE.md` of a project that uses
Soma; `soma init` writes it, `soma doctor` says when it is out of date. It
is short on purpose: the installed toolchain carries the rest, at the
version you run.

## What this project is written in

`.cell` files are [Soma](https://soma-lang.dev): a *cell* holds a contract
(`face`), storage with invariants (`memory`), a model-checked lifecycle
(`state`), handlers (`on`), HTTP routes and tests. A larger program is a
*skeleton* (the structure, written and proven first) and its *flesh* (the
bodies, one signal at a time). Soma is experimental: what is proven,
checked at run time or not covered is in `soma docs guarantees`; the
release's validation figures are at https://soma-lang.dev/status.

## The loop — after every edit, in this order

```
soma check  app.cell     # static gates: contracts, names, frames, the law
soma verify app.cell     # proves supported properties; reports runtime checks
soma test   app.cell     # runs `cell test` assertions
```

- Fix `check` first: it is cheap and each error names its fix.
- A `verify` failure is an error. Do not delete the property to pass:
  change the machine or the handler.
- When you report the work, paste the `verify` summary line. That is the
  evidence; "should be correct" is not.

## Skeleton first

```
soma next     app.cell                  # the ONE thing to do now, and its command
soma skeleton app.cell Orders.place     # the brief for ONE signal: all its body needs
soma fill     app.cell Orders.place     # write that body (on stdin); it is checked
soma docs flesh                         # the syntax a body needs, on one page
```

- One hole at a time, until `soma next` answers `complete`.
- A body may only do what its signal's frame says (`reads` / `writes` /
  `moves` / `calls` / `emits`); `soma check` names the missing clause.

## What you do not edit

- A `skeleton`, when `skeleton.lock` exists: fix the body. A real design
  change is a separate act (`soma skeleton app.cell --amend`) and belongs
  in your report.
- A `.law` file or `law.lock`: the business owner writes the law. The code
  is aligned on it (`soma law app.cell --todo`).
- A frame, to silence an error: a frame widened for one body is a design
  change nobody reviewed.

## Never guess

```
soma describe --builtins --json      # exact signature of every builtin
soma describe app.cell --faces       # the contract of every cell
soma explain E0080                   # a diagnostic code, with its fix
soma example invariant state_machine # a verified program with these features
soma docs                            # the topics; `method` and `gotchas` first
```

## The rules models get wrong

- `match` arms use `->`; lambdas use `=>`. Return types go on the `signal`
  in `face`, never on `on`. No semicolons.
- `7 / 2` is `3.5`; `idiv(7, 2)` is `3`.
- `if` is an expression when it has an `else`. `return` exits the handler,
  including from a loop.
- `.get/.set/.delete` are for `memory` slots; local maps use `m["k"]`.
- Null is `()` (`x == ()`, `x ?? 0`). `push` / `concat` return NEW lists
  (`xs = push(xs, x)`). `==` is structural on lists and maps.
- Interpolation takes expressions: `"{split(text, \":\")[0]}"`; double
  braces are literal braces.
- `transition(id, "from", "to")` declares the source: `check` verifies the
  edge. Read the state with `get_status(id)`.
- A local is not named like a slot of its cell (an error from edition 2027).

The rest, each with its real diagnostic: `soma docs gotchas`.

## Install

```
curl -fsSL https://soma-lang.dev/install.sh | sh
```
