# RFC 0001 — a `let` named like a memory slot is an error

- Edition: 2027
- Diagnostic codes added: none (E0080 `local_shadows_slot` existed and was unreachable: the rule was filed under W0014)

## Problem

A `let` that takes the name of a memory slot of its cell hides the slot for
the rest of the block. Every later `name.get`, `name.set`, `name[...]` means
the local; the slot is never read or written.

```
cell Compteur {
  memory { total: Map<String, Int> [persistent] }
  on ajouter(k: String, n: Int) {
    let total = n + 1
    total.set(k, total)      // the local, not the slot: nothing is stored
    return total
  }
}
```

`soma check` already said so, as a habit warning (W0014). In Gérance
(a fund-administration case study, not published; 12,000 lines of skeleton and flesh) that warning was
one of 180: it was not read, and the defect was found by a wrong net asset
value in a scenario test (S90, a class conversion), not by the checker. The
same name on a handler *parameter* is already an error
(`param_shadows_slot`); on a `let` it was tolerated "unless that is
intended".

Shadowing has also been a recurring source of holes in the proofs: eight
fixtures of the skeleton review rounds exist only to attack it (`L1`, `B2`,
`b26`, `q5`, …).

## Change

In edition 2027, a `let` whose name is a memory slot of the cell it is
written in is an error:

```
error: `let total` hides the memory slot `total` for the rest of this block —
reads, indexes and writes of `total` now mean the local, and the slot is not
read or written; rename the local (e.g. `total_local`) (edition 2027: an error)
  = code: E0080
```

In edition 2026 nothing changes except the text of the warning, which now
ends with `(an error in edition 2027)`.

Not covered, in either edition: a lambda parameter (`(total, y) => …`, which
the parser reads as a `let` over its packed arguments — excluded), a loop
variable, a `match` binder, a local named like *another* cell's slot.

## Migration

`soma fix --edition 2027` does not rename: the local may be read under that
name by a string interpolation or a lambda, and a mechanical rename is the
author's to check. The diagnostic names the line and proposes `name_local`.

## Measured effect

- Repository programs that shadow a slot with a `let`: 9 of 3,442 `.cell`
  files (Gérance apart: 0 after S90). Eight are test fixtures written for
  the old behaviour, unchanged since they are edition 2026. One is a real
  program, `rebalancer/app.cell` (`set_history`: `let prices`), where the
  local was the intent; it is renamed to `series` in this change, which
  removes its warning in 2026 and its error in 2027.
- Compiler test suite: 1,157 passed, 0 failed, before and after.
- Agent benchmark and backend fuzzer: not run for this change (a check
  only; no change to what a program does).

## Alternatives considered

- **Do nothing.** The warning exists and is correct. It did not prevent the
  defect in the largest Soma program written so far.
- **An error in every edition.** Refuses a program that ran correctly
  (`rebalancer`), which the rules reserve to a new edition. (The first
  version of this change did exactly that, and was corrected.)
- **A lint outside the compiler.** Gérance has one
  (`essais/lint_masquage.py`); it protects one project.
