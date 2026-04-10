# Soma v1 — Operational Semantics & Honesty Notes

> The unglamorous spine of v1, after the V1.2 subtraction. These notes
> turn the manifesto's claims from a vibe into theorems with explicit
> quantifiers. Anywhere the v1 implementation deviates from the rules
> below, that's a v1 bug to be filed against the compiler, not a
> license to weaken the rules.

This document covers four things:

  1. Cell calculus — small-step operational semantics
  2. Backend equivalence: `interpreter ≡ bytecode ≡ [native]`
  3. State-explosion honesty: where `soma verify` is bounded
  4. The V1.2 subtraction and what it teaches

It is meant to be read alongside `docs/V1.md`. For the **CTL model
checker soundness theorem** that turns the temporal property checks
into a real claim, see `docs/SOUNDNESS.md`. For the **adversary model**
each property holds against, see `docs/ADVERSARIES.md`. For the
**executable witness** of backend equivalence on the intersection
corpus, see `compiler/tests/equivalence.rs`. For the **measured**
state-explosion cliff, see `docs/rigor/results/state_explosion.md`.

> **2026-04-10 rigor pass status:** the CTL soundness gap from the
> hard-coded depth-50 DFS bound was found and closed in this revision.
> Repro and regression test:
> `compiler/tests/rigor_eventually_long_chain.rs`. Bench data and
> equivalence harness now back the §2 and §3 claims with reproducible
> numbers. The rigor scorecard is at `docs/rigor/README.md`.

---

## 1. Cell calculus — small-step operational semantics

### 1.1 Whole-program structure

A Soma program `P` is a finite, ordered set of cells:

    P  ::=  cell₁ … cellₙ

Each cell `C` is a tuple

    C = ⟨name, face, memory, handlers, state, scale⟩

where:

  - `face`     declares signals the cell exposes (`signal name(args)`)
  - `memory`   declares typed slots, each tagged with a property bag:
               `[persistent]`, `[ephemeral]`, `[consistent]`, `[eventual]`,
               `[local]`
  - `handlers` is a finite set of `on Sig(args) [props] { body }`
  - `state`    is an optional finite state machine over a set Σ_C
  - `scale`    is an optional orchestration block

A program configuration is `σ ∈ Σ`. The initial configuration `σ₀`
empties every handler stack, sets every memory slot to its declared
default, and starts every state machine in its `initial:` state.

### 1.2 Expressions

Expressions are pure: `e ::= n | x | e₁ ⊕ e₂ | f(e̅) | …`. Their
evaluation is single-step `e ⇓ v` and is fully standard. Two notes:

  - **Integer arithmetic is always correct.** Every operation either
    produces a mathematically exact result or rolls over into the
    BigInt fallback (`SomaInt::Big`). There is no silent wraparound.
    The `[native]` codegen preserves this contract via dual-mode
    dispatch + `panic::catch_unwind` on overflow. This is *the*
    correctness contract Soma makes — it's the reason a Soma program
    can never give a wrong arithmetic answer that compiles.
  - **Float arithmetic is IEEE 754 binary64 with `to_nearest_even`.**
    We never use `-ffast-math` style reordering in any backend.

### 1.3 Statements & handlers

Handler bodies are sequences of statements `s`. Statement evaluation is
a small-step relation

    ⟨s, env, μ⟩  →  ⟨s', env', μ'⟩

where `env` is a local variable environment and `μ` is the cell's
memory snapshot. The rules `(Let)`, `(Assign)`, `(If)`, `(While)`,
`(For)`, `(Return)`, `(Emit)`, `(Require)`, `(Ensure)` are the obvious
ones; we omit them for brevity.

### 1.4 Signal dispatch

Cells communicate via signals:

    ⟨emit Sig(v̅), σ⟩  →  σ ⊕ enqueue(Sig(v̅))

A scheduler chooses one in-flight signal and dispatches it to a handler
matching `Sig`. The interpreter, bytecode VM and `[native]` backends
agree on the handler-selection rule:

  > For each signal name, there is at most one handler per cell. If
  > multiple cells declare a handler, the dispatcher picks the first
  > one in source order. (Multiple-binding routing is V1.2.)

