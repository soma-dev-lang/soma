---
name: stdlib-linalg
description: Quantum-inspired sublinear linear algebra — matrix construction, SVD, regression, sampling.
type: reference
since: V1.5
related: [stdlib-risk, sum-types, budget-proof, verified-pretrade]
---

# Stdlib: linalg

V1.5 added a quant-grade linear-algebra module backed by Tang
2018–2020 quantum-inspired classical algorithms and Bouchaud-Potters
RMT cleaning. Every builtin has a closed-form [[budget-proof]] rule.

The full implementation: `compiler/src/interpreter/builtins/linalg.rs`.

## Matrix constructors

```soma
let A = matrix("1 2 3; 4 5 6")              // MATLAB-style, ';' rows, ws/',' entries
let B = rows(list(1.0, 2.0), list(3.0, 4.0))  // variadic row vectors
let C = cols(list(1.0, 2.0), list(3.0, 4.0))  // variadic columns (transposes)
let D = mat(2, 3, list(1.0, 2.0, 3.0, 4.0, 5.0, 6.0))  // flat list + dimensions
let I = eye(3)                                // 3×3 identity
let Z = zeros(2, 4)
let O = ones(3, 3)
let G = diag(list(1.0, 2.0, 3.0))            // 3×3 diagonal
```

These are universal: Soma had no nested-list literal syntax before
V1.5 (the `list(list(…))` builtin flattens), so the matrix
constructors are useful regardless of whether you use the algorithms.

## Sublinear sampling (Tang)

```soma
let A = to_sampled(dense, map("max_rows", 1000, "max_cols", 50))
// A is a handle: { __sampled__, rows, cols, fro_norm, kind: "sampled" }

let s = sample_row(A)
// → { index, prob, weight, row }  in O(log m)

let isr = importance_sample_rows(A, map(
    "samples", 100, "max_dim", 1000
))
// → { indices, weights, submatrix, fro_norm }

drop_sampled(A)                              // free the BST
```

`to_sampled` precomputes the binary-search-tree of row-norm CDFs from
Tang `1807.04271 §3`. Subsequent sampling is O(log n) per call.

The sampled handle is accepted transparently everywhere a dense matrix
is — see the SVD / regression below.

## Algorithms

```soma
let svd = svd_lowrank(A, map(
    "row_samples", 100,
    "col_samples", 50,
    "rank", 10,
    "max_dim", 1000
))
// Returns { U, S, V, rank, fro_norm, row_indices, col_indices }
// FKV-style sublinear SVD; A may be dense or sampled.

let fit = regress_sgd(A, b, map(
    "eps", 0.01,
    "lambda", 0.1,
    "samples_per_iter", 4,
    "max_iter", 10000,
    "max_dim", 1000
))
// Returns { x, x_last, iterations, eta, spec_norm, fro_norm }
// Stochastic-gradient regression à la Gilyén-Song-Tang 2022.

let cov = clean_covariance(returns, map(
    "method", "rie",         // "rie" | "clip" | "raw"
    "eta", 0.1,
    "center", true,
    "max_assets", 500,
    "max_obs", 1000
))
// Returns { matrix, eigenvalues_clean, eigenvalues_raw, method, q, n_obs, n_assets }
// Bouchaud-Potters RMT cleaning (1610.08104).
```

## Budget bounds

The cost rules read the options map at compile time. With explicit
bounds:

```soma
on optimize(A: List<List<Float>>, b: List<Float>) {
    return regress_sgd(A, b, map(
        "max_iter", 10000,
        "samples_per_iter", 4,
        "max_dim", 32
    ))
}
```

`soma check` proves a closed-form bound on the handler's memory
contribution. See [[budget-proof]] for the formula.

## When to use which

- **`svd_lowrank`** — recover top-k singular vectors of a low-rank
  matrix sublinearly. Useful for factor models, dimensionality
  reduction, recommendation systems.
- **`regress_sgd`** — solve `min ||Ax - b||² + λ||x||²` with
  importance-sampled gradients. Useful for portfolio optimization,
  ridge regression, where the input is too big for dense SVD.
- **`clean_covariance`** — replace sample covariance noise eigenvalues
  with their RMT-shrunk versions. Drop-in for `cov(returns)` whenever
  N (assets) ~ T (observations). Single most impactful out-of-sample
  fix for Markowitz optimization.
- **`importance_sample_rows`** — when you need an explicit sketch of a
  matrix (not full SVD), e.g. for randomized solvers.

## Examples

A factor-model recovery:

```soma
let returns = matrix("...")              // T×N stock returns
let svd = svd_lowrank(returns, map(
    "row_samples", 200,
    "col_samples", 30,
    "rank", 5,
    "max_dim", 500
))
print("top factor: σ_1 = {nth(svd.S, 0)}")
```

Cleaned covariance for portfolio optimization:

```soma
let cov = clean_covariance(returns, map(
    "method", "rie", "max_assets", 500, "max_obs", 1000
))
// cov.matrix is the cleaned N×N matrix — use it in mean-variance opt.
```

Verified pre-trade with regression:

```soma
on optimal_weights(factors: List<List<Float>>, target: List<Float>) {
    let fit = regress_sgd(factors, target, map(
        "eps", 0.05, "lambda", 0.01,
        "max_iter", 5000, "max_dim", 100
    ))
    ensure fit.iterations <= 5000          // sanity check
    fit.x
}
```

See `examples/qi_rebalancer.cell` for the full end-to-end demo with
budget proof.

## Algorithmic basis

References (all in `ewin-tang-papers/WIKI.md`):

- `svd_lowrank` — Tang 1807.04271 (FKV / ModFKV sampling SVD)
- `regress_sgd` — Gilyén-Song-Tang 2009.07268 (SGD + importance
  sampling)
- `to_sampled` / sampling — Tang 1807.04271 §3 (BST data structure)
- `clean_covariance` — Bun-Bouchaud-Potters 2017 (RIE) / Laloux et
  al. 2000 (clipping)

## Edge cases

- All matrix entries are `Float` internally — `Int` inputs are
  promoted. For huge integer matrices, expect precision loss past
  ~10^15.
- `to_sampled` allocates 2× the matrix size (data + CDFs). The budget
  rule accounts for this — pass `max_rows` / `max_cols` to bound.
- The `[sampled]` memory property is declared but no storage backend
  implements it yet. `to_sampled` is the only way to get a sampled
  handle. See [[whats-missing]].
- Interpreter execution of these algorithms is NOT native-compiled
  yet. For large matrices (N > ~500) the interpreter loop becomes the
  bottleneck. Roadmap item: `[native]` codegen integration.

## Related

- [[stdlib-risk]] — VaR, ES, market impact built on top.
- [[budget-proof]] — how the bounds compose into a cell-level budget.
- [[verified-pretrade]] — case study using these builtins.
