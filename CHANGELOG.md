# Changelog

## Unreleased

### Lifecycle and data: `status` invariants, declared transition sources, proven guards

- An invariant may read `status`, the state of the machine instance whose id
  is the written key: `invariant status != "released" || (balances ?? 0) == 0`
  refuses a released escrow that keeps a balance. It is checked on every write
  of the slot and on every `transition()` of that id (with the target state);
  `verify` lists it as runtime-checked (a Note), `check` refuses it in a cell
  without a state machine.
- `transition(id, from, to)` declares the source of a move. `soma check`
  verifies the edge `from -> to` exists (and both states); at run time the call
  raises `invalid_transition` when the instance is anywhere else, even if an
  edge into `to` exists from there. `verify` shows the edge a handler takes
  (`fund ⟶ {open → funded}`), counts a declared source as reaching only that
  edge, and notes the handlers that still call the two-argument form.
- Transition guards are proven per calling handler: a guard made of
  comparisons of a handler local with a literal is proven when every handler
  that transitions to that edge narrows those variables with a `require` (or
  an enclosing `if`) before the call and does not reassign them in between.
  Unproven guards are ⚠ runtime-checked, so `verify --strict` no longer
  passes on a guard the runtime alone enforces. Compatibility: programs with
  guards that were green under `--strict` may now need a `require` before the
  transition, or lose `--strict`.
- The corpus gains `state_machines/wildcard_failure`: `* -> failed except […]`
  with `eventually`, the combination that traps a first attempt.
- Typed machines: a handler whose transition target is another variant no
  longer counts as taking every guarded edge (`check` refused a guard reading
  a local — "handler `reopen` has no variable 'amount'" — for a handler moving
  to `Open`). The turnstile example's note claimed a move to the current state
  is a silent no-op; every move needs a declared edge, and the note says so.

### Stable output, honest rendering, negated `require`

- `verify` printed terminal states, deadlocked states, liveness violations and
  the `[verify.after.*]` properties in hash order: two runs of the same file
  could differ, which made a diff of its output useless in CI. They now come in
  declaration order (of the machine, of the manifest). `check` sorted the
  `implies` notes of a custom property the same way; they are alphabetical.
- `describe --faces` and the `soma test` report printed an invariant or an
  assertion in its desugared form: `_coalesce(balances, 0)` for
  `balances ?? 0`, and `!n < 0` for `!(n < 0)`. Every diagnostic now uses the
  one renderer, which parenthesizes a negated comparison.
- `verify --strict` in a directory whose shared `soma.toml` has an `entry`
  that is another existing file no longer fails a sibling program with
  "[verify] cells names no state machine of this file": the gate belongs to
  the entry (the corpus directories hold one manifest for many programs). A
  manifest without an entry, or whose entry is the verified file, is refused
  as before. One regression test.
- `soma test`: an assertion comparing values of different types
  (`assert n != "5"`, `assert C.list() != "x"`) read as `false` — the report
  showed two identical-looking sides and `assert_fails` on it passed for the
  wrong reason. It is now the same error a handler raises, `cannot compare Int
  and String`, with both sides typed. One regression test.
- `identity(n)` is the alias of `eye(n)` its doc promised: it read a negative
  or Float size as 0 and had no cell limit (`identity(100000)` tried to build
  10^10 cells); `reshape` with a negative dimension said "cannot fill a 0x0
  matrix" instead of refusing the size. One regression test.
