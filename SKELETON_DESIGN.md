# Skeleton and Flesh — Design

> **Soma's goal: let programming agents design large applications in a
> solid manner.** This is the first feature judged by that line.
>
> Status: implemented, unreleased (see `CHANGELOG.md`, "Unreleased").
> User-facing description: "Skeleton first" in `site/llms.txt`
> (`soma docs agent`). This file records the design, the decisions, the
> evidence and what is left.

## TL;DR

A cell is split into two objects:

- a **`skeleton`** — memory, invariants, state machines, and a face whose
  signals each carry a **frame**: what the handler may read, write, call,
  emit, and which machine edges it may take. No handler bodies.
- a **`flesh`** — handler bodies and private helpers, nothing else.

`soma check` accepts a flesh only if it **conforms**: every effect of a body
is inside the frame its signal declares. An agent designs the whole
application as a short, complete, provable object first; the code added
afterwards decorates that object and cannot change it.

> **Monotonicity.** Everything read off a skeleton — and every `✓`
> `soma verify` prints on it — stays true for every conforming flesh.
> This is tested, not assumed: see "Evidence" and "The review".

## Why: the agent's point of view

An LLM agent writing a handler body needs five facts: the signature, the
machine edges it must take, the memory it may touch, the signals it may
call, and what it owes (`requires` / `ensures`, invariants on the slots it
writes). Everything else in the file costs context and invites mistakes.

Before this work (3.0.0), measured on `treasury/app.cell` (13,293 bytes):

| View | Size | What was missing |
|---|---|---|
| the source | 13,293 B | nothing, which is the problem |
| `soma describe` (JSON) | 9,792 B | no compression |
| `soma describe --faces` | 1,510 B | which handler writes which slot, calls whom, takes which edge; guard text |

and three gaps:

1. **A hole was indistinguishable from an oversight.** `on pay(id) { }`
   passed `check` silently; `verify` said OK with a "never reached" warning.
2. **The structure was not protected.** A body and a state machine could
   change in the same diff, with nothing marking the second as different.
3. **No frame.** That `settle` takes `authorized -> paid` and touches
   nothing else was discovered by reading every body.

## The objects

```soma
skeleton Payments {
    state flow {
        initial: proposed
        proposed -> authorized { guard { amount <= 500 } }
        proposed -> pending_signoff
        pending_signoff -> signed_off
        signed_off -> authorized
        authorized -> paid
        proposed -> rejected
        pending_signoff -> rejected
    }
    face {
        signal authorize(id: String, amount: Int) -> String
            moves proposed -> authorized, signed_off -> authorized
        signal settle(id: String) -> String         moves authorized -> paid
        signal payment_state(id: String) -> String  reads flow
    }
}

flesh Payments {
    on settle(id) {
        transition(id, "authorized", "paid")
        return "paid"
    }
}
```

### The frame

| Clause | Grants |
|---|---|
| `reads a, b` | reading slots; a machine name grants reading that machine |
| `writes a` | writing and reading slot `a` |
| `moves s -> t` | taking that edge, and reading the machine. `Cell.machine: s -> t`, `* -> t`, `* -> *` |
| `calls f, Cell.g, think` | signals, and builtins with an effect or a varying answer |
| `emits s` | `emit s(…)` |

- **Closed.** A missing clause grants nothing; no clause at all is a pure
  function of the parameters. Pure builtins are free.
- **Not transitive.** `calls Ledger.debit` does not restate `writes
  account`: the callee has its own frame.
- The seven words (`skeleton`, `flesh`, `reads`, `writes`, `calls`,
  `moves`, `emits`) are contextual: ordinary identifiers everywhere else.

### The flesh

- `on` handlers only — `memory`, `state`, `face` there is a syntax error
  that says where the thing goes; so is an `on` in a skeleton.
- An `on` naming a face signal is its body: same parameter names, types
  optional (and equal if written).