### 1.5 Refinement: handler bodies cannot lie to the state machine

Soma's tagline is *"the specification is the program"*. Before V1.3,
it was half true: the `state` block was a spec, and the handler
bodies were code, and `soma verify` only proved CTL properties about
the spec (the picture). The handler bodies could call `transition()`
with whatever target they wanted. They could lie. They no longer can.

For each cell with a `state M { … }` block, define:

  - `States(M)` = the set of state names mentioned anywhere in `M`
    (the initial state, every `from`, every `to`, excluding the
    wildcard `*`)
  - `Transitions(M)` = the set of declared `(from, to)` edges in `M`
  - For each handler `H`, let `Effect(H)` = the multiset of pairs
    `(target, path_condition)` where `target` is the literal target
    of every static `transition("inst", target)` call inside `H`'s
    body, and `path_condition` is the chain of `if` guards leading
    to that call site.

**The refinement check** rejects a program iff any of the following hold:

  1. **Undeclared target.** ∃ handler `H`, ∃ `(target, _) ∈ Effect(H)`,
     such that `target ∉ States(M)`. The compiler reports the handler
     name, the bad target, and the file location.
  2. **Dead transition.** ∃ `(from, to) ∈ Transitions(M)` such that
     `to ∉ ⋃ₕ { target : (target, _) ∈ Effect(H) }`. Reported as a
     warning (the spec might be aspirational), unless any handler has
     a *dynamic* transition target — in which case dead-transition
     warnings are suppressed because we cannot be sure they're real.

**Theorem (Refinement soundness, V1.3 — syntactic).** *If `soma verify`
emits no `UndeclaredTarget` finding for cell `C`, then no execution
of `C` can reach a state outside `States(M)` via a static
`transition()` call. Equivalently: every literal target of every
`transition()` call in `C`'s handler bodies is a state the verifier
already proved CTL properties about.*

*Proof sketch.* By construction. The walker in
`compiler/src/checker/refinement.rs` recurses into every statement
and expression of every handler body — `If`, `Else`, `For`, `While`,
`Match`, `Try`, lambda bodies, pipes, ifexprs — and emits a finding
for every literal-target `transition()` call whose target is not in
`States(M)`. Since the only call sites that *can* reach a non-literal
target are flagged as `DynamicTarget`, the user is told exactly when
the soundness claim degrades. ∎

**What V1.3 does NOT yet prove** (V1.4 work):

  - **Source-state correctness.** A handler doesn't statically know
    which state the machine was in when it was called — that's runtime
    info. V1.3 only checks the *target* of each `transition()` call,
    not that the call is legal *from the current state*. SMT-backed
    control-flow analysis closes this gap.
  - **Guard implication.** When the state block says
    `pending → authorized when amount > 0` and the handler writes
    `if amount > 0 { transition(…) }`, V1.3 records both as text but
    doesn't prove implication. SMT.
  - **Dynamic targets.** `transition(id, target_var)` where
    `target_var` is computed at runtime: V1.3 emits a warning that
    refinement coverage is incomplete here.

These are intentional. V1.3 is the *syntactic, sound* refinement
check — incomplete (some real bugs slip through dynamic targets and
guard arithmetic) but it never falsely accuses a correct program.
The incompleteness is documented per-handler in the verifier output
so the user can see exactly which handlers got the strong check.

This is the WOW feature the manifesto was claiming. Before V1.3,
"the specification is the program" was a poster. Now it's a theorem.

### 1.6 Recording & replay (the one V1 feature kept after V1.2 subtraction)

A cell run under `soma run --record` writes one JSON-line entry per
handler invocation to `<source>.somalog`. The entry contains:

    {
      "v": 1,
      "ts": <unix-ms>,
      "cell": <cell name>,
      "handler": <signal name>,
      "args": <serialized arg list>,
      "result": <serialized return value>,
      "nondet": [<names of nondeterministic builtins called>]
    }

