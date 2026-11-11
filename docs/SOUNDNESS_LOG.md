# Soundness log

Every known case where `soma verify` or `soma check` reported something as
**proven** that was false. A prover's worth is the list of times it was
wrong and what was done about it; this one is public and append-only.

An entry is added whenever a false proof is found — by a user, an audit
pass, the fuzzer or a contributor — in the release that fixes it. A fix
that turns a "proven" into "runtime-checked" counts as fixed: the claim is
gone. A bug that makes the *runtime* accept what it should refuse is a
correctness or security bug and goes to the changelog, not here.

Report one: see [SECURITY.md](../SECURITY.md). A false proof is treated as
a security issue.

| # | Found | What was "proven" and false | Now |
|---|---|---|---|
| 1 | 2026-09, agent-UX cycle 1 | `[verify.after.X] never = ["Y"]` was encoded as "after X, eventually some state ≠ Y" — true the instant the machine is in X. It proved `after executed never cancelled` for a machine where executed → failed → pending → cancelled exists. | Y must be unreachable from X; a failure prints the path. |
| 2 | cycle 13 | A Float invariant after an early-return narrowing (`if x > 10.0 { return }`): NaN passes the negated comparison. | Negated comparisons narrow only NaN-free values. |
| 3 | cycle 13 | Invariant, size and termination proofs when a match binding, a lambda parameter or a nested `let` shadowed the slot or local the proof was about; growth through another cell or an emit listener; a cost bound that ignored emit listeners. | Shadowing, cross-cell callers and listeners are part of every analysis. |
| 4 | cycle 14 | A `require` placed AFTER a write (or after an early return) was credited to that write. Reassignment inside a match arm, `try` or if-expression was invisible. Writes in `every` / `after` blocks were not writers. | An invariant is checked at the write: only later writes are narrowed; all three constructs and both block kinds are analysed. |
| 5 | cycle 14 | Termination through `x \|> f()`, `Cell.f()`, a base case that recurses, `while true` in an `every` block. | All four are seen; an unprovable handler is a ⚠, never "terminates". |
| 6 | cycle 15 | Calls inside a string interpolation, UFCS, pipes and literal `delegate(…)` were invisible to the invariant, termination and cost analyses. | The analyses run on the desugared program, where each is an explicit call. |
| 7 | cycle 16 | A size proof accepted a key whose value changed between the `require` and the `set`; recursion placed before its base case; `delegate()` to a computed name. | Keys must be pure texts over unchanged names; the other two are refused. |
| 8 | cycle 19 | `require` conditions and details were invisible to every analysis: calls in them gave "structurally terminate", "bound proven — peak 0" and "no handler writes to guarded slots". | They are statements like any other for termination, cost and the invariant prover. |
| 9 | cycle 19 | A size proof after a delete (`require m.get(k) != ()  m.delete(k)  …  m.set(k, 2)`); a Float bound "proven" for a computed value that may be NaN. | A delete drops the key-exists fact; a computed Float is runtime-checked unless it is a constant or a slot read ± a literal. |
| 10 | later pass | A handler parameter named like a slot: `require len(m) < 2` read the parameter while `m.set` wrote the slot — a false size proof. | A parameter may not take a slot's name (check error). |

Source: `docs/agent-ux/LEDGER.md`, where each entry has its reproduction
and the regression test that pins it. Entries before this file existed are
summarised from the ledger; from 3.0.0 on they are written here first.

## 3.0.0

No false proof was found in the 3.0.0 work. Two analyses added in 3.0.0
were wrong *before release* and are recorded because the reasoning error is
instructive:

- **Transition-source inference** first refused `if a > n { transition(id,
  "passed") }  if a <= n { transition(id, "failed") }` as a move from
  `passed` to `failed`: it joins paths without their data conditions, so
  "voting or passed" reached the second call. A call is now refused only
  when NO known source has an edge to the target — the case that raises on
  every path. Found by diffing `verify` on the 1,411 programs of the
  repository against the previous binary (2 corpus programs changed
  verdict); pinned by `b3_exclusive_branches_are_not_refused`.
- **Static types** first refused `for x in m.get(k)` and `opt == 3` on a
  `T?`: `()` is legal in both places. A `T?` is refused only where `()`
  itself would raise.
