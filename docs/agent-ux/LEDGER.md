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
- [ ] S3 `invariant len(links) <= 1000` silently bounds the written VALUE's length, not the entry count; no per-slot size bound syntax.
- [ ] S4 cost prover says "no max_tokens" for `think(prompt, system, map("max_tokens", N))` — only reads opts in arg 2.
- [ ] S5 `assert_fails` passes for the wrong reason (UndefinedVar); wants `assert_fails expr matching "text"`.
- [ ] S6 `promise cost <= 1000` in face silently accepted, no effect.
- [ ] S7 `SOMA_LLM_MOCK=bogus` silently accepted.
- [x] S8 undefined variable (typo) passed check, failed only at runtime (found by me before the reports).

### Diagnostics
- [x] D1 `soma test` hid the asserted expression (`assert ... == 400`), no line numbers, left/right printed before the ✗ line; a null from a wrong field name unexplained.
- [x] D2 `soma check` diagnostics had no file:line:col (text and JSON).
- [x] D3 verify noise: "has no guard" on every transition (pushes toward S2); "states [X] cannot reach terminal 'rejected'" for intended designs.
- [ ] D4 verify: two tallies in odd order; "18 properties loaded" vs "15 passed"; after+never worded "all paths eventually satisfy".
- [ ] D5 `.url` on a try-result is `()` silently (should be `.value.url`): static check.

### Docs
- [ ] O1 response shape (`_status`, `_body`) documented nowhere.
- [ ] O2 `cost { tokens: N }` section documented nowhere; no note when think() is used without one.
- [ ] O3 verify property vocabulary (eventually/never/always/after) undocumented → `soma docs verify`.
- [ ] O4 `require X else name` appears in corpus, not in docs.
- [ ] O5 the advertised `soma example invariant http` returns nothing; no web example uses an invariant.
- [ ] O6 `soma example <id>` does not print the companion soma.toml an example depends on.

### Language / features wanted
- [ ] L1 scriptable LLM mock per test (`mock think "abuse"`, queue, error injection); `soma test` should mock by default when no key.
- [ ] L2 `* -> failed` also applies to terminal states (closed is no longer terminal). No "non-terminal only" wildcard.
- [ ] L3 invariants like `counts >= 0` with write `(counts.get(k) ?? 0) + 1` are only runtime-checked: inductive proof would make "proven" mean more.
- [ ] L4 properties inline in the state block instead of soma.toml; a precedence property ("paid requires manager_approved").
- [ ] L5 `get_status(id)` returns the initial state for an unknown id.
- [ ] L6 `always` property is useless as defined (single state name).

### From the data/algorithms agent (the only one who would NOT pick Soma again)
- [x] E1 `==` on lists/maps is not structural — `[] == []` is false; the test runner prints identical left/right and says FAILED; the documented `to_string` workaround breaks on map key order.
- [x] E2 `rows.push(x)` as a statement silently does nothing (push returns a new list) while `xs[i] = v` mutates.
- [x] E3 unknown method (`xs.includes(x)`) only fails at runtime, with a Rust Debug dump and no did-you-mean.
- [~] E4 `[a] + list` is element-wise with silent broadcasting: `merge_sorted` returned `[29.0]`. Length mismatch raises nothing.
- [ ] E5 parser stops at the first error and gives no hint for foreign idioms: `{k: v}`, `@native`, `xs[a:b]`, `(a, b) =>`, `for (k, v) in`, `null`/`None`.
- [x] E6 missing everyday builtins: `keys(m)`/`values(m)`/`entries(m)` as functions, `contains(list, x)`, `slice`, comparator/key-function sort, `round(x, digits)`, `deep_eq`.
- [ ] E7 no `let` inside test `rules` — every assertion re-calls the fixture handler.
- [ ] E8 `soma test` does not compile `[native]` handlers: a native-vs-interpreted assertion compares the interpreter with itself.
- [ ] E9 `soma run app.cell validate '{"id":1}'` passes a String into a `Map` parameter; error surfaces deep inside the handler.
- [ ] E10 `[native]` placement (`on f(n: Int) [native] {`) and test-cell anatomy (`cell test Name { rules { } }`) not in `docs agent`.

### Cycle 1 — decisions taken
- **Dispatch rule changed**: `f(args)` resolves to the program's handler `f` when one takes that many arguments, the builtin otherwise (was: builtins always win). Reason: the author of this ledger fell into the old rule four times in one night; user definitions shadow the library in every language agents know; and under "builtins win" each new builtin silently hijacked homonymous handlers. Impact measured on 986 programs: 2 broke (`on list() { let items = list() }` → now `[]`), fixed.
- `==` is structural on lists / maps / variants (was a runtime error).
- Guards now see the calling handler's locals; `soma check` verifies their scope. Two examples declared guards they could never exercise — fixed.
- verify no longer nags "has no guard" / "cannot reach terminal X".
