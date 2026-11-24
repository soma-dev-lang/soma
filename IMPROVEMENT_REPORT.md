# Improvement program — session report (2026-11-23)

What was done of `IMPROVEMENT_PROGRAM.md`, what was not, and the decisions
that wait for the owner. Numbers are from `tools/harness/run.py` on one
machine (Apple silicon, 10 cores); the reference binary is 3.2.4
(`origin/main`, c5eddca).

## Where each workstream stands

| | State | In one line |
|---|---|---|
| W0 green `main` | **not green** | the three skeleton tools had never run in CI (fixed); they now run, and say two true things: files they cannot finish, and proofs lost between Orbite's skeleton and its bodies |
| W1 size | Orbite: done for `app`, `prod`, most test files; probes: faster, exponents not all at target | `verify app.cell` 249 s → 10 s |
| W2 values | **done** | lists and maps shared until written; every `run_*` probe linear |
| W3 false `✗` | **42 → 0** (decided 2026-11-24: a `✗` is an input that breaks the clause) | no false proof; false clauses named 306 → 3,020 of 3,020 |
| W4 native | gate complete by construction; the single way to bind arguments is **not done** | 100 shapes, 16 differ on 3.2.4, 0 now |
| W5 surface | proposals below, nothing changed | one of them is a hole in nominal types |
| W6 hunting | four oracles run, clean; two findings | see below |

## The last full run

`python3 tools/harness/run.py --full --ref tools/harness/.ref/soma --orbite`,
on the last commit: **10 gates green, 3 red** — and the three are findings,
not regressions.

| Gate | |
|---|---|
| `no_false_proof`, `native_argument_order` (100 shapes), `outliers`, `cli_fixtures`, `numeric_oracle` | green |
| `verdicts_unchanged` | green — `check` exits the same on 1,855 programs |
| `verify_output_unchanged` | green — 1,810 print what 3.2.4 prints; 3.2.4 cannot finish the other 45 in a minute |
| `cargo_test` | green — 1,119 |
| `native_values`, `frame_oracle`, `grammar_artifacts` | green |
| `split_roundtrip` | **red** — 4 files not finished in 120 s (1,224 ok) |
| `skeleton_monotone` | **red** — 11 files not finished, and **31 Orbite files where a rule proven on the skeleton is not proven with the bodies** |
| `orbite_verify_prod` | **red** — `verify prod.cell` finishes and answers `VERIFY FAILED` |

The baseline was not tightened: `--tighten` refuses while a gate is red, and
no number was written by hand. Every metric it would record is better than
the baseline or equal.

## Scorecard

| Metric | Start of session | Now | Target |
|---|---|---|---|
| `orbite.verify_app` | 249 s (475 s on 3.2.4) | 3.5 s | 20 s |
| `orbite.verify_prod` | did not finish in 20 min | 13 s | finishes, then 60 s |
| `verify test_ref.cell` | did not finish in 10 min | 9 s | 20 s |
| `verify test_coordination.cell`, `banc_coordination.cell` | did not finish | 60 s (3.2.6: a summary per handler call) | 20 s |
| `verify test_aeolus.cell`, `test_bout_en_bout.cell` | did not finish | 15 s | 20 s |
| `prover_false_failures` (of 1,510) | 42 | 0 | 0 |
| `prover_false_clauses_refuted` (of 3,020) | 306 on 3.2.4 | 3,020 | — |
| `prover_true_clauses_proven` | 1,083 | 1,120 | — |
| false proofs | 0 | 0 | 0 |
| `run_nested_list` (16,000 wraps) | 4.2 s | 0.006 s | 0.05 s |
| `run_nested_map` | 1.3 s | 0.005 s | 0.02 s |
| list of 20,000 passed 20,000 times | 9 s | 0.016 s | — |
| `check_handlers_call_chain` (2,000) | 2.10 s · 2.32 | 0.08 s · 1.2 | 0.15 s · ≤ 1.3 |
| `check_handlers_independent` (4,000) | 0.93 s · 1.87 | 0.23 s · 1.7 | 0.15 s · ≤ 1.3 |
| `verify_cells_call_chain` (800) | 3.98 s · 2.72 | 0.45 s · 1.8 | 0.3 s · ≤ 1.3 |
| `verify_cells_independent` (800) | 0.35 s · 1.78 | 0.22 s · 1.6 | 0.15 s · ≤ 1.3 |
| `verify_handlers_independent` (2,000) | 0.42 s · 1.89 | 0.24 s · 1.8 | 0.15 s · ≤ 1.3 |
| Rust tests | 1,112 | 1,119 | — |

`verify` prints the same, byte for byte, as the reference on the 1,809
tracked programs the reference finishes in a minute (`verify_same.py`), and
on Orbite's `app.cell`.