- Any other `on` is a **private helper**: named `_helper`, typed. Its
  effects are charged to every signal that reaches it. The underscore is
  the language's existing privacy rule (never routed, never an event), so
  no new runtime mechanism was needed; a helper without it is a check
  error, and a call of one from another cell is refused.
- Several `flesh` blocks per skeleton, across files; two bodies for one
  signal is an error.
- A signal with no body is a **hole**: a handler with the property `hole`
  that fails with kind `not_implemented`.
- A flesh reads its skeleton's `const`s.

### After parsing

`checker::flesh::attach` moves each body into its skeleton — after the
parse of a file and again after its imports. From then on the program is
ordinary `CellDef`s: the interpreter, `[native]`, `serve`, `test` and the
existing verifier are untouched. A `cell` remains a skeleton and its flesh
in one, with open frames.

## Conformance (`checker/conform.rs`)

On the skeleton alone (errors of the design, found before any body):

- every slot, edge, machine and signal a frame names exists; another
  cell's slot is refused with the way through its signals; a helper or a
  bare name of another cell's signal in `calls` is refused with the fix;
- a schedule (`every` / `after`) only calls signals;
- warnings: an edge no frame in the program moves, a slot no signal may
  write, a helper nothing calls, a pure builtin in `calls`.

Once a signal has a body (errors, kind `frame`):

| The body… | must be covered by |
|---|---|
| writes a slot | `writes` |
| reads a slot | `reads` or `writes` |
| reads a machine | `reads <machine>` or a `moves` on that machine |
| calls a signal / an effectful builtin | `calls` |
| emits | `emits` |
| `transition(id, from, to)`, or a target reached from a state the body established | that edge in `moves` (or `* -> to`, `* -> *`) |
| `transition(id, to)` otherwise | some edge into `to` in `moves` |
| `transition(id, computed)` | `moves * -> *` |

and a frame wider than its body is a warning. Errors carry their fix:

```
error: `Orders.place` writes `log`, outside its frame (no `writes`) — if the design means it,
       add it to `signal place` in `skeleton Orders`: `writes log`
```

The inference (`checker/frame.rs`) is syntactic and reuses the passes that
already exist: `refinement::analyze_handler` for transitions, the
`determinism` rules for slots and calls, `desugar::expose_handler` so that
effects written inside string interpolations are seen.

## The agent's views (`commands/skeleton.rs`)

```
soma skeleton app.cell                  the whole map, as skeleton source
soma skeleton app.cell Cell.signal      the brief for one signal
soma holes    app.cell [--json]         open obligations, callees first
soma skeleton app.cell --split          cells rewritten as skeleton + flesh
soma skeleton app.cell --freeze|--amend skeleton.lock
soma init --skeleton                    a starter laid out this way
```

On a plain `cell` program, `soma skeleton` prints the skeleton it would
have, frames inferred — the compact view of existing code. `--split`
writes that as real source: structure and comments verbatim, each signal
with the frame its body has, bodies unchanged in a `flesh`; every handler
the outside could call (no leading `_`) joins the face, so the program
stays the same program.

## Freezing (`checker/lock.rs`)

`skeleton.lock` holds, per skeleton, a hash of its source tokens and its
text. `check`, `run`, `test`, `serve` and `verify` refuse a program whose
skeletons differ, printing the lines that changed; comments, layout and
flesh are free. `--amend` accepts a change and prints it. This does not
stop a determined agent — it can run `--amend` — but it makes a structural
change a separate, named act that shows in review and that a harness can
deny independently of the right to write flesh.

## Evidence

**Size.** `soma skeleton app.cell` on the demo apps (bytes):

