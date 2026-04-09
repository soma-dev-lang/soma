# Refinement examples (V1.3)

`soma verify` now proves that the **handler bodies** refine the
**state machine** declared in the same cell. Before V1.3, the verifier
only checked the picture; the handler bodies were trusted. They could
lie. Today they can't.

| File                                       | What it shows                                  | `soma verify` exit |
|--------------------------------------------|------------------------------------------------|--------------------|
| `01_payment_correct.cell`                  | The well-behaved baseline. All checks pass.    | 0 ✓               |
| `02_payment_undeclared_target.cell`        | A handler invents a state. Compiler rejects.   | 1 ✗               |
| `03_payment_dead_transition.cell`          | The spec declares a transition no handler implements. Warning, not error. | 0 ⚠ |
| `04_path_conditions.cell`                  | Effect summary surfaces path conditions per handler. | 0 ✓         |

## Try it

```sh
SOMA=./compiler/target/release/soma

# 1. Correct version
$SOMA verify examples/refinement/01_payment_correct.cell
# Expected: ✓ refinement: handler `authorize` ⟶ {authorized}
#           ✓ refinement: handler `capture` ⟶ {captured}
#           ✓ refinement: handler `settle` ⟶ {settled}
#           ✓ refinement: handler `refund` ⟶ {refunded}

# 2. Undeclared target — the build FAILS
$SOMA verify examples/refinement/02_payment_undeclared_target.cell
# Expected: ✗ refinement: handler `settle` calls transition(_, "completed")
#               but "completed" is not in state machine `lifecycle`

# 3. Dead transition — warning, build still passes
$SOMA verify examples/refinement/03_payment_dead_transition.cell
# Expected: ⚠ refinement: declared transition `* → refunded` is never reached
#               by any handler — spec may be aspirational or stale

# 4. Path conditions — effect summary
$SOMA verify examples/refinement/04_path_conditions.cell
# Expected: ✓ refinement: handler `process` ⟶
#               {authorized [if amount > 0], rejected [if not (amount > 0)]}
```

## What V1.3 proves and what it doesn't

**Proves (sound, no false positives):**
- Every `transition("inst", "X")` call with a literal target `X` names a
  state that exists in the cell's state block.
- Every transition declared in the state block is the target of at least
  one handler's `transition()` call.
- Per-handler effect: the set of target states the handler can reach,
  with the path condition (chain of `if` guards) leading to each call.

**Does not prove yet (V1.4 work):**
- **Source-state correctness.** A handler doesn't statically know which
  state the machine was in when it was called — that's runtime info.
  V1.3 only checks the *target* of each `transition()` call, not that
  the call is legal *from the current state*. SMT-backed control-flow
  analysis is the V1.4 job.
- **Guard implication.** When the state block says `pending → authorized
  when amount > 0` and the handler writes `if amount > 0 { transition }`,
  V1.3 records both as text but doesn't try to prove implication. SMT.
- **Dynamic targets.** `transition(id, target_var)` where `target_var`
  is computed at runtime: V1.3 emits a warning that this handler's
  effect can't be statically analyzed and refinement coverage is
  incomplete here.

These are intentional. V1.3 ships the *syntactic, sound* refinement
check — incomplete (some real bugs slip through dynamic targets and
guard arithmetic) but it never falsely accuses a correct program.
The incompleteness is documented per-handler in the verifier output
so the user can see exactly which handlers got the strong check.

## Why this is the WOW feature

Soma's tagline is *"the specification is the program"*. Before V1.3 it
was half true: the state block was a spec, and the handler bodies were
code, and the compiler treated them as independent documents. They
could drift apart. V1.3 closes that gap. Today, when you run `soma
verify`, the artifact that gets *proved* and the artifact that *runs*
are the same artifact. The poster became a theorem.
