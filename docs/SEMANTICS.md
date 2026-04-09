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

### 1.7 CTL model checker soundness

Refinement (§1.5) and the runtime transition guard
(`do_transition_for`) together give:

**Lemma 1.7 (Runtime fidelity).** *Every successful runtime
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
