# Soma improvement program

A work order for a coding agent. It says what is wrong with Soma today, with
numbers; what "better" means, as numbers a script prints; and how to work so
that nothing already true becomes false. Read it whole once, then work from
section 4.

Soma's goal is that *programming agents design large applications in a solid
manner*. The audit behind this program (November 2026, on 3.2.3 then 3.2.4)
found the "solid" half in good shape and the "large" half untested:

| What was tried | What came out |
|---|---|
| `soma verify` against every input, on ~4,000 random handlers | **no false proof** |
| the same, counting correct programs it fails | 79 of 1,471, 40 after a first fix |
| `[native]` against the interpreter, ~2,700 random expressions | no wrong value; **arguments evaluated out of order, or twice**, in 10 builtins (fixed) |
| 37 algorithms far from Soma's domain, answers known | 37 right the first time |
| hostile HTTP, 600 concurrent requests, oversized inputs | no panic, every limit a clear error |
| generated programs of 400–1,500 cells, 4,000 handlers | **`verify` cubic** (400 cells: 25 s; fixed), `check` still quadratic to cubic |
| Orbite, the one large program Soma has (`site/cases/orbite`, 69,000 lines) | `verify app.cell`: 8 minutes on 3.2.4, 4 after the first fix; `verify prod.cell`: does not finish; one test file: over 10 minutes |
| the CI of `main` | red since 3.2.4, four separate causes, two fixed |

So: the prover does not lie, the interpreter is right, and the toolchain is
too slow on the one program that is the size its motto promises — slow enough
that the CI cannot finish on it. That is the gap this program closes.

## 1. Rules that do not bend

1. **Never trade soundness for anything.** A proof that is false is the one
   defect Soma cannot have. `no_false_proof` is a gate; no metric below is
   worth a single false `✓`.
2. **The interpreter is the reference.** When `[native]`, the prover, the
   docs and the interpreter disagree, the interpreter is right until the owner
   says otherwise. Do not "fix" the interpreter to match something else.
3. **Test first.** Every fix lands with a test that fails on the binary
   before it — in `compiler/tests/`, or as a new gate or probe in the harness.
   Say in the commit what failed before ("three assertions failed on 3.2.3").
4. **Never weaken the harness to pass it.** Do not delete a gate, raise a
   noise margin, shrink a seed range, edit `baseline.json` by hand, or add a
   path to `accepted_verdict_changes.txt` without the reason beside it. If a
   gate is wrong, that is a finding: report it, fix the gate in its own
   commit, and show a case it now catches.
5. **A verdict that moves is a decision, not a side effect.** If `check` or
   `verify` answers differently on a tracked program, either the change is a
   bug or it is the point of the change — and then it is named in the
   CHANGELOG and listed in `accepted_verdict_changes.txt`.
6. **One finding, one commit.** Fix, test, CHANGELOG line (under
   `## Unreleased`, in the voice of the file: what was wrong, on what example,
   what it says now). No drive-by refactoring in a fix.
7. **Do not release.** No version bump, no tag, no site deploy, no push to
   `main`. Work on a branch, push the branch, open a PR. The release
   procedure belongs to the owner.
8. **Report what happened.** A red gate is reported red, with its output. A
   metric that did not move is reported as not moved. "Should be faster" is
   not a result; the scorecard is.

## 2. The harness

Everything this program asks for is measured by one command. Nothing counts
as done until it prints `OK`.

```
(cd compiler && cargo build --release)

python3 tools/harness/run.py              # quick — about 1 minute — after every change
python3 tools/harness/run.py --full       # before every commit — today it cannot finish: see W0
python3 tools/harness/run.py --orbite     # the large program: check / verify on site/cases/orbite (minutes)
python3 tools/harness/run.py --full --ref tools/harness/.ref/soma    # before every push
python3 tools/harness/run.py --tighten    # after a real improvement: record it
```

The reference binary — Soma as it was when your work started — is built once:

```
tools/harness/make_ref.sh                 # origin/main -> tools/harness/.ref/soma
```

### 2.1 Gates — yes or no

A run with one red gate has failed (exit 1), whatever else improved.