## Commits, and the test that was red before each

| Commit | Red before |
|---|---|
| The skeleton tools take a relative `--soma` path… | `frame_oracle.py` on `test_lancements.cell`: `MISSED: 1`; CI log of 3.2.3: `FileNotFoundError` |
| verify: rules between machines at the size of Orbite | `v3_product::a_sweep_of_tries_…` (24 tries: no answer in 60 s) |
| verify: two states that differ only in an undecided id choice are one | `v3_product::reading_the_status_of_many_ids_…` (22 reads: no answer in 60 s) |
| check and verify: four passes over the whole program once per handler | `regressions::a_long_chain_of_calls_…` |
| Tests: … a test server that lost its port starts again | not reproduced on demand (said so in the commit) |
| A list or a map is shared until it is written… | `regressions::wrapping_passing_and_returning_a_large_value_…` (over 10 s) |
| verify: three more statements no input reaches are not a ✗ | `v3_contracts::a_statement_after_a_certain_return_…` (six ✗ on 3.2.4, three of them wrong) |
| native_order: every builtin of the native vocabulary has a shape… | the gate itself: three builtins uncovered; 16 shapes differ on 3.2.4 |
| The skeleton tools count a file they could not finish | `split_roundtrip` reported `ok` for two `verify` that both timed out |

## Findings nobody had seen

