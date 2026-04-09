# Soma Rebalancer

A verified systematic rebalancing tool for a quantitative investment firm.
Built in [Soma](https://soma-lang.dev). Every rebalance run is provably
guaranteed to terminate; the compiler statically extracts the entire
decision tree of the hot path with path conditions an auditor can read
line by line.

```
soma serve rebalancer/app.cell -p 8080
```

## What this is

A real-shape pipeline you could plug into a quant firm's existing
infrastructure as the workflow brain:

```
   POST /strategies + /history + /prices       (you supply data)
                       │
                       ▼
              POST /rebalance
                       │
       ┌───────────────┼───────────────┐
       ▼               ▼               ▼
    Alpha          Optimizer       Compliance       (LLM, pre-trade
    (pure)         (pure)          (LLM reviewer)    not decider)
       │               │               │
       └───────┬───────┘               │
               ▼                       │
         trade list ──────────────────►│
                                       │
              ┌────────────────────────┘
              ▼
      flagged (human gate)
              │              POST /approve
              ▼              POST /cancel
          approved
              │              POST /execute
              ▼
          executed
              │              POST /reconcile
              ▼
       reconciled (Commentary LLM)
              │
              ▼
           closed
```

The key architectural decision: **the LLM never makes investment
decisions**. It does pre-trade compliance review (catches policy
violations) and post-trade commentary (PM morning report). All math
— signal, optimization, constraints, fills — is in pure deterministic
cells. This is how you defend the system to a regulator.

## Cell layout

| Cell | Type | Job | LLM? |
|---|---|---|---|
| `Alpha` (`lib/alpha.cell`) | pure | Momentum + inverse-vol signal computation. Reads price histories, returns target weights. | No |
| `Optimizer` (`lib/optimizer.cell`) | pure | Per-name delta with position cap, turnover cap, and proportional cash-floor scaling. | No |
| `Compliance` (`app.cell`) | agent | Pre-trade reviewer. Reads the trade list and the firm's free-text policy doc, returns `APPROVE / FLAG / BLOCK`. | Yes |
| `Commentary` (`app.cell`) | agent | Post-trade narrator for the PM morning report. | Yes |
| `Portfolio` (`app.cell`) | orchestrator | Owns memory, the state machine, the audit trail, and the HTTP API. The only cell that calls `transition()`. | No |

## State machine

`rebalance` — 15 states, 20 transitions, DAG, single terminal `closed`:

```
requested ─► signal_pending ─► optimizing ─► optimized ─► compliance_review ─► approved ─► executing ─► executed ─► reconciling ─► reconciled ─► closed
                            └► failed                                       ├► flagged ──┴► cancelled                            └► failed
                                                                            └► blocked
```

`soma verify rebalancer/app.cell` proves:

- 15/15 states reachable from `requested`
- terminal set = `{closed}`
- no deadlocks
- liveness: every state can reach a terminal
- 16/16 user-defined temporal properties (every state eventually closes)

The refinement checker prints the exact decision tree of the
`rebalance` handler with the **conditions extracted from the source**:

```
rebalance ⟶ {
    signal_pending,
    failed   [if alpha_cfg != () ∧ alpha_result.error != ()],
    closed   [if alpha_cfg != () ∧ alpha_result.error != ()],
    optimizing,
    failed   [if opt.error != ()],
    closed   [if opt.error != ()],
    optimized,
    compliance_review,
    blocked  [if verdict == "BLOCK"],
    closed   [if verdict == "BLOCK"],
    approved [if verdict == "APPROVE"],
    flagged
}
```

This is a machine-checked specification of what the hot path can do.
An auditor reads this and confirms: the only way to short-circuit
to `failed` is an alpha or optimizer error; the only way to BLOCK is
a compliance verdict of BLOCK; auto-APPROVE requires verdict APPROVE;
everything else flows through to `flagged` for human review.

## Verified properties

```
$ soma verify rebalancer/app.cell

State machine 'rebalance': 15 states, initial 'requested'
  ✓ 15 states, 20 transitions
  ✓ all states reachable from 'requested'
  ✓ terminal states: [closed]
  ✓ no deadlocks
  ✓ liveness: every state can eventually reach a terminal state
  ✓ refinement: handler `rebalance` ⟶ {...12-element decision tree...}
  ✓ refinement: handler `approve`   ⟶ {approved}
  ✓ refinement: handler `cancel`    ⟶ {cancelled, closed}
  ✓ refinement: handler `execute`   ⟶ {executing, executed}
  ✓ refinement: handler `reconcile` ⟶ {reconciling, reconciled, closed}

10 passed, 6 warnings, 0 failures

Temporal: 16 passed, 0 failed
```

## Running the tests

The repo ships **89 tests across 4 layers**, all green:

```
$ rebalancer/bin/test_all.sh           # ~5s, no LLM required
$ rebalancer/bin/test_all.sh --live    # ~80s, requires ollama + gemma4:26b
```

| Suite | Tests | What it covers | Speed |
|---|---|---|---|
| **Static** | 16 properties | `soma check` + `soma verify` — state machine, liveness, refinement | <1s |
| **Unit** | 25 (10 alpha + 15 optimizer) | Pure-cell math: momentum top-K, inverse-vol normalization, position cap, turnover cap, cash floor, no-shorting clamps | <1s |
| **CRUD** | 17 | HTTP CRUD on `/strategies`, `/positions`, `/prices`, `/history`, `/policy`, `/portfolio`. Asserts NAV and weight arithmetic against hand-computed values. | ~3s |
| **E2E (mock)** | 40 | Full lifecycle through every state machine path: APPROVE / BLOCK / FLAG→approve / FLAG→cancel. Asserts on the audit trail and verifies negative cases (approve/execute on closed runs are rejected). Uses `SOMA_LLM_MOCK` for determinism. | ~3s |
| **Live LLM** | 7 | Same lifecycle against real `gemma4:26b` via ollama. Proves the HTTP wiring to the model and the verdict parser handle real model output. Verdict varies between runs (model is non-deterministic). | ~80s |

The unit tests live in `rebalancer/tests/test_alpha.cell` and
`rebalancer/tests/test_optimizer.cell` as Soma `cell test` blocks.
Run individually:

```
soma test rebalancer/tests/test_alpha.cell
soma test rebalancer/tests/test_optimizer.cell
```

The integration tests are in `rebalancer/bin/test_*.sh` and exercise
the running server with `curl + jq`.

## The math the optimizer actually does

Three deterministic passes:

1. **Per-name delta with position cap.** For each symbol in the
   union of (currently held, target list), compute
   `delta_w = capped_target − current_weight`, convert to a share
   delta via `round(delta_$ / price)`, clamp the resulting position
   to non-negative when shorting is disabled.

2. **Turnover cap.** Sum gross notional. If gross > `max_turnover ×
   NAV`, scale every trade quantity by `max_turnover × NAV / gross`
   and re-round. Each scaled trade gets `scaled: "true"` for audit.

3. **Cash floor (proportional scale).** Compute resulting cash after
   the trades. If it would drop below `cash_floor × NAV`, scale all
   BUY notional down proportionally so the floor is met exactly:
   `scale = 1 − (need / total_buys)`, then `floor()` each share
   quantity (never `round()` — `floor` guarantees the floor is
   strictly satisfied even with discrete share rounding).

Each scaled trade is flagged `cash_floored: "true"` so a downstream
viewer can see why it's smaller than the nominal target. SELLs are
never touched by the cash floor pass — they only increase cash.

CASH is tracked as a special position (symbol `"CASH"`, price 1) and
the optimizer explicitly **never** produces a trade for it. There's a
unit test for that adversarial case (`test_cash_never_traded`).

## Alpha methods

### Momentum (top-K equal-weight)

```
alpha: { method: "momentum", universe: [...], top_k: "5", lookback: "20" }
```

For each symbol in the universe, compute total return over the
lookback window: `(P[end] − P[end−lookback]) / P[end−lookback]`.
Select the top-K by return (repeated max selection — O(N·K), fine
for the K values quants actually use). Equal-weight the selected
names at `1/K`. Symbols with insufficient history or zero/negative
prices are excluded.

### Inverse volatility

```
alpha: { method: "inverse_vol", universe: [...], lookback: "20" }
```

For each symbol, compute simple returns over the lookback window,
take the population stdev (`σ = sqrt(mean((r − mean_r)²))`), and
weight ∝ `1/σ` normalized to sum to 1.0. Symbols with zero vol
(constant prices) are excluded.

## HTTP API

```
POST /strategies      {id, name, targets:{sym:w}, alpha:{method,universe,top_k,lookback},
                       max_position, max_turnover, cash_floor, allow_shorting}
POST /positions       {symbol, strategy, qty}
POST /prices          {prices: {sym: price, ...}}
POST /history         {symbol, prices: [p1, p2, ...]}
POST /policy          {key, value}                # e.g. compliance_doc

POST /rebalance       {strategy_id}               # the hot path
POST /approve         {run_id}                    # gate flagged runs
POST /cancel          {run_id, reason}
POST /execute         {run_id}                    # sim fills against marks
POST /reconcile       {run_id}                    # post-trade commentary + close

GET  /portfolio
GET  /strategies            GET /strategy/{id}
GET  /history/{symbol}
GET  /runs                  GET /run/{id}
GET  /trades/{run_id}
GET  /audit/{run_id}                              # full event log per run
GET  /snapshot/{run_id}                           # pre-trade weight snapshot
GET  /policy
```

## End-to-end demo

```
$ rebalancer/bin/demo.sh           # mocked LLM, fast
$ rebalancer/bin/demo.sh --live    # real ollama / gemma4:26b
```

What the demo does:

1. Defines a `FAANG+ Momentum Top-3` strategy across 7 names with
   alpha config (top_k=3, lookback=20).
2. Seeds $5M cash, no positions.
3. Marks to current prices.
4. Seeds 30 days of made-up price history per name (NVDA strongest
   momentum, META second, AAPL third, AMZN flat, TSLA worst).
5. Sets a free-text compliance policy.
6. Runs the full hot path: `POST /rebalance`.
7. Reads back the snapshot, the trade list, the run, and the audit.
8. Approves (manual gate if flagged, no-op if auto-approved),
   executes against the current marks, reconciles, prints commentary,
   and prints the final portfolio.

Sample output (mocked LLM, momentum top-3 picks NVDA + META + AAPL,
each at ~31.67% of NAV, cash floored at 5.02% which is strictly above
the 5% floor):

```
── 12. GET /portfolio — post-trade state ─────────────────────
[
  {
    "strategy_id": "faang_momo",
    "nav": "5000000.0",
    "weights": [
      { "symbol": "AAPL", "qty": "8334.0", "weight": "0.316692" },
      { "symbol": "CASH", "qty": "251120.0", "weight": "0.050224" },
      { "symbol": "META", "qty": "3104.0", "weight": "0.316608" },
      { "symbol": "NVDA", "qty": "1341.0", "weight": "0.316476" }
    ]
  }
]
```

## What this is NOT (yet)

Honest scope statement:

| Concern | Status | Where it would slot in |
|---|---|---|
| Alpha model / target generation | **Out** — targets come from `lib/alpha.cell` (momentum, inverse-vol) or are POSTed as static targets | More methods are pure-additive to `Alpha` |
| Real broker connectivity (OMS/EMS) | **Out** — execution is simulated against the marks you POST | Replace `Portfolio.execute()` body with `http_post` to the broker; introduce `broker_pending → broker_filled` substates |
| Tax-lot accounting (FIFO/LIFO/HIFO) | **Out** — single average cost | New `TaxLots` cell called from `execute()`; affects no state machine work |
| Sector/factor/country exposure caps | **Out** — only per-name caps today | Optimizer extends to take a `groups` parameter; pure addition |
| Wash-sale and PDT compliance rules | **Out** — only the LLM compliance review exists | Add a deterministic `WashSaleChecker` cell between `optimized` and `compliance_review`; introduce `wash_blocked` state |
| Multi-strategy NAV reconciliation across funds | **Out** | New top-level cell `Fund` owning multiple `Portfolio` instances |
| Pre-rebalance liquidity check (ADV %) | **Out** | New `Liquidity` cell called from optimizer |
| Production-grade money handling (Int basis points) | **Partial** — uses Float throughout; safe-rounded but not bit-exact | Mechanical rewrite to Int basis points; doesn't change architecture |

Everything in that list is a *new cell* or a *new state*, not a
rewrite. The orchestrator skeleton and the state-machine verification
keep working as you add features — that's the fractal-cells payoff.

## Files

```
rebalancer/
├── app.cell                    # orchestrator + Compliance + Commentary
├── soma.toml                   # verify properties + LLM config
├── lib/
│   ├── alpha.cell              # pure signal cell (momentum, inverse_vol)
│   └── optimizer.cell          # pure constraint engine
├── tests/
│   ├── test_alpha.cell         # 10 unit tests
│   ├── test_optimizer.cell     # 15 unit tests
│   └── lib → ../lib            # symlink so `use lib::alpha` resolves
├── bin/
│   ├── test_unit.sh            # runs both unit suites
│   ├── test_crud.sh            # HTTP CRUD smoke (no LLM)
│   ├── test_e2e.sh             # full lifecycle, mocked LLM
│   ├── test_live.sh            # full lifecycle, real ollama
│   ├── test_all.sh             # everything
│   └── demo.sh                 # end-to-end realistic demo
└── README.md                   # you are here
```

Total: **~1400 lines of Soma + ~1100 lines of test/script bash**.

## License

Same as the surrounding Soma repo.
