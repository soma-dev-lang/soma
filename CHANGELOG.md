# Changelog

## Unreleased

### Skeleton first: `skeleton` / `flesh`, frames, holes, `soma skeleton`, `soma holes`, `skeleton.lock`

Goal of the release line: let programming agents design large applications in a solid manner. Design and measurements: `SKELETON_DESIGN.md`.

- **`skeleton Name { … }`** (and `skeleton agent`): a cell without handler bodies — memory, invariants, state machines, and a face whose signals each carry a **frame**: `reads a, b`, `writes a`, `moves s -> t` (`moves Cell.machine: s -> t`, `moves * -> t`, `moves * -> *`), `calls f, Cell.g, think`, `emits s`. The frame is closed: a signal without a clause is a pure function of its parameters. `skeleton`, `flesh` and the five clause words are contextual — every existing program parses as before (none of the repository's 1,059 valid programs changes verdict).
- **`flesh Name { on … }`**: the bodies, and nothing else, in the same file or in one a `use` brings in. A parameter takes its type from the signal (`on pay(id)`); a handler the face does not declare is a private helper, named `_helper` and typed. A flesh reads its skeleton's `const`s. After parsing, a skeleton and its flesh are an ordinary cell: the interpreter, `[native]`, `serve` and the verifier are unchanged.
- **Conformance (`soma check`, kind `frame`, E0120).** Everything a body does — and the helpers it reaches do — must be inside its signal's frame: a write, a read, a call (of a signal or of a builtin with an effect), an emit, a machine edge (the edge itself when the body names or established the source, the target otherwise). Effects written inside string interpolations count. Each error names the clause to add. The frame itself must make sense on the skeleton alone (slots, edges and signals that exist; another cell's slot is refused with the way through its signals). A schedule in a skeleton only calls signals. Warnings (W, `frame_warning`): a frame wider than its body, an edge no signal moves, a slot no signal writes, a helper nothing calls.
- **Holes.** A signal no flesh fills: `check` passes and counts them, `verify` proves the machines and ends `VERIFY OK (skeleton — the holes above are open obligations)`, an edge only a hole will take "waits for a hole" instead of being called stale, a call fails with kind `not_implemented`. `soma holes app.cell [--json]` lists them in the order to write them (callees first).
- **`soma skeleton app.cell`**: any program as its skeleton — on plain cells the frames are inferred from the bodies. `soma skeleton app.cell Cell.signal` is the brief for one signal: its slots with their invariants, its edges with their guards, and the signature, frame and contract of everything it may call. `--json` for both. On the demo apps the map is 9–20% of the source.
- **`soma skeleton app.cell --split`**: the file rewritten as skeleton + flesh — structure and comments verbatim, each signal with the frame its body has, bodies unchanged; a handler the outside could call joins the face. `tools/split_roundtrip.py` splits every valid program of the repository (1,049) and requires the same `check`, `test` tally and `verify` verdict.
- **`soma skeleton app.cell --freeze` / `--amend`**: `skeleton.lock` holds each skeleton's token hash and text; `check`, `run`, `test`, `serve` and `verify` refuse a program whose skeletons differ (kind `skeleton_lock`, E0121), printing the lines that changed. Comments, layout and flesh are free. `--amend` accepts a change on purpose.
- **`soma init --skeleton`**: a starter laid out skeleton-first (`app.cell` + `lib/counter.cell`), with one hole.
- **`soma describe --faces`** prints a skeleton with its frames (`--json`: `skeleton`, `frames`, `hole`); `soma fmt` hangs frame clauses under their signal.
- **Soundness check of the frames.** `SOMA_EFFECT_TRACE=<file>` makes the interpreter log every slot read and write, handler call, effectful builtin, transition and emit with the handler stack; `tools/frame_oracle.py` runs the test cells of every program (358 have some) and requires each observed effect to be inside the inferred frame of the signal that made it.

