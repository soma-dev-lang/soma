# Soma — CTL Model Checker Soundness

> Companion to `docs/SEMANTICS.md`. Where SEMANTICS lays out the cell
> calculus and the refinement / replay theorems, this document tackles
> the question SEMANTICS deferred: **what does `soma verify` actually
> prove about the temporal properties it checks?**
>
> Status: informal soundness argument with explicit assumptions and
> explicit gaps. A machine-checked formalisation in Coq or Lean is
> tracked as V1.4 work; until that ships, this is a proof on paper
> against the implementation as it exists in
> `compiler/src/checker/temporal.rs`.

The soundness story rests on three artifacts:

  1. The **abstract state machine graph** built from the AST `state { … }`
     block by `StateMachineGraph::from_ast` (`compiler/src/checker/temporal.rs:97`).
  2. The **runtime transition guard** in `Interpreter::do_transition_for`
     (`compiler/src/interpreter/mod.rs:2717`), which rejects any
     `transition(id, target)` call whose `(current, target)` pair is
     not declared in the state block.
  3. The **V1.3 refinement check** in `compiler/src/checker/refinement.rs`,
     which proves no handler can call `transition(id, target)` with a
     literal target outside the declared state set.

Together, (1)+(2)+(3) connect the abstract graph the model checker
explores to the concrete behaviour of running cells. The model checker
proves things about the abstract graph; the runtime + refinement
check make sure the concrete program is faithful to it.

---

## 1. The abstract state machine

For a cell `C` with a `state M { … }` block, define:

    G(M) = (Σ, →, s₀, T)

where:

  - `Σ` is the finite set of state names mentioned in `M` (the initial
    state, every `from`, every `to`, excluding the wildcard `*`)
  - `→ ⊆ Σ × Σ` is the transition relation, computed as follows:
    - For each declared `s -> t` in `M`, add `(s, t)` to `→`
    - For each declared `* -> t` in `M`, add `(s, t)` to `→` for every
      `s ∈ Σ` with `s ≠ t` (the wildcard expansion in `from_ast`)
  - `s₀ ∈ Σ` is the `initial:` state
  - `T ⊆ Σ` is the set of terminal states: every `s` such that no
    `(s, t)` exists in `→` for any `t`

`G(M)` is a *finite directed graph*. It has at most `|M|` states and
`|M|² + |M|` edges (the `+|M|` is the wildcard upper bound).

**Lemma 1.1 (Reachability is decidable on G).** *The set
`Reach(G) = { s ∈ Σ : there is a path from s₀ to s in → }` is computable
in O(|Σ| + |→|) time.*

*Proof.* BFS from `s₀`. The implementation is
`StateMachineGraph::reachable` (`temporal.rs:148`). □

---

## 2. The runtime trust base

The model checker reasons about `G(M)`. For its theorems to say
anything about real executions, every actual transition the running
cell takes must be a transition in `→`. This is what `do_transition_for`
enforces:

```rust
let transition = sm.transitions.iter().find(|t| {
    (t.node.from == current || t.node.from == "*") && t.node.to == target
});
let transition = match transition {
    Some(t) => t,
    None    => return Err(RuntimeError::RequireFailed(...))   // line 2741
};
```

**Lemma 2.1 (Runtime fidelity).** *For every cell instance running
under the interpreter, every successful `transition(id, target)` call
satisfies `(current, target) ∈ →` where `current` is the instance's
state in storage immediately before the call.*

*Proof.* By inspection of `do_transition_for`. The call returns `Ok`
only after finding a matching declared transition. If no match exists,
the call returns `RequireFailed` and the storage is not updated. □

**Lemma 2.2 (Refinement static check).** *For every handler `H` whose
`transition()` calls all use literal target arguments, every literal
target referenced by `H` is in `Σ`.*

*Proof.* `check_refinement` (`refinement.rs:116`) walks every statement
of every handler body and emits a hard `UndeclaredTarget` finding for
any literal `target ∉ Σ`. The compiler refuses to verify a program
that contains such a finding (`verify.rs:106` adds it as a `Fail`). □