- A map pattern needs its keys: `{kind} -> …` matched a map WITHOUT a `kind`
  key (binding `kind = ()`), so the arm after it was unreachable and the body
  failed later with "cannot add String and Unit". A key the map lacks no
  longer matches; a key present with value `()` still does. `"a" + 1.5` is
  refused as `cannot add String and Float`, like `"a" + 1` (it said "expected
  Float, got String"). One regression test.
- The reference states what `for` iterates: a Map's `{key, value}` entries,
  a String's non-empty lines (a one-line string is one element), nothing for
  `()`; an Int is refused. It was undocumented.
- `soma check` refuses a generic `key` invariant typed for the wrong slots:
  `invariant key != ""` names no slot, so it guards every slot of its section,
  and on a List slot `key` is the index — every push failed at run time with
  "cannot compare Int and String" (the Int form beside a Map slot likewise).
  The message names the slots and says to give them their own `memory { }`
  section. One regression test.
- `soma describe` lists the HTTP routes the server actually matches: it
  found them by grepping `path ==` in `./app.cell` — the file of the current
  directory, whatever file was described — and missed every
  `match path { "/x" -> … }` arm. A prefix route is shown as `/items/*`. One
  regression test.
- `soma run app.cell` without a handler name no longer guesses among several
  public handlers: it ran the first zero-argument one (a `reset`), and
  `soma run app.cell 5` ran `close("5")` because it was declared before
  `echo(s)` — a transition, committed. Without a name the choice must be
  unambiguous: `main`/`run`, the cell's only public handler, or the only one
  taking these arguments (`soma run fact.cell 5` still reaches `compute(n)`);
  otherwise the command refuses and lists the candidates. One regression test.
- `use lib::x` inside a file of `lib/` resolves from the project root: it was
  looked up beside the importing file (`lib/lib/x`), so a library importing a
  sibling never loaded, and `soma check lib/scoring.cell` on its own failed.
  A `use` path is now tried beside the importing file, then at the entry
  file's directory, then at the nearest ancestor holding a `soma.toml`. One
  regression test.
- Docs: `is_a` / `is_type` recognize sum-type variants (by variant name and by
  type name); the gotchas said the opposite, and the builtin doc mentioned
  records only. The gotcha now shows the two names `is_a` accepts, that a
  struct variant's field reads with `.`, and `match` for branching.
- The guard prover reads a negated `require`: `require !(n < 0)` establishes
  `n >= 0`, `!(a || b)` establishes both negations, and a guard written as
  `!(n < 0)` is proven from `require n >= 0`. Two regression tests.

### `subscribe()` reconnects

- A `subscribe(url)` stream ended for good when the publisher closed or
  restarted: the reader thread logged the error and stopped, so the subscriber
  received nothing until its own restart. It now reconnects like a `[peers]`
  link (1 s, backing off to 30 s) and logs `subscribe: linked again`. The first
  connection is still the caller's: a publisher that is down when `subscribe`
  runs is a handler error. One regression test.
- `ws_connect` is documented as send-only (its replies are not read); the
  builtin doc claimed incoming messages dispatched as signals.

## 2.8.10 — 2026-09-21

### Native handlers: Int-only builtins and exact quotients

- `b / (c % 3)` in a `[native]` handler returns the exact Int the interpreter
  answers (63, not 63.0): `%` counts as an integer-valued operand when the
  handler's Float return is restored to an Int.
- Int-only builtins in Float-mode native code (`band`, `bor`, `bxor`, `bnot`,
  `shl`, `shr`, `bit_len`, `bit_test`/`bit_set`/`bit_clr`/`bit_next`, `idiv`,
  `gcd`, `sqrt_int`, `pow_mod`) refuse a Float like the interpreter does
  instead of truncating it (`bit_len(2.5)` gave 2.0, `pow_mod(2.5, 2, 7)` gave
  4.0) or failing with raw rustc output (`gcd(2.5, 5)`, `shl(1, 2.5)`). A Float
  literal, Float parameter or Float-valued builtin is refused by `soma check`;
  a local that may hold an exact Int / Int quotient is checked at run time.
- A BigInt `/` landing in an Int slot (a `pow_mod` exponent) emits the
  exact-or-error division instead of an "internal" refusal.
- One native regression test covers these paths.

### Parser: calling a parenthesized expression

- `(f)(2)` and `(x => x + 1)(2)` parsed as two statements: `return` handed back
  the lambda and `(2)` was a discarded value (`soma check` only warned about
  unreachable code). They are now refused like `f(a)(b)` with the same
  "bind it first" fix; grouping parentheses are unaffected. One regression test.

### Checker: quadratic-concatenation lint

- The "grows by concatenation inside a loop" warning fired on every numeric
  accumulator (`total = total + x`, `n = n + 1`, `acc = acc + 0.5`), and
  `soma verify --strict` rejected such programs. It now fires only when the
  variable or the added operand is known to be a String (a String literal or
  parameter, a String-returning builtin); unknown shapes stay silent. One
  regression test.

### Interpolation: numeric literals

- `"{0x1F}"`, `"{1e3}"` and `"{1_000}"` passed `soma check` and raised
  "undefined variable" at run time: the interpolation fast path looked up any
  alphanumeric segment as a variable. A segment starting with a digit now goes
  through the expression evaluator; `{42}` stays literal text as documented.
  One regression test.

### Native handlers: huge literal times a small local

- `9223372036854775807 * i` with `let i = 11` in a `[native]` handler answered
  a `range` error: the i64 fast path overflowed, and the BigInt fallback
  multiplied two plain i64 operands (a huge literal and a small local) and
  overflowed again. The fallback now promotes one side to an Integer; the
  handler answers the exact BigInt like the interpreter. One regression test.

### Cluster: restart warning

- A restarted cluster node warned that the tables of a removed cell were
  still in `.soma_data` (`_soma_cluster_v2_<Cell>.<slot>`): those are the
  runtime's own per-key versions and tombstones for a declared slot. They are
  no longer reported as orphaned data. The restart regression asserts it.

### Native buffers: negative size

- `buffer(-3)` / `buffer_f(-3)` in a `[native]` handler failed with Rust's
  "capacity overflow" (the size was cast to an unsigned length). They now raise
  a `range` error naming the argument. One regression test.

### `check --json` schema on load failures

- A parse error or an unreadable file answered a JSON record without the
  `notes` and `warning_count` keys that a normal `check --json` carries. The
  load-failure record now has the same keys. One regression test.

### Native handlers: `to_string` of a quotient, `str_at` bounds, error wording

- `to_string(n / 2)` in a `[native]` handler printed "2.0" where the interpreter
  prints the exact quotient as an Int ("2"). The exactness is now decided at run
  time; an inexact quotient still prints "3.5". The inexact-division flag is
  cleared at every native call instead of leaking into the next one.
- `str_at` out of range raised Rust's index panic text natively; it raises the
  interpreter's message with kind `index`. Native errors mapped back to the
  interpreter no longer carry a `kind: ` prefix in their text (`type: band(): …`
  read `band(): …` interpreted).
- One regression test.

### Storage and budget: Ints past 64 bits, reserved Map keys

- An Int past 64 bits as a List-slot index acted on element 0: `rows.delete(2^63)`
  removed the first row, `rows.set(2^63, v)` overwrote it and `rows.get(2^63)`
  read it, while `rows[2^63]` was refused. Such an index is out of bounds like
  any other index past the end; a local list's `.get` answers `()`.
- A Map whose key is `$serde_json::private::Number` (a spelling serde_json
  reserves under `arbitrary_precision`) read back from storage as an Int, or as
  a String holding the whole map. The key is escaped on disk and restored on
  read; keys starting with `__key__` are escaped the same way.
- `set_budget(-10^21)` (an Int past 64 bits) escaped the negative check and set
  an unlimited budget; it raises `range`. A nested `set_budget` no longer
  overflows when it adds a huge budget to the tokens used.
- Three regression tests.

### Pipelines: joins are linear

- `inner_join`, `left_join` and `join(left, right, key)` scanned the right list
  once per left row: joining two 20 000-row lists took 8 s. The right side is
  indexed once (first row per key wins, keys compared as text as before);
  100 000 rows join in under a second. One regression test.
- `m = with(m, k, v)` and `m = without(m, k)` on a local map copied the map at
  every step (20 000 removals took 26 s). Like the pipe form, they now update
  the map in place; earlier aliases keep their value. One regression test.
- `rows[i] = v` on a persistent List slot read the log three times and rewrote
  every row (23 ms per write on 20 000 rows). It now updates the one row by
  id; a failing `try` or handler restores that element alone. Backends without
  a specific implementation keep the rewriting path. One regression test.
- `rows.delete(i)` on a persistent List slot likewise rewrote every row (23 ms
  per delete on 20 000 rows). It deletes the one row by id and, on rollback,
  puts it back at its original position. One regression test.
- `m.len` on a persistent Map evaluated `substr(key, 1, 2)` on every row
  (0.5 ms per call on 20 000 entries); it counts the index and subtracts the
  reserved `__` keys by an index range (3 µs).

### Documentation

- `AGENT_GOTCHAS.md` #18 describes the current rule for an invariant between
  two slots (default both sides with `?? 0`; runtime-checked) instead of the
  removed "one invariant, one slot" error.
- `mock` with a List is documented as a queue of replies: a handler or
  `http_get` answering a JSON array is mocked with a nested list
  (`mock http_get [[1, 2]]`), in the reference and in gotcha #17.
- The `messenger`, `statuspage` and `pricing` examples declared
  `consistency: strong`, which the cluster runtime has refused since 2.8.2, so
  `soma verify` failed on them; they declare `eventual`, the implemented mode.

## 2.8.9 — 2026-09-20

### Storage reads, legacy lists and provider failures

- SQLite read failures are distinct from missing keys and empty collections.
  Unreadable rows and refused queries propagate errors instead of disappearing,
  returning fabricated defaults or panicking. Bare and indexed slot reads stop
  the invocation and roll back earlier local writes. An unreadable previous
  value cannot become a compensating delete in the undo journal.
- Appending to a legacy list preserves its keyed values in their existing sorted
  order. Replacing or deleting its last item cannot resurrect the old values.
  Memory and JSON backends replace legacy lists directly; the generic provider
  fallback detects a non-progressing removal instead of looping indefinitely.
- The JSON file backend refuses malformed files, invalid map/log shapes and
  unreadable paths. Mutations publish a uniquely named temporary file before
  changing the in-memory state; a refused write or rename preserves the old
  state. Keys and values have matching deterministic order.
- The HTTP storage adapter checks status codes, acknowledgements, typed values
  and collection shapes. Failures are reported instead of becoming success,
  absence, false or zero. Responses are bounded to 16 MiB and requests time out
  after 30 seconds. Typed non-finite Floats preserve NaN and both infinities;
  legacy integer replies outside signed 64-bit range retain their precision.
- `soma test-provider` fails when its CRUD sequence reports a storage error.
  Both provider demos preserve typed envelopes and isolate cell/slot names;
  their list removal and collection endpoints follow the adapter protocol.
- Add 21 isolated backend unit tests, 13 CLI storage regressions and two provider
  demo integration checks (including three Python HTTP scenarios). Thirty new
  storage regressions were reproduced on 2.8.8; both old demos also fail the new
  protocol checks.

**Compatibility:** a storage read failure aborts the invocation, including
inside `try`, because its value cannot safely drive further writes or rollback.
Write refusals still report kind `storage` when the transaction remains usable.
The legacy JSON backend supports per-file replacement, not SQLite multi-slot
transactions or cross-process isolation. HTTP providers have no transaction or
exactly-once protocol: a transport failure can leave a remote write's outcome
unknown. These adapter fixes do not add HTTP provider wiring to `run`/`serve`.
Cluster replication remains experimental and eventual.

## 2.8.8 — 2026-09-20

### SQLite failures, transaction boundaries and persistent lists

- A refused SQLite `BEGIN` stops execution. A failed `COMMIT` returns an error,
  rolls back SQLite and in-memory slot writes, and withholds queued events,
  replication and horde starts. Internal writes are checked before commit too.
- Task steps must commit before invoking external work and must acquire a new
  transaction before resuming Soma. A failed boundary aborts the invocation;
  `try` cannot continue execution after SQLite has ended the transaction.
  Already committed task steps retain their effects.
- Auxiliary counter and agent-memory tables open before the transaction,
  including in programs without declared persistent slots. A first-use write
  is transactional, and rollback cannot remove a cached auxiliary table.
- `remember()`, `next_id()` and state transitions propagate database write
  refusals. SQLite backend operations use savepoints, including when a SQL
  `FAIL` leaves partial statement effects. A database-initiated rollback cannot
  be followed by writes accidentally committed outside the handler transaction.
- List replacements stop on a refused delete or insert. A refused append inside
  `try` no longer removes the previously committed last item. Indexed reads
  handle gaps left by earlier undo operations or legacy data. Contiguous logs
  retain primary-key lookup; the layout cache tracks writes, external commits
  and transaction rollback.
- `next_id()` refuses exhaustion at 2^63 - 1 with `range`, and rejects negative
  or malformed counters with `storage`. Legacy migration only reads the
  calling cell's Map or state-machine backend.
- A normal `return` from a scheduled tick commits its successful writes.
- Concurrent processes retry SQLite initialization lock upgrades within the
  existing 120-second storage timeout; startup contention no longer produces
  an immediate refusal while enabling WAL or creating tables.
- Add 19 integration tests and 10 isolated unit tests. Sixteen integration
  regressions and five transaction unit regressions were reproduced on 2.8.7;
  remaining cases cover migration and cache/transaction boundaries.

**Compatibility:** database refusals previously reported as success now fail.
Recoverable write errors have kind `storage`; a lost transaction or failed task
boundary aborts the invocation instead of returning a catchable `try` result.
`next_id()` remains a per-cell signed 64-bit counter, and its range is finite.
These local transaction corrections do not add distributed consensus or make
external HTTP/file/model effects transactional.

## 2.8.7 — 2026-09-20

### Lexical scopes, interpolation analysis and agent memory

- `if` expression branches scope their `let` bindings, including when returning
  a closure or raising a caught error. Assignments to existing outer variables
  retain their effects. `soma check` rejects branch locals used in the other
  branch or after the expression.
- Lambda capture walks full interpolation expressions and `require` error
  details. Arithmetic operands, indexes and quoted delimiters no longer lose
  captured names. Collection lambdas get a fresh environment when interpolation
  executes assignments; optimized list reads/appends and map updates preserve
  their original arguments across those assignments.
- Runtime interpolation and static analyses share segment classification.
  Quoted colons no longer hide recursive calls, memory writes or `think()` from
  termination, invariant and token-cost checks. Such programs must now satisfy
  the same proof obligations as ordinary calls.
- `recall()` reads only the current cell's agent memory. Legacy migration reads
  only that cell's declared storage slots; a missing key cannot reveal another
  cell's value. Current agent-memory reads preserve stored types, including
  Strings that happen to contain JSON.
- `remember()` uses the storage payload validator before writing: functions
  (including nested variant payloads) and encodings deeper than 100 levels are
  rejected without overwriting the previous value.
- Add 24 integration regressions reproduced on 2.8.6 and a unit test for legacy
  memory isolation and migration, including a real process restart.

**Compatibility:** branch-local variables no longer escape `if` expressions.
Code that relied on `recall()` parsing a stored String must call `from_json()`
explicitly. Hidden interpolation effects can now make `check` or `verify --strict`
fail; fix the declared bound or the program rather than suppressing the check.

## 2.8.6 — 2026-09-20

### Evaluation order, call resolution and constructors

- Arithmetic assignments evaluate their right operand once. Non-exact division,
  remainder and mixed Float arithmetic no longer run a callback twice; logical
  assignments retain left-operand validation and short-circuit behavior.
- List self-appends keep the original list readable while evaluating arguments.
  Refused appends preserve the local binding. Arguments that reassign a local
  retain the ordinary call's left-to-right snapshot semantics. The legacy
  `xs = list(xs, item)` / `append` self-append idioms remain supported.
- In-place piped map updates evaluate and apply every pair. A later argument's
  failure is propagated, and record/list fallbacks do not reevaluate arguments.
  `with` rejects incomplete key-value pairs. Map keys represented by large Ints
  are accepted; machine-index bounds apply to lists.
- Optimized reads, appends, map updates and range loops respect handlers, local
  callables and scripted builtin mocks. `nth` does not retry an invalid index
  expression or read a replacement list created while evaluating that index.
- Calls and pipes share dispatch: pipes invoke local lambdas and the calling
  cell's own handler. Ordinary higher-order calls accept block lambdas.
  Record updates honor builtin mocks before validating the real update.
- Struct and tuple variant constructors recursively coerce declared fields,
  including Int-to-Float promotion and nested record maps, and reject Float
  overflow. A declared tuple/unit variant cannot masquerade as an untyped
  record through a named-field constructor.
- Add 24 integration regressions, all reproduced on 2.8.5, covering side-effect
  counts, value preservation, dispatch, mocks and constructor boundaries.

**Compatibility:** declared Float fields in newly constructed variants now hold
Floats, including ordinary Int rounding; overflowing promotion raises `type`.
Incomplete `with` pairs and incorrectly shaped variant constructors now fail.
Programs that relied on duplicate or skipped effects, ignored mocks or the wrong
handler being selected observe the corrected evaluation and call semantics.

## 2.8.5 — 2026-09-20

### Typed inputs and persistent values

- Implicit Int-to-Float conversions in parameters, record input/updates and
  Float slots reject integers beyond Float range instead of producing infinity.
  Nested parameter collections and JSON record inputs use the same check.
- Typed maps validate every key, including `_speed`, `__private` and `_type`,
  and inspect record/tuple-variant payloads. These checks apply to parameters,
  declared returns, record fields and memory writes.
- Declared sum/record return types validate the actual variant and its payload.
  Returning another type now fails at the face boundary and rolls back writes.
- Memory writes inspect variant payloads for functions and count the depth of
  the actual storage encoding, including record and escaped-map wrappers.
  Values that would become unreadable on reload are refused before commit.
- The local-list optimization of `nth` rejects indices beyond signed 64 bits,
  matching the ordinary builtin. In-range missing indices still return `()`.
- `sleep` requires one Int argument: fractional durations, strings, booleans,
  null and NaN no longer silently turn into a delay. Out-of-range Int durations
  retain error kind `range`; invalid argument types raise `type`.
- Date-count range checks handle the minimum signed integer without overflowing
  when Rust overflow checks are enabled.
- Add 18 integration regressions, each reproduced on 2.8.4, plus a date-count
  unit regression. Persistence tests restart the process to verify round trips
  and preservation of the previous value after a refused write.

**Compatibility:** programs relying on the invalid values above now receive
catchable errors. The storage depth limit is 100 encoded container levels;
record and escaped-map wrappers count toward it. Existing Float NaN/infinity
values and ordinary representable Int-to-Float rounding retain their semantics.

## 2.8.4 — 2026-09-20

### Numeric boundaries

- `avg`, `avg_by` and grouped averages divide the exact finite sum before
  converting to Float. `avg([1e308, 1e308])` is `1e308`, not infinity;
  cancelling huge Ints around a Float no longer produces a spurious NaN.
  Actual NaN and infinity inputs retain their nonfinite meaning.
- `pstdev`, `stddev` and `stdev` take the square root before converting
  the variance to Float. A finite deviation no longer becomes infinity or
  zero because its square is outside Float range. Exact midpoint checks
  handle subnormal values and ties to even.
- Mixed Int/Float medians compare the original values and preserve the
  selected Int. Even medians use the corrected mean. NaN consistently makes
  the median indeterminate, independent of input order.
- `clamp` rejects nonnumeric arguments and NaN bounds, compares Int/Float
  operands exactly and returns the selected operand intact. Rounding a large
  bound can no longer place the result outside the requested interval.
- Add nine regression tests that all failed before the fixes. CI also checks
  320 deterministic vectors (1,280 results) against independent Python
  Fraction/Decimal references: `SOMA=compiler/target/release/soma python3 tools/check_numeric_oracle.py`.
- Include the Linux cluster test port-reservation fix: retain the proxy's
  allocated listener and bound the node port pool below ephemeral ranges.

**Compatibility:** means can change their last bits because finite values are
averaged before rounding. A mixed median or clamp can now return an exact Int
instead of converting it to Float. `sum` and `product` keep their documented
Float arithmetic. The cluster protocol and eventual consistency are unchanged.

## 2.8.3 — 2026-09-20

### Robotics robustness campaign

- Add 100 independently named integration scenarios for numeric boundaries,
  invalid sensor data, mission transitions, storage invariants, rollback,
  collection and JSON boundaries, verifier refusals, native execution,
  persistence across processes and error record/replay.
- Preserve expected refusal diagnostics, check both successful and rejected
  operations, and compare native results with interpreted arithmetic.
- Publish the complete matrix, reproduction commands and coverage limits at
  `docs/robotics/ROBUSTNESS.md`, `/docs/robotics.md` and `soma docs robotics`.
  Synchronize the website and embedded agent guidance.
- This release adds regression coverage and documentation. The campaign found
  no additional runtime defect. It does not qualify physical robot hardware,
  hard real-time behavior or a lunar system; cluster consistency stays eventual.

## 2.8.2 — 2026-09-20

### Cluster correctness and recovery

- Preserve stored value types in replication, including BigInt, nested
  values, empty strings and null. Keep cell-qualified slot identities.
- Publish writes and deletes only after local commit; roll back replication
  metadata with failed handlers and `try` blocks. Apply incoming updates
  under the normal handler lock and transaction, with slot type checks.
- Replace duplicate, unmonitored cluster links with versioned full-mesh
  discovery, bounded queues and reconnect. Canonical identities prevent
  seed aliases from creating phantom members. Seeds require an acknowledgement.
- Resolve per-key conflicts by `(Lamport counter, node ID)`; ignore stale
  and duplicate updates. Persist deletion tombstones and versions with
  SQLite data. Periodic state exchange repairs missed writes and deletions
  after disconnect or restart. Local collection reads no longer fan out,
  duplicate entries, lose types or wait for replies that cannot arrive.
- Restrict cluster mutation to explicitly selected mutable Map slots.
  Legacy private events cannot write arbitrary memory on ordinary services.
  Require distinct data directories, reachable advertised IDs on public
  interfaces, and fixed ports that leave room for the bus.
- Re-evaluate advisory scheduler leadership on every tick; use monotonic
  heartbeat expiry. Consistent-hash utility now distributes adjacent virtual
  node names evenly; the runtime itself uses full replication.
- Remove false verifier claims of linearizability, CAP modes, automatic
  replication and guaranteed failure tolerance. Strong/causal consistency,
  replicated Lists, immutable slots and memory invariants are refused.
  `verify --strict` explicitly rejects unproved distributed behavior.
- Document and test the supported scope: eventual full Map replication,
  local reads, no consensus, no fenced scheduler and no exactly-once signals.

- Native compilation pins its output directory so an inherited
  `CARGO_TARGET_DIR` cannot make a successful build lose its shared library.
  Test launchers use Cargo's actual binary path for alternate target directories.

**Cluster upgrade:** protocol v2 requires upgrading all nodes together.
Retain each `.soma_data` directory. Do not mix old and new cluster binaries.
See `docs/site/cluster.md` for setup and limitations.

## 2.8.1 — 2026-09-20

### Continued language audit — 2026-09-20

- Integer `while` optimization validates the whole body before running.
  Read-only operands are retained, BigInt promotion never drops or repeats
  an assignment, and errors propagate. Other loop bodies use normal
  execution, including Float counters, division, return and break.
- Int division rounds the exact ratio once, including subnormal Floats.
  Huge finite quotients no longer become NaN through `inf / inf`.
  Integer means and pipeline averages share the corrected conversion.
- Variance and standard deviation retain the differences between nearby
  large numbers, including mixed Int/Float inputs. Finite equal large
  Floats have zero variance; their median no longer overflows.
- `index_of` uses structural equality without an approximate numeric
  fallback. `substring` clamps negative end indexes to zero.
- `chr` rejects invalid Unicode code points instead of wrapping modulo
  2^32. Math, integer-bit, random and byte-index builtins reject wrong
  operand types instead of silently coercing them to zero or truncating.
- Huge shift counts cannot become zero. Right shifts preserve sign fill;
  bit mutations enforce the Int size limit. Padding a BigInt value is
  permitted; the size check applies to the width argument.
- Native right shifts accept arbitrary-size nonnegative counts. Random
  intervals spanning the signed 64-bit range do not overflow. `bit_next`
  handles indexes 63 and above for small locals as well as BigInt operands.
- Generated native temporaries no longer shadow operands in shifts,
  bit operations, min/max and modular powers. Loop-bound counters have
  fresh names that avoid user variables, including nested loops.
- The deprecated `--jit` flag now really is ignored, matching its warning.
  It preserves normal arithmetic, structural equality, errors, qualified
  dispatch and recording. The old bytecode backend remains experimental;
  the CLI compatibility tests do not claim bytecode equivalence.

### Language audit — 2026-09-20

- Cost proofs cannot wrap large range lengths, loop annotations or sums
  into zero/negative costs. Saturated token/latency bounds are unprovable;
  USD estimates use a wide intermediate. A user-defined `range` is not
  assumed to have the builtin's iteration count, including horde inputs.
- Invalid or overflowing `mock now` / `mock now_ms` values fail the test
  instead of silently restoring the real clock. `days_in_month` rejects
  non-Int arguments; `min` / `max` reject nonnumeric scalar arguments.
- Mixed Int/Float comparisons preserve the integer exactly beyond 2^53 in
  the interpreter, VM and native backend. Sorting, numeric pipelines and
  vector/matrix masks use the same ordering; `filter_by` no longer treats
  NaN as equal to every number. Structural equality and deduplication preserve distinct big Ints.
- `distinct` and `distinct_by` compare nested values structurally, ignoring
  Map insertion order and keeping values containing NaN distinct.
- `range` rejects a zero step, counts the final partial stride against the
  allocation limit, and streams stepped loops. Loop bounds cannot overflow;
  a user-defined `range` is dispatched normally and arguments run once.
- `slice` rejects fractional indexes instead of truncating them.
- `top` / `bottom` reject negative or non-Int counts and non-List inputs;
  a positive BigInt count returns the whole list instead of an empty one.
- HTTP queries retain encoded plus signs and bare keys (`?flag` becomes
  `flag: ""`), for both explicit request handlers and automatic routes.
- `soma fix --json` includes syntax repairs in its list and count; text
  output no longer claims nothing was fixed after changing the file.
- Record/replay v2 separates raised errors from returned user data. A map
  containing `__error__` can no longer masquerade as a recorded exception.
  Legacy v1 logs remain readable with their original ambiguous convention.
- Native failures are recorded too, and the log captures the outcome after
  the transaction commits or rolls back. Replay refuses malformed v2 or
  unknown-version logs and refuses failed native compilation.
- `soma env` excludes internal Git checkouts from installed packages.
- The core CLI runner finds its fixtures relative to the repository,
  reports missing fixtures as failures, and exits nonzero on failed tests.
  Its division/vector expectations match current semantics. GitHub Actions
  now runs the Rust suite, the CLI fixtures and the published corpus on
  pushes and pull requests.
- The agent guide, embedded docs and generated website are synchronized
  with the release; build provenance is published.

- Check catches two silent foot-guns a port lost data to: a `refusal()`
  whose result is thrown away (a refusal does not roll the caller back, so
  the caller carried on as if the call had succeeded), and a
  `transition()` BEFORE a `think()` in a `[task]` or horde target (a crash
  replays that step and the edge is already taken: the task never
  finishes). Also: a String grown by concatenation in a loop (quadratic).
- The between-slot write-order warning is silent when a handler seeds both
  slots together.

- Interpolation: a `:` inside `{…}` no longer disables it — only a colon
  OUTSIDE quotes with no call or index is literal (CSS, `{n:>5}`).
  `"{split(t, \":\")[0]}"` and `"{\"http://x\"}"` used to print their own
  source, and check saw no undefined variable in them.
- `filter_by(rows, field, ">=", "2026-01-01")` compares Strings
  lexicographically (a date range answered `[]` silently).
- Cost: `if c { return think(…) }` followed by another think is ONE path,
  not two (a correct `cost { tokens: 100 }` was refused at 200).
- `invariant b.size <= a.size` between slots is accepted (nothing is read at
  a key); termination follows a local binding (`let m = n - 1  c(m)`) and
  halving (`c(idiv(n, 2))`).
- `distinct` uses the equality of `==` (1 and 1.0 were kept as two, NaN
  collapsed); `clamp(NaN, lo, hi)` stays NaN; docs: `%e`, empty `min`/`max`.

- Rules between slots (2.8.0) hold on `delete` too — dropping the other
  side's entry took it to () and the state stayed violated, while verify
  printed a ✓ claiming a delete cannot break such a rule; `other.size` is
  that slot's entry count (both names used to collapse to one, making the
  rule vacuous); `other.get(key)` is refused at check (it is the value
  BEFORE the write, so it guarded nothing).
