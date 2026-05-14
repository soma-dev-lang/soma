# Sum Types for Soma — Design

> Status: proposal. Not implemented.
> Target: Soma v1.5.

## TL;DR

Add **sum types** (a.k.a. tagged unions, ADTs, enums) to Soma as a
first-class language feature with exhaustive pattern matching, integrated
into the existing `state { }` blocks, `face { }` contracts, and `match`
expressions. Hardcode `Option<T>` and `Result<T, E>` as built-ins.

The single biggest payoff: **state machines stop being stringly-typed**.
A `transition("order_id", "VALIDTAED")` typo becomes a compile error
instead of a runtime surprise.

## Why this is the right gap to close

Soma's tagline is "the language where handler bodies cannot lie to the
state machine." V1.3 closed *half* of that gap (every `transition("X")`
target must exist in `state { }`). The other half — making the state
name a *type*, not a string — is still open. As long as states are
strings, the compiler can verify *which* states are reached but not
*what kind of value* a state is. Sum types close this gap properly.

Beyond state machines, Soma currently has no way to express a value
that is "exactly one of these N alternatives, each with possibly
different fields." Today users encode this as `Map<String, String>` with
a `"_type"` field — a pattern that's documented in
`SOMA_REFERENCE.md` and used throughout the rebalancer. That's a
type-system-shaped hole, currently filled with conventions.

## Design

### 1. Declaration syntax

```soma
type OrderStatus {
    Pending
    Validated
    Filled
    Cancelled
}
```

Variants are introduced one per line, no separators. The type name and
each variant name are PascalCase by convention. This declaration form
sits at the top level alongside `cell`, like `cell property` does.

With payloads (struct-like variants):

```soma
type OrderResult {
    Accepted { order_id: String, exec_price: Float }
    Rejected { reason: String, code: Int }
    Pending
}
```

With tuple payloads (positional):

```soma
type Move {
    Up(Int)
    Down(Int)
    Left(Int)
    Right(Int)
    Wait
}
```

Both forms can coexist in one declaration. Struct fields are preferred
for >1 field; tuple variants are sugar for single-field cases.

### 2. Construction

```soma
let s = Pending
let r = Accepted { order_id: "ABC-123", exec_price: 105.50 }
let m = Up(3)
```

When the variant name is ambiguous across types, qualify:

```soma
let s = OrderStatus::Pending
```

The compiler resolves unqualified names to the type expected by
context (assignment target type, handler parameter type, return type).

### 3. Pattern matching

Existing `match` extends naturally:

```soma
match result {
    Accepted { order_id, exec_price }       -> handle(order_id, exec_price)
    Rejected { reason, code: 503, .. }      -> retry(reason)
    Rejected { reason, code }               -> log_error(reason, code)
    Pending                                  -> wait()
}
```

- Struct-variant fields use existing map-destructuring syntax (`{ field, field2 }`).
- `..` matches "any remaining fields" — already familiar from Rust users.
- Guards (`if condition`) continue to work.

Tuple variants:

```soma
match move {
    Up(n) | Down(n)    -> vertical(n)
    Left(n) | Right(n) -> horizontal(n)
    Wait                -> ()
}
```

### 4. Exhaustiveness checking

The compiler emits a hard error if a `match` doesn't cover every variant
of the subject's type, unless a wildcard `_` or variable-binding pattern
catches the rest:

```soma
// ERROR: non-exhaustive match — missing variant `Cancelled`
let label = match status {
    Pending   -> "waiting"
    Validated -> "ready"
    Filled    -> "done"
}

// OK: wildcard explicitly catches the unhandled case
let label = match status {
    Pending   -> "waiting"
    Validated -> "ready"
    Filled    -> "done"
    _         -> "unknown"
}
```

The same check applies inside guarded arms — if the guards collectively
don't cover all values, the compiler refuses. For pragmatic escape,
`match status { _ -> ... }` always type-checks.

### 5. State machine integration — the killer feature

#### Current (V1.3)

```soma
state order {
    initial: pending
    pending -> validated
    validated -> filled
    * -> cancelled
}

on advance(id: String) {
    transition(id, "validated")          // string — typo = runtime error
}
```

#### Proposed (v1.5)

```soma
type OrderState {
    Pending
    Validated
    Filled
    Cancelled
}

state order: OrderState {
    initial: Pending
    Pending -> Validated
    Validated -> Filled
    * -> Cancelled
}

on advance(id: String) {
    transition(id, Validated)            // typed — typo = compile error
}
```

The `state` block's state names are now variant references. `transition`
signature changes from `(String, String) -> Map` to
`<S>(String, S) -> Map` where `S` is the state-machine type.