1. **`verify prod.cell` answers `VERIFY FAILED`: 228 `✗`, 3,846 `✓`**, and
   `skeleton_monotone` — run on Orbite for the first time — reports 31 files
   where rules between machines that `app.cell` (the skeleton) proves are
   lost once the bodies are there (`test_ref.cell`: six, e.g. "entering
   RefObjet.en_service from catalogue while RefRegistre.immatricule |
   RefRegistre.a_immatriculer"). The two are one finding: what the skeleton
   promises, the flesh is not shown to keep.
   Orbite's README says this is where 151 open obligations close; the command
   had never finished. `verify test_ref.cell` fails too (22 `✗`, each marked
   "in the rule's cone … this run may be spurious"). Not analysed: whether
   these are real runs of the handlers or the search's over-approximation is
   the first thing to find out.
2. **The three skeleton tools never ran in CI** (since at least 3.2.3): the
   step died on the relative path of the binary.
3. **A nominal type's bound does not hold inside a returned list.** With
   `cell type Lots = Int where value >= 1 && value <= 100`, `-> Lots`
   returning 500 is a `✗` and a run-time `ensures` error; `-> List<Lots>`
   returning `[500]` passes `verify` and returns `[500]`. (W5, below.)
4. `nth` is in the checker's native vocabulary; the native compiler has no
   such call.

## The 228 ✗ of `prod.cell`, read (2026-11-24)

Finding 1 above is answered in part. The ✗ were of three kinds, and of two causes.

| Kind | 3.2.6 | Now | Cause |
|---|---|---|---|
| "needs the link written once" | 88 | 15 | the prover asked for one spelling of the guard; Orbite writes four others (an `[immutable]` slot, a key from `next_id()`, the guard on a sibling slot, `!slot.has`) |
| "handler calls nest deeper than 12" | 26 | 0 | a limit of the prover; 40 now |
| "a run … enters X without it" | 113 | 94 | both: getters the prover did not read through (fixed for five shapes), **and handlers that do not check** |
| total ✗ / ✓ | 228 / 3,846 | 110 / 3,964 | |

**True defects of Orbite.** `ReeRisque.figer` was run: it enters `fige` on an open campaign, against the rule on its own edge. Five more handlers read the same way (`EvtRapports.classer`, `EvtCollisions.classer`, `EvtNuages.ouvrir`, `RespCommissions.saisir`, `EscPriseEnCharge.ouvrir_prise`; not run) — the frame states `moves a -> b when Other.state`, the body checks nothing, a comment says the caller does. Under the model `verify` uses (every handler is an entry point) these ✗ are right.

**Not known yet.** Of the 94 runs left: 8 end in a handler that never reads the required cell (7 handlers; 6 read, all of the kind above); 23 end in a handler that does read it, through a shape the prover still loses; 32 enter through another cell and 32 from a timer — a few read (`FdvPlan.clore` checks, and is still lost). 15 links are left (a key that is a local name of a parameter, two writers).

**Decision D8 — a handler only another handler may call.** These handlers say "called by X alone" in a comment. Soma has no way to state it, so `verify` must take them as entry points, and it is right to fail them.
- Option A: nothing; such a handler checks its own rule (a `require` reading the other machine), as the note of `verify` already says.
- Option B: a property on the signal (`[internal]`, or `called_by X.f`) that `check` enforces (no route, no call from elsewhere) and that removes the handler from the entry points of the search.
- **Recommendation: A now** — it is what makes the frame true of the handler alone, which is what a frame is for; B is a language change to weigh on its own.

## What was tried and did not work, or was left

- *(Done in 3.2.6: the summary per handler call below was written; the four files take 15 to 60 s. What follows is the note as it stood.)*
- **Four Orbite test files still take more than ten minutes** to verify
  (`test_coordination`, `test_aeolus`, `test_bout_en_bout`,
  `banc_coordination`). No state explosion is left there (the largest set of
  states is under 20,000); single searches of the coordination rules over the
  bodies take 80 to 210 s. The next thing to try is a summary per handler
  call (what a callee does to a set of states, kept by callee and argument
  pattern), which the search does not have: it walks every body again from
  every state and every entry point.
- **`check` on one very wide cell** (8,000 handlers) went from 7.7 s to
  2.5 s and is still more than linear: `frame::build_with`,
  `Checker::check_cell` and `interpolation_check` each search a list of the
  cell's signals per handler. Not done: a long tail of small sites.
- **W4, one way to bind a builtin's arguments** in `codegen/native.rs`: not
  started. The gate now catches the bug at once; the code can still be
  written wrong.
- **W3, widening `prove_fuzz.py`** (invariants on writes, `requires` at call
  sites, Float, nominal types, loops): not started.
- **The flaky test** was not reproduced; its fix is read from the code.

## Decisions waiting for the owner

**D1 — what a `✗` means (W3). Decided and done (2026-11-24):** an unproven `ensures` is tried by running the handler; a `✗` shows the input (`checker/witness.rs`). What follows is the question as it stood. Before, `✗ … can never satisfy it` is said of a
`return` the prover cannot show to be unreachable. 12 of 1,510 random correct
handlers still get it (47 of 3,083 on seeds never tuned on), each for
arithmetic the prover does not do (`if n != n % n`).
- Option A: keep proving more paths dead, one shape at a time. Never ends.
- Option B: a `✗` needs a path known to be reachable; anything else is
  `⚠ runtime-checked`. `--strict` then rejects fewer programs, and a `✗` is
  always true.
- **Recommendation: B**, with the witness input printed when the prover has
  one. A verdict that is sometimes false teaches agents to ignore it.

**D2 — a nominal bound inside a container (W5).** `-> List<Lots>` and
`-> Map<String, Lots>` carry no `ensures`, where `-> Lots` and a
`List<Lots>` slot do.
- Option A: check every element at the return, as a slot does on write.
- Option B: refuse the type in a signature until it is checked.
- **Recommendation: A.** It is the same rule as the slot's, and with shared
  lists a value already checked against a type is not gone through again.

**D3 — an interpolation that does not parse (W5).** `"{1 < 2 == true}"` and
`"{(a + b}"` print literally and pass `check`; E0085 only fires when the
segment starts with a name in scope.
- Option A: any `{…}` that looks like code (an operator, a parenthesis, a
  call) and does not parse is an error; `{{` stays the way to write a brace.
- Option B: a warning.
- **Recommendation: A in edition 2027, B before.**

**D4 — `a < b == c` (W5).** A syntax error: "expected '{', found '=='".
- **Recommendation:** keep it an error, and say why: "comparisons do not
  chain: write `(a < b) == c`".

**D5 — `to_int` and `parse_int` disagree (W5).** `to_int("1e3")` is 1000 and
`to_int(" 4 ")` is 4; `parse_int` answers `()` for both, and for `"0x1F"`
though `0x1F` is a literal.
- Option A: `parse_int` is the strict one by design; document the two side
  by side and make `parse_int("0x1F")` 31, as the literal.
- Option B: align `to_int` on `parse_int` (a change of behaviour).
- **Recommendation: A.**

**D6 — `nth` in the native vocabulary.** Remove it from
`ALLOWED_BUILTINS`, or implement it for `List<Int>` / `List<Float>`
parameters. **Recommendation:** remove; `xs[i]` exists.

**D7 — CI.** With the tools fixed, CI is red for a true reason: the four
slow files. Either the limit of the job is raised and the tools' timeouts
stay red until W1 closes them, or those four are listed as known-slow with
their own number in the harness. **Recommendation:** the second, in a file
like `accepted_verdict_changes.txt`, each line with its reason — an
exclusion nobody has to remember is the kind this program was written
against.

## Next, in order

1. Read 20 of the 228 `✗` of `prod.cell`: real, or the cone's
   over-approximation?
2. A summary per handler call in `checker/product.rs` (the four slow files).
3. D1 and D2, once decided.
4. W4's single binding of arguments; W3's wider fuzz.
