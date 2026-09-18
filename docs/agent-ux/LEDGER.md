# Agent-UX ledger

Method: fresh agents that have never seen Soma get the binary, a task and
nothing else (`soma docs`, `soma example`, `soma describe`, the compiler's
messages). Each keeps a friction log. Findings are fixed, regression-swept
(274+ Rust tests, all .cell programs old-vs-new binary), and a new cycle
starts with new agents and new tasks. Status: [ ] open, [x] fixed, [~] partial,
[-] won't fix (with reason).

## Cycle 1 — 2026-09-18 (4 agents: URL shortener, expense approval, triage agent, order analytics)

All finished; first drafts passed `check` on the first try in 3/3 reported.
What cost them time, by severity:

### Silent failures (an agent reports proofs it does not have)
- [x] S1 `soma.toml` without `[package]` is silently ignored: `[verify]` properties never run yet verify says passed; `[agent] mock` has no effect.
- [x] S2 transition guards (`a -> b { guard { amount < 1 } }`) pass check AND verify, then raise UndefinedVar at runtime; guard scope undocumented.
- [x] S3 `invariant len(links) <= 1000` silently bounds the written VALUE's length, not the entry count; no per-slot size bound syntax.
- [x] S4 cost prover says "no max_tokens" for `think(prompt, system, map("max_tokens", N))` — only reads opts in arg 2.
- [x] S5 `assert_fails` passes for the wrong reason (UndefinedVar); wants `assert_fails expr matching "text"`.
- [x] S6 `promise cost <= 1000` in face silently accepted, no effect.
- [x] S7 `SOMA_LLM_MOCK=bogus` silently accepted.
- [x] S8 undefined variable (typo) passed check, failed only at runtime (found by me before the reports).

### Diagnostics
- [x] D1 `soma test` hid the asserted expression (`assert ... == 400`), no line numbers, left/right printed before the ✗ line; a null from a wrong field name unexplained.
- [x] D2 `soma check` diagnostics had no file:line:col (text and JSON).
- [x] D3 verify noise: "has no guard" on every transition (pushes toward S2); "states [X] cannot reach terminal 'rejected'" for intended designs.
- [x] D4 verify: two tallies in odd order; "18 properties loaded" vs "15 passed"; after+never worded "all paths eventually satisfy".
- [ ] D5 `.url` on a try-result is `()` silently (should be `.value.url`): static check.

### Docs
- [x] O1 response shape (`_status`, `_body`) documented nowhere.
- [x] O2 `cost { tokens: N }` section documented nowhere; no note when think() is used without one.
- [x] O3 verify property vocabulary (eventually/never/always/after) undocumented → `soma docs verify`.
- [ ] O4 `require X else name` appears in corpus, not in docs.
- [~] O5 the advertised `soma example invariant http` returns nothing; no web example uses an invariant.
- [ ] O6 `soma example <id>` does not print the companion soma.toml an example depends on.

### Language / features wanted
- [x] L1 scriptable LLM mock per test (`mock think "abuse"`, queue, error injection); `soma test` should mock by default when no key.
- [ ] L2 `* -> failed` also applies to terminal states (closed is no longer terminal). No "non-terminal only" wildcard.
- [ ] L3 invariants like `counts >= 0` with write `(counts.get(k) ?? 0) + 1` are only runtime-checked: inductive proof would make "proven" mean more.
- [~] L4 properties inline in the state block instead of soma.toml; a precedence property ("paid requires manager_approved").
- [ ] L5 `get_status(id)` returns the initial state for an unknown id.
- [x] L6 `always` property is useless as defined (single state name).

### From the data/algorithms agent (the only one who would NOT pick Soma again)
- [x] E1 `==` on lists/maps is not structural — `[] == []` is false; the test runner prints identical left/right and says FAILED; the documented `to_string` workaround breaks on map key order.
- [x] E2 `rows.push(x)` as a statement silently does nothing (push returns a new list) while `xs[i] = v` mutates.
- [x] E3 unknown method (`xs.includes(x)`) only fails at runtime, with a Rust Debug dump and no did-you-mean.
- [~] E4 `[a] + list` is element-wise with silent broadcasting: `merge_sorted` returned `[29.0]`. Length mismatch raises nothing.
- [x] E5 parser stops at the first error and gives no hint for foreign idioms: `{k: v}`, `@native`, `xs[a:b]`, `(a, b) =>`, `for (k, v) in`, `null`/`None`.
- [x] E6 missing everyday builtins: `keys(m)`/`values(m)`/`entries(m)` as functions, `contains(list, x)`, `slice`, comparator/key-function sort, `round(x, digits)`, `deep_eq`.
- [x] E7 no `let` inside test `rules` — every assertion re-calls the fixture handler.
- [x] E8 `soma test` does not compile `[native]` handlers: a native-vs-interpreted assertion compares the interpreter with itself.
- [x] E9 `soma run app.cell validate '{"id":1}'` passes a String into a `Map` parameter; error surfaces deep inside the handler.
- [ ] E10 `[native]` placement (`on f(n: Int) [native] {`) and test-cell anatomy (`cell test Name { rules { } }`) not in `docs agent`.