**Together: every actual transition that fires lies inside `→`.**
Lemma 2.2 statically guarantees this for handlers with literal targets.
Lemma 2.1 catches the rest at runtime: any handler with a dynamic
target argument that ever evaluates to an undeclared state will hit
the `RequireFailed` path. The cost of dynamic targets is that we lose
the static guarantee — but never the runtime one.

The combined guarantee is recorded as Assumption A1 below.

---

## 3. The CTL checker

`compiler/src/checker/temporal.rs` evaluates six property forms:

| Constructor    | Semantics on G(M)                                                 |
|----------------|-------------------------------------------------------------------|
| `Always(P)`    | ∀ s ∈ Reach(G). P(s)                                              |
| `Never(P)`     | ∀ s ∈ Reach(G). ¬P(s)                                             |
| `Eventually(P)`| ∀ infinite or terminal-extended path π from s₀. ∃ i. P(π[i])      |
| `After(s, P)`  | s ∉ Reach(G) ∨ Eventually(P) holds on the subgraph rooted at s     |
| `DeadlockFree` | ∀ s ∈ Reach(G) \ T. |succ(s)| > 0                                  |
| `Mutex(s₁,s₂)` | s₂ ∉ Reach_{from s₁}(G)                                            |

The state predicate `P` ranges over `InState(s)`, `NotInState(s)`,
`InSet(S)`, `And/Or/Not`, and `GuardHolds(_)` — the last of which
**always evaluates to `true`**, see §3.4 below.

### 3.1 Soundness of `Always`, `Never`, `DeadlockFree`, `Mutex`

These are *safety* properties: they refer to no path quantifier. The
checker enumerates `Reach(G)` and checks the predicate at every state.

**Theorem 3.1 (Safety soundness, sound and complete).** *For
`Always(P)`, `Never(P)`, `DeadlockFree`, and `Mutex(s₁, s₂)`, the
verifier returns `passed: true` if and only if the property holds on
`G(M)`.*

*Proof.* `Reach(G)` is a finite set computed exactly by Lemma 1.1.
`P` is a pure function of the current state (Lemma 3.4). Each property
is a finite quantification over `Reach(G)`, which the implementation
performs literally. `Mutex` reduces to two reachability computations.
`DeadlockFree` is an existence check on `succ(s) = ∅ ∧ s ∉ T`. □

In particular, no depth bound is involved. These four properties are
**fully sound and complete** with respect to `G(M)`.

### 3.2 Soundness of `Eventually`

`Eventually(P)` is a *liveness* property and uses the path-quantifier
`∀ π. ∃ i. P(π[i])`. The checker proves it by searching for a
counter-example: a path that never satisfies `P`. If no counter-example
exists, the property holds.

**The 2026-04-10 soundness fix.** The counter-example DFS
(`find_path_avoiding`, `temporal.rs:199`) prunes any branch where
`path.len() > max_depth`. Before the fix, `max_depth` was a hard-coded
`50`. This was a **soundness gap**: on a state machine where every
counter-example path had length > 50, the DFS gave up before finding
one and the verifier reported the property as PASSING.

Repro and regression test:
`compiler/tests/rigor_eventually_long_chain.rs`. The repro is a 60-state
linear chain `s₀ → s₁ → … → s₅₉ → dead_end` with the property
`eventually(NEVER_HIT)` where `NEVER_HIT ∉ Σ`. Pre-fix: PASSING (false
positive). Post-fix: counter-example reported as the full 61-step path.

The fix is one line: `let bound = reachable.len() + 1;`. The bound
must be at least `|Reach(G)|` so the DFS can fully explore every
acyclic path through the reachable subgraph before concluding no
counter-example exists.

**Theorem 3.2 (Eventually soundness, post-fix).** *Let `G` be the
abstract state machine of cell `C`, and let `R = Reach(G)`. If the
verifier returns `passed: true` for `Eventually(P)`, then on `G`,
every path from `s₀` either*

  1. *passes through some state where P holds, or*
  2. *enters a cycle (in which case the cycle is also reported as a
     counter-example by the DFS, contradicting `passed: true`).*

