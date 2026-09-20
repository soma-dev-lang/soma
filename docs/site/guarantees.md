# What Soma guarantees — and what it does not

Three columns, no adjectives. If a claim about Soma is not in the first two
columns, it is not a guarantee.

## 1. PROVEN statically — by `soma verify`, before the program runs

On the **state machine** of each cell (a finite graph, model-checked):

| Property | Meaning | How to ask |
|---|---|---|
| Reachability | every declared state can be reached from `initial` | always |
| Deadlock-freedom | no reachable non-final state without an exit | always |
| Liveness | every state can reach some final state — or, for a REACTIVE machine with no final state (a pump, an interlock), every reachable state can return to the initial one | always (a cyclic machine that cannot return home is a ⚠) |
| Stored history | `[verify]` properties are proven for the CURRENT graph: an instance that took an edge an older version of the program allowed keeps that history (the start-up audit reports only instances in undeclared states) — migrate or reset those instances when you remove an edge; one `soma serve` per program and data directory runs the every/after ticks (a second one logs that it does not, and takes over within ~2 s if the first stops) | — |
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
| Termination | in every cell: bounded loops, recursion on an Int argument that decreases towards a lower-bound base case (`if n <= 0 { return … }`), no call cycles; anything else is a ⚠ (a failure under `--strict`) |
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
| Memory invariants | checked **before** every `set`, bracket write, `push` and `delete` (for `size`). A violating write raises (`kind "invariant"`) and the slot is unchanged. `soma verify` lists each write it could not prove as *runtime-checked*. On a Float slot a computed value may be NaN (`sqrt(-1.0)`, `inf - inf`), which fails every comparison: such writes are runtime-checked, never "proven". The prover reasons about numbers: an invariant on a FIELD of a record-valued slot (`plans.price >= 0`) or over a structure (`sum_by(inv.lines, "amount") == inv.total`) is runtime-checked, so it is a ⚠ under `verify --strict` — keep a number you need proven in its own `Map<String, Int>` slot. |
| Transitions | `transition()` to an undeclared edge raises `invalid_transition` with the valid targets. Guards raise `guard_failed`. |
| `[immutable]` slots | an entry, once written, never changes: a List slot only grows by `push`, a Map slot only gains new keys — overwriting or deleting an entry raises `kind "invariant"` and the slot is unchanged (an append-only audit log). |
| **Atomic handlers** | a handler that raises leaves no memory write and no transition behind: they are rolled back. Effects outside the program (an HTTP call, a file, an email, an LLM call) are not undone. A failing `try { }` block's slot writes, transitions and pushes are rolled back to where it started (plain locals it assigned keep their value). Consequence: an error means "nothing happened" — to *record* a refusal (a reservation moved to `rejected`), return it as a value instead of raising. |
| **Cross-cell rules** | NOT verified: proofs are per cell. `soma verify` now NAMES each handler that transitions its own machine while acting on another cell with a machine (`note: cross-cell: …`) — enforce those rules yourself with a `require` that reads the other cell (`require Subjects.status(id) != "withdrawn" else Withdrawn`). |
| **Invariants between slots** | `invariant (reserved ?? 0) <= (stock ?? 0)` in a cell's `memory` holds on every write to either slot: the written one is its new value, the others are read at the same key. Enforced at run time, never proven by induction (verify lists it as runtime-checked). |
| **Serialized handlers** | within each `soma serve` process, top-level handler invocations run one at a time: read-modify-write needs no lock. Exception: a `[task]` handler (or tick) runs as steps and waits for each `think()` outside the lock — each step is serialized and atomic, the task as a whole is not (see serving.md). |
| `require` / `ensure` / `fail` | raise; errors carry a `kind` a caller can branch on. |
| Token budget | `set_budget(N)` stops `think()` when the budget is spent; a `set_budget` inside a model's tool call can only lower it. |
| Exhaustive `match` | a missing sum-type arm is a `soma check` error, not a runtime surprise. |

## Cluster scope (Soma 2.8.2)

`scale.shard` selects mutable Map slots for **eventual full replication**.
Typed updates are sent only after the local handler commits; incoming
updates use the handler lock and a local transaction. Local `get`, `keys`,
`values` and `len` read the same replica. Logical versions order conflicts;
deletes retain tombstones, persisted with SQLite data for persistent slots.
Reconnect and periodic state exchange repair missed updates when peers can
communicate again. These behaviors have process-level regression tests;
**the verifier does not prove distributed convergence or fault tolerance**.

`strong` and `causal` declarations fail verification and server startup.
A distributed slot with memory invariants, `immutable`, or List operations
is refused because the runtime cannot preserve its guarantees across nodes.
`replicas` does not provision processes, and `tolerance` does not establish a
quorum. A resource-only scale block leaves memory local. Advisory scheduler
leadership follows live membership and can split during a partition.
`verify --strict` rejects the explicit warning about unproved distribution.
See [cluster.md](cluster.md) for setup and migration.

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
  files, events sent over the `[peers]` bus to another process. (An `emit`
  handled by a cell of the same process IS inside the handler's transaction:
  synchronous, and rolled back with it.)
- **Untyped records.** A record is a map: a typo'd field name reads as `()`.
  Ordering against `()` raises, equality does not.
- **Authentication, authorization, TLS, rate limiting**: `soma serve` has none.
  Put it behind a reverse proxy.
- **Throughput.** Handlers are serialized and the interpreter walks the AST;
  `[native]` is for numeric kernels only.
- **Maturity.** Experimental, one author, one package in the registry. The
  verifier itself is tested (adversarially, with mutation and differential
  runs) but not mechanically verified.
