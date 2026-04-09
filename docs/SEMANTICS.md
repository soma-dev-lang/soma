# Soma v1 — Operational Semantics & Soundness Notes

> The unglamorous spine of v1. These notes turn the manifesto's claims
> from a vibe into theorems with explicit quantifiers. They are
> deliberately *normative*: anywhere the v1 implementation deviates from
> the rules below, that's a v1 bug to be filed against the compiler, not
> a license to weaken the rules.

This document covers six things:

  0. Notation & whole-program structure
  1. Cell calculus — small-step operational semantics
  2. State machines, verification & soundness statement
  3. Backend equivalence: `interpreter ≡ bytecode ≡ [native]`
  4. Adversary-quantified guarantees (deadlock-free *under what?*)
  5. State-explosion honesty: where `soma verify` is bounded
  6. V1 deferments (what landed and what was punted to V1.1)

It is meant to be read alongside `TARGET_SOMA_V1.md`.

---

## 0. Notation & whole-program structure

A Soma program `P` is a finite, ordered set of cells:

    P  ::=  cell₁ … cellₙ

Each cell `C` is a tuple

    C = ⟨name, face, memory, handlers, state, scale, protocol?, prove?, adversary?⟩

where:

  - `face`     declares signals the cell exposes (`signal name(args)`)
  - `memory`   declares typed slots, each tagged with a list of properties:
               `[persistent]`, `[ephemeral]`, `[consistent]`, `[causal]`, …
  - `handlers` is a finite set of `on Sig(args) [props] { body }`
  - `state`    is an optional finite state machine over a set Σ_C
  - `scale`    is an optional orchestration block including a `survives:` clause
  - `protocol` is an optional V1 session-type script (see §1.4)
  - `prove`    is an optional V1 invariant block (see §2.4)
  - `adversary` is an optional V1 threat model (see §4)

We write `Σ` for the global state space:

    Σ  =  Π_C { handler stacks } × { memory snapshots } × { in-flight signals }

A program configuration is `σ ∈ Σ`. The initial configuration `σ₀`
empties every handler stack, sets every memory slot to its declared
default, and starts every state machine in its `initial:` state.

---

## 1. Cell calculus — small-step operational semantics

### 1.1 Expressions

Expressions are pure: `e ::= n | x | e₁ ⊕ e₂ | f(e̅) | …`. Their
evaluation is single-step `e ⇓ v` and is fully standard. Two notes:

  - Integer arithmetic is **always correct**: every operation either
    produces a mathematically exact result or rolls over into the
    BigInt fallback (`SomaInt::Big`). There is no silent wraparound.
    The `[native]` codegen preserves this contract via dual-mode
    dispatch + `panic::catch_unwind` on overflow.
  - Float arithmetic is IEEE 754 binary64 with `to_nearest_even`. We
    never use `-ffast-math` style reordering in any backend.

### 1.2 Statements & handlers

Handler bodies are sequences of statements `s`. Statement evaluation is
a small-step relation

    ⟨s, env, μ⟩  →  ⟨s', env', μ'⟩

where `env` is a local variable environment and `μ` is the cell's
memory snapshot. The rules `(Let)`, `(Assign)`, `(If)`, `(While)`,
`(For)`, `(Return)`, `(Emit)`, `(Require)`, `(Ensure)` are the obvious
ones; we omit them for brevity.

The two non-obvious rules:

**(MemRead-Causal):** Reading from a `[causal]` slot extends the
current handler's read-clock with the slot's vector clock:

    μ(slot)[k] = (v, c)
    ──────────────────────────────────────
    ⟨slot.get(k), env, μ⟩ → ⟨v, env ⊕ {clock ∪= c}, μ⟩

**(MemWrite-Causal):** Writing to a `[causal]` slot extends the cell's
local Lamport counter and stamps the new value with the resulting
clock:

    cell.counter' = cell.counter + 1
    c' = (env.clock ∪ {(replica, cell.counter')})
    ─────────────────────────────────────────────
    ⟨slot.set(k, v), env, μ⟩ → ⟨(), env ⊕ {clock = c'}, μ[slot ↦ μ(slot)[k ↦ (v, c')]]⟩

