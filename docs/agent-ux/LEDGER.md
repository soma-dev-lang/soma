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

## Cycle 11 (2026-09-18) — the public install path, and an attack on the newest features

Scores: public path on Apple silicon 9/10 (4 s install, checksum matches, versions and docs agree, service green on the first write), 7/10 overall because Linux agents fall back to a source build. Attack: one data-corruption bug and four false size proofs.

### Fixed
- [x] **Data corruption**: `x[i]` on a local, parameter, loop or lambda variable named like a memory slot read AND wrote the slot (`let rows = [1, 2]  rows[0] = 99` rewrote the persistent `rows`); the local now wins everywhere, `rows.push(x)` on such a local is refused, and check warns when a `let` hides a slot.
- [x] **Prover**: a size proof no longer credits a write when a called handler or an emitted event also adds to the slot, when the write is inside a lambda, or when the "existing key" was re-bound; `len(slot)` in an invariant is the written value's length, not a size (docs said the opposite — `rows.size <= K` is the form).
- [x] **Security**: `request` parameters 4 and 5 are bound by name (a 4-parameter `request(…, headers: Map)` received the query — `?authorization=` forged a header); repeated headers are joined; `soma run` lower-cases header names like serve.
- [x] **HTTP client**: a body that stalls is `timeout`, an unfollowed 3xx is `http_status`, a body over `max_bytes` is `too_large` (all three returned a success), option values are validated, a String body is text/plain.
- [x] **Audit**: List↔Map re-declaration, `size` invariants over List slots, removed cells (serve only — `soma run` shares directories), the cell named in orphan warnings.
- [x] One indexing rule for local lists, List slots and strings; `nth` counts negative from the end; `emit` reaches the emitting cell's own listener (deterministic order); `/add/1?step=7` fills a trailing `opts: Map`; the arity message mentions optional Map parameters.
- [x] Installer: version from the latest release (not `main`), SHA256 verified, no `sudo`, source build at the release tag; `verify`'s verdict on stdout; `lint` knows `for k in slot.keys { slot.get(k) }`; serve logs ticks that commit writes.

### Open
- [ ] Release binaries for Linux (x86_64, aarch64) and Intel macOS — needs a CI build (GMP); the installer builds from source there.
- [ ] A non-ASCII header value is dropped by the HTTP library before Soma sees it.

## Cycle 12 (2026-09-18) — a Rust port and a line-by-line audit of llms.txt

A Rust payment-protocol port (no false ✓ — the prover held) and an agent that
tested every claim of llms.txt, serving.md and operations.md (~140 claims, 82
runs): 11 failed, two of them serious.

### Fixed
- [x] **Dispatch**: a bare call inside a cell to a handler name another cell also defines ran the OTHER cell's handler (a HashMap walk); the calling cell's own handler now wins, then declaration order. `Orders.pack(id)` reaches the other one; the error from a third cell says "qualify it".
- [x] **Serve**: the start-up hook (`init`, else `start`) was also a public endpoint — any GET could re-run it; it is now routed like a `_` handler. Static `.txt`/`.md`/`.csv`/… get real content types.
- [x] **verify**: always ends with a verdict line (a parse error, a bad soma.toml or an unreadable file printed none); check errors print on stdout with the verdict; a check failure stops verify before any proof; plain-language messages for dynamic targets; the ⚠ for an invariant now says what range the written value has and which `require` would prove it.
- [x] **Native**: check refuses a buffer passed to a sibling handler; an Int-valued call (`floor`, `len`, a sibling) in a Float expression is converted (it was a rustc "cannot add i64 to f64"); a constant overflow is the documented `range` error, not a rustc lint.
- [x] **Check**: `require` without `else` names the fix; `and` / `or` / `not` name `&&` / `||` / `!`; a `"` inside `{…}` says to bind the value first; a builtin call with the arity a same-named handler does not take is neither type-checked against the handler nor counted as recursion by verify; `soma test` prints whole error blocks.
- [x] **Docs**: a bare `size` invariant bounds every slot; `body: Map` in a test takes a Map; `require` examples carry `else`; token caps, `forall` sampling, redirects, local-map key order, `nth` negative indices, `keys(record)` spelled out; `soma docs agent` is rebuilt from the edited file.

### Open
- [ ] A formerly terminal state becoming non-terminal (new edge) gives no warning; an `after.X never` counter-example starts at X, not from the initial state; map-literal writes are not bounded by the prover; a handler's source states are not checked against its callers.

## Cycle 13 (2026-09-18) — a multi-tenant ticket backend, end to end

Docs 8/10: 79/79 tests, verify --strict green, 600 mixed requests and races
correct under serve. Eight bugs, two of them in the HTTP layer.

### Fixed
- [x] **verify**: a `[verify] cells` entry that is a near-miss of a cell of the file (or names a cell with no state machine) is an error — every property was silently skipped and verify said OK, even with `--strict`.
- [x] **Security (serve)**: a request body carrying `_status` is refused — a handler echoing a client object let the client pick the HTTP status and inject response headers; a status outside 100–599 is a 500 (it was wrapped mod 65536: 65736 went out as 200); `response()` refuses one; 204/304 carry no body; a non-UTF-8 body is `400 json` (it read as `map()`); `%ZZ` stays literal (it decoded to NUL); a non-object JSON body says "must be a JSON object".
- [x] **cost**: the proven `tokens` bound is said to cover reply tokens (the max_tokens caps), not prompts; the `echo`/`fixed:` mocks stop at max_tokens like a provider.
- [x] **Budgets**: a test rule starts with no token budget (one rule = one request, as under serve); `tokens_remaining()` never goes below 0.
- [x] **Prover**: an `if` condition narrows the writes of its branch (its negation those of `else`); `for k in rows.keys { rows.set(k, …) }` and `let r = rows.get(k)  require r != ()` no longer count as growing the slot.
- [x] **Check**: a reserved word used as a name says "reserved word — rename it"; `after`/`every`/`ensure` bound by `let` can be read; an Int-answering builtin (`regex_match`) used as a Bool is an error; `soma test` names the line that raised; `describe` keeps `except [...]`.
- [x] **Docs**: forall takes one Int variable; test cells may hold helpers; guards bind for every handler targeting the state; repeated query keys, OPTIONS; budget scope.

### Open
- [ ] `eventually` has no fairness option (a legitimate assigned ↔ waiting cycle fails it).
- [ ] `response()` with a bad status raises kind `type` (answered 400) — a server bug that reads as a client error.

### Cycle 13 — attack (same binary)

Six false proofs, two data-corruption bugs, one data-loss bug, four security issues.

- [x] **Data corruption**: a persistent List slot never enforced `rows.size <= N` on `push` (the size came from the map table: 0); match-arm bindings overwrote the enclosing variable — and a failed guard DELETED it (a guard read the wrong `amount` and was bypassed); arms are now scopes, in check too.
- [x] **Data loss**: a state machine in a program with no persistent slot kept its instances in memory — every transition forgotten after the handler (`soma run` / `serve` now persist them in .soma_data).
- [x] **False proofs**: NaN passes an early-return narrowing on a Float (`if x > 10.0 { return }`) — negated comparisons now only narrow NaN-free values; a match binding / lambda parameter / nested `let` shadowing a slot or a local; growth through another cell (`B.relay` → `A.extra`) and through emit listeners; termination with a re-assigned or re-bound parameter, and call cycles through other cells and emits; a cost bound ignoring emit listeners.
- [x] **Security**: a GET reached state-changing handlers (CORS `*`: any page could write) — 405 now; clients could forge records and sum-type variants with `_type`/`_variant` (body, path, query) — refused; with both `start` and `init`, `start` was an endpoint; private handlers listed in the dashboard.
- [x] **Wrong results**: `transition()` from a cell without a machine moved a random other cell's machine (now only when the program has exactly one); `NaN`/`inf` path and CLI arguments; big Ints in paths; `+` in paths; a trailing slash argument; `soma run z.cell nan` called another handler; `soma run` ran programs failing check.
- [x] **Native**: negative shift counts hung (4-billion-bit shift) or returned 0; `sqrt_int` of a negative returned 0 and was inexact past 2^52; `round(x, 2)` dropped the digits; `sb_push_char` took the low byte; check now runs the native code generator and reports its refusals, and refuses an Int variable later given a Float.
- [x] Check no longer refuses escaped string literals inside `{…}` (the runtime evaluates them).

### Open
- [ ] `emit` inside a listener for the SAME event is not delivered (documented now; a queue with a depth bound would be the real fix).
- [ ] Native `/` answers a Float where the interpreter answers an exact Int.
- [ ] Deep (100k) nesting in `to_json` overflows the stack; nested-map construction is quadratic.
- [ ] `from_json` of a String body can still build variants a type does not declare.

## Cycle 14 (2026-09-18) — an expense pipeline ported from Python, and an approval agent

8/10: byte-identical output to Python on 5 datasets, 51/51 tests, the per-employee-month
limit PROVEN (7 mutants all rejected), kill -9 and 50-way concurrency exact.

### Fixed
- [x] `"^[0-9]{4}"`: a `{4}` / `{2,3}` segment is literal text (check passed, run raised "undefined variable: 4").
- [x] A `while` loop with no think() in it no longer makes a `cost` bound advisory.
- [x] `transition(id, "draft")` into a state no declared edge enters (the initial state) is a check error — verify had ✓ refinement and every call failed.
- [x] An imported file's test cells no longer run (with the importer's file and lines) under the importer's `soma test`.
- [x] `use helper` imports helper.cell beside the program (it said "package not installed").
- [x] `read_csv` is RFC 4180 (quoted commas, `""`, multi-line cells, BOM); quoted cells and `007` stay Strings; `map("raw", true)` keeps text; `write_csv` quotes newlines.
- [x] Between cycles: a variant a sum type does not declare cannot enter a slot of that type; `response()` with a non-HTTP status is kind `response` (500).

### Open
- [ ] Updating a list nested in a local record (`g[0].rows = push(g[0].rows, i)`) is quadratic.
- [ ] No HALF_EVEN rounding builtin; no JSON-schema helper for think_json.

### Cycle 14 — attack (same binary)

- [x] **Security (serve)**: a map returned from client data (`{"_status":200,"_body":"<script>…","content-type":"text/html","set-cookie":…}` stored then echoed, a query/header/form Map) became a raw HTTP response — stored/reflected XSS, forged cookies, open redirects. Only maps built by `response()`/`html()`/`redirect()`/`sse()` (an unforgeable mark) are responses now. Header values with CR/LF, invalid names and framing headers (Content-Length…) are dropped. Writers answer 405 to every method but POST/PUT/PATCH/DELETE (case-insensitive). The bus port refuses HTTP requests (cross-protocol POST from a page) and `_private` events.
- [x] **False proofs**: reassignment inside a match arm / `try` / if-expression was invisible; a `require` AFTER a write (or after an early return) was credited to it — an invariant is checked at the write, so only later writes are narrowed (size proofs too); writes in `every` / `after` blocks were not writers; termination through `x |> f()`, `Cell.f()`, a base case that recurses, and `while true` in an `every` block (it hung every request).
- [x] **Data**: NaN / ±inf nested in a persistent Map or List read back as `()`; a lambda stored in a slot became the text "<lambda>" (refused now); `soma run` with a near-miss handler name ran another handler with the typo as argument; with `start()` and `init(x)` serve called `init()` and never `start()`.
- [x] **Native**: `sb_push_char` statement form took the low byte; `bit_test(-1, 64)`; `pow_mod` of a negative base (and a negative exponent: interpreter answered 1, now both raise); `floor` of a big Int; check refuses mixed Bool/String/number variables and returns, `to_int` of a String, ordering Strings, growing a String parameter alias — each passed check and failed in rustc. Interpreter `gcd(i64::MIN, 0)` was negative.
- [x] UFCS on a user handler (`(n + 1).dbl()`) runs it; `for [loop_bound(N)] x in xs` placement documented; a lambda assigning an outer local is a warning.

### Open
- [ ] Native `/` answers a Float where the interpreter answers an exact Int (6 / 3); `0 / -1` is `-0.0`.
- [ ] A guard reads a loop/lambda/match binding that shadows the handler's local of the same name.
- [ ] verify's reason for an invariant that does not name the slot ("`1` is only known to lie in [1, 1]").

## Cycle 15 (2026-09-18) — a realtime tic-tac-toe arena ported from TypeScript

7/10: identical results to the TS reference on 5 scripts (up to 2,670 ops) and a live
run with WebSocket, SSE, concurrent moves, kill -9 restarts and timeouts; 33 temporal
properties and all invariants proven — after routing around B1.

