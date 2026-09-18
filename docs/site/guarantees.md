# What Soma guarantees — and what it does not

Three columns, no adjectives. If a claim about Soma is not in the first two
columns, it is not a guarantee.

## 1. PROVEN statically — by `soma verify`, before the program runs

On the **state machine** of each cell (a finite graph, model-checked):

| Property | Meaning | How to ask |
|---|---|---|
| Reachability | every declared state can be reached from `initial` | always |
| Deadlock-freedom | no reachable non-final state without an exit | always |
| Liveness | every state can reach some final state | always (n/a for cyclic machines — said so) |
| `eventually = [...]` | every path reaches one of these states | `soma.toml [verify]` |
| `never = [...]` | these states are unreachable | `soma.toml [verify]` |
| `always = [...]` | the machine is only ever in one of these states | `soma.toml [verify]` |
| after … `never` | once X was reached, Y is unreachable | `[verify.after.X] never = ["Y"]` |
| after … `eventually` | after X every path reaches one of … | `[verify.after.X] eventually = [...]` |
| precedence | no path reaches T without one of / each of … | `[verify.before.T] requires / requires_all` |

On the **handlers**:

| Property | Meaning |
|---|---|
| Refinement | every `transition(id, "x")` with a literal target is a declared edge, and every declared edge is taken by some handler (or reported) |
| Termination | handlers terminate: bounded loops, recursion with a decreasing argument and a base case, no call cycles |
| Think-isolation | with only literal transition targets, the properties above hold whatever an LLM returns |
| Cost bound | `cost { tokens: N }` holds when every `think()` has a literal `max_tokens` and runs a known number of times (across sibling handlers) — a declared bound that cannot be proven is a `soma check` error (worded "bound is advisory"), never a silent pass |
| Invariants on known values | a write of a literal, a `clamp(..)`, or a value interval reasoning can bound — including by induction on the slot's own invariant: `(counts.get(k) ?? 0) + 1` keeps `counts >= 0` |

A failed property prints a counter-example path. A property that names a
state no machine declares is an error. `soma verify` refuses a program that
fails `soma check`.

**"Eventually" is a statement about the graph.** It says no path avoids the
target forever; it does not say anyone will call your handlers. A payment can
sit in `authorized` until someone acts.

## 2. ENFORCED at runtime — always on, cannot be bypassed from Soma code

| Mechanism | Guarantee |
|---|---|
| Memory invariants | checked **before** every `set`, bracket write, `push` and `delete` (for `size`). A violating write raises (`kind "invariant"`) and the slot is unchanged. `soma verify` lists each write it could not prove as *runtime-checked*. |
| Transitions | `transition()` to an undeclared edge raises `invalid_transition` with the valid targets. Guards raise `guard_failed`. |
| **Atomic handlers** | a handler that raises leaves nothing behind: its memory writes and transitions are rolled back. A failing `try { }` block is rolled back to where it started. Consequence: an error means "nothing happened" — to *record* a refusal (a reservation moved to `rejected`), return it as a value instead of raising. |
| **Serialized handlers** | under `soma serve`, top-level handler invocations run one at a time: read-modify-write needs no lock. |
| `require` / `ensure` / `fail` | raise; errors carry a `kind` a caller can branch on. |
| Token budget | `set_budget(N)` stops `think()` when the budget is spent. |
| Exhaustive `match` | a missing sum-type arm is a `soma check` error, not a runtime surprise. |

## 3. NOT COVERED — know this before you rely on Soma

- **Data-dependent rules inside handlers** ("amount ≤ order total", "no refund
  after 30 days"). They are your `if`s and guards. Guards are enforced at
  runtime, not proven; the model checker keeps guarded edges.
- **Cross-cell composition.** Verification is per cell. "stock was taken iff
  the reservation is held" across two cells is tested, not proven (atomic
  handlers do cover the rollback of both cells' writes within one invocation).
- **Conservation / aggregate properties** ("the sum of balances never
  changes"). An invariant sees one written value at a time.
- **Effects outside the process** are not rolled back: HTTP calls, `think()`,
  emitted events, files.
- **Untyped records.** A record is a map: a typo'd field name reads as `()`.
  Ordering against `()` raises, equality does not.
- **Authentication, authorization, TLS, rate limiting**: `soma serve` has none.
  Put it behind a reverse proxy.
- **Throughput.** Handlers are serialized and the interpreter walks the AST;
  `[native]` is for numeric kernels only.
- **Maturity.** Experimental, one author, one package in the registry. The
  verifier itself is tested (adversarially, with mutation and differential
  runs) but not mechanically verified.
