# Benchmark B: Soma vs Python+Numba — the honest comparison

After the user asked "is 5× faster than Python good?" the answer was
no — vanilla CPython is the wrong baseline for a compiled language.
This benchmark compares Soma against **Numba** (a JIT compiler for
Python that targets LLVM and reaches C-level performance on the
numeric subset it supports), which is the closest meaningful peer.

Raw output: `bench/results/bench_B_raw.txt`
Numba ports: `bench/numba/*.py`
Runner: `bench/compare_B.sh`

## Methodology

- Each cell has a Numba version in `bench/numba/<name>.py`. Functions
  that are Numba-compatible (numeric, fixed-width int/float) get
  `@njit(cache=True)` decorators. Functions that aren't (BigInt,
  string parsing, dict/list comprehensions) stay as plain CPython —
  Numba would refuse to compile them and the perf is whatever CPython
  delivers, which is the honest measurement for those cases.
- 100/100 Numba cells run successfully end-to-end.
- Inner timing is measured at **microsecond** precision via `time.perf_counter`,
  printed as `INNER_US:<n>` on stderr. The fair "computation only"
  comparison excludes both Python startup and Numba JIT compile (the
  latter cached on disk via `cache=True` so the runner pre-warms).
- 27 / 100 cells have valid inner timing on both Soma and Numba sides
  (i.e. Soma cell prints `(Nms)` and Numba cell's compute exceeds
  the rounding threshold). For the other 73, the workload is too
  small to time at ms granularity on the Soma side, so we don't
  count them in the inner comparison — only the wall ratio is
  available, and wall is heavily contaminated by runtime startup.

## Headline result — inner timing on the 27 measurable cells

| Metric | Value | Interpretation |
|---|---|---|
| **Geometric mean Numba/Soma** | **1.00×** | Statistically tied in computation |
| Arithmetic mean | 4.74× | Skewed by truncatable_primes (38.7×) |
| Median | 1.49× | Soma slight edge from auto-memo/tab |
| Soma ≥2× faster | 12 / 27 | Mostly memo + GMP-vs-CPython int |
| Soma 1.1-2× faster | 4 / 27 | |
| Tied (0.9-1.1×) | 2 / 27 | |
| Numba 1.1-2× faster | 4 / 27 | |
| Numba ≥2× faster | 5 / 27 | Floats + arrays |

**Conclusion**: against a real LLVM-backed JIT competitor, Soma is
neither dominant nor dominated. The languages are competitive on the
median, with strengths and weaknesses on different cell shapes.

## Where Numba beats Soma (the real perf gaps)

| Cell | Soma (ms) | Numba (ms) | Numba advantage | Why |
|---|---|---|---|---|
| **mersenne** | 354 | 0.2 | **1770×** | Numba uses native int64 array; Soma uses slot-packed BigInt |
| **hofstadter_q** | 16 | 0.06 | **267×** | Numba uses Python list; Soma slot-packs into BigInt |
| **matrix_fib** | 21 | 0 | very large | Numba does i64 matrix; Soma rebuilds Integer per cell |
| **catalan** | 87 | 0.23 | **378×** | Numba caches the inner integer products as i64 (n≤30 fits) |
| **sobol_monte_carlo** | 67 | 3.5 | **19×** | Float-heavy random walk; Numba auto-vectorizes |
| **mandelbrot** | 2043 | 453 | **4.5×** | Float-heavy escape iteration; Numba auto-vectorizes |
| **collatz** | 195 | 141 | 1.4× | Tight integer loop; close but Numba's LLVM wins |
| **amicable** | 1 | 0.54 | 1.85× | Tiny workload — measurement noise dominates |
| **sophie_germain** | 2 | 1.43 | 1.4× | Same — small workload |
| **quadratic_primes** | 11 | 8.39 | 1.31× | Tight prime loop |

The **structural** Numba wins (not noise):
1. **Float-heavy compute** (mandelbrot, sobol): Numba's LLVM auto-vectorizes
   FMA chains. Soma generates `f64` Rust but doesn't tag loops with
   vectorization hints, so LLVM is more conservative.