- `soma verify` prints such a rule as a `·` note — enforced at run time by
  design — so `verify --strict` passes, and it no longer suggests a
  `require` that cannot evaluate; check warns about the write order the
  `?? 0` default imposes.
- The cross-cell notes follow same-cell helpers and emits (a helper used to
  hide the gap) and skip calls that change or read nothing.
- A file with no state machine beside a soma.toml that declares [verify]
  properties is a note, not a failure.
- `refusal()`'s documented shape: an HTTP response envelope (read `_body`).

## 2.8.0 — 2026-09-20

- Invariants BETWEEN slots of a cell: `invariant (reserved ?? 0) <= (stock
  ?? 0)` is checked on every write to either slot (the written one is its
  new value, the others are read at the same key); verify reports it
  runtime-checked. The bare form is refused with the fix in the message.
- `refusal(kind, detail?)`: the body and HTTP status a raised error of that
  kind would give — refuse a request AND keep what the handler recorded
  (raising rolls it back).
- `soma verify` names the cross-cell rules it does not prove (`note:
  cross-cell: handler X transitions its machine and acts on cell Y`).
- Check: an `ensure` in a `[task]` handler after writes and a think(); a
  face parameter whose name differs from the handler's.
- `delete` on an `[immutable]` List refuses whatever the index (out of
  range answered `false`); dropping a stalled SSE subscriber is logged, and
  the docs give its real memory cost.

