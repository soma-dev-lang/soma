# Soma vs Python — 100 challenge benchmark (post auto-iter / auto-tab / auto-CSE)

After implementing the three auto-optimizations on top of auto-memo,
this is the current state of the Soma-vs-CPython speedup distribution
across all 100 `[native]` challenges in `examples/`.

The raw output is in `bench/results/post_auto_opts.txt`. Each measurement
is the minimum of 5 wall-clock subprocess invocations on a quiet machine
via `bench/compare.sh`.

## Headline numbers

| Metric | Before auto-opts | After auto-opts | Δ |
|---|---|---|---|
| Total challenges | 100 | 100 | — |
| Mean speedup | 10.4× | **14.9×** | **+43%** |
| Median speedup | 4.4× | **7.3×** | **+66%** |
| Above 3× | 95 / 100 | 95 / 100 | — |
| Above 5× | 42 / 100 | **81 / 100** | **+93%** |
| Above 10× | 23 / 100 | **28 / 100** | +22% |
| Above 25× | 11 / 100 | **15 / 100** | +36% |
| Above 50× | 0 / 100 | **5 / 100** | new |
| Above 100× | 0 / 100 | **1 / 100** | new |
| Max | 69× (goldbach) | **249× (ackermann)** | **+261%** |

The biggest single jump is **ackermann: 16× → 249×**, driven by
auto-memo of the nested-call recurrence that auto-tab can't reach but
the HashMap-memo wrapper handles cleanly.

## Top 15

| Challenge | Speedup |
|---|---|
| ackermann | **249×** |
| goldbach | 70× |
| leibniz | 61× |
| sophie_germain | 56× |
| quadratic_primes | 51× |
| twin_primes | 44× |
| circular_primes | 44× |
| sieve | 41× |
| nqueens | 40× |
| levenshtein | 39× |
| truncatable_primes | 39× |
| perrin | 31× |
| collatz | 31× |
| champernowne | 25× |
| random_walk | 25× |

## What each auto-optimization fires on (real cells)

- **auto-iteration** (`bench/auto_opts_study.md`, candidate 1): pure
  tail self-call rewritten as a `while true` rebinding loop. Real-cell
  fires: `euclidean_alg::gcd_basic`, `ext_gcd::egcd_g`. Eliminates
  stack-overflow risk and call overhead for tail-recursive helpers.

- **auto-tabulation** (`bench/auto_opts_study.md`, candidate 2): when
  the body is "tab-simple" (guards + one linear-combo return) and every
  self-call has position-preserving strict-shrink args, the HashMap memo
  wrapper is replaced with a bottom-up `Vec` fill. Bypasses to memo for
  inputs above the per-arity cap (1024 dim, 1M total cells). Real
  in-corpus fires: m01_fib, m02_tribonacci, m03_lcs2d, m04_edit,
  m05_binomial_rec, m06_delannoy, m07_motzkin, m09_lucas, m20_branches.
  No real-cell fires in the 100-challenge suite (the recursive
  multi-call patterns there have derived args), but every memo
  candidate among the corpus is now Vec-backed.

- **auto-CSE** (`bench/auto_opts_study.md`, candidate 3): repeated
  pure-call expressions in the same straight-line block hoisted to a
  fresh `let` JUST BEFORE the first occurrence (critical: hoisting to
  body start would lift the call above its guards and break handlers
  like m15_hanoi). Real-cell fire: `mandelbrot::count_inside`
  (`to_float(grid_n)` × 2 in straight-line lets).

## Below 3× — unchanged structural reasons

The 5 challenges still below 3× are the same ones from the previous
benchmark — they're language-vs-language parity points, not optimizer
gaps:

- **hofstadter_q (0.1×)** — needs a real array primitive; the
  slot-packed BigInt loses to Python's `list` on random access.
- **pandigital (0.9×)** — small workload (~7 hits) dominated by
  process startup overhead; variance puts individual runs in 0.9–1.4×.
- **dice (2.0×)** — sub-millisecond compute; startup overhead
  dominates ratio.
- **catalan (2.5×)** — GMP-vs-CPython parity at ~6,000-digit
  integers; both languages use Karatsuba/Toom-Cook.
- **pollard_rho (2.9×)** — GMP-vs-CPython parity at ~28-digit
  factor inputs; nearly identical multiplication / mod / gcd cost.

## Validation

- 48/48 stress corpora pass (memo 20 + iter 10 + tab 11 + cse 7).
- 100/100 challenges pass.
- Detection rules verified by Python pre-implementation
  (`bench/check_*_corpus.py`) — zero false positives.

## Reproduce

```sh
bash bench/compare.sh > bench/results/post_auto_opts.txt 2>&1
```
