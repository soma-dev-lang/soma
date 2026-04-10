# soma

The language where the **handler bodies cannot lie to the state machine**.

`cell agent` + `think()` + state machine = **proven termination**, and as
of V1.3 the verifier proves the handlers actually implement the picture
they're drawn next to — the spec and the code can no longer drift apart.

```
soma serve agent.cell -p 8080                     # serve agent
soma verify agent.cell                            # PROVE the spec AND the handlers
soma serve app.cell -p 8081 --join localhost:8082  # cluster
```

## Install

```bash
curl -fsSL https://soma-lang.dev/install.sh | sh
```

Or build from source:
```bash
git clone https://github.com/soma-dev-lang/soma.git
cd soma/compiler && cargo build --release
sudo cp target/release/soma /usr/local/bin/
```

## Quick start

```bash
soma init myapp && cd myapp
soma serve app.cell          # http://localhost:8080
soma fix app.cell            # auto-repair errors
soma verify app.cell         # prove state machines
soma lint app.cell           # catch anti-patterns
```

## Verified AI Agents

```soma
cell agent Researcher {
    face {
        signal research(topic: String) -> Map
        tool search(query: String) -> String "Search the web"
    }

    state workflow {
        initial: idle
        idle -> researching -> analyzing -> done
        * -> failed
    }

    on search(query: String) {
        http_get("https://api.search.com?q={query}")
    }

    on research(topic: String) {
        set_budget(5000)                          // hard token cap
        transition("t", "researching")
        let facts = think("Research: {topic}")    // LLM + tool calling
        transition("t", "analyzing")
        let summary = think("Synthesize: {facts}") // multi-turn context
        transition("t", "done")
        map("summary", summary, "tokens", tokens_used())
    }
}
```

```
$ soma verify agent.cell

✓ no deadlocks
✓ eventually(done | failed)     ← PROVEN: agent always terminates
✓ after(researching, analyzing | failed)
4 passed, 0 failed
```

No other agent framework can prove this.

## Agent Runtime

| Builtin | What it does |
|---------|-------------|
| `think(prompt)` | LLM call with auto tool dispatch + retry |
| `think_json(prompt)` | LLM returns structured Map |
| `delegate(cell, signal, args)` | Cross-agent task dispatch |
| `set_budget(n)` / `tokens_used()` | Hard token cap enforcement |
| `remember(k, v)` / `recall(k)` | Persistent agent memory |
| `approve(action)` | Human-in-the-loop gate |
| `trace()` | Full execution log |

Config: `SOMA_LLM_KEY`, `SOMA_LLM_URL` (OpenAI or ollama), `SOMA_LLM_MODEL`

## Pattern Matching

```soma
on request(method: String, path: String, body: String) {
    let req = map("method", method, "path", path)
    match req {
        {method: "GET", path: "/"}                   -> home()
        {method: "GET", path: "/api/" + resource}    -> list(resource)
        {method: "POST", path: "/api/" + resource}   -> create(resource, body)
        n if n.method == "OPTIONS"                   -> cors()
        _ -> response(404, map("error", "not found"))
    }
}
```

Map destructuring, string prefix, guard clauses, or-patterns, range patterns — all composable.

## Agent Workflow

```
generate  →  fix  →  lint  →  check  →  verify  →  serve
```

- `soma fix` auto-repairs missing handlers, contradictory properties
- `soma lint` catches redundant to_json, unchecked .get(), if-chains
- `soma check --json` returns errors with `kind` + `fix` fields
- `soma describe` outputs rich JSON: handlers, memory, state machines, tools
- `soma verify` proves state machine properties with CTL model checking

## Refinement: handler bodies vs state machine (V1.3)

Soma's tagline is *"the specification is the program."* Before V1.3, that
was half true: the `state` block was the spec, the handler bodies were
the code, and the compiler treated them as independent documents. They
could drift apart silently — and they did, often.

V1.3 closes the gap. `soma verify` now proves three things about every
cell with a state machine:

1. **Every `transition("inst", "X")` call in any handler body names a
   state `X` that exists in the cell's `state { }` block.** A typo in a
   target state name is a compile error, not a runtime surprise.
2. **Every transition declared in the state block is reached by some
   handler.** Dead transitions in the spec become warnings — the spec
   might be aspirational, but the reader is told.
3. **Per-handler effect summary** — for every handler, the verifier
   prints the set of states it can transition to, with the path
   conditions (`if` guards) leading to each call.