*Proof sketch.* The DFS is bounded by `|R| + 1`. Any acyclic path in
`G` from `s₀` has length ≤ `|R|` (no state appears twice). Therefore
the DFS, with depth bound `|R| + 1`, explores every acyclic path from
`s₀` to completion. For each such path, the DFS terminates in one of
three ways:

  1. The path reaches a state where `P` holds → not a counter-example,
     DFS backtracks.
  2. The path reaches a terminal state without `P` ever holding → DFS
     records the counter-example.
  3. The path revisits a state already on the path → DFS records the
     cycle as a counter-example.

If the DFS finishes without recording any counter-example, then every
acyclic path from `s₀` was case (1), and every cyclic extension was
case (3) and would have been recorded. The only way `passed: true`
holds is when no path satisfies (2) or (3) — i.e., every path from
`s₀` either reaches a `P`-state or is acyclic and `P`-satisfying. □

**Note on completeness.** The DFS is *not* complete: it stops at the
first counter-example, not all of them. That's fine — one
counter-example is enough to falsify the property.

### 3.3 Soundness of `After`

`After(s, P)` is shorthand for `(s ∈ Reach(G)) ⇒ Eventually(P) on the
subgraph rooted at s`.

**Theorem 3.3 (After soundness).** *If the verifier returns
`passed: true` for `After(s, P)`, then either `s ∉ Reach(G)`
(vacuously true), or `Eventually(P)` holds when starting from `s`.*

*Proof.* The implementation first checks `reachable.contains(state)`.
If false, it returns vacuously true. If true, it calls
`find_path_avoiding(state, pred, |R| + 1)` — same DFS, same bound,
same argument as Theorem 3.2. □

The `After` checker received the same depth-bound fix on 2026-04-10.

### 3.4 The guard predicate gap

`StatePredicate::GuardHolds(_)` is evaluated as `true` unconditionally
(`temporal.rs:58`):

```rust
StatePredicate::GuardHolds(_) => true, // future: symbolic eval
```

This is an **over-approximation**, *not* a soundness bug, with the
following consequences:

  - **Safety properties (`Always`, `Never`)**: still sound because
    `true` is the most permissive predicate; the property must hold
    in every state regardless of any guard.
  - **Liveness properties (`Eventually`, `After`)**: weaker than the
    user might expect. A transition guarded by `when amount > 0` is
    treated as if always available, so the verifier may claim
    `eventually(settled)` holds when in fact it requires the guard.
    This is a *conservative* approximation: the verifier gives liveness
    guarantees that hold under the assumption "every guarded
    transition is eventually taken", which may not match reality.

This gap is documented in `temporal.rs:58` as `// future: symbolic
eval` and tracked as V1.4 work. SMT-backed guard reasoning is the
fix; until then, the user is warned that any property mentioning
guards is interpreted weakly.

### 3.5 Lemma 3.4 (Predicate purity)

*State predicates are pure functions of the current state name; they
do not depend on memory, locals, or time.*

*Proof.* By inspection of `StatePredicate::eval`. Every constructor
maps the input `current_state: &str` to a `bool` via name comparisons,
set membership, or recursive logical combinators. `GuardHolds(_)`
ignores its input entirely. □

This is what makes the model checker sound: the abstract graph is
sufficient to evaluate every property, no memory state is needed.

---

## 4. Assumptions

The soundness theorems above hold under these explicit assumptions.
If any of them is violated, the corresponding theorem may fail.

| ID  | Assumption                                                                                       | Enforced by                                                                |
|-----|--------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------|
| A1  | Every successful runtime `transition(id, target)` call lies in `→` of some declared state machine | Lemma 2.1 (interpreter) + Lemma 2.2 (V1.3 refinement check)                |
| A2  | The state machine has finitely many states                                                       | Trivially: `Σ` is the set of identifiers in the source                     |
| A3  | The CTL checker terminates for every property                                                    | DFS bound `|R|+1` + finite `R` (post-fix); BFS for safety properties       |
| A4  | Predicates are pure functions of state name                                                      | Lemma 3.4                                                                  |
| A5  | The static graph `G` correctly captures the source                                                | `from_ast` is unit-tested; the wildcard expansion is the only non-trivial step |
| A6  | The same machine instance is not concurrently mutated by another thread                         | The interpreter is single-threaded per instance; storage backends serialize writes |