- Storage: a write the database refuses (a read-only `.soma_data`, a full
  disk) raises kind `storage` and rolls the handler back — `set` / `push`
  were silent no-ops answered 200; `rows[i]` on a persistent List is an
  indexed read again (20 000 reads: 0.09 s, was 0.72 s; 100 000 took 17 s).
- Packages: `soma install` reinstalls a `path` dependency whose source
  changed (every build and test kept running against the first copy).
- `soma test <dir>` / `soma check <dir>` run every .cell under a directory.
- `soma serve`: SIGINT stops it (a shell that starts it in the background
  leaves SIGINT ignored); `-w` checks the new file before stopping the
  running one, and says when the server died; `-p 0` prints the port the OS
  gave; a damaged database is refused before the banner.
- CLI: a Map argument carrying `_type` / `_variant` / `_values` at any depth
  is refused, as over HTTP.
- The prover reaches further: `x % n` / `mod(x, n)`, `x * x`, `len("abc")`,
  `idiv`, `sqrt_int`, `band(x, mask)`, and the documented monotone counter
  `invariant value >= (seq.get(key) ?? 0)` written as `+ n` or `max(…)`.

- Soundness: `vote()` is in the call graph of every proof — a size
  invariant past a vote whose target wrote the slot was "proven" (and
  raised), recursion through vote was "proven" to terminate, its latency
  counted one voter instead of k; `horde()` targets and callbacks are in
  the termination graph (a horde restarted from on_done ran 20 821 times
  under "✓ terminates").
- The wait for a rate limit (`SOMA_LLM_RPM`, `[agent] rpm`) and a mock's
  latency are inside the think's `timeout` (a "proven" 2 s took 60 s).
- Bus: an event emitted while a `[peers]` peer's link is down is logged NOT
  delivered to it, even while other links are up (50 alerts lost, 3
  logged); docs: `emit` reaches every link, receivers filter by `[bus]
  accept`.
- `"{Other.ask(1)}"` is fine again; `range(a, b, step)` has a known length
  for cost bounds; docs: what `usd` counts.

- Budgets: `set_budget(0)` means no more model calls (it meant "no
  limit": a computed `quota - spent` reaching 0 lifted the cap); a negative
  budget is refused; `vote()`'s concurrent voters reserve against what
  `set_budget` left and their tokens and trace count for the caller (25
  voters spent 1 550 tokens under `set_budget(50)`, uncounted); a horde
  without `budget_tokens` counts every think of it (callbacks, votes, nested
  hordes) in `horde_status.tokens`; check warns on a computed `budget_tokens`.
- `http_get` / `http_post` never turn a remote JSON with `_type` /
  `_variant` into one of your records (a reflected `Admin` matched the Admin
  arm): such a body stays text.
- `"{len}"`, `"{helper}"`, `"{Cell}"` are check errors (a function or cell
  is not a value); `"{true}"` / `"{false}"` interpolate.
- `[task]` lints: a read in an `if` around the think, through a helper, or a
  write through a helper is stale; a value re-read after the think is not;
  `transition()` / `emit` before a think in a `try` count as writes; a
  literal `delegate` to a thinking handler is a think.
- Termination: `map("k", v)` in a lambda is not a function call, and a
  parameter typed as data is not a function value (`verify --strict` failed
  on CSV-row building).
- An Int given for a `Float` field or parameter becomes a Float.

- Soundness: a `[task]` tick (`every … [task]`) and `vote()` end a step —
  the prover keeps no fact across them (a "proven" `members.size <= 3` was
  broken at run time); the stale-read, try-write and no-think lints follow
  helpers, other cells' agents, emit listeners and `"{think(…)}"`, and
  apply to `[task]` ticks.
- Hordes: a horde whose task starts a horde of itself is flagged by verify
  and stopped (hordes started from tasks nest at most 4 deep; rounds started
  by on_done do not nest; at most 1 000 000 queued tasks per process); a
  server that lost a horde's lease stops recording its tasks (results after
  on_done); a horde's creator no longer resumes it a second time (double
  concurrency); `on_error` counts in the cost bound; `seed` covers apply and
  on_done; `instance` memory is per owner cell; status says done inside
  on_done; check: `seed` / `instance` / on_done parameter types.
