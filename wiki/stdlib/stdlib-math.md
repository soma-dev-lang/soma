---
name: stdlib-math
description: Math builtins — arithmetic, sqrt, pow, random, bit ops.
type: reference
since: V1.0
related: [stdlib-linalg]
---

# Stdlib: math

Numeric builtins. Soma has a single integer type `Int` (auto-promotes
to BigInt on overflow — see `MEMORY_DESIGN.md`) and `Float` (f64).

## Basic arithmetic

Operators: `+`, `-`, `*`, `/`, `%`. Mixed Int/Float promotes to Float
when non-exact (`7 / 2 = 3.5`).

- `abs(n)` — absolute value.
- `min(a, b)` / `max(a, b)`.
- `clamp(v, lo, hi)` — clamp `v` to `[lo, hi]`.

## Rounding

- `round(x)` — half away from zero.
- `floor(x)` / `ceil(x)`.

## Exponents & logarithms

- `pow(base, exp)` — returns `Float`.
- `sqrt(x)` — returns `Float`.
- `exp(x)` / `log(x)` / `log10(x)`.

## Conversion

- `to_int(v)` — parse from String or convert Float→Int. Returns `()`
  on failure (e.g. `to_int("abc")`).
- `to_float(v)` — Int→Float, or parse from String.
- `type_of(v)` — String name: `"Int"`, `"Float"`, `"String"`, `"Bool"`,
  `"List"`, `"Map"`, `"Lambda"`, `"Variant"`, `"Unit"`.

## Random

- `random()` — Float in `[0, 1)`.
- `random(max)` — Int in `[0, max)`.
- `random(min, max)` — Int in `[min, max)`.

Nondeterministic — calls are logged for [[handler]]-level replay
detection.

## Bit operations

- `band(a, b)` / `bor(a, b)` / `bxor(a, b)` — bitwise AND, OR, XOR.
- `bnot(a)` — bitwise NOT.
- `shl(a, n)` / `shr(a, n)` — bit shift.
- `bit_test(a, n)` — bit `n` of `a` (0 or 1).
- `bit_set(a, n)` / `bit_clr(a, n)` — set / clear bit `n`.
- `bit_next(a, n)` — index of next set bit `≥ n`, or `-1`.
- `bit_len(a)` — number of bits to represent `a`.

## Number theory

- `gcd(a, b)` — greatest common divisor.
- `sqrt_int(n)` — integer square root (`(int)sqrt(n)`).
- `pow_mod(b, e, m)` — `(b ** e) mod m`, modular exponentiation.

## Examples

```soma
abs(-7)                  // 7
sqrt(16.0)               // 4.0
pow(2, 10)               // 1024.0
clamp(150, 0, 100)       // 100
random()                 // 0.823...
random(0, 6)             // 0..5
band(0b1100, 0b1010)     // 0b1000 = 8

let r = sqrt(2.0)
to_int(r)                // 1 (truncated)
to_string(r)             // "1.4142135..."

gcd(12, 8)               // 4
pow_mod(2, 10, 1000)     // 24
```

## BigInt behavior

Soma's `Int` auto-promotes to BigInt on i64 overflow. The arithmetic
operators handle this transparently:

```soma
let n = 1
for i in range(1, 30) {
    n = n * i           // n becomes BigInt past 20!
}
print(n)                // 8841761993739701954543616000000 (huge)
```

`MEMORY_DESIGN.md` has the full story (NaN-boxed inline small ints,
pointer to a BigInt arena otherwise).

## Edge cases

- `to_int("abc")` returns `()`, not `0`. Always check.
- `to_int(3.7)` truncates toward zero → `3`. Use `floor`/`round` for
  explicit behavior.
- `abs(i64::MIN)` is a runtime error (no positive equivalent).
- `random()` uses a thread-local xorshift+LCG seeded with system
  nanoseconds. Not cryptographic; use `crypto_random()` (if added) for
  security-sensitive use.

## Related

- [[stdlib-linalg]] — `matrix`, `svd_lowrank`, `regress_sgd`.
- [[stdlib-risk]] — `var_historical`, `impact_sqrt`,
  `clean_covariance`.