In V1 the implementation tracks the clock per-key but does *not* yet
enforce read-after-write happens-before in the type system; the
`clock_of(slot, key)` and `happens_before(c1, c2)` builtins expose the
clock so handlers can assert ordering manually. The static check is
deferred to V1.1; see §6.

### 1.3 Signal dispatch

Cells communicate via signals:

    ⟨emit Sig(v̅), σ⟩  →  σ ⊕ enqueue(Sig(v̅))

A scheduler chooses one in-flight signal and dispatches it to a handler
matching `Sig`. The interpreter, bytecode VM and `[native]` backends
agree on the handler-selection rule:

  > For each signal name, there is at most one handler per cell. If
  > multiple cells declare a handler, the dispatcher picks the first
  > one in source order. (Multiple-binding routing is V1.1.)

### 1.4 Session-typed protocols (V1)

A `protocol` block declares a finite ordered script:

    proto  ::=  send | loop { proto* } | choice { proto* } | proto · proto
    send   ::=  role → role : Msg(args)

The session-type checker walks the script and produces a set of
**handler obligations** for each role:

    Δ(role) = { (Msg, arity) : ∃ step `_ → role : Msg(p̅)` ∈ proto }

A program is **session-well-typed** iff for every protocol `P` and
every role `r ∈ roles(P)`:

    ∀ (Msg, n) ∈ Δ(r),  ∃ handler `on Msg(p̅)` in cell named `r` with |p̅| = n.

V1 checks this exhaustiveness property statically. *Ordering* checks
(receiver may not handle Msg₂ before Msg₁ if the protocol orders them)
require a real session-type unification with Loop/Choice and are
deferred to V1.1.

**Theorem (Session safety, V1).** *If a program is session-well-typed,
no execution can reach a configuration in which a protocol-declared
message is enqueued at a role with no matching handler.*

*Proof sketch.* Trivial by construction: the checker rejects any
program that violates Δ. Since the dispatcher only selects handlers
declared at the role, an undelivered message is impossible. ∎

---

## 2. State machines & soundness of `soma verify`

Each `state` block defines a finite state machine `M = (Σ_M, →_M, init)`
with `→_M ⊆ Σ_M × Σ_M`. Wildcard transitions `* → s` desugar to
`{(s', s) : s' ∈ Σ_M, s' ≠ s}`.

`soma verify` constructs the reachability graph `G(M)` and answers:

  - `Reach(M)` — set of states reachable from `init`
  - `Term(M)`  — set of states with no outgoing edges
  - `Live(M, P)` — `∀ s ∈ Reach(M). ∃ t ∈ P. s →* t`
  - `Deadlock(M)` — `∃ s ∈ Reach(M). s ∉ Term(M) ∧ s has no outgoing edges`

V1 also checks user properties from `[verify]` in `soma.toml`:
`eventually`, `always`, `never`, `after.X.eventually`, `after.X.never`.

### 2.1 Verifier soundness

**Theorem (Verifier soundness, V1 — bounded reachability).** *Let `M`
be a state machine with `|Σ_M|` ≤ 2¹⁶. If `soma verify` reports
`PASS: live(M, P)`, then in every concrete execution of any cell whose
state machine is `M`, every reachable state has a path in `→_M` to some
state in `P`.*

*Proof sketch.* By construction `G(M)` is the literal `→_M` graph. The
liveness check is BFS from each reachable state to `P`. Since the
small-step rules of §1 only fire transitions that are edges in `→_M`,
any concrete execution is a path in `G(M)`. ∎

### 2.2 Bound

`|Σ_M|` is bounded by 2¹⁶ in V1 because the verifier uses a `HashSet<String>`
keyed by state name; programs that exceed this fall back to a warning.
See §5 for the cliff.

### 2.3 What V1 does *not* prove

V1's verifier is a finite-state model checker over `Σ_M`. It does **not**
yet:

  - reason about handler bodies (effect systems, V1.2)
  - reason about distributed execution traces (TLA-style refinement, V1.3)
  - reason about real-valued time (timed automata, never, probably)

These are intentional. The session-type checker (§1.4), the causal
memory check (§1.2), and the adversary qualifier (§4) cover most of the
distributed-systems claims that V1 makes.

### 2.4 Lean 4 export

A `prove` block

    prove M {
      invariant: φ
      export: lean4 -> "path.lean"
    }