- Records: `r.field += 1`, `r["field"]`, `xs[0].field = v`, `with(record, …)`;
  a field write must match its declared type.
- Strings: `"{\"a\": {\"b\": 1}}"` kept its last `}` — a `}` closing a
  literal `{` is never half of a `}}` escape.
- A handler another handler called can only lower the caller's `set_budget`.
- Docs: `SOMA_LLM_MOCK=rules:<file>` format; `keys(record)`.

## 2.7.0 — 2026-09-19

- Hordes (attack pass): nested hordes and hordes started from callbacks run
  under their parent's budget (a nested horde spent 6 000 tokens past a
  "proven" 100); check refuses a computed target or options (an HTTP client
  could pick a private handler or drop the budget) and literal options out of
  range, and warns on public callbacks; two servers on one `.soma_data` run
  each horde once (a lease, taken over when its server stops); hordes are
  persisted under serve / run even without a `[persistent]` slot; the queue
  tables cannot be reached from a slot named `_hordes`; only the owner cell
  reads or cancels a horde; the rate limiter is an exact 60 s sliding window.
- Hordes (quality pass, phase 5): a crash while a horde was cancelling
  resumes it as cancelled with counts that add up; a restart does not resume
  a horde whose handlers the new code renamed (it says why, state `paused`);
  under `soma test` a horde runs after the caller commits, like serve (a
  callback can cancel it); `on_error` receives `{error, kind, detail}`;
  tasks the budget stopped count as cancelled; `soma verify` prints each
  horde bound once and `--strict` fails on an unbounded one; each task
  starts with a fresh model context. Dashboard: live hordes at `/__soma/`.
  `SOMA_LLM_MOCK=rules:mocks.json` answers by prompt pattern. Corpus:
  `agents/horde_audit.cell`, `agents/horde_rounds.cell`.
- Hordes (phase 4): rounds for simulations — `snapshot` (every agent sees
  the same world), `apply` (results applied at the end in input order, once),
  `seed` (reproducible `random()` per task), `instance` (per-agent
  remember/recall and conversation across rounds); `vote(handler, input, k)`.
  10 000 agents × 20 rounds: 46 s, identical on every run.
- Hordes (phase 3): `budget_tokens` is a hard ceiling — each think() of a
  horde reserves an upper bound (request bytes + max_tokens) before calling
  the model, waits outside the lock for calls in flight when it does not fit
  yet, and is refused (kind `budget`, state `exhausted`) when it never will.
  `soma verify` prints each horde's cost bound; in a cell with `cost { }` a
  horde over inputs of unknown size needs a literal `budget_tokens`.
- Hordes (phase 2): `horde(Reviewer.review, docs, map("concurrency", 200,
  "on_result", "_store", "on_done", "_done"))` runs a `[task]` handler once
  per input with a bounded pool and returns an id at once; `horde_status`,
  `horde_results`, `horde_cancel`. The queue is persisted with the data and
  resumes after a restart; each result is recorded (and `on_result` called)
  with the task's last step, so once even after `kill -9`. Retries
  (`max_attempts`), `on_error`, `on_done`. Provider limits `[agent] rpm` /
  `tpm` (SOMA_LLM_RPM / SOMA_LLM_TPM) shared by every think(). 10 000 tasks
  with a 2 s mocked model at concurrency 500: 41 s. Check validates the
  target, callbacks and options.
- `[task]` handlers (hordes, phase 1): `on h(…) [task]` runs as steps —
  each `think()` commits the current step and waits for the model outside
  the handler lock, so concurrent requests overlap their model calls (200 ×
  2 s mocked calls in ~9 s). A failure rolls back the current step only; the
  prover carries no fact across a `think()`. Ticks take it too:
  `every 1min [task] { … }`, `after 5s [task] { … }`.
  `SOMA_LLM_MOCK_LATENCY_MS` gives the mock a latency.
- Check warns when a plain handler or tick (`on request` routing, a
  listener) calls a `[task]` handler — it would hold the lock; when a
  `[task]` reads a slot before a `think()` and writes it after; when a `try`
  writes and thinks; when a `[task]` has no `think()`. `[task, native]` and
  unknown handler annotations (`[tsak]`) are errors.
- Records: `r.field = v` on a record, `is_a(r, "Line")`, `keys(r)` /
  `values(r)`; a record prints `Line { sku: a, qty: 2 }`.
- Cost: a loop over a collection of unknown size counts once (a lower
  bound, the bound stays advisory) instead of an invented ×100 that could
  report a false "budget exceeded"; verify counts think() call sites.
- "undefined function" points at the call, not the handler header.

- Scheduler: a second `soma serve` takes over the every/after blocks when
  the owner stops; the lock is per program; each tick's token budget starts
  fresh.
- Records: a JSON object / Map given for a one-variant `cell type` becomes
  that record (errors name the field); variant fields read as `v.field`;
  deleting from an `[immutable]` slot is a check warning.
- Your handler named `subscribe`, `link`, `ws_connect` or `ws_send` wins
  over the network builtin at its arity; `cell test` helpers are held to
  the declaring cell's invariants and `[immutable]`.
- `soma run` checkpoints the WAL on exit; a damaged BigInt row is reported
  instead of reading as 0; `--fresh` resets data only after the check
  passes; handlers reached from `request` through a model tool are not
  endpoints; `ensure` after an early `return` is a check warning; a raising
  `forall` names its value; notes are not counted as warnings.
- Soundness: a `require` on a slot read no longer proves writes made after
  the slot was rewritten (here, through a helper or `delegate`); handlers
  reached from `request` through an `emit` at any depth are not endpoints.
- `[immutable]` slots are enforced: append-only Lists, add-only Maps.
- CSV "NaN" / "inf" stay text; `soma test` never touches `.soma_data`;
  `delegate` keeps the callee's error kind; `soma run` refuses a damaged
  database; `--fresh --record` starts a new log.
- Soundness: a negative List index is checked at its real index; a List
  delete re-checks shifted elements against `key` invariants (verify
  reports it runtime-checked); a self-recursive writer counts for size
  proofs; untyped / Any slots get no integer narrowing; file writes and
  `subscribe` make a latency bound advisory, `subscribe` has connect and
  handshake timeouts.
- `()` is refused for a List parameter; a slot-less invariant over several
  slots is a check warning; `write_file` / `write_csv` create the directory;
  `to_csv` keeps every column.
- Site: external effects are not rolled back (/agents); replay re-runs LLM
  and HTTP calls live (README).
## 2.6.1 — 2026-09-19

- Site and README: the stated guarantees now match the documented ones —
  distribution checks prove the declarations' coherence (the prototype
  replicates eventually), rollback covers slot writes and transitions (not
  external effects), `set_budget` stops the next call, default liveness
  means an exit stays reachable (`eventually` for every run), the paper's
  quorum is ⌊N/2⌋ + 1; the /agents payment example requires `amount > 0`;
  a homepage demo shows proven / runtime-checked / not covered.
- `soma run --fresh` resets exactly this program's tables (a cell `A`
  reset another program's `A_b`); an unreadable soma.toml fails closed;
  `soma run` without a handler prefers `main` / `run` and never runs a
  `_private` one; `soma build -o x.cell` is refused and `soma deploy`
  keeps existing files; `verify --json` is JSON when check fails.
- `soma fix` handles non-ASCII lines; `soma replay` keeps numeric-looking
  Strings as Strings and fails on unreadable or empty logs; the dashboard
  is same-origin only.
- New `asin`, `acos`, `pi()`; `tan`, `atan`, `atan2`, `asin`, `acos`, `pi`
  in `[native]`.
- Bus: a connection must send its first line within 10 s; at most 256
  are open at once (half-open connections held a thread each).
- `require <Int builtin>` is a check error like `if`; `soma run --fresh`
  resets only this program's tables when other programs share the database.
- New `to_csv(rows)`; `round` never returns -0.0; clearer errors for
  negative `round` digits and multi-variable `forall`; recursion limits
  documented (512 interpreted, 20 000 `[native]`).
- Peer bus: two processes listing each other exchange each event once
  (links open with `HELLO`); closed connections free their thread and
  socket; a `[peers]` address that is this process is refused; the
  reconnect back-off holds for links that drop at once.
- The start-up audit no longer reports write-once rows; a prompt that
  alone overruns the remaining `set_budget` raises before it is sent.
- `body: String` is the exact bytes received; `to_json` escapes `</` and
  `<!--`; the injected htmx script is pinned with SRI; `hmac_sha256` with
  an empty key fails closed; `--record` logs are owner-only.
- `[peers]` links are supervised: a peer down at start-up, restarted, or
  dropped for reading too slowly is reconnected.
- Packages: sub-directories are installed and covered by the lock's
  sha256; a file the lock does not list, or a case-variant `use`, cannot
  bypass the check; an installed package missing from the lock is refused.