- **Hardening after review (2026-11-13).** Closed, each with a test (`compiler/tests/skeleton.rs`, reproducers in `compiler/tests/fixtures/skeleton_review/`): `[hole]` on a handler with a body disabled conformance (it is no longer writable); a local that only MAY be bound (dead branch, loop, later `let`, lambda / match binder elsewhere) hid a call, a cell or a slot — names are now resolved by lexical scope, which also ends two false positives; an interior cell named like a skeleton; an `emit` reaching a private helper; `transition(id, to)` taking an edge the frame did not list (every edge into the target is now required, or the three-argument form); calls in method form, pipes, `require` messages and `std.`. `soma verify` reads a hole as its frame: a rule its frame can break is `⚠ open obligation`, an invariant on a slot it may write is `⚠ runtime-checked`, never `✓` (`tools/skeleton_monotone.py`: every `✓` of a derived skeleton is a `✓` of the program, on every valid program of the repository). `skeleton.lock` covers interior skeletons, outside movers of a frozen machine and soma.toml's `[verify]` tables, holds under any file name of its directory and for `eval` / `build` / `replay` / `deploy` / `serve --no-check`, has a checksum (unreadable = error; `--freeze` never overwrites), and its difference names the signal. Also fixed on the way: `horde(variable, …)` was reported "its tasks call no think()"; a guard the face's `requires` already establishes was "runtime-checked".
- **The brief is enough to write a body.** `soma skeleton app.cell Cell.signal` adds the skeleton's comments (cell, signal, slots, edges), the cell's constants, the types named, each machine touched in full, builtin signatures, callee comments and contracts, callers. `soma holes app.cell [Cell]` groups by cell; `soma verify --summary`; `soma init --skeleton .`; `get_status` is documented per edition.
- **Red-team rounds 2 and 3 (2026-11-13), 14 then 26 confirmed problems, closed with tests `r2_*` / `r3_*`.** A local named like a slot is the local at run time too (`let acct = 1  acct.set(…)` wrote the slot: an interpreter fix). Only `transition(id, from, to)` pins an edge for a frame — what a body infers from `get_status` is no longer evidence; without the source, every edge into the target must be in the frame. An impure lambda may only be written as the direct argument of a builtin that applies it at once (`xs.map(x => …)`, `xs |> filter(…)`); bound, stored, passed to a signal or returned, a lambda must be pure. `std.f(…)` is charged as the builtin `f`. A cost bound is not "proven" while a hole may call or emit. `skeleton.lock` reads `[verify]` as TOML, holds the edition, follows types into the types they name, and is looked up beside every file that declares a skeleton and above. The effect trace logs the edge each transition takes, so the oracle sees a move by the wrong edge. `soma fix` reports the lock.
- **`soma docs flesh`**: one page (100 lines) with the syntax a body needs, for whoever fills a hole from its brief; every construct in it is run by a test. The brief adds `/* … */` comments, a tool's description, the listeners of what it emits and `emitted by`.
- **Pilot harness** `bench/agents/pilot.py` (`--selftest`): three arms (plain cells / skeleton first, one agent / skeleton first, one tool-less sub-agent per hole given the brief alone), hidden tests, a token ceiling that counts design, refusal of anything but flesh after the freeze, an audit of every prompt for hidden material. Applications: `bench/agents/pilot/marketplace` (27 signals) and `hotel` (40).
- **Red-team rounds 4 and 5 (2026-11-13), tests `r4_*` / `r5_*`.** The lambda line is held twice. Statically: an impure lambda is written only as the last argument of a builtin that applies it (`xs.map(x => …)`, a pipe). At run time, **for every program**: a lambda carries the cell that wrote it; applied in another cell it may only compute — a slot (also `len(slot)` and `slot[i]`, which went round the guard), a handler call, a transition, an effectful builtin (also `link`, `subscribe`, `ws_connect`, `ws_send`) or an emit raises kind `lambda`. A lambda is at home anywhere in its own cell, whatever carried it there (a helper `_each(xs, f)`, a combinator returned by another cell). `soma verify`, rules between machines: the right side of `&&` / `||` / `??`, a `require`'s detail, a `while` condition, a match guard and a `try` no longer count the calls in their string interpolations as having run once for sure; a variable assigned in a nested block, a loop variable at each turn, a match binder and a lambda parameter are values of their own (a binder named like the id, or like a state, was read as the id or the state — false ✓); under edition 2027 `get_status(id) == "initial"` is false for an id never moved and is read so (`get_status(id) ?? "initial"` is read exactly in both editions). A brief whose comment leans on the one above ("The same, paid with points.") brings that comment along.
- **Round 6 (2026-11-13): a red team (about 520 probes) and a builder agent that wrote a 14-cell, 115-signal clinic from the embedded docs alone (`bench/agents/large/clinic`, with its journal). Tests `r6_*`.** The static frame and the run-time guard held; `soma verify` did not, in seven ways, all closed: `continue` lost the following turns of its loop; `std.transition(…)` / `std.delegate(…)` / `std.think(…)` were invisible to the product search, the guard proof and termination; an interpolated string (`"{x}"`) was read as a constant id or target; two states differing by case; a guard "proven" although an effect in the same statement (or in the condition leading to it) ran before its transition; `assuming = "amount > 5"` reasoned in integers (5.5 passes `< 6`); a lambda that moves a machine and is kept for later is no longer read where it is written — the rule is then "not proven". For large programs: **`soma skeleton app.cell --index`** (one line per cell, then who calls whom), **`soma skeleton app.cell Cell`** (one cell with its comments and outside callers); a rule between machines accepts the evidence people write — `require Other.status(id) == "x"` through the other cell's getter, or a status held in a variable until the next call; `verify --summary` on an empty skeleton says once how many contracts wait for a body and names the run that leaves a rule open; sizes, counts and sums of a non-negative slot are proven `>= 0`; `[verify.after."Cell.state"]` says that only `before` reads qualified states; no "cross-cell not verified" note for a cell whose rule soma.toml states. Fixed on the way: `return Rec { … }` where the face says `-> Rec` (it was "a Map literal", E0040); a wrong argument count in a test cell's helper passes check no more; `reads Other.machine` is refused where it is declared when no body could read it; a handler whose `transition(id, "ready", "paid")` names another source no longer owes the guard of `due -> paid`; the frozen `[verify]` tables and edition hold for every entry file of the lock's directory; `soma run app.cell _helper` is refused for a skeleton; `describe` and `lint` refuse what the lock refuses. The brief prints the types of the slots it lists and how to build a value; `soma docs flesh` adds `emit`, declared types and variants, `.entries` / `.values`, `sort_by`.
- **Round 7, maintenance (2026-11-13): an agent that did not know the clinic made five change requests on it without reading its sources, with `--index`, the one-cell view and the briefs. Tests `r7_*`.** Its blocking finding: `[verify.before."Lab.asked"] requires = […]` on an INITIAL state was ticked whatever the bodies did — such a rule is now refused (an instance is there from the start, by no transition). New: **`while = ["Visit.open"]`** in a `[verify.before."Cell.state"]` table — the other machine IS in that state when the target is entered (`requires` is the past: "was open once"); read on holes like the rest. A property that proves nothing on a machine (its state is not one the machine reaches) prints `·`, not `✓`. **`soma refs app.cell Cell.signal`**: where the signal is declared, where its body is, every call and emit — program and tests — as `file:line`; the brief gives the two places to edit. **`soma check .`** is the project gate: skeleton and flesh files are checked through the file that imports them (each flesh failed alone), so the entry and every test file are judged in one command; `soma test .` ends with one total. **`soma rename app.cell Cell.signal new [--dry-run]`** rewrites the declaration, the body, every call (also inside a string's `{…}`), the frame clauses and the tests under the entry's directory, and renames an event in every cell that hears it (on the 115-signal clinic: one command, then `--amend`, `check .`, `test .`). A body left behind by a rename says which signal has no body; the one-cell view prints each helper's parameters.
- **Round 7, red team on `soma verify` alone (423 probes, 11 false ✓, closed; reproducers in `fixtures/skeleton_review/round7/red/`).** Invariants and contracts: a `match` binder or a `let` inside an expression block named like an outer local kept that local's range (`let v = 5  let u = match x { v -> v }`), and a Float reached `%` through a local — binders and Float marks are now known before any range is computed; a slot named `size`, `key`, `status` or `_slot_len` took the invariant on that word for a bound on its values; `x.count()` / `len` / `size` were Ints even when a handler of that name ran; `k |> c.set(x)` and `x |> log.push()` were not writes ("no handler writes to guarded slots"); a `requires` was "proven at N call sites" while an `emit`, a call that leaves trailing parameters out or a schedule's call went unjudged. Guards: a call into a cell with a lower-case name, or a builtin applying a lambda it is handed (`xs.map(f)`), between the `require` and the `transition()`. Rules between machines: a lambda kept for later in any form (an indexed list, `??`, an if-expression, a Map); `assuming` with a literal past 2^62. **Run time, found by the frame oracle on one of these**: `x.count()` — a method call named like a builtin that the builtin does not take — ran the handler `count` of ANOTHER cell, in a signal whose frame said nothing of it; it is refused now, as `count(x)` already was at check ("a handler of another cell never replaces a builtin").
- **Round 8 (2026-11-13), about 960 probes: 30 causes of false ✓, one frame leak, gaps in `refs` and `check <dir>`. Tests `r8_*`, reproducers in `fixtures/skeleton_review/round8/`.** Run time: a call the builtin does not take (`std.count(5, 1, 2, 3, 4)`, or `count(x)` written in a cell) no longer falls through to a handler of that name in ANOTHER cell. Rules between machines: a string's calls are read where the string stands, a `require`'s operand once; a condition that moved a machine narrows nothing (a status read before the move, or held in a variable, is stale); `vote(Cell.h, …)` runs `h`; `deliver`, `resume` and `horde` make the rule "not proven" (so does a hole whose frame calls one); two cells of the same name keep their own machines; a rule that is only a `while` no longer ends in "lists no state". Guards: never "proven" by a hole, by `if cap > 0 && zero(k) == 0 { transition… }`, by an assignment inside a `require`, by `o.get_status()`. Invariants and contracts: `let`s at any depth of an expression block, a local / parameter / slot named like a cell (`L.bal()`), a handler `max` or `keys` of the cell where the rule expected the builtin, the written key rebound in the value, interpolation in what is proven, facts carried across `await` / `resume` / `deliver`, a string in a match pattern, `slot[k] = v` in an if-expression of a `require`, `_slot_len` on a delete; `requires` sites reached by `vote`, `horde`, or a method call on a local. Tools: `soma check <dir>` checks a part that has tests of its own, files that import each other and an entry only tests import, and resolves `use name` / `use lib::x` as the loader does; `soma refs` finds the method form (`x.f()`), a bare call from another cell, `vote` / `horde` / `delegate`, strings inside strings, files under `target/`, and reads only the files an import ties to the program (a backup lying there is not the program; a test that does not parse is named). Also closed, in 3.0 analyses beside the skeleton: two state machines kept under one storage key (`A` + `B_m`, `A_B` + `m`) are refused at check; the `[task]` wait report says which calls before the wait it does not follow, and no longer counts a `transition(id, "b", "c")` as a way out of `a`; `effects` reads a think() without options (or with options it cannot list) as offering every tool; `composition` sees an `emit` inside a `try` or a lambda; `cost` counts `std.sleep(…)`, `d.sleep()` and a method call on a local named like a cell; `[deterministic]` follows a handler hidden by a local bound in a branch; `verify --attest` hashes imported files.
- **Round 9 (2026-11-13): 20 causes of false ✓, 7 frame leaks, 15 tool gaps (`rename` attacked for the first time). Tests `r9_*`, reproducers in `fixtures/skeleton_review/round9/`. Three roots, treated as such.** (1) *Two name resolvers*: the run looked a name up in every cell and in the caller first, the checker in `cell` / `cell agent` only and read `std.f` as the builtin. The run now agrees with the checker: `std.f(…)` is the builtin f and nothing else (not the cell's own handler `f`, not a handler elsewhere, `await_*` included); a bare or method call from a cell never reaches a handler of a `cell test`, `cell type`, `cell property`…; `x |> slot.method` no longer writes the slot NAMED BY THE VALUE of a local; a frame counts `vote("Cell.h", …)` as the call it is; a skeleton's slot may not bear the name of a cell or of its machine. (2) *An evaluation order rewritten by hand*: the product search follows the interpreter (a method call's arguments before its receiver, an index before what is indexed, a pattern's string where the arm is tried), counts edges taken so that a round trip narrows nothing, follows `x.h()` to the handler `h` also when `x` bears a cell's name, `vote("Cell.h")`, and reads a bare name as a state only when it is one. (3) *Facts compared by their text*: a text that leaves a body out (`if c { … } else { … }`) is no longer a key — in the interval prover and in the guard proof — and a local named `size` / `key` no longer satisfies an invariant clause by spelling. Also: the monotone-counter rule compares the `??` defaults and wants integers for a strict `>`; `effects` reads `m[k] = think(…)` and a `require`'s operands; the wait report counts the enclosing condition and the wait's arguments, and says when it cannot place a transition. `soma rename`: cells declared `cell agent` or in lower case, method form, bare calls from another cell, `delegate("Cell", "h")`, `tools_allowed`, every listener's direct calls for an event; it rewrites only the program and its tests, only inside a string's `{…}`, never a slot or a local that follows a frame clause; a new name that is a word of the language or already a name in the program is refused. `soma check <dir>` follows `use lib::dir` (a directory) and installed packages as the loader does; `--index` counts a plain cell's handlers.
- **Round 10 (2026-11-13): the red team was asked for DISAGREEMENTS between components, question by question (who runs a name, in what order, what binds a name, what writes a slot, what "the same" expression is, what a cell of the program is): about 25, with 52 reproducers (`fixtures/skeleton_review/round10/`, test `r10_*`).** Settled by making the run and the analyses answer alike. *Who runs a name*: `x.f(a)` now resolves like `f(x, a)` — this cell's own handler first (the builtin ran in method form, the handler in bare form); `x |> obj.f` without a call is refused at run time (it reached a slot or a handler under no analysis); `delegate` reaches signals of `cell` / `cell agent` only, never a `_helper`; an `emit` is heard by `cell` / `cell agent` (and by the test cell under `soma test`), no longer by a `cell type` or by a test cell under `soma run`; a handler named `transition` … is refused in interior cells too; the product reads `std.f` as the builtin whatever handler bears the name, a tuple variant `Opener(id)` as a value, `vote` by its target or not at all, a slot before a cell of the same name. *Order*: a lambda handed to a method runs after the receiver; the arms of a `match` chain (a tried arm has run its pattern and guard); a condition's baseline is its own; a status in a variable is stale from the first call of the very statement; a variable assigned inside an expression is unknown from the start of its statement. *Binding*: a frame counts `x.h()` as the call of `h` when a local MAY bear the name of the cell or of the slot `x` (an assignment without `let`, one alternative of an `||` pattern) — two leaks; a slot is the slot before a cell of the same name in inferred frames. *Same expression*: quotes are escaped in the text of an expression; a function a fact calls is a name it reads; a slot named `key` / `size` / `status` is that binding, not the value. Guards: a handler named like a builtin is a call; a call hoisted out of a string is judged with what its statement does. `soma refs` / `rename` read `vote("Cell.h")`, `horde("Cell.h")`, `std.delegate("Cell", "h")`.
- **Round 11 (2026-11-13): the answers of round 10 held (frames over 57 forms of slot write, the product over 36 neighbours, guards, `requires` sites); 10 disagreements in what had not been compared. Test `r11_*`, reproducers in `fixtures/skeleton_review/round11/`.** *The hole model*: a hole was read as ONE ordered pass over what its frame allows, so a rule ticked on a skeleton could be broken by a conforming body that calls twice or takes two edges in the other order — the model is now that set of steps under a loop ("a ✓ on a skeleton holds for every flesh that conforms" is true again, with a test). *Frames*: a handler run by `vote("_w", …)`, `vote(_w, …)` or named in a horde's options (`map("on_result", "_store")`) is this signal's call — two leaks. *Rules between machines*: a `[task]` that awaits, called by a handler, hands `{waiting}` back at the wait — the rule is "not proven" rather than read as if the callee had finished; the statements of `runtime { }` are an entry like a handler's for every proof (they wrote slots, moved machines and called contracted signals unseen). *Agent tools*: `tools_allowed` is read in the LAST argument of think(), as the run reads it; a tool that thinks, or a helper that does, offers every tool again. *Taint*: `q.think()`, `std.think(q)`, `id.transition(d)` are the source and the sink they are. `soma rename` rewrites a handler named in `vote` / `horde` options.
- **Round 12 (2026-11-13): the hole model and the product search on a flat program held everything thrown at them; 28 causes in what had never been compared (interior cells, timers, `runtime { }` under `check`, the type names, tools over several folders). Test `r12_*`, reproducers in `fixtures/skeleton_review/round12/`.** Fewer answers, rather than more cases: "is this name a cell?" is asked of the program (`names::cellish`) where a capital letter decided it in a dozen places (`cell counter` was a local to the invariant prover, the taint check, termination, the wait report, `soma serve`'s GET rule); the statements of `runtime { }` are an entry of every analysis that reads the exposed program (cost, taint and capabilities at `check` as well as the proofs), and in a skeleton they only call signals or raise events, as a schedule does; a schedule may not call another cell's private helper. Frames: `vote` / `horde` whose target is a variable count every handler of the cell; at run time a private helper is reached by name from its own cell only. Interior cells: a bare call goes to a top-level handler (the proofs took the first homonym in file order, interior included), an event a parent raises is heard by its interior (rules between machines, cost). `tools_allowed` is narrowed only when nothing the handler runs can think again (helpers of helpers, an emit, another cell). Guards: a literal built by interpolation is no constant; a slot written is another value for a fact that read it by its bare name. Numbers: `-> Int` answering a whole Float gives the Int it promises (the proofs read an Int; `a + 1 - a` was 0 past 2^53); `BigInt`, a name the run does not check, is no Int for the prover. Tools: `refs` / `rename` find the project from an entry in a sub-folder (the nearest soma.toml above) and read `runtime { }`; an imported folder is read one level deep, as the loader reads it.
- **After the review (2026-11-13).** A signal's comment is the brief its body is written from: `skeleton.lock` records a digest of it, and `soma check` WARNS when it changed since the freeze (not an error: a retouched sentence is no amendment); `--amend` records the new one. `checker/bodies.rs`: one list of what runs in a cell — handlers, `every` / `after`, `runtime { }` — read by `effects`, `capabilities` and `budget`, which saw handlers only. `tools/verdicts.sh` compares the files of `main`. What is left to unify, and two proposals that need a decision (one rule for events and interior cells; rules between machines over different ids): `STRUCTURE_DESIGN.md`.
- **The structural work (2026-11-13), `STRUCTURE_DESIGN.md`.** *One rule for who hears an event*, whatever started the program and for every analysis: raised by a top-level cell, the top-level cells and its own interior; raised by an interior cell, its siblings and its parent (`Interpreter::hears`; `check` counts a parent's events as reaching its interior). *One store per slot*: the scope runtime of `soma run app.cell` takes its back-ends from the registry like every other command — a `[persistent]` slot was a JSON file there and a sqlite table elsewhere (**data written by `soma run app.cell` in a JSON file under `.soma_data/` is not read any more**). The start-up audit covers interior cells and every cell of a slot name. *One answer to "where is the project"* (`project::root`): a test file in a sub-folder that imports the program gets the program's manifest — its edition and its `[verify]` tables. *The older analyses follow values and calls*: taint through a list, an index write, a lambda parameter and `vote`; composition by the arity of the listener and only among cells of the program; budget with the largest callee frame and `map` over an unknown length as no bound. **A rule between machines in the frame**: `moves drafted -> delivered when Consult.done [|| …] [via consult_of]` — the edge is taken only while the other machine is in one of those states; `verify` judges it on the bodies and on the holes (whose model takes the edge only under the clause), the brief prints it, the lock holds it. **`via = "Cell.slot"`** in a `[verify.before."Cell.state"]` table: the states required are those of the instance the target's instance names in that slot (written once, when the entry is created, never deleted — checked); the search follows the pair.

Not covered: a helper's privacy is its `_` name (the existing rule for routes and events); frames are per cell and syntactic; an edge's `effect { }` block and what guards and invariants read belong to the structure, not to a frame; a flesh sees its skeleton's constants when the skeleton is in the same file or in the file that imports the flesh.

## 3.0.0 — 2026-11-11

### 3.0.0: static types for expressions, every syntax error in one pass, diagnostic codes, editions, constants, multi-parameter lambdas, `++`, typed model answers, face contracts, durable waits, external tools (MCP / OpenAPI), lock-free readers under serve, `soma embed` with Python and npm packages, eight standard packages, transition sources, untrusted values, `soma fmt` / `eval` / `lsp`, attestations, recorded replay, a backend fuzzer and an agent benchmark

Specification: `docs/design/AGENT_REFERENCE_SPEC.md` (requirement IDs in brackets; its status table lists what 3.0.0 does not cover).

- **[A1] `soma check` types expressions.** Types are inferred per handler from literals, parameter types, face return types, slot declarations, `cell type` fields and the builtins whose result never varies — no annotation is added. Refused when both sides are known and every execution would raise kind `type` (the operator table was measured against the interpreter, 9 × 9 operand kinds per operator): `s * 2` on a String, `n == s`, `xs % 2`, a List indexed by a String, `for` over a number, a non-Bool condition. A field the record type does not declare (`l.quantity` on `Line { sku, qty }`, which read as `()`) names the declared fields and the nearest one; a field of a `try` result other than value / error / kind / detail names `r.value.field` (ledger D5). An inferred argument of the wrong kind (`send(body)` with `body: String` for `data: Map`, a Float for an Int parameter) is refused like a literal one was. Nothing is refused inside `try { }` or in a handler a test targets with `assert_fails`. Five example programs called a `Map` handler with the raw `body: String` on their POST routes (a run-time `type` error on every such request); they pass `from_json(body)`.
- **[A1] Absence is a type.** `slot.get(k)`, `m[k]` and `m.field` on a Map and `try { }.value` are `T?`. In edition 2027 a `T?` where a value is required — `counts.get(k) + 1`, `c.total < 3`, `rec.field` — is a check error whose fix is in the message (`(counts.get(k) ?? 0)`); `x ?? d`, `if x == () { return … }`, `if x != () { … }`, `require x != () else …` and `&&` / `||` on those tests narrow it. On the repository's 1,411 programs this finds 44 such reads in 35 files; edition 2026 leaves them to the run-time check.
- **[A2] Every syntax error in one pass.** The parser records an error and resumes at the next statement, section or cell: a file with five independent syntax errors gets five diagnostics (it took five runs). `--json` gives each the fields a semantic error has (`kind: "syntax"`, `file`, `line`, `col`, `source_line`, `fix`, `code`) — it was one `kind: "load"` object with the position inside the message; a lexer error has them too. `=>` in a match arm and `-> T` on a handler are read as meant, so the semantic errors of the same file are reported beside them. `run`, `test`, `verify` and `serve` still refuse a program with any syntax error.
- **[A3] Stable diagnostic codes.** Every error and warning carries a code (`E0110`, `W0014`, syntax `E1001`…) in `--json` (`code`; warnings gain `kind`) and in the text report (`= code: E0110`). `soma explain E0110` prints its kind and meaning, `soma explain` (or `--json`) lists them. A code never changes meaning: new kinds are appended to `checker/codes.rs`, and a unit test fails on a `kind` the checker writes that the table lacks.
- **[D1] Editions.** `edition = "2027"` under `[package]` in soma.toml; absent means "2026", the edition of every earlier program; an unknown edition is a check error. `soma init` writes "2027". Edition 2027 refuses, at check: a `T?` where a value is required; `a ?? b + c` without parentheses (it reads `a ?? (b + c)`; the 2026 warning becomes an error rather than the precedence changing silently between editions); `+` between two lists of non-numbers (join with `++`); a `return` whose inferred type contradicts the face (a 2026 warning: the run time validates only record and sum returns). `SOMA_EDITION=2027 soma check app.cell` tries a program under the next edition without editing it.
- **[A4] Cell constants.** `const LIMIT = 500` directly inside a cell (a number, a String without interpolation or a Bool literal). A constant is its literal wherever the cell reads it — handlers above the declaration, guards, invariants, `require`, interpolations (`"at most {LIMIT}"`), interior cells — so `invariant rows.size <= LIMIT` with `require len(rows) < LIMIT else Full` is proven without repeating the number. It cannot be rebound (`let`, a parameter, a loop variable, a lambda parameter), assigned, declared twice with different values, or computed (`const X = 1 + 2` is refused with the reason).
- **[A4] Lambdas of several parameters.** `(a, b) => a + b`, block form included. Called with that many arguments (`add(1, 2)`); handed ONE composite by a builtin it takes it apart in order — `reduce(xs, 0, (acc, x) => acc + x)`, `entries(m) |> map((k, v) => "{k}={v}")`, a 2-element list. A wrong count names both numbers.
- **[A4] `a ++ b`** joins two lists or two Strings (it is `concat(a, b)`), leaving `+` on lists to element-wise arithmetic.
- **[C1] Typed model answers.** `think_json(prompt, map("as", "Decision"))` returns a value of the declared `cell type`: the JSON shape derived from the type is appended to the prompt, and the answer is enforced (unknown variant, missing / extra / mistyped field → kind `json`, naming it). Check knows the result's type (exhaustive `match`, field typos) and refuses an `as` that names no declared type; `as` on `think()` is refused.
- **[A5] `soma fmt`.** The canonical layout: 4 spaces per open `{` `[` `(`, no tabs or trailing spaces, one blank line at most (none after an opener or before a closer), one final newline. Line breaks, the spacing inside a line, comments and multi-line strings are not touched, and the result must lex to the same tokens with the same line breaks or nothing is written. `soma fmt . --check` lists what would change and exits 1; `--json`. All 1,411 programs of the repository format, and format again to themselves.
- **[B5] `soma verify --attest out.json`.** The proof as an artifact: sha256 of each source (local `use` targets, soma.toml and soma.lock included) and of the soma binary, the edition, `--strict`, every property with its class (`proven`, `runtime-checked`, `unproven`, `failed`, `note`) and one digest over the document. Same sources and toolchain give the same bytes. `soma verify --check-attestation out.json` refuses an edited document, a changed source, or a re-run that differs.

- **[A6] One grammar source, three artifacts, held to the corpus.** `spec/soma.grammar` is the grammar of Soma in GBNF notation; `tools/gen_grammar.py` writes from it `spec/soma.gbnf` (for constrained decoding), `spec/grammar.md` (it replaces the 0.1 text, which described a language that no longer exists) and `editors/tree-sitter-soma/grammar.js` (`--check` fails when one is out of date). A test reads the GBNF as a decoder would and recognises every `.cell` file of the repository that `soma` parses — 1,411 of 1,411 — and refuses seven kinds of broken text. The grammar is exact for cells, the face, memory, state machines, handlers, tests, statements and expressions, and takes any balanced block in `rules` / `runtime` / `scale` / `cost` / `protocol` and in a transition's guard block; it knows none of `soma check`'s rules. Not done: the tree-sitter grammar is a mechanical translation that has not been compiled with the tree-sitter tool, the GBNF has not been run under a decoder, and `soma lsp` still has no hover, completion or go-to-definition.
- **[A4] `std.f(…)`, one vocabulary for locals and slots, and `get_status` of an unknown id.** `std.len(x)` is the builtin `len` whatever handlers the program names `len` (also piped: `xs |> std.map(f)`); `check` refuses a `std.` name that is no builtin, and a `std.` call where `std` is also a local. In edition 2027 a bare call of a name the program gives a handler IS that handler: the call an argument count used to send to the builtin (a warning in 2026, which now names `std.f(…)`) is an error. As statements, `xs.push(x)` on a local list and `m.set(k, v)` / `m.delete(k)` on a local map now write the local, as they write a slot (the first did nothing and drew a warning, the others were check errors); as an expression `xs.push(x)` still gives a new list. A statement that computes a value and drops it (`push(xs, x)`) stays a warning in 2026 and is an error in 2027. In edition 2027 `get_status(id)` of an id that was never transitioned is `()` instead of the initial state; `soma fix --edition 2027` counts the `get_status` calls to read, since no rewrite can make that change. The edition now reaches `soma run`, `soma serve` and `soma test`: their check read no soma.toml and applied the 2026 rules to a 2027 program.
- **[B4] Linear integer arithmetic behind the interval prover.** An interval knows `to <= 1000` and `amount >= 1`, not `amount <= 1000 - to`, so `acct.set(b, to + amount)` after `if amount > 1000 - to { return 0 }` was "runtime-checked" until the author restated the invariant as a `require` over the written expression. A clause the intervals leave open is now tried as linear arithmetic: the facts that hold at the write — the `require`s before it, the conditions of the `if`s around it, the negation of an early exit, as the interval pass already selects them — become constraints between the handler's Int values, and the clause is proven when they have no solution together with its negation (Fourier–Motzkin elimination, a decision procedure written here: no solver to install, the same verdict on every machine, a bound on the work past which nothing is proven). A value is an `Int` parameter the handler never reassigns, a local bound once to `Slot.get(k) ?? <Int literal>` on an Int slot (it satisfies the slot's interval invariant, or is the default) or to a call of a handler whose face says `-> Int`, and any sum, difference or constant multiple of those; a Float, a division, a product of two values, a reassigned or shadowed local, a read without a default or inline is not one — a fact over it is dropped and a goal over it stays runtime-checked. The same step proves a face `ensures` that relates the result to a parameter (`ensures result <= amount`, enforced but unproven until now) for every returned value. `soma verify` says which proofs are of this kind (`proven (linear arithmetic over the handler's requires and conditions)`). On the corpus it proves two writes that were runtime-checked (the dialysis ultrafiltration ledger, the `ratelimit` package's token bucket, which now passes `verify --strict`) and changes no other verdict. Deviations from B4: a built-in procedure for linear integer arithmetic, not an external SMT solver; not for Float slots, `[task]` handlers or size clauses; a rule between two slots is still enforced on every write and not proven.
- **[D3] Eight packages in pure Soma, each with its proofs.** The registry held one package (`matrix`). It now lists `money` (minor units, no Float; `allocate` keeps every cent — a property test over 60 totals; half-away-from-zero rounding; ISO 4217 exponents), `time` (Unix seconds ↔ civil time in a zone, daylight saving by rule for 40 zones — EU, US, south-east Australia —, ISO 8601 in and out; its expected values were taken from the tz database for 2026), `auth` (JWT HS256 sign / verify with expiry and not-before, `none` and every other algorithm refused, constant-time comparison; signed webhook payloads with a replay window; base64url — checked against RFC 4648 and the reference HS256 token), `ratelimit` (fixed windows and token buckets in the cell's persistent memory, `Retry-After`), `queue` (durable jobs: claim with a visibility timeout, ack, nack, dead after N deliveries, as a declared state machine), `text` (words, sentences, n-grams, edit distance, a line diff), `vector` (cosine, nearest-k, and a local hashing-trick embedding that needs no model) and `http_client` (retries with backoff on timeouts / 408 / 425 / 429 / 5xx, a POST only when idempotent, pagination by URL or cursor, percent-encoding). 260 assertions in all; every package passes `check` without a warning, `test` and `verify`, and one application uses the eight together (a test). Every function that needs the time takes it as an argument, so the packages are pure and their tests exact. Each has a `soma.toml`, a README and an entry in `site/repo/`. Not done from D3: the Postgres and S3 adapters (they need a wire protocol and binary HMAC keys the builtins do not offer), time zones beyond rules (no tz database, no history), RS256 / ES256 and password hashing in `auth`, a model's embeddings in `vector`; the registry entries are in the repository, not deployed.
- **[C9] A Soma program as a library: `soma embed`, and the Python and npm packages.** `soma embed app.cell` serves the program's handlers to a host process over stdin / stdout, one JSON object per line: the first line out is `{"ready": true, "cells": {…}}` with every handler, its typed parameters and its result type (or `{"ready": false, "errors": […]}` and exit 1 for a program that fails `soma check`); a request is `{"id": 1, "call": "refund", "args": ["o1", 40]}` — `Cell.handler` when two cells share a name, `args` by position or by parameter name — and the answer `{"id": 1, "ok": true, "value": …}` or `{"id": 1, "ok": false, "error": {"kind": "invariant", "message": …}}`. Each call is one atomic invocation with the program's data beside it, as under `soma run`; only handlers are callable (a builtin is not); values keep their types — an Int past 2^53 is `{"__soma_int__": "digits"}` both ways, a variant of the program's sum types is `{"_type", "_variant", …}` and is held to the type when it comes in; `print()` goes to stderr. `packages/python` (`import soma`: `with soma.load("app.cell") as app: app.refund("o1", 40)`, `soma.SomaError.kind`, Python ints of any size, `soma.Variant`, `soma.LoadError` with the diagnostics) and `packages/npm` (`const app = await load("app.cell")`, the same with promises, `bigint`, TypeScript declarations) are the host's side: no dependency, they start the `soma` binary (`SOMA_BIN`, `PATH`, `~/.soma/bin`). Both run their tests against `packages/fixture/app.cell` from the test suite. Not done from C9: neither package is published to PyPI or npm (the spec's `pip install soma` / `npm i soma`), they need the `soma` binary installed rather than bundling it, `[native]` handlers run interpreted under `embed`, and calls are served one at a time.
- **[C8] Readers no longer wait: under `soma serve` a handler that only reads runs on a snapshot, beside the handler lock.** Every invocation took the one handler lock, so a slow read (an `http_get`, a large scan) stopped every other request, and a read waited for whatever write was in progress. A read-only invocation now opens a read transaction on its own connection — the database as of the last commit, for as long as it runs (SQLite's WAL) — and never takes the lock: readers run in parallel, do not wait for a writer, and see none or all of what a writer does. Which invocations are readers: a handler of the served cell that writes nothing by its text (no slot write, transition, emit, effect builtin, call into another cell — the analysis that already keeps a GET from writing), and a `GET` / `HEAD` through `request`. The second is a guess, so the guarantee does not rest on it: on a snapshot a slot write is not made, an effect builtin (`think`, `http_post`, `transition`, `publish`, `next_id`, …) and an `emit` are refused BEFORE they run, the invocation ends there even inside a `try`, nothing it did is kept, and it runs again under the lock as a writer — once; the route (`GET /counted`) is then remembered and no longer tried, with one line in the server log. Atomicity and write ordering are unchanged: writers are still one at a time. A snapshot is used only when every slot and machine of the program is in the database (a slot kept in memory has none: a writer's uncommitted value would be read), when the program has no `[native]` handler (compiled code was only ever entered by one invocation at a time), not in a cluster, not for a `[task]` handler; `SOMA_SERIAL=1` keeps every invocation under the lock. Not done from C8: two WRITERS with disjoint footprints still run one after the other (one SQLite file has one write transaction), and `http_get` / `sleep` in a writer still hold the lock.
- **[C5] Tools from outside: a `tool` bound to an MCP server or an OpenAPI operation.** `tool search(query: String, limit: Int) -> String [mcp: "docs", capability: "https://docs.example/*"] "Search the docs"` and `tool get_pet(petId: Int) -> Map [openapi: "pets/getPetById"]` have no handler: `docs` and `pets` are `[tools.<name>]` tables of soma.toml (`mcp = "https://…/mcp"` — the Model Context Protocol over Streamable HTTP, JSON or event-stream answers, sessions — or `openapi = "petstore.json"`, a JSON OpenAPI 3 document here or at a URL, with `base_url`; `token_env` names the environment variable holding a bearer token). The program and the model call such a tool like any other. `soma tools sync` asks each server for the contract of the tools bound to it and pins it in soma.lock (canonical schema and its sha256; a `CHANGED` line shows the old and the new one); `soma tools list` shows the pins. `soma check` then holds the declaration to the pin without the network (kind `tool_binding`, E0118): a server soma.toml does not define or of the other kind, a tool not pinned, an argument the server does not take, a wrong type, a required argument left out, a handler beside the binding, capabilities that do not allow the tool's own server. At run time the first call of a process compares the server's present contract with the pin and refuses a drifted one before sending anything (kind `tool_schema`); the tool's capabilities apply to every call, the program's own included; redirects are not followed; a failure on the server is kind `tool`; an answer is held to the declared return type and another server's JSON never becomes one of the program's records. An OpenAPI call fills path, query and header parameters and builds the JSON body from the remaining arguments (a non-object body is one argument, `body`). In a `[task]` handler the request waits outside the handler lock, as a model call does. A test never reaches a server: unscripted, the call raises and names `mock <tool> …`. `soma describe` shows `bound_to`. This is Soma consuming tools; it does not bring back a Soma tool server. Not done from C5: MCP over stdio, YAML documents, nested argument schemas, MCP resources / prompts.
- **[C3] Durable waits: `await approve(…)`, `await signal(name, key)`, `await sleep(duration)` in a `[task]` handler.** `approve()` under `soma serve` raised `approval_required`, and the documented pattern was a hand-built pending state and a second endpoint. Now `let ok = await approve("refund {amount} for {id}")` ends the step when nobody can answer: what the handler wrote commits, its position (the n-th wait of the handler) and its locals are stored in the cell's `_waits` table in the same transaction, and the call returns HTTP 202 `{"waiting": "w_…", "kind": "approve", "message": …}`. The rest of the handler runs — as a step of its own, after a restart too — on `POST /_approvals/<id>` `{"approved": true|false}`, on `resume(id, answer)` from a route of the program, on `deliver(name, key, value)` / `POST /_signals/<name>/<key>` for a signal, and from serve's timer when a sleep is due (a sleep that came due while the server was down resumes at start-up). A resumed step that fails rolls back and leaves the wait to be answered again; a wait is consumed once (a second answer is `not_waiting`, 404); `waiting()` lists them and `cancel_wait(id)` drops one. Scripted and immediate answers still win: `mock approve true`, `SOMA_APPROVE`, a person at the terminal under `soma run`; `await sleep` sleeps between two steps under `soma run` and does not wait in a test. The `/_approvals` and `/_signals` routes exist only for a program that waits, and answer only a loopback server or the bearer of `SOMA_WAIT_TOKEN` (401 / 403 otherwise); `resume` and `cancel_wait` are refused inside a tool call — the model does not answer its own request. `soma check` refuses a wait it could not resume (kind `await`, E0117): outside a `[task]` handler, in a timer block, in a loop, a lambda, a `match` arm, `try` or a larger expression, or in a handler whose face states an `ensures`; a local holding a lambda at the wait is a run-time error of the same kind. A wait is only resumed into the handler it was taken from: an edit of that handler makes its stored waits `wait_stale` (409), an edit elsewhere in the file does not. `soma verify` adds a `Cell/waits` block: per wait, what is committed before it, the machine state an instance rests in, and whether another handler or timer takes a transition out of that state — a state only the answer leaves is a warning (an unanswered instance stays there forever). A `transition()` before a wait no longer draws the "runs BEFORE a think()" warning (a wait is stored, not replayed); the stale-read warning of `[task]` now counts a wait as a boundary. `soma describe` lists a handler's `waits`. Not done from C3: waits inside loops, the wait as a state of the machine's temporal check, a migration path for waits across a handler edit.
- **A constant before a block's brace is the constant.** `if amount > LIMIT {`, `while n > LIMIT {` and `match LIMIT {` reported "undefined variable 'LIMIT'": a constant followed by `{` was left as a name, in case it began a record. Only `Name { field: …` does.
- **[B2] A rule between two cells' machines, proven on their product; `assuming` for a rule that depends on a guard.** `[verify.before."Pay.paid"] requires = ["Approval.signed_off"]` names states of two cells: `soma verify` follows one instance id through every machine of the program at once and runs every handler from every reachable product state — a handler invocation is one atomic step that applies each `transition()` it makes, through the handlers it calls in any cell (`Cell.h(…)`, bare calls, `delegate` with literal names), the listeners of what it emits, an edge's `effect` and the tools its model may call, or fails and moves nothing. A failure prints the run of handlers that breaks the rule (`Pay.open → Pay.settle ← enters paid here`); a pass prints the number of product states explored and the machines (`6 product state(s) explored over Approval.review (3) × Pay.payment (3)`). The abstraction is always towards more behaviours, so a `✓` holds for the program: a condition is taken both ways unless it tests `get_status(id)` against a state (directly, through `!` / `&&` / `||`, a `require`, or a handler whose body is one `return` of such a test), an id is the followed one or another one — both tried, the same variable being the same id within one invocation —, a guard may hold or not, a loop, a lambda or a tool runs any number of times, a failed `try` leaves the state it started from, and each transition of a `[task]` handler (which commits step by step while other invocations run) is a step of its own. The search is exhaustive or refused: past 200,000 product states the property fails with that count. Recursion deeper than 12 calls and a `delegate` with a computed target fail the property rather than pass it. `assuming = "amount > 500"` on a `[verify.before.…]` entry (a conjunction of integer comparisons) restricts the rule to the runs where it holds: the edges whose guard cannot hold then are left out, and verify names them. treasury states its headline that way — the direct edge `proposed -> authorized` now carries the guard `amount <= 500` (`authorize` takes the amount), and "assuming `amount > 500`, no path reaches `paid` without `signed_off`" is a `✓` in `soma verify`, no longer a comment pinned by tests. Not done: the other property kinds (`never`, `eventually`, `after`) are still per machine; a clause relating two parameters (`ensures result <= amount`) is enforced but not proven.
- **[B2] Face contracts: `requires` / `ensures` on a signal, checked on every call and proven across cells.** `signal debit(amount: Int) -> Int  requires amount > 0 && amount <= LIMIT  ensures result >= 0` — a clause compares the parameters (and `result` in `ensures`) with literals, constants and pure builtins; a slot, a handler call, `now()` or a model call in a clause is a check error (kind `contract`, E0116), as is a contract on a `[native]` handler (compiled code calls its siblings past it) or on a handler that renames a parameter. The interpreter evaluates `requires` before the handler runs (kind `requires`, HTTP 400, nothing written) and `ensures` on what it returned (kind `ensures`, HTTP 500, the invocation rolls back) — on every way in: a nested or recursive call, a scripted `mock`, a model's tool call. Because it is enforced, `soma verify` uses it without looking across the boundary: inside the handler a `requires` is a fact (a write of a bounded parameter is proven against the slot's invariant), and at a call site in ANY cell the result lies in what `ensures` says (`seen.set(k, balance())` and `Ledger.balance()` were "unbounded"). Verify then proves that the checks cannot fail, in a `Cell/contracts` block: each `ensures` conjunct against every returned value, path by path (a result the face types `-> Int`; a slot read that may be `()` is not a number); each `requires` conjunct at every call site in the program, naming the callers that do not establish it. A call or a return that can never satisfy its clause fails verify. `soma describe` publishes the clauses; `soma fmt` hangs them under their signal. Not done from B2: the product of two cells' machines is still not explored (a property like "no path to `paid` over the limit without `signed_off`" stays a test), and a clause relating two parameters or `result` to a parameter (`ensures result <= amount`) is enforced but not proven.
- **[B3] `soma verify` knows where a handler starts.** A `transition(id, target)` after `require get_status(id) == "held" else …`, inside `if get_status(id) == …` / `match get_status(id) { "held" -> … }`, after `let s = get_status(id)  if s != "held" { return … }`, or after an earlier `transition(id, …)` of the same handler is proven as the edge it is (`handler `pay` ⟶ {held → paid}`), and leaves the "transition sources" note. A call none of whose known sources has an edge to the target is a verify failure (it raises `invalid_transition` on every path). Facts are dropped at a call into another handler, an `emit`, a model call and around loops; paths are joined without their data conditions, so a call some known source allows is never refused. 113 of the repository's programs gain inferred sources; none changes verdict; 7 declared edges are now (correctly) reported as never taken.
- **[B6] Untrusted values.** What `think()` / `think_json()` / `http_*()` / `read_stdin()` return, and what is computed from it, may not decide the target of a `transition()`, the URL of an `http_*` call, a file path or `set_budget` until the handler tests it (a `require`, an `if` / `while` condition, a `match` subject) or hands it to another handler. A warning in edition 2026, error kind `untrusted_value` in 2027. No program of the repository does this.
- **[B1] Backend fuzzer.** `tools/diff_fuzz.py` generates handlers in the native-eligible subset (Int / Float arithmetic, `idiv`, `%`, bit operations, conversions, `if`, `for`, `while`, early returns), writes each twice — interpreted and `[native]` — and compares value-with-type or error kind on the same arguments. 108,000 comparisons: no differing answer; 388 cases where native code refuses with its own message (an Int past 64 bits inside a Float expression, an exact quotient past 2^53 in one) are counted apart. A small campaign runs in the test suite.
- **[C4] `soma replay` reproduces the model.** `soma run --record` writes each `think()` / `think_json()` answer (or failure) to `app.somalog.replies`; `soma replay` hands them back in order instead of asking the model again, and `--live` keeps the old behaviour.
- **[C6] `soma eval`.** `soma eval app.cell --handler classify --dataset cases.jsonl [--grade exact|contains|handler:Cell.judge] [--threshold 0.9]` grades a handler on JSON-lines cases, stores the run in `.soma_eval/` and names the cases that passed in the previous run and fail now.
- **[C7] OpenTelemetry.** `SOMA_OTEL_FILE=spans.jsonl` appends one OTLP/JSON span per top-level handler invocation, with an event per model call (GenAI attributes), tool call, approval and transition; content is exported as sizes unless `SOMA_OTEL_CONTENT=1`.
- **[D1] `soma fix --edition 2027`** writes the parentheses of `a ?? b op c` (as `a ?? (b op c)`, what it meant), rewrites list joins to `++`, sets the edition in soma.toml, and reports what is left (absent values, face returns) as check does. The parenthesised program must have the same AST or nothing is written.
- **[A6] `soma lsp`.** A language server over stdio: `soma check` diagnostics (codes and fixes) when a file is opened or saved, whole-document formatting. No diagnostics for unsaved text, hover or completion yet.
- **[D2] The agent guide is split.** `soma docs agent` (site/llms.txt) is the language, 692 lines instead of 992; `soma docs agents` (LLM agents in depth), `soma docs native` (the `[native]` vocabulary) and `soma docs releases` (what each release changed) are their own files, all carried by llms-full.txt / llms-service.txt.
- **[A7] Agent-convergence benchmark.** `bench/agents/`: 322 tasks generated from the corpus (scenario, contracts, acceptance tests; the corpus program is the unseen reference), a runner that drives any command-line agent through check / test rounds, and a report (first-pass check rate, rounds to green, characters, fraction of stated properties proven). The reference solutions score 322 / 322 and 86 % proven. No model results are included.
- **[D4]** `SECURITY.md` (a false proof is a security issue), `docs/SOUNDNESS_LOG.md` (the ten known false proofs and their fixes), `docs/rfcs/` (what needs an RFC, what may land outside an edition).

Compatibility: a program whose every path ran still checks (0 new errors on the 322 corpus programs, the six demo applications and the matrix package; all keep their `test` verdicts). A program with a line that could only raise now fails `soma check` there. `(a, b) => …`, `const`, and `++` were syntax errors. The text report gains one `= code:` line per error; a syntax-only failure of `check --json` reports `kind: "syntax"` objects instead of one `kind: "load"`.
Scope: inference is per handler and one-sided — a reassigned local, a lambda parameter, a match binding and most builtin results are untyped, `[native]` handlers keep their own typing, test cells are not typed. Edition 2027 changes what check refuses, not what the interpreter does (`["a"] + ["b"]` still joins at run time). `soma fmt` settles layout, not spacing within a line. An attestation records the verifier's verdicts; it is not an independently checkable proof witness. The replayed model answers are final texts: a `think()` that called tools does not run them again, and only `--record` runs write them. Evals run as generated test rules (no per-case token cost). The benchmark has no Python / TypeScript arm and its acceptance tests are given, not hidden. Not in this release: product properties other than `before` [B2], two-slot invariants as proof obligations and an external SMT solver [B4], a fix for what native code refuses [B1], provider adapters beyond the OpenAI-compatible and Anthropic wires, streaming and structured-output schemas [C2], waits inside loops [C3], MCP over stdio [C5], parallel writers [C8], published Python / npm packages [C9], a compiled tree-sitter grammar and the language server's hover / completion [A6], and the Postgres / S3 adapters [D3].

### Parser and check pass: a string after `require … else Tag`, too few builtin arguments, piped method calls

- **A string on the line after `require … else Tag` became the require's detail**: `require x > 0 else Tag` followed by `"hello"` as a block's last value returned `()` (and a failing require printed "Tag: hello"). The detail must sit on the require's line; a string on the next line is a statement of its own.
- **Too few arguments to a builtin passed check**: `think()`, `pad_left("x")`, `count(xs)` failed only at run time. Check refuses a call with fewer arguments than any documented form takes (not inside `try { }` or `assert_fails`, which expect the raise); `map("a", 1, "b")` (odd key-value pairs) likewise. `filter_by(rows, field, value)` — the `==` form that always ran — is now in its documented signature.
- **`x |> obj.m(a)` passed check and raised "right side must be a function call"**: it is `obj.m(x, a)`, as `x |> f(a)` is `f(x, a)` — on a slot (`"k" |> counts.set(1)`), another cell's handler (its arity counts the piped value) or a value's method.

### serve tenth pass: a reset client ended the server, HTTP/2 lines hung, pipelined refusals stalled the accept loop, bodiless statuses, request targets and Host, loopback by address, WebSocket JSON and logs, query-borne inf, static files

- **A client could stop `soma serve`**: a request followed at once by a TCP reset left the accepted connection without a peer address; the vendored tiny_http unwrapped it and the panic ended the process (exit 70). The request now has no address instead.
- **A refusal pipelined behind a long answer stalled every client**: refusals made in the accept loop (403 Host / Origin, framing 400s) waited for the earlier answers on their connection — behind an SSE stream or a large response the client did not read, the accept loop (and so the whole server) stopped. They are written on a thread of their own.
- `GET / HTTP/2.0` (or 3.0) was never answered: the 505 waited behind the request's own writer forever. It is answered 505 and the connection closes; `HTTP/0.9` is 505 too (it was served as keep-alive 1.x).
- RFC 9110 §8.6: a 204 and the interim `100 Continue` carried `Content-Length`, a 304 `Content-Length: 0`, the OPTIONS 204 `Content-Type: text/plain`; none now. A 205 has no content. An HTTP/1.0 `Expect: 100-continue` is ignored (the 1.0 client read the 100 as its response).
- A request target that is not `/…` (`GET hello/x`) was routed to `hello`; an HTTP/1.1 request without Host was served; `Transfer-Encoding` in an HTTP/1.0 request was decoded. All three are 400 and the connection closes.
- A HEAD on an SSE route was sent the endless stream; it gets the headers only.
- The refusals the HTTP layer makes itself (400 on a bad request line, 431, 505, 417) carry `Access-Control-Allow-Origin: *` like every other answer (a 431 for a browser's oversized cookies was unreadable to a page on another origin).
- `--host [::1]` (or any loopback address but the three spellings `127.0.0.1`, `localhost`, `::1`) turned off the Host / Origin checks of a loopback server and the WebSocket Origin rule; loopback is now decided by address. An IPv6 host is bracketed in the banner (`http://[::1]:8080`) and the in-use probe.
- `on ws` replies: a Map / List is sent as JSON (its display text sent `NaN`); the error log line escapes the frame's text (`fail("bad", message)` with a newline forged log lines), as do the bus failure line and `--verbose` bodies.
- A Map argument from the query or a path segment refuses a number beyond Float range, as the body does (`?data={"a":1e400}` handed the handler an inf).
- Static files: an unreadable file answered 200 with an empty body (a cache kept it) — 404 now; an upper-case extension (`logo.PNG`, `IMG_1.JPG`) gets its content type.

### Ninth pass: merge and _status, typed Float/Bool/Int keys, from_json Float fields, rows only in pipelines, wide CSV records

merge() checks a response's `_status` as with() does (a 999 answered 500 after the writes committed; a String was ignored). A Map<Float, V> / Map<Bool, V> slot refuses keys of another type and gives its keys back typed; a Map<Int|Float|Bool, V> parameter checks its keys (it took {"007": …}). from_json of a `_type` object gives a declared Float field a Float. The pipeline builtins with a field (sum_by, avg_by, min_by, max_by, count_by, filter_by, pluck, group_by, agg, select, distinct_by, distinct(rows, f)) refuse an item that is not a row, as sort_by does (a number was its own field value in distinct, dropped from avg_by). from_csv refuses a record wider than its header (the extra cells were dropped). pad_left/pad_right refuse a fill that is not a String. The start-up audit of old Float-written keys skips String-keyed slots (a version key "2.0" is real). from_json names the nesting limit instead of "not valid JSON". The corpus programs that plucked the values distinct(rows, f) already returns are fixed.

### serve ninth pass: refusals close the connection, strict header lines, header limits, URL secrets redacted, Upgrade framing, typed query/headers maps, --fresh with an unparsable sibling, stable check order

- **Request smuggling closed (again)**: a 403 Host / Origin refusal answered before the body was read left a chunked body on the connection, parsed as the next request. Every refusal before the body is read now closes the connection, and in the vendored tiny_http any response given while a chunked body is unread closes it too; once closed, no request already buffered behind it is parsed.
- **Header lines are strict**: an obs-fold line (leading space / tab) became a header of its own (an injected `Authorization` or `Transfer-Encoding`), and a bare LF / CR or a NUL inside a header line hid a second header (a hidden `Content-Length`). All are 400 and the connection closes; so is junk after the HTTP version.
- **Header limits**: a request line or header line past 16 KiB, a header section past 64 KiB or more than 200 headers is answered 431 and the connection closes (they were unbounded).
- `Connection: Upgrade` no longer hands the raw socket over as the body (a hang, or the next request swallowed): the body is framed by Content-Length / Transfer-Encoding as always.
- **URL secrets**: under `soma serve` the `error` text of an `http_*` failure map names the URL without userinfo, query string or fragment (`?api_key=…` and `user:pw@` reached the client via `return r` / `fail(r.error)`). `soma run` / `soma test` keep the full URL.
- `request`'s `query` / `headers` must be `Map`, `Map<String, String>` or `Any`: `Map<String, Int>`, `Map<Int, String>` or `Map<String, List>` passed check and start-up, then every request failed echoing a header value. And a trailing query / headers parameter typed `Map<String, String>` or `Any` may be left out like plain `Map` (`request("GET", "/x", "")` in a test, `soma run app.cell request GET /x ""`).
- `soma run --fresh` with a sibling `.cell` that does not parse keeps every table of the cells it seems to declare (the shared `App_items` slot table was dropped), and says which file.
- A body declared `application/x-www-form-urlencoded` made of keys only (`flag`, `flag&x`) is a form (it was refused "must be JSON").
- `soma check` prints sibling signal errors in declaration order (the order changed on every run).
- A directory run's `--json` summary always has `note` (null when there is none).
- Chunked bodies: whitespace before `;` in a chunk size line and leading zeros past 15 digits are accepted (the value is still capped).
- An absolute-form request target (`GET http://localhost/x`) is routed by its path, its authority taking the place of Host (it was a 404).
- `-p 0` no longer logs "port 2 stays closed" (nor binds the bus on port 2).

### interior ninth pass: timers refused, qualified runtime tables, scoped signals, evaluated runtime section, --fresh for interior data, no guessed handler, deterministic fan-out

- **`every` / `after` in an interior cell are a check error**: they never ran (neither `soma run` nor `soma serve` schedules interior cells — serve does not instantiate them at all), silently. The error names the block and the fix: put the timer in a top-level cell. llms.txt, SKILL.md and SOMA_SPEC.md say timers run in any *top-level* cell.
- **Interior runtime tables are per qualified cell**: an interior `Worker`'s state-machine instances, `next_id()` counter and `remember()` memory lived in `Worker__sm_flow`, `Worker__counters`, `Worker__agent_memory` — shared with a top-level `Worker` and with a `Worker` inside another program of the directory. They are now `Sys1.Worker__…`, like its slot files. On first run the rows of the old bare-named table are copied once (a start-up note says so; the old table is left untouched, as another cell may own it) — except when the program has a top-level cell of that name, whose table it is.
- **An interior `signal` stays in its scope**: it reached every cell of the program with a handler of that name, top-level ones included (which then failed on their own slot, not injected). It now reaches the siblings (and the parent); a top-level emit reaches top-level cells. The sibling fan-out follows declaration order (it followed HashMap order).
- **The runtime section is evaluated**: `signal go([1, 2])`, `signal go(1 + 2)`, `signal cfg(map("limit", 3))` delivered `()`, `let k = 4  signal go(k)` delivered `"k"`, an undefined name delivered its own text, and `print(…)` / `let` were ignored. Statements now run in the interpreter, in order, in one environment, and `signal` arguments are evaluated. `soma check` walks the section like handler code (undefined names and functions), and checks `start` / `connect` cell names and the arguments of each `signal` against the handlers (count, literal of the wrong type: `signal go("5")` for `on go(n: Int)`). `start` / `connect` are declarations (interior cells are live from the start); the signal log says so instead of "wired".
- **`soma run --fresh` resets interior cells**: their JSON slot files survived a partial reset (another program's tables in the directory), and their machine tables were under bare names. Their files are now removed with the program's tables; another program's JSON files count as foreign data (they are kept, and the directory is not removed wholesale).
- **`soma run app.cell 7` on a program with a runtime section is refused** with the list of handlers (`A.withdraw, B.echo`): it ran the first child's first handler with 7.
- Docs: `--jit` is documented as the deprecated no-op it is (SOMA_SPEC.md, SKILL.md said "bytecode VM"); `soma init` no longer describes `.soma_env/cache/` as compiled bytecode. The uncalled `run_with_vm` is removed (the `vm` module stays).

### serve eighth pass: malformed chunked bodies refused, client-safe try results, typed query/headers, interior handlers by name, fix with generic types, --fresh sibling slots, stable --json schemas, form flags, static %2F

- **Request smuggling closed**: a malformed or truncated body (a bad chunk size, a chunk longer than declared, an EOF before the last chunk or before Content-Length bytes) ran the handler on what was read and left the connection open, its rest parsed as the next request. It is now answered 400 and the connection closed and poisoned. The vendored tiny_http gets a strict chunked decoder: trailers after the last chunk are read and discarded (`0\r\nX-T: 1\r\n\r\n` made the next pipelined request a 400), chunk sizes are plain hex, bare CR/LF are errors.
- **Try results no longer leak stored values to clients**: under `soma serve`, a caught error's `r.error` / `r.detail` hold the text a client would get for it (the program's own `fail`/`require` words unchanged, a runtime error as a value-free sentence), and the full text goes to the server log once. `fail(map("error", r.error, …))`, `fail(r.kind, r.detail)`, `fail(r.error)`, `return r` and `response(410, map("error", r.detail))` sent the runtime's text, quoting stored values. `soma run` / `soma test` keep the full text. serving.md's example now writes its own message.
- `request`'s `query` / `headers` parameters must be typed `Map` (or `Any`): `query: String`, `headers: String` or `query: List` passed check and start-up, then every request failed (and `headers: String` echoed the client's headers, cookies included, in the 400). Refused by `soma check`, `soma serve` and `soma run`.
- `soma run app.cell booked 5`, where `booked` is a handler of an interior cell, ran the first child's first handler with "booked" as its argument; it now runs that handler (through the runtime, siblings registered). `Child.handler` names one; a name two cells declare asks for `Cell.handler`; an unknown name on a runtime program is `error: no handler named 'x'` (it ran the first child's handler with the name as data).
- `soma fix` moves a return type when a parameter's type holds a comma (`m: Map<String, List<Int>>`), and the check message shows the whole type (`-> Map<String, List<Int>>`, not `-> Map`).
- `soma run --fresh a.cell` keeps a slot table another `.cell` file in the directory declares (the same cell and slot); sibling files are parsed (a `cell X` inside a multi-line string counted as a cell), and a sibling cell `App_x` no longer makes `App`'s counters look shared.
- `--json`: `soma test` prints the same keys for every outcome (`error`, `errors`, `check_errors` always, null/empty when absent); `soma verify` always has `note`; `soma lint`, `soma describe` and `soma fix` print their own schema with an `error` field on a lex/parse error, a missing file or a directory (it was the check schema).
- A form body `a=1&flag` keeps `flag` (present with "", as in the query).
- `/static/`: `%2F` is not a path separator (`/static/sub%2fx.txt` was served) and a `/` after a file name is a 404; a name is matched with its case as on disk.

### native eighth pass: Int locals given a Float, Float parameters, quotients read through to_int, Float range bounds and Int arguments, String parameters assigned, interpreter texts, one memo per call

A [native] handler gives the interpreted answer or is refused. An Int local later given a Float (`let t = 0  t = f`, `t = t + f` in a loop, `t = fl(x)` of a Float sibling, `t = 1.5`) is typed as the join of what it holds — a Float that knows when it is still an Int — where it was read as an Int (3.0 answered 3, bit_len(4.0) answered 3; the literal case was refused); an Int parameter given a Float is refused at check. A Float parameter later assigned a quotient starts as a Float (`if x > 0 { return f }  f = x / 2` printed 3 for 3.0), and `g = x / to_int(g)` no longer takes g's Float-ness through to_int (an exact 4 printed "4.0"). A Float where an Int must be raises the interpreter's error: a range bound (`range(0, f)` looped f times; `range(0, 7 / 2)` said "Int / Int is not exact here"), an Int builtin's argument (`bit_len(t)` of a local holding 0.0 answered 0), a sibling's Int parameter ("n(): parameter 'n' expects Int, got Float 2.0" — it said "an Int parameter got the Float"). Assigning a String parameter compiles (`s = "z"`, `s = s + "!"`, `s = id(s)` failed in rustc), a local copied from a String parameter may grow with `+`, a range bound of overflowing literal arithmetic compiles, and Rust keywords are fine names (`let fn = 1`, a parameter `self`: bindings are renamed). Texts as interpreted: an Int past 64 bits as a str_at index or range bound, shl / bit_set past the 2^24-bit cap, floor/ceil/round of inf in BigInt mode. A memo cache belongs to one call from the host (a warm cache let `f(30000)` pass where a cold one overflowed the stack, and a serving process only grew it). Messages quote the user's names only where the code has them (a sibling `u_id` and the literal "u_x" lost their `u_`), and an unsupported statement is named instead of dumped.

### Eighth pass: guards across cells, one name per scheduled block, effect blocks refused, deterministic scopes, fast-path keys and _status

verify: a guard that reads a slot is not proven by a caller of another cell (its require read its own slot of that name, the guard reads the machine cell's); two every/after blocks of one interval are proven one by one (`every@1000ms#3`, the second was never walked); a `!=` guard is decided on values (`-0` and `0`); a transition `effect { … }` block, which the runtime never ran, is refused by check; [deterministic] sees a slot read after a binder of that name, and refuses to_sampled/drop_sampled (a process-wide registry). A [native] handler ending in `if … { a } else { b }` is told to return in each branch (the refusal said it answered (), which was false). `m = with(m, 3.0, …)`, `m |> with(…)` and `w = without(w, 5.0)` normalize Float keys and check `_status` like the call forms. The builtin method arity check applies whatever handlers the cell has (a builtin-named method always runs the builtin). The start-up audit names Map keys an older version wrote for a whole Float ("3.0"). remember/recall normalize keys; a Map<Int, V> slot takes the canonical text of a big Int; count_by/filter_by refuse a field no row has; join refuses a () element.

### A bare soma run starts the cell with the runtime section

With a helper cell declared above the cell that has `runtime { }` (or `interior`), a bare `soma run app.cell` ran the first cell and the runtime section never ran; it now starts the cell that has the runtime section, else one with an interior.

### verify seventh pass: interior cells keep their rules at run time, slot reads by name, same-named foreign handlers, builtin arities, interpolated literals, [deterministic] on hidden calls, geometric growth

An interior cell run by the runtime now has its memory invariants and its state machines (a violating write was accepted while `soma verify` proved the rule; `transition()` raised "has no state machine"), keeps its machine instances from one signal to the next, finds its siblings' slots when a `signal` runs them, and runs each of their handlers once (they ran twice). Interior handlers are edges of the termination call graph (A.ping → B.pong → A.ping was "✓ terminates") and are desugared for every proof. The invariant prover counts a slot passed whole (`len(c) + n`, `sum(c)`, `c |> …`) as a read, re-evaluates an accessor handler in a loop or a lambda, takes a handler named like a builtin but called with an arity it does not accept for the builtin (the builtin runs), gives `len()` of a literal with a brace only `>= 0` (`"{s}"`, `"{{}}"`), no longer treats `round(x, digits)` (a Float) or a handler named like a builtin as an Int, and treats a match without a catch-all as possibly `()`. Guard proofs keep another cell's handler that shares a name with one of the machine's cell (`Force.appr` beside `appr` was dropped) and no longer take a reassigned or shadowed Int parameter as NaN-free (`amount = sqrt(-1.0)`). check refuses a call of a builtin-named local that a later assignment in the same loop makes a lambda, and no longer calls a DATA local named like a builtin a local function (`let max = max(rows)` then `max(4, 5)`). `[deterministic]` sees calls in interpolation and method form (`"{random()}"`, `x.roll()`) and a data local named like the builtin (`let random = 0  random()`). A budget has no "✓ holds" when a loop-carried value is rebuilt from two or more loop-carried values (`s = "{s}{s}"`, `ys = xs  xs = concat(xs, ys)`, `a = concat(a, b)  b = concat(b, a)`).

### Response equality, Float keys everywhere, canonical Int text on Map<Int> slots, List get, check false positives, ttl/evict honesty

Two responses (response(), html(), redirect()) compare equal when their fields do — a response never equalled itself, so `assert request(…) == response(404, …)` failed with identical sides. A whole Float key is its Int in map(), group_by, agg, contains and without too (`map(3.0, "q")[3.0]` was ()), past 2^53 as well. A `Map<Int, V>` slot takes the canonical text of an Int as that Int on read and write ("9" is 9, as the docs say; "007" and "abc" are refused), so the keys of a local copy work against the slot again. A List slot refuses a non-Int index in get (it answered ()), and a local list honours `get(i, default)`. sum_by/avg_by/min_by/max_by refuse a field no row has; count_by counts what filter_by(rows, field, "==", v) keeps. check: no "catch-all" warning on a Bool match with both arms or an or-pattern sum-type match; the builtin method arity check applies unless THIS cell has a handler of that name, and to test rules too; merge is documented variadic. A slot marked [ttl] or [evict] gets a warning that the runtime does not expire or evict entries.

### serve: request parameters by name, refusals without values, closed connections; fix keeps return types

`on request(method, path, body, …)` binds its 4th / 5th parameters by NAME: `query` / `q` get the query, `headers` / `hdrs` the headers (with five: query then headers); any other name is refused by `soma check`, at `soma serve` start-up and by `soma run … request` — a lone `h: Map` received the query, so `?authorization=…` forged a header. A memory invariant that cannot be evaluated (or reads `status` and fails) answers a client with the slot name only, not the rule and the value it failed on; every runtime refusal text is whitelisted by shape (a guard loses its rule, a declared-source transition its instance id). `require … else "text {x}"`, `require … else Tag "detail"` and `fail(map("error", …, "kind", …))` are the program's own words and reach the client as written (a re-raised try-result of a runtime error does not). A refused framing (413, 501, the 400s) closes the connection instead of answering 400 to every later request on it for 60 s; those answers carry `Content-Type: application/json` and are logged, like the 403 Host/Origin refusals. `/static/` answers 404 for anything outside static/, existing or not (a 403 told which files exist); `/static/sub/./x.txt` is served. A form body whose percent-escapes are not UTF-8 is a 400 (`%E9=a&%EA=b` merged into one key). The Int-key audit flags `"007"` and `"-0"`. `soma fix` no longer panics on a non-ASCII character at a lex error, reports under `--json` the fixes it wrote before a parse error that remains, adds a missing handler to `cell agent X`, moves a handler's return type onto the face signal (a new `signal` line or face when needed; a conflicting face type is left and reported) instead of dropping it, and `--native-idiv --json` prints JSON. `soma run --fresh a.cell` keeps the remember() memory and next_id() counters of a cell another .cell file beside it declares. `test --json` and `verify --json` keep one schema whatever the outcome (`passed` is a count for test, a Bool equal to `ok` for verify, also on a load failure).

### native: one Rust name per binding, generated names apart, write_str's count, "}}" kept, every path returns, product cap on loop lets, BigInt bit_* arguments, CSE after effects, String copies, fast `for` in BigInt mode

A [native] handler gives the interpreted answer or is refused. Every parameter, `let` and loop variable now has its own Rust name: a tail call after `let acc = acc * 2` rebinds the parameter (it assigned the shadow — a wrong answer, or a loop that never ended for `let n = n - 1`), an inner-block `let x = 2.5` no longer retypes the outer Int `x`, `let t = t` in a loop compiles, and a user name can no longer collide with a generated one (a parameter `_i0` read the table's loop counter, a parameter `c` the memo closure's, a handler `f_compute` redefined `f`'s memo body, `total_par` replaced `total` in the FFI table). The native checks see the same scopes (an inner `let s = 1` hid the outer String `s` in `s + n`). `write_str` answers its byte count (0 natively); `"a}}b"` stays `a}}b` as interpreted (only a literal with `{` unescapes); a handler that can reach its end without `return` is refused at check (native answered 0 where the interpreter answers ()). The 2^24-bit Int cap is checked before each product, with the interpreter's message, also for a `let` product in a loop. `bit_test(x, k) == 1` in a BigInt condition checks k (-1 read bit 4294967295; k ≥ 64 of a small local panicked), and `bit_len` / `bit_test` / `bit_next` of a BigInt bound to a local no longer raise "an Int past 64 bits". A shared call is not hoisted above an effect of its statement (`write_str("A") + str_at(s, i)` raised before writing "A"). Copying a String local or parameter (`let b = a`, `let t = s`) compiles. In BigInt mode a `for` variable stays 64-bit (60-140x faster), and an index or range bound computed from a BigInt that fits 64 bits works (`buf_set(b, big % 10, v)`). hm_inc past 2^63 says so; the memo wrapper no longer counts twice against the 20000-call recursion limit; the concatenation-in-a-loop warning suggests strbuf in a [native] handler.

### Named handlers with interior, text keys on Int-keyed slots, sort_by keys, list +, -0.0, Float keys, check warnings

`soma run app.cell main` runs main in a program with an `interior` section (it dumped the runtime state; `total 2 3` went to a child's handler). A Map<Int, V> slot refuses a text key on read as on write (`qty.get("42")` answered () for an existing row). check warns on an indexed or field write to an outer local inside a lambda (`counts[w] = …` changed a copy), on a value match with no catch-all (it answers ()), and refuses a builtin method call with too many arguments (`n.max(1, 9)`, `s.split(",", 1)`). lowercase/uppercase/pad_left/pad_right refuse () (a missing field became "null"). sort_by refuses a field no row has, non-row items and Map/record keys (they came back unsorted); a List key orders element by element. `[10, 20] + [1, ()]` and a matrix plus a vector are refused instead of joined. `-z` of a Float is its negation, -0.0 for 0.0, interpreted and [native]. A whole Float key is its Int (`m[3.0]` and `m[3]` are one entry). count_by compares numbers as numbers. `refusal(kind)` answers the error text raising it would.

### verify: no proof through local functions, Float requires, get defaults, mod signs; [deterministic] is transitive; budgets and all_encrypted say what they do not cover

`soma check` refuses a local function named like a builtin or a handler (`let abs = v => v` then `vals.set(k, abs(x))` was proven `vals >= 0`; `let range = n => …` fooled the cost line, `abs => abs(abs)` the termination proof) and a local named like a state or variant (`let Rejected = "Approved"  transition(id, Rejected)` was read as the state Rejected). Plain data locals (`let len = len(rows)`, `let max = 3`) stay allowed. `require y < 10` tightens to `y <= 9` only when `y` is known to be an Int (a Float from `parse_float`, a map field, a list element or an if/match of Floats was "proven" `<= 9.0` at 9.5). `slot.get(k, default)` ranges over the default too; `mod(x, n)` takes the divisor's sign (`mod(7, -3) == -2` was proven `>= 0`). A local lambda is as stable as its body (`let f = v => random()`: the `require f(1) <= 0.5` said nothing about the written `f(1)`). Termination: `every` / `after` blocks get the self-application check and inherit a non-terminating callee, and a capitalized or builtin-named local called as a function (`let K = g  K(K)`) is a function value. `[deterministic]` now refuses slot reads, `transition` / `get_status` / `has_state`, file and network I/O, and calls of handlers (this cell's or `Other.h`) that do any of these; the atlas example's `draft_ack` (it calls transition()) lost the mark. The memory budget no longer says "✓ holds" when a loop rebuilds a value from two copies of itself (`xs = concat(xs, xs)` 24 times was 24 MiB, 5.9 GB at run time): it is an advisory naming the growth. `[encrypted]` is described as what it is — a mark, stored in plaintext by this runtime — and `promise all_encrypted` warns that it is not enforced.

### serve: chunked bodies capped, request headers by position, runtime errors whitelisted, stored responses, storage 500

A `Transfer-Encoding: chunked` body is read to 256 MB at most (it had no limit: only a declared Content-Length was checked) — past it, 413 and the connection is refused further requests. `request`'s 4th and 5th parameters are bound by position, query then headers, whatever their names (a 5th `hdrs: Map` received the QUERY, so `?authorization=…` forged a header); a lone 4th `headers` still gets the headers, and `headers` 4th of 5 or `query` 5th is refused at start-up; `soma run … request` binds the same way. A client's error body now carries runtime text only through a whitelist: `require`/guard/invariant refusals and `fail(kind, detail)` as the program wrote them, a parameter of the called handler that does not fit (it quotes only the client's input), a few value-free shapes (`cannot add String and Int`), and otherwise a fixed sentence per kind — `not valid JSON: <text>`, date, regex, `cannot read field 'x' of String "…"`, element-type and unknown-option messages quoted stored values past the old redaction. A `response()` / `html()` / `redirect()` map stored in a slot comes back a response (its mark was dropped: a cached idempotent reply answered 200 with `_status` in the JSON). A storage failure (a read-only database) answers 500, not 400. The start-up audit reports String keys left in a slot re-typed `Map<Int, …>`. `soma run --fresh` keeps `<Cell>__counters` / `__agent_memory` when another program in the directory uses the same cell. `soma check`'s warning for a handler named like a `request` route now says what serve does (the route owns `/<name>` and `/<name>/…`; the handler is not an endpoint), and the docs say precisely what a client cannot forge (values serve binds) and that `from_json` of client text can build a declared variant.

### [native]: BigInt mode, shared calls, caches and tables give the interpreted answer

In BigInt mode `!(m > 1)` is negated after the comparison (it answered 0, or failed to compile on a parameter), a swapped BigInt is no longer read after the enclosing `if`, a loop's `let x` no longer replaces an outer `x`, `let x = x + 1` in a loop and `s = s + s` compile, and a local kept as a 64-bit Int is one whose size is provably bounded (a sibling's result, `y * 2^32`, `x = x - (0 - x)` or `k = x  x = x + k` now promote to BigInt instead of raising a range error). A call is shared between two uses only when neither it nor any handler it reaches draws a random number or does I/O, the name was not re-bound with `let`, and it is not on the right of `&&` / `||`; a recursive handler with I/O or randomness is no longer cached; a bottom-up table is built only when every guard and value translates exactly (an `else`, `||`, `!`, `/`, a Float or a builtin call used to be dropped or mistranslated). `write_str` is flushed and written once when a handler re-runs in BigInt mode. String literals and regex patterns with `\r` or other control characters compile as written. `read_file` is refused in [native] handlers at check (its failure value is a Map). `hm_inc` / `hm_get` / `hm_has` accept a key computed from a BigInt, `hm_has` answers a Bool as documented, and `%` by zero, `pow_mod` with modulus 0 and a negative bit index raise the interpreter's messages.

### serve: no values in client errors, /signal/ oracle, _status checked on write, framing, CORS headers; test dir and init

An HTTP error answer no longer quotes the values a runtime error names (`cannot multiply Map and Int: {"pw_hash": …}` sent a stored record, `got String sk-live-…` a secret); the server log keeps them. `GET /signal/<name>` answers 405 only for a routable endpoint (a private, event-only or request-owned handler was confirmed), and a path `request` matches explicitly (`/signal/…`, `/static/…`) goes to request. A `_status` set after response() (`with(r, "_status", code)`, `r["_status"] = code`) is checked where it is written, before the writes commit (it answered 500 after them; a String was ignored). A Transfer-Encoding other than `chunked` is 501 (the body was dropped and the handler answered 200); percent-escapes that are not UTF-8 are 400 (`%E9` and `%EA` were one key). A preflight allows the header names the browser asks for. `emit` from `on ws` reaches [peers]. Path segments are coerced as query values are (`/typ/1.5` is a Float for `Any`). An explicit JSON Content-Type sends any JSON text as it is. `redirect` encodes spaces. `soma test <dir>` finds test cells by parsing (a test cell on the same line was skipped) and runs a file that does not parse (it was skipped with exit 0). `soma init` refuses `..`, absolute paths and names outside letters, digits, `-`, `_`, `.`.

### Invariants on List deletes and transitions, slot method arities, agent memory keys

A delete from a List slot re-checks a rule between slots at every shifted index (`a.delete(0)` kept `b <= a` broken). A `status` invariant is checked when transition() comes from another cell or a test, and on a `Map<Int, V>` slot keyed by the instance id; a guard reads the machine's own slots. Slot methods check their arity (`rows.push(1, 2)`, `m.set("a", 1, "b", 2)` dropped the extras), `m.get(k, default)` answers the default (slots and local maps), and `names.delete("x")` on a List slot is a type error. `rows.sum()`, `rows.max`, `rows.sort()` on a List slot call the builtin instead of answering (). check warns when `push` & co. are called on an element (`li[0].push(2)`), whose result is lost. Horde instances' memory keys no longer collide (`a` + `b/c` vs `a/b` + `c`); remember(()) is refused and recall(1) finds remember(1, v).

### Interpolation typos, loop scopes, exact durations, quoted record fields

`soma check` refuses a `{…}` segment that starts with a local or a known call but does not parse (`{total +}`, `{items[0}`, `{n, total}`, `{len(items}`) or carries a Python format spec (`{total:.2f}`, with a to_fixed/format hint) — each printed literally; JSON, CSS and prose braces stay text. A `let` inside a loop body is no longer visible before it on the next iteration (use-before-let there passed check), and a typo'd assignment in a loop gets the "creates a new variable" warning. Check warns when the right side of `??` is an unparenthesized arithmetic or comparison (`m.a ?? 0 + 1` is `m.a ?? (0 + 1)`), on `a // b` (a comment, not floor division) and on escapes like `\q` that keep their backslash. A bare `return` ends at the newline (it returned the next line's value). `for l in s` over "" runs 0 times and strips a CRLF `\r`. Duration literals are exact whole milliseconds (`1.15h` was 4139999, `1.005s` 1004; a fraction of a ms or a value past Int is refused). Records and variants quote their String fields when printed, keep their declared field order (keys/values/to_csv/entries), and answer () for `.len`/`.size`/`.keys` without such a field. Arity, record-shape, delegate, think-option, task, refusal and other check findings point at the offending call or literal instead of the handler's `on` line; a runtime error inside a multi-line expression points at the failing operation. Clearer messages for a nested write under a missing key, `x => return …`, `not ok`, `-()`; mixing an Int past the Float range with a Float raises `range` instead of answering inf.

### NaN, big Ints, records and incomplete arguments in builtins

min/max and quantile (so var_historical and expected_shortfall_historical) answer NaN for a NaN input instead of an infinity or a misplaced percentile. pow keeps an Int exponent's parity (`pow(-1, 2^53 + 1)` is -1.0) and refuses an Int base past the Float range; `round(Int, digits)` keeps the Int exact. render_each, word_count and par_word_count read records; to_csv/write_csv refuse a row that is neither a Map nor a record with fields (it was dropped). agg refuses a group field named like an output column. `s.split(d)` and `xs.get(i)` behave as split(s, d) and nth (negative index from the end, non-Int refused). type_of of a function is "Function", as documented. render/load_template and response/html refuse a key or header with no value; render takes one Map. concat refuses a List, Map or () mixed with another type; join needs a String separator. impact_sqrt refuses a volume ≤ 0, a negative σ or Y and a NaN quantity. to_json refuses a function; from_json passes a number or Bool through.

### Undeclared indexed writes, hoisted index names, static paths, query keys in opts

`ys[1] = 1` or `ys.x = 1` on a name never bound is a check error (it failed at run time with "undefined variable"). Verify hints quote a compound write's index as written (`bal[ks[pick(0)]]`) instead of a parser temporary. `soma check|test|verify <dir> --json` on a directory with no .cell file prints one JSON document. `soma serve` percent-decodes `/static/` paths (`a%20b.css`) and still refuses `%2e%2e`; a POST whose JSON body fills `opts` keeps the query's unknown keys too (the body wins a tie).

### verify: writes in index expressions, horde targets, sample_row, part-written locals, recursion in branches

`soma verify` no longer proves what can fail at run time. A slot write inside an index (`xs[bal.set(k, -5) ?? 0]`) is a write. A `horde(h, …)` task or callback that deletes the required key ends the "existing key" size proof. `sample_row`, `importance_sample_rows`, `svd_lowrank`, `regress_sgd`, `random_token` and a handler that draws a random number give no fact to a guard or an invariant. A `require cur.amount - n >= 0` stops holding once `cur.amount` is written. A callee that can end without a value returns (), and a callee's own `let` of a slot name hides that slot. A monotone counter whose increment calls a handler that writes it is runtime-checked. A lambda stored in a name judges its transition() where it is called. Termination rejects a self-call in a branch of an `if` that is not the base case and a decreasing local shadowed by a match arm or lambda parameter. think() options: the last key wins, a computed key may be any, and a `timeout` of 0 or less is not a bound (a negative http timeout no longer reduces the latency bound). A `[task]` handler's think() keeps what a `require` said about its parameters.

### Regex brace groups, CSV signs, text keys on Int-keyed slots, () map keys

`${{5}}` or `${{USD}}` in a regex replacement with no such group is kept as text (it was deleted), in interpreted and [native] handlers alike. from_csv keeps `+0612345678`, `-007` and `+5` as text instead of reading them as numbers without their sign or zeros. `contains(seats, "1")`, `seats |> contains("1")` and `without(seats, "1")` over a `Map<Int, V>` slot treat the text key as absent, as `seats.has("1")` does. `map(m.nope, 5)` is refused instead of storing the key "null".

### Records in pipeline builtins; `%` touches its operand; field names are Strings

sum_by, avg_by, min_by, max_by, pluck, group_by, count_by, agg, filter_by, sort_by, to_csv and the joins read typed records the same as maps (they returned 0, [] or the input unchanged). `select(rows, ["a", "b"])` takes a list of names. A non-String field (`sort_by(rows, 5)`) is refused instead of ignored. `5% - 0.01` is a percentage and `10%-3` a modulo: a percentage is a `%` touching its number with no operand right after.

### Fourth pass: `serve`, route ownership, `soma fix`, `check`

- The internal `_content_type` key (set with `with(html(x), "_content_type",
  t)` or `merge`) escaped the header check of the first pass: a CR/LF from
  the query added headers and a non-ASCII byte panicked the request thread.
  It is checked like `Content-Type`. `redirect("/café")` sent a 302 without
  `Location`; its non-ASCII bytes are percent-encoded.
- Route ownership: `delegate(who, "wipe", k)` (a computed cell, a literal
  handler) made nothing request's, and with several computed delegates only
  the last one counted — `POST /wipe/a` ran around `request`'s auth.
- `soma fix` located the slot of a contradictory property by a character
  offset that earlier edits (`None` → `()`) had shifted, and edited another
  cell's slot; it now searches the error's cell. Two `on ping` handlers
  are left to the author (the fix deleted a face `signal ping` line), and a
  `}` in a comment or a `"""…"""` string no longer ends a cell.
- `on request(method: String, path: String)` passed check and every request
  failed ("expected 2 arguments, got 3"): it receives the arguments it
  declares. `ledger[k] = 0` on another cell's slot passed check (`.set` was
  refused); it is refused too.

### Fourth pass: native regressions of the return-type and exactness rework

- Since "every return types the handler", a self- or mutually recursive
  Int handler read its sibling's placeholder return type (Float) and stayed
  Float: `fact(20)` raised "an exact Int … went past 2^53" (no division),
  and an `even`/`odd` Bool pair failed in rustc. The placeholder now starts
  at Int and only rises.
- An exact quotient handed to an Int parameter of a sibling was refused
  (`sq(n / 2)` for 8 is `sq(4)` = 16); only a Float that is no exact Int is.
- `abs`, `min`, `max`, `to_int`, `floor`, `round` of an exact quotient past
  2^53, and a comparison of two exact Ints, answered lost digits silently
  (`abs(q * q) % 1000` gave 0 for 1; `q * q == 10000000200000000` took the
  wrong branch). Only a Float function's arguments and a comparison with a
  Float are Float operands; elsewhere the value past 2^53 raises.
- `to_int(to_string(n))` (a String from a builtin) passed check and failed
  in rustc; it is refused like a String parameter.

### Third pass: native code

- "An Int spot never truncates" covered the 64-bit path only: once a
  handler re-ran in BigInt mode, a Float in an Int position was cut
  (`s + sq(2.5)` computed `sq(2)`, `shr(s, 3.5)` shifted by 3). Every
  conversion goes through the checked one, and a Float handed to an Int
  parameter of a sibling is refused as interpreted ("an Int parameter got
  the Float 3.0" — an integral one passed natively).
- A Float-typed handler returning an Int past 2^53 on one path rounded it
  (`100000000000000001` came back `1e17`); it raises, as an exact quotient
  past 2^53 does.
- `pos(n) + 1` with a Bool-returning sibling, `st(n) + n` with a String one,
  and `str_eq(…) + 1` passed check (a wrong answer or a rustc error): a
  sibling's return kind now counts in those refusals.
- False refusal: an Int local given `to_int(x * 100.0)`, `round(x * 10.0)` or
  `floor(sqrt(x))` was refused as receiving a Float.
- A quotient local reassigned a Float on a path not taken (`if n > 100 { q =
  q + 0.5 }`) printed its exact value as a Float on every path; exactness is
  decided when each definition runs.
- The 2^53 check raised where a quotient is only an operand of a Float
  (`q * 10^12 * 1.5`, a comparison, `sqrt(…)`); it applies where the exact
  Int itself is kept.

### Third pass: core evaluation

- The `let` restoration of the second pass missed match arms and
  multi-statement `try` blocks (a try block is a one-arm match): `match 1 {
  1 -> { x = 5  let x = 6 } … }` left x at 1, and a `let` inside an arm wrote
  into the enclosing block's record, losing a later assignment there
  (`total = total + i` in the loop body).
- An index holding an interpolated call (`stock["sku{pick()}"] -= 1`) was
  still evaluated twice (sku0 received sku1 - 1); an interpolated key is
  evaluated once, like any index that calls.
- A `()` key through `m = with(m, k, v)` and `m = m |> with(k, v)` (the
  in-place forms) still wrote the entry "null"; `without(m, ())` deleted it
  and `contains(m, ())` found it. All refuse or answer absent, as a slot does.
- A `Map<Int, V>` slot answered `get("1")`, `has("1")` and `delete("1")` for
  the Int key 1 (and the delete removed it): a key of another type is absent
  for reads and refused for a delete.

### Third pass: `verify` and `check`

- `delegate("{who}", "drain", k)`: the analyses read the interpolated cell
  name as a fixed (missing) cell, so an invariant, termination, a latency
  and a token bound were "proven" while the call ran the real handler, and
  a GET served it. `check` refuses an interpolated cell name (as it refused
  an interpolated handler name); `delegate(who, …)` is a computed target
  every analysis already treats as such.
- `invariant size <= N` was proven by "the key already exists" across a
  `think()` of a `[task]` handler, while other requests may delete it then.
- An http options map naming `timeout` twice was read at its first value;
  the runtime uses the last. `link()` counted 0 ms of latency (it waits on a
  connection); the bound is advisory.
- A local named `_from` / `_to` / `_id` proved a guard on the machine's own
  `_from` (`let _from = "pending"  require _from != "paid"`).
- False positive: a guard variable bound in the branch that takes the
  transition (`else { let reason = …  transition(id, "filed", "approved") }`)
  was refused as bound "only inside a branch".
- A returned `()` element (`[1, ()]` for `-> List<Int>`) passed check.

### Third pass: parser and builtins

- The new line rules had two gaps: `let k = m.len` then `(n * 2)` on the
  next line still called `len` (the handler answered `()`), and inside
  parentheses or brackets a line starting with `-2` (`(a\n -2)`) was refused
  although no statement can start there — it continues the expression again.
- A percentage literal was its raw number: `5%` was 5.0, so `p * (1 - 5%)`
  answered `-4p`; it is 0.05. It may also start a statement, and a keyword
  after it (`let d = 5%  return …`) is no modulo operand.
- `to_fixed(x, d)` past 15 digits printed the binary expansion
  (`to_fixed(1.005, 16)` was "1.0049999999999999"); every width rounds on the
  decimal text, as documented.
- `regex_replace` deleted `$USD` (an unknown group name) and read `$y_x` as a
  group named "y_x"; `$name` is the longest prefix naming a group, else text
  (interpreted and native).
- `format("%d", 10 / 4)` printed "2": a Float with a fraction is refused (a
  whole Float prints its digits). `format("%e", n)` of an Int past the Float
  range printed "inf"; it prints the exact leading digits.
- `matrix("1e400 1")` held `inf`, and a CLI Int past the Float range passed to
  a Float parameter became `inf`; both are refused. A CLI Float parameter
  accepts `1_000` as an Int one does.
- Parse errors no longer name Rust tokens (`DurationLit(5.0, S)`, `DotDot`).

### Third pass: `soma serve` binding, gates and `soma fix`

- An unknown query key took the body's place: `PUT /put_doc/k1?v=2` with a
  JSON body stored `{"v": 2}`, and `?utm_source=mail` replaced a text body.
  Parameters now bind in this order: the query's names, the body's names,
  the body whole (when none of its keys names a parameter — as text for a
  String parameter, so `on hook(payload: String)` receives a JSON body), a
  trailing `opts: Map` from the keys that name no parameter (the body's
  too: `{"n": 1, "step": 7}` for `add(n, opts)`), then the query's unnamed
  values. A missing trailing Map is empty (`{"name": "x"}` for `tag(name,
  meta: Map)` was a 400 "the request body must be JSON"), and a body sent to
  a handler with no open parameter is ignored (it was an arity 400).
- `horde(App.reset_one, …)` — the documented `Cell.handler` spelling — did
  not make `request` own `reset_one` (reachable directly, around its auth),
  and `horde_cancel` did not count as a write for the GET gate (a GET, e.g.
  an `<img>` on any site, cancelled a running horde).
- `soma fix` removed a contradictory property from a slot of the same name
  in ANOTHER cell, and put a handler stub into the cell a comment named
  (`// Helper is used by cell App`); it now edits the declaration the error
  points at and skips comments and strings.
- `soma test <dir> --json` wrote nothing on stdout when no file has a test
  cell; `redirect(url)` with a CR/LF sent a 302 without `Location` (it
  raises); the undefined-name advice of `check --json` no longer speaks of
  braces for code.

### Second pass: tools and diagnostics

- `soma fix` searched names as substrings: removing a contradictory property
  of the slot `log` edited `catalog` (which silently lost `ephemeral`), and
  a handler stub for `cell App` went into `cell AppStore` (one more copy per
  run). Names are matched as whole words, a slot by its declaration, and a
  property exactly.
- `soma check` accepted two `on ping()` in one cell, of which the runtime
  keeps the last one ("last definition wins" was printed only by `soma
  run`); it is an error.
- A local lambda calling itself (`let fib = n => … fib(n - 1) …`) or a lambda
  bound after it passed `check` and raised "undefined function" at run time:
  a lambda captures the locals bound before it. `check` says so and points
  at a handler for recursion.
- `soma verify <dir>` answered "cannot read '.': Is a directory"; it runs
  over the directory like `check` and `test` (with `--strict` and `--json`).
- `soma run app.cell h -0` passed the Float -0.0; `-0` is the text "-0", as
  over HTTP (a Float is written with a point or an exponent).
- Messages: a parse error named the Rust token `PercentLit(50.0)`;
  `round(x, 10^20)` said "digits must be ≥ 0"; `format_date(1.5e9)` asked
  for a String only (it takes an Int of Unix seconds too).

### Second pass: parser and builtins

- A line starting with `-` touching its operand (`-amount`) continued the
  line before: `let fee = 2` then `-amount` as the value of a block read
  `2 - amount` (the block answered `()`). It starts a new expression;
  `- amount` with a space still continues the line.
- `filter_by(rows, "price", ">", 100)` compared numeric text as text ("99"
  passed) and kept `()` and `true` values ("null" > "5"). Against a number,
  numeric text compares as a number and a value that is no number passes no
  ordering; two Strings still compare as text (ISO dates).
- `min_by` / `max_by` skipped a value that is no number (`"n/a"`) where `agg`
  refuses it; they refuse it too. `distinct`, `pluck`, `group_by`, `select`,
  `agg`, `count_by`, `avg_by`, `min_by`, `max_by` answered an empty result
  for a Map or a String, and `join` joined the text of a non-list; they are
  refused (`()` still gives the empty result).
- `regex_replace(s, p, "doc_$1_v2")` dropped the group (`$1_v2` read as a
  group named "1_v2"); `$N` is group N whatever follows, and `$5` with no
  such group is the text "$5" (interpreted and native).
- `sqrt`, `log` and `log10` of an Int past the Float range answered `inf`
  (`sqrt(10^400)` is 1e200); `to_float("1e400")` was `inf` and is refused as
  the literal is.
- A triple-quoted string in a CRLF file kept a leading newline and a
  trailing "\r".
- `scale(m, "3")` zeroed the matrix; `shape` of a ragged matrix answered the
  first row's width. Both are refused.

### A parameter bound by name keeps its position

- `soma serve` compacted the parameters a query bound by name before the
  body was bound: `POST /two?b=B` with `{"a":"A"}` bound "B" to `a` and the
  whole body to `b`; `POST /pay?amount=5` with `{"to": …, "memo": …}` put 5
  into `to`. The query's names, then the body's names, then the unnamed
  values fill their own parameters; a missing parameter before a bound one
  is reported by name (`GET /two?b=B`: "parameter 'a' expects String")
  instead of "expected 2 arguments, got 1".

### Second pass: `verify` and `check` no longer prove what a run violates

- An invariant was proven for a write inside a loop from one `require`
  before it (`require (c.get(k) ?? 0) - n >= 0` then three
  `c.set(k, (c.get(k) ?? 0) - n)`): a fact over state holds for the first
  pass only. A bare call resolving to another cell's handler
  (`sabotage(k)` → `B.sabotage`), and `Other.h()` / `delegate` inside a
  called handler, now count as effects that may change what was read.
- Termination: `emit ping(…)` inside `on ping` reaches every other cell's
  `on ping` (the call graph, by name, read it as a self-call): a listener
  that may not terminate, or that emits it back, now makes the emitter
  unproven. A recursive call inside the base case's own condition
  (`if f(n - 1) > 100 || n <= 0 { return 0 }`) no longer passes as a base
  case.
- Latency: `http_patch` took no time, and the timeout of `http_post` /
  `http_put` was read from the body (their options come third). An options
  map with a computed timeout makes the bound advisory.
- Cost: `emit ping(…)` inside `on ping` counted the handler twice (a false
  "budget exceeded").
- Memory budget: the slot sizes it uses (the `[capacity]` /
  `[max_*_bytes]` annotations, else defaults) are assumptions nothing
  enforces at run time; the line said "budget proven". It now says "budget
  holds … while its slots stay within their assumed sizes" and names the
  assumption (the verdict is unchanged).

### Second pass: native code answers like the interpreter

- The return type of a `[native]` handler came from its FIRST `return`:
  `if x < 0.0 { return 0 }  return x` was typed Int, and since native code
  no longer truncates, `clamp0(1.5)` raised (it answered 1 before);
  `if n == 0 { return 0 }  return n / 2` raised on 5. Every return counts now
  (a Float anywhere makes the handler Float), and each return reports
  whether it is an Int: `clamp0(-2.0)` is `0`, not `0.0`, as interpreted.
  `abs`, `min` and `max` of a quotient keep its exactness (`abs(n / 2)` was
  `4.0`).
- An inexact `a / b` with an operand past 2^53 was rounded twice natively
  (fib(90) / fib(89) one ulp low); it is rounded once, as interpreted.
- A BigInt local handed to a sibling `[native]` handler went through i64 and
  raised "an Int past 64 bits … bind it with let" although it was bound; a
  sibling with a BigInt variant gets the Integer, and a 64-bit-only sibling
  makes the call run again interpreted.
- `soma check` refuses what native code answered differently or failed to
  compile: a handler ending in an `if` whose branches end in values (it
  returned 0.0), `String + number` through a number-returning builtin
  (`s + str_len(s)` gave "abc3"), comparisons between a Bool, a number and a
  String (`b == n` was true for (true, 1)), and `to_int` / `to_float` of a
  Bool (a rustc error).

### Second pass: a handler passed as a value is a callee

- `horde(worker, xs)` and `vote(worker, q, k)` run the handler they are
  given, but only a called name counted: `GET /wipe_all/a,b` ran a writing
  `worker` (the GET gate answers 405 for a writer), and a handler that
  `request` reached only through `horde(reset_one, [k])` was callable
  directly at `POST /reset_one/a`, around `request`'s auth check. A handler
  name passed as an argument is now a callee for both, and `horde` / `vote`
  count as effects (a guard or an invariant may not call them).
- A `request` declaring `body: List` answered every body-less request (every
  GET) with a 400: an empty body is `[]` for a List.
- The CORS preflight and the 405 `Allow` header left out PATCH (and PUT,
  DELETE for `Allow`), the methods that carry a body.

### Second pass: core evaluation

- An index expression that calls something was evaluated two or three
  times by a nested or compound assignment: `grid[next()][1] = 5` copied
  another row over row 0, and `xs[next()] += 1` wrote `xs[0] = xs[1] + 1`
  (a List slot too). Such an index is evaluated once.
- A `return` ending a branch of an if-expression (`let found = if
  len(xs) == 0 { return "empty" } else { xs[0] }`) became the value of the
  `if` instead of returning.
- An assignment to an outer variable was undone when the same block later
  declared a `let` of that name (`if c { x = 5  let x = 6 }` left x at 1): a
  block now restores the value its `let` shadowed when that `let` ran.
- A negative index failed below the first level of an assignment
  (`g[0][-1] = 8`, `p.pts[-1] = 9`); `with(list, -1, v)` counts from the end.
- A lambda did not capture a name read by an interpolated match pattern
  (`map(v => match v { "user-{id}" -> … })` raised "undefined variable: id").
- A record pattern (`Game { bet }` for `Game { bet: 10 }`) was refused as
  soon as the file declared any `cell type`; it is refused only when it
  looks like a mistyped variant.
- A `()` key on a local map wrote and read the entry "null"
  (`mm[body.userid] = v` with a mistyped field); it is refused for writes
  and absent for reads, as on a slot.

### Second pass: a `Map<Int, V>` slot reads the same through every spelling

- The Int-keys fix covered `seats.keys` and `seats.entries` only: `values`
  (and `list`) stayed in text order, so `zip(seats.keys, seats.values)`
  paired 2 with "ten"; `keys(seats)`, `entries(seats)`, `seats |> keys` and
  `for e in seats` answered text keys in text order; and a `cell test` rule
  read the slot with text keys and could write any key. Every spelling now
  answers Int keys in numeric order, with values in the same order.

### `soma test --json` keeps a rule with a dash and says what raised

- The JSON records are read back from the report text: a rule holding a
  string with " — " (`assert s() == "a — b"`) was cut inside the string, and
  an `assert_fails` that did not raise was recorded `"raised": true`.

### `lint` and `fix` advice keeps the program working

- `soma lint` advised replacing `r.length` with `r.len`: without
  parentheses `r.length` reads the key "length" of a map (5) and `r.len` is
  its entry count (2). The alias advice is kept where it holds: calls, and
  a memory slot (`items.length` is its entry count).
- The if-chain advice ("consider using match") fired for ifs with `else`
  branches, which run for every other value and add up; no `match` gives
  that answer.
- `soma fix` wrote `return map("status", "ok")` for a face declaring
  `-> Map<String, Int>` or a sum type, then reported "All checks passed"
  while every call failed. A Map gets `map()`, a sum type one of its bare
  variants (or a `fail("not_implemented", …)` stub when it has none).
- `soma check` refused a returned literal of the wrong kind but not its
  elements: `map("status", "ok")` for `-> Map<String, Int>` and `[1, "x"]`
  for `-> List<Int>` passed and failed at run time.

### `soma serve` binds every request body and keeps a `+` in a path

- A request body was bound to a handler only for an exact `POST`: `PUT`,
  `PATCH` and `DELETE` (which the write gate accepts), a lowercase `post`, and
  every `/signal/<handler>` route dropped it — `on save(data: Map)` stored
  `{}` and answered 200. They bind it as POST does.
- A body with an `=` was parsed as a form whatever its `Content-Type`: a
  `text/plain` "aGVsbG8=" reached a String parameter as the map
  `{"aGVsbG8": ""}` (a 400). A body is a form when it says so, or says
  nothing.
- `request` received `/a+b` as `"/a b"`: a `+` in a path is a plus sign
  (the automatic routes already kept it).
- The 404 of an unknown `/signal/` route was built without escaping (a `"`
  in the URL made it invalid JSON) and had no `kind`; it is `not_found`.

### Small builtin, literal and diagnostic fixes

- `round(x, digits)` capped `digits` at 15: `round(1e-16, 16)` was 0.0.
- `to_fixed` of an Int went through f64: `to_fixed(9007199254740993, 0)` was
  "9007199254740992" (`format("%.2f", …)` was already exact).
- `-2.abs()` failed to parse (the negative literal left `.abs()` behind); it
  is `-(2.abs())`.
- A float literal past the Float range (`1e400`) was silently `inf`; it is a
  lexing error, as `from_json("1E400")` raises.
- `format("%08.2f", inf)` padded with zeros ("00000inf"); inf and NaN pad
  with spaces, as in C.
- A builtin called with too many arguments was reported at the header of
  the handler, and only its first occurrence in the program; every call is
  now reported at its own line.

### Integer text is digits; a decimal string truncates exactly

- `to_int`, the aggregates (`sum_by`, `agg`, `filter_by`…), a `soma run`
  argument and an HTTP parameter read integers with a parser that skipped
  spaces inside the digits: `to_int("1 2 3")` was 123, `sum_by` summed
  `"1 2"` as 12, `GET /echo_int?n=1+2` answered 13. Only digits count now,
  with `_` between two digits as in a literal (`1_000`); `to_float` reads
  the same separators.
- `to_int` of a decimal string went through f64: `to_int("9223372036854775807.5")`
  was …808 and `to_int("99999999999999999999.9")` 10^20. The digits are now
  truncated exactly, and surrounding whitespace is allowed as for an Int
  (`to_int(" 12.5 ")` was `()`).
- An HTTP query value past i64 that prints like a Float (`?n=100000000000000000000`)
  became a Float: an Int parameter refused it and a String parameter got
  `"100000000000000000000.0"`. It is an Int.

### Interpolation reads quoted braces and semicolons as code

- `"parts: {join(parts, \"}\")}"` printed its own source: the segment ended
  at the quoted `}`. A `;` inside a quoted argument (`{join(xs, \";\")}`)
  made the segment literal text (CSS is still text: its `;` is not quoted).
  Both passed `check`.
- A string pattern holding `}}` did not match the same literal: `match s {
  "a}}" -> … }` for `s = "a}}"` fell through, since a pattern was
  interpolated on a `}` alone and an expression only on a `{`. Both now
  interpolate on a `{`.

### `10%-3` is the modulo

- A number touching `%` lexed as a percentage literal whatever followed:
  `10%-3` was `10.0 - 3` = 7.0 (the modulo is 1) and passed `check`,
  `100%+1` was 101.0, and `10%3` / `10%n` failed to parse. `N%` is a
  percentage only where no operand follows it (`[threshold(50%)]`).

### A line starting with `[` or `(` is a new expression

- "Newlines separate statements", but a line starting with `[` indexed the
  value that ended the line before: `let xs = [10, 20, 30]` then `[i]`
  parsed as `[10, 20, 30][i]` (the handler answered `()` instead of `[1]`,
  after a clean check), and `let b = a` then `(1 + 2)` called `a`. An index
  or a call now needs its `[` or `(` on the same line; a `.method` chain
  continued on the next line is unchanged. No program of the repository
  changes its `check` result.

### A triple-quoted string with a non-breaking space no longer panics

- The dedent of a `"""…"""` string measured indentation with Unicode
  whitespace and cut it in bytes: a pasted U+00A0 (or U+3000) at the start
  of a line panicked `soma check` ("byte index 5 is not a char boundary").
  Indentation is spaces and tabs; any other character is content.

### `check` counts a `think()` in an index, an assignment in a `require`, and `delegate`'s arguments

- The cost proof never looked inside an index: `labels[to_int(think(…,
  map("max_tokens", 100)))]` beside another 100-token `think()` was "proven"
  under a 100-token bound while both calls ran.
- An assignment inside a `require` condition (`require len(try { v = n }) >
  0 else Never`) was not a reassignment for the invariant prover, which
  proved `c.set(k, v)` with `v = 5`.
- `delegate("A", "h", 1)` for `on h(x: Int, y: Int)` passed `check` and
  raised "expected 2 arguments" at run time; a literal delegate is now
  checked for its argument count as a direct call is.

### `verify` no longer proves an invariant or a guard through a value that changed

- Invariants: a callee's `return` inside a match arm, a `try` block or an
  if-expression was not one of its values (`c.set(k, score(n))` proven
  `c >= 0` while `score(1)` returned -5); and a `require` over an expression
  was taken to hold at the write even when that expression reads a slot the
  handler rewrites in between (directly or through a called handler), or
  calls `random()`, `now()`, `think()` or the network. Such a fact now counts
  only when the expression gives the same value at the write.
- Transition guards: a `require` was taken to hold at the transition after a
  field write (`cap.max = 0`), a slot write, a handler call or a `sleep`; a
  guard over `now()` or `random()` was "proven"; `require !(x < 0)` proved
  `x >= 0` for a `sqrt` that is NaN; `require code == "5"` proved
  `code == 5`; `require amount > 9007199254740992` proved `amount >
  9007199254740993` (one f64); an assignment inside an `if` condition and a
  transition() inside a `for` iterator or `while` condition were not seen;
  and a guard read "proven — every caller narrows it" beside a handler that
  transitions to a computed target. All of these are now runtime-checked
  (⚠). No program of the repository changes verdict.

### A `Map<Int, V>` slot keeps Int keys

- The key type of a Map slot was ignored: `Map<Int, String>` answered its
  keys as Strings sorted as text (`["10", "2"]`, so `max(seats.keys)` and
  `t + k` raised), stored `seats.set("not-a-number", …)`, and `check` refused
  `invariant key >= 1` as comparing a String key. Its `keys` and `entries`
  are now Ints in numeric order, a key of another type is refused (kind
  `type`), and the invariant binds `key` as an Int.

### An Int in a nested Float position becomes a Float

- "An Int written to a Float slot becomes a Float" held at the top level
  only: `Map<String, List<Float>>` stored `[1, 2]` as Ints (`type_of` said
  "Int", `to_json` gave `[1,2]`), and so did `Map<String, Map<String,
  Float>>`. The declared type now promotes at every depth.

### A typed machine state that carries fields is refused

- `state s: St` where `St` declares `Funded(Int)` passed `check` and `verify`,
  then `transition(id, Funded(n))` raised "invalid transition: Pending →
  Funded(5)" on every call (and `transition(id, Funded)` "takes payload"). A
  machine state is a name — `get_status` answers `"Funded"` — so `check`
  now refuses such a state and a transition target built with fields, and
  says to keep the data in a slot keyed by the instance id.

### A `()` key reads nothing and deletes nothing

- `set((), v)` is refused (an absent value, often a mistyped field, is no
  key), but `get(())`, `has(())` and `slot[()]` read the entry `"null"`, and
  `users.delete(body.userid)` deleted it. A `()` key now reads as absent
  (`()`, `false`) and `delete(())` is refused like `set((), v)`.

### Native BigInt mode: a product of two small locals promotes

- `let x = 3037000500  return x * x` failed natively with "an Int past 64
  bits inside a Float, Bool or index expression": the BigInt fallback
  multiplied its two i64 locals without an overflow check. The i64 fast
  path now promotes on overflow.
- A range bound or index past 64 bits leaked Rust's "called `Option::unwrap()`
  on a `None` value"; the native messages of `range`, `asin`/`acos` and
  `pow_mod` read as the interpreter's.

### Native code: each quotient keeps its own exactness, and an Int spot never truncates

- Whether an Int quotient carried as a Float was exact was one flag for the
  whole call: after any inexact division (`let mean = total / n`), an exact
  `let h = n / 2` returned `2.0` and printed "2.0". Each quotient local now
  records its own exactness, a `return` reports only its own, and a sibling
  `[native]` handler returning a quotient passes its exactness to the caller
  (`to_string(half(8))` was "4.0", `half(8) * 2` was `8.0`).
- An exact quotient used in further arithmetic lost digits past 2^53 without
  a word: `let q = x / y  return q * q % 1000` answered 0 for 100000001 / 1
  (the interpreter answers 1). It now raises, as an exact quotient past 2^53
  already did, pointing at `idiv(a, b)`.
- A Float value in a spot that holds an Int (`s = s + q` with an Int `s`)
  was truncated: 10.5 became 9. It raises now, as documented ("it never
  truncates"); an integral value converts as before.

### Native code: Float locals print as Floats; mixed Bool returns and `String + number` are refused

- `to_string` of a Float local printed an integral value as an Int: every
  local of type Float counted as a possible exact quotient, so a sum of
  `0.5`s printed "2", `-0.0` printed "0" and a copy of a Float parameter "3".
  Only a local whose every definition is an Int quotient (`let q = a / b`)
  prints as an Int when exact, as interpreted.
- A `[native]` handler returning a Bool on one path and a number on another
  (`if x < 0 { return false }  return x * 2`) passed check and answered
  `true` for 5; it is refused like a String/number mix.
- `"n=" + n` in a `[native]` handler joined the text where the interpreter
  raises "cannot add String and Int" (and `"n=" + (n + 1)` past 64 bits joined
  the digits of both operands); it is refused with the fix, `to_string(n)`.
- Assigning a `for` loop variable in a `[native]` handler (`i = i + 2`)
  passed check and failed in rustc (E0384).
- Diagnostics printed `a + (b + c)` as `a + b + c`: a right operand of the
  same precedence keeps its parentheses (`"n=" + (n + 1)` is not
  `("n=" + n) + 1`).

### A failing `try` in a test rule rolls back

- In a `cell test` rule, `try { dep("x", 5)  transition("x", "frozen")
  fail("boom", "") }` kept the write and the transition: each handler the
  block called committed on its own. The same block in a handler is undone.
  A `try` outside any handler is now one unit, rolled back when it fails, so
  a test sees what production does (a `[task]` handler it calls still commits
  the steps before each `think()`, as a task entry does).

### `id.transition(…)` evaluates its guard over the caller's locals

- The method form of `transition` never handed the calling handler's locals
  to the guard: `id.transition("approved")` under `guard { amount < 1000 }`
  failed with "undefined variable: amount" — or, after an invalid
  `transition()` earlier in the same run, read the locals that call had left
  behind and approved an order of 5000. Both forms now evaluate the guard
  over the caller's locals, and an invalid move no longer leaves them behind.

### A slot name starting with `__` is refused

- A slot named `__sm_<Cell>_<machine>` shared the store of the state machine:
  `__sm_Order_order.set(id, "shipped")` moved an instance with no
  `transition()`, so no edge, guard or `status` rule was checked, and the
  properties `verify` had proven no longer held. `soma check` now reserves
  the `__` prefix for the runtime's own storage.

### A `Content-Type` value is checked like any response header

- `response(200, body, "Content-Type", v)` sent `v` unchecked: a CR/LF in it
  (a value taken from the query) added headers of the client's choosing
  (`Set-Cookie`), and a non-ASCII byte panicked the request thread (500, empty
  body). Such a value is now dropped with a warning, as every other header
  value is; a non-ASCII value of another header is named in that warning
  instead of vanishing silently.

### A handler that writes through `slot.put(k, v)` is not run on GET

- `put` is `set`, but the GET gate of `soma serve` did not count it as a
  write: `GET /wipe/alice` ran `bal.put(k, 0)` (a link or an image tag could
  trigger it). It now answers 405 like `set`.

### `[deterministic]` refuses every storage write

- Only `.set()`, `.delete()`, `.append()`, `.push()` and `.clear()` were
  refused: `slot.put(k, v)`, `slot.remove(k)` and `slot[k] = v` passed the
  contract, and a call inside a `require` condition (`require now() > 0`) was
  never looked at. The whole body is now walked, blocks included.

### A variant pattern on `get_status` is refused

- `get_status(id) == Funded` was already refused (the state is a String),
  but `match get_status(id) { Pending -> … }` passed `soma check` and raised
  "match expects a variant … got String" on every call. It is now refused
  the same way, pointing at `"Pending" -> …`.

### `format("%.3d", 7)` is "007"

- The precision of a `%d` directive was parsed and dropped, so `%.3d`
  printed `7`. It is now a minimum number of digits, as in C (`-007` for a
  negative value; a width still pads around it).

### `soma test <dir>` passes a multi-file project, and `--json` is one document

- Over a directory, `soma test` counted a file with no test cell — a
  `lib/helpers.cell` library the entry file `use`s — as a failed file, so
  every multi-file project failed `soma test .` (exit 1) while `soma test
  app.cell` passed. Such files are now skipped and named; a directory with
  no test cell at all still fails.
- `--json` over a directory wrote one object per file back to back, which no
  JSON reader accepts. It now writes one document: `{command, files: [{file,
  ok, report}], total, failed, ok}` (for `check` and `verify` too).

### `mock approve` takes a Bool

- `mock approve "false"` approved: the scripted answer went through
  truthiness, and a non-empty String is truthy. An answer is now `true` or
  `false` (or a List of them); anything else is reported on the mock line.

### `soma lint` keeps the JSON around a String slot

- `redundant_to_json` / `redundant_from_json` advised dropping `to_json` and
  `from_json` around any slot. For a `Map<String, String>` slot they are
  what keeps the value text: following the advice made the write a type
  error and the read a String, and the program broke. The advice is now
  given only for slots whose values are not declared String.

### Each test cell starts with no hordes

- "Each `cell test` starts with fresh slots, machines, mocks and trace()",
  but not fresh hordes: a horde started in one test cell, with its results,
  was visible to the next, whose first horde was numbered after it
  (`Audit:h2`). The horde tables are now reset with the rest.

### A record pattern matches a record

- `match v { Game { bet } -> … }` matched only a struct variant named
  `Game`: a record built as `Game { bet: 10 }` — a Map carrying `_type` —
  always fell through, though the design shares the syntax. A record now
  matches when its `_type` is the name, with the fields read like a struct
  variant's (all of them, or some with `..`).

### An or-pattern binds only what every alternative binds

- `match m { Up(n) || Wait -> n … }` passed `soma check`, and `Wait` then
  raised "undefined variable: n": the checker took the names bound by ANY
  alternative. A name is now in scope only when every alternative binds it
  (`Up(n) || Down(n) -> n` is unchanged).

### A string pattern is interpolated like any string

- `v == "user-{id}"` was true, but `match v { "user-{id}" -> … }` compared
  the raw text, braces included, and fell through; `"{{x}}"` was not
  unescaped either, a `"{base}/" + rest` prefix pattern the same, and
  `"{nosuch}"` passed `soma check`. Pattern strings are now interpolated in
  the enclosing scope, and checked like any other interpolation.

### A literal arm does not make a variant `match` exhaustive

- Any arm that was not a variant pattern counted as a catch-all: `match l {
  Red -> …  "Yellow" -> … }` or `Red -> …  () -> …` passed `soma check`, and
  `Green` then raised "non-exhaustive match" at run time. Only `_` and a
  bare binding catch every value now; a literal, `()`, a range or a prefix
  arm covers no variant.

### An invariant is not proven through a Float modulo, or a NaN into an untyped slot

- `x % 3` was bounded by 2 whatever `x` held, so `invariant c <= 2` was
  proven for `c.set("r", x % 3)` with `x: Float`, and `rem(2.5)` was then
  rejected at run time. For a dividend that may be a Float, the bound is
  now below 3, not 2; an Int dividend keeps `n - 1`.
- The NaN guard (NaN fails every comparison) applied only to slots typed
  `Float`: `abs(sqrt(x))` written into a `Map` slot with no value type was
  proven `>= 0`, and `mag(-1.0)` was rejected. It now applies to every slot
  that may hold a Float, `Map`, `Any` and `List` included.
- The repository's invariant verdicts are unchanged.

### Termination needs one measure, and a base case at the halving fixed point

- A recursive handler was proven terminating when each self-call decreased
  some argument on its own: `walk(n - 1, m + 1)` beside `walk(n + 1, m - 1)`
  passed `verify --strict`, and `walk(5, 5)` cycles (5, 5) → (6, 4) → (5, 5)
  into a stack overflow. One parameter must now decrease in every self-call.
- Halving was accepted with any lower-bound base case: `log2(idiv(n, 2))`
  with `if n < 0` passed, and `log2(0)` recursed forever since `idiv(0, 2)`
  is 0. The base case must now catch the fixed point: 0 for `idiv` and `/`
  (`n == 0` is enough), 0 and -1 for `floor_div`, `div` and `shr`.
- `require n >= 0` as the lower bound of `n - 1` was never recognised (it is
  read as `(n >= 0) == true`); it now is, and for halving it takes
  `require n >= 1`. The repository's termination verdicts are unchanged.

### `soma fix` keeps annotations and the meaning of arithmetic

- Removing a handler's return type took everything up to the `{`: `on
  sq(x: Int) -> Int [native] {` became `on sq(x: Int) {`, silently dropping
  `[native]` (and `[task]`), while the fix reported "All checks passed". The
  removal now stops at the annotation.
- `;` was replaced by spaces even when the next statement began with an
  operator: `let d = a; -b` became `let d = a   -b`, which is `a - b`. Such a
  `;` is now left for the author, with its diagnostic.

### `to_float` reads what `to_int` reads

- `to_int(" 12 ")` and `to_int("12\n")` trim the text; `to_float(" 1.5 ")`
  and `to_float("1.5\n")` — a line read from a file or stdin — answered
  `()`. And `to_float(true)` answered `()` where `to_int(true)` is 1.
  `to_float` now trims surrounding whitespace and reads a Bool as 1.0 / 0.0.

### `zip`, `enumerate` and `flatten` refuse what is not a List

- `zip([1], "ab")`, `enumerate("ab")` and `flatten(5)` answered `[]` in
  silence, so a String or a Map where a List was meant emptied the result.
  Each now raises kind `type`, naming what it got.

### A native BigInt prints through `to_string` and `sb_push_int`

- In a `[native]` handler, an Int grown past 64 bits could be returned as
  `to_string(s)`, but `str_len(to_string(s))` and `sb_push_int(sb, s)` —
  the way the reference says to pack results — lowered it to i64 and raised
  kind `range`, advising a `let` the value already had. Both now print the
  full Int, as the interpreter does.

### An edge repeating a move with another guard is refused

- A transition takes the first declared edge that matches. After `* ->
  cancelled`, a `pending -> cancelled { guard { reason != "" } }` was never
  taken — its guard never ran, while `soma verify` reported it as
  runtime-checked — and `b -> d { guard { … } }` declared before `b -> d`
  refused a move the second edge allows. `soma check` now reports two edges
  that share a move but not a guard, suggesting `except` for a wildcard.
  Repeating a move with the same guard (or none on either) is unchanged.

### A compound or comparison face promise is checked or refused

- Only a bare structural promise was evaluated. `promise all_persistent &&
  has_auth` passed on a cell with neither, `promise all_persistemt ||
  has_face` hid its typo, and `promise balance >= 0` — enforced by nothing,
  at check or at run time — let `put(-5)` store -5. Structural promises
  combined with `&&`, `||` and `!` are now evaluated, and any other promise
  is refused, pointing at `invariant` for stored values and `cost {
  latency: … }` for latency. The `custom_property` example's `promise
  latency < 200ms` is now the quoted note it always was.

### A native BigInt division rounds once

- In a `[native]` handler, an inexact `BigInt / Int` converted both operands
  to Float and divided them — two roundings, so `10^25 / 7` answered
  `1428571428571428800000000.0` where the interpreter's correctly rounded
  `1428571428571428500000000.0` is right. The quotient is now rounded once,
  as the interpreter does.

### Native runaway recursion is a `stack_overflow`

- A `[native]` handler recursing past 20,000 calls came back as kind `type`
  (detail "stack_overflow: …"), which a `try` caught — though the reference
  says it raises `stack_overflow`, and interpreted runaway recursion fails
  the handler. It is now the same `stack_overflow` error.

### A native Float → Int conversion past 64 bits answers

- In a `[native]` handler, `let n = to_int(x)` — and `floor`, `ceil`,
  `round`, assigned or accumulated — was kept an i64 local whatever `x`
  held, so `x = 1e30` raised kind `range` where the interpreter answered
  `1000000000000000019884624838657`. Such a local is now kept i64 only when
  its argument is built from small Ints; the repository's native programs
  run at the same speed.

### `push` and `merge` refuse what is not their collection

- `push(list(1))` answered `[[1]]`, `push(5, 6)` the list `[5, 6]` and
  `push("ab", "c")` `["ab", "c"]`: a first argument that was not a List
  became an element. `push(xs)` now answers a copy of `xs`, and a non-List
  first argument raises kind `type`.
- `merge(5, m)` answered `5`, `merge(m, [1])` answered `m` unchanged, and
  `merge(a, b, c)` ignored `c`. Every argument must be a Map (kind `type`
  otherwise), and all of them are merged, later ones winning.

### `soma serve -w` keeps its flags, and its server stops with it

- The watched server was started as `serve <file> -p <port>` only: with
  `-w`, `--no-schedule` still ran every timer, and `--host`, `--verbose`,
  `--join`, `--no-check` and the global flags were dropped in silence. It
  now gets the same command line, minus `-w`.
- Killing the watcher (`kill`, a supervisor's SIGTERM) left that server
  running under init, serving and holding the port, so the next `soma
  serve` on it failed. On Unix the server now exits once its watcher is
  gone.

### A wildcard `except` names a declared state

- `* -> cancelled except [setled]` was accepted: the misspelt name excepted
  nothing, so `settled` — the state the author meant to keep final — could
  still be cancelled, and nothing mentioned the name. `soma check` now
  refuses a name no transition declares, with the near miss.

### An empty `[verify]` state list is refused

- `eventually = []`, `never = []` and `always = []` under `[verify]` checked
  nothing and `soma verify` printed VERIFY OK — while the same empty list
  under `[verify.after.X]` or `[verify.before.X]` was already refused. They
  are now a manifest error naming the key. `soma init` no longer writes an
  empty list into a new soma.toml, and the `messenger` example loses the
  `never = []` that checked nothing.

### `word_count` answers its words in a stable order

- `word_count` and `par_word_count` collected their counts in a hash map,
  so `keys(word_count(text))` and `to_json(...)` came out in another order
  on every run — though neither is listed as nondeterministic, and a replay
  compares them. The words now come in the order they first appear, for the
  parallel form too.

### `html(status, body)` checks its status like `response()`

- `html(query.code, …)` with the String `"404"` answered 200, and
  `html(1000, …)` answered 500 after the handler's writes had committed —
  `response()` refuses both, rolling the handler back. `html` now raises
  kind `response` the same way; `html(body)` alone still answers 200.

### A response header named twice sends both values

- `response(200, "ok", "Set-Cookie", "sid=1; HttpOnly", "Set-Cookie",
  "csrf=2")` sent only `csrf=2`: the second pair overwrote the first, so the
  session cookie was dropped in silence. `html(status, body, …)` did the
  same. A header named several times now goes out once per value.

### A native comparison on a BigInt answers outside an `if`

- In a `[native]` handler, `if x > 0` compared an Int past 64 bits, but
  `return x > 0`, `let b = x > 0` and `b = x > 0` lowered the Int to i64 and
  raised kind `range` where the interpreter answered — the error even
  advised binding the value with `let`, which it already was. A comparison
  and its `!`, `&&`, `||` now take the same path as a condition.

### A list literal needs its commas

- The comma between list elements was optional: `[1 2 3]` and `["a" "b"]`
  passed `soma check`, and `["n={m.get("1")}"]` — an unescaped quote inside
  an interpolation, a check error anywhere else — silently became a list of
  three elements. A missing comma is now a parse error that says so. A
  trailing comma is still accepted, and every program of the repository
  still parses.

### `agg` refuses a function it does not compute

- `agg(rows, "g", "v:median")` and `"v:SUM"` answered `v_median: ()`, a bare
  `"v"` was dropped, and a text value in a summed column was skipped where
  `sum_by` refuses it. Each now raises kind `type`, naming the five functions
  (sum, avg, min, max, count) or the value that is not a number. `count`
  still counts every present value, and a blank CSV cell is still missing.

### A sort order other than "asc" or "desc" is refused

- `sort(xs, "DESC")`, `sort(xs, "descending")`, `xs.sort(true)` and the same
  in both forms of `sort_by` sorted ascending in silence: only the exact
  text "desc" was read. The order is now `"asc"` or `"desc"`; anything else
  raises kind `type`.

### `/signal/<handler>?…` binds the query by name

- `POST /signal/store?v=VAL&k=KEY` filled `on store(k, v)` in query order,
  so the write stored `VAL → KEY`: key and value swapped, with no error.
  `/store?v=VAL&k=KEY` already bound by name. Both routes now share one
  binding: by name when a key names a parameter, else in order, with a
  trailing `opts: Map` collecting the other keys.

### A transition guard is not proven through a rebinding

- `soma verify` proved `guard { amount > 0 }` whenever the calling handler
  had `require amount > 0` before the `transition()`, and forgot the narrowing
  only on a plain `let` or `=`. An assignment inside a `try` block, the loop
  variable of a `for`, an assignment later in the loop body (seen by the next
  pass), a lambda parameter or a `match` binding named `amount` all went
  unseen: verify, even `--strict`, said "proven" and `soma run` failed the
  guard. Each is now runtime-checked; the verdicts of the repository's
  programs are unchanged.

### `par_read_files` and `par_word_count` answer an empty input

- `par_word_count(list())`, and `par_read_files` on an empty or missing
  directory or with a count of 0, panicked (kind `internal`) where
  `word_count` and `read_files` answer `{}` and `[]`.
- `par_read_files(dir, n)` took the first `n` files in directory order, so it
  read other files than `read_files(dir, n)`, and a file that is not UTF-8
  used up a place before being dropped. It now reads the same files: the
  first `n` text files by name.

### A `cost` axis speaks only for the bound it was given

- An empty `cost { }`, and `cost { usd: 1 }`, were both told their `tokens`
  bound was advisory — a bound the author never wrote. The tokens advisory is
  now raised only when a tokens bound is declared, as the latency axis
  already did.
- A declared `usd` or `latency` bound that unbounded `think()` sites keep from
  being proven said nothing at all: neither proven nor advisory. Each now
  names those sites on its own axis.

### A malformed budget annotation is refused, not quietly defaulted

- `capacity(N)`, `max_key_bytes(N)`, `max_value_bytes(N)` and
  `max_element_bytes(N)` feed the memory proof. A malformed argument fell back
  to the built-in default and `soma check` still printed "budget proven" — on
  a number the author never wrote. `capacity("big")`, `capacity()` and
  `capacity(1.5)` all proved 66 MiB where `capacity(10)` proves 24, and
  `capacity(10, 20)` silently read only the first. Each is now refused, naming
  what is wrong. A parameterised property that feeds no proof, such as
  `ttl(30min)`, is untouched.
- `state foo [max_instances(N)]` is checked the same way, and any other
  annotation in that position is refused: `max_instances` is the only one
  anything reads there, so `[max_instanes(10)]` sat doing nothing while the
  proof used the default instance count.
- A `scale { memory: … }` the budget parser cannot read was indistinguishable
  from declaring none: the proof was silently skipped, `soma check` said
  nothing, and `soma deploy` wrote the string out as it stood. `"128 Mo"`,
  `""`, `"abc"` and `"12.5Gi"` are now refused, naming the shapes that work.

### A `cell property` rule naming nothing is reported

- `cell property my_flag { rules { implies [persistant] } }` was accepted in
  silence, and the note even told the author that the slot implied
  `persistant` — a name nothing defines, so the slot got no persistence at
  all. The same held for `contradicts`, where the constraint it was meant to
  forbid simply never applied. `implies`, `contradicts` and `requires` now
  report a name no loaded cell property defines, with the near miss when
  there is one. A name defined further down the file still resolves. Under
  `requires` the warning explains the "not present" error that already
  followed.

### An unknown slot property names the one it is a letter away from

- `[natif]` on a handler already answered "did you mean [native]?". A slot
  property did not, so `[persistant]` read as a property of the author's own
  and the slot silently kept nothing across a restart, `[consistant]` gave no
  ordering, `[encripted]` no encryption. The warning now names the near miss
  when there is one, in the text and in the `--json` fix. A property of the
  author's own is not second-guessed, and an unknown property stays a
  warning, not an error.

### Nine quantitative builtins refuse an option they do not read

- `http_*`, `from_csv` / `read_csv`, `think` and `soma.toml` all refuse an
  unknown option. These nine dropped it, so `var_historical(r, map("alfa",
  0.99))` quietly computed the default confidence level and answered a
  different risk number with no warning. `var_historical`, `var_gaussian`,
  `expected_shortfall_historical`, `clean_covariance`, `impact_sqrt`,
  `importance_sample_rows`, `svd_lowrank`, `regress_sgd` and `to_sampled` now
  name the unknown option and the ones they take.
- Three signatures did not list every option the code reads, so following
  them would now be refused: `var_gaussian` also takes `max_obs`,
  `importance_sample_rows` also takes `max_dim`, and `regress_sgd` also takes
  `eta`, `rigorous` and `samples_per_iter`.

### `soma.toml`: a mistyped key is refused in every section

- `[verify]` and `[agent]` already refused an unknown key, on the grounds
  that a typo'd property must not vanish silently. Five sections still
  dropped one: `[cluster]`, `[compute]`, `[compute.parallel]`, `[package]`
  and a dependency table. So `seedz` left the node standalone while the
  operator believed it had joined a cluster, `threadz` ran sequentially,
  `entri` fell back to `main.cell`, and `pathh = "./lib"` resolved a local
  dependency through the registry by name. All five now name the unknown key
  and the ones the section takes. A dependency table reports through an
  untagged enum, which names no field, so the valid keys are spelled out.
- The `[verify]` key list is printed only under a `[verify]` error. It used
  to follow every manifest error, pointing the wrong way.

### The file write guard follows the link to what the write lands on

- The guard that refuses a write to the program, its configuration or its
  storage read the path as written, so a symlink walked straight past it:
  `write_file("upload.csv", …)` with `upload.csv` pointing at `app.cell`
  replaced the program's own source, which is the case the guard exists for.
  It now resolves the path first, so the check sees the file that would
  change. A directory link into `.soma_data` is refused the same way, and a
  trailing dot or space (which Windows drops from a file name) no longer
  hides a `.cell` or `soma.toml`. Ordinary writes, including into a directory
  that does not exist yet, are unaffected.

### `soma check`: one call text cannot mean two things

- `buffer`, `hashmap`, `strbuf` and their fifteen operations exist only inside
  a `[native]` handler, so they are absent from the builtin list a handler
  name is checked against — and a handler could be named after one. The same
  call then meant the primitive inside `[native]` and the handler everywhere
  else, and both compiled without a word: renaming a handler to `buffer` made
  every native call stop reaching it. A `[native]` handler that calls a name
  the program also defines as a handler is now refused, and told which name
  to change. A program with no `[native]` handler, and a `[native]` handler
  with no homonym, are untouched.

### `soma verify`: a transition past a `return` is not an edge the handler takes

- The refinement proof walked a handler's whole body, so a `transition()`
  after a `return` counted as a move the handler makes. `soma check` already
  warns that the statement is unreachable, and the runtime leaves the instance
  where it was, but verify answered that the handler reaches that state — the
  opposite of what an audit asking "what can move this to `released`?" needs.
  The walk now stops where the flow leaves the block. A `return` inside an
  `if` still does not end the handler, so what follows is counted as before.

### `soma verify`: the reason a write is not proven names the rebinding

- A handler that reassigns a parameter before writing it was told that
  `amount` is a parameter and to narrow it with `require amount >= 0 else …`
  — advice it had already followed, and which cannot work: the require
  describes a value that is gone by the write. The reason now names the
  rebinding and says to write the value that was checked, or to require the
  one written. A parameter written straight through still gets the require
  advice.

### `soma check`: a name reached for out of habit gets the same answer either way

- `x.str()` was answered with `to_string(x)`, while `str(x)` fell through to
  edit distance and suggested `shr`, a bit shift. So did `size` (`sin`),
  `pop` (`pow`) and `add` (`abs`), and thirteen more names got no help at all.
  A plain call and a string interpolation now consult the same table the
  method form does. A genuine typo still gets its near-miss.

### `soma lint`: no `match` suggestion that would change the answer

- A branch that rewrites the variable the chain compares makes the ifs run in
  sequence on the new value: `if s == "a" { s = "b" }` followed by
  `if s == "b" { … }` runs both, where a `match s` takes one arm. The note
  suggested the rewrite there anyway, so following it changed what the handler
  returned. It is no longer raised when any branch of the chain assigns the
  compared variable; a chain that leaves it alone is still reported.

### `soma lint`: the unrouted-handler note matches what `soma serve` does

- The note named POST for every handler: `soma serve` answers a GET on one
  that does not write, so a read-only handler now reads `reachable as GET or
  POST /<name>/<args>`. The two commands share one write analysis, so they
  cannot drift apart.
- A handler `request` reaches through one of its own helpers was reported as
  an open endpoint. `soma serve` does not expose it — `request` owns it — so
  `request` → `submit` → `validate` left `validate` described as reachable at
  `/validate/<args>` when nothing answered there, with the advice to rename it
  private. The note now uses the same ownership analysis `soma serve` applies.
  Of the nineteen shipped `soma example` programs carrying the note, eleven
  were in this case.

### `soma lint`: the `.get()` fallback has the slot's value type

- The `unchecked .get()` warning suggested `?? map()` whatever the slot held.
  A `Map<String, String>` was told to fall back on a Map, so applying the
  suggestion put a Map where a String belonged. The default now comes from the
  declared value type: `""` for a String, `0` for an Int, `0.0` for a Float,
  `false` for a Bool, `[]` for a List, `map()` for a Map, and the element type
  for a `List<T>` slot. A type with no literal default (`Any`, a user type) is
  no longer given an invented one: the suggestion says to compare the result
  to `()` instead. Nineteen of the shipped `soma example` programs carried the
  wrong advice.

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
- Docs: `divmod` promised `0 <= r < |b|`; with a negative divisor the remainder
  takes the divisor's sign (`divmod(7, -3)` is `[-3, -2]`), as `mod` documents.
- `.count` and `.length`: on a local Map `.count` reads the field (a JSON
  record's `count`), yet `check` warned "on a Map `.count` is the number of
  entries (never ())" and told the author to rewrite a correct read. The
  warning is limited to `len`, `size`, `length`, `keys`, `values`. On a memory
  slot `.length` read as a missing field while it counts on a value; slots now
  accept `len`, `size`, `count` and `length` alike. One regression test.
- A slot's no-argument aliases no longer swallow a predicate: `rows.all(r => …)`
  on a List slot returned the slot's content — a non-empty list, so
  `if rows.all(p)` was always taken and a `require` on it never refused — and
  `rows.count(p)` returned the entry count instead of the matches. `all`,
  `count`, `values`, `list`, `entries`, `items`, `first` and `last` answer as
  slot aliases only when called without arguments; with one they are the
  builtin over the slot's content, exactly as on a local list. One regression
  test.
- `soma check` reads `every` / `after` bodies when it proves a `match`
  exhaustive: only `on` handlers were walked, so a scheduled tick missing a
  variant passed the gate and then failed on every fire, rolled back, visible
  only in the server log. Every other static check already covered those
  blocks. One regression test.
- `soma check` refuses a `face` promise nothing checks: an unknown predicate
  was silently true, so `promise all_persistemt` was green on a cell without a
  single persistent slot, and the face claimed a guarantee no check backed.
  The message names the known structural promises and suggests the nearest, or
  quoting the text to keep it a note. One regression test.
- A `cell checker` rule naming a predicate nothing implements is refused:
  `require has_auht else Tag` (a typo) held for every cell, so the project
  rule enforced nothing, and the same typo under a `!` failed every cell
  instead. A checker now reads the same vocabulary as `face { promise … }`
  (`require all_persistent` was silently true there too) and an unknown name
  is an error naming the predicates, reported once. One regression test.
- A `match` arm whose variant pattern does not fit the variant is refused:
  an unknown variant name, a struct field the variant does not declare, a
  tuple pattern with the wrong number of bindings, or a struct pattern on a
  tuple variant all passed `check` — which then called the match exhaustive —
  and raised "variant not handled" at run time on every value of it. The
  message names what the variant declares. One regression test.
- `get_status(id) == Funded` is refused: in a typed machine `transition()`
  takes the variant while `get_status()` answers the state's name, so the
  comparison raised `cannot compare String and Variant` on every run and
  could never be true. `check` names the fix (`== "Funded"`); a
  variant-to-variant test is untouched. One regression test.
- `soma check` reports a `match` arm that can never run: a case appended below
  an unguarded `_ -> …` (the match silently kept taking the catch-all), or one
  repeating an earlier unguarded pattern — a literal, a variant, a prefix, a
  range, or an or-pattern whose alternatives are all already taken. A guarded
  arm may fail its guard, so it kills nothing. It is a warning, like the other
  unreachable code. One regression test.
- A cell nested in `interior { }` gets the same static checks as a top-level
  one: the sum-type gates (exhaustive `match`, variant-pattern shape, a state
  compared with a variant), the invariant validation (an unknown name, a
  `status` rule without a machine) and the builtin arity check walked only
  top-level cells, so the same code was refused outside and passed inside.
  One regression test.
- `soma verify` proves a cell inside `interior { }` like any other: it walked
  only top-level cells, so a state machine, a loop and a memory invariant
  nested there were never examined — a STATICALLY violated invariant came out
  as `VERIFY OK`. The machine, refinement, termination and invariant proofs
  now run on interior cells (the composition check already did). One
  regression test.
- `soma describe` shows the cells inside `interior { }`: the contract summary
  of "every cell" listed only top-level ones, so their face, memory and
  handlers were invisible to a reader and to an agent — `chat.cell` reported
  one cell of its four. Each nested cell is listed after its parent, named
  `(interior of X)` in the text and carrying `interior_of` in the JSON. One
  regression test.
- Calling a cell of an `interior { }` by name is refused: the interpreter
  never registers those cells, so `Worker.ping(…)` passed `check` and raised
  `undefined variable: Worker` when the test rule or the handler ran. The
  bare-name form was already refused; the qualified one now is too, with the
  reason (a sibling's signals drive them). One regression test.
- `soma lint` reads the cells inside `interior { }`: it walked only top-level
  cells, so their handlers were never linted (it already covered `every` and
  `after` bodies). One regression test.
- `soma fix` no longer deletes code past the handler it is fixing: removing a
  handler's `-> T` searched the whole file for the opening brace, so a handler
  written without a body on that line had everything up to the NEXT cell's `{`
  deleted — the closing brace and a whole cell declaration — and the fix was
  reported as applied. The brace is looked for on that line only; otherwise
  the file is left untouched and `check` explains what to write. One
  regression test.
- `soma add` keeps the manifest the author wrote: it re-serialized the parsed
  `soma.toml`, so every comment disappeared and every default value was
  spelled out. It now edits the text, touching only the dependency's own
  entry (replacing it when the package is already there), keeping the file's
  own line endings, and refuses rather than write a manifest it cannot edit
  safely. One regression test.
- Docs: `soma docs serving` listed the dashboard among the responses carrying
  `Access-Control-Allow-Origin: *`. It does not: `/__soma/` and
  `/__soma/hordes` deliberately withhold the header, so a page on another
  origin cannot read the program's cells, invariants and machines. A test
  pins both sides of that posture.
- A duration or percentage literal renders as written everywhere: the shared
  expression renderer had no arm for either, so `promise latency < 200ms`
  came out of `describe --faces` as Rust's `Debug`
  (`Duration(Duration { value: 200.0, unit: Milliseconds })`). One regression
  test.
- A builtin taking a lambda refuses extra arguments like every other one:
  reading its registry signature, the `>` of the arrow in
  `filter(list: List, x => Bool)` counted as a closing generic, so no arity
  could be derived and `filter`, `find`, `any`, `all`, `count`, `reduce` and
  `sort_by` accepted any number of arguments, the extra ones silently ignored
  at run time. One regression test.
- `concat` with one argument is refused: it answered that argument unchanged,
  so a forgotten second one produced a wrong result in silence, while every
  sibling (`split`, `replace`, `contains`…) raises on a missing argument. The
  message names the two forms. One regression test.
- `lowercase` / `uppercase` case the value they stringify: their doc says
  "non-strings are stringified first", but the stringified form came back
  untouched, so `uppercase(true)` answered `"true"` and the call quietly did
  nothing. One regression test.
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