| App | Source | `--faces` | Skeleton | Skeleton / source |
|---|---|---|---|---|
| rebalancer | 34,807 | 2,085 | 3,372 | 9% |
| poker | 22,153 | 1,112 | 2,363 | 10% |
| airlock | 8,664 | 596 | 947 | 10% |
| incident-response | 15,315 | 1,190 | 1,896 | 12% |
| elevator | 10,976 | 688 | 1,486 | 13% |
| loan-origination | 18,238 | 1,465 | 2,581 | 14% |
| hft | 16,864 | 1,440 | 2,698 | 15% |
| treasury | 13,293 | 1,510 | 2,698 | 20% |
| delivery | 28,503 | 3,173 | 5,791 | 20% |
| car-rental | 5,861 | 1,529 | 3,095 | 52% |

(Measured when the map view was first built; later changes to the frame
rules move these by a few bytes.)

**The split is the same program.** `tools/split_roundtrip.py` rewrites
every tracked `.cell` file that passes `soma check` (1,049 with a cell) as
skeleton + flesh and requires: `check` passes — so every inferred frame
holds when enforced on the body it came from — the same `soma test` tally,
the same `soma verify` verdict. All 1,049 pass.

**The frames are sound against execution.** A frame inferred and enforced
by the same analysis proves nothing about what the analysis misses. So the
interpreter can log what handlers really do (`SOMA_EFFECT_TRACE=<file>`:
every slot read and write, handler call, effectful builtin, transition and
emit, with the handler stack — `interpreter/effect_trace.rs`), and
`tools/frame_oracle.py` runs the test cells of every program that has some
(358) and requires each observed effect to be inside the frame of the
signal that made it. All pass — which did not stop a review from finding
leaks: the oracle only sees what tests exercise. What the oracle is blind to: programs
without tests (738 files), and paths the tests do not take.

**Dogfooding.** `marketplace/` was written the way the feature intends:
six skeletons first (27 signals, two lifecycles) — checked, proven and
frozen with no body — then one flesh file per cell, following `soma holes`.
The skeleton is 5.3 KB of 21 KB. `soma check` caught, by name, every place
where a body and its frame disagreed while the corpus examples were being
written (a forgotten `calls count`, a helper reading `titles` from three
signals whose frames did not say so).

**Found on the way.** Writing the first skeleton by hand showed that
`Treasurer.process_invoice` moves the `Payments` machine directly
(`treasury/app.cell:263`) instead of going through a `Payments` signal —
one visible line in the skeleton, invisible in the source. The first
conformance test found that effects inside string interpolations were not
seen by the inference (fixed before anything relied on it).

**No regression.** None of the 1,059 valid programs of the repository
changes verdict under `soma check`; the Rust suite passes.

## The review (2026-11-13)

An independent review found cases where `check` and `verify` accepted
wrongly. Each is closed with a test in `compiler/tests/skeleton.rs`; the
reproducers are kept in `compiler/tests/fixtures/skeleton_review/`.

**Frame leaks (`check` passed, the trace showed the effect).**
- `[hole]` written on a handler with a body switched conformance off. The
  hole marker is no longer a name a program can write (`<hole>`).
- Locals were collected for the whole body: a `let` in a dead branch, a
  loop, after the call, a lambda or match binder elsewhere hid a call, a
  cell or a slot. Now a name is the slot / the cell it names unless a local
  is CERTAINLY bound where it stands (parameter, earlier `let` of an
  enclosing block, loop variable, lambda parameter, match binder); a call
  is counted whatever is bound. This also removed the false positives (a
  `let` or a lambda parameter named like a slot).
- An interior cell named like a skeleton made the analysis read the wrong
  body: refused (two plain interior cells of one name stay legal).
- `emit` reached a private helper: refused. Rule: an emit runs every `on x`
  of the program; a listener that is a signal answers with its own frame,
  as a callee does; a helper cannot listen.
- `transition(id, to)` under `moves a -> to` also took `b -> to`: without a
  source the body needs every edge into the target in its frame.
- Calls in method form (`id.bump()`, `id.transition("x")`), through pipes,
  in a `require` message and through `std.` are counted.
- Examined, no leak: a lambda cannot be stored in a slot. Builtins that
  run a handler named at run time (`delegate`, `_std`, `resume`, `deliver`,
  `horde`, `vote`) must be named in `calls`, and a frame that names one
  counts as able to reach anything.

