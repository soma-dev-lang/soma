# RFC 0003 — the law as an abstract machine, the skeleton as its refinement

- Status: phase 1 implemented in 3.5.0 (steps 1–7 of the plan). The answers to the design note's open questions are proposals the language owner has yet to confirm; phases 2 and 3 are described so that phase 1 closes no door.
- Edition: any release (additive: new words in a `.law` file and three clauses that are syntax errors today)
- Diagnostic codes added: E0132–E0140; the two warnings are W0020 `law_warning` (the table in "Diagnostics")
- Builds on RFC 0002 (the law). Source of the design: the note "la loi comme machine abstraite, le squelette comme raffinement"; the scope between cells is the owner's decision of 2026-11-27 ("option 3").

## Problem

RFC 0002 joins the law to the code by annotations written by hand. The tool
checks that a rule is *cited* where it should be, not that it is *kept*.
Three programs show it; each was run on Soma 3.3.0 before this RFC was
written, and each passes `soma check` and `soma law` without a word.

1. **A guard that refuses nothing.**
   `require true else Frozen "…" under "SAN-02"`: `soma law` answers
   `status checked`, `nothing to do`. A test `under "SAN-02"` that pays a
   frozen holder and asserts that the payment *succeeds* is counted as the
   rule's test.
2. **A forgotten path.** A signal whose frame says `writes solde` and which
   declares no `realizes` raises no diagnostic: E0130 only sees the signals
   that chose to say what they realize.
3. **A program that leaves the law.** Removing the `[law]` table from
   soma.toml passes `soma check`, locks in place: the list of the laws a
   program is under is locked nowhere.

A fourth fact shaped the design: the frame of a skeleton refuses a guard
that reads a slot its signal does not declare (`reads etat_de`, outside its
frame). A guard the toolchain derives must be granted its reads.

## Change

One relation, applied twice: **refinement**. The flesh refines the skeleton
(frames, since 3.1). The skeleton now refines the law: every behaviour of
the program, seen through a correspondence, is a behaviour the law permits.

That needs two things 3.3 does not have. The law gets a **state** and
**actions** — still naming no code. And `realizes` becomes a **binding** of a
signal to an action, with a **map** from the program's memory to the law's
state, instead of a label.

### 1. The law: entities, states, actions, and a formal reading of a rule

```
law Sanctions

entity Holder "Whoever holds units of the fund, on its register."

state frozen(Holder): Bool "The holder is under an asset freeze."

action Payment(h: Holder, n: Int)
    "Cash paid by the fund to a holder: a redemption, a distribution, an indemnity."
action Freeze(h: Holder)   "The holder is placed under an asset freeze."
action Unfreeze(h: Holder) "The freeze on the holder is lifted."

rule "SAN-02" "No payment to a frozen holder"
    about    Payment, Holder
    source   "Regulation (EU) 269/2014, art. 2"
    owner    "compliance"
    requires Payment(h, n): not frozen(h)
```

- `entity Name "definition"` — a thing the law speaks of.
- `state name(Entity): Bool | Int "definition"` — a fact about one entity.
- `action Name(param: Type, …) "definition"` — something that happens.
  Parameter types: an entity, `Int`, `String`, `Bool`.
- `requires Action(params): condition` under a rule — the action is allowed
  only when the condition holds. Parameters bind by position.
- **The condition language is closed**: `not`, `and`, `or`, parentheses, a
  state applied to a parameter, and a comparison (`==`, `!=`, `<`, `<=`,
  `>`, `>=`) between a state `Int` or a parameter `Int` and a literal or a
  parameter. No arithmetic, no call, no quantifier. The owner of the law
  must be able to read it; a rule that does not fit stays a sentence, with
  its 3.3 status.

`entity`, `state` and `action` are terms: they may appear in `about`. `term`
and a rule without `requires` keep their 3.3 behaviour exactly.

### 2. The skeleton: the correspondence

