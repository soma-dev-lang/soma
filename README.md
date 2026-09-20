# Soma

**The language for programs you can prove — built for the age of AI agents.**

Soma is a declarative language where systems are *cells*: state, contract,
behavior, and lifecycle in one unit. Its center of gravity is verification —
not as an add-on, but as the reason the language exists:

- **State machines are proven.** `soma verify` model-checks every lifecycle:
  reachability, deadlocks, liveness, temporal properties — and proves the
  handler *bodies* implement the machine they're drawn next to.
- **Memory carries invariants.** `invariant balance >= 0` is enforced before
  any write commits. An overdraft isn't a bug to catch; it's unrepresentable.
- **LLMs run inside the cage.** A `cell agent`'s lifecycle is a state machine
  whose terminal states the compiler proves reachable from every state (add an
  `eventually` property to prove that every run reaches one); `set_budget` stops
  further `think()` calls once the budget is spent (the provider round that
  crosses it completes, up to its `max_tokens`; a prompt that alone overruns
  what is left is refused before it is sent); tool calls are
  capability-scoped. The model proposes — the language disposes.
- **Hordes of agents, one ceiling.** `horde(Reviewer.review, docs, map(
  "concurrency", 500, "budget_tokens", 2000000, "on_result", "_store"))` runs a
  `[task]` agent once per input: model calls wait outside the lock, every call
  reserves its worst case against the token ceiling first, the queue survives
  a restart and each result is recorded once. 10 000 mocked 2 s tasks in 41 s.

```soma
cell Ledger {
    memory {
        account: Map<String, Int> [persistent]
        invariant account >= 0 && account <= 1000000   // the wall
    }
    state flow {
        initial: proposed
        proposed -> authorized
        authorized -> paid          // `paid` is provably unreachable
        proposed -> rejected        // without passing `authorized`
    }
}
```

```
$ soma verify app.cell
  ✓ terminal states: [paid, rejected]
  ✓ liveness: every state can eventually reach a terminal state
  ✓ refinement: handler `settle` ⟶ {paid}
  ⚠ invariant account >= 0 && account <= 1000000 — runtime-checked (computed values)
```

Every claim in this README is a command you can run.

## Install

```bash
curl -fsSL https://soma-lang.dev/install.sh | sh
# or from source:
git clone https://github.com/soma-dev-lang/soma && cd soma/compiler
cargo build --release
```

## Sixty seconds

```bash
soma init myapp && cd myapp
soma check app.cell        # static gates: contracts, interpolation, dispatch
soma verify app.cell       # PROVE the state machines + invariants
soma test app.cell         # run `cell test` assertions
soma serve app.cell        # http://localhost:8080
```

## Proof-carrying demo apps

Each is a complete, working application where the safety property is a
theorem, not a code review hope:

| App | The theorem | Try it |
|-----|-------------|--------|
| [`treasury/`](treasury/) | An adversarial LLM with a checkbook **cannot overdraw the account** — model demands $999,999, ledger pays $200 | `soma test treasury/app.cell` |
| [`airlock/`](airlock/) | "Both doors open" (vacuum breach) is **unrepresentable** — no command sequence reaches it | `soma verify airlock/app.cell` |
| [`poker/`](poker/) | A Hold'em server where **chips can't leak** — bots play over HTTP; a cheater's overbet is rejected at the ledger | `python3 poker/bots.py 12` |
| [`elevator/`](elevator/) | SCAN scheduling with a **proven door/motion interlock** — the scheduler can be buggy; the safety can't | `soma run elevator/app.cell run_sim 8` |
| [`hft/`](hft/) | An Avellaneda-Stoikov market maker whose kill switch **provably cannot re-arm** | `soma run hft/app.cell run_stress 5000` |
| [`delivery/`](delivery/) | A food-delivery platform: typed state machines, sum-type payments, verified lifecycle | `soma serve delivery/app.cell` |

## The language, fast