| Gate | Mode | What it asserts | Tool |
|---|---|---|---|
| `no_false_proof` | quick | on 1,510 random handlers run on every input, `verify` proves no false `ensures` | `tools/harness/prove_fuzz.py` |
| `native_argument_order` | quick | in 82 shapes, `[native]` evaluates arguments in the order written, each once | `tools/harness/native_order.py` |
| `outliers` | quick | 37 assertions on algorithms with known answers | `tools/harness/outliers.cell` |
| `cli_fixtures` | quick | the 118 CLI checks | `tests/run_all.sh` |
| `numeric_oracle` | quick | 1,280 statistics against exact fractions | `tools/check_numeric_oracle.py` |
| `verdicts_unchanged` | `--ref` | `check` and `verify` exit the same as the reference on every tracked program, except those listed in `accepted_verdict_changes.txt` | `run.py` |
| `cargo_test` | full | every Rust test | `cargo test --release --no-fail-fast` |
| `native_values` | full | 3,600 native results equal the interpreter's | `tools/diff_fuzz.py` |
| `split_roundtrip`, `frame_oracle`, `skeleton_monotone` | full | skeleton / flesh: split and compared, frames against observed effects, proofs monotone | `tools/*.py` |
| `grammar_artifacts` | full | `soma.gbnf`, `grammar.md`, tree-sitter are what `soma.grammar` generates | `tools/gen_grammar.py --check` |

### 2.2 Metrics — numbers with a direction

Compared with `tools/harness/baseline.json`. One that got **worse** fails the
run. The baseline is a ratchet: `--tighten` moves it only towards better, and
only when every gate is green.

| Metric | Direction | Baseline | Target |
|---|---|---|---|
| `prover_false_failures` — correct handlers `verify` fails with `✗`, of 1,510 | lower | 42 | **0** |
| `prover_true_clauses_proven` — true `ensures` proven, of 3,020 | higher | 1,083 | as high as soundness allows |
| `scale.<probe>.exponent` — `log2(t(2N) / t(N))` | lower | 1.5 – 2.7 | **≤ 1.3** on every `check` / `verify` probe, **≤ 1.2** on `run` |
| `scale.<probe>.seconds` — time at 2N | lower | see below | see below |

The scale probes (`tools/harness/scale.py`), each a generated program at N
and 2N:

| Probe | 2N | Baseline | Target |
|---|---|---|---|
| `verify_cells_independent` | 800 cells | 0.35 s · 1.78 | 0.15 s |
| `verify_cells_call_chain` | 800 cells, each calling the next | 3.98 s · 2.72 | 0.3 s |
| `check_cells_independent` | 800 cells | 0.12 s · 1.54 | 0.08 s |
| `check_handlers_independent` | 4,000 handlers in one cell | 0.93 s · 1.87 | 0.15 s |
| `check_handlers_call_chain` | 2,000 handlers, each calling the next | 2.10 s · 2.32 | 0.15 s |
| `verify_handlers_independent` | 2,000 handlers | 0.42 s · 1.89 | 0.15 s |
| `run_nested_list` | `v = [v]` 16,000 times | 4.22 s · 2.05 | 0.05 s |
| `run_nested_map` | `v = map("a", v)` 4,000 times | 1.29 s · 2.01 | 0.02 s |

And the one that matters most, measured with `--orbite` on
`site/cases/orbite/src` (13,657 lines of skeleton, 97 rules between machines;
`prod.cell` adds the 1,787 bodies):

| Metric | Baseline | Target |
|---|---|---|
| `orbite.check_app.seconds` | 1 s | 1 s |
| `orbite.verify_app.seconds` | 249 s (475 s on 3.2.4, same output byte for byte) | **20 s** |
| `orbite.verify_prod.seconds` | does not finish in 20 minutes (recorded as 9999) | **finishes**, then 60 s |
| one `soma verify test_*.cell` of Orbite | over 10 minutes | 20 s |

Times are worse only past +30 % and +0.15 s, exponents past +0.25: machines
differ and runs jitter. Compare runs made on the same machine, and take the
baseline again on yours before you start (`--tighten` on a clean checkout
records it).

### 2.3 Extending the harness

The harness is part of the product of this program. Three duties:

- **Every new class of bug becomes a gate or a probe.** A fix whose only
  guard is one unit test will be undone by the next builtin. The
  evaluation-order bugs were found by `native_order.py` in seconds and had
  survived 1,105 tests.
- **A gate must be shown to bite.** When you add or change one, run it on the
  reference binary and show the red it produces (`native_order.py --soma
  tools/harness/.ref/soma` reports 10 shapes on 3.2.3).
- **A gate stays fast.** Quick mode must stay under two minutes or nobody
  runs it after every change.

Each tool prints JSON and takes `--keep` to leave its generated program in
the current directory; that program is the reproducer to put in the test.

## 3. How to work

One finding at a time, in this order:

1. **Reproduce** on the smallest program you can write. If you cannot
   reproduce it, you do not understand it yet.
2. **Read the cause** in the source before you write anything. Name the file
   and the line in the commit message.
3. **Write the test**; watch it fail on the current binary.
4. **Fix** the cause, not the symptom, and look for its siblings: the bug in
   `bit_test` was in `bit_set`, `bit_clr`, `buf_set` and `pow_mod` too.
