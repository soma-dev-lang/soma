# Changelog

## Unreleased

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