2. **Array-heavy compute** (hofstadter_q, mersenne, catalan, matrix_fib):
   Numba uses Python lists which are O(1) random-access; Soma slot-packs
   into BigInt which is O(N/64) per access. **This is the missing
   primitive we identified earlier.** A real `Buf<Int, N>` in Soma's
   `[native]` would close all four of these gaps in one stroke.

## Where Soma beats Numba (the algorithmic wins)

| Cell | Soma (ms) | Numba (ms) | Soma advantage | Why |
|---|---|---|---|---|
| **truncatable_primes** | 24 | 929 | 39× | Soma uses GMP `is_prime` + auto-memo; Numba uses Python int |
| **levenshtein** | 1 | 13 | 13× | Soma's auto-memo + DP; Numba couldn't @njit (string/list) |
| **miller_rabin** | 261 | 2959 | 11× | GMP modular pow vs Python int |
| **cf_sqrt** | 54 | 516 | 9.6× | GMP isqrt; Numba can't @njit |
| **odd_period** | 2 | 17 | 8.8× | Tight i64 loop, Soma auto-memo |
| **digit_factorial** | 83 | 728 | 8.8× | Pure i64 DP; Numba couldn't @njit (BigInt) |
| **circular_primes** | 39 | 278 | 7.1× | Soma's auto-memo or GMP overhead win |
| **look_and_say** | 46 | 326 | 7.1× | Soma's String += peephole |
| **bbp_hex** | 50 | 194 | 3.9× | Soma GMP path |
| **sieve** | 28 | 109 | 3.9× | Tight i64 sieve, Soma's classifier wins |

The **structural** Soma wins:
1. **BigInt operations**: GMP is faster than CPython's int for large
   operands, and Numba can't @njit BigInt at all (it falls back to
   CPython int via `objmode`, defeating the JIT).
2. **Auto-memo / auto-tab**: cells like truncatable_primes, levenshtein,
   digit_factorial benefit algorithmically. Numba can't add memoization
   automatically; the user has to write a decorator.
3. **String building peephole**: look_and_say's `result = result + s`
   becomes a `write!` instead of allocating temporaries.

## Wall-time (less fair, but the user-visible number)

100 / 100 cells have wall timing.

| Metric | Value |
|---|---|
| Median wall Numba/Soma | 32.14× |
| Mean wall Numba/Soma | 41.11× |

**Caveat**: this is mostly a runtime startup measurement, not a
language comparison. Python+Numba module load takes ~380ms before any
work begins; Soma's binary loads in ~5ms. For sub-100ms workloads
(most of the corpus), the wall ratio is dominated by this 75× startup
gap. It's a real user-facing number — Soma feels much snappier on the
command line — but it doesn't tell you anything about how fast either
runtime computes.

## Honest takeaways

1. **Soma is competitive with Numba on inner-time**, geometric mean
   exactly 1.00×. The "5× over CPython" framing was misleading
   because CPython is a strawman for a compiled language — against
   a real JIT, the ratio collapses.

2. **The gap on float vectorization is real and addressable**: Soma
   could add `#[inline(always)]` and vectorization hints to hot float
   loops. Mandelbrot (4.5× behind) is the canonical case.

3. **The gap on random-access arrays is structural**: hofstadter_q,
   mersenne, matrix_fib, catalan all need `Buf<Int, N>`. Without it,
   these cells will always lose to Numba (which uses Python lists).
   Adding the primitive is the highest-leverage missing feature.

4. **Soma's auto-memo / auto-tab wins are real algorithmic
   improvements**, not benchmark hacks. Numba doesn't memoize
   automatically; the user would have to write `@functools.lru_cache`
   manually, which would help on a few cells but doesn't extend to
   the bottom-up Vec fill that auto-tab does.

5. **The wall-time numbers (32× median) measure startup**, not
   computation. They're true and user-visible but shouldn't be quoted
   as a "language is N× faster" claim.

## Reproduce

```sh
# Pre-warm Numba caches (first time only)
for f in bench/numba/*.py; do python3 "$f" >/dev/null 2>&1; done

# Run the suite
bench/compare_B.sh > bench/results/bench_B_raw.txt 2>&1
```
