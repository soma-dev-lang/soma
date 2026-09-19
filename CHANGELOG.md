# Changelog

## Unreleased

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