- Latency bound: http without timeout counts 30 s; a think without a
  literal timeout makes it advisory. The JSON cap weighs objects and lists;
  `self_call` catches IPv4-mapped IPv6; CSV cannot carry `_type` /
  `_variant` columns; invariant source is not sent to clients; duplicate
  Origin headers are refused; imported `every` / `after` is warned.
- `split(s, "")` splits into characters; `parse_int(s, base)`; numbers
  beyond the Float range are refused by `from_json`, HTTP bodies and bus
  events; NaN sorts last in descending `sort_by`; `ipow` of 0 / ±1 to huge
  exponents; `sum_by` of a non-list raises.
## 2.6.0 — 2026-09-19

- soma.lock records a sha256 of each package's files: a package modified
  after install is refused at import, and `soma install` restores it.
- Check warns when an imported cell defines `request` or `ws` (it would
  own HTTP routing / the WebSocket port).
- `self_call` counts only ports this process listens on; a failed guard's
  source is not sent to clients; 204 / 304 carry no Content-Type; a quote
  inside `{…}` in a call argument gets the escaping hint.
- Performance: a lambda's captured lists and maps are no longer copied
  per element — `|> map` / `filter` / … over a captured list is linear
  (was quadratic).
- `[native]`: an Int `/` stays exact after the BigInt re-run (it truncated
  the docs' midpoint example); a handler returning its String parameter
  compiles; an Int overflow inside a Float/Bool expression is kind `range`.
- `sort_by` puts NaN last; an unclosed CSV quote is an error; `quantile`
  refuses q outside [0, 1]; blank CSV cells are missing values for
  `sum_by` / `avg_by`; operator chains count toward the nesting limit.
- `"s" |> map(f)` and `"s".map(f)` raise a type error; kinds
  `rate_limited` / `too_many_requests` answer 429.
- WebSocket: a returned `response(…)` sends its body; error bodies hide
  private handler names; binary frames are answered with an error.
- Clearer errors for reserved words used as JSON fields (`d["cell"]`) and
  for guard locals of a handler that may take a guarded edge.
- Soundness: `"C".delegate(…)` / `"C" |> delegate(…)` are seen by the
  termination and size proofs; a `let` hiding a slot no longer lends the
  slot's bound; the `latency` bound counts retries (think `timeout` now
  covers them), tool rounds, `sleep`, and is advisory with approve / file
  I/O; an interpolated transition target is treated as computed.
- `[native]`: an exact Int / Int with operands past 2^53 is exact; stdin
  read before a BigInt re-run is replayed.
- New `ipow` (exact Int power) and `from_csv(text)`; `read_csv` takes
  `delimiter` and refuses unknown options; `map`/`filter`/… on a non-list
  raise a type error; `format("%.2f", Int)` is exact; `to_float` past the
  Float range raises `range`; `pow` of a non-number raises.
- In an invariant, `slot.get(key)` reads the stored value on Map- and
  record-valued slots too: write-once invariants on `Map<String, Map>` /
  `List<Map>` slots were not enforced.
- approve() shows control characters as escapes (a model could redraw the
  prompt); every reply shape is measured against max_tokens and the budget,
  a reply with no text raises kind `llm`; a `set_budget` reached from a tool
  call can only lower the caller's budget; `..;/` is refused by capability
  path scopes.
- A List `delete` is checked by size invariants only (as verify says);
  `require … else budget` answers 400.
- A JSON request body or bus event with more than 1 000 000 values is
  refused before parsing (a 15 MB line became 2.4 GB in memory).
- Events sent after a linked peer disconnected are logged NOT delivered;
  `soma run` warns when an `emit` meant for `[peers]` goes nowhere.
- Check warns on `m.size ?? default` (`.size` is the entry count, never
  `()`); a negative `[native]` buffer index is reported as written.
- Every path-taking builtin (`load`, `include`, `load_template`,
  `read_files`, `par_read_files`, `word_count`) refuses a `..` segment:
  `load("templates/" + name)` could serve any file.
- The bus port opens only for `[peers]`, `[bus] accept`, `scale` or
  `--join`: an in-process `emit` no longer exposes the program's listeners.
- Kinds `unauthorized` / `unauthenticated` answer 401; error bodies no
  longer name private handlers; the loopback Host check parses the whole
  authority and refuses duplicate Host headers.
- `SOMA_LLM_MOCK=fixed:` over max_tokens raises kind `llm` like a scripted
  mock; guard locals bound after a transition to another state are accepted.
- The file write guard is case-insensitive; `self_call` recognises every
  spelling of this machine; `Origin: null` / `file://` writes are refused
  on loopback servers.
- LLM replies are measured as well as counted: an under-reported reply
  cannot pass max_tokens or the proven cost bound.
- File builtins refuse `..` segments and writes over the program, its
  configuration or storage; a loopback server refuses foreign Host headers
  and cross-origin writes; an HTTP call to the server itself answers
  `self_call`; tool capabilities match query spaces encoded.
- get_status & co. from a machine-less cell with several machines, and
  variant literals with wrong fields, are check errors; the secret recipe
  fails closed; security notes in the serving docs.
- `pow_mod` work is bounded (bits(exp) × bits(m) ≤ 2^30); `to_int` /
  `parse_int` refuse text past the 2^24-bit Int cap.
- A bus event that reaches no peer is logged as NOT delivered; the docs
  give an outbox + acknowledgement pattern for transfers between processes.
- An Int holds at most 2^24 bits: products, `shl` and `product()` past it
  raise kind `range` before they are built (interpreter and native);
  `matmul` is capped at 10^9 multiply-adds.
- `substring` and `range` refuse wrongly typed arguments (kind `type`).
- A huge declared Content-Length no longer aborts `soma serve` (tiny_http
  vendored with a chunked drain; bodies past 256 MB are refused 413).
- A scripted `mock think` reply longer than max_tokens raises kind `llm`.
- Bus peers have bounded send queues (a peer that stops reading is
  disconnected); WebSocket clients are also dropped past 64 MB queued; the
  WebSocket Origin check refuses userinfo and control characters.
- A split string as the last statement of a loop body is a check error;
  disjunctive invariants over the written value are proven.
- Duplicate or list-valued Content-Length is refused and the connection's
  pipelined bytes never run; request-log lines escape control characters.
- Unary operator chains count toward the nesting limit; a List slot's
  invariant `key` is the Int index; `html()` with header pairs passes the
  arity check; guard variables bound in a loop body are recognised.
- `emit` arity mismatches are check errors; a computed Float is refused by
  an Int parameter; CSV rows of empty cells survive a round trip and
  duplicate CSV headers are refused.
- Parameter types are checked all the way down (HTTP, bus, tools, calls);
  a Float past 2^53 is not coerced into an Int parameter; `[native]`
  handlers and mocks are held to the face's return type.
- `from_json` refuses undeclared `_type`s; misspelled builtin types and
  extra builtin arguments are check errors; `()` is refused where a nested
  scalar is declared; `write_csv` keeps rows holding an empty String.
- The equal-clause proof tolerates callees that cannot write the slot;
  contextual keywords work as statement variables.
- A handler parameter may not take a slot's name; face parameter types must
  match the handler's.
- verify and the guard rule see every / after ticks of machine-less cells;
  instance ids are Strings or Ints (`()` refused); a renamed machine is
  reported under `soma run` too.
- `soma run` treats any non-data token as a handler name when several
  exist; NaN / lambdas in responses are valid JSON.
- `[native]` `bnot` is exact past 2^63; native `loop_bound` errors keep
  their kind; `soma run … request` decodes the path like serve.
- Guards of transitions taken from machine-less cells are checked;
  `--strict` fails when `[verify] cells` names no cell of the file; `for`
  over any finite value terminates; an `emit` in an imported file opens the
  bus.
- The equal-clause proof applies only to pure clauses (locals, arithmetic,
  `??`, `.get` of the written slot) with a plain key — other slots,
  nondeterministic or external reads between the require and the write
  made it prove refused writes.
- Invariant slot references are counted deep (interpolation, lambdas,
  match arms) by the checker and the runtime; verify hints never name
  size / key / value; undefined functions in properties are check errors.
- `mock Cell.handler` stubs that cell's handler only; mocks naming nothing,
  test helpers shadowing program handlers or builtins, and test cells with no
  assertion are errors; mocks reach `[native]` sibling calls.
- `--record` logs calls that raised; `describe` / the dashboard keep
  `except` on `*` edges; `soma fix` exits 1 when errors remain.
- A `require` that is exactly an invariant clause over the written value
  proves it (monotone versions); `value` / `key` / `size` outside an
  invariant are check errors; slots in nested lambdas are not function values.
- `soma serve` refuses to start on a damaged database (`quick_check`); a
  file that is not a database is a clean error, not a panic.
- A materialized `range()` is capped at 10M elements; dotfiles under
  static/ are not served; Content-Length + Transfer-Encoding is a 400.
- Quant builtins enforce `max_obs` / `max_assets` and a confidence `alpha` in
  (0, 1); no false "share the path" warning for emit listeners.
- The invariant prover is exact for Ints past 2^53 (intervals widen
  outward instead of rounding), counts think() tools and computed delegates
  among a handler's callees, and does not prove a bare slot read that may be
  `()`.
- `http_get` in a loop, lambda or tool no longer makes a token bound
  advisory (only a latency bound); a saturated cost peak is advisory.
- `[native]` `i64::MIN / -1` raises instead of returning a rounded Float;
  guarded transitions in lambdas see the handler's locals; refinement paths
  keep their parentheses.
- Statements inside block lambdas, `try { }`, if-expressions and match arms
  (`slot[k] = v`, `emit`) are seen by GET→405, the listener set, the
  termination graph, the size prover and guard / invariant purity.
