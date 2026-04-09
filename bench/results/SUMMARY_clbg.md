# Computer Language Benchmarks Game — Soma vs Rust

The CLBG is the canonical cross-language perf comparison. Every
mainstream compiled language publishes results against it. This
document is Soma's first pass at the 10 standard challenges,
benched against hand-written sequential Rust on the same machine
(Apple M-series, sequential, target-cpu=native, thin LTO).

## Headline result

**On the 5 numeric challenges Soma can fully implement, geomean
Soma/Rust = ~1.16×** — Soma is within 16% of hand-written Rust on
the median challenge, and on pidigits (BigInt-heavy) **Soma is
actually faster than Rust**.

| # | Challenge | Rust ref | Soma | Ratio | Notes |
|---|---|---|---|---|---|
| 1 | **n-body** 50M | 912ms | 1176ms | 1.29× | Float-heavy, hand-unrolled 5-body |
| 2 | **spectral-norm** 5500 | 744ms | 875ms | 1.18× | Float, uses new `Buf<Float>` primitive |
| 3 | **pidigits** 10K | 541ms | 467ms | **0.86× ⬇** | **Soma faster** — swap-on-assign GMP win |
| 4 | **mandelbrot** 16000² | 3638ms | 3722ms | 1.02× | Essentially tied |
| 5 | **fannkuch-redux** 12 | 21.5s | 33.7s | 1.57× | i64-heavy permutation enum |
| 6 | fasta 25M | (skipped) | stub | — | needs StringBuf primitive |
| 7 | k-nucleotide | (skipped) | stub k=4 | — | needs HashMap primitive |
| 8 | regex-redux | (skipped) | stub | — | needs regex builtin |
| 9 | binary-trees 21 | (skipped) | stub | — | needs heap structs OR arena |
| 10 | reverse-complement | (skipped) | stub | — | needs file I/O |

The 5 numeric challenges are real apples-to-apples — same algorithm,
same machine, same compiler backend (LLVM via cargo). The 5 stub
challenges document missing language features.

## Same-machine, same-algorithm methodology

The CLBG website publishes numbers from a Debian server with older
hardware. To get an honest comparison, I built sequential Rust
reference implementations of all 5 numeric challenges in
`bench/clbg_rust_ref/`, with identical settings to Soma's per-cell
build (target-cpu=native, lto=thin, codegen-units=1, opt-level=3,
overflow-checks=true).

The Rust references are NOT the parallel/SIMD-heavy versions from
the CLBG repo — those use Rayon and packed_simd to win the actual
Game. They're "what you'd write if you weren't trying to game the
benchmark", which is the fair comparison for Soma's [native]
codegen.

## What landed during this pass

### New: `Buf<Float>` primitive

To implement spectral-norm, I added 3 builtins to `[native]`:

- `buffer_f(N)` — creates a `Vec<f64>` of size N
- `buf_get_f(b, i)` — reads `b[i] as f64`
- `buf_set_f(b, i, v)` — writes `b[i] = v`

Same shape as the existing `Buf<Int>` primitive (added earlier for
hofstadter_q). Together they cover most "I need a fixed-size scalar
array" needs in [native]. With `Buf<Int>` + `Buf<Float>`, Soma can
do the kind of dense numeric work that was previously stuck on
slot-packed BigInt.

### CLBG corpus

`examples/clbg_corpus/clbg01..clbg10.cell` — 10 cells, one per
challenge. Each runs through `soma run` and either prints a
verified-correct result + timing, or prints `GAP NOTED:` followed
by the missing language feature.

### Rust reference suite

`bench/clbg_rust_ref/` — Cargo project with 5 hand-written sequential
binaries. Build with `cargo build --release`, run as
`./target/release/<name> <workload>`.

## Where Soma can improve

Sorted by leverage (biggest realistic perf gain at the top):

### 1. SIMD vectorization in [native] (1.5-3× on float loops)

- **Affects**: mandelbrot, fannkuch, n-body inner loop, spectral-norm
- **Status**: Soma's codegen emits scalar f64 ops. LLVM auto-vectorizes
  some patterns under target-cpu=native, but data-dependent inner loops
  (mandelbrot's escape check, fannkuch's flip count) defeat it.
- **What's needed**: emit explicit SIMD primitives in the codegen, OR
  add `simd_*` builtins that the user can call. Numba uses `@vectorize`;
  Soma could add similar.
- **Estimated win**: 1.5-2× on mandelbrot, 1.3-2× on fannkuch.

### 2. `Buf` as parameter type in [native] (small but ergonomic)

- **Affects**: code organization for multi-function buffer-based
  algorithms (spectral-norm, k-nucleotide).
- **Status**: I had to inline the `mul_av` / `mul_atv` helper functions
  into spectral-norm's main `compute()` because the [native] checker
  rejects `Buf` as a parameter type. The result works but is uglier.
- **What's needed**: extend the checker to accept `Buf` (and `Buf<Float>`)
  as `[native]` parameter types, with the codegen passing them as
  `&mut Vec<i64>` / `&mut Vec<f64>` references.
- **Estimated win**: zero perf, big readability win.

### 3. `HashMap<Int, Int>` primitive (unblocks k-nucleotide)

- **Affects**: k-nucleotide, any cell that needs sparse counting.
- **Status**: completely missing from `[native]`. The cell uses
  `Buf<Int>` as a flat hash table for k=4 (256 buckets), but k=12
  needs 16M buckets and k=18 needs 68 billion (Hashmap territory).
