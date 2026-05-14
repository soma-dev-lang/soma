---
name: budget-proof
description: V1.4 — compile-time proof that peak memory ≤ declared budget.
type: feature
since: V1.4
related: [scale, memory, handler, think, stdlib-linalg, verification-overview]
---

# Memory budget proof (V1.4)

If a cell declares `scale { memory: "128Mi" }`, the compiler proves the
peak memory consumption is bounded by that value. Three outcomes:

- **Proven** — closed-form bound fits. Emit a `BudgetOk` info.
- **Exceeded** — bound exceeds budget. Compile error with breakdown.
- **Advisory** — handler calls an unbounded builtin without a bound.
  Lists the call sites that prevent the proof.

```
$ soma check rebalancer/app.cell

✓ budget proven for cell 'Optimizer':
  peak ≤ 69.89 MiB ≤ declared 128.00 MiB
  breakdown: slots 0 B + max-handler 53.89 MiB + state 0 B + runtime 16.00 MiB
```

## The formula

```
peak_memory(C) ≤ slot_sum(C)
              + max_{h ∈ handlers(C)} handler_peak(h)
              + state_machine_bound(C)
              + C_runtime
```

The `max` (not `sum`) over handlers reflects the runtime model: one
cell instance runs one handler at a time. Summing was unsound (overly
conservative by a factor of |handlers|).

For interior children, peaks aggregate into the parent's budget.

## Annotation surface

The checker reads existing memory-property annotations:

- `[capacity(N)]` — upper bound on collection entries.
- `[max_key_bytes(N)]` — upper bound on Map keys.
- `[max_value_bytes(N)]` — upper bound on Map values.
- `[max_element_bytes(N)]` — upper bound on List elements.
- `[max_instances(N)]` on `state` blocks — bounds dynamic state-machine
  instances.
- `[loop_bound(N)]` on `for`/`while` — explicit iteration bound.
- `[max_input_bytes(N)]` on handlers — bounds parameter size.

Defaults (used when annotation is missing):

- `DEFAULT_CAPACITY = 10_000`
- `DEFAULT_MAX_KEY_BYTES = 256`
- `DEFAULT_MAX_VALUE_BYTES = 4 KiB`
- `DEFAULT_MAX_ELEMENT_BYTES = 4 KiB`
- `DEFAULT_MAX_INSTANCES = 10_000`
- `HANDLER_STACK_OVERHEAD = 8 MiB` (per active handler)
- `C_RUNTIME = 16 MiB`

## Unbounded builtins

Some builtins are inherently unbounded. Calling one without
bracketing options downgrades the cell to **advisory**:

```
think(prompt)                          — LLM response unbounded
http_get(url)                          — HTTP response unbounded
from_json(input)                       — input size unbounded
read_file(path)                        — file size unbounded
delegate("X", "sig", args)             — target response unbounded
sql_query(...) / query(...)            — result set unbounded
recall(key) / recall_similar(query)    — recall result unbounded
to_json(value)                          — output scales with input
```

The fix: pass bounds in an options map:

```soma
think("prompt", map("max_tokens", 500, "timeout", 10000))  // → bounded
http_get(url,   map("max_bytes", 65536, "timeout", 5000))  // → bounded
```

The checker reads `max_tokens` / `max_bytes` from the literal map at
compile time and computes a closed-form contribution (e.g. 4 bytes per
token for LLM responses).

## V1.5 extension: linalg / risk builtins

Sample / iteration bounds on quantum-inspired algorithms (and friends)
also become budget contributions:

```soma
regress_sgd(A, b, map(
    "max_iter", 10000,
    "samples_per_iter", 4,
    "max_dim", 32
))
// Bound: 4 transient vectors of 8·32 bytes + output Map +
//        audit budget (max_iter × samples_per_iter × 8)

svd_lowrank(A, map(
    "row_samples", 100, "col_samples", 50,
    "rank", 10, "max_dim", 1000
))

clean_covariance(returns, map(
    "method", "rie",
    "max_assets", 500, "max_obs", 1000
))
```

See [[stdlib-linalg]] and [[stdlib-risk]] for the per-builtin cost
contributions.

## How tight is the bound?

Measured on real data (rebalancer):

- `[ephemeral]` (HashMap, 10–50 MiB of data): bound ≈ **1.3–2.2× real RSS**.
- `[persistent]` (SQLite on disk, 100 MiB of data): bound ≈ 150 MiB
  (worst-case if backend were HashMap); actual RSS ~8 MiB (page
  cache). The checker is honest about modelling the worst case.

Tight enough to be useful, conservative enough to be safe.

## Coq backing

`Soma_Budget.v` + `Soma_BudgetOps.v`: cost lattice laws are
mechanically verified. Zero axioms. Reproducible with
`make -C docs/rigor/coq check`.

Per-builtin cost assignments and the AST walker are **trusted by
source inspection** — not (yet) machine-verified. See
[[coq-scorecard]] for what's behind the asterisk.

## Examples

A simple bounded cell:

```soma
cell Counter {
    scale { memory: "1Mi" }
    memory {
        counts: Map<String, String> [
            ephemeral, local, capacity(100), max_value_bytes(16)
        ]
    }
    on inc(k: String) {
        counts.set(k, to_string(to_int(counts.get(k) ?? "0") + 1))
    }
}

// soma check:
// ✓ budget proven for cell 'Counter': peak ≤ 17.00 KiB ≤ 1.00 MiB
```

A bounded agent:

```soma
cell agent Researcher {
    scale { memory: "256Mi" }
    on research(topic: String) {
        let facts = think("Research {topic}", map("max_tokens", 3000))  // bounded
        let summary = think("Summarize: {facts}", map("max_tokens", 500))
        map("summary", summary)
    }
}

// soma check:
// ✓ budget proven for cell 'Researcher': peak ≤ 24.34 MiB ≤ 256.00 MiB
//   breakdown: slots 0 B + max-handler 8.34 MiB (think bounds)
//            + state 0 B + runtime 16.00 MiB
```

An unbounded cell (advisory path):

```soma
cell Adaptive {
    scale { memory: "256Mi" }
    on summarize(text: String) {
        think(text)        // no max_tokens
    }
}

// soma check:
// ⚠ advisory: cell 'Adaptive' declares budget 256.00 MiB; bounded
//   portion is 24.00 MiB, but the following handlers call unbounded
//   builtins so the proof is incomplete:
//   → think at file.cell:3:9 — LLM response size is not statically bounded
```

## Edge cases

- `think_json` is treated identically to `think` for budget purposes.
- `to_json(x)` is unbounded because its output scales with the
  unknown size of `x`. Use it only with bounded inputs.
- The checker walks `match` arms and takes the **max** over arms
  (only one fires per match). Same for `if` then/else.
- Recursive handlers compound the per-handler overhead up to the
  global depth limit (512). Unbounded recursion is rejected by
  [[termination]].

## What this does NOT cover

- **Schema-evolution bounds.** Adding a field to a `[persistent,
  consistent]` slot in V2 may grow memory; the checker doesn't track
  versions.
- **Concurrent handler execution.** The `max` formula assumes one
  active handler per cell. Soma's runtime currently enforces this;
  if that ever changes the formula needs revisiting.
- **The Coq proof of the per-builtin cost assignments.** Currently
  trusted by inspection.
