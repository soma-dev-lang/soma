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
- [ ] E9 `soma run app.cell validate '{"id":1}'` passes a String into a `Map` parameter; error surfaces deep inside the handler.
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
- [ ] R2 `require c else "msg {x}"` / `else variable` silently taken literally; `require … else Tag` absent from docs.
- [ ] R3 `try { f() }?` returns the error map as a normal value (not a propagate).

### Still reaching runtime past a green check
- [ ] C1 call with the wrong argument count; face return type vs returned value.
- [x] C2 `() >= 500` was silently false (typo'd field) → ordering against () raises.
- [x] C3 variant `==` false when a payload is a Map/List.
- [ ] C4 field typo on an untyped record (`order.statuss` → ()). Needs typed records.
- [ ] C5 record read from a slot, mutated, never written back: silent lost update. Wants a lint.
- [ ] C6 `invariant accts.balance >= 0` passes check, rejects every write (`value.balance` works).
- [ ] C7 `from_json("garbage")` returns the string; `to_int("1.5")` = 1; no strict parse.
- [x] C8 `m[k] += 1`, `acc.balance += x` unsupported.

### Verify
- [x] V1 `* -> failed` made success states non-terminal (2 agents). First fix (wildcard skips final states) broke `* -> deleted` in 4 programs and was REVERTED; now: verify warns with the exact fix, and `* -> failed except [paid, denied]` keeps states final.
- [x] V2 invariants on computed values: interval + induction prover; per-conjunct report; always-rejected-and-caught writes reported as such.
- [x] V3 `requires = [a, b]` is "one of" (passes silently when you meant both) → `requires_all`.
- [x] V4 summary says "0 failures" before the temporal section fails; streams interleave; "think-isolated" printed for cells with no LLM; "N literal transitions" counts call sites.
- [ ] V5 `get_status(unknown id)` returns the initial state.

### Tests / mocks
- [ ] T1 `mock approve false`; scripted tool calls; mixed queue with an error in the middle; mock clock; per-assert isolation.
- [ ] T2 `soma check --json` lists the proven cost bound under "warnings".

### Small language gaps
- [ ] G1 `parse_int`, `pad_left`, negative index `xs[-1]`, `avg([1,2])` truncates, default parameters, unknown function alias table (`parseInt` → to_int) with the span on the call, parser reports one error per run.

### Site (evaluator)
- [ ] W1 add /docs/guarantees.md (PROVEN | ENFORCED AT RUNTIME | NOT COVERED) and align the /agents headline ("PROVES … memory invariants" is broader than the truth).
- [ ] W2 /docs/serving.md: routing table, exposure rules, bind address/ports, threading, response shape.
- [ ] W3 corpus: /corpus/domains.json (~1 KB), finer features (guard, precedence, router, two_cell), drop the `tests` feature (all 316 have it), programs combining http + state_machine + invariant, each program's soma.toml + verify line in the index; audit narratives against behaviour.
- [ ] W4 /status: license (SPDX), changelog, release date, known limits, security contact, security.txt; LICENSE on the site; pin install.sh to a tag + checksum.
- [ ] W5 llms-full.txt repeats llms.txt (10 KB paid twice) and carries ~20 KB of linalg/quant irrelevant to a service → profiles; spec.md says "Version: 2.2.1"; /examples is a dead end; repo-relative links dangle; docs hash in version.json.

### Cycle 2 — decisions taken
- **Handlers are atomic.** Undo journal in the interpreter (writes, deletes, appends, transitions); a failing top-level handler is rolled back; a failing `try` is a savepoint; top-level invocations are serialized process-wide. The evaluator's experiment now pays exactly 100/300 (was 122–153), pinned by a test.
- **Error model.** `fail(kind, detail)`, `r.kind` / `r.detail`, `fail(r)`; "require failed:" only on requires; no Rust Debug dumps in test output.
- **Routing.** A path `request` matches explicitly wins over the auto-exposed handler of the same name; check warns (19 true collisions in the repo, 4 in rebalancer).
- **Gates.** verify and test refuse a program that fails check; properties on undeclared states are errors; one final VERIFY verdict; `requires_all`.
- **Prover.** Interval + induction reasoning on invariants, reported per conjunct; an always-rejected write under `try` is reported as "can never commit" (2 governance programs rely on it).
- Ordering against `()` raises (was silently false); variant payloads compare structurally; `m[k] += 1` / `a.f += x`.