`soma replay <source>` re-runs each entry against a fresh interpreter
and bit-compares the result. **Theorem (Replay determinism, V1).**
*If a recorded handler call returns value `v` during recording and
the same handler is replayed against an identically-initialised
interpreter with the same args, then either:*

  1. *the replay returns the same `v` (verbatim equivalence), OR*
  2. *the replay's `nondet` list contains at least one builtin —
     and replay reports it as a divergence cause with a suggested fix.*

*Proof sketch.* The `call_builtin` dispatch records every call to a
nondeterministic builtin (`now`, `now_ms`, `timestamp`, `today`,
`date_now`, `random`, `rand`) before delegating. If the recorded
nondet list is empty, every call in the handler body was either pure
or to a deterministic builtin; given the same args and initial memory,
the body produces the same value. ∎

### 1.7 Memory-budget proof obligation (V1.4)

`scale { memory: "256Mi" }` was an advisory until V1.4. The V1.4
budget checker (`compiler/src/checker/budget.rs`) turns it into a
**compile-time proof obligation**: `soma check` either proves
`peak_memory(C) ≤ B` for the declared budget `B`, or it produces a
concrete bound that exceeds `B` and fails, or it downgrades to an
advisory listing the unbounded builtins that prevent the proof.

**The formula.**

```
peak_memory(C) ≤ slot_sum(C)
              + max_{h ∈ handlers(C)} handler_peak(h)
              + state_machine_bound(C)
              + C_runtime
```

The crucial detail is that the handler contribution is the **maximum**
across handlers, not the **sum**. A previous design (the original
proposal in `discussions/budget_proposal.md`) used `sum`, which was
unsound by a factor of `|handlers|`: it modelled "every handler holds
its own stack frame simultaneously", but the Soma runtime only ever
executes one handler at a time per cell instance. The unit test
`budget_pass_many_handlers_uses_max_not_sum`
(`compiler/tests/rigor_budget.rs`) pins this corrected formulation.

**Slot bound.** For each `Map<K, V> [capacity(N), max_key_bytes(K),
max_value_bytes(V)]`, the contribution is `N × (K + V + 64)` bytes
(64 bytes for per-entry header). Missing annotations fall back to
conservative defaults:

| annotation         | default      |
|--------------------|-------------:|
| `capacity`         |       10 000 |
| `max_key_bytes`    |          256 |
| `max_value_bytes`  |        4 096 |
| `max_element_bytes`|        4 096 |

**Handler peak.** A recursive walk over the handler body (statements
and expressions) sums per-allocation costs:

  - Each `list(...)` allocates `64 + n × 16` bytes;
  - Each `map(...)` allocates `256 + (n/2) × 64`;
  - Each `with(m, k, v)`, `push(l, v)` adds a small constant;
  - String literals contribute `len + 32` bytes;
  - For loops multiply the body cost by the iteration count, taken
    from (in priority order): an explicit `[loop_bound(N)]`
    annotation, a literal `range(0, N)` argument, or
    `DEFAULT_CAPACITY = 10 000` as a conservative fallback;
  - Branches (`if/else`, `match`) take the `max` of arm costs;
  - Calls to *unbounded builtins* (`think`, `from_json`, `http_get`,
    `read_file`, `delegate`, …) propagate `Unbounded(reason)` up the
    cost lattice.

A single per-handler stack overhead of 8 MiB is added at the end.
With the max-not-sum aggregate, the cell pays for **one** active
stack frame at a time.

