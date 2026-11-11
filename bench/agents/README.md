# Agent-convergence benchmark

How many rounds does a model need to turn a task into a Soma program that
passes `soma check` and its acceptance tests — and how much of what the
program promises ends up **proven** rather than tested?

```bash
python3 bench/agents/make_tasks.py                    # 322 tasks from examples/corpus → tasks.json
python3 bench/agents/run.py --agent reference         # harness self-test: 322 / 322 green in round 1
python3 bench/agents/run.py --agent-cmd 'claude -p' --name claude --tasks 40 --rounds 5 --out results/claude.json
python3 bench/agents/run.py --report results/*.json   # the comparison table
```

## A task

One per corpus program: its scenario (the header comment), the contracts to
implement (`soma describe --faces`: cells, signals, slots) and its
`cell test`, given to the agent as acceptance tests. The corpus program is
the reference solution — it is never shown — so every task is solvable.

## An agent

A command that reads a prompt on stdin and prints the program. Round 1 gets
the language guide the binary ships (`soma docs agent`) and the task; a
failed round gets the task, its own last program, and the diagnostics of
`soma check --json` or `soma test --json`.

## What is measured

| column | meaning |
|---|---|
| `green` | tasks passing check + tests within `--rounds` |
| `check@1` | first answer passes `soma check` |
| `green@1` | first answer passes everything |
| `rounds` | mean rounds to green, over the green tasks |
| `chars` | mean characters read + written to green (a token proxy) |
| `proven` | of the properties `soma verify --attest` states for the green programs, the fraction that is `proven` (the rest: `runtime-checked`, `unproven`) |

The reference solutions score 86 % proven on soma 3.0.0 (2,182 proven, 152
runtime-checked, 206 unproven): that is the ceiling an agent is compared to.

## Limits

- The acceptance tests are given, not hidden: the benchmark measures
  convergence on a stated contract, not specification inference.
- There is no Python / TypeScript arm yet, so it compares models and Soma
  releases with each other, not Soma with other languages.
- No model results are committed: running it costs the runner's own API
  budget. `--agent reference` and `--agent empty` exercise the harness.