**`verify` on holes.** A hole was read as an empty body, so a property
held "because nothing happened". `flesh::model_holes` now gives the
verifier, for each hole, the body that does everything its frame allows.
A `[verify.before]` rule the frames can break is `⚠ open obligation`; an
invariant on a slot a hole may write is `⚠ runtime-checked`; termination,
think-isolation and per-handler lines wait for the bodies.
`tools/skeleton_monotone.py` checks the criterion on every valid program
of the repository: each `✓` of the derived skeleton (every signal a hole)
is a `✓` of the program. Two more fixes came out of it: a `horde` whose
target is a variable was reported "its tasks call no think()", and a guard
identical to the face's `requires` was "runtime-checked".

**The lock.** It now covers interior skeletons, the signals of other cells
that move a frozen machine, and soma.toml's `[verify]` tables; it applies
to any file of its directory by skeleton name; `eval`, `build`, `replay`,
`deploy` and `serve --no-check` apply it; a lock that cannot be read (a
checksum line protects it) is an error and `--freeze` never writes over a
lock; the difference prints one line per signal and names it.
Decision: the `[verify]` tables ARE part of the fingerprint — dropping a
theorem weakens the design as surely as dropping an edge.

**The brief.** It now carries the comments of the skeleton (cell, signal,
slots, edges), the cell's constants, the types the signatures name, every
machine touched in full, builtin signatures, callee contracts and callers.
Test: in the 11-cell `quarter` trial, three signals of three cells were
emptied and rewritten by a fresh agent given the three briefs and the
language docs only — `check` passed first time, 123 of 123 tests passed.
The designer's intent had to be written as comments in the skeleton first:
a brief cannot pass on what the skeleton does not say (it now says so when
a hole has no comment).

### Rounds 2 and 3 (red team, 2026-11-13)

Two more adversarial rounds (about 330 and 190 probes) found 14 then 26
confirmed problems; reproducers are in `compiler/tests/fixtures/
skeleton_review/round2` and `round3`, the tests are `r2_*` and `r3_*`.
What they changed, beyond individual fixes:

- **The run and the check agree on what a name is.** A local named like a
  slot no longer falls back to the slot at run time (an interpreter fix).
- **Source inference is withdrawn from conformance.** Round 2 asked that
  `if get_status(id) == "a"` establish the source; round 3 broke that
  inference ten ways (a rebinding, a stale alias, a call in the same
  condition, a lambda, a match). Only `transition(id, from, to)` pins an
  edge now — the run enforces it — otherwise the frame needs every edge
  into the target. Less convenient, and not foolable.
- **Lambdas are judged by where they are written.** A lambda runs in the
  cell of whoever calls it. Tracking where a lambda value goes lost to
  every indirection, so the rule is positional: an impure lambda may only
  be the direct argument of a builtin that applies it at once.
- **The oracle sees edges.** The effect trace logs `from>to` for every
  transition; before, a move that reached the right state by the wrong
  edge was invisible to it.
- **The lock** reads `[verify]` as TOML and holds the edition, follows
  types into the types they name, and is looked up beside every file that
  declares a skeleton and above (real paths).
- **Cost bounds** are not "proven" while a hole may call or emit.
- `std.f(…)` is charged as the builtin `f`.

### Round 4

About 260 probes; the source rule held. The lambda rule still had side
doors (`reduce`'s initial value, a flat set of binders inside the lambda, a
signal named `filter`, "visibly a list" judged by a name) and one gap no
static rule on flesh can close: a plain cell or a test hands an impure
lambda to a skeleton's pure signal. So the line is now held twice:

- **statically** (`check`, E0120): tightened — only the last argument of a
  real builtin no local hides, `map(a, f)` only over a list literal /
  `list(…)` / `range(…)`, the lambda judged with the body's lexical scope;