### Cycle 1 — decisions taken
- **Dispatch rule changed**: `f(args)` resolves to the program's handler `f` when one takes that many arguments, the builtin otherwise (was: builtins always win). Reason: the author of this ledger fell into the old rule four times in one night; user definitions shadow the library in every language agents know; and under "builtins win" each new builtin silently hijacked homonymous handlers. Impact measured on 986 programs: 2 broke (`on list() { let items = list() }` → now `[]`), fixed.
- `==` is structural on lists / maps / variants (was a runtime error).
- Guards now see the calling handler's locals; `soma check` verifies their scope. Two examples declared guards they could never exercise — fixed.
- verify no longer nags "has no guard" / "cannot reach terminal X".
- **Soundness bug found while documenting**: `[verify.after.X] never = ["Y"]` was encoded as "after X, eventually some state ≠ Y" — true the instant the machine is in X. It "proved" `after executed never cancelled` in examples/pricing, which is false (executed → failed → pending → cancelled). Now: Y must be unreachable from X, with a counter-example. New precedence property `[verify.before.T] requires = [...]`; `always = [...]` is one property over the set.
- Tests: `let` and `mock think "…"` / `mock think error "…"` in rules, `assert_fails … matching "text"`, `[native]` handlers run natively, think() auto-mocked when no key, UndefinedVar no longer counts as a passing assert_fails.
- Parser: hints for 13 Python/TS idioms (`{k: v}`, `xs[a:b]`, `(a, b) =>`, `for (k, v) in`, ternary, and/or, const, elif, `;`, `@decorator`, `def`, unnamed test cell).
- Invariants: `links.size <= N` bounds one slot; `len(slot)` warns (it measures the written value).

## Cycle 2 — 2026-09-18 night (5 agents: log analytics, warehouse reservations (2 cells), refund agent with tools, port of a Python Bank class, cold evaluation of soma-lang.dev)

Scores ("I would want to use this again"): data 6/10 (cycle 1: "no"), reservations 7, refund agent 7, Python port 6, website: want 6 / easy 7, verdict PILOT.
Measured progress on the data task: green in 16 invocations (cycle 1: 28), 4 check cycles (cycle 1: ~10); "each hint resolved in one step".

### Blockers (found by the site evaluator, reproduced with numbers)
- [x] B1 `soma serve` is threaded, handlers are not atomic: 300 payments of 10 against a balance of 1000, 50 parallel calls → 122–153 paid. Lost updates on read-modify-write.
- [x] B2 no rollback: a handler that raises keeps its earlier writes/transitions. corpus/escrow_finance/wire_dual_control's story ("the wire never exists") is false: the wire ends `drafted`, then can be sent. Four agents hand-wrote compensation.
- [x] B3 every non-underscore handler of the request-owning cell is auto-routed (`GET /open_account/acme/999999` sets a balance, bypassing the router) and shadows `request` routes (`POST /hold/r1` → 500). Passed check, verify, test AND `soma run … request`.
- [x] B4 vacuous properties pass: `never = ["piad"]` (undeclared state); `soma verify` exits 0 on a file that fails `soma check`; `soma test` runs a file that fails check.

### Error model (2 agents, top friction)
- [x] R1 no documented way to raise; errors stringly typed, no payload, no re-raise; everything prefixed "require failed:" → `fail(kind, detail)`, `r.kind` / `r.detail` on try-results, `fail(r)` re-raises, prefix only on real requires, test output in the language's words (no Rust Debug dumps).
- [x] R2 `require c else "msg {x}"` / `else variable` silently taken literally; `require … else Tag` absent from docs.
- [ ] R3 `try { f() }?` returns the error map as a normal value (not a propagate).

