# Soma V1 examples

The five `TARGET_SOMA_V1` features, one example each. Each file is a
self-contained, runnable demo. Run `soma check` first to see the static
guarantees, then `soma run` to see them in motion.

| File                                  | V1 feature           | Try                                                          |
|---------------------------------------|----------------------|--------------------------------------------------------------|
| `01_session_types_auction.cell`       | session-typed signal | `soma check 01_session_types_auction.cell`                   |
| `02_replay_trader.cell`               | record / replay      | `soma run 02_replay_trader.cell && soma replay 02_replay_trader.cell` |
| `03_prove_payment.cell`               | Lean 4 proof export  | `soma verify 03_prove_payment.cell --export-proof`           |
| `04_adversary_quorum.cell`            | declarative threat   | `soma verify 04_adversary_quorum.cell`                       |
| `05_causal_orders.cell`               | causal memory        | `soma run 05_causal_orders.cell`                             |

See `docs/SEMANTICS.md` for the normative semantics that backs every
guarantee these examples make.

## What's "in the spirit of Soma" about each?

- **`protocol` blocks** look exactly like signal declarations, just at
  the program scope rather than the cell scope. Same `signal_name(p:
  T)` shape. Zero new vocabulary; the only new concept is `loop`/`choice`
  for branches and that the compiler refuses to deploy if a handler is
  missing.

- **`[record]`** is just another handler annotation, like `[native]` and
  `[pure]`. Recording is automatic; replay reads the same `.somalog`
  file the run produced.

- **`prove`** lives next to `state`, like `face` lives next to
  `memory`. It's the exportable counterpart of an `assert` rule.

- **`adversary`** lives next to `cell`, like `cell property` does.
  The `survives:` clause in `scale {}` references it by name —
  same shape as the existing `shard:` and `consistency:` clauses.

- **`[causal]`** is a memory property, like `[persistent]` and
  `[consistent]`. The runtime tracks vector clocks transparently;
  the only new builtins are `clock_of` and `happens_before` for
  inspection.

Five features, zero new vocabulary categories. The cell calculus
absorbed every one.
