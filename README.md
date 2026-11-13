# Soma

**The language for programs you can prove — built for the age of AI agents.**

*The goal: let programming agents design large applications in a solid manner.*

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
| [`treasury/`](treasury/) | An adversarial LLM with a checkbook **cannot talk a payment up** — the model answers $999,999 on a $200 invoice, the ledger moves $200 | `soma test treasury/app.cell` |
| [`airlock/`](airlock/) | "Both doors open" (vacuum breach) is **not a state the machine has** — the breach command is refused, never handled | `soma run airlock/app.cell demo` |
| [`poker/`](poker/) | A Hold'em server where **chips can't leak** — bots play over HTTP; a cheater's overbet is rejected at the ledger | `soma serve poker/app.cell -p 8080`, then `python3 poker/bots.py 12` |
| [`elevator/`](elevator/) | SCAN scheduling with a **proven door/motion interlock** — the scheduler can be buggy; the safety can't | `soma run elevator/app.cell run_sim 8` |
| [`hft/`](hft/) | An Avellaneda-Stoikov market maker whose kill switch **provably cannot re-arm** | `soma run hft/app.cell run_stress 5000` |
| [`delivery/`](delivery/) | A food-delivery platform: typed state machines, sum-type payments, verified lifecycle | `soma serve delivery/app.cell` |
| [`marketplace/`](marketplace/) | Designed **skeleton-first**: the whole design is one file with no body; only `Wallets` can write money and only `Escrow` can call it — read off the frames, enforced on every body | `soma skeleton marketplace/app.cell` |

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

Two execution backends: a tree-walking interpreter (the reference semantics)
and `[native]` Rust codegen for hot handlers (a native compile failure aborts
the run — there is no silent fall-back to the interpreter). `tools/diff_fuzz.py`
generates handlers and compares the two (108,000 comparisons, no differing
answer, on 3.0.0). An experimental bytecode VM is in the tree and not reachable
from the CLI.
The benchmark harness lives in [`bench/`](bench/), with C and Rust reference
implementations to measure against.

## Built for AI agents — in both directions

**Agents writing Soma:** the toolchain is designed so a model's iteration
loop converges. `soma check` catches undefined interpolation variables,
unknown/ambiguous calls, and invalid invariants *before* runtime; error
messages contain their own fix (`invalid transition: Placed → Delivered.
Valid targets: [Accepted, Cancelled]`); `soma describe --builtins --json`
and `--faces` give exact signatures so nothing is guessed. Start with
[`AGENT_GOTCHAS.md`](AGENT_GOTCHAS.md) — verified wrong→right pairs —
and [`examples/corpus/`](examples/corpus/): **hundreds of complete programs, every
one passing `check` and `test`**, generated as LLM training data.

**Large programs, skeleton first.** An agent designs the structure before
any body exists — a `skeleton` holds memory, lifecycles and, for each
signal, its *frame* (what its body may read, write, call, and which edges
it may take); the bodies come later in `flesh` blocks and `soma check`
refuses one that leaves its frame. So the design fits in a few screens,
is proven before it is filled, and cannot be changed in passing:

```bash
soma skeleton app.cell                 # the whole map — 9–20% of the source on the demo apps
soma holes    app.cell                 # the signals with no body yet, in the order to write them
soma skeleton app.cell Orders.place    # the brief for one signal: all an agent needs in context
soma skeleton app.cell --freeze        # from here on, the structure only changes on purpose
```
See "Skeleton first" in [`site/llms.txt`](site/llms.txt) and [`SKELETON_DESIGN.md`](SKELETON_DESIGN.md).

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
| [docs/robotics/ROBUSTNESS.md](docs/robotics/ROBUSTNESS.md) | 100 robotics robustness scenarios, reproduction and limits |
| [wiki/](wiki/) | concepts, verification theory, design notes |
| [site/llms.txt](site/llms.txt) | the whole language in one file, for LLM context |

## Honest status

Soma is an experimental language (release: `soma 3.1.0`; see
[the changelog](CHANGELOG.md) for what it adds and what it leaves out). The
verifier proves state-machine and invariant properties per cell; a rule
between two cells is proven when it is stated — a face contract
(`requires` / `ensures`) or a `[verify.before."Cell.state"]` property. The interpreter is
an AST walker (use `[native]` for hot paths). One known semantic
asymmetry: `+` on non-numeric lists concatenates, on numeric vectors it
adds elementwise — `concat(a, b)` is always explicit concatenation.
The test suite includes hundreds of Rust tests and verified `.cell` programs;
`soma verify` failures are CI-grade errors, not warnings.

## License

[MIT](LICENSE).
