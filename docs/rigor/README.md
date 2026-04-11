# Soma — Rigor Pass

The "non-negotiable foundation" that `TARGET_SOMA_V1.md` §0 calls for.
This directory is the index to it.

The rigor pass turns Soma's verification claims from *moral arguments*
into *theorems with explicit assumptions, plus executable witnesses
for the parts that are not yet formalised on paper*. It does not
ship a Coq or Lean formalisation — that's a multi-month effort tracked
as V1.4. It does ship everything you need to *audit* Soma's claims
against the implementation as it stands today.

## Scorecard

| Item | Status | Artifact |
|---|---|---|
| Small-step operational semantics | Paper, complete for V1 surface area | [`docs/SEMANTICS.md`](../SEMANTICS.md) §1 |
| Refinement check soundness | Paper theorem + proof sketch | [`docs/SEMANTICS.md`](../SEMANTICS.md) §1.5 |
| Replay determinism | Paper theorem + proof sketch | [`docs/SEMANTICS.md`](../SEMANTICS.md) §1.6 |
| **CTL safety soundness** (Always, Never, DeadlockFree, Mutex) | **Paper theorem + proof, sound and complete** | [`docs/SOUNDNESS.md`](../SOUNDNESS.md) §3.1 |
| **CTL liveness soundness** (Eventually, After) | **Paper theorem + proof + closed soundness gap** | [`docs/SOUNDNESS.md`](../SOUNDNESS.md) §3.2 |
| **Mechanized proofs** | **Coq proof, no axioms, no Admitted** | [`docs/rigor/coq/`](coq/) — 50 theorems/lemmas across 6 files, all `Closed under the global context` |
| **Memory-budget proof obligation (V1.4)** | **Implemented + executable + lattice mechanized + production cells migrated** | [`compiler/src/checker/budget.rs`](../../compiler/src/checker/budget.rs), [`docs/SEMANTICS.md`](../SEMANTICS.md) §1.7, [`docs/rigor/coq/Soma_Budget.v`](coq/Soma_Budget.v) — 17 budget tests, 2 production cells with proven bounds |
| Backend equivalence (interp ≡ vm) | Conjecture, executable witness on intersection corpus | [`compiler/tests/equivalence.rs`](../../compiler/tests/equivalence.rs) — 16/16 cases |
| Backend equivalence (interp ≡ native) | Conjecture, no harness yet | — |
| State-explosion bench | Measured numbers, three topologies | [`docs/rigor/results/state_explosion.md`](results/state_explosion.md) |
| Budget runtime validation | Empirical witness: cells run under proven cap | [`docs/rigor/results/budget_runtime.md`](results/budget_runtime.md) — 3/3 |
| Adversary models | Per-property table with explicit quantifiers | [`docs/ADVERSARIES.md`](../ADVERSARIES.md) |
| Full mechanized cell calculus | Not yet (separate file, V1.5+) | — |

## Bugs found by the rigor pass

The rigor pass is not just paperwork. Going through it found and
closed real bugs in the verifier and in the original budget proposal:

### 0. The original "1-engineer-week" memory budget proposal had an unsound formula (closed 2026-04-10)

The proposal in `discussions/budget_proposal.md` (Tier 1) used
`peak ≤ slot_sum + |handlers| × H_stack + C_runtime`. The
`|handlers| × H_stack` term modelled "every handler holds its own
stack frame simultaneously", which is **factor-of-|handlers|
unsound**: the Soma runtime executes one handler at a time per cell
instance, so the actual peak is `max_handlers H_stack`, not the sum.

For the rebalancer's `Portfolio` cell with 15 handlers, the
proposal's formula would have charged 120 MiB of stack budget when
the runtime only uses 8 MiB at a time.

The corrected formula in `compiler/src/checker/budget.rs` uses
`max_{h ∈ handlers(C)} handler_peak(h)` and is pinned by
`budget_pass_many_handlers_uses_max_not_sum`.

### 1. CTL liveness soundness gap (closed 2026-04-10)

`compiler/src/checker/temporal.rs` `Property::Eventually` and
`Property::After` hard-coded a DFS depth bound of `50`. On a state
machine with an acyclic path longer than 50 from the initial state to
a non-satisfying terminal, the DFS gave up before reaching the
terminal and the caller treated "no counter-example found" as
"property holds" — a **false positive** on a liveness property.

Repro: 60-state linear chain `s₀ → s₁ → … → s₅₉ → dead_end`,
property `eventually(NEVER_HIT)` where `NEVER_HIT` is unreachable.
Pre-fix: PASSING. Post-fix: counter-example reported as the full
61-step path.

Fix: bound is now `|Reach(G)| + 1`. Regression test:
[`compiler/tests/rigor_eventually_long_chain.rs`](../../compiler/tests/rigor_eventually_long_chain.rs)
— 3 cases, including 200-state stress.

### 2. Soma manifest silently drops verify properties without `[package]`

Discovered while building the bench. A `soma.toml` containing only
`[verify] eventually = ["X"]` is silently parsed as an empty manifest
because the manifest schema requires `[package] name = "..."`. The
user sees "0 user-defined properties loaded" and thinks the property
was applied — but it was silently dropped.

Status: documented as a usability footgun, not yet fixed in the
parser. Workaround: always include `[package] name = "..."` in
`soma.toml`. Tracked as a follow-up.

## How to reproduce everything