- **at run time, for every program**: a lambda carries the handler that
  wrote it; called anywhere else it may only compute — a slot, a handler
  call, a transition, an effectful builtin or an emit raises kind `lambda`
  (`interpreter/mod.rs`, `in_foreign_lambda`). What `check` misses is then
  a refusal, not a leak.

Also: `verify`'s product search no longer counts the right side of `&&`,
`||`, `??` as always run (a false ✓ on a filled program); the brief and
`soma holes` refuse a program its lock refuses; a lock is found from any
depth; a lock that is not the program's own judges only the declaration
(nested locks could not be amended); a builtin called in method form is
the builtin whatever signal another cell names the same.

### Round 5

About 400 probes. The static line held (no leak through `check`); what
gave was beside it:

- **the run-time guard had two side doors and was too strict.** `len(slot)`
  and `listslot[i]` are fast paths that read the backend without passing the
  guard (or the effect trace); `link`, `subscribe`, `ws_connect`, `ws_send`
  are dispatched before `call_builtin`. Both closed. And a lambda's home is
  now its **cell**, decided by the innermost lambda running: a helper
  `_each(xs, f)`, a lambda one handler returns to another, a callback
  applied by a combinator that came from another cell are at home — they
  were refused at run time in plain cells, with no diagnostic at check.
- **`verify` proved four kinds of thing it had not seen.** (1) a call in a
  string interpolation was hoisted before its statement even from a place
  that may not run (right of `&&`, `??`, a `require`'s detail, a match
  guard, a `try`) or runs each turn (a `while` condition): `desugar` now
  keeps it there as `if true { call } …`, which every analysis reads as
  "may run". (2) an assignment inside a nested block was lost with the
  block's copy of the frame: after a statement, what its inner blocks
  assign is an unknown value; in a loop, also at each turn, with the loop
  variable. (3) (4) a match binder or a lambda parameter named like the id
  — or like a state — was read as the id, or as the state: binders get keys
  of their own. Found on the way: under edition 2027 `get_status(id)` is
  `()` for an id never moved, and the product read the initial state there
  (a false ✓); `get_status(id) ?? "initial"` is now read exactly.
- **the brief** quoted "The same, paid with points." without the comment it
  leans on: it now brings the neighbour's.

Known and accepted: a plain cell that passes an impure lambda through its
own helper is a valid program that `--split` cannot turn into a skeleton
(the static rule refuses it there) — four fixtures of that shape.

### Round 6: a red team and a builder

