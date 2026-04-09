# Soma V1 / V1.1 examples

The five `TARGET_SOMA_V1` features, one example each. Each file is a
self-contained, runnable demo. Run `soma check` first to see the static
guarantees, then `soma run` to see them in motion.

| File                                  | Feature               | Try                                                                       |
|---------------------------------------|-----------------------|---------------------------------------------------------------------------|
| `01_session_types_auction.cell`       | session-typed signal  | `soma check 01_session_types_auction.cell`                                |
| `02_replay_trader.cell`               | record / replay       | `soma run --record 02_replay_trader.cell && soma replay 02_replay_trader.cell` |
| `03_prove_payment.cell`               | Lean 4 proof export   | `soma verify 03_prove_payment.cell --export-proof`                        |
| `04_adversary_quorum.cell`            | declarative threat    | `soma verify 04_adversary_quorum.cell`                                    |
| `05_causal_orders.cell`               | causal memory (default-on) | `soma run 05_causal_orders.cell`                                     |

See `docs/SEMANTICS.md` for the normative semantics that backs every
guarantee these examples make.

## V1.1: brackets are compiler output, not user input

After V1 shipped, we recognised that `[record]` and `[causal]` were a
category mistake. The intuition for `[brackets]` is "tags": adjectives
that *describe* a thing without changing what it does. But:

  - `[record]` writes to disk on every call — a side effect, not a tag.
  - `[causal]` adds new operations (`clock_of`) — semantic, not metadata.

V1.1 removes both from the user-facing syntax:

  - **`[record]` → `soma run --record`** — recording is opt-in at the
    command line, off by default, zero overhead. The user no longer
    annotates handlers; the operator decides what to record.
  - **`[causal]` → default-on** — every memory slot transparently
    carries a per-key vector clock. The cost is one extra HashMap
    per slot. Hot paths can opt out with `[uncausal]`.
  - **Bracket on memory slots is now optional** — `orders: Map<String, String>`
    is a legal slot declaration.

The brackets that remain (`[native]`, `[persistent]`, `[consistent]`)
are the ones that pass the smell test "if I delete this, does the
program still do the same thing?". Effects don't go in tag bags.

## Why this is "in the spirit of Soma"

- **`protocol` blocks** look exactly like signal declarations, just at
  the program scope. Same shape; zero new vocabulary except `loop` and
  `choice` for branches.

- **Recording** is now an *operator* concern (the person running the
  program decides), not a *programmer* concern (the person writing the
  cell). This matches Soma's philosophy that operations live in
  `scale {}` blocks and CLI flags, not in handler bodies.

- **`prove`** lives next to `state`, like `face` lives next to `memory`.
  It's the exportable counterpart of an `assert` rule.

- **`adversary`** lives next to `cell`, like `cell property` does.
  The `survives:` clause in `scale {}` references it by name — same
  shape as the existing `shard:` and `consistency:` clauses.

- **Causal memory** is just *what memory does now*. No annotation. The
  vector clocks are always there; `clock_of()` and `happens_before()`
  let handlers inspect them when they care.

Five features, zero new vocabulary categories, two annotations
*subtracted*. The cell calculus absorbed every feature, and V1.1
proved that subtraction is sometimes the right answer.