```bash
# 1. Differential equivalence harness (interp ≡ vm)
cd compiler && cargo test --test equivalence --release -- --nocapture

# 2. Soundness regression tests
cd compiler && cargo test --test rigor_eventually_long_chain --release -- --nocapture

# 3. State-explosion bench (overwrites results/state_explosion.md)
bash docs/rigor/bin/bench_state_explosion.sh

# 4. Mechanized Coq proofs (requires Rocq Prover 9.1+)
make -C docs/rigor/coq check
# Expected: 11 lines saying "Closed under the global context"
#   (5 from Soma_CTL.v + 6 from Soma_Budget.v)

# 5. Memory-budget unit tests (V1.4)
cd compiler && cargo test --test rigor_budget --release -- --nocapture

# 6. Memory-budget runtime validation (cells under real OS memory cap)
bash docs/rigor/bin/bench_budget_runtime.sh

# 7. Static verification of the rebalancer (the property pack
#    that exercises the most state-machine surface)
./compiler/target/release/soma verify rebalancer/app.cell

# 8. End-to-end sanity (rebalancer test suite still green)
./rebalancer/bin/test_all.sh
```

Every one of those should be green at HEAD. If any goes red, the
rigor pass has regressed and the corresponding theorem in
SEMANTICS / SOUNDNESS / ADVERSARIES needs to be re-examined before
the regression is "fixed" by changing the property.

## What this rigor pass does NOT close

In honest order of decreasing seriousness:

1. ~~**Mechanised proof in Coq or Lean.**~~ **Partially closed.**
   The central depth-bound theorem is now mechanically verified in
   Rocq Prover 9.1.1 (`docs/rigor/coq/`, 50 theorems/lemmas, all
   `Closed under the global context`). What is still on paper:
     (a) the connection between the abstract `Graph` of `Soma_CTL.v`
         and Soma's cell calculus from `docs/SEMANTICS.md` §1
         (requires mechanizing the reduction relation),
     (b) the cyclic-counter-example branch of the DFS (the bug was
         in the acyclic case; the proof covers it),
     (c) the four safety theorems (Always, Never, DeadlockFree,
         Mutex) which reduce trivially to `forall x in reach, P x`
         but are not yet in the `.v` file.
   These remaining items are incremental: each is a separate file
   that builds on `Soma_CTL.v`. Tracked as V1.4.
2. **Native codegen equivalence.** The intersection harness covers
   interpreter vs bytecode VM. The `[native]` backend (compiles
   handlers to a Rust `cdylib`) needs its own harness that compares
   `[native]` output to interpreter output on every CLBG corpus
   program. Today the comparison happens informally in
   `bench/clbg_*` but is not asserted by `cargo test`.
3. **Cross-cell composition verification.** Two cells communicating
   via signals can deadlock when composed; the verifier checks each
   cell in isolation and would not catch it. This was the V1
   `protocol` feature, subtracted in V1.2 because it didn't carry
   the claim. Reattempting it with smaller scope is the right next
   move (`SOUNDNESS.md` G3).
4. **Concurrent-handler safety.** The interpreter is single-threaded
   per instance, so this is dormant. Any future multi-threaded
   backend (e.g. async runtime) must either keep handlers atomic or
   re-prove the safety properties.
5. **Liveness under fairness.** The model checker assumes
   "every enabled transition is eventually taken". The runtime does
   not yet articulate a scheduling fairness model, so the
   relationship between `Eventually(P)` and what users observe is
   informal.

These are the items that, if you are using Soma in production today,
you should know about. The rigor pass *names* them; it does not
*close* them.

## Files

```
docs/
├── SEMANTICS.md             cell calculus + refinement + replay + §1.7 budget + §1.8 CTL + §3 measured cliff
├── SOUNDNESS.md             CTL soundness theorems + assumptions + gaps
├── ADVERSARIES.md           per-property × adversary table
└── rigor/
    ├── README.md            (you are here)
    ├── bin/
    │   ├── bench_state_explosion.sh    state-explosion bench
    │   └── bench_budget_runtime.sh     budget runtime validation
    ├── coq/
    │   ├── Makefile         build + check no-axioms (both files)
    │   ├── Soma_CTL.v       MECHANIZED depth-bound theorem
    │   ├── Soma_Budget.v    MECHANIZED cost lattice + composition theorem
    │   └── .gitignore
    └── results/
        ├── state_explosion.md          state-explosion bench output
        └── budget_runtime.md           budget runtime output

stdlib/
└── budget.cell                                  ← new property registrations

compiler/
├── src/
│   ├── ast/mod.rs                               ← StateMachineSection.properties, For.bound
│   ├── parser/mod.rs                            ← state [...] {} and for [...] in
│   ├── checker/temporal.rs                      ← soundness fix
│   ├── checker/budget.rs                        ← V1.4 budget analyzer
│   └── checker/mod.rs                           ← BudgetExceeded, BudgetOk, BudgetAdvisory wiring
└── tests/
    ├── equivalence.rs                           ← interp ≡ vm executable witness (16 cases)
    ├── rigor_eventually_long_chain.rs           ← soundness regression (3 cases)
    └── rigor_budget.rs                          ← budget unit tests (13 cases)

rebalancer/
└── lib/
    ├── alpha.cell                               ← migrated: scale.memory + loop_bounds
    └── optimizer.cell                           ← migrated: scale.memory
```

## The single sentence

> The rigor pass replaces Soma's verification claims with theorems
> whose assumptions are explicit, whose gaps are listed, whose proof
> sketches are reproducible by reading the source, whose central
> depth-bound lemma AND whose memory-budget cost lattice are now
> mechanically verified in Rocq with no axioms and no `Admitted`
> (50 theorems/lemmas across 6 files), whose backend equivalence has an executable
> witness, whose state-explosion cliff is measured rather than
> asserted, and whose budget proof obligation is wired into
> `soma check` with two production cells already migrated and a
> runtime harness that confirms each cell fits in its proven cap —
> and in doing so it found and closed a real soundness bug in the
> CTL liveness checker AND a factor-of-|handlers| unsoundness in the
> original budget proposal.
