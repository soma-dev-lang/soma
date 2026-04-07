# Plan: [native] PI spigot at Rust/GMP speed (~51s for 100K digits)

## Current state

| Version | 100K digits | Why |
|---------|------------|-----|
| Rust/GMP reference | **51s** | Direct rug::Integer calls, zero overhead |
| Soma interpreted | ~Xmin | GMP under the hood, but interpreter dispatch per op |
| Soma [native] | broken | `transform_v_arithmetic` can't handle compound expressions |

## Root cause

The [native] codegen has a fundamental design flaw: it generates Rust code using a `V` enum (i64/BigInt), then applies **text-based regex transforms** (`transform_v_arithmetic`) to convert operator syntax into method calls. This approach:

1. Only handles simple patterns like `x = ((&x) + (&y))` → `x.add_v(&y)`
2. Breaks on compound expressions like `(q * 3 + r) / (s * 3 + t)`
3. Can't handle `x = ((&x) * V::S(10))` (literal on right side)
4. Requires users to manually decompose expressions into single-op assignments

## Target

Generate native Rust that runs at the **same speed as hand-written Rust/GMP** (~51s for 100K digits). This means:

- Zero interpreter dispatch during the computation
- Direct `rug::Integer` operations (no V enum overhead)
- Single [native] handler runs the full spigot loop

## Approach: replace V enum with direct rug::Integer codegen

Instead of generating code using the `V` enum (which needs broken text transforms), generate code that directly uses `rug::Integer`. The `rug` crate is already a dependency (it's the same GMP that SomaInt uses).

### Why rug instead of V

The V enum exists to auto-promote i64→BigInt. But for the PI spigot, values overflow i64 within 20 digits — after that, everything is BigInt. The V optimization (stay in i64 while small) saves nothing for BigInt-heavy workloads and adds:
- Pattern matching overhead on every operation
- Complex text-based code transforms that break on compound expressions
- Two code paths (i64 and BigInt) that both need testing

With direct `rug::Integer`, every variable is a heap-allocated GMP integer. Operations are just `q *= 3; q += &r;` — no transforms needed, no pattern matching, standard Rust.

### Generated code target

For `let d = (q * 3 + r) / (s * 3 + t)`, generate:

```rust
let mut d = Integer::from(&q * 3u32);
d += &r;
{
    let mut _den = Integer::from(&s * 3u32);
    _den += &t;
    d /= &_den;
}
```

For `let nq = q * k`, generate:
```rust
let mut nq = Integer::from(&q * &k);
```

For `result = result + to_string(d)`:
```rust
result.push_str(&d.to_string());
```

### What changes

#### 1. Add codegen mode: `NativeMode::Rug` (new)

In `codegen/native.rs`, add a second codegen mode alongside the existing V-enum mode:

```rust
enum NativeMode {
    V,    // existing: i64 with BigInt promotion (good for Float-heavy, i64-range)
    Rug,  // new: direct rug::Integer (good for BigInt-heavy workloads)
}
```

**Selection heuristic**: if the handler has a `while` loop with Int arithmetic and no Float operations, use `Rug` mode. Otherwise use `V` mode. Or: add a `[native(rug)]` annotation.

Simpler alternative: always use Rug mode for Int-returning or String-returning handlers that use Int variables. The V mode stays for Float-only handlers.

#### 2. `gen_expr_rug` — expression codegen for Rug mode

Instead of generating `((&q) * V::S(3i64))` and hoping text transforms fix it, generate proper Rust directly:

| Soma expression | Generated Rust |
|----------------|---------------|
| `q * 3` | `Integer::from(&q * 3u32)` |
| `q * k` | `Integer::from(&q * &k)` |
| `q + r` | `Integer::from(&q + &r)` |
| `q / r` | `Integer::from(&q / &r)` |
| `q - d * s` | `{ let mut _t = Integer::from(&d * &s); _t = Integer::from(&q - &_t); _t }` |

Key: all intermediate results are `Integer`, no V enum, no transforms needed.

#### 3. `gen_stmt_rug` — statement codegen

| Soma statement | Generated Rust |
|---------------|---------------|
| `let x = expr` | `let mut x: Integer = {gen_expr_rug(expr)};` |
| `x = expr` | `x = {gen_expr_rug(expr)};` or `x.assign(...)` |
| `let s = ""` | `let mut s = String::new();` |
| `s = s + to_string(d)` | `s.push_str(&d.to_string());` |
| `return s` | `unsafe { _SOMA_RESULT = Some(s); } return i64::MIN + 1;` |
| `if d == d4` | `if d == d4 {` |
| `while digits <= target` | `while digits <= target {` (Int comparison works directly) |

#### 4. Cargo.toml — add rug dependency

Currently the native projects use `num-bigint`. Change to `rug`:

```toml
[dependencies]
rug = "1"
```

`rug` wraps GMP which is the same library SomaInt already uses. The native dylib will link against the same libgmp.

#### 5. FFI — String return (already works)

The String return path via `_SOMA_RESULT` is already implemented and tested. No changes needed.

#### 6. FFI — Int args for Rug mode

For `compute(target: Int)` where `target` fits in i64: pass as i64, convert to `Integer::from(target)` in generated code. This already works since the shared buffer pushes i64.

### Files to change

| File | Changes | Est. lines |
|------|---------|-----------|
| `codegen/native.rs` | Add `gen_expr_rug`, `gen_stmt_rug`, Rug mode selection, rug Cargo.toml | ~200 |
| `native_ffi.rs` | Change Cargo.toml generation to use rug instead of num-bigint | ~5 |
| `checker/native.rs` | No changes (already allows String) | 0 |

### Implementation order

1. **Change Cargo.toml generation** to use `rug` instead of `num-bigint` (5 min)
2. **Add `gen_expr_rug`** — pure rug::Integer expression generator, no transforms (1 hr)
3. **Add `gen_stmt_rug`** — statement generator using gen_expr_rug (30 min)
4. **Wire it up** — use Rug mode for handlers with Int+while loops (15 min)
5. **Test** with PI spigot (15 min)
6. **Keep V mode** as fallback for Float-heavy handlers that benefit from i64 fast path

### Risk: GMP linking

The native dylib needs to link against libgmp. On macOS with Homebrew: `-L /opt/homebrew/lib -lgmp`. The `rug` crate handles this via its build script, but the cargo project needs `gmp-mpfr-sys` as a transitive dep. The existing compiler already has this (SomaInt uses rug), so the system has GMP installed.

### Expected performance

The generated Rust should be ~identical to `pi_rust.rs` — same rug::Integer operations, same algorithm, same GMP underneath. Expected: **~51-55s** for 100K digits (within 10% of hand-written Rust).

### What NOT to do

- Don't try to fix `transform_v_arithmetic` for more patterns — it's a dead end
- Don't pass BigInt values through FFI per-operation — the whole point is one native call
- Don't keep using `num-bigint` — `rug` (GMP) is 2-5x faster for large integers