**This is what V1.3 always wanted to be.** The refinement check verifies
that `transition("x", Validated)` mentions a real variant. The type
checker now also verifies that `Validated` is a variant of the *right
state machine* — you can't accidentally transition `OrderState` to a
`TradeState::Settled`.

Backward compatibility: state blocks without a `: TypeName` annotation
continue to behave as today (string state names). Authors opt in by
naming the state type.

### 6. Face contracts

Sum types in signal return types:

```soma
cell PaymentGateway {
    face {
        signal charge(amount: Int, card: String) -> PaymentResult
    }
    on charge(amount: Int, card: String) {
        if validate(card) {
            return Charged { transaction_id: gen_id(), amount }
        }
        return Declined { reason: "invalid card" }
    }
}

cell Caller {
    on pay() {
        let res = delegate("PaymentGateway", "charge", 1000, "4111...")
        match res {
            Charged { transaction_id, amount } -> log_success(transaction_id)
            Declined { reason }                -> notify_user(reason)
            // Compiler: missing variant `Pending`
        }
    }
}
```

The caller is *forced* by the type system to handle every outcome
the contract advertises. This is the same property Rust gets from
`Result<T, E>` — but cross-cell, with the signal-bus boundary.

### 7. Hardcoded built-ins

For v1.5, two parametric types are baked into the language:

```soma
type Option<T> {
    Some(T)
    None
}

type Result<T, E> {
    Ok(T)
    Err(E)
}
```

These are the only generics that exist initially. They're enough to
replace ~80% of current `()`-as-null patterns and `try { } ?`
constructs.

```soma
// Current
let raw = data.get("key")
if raw == () {
    return response(404, map("error", "not found"))
}
let value = from_json(raw)

// Proposed
match data.get("key") {
    Some(raw) -> from_json(raw)
    None      -> return response(404, map("error", "not found"))
}
```

`try { expr }` becomes sugar for matching on a `Result`. The `?`
operator propagates `Err` upward — same semantics, now type-checked.

The parser knows `Option` and `Result` syntactically. Full
parametric polymorphism over user-defined types is a v2 problem.

### 8. Coexistence with records

Records (`User { name: "Alice", age: 30 }`) are *unit* sum types —
exactly one variant. The proposal does not deprecate records;
struct-variant patterns and record-field access share syntax:

```soma
match u {
    User { name, age } -> ...
}
```

A record literal can be viewed as a `type User { User { name, age } }`
where the type name and the single variant name coincide. The
compiler may eventually merge the two declarations; for v1.5 they stay
separate to avoid breaking existing code.

## Migration plan

### Phase 1 (v1.5.0): opt-in

- `type` declarations parse and type-check.
- `match` gains exhaustiveness checking *only* for sum-typed subjects.
- String-typed `match` continues to behave as today (wildcard required).
- State blocks without `: TypeName` annotation behave as today.
- `Option<T>` and `Result<T, E>` are introduced as built-ins but
  existing `()` and `try { }` patterns continue to work.

### Phase 2 (v1.6): nudge

- The linter emits a warning for stringly-typed state machines that
  could be sum-typed: "consider `type StateName { ... }` for stronger
  guarantees."
- The linter warns on `if x == ()` patterns: "consider matching on
  `Option<T>`."
- New examples in `examples/` use sum types throughout.

### Phase 3 (v2.0): default

- Documentation defaults to sum types.
- New state blocks require a type annotation (existing ones grandfathered).
- `()` as null still works for legacy paths but is documented as
  discouraged.
- Custom generics (parametric polymorphism over user-defined types)
  arrive — `type Pair<A, B> { ... }`.

No existing Soma program needs to change for Phase 1. Adoption is
purely additive.

## Implementation plan

### AST changes

Add three node kinds to `compiler/src/ast/mod.rs`:

```rust
pub enum TopLevel {
    Cell(Cell),
    Type(TypeDecl),        // NEW
    // ...
}

pub struct TypeDecl {
    pub name: String,
    pub generics: Vec<String>,    // ["T"] for Option<T>
    pub variants: Vec<Variant>,
    pub span: Span,
}

pub struct Variant {
    pub name: String,
    pub fields: VariantFields,
    pub span: Span,
}

pub enum VariantFields {
    Unit,
    Tuple(Vec<TypeExpr>),
    Struct(Vec<(String, TypeExpr)>),
}
```

Add new pattern kinds in `Pattern`:

```rust
pub enum Pattern {
    // existing
    Variant {
        type_name: Option<String>,        // None when unqualified
        variant_name: String,
        fields: VariantPatternFields,
    },
}

pub enum VariantPatternFields {
    Unit,
    Tuple(Vec<Pattern>),
    Struct(Vec<(String, Pattern)>, /* rest */ bool),
}
```

### Parser changes