- `return` inside a block lambda is a check error; a bare `return` returns
  `()`; `soma run … request GET "/a?x=1"` splits the query; `every 90m`
  names the duration units.
- `x |> h` with a bare handler name is a call for every analysis
  (termination, cost, route ownership, GET→405, model-driven recursion).
- `subscribe()` dispatches only events the program emits or `[bus] accept`
  lists; invariants and guards may not read files, print or read stdin.
- `.field` on a String or a List raises kind `type`; serve lists only real
  endpoints; `[native]` literal arithmetic that overflows i64 promotes to
  BigInt as interpreted code does.
- Capability-scoped tools cannot load files (`load`, `include`,
  `par_read_files`, …) or `link()`; invariants are pure conditions (no
  handler call or effect builtin, however hidden); a tool calling back its
  think() handler is a termination ⚠; `link()` makes a handler POST-only;
  `load()` substitutes in one pass; a reply over its `max_tokens` raises.
- Errors in imported files are reported in that file.
- `??` short-circuits; `with()` stores BigInt values; strict Float bounds
  after a `require` are proven; `tools_allowed` must name tools.
- Interpolation segments are analysed exactly where the runtime finds them:
  `"{ { expr } }"` ran `expr` unseen by route ownership, termination, cost,
  GET→405, guard and invariant checks.
- A returned String is sent raw only when it is valid JSON text; JSON
  answers carry `nosniff`. `[native]` string literals unescape `{{`/`}}`.
  Face return types are checked element-wise.
- An interpolation segment holding a block (`"{if c { 1 } else { 2 }}"`)
  ends at its matching brace; `.size` on a record without that field is `()`;
  `from_json` checks a declared variant's shape (a missing or mistyped field
  raises kind `type`).
- A stack overflow is not caught by `try`; lambdas handing function values to
  map/filter are a termination ⚠; a computed delegate in `request` owns the
  cell's handlers; face tools are not HTTP endpoints.
- Storage escapes map keys starting with `__` (no refusal, no forgery);
  `()` is false in every condition; invariants must be Bool.
- Floats print shortest digits; `format("%e")`; 5 redirect hops;
  `f(a)(b)` refused; unknown handler names in `soma run` are errors.
- Storage refuses map keys and slot keys starting with `__` (reserved by
  the encoding: a client body could come back as a forged variant, and such
  keys escaped len and size invariants).
- A block lambda's last statement runs when it is not an expression (an
  `emit` there was dropped); conditions and predicates take Bools; route
  ownership follows calls through other cells; `run --fresh` refuses to wipe
  a directory a `soma serve` is using; verify names machines per cell.
- A lambda call counts toward the recursion guard (self-application aborted
  the process); calling a function value is a termination ⚠.
- Cost sees think() in index/field assignments; the guard rule and route
  ownership see UFCS, interpolation, require and emit chains.
- A bare slot name never reaches another cell's storage (it bypassed the
  owner's invariant); writes to slots with non-size invariants no longer
  count the slot (bulk loads were quadratic).
- `mock think` token accounting is capped at max_tokens.

## 2.5.1 — 2026-09-18

A hardening release: seventeen more fresh-agent cycles (realistic ports —
billing, support agents, reservations, analytics, safety interlocks, kanban,
eMAR, a game economy, a WMS — each paired with an adversarial round). Every
finding is fixed or listed as open in docs/agent-ux/LEDGER.md. Highlights:
false proofs closed (require/guards/while conditions in cost and termination,
NaN on Float slots, deletes, shadowed builtins), security fixes in serve
(route ownership, CSRF through cross-cell calls, SSE/WebSocket isolation and
injection, slow-client and flood DoS, capability SSRF, private slots, record
forgery), crypto builtins for authentication, and exact BigInt arithmetic
across builtins.

- Cost: think() in a while condition counts per iteration; the last
  `max_rounds` wins. Route ownership sees UFCS, interpolation and delegate
  calls (auth bypass).
- Storage: an Any slot gives back the String it stored (no JSON re-parse).
- `else { if … }` is a value; exhaustiveness ignores refutable sub-patterns;
  BigInt-exact clamp / pow_mod, 64-bit limits raise; native len counts
  characters; invariants are per cell; qualified-call arity is checked.
- verify models transitions from a machine-less cell (single-machine
  programs); the composition lint flags only callees that swallow failures.
- serve: `emit` is not pushed to WebSocket clients (only `publish` is; SSE
  clients get an emit only when they name it); each WebSocket client has its
  own queue (a slow client is dropped instead of starving the others);
  websocket, tick and bus threads have the 64 MB handler stack (deep
  recursion there aborted the process).
- `try { … }?` re-raises; a guarded match arm does not cover its variant;
  `"{slot}"` works; `{…}` must be one expression.
- `mod` / `idiv` / `floor_div` / `div_round` take Ints; `sum_by` / `avg_by`
  refuse non-numbers; `distinct` keeps values of different kinds.
- serve: a GET that calls into another cell's handler (bare, UFCS or pipe)
  is 405 like a qualified call.
- Crypto builtins take Strings only (`secure_eq("null", ())` was true);
  `random_token` is nondeterministic for replay.
- Check: a test rule calling transition() in a multi-machine program; foreign
  slots through `Cell["slot"]` and interpolation.
- Security: a cell's slots are private (reading another cell's slot by bare
  name is a check error); another cell's handler never replaces a builtin
  (a library's `escape_html` disabled escaping); `soma install` refuses
  dependency names with `/` or `..`.
- New builtins: `sha256`, `hmac_sha256`, `random_token`, `secure_eq`.
- `use` imports each file once (cycles and diamonds load); `soma run` takes
  everything after the handler as arguments; `--json` is always JSON.
- `response()` honours an explicit Content-Type; `html()` takes headers;
  properties see the rules' `let` fixtures.
- Check: a handler cannot be named after a safety builtin (`transition`,
  `approve`, `fail`, `think`…) — it replaced the builtin program-wide while
  verify still proved the edges; one `initial:` per machine.
- verify: a reactive machine (no terminal state, every state returns to the
  initial one) passes --strict.
- serve: one scheduler per data directory (a second serve doubled ticks).
- Capitalised field names in assignments and record literals.
- A panic inside a builtin is a catchable error (kind `internal`); matrix
  builtins cap each dimension (a zero dimension bypassed the size cap).
- Native: i64-to-BigInt local assignment compiles; mixed Float/String
  returns are a check error. Regexes are compiled once per pattern;
  `read_files` is in name order; `format("%d", inf)` raises.
- serve: WebSocket and SSE pushes carry the data as one-line JSON (a String
  payload could forge envelope fields and SSE events for other clients).
- `range` near i64::MAX ends (the step wrapped); `format("%.Nf")` past 1000
  decimals is a `range` error, not a crash; `round`/`floor`/`ceil` of 2^63.
- `soma test`: a test cell's own helpers win its bare calls.
- Security: `&&`/`||`, `ensure`, match guards and properties take Bools (a
  list mask or a String passed compound invariants, guards and asserts);
  tool capabilities match host and path separately and refuse `..`,
  userinfo and fragments; a tool's scope holds inside the agents it calls.
- Agents: `map("tools_allowed", [...])` restricts the tools one think()
  offers; a timeout is not retried (it billed up to 4 × max_tokens past the
  proven bound); a literal transition in a tool keeps think-isolation;
  `recall` works across processes; unknown think() options are check errors.
- Replay records only top-level calls and uses soma.toml [agent].
- Check: a function used as a value, `()` as a slot key, Bool arithmetic and
  Rust keywords in `[native]` code.
- serve: an SSE client receives only the streams it subscribed to (every
  client received every publish); a connection flood that broke the HTTP
  worker pool exits the process (status 70) instead of leaving it alive and
  deaf; a literal `delegate` to a missing handler is a check error.
- Prover soundness: calls in `require` conditions and details are analysed
  (termination, cost, invariants); transition guards must be pure; a delete
  voids a key-exists size proof; Float-slot writes that may be NaN are
  runtime-checked.
- serve: a handler `request` calls is reachable only through `request`
  (auth checks written as `if`/`starts_with` were bypassable); one builtin
  call builds at most 10^8 elements (a single request aborted the process);
  `sleep` bounded; bus lines capped at 16 MB; cross-process `emit` sent at
  commit; `[bus] accept` filters outbound links too.
- Data: `next_id()` ids stay unique across a failing `try`; sum-typed
  parameters and generic variant fields are checked; `write_csv` quotes
  Lists, headers and numeric Strings; exact Int vectors, matrices and
  `median`; date builtins bounded; `for` over `()` runs zero times.
- Check: reading another cell's slot, `every 0ms`, and a native local
  reassigned to another type are errors with a fix.
- A bare call inside a cell to a handler name another cell also defines runs
  the calling cell's own handler (it ran the other cell's).
- `soma serve` no longer exposes the start-up hook (`init` / `start`) as an
  HTTP endpoint; static text files get real content types.
- `soma verify` always ends with a verdict line, prints check errors on
  stdout, and says which `require` would prove an open invariant.
- Native: a buffer passed to a sibling is a check error; Int-valued calls in
  Float expressions compile; a constant overflow is the `range` error.
- `soma verify` fails when `[verify] cells` names a misspelled cell or one
  without a state machine (every property was silently skipped).
