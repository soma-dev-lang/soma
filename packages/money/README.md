# money

Amounts in minor units with their currency: no Float holds money. add / subtract (same currency only), scale and basis points rounded half away from zero, `allocate` that keeps every cent, format and parse.

```soma
use money

money(1999, "EUR")
allocate(1000, [1, 1, 1])        // [334, 333, 333] — the parts always sum to the total
basis_points(m, 250)              // 2.5 %
format_money(m)  parse_money("19.99", "EUR")
```

Kinds: `BadCurrency`, `CurrencyMismatch`, `BadRatio`, `BadAmount`.

Pure Soma. Its proofs are its tests — re-run them on your toolchain:

```
soma test packages/money/money.cell
```