### Still reaching runtime past a green check
- [x] C1 call with the wrong argument count; face return type vs returned value.
- [x] C2 `() >= 500` was silently false (typo'd field) → ordering against () raises.
- [x] C3 variant `==` false when a payload is a Map/List.
- [ ] C4 field typo on an untyped record (`order.statuss` → ()). Needs typed records.
- [ ] C5 record read from a slot, mutated, never written back: silent lost update. Wants a lint.
- [x] C6 `invariant accts.balance >= 0` passes check, rejects every write (`value.balance` works).
- [x] C7 `from_json("garbage")` returns the string; `to_int("1.5")` = 1; no strict parse.
- [x] C8 `m[k] += 1`, `acc.balance += x` unsupported.

### Verify
- [x] V1 `* -> failed` made success states non-terminal (2 agents). First fix (wildcard skips final states) broke `* -> deleted` in 4 programs and was REVERTED; now: verify warns with the exact fix, and `* -> failed except [paid, denied]` keeps states final.
- [x] V2 invariants on computed values: interval + induction prover; per-conjunct report; always-rejected-and-caught writes reported as such.
- [x] V3 `requires = [a, b]` is "one of" (passes silently when you meant both) → `requires_all`.
- [x] V4 summary says "0 failures" before the temporal section fails; streams interleave; "think-isolated" printed for cells with no LLM; "N literal transitions" counts call sites.
- [x] V5 `get_status(unknown id)` returns the initial state → documented as such; `has_state(id)` added.

### Tests / mocks
- [~] T1 `mock approve false`; scripted tool calls; mixed queue with an error in the middle; mock clock; per-assert isolation.
- [x] T2 `soma check --json` lists the proven cost bound under "warnings".

### Small language gaps
- [~] G1 `parse_int`, `pad_left`, negative index `xs[-1]`, `avg([1,2])` truncates, default parameters, unknown function alias table (`parseInt` → to_int) with the span on the call, parser reports one error per run.

### Site (evaluator)
- [x] W1 add /docs/guarantees.md (PROVEN | ENFORCED AT RUNTIME | NOT COVERED) and align the /agents headline ("PROVES … memory invariants" is broader than the truth).
- [x] W2 /docs/serving.md: routing table, exposure rules, bind address/ports, threading, response shape.
- [x] W3 corpus: /corpus/domains.json (~1 KB), finer features (guard, precedence, router, two_cell), drop the `tests` feature (all 316 have it), programs combining http + state_machine + invariant, each program's soma.toml + verify line in the index; audit narratives against behaviour.
- [x] W4 /status: license (SPDX), changelog, release date, known limits, security contact, security.txt; LICENSE on the site; pin install.sh to a tag + checksum.
- [x] W5 llms-full.txt repeats llms.txt (10 KB paid twice) and carries ~20 KB of linalg/quant irrelevant to a service → profiles; spec.md says "Version: 2.2.1"; /examples is a dead end; repo-relative links dangle; docs hash in version.json.

### Cycle 2 — decisions taken
- **Handlers are atomic.** Undo journal in the interpreter (writes, deletes, appends, transitions); a failing top-level handler is rolled back; a failing `try` is a savepoint; top-level invocations are serialized process-wide. The evaluator's experiment now pays exactly 100/300 (was 122–153), pinned by a test.
- **Error model.** `fail(kind, detail)`, `r.kind` / `r.detail`, `fail(r)`; "require failed:" only on requires; no Rust Debug dumps in test output.
- **Routing.** A path `request` matches explicitly wins over the auto-exposed handler of the same name; check warns (19 true collisions in the repo, 4 in rebalancer).
- **Gates.** verify and test refuse a program that fails check; properties on undeclared states are errors; one final VERIFY verdict; `requires_all`.
- **Prover.** Interval + induction reasoning on invariants, reported per conjunct; an always-rejected write under `try` is reported as "can never commit" (2 governance programs rely on it).
- Ordering against `()` raises (was silently false); variant payloads compare structurally; `m[k] += 1` / `a.f += x`.

### Cycle 2 — site and corpus
- /docs/guarantees.md (proven | enforced | not covered), /docs/serving.md (routing, exposure, atomicity, ports), /status (maturity, MIT, changelog, limits, security contact), /.well-known/security.txt, /LICENSE, /CHANGELOG.md, /llms-service.txt (~35 KB, no linalg), /corpus/domains.json; each corpus program's soma.toml and verify verdict in its index; features guard/fail/cost/tools/multi_cell/service, `tests` dropped; spec.md (2.2.1) withdrawn; version.json carries the sha256 of `soma docs agent` from the binary.
- New corpus domain `services/`: payments_approval (Ledger + Api, proven precedence), warehouse_reservations (two cells, guard, slot.size, except), refund_agent (tools, proven cost, mock think / mock approve, daily cap).
- Found on the way: any URL with a query string was an HTTP 500 for a 3-parameter `request` (the query map was pushed unconditionally) — fixed. `try { }` now accepts a block of statements.
- Lesson written into guarantees.md: an error means "nothing happened" (rollback includes a rejection transition) — a refusal that must be recorded is returned, not raised.

### Cycle 3 — found while preparing the cycle (2026-09-18, night)
- `Ledger.deposit(x)` (qualified cross-cell call) passed check and died at runtime with "undefined variable: Ledger"; only the bare handler name worked. Now: `Cell.handler(args)` calls the handler, check reports a missing handler with did-you-mean. llms.txt gotcha 1 says both forms.
- `final: paid` in a state block gave "expected '->', found ':'" — now a fix-it ("states with no outgoing transition are final; delete this line").
- `r.after` was a parse error (`after` is a verify keyword): verify words are now accepted as field names.
- `soma run app.cell validate '{"id":1}'` passed a String to a `Map` parameter (E9): the token is parsed as JSON for Map/List parameters, with a boundary error when it is not.

## Cycle 3 (2026-09-18, night) — 5 fresh agents: booking service, Python port, LLM triage, site evaluator, adversarial soundness

Scores before fixes: booking 16/40 invocations, first check/verify/test green, correctness confidence 7/10; port 26/35, byte-identical to Python on first run, obviousness 7/10; triage 31/40, 53/53 first try, unattended trust 6/10; site: credibility 7, completeness 6, agent-friendliness 8, desirability 6; adversarial: 9 unsound + 4 wrong-output + 14 gaps in 55 invocations.

### Blockers / unsound (all fixed unless noted)
- [x] N1 `next_id()` picked its counter slot in HashMap order (differs per serve thread) and wrote outside the journal: an existing id was handed out again, a refused request burned an id. Now: first declared Map slot (else the machine's status backend), journaled.
- [x] N2 `approve()` auto-approved under headless serve AND in tests ("use soma serve for interactive approval" printed under soma serve). Now fails closed: `mock approve`, SOMA_APPROVE=always|never, a terminal under `soma run`, else kind `approval_required`. approval_gate example rewritten.
- [x] N3 `history = push(history, v)` on a List slot silently built a local list (count always 1; check passed). Slots read by bare name materialize; whole-slot assignment is a check AND runtime error naming `.push/.set`.
- [x] N4 `forall n in 0..100` was 50 random samples with a wall-clock seed: ✓ then ✗ on the same file. Exhaustive up to 20k values, fixed seed beyond, labelled "NOT a proof".
- [x] N5 unbounded recursion under serve: thread stack overflow killed the process. 64 MB request threads; the depth guard answers 500 `stack_overflow`; the service stays up.
- [x] N6 serve bound 0.0.0.0, claimed "listening on localhost" while another process owned 127.0.0.1:port. Now 127.0.0.1 (`--host`), probes the port first, refuses a program that fails check (`--no-check`).
- [x] N7 errors under serve were all 500 with the raw text; `()` became `{}`. Kinds map to 404/403/409/422/400 as `{"error", "kind"}`; `()` is `null`; strings are JSON-escaped.
- [x] N8 `body: String` received a parsed Map under serve but a String under test. The declared type decides (String raw, Map parsed or 400), everywhere.
- [x] N9 `fail("kind")` inside a `map`/`filter` lambda reached the caller as kind "type" with a Rust Debug dump. Errors keep their identity.
- [x] N10 check missed names used outside their scope (lambda param, `let` in a block, loop var); now block-scoped like the runtime.
- [x] N11 handler parameter types unchecked at call sites (`typed_add("a", 1)`, `"ten"` into `Int` over HTTP). Checked at the boundary (Map accepts variants/`()`/callbacks as the corpus does).
- [x] N12 native: `shl(2^62, 1)` returned a STALE value (i64::MIN sentinel collided with "big result in buffer"); `t += 2^62` overflowed in the Rug fallback (literal steps up to 2^32 are "small"); interpreter `shl` wrapped. Fixed all three.
- [x] N13 `x |> f() == y` parsed as `x |> (f() == y)`: `|>` now binds between comparison and arithmetic; check rejects a non-call right side.
- [x] N14 two state machines in one cell: `transition()` picked one at random. check error (one lifecycle per cell); semaphore apps fixed. Also: duplicate cell names, empty file.
- [x] N15 `soma run app.cell nosuch 1` ran the first handler with "nosuch" as its argument. Error with did-you-mean.
- [x] N16 `deploy` copied the macOS binary into a debian image and exited 0 when the provider CLI was missing. Multi-stage Dockerfile builds the tag from source; exit 1.
- [x] N17 builtins.json listed `timestamp()`/`date_now()` (reserved, not callable). Filtered; `replay` field explains `deterministic`.
- [x] N18 cost proof ignores cross-cell calls (`Api.f()` → 5×think in Ledger reported "peak 50"). Bare and `Cell.handler` calls into other cells are composed.
- [x] N19 face return types never checked. Literal mismatches are check errors; every return is checked at the boundary (Map takes record/variant/`()`, `request` exempt). 17 corpus faces were lying and are fixed.
- [ ] N20 `[native]` vocabulary only checked at run time, one error per run; a `List<Float>` parameter is lowered to f64 with a rustc dump; the buffer API (buf_get…) is undocumented. Open.
- [ ] N21 auto-exposure of face signals when `request` exists (3 agents): unplanned surface, no auth story. Open (documented).
- [x] N22 lint false positives: routing collection sees through try/match/lambdas; `.get()` followed by a `()` test or `require` is not "unchecked".
- [ ] N23 verify prints "proven (writes abs(v))" for a Float writer where NaN is rejected at runtime. Wording open.

### Smaller (fixed)
- `soma docs guarantees | serving` offline; `mock approve` documented; `soma verify` always ends with a verdict (vacuous when no machine); `assert` needs a Bool; `()` and `matching` echoed whole in test output; from_json("") raises; to_int out of range raises; sum() on a non-number raises; List slot `.len/.get(i)/.last/.has`; `f() + 1` as a statement; nested quote inside `{…}` named; `final:` in a state block named; `r.after` (verify words as field names); starter uses request routes only, a provable invariant and maps refusals to 400; `soma example --all` and the terms that narrow; landing page leads with why, status below, reproduce section works after install; `/examples/` dead links removed; `--jit` no longer claims 200x; llms.txt: HTTP rules, verify wording (termination proven only with a decreasing argument), forall sampling, the facts agents had to guess.

## Cycle 4 (2026-09-18, night) — 5 fresh agents on the cycle-3 binary

Scores: library service 12/40 invocations, first check/verify/test green, confidence 8/10 (cycle 3: 16, 7/10, one blocker); TS port 26/35, byte-identical to node first run, obviousness 7/10; purchase-order agent 33/40, 66/66 first try, trust 7/10; site: credibility 7, completeness 7, agent-friendliness 8, desirability 5 (24 of 34 claims exact); adversarial: "enforced" held everything (rollback across cells + transitions + next_id + mocked think, 50-way races, restart), 3 unsound findings.

### Fixed
- [x] `soma serve` never compiled `[native]` handlers (interpreted: buffer() answered 400, native/interpreted results differed). Compiled once at start, shared by every request; `soma test` now refuses natives that do not compile instead of running them interpreted.
- [x] Two `soma run` processes on one `.soma_data` lost updates (3082 of 6000). A cross-process lock (SQLite `BEGIN IMMEDIATE` on `.soma_data/lock.db`) is held per handler.
- [x] `never = ["shipped"]` with `cells = ["Ticket"]` passed vacuously (shipped belongs to Order). Unknown-state check is per targeted cell.
- [x] `"v {n}"` in a `[native]` body returned the literal with check ✓ → check error. Native `String` parameters did not compile (`&str`/`String`), `min(Int, Float)` did not compile, `to_string(1.0)` was "1" natively, `sqrt_int` of a BigInt was 0 in the interpreter — all fixed; 7 corpus programs with native String params run again.
- [x] `check --json` reported the new errors as `interpolation_undefined` with an unrelated fix text, and older ones as `other`. Every error now has a stable kind (`CheckError::Static`), the fix is the clause after the dash.
- [x] Literal checks at check time: `transition(id, "c")` to an undeclared state, `takes_int("s")` against `n: Int`, a slot declared `n: Int` (scalar), a declared `cost` bound that cannot be proven (was a note with exit 0).
- [x] `require open < 3 else LoanLimit` then `open + 1` is now PROVEN to keep `open_count <= 3` (unconditional require on a once-bound local narrows its interval).
- [x] `require cond else Tag "detail {x}"`: a kind and an interpolated detail.
- [x] `mock price_check 42` / `mock price_check error "down"`: any handler (a tool, an http wrapper) can be scripted in tests.
- [x] serve: path segments and query values coerced to the declared parameter type (`/decide/x/true` → Bool, `/f/1.5` → Float, `/sval/123` → String); a trailing Map/List parameter with no body is empty; `soma run app.cell request GET /x ""` with `body: Map` gets `map()`; raised errors and `response(...)` maps share one JSON style; the 404 lists public handlers as a JSON array; JSON bodies keep big integers exact and 1e400 is infinity (serve had its own lossy converter); the bus port is opened only for programs that can use it (emit / scale / --join).
- [x] verify's liveness counter-example named an edge that does not exist (printed from the DFS start, not the cycle); Temporal section had colour codes when piped.
- [x] `.set()`/`.push()` on a LOCAL map/list is a check error naming `m[k] = v` / `xs = push(xs, x)`; `throw` / `===` / `new` / `class` get hints; llms.txt: classes → cells, `Number()`, `.soma_data`, guard syntax and scoping, `"""` (escapes raw, `{x}` interpolated), forall end-exclusive, `{"result": …}` for scalars, CORS `*`, `request` in face optional; serving.md no longer claims all interfaces; `soma docs` index says `all` includes operations; fly.toml app name falls back to the cell name; `distinct_by`; `stddev`/`stdev` registered; reference's `to_json` example and `r.detail`.
- [x] `think()` with no key prints one stderr note even when the program catches the error.

### Not reproducible / by design
- `soma run run.cell second y` running `first("second y")`: not reproducible with the same binary (both "second" and "third" resolve; likely a shell wrapper quoting `"$*"`). `GET` on a mutating handler: documented, auto-routes take any method.

### Open
- [ ] Slot VALUE types are not enforced (`Map<String, Int>` stores 2.5). Considered; the corpus writes Floats into Int maps in places — needs a sweep-driven decision.
- [x] `[native]`/interpreter edge cases aligned: `to_int`/`floor` of NaN, inf or beyond i64 raise on both (a `soma:` panic reaches the guard instead of the Rug fallback), `abs(i64::MIN)` is 2^63 on both, `bit_len` of the magnitude, `bit_test`/`shr`/`shl` with a count ≥ 64 give 0 / saturate on both.
- [ ] No header access / auth in `soma serve` (3 agents): a reverse proxy is the documented answer.
- [ ] Parse errors stop at the first one; uppercase slot names are not indexable.
- [ ] No published Linux binary (site says so; deploy builds from source).

## Cycle 5 (2026-09-18, night) — 5 fresh agents on the cycle-4 binary

Scores: tickets-with-expiry 23/40, 65/65 first try, 40-way race → exactly 3 winners, real `every` expiry, 8/10; Go port 29/35, 18/18 first try, native 244× measured (1.2 ns/Collatz step), Go-obviousness 6/10; 4-cell support system 26/40, 62/62, cost proof followed cross-cell calls (mutants caught), 6/10 unattended; site: time-to-first-correct-program 6 invocations (2 check-time retries), 20/22 claims exact; adversarial: ENFORCED column held everything (tick atomicity, tick/request/CLI serialization across processes, replay), 3 false statements found.

### Fixed
- [x] `every` / `after` blocks were invisible to refinement and to the literal checks: `every 1s { transition("t", "zzz") }` passed check+verify and raised every tick. Scheduler blocks are handler bodies for both now.
- [x] `[native]`: `map(…)` accepted by check (rustc said no); `if is_prime(i)` sibling Bool condition in BigInt mode; `for i in range` loop variable typed i64 while siblings took Integer (rustc E0308, and the error was hidden behind two warnings); rustc error blocks now come first, 40 lines shown.
- [x] Bit operations are arbitrary precision everywhere (Python semantics): `shl`/`shr`/`band`/`bor`/`bxor`/`bnot`/`bit_test` on BigInt in the interpreter; native fast path falls back to BigInt on a lost bit. The four xorshift demos mask explicitly; the bit-packing DP examples (coin_change, knapsack…) work again.
- [x] `r.len` on `map("len", 351)` answered 2 (pseudo-fields shadowed real keys): a real key wins.
- [x] The invariant prover narrows parameters (`require p > 0 … set(k, p)`) and knows `require b <= a` for `a - b >= 0`; `require open < 3 … open + 1` was already proven.
- [x] `mock now 1700000000` freezes now()/now_ms()/today(); `mock Cell.handler …` accepted; a mocked error's kind is the text before ": " (or the text); a mock left unused after a rule that raised is discarded with a note (it used to script the next rule's call); each `cell test` starts with fresh slots, machines and mocks.
- [x] `soma test --json`; `soma verify --strict` (every ⚠ fails — the CI gate two agents asked for); `check --json` on an unreadable file prints a JSON error; `soma check` refuses >400-deep expressions instead of aborting (exit 134).
- [x] The first `every` tick runs at start-up (a sweeper sees work due while the server was down).
- [x] serve: non-JSON body to a public `body: Map` handler → 400 kind `json`; the pre-handler 400 is logged; the route/handler collision warning is silent for the documented delegation shape (`/add/<id> -> add(id)`); the bus port only opens when usable.
- [x] `promise "…"` is a note, not a nagging warning; `soma run` prints nothing for `()`; `fields(s)`, `trim(s, chars)`, `distinct_by`; `x.match` as a field; empty `match { }` is a check error; undefined `{var}` inside test rules is a check error; starter renamed `open_session`/`close_session` (serve calls a zero-arg `start()`), its invariant comment truthful.
- [x] serving.md rewritten (statuses by kind, String → `{"result"}`, one path variable per pattern, auto-route coercion, `start()` hook, CORS); llms.txt: scheduler section, `has_state` caveat, `mock now`, `--strict`, `test --json`, one variable per pattern, bare calls in `rules`; sizes/counts no longer hard-coded; operations.md: slow handlers, `.soma_data` after a program change.

### Open
- [x] Guard binding: a guard local bound only inside a branch is a check error (it was `undefined_variable` at runtime).
- [x] `.soma_data` evolution: `soma serve` audits at start-up (instances in removed states, stored values violating an invariant) and warns; a re-typed slot is documented as undetected.
- [ ] No per-request time limit (documented).
- [ ] `soma example` on the live site still serves the previous corpus until the site is deployed (the landing block's 6th line depends on it).
- [ ] Parser: `-10..-2` range patterns, non-ASCII identifiers.

## Cycle 6 (2026-09-18, night) — 5 fresh agents on the cycle-5 binary

Scores: Ruby port (invoice/ledger with dates and money formatting) 17 invocations, identical output first run, 7/10; job queue with retries 23/62 62/62 `--strict` OK 8/10; LLM moderation pipeline 24, 60/60, cost proof transitive across cells (a hidden `think()` in the human cell is caught by `cost { tokens: 0 }`), 7/10; docs audit: 100 facts checked, 18 contradictions site/binary (builtin count 181/204/207, CORS not on every response, bus port "always", no verdict when check fails, `Int … 64-bit`, `m.keys` in reference, cost bound "advisory" vs error); adversarial c5: two real holes — a BigInt written to any slot read back as a **String** (a verified `n >= 0` counter bricked past i64::MAX), and `rows[0] = …` / `rows[0].id = …` / `rows.delete(0)` on a List slot silently dropped.

### Fixed
- [x] Storage keeps big Ints as Ints (`StoredValue::BigInt`, SQLite tag `bigint`, JSON `{"__bigint__": …}`); `type_of` says `Int` for every integer.
- [x] List slots: `rows[i] = v`, `rows.set(i, v)`, `rows.delete(i)` replace/remove by index (journaled, invariant-checked); out of range is an `index` error; delete out of range answers false.
- [x] Slot value types are enforced on every write (`Map<String, Int>` given a String → kind `type`, 400); 12 corpus programs declared `Map<String, String>` for records and were corrected to `Map<String, Map>`.
- [x] `buffer/hashmap/strbuf` (native-only) in an interpreted handler are a check error (they were a runtime "undefined function"); `sin cos tan atan atan2`, `regex_count/regex_replace/regex_match`, `read_stdin`, `write_str` now exist in the interpreter too, so the native vocabulary claim "same semantics" holds.
- [x] `format(fmt, args…)` (printf subset: `%d %s %f %.Nf`, widths, `-` and `0` flags); `to_fixed`, `floor_div`, `mod`, `divmod`, `round(x, d)` decimal-exact; dates: `parse_date`, `add_days`, `add_months`, `days_between`, `days_in_month`, and `format_date` correct before 1970.
- [x] `j.state = …` (keyword field names in assignment chains); `m.keys` / `m.len` pseudo-fields inside test assertions; `assert_fails … matching "text"` matches the kind too; `soma test --json` records carry `rule`, `message`, `raised`, `left`, `right`, and a JSON body is printed when check fails or no test cell exists.
- [x] Prover: `require n <= limit` where `limit` came out of a guarded slot chains that slot's invariant into the proof (`attempts <= 5` from `limits <= 5`); a `delete` cannot break a value invariant (only `size` clauses are re-checked).
- [x] `soma verify` prints `VERIFY FAILED — soma check failed` (one verdict line, always); `soma example --json` exits 1 on no match; `soma describe --json` lists sum types; `soma lint` no longer suggests renaming public handlers to `_x` (that removes the endpoint); a UTF-8 BOM is accepted; `assert`/`property` in a non-test cell is a check error (they never ran); `rows[0] = …` no longer hides a later `rows = …` from check.
- [x] serve: CORS on every response (static, dashboard, pre-handler 400s); `--no-schedule`; start-up prints `llm: MOCK … / provider … / NO KEY` when the program calls think.
- [x] Docs: SOMA_BUILTINS.md regenerated by build_site.py from the binary (reserved names dropped, `native` section marked); reference.md (Int arbitrary precision, `m.keys()`, ports conditional, `promise` is a note, try vs check errors, `[verify]` keys, `"""` interpolation); gotchas 13/15/16 show the current diagnostics, gotcha 20 and guarantees.md say an unprovable cost bound is an error; operations.md: `index` row, env-var table, `emit` between processes, `/version.json` is the website's, slot writes cost ~1 ms; serving.md: bus/WS conditional, `--no-schedule`; llms.txt: complete kind list, `deadlock_free`, keyword args / records / printf / `//` and `%` from Python, List-slot forms, `ensure`, dates, refinement is target-level.

### Open
- [ ] Refinement checks transition TARGETS; a removed edge whose `transition()` remains is a runtime `invalid_transition`, not a verify failure (documented; `[verify.before.X] requires` covers the edges that matter).
- [ ] `emit` across processes needs `[peers]`; a peer down at start-up is not retried (documented).
- [ ] `soma fix` repairs missing handlers and `--native-idiv` only (documented as such).
- [x] Variants round-trip through `to_json` / `from_json` as `{"_type", "_variant", …}` (they used to be a JSON string); `soma serve` renders every body with the same JSON writer (a returned variant is an object, not `{"result": Charged { … }}`).
- [x] `trace()` records the system prompt of each Think step.

## Cycle 7 (2026-09-18, morning) — 5 fresh agents on the cycle-6 binary

Scores: warehouse inventory (2 cells + router) 6 invocations to check+verify --strict+42/42 green, cross-cell atomic rollback and serialization held under 50 parallel confirms, 7/10; Java `Loan` port (BigDecimal HALF_UP, LocalDate, enum → machine) 15 invocations, first `soma run` byte-identical to Java, 7/10; pandas-style CSV job (20k rows) correct in 2 invocations but 104 s — `|> map` and `xs[i]` were O(n) per element — 4/10; LLM inbox assistant green at invocation 3, 34/34, cage claims held (validation, cost proof, approval gate, cross-cell rollback), 6.5/10; adversarial c7: two unsound findings — scheduled blocks kept their writes when they raised, and `kill -9` mid-handler left a half-committed handler on disk (writes were committed one statement at a time).

### Fixed
- [x] **Atomicity**: `every` / `after` ticks run through the same atomic path as handlers (rolled back when they raise); with persistent slots a top-level handler is ONE SQLite transaction on a process-wide connection (`BEGIN IMMEDIATE … COMMIT`, which also serializes processes) — a `kill -9` mid-handler leaves nothing. `.soma_data/` now lives beside the program regardless of the working directory; `soma run --fresh`.
- [x] **Performance**: lambdas capture only the names they use (the closure used to copy the whole environment — the 20k-row list — once per element); `xs[i]` / `m[k]` on a local index in place. `rows |> map(r => …)` over 20k rows: 104 s → 0.1 s.
- [x] `request` is never an endpoint (`GET /request/POST/%2Fcredit/x` ran a POST-only route); path segments reach `request` percent-decoded; every `soma serve` body and `soma run` output goes through the JSON writer (NaN/inf → null, variants as tagged objects).
- [x] Slot value types: a whole Float is not an Int (`1.0` into `Map<String, Int>` → kind `type`); an Int into a `Float` slot is stored as a Float; `Map<String, Pay>` takes only `Pay` variants; `match` with variant arms on a non-variant value raises instead of answering `()`.
- [x] A bare state name is that state at runtime (`transition(id, CLOSED)` — check and verify already accepted it); `div_round` (HALF_UP integer division, exact money), `months_between`, `chr`/`ord`; `stdev` / `variance` are sample statistics like Python's, `pstdev` / `pvariance` population; `think_json` raises kind `json` on a non-object reply (```json fences tolerated); mocked `think` costs ~4 chars/token so `budget` is testable; `trace()` under serve keeps the last 1000 steps process-wide; guard failures name the condition; `assert_fails … matching` failure names kind and message; the test runner echoes string literals with their spaces.
- [x] Prover: `require` directly inside a loop body narrows (no break/continue), sibling handler calls with arguments are followed for their return range, and each ⚠ says WHY (`because `x` is a parameter (narrow it: …)`, `… reassigned 2 times (a loop accumulator?)`).
- [x] `[native]`: `strbuf()` without a capacity compiles; `if hm_has(m, k)` and `let ok = hm_has(…)  if ok` compile in BigInt mode; `soma fix` mends `;`, `=>` in match arms, `-> T` on handlers, `null`/`None`/`True`/`False`; `if s == Pending { … }` parses (a unit variant before a block is not a record literal).
- [x] Docs: llms.txt (transactions, `--fresh`, money recipe, dates, data builtins, think_json/tokens/trace), operations.md (SIGTERM, ticks, transaction), serving.md (`request` not routable, decoding, error body shape), reference.md Storage rewritten around `Map<String, Map>` (no `to_json` advice); corpus: loan_amortization rounds HALF_UP, triage_agent header truthful.

### Open
- [ ] Relational invariants across slots (`reserved <= on_hand`) — restructure the schema so the property is single-slot (documented).
- [ ] No auth hook, no per-request timeout, no header access under serve (proxy's job; documented).
- [ ] Native boundary takes scalars only (a List cannot cross) — documented; the interpreter is now linear, which removes most of the pressure.
- [ ] Dashboard shows verification, not live traces or token totals.

## Cycle 8 (2026-09-18, morning) — 5 fresh agents on the cycle-7 binary

Scores: TypeScript Kanban port green in 4 invocations, 38/38, WIP invariant proven, 6.5/10; cold site evaluation → "try", 7/10, a strictly verified booking service in 7 invocations; 4-cell fulfilment saga (Erlang/Temporal persona) 5/10 — a bare ambiguous call resolved at random across runs; adversarial prover audit: no runtime violation, but eight FALSE ✓ proofs; native numerics: sieve 10^8 in 0.7 s, 512²×200 Jacobi in 0.08 s, results bit-identical to the interpreter, 5/10 for the boundary limits and four check-passes-rustc-fails constructs.

### Fixed
- [x] **Prover soundness**: a `require` inside a loop narrows only that loop's per-iteration locals (a loop can run zero times — the cycle-7 rule was unsound); match-arm and lambda bindings shadow narrowed names; intervals beyond 2^53 are unknown (i64::MAX + 10 rounded to i64::MAX in f64); `require cur + n <= K` on the written expression itself is a fact; an early `if n >= 1 { return … }` narrows what follows; termination of `down(n - 1)` needs a lower-bound base case on an Int (`if n <= 0`), not `== 0`; `every` / `after` blocks and `delegate("Cell", "h", …)` count in cost proofs; a self-loop no longer breaks liveness; an interpolated transition target is dynamic; liveness / `eventually` lines state that guarded edges are assumed passable; orphan `[verify]` properties (no machine) fail instead of "Temporal: 0 passed".
- [x] A bare call to a name two cells define is a check error in test cells too, and runtime dispatch is deterministic (declaration order); slot `.keys` / `.values` sorted in every backend; `emit` with no listener is a check warning; `soma serve` lists endpoints (no private handlers); serve accepts whitespace-padded JSON bodies; stored records keep their field order.
- [x] `[native]`: `soma check` refuses what codegen refuses (buffer re-binding, list/buffer returns, `range(a, b, step)`, a literal `/ 0`); an Int that overflows i64 while stored into a Buf is a `range` error (BigInt retry) instead of a second overflow panic; `write_str` flushes; codegen errors render the expression, rustc failures show only the error blocks and the path of the generated crate; the crate silences its own lint noise.
- [x] The starter and `gate_appointment` are `--strict` clean; landing page links `/status` (maturity), guarantees, serving, operations; dead links fixed; `soma run` audits stored data; `trace()` records the system prompt; `--strict` repeats the ⚠ lines by the verdict.

### Open
- [ ] `cost { }` is a static bound on output tokens; `set_budget` is the runtime cap (documented). A `Buf` cannot cross handler boundaries (documented).
- [ ] Cross-cell invariants (`Orders.cancelled ⇒ Warehouse.released`) are tested, not proven.
- [ ] `{k: v}` map literal, optional parameter types (`String?`) — language changes, not taken.

## Cycle 9 (2026-09-18) — maintenance, HTTP client, Python port, fuzzing (regression replay cut short by a rate limit)

Scores: day-two migration of `warehouse_reservations` (added fields, renamed state, sweeper, report) done live on the served database, 6/10; two services over HTTP 4/10 (server side 7, client side 2); Python `Library` port green at invocation 2, values identical to CPython, 7/10; front-end fuzzing: one compiler crash (10 000 nested `if` → stack overflow, SIGABRT) and nine wrong acceptances.

### Fixed
- [x] **Crash**: nested blocks share the 400-level depth budget with expressions (an error instead of an abort); an error echoes a 120-column window of the line, control characters escaped.
- [x] **Lexer**: `1_000_000`, `0xFF`, `0b101`, `0o17` (they lexed as `1` + identifier `_000`); `12abc` is an invalid number; `\u{1F600}`, `\r`, `\0` escapes (`\u{…}` reached the interpolator); a value on its own in the middle of a block is a check error (`let j = 1 2`).
- [x] **Check**: `break`/`continue` outside a loop; `transition()` arity; `!` needs a Bool (`!0` was `true`); negative range patterns `-10..-2` (and a `-1` pattern after an arm is not a subtraction); a bare call ambiguous between cells stays an error; type messages show Soma values, never a Rust dump.
- [x] **HTTP client**: one implementation for `http_get/post/put/patch/delete`; default timeout 30 s and `timeout` honoured on every method (POST ignored it and waited 40 s); non-2xx returns `{error, kind: http_status, status, body}` with the upstream body; `timeout` / `refused` / `network` kinds; `headers`; unknown options refused; `mock http_post …` scripts the builtin in tests (it used to be accepted and hit the network) and an unscripted call prints a note.
- [x] **Migration**: `*` edges no longer fire from states the program does not declare (the audit said "no transition" while a sweeper could cancel them); the audit also reports values of another type than declared and slot tables no slot declares (renamed slots), in `soma run` too; operations.md has a migration section.
- [x] **Prover**: a require counts for the writes of its own block and nested blocks (a conditional require dominating the write now proves it; an early exit never narrows its own branch); `size` / `len(slot)` invariants are provable (`require len(rows) < K` before the one adding write, outside loops; a set on a key known to exist does not grow); reasons name only numeric parameters and say what a size bound needs.
- [x] Python habits: UFCS on slots (`rows.any(…)`), `() + 1` names the `?? 0` fix, a write to a loop copy warns; exact CLI Ints beyond 64 bits; `warehouse_reservations` is `--strict` clean.

### Open
- [ ] Outbound HTTP holds the process-wide handler lock (documented; timeouts bound it).
- [ ] No migration command or schema version (a documented recipe instead).
- [x] `let match = 1` is refused with a fix; `cell test` without `rules` is a check error.

## Cycle 10 (2026-09-18, after the 2.5.0 release) — regression replay of 25 fixes, Go quota service

Scores: all 25 fixes held on their reported path; six defects in neighbouring variants. Go port (token bucket, quota, monthly reset) green in 5 invocations, 20-way burst admitted exactly 10, 6/10.

### Fixed
- [x] Slot `.entries` returned the values, and a SQL `LIKE '__%'` (where `_` is a wildcard) dropped every two-character key from `list()` after a restart.
- [x] `parse_date` weekday is ISO (1 = Monday; it was 1 = Sunday) and the format is strict (`"2026-3-1"` is kind `date`).
- [x] `[native]` hashmap: an i64 overflow in a key or value is a `range` error, not a leaked Rust panic of kind `type`.
- [x] `len(xs)`, `nth(xs, i)`, `xs.len`, a field of a local record, a List slot's `rows[i]` / `rows.get(i)` / `rows.len`: no copy of the whole list per call (5 000 calls: 2.2 s → 1 ms).
- [x] Prover: if-expressions and matches with bounded arms are bounded; the "narrow it" hint states the bound the open clause needs (`require n <= 3`, not `>= 0`); termination is analysed in cells without a state machine too.
- [x] `mock now 1700000000.25` and `mock now_ms …`; HTTP headers reach `request` as an optional fifth `headers: Map` parameter; a trailing `Map` parameter may be left out by any caller (`add(1)` for `on add(a: Int, opts: Map)`).
- [x] A scheduler tick error is logged once; the `() - 1` hint says `- 1`; `break` in a lambda inside a loop is an error.

### Open
- [ ] Passing a large list to a handler copies it (value semantics; documented). Structural sharing would remove it.
- [ ] Invariants over two slots / record fields (`used <= limit`): model as one slot (headroom) — documented.