`compiler/src/parser/mod.rs` gets a `parse_type_decl` entry that handles
the new top-level `type` keyword. Pattern parsing is extended to
recognize PascalCase identifiers as variant patterns and to accept the
`..` rest marker.

### Checker changes

A new pass `compiler/src/checker/sum_types.rs` builds a registry of
`TypeDecl`s indexed by name, validates that variant references resolve,
and emits exhaustiveness diagnostics for `match` arms whose subject
has a known sum type. The pass runs after `properties.rs` and before
`refinement.rs` so that state-block annotations can be cross-checked.

State-block refinement (`refinement.rs`) gets extended:
- If `state order: OrderState { ... }`, every state name in the block
  must be a variant of `OrderState`.
- Every `transition(id, expr)` call site whose `id` belongs to such a
  block must produce a value of type `OrderState`.

### Interpreter changes

`compiler/src/interpreter/mod.rs` adds a new `Value` variant:

```rust
pub enum Value {
    // existing
    Variant {
        type_name: String,
        variant: String,
        fields: VariantValueFields,
    },
}

pub enum VariantValueFields {
    Unit,
    Tuple(Vec<Value>),
    Struct(IndexMap<String, Value>),
}
```

Pattern matching dispatches the new pattern kinds.
String-typed `transition()` continues to work; the new typed form is
detected by checking whether the second argument evaluates to a
`Value::Variant`.

### Coq proofs

Two new files under `docs/rigor/coq/`:

- `Soma_SumTypes.v`: well-formed sum types are sound under reduction
  (variants of `T` evaluate to values of `T`, exhaustive match never
  gets stuck).
- `Soma_RefinementSum.v`: extends the V1.3 refinement theorem to
  variant-typed state blocks. The theorem statement becomes "every
  transition target is a variant of the state type" rather than "every
  transition target is a literal state name."

The existing `Soma_CTL.v` is parameterised over a state type, not
strings, so the proof carries through with minor adjustments.

### Cost-lattice rule

Sum-type values cost `8 + sum(field costs)` bytes — unit variant is 8
(tag only), tuple/struct variants pay for the tag plus the fields. The
existing record cost rule (`Cost::bytes(64) + field costs`) is
reused with adjusted constants.

## Examples

### Replacing the rebalancer's stringly-typed state

```soma
// Current
on rebalance(id: String) {
    transition(id, "alpha_pending")
    let alpha = delegate("Alpha", "score")
    if alpha.error != () {
        transition(id, "failed")
        return map("status", "failed", "reason", alpha.error)
    }
    transition(id, "optimizing")
    // ...
}

// Proposed
type Rebalance {
    Requested
    AlphaPending
    Optimizing
    Approved
    Blocked
    Flagged
    Failed { reason: String }
}

on rebalance(id: String) {
    transition(id, AlphaPending)
    let alpha = delegate("Alpha", "score")
    match alpha {
        Err(e) -> {
            transition(id, Failed { reason: e })
            return Failed { reason: e }
        }
        Ok(score) -> {
            transition(id, Optimizing)
            // ...
        }
    }
}
```

Now the rebalance state isn't just *named* `Failed` — it *carries* the
reason. Downstream code that consumes the state knows there's a
`reason: String` field to match on. The state machine and the error
payload are unified.

### Bouchaud impact gate, type-safe

```soma
type ImpactCheck {
    Approved { estimated_bps: Float }
    Rejected { estimated_bps: Float, max_bps: Float }
}

on check_impact(qty: Float, vol: Float, sigma: Float, max: Float) -> ImpactCheck {
    let imp = impact_sqrt(qty, vol, sigma, map("Y", 1.0))
    if imp.bps > max {
        return Rejected { estimated_bps: imp.bps, max_bps: max }
    }
    Approved { estimated_bps: imp.bps }
}

on submit() {
    match check_impact(qty, vol, sigma, 30.0) {
        Approved { estimated_bps } -> emit place_order(qty)
        Rejected { estimated_bps, max_bps } ->
            log_warn("rejected: {estimated_bps} bps > {max_bps}")
    }
}
```

The `ensure` clause from the current `risk_check.cell` example becomes
a `match` arm. Failure modes are *named*, *payload-carrying*, and
*exhaustively handled*. This is exactly what Soma's verification stance
is for — pushing failure cases into the type system.

## Open questions

### Q1: Naming conflicts across cells

If cell A declares `type Status { Active, Failed }` and cell B
declares `type Status { Running, Stopped }`, they conflict at link
time. Options:

- **(a)** Module-qualified by default: `A::Status` and `B::Status`.
- **(b)** Implicit per-file scoping with explicit re-exports.
- **(c)** Compile error on duplicates.

Recommendation: **(a)** — module qualification matches existing `use lib::risk` discipline.

### Q2: Variant uniqueness across types