- **What's needed**: `hashmap()`, `hm_get(h, k)`, `hm_set(h, k, v)`,
  `hm_inc(h, k)` builtins. Compile to `std::collections::HashMap<i64, i64>`.
- **Estimated win**: enables the k-nucleotide challenge entirely.

### 4. Regex builtin (unblocks regex-redux)

- **Affects**: regex-redux, any text-processing cell.
- **Status**: no regex engine in [native].
- **What's needed**: `regex_count(text, pattern)` and
  `regex_replace(text, pattern, repl)` builtins that compile to
  Rust's `regex` crate calls. Adds ~1MB to dylib size when used.
- **Estimated win**: enables regex-redux.

### 5. File I/O builtins (unblocks fasta + reverse-complement)

- **Affects**: fasta, reverse-complement, k-nucleotide (FASTA input).
- **Status**: [native] has no file I/O. Cells that need to read
  files have to do it via the interpreter and pass strings into
  [native] handlers, which costs an extra String round-trip.
- **What's needed**: `read_file_buf(path)` returning `Buf<Int>` (bytes)
  and `write_buf(buf, len)` to stdout. Bonus: `read_stdin_buf()`
  for piped input (CLBG-spec fasta input format).
- **Estimated win**: enables 3 of the 5 string-heavy challenges.

### 6. StringBuf / efficient print stream (small win)

- **Affects**: fasta output, any cell that builds large strings.
- **Status**: Soma already has a String += peephole that's fast,
  but for 250MB output it's still 2-3× slower than Rust's `write!`
  macro to a buffered writer.
- **What's needed**: `print_chunk(buf, len)` that writes a Buf<Int>
  segment as bytes to stdout without going through String.
- **Estimated win**: 2× on fasta-style output workloads.

### 7. Heap structs / arena allocator (unblocks binary-trees properly)

- **Affects**: binary-trees, any tree-based algorithm.
- **Status**: [native] has no heap structs. The cell uses Buf<Int>
  as a flat binary heap which works for fully-balanced trees but
  is awkward for arbitrary structures.
- **What's needed**: a `[native]`-compatible struct primitive, OR
  an arena allocator that returns indices. The Soma philosophy
  resists this — cells are the only struct — but [native] is a
  different layer.
- **Estimated win**: would let binary-trees be a real test of
  allocator performance.

## Why mandelbrot is essentially tied

Mandelbrot is the most interesting result. The published CLBG Rust
number is much faster than 3.6s — but that's because the official
Rust version uses hand-written SIMD intrinsics (`packed_simd`,
processing 8 pixels at once). Our "fair fight" Rust version uses
the same naive scalar loop as Soma, and it lands at 3.638s vs Soma's
3.722s — **a 2% gap**.

This means Soma's codegen is producing nearly-identical scalar
machine code to what `rustc` produces. The remaining 2% is probably
register allocation differences and the `catch_unwind` overhead in
the FFI dispatch.

To beat the CLBG Rust mandelbrot we'd need SIMD, which is gap #1
above. But against equally-effort Rust, we're already there.

## Why pidigits is FASTER

Soma 467ms vs Rust 541ms. **Soma is 16% faster than the same
algorithm in Rust using the same `rug` crate.**

The reason: Soma's swap-on-assign optimization (commit `0b41499`)
detects that the inner Integer reassignments in the spigot loop
are safe to satisfy via `std::mem::swap` instead of `clone_from`.
The Rust version I wrote uses idiomatic `let nq: Integer = ...; q = nq;`
which compiles to `clone_from`. With the swap optimization, Soma
saves a full BigInt clone per loop iteration on 6 different
variables.

This is a real Soma win that comes from the dual-mode codegen
investing in BigInt-specific peepholes that idiomatic Rust doesn't
get for free.

## Numbers at a glance

```
Sequential Rust (hand-written, target-cpu=native, no SIMD):
  pidigits 10K       541ms
  spectral-norm 5500 744ms
  nbody 50M          912ms
  mandelbrot 16K²    3638ms
  fannkuch 12        21533ms

Soma [native] same machine:
  pidigits 10K       467ms     0.86× (FASTER)
  spectral-norm 5500 875ms     1.18×
  nbody 50M          1176ms    1.29×
  mandelbrot 16K²    3722ms    1.02× (TIED)
  fannkuch 12        33700ms   1.57×

Geomean Soma/Rust: ~1.16×  (Soma is within 16% of hand-written Rust)
```

## What this proves

1. **Soma is competitive with hand-written sequential Rust on the
   standard CLBG numeric challenges.** Within 16% on geomean. The
   gaps are small constants (register allocation, function-call
   overhead, missing SIMD), not order-of-magnitude algorithmic
   problems.

2. **For BigInt-heavy work, Soma is faster than Rust.** The
   swap-on-assign codegen + dual-mode dispatch are real wins that
   Rust gets only via careful manual coding.

3. **The 5 challenges Soma can't do are language-feature gaps**, not
   compiler/codegen gaps. Adding HashMap, regex, file I/O, and SIMD
   primitives to `[native]` would let Soma run all 10. Each of these
   is a few hundred lines in the codegen.

## Reproduce

```sh
# Build the Rust references
cd bench/clbg_rust_ref
cargo build --release

# Run them
./target/release/nbody 50000000
./target/release/spectral_norm 5500
./target/release/mandelbrot 16000 50
./target/release/fannkuch 12
./target/release/pidigits 10000

# Run the Soma versions
cd ../..
for f in examples/clbg_corpus/clbg0[1-5]_*.cell; do
  ./compiler/target/release/soma run "$f"
done
```
