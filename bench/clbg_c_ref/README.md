# CLBG C reference suite

Hand-written C implementations of the 5 numeric Computer Language
Benchmarks Game challenges, used as the gold-standard baseline for
rating Soma's `[native]` codegen.

Same algorithms as `bench/clbg_rust_ref/` and the matching Soma
cells in `examples/clbg_corpus/`.

## Build

```sh
make
```

Requires `clang` and `gmp` (Homebrew: `brew install gmp`). The
Makefile points to `/opt/homebrew/opt/gmp` for the GMP headers and
library — adjust `GMP_PREFIX` for other installs.

Build flags: `-O3 -march=native -std=c11`. We deliberately do NOT
pass `-ffast-math` so the floating-point semantics match Rust's
default and Soma's `[native]` codegen — a fair head-to-head.

## Run

```sh
./nbody         50000000   # CLBG full workload
./spectral_norm 5500
./pidigits      10000      # uses libgmp
./mandelbrot    16000 50
./fannkuch      12
```

Each binary prints the result and an `elapsed:` line in milliseconds.

## Notes

- `pidigits.c` uses the same Gibbons spigot algorithm as the Soma
  and Rust versions, calling `mpz_*` directly.
- `nbody.c` is hand-unrolled over 5 bodies, no SIMD intrinsics.
- `mandelbrot.c` is the scalar version, no SIMD intrinsics — the
  CLBG-winning C version uses hand-written AVX, which is not a
  fair comparison for Soma.
- `fannkuch.c` is the straight permutation enumerator, no
  parallelism.

Results are tabulated in `bench/results/SUMMARY_clbg.md`.
