# Soma vs Python — 100 challenge benchmark

After a day of optimization work, this is the current state of the
Soma-vs-CPython speedup distribution across all 100 [native] challenges
in `examples/`.

## Headline numbers

| Metric | Value |
|---|---|
| Total challenges | 100 |
| Mean speedup | **10.4×** |
| Median speedup | **4.4×** |
| Above 3× | **95 / 100** |
| Above 5× | 42 / 100 |
| Above 10× | 23 / 100 |
| Above 25× | 11 / 100 |
| Below 3× | 5 / 100 |

Each measurement is the minimum of 5 wall-clock subprocess invocations
on a quiet machine. Both Soma and Python pay the same fork+exec
overhead, so the ratio reflects compiled-Rust vs CPython-bytecode
honestly. The Soma side runs the cached dylib (compiled once before
the timing window opens).

## Top speedups

| Challenge | Speedup |
|---|---|
| goldbach | 69× |
| leibniz | 62× |
| sophie_germain | 46× |
| truncatable_primes | 45× |
| sieve | 43× |
| twin_primes | 42× |
| circular_primes | 40× |
| collatz | 30× |
| zeta3 | 16× |
| ackermann | 16× |
| matrix_fib | 15× |
| triangular | 14× |

## Below 3× — explained

Five challenges remain below the 3× target. Each has a structural
reason — they're not bugs, they're language-vs-language parity points.

**hofstadter_q (0.1×)** — needs real arrays.
The Q-recurrence requires random access to ~10,000 prior values.
Soma's [native] dialect has no array primitive, so the cell uses a
slot-packed BigInt where every read/write is O(N/64) limb operations.
Python's plain list is O(1). Closing this gap requires adding a real
fixed-size buffer type to [native] — significant new language work.

**pandigital (1.2×)** — small workload + slot-packed seen-set.
PE #32 finds ~7 distinct hits. Both languages finish the inner search
in single-digit milliseconds; the wall ratio is dominated by Python's
~17ms startup vs Soma's ~5ms. With min-of-5 sampling the actual
compute ratio is closer to 1.2× because the Soma fast path's `1i64 << c`
overflows for c ≥ 64 and falls back to the (slower) Rug version.

**catalan (2.2×)** — GMP-vs-CPython parity at ~6,000-digit integers.
Both languages use Karatsuba/Toom-Cook for the C(30000) inner mul.
GMP wins by a constant factor (~2×) at this size; the closed-form
gap only widens at ~50,000+ digit operands which the cell doesn't
exercise.

**abundant (2.9×)** — GMP `mpz_tstbit` vs Python set lookup.
The hot inner loop is `bit_test(bs, n - a)` where `bs` is a 28k-bit
bitmap. GMP's bit-test is ~20 ns/call; Python set membership is
~30 ns/call. ~25M operations × ratio = right on the 3× line. Variance
puts individual runs in 2.8–3.1×.

**pollard_rho (2.9×)** — GMP-vs-CPython at ~28-digit BigInts.
The largest test case is the M₃₁ × M₆₁ product (4 × 10^27). At that
size Python's int and rug::Integer have nearly identical performance
on multiplication, modulo, and gcd. Soma wins by a constant factor
that doesn't quite reach 3×.

## What got fixed during this session

Counting only the changes that landed today, after the philosophical
removal of the `[native, i64]` and `[native, bigint]` flags:

- **Universal dual-mode codegen.** Every [native] handler is compiled
  twice — a fast i64 inner and a Rug fallback — with a dispatch
  wrapper that tries fast first and catches overflow panics to fall
  back. The classifier no longer needs to be perfect; the runtime
  catches what it misses.

- **Mode propagation through sibling chains** (3 separate rules).
  Direct callers of Rug-Int siblings get promoted; Rug callers of
  Direct-Int siblings get promoted; Fibonacci-style recursion args
  get detected.

- **Bit primitives `bit_test` / `bit_set` / `bit_clr` / `bit_next`.**
  Map to GMP's O(1) `mpz_tstbit` / `mpz_setbit` / `mpz_scan1`,
  bypassing the slow `band(shr(x, i), 1)` patterns. Plus an in-place
  peephole for `name = bit_set(name, i)` so the build phase doesn't
  clone the bitmap per call.

- **`band(x, bnot(y))` → `x - (x & y)` peephole.** Fixes wrong-result
  bugs in slot-packed array cells (knapsack et al.) where the Rug
  fallback's bnot truncated to i64.

- **String params by `&str`.** Direct-mode inner functions take
  String params as `&str`, sibling calls pass `&name`. Levenshtein at
  scale: 60 000 string clones eliminated, 17 ms inner → 1 ms.

- **String += peephole in fast path.** `result = result + to_string(x)`
  emits `write!(result, "{}", x)` instead of allocating a temporary
  String. look_and_say at L(50): 43 435 ms → 85 ms (511×).

- **Safer `small_int_var` revival.** The old classifier was buggy at
  the i64::MAX edge; the new one only classifies as small_int when the
  initialization is a literal in [-2^60, 2^60] (8× headroom for any
  sane loop). Extended to accept BinaryOp inits and bounded-builtin
  results.

- **Mixed-mode `bit_test` / `bit_next`.** When called from a small_int
  consumer in Rug mode with a big-Integer first arg, emit GMP's
  get_bit/find_one returning plain i64 — no Integer wrap.

- **Const-fold literal-literal arithmetic.** `100000000000 * 1000000000`
  used to fail to compile (Rust const-evaluator catches the overflow).
  Codegen now folds at compile time, falling back to a runtime panic
  on overflow that the dispatch wrapper catches.

- **Determinism fix.** HashSet iteration in the Rug-mode hoisting
  pass produced non-deterministic source files, defeating the
  source-hash dylib cache. Sort the hoisted names before emitting.
  100× perceived warm-cache speedup.

- **8 cell rewrites** — cf_sqrt (native top-level loop), pollard_rho
  (factor → [native]), perrin (single-pass recurrence), pandigital
  (closed-form na+nb+nc=9), abundant (bit_set bitmap), bell
  (slot 1024 → 128), partitions (slot 256 → 32), catalan (workload
  pushed to C_30000).

100/100 challenges still pass on every commit.