Two agents, on a copy of the binary. The **red team** (about 520 probes):
no leak through `check`, no way round the run-time guard, no wrong refusal,
no panic, the lock detected everything but one case (another entry file of
the lock's directory read a changed soma.toml). What gave is `verify`,
seven families of false ✓ (`continue`, `std.f(…)`, interpolated ids and
targets, states differing by case, a guard "proven" across an effect of the
same statement, `assuming` over a Float, a lambda kept for later) — closed,
reproducers in `fixtures/skeleton_review/round6/`. The pattern of rounds 4
to 6 is clear: the frame is syntactic and closed, so it holds; the proofs
interpret bodies, and every construct they do not model is a false ✓. The
product search now refuses to conclude ("not proven") where it cannot
follow, instead of reading on.

The **builder** wrote a clinic — 14 cells, 115 signals, 6 machines, 751
lines of skeleton, 715 of flesh, 218 assertions — from `soma docs` alone,
then made three change requests (`bench/agents/large/clinic`, journal
included). What it measured:

- writing: 112 bodies passed `check` with two families of errors, the tests
  with one red assertion; but it judges it would have written the first
  version about a third faster with plain cells;
- **maintenance is where the skeleton pays**: adding a state, `check` named
  the signal whose `transition(id, "annule")` now took an edge outside its
  frame, and the edge no signal moved; adding a parameter, one `check`
  listed every body and caller to touch; changing a rule, `verify` went red
  where 207 assertions stayed green;
- what cost most: proving a rule between machines dictated the data model
  (one id through every machine). Done since: the getter form is evidence.
  **Not done**: tying instances with different ids (an invoice named in an
  order's slot), and a frame clause that states an inter-machine rule so
  that `verify` can judge it on a hole;
- at this size the map alone (469 lines, no comments) was not used:
  `--index` and the one-cell view answer that.

Left open by this round, for a decision: comments are the brief yet are not
in the lock (a spec change in a comment passes unseen); `use` from a
`tests/` sub-directory; E0111 on typed slot values read by `.keys`
(`.entries` is the documented way round).

### Round 7: maintaining what one did not write

An agent received the clinic knowing nothing of it, with the instruction
not to read sources to understand it, and made five change requests (a
waiting list, a discount, a rule between machines, a rename, a removal):
all done, 310 assertions, one file read whole. The index, the one-cell view
and the briefs were enough to find one's way among 115 signals — "the
quality is the author's comments; without them the brief is a shell".

What it found, and what was done:

- a `before` rule whose target is an initial state was ticked whatever the
  bodies did (the natural first wording of "a lab request only while the
  visit is open"): refused now. "Only while the other machine is in S" had
  no wording at all — `before` is history: **`while = [...]`** states it,
  on the product, holes included;
- "where is this signal, who calls it, which tests exercise it?" had no
  answer but grep: **`soma refs`**, and `file:line` in the brief;
- `soma check app.cell` green with broken test files, `soma check .` red on
  every flesh file: the directory form now goes through the entry.

Its verdict against an ordinary code base with tests: faster there for a
rename and a removal (an LSP does both), comparable for the two additions
with better safety here (frames, a proven "never negative"), and for the
rule between machines "the proof is real, with no common equivalent — but
the first natural wording gave a false ✓". Still open from its list: a
`soma rename`; warnings for a signal no one calls any more and for a field
of an untyped Map written and never read; trying a rule without amending
the lock; values read from a `Map` are unbounded for the prover (a typed
slot is needed).

The same round's red team worked on `verify` alone, reading the provers for
what they do not model: 423 probes, 11 false ✓, all in the provers the
skeleton leans on rather than in the skeleton — the interval prover
(binders shadowing a local, a slot named like an invariant binding, a pipe
into a slot write, call sites not counted), the guard proof (a lower-case
cell, a lambda handed to a builtin) and the product (a lambda kept in any
container). One of its reproducers made the frame oracle fail: `x.count()`
ran another cell's handler `count` from a signal with an empty frame — the
one frame leak of rounds 5 to 7, closed in the interpreter. Holes held in
every probe: an unfilled signal never earned a ✓ a conforming body could
break.

### Round 8

One more red team, on `verify` and on the tools added in round 7: about 960
probes, 30 causes of false ✓, one frame leak, and gaps in `soma refs` and
`soma check <dir>`. Closed (tests `r8_*`): the leak (`std.count(5, 1, 2, 3,
4)` — a `std.` call the builtin does not take — ran another cell's handler);
in the product, a string's calls are now read where the string stands and a
`require`'s operands once (`desugar::expose_in_place`), nothing is narrowed
by a condition that moved a machine, `vote(Cell.h, …)` runs `h`,
`deliver` / `resume` / `horde` make a rule "not proven", a machine belongs
to its cell and not to its cell's name; a guard is never "proven" by a hole,
by a condition that undoes itself, by a status read in method form; in the
interval prover, `let`s at any depth of an expression block, a local or a
parameter named like a cell, a handler named like the builtin a rule
recognises, interpolation inside what is proven, facts across an `await`,
a `resume` or a `deliver`; `requires` sites reached by `vote`, `horde`, a
method call on a local. `soma check <dir>` no longer skips a part that has
tests of its own, two files that import each other, or an entry only tests
import; `soma refs` finds the method form, bare calls from another cell,
`vote` / `horde` / `delegate`, nested strings, and reads only the files
tied to the program by an import. Seven more were in 3.0 analyses beside the skeleton and were closed in
the same round: two machines kept under one storage key (refused at
check), the `[task]` wait report (it says what it does not follow, and
reads a transition's source), `effects` (a think() without options offers
every tool), `composition` (an emit at any depth), the latency and token
bounds of `cost`, `[deterministic]` behind a local bound in a branch, and
`verify --attest` (imported files are hashed).

Four rounds on `verify` (5 to 8): 4, 7, 11 then 30 causes — the count rose
because each round read more of the provers, not because they got worse.
The honest state: the frame, the run-time guard and the lock have held
since round 5 (two leaks, both found, one by the oracle); `verify` is
sound on every reproducer kept (rounds 2 to 8 are tests), and is not yet
at the point where a round comes back empty.

### Round 9

20 causes of false ✓, 7 frame leaks, 15 tool gaps — and, for the first
time, a report that named roots instead of cases:

- **two name resolvers.** The interpreter resolved a name in every cell,
  the caller's first; the checker in `cell` / `cell agent`, and read `std.f`
  as the builtin. Every leak of rounds 7 to 9 came through that gap. The
  run now resolves as the checker reads: `std.f` is the builtin alone, a
  call never reaches a handler of a test / type / property cell;
- **an evaluation order written twice.** The product search had its own
  order for method calls, indexing and match patterns; it follows the
  interpreter's now, and counts edges taken instead of comparing sets of
  states;
- **facts compared by text.** `render_expr` leaves bodies out (`if c { … }`),
  so two expressions shared a text and one's `require` proved the other's
  write. A text that leaves something out is no key any more.

The hole model and the run-time lambda guard held wherever probed. What
this round says about method: fixing reproducers one by one found 4, 7, 11
and 30 causes in rounds 5 to 8; the roots above were visible only when a
round was asked to read the code for what two components disagree on.

### Round 10: disagreements, not cases

The red team was given six questions and told to tabulate, for each, the
interpreter's answer and every analysis's: who runs a name, in what order
sub-expressions run, what binds a name and how far, what writes a slot or
moves a machine, what "the same" expression is, what a cell of the program
is. About 25 disagreements, 52 reproducers. Most were settled on the side
that removes the question: the run now resolves `x.f()` as it resolves
`f(x)`, refuses `x |> obj.f`, delivers an `emit` and a `delegate` to cells
of the program only — so there is one answer, and the analyses already had
it. The rest aligned the product search (order, match arms, `std.`,
variants) and the frames (a local that MAY hide a cell or a slot counts
both readings; a slot before a cell of its name).

Still two answers, known: `refs` and `rename` have no notion of scope (a
parameter named like a cell); a `cell test` that listens to an event moves
the machines of its own test (by design); the lists of "slot-writing
methods" are written eleven times in the checker (five different contents —
no consequence found, one source of truth wanted).

### Round 11

The first round where earlier answers were probed again and held: 57 forms
of slot write against the frames, 36 neighbours of the product fixes, 22 of
the guard proof, 25 `requires` call sites. Ten disagreements in what round
10 had not compared, of which one touched the central claim: **the model
of a hole was one ordered pass** over what the frame allows, so `verify`
ticked a `while` rule on a skeleton that a conforming body broke by calling
a signal twice, or by taking two declared edges in the other order. The
model is now those steps under a loop (`flesh::model_body`); both
reproducers are tests. The others: handlers run by name through `vote` /
`horde` (two frame leaks), a `[task]` callee that stops at its wait, the
`runtime { }` section no proof read, `tools_allowed` read at the wrong
argument, taint in method form.

Count of causes per round on the proofs: 4, 7, 11, 30, 20, 25, 10.

### Round 12, and where this stands

The core held: the hole model (every clause, every family of property) and
the product search on a flat program gave no false ✓. The round still
counted 28 causes, all in what had never been compared — interior cells,
timers, `runtime { }` as `check` reads it, type names, tools over several
folders — and its conclusion is the one to keep: **the count falls by
reducing the number of answers, not by closing cases.** Done in that
spirit: one answer to "is this name a cell?", `runtime { }` as an entry of
the exposed program for every analysis, top-level resolution of a bare
call, `-> Int` that gives an Int.

Causes per round: 4, 7, 11, 30, 20, 25, 10, 28. Where a question has one
shared answer nothing is found any more (hole model, evaluation order of
the product, wildcard edges, desugaring before frames, name resolution
since round 10). What still produces disagreements is structural, and is
the work to do before any release:

1. **each analysis walks the AST itself and has its own list of entries**
   (handlers / `every` / `after` / `runtime`): budget, effects and
   capabilities still do not read timers; one function "the executable
   bodies of the program" should feed all of them;
2. **two executors**: the scope runtime of interior cells
   (`runtime/mod.rs`) and the interpreter differ on delivery, resolution
   and storage; the proofs model one;
3. **facts copied instead of shared**: "is an Int" (five lists in the
   prover, four at run time), "where is the project" (loader, manifest,
   lock, tools), the slot-writing methods (eleven lists);
4. **the older analyses** (taint, effects, capabilities, composition,
   budget) are syntactic and per handler: no closure over calls.

## Decisions

- **`skeleton` / `flesh` are new top-level objects**, not annotations on
  `cell`: physical separation is what lets the structure be frozen and the
  right to write flesh be granted alone.
- **Helpers are private by name** (`_helper`), not by a new mechanism.
- **`every` / `after` keep a body in the skeleton but it only calls
  signals**: a schedule has no frame of its own.
- **`moves` grants reading the machine**, as `writes` grants reading the
  slot.
- **A computed transition target is declared, not refused**: `moves * ->
  *`. `airlock` passes the operator's command straight to `transition()`
  and lets the machine refuse what it has no edge for — a legitimate
  idiom, and one line in the skeleton says so.
- **An unclaimed edge or an unwritten slot is a warning**, not an error: a
  design in progress must still check.
- **Guards, invariants and an edge's `effect { }` block belong to the
  structure.** What they read or do is not charged to any frame.

## Not done

- Frames are per cell and syntactic.
- The measure: nothing yet shows that an agent builds a large application
  better this way. The pilot (two ~40-handler apps, three arms, three
  seeds, hidden cross-cell tests) is specified and not run.
- A flesh sees its skeleton's constants when the skeleton is in the same
  file or in the file that imports the flesh — not when the skeleton is
  itself imported by the flesh's file.
- The map view (`soma skeleton` without `--split`) does not print `scale`
  and `runtime` sections, a tool's description or capabilities.
- Finer frames — token budgets, determinism, taint, per-key writes
  (`writes account["balance"]`) — are natural extensions, left out on
  purpose.
- Requiring a test before a signal counts as filled.
- **Known and not closed after round 12** (reproducers in
  `compiler/tests/fixtures/skeleton_review/round12/`): `budget` counts an
  unknown call for 0 bytes and does not read timers (`b2_budget_*`,
  `b3_timer_unseen_*`); `composition` pairs an emit with a listener by name
  alone (`b2_composition_*`); the taint check does not follow a value
  through a list, an index write, a lambda parameter or `vote(…).winner`
  (`a4_taint_*`); the start-up audit of stored data skips a second cell
  with a slot of the same name and every interior cell (`b4_audit_*`); an
  interior cell's `[persistent]` slot is stored by two back-ends
  (`b5_scope_*`); the invariant prover reads the answer of a `[task]`
  callee that waits as its final answer (`b5_invariant_value_*`); the
  manifest is read beside the entry file only, so a test file in a
  sub-folder runs without the project's edition and `[verify]` tables
  (`b7_manifest_*`).
- Rules between machines follow ONE id through every machine: instances
  tied by a slot (an invoice an order names) are not followed.
- `soma refs` and `soma rename` read syntax: a handler named at run time
  (`delegate(cell, name)` with computed names) is not found.