### Fixed
- [x] A `delete` on a slot with `size <= K` fell back to "runtime-checked" (a bounded queue could not pass --strict): a delete cannot grow a slot.
- [x] `[loop_bound(N)]` was trusted: a `cost` bound "proven" at 100 tokens spent 300. More iterations now raise kind `loop_bound`; a literal list longer than the bound is a ⚠ and the cost uses the real length. A loop over a literal list counts as bounded.
- [x] `index_of(xs, x)` works on Lists (it answered -1 for everything); a non-String/List argument raises.
- [x] `publish` / `emit` pushes reached SSE and WebSocket clients from handlers that were rolled back — pushes now leave at commit.
- [x] WebSocket: cross-origin browser connections are refused; a raise answers `{"error", "kind"}`; messages are logged; `ws` is not an HTTP endpoint.
- [x] Event listeners (targets of an `emit`) are not HTTP endpoints; `publish` counts as a state change (GET → 405).
- [x] Docs: a Realtime section (on ws, publish, sse, commit-time pushes).

### Open
- [ ] No per-client / per-stream WebSocket routing; no SSE replay ids.
- [ ] Serve log lines have no timestamps.

### Cycle 15 — attack (same binary)

- [x] **Data corruption**: a String slot value holding JSON text (`"{\"x\": 1}"`, `"[1, 2]"`) came back a Map / List — String slots now give back their text.
- [x] **Security (LLM)**: the model could call ANY handler of the agent cell (a private `_admin`, a state-changing `pay`) and reach the network past the tool capabilities; only the face's `tool`s are callable now, and a tool call that raises is rolled back (its writes were kept).
- [x] **Crash**: a deep `[native]` recursion overflowed the thread stack and aborted `soma serve`; past 20,000 native frames it is the `stack_overflow` error.
- [x] **False proofs / 405 bypass**: calls inside a string interpolation (`"{bal.set(k, n)}"`, `"{_more(x)}"`), UFCS (`x._more()`, `p.think()`), pipes and literal `delegate(...)` were invisible to invariant, termination, cost and GET→405 analyses — every analysis now sees them (the runtime is unchanged).
- [x] **Cost**: a think() in an agent with tools makes up to 10 provider rounds — the bound counts them; `map("max_rounds", N)` caps them (the refund_agent example uses 1).
- [x] **soma.toml**: unknown sections (`[verfy]`) and `[agent]` keys are errors; `cells = ["b"]` for `B` says so; `requires = []` is an error.
- [x] **Data**: path/query/CLI text keeps its spelling for String parameters (`00123` was stored as `123`); `remember()` has its own journaled storage (it was never rolled back and wrote into a random user slot); bus / `subscribe` events refuse forged `_type`/`_variant` and private handlers, log failures, and send valid JSON (an `inf` dropped the event).
- [x] Replay: `--at` parses ISO 8601 (garbage replayed everything); the fix suggestions and the builtins doc no longer claim think/http/read are replayed from the log. Test cells start with an empty trace(); a taken WebSocket / bus port is exit 1; one CORS header; verify's hints substitute on the AST with parentheses (`"admin"` became `"1dmin"`); interpreter bit_set/bit_clr/bit_test/bit_next on arbitrary-precision Ints.

### Open
- [ ] Runtime errors in an imported file point into the importing file.
- [ ] A JSON body number beyond Float range reads as `inf`.

## Cycle 16 (2026-09-18) — Monte Carlo, graphs and big integers ported from Python

7/10: all three ports bit-identical to Python (native 35-50x faster than CPython,
interpreted BigInt faster than CPython); a persistent job queue survived kill -9
mid-job. The native backend disagreed with the interpreter in four places.

