---
name: workflow
description: The Soma development loop — generate → fix → lint → check → verify → serve.
type: synthesis
since: V1.0
related: [verification-overview, manifesto]
---

# Workflow

The Soma development loop has six tight steps. Each is a verification
gate. Failures at any step block deployment.

```
generate  →  fix  →  lint  →  check  →  verify  →  serve
```

## `soma generate` — from intent to cell

```
soma generate "an order management cell with submit, validate, settle"
```

Outputs a `.cell` file with face, memory, state, and handler stubs.
Useful for greenfield work; agents and humans alike start here.

The output is **never** assumed correct — it must pass the next steps.

## `soma fix` — auto-repair

```
soma fix app.cell
```

Walks the AST, fixes common mistakes:

- Missing handlers for declared face signals → adds a stub.
- Contradictory memory properties → suggests a resolution.
- Stale state names in `transition()` calls → fixes typos (V1.5: uses
  variant similarity).
- Missing `[loop_bound(N)]` annotations on bounded loops → adds them.

The fixes are conservative — they don't change semantics, just close
syntactic gaps.

## `soma lint` — style and anti-patterns

```
soma lint app.cell
```

Catches non-error issues:

- Redundant `to_json` in trivial cases.
- Unchecked `.get()` (no `?? default`).
- Long `if-else if-else if` chains that should be `match`.
- Handlers that mutate memory but don't transition — possible bug.

Linter rules can be **project-specific**: `cell checker my_rule { … }`
declares a check that `soma lint` runs.

## `soma check` — compile-time guarantees

```
soma check app.cell
```

The fast feedback loop. Runs in milliseconds. Verifies:

- Structural correctness (face contracts, memory properties, scale
  consistency).
- [[refinement]] (handler bodies match the spec).
- [[budget-proof]] (V1.4 memory bound).
- Sum-type exhaustiveness (V1.5).
- Typed state-machine variant consistency (V1.5).
- Custom checkers (`cell checker`s in scope).

Exit code 0 → green. Exit code 1 → at least one error. Output is
JSON-parseable with `--json`.

## `soma verify` — deep semantic checks

```
soma verify app.cell
```

Adds on top of `check`:

- [[ctl-model-checking]] on every state machine.
- [[termination]] on every handler.
- [[think-isolation]] for `cell agent`s.
- [[composition]] of interior cells.
- Per-handler effect summaries.

Slower than `check` (seconds for complex cells), but exhaustive.
Output includes counter-examples for failures.

## `soma serve` — run it

```
soma serve app.cell -p 8080
soma serve app.cell -p 8080 --join coordinator:9000     # cluster mode
```

Starts the HTTP server (default :8080), the signal bus (8081), and the
WebSocket bus (8082). The dashboard at `/__soma/` shows state machines,
verification status, and the bounded-think token budget.

`--join` flips the runtime from standalone to cluster member. The
source code is identical.

## `soma run` — non-server execution

```
soma run script.cell
soma run script.cell arg1 arg2          # passes args to on run()
soma run --record bot.cell              # log [record] handlers for replay
soma replay bot.cell                     # bit-deterministic replay
```

`run` invokes `on run()` with optional args. Useful for batch scripts,
backtests, and offline analysis.

## The MCP tooling

For agents driving Soma development, every command has an MCP tool:

- `mcp__soma__soma_generate`
- `mcp__soma__soma_check`
- `mcp__soma__soma_verify`
- `mcp__soma__soma_serve`
- `mcp__soma__soma_describe` — structured introspection (JSON)
- `mcp__soma__soma_test`
- `mcp__soma__soma_stop`

Each returns machine-readable output (typed errors, breakdowns) so
agents can react programmatically.

## Agents working on Soma

The full agent loop (from `VISION.md`):

1. Express intent (could be natural language).
2. `soma generate` → first draft.
3. `soma check` → structured errors with `kind` + `fix` fields.
4. Agent applies fixes; loop.
5. `soma verify` → temporal proof; if it fails, parse the
   counter-example and either fix the state machine or weaken the
   property.
6. `soma serve` → production.

The tight feedback loop with machine-readable errors is what makes
agents productive on Soma.

## Examples

A typical session:

```
$ soma generate "an order pipeline"
[generates orders.cell]

$ soma check orders.cell
error: face contract: signal 'cancel' has no handler

$ soma fix orders.cell
✓ added stub for on cancel(id: String)

$ soma check orders.cell
✓ All checks passed.

$ soma verify orders.cell
✓ no deadlocks
✓ eventually(settled | cancelled)
✓ refinement: all 4 handlers checked
✓ termination: all 4 handlers structurally terminate
✓ budget proven: peak ≤ 23.4 MiB ≤ declared 32 MiB

$ soma serve orders.cell -p 8080
serving at http://localhost:8080
```

Six gates, all green, then deploy.

## Related

- [[verification-overview]] — what `check` and `verify` actually prove.
- [[manifesto]] — the philosophy this workflow implements.