```
skeleton Fund {
    memory {
        paid:   Map<String, Int>    [persistent]
        status: Map<String, String> [persistent]
    }

    maps frozen(h) = (status.get(h) ?? "valid") == "frozen"

    face {
        signal redeem(holder: String, n: Int) -> Int
            writes paid
            realizes Payment(holder, n)
        signal freeze(holder: String) -> String
            writes status
            realizes Freeze(holder)
        signal purge() -> Int
            writes paid
            silent "removes zero balances; no holder's state changes"
    }
}
```

- `maps state(p) = expression` — a clause of the skeleton, beside `memory`.
  It defines one state of the law from the program. The expression is pure
  and has the type the law declares. A state is mapped at most once in a
  program. **What it may read** (the owner's option 3):
  - the slots of its own cell;
  - the state of a machine of its own cell (`get_status(p)`);
  - **another cell**, through a signal of that cell whose frame is
    read-only (no `writes`, `moves`, `emits`, no effectful call):
    `maps frozen(h) = Register.state_of(h) == "frozen"`.
- `realizes Action(arg, …)` — the signal **is** this action, its arguments
  bound to the action's parameters. `realizes Term` without arguments keeps
  its 3.3 meaning and gives no derived guard.
- `silent "reason"` — the signal is no action of the law. The reason is
  mandatory, declared and reviewed (phase 2 proves it).

### 3. Obligation 1 — the derived guard

For each signal that `realizes A(args)` and each rule `requires A(params):
C`: substitute the arguments in `C`, replace each state by its `maps`, and
the result is a **precondition of the signal**, evaluated when the handler
is entered, before any effect. If it is false the call fails with kind
`law`, and a message that carries the rule's identifier, its sentence and
its source. A handler that fails leaves no write: atomicity does the rest.

- The rule's status is `checked`, said to be **derived**. The agent that
  writes the body does not write the guard and cannot forget it.
- The guard's reads are granted: they need no `reads` or `calls` clause in
  the signal's frame, and they do not count as the body's.
- A hand-written `require … under "ID"` on a rule that has a formal reading
  stays allowed and **no longer counts as what serves the rule**: program 1
  of the problem stops passing.
- A test `under "ID"` of a rule that has a formal reading must hold an
  `assert_fails`: a test that only shows the allowed case is a warning.
- `proved` (a guarded edge) still wins over `checked`.

### 4. Obligation 2 — closure by effects

A slot or a machine is **under the law** when a `maps` reads it, directly or
through the read-only signals it calls.

Every signal whose frame `writes` a slot under the law, or `moves` a
machine under the law, must carry `realizes Action(…)` or `silent "…"`.
A private helper counts against each signal that reaches it, as for frames.
This is judged on the frames, before any flesh exists.

It closes the forgotten path: the coverage of the law rests on the frames,
which are checked, not on the honesty of annotations. It also means the law
must name who may freeze (`Freeze`, `Unfreeze`): that is intended.

### 5. The list of laws is locked

`law.lock` records the `[law] files` list with each file's digest. A
program whose soma.toml no longer names a locked law fails the check
(program 3 of the problem).

### Diagnostics

| Code | Kind | Refused | The fix the message gives |
|---|---|---|---|
| E0132 | `law_condition` | a rule's condition outside the closed language | the nearest permitted form |
| E0133 | `law_requires_unknown` | `requires` names an action, a parameter or a state the law does not declare | the declaration to ask the owner for |
| E0134 | `law_maps_unknown` | `maps` of a state no law declares, or mapped twice | the name expected, or the other definition |
| E0135 | `law_maps_impure` | `maps` that writes, has the wrong type, or reads what it may not | what it may read |
| E0136 | `law_state_unmapped` | a state a rule needs has no `maps` in the program | the line `maps …` to write, and where |
| E0137 | `law_realizes_signature` | `realizes A(args)`: arity or types that do not fit | the action's signature |
| E0138 | `law_unbound_effect` | a signal writes a slot or moves a machine under the law with neither `realizes` nor `silent` | the two clauses possible |
| E0139 | `law_silent_reason` | `silent` without a reason | — |
| E0140 | `law_unlocked` | a law in `law.lock` that soma.toml no longer names | the line to restore |
| W0020 | `law_warning` | an action no signal realizes; a test `under` a formal rule with no `assert_fails` | — |

### Commands

- `soma law app.cell` — for a rule with a formal reading: the abstract
  condition, the concrete guard derived, the signals that carry it.
- `soma law app.cell --map` — the dictionary the owner of the law reviews:
  each state and its `maps`, each action and the signals that realize it,
  each `silent` signal and its reason. `--json`.
- `soma skeleton app.cell Cell.signal` — the brief shows the action
  realized, the rules owed and the guards derived: the agent never opens
  the law.
- `soma next` hands out E0136 and E0138 like any error of check.
- Locks: the formal content of the law is in `law.lock` (the whole file
  already is); `maps`, `realizes` and `silent` are tokens of the skeleton
  and so already in `skeleton.lock`.

## Answers proposed to the open questions

The design note asks that these not be decided alone. What the code
suggests, for the owner to confirm:

1. **`realizes Action(args)`** extends the keyword without ambiguity: after
   the name, a `(` starts the binding. No new word.
2. **`maps` at skeleton level**, not in `memory`: with option 3 it reads
   machines and other cells, which are not memory.
3. **The derived guard is its own list on the signal**, shown in the brief
   and in `soma skeleton` as `law "SAN-02": not frozen(holder)  ⇒  <the
   concrete condition>`, and not merged into the face's `requires`. A face
   `requires` reads parameters only and `verify` judges it at call sites; a
   law guard reads memory and cannot be judged there.
4. **Skeletons only.** Closure by effects rests on closed frames. On a
   plain `cell`, `maps` and `realizes A(args)` are refused, naming `soma
   skeleton --split`.
5. **Additive, any edition.** No program that runs today is refused: the
   new refusals apply only to a law that uses the new words. One exception
   to weigh: E0140 refuses a program whose soma.toml dropped a locked law —
   that program was already outside its own lock, so it is treated as a
   defect fixed, not as a breaking change.

## Migration

None for programs. A law moves rule by rule: declare `entity` / `state` /
`action`, give the rule its `requires`, add `maps` and the bindings; the
check then lists the hand-written guards that no longer count and the
signals that owe a binding.

## Measured effect

- **The three programs of the problem** no longer pass: the empty guard is
  refused (a hand-written tag does not serve a formal rule), the signal
  that writes under the law without a binding is E0138, the program that
  dropped its `[law]` table is E0140. Each is a test of
  `compiler/tests/law_refinement.rs` (16 tests).
- **Programs under a 3.3 law**: same outputs. The Rust suite (1,199 tests)
  passes; the pilot of RFC 0002 checks as before, in the same 0.2 s.
- **Every way into a handler**: the derived guard was run by `soma run`,
  `soma test`, `soma verify`, under `soma serve` (the refusal answers 400,
  kind `law`, with the rule in its text) and on a `[native]` handler.

### What the pilot of RFC 0002 shows, read against this RFC (not yet ported)

The fund's sanctions law has four rules. Reading its signals against the
forms of this RFC, before changing any of them:

| Rule | Signals that realize its term | Fits phase 1? |
|---|---|---|
| no transfer from or to a frozen holder | `transferer(…, de, vers, …)` | yes: `Transfer(from, to)`, `not frozen(from) and not frozen(to)` |
| no payment to a frozen holder | `regler(…, p)` — `p` is an order | yes, through a link: `Payment(p via holder_of, …)` |
| | `payer_retenu(…, d, porteur)` | yes |
| | `executer(…, f)`, `valoriser(…, f, …)` — one call pays every holder of a fund | **no**: the action happens once per holder inside the call; a precondition on the call cannot say "skip the frozen ones" |
| no subscription for a frozen holder | `recevoir(…, porteur, …)` | **not as it stands**: the program records the order, then rejects it — a derived guard would refuse to record it |
| an unfreeze is motivated, and made by another than who froze | `debloquer(…)` | **no**: separation of roles is a state of history (phase 3) |

Two findings for the owner of the language:

1. **A batch has no form.** A signal that performs an action for many
   entities in one call is outside phase 1. Either the program is
   restructured — the batch calls one signal per entity, which carries the
   guard, and handles its refusal — or the rule stays a sentence. Because
   a rule is formal for all the signals that realize its action, one batch
   keeps the whole rule informal. This is the first thing a second step
   should address (an action realized *inside* a signal, per element).
2. **"Refused" is not the only way to keep a rule.** Recording an order and
   rejecting it keeps "no subscription is executed"; the action the rule
   speaks of is the execution, not the reception. The law must name the
   action precisely, and the program must have a signal that is that
   action — which is a question of modelling the owner of the law settles,
   not the compiler.

3. **The closure sees an action only through a state it changes.** Writing
   the site's example showed it: with `maps frozen(h)` alone, a second
   signal that `writes paid` and declares nothing passes — `paid` is under
   no `maps`. The design note's own acceptance case ("add `indemniser` with
   `writes solde`") has the same hole. The forgotten payment is caught once
   the law has a state a payment changes (`state paid_to(Holder): Int`) and
   the skeleton maps it. Phase 2's `effect` on an action would make that a
   requirement instead of a habit; until then the guide says it.

The port itself changes the fund's law and its structure, and is left to
its owner.

## Implementation plan

Each step lands with its tests, red first.

1. The `.law` parser: `entity`, `state`, `action`, `requires` and the closed
   condition language (E0132, E0133); `law.lock` holds the list (E0140).
2. Parser and AST of the skeleton: `maps`, `realizes A(args)`, `silent`;
   grammar files; static checks (E0134, E0135, E0137, E0139).
3. Closure by effects on the frames (E0138), unmapped states (E0136).
4. The derived guard: synthesized at load for every command (run, serve,
   test, verify), its reads granted to the frame checker; error kind `law`.
5. `soma law` (formal rules, `--map`), the brief, `soma next`.
6. Other cells (option 3): `maps` through read-only signals, the closure
   following them; the pilot.
7. Documentation: `soma docs law`, `soma docs method` (the "known limit" of
   the law is withdrawn where a rule has a formal reading), one page per
   diagnostic, changelog.

## Later phases (not implemented by this RFC)

**Phase 2 — prove.** `always condition` in the law, proven by induction
through `maps`; `effect` on an action; `silent` proven (the body leaves
every `maps` unchanged); tests derived from the form of a rule, and
mutation — removing a guard must fail a test `under` it.

**Phase 3 — the law alone.** Exploring a law without a program; allowed and
forbidden scenarios shown to the owner and reused as tests; states of
history (`only after`, `at most once`, separation of roles); sub-typing of
actions; structured requests for amendment; the fate of `exempt`.

Out of scope: liveness, and an abstract action that crosses several cells.

## Alternatives considered

- **Do nothing.** The three programs keep passing; the method page keeps
  its "known limit" on what a rule tag proves.
- **Cell-local `maps` only** (the note's phase 1 as written). Smaller, but
  the pilot's rule does not fit: its state lives in another cell than its
  payments. Rejected by the owner (option 3).
- **Merge the derived guard into the face's `requires`.** One list instead
  of two, but a face `requires` may read only parameters, and `verify`
  judges it at every call site; a guard over memory would be a false
  failure there.
- **Write the guard into the flesh** (generate a `require`). The agent
  could then edit or delete it, which is what the derivation is for.