### Fixed
- [x] **Native argument types** were not checked: `sq(2.5)` squared 2.5's bit pattern, `half(3)` read 3 as 5e-324, a String gave an index panic (a Float `paths` would have looped 4.6e18 times holding the lock). Native handlers type-check their arguments like interpreted ones (kind `type`).
- [x] **Native `random`** ignored its arguments, answered Floats and used a fixed seed; both backends now share a clock-seeded splitmix64 (`random()` has 53 bits — the interpreter's had 6 decimals).
- [x] **Native buffers** accepted a Float index (truncated silently): a check error now.
- [x] **Native `loop_bound`** was not enforced (101 iterations under a bound of 100): checked in Direct and BigInt code.
- [x] `while [loop_bound(N)]` satisfies verify's termination check (it is now enforced everywhere).
- [x] `to_int` / `floor` / `ceil` / `round` of a large finite Float give the exact Int (they raised); native falls back to BigInt for them.
- [x] `bit_len` of a BigInt is exact (the interpreter estimated from the decimal length); `gcd` works on BigInts (it truncated them).
- [x] A 64-bit-plus Int literal in [native] says how to build it; the shl doc's mask no longer suggests a literal native refuses; a missing `cargo` says to install Rust; docs: cargo, .soma_cache, float printing, native indexes.

### Open
- [ ] Native errors carry no line number.
- [ ] Interpreted numeric loops are 4-8x slower than CPython.

### Cycle 16 — attack (same binary)

- [x] **False proofs**: a size proof accepted a key whose value changed between the require and the set (`p.k = j`, `ids[0] = j`, `cur.get("k")`, `to_string(random(2))`) — keys must be pure texts over unchanged names; recursion placed before the base case; `delegate()` to a computed handler name.
- [x] **Data corruption / forgery**: storage tables `<Cell>_<slot>` collide (case-insensitive SQLite, `_` joins, `_log` tables, machine tables `__sm_`) — slots `acct`/`Acct`, cell `User` slot `pass_hash` vs cell `User_pass` slot `hash`, a slot rewriting machine states, a `_log` collision crashing every command: a check error now.
- [x] `next_id()` lived in the user's first Map slot under `"__next_id"` (a user key of that name reset it; a List first slot answered 1 forever): its own table now (legacy value carried over once), reset per test cell like `remember()`.
- [x] Values nested past 100 levels are refused (they came back as a String); nested generic slot types are enforced (`Map<String, List<Int>>` took `["x"]`); an invariant over a List value holds for every element and a List in an `if`/`while` condition is an error (a non-empty 0/1 mask read as true).
- [x] **Security**: `_type` in a FORM body or the QUERY string forged records; a GET reached state changes through an LLM tool or `next_id()` (think and next_id count as state changes); the bus refuses `request`/`ws`/`start`/`init` (an EVENT bypassed the WebSocket origin check); on a loopback bind only localhost origins open a WebSocket (DNS rebinding had Origin == Host); `html()` injects htmx only for a real `hx-` attribute.
- [x] **LLM**: tool capabilities with URL patterns (`https://api.x.com/*`) never matched — `*` patterns work; tool arguments are type-checked (garbage ran the tool with ""); a think() inside a tool has its own conversation and makes a cost bound unprovable; the budget is charged when a provider omits `usage`; cost takes the max over if/else and match branches.
- [x] **Native**: a negative `bit_set` index hung serve (and `bit_clr` / `bit_next` differed); BigInt-mode `to_string` of an integral Float printed `8`, not `8.0`.
- [x] Guard names bound only after `transition()` are a check error; `soma test --json` keeps a sampled property's text whole.

## Cycle 17 (2026-09-18) — a multi-agent research & write pipeline

7/10: four agents + a coordinator machine, 14 temporal properties proven, 52 offline
tests, 30 concurrent jobs against a fake OpenAI server with outages, budget
exhaustion, approval parking and kill -9 recovery.

### Fixed
- [x] `every` / `after` ticks (and init, bus, websocket interpreters) ignored `[agent]` in soma.toml — a tick sent the env key to api.openai.com instead of the configured url, and ignored `[agent] mock`.
- [x] The `cost` bound of a cell calling an agent with tools in ANOTHER cell counted one round (600 real vs 300 "proven"): each callee carries its own cell's rounds.
- [x] think() shared ONE conversation across agent cells in a request (B was sent A's prompts and data, and B's system prompt was dropped): one context per agent cell, and an explicit system prompt replaces the previous one.
- [x] The start-up audit called next_id's / remember's own tables orphaned slot data.
- [x] An unknown think() option (`max_token`) is an error (it was ignored: 2048 tokens); `timeout_ms` is accepted as documented.
- [x] `describe --builtins` no longer says think/http are "pure" on replay; the agents page no longer promises deterministic replay of LLM calls; docs: per-cell context, tick config, offline tests with a key, mocking a List answer.

### Open
- [ ] A slow think() holds the process-wide handler lock (9 s GET while a job researches).
- [ ] No schema option for think_json; no way to script tool-call rounds with `mock think`.

### Cycle 17 — attack (same binary)

- [x] **False cost proofs**: a think() in an agent WITHOUT tools was costed at one round while a model answering with tool calls was asked again up to 10 times (proven 100, spent 300) — one round without tools; `max_tokens` 0 / negative was "peak 0" while 2048 was sent — refused at run time and not a bound.
- [x] **Security**: a redirect escaped a tool's URL capability (no redirects inside a capability-scoped tool); `read_file` / `write_file` / `ws_connect` / `connect` / `subscribe` ignored capabilities (refused inside a scoped tool); `_type` as a request HEADER forged a record in a `headers: Map`.
- [x] **Data**: `.push()` on a Map slot wrote rows it never read back (refused); `rows.set("0", 7)` on a List slot went into an invisible table (refused); a renamed state machine restarted every instance silently (serve warns now); a JSON `null` passed the Map/List parameter check under HTTP; List<Int> / Map<String, Int> parameters check their elements.
- [x] **Wrong results**: BigInt → Float truncated (native and interpreter) — rounded to nearest now, so native BigInt mode matches the interpreter; tool JSON schema advertised Map/List as strings and Int as number; bus events reached only the router cell.
- [x] **Crash / exhaustion**: `soma run` aborted on a 45,000-deep value (512 MB main stack now); a non-reading SSE client grew memory without bound (per-client queue of 1024, then dropped).
- [x] Dashboard: invariants are "runtime-checked", not ✓, and author text cannot close its inline script; verify's verdict names an empty `[verify.before]` list correctly; docs: try keeps plain locals, float printing in JSON, request routes and GET, SSE queue.

### Open
- [ ] Storage tables can still collide ACROSS programs sharing one directory (check sees one program).
- [ ] A variant whose FIELDS changed is not reported by the start-up audit.

## Cycle 18 (2026-09-18) — a double-entry ledger ported from Python

8.5/10: identical reports to the reference on 3 streams of 5,002 operations, verify
--strict OK with 29 checks + 12 temporal properties, 4,200 concurrent requests with
kill -9 restarts and live period closes: trial balance 0 throughout. No false proof.

### Fixed
- [x] `if x > 0 { … } else { … }` narrowed the `else` for an Int parameter but not for `let x = amt` — once-bound Int locals are NaN-free like Int parameters.
- [x] An untyped parameter (`on f(x)`) says "needs a type … `x: Any`" (it was "expected ':'"); `const` / a cell-level `let` say there are no constants and how proofs need the literal twice; the section list names the real keywords.
- [x] Docs: the gotchas' untyped `request` example; `Any` in the types table and llms.txt; property scoping with several machines; literal bounds in invariant and require.

### Open
- [ ] No per-machine scoping of `[verify]` properties; no named constants.
- [ ] A property naming a state no machine has is printed as a vacuous ✓ before the error.

### Cycle 18 — attack (same binary)

No false proof found. Fixed:
- [x] **Security**: the bus ran ANY public 1-argument handler of any cell (`EVENT drain {}`) — only events this program emits or soma.toml `[bus] accept` lists; `//withdraw/…` (collapsed slashes), `/withdraw?id=…` and `/signal/withdraw` reached a handler around `request`'s authenticated route — paths are canonical and a handler an explicit route owns is reachable only through it.
- [x] **Data**: `agg` / `sum_by` / `avg_by` / `max_by` / `filter_by` went through i64 (9.99 summed as 9, 2^70 dropped) — exact Ints, real Floats, averages like `/`; Int-vector `+ - *` went through f64 — exact; `from_json` / literals could store a variant with missing, extra or mistyped fields, and move a typed machine with an undeclared variant — checked; CSV round trip (`()` as "null", edge spaces lost, raw trimmed).
- [x] **Native**: `bit_set` / `bit_clr` at bit 63 wrapped; `shl` had no size cap (301 MB, 88 s holding the lock); a mixed Int/Float handler named `sqrt` "unknown".
- [x] **Check gaps**: an undefined name in `require … else Tag "{x}"`; guards read by `every` / `after` transitions; `loop_bound(zb)` / `loop_bound(2.5)` were silently ignored.
- [x] Smaller: `to_fixed` digits capped at 15 / negative digits ignored; `add_days` overflow and years past 9999; `days_in_month(2026, 13)`; `response(101)` hung the client; a JSON body `1e400` read as `inf`; two `request` cells warn; `abs` doc.

### Open
- [ ] Native `/` answers a Float where the interpreter answers an exact Int (`to_string(n / 3)`, quotients past 2^53).
- [ ] `to_int` of a big Float into a `let` in a Direct-only native handler raises instead of promoting.


## Cycle 19 (2026-09-18) — an IoT telemetry pipeline across processes (`[peers]` bus)

### Fixed
- [x] **Bus**: an outbound `--join` link dispatched any incoming event, `[bus] accept` included or not — the same filter as the listening side; an accept-only receiver did not open the bus; a handler listed in `[bus] accept` was also an HTTP endpoint (forgeable) — hidden from routing; a cross-process `emit` left before the handler committed (sent by a handler that then raised) — journaled and sent at COMMIT like SSE/WS pushes; a line with no newline grew the receiver without limit — 16 MB cap.
- [x] `Store.config.get(k)` from another cell passed check and raised "undefined variable: Store" — a check error naming the accessor handler to write.
- [x] Performance: every `set` counted the slot even without invariants (COUNT only when the slot has invariants); `len(slot)` materialized every row (counted by the backend).
- [x] Native: `x = to_string(x)` on a number local blamed "BigInt mode" — says the local keeps the type of its `let`.
- [x] Docs: the bus wire format, broadcast to every peer at commit, no reconnect after a peer restarts, `[bus] accept` in the start-up rule.

### Open
- [ ] A peer that restarts is not reconnected (documented; restart the sender or re-join).

### Cycle 19 — attack (same binary)

False proofs found and fixed:
- [x] **`require` was invisible to every analysis**: `require r(n) > 0`, `require think(p) != ""`, `require … else Big "{r3(n)}"`, `require rows.set(k, 1) == ()` gave "structurally terminate", "bound proven — peak 0" and "no handler writes to guarded slots" — the calls in the condition and the detail are exposed to termination, cost and the invariant prover.
- [x] **Transition guards with effects** (a handler call, think(), a slot write) ran on every `transition()` unseen by the proofs, and an undefined function in a guard passed check — a guard is a pure condition (check error).
- [x] **Size proof after a delete**: `require m.get(k) != ()  m.delete(k)  …  m.set(k, 2)` was "proven" — a delete in the handler (or a handler it reaches) drops the key-exists fact.
- [x] **NaN on Float slots**: `x >= 0.0` "proven" for `abs(v)` and `(x.get(k) ?? 0.0) + abs(v)` — a computed Float may be NaN: runtime-checked unless it is a constant or a slot read ± a literal.

Other fixes:
- [x] **Security**: routes written as `if starts_with(path, "/wipe/")`, `path == "/reset"`, guard arms or `split()` did not own the handler — `POST /wipe/a` skipped `request`'s auth. A public handler `request` calls (directly or through its helpers) is reachable only through `request`; tested literal paths are routes. `/signal/<not a handler>` is 404 (was 500).
- [x] **One request killed or wedged serve**: `pad_left("x", 2^62)` and `format("%{w}d")` aborted the process, `zeros`/`eye`/`ones`/`reshape` with huge sizes panicked past `try`, `range(0, 2^62)` grew without bound, `sleep(-1)` never returned (holding the handler lock) — one builtin call builds at most 10^8 elements (kind `range`); sleep is 0..86,400,000 ms.
- [x] **Data**: `next_id()` handed out the same id twice when a failing `try` rolled it back after the id escaped into a local — ids drawn in a failing try are kept (a failing handler still burns none); a sum-typed parameter took any value — `on f(p: Pay)` takes a Pay variant (a unit variant's name from the CLI); variant constructors and slots check `List<Int>` / `Map<…>` / nested variant fields; `write_csv` wrote Lists unquoted (the row shifted), headers unquoted, and numeric Strings came back as numbers — quoted.
- [x] **Exactness**: Int vector comparisons, `/` (the scalar rule per element), Int matrix `+ - *` and matrix × Int, and `median` of Ints are exact past 2^53.
- [x] **Dates**: `add_months` past year 9999 / with 2^63 months, BigInt day counts wrapping, `format_date` of a Float or past 9999, `parse_date(" 2026-01-01")` — errors of kind `date`.
- [x] **Native**: `bit_test(a, 63)` with a literal index raised in a Direct handler and `to_string(bit_test(a, 62))` did not compile — the literal bail-out removed.
- [x] Smaller: `for x in ()` runs zero times and `for x in 5` is a type error (both ran once); `every 0ms` is a check error; docs: native Int / Int is a Float, masks are 0.0/1.0, Map slot keys are text, both `start` and `init` run, an undefined variable in `try` is catchable, `sum_by` coerces numeric Strings.

### Open
- [ ] Native `/` on two Ints is always a Float (documented, warned) — unchanged.
- [ ] Map slot key types are not enforced (`Map<Int, …>` takes "abc"; `1` and `"1"` are one key) — documented.
- [ ] Misleading messages remain for `rows[k].x = 1` on a missing key and for a think() exceeding max_rounds (points at the tool body).

## Cycle 20 (2026-09-18) — multi-tenant subscription billing (proration, dunning, SSE)

7/10: check, 63 tests and verify --strict (7 temporal properties, 6 invariant proofs)
green; ~15k requests with two kill -9 (one mid renewal tick): reconcile exact
(60,242,522 cents paid = collected), no duplicate invoice, no cross-tenant 2xx. No false proof.

### Fixed
- [x] **Security**: every SSE client received every published stream — `sse("t_ten_2")` got tenant A's invoice events. A client now receives only the streams it named (`sse()` with no name: all).
- [x] Docs: an invariant on a record FIELD or over a structure is runtime-checked (a ⚠ under --strict — keep proven numbers in their own slots); WebSocket clients all receive every event (use per-tenant SSE streams).

### Open
- [ ] One process-wide handler lock caps throughput (~200 req/s here); no secondary indexes (tenant listings scan the slot); no cryptographic random for API keys.

### Cycle 20 — attack (same binary)

No false proof found (termination, intervals, cost composition, temporal properties, rollback all held).
- [x] **DoS**: ~8,300 half-open connections reached the OS thread limit; a tiny_http worker panicked, poisoned its pool, and the process stayed up serving nothing (a supervisor never restarts a live process) — a panic in the HTTP layer now exits with status 70; docs say to cap connections in the proxy.
- [x] `delegate("A", "nope", …)` with literal names passed check — a missing handler of a cell of this program is an error; a cell this program does not define is a warning (libraries composed at run time use it).

### Open
- [ ] Connections are still one thread each (the exit is a clean death, not a limit).

## Cycle 21 (2026-09-18) — customer-support triage agent (tools, approvals, budgets)

7/10: check (cost bound 1900 proven), 48 tests, verify --strict (7 temporal
properties) green; 1,000 tickets with a scripted fake model (malformed JSON,
unknown tools, wrong argument types, SSRF, tool loops, slow replies) and three
kill -9: no double refund, no refund past the thresholds, budgets held.

### Fixed
- [x] **Cost / budget**: a timed-out provider round was retried up to 3 times — one think() could make the provider generate (and bill) 4 × max_tokens past the proven bound while tokens_used() stayed 0, holding the handler lock 4 × timeout. Timeouts are not retried (429/5xx still are).
- [x] **Replay**: nested handler calls were recorded and replayed as well as their caller (`_inner` ran twice: "1 diverged") — only top-level calls are recorded; replay ignored soma.toml [agent] and sent prompts and the key to api.openai.com — it reads [agent] like run/serve.
- [x] `map("tools_allowed", [...])` was read by `soma verify` ("can dispatch [vault_read]") but REFUSED at run time as an unknown option, so a think() had no per-call tool list (a classifier could call `issue_refund`) — enforced at run time (only those tools are offered; another is refused to the model); `requires` accepted. Unknown think() options in a literal map are check errors.
- [x] A tool whose handler transitions to a LITERAL state failed `--strict` ("computed at run time") — it is one of the declared edges; think-isolation holds.
- [x] A tool call from a model to a cell without tools said "exceeded max rounds (1)" — says the cell has no tools.
- [x] Docs: timeouts, `tools_allowed`, think_json's `max_rounds` and no schema option, what record/replay covers.

### Cycle 21 — attack (same binary)

- [x] **`&&` / `||` took any value**: a comparison mask (`xs >= 0` on a list) or the String "false" counted as true, so `invariant m >= 0 && m <= 100` stored `[5000000]` over HTTP, `guard { amount > 0 && amount <= 1000 }` let `{"amount": [5000000]}` through, and `assert scores() >= 0 && true` PASSED with negative scores. Both operands must be Bools (a type error otherwise — an invariant that cannot be evaluated refuses the write); `ensure`, match guards and `forall` properties too.
- [x] **Capability SSRF**: `http://*.x.com/*` matched `http://127.0.0.1/a.x.com/b` (the `*` crossed the host), and `/public/../admin` left a `/public/*` scope — host and path are matched separately; userinfo, fragments, backslashes and `.`/`..` segments (encoded too) are refused.
- [x] **Scope lost across agents**: a scoped tool calling another agent whose own tool is unscoped fetched anything — nested tools answer to every enclosing scope.
- [x] `recall` in a new process (or another serve thread) answered null — the memory table is opened on read.
- [x] **Native**: Bool arithmetic returned a Bool (the interpreter raises); Rust keywords as names, and handlers `f` + `f_fast`, passed check and failed in rustc — check errors.
- [x] Check: a function used as a value (`let f = len`, `map(dbl)`) is an error (`xs |> len` stays a call); `m.set((), v)` wrote the key "null" — refused. Docs: native `/` precisely (an Int result stays Int, `to_string(a / b)` is "-7.0").

### Open
- [ ] Replay starts from empty storage and compares results only: a state divergence shows only in a later result (documented).
- [ ] Native BigInt mixed with Float (`abs(x) + min(x, 2.5)` with x past i64) overflows where the interpreter answers.
- [ ] Int/Float `==` compares through f64 (`2^53 + 1 == 2^53 as Float` is true).
- [ ] `assert_fails … matching ""` matches everything; the max-rounds error points at the tool body.

## Cycle 22 (2026-09-18) — restaurant reservations and table inventory

8/10: check, 64 tests, verify --strict (25 invariant proofs, 7 temporal
properties) green; "no double booking" proven through a per-slot claim
invariant; 3,000 concurrent bookings for one slot gave one winner per table;
two kill -9 under 30k–50k requests: every acknowledged booking present, holds
expired after restart. No false proof.

### Fixed
- [x] A helper of one `cell test` ran for a same-named helper call in another test cell's rules (a second `_setup` silently ran the first) — the running test cell's own helper wins.
- [x] Docs: a Float inside a printed map prints its digits (only to_json uses `1.5e21`); dates are UTC, no time zones or time-of-day builtins — the fixed-offset recipe; `every` runs in any cell.

### Cycle 22 — attack (same binary)

No false proof found.
- [x] **DoS**: `range(-5, 2^63 - 1, 2^63 - 1)` passed the 10^8 cap (2 elements) and then the `i += step` wrapped negative: 9 GB before the kill — the step is checked.
- [x] **Cross-client injection**: a String payload of `publish` / `emit` went into the WebSocket envelope unquoted (`hi","event":"admin",…` rewrote it for every client) and into the SSE `data:` line raw (a newline forged `event:` lines for every subscriber) — both carry the value as one-line JSON.
- [x] `format("%.99999f", x)` panicked past `try` (Rust's precision limit) — at most 1000 decimals, kind `range`.
- [x] `round` / `floor` / `ceil` of 2^63 answered 2^63 − 1 in the interpreter (native was right).

### Open
- [ ] `x % 0` says "modulo by zero" interpreted, "division by zero" native (same kind).

## Cycle 23 (2026-09-18) — batch + streaming e-commerce analytics (pandas port)

7/10: every aggregate matches an independent Python reference exactly (daily
revenue per country, cohorts, refund rates, median, p90/p99 bit-for-bit with
numpy); native path 108× faster (20.5 s → 0.19 s on 201k rows) with identical
results; idempotent directory ingest survives kill -9 mid-file; 8 invariants
proven by induction.

### Fixed
- [x] **Native**: `di = nd` from an i64 local into a BigInt-mode local compiled to `std::mem::swap` of two Rust types (E0308 after a clean check) — an `assign`.
- [x] The "`let` hides the memory slot" warning fired for another cell's slot name — only the walked cell's slots.
- [x] Regexes were compiled on every call (~12 µs; 60% of an interpreted validation) — cached per pattern.
- [x] `read_files(dir, n)` took the first N in file-system order — by file name.
- [x] Docs: persistent write cost (~0.1 ms measured, not 1 ms); read_files has no listing / move / delete.

### Cycle 23 — attack (same binary)

No false proof.
- [x] `zeros(0, 2^62)` / `ones(2^40, 0)` / `reshape` / `mat` with one zero dimension passed the 10^8-cell cap (a product of 0): capacity-overflow panic past `try`, or 18 GB — each dimension is capped; a panic inside any builtin is now a `try`-catchable error of kind `internal` instead of ending `soma run`.
- [x] A `[native]` handler returning `a / b` on one path and a String on another passed check (rustc E0308) — one return type is a check error.
- [x] `format("%d", inf)` printed 9223372036854775807 — kind `range`; a big finite Float prints exactly.
- [x] Docs: group_by / agg keys are the field's text (1 and "1" share a group, missing → "unknown", () → "null").

### Open
- [ ] read_csv drops extra fields silently (no strict mode / line numbers).
- [ ] One unexplained silent exit of a serve process after a tick (not reproduced in 150 iterations).

## Cycle 24 (2026-09-18) — water-treatment safety interlock controller

6.5/10: check, 135 tests, verify --strict (52 temporal properties) green; the
inlet/drain interlock is proven (no state or output word represents both
open); flood, stale sensors and 12 random kill -9 always restarted safe. No
false proof. Cross-device interlocks stay runtime-checked (verification is
per cell).

### Fixed
- [x] `s.LT1 = 5` and `Tank { LT1: 3 }` were parse errors (a capitalised field name is a type token) — PLC tag names work.
- [x] A reactive machine (no terminal state by design) could never pass --strict ("no terminal states" + liveness ⚠) — when every reachable state can return to the initial one, both are ✓ ("reactive machine").
- [x] Docs: [verify] properties hold for the current graph, not for history stored under an older program.

### Cycle 24 — attack (same binary)

The state-machine model checker held everywhere; one hole around it:
- [x] **A handler named like a builtin replaced it program-wide**: an imported `on transition(id, to) { log }` or `on approve(msg) { return true }` turned every interlock and approval into a no-op (SOMA_APPROVE=never ignored) while verify printed "refinement ✓" and VERIFY OK — `transition`, `approve`, `fail`, `get_status`, `has_state`, `valid_transitions`, `think`, `think_json`, `set_budget`, `tokens_used` are reserved handler names (check error). Examples renamed (`approve_doc`, `approve_mail`).
- [x] Two `soma serve` on one .soma_data both ran every `every`/`after` (ticks doubled) — one scheduler per data directory (an exclusive SQLite lock; the second logs that it does not schedule).
- [x] `[verify.after.X]` with empty lists and two `initial:` lines were accepted silently — errors.

### Open
- [ ] `A.w.set(…)` in a test cell passes check and fails with "undefined variable: A" (write `w.set` in tests).
- [ ] A payload-variant target (`transition(id, Failed("x"))`) is counted by verify but always raises at run time.
- [ ] The guard-binding check misses transitions from other cells, emit listeners and interpolations (they fail closed at run time).
- [ ] `eventually = []` / `always = []` at the top level are silently empty.

## Cycle 25 (2026-09-18) — collaborative kanban / issue tracker

8/10: check, 82 tests, verify --strict green (WIP limits, "done only after
review", reactive machine); 300 SSE listeners + 300 movers with no lost or
reordered event, 30 simultaneous moves of one card → exactly one 200, kill -9
lost none of 120 acknowledged versions; real Chrome saw peers' moves in 0.5 s.

### Fixed
- [x] A `property` could not see the rules' `let` fixtures (undefined variable at run time after a clean check) — fixtures are in scope.
- [x] `response(200, "<b>x</b>", "Content-Type", "text/html")` sent a JSON-wrapped body under an HTML type — the header is the content type and a String body goes out as is; `html(status, body, "Set-Cookie", …)` dropped its headers — header pairs are sent.
- [x] No cryptographic primitive (tokens and password hashes could not be secure): `sha256`, `hmac_sha256`, `random_token` (OS CSPRNG), `secure_eq` (constant time).
- [x] Docs: auth recipe (tokens, hashes, cookies, SameSite), repeated form fields, the unauthenticated dashboard.

### Cycle 25 — attack (same binary)

- [x] **Private slots were not private**: any cell (an imported library included) read and rewrote another cell's slot by its bare name (`api_keys.set("admin", …)`), unseen by verify — a check error (test cells excepted); the tradingbot example got accessor handlers.
- [x] **A library replaced builtins program-wide**: `on escape_html(s) { return s }` in an imported file turned escaping off (XSS); `on clamp(x, lo, hi) { return x }` made "writer proven" false — only the CALLING cell's own handler shadows a builtin at run time; a bare call to another cell's builtin-named handler is a check error (`H.rows()`); the prover treats a handler named like clamp/abs/min/max/len as unknown.
- [x] **Path traversal in `soma install`**: a dependency named `../../x` or `/abs` wrote .cell files anywhere — names are validated (add and install).
- [x] Import cycles (`use self`, a ↔ b) overflowed the stack; diamonds defined a cell twice — each file is imported once.
- [x] `soma run app.cell put --fresh` wiped the database — everything after the handler name is an argument.
- [x] `--json` printed nothing on a parse error; `test --json` mixed print() output into stdout — one JSON object; print goes to stderr under --json.

### Open
- [ ] Errors inside an imported file are reported with the importer's file name and lines (parse errors are right).
- [ ] The replay source hash ignores imported files; `soma add` rewrites soma.toml without its comments; a package shadows a same-named local file silently.
- [ ] A machine-less cell calling transition() drives the program's only machine; an import that adds a second machine makes it fail at run time after a clean check.

## Cycle 26 (2026-09-18) — hospital medication administration (eMAR)

7.5/10: check, 117 tests, verify --strict (58 temporal properties, 0 ⚠) green;
"never administered twice", "no controlled dose without a witness", "nothing
after stop" proven; a 3-nurse race gave exactly-once administration; kill -9
lost only in-flight requests and the sha256 audit chain stayed intact across
1,968 entries. The verifier's ✓s held under mutation.

### Fixed
- [x] **False green**: `assert_fails transition(id, "illegal")` in a program with several machines passed on the "which machine?" error (kind type), not on the invalid transition — a test rule calling transition()/get_status() there is a check error; gotcha 12 now writes `matching "invalid_transition"`. delivery/app.cell had exactly this test (and a face `-> String` on handlers returning responses, so every 409 was a 500) — both fixed.
- [x] Docs: gotcha 1 (escaped quotes inside `{…}` work; an unescaped `"` ends the string); test-cell helpers are private to their test cell.

### Cycle 26 — attack (same binary)

- [x] **CSRF**: a GET to a handler calling another cell's handler by bare name, UFCS or pipe answered 200 and wrote (only `Store.bump()` was 405) — any call into another cell's handler counts as a write.
- [x] **Auth bypass through coercion**: `secure_eq("null", ())` was true (a missing token stringified to "null"), so `secure_eq(provided, tokens.get(user))` let an unknown user in with the text "null" — sha256 / hmac_sha256 / secure_eq take Strings only (kind type).
- [x] `random_token` was classed deterministic (replay could not attribute its divergence) — nondeterministic.
- [x] `Store["secret"]` and `"{Store.secret}"` escaped the foreign-slot check — caught.

### Open
- [ ] Verify prints each property per machine (vacuous lines for machines without the state).

## Cycle 27 (2026-09-18) — multiplayer card-game backend with an economy

7/10: check, 76 tests, verify --strict (12 temporal properties, 0 ⚠) green;
400 concurrent players trading and playing with two kill -9: currency
conserved (175,000 minted = balances + escrow), 700/700 items unique, 695
out-of-hand plays refused. No false proof.

### Fixed
- [x] **Data leak**: every `emit` (cell-to-cell) was pushed to every WebSocket client — an internal `emit secret_hand(…)` reached an unauthenticated client verbatim. An emit now reaches only SSE clients that name it; WebSocket clients and `sse()` without names get `publish` streams only.
- [x] `Store.config.get(…)` in a test rule passed check and raised "undefined variable: Store" — a check error pointing at the bare `config`.

### Cycle 27 — attack (same binary)

- [x] **One WebSocket message or tick killed the process**: recursion ~250 deep in `on ws`, an `every`/`after` block or a bus listener overflowed the default thread stack (HTTP threads had 64 MB) — every thread that runs handlers gets the 64 MB stack; the 512-frame guard answers `stack_overflow`.
- [x] **One slow WebSocket client starved all others**: the broadcast blocked behind it, then healthy clients silently lost events (1052/3000) — each client has its own bounded queue and writer; a client whose queue fills is dropped and its socket closed (3000/3000 delivered to the healthy client).
- [x] `try { … }?` returned the error map as a 200 value, committing earlier writes — it re-raises the error (same kind).
- [x] A guarded arm (`Sq(s) if s > 5.0`) counted as covering its variant — it does not (check error for the missing plain arm).
- [x] `"{slot}"` passed check and raised "undefined variable" — interpolation reads slots like code; `"{a b}"` silently dropped `b` (and never checked it) — one expression per `{…}` (check and run).
- [x] `mod(7.5, 2)` / `idiv` / `floor_div` / `div_round` truncated Floats — Int arguments only (kind type); `sum_by` / `avg_by` skipped non-numbers silently — kind type; `distinct(["1", 1])` dropped the Int — keyed by kind and value.
- [x] `on ws` in a cell that does not own `request` (never runs) or typed other than String — check errors.

### Open
- [ ] A string literal on the line after `require … else Tag` becomes the require's detail (the parser does not see newlines).
- [ ] `sort_by` with NaN keys does not sort; Int/Float `==` goes through f64.

## Cycle 28 (2026-09-18) — warehouse management across two processes

7/10: check, 83 + 16 tests, verify --strict green on both processes; 2,124
scans through an idempotent bus outbox with kill -9 of each process: nothing
lost, ledger audit exact (received − adjusted − counted − shipped = on hand).
No proven property failed.

### Fixed
- [x] **Silent wrong value**: `let s = if … { 1 } else { if … { 2 } else { 3 } }` was `()` (a nested if STATEMENT as the block's last line) — it is the block's value.
- [x] The composition lint demanded compensation for a callee that RAISES (the whole handler rolls back) and failed --strict on correct code — it now flags only callees that catch their own failed transition and return normally.
- [x] `soma check` warned "emit … goes nowhere" for every cross-process emit — silent when soma.toml lists `[peers]`.
- [x] Docs: start() runs before the peer links; bus events wait for the handler lock; two-way peers reconnect.

### Cycle 28 — attack (same binary)

- [x] **False cost proofs**: a think() in a `while` CONDITION was costed once ("proven 10", spent 50) — counted per iteration + 1, and an unbounded while spending in its condition is advisory; `max_rounds` given twice was costed by its first value and run with its last — the last.
- [x] **Auth bypass**: a handler `request` reaches through UFCS (`k.wipe()`), interpolation (`"{wipe(k)}"`) or `delegate("Api", "wipe", k)` was still a direct endpoint — route ownership is computed on the desugared program.
- [x] **Record forgery through storage**: a String in a `Map<String, Any>` / `List<Any>` slot came back parsed as JSON (`{"_type": "Admin"}` became a record, is_a true) — Any and String slots give back the String.
- [x] Transitions from a cell without a machine escaped refinement, think-isolation and the literal-target check — with one machine they are modelled (an undeclared target is ✗); with none or several they are a check error.
- [x] A literal sub-pattern (`Charged { tx: "t1" }`) counted as covering the variant — it does not.
- [x] BigInt arguments read as 0: `clamp(2^70, 10, 20)` was 10 (and "proven"), `pow_mod` with big operands 0/1 — exact; counts, indexes, widths and code points past 64 bits raise kind `range`.
- [x] Native `len(s)` counted bytes (interpreted: characters) — characters; `str_at` errors say the index as written, kind `index`, in both.
- [x] An invariant of one cell applied to another cell's slot of the same name — scoped to its cell.
- [x] `B.bh()` with the wrong argument count passed check — an error.
- [x] verify printed "✓ refinement: h ⟶ {nowhere}" next to the ✗ for that undeclared target — the ✓ is gone.

### Open
- [ ] Several check gaps from the attack remain (variant constructor field types, literal wrong-type slot writes, `m.push` on a Map slot, `map("a")`, `think()` with no argument) — all raise at run time.
- [ ] `to_string(1.5e300)` prints every binary digit; `-0.0` round-trips as `0.0` in a Float slot.
- [ ] A string on the line after `require … else Tag` becomes its detail.

## Cycle 29 (2026-09-18) — insurance policy administration and claims

7.5/10: check, 145 tests (4 exhaustive forall properties), verify --strict
(45 temporal properties, 10 money writers proven) green; 50k-policy nightly
rating byte-identical to a Python Decimal reference (native 7 ms vs 336 ms
interpreted); kill -9 twice under 25k requests: nothing lost, audit exact.

### Fixed
- [x] **Quadratic writes**: every write to a slot with an invariant counted the whole slot (1,000 writes on 20k entries: 444 ms; a 50k load took 417 s) — the COUNT only when an invariant reads `size` (4 ms); the start-up audit counted the slot once per key (4.3 s → 0.5 s).
- [x] `mock think` replies were counted past max_tokens in tests (41 tokens against a proven 10) — the reply part is capped like a provider's.
- [x] A [native] call into another cell's [native] handler said "non-native function" — says the callee must be in the same cell.

### Cycle 29 — attack (construct × analysis matrix, same binary)

65 constructs × 8 analyses: invariant prover, think-isolation, refinement and GET-405 held everywhere; the holes:
- [x] **One GET killed the server**: recursion through lambdas only (`let f = g => g(g)  f(f)`) bypassed the recursion guard (OS stack overflow, exit 134) and verify said "structurally terminates" — lambda calls count toward the guard (`stack_overflow`), and calling a function value is a termination ⚠.
- [x] **False cost proof**: think() in any lvalue assignment (`answers[k] = think(…)`, `g.f += …`) was invisible ("peak 0", spent 3000) — costed.
- [x] **Private slots bypassed**: a bare foreign slot inside `"{…}"` (`"{bal.set(id, -3)}"`) wrote another cell's storage past its invariant, and reads leaked its secrets — the unqualified storage fallback is for test rules only; check refuses it in segments too. (quant's template read another cell's slot this way — fixed.)
- [x] The guard-binding rule missed transitions in interpolation, UFCS and require — run on the desugared program.
- [x] Route ownership did not follow `request → emit → listener → handler` (POST /wipe open without auth) — it follows emits.

### Open
- [ ] `try { … }?` of an always-rejected write is described as "the handler catches it"; `"a" |> counts.set(99)` and a self-referencing lambda pass check.
- [ ] Warnings from imported files still carry the importer's file and line.

## Cycle 30 (2026-09-18) — deployment orchestrator (canary, approvals, freezes)

8/10: check, 85 tests, verify --strict (21 temporal properties, capacity bound
proven) green; 8 random kill -9 plus a webhook outage: 0 stuck, 0 double
promotions, webhooks 331/331 exactly once, max 3 concurrent prod deploys.
No bug and no false proof.

### Fixed
- [x] verify names each machine `Cell.machine` when several cells have one (two `s` blocks were indistinguishable).
- [x] Docs: "vacuously true" is for a missing TARGET state (a `requires` naming a missing state fails); keywords and reserved handler names; tool capabilities bind model calls only; tool calls are tested offline with a fake OpenAI-compatible server.

### Cycle 30 — attack (regressions and storage, same binary)

- [x] **Forged variant through storage**: the storage encodes variants / BigInts / special floats with in-band `__variant__` / `__bigint__` / `__float__` keys, and a client JSON body stored in an Any slot came back as `Admin {who: "mallory"}` — map keys starting with `__` are refused in slot values and `remember`.
- [x] **Proven size bound broken**: slot keys starting with `__` were hidden from len / keys / the size invariant (6 rows under `size <= 3`) — refused.
- [x] **False termination**: lambda self-application through an alias (`let k = [g][0]  k(k)`) — the rule now flags any lambda whose body calls a function value, or a handler calling its function parameter; the documented `let f = x => x + 1  f(2)` is no longer a ⚠ under --strict.
- [x] **Dropped statements**: the last statement of a block lambda was discarded when it was not an expression — `x => { emit credit(x) }` never emitted (and its target stayed a forgeable endpoint), a final `set` / `transition` never ran.
- [x] Route ownership stopped at other cells (`request → Domain.run → Api.wipe` left POST /wipe open) — it follows calls through every cell.
- [x] `if` / `while` / filter / find / any / all / count took truthiness (`if body.admin` was true for the String "false") — a Bool, `()` being false.
- [x] `soma run --fresh` deleted the database under a running `soma serve` — refused while a serve holds the directory.
- [x] A slot name two cells declare, read in a test rule, silently read one of them — check error; `"{secret}"` of another cell's slot is refused by check.

### Open
- [ ] Writing a List slot by index or deleting from it is O(n) per write (quadratic loops).
- [ ] `delegate("Api", h, …)` with a computed handler name inside `request` does not own `h`.
- [ ] `rows.delete(5)` out of range is a silent no-op; a Map slot re-declared List loses its old entries on the first push.

## Cycle 31 (2026-09-18) — lab experiment pipeline + numerical toolkit

8/10: check, 60 tests (8 forall properties), verify --strict green; native
Monte Carlo / Simpson / Gaussian elimination bit-identical to the interpreter
and to NumPy (≤ 4e-16 relative), 3–6× faster than NumPy; kill -9 during an
analysis never exposed a half-analysed result.

### Fixed
- [x] Large Floats printed their exact binary expansion (6.02214076e23 → 602214075999999987023872.0) — shortest round-trip digits (interpreter, native, CSV).
- [x] `NaN` / `inf` written by write_csv came back as Strings — read back as Floats.
- [x] A handler calling an imported handler that may not terminate was "✓ structurally terminates" — non-termination propagates through calls.
- [x] `format("%.3e", x)` — C-style scientific notation; docs: random() has no seed, sum's order, matrix × vector.

### Cycle 31 — attack (regressions + the docs run as a test suite)

- [x] **One request wedged serve**: self-application passed to a higher-order builtin (`g => try { [g, g] |> map(g) }`) passed verify --strict and, with `try` catching each stack overflow, ran at 100% CPU / 940 MB holding the lock — a stack overflow is not caught by `try` (it fails the handler), and a lambda handing a function value to map / filter / … is a termination ⚠.
- [x] **Auth bypass**: `delegate("Api", op, id)` with a computed handler inside `request` — every public handler of that cell is request's.
- [x] Regressions of cycle 30: refusing `__` map keys broke `word_count("__init__")` and `to_sampled` handles — such maps are escaped by the storage encoding instead (still never a forged variant); the function-value ⚠ fired on `let dbl = x => inc(x)` — only function values that could be the lambda itself count; ⚠ lines name the handler.
- [x] `()` is false everywhere a condition is read (`!()`, `&&`, require, guards raised); invariants must be Bool conditions (`invariant vals` was truthiness, and verify suggested `require v - 3`).
- [x] A face `tool` was a public HTTP endpoint around request's auth — not exposed.
- [x] `soma run app.cell nosuch` ran the first handler with "nosuch" — an error when several handlers exist.
- [x] `return f(a)(b)` returned the lambda silently — refused; test-cell helpers read slots; undefined functions in test rules are check errors; native conditions are Bools; 5 redirect hops are followed (the 5th failed).
- [x] Docs examples fixed: the agent example (missing tool handler, hard-coded instance, cost claim) now passes verify --strict; the cell-anatomy example checks; gotcha 2's `*` wildcard is `_`; predicate and recall docs.

### Open
- [ ] car-rental/lib/tests.cell calls `seed()` from the app that imports it (it can never run on its own) — now a check error.
- [ ] Writing a List slot by index is O(n); `soma run` picks one of two same-named handlers silently.

### Cycle 32 — realistic port (shop) + attack

Shop port 7.5/10. Attack (prover, termination, cost, GET→405, route
ownership, static traversal, WS origin, 300 concurrent payments, 200k-deep
JSON): no HIGH — every guarantee attacked held.

### Fixed
- [x] `"{if c { 1 } else { 2 }}"` printed garbage with a clean check — an interpolation segment holding a block ends at its matching brace (runtime and checker agree).
- [x] `r.size` on a record without that field gave the entry count — `()`; a plain map keeps `.len` / `.size` / `.keys` when it has no such key (`m["size"]` always reads the key), documented.
- [x] `from_json(from_json(body))` built a `Charged` without its `tx` — a match check proved exhaustive raised "non-exhaustive match" at run time; from_json now checks a declared variant's shape (kind `type`).
- [x] Docs said `body: String` + from_json was the way to accept a client-named variant, but the reserved-key 400 applies to every body — documented as such.

### Open
- [ ] Runtime errors inside an imported file report the importer's path.

### Cycle 33 — realistic port (ticketing) + attack

Ticketing port 8/10: holds expiring via `every`, waitlist promotion, 50
parallel requests for the last seat → exactly one 201, verify --strict green.

### Fixed
- [x] **Every analysis skipped `"{ { expr } }"`**: an unparseable segment is literal text and the runtime rescans from the next byte, running the inner `{ expr }` — the analyses skipped the whole segment. Auth bypass through request (`"{ { wipe() } }"`), false termination and cost proofs, a GET that wrote, a guard that wrote, a false invariant ✓. Segments are now found exactly as the runtime finds them; guards are walked through their strings.
- [x] A returned String starting with `{`/`[` was sent raw as JSON even when it was not JSON (`[x`, `{<script>`) — only valid JSON text is sent as is; JSON answers carry `X-Content-Type-Options: nosniff`.
- [x] `[native]` kept `{{` / `}}` literally (`a{{b}}c`, str_len 6 vs 4).
- [x] `-> List<Int>` returning `["a"]` passed — face return types are checked element-wise.
- [x] verify printed "only deletes from 'm'" for a handler that also sets it; suggested a `require map(…).taken <= …` that cannot prove anything; type errors said "Null" (`type_of(())` is "Unit").
- [x] Docs: secrets come from a file read in `on start()`, relative paths resolve against the start directory, `substring(s, i, i + 1)` vs `str_at` (a byte).

### Open
- [ ] Errors inside an imported file are reported at the importer's path.
- [ ] Invariants cannot relate two slots; no CSV-to-String builtin.

### Cycle 34 — realistic port (double-entry ledger, two files) + attack

Ledger port 7.5/10: 200 concurrent postings balanced, 300 posts racing a
period close → exactly the accepted ones exist, verify --strict green.

### Fixed
- [x] **A capability-scoped tool read any file and exfiltrated emits**: `load` / `include` / `load_template` / `par_read_files` / `read_stdin` were not denied (the model read `../secrets/admin_token`), nor `link()` / `ws_send` (every later emit went to the attacker's peer).
- [x] **Invariants ran arbitrary code**: a handler call hidden in interpolation / a lambda / UFCS, or an effect builtin (`think`, `transition`, `approve`, `link`, …) passed check — false cost, termination and refinement proofs. Invariants are pure conditions now (a deep walk, one shared list of effect builtins also used by guards and GET→405).
- [x] **Termination claimed for recursion the model drives**: a tool calling back the handler whose think() dispatches it (5 250 LLM calls from one run) — think()→tool edges are in the call graph.
- [x] `link()` was not a state change (GET 200, any web page could re-route emits) nor refused in a guard.
- [x] `load(t, "name", n, "token", secret)` substituted sequentially — `name="{token}"` printed the secret; one pass, as render().
- [x] A provider ignoring `max_tokens` broke a "proven" cost bound — a reply longer than its cap raises kind `llm`.
- [x] **Errors inside a `use`d file reported the importer's path and a line past its end** — imported spans carry their file (runtime, check, JSON).
- [x] Port: `??` evaluated its right side always (`get(k) ?? fail(…)` failed on a present key, `?? next_id()` burned ids); `with()` refused a BigInt value; a Float-slot invariant could never pass --strict (strict bounds, and a value that passed a require comparison is not NaN — an early-exit's negation still is); `tools_allowed` naming a non-tool is a check error.

### Open
- [ ] `soma test` "raised at line N" and the max-rounds error point at the wrong line; serve's `endpoints:` line lists request-owned handlers; GET /<tool> answers 405 (POST 404).
- [ ] A constant `idiv(i64::MIN, -1)` in a [native] handler fails the rustc build after a clean check.

### Cycle 35 — realistic port (multi-tenant job queue) + attack

Job-queue port 8.5/10: 20 workers claiming 15 jobs → each leased once in
priority order, idempotency and rate limit exact under load, lease expiry →
retry → dead-letter live, verify --strict green with 4 temporal properties.

### Fixed
- [x] **`x |> h` (a bare handler name) was invisible to every analysis** — the runtime calls h(x): `n |> up` recursed with "✓ terminates", `7 |> wipe` wrote on GET, `1 |> wipe` inside request left POST /wipe open around its auth, `q |> think` escaped the cost bound, a tool `q |> ask` looped the model 512 times. The parser makes it `x |> h()`.
- [x] `subscribe()` ran any public handler a remote stream named (`on ws`, a `wipe`) — the bus's policy: only events the program emits or `[bus] accept` lists.
- [x] File reads, `print`, `read_stdin`, `recall` were accepted in invariants and guards — check errors (`random` / `now` stay: no effect).
- [x] `[native]` `(0 - 9223372036854775807 - 1) * -1` panicked (two small operands, an overflowing product) — the whole operation must fit i64 to stay i64; `to_string(i64::MAX + 1)` natively failed the rustc build (an untyped panic block).
- [x] Port: `.field` on a String / List read `()` (a JSON body `"str"` passed with every `??` default) — kind `type`; serve's `endpoints:` line and 404 listing named request-owned handlers, and GET on them answered 405 (confirming they exist); the size-invariant hint names the `require slot.get(k) != ()` route.

### Open
- [ ] `"k" |> c.set(99)` passes check, fails at run time; `--fresh` beside a running `soma run` deletes its committed data; no scheduler failover after the tick owner dies; replay re-runs http_post / write_file; a renamed machine is only warned about under serve; a cross-process emit ping-pong livelocks silently.

### Cycle 36 — realistic port (restaurant bookings + kitchen, 4 files) + attack

Restaurant port 8/10: 20 orders racing for the last beef → one 201, stock 0;
10 bookings for one slot → 3 table combinations, no double occupancy.

### Fixed
- [x] **Statements inside expression blocks were invisible** — block lambdas, `try { }`, if-expression branches, match-arm bodies: `slot[k] = v` / `slot.k = v` / `emit` there let a GET write (200), left a listener a public forgeable endpoint (`try { emit grant(..) }` → POST /grant gave eve 1 000 000 credits), verified ping/pong emit recursion as terminating, let guards and invariants write and emit, and "proved" a size bound an emitted listener broke. One deep statement walker (`for_each_stmt_deep`) now feeds GET→405, the listener set, the termination call graph, the size prover's callees and guard / invariant purity.
- [x] `return` inside a block lambda passed check and always raised — a check error; a bare `return` is `return ()`.
- [x] Port: `soma run app.cell request GET "/a?x=1" ""` passed the query inside the path — split as serve does; a test error after a call returned pointed at the callee's line; `every 90m` said "invalid number" — names the units; a loop-bound value was reported as "`e` = ()"; intervals printed `-0`.

### Open
- [ ] Termination false positives: `if !(n > 0) { return 0 }` as a base case, a base case inside `try`; `m.a.b = 1` on a local map fails at run time with a `with` message; `1..10` in a match pattern is inclusive while `in 0..100` in a property is not; a face `-> Int` returning a computed String passes check.

### Cycle 37 — realistic port (support tickets + LLM triage agent) + attack on the provers

Support port 8/10: think_json triage with a scoped http tool against a
fake orders API and a fake OpenAI server making real tool calls, a proven
1200-token bound, 8-state machine with 7 temporal properties, verify --strict
green. The attack ran 600 random interval expressions with no false ✓.

### Fixed
- [x] **Int bounds above 2^53 were "proven" through f64 rounding** (`require v <= 2^53` then `v + 1` "✓ <= 2^53"; `9007199254740993 - 9007199254740992` folded to 0) — an interval end past 2^53 is widened outward (a lower bound stays ≥ 2^53 − 1), Int literals and constant folds go through the same guard.
- [x] **The size proof ignored think() tools**: a tool pushing to the slot broke a "proven" `rows.size <= 3` — a handler that calls think() reaches its cell's tools; a computed `delegate(…, op, …)` reaches every handler of the cell.
- [x] A bare `c.get(k)` / `c[k]` written as is on an untyped Map was "proven" but may be () — runtime-checked, with the `?? default` fix named.
- [x] `[native]` `i64::MIN / -1` returned a rounded Float — the too-large-quotient error; `idiv(MIN, -1)` as literals failed the rustc build (unconditional_panic allowed; BigInt fallback at run time); a cost peak saturating i64 "proved" `tokens: i64::MAX` — advisory.
- [x] Port: `http_get` in a lambda / unbounded loop / tool made the TOKEN bound advisory — I/O only makes a `latency` bound advisory (examples/atlas dropped a latency axis whose proof ignored its I/O tools); a guarded transition() inside a lambda lost the handler locals the guard reads; refinement paths dropped parentheses; `[verify] cells` naming no cell of the file says NONE applies; docs: approve() web flow, budget per provider round, the OpenAI-compatible wire format.

### Open
- [ ] `A.rows.push(x)` inside A and `let g = rows.push` pass check, fail at run time; `rows[0][0] = 5` reported "may grow"; `x > 10 == true` does not parse; a computed-delegate recursion repeats "delegate error: " thousands of times.

### Cycle 38 — realistic port (quant portfolio tracker, two peered instances) + runtime attack

Portfolio port 7.5/10: FIFO lots and P&L in exact money, covariance and
min-variance weights identical to numpy to the last printed digit, a native
kernel matching the interpreter, two instances exchanging prices over the
bus. The runtime attack (HTTP fuzzing, slowloris, smuggling, static
traversal and symlinks, kill -9, concurrent processes) found no crash.

### Fixed
- [x] **A database damaged mid-file was served as truth** (rows silently missing, a cross-slot invariant broken, no warning) — serve runs `PRAGMA quick_check` at start-up and refuses to answer from a damaged file; a file that is not a database was a Rust panic naming storage.rs — a clean error with the fix, exit 1.
- [x] `range(0, n)` from a client Int took the server from 126 MB to 2.3 GB — a materialized List is capped at 10M elements (kind range; `for i in range(a, b)` is lazy and not capped).
- [x] Dotfiles under static/ were served (`.env`, `.git`); a request with both Content-Length and Transfer-Encoding was accepted (smuggling behind a proxy) — 404 / 400.
- [x] Port: quant builtins ignored their declared bounds (`max_obs`, `max_assets`) and accepted `alpha` 1.5 — kind range; `soma check` warned that an emit listener reached through a helper "shares the path" of a route (it never did); `x => require …` said "reserved word … rename it" — a statement where a value is expected; clean_covariance / VaR / ES conventions documented.

### Open
- [ ] One handler looping over a client Int holds the process lock for minutes (serialized handlers, no per-request time limit — documented); no linear solve / inverse / eigenvectors builtin; property tests take one Int only; `read_csv` types `1E3` as a Float.

### Cycle 39 — realistic port (collaborative wiki) + attack on the test runner and tooling

Wiki port 7.5/10: 20 concurrent PUTs on one version → one 200, nineteen
409; XSS-safe rendering; WebSocket origin checks held.

### Fixed
- [x] **`mock Cell.handler` ignored the cell** (`mock Email.send` answered `Sms.send` — the test passed on wrong code; `mock Nope.charge` stubbed `Pay.charge`) — qualified mocks stub that cell's handler only; a mock naming no handler / cell is a check error.
- [x] **A test cell's own `on total(…)` silently replaced `Cart.total`** in its assertions (and `on len` the builtin) — a check error.
- [x] Mocks did not reach `[native]` calls to a mocked sibling (tests passed interpreted, failed native) — a native handler is interpreted while a mock is pending.
- [x] `soma describe` JSON and the dashboard dropped `except` from `*` edges (drew done → failed); `soma run --record` skipped calls that raised (replay compares their kind now); `soma fix` exited 0 with errors left; test cells asserting nothing passed; `soma docs … | head` panicked.
- [x] Port: a memory slot inside a nested lambda was "a function value" (false termination ⚠); `value` / `key` / `size` passed check in handlers and raised at run time, and verify suggested exactly that (`require value >= …`) — hints substitute the written value and key; a `require` that IS the invariant clause over the written value now proves it (one write to the slot, outside loops, no callee that could write it, no name rebound or partly written in between — `a.x = 500` after `require a.x …` is not proven).

### Open
- [ ] `soma install` never checks soma.lock hashes nor refreshes path dependencies; `use x` silently prefers an installed package over a local x.cell; replay re-runs real side effects; `fix --native-idiv` can turn a Float `/` into idiv; WebSocket connections have no id / disconnect hook.

### Cycle 40 — realistic port (IoT telemetry) + attack on last cycle's equal-clause rule

Telemetry port 8.5/10: HMAC-signed ingest with nonce replay protection,
~28 000 readings/s over 16 threads, 1.12M readings with none lost or
double-counted, 4 retention size invariants proven, SSE atomic with rollback.

### Fixed
- [x] **The cycle-39 equal-clause rule proved writes the runtime refused**: a `.set` / `.push` on another slot between the require and the write (`c.get("x")` changed), nondeterministic builtins read twice (`next_id`, `random`, `now_ms`, `think`), external state (`read_file` / `recall` / `get_status`), a computed key (`to_string(next_id())`). The rule now applies only when the clause and the written value are pure — literals, locals, arithmetic, comparisons, `??` and `.get` of the WRITTEN slot — and the key is a plain name or literal.
- [x] A second slot named inside an interpolation, a lambda or a match arm escaped the "one invariant, one slot" check (and a lambda hiding the slot made the invariant guard every slot at run time) — slots are counted deep, by the checker and the runtime alike.
- [x] Verify hints suggested `require v + size <= 3` / `require key != 0 …` (check errors) — no hint names size / key / value.
- [x] Port: an undefined function inside a `property … ensures` passed check.

### Open
- [ ] `let t = "alarm"  transition(id, t)` is a dynamic target; a lambda parameter read by a transition guard is refused; a literal List written into `Map<String, List<Int>>` with the wrong element type passes check; `[native]` cannot take a List or parse text; List-slot keys are Strings in invariants (`key != 0` cannot evaluate).

### Cycle 41 — realistic port (e-learning, 6 files) + differential attack

E-learning port 8.5/10: 50 parallel certificate requests → exactly one
certificate; 20k-user leaderboard top-5 in 0.06 s. The differential attack
ran ~27k interpreter-vs-native cases plus run / serve / test and
verify-vs-runtime comparisons.

### Fixed
- [x] **Native `bnot` returned -1 for every Int past 2^63** (converted to i64, defaulting to 0) — the unbounded `-a - 1`, as interpreted.
- [x] A native `loop_bound` overrun had kind `type` (interpreted: `loop_bound`); a native exact quotient of exactly 2^53 came back a Float — kinds and the Int are kept.
- [x] `soma run … request GET /w/%C3%A9` passed the raw path — decoded as serve does.
- [x] Port: a guarded transition taken from a machine-less cell (`N.force`) passed check and raised undefined_variable in the guard — the guard-binding rule covers those callers; `[verify] cells` naming no cell of the file passed --strict with every property unchecked — a failure under --strict; `for x in m["items"]` was an "unbounded iterator" (any finite value is); an `emit` only in an imported file left the bus closed ("no emit").

### Open
- [ ] A native `/` whose exact quotient is past 2^53 raises (documented; use idiv); face `-> List<String>` returning `[1, 2]` passes check; a raising property does not print its counter-example `n`.

### Cycle 42 — realistic port (hotel channel manager) + attack on state machines

Hotel port 8/10: 3 channels racing for the last room → one booking; 3
`soma run` + 3 HTTP clients on one database → one winner; webhook retries
through a transactional outbox. The state-machine attack found no HIGH: no
undeclared edge, no skipped guard, every property matched hand analysis.

### Fixed
- [x] A handler parameter named like a slot (`on add(k, m: Map)` with slot `m`): `require len(m) < 2` read the parameter while `m.set` wrote the slot — a false size proof; a shadowing List parameter dropped `rows.push` — a check error.
- [x] Face parameter TYPES were not compared with the handler's (`signal setup(capacity: Int, name: String)` over `on setup(name, capacity)`) — a check error (names stay documentation).
- [x] every / after ticks of a machine-less cell were invisible to verify (an undeclared target raised every tick) and to the guard-binding rule.
- [x] `soma run app.cell Arch` (typo of `arch`) ran another handler by arity and committed — any token that is not plainly data names a handler when there are several.
- [x] Renaming a machine silently reset every instance under `soma run` (terminal orders fresh again) — warned as under serve.
- [x] Instance ids were stringified: `transition((), …)` moved one shared instance "null" — an id is a String or an Int.
- [x] `{"result": NaN}` / a lambda were sent as invalid JSON — encoded.

### Open
- [ ] `except [done, nosuch]` passes check; get_status in a machine-less cell with two machines passes check; a dashboard with no verify results; ~2 KB held per transition until the handler ends (1M transitions → GBs); `memory: "30MB"` silently ignored.

### Cycle 43 — realistic port (feature flags + experiments) + attack on the type system

Feature-flag port 8/10: typed flag variants, sha256 buckets, a typed staged
rollout with 3 temporal properties, 20 concurrent optimistic edits → one 200,
nineteen 409; 15 adversarial cost programs all refused.

### Fixed
- [x] **Parameter types were checked one level deep** — `xs: List<Map<String, Int>>` took `[{"a": "x"}]` from HTTP, the bus, an LLM tool call and in-language calls — checked all the way down, as slots are (native parameters too).
- [x] **`1e20` saturated to i64::MAX in an Int parameter** over HTTP and the CLI (and was stored) — only exactly-representable Floats convert.
- [x] **`[native]` handlers skipped the face return type** (`-> Int` returned 3.5) — one check for both backends; mocks are held to it too.
- [x] `from_json` with an undeclared `_type` built a variant a `match` took for the real type — refused.
- [x] Misspelled builtin types (`Integer`, `Strng`) read as Any; builtins silently ignored extra arguments (`max(1, 2, 3)` = 2) — check errors; `()` nested where Int / Float / String / Bool is declared was accepted; `write_csv` dropped a row holding an empty String.
- [x] Port: a pure helper call before the write defeated the equal-clause proof (a callee blocks it only when it can write the slot, transitively); the hint repeated a require already present — it now says what blocks the proof; `_coalesce()` leaked into messages; `variants = 2` did not parse after `let variants = 1`; a computed max_tokens said "no max_tokens".

### Open
- [ ] Map key types are text (`Map<Int, …>` keys read back as Strings); Int → Float is not coerced inside nested values; think_json can return a Map with `_type` (is_a true); tool schemas advertise variant parameters as strings.

### Cycle 44 — realistic port (warehouse management) + second pass on type enforcement

WMS port 8.5/10: 50 concurrent allocations against 310 units → exactly 31
winners, stock exact; 10 temporal properties proven. The attack agent's
report was largely written against an older binary (most of its "HIGH"s
reproduce with 2.5.1, not with the frozen ux44 build — re-checked one by
one); what it found that was real is fixed below.

### Fixed
- [x] `emit ev(1)` to a listener taking two parameters passed check (raised at run time and rolled the emitter back) — an arity check error; `Json`-style type names map to `Any` in the unknown-type hint.
- [x] Port: a computed Float `2.0` entered an Int parameter (a literal, a List<Int> element and an Int slot refused it) — refused; `write_csv` of a row whose cells are all `()` wrote a blank line read_csv skips (the row was lost) — `""`; a duplicate CSV header silently dropped a column — kind `csv`; "reassigned 2 times" counted the let.

### Open
- [ ] Int → Float is not coerced inside nested values; huge Floats print all their digits (no exponent); `()` enters a `List` / `Map` parameter (documented optional containers); `write_csv` does not guard against formula injection; a per-key capacity read from another slot cannot be proven.

### Cycle 45 — realistic port (clinic appointments + prescriptions) + open-ended claims audit

Clinic port 8/10: 30 parallel bookings of the last slot (and 5 `soma run`
+ 5 curls across processes) → one winner each; hash-chained audit survives
GDPR erasure; every authorization bypass tried failed. The claims audit
found no HIGH: the error-kind → status table, GET→405, static files, the
start-up audit, init templates and offline docs all matched.

### Fixed
- [x] **Request smuggling with duplicate / list Content-Length** (`Content-Length: 0` then `<n>` ran the body as a second request) — refused 400 in the accept loop, and the connection is poisoned so the pipelined bytes never run (a check in the handler thread raced them).
- [x] A 50 000-deep chain of unary operators passed check and aborted `soma serve` with a native stack overflow — unary operators count toward the 400-level depth.
- [x] **Log injection**: client text in an error detail (`%0A`, `%00`) forged request-log lines — control characters are escaped in every request log line.
- [x] A List slot's invariant `key` was "" (push) or the text "0" (index write), so `entries.get(key) == ()` guarded nothing — the Int index.
- [x] The cycle-43 arity check refused the documented `html(200, page, "Set-Cookie", …)` (its registry signature lacked the header pairs); `soma run … ix 1.0` still converted to Int (HTTP refused) — both consistent now.
- [x] Port: a guard variable bound in a loop body before transition() was "bound only inside a branch"; docs: the time-zone recipe used now() (seconds) as milliseconds; property iterations share slots.

### Open
- [ ] Two Set-Cookie pairs in response() send only one; the dashboard shows no verify verdicts; `soma describe` has no routes and marks request-owned handlers public; verify is superlinear past ~1000 chained handlers; a write-once proof depends on the written value's bounds.

### Cycle 46 — realistic port (ride-hailing dispatch) + attack on the realtime surface

Dispatch port 7/10: two riders racing for one driver → one match; 20
riders vs 6 drivers → one trip per driver; the 30 s accept timeout
re-matched live. The realtime attack found the WebSocket frame parser
strict and RFC-correct, SSE streams isolated, bus authorization and
invariants enforced on peer events, and slow readers never stalling the
handler lock.

### Fixed
- [x] **The per-peer bus send queue was unbounded**: a peer that stopped reading (or a socket that never read) grew the emitter from 233 MB to 2.25 GB — bounded queues; a peer whose queue is full is disconnected, as a WebSocket client is.
- [x] The WebSocket per-client queue capped EVENTS (1024) but not bytes (5 idle clients × 4 MB publishes → 1.2 GB) — 64 MB per client too.
- [x] The WebSocket Origin check read `http://localhost:1@evil.com` as localhost (userinfo) and accepted control characters — the authority is parsed; userinfo / control characters refused.
- [x] Port: a string split by an unescaped `"` as the LAST statement of a loop body passed check (a loop body has no value) — flagged, with the quote hint also after an assignment; `invariant n == 0 || n == 5` was not proven for a literal write — disjunctions over the written value are proven (a side reading `key` can raise, so it stays runtime-checked).

### Open
- [ ] `s == "" || s == "x"` for String literal writes is not proven; native vocabulary lacks atan / atan2 / asin / acos; WS replies and publish pushes use different JSON spacing; `verify --strict` repeats ⚠ lines in its summary.

### Cycle 47 — realistic port (crowdfunding, 7 files) + attack on the newest rules

Crowdfunding port 8/10: 20 concurrent pledges for the last reward → one
201; 30 on a 3-unit reward → exactly 3; the per-backer daily limit held
under concurrency; 9 temporal properties proven. The attack on the newest
rules (equal-clause proofs, disjunctions, size bounds, List keys, deep
types, arity checks, smuggling defenses) found them sound.

### Fixed
- [x] **One 15-byte request killed `soma serve`**: a declared `Content-Length: 1000000000000000` with a 3-byte body aborted the process ("memory allocation … failed") — tiny_http's `EqualReader::drop` drained the unread body with ONE allocation of the declared length. tiny_http is vendored (compiler/vendor/tiny_http, MIT/Apache) with a chunked drain, and a declared body past 256 MB is refused 413 before any read.
- [x] Port: a `mock think` reply longer than its max_tokens was returned whole (and under-counted) — it raises kind `llm`, as a provider's over-cap reply does.

### Open
- [ ] `Map` / `List` parameters and returns accept `()` (the documented "absent record" leniency), not only a trailing optional Map; `-> List<Int>` returning `["a"]` is caught at run time, not by check; a GET route calling think() spends tokens from any web page (CORS *).

### Cycle 48 — realistic port (online exams + proctoring) + attack on "work ∝ an attacker's number"

Exam port 8/10: 50 answers fired around deadline+grace split cleanly at the
cut-off; 30 parallel idempotent posts → one fresh, 29 replays; 12 temporal
properties proven. The attack confirmed every size-taking builtin capped,
deep JSON and recursion refused cleanly — except arithmetic.

### Fixed
- [x] **An Int's size had no cap**: `x = x * x` in a verified-terminating loop over a client number grew to 640M bits (3.4 GB) from a 22-byte request and held the handler lock for seconds — a product, `shl` and `product()` past 2^24 bits are refused (kind `range`) before they are built, in the interpreter and in `[native]` code (square_mut / in-place products checked).
- [x] `matmul` did n³ work from an n that passed the per-dimension check (minutes under the lock) — at most 10^9 multiply-adds.
- [x] Port: `substring` with a Float index / an Int subject returned `()`; `range(0, 3.7)` truncated silently — kind `type`; date builtins taking an Int (Unix seconds) documented.

### Open
- [ ] `top` / `slice` still truncate a Float count; `contains("abc", 5)` is false while `index_of` raises; a WebSocket client has no identity for per-session auth; no String-returning CSV builtin.

### Cycle 49 — realistic port (loyalty points, two peered instances) + amplification hunt, continued

Loyalty port 8/10: 60 concurrent redemptions on a stock of 30 → exactly 30;
40 against a balance fitting 2 → exactly 2; idempotent replays debited once;
velocity fraud limits exact under concurrency. The attack confirmed parsing,
regex, formatting, deep JSON, nested source and to_string bounded.

### Fixed
- [x] **`pow_mod` with a large exponent AND a large modulus did unbounded work** — a 5-byte body froze the whole service 8 s (at the size cap, never ending) — at most bits(exp) × bits(m) = 2^30.
- [x] `to_int` / `parse_int` of a long digit string built Ints past the 2^24-bit cap (66M bits) — refused, kind `range`.
- [x] Port: a bus event emitted while the peer was down vanished with no trace on the sender (points debited here, never credited there) — `bus: event '…' NOT delivered` is logged (no peer connected, or a peer that dropped); the docs give the outbox + ack pattern and say the bus port is unauthenticated; `sum_by`'s error claimed a value "was skipped silently" while raising.

### Open
- [ ] `sum_by` accepts padded / exponent numeric text ("1e3" turns an Int ledger into Floats); `Bronze == Red` (two sum types) is false while `5 == ""` raises; no way to pass headers to `soma run … request`.

### Cycle 50 — realistic port (job board / ATS) + attack from an agent following the docs literally

ATS port 8/10: 35 concurrent offers for one candidate → exactly one; an
anonymised reviewer view enforced by a one-variant type; 14 temporal
properties proven.

### Fixed
- [x] **The documented secret recipe failed open**: `read_file` of a missing secret returns `{error}`, and `"Bearer {t}"` turned that into guessable text that authenticated — the recipe now requires `type_of(t) == "String"` (fail closed) and secure_eq's hint says so instead of pointing to interpolation.
- [x] **`read_file` / `write_file` on client input escaped their directory** (a `..` in a decoded path argument read the secret; a write replaced the program's own `.cell`) — `..` segments are refused, and writes to `.cell` / `soma.toml` / `soma.lock` / `.soma_data` (kind `path`).
- [x] **A loopback server took cross-site writes** (a form POST from any page, DNS rebinding via Host) — on loopback, a foreign `Host` and a foreign `Origin` on POST/PUT/PATCH/DELETE are 403.
- [x] A handler calling its own server over HTTP froze every client until the timeout (the lock is held) — `kind: "self_call"` at once.
- [x] A tool capability refused a query with spaces (`?q=quantum computing`) that the same URL sent outside a tool — query spaces are matched encoded.
- [x] Port: get_status / has_state / valid_transitions from a machine-less cell with several machines passed check; a variant literal with a missing / extra / literally mistyped field passed check — both check errors. Docs: security notes (SSRF, http result shapes, template escaping, open redirects, CSV formulas).

### Open
- [ ] `soma fix --native-idiv` rewrites Float divisions; `render` does not escape by default; `html()` loads htmx from unpkg without SRI; `.field` on a sum-type value passes check; request logs include query strings (personal data).

### Cycle 51 — realistic port (delivery marketplace, 3 roles) + attack on last cycle's HTTP/file defenses

Marketplace port 8/10: 20 parallel orders on a 5-use promo → exactly 5;
12 parallel courier accepts → one; per-order SSE streams isolated; an
adversarial fake model could not reach private handlers or other users'
orders through tools.

### Fixed
- [x] **The cycle-50 write guard was case-sensitive on case-insensitive file systems**: `APP.CELL` overwrote the running program and `.SOMA_DATA/soma.db` the live database — compared case-insensitively.
- [x] **`self_call` missed other spellings of this machine** (`2130706433`, `0x7f000001`, `0177.0.0.1`, `[0:0:…:1]`): the handler dialled itself and froze the server 30 s — the host is resolved as the request would resolve it.
- [x] An opaque `Origin: null` (sandboxed iframe, data: page) or `file://` passed the loopback cross-site write check — refused.
- [x] Port: a provider reporting 20 tokens for a 15 000-character reply passed max_tokens and the proven cost bound — the reply is measured too (~4 characters per token) and the difference is charged.

### Open
- [ ] Absolute paths are not confined (only `..` segments and the program's own files are refused); an SSE subscription is authorized once, at subscribe time; no idiom for binding a tool to the calling user.

### Cycle 52 — realistic port (clinic booking) + attack (verifier soundness, native divergence, serving, file builtins)

Clinic port 7.5/10: 30 parallel bookings of one slot → 1 booked, 29 × 409;
waitlist promotion and rollback exact. Attack: no unsound ✓ in invariants,
termination, cost or temporal properties; no interpreter/native value
divergence in ~60 kernels; framing and static confinement held; no crash
on deep or huge inputs.

### Fixed
- [x] **Attack: `load` / `include` / `load_template` / `read_files` / `par_read_files` / `word_count` did not refuse `..`** — `load("templates/" + query.tpl)` served any file over HTTP; every path-taking builtin now refuses it (kind `path`).
- [x] **Port: an `emit` with no `[peers]` opened the bus port**, and any local process ran the program's own listeners with forged data (`EVENT freed "x"`) — the port opens only for `[peers]`, `[bus] accept`, `scale` or `--join`; self-emitted events come in only over a declared peer network.
- [x] Port: `SOMA_LLM_MOCK=fixed:` cut a reply over max_tokens short while `mock think` raised — it raises kind `llm` too (`echo` stays cut like a provider).
- [x] Port: a guard local bound AFTER an early `transition(id, "c") return` to another state was refused — only a transition that may take the guarded edge ends the scan.
- [x] Port: the `len(slot)` lint advised `len(value)`, which bounds EVERY slot — it now says `len(slot)` already means the written value's length.
- [x] Port: no status for "not authenticated" — kinds `unauthorized` / `unauthenticated` answer 401.
- [x] Port: an error body named private handlers (`_book(): parameter …`) — hidden from the client, kept in the log.
- [x] Attack: the loopback Host check read the text before the first ':' (`localhost:9540.evil.com`, `localhost:9540, evil.com` passed); the whole authority is parsed, two Host headers are refused.

### Open
- [ ] Absolute paths are still not confined; the prover cannot bound strings (`"bk{n}" != ""`); route patterns take one variable; no time zones.

### Cycle 53 — realistic port (multi-warehouse inventory, two processes over the bus) + attack (bus, test/serve divergence, verifier, exposure)

Inventory port 8/10: 30 parallel reservations of the last unit → exactly
one; a duplicate webhook delivered 5× concurrently applied once; atomic CSV
import. Attack: every bus filter (private/lifecycle names, `_type`,
unlisted events, malformed lines) held; no unsound ✓ in guards, `except`,
tool cost bounds or List-slot invariants; no crash.

### Fixed
- [x] **Attack: parse amplification** — a valid 15 MB bus line or JSON body of `[0,0,…]` became ~2.4 GB of parsed values before any check; more than 1 000 000 JSON values is refused before parsing (413 / bus connection closed).
- [x] **Port: events emitted after a linked peer went down vanished silently** (the write "succeeded" into the kernel buffer) — the next event is logged NOT delivered and the dead link is dropped.
- [x] Port: `soma run` of a handler emitting to `[peers]` committed and sent nothing, silently — a warning names the undelivered events.
- [x] Port: `b.size ?? "M"` read the entry count (never `()`) — a check warning points to `b.get("size")`.
- [x] Port: a negative `[native]` buffer index was shown as 18446744073709551615 — shown as -1.

### Open
- [ ] A missing field interpolates as `null`; no CSV-from-string parser; a restarted one-way peer is not re-linked; `write_csv(path, [])` writes no header; serve prints "listening" before the bus port binds.

### Cycle 54 — realistic port (agent-driven support desk with tools, approvals, budgets) + attack (the LLM as the attacker)

Support port 7/10: tool scoping (`tools_allowed`, unknown tools, typed
arguments) refused every scripted misbehaviour; 20 concurrent approvals of
one refund → one paid. Attack with a fake OpenAI-compatible provider: tool
scope, argument forgery, `max_rounds`, SSE/log injection all held.

### Fixed
- [x] **Port: a write-once invariant on a `Map<String, Map>` / `List<Map>` slot did nothing** — `docs.get(key)` read the field `key` of the NEW value (the slot's name is bound to it); in an invariant `slot.get(k)` / `slot[k]` / `slot.has(k)` now read the stored slot.
- [x] **Attack: approve() printed model-written text raw** — `\r\x1b[2K` erased "Refund 5000" and drew "Refund 5"; control and bidi characters are shown as escapes.
- [x] **Attack: replies in other shapes skipped max_tokens and the budget** — OpenAI content-part lists and extra Anthropic text blocks were not measured (400 000 characters against a proven 100); an empty/null content returned the provider's raw JSON as the answer. All text is measured; a reply with neither text nor a tool call raises kind `llm`; tool-call ids are measured.
- [x] **Attack: a delegated agent's `set_budget` reset the caller's** — reached from a tool call it only lowers what is left (300 stayed 300).
- [x] Attack: `..;/` passed a capability's path scope — refused like `..`.
- [x] Port: verify said a List `delete` cannot break a value invariant while the runtime refused it — a List delete, like a Map delete, is checked by size invariants only.
- [x] Port: `require … else budget` (a program's tag named like a runtime kind) answered 500 — 400.

### Open
- [ ] No append-only slot declaration; `mock think` cannot script tool calls; tools get no hidden caller context; a think() holds the handler lock; parallel tool calls can split per-call approval thresholds.

### Cycle 55 — realistic port (double-entry bookkeeping, exact money) + attack (soundness of every static guarantee, row by row)

Bookkeeping port 7.5/10: 300 parallel postings of 0.01 moved the balance by
exactly 300 cents; native IRR / amortization identical to the interpreter.
Attack held on state machines, interval invariants, other termination
routes, token cost bounds, forall and rollback — and found five breaks.

### Fixed
- [x] **Attack: `"C".delegate("h", x)` and `"C" |> delegate("h", x)` were invisible to the call graph** — a self-delegating handler "structurally terminated" (then overflowed the stack) and a size proof missed the delegated write; both forms are parsed as `delegate("C", "h", x)`.
- [x] **Attack: the `latency` bound was printed proven but did not hold** — LLM retries with back-off took 6.7 s against a proven 1 s; `sleep`, `approve`, file reads were not counted. `timeout` now bounds the whole think() call (retries included, no 1 s floor); latency is × tool rounds; literal sleeps are added; approve / file I/O / computed sleeps make it advisory. It counts waiting, not CPU (documented).
- [x] Attack: native `/` with operands past 2^53 and an exact quotient was rounded (9007199254740993 / 3 → …330.5) — the exact quotient is returned.
- [x] Attack: a native handler re-run in BigInt mode read stdin again (answered 0) — the fast run's reads are replayed.
- [x] Attack: `let b = …` hiding slot `b` lent the slot's bound to `b.get(…)` in an induction proof — a top-level let hides the slot.
- [x] Attack (minor): an interpolated transition target `"{t}"` was refused as the literal state — treated as computed (in refinement and guard binding).
- [x] Port: `x |> map(f)` / `filter` / `find` / `any` / `all` / `count` / `sort_by` on a non-list built a Map or said "undefined function" — a type error at the call.
- [x] Port: no exact Int power — `ipow(a, b)`; `to_int(pow(…))` is a check warning; `2 ** 3` hints at pow/ipow; `pow` of a non-number raises (was 0.0).
- [x] Port: `read_csv` ignored unknown options — refused; `delimiter` supported; `from_csv(text)` parses CSV in memory.
- [x] Port: `format("%.2f", BigInt)` went through a Float — exact; `to_float` of an Int past the Float range raises `range` (was inf).

### Open
- [ ] Float/String `==` raises a type error that reads like a parameter error; a GET on a handler calling a pure helper of another cell answers 405; `value.field` invariants apply to every slot without warning; native `hm_inc` overflow raises kind type; Int/Float comparison is lossy past 2^53.

### Cycle 56 — realistic port (turn-based game server: lobby, matchmaking, WS/SSE, timers, ELO, native minimax) + attack (WS/SSE framing and auth, new builtins, scheduler, soundness)

Game port 7.5/10: two players moving at the same instant were serialized
(one wins, the other gets 403/400); forfeits by the tick pushed over SSE;
state and ELO survived restarts; native negamax searched the full tree in
~20 ms. Attack: WS masking/UTF-8/RSV/length/fragmentation and Origin checks,
SSE authorization, cost via delegate and emit, scheduler atomicity and
single-runner all held; no crash.

### Fixed
- [x] Attack: `"hello" |> map(f)` / `"hello".map(f)` still built `{"hello": <lambda>}` (the Map constructor) — a type error like every other non-list.
- [x] Port: `response(429, body)` returned from `on ws` was sent as `{"_status", "_body"}` — the socket gets the body.
- [x] Port: WS error bodies named private handlers (HTTP hides them) — hidden too.
- [x] Port: a binary WS frame was dropped silently — logged and answered with a `type` error.
- [x] Port: no status for throttling — kinds `rate_limited` / `too_many_requests` answer 429.
- [x] Port: `d.cell` (a reserved word as a JSON field) — the error shows `d["cell"]`.
- [x] Port: the guard-binding error said a handler "takes that transition" when its target only MAY take the guarded edge — the message explains that the edge is chosen at run time and offers a slot keyed by `_id`.

### Open
- [ ] No per-connection WS identity or open/close hook; a counter cannot commit while the request raises (rate limiting of failing requests); native index errors carry Rust text and no line; termination warnings do not name the caller.

### Cycle 57 — realistic port (pandas-style ETL / analytics on 200k rows) + attack (differential fuzzing: native vs interpreted, run vs test, verifier vs runtime, parser)

ETL port 6/10: every aggregate matched pandas (per-day sums, medians,
p95/p99, OLS, IQR fences); native kernel 12 ms vs 332 ms interpreted.
Fuzzing: 13k native/interpreted comparisons, 2 600 run-vs-test
expressions (0 divergences), 1 135 fully proven writers fuzzed at run time
(0 unsound ✓), 5 700 mutated programs through check/verify/describe (0
panics).

### Fixed
- [x] **Port: a pure lambda cloned every captured list/map on EACH call** — `range(0, n) |> map(i => xs[i])` was quadratic (2.8 s at 16 000; a 200k zip never finished); the environment is built once per map/filter/…: 5 ms at 16 000, 51 ms at 200 000.
- [x] **Attack: native `/` truncated after the BigInt re-run** — the docs' own Gotcha 19 midpoint `m = (lo + hi) / 2` returned a truncated value on overflow; the in-place assign path and index expressions now use the exact quotient (or raise).
- [x] Port: `sort_by` over a column holding NaN came back unsorted — NaN sorts after every number.
- [x] Port: an unclosed CSV quote swallowed the rest of the file silently — kind `csv` error naming the record.
- [x] Port: `quantile` clamped q outside [0, 1] — kind `range`; `quantile([])` is kind `empty` like median.
- [x] Port: blank CSV cells made `sum_by` / `avg_by` raise while `agg` skipped them — a blank cell is a missing value; the error quotes a bad String value.
- [x] Attack: `soma check` passed a native handler returning its String parameter, then rustc failed — it compiles.
- [x] Attack: an Int overflow inside a native Float/Bool expression raised kind `type` with Rust panic text (and i64::MIN % -1 read as "modulo by zero") — kind `range` with the `let` workaround.
- [x] Attack: a chain of 50 000 `|>` escaped the nesting limit and made check run for minutes — operator chains count toward the 400-level limit.

### Open
- [ ] Appending to a list inside a map (`b[k] = push(b[k] ?? [], i)`) is quadratic; memory ~8× pandas; no datetime/offset parsing; native `if` expressions and `round(x) + Int` refused; native Int overflow inside Float/Bool expressions raises instead of promoting; `soma fmt` does not exist.