5. `run.py` quick. Green, or back to 2.
6. `run.py --full --ref …` before the commit. Then commit, CHANGELOG line
   included.
7. If a metric moved the right way, `run.py --tighten` and commit the
   baseline with the fix that earned it.

Stop and ask the owner when: a fix needs a change of documented behavior; a
tracked program's verdict must move and you are not sure it should; two
rules of section 1 seem to conflict; or you have spent three attempts on one
finding without the harness moving.

At the end of each session, report: the scorecard before and after (paste
it), each commit with the test that was red before it, what you tried that
did not work, and what you would do next.

## 4. Workstreams

In order. W0 first: a red `main` hides everything behind it.

### W0 — Make `main` green and keep it green

`--full` must print `OK` on the branch, and the CI of the PR must pass. It has
been red since 3.2.4 for four reasons. Two are fixed on the branch this
program starts from: the grammar did not accept `while [loop_bound(N)] cond`
(`a6_every_cell_file_soma_parses_is_in_the_grammar`), and `frame_oracle.py`
crashed on the non-UTF-8 fixture of E0115. Two are yours:

- **CI cannot finish.** `split_roundtrip.py` and `skeleton_monotone.py` run
  `soma verify` on every tracked `.cell`, and each of Orbite's 59 files takes
  minutes (the job's limit is 30). This is W1's problem showing up as a red
  build. Until W1 lands, do not hide it: the honest interim is a per-file
  time limit that reports the file as `timeout` and a gate that counts
  timeouts — a number that must reach 0 — not an exclusion of the directory.

- **`frame_oracle` reports 3 programs** (`site/cases/orbite/src/test_*.cell`):
  `LancLancement.notifier` calls `LancLancement.creer`, and `LancPiece.cataloguer`
  / `rattacher` call `LancPiece.lier`, outside their frames. Either the frame
  inference misses a call it should see, or the oracle attributes an effect to
  the wrong signal. Find which; a frame that is too small is a soundness
  problem of skeleton-first, not a tooling detail.
- **A flaky test**: `regressions::a_content_type_value_is_checked_like_any_header`
  failed once in a full parallel run ("server on PORT did not answer") and
  passes alone. Find how the port is chosen and how long the test waits.
- CI ran nothing after `cargo test` since 3.2.4 because that step was red.
  Once it is green, read what the later steps say for the first time.

Done when: three consecutive `--full` runs are green, and the PR's CI is.

### W1 — The toolchain at the size of a large application

Targets: section 2.2 — the Orbite times first, then every `check` / `verify`
probe.

**Start with Orbite.** A profile of `soma verify test_ref.cell` (the first
ten minutes of it) is entirely in `checker::product` — `product::check` →
`Engine::block` → `Engine::narrow` → `Engine::branch`: the search of the
rules between machines that 3.2.4 added. 3.2.4 restricted each rule to its
cone of influence, and that made `app.cell` provable at all; the search
inside a cone is what is left. Questions to answer with a profile, in order:
is the same rule searched once per file that `use`s the program (a test file
re-proves all 97 rules of `app.cell`)? is a state visited more than once
(what is the key of the visited set)? how many states and branches per rule
— print them, find the rule that dominates. `README.md` there says what the
authors expect: `verify prod.cell` is where 151 open obligations close.

What is known: `verify`'s termination pass rebuilt the call graph per cell
(fixed: `checker/termination.rs`, `program_scope`). The same shape —
*a pass over the whole program called from inside a loop over the whole
program* — is the first thing to look for elsewhere. Profile before you
guess: `sample <pid> 2` on macOS, `perf` on Linux, on the probe's program
(`scale.py --only check_handlers_call_chain --keep`).

Then extend `scale.py`: `soma test`, `describe`, `skeleton`, `holes`, `fmt`,
`lsp` diagnostics, at 800 cells and 4,000 handlers; one program with 400
state machines and rules between them in `soma.toml`; one cell with 2,000
memory slots. A probe that is already linear costs a second and stays as a
guard.

Trap: `soma run` and `soma test` run `check` first, so every run of a large
program pays it. Trap: a cache keyed on a pointer must not outlive the
program (the LSP is one long process); scope it, as `program_scope` does.

Done when: every exponent is ≤ 1.3, the times are at target, and `verify`'s
output is byte-identical to the reference on every tracked program.

### W2 — Values that are wrapped, passed and stored

Targets: `run_nested_*`, exponent ≤ 1.2.

`v = [v]` copies `v`: building any linked or nested structure is quadratic
(16,000 levels: 4 s; 100,000: 3 minutes). Find where the copy happens
(reading a local? building the literal?), and whether the same copy is paid
when a large list is passed to a handler or returned from one — add those
probes first: a 100,000-element list passed to a handler 10,000 times;
returned 10,000 times; read by index in a loop; a map of 100,000 keys
updated key by key.

The constraint is the language's value semantics, which programs rely on:
`let b = a  b[0] = 99` leaves `a` unchanged. Sharing with copy-on-write keeps
it; anything that lets a write show through an alias is a wrong answer. Write
the aliasing tests before you touch the representation.

### W3 — A `✗` that is always true

Target: `prover_false_failures` 42 → 0, `no_false_proof` green throughout.

`verify` fails a correct program when a statement that breaks a contract
sits on a path no input takes and the prover cannot see that (`if n != n`,
`if n == n % n`, relational facts between locals). `dead_path` in
`checker/invariants.rs` now recognises an empty interval on a parameter and
unsatisfiable linear facts; the rest remain, with a message that is false
("the call raises kind `ensures` on that path").

Two ways, not exclusive: prove more paths dead; or stop claiming more than is
known — a `✗` says *some input breaks the contract*, so it needs a path that
is known to be reachable, and anything else is `⚠ runtime-checked`. The second
changes what a `✗` means and what `--strict` rejects: **ask the owner before
doing it.**

Then widen `prove_fuzz.py`, because it only covers `ensures` on Int results
today: memory invariants on writes, `requires` at call sites, slot reads
(`m.get(k) ?? d`), Float values, nominal types with a `where` bound, loops.
Each widening is a new chance of a false proof — that is the point.

### W4 — `[native]`: make the bug class impossible

Targets: `native_argument_order` and `native_values` stay green while the
table grows.

The ten order bugs had one cause: each builtin's Rust is written by hand as
a format string, twice (i64 and BigInt), and each author chose how to bind
its arguments. Give `codegen/native.rs` one way to bind the arguments of a
builtin — in order, once, in a single pattern so that no temporary shadows a
user variable (`native_temporaries_do_not_shadow_user_arguments` holds the
reason) — and move every builtin to it.

Make `native_order.py` complete by construction: read the native vocabulary
from the compiler (`soma describe --builtins --json`, or the list in
`native.rs`) and fail when a builtin of arity ≥ 2 has no shape. Add the other
half to `diff_fuzz.py`: two arguments that both fail must raise the error of
the first (`bit_clr(5 % n, n - 1)` with `n = 0` is `division_by_zero`, not
`type`).

### W5 — Surface: what an agent trips on

Small, each needs a decision; propose, do not impose. For each: the example,
what happens today, two options, your recommendation.

- An interpolation that does not parse and does not start with a name of the
  scope prints literally and passes `check`: `"{1 < 2 == true}"`,
  `"{(a + b}"`. E0085 exists for `{a < b == ok}`; `code_like_start` in
  `checker/interpolation_check.rs` only looks at the first word.
- `a < b == c` is a syntax error whose message is "expected statement …
  found '=='". Say that comparisons do not chain.
- `to_int("1e3")` is 1000 and `to_int(" 4 ")` is 4, but `parse_int` refuses
  both; `parse_int("0x1F")` is `()` while `0x1F` is a literal.
- `-> List<Lots>` carries no `ensures` where `-> Lots` and a `List<Lots>`
  slot do.
- A dead call site the prover already rated `⚠` stays `⚠`; W3's `dead_path`
  was applied to `✗` only.

### W6 — Keep hunting

The audit found its bugs with four moves; each is cheap to repeat on a part
of Soma it did not reach:

- **Two implementations, one input**: native vs interpreter was done;
  `soma embed` vs `soma run`, a program vs its `skeleton --split`, a value
  vs its storage round trip, `fmt` twice.
- **An oracle from outside**: Python for numbers was done; dates, CSV, JSON,
  regex, sorting and statistics have exact references too.
- **Brute force against a proof**: done for `ensures`; state machines
  (enumerate the runs of a small machine against `eventually` / `never`),
  termination, cost bounds.
- **Size**: W1 and W2.

Not reached at all: `cell agent` and hordes under the mock provider, the
cluster, `[task]` handlers across a `think()`, the storage providers, the
LSP, `soma fix`, the Python and npm packages.

## 5. What "finished" looks like

- `python3 tools/harness/run.py --full --ref tools/harness/.ref/soma` prints
  `OK`, three times in a row, and the PR's CI is green.
- `prover_false_failures` is 0; every scale exponent is at target.
- The harness has more gates and probes than it started with, each shown to
  bite, and quick mode still runs in under two minutes.
- Every commit names the test that was red before it.
- A short note for the owner lists the decisions waiting (W3's meaning of
  `✗`, each item of W5) with a recommendation for each.