```soma
// vectorized (numpy/MATLAB style)
let M = [1, 0, 0, 1].reshape(2, 2)     // matrices are first-class
let P = A * B                           // matmul; + - elementwise; A/2, A+10 broadcast
let mask = A > 2                        // comparison masks
let v2 = v * v + v                      // vectors: + * / - elementwise

// records, indexing, mutation
let g = Game { bet: 10, board: list(1, 2, 3) }
g.bet = 20
g.board[0] = 99                         // nested lvalues
let x = m["key"] ?? 0                   // index + null-coalescing

// sum types with exhaustive matching (compiler refuses a missing arm)
cell type Pay { variants { Charged { tx: String }  Declined { reason: String }  Cash } }
match result {
    Charged { tx }     -> "paid ({tx})"
    Declined { reason } -> "no: {reason}"
    Cash                -> "cash"
}

// pipelines over records
sales |> filter_by("region", "east") |> agg("product", "qty:sum")

// any builtin is a method (UFCS)
xs.sort()    xs.sum()    m.transpose()    m.det()
```

Three execution backends: a tree-walking interpreter (reference semantics),
a bytecode VM, and `[native]` Rust codegen — measured at parity with
`rustc -O` (16.8 ns/op on the same workload; see the benchmark in the repo
history).

## Built for AI agents — in both directions

**Agents writing Soma:** the toolchain is designed so a model's iteration
loop converges. `soma check` catches undefined interpolation variables,
unknown/ambiguous calls, and invalid invariants *before* runtime; error
messages contain their own fix (`invalid transition: Placed → Delivered.
Valid targets: [Accepted, Cancelled]`); `soma describe --builtins --json`
and `--faces` give exact signatures so nothing is guessed. Start with
[`AGENT_GOTCHAS.md`](AGENT_GOTCHAS.md) — 20 verified wrong→right pairs —
and [`examples/corpus/`](examples/corpus/): **hundreds of complete programs, every
one passing `check` and `test`**, generated as LLM training data.

Everything an agent needs is one command or one fetch away:

```bash
soma docs agent                  # the language summary, embedded in the binary
soma describe --builtins --json  # exact signatures — never guess
soma example invariant state_machine      # verified programs with those features
soma example <id>                # …and the source of one
soma init myapp                  # app.cell + soma.toml + AGENTS.md for the next agent
```

Online, [soma-lang.dev/agents](https://soma-lang.dev/agents) lists the
machine-readable surface: `llms.txt`, `llms-full.txt`, `builtins.json`,
`gotchas.json`, `corpus/index.json` — all generated from the compiler and
the corpus by `tools/build_site.py`, so the site cannot drift.

**Soma running agents:** `cell agent` + `think()` + a state machine =
a lifecycle whose exits are proven reachable, token budgets that stop the next call, capability-scoped
tools, human approval gates, and record / replay of handler inputs
(`--record` / `soma replay`) for audits — the program's own logic replays
exactly; LLM and HTTP calls are made again live, so their answers may differ.

## Packages

A sparse HTTP registry (the Cargo model) lives at `soma-lang.dev/repo`:

```toml
[dependencies]
matrix = "^0.2"        # semver ranges, resolved to the highest match
```

```bash
soma install           # → .soma_env/packages/, commit-pinned in soma.lock
```

A package's API is its cells' `face` sections (`soma describe --faces`),
and its proofs travel with it: `soma test` the installed package re-verifies
its `cell test` assertions on *your* toolchain. The first package,
[`matrix`](packages/matrix/), brings numpy-style linear algebra (inverse,
solve, lstsq, broadcasting helpers) in pure Soma — 43 self-proofs included.

## Docs

| | |
|---|---|
| [SOMA_REFERENCE.md](SOMA_REFERENCE.md) | the language, for agents and humans |
| [SOMA_BUILTINS.md](SOMA_BUILTINS.md) | every builtin — generated from the compiler, can't drift |
| [AGENT_GOTCHAS.md](AGENT_GOTCHAS.md) | verified wrong→right pairs |
| [SOMA_SPEC.md](SOMA_SPEC.md) | machine-readable spec |
| [wiki/](wiki/) | concepts, verification theory, design notes |
| [site/llms.txt](site/llms.txt) | the whole language in one file, for LLM context |

## Honest status

Soma is an experimental language (release: `soma 2.8.1`; see
[the changelog](CHANGELOG.md) for fixes and compatibility notes). The verifier
proves state-machine and invariant properties per cell; cross-cell
composition is statically *linted*, not yet proven. The interpreter is
an AST walker (use `[native]` for hot paths). One known semantic
asymmetry: `+` on non-numeric lists concatenates, on numeric vectors it
adds elementwise — `concat(a, b)` is always explicit concatenation.
The test suite includes hundreds of Rust tests and verified `.cell` programs;
`soma verify` failures are CI-grade errors, not warnings.

## License

[MIT](LICENSE).
