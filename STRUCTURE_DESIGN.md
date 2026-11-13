# One answer per question — the structural work before a release

Eight rounds of adversarial review (`SKELETON_DESIGN.md`, rounds 5 to 12)
found 4, 7, 11, 30, 20, 25, 10 and 28 causes of a false ✓ or of an effect
outside a frame. Where a question had ONE shared answer, later rounds found
nothing (the hole model, the evaluation order of the product search,
wildcard edges, name resolution since round 10). Every remaining cause is
a question answered in several places. This file lists those questions,
what is done, and what needs a decision before it is implemented.

## 1. What runs in a program? — DONE (2026-11-13)

A handler, an `every` / `after` block and the statements of `runtime { }`
all run. `checker/bodies.rs::entries(cell)` gives them all as handlers (a
timer is `every 1000ms`, the section is `runtime { }`).

- `desugar::expose_for_analysis` adds `runtime { }` as a handler, so every
  analysis that reads the exposed program sees it: the proofs of `verify`,
  and at `check` the cost, the taint and the capabilities.
- `effects`, `capabilities` and `budget` iterate `bodies::entries` (they
  read handlers only: a timer's think() or allocation was never counted).
- In a skeleton, `runtime { }` and a schedule only call signals.

Left: `frame.rs`, `invariants.rs`, `product.rs`, `termination.rs` and
`waits.rs` still enumerate handlers and timers themselves (they agree
today, by test). Moving them to `bodies::entries` is mechanical and should
be done one analysis at a time, each behind the verdict harness.

## 2. Facts written once — PARTLY DONE

| Fact | State |
|---|---|
| "is this name a cell?" | `names::cellish` (the program's cells; a capital letter only when the program is not known). Was decided by a capital in a dozen places. |
| "which methods write a slot?" | six methods at run time (`set` / `put`, `delete` / `remove`, `push` / `append`); eleven lists in the checker, five contents. No consequence found. **To do**: one `names::SLOT_WRITES`. |
| "is this value an Int?" | `-> Int` now answers an Int at run time; `BigInt` is no Int for the prover. Five lists remain in `invariants.rs` (`known_int`, `SLOT_INT`, `int_params`, `int_returning`, `may_be_float`). **To do**: one predicate over the declared type. |
| "where is the project?" | the loader (beside the importer, then the root), the manifest (beside the entry only), the lock (every folder up to the root), the tools (`refs::survey`: nearest soma.toml above). **To do**: one `project::root(entry)` used by the four; a test file in a sub-folder then runs with the project's edition and `[verify]` tables. |

## 3. Analyses that do not follow calls — DONE for taint, composition, budget

`taint`, `effects`, `capabilities`, `composition` and `budget` are
syntactic and per handler. `effects` now refuses to narrow when anything
the handler runs may think again; the others still miss a value or an
effect that goes through a helper, a list, a lambda parameter or an emit.
The product search and the frames have a shared call resolution
(`frame::resolve_call`, the interpreter's rule since round 10): these five
should use it instead of matching names.

Done (test `s3_*`): taint follows a value into a list, an index write, a
lambda parameter and a `vote`; composition pairs an emit with a listener of
the program that takes its arity; budget charges the largest callee frame
(one thread stack) and a `map` over an unknown length as no bound. Left:
`capabilities` and `effects` still match names rather than resolving calls.

## 4. One executor for interior cells — DONE (the rule), 2026-11-13

What exists: `runtime/mod.rs` (422 lines) is not a second interpreter. It
builds an `Interpreter` per handler run, registers one parent and its
children, gives them qualified storage, and sets `emit_scope`. The
disagreement is in WHICH path a handler takes:

| Entry | Path | An `emit` from top-level cell `P` reaches | Interior cells registered |
|---|---|---|---|
| `soma serve`, `soma test`, `soma run app.cell P.h` | plain interpreter | top-level cells | no |
| `soma run app.cell` (a `runtime { }` or an `interior`) | scope runtime | the children of the main cell | the main cell's |
| `soma run app.cell Child.h` | scope runtime | `P`'s children and `P` | `P`'s |

So the same handler hears, or does not hear, the same event depending on
how the program was started; an interior cell's `[persistent]` slot is
written by two back-ends (`b5_scope_parent_persistent_slot_two_stores`);
and every proof models one of the three rows.

**Proposed rule** (one, for every entry path):

- an `emit` from a top-level cell is heard by the top-level cells AND by
  the cells of its own interior;
- an `emit` from an interior cell is heard by its siblings and by its
  parent — never by another top-level cell (as today);
- an interior cell is never called by name from outside its parent (as
  today); a bare call from an interior cell resolves in its own cell, then
  among its siblings, then at top level.

Implementation: the interpreter registers interior cells always, with the
qualified storage `runtime/mod.rs` builds today; `emit_scope` becomes that
rule; `Runtime` keeps only the driving of `runtime { }` and the CLI entry
for `Child.h`. The analyses read the same function (`scope::listeners`).

Done, in a narrower form than "one executor": the scope runtime stays (it
drives `runtime { }` and the CLI entry `Child.h`), but every path answers
the same three questions the same way — `Interpreter::hears` for events
(also used by `check`'s signal matching and the product search), the
registry for slot back-ends (one store per slot), and the start-up audit
over interior cells. Under `serve` and `test` interior cells are still not
started (they never were): an event a top-level cell raises reaches its
interior only under `soma run`. Test `s4_*`.

## 5. Rules between machines over different ids — DONE (`via`), 2026-11-13

Today a rule follows ONE id through every machine. The clinic had to key
the appointment, the consultation, the prescription and the invoice by one
visit id.

**Done** with a slot of its own rather than a field (a field of a record
is rewritten with the record): a `via` on the rule, naming the slot where
the other id is read:

```toml
[verify.before."Ordonnances.delivree"]
requires = ["Consultations.terminee"]
via = "Ordonnances.consult_of"     # Map<String, String>: prescription → consultation
```

Meaning: for the prescription `o` that is delivered, the consultation
whose id is `ordonnances.get(o).consult` has been `terminee`. The search
then follows a PAIR of ids, and needs to know that the slot field does
not change once written. That is provable when the skeleton says so — a
slot (or a field) declared write-once — and is the honest scope of a first
version: `via` is accepted only over a field no signal rewrites after the
entry is created (checked on the frames: one `writes`, by the signal that
creates the entry), and a body that reads the link must read it from that
slot. Anything else is "not proven".

Alternative, cheaper: the same clause in the frame —
`moves signee -> delivree when Consultations.consult(consult_of(id)) in terminee`
— which `verify` can judge on a hole (decision 10). Both need the pair of
ids; the frame form also answers "state an inter-machine rule before the
bodies exist".

Implementation (`checker/product.rs`): the machines whose states the rule
requires follow a second instance; `consult_of.get(o)` / `consult_of[o]`
read in the target's cell is the key `link:o`, which IS that instance when
`o` is the followed one; the two decisions are kept consistent. The write-
once condition is checked on the bodies (`link_written_once`): one writer,
one `set`, after `require consult_of.get(id) == () else …`, no delete.
Eight probes (other id, reopen under `while`, index form, a helper, an id
swapped, a `??` default, a hole) give the expected verdict; test `s5_*`.

## 6. Next

- `capabilities` and `effects` on the shared call resolution;
- `frame`, `invariants`, `product`, `termination`, `waits` on
  `bodies::entries` (they agree today, by test);
- one `names::SLOT_WRITES`, one "is an Int" predicate;
- DONE: the frame clause `moves s -> t when Other.s [|| …] [via slot]`
  (decision 10) — a rule between machines stated in the skeleton, judged on
  the holes and on the bodies (tests `s6_*`).

## 7. Round 13: the single answers, attacked

14 disagreements, all where a single answer was only half shared — fixed by
sharing it: `names::hears` (the interpreter's rule, by name) is now what
`check` uses; the runtime section's `signal` follows it; a listener in an
interior is "may run" for the proofs (the interior starts only under `soma
run app.cell`, for the driving cell); a cell name names one cell (an
interior cell may not take a top-level cell's name). `project::root` follows
the loader's order. The write-once condition of `via` reads values and
time (a key left as it came, no `[task]`), not only text.
