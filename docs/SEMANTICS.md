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

It is meant to be read alongside `docs/V1.md`.

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

---

## 2. Backend equivalence: `interpreter ≡ bytecode ≡ [native]`

Soma has three execution backends:

  1. **Interpreter** — tree-walking over the AST (`compiler/src/interpreter`)
  2. **Bytecode VM** — stack machine over a custom IR (`compiler/src/vm`)
  3. **`[native]`** — Rust source → `cdylib` per cell (`compiler/src/codegen/native.rs`)

**Conjecture (Backend equivalence).** *For every well-typed Soma program
`P`, every signal `Sig`, every input `v̅`, and every backend `B ∈ {interp,
vm, native}`:*

    run_B(P, Sig, v̅) = run_interp(P, Sig, v̅)

That is: the three backends are observationally equivalent on terminating
programs. Without this property, the 200×–300× `[native]` speedup would be
meaningless — it could be 200× the wrong answer.

**Status in V1.** This is a *conjecture*, not a theorem. We support it via:

  - **Differential testing.** `examples/clbg_corpus/` and the 100 verified
    Soma use cases run under all three backends and the results are
    bit-compared. The `[native]` SomaInt fallback (overflow → BigInt) is
    enforced by `overflow_checks = true` + `panic::catch_unwind`, so any
    arithmetic disagreement panics rather than silently miscalculating.
  - **The integer correctness contract** (§1.2), which is the most likely
    source of divergence.

A formal proof — by showing that the bytecode compiler and the native
codegen are simulations of the small-step semantics in §1 — is V1.2 work.

---

## 3. State-explosion honesty

`soma verify` is a bounded model checker. The cliff:

| `|Σ_M|`            | wall clock        | memory     | comment                          |
|--------------------|-------------------|------------|----------------------------------|
| ≤ 100              | < 1 ms            | KB         | most production state machines   |
| 100 – 10 000       | 1 – 100 ms        | MB         | comfortable                      |
| 10 000 – 65 536    | 100 ms – seconds  | tens of MB | hard ceiling in V1               |
| > 65 536           | refused           | —          | warning, no proof attempted      |

If your machine is >65 K states, you're using state machines wrong —
factor the system into multiple smaller cells, each with its own
state machine, connected by signals. The session-type checker (when it
actually does ordering checks, V1.2) will then prove the composition
deadlock-free *without* exploring the product state space. That's the
whole point of session types — and also the reason V1's exhaustiveness-
only check was theatre: it didn't prove anything new beyond what
`face` blocks already proved.

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