```
$ soma verify rebalancer/app.cell

  ✓ refinement: handler `rebalance` ⟶ {signal_pending,
        failed [if alpha_cfg != () ∧ alpha_result.error != ()],
        blocked [if verdict == "BLOCK"],
        approved [if verdict == "APPROVE"],
        flagged}
```

This is the WOW feature the manifesto was claiming. Before V1.3,
"specification is the program" was a poster. Now it's a theorem.

## Memory-budget proof obligation (V1.4)

`scale { memory: "128Mi" }` is no longer advisory — the compiler
**proves** your cell fits.

```soma
cell Optimizer {
    scale {
        replicas: 1
        memory: "128Mi"
    }

    on optimize(input: Map) {
        // 200 lines of constraint math: position caps, turnover caps,
        // cash floor scaling, nested loops...
    }
}
```

```
$ soma check rebalancer/app.cell

✓ budget proven for cell 'Optimizer': peak ≤ 69.89 MiB ≤ declared 128.00 MiB
    breakdown: slots 0 B + max-handler 53.89 MiB + state 0 B + runtime 16.00 MiB
```

The checker walks every handler body, counts every allocation
(`list()`, `map()`, `push()`, string literals), unrolls loops by
their `[loop_bound(N)]` annotation or literal `range(0, N)`, takes
the **max** across handlers (not sum — only one runs at a time),
adds slot capacities and runtime overhead, and compares against the
declared budget. Three outcomes:

- **Proven** — closed-form bound fits. The cell will not OOM.
- **Exceeded** — bound exceeds budget. Compile error with breakdown.
- **Advisory** — handler calls an unbounded builtin (`think()`,
  `from_json()`, `http_get()`). The checker lists the exact call
  sites that prevent the proof instead of lying.

The cost lattice and the composition theorem are **mechanically
verified in Coq** (Rocq 9.1.1, zero axioms, zero `Admitted`).
No other general-purpose language proves memory budgets at compile
time. The only tools that do this are $100K/seat avionics analyzers
on restricted input languages.

**How tight is the bound?** Measured on real data (10K–50K entries
with unique ~1 KiB values):

| Slot type | Data | RSS | Proven bound | Ratio |
|---|---|---|---|---|
| `[ephemeral]` (HashMap in RAM) | 10 MiB | 17 MiB | 38 MiB | **2.2×** |
| `[ephemeral]` (HashMap in RAM) | 50 MiB | 67 MiB | ~85 MiB | **1.3×** |
| `[persistent]` (SQLite on disk) | 100 MiB | 8 MiB | 150 MiB | n/a — data lives on disk |

For `[ephemeral]` slots the bound is **1.3–2.2× the real RSS** —
tight enough to be useful, conservative enough to be safe. For
`[persistent]` slots the checker models in-memory capacity (sound
upper bound) but the runtime uses SQLite, so the actual RSS is just
the page cache (~2 MiB). The checker is honest about this: the
proven bound is what *would* happen with a HashMap backend, which is
the worst case. Interior cells that share a process get their peaks
aggregated into the parent's budget.

Technical details: `docs/SEMANTICS.md` §1.7. Coq proof:
`docs/rigor/coq/Soma_Budget.v`.

## Soundness — mechanically verified

The model checker's correctness is not just claimed — it's proven.

- **CTL safety** (`deadlock_free`, `always`, `never`, `mutex`):
  sound and complete on the abstract state machine (`Soma_CTL.v`).
- **CTL liveness** (`eventually`, `after`): sound after a depth-bound
  fix discovered during the rigor pass (`Soma_CTL.v`).
- **Think-isolation**: if all transition targets are literal,
  **CTL safety holds regardless of what the LLM returns**
  (`Soma_Isolation.v`). Tool handlers that call `transition()` are
  detected and excluded. Adversarial review found and closed the
  tool-calling side channel.
- **Runtime fidelity → safety transfer**: the full chain from
  "runtime guards transitions against G" to "safety holds on the
  trace" is mechanized with **no unproven gap** (`Soma_RuntimeFidelity.v`).
- **Budget composition**: cost lattice + per-builtin allocation
  bounds (`Soma_Budget.v` + `Soma_BudgetOps.v`).
- **Handler termination**: every handler body structurally
  terminates (no unbounded `while`, bounded `for` loops,
  decreasing recursion). `soma verify` reports it per cell.
- **Signal composition**: every `emit` in an `interior` block has a
  matching handler, every handler has a signal source. `soma verify`
  reports matched pairs and orphans.

