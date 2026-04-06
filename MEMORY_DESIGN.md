# Soma Integer Memory System — Design

## Principle

Soma has **one integer type**: `Int`. It handles 42, 100!, and everything between. The programmer never thinks about i64 vs BigInt. The runtime handles it.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    SomaInt (64 bits)                 │
├─────────────────────────────────────────────────────┤
│ If bit 0 = 1:  small int (63-bit value, inline)     │
│ If bit 0 = 0:  pointer to IntCell in arena          │
└─────────────────────────────────────────────────────┘
```

A `SomaInt` is always 8 bytes. It's either:
- **Small**: the value fits in 63 bits (±4.6 × 10^18). Stored inline. Zero allocation. This covers 99.9% of real code.
- **Big**: a pointer into the integer arena. The arena holds the limbs.

## Integer Arena

```
┌──────────────────────────────────────────────────────┐
│                  IntArena (128 MB default)            │
├──────────────────────────────────────────────────────┤
│  ┌──────────┐ ┌──────────┐ ┌──────────┐             │
│  │ IntCell  │ │ IntCell  │ │ IntCell  │  ...         │
│  │ header   │ │ header   │ │ header   │             │
│  │ limbs[]  │ │ limbs[]  │ │ limbs[]  │             │
│  └──────────┘ └──────────┘ └──────────┘             │
│                                                      │
│  free_ptr ──────────────────────►  [free space]      │
└──────────────────────────────────────────────────────┘
```

### IntCell layout

```
┌──────────────────────────────────────┐
│ header (16 bytes)                    │
│   ref_count : u32   (reference cnt)  │
│   limb_count: u16   (number of u64s) │
│   sign      : u8    (0=pos, 1=neg)   │
│   _pad      : u8                     │
│   next_free : u32   (free list link) │
├──────────────────────────────────────┤
│ limbs[0]  : u64  (least significant) │
│ limbs[1]  : u64                      │
│ ...                                  │
│ limbs[N-1]: u64  (most significant)  │
└──────────────────────────────────────┘
```

### Size math

- 128 MB arena ÷ 16 bytes overhead = millions of small BigInts
- 100000! = 456,574 digits = ~190 KB of limbs = 1 IntCell
- Sum of 1M BigInts = ~8 MB (most are small, get reclaimed)

## Memory Cleaner

**Reference counting** — no GC pauses, deterministic.

Integers can't form cycles (an Int never points to another Int). So refcounting is perfect:

```
soma_int_clone(x)  →  x.ref_count += 1
soma_int_drop(x)   →  x.ref_count -= 1; if 0: add to free list
```

### Arena operations

| Operation | What happens |
|-----------|-------------|
| `new(i64)` | If fits 63 bits: return tagged inline. Else: allocate IntCell from free list or bump pointer |
| `add(a, b)` | If both small: try i64 add. Overflow? Allocate big result. |
| `mul(a, b)` | If both small: try i64 mul. Overflow? Allocate big result. |
| `drop(x)` | If small: no-op. If big: decrement refcount, free if zero |
| `clone(x)` | If small: copy. If big: increment refcount |

### When arena is full

1. **Compact**: walk all live cells, move to front of arena, update pointers
2. If still full: **error** "integer memory limit exceeded (128MB)"
3. The limit is configurable in soma.toml: `[runtime] int_memory = "256MB"`

## For the interpreter

Replace:
```rust
// Old
pub enum Value {
    Int(i64),
    Big(BigInt),
    ...
}
```

With:
```rust
// New
pub enum Value {
    Int(SomaInt),   // 8 bytes — inline small or pointer to arena
    ...
}
```

All arithmetic goes through `SomaInt::add`, `SomaInt::mul`, etc. The caller never knows if it's small or big.

## For [native] compiled code

The generated code calls arena functions via FFI:

```rust
// Generated native code
extern "C" {
    fn soma_int_new(v: i64) -> u64;
    fn soma_int_add(a: u64, b: u64) -> u64;
    fn soma_int_mul(a: u64, b: u64) -> u64;
    fn soma_int_drop(a: u64);
    fn soma_int_to_i64(a: u64) -> i64;  // returns i64::MIN as sentinel if too big
}

// handler_factorial compiles to:
#[no_mangle]
pub extern "C" fn handler_factorial(n: i64) -> u64 {
    let mut result = soma_int_new(1);
    let mut i = soma_int_new(1);
    let limit = soma_int_new(n);
    while soma_int_le(i, limit) {
        result = soma_int_mul(result, i);  // auto-grows, always correct
        i = soma_int_add(i, soma_int_new(1));
    }
    soma_int_drop(i);
    soma_int_drop(limit);
    result  // returned as SomaInt, interpreter reads it
}
```

The native code shares the **same arena** as the interpreter. No FFI marshaling of BigInt values. The `u64` IS the SomaInt (tagged pointer).

## Performance

### Small int fast path (99.9% of operations)

```
soma_int_add(a, b):
    if a & 1 && b & 1:           // both small?
        result = (a >> 1) + (b >> 1)
        if no overflow:
            return (result << 1) | 1  // still small
    // fallback to big path
```

Two bit checks + one add + one shift. **Same speed as raw i64.**

### Big int path

Uses schoolbook multiplication for small numbers, Karatsuba for large. The arena amortizes allocation cost.

### Expected performance

| Operation | Small int (i64) | Big int (arena) |
|-----------|----------------|-----------------|
| Add | 1 ns | 10-100 ns |
| Mul | 1 ns | 10 ns - 10 µs |
| 100000! | N/A | ~1s (same as num-bigint) |
| Memory | 0 (inline) | arena cell |

## Configuration

```toml
# soma.toml
[runtime]
int_memory = "128MB"    # arena size (default 128MB)
```

## Implementation plan

1. **Create `soma_int.rs`** — the SomaInt type + arena
2. **Replace `Value::Int(i64)` and `Value::Big(BigInt)`** with `Value::Int(SomaInt)`
3. **Update interpreter arithmetic** to use SomaInt ops
4. **Export `soma_int_*` functions** from the soma binary
5. **Update [native] codegen** to call `soma_int_*` instead of raw i64
6. **Add arena stats** to `soma describe` output
7. **Add `[runtime] int_memory` config** to soma.toml