**State machine bound.** Each `state foo [max_instances(N)] { … }`
contributes `N × 256` bytes (256 bytes per instance ID entry,
matching the storage backend's per-row overhead).

**Runtime constant.** `C_runtime = 16 MiB` covers the interpreter,
the storage backends, the HTTP server when present, the agent
trace, and string interning.

**Cost lattice.** The analyzer operates over an abstract cost type
`Cost = Bounded(n) | Unbounded(reasons)` with three operations:

  - `plus`: sequential composition (`Bounded(a) + Bounded(b) = Bounded(a+b)`)
  - `max`: branching join (`Bounded(a) ⊔ Bounded(b) = Bounded(max(a,b))`)
  - `times n`: loop unrolling (`Bounded(a) × n = Bounded(a × n)`)

`Unbounded` absorbs everything: `plus`, `max`, and `times` with an
`Unbounded` operand return `Unbounded` (the reasons accumulate).

**Theorem 1.7 (Budget soundness, V1.4 — Tier 1).** *Let `C` be a cell
with declared budget `B`. If `soma check` reports `BudgetOk` for `C`,
then for every execution of `C` against a local-adversary call schedule
(see `docs/ADVERSARIES.md` §1) that does not invoke any builtin in
`unbounded_builtin_reason()` (see `compiler/src/checker/budget.rs`),
the peak resident memory of the cell does not exceed `B`.*

*Proof sketch.* The bound is computed by a closed-form recursive
walk of the AST. For each construct, the per-construct cost is at
least the maximum number of bytes the runtime can allocate during
that construct's evaluation, by inspection of
`compiler/src/interpreter/builtins/*.rs`. Sequential composition is
sound by `cost_composition_sound` (mechanized in
`docs/rigor/coq/Soma_Budget.v`). Branching is sound by `cmax_lub`
(mechanized). Loop unrolling is sound by `ctimes_monotone_l` and
the explicit `[loop_bound(N)]` annotation when present, or the
literal `range(0, N)` form, or the conservative default. The cell-
level aggregate uses **max** over handlers (not sum), which is
sound under the local adversary because the runtime only schedules
one handler at a time per cell instance. ∎

**Mechanization.** The cost lattice operations and the headline
composition theorem are mechanically verified in
`docs/rigor/coq/Soma_Budget.v` (Rocq 9.1.1, no axioms, no
`Admitted`, 6 theorems all `Closed under the global context`).
What remains on paper is the bridge from the abstract `Cost` type
in Coq to the AST walk in Rust — that bridge is by inspection of
`compiler/src/checker/budget.rs::expr_cost` and `stmt_cost` matching
the operations of the abstract lattice.

**Empirical witness.** `docs/rigor/bin/bench_budget_runtime.sh` runs
each test cell under a real OS memory cap set to
`proven_peak + 32 MiB safety margin` and asserts that the cell
completes without OOM. Three cells (trivial, many-handlers, state-
machine) pass at HEAD; results in
`docs/rigor/results/budget_runtime.md`.

**Production cells with proven bounds.** Two cells in the rebalancer
production code carry `scale { memory: ... }` declarations and pass
the budget proof:

  - `rebalancer/lib/optimizer.cell::Optimizer`: peak ≤ 69.89 MiB ≤ 128 MiB
  - `rebalancer/lib/alpha.cell::Alpha`: peak ≤ 62.49 MiB ≤ 128 MiB

Both are pure cells (no LLM, no HTTP, no JSON parsing of unknown
inputs). The orchestrator cells (Portfolio, Compliance, Commentary)
do not yet declare budgets because they call unbounded builtins; the
checker would emit advisories listing the call sites if asked.

**Tiers (where this lives in the proposal staging).**

  - **Tier 1 (V1.4, this section)**: slot-level bounds + handler-body
    walk + cost lattice composition + corrected max-not-sum aggregate
    + mechanized soundness for the lattice. *Shipped.*
  - **Tier 2 (future)**: smarter loop bounds (length-of-list inference,
    SMT-backed loop count inference), per-statement live-vs-dead
    allocation tracking (currently we conservatively sum within a
    handler body), recursion-depth bounds via well-founded measure.
  - **Tier 3 (research-grade)**: Hoffmann's RAML potential-function
    technique for amortized per-request bounds.

**What V1.4 does not yet prove:**

  - Cells that call unbounded builtins (the analyzer correctly
    degrades to ADVISORY rather than claiming a false bound).
  - Cross-cell flows: a cell that `delegate`s to another cell ships
    data to the receiver, and the receiver's budget is checked
    independently. The aggregate fleet bound is the sum of per-cell
    bounds.
  - Concurrent handler execution (the local-adversary assumption).
  - The bridge from abstract `Cost` to operational allocation
    semantics in Coq (V1.5 work).

For the full open-question list, see `docs/rigor/README.md` and
`docs/SOUNDNESS.md` §6.

### 1.8 CTL model checker soundness

Refinement (§1.5) and the runtime transition guard
(`do_transition_for`) together give:

**Lemma 1.8 (Runtime fidelity).** *Every successful runtime
`transition(id, target)` call lies in the abstract state machine `→`
of some declared state block.*

This is the trust base on which the temporal property checker rests.
The full theorem statements for `Always`, `Never`, `DeadlockFree`,
`Mutex`, `Eventually`, and `After` — along with the assumptions
they depend on, the gaps that remain, and the **soundness fix
applied 2026-04-10** that closed the depth-50 false-positive on
liveness — are in **`docs/SOUNDNESS.md`**.

The short version:

  - **Safety properties** (`Always`, `Never`, `DeadlockFree`, `Mutex`)
    are sound *and* complete with respect to the abstract state
    machine. Closed-form proof in SOUNDNESS §3.1.
  - **Liveness properties** (`Eventually`, `After`) are sound after
    the depth-bound fix. The DFS bound is now `|Reach(G)| + 1`, which
    suffices to explore every acyclic path; before, it was a hard-
    coded `50`, which produced false positives on long chains.
    Regression test: `compiler/tests/rigor_eventually_long_chain.rs`.
  - **Guard predicates** are over-approximated as `true`. This is
    sound for safety, conservative for liveness. SMT-backed guard
    reasoning is V1.4.

The adversary model under which each property holds is in
**`docs/ADVERSARIES.md`**.

---

## 2. Backend equivalence: `interpreter ≡ bytecode ≡ [native]`

Soma has three execution backends:

  1. **Interpreter** — tree-walking over the AST (`compiler/src/interpreter`)
  2. **Bytecode VM** — stack machine over a custom IR (`compiler/src/vm`),
     invoked via the deprecated `--jit` flag, **feature-incomplete**
  3. **`[native]`** — Rust source → `cdylib` per cell (`compiler/src/codegen/native.rs`)

**Conjecture (Backend equivalence).** *For every well-typed Soma program
`P` in the intersection of features supported by both the interpreter
and the bytecode VM, every signal `Sig`, every input `v̅`, and every
backend `B ∈ {interp, vm}`:*

    run_B(P, Sig, v̅) = run_interp(P, Sig, v̅)

That is: the two interpreted backends are observationally equivalent on
terminating programs in the intersection corpus. Without this property,
the speedup of any backend would be meaningless — it could be the wrong
answer faster.

**Status in V1.** This is a *conjecture* with an **executable witness**.

  - **Differential testing harness.** `compiler/tests/equivalence.rs`
    runs a curated corpus of programs through both backends and asserts
    bit-equal output, every call to `cargo test --test equivalence`. The
    corpus exercises the integer correctness contract (§1.2) — including
    `25!` overflow → BigInt promotion, which is the most likely source
    of divergence — plus arithmetic, control flow, recursion, lists, maps,
    string ops, and multi-arg dispatch.
    As of 2026-04-10: **16/16 cases pass**.
  - **Documented gaps.** The intersection corpus deliberately excludes
    features the VM does not yet implement (string interpolation, pipes
    with lambdas, complex pattern matching, `delegate`, `transition`,
    LLM builtins). The list is at the bottom of `equivalence.rs` and
    must grow monotonically with the VM's capability.

A formal proof — showing that the bytecode compiler is a simulation of
the small-step semantics in §1, and that the native codegen preserves
the same observation function — remains V1.4 work. Until then, the
empirical witness is what stands between Soma and a backend miscompile.

---

## 3. State-explosion honesty

`soma verify` is a bounded model checker. The previous version of this
doc had a hand-waved cliff table; the rigor pass replaced it with
**measured** numbers from `docs/rigor/bin/bench_state_explosion.sh`.

The bench generates synthetic state machines of three topologies:

  - **Linear chain**: `s0 → s1 → … → s_{N-1} → done`. Worst case for
    path search.
  - **Diamond**: bounded fan-out × fan-in, k ≈ √N layers. Best case
    for reachability.
  - **Cyclic**: linear chain plus one back-edge that creates a cycle
    so `eventually(s_{N-1})` has a counter-example. Measures the cost
    of the failing-property path.

Latest numbers (from `docs/rigor/results/state_explosion.md`,
single-run wall time on a single machine — characterise the cliff,
not microbench precision):

| `|Σ_M|` | linear  | diamond | cyclic (counter-ex) |
|--------:|--------:|--------:|--------------------:|
| 10      | 30 ms   | 21 ms   | 21 ms               |
| 100     | 20 ms   | 20 ms   | 21 ms               |
| 1 000   | 61 ms   | 21 ms   | 76 ms               |
| 2 000   | 193 ms  | 22 ms   | 249 ms              |
| 5 000   | 1.1 s   | 25 ms   | (skipped)           |
| 10 000  | 4.5 s   | (skipped) | (skipped)         |

**The honest cliff:**

  - **≤1 000 states**: well under 100 ms on every topology. Fits any
    production state machine. This is the comfortable zone.
  - **1 000 – 5 000 states**: linear chains hit the seconds range
    here. Diamonds stay flat because BFS reachability is essentially
    O(V+E) and the diameter is bounded.
  - **5 000 – 10 000 states**: the linear cliff. 10K states on a
    linear topology is 4.5 seconds — still verifiable in CI, but
    no longer interactive. **This is where you should be factoring
    your state machine into smaller compositional pieces.**
  - **>10 000 states**: not measured. Extrapolating from the linear
    topology suggests roughly tens of seconds for 20K and minutes for
    50K. There is no hard refusal — `soma verify` will keep trying.

**The previous doc was wrong about two things.** It claimed
"100 – 10 000 states: 1 – 100 ms" — that's true for diamonds but
**false for linear chains**, which hit 1 second at 5 K states. And it
claimed "> 65 536 states: refused" — there is no such refusal in the
implementation. The corrected story above is what the bench actually
measures.

If your machine is in the cliff zone, you're using state machines
wrong: factor the system into multiple smaller cells, each with its
own state machine, connected by signals. Composition verification —
the V1.2 subtraction story — would have helped here, and is tracked
as future work in `SOUNDNESS.md` G3.

---

## 4. The V1.2 subtraction and what it teaches

V1 originally proposed five features. Four were subtracted in V1.2
because they didn't bring value over what already existed.

| Feature             | Why it didn't survive                                            |
|---------------------|------------------------------------------------------------------|
| `protocol` blocks   | Exhaustiveness only — already covered by `face { signal X }`     |
| `prove` → Lean 4    | Theorem stubs with `trivial` bodies — aspirational, not real     |
| `adversary` blocks  | Stamped output without actually modeling the threats — misleading|
| `causal` memory     | Vector clocks with one replica = sequence numbers + ceremony     |
| `--record` / replay | **Kept.** Real implementation, real value, honest theorem.       |

The principle:

> **A feature ships when it works end-to-end and brings value over
> what existed. Stubs, hooks, and ceremony do not count.**

This is the *operational* counterpart to the V1.1 inversion ("brackets
are compiler output, not user input"): both are subtraction. V1.1
removed annotations the compiler could infer. V1.2 removed features
the implementation didn't actually deliver. Both releases shipped
*less code* than the previous one — and that's the right direction.

### Things deferred to V1.2 *with intent to actually deliver*

  - **Session-type ordering** (loop/choice exhaustion, not just exhaustiveness)
  - **Adversary-aware verification** (model drops/reorders/partitions in the state space)
  - **Distributed causal memory** (vector clocks become useful with >1 replica)
  - **Proof-carrying Lean 4 export with discharged proofs** (not stubs)
  - **Backend-equivalence theorem** (formalise the conjecture in §2)
  - **`[pure]` / `[persistent]` / `[consistent]` inference** (continue subtracting brackets)

These will only ship when they bring value. If a year from now we
realise we still can't make session-type ordering work cleanly, we'll
say so honestly and move it to V1.3 or drop it entirely.

---

## The single sentence

> Soma v1 is the language where every production incident is a single
> command away from being replayed deterministically on your laptop —
> and where every other promise is held back until the implementation
> can actually carry it.
