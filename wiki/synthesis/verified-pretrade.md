---
name: verified-pretrade
description: Case study — turning empirical models into compile-checked preconditions.
type: synthesis
since: V1.5
related: [ensure, stdlib-risk, stdlib-linalg, budget-proof, state-machine, manifesto]
---

# Case study: verified pre-trade gates

The clearest demonstration of Soma's unique value proposition: combining
an empirically-grounded model with a compile-time-shaped, runtime-
enforced precondition. The example lives in
`examples/risk_check.cell` and is wired into the production-ish `mft/`
engine.

## The pattern

```soma
on submit_order(qty: Float, daily_volume: Float, sigma: Float, symbol: String) {
    let imp = impact_sqrt(qty, daily_volume, sigma, map("Y", 1.0))
    ensure imp.bps <= 30.0                          // ① Bouchaud square-root law

    let var = var_historical(returns_for(symbol),
                             map("alpha", 0.99, "max_obs", 250))
    ensure var <= 0.05                              // ② Fat-tail VaR

    emit place_order(qty)
}
```

Two preconditions, two published models, one `emit`. The handler
**cannot** reach `place_order` unless both ensures hold.

## What's new about this pattern

Before V1.5, an [[ensure]] could express *arbitrary* conditions, but
the language had no library of empirical models. Users wrote:

```soma
ensure qty < 1000     // magic number, no model
```

V1.5 added [[stdlib-risk]] with the Bouchaud-Potters baselines
(`impact_sqrt`, `var_historical`, `expected_shortfall_historical`,
`var_gaussian`, `clean_covariance`) — each with closed-form
[[budget-proof]] rules and known mathematical pedigree.

Now `ensure` can reference *published, validated* models. The
precondition is grounded in reality, not folklore.

## What the compiler proves

`soma check` on this handler proves:

1. **The handler terminates** — no unbounded loops.
2. **The memory budget** — `impact_sqrt` is O(1), `var_historical`
   is bounded by `max_obs` × 8 bytes. Both contribute to the cell's
   declared budget.
3. **The state-machine transitions are valid** — if the handler
   triggers any `transition(id, X)`, X must be a known state.
4. **The ensure clauses are syntactically clean** — they reference
   valid identifiers and expressions.

What it does NOT prove (would require SMT):

- That the model `impact_sqrt(qty, vol, sigma) <= 30` is **always**
  satisfied for the inputs the caller passes. The check is runtime.

This is the right line: empirical models can't be proven correct by
the compiler, but the *use* of them as preconditions can be made
load-bearing.

## The mft/ integration

V1.5 wired `impact_sqrt` into the `mft/` (medium-frequency trading)
engine, replacing a placeholder linear-impact model:

In `mft/lib/risk.cell`:

```soma
on check_order(symbol: String, qty: String, price: String, side: String, nav: String) {
    // ... position limits, concentration, notional, rate ...

    // Bouchaud square-root impact gate.  Disabled when max_impact_bps == 0.
    let max_bps = to_float(limits.get("max_impact_bps") ?? "0")
    if max_bps > 0.0 {
        let y = to_float(limits.get("impact_Y") ?? "1.0")
        let dv = to_float(limits.get("daily_volume") ?? "1000000")
        let sigma_d = to_float(limits.get("daily_sigma") ?? "0.02")
        let imp = impact_sqrt(to_float(qty), dv, sigma_d, map("Y", y))
        if imp.bps > max_bps {
            return map(
                "allowed", "false",
                "reason", "Bouchaud impact gate: {imp.bps} bps > max {max_bps} bps"
            )
        }
    }
    map("allowed", "true", "reason", "passed all checks")
}
```

In `mft/lib/execution.cell`, the slippage simulation:

```soma
on _fill_price(mid: Float, spread: Float, qty: Float, side: String) {
    let half_spread = spread / 2.0
    let dv = 1000000.0
    let sigma_d = 0.02
    let imp = impact_sqrt(qty, dv, sigma_d, map("Y", 1.0))
    let impact = imp.impact * mid
    if side == "BUY" { return mid + half_spread + impact }
    return mid - half_spread - impact
}
```

Operators turn it on via `setup`:

```
POST /setup {"max_impact_bps": "30", "daily_volume": "5000000", "daily_sigma": "0.02"}
```

The 42 temporal properties of the `mft/` state machine still verify.
The handler-effect summary still proves every order reaches `settled`.
The Bouchaud gate is a *strict addition* to the existing risk machinery.

## Why this matters

Three things compose:

1. **The model is real.** Tóth-Lemperière-Bouchaud (`arXiv:1105.1694`)
   is one of the most-validated empirical regularities in market
   microstructure. Universal across asset classes, brokers, decades.
2. **The check is enforced.** Not a comment; not a test; an
   ensure-clause the runtime enforces. Failure is a structured error
   that propagates via `try { … }`.
3. **The runtime cost is bounded.** `impact_sqrt` is two
   floating-point operations. The [[budget-proof]] reads the
   options map and bounds it at compile time.

This is the kind of guarantee no LangChain / Python-on-EC2 system can
make. The "impact must be under 30bps before we send the order" is a
sentence in compliance documentation. In Soma it's a compile-time-
typed precondition.

## Generalization

The pattern works for any domain where:

- There's an empirical model with a closed-form computation.
- The model takes inputs available at handler call time.
- A threshold can be expressed as a Boolean predicate.

Examples that fit:

- **Trading**: impact, VaR, ES, drawdown, leverage.
- **Lending**: PD (probability of default), LGD (loss given default),
  exposure-at-default.
- **Healthcare**: drug interaction scores, lab-value bounds.
- **Recommendations**: confidence bounds (Hoeffding, Bayesian
  intervals).
- **Quality**: SLA thresholds, latency budgets.

In each case: load the model into the builtin (or call into a
delegate cell that computes it), guard the action with `ensure`,
let the compiler enforce the rest.

## The honest cut

The pattern doesn't prevent:

- A bad model. `impact_sqrt` is empirically validated; that's a
  claim from the literature, not a Coq theorem.
- A bad threshold. `ensure imp.bps <= 30` might be too lax for some
  regimes.
- A bad input. If `qty` is wrong, the ensure can pass spuriously.

What it prevents:

- Forgetting to check.
- Drift between "documented compliance gate" and "actual code path."
- Silent failure mode where the check is logged but doesn't block.

For the kind of regulated environments where these matter (finance,
healthcare, anything safety-critical), preventing those three is the
ballgame.

## Related

- [[ensure]] — the runtime mechanism.
- [[stdlib-risk]] — the model library.
- [[stdlib-linalg]] — broader quantitative primitives.
- [[budget-proof]] — closed-form cost bounds.
- [[manifesto]] — why Soma exists.