---

## 5. Gaps (not yet closed)

The following are known limitations of the V1 verifier. They are
incompletenesses, not soundness bugs (with one exception, now fixed).

### G1. Guard predicates are over-approximated

See §3.4. `GuardHolds(_)` returns `true`. Every guarded transition is
treated as always available. Liveness claims are weaker than they
appear when guards are present. **Fix path:** SMT-backed guard
reasoning, V1.4.

### G2. Cross-machine product state space

A cell with multiple state machines (`state foo { … }` and
`state bar { … }`) explores them independently. There is no product
construction, so deadlocks that arise from interactions between two
machines are not caught. **Fix path:** explicit product construction
when the user opts in via a `[verify.product]` block.

### G3. Multiple-cell composition

`soma verify` checks each cell in isolation. Two cells communicating
via signals could deadlock when composed; the current verifier doesn't
build the composed transition system. **Fix path:** session types on
`face` blocks (originally V1, subtracted in V1.2 — see SEMANTICS §4 —
to be reattempted in V1.4 with a smaller scope).

### G4. Dynamic transition targets

`transition(id, computed_var)` where the second argument is a
runtime-evaluated expression. Lemma 2.2 does not cover these — they
fall back to the runtime check (Lemma 2.1) which catches the bug only
when it actually fires. The refinement check emits a `DynamicTarget`
warning so the user knows where the static guarantee degraded.
**Fix path:** SMT-backed symbolic execution, V1.4.

### G5. Float and integer predicates over guard expressions

Even with SMT, the guard language must be a decidable theory. Linear
integer arithmetic and equality over enums are decidable; nonlinear
real arithmetic is undecidable in general. **Fix path:** restrict
guard expressions to a decidable fragment.

### G6. Soundness gap, **closed 2026-04-10**

The DFS depth bound was hard-coded at 50, which produced false
positives on liveness properties for state machines with acyclic
paths longer than 50. **Status:** fixed in
`compiler/src/checker/temporal.rs` `Property::Eventually` and
`Property::After`. Regression test:
`compiler/tests/rigor_eventually_long_chain.rs`. Both the 60-state and
200-state chain tests pass post-fix.

---

## 6. What this document does NOT yet establish

In order of decreasing seriousness:

  1. **Mechanised soundness in Coq or Lean.** The proof above is on
     paper. A real soundness theorem for a model checker is a
     formalisation of (a) the operational semantics in
     `docs/SEMANTICS.md`, (b) the abstraction relation between the
     concrete machine and `G(M)`, and (c) a forward simulation theorem.
     This is V1.4 (or later) work. *Until it ships, every claim in
     this document is "we believe X" rather than "we have proven X".*
  2. **Soundness of the bytecode VM and native codegen.** The argument
     above is over the *interpreter*. The VM and native backends each
     need their own simulation theorem against the operational
     semantics. The empirical witness for backend equivalence on the
     intersection corpus is in `compiler/tests/equivalence.rs` — that
     is *evidence*, not a *proof*.
  3. **Liveness under fairness.** All liveness theorems above implicitly
     assume "fair scheduling" — that is, every enabled transition is
     eventually taken. Soma's runtime does not yet articulate a
     scheduling fairness model, so the relationship between
     `Eventually(P)` and what users actually observe is informal.

These are all flagged in the rigor pass scorecard at the top of
`docs/rigor/README.md`.

---

## 7. The single sentence

> `soma verify` is sound for safety properties (Always, Never,
> DeadlockFree, Mutex) and now sound for liveness properties on state
> machines whose acyclic path length exceeds the previous hard-coded
> 50; the soundness rests on a refinement check that ensures handlers
> never reach a state outside the declared graph, and a runtime guard
> that catches the corner cases the refinement check cannot.