If two types each have a variant named `Pending`, can the compiler
disambiguate by context, or must users write `OrderState::Pending`?

Recommendation: **disambiguate by context where unambiguous**, require
qualification only when the type system can't infer. Same rule as Rust.

### Q3: Generic user types in v1.5

Strictly speaking, only `Option<T>` and `Result<T, E>` are hardcoded.
Should we allow users to write their own generics?

Recommendation: **no for v1.5.** Hardcoding the two built-ins delivers
most of the value at a fraction of the implementation cost. Full
parametric polymorphism is its own Tier-1 work item (separate proposal).

### Q4: Methods on sum types

Rust has `impl Status { fn label(&self) -> String { ... } }`. Should
Soma have `cell impl OrderStatus { on label() { ... } }`?

Recommendation: **defer to v2.** For v1.5, free functions that take a
sum type and pattern-match work fine and avoid object-vs-function
debates.

### Q5: Equality and hashing

`Pending == Pending` should be `true`. `Accepted { id: "X" } ==
Accepted { id: "X" }` should be `true`. Structural equality on variants
must be defined. Hashing follows from equality. Both are mechanical;
no design surprises.

### Q6: Serialization

`to_json(Approved { estimated_bps: 30.0 })` — what does it return?
Options:

- `{"_type": "Approved", "estimated_bps": 30.0}` (tagged)
- `{"Approved": {"estimated_bps": 30.0}}` (externally tagged, OCaml-style)
- `{"estimated_bps": 30.0}` (untagged, requires schema on the consumer)

Recommendation: **(a) — tagged.** Symmetric with `from_json`, easiest to
discriminate on the consumer side, matches the existing `_type` field
convention in records.

### Q7: Empty types

`type Never { }` — no variants. Useful as a return type for functions
that don't return (panic, infinite loops). Symmetric with Rust's `!`.

Recommendation: **allow it.** Zero variants means any `match never_value
{ }` is vacuously exhaustive. Useful for the rare cases where a handler
provably diverges.

## Risk and downsides

1. **Surface-area increase.** Adds one top-level keyword (`type`), one
   pattern kind, and a new exhaustiveness check. Non-trivial but
   contained.
2. **Compiler complexity.** New AST node, new resolver pass, new
   value variant, new pattern variant. ~1500 LOC estimated, similar
   to what was added for the linalg module.
3. **Existing tooling.** `soma fix` and `soma lint` need to learn about
   the new constructs.
4. **Migration cost for examples.** ~20 example cells reference state
   names as strings. Each is a 1-line change but the cumulative effort
   adds up.
5. **The Coq proofs grow.** Two new files, plus modest updates to
   existing ones to thread the state type parameter. Adam Chlipala would
   approve; the maintainer has to do the work.

The single biggest non-obvious risk: **state machines as sum types
might tempt users to encode business logic in variant payloads**
(e.g. `Failed { reason: String, retry_count: Int }`), which then can't
be queried as easily as a memory slot. The mitigation is clear
documentation that variant payloads should be small, immutable, and
descriptive — the state machine is for *control flow*, the memory
slots are for *data*.

## Why this earns its surface area

The exhaustiveness check alone is a property *no current Soma
program can express*. It would catch a class of bugs — "I added a new
state and forgot to update one of the handlers" — that today only
shows up at runtime, if you're lucky, or in production, if you're not.

Combined with the state-machine integration, sum types pull V1.3's
"refinement" promise across the finish line: the spec, the handler
bodies, *and* the type system all agree on what states exist. That's
the final piece of "the spec is the program."

For the agent-oriented direction, sum types massively improve the
LLM-writing-Soma experience: instead of memorizing string state names
and praying the LLM doesn't typo, the agent gets type errors from the
compiler the moment it produces a wrong variant name. This is the same
reason TypeScript is the dominant LLM-writable language right now —
sum types (discriminated unions) are doing 90% of that work.

## Next steps

1. **Discuss this document.** Iterate on the open questions.
2. **Prototype the parser.** ~200 LOC, one weekend. Lands on a feature
   branch.
3. **Prototype the checker.** Exhaustiveness checking on toy inputs.
4. **Migrate one example.** Pick `rebalancer/app.cell` or `mft/app.cell`
   and convert its state machine. See whether the rewrite feels good or
   forced.
5. **Coq sketch.** Prove `Soma_SumTypes.v` for a 3-variant toy. This
   tests whether the existing `Soma_CTL.v` parameterization holds up.
6. **Decide on Phase 1 scope.** What lands in v1.5 vs v1.6.

A reasonable target is **prototype in 3-4 weeks**, **stabilize in 8**,
**ship in v1.5 within a quarter**.

---

*Authors: drafted by Claude in dialogue with antoine. Comments and
suggestions: open an issue against this file, or amend in a PR.*
