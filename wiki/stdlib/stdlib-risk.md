---
name: stdlib-risk
description: Market impact, VaR, expected shortfall — Bouchaud-Potters baseline.
type: reference
since: V1.5
related: [stdlib-linalg, ensure, verified-pretrade, budget-proof]
---

# Stdlib: risk

V1.5 risk-metric builtins. All come from Bouchaud-Potters
*Theory of Financial Risk*; the impact formula is the Tóth et al.
square-root law (`arXiv:1105.1694`).

The full implementation is in `compiler/src/interpreter/builtins/linalg.rs`.

## Market impact (Bouchaud square-root law)

```soma
let imp = impact_sqrt(qty, daily_volume, sigma, map("Y", 1.0))
// → { impact, bps, Y, q_over_v }

// impact = Y · σ · √(Q/V)             (fraction of price)
// bps    = impact · 10_000             (basis points)
```

The constant `Y` defaults to 1.0 — typical for equities. Less liquid
assets have `Y` ≈ 2–5.

**Use this as a precondition.** The canonical Soma idiom:

```soma
on submit(qty: Float, vol: Float, sigma: Float) {
    let imp = impact_sqrt(qty, vol, sigma, map("Y", 1.0))
    ensure imp.bps <= 30.0                   // empirical model as gate
    emit place_order(qty)
}
```

See [[ensure]] and [[verified-pretrade]] for the full pattern.

## Historical VaR

```soma
let var = var_historical(returns, map(
    "alpha", 0.95,                           // confidence level
    "max_obs", 250
))
// → Float: the α-VaR, positive number meaning "loss"
```

`var_historical` is non-parametric — it makes no Gaussian assumption,
just takes the empirical (1−α) quantile of the returns. Robust to fat
tails.

## Expected shortfall (CVaR)

```soma
let es = expected_shortfall_historical(returns, map(
    "alpha", 0.95,
    "max_obs", 250
))
// → Float: mean of returns at or below the α-VaR
```

ES dominates VaR when tails are fat — it captures the *severity* of
left-tail events, not just their frequency.

## Gaussian VaR (for comparison)

```soma
let varg = var_gaussian(returns, map(
    "alpha", 0.95,
    "mu", 0.0,          // optional; inferred from sample otherwise
    "sigma", 0.01
))
// → Float: parametric VaR assuming N(mu, sigma²)
```

Useful only as a baseline. Real markets have fatter tails than
Gaussian; the difference between `var_historical` and `var_gaussian`
quantifies how fat.

The inverse-normal CDF uses Acklam's rational approximation
(|error| < 1.15e-9).

## Quantile

```soma
let q5 = quantile(values, 0.05)              // 5th percentile
let median = quantile(values, 0.5)
```

Linear-interpolated empirical quantile. Building block for the VaR
functions but independently useful.

## Closed-form bounds

The [[budget-proof]] cost rules:

```
impact_sqrt(qty, vol, sigma, opts)                  — O(1) bytes
quantile(values, q)                                 — 8 · max_obs bytes
var_historical(returns, map("max_obs", N))           — 8N + 256 bytes
expected_shortfall_historical(returns, map("max_obs", N)) — 8N + 256
var_gaussian(returns, map("max_obs", N))             — 8N + 256
```

All bounds are read from positional or map-literal arguments at
compile time.

## Examples

A full pre-trade gate:

```soma
on submit(qty: Float, vol: Float, sigma: Float, symbol: String, max_bps: Float, max_var: Float) {
    let imp = impact_sqrt(qty, vol, sigma, map("Y", 1.0))
    ensure imp.bps <= max_bps                // model-grounded impact cap

    let hist = returns_for(symbol)            // 250 daily returns
    let var95 = var_historical(hist, map("alpha", 0.95, "max_obs", 250))
    ensure var95 <= max_var                   // tail-risk cap

    emit place_order(qty)
}
```

Tail-risk reporting:

```soma
on tail_metrics(returns: List<Float>) {
    map(
        "var_95",  var_historical(returns, map("alpha", 0.95, "max_obs", 250)),
        "var_99",  var_historical(returns, map("alpha", 0.99, "max_obs", 250)),
        "es_95",   expected_shortfall_historical(returns, map("alpha", 0.95)),
        "var_gauss_95", var_gaussian(returns, map("alpha", 0.95))
    )
}
```

`var_historical - var_gaussian` is the "fat tail premium" — positive
when historical tails exceed Gaussian expectation. Useful for monitoring
regime changes.

## Why this matters

Before V1.5, Soma had no risk-metric library. Users wrote ad-hoc
`sort` + index-lookup VaR by hand, with no compile-time bounds. The
new builtins:

- **Have known mathematical pedigree.** Bouchaud-Potters Ch. 3 is the
  standard reference for fat-tail-aware risk.
- **Are budget-proven.** Pass `max_obs` and the cost is bounded at
  compile time.
- **Plug into `ensure`.** Turn an empirical risk number into a
  compile-time-typed precondition.

The combination is what makes Soma's "verified pre-trade" claim real.
See [[verified-pretrade]].

## Edge cases

- `var_historical` returns a *positive* number meaning "loss".
  Compare with `>` (worse than threshold), not `<`.
- `var_gaussian` with `mu` / `sigma` not provided infers them from the
  sample. With a short sample these estimates are noisy — pass them
  explicitly when you have better estimates from elsewhere.
- `quantile([], 0.5)` is a type error (empty input). Defend with
  `if len(returns) > 0` if necessary.
- The `expected_shortfall_historical` uses the *strict* left tail
  (returns at or below VaR). If your sample is short, this can be a
  single point.

## Related

- [[stdlib-linalg]] — `clean_covariance` for the second-moment side
  of portfolio risk.
- [[ensure]] — the precondition mechanism that makes these checks
  load-bearing.
- [[verified-pretrade]] — case study: combining impact + VaR + state
  machine.