**20 Coq theorems** across 6 files, all `Closed under the global
context` (zero axioms). Reproduce: `make -C docs/rigor/coq check`.

```
$ soma verify rebalancer/app.cell

✓ think-isolated: CTL safety holds regardless of LLM output
  (5 handlers, 20 literal transitions, 0 dynamic)
✓ termination: all 31 handlers structurally terminate
✓ 16 temporal properties passed
```

Every property names its **adversary model** explicitly in
`docs/ADVERSARIES.md`. Full rigor scorecard: `docs/rigor/README.md`.

## Deterministic record / replay

Production incidents, single-stepped on your laptop:

```bash
soma run --record bot.cell    # writes bot.somalog (JSON-lines, opt-in)
soma replay bot.cell          # bit-deterministic re-execution
```

Each replay entry passes if the live result matches the recorded one.
When a handler calls a nondeterministic builtin (`now`, `random`, …),
the recorder logs the call site and replay reports each divergence
with a suggested fix. Demo: `examples/v1/02_replay_trader.cell`.

## Performance: `[native]` vs Rust and C

`[native]` compiles handlers to a Rust `cdylib` per cell. Same source,
~100–300× speedup over the interpreter on tight numeric loops, and
**essentially tied with hand-written sequential Rust on the CLBG
numeric challenges** (geomean ~1.02×, faster on 3 of 5). The C
reference suite under `bench/clbg_c_ref/` reproduces the same
comparison against `clang -O3 -march=native`. Writeups:
`bench/results/SUMMARY_clbg.md`, `bench/results/clbg_c_vs_rust_vs_soma_raw.txt`.

Soma never returns wrong answers on integer overflow — the dual-mode
dispatch wrapper falls back to GMP (`rug` crate) on i64 overflow.
Numba's `@njit` silently returns garbage in the same situation;
`examples/overflow_corpus/` exercises this on every commit.

## What makes Soma different

| | LangChain | CrewAI | Kubernetes | Soma |
|---|---|---|---|---|
| Agent termination proof | No | No | No | **Yes (CTL, mechanized in Coq)** |
| Handler-body refinement check | No | No | No | **Compiler extracts decision tree** |
| Memory budget proof | No | No | No | **`soma check` proves peak ≤ budget** |
| Tool calling verified | No | No | No | **Compiler-checked** |
| Distribution model | No | No | YAML | **In the language** |
| Auto-repair | No | No | No | **soma fix** |
| Same code local/cluster | N/A | N/A | No | **Yes** |

## For AI agents

- **Agent guide**: [AGENT.md](AGENT.md)
- **Language reference**: [SOMA_REFERENCE.md](SOMA_REFERENCE.md)
- **LLM reference**: [llms.txt](https://soma-lang.dev/llms.txt)
- **Paper**: [Scale as a Type](https://soma-lang.dev/paper)
- **Examples**: `examples/` — agents, pipelines, pricing engine, chat, 100+ more

## Real applications

The `rebalancer/` directory is a **1400-line systematic rebalancing
tool** for a quantitative investment firm — 5 cells (Alpha signal,
Optimizer, Compliance LLM, Commentary LLM, Portfolio orchestrator),
15-state verified lifecycle, 89 tests across 4 layers, a demo script,
and two cells with mechanically-proven memory bounds. The LLM never
makes investment decisions; all math is in pure deterministic cells.

```
$ soma verify rebalancer/app.cell
State machine 'rebalance': 15 states, initial 'requested'
  ✓ 15 states, 20 transitions
  ✓ no deadlocks
  ✓ liveness: every state can eventually reach a terminal state

$ soma check rebalancer/app.cell
✓ budget proven for cell 'Alpha':     peak ≤ 62.49 MiB ≤ declared 128.00 MiB
✓ budget proven for cell 'Optimizer': peak ≤ 69.89 MiB ≤ declared 128.00 MiB
```

Also: `incident-response/` (SRE on-call with LLM triage) and
`loan-origination/` (consumer lending pipeline with LLM underwriting).
All three have verified state machines with `eventually(closed)`.

## Test Suite

111 language tests + 89 rebalancer tests + 29 rigor tests + 11 Coq
theorems. 100-cell language corpus (`examples/usecases/`), 10 CLBG
challenges, state-explosion bench, backend-equivalence harness, and
a live-LLM integration test against gemma4:26b via ollama.

## License

MIT