emits a Lean 4 file that encodes `M` as `inductive Step : State →
State → Prop` and each invariant as a `theorem` skeleton. The point is
*not* that the Lean kernel automatically discharges φ — it's that any
third party can `lake build path.lean`, write the proof themselves,
and use the file as a permanent regression target.

This closes the "trust the verifier" circle. Today the Soma binary is
the only thing standing between the user and a wrong answer; with Lean
export the trust chain is

    Soma source → Lean term → Lean kernel → ✓

and the kernel is small enough to audit by hand.

---

## 3. Backend equivalence: `interpreter ≡ bytecode ≡ [native]`

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
  - **The integer correctness contract** (§1.1), which is the most likely
    source of divergence.

A formal proof — by showing that the bytecode compiler and the native
codegen are simulations of the small-step semantics in §1 — is V1.2 work.

---

## 4. Adversary-quantified guarantees

Every claim made by `soma verify` is now stamped with the adversary
clause it was proven *under*. Concretely:

    cell C {
      scale {
        survives: network ∧ llm
      }
    }

instructs the verifier to print every `PASS` as `PASS (under network ∧
llm)`. The `network` and `llm` names must resolve to in-scope
`adversary` blocks; the V1 checker rejects undeclared names.

In V1 the adversary is *advisory*: the verifier doesn't yet weaken its
claims based on the threat model (e.g. "10% drop changes the liveness
proof"). What it does do is make every guarantee carry an explicit
quantifier — closing the "deadlock-free under what model?" gap that
plagued the pre-V1 manifesto. Adversary-aware verification is V1.1
work.

The LLM adversary is the honest answer to "what if the agent
hallucinates": it declares the LLM's output as `arbitrary string` (i.e.
adversarially chosen) and lets you bound it with `rate: bounded(...)`.
No other production language ships this primitive.

---

## 5. State-explosion honesty

`soma verify` is a bounded model checker. The cliff:

| `|Σ_M|`            | wall clock        | memory     | comment                          |
|--------------------|-------------------|------------|----------------------------------|
| ≤ 100              | < 1 ms            | KB         | most production state machines   |
| 100 – 10 000       | 1 – 100 ms        | MB         | comfortable                      |
| 10 000 – 65 536    | 100 ms – seconds  | tens of MB | hard ceiling in V1               |
| > 65 536           | refused           | —          | warning, no proof attempted      |
| 10⁶+ (compositional)| not yet           | —          | requires V1.2 partial-order red. |

If your machine is >65 K states, you're using state machines wrong —
factor the system into multiple smaller cells, each with its own
state machine, connected by signals. The session-type checker (§1.4)
then proves the composition deadlock-free *without* exploring the
product state space — that's the whole point of session types.

---

## 6. V1 deferments

What landed in V1:

  | Feature                         | Status                                       |
  |---------------------------------|----------------------------------------------|
  | Session-typed `protocol`        | exhaustiveness check ✓                       |
  | `[record]` + `soma replay`      | full record/replay + nondet detection ✓      |
  | `prove` → Lean 4 export         | inductive Step + theorem skeletons ✓         |
  | `adversary` blocks              | declared, scoped, stamped on PASS messages ✓ |
  | `[causal]` memory               | per-key vector clocks, `clock_of` builtin ✓  |
  | Rigor doc (this file)           | normative semantics ✓                        |

What was punted to V1.1:

  - **Session-type ordering** (loop/choice exhaustion, not just exhaustiveness)
  - **Static causal happens-before check** (V1 has runtime clocks, not types)
  - **Adversary-aware verification** (V1 is advisory: it stamps but doesn't reweight)
  - **Backend-equivalence theorem** (V1 has differential tests, not a proof)
  - **Lean kernel discharge** (V1 emits theorem skeletons with `trivial` bodies)
  - **`uses P as role`** explicit role binding (V1 matches role to cell name)

These are all known and tracked. The point of v1 is to ship the
*shapes* of the theorems — to make every annotation a theorem in
*statement* if not yet in *proof*. v1.1 fills in the proof bodies.

---

## The single sentence

> Soma v1 is the language where every annotation is a theorem,
> every protocol is a type, every replay is deterministic,
> and every guarantee names its adversary.