- serve refuses a request body carrying `_status` (a handler echoing it let
  the client pick the status and headers), answers 500 for a status outside
  100–599, 400 for a non-UTF-8 body, and keeps `%ZZ` literal.
- The prover narrows by `if` branches and accepts update loops over a
  slot's keys; `cost { tokens }` is stated as reply tokens.
- Data safety: persistent List slots enforce `size` invariants on push;
  state machines persist without a persistent slot; match arms are scopes
  (a failed guard deleted the outer variable).
- Prover soundness: NaN, shadowing by match/lambda/nested lets, growth and
  cost through other cells and emits, termination with re-bound parameters.
- serve: GET/HEAD to a state-changing handler is 405; `_type`/`_variant`
  in client JSON is refused; neither `start` nor `init` is an endpoint.
- `soma run` refuses a program that fails check; native check runs the code
  generator; native shifts, sqrt_int, sb_push_char fixed.
- serve: only `response()`/`html()`/`redirect()` maps are HTTP responses (a
  client map with `_status` stored or echoed forged status, headers and XSS);
  CR/LF header values dropped; the bus port refuses HTTP and private events.
- Prover: a `require` proves only the writes after it; reassignments in match
  arms, `try` and if-expressions; `every`/`after` writers; termination through
  pipes, qualified self-calls and tick loops.
- Hints for `require` without `else`, `and`/`or`/`not`, a quote inside `{…}`;
  CI builds Linux and Intel macOS binaries for every release tag.

## 2.5.0 — 2026-09-18

The agent-experience release. Nine cycles of fresh AI agents (none had seen
Soma) built services, ported Python/Java/TypeScript/Go/Ruby programs, ran
data jobs and LLM pipelines, and attacked the prover, the runtime and the
parser — learning only from the website. Every finding below was reproduced,
fixed, and pinned by a regression test; the whole repository (1,405 `.cell`
files) was re-run through check / verify / test after every batch.

### Soundness and atomicity

- A handler with persistent slots is **one SQLite transaction**: a process
  killed mid-handler (`kill -9`) leaves nothing of it on disk (writes used to
  commit one statement at a time). `every` / `after` ticks are rolled back
  when they raise, like handlers.
- Slots give back exactly what they stored: an Int beyond 64 bits stayed a
  String; records keep their field order; `.keys` / `.values` are sorted.
- Slot value types are enforced on every write (`Map<String, Int>` refuses a
  String, `1.5` or `1.0`; `Map<String, Pay>` refuses a plain map).
- List slots: `rows[i] = v`, `rows[i].f = v`, `rows.delete(i)` used to be
  silently dropped.
- The prover no longer issues false ✓: a `require` in a loop that may run
  zero times, bindings that shadow a narrowed name, interval overflow past
  2^53, termination without a lower-bound base case, cost bounds that skipped
  `every` blocks and `delegate`, liveness that assumed guards pass (now said).
- The prover proves more: a `require` counts for the writes of its own block,
  early exits (`if n >= 1 { return … }`), `require a + b <= K` on the written
  expression, one slot's invariant chained through a local, `size` /
  `len(slot)` invariants, deletes against value invariants. Every ⚠ says why.
- `request` is never an HTTP endpoint (a GET could run a POST route);
  path segments are percent-decoded; CORS on every response.
- A bare call to a name two cells define is a check error (it resolved at
  random); `*` edges no longer fire from states a program stopped declaring.

### Language and builtins

- `format(fmt, …)` (printf subset), `div_round` (HALF_UP), `floor_div`,
  `mod`, `divmod`, `to_fixed`, exact `round(x, d)`; dates: `parse_date`,
  `add_days`, `add_months`, `days_between`, `months_between`,
  `days_in_month`; `chr`, `ord`; `stdev`/`variance` are sample statistics
  (`pstdev`/`pvariance` population); `sin cos tan atan atan2`, `regex_*`,
  `read_stdin`, `write_str` in interpreted handlers too.
- HTTP client: `http_get/post/put/patch/delete` with a default 30 s timeout,
  `headers`, and `{error, kind, status, body}` on failure (the upstream body is
  kept); mockable in tests.
- `think_json` raises kind `json` on a non-object reply; mocked `think`
  costs tokens (budgets testable offline); `trace()` survives requests under
  `soma serve` and records the system prompt.
- Variants round-trip through `to_json` / `from_json`; `soma run` and
  `soma serve` answer valid JSON (NaN/inf → null).
- Literals `1_000_000`, `0xFF`, `0b101`, `"\u{1F600}"`; negative range
  patterns; bare state names; keyword field names (`j.state = …`).
- `|> map`, `|> filter` and `xs[i]` are linear (a 20k-row job went from
  104 s to 0.1 s).

### Toolchain

- `soma check` catches what used to fail at run time: native-only
  primitives outside `[native]`, what the native codegen refuses (buffer
  re-binding, list returns, stepped ranges, literal `/ 0`), `break` outside
  a loop, `transition()` arity, values thrown away (`let j = 1 2`), rules
  outside a test cell, empty test cells, writes to a loop copy (warning),
  `emit` with no listener (warning). 10 000 nested blocks no longer crash it.
- `soma verify --strict` repeats the ⚠ lines by the verdict and always ends
  with one verdict line; orphan `[verify]` properties fail.
- `soma test --json` records carry rule, message, left/right, raised;
  `assert_fails … matching` matches the kind too; `mock` works for any
  handler, `Cell.handler`, `now`, and builtins such as `http_post`.
- `soma serve`: `--no-schedule`, an `llm:` start-up line, endpoints listed,
  whitespace-padded JSON bodies accepted, stored-data audit at start-up
  (undeclared states, invariant violations, re-typed values, renamed slots)
  — also under `soma run`.
- `soma run --fresh`; `.soma_data/` lives beside the program; exact big-Int
  CLI arguments. `soma fix` repairs `;`, `=>` arms, `-> T` on handlers,
  `null`/`True`. `soma describe --json` lists sum types.

### Site and docs

- New `docs/operations.md` (failure modes, statuses, exit codes, migration
  guide, environment variables); serving/guarantees/reference corrected
  against the binary; builtins regenerated from the compiler; the landing
  page links status, guarantees and serving, and states what verify proves.
- The `soma init` starter and the corpus exemplars pass `--strict`.

## 2.4.0 — 2026-09-18

An audit release: the verifier and the runtime were attacked with adversarial
programs, a differential interpreter/native harness and a fuzzer; everything
found is fixed here, each fix with a regression test (274 Rust tests, and
all 1,083 `.cell` programs of the repository re-run with no regression).

### Language

- **`7 / 2` is `3.5` everywhere.** `[native]` handlers used to truncate
  (`3`). Native `/` on two Ints is now a Float; an exact quotient is still an
  Int (BigInt-exact); a slot that can only hold an Int refuses a non-exact
  quotient with a runtime error instead of truncating. `idiv(a, b)` is the
  integer quotient on every backend (now supported in `[native]`, and
  BigInt-exact in the interpreter, where it used to turn any value beyond
  i64 into 0). `soma fix f.cell --native-idiv` migrates old code.
- `soma check` now rejects: a string literal dangling after `return`
  (`return "a" "b"` silently returned `"a"`), a memory invariant naming
  several slots (every write would have been rejected at runtime), a
  non-exhaustive `match` inside a lambda. It now warns on: a call that
  resolves to a builtin instead of the homonymous handler, unreachable code,
  Int / Int in a `[native]` handler.

### Soundness

- Memory invariants: `delete` bypassed `size` invariants; `soma verify` did
  not see bracket writes (`slot[k] = v`) or deletes.
- Termination proof: mutual recursion, recursion hidden in an operand
  (`1 + f(n + 1)`) and decreasing recursion without a base case were all
  reported as "structurally terminate".
- Cost proof: "`tokens` bound proven" was claimed for a `think()` reached
  through a sibling handler, a lambda, or a loop over a list of unknown
  size. Calls are now composed; unknown counts make the bound advisory.

### Runtime

- **Security:** `soma serve` served `/static/../soma.toml` (API keys), the
  `.cell` sources and `.soma_data`. Static files are confined to `static/`.
- `[native]`: a division by zero aborted the whole process (SIGABRT); a
  panic in native code is now an ordinary, `try`-catchable error. Concurrent
  native builds in one directory poisoned the dylib cache (wrong results,
  silently): builds take a lock, publish atomically, and every dylib carries
  a build id verified at load.
- `soma test` ignored `[agent]` / `[models]` in `soma.toml` (so `mock` had no
  effect); `soma replay` blamed nondeterminism when the source had changed;
  `soma run f.cell -7` rejected negative arguments.

### For agents

- `soma init` creates `app.cell` (the name every doc uses — it used to write
  `main.cell`), a starter that passes check/verify/test, and an `AGENTS.md`.
  The stdlib is embedded in the binary: a fresh project no longer warns
  `unknown property 'persistent'`.
- `soma docs agent|reference|gotchas|builtins|all` — embedded, offline.
- `soma example <terms…>` searches the verified corpus; `soma example <id>`
  prints a program's source.
- soma-lang.dev is generated by `tools/build_site.py`: `llms-full.txt`,
  `builtins.json`, `gotchas.json`, `corpus/index.json` (316 programs, each
  re-verified at build time), `agent.md`, real 404s, open CORS.

### Examples

- `rebalancer`: `POST /approve` reached the `approve()` builtin, not the
  handler — the human approval gate was unreachable through `request`.
  `examples/atlas`: same shadowing in `verdict`. `examples/padovan.cell`
  checked against wrong expected values.

MIT license added.
